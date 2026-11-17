//! Background subagents (the pi-subagents package): how a run is going, read from the status file its runner keeps.
//!
//! A background run reports nothing to the parent pi until it ends; its runner writes `status.json` in the run's
//! folder (`$TMPDIR/pi-subagents-<scope>/async-subagent-runs/<id>/`, or under `PI_SUBAGENTS_TEMP_ROOT`). The page
//! learns the folder from the launch's tool result and asks here every couple of seconds while the run is going.
//! Only such a folder is read, only its `status.json`, and only the fields the page draws.

use axum::{extract::Query, http::StatusCode, response::{IntoResponse, Response}, Json};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::api::fail;

const RUNS: &str = "async-subagent-runs";
const MAX_FILE: u64 = 4 << 20;
const MAX_STEPS: usize = 64;

/// The run folder, resolved, if it is one of pi-subagents' (else `None`).
fn run_dir(dir: &Path, temp: &Path, root_env: Option<&Path>) -> Option<PathBuf> {
    let d = dir.canonicalize().ok()?;
    let runs = d.parent()?;
    if runs.file_name()? != RUNS {
        return None;
    }
    let root = runs.parent()?;
    let ok = match root_env {
        Some(r) => r.canonicalize().ok().as_deref() == Some(root),
        None => temp.canonicalize().ok().as_deref() == root.parent() && root.file_name()?.to_str()?.starts_with("pi-subagents-"),
    };
    ok.then_some(d)
}

fn cut(v: &Value, n: usize) -> Value {
    match v.as_str() {
        Some(s) if s.chars().count() > n => Value::String(s.chars().take(n).collect::<String>() + "…"),
        _ => v.clone(),
    }
}

/// What the page draws, and nothing else (the file also holds paths, pids and budgets).
fn trim(v: &Value) -> Value {
    let steps: Vec<Value> = v["steps"].as_array().map(|a| a.iter().take(MAX_STEPS).collect::<Vec<_>>()).unwrap_or_default().into_iter().map(|s| {
        json!({
            "agent": cut(&s["agent"], 80),
            "status": s["status"],
            "description": cut(&s["description"], 300),
            "label": cut(&s["label"], 80),
            "currentTool": cut(&s["currentTool"], 80),
            "currentToolArgs": cut(&s["currentToolArgs"], 200),
            "startedAt": s["startedAt"],
            "endedAt": s["endedAt"],
            "durationMs": s["durationMs"],
        })
    }).collect();
    json!({
        "runId": cut(&v["runId"], 120),
        "state": v["state"],
        "mode": v["mode"],
        "startedAt": v["startedAt"],
        "endedAt": v["endedAt"],
        "currentStep": v["currentStep"],
        "currentTool": cut(&v["currentTool"], 80),
        "steps": steps,
    })
}

fn read(dir: &Path) -> Result<Value, (StatusCode, &'static str)> {
    let temp = std::env::temp_dir();
    let root_env = std::env::var_os("PI_SUBAGENTS_TEMP_ROOT").filter(|v| !v.is_empty()).map(PathBuf::from);
    let d = run_dir(dir, &temp, root_env.as_deref()).ok_or((StatusCode::FORBIDDEN, "not a subagent run folder"))?;
    let f = d.join("status.json").canonicalize().map_err(|_| (StatusCode::NOT_FOUND, "no status yet"))?;
    if f.parent() != Some(d.as_path()) {
        return Err((StatusCode::FORBIDDEN, "not a subagent run folder"));
    }
    let meta = std::fs::metadata(&f).map_err(|_| (StatusCode::NOT_FOUND, "no status yet"))?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return Err((StatusCode::FORBIDDEN, "not a status file"));
    }
    let text = std::fs::read_to_string(&f).map_err(|_| (StatusCode::NOT_FOUND, "no status yet"))?;
    // the runner may be halfway through writing it: the page simply asks again
    let v: Value = serde_json::from_str(&text).map_err(|_| (StatusCode::CONFLICT, "status is being written"))?;
    Ok(trim(&v))
}

#[derive(serde::Deserialize)]
pub struct StatusQuery {
    dir: String,
}

pub async fn status(Query(q): Query<StatusQuery>) -> Response {
    let dir = PathBuf::from(q.dir);
    match tokio::task::spawn_blocking(move || read(&dir)).await {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err((code, msg))) => fail(code, msg),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_a_run_folder_of_pi_subagents() {
        let base = std::env::temp_dir().join(format!("oven-sa-test-{}", std::process::id()));
        let temp = base.join("tmp");
        let run = temp.join("pi-subagents-u1000").join(RUNS).join("r1");
        let other = temp.join("elsewhere").join(RUNS).join("r1");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        assert!(run_dir(&run, &temp, None).is_some());
        assert!(run_dir(&other, &temp, None).is_none(), "not pi-subagents' folder");
        assert!(run_dir(&run.join(".."), &temp, None).is_none(), "the runs folder itself");
        assert!(run_dir(&temp.join("pi-subagents-u1000"), &temp, None).is_none());
        // with PI_SUBAGENTS_TEMP_ROOT only that root counts
        assert!(run_dir(&other, &temp, Some(&temp.join("elsewhere"))).is_some());
        assert!(run_dir(&run, &temp, Some(&temp.join("elsewhere"))).is_none());
        // a link out of the temp folder does not count as being in it
        let outside = base.join("outside").join(RUNS).join("r2");
        std::fs::create_dir_all(&outside).unwrap();
        let link = temp.join("pi-subagents-u1000").join(RUNS).join("r2");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert!(run_dir(&link, &temp, None).is_none());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn keeps_only_what_the_page_draws() {
        let v = json!({
            "runId": "r1", "state": "running", "mode": "single", "startedAt": 1, "pid": 42, "cwd": "/home/x",
            "steps": [{ "agent": "reviewer", "status": "running", "currentTool": "read", "currentToolArgs": "x".repeat(500), "sessionFile": "/home/x/s.jsonl" }]
        });
        let t = trim(&v);
        assert_eq!(t["state"], "running");
        assert!(t.get("pid").is_none() && t.get("cwd").is_none());
        assert_eq!(t["steps"][0]["agent"], "reviewer");
        assert!(t["steps"][0].get("sessionFile").is_none());
        assert_eq!(t["steps"][0]["currentToolArgs"].as_str().unwrap().chars().count(), 201);
    }
}
