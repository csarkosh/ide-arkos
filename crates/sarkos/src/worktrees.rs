//! The worktree explorer's model (spec 6): `git worktree list` only, until a worktree is
//! expanded, which runs `git status` for it on the background executor; collapsing drops the
//! result. Three actions, each one git command the user chose by name. Pure apart from the
//! four functions that run git.

use crate::explorer::{git, git_read};
use crate::gitstatus::Status;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    /// The commit, abbreviated to 8 for the row.
    pub head: String,
    /// `None` when detached.
    pub branch: Option<String>,
    /// The main checkout: first in the list, never removable.
    pub is_main: bool,
    /// git calls it prunable: its directory is gone.
    pub missing: bool,
    /// A bare repository's entry (only ever the main one): no files, so never expanded.
    pub bare: bool,
}

impl Worktree {
    /// Whether it has a working tree to run `git status` in: not missing, not bare.
    pub fn has_files(&self) -> bool {
        !self.missing && !self.bare
    }
}

pub type Changes = Vec<(PathBuf, Status)>;

/// `git worktree list --porcelain -z`: blocks of NUL-terminated lines, each block ended by a
/// second NUL, so a path may hold a newline. The lines are `worktree <path>`, `HEAD <sha>`,
/// `branch refs/heads/<name>` or `detached`, and optional `bare`, `locked [<reason>]`,
/// `prunable <reason>`. The first block is the main checkout.
pub fn parse_worktree_list(text: &str) -> Vec<Worktree> {
    let mut out = Vec::new();
    for block in text.split("\0\0").filter(|b| !b.trim_matches('\0').is_empty()) {
        let mut path = None;
        let mut head = String::new();
        let mut branch = None;
        let mut missing = false;
        let mut bare = false;
        for line in block.split('\0') {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(p));
            } else if let Some(h) = line.strip_prefix("HEAD ") {
                head = h.chars().take(8).collect();
            } else if let Some(b) = line.strip_prefix("branch ") {
                branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
            } else if line.starts_with("prunable") {
                missing = true;
            } else if line == "bare" {
                bare = true;
            }
        }
        if let Some(path) = path {
            let is_main = out.is_empty();
            out.push(Worktree { path, head, branch, is_main, missing, bare });
        }
    }
    out
}

/// `None` outside a repository, and when `git_read` refuses the repository's filter config.
pub fn list(root: &Path) -> Option<Vec<Worktree>> {
    let out = git_read(root)?.args(["worktree", "list", "--porcelain", "-z"]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(parse_worktree_list(&String::from_utf8_lossy(&out.stdout)))
}

/// Beside the project, `<project>-<branch>` with `/` in the branch turned into `-`.
pub fn default_path(root: &Path, branch: &str) -> PathBuf {
    let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let parent = root.parent().unwrap_or(root);
    parent.join(format!("{name}-{}", branch.replace('/', "-")))
}

fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let out = git(root).args(args).stdin(std::process::Stdio::null()).output().map_err(|e| format!("cannot run git: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn branch_exists(root: &Path, branch: &str) -> bool {
    let Some(mut cmd) = git_read(root) else { return false };
    cmd.args(["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

/// `git worktree add <path> <branch>`, creating the branch when it does not exist yet.
pub fn create(root: &Path, branch: &str, path: &Path) -> Result<(), String> {
    let path = path.to_string_lossy();
    if branch_exists(root, branch) {
        run(root, &["worktree", "add", "--", &path, branch])
    } else {
        run(root, &["worktree", "add", "-b", branch, "--", &path])
    }
}

/// `git worktree remove [--force] <path>`. Without force git refuses a worktree with changes;
/// the error is git's own text.
pub fn remove(root: &Path, path: &Path, force: bool) -> Result<(), String> {
    let path = path.to_string_lossy();
    if force {
        run(root, &["worktree", "remove", "--force", "--", &path])
    } else {
        run(root, &["worktree", "remove", "--", &path])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WtRow {
    Worktree { wt: Worktree, expanded: bool, loading: bool, dirty: Option<usize> },
    Change { worktree: PathBuf, rel: PathBuf, status: Status },
}

pub struct Worktrees {
    pub list: Option<Vec<Worktree>>,
    pub not_git: bool,
    pub expanded: BTreeSet<PathBuf>,
    pub changes: HashMap<PathBuf, Changes>,
    loading: HashSet<PathBuf>,
    /// When each loading worktree's `git status` started, for the shimmer's phase.
    pub loading_since: HashMap<PathBuf, Instant>,
    /// Expanded worktrees whose loaded changes are due a reload; their rows stay up meanwhile.
    stale: HashSet<PathBuf>,
    /// When each expanded worktree's changes landed, for the ease-in.
    pub opened_at: HashMap<PathBuf, Instant>,
    /// The keyboard's row, by index into `rows()`.
    pub selected: Option<usize>,
}

impl Worktrees {
    pub fn new(expanded: Vec<PathBuf>) -> Self {
        Worktrees { list: None, not_git: false, expanded: expanded.into_iter().collect(), changes: HashMap::new(), loading: HashSet::new(), loading_since: HashMap::new(), stale: HashSet::new(), opened_at: HashMap::new(), selected: None }
    }

    pub fn set_list(&mut self, list: Option<Vec<Worktree>>) {
        self.not_git = list.is_none();
        self.list = list;
        // `None` keeps the expanded set, for the save; `to_load` runs nothing without a list.
        if let Some(l) = &self.list {
            // A worktree that went away, or whose directory did, takes its state with it.
            let known: HashSet<&Path> = l.iter().filter(|w| !w.missing && !w.bare).map(|w| w.path.as_path()).collect();
            self.expanded.retain(|p| known.contains(p.as_path()));
            self.changes.retain(|p, _| known.contains(p.as_path()));
            self.opened_at.retain(|p, _| known.contains(p.as_path()));
            self.stale.retain(|p| known.contains(p.as_path()));
        }
    }

    pub fn find(&self, path: &Path) -> Option<&Worktree> {
        self.list.as_ref()?.iter().find(|w| w.path == path)
    }

    /// Expands or collapses; a path the list does not have, a missing worktree and a bare one
    /// stay collapsed. Collapsing drops its changes.
    pub fn toggle(&mut self, path: &Path) -> bool {
        if self.expanded.remove(path) {
            self.changes.remove(path);
            self.loading.remove(path);
            self.loading_since.remove(path);
            self.stale.remove(path);
            self.opened_at.remove(path);
            return false;
        }
        if !self.find(path).is_some_and(Worktree::has_files) {
            return false;
        }
        self.expanded.insert(path.to_path_buf());
        true
    }

    /// Expanded worktrees the list has with files, not loaded yet or stale, and not already
    /// loading.
    pub fn to_load(&self) -> Vec<PathBuf> {
        self.expanded
            .iter()
            .filter(|p| self.find(p).is_some_and(Worktree::has_files) && (!self.changes.contains_key(*p) || self.stale.contains(*p)) && !self.loading.contains(*p))
            .cloned()
            .collect()
    }

    /// The slow timer: every expanded worktree with changes loaded reloads them, keeping its
    /// rows until the new result lands.
    pub fn mark_all_stale(&mut self) {
        self.stale.extend(self.expanded.iter().filter(|p| self.changes.contains_key(*p)).cloned());
    }

    pub fn mark_loading(&mut self, path: &Path) {
        self.loading.insert(path.to_path_buf());
        self.loading_since.entry(path.to_path_buf()).or_insert_with(Instant::now);
    }

    /// The status result for an expanded worktree; `None` when git failed (shown as no changes).
    pub fn set_changes(&mut self, path: &Path, changes: Option<Changes>) {
        self.loading.remove(path);
        self.loading_since.remove(path);
        self.stale.remove(path);
        if !self.expanded.contains(path) {
            return;
        }
        // A reload replaces the rows without a second ease-in.
        if self.changes.insert(path.to_path_buf(), changes.unwrap_or_default()).is_none() {
            self.opened_at.insert(path.to_path_buf(), Instant::now());
        }
    }

    pub fn dirty_count(&self, path: &Path) -> Option<usize> {
        self.changes.get(path).map(Vec::len)
    }

    pub fn rows(&self) -> Vec<WtRow> {
        let mut out = Vec::new();
        let Some(list) = &self.list else { return out };
        for wt in list {
            let expanded = self.expanded.contains(&wt.path);
            let changes = self.changes.get(&wt.path);
            out.push(WtRow::Worktree { wt: wt.clone(), expanded, loading: expanded && changes.is_none(), dirty: changes.map(Vec::len) });
            if let Some(changes) = changes.filter(|_| expanded) {
                for (rel, status) in changes {
                    out.push(WtRow::Change { worktree: wt.path.clone(), rel: rel.clone(), status: *status });
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gitstatus::run_status;

    #[test]
    fn the_porcelain_list_is_parsed_with_main_first_branches_and_prunable() {
        let text = "worktree /Users/me/proj\0HEAD 0123456789abcdef0123456789abcdef01234567\0branch refs/heads/main\0\0\
worktree /Users/me/proj-feature\0HEAD 89abcdef0123456789abcdef0123456789abcdef\0branch refs/heads/feature/x\0\0\
worktree /Users/me/proj-detached\0HEAD aaaaaaaabbbbbbbbccccccccddddddddeeeeeeee\0detached\0\0\
worktree /Users/me/gone\0HEAD bbbbbbbbccccccccddddddddeeeeeeeeffffffff\0branch refs/heads/old\0prunable gitdir file points to non-existent location\0\0";
        let list = parse_worktree_list(text);
        assert_eq!(list.len(), 4);
        assert_eq!(list[0], Worktree { path: "/Users/me/proj".into(), head: "01234567".into(), branch: Some("main".into()), is_main: true, missing: false, bare: false });
        assert_eq!(list[1].branch.as_deref(), Some("feature/x"));
        assert!(!list[1].is_main);
        assert_eq!(list[2].branch, None, "detached");
        assert!(list[3].missing, "prunable");
        assert!(parse_worktree_list("").is_empty());
    }

    #[test]
    fn a_bare_main_spaces_and_newlines_in_paths_and_locked_worktrees_parse() {
        let text = "worktree /Users/me/proj.git\0bare\0\0\
worktree /Users/me/my proj-feat\0HEAD 0123456789abcdef0123456789abcdef01234567\0branch refs/heads/feat\0locked on a usb disk\0\0\
worktree /Users/me/odd\nname\0HEAD 89abcdef0123456789abcdef0123456789abcdef\0detached\0locked\0";
        assert!(!parse_worktree_list(text)[1].bare, "only the bare block is bare");
        let list = parse_worktree_list(text);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0], Worktree { path: "/Users/me/proj.git".into(), head: String::new(), branch: None, is_main: true, missing: false, bare: true });
        assert_eq!(list[1].path, PathBuf::from("/Users/me/my proj-feat"), "a space is part of the path");
        assert_eq!(list[1].branch.as_deref(), Some("feat"));
        assert!(!list[1].missing && !list[1].is_main, "locked is neither missing nor main");
        assert_eq!(list[2].branch, None);
        assert_eq!(list[2].path, PathBuf::from("/Users/me/odd\nname"), "with -z a newline is part of the path");
    }

    #[test]
    fn default_path_sits_beside_the_project_named_after_the_branch() {
        assert_eq!(default_path(Path::new("/Users/me/proj"), "feature/x"), PathBuf::from("/Users/me/proj-feature-x"));
        assert_eq!(default_path(Path::new("/Users/me/proj"), "fix"), PathBuf::from("/Users/me/proj-fix"));
    }

    #[test]
    fn rows_list_worktrees_and_the_changes_of_the_expanded_ones() {
        let main = Worktree { path: "/p".into(), head: "1".into(), branch: Some("main".into()), is_main: true, missing: false, bare: false };
        let feat = Worktree { path: "/p-f".into(), head: "2".into(), branch: Some("f".into()), is_main: false, missing: false, bare: false };
        let mut w = Worktrees::new(vec![]);
        assert!(w.rows().is_empty(), "nothing until the list lands");
        w.set_list(Some(vec![main.clone(), feat.clone()]));
        assert!(!w.not_git);
        assert_eq!(w.to_load(), Vec::<PathBuf>::new(), "collapsed worktrees never run status");
        assert!(w.toggle(Path::new("/p-f")));
        assert_eq!(w.to_load(), vec![PathBuf::from("/p-f")]);
        let rows = w.rows();
        assert!(matches!(&rows[1], WtRow::Worktree { expanded: true, loading: true, dirty: None, .. }), "expanded and loading");
        w.mark_loading(Path::new("/p-f"));
        assert!(w.to_load().is_empty());
        w.set_changes(Path::new("/p-f"), Some(vec![(PathBuf::from("a.rs"), Status::Modified), (PathBuf::from("b.rs"), Status::Untracked)]));
        let rows = w.rows();
        assert_eq!(rows.len(), 4);
        assert!(matches!(&rows[1], WtRow::Worktree { dirty: Some(2), loading: false, .. }));
        assert!(matches!(&rows[2], WtRow::Change { rel, status: Status::Modified, .. } if rel == Path::new("a.rs")));
        assert_eq!(w.dirty_count(Path::new("/p-f")), Some(2));
        assert_eq!(w.dirty_count(Path::new("/p")), None, "not loaded: no count");
        let opened = w.opened_at[Path::new("/p-f")];
        w.mark_all_stale();
        assert_eq!(w.to_load(), vec![PathBuf::from("/p-f")], "the slow timer reloads it");
        let rows = w.rows();
        assert_eq!(rows.len(), 4, "the old changes stay up while stale");
        assert!(matches!(&rows[1], WtRow::Worktree { dirty: Some(2), loading: false, .. }), "no shimmer on a refresh");
        w.mark_loading(Path::new("/p-f"));
        assert!(w.to_load().is_empty(), "already loading");
        w.set_changes(Path::new("/p-f"), Some(vec![(PathBuf::from("c.rs"), Status::Added)]));
        let rows = w.rows();
        assert_eq!(rows.len(), 3, "the new list replaces the old");
        assert!(matches!(&rows[2], WtRow::Change { rel, status: Status::Added, .. } if rel == Path::new("c.rs")));
        assert_eq!(w.opened_at[Path::new("/p-f")], opened, "no second ease-in");
        assert!(w.to_load().is_empty(), "fresh again");
        assert!(!w.toggle(Path::new("/p-f")), "collapsed");
        assert_eq!(w.rows().len(), 2);
        assert_eq!(w.dirty_count(Path::new("/p-f")), None, "collapsing drops the result");
        w.set_list(None);
        assert!(w.not_git);
        assert!(w.rows().is_empty());
    }

    #[test]
    fn a_missing_worktree_cannot_be_expanded() {
        let gone = Worktree { path: "/gone".into(), head: "3".into(), branch: None, is_main: false, missing: true, bare: false };
        let mut w = Worktrees::new(vec![]);
        w.set_list(Some(vec![gone]));
        assert!(!w.toggle(Path::new("/gone")));
        assert!(w.to_load().is_empty());
        let bare = Worktree { path: "/p.git".into(), head: String::new(), branch: None, is_main: true, missing: false, bare: true };
        w.set_list(Some(vec![bare]));
        assert!(!w.toggle(Path::new("/p.git")), "a bare main checkout has no files to show");
        assert!(!w.toggle(Path::new("/not-listed")), "a path the list does not have stays collapsed");
        assert!(w.expanded.is_empty());
    }

    #[test]
    fn status_runs_only_for_listed_worktrees_with_files() {
        let main = Worktree { path: "/p".into(), head: "1".into(), branch: Some("main".into()), is_main: true, missing: false, bare: false };
        let feat = Worktree { path: "/p-f".into(), head: "2".into(), branch: Some("f".into()), is_main: false, missing: false, bare: false };
        let mut w = Worktrees::new(vec![PathBuf::from("/p-f")]);
        assert!(w.to_load().is_empty(), "saved expansions wait for the list");
        w.set_list(None);
        assert!(w.to_load().is_empty(), "not a repository: nothing loads");
        assert_eq!(w.expanded.len(), 1, "but the expansion is kept for the save");
        w.set_list(Some(vec![main.clone(), feat.clone()]));
        assert_eq!(w.to_load(), vec![PathBuf::from("/p-f")]);
        w.mark_loading(Path::new("/p-f"));
        w.set_changes(Path::new("/p-f"), Some(vec![]));
        w.mark_all_stale();
        w.set_list(Some(vec![main, Worktree { missing: true, ..feat }]));
        assert!(w.to_load().is_empty(), "an expanded worktree that went prunable gets no status");
        assert!(w.expanded.is_empty() && w.changes.is_empty() && w.opened_at.is_empty(), "and drops its state");
    }

    #[test]
    fn a_vanished_worktree_drops_its_state_and_a_late_status_for_a_collapsed_one_is_ignored() {
        let main = Worktree { path: "/p".into(), head: "1".into(), branch: Some("main".into()), is_main: true, missing: false, bare: false };
        let feat = Worktree { path: "/p-f".into(), head: "2".into(), branch: Some("f".into()), is_main: false, missing: false, bare: false };
        let mut w = Worktrees::new(vec![PathBuf::from("/p-f")]);
        w.set_list(Some(vec![main.clone(), feat]));
        w.mark_loading(Path::new("/p-f"));
        w.set_changes(Path::new("/p-f"), Some(vec![]));
        assert!(w.opened_at.contains_key(Path::new("/p-f")));
        w.set_list(Some(vec![main.clone()]));
        assert!(w.expanded.is_empty() && w.changes.is_empty() && w.opened_at.is_empty(), "the gone worktree's state goes with it");
        assert!(w.toggle(Path::new("/p")));
        w.mark_loading(Path::new("/p"));
        assert!(!w.toggle(Path::new("/p")), "collapsed while git ran");
        w.set_changes(Path::new("/p"), Some(vec![(PathBuf::from("a"), Status::Added)]));
        assert_eq!(w.dirty_count(Path::new("/p")), None, "the late result is ignored");
        assert_eq!(w.rows().len(), 1);
    }

    /// A temp dir holding the repository one level down, so the worktrees the tests create
    /// beside it (`root.parent()`) land inside the temp dir.
    fn repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        let run = |args: &[&str]| {
            let st = crate::explorer::fixture_git(&root).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap();
            assert!(st.success(), "git {args:?}");
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "t@example.com"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        run(&["add", "a.txt"]);
        run(&["commit", "-q", "-m", "init"]);
        let root = root.canonicalize().unwrap();
        (dir, root)
    }

    #[test]
    fn list_create_status_and_remove_round_trip_on_a_real_repository() {
        let _lock = crate::client::fork_lock();
        let (_dir, root) = repo();
        let list = list(&root).expect("a repository");
        assert_eq!(list.len(), 1);
        assert!(list[0].is_main);
        assert_eq!(list[0].branch.as_deref(), Some("main"));
        let wt = root.parent().unwrap().join(format!("{}-feat", root.file_name().unwrap().to_string_lossy()));
        create(&root, "feat", &wt).expect("a new branch and worktree");
        let list = super::list(&root).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].branch.as_deref(), Some("feat"));
        assert_eq!(list[1].path.canonicalize().unwrap(), wt.canonicalize().unwrap());
        std::fs::write(wt.join("a.txt"), "changed\n").unwrap();
        assert_eq!(run_status(&wt).unwrap(), vec![(PathBuf::from("a.txt"), Status::Modified)]);
        let err = remove(&root, &wt, false).unwrap_err();
        assert!(err.contains("modified") || err.contains("contains"), "git refuses a dirty worktree: {err}");
        remove(&root, &wt, true).expect("force");
        assert_eq!(super::list(&root).unwrap().len(), 1);
        assert!(!wt.exists());
        // An existing branch is checked out rather than created.
        create(&root, "feat", &wt).expect("the branch feat still exists");
        remove(&root, &wt, true).unwrap();
    }

    #[test]
    fn the_linked_worktree_lists_the_main_checkout_first_and_git_errors_are_its_own_text() {
        let _lock = crate::client::fork_lock();
        let (_dir, root) = repo();
        let wt = root.parent().unwrap().join("linked");
        create(&root, "linked", &wt).unwrap();
        let list = list(&wt).expect("a linked worktree is a repository");
        assert!(list[0].is_main && list[0].path.canonicalize().unwrap() == root, "the main checkout, not the opened folder");
        assert!(!list[1].is_main);
        let err = create(&root, "bad..name", &root.parent().unwrap().join("bad")).unwrap_err();
        assert!(err.contains("bad..name"), "git's refusal: {err}");
        assert!(remove(&wt, &root, true).is_err(), "git refuses to remove the main checkout");
        remove(&root, &wt, true).unwrap();
    }

    #[test]
    fn a_prunable_worktree_is_listed_as_missing() {
        let _lock = crate::client::fork_lock();
        let (_dir, root) = repo();
        let wt = root.parent().unwrap().join(format!("{}-tmp", root.file_name().unwrap().to_string_lossy()));
        create(&root, "tmp", &wt).unwrap();
        std::fs::remove_dir_all(&wt).unwrap();
        let list = list(&root).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list[1].missing);
        remove(&root, &wt, true).expect("force removes a missing worktree");
        assert_eq!(super::list(&root).unwrap().len(), 1);
    }

    #[test]
    fn worktrees_outside_a_repository_is_none() {
        let _lock = crate::client::fork_lock();
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(list(dir.path()), None);
    }
}
