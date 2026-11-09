//! "100h mode": when pi has used up its own retries on a temporary API error, try again every hour, up to 100 times.
//!
//! The schedule lives here and not in the page, so it keeps going with every browser closed. pi's events say when to
//! start and stop; a try is `/oven-retry`, a command of the bundled extension (`ext/oven-retry.ts`), because RPC has no
//! command that repeats a failed request. Errors pi does not retry itself (a bad key, a used-up plan) never get here.
//! The wait lives as long as the pi process does.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use crate::{api::fail, runtime::now_ms, AppState};

pub const INTERVAL_MS: u64 = 60 * 60 * 1000;
pub const MAX_TRIES: u32 = 100;
/// What pi calls a retry the user stopped: that is not a failure to carry on from.
const CANCELLED: &str = "Retry cancelled";
pub const TRY_COMMAND: &str = r#"{"type":"prompt","message":"/oven-retry"}"#;

/// The on/off switch, shared by every session and device. On unless turned off.
pub struct Long {
    on: AtomicBool,
    file: Option<PathBuf>,
    /// Without the extension there is nothing to run: `/oven-retry` would go to the model as text.
    available: bool,
}

impl Long {
    pub fn load(extension: Option<&PathBuf>) -> Self {
        let file = extension.and_then(|e| e.parent()).map(|d| d.join("long-retry"));
        let off = file.as_ref().and_then(|f| std::fs::read_to_string(f).ok()).is_some_and(|v| v.trim() == "off");
        Self { on: AtomicBool::new(!off), file, available: extension.is_some() }
    }

    pub fn enabled(&self) -> bool {
        self.available && self.on.load(Ordering::Relaxed)
    }

    fn set(&self, on: bool) -> std::io::Result<()> {
        self.on.store(on, Ordering::Relaxed);
        let Some(file) = &self.file else { return Ok(()) };
        let tmp = file.with_extension("tmp");
        std::fs::write(&tmp, if on { "on" } else { "off" })?;
        std::fs::rename(&tmp, file)
    }
}

/// A session's place in the schedule. `next_at` is empty while a try is under way.
#[derive(Clone, Debug, PartialEq)]
pub struct Wait {
    pub attempt: u32,
    pub next_at: Option<u64>,
    pub error: String,
}

impl Wait {
    /// For the page. The time left, not the time: a phone's clock need not agree with the server's.
    pub fn view(&self, now: u64) -> serde_json::Value {
        serde_json::json!({
            "attempt": self.attempt,
            "max": MAX_TRIES,
            "inMs": self.next_at.map(|t| t.saturating_sub(now)),
            "error": self.error,
        })
    }
}

#[derive(Debug, PartialEq)]
pub enum Event {
    Started,
    Settled,
    Failed(String),
}

/// The events of pi that matter here. Looks at the start of the line first: this runs for every line pi prints.
pub fn read(line: &str) -> Option<Event> {
    if line.starts_with(r#"{"type":"agent_start""#) {
        return Some(Event::Started);
    }
    if line.starts_with(r#"{"type":"agent_settled""#) {
        return Some(Event::Settled);
    }
    if !line.starts_with(r#"{"type":"auto_retry_end""#) {
        return None;
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct End {
        success: bool,
        final_error: Option<String>,
    }
    let end: End = serde_json::from_str(line).ok()?;
    let error = end.final_error.unwrap_or_default();
    (!end.success && error != CANCELLED).then_some(Event::Failed(error))
}

#[derive(Debug, PartialEq)]
pub enum Step {
    Same,
    Changed,
    GaveUp,
}

pub fn advance(state: &mut Option<Wait>, event: Event, enabled: bool, now: u64) -> Step {
    match (event, state.as_mut()) {
        (Event::Failed(error), None) if enabled => {
            *state = Some(Wait { attempt: 1, next_at: Some(now + INTERVAL_MS), error });
            Step::Changed
        }
        (Event::Failed(_), None) => Step::Same,
        (Event::Failed(_), Some(w)) if w.attempt >= MAX_TRIES => {
            *state = None;
            Step::GaveUp
        }
        (Event::Failed(_), Some(_)) if !enabled => {
            *state = None;
            Step::Changed
        }
        (Event::Failed(error), Some(w)) => {
            w.attempt += 1;
            w.next_at = Some(now + INTERVAL_MS);
            w.error = error;
            Step::Changed
        }
        // while waiting, a run that starts is the user's: what they sent replaces the failed request
        (Event::Started, Some(w)) if w.next_at.is_some() => {
            *state = None;
            Step::Changed
        }
        // a try that ends without a new failure is over: it worked, it was stopped, or pi will not retry its error
        (Event::Settled, Some(w)) if w.next_at.is_none() => {
            *state = None;
            Step::Changed
        }
        _ => Step::Same,
    }
}

#[derive(Serialize)]
pub struct Mode {
    enabled: bool,
    available: bool,
}

fn mode(long: &Long) -> Json<Mode> {
    Json(Mode { enabled: long.enabled(), available: long.available })
}

pub async fn get_mode(State(app): State<Arc<AppState>>) -> Json<Mode> {
    mode(&app.runtimes.long)
}

#[derive(Deserialize)]
pub struct SetMode {
    enabled: bool,
}

pub async fn set_mode(State(app): State<Arc<AppState>>, Json(req): Json<SetMode>) -> Response {
    let long = &app.runtimes.long;
    if !long.available {
        return fail(StatusCode::CONFLICT, tr!("이 서버에는 재시도 확장이 설치되지 않았습니다", "the retry extension is not installed on this server"));
    }
    if let Err(e) = long.set(req.enabled) {
        tracing::warn!("could not save the 100h mode: {e}"); // still applies until the server restarts
    }
    mode(long).into_response()
}

#[derive(Deserialize)]
pub struct Act {
    /// "now" or "stop"
    action: String,
}

pub async fn act(State(app): State<Arc<AppState>>, Path(id): Path<String>, Json(req): Json<Act>) -> Response {
    let Some(rt) = app.runtimes.get(&id).await else { return fail(StatusCode::NOT_FOUND, tr!("세션을 찾을 수 없습니다", "no such session")) };
    match req.action.as_str() {
        "now" => rt.long_try_now(now_ms()),
        "stop" => rt.long_stop(),
        _ => return fail(StatusCode::BAD_REQUEST, tr!("action은 now, stop 중 하나여야 합니다", "action must be now or stop")),
    }
    StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed() -> Event {
        Event::Failed("503".into())
    }

    #[test]
    fn reads_only_what_it_needs() {
        assert_eq!(read(r#"{"type":"agent_start"}"#), Some(Event::Started));
        assert_eq!(read(r#"{"type":"agent_settled","aborted":false}"#), Some(Event::Settled));
        assert_eq!(read(r#"{"type":"auto_retry_end","success":false,"attempt":3,"finalError":"503"}"#), Some(failed()));
        assert_eq!(read(r#"{"type":"auto_retry_end","success":false,"attempt":1,"finalError":"Retry cancelled"}"#), None);
        assert_eq!(read(r#"{"type":"auto_retry_end","success":true,"attempt":2}"#), None);
        assert_eq!(read(r#"{"type":"message_end","note":"agent_start"}"#), None);
    }

    #[test]
    fn starts_after_pi_gives_up_and_only_when_on() {
        let mut s = None;
        assert_eq!(advance(&mut s, failed(), false, 0), Step::Same);
        assert_eq!(s, None);
        assert_eq!(advance(&mut s, failed(), true, 5), Step::Changed);
        assert_eq!(s, Some(Wait { attempt: 1, next_at: Some(5 + INTERVAL_MS), error: "503".into() }));
        // the end of the run that failed does not end the wait
        assert_eq!(advance(&mut s, Event::Settled, true, 6), Step::Same);
        assert!(s.is_some());
    }

    #[test]
    fn a_failed_try_waits_again_and_the_last_one_gives_up() {
        let mut s = Some(Wait { attempt: 1, next_at: None, error: String::new() });
        advance(&mut s, Event::Started, true, 0);
        assert_eq!(advance(&mut s, failed(), true, 10), Step::Changed);
        assert_eq!(s.as_ref().map(|w| (w.attempt, w.next_at)), Some((2, Some(10 + INTERVAL_MS))));

        let mut s = Some(Wait { attempt: MAX_TRIES, next_at: None, error: String::new() });
        assert_eq!(advance(&mut s, failed(), true, 0), Step::GaveUp);
        assert_eq!(s, None);
    }

    #[test]
    fn a_try_that_settles_clean_ends_it() {
        let mut s = Some(Wait { attempt: 4, next_at: None, error: String::new() });
        advance(&mut s, Event::Started, true, 0);
        assert!(s.is_some());
        assert_eq!(advance(&mut s, Event::Settled, true, 0), Step::Changed);
        assert_eq!(s, None);
    }

    #[test]
    fn a_new_run_while_waiting_replaces_the_failed_request() {
        let mut s = Some(Wait { attempt: 4, next_at: Some(99), error: String::new() });
        assert_eq!(advance(&mut s, Event::Started, true, 0), Step::Changed);
        assert_eq!(s, None);
    }

    #[test]
    fn turned_off_during_a_try_it_does_not_wait_again() {
        let mut s = Some(Wait { attempt: 2, next_at: None, error: String::new() });
        assert_eq!(advance(&mut s, failed(), false, 0), Step::Changed);
        assert_eq!(s, None);
    }

    #[test]
    fn the_switch_is_on_by_default_and_remembered() {
        let dir = std::env::temp_dir().join(format!("pi-oven-long-{}", crate::config::random_hex(4)));
        std::fs::create_dir_all(&dir).unwrap();
        let ext = dir.join("oven-retry.ts");
        let long = Long::load(Some(&ext));
        assert!(long.enabled());
        long.set(false).unwrap();
        assert!(!Long::load(Some(&ext)).enabled());
        assert!(!Long::load(None).enabled());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
