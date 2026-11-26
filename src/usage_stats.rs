//! Tokens used so far, per day, provider and model, from every pi session file under the sessions folder
//! (`GET /api/usage/stats`). The same response is copied into several files (sub-agent runs keep their own copy of the
//! session), so each is counted once, by its `responseId`. A file is read on from where it stopped, like `sessions.rs`.

use axum::{
    extract::{Query, State},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const MAX_DAYS: usize = 400;
const DAY_MS: i64 = 86_400_000;

#[derive(Clone)]
struct Entry {
    key: String,
    ts: i64,
    provider: String,
    model: String,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    cost: f64,
}

#[derive(Default)]
struct Sum {
    requests: u64,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    cost: f64,
}

#[derive(Clone, Default)]
struct FileUsage {
    size: u64,
    /// bytes read, up to the last whole line
    at: u64,
    cwd: Option<String>,
    entries: Vec<Entry>,
}

#[derive(Default)]
pub struct UsageIndex {
    cache: Mutex<HashMap<PathBuf, FileUsage>>,
}

fn num(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_f64).map_or(0, |n| n.max(0.0) as u64)
}

fn entry_of(line: &str) -> Option<Entry> {
    let v: Value = serde_json::from_str(line).ok()?;
    let m = v.get("message")?;
    if m.get("role")?.as_str()? != "assistant" {
        return None;
    }
    let u = m.get("usage")?;
    let provider = m.get("provider")?.as_str()?.to_owned();
    let model = m.get("model").and_then(Value::as_str).unwrap_or("").to_owned();
    let ts = m.get("timestamp").and_then(Value::as_f64)? as i64;
    let (input, output, cache_read, cache_write) = (num(u, "input"), num(u, "output"), num(u, "cacheRead"), num(u, "cacheWrite"));
    let key = match m.get("responseId").and_then(Value::as_str) {
        Some(id) if !id.is_empty() => format!("id:{id}"),
        _ => format!("{provider}|{model}|{ts}|{output}|{cache_read}"),
    };
    // a failed request (a limit, an overload) comes back with no tokens: not a request that used anything
    if input + output + cache_read + cache_write == 0 {
        return None;
    }
    let cost = u.get("cost").and_then(|c| c.get("total")).and_then(Value::as_f64).unwrap_or(0.0);
    Some(Entry { key, ts, provider, model, input, output, cache_read, cache_write, cost })
}

/// The file's usage; `prev` is read on from where it stopped, otherwise from the start.
fn parse(path: &Path, size: u64, prev: Option<FileUsage>) -> Option<FileUsage> {
    let mut rd = BufReader::new(std::fs::File::open(path).ok()?);
    let mut buf = Vec::new();
    let mut out = match prev {
        Some(p) => {
            rd.seek(SeekFrom::Start(p.at)).ok()?;
            p
        }
        None => {
            let n = rd.read_until(b'\n', &mut buf).ok()?;
            let header: Value = serde_json::from_slice(&buf).ok()?;
            if header.get("type")?.as_str()? != "session" {
                return None;
            }
            FileUsage { at: n as u64, cwd: header.get("cwd").and_then(Value::as_str).map(str::to_owned), ..Default::default() }
        }
    };
    loop {
        buf.clear();
        let n = rd.read_until(b'\n', &mut buf).ok()?;
        // a line pi is still writing is read next time, whole
        if n == 0 || buf.last() != Some(&b'\n') {
            break;
        }
        out.at += n as u64;
        let line = String::from_utf8_lossy(&buf);
        // cheap check first: message lines can be megabytes
        if line.contains(r#""usage""#) && line.contains(r#""role":"assistant""#) {
            out.entries.extend(entry_of(&line));
        }
    }
    out.size = size;
    Some(out)
}

fn jsonl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(t) = e.file_type() else { continue };
        let p = e.path();
        if t.is_dir() {
            jsonl_files(&p, out);
        } else if t.is_file() && p.extension().and_then(|x| x.to_str()) == Some("jsonl") {
            out.push(p);
        }
    }
}

/// "YYYY-MM-DD" of a day number counted from 1970-01-01.
fn day_name(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

impl UsageIndex {
    /// `tz` is JS `getTimezoneOffset()`: minutes UTC is ahead of local time.
    pub fn stats(&self, dir: &Path, root: Option<&Path>, tz: i64) -> Value {
        let mut files = Vec::new();
        jsonl_files(dir, &mut files);
        files.sort();
        let mut seen_keys = HashSet::new();
        let mut counted = 0;
        // day -> (provider, model) -> sums
        let mut days: BTreeMap<i64, BTreeMap<(String, String), Sum>> = BTreeMap::new();
        let mut alive = HashSet::new();
        for p in files {
            let Ok(meta) = std::fs::metadata(&p) else { continue };
            let size = meta.len();
            let hit = self.cache.lock().unwrap().get(&p).cloned();
            let usage = match hit {
                Some(h) if h.size == size => h,
                // grown: read on; shrunk: from the start
                prev => {
                    let prev = prev.filter(|h| size >= h.size && size >= h.at);
                    let Some(fresh) = parse(&p, size, prev) else { continue };
                    self.cache.lock().unwrap().insert(p.clone(), fresh.clone());
                    fresh
                }
            };
            alive.insert(p);
            if root.is_some() && !usage.cwd.as_deref().is_some_and(|c| crate::scope::allows(root, Path::new(c))) {
                continue;
            }
            counted += 1;
            for e in &usage.entries {
                if !seen_keys.insert(e.key.clone()) {
                    continue;
                }
                let day = (e.ts - tz * 60_000).div_euclid(DAY_MS);
                let r = days.entry(day).or_default().entry((e.provider.clone(), e.model.clone())).or_default();
                r.requests += 1;
                r.input += e.input;
                r.output += e.output;
                r.cache_read += e.cache_read;
                r.cache_write += e.cache_write;
                r.cost += e.cost;
            }
        }
        self.cache.lock().unwrap().retain(|k, _| alive.contains(k));
        let skip = days.len().saturating_sub(MAX_DAYS);
        let days: Vec<_> = days
            .into_iter()
            .skip(skip)
            .map(|(d, rows)| {
                let rows: Vec<_> = rows
                    .into_iter()
                    .map(|((provider, model), r)| json!({ "provider": provider, "model": model, "requests": r.requests, "input": r.input, "output": r.output, "cacheRead": r.cache_read, "cacheWrite": r.cache_write, "cost": r.cost }))
                    .collect();
                json!({ "day": day_name(d), "rows": rows })
            })
            .collect();
        json!({ "since": days.first().map(|d| d["day"].clone()), "days": days, "files": counted })
    }
}

#[derive(Deserialize)]
pub struct StatsQuery {
    tz: Option<i64>,
}

pub async fn get(State(app): State<Arc<crate::AppState>>, Query(q): Query<StatsQuery>) -> Response {
    // a bogus offset only shifts the days; keep it within what a clock can be
    let tz = q.tz.unwrap_or(0).clamp(-14 * 60, 14 * 60);
    match tokio::task::spawn_blocking(move || app.usage_stats.stats(&app.sessions_dir, app.root.as_deref(), tz)).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => crate::api::fail(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn msg(id: &str, ts: i64, out: u64) -> String {
        format!(r#"{{"type":"message","message":{{"role":"assistant","provider":"anthropic","model":"m","responseId":"{id}","timestamp":{ts},"usage":{{"input":1,"output":{out},"cacheRead":2,"cacheWrite":3,"cost":{{"total":0.5}}}}}}}}"#)
    }

    #[test]
    fn counts_once_reads_on_and_cuts_days_by_zone() {
        let dir = std::env::temp_dir().join(format!("oven-usage-{}", std::process::id()));
        let deep = dir.join("cwd/sid/run/run-0");
        std::fs::create_dir_all(&deep).unwrap();
        let head = "{\"type\":\"session\",\"id\":\"x\",\"cwd\":\"/\"}\n";
        // 2026-01-01 23:30 UTC
        let t = 1_767_310_200_000;
        let main = dir.join("cwd/a.jsonl");
        std::fs::write(&main, format!("{head}{}\n{}\n{{\"type\":\"mess", msg("a", t, 10), "{\"type\":\"message\",\"message\":{\"role\":\"user\"}}")).unwrap();
        // the sub-agent copy of response "a", and its own response "b"
        std::fs::write(deep.join("session.jsonl"), format!("{head}{}\n{}\n", msg("a", t, 10), msg("b", t + DAY_MS, 20))).unwrap();
        let idx = UsageIndex::default();
        let s = idx.stats(&dir, None, 0);
        assert_eq!(s["files"], 2);
        assert_eq!(s["days"][0]["day"], "2026-01-01");
        assert_eq!(s["days"][0]["rows"][0]["requests"], 1);
        assert_eq!(s["days"][1]["rows"][0]["output"], 20);
        // the half line is finished and one more comes
        let mut f = std::fs::OpenOptions::new().append(true).open(&main).unwrap();
        write!(f, "age\",\"message\":{{}}}}\n{}\n", msg("c", t, 5)).unwrap();
        let s = idx.stats(&dir, None, 0);
        assert_eq!(s["days"][0]["rows"][0]["requests"], 2);
        // KST (-540): 23:30 UTC is already the next day
        let s = idx.stats(&dir, None, -540);
        assert_eq!(s["days"][0]["day"], "2026-01-02");
        // a file that shrinks is read again from the start
        std::fs::write(&main, format!("{head}{}\n", msg("a", t, 10))).unwrap();
        assert_eq!(idx.stats(&dir, None, 0)["days"][0]["rows"][0]["requests"], 1);
        // outside --root
        assert_eq!(idx.stats(&dir, Some(Path::new("/nowhere")), 0)["files"], 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
