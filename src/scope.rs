//! `--root`: the one folder (and what is under it) this server will work in.
//!
//! It bounds what pi-oven itself does: browsing, making folders, starting pi, and which sessions are listed, opened,
//! renamed, branched or deleted. pi is a separate program and its tools can still reach outside; see `docs/USAGE.md`.

use std::path::{Component, Path, PathBuf};

/// True when there is no root, or `path` is the root or inside it. A path that does not exist (an old session's
/// folder) is judged as written, and one with `..` in it never passes.
pub fn allows(root: Option<&Path>, path: &Path) -> bool {
    let Some(root) = root else { return true };
    match path.canonicalize() {
        Ok(p) => p.starts_with(root),
        Err(_) => path.is_absolute() && !path.components().any(|c| matches!(c, Component::ParentDir)) && path.starts_with(root),
    }
}

/// The folder a session was started in, from its first line.
pub fn session_cwd(file: &Path) -> Option<PathBuf> {
    use std::io::BufRead;
    let mut line = String::new();
    std::io::BufReader::new(std::fs::File::open(file).ok()?).read_line(&mut line).ok()?;
    let v: serde_json::Value = serde_json::from_str(&line).ok()?;
    v["cwd"].as_str().map(PathBuf::from)
}

/// Whether a session file belongs to the allowed folder.
pub fn allows_session(root: Option<&Path>, file: &Path) -> bool {
    root.is_none() || session_cwd(file).is_some_and(|cwd| allows(root, &cwd))
}

/// A new folder's name: one plain path segment.
pub fn folder_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', '\0'])
        && name.trim() == name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_and_outside() {
        let dir = std::env::temp_dir().join(format!("pi-oven-scope-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        let root = dir.join("a").canonicalize().unwrap();
        let r = Some(root.as_path());
        assert!(allows(None, Path::new("/etc")));
        assert!(allows(r, &root));
        assert!(allows(r, &root.join("b")));
        assert!(!allows(r, &dir));
        assert!(!allows(r, &root.join("b/../..")));
        // gone, but written inside
        assert!(allows(r, &root.join("old")));
        assert!(!allows(r, &root.join("../x")));
        // a link that leads out does not count as inside
        std::os::unix::fs::symlink(&dir, root.join("out")).unwrap();
        assert!(!allows(r, &root.join("out")));
        std::fs::remove_file(root.join("out")).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn folder_names() {
        for ok in ["new", "새 폴더", ".config", "a.b"] {
            assert!(folder_name_ok(ok), "{ok}");
        }
        for bad in ["", ".", "..", "a/b", "a\\b", " x", "x ", "a\0b"] {
            assert!(!folder_name_ok(bad), "{bad:?}");
        }
    }
}
