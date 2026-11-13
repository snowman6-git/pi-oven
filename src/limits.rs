//! Subscription usage limits (ChatGPT, Claude): the latest numbers each provider sent, per provider.
//!
//! The bundled extension (`ext/oven-limits.ts`) reads them off each response and reports them as the status line
//! `oven-limits`. They belong to the account, not the session, so they are kept here for every session and device,
//! and in a file so a restart does not empty them. Only replies bring new numbers: nothing is fetched.

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, sync::Mutex};

use crate::{runtime::now_ms, AppState};

const STATUS_KEY: &str = "oven-limits";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub minutes: u32,
    pub used: f64,
    pub reset_at: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub provider: String,
    pub plan: Option<String>,
    pub at: u64,
    #[serde(default)]
    pub limit_reached: bool,
    pub windows: Vec<Window>,
}

impl Entry {
    /// Keeps only what the page can show safely: the status line is written by code running in pi.
    fn clean(mut self) -> Option<Self> {
        let name_ok = |s: &str, max: usize| !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
        if !name_ok(&self.provider, 64) {
            return None;
        }
        self.plan = self.plan.filter(|p| name_ok(p, 32));
        self.windows.retain(|w| (1..=100_000).contains(&w.minutes) && w.used.is_finite());
        self.windows.truncate(4);
        for w in &mut self.windows {
            w.used = w.used.clamp(0.0, 100.0);
        }
        self.windows.sort_by_key(|w| w.minutes);
        (!self.windows.is_empty()).then_some(self)
    }

    /// For the page: times as "how long from now", since a phone's clock need not agree with the server's.
    fn view(&self, now: u64) -> serde_json::Value {
        serde_json::json!({
            "provider": self.provider,
            "plan": self.plan,
            "ageMs": now.saturating_sub(self.at),
            "limitReached": self.limit_reached,
            "windows": self.windows.iter().map(|w| serde_json::json!({
                "minutes": w.minutes,
                "used": w.used,
                "resetInMs": w.reset_at.map(|t| t as i64 - now as i64),
            })).collect::<Vec<_>>(),
        })
    }
}

/// The `oven-limits` status line from pi's stdout, if this is one.
pub fn read(line: &str) -> Option<Entry> {
    if !line.starts_with(r#"{"type":"extension_ui_request""#) || !line.contains(STATUS_KEY) {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v["method"] != "setStatus" || v["statusKey"] != STATUS_KEY {
        return None;
    }
    serde_json::from_str::<Entry>(v["statusText"].as_str()?).ok()?.clean()
}

#[derive(Default)]
pub struct Limits {
    map: Mutex<BTreeMap<String, Entry>>,
    file: Option<PathBuf>,
}

impl Limits {
    pub fn load(extension: Option<&PathBuf>) -> Self {
        let file = extension.and_then(|e| e.parent()).map(|d| d.join("limits.json"));
        let map = file
            .as_ref()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|b| serde_json::from_slice::<Vec<Entry>>(&b).ok())
            .unwrap_or_default()
            .into_iter()
            .filter_map(Entry::clean)
            .map(|e| (e.provider.clone(), e))
            .collect();
        Self { map: Mutex::new(map), file }
    }

    pub fn record(&self, e: Entry) {
        let all: Vec<Entry> = {
            let mut map = self.map.lock().unwrap();
            if map.get(&e.provider).is_some_and(|old| old.at > e.at) {
                return;
            }
            map.insert(e.provider.clone(), e);
            map.values().cloned().collect()
        };
        let Some(file) = &self.file else { return };
        let tmp = file.with_extension("tmp");
        if serde_json::to_vec(&all).is_ok_and(|b| std::fs::write(&tmp, b).is_ok()) {
            let _ = std::fs::rename(&tmp, file);
        }
    }

    /// When a used-up window of this provider resets (the latest, if more than one is used up).
    pub fn reset_of(&self, provider: Option<&str>, now: u64) -> Option<u64> {
        let map = self.map.lock().unwrap();
        let entries: Vec<&Entry> = match provider {
            Some(p) => map.get(p).into_iter().collect(),
            None => map.values().collect(),
        };
        entries
            .iter()
            .flat_map(|e| e.windows.iter())
            .filter(|w| w.used >= 99.5)
            .filter_map(|w| w.reset_at)
            .filter(|&t| t > now)
            .max()
    }

    pub fn view(&self, now: u64) -> serde_json::Value {
        let map = self.map.lock().unwrap();
        serde_json::json!({ "items": map.values().map(|e| e.view(now)).collect::<Vec<_>>() })
    }
}

pub async fn get(State(app): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(app.runtimes.limits.view(now_ms()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str) -> String {
        // pi writes `type` first; `json!` would sort the keys
        format!(r#"{{"type":"extension_ui_request","id":"u","method":"setStatus","statusKey":"{STATUS_KEY}","statusText":{}}}"#, serde_json::Value::from(text))
    }

    #[test]
    fn reads_the_status_line() {
        let text = r#"{"provider":"openai-codex","plan":"plus","at":5,"limitReached":false,"windows":[{"minutes":10080,"used":27,"resetAt":9},{"minutes":300,"used":75,"resetAt":7}]}"#;
        let e = read(&line(text)).unwrap();
        assert_eq!(e.provider, "openai-codex");
        assert_eq!(e.windows.iter().map(|w| w.minutes).collect::<Vec<_>>(), [300, 10080]);
        let v = e.view(6);
        assert_eq!(v["ageMs"], 1);
        assert_eq!(v["windows"][0]["resetInMs"], 1);
    }

    #[test]
    fn ignores_other_lines_and_bad_values() {
        assert!(read(r#"{"type":"extension_ui_request","id":"u","method":"setStatus","statusKey":"goal","statusText":"oven-limits"}"#).is_none());
        assert!(read(&line("not json")).is_none());
        assert!(read(&line(r#"{"provider":"../x","plan":null,"at":1,"windows":[{"minutes":300,"used":1,"resetAt":null}]}"#)).is_none());
        assert!(read(&line(r#"{"provider":"x","plan":null,"at":1,"windows":[]}"#)).is_none());
        let e = read(&line(r#"{"provider":"x","plan":"<b>","at":1,"windows":[{"minutes":300,"used":250,"resetAt":null}]}"#)).unwrap();
        assert_eq!(e.plan, None);
        assert_eq!(e.windows[0].used, 100.0);
    }

    #[test]
    fn keeps_the_newest_and_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("pi-oven-limits-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ext = dir.join("oven-limits.ts");
        let e = |at: u64, used: f64| Entry { provider: "p".into(), plan: None, at, limit_reached: false, windows: vec![Window { minutes: 300, used, reset_at: None }] };
        let l = Limits::load(Some(&ext));
        l.record(e(2, 20.0));
        l.record(e(1, 10.0));
        assert_eq!(l.view(3)["items"][0]["windows"][0]["used"], 20.0);
        assert_eq!(Limits::load(Some(&ext)).view(3)["items"][0]["windows"][0]["used"], 20.0);
        std::fs::remove_file(dir.join("limits.json")).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }
}
