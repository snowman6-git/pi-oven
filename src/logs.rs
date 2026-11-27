//! What the server keeps on disk to look back on, in `<cache>/logs/` (`$XDG_CACHE_HOME/pi-oven/logs`):
//!
//! - `server.log`: the server's own log, the same lines as the terminal (without colours). Moved to `server.log.1` at
//!   10 MB, so at most two are kept.
//! - `events-<YYYY-MM-DD>.jsonl` (UTC): one line per thing worth looking back on, each with `t` (ms), `kind` and the
//!   session (`rt`). Kept for good, to look back on a problem that grew slowly; only when the files together pass
//!   `EVENTS_MAX` (a busy week is about a megabyte) are the oldest days removed.
//!   - `reply`: every reply of the model: provider, model, stop reason, tokens (input, output, cache read, cache write),
//!     whether the prompt cache broke (`cacheBreak`), and what the bundled extension saw of the request (`req`: how many
//!     messages, from which one on it differed from the request before, whether the system prompt or tools changed).
//!   - `limits`: the subscription's numbers whenever they change (`used` per window), so a jump can be put next to the
//!     replies that came before it.
//!   - `retry`, `limit`, `baton`, `resume`, `halt`: what the server decided when a reply failed.
//!
//! Nothing of the conversation itself is written: no prompts, no replies, no file contents. Error messages are cut to
//! 300 characters.

use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

const SERVER_MAX: u64 = 10 * 1024 * 1024;
const EVENTS_MAX: u64 = 512 * 1024 * 1024;
const DAY_MS: u64 = 86_400_000;

pub fn dir() -> PathBuf {
    crate::config::cache_dir().join("logs")
}

// ── server.log ──────────────────────────────────────────────────────────────

struct Rolling {
    path: PathBuf,
    file: Option<File>,
    size: u64,
}

impl Rolling {
    fn open(path: PathBuf) -> Self {
        let file = OpenOptions::new().create(true).append(true).open(&path).ok();
        let size = file.as_ref().and_then(|f| f.metadata().ok()).map_or(0, |m| m.len());
        Self { path, file, size }
    }

    fn write(&mut self, buf: &[u8]) {
        if self.size + buf.len() as u64 > SERVER_MAX {
            self.file = None;
            let _ = std::fs::rename(&self.path, self.path.with_extension("log.1"));
            *self = Self::open(self.path.clone());
        }
        if let Some(f) = self.file.as_mut() {
            if f.write_all(buf).is_ok() {
                self.size += buf.len() as u64;
            }
        }
    }
}

/// The writer `tracing` gets for the file: one formatted line per `write_all`.
#[derive(Clone)]
pub struct ServerLog(Arc<Mutex<Rolling>>);

impl ServerLog {
    pub fn open() -> Option<Self> {
        let d = dir();
        std::fs::create_dir_all(&d).ok()?;
        Some(Self(Arc::new(Mutex::new(Rolling::open(d.join("server.log"))))))
    }
}

impl Write for ServerLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Ok(mut r) = self.0.lock() {
            r.write(buf);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// ── events-<date>.jsonl ─────────────────────────────────────────────────────

struct Daily {
    dir: PathBuf,
    day: u64,
    file: Option<File>,
}

static EVENTS: OnceLock<Mutex<Daily>> = OnceLock::new();

fn day_name(day: u64) -> String {
    let (y, m, d, ..) = crate::branch::civil(day * DAY_MS);
    format!("events-{y:04}-{m:02}-{d:02}.jsonl")
}

/// The day files over `max` bytes together: the oldest go until they fit (today's never). Only our own files.
fn prune(dir: &Path, today: u64, max: u64) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(String, u64, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let ours = name.starts_with("events-") && name.ends_with(".jsonl") && name.len() == "events-0000-00-00.jsonl".len();
            ours.then(|| (name, e.metadata().map_or(0, |m| m.len()), e.path()))
        })
        .collect();
    files.sort(); // by date: the names sort as the days do
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    let today = day_name(today);
    for (name, size, path) in files {
        if total <= max || name == today {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= size;
        }
    }
}

/// Keep one event: `{"t":…,"kind":…, …fields}`. Never fails the caller; a disk that cannot be written loses the line.
pub fn event(kind: &str, mut fields: serde_json::Value) {
    if cfg!(test) {
        return;
    }
    let now = crate::runtime::now_ms();
    if !fields.is_object() {
        fields = serde_json::json!({ "value": fields });
    }
    fields["t"] = now.into();
    fields["kind"] = kind.into();
    let mut line = fields.to_string();
    line.push('\n');
    let cell = EVENTS.get_or_init(|| Mutex::new(Daily { dir: dir(), day: 0, file: None }));
    let Ok(mut g) = cell.lock() else { return };
    let today = now / DAY_MS;
    if g.day != today || g.file.is_none() {
        let _ = std::fs::create_dir_all(&g.dir);
        prune(&g.dir, today, EVENTS_MAX);
        g.file = OpenOptions::new().create(true).append(true).open(g.dir.join(day_name(today))).ok();
        g.day = today;
    }
    if let Some(f) = g.file.as_mut() {
        let _ = f.write_all(line.as_bytes());
    }
}

/// An error message for a log line: at most 300 characters.
pub fn clip(s: &str) -> String {
    s.chars().take(300).collect()
}

// ── requests ────────────────────────────────────────────────────────────────

/// The bundled extension's look at a request (status line `oven-trace`, see `ext/oven-limits.ts`). Only the numbers
/// and flags it is meant to carry are kept: the line is written by code running in pi.
pub fn trace_of(line: &str) -> Option<serde_json::Value> {
    if !line.starts_with(r#"{"type":"extension_ui_request""#) || !line.contains(r#""oven-trace""#) {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v["method"] != "setStatus" || v["statusKey"] != "oven-trace" {
        return None;
    }
    let t: serde_json::Value = serde_json::from_str(v["statusText"].as_str()?).unwrap_or_default();
    let mut out = serde_json::Map::new();
    for k in ["at", "n", "prevN", "first", "bytes"] {
        if t[k].is_u64() {
            out.insert(k.into(), t[k].clone());
        }
    }
    // `first: null`: the request before is all still there (only added to)
    if t.get("first").is_some_and(serde_json::Value::is_null) {
        out.insert("first".into(), serde_json::Value::Null);
    }
    for k in ["sys", "tools"] {
        if let Some(b) = t[k].as_bool() {
            out.insert(k.into(), b.into());
        }
    }
    if let Some(r) = t["role"].as_str().filter(|r| r.len() <= 20 && r.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')) {
        out.insert("role".into(), r.into());
    }
    if let Some(m) = t["marks"].as_array() {
        out.insert("marks".into(), m.iter().filter(|x| x.is_u64()).take(8).cloned().collect::<Vec<_>>().into());
    }
    Some(out.into())
}

// ── replies ─────────────────────────────────────────────────────────────────

/// What a session remembers between replies to tell a broken prompt cache.
#[derive(Default)]
pub struct Replies {
    /// The prompt of the last reply that got through: its size (cache read + write + input) and when.
    last: Option<(u64, u64)>,
    /// What the extension said about the next request (`oven-trace`), waiting for its reply.
    pub request: Option<serde_json::Value>,
}

/// One reply, read from pi's `message_end` of the assistant.
pub struct Reply {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub stop: Option<String>,
    pub input: u64,
    pub output: u64,
    pub read: u64,
    pub write: u64,
    pub error: Option<String>,
}

/// pi's `message_end` of an assistant reply. Only the small fields at its end are read; the content is skipped by
/// serde without being kept.
pub fn reply_of(line: &str) -> Option<Reply> {
    if !line.starts_with(r#"{"type":"message_end""#) || !line.contains(r#""role":"assistant""#) {
        return None;
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Usage {
        #[serde(default)]
        input: u64,
        #[serde(default)]
        output: u64,
        #[serde(default)]
        cache_read: u64,
        #[serde(default)]
        cache_write: u64,
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Msg {
        role: String,
        provider: Option<String>,
        model: Option<String>,
        stop_reason: Option<String>,
        usage: Option<Usage>,
        error_message: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct End {
        message: Msg,
    }
    let end: End = serde_json::from_str(line).ok()?;
    let m = end.message;
    if m.role != "assistant" {
        return None;
    }
    let u = m.usage.unwrap_or(Usage { input: 0, output: 0, cache_read: 0, cache_write: 0 });
    Some(Reply { provider: m.provider, model: m.model, stop: m.stop_reason, input: u.input, output: u.output, read: u.cache_read, write: u.cache_write, error: m.error_message })
}

/// The prompt cache broke: within the hour the cache lasts, a prompt at least as big as the last one read less than
/// half of it from the cache and wrote the rest again. (A cache written with the short 5-minute life is read as broken
/// after 5 minutes too, which is what it costs.)
pub fn cache_broke(last: Option<(u64, u64)>, r: &Reply, now: u64) -> bool {
    let Some((size, at)) = last else { return false };
    now.saturating_sub(at) < 3_600_000 && size >= 20_000 && r.read < size / 2 && r.write >= size / 2
}

impl Replies {
    /// Log a reply; returns whether the cache broke.
    pub fn record(&mut self, rt: &str, r: &Reply, now: u64) -> bool {
        let ok = r.stop.as_deref() != Some("error") && r.stop.as_deref() != Some("aborted");
        let broke = ok && cache_broke(self.last, r, now);
        let mut v = serde_json::json!({
            "rt": rt,
            "provider": r.provider,
            "model": r.model,
            "stop": r.stop,
            "input": r.input,
            "output": r.output,
            "cacheRead": r.read,
            "cacheWrite": r.write,
        });
        if broke {
            v["cacheBreak"] = true.into();
            v["lastPrompt"] = self.last.map(|l| l.0).into();
        }
        if let Some(e) = &r.error {
            v["error"] = clip(e).into();
        }
        if let Some(q) = self.request.take() {
            v["req"] = q;
        }
        event("reply", v);
        if ok && r.read + r.write + r.input > 0 {
            self.last = Some((r.read + r.write + r.input, now));
        }
        broke
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(read: u64, write: u64) -> Reply {
        Reply { provider: None, model: None, stop: Some("toolUse".into()), input: 4, output: 100, read, write, error: None }
    }

    #[test]
    fn reads_a_reply_without_its_content() {
        let l = r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"\"role\":\"user\""}],"provider":"anthropic","model":"claude-sonnet-5-5","usage":{"input":4,"output":1130,"cacheRead":24946,"cacheWrite":137601,"totalTokens":163681,"cost":{"total":1}},"stopReason":"toolUse"}}"#;
        let r = reply_of(l).unwrap();
        assert_eq!((r.read, r.write, r.output), (24946, 137601, 1130));
        assert_eq!(r.model.as_deref(), Some("claude-sonnet-5-5"));
        assert!(reply_of(r#"{"type":"message_end","message":{"role":"user","content":"x"}}"#).is_none());
    }

    #[test]
    fn a_cache_that_broke_mid_conversation_is_told_from_one_that_grew() {
        // what the session on the test server did: 178k read before, then only the system prompt
        let last = Some((186_440, 0));
        assert!(cache_broke(last, &reply(24_946, 137_601), 9_000));
        // a normal step: the old prompt read, the new part written
        assert!(!cache_broke(last, &reply(186_436, 1_200), 9_000));
        // past the cache's hour: a new write is expected
        assert!(!cache_broke(last, &reply(0, 190_000), 3_700_000));
        // a compaction made the prompt small
        assert!(!cache_broke(last, &reply(24_946, 8_000), 9_000));
    }

    #[test]
    fn the_extensions_look_at_a_request_keeps_only_its_numbers() {
        let text = r#"{"at":1,"n":120,"prevN":118,"first":0,"role":"user","sys":false,"tools":false,"marks":[118,119],"bytes":900000,"secret":"x"}"#;
        // as pi prints it: `type` first
        let line = format!(r#"{{"type":"extension_ui_request","id":"1","method":"setStatus","statusKey":"oven-trace","statusText":{}}}"#, serde_json::Value::from(text));
        let t = trace_of(&line).unwrap();
        assert_eq!(t["first"], 0);
        assert_eq!(t["role"], "user");
        assert!(t.get("secret").is_none());
        assert!(trace_of(r#"{"type":"extension_ui_request","id":"1","method":"setStatus","statusKey":"oven-limits","statusText":"oven-trace"}"#).is_none());
    }

    #[test]
    fn old_days_stay_until_the_files_are_too_big() {
        let dir = std::env::temp_dir().join(format!("pi-oven-logs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let today = 1_795_353_568_792 / DAY_MS;
        for d in 0..30 {
            std::fs::write(dir.join(day_name(today - d)), vec![b'x'; 100]).unwrap();
        }
        std::fs::write(dir.join("other.jsonl"), "x").unwrap();
        prune(&dir, today, 1_000_000);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 31, "a month of small files is kept");
        prune(&dir, today, 1_000);
        let left: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(left.len(), 11, "{left:?}"); // the 10 newest days and the file that is not ours
        assert!(left.contains(&day_name(today)) && !left.contains(&day_name(today - 10)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn day_files_are_named_by_utc_date() {
        assert_eq!(day_name(1_795_353_568_792 / DAY_MS), "events-2026-11-22.jsonl");
    }
}
