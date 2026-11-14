//! ChatGPT (Codex) plan limits: `GET https://chatgpt.com/backend-api/wham/usage`, the address Codex CLI's `/status`
//! uses (not a public API). Everything that knows about ChatGPT lives here; `mod.rs` decides when to ask.
//!
//! The sign-in is pi's: `pi auth print-bearer-token --provider openai-codex` hands out a fresh token (refreshing it
//! when needed), and the account id comes from inside that token, so `auth.json` is never read. The token goes to
//! chatgpt.com only, on curl's standard input (never on a command line, where `ps` would show it), and is not kept.

use std::process::Stdio;
use tokio::{io::AsyncWriteExt, process::Command};

use super::Asked;
use crate::limits::{Entry, Window};

pub const PROVIDER: &str = "openai-codex";
const USAGE: &str = "https://chatgpt.com/backend-api/wham/usage";

/// The ChatGPT account id inside the access token (a JWT): `https://api.openai.com/auth` → `chatgpt_account_id`.
pub fn account_id(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = b64url(payload)?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v["https://api.openai.com/auth"]["chatgpt_account_id"].as_str().map(str::to_owned)
}

fn b64url(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &c in s.as_bytes().iter().filter(|&&c| c != b'=') {
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// The answer → an entry for the limits store.
pub fn entry_from(v: &serde_json::Value, now: u64) -> Option<Entry> {
    let rl = &v["rate_limit"];
    let window = |w: &serde_json::Value| -> Option<Window> {
        let used = w["used_percent"].as_f64()?;
        let minutes = (w["limit_window_seconds"].as_u64()? / 60) as u32;
        let reset_at = w["reset_at"].as_u64().map(|s| s * 1000).or_else(|| w["reset_after_seconds"].as_u64().map(|s| now + s * 1000));
        Some(Window { minutes, used, reset_at })
    };
    let windows: Vec<Window> = [&rl["primary_window"], &rl["secondary_window"]].into_iter().filter_map(window).collect();
    if windows.is_empty() {
        return None;
    }
    Some(Entry {
        provider: PROVIDER.into(),
        plan: v["plan_type"].as_str().map(str::to_owned),
        at: now,
        limit_reached: rl["limit_reached"].as_bool().unwrap_or(false) || rl["allowed"].as_bool() == Some(false),
        windows,
        resets: v["rate_limit_reset_credits"]["available_count"].as_u64().map(|n| n as u32),
    })
}

/// Ask ChatGPT once. `pi_bin` gives the sign-in; `now` stamps the answer.
pub async fn ask(pi_bin: &str, now: u64) -> Asked {
    let tok = match Command::new(pi_bin)
        .args(["auth", "print-bearer-token", "--provider", PROVIDER])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
    {
        Ok(o) if o.status.success() => o.stdout,
        // not signed in to ChatGPT: nothing to ask, and nothing wrong
        Ok(_) => return Asked::NoSignIn,
        Err(e) => return Asked::Failed(format!("pi auth: {e}")),
    };
    let token = String::from_utf8_lossy(&tok).trim().to_owned();
    let Some(account) = account_id(&token) else { return Asked::Failed("no account id in the token".into()) };
    if token.contains(['\r', '\n']) || account.contains(['\r', '\n']) {
        return Asked::Failed("unexpected token".into());
    }

    // -w adds the status on its own last line, so a 429 can be told from other failures
    let child = Command::new("curl")
        .args(["-sS", "--proto", "=https", "--max-time", "20", "-H", "@-", "-H", "Accept: application/json", "-H", "User-Agent: pi-oven", "-w", "\n%{http_code}", USAGE])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return Asked::Failed(format!("curl: {e}")),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let headers = format!("Authorization: Bearer {token}\nchatgpt-account-id: {account}\n");
        if let Err(e) = stdin.write_all(headers.as_bytes()).await {
            return Asked::Failed(e.to_string());
        }
    }
    let out = match child.wait_with_output().await {
        Ok(o) => o,
        Err(e) => return Asked::Failed(e.to_string()),
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", text.as_ref()));
    match code.trim().parse::<u16>().unwrap_or(0) {
        200 => match serde_json::from_str::<serde_json::Value>(body).ok().and_then(|v| entry_from(&v, now)) {
            Some(e) => Asked::Got(e),
            None => Asked::Failed("usage: no limits in the answer".into()),
        },
        429 => Asked::TooMany,
        0 => Asked::Failed(format!("usage: {}", String::from_utf8_lossy(&out.stderr).trim())),
        c => Asked::Failed(format!("usage: HTTP {c}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_account_from_the_token() {
        // header.payload.signature, payload = {"https://api.openai.com/auth":{"chatgpt_account_id":"acc-1"}}
        let payload = "eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjLTEifX0";
        assert_eq!(account_id(&format!("x.{payload}.y")).as_deref(), Some("acc-1"));
        assert_eq!(account_id("not-a-jwt"), None);
    }

    #[test]
    fn turns_the_answer_into_windows() {
        let v = serde_json::json!({
            "plan_type": "plus",
            "rate_limit": {
                "allowed": false, "limit_reached": true,
                "primary_window": { "used_percent": 100, "limit_window_seconds": 18000, "reset_after_seconds": 10356, "reset_at": 1794656515 },
                "secondary_window": { "used_percent": 49, "limit_window_seconds": 604800, "reset_at": 1795181413 }
            },
            "rate_limit_reset_credits": { "available_count": 0 }
        });
        let e = entry_from(&v, 1).unwrap();
        assert_eq!(e.plan.as_deref(), Some("plus"));
        assert!(e.limit_reached);
        assert_eq!((e.windows[0].minutes, e.windows[0].used, e.windows[0].reset_at), (300, 100.0, Some(1794656515000)));
        assert_eq!((e.windows[1].minutes, e.windows[1].used), (10080, 49.0));
        assert_eq!(e.resets, Some(0));
        assert!(entry_from(&serde_json::json!({}), 1).is_none());
    }
}
