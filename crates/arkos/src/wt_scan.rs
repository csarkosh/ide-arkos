//! Finding the git repositories under the open folder, for the sidebar's Worktrees section.

// Nothing calls this yet; the scan's later parts do, and this allowance goes with them.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// Folders never entered: dependency, build and output folders that are large and hold no
/// repositories the user works in.
pub const SKIP_DIRS: &[&str] = &[
    "node_modules", "target", "build", "dist", "vendor", "Pods", "venv", "__pycache__", "out",
];

/// Folders never entered when the open folder is the home folder.
pub const HOME_SKIP_DIRS: &[&str] = &["Library", "Applications"];

/// Hidden cache folders, never entered.
pub const CACHE_DIRS: &[&str] = &[".cache", ".pytest_cache", ".mypy_cache", ".gradle", ".npm", ".cargo", ".rustup"];

/// How many folders deep below the open folder a repository is looked for.
pub const DISCOVERY_DEPTH: usize = 4;

/// The most folders one walk reads.
pub const DISCOVERY_BUDGET: usize = 2_000;

/// Whether a folder of this name is never entered: skipped, or a cache. `home` adds `HOME_SKIP_DIRS`.
pub fn skipped_dir(name: &str, home: bool) -> bool {
    SKIP_DIRS.contains(&name) || CACHE_DIRS.contains(&name) || (home && HOME_SKIP_DIRS.contains(&name))
}

/// What a walk found: the repositories, how many folders it read, and whether the budget cut it short.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    pub repos: Vec<PathBuf>,
    pub folders_read: usize,
    pub capped: bool,
}

/// `canonicalize`, or the path as given when it does not exist.
pub fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The repository `path` is inside: the nearest ancestor (itself included) holding `.git`.
pub fn enclosing_repo(path: &Path) -> Option<PathBuf> {
    path.ancestors().find(|a| a.join(".git").exists()).map(Path::to_path_buf)
}

/// Repositories under `open` (canonical): the one `open` is inside, found upward, first; then
/// every folder holding a `.git` (directory or file) found by a walk that does not enter
/// repositories, hidden folders, symlinks or skipped folders, `DISCOVERY_DEPTH` deep, reading at
/// most `budget` folders. `open` itself is entered even when it is a repository.
pub fn find_repos(open: &Path, budget: usize) -> Discovery {
    let home = std::env::var_os("HOME").map(|h| canonical(Path::new(&h))).is_some_and(|h| h == open);
    let mut repos: Vec<PathBuf> = enclosing_repo(open).into_iter().collect();
    let mut below = Vec::new();
    let mut folders_read = 0;
    let mut capped = false;
    let mut stack = vec![(open.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if folders_read >= budget {
            capped = true;
            break;
        }
        folders_read += 1;
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            if !kind.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.starts_with('.') || skipped_dir(name, home) {
                continue;
            }
            let path = entry.path();
            if path.join(".git").exists() {
                below.push(path);
            } else if depth < DISCOVERY_DEPTH {
                stack.push((path, depth + 1));
            }
        }
    }
    below.sort();
    repos.extend(below);
    Discovery { repos, folders_read, capped }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn mk(p: &Path) {
        fs::create_dir_all(p).unwrap();
    }

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let d = canonical(t.path());
        (t, d)
    }

    #[test]
    fn skipped_folders_are_never_entered_and_the_home_ones_only_in_a_home_folder() {
        assert!(skipped_dir("node_modules", false) && skipped_dir("target", false) && skipped_dir(".pytest_cache", false));
        assert!(!skipped_dir("Library", false) && skipped_dir("Library", true));
        assert!(!skipped_dir("src", false) && !skipped_dir(".github", false), "hidden folders are not skipped by name");
    }

    #[test]
    fn repositories_are_found_below_the_open_folder_and_the_walk_stops_at_each() {
        let (_t, d) = fixture();
        mk(&d.join("a/.git"));
        mk(&d.join("b/sub/.git"));
        mk(&d.join("a/inner/.git"));
        mk(&d.join("node_modules/x/.git"));
        mk(&d.join(".hidden/y/.git"));
        std::os::unix::fs::symlink(d.join("a"), d.join("link")).unwrap();
        let found = find_repos(&d, DISCOVERY_BUDGET);
        assert_eq!(found.repos, vec![d.join("a"), d.join("b/sub")]);
        assert!(!found.capped);
    }

    #[test]
    fn a_git_file_marks_a_repository_too() {
        let (_t, d) = fixture();
        mk(&d.join("w"));
        fs::write(d.join("w/.git"), "gitdir: /elsewhere/.git/worktrees/w\n").unwrap();
        assert!(find_repos(&d, DISCOVERY_BUDGET).repos.contains(&d.join("w")));
    }

    #[test]
    fn the_open_folder_inside_a_repository_names_that_repository_first() {
        let (_t, d) = fixture();
        mk(&d.join(".git"));
        mk(&d.join("crates/x"));
        assert_eq!(enclosing_repo(&d.join("crates/x")), Some(d.clone()));
        assert_eq!(find_repos(&d.join("crates/x"), DISCOVERY_BUDGET).repos, vec![d.clone()]);
        mk(&d.join("nested/.git"));
        assert_eq!(find_repos(&d, DISCOVERY_BUDGET).repos, vec![d.clone(), d.join("nested")]);
    }

    #[test]
    fn the_depth_cap_and_the_budget_stop_the_walk() {
        let (_t, d) = fixture();
        mk(&d.join("1/2/3/4/.git"));
        mk(&d.join("1/2/3/4/5/.git"));
        let found = find_repos(&d, DISCOVERY_BUDGET);
        assert_eq!(found.repos, vec![d.join("1/2/3/4")]);

        let (_t2, e) = fixture();
        for i in 0..30 {
            mk(&e.join(format!("s{i}")));
        }
        let found = find_repos(&e, 10);
        assert!(found.folders_read <= 10, "read {}", found.folders_read);
        assert!(found.capped);
    }

    #[test]
    fn canonical_resolves_a_symlinked_folder_and_keeps_a_missing_path() {
        let t = tempfile::tempdir().unwrap();
        let c = canonical(t.path());
        assert!(c.starts_with("/private"), "{}", c.display());
        let missing = t.path().join("nope");
        assert_eq!(canonical(&missing), missing);
    }
}
