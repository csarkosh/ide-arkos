//! Git status for the tree and the worktree explorer (spec 6): `git status --porcelain=v2 -z`
//! parsed into (path, status) pairs, run through `explorer::git` on the background executor.

use crate::explorer::git;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Added,
    Modified,
    Deleted,
    Untracked,
    Conflicted,
}

/// `git status --porcelain=v2 -z` records: `1 XY ...` ordinary, `2 XY ...` renames (not
/// produced: `--no-renames`), `u XY ...` unmerged, `? path` untracked, `! path` ignored (not
/// produced). Paths are relative to the root. A record this does not know is skipped.
pub fn parse_porcelain_v2(bytes: &[u8]) -> Vec<(PathBuf, Status)> {
    let mut out = Vec::new();
    for rec in bytes.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let text = String::from_utf8_lossy(rec);
        let mut fields = text.splitn(2, ' ');
        let kind = fields.next().unwrap_or("");
        let rest = fields.next().unwrap_or("");
        let (status, path) = match kind {
            "?" => (Status::Untracked, rest),
            // The path is the last field and may hold spaces: `XY sub m1 m2 m3 mW h1 h2 h3 path`.
            "u" => (Status::Conflicted, rest.splitn(10, ' ').nth(9).unwrap_or("")),
            "1" => {
                // `XY sub mH mI mW hH hI path`.
                let mut f = rest.splitn(8, ' ');
                let xy = f.next().unwrap_or("..");
                let path = f.nth(6).unwrap_or("");
                let (x, y) = (xy.chars().next().unwrap_or('.'), xy.chars().nth(1).unwrap_or('.'));
                let status = match (x, y) {
                    ('A', _) if y == '.' => Status::Added,
                    (_, 'D') | ('D', _) => Status::Deleted,
                    _ => Status::Modified,
                };
                (status, path)
            }
            _ => continue,
        };
        if path.is_empty() {
            continue;
        }
        out.push((PathBuf::from(path.trim_end_matches('/')), status));
    }
    out
}

/// The repository's changes under `root`, relative to `root`. `None` outside a repository (or
/// without git). Untracked directories are one entry each; renames show as a deletion and an
/// addition; ignored entries are not listed (the tree asks `check-ignore` for those).
pub fn run_status(root: &Path) -> Option<Vec<(PathBuf, Status)>> {
    use std::process::Stdio;
    // Porcelain paths are relative to the repository's top, which `root` may be inside of.
    let prefix = git(root).args(["rev-parse", "--show-prefix"]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    if !prefix.status.success() {
        return None;
    }
    let prefix = PathBuf::from(String::from_utf8_lossy(&prefix.stdout).trim_end_matches('\n'));
    let out = git(root)
        .args(["status", "--porcelain=v2", "-z", "--untracked-files=normal", "--no-renames", "--ignored=no", "--", "."])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // An entry that is `root` itself (a wholly untracked project) is not a row: dropped.
    let entries = parse_porcelain_v2(&out.stdout).into_iter().filter_map(|(p, s)| p.strip_prefix(&prefix).ok().filter(|r| !r.as_os_str().is_empty()).map(|r| (r.to_path_buf(), s)));
    Some(entries.collect())
}

/// Every directory strictly between the root and a changed path: those rows carry a dot.
pub fn dirty_dirs(root: &Path, entries: &[(PathBuf, Status)]) -> HashSet<PathBuf> {
    let mut out = HashSet::new();
    for (rel, _) in entries {
        let mut p = rel.parent();
        while let Some(parent) = p {
            if parent.as_os_str().is_empty() {
                break;
            }
            out.insert(root.join(parent));
            p = parent.parent();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_v2_entries_are_parsed_with_their_status() {
        // `1 XY sub mH mI mW hH hI path` for ordinary changes, `? path` untracked, `u` conflicts.
        let out = b"1 .M N... 100644 100644 100644 abc def src/main.rs\0\
1 A. N... 000000 100644 100644 000 def new.rs\0\
1 .D N... 100644 100644 000000 abc 000 gone.rs\0\
1 MM N... 100644 100644 100644 abc def both.rs\0\
? notes.txt\0\
? untracked-dir/\0\
u UU N... 100644 100644 100644 100644 a b c conflict.rs\0";
        let entries = parse_porcelain_v2(out);
        let get = |p: &str| entries.iter().find(|(path, _)| path == Path::new(p)).map(|(_, s)| *s);
        assert_eq!(get("src/main.rs"), Some(Status::Modified));
        assert_eq!(get("new.rs"), Some(Status::Added));
        assert_eq!(get("gone.rs"), Some(Status::Deleted));
        assert_eq!(get("both.rs"), Some(Status::Modified), "staged and unstaged both: modified");
        assert_eq!(get("notes.txt"), Some(Status::Untracked));
        assert_eq!(get("untracked-dir"), Some(Status::Untracked), "a directory entry loses its slash");
        assert_eq!(get("conflict.rs"), Some(Status::Conflicted));
        assert_eq!(entries.len(), 7);
        assert!(parse_porcelain_v2(b"").is_empty());
        assert!(parse_porcelain_v2(b"garbage line\0").is_empty(), "an unknown record type is skipped");
    }

    #[test]
    fn dirty_dirs_are_every_ancestor_of_a_change_inside_the_root() {
        let root = Path::new("/p");
        let entries = vec![(PathBuf::from("src/deep/x.rs"), Status::Modified), (PathBuf::from("top.rs"), Status::Added)];
        let dirs = dirty_dirs(root, &entries);
        assert!(dirs.contains(Path::new("/p/src")));
        assert!(dirs.contains(Path::new("/p/src/deep")));
        assert!(!dirs.contains(Path::new("/p")), "the root itself is not a row");
        assert_eq!(dirs.len(), 2);
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            let st = git(dir.path()).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap();
            assert!(st.success(), "git {args:?}");
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "t@example.com"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(dir.path().join("tracked.txt"), "one\n").unwrap();
        run(&["add", "tracked.txt"]);
        run(&["commit", "-q", "-m", "init"]);
        dir
    }

    #[test]
    fn run_status_sees_a_modified_and_an_untracked_entry() {
        let _lock = crate::client::fork_lock();
        let dir = repo();
        std::fs::write(dir.path().join("tracked.txt"), "two\n").unwrap();
        std::fs::write(dir.path().join("new.txt"), "").unwrap();
        let entries = run_status(dir.path()).expect("a repository");
        assert_eq!(entries, vec![(PathBuf::from("tracked.txt"), Status::Modified), (PathBuf::from("new.txt"), Status::Untracked)]);
    }

    #[test]
    fn an_untracked_directory_marks_the_directory_not_its_files() {
        let _lock = crate::client::fork_lock();
        let dir = repo();
        std::fs::create_dir_all(dir.path().join("pile/sub")).unwrap();
        for i in 0..50 {
            std::fs::write(dir.path().join("pile").join(format!("f{i}.txt")), "").unwrap();
        }
        std::fs::write(dir.path().join("pile/sub/g.txt"), "").unwrap();
        let entries = run_status(dir.path()).unwrap();
        assert_eq!(entries, vec![(PathBuf::from("pile"), Status::Untracked)], "--untracked-files=normal collapses the directory");
    }

    #[test]
    fn a_project_inside_a_repository_sees_its_own_changes_relative_to_itself() {
        // Porcelain paths are relative to the repository's top, not to `-C`.
        let _lock = crate::client::fork_lock();
        let dir = repo();
        std::fs::create_dir_all(dir.path().join("sub/src")).unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("sub/kept.txt"), "").unwrap();
        std::fs::write(dir.path().join("sub/my file \"q\".txt"), "").unwrap();
        let st = git(dir.path()).args(["add", "sub/kept.txt", "sub/my file \"q\".txt"]).status().unwrap();
        assert!(st.success());
        std::fs::write(dir.path().join("sub/src/new.txt"), "").unwrap();
        std::fs::write(dir.path().join("src/outside.txt"), "").unwrap();
        std::fs::write(dir.path().join("tracked.txt"), "two\n").unwrap();
        let entries = run_status(&dir.path().join("sub")).unwrap();
        let want = vec![(PathBuf::from("kept.txt"), Status::Added), (PathBuf::from("my file \"q\".txt"), Status::Added), (PathBuf::from("src"), Status::Untracked)];
        assert_eq!(entries, want, "only the project's own changes, relative to it; a name with spaces and quotes whole");
        // A wholly untracked project is one entry for the root itself, which is not a row.
        assert_eq!(run_status(&dir.path().join("sub/src")), Some(vec![]));
    }

    #[test]
    fn a_project_opened_through_a_symlink_sees_the_same_changes() {
        let _lock = crate::client::fork_lock();
        let dir = repo();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/new.txt"), "").unwrap();
        let links = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(dir.path(), links.path().join("repo")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("sub"), links.path().join("sub")).unwrap();
        assert_eq!(run_status(&links.path().join("repo")), Some(vec![(PathBuf::from("sub"), Status::Untracked)]));
        assert_eq!(run_status(&links.path().join("sub")), Some(vec![]), "the wholly untracked sub, through a link");
    }

    #[test]
    fn status_outside_a_repository_is_empty_and_marks_the_root_as_not_git() {
        let _lock = crate::client::fork_lock();
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(run_status(dir.path()), None);
    }
}
