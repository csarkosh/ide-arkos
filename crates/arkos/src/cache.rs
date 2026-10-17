//! The caches under Application Support: `cache/images` (remote images), `cache/posters` (video
//! posters) and `cache/mermaid` (diagrams), each made on demand by what fills it and swept of
//! old entries once at launch, on a thread of its own (`sweep_all`), with the work folders a
//! process left behind when it ended mid-job (`posters/job.<pid>`, `mermaid/profile-<pid>`).

use crate::paths;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The suffixes of the files the caches keep: only these, and part files (`PART`), are ever
/// swept, so a file of any other kind (the mermaid script, say) stays. A diagram's size is in a
/// `json` file beside its PNG.
const KNOWN: [&str; 8] = ["png", "jpg", "jpeg", "gif", "webp", "svg", "html", "json"];

/// The suffix of a download in progress (`remote_image::fetch`). One older than `PART_AGE` was
/// left by a fetch that never ended (a crash, a quit), since a fetch takes 20 seconds at most.
const PART: &str = "part";
const PART_AGE: Duration = Duration::from_secs(DAY);

const DAY: u64 = 86_400;

/// How long the launch sweep keeps each cache's entries: 30 days for images and diagrams, 90 for
/// posters (a poster costs a Quick Look run to make again).
pub const IMAGES_AGE: Duration = Duration::from_secs(30 * DAY);
pub const MERMAID_AGE: Duration = Duration::from_secs(30 * DAY);
pub const POSTERS_AGE: Duration = Duration::from_secs(90 * DAY);

/// Whether the cache entry at `path` may be used: a plain file (a link is followed) modified at
/// most `max_age` less a day ago. An older entry counts as missing, so it is fetched or made
/// again: the launch sweep, on a thread of its own, removes the entries older than `max_age`, and
/// one handed out just before it went would stay broken for the process, as gpui keeps a failed
/// load per path. A modification time in the future is taken as now.
pub fn fresh(path: &Path, max_age: Duration) -> bool {
    let Ok(meta) = std::fs::metadata(path) else { return false };
    let age = meta.modified().ok().and_then(|t| SystemTime::now().duration_since(t).ok()).unwrap_or(Duration::ZERO);
    meta.is_file() && age <= max_age.saturating_sub(Duration::from_secs(DAY))
}

/// The folder the caches live in.
pub fn root() -> PathBuf {
    paths::app_support().join("cache")
}

/// Removes the plain files in `dir` (never a folder or a link, never below it) whose name ends in
/// one of the known suffixes and whose modification time is more than `max_age` ago, and the part
/// files more than a day old (or `max_age`, when that is shorter). Returns how many went; a folder
/// that is not there has none.
pub fn sweep(dir: &Path, max_age: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    let now = SystemTime::now();
    entries
        .flatten()
        .filter(|entry| {
            let path = entry.path();
            let limit = match path.extension().and_then(|e| e.to_str()) {
                Some(PART) => max_age.min(PART_AGE),
                Some(e) if KNOWN.contains(&e) => max_age,
                _ => return false,
            };
            // `DirEntry::metadata` does not follow a link, so a link is not a plain file.
            let old = entry.metadata().is_ok_and(|m| m.is_file() && m.modified().ok().and_then(|t| now.duration_since(t).ok()).is_some_and(|age| age > limit));
            old && std::fs::remove_file(&path).is_ok()
        })
        .count()
}

/// Removes the folders in `dir` named `prefix` and a pid (`profile-123`) whose process has
/// ended, with everything in them: a poster's job folder or a Chrome profile its process left
/// when it ended mid-job. A folder of a running process (this one's too) stays. Returns how many
/// went; a folder that is not there has none.
pub fn sweep_ended(dir: &Path, prefix: &str) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let pid = name.to_str().and_then(|n| n.strip_prefix(prefix)).filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())).and_then(|p| p.parse::<libc::pid_t>().ok());
            // `DirEntry::metadata` does not follow a link, so a link is not a folder.
            let ended = pid.is_some_and(|pid| !alive(pid)) && entry.metadata().is_ok_and(|m| m.is_dir());
            ended && std::fs::remove_dir_all(entry.path()).is_ok()
        })
        .count()
}

/// Whether process `pid` is running: signal 0 checks without sending anything, and only "no such
/// process" says it ended (a process of another user refuses the signal and is running).
pub fn alive(pid: libc::pid_t) -> bool {
    // SAFETY: signal 0 is never delivered; kill only checks that the process exists.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// Sweeps the three caches at their ages (`IMAGES_AGE`, `MERMAID_AGE`, `POSTERS_AGE`); then the
/// job folders and Chrome profiles of processes that ended.
pub fn sweep_all() {
    let root = root();
    for (folder, age) in [("images", IMAGES_AGE), ("mermaid", MERMAID_AGE), ("posters", POSTERS_AGE)] {
        sweep(&root.join(folder), age);
    }
    sweep_ended(&root.join("posters"), "job.");
    sweep_ended(&root.join("mermaid"), "profile-");
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
        let (stale, fresh) = (dir.path().join("a.123.part"), dir.path().join("b.123.part"));
        for p in [&old, &new, &other, &stale, &fresh] {
            std::fs::write(p, "x").unwrap();
        }
        std::fs::create_dir(dir.path().join("profile-1")).unwrap();
        let past = SystemTime::now() - Duration::from_secs(40 * 86_400);
        for p in [&old, &other] {
            std::fs::File::options().write(true).open(p).unwrap().set_modified(past).unwrap();
        }
        std::fs::File::options().write(true).open(&stale).unwrap().set_modified(SystemTime::now() - Duration::from_secs(2 * 86_400)).unwrap();
        assert_eq!(sweep(dir.path(), Duration::from_secs(30 * 86_400)), 2);
        assert!(!old.exists() && new.exists() && other.exists() && dir.path().join("profile-1").is_dir());
        assert!(!stale.exists() && fresh.exists(), "a part file goes after a day, whatever the cache's age");
        assert_eq!(sweep(&dir.path().join("absent"), Duration::from_secs(1)), 0);
    }

    #[test]
    fn an_entry_within_a_day_of_its_sweep_age_counts_as_missing() {
        let dir = tempfile::tempdir().unwrap();
        let age = Duration::from_secs(30 * DAY);
        let set_age = |p: &Path, days: u64| std::fs::File::options().write(true).open(p).unwrap().set_modified(SystemTime::now() - Duration::from_secs(days * DAY)).unwrap();
        let (young, edge, old, future) = (dir.path().join("young.png"), dir.path().join("edge.png"), dir.path().join("old.png"), dir.path().join("future.png"));
        for p in [&young, &edge, &old, &future] {
            std::fs::write(p, "x").unwrap();
        }
        set_age(&young, 28);
        set_age(&edge, 29);
        set_age(&old, 31);
        std::fs::File::options().write(true).open(&future).unwrap().set_modified(SystemTime::now() + Duration::from_secs(DAY)).unwrap();
        assert!(fresh(&young, age) && fresh(&future, age));
        assert!(!fresh(&edge, age), "a day from the sweep: made again, so the sweep cannot remove it from under a view");
        assert!(!fresh(&old, age));
        assert!(!fresh(&dir.path().join("absent.png"), age) && !fresh(dir.path(), age), "missing, or not a file");
    }

    #[test]
    fn the_folders_of_processes_that_ended_are_swept_and_the_rest_stay() {
        let dir = tempfile::tempdir().unwrap();
        // macOS pids stay below 100000, so this one is never a running process.
        let ended = dir.path().join("profile-99999999");
        let ours = dir.path().join(format!("profile-{}", std::process::id()));
        let (job, other, file) = (dir.path().join("job.99999999"), dir.path().join("profile-x"), dir.path().join("profile-99999998"));
        for d in [&ended, &ours, &job, &other] {
            std::fs::create_dir(d).unwrap();
            std::fs::write(d.join("Local State"), "x").unwrap();
        }
        std::fs::write(&file, "x").unwrap();
        assert_eq!(sweep_ended(dir.path(), "profile-"), 1);
        assert!(!ended.exists() && ours.is_dir() && job.is_dir() && other.is_dir() && file.is_file(), "only a folder of an ended process goes");
        assert_eq!(sweep_ended(dir.path(), "job."), 1);
        assert!(!job.exists());
        assert_eq!(sweep_ended(&dir.path().join("absent"), "job."), 0);
    }
}
