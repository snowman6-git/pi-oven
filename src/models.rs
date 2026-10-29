//! Adding models to pi: the providers in pi's own `models.json` (an endpoint address, an API style, a key, and the
//! models behind it). The file is pi's; this only edits what it is asked to and leaves everything else in it as it was.
//! The key is written the way pi reads it (a literal, `$ENV_NAME`, or `!command`) and is never sent back: the page
//! gets a hint of it, enough to recognise which key is there.

use std::{io::Write, path::PathBuf, process::Stdio, sync::Arc, time::Duration};

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use tokio::{io::AsyncWriteExt, process::Command};

use crate::{api::fail, config::home_dir, AppState};

/// What pi can talk to. (pi has more; these are the ones worth typing an address for.)
const APIS: &[&str] = &["openai-completions", "openai-responses", "anthropic-messages", "google-generative-ai"];

fn models_path() -> PathBuf {
    std::env::var_os("PI_CODING_AGENT_DIR").map(PathBuf::from).unwrap_or_else(|| home_dir().join(".pi/agent")).join("models.json")
}

type Bad = (StatusCode, String);
fn bad(m: impl Into<String>) -> Bad {
    (StatusCode::BAD_REQUEST, m.into())
}

fn read() -> Result<Value, Bad> {
    match std::fs::read_to_string(models_path()) {
        Ok(t) if t.trim().is_empty() => Ok(json!({})),
        Ok(t) => serde_json::from_str(&t).map_err(|e| (StatusCode::UNPROCESSABLE_ENTITY, format!("models.json을 읽을 수 없습니다: {e}"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

/// One backup of the previous file, then the new one by rename so pi never reads half of it.
fn write(doc: &Value) -> Result<(), Bad> {
    let path = models_path();
    let io = |e: std::io::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io)?;
    }
    if path.exists() {
        let _ = std::fs::copy(&path, path.with_extension("json.bak"));
    }
    let tmp = path.with_extension(format!("json.tmp-{}", std::process::id()));
    let mut f = std::fs::File::create(&tmp).map_err(io)?;
    f.write_all(serde_json::to_string_pretty(doc).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?.as_bytes()).map_err(io)?;
    f.write_all(b"\n").map_err(io)?;
    f.sync_all().map_err(io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // it may hold a key: not for other users of this machine
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io(e)
    })
}

fn key_hint(key: &Value) -> Value {
    let Some(k) = key.as_str().filter(|k| !k.is_empty()) else { return json!({ "kind": "none" }) };
    if let Some(var) = k.strip_prefix('$') {
        return json!({ "kind": "env", "hint": format!("${var}") }); // a variable's name is not a secret
    }
    if k.starts_with('!') {
        return json!({ "kind": "command", "hint": "명령의 출력" });
    }
    let chars: Vec<char> = k.chars().collect();
    let hint = if chars.len() <= 8 { "•".repeat(chars.len()) } else { format!("{}••••{}", chars[..3].iter().collect::<String>(), chars[chars.len() - 2..].iter().collect::<String>()) };
    json!({ "kind": "literal", "hint": hint })
}

pub async fn list(State(_): State<Arc<AppState>>) -> Response {
    let doc = match read() {
        Ok(d) => d,
        Err((c, m)) => return fail(c, m),
    };
    let providers: Vec<Value> = doc["providers"]
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(name, p)| {
                    let models: Vec<Value> = p["models"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .map(|m| json!({
                                    "id": m["id"], "name": m["name"], "reasoning": m["reasoning"].as_bool().unwrap_or(false),
                                    "image": m["input"].as_array().is_some_and(|i| i.iter().any(|x| x == "image")),
                                    "contextWindow": m["contextWindow"], "maxTokens": m["maxTokens"],
                                }))
                                .collect()
                        })
                        .unwrap_or_default();
                    json!({ "name": name, "baseUrl": p["baseUrl"], "api": p["api"], "key": key_hint(&p["apiKey"]), "models": models })
                })
                .collect()
        })
        .unwrap_or_default();
    Json(json!({ "path": models_path().to_string_lossy(), "providers": providers, "apis": APIS })).into_response()
}

#[derive(Deserialize)]
pub struct ModelSpec {
    id: String,
    name: Option<String>,
    #[serde(default)]
    reasoning: bool,
    #[serde(default)]
    image: bool,
    #[serde(rename = "contextWindow")]
    context_window: Option<u64>,
    #[serde(rename = "maxTokens")]
    max_tokens: Option<u64>,
}

#[derive(Deserialize)]
pub struct Upsert {
    provider: String,
    #[serde(rename = "baseUrl")]
    base_url: String,
    api: String,
    /// None keeps the key that is there; an empty string takes it away.
    #[serde(rename = "apiKey")]
    api_key: Option<String>,
    model: Option<ModelSpec>,
}

fn plain(s: &str, what: &str, max: usize) -> Result<(), Bad> {
    if s.trim().is_empty() || s.chars().count() > max || s.chars().any(char::is_control) {
        return Err(bad(format!("{what}이(가) 비었거나 올바르지 않습니다.")));
    }
    Ok(())
}

fn check_url(u: &str) -> Result<(), Bad> {
    let rest = u.strip_prefix("http://").or_else(|| u.strip_prefix("https://")).ok_or_else(|| bad("주소는 http:// 또는 https:// 로 시작해야 합니다."))?;
    if rest.is_empty() || rest.starts_with('/') || u.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(bad("주소가 올바르지 않습니다."));
    }
    Ok(())
}

pub async fn upsert(State(_): State<Arc<AppState>>, Json(req): Json<Upsert>) -> Response {
    let res = (|| -> Result<(), Bad> {
        plain(&req.provider, "제공자 이름", 64)?;
        if !req.provider.chars().all(|c| c.is_alphanumeric() || " ._-".contains(c)) {
            return Err(bad("제공자 이름에는 글자, 숫자, 공백, . _ - 만 쓸 수 있습니다."));
        }
        check_url(req.base_url.trim())?;
        if !APIS.contains(&req.api.as_str()) {
            return Err(bad("지원하지 않는 API 방식입니다."));
        }
        if let Some(k) = &req.api_key {
            if k.chars().any(char::is_control) || k.len() > 2000 {
                return Err(bad("API 키가 올바르지 않습니다."));
            }
        }
        if let Some(m) = &req.model {
            plain(&m.id, "모델 ID", 200)?;
            if let Some(n) = &m.name {
                plain(n, "표시 이름", 200)?;
            }
            for (v, what) in [(m.context_window, "컨텍스트 창"), (m.max_tokens, "최대 출력 토큰")] {
                if v.is_some_and(|v| v == 0 || v > 100_000_000) {
                    return Err(bad(format!("{what}이(가) 올바르지 않습니다.")));
                }
            }
        }
        let mut doc = read()?;
        let root = doc.as_object_mut().ok_or_else(|| bad("models.json의 최상위가 객체가 아닙니다."))?;
        let providers = root.entry("providers").or_insert_with(|| json!({}));
        let providers = providers.as_object_mut().ok_or_else(|| bad("models.json의 providers가 객체가 아닙니다."))?;
        let p = providers.entry(req.provider.clone()).or_insert_with(|| json!({}));
        let p = p.as_object_mut().ok_or_else(|| bad("이 제공자 항목이 객체가 아닙니다."))?;
        p.insert("baseUrl".into(), json!(req.base_url.trim()));
        p.insert("api".into(), json!(req.api));
        match req.api_key.as_deref() {
            None => {}
            Some("") => {
                p.remove("apiKey");
            }
            Some(k) => {
                p.insert("apiKey".into(), json!(k));
            }
        }
        if let Some(m) = &req.model {
            let mut entry = Map::new();
            entry.insert("id".into(), json!(m.id.trim()));
            if let Some(n) = &m.name {
                entry.insert("name".into(), json!(n.trim()));
            }
            entry.insert("reasoning".into(), json!(m.reasoning));
            entry.insert("input".into(), if m.image { json!(["text", "image"]) } else { json!(["text"]) });
            if let Some(v) = m.context_window {
                entry.insert("contextWindow".into(), json!(v));
            }
            if let Some(v) = m.max_tokens {
                entry.insert("maxTokens".into(), json!(v));
            }
            let models = p.entry("models").or_insert_with(|| json!([]));
            let models = models.as_array_mut().ok_or_else(|| bad("이 제공자의 models가 목록이 아닙니다."))?;
            // the same id replaces: fields set by hand that this form does not know (compat, cost, …) stay
            match models.iter_mut().find(|x| x["id"] == m.id.trim()) {
                Some(old) => {
                    if let Some(o) = old.as_object_mut() {
                        for (k, v) in entry {
                            o.insert(k, v);
                        }
                    }
                }
                None => models.push(Value::Object(entry)),
            }
        }
        write(&doc)
    })();
    match res {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err((c, m)) => fail(c, m),
    }
}

#[derive(Deserialize)]
pub struct Remove {
    provider: String,
    model: Option<String>,
}

/// A model, or (with no model named) a whole provider.
pub async fn remove(State(_): State<Arc<AppState>>, Query(q): Query<Remove>) -> Response {
    let res = (|| -> Result<(), Bad> {
        let mut doc = read()?;
        let providers = doc["providers"].as_object_mut().ok_or((StatusCode::NOT_FOUND, "제공자가 없습니다.".to_string()))?;
        match &q.model {
            None => {
                providers.remove(&q.provider).ok_or((StatusCode::NOT_FOUND, "제공자가 없습니다.".to_string()))?;
            }
            Some(id) => {
                let models = providers.get_mut(&q.provider).and_then(|p| p["models"].as_array_mut()).ok_or((StatusCode::NOT_FOUND, "모델이 없습니다.".to_string()))?;
                let before = models.len();
                models.retain(|m| m["id"] != id.as_str());
                if models.len() == before {
                    return Err((StatusCode::NOT_FOUND, "모델이 없습니다.".into()));
                }
            }
        }
        write(&doc)
    })();
    match res {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err((c, m)) => fail(c, m),
    }
}

#[derive(Deserialize)]
pub struct Probe {
    #[serde(rename = "baseUrl")]
    base_url: String,
    #[serde(rename = "apiKey")]
    api_key: Option<String>,
    /// Which provider's saved key to use when none is typed here.
    provider: Option<String>,
}

/// Asks the endpoint what it serves (`GET {baseUrl}/models`, the OpenAI-style listing most servers offer). It goes
/// through `curl` with the key on its stdin, so the key never shows up in a process list.
pub async fn probe(State(_): State<Arc<AppState>>, Json(req): Json<Probe>) -> Response {
    let url = req.base_url.trim().trim_end_matches('/').to_owned();
    if let Err((c, m)) = check_url(&url) {
        return fail(c, m);
    }
    let mut key = req.api_key.filter(|k| !k.is_empty());
    if key.is_none() {
        if let (Some(name), Ok(doc)) = (&req.provider, read()) {
            key = doc["providers"][name.as_str()]["apiKey"].as_str().filter(|k| !k.starts_with('!')).map(|k| k.strip_prefix('$').map_or_else(|| k.to_owned(), |v| std::env::var(v).unwrap_or_default())).filter(|k| !k.is_empty());
        }
    }
    let q = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    if url.chars().any(|c| c == '"' || c == '\\') || key.as_deref().is_some_and(|k| k.chars().any(|c| c.is_control())) {
        return fail(StatusCode::BAD_REQUEST, "주소나 키에 쓸 수 없는 문자가 있습니다.");
    }
    let mut cfg = format!("url = \"{}/models\"\n", q(&url));
    if let Some(k) = &key {
        cfg.push_str(&format!("header = \"Authorization: Bearer {}\"\n", q(k)));
    }
    let run = async {
        let mut child = Command::new("curl")
            .args(["-sS", "-m", "8", "--max-filesize", "2000000", "-K", "-", "-w", "\n%{http_code}"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| "curl을 실행할 수 없습니다. curl을 설치하거나 모델 ID를 직접 입력하세요.".to_string())?;
        child.stdin.take().ok_or("curl에 연결하지 못했습니다.")?.write_all(cfg.as_bytes()).await.map_err(|e| e.to_string())?;
        let out = tokio::time::timeout(Duration::from_secs(12), child.wait_with_output()).await.map_err(|_| "응답이 없습니다.".to_string())?.map_err(|e| e.to_string())?;
        if !out.status.success() {
            let why = String::from_utf8_lossy(&out.stderr);
            return Err(format!("연결하지 못했습니다: {}", why.trim().trim_start_matches("curl: ")));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let body = match run.await {
        Ok(b) => b,
        Err(e) => return fail(StatusCode::BAD_GATEWAY, e),
    };
    let (text, code) = body.rsplit_once('\n').unwrap_or((&body, ""));
    if code.trim() != "200" {
        let hint = if code.trim() == "401" || code.trim() == "403" { " (키를 확인하세요)" } else { "" };
        return fail(StatusCode::BAD_GATEWAY, format!("서버가 {}로 답했습니다{hint}.", code.trim()));
    }
    let Ok(v) = serde_json::from_str::<Value>(text) else { return fail(StatusCode::BAD_GATEWAY, "모델 목록 형식이 아닙니다(JSON이 아님). 모델 ID를 직접 입력하세요.") };
    // OpenAI style {"data":[{"id":..}]}, or a bare list, or {"models":[..]}
    let list = v["data"].as_array().or_else(|| v["models"].as_array()).or_else(|| v.as_array());
    let ids: Vec<String> = list.map(|a| a.iter().filter_map(|m| m["id"].as_str().or_else(|| m["name"].as_str()).or_else(|| m.as_str()).map(str::to_owned)).take(500).collect()).unwrap_or_default();
    Json(json!({ "ids": ids })).into_response()
}
