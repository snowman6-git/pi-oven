//! Session trees. A pi session file is a tree of entries (`id` / `parentId`), and the "current
//! position" is simply the last entry in the file. pi's RPC can only walk the active branch, so this
//! module reads the file directly: it summarizes the whole tree for display, and makes a new session
//! out of any path through it (root to one entry), which pi then opens as that branch.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{api::fail, config::random_hex, AppState};

const USER_TEXT: usize = 220;
const OTHER_TEXT: usize = 140;

/// The session file behind `path`, only if it really is one of pi's session files.
fn resolve(app: &AppState, path: &str) -> Result<PathBuf, (StatusCode, &'static str)> {
    let (Ok(file), Ok(root)) = (PathBuf::from(path).canonicalize(), app.sessions_dir.canonicalize()) else {
        return Err((StatusCode::NOT_FOUND, "session not found"));
    };
    if !file.starts_with(&root) || file.extension().and_then(|e| e.to_str()) != Some("jsonl") {
        return Err((StatusCode::FORBIDDEN, "not a session file"));
    }
    Ok(file)
}

// ── reading ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct Raw {
    #[serde(rename = "type")]
    kind: String,
    id: Option<String>,
    #[serde(rename = "parentId")]
    parent_id: Option<String>,
    timestamp: Option<String>,
    message: Option<RawMessage>,
    #[serde(rename = "targetId")]
    target_id: Option<String>,
    label: Option<String>,
    summary: Option<String>,
}

#[derive(Deserialize)]
struct RawMessage {
    role: Option<String>,
    content: Option<Content>,
    #[serde(rename = "toolName")]
    tool_name: Option<String>,
    #[serde(rename = "isError")]
    is_error: Option<bool>,
    command: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Content {
    Text(String),
    Parts(Vec<Part>),
}

#[derive(Deserialize)]
struct Part {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
    name: Option<String>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Entry {
    pub id: String,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    /// user | assistant | tool | bash | compaction | branch | other
    pub kind: &'static str,
    pub text: String,
    pub tools: Vec<String>,
    pub ts: Option<String>,
    pub error: bool,
}

#[derive(Serialize, Debug)]
pub struct Tree {
    pub path: String,
    pub cwd: String,
    #[serde(rename = "leafId")]
    pub leaf_id: Option<String>,
    pub entries: Vec<Entry>,
    /// `label` entries, attached to the entry they name.
    pub labels: HashMap<String, String>,
}

fn clip(s: &str, max: usize) -> String {
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max { flat } else { format!("{}…", flat.chars().take(max).collect::<String>()) }
}

/// What the person typed: skill text pi injects in front of the message is not theirs.
fn typed(s: &str) -> &str {
    match (s.trim_start().starts_with("<skill"), s.find("</skill>")) {
        (true, Some(end)) => s[end + "</skill>".len()..].trim_start(),
        _ => s,
    }
}

fn first_text(c: &Content) -> String {
    match c {
        Content::Text(t) => t.clone(),
        Content::Parts(parts) => parts.iter().find(|p| p.kind.as_deref() == Some("text")).and_then(|p| p.text.clone()).unwrap_or_default(),
    }
}

fn summarize(raw: Raw) -> Option<Entry> {
    let id = raw.id?;
    let mut e = Entry { id, parent_id: raw.parent_id, kind: "other", text: String::new(), tools: Vec::new(), ts: raw.timestamp, error: false };
    match raw.kind.as_str() {
        "message" => {
            let m = raw.message?;
            match m.role.as_deref() {
                Some("user") => {
                    e.kind = "user";
                    e.text = clip(typed(&m.content.as_ref().map(first_text).unwrap_or_default()), USER_TEXT);
                }
                Some("assistant") => {
                    e.kind = "assistant";
                    if let Some(c) = &m.content {
                        e.text = clip(&first_text(c), OTHER_TEXT);
                        if let Content::Parts(parts) = c {
                            e.tools = parts.iter().filter(|p| p.kind.as_deref() == Some("toolCall")).filter_map(|p| p.name.clone()).collect();
                        }
                    }
                }
                Some("toolResult") => {
                    e.kind = "tool";
                    e.tools = m.tool_name.into_iter().collect();
                    e.error = m.is_error.unwrap_or(false);
                }
                Some("bashExecution") => {
                    e.kind = "bash";
                    e.text = clip(&m.command.unwrap_or_default(), OTHER_TEXT);
                }
                _ => {}
            }
        }
        "compaction" => {
            e.kind = "compaction";
            e.text = clip(&raw.summary.unwrap_or_default(), OTHER_TEXT);
        }
        "branch_summary" => {
            e.kind = "branch";
            e.text = clip(&raw.summary.unwrap_or_default(), OTHER_TEXT);
        }
        _ => {}
    }
    Some(e)
}

/// Raw lines of a session file, tolerating a half-written last line (pi may be appending right now).
fn read_lines(file: &PathBuf) -> std::io::Result<Vec<Vec<u8>>> {
    let mut rd = BufReader::new(std::fs::File::open(file)?);
    let mut lines = Vec::new();
    loop {
        let mut buf = Vec::new();
        if rd.read_until(b'\n', &mut buf)? == 0 {
            break;
        }
        if buf.last() == Some(&b'\n') {
            buf.pop();
            lines.push(buf);
        } // a line without its newline is still being written
    }
    Ok(lines)
}

pub fn read_tree(file: &PathBuf) -> Result<Tree, String> {
    let lines = read_lines(file).map_err(|e| e.to_string())?;
    let header: Value = lines.first().and_then(|l| serde_json::from_slice(l).ok()).ok_or("empty or unreadable session file")?;
    if header["type"] != "session" {
        return Err("not a pi session (no header)".into());
    }
    let mut entries = Vec::new();
    let mut labels = HashMap::new();
    for line in &lines[1..] {
        let Ok(raw) = serde_json::from_slice::<Raw>(line) else { continue };
        if raw.kind == "label" {
            if let (Some(t), l) = (raw.target_id.clone(), raw.label.clone()) {
                match l {
                    Some(l) if !l.is_empty() => labels.insert(t, l),
                    _ => labels.remove(&t),
                };
            }
        }
        if let Some(e) = summarize(raw) {
            entries.push(e);
        }
    }
    Ok(Tree {
        path: file.to_string_lossy().into_owned(),
        cwd: header["cwd"].as_str().unwrap_or("").to_owned(),
        leaf_id: entries.last().map(|e| e.id.clone()),
        entries,
        labels,
    })
}

#[derive(Deserialize)]
pub struct TreeQuery {
    path: String,
}

pub async fn tree(State(app): State<Arc<AppState>>, Query(q): Query<TreeQuery>) -> Response {
    let file = match resolve(&app, &q.path) {
        Ok(f) => f,
        Err((code, msg)) => return fail(code, msg),
    };
    match tokio::task::spawn_blocking(move || read_tree(&file)).await {
        Ok(Ok(t)) => Json(t).into_response(),
        Ok(Err(e)) => fail(StatusCode::UNPROCESSABLE_ENTITY, e),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

// ── branching ───────────────────────────────────────────────────────────────

fn civil(ms: u64) -> (i64, u32, u32, u32, u32, u32, u32) {
    let (secs, milli) = ((ms / 1000) as i64, (ms % 1000) as u32);
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Howard Hinnant's days -> civil date.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, (rem / 3600) as u32, (rem % 3600 / 60) as u32, (rem % 60) as u32, milli)
}

/// "2026-10-05T08:04:19.415Z" (inside the file) and "2026-10-05T08-04-19-415Z" (in its name).
fn stamps(ms: u64) -> (String, String) {
    let (y, mo, d, h, mi, s, milli) = civil(ms);
    (format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{milli:03}Z"), format!("{y:04}-{mo:02}-{d:02}T{h:02}-{mi:02}-{s:02}-{milli:03}Z"))
}

/// A time-ordered UUID (version 7 layout), like the ones pi writes.
fn uuid7(ms: u64) -> String {
    let r = random_hex(10); // 20 hex digits: 3 + 1 + 3 + 12 are used, plus spare
    let variant = ["8", "9", "a", "b"][(r.as_bytes()[3] % 4) as usize];
    format!("{:08x}-{:04x}-7{}-{}{}-{}", (ms >> 16) & 0xffff_ffff, ms & 0xffff, &r[0..3], variant, &r[4..7], &r[8..20])
}

#[derive(Deserialize)]
pub struct BranchReq {
    path: String,
    /// The last entry to keep, or none for a session that starts empty.
    leaf: Option<String>,
    /// A user message whose full text should come back too (for "rewrite from this message").
    #[serde(rename = "textOf")]
    text_of: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct Branched {
    pub path: String,
    pub id: String,
    pub cwd: String,
    pub entries: usize,
    /// The full text of the `textOf` message, as typed.
    pub text: Option<String>,
}

/// Copy the path from the root down to `leaf` into a new session file next to the original.
pub fn make_branch(original: &PathBuf, leaf: Option<&str>, text_of: Option<&str>) -> Result<Branched, (StatusCode, String)> {
    let bad = |m: &str| (StatusCode::UNPROCESSABLE_ENTITY, m.to_string());
    let lines = read_lines(original).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let header: Value = lines.first().and_then(|l| serde_json::from_slice(l).ok()).ok_or_else(|| bad("empty or unreadable session file"))?;
    if header["type"] != "session" {
        return Err(bad("not a pi session (no header)"));
    }

    // id -> (parent id, the line as written)
    let mut by_id: HashMap<String, (Option<String>, &Vec<u8>)> = HashMap::new();
    for line in &lines[1..] {
        if let Ok(v) = serde_json::from_slice::<Value>(line) {
            if let Some(id) = v["id"].as_str() {
                by_id.insert(id.to_owned(), (v["parentId"].as_str().map(str::to_owned), line));
            }
        }
    }

    let mut path: Vec<&Vec<u8>> = Vec::new();
    let mut cursor = leaf.map(str::to_owned);
    while let Some(id) = cursor {
        let (parent, line) = by_id.get(&id).ok_or((StatusCode::NOT_FOUND, format!("no entry {id:?} in this session")))?;
        path.push(line);
        if path.len() > by_id.len() {
            return Err(bad("the session's entries loop back on themselves"));
        }
        cursor = parent.clone();
    }
    path.reverse();

    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let (iso, stamp) = stamps(now);
    let id = uuid7(now);
    let mut new_header = header.clone();
    new_header["id"] = id.clone().into();
    new_header["timestamp"] = iso.into();
    new_header["parentSession"] = original.to_string_lossy().into_owned().into();

    let dest = original.with_file_name(format!("{stamp}_{id}.jsonl"));
    let mut out = std::fs::OpenOptions::new().write(true).create_new(true).open(&dest).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let write = |out: &mut std::fs::File| -> std::io::Result<()> {
        out.write_all(new_header.to_string().as_bytes())?;
        out.write_all(b"\n")?;
        for line in &path {
            out.write_all(line)?;
            out.write_all(b"\n")?;
        }
        out.flush()
    };
    if let Err(e) = write(&mut out) {
        let _ = std::fs::remove_file(&dest);
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
    }
    // The message to rewrite lives on another branch or after the leaf, so it is looked up in the whole file.
    let text = text_of.and_then(|want| {
        let (_, line) = by_id.get(want)?;
        let raw: Raw = serde_json::from_slice(line).ok()?;
        let content = raw.message?.content?;
        Some(typed(&first_text(&content)).trim().to_owned())
    });
    Ok(Branched { path: dest.to_string_lossy().into_owned(), id, cwd: header["cwd"].as_str().unwrap_or("").to_owned(), entries: path.len(), text })
}

#[derive(Deserialize)]
pub struct RenameReq {
    path: String,
    name: String,
}

/// Names a session that is not open anywhere, the way pi does: one more entry at the end of the file,
/// hung off the last one (an empty name takes the name away). A session that is open is renamed through
/// its own pi process, which owns the file while it runs.
pub fn rename_session(file: &PathBuf, name: &str) -> Result<(), (StatusCode, String)> {
    let bad = |m: &str| (StatusCode::UNPROCESSABLE_ENTITY, m.to_owned());
    let name: String = name.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_owned();
    if name.chars().count() > 200 {
        return Err(bad("the name is too long"));
    }
    let mut bytes = std::fs::File::open(file).map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    {
        use std::io::{Read, Seek, SeekFrom};
        let len = bytes.metadata().map(|m| m.len()).unwrap_or(0);
        if len == 0 {
            return Err(bad("empty session file"));
        }
        let mut last = [0u8; 1];
        bytes.seek(SeekFrom::Start(len - 1)).and_then(|_| bytes.read_exact(&mut last)).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        if last[0] != b'\n' {
            return Err((StatusCode::CONFLICT, "the session is being written".to_owned()));
        }
    }
    let lines = read_lines(file).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let header_ok = lines.first().and_then(|l| serde_json::from_slice::<Value>(l).ok()).is_some_and(|v| v["type"] == "session");
    if !header_ok {
        return Err(bad("not a pi session file"));
    }
    let leaf = lines.iter().skip(1).rev().find_map(|l| serde_json::from_slice::<Value>(l).ok().and_then(|v| v["id"].as_str().map(str::to_owned)));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let entry = serde_json::json!({ "type": "session_info", "id": random_hex(4), "parentId": leaf, "timestamp": stamps(now).0, "name": name });
    let mut f = std::fs::OpenOptions::new().append(true).open(file).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    use std::io::Write;
    f.write_all(format!("{entry}\n").as_bytes()).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

pub async fn rename(State(app): State<Arc<AppState>>, Json(req): Json<RenameReq>) -> Response {
    let file = match resolve(&app, &req.path) {
        Ok(f) => f,
        Err((code, msg)) => return fail(code, msg),
    };
    if app.runtimes.list().await.iter().any(|r| r.session_path.as_deref().map(std::path::Path::new).and_then(|p| p.canonicalize().ok()).as_ref() == Some(&file)) {
        return fail(StatusCode::CONFLICT, "the session is open: rename it from its own window");
    }
    match tokio::task::spawn_blocking(move || rename_session(&file, &req.name)).await {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err((code, msg))) => fail(code, msg),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(Deserialize)]
struct Light {
    id: Option<String>,
    #[serde(rename = "parentId")]
    parent_id: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(rename = "customType")]
    custom_type: Option<String>,
}

/// The newest `custom` entry of this type on the way from the start to where the session stands now (the last
/// entry of the file): extensions keep their state this way (`appendEntry`), and it survives compaction, which
/// the conversation's own messages do not. Entries on other branches do not count.
pub fn latest_state(file: &PathBuf, custom_type: &str) -> std::io::Result<Option<Value>> {
    let lines = read_lines(file)?;
    let mut parent: HashMap<String, Option<String>> = HashMap::new();
    let mut found: HashMap<String, usize> = HashMap::new();
    let mut leaf: Option<String> = None;
    for (i, line) in lines.iter().enumerate().skip(1) {
        let Ok(e) = serde_json::from_slice::<Light>(line) else { continue };
        let Some(id) = e.id else { continue };
        if e.kind.as_deref() == Some("custom") && e.custom_type.as_deref() == Some(custom_type) {
            found.insert(id.clone(), i);
        }
        parent.insert(id.clone(), e.parent_id);
        leaf = Some(id);
    }
    let mut cursor = leaf;
    for _ in 0..=parent.len() {
        let Some(id) = cursor else { break };
        if let Some(&i) = found.get(&id) {
            return Ok(serde_json::from_slice::<Value>(&lines[i]).ok().map(|v| v["data"].clone()));
        }
        cursor = parent.get(&id).cloned().flatten();
    }
    Ok(None)
}

#[derive(Deserialize)]
pub struct StateQuery {
    path: String,
    #[serde(rename = "type")]
    kind: String,
}

pub async fn state(State(app): State<Arc<AppState>>, Query(q): Query<StateQuery>) -> Response {
    if q.kind.is_empty() || q.kind.len() > 64 || !q.kind.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) {
        return fail(StatusCode::BAD_REQUEST, "bad type");
    }
    let file = match resolve(&app, &q.path) {
        Ok(f) => f,
        Err((code, msg)) => return fail(code, msg),
    };
    match tokio::task::spawn_blocking(move || latest_state(&file, &q.kind)).await {
        Ok(Ok(data)) => Json(serde_json::json!({ "data": data })).into_response(),
        Ok(Err(e)) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(Deserialize)]
pub struct TailQuery {
    path: String,
    #[serde(default = "default_tail_n")]
    n: u32,
}

fn default_tail_n() -> u32 {
    20
}

/// The last `n` lines of a session file. Background-task notices only ever land in the file — the RPC
/// stream never emits them — so the client reads them out of the tail while a task awaits its notice.
pub async fn tail(State(app): State<Arc<AppState>>, Query(q): Query<TailQuery>) -> Response {
    let n = q.n.clamp(1, 100);
    let file = match resolve(&app, &q.path) {
        Ok(f) => f,
        Err((code, msg)) => return fail(code, msg),
    };
    match tokio::task::spawn_blocking(move || tail_lines(&file, n)).await {
        Ok(Ok(lines)) => Json(serde_json::json!({ "lines": lines })).into_response(),
        Ok(Err(e)) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

fn tail_lines(file: &Path, n: u32) -> Result<Vec<String>, String> {
    let data = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
    Ok(data.lines().rev().take(n as usize).map(str::to_string).collect::<Vec<_>>().into_iter().rev().collect())
}

pub async fn branch(State(app): State<Arc<AppState>>, Json(req): Json<BranchReq>) -> Response {
    let file = match resolve(&app, &req.path) {
        Ok(f) => f,
        Err((code, msg)) => return fail(code, msg),
    };
    match tokio::task::spawn_blocking(move || make_branch(&file, req.leaf.as_deref(), req.text_of.as_deref())).await {
        Ok(Ok(b)) => Json(b).into_response(),
        Ok(Err((code, msg))) => fail(code, msg),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_formatted_in_utc_the_way_pi_names_files() {
        assert_eq!(stamps(0).0, "1970-01-01T00:00:00.000Z");
        assert_eq!(stamps(1_793_132_632_863), ("2026-10-27T20:23:52.863Z".to_string(), "2026-10-27T20-23-52-863Z".to_string()));
        assert_eq!(stamps(951_827_696_005).0, "2000-02-29T12:34:56.005Z", "a leap day");
        assert_eq!(stamps(4_102_444_799_999).0, "2099-12-31T23:59:59.999Z");
    }

    #[test]
    fn new_ids_look_like_pis_and_do_not_repeat() {
        let a = uuid7(1_793_132_632_863);
        let b = uuid7(1_793_132_632_863);
        assert_ne!(a, b);
        let parts: Vec<&str> = a.split('-').collect();
        assert_eq!(parts.iter().map(|p| p.len()).collect::<Vec<_>>(), [8, 4, 4, 4, 12], "{a}");
        assert!(parts[2].starts_with('7') && "89ab".contains(&parts[3][..1]), "{a}");
        assert!(a.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
    }

    #[test]
    fn skill_text_pi_injects_is_not_what_was_typed() {
        assert_eq!(typed("<skill name=\"x\">long text</skill>\n\n진짜 질문"), "진짜 질문");
        assert_eq!(typed("그냥 질문"), "그냥 질문");
        assert_eq!(typed("<skill never closed"), "<skill never closed");
    }

    #[test]
    fn text_is_flattened_and_clipped_by_characters() {
        assert_eq!(clip("a\n\n  b   c", 20), "a b c");
        assert_eq!(clip(&"가".repeat(10), 4), "가가가가…");
    }
}
