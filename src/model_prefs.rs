//! Which model a new session starts on, and the reasoning level each model starts with (Settings → Models).
//!
//! A new session starts on the model last picked in pi-oven, on any device (pi's own default otherwise, which only
//! pi's `/model` changes). A session that already exists is left on the model it was using: only a new one is told.
//! The model is set after pi starts (`set_model`) rather than passed as `--model`: pi refuses to start at all on a
//! model it does not know (one removed since), while a failed `set_model` leaves the session on pi's default.
//!
//! Both wait for pi's answer to `set_model`: a model pi could not switch to (no key for it, say) is not remembered as
//! picked, and its level is not given to the model the session stayed on.
//!
//! The level for a model is used when a new session starts on it and when the page switches to it. Unset, a model starts
//! on its provider's recommended level where the provider gives one (ChatGPT does), else on pi's; `pi` means pi's
//! even where the provider recommends one.
//!
//! Models can also be left out of the page's model menu (`hidden`), or whole providers (`hiddenProviders`); the model
//! in use always shows.
//!
//! These live in pi-oven's own file, not in pi's settings.json (that file is the person's, and pi rewrites it).

use axum::{extract::State, http::StatusCode, response::{IntoResponse, Response}, Json};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use crate::{api::fail, AppState};

const FILE: &str = "model-prefs.json";
const LEVELS: [&str; 7] = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];
/// The stored choice "pi's own level" (no level is given, not even the provider's recommended one).
const PI: &str = "pi";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    /// `[provider, model]` last picked on a page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<(String, String)>,
    /// `provider/model` → level, or `pi`.
    #[serde(default)]
    pub thinking: BTreeMap<String, String>,
    /// `provider/model` left out of the model menu.
    #[serde(default)]
    pub hidden: BTreeSet<String>,
    /// Providers left out of the model menu, all their models at once (each model's own switch is kept for when the
    /// provider is switched on again).
    #[serde(default, rename = "hiddenProviders")]
    pub hidden_providers: BTreeSet<String>,
}

pub struct ModelPrefs {
    inner: Mutex<Prefs>,
}

impl Default for ModelPrefs {
    fn default() -> Self {
        let p = std::fs::read_to_string(crate::config::cache_dir().join(FILE))
            .ok()
            .and_then(|s| serde_json::from_str::<Prefs>(&s).ok())
            .unwrap_or_default();
        Self { inner: Mutex::new(p) }
    }
}

fn key(provider: &str, model: &str) -> String {
    format!("{provider}/{model}")
}

impl ModelPrefs {
    fn save(p: &Prefs) {
        if cfg!(test) {
            return; // tests must not write the person's file
        }
        let path = crate::config::cache_dir().join(FILE);
        let ok = serde_json::to_vec_pretty(p).is_ok_and(|b| crate::config::atomic_write(&path, &b, false).is_ok());
        if !ok {
            tracing::warn!("could not save {FILE}");
        }
    }

    /// The person picked this model on a page.
    pub fn chosen(&self, provider: &str, model: &str) {
        let mut p = self.inner.lock().unwrap();
        let next = Some((provider.to_owned(), model.to_owned()));
        if p.last != next {
            p.last = next;
            Self::save(&p);
        }
    }

    /// The model a new session is put on right after pi starts, and the level to give it once pi has taken it:
    /// the one set for it, else its provider's recommended one (`recommended`, `provider` → model → level).
    pub fn for_new_session(&self, recommended: &BTreeMap<String, BTreeMap<String, String>>) -> Option<(String, String, Option<String>)> {
        let p = self.inner.lock().unwrap();
        let (provider, model) = p.last.clone()?;
        let level = match p.thinking.get(&key(&provider, &model)) {
            Some(l) if l == PI => None,
            Some(l) => Some(l.clone()),
            None => recommended.get(&provider).and_then(|m| m.get(&model)).cloned(),
        };
        Some((provider, model, level))
    }

    fn view(&self) -> Prefs {
        self.inner.lock().unwrap().clone()
    }

    /// Apply one change to the provider and to each of `models` (all at once, saved once).
    fn apply(&self, provider: &str, models: &[String], level: Option<Option<&str>>, shown: Option<bool>, provider_shown: Option<bool>) -> bool {
        if provider.is_empty() || models.iter().any(String::is_empty) {
            return false;
        }
        if level.flatten().is_some_and(|l| l != PI && !LEVELS.contains(&l)) {
            return false;
        }
        let mut p = self.inner.lock().unwrap();
        for model in models {
            let k = key(provider, model);
            match level {
                Some(Some(l)) => { p.thinking.insert(k.clone(), l.to_owned()); }
                Some(None) => { p.thinking.remove(&k); }
                None => {}
            }
            match shown {
                Some(true) => { p.hidden.remove(&k); }
                Some(false) => { p.hidden.insert(k); }
                None => {}
            }
        }
        match provider_shown {
            Some(true) => { p.hidden_providers.remove(provider); }
            Some(false) => { p.hidden_providers.insert(provider.to_owned()); }
            None => {}
        }
        Self::save(&p);
        true
    }
}

pub async fn get(State(app): State<Arc<AppState>>) -> Json<Prefs> {
    Json(app.runtimes.prefs.view())
}

/// A change to models of one provider (`model`, and/or several in `models`): their level (`null` clears it), whether
/// the menu shows them, and/or whether the menu shows the provider at all.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    provider: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    models: Vec<String>,
    /// Absent: unchanged. `null`: its provider's recommended level, else pi's. `pi`: pi's.
    #[serde(default, deserialize_with = "present")]
    level: Option<Option<String>>,
    shown: Option<bool>,
    provider_shown: Option<bool>,
}

/// A field that is there, even as `null`, is `Some` (serde makes absent and `null` both `None` otherwise).
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}

pub async fn update(State(app): State<Arc<AppState>>, Json(req): Json<Update>) -> Response {
    let prefs = &app.runtimes.prefs;
    let models: Vec<String> = req.model.into_iter().chain(req.models).collect();
    if models.len() > 2000 || !prefs.apply(&req.provider, &models, req.level.as_ref().map(|l| l.as_deref()), req.shown, req.provider_shown) {
        return fail(StatusCode::BAD_REQUEST, "unknown level or model");
    }
    Json(prefs.view()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_session_gets_the_last_model_and_its_level() {
        let none = BTreeMap::new();
        let m = ModelPrefs { inner: Mutex::new(Prefs::default()) };
        assert!(m.for_new_session(&none).is_none());
        m.inner.lock().unwrap().last = Some(("openai-codex".into(), "gpt-5".into()));
        assert_eq!(m.for_new_session(&none), Some(("openai-codex".into(), "gpt-5".into(), None)));
        // unset: the provider's recommended level
        let rec = BTreeMap::from([("openai-codex".to_owned(), BTreeMap::from([("gpt-5".to_owned(), "medium".to_owned())]))]);
        assert_eq!(m.for_new_session(&rec).unwrap().2.as_deref(), Some("medium"));
        // "pi": not even that
        m.inner.lock().unwrap().thinking.insert("openai-codex/gpt-5".into(), "pi".into());
        assert_eq!(m.for_new_session(&rec).unwrap().2, None);
        m.inner.lock().unwrap().thinking.insert("openai-codex/gpt-5".into(), "high".into());
        assert_eq!(m.for_new_session(&rec).unwrap().2.as_deref(), Some("high"));
    }

    #[test]
    fn an_update_tells_a_cleared_level_from_an_untouched_one() {
        let u: Update = serde_json::from_str(r#"{"provider":"a","model":"b","level":null}"#).unwrap();
        assert_eq!(u.level, Some(None));
        let u: Update = serde_json::from_str(r#"{"provider":"a","model":"b","shown":false}"#).unwrap();
        assert_eq!(u.level, None);
        assert_eq!(u.shown, Some(false));
        let u: Update = serde_json::from_str(r#"{"provider":"a","providerShown":false}"#).unwrap();
        assert_eq!(u.provider_shown, Some(false));
    }

    #[test]
    fn a_provider_switch_keeps_its_models_own() {
        let m = ModelPrefs { inner: Mutex::new(Prefs::default()) };
        let ms = ["x".to_owned(), "y".to_owned()];
        assert!(m.apply("p", &ms[..1], None, Some(false), None));
        assert!(m.apply("p", &[], None, None, Some(false)));
        assert!(m.apply("p", &ms, Some(Some("high")), None, None));
        let v = m.view();
        assert!(v.hidden_providers.contains("p") && v.hidden.contains("p/x") && !v.hidden.contains("p/y"));
        assert_eq!(v.thinking.get("p/y").map(String::as_str), Some("high"));
        assert!(!m.apply("p", &ms, Some(Some("huge")), None, None));
    }

    #[test]
    fn old_file_shapes_still_read() {
        let p: Prefs = serde_json::from_str(r#"{"thinking":{}}"#).unwrap();
        assert!(p.last.is_none());
        let p: Prefs = serde_json::from_str(r#"{"last":["a","b"],"thinking":{"a/b":"low"}}"#).unwrap();
        assert_eq!(p.last, Some(("a".into(), "b".into())));
    }
}
