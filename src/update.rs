//! Updates for pi and its packages, from the side panel instead of a dialog in every new session.
//!
//! The `pi-updater` package checks at each session start and asks with `ctx.ui.select`. Over RPC its answer cannot
//! work: the install runs inside `ctx.ui.custom`, a terminal-only call. So the server answers that one prompt itself
//! ("cancelled", the same as Skip), keeps what it offered, and runs `pi update` on request. A prompt that does not look
//! exactly like pi-updater's goes to the browser as before.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::{
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::process::Command;

use crate::{api::fail, config::home_dir, AppState};

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    /// pi itself: `[current, latest]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pi: Option<[String; 2]>,
    pub extensions: Vec<String>,
}

impl Offer {
    fn is_empty(&self) -> bool {
        self.pi.is_none() && self.extensions.is_empty()
    }
}

#[derive(Default)]
pub struct Updates {
    offer: Mutex<Option<(Offer, u64)>>,
    running: tokio::sync::Mutex<()>,
}

/// An offer this old is dropped: a pi that finds nothing new says nothing, so an old one is never replaced.
const STALE_MS: u64 = 12 * 3600 * 1000;
const RUN_TIMEOUT: Duration = Duration::from_secs(300);
const OUTPUT_TAIL: usize = 4000;

#[derive(Deserialize)]
struct Prompt {
    id: String,
    method: String,
    title: String,
    #[serde(default)]
    options: Vec<String>,
}

fn list(s: &str) -> Vec<String> {
    s.split(", ").map(str::trim).filter(|x| !x.is_empty()).map(str::to_owned).collect()
}

fn versions(s: &str) -> Option<[String; 2]> {
    let (cur, latest) = s.split_once(" → ")?;
    let ok = |v: &str| !v.is_empty() && !v.contains(char::is_whitespace);
    (ok(cur) && ok(latest)).then(|| [cur.to_owned(), latest.to_owned()])
}

/// pi-updater's prompt, recognised by its title and its exact options. Returns the request id and what it offers.
pub fn from_prompt(line: &str) -> Option<(String, Offer)> {
    let p: Prompt = serde_json::from_str(line).ok()?;
    if p.method != "select" {
        return None;
    }
    let opts: Vec<&str> = p.options.iter().map(String::as_str).collect();
    let offer = if let Some(rest) = p.title.strip_prefix("Update pi ") {
        if opts != ["Update all", "Update pi only", "Update extensions only", "Skip"] {
            return None;
        }
        let (v, ext) = rest.split_once(" · extensions: ")?;
        Offer { pi: Some(versions(v)?), extensions: list(ext) }
    } else if let Some(rest) = p.title.strip_prefix("Extension updates available: ") {
        if opts != ["Update now", "Skip"] {
            return None;
        }
        Offer { pi: None, extensions: list(rest) }
    } else {
        let v = versions(p.title.strip_prefix("Update ")?)?;
        if opts != ["Update now".to_owned(), "Skip".to_owned(), format!("Ignore {}", v[1])] {
            return None;
        }
        Offer { pi: Some(v), extensions: vec![] }
    };
    (!offer.is_empty()).then_some((p.id, offer))
}

impl Updates {
    pub fn offered(&self, offer: Offer, now: u64) {
        *self.offer.lock().unwrap() = Some((offer, now));
    }

    fn current(&self, now: u64) -> Option<Offer> {
        let mut g = self.offer.lock().unwrap();
        if g.as_ref().is_some_and(|(_, at)| now.saturating_sub(*at) > STALE_MS) {
            *g = None;
        }
        g.as_ref().map(|(o, _)| o.clone())
    }

    fn installed(&self, pi: bool, extensions: bool) {
        let mut g = self.offer.lock().unwrap();
        if let Some((o, _)) = g.as_mut() {
            if pi {
                o.pi = None;
            }
            if extensions {
                o.extensions.clear();
            }
            if o.is_empty() {
                *g = None;
            }
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    offer: Option<Offer>,
    running: bool,
}

pub async fn status(State(app): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let u = &app.runtimes.updates;
    let s = Status { offer: u.current(crate::runtime::now_ms()), running: u.running.try_lock().is_err() };
    Json(serde_json::to_value(s).unwrap_or_default())
}

#[derive(Deserialize)]
pub struct Run {
    /// "pi", "extensions" or "all"
    target: String,
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            if it.peek() == Some(&'[') {
                it.next();
                for d in it.by_ref() {
                    if ('@'..='~').contains(&d) {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn tail(s: &str) -> String {
    let s = s.trim();
    match s.char_indices().rev().nth(OUTPUT_TAIL) {
        Some((i, _)) => format!("…{}", &s[i..]),
        None => s.to_owned(),
    }
}

/// Runs `pi update` with fixed arguments from the home folder (so no project's own packages are involved) and no
/// stdin (so a question fails instead of waiting forever).
pub async fn run(State(app): State<Arc<AppState>>, Json(req): Json<Run>) -> Response {
    let (pi, ext) = match req.target.as_str() {
        "pi" => (true, false),
        "extensions" => (false, true),
        "all" => (true, true),
        _ => return fail(StatusCode::BAD_REQUEST, tr!("target은 pi, extensions, all 중 하나여야 합니다", "target must be pi, extensions or all")),
    };
    let u = &app.runtimes.updates;
    let Ok(_one) = u.running.try_lock() else { return fail(StatusCode::CONFLICT, tr!("이미 업데이트가 진행 중입니다", "an update is already running")) };

    let mut cmd = Command::new(&app.pi_bin);
    cmd.arg("update");
    if pi {
        cmd.arg("--self");
    }
    if ext {
        cmd.arg("--extensions");
    }
    cmd.current_dir(home_dir()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    tracing::info!(target = %req.target, "pi update");
    let out = match tokio::time::timeout(RUN_TIMEOUT, cmd.output()).await {
        Err(_) => return fail(StatusCode::GATEWAY_TIMEOUT, tr!("pi update가 5분 안에 끝나지 않았습니다", "pi update did not finish in 5 minutes")),
        Ok(Err(e)) => return fail(StatusCode::INTERNAL_SERVER_ERROR, tr!(format!("{}을(를) 실행하지 못했습니다: {e}", app.pi_bin), format!("could not start {}: {e}", app.pi_bin))),
        Ok(Ok(o)) => o,
    };
    let text = strip_ansi(&format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)));
    if out.status.success() {
        u.installed(pi, ext);
    } else {
        tracing::warn!(code = ?out.status.code(), "pi update failed");
    }
    Json(serde_json::json!({ "ok": out.status.success(), "code": out.status.code(), "output": tail(&text) })).into_response()
}


#[cfg(test)]
mod tests {
    use super::*;

    fn req(title: &str, options: &[&str]) -> String {
        serde_json::json!({ "type": "extension_ui_request", "id": "u1", "method": "select", "title": title, "options": options }).to_string()
    }

    #[test]
    fn reads_the_three_pi_updater_prompts() {
        let both = req("Update pi 1.4.0 → 1.5.0 · extensions: pi-lens, pi-web-access", &["Update all", "Update pi only", "Update extensions only", "Skip"]);
        assert_eq!(
            from_prompt(&both),
            Some(("u1".into(), Offer { pi: Some(["1.4.0".into(), "1.5.0".into()]), extensions: vec!["pi-lens".into(), "pi-web-access".into()] }))
        );
        let pi = req("Update 1.4.0 → 1.5.0", &["Update now", "Skip", "Ignore 1.5.0"]);
        assert_eq!(from_prompt(&pi).unwrap().1, Offer { pi: Some(["1.4.0".into(), "1.5.0".into()]), extensions: vec![] });
        let ext = req("Extension updates available: @a/b", &["Update now", "Skip"]);
        assert_eq!(from_prompt(&ext).unwrap().1, Offer { pi: None, extensions: vec!["@a/b".into()] });
    }

    #[test]
    fn leaves_other_prompts_alone() {
        assert_eq!(from_prompt(&req("Update 1.4.0 → 1.5.0", &["Update now", "Skip"])), None);
        assert_eq!(from_prompt(&req("Update the config?", &["Update now", "Skip", "Ignore x"])), None);
        assert_eq!(from_prompt(&req("Extension updates available: x", &["Yes", "No"])), None);
        assert_eq!(from_prompt(&req("Pick one", &["a", "b"])), None);
        let confirm = serde_json::json!({ "type": "extension_ui_request", "id": "u", "method": "confirm", "title": "Update 1 → 2", "message": "" });
        assert_eq!(from_prompt(&confirm.to_string()), None);
    }

    #[test]
    fn an_install_clears_what_it_covered() {
        let u = Updates::default();
        u.offered(Offer { pi: Some(["1".into(), "2".into()]), extensions: vec!["x".into()] }, 0);
        u.installed(false, true);
        assert_eq!(u.current(1).unwrap().extensions.len(), 0);
        u.installed(true, false);
        assert_eq!(u.current(1), None);
        u.offered(Offer { pi: None, extensions: vec!["x".into()] }, 0);
        assert_eq!(u.current(STALE_MS + 1), None);
    }

    #[test]
    fn output_is_plain_and_short() {
        assert_eq!(strip_ansi("\u{1b}[32mok\u{1b}[0m done"), "ok done");
        assert!(tail(&"가".repeat(10_000)).chars().count() <= OUTPUT_TAIL + 2);
    }
}
