//! What changes in a session's folder, told to its pages as it happens: the file tree marks new and changed files and
//! the live view follows them, also those `bash`, a subagent or another program wrote (pi tells only of its own tools).
//!
//! One watcher per folder, shared by the pages that look at it and gone with the last of them. `.git` and what git
//! ignores are left out, as are the usual heaps of made files (`node_modules`, `target`); the number of folders is
//! capped, the ones nearest the top first. Only names go out, never contents, and a name only when the file's size or
//! time really changed. A temp file that is gone again by the time it is looked at is never told of.

use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{mpsc, Arc, Mutex, Weak},
    time::{Duration, Instant, SystemTime},
};

use notify::{EventKind, RecursiveMode, Watcher};
use tokio::sync::broadcast;

const MAX_DIRS: usize = 4000;
/// Without git there is no saying what is made and what is written: less of the folder is watched.
const MAX_DIRS_PLAIN: usize = 1500;
const MAX_KNOWN: usize = 60_000;
/// Files named in one message; with more than this the page is told to look for itself.
const MAX_TOLD: usize = 120;
const SKIP: [&str; 14] =
    [".git", "node_modules", "target", ".venv", "venv", "__pycache__", ".cache", ".next", ".svelte-kit", ".gradle", ".pytest_cache", ".mypy_cache", ".terraform", ".tox"];

#[derive(Default)]
pub struct Watches {
    hubs: Mutex<HashMap<PathBuf, Weak<Hub>>>,
}

pub struct Hub {
    out: broadcast::Sender<Arc<str>>,
    stop: mpsc::Sender<Msg>,
}

impl Drop for Hub {
    fn drop(&mut self) {
        let _ = self.stop.send(Msg::Stop);
    }
}

impl Hub {
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<str>> {
        self.out.subscribe()
    }
}

enum Msg {
    Fs(notify::Event),
    Stop,
}

impl Watches {
    /// The watcher of a folder, as long as the returned handle is kept.
    pub fn of(&self, cwd: &Path) -> Option<Arc<Hub>> {
        let root = cwd.canonicalize().ok().filter(|p| p.is_dir())?;
        let mut hubs = self.hubs.lock().unwrap();
        hubs.retain(|_, w| w.strong_count() > 0);
        if let Some(h) = hubs.get(&root).and_then(Weak::upgrade) {
            return Some(h);
        }
        let (tx, rx) = mpsc::channel();
        let (out, _) = broadcast::channel(64);
        let hub = Arc::new(Hub { out: out.clone(), stop: tx.clone() });
        let at = root.clone();
        std::thread::Builder::new().name("watch".into()).spawn(move || run(at, tx, rx, out)).ok()?;
        hubs.insert(root, Arc::downgrade(&hub));
        Some(hub)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    New,
    Mod,
    Gone,
}

type Stamp = (Option<SystemTime>, u64);

struct Tree {
    root: PathBuf,
    git: bool,
    /// Folders watched, and files known (by size and time), as absolute paths.
    dirs: HashSet<PathBuf>,
    files: HashMap<PathBuf, Stamp>,
    /// The system would take no more.
    full: bool,
}

fn run(root: PathBuf, tx: mpsc::Sender<Msg>, rx: mpsc::Receiver<Msg>, out: broadcast::Sender<Arc<str>>) {
    let w = notify::recommended_watcher(move |ev: notify::Result<notify::Event>| {
        if let Ok(ev) = ev {
            let _ = tx.send(Msg::Fs(ev));
        }
    });
    let mut w = match w {
        Ok(w) => w,
        Err(e) => return tracing::debug!("cannot watch {}: {e}", root.display()),
    };
    let mut tree = Tree { git: is_git(&root), root: root.clone(), dirs: HashSet::new(), files: HashMap::new(), full: false };
    // one watch for everything where the system has such a thing; a watch per folder on Linux
    if !cfg!(target_os = "linux") && w.watch(&root, RecursiveMode::Recursive).is_err() {
        return;
    }
    tree.take(&mut w, vec![root]);

    let mut window = Duration::from_millis(120);
    loop {
        let mut touched: BTreeSet<PathBuf> = BTreeSet::new();
        let mut lost = false;
        let mut note = |m: Msg| match m {
            Msg::Stop => false,
            Msg::Fs(ev) => {
                lost |= ev.need_rescan();
                // a file read or its mode changed is no news; a size or time that did not change is dropped below
                if !matches!(ev.kind, EventKind::Access(_)) && touched.len() < 5000 {
                    touched.extend(ev.paths);
                }
                true
            }
        };
        if !rx.recv().is_ok_and(&mut note) {
            return;
        }
        let until = Instant::now() + window;
        loop {
            match rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
                Ok(m) => {
                    if !note(m) {
                        return;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(_) => return,
            }
        }
        if lost {
            let roots = vec![tree.root.clone()];
            tree.files.clear();
            tree.dirs.clear();
            tree.full = false;
            tree.take(&mut w, roots);
        }
        let (told, more) = tree.look(&mut w, touched);
        if !told.is_empty() || lost {
            let _ = out.send(message(&told, more || lost).into());
        }
        // a build or an install writes without end: told of less often
        window = Duration::from_millis(if told.len() > 40 || more { 700 } else { 120 });
    }
}

fn message(told: &[(String, Kind, bool)], more: bool) -> String {
    let files: Vec<serde_json::Value> = told
        .iter()
        .map(|(p, k, dir)| {
            let k = match k {
                Kind::New => "new",
                Kind::Mod => "mod",
                Kind::Gone => "gone",
            };
            serde_json::json!({ "p": p, "k": k, "dir": dir })
        })
        .collect();
    // `type` first: the page's and the server's readers look for it there
    format!(r#"{{"type":"oven_files","files":{},"more":{more}}}"#, serde_json::Value::Array(files))
}

impl Tree {
    fn cap(&self) -> usize {
        if self.git { MAX_DIRS } else { MAX_DIRS_PLAIN }
    }

    /// A folder never looked into, by its name.
    fn skips(&self, name: &str) -> bool {
        SKIP.contains(&name) || (!self.git && name.starts_with('.'))
    }

    fn rel(&self, p: &Path) -> Option<String> {
        let r = p.strip_prefix(&self.root).ok()?;
        let mut parts = Vec::new();
        for c in r.components() {
            parts.push(c.as_os_str().to_str()?);
        }
        (!parts.is_empty()).then(|| parts.join("/"))
    }

    /// Inside a folder that is not looked into (the parts above the last name).
    fn outside(&self, rel: &str) -> bool {
        let mut parts: Vec<&str> = rel.split('/').collect();
        parts.pop();
        parts.iter().any(|n| self.skips(n))
    }

    /// Watches these folders and all below, nearest first. Returns the files met that were not known.
    fn take(&mut self, w: &mut dyn Watcher, roots: Vec<PathBuf>) -> Vec<PathBuf> {
        let mut met = Vec::new();
        let mut level: VecDeque<PathBuf> = roots.into();
        while !level.is_empty() {
            let mut below = Vec::new();
            for d in level.drain(..) {
                if self.full || self.dirs.len() >= self.cap() {
                    return met;
                }
                if self.dirs.contains(&d) {
                    continue;
                }
                if cfg!(target_os = "linux") && w.watch(&d, RecursiveMode::NonRecursive).is_err() {
                    // gone meanwhile, or the system's limit: only the second is for good
                    if d.is_dir() {
                        self.full = true;
                        return met;
                    }
                    continue;
                }
                self.dirs.insert(d.clone());
                let Ok(list) = fs::read_dir(&d) else { continue };
                for e in list.flatten() {
                    let Ok(m) = e.metadata() else { continue }; // (of the link itself)
                    if m.is_dir() {
                        if !self.skips(&e.file_name().to_string_lossy()) {
                            below.push(e.path());
                        }
                    } else if self.files.len() < MAX_KNOWN && self.files.insert(e.path(), stamp(&m)).is_none() {
                        met.push(e.path());
                    }
                }
            }
            let ignored = self.ignored(&below);
            below.retain(|d| !ignored.contains(d));
            level = below.into();
        }
        met
    }

    /// What became of these paths.
    fn look(&mut self, w: &mut dyn Watcher, touched: BTreeSet<PathBuf>) -> (Vec<(String, Kind, bool)>, bool) {
        let mut found: Vec<(PathBuf, Kind, bool)> = Vec::new();
        let mut fresh = Vec::new();
        for p in touched {
            let Some(rel) = self.rel(&p) else { continue };
            if self.outside(&rel) {
                continue;
            }
            match fs::symlink_metadata(&p) {
                Ok(m) if m.is_dir() => {
                    if !self.dirs.contains(&p) && !self.skips(&p.file_name().unwrap_or_default().to_string_lossy()) {
                        fresh.push(p);
                    }
                }
                Ok(m) => {
                    let s = stamp(&m);
                    if self.files.contains_key(&p) {
                        if self.files.insert(p.clone(), s) != Some(s) {
                            found.push((p, Kind::Mod, false));
                        }
                    } else {
                        if self.files.len() < MAX_KNOWN {
                            self.files.insert(p.clone(), s);
                        }
                        found.push((p, Kind::New, false));
                    }
                }
                Err(_) => {
                    if self.files.remove(&p).is_some() {
                        found.push((p, Kind::Gone, false));
                    } else if self.dirs.remove(&p) {
                        self.dirs.retain(|d| !d.starts_with(&p));
                        self.files.retain(|f, _| !f.starts_with(&p));
                        found.push((p, Kind::Gone, true));
                    }
                }
            }
        }
        if !fresh.is_empty() {
            let ignored = self.ignored(&fresh);
            fresh.retain(|d| !ignored.contains(d));
            found.extend(fresh.iter().map(|d| (d.clone(), Kind::New, true)));
            // what was put in before it was watched
            found.extend(self.take(w, fresh).into_iter().map(|f| (f, Kind::New, false)));
        }
        let names: Vec<PathBuf> = found.iter().filter(|f| f.1 != Kind::Gone && !f.2).map(|f| f.0.clone()).collect();
        let ignored = self.ignored(&names);
        found.retain(|f| !ignored.contains(&f.0));
        let more = found.len() > MAX_TOLD;
        found.truncate(MAX_TOLD);
        (found.into_iter().filter_map(|(p, k, d)| self.rel(&p).map(|r| (r, k, d))).collect(), more)
    }

    /// Those of the paths git ignores. (`check-ignore` reads the ignore rules and runs nothing.)
    fn ignored(&self, paths: &[PathBuf]) -> HashSet<PathBuf> {
        if !self.git || paths.is_empty() {
            return HashSet::new();
        }
        let mut input = Vec::new();
        for p in paths {
            if let Some(r) = self.rel(p) {
                input.extend_from_slice(r.as_bytes());
                input.push(0);
            }
        }
        let Ok(mut child) = git(&self.root).args(["check-ignore", "--stdin", "-z"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn() else {
            return HashSet::new();
        };
        // written from aside: git answers as it reads, and both pipes full would wait on each other for good
        let sink = child.stdin.take();
        let Ok(feed) = std::thread::Builder::new().name("watch-git".into()).spawn(move || sink.map(|mut s| s.write_all(&input))) else {
            let _ = child.kill();
            let _ = child.wait();
            return HashSet::new();
        };
        let mut out = Vec::new();
        if let Some(mut o) = child.stdout.take() {
            let _ = o.read_to_end(&mut out);
        }
        let _ = feed.join();
        let _ = child.wait();
        out.split(|b| *b == 0).filter(|s| !s.is_empty()).filter_map(|s| std::str::from_utf8(s).ok()).map(|s| self.root.join(s)).collect()
    }
}

fn git(dir: &Path) -> Command {
    let mut c = Command::new("git");
    c.args(["-c", "core.fsmonitor=false", "-c", "core.quotepath=off"])
        .current_dir(dir)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}

fn is_git(dir: &Path) -> bool {
    git(dir).args(["rev-parse", "--is-inside-work-tree"]).status().is_ok_and(|s| s.success())
}

fn stamp(m: &fs::Metadata) -> Stamp {
    (m.modified().ok(), m.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oven-watch-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    /// What is told until `n` files were (or nothing came for a while).
    fn told(rx: &mut broadcast::Receiver<Arc<str>>, n: usize) -> Vec<String> {
        let mut out = Vec::new();
        let end = Instant::now() + Duration::from_secs(4);
        while out.len() < n && Instant::now() < end {
            match rx.try_recv() {
                Ok(m) => {
                    let v: serde_json::Value = serde_json::from_str(&m).unwrap();
                    assert!(m.starts_with(r#"{"type":"oven_files""#));
                    for f in v["files"].as_array().unwrap() {
                        out.push(format!("{} {}{}", f["k"].as_str().unwrap(), f["p"].as_str().unwrap(), if f["dir"] == true { "/" } else { "" }));
                    }
                }
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        out.sort();
        out
    }
    fn none(rx: &mut broadcast::Receiver<Arc<str>>) {
        std::thread::sleep(Duration::from_millis(600));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn tells_what_is_new_changed_and_gone_but_not_what_git_ignores() {
        let d = folder("git");
        if !git(&d).args(["init", "-q"]).status().is_ok_and(|s| s.success()) {
            return; // no git here
        }
        fs::write(d.join(".gitignore"), "out/\n*.log\n").unwrap();
        fs::create_dir_all(d.join("src")).unwrap();
        fs::create_dir_all(d.join("out")).unwrap();
        fs::write(d.join("src/a.rs"), "a").unwrap();
        let w = Watches::default();
        let hub = w.of(&d).unwrap();
        let mut rx = hub.subscribe();
        std::thread::sleep(Duration::from_millis(400));

        fs::write(d.join("src/a.rs"), "ab").unwrap();
        fs::write(d.join("src/b.rs"), "b").unwrap();
        fs::write(d.join("x.log"), "x").unwrap();
        fs::write(d.join("out/o.js"), "o").unwrap();
        fs::write(d.join(".git/z"), "z").unwrap();
        assert_eq!(told(&mut rx, 2), ["mod src/a.rs", "new src/b.rs"]);

        // saved the way an editor does: a temp file moved over it is a change, and the temp file is not news
        fs::write(d.join("src/.a.tmp"), "abc").unwrap();
        fs::rename(d.join("src/.a.tmp"), d.join("src/a.rs")).unwrap();
        assert_eq!(told(&mut rx, 1), ["mod src/a.rs"]);

        // a folder with something already in it, then more in it
        fs::create_dir_all(d.join("new/deep")).unwrap();
        fs::write(d.join("new/deep/1.txt"), "1").unwrap();
        let got = told(&mut rx, 3);
        assert!(got.contains(&"new new/".to_owned()) && got.contains(&"new new/deep/1.txt".to_owned()), "{got:?}");
        fs::write(d.join("new/deep/2.txt"), "2").unwrap();
        assert_eq!(told(&mut rx, 1), ["new new/deep/2.txt"]);

        fs::remove_file(d.join("src/b.rs")).unwrap();
        assert_eq!(told(&mut rx, 1), ["gone src/b.rs"]);
        let _ = fs::read_to_string(d.join("src/a.rs"));
        none(&mut rx);

        // two pages, one watcher; gone with the last
        assert!(Arc::ptr_eq(&hub, &w.of(&d).unwrap()));
        drop(hub);
        assert!(w.hubs.lock().unwrap().values().all(|h| h.strong_count() == 0));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_folder_without_git_leaves_out_hidden_and_made_folders() {
        let d = folder("plain");
        for x in [".hid", "node_modules/p", "a"] {
            fs::create_dir_all(d.join(x)).unwrap();
        }
        let hub = Watches::default().of(&d).unwrap();
        let mut rx = hub.subscribe();
        std::thread::sleep(Duration::from_millis(400));
        fs::write(d.join(".hid/x"), "").unwrap();
        fs::write(d.join("node_modules/p/x"), "").unwrap();
        none(&mut rx);
        fs::write(d.join("a/x"), "").unwrap();
        fs::write(d.join(".env"), "").unwrap();
        assert_eq!(told(&mut rx, 2), ["new .env", "new a/x"]);
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_flood_is_cut_short() {
        let d = folder("flood");
        let hub = Watches::default().of(&d).unwrap();
        let mut rx = hub.subscribe();
        std::thread::sleep(Duration::from_millis(300));
        for i in 0..400 {
            fs::write(d.join(format!("f{i}")), "x").unwrap();
        }
        std::thread::sleep(Duration::from_millis(900));
        let v: serde_json::Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert!(v["files"].as_array().unwrap().len() <= MAX_TOLD && v["more"] == true);
        fs::remove_dir_all(&d).unwrap();
    }
}
