//! Add-on for the pi package `pi-lens`: whether it may add its findings to each request (Settings → Auto → pi-lens).
//!
//! pi-lens appends its findings (diagnostics after an edit, test results, nudges, and the same blockers again at
//! every prompt while they stand) to the end of a request and takes them out of the next one. The prompt cache then
//! never finds the conversation it wrote: on the test server, 84 replies in a week wrote the whole conversation again,
//! about a third of the week's plan. With this off pi starts with `PI_LENS_NO_CONTEXT_INJECTION=1`; pi-lens keeps its
//! tools, LSP, read guard and formatting, and still reports on the edit itself in the tool's result.
//!
//! Off by default. pi reads the variable when it starts, so a change applies to sessions started after it. A value
//! already in the server's environment wins and the switch only shows it. Without pi-lens the variable does nothing,
//! and the page shows the switch only to a session that has pi-lens's commands.

use axum::{extract::State, http::StatusCode, response::{IntoResponse, Response}, Json};
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};

use crate::{api::fail, AppState};

pub const VAR: &str = "PI_LENS_NO_CONTEXT_INJECTION";
const FILE: &str = "lens-context";

pub struct Lens {
    inject: AtomicBool,
}

impl Default for Lens {
    fn default() -> Self {
        let on = std::fs::read_to_string(crate::config::cache_dir().join(FILE)).is_ok_and(|v| v.trim() == "on");
        Self { inject: AtomicBool::new(on) }
    }
}

impl Lens {
    /// What the server was started with, if anything (then the switch does nothing).
    fn from_env() -> Option<String> {
        std::env::var(VAR).ok().filter(|v| !v.is_empty())
    }

    /// The value to give a new pi, or `None` to leave its environment as it is.
    pub fn for_pi(&self) -> Option<&'static str> {
        (Self::from_env().is_none() && !self.inject.load(Ordering::Relaxed)).then_some("1")
    }

    fn view(&self) -> serde_json::Value {
        match Self::from_env() {
            Some(v) => serde_json::json!({ "inject": !matches!(v.as_str(), "1" | "true"), "fromEnv": true }),
            None => serde_json::json!({ "inject": self.inject.load(Ordering::Relaxed), "fromEnv": false }),
        }
    }
}

#[derive(serde::Deserialize)]
pub struct Set {
    inject: bool,
}

pub async fn get(State(app): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(app.runtimes.lens.view())
}

pub async fn set(State(app): State<Arc<AppState>>, Json(req): Json<Set>) -> Response {
    let l = &app.runtimes.lens;
    l.inject.store(req.inject, Ordering::Relaxed);
    let dir = crate::config::cache_dir();
    if std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join(FILE), if req.inject { "on" } else { "off" })).is_err() {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, "could not save the setting");
    }
    Json(l.view()).into_response()
}
