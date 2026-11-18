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

/// One agent's step as the page draws it. `own` is the agent's own run status, when the step is a workflow child
/// that keeps its progress there (the workflow's file only says whether it is running).
fn step(s: &Value, own: Option<&Value>) -> Value {
    let o = own.and_then(|o| o["steps"].get(0)).unwrap_or(s);
    let pick = |k: &str| if s[k].is_null() { o[k].clone() } else { s[k].clone() };
    let last = o["recentTools"].as_array().and_then(|a| a.last()).cloned().unwrap_or(Value::Null);
    json!({
        "agent": cut(&s["agent"], 80),
        "status": s["status"],
        "description": cut(&pick("description"), 300),
        "sessionName": cut(&pick("sessionName"), 300),
        "label": cut(&s["label"], 80),
        "currentTool": cut(&o["currentTool"], 80),
        "currentToolArgs": cut(&o["currentToolArgs"], 200),
        "recentTool": cut(&last["tool"], 80),
        "recentToolArgs": cut(&last["args"], 200),
        "turnCount": o["turnCount"],
        "toolCount": o["toolCount"],
        "context": o["tokens"]["window"],
        "lastActivityAt": o["lastActivityAt"],
        "startedAt": s["startedAt"],
        "endedAt": pick("endedAt"),
        "durationMs": pick("durationMs"),
    })
}

/// A workflow child's own status file, next to the workflow's (its run id names the folder).
fn child(runs: &Path, s: &Value) -> Option<Value> {
    let id = s["runId"].as_str()?;
    if id.is_empty() || id.len() > 100 || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return None;
    }
    read_file(&runs.join(id)).ok()
}

/// What the page draws, and nothing else (the file also holds paths, pids and budgets).
fn trim(v: &Value, runs: Option<&Path>) -> Value {
    let steps: Vec<Value> = v["steps"].as_array().map(|a| a.iter().take(MAX_STEPS).collect::<Vec<_>>()).unwrap_or_default().into_iter().map(|s| {
        let own = runs.filter(|_| s["recentTools"].is_null()).and_then(|r| child(r, s));
        step(s, own.as_ref())
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
    let v = read_file(&d)?;
    Ok(trim(&v, d.parent()))
}

/// `status.json` of a run folder already known to be one (or a sibling of one).
fn read_file(d: &Path) -> Result<Value, (StatusCode, &'static str)> {
    let d = d.canonicalize().map_err(|_| (StatusCode::NOT_FOUND, "no status yet"))?;
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
    serde_json::from_str(&text).map_err(|_| (StatusCode::CONFLICT, "status is being written"))
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

// ── the agents set up (for the info tab) ─────────────────────────────────────

/// One agent definition, as the info tab shows it.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentDef {
    pub name: String,
    pub description: Option<String>,
    /// The model it runs on, when something sets one; `None`: the parent session's.
    pub model: Option<String>,
    /// What set the model: "override", "agent", "default", or "cli" (an external program's own).
    pub model_from: Option<&'static str>,
    pub thinking: Option<String>,
    /// "read-only" or "writer", when the definition says.
    pub role: Option<String>,
    /// "builtin", "user", "project", or "external" (runs another CLI).
    pub kind: &'static str,
    /// "fork" (carries the conversation) or "fresh".
    pub context: Option<String>,
    pub tools: usize,
    /// It can change files (an `edit` or `write` tool, or a writer by its role).
    pub edits: bool,
}

/// The `key: value` lines of a definition's front matter (and `runner:`'s indented ones as `runner.key`).
fn front_matter(text: &str) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return out;
    }
    let mut parent: Option<String> = None;
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        let indented = line.starts_with(' ') || line.starts_with('\t');
        let Some((k, v)) = line.trim().split_once(':') else { continue };
        let (k, v) = (k.trim(), v.trim().trim_matches(['"', '\'']));
        if indented {
            if let Some(p) = &parent {
                out.insert(format!("{p}.{k}"), v.to_owned());
            }
        } else {
            parent = v.is_empty().then(|| k.to_owned());
            if !v.is_empty() {
                out.insert(k.to_owned(), v.to_owned());
            }
        }
    }
    out
}

fn definitions(dir: &Path, kind: &'static str, out: &mut std::collections::BTreeMap<String, (std::collections::BTreeMap<String, String>, &'static str)>) {
    let mut stack = vec![(dir.to_path_buf(), 0)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten().take(500) {
            let p = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() && depth < 4 {
                stack.push((p, depth + 1));
            } else if ft.is_file() && p.extension().is_some_and(|x| x == "md") {
                let Ok(text) = std::fs::read_to_string(&p) else { continue };
                let fm = front_matter(&text);
                if let Some(name) = fm.get("name").cloned() {
                    // a later source (user over builtin, project over user) replaces the same name
                    out.insert(name, (fm, kind));
                }
            }
        }
    }
}

/// `subagents` in a settings file: (default model, per-agent overrides).
fn settings(path: &Path) -> (Option<String>, serde_json::Map<String, Value>) {
    let v: Value = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let s = &v["subagents"];
    (s["defaultModel"].as_str().map(str::to_owned), s["agentOverrides"].as_object().cloned().unwrap_or_default())
}

pub fn agents(cwd: Option<&Path>) -> Vec<AgentDef> {
    let agent_dir = std::env::var_os("PI_CODING_AGENT_DIR").map(PathBuf::from).unwrap_or_else(|| crate::config::home_dir().join(".pi/agent"));
    let mut found = std::collections::BTreeMap::new();
    definitions(&agent_dir.join("npm/node_modules/pi-subagents/agents"), "builtin", &mut found);
    definitions(&agent_dir.join("agents"), "user", &mut found);
    let (mut default_model, mut overrides) = settings(&agent_dir.join("settings.json"));
    if let Some(cwd) = cwd {
        definitions(&cwd.join(".pi/agents"), "project", &mut found);
        // the project's settings win, field by field
        let (pd, po) = settings(&cwd.join(".pi/settings.json"));
        default_model = pd.or(default_model);
        for (k, v) in po {
            let e = overrides.entry(k).or_insert_with(|| json!({}));
            if let (Some(e), Some(v)) = (e.as_object_mut(), v.as_object()) {
                e.extend(v.clone());
            }
        }
    }
    let mut out = Vec::new();
    for (name, (fm, kind)) in found {
        let o = overrides.get(&name).cloned().unwrap_or(Value::Null);
        if o["disabled"] == true || fm.get("disabled").is_some_and(|v| v == "true") {
            continue;
        }
        let get = |k: &str| o[k].as_str().map(str::to_owned).or_else(|| fm.get(k).cloned()).filter(|v| !v.is_empty() && v != "false");
        let external = fm.get("runner.type").is_some_and(|t| t == "external-cli");
        let (model, model_from) = if let Some(m) = o["model"].as_str().filter(|m| !m.is_empty()) {
            (Some(m.to_owned()), Some("override"))
        } else if let Some(m) = fm.get("model").filter(|m| !m.is_empty()) {
            (Some(m.clone()), Some("agent"))
        } else if external {
            (Some(format!("{} CLI", fm.get("runner.command").map(String::as_str).unwrap_or("external"))), Some("cli"))
        } else if let Some(m) = &default_model {
            (Some(m.clone()), Some("default"))
        } else {
            (None, None)
        };
        let tool_list: Vec<String> = get("tools").map(|t| t.split(',').map(|x| x.trim().to_owned()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
        let tools = tool_list.len();
        let role = get("acceptanceRole");
        let edits = role.as_deref() == Some("writer") || (role.as_deref() != Some("read-only") && tool_list.iter().any(|t| t == "edit"));
        out.push(AgentDef {
            name: name.chars().take(80).collect(),
            description: get("description").map(|d| d.chars().take(200).collect()),
            model,
            model_from,
            thinking: get("thinking"),
            role,
            kind: if external { "external" } else { kind },
            context: get("defaultContext"),
            tools,
            edits,
        });
    }
    out
}

#[derive(serde::Deserialize)]
pub struct AgentsQuery {
    runtime: Option<String>,
}

pub async fn agents_route(axum::extract::State(app): axum::extract::State<std::sync::Arc<crate::AppState>>, Query(q): Query<AgentsQuery>) -> Response {
    let cwd = match &q.runtime {
        Some(id) => app.runtimes.get(id).await.map(|rt| PathBuf::from(&rt.info.cwd)),
        None => None,
    };
    match tokio::task::spawn_blocking(move || agents(cwd.as_deref())).await {
        Ok(list) => Json(json!({ "agents": list })).into_response(),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_front_matter_and_the_runner() {
        let fm = front_matter("---\nname: claude-code\ndescription: \"Read-only\"\nrunner:\n  type: external-cli\n  command: claude\nthinking: high\n---\nbody: no\n");
        assert_eq!(fm.get("name").map(String::as_str), Some("claude-code"));
        assert_eq!(fm.get("description").map(String::as_str), Some("Read-only"));
        assert_eq!(fm.get("runner.type").map(String::as_str), Some("external-cli"));
        assert_eq!(fm.get("runner.command").map(String::as_str), Some("claude"));
        assert_eq!(fm.get("thinking").map(String::as_str), Some("high"));
        assert!(!fm.contains_key("body"));
    }

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
    fn a_workflow_child_reports_from_its_own_file() {
        let base = std::env::temp_dir().join(format!("oven-sa-child-{}", std::process::id()));
        let runs = base.join(RUNS);
        std::fs::create_dir_all(runs.join("c1")).unwrap();
        std::fs::write(runs.join("c1/status.json"), r#"{"steps":[{"agent":"reviewer","sessionName":"reviewer: 읽기 전용 검토","turnCount":19,"toolCount":30,"tokens":{"window":130959},"recentTools":[{"tool":"grep","args":"src/agent"}]}]}"#).unwrap();
        let wf = json!({ "state": "running", "mode": "workflow", "steps": [
            { "agent": "reviewer", "label": "task-1", "status": "running", "runId": "c1" },
            { "agent": "reviewer", "label": "task-2", "status": "running", "runId": "../../etc" },
        ]});
        let t = trim(&wf, Some(&runs));
        assert_eq!(t["steps"][0]["turnCount"], 19);
        assert_eq!(t["steps"][0]["recentTool"], "grep");
        assert_eq!(t["steps"][0]["context"], 130959);
        assert_eq!(t["steps"][0]["sessionName"], "reviewer: 읽기 전용 검토");
        assert!(t["steps"][1]["turnCount"].is_null(), "a run id that is not a plain name is not followed");
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn keeps_only_what_the_page_draws() {
        let v = json!({
            "runId": "r1", "state": "running", "mode": "single", "startedAt": 1, "pid": 42, "cwd": "/home/x",
            "steps": [{ "agent": "reviewer", "status": "running", "currentTool": "read", "currentToolArgs": "x".repeat(500), "sessionFile": "/home/x/s.jsonl" }]
        });
        let t = trim(&v, None);
        assert_eq!(t["state"], "running");
        assert!(t.get("pid").is_none() && t.get("cwd").is_none());
        assert_eq!(t["steps"][0]["agent"], "reviewer");
        assert!(t["steps"][0].get("sessionFile").is_none());
        assert_eq!(t["steps"][0]["currentToolArgs"].as_str().unwrap().chars().count(), 201);
    }
}
