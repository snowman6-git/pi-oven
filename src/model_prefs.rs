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
//! The level for a model, where one is set, is used when a new session starts on it and when the page switches to it.
//! These live in pi-oven's own file, not in pi's settings.json (that file is the person's, and pi rewrites it).

use axum::{extract::State, http::StatusCode, response::{IntoResponse, Response}, Json};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::{api::fail, AppState};

const FILE: &str = "model-prefs.json";
const LEVELS: [&str; 7] = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    /// `[provider, model]` last picked on a page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<(String, String)>,
    /// `provider/model` → level.
    #[serde(default)]
    pub thinking: BTreeMap<String, String>,
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
        let dir = crate::config::cache_dir();
        let tmp = dir.join(format!("{FILE}.tmp"));
        let ok = std::fs::create_dir_all(&dir).is_ok()
            && serde_json::to_vec_pretty(p).is_ok_and(|b| std::fs::write(&tmp, b).is_ok())
            && std::fs::rename(&tmp, dir.join(FILE)).is_ok();
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

    /// The model a new session is put on right after pi starts, and the level to give it once pi has taken it.
    pub fn for_new_session(&self) -> Option<(String, String, Option<String>)> {
        let p = self.inner.lock().unwrap();
        let (provider, model) = p.last.clone()?;
        let level = p.thinking.get(&key(&provider, &model)).cloned();
        Some((provider, model, level))
    }

    fn view(&self) -> Prefs {
        self.inner.lock().unwrap().clone()
    }

    fn set_level(&self, provider: &str, model: &str, level: Option<&str>) -> bool {
        if level.is_some_and(|l| !LEVELS.contains(&l)) || provider.is_empty() || model.is_empty() {
            return false;
        }
        let mut p = self.inner.lock().unwrap();
        match level {
            Some(l) => p.thinking.insert(key(provider, model), l.to_owned()),
            None => p.thinking.remove(&key(provider, model)),
        };
        Self::save(&p);
        true
    }
}

pub async fn get(State(app): State<Arc<AppState>>) -> Json<Prefs> {
    Json(app.runtimes.prefs.view())
}

#[derive(Deserialize)]
pub struct SetLevel {
    provider: String,
    model: String,
    /// None: the model starts with pi's own level.
    level: Option<String>,
}

pub async fn set_level(State(app): State<Arc<AppState>>, Json(req): Json<SetLevel>) -> Response {
    if !app.runtimes.prefs.set_level(&req.provider, &req.model, req.level.as_deref()) {
        return fail(StatusCode::BAD_REQUEST, "unknown level");
    }
    Json(app.runtimes.prefs.view()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_session_gets_the_last_model_and_its_level() {
        let m = ModelPrefs { inner: Mutex::new(Prefs::default()) };
        assert!(m.for_new_session().is_none());
        m.inner.lock().unwrap().last = Some(("anthropic".into(), "claude-opus-5-5".into()));
        assert_eq!(m.for_new_session(), Some(("anthropic".into(), "claude-opus-5-5".into(), None)));
        m.inner.lock().unwrap().thinking.insert("anthropic/claude-opus-5-5".into(), "high".into());
        assert_eq!(m.for_new_session().unwrap().2.as_deref(), Some("high"));
    }

    #[test]
    fn old_file_shapes_still_read() {
        let p: Prefs = serde_json::from_str(r#"{"thinking":{}}"#).unwrap();
        assert!(p.last.is_none());
        let p: Prefs = serde_json::from_str(r#"{"last":["a","b"],"thinking":{"a/b":"low"}}"#).unwrap();
        assert_eq!(p.last, Some(("a".into(), "b".into())));
    }
}
