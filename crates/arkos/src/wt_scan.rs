//! Finding the git repositories under the open folder, for the sidebar's Worktrees section.

// Temporary: nothing calls this yet; the allowance is removed when Task 6 and Task 9 use the module.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Folders never entered: dependency, build and output folders that are large and hold no
/// repositories the user works in.
pub const SKIP_DIRS: &[&str] = &[
    "node_modules", "target", "build", "dist", ".build", "Pods", "DerivedData", "vendor", "__pycache__", ".venv",
    "venv",
];

/// Folders never entered when the open folder is the home folder.
pub const HOME_SKIP_DIRS: &[&str] = &["Library", "Applications"];

/// Hidden cache folders, never entered.
pub const CACHE_DIRS: &[&str] = &[
    ".pytest_cache", ".mypy_cache", ".ruff_cache", ".next", ".nuxt", ".gradle", ".turbo", ".cache", ".parcel-cache",
    ".svelte-kit",
];

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
            } else if depth + 1 < DISCOVERY_DEPTH {
                // Its own children would be at depth + 2, still within the cap only if this holds.
                stack.push((path, depth + 1));
            }
        }
    }
    below.sort();
    repos.extend(below);
    Discovery { repos, folders_read, capped }
}

/// The most entries one age walk reads.
pub const AGE_BUDGET: usize = 50_000;

/// A worktree with no change for this long counts as stale.
pub const STALE_AFTER: Duration = Duration::from_secs(7 * 86_400);

/// The newest file change found, and whether the budget cut the walk short.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Newest {
    pub at: SystemTime,
    pub capped: bool,
}

/// The newest modification time of any file under `dir`, skipping `.git`, symlinks, any folder
/// holding a `.git` of its own, skipped and cache folders, and `.DS_Store`; `None` when no file
/// counts. Reads at most `budget` entries; past it the result is what was found, `capped`.
pub fn newest_change(dir: &Path, budget: usize) -> Option<Newest> {
    let mut newest: Option<SystemTime> = None;
    let mut read = 0;
    let mut capped = false;
    let mut stack = vec![dir.to_path_buf()];
    'walk: while let Some(folder) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            if read >= budget {
                capped = true;
                break 'walk;
            }
            read += 1;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name == ".git" || name == ".DS_Store" {
                continue;
            }
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() {
                if !skipped_dir(name, false) && !path.join(".git").exists() {
                    stack.push(path);
                }
            } else if kind.is_file() {
                if let Ok(at) = entry.metadata().and_then(|m| m.modified()) {
                    newest = Some(newest.map_or(at, |n| n.max(at)));
                }
            }
        }
    }
    newest.map(|at| Newest { at, capped })
}

/// The short age: `now` under a minute, then `4 min`, `7 h`, `2 d`, `3 w`, `5 mo`, `2 y`.
pub fn age_text(age: Duration) -> String {
    let secs = age.as_secs();
    let days = secs / 86_400;
    match secs {
        0..=59 => "now".to_string(),
        60..=3_599 => format!("{} min", secs / 60),
        3_600..=86_399 => format!("{} h", secs / 3_600),
        _ if days < 7 => format!("{days} d"),
        _ if days < 30 => format!("{} w", days / 7),
        _ if days < 365 => format!("{} mo", days / 30),
        _ => format!("{} y", days / 365),
    }
}

/// `7d`, `36h`, `90m`, `2w` to a duration; `None` for anything else.
pub fn parse_age(text: &str) -> Option<Duration> {
    let unit = text.chars().last()?;
    let per = match unit {
        'm' => 60,
        'h' => 3_600,
        'd' => 86_400,
        'w' => 7 * 86_400,
        _ => return None,
    };
    let digits = &text[..text.len() - 1];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u64>().ok()?.checked_mul(per).map(Duration::from_secs)
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
        assert_eq!(find_repos(&d, DISCOVERY_BUDGET).repos, vec![d.join("1/2/3/4")]);

        let (_t3, deep) = fixture();
        mk(&deep.join("1/2/3/4/5/.git"));
        assert!(find_repos(&deep, DISCOVERY_BUDGET).repos.is_empty(), "depth 5 is past the cap");

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

    fn touch(p: &Path, at: SystemTime) {
        if let Some(parent) = p.parent() {
            mk(parent);
        }
        if !p.exists() {
            fs::write(p, "x").unwrap();
        }
        fs::File::options().write(true).open(p).unwrap().set_modified(at).unwrap();
    }

    #[test]
    fn the_newest_file_wins_and_git_caches_other_checkouts_and_ds_store_do_not_count() {
        let (_t, d) = fixture();
        let w = d.join("w");
        let t = SystemTime::now() - Duration::from_secs(10_000);
        let later = t + Duration::from_secs(100);
        touch(&w.join("src/a.rs"), t);
        touch(&w.join(".git/index"), later);
        touch(&w.join("target/x"), later);
        touch(&w.join(".pytest_cache/y"), later);
        mk(&w.join("nested/.git"));
        touch(&w.join("nested/z"), later);
        touch(&w.join(".DS_Store"), later);
        touch(&w.join(".github/ci.yml"), t + Duration::from_secs(10));
        assert_eq!(
            newest_change(&w, AGE_BUDGET).unwrap(),
            Newest { at: t + Duration::from_secs(10), capped: false },
            "the hidden .github counts, the rest do not"
        );
    }

    #[test]
    fn a_worktree_with_no_files_has_no_age() {
        let (_t, d) = fixture();
        let w = d.join("w");
        mk(&w.join(".git"));
        assert_eq!(newest_change(&w, AGE_BUDGET), None);
    }

    #[test]
    fn the_budget_caps_the_walk_and_says_so() {
        let (_t, d) = fixture();
        let w = d.join("w");
        let t = SystemTime::now() - Duration::from_secs(10_000);
        for i in 0..100 {
            touch(&w.join(format!("f{i}")), t);
        }
        let found = newest_change(&w, 10).unwrap();
        assert!(found.capped);
    }

    #[test]
    fn ages_read_short() {
        assert_eq!(age_text(Duration::from_secs(30)), "now");
        assert_eq!(age_text(Duration::from_secs(4 * 60)), "4 min");
        assert_eq!(age_text(Duration::from_secs(7 * 3600)), "7 h");
        assert_eq!(age_text(Duration::from_secs(2 * 86_400)), "2 d");
        assert_eq!(age_text(Duration::from_secs(3 * 7 * 86_400)), "3 w");
        assert_eq!(age_text(Duration::from_secs(150 * 86_400)), "5 mo");
        assert_eq!(age_text(Duration::from_secs(800 * 86_400)), "2 y");
    }

    #[test]
    fn an_age_flag_parses_days_hours_minutes_and_weeks() {
        assert_eq!(parse_age("7d"), Some(Duration::from_secs(7 * 86_400)));
        assert_eq!(parse_age("36h"), Some(Duration::from_secs(36 * 3600)));
        assert_eq!(parse_age("2w"), Some(Duration::from_secs(14 * 86_400)));
        assert_eq!(parse_age("90m"), Some(Duration::from_secs(90 * 60)));
        assert!(parse_age("7").is_none() && parse_age("d").is_none() && parse_age("7 days").is_none());
    }
}
