//! Lists pi's on-disk sessions (`<dir>/<encoded-cwd>/*.jsonl`) without starting pi.

use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub path: String,
    pub id: String,
    pub cwd: String,
    pub name: Option<String>,
    pub preview: Option<String>,
    pub modified: u64,
    pub turns: u32,
}

#[derive(Default)]
pub struct SessionIndex {
    cache: Mutex<HashMap<PathBuf, (u64, u64, SessionSummary)>>,
}

fn millis(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn first_text(content: &Value) -> Option<String> {
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .find_map(|p| (p.get("type")?.as_str()? == "text").then(|| p.get("text")?.as_str().map(str::to_owned))?)?,
        _ => return None,
    };
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(flat.chars().take(160).collect())
}

fn parse(path: &Path, modified: u64) -> Option<SessionSummary> {
    let file = std::fs::File::open(path).ok()?;
    let mut rd = BufReader::new(file);
    let mut buf = Vec::new();

    rd.read_until(b'\n', &mut buf).ok()?;
    let header: Value = serde_json::from_slice(&buf).ok()?;
    if header.get("type")?.as_str()? != "session" {
        return None;
    }
    let id = header.get("id")?.as_str()?.to_owned();
    let cwd = header.get("cwd").and_then(Value::as_str).unwrap_or("").to_owned();

    let (mut name, mut preview, mut turns) = (None, None, 0u32);
    loop {
        buf.clear();
        if rd.read_until(b'\n', &mut buf).ok()? == 0 {
            break;
        }
        // Cheap substring checks first: message lines can be megabytes.
        let line = String::from_utf8_lossy(&buf);
        if line.starts_with(r#"{"type":"session_info""#) || line.contains(r#""type":"session_info""#) {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                name = v.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned);
            }
        } else if line.contains(r#""message":{"role":"user""#) {
            turns += 1;
            if preview.is_none() {
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    // Skill/context injections are not what the person typed.
                    preview = v
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(first_text)
                        .filter(|t| !t.starts_with("<skill"));
                }
            }
        }
    }
    Some(SessionSummary { path: path.to_string_lossy().into_owned(), id, cwd, name, preview, modified, turns })
}

impl SessionIndex {
    pub fn list(&self, root: &Path) -> Vec<SessionSummary> {
        let mut out = Vec::new();
        let Ok(dirs) = std::fs::read_dir(root) else { return out };
        let mut seen = std::collections::HashSet::new();
        for dir in dirs.flatten().filter(|d| d.path().is_dir()) {
            let Ok(files) = std::fs::read_dir(dir.path()) else { continue };
            for f in files.flatten() {
                let p = f.path();
                if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                let Ok(meta) = f.metadata() else { continue };
                let (mtime, size) = (millis(meta.modified().unwrap_or(UNIX_EPOCH)), meta.len());
                seen.insert(p.clone());

                let hit = self.cache.lock().unwrap().get(&p).filter(|(m, s, _)| *m == mtime && *s == size).cloned();
                if let Some((_, _, summary)) = hit {
                    out.push(summary);
                } else if let Some(summary) = parse(&p, mtime) {
                    self.cache.lock().unwrap().insert(p, (mtime, size, summary.clone()));
                    out.push(summary);
                }
            }
        }
        self.cache.lock().unwrap().retain(|k, _| seen.contains(k));
        out.sort_by_key(|s| std::cmp::Reverse(s.modified));
        out
    }
}
