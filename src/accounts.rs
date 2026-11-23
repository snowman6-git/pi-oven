//! Several subscription accounts for one provider (two Claude plans, say), and switching between them.
//!
//! pi keeps one sign-in per provider in `auth.json`. pi-oven keeps the others next to it, in
//! `pi-oven-accounts.json` (same folder, owner-only), and switches by writing the chosen account's sign-in into
//! `auth.json` under pi's own lock. pi reads the file again whenever it changes, so every session uses the new
//! account from its next request.
//!
//! pi refreshes tokens and writes them back, which changes the refresh token: before anything is switched the sign-in
//! in `auth.json` is first copied back to its own account here (found by its refresh token, or else by asking who it
//! is), so a stored account never holds a token that no longer works.
//!
//! Adding an account runs `pi auth login <provider> --no-browser` in a temporary agent folder, inside a pseudo
//! terminal (`script`): its text goes to the page, the code the person pastes comes back. The account in use and the
//! running sessions are not touched; the new account joins the list.
//!
//! When Zero-Waste meets a limit it can switch to an account of the same provider that is not limited, and go on at
//! once instead of waiting ("Baton Touch", `baton_touch`). Tokens never leave the server: the page sees who, which plan, and when.

use axum::{extract::State, http::StatusCode, response::{IntoResponse, Response}, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{api::fail, runtime::now_ms, AppState};

pub const PROVIDERS: [&str; 2] = ["anthropic", "openai-codex"];
const PROFILE: &str = "https://api.anthropic.com/api/oauth/profile";
const LOGIN_MAX_MS: u64 = 10 * 60_000;
const LOGIN_LINES: usize = 40;

fn agent_dir() -> PathBuf {
    std::env::var_os("PI_CODING_AGENT_DIR").map(PathBuf::from).unwrap_or_else(|| crate::config::home_dir().join(".pi/agent"))
}
fn store_path() -> PathBuf {
    agent_dir().join("pi-oven-accounts.json")
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// `<provider>:<the provider's own id of the person>`
    pub id: String,
    pub provider: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub plan: Option<String>,
    pub saved_at: u64,
    /// Ran into a limit that resets then (ms); a swap passes it over until then.
    #[serde(default)]
    pub limited_until: Option<u64>,
    /// The sign-in as pi stores it. Never sent to the page.
    cred: Value,
}

/// Another provider Baton Touch may hand over to, and the model to use there.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Target {
    pub provider: String,
    pub model: String,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Store {
    #[serde(default)]
    baton_touch: Option<bool>,
    /// When no account of the same provider is left: hand over to another provider (off unless turned on).
    #[serde(default)]
    cross_provider: Option<bool>,
    /// Which other providers, in order of preference.
    #[serde(default)]
    targets: Vec<Target>,
    /// After handing over to another provider: go back to the model it came from once that plan has reset (on unless
    /// turned off).
    #[serde(default)]
    return_home: Option<bool>,
    #[serde(default)]
    accounts: Vec<Account>,
}

/// Who a sign-in belongs to.
struct Who {
    key: String,
    email: Option<String>,
    name: Option<String>,
    plan: Option<String>,
}

// ── files ────────────────────────────────────────────────────────────────────

fn write_private(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    f.write_all(text.as_bytes()).and_then(|_| f.sync_all()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

fn read_store() -> Store {
    std::fs::read_to_string(store_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}
fn save_store(s: &Store) -> Result<(), String> {
    write_private(&store_path(), &serde_json::to_string_pretty(s).map_err(|e| e.to_string())?)
}

fn read_auth(dir: &Path) -> Value {
    std::fs::read_to_string(dir.join("auth.json")).ok().and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()).unwrap_or_else(|| json!({}))
}

/// Run `f` holding pi's own lock on `auth.json` (proper-lockfile: a `<file>.lock` folder; one older than 10 s is stale).
fn with_auth_lock<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let path = agent_dir().join("auth.json");
    let real = path.canonicalize().unwrap_or(path);
    let lock = PathBuf::from(format!("{}.lock", real.display()));
    let mut held = false;
    for _ in 0..80 {
        match std::fs::create_dir(&lock) {
            Ok(()) => {
                held = true;
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let stale = lock.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|d| d.as_secs() > 10);
                if stale {
                    let _ = std::fs::remove_dir(&lock);
                    continue;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    if !held {
        return Err("auth.json is locked by pi; try again".into());
    }
    let out = f();
    let _ = std::fs::remove_dir(&lock);
    out
}

/// Put `cred` in `auth.json` under `provider`, keeping everything else in it.
fn write_auth(provider: &str, cred: &Value) -> Result<(), String> {
    with_auth_lock(|| {
        let dir = agent_dir();
        let mut auth = read_auth(&dir);
        let obj = auth.as_object_mut().ok_or("auth.json is not an object")?;
        obj.insert(provider.to_owned(), cred.clone());
        let path = dir.join("auth.json");
        let real = path.canonicalize().unwrap_or(path);
        write_private(&real, &serde_json::to_string_pretty(&auth).map_err(|e| e.to_string())?)
    })
}

// ── who is it ────────────────────────────────────────────────────────────────

fn b64url(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            b'=' => break,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// ChatGPT: the access token is a JWT that says who and which plan.
fn who_openai(cred: &Value) -> Option<Who> {
    let payload = cred["access"].as_str()?.split('.').nth(1)?;
    let c: Value = serde_json::from_slice(&b64url(payload)?).ok()?;
    let auth = &c["https://api.openai.com/auth"];
    let key = auth["chatgpt_account_user_id"].as_str().or(auth["user_id"].as_str()).or(c["sub"].as_str())?.to_owned();
    let profile = &c["https://api.openai.com/profile"];
    Some(Who { key, email: profile["email"].as_str().map(str::to_owned), name: profile["name"].as_str().map(str::to_owned), plan: auth["chatgpt_plan_type"].as_str().map(str::to_owned) })
}

/// Claude: ask its profile (the token is fresh: pi just printed it).
async fn who_anthropic(token: &str) -> Option<Who> {
    let (code, body) = crate::usage::get(PROFILE, &format!("Authorization: Bearer {token}\nanthropic-beta: oauth-2025-04-20\n")).await.ok()?;
    if code != 200 {
        return None;
    }
    let v: Value = serde_json::from_str(&body).ok()?;
    let a = &v["account"];
    let plan = if a["has_claude_max"].as_bool() == Some(true) {
        Some("max".to_owned())
    } else if a["has_claude_pro"].as_bool() == Some(true) {
        Some("pro".to_owned())
    } else {
        v["organization"]["organization_type"].as_str().map(str::to_owned)
    };
    Some(Who { key: a["uuid"].as_str()?.to_owned(), email: a["email"].as_str().map(str::to_owned), name: a["display_name"].as_str().map(str::to_owned), plan })
}

/// A fresh token for `provider` from pi, in `dir` (another agent folder) or pi's own.
async fn token_in(pi_bin: &str, provider: &str, dir: Option<&Path>) -> Option<String> {
    let mut cmd = tokio::process::Command::new(pi_bin);
    cmd.args(["auth", "print-bearer-token", "--provider", provider]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    if let Some(d) = dir {
        cmd.env("PI_CODING_AGENT_DIR", d);
    }
    // pi may refresh the token over the network; the accounts lock is held meanwhile, so a hang must not hold it
    let out = tokio::time::timeout(std::time::Duration::from_secs(30), cmd.output()).await.ok()?.ok()?;
    let t = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !t.is_empty() && !t.contains(['\r', '\n'])).then_some(t)
}

/// Who signed in to `provider` in `dir`, and that sign-in as it stands after pi refreshed it.
async fn who_in(pi_bin: &str, provider: &str, dir: Option<&Path>) -> Option<(Who, Value)> {
    let folder = dir.map(Path::to_path_buf).unwrap_or_else(agent_dir);
    if provider == "openai-codex" {
        let cred = read_auth(&folder)[provider].clone();
        return who_openai(&cred).map(|w| (w, cred));
    }
    let token = token_in(pi_bin, provider, dir).await?;
    let who = who_anthropic(&token).await?;
    Some((who, read_auth(&folder)[provider].clone()))
}

fn same_sign_in(a: &Value, b: &Value) -> bool {
    let r = |v: &Value| v["refresh"].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
    r(a).is_some() && r(a) == r(b)
}

fn upsert(store: &mut Store, provider: &str, who: Who, cred: Value) -> String {
    let id = format!("{provider}:{}", who.key);
    match store.accounts.iter_mut().find(|a| a.id == id) {
        Some(a) => {
            a.cred = cred;
            a.email = who.email.or(a.email.take());
            a.name = who.name.or(a.name.take());
            a.plan = who.plan.or(a.plan.take());
        }
        None => store.accounts.push(Account { id: id.clone(), provider: provider.to_owned(), email: who.email, name: who.name, plan: who.plan, saved_at: now_ms(), limited_until: None, cred }),
    }
    id
}

// ── the accounts ─────────────────────────────────────────────────────────────

/// What Baton Touch did about a limit.
pub enum Baton {
    /// Switched to `next`; `was`: who the account in use was (its email), for the note.
    Swapped { next: Account, was: Option<String> },
    /// Every account of the provider is limited, and `next` resets first (`at`, ms), before the one in use: switch
    /// to it then.
    Later { next: Account, at: u64, was: Option<String> },
    /// Already switched for this provider a moment ago: the same failure, reported again (pi's every retry, then its
    /// giving up). Nothing more to do.
    Handled,
    /// Off, or no account left: Zero-Waste waits as usual.
    No,
}

/// Limit reports for a provider this soon after a switch belong to the failure that caused it.
const BATON_SETTLE_MS: u64 = 60_000;
/// A limit whose reset is not known at all: taken as an hour, so the account is not chosen again straight away.
const UNKNOWN_RESET_MS: u64 = 3_600_000;

/// Baton Touch switched `provider` a moment ago.
fn just_switched(batons: &std::collections::BTreeMap<String, u64>, provider: &str, now: u64) -> bool {
    batons.get(provider).is_some_and(|t| now.saturating_sub(*t) < BATON_SETTLE_MS)
}

/// Until when an account cannot take over: its numbers decide when they were asked lately (free, or used up until
/// a reset); otherwise its own mark (set when it ran out, perhaps a guess) or older numbers, whichever is later.
fn busy_until(mark: Option<u64>, numbers: Option<(&crate::limits::Entry, u64)>, now: u64) -> Option<u64> {
    match numbers {
        Some((e, at)) if now.saturating_sub(at) < 10 * 60_000 => e.used_up_until(now, 95.0),
        numbers => mark.filter(|t| *t > now).max(numbers.and_then(|(e, _)| e.used_up_until(now, 95.0))),
    }
}

/// Which account of `provider` takes over from `cur`.
#[derive(Debug, PartialEq)]
enum Pick {
    /// This one (index), free now.
    Now(usize),
    /// None is free: this one resets first (then), sooner than `cur`.
    Later(usize, u64),
    None,
}

/// `busy(a)`: until when account `a` cannot be used (its own mark, or its numbers), `None` when it can.
fn pick_next(accounts: &[Account], provider: &str, cur: &str, now: u64, busy: impl Fn(&Account) -> Option<u64>) -> Pick {
    let cur_until = accounts.iter().find(|a| a.id == cur).and_then(&busy).unwrap_or(u64::MAX);
    let mut best: Option<(usize, u64)> = None;
    for (i, a) in accounts.iter().enumerate().filter(|(_, a)| a.provider == provider && a.id != cur) {
        match busy(a).filter(|t| *t > now) {
            None => return Pick::Now(i),
            Some(t) if best.is_none_or(|b| t < b.1) => best = Some((i, t)),
            _ => {}
        }
    }
    match best {
        Some((i, t)) if t < cur_until => Pick::Later(i, t),
        _ => Pick::None,
    }
}

/// The first allowed provider other than `from` whose plan is not used up.
fn pick_target(targets: &[Target], from: &str, used_up: impl Fn(&str) -> bool) -> Option<Target> {
    targets.iter().find(|t| t.provider != from && !t.model.is_empty() && !used_up(&t.provider)).cloned()
}

#[derive(Default)]
pub struct Accounts {
    /// One change at a time (sync, switch, remove, a login landing).
    busy: tokio::sync::Mutex<()>,
    /// When Baton Touch last switched each provider.
    batons: std::sync::Mutex<std::collections::BTreeMap<String, u64>>,
    /// The account in use per provider, as last found (for the limits view, without asking anyone).
    current: std::sync::Mutex<std::collections::BTreeMap<String, String>>,
    /// The plan numbers of the accounts not in use: account id → (numbers, when they were asked). Kept in a file
    /// (`account-limits.json`), so a restart does not leave Baton Touch with guesses.
    others: std::sync::Mutex<std::collections::BTreeMap<String, (crate::limits::Entry, u64)>>,
    /// Accounts not in use whose numbers were last asked for (account id → when), so they are asked at most every 5 min.
    asked: std::sync::Mutex<std::collections::BTreeMap<String, u64>>,
    login: Arc<tokio::sync::Mutex<Option<Login>>>,
    /// Account changes, for every session's page: the account in use is one for all of them (`auth.json`).
    changes: std::sync::OnceLock<tokio::sync::broadcast::Sender<Arc<str>>>,
}

struct Login {
    provider: String,
    lines: Vec<String>,
    /// running | done | failed
    state: &'static str,
    error: Option<String>,
    account: Option<String>,
    stdin: Option<tokio::process::ChildStdin>,
    kill: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Accounts {
    /// Copy what `auth.json` holds back to its account (adding the account if it is new). Returns, per provider, the
    /// account in use.
    async fn sync(&self, pi_bin: &str, store: &mut Store) -> std::collections::BTreeMap<String, String> {
        let mut current = std::collections::BTreeMap::new();
        for p in PROVIDERS {
            let cred = read_auth(&agent_dir())[p].clone();
            if !cred.is_object() || cred["type"] != "oauth" {
                continue;
            }
            if let Some(a) = store.accounts.iter_mut().find(|a| a.provider == p && same_sign_in(&a.cred, &cred)) {
                a.cred = cred;
                current.insert(p.to_owned(), a.id.clone());
                continue;
            }
            // a sign-in this list has not seen (refreshed, or logged in with pi directly): find out whose it is
            if let Some((who, cred)) = who_in(pi_bin, p, None).await {
                current.insert(p.to_owned(), upsert(store, p, who, cred));
            }
        }
        *self.current.lock().unwrap() = current.clone();
        current
    }

    async fn view(&self, pi_bin: &str) -> Value {
        let _one = self.busy.lock().await;
        let mut store = read_store();
        let current = self.sync(pi_bin, &mut store).await;
        let _ = save_store(&store);
        let now = now_ms();
        let providers: Vec<Value> = PROVIDERS
            .iter()
            .map(|p| {
                let list: Vec<Value> = store
                    .accounts
                    .iter()
                    .filter(|a| a.provider == *p)
                    .map(|a| {
                        json!({
                            "id": a.id, "email": a.email, "name": a.name, "plan": a.plan, "savedAt": a.saved_at,
                            "limitedUntil": a.limited_until.filter(|t| *t > now),
                            "current": current.get(*p) == Some(&a.id),
                        })
                    })
                    .collect();
                json!({ "provider": p, "accounts": list })
            })
            .collect();
        let login = self.login.lock().await.as_ref().map(|l| json!({ "provider": l.provider, "state": l.state, "lines": l.lines, "error": l.error, "account": l.account }));
        json!({
            "batonTouch": store.baton_touch.unwrap_or(true),
            "crossProvider": store.cross_provider.unwrap_or(false),
            "targets": store.targets,
            "returnHome": store.return_home.unwrap_or(true),
            "providers": providers,
            "login": login,
        })
    }

    /// No account of `from` is left: the other provider to hand over to (allowed in Settings, its plan not used up),
    /// if any. Remembered as a hand-over of `from`, so the failure's other reports are taken as handled.
    pub fn cross_target(&self, from: &str, limits: &crate::limits::Limits) -> Option<Target> {
        let store = read_store();
        if !store.baton_touch.unwrap_or(true) || !store.cross_provider.unwrap_or(false) {
            return None;
        }
        let now = now_ms();
        let t = pick_target(&store.targets, from, |p| limits.used_up(p, now, 95.0))?;
        self.batons.lock().unwrap().insert(from.to_owned(), now);
        Some(t)
    }

    /// Go back to the model a hand-over to another provider came from, once its plan has reset.
    pub fn return_home(&self) -> bool {
        read_store().return_home.unwrap_or(true)
    }

    /// Account changes as frames for a page (`oven_account`), with the session they came from (`None`: Settings).
    pub fn changes(&self) -> tokio::sync::broadcast::Receiver<Arc<str>> {
        self.changes.get_or_init(|| tokio::sync::broadcast::channel(16).0).subscribe()
    }

    /// Tell every session that `provider` now uses `a` (`origin`: the session whose limit caused it, which says so itself).
    pub fn announce(&self, origin: Option<&str>, a: &Account) {
        let frame = json!({ "type": "oven_account", "origin": origin, "provider": a.provider, "email": a.email, "plan": a.plan });
        let _ = self.changes.get_or_init(|| tokio::sync::broadcast::channel(16).0).send(Arc::from(frame.to_string()));
    }

    /// Baton Touch switched some provider a moment ago (a limit report without a provider belongs to that failure).
    pub fn baton_just_now(&self) -> bool {
        let now = now_ms();
        self.batons.lock().unwrap().values().any(|t| now.saturating_sub(*t) < BATON_SETTLE_MS)
    }

    /// Make `id` the account pi uses.
    pub async fn switch(&self, pi_bin: &str, id: &str, limits: &crate::limits::Limits) -> Result<Account, String> {
        let _one = self.busy.lock().await;
        let mut store = read_store();
        let current = self.sync(pi_bin, &mut store).await;
        save_store(&store)?;
        let a = store.accounts.iter().find(|a| a.id == id).ok_or("no such account")?.clone();
        if current.get(&a.provider) != Some(&a.id) {
            write_auth(&a.provider, &a.cred)?;
            self.handed_over(&a.provider, current.get(&a.provider).map(String::as_str), &a.id, limits);
        }
        Ok(a)
    }

    /// `provider` went from account `from` to `to`: the numbers go with them (the card of the account left keeps its
    /// own, the one taking over shows what is known of it until it is asked).
    fn handed_over(&self, provider: &str, from: Option<&str>, to: &str, limits: &crate::limits::Limits) {
        let now = now_ms();
        let mut others = self.others.lock().unwrap();
        if let (Some(from), Some(e)) = (from, limits.entry(provider)) {
            others.insert(from.to_owned(), (e, now));
        }
        match others.remove(to) {
            Some((e, _)) => limits.replace(e),
            // nothing known of it yet: no numbers rather than the other account's (the next reply or question fills them)
            None => limits.replace(crate::limits::Entry { provider: provider.to_owned(), plan: None, at: 0, limit_reached: false, windows: vec![], resets: None, reset_kinds: vec![] }),
        }
        self.current.lock().unwrap().insert(provider.to_owned(), to.to_owned());
        drop(others);
        self.save_numbers();
    }

    async fn remove(&self, pi_bin: &str, id: &str) -> Result<(), String> {
        let _one = self.busy.lock().await;
        let mut store = read_store();
        let current = self.sync(pi_bin, &mut store).await;
        if current.values().any(|c| c == id) {
            return Err("the account in use cannot be removed; switch to another first".into());
        }
        store.accounts.retain(|a| a.id != id);
        save_store(&store)
    }

    /// Zero-Waste ran into `provider`'s limit (resetting `until`, when known): mark the account in use, and switch to
    /// another of the same provider that is not limited (`Swapped`). When every one is, the one that resets first,
    /// if that is sooner than the one in use (`Later`: the caller switches then).
    pub async fn baton_touch(&self, pi_bin: &str, provider: &str, until: Option<u64>, limits: &crate::limits::Limits) -> Baton {
        let _one = self.busy.lock().await;
        let now = now_ms();
        if just_switched(&self.batons.lock().unwrap(), provider, now) {
            return Baton::Handled;
        }
        let mut store = read_store();
        if !store.baton_touch.unwrap_or(true) || store.accounts.iter().filter(|a| a.provider == provider).count() < 2 {
            return Baton::No;
        }
        // which account is in use, then every other account's numbers as they are now (asked without the lock: a
        // refreshed token is saved under it)
        if self.current.lock().unwrap().get(provider).is_none() {
            self.sync(pi_bin, &mut store).await;
            let _ = save_store(&store);
        }
        drop(_one);
        self.refresh_others(pi_bin, provider, None).await;
        let _one = self.busy.lock().await;
        let mut store = read_store();
        let current = self.sync(pi_bin, &mut store).await;
        let Some(cur) = current.get(provider).cloned() else { return Baton::No };
        // when it resets: the error or the numbers; else the latest window close to used up; else an hour
        let until = until
            .filter(|t| *t > now)
            .or_else(|| limits.used_up_until(provider, now, 95.0))
            .or_else(|| limits.used_up_until(provider, now, 80.0))
            .unwrap_or(now + UNKNOWN_RESET_MS);
        if let Some(a) = store.accounts.iter_mut().find(|a| a.id == cur) {
            a.limited_until = Some(until);
        }
        let pick = {
            let others = self.others.lock().unwrap();
            // numbers asked a moment ago decide (free, or until when); without them, its own mark or older numbers
            let busy = |a: &Account| busy_until(a.limited_until, others.get(&a.id).map(|(e, at)| (e, *at)), now);
            // keep the marks true to the numbers, so Settings → Accounts shows when each one is free
            for a in store.accounts.iter_mut().filter(|a| a.provider == provider && a.id != cur) {
                if others.get(&a.id).is_some_and(|(_, at)| now.saturating_sub(*at) < 10 * 60_000) {
                    a.limited_until = busy(a);
                }
            }
            pick_next(&store.accounts, provider, &cur, now, busy)
        };
        let _ = save_store(&store);
        let was = store.accounts.iter().find(|a| a.id == cur).and_then(|a| a.email.clone().or(a.name.clone()));
        let next = match pick {
            Pick::Now(i) => store.accounts[i].clone(),
            Pick::Later(i, at) => {
                self.batons.lock().unwrap().insert(provider.to_owned(), now);
                return Baton::Later { next: store.accounts[i].clone(), at, was };
            }
            Pick::None => return Baton::No,
        };
        if let Err(e) = write_auth(provider, &next.cred) {
            tracing::warn!("Baton Touch: {e}");
            return Baton::No;
        }
        self.batons.lock().unwrap().insert(provider.to_owned(), now);
        self.handed_over(provider, Some(&cur), &next.id, limits);
        tracing::info!("Baton Touch: {provider} limit reached, switched to another account");
        Baton::Swapped { next, was }
    }

    /// The time has come for a `Later` hand-over: switch, unless someone already did.
    pub async fn take_over(&self, pi_bin: &str, id: &str, limits: &crate::limits::Limits) -> Option<Account> {
        let a = self.switch(pi_bin, id, limits).await.inspect_err(|e| tracing::warn!("Baton Touch: {e}")).ok()?;
        self.batons.lock().unwrap().insert(a.provider.clone(), now_ms());
        Some(a)
    }

    // ── the numbers of the accounts not in use ─────────────────────────────────

    fn numbers_file() -> PathBuf {
        crate::config::cache_dir().join("account-limits.json")
    }

    /// Read the numbers kept from before a restart.
    pub fn load_numbers(&self) {
        let Some(map) = std::fs::read(Self::numbers_file()).ok().and_then(|b| serde_json::from_slice::<std::collections::BTreeMap<String, (crate::limits::Entry, u64)>>(&b).ok()) else { return };
        let mut others = self.others.lock().unwrap();
        for (id, v) in map {
            others.entry(id).or_insert(v);
        }
    }

    fn save_numbers(&self) {
        if cfg!(test) {
            return; // tests leave the real cache folder alone
        }
        let Ok(body) = serde_json::to_string(&*self.others.lock().unwrap()) else { return };
        if let Err(e) = write_private(&Self::numbers_file(), &body) {
            tracing::warn!("could not keep the accounts' plan numbers: {e}");
        }
    }

    /// Put an account's fresh numbers in: kept unless that account went into use since they were asked for (then the
    /// numbers held for its provider are newer).
    fn keep_numbers(&self, id: &str, e: crate::limits::Entry, asked: u64) {
        if self.current.lock().unwrap().get(&e.provider).is_some_and(|c| c == id) {
            return;
        }
        self.others.lock().unwrap().insert(id.to_owned(), (e, asked));
        self.save_numbers();
    }

    /// Could Baton Touch hand `provider` over to another saved account now (it is on, and an account not in use is
    /// not known to be used up)? Then a message is not held back for the plan in use.
    pub fn other_free(&self, provider: &str, now: u64) -> bool {
        let store = read_store();
        if !store.baton_touch.unwrap_or(true) {
            return false;
        }
        let current = self.current.lock().unwrap().get(provider).cloned();
        let others = self.others.lock().unwrap();
        store.accounts.iter().filter(|a| a.provider == provider && current.as_ref() != Some(&a.id)).any(|a| {
            let marked = a.limited_until.is_some_and(|t| t > now);
            let used_up = others.get(&a.id).is_some_and(|(e, _)| e.used_up_until(now, 100.0).is_some());
            !marked && !used_up
        })
    }

    /// The soonest reset after `after` among the accounts not in use.
    pub fn next_reset(&self, after: u64) -> Option<u64> {
        self.others.lock().unwrap().values().flat_map(|(e, _)| e.windows.iter()).filter_map(|w| w.reset_at).filter(|t| *t > after).min()
    }

    /// Ask for the plan numbers of the saved accounts not in use, each at most every 5 minutes. A token close to
    /// expiry is refreshed by pi in a folder of its own, and the refreshed sign-in is kept.
    pub async fn poll_others(&self, pi_bin: &str) {
        const GAP: u64 = 5 * 60_000;
        // just started: find out which accounts are in use first
        if self.current.lock().unwrap().is_empty() && read_store().accounts.len() > 1 {
            let _one = self.busy.lock().await;
            let mut store = read_store();
            self.sync(pi_bin, &mut store).await;
            let _ = save_store(&store);
        }
        let now = now_ms();
        let current = self.current.lock().unwrap().clone();
        let due: Vec<Account> = read_store()
            .accounts
            .into_iter()
            .filter(|a| current.get(&a.provider).is_some_and(|c| *c != a.id))
            .filter(|a| {
                let last = self.asked.lock().unwrap().get(&a.id).copied();
                // a window it had has reset since: what it says now matters, gap or not
                let reset = self.others.lock().unwrap().get(&a.id).is_some_and(|(e, _)| e.windows.iter().any(|w| w.reset_at.is_some_and(|t| t <= now && last.is_none_or(|l| l < t))));
                reset || last.is_none_or(|t| now.saturating_sub(t) >= GAP)
            })
            .collect();
        for a in due {
            self.asked.lock().unwrap().insert(a.id.clone(), now);
            let Some(token) = self.token_of(pi_bin, &a).await else { continue };
            let asked = if a.provider == "openai-codex" { crate::usage::chatgpt::ask_with(&token, now).await } else { crate::usage::claude::ask_with(&token).await };
            if let crate::usage::Asked::Got(e) = asked {
                self.keep_numbers(&a.id, e, now);
            }
        }
    }

    /// Ask now, whatever the 5-minute pacing, for the numbers of `provider`'s accounts not in use (only `only`, when
    /// given). Baton Touch decides on these, not on an hour guessed when an account last ran out.
    async fn refresh_others(&self, pi_bin: &str, provider: &str, only: Option<&str>) {
        let now = now_ms();
        let current = self.current.lock().unwrap().get(provider).cloned();
        let list: Vec<Account> = read_store()
            .accounts
            .into_iter()
            .filter(|a| a.provider == provider && current.as_deref() != Some(a.id.as_str()) && only.is_none_or(|o| o == a.id))
            .collect();
        for a in list {
            self.asked.lock().unwrap().insert(a.id.clone(), now);
            let Some(token) = self.token_of(pi_bin, &a).await else { continue };
            let asked = if a.provider == "openai-codex" { crate::usage::chatgpt::ask_with(&token, now).await } else { crate::usage::claude::ask_with(&token).await };
            if let crate::usage::Asked::Got(e) = asked {
                self.keep_numbers(&a.id, e, now_ms());
            }
        }
    }

    /// When account `id` (not in use) is free again, by its numbers asked now: `None` when it is free now (or cannot
    /// be told).
    pub async fn free_at(&self, pi_bin: &str, id: &str) -> Option<u64> {
        let provider = read_store().accounts.into_iter().find(|a| a.id == id)?.provider;
        self.refresh_others(pi_bin, &provider, Some(id)).await;
        let now = now_ms();
        self.others.lock().unwrap().get(id).and_then(|(e, _)| e.used_up_until(now, 95.0))
    }

    /// A usable access token for an account not in use.
    async fn token_of(&self, pi_bin: &str, a: &Account) -> Option<String> {
        let fresh = a.cred["expires"].as_u64().is_some_and(|t| t > now_ms() + 5 * 60_000);
        if fresh {
            return a.cred["access"].as_str().map(str::to_owned);
        }
        let dir = crate::config::cache_dir().join(format!("refresh-{}", now_ms()));
        std::fs::create_dir_all(&dir).ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        let out = async {
            write_private(&dir.join("auth.json"), &json!({ (a.provider.clone()): a.cred }).to_string()).ok()?;
            let token = token_in(pi_bin, &a.provider, Some(&dir)).await?;
            let cred = read_auth(&dir)[&a.provider].clone();
            // keep the refreshed sign-in, unless the account has been put in use meanwhile (auth.json rules then)
            let _one = self.busy.lock().await;
            let mut store = read_store();
            let in_use = self.current.lock().unwrap().get(&a.provider) == Some(&a.id);
            if let Some(s) = store.accounts.iter_mut().find(|s| s.id == a.id).filter(|_| !in_use && cred.is_object()) {
                s.cred = cred;
                let _ = save_store(&store);
            }
            Some(token)
        }
        .await;
        let _ = std::fs::remove_dir_all(&dir);
        out
    }

    /// The limits as the page sees them, with who each provider's numbers belong to and the other accounts' cards.
    pub fn limits_view(&self, limits: &crate::limits::Limits, now: u64) -> Value {
        let mut v = limits.view(now);
        let store = read_store();
        let current = self.current.lock().unwrap().clone();
        let who = |id: &str| store.accounts.iter().find(|a| a.id == id).map(|a| json!({ "id": a.id, "email": a.email, "plan": a.plan }));
        if let Some(items) = v["items"].as_array_mut() {
            for it in items {
                if let Some(id) = it["provider"].as_str().and_then(|p| current.get(p)) {
                    it["account"] = who(id).unwrap_or(Value::Null);
                }
            }
        }
        let others = self.others.lock().unwrap();
        v["others"] = store
            .accounts
            .iter()
            .filter(|a| current.get(&a.provider).is_some_and(|c| *c != a.id))
            .map(|a| {
                let mut e = others.get(&a.id).map(|(e, _)| e.view(now)).unwrap_or_else(|| json!({ "provider": a.provider, "plan": a.plan, "windows": [] }));
                e["account"] = who(&a.id).unwrap_or(Value::Null);
                e
            })
            .collect();
        v
    }

    // ── adding an account ───────────────────────────────────────────────────

    async fn login_start(self: &Arc<Self>, pi_bin: &str, provider: &str) -> Result<(), String> {
        if !PROVIDERS.contains(&provider) {
            return Err("unknown provider".into());
        }
        if pi_bin.contains('\'') {
            return Err("pi's path cannot hold a quote".into());
        }
        script_args("")?;
        let mut slot = self.login.lock().await;
        if slot.as_ref().is_some_and(|l| l.state == "running") {
            return Err("a sign-in is already under way".into());
        }
        let dir = crate::config::cache_dir().join(format!("login-{}", now_ms()));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        // a wide terminal, so the sign-in address stays on one line
        let line = format!("stty cols 2000 rows 40 2>/dev/null; exec '{pi_bin}' auth login {provider} --no-browser");
        let mut child = tokio::process::Command::new("script")
            .args(script_args(&line)?)
            .env("PI_CODING_AGENT_DIR", &dir)
            .env("TERM", "xterm")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                let _ = std::fs::remove_dir_all(&dir);
                if e.kind() == std::io::ErrorKind::NotFound { NO_TERMINAL.to_owned() } else { format!("script: {e}") }
            })?;
        let stdout = child.stdout.take().ok_or("no output")?;
        let (kill_tx, kill_rx) = tokio::sync::oneshot::channel();
        *slot = Some(Login { provider: provider.to_owned(), lines: vec![], state: "running", error: None, account: None, stdin: child.stdin.take(), kill: Some(kill_tx) });
        drop(slot);

        // what it says, for the page
        let shared = self.login.clone();
        tokio::spawn(async move {
            let mut out = stdout;
            let mut buf = [0u8; 4096];
            let mut text = String::new();
            while let Ok(n) = out.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                text.push_str(&String::from_utf8_lossy(&buf[..n]));
                if text.len() > 64 * 1024 {
                    text.drain(..text.len() - 32 * 1024);
                }
                let lines = screen_lines(&text);
                if let Some(l) = shared.lock().await.as_mut() {
                    l.lines = lines;
                }
            }
        });

        // the end: the new sign-in joins the list
        let (me, pi_bin, provider) = (self.clone(), pi_bin.to_owned(), provider.to_owned());
        tokio::spawn(async move {
            let status = tokio::select! {
                s = child.wait() => s.ok(),
                _ = kill_rx => { let _ = child.kill().await; None }
                _ = tokio::time::sleep(std::time::Duration::from_millis(LOGIN_MAX_MS)) => { let _ = child.kill().await; None }
            };
            let signed_in = status.is_some_and(|s| s.success()) && read_auth(&dir)[&provider].is_object();
            let result = if signed_in {
                match who_in(&pi_bin, &provider, Some(&dir)).await {
                    Some((who, cred)) => {
                        let _one = me.busy.lock().await;
                        let mut store = read_store();
                        let id = upsert(&mut store, &provider, who, cred);
                        save_store(&store).map(|_| id)
                    }
                    None => Err("signed in, but could not tell whose account it is".to_owned()),
                }
            } else {
                Err(if status.is_none() { "cancelled".to_owned() } else { "sign-in did not finish".to_owned() })
            };
            let _ = std::fs::remove_dir_all(&dir);
            if let Some(l) = me.login.lock().await.as_mut() {
                l.stdin = None;
                l.kill = None;
                match result {
                    Ok(id) => {
                        l.state = "done";
                        l.account = Some(id);
                    }
                    Err(e) => {
                        l.state = "failed";
                        l.error = Some(e);
                    }
                }
            }
        });
        Ok(())
    }

    async fn login_input(&self, text: &str) -> Result<(), String> {
        let text = text.trim();
        if text.is_empty() || text.len() > 4096 || text.contains(['\r', '\n']) {
            return Err("paste the code on one line".into());
        }
        let mut slot = self.login.lock().await;
        let stdin = slot.as_mut().and_then(|l| l.stdin.as_mut()).ok_or("no sign-in is waiting")?;
        stdin.write_all(format!("{text}\r").as_bytes()).await.map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())
    }

    async fn login_cancel(&self) {
        let mut slot = self.login.lock().await;
        match slot.as_mut() {
            Some(l) if l.state == "running" => {
                if let Some(k) = l.kill.take() {
                    let _ = k.send(());
                }
            }
            _ => *slot = None,
        }
    }
}

/// Signing in from the page needs `script` (a pseudo terminal); without it the person signs in with pi in a terminal
/// and this list picks the new sign-in up (the account it replaces was saved when the list was shown).
const NO_TERMINAL: &str = "no-terminal";

/// `script` running `line` in a pseudo terminal, output unbuffered: util-linux and BSD/macOS take different arguments.
fn script_args(line: &str) -> Result<Vec<String>, String> {
    if cfg!(target_os = "linux") {
        Ok(vec!["-qfec".into(), line.into(), "/dev/null".into()])
    } else if cfg!(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd", target_os = "netbsd")) {
        Ok(vec!["-q".into(), "/dev/null".into(), "sh".into(), "-c".into(), line.into()])
    } else {
        Err(NO_TERMINAL.into())
    }
}

/// The terminal's text as lines a page can show: escape sequences out, carriage returns resolved, blanks dropped.
fn screen_lines(raw: &str) -> Vec<String> {
    let mut clean = String::with_capacity(raw.len());
    let mut it = raw.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\u{1b}' => match it.next() {
                Some('[') => {
                    while let Some(&n) = it.peek() {
                        it.next();
                        if n.is_ascii_alphabetic() || n == '~' {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(n) = it.next() {
                        if n == '\u{7}' || (n == '\u{1b}' && it.peek() == Some(&'\\')) {
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\r' => {
                // a carriage return before a newline is a line end; alone it rewrites the line
                if it.peek() != Some(&'\n') {
                    if let Some(i) = clean.rfind('\n') {
                        clean.truncate(i + 1);
                    } else {
                        clean.clear();
                    }
                }
            }
            c if c.is_control() && c != '\n' && c != '\t' => {}
            c => clean.push(c),
        }
    }
    let lines: Vec<String> = clean.lines().map(|l| l.trim_end().to_owned()).filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(LOGIN_LINES)..].to_vec()
}

// ── routes ───────────────────────────────────────────────────────────────────

pub async fn get(State(app): State<Arc<AppState>>) -> Json<Value> {
    Json(app.runtimes.accounts.view(&app.pi_bin).await)
}

#[derive(Deserialize)]
pub struct IdReq {
    id: String,
}

pub async fn switch(State(app): State<Arc<AppState>>, Json(req): Json<IdReq>) -> Response {
    match app.runtimes.accounts.switch(&app.pi_bin, &req.id, &app.runtimes.limits).await {
        Ok(a) => {
            app.runtimes.accounts.announce(None, &a);
            // the plan numbers now belong to another account: ask again
            let _ = app.runtimes.usage.poll(&app.pi_bin, &app.runtimes.limits, true, Some(&a.provider)).await;
            Json(app.runtimes.accounts.view(&app.pi_bin).await).into_response()
        }
        Err(e) => fail(StatusCode::CONFLICT, e),
    }
}

pub async fn remove(State(app): State<Arc<AppState>>, Json(req): Json<IdReq>) -> Response {
    match app.runtimes.accounts.remove(&app.pi_bin, &req.id).await {
        Ok(()) => Json(app.runtimes.accounts.view(&app.pi_bin).await).into_response(),
        Err(e) => fail(StatusCode::CONFLICT, e),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatonReq {
    enabled: Option<bool>,
    cross_provider: Option<bool>,
    targets: Option<Vec<Target>>,
    return_home: Option<bool>,
}

pub async fn set_baton(State(app): State<Arc<AppState>>, Json(req): Json<BatonReq>) -> Response {
    let a = &app.runtimes.accounts;
    let r = {
        let _one = a.busy.lock().await;
        let mut store = read_store();
        if let Some(on) = req.enabled {
            store.baton_touch = Some(on);
        }
        if let Some(on) = req.cross_provider {
            store.cross_provider = Some(on);
        }
        if let Some(on) = req.return_home {
            store.return_home = Some(on);
        }
        if let Some(t) = req.targets {
            let ok = |s: &str, n: usize| !s.is_empty() && s.len() <= n && !s.chars().any(char::is_control);
            if t.len() > 32 || !t.iter().all(|t| ok(&t.provider, 100) && ok(&t.model, 200)) {
                return fail(StatusCode::BAD_REQUEST, "bad provider list");
            }
            store.targets = t;
        }
        save_store(&store)
    };
    match r {
        Ok(()) => Json(a.view(&app.pi_bin).await).into_response(),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(Deserialize)]
pub struct LoginReq {
    provider: String,
}

pub async fn login_start(State(app): State<Arc<AppState>>, Json(req): Json<LoginReq>) -> Response {
    match app.runtimes.accounts.login_start(&app.pi_bin, &req.provider).await {
        Ok(()) => {
            // the first lines (the address) come within a moment
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            Json(app.runtimes.accounts.view(&app.pi_bin).await).into_response()
        }
        Err(e) => fail(StatusCode::CONFLICT, e),
    }
}

#[derive(Deserialize)]
pub struct InputReq {
    text: String,
}

pub async fn login_input(State(app): State<Arc<AppState>>, Json(req): Json<InputReq>) -> Response {
    match app.runtimes.accounts.login_input(&req.text).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => fail(StatusCode::CONFLICT, e),
    }
}

pub async fn login_cancel(State(app): State<Arc<AppState>>) -> Response {
    app.runtimes.accounts.login_cancel().await;
    StatusCode::NO_CONTENT.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_chatgpt_token_without_the_network() {
        // header.payload.signature, payload = {"sub":"s","https://api.openai.com/auth":{"chatgpt_account_user_id":"u1","chatgpt_plan_type":"plus"},"https://api.openai.com/profile":{"email":"a@b.c"}}
        let payload = r#"{"sub":"s","https://api.openai.com/auth":{"chatgpt_account_user_id":"u1","chatgpt_plan_type":"plus"},"https://api.openai.com/profile":{"email":"a@b.c"}}"#;
        let enc = |b: &[u8]| {
            const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
            let mut s = String::new();
            for ch in b.chunks(3) {
                let n = (ch[0] as u32) << 16 | (*ch.get(1).unwrap_or(&0) as u32) << 8 | *ch.get(2).unwrap_or(&0) as u32;
                for i in 0..=ch.len() {
                    s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
                }
            }
            s
        };
        let cred = json!({ "access": format!("h.{}.s", enc(payload.as_bytes())) });
        let w = who_openai(&cred).unwrap();
        assert_eq!((w.key.as_str(), w.email.as_deref(), w.plan.as_deref()), ("u1", Some("a@b.c"), Some("plus")));
    }

    #[test]
    fn the_terminal_text_becomes_plain_lines() {
        let raw = "\u{1b}[?25l\u{1b}[2KSelect method: Copy code\r\n\u{1b}[1mOpen this URL:\u{1b}[0m\r\n  https://claude.ai/oauth/authorize?a=1\r\nPaste the code: \rPaste the code (code#state): ";
        assert_eq!(screen_lines(raw), ["Select method: Copy code", "Open this URL:", "  https://claude.ai/oauth/authorize?a=1", "Paste the code (code#state):"]);
    }

    fn acct(id: &str, provider: &str, limited_until: Option<u64>) -> Account {
        Account { id: id.into(), provider: provider.into(), email: None, name: None, plan: None, saved_at: 0, limited_until, cred: json!({}) }
    }

    #[test]
    fn baton_touch_picks_a_free_account_of_the_same_provider() {
        let now = 1_000_000;
        let list = [acct("a:1", "anthropic", Some(now + 500)), acct("o:1", "openai-codex", None), acct("a:2", "anthropic", Some(now - 1)), acct("a:3", "anthropic", None)];
        let busy = |a: &Account| a.limited_until;
        // a:2's mark has passed: it is free, and comes before a:3
        assert_eq!(pick_next(&list, "anthropic", "a:1", now, busy), Pick::Now(2));
        // its numbers can still say it is used up
        let busy = |a: &Account| if a.id == "a:2" { Some(now + 100) } else { a.limited_until };
        assert_eq!(pick_next(&list, "anthropic", "a:1", now, busy), Pick::Now(3));
    }

    #[test]
    fn baton_touch_waits_for_the_account_that_resets_first() {
        let now = 1_000_000;
        let list = [acct("a:1", "anthropic", Some(now + 5_000)), acct("a:2", "anthropic", Some(now + 3_000)), acct("a:3", "anthropic", Some(now + 2_000))];
        assert_eq!(pick_next(&list, "anthropic", "a:1", now, |a| a.limited_until), Pick::Later(2, now + 2_000));
        // the one in use resets first: nothing to win by switching
        let list = [acct("a:1", "anthropic", Some(now + 1_000)), acct("a:2", "anthropic", Some(now + 3_000))];
        assert_eq!(pick_next(&list, "anthropic", "a:1", now, |a| a.limited_until), Pick::None);
        // the only account
        assert_eq!(pick_next(&list[..1], "anthropic", "a:1", now, |a| a.limited_until), Pick::None);
    }

    #[test]
    fn another_provider_is_the_first_allowed_one_with_plan_left() {
        let t = |p: &str, m: &str| Target { provider: p.into(), model: m.into() };
        let targets = [t("anthropic", "x"), t("openai-codex", ""), t("openai-codex", "gpt"), t("zai", "glm")];
        assert_eq!(pick_target(&targets, "anthropic", |_| false), Some(t("openai-codex", "gpt")));
        assert_eq!(pick_target(&targets, "anthropic", |p| p == "openai-codex"), Some(t("zai", "glm")));
        assert_eq!(pick_target(&targets, "anthropic", |_| true), None);
    }

    #[test]
    fn fresh_numbers_beat_a_guessed_mark() {
        let now = 10_000_000;
        let e = |used: f64, reset: u64| crate::limits::Entry { provider: "anthropic".into(), plan: None, at: 1, limit_reached: false, windows: vec![crate::limits::Window { minutes: 300, used, reset_at: Some(reset) }], resets: None, reset_kinds: vec![] };
        // marked for an hour (a guess), but asked a moment ago: still used up for two hours
        let used = e(100.0, now + 7_200_000);
        assert_eq!(busy_until(Some(now + 3_600_000), Some((&used, now - 1_000)), now), Some(now + 7_200_000));
        // the guess has passed, the numbers say it is still used up
        assert_eq!(busy_until(Some(now - 60_000), Some((&used, now - 1_000)), now), Some(now + 7_200_000));
        // marked, but the numbers asked now say it is free again
        let free = e(10.0, now + 7_200_000);
        assert_eq!(busy_until(Some(now + 3_600_000), Some((&free, now - 1_000)), now), None);
        // no fresh numbers: the mark, or old numbers, whichever is later
        assert_eq!(busy_until(Some(now + 3_600_000), None, now), Some(now + 3_600_000));
        assert_eq!(busy_until(Some(now + 3_600_000), Some((&used, now - 3_600_000)), now), Some(now + 7_200_000));
    }

    #[test]
    fn one_failure_is_handed_over_once() {
        let mut b = std::collections::BTreeMap::new();
        b.insert("anthropic".to_owned(), 1_000_000u64);
        assert!(just_switched(&b, "anthropic", 1_000_000 + BATON_SETTLE_MS - 1));
        assert!(!just_switched(&b, "anthropic", 1_000_000 + BATON_SETTLE_MS));
        assert!(!just_switched(&b, "openai-codex", 1_000_000));
    }

    #[test]
    fn the_numbers_go_with_the_account() {
        let accounts = Accounts::default();
        let limits = crate::limits::Limits::load(None);
        let w = |used: f64| crate::limits::Window { minutes: 300, used, reset_at: Some(now_ms() + 60_000) };
        let entry = |used: f64| crate::limits::Entry { provider: "anthropic".into(), plan: Some("max".into()), at: 1, limit_reached: false, windows: vec![w(used)], resets: None, reset_kinds: vec![] };
        limits.replace(entry(100.0));
        // a:2 has not been asked: no numbers rather than a:1's
        accounts.handed_over("anthropic", Some("a:1"), "a:2", &limits);
        assert!(limits.entry("anthropic").unwrap().windows.is_empty());
        // and back: a:1's numbers come back with it
        limits.replace(entry(10.0));
        accounts.handed_over("anthropic", Some("a:2"), "a:1", &limits);
        assert_eq!(limits.entry("anthropic").unwrap().windows[0].used, 100.0);
        assert_eq!(accounts.others.lock().unwrap()["a:2"].0.windows[0].used, 10.0);
        assert_eq!(accounts.current.lock().unwrap()["anthropic"], "a:1");
    }

    #[test]
    fn a_sign_in_is_the_same_by_its_refresh_token() {
        assert!(same_sign_in(&json!({"refresh":"r1"}), &json!({"refresh":"r1","access":"x"})));
        assert!(!same_sign_in(&json!({"refresh":""}), &json!({"refresh":""})));
        assert!(!same_sign_in(&json!({}), &json!({})));
    }
}
