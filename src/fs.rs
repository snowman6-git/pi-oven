use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<DirEntry>,
    pub is_git: bool,
}

pub fn list_dirs(path: &Path, hidden: bool) -> std::io::Result<Listing> {
    let path: PathBuf = path.canonicalize()?;
    let mut entries: Vec<DirEntry> = std::fs::read_dir(&path)?
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (hidden || !name.starts_with('.')).then(|| DirEntry { path: e.path().to_string_lossy().into_owned(), name })
        })
        .collect();
    entries.sort_by_key(|e| e.name.to_lowercase());
    Ok(Listing {
        parent: path.parent().map(|p| p.to_string_lossy().into_owned()),
        is_git: path.join(".git").exists(),
        path: path.to_string_lossy().into_owned(),
        entries,
    })
}
