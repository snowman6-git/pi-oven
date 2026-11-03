//! Settings → About: which pi, which packages, which MCP servers. Read from pi's own files; nothing secret goes out
//! (an MCP server is its name, how it connects and how many tools it last had, never its command, URL, env or headers).

use axum::{extract::State, Json};
use serde::Serialize;
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf, process::Stdio, sync::Arc, time::Duration};
use tokio::process::Command;

use crate::{config::home_dir, AppState};

fn agent_dir() -> PathBuf {
    std::env::var_os("PI_CODING_AGENT_DIR").map(PathBuf::from).unwrap_or_else(|| home_dir().join(".pi/agent"))
}

fn read_json(path: PathBuf) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Package {
    /// what settings.json says, e.g. `npm:pi-lens` or `git:github.com/a/b`
    source: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Mcp {
    name: String,
    /// "stdio", "http", "sse"… as configured
    #[serde(skip_serializing_if = "Option::is_none")]
    transport: Option<String>,
    /// from pi-mcp-adapter's cache: what the server offered the last time it was asked
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompts: Option<usize>,
    /// in the global mcp.json (else it was only seen in the cache, from a project's own config)
    configured: bool,
    disabled: bool,
}

/// `npm:@scope/name@latest` → `@scope/name`
fn npm_name(spec: &str) -> &str {
    let at = if let Some(rest) = spec.strip_prefix('@') { rest.find('@').map(|i| i + 1) } else { spec.find('@') };
    at.map_or(spec, |i| &spec[..i])
}

fn packages(dir: &std::path::Path, settings: &Value) -> Vec<Package> {
    let list = settings["packages"].as_array().cloned().unwrap_or_default();
    list.iter()
        .filter_map(|p| p.as_str().or_else(|| p["source"].as_str()).map(str::to_owned))
        .map(|source| {
            if let Some(spec) = source.strip_prefix("npm:") {
                let name = npm_name(spec).to_owned();
                let meta = read_json(dir.join("npm/node_modules").join(&name).join("package.json"));
                let field = |k: &str| meta.as_ref().and_then(|m| m[k].as_str()).map(str::to_owned);
                Package { version: field("version"), description: field("description"), name, source }
            } else {
                // git or a local folder: the last part of it is the name; the full source may be a private path
                let name = source.trim_end_matches('/').rsplit(['/', ':']).next().unwrap_or(&source).to_owned();
                let kind = source.split(':').next().filter(|k| *k == "git" || *k == "https").unwrap_or("local");
                Package { source: kind.to_owned(), name, version: None, description: None }
            }
        })
        .collect()
}

fn mcp(dir: &std::path::Path) -> Vec<Mcp> {
    let mut out: BTreeMap<String, Mcp> = BTreeMap::new();
    if let Some(cfg) = read_json(dir.join("mcp.json")) {
        for (name, s) in cfg["mcpServers"].as_object().into_iter().flatten() {
            let transport = s["type"].as_str().map(str::to_owned).or_else(|| {
                if s.get("command").is_some() {
                    Some("stdio".into())
                } else if s.get("url").is_some() {
                    Some("http".into())
                } else {
                    None
                }
            });
            let disabled = s["disabled"].as_bool().unwrap_or(false) || s["enabled"].as_bool() == Some(false);
            out.insert(name.clone(), Mcp { name: name.clone(), transport, tools: None, prompts: None, configured: true, disabled });
        }
    }
    if let Some(cache) = read_json(dir.join("mcp-cache.json")) {
        for (name, s) in cache["servers"].as_object().into_iter().flatten() {
            let e = out.entry(name.clone()).or_insert_with(|| Mcp {
                name: name.clone(),
                transport: None,
                tools: None,
                prompts: None,
                configured: false,
                disabled: false,
            });
            e.tools = s["tools"].as_array().map(Vec::len);
            e.prompts = s["prompts"].as_array().map(Vec::len).filter(|n| *n > 0);
        }
    }
    out.into_values().collect()
}

async fn pi_version(bin: &str) -> Option<String> {
    let mut cmd = Command::new(bin);
    cmd.arg("--version").current_dir(home_dir()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    let out = tokio::time::timeout(Duration::from_secs(15), cmd.output()).await.ok()?.ok()?;
    let v = String::from_utf8_lossy(&out.stdout).trim().lines().last()?.trim().to_owned();
    (out.status.success() && !v.is_empty() && v.len() < 64).then_some(v)
}

pub async fn about(State(app): State<Arc<AppState>>) -> Json<Value> {
    let dir = agent_dir();
    let (pi, (pkgs, servers)) = tokio::join!(pi_version(&app.pi_bin), async {
        let dir = dir.clone();
        tokio::task::spawn_blocking(move || {
            let settings = read_json(dir.join("settings.json")).unwrap_or(Value::Null);
            (packages(&dir, &settings), mcp(&dir))
        })
        .await
        .unwrap_or_default()
    });
    Json(serde_json::json!({
        "pi": pi,
        "oven": env!("CARGO_PKG_VERSION"),
        "packages": pkgs,
        "mcp": servers,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn npm_names() {
        assert_eq!(npm_name("pi-lens"), "pi-lens");
        assert_eq!(npm_name("pi-background-tasks@latest"), "pi-background-tasks");
        assert_eq!(npm_name("@juicesharp/rpiv-todo"), "@juicesharp/rpiv-todo");
        assert_eq!(npm_name("@xzzpig/pi-goal-x@1.2.0"), "@xzzpig/pi-goal-x");
    }

    #[test]
    fn mcp_shows_names_not_commands() {
        let dir = std::env::temp_dir().join(format!("pi-oven-about-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mcp.json"), r#"{"mcpServers":{"a":{"command":"secret-cmd","args":["--key","s3cr3t"]},"b":{"url":"https://x/y","headers":{"Authorization":"t"},"disabled":true}}}"#).unwrap();
        std::fs::write(dir.join("mcp-cache.json"), r#"{"servers":{"a":{"tools":[{},{}],"prompts":[]},"c":{"tools":[{}]}}}"#).unwrap();
        let list = mcp(&dir);
        let text = serde_json::to_string(&list).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(!text.contains("secret") && !text.contains("s3cr3t") && !text.contains("https://x") && !text.contains("Authorization"));
        assert_eq!(list.len(), 3);
        assert_eq!((list[0].transport.as_deref(), list[0].tools), (Some("stdio"), Some(2)));
        assert!(list[1].disabled && list[1].transport.as_deref() == Some("http"));
        assert!(!list[2].configured);
    }
}
