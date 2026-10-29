//! The session folder's files, for the explorer and the editor in the web UI.
//!
//! Everything is relative to the folder the session works in and never leaves it: a path that climbs out
//! (`..`), or a symlink that points outside, is refused. `.git` is left alone. Files are only ever
//! written as UTF-8 text, atomically (a temp file in the same folder, then a rename), and a write that
//! names the revision it was based on is refused if the file changed on disk since, so an edit made here
//! cannot silently undo what pi (or anything else) wrote in the meantime.

use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::UNIX_EPOCH,
};

use axum::{
    extract::{Path as UrlPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    api::fail,
    git::{runtime_cwd, safe_rel_path},
    AppState,
};

const MAX_READ: u64 = 2_000_000;
pub const MAX_WRITE: usize = 5_000_000;
const MAX_LIST: usize = 2000;
const MAX_STAT: usize = 100;

type Fail = (StatusCode, &'static str);

fn protected(rel: &str) -> bool {
    Path::new(rel).components().any(|c| matches!(c, Component::Normal(n) if n == ".git"))
}

/// The folder itself, resolved once so every comparison below is between canonical paths.
async fn root_of(app: &AppState, id: &str) -> Result<PathBuf, Fail> {
    let cwd = runtime_cwd(app, id).await?;
    cwd.canonicalize().map_err(|_| (StatusCode::NOT_FOUND, "the folder is gone"))
}

fn rel_ok(rel: &str, allow_root: bool) -> Result<(), Fail> {
    if rel.is_empty() && allow_root {
        return Ok(());
    }
    if !safe_rel_path(rel) {
        return Err((StatusCode::BAD_REQUEST, "bad path"));
    }
    if protected(rel) {
        return Err((StatusCode::FORBIDDEN, "protected"));
    }
    Ok(())
}

/// An existing file or folder, links followed, and it must still be inside the folder.
fn existing(root: &Path, rel: &str, allow_root: bool) -> Result<PathBuf, Fail> {
    rel_ok(rel, allow_root)?;
    let p = root.join(rel).canonicalize().map_err(|_| (StatusCode::NOT_FOUND, "not found"))?;
    if !p.starts_with(root) {
        return Err((StatusCode::FORBIDDEN, "outside the folder"));
    }
    Ok(p)
}

/// An entry by its own name, links not followed (to delete or rename a link itself): its folder is
/// resolved and checked, the last name is not.
fn entry(root: &Path, rel: &str) -> Result<PathBuf, Fail> {
    rel_ok(rel, false)?;
    let full = root.join(rel);
    let name = full.file_name().ok_or((StatusCode::BAD_REQUEST, "bad path"))?.to_owned();
    let parent = full.parent().ok_or((StatusCode::BAD_REQUEST, "bad path"))?.canonicalize().map_err(|_| (StatusCode::NOT_FOUND, "not found"))?;
    if !parent.starts_with(root) {
        return Err((StatusCode::FORBIDDEN, "outside the folder"));
    }
    Ok(parent.join(name))
}

fn rev(m: &fs::Metadata) -> String {
    let ns = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    format!("{ns}-{}", m.len())
}

fn millis(m: &fs::Metadata) -> u64 {
    m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, Fail> + Send + 'static) -> Result<T, Fail> {
    tokio::task::spawn_blocking(f).await.unwrap_or(Err((StatusCode::INTERNAL_SERVER_ERROR, "worker failed")))
}

fn io_fail(e: std::io::Error) -> Fail {
    match e.kind() {
        std::io::ErrorKind::NotFound => (StatusCode::NOT_FOUND, "not found"),
        std::io::ErrorKind::PermissionDenied => (StatusCode::FORBIDDEN, "permission denied"),
        std::io::ErrorKind::AlreadyExists => (StatusCode::CONFLICT, "already exists"),
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "could not complete"),
    }
}

// ── list ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct PathQuery {
    #[serde(default)]
    path: String,
}

#[derive(Serialize)]
struct Item {
    name: String,
    kind: &'static str,
    size: u64,
    mtime: u64,
    /// A symbolic link (shown, but one that points outside the folder cannot be opened).
    link: bool,
    blocked: bool,
}

pub async fn list(State(app): State<Arc<AppState>>, UrlPath(id): UrlPath<String>, Query(q): Query<PathQuery>) -> Response {
    let root = match root_of(&app, &id).await {
        Ok(r) => r,
        Err((c, m)) => return fail(c, m),
    };
    let rel = q.path.trim_matches('/').to_owned();
    let res = blocking(move || {
        let dir = existing(&root, &rel, true)?;
        if !dir.is_dir() {
            return Err((StatusCode::BAD_REQUEST, "not a folder"));
        }
        let mut items = Vec::new();
        let mut truncated = false;
        for e in fs::read_dir(&dir).map_err(io_fail)? {
            let Ok(e) = e else { continue };
            let name = e.file_name().to_string_lossy().into_owned();
            if name == ".git" {
                continue;
            }
            if items.len() >= MAX_LIST {
                truncated = true;
                break;
            }
            let link = e.file_type().map(|t| t.is_symlink()).unwrap_or(false);
            let (meta, blocked) = if link {
                match e.path().canonicalize() {
                    Ok(t) if t.starts_with(&root) => (fs::metadata(&t).ok(), false),
                    _ => (None, true),
                }
            } else {
                (e.metadata().ok(), false)
            };
            let (kind, size, mtime) = match &meta {
                Some(m) if m.is_dir() => ("dir", 0, millis(m)),
                Some(m) => ("file", m.len(), millis(m)),
                None => ("file", 0, 0),
            };
            items.push(Item { name, kind, size, mtime, link, blocked });
        }
        items.sort_by(|a, b| (a.kind != "dir").cmp(&(b.kind != "dir")).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())).then_with(|| a.name.cmp(&b.name)));
        Ok(json!({ "path": rel, "entries": items, "truncated": truncated }))
    })
    .await;
    match res {
        Ok(v) => Json(v).into_response(),
        Err((c, m)) => fail(c, m),
    }
}

// ── read ─────────────────────────────────────────────────────────────────────

pub async fn read(State(app): State<Arc<AppState>>, UrlPath(id): UrlPath<String>, Query(q): Query<PathQuery>) -> Response {
    let root = match root_of(&app, &id).await {
        Ok(r) => r,
        Err((c, m)) => return fail(c, m),
    };
    let res = blocking(move || {
        let p = existing(&root, q.path.trim_matches('/'), false)?;
        let meta = fs::metadata(&p).map_err(io_fail)?;
        if !meta.is_file() {
            return Err((StatusCode::BAD_REQUEST, "not a file"));
        }
        if meta.len() > MAX_READ {
            return Err((StatusCode::PAYLOAD_TOO_LARGE, "too large"));
        }
        let bytes = fs::read(&p).map_err(io_fail)?;
        if bytes[..bytes.len().min(8000)].contains(&0) {
            return Err((StatusCode::UNSUPPORTED_MEDIA_TYPE, "binary"));
        }
        let mut text = String::from_utf8(bytes).map_err(|_| (StatusCode::UNSUPPORTED_MEDIA_TYPE, "not utf-8"))?;
        let bom = text.starts_with('\u{feff}');
        if bom {
            text.remove(0);
        }
        let crlf = text.matches("\r\n").count();
        let lf = text.matches('\n').count() - crlf;
        let eol = if crlf > lf { "crlf" } else { "lf" };
        if eol == "crlf" {
            text = text.replace("\r\n", "\n");
        }
        Ok(json!({ "path": q.path, "content": text, "rev": rev(&meta), "mtime": millis(&meta), "size": meta.len(), "eol": eol, "bom": bom }))
    })
    .await;
    match res {
        Ok(v) => Json(v).into_response(),
        Err((c, m)) => fail(c, m),
    }
}

// ── write ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct WriteReq {
    path: String,
    content: String,
    /// The revision this text was based on. Required to overwrite a file unless `overwrite` is set.
    rev: Option<String>,
    #[serde(default)]
    overwrite: bool,
    eol: Option<String>,
    #[serde(default)]
    bom: bool,
}

pub async fn write(State(app): State<Arc<AppState>>, UrlPath(id): UrlPath<String>, Json(req): Json<WriteReq>) -> Response {
    if req.content.len() > MAX_WRITE {
        return fail(StatusCode::PAYLOAD_TOO_LARGE, "too large");
    }
    let root = match root_of(&app, &id).await {
        Ok(r) => r,
        Err((c, m)) => return fail(c, m),
    };
    enum Outcome {
        Done(String, u64, u64),
        Changed(String),
        Gone,
    }
    let res = blocking(move || {
        let rel = req.path.trim_matches('/');
        // an existing file is written through any link (and the target must be inside); a new one is made in an existing folder
        let (target, current) = match existing(&root, rel, false) {
            Ok(p) => {
                let m = fs::metadata(&p).map_err(io_fail)?;
                if !m.is_file() {
                    return Err((StatusCode::BAD_REQUEST, "not a file"));
                }
                (p, Some(m))
            }
            Err((StatusCode::NOT_FOUND, _)) => (entry(&root, rel)?, None),
            Err(e) => return Err(e),
        };
        match (&current, &req.rev) {
            (Some(m), Some(r)) if !req.overwrite && *r != rev(m) => return Ok(Outcome::Changed(rev(m))),
            (Some(m), None) if !req.overwrite => return Ok(Outcome::Changed(rev(m))),
            (None, Some(_)) if !req.overwrite => return Ok(Outcome::Gone),
            _ => {}
        }
        let mut data = if req.eol.as_deref() == Some("crlf") { req.content.replace('\n', "\r\n") } else { req.content.clone() };
        if req.bom {
            data.insert(0, '\u{feff}');
        }
        let dir = target.parent().ok_or((StatusCode::BAD_REQUEST, "bad path"))?;
        let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let stamp = std::time::SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let tmp = dir.join(format!(".{name}.oven-{}-{stamp}.tmp", std::process::id()));
        let put = (|| -> std::io::Result<()> {
            let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
            f.write_all(data.as_bytes())?;
            f.sync_all()?;
            if let Some(m) = &current {
                fs::set_permissions(&tmp, m.permissions())?;
            }
            fs::rename(&tmp, &target)
        })();
        if let Err(e) = put {
            let _ = fs::remove_file(&tmp);
            return Err(io_fail(e));
        }
        let m = fs::metadata(&target).map_err(io_fail)?;
        Ok(Outcome::Done(rev(&m), millis(&m), m.len()))
    })
    .await;
    match res {
        Ok(Outcome::Done(rev, mtime, size)) => Json(json!({ "rev": rev, "mtime": mtime, "size": size })).into_response(),
        Ok(Outcome::Changed(cur)) => (StatusCode::CONFLICT, Json(json!({ "error": "changed on disk", "rev": cur }))).into_response(),
        Ok(Outcome::Gone) => (StatusCode::CONFLICT, Json(json!({ "error": "deleted on disk", "rev": null }))).into_response(),
        Err((c, m)) => fail(c, m),
    }
}

// ── create / rename / delete ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateReq {
    path: String,
    kind: String,
}

pub async fn create(State(app): State<Arc<AppState>>, UrlPath(id): UrlPath<String>, Json(req): Json<CreateReq>) -> Response {
    let root = match root_of(&app, &id).await {
        Ok(r) => r,
        Err((c, m)) => return fail(c, m),
    };
    let res = blocking(move || {
        let p = entry(&root, req.path.trim_matches('/'))?;
        match req.kind.as_str() {
            "file" => fs::OpenOptions::new().write(true).create_new(true).open(&p).map(|_| ()).map_err(io_fail),
            "dir" => fs::create_dir(&p).map_err(io_fail),
            _ => Err((StatusCode::BAD_REQUEST, "bad kind")),
        }
    })
    .await;
    match res {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err((c, m)) => fail(c, m),
    }
}

#[derive(Deserialize)]
pub struct RenameReq {
    from: String,
    to: String,
}

pub async fn rename(State(app): State<Arc<AppState>>, UrlPath(id): UrlPath<String>, Json(req): Json<RenameReq>) -> Response {
    let root = match root_of(&app, &id).await {
        Ok(r) => r,
        Err((c, m)) => return fail(c, m),
    };
    let res = blocking(move || {
        let from = entry(&root, req.from.trim_matches('/'))?;
        let to = entry(&root, req.to.trim_matches('/'))?;
        if fs::symlink_metadata(&from).is_err() {
            return Err((StatusCode::NOT_FOUND, "not found"));
        }
        if fs::symlink_metadata(&to).is_ok() {
            return Err((StatusCode::CONFLICT, "already exists"));
        }
        fs::rename(&from, &to).map_err(io_fail)
    })
    .await;
    match res {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err((c, m)) => fail(c, m),
    }
}

/// Deletes for good (there is no trash); the web UI asks first. A symbolic link is removed itself, never what it points at.
pub async fn delete(State(app): State<Arc<AppState>>, UrlPath(id): UrlPath<String>, Query(q): Query<PathQuery>) -> Response {
    let root = match root_of(&app, &id).await {
        Ok(r) => r,
        Err((c, m)) => return fail(c, m),
    };
    let res = blocking(move || {
        let p = entry(&root, q.path.trim_matches('/'))?;
        let m = fs::symlink_metadata(&p).map_err(io_fail)?;
        if m.is_dir() { fs::remove_dir_all(&p) } else { fs::remove_file(&p) }.map_err(io_fail)
    })
    .await;
    match res {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err((c, m)) => fail(c, m),
    }
}

// ── stat ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct StatReq {
    paths: Vec<String>,
}

/// For open editors: has the file changed on disk? One request answers for all of them.
pub async fn stat(State(app): State<Arc<AppState>>, UrlPath(id): UrlPath<String>, Json(req): Json<StatReq>) -> Response {
    if req.paths.len() > MAX_STAT {
        return fail(StatusCode::BAD_REQUEST, "too many paths");
    }
    let root = match root_of(&app, &id).await {
        Ok(r) => r,
        Err((c, m)) => return fail(c, m),
    };
    let out = blocking(move || {
        let mut map = serde_json::Map::new();
        for p in &req.paths {
            let v = existing(&root, p.trim_matches('/'), false).ok().and_then(|f| fs::metadata(f).ok()).filter(|m| m.is_file()).map_or(serde_json::Value::Null, |m| json!({ "rev": rev(&m), "mtime": millis(&m), "size": m.len() }));
            map.insert(p.clone(), v);
        }
        Ok(serde_json::Value::Object(map))
    })
    .await;
    match out {
        Ok(v) => Json(v).into_response(),
        Err((c, m)) => fail(c, m),
    }
}
