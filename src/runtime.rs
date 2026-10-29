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

use crate::config::{random_hex, Config};

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
    /// Latest setStatus / setWidget per key, so a reload keeps extension state.
    ui: HashMap<String, Arc<str>>,
    /// Dialogs nobody has answered yet.
    dialogs: Vec<(String, Arc<str>)>,
}

impl Replay {
    fn record(&mut self, line: &Arc<str>) {
        if line.starts_with(r#"{"type":"agent_start""#) {
            self.active = true;
            self.run.clear();
        }
        if self.active {
            if !line.starts_with(r#"{"type":"extension_ui_request""#) {
                self.run.push(line.clone());
            }
            if line.starts_with(r#"{"type":"agent_settled""#) {
                self.active = false;
                self.run.clear();
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

    pub fn dialog_answered(&self, id: &str) {
        self.pet.answered(&self.info.id);
        self.replay.lock().unwrap().dialogs.retain(|(d, _)| d != id);
    }

    pub fn close(&self) {
        let _ = self.shutdown.send(true);
    }
}

pub struct Manager {
    cfg: Arc<Config>,
    map: RwLock<HashMap<String, Arc<Runtime>>>,
    /// What the desktop pet follows: pi's events, translated.
    pub pet: Arc<crate::pet::PetHub>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Manager {
    pub fn new(cfg: Arc<Config>) -> Arc<Self> {
        Arc::new(Self { cfg, map: RwLock::new(HashMap::new()), pet: crate::pet::PetHub::new() })
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
        if let Some(ext) = &self.cfg.extension_path {
            cmd.arg("--extension").arg(ext);
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
        });
        self.pet.attach(rt.clone());

        // stdout → replay buffer + broadcast. Split on LF only: U+2028/2029 are legal inside JSON strings.
        {
            let rt = rt.clone();
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
                            let mut replay = rt.replay.lock().unwrap();
                            replay.record(&line);
                            let _ = rt.events.send(line);
                        }
                    }
                }
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
                let frame = serde_json::json!({ "type": "oven_exit", "code": code });
                let _ = rt.events.send(Arc::from(frame.to_string()));
                mgr.map.write().await.remove(&id);
                tracing::info!(runtime = %id, ?code, "pi exited");
            });
        }

        tracing::info!(runtime = %id, pid = ?rt.info.pid, cwd = %rt.info.cwd, "pi started");
        self.map.write().await.insert(id, rt.clone());
        Ok(rt)
    }
}
