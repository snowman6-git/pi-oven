//! Compaction while nobody is working (Settings → Auto → Compaction), and what to do when a plan's limit is nearly used.
//!
//! The prompt cache lasts an hour. A session left alone longer than that pays for its whole context again at the
//! next message (a 300 000-token context written anew, not read), so a session that has been entirely idle for a while
//! is compacted before the cache goes: the next message then goes out on a small context. Only the server can do it
//! with every browser closed, so the schedule is kept here, per session, from pi's events.
//!
//! The same loop does the second job: when the plan the session runs on has little left (`limit_percent`), once per
//! window and session, the chosen action runs as soon as the session is idle — compact it, or send a prompt that makes
//! it write a hand-over or commit its work. After a limit is used up Zero-Waste and Baton Touch take over.
//!
//! The numbers (wait, smallest size, the limit's threshold and action) are pi-oven's own: `<cache>/auto-compact.json`.
//! `keepRecentTokens` is pi's and lives in its settings (`pi_settings.rs`).

use axum::{extract::State, http::StatusCode, response::{IntoResponse, Response}, Json};
use serde::{Deserialize, Serialize};
use std::{path::{Path, PathBuf}, sync::{Arc, Mutex}};

use crate::{api::fail, AppState};

pub const DEFAULT_IDLE_MINUTES: u32 = 50;
pub const DEFAULT_MIN_TOKENS: u64 = 120_000;
pub const DEFAULT_LIMIT_PERCENT: u32 = 15;
const MAX_PROMPT: usize = 8000;

/// What the compaction is told to keep: what the next message needs to carry on without asking again.
pub const COMPACT_INSTRUCTIONS: &str = "Keep, in this order: the user's instructions and preferences that still apply; decisions made and why; the files touched or still relevant (full paths); what is done and what remains, with the next step. Drop tool output and dead ends.";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Compact a session left entirely idle.
    pub idle_enabled: bool,
    /// ... after this many minutes.
    pub idle_minutes: u32,
    /// Not below this many context tokens (compaction and the limit's compact action alike).
    pub min_tokens: u64,
    pub limit_enabled: bool,
    /// Act when this % or less of the plan's window is left.
    pub limit_percent: u32,
    /// "compact", "summary", "commit" or "custom".
    pub limit_action: String,
    /// What the page says to send for the prompt actions (the page writes it in the person's language).
    pub limit_prompt: String,
    /// "provider/modelId" that writes the summary; none: the conversation's own model (`ext/oven-compact.ts`).
    pub model: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { idle_enabled: false, idle_minutes: DEFAULT_IDLE_MINUTES, min_tokens: DEFAULT_MIN_TOKENS, limit_enabled: false, limit_percent: DEFAULT_LIMIT_PERCENT, limit_action: "compact".into(), limit_prompt: String::new(), model: None }
    }
}

impl Settings {
    fn check(&self) -> Result<(), String> {
        if !(1..=720).contains(&self.idle_minutes) {
            return Err("the wait must be 1 to 720 minutes".into());
        }
        if !(1_000..=100_000_000).contains(&self.min_tokens) {
            return Err("the smallest size must be 1,000 to 100,000,000 tokens".into());
        }
        if !(1..=99).contains(&self.limit_percent) {
            return Err("the limit threshold must be 1 to 99 percent".into());
        }
        if self.model.as_deref().is_some_and(|m| m.len() > 200 || m.split_once('/').is_none_or(|(p, i)| p.is_empty() || i.is_empty())) {
            return Err("the compaction model must be provider/modelId".into());
        }
        match self.limit_action.as_str() {
            "compact" => {}
            "summary" | "commit" | "custom" => {
                let n = self.limit_prompt.trim().chars().count();
                if n == 0 || self.limit_prompt.chars().count() > MAX_PROMPT {
                    return Err("the prompt must be 1 to 8000 characters".into());
                }
            }
            _ => return Err("action must be compact, summary, commit or custom".into()),
        }
        Ok(())
    }
}

/// A plan window acted on: provider, its length in minutes, its reset time.
pub type DoneWindow = (String, u32, u64);

/// The settings, shared by every session and device.
pub struct AutoCompact {
    s: Mutex<Settings>,
    file: PathBuf,
    /// The plan windows already acted on near their limit, by session file (kept while the server runs).
    limit_done: Mutex<std::collections::HashMap<String, Vec<DoneWindow>>>,
}

impl Default for AutoCompact {
    fn default() -> Self {
        Self::load(crate::config::cache_dir().join("auto-compact.json"))
    }
}

impl AutoCompact {
    pub fn load(file: PathBuf) -> Self {
        let s = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice::<Settings>(&b).ok()).filter(|s| s.check().is_ok()).unwrap_or_default();
        Self { s: Mutex::new(s), file, limit_done: Default::default() }
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    pub fn get(&self) -> Settings {
        self.s.lock().unwrap().clone()
    }

    /// Saved first: a setting that could not be written is not applied either.
    fn set(&self, s: Settings) -> std::io::Result<()> {
        let mut g = self.s.lock().unwrap();
        let bytes = serde_json::to_vec(&s).unwrap_or_default();
        crate::config::atomic_write(&self.file, &bytes, false)?;
        *g = s;
        Ok(())
    }

    pub fn limit_done(&self, session: &str) -> Vec<DoneWindow> {
        self.limit_done.lock().unwrap().get(session).cloned().unwrap_or_default()
    }

    pub fn mark_limit_done(&self, session: &str, window: DoneWindow) {
        let now = crate::runtime::now_ms();
        let mut map = self.limit_done.lock().unwrap();
        let list = map.entry(session.to_owned()).or_default();
        list.retain(|(_, _, reset_at)| *reset_at + 10 * 60_000 > now);
        list.push(window);
        if map.len() > 500 {
            map.retain(|_, v| {
                v.retain(|(_, _, reset_at)| *reset_at + 10 * 60_000 > now);
                !v.is_empty()
            });
        }
    }
}

/// How long one session has been idle, for the compaction while resting. `since` is when the count began (the last
/// settled run); anything that is not idle starts it again, so the wait is a whole one.
#[derive(Default, Debug)]
pub struct Clock {
    since: Option<u64>,
    fired: bool,
}

impl Clock {
    pub fn started(&mut self) {
        *self = Self::default();
    }

    pub fn settled(&mut self, now: u64) {
        self.since = Some(now);
        self.fired = false;
    }

    /// Has a run settled in this session (so there is something to count from)?
    pub fn counting(&self) -> bool {
        self.since.is_some()
    }

    /// Called every few seconds. True once, when the session has been entirely idle for `wait_ms`.
    pub fn tick(&mut self, idle: bool, now: u64, wait_ms: u64) -> bool {
        let Some(since) = self.since else { return false };
        if !idle {
            self.since = Some(now);
            return false;
        }
        if self.fired || now < since + wait_ms {
            return false;
        }
        self.fired = true;
        true
    }

    /// The compaction was not done (too small, no numbers): count the wait again rather than every tick.
    pub fn skipped(&mut self, now: u64) {
        self.since = Some(now);
        self.fired = false;
    }
}

/// A window of the plan that is nearly used and was not acted on yet.
#[derive(Debug, PartialEq)]
pub struct Near {
    pub provider: String,
    pub minutes: u32,
    pub reset_at: u64,
    pub left: f64,
}

/// Which window to act on: the one with the least left of those still running, if that is at or under `percent` and
/// not used up (Zero-Waste and Baton Touch have that). `done`: windows already acted on (a reset time within ten
/// minutes is the same window: the sources differ by seconds).
pub fn near_limit(entry: &crate::limits::Entry, now: u64, percent: u32, done: &[(String, u32, u64)]) -> Option<Near> {
    let w = entry.windows.iter().filter(|w| w.reset_at.is_some_and(|t| t > now)).min_by(|a, b| (100.0 - a.used).total_cmp(&(100.0 - b.used)))?;
    let left = 100.0 - w.used;
    let reset_at = w.reset_at?;
    if left <= 0.0 || left > percent as f64 || entry.limit_reached {
        return None;
    }
    let seen = done.iter().any(|(p, m, t)| *p == entry.provider && *m == w.minutes && t.abs_diff(reset_at) <= 10 * 60_000);
    (!seen).then(|| Near { provider: entry.provider.clone(), minutes: w.minutes, reset_at, left })
}

const BG_ACTIVE: &str = "running";
/// A run whose status file was not touched for this long is taken as dead (a crashed runner leaves "running").
const SUBAGENT_STALE_MS: u64 = 10 * 60_000;

/// Is something still going on beside pi's own run: a background task or an asynchronous subagent of this pi
/// (pi-background-tasks and pi-subagents keep their state in files, and name the pi by its pid)? Reads files: not on the
/// async workers.
pub fn others_busy(cwd: &str, pid: u32, now: u64) -> bool {
    let suffix = format!("-{pid}");
    let tasks = std::fs::read_dir(PathBuf::from(cwd).join(".pi/tasks")).into_iter().flatten().flatten().filter(|e| e.path().is_dir() && e.file_name().to_string_lossy().ends_with(&suffix));
    for dir in tasks {
        for f in std::fs::read_dir(dir.path()).into_iter().flatten().flatten() {
            let name = f.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".json") || name.ends_with(".attestation.json") {
                continue;
            }
            let v = std::fs::read_to_string(f.path()).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
            if v.is_some_and(|v| v["status"] == BG_ACTIVE) {
                return true;
            }
        }
    }
    subagents_busy(&std::env::temp_dir(), std::env::var_os("PI_SUBAGENTS_TEMP_ROOT").filter(|v| !v.is_empty()).map(PathBuf::from).as_deref(), pid, now)
}

fn subagents_busy(temp: &Path, root_env: Option<&Path>, pid: u32, now: u64) -> bool {
    let mut roots: Vec<PathBuf> = Vec::new();
    match root_env {
        Some(r) => roots.push(r.to_owned()),
        None => roots.extend(std::fs::read_dir(temp).into_iter().flatten().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("pi-subagents-")).map(|e| e.path())),
    }
    for root in roots {
        for run in std::fs::read_dir(root.join("async-subagent-runs")).into_iter().flatten().flatten() {
            let Some(v) = std::fs::read_to_string(run.path().join("status.json")).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { continue };
            if !matches!(v["state"].as_str(), Some("running" | "queued" | "pending" | "starting")) {
                continue;
            }
            let mine = v["completionOwnerProcess"]["pid"].as_u64() == Some(pid as u64);
            let fresh = v["lastUpdate"].as_u64().or(v["lastActivityAt"].as_u64()).is_none_or(|t| now.saturating_sub(t) < SUBAGENT_STALE_MS);
            if mine && fresh {
                return true;
            }
        }
    }
    false
}

pub async fn get(State(app): State<Arc<AppState>>) -> Json<Settings> {
    Json(app.runtimes.auto.get())
}

pub async fn put(State(app): State<Arc<AppState>>, Json(mut req): Json<Settings>) -> Response {
    req.limit_prompt = if req.limit_action == "compact" { String::new() } else { req.limit_prompt.trim().to_owned() };
    if let Err(e) = req.check() {
        return fail(StatusCode::BAD_REQUEST, e);
    }
    if let Err(e) = app.runtimes.auto.set(req.clone()) {
        tracing::warn!("could not save the compaction settings: {e}");
        return fail(StatusCode::INTERNAL_SERVER_ERROR, format!("could not save: {e}"));
    }
    Json(req).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::{Entry, Window};

    #[test]
    fn counts_a_whole_idle_wait_and_fires_once() {
        let mut c = Clock::default();
        let wait = 50 * 60_000;
        assert!(!c.tick(true, 10 * wait, wait), "nothing settled yet: nothing to count from");
        c.settled(1_000);
        assert!(!c.tick(true, 1_000 + wait - 1, wait));
        // something is going on at minute 40 (a background task): the wait begins again from there
        assert!(!c.tick(false, 40 * 60_000, wait));
        assert!(!c.tick(true, 1_000 + wait, wait));
        assert!(!c.tick(true, 40 * 60_000 + wait - 1, wait));
        assert!(c.tick(true, 40 * 60_000 + wait, wait));
        assert!(!c.tick(true, 40 * 60_000 + 2 * wait, wait), "once per idle stretch");
        // a new run: the next stretch is counted again
        c.started();
        assert!(!c.tick(true, 9_000_000, wait));
        c.settled(10_000_000);
        assert!(c.tick(true, 10_000_000 + wait, wait));
        // not done (too small): the wait runs again from now, not every tick
        c.skipped(20_000_000);
        assert!(!c.tick(true, 20_000_000 + wait - 1, wait));
        assert!(c.tick(true, 20_000_000 + wait, wait));
    }

    fn entry(windows: &[(u32, f64, u64)], reached: bool) -> Entry {
        Entry { provider: "anthropic".into(), plan: None, at: 0, limit_reached: reached, windows: windows.iter().map(|&(minutes, used, r)| Window { minutes, used, reset_at: Some(r) }).collect(), resets: None, reset_kinds: vec![] }
    }

    #[test]
    fn the_window_with_least_left_decides_and_once_per_window() {
        let now = 1_000_000;
        let e = entry(&[(300, 50.0, now + 3_600_000), (10080, 88.0, now + 86_400_000)], false);
        let near = near_limit(&e, now, 15, &[]).unwrap();
        assert_eq!((near.minutes, near.left), (10080, 12.0));
        assert_eq!(near_limit(&e, now, 10, &[]), None, "12 % left is not under 10");
        // the same window again (its reset reported a few seconds off) is not acted on twice
        let done = vec![("anthropic".to_owned(), 10080, now + 86_400_000 + 5_000)];
        assert_eq!(near_limit(&e, now, 15, &done), None);
        // after that window reset, a new one counts
        let later = entry(&[(300, 50.0, now + 3_600_000), (10080, 90.0, now + 8 * 86_400_000)], false);
        assert!(near_limit(&later, now, 15, &done).is_some());
        // used up, or the provider said so: Zero-Waste and Baton Touch have it
        assert_eq!(near_limit(&entry(&[(300, 100.0, now + 1000)], false), now, 15, &[]), None);
        assert_eq!(near_limit(&entry(&[(300, 90.0, now + 1000)], true), now, 15, &[]), None);
        // a window that has reset already says nothing
        assert_eq!(near_limit(&entry(&[(300, 95.0, now - 1)], false), now, 15, &[]), None);
    }

    #[test]
    fn settings_are_checked_and_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("pi-oven-ac-{}", std::process::id()));
        let f = dir.join("auto-compact.json");
        let a = AutoCompact::load(f.clone());
        assert_eq!(a.get(), Settings::default());
        let mut s = Settings { idle_minutes: 30, limit_enabled: true, limit_action: "commit".into(), limit_prompt: "commit it".into(), ..Default::default() };
        a.set(s.clone()).unwrap();
        assert_eq!(AutoCompact::load(f.clone()).get(), s);
        s.limit_prompt = "  ".into();
        assert!(s.check().is_err(), "a prompt action needs its prompt");
        s.limit_action = "nap".into();
        assert!(s.check().is_err());
        // a settings file that cannot be written is not applied
        let blocked = AutoCompact::load(f.join("x/auto-compact.json"));
        std::fs::write(&f, "x").ok();
        assert!(blocked.set(s.clone()).is_err() && blocked.get() == Settings::default());
        a.set(s.clone()).unwrap();
        let bad = Settings { idle_minutes: 0, ..Default::default() };
        assert!(bad.check().is_err());
        assert!(Settings { limit_percent: 100, ..Default::default() }.check().is_err());
        assert!(Settings { model: Some("nomodel".into()), ..Default::default() }.check().is_err());
        assert!(Settings { model: Some("anthropic/claude-x".into()), ..Default::default() }.check().is_ok());
        std::fs::write(&f, "{ nope").unwrap();
        assert_eq!(AutoCompact::load(f).get(), Settings::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_work_still_going_beside_the_run() {
        let dir = std::env::temp_dir().join(format!("pi-oven-busy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cwd = dir.join("proj");
        let tasks = cwd.join(".pi/tasks/sid-4242");
        std::fs::create_dir_all(&tasks).unwrap();
        let ci = cwd.to_string_lossy().into_owned();
        assert!(!others_busy(&ci, 4242, 0));
        std::fs::write(tasks.join("a.json"), r#"{"status":"completed"}"#).unwrap();
        assert!(!others_busy(&ci, 4242, 0));
        std::fs::write(tasks.join("b.json"), r#"{"status":"running"}"#).unwrap();
        assert!(others_busy(&ci, 4242, 0));
        assert!(!others_busy(&ci, 7, 0), "another pi's tasks");
        // subagents: this session's, still running, recently touched
        let root = dir.join("sub");
        let run = root.join("async-subagent-runs/r1");
        std::fs::create_dir_all(&run).unwrap();
        let now = 1_000_000_000;
        let put = |state: &str, pid: u32, sess: &str, upd: u64| std::fs::write(run.join("status.json"), format!(r#"{{"state":"{state}","sessionId":"{sess}","completionOwnerProcess":{{"pid":{pid}}},"lastUpdate":{upd}}}"#)).unwrap();
        put("running", 4242, "/s/a.jsonl", now);
        assert!(subagents_busy(&dir, Some(&root), 4242, now));
        assert!(!subagents_busy(&dir, Some(&root), 1, now), "another pi's");
        put("complete", 4242, "/s/a.jsonl", now);
        assert!(!subagents_busy(&dir, Some(&root), 4242, now));
        put("running", 4242, "/s/a.jsonl", now - 11 * 60_000);
        assert!(!subagents_busy(&dir, Some(&root), 4242, now), "a runner that went quiet is not work");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
