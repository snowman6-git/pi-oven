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
    /// "Rate limit reset credits" the plan has left (ChatGPT's usage answer only).
    #[serde(default)]
    pub resets: Option<u32>,
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
            "resets": self.resets,
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
            let old = map.get(&e.provider);
            if old.is_some_and(|old| old.at > e.at) {
                return;
            }
            // replies do not carry the reset credits; keep what the usage answer said
            let mut e = e;
            if e.resets.is_none() {
                e.resets = old.and_then(|o| o.resets);
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

    /// After a usage-limit error: the window that ran out, when it resets, and whether that is known rather than
    /// guessed. The stored numbers can be hours old, so "most used" is not enough (a week at 49% beat five hours at 26%
    /// that had since run out):
    /// 1. the error names a time ("Try again in ~182 min"): the window whose reset is closest to it (within 30 min);
    /// 2. a window at 90% or more: the shortest such;
    /// 3. else the shortest window: a fresh limit is far more often the 5 hours than the week.
    pub fn culprit(&self, provider: Option<&str>, now: u64, hint: Option<u64>) -> Option<(u64, u32, bool)> {
        let map = self.map.lock().unwrap();
        let entries: Vec<&Entry> = match provider {
            Some(p) => map.get(p).into_iter().collect(),
            None => map.values().collect(),
        };
        let ws: Vec<(f64, u64, u32)> = entries
            .iter()
            .flat_map(|e| e.windows.iter())
            .filter_map(|w| w.reset_at.filter(|&t| t > now).map(|t| (w.used, t, w.minutes)))
            .collect();
        if let Some(h) = hint {
            if let Some(w) = ws.iter().filter(|w| w.1.abs_diff(h) <= 30 * 60_000).min_by_key(|w| w.1.abs_diff(h)) {
                return Some((w.1, w.2, true));
            }
        }
        if let Some(w) = ws.iter().filter(|w| w.0 >= 90.0).min_by_key(|w| w.2) {
            return Some((w.1, w.2, true));
        }
        ws.iter().min_by_key(|w| w.2).map(|w| (w.1, w.2, false))
    }

    /// The provider said the limit is reached: that window is used up, whatever the last reply's numbers were.
    pub fn mark_used_up(&self, provider: &str, minutes: u32) {
        let changed = {
            let mut map = self.map.lock().unwrap();
            let Some(e) = map.get_mut(provider) else { return };
            e.limit_reached = true;
            let mut hit = false;
            for w in e.windows.iter_mut().filter(|w| w.minutes == minutes) {
                w.used = 100.0;
                hit = true;
            }
            hit
        };
        if changed {
            self.save();
        }
    }

    fn save(&self) {
        let all: Vec<Entry> = self.map.lock().unwrap().values().cloned().collect();
        let Some(file) = &self.file else { return };
        let tmp = file.with_extension("tmp");
        if serde_json::to_vec(&all).is_ok_and(|b| std::fs::write(&tmp, b).is_ok()) {
            let _ = std::fs::rename(&tmp, file);
        }
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
    fn the_most_used_window_is_the_one_that_ran_out() {
        let l = Limits::default();
        let w = |minutes, used, reset_at| Window { minutes, used, reset_at: Some(reset_at) };
        l.record(Entry { provider: "p".into(), plan: None, at: 1, limit_reached: false, windows: vec![w(300, 98.0, 5000), w(10080, 15.0, 9000)], resets: None });
        assert_eq!(l.culprit(Some("p"), 100, None), Some((5000, 300, true)));
        // once the 5 hours have reset, the week is what is left to wait for
        assert_eq!(l.culprit(Some("p"), 6000, None), Some((9000, 10080, false)));
        assert_eq!(l.culprit(Some("other"), 100, None), None);
        // what really happened: old numbers (5 h at 26%, week at 49%), and the error says ~182 min, which is the 5 h reset
        let m = 60_000;
        let l2 = Limits::default();
        l2.record(Entry { provider: "p".into(), plan: None, at: 1, limit_reached: false, windows: vec![w(300, 26.0, 200 * m), w(10080, 49.0, 9000 * m)], resets: None });
        assert_eq!(l2.culprit(Some("p"), 0, Some(182 * m)), Some((200 * m, 300, true)));
        // and without a time the 5 hours are still the better guess than the week
        assert_eq!(l2.culprit(Some("p"), 0, None), Some((200 * m, 300, false)));
        // a week that really is nearly used up wins
        l2.record(Entry { provider: "p".into(), plan: None, at: 2, limit_reached: false, windows: vec![w(300, 26.0, 200 * m), w(10080, 99.0, 9000 * m)], resets: None });
        assert_eq!(l2.culprit(Some("p"), 0, None), Some((9000 * m, 10080, true)));
        l.mark_used_up("p", 300);
        assert_eq!(l.view(100)["items"][0]["windows"][0]["used"], 100.0);
    }

    #[test]
    fn keeps_the_newest_and_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("pi-oven-limits-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ext = dir.join("oven-limits.ts");
        let e = |at: u64, used: f64| Entry { provider: "p".into(), plan: None, at, limit_reached: false, windows: vec![Window { minutes: 300, used, reset_at: None }], resets: None };
        let l = Limits::load(Some(&ext));
        l.record(e(2, 20.0));
        l.record(e(1, 10.0));
        assert_eq!(l.view(3)["items"][0]["windows"][0]["used"], 20.0);
        assert_eq!(Limits::load(Some(&ext)).view(3)["items"][0]["windows"][0]["used"], 20.0);
        std::fs::remove_file(dir.join("limits.json")).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }
}
