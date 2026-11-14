//! Plan limits asked for directly, without sending a message: when to ask, and how often at most.
//!
//! Each provider has its own file (`chatgpt.rs`; Claude is planned, see `docs/PLANNED.md`). The answers go into the
//! same store as the numbers replies bring (`limits.rs`); the newer one wins. If asking fails, the replies' numbers
//! are what is left.
//!
//! These addresses are not public APIs, so they are asked gently: never more than once a minute (once every 15 s
//! right after a usage-limit error), and after a failure less and less often (up to 30 minutes apart); a 429 waits
//! 15 minutes. However many pages are open and however often the limits are looked at, the server asks for all.

pub mod chatgpt;

use axum::{extract::State, http::StatusCode, response::{IntoResponse, Response}, Json};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    Arc,
};

use crate::{api::fail, limits::Entry, runtime::now_ms, AppState};

/// The least time between two questions when nothing is wrong.
const MIN_GAP_MS: u64 = 60_000;
/// Right after a usage-limit error the numbers matter at once, but not more often than this.
const MIN_GAP_FORCED_MS: u64 = 15_000;
const BACKOFF_MAX_MS: u64 = 30 * 60_000;
const TOO_MANY_MS: u64 = 15 * 60_000;

/// What one question came back with.
pub enum Asked {
    Got(Entry),
    NoSignIn,
    TooMany,
    Failed(String),
}

pub struct Usage {
    /// The switch (Settings → Chat → Plan limits), on unless turned off.
    pub on: AtomicBool,
    last: AtomicU64,
    /// No question before this time (after failures, or a 429).
    quiet_until: AtomicU64,
    failures: AtomicU32,
    busy: tokio::sync::Mutex<()>,
}

impl Default for Usage {
    fn default() -> Self {
        let off = std::fs::read_to_string(crate::config::cache_dir().join("usage-poll")).is_ok_and(|v| v.trim() == "off");
        Self { on: AtomicBool::new(!off), last: AtomicU64::new(0), quiet_until: AtomicU64::new(0), failures: AtomicU32::new(0), busy: Default::default() }
    }
}

/// How long to stay quiet after `n` failures in a row: 1, 2, 4 … minutes, at most 30.
pub fn backoff(n: u32) -> u64 {
    (60_000u64 << n.saturating_sub(1).min(5)).min(BACKOFF_MAX_MS)
}

impl Usage {
    /// May a question go out now? `forced`: right after a usage-limit error.
    pub fn may_ask(&self, now: u64, forced: bool) -> bool {
        let gap = if forced { MIN_GAP_FORCED_MS } else { MIN_GAP_MS };
        self.on.load(Ordering::Relaxed) && now >= self.quiet_until.load(Ordering::Relaxed) && now.saturating_sub(self.last.load(Ordering::Relaxed)) >= gap
    }

    fn note(&self, asked: &Asked, now: u64) {
        match asked {
            Asked::Got(_) | Asked::NoSignIn => {
                self.failures.store(0, Ordering::Relaxed);
                self.quiet_until.store(0, Ordering::Relaxed);
            }
            Asked::TooMany => {
                self.failures.fetch_add(1, Ordering::Relaxed);
                self.quiet_until.store(now + TOO_MANY_MS, Ordering::Relaxed);
            }
            Asked::Failed(_) => {
                let n = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
                self.quiet_until.store(now + backoff(n), Ordering::Relaxed);
            }
        }
    }

    /// Ask (if allowed now) and store the answer. True when fresh numbers came in.
    pub async fn poll(&self, pi_bin: &str, limits: &crate::limits::Limits, forced: bool) -> bool {
        let Ok(_one) = self.busy.try_lock() else { return false };
        let now = now_ms();
        if !self.may_ask(now, forced) {
            return false;
        }
        self.last.store(now, Ordering::Relaxed);
        let asked = chatgpt::ask(pi_bin, now).await;
        self.note(&asked, now_ms());
        match asked {
            Asked::Got(e) => {
                limits.record(e);
                true
            }
            Asked::NoSignIn => false,
            Asked::TooMany => {
                tracing::info!("usage: ChatGPT said too many requests; asking again in 15 minutes");
                false
            }
            Asked::Failed(e) => {
                tracing::debug!("usage: {e}");
                false
            }
        }
    }
}

/// POST: ask now if allowed (the page does this when it opens, every few minutes while it is in view, and when the
/// limits are looked at). Answers with the store, whatever happened.
pub async fn refresh(State(app): State<Arc<AppState>>) -> Json<serde_json::Value> {
    app.runtimes.usage.poll(&app.pi_bin, &app.runtimes.limits, false).await;
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
    fn asks_at_most_once_a_minute_and_backs_off_after_failures() {
        let u = Usage { on: AtomicBool::new(true), last: AtomicU64::new(0), quiet_until: AtomicU64::new(0), failures: AtomicU32::new(0), busy: Default::default() };
        let t = 10_000_000;
        assert!(u.may_ask(t, false));
        u.last.store(t, Ordering::Relaxed);
        // opening and closing the limits over and over: nothing goes out for a minute
        for dt in [1_000, 10_000, 30_000, 59_000] {
            assert!(!u.may_ask(t + dt, false), "{dt}");
        }
        assert!(u.may_ask(t + 60_000, false));
        // a usage-limit error may ask sooner, but not in a burst
        assert!(!u.may_ask(t + 10_000, true));
        assert!(u.may_ask(t + 15_000, true));
        // failures: 1, 2, 4 … minutes, at most 30
        assert_eq!([1, 2, 3, 4, 5, 6, 9].map(backoff), [60_000, 120_000, 240_000, 480_000, 960_000, 1_800_000, 1_800_000]);
        u.note(&Asked::Failed("x".into()), t);
        assert!(!u.may_ask(t + 59_000, true));
        u.note(&Asked::TooMany, t);
        assert!(!u.may_ask(t + 14 * 60_000, false));
        assert!(u.may_ask(t + 15 * 60_000, false));
        // a good answer clears it
        u.note(&Asked::NoSignIn, t);
        assert!(u.may_ask(t + 60_000, false));
        // and the switch is the switch
        u.on.store(false, Ordering::Relaxed);
        assert!(!u.may_ask(t + 99_000_000, false));
    }
}
