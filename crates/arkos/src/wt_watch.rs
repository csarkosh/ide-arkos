//! What a changed folder means to the sidebar's Worktrees section: the rules that turn the
//! file-system watcher's reports into work, and the set of paths the watcher covers.

use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::worktrees::Worktrees;
use crate::wt_scan::{skipped_dir, DISCOVERY_DEPTH};

/// Whether an event path is worth looking at: not under a skipped or cache folder, and not under
/// `.git` except `.git/worktrees` and its immediate children. `home` as in `skipped_dir`. A path
/// not under `open` passes, since only the model can say whether it is under a worktree.
pub fn passes(path: &Path, open: &Path, home: bool) -> bool {
    let Ok(rel) = path.strip_prefix(open) else { return true };
    let names: Vec<_> = rel.components().filter_map(|c| match c {
        Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
        _ => None,
    }).collect();
    for (i, name) in names.iter().enumerate() {
        if name == ".git" {
            let after = &names[i + 1..];
            return after.first().is_some_and(|n| n == "worktrees") && after.len() <= 2;
        }
        if skipped_dir(name, home) {
            return false;
        }
    }
    true
}

/// Whether `path` has a `.git`, skipped or cache folder anywhere below `root`. For a worktree
/// outside the open folder, which `passes` does not cover (`stream_filter` uses it): a build
/// writing into its `target` must not read as the worktree being touched. False when `path` is
/// not under `root`.
pub fn skipped_below(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|rel| {
        rel.components().any(|c| {
            let name = c.as_os_str().to_string_lossy();
            name == ".git" || skipped_dir(&name, false)
        })
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Rule {
    /// `<repo>/.git/worktrees` itself: list that repository again.
    RelistRepo(PathBuf),
    /// `<repo>/.git/worktrees/<name>`: compare the stamp; relist only if it moved.
    CheckStamp { repo: PathBuf, name: String },
    /// Inside a listed worktree: it changed now (after the noise check, `only_noise_newer`).
    Touched(PathBuf),
    /// Read that folder once: a `.git` there registers a repository (one not known yet below the
    /// open folder within the discovery depth, or one nested in a known repository's own files,
    /// which is how nested repositories are found), a known repository whose `.git` is gone is dropped.
    Unknown(PathBuf),
    /// Past the discovery depth and inside no known repository: nothing.
    Ignore,
}

/// The innermost known repository or worktree holding `path` decides: a worktree means `Touched`,
/// a repository's `.git/worktrees` the two stamp rules, a repository's other files `Unknown`.
/// Known repositories are the main checkouts (the keys of `by_repo`) and every listed worktree.
pub fn classify(path: &Path, open: &Path, model: &Worktrees) -> Rule {
    let depth = |p: &Path| p.components().count();
    let repo = model.by_repo.keys().filter(|r| path.starts_with(r)).max_by_key(|r| depth(r));
    let worktree = model.worktree_at(path);
    match (repo, worktree) {
        (Some(r), Some(w)) if depth(r) > depth(&w.path) => in_repo(path, r),
        (_, Some(w)) => Rule::Touched(w.path.clone()),
        (Some(r), None) => in_repo(path, r),
        (None, None) => match path.strip_prefix(open) {
            Ok(rel) if rel.components().count() <= DISCOVERY_DEPTH => Rule::Unknown(path.to_path_buf()),
            _ => Rule::Ignore,
        },
    }
}

/// The rule for a path inside the repository `repo` and in none of its worktrees.
fn in_repo(path: &Path, repo: &Path) -> Rule {
    let rel: Vec<_> = path.strip_prefix(repo).into_iter().flat_map(|r| r.components())
        .map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    match rel.first().map(String::as_str) {
        Some(".git") => match rel.as_slice() {
            [_, w] if w == "worktrees" => Rule::RelistRepo(repo.to_path_buf()),
            [_, w, name] if w == "worktrees" => Rule::CheckStamp { repo: repo.to_path_buf(), name: name.clone() },
            _ => Rule::Ignore,
        },
        _ => Rule::Unknown(path.to_path_buf()),
    }
}

/// Whether every entry in `dir` newer than `since` is noise (one `read_dir`): `.DS_Store` (Finder
/// looked at the folder), `.git` (git's own) or a folder `skipped_dir` accepts (a build's `target`,
/// a cache), so a first build or a clean is not work in the worktree. False only when an entry
/// that is not noise is newer. Nothing newer, or a folder that cannot be read (it was removed),
/// is true too: a deletion alone does not count as work, so a worktree where only files were
/// deleted may read older than it is.
pub fn only_noise_newer(dir: &Path, since: SystemTime) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return true };
    entries.flatten().all(|e| {
        let newer = e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t > since);
        let name = e.file_name();
        let name = name.to_string_lossy();
        let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
        !newer || name == ".DS_Store" || name == ".git" || (is_dir && skipped_dir(&name, false))
    })
}

/// One batch's rules, each to act on once, in the order each first appeared: `Ignore` dropped, a
/// repeated rule dropped, and a worktree's `Touched` once with every folder it came from (the
/// noise check reads them until one is work). Other rules carry no folders.
pub fn collapse(rules: impl IntoIterator<Item = (Rule, PathBuf)>) -> Vec<(Rule, Vec<PathBuf>)> {
    let mut out: Vec<(Rule, Vec<PathBuf>)> = Vec::new();
    let mut at: std::collections::HashMap<Rule, usize> = std::collections::HashMap::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for (rule, path) in rules {
        if rule == Rule::Ignore {
            continue;
        }
        let touched = matches!(rule, Rule::Touched(_));
        let i = *at.entry(rule.clone()).or_insert_with(|| {
            out.push((rule, Vec::new()));
            out.len() - 1
        });
        if touched && seen.insert(path.clone()) {
            out[i].1.push(path);
        }
    }
    out
}

/// How often the watcher's view is checked against a full pass, in case it missed events.
pub const FULL_PASS_EVERY: Duration = Duration::from_secs(600);
/// The most paths the stream watches besides the open folder: outside repositories' `.git`
/// folders and worktrees outside it.
pub const EXTRA_PATHS: usize = 16;

/// The stream's path set: the open folder, then up to `EXTRA_PATHS` others, the `.git` folders of
/// known repositories whose `.git` is outside the open folder first (so a worktree added there
/// shows at once), then worktrees outside it, each group sorted; and which worktrees were left
/// out (they lag). A repository left out is not listed: the ten minute pass still lists it. A
/// missing worktree is never a path.
pub fn stream_paths(open: &Path, model: &Worktrees) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut gits: Vec<PathBuf> = model.by_repo.keys()
        .map(|r| r.join(".git"))
        .filter(|g| !g.starts_with(open))
        .collect();
    gits.sort();
    gits.dedup();
    gits.truncate(EXTRA_PATHS);
    let mut outside: Vec<PathBuf> = model.all()
        .filter(|w| !w.missing && !w.path.starts_with(open))
        .map(|w| w.path.clone())
        .collect();
    outside.sort();
    outside.dedup();
    let left_out = outside.split_off(outside.len().min(EXTRA_PATHS - gits.len()));
    let mut paths = vec![open.to_path_buf()];
    paths.extend(gits);
    paths.extend(outside);
    (paths, left_out)
}

/// The test the stream's callback puts every changed folder to before sending it, so a flood
/// (a build, `~/Library` in a home folder) is dropped on the stream's own thread. `outside` is
/// the stream's paths besides `open`. The innermost of `open` and `outside` holding the path
/// decides: under `open`, `passes`; under an outside repository's `.git`, only `.git/worktrees`
/// and its immediate children; under an outside worktree, nothing `skipped_below` it. A path
/// under none of them is sent, for `classify` to judge.
pub fn stream_filter(open: PathBuf, home: bool, outside: Vec<PathBuf>) -> std::sync::Arc<dyn Fn(&Path) -> bool + Send + Sync> {
    std::sync::Arc::new(move |path: &Path| {
        let depth = |p: &Path| p.components().count();
        let root = outside.iter().filter(|r| path.starts_with(r)).max_by_key(|r| depth(r));
        match root {
            Some(r) if !(path.starts_with(&open) && depth(&open) >= depth(r)) => {
                if r.file_name().is_some_and(|n| n == ".git") {
                    let rel: Vec<_> = path.strip_prefix(r).into_iter().flat_map(|p| p.components()).collect();
                    rel.first().is_some_and(|c| c.as_os_str() == "worktrees") && rel.len() <= 2
                } else {
                    !skipped_below(r, path)
                }
            }
            _ => passes(path, &open, home),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktrees::Worktree;
    use crate::wt_scan::canonical;
    use std::time::Duration;

    #[test]
    fn a_batch_acts_once_per_worktree_repository_and_folder() {
        fn p(s: &str) -> PathBuf {
            PathBuf::from(s)
        }
        let touched = |w: &str| Rule::Touched(p(w));
        let rules = vec![
            (touched("/w"), p("/w/src")),
            (Rule::RelistRepo(p("/r")), p("/r/.git/worktrees")),
            (touched("/w"), p("/w/docs")),
            (Rule::Ignore, p("/far/away")),
            (Rule::CheckStamp { repo: p("/r"), name: "a".into() }, p("/r/.git/worktrees/a")),
            (Rule::RelistRepo(p("/r")), p("/r/.git/worktrees")),
            (Rule::CheckStamp { repo: p("/r"), name: "b".into() }, p("/r/.git/worktrees/b")),
            (Rule::CheckStamp { repo: p("/r"), name: "a".into() }, p("/r/.git/worktrees/a")),
            (Rule::Unknown(p("/o/x")), p("/o/x")),
            (touched("/v"), p("/v")),
            (Rule::Unknown(p("/o/x")), p("/o/x")),
            (touched("/w"), p("/w/src")),
        ];
        let got = collapse(rules);
        assert_eq!(got, vec![
            (touched("/w"), vec![p("/w/src"), p("/w/docs")]),
            (Rule::RelistRepo(p("/r")), vec![]),
            (Rule::CheckStamp { repo: p("/r"), name: "a".into() }, vec![]),
            (Rule::CheckStamp { repo: p("/r"), name: "b".into() }, vec![]),
            (Rule::Unknown(p("/o/x")), vec![]),
            (touched("/v"), vec![p("/v")]),
        ], "first appearance order, Ignore dropped, a worktree's folders kept for the noise check, each once");
    }

    fn wt(path: &Path, repo: &Path) -> Worktree {
        Worktree {
            path: path.to_path_buf(),
            repo: repo.to_path_buf(),
            admin: Some(repo.join(".git/worktrees").join(path.file_name().unwrap())),
            head: "abcd1234".into(),
            branch: Some("w".into()),
            missing: false,
            bare: false,
            locked: false,
        }
    }

    #[test]
    fn a_build_writing_target_is_dropped_before_it_costs_anything() {
        let open = PathBuf::from("/o");
        assert!(!passes(&open.join("ide-arkos/.worktrees/x/target/debug"), &open, false));
        assert!(!passes(&open.join("a/node_modules/b"), &open, false));
        assert!(!passes(&open.join("a/.pytest_cache"), &open, false));
        assert!(!passes(&open.join("a/.git/objects/ab"), &open, false));
        assert!(passes(&open.join("a/.git/worktrees"), &open, false) && passes(&open.join("a/.git/worktrees/x"), &open, false));
        assert!(!passes(&open.join("a/.git/worktrees/x/logs"), &open, false), "deeper than a worktree's own folder");
        assert!(passes(&open.join("a/.github/workflows"), &open, false), "hidden but not a cache");
        assert!(!passes(&open.join("Library/x"), &open, true), "the home folder skips Library");
        assert!(passes(&open.join("Library/x"), &open, false));
    }

    fn model(open: &Path) -> (Worktrees, PathBuf, PathBuf) {
        let r = open.join("a");
        let w = r.join(".worktrees/w");
        let mut m = Worktrees::new(vec![]);
        m.set_repo(&r, vec![wt(&w, &r)]);
        (m, r, w)
    }

    #[test]
    fn events_classify_by_where_they_land() {
        let open = PathBuf::from("/o");
        let (m, r, w) = model(&open);
        assert_eq!(classify(&r.join(".git/worktrees"), &open, &m), Rule::RelistRepo(r.clone()));
        assert_eq!(classify(&r.join(".git/worktrees/w"), &open, &m), Rule::CheckStamp { repo: r.clone(), name: "w".into() });
        assert_eq!(classify(&w.join("src"), &open, &m), Rule::Touched(w.clone()));
        assert_eq!(classify(&w, &open, &m), Rule::Touched(w.clone()));
        assert_eq!(classify(&r.join("src"), &open, &m), Rule::Unknown(r.join("src")), "one read_dir: a nested repository registers when first touched");
        assert_eq!(classify(&open.join("new"), &open, &m), Rule::Unknown(open.join("new")));
        assert_eq!(classify(&open.join("1/2/3/4/5/6"), &open, &m), Rule::Ignore, "past the discovery depth and in no repository");
        assert_eq!(classify(&open.join("1/2/3/4"), &open, &m), Rule::Unknown(open.join("1/2/3/4")), "the deepest folder a walk still reads");
        assert_eq!(classify(&open.join("1/2/3/4/5"), &open, &m), Rule::Ignore, "one past it");
    }

    #[test]
    fn the_enclosing_repositorys_own_git_events_are_ignored() {
        let open = PathBuf::from("/r/sub");
        let r = PathBuf::from("/r");
        let mut m = Worktrees::new(vec![]);
        m.set_repo(&r, vec![wt(&PathBuf::from("/x/w"), &r)]);
        assert_eq!(classify(&r.join(".git/index"), &open, &m), Rule::Ignore);
        assert_eq!(classify(&r.join(".git/worktrees/w/logs"), &open, &m), Rule::Ignore);
    }

    #[test]
    fn a_build_inside_an_outside_worktree_is_not_a_touch() {
        let w = PathBuf::from("/elsewhere/w");
        assert!(skipped_below(&w, &w.join("target/debug")));
        assert!(skipped_below(&w, &w.join(".git/index")));
        assert!(!skipped_below(&w, &w.join("src")));
        assert!(!skipped_below(&w, &w));
        assert!(!skipped_below(&w, Path::new("/elsewhere")));
    }

    #[test]
    fn an_event_under_a_symlinked_open_folder_still_matches_its_worktree() {
        let t = tempfile::tempdir().unwrap();
        let real = t.path().join("real");
        std::fs::create_dir_all(real.join("a/.worktrees/w/src")).unwrap();
        let link = t.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let open = canonical(&link);
        let (m, _, w) = model(&open);
        assert_eq!(classify(&canonical(&real).join("a/.worktrees/w/src"), &open, &m), Rule::Touched(w));
    }

    #[test]
    fn a_folder_whose_only_newer_entries_are_noise_does_not_count() {
        let t = tempfile::tempdir().unwrap();
        let base = SystemTime::now() - Duration::from_secs(1000);
        let set = |name: &str, at: SystemTime| {
            let f = std::fs::File::create(t.path().join(name)).unwrap();
            f.set_modified(at).unwrap();
        };
        let set_dir = |name: &str, at: SystemTime| {
            std::fs::create_dir(t.path().join(name)).unwrap();
            std::fs::File::open(t.path().join(name)).unwrap().set_modified(at).unwrap();
        };
        set("a.rs", base);
        set(".DS_Store", base + Duration::from_secs(10));
        assert!(only_noise_newer(t.path(), base + Duration::from_secs(5)), "Finder looked at the folder");
        assert!(only_noise_newer(t.path(), base + Duration::from_secs(50)), "nothing newer: a clean or a deletion, which does not count");
        set_dir("target", base + Duration::from_secs(10));
        set(".git", base + Duration::from_secs(10));
        assert!(only_noise_newer(t.path(), base + Duration::from_secs(5)), "a first build made target; .git is git's own");
        set("target-notes.md", base + Duration::from_secs(10));
        assert!(!only_noise_newer(t.path(), base + Duration::from_secs(5)), "a file named like a skipped folder is work");
        std::fs::remove_file(t.path().join("target-notes.md")).unwrap();
        set("b.rs", base + Duration::from_secs(10));
        assert!(!only_noise_newer(t.path(), base + Duration::from_secs(5)), "a source file touched beside the new target");
    }

    #[test]
    fn the_stream_covers_the_open_folder_and_up_to_sixteen_outside_paths() {
        let open = PathBuf::from("/o");
        let mut m = Worktrees::new(vec![]);
        let repo = PathBuf::from("/elsewhere/r");
        let mut list: Vec<Worktree> = (0..20).map(|i| wt(&PathBuf::from(format!("/elsewhere/w{i:02}")), &repo)).collect();
        list.push(wt(&open.join("inside"), &repo));
        let mut gone = wt(Path::new("/elsewhere/gone"), &repo);
        gone.missing = true;
        list.push(gone);
        m.set_repo(&repo, list);
        let (paths, left_out) = stream_paths(&open, &m);
        assert_eq!(paths.len(), 17, "the open folder and sixteen others");
        assert_eq!(paths[0], open);
        assert_eq!(paths[1], repo.join(".git"), "the outside repository's .git comes before the worktrees");
        assert_eq!(left_out.len(), 5);
        assert!(!paths.iter().chain(&left_out).any(|p| p.ends_with("gone") || p.ends_with("inside")));
        assert_eq!(left_out[0], PathBuf::from("/elsewhere/w15"));
    }

    #[test]
    fn an_outside_repositorys_git_is_watched_and_an_inside_ones_is_not() {
        let open = PathBuf::from("/o");
        let mut m = Worktrees::new(vec![]);
        let outside = PathBuf::from("/elsewhere/r");
        let inside = open.join("a");
        m.set_repo(&outside, vec![wt(&open.join("linked"), &outside)]);
        m.set_repo(&inside, vec![wt(&inside.join(".worktrees/w"), &inside)]);
        let (paths, left_out) = stream_paths(&open, &m);
        assert_eq!(paths, vec![open.clone(), outside.join(".git")]);
        assert!(left_out.is_empty());

        let mut m = Worktrees::new(vec![]);
        for i in 0..20 {
            let r = PathBuf::from(format!("/elsewhere/r{i:02}"));
            m.set_repo(&r, vec![wt(&PathBuf::from(format!("/far/w{i:02}")), &r)]);
        }
        let (paths, left_out) = stream_paths(&open, &m);
        assert_eq!(paths.len(), 1 + EXTRA_PATHS, "repositories count against the cap");
        assert!(paths[1..].iter().all(|p| p.ends_with(".git")), "repositories first");
        assert_eq!(left_out.len(), 20, "every outside worktree lags; a repository left out is not listed");
    }

    #[test]
    fn the_stream_filter_drops_noise_under_the_open_folder_and_under_outside_roots() {
        let open = PathBuf::from("/o");
        let w = PathBuf::from("/elsewhere/w");
        let git = PathBuf::from("/elsewhere/r/.git");
        let f = stream_filter(open.clone(), false, vec![w.clone(), git.clone()]);
        assert!(!f(&open.join("ide-arkos/.worktrees/x/target/debug")));
        assert!(!f(&open.join("a/node_modules/b")));
        assert!(!f(&open.join("a/.pytest_cache")));
        assert!(!f(&open.join("a/.git/objects/ab")));
        assert!(f(&open.join("a/.git/worktrees")) && f(&open.join("a/.git/worktrees/x")));
        assert!(!f(&open.join("a/.git/worktrees/x/logs")), "deeper than a worktree's own folder");
        assert!(f(&open.join("a/.github/workflows")), "hidden but not a cache");
        assert!(f(&open.join("Library/x")), "not the home folder");
        assert!(!stream_filter(open.clone(), true, vec![])(&open.join("Library/x")), "the home folder skips Library");
        assert!(!f(&w.join("target/debug")), "a build inside an outside worktree");
        assert!(!f(&w.join(".git")));
        assert!(f(&w.join("src")) && f(&w));
        assert!(f(&git.join("worktrees")) && f(&git.join("worktrees/x")), "an outside repository's worktree list");
        assert!(!f(&git.join("worktrees/x/logs")) && !f(&git.join("index")) && !f(&git), "the rest of an outside .git");
    }
}
