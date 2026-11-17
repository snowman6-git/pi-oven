//! Claude's prompt cache for one hour instead of five minutes (Settings → Auto → Claude cache).
//!
//! pi keeps Claude's cache for 5 minutes unless it starts with `PI_CACHE_RETENTION=long`; then every cache entry
//! lives an hour, as in Claude Code. Writing an hour's entry costs more (2× input instead of 1.25×), but coming back
//! after a break of 5–60 minutes reads the conversation (0.1×) instead of writing it all again. On by default.
//!
//! pi reads the variable when it starts, so a change applies to the sessions opened after it. A value already in
//! the server's environment wins and the switch only shows it.

use axum::{extract::State, http::StatusCode, response::{IntoResponse, Response}, Json};
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};

use crate::{api::fail, AppState};

pub const VAR: &str = "PI_CACHE_RETENTION";
const FILE: &str = "claude-cache-1h";

pub struct CacheTtl {
    on: AtomicBool,
}

impl Default for CacheTtl {
    fn default() -> Self {
        let off = std::fs::read_to_string(crate::config::cache_dir().join(FILE)).is_ok_and(|v| v.trim() == "off");
        Self { on: AtomicBool::new(!off) }
    }
}

impl CacheTtl {
    /// What the server was started with, if anything (then the switch does nothing).
    fn from_env() -> Option<String> {
        std::env::var(VAR).ok().filter(|v| !v.is_empty())
    }

    /// The value to give a new pi, or `None` to leave its environment as it is.
    pub fn for_pi(&self) -> Option<&'static str> {
        (Self::from_env().is_none() && self.on.load(Ordering::Relaxed)).then_some("long")
    }

    fn view(&self) -> serde_json::Value {
        match Self::from_env() {
            Some(v) => serde_json::json!({ "enabled": v == "long", "fromEnv": true }),
            None => serde_json::json!({ "enabled": self.on.load(Ordering::Relaxed), "fromEnv": false }),
        }
    }
}

#[derive(serde::Deserialize)]
pub struct Set {
    enabled: bool,
}

pub async fn get(State(app): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(app.runtimes.cache_ttl.view())
}

pub async fn set(State(app): State<Arc<AppState>>, Json(req): Json<Set>) -> Response {
    let c = &app.runtimes.cache_ttl;
    c.on.store(req.enabled, Ordering::Relaxed);
    let dir = crate::config::cache_dir();
    if std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join(FILE), if req.enabled { "on" } else { "off" })).is_err() {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, "could not save the setting");
    }
    Json(c.view()).into_response()
}
