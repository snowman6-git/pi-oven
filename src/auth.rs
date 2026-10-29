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

fn host_only(authority: &str) -> &str {
    if let Some(rest) = authority.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest);
    }
    authority.rsplit_once(':').map_or(authority, |(h, _)| h)
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// `%41` -> `A`. A password with `&`, `+` or `%` in it has to survive being put in a URL (the WebSocket carries it there).
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
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

fn client_ip(headers: &HeaderMap, peer: SocketAddr, trust_proxy: bool) -> IpAddr {
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

    if !app.secrets.is_empty() && req.uri().path().starts_with("/api") {
        let bearer = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).map(str::to_owned);
        let query = req.uri().query().and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("token="))).map(percent_decode);
        let ip = client_ip(headers, peer, app.trust_proxy);
        if let Some(left) = app.throttle.locked(ip) {
            let mut res = deny(StatusCode::TOO_MANY_REQUESTS, "too many wrong attempts; try again later");
            if let Ok(v) = header::HeaderValue::from_str(&left.as_secs().max(1).to_string()) {
                res.headers_mut().insert(header::RETRY_AFTER, v);
            }
            return res;
        }
        match bearer.or(query) {
            // nothing offered is not a wrong guess: the page asks for it
            None => return deny(StatusCode::UNAUTHORIZED, "missing or wrong token"),
            Some(offered) => {
                // every secret is compared, so how long it takes does not say which one matched
                let ok = app.secrets.iter().fold(false, |acc, s| constant_time_eq(&offered, s) | acc);
                if !ok {
                    app.throttle.failed(ip);
                    return deny(StatusCode::UNAUTHORIZED, "missing or wrong token");
                }
                app.throttle.succeeded(ip);
            }
        }
    }
    next.run(req).await
}
