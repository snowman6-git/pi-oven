//! What changed on the server, for every page at once (`/api/events`, SSE): accounts, sessions, models, settings,
//! limits, updates. Only the kind goes out, never the data: a page reads the API it already uses again. That way a
//! sign-in finished in Settings, a session opened on the phone or a pin set on the desktop shows on every page without
//! a reload, also on the empty screen (where no session's socket is open).
//!
//! Most changes are a page's own request: `layer` names the kind from the path of a write that succeeded. Changes the
//! server makes by itself (a sign-in ending, Baton Touch, pi exiting, a usage check) call `changed` where they happen.

use axum::{
    extract::Request,
    http::{header, HeaderValue, Method},
    middleware::Next,
    response::{sse::{Event, KeepAlive}, IntoResponse, Response, Sse},
};
use futures_util::StreamExt;
use std::sync::OnceLock;
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream};

static TX: OnceLock<tokio::sync::broadcast::Sender<&'static str>> = OnceLock::new();

fn tx() -> &'static tokio::sync::broadcast::Sender<&'static str> {
    TX.get_or_init(|| tokio::sync::broadcast::channel(64).0)
}

/// Tell every page that `kind` changed.
pub fn changed(kind: &'static str) {
    let _ = tx().send(kind);
}

/// The kind a successful write to `path` changes, if any page shows it.
fn kind_of(method: &Method, path: &str) -> Option<&'static str> {
    if method == Method::GET || method == Method::HEAD || method == Method::OPTIONS {
        return None;
    }
    let p = path.strip_prefix("/api/")?;
    Some(match p {
        _ if p.starts_with("accounts") => "accounts",
        _ if p.starts_with("sessions") => "sessions",
        // opening a session (POST /api/runtimes), closing one (DELETE /api/runtimes/{id}), or starting it again
        "runtimes" => "sessions",
        _ if p.starts_with("runtimes/") && (method == Method::DELETE && p.matches('/').count() == 1 || p.ends_with("/restart")) => "sessions",
        "models/prefs" | "models/config" => "models",
        "resume" | "auto-compact" | "pi-compaction" | "claude-cache" | "addons/lens" | "limits/poll" | "retry" | "oven/auto" => "settings",
        "limits/refresh" => "limits",
        "update" => "updates",
        "login" | "logout" => "auth",
        _ if p.starts_with("auth/sessions/") => "auth",
        _ => return None,
    })
}

pub async fn layer(req: Request, next: Next) -> Response {
    let kind = kind_of(req.method(), req.uri().path());
    let res = next.run(req).await;
    if let Some(k) = kind.filter(|_| res.status().is_success()) {
        changed(k);
    }
    res
}

pub async fn stream(axum::extract::State(app): axum::extract::State<std::sync::Arc<crate::AppState>>, current: Option<axum::Extension<crate::auth::Current>>) -> Response {
    // a page that fell behind has missed something: it reads everything again
    let live = BroadcastStream::new(tx().subscribe()).map(|r| match r {
        Ok(k) => k,
        Err(BroadcastStreamRecvError::Lagged(_)) => "all",
    });
    // the stream ends with the session that opened it (the page then finds itself signed out)
    let mut revoked = app.auth.revoked();
    let session = current.map(|c| c.0 .0).filter(|s| !s.is_empty());
    // ended between the guard and the subscription above: over at once
    let gone = session.as_ref().is_some_and(|s| !app.auth.alive(s));
    let ended = async move {
        if gone {
            return;
        }
        let Some(me) = session else { return std::future::pending().await };
        loop {
            match revoked.recv().await {
                Ok(id) if id == me => return,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => return,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return std::future::pending().await,
                _ => {}
            }
        }
    };
    let live = live.take_until(ended);
    let events = live.map(|k| Ok::<_, std::convert::Infallible>(Event::default().data(format!(r#"{{"type":"{k}"}}"#))));
    // the keep-alive is an event, not a comment: a page sees it, and a link that went quiet (a phone that slept) is
    // noticed by its silence
    let ping = Event::default().data(r#"{"type":"ping"}"#);
    let mut res = Sse::new(events).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)).event(ping)).into_response();
    // a proxy that buffers (nginx does by default) would hold every event back
    res.headers_mut().insert("x-accel-buffering", HeaderValue::from_static("no"));
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_what_a_write_changes() {
        let (get, post, put, del) = (Method::GET, Method::POST, Method::PUT, Method::DELETE);
        assert_eq!(kind_of(&get, "/api/accounts"), None);
        assert_eq!(kind_of(&post, "/api/accounts/login/input"), Some("accounts"));
        assert_eq!(kind_of(&put, "/api/sessions/pins"), Some("sessions"));
        assert_eq!(kind_of(&post, "/api/runtimes"), Some("sessions"));
        assert_eq!(kind_of(&del, "/api/runtimes/ab12"), Some("sessions"));
        assert_eq!(kind_of(&post, "/api/runtimes/ab12/restart"), Some("sessions"));
        assert_eq!(kind_of(&del, "/api/runtimes/ab12/files"), None);
        assert_eq!(kind_of(&put, "/api/runtimes/ab12/files/write"), None);
        assert_eq!(kind_of(&post, "/api/runtimes/ab12/git/commit"), None);
        assert_eq!(kind_of(&put, "/api/models/prefs"), Some("models"));
        assert_eq!(kind_of(&put, "/api/resume"), Some("settings"));
        assert_eq!(kind_of(&post, "/api/fs/mkdir"), None);
        assert_eq!(kind_of(&post, "/api/login"), Some("auth"));
        assert_eq!(kind_of(&del, "/api/auth/sessions/ab12"), Some("auth"));
        assert_eq!(kind_of(&post, "/api/ticket"), None);
    }
}
