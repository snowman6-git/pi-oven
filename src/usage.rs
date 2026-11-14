//! Plan limits asked for directly, without sending a message.
//!
//! ChatGPT: `GET https://chatgpt.com/backend-api/wham/usage`, the address Codex CLI's `/status` uses (not a public API).
//! The sign-in is pi's: `pi auth print-bearer-token` hands out a fresh token (refreshing it when needed), and the
//! account id comes from inside that token, so pi-oven never reads `auth.json`. The token goes to chatgpt.com only, on
//! curl's standard input (never on a command line, where `ps` would show it), and is not kept.
//!
//! What comes back goes into the same store as the numbers replies bring (`limits.rs`); the newer one wins. If the
//! address changes or the sign-in is missing, the replies' numbers are what is left, as before.

use axum::{extract::State, http::StatusCode, response::{IntoResponse, Response}, Json};
use std::{
    process::Stdio,
    sync::{atomic::{AtomicBool, AtomicU64, Ordering}, Arc},
};
use tokio::{io::AsyncWriteExt, process::Command};

use crate::{
    api::fail,
    limits::{Entry, Window},
    runtime::now_ms,
    AppState,
};

const CODEX_USAGE: &str = "https://chatgpt.com/backend-api/wham/usage";
/// Asking more often than this gives nothing new.
const MIN_GAP_MS: u64 = 20_000;

pub struct Usage {
    /// The switch (Settings → Chat → Plan limits), on unless turned off.
    pub on: AtomicBool,
    last: AtomicU64,
    busy: tokio::sync::Mutex<()>,
}

impl Default for Usage {
    fn default() -> Self {
        let off = std::fs::read_to_string(crate::config::cache_dir().join("usage-poll")).is_ok_and(|v| v.trim() == "off");
        Self { on: AtomicBool::new(!off), last: AtomicU64::new(0), busy: Default::default() }
    }
}

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
        provider: "openai-codex".into(),
        plan: v["plan_type"].as_str().map(str::to_owned),
        at: now,
        limit_reached: rl["limit_reached"].as_bool().unwrap_or(false) || rl["allowed"].as_bool() == Some(false),
        windows,
        resets: v["rate_limit_reset_credits"]["available_count"].as_u64().map(|n| n as u32),
    })
}

impl Usage {
    /// Ask ChatGPT now (unless it was asked moments ago) and store the answer. `Ok(false)`: nothing asked.
    pub async fn poll(&self, pi_bin: &str, limits: &crate::limits::Limits, force: bool) -> Result<bool, String> {
        if !self.on.load(Ordering::Relaxed) {
            return Ok(false);
        }
        let Ok(_one) = self.busy.try_lock() else { return Ok(false) };
        let now = now_ms();
        if !force && now.saturating_sub(self.last.load(Ordering::Relaxed)) < MIN_GAP_MS {
            return Ok(false);
        }
        self.last.store(now, Ordering::Relaxed);

        let tok = Command::new(pi_bin)
            .args(["auth", "print-bearer-token", "--provider", "openai-codex"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|e| format!("pi auth: {e}"))?;
        if !tok.status.success() {
            return Err("no ChatGPT sign-in".into()); // not signed in: nothing to show, and no error worth a notice
        }
        let token = String::from_utf8_lossy(&tok.stdout).trim().to_owned();
        let account = account_id(&token).ok_or("no account id in the token")?;
        if token.contains(['\r', '\n']) || account.contains(['\r', '\n']) {
            return Err("unexpected token".into());
        }

        let mut curl = Command::new("curl")
            .args(["-fsS", "--proto", "=https", "--max-time", "20", "-H", "@-", "-H", "Accept: application/json", "-H", "User-Agent: pi-oven", CODEX_USAGE])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("curl: {e}"))?;
        if let Some(mut stdin) = curl.stdin.take() {
            let headers = format!("Authorization: Bearer {token}\nchatgpt-account-id: {account}\n");
            stdin.write_all(headers.as_bytes()).await.map_err(|e| e.to_string())?;
        }
        let out = curl.wait_with_output().await.map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!("usage: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("usage: {e}"))?;
        let e = entry_from(&v, now_ms()).ok_or("usage: no limits in the answer")?;
        limits.record(e);
        Ok(true)
    }
}

/// POST: ask now (the page does this when it opens, every few minutes while it is in view, and when the limits are
/// looked at). Answers with the store, whatever happened.
pub async fn refresh(State(app): State<Arc<AppState>>) -> Json<serde_json::Value> {
    if let Err(e) = app.runtimes.usage.poll(&app.pi_bin, &app.runtimes.limits, false).await {
        tracing::debug!("usage poll: {e}");
    }
    Json(app.runtimes.limits.view(now_ms()))
}

#[derive(serde::Deserialize)]
pub struct SetPoll {
    enabled: bool,
}

pub async fn get_poll(State(app): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "enabled": app.runtimes.usage.on.load(Ordering::Relaxed) }))
}

pub async fn set_poll(State(app): State<Arc<AppState>>, Json(req): Json<SetPoll>) -> Response {
    app.runtimes.usage.on.store(req.enabled, Ordering::Relaxed);
    let dir = crate::config::cache_dir();
    if std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join("usage-poll"), if req.enabled { "on" } else { "off" })).is_err() {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, "could not save the setting");
    }
    Json(serde_json::json!({ "enabled": req.enabled })).into_response()
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
