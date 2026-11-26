//! Sessions pinned to the list's Open tab: they stay there, closed or not, on every device (one file for the server).
//!
//! Only the session files' paths are kept; nothing is read from them here. A pin whose file is gone is not shown
//! (the page matches pins against the session list) and goes the next time pins change.

use axum::{http::StatusCode, response::{IntoResponse, Response}, Json};
use std::sync::Mutex;

use crate::api::fail;

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
    let dir = crate::config::cache_dir();
    let tmp = dir.join(format!("{FILE}.tmp"));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, serde_json::to_vec(pins).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join(FILE)).map_err(|e| e.to_string())
}

pub async fn get() -> Json<Vec<String>> {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    Json(read())
}

#[derive(serde::Deserialize)]
pub struct Set {
    path: String,
    pinned: bool,
}

/// Pin or unpin one session; answers with every pin, newest pin last.
pub async fn set(Json(req): Json<Set>) -> Response {
    if req.path.is_empty() || req.path.len() > 4096 || !req.path.ends_with(".jsonl") {
        return fail(StatusCode::BAD_REQUEST, "bad path");
    }
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut pins = read();
    pins.retain(|p| p != &req.path && std::path::Path::new(p).exists());
    if req.pinned {
        if pins.len() >= MAX {
            return fail(StatusCode::UNPROCESSABLE_ENTITY, "too many pinned sessions");
        }
        pins.push(req.path);
    }
    match write(&pins) {
        Ok(()) => Json(pins).into_response(),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}
