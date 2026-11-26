//! Request guard. pi can run shell commands, so the API must not be reachable from other web pages
//! (CSRF / DNS rebinding) and, off loopback, must require a token.

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::AppState;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

fn host_only(authority: &str) -> &str {
    if let Some(rest) = authority.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest);
    }
    authority.rsplit_once(':').map_or(authority, |(h, _)| h)
}

pub fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Wrong credentials are counted per client; too many in a minute and that client is locked out for a while
/// (longer each time), even with the right password, so a password cannot be guessed at speed.
const WINDOW: Duration = Duration::from_secs(60);
const MAX_FAILS: u32 = 8;
const FIRST_LOCK: Duration = Duration::from_secs(30);
const MAX_LOCK: Duration = Duration::from_secs(15 * 60);

struct Entry {
    fails: u32,
    since: Instant,
    locked_until: Option<Instant>,
    lockouts: u32,
}

#[derive(Default)]
pub struct Throttle(Mutex<HashMap<IpAddr, Entry>>);

impl Throttle {
    /// How much longer this client is locked out, if it is.
    pub fn locked(&self, ip: IpAddr) -> Option<Duration> {
        let map = self.0.lock().ok()?;
        let until = map.get(&ip)?.locked_until?;
        until.checked_duration_since(Instant::now())
    }

    pub fn failed(&self, ip: IpAddr) {
        let Ok(mut map) = self.0.lock() else { return };
        let now = Instant::now();
        if map.len() > 2000 {
            map.retain(|_, e| e.locked_until.is_some_and(|u| u > now) || now.duration_since(e.since) < WINDOW);
        }
        let e = map.entry(ip).or_insert(Entry { fails: 0, since: now, locked_until: None, lockouts: 0 });
        if now.duration_since(e.since) > WINDOW {
            e.fails = 0;
            e.since = now;
        }
        e.fails += 1;
        if e.fails >= MAX_FAILS {
            let lock = FIRST_LOCK.saturating_mul(1 << e.lockouts.min(5)).min(MAX_LOCK);
            e.locked_until = Some(now + lock);
            e.lockouts += 1;
            e.fails = 0;
            e.since = now;
        }
    }

    pub fn succeeded(&self, ip: IpAddr) {
        if let Ok(mut map) = self.0.lock() {
            map.remove(&ip);
        }
    }
}

pub fn client_ip(headers: &HeaderMap, peer: SocketAddr, trust_proxy: bool) -> IpAddr {
    if trust_proxy {
        if let Some(ip) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()).and_then(|v| v.split(',').next()).and_then(|v| v.trim().parse().ok()) {
            return ip;
        }
    }
    peer.ip()
}

fn deny(code: StatusCode, msg: &str) -> Response {
    (code, msg.to_owned()).into_response()
}

pub async fn guard(State(app): State<Arc<AppState>>, ConnectInfo(peer): ConnectInfo<SocketAddr>, req: Request, next: Next) -> Response {
    let headers = req.headers();
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");

    if app.secrets.is_empty() && !matches!(host_only(host), "localhost" | "127.0.0.1" | "::1") {
        let allowed = app.allow_origins.iter().any(|o| o.split("://").nth(1).is_some_and(|a| a == host));
        if !allowed {
            return deny(StatusCode::FORBIDDEN, "unexpected Host header");
        }
    }

    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let same = origin.split("://").nth(1).is_some_and(|a| a == host);
        if !same && !app.allow_origins.iter().any(|o| o == origin) {
            return deny(StatusCode::FORBIDDEN, "cross-origin request blocked");
        }
    }

    let mut req = req;
    if !app.secrets.is_empty() && req.uri().path().starts_with("/api") {
        let headers = req.headers();
        let path = req.uri().path();
        let ip = client_ip(headers, peer, app.trust_proxy);
        if let Some(left) = app.throttle.locked(ip) {
            let mut res = deny(StatusCode::TOO_MANY_REQUESTS, "too many wrong attempts; try again later");
            if let Ok(v) = header::HeaderValue::from_str(&left.as_secs().max(1).to_string()) {
                res.headers_mut().insert(header::RETRY_AFTER, v);
            }
            return res;
        }
        if path != "/api/login" {
            let bearer = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
            let query = |key: &str| req.uri().query().and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix(key)));
            let is_ws = path.strip_prefix("/api/runtimes/").and_then(|r| r.strip_suffix("/ws")).is_some_and(|id| !id.is_empty() && !id.contains('/'));
            // None: nothing offered (not a wrong guess: the page asks for it). Some(None): offered and wrong.
            let found: Option<Option<String>> = match bearer {
                Some(t) => Some(app.auth.check(t)),
                None => query("ticket=").filter(|_| is_ws).map(|t| app.auth.use_ticket(t)),
            };
            match found {
                None => return deny(StatusCode::UNAUTHORIZED, "missing or wrong token"),
                Some(None) => {
                    app.throttle.failed(ip);
                    return deny(StatusCode::UNAUTHORIZED, "missing or wrong token");
                }
                Some(Some(id)) => {
                    app.throttle.succeeded(ip);
                    if !id.is_empty() {
                        req.extensions_mut().insert(Current(id));
                    }
                }
            }
        }
    }
    next.run(req).await
}

/// The id of the session that made the request (a request with a ticket, or without any secret set, has none).
#[derive(Clone)]
pub struct Current(pub String);

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    /// SHA-256 hex of the token; the token itself is never kept.
    pub hash: String,
    pub device: String,
    pub created_at: u64,
    pub last_used_at: u64,
    /// When `last_used_at` last reached the file.
    #[serde(skip)]
    saved_at: u64,
}

const IDLE_LIMIT_MS: u64 = 30 * 24 * 3600 * 1000;
const SAVE_EVERY_MS: u64 = 10 * 60 * 1000;
const MAX_SESSIONS: usize = 200;
const TICKET_TTL: Duration = Duration::from_secs(30);
const MAX_TICKETS: usize = 500;
const FILE: &str = "auth-sessions.json";

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn hash_of(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Default)]
struct Inner {
    list: Vec<Session>,
    /// ticket -> (made at, the session it was made for)
    tickets: HashMap<String, (Instant, String)>,
}

/// What the file holds: the sessions, and which secrets they were signed in with (a salted, slow hash of them). Started
/// with other secrets (a password changed because it leaked, or `--lan`'s new token), the server forgets every session.
#[derive(Serialize, Deserialize, Default)]
struct Stored {
    #[serde(default)]
    salt: String,
    secrets: String,
    sessions: Vec<Session>,
}

/// The configured secrets, hashed with `salt` many times over (whatever their order): the file must not make a weak
/// password cheap to guess offline.
pub fn fingerprint(salt: &str, secrets: &[String]) -> String {
    let mut v: Vec<&str> = secrets.iter().map(String::as_str).collect();
    v.sort_unstable();
    let mut h = Sha256::digest(format!("{salt}\0{}", v.join("\0")).as_bytes());
    for _ in 0..FINGERPRINT_ROUNDS {
        h = Sha256::digest(h);
    }
    h.iter().map(|b| format!("{b:02x}")).collect()
}

const FINGERPRINT_ROUNDS: u32 = 200_000;

/// Login sessions (kept in `<cache>/auth-sessions.json`, as hashes) and one-time WebSocket tickets (memory only).
pub struct Auth {
    inner: Mutex<Inner>,
    file: std::path::PathBuf,
    salt: String,
    secrets: String,
    /// ids of sessions just ended: the sockets and event streams they opened close on it
    revoked: tokio::sync::broadcast::Sender<String>,
}

impl Auth {
    /// A file that is missing or broken means no sessions: everyone signs in again.
    pub fn load(file: std::path::PathBuf, secrets: &[String]) -> Self {
        let stored: Stored = std::fs::read_to_string(&file).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let salt = if stored.salt.is_empty() { crate::config::random_hex(16) } else { stored.salt };
        let fp = if secrets.is_empty() { String::new() } else { fingerprint(&salt, secrets) };
        // other secrets: the sessions go, from the file too (or setting the old password again would bring them back)
        let forget = stored.secrets != fp;
        let mut list = if forget { Vec::new() } else { stored.sessions };
        let now = now_ms();
        list.retain(|s| now.saturating_sub(s.last_used_at) < IDLE_LIMIT_MS);
        for s in &mut list {
            s.saved_at = s.last_used_at;
        }
        let me = Auth { inner: Mutex::new(Inner { list, tickets: HashMap::new() }), file, salt, secrets: fp, revoked: tokio::sync::broadcast::channel(64).0 };
        if forget {
            me.save(&[]);
        }
        me
    }

    pub fn open(secrets: &[String]) -> Self {
        Self::load(crate::config::cache_dir().join(FILE), secrets)
    }

    /// Whether the session `id` is still there.
    pub fn alive(&self, id: &str) -> bool {
        self.inner().list.iter().any(|s| s.id == id)
    }

    /// Ids of sessions as they end.
    pub fn revoked(&self) -> tokio::sync::broadcast::Receiver<String> {
        self.revoked.subscribe()
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn save(&self, list: &[Session]) {
        // a session list is a few KB; the write is atomic (temp file, then rename)
        let write = || -> std::io::Result<()> {
            use std::io::Write;
            if let Some(dir) = self.file.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = self.file.with_extension("json.tmp");
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
            let stored = serde_json::json!({ "salt": self.salt, "secrets": self.secrets, "sessions": list });
            opts.open(&tmp)?.write_all(&serde_json::to_vec(&stored)?)?;
            std::fs::rename(&tmp, &self.file)
        };
        if let Err(e) = write() {
            tracing::warn!("could not save {}: {e}", self.file.display());
        }
    }

    /// Who this token is (`Some(id)`), if it belongs to a live session.
    pub fn check(&self, token: &str) -> Option<String> {
        let hash = hash_of(token);
        let now = now_ms();
        let mut g = self.inner();
        // every hash is compared, so how long it takes does not say which one matched
        let mut at = None;
        for (i, s) in g.list.iter().enumerate() {
            if constant_time_eq(&s.hash, &hash) {
                at = Some(i);
            }
        }
        let i = at?;
        let s = &mut g.list[i];
        if now.saturating_sub(s.last_used_at) >= IDLE_LIMIT_MS {
            return None;
        }
        s.last_used_at = now;
        let id = s.id.clone();
        if now.saturating_sub(s.saved_at) >= SAVE_EVERY_MS {
            s.saved_at = now;
            self.save(&g.list);
        }
        Some(id)
    }

    /// Makes a session; returns its token (shown once) and id.
    pub fn create(&self, device: &str) -> (String, String) {
        let token = crate::config::random_hex(32);
        let id = crate::config::random_hex(6);
        let now = now_ms();
        let mut g = self.inner();
        g.list.retain(|s| now.saturating_sub(s.last_used_at) < IDLE_LIMIT_MS);
        while g.list.len() >= MAX_SESSIONS {
            let Some(oldest) = g.list.iter().enumerate().min_by_key(|(_, s)| s.last_used_at).map(|(i, _)| i) else { break };
            g.list.remove(oldest);
        }
        let device = device.chars().take(80).collect();
        g.list.push(Session { id: id.clone(), hash: hash_of(&token), device, created_at: now, last_used_at: now, saved_at: now });
        self.save(&g.list);
        (token, id)
    }

    /// Most recently used first, without the hashes.
    pub fn list(&self) -> Vec<serde_json::Value> {
        let now = now_ms();
        let mut v: Vec<Session> = self.inner().list.iter().filter(|s| now.saturating_sub(s.last_used_at) < IDLE_LIMIT_MS).cloned().collect();
        v.sort_by_key(|s| std::cmp::Reverse(s.last_used_at));
        v.iter().map(|s| serde_json::json!({ "id": s.id, "device": s.device, "createdAt": s.created_at, "lastUsedAt": s.last_used_at })).collect()
    }

    pub fn remove(&self, id: &str) -> bool {
        let mut g = self.inner();
        let before = g.list.len();
        g.list.retain(|s| s.id != id);
        g.tickets.retain(|_, (_, of)| of != id);
        let removed = g.list.len() != before;
        if removed {
            self.save(&g.list);
            let _ = self.revoked.send(id.to_owned());
        }
        removed
    }

    /// A one-time WebSocket ticket for the session `of` (empty: a server without secrets).
    pub fn new_ticket(&self, of: &str) -> String {
        let t = crate::config::random_hex(24);
        let now = Instant::now();
        let mut g = self.inner();
        g.tickets.retain(|_, (at, _)| now.duration_since(*at) < TICKET_TTL);
        if g.tickets.len() >= MAX_TICKETS {
            if let Some(oldest) = g.tickets.iter().min_by_key(|(_, (at, _))| *at).map(|(k, _)| k.clone()) {
                g.tickets.remove(&oldest);
            }
        }
        g.tickets.insert(t.clone(), (now, of.to_owned()));
        t
    }

    /// The session a ticket was made for: once per ticket, within its 30 seconds.
    pub fn use_ticket(&self, t: &str) -> Option<String> {
        self.inner().tickets.remove(t).filter(|(at, _)| at.elapsed() < TICKET_TTL).map(|(_, of)| of)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("oven-auth-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join(FILE)
    }

    #[test]
    fn sessions_outlive_a_restart_but_not_a_changed_secret() {
        let f = scratch("secret");
        let pw = vec!["first-password".to_owned()];
        let (token, id) = Auth::load(f.clone(), &pw).create("phone");
        assert_eq!(Auth::load(f.clone(), &pw).check(&token), Some(id), "same secrets: still signed in");
        let raw = std::fs::read_to_string(&f).unwrap();
        assert!(!raw.contains(&token) && !raw.contains("first-password"), "neither the token nor the secret is in the file");
        assert_eq!(Auth::load(f.clone(), &["new-password".to_owned()]).check(&token), None, "a changed secret ends every session");
        assert_eq!(Auth::load(f.clone(), &pw).check(&token), None, "and setting the old one again does not bring them back");
        let _ = std::fs::remove_dir_all(f.parent().unwrap());
    }

    #[test]
    fn a_ticket_names_its_session_once_and_dies_with_it() {
        let a = Auth::load(scratch("ticket"), &["pw-pw-pw-pw".to_owned()]);
        let (_, id) = a.create("laptop");
        let t = a.new_ticket(&id);
        assert_eq!(a.use_ticket(&t), Some(id.clone()));
        assert_eq!(a.use_ticket(&t), None, "once only");
        let t2 = a.new_ticket(&id);
        let mut gone = a.revoked();
        assert!(a.remove(&id));
        assert_eq!(gone.try_recv().ok(), Some(id), "open sockets hear that their session ended");
        assert_eq!(a.use_ticket(&t2), None, "its tickets went with it");
        let _ = std::fs::remove_dir_all(a.file.parent().unwrap());
    }
}
