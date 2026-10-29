use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::{path::PathBuf, sync::Arc};

use crate::{config::home_dir, fs, runtime::SpawnRequest, AppState};

pub fn fail(code: StatusCode, msg: impl ToString) -> Response {
    (code, Json(json!({ "error": msg.to_string() }))).into_response()
}

pub async fn info(State(app): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "home": home_dir(),
        "sessionsDir": app.sessions_dir,
        "piBin": app.pi_bin,
    }))
}

pub async fn sessions(State(app): State<Arc<AppState>>) -> Response {
    let app2 = app.clone();
    match tokio::task::spawn_blocking(move || app2.sessions.list(&app2.sessions_dir)).await {
        Ok(list) => Json(list).into_response(),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(Deserialize)]
pub struct PathQuery {
    path: Option<String>,
    hidden: Option<bool>,
}

/// Only files inside the sessions directory may be deleted.
pub async fn delete_session(State(app): State<Arc<AppState>>, Query(q): Query<PathQuery>) -> Response {
    let Some(path) = q.path else { return fail(StatusCode::BAD_REQUEST, "path is required") };
    let (Ok(file), Ok(root)) = (PathBuf::from(&path).canonicalize(), app.sessions_dir.canonicalize()) else {
        return fail(StatusCode::NOT_FOUND, "session not found");
    };
    if !file.starts_with(&root) || file.extension().and_then(|e| e.to_str()) != Some("jsonl") {
        return fail(StatusCode::FORBIDDEN, "not a session file");
    }
    if app.runtimes.list().await.iter().any(|r| r.session_path.as_deref() == Some(path.as_str())) {
        return fail(StatusCode::CONFLICT, "session is open — close it first");
    }
    match std::fs::remove_file(&file) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

pub async fn list_runtimes(State(app): State<Arc<AppState>>) -> impl IntoResponse {
    Json(app.runtimes.list().await)
}

pub async fn create_runtime(State(app): State<Arc<AppState>>, Json(req): Json<SpawnRequest>) -> Response {
    match app.runtimes.spawn(req).await {
        Ok(rt) => Json(&rt.info).into_response(),
        Err(e) => fail(StatusCode::BAD_REQUEST, format!("{e:#}")),
    }
}

pub async fn delete_runtime(State(app): State<Arc<AppState>>, Path(id): Path<String>) -> StatusCode {
    if app.runtimes.close(&id).await {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RestartBody {
    session_path: Option<String>,
}

pub async fn restart_runtime(
    State(app): State<Arc<AppState>>,
    Path(id): Path<String>,
    body: Option<Json<RestartBody>>,
) -> Response {
    let session_path = body.and_then(|b| b.0.session_path);
    match app.runtimes.restart(&id, session_path).await {
        Ok(rt) => Json(&rt.info).into_response(),
        Err(e) => fail(StatusCode::BAD_REQUEST, format!("{e:#}")),
    }
}

/// pi-background-tasks keeps one folder per pi process: `<project>/.pi/tasks/<session-id>-<pid>/`.
/// The pid is the one we spawned, so the folder is found without guessing.
fn task_dirs(cwd: &str, pid: u32) -> Vec<PathBuf> {
    let suffix = format!("-{pid}");
    std::fs::read_dir(PathBuf::from(cwd).join(".pi/tasks"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir() && e.file_name().to_string_lossy().ends_with(&suffix))
        .map(|e| e.path())
        .collect()
}

const TASK_FIELDS: [&str; 12] = [
    "id", "name", "command", "description", "status", "startTime", "endTime", "exitCode", "bytesWritten", "isAgent", "error", "model",
];

pub async fn tasks(State(app): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let Some(rt) = app.runtimes.get(&id).await else { return fail(StatusCode::NOT_FOUND, "no such runtime") };
    let Some(pid) = rt.info.pid else { return Json(Vec::<serde_json::Value>::new()).into_response() };
    let cwd = rt.info.cwd.clone();
    let list = tokio::task::spawn_blocking(move || {
        let mut out: Vec<serde_json::Value> = Vec::new();
        for dir in task_dirs(&cwd, pid) {
            for f in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let name = f.file_name().to_string_lossy().into_owned();
                if !name.ends_with(".json") || name.ends_with(".attestation.json") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(f.path()) else { continue };
                let Ok(serde_json::Value::Object(full)) = serde_json::from_str(&text) else { continue };
                // Only what the UI shows; paths and internals stay on the server.
                let slim: serde_json::Map<_, _> =
                    TASK_FIELDS.iter().filter_map(|k| full.get(*k).map(|v| ((*k).to_owned(), v.clone()))).collect();
                out.push(slim.into());
            }
        }
        out.sort_by_key(|t| std::cmp::Reverse(t["startTime"].as_u64().unwrap_or(0)));
        out
    })
    .await
    .unwrap_or_default();
    Json(list).into_response()
}

#[derive(Deserialize)]
pub struct TailQuery {
    tail: Option<u64>,
}

pub async fn task_output(
    State(app): State<Arc<AppState>>,
    Path((id, task)): Path<(String, String)>,
    Query(q): Query<TailQuery>,
) -> Response {
    // A task id becomes part of a file name: letters, digits, '-', '_' only.
    if task.is_empty() || !task.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return fail(StatusCode::BAD_REQUEST, "bad task id");
    }
    let Some(rt) = app.runtimes.get(&id).await else { return fail(StatusCode::NOT_FOUND, "no such runtime") };
    let Some(pid) = rt.info.pid else { return fail(StatusCode::NOT_FOUND, "no tasks") };
    let want = q.tail.unwrap_or(32 * 1024).min(512 * 1024);
    let cwd = rt.info.cwd.clone();
    let res = tokio::task::spawn_blocking(move || {
        use std::io::{Read, Seek, SeekFrom};
        let path = task_dirs(&cwd, pid).into_iter().map(|d| d.join(format!("{task}.output"))).find(|p| p.is_file())?;
        let mut f = std::fs::File::open(path).ok()?;
        let len = f.metadata().ok()?.len();
        f.seek(SeekFrom::Start(len.saturating_sub(want))).ok()?;
        let mut buf = Vec::new();
        f.take(want).read_to_end(&mut buf).ok()?;
        Some((len, String::from_utf8_lossy(&buf).into_owned()))
    })
    .await
    .ok()
    .flatten();
    match res {
        Some((total, text)) => ([("x-total-bytes", total.to_string()), ("content-type", "text/plain; charset=utf-8".to_owned())], text).into_response(),
        None => fail(StatusCode::NOT_FOUND, "no output yet"),
    }
}

pub async fn browse(Query(q): Query<PathQuery>) -> Response {
    let path = q.path.map(PathBuf::from).unwrap_or_else(home_dir);
    let hidden = q.hidden.unwrap_or(false);
    let p2 = path.clone();
    // A slow disk or network folder must not hold up the other requests.
    match tokio::task::spawn_blocking(move || fs::list_dirs(&p2, hidden)).await {
        Ok(Ok(l)) => Json(l).into_response(),
        Ok(Err(e)) => fail(StatusCode::BAD_REQUEST, format!("{}: {e}", path.display())),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// Server-sent events: what the desktop pet should look like, for every open session. A pet that
/// connects is told the current state first, then each change as it happens.
pub async fn pet_stream(
    State(app): State<Arc<AppState>>,
) -> axum::response::Sse<impl futures_util::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>> {
    use axum::response::sse::{Event, KeepAlive};
    use futures_util::StreamExt;
    use tokio_stream::wrappers::BroadcastStream;

    // Subscribe before reading the snapshot, so nothing that happens in between is lost.
    let live = BroadcastStream::new(app.runtimes.pet.subscribe()).filter_map(|r| async move { r.ok() });
    let first = futures_util::stream::iter(app.runtimes.pet.snapshot());
    let events = first.chain(live).map(|data| Ok(Event::default().data(data)));
    axum::response::Sse::new(events).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
}
