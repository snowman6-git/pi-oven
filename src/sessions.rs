//! Lists pi's on-disk sessions (`<dir>/<encoded-cwd>/*.jsonl`) without starting pi.

use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Seek, SeekFrom},
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
    /// (mtime, size, bytes read up to the last whole line, summary so far): a file pi appends to is read on from there
    cache: Mutex<HashMap<PathBuf, (u64, u64, u64, SessionSummary)>>,
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

/// The summary of a session file; `prev` (bytes read, summary then) goes on from there instead of from the start.
fn parse(path: &Path, modified: u64, prev: Option<(u64, SessionSummary)>) -> Option<(u64, SessionSummary)> {
    let file = std::fs::File::open(path).ok()?;
    let mut rd = BufReader::new(file);
    let mut buf = Vec::new();

    let (mut at, id, cwd, mut name, mut preview, mut turns) = match prev {
        Some((at, s)) => {
            rd.seek(SeekFrom::Start(at)).ok()?;
            (at, s.id, s.cwd, s.name, s.preview, s.turns)
        }
        None => {
            let n = rd.read_until(b'\n', &mut buf).ok()?;
            let header: Value = serde_json::from_slice(&buf).ok()?;
            if header.get("type")?.as_str()? != "session" {
                return None;
            }
            let id = header.get("id")?.as_str()?.to_owned();
            let cwd = header.get("cwd").and_then(Value::as_str).unwrap_or("").to_owned();
            (n as u64, id, cwd, None, None, 0u32)
        }
    };
    loop {
        buf.clear();
        let n = rd.read_until(b'\n', &mut buf).ok()?;
        if n == 0 {
            break;
        }
        // a line pi is still writing is read next time, whole (pi ends every line with a newline)
        if buf.last() != Some(&b'\n') {
            break;
        }
        at += n as u64;
        // Cheap substring checks first: message lines can be megabytes.
        let line = String::from_utf8_lossy(&buf);
        if line.starts_with(r#"{"type":"session_info""#) || line.contains(r#""type":"session_info""#) {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if v.get("type").and_then(Value::as_str) == Some("session_info") {
                    name = v.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned);
                }
            }
        }
        if line.contains(r#""role":"user""#) {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if v.get("type").and_then(Value::as_str) == Some("message")
                    && v.get("message").and_then(|m| m.get("role")).and_then(Value::as_str) == Some("user")
                {
                    turns += 1;
                    if preview.is_none() {
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
    }
    Some((at, SessionSummary { path: path.to_string_lossy().into_owned(), id, cwd, name, preview, modified, turns }))
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

                let hit = self.cache.lock().unwrap().get(&p).cloned();
                if let Some((_, _, _, summary)) = hit.as_ref().filter(|(m, s, _, _)| *m == mtime && *s == size) {
                    out.push(summary.clone());
                    continue;
                }
                // grown: read on from the last whole line (pi only appends); shrunk: from the start
                let prev = hit.filter(|(_, s, at, _)| size >= *s && size >= *at).map(|(_, _, at, s)| (at, s));
                if let Some((at, summary)) = parse(&p, mtime, prev) {
                    self.cache.lock().unwrap().insert(p, (mtime, size, at, summary.clone()));
                    out.push(summary);
                }
            }
        }
        self.cache.lock().unwrap().retain(|k, _| seen.contains(k));
        out.sort_by_key(|s| std::cmp::Reverse(s.modified));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reads_on_from_where_it_stopped() {
        let dir = std::env::temp_dir().join(format!("oven-sess-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("s.jsonl");
        let user = r#"{"type":"message","message":{"role":"user","content":"hi"}}"#;
        std::fs::write(&p, format!("{{\"type\":\"session\",\"id\":\"x\",\"cwd\":\"/\"}}\n{user}\n{{\"type\":\"mess")).unwrap();
        let (at, s) = parse(&p, 1, None).unwrap();
        assert_eq!((s.turns, s.preview.as_deref()), (1, Some("hi")));
        // the half-written line is finished and another comes
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        write!(f, "age\",\"message\":{{\"role\":\"user\",\"content\":\"again\"}}}}\n{user}\n").unwrap();
        let (_, s) = parse(&p, 2, Some((at, s))).unwrap();
        assert_eq!((s.turns, s.preview.as_deref(), s.modified), (3, Some("hi"), 2));
        // a message body mentioning `"type":"session_info"` must not wipe the session name
        writeln!(f, r#"{{"type":"session_info","name":"keep-me"}}"#).unwrap();
        writeln!(f, r#"{{"type":"message","message":{{"role":"assistant","content":"discussing \"type\":\"session_info\""}}}}"#).unwrap();
        let (_, s) = parse(&p, 3, None).unwrap();
        assert_eq!(s.name.as_deref(), Some("keep-me"));
        assert_eq!(s.turns, 3);
        std::fs::remove_file(&p).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }
}
