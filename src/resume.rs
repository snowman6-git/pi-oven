//! "Continue after the limit": when a subscription's usage limit stops a run, send a chosen message (default "계속해")
//! once the limit has reset, so the work carries on without anyone at the keyboard.
//!
//! pi does not retry a usage limit (it resets in hours), so the run just ends with that error. The server sees it,
//! works out when the limit resets (the time in the error, else the limits the provider last sent), and keeps the
//! schedule here, so it goes on with every browser closed. Anything the user sends before then replaces it.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc, sync::Mutex};

use crate::{api::fail, AppState};

pub const DEFAULT_TEXT: &str = "계속해";
/// After the reset: just enough for the provider's clock to agree. The point is not to lose a minute.
const MARGIN_MS: u64 = 10_000;
/// Sent a little too early (the limit still holds right after the reset): try again soon, not after a whole window.
pub const EARLY_RETRY_MS: u64 = 60_000;
/// A limit hit this soon after our own message counts as "a little too early".
pub const EARLY_WINDOW_MS: u64 = 10 * 60_000;
/// When nothing says when the limit resets: look again in an hour.
const FALLBACK_MS: u64 = 60 * 60_000;
/// A message that runs into the limit again waits again, but not forever.
pub const MAX_TRIES: u32 = 10;
const MAX_TEXT: usize = 2000;

#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    pub enabled: bool,
    pub text: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self { enabled: true, text: DEFAULT_TEXT.into() }
    }
}

/// The switch and the message, shared by every session and device.
pub struct Resume {
    s: Mutex<Settings>,
    file: Option<PathBuf>,
}

impl Resume {
    pub fn load(dir: Option<&std::path::Path>) -> Self {
        let file = dir.map(|d| d.join("resume.json"));
        let s = file
            .as_ref()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|b| serde_json::from_slice::<Settings>(&b).ok())
            .filter(|s| !s.text.trim().is_empty())
            .unwrap_or_default();
        Self { s: Mutex::new(s), file }
    }

    pub fn get(&self) -> Settings {
        self.s.lock().unwrap().clone()
    }

    fn set(&self, s: Settings) -> std::io::Result<()> {
        *self.s.lock().unwrap() = s.clone();
        let Some(file) = &self.file else { return Ok(()) };
        let tmp = file.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec(&s).unwrap_or_default())?;
        std::fs::rename(&tmp, file)
    }
}

/// A session waiting for its limit to reset.
#[derive(Clone, Debug, PartialEq)]
pub struct Pending {
    pub at: u64,
    pub tries: u32,
    /// Which window ran out (minutes), when the limits say so.
    pub window: Option<u32>,
    /// Whether the reset time is known (from the error or the limits) or only a guess.
    pub known: bool,
}

impl Pending {
    /// For the page: the time left, since a phone's clock need not agree with the server's.
    pub fn view(&self, now: u64, text: &str) -> serde_json::Value {
        serde_json::json!({ "inMs": self.at.saturating_sub(now), "text": text, "tries": self.tries, "window": self.window, "known": self.known })
    }
}

/// A run that ended on a usage limit: the error text and the provider, if this line says so.
pub fn limit_error(line: &str) -> Option<(String, Option<String>)> {
    let (error, provider) = if line.starts_with(r#"{"type":"message_end""#) && line.contains(r#""stopReason":"error""#) {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        (v["message"]["errorMessage"].as_str()?.to_owned(), v["message"]["provider"].as_str().map(str::to_owned))
    } else if line.starts_with(r#"{"type":"auto_retry_end""#) && line.contains(r#""success":false"#) {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        (v["finalError"].as_str()?.to_owned(), None)
    } else {
        return None;
    };
    is_usage_limit(&error).then_some((error, provider))
}

/// The subscription's own limit, not a short rate limit (pi retries those itself).
pub fn is_usage_limit(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    e.contains("usage limit") || e.contains("usage_limit") || e.contains("hit your limit") || e.contains("limit reached")
}

/// "… Try again in ~42 min." → 42 minutes.
fn minutes_in(error: &str) -> Option<u64> {
    let i = error.find("again in")?;
    let digits: String = error[i + 8..].chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).take(6).collect();
    digits.parse().ok()
}

/// When to send: the reset the error names, else when the window that ran out resets, else in an hour.
/// Returns the time, the window (minutes) and whether the time is known.
pub fn due(error: &str, culprit: Option<(u64, u32)>, now: u64) -> (u64, Option<u32>, bool) {
    if let Some(m) = minutes_in(error) {
        return (now + m * 60_000 + MARGIN_MS, culprit.map(|c| c.1), true);
    }
    match culprit {
        Some((t, w)) if t > now => (t + MARGIN_MS, Some(w), true),
        _ => (now + FALLBACK_MS, None, false),
    }
}

#[derive(Deserialize)]
pub struct SetSettings {
    enabled: bool,
    text: String,
}

pub async fn get_settings(State(app): State<Arc<AppState>>) -> Json<Settings> {
    Json(app.runtimes.resume.get())
}

pub async fn set_settings(State(app): State<Arc<AppState>>, Json(req): Json<SetSettings>) -> Response {
    let text = req.text.trim();
    if text.is_empty() || text.chars().count() > MAX_TEXT {
        return fail(StatusCode::BAD_REQUEST, "the message must be 1 to 2000 characters");
    }
    let s = Settings { enabled: req.enabled, text: text.to_owned() };
    if let Err(e) = app.runtimes.resume.set(s.clone()) {
        tracing::warn!("could not save the continue setting: {e}");
    }
    Json(s).into_response()
}

#[derive(Deserialize)]
pub struct Act {
    /// "now" or "stop"
    action: String,
}

pub async fn act(State(app): State<Arc<AppState>>, Path(id): Path<String>, Json(req): Json<Act>) -> Response {
    let Some(rt) = app.runtimes.get(&id).await else { return fail(StatusCode::NOT_FOUND, "no such session") };
    match req.action.as_str() {
        "now" => rt.resume_now(),
        "stop" => rt.resume_stop(),
        _ => return fail(StatusCode::BAD_REQUEST, "action must be now or stop"),
    }
    StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_a_usage_limit_from_other_errors() {
        let codex = r#"{"type":"message_end","message":{"role":"assistant","stopReason":"error","errorMessage":"You have hit your ChatGPT usage limit (plus plan). Try again in ~42 min."}}"#;
        assert!(limit_error(codex).is_some());
        let other = r#"{"type":"message_end","message":{"role":"assistant","stopReason":"error","errorMessage":"503 overloaded"}}"#;
        assert!(limit_error(other).is_none());
        let fine = r#"{"type":"message_end","message":{"role":"assistant","stopReason":"stop"}}"#;
        assert!(limit_error(fine).is_none());
        let retried = r#"{"type":"auto_retry_end","success":false,"attempt":3,"finalError":"usage_limit_reached"}"#;
        assert!(limit_error(retried).is_some());
    }

    #[test]
    fn waits_for_the_named_time_then_the_known_reset_then_an_hour() {
        assert_eq!(due("… Try again in ~42 min.", Some((9_999_999, 300)), 1000), (1000 + 42 * 60_000 + MARGIN_MS, Some(300), true));
        // what Codex really says: no time in it
        assert_eq!(due("Codex error: The usage limit has been reached", Some((5000, 300)), 1000), (5000 + MARGIN_MS, Some(300), true));
        assert_eq!(due("usage limit", Some((500, 300)), 1000), (1000 + FALLBACK_MS, None, false));
        assert_eq!(due("usage limit", None, 1000), (1000 + FALLBACK_MS, None, false));
    }
}
