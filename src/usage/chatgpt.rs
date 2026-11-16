//! ChatGPT (Codex) plan limits: `GET https://chatgpt.com/backend-api/wham/usage`, the address Codex CLI's `/status`
//! uses (not a public API). Everything that knows about ChatGPT lives here; `mod.rs` decides when to ask.
//!
//! The sign-in is pi's: `pi auth print-bearer-token --provider openai-codex` hands out a fresh token (refreshing it
//! when needed), and the account id comes from inside that token, so `auth.json` is never read. The token goes to
//! chatgpt.com only, on curl's standard input (never on a command line, where `ps` would show it), and is not kept.

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
        reset_kinds: vec![],
    })
}

/// Ask ChatGPT once. `pi_bin` gives the sign-in; `now` stamps the answer.
pub async fn ask(pi_bin: &str, now: u64) -> Asked {
    let token = match super::token(pi_bin, PROVIDER).await {
        Ok(Some(t)) => t,
        Ok(None) => return Asked::NoSignIn,
        Err(e) => return Asked::Failed(e),
    };
    let Some(account) = account_id(&token) else { return Asked::Failed("no account id in the token".into()) };
    if account.contains(['\r', '\n']) {
        return Asked::Failed("unexpected account id".into());
    }
    let headers = format!("Authorization: Bearer {token}\nchatgpt-account-id: {account}\n");
    match super::get(USAGE, &headers).await {
        Ok((200, body)) => match serde_json::from_str::<serde_json::Value>(&body).ok().and_then(|v| entry_from(&v, now)) {
            Some(mut e) => {
                if e.resets.unwrap_or(0) > 0 {
                    e.reset_kinds = credit_kinds(&token, &account).await;
                }
                Asked::Got(e)
            }
            None => Asked::Failed("usage: no limits in the answer".into()),
        },
        Ok((429, _)) => Asked::TooMany,
        Ok((c, _)) => Asked::Failed(format!("usage: HTTP {c}")),
        Err(e) => Asked::Failed(e),
    }
}

/// The list of reset credits (asked only when there are some).
const CREDITS: &str = "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits";

/// What one credit resets. The list's fields are not documented (and an empty list shows none), so the likely ones
/// are read loosely: a model named → "model:<name>"; a week → "week"; five hours / a session → "5h"; else "other".
pub fn credit_kind(c: &serde_json::Value) -> String {
    let field = |k: &str| c[k].as_str().map(str::to_ascii_lowercase);
    if let Some(m) = ["model", "model_slug", "model_name"].iter().find_map(|k| c[*k].as_str()).filter(|m| !m.is_empty()) {
        return format!("model:{}", m.chars().take(60).collect::<String>());
    }
    let words: String = ["limit_type", "reset_type", "type", "window", "scope", "kind", "name"].iter().filter_map(|k| field(k)).collect::<Vec<_>>().join(" ");
    if words.contains("week") || words.contains("secondary") || words.contains("7d") {
        "week".into()
    } else if words.contains("5h") || words.contains("hour") || words.contains("primary") || words.contains("session") {
        "5h".into()
    } else {
        "other".into()
    }
}

pub async fn credit_kinds(token: &str, account: &str) -> Vec<String> {
    match super::get(CREDITS, &format!("Authorization: Bearer {token}\nchatgpt-account-id: {account}\n")).await {
        Ok((200, body)) => serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["credits"].as_array().cloned())
            .map(|a| a.iter().map(credit_kind).collect())
            .unwrap_or_default(),
        _ => vec![],
    }
}

/// ChatGPT's own list of Codex models (what Codex CLI reads): per model, the reasoning level it recommends.
const MODELS: &str = "https://chatgpt.com/backend-api/codex/models?client_version=0.200.0";

/// The answer → model slug → recommended reasoning level ("low", "medium" …), as ChatGPT names them.
pub fn recommended_from(v: &serde_json::Value) -> std::collections::BTreeMap<String, String> {
    v["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| Some((m["slug"].as_str()?.to_owned(), m["default_reasoning_level"].as_str()?.to_owned())))
        .collect()
}

pub async fn recommended(pi_bin: &str) -> Result<std::collections::BTreeMap<String, String>, String> {
    let token = super::token(pi_bin, PROVIDER).await?.ok_or("no ChatGPT sign-in")?;
    let account = account_id(&token).ok_or("no account id in the token")?;
    if account.contains(['\r', '\n']) {
        return Err("unexpected account id".into());
    }
    match super::get(MODELS, &format!("Authorization: Bearer {token}\nchatgpt-account-id: {account}\n")).await? {
        (200, body) => serde_json::from_str::<serde_json::Value>(&body).map(|v| recommended_from(&v)).map_err(|e| e.to_string()),
        (c, _) => Err(format!("models: HTTP {c}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_what_a_credit_resets() {
        assert_eq!(credit_kind(&serde_json::json!({ "limit_type": "weekly" })), "week");
        assert_eq!(credit_kind(&serde_json::json!({ "window": "primary" })), "5h");
        assert_eq!(credit_kind(&serde_json::json!({ "model_slug": "gpt-6-sol" })), "model:gpt-6-sol");
        assert_eq!(credit_kind(&serde_json::json!({})), "other");
    }

    #[test]
    fn reads_recommended_levels() {
        let v = serde_json::json!({ "models": [ { "slug": "gpt-6-luna", "default_reasoning_level": "medium" }, { "slug": "x" } ] });
        let m = recommended_from(&v);
        assert_eq!(m.get("gpt-6-luna").map(String::as_str), Some("medium"));
        assert_eq!(m.len(), 1);
    }

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
