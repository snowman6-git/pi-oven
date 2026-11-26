//! One `pi --mode rpc` child process per runtime. Stdout records are fanned out to every
//! attached WebSocket; commands from any client are serialized onto stdin.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::{broadcast, mpsc, watch, RwLock},
};

use crate::{
    config::{random_hex, Config},
    retry::{self, Event, Long, Step, Wait},
};

const STDERR_TAIL: usize = 40;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnRequest {
    pub cwd: PathBuf,
    pub session_path: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub thinking: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    pub id: String,
    pub cwd: String,
    pub session_path: Option<String>,
    pub pid: Option<u32>,
    pub started_at: u64,
}

/// What a late joiner needs to catch up with a run that is already in flight.
#[derive(Default)]
struct Replay {
    active: bool,
    run: Vec<Arc<str>>,
    /// Where in `run` each running tool's latest output update sits: an update carries the whole output so
    /// far, so only the newest one is worth keeping.
    tool_updates: HashMap<String, usize>,
    /// Latest setStatus / setWidget per key, so a reload keeps extension state.
    ui: HashMap<String, Arc<str>>,
    /// Dialogs nobody has answered yet.
    dialogs: Vec<(String, Arc<str>)>,
}

const TOOL_UPDATE: &str = r#"{"type":"tool_execution_update""#;
/// This many failed replies in a row, each within `HALT_GAP_MS` of the one before, stop the run
/// (`Runtime::reply_failed`). pi waits at least 2 s before its own retries, so they never count; the loop of 2026-11-22
/// failed every 0.6 s.
const HALT_FAILS: usize = 6;
const HALT_GAP_MS: u64 = 1_500;

/// A response to a command (`{"type":"response"…`, or `{"id":"…","type":"response"…` when the command had an id).
fn is_response(line: &str) -> bool {
    line.starts_with(r#"{"type":"response""#)
        || line.strip_prefix(r#"{"id":""#).and_then(|r| r.find('"').map(|e| r[e..].starts_with(r#"","type":"response""#))).unwrap_or(false)
}

fn tool_call_id(line: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Id {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
    }
    serde_json::from_str::<Id>(line).ok().map(|i| i.tool_call_id)
}

impl Replay {
    /// A late joiner loads the history (every finished message, tool results included) and then this: only what the
    /// history cannot show yet. A run can go on for hours: keeping all of it made a 12-hour run's replay 16 MB, which a
    /// phone could not take in before the socket gave up, so it reconnected and was sent it again, for ever.
    fn record(&mut self, line: &Arc<str>) {
        let is = |t: &str| line.starts_with(&format!(r#"{{"type":"{t}""#));
        if is("agent_start") {
            self.active = true;
            self.clear_run();
        }
        if self.active {
            if line.starts_with(TOOL_UPDATE) {
                match tool_call_id(line) {
                    Some(id) => match self.tool_updates.get(&id) {
                        Some(&i) => self.run[i] = line.clone(),
                        None => {
                            self.tool_updates.insert(id, self.run.len());
                            self.run.push(line.clone());
                        }
                    },
                    None => self.run.push(line.clone()),
                }
            } else if is("tool_execution_end") {
                // over: its result comes into the history with its message
                if let Some(id) = tool_call_id(line) {
                    let mine = |l: &Arc<str>| (l.starts_with(TOOL_UPDATE) || l.starts_with(r#"{"type":"tool_execution_start""#)) && tool_call_id(l).as_deref() == Some(id.as_str());
                    self.run.retain(|l| !mine(l));
                    self.reindex();
                }
            } else if is("queue_update") {
                // only the newest says what is waiting
                self.run.retain(|l| !l.starts_with(r#"{"type":"queue_update""#));
                self.run.push(line.clone());
                self.reindex();
            } else if is("auto_retry_end") {
                self.run.retain(|l| !l.starts_with(r#"{"type":"auto_retry_start""#));
                self.reindex();
            } else if is("compaction_end") {
                self.run.retain(|l| !l.starts_with(r#"{"type":"compaction_start""#));
                self.reindex();
            } else if is("message_end") {
                // the message is in the history now: of everything so far, keep only what still goes on
                self.run.retain(|l| {
                    ["agent_start", "queue_update", "auto_retry_start", "compaction_start", "tool_execution_start", "tool_execution_update"]
                        .iter()
                        .any(|t| l.starts_with(&format!(r#"{{"type":"{t}""#)))
                });
                self.reindex();
            } else if !is("extension_ui_request") && !is_response(line) {
                // (answers belong to the page that asked: a history answer is megabytes, and no late joiner needs it)
                self.run.push(line.clone());
            }
            if is("agent_settled") {
                self.active = false;
                self.clear_run();
            }
        }
        if line.starts_with(r#"{"type":"extension_ui_request""#) {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { return };
            let id = v["id"].as_str().unwrap_or_default().to_owned();
            match v["method"].as_str() {
                Some("setStatus") => self.keep(format!("s:{}", v["statusKey"].as_str().unwrap_or_default()), v["statusText"].is_null(), line),
                Some("setWidget") => self.keep(format!("w:{}", v["widgetKey"].as_str().unwrap_or_default()), v["widgetLines"].is_null(), line),
                Some("select" | "confirm" | "input" | "editor") => self.dialogs.push((id, line.clone())),
                _ => {}
            }
        }
    }

    fn clear_run(&mut self) {
        self.run.clear();
        self.tool_updates.clear();
    }

    fn reindex(&mut self) {
        self.tool_updates.clear();
        for (i, l) in self.run.iter().enumerate() {
            if l.starts_with(TOOL_UPDATE) {
                if let Some(id) = tool_call_id(l) {
                    self.tool_updates.insert(id, i);
                }
            }
        }
    }

    fn keep(&mut self, key: String, cleared: bool, line: &Arc<str>) {
        if cleared {
            self.ui.remove(&key);
        } else {
            self.ui.insert(key, line.clone());
        }
    }
}

pub struct Runtime {
    pub info: RuntimeInfo,
    replay: Mutex<Replay>,
    pub events: broadcast::Sender<Arc<str>>,
    stdin_tx: mpsc::Sender<String>,
    shutdown: watch::Sender<bool>,
    stderr_tail: Mutex<VecDeque<String>>,
    pet: Arc<crate::pet::PetHub>,
    /// Where this session is in the 100h mode's schedule.
    long: Mutex<Option<Wait>>,
    /// Waiting for a usage limit to reset ("Zero-Waste").
    resume: Mutex<Option<crate::resume::Pending>>,
    /// How many times in a row the continue message ran into the limit again.
    resume_tries: std::sync::atomic::AtomicU32,
    /// When our continue message was last sent (0: never).
    resume_fired_at: std::sync::atomic::AtomicU64,
    resume_cfg: Arc<crate::resume::Resume>,
    /// Plan-limit checks under way (a 429 that may be the plan: the numbers are being asked for).
    limit_checks: std::sync::atomic::AtomicU32,
    /// When a check last found that a 429 was the plan's limit (0: never).
    limit_found_at: std::sync::atomic::AtomicU64,
    /// The reply that just failed met the plan's limit: pi's automatic retry that follows is stopped as it starts (it
    /// would only meet the same limit, three more times).
    abort_next_retry: std::sync::atomic::AtomicBool,
    /// The provider and model of the last reply (what Baton Touch hands over from, and whether it is still on the
    /// model it handed over to).
    last_model: Mutex<Option<(String, String)>>,
    /// No run under way (pi's `set_model` waits for one to end).
    idle: std::sync::atomic::AtomicBool,
    /// The bundled extension is loaded: notes can be kept in the session (`/oven-note`).
    notes: bool,
    /// The last reply ended in an error (a continue message is only worth sending then).
    last_failed: std::sync::atomic::AtomicBool,
    /// Zero-Waste is waiting for this provider's plan to reset: its numbers are asked again before the continue goes
    /// (a guessed reset, or numbers hours old with nobody looking, sent it into the same limit).
    wait_for: Mutex<Option<String>>,
    usage: Arc<crate::usage::Usage>,
    limits: Arc<crate::limits::Limits>,
    pi_bin: String,
    /// The session file pi reported last (a new session has none at spawn, and pi can switch files).
    session_file: Mutex<Option<String>>,
    /// The replies so far, for the log (`logs::Replies`: the last prompt's size, the extension's look at the request).
    replies: Mutex<crate::logs::Replies>,
    /// pi holds messages to send later (`queue_update`: steering or follow-up).
    queued: std::sync::atomic::AtomicBool,
    /// When the replies that failed since the last one that got through came (ms).
    fails: Mutex<VecDeque<u64>>,
    accounts: Arc<crate::accounts::Accounts>,
    /// The provider of the last reply, failed or not (the session's provider before any reply got through).
    reply_provider: Mutex<Option<String>>,
    prefs: Arc<crate::model_prefs::ModelPrefs>,
    /// A `set_model` waiting for pi's answer: whether to remember it as picked (a page asked), and the level to give the
    /// model once pi has taken it (a new session's start).
    want_model: Mutex<Option<WantModel>>,
}

struct WantModel {
    provider: String,
    model: String,
    remember: bool,
    level: Option<String>,
}

impl Runtime {
    pub async fn send(&self, line: String) -> bool {
        self.stdin_tx.send(line).await.is_ok()
    }

    pub fn stderr_tail(&self) -> Vec<String> {
        self.stderr_tail.lock().unwrap().iter().cloned().collect()
    }

    /// Subscribe and snapshot under one lock, so no event is missed or seen twice.
    /// The snapshot is a single JSON frame: `{"type":"oven_replay","events":[...]}`.
    pub fn attach(&self) -> (broadcast::Receiver<Arc<str>>, Option<String>) {
        let r = self.replay.lock().unwrap();
        let rx = self.events.subscribe();
        let parts: Vec<&str> = r
            .ui
            .values()
            .map(|l| &**l)
            .chain(r.dialogs.iter().map(|(_, l)| &**l))
            .chain(r.run.iter().map(|l| &**l))
            .collect();
        let frame = (!parts.is_empty()).then(|| format!(r#"{{"type":"oven_replay","events":[{}]}}"#, parts.join(",")));
        (rx, frame)
    }

    /// A page picked a model: the session is on it from now (Baton Touch's way back leaves the person's choice alone).
    /// pi answered a `set_model`: only a switch it made is remembered, and only then is the model's level given.
    fn model_answered(&self, ok: bool) {
        let Some(w) = self.want_model.lock().unwrap().take() else { return };
        if !ok {
            return;
        }
        if w.remember {
            self.prefs.chosen(&w.provider, &w.model);
        }
        if let Some(level) = w.level {
            let _ = self.stdin_tx.try_send(serde_json::json!({ "type": "set_thinking_level", "level": level }).to_string());
        }
    }

    pub fn model_chosen(&self, provider: &str, model: &str) {
        *self.last_model.lock().unwrap() = Some((provider.to_owned(), model.to_owned()));
        *self.want_model.lock().unwrap() = Some(WantModel { provider: provider.to_owned(), model: model.to_owned(), remember: true, level: None });
    }

    /// A message the page is about to send would go to a plan that is used up (its numbers say 100 %, or the provider
    /// just refused for the limit and no numbers came since), and nothing would take over (Baton Touch to another
    /// account, or to another provider's model): what to tell the page instead, or `None` to send it.
    pub fn limit_hold(&self) -> Option<serde_json::Value> {
        let provider = match self.last_model.lock().unwrap().clone() {
            Some((p, _)) => p,
            None => self.reply_provider.lock().unwrap().clone()?,
        };
        let now = now_ms();
        let (reset, window) = self.limits.blocked(&provider, now)?;
        if self.accounts.other_free(&provider, now) || self.accounts.cross_target(&provider, &self.limits).is_some() {
            return None;
        }
        let v = serde_json::json!({ "provider": provider, "inMs": reset.map(|t| t.saturating_sub(now)), "window": window });
        crate::logs::event("hold", serde_json::json!({ "rt": self.info.id, "provider": provider, "resetAt": reset, "window": window }));
        Some(v)
    }

    pub fn dialog_answered(&self, id: &str) {
        self.pet.answered(&self.info.id);
        self.replay.lock().unwrap().dialogs.retain(|(d, _)| d != id);
    }

    pub fn close(&self) {
        let _ = self.shutdown.send(true);
    }

    pub fn resume_view(&self) -> Option<serde_json::Value> {
        let text = self.resume_cfg.get().text;
        self.resume.lock().unwrap().as_ref().map(|p| p.view(now_ms(), &text))
    }

    fn resume_publish(&self, gave_up: bool) {
        let frame = serde_json::json!({ "type": "oven_resume", "state": self.resume_view(), "gaveUp": gave_up });
        let _ = self.events.send(Arc::from(frame.to_string()));
    }

    /// pi gave up on a reply with a 429 that may be the plan's limit. Wait for the checks of that reply to finish (at
    /// most 30 s): if one found the plan used up, Zero-Waste has it and 100h mode stays out; otherwise it was a short
    /// rate limit after all, and 100h mode takes it as it takes any failure.
    async fn settle_held_failure(self: Arc<Self>, error: String, since: u64, long: Arc<Long>) {
        use std::sync::atomic::Ordering;
        let give_up = since + 30_000;
        while self.limit_checks.load(Ordering::SeqCst) > 0 && now_ms() < give_up {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        // a check of this failure's replies (they came a little before it) found the plan's limit
        if self.limit_found_at.load(Ordering::SeqCst) + 10 * 60_000 >= since {
            // the plan's limit: a wait for its reset, or Baton Touch's continue, is set up; if neither is (Baton Touch
            // switched a moment ago and these were pi's own retries), one goes once pi has settled
            self.resume_soon();
            return;
        }
        self.long_event(Event::Failed(error), &long);
    }

    /// Baton Touch handed over (to another account, or another provider's model): the continue message goes in a few
    /// seconds, not at the reset; `at`: the account taking over is free only then (it goes a few seconds after). The
    /// page hears of it (`oven_baton`), and the session keeps a note of it.
    async fn resume_baton(self: &Arc<Self>, at: Option<u64>, to: serde_json::Value) {
        use std::sync::atomic::Ordering;
        let tries = self.resume_tries.load(Ordering::Relaxed) + 1;
        if tries > crate::resume::MAX_TRIES {
            self.resume_tries.store(0, Ordering::Relaxed);
            self.resume_publish(true);
            return;
        }
        *self.wait_for.lock().unwrap() = None; // another account or model: nothing to wait for
        let at = at.unwrap_or_else(now_ms) + crate::resume::BATON_DELAY_MS;
        let pending = crate::resume::Pending { at, tries, window: None, known: true, held: vec![] };
        let begun = self.set_pending(pending);
        self.note(to).await;
        self.resume_publish(false);
        if begun {
            tokio::spawn(self.clone().resume_clock());
        }
    }

    /// A failure Baton Touch already handled (pi's retries ran out on it, or came back the same): make sure a continue
    /// is waiting, so the session does not stop there. It goes once pi has settled, and only if the run failed.
    fn resume_soon(self: &Arc<Self>) {
        // a continue just went (the same failure reported again by pi's last retry): that one is enough
        if now_ms().saturating_sub(self.resume_fired_at.load(std::sync::atomic::Ordering::Relaxed)) < 30_000 {
            return;
        }
        self.resume_after(crate::resume::BATON_DELAY_MS, true);
    }

    /// Continue in `ms` (once pi has settled, and only if the run failed), unless a wait is already set up.
    fn resume_after(self: &Arc<Self>, ms: u64, known: bool) {
        use std::sync::atomic::Ordering;
        if !self.resume_cfg.get().enabled || self.resume.lock().unwrap().is_some() {
            return;
        }
        let tries = self.resume_tries.load(Ordering::Relaxed) + 1;
        if tries > crate::resume::MAX_TRIES {
            self.resume_tries.store(0, Ordering::Relaxed);
            self.resume_publish(true);
            return;
        }
        crate::logs::event("resume", serde_json::json!({ "rt": self.info.id, "action": "wait", "at": now_ms() + ms, "known": known, "tries": tries }));
        let pending = crate::resume::Pending { at: now_ms() + ms, tries, window: None, known, held: vec![] };
        let begun = self.set_pending(pending);
        self.resume_publish(false);
        if begun {
            tokio::spawn(self.clone().resume_clock());
        }
    }

    /// pi's automatic retries of a reply that met the plan's limit: pointless while the server waits for the reset.
    async fn stop_retrying(&self) {
        self.cancel_retry("waiting for the limit");
    }

    /// Stop pi's automatic retry. With messages in pi's queue, the run too: pi 1.5 starts the run again at once to
    /// deliver them, the request fails the same way before they go in, and round it goes. With the retry's wait cut
    /// short that was two requests a second (100 in 71 s on the test server, 2026-11-22). The queue stays and goes with
    /// the next run.
    fn cancel_retry(&self, why: &str) {
        use std::sync::atomic::Ordering;
        let _ = self.stdin_tx.try_send(r#"{"type":"abort_retry"}"#.to_owned());
        let queued = self.queued.load(Ordering::Relaxed);
        if queued {
            let _ = self.stdin_tx.try_send(r#"{"type":"abort"}"#.to_owned());
        }
        crate::logs::event("retry", serde_json::json!({ "rt": self.info.id, "action": if queued { "abort_run" } else { "abort_retry" }, "why": why }));
    }

    /// A reply failed. Many in a row, each too soon after the last for pi's own retries (2, 4, 8 s apart): something
    /// keeps starting the request again. Stop the run, so a loop cannot burn through a plan or the account's standing
    /// with the provider.
    fn reply_failed(&self, now: u64, error: Option<&str>) {
        let n = {
            let mut f = self.fails.lock().unwrap();
            if f.back().is_some_and(|t| now.saturating_sub(*t) > HALT_GAP_MS) {
                f.clear();
            }
            f.push_back(now);
            if f.len() < HALT_FAILS {
                return;
            }
            let n = f.len();
            f.clear();
            n
        };
        let _ = self.stdin_tx.try_send(r#"{"type":"abort_retry"}"#.to_owned());
        let _ = self.stdin_tx.try_send(r#"{"type":"abort"}"#.to_owned());
        tracing::warn!(runtime = %self.info.id, "{n} failed replies back to back: run stopped");
        crate::logs::event("halt", serde_json::json!({ "rt": self.info.id, "fails": n, "error": error.map(crate::logs::clip) }));
        let frame = serde_json::json!({ "type": "oven_halt", "fails": n });
        let _ = self.events.send(Arc::from(frame.to_string()));
    }

    /// Tell the page what Baton Touch did (`oven_baton`), and keep it in the session when the extension is there.
    async fn note(&self, to: serde_json::Value) {
        let mut frame = to.clone();
        frame["type"] = "oven_baton".into();
        frame["noted"] = self.notes.into();
        let _ = self.events.send(Arc::from(frame.to_string()));
        if self.notes {
            let cmd = serde_json::json!({ "type": "prompt", "message": format!("/oven-note {to}") });
            let _ = self.stdin_tx.send(cmd.to_string()).await;
        }
    }

    /// A `Later` hand-over: at `at`, if Zero-Waste is still waiting, switch to the account.
    #[allow(clippy::too_many_arguments)]
    async fn take_over_at(self: Arc<Self>, at: u64, id: String, was: Option<String>, accounts: Arc<crate::accounts::Accounts>, limits: Arc<crate::limits::Limits>, usage: Arc<crate::usage::Usage>, pi_bin: String) {
        let mut at = at;
        loop {
            tokio::time::sleep(Duration::from_millis(at.saturating_sub(now_ms()))).await;
            if *self.shutdown.borrow() || self.resume.lock().unwrap().is_none() {
                return;
            }
            // its numbers now: still used up (its reset was guessed, or moved), so wait for the real one
            match accounts.free_at(&pi_bin, &id).await {
                Some(t) if t > now_ms() => {
                    at = t + 5_000;
                    if let Some(p) = self.resume.lock().unwrap().as_mut() {
                        p.at = at + crate::resume::BATON_DELAY_MS;
                    }
                    tracing::info!("Baton Touch: the account taking over is not free yet; waiting for its reset");
                    self.resume_publish(false);
                }
                _ => break,
            }
        }
        if let Some(a) = accounts.take_over(&pi_bin, &id, &limits).await {
            accounts.announce(Some(&self.info.id), &a);
            let _ = usage.poll(&pi_bin, &limits, true, Some(&a.provider)).await;
            self.note(serde_json::json!({ "provider": a.provider, "email": a.email, "plan": a.plan, "was": was })).await;
        }
    }

    /// After a hand-over to another provider's model (`to`): once the plan it came from has reset, and while the
    /// session is still on `to`, go back to `model` of `provider`. Checks the numbers first; tries for a day.
    #[allow(clippy::too_many_arguments)]
    async fn go_home(self: Arc<Self>, provider: String, model: String, to: crate::accounts::Target, reset: Option<u64>, limits: Arc<crate::limits::Limits>, usage: Arc<crate::usage::Usage>, pi_bin: String, accounts: Arc<crate::accounts::Accounts>) {
        use std::sync::atomic::Ordering;
        let give_up = now_ms() + 24 * 3_600_000;
        let mut wake = reset.or_else(|| limits.used_up_until(&provider, now_ms(), 95.0)).unwrap_or(now_ms() + 3_600_000).max(now_ms() + 60_000);
        loop {
            tokio::time::sleep(Duration::from_millis(wake.saturating_sub(now_ms()))).await;
            if *self.shutdown.borrow() || now_ms() > give_up || !accounts.return_home() {
                return;
            }
            // the person (or something else) changed the model since: theirs to keep
            let on = self.last_model.lock().unwrap().clone();
            if on.as_ref().is_some_and(|(p, m)| *p != to.provider || *m != to.model) {
                return;
            }
            let _ = usage.poll(&pi_bin, &limits, true, Some(&provider)).await;
            let now = now_ms();
            if let Some(t) = limits.used_up_until(&provider, now, 95.0) {
                wake = t + 30_000;
                continue;
            }
            if !self.idle.load(Ordering::Relaxed) || self.resume.lock().unwrap().is_some() {
                wake = now + 60_000;
                continue;
            }
            let cmd = serde_json::json!({ "type": "set_model", "provider": provider, "modelId": model });
            if self.stdin_tx.send(cmd.to_string()).await.is_ok() {
                *self.last_model.lock().unwrap() = Some((provider.clone(), model.clone()));
                self.note(serde_json::json!({ "provider": provider, "model": model, "back": true, "was": to.model })).await;
            }
            return;
        }
    }

    /// A run ended on the usage limit: wait for the reset, then send the chosen message.
    fn resume_begin(self: &Arc<Self>, error: &str, culprit: Option<(u64, u32)>) {
        use std::sync::atomic::Ordering;
        if !self.resume_cfg.get().enabled {
            return;
        }
        let tries = self.resume_tries.load(Ordering::Relaxed) + 1;
        if tries > crate::resume::MAX_TRIES {
            self.resume_tries.store(0, Ordering::Relaxed);
            self.resume_publish(true);
            return;
        }
        let now = now_ms();
        let fired = self.resume_fired_at.load(Ordering::Relaxed);
        let (at, window, known) = if tries > 1 && fired > 0 && now.saturating_sub(fired) < crate::resume::EARLY_WINDOW_MS {
            (now + crate::resume::EARLY_RETRY_MS, culprit.map(|c| c.1), true)
        } else {
            crate::resume::due(error, culprit, now)
        };
        crate::logs::event("resume", serde_json::json!({ "rt": self.info.id, "action": "wait", "at": at, "window": window, "known": known, "tries": tries }));
        let pending = crate::resume::Pending { at, tries, window, known, held: vec![] };
        let begun = self.set_pending(pending);
        self.resume_publish(false);
        if begun {
            tokio::spawn(self.clone().resume_clock());
        }
    }

    /// Put a wait in place; messages held back by the one before stay with it. Returns whether none was running.
    fn set_pending(&self, mut p: crate::resume::Pending) -> bool {
        let mut g = self.resume.lock().unwrap();
        if let Some(old) = g.take() {
            let mut held = old.held;
            held.append(&mut p.held);
            p.held = held;
            *g = Some(p);
            return false;
        }
        *g = Some(p);
        true
    }

    /// A message from the page while the plan it would go to is used up (`limit_hold`): Zero-Waste keeps it and sends
    /// it when the limit resets (checking the numbers first), in place of the continue message. "Now" on the strip sends
    /// it at once. Returns whether it was held (Zero-Waste is on); otherwise it goes to pi as usual.
    pub fn hold(self: &Arc<Self>, mut prompt: serde_json::Value, held: &serde_json::Value) -> bool {
        if !self.resume_cfg.get().enabled {
            return false;
        }
        if let Some(o) = prompt.as_object_mut() {
            o.remove("id");
            o.remove("streamingBehavior");
        }
        let now = now_ms();
        let reset = held["inMs"].as_u64().map(|ms| now + ms);
        let window = held["window"].as_u64().map(|w| w as u32);
        // a known reset: then (and the numbers are asked again first); a refusal with no numbers: ask again soon
        let (at, known) = match reset {
            Some(t) => (t + 10_000, true),
            None => (now + crate::resume::RATE_LIMIT_WAIT_MS, false),
        };
        *self.wait_for.lock().unwrap() = held["provider"].as_str().map(str::to_owned);
        let tries = self.resume_tries.load(std::sync::atomic::Ordering::Relaxed).max(1);
        crate::logs::event("resume", serde_json::json!({ "rt": self.info.id, "action": "hold", "at": at, "window": window, "known": known }));
        let begun = {
            let mut g = self.resume.lock().unwrap();
            match g.as_mut() {
                // already waiting (a failed run's continue, or held messages): add this one, keep the time
                Some(p) => {
                    p.held.push(prompt);
                    false
                }
                None => {
                    *g = Some(crate::resume::Pending { at, tries, window, known, held: vec![prompt] });
                    true
                }
            }
        };
        self.resume_publish(false);
        if begun {
            tokio::spawn(self.clone().resume_clock());
        }
        true
    }

    /// pi started a run. Ours takes the wait away before it sends, so a wait still here means the user sent something.
    fn resume_on_start(&self) {
        // (messages the person sent and Zero-Waste holds stay held: they still wait for the limit)
        let taken = {
            let mut g = self.resume.lock().unwrap();
            if g.as_ref().is_some_and(|p| p.held.is_empty()) { g.take() } else { None }
        };
        if taken.is_some() {
            self.resume_tries.store(0, std::sync::atomic::Ordering::Relaxed);
            self.resume_publish(false);
        }
    }

    /// A run that ends with nothing waiting got through: the count starts again.
    fn resume_on_settled(&self) {
        // (a run that failed has not got through: its limit check may still be under way, and the count must hold
        // until our continue message gets through, or ten in a row stop it)
        if self.resume.lock().unwrap().is_none() && !self.last_failed.load(std::sync::atomic::Ordering::Relaxed) {
            self.resume_tries.store(0, std::sync::atomic::Ordering::Relaxed);
        }
    }

    async fn resume_clock(self: Arc<Self>) {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if *self.shutdown.borrow() {
                return;
            }
            let due = {
                let mut g = self.resume.lock().unwrap();
                let Some(p) = g.as_ref() else { return };
                if !self.resume_cfg.get().enabled {
                    *g = None;
                    drop(g);
                    self.resume_publish(false);
                    return;
                }
                // pi still going (its own retries, after Baton Touch put another account in): wait for it to settle
                now_ms() >= p.at && self.idle.load(std::sync::atomic::Ordering::Relaxed)
            };
            if !due {
                continue;
            }
            // the plan it waited for: ask its numbers now, and if a window is still used up, wait for that reset instead
            let waited = self.wait_for.lock().unwrap().clone();
            if let Some(p) = waited {
                let _ = self.usage.poll(&self.pi_bin, &self.limits, true, Some(&p)).await;
                if let Some(t) = self.limits.used_up_until(&p, now_ms(), 95.0) {
                    if let Some(w) = self.resume.lock().unwrap().as_mut() {
                        w.at = t + 15_000;
                        w.known = true;
                    }
                    tracing::info!("Zero-Waste: {p} is still used up; waiting for its reset");
                    crate::logs::event("resume", serde_json::json!({ "rt": self.info.id, "action": "still_used_up", "provider": p, "at": t + 15_000 }));
                    self.resume_publish(false);
                    continue;
                }
            }
            let held = {
                let mut g = self.resume.lock().unwrap();
                match g.take() {
                    Some(p) => {
                        self.resume_tries.store(p.tries, std::sync::atomic::Ordering::Relaxed);
                        self.resume_fired_at.store(now_ms(), std::sync::atomic::Ordering::Relaxed);
                        p.held
                    }
                    None => return,
                }
            };
            // the messages held back go in place of the continue message: the first starts the run, the rest follow
            if !held.is_empty() {
                *self.wait_for.lock().unwrap() = None;
                crate::logs::event("resume", serde_json::json!({ "rt": self.info.id, "action": "send_held", "n": held.len() }));
                for (i, mut m) in held.into_iter().enumerate() {
                    if i > 0 {
                        m["streamingBehavior"] = "followUp".into();
                    }
                    let _ = self.stdin_tx.send(m.to_string()).await;
                }
                self.resume_publish(false);
                return;
            }
            {
                *self.wait_for.lock().unwrap() = None;
                // the run got through after all (pi's retry, on the new account): nothing to continue
                let failed = self.last_failed.load(std::sync::atomic::Ordering::Relaxed);
                crate::logs::event("resume", serde_json::json!({ "rt": self.info.id, "action": if failed { "send" } else { "not_needed" } }));
                if failed {
                    let text = self.resume_cfg.get().text;
                    let _ = self.stdin_tx.send(serde_json::json!({ "type": "prompt", "message": text }).to_string()).await;
                }
                self.resume_publish(false);
                return;
            }
        }
    }

    pub fn resume_now(&self) {
        // the person said now: no second look at the numbers
        *self.wait_for.lock().unwrap() = None;
        if let Some(p) = self.resume.lock().unwrap().as_mut() {
            p.at = now_ms();
        }
    }

    pub fn resume_stop(&self) {
        let Some(p) = self.resume.lock().unwrap().take() else { return };
        self.resume_tries.store(0, std::sync::atomic::Ordering::Relaxed);
        self.resume_publish(false);
        // messages it held are not lost: the page puts them back in the message box
        let texts: Vec<&str> = p.held.iter().filter_map(|h| h["message"].as_str()).collect();
        if !texts.is_empty() {
            let frame = serde_json::json!({ "type": "oven_unheld", "text": texts.join("\n\n") });
            let _ = self.events.send(Arc::from(frame.to_string()));
        }
    }

    pub fn long_view(&self) -> Option<serde_json::Value> {
        self.long.lock().unwrap().as_ref().map(|w| w.view(now_ms()))
    }

    fn long_publish(&self, gave_up: bool) {
        let frame = serde_json::json!({ "type": "oven_long", "state": self.long_view(), "gaveUp": gave_up });
        let _ = self.events.send(Arc::from(frame.to_string()));
    }

    fn long_event(self: &Arc<Self>, event: Event, long: &Arc<Long>) {
        let (step, begun) = {
            let mut g = self.long.lock().unwrap();
            let idle = g.is_none();
            let step = retry::advance(&mut g, event, long.enabled(), now_ms());
            (step, idle && g.is_some())
        };
        if step != Step::Same {
            let state = self.long_view();
            crate::logs::event("100h", serde_json::json!({ "rt": self.info.id, "gaveUp": step == Step::GaveUp, "state": state }));
            self.long_publish(step == Step::GaveUp);
        }
        if begun {
            tokio::spawn(self.clone().long_clock(long.clone()));
        }
    }

    /// Runs while a wait exists. A tick rather than one long sleep, so "try now", "stop" and the switch need no wake-up
    /// call and a machine that slept is caught up with.
    async fn long_clock(self: Arc<Self>, long: Arc<Long>) {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if *self.shutdown.borrow() {
                return;
            }
            let (ended, due) = {
                let mut g = self.long.lock().unwrap();
                let Some(w) = g.as_mut() else { return };
                let waiting = w.next_at.is_some();
                if waiting && !long.enabled() {
                    *g = None;
                    (true, false)
                } else if w.next_at.is_some_and(|t| now_ms() >= t) {
                    w.next_at = None;
                    (false, true)
                } else {
                    (false, false)
                }
            };
            if due {
                let _ = self.stdin_tx.send(retry::TRY_COMMAND.to_owned()).await;
            }
            if ended || due {
                self.long_publish(false);
            }
            if ended {
                return;
            }
        }
    }

    pub fn long_try_now(&self, now: u64) {
        if let Some(w) = self.long.lock().unwrap().as_mut() {
            if w.next_at.is_some() {
                w.next_at = Some(now);
            }
        }
    }

    pub fn long_stop(&self) {
        let had = self.long.lock().unwrap().take().is_some();
        if had {
            self.long_publish(false);
        }
    }
}

pub struct Manager {
    cfg: Arc<Config>,
    map: RwLock<HashMap<String, Arc<Runtime>>>,
    /// Held for a whole spawn: two requests for the same session at once must not both start pi.
    spawning: tokio::sync::Mutex<()>,
    /// What the desktop pet follows: pi's events, translated.
    pub pet: Arc<crate::pet::PetHub>,
    /// What pi-updater last offered (its prompt is answered here, not shown in every new session).
    pub updates: Arc<crate::update::Updates>,
    pub long: Arc<Long>,
    pub limits: Arc<crate::limits::Limits>,
    pub resume: Arc<crate::resume::Resume>,
    pub usage: Arc<crate::usage::Usage>,
    pub cache_ttl: crate::cache_ttl::CacheTtl,
    pub lens: crate::lens::Lens,
    /// The model a new session starts on, and each model's starting level (`model_prefs.rs`).
    pub prefs: Arc<crate::model_prefs::ModelPrefs>,
    pub accounts: Arc<crate::accounts::Accounts>,
}

/// The frame that says pi has ended. Written out by hand: the restart and the WebSocket loop find it by its
/// `{"type":"oven_exit"` prefix, and `json!` would put "code" first (keys come out sorted).
fn exit_frame(code: Option<i32>) -> String {
    format!(r#"{{"type":"oven_exit","code":{}}}"#, code.map_or_else(|| "null".to_owned(), |c| c.to_string()))
}

/// `sessionFile` from pi's answer to `get_state`.
fn state_session_file(line: &str) -> Option<String> {
    // pi puts the request's `id` first when there is one
    if !line.contains(r#""type":"response""#) || !line.contains(r#""command":"get_state""#) {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    v["data"]["sessionFile"].as_str().map(str::to_owned)
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Manager {
    pub fn new(cfg: Arc<Config>) -> Arc<Self> {
        let long = Arc::new(Long::load(cfg.retry_extension_path.as_ref()));
        let limits = Arc::new(crate::limits::Limits::load(cfg.limits_extension_path.as_ref()));
        let resume = Arc::new(crate::resume::Resume::load(cfg.limits_extension_path.as_deref().and_then(std::path::Path::parent)));
        Arc::new(Self { cfg, map: RwLock::new(HashMap::new()), spawning: Default::default(), pet: crate::pet::PetHub::new(), updates: Default::default(), long, limits, resume, usage: Default::default(), cache_ttl: Default::default(), lens: Default::default(), prefs: Default::default(), accounts: Default::default() })
    }

    pub async fn get(&self, id: &str) -> Option<Arc<Runtime>> {
        self.map.read().await.get(id).cloned()
    }

    /// models.json was changed here: every pi reads it again (it reads it only when it starts), with the bundled
    /// extension's `/oven-models`. Without the extension the command would go to the model as text, so nothing is sent.
    pub async fn models_changed(&self) {
        if !self.long.available() {
            return;
        }
        let all: Vec<Arc<Runtime>> = self.map.read().await.values().cloned().collect();
        for rt in all {
            rt.send(crate::retry::MODELS_COMMAND.to_owned()).await;
        }
    }

    pub async fn list(&self) -> Vec<RuntimeInfo> {
        let mut v: Vec<_> = self.map.read().await.values().map(|r| r.info.clone()).collect();
        v.sort_by_key(|i| i.started_at);
        v
    }

    pub async fn close(&self, id: &str) -> bool {
        match self.map.write().await.remove(id) {
            Some(rt) => {
                rt.close();
                true
            }
            None => false,
        }
    }

    /// The runtimes that have this session file open (by what pi last reported, or what they were opened with).
    /// The runtimes whose session file is `file` (resolved by `resolve_path`).
    pub async fn using_file(&self, file: &std::path::Path) -> Vec<String> {
        let same = |p: &str| resolve_path(std::path::Path::new(p)).is_some_and(|p| p == file);
        self.map
            .read()
            .await
            .values()
            .filter(|r| {
                let known = r.session_file.lock().unwrap().clone();
                known.as_deref().is_some_and(same) || r.info.session_path.as_deref().is_some_and(same)
            })
            .map(|r| r.info.id.clone())
            .collect()
    }

    /// Stop a runtime and wait (a few seconds at most) for pi to exit, so its session file is no longer written.
    pub async fn close_and_wait(&self, id: &str) {
        let Some(rt) = self.get(id).await else { return };
        let mut rx = rt.events.subscribe();
        self.close(id).await;
        let _ = tokio::time::timeout(Duration::from_secs(8), async {
            while let Ok(line) = rx.recv().await {
                if line.starts_with(r#"{"type":"oven_exit""#) {
                    break;
                }
            }
        })
        .await;
    }

    /// Stop a runtime, wait for pi to exit (so the session file is released), start it again.
    pub async fn restart(self: &Arc<Self>, id: &str, session_path: Option<String>) -> Result<Arc<Runtime>> {
        let old = self.get(id).await.context("no such runtime")?;
        let mut rx = old.events.subscribe();
        let cwd = PathBuf::from(&old.info.cwd);
        self.close(id).await;
        let _ = tokio::time::timeout(Duration::from_secs(8), async {
            while let Ok(line) = rx.recv().await {
                if line.starts_with(r#"{"type":"oven_exit""#) {
                    break;
                }
            }
        })
        .await;
        let session_path = session_path.or_else(|| old.info.session_path.clone());
        self.spawn(SpawnRequest { cwd, session_path, provider: None, model: None, thinking: None }).await
    }

    pub async fn close_all(&self) {
        let all: Vec<_> = self.map.write().await.drain().map(|(_, r)| r).collect();
        for rt in &all {
            rt.close();
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }

    pub async fn spawn(self: &Arc<Self>, req: SpawnRequest) -> Result<Arc<Runtime>> {
        let _one_at_a_time = self.spawning.lock().await;
        // Opening the same session twice would give two processes writing one file.
        if let Some(path) = &req.session_path {
            let map = self.map.read().await;
            if let Some(rt) = map.values().find(|r| r.info.session_path.as_deref() == Some(path)) {
                return Ok(rt.clone());
            }
        }

        let cwd = req.cwd.canonicalize().with_context(|| format!("folder not found: {}", req.cwd.display()))?;
        if !cwd.is_dir() {
            anyhow::bail!("not a folder: {}", cwd.display());
        }
        if !crate::scope::allows(self.cfg.root.as_deref(), &cwd) {
            anyhow::bail!("outside the allowed folder: {}", cwd.display());
        }
        if let Some(p) = &req.session_path {
            if !crate::scope::allows_session(self.cfg.root.as_deref(), std::path::Path::new(p)) {
                anyhow::bail!("session outside the allowed folder");
            }
        }

        let mut cmd = Command::new(&self.cfg.pi_bin);
        cmd.arg("--mode").arg("rpc");
        if let Some(p) = &req.session_path {
            cmd.arg("--session").arg(p);
        }
        if let (Some(provider), Some(model)) = (&req.provider, &req.model) {
            cmd.arg("--provider").arg(provider).arg("--model").arg(model);
        }
        if let Some(t) = &req.thinking {
            cmd.arg("--thinking").arg(t);
        }
        for ext in [&self.cfg.retry_extension_path, &self.cfg.limits_extension_path].into_iter().flatten() {
            cmd.arg("--extension").arg(ext);
        }
        if let Some(ext) = &self.cfg.extension_path {
            cmd.arg("--extension").arg(ext);
            // the chosen permission mode lives next to the extension, so a new pi starts in it (not in "auto"
            // until a browser happens to connect and say otherwise)
            if let Some(dir) = ext.parent() {
                cmd.env("PI_OVEN_PERMISSION_FILE", dir.join("permission"));
            }
        }
        // Claude's cache for an hour (see cache_ttl.rs); pi reads it when it starts
        if let Some(v) = self.lens.for_pi() {
            cmd.env(crate::lens::VAR, v); // pi-lens leaves the request alone (see lens.rs)
        }
        if let Some(v) = self.cache_ttl.for_pi() {
            cmd.env(crate::cache_ttl::VAR, v);
        }
        cmd.args(&self.cfg.pi_args)
            .current_dir(&cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .with_context(|| format!("could not start `{}` — is pi installed and on PATH?", self.cfg.pi_bin))?;
        let stdin = child.stdin.take().context("no stdin")?;
        let stdout = child.stdout.take().context("no stdout")?;
        let stderr = child.stderr.take().context("no stderr")?;

        let (events, _) = broadcast::channel::<Arc<str>>(4096);
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(256);
        let (shutdown, mut shutdown_rx_w) = watch::channel(false);
        let mut shutdown_rx_c = shutdown.subscribe();

        let id = random_hex(6);
        let info = RuntimeInfo {
            id: id.clone(),
            cwd: cwd.to_string_lossy().into_owned(),
            session_path: req.session_path.clone(),
            pid: child.id(),
            started_at: now_ms(),
        };
        let rt = Arc::new(Runtime {
            info,
            replay: Mutex::new(Replay::default()),
            events: events.clone(),
            stdin_tx,
            shutdown,
            stderr_tail: Mutex::new(VecDeque::new()),
            pet: self.pet.clone(),
            long: Mutex::new(None),
            session_file: Mutex::new(None),
            resume: Mutex::new(None),
            resume_tries: Default::default(),
            resume_fired_at: Default::default(),
            resume_cfg: self.resume.clone(),
            limit_checks: Default::default(),
            limit_found_at: Default::default(),
            abort_next_retry: Default::default(),
            last_model: Mutex::new(None),
            idle: std::sync::atomic::AtomicBool::new(true),
            notes: self.long.available(),
            last_failed: Default::default(),
            wait_for: Mutex::new(None),
            usage: self.usage.clone(),
            limits: self.limits.clone(),
            pi_bin: self.cfg.pi_bin.clone(),
            replies: Default::default(),
            queued: Default::default(),
            fails: Default::default(),
            accounts: self.accounts.clone(),
            reply_provider: Mutex::new(None),
            prefs: self.prefs.clone(),
            want_model: Mutex::new(None),
        });
        self.pet.attach(rt.clone());
        // a new session starts on the model last picked here; one that exists stays on its own (model_prefs.rs)
        if req.session_path.is_none() && req.provider.is_none() {
            if let Some((provider, model, level)) = self.prefs.for_new_session(&self.usage.recommended_now()) {
                let cmd = serde_json::json!({ "type": "set_model", "provider": provider, "modelId": model });
                *rt.want_model.lock().unwrap() = Some(WantModel { provider, model, remember: false, level });
                let _ = rt.stdin_tx.try_send(cmd.to_string());
            }
        }

        // the account in use is one for every session: tell this one's page when another session or Settings changes it
        {
            let rt = rt.clone();
            let mut changes = self.accounts.changes();
            let mut stop = rt.shutdown.subscribe();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = stop.changed() => return,
                        got = changes.recv() => match got {
                            Ok(frame) => {
                                if !frame.contains(&format!(r#""origin":"{}""#, rt.info.id)) {
                                    let _ = rt.events.send(frame);
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                            Err(_) => return,
                        },
                    }
                }
            });
        }

        // stdout → replay buffer + broadcast. Split on LF only: U+2028/2029 are legal inside JSON strings.
        {
            let rt = rt.clone();
            let updates = self.updates.clone();
            let long = self.long.clone();
            let limits = self.limits.clone();
            let usage = self.usage.clone();
            let pi_bin = self.cfg.pi_bin.clone();
            let accounts = self.accounts.clone();
            tokio::spawn(async move {
                let mut rd = BufReader::new(stdout);
                let mut buf = Vec::new();
                loop {
                    // one big line (a long history) would keep its size for the session's life
                    if buf.capacity() > 1 << 20 {
                        buf = Vec::new();
                    }
                    buf.clear();
                    match rd.read_until(b'\n', &mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            while matches!(buf.last(), Some(b'\n' | b'\r')) {
                                buf.pop();
                            }
                            if buf.is_empty() {
                                continue;
                            }
                            let text = String::from_utf8_lossy(&buf);
                            let line: Arc<str> = match slim_update(&text) {
                                Some(slim) => Arc::from(slim),
                                None => Arc::from(text.as_ref()),
                            };
                            // pi-updater 0.8's notice: the update card has it (not a banner in every session)
                            if line.starts_with(r#"{"type":"extension_ui_request""#) && line.contains(r#""method":"notify""#) {
                                if let Some(offer) = crate::update::from_notify(&line) {
                                    updates.offered(offer, now_ms());
                                    let _ = rt.events.send(Arc::from(r#"{"type":"oven_update"}"#));
                                    continue;
                                }
                            }
                            if line.starts_with(r#"{"type":"extension_ui_request""#) && line.contains(r#""method":"select""#) {
                                if let Some((ask, offer)) = crate::update::from_prompt(&line) {
                                    updates.offered(offer, now_ms());
                                    let answer = serde_json::json!({ "type": "extension_ui_response", "id": ask, "cancelled": true });
                                    let _ = rt.stdin_tx.try_send(answer.to_string());
                                    let _ = rt.events.send(Arc::from(r#"{"type":"oven_update"}"#));
                                    continue;
                                }
                            }
                            if line.starts_with(r#"{"type":"message_start""#) {
                                rt.abort_next_retry.store(false, std::sync::atomic::Ordering::Relaxed);
                            } else if line.starts_with(r#"{"type":"auto_retry_start""#) && rt.abort_next_retry.swap(false, std::sync::atomic::Ordering::Relaxed) {
                                // pi set its retry up a moment before saying so: the abort finds it waiting. The page
                                // never sees it ("retry 1/3" for a limit Zero-Waste already has)
                                rt.cancel_retry("limit");
                                continue;
                            }
                            if let Some(f) = state_session_file(&line) {
                                *rt.session_file.lock().unwrap() = Some(f);
                            }
                            if line.contains(r#""command":"set_model""#) && is_response(&line) {
                                rt.model_answered(line.contains(r#""success":true"#));
                            }
                            // the session's provider before any reply (a message held for a used-up plan needs it)
                            if line.contains(r#""command":"get_state""#) && rt.reply_provider.lock().unwrap().is_none() {
                                if let Some(p) = serde_json::from_str::<serde_json::Value>(&line).ok().and_then(|v| v["data"]["model"]["provider"].as_str().map(str::to_owned)) {
                                    *rt.reply_provider.lock().unwrap() = Some(p);
                                }
                            }
                            // the extension's look at the request about to go: for the log only, not the page
                            if let Some(t) = crate::logs::trace_of(&line) {
                                rt.replies.lock().unwrap().request = Some(t);
                                continue;
                            }
                            if line.starts_with(r#"{"type":"queue_update""#) {
                                let waiting = !line.contains(r#""steering":[]"#) || !line.contains(r#""followUp":[]"#);
                                rt.queued.store(waiting, std::sync::atomic::Ordering::Relaxed);
                            }
                            if let Some(r) = crate::logs::reply_of(&line) {
                                let now = now_ms();
                                if let Some(p) = &r.provider {
                                    *rt.reply_provider.lock().unwrap() = Some(p.clone());
                                }
                                if rt.replies.lock().unwrap().record(&rt.info.id, &r, now) {
                                    tracing::warn!(runtime = %rt.info.id, "prompt cache broke: {} read from the cache, {} written again", r.read, r.write);
                                }
                                match r.stop.as_deref() {
                                    Some("error") => rt.reply_failed(now, r.error.as_deref()),
                                    Some("aborted") => {}
                                    _ => rt.fails.lock().unwrap().clear(),
                                }
                            }
                            if let Some(e) = crate::limits::read(&line) {
                                limits.record_reply(e);
                            }
                            // The page hears of a failure first, so the first one still gets its notice; a run's end
                            // comes after the wait is settled, so the page knows whether it was a real end.
                            let mut event = retry::read(&line);
                            let mut held = None;
                            match &event {
                                Some(Event::Started) => {
                                    rt.idle.store(false, std::sync::atomic::Ordering::Relaxed);
                                    rt.resume_on_start()
                                }
                                Some(Event::Settled) => {
                                    rt.idle.store(true, std::sync::atomic::Ordering::Relaxed);
                                    rt.resume_on_settled()
                                }
                                // a usage limit resets in hours: "Zero-Waste" waits for it, not 100h mode
                                Some(Event::Failed(e)) if crate::resume::is_usage_limit(e) && rt.resume_cfg.get().enabled => event = None,
                                // a 429 that may be the plan's limit: 100h mode only gets it once the numbers say it was
                                // not (the check started on the reply that failed, a line earlier)
                                Some(Event::Failed(e)) if crate::resume::maybe_plan_limit(e) && rt.resume_cfg.get().enabled => {
                                    held = Some(e.clone());
                                    event = None;
                                }
                                _ => {}
                            }
                            if let Some(e) = held {
                                let (rt, long) = (rt.clone(), long.clone());
                                let since = now_ms();
                                tokio::spawn(async move { rt.settle_held_failure(e, since, long).await });
                            }
                            let limit = crate::resume::limit_error(&line);
                            let model = crate::resume::model_of(&line);
                            // a reply that got through says which model the session is on; a failed one may be a retry
                            // of the model just handed over from
                            if model.is_some() {
                                let failed = line.contains(r#""stopReason":"error""#);
                                rt.last_failed.store(failed, std::sync::atomic::Ordering::Relaxed);
                                if let Some(m) = model.as_ref().filter(|_| limit.is_none() && !failed) {
                                    *rt.last_model.lock().unwrap() = Some(m.clone());
                                }
                            }
                            let failed = matches!(event, Some(Event::Failed(_)));
                            let event = match event {
                                Some(e) if !failed => {
                                    rt.long_event(e, &long);
                                    None
                                }
                                e => e,
                            };
                            {
                                let mut replay = rt.replay.lock().unwrap();
                                replay.record(&line);
                                let _ = rt.events.send(line);
                            }
                            if let Some(e) = event {
                                rt.long_event(e, &long);
                            }
                            if let Some((error, provider, sure)) = limit {
                                // Claude's 429 is its plan (or a rate limit Zero-Waste waits out better than pi's quick
                                // retries do); a sure limit too: no retry of pi's then
                                if rt.resume_cfg.get().enabled && (sure == crate::resume::Limit::Sure || crate::resume::always_plan(provider.as_deref())) {
                                    rt.abort_next_retry.store(true, std::sync::atomic::Ordering::Relaxed);
                                }
                                // ask the provider for the numbers as they are now, then decide; without them, the last reply's
                                let (rt, limits, usage, pi_bin, accounts) = (rt.clone(), limits.clone(), usage.clone(), pi_bin.clone(), accounts.clone());
                                let failed_model = model.clone();
                                rt.limit_checks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                tokio::spawn(async move {
                                    let fresh = provider.is_some() && usage.poll(&pi_bin, &limits, true, provider.as_deref()).await;
                                    let now = now_ms();
                                    // a 429 that may be the plan: only a window the numbers show as (nearly) used up makes it one;
                                    // otherwise it was a short rate limit, which pi retries by itself
                                    // The three ways of waiting, each for its own cause:
                                    //  - the plan's limit: Baton Touch hands over to another account when one is free
                                    //    (then continues at once); without one, Zero-Waste waits for the reset;
                                    //  - anything else (no answer, overloaded, a server error): 100h mode.
                                    // Claude's 429 is its plan's limit, whatever the numbers say yet (they lag).
                                    let confirmed = sure == crate::resume::Limit::Sure || provider.as_deref().is_some_and(|p| limits.used_up(p, now, 95.0));
                                    let plan = confirmed || crate::resume::always_plan(provider.as_deref());
                                    if plan {
                                        rt.limit_found_at.store(now, std::sync::atomic::Ordering::SeqCst);
                                    }
                                    rt.limit_checks.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                                    let hint = crate::resume::minutes_in(&error).map(|m| now + m * 60_000);
                                    let found = limits.culprit(provider.as_deref(), now, hint);
                                    crate::logs::event("limit", serde_json::json!({
                                        "rt": rt.info.id, "provider": provider, "sure": sure == crate::resume::Limit::Sure, "fresh": fresh,
                                        "confirmed": confirmed, "plan": plan, "reset": found.map(|f| serde_json::json!({ "at": f.0, "minutes": f.1, "known": f.2 })),
                                        "limits": provider.as_deref().and_then(|p| limits.entry(p)).map(|e| e.windows), "error": crate::logs::clip(&error),
                                    }));
                                    if !plan {
                                        return;
                                    }
                                    // the refusal itself says the plan is used up, until numbers say which window
                                    if let (Some(p), true) = (provider.as_deref(), confirmed || !fresh) {
                                        limits.mark_reached(p, now);
                                    }
                                    // only a window we are sure of is marked used up; fresh numbers already say so
                                    if let (false, Some(p), Some((_, minutes, true))) = (fresh, provider.as_deref(), found) {
                                        limits.mark_used_up(p, minutes);
                                    }
                                    // Baton Touch: another account of the same plan provider takes over at once
                                    if rt.resume_cfg.get().enabled {
                                        if let Some(p) = provider.as_deref() {
                                            match accounts.baton_touch(&pi_bin, p, found.map(|(t, _, _)| t), &limits).await {
                                                crate::accounts::Baton::Swapped { next, was } => {
                                                    crate::logs::event("baton", serde_json::json!({ "rt": rt.info.id, "result": "swapped", "provider": p }));
                                                    accounts.announce(Some(&rt.info.id), &next);
                                                    let _ = usage.poll(&pi_bin, &limits, true, Some(p)).await;
                                                    rt.resume_baton(None, serde_json::json!({ "provider": next.provider, "email": next.email, "plan": next.plan, "was": was })).await;
                                                    return;
                                                }
                                                crate::accounts::Baton::Later { next, at, was } => {
                                                    crate::logs::event("baton", serde_json::json!({ "rt": rt.info.id, "result": "later", "provider": p, "at": at }));
                                                    // every account is limited: the one that resets first takes over then
                                                    let to = serde_json::json!({ "provider": next.provider, "email": next.email, "plan": next.plan, "at": at, "was": was });
                                                    rt.stop_retrying().await;
                                                    rt.resume_baton(Some(at), to).await;
                                                    tokio::spawn(rt.clone().take_over_at(at, next.id, was, accounts.clone(), limits.clone(), usage.clone(), pi_bin.clone()));
                                                    return;
                                                }
                                                crate::accounts::Baton::Handled => {
                                                    crate::logs::event("baton", serde_json::json!({ "rt": rt.info.id, "result": "handled", "provider": p }));
                                                    rt.resume_soon();
                                                    return;
                                                }
                                                crate::accounts::Baton::No => {
                                                    crate::logs::event("baton", serde_json::json!({ "rt": rt.info.id, "result": "none", "provider": p }));
                                                    // no account of this provider left: another provider, when allowed
                                                    if let Some(t) = accounts.cross_target(p, &limits) {
                                                        let cmd = serde_json::json!({ "type": "set_model", "provider": t.provider, "modelId": t.model });
                                                        if rt.stdin_tx.send(cmd.to_string()).await.is_ok() {
                                                            // on the new model from now (its first reply would say so only later)
                                                            *rt.last_model.lock().unwrap() = Some((t.provider.clone(), t.model.clone()));
                                                            let home = failed_model.filter(|(fp, _)| fp == p);
                                                            let from = home.as_ref().map(|(_, m)| m.clone());
                                                            rt.resume_baton(None, serde_json::json!({ "provider": t.provider, "model": t.model, "from": p, "was": from })).await;
                                                            if let Some((fp, fm)) = home.filter(|_| accounts.return_home()) {
                                                                let reset = found.map(|(t, _, _)| t);
                                                                tokio::spawn(rt.clone().go_home(fp, fm, t, reset, limits.clone(), usage.clone(), pi_bin.clone(), accounts.clone()));
                                                            }
                                                            return;
                                                        }
                                                    }
                                                }
                                            }
                                        } else if accounts.baton_just_now() {
                                            // pi's giving up names no provider: the failure Baton Touch already handled
                                            rt.resume_soon();
                                            return;
                                        }
                                    }
                                    // waiting for the reset: pi's own retries would only meet the same limit again
                                    rt.stop_retrying().await;
                                    *rt.wait_for.lock().unwrap() = provider.clone();
                                    if confirmed || crate::resume::minutes_in(&error).is_some() {
                                        rt.resume_begin(&error, found.map(|(t, m, _)| (t, m)));
                                    } else {
                                        // Claude's numbers do not show the plan used up (yet): ask them again in a few
                                        // minutes; a used-up window then makes the wait its reset, otherwise it goes on
                                        rt.resume_after(crate::resume::RATE_LIMIT_WAIT_MS, false);
                                    }
                                });
                            }
                        }
                    }
                }
                rt.long.lock().unwrap().take();
            });
        }

        // stderr → tail buffer + diagnostic frames. Never parsed as protocol.
        {
            let rt = rt.clone();
            tokio::spawn(async move {
                let mut rd = BufReader::new(stderr);
                let mut line = String::new();
                loop {
                    line.clear();
                    match rd.read_line(&mut line).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let text = line.trim_end().to_string();
                            if text.is_empty() {
                                continue;
                            }
                            {
                                let mut tail = rt.stderr_tail.lock().unwrap();
                                if tail.len() >= STDERR_TAIL {
                                    tail.pop_front();
                                }
                                tail.push_back(text.clone());
                            }
                            let frame = serde_json::json!({ "type": "oven_stderr", "line": text });
                            let _ = rt.events.send(Arc::from(frame.to_string()));
                        }
                    }
                }
            });
        }

        // stdin writer. Dropping stdin asks pi to shut down in an orderly way.
        tokio::spawn(async move {
            let mut stdin = stdin;
            loop {
                tokio::select! {
                    line = stdin_rx.recv() => {
                        let Some(line) = line else { break };
                        if stdin.write_all(line.as_bytes()).await.is_err()
                            || stdin.write_all(b"\n").await.is_err()
                            || stdin.flush().await.is_err()
                        {
                            break;
                        }
                    }
                    _ = shutdown_rx_w.changed() => break,
                }
            }
        });

        // Reaper: wait for exit, or on shutdown give pi a moment before killing it.
        {
            let mgr = self.clone();
            let rt = rt.clone();
            let id = id.clone();
            tokio::spawn(async move {
                let code = tokio::select! {
                    st = child.wait() => st.ok().and_then(|s| s.code()),
                    _ = shutdown_rx_c.changed() => {
                        match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
                            Ok(st) => st.ok().and_then(|s| s.code()),
                            Err(_) => { let _ = child.kill().await; None }
                        }
                    }
                };
                let _ = rt.events.send(Arc::from(exit_frame(code)));
                mgr.map.write().await.remove(&id);
                tracing::info!(runtime = %id, ?code, "pi exited");
            });
        }

        tracing::info!(runtime = %id, pid = ?rt.info.pid, cwd = %rt.info.cwd, "pi started");
        self.map.write().await.insert(id, rt.clone());
        Ok(rt)
    }
}

/// pi's `message_update` comes with every piece of an answer, a few hundred a reply, each with the reply's whole
/// `usage` (tokens and cost, about 170 bytes; more than the piece). The page reads only the output token count from it.
/// Kept: the event as it is, minus a `partial` copy of the answer when one comes; `usage.output`. The event's `type`
/// stays first (the pet reads it there).
fn slim_update(line: &str) -> Option<String> {
    if !line.starts_with(r#"{"type":"message_update""#) {
        return None;
    }
    let mut v: serde_json::Value = serde_json::from_str(line).ok()?;
    // taken, not cloned: `partial` is the whole answer so far, a few hundred times a reply
    let mut ev = std::mem::take(v.get_mut("assistantMessageEvent")?.as_object_mut()?);
    let kind = ev.remove("type")?;
    let output = v.pointer("/usage/output").or_else(|| ev.get("partial").and_then(|p| p.pointer("/usage/output"))).cloned();
    ev.remove("partial");
    let mut out = String::from(r#"{"type":"message_update""#);
    if let Some(n) = output {
        out.push_str(&format!(r#","usage":{{"output":{n}}}"#));
    }
    out.push_str(&format!(r#","assistantMessageEvent":{{"type":{kind}"#));
    for (k, val) in &ev {
        out.push(',');
        out.push_str(&serde_json::Value::String(k.clone()).to_string());
        out.push(':');
        out.push_str(&val.to_string());
    }
    out.push_str("}}");
    Some(out)
}

/// A path with its links and `..` resolved, also for a file not written yet: a new session's pi names its file at once
/// but writes it only with the first reply, and such a session must still be found (to delete it, for one).
pub fn resolve_path(p: &std::path::Path) -> Option<PathBuf> {
    p.canonicalize().ok().or_else(|| Some(p.parent()?.canonicalize().ok()?.join(p.file_name()?)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_update_keeps_its_piece_and_the_output_count_only() {
        let answer = "x".repeat(5000);
        let partial = serde_json::json!({ "role": "assistant", "content": [{ "type": "text", "text": answer }], "usage": { "input": 9, "output": 1234 } });
        let line = serde_json::json!({ "type": "message_update", "assistantMessageEvent": { "type": "text_delta", "contentIndex": 0, "delta": "a\"b", "partial": partial }, "message": partial }).to_string();
        // serde_json writes keys in order: put `type` first as pi does
        let line = line.replacen(r#"{"assistantMessageEvent""#, r#"{"type":"message_update","assistantMessageEvent""#, 1).replacen(r#","type":"message_update"}"#, "}", 1);
        assert!(line.starts_with(r#"{"type":"message_update""#) && line.len() > 10_000, "{}", &line[..80]);
        let slim = super::slim_update(&line).unwrap();
        assert!(slim.starts_with(r#"{"type":"message_update","usage":{"output":1234},"assistantMessageEvent":{"type":"text_delta""#), "{slim}");
        assert!(slim.len() < 200, "{slim}");
        let v: serde_json::Value = serde_json::from_str(&slim).unwrap();
        assert_eq!(v["assistantMessageEvent"]["delta"], "a\"b");
        assert_eq!(v["assistantMessageEvent"]["contentIndex"], 0);
        assert_eq!(v["usage"]["output"], 1234);
        assert!(v.get("message").is_none() && v["assistantMessageEvent"].get("partial").is_none());
        // as pi's RPC sends it: the whole usage beside the event
        let rpc = r#"{"type":"message_update","usage":{"input":10,"output":77,"cacheRead":0,"cacheWrite":0,"totalTokens":87,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}},"assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"조각"}}"#;
        assert_eq!(super::slim_update(rpc).unwrap(), r#"{"type":"message_update","usage":{"output":77},"assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"조각"}}"#);
        // a block's end keeps its whole content
        let end = r#"{"type":"message_update","assistantMessageEvent":{"type":"text_end","contentIndex":0,"content":"all of it","partial":{"content":[]}},"message":{}}"#;
        let v: serde_json::Value = serde_json::from_str(&super::slim_update(end).unwrap()).unwrap();
        assert_eq!(v["assistantMessageEvent"]["content"], "all of it");
        assert!(super::slim_update(r#"{"type":"message_end","message":{}}"#).is_none());
    }

    use super::*;

    #[test]
    fn learns_the_session_file_from_get_state() {
        let ok = r#"{"id":"1","type":"response","command":"get_state","success":true,"data":{"sessionFile":"/s/a.jsonl","model":null}}"#;
        assert_eq!(state_session_file(ok).as_deref(), Some("/s/a.jsonl"));
        assert_eq!(state_session_file(r#"{"type":"response","id":"1","command":"get_state","success":true,"data":{}}"#), None);
        assert_eq!(state_session_file(r#"{"type":"message_end","sessionFile":"/x"}"#), None);
    }

    fn feed(r: &mut Replay, lines: &[&str]) {
        for l in lines {
            r.record(&Arc::from(*l));
        }
    }

    #[test]
    fn a_long_run_keeps_only_what_a_late_joiner_needs() {
        let mut r = Replay::default();
        feed(
            &mut r,
            &[
                r#"{"type":"agent_start"}"#,
                r#"{"type":"message_start","message":{"role":"assistant"}}"#,
                r#"{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"a"}}"#,
                r#"{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"b"}}"#,
                r#"{"type":"message_end","message":{"role":"assistant"}}"#,
                r#"{"type":"tool_execution_start","toolCallId":"t1"}"#,
                r#"{"type":"tool_execution_update","toolCallId":"t1","partialResult":"1"}"#,
                r#"{"type":"tool_execution_start","toolCallId":"t2"}"#,
                r#"{"type":"tool_execution_update","toolCallId":"t1","partialResult":"12"}"#,
                r#"{"type":"tool_execution_update","toolCallId":"t2","partialResult":"x"}"#,
                r#"{"type":"tool_execution_update","toolCallId":"t1","partialResult":"123"}"#,
            ],
        );
        let kinds: Vec<String> = r.run.iter().map(|l| l.chars().take(60).collect()).collect();
        assert!(!kinds.iter().any(|l| l.contains("message_update")), "finished message's pieces dropped: {kinds:?}");
        let t1: Vec<_> = r.run.iter().filter(|l| l.contains(r#""toolCallId":"t1","partialResult""#)).collect();
        assert_eq!(t1.len(), 1);
        assert!(t1[0].contains(r#""123""#), "the newest output stays");
        assert_eq!(r.run.iter().filter(|l| l.contains(r#""t2","partialResult""#)).count(), 1);

        feed(&mut r, &[r#"{"type":"tool_execution_end","toolCallId":"t1"}"#, r#"{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"c"}}"#]);
        assert!(!r.run.iter().any(|l| l.contains(r#""t1""#)), "an ended tool goes entirely (its result is in the history)");
        assert!(r.run.iter().any(|l| l.contains(r#""t2","partialResult""#)), "a running one's stays");
        assert!(r.run.iter().any(|l| l.contains(r#""delta":"c""#)), "the message still streaming is kept");
        assert_eq!(r.tool_updates.get("t2").map(|&i| r.run[i].contains("t2")), Some(true), "index follows the removals");

        feed(&mut r, &[r#"{"type":"agent_settled"}"#]);
        assert!(r.run.is_empty() && r.tool_updates.is_empty());
    }

    #[test]
    fn hours_of_a_run_do_not_pile_up() {
        let mut r = Replay::default();
        feed(&mut r, &[r#"{"type":"agent_start"}"#]);
        for i in 0..500 {
            let big = "x".repeat(4000);
            feed(
                &mut r,
                &[
                    r#"{"type":"turn_start"}"#,
                    r#"{"type":"message_start","message":{"role":"assistant"}}"#,
                    &format!(r#"{{"type":"message_end","message":{{"role":"assistant","content":"{big}"}}}}"#),
                    &format!(r#"{{"type":"tool_execution_start","toolCallId":"c{i}"}}"#),
                    &format!(r#"{{"type":"tool_execution_end","toolCallId":"c{i}","result":"{big}"}}"#),
                    &format!(r#"{{"type":"message_end","message":{{"role":"toolResult","content":"{big}"}}}}"#),
                    r#"{"type":"queue_update","steering":[],"followUp":[]}"#,
                ],
            );
        }
        feed(&mut r, &[r#"{"type":"auto_retry_start","attempt":1}"#, r#"{"type":"message_start","message":{"role":"assistant"}}"#, r#"{"type":"message_update","x":1}"#]);
        let kinds: Vec<&str> = r.run.iter().map(|l| l.split('"').nth(3).unwrap_or("")).collect();
        assert_eq!(kinds, ["agent_start", "queue_update", "auto_retry_start", "message_start", "message_update"], "{kinds:?}");
        let bytes: usize = r.run.iter().map(|l| l.len()).sum();
        assert!(bytes < 1000, "{bytes} bytes kept");
        feed(&mut r, &[r#"{"id":"ab-3","type":"response","command":"get_messages","data":{"messages":[]}}"#, r#"{"type":"response","command":"set_model"}"#]);
        assert!(!r.run.iter().any(|l| l.contains("response")), "answers are the asking page's");
        feed(&mut r, &[r#"{"type":"auto_retry_end","success":true}"#]);
        assert!(!r.run.iter().any(|l| l.contains("auto_retry")));
    }

    #[test]
    fn the_exit_frame_starts_with_its_type() {
        for code in [Some(0), Some(-1), None] {
            let f = super::exit_frame(code);
            assert!(f.starts_with(r#"{"type":"oven_exit""#), "{f}");
            assert_eq!(serde_json::from_str::<serde_json::Value>(&f).unwrap()["type"], "oven_exit");
        }
    }
}
