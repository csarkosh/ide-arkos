//! The folders opened recently, newest first, for the launch chooser (`launch.rs`). Kept in
//! `~/Library/Application Support/arkos/recent.json`; every project window notes its folder.
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// How many folders the file keeps.
pub const CAP: usize = 50;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recent {
    pub path: PathBuf,
    pub opened_unix: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RecentFile {
    folders: Vec<Recent>,
}

/// The list in `file`, newest first. A missing or unreadable file is an empty list.
pub fn load(file: &Path) -> Vec<Recent> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            eprintln!("arkos: cannot read {}: {e}", file.display());
            return Vec::new();
        }
    };
    match serde_json::from_str::<RecentFile>(&text) {
        Ok(f) => f.folders,
        Err(e) => {
            eprintln!("arkos: ignoring corrupt {}: {e}", file.display());
            Vec::new()
        }
    }
}

/// The path with symlinks resolved, or as given when it cannot be (a folder since deleted).
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// `list` with `folder` first, opened at `now`: any older entry for the same folder goes and
/// the list keeps `CAP` entries.
pub fn with_noted(list: Vec<Recent>, folder: &Path, now: u64) -> Vec<Recent> {
    let folder = canonical(folder);
    let mut out = vec![Recent { path: folder.clone(), opened_unix: now }];
    out.extend(list.into_iter().filter(|r| canonical(&r.path) != folder));
    out.truncate(CAP);
    out
}

/// Records that `folder` was opened at `now` (seconds since the epoch). The file is written to a
/// sibling temp file and renamed over, so a crash never leaves it torn.
pub fn note(file: &Path, folder: &Path, now: u64) -> std::io::Result<()> {
    let list = with_noted(load(file), folder, now);
    let parent = file.parent().ok_or_else(|| std::io::Error::other("recent file has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let name = file.file_name().ok_or_else(|| std::io::Error::other("recent file has no name"))?;
    let tmp = parent.join(format!(".{}.{}.tmp", name.to_string_lossy(), std::process::id()));
    let text = serde_json::to_string_pretty(&RecentFile { folders: list }).map_err(std::io::Error::other)?;
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, file)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// The entries whose folder still exists. The file keeps the rest: a disconnected volume's
/// folders come back when it does.
pub fn visible(list: &[Recent]) -> Vec<Recent> {
    list.iter().filter(|r| r.path.is_dir()).cloned().collect()
}

/// Seconds since the epoch, for `note`.
pub fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[Recent]) -> Vec<PathBuf> {
        list.iter().map(|r| r.path.clone()).collect()
    }

    #[test]
    fn recent_newest_first_and_deduplicated() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let (a, b) = (root.join("a"), root.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let file = root.join("support/recent.json");
        note(&file, &a, 10).unwrap();
        note(&file, &b, 20).unwrap();
        assert_eq!(paths(&load(&file)), vec![b.clone(), a.clone()]);
        // The same folder by another spelling is one entry, moved to the top.
        note(&file, &root.join("b/../a"), 30).unwrap();
        let list = load(&file);
        assert_eq!(paths(&list), vec![a.clone(), b.clone()]);
        assert_eq!(list[0].opened_unix, 30);
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("\"folders\"") && text.contains("\"opened_unix\""), "{text}");
        let leftovers: Vec<_> = std::fs::read_dir(file.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(leftovers.len(), 1, "no temp file left behind: {leftovers:?}");
    }

    #[test]
    fn recent_keeps_fifty() {
        let mut list = Vec::new();
        for i in 0..60u64 {
            list = with_noted(list, Path::new(&format!("/nonexistent/arkos-recent/{i}")), i);
        }
        assert_eq!(list.len(), CAP);
        assert_eq!(list[0].path, PathBuf::from("/nonexistent/arkos-recent/59"));
        assert_eq!(list[CAP - 1].path, PathBuf::from("/nonexistent/arkos-recent/10"));
    }

    #[test]
    fn recent_missing_or_corrupt_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&dir.path().join("recent.json")).is_empty());
        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, "{\"folders\": [").unwrap();
        assert!(load(&bad).is_empty());
        // A corrupt file is replaced by the next note.
        let folder = std::fs::canonicalize(dir.path()).unwrap();
        note(&bad, &folder, 5).unwrap();
        assert_eq!(paths(&load(&bad)), vec![folder]);
    }

    #[test]
    fn recent_visible_drops_a_deleted_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (kept, gone) = (dir.path().join("kept"), dir.path().join("gone"));
        std::fs::create_dir_all(&kept).unwrap();
        std::fs::create_dir_all(&gone).unwrap();
        let file = dir.path().join("recent.json");
        note(&file, &kept, 1).unwrap();
        note(&file, &gone, 2).unwrap();
        std::fs::remove_dir(&gone).unwrap();
        let list = load(&file);
        assert_eq!(list.len(), 2, "the file keeps it");
        let shown = visible(&list);
        assert_eq!(shown.len(), 1);
        assert!(shown[0].path.ends_with("kept"));
    }
}
