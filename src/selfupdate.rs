//! pi-oven updating itself from the newest GitHub release.
//!
//! Releases keep their version and get their files replaced, so "newer" is decided by content: the release's server
//! binary against the one running now, by SHA-256. A download is trusted only when the archive matches the release's
//! `SHA256SUMS`. The binary is replaced in place (temporary file, then rename), and the server starts itself again with
//! the same arguments (`exec`: same process, same environment), after closing every pi it runs.
//!
//! Downloads use the system's `curl` and `tar`, so the server needs no HTTP or archive code of its own.

use axum::{extract::State, http::StatusCode, response::{IntoResponse, Response}, Json};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex, OnceLock},
};
use tokio::process::Command;

use crate::{api::fail, AppState};

pub const REPO: &str = "snowman6-git/pi-oven";
const MAX_NOTES: usize = 40_000;

/// The running binary's path and hash, taken at start: once the file is replaced, /proc/self/exe no longer says.
static EXE: OnceLock<Option<(PathBuf, String)>> = OnceLock::new();

pub fn init() {
    EXE.get_or_init(|| {
        let p = std::env::current_exe().ok()?.canonicalize().ok()?;
        let h = sha256_file(&p).ok()?;
        Some((p, h))
    });
}

fn exe() -> Option<&'static (PathBuf, String)> {
    EXE.get().and_then(Option::as_ref)
}

/// The first 12 hex digits of the running binary's hash (for the page and `--version`-like checks).
pub fn build_id() -> Option<String> {
    exe().map(|(_, h)| h[..12].to_owned())
}

pub fn sha256_file(p: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(p)?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h)?;
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// The release file for this machine, by the end of its name.
fn asset_suffix() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("-linux-x86_64.tar.gz"),
        ("linux", "aarch64") => Some("-linux-aarch64.tar.gz"),
        _ => None,
    }
}

/// `SHA256SUMS` → the hash written for `name`.
pub fn sum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let (h, n) = (it.next()?, it.next()?);
        (n.trim_start_matches('*') == name && h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())).then(|| h.to_ascii_lowercase())
    })
}

async fn curl(url: &str, out: Option<&Path>) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new("curl");
    cmd.args(["-fsSL", "--proto", "=https", "--max-time", "180", "-H", "User-Agent: pi-oven", "-H", "Accept: application/vnd.github+json"]);
    if let Some(o) = out {
        cmd.arg("-o").arg(o);
    }
    cmd.arg(url).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let o = cmd.output().await.map_err(|e| format!("curl: {e}"))?;
    if !o.status.success() {
        return Err(format!("{url}: {}", String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(o.stdout)
}

/// A release binary downloaded, checked and unpacked, ready to put in place.
#[derive(Clone)]
pub struct Staged {
    pub tag: String,
    pub hash: String,
    pub updated_at: String,
    pub bin: PathBuf,
    /// The release notes (Markdown), as written on GitHub.
    pub notes: String,
    /// The archive's hash from SHA256SUMS (what the light check compares).
    pub archive: String,
}

fn work_dir() -> PathBuf {
    crate::config::cache_dir().join("update")
}

/// Download the newest release's server file for this machine, check it against `SHA256SUMS`, unpack it.
pub async fn fetch_latest() -> Result<Staged, String> {
    let suffix = asset_suffix().ok_or_else(|| tr!("이 운영체제용 릴리즈 파일이 없습니다", "there is no release file for this system").to_string())?;
    let rel: serde_json::Value = serde_json::from_slice(&curl(&format!("https://api.github.com/repos/{REPO}/releases/latest"), None).await?)
        .map_err(|e| format!("release: {e}"))?;
    let tag = rel["tag_name"].as_str().unwrap_or("").to_owned();
    let assets = rel["assets"].as_array().cloned().unwrap_or_default();
    let find = |pred: &dyn Fn(&str) -> bool| assets.iter().find(|a| a["name"].as_str().is_some_and(pred)).cloned();
    let server = find(&|n| n.starts_with("server-pi-oven-") && n.ends_with(suffix)).ok_or_else(|| tr!("릴리즈에 서버 파일이 없습니다", "the release has no server file").to_string())?;
    let sums = find(&|n| n == "SHA256SUMS").ok_or_else(|| tr!("릴리즈에 SHA256SUMS가 없습니다", "the release has no SHA256SUMS").to_string())?;
    let name = server["name"].as_str().unwrap_or_default().to_owned();
    let url = |a: &serde_json::Value| a["browser_download_url"].as_str().unwrap_or_default().to_owned();
    if !url(&server).starts_with("https://") || !url(&sums).starts_with("https://") {
        return Err("unexpected download address".into());
    }

    let sums_text = String::from_utf8_lossy(&curl(&url(&sums), None).await?).into_owned();
    let expected = sum_for(&sums_text, &name).ok_or_else(|| tr!("SHA256SUMS에 서버 파일의 해시가 없습니다", "SHA256SUMS does not list the server file").to_string())?;

    let dir = work_dir();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let archive = dir.join("server.tar.gz");
    curl(&url(&server), Some(&archive)).await?;
    let got = sha256_file(&archive).map_err(|e| e.to_string())?;
    if got != expected {
        let _ = std::fs::remove_file(&archive);
        return Err(tr!("내려받은 파일의 해시가 SHA256SUMS와 다릅니다", "the downloaded file does not match SHA256SUMS").to_string());
    }
    let out = dir.join("x");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let st = Command::new("tar").arg("-xzf").arg(&archive).arg("-C").arg(&out).arg("--no-same-owner").status().await.map_err(|e| format!("tar: {e}"))?;
    if !st.success() {
        return Err("tar failed".into());
    }
    let bin = out.join("pi-oven-server").join("pi-oven");
    let meta = std::fs::symlink_metadata(&bin).map_err(|_| tr!("압축 안에 pi-oven이 없습니다", "pi-oven is not in the archive").to_string())?;
    if !meta.is_file() {
        return Err("pi-oven in the archive is not a file".into());
    }
    let hash = sha256_file(&bin).map_err(|e| e.to_string())?;
    let notes: String = rel["body"].as_str().unwrap_or("").chars().take(MAX_NOTES).collect();
    Ok(Staged { tag, hash, updated_at: server["updated_at"].as_str().unwrap_or("").to_owned(), bin, notes, archive: expected })
}

/// Put `new` where the running binary is: a temporary file next to it, the old file's mode, then rename.
pub fn replace_exe(new: &Path) -> Result<PathBuf, String> {
    let (exe, _) = exe().ok_or("cannot tell where pi-oven runs from")?;
    let dir = exe.parent().ok_or("no folder")?;
    let tmp = dir.join(format!(".pi-oven.update-{}", std::process::id()));
    let res = (|| -> std::io::Result<()> {
        std::fs::copy(new, &tmp)?;
        let mode = std::fs::metadata(exe)?.permissions();
        std::fs::set_permissions(&tmp, mode)?;
        std::fs::File::open(&tmp)?.sync_all()?;
        std::fs::rename(&tmp, exe)
    })();
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(match e.kind() {
            std::io::ErrorKind::PermissionDenied => tr!(format!("{}에 쓸 권한이 없습니다", dir.display()), format!("no permission to write to {}", dir.display())),
            _ => e.to_string(),
        });
    }
    Ok(exe.clone())
}

/// Start again as the (new) binary, with the arguments and environment this one was started with.
#[cfg(unix)]
pub fn reexec() -> std::io::Error {
    use std::os::unix::process::CommandExt;
    let Some((exe, _)) = exe() else { return std::io::Error::other("unknown executable") };
    std::process::Command::new(exe).args(std::env::args_os().skip(1)).exec()
}

pub struct SelfUpdate {
    busy: Arc<tokio::sync::Mutex<()>>,
    staged: Mutex<Option<Staged>>,
    /// The background check (on unless turned off in Settings → About).
    auto: std::sync::atomic::AtomicBool,
    /// What the last check found, kept in a file so a restart need not download again.
    known: Mutex<Option<Known>>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Known {
    pub tag: String,
    /// The archive's hash in SHA256SUMS, and the hash of the binary inside it.
    pub archive: String,
    pub binary: String,
    pub checked_at: u64,
}

impl Default for SelfUpdate {
    fn default() -> Self {
        let dir = crate::config::cache_dir();
        let auto = std::fs::read_to_string(dir.join("update-check")).map(|v| v.trim() != "off").unwrap_or(true);
        let known = std::fs::read(dir.join("update-known.json")).ok().and_then(|b| serde_json::from_slice(&b).ok());
        Self { busy: Default::default(), staged: Default::default(), auto: auto.into(), known: Mutex::new(known) }
    }
}

impl SelfUpdate {
    fn remember(&self, s: &Staged) {
        let k = Known { tag: s.tag.clone(), archive: s.archive.clone(), binary: s.hash.clone(), checked_at: crate::runtime::now_ms() };
        let dir = crate::config::cache_dir();
        if std::fs::create_dir_all(&dir).is_ok() {
            let _ = std::fs::write(dir.join("update-known.json"), serde_json::to_vec(&k).unwrap_or_default());
        }
        *self.known.lock().unwrap() = Some(k);
    }

    /// Whether the newest known release differs from the running binary.
    pub fn status(&self) -> serde_json::Value {
        let k = self.known.lock().unwrap().clone();
        let current = exe().map(|(_, h)| h.clone());
        serde_json::json!({
            "auto": self.auto.load(std::sync::atomic::Ordering::Relaxed),
            "available": match (&k, &current) { (Some(k), Some(c)) => k.binary != *c, _ => false },
            "tag": k.as_ref().map(|k| k.tag.clone()),
            "latest": k.as_ref().map(|k| k.binary[..12].to_owned()),
            "current": current.map(|c| c[..12].to_owned()),
            "checkedAt": k.as_ref().map(|k| k.checked_at),
        })
    }

    /// The light check: only SHA256SUMS, unless the archive changed since last time (then a full, checked download).
    async fn light_check(&self) -> Result<(), String> {
        let Ok(_one) = self.busy.try_lock() else { return Ok(()) };
        let suffix = asset_suffix().ok_or("no release file for this system")?;
        let rel: serde_json::Value = serde_json::from_slice(&curl(&format!("https://api.github.com/repos/{REPO}/releases/latest"), None).await?).map_err(|e| e.to_string())?;
        let assets = rel["assets"].as_array().cloned().unwrap_or_default();
        let name_of = |pred: &dyn Fn(&str) -> bool| assets.iter().find(|a| a["name"].as_str().is_some_and(pred)).cloned();
        let server = name_of(&|n| n.starts_with("server-pi-oven-") && n.ends_with(suffix)).ok_or("no server file")?;
        let sums = name_of(&|n| n == "SHA256SUMS").ok_or("no SHA256SUMS")?;
        let sums_url = sums["browser_download_url"].as_str().unwrap_or_default();
        if !sums_url.starts_with("https://") {
            return Err("unexpected download address".into());
        }
        let text = String::from_utf8_lossy(&curl(sums_url, None).await?).into_owned();
        let archive = sum_for(&text, server["name"].as_str().unwrap_or_default()).ok_or("SHA256SUMS does not list the server file")?;
        let same = self.known.lock().unwrap().as_ref().is_some_and(|k| k.archive == archive);
        if same {
            if let Some(k) = self.known.lock().unwrap().as_mut() {
                k.checked_at = crate::runtime::now_ms();
            }
            return Ok(());
        }
        let s = fetch_latest().await?;
        self.remember(&s);
        *self.staged.lock().unwrap() = Some(s);
        Ok(())
    }

    /// Runs for the life of the server: shortly after start, then every 6 hours.
    pub async fn watch(self: Arc<Self>) {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        loop {
            if self.auto.load(std::sync::atomic::Ordering::Relaxed) {
                if let Err(e) = self.light_check().await {
                    tracing::debug!("update check: {e}");
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
        }
    }
}

pub async fn status(State(app): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(app.self_update.status())
}

#[derive(serde::Deserialize)]
pub struct SetAuto {
    enabled: bool,
}

pub async fn set_auto(State(app): State<Arc<AppState>>, Json(req): Json<SetAuto>) -> Json<serde_json::Value> {
    app.self_update.auto.store(req.enabled, std::sync::atomic::Ordering::Relaxed);
    let dir = crate::config::cache_dir();
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join("update-check"), if req.enabled { "on" } else { "off" });
    }
    Json(app.self_update.status())
}

fn view(s: &Staged) -> serde_json::Value {
    let current = exe().map(|(_, h)| h.clone()).unwrap_or_default();
    serde_json::json!({
        "tag": s.tag,
        "current": current.get(..12),
        "latest": &s.hash[..12],
        "upToDate": current == s.hash,
        "updatedAt": s.updated_at,
        "notes": s.notes,
    })
}

/// GET: download and check the newest release (nothing is replaced).
pub async fn check(State(app): State<Arc<AppState>>) -> Response {
    let u = &app.self_update;
    let Ok(_one) = u.busy.try_lock() else { return fail(StatusCode::CONFLICT, tr!("이미 확인하거나 업데이트하는 중입니다", "already checking or updating")) };
    match fetch_latest().await {
        Ok(s) => {
            let v = view(&s);
            u.remember(&s);
            *u.staged.lock().unwrap() = Some(s);
            Json(v).into_response()
        }
        Err(e) => fail(StatusCode::BAD_GATEWAY, e),
    }
}

/// POST: put the checked release in place and restart with the same arguments.
pub async fn apply(State(app): State<Arc<AppState>>) -> Response {
    let u = app.self_update.clone();
    let Ok(one) = u.busy.clone().try_lock_owned() else { return fail(StatusCode::CONFLICT, tr!("이미 확인하거나 업데이트하는 중입니다", "already checking or updating")) };
    let staged = u.staged.lock().unwrap().clone().filter(|s| s.bin.is_file());
    let s = match staged {
        Some(s) => s,
        None => match fetch_latest().await {
            Ok(s) => s,
            Err(e) => return fail(StatusCode::BAD_GATEWAY, e),
        },
    };
    if exe().is_some_and(|(_, h)| *h == s.hash) {
        return fail(StatusCode::CONFLICT, tr!("이미 최신입니다", "already up to date"));
    }
    if let Err(e) = replace_exe(&s.bin) {
        return fail(StatusCode::INTERNAL_SERVER_ERROR, e);
    }
    tracing::info!(tag = %s.tag, hash = %&s.hash[..12], "pi-oven updated; restarting");
    let runtimes = app.runtimes.clone();
    tokio::spawn(async move {
        let _one = one;
        // let the answer reach the page first
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        runtimes.close_all().await;
        #[cfg(unix)]
        {
            let e = reexec();
            tracing::error!("could not restart: {e}; the new version runs after a manual restart");
        }
    });
    Json(serde_json::json!({ "restarting": cfg!(unix), "latest": &s.hash[..12] })).into_response()
}

/// `pi-oven update [--check]`: the same from a terminal. The running server (if any) is not restarted.
pub async fn cli(args: &[String]) -> i32 {
    init();
    let check_only = args.iter().any(|a| a == "--check");
    eprintln!("{}", tr!(format!("{REPO}의 최신 릴리즈를 확인합니다…"), format!("checking the newest release of {REPO}…")));
    let s = match fetch_latest().await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("pi-oven: {e}");
            return 1;
        }
    };
    let current = exe().map(|(_, h)| h.clone()).unwrap_or_default();
    println!("{}  {} {}", s.tag, tr!("지금", "current"), current.get(..12).unwrap_or("?"));
    println!("{}  {} {}", s.tag, tr!("최신", "latest "), &s.hash[..12]);
    if current == s.hash {
        println!("{}", tr!("이미 최신입니다.", "Already up to date."));
        return 0;
    }
    if check_only {
        if !s.notes.trim().is_empty() {
            println!();
            for l in s.notes.lines().take(20) {
                println!("  {l}");
            }
            println!("  …  https://github.com/{REPO}/releases/tag/{}", s.tag);
            println!();
        }
        println!("{}", tr!("업데이트가 있습니다. `pi-oven update`로 설치합니다.", "An update is available. Install it with `pi-oven update`."));
        return 0;
    }
    match replace_exe(&s.bin) {
        Ok(p) => {
            println!("{}", tr!(format!("{}를 바꿨습니다. 실행 중인 서버는 다시 시작해야 적용됩니다.", p.display()), format!("Replaced {}. A running server picks it up when restarted.", p.display())));
            0
        }
        Err(e) => {
            eprintln!("pi-oven: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_right_line_in_sha256sums() {
        let sums = "aaaa  other.apk\n6d373794f094ec3dc1f83db6ec2a5be847bf9337a43d961e509b4cabd8b734da  server-pi-oven-0.1.0-linux-x86_64.tar.gz\n";
        assert_eq!(sum_for(sums, "server-pi-oven-0.1.0-linux-x86_64.tar.gz").as_deref(), Some("6d373794f094ec3dc1f83db6ec2a5be847bf9337a43d961e509b4cabd8b734da"));
        assert_eq!(sum_for(sums, "other.apk"), None); // not a real hash
        assert_eq!(sum_for(sums, "missing"), None);
    }

    #[test]
    fn hashes_files() {
        let p = std::env::temp_dir().join(format!("oven-sha-{}", std::process::id()));
        std::fs::write(&p, "abc").unwrap();
        assert_eq!(sha256_file(&p).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        std::fs::remove_file(&p).unwrap();
    }
}
