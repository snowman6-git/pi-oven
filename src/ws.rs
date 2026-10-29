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

async fn attach(mut sock: WebSocket, rt: Arc<Runtime>) {
    let (mut events, replay) = rt.attach();

    let hello = serde_json::json!({
        "type": "oven_attached",
        "runtime": rt.info,
        "stderrTail": rt.stderr_tail(),
    });
    if sock.send(Message::text(hello.to_string())).await.is_err() {
        return;
    }
    if let Some(frame) = replay {
        if sock.send(Message::text(frame)).await.is_err() {
            return;
        }
    }

    let mut ping = tokio::time::interval(Duration::from_secs(25));
    loop {
        tokio::select! {
            ev = events.recv() => match ev {
                Ok(line) => {
                    if sock.send(Message::text(line.to_string())).await.is_err() { break }
                    if line.starts_with(r#"{"type":"oven_exit""#) { break }
                }
                Err(RecvError::Lagged(n)) => {
                    let f = serde_json::json!({ "type": "oven_lagged", "missed": n });
                    if sock.send(Message::text(f.to_string())).await.is_err() { break }
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
                    }
                    if ok && !rt.send(t.to_string()).await { break }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
            _ = ping.tick() => {
                if sock.send(Message::Ping(Vec::new().into())).await.is_err() { break }
            }
        }
    }
}
