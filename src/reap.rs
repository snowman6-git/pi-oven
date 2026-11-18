//! What a self-update leaves behind. `pi-oven update` (and the update card) replace the binary and start it again in
//! the same process (`exec`), so the processes the old one started (a session's pi, a sign-in's `script`) are still
//! this process's children, but the new one holds nothing to wait on them with: when they end they stay as zombies, and
//! the temporary folders of an unfinished sign-in or token refresh stay on disk.
//!
//! At start, before anything is spawned, the children already there are those inherited ones: they are waited for by
//! their own pids (never "any child", which would take exits tokio is waiting for). Temporary folders older than an hour
//! are removed.

use std::time::Duration;

/// The children this process has right now (Linux: `/proc/self/task/*/children`).
#[cfg(target_os = "linux")]
fn children() -> Vec<i32> {
    let Ok(tasks) = std::fs::read_dir("/proc/self/task") else { return vec![] };
    let mut out = vec![];
    for t in tasks.flatten() {
        if let Ok(s) = std::fs::read_to_string(t.path().join("children")) {
            out.extend(s.split_whitespace().filter_map(|p| p.parse::<i32>().ok()));
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Call first thing in `main`: wait for the inherited children in the background, and clear old temporary folders.
pub fn start() {
    #[cfg(target_os = "linux")]
    {
        let mut pids = children();
        if !pids.is_empty() {
            tracing::info!(count = pids.len(), "children from before an update: waiting for them");
            std::thread::spawn(move || loop {
                pids.retain(|&p| {
                    let mut status = 0;
                    // SAFETY: waitpid on a pid of our own child, without blocking; it only reads the status out.
                    let r = unsafe { libc::waitpid(p, &mut status, libc::WNOHANG) };
                    r == 0 // still running
                });
                if pids.is_empty() {
                    return;
                }
                std::thread::sleep(Duration::from_secs(30));
            });
        }
    }
    clean_temp();
}

/// Remove sign-in and token-refresh folders (`login-*`, `refresh-*` in pi-oven's cache) older than an hour.
fn clean_temp() {
    let dir = crate::config::cache_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("login-") || name.starts_with("refresh-")) {
            continue;
        }
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|d| d > Duration::from_secs(3600));
        let is_dir = e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink());
        if old && is_dir {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}
