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
const MESSAGE_UPDATE: &str = r#"{"type":"message_update""#;

fn tool_call_id(line: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Id {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
    }
    serde_json::from_str::<Id>(line).ok().map(|i| i.tool_call_id)
}

impl Replay {
    fn record(&mut self, line: &Arc<str>) {
        if line.starts_with(r#"{"type":"agent_start""#) {
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
            } else if !line.starts_with(r#"{"type":"extension_ui_request""#) {
                self.run.push(line.clone());
            }
            // A finished message is in the history a late joiner loads anyway: its streamed pieces are not needed.
            if line.starts_with(r#"{"type":"message_end""#) {
                self.run.retain(|l| !l.starts_with(MESSAGE_UPDATE));
                self.reindex();
            } else if line.starts_with(r#"{"type":"tool_execution_end""#) {
                if let Some(i) = tool_call_id(line).and_then(|id| self.tool_updates.remove(&id)) {
                    self.run.remove(i);
                    self.reindex();
                }
            }
            if line.starts_with(r#"{"type":"agent_settled""#) {
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
    /// The provider and model of the last reply (what Baton Touch hands over from, and whether it is still on the
    /// model it handed over to).
    last_model: Mutex<Option<(String, String)>>,
    /// No run under way (pi's `set_model` waits for one to end).
    idle: std::sync::atomic::AtomicBool,
    /// The bundled extension is loaded: notes can be kept in the session (`/oven-note`).
    notes: bool,
    /// The session file pi reported last (a new session has none at spawn, and pi can switch files).
    session_file: Mutex<Option<String>>,
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
    pub fn model_chosen(&self, provider: &str, model: &str) {
        *self.last_model.lock().unwrap() = Some((provider.to_owned(), model.to_owned()));
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
        let at = at.unwrap_or_else(now_ms) + crate::resume::BATON_DELAY_MS;
        let pending = crate::resume::Pending { at, tries, window: None, known: true };
        let begun = self.resume.lock().unwrap().replace(pending).is_none();
        self.note(to).await;
        self.resume_publish(false);
        if begun {
            tokio::spawn(self.clone().resume_clock());
        }
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
        tokio::time::sleep(Duration::from_millis(at.saturating_sub(now_ms()))).await;
        if *self.shutdown.borrow() || self.resume.lock().unwrap().is_none() {
            return;
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
        // a weekly (or longer) limit is days away: nothing to win by waiting, so the page just says when it resets
        if let Some((reset_at, minutes)) = culprit.filter(|c| c.1 > crate::resume::SHORT_WINDOW_MAX) {
            self.resume_tries.store(0, Ordering::Relaxed);
            self.resume.lock().unwrap().take();
            let frame = serde_json::json!({ "type": "oven_resume", "state": null, "gaveUp": false, "skipped": { "window": minutes, "inMs": reset_at.saturating_sub(now_ms()) } });
            let _ = self.events.send(Arc::from(frame.to_string()));
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
        let pending = crate::resume::Pending { at, tries, window, known };
        let begun = self.resume.lock().unwrap().replace(pending).is_none();
        self.resume_publish(false);
        if begun {
            tokio::spawn(self.clone().resume_clock());
        }
    }

    /// pi started a run. Ours takes the wait away before it sends, so a wait still here means the user sent something.
    fn resume_on_start(&self) {
        if self.resume.lock().unwrap().take().is_some() {
            self.resume_tries.store(0, std::sync::atomic::Ordering::Relaxed);
            self.resume_publish(false);
        }
    }

    /// A run that ends with nothing waiting got through: the count starts again.
    fn resume_on_settled(&self) {
        if self.resume.lock().unwrap().is_none() {
            self.resume_tries.store(0, std::sync::atomic::Ordering::Relaxed);
        }
    }

    async fn resume_clock(self: Arc<Self>) {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if *self.shutdown.borrow() {
                return;
            }
            let fire = {
                let mut g = self.resume.lock().unwrap();
                let Some(p) = g.as_ref() else { return };
                if !self.resume_cfg.get().enabled {
                    *g = None;
                    drop(g);
                    self.resume_publish(false);
                    return;
                }
                if now_ms() >= p.at {
                    self.resume_tries.store(p.tries, std::sync::atomic::Ordering::Relaxed);
                    self.resume_fired_at.store(now_ms(), std::sync::atomic::Ordering::Relaxed);
                    *g = None;
                    true
                } else {
                    false
                }
            };
            if fire {
                let text = self.resume_cfg.get().text;
                let _ = self.stdin_tx.send(serde_json::json!({ "type": "prompt", "message": text }).to_string()).await;
                self.resume_publish(false);
                return;
            }
        }
    }

    pub fn resume_now(&self) {
        if let Some(p) = self.resume.lock().unwrap().as_mut() {
            p.at = now_ms();
        }
    }

    pub fn resume_stop(&self) {
        if self.resume.lock().unwrap().take().is_some() {
            self.resume_tries.store(0, std::sync::atomic::Ordering::Relaxed);
            self.resume_publish(false);
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
        Arc::new(Self { cfg, map: RwLock::new(HashMap::new()), spawning: Default::default(), pet: crate::pet::PetHub::new(), updates: Default::default(), long, limits, resume, usage: Default::default(), cache_ttl: Default::default(), accounts: Default::default() })
    }

    pub async fn get(&self, id: &str) -> Option<Arc<Runtime>> {
        self.map.read().await.get(id).cloned()
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
    pub async fn using_file(&self, file: &std::path::Path) -> Vec<String> {
        let same = |p: &str| PathBuf::from(p).canonicalize().is_ok_and(|p| p == file);
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
            last_model: Mutex::new(None),
            idle: std::sync::atomic::AtomicBool::new(true),
            notes: self.long.available(),
        });
        self.pet.attach(rt.clone());

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
                            let line: Arc<str> = Arc::from(String::from_utf8_lossy(&buf).as_ref());
                            if line.starts_with(r#"{"type":"extension_ui_request""#) && line.contains(r#""method":"select""#) {
                                if let Some((ask, offer)) = crate::update::from_prompt(&line) {
                                    updates.offered(offer, now_ms());
                                    let answer = serde_json::json!({ "type": "extension_ui_response", "id": ask, "cancelled": true });
                                    let _ = rt.stdin_tx.try_send(answer.to_string());
                                    let _ = rt.events.send(Arc::from(r#"{"type":"oven_update"}"#));
                                    continue;
                                }
                            }
                            if let Some(f) = state_session_file(&line) {
                                *rt.session_file.lock().unwrap() = Some(f);
                            }
                            if let Some(e) = crate::limits::read(&line) {
                                limits.record(e);
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
                            if let Some(m) = model.as_ref().filter(|_| limit.is_none() && !line.contains(r#""stopReason":"error""#)) {
                                *rt.last_model.lock().unwrap() = Some(m.clone());
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
                                // ask the provider for the numbers as they are now, then decide; without them, the last reply's
                                let (rt, limits, usage, pi_bin, accounts) = (rt.clone(), limits.clone(), usage.clone(), pi_bin.clone(), accounts.clone());
                                let failed_model = model.clone();
                                rt.limit_checks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                tokio::spawn(async move {
                                    let fresh = provider.is_some() && usage.poll(&pi_bin, &limits, true, provider.as_deref()).await;
                                    let now = now_ms();
                                    // a 429 that may be the plan: only a window the numbers show as (nearly) used up makes it one;
                                    // otherwise it was a short rate limit, which pi retries by itself
                                    let plan = sure == crate::resume::Limit::Sure || provider.as_deref().is_some_and(|p| limits.used_up(p, now, 95.0));
                                    if plan {
                                        rt.limit_found_at.store(now, std::sync::atomic::Ordering::SeqCst);
                                    }
                                    rt.limit_checks.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                                    if !plan {
                                        return;
                                    }
                                    let hint = crate::resume::minutes_in(&error).map(|m| now + m * 60_000);
                                    let found = limits.culprit(provider.as_deref(), now, hint);
                                    // only a window we are sure of is marked used up; fresh numbers already say so
                                    if let (false, Some(p), Some((_, minutes, true))) = (fresh, provider.as_deref(), found) {
                                        limits.mark_used_up(p, minutes);
                                    }
                                    // Baton Touch: another account of the same plan provider takes over at once
                                    if rt.resume_cfg.get().enabled {
                                        if let Some(p) = provider.as_deref() {
                                            match accounts.baton_touch(&pi_bin, p, found.map(|(t, _, _)| t), &limits).await {
                                                crate::accounts::Baton::Swapped { next, was } => {
                                                    accounts.announce(Some(&rt.info.id), &next);
                                                    let _ = usage.poll(&pi_bin, &limits, true, Some(p)).await;
                                                    rt.resume_baton(None, serde_json::json!({ "provider": next.provider, "email": next.email, "plan": next.plan, "was": was })).await;
                                                    return;
                                                }
                                                crate::accounts::Baton::Later { next, at, was } => {
                                                    // every account is limited: the one that resets first takes over then
                                                    let to = serde_json::json!({ "provider": next.provider, "email": next.email, "plan": next.plan, "at": at, "was": was });
                                                    rt.resume_baton(Some(at), to).await;
                                                    tokio::spawn(rt.clone().take_over_at(at, next.id, was, accounts.clone(), limits.clone(), usage.clone(), pi_bin.clone()));
                                                    return;
                                                }
                                                crate::accounts::Baton::Handled => return,
                                                crate::accounts::Baton::No => {
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
                                            return;
                                        }
                                    }
                                    rt.resume_begin(&error, found.map(|(t, m, _)| (t, m)));
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

#[cfg(test)]
mod tests {
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
        assert!(!r.run.iter().any(|l| l.contains(r#""t1","partialResult""#)), "an ended tool's update goes");
        assert!(r.run.iter().any(|l| l.contains(r#""t2","partialResult""#)), "a running one's stays");
        assert!(r.run.iter().any(|l| l.contains(r#""delta":"c""#)), "the message still streaming is kept");
        assert_eq!(r.tool_updates.get("t2").map(|&i| r.run[i].contains("t2")), Some(true), "index follows the removals");

        feed(&mut r, &[r#"{"type":"agent_settled"}"#]);
        assert!(r.run.is_empty() && r.tool_updates.is_empty());
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
