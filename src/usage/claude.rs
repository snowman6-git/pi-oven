//! Claude (Pro/Max) plan limits: `GET https://api.anthropic.com/api/oauth/usage`, the address Claude Code's `/usage`
//! uses (not a public API). Everything that knows about Claude lives here; `mod.rs` decides when to ask.
//!
//! The sign-in is pi's (`pi auth print-bearer-token --provider anthropic`); the token goes to api.anthropic.com only,
//! on curl's standard input, and is not kept.

use super::Asked;
use crate::limits::{Entry, Window};

pub const PROVIDER: &str = "anthropic";
const USAGE: &str = "https://api.anthropic.com/api/oauth/usage";

/// "2026-11-16T17:40:00.421989+00:00" → milliseconds since 1970 (UTC). Only this RFC 3339 shape is read.
pub fn parse_time(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    let mut i = 19;
    let mut ms = 0i64;
    if b.get(i) == Some(&b'.') {
        let start = i + 1;
        i = start;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        let frac = &s[start..i.min(start + 3)];
        ms = format!("{frac:0<3}").parse().ok()?;
    }
    let offset = match b.get(i) {
        Some(b'Z') | None => 0,
        Some(b'+' | b'-') => {
            let sign = if b[i] == b'-' { -1 } else { 1 };
            sign * (n(i + 1..i + 3)? * 60 + n(i + 4..i + 6)?)
        }
        _ => return None,
    };
    // days from the civil date (Howard Hinnant's algorithm)
    let (y2, m2) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + h * 3600 + mi * 60 + se - offset * 60;
    u64::try_from(secs * 1000 + ms).ok()
}

/// The answer → an entry for the limits store: the 5-hour and 7-day windows.
pub fn entry_from(v: &serde_json::Value) -> Option<Entry> {
    let window = |w: &serde_json::Value, minutes: u32| -> Option<Window> {
        let used = w["utilization"].as_f64()?;
        Some(Window { minutes, used, reset_at: w["resets_at"].as_str().and_then(parse_time) })
    };
    let windows: Vec<Window> = [window(&v["five_hour"], 300), window(&v["seven_day"], 10080)].into_iter().flatten().collect();
    if windows.is_empty() {
        return None;
    }
    let reached = windows.iter().any(|w| w.used >= 100.0);
    Some(Entry { provider: PROVIDER.into(), plan: None, at: crate::runtime::now_ms(), limit_reached: reached, windows, resets: None, reset_kinds: vec![] })
}

pub async fn ask(pi_bin: &str) -> Asked {
    let token = match super::token(pi_bin, PROVIDER).await {
        Ok(Some(t)) => t,
        Ok(None) => return Asked::NoSignIn,
        Err(e) => return Asked::Failed(e),
    };
    let headers = format!("Authorization: Bearer {token}\nanthropic-beta: oauth-2025-04-20\n");
    match super::get(USAGE, &headers).await {
        Ok((200, body)) => match serde_json::from_str::<serde_json::Value>(&body).ok().and_then(|v| entry_from(&v)) {
            Some(e) => Asked::Got(e),
            None => Asked::Failed("usage: no limits in the answer".into()),
        },
        Ok((429, _)) => Asked::TooMany,
        Ok((c, _)) => Asked::Failed(format!("usage: HTTP {c}")),
        Err(e) => Asked::Failed(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_rfc3339_times() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2026-11-16T17:40:00.421989+00:00"), Some(1_794_850_800_421));
        assert_eq!(parse_time("2026-11-16T18:40:00+01:00"), Some(1_794_850_800_000));
        assert_eq!(parse_time("garbage"), None);
    }

    #[test]
    fn turns_the_answer_into_windows() {
        let v = serde_json::json!({
            "five_hour": { "utilization": 17.0, "resets_at": "2026-11-16T17:40:00.421989+00:00" },
            "seven_day": { "utilization": 3.0, "resets_at": "2026-11-23T12:00:00.422015+00:00" },
            "seven_day_opus": null
        });
        let e = entry_from(&v).unwrap();
        assert_eq!(e.provider, "anthropic");
        assert_eq!(e.windows.iter().map(|w| (w.minutes, w.used)).collect::<Vec<_>>(), [(300, 17.0), (10080, 3.0)]);
        assert_eq!(e.windows[0].reset_at, Some(1_794_850_800_421));
        assert!(!e.limit_reached);
        assert!(entry_from(&serde_json::json!({})).is_none());
    }
}
