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

pub async fn handler(State(app): State<Arc<AppState>>, Path(id): Path<String>, current: Option<axum::Extension<crate::auth::Current>>, ws: WebSocketUpgrade) -> Response {
    // the session that opened it (none on a server without secrets): when that session is ended, the socket closes
    let session = current.map(|c| c.0 .0).filter(|s| !s.is_empty());
    match app.runtimes.get(&id).await {
        Some(rt) => ws.max_message_size(32 << 20).on_upgrade(move |sock| attach(sock, rt, app, session)).into_response(),
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

/// What part of pi's history a page asked for. pi always answers with all of it (megabytes, for a long session), which
/// a slow link cannot take in one go: the page gets only its newest messages up to a size, and older ones as it
/// scrolls up.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Want {
    /// The newest messages, up to about `bytes`.
    Tail { bytes: usize },
    /// What came after the page's `n` messages (counted from the start), the last of which is at `ts` (pi's own
    /// timestamp); the newest up to `bytes` when the history no longer starts the same (compacted, another branch).
    Since { n: usize, ts: u64, bytes: usize },
    /// Older messages, up to about `bytes`, before the page's first one: the `n`-th (counted from 0), at `ts`.
    Before { n: usize, ts: u64, bytes: usize },
}

impl Want {
    fn from_request(v: &mut serde_json::Value) -> Option<Self> {
        let o = v.as_object_mut()?;
        let tail = o.remove("oven_tail");
        let since = o.remove("oven_since");
        let before = o.remove("oven_before");
        // a page that does not say how much takes all of it (as it always did)
        let raw_bytes = tail
            .as_ref()
            .and_then(|t| t["bytes"].as_u64())
            .or_else(|| before.as_ref().and_then(|b| b["bytes"].as_u64()))
            .or_else(|| since.as_ref().and_then(|s| s["bytes"].as_u64()));
        let bytes = raw_bytes.map_or(usize::MAX, |b| (b as usize).clamp(16 << 10, 64 << 20));
        let pair = |s: &serde_json::Value| Some((s["n"].as_u64()? as usize, s["ts"].as_u64()?));
        if let Some((n, ts)) = before.as_ref().and_then(pair) {
            return Some(Want::Before { n, ts, bytes });
        }
        if let Some((n, ts)) = since.as_ref().and_then(pair) {
            return Some(Want::Since { n, ts, bytes });
        }
        tail.is_some().then_some(Want::Tail { bytes })
    }
}

/// A response line's request id (pi puts it first) and whether the line is a response at all.
pub(crate) fn response_id(line: &str) -> Option<Option<&str>> {
    if let Some(rest) = line.strip_prefix(r#"{"type":"response""#) {
        // the id may come after the type (not from pi itself, but nothing should hang on the order)
        // (160 bytes may end inside a character: a model's name in a set_model answer can be Korean)
        let mut n = rest.len().min(160);
        while !rest.is_char_boundary(n) {
            n -= 1;
        }
        let head = &rest[..n];
        return Some(head.find(r#","id":""#).and_then(|i| {
            let id = &rest[i + 7..];
            id.find('"').map(|e| &id[..e])
        }));
    }
    let rest = line.strip_prefix(r#"{"id":""#)?;
    let end = rest.find('"')?;
    rest[end..].starts_with(r#"","type":"response""#).then_some(Some(&rest[..end]))
}

fn is_user(m: &serde_json::Value) -> bool {
    m["role"] == "user"
}

/// Where a page's share of `msgs[..end]` starts: as far back as fits in `bytes` (at least one message), then moved to
/// where a turn starts (a person's message), so a reply is never shown without what it answered or a tool's result
/// without its call. A last turn larger than `bytes` comes whole.
fn cut(msgs: &[serde_json::Value], end: usize, bytes: usize) -> usize {
    if bytes == usize::MAX {
        return 0;
    }
    let (mut s, mut size) = (end, 0usize);
    while s > 0 {
        let len = msgs[s - 1].to_string().len();
        if s < end && size + len > bytes {
            break;
        }
        size += len;
        s -= 1;
    }
    if s == 0 {
        return 0;
    }
    (s..end).find(|&i| is_user(&msgs[i])).or_else(|| (0..s).rev().find(|&i| is_user(&msgs[i]))).unwrap_or(0)
}

/// The first thing the person said, for the session's title when that message is not among those sent.
fn first_said(msgs: &[serde_json::Value]) -> Option<String> {
    let m = msgs.iter().find(|m| is_user(m))?;
    let text = match &m["content"] {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(parts) => parts.iter().find(|p| p["type"] == "text").and_then(|p| p["text"].as_str()).unwrap_or("").to_owned(),
        _ => String::new(),
    };
    Some(text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").chars().take(200).collect())
}

/// pi's whole history, cut down to the part the page asked for. The answer says where that part starts in the
/// history (`from`), how long the history is (`total`), how many of the person's messages come before it
/// (`usersBefore`, to address pi's fork points), and `rest` when it is what follows what the page has. An older
/// part asked of a history that no longer starts the same says `reset` (the page loads its newest part again).
fn shape(line: &str, want: Want) -> Option<String> {
    let mut v: serde_json::Value = serde_json::from_str(line).ok()?;
    let data = v.get_mut("data")?.as_object_mut()?;
    let msgs = std::mem::take(data.get_mut("messages")?.as_array_mut()?);
    let total = msgs.len();
    let at = |n: usize, ts: u64| msgs.get(n).and_then(|m| m["timestamp"].as_u64()) == Some(ts);
    let (start, end, rest) = match want {
        Want::Tail { bytes } => (cut(&msgs, total, bytes), total, false),
        Want::Since { n, ts, bytes } => {
            if n > 0 && n <= total && at(n - 1, ts) {
                (n, total, true)
            } else {
                (cut(&msgs, total, bytes), total, false)
            }
        }
        Want::Before { n, ts, bytes } => {
            if n < total && at(n, ts) {
                (cut(&msgs, n, bytes), n, false)
            } else {
                data.insert("reset".into(), true.into());
                (total, total, false)
            }
        }
    };
    if start > 0 {
        if let Some(first) = first_said(&msgs[..start]) {
            data.insert("firstUser".into(), first.into());
        }
    }
    data.insert("from".into(), start.into());
    data.insert("total".into(), total.into());
    data.insert("usersBefore".into(), msgs[..start].iter().filter(|m| is_user(m)).count().into());
    if rest {
        data.insert("rest".into(), true.into());
    }
    let part: Vec<serde_json::Value> = msgs.into_iter().take(end).skip(start).collect();
    data.insert("messages".into(), part.into());
    Some(v.to_string())
}

/// The next word of what changed in the folder (never, once the watcher is gone or there is none).
async fn changed(rx: &mut Option<tokio::sync::broadcast::Receiver<Arc<str>>>) -> Result<Arc<str>, RecvError> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

async fn attach(mut sock: WebSocket, rt: Arc<Runtime>, app: Arc<crate::AppState>, session: Option<String>) {
    let mut revoked = app.auth.revoked();
    // ended between the guard and here: the notice above came too late for this socket
    if session.as_ref().is_some_and(|s| !app.auth.alive(s)) {
        return;
    }
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
    // history (megabytes) is no business of another's. `Some(want)`: a history request, and the part of it the page wants.
    let mut mine: std::collections::HashMap<String, Option<Want>> = std::collections::HashMap::new();
    let mut mine_order: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    // (resolving the folder may touch a slow disk; the handle keeps the watcher alive as long as this page looks)
    let cwd = std::path::PathBuf::from(&rt.info.cwd);
    let hub = tokio::task::spawn_blocking(move || app.watches.of(&cwd)).await.ok().flatten();
    let mut files = hub.as_ref().map(|h| h.subscribe());
    let mut ping = tokio::time::interval(Duration::from_secs(25));
    loop {
        tokio::select! {
            ev = events.recv() => match ev {
                Ok(line) => {
                    let mut out: Option<String> = None;
                    if let Some(id) = response_id(&line) {
                        let Some(want) = id.and_then(|id| mine.remove(id)) else { continue };
                        if let Some(want) = want {
                            // megabytes of JSON parsed and written again: not on the async worker
                            let l = line.clone();
                            out = tokio::task::spawn_blocking(move || shape(&l, want)).await.ok().flatten();
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
                            // a history request may say which part the page wants; pi is not told (it answers with all of it)
                            let want = if v["type"] == "get_messages" { Want::from_request(v) } else { None };
                            if want.is_some() {
                                forward = v.to_string();
                            }
                            // pi does not answer a dialog's answer: waiting for one would only fill the table
                            if v["type"] != "extension_ui_response" {
                                while mine.len() >= 1000 {
                                    let Some(oldest) = mine_order.pop_front() else { break };
                                    mine.remove(&oldest);
                                }
                                if mine.insert(id.clone(), want).is_none() {
                                    mine_order.push_back(id);
                                }
                            }
                        }
                        if v["type"] == "extension_ui_response" {
                            if let Some(id) = v["id"].as_str() { rt.dialog_answered(id); }
                        }
                        if v["type"] == "set_model" {
                            if let (Some(p), Some(m)) = (v["provider"].as_str(), v["modelId"].as_str()) { rt.model_chosen(p, m); }
                        }
                        // a message to a plan that is used up is held back, unless the person says send it anyway
                        if v["type"] == "prompt" {
                            let forced = v.as_object_mut().and_then(|o| o.remove("oven_force")).is_some_and(|f| f == true);
                            if forced {
                                forward = v.to_string();
                            } else if !v["message"].as_str().is_some_and(|m| m.trim_start().starts_with('/')) {
                                // Zero-Waste keeps it and sends it when the limit resets (the strip shows it)
                                if let Some(held) = rt.limit_hold().filter(|h| rt.hold(v.clone(), h)) {
                                    let mut r = serde_json::json!({ "type": "response", "command": "prompt", "success": true, "data": { "held": held } });
                                    if let Some(id) = v["id"].as_str() {
                                        mine.remove(id); // pi gets it later without its id: no answer to wait for
                                        r["id"] = id.into();
                                    }
                                    if !send(&mut sock, Message::text(r.to_string())).await { break }
                                    continue;
                                }
                            }
                        }
                    }
                    let kind = parsed.as_ref().and_then(|v| v["type"].as_str()).unwrap_or("");
                    if matches!(kind, "prompt" | "steer" | "follow_up") { rt.page_sent() }
                    // pi refuses a prompt while our compaction runs: it goes when that is over (not waited for here: this loop carries events and pings)
                    if ok && kind == "prompt" && rt.is_compacting() {
                        rt.send_after_compaction(forward);
                    } else if ok && !rt.send(forward).await { break }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
            f = changed(&mut files) => match f {
                Ok(line) => { if !send(&mut sock, Message::text(line.to_string())).await { break } }
                // some were missed: the page looks at the folder itself
                Err(RecvError::Lagged(_)) => { if !send(&mut sock, Message::text(r#"{"type":"oven_files","files":[],"more":true}"#)).await { break } }
                Err(RecvError::Closed) => files = None,
            },
            gone = revoked.recv() => match gone {
                Ok(id) if Some(&id) == session.as_ref() => {
                    let _ = send(&mut sock, Message::Close(None)).await;
                    break;
                }
                // missed some: this session may be among them, and a page that is still signed in just reconnects
                Err(RecvError::Lagged(_)) if session.is_some() => break,
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
        // byte 160 inside a Korean character (3 bytes each) must not panic
        for pad in 0..3 {
            let line = format!(r#"{{"type":"response","command":"set_model","data":{{"name":"{}{}"}}}}"#, "a".repeat(pad), "모델".repeat(60));
            assert_eq!(response_id(&line), Some(None));
        }
    }

    fn shaped(line: &str, w: Want) -> serde_json::Value {
        serde_json::from_str(&shape(line, w).unwrap()).unwrap()
    }
    fn stamps(v: &serde_json::Value) -> Vec<u64> {
        v["data"]["messages"].as_array().unwrap().iter().map(|m| m["timestamp"].as_u64().unwrap()).collect()
    }

    #[test]
    fn sends_only_the_history_the_page_lacks() {
        let line = r#"{"id":"a","type":"response","command":"get_messages","success":true,"data":{"messages":[{"role":"user","timestamp":1},{"role":"assistant","timestamp":2},{"role":"user","timestamp":3}]}}"#;
        let all = usize::MAX;
        let v = shaped(line, Want::Since { n: 2, ts: 2, bytes: all });
        assert_eq!((v["data"]["from"].as_u64(), v["data"]["rest"].as_bool()), (Some(2), Some(true)));
        assert_eq!(stamps(&v), [3]);
        assert_eq!(v["id"], "a");
        // nothing new
        assert!(stamps(&shaped(line, Want::Since { n: 3, ts: 3, bytes: all })).is_empty());
        // not the same history (compacted, another branch, a gap): all of it
        for w in [Want::Since { n: 2, ts: 9, bytes: all }, Want::Since { n: 5, ts: 3, bytes: all }, Want::Since { n: 0, ts: 0, bytes: all }] {
            let v = shaped(line, w);
            assert_eq!((stamps(&v), v["data"]["from"].as_u64(), v["data"]["rest"].as_bool()), (vec![1, 2, 3], Some(0), None));
        }
    }

    #[test]
    fn a_long_history_goes_in_parts_from_the_newest() {
        // five turns of a person's message and a 1000-byte reply
        let big = "x".repeat(1000);
        let mut msgs = Vec::new();
        for t in 0..5u64 {
            msgs.push(serde_json::json!({ "role": "user", "timestamp": t * 10, "content": format!("q{t}") }));
            msgs.push(serde_json::json!({ "role": "assistant", "timestamp": t * 10 + 1, "content": big }));
            msgs.push(serde_json::json!({ "role": "toolResult", "timestamp": t * 10 + 2, "content": "ok" }));
        }
        let line = serde_json::json!({ "type": "response", "id": "a", "command": "get_messages", "success": true, "data": { "messages": msgs } }).to_string();
        // about two turns' worth: the part starts at a person's message
        let v = shaped(&line, Want::Tail { bytes: 2500 });
        assert_eq!(stamps(&v), [30, 31, 32, 40, 41, 42]);
        assert_eq!((v["data"]["from"].as_u64(), v["data"]["total"].as_u64(), v["data"]["usersBefore"].as_u64()), (Some(9), Some(15), Some(3)));
        assert_eq!(v["data"]["firstUser"], "q0");
        // then older, before the page's first message
        let v = shaped(&line, Want::Before { n: 9, ts: 30, bytes: 2500 });
        assert_eq!(stamps(&v), [10, 11, 12, 20, 21, 22]);
        assert_eq!(v["data"]["from"].as_u64(), Some(3));
        let v = shaped(&line, Want::Before { n: 3, ts: 10, bytes: 2500 });
        assert_eq!((stamps(&v), v["data"]["from"].as_u64(), v["data"].get("firstUser")), (vec![0, 1, 2], Some(0), None));
        // a history that changed under the page
        let v = shaped(&line, Want::Before { n: 9, ts: 31, bytes: 2500 });
        assert_eq!((v["data"]["reset"].as_bool(), stamps(&v).len()), (Some(true), 0));
        // a last turn larger than asked comes whole
        assert_eq!(stamps(&shaped(&line, Want::Tail { bytes: 100 })), [40, 41, 42]);
        // a page that does not say: all of it
        let mut req = serde_json::json!({ "type": "get_messages", "oven_since": { "n": 1, "ts": 0 } });
        assert_eq!(Want::from_request(&mut req), Some(Want::Since { n: 1, ts: 0, bytes: usize::MAX }));
        assert_eq!(req, serde_json::json!({ "type": "get_messages" }));
    }
}
