//! What changed in a session's working folder, straight from git: it catches edits made by any tool,
//! `bash` included, not only the ones pi's own `edit` and `write` report.
//!
//! git can run programs named in a repository's own config (`core.fsmonitor`, external diff drivers,
//! text converters). The folder is the person's own, but these are switched off anyway: this server
//! only ever needs to *read* the state. The two things it writes (staging and committing) are the
//! ones a person asked for with a click; a commit runs the repository's own hooks, as it would in a shell.

use std::{path::Path, process::Stdio, sync::Arc, time::Duration};

use axum::{
    extract::{Path as UrlPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use tokio::{io::AsyncReadExt, process::Command};

use crate::{api::fail, AppState};

const MAX_OUTPUT: usize = 1_000_000;
const MAX_FILES: usize = 500;
const MAX_DIFF_LINES: usize = 4000;

/// Runs `git` in `dir`; returns (stdout, was it cut at the size limit).
async fn git(dir: &Path, args: &[&str]) -> Result<(Vec<u8>, bool), String> {
    let mut child = Command::new("git")
        .args(["-c", "core.fsmonitor=false", "-c", "core.pager=cat", "-c", "color.ui=never", "-c", "core.quotepath=off"])
        .args(args)
        .current_dir(dir)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EXTERNAL_DIFF", "")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("could not run git: {e}"))?;
    let mut out = child.stdout.take().ok_or("no stdout")?;
    let read = async {
        let mut buf = Vec::new();
        (&mut out).take(MAX_OUTPUT as u64 + 1).read_to_end(&mut buf).await.map(|_| buf)
    };
    let mut buf = tokio::time::timeout(Duration::from_secs(10), read).await.map_err(|_| "git took too long")?.map_err(|e| e.to_string())?;
    let cut = buf.len() > MAX_OUTPUT;
    buf.truncate(MAX_OUTPUT);
    if cut {
        let _ = child.kill().await;
        return Ok((buf, true));
    }
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait()).await.map_err(|_| "git did not exit")?.map_err(|e| e.to_string())?;
    if status.success() { Ok((buf, false)) } else { Err(format!("git exited with {status}")) }
}

/// A path git will be handed after `--`: relative, no way up.
pub fn safe_rel_path(p: &str) -> bool {
    !p.is_empty() && !p.contains('\0') && !p.starts_with('/') && !p.starts_with('\\') && !p.split(['/', '\\']).any(|c| c == "..")
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Changed {
    pub path: String,
    /// M modified, A added, D deleted, R renamed, ? untracked, U conflicted, ! ignored
    pub status: char,
    /// The index holds a change for this file.
    pub staged: bool,
    /// The working tree differs from the index (so a staged file can be staged only in part).
    pub unstaged: bool,
    pub added: Option<u32>,
    pub removed: Option<u32>,
}

/// `git status --porcelain=v1 -z`
pub fn parse_status(raw: &[u8]) -> Vec<Changed> {
    let text = String::from_utf8_lossy(raw);
    let mut parts = text.split('\0');
    let mut out = Vec::new();
    while let Some(rec) = parts.next() {
        if rec.len() < 4 {
            continue;
        }
        let (x, y, path) = (rec.as_bytes()[0] as char, rec.as_bytes()[1] as char, &rec[3..]);
        if matches!(x, 'R' | 'C') || matches!(y, 'R' | 'C') {
            parts.next(); // the name it was renamed from
        }
        let status = match (x, y) {
            ('?', '?') => '?',
            ('!', '!') => '!',
            ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D') => 'U',
            (_, c) if c != ' ' => c,
            (c, _) => c,
        };
        let staged = !matches!(x, ' ' | '?' | '!');
        let unstaged = y != ' ' && !matches!(x, '!');
        out.push(Changed { path: path.to_owned(), status, staged, unstaged, added: None, removed: None });
    }
    out
}

/// `git diff --numstat -z`: path -> (added, removed); binary files are `-` and give None.
pub fn parse_numstat(raw: &[u8]) -> Vec<(String, Option<u32>, Option<u32>)> {
    let text = String::from_utf8_lossy(raw);
    let mut parts = text.split('\0');
    let mut out = Vec::new();
    while let Some(rec) = parts.next() {
        let mut f = rec.splitn(3, '\t');
        let (Some(a), Some(r), Some(path)) = (f.next(), f.next(), f.next()) else { continue };
        let path = if path.is_empty() {
            parts.next(); // renames: "A\tR\t" then the old and new names
            parts.next().unwrap_or("").to_owned()
        } else {
            path.to_owned()
        };
        out.push((path, a.parse().ok(), r.parse().ok()));
    }
    out
}

/// A new file as a diff, for files git does not know yet.
fn untracked_diff(root: &Path, rel: &str) -> String {
    let Ok(bytes) = std::fs::read(root.join(rel)) else { return String::new() };
    if bytes.contains(&0) {
        return format!("Binary file {rel} (new)\n");
    }
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().take(MAX_DIFF_LINES).collect();
    let mut out = format!("--- /dev/null\n+++ b/{rel}\n@@ -0,0 +1,{} @@\n", lines.len());
    for l in &lines {
        out.push('+');
        out.push_str(l);
        out.push('\n');
    }
    if text.lines().count() > lines.len() {
        out.push_str("\\ (the rest of the file is not shown)\n");
    }
    out
}

fn count_lines(root: &Path, rel: &str) -> Option<u32> {
    let meta = std::fs::metadata(root.join(rel)).ok()?;
    if !meta.is_file() || meta.len() > 2_000_000 {
        return None;
    }
    let bytes = std::fs::read(root.join(rel)).ok()?;
    (!bytes.contains(&0)).then(|| String::from_utf8_lossy(&bytes).lines().count() as u32)
}

async fn repo_root(cwd: &Path) -> Option<std::path::PathBuf> {
    let (out, _) = git(cwd, &["rev-parse", "--show-toplevel"]).await.ok()?;
    let s = String::from_utf8_lossy(&out).trim().to_owned();
    (!s.is_empty()).then(|| s.into())
}

async fn has_head(root: &Path) -> bool {
    git(root, &["rev-parse", "--verify", "-q", "HEAD"]).await.is_ok()
}

#[derive(Serialize)]
struct Commit {
    hash: String,
    subject: String,
    /// Unix seconds.
    when: i64,
}

fn parse_log(raw: &[u8]) -> Vec<Commit> {
    String::from_utf8_lossy(raw)
        .split('\0')
        .filter_map(|rec| {
            let mut f = rec.trim_start_matches('\n').splitn(3, '\x1f');
            Some(Commit { hash: f.next()?.to_owned(), subject: f.next()?.to_owned(), when: f.next()?.trim().parse().ok()? })
        })
        .collect()
}

#[derive(Serialize)]
struct Overview {
    repo: bool,
    branch: Option<String>,
    root: Option<String>,
    files: Vec<Changed>,
    truncated: bool,
    commits: Vec<Commit>,
}

pub async fn runtime_cwd(app: &AppState, id: &str) -> Result<std::path::PathBuf, (StatusCode, &'static str)> {
    match app.runtimes.get(id).await {
        Some(rt) => Ok(rt.info.cwd.clone().into()),
        None => Err((StatusCode::NOT_FOUND, "no such runtime")),
    }
}

pub async fn status(State(app): State<Arc<AppState>>, UrlPath(id): UrlPath<String>) -> Response {
    let cwd = match runtime_cwd(&app, &id).await {
        Ok(c) => c,
        Err((code, msg)) => return fail(code, msg),
    };
    let Some(root) = repo_root(&cwd).await else {
        return Json(Overview { repo: false, branch: None, root: None, files: vec![], truncated: false, commits: vec![] }).into_response();
    };
    let branch = git(&root, &["rev-parse", "--abbrev-ref", "HEAD"]).await.ok().map(|(o, _)| String::from_utf8_lossy(&o).trim().to_owned()).filter(|b| !b.is_empty());
    let mut files = match git(&root, &["status", "--porcelain=v1", "-z", "--untracked-files=normal", "--ignored"]).await {
        Ok((out, _)) => parse_status(&out),
        Err(e) => return fail(StatusCode::BAD_GATEWAY, e),
    };
    files.sort_by_key(|f| f.status == '!'); // ignored files last, so they never push real changes out
    let truncated = files.len() > MAX_FILES;
    files.truncate(MAX_FILES);
    if has_head(&root).await {
        if let Ok((out, _)) = git(&root, &["diff", "--numstat", "-z", "--no-ext-diff", "--no-textconv", "HEAD", "--"]).await {
            for (path, a, r) in parse_numstat(&out) {
                if let Some(f) = files.iter_mut().find(|f| f.path == path) {
                    (f.added, f.removed) = (a, r);
                }
            }
        }
    }
    let root2 = root.clone();
    let files = tokio::task::spawn_blocking(move || {
        for f in files.iter_mut().filter(|f| f.status == '?' || (f.status == 'A' && f.added.is_none())) {
            f.added = count_lines(&root2, &f.path);
            f.removed = Some(0);
        }
        files
    })
    .await
    .unwrap_or_default();
    let commits = match git(&root, &["log", "-n", "8", "-z", "--format=%h%x1f%s%x1f%ct"]).await {
        Ok((out, _)) => parse_log(&out),
        Err(_) => vec![], // no commits yet
    };
    Json(Overview { repo: true, branch, root: Some(root.to_string_lossy().into_owned()), files, truncated, commits }).into_response()
}

#[derive(Deserialize)]
pub struct DiffQuery {
    path: String,
}

pub async fn diff(State(app): State<Arc<AppState>>, UrlPath(id): UrlPath<String>, Query(q): Query<DiffQuery>) -> Response {
    if !safe_rel_path(&q.path) {
        return fail(StatusCode::BAD_REQUEST, "bad path");
    }
    let cwd = match runtime_cwd(&app, &id).await {
        Ok(c) => c,
        Err((code, msg)) => return fail(code, msg),
    };
    let Some(root) = repo_root(&cwd).await else { return fail(StatusCode::NOT_FOUND, "not a git repository") };
    let tracked = git(&root, &["ls-files", "--error-unmatch", "--", &q.path]).await.is_ok();
    let text = if tracked {
        let base: &[&str] = if has_head(&root).await { &["diff", "HEAD"] } else { &["diff", "--cached"] };
        let mut args: Vec<&str> = base.to_vec();
        args.extend(["--no-ext-diff", "--no-textconv", "-U3", "--", &q.path]);
        match git(&root, &args).await {
            Ok((o, cut)) => {
                let mut t = String::from_utf8_lossy(&o).into_owned();
                if cut {
                    t.push_str("\n\\ (the rest of the diff is not shown)\n");
                }
                t
            }
            Err(e) => return fail(StatusCode::BAD_GATEWAY, e),
        }
    } else {
        let (root2, rel) = (root.clone(), q.path.clone());
        tokio::task::spawn_blocking(move || untracked_diff(&root2, &rel)).await.unwrap_or_default()
    };
    ([("content-type", "text/plain; charset=utf-8")], text).into_response()
}

/// Runs a `git` command that changes something; returns (succeeded, stdout and stderr together).
async fn git_write(dir: &Path, args: &[&str], stdin: Option<&str>) -> Result<(bool, String), String> {
    use tokio::io::AsyncWriteExt;
    let mut child = Command::new("git")
        .args(["-c", "core.fsmonitor=false", "-c", "core.pager=cat", "-c", "color.ui=never", "-c", "core.quotepath=off"])
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("could not run git: {e}"))?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let _ = pipe.write_all(text.as_bytes()).await;
    }
    let out = tokio::time::timeout(Duration::from_secs(60), child.wait_with_output()).await.map_err(|_| "git took too long")?.map_err(|e| e.to_string())?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    text.truncate(text.char_indices().nth(8000).map_or(text.len(), |(i, _)| i));
    Ok((out.status.success(), text.trim().to_owned()))
}

#[derive(Deserialize)]
pub struct StageReq {
    paths: Vec<String>,
    stage: bool,
}

pub async fn stage(State(app): State<Arc<AppState>>, UrlPath(id): UrlPath<String>, Json(req): Json<StageReq>) -> Response {
    if req.paths.is_empty() || req.paths.len() > MAX_FILES || !req.paths.iter().all(|p| safe_rel_path(p)) {
        return fail(StatusCode::BAD_REQUEST, "bad path");
    }
    let cwd = match runtime_cwd(&app, &id).await {
        Ok(c) => c,
        Err((code, msg)) => return fail(code, msg),
    };
    let Some(root) = repo_root(&cwd).await else { return fail(StatusCode::NOT_FOUND, "not a git repository") };
    let mut args: Vec<&str> = if req.stage {
        vec!["add", "-A", "--"]
    } else if has_head(&root).await {
        vec!["reset", "-q", "HEAD", "--"]
    } else {
        vec!["rm", "-r", "-q", "--cached", "--"]
    };
    args.extend(req.paths.iter().map(String::as_str));
    match git_write(&root, &args, None).await {
        Ok((true, _)) => StatusCode::NO_CONTENT.into_response(),
        Ok((false, why)) => fail(StatusCode::UNPROCESSABLE_ENTITY, why),
        Err(e) => fail(StatusCode::BAD_GATEWAY, e),
    }
}

#[derive(Deserialize)]
pub struct CommitReq {
    message: String,
}

pub async fn commit(State(app): State<Arc<AppState>>, UrlPath(id): UrlPath<String>, Json(req): Json<CommitReq>) -> Response {
    let message = req.message.trim();
    if message.is_empty() || message.len() > 20_000 {
        return fail(StatusCode::BAD_REQUEST, "bad message");
    }
    let cwd = match runtime_cwd(&app, &id).await {
        Ok(c) => c,
        Err((code, msg)) => return fail(code, msg),
    };
    let Some(root) = repo_root(&cwd).await else { return fail(StatusCode::NOT_FOUND, "not a git repository") };
    // `diff --cached --quiet` exits 0 when the index matches HEAD: nothing to commit.
    if matches!(git_write(&root, &["diff", "--cached", "--quiet"], None).await, Ok((true, _))) {
        return fail(StatusCode::CONFLICT, "nothing is staged");
    }
    match git_write(&root, &["commit", "-F", "-"], Some(message)).await {
        Ok((true, _)) => {
            let hash = git(&root, &["rev-parse", "--short", "HEAD"]).await.ok().map(|(o, _)| String::from_utf8_lossy(&o).trim().to_owned()).unwrap_or_default();
            Json(serde_json::json!({ "hash": hash })).into_response()
        }
        Ok((false, why)) => fail(StatusCode::UNPROCESSABLE_ENTITY, why),
        Err(e) => fail(StatusCode::BAD_GATEWAY, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_that_could_leave_the_repo_are_refused() {
        for bad in ["", "/etc/passwd", "../x", "a/../../b", "a\\..\\b", "x\0y", "\\server\\share"] {
            assert!(!safe_rel_path(bad), "{bad:?}");
        }
        for ok in ["src/main.rs", "한글/파일.txt", "a/..b/c", ".hidden", "a b/c"] {
            assert!(safe_rel_path(ok), "{ok:?}");
        }
    }

    #[test]
    fn porcelain_status_including_renames_and_untracked() {
        let raw = b" M src/a.rs\0A  new.rs\0?? scratch.txt\0R  moved.rs\0old.rs\0UU both.rs\0 D gone.rs\0MM both-changed.rs\0";
        let got = parse_status(raw);
        let view: Vec<(&str, char, bool)> = got.iter().map(|c| (c.path.as_str(), c.status, c.staged)).collect();
        let partial: Vec<&str> = got.iter().filter(|c| c.staged && c.unstaged).map(|c| c.path.as_str()).collect();
        assert_eq!(partial, ["both.rs", "both-changed.rs"], "staged and also changed since");
        assert_eq!(
            view,
            [("src/a.rs", 'M', false), ("new.rs", 'A', true), ("scratch.txt", '?', false), ("moved.rs", 'R', true), ("both.rs", 'U', true), ("gone.rs", 'D', false), ("both-changed.rs", 'M', true)]
        );
    }

    #[test]
    fn numstat_with_binary_files_and_renames() {
        let raw = b"3\t1\tsrc/a.rs\0-\t-\timage.png\x000\t0\t\0old name.rs\0new name.rs\0";
        assert_eq!(
            parse_numstat(raw),
            [("src/a.rs".to_string(), Some(3), Some(1)), ("image.png".to_string(), None, None), ("new name.rs".to_string(), Some(0), Some(0))]
        );
    }

    #[test]
    fn ignored_files_are_marked_and_never_count_as_staged() {
        let got = parse_status(b" M a.rs\0!! target/\0!! .env\0");
        let view: Vec<(&str, char, bool, bool)> = got.iter().map(|c| (c.path.as_str(), c.status, c.staged, c.unstaged)).collect();
        assert_eq!(view, [("a.rs", 'M', false, true), ("target/", '!', false, false), (".env", '!', false, false)]);
    }

    #[test]
    fn the_log_is_read_even_with_odd_characters_in_a_subject() {
        let got = parse_log("abc123\x1f한글 제목, with | pipes\x1f1700000000\0def456\x1fsecond\x1f1699999999".as_bytes());
        assert_eq!(got.len(), 2);
        assert_eq!((got[0].hash.as_str(), got[0].subject.as_str(), got[0].when), ("abc123", "한글 제목, with | pipes", 1_700_000_000));
    }
}
