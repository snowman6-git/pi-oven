//! `compaction.keepRecentTokens` in pi's own `settings.json` (Settings → Auto → Compaction): how much of the newest
//! conversation a compaction keeps as it was.
//!
//! pi reads only this file for it, so it is the one place pi-oven writes there, and only that key: the edit is made
//! in the file's text, so every other key, its order and its indentation stay as they were (serde_json without
//! `preserve_order` would sort the keys). A file that does not parse is left alone. pi reads the value when a session
//! starts (and on its `/reload`), so a running session keeps the number it started with.

use axum::{extract::State, http::StatusCode, response::{IntoResponse, Response}, Json};
use serde::Deserialize;
use std::{path::PathBuf, sync::Arc};

use crate::{api::fail, AppState};

/// pi's own default.
pub const DEFAULT_KEEP: u64 = 20_000;
const MAX_KEEP: u64 = 100_000_000;

fn path() -> PathBuf {
    crate::config::agent_dir().join("settings.json")
}

/// The member `key` of the object that starts at `open` (a `{`): where its value starts and ends, and where its key starts.
/// The text is known to be valid JSON. Keeps the last occurrence on duplicate keys (matching serde_json::Value).
fn member(text: &str, open: usize, key: &str) -> Option<(usize, usize, usize)> {
    let b = text.as_bytes();
    if b.get(open) != Some(&b'{') {
        return None;
    }
    let mut i = open + 1;
    let mut found = None;
    loop {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b',') {
            i += 1;
        }
        if i >= b.len() || b[i] == b'}' {
            return found;
        }
        let key_at = i;
        let key_end = string_end(b, i)?;
        let name: String = serde_json::from_str(&text[key_at..key_end]).ok()?;
        i = key_end;
        while i < b.len() && b[i] != b':' {
            i += 1;
        }
        if i >= b.len() {
            return None;
        }
        i += 1;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let val_at = i;
        i = value_end(b, i)?;
        if name == key {
            found = Some((key_at, val_at, i));
        }
    }
}

/// One past the closing quote of the string starting at `i`.
fn string_end(b: &[u8], mut i: usize) -> Option<usize> {
    if b.get(i) != Some(&b'"') {
        return None;
    }
    i += 1;
    while i < b.len() && b[i] != b'"' {
        i += if b[i] == b'\\' { 2 } else { 1 };
    }
    (i < b.len()).then_some(i + 1)
}

/// One past the value starting at `i`.
fn value_end(b: &[u8], mut i: usize) -> Option<usize> {
    match *b.get(i)? {
        b'"' => string_end(b, i),
        b'{' | b'[' => {
            let mut depth = 0usize;
            while i < b.len() {
                match b[i] {
                    b'"' => {
                        i = string_end(b, i)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => depth = depth.checked_sub(1)?,
                    _ => {}
                }
                i += 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            None
        }
        _ => {
            while i < b.len() && !matches!(b[i], b',' | b'}' | b']') && !b[i].is_ascii_whitespace() {
                i += 1;
            }
            Some(i)
        }
    }
}

/// The whitespace in front of the member that starts at `at`, when it begins its line (else it is on one line with the rest).
fn line_indent(text: &str, at: usize) -> Option<&str> {
    let start = text[..at].rfind('\n')? + 1;
    let lead = &text[start..at];
    lead.chars().all(|c| c == ' ' || c == '\t').then_some(lead)
}

/// `text` with `compaction.keepRecentTokens` set to `n`, the rest as it was.
pub fn with_keep(text: &str, n: u64) -> Result<String, String> {
    let blank = text.trim().is_empty();
    let doc: serde_json::Value = if blank { serde_json::json!({}) } else { serde_json::from_str(text).map_err(|e| e.to_string())? };
    let Some(top) = doc.as_object() else { return Err("settings.json is not an object".into()) };
    if blank {
        return Ok(format!("{{\n  \"compaction\": {{\n    \"keepRecentTokens\": {n}\n  }}\n}}\n"));
    }
    let open = text.find('{').ok_or("settings.json is not an object")?;
    let out = match top.get("compaction") {
        Some(c) if !c.is_object() => return Err("\"compaction\" in settings.json is not an object".into()),
        Some(c) => {
            let (key_at, val_at, val_end) = member(text, open, "compaction").ok_or("could not find \"compaction\"")?;
            let base = line_indent(text, key_at).unwrap_or("");
            let step = if c.as_object().is_some_and(|o| o.is_empty()) { "  " } else { "" };
            match member(text, val_at, "keepRecentTokens") {
                Some((_, v_at, v_end)) => format!("{}{n}{}", &text[..v_at], &text[v_end..]),
                None if step.is_empty() => {
                    // first in the object, laid out like the member that is first now
                    let inner = val_at + 1;
                    let first = text[inner..val_end].find('"').map(|o| inner + o).unwrap_or(inner);
                    match line_indent(text, first) {
                        Some(ind) => format!("{}\n{ind}\"keepRecentTokens\": {n},{}", &text[..inner], &text[inner..]),
                        None => format!("{} \"keepRecentTokens\": {n},{}", &text[..inner], &text[inner..]),
                    }
                }
                None => format!("{}{{\n{base}  \"keepRecentTokens\": {n}\n{base}}}{}", &text[..val_at], &text[val_end..]),
            }
        }
        None => {
            let after = open + 1;
            let first = text[after..].find('"').map(|o| after + o);
            let (has_members, ind) = match first {
                Some(f) if !text[after..f].contains('}') => (true, line_indent(text, f)),
                _ => (false, None),
            };
            match (has_members, ind) {
                (true, Some(ind)) => format!("{}\n{ind}\"compaction\": {{\n{ind}  \"keepRecentTokens\": {n}\n{ind}}},{}", &text[..after], &text[after..]),
                (true, None) => format!("{} \"compaction\": {{ \"keepRecentTokens\": {n} }},{}", &text[..after], &text[after..]),
                (false, _) => format!("{{\n  \"compaction\": {{\n    \"keepRecentTokens\": {n}\n  }}\n}}\n"),
            }
        }
    };
    // what was written must read back as the same document with only this key changed
    let back: serde_json::Value = serde_json::from_str(&out).map_err(|e| format!("the edit would break settings.json: {e}"))?;
    let mut want = doc;
    want["compaction"]["keepRecentTokens"] = n.into();
    if back != want {
        return Err("the edit would change more than the one key".into());
    }
    Ok(out)
}

/// The value now, `None` when the file does not set it.
pub fn current_keep(text: &str) -> Result<Option<u64>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let doc: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    Ok(doc.pointer("/compaction/keepRecentTokens").and_then(|v| v.as_u64()))
}

fn read_text(p: &std::path::Path) -> Result<String, String> {
    match std::fs::read_to_string(p) {
        Ok(t) => Ok(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e.to_string()),
    }
}

/// One backup of the file as it was, then the new one by rename (permissions kept), so pi never reads half of it.
fn write_text(p: &std::path::Path, text: &str) -> std::io::Result<()> {
    let mode = std::fs::metadata(p).ok().map(|m| m.permissions());
    if let Ok(prev) = std::fs::read(p) {
        let _ = crate::config::atomic_write_with_mode(&p.with_extension("json.bak"), &prev, mode.clone(), false);
    }
    crate::config::atomic_write_with_mode(p, text.as_bytes(), mode, false)
}

/// Under pi's lock, and on the real file when `p` is a link (a dotfiles setup keeps its link).
pub fn set_keep_in(p: &std::path::Path, n: u64) -> Result<(), String> {
    let real = p.canonicalize().unwrap_or_else(|_| p.to_owned());
    crate::config::with_lockfile(&real, "settings.json", || {
        let text = read_text(&real)?;
        let out = with_keep(&text, n)?;
        write_text(&real, &out).map_err(|e| e.to_string())
    })
}

pub async fn get(State(_): State<Arc<AppState>>) -> Response {
    let p = path();
    match read_text(&p).and_then(|t| current_keep(&t)) {
        Ok(v) => Json(serde_json::json!({ "keepRecentTokens": v, "default": DEFAULT_KEEP })).into_response(),
        Err(e) => fail(StatusCode::CONFLICT, format!("settings.json: {e}")),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Set {
    keep_recent_tokens: u64,
}

pub async fn set(State(_): State<Arc<AppState>>, Json(req): Json<Set>) -> Response {
    if !(1..=MAX_KEEP).contains(&req.keep_recent_tokens) {
        return fail(StatusCode::BAD_REQUEST, "keepRecentTokens must be a positive number");
    }
    let p = path();
    match tokio::task::spawn_blocking(move || set_keep_in(&p, req.keep_recent_tokens)).await {
        Ok(Ok(())) => Json(serde_json::json!({ "keepRecentTokens": req.keep_recent_tokens, "default": DEFAULT_KEEP })).into_response(),
        Ok(Err(e)) => fail(StatusCode::CONFLICT, format!("settings.json: {e}")),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = "{\n  \"lastChangelogVersion\": \"1.6.1\",\n  \"packages\": [\n    \"npm:a\"\n  ],\n  \"zeta\": { \"x\": [1, {\"y\": \"}\"}] },\n  \"compaction\": {\n    \"enabled\": true,\n    \"reserveTokens\": 240564,\n    \"keepRecentTokens\": 4000\n  },\n  \"last\": 1\n}\n";

    #[test]
    fn replaces_only_the_number() {
        let out = with_keep(REAL, 40000).unwrap();
        assert_eq!(out, REAL.replace("4000\n", "40000\n"));
        assert_eq!(current_keep(&out).unwrap(), Some(40000));
    }

    #[test]
    fn adds_the_key_where_it_is_missing_and_keeps_everything_else() {
        let no_key = REAL.replace(",\n    \"keepRecentTokens\": 4000", "");
        let out = with_keep(&no_key, 20000).unwrap();
        assert!(out.contains("\"compaction\": {\n    \"keepRecentTokens\": 20000,\n    \"enabled\": true"), "{out}");
        assert!(out.starts_with("{\n  \"lastChangelogVersion\"") && out.contains("\"zeta\": { \"x\": [1, {\"y\": \"}\"}] }"));
        let no_section = "{\n    \"a\": 1,\n    \"b\": [2]\n}\n";
        let out = with_keep(no_section, 5).unwrap();
        assert_eq!(out, "{\n    \"compaction\": {\n      \"keepRecentTokens\": 5\n    },\n    \"a\": 1,\n    \"b\": [2]\n}\n");
        let empty_section = "{\n  \"compaction\": {},\n  \"a\": 1\n}";
        assert_eq!(with_keep(empty_section, 5).unwrap(), "{\n  \"compaction\": {\n    \"keepRecentTokens\": 5\n  },\n  \"a\": 1\n}");
        assert_eq!(with_keep("{ \"a\": 1 }", 5).unwrap(), "{ \"compaction\": { \"keepRecentTokens\": 5 }, \"a\": 1 }");
        assert!(with_keep("", 5).unwrap().contains("\"keepRecentTokens\": 5"));
        assert_eq!(with_keep("{}", 5).unwrap(), "{\n  \"compaction\": {\n    \"keepRecentTokens\": 5\n  }\n}\n");
    }

    #[test]
    fn refuses_a_file_it_cannot_read() {
        assert!(with_keep("{ \"a\": ", 5).is_err());
        assert!(with_keep("[1]", 5).is_err());
        assert!(with_keep("{\"compaction\": null}", 5).is_err());
        assert!(current_keep("{ nope").is_err());
        // duplicate key where an earlier value is a string with trailing escape must not panic
        let dup = "{\"compaction\":\"a\\\\\",\"compaction\":{}}";
        assert_eq!(current_keep(&with_keep(dup, 777).unwrap()).unwrap(), Some(777));
    }

    #[test]
    fn writes_with_a_backup_and_keeps_the_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("pi-oven-ps-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("settings.json");
        std::fs::write(&f, REAL).unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
        set_keep_in(&f, 40000).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), REAL.replace("4000\n", "40000\n"));
        assert_eq!(std::fs::read_to_string(dir.join("settings.json.bak")).unwrap(), REAL);
        assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
        // a link stays a link, and the lock folder is gone
        let link = dir.join("link.json");
        std::os::unix::fs::symlink(&f, &link).unwrap();
        set_keep_in(&link, 5000).unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(current_keep(&std::fs::read_to_string(&f).unwrap()).unwrap(), Some(5000));
        assert!(!dir.join("settings.json.lock").exists());
        // a lock pi holds is waited for, then reported
        std::fs::create_dir(dir.join("settings.json.lock")).unwrap();
        assert!(set_keep_in(&f, 1).unwrap_err().contains("locked"));
        std::fs::remove_dir(dir.join("settings.json.lock")).unwrap();
        // a broken file is not written
        std::fs::write(&f, "{ broken").unwrap();
        assert!(set_keep_in(&f, 1).is_err());
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "{ broken");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
