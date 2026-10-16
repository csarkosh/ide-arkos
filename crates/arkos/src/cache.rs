//! The caches under Application Support: `cache/images` (remote images), `cache/posters` (video
//! posters) and `cache/mermaid` (diagrams), each made on demand by what fills it and swept of
//! old entries once at launch, on a thread of its own (`sweep_all`).

use crate::paths;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The suffixes of the files the caches write: only these are ever swept, so a file of any
/// other kind (a browser profile's, a part file of a fetch in flight) stays.
const KNOWN: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "svg", "html"];

const DAY: u64 = 86_400;

/// The folder the caches live in.
pub fn root() -> PathBuf {
    paths::app_support().join("cache")
}

/// Removes the plain files in `dir` (never a folder or a link, never below it) whose name ends in
/// one of the known suffixes and whose modification time is more than `max_age` ago. Returns how
/// many went; a folder that is not there has none.
pub fn sweep(dir: &Path, max_age: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    let now = SystemTime::now();
    entries
        .flatten()
        .filter(|entry| {
            let path = entry.path();
            let known = path.extension().and_then(|e| e.to_str()).is_some_and(|e| KNOWN.contains(&e));
            // `DirEntry::metadata` does not follow a link, so a link is not a plain file.
            let old = entry.metadata().is_ok_and(|m| m.is_file() && m.modified().ok().and_then(|t| now.duration_since(t).ok()).is_some_and(|age| age > max_age));
            known && old && std::fs::remove_file(&path).is_ok()
        })
        .count()
}

/// Sweeps the three caches at their ages: 30 days for images and diagrams, 90 for posters (a
/// poster costs a Quick Look run to make again).
pub fn sweep_all() {
    let root = root();
    for (folder, days) in [("images", 30), ("mermaid", 30), ("posters", 90)] {
        sweep(&root.join(folder), Duration::from_secs(days * DAY));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    #[test]
    fn the_sweep_removes_only_old_known_files() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.png");
        let new = dir.path().join("new.png");
        let other = dir.path().join("keep.js");
        for p in [&old, &new, &other] {
            std::fs::write(p, "x").unwrap();
        }
        std::fs::create_dir(dir.path().join("profile-1")).unwrap();
        let past = SystemTime::now() - Duration::from_secs(40 * 86_400);
        for p in [&old, &other] {
            std::fs::File::options().write(true).open(p).unwrap().set_modified(past).unwrap();
        }
        assert_eq!(sweep(dir.path(), Duration::from_secs(30 * 86_400)), 1);
        assert!(!old.exists() && new.exists() && other.exists() && dir.path().join("profile-1").is_dir());
        assert_eq!(sweep(&dir.path().join("absent"), Duration::from_secs(1)), 0);
    }
}
