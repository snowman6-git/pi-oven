use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::{sync::Arc, time::Duration};
use tokio::sync::broadcast::error::RecvError;

use crate::{runtime::Runtime, AppState};

pub async fn handler(State(app): State<Arc<AppState>>, Path(id): Path<String>, ws: WebSocketUpgrade) -> Response {
    match app.runtimes.get(&id).await {
        Some(rt) => ws.max_message_size(32 << 20).on_upgrade(move |sock| attach(sock, rt)).into_response(),
        None => (StatusCode::NOT_FOUND, "no such runtime").into_response(),
    }
}

/// A page that has gone without saying so (a phone asleep, a dropped mobile link) leaves a socket whose sends block
/// until TCP gives up, many minutes later; meanwhile its events pile up and the ping cannot go out. A send that takes
/// this long counts as a dead page: the socket is dropped, and the page reconnects when it is back.
const SEND_LIMIT: Duration = Duration::from_secs(15);

async fn send(sock: &mut WebSocket, msg: Message) -> bool {
    matches!(tokio::time::timeout(SEND_LIMIT, sock.send(msg)).await, Ok(Ok(())))
}

async fn attach(mut sock: WebSocket, rt: Arc<Runtime>) {
    let (mut events, replay) = rt.attach();

    let hello = serde_json::json!({
        "type": "oven_attached",
        "runtime": rt.info,
        "stderrTail": rt.stderr_tail(),
        "long": rt.long_view(),
        "resume": rt.resume_view(),
    });
    if !send(&mut sock, Message::text(hello.to_string())).await {
        return;
    }
    if let Some(frame) = replay {
        if !send(&mut sock, Message::text(frame)).await {
            return;
        }
    }

    let mut ping = tokio::time::interval(Duration::from_secs(25));
    loop {
        tokio::select! {
            ev = events.recv() => match ev {
                Ok(line) => {
                    if !send(&mut sock, Message::text(line.to_string())).await { break }
                    if line.starts_with(r#"{"type":"oven_exit""#) { break }
                }
                Err(RecvError::Lagged(n)) => {
                    // events were skipped for this page (a slow link): patching around the gap leaves stale state (a
                    // queue entry pi already delivered, a tool that never ends), so close and let the page reconnect,
                    // which loads the history again and gets the run so far as one replay
                    let f = serde_json::json!({ "type": "oven_lagged", "missed": n });
                    let _ = send(&mut sock, Message::text(f.to_string())).await;
                    break;
                }
                Err(RecvError::Closed) => break,
            },
            msg = sock.recv() => match msg {
                Some(Ok(Message::Text(t))) => {
                    // Only forward well-formed protocol records; pi answers garbage with a parse error anyway.
                    let parsed = serde_json::from_str::<serde_json::Value>(&t).ok();
                    let ok = parsed.as_ref().is_some_and(|v| v.get("type").is_some_and(|t| t.is_string()));
                    if let Some(v) = &parsed {
                        if v["type"] == "extension_ui_response" {
                            if let Some(id) = v["id"].as_str() { rt.dialog_answered(id); }
                        }
                        if v["type"] == "set_model" {
                            if let (Some(p), Some(m)) = (v["provider"].as_str(), v["modelId"].as_str()) { rt.model_chosen(p, m); }
                        }
                    }
                    if ok && !rt.send(t.to_string()).await { break }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
            _ = ping.tick() => {
                if !send(&mut sock, Message::Ping(Vec::new().into())).await { break }
            }
        }
    }
}
