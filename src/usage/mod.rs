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
pub mod claude;

use axum::{extract::State, http::StatusCode, response::{IntoResponse, Response}, Json};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    Arc,
};

use crate::{api::fail, limits::Entry, runtime::now_ms, AppState};
use std::process::Stdio;
use tokio::{io::AsyncWriteExt, process::Command};

/// A fresh sign-in token for `provider` from pi (it refreshes an expired one). `None`: not signed in there.
pub(crate) async fn token(pi_bin: &str, provider: &str) -> Result<Option<String>, String> {
    let run = Command::new(pi_bin)
        .args(["auth", "print-bearer-token", "--provider", provider])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output();
    // a limit check waits on this (Zero-Waste decides after it): pi refreshing over a dead network must not hang it
    let out = tokio::time::timeout(std::time::Duration::from_secs(30), run)
        .await
        .map_err(|_| "pi auth: timed out".to_owned())?
        .map_err(|e| format!("pi auth: {e}"))?;
    if !out.status.success() {
        return Ok(None);
    }
    let t = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if t.is_empty() || t.contains(['\r', '\n']) {
        return Err("unexpected token".into());
    }
    Ok(Some(t))
}

/// GET `url` with `headers` ("Name: value" lines) given on curl's standard input, never on its command line (where
/// `ps` would show a token). Returns the status and the body.
pub(crate) async fn get(url: &str, headers: &str) -> Result<(u16, String), String> {
    let mut child = Command::new("curl")
        .args(["-sS", "--proto", "=https", "--max-time", "20", "-H", "@-", "-H", "Accept: application/json", "-H", "User-Agent: pi-oven", "-w", "\n%{http_code}", url])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("curl: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(headers.as_bytes()).await.map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().await.map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", text.as_str()));
    match code.trim().parse::<u16>().unwrap_or(0) {
        0 => Err(format!("usage: {}", String::from_utf8_lossy(&out.stderr).trim())),
        c => Ok((c, body.to_owned())),
    }
}

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

/// One provider's pacing: when it was last asked, and how long to keep quiet after failures.
#[derive(Default)]
pub struct Gate {
    last: AtomicU64,
    /// No question before this time (after failures, or a 429).
    quiet_until: AtomicU64,
    failures: AtomicU32,
}

impl Gate {
    /// May a question go out now? `forced`: right after a usage-limit error.
    pub fn may_ask(&self, now: u64, forced: bool) -> bool {
        let gap = if forced { MIN_GAP_FORCED_MS } else { MIN_GAP_MS };
        now >= self.quiet_until.load(Ordering::Relaxed) && now.saturating_sub(self.last.load(Ordering::Relaxed)) >= gap
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
}

pub struct Usage {
    /// The switch (Settings → Plan limits), on unless turned off.
    pub on: AtomicBool,
    chatgpt: Gate,
    claude: Gate,
    busy: tokio::sync::Mutex<()>,
    /// When the recommended reasoning levels were last fetched, and what they were.
    recommended: std::sync::Mutex<(u64, std::collections::BTreeMap<String, String>)>,
}

impl Default for Usage {
    fn default() -> Self {
        let off = std::fs::read_to_string(crate::config::cache_dir().join("usage-poll")).is_ok_and(|v| v.trim() == "off");
        Self { on: AtomicBool::new(!off), chatgpt: Gate::default(), claude: Gate::default(), busy: Default::default(), recommended: Default::default() }
    }
}

/// How long to stay quiet after `n` failures in a row: 1, 2, 4 … minutes, at most 30.
pub fn backoff(n: u32) -> u64 {
    (60_000u64 << n.saturating_sub(1).min(5)).min(BACKOFF_MAX_MS)
}

impl Usage {
    /// Ask the providers that may be asked now (only `provider`, when given) and store the answers. True when fresh
    /// numbers came in. A provider without a sign-in is a quiet no.
    pub async fn poll(&self, pi_bin: &str, limits: &crate::limits::Limits, forced: bool, provider: Option<&str>) -> bool {
        if !self.on.load(Ordering::Relaxed) {
            return false;
        }
        // one question round at a time; a later caller waits its turn (the gates then keep it from asking twice)
        // instead of giving up, which used to drop the question for a model just switched to
        let _one = self.busy.lock().await;
        let mut fresh = false;
        for (id, gate) in [(chatgpt::PROVIDER, &self.chatgpt), (claude::PROVIDER, &self.claude)] {
            if provider.is_some_and(|p| p != id) {
                continue;
            }
            let now = now_ms();
            if !gate.may_ask(now, forced) {
                continue;
            }
            gate.last.store(now, Ordering::Relaxed);
            let asked = if id == chatgpt::PROVIDER { chatgpt::ask(pi_bin, now).await } else { claude::ask(pi_bin).await };
            gate.note(&asked, now_ms());
            let next = gate.quiet_until.load(Ordering::Relaxed);
            match asked {
                Asked::Got(e) => {
                    limits.record_since(now, e);
                    limits.note_problem(id, None);
                    fresh = true;
                }
                Asked::NoSignIn => limits.note_problem(id, None),
                Asked::TooMany => {
                    tracing::info!("usage: {id} said too many requests; asking again in 15 minutes");
                    limits.note_problem(id, Some(("HTTP 429".into(), next)));
                }
                Asked::Failed(e) => {
                    tracing::info!("usage {id}: {e}");
                    limits.note_problem(id, Some((e.chars().take(120).collect(), next)));
                }
            }
        }
        fresh
    }
}

/// How often the server asks by itself, whether or not a page is open (on the clock: :00, :05, …).
pub const EVERY_MS: u64 = 5 * 60_000;
/// After a known reset, the numbers are asked this much later (the provider has moved on by then).
const AFTER_RESET_MS: u64 = 20_000;

/// The server's own questions: every five minutes on the clock, and just after any window held (in use or not) is due
/// to reset, so a reset is seen when it happens rather than when a page next looks. The usual gates still apply.
pub async fn watch(rt: Arc<crate::runtime::Manager>, pi_bin: String) {
    let mut tick = (now_ms() / EVERY_MS + 1) * EVERY_MS;
    // resets up to this one have been asked after (those before the start are not chased)
    let mut done_reset = now_ms();
    loop {
        // looked at again every minute at most: numbers that came in meanwhile can bring a reset sooner than the tick
        let now = now_ms();
        let reset = [rt.limits.next_reset(done_reset), rt.accounts.next_reset(done_reset)].into_iter().flatten().min();
        let after = reset.map(|t| t + AFTER_RESET_MS);
        let (at, forced) = match after {
            Some(t) if t < tick => (t, true),
            _ => (tick, false),
        };
        if at > now {
            tokio::time::sleep(std::time::Duration::from_millis((at - now).min(60_000))).await;
            continue;
        }
        if forced {
            done_reset = reset.unwrap_or(now);
        } else {
            tick = (now / EVERY_MS + 1) * EVERY_MS;
        }
        if !rt.usage.on.load(Ordering::Relaxed) {
            continue;
        }
        rt.usage.poll(&pi_bin, &rt.limits, forced, None).await;
        rt.accounts.poll_others(&pi_bin).await;
    }
}

/// POST: ask now if allowed (the page does this when it opens, every few minutes while it is in view, and when the
/// limits are looked at). Answers with the store, whatever happened.
pub async fn refresh(State(app): State<Arc<AppState>>) -> Json<serde_json::Value> {
    app.runtimes.usage.poll(&app.pi_bin, &app.runtimes.limits, false, None).await;
    // the other saved accounts too (each at most every 5 minutes), when the switch for asking is on
    if app.runtimes.usage.on.load(Ordering::Relaxed) {
        app.runtimes.accounts.poll_others(&app.pi_bin).await;
    }
    Json(app.runtimes.accounts.limits_view(&app.runtimes.limits, now_ms()))
}

/// GET: the reasoning level each model's provider recommends, where the provider says (ChatGPT does; others do not,
/// and nothing is made up for them). Asked at most once an hour.
impl Usage {
    /// The recommended levels last fetched, `provider` → model → level (what a new session starts with when its model
    /// has no level of its own: `model_prefs.rs`).
    pub fn recommended_now(&self) -> std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>> {
        let c = self.recommended.lock().unwrap();
        std::collections::BTreeMap::from([(chatgpt::PROVIDER.to_owned(), c.1.clone())])
    }
}

pub async fn recommended(State(app): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let u = &app.runtimes.usage;
    let now = now_ms();
    {
        let c = u.recommended.lock().unwrap();
        if c.0 > 0 && now.saturating_sub(c.0) < 3_600_000 {
            return Json(serde_json::json!({ (chatgpt::PROVIDER): c.1 }));
        }
    }
    match chatgpt::recommended(&app.pi_bin).await {
        Ok(m) => {
            *u.recommended.lock().unwrap() = (now, m.clone());
            Json(serde_json::json!({ (chatgpt::PROVIDER): m }))
        }
        Err(e) => {
            tracing::debug!("recommended levels: {e}");
            let c = u.recommended.lock().unwrap();
            Json(serde_json::json!({ (chatgpt::PROVIDER): c.1 }))
        }
    }
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
        let u = Gate::default();
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
    }

}
