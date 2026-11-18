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
    // a big frame (a long history's replay) gets more time: a second for every 128 KB on top, as a slow phone link needs
    let len = match &msg {
        Message::Text(t) => t.len(),
        Message::Binary(b) => b.len(),
        _ => 0,
    };
    let extra = Duration::from_secs((len / (128 * 1024)) as u64);
    matches!(tokio::time::timeout(SEND_LIMIT + extra, sock.send(msg)).await, Ok(Ok(())))
}

/// What a page already has of the history: `n` messages, the last one at `ts` (pi's own timestamp).
#[derive(Clone, Copy)]
struct Since {
    n: usize,
    ts: u64,
}

/// A response line's request id (pi puts it first) and whether the line is a response at all.
fn response_id(line: &str) -> Option<Option<&str>> {
    if let Some(rest) = line.strip_prefix(r#"{"type":"response""#) {
        // the id may come after the type (not from pi itself, but nothing should hang on the order)
        let head = &rest[..rest.len().min(160)];
        return Some(head.find(r#","id":""#).and_then(|i| {
            let id = &rest[i + 7..];
            id.find('"').map(|e| &id[..e])
        }));
    }
    let rest = line.strip_prefix(r#"{"id":""#)?;
    let end = rest.find('"')?;
    rest[end..].starts_with(r#"","type":"response""#).then_some(Some(&rest[..end]))
}

/// pi's whole history, cut down to what the page does not have yet: when its `n`-th message is the one the page has
/// last (same timestamp), only the messages after it go, with `from: n`; otherwise (compacted, another branch, a gap)
/// all of it, as asked.
fn history_after(line: &str, since: Since) -> Option<String> {
    let mut v: serde_json::Value = serde_json::from_str(line).ok()?;
    let msgs = v["data"]["messages"].as_array_mut()?;
    if since.n == 0 || since.n > msgs.len() || msgs[since.n - 1]["timestamp"].as_u64() != Some(since.ts) {
        return None;
    }
    let tail: Vec<serde_json::Value> = msgs.drain(since.n..).collect();
    v["data"] = serde_json::json!({ "messages": tail, "from": since.n });
    Some(v.to_string())
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

    // The requests this page sent and is waiting for: pi's answers go to every page of the session, and one page's
    // history (megabytes) is no business of another's. `Some(since)`: a history request that only needs the rest.
    let mut mine: std::collections::HashMap<String, Option<Since>> = std::collections::HashMap::new();
    let mut ping = tokio::time::interval(Duration::from_secs(25));
    loop {
        tokio::select! {
            ev = events.recv() => match ev {
                Ok(line) => {
                    let mut out: Option<String> = None;
                    if let Some(id) = response_id(&line) {
                        let Some(since) = id.and_then(|id| mine.remove(id)) else { continue };
                        if let Some(since) = since {
                            out = history_after(&line, since);
                        }
                    }
                    let text = out.unwrap_or_else(|| line.to_string());
                    if !send(&mut sock, Message::text(text)).await { break }
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
                    let mut parsed = serde_json::from_str::<serde_json::Value>(&t).ok();
                    let ok = parsed.as_ref().is_some_and(|v| v.get("type").is_some_and(|t| t.is_string()));
                    let mut forward = t.to_string();
                    if let Some(v) = parsed.as_mut() {
                        if let Some(id) = v["id"].as_str().filter(|id| id.len() <= 64).map(str::to_owned) {
                            // a history request may say what the page has; pi is not told (it answers with all of it)
                            let since = v.as_object_mut().and_then(|o| o.remove("oven_since")).and_then(|s| {
                                Some(Since { n: s["n"].as_u64()? as usize, ts: s["ts"].as_u64()? })
                            });
                            if since.is_some() {
                                forward = v.to_string();
                            }
                            if mine.len() < 1000 {
                                mine.insert(id, since.filter(|_| v["type"] == "get_messages"));
                            }
                        }
                        if v["type"] == "extension_ui_response" {
                            if let Some(id) = v["id"].as_str() { rt.dialog_answered(id); }
                        }
                        if v["type"] == "set_model" {
                            if let (Some(p), Some(m)) = (v["provider"].as_str(), v["modelId"].as_str()) { rt.model_chosen(p, m); }
                        }
                    }
                    if ok && !rt.send(forward).await { break }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_a_response_and_its_id() {
        assert_eq!(response_id(r#"{"id":"ab-1","type":"response","command":"get_state"}"#), Some(Some("ab-1")));
        assert_eq!(response_id(r#"{"type":"response","command":"set_model"}"#), Some(None));
        assert_eq!(response_id(r#"{"type":"response","id":"z-9","command":"get_state"}"#), Some(Some("z-9")));
        assert_eq!(response_id(r#"{"type":"message_end","id":"x"}"#), None);
        assert_eq!(response_id(r#"{"id":"x","type":"extension_ui_request"}"#), None);
    }

    #[test]
    fn sends_only_the_history_the_page_lacks() {
        let line = r#"{"id":"a","type":"response","command":"get_messages","success":true,"data":{"messages":[{"role":"user","timestamp":1},{"role":"assistant","timestamp":2},{"role":"user","timestamp":3}]}}"#;
        let v: serde_json::Value = serde_json::from_str(&history_after(line, Since { n: 2, ts: 2 }).unwrap()).unwrap();
        assert_eq!(v["data"]["from"], 2);
        assert_eq!(v["data"]["messages"].as_array().unwrap().len(), 1);
        assert_eq!(v["data"]["messages"][0]["timestamp"], 3);
        assert_eq!(v["id"], "a");
        // nothing new
        let v: serde_json::Value = serde_json::from_str(&history_after(line, Since { n: 3, ts: 3 }).unwrap()).unwrap();
        assert_eq!(v["data"]["messages"].as_array().unwrap().len(), 0);
        // not the same history (compacted, another branch, a gap): all of it
        assert!(history_after(line, Since { n: 2, ts: 9 }).is_none());
        assert!(history_after(line, Since { n: 5, ts: 3 }).is_none());
        assert!(history_after(line, Since { n: 0, ts: 0 }).is_none());
    }
}
