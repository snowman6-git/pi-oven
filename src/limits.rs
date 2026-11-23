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
    /// What those credits reset, one per credit ("5h", "week", "model:<name>", "other"), when the provider lists them.
    #[serde(default)]
    pub reset_kinds: Vec<String>,
}

impl Entry {
    /// When every window used `at_least` % or more has reset again (none is: `None`).
    pub fn used_up_until(&self, now: u64, at_least: f64) -> Option<u64> {
        self.windows.iter().filter(|w| w.used >= at_least).filter_map(|w| w.reset_at.filter(|t| *t > now)).max()
    }

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
        self.reset_kinds.retain(|k| k.len() <= 80 && !k.contains(['<', '>', '\n']));
        self.reset_kinds.truncate(20);
        (!self.windows.is_empty()).then_some(self)
    }

    /// For the page: times as "how long from now", since a phone's clock need not agree with the server's.
    pub fn view(&self, now: u64) -> serde_json::Value {
        serde_json::json!({
            "provider": self.provider,
            "plan": self.plan,
            "ageMs": now.saturating_sub(self.at),
            "limitReached": self.limit_reached,
            "resets": self.resets,
            "resetKinds": self.reset_kinds,
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
    /// Providers whose last direct question failed: why, and when the next may go out. Shown, so a chip that does not
    /// follow a model switch has a reason on screen instead of looking stuck.
    problems: Mutex<BTreeMap<String, (String, u64)>>,
    /// When each provider last went to another account. Numbers that left before it belong to the account before:
    /// a reply sent on the old sign-in, or a question asked with its token, can land after the switch.
    switched: Mutex<BTreeMap<String, u64>>,
    /// The provider refused a request for the plan's limit (a 429 that is the plan), with no numbers to say which
    /// window: provider → when. Numbers that come after it decide again (`record`); without any it lapses after
    /// `REACHED_FOR_MS`.
    reached: Mutex<BTreeMap<String, u64>>,
}

/// How long a refusal counts as "used up" when no numbers came after it.
pub const REACHED_FOR_MS: u64 = 30 * 60_000;

/// A reply's numbers come with its first bytes, which can come this long after the request went out on the sign-in
/// of the moment.
pub const REPLY_LAG_MS: u64 = 30_000;

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
        Self { map: Mutex::new(map), file, problems: Default::default(), switched: Default::default(), reached: Default::default() }
    }

    /// The numbers held for `provider` (those of the account in use).
    pub fn entry(&self, provider: &str) -> Option<Entry> {
        self.map.lock().unwrap().get(provider).cloned()
    }

    /// Put `e` in, older or not: another account just took over, and these are its numbers.
    pub fn replace(&self, e: Entry) {
        self.switched.lock().unwrap().insert(e.provider.clone(), crate::runtime::now_ms());
        let windows: Vec<_> = e.windows.iter().map(|w| serde_json::json!({ "minutes": w.minutes, "used": w.used })).collect();
        crate::logs::event("limits", serde_json::json!({ "provider": e.provider, "plan": e.plan, "windows": windows, "reached": e.limit_reached, "account": "switched" }));
        self.map.lock().unwrap().insert(e.provider.clone(), e);
        self.save();
    }

    /// Numbers a reply brought: kept unless the account changed since its request may have gone out.
    pub fn record_reply(&self, e: Entry) {
        if self.is_before_switch(&e, e.at.saturating_sub(REPLY_LAG_MS)) {
            return;
        }
        self.record_from(e, "reply");
    }

    /// Numbers asked for at `asked`: kept unless the account changed since.
    pub fn record_since(&self, asked: u64, e: Entry) {
        if self.is_before_switch(&e, asked) {
            return;
        }
        self.record_from(e, "poll");
    }

    fn is_before_switch(&self, e: &Entry, asked: u64) -> bool {
        let old = self.switched.lock().unwrap().get(&e.provider).is_some_and(|t| *t > asked);
        if old {
            tracing::info!("usage: numbers for {} from before the account changed; left out", e.provider);
        }
        old
    }

    /// The provider refused a request for the plan's limit and no fresh numbers say which window: the plan counts as
    /// used up until numbers come (or `REACHED_FOR_MS` passes).
    pub fn mark_reached(&self, provider: &str, now: u64) {
        self.reached.lock().unwrap().insert(provider.to_owned(), now);
        crate::logs::event("limits", serde_json::json!({ "provider": provider, "reached": true, "source": "refused" }));
    }

    /// Is `provider`'s plan used up now, as far as anything says? `Some((reset, window))`: a window at 100 % that has
    /// not reset (its reset time and length), or a refusal no numbers have answered yet (`(None, None)`).
    pub fn blocked(&self, provider: &str, now: u64) -> Option<(Option<u64>, Option<u32>)> {
        let map = self.map.lock().unwrap();
        if let Some(e) = map.get(provider) {
            let out = e.windows.iter().filter(|w| w.used >= 100.0).filter_map(|w| w.reset_at.filter(|t| *t > now).map(|t| (t, w.minutes))).max();
            if let Some((t, m)) = out {
                return Some((Some(t), Some(m)));
            }
        }
        let at = *self.reached.lock().unwrap().get(provider)?;
        (now.saturating_sub(at) < REACHED_FOR_MS && map.get(provider).is_none_or(|e| e.at < at)).then_some((None, None))
    }

    /// The soonest reset after `after`, of any window held.
    pub fn next_reset(&self, after: u64) -> Option<u64> {
        self.map.lock().unwrap().values().flat_map(|e| e.windows.iter()).filter_map(|w| w.reset_at).filter(|t| *t > after).min()
    }

    pub fn record(&self, e: Entry) {
        self.record_from(e, "poll");
    }

    /// `source`: where the numbers came from, for the log (`reply`: a response's headers or events; `poll`: asked).
    fn record_from(&self, e: Entry, source: &str) {
        // numbers newer than a refusal say what it was
        {
            let mut r = self.reached.lock().unwrap();
            if r.get(&e.provider).is_some_and(|t| *t <= e.at) {
                r.remove(&e.provider);
            }
        }
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
                e.reset_kinds = old.map(|o| o.reset_kinds.clone()).unwrap_or_default();
            }
            // for the log: a jump in the numbers, to put next to the replies before it
            let used = |x: &Entry| x.windows.iter().map(|w| (w.minutes, (w.used * 10.0).round() as i64)).collect::<Vec<_>>();
            if old.is_none_or(|o| used(o) != used(&e)) {
                let windows: Vec<_> = e.windows.iter().map(|w| serde_json::json!({ "minutes": w.minutes, "used": w.used, "was": old.and_then(|o| o.windows.iter().find(|x| x.minutes == w.minutes)).map(|x| x.used) })).collect();
                crate::logs::event("limits", serde_json::json!({ "provider": e.provider, "plan": e.plan, "windows": windows, "reached": e.limit_reached, "source": source }));
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

    /// Does `provider` have a window at `at_least` % used that has not reset yet?
    pub fn used_up(&self, provider: &str, now: u64, at_least: f64) -> bool {
        let map = self.map.lock().unwrap();
        map.get(provider).is_some_and(|e| e.windows.iter().any(|w| w.used >= at_least && w.reset_at.is_some_and(|t| t > now)))
    }

    pub fn used_up_until(&self, provider: &str, now: u64, at_least: f64) -> Option<u64> {
        self.map.lock().unwrap().get(provider).and_then(|e| e.used_up_until(now, at_least))
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

    /// `Some`: asking `provider` failed (`why`), next try at `retry_at`. `None`: it answered.
    pub fn note_problem(&self, provider: &str, problem: Option<(String, u64)>) {
        let mut p = self.problems.lock().unwrap();
        match problem {
            Some(x) => p.insert(provider.to_owned(), x),
            None => p.remove(provider),
        };
    }

    pub fn view(&self, now: u64) -> serde_json::Value {
        let map = self.map.lock().unwrap();
        let problems = self.problems.lock().unwrap();
        serde_json::json!({
            "items": map.values().map(|e| e.view(now)).collect::<Vec<_>>(),
            "problems": problems.iter().map(|(k, (why, at))| serde_json::json!({ "provider": k, "why": why, "retryInMs": at.saturating_sub(now) })).collect::<Vec<_>>(),
        })
    }
}

pub async fn get(State(app): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(app.runtimes.accounts.limits_view(&app.runtimes.limits, now_ms()))
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
        l.record(Entry { provider: "p".into(), plan: None, at: 1, limit_reached: false, windows: vec![w(300, 98.0, 5000), w(10080, 15.0, 9000)], resets: None, reset_kinds: vec![] });
        assert_eq!(l.culprit(Some("p"), 100, None), Some((5000, 300, true)));
        // once the 5 hours have reset, the week is what is left to wait for
        assert_eq!(l.culprit(Some("p"), 6000, None), Some((9000, 10080, false)));
        assert_eq!(l.culprit(Some("other"), 100, None), None);
        // what really happened: old numbers (5 h at 26%, week at 49%), and the error says ~182 min, which is the 5 h reset
        let m = 60_000;
        let l2 = Limits::default();
        l2.record(Entry { provider: "p".into(), plan: None, at: 1, limit_reached: false, windows: vec![w(300, 26.0, 200 * m), w(10080, 49.0, 9000 * m)], resets: None, reset_kinds: vec![] });
        assert_eq!(l2.culprit(Some("p"), 0, Some(182 * m)), Some((200 * m, 300, true)));
        // and without a time the 5 hours are still the better guess than the week
        assert_eq!(l2.culprit(Some("p"), 0, None), Some((200 * m, 300, false)));
        // a week that really is nearly used up wins
        l2.record(Entry { provider: "p".into(), plan: None, at: 2, limit_reached: false, windows: vec![w(300, 26.0, 200 * m), w(10080, 99.0, 9000 * m)], resets: None, reset_kinds: vec![] });
        assert_eq!(l2.culprit(Some("p"), 0, None), Some((9000 * m, 10080, true)));
        l.mark_used_up("p", 300);
        assert_eq!(l.view(100)["items"][0]["windows"][0]["used"], 100.0);
    }

    #[test]
    fn numbers_from_before_an_account_change_are_left_out() {
        let limits = Limits::load(None);
        let now = crate::runtime::now_ms();
        let e = |used: f64, at: u64| Entry { provider: "anthropic".into(), plan: None, at, limit_reached: false, windows: vec![Window { minutes: 300, used, reset_at: Some(now + 3_600_000) }], resets: None, reset_kinds: vec![] };
        limits.replace(e(10.0, now)); // the new account's numbers, at the switch
        // a reply that went out on the old sign-in lands 5 s later with its 100 %
        limits.record_reply(e(100.0, now + 5_000));
        assert_eq!(limits.entry("anthropic").unwrap().windows[0].used, 10.0);
        // a question asked before the switch
        limits.record_since(now - 1, e(100.0, now + 6_000));
        assert_eq!(limits.entry("anthropic").unwrap().windows[0].used, 10.0);
        // later replies are the new account's
        limits.record_reply(e(20.0, now + REPLY_LAG_MS + 1_000));
        assert_eq!(limits.entry("anthropic").unwrap().windows[0].used, 20.0);
        assert_eq!(limits.next_reset(now), Some(now + 3_600_000));
        assert_eq!(limits.next_reset(now + 3_600_000), None);
    }

    #[test]
    fn keeps_the_newest_and_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("pi-oven-limits-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ext = dir.join("oven-limits.ts");
        let e = |at: u64, used: f64| Entry { provider: "p".into(), plan: None, at, limit_reached: false, windows: vec![Window { minutes: 300, used, reset_at: None }], resets: None, reset_kinds: vec![] };
        let l = Limits::load(Some(&ext));
        l.record(e(2, 20.0));
        l.record(e(1, 10.0));
        assert_eq!(l.view(3)["items"][0]["windows"][0]["used"], 20.0);
        assert_eq!(Limits::load(Some(&ext)).view(3)["items"][0]["windows"][0]["used"], 20.0);
        std::fs::remove_file(dir.join("limits.json")).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn a_plan_is_held_back_while_used_up_or_refused_until_numbers_come() {
        let l = Limits::load(None);
        let w = |minutes, used: f64, reset_at| Window { minutes, used, reset_at: Some(reset_at) };
        let e = |at, used| Entry { provider: "anthropic".into(), plan: None, at, limit_reached: false, windows: vec![w(300, 10.0, 50_000), w(10080, used, 900_000)], resets: None, reset_kinds: vec![] };
        l.record(e(1_000, 100.0));
        assert_eq!(l.blocked("anthropic", 2_000), Some((Some(900_000), Some(10080))));
        assert_eq!(l.blocked("anthropic", 900_001), None, "reset since");
        l.record(e(3_000, 40.0));
        assert_eq!(l.blocked("anthropic", 4_000), None);
        // a refusal with no numbers after it
        l.mark_reached("anthropic", 5_000);
        assert_eq!(l.blocked("anthropic", 6_000), Some((None, None)));
        assert_eq!(l.blocked("anthropic", 5_000 + REACHED_FOR_MS), None, "lapses");
        // numbers after it decide
        l.record(e(7_000, 40.0));
        assert_eq!(l.blocked("anthropic", 8_000), None);
        assert_eq!(l.blocked("openai-codex", 8_000), None);
    }
}
