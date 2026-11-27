//! Sessions pinned to the list's Open tab: they stay there, closed or not, on every device (one file for the server).
//!
//! Only the session files' paths are kept; nothing is read from them here. A pin whose file is gone is not shown
//! (the page matches pins against the session list) and goes the next time pins change.

use axum::{extract::State, http::StatusCode, response::{IntoResponse, Response}, Json};
use std::{path::Path, sync::{Arc, Mutex}};

use crate::{api::fail, AppState};

const FILE: &str = "pinned-sessions.json";
const MAX: usize = 200;

static LOCK: Mutex<()> = Mutex::new(());

fn read() -> Vec<String> {
    std::fs::read_to_string(crate::config::cache_dir().join(FILE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write(pins: &[String]) -> Result<(), String> {
    let path = crate::config::cache_dir().join(FILE);
    let bytes = serde_json::to_vec(pins).map_err(|e| e.to_string())?;
    crate::config::atomic_write(&path, &bytes, false).map_err(|e| e.to_string())
}

fn allowed_session(app: &AppState, raw: &str) -> Result<String, (StatusCode, &'static str)> {
    let (Some(file), Ok(sessions_root)) = (crate::runtime::resolve_path(Path::new(raw)), app.sessions_dir.canonicalize()) else {
        return Err((StatusCode::BAD_REQUEST, "bad path"));
    };
    if !file.starts_with(&sessions_root) || file.extension().and_then(|e| e.to_str()) != Some("jsonl") {
        return Err((StatusCode::FORBIDDEN, "not a session file"));
    }
    if file.exists() && !crate::scope::allows_session(app.root.as_deref(), &file) {
        return Err((StatusCode::FORBIDDEN, "outside the allowed folder"));
    }
    Ok(file.to_string_lossy().into_owned())
}

pub async fn get(State(app): State<Arc<AppState>>) -> Response {
    match tokio::task::spawn_blocking(move || {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut pins = read();
        if let Ok(sessions_root) = app.sessions_dir.canonicalize() {
            pins.retain(|p| {
                let path = Path::new(p);
                path.starts_with(&sessions_root) && (!path.exists() || crate::scope::allows_session(app.root.as_deref(), path))
            });
        }
        pins
    })
    .await
    {
        Ok(pins) => Json(pins).into_response(),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(serde::Deserialize)]
pub struct Set {
    path: String,
    pinned: bool,
}

/// Pin or unpin one session; answers with every pin, newest pin last.
pub async fn set(State(app): State<Arc<AppState>>, Json(req): Json<Set>) -> Response {
    if req.path.is_empty() || req.path.len() > 4096 || !req.path.ends_with(".jsonl") {
        return fail(StatusCode::BAD_REQUEST, "bad path");
    }
    match tokio::task::spawn_blocking(move || -> Result<Vec<String>, (StatusCode, String)> {
        let target = allowed_session(&app, &req.path).map_err(|(c, m)| (c, m.to_owned()))?;
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut pins = read();
        pins.retain(|p| p != &req.path && p != &target && Path::new(p).exists());
        if req.pinned {
            if pins.len() >= MAX {
                return Err((StatusCode::UNPROCESSABLE_ENTITY, "too many pinned sessions".to_owned()));
            }
            pins.push(target);
        }
        write(&pins).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
        Ok(pins)
    })
    .await
    {
        Ok(Ok(pins)) => Json(pins).into_response(),
        Ok(Err((code, msg))) => fail(code, msg),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}
