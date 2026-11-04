//! The worktree explorer's model: the side worktrees (never the main checkouts) of each
//! repository, with the time since any of their files changed, newest first. A worktree's
//! `git status` runs only once it is expanded, on the background executor; collapsing drops the
//! result. Pure apart from the functions that run git or read the file system.

use crate::explorer::{git, git_read};
use crate::gitstatus::Status;
use crate::wt_scan::{canonical, STALE_AFTER};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    /// Canonical when the folder exists; a missing one keeps the path git printed.
    pub path: PathBuf,
    /// The main checkout this worktree belongs to, canonical: the first block git printed.
    pub repo: PathBuf,
    /// The commit, abbreviated to 8 for the row.
    pub head: String,
    /// `None` when detached.
    pub branch: Option<String>,
    /// git calls it prunable: its directory is gone.
    pub missing: bool,
    /// A bare repository's entry: no files, so never expanded.
    pub bare: bool,
    /// git printed a `locked` line for it.
    pub locked: bool,
    /// The worktree's admin folder, `<repo>/.git/worktrees/<name>`, canonical: read from the
    /// `gitdir:` line of the worktree's own `.git` file. `None` for a missing worktree.
    pub admin: Option<PathBuf>,
}

impl Worktree {
    /// Whether it has a working tree to run `git status` in: not missing, not bare.
    pub fn has_files(&self) -> bool {
        !self.missing && !self.bare
    }
}

pub type Changes = Vec<(PathBuf, Status)>;

/// The modification times of `HEAD`, `gitdir` and `locked` in a worktree's admin folder, each
/// `None` when the file is not there. A branch switch, a move and a lock or unlock each change
/// one; a `git status` changes none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stamp {
    pub head: Option<SystemTime>,
    pub gitdir: Option<SystemTime>,
    pub locked: Option<SystemTime>,
}

/// The stamp of the admin folder `admin` (`<repo>/.git/worktrees/<name>`).
pub fn stamp(admin: &Path) -> Stamp {
    let at = |name: &str| std::fs::metadata(admin.join(name)).and_then(|m| m.modified()).ok();
    Stamp { head: at("HEAD"), gitdir: at("gitdir"), locked: at("locked") }
}

/// The time since a worktree's files changed: the newest change found (`None` before the first
/// walk lands or when no file counts), whether the walk hit its budget, and whether the watcher
/// may have missed changes since.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Age {
    pub changed: Option<SystemTime>,
    pub capped: bool,
    pub lagging: bool,
}

/// Whether the file-system watcher behind the ages is running, and if not, why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Watched {
    Watching,
    NotLocal,
    Failed(String),
    FolderGone,
}

/// `git worktree list --porcelain -z`: blocks of NUL-terminated lines, each block ended by a
/// second NUL, so a path may hold a newline. The lines are `worktree <path>`, `HEAD <sha>`,
/// `branch refs/heads/<name>` or `detached`, and optional `bare`, `locked [<reason>]`,
/// `prunable <reason>`. The first block is the main checkout, and every block's `repo` is its
/// path as printed; `admin` is left `None`, since git prints no `gitdir` line.
pub fn parse_worktree_list(text: &str) -> Vec<Worktree> {
    let mut out: Vec<Worktree> = Vec::new();
    for block in text.split("\0\0").filter(|b| !b.trim_matches('\0').is_empty()) {
        let mut path = None;
        let mut head = String::new();
        let mut branch = None;
        let mut missing = false;
        let mut bare = false;
        let mut locked = false;
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
            } else if line == "locked" || line.starts_with("locked ") {
                locked = true;
            }
        }
        if let Some(path) = path {
            let repo = out.first().map_or_else(|| path.clone(), |m| m.repo.clone());
            out.push(Worktree { path, repo, head, branch, missing, bare, locked, admin: None });
        }
    }
    out
}

/// The admin folder a linked worktree's `.git` file points at, canonical; `None` when `.git` is
/// not a file holding a `gitdir:` line (a main checkout, a missing worktree).
fn admin_dir(worktree: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(worktree.join(".git")).ok()?;
    let dir = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
    Some(canonical(&worktree.join(dir)))
}

/// `git worktree list` for the repository `path` is in (a main checkout or a linked worktree):
/// the side worktrees only, each with `repo` set, paths canonical where they exist. Err is the
/// notice text: `explorer::STATUS_OFF` for a refused filter config, else git's words.
pub fn list_side(path: &Path) -> Result<Vec<Worktree>, String> {
    let mut cmd = git_read(path).ok_or_else(|| crate::explorer::STATUS_OFF.to_string())?;
    let out = cmd.args(["worktree", "list", "--porcelain", "-z"]).stdin(std::process::Stdio::null()).output().map_err(|e| format!("cannot run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let mut list = parse_worktree_list(&String::from_utf8_lossy(&out.stdout));
    if list.is_empty() {
        return Ok(list);
    }
    let repo = canonical(&list.remove(0).path);
    for wt in &mut list {
        wt.path = canonical(&wt.path);
        wt.repo = repo.clone();
        wt.admin = if wt.missing { None } else { admin_dir(&wt.path) };
    }
    Ok(list)
}

fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let out = git(root).args(args).stdin(std::process::Stdio::null()).output().map_err(|e| format!("cannot run git: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Test fixture: `git worktree add <path> <branch>`, creating the branch when it does not
/// exist yet. The app never creates worktrees (Claude does); tests need some to list.
#[cfg(test)]
pub fn create(root: &Path, branch: &str, path: &Path) -> Result<(), String> {
    let path = path.to_string_lossy();
    let exists = git_read(root).is_some_and(|mut cmd| cmd.args(["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false));
    if exists {
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
    Worktree { wt: Worktree, age: Option<Age>, expanded: bool, loading: bool, dirty: Option<usize> },
    Change { worktree: PathBuf, rel: PathBuf, status: Status },
}

pub struct Worktrees {
    /// Each repository's side worktrees, by the main checkout's canonical path.
    pub by_repo: BTreeMap<PathBuf, Vec<Worktree>>,
    pub ages: HashMap<PathBuf, Age>,
    /// By worktree path.
    pub stamps: HashMap<PathBuf, Stamp>,
    /// Repositories that could not be listed, with the notice text.
    pub errors: BTreeMap<PathBuf, String>,
    pub watched: Watched,
    /// The pointer or the keyboard is in the section: `ordered` keeps the order it last gave.
    pub hold_order: bool,
    /// The order `ordered` last computed, by path.
    order: RefCell<Vec<PathBuf>>,
    pub expanded: BTreeSet<PathBuf>,
    pub changes: HashMap<PathBuf, Changes>,
    loading: HashSet<PathBuf>,
    /// When each loading worktree's `git status` started, for the shimmer's phase.
    pub loading_since: HashMap<PathBuf, Instant>,
    /// Expanded worktrees whose loaded changes are due a reload; their rows stay up meanwhile.
    stale: HashSet<PathBuf>,
    /// When each expanded worktree's changes landed, for the ease-in.
    pub opened_at: HashMap<PathBuf, Instant>,
    /// The keyboard's row: a worktree's path, or a changed file's full path.
    pub selected: Option<PathBuf>,
}

impl Worktrees {
    pub fn new(expanded: Vec<PathBuf>) -> Self {
        Worktrees {
            by_repo: BTreeMap::new(),
            ages: HashMap::new(),
            stamps: HashMap::new(),
            errors: BTreeMap::new(),
            watched: Watched::Watching,
            hold_order: false,
            order: RefCell::new(Vec::new()),
            expanded: expanded.into_iter().collect(),
            changes: HashMap::new(),
            loading: HashSet::new(),
            loading_since: HashMap::new(),
            stale: HashSet::new(),
            opened_at: HashMap::new(),
            selected: None,
        }
    }

    /// Drops the expansion and loaded changes of `path`.
    fn collapse_state(&mut self, path: &Path) {
        self.expanded.remove(path);
        self.changes.remove(path);
        self.loading.remove(path);
        self.loading_since.remove(path);
        self.stale.remove(path);
        self.opened_at.remove(path);
    }

    /// Drops everything kept for `path`: its expansion, changes, age, stamp, and a selection on
    /// it or inside it.
    fn forget(&mut self, path: &Path) {
        self.collapse_state(path);
        self.ages.remove(path);
        self.stamps.remove(path);
        if self.selected.as_deref().is_some_and(|s| s.starts_with(path)) {
            self.selected = None;
        }
    }

    /// Replaces `repo`'s worktrees with `list` and clears its error. A worktree no longer listed
    /// takes its state with it; one that went missing keeps its age but loses its expansion.
    pub fn set_repo(&mut self, repo: &Path, list: Vec<Worktree>) {
        let old = self.by_repo.insert(repo.to_path_buf(), list).unwrap_or_default();
        self.errors.remove(repo);
        let new = &self.by_repo[repo];
        let listed: HashSet<PathBuf> = new.iter().map(|w| w.path.clone()).collect();
        let without_files: Vec<PathBuf> = new.iter().filter(|w| !w.has_files()).map(|w| w.path.clone()).collect();
        for gone in old.iter().map(|w| &w.path).filter(|p| !listed.contains(*p)) {
            self.forget(gone);
        }
        for p in without_files {
            self.collapse_state(&p);
        }
    }

    /// Forgets `repo` and every worktree it listed.
    pub fn remove_repo(&mut self, repo: &Path) {
        self.errors.remove(repo);
        for wt in self.by_repo.remove(repo).unwrap_or_default() {
            self.forget(&wt.path);
        }
    }

    pub fn set_age(&mut self, path: &Path, age: Age) {
        self.ages.insert(path.to_path_buf(), age);
    }

    /// A file in the worktree changed at `at`: its age moves there (never back), keeping
    /// whether the last walk was capped.
    pub fn touched(&mut self, path: &Path, at: SystemTime) {
        let age = self.ages.entry(path.to_path_buf()).or_default();
        age.changed = Some(age.changed.map_or(at, |c| c.max(at)));
    }

    /// The worktree's folder went away: it shows as missing and collapses.
    pub fn mark_missing(&mut self, path: &Path) {
        if let Some(wt) = self.by_repo.values_mut().flatten().find(|w| w.path == path) {
            wt.missing = true;
        }
        self.collapse_state(path);
    }

    /// The repository whose list holds the worktree `path`.
    pub fn repo_of(&self, path: &Path) -> Option<&Path> {
        self.by_repo.iter().find(|(_, l)| l.iter().any(|w| w.path == path)).map(|(r, _)| r.as_path())
    }

    /// The listed worktree `dir` is in (itself included), the deepest when worktrees nest.
    pub fn worktree_at(&self, dir: &Path) -> Option<&Worktree> {
        self.all().filter(|w| dir.starts_with(&w.path)).max_by_key(|w| w.path.components().count())
    }

    pub fn all(&self) -> impl Iterator<Item = &Worktree> {
        self.by_repo.values().flatten()
    }

    pub fn find(&self, path: &Path) -> Option<&Worktree> {
        self.all().find(|w| w.path == path)
    }

    /// Every worktree, newest change first, those with no age last, ties by path. While
    /// `hold_order` is set the order last computed holds: worktrees listed since go after it,
    /// in their own order, and ones that went are left out.
    pub fn ordered(&self) -> Vec<&Worktree> {
        let mut fresh: Vec<&Worktree> = self.all().collect();
        fresh.sort_by(|a, b| {
            let (x, y) = (self.ages.get(&a.path).and_then(|g| g.changed), self.ages.get(&b.path).and_then(|g| g.changed));
            match (x, y) {
                (Some(x), Some(y)) => y.cmp(&x),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
            .then_with(|| a.path.cmp(&b.path))
        });
        if !self.hold_order {
            *self.order.borrow_mut() = fresh.iter().map(|w| w.path.clone()).collect();
            return fresh;
        }
        let held = self.order.borrow();
        let rank: HashMap<&Path, usize> = held.iter().enumerate().map(|(i, p)| (p.as_path(), i)).collect();
        // A stable sort keeps the fresh order among the worktrees the held order does not have.
        fresh.sort_by_key(|w| rank.get(w.path.as_path()).copied().unwrap_or(usize::MAX));
        fresh
    }

    /// Whether the worktree's newest change is at least `STALE_AFTER` before `now`; never for
    /// one with no age.
    pub fn is_stale(&self, path: &Path, now: SystemTime) -> bool {
        self.ages.get(path).and_then(|a| a.changed).is_some_and(|c| now.duration_since(c).is_ok_and(|d| d >= STALE_AFTER))
    }

    /// No repository listed and none failed: the open folder holds no git repository.
    pub fn not_git(&self) -> bool {
        self.by_repo.is_empty() && self.errors.is_empty()
    }

    /// Expands or collapses; a path the list does not have, a missing worktree and a bare one
    /// stay collapsed. Collapsing drops its changes.
    pub fn toggle(&mut self, path: &Path) -> bool {
        if self.expanded.contains(path) {
            self.collapse_state(path);
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

    /// The rows in `ordered` order: each worktree, then the changes of an expanded one.
    pub fn rows(&self) -> Vec<WtRow> {
        let mut out = Vec::new();
        for wt in self.ordered() {
            let expanded = self.expanded.contains(&wt.path);
            let changes = self.changes.get(&wt.path);
            out.push(WtRow::Worktree { wt: wt.clone(), age: self.ages.get(&wt.path).copied(), expanded, loading: expanded && changes.is_none(), dirty: changes.map(Vec::len) });
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
    use std::time::{Duration, SystemTime};

    /// A side worktree of `repo` at `path`, on a branch named after it.
    fn wt(path: &str, repo: &str) -> Worktree {
        let name = Path::new(path).file_name().unwrap().to_string_lossy().into_owned();
        Worktree { path: path.into(), repo: repo.into(), head: "1".into(), branch: Some(name), missing: false, bare: false, locked: false, admin: None }
    }

    #[test]
    fn the_porcelain_list_is_parsed_with_main_first_branches_and_prunable() {
        let text = "worktree /Users/me/proj\0HEAD 0123456789abcdef0123456789abcdef01234567\0branch refs/heads/main\0\0\
worktree /Users/me/proj-feature\0HEAD 89abcdef0123456789abcdef0123456789abcdef\0branch refs/heads/feature/x\0\0\
worktree /Users/me/proj-detached\0HEAD aaaaaaaabbbbbbbbccccccccddddddddeeeeeeee\0detached\0\0\
worktree /Users/me/gone\0HEAD bbbbbbbbccccccccddddddddeeeeeeeeffffffff\0branch refs/heads/old\0prunable gitdir file points to non-existent location\0\0";
        let list = parse_worktree_list(text);
        assert_eq!(list.len(), 4);
        assert_eq!(list[0], Worktree { path: "/Users/me/proj".into(), repo: "/Users/me/proj".into(), head: "01234567".into(), branch: Some("main".into()), missing: false, bare: false, locked: false, admin: None });
        assert_eq!(list[1].branch.as_deref(), Some("feature/x"));
        assert_eq!(list[1].repo, PathBuf::from("/Users/me/proj"), "every block's repo is the first block's path");
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
        assert_eq!(list[0], Worktree { path: "/Users/me/proj.git".into(), repo: "/Users/me/proj.git".into(), head: String::new(), branch: None, missing: false, bare: true, locked: false, admin: None });
        assert_eq!(list[1].path, PathBuf::from("/Users/me/my proj-feat"), "a space is part of the path");
        assert_eq!(list[1].branch.as_deref(), Some("feat"));
        assert!(!list[1].missing, "locked is not missing");
        assert_eq!(list[2].branch, None);
        assert_eq!(list[2].path, PathBuf::from("/Users/me/odd\nname"), "with -z a newline is part of the path");
    }

    #[test]
    fn the_porcelain_list_keeps_locked() {
        let text = "worktree /r\0HEAD 0123456789abcdef0123456789abcdef01234567\0branch refs/heads/main\0\0\
worktree /r-a\0HEAD 0123456789abcdef0123456789abcdef01234567\0branch refs/heads/a\0locked on a usb disk\0\0\
worktree /r-b\0HEAD 0123456789abcdef0123456789abcdef01234567\0branch refs/heads/b\0locked\0\0\
worktree /r-c\0HEAD 0123456789abcdef0123456789abcdef01234567\0branch refs/heads/c\0\0";
        let list = parse_worktree_list(text);
        assert_eq!(list.iter().map(|w| w.locked).collect::<Vec<_>>(), [false, true, true, false], "a locked line, with or without a reason");
        assert!(list.iter().all(|w| w.admin.is_none()), "the porcelain list prints no gitdir line");
    }

    #[test]
    fn rows_list_worktrees_and_the_changes_of_the_expanded_ones() {
        let a = wt("/p-a", "/p");
        let f = wt("/p-f", "/p");
        let mut w = Worktrees::new(vec![]);
        assert!(w.rows().is_empty(), "nothing until the list lands");
        assert!(w.not_git(), "no repositories and no errors");
        w.set_repo(Path::new("/p"), vec![a.clone(), f.clone()]);
        assert!(!w.not_git());
        assert_eq!(w.to_load(), Vec::<PathBuf>::new(), "collapsed worktrees never run status");
        assert!(w.toggle(Path::new("/p-f")));
        assert_eq!(w.to_load(), vec![PathBuf::from("/p-f")]);
        let rows = w.rows();
        assert!(matches!(&rows[1], WtRow::Worktree { expanded: true, loading: true, dirty: None, age: None, .. }), "expanded and loading");
        w.mark_loading(Path::new("/p-f"));
        assert!(w.to_load().is_empty());
        w.set_changes(Path::new("/p-f"), Some(vec![(PathBuf::from("a.rs"), Status::Modified), (PathBuf::from("b.rs"), Status::Untracked)]));
        let rows = w.rows();
        assert_eq!(rows.len(), 4);
        assert!(matches!(&rows[1], WtRow::Worktree { dirty: Some(2), loading: false, .. }));
        assert!(matches!(&rows[2], WtRow::Change { rel, status: Status::Modified, .. } if rel == Path::new("a.rs")));
        assert_eq!(w.dirty_count(Path::new("/p-f")), Some(2));
        assert_eq!(w.dirty_count(Path::new("/p-a")), None, "not loaded: no count");
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
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        w.set_age(Path::new("/p-f"), Age { changed: Some(at), capped: true, lagging: false });
        assert!(matches!(&w.rows()[0], WtRow::Worktree { wt, age: Some(Age { changed: Some(c), capped: true, .. }), .. } if wt.path == Path::new("/p-f") && *c == at), "a row carries its age");
        w.remove_repo(Path::new("/p"));
        assert!(w.not_git());
        assert!(w.rows().is_empty());
        assert!(w.ages.is_empty(), "the repository's state goes with it");
    }

    #[test]
    fn a_missing_worktree_cannot_be_expanded() {
        let gone = Worktree { missing: true, ..wt("/gone", "/p") };
        let mut w = Worktrees::new(vec![]);
        w.set_repo(Path::new("/p"), vec![gone]);
        assert!(!w.toggle(Path::new("/gone")));
        assert!(w.to_load().is_empty());
        let live = wt("/p-l", "/p");
        w.set_repo(Path::new("/p"), vec![live]);
        assert!(w.toggle(Path::new("/p-l")));
        w.mark_missing(Path::new("/p-l"));
        assert!(w.find(Path::new("/p-l")).unwrap().missing, "marked missing");
        assert!(w.expanded.is_empty() && w.to_load().is_empty(), "and its expansion goes");
        assert!(!w.toggle(Path::new("/p-l")));
        assert!(!w.toggle(Path::new("/not-listed")), "a path the list does not have stays collapsed");
        assert!(w.expanded.is_empty());
    }

    #[test]
    fn status_runs_only_for_listed_worktrees_with_files() {
        let feat = wt("/p-f", "/p");
        let mut w = Worktrees::new(vec![PathBuf::from("/p-f")]);
        assert!(w.to_load().is_empty(), "saved expansions wait for the list");
        w.errors.insert(PathBuf::from("/p"), "fatal: not a git repository".into());
        assert!(!w.not_git(), "an error is shown, not the not-git line");
        assert!(w.to_load().is_empty(), "a repository that cannot be listed: nothing loads");
        assert_eq!(w.expanded.len(), 1, "but the expansion is kept for the save");
        w.set_repo(Path::new("/p"), vec![feat.clone()]);
        assert!(w.errors.is_empty(), "a listing clears the repository's error");
        assert_eq!(w.to_load(), vec![PathBuf::from("/p-f")]);
        w.mark_loading(Path::new("/p-f"));
        w.set_changes(Path::new("/p-f"), Some(vec![]));
        w.mark_all_stale();
        w.set_repo(Path::new("/p"), vec![Worktree { missing: true, ..feat }]);
        assert!(w.to_load().is_empty(), "an expanded worktree that went prunable gets no status");
        assert!(w.expanded.is_empty() && w.changes.is_empty() && w.opened_at.is_empty(), "and drops its state");
    }

    #[test]
    fn a_vanished_worktree_drops_its_state_and_a_late_status_for_a_collapsed_one_is_ignored() {
        let a = wt("/p-a", "/p");
        let f = wt("/p-f", "/p");
        let mut w = Worktrees::new(vec![PathBuf::from("/p-f")]);
        w.set_repo(Path::new("/p"), vec![a.clone(), f]);
        w.mark_loading(Path::new("/p-f"));
        w.set_changes(Path::new("/p-f"), Some(vec![]));
        assert!(w.opened_at.contains_key(Path::new("/p-f")));
        w.set_repo(Path::new("/p"), vec![a.clone()]);
        assert!(w.expanded.is_empty() && w.changes.is_empty() && w.opened_at.is_empty(), "the gone worktree's state goes with it");
        assert!(w.toggle(Path::new("/p-a")));
        w.mark_loading(Path::new("/p-a"));
        assert!(!w.toggle(Path::new("/p-a")), "collapsed while git ran");
        w.set_changes(Path::new("/p-a"), Some(vec![(PathBuf::from("a"), Status::Added)]));
        assert_eq!(w.dirty_count(Path::new("/p-a")), None, "the late result is ignored");
        assert_eq!(w.rows().len(), 1);
    }

    #[test]
    fn rows_come_newest_first_with_unaged_last_and_hold_while_the_pointer_is_in() {
        let t = |s: u64| SystemTime::UNIX_EPOCH + Duration::from_secs(s);
        let mut w = Worktrees::new(vec![]);
        w.set_repo(Path::new("/r1"), vec![wt("/r1-older", "/r1"), wt("/r1-unaged", "/r1")]);
        w.set_repo(Path::new("/r2"), vec![wt("/r2-newer", "/r2")]);
        w.set_age(Path::new("/r1-older"), Age { changed: Some(t(100)), capped: false, lagging: false });
        w.set_age(Path::new("/r2-newer"), Age { changed: Some(t(200)), capped: false, lagging: false });
        let paths = |w: &Worktrees| w.ordered().into_iter().map(|x| x.path.clone()).collect::<Vec<_>>();
        let (older, newer, unaged) = (PathBuf::from("/r1-older"), PathBuf::from("/r2-newer"), PathBuf::from("/r1-unaged"));
        assert_eq!(paths(&w), [newer.clone(), older.clone(), unaged.clone()], "newest first across repositories, no age last");
        assert_eq!(w.all().count(), 3);
        assert_eq!(w.repo_of(&older), Some(Path::new("/r1")));
        assert_eq!(w.repo_of(&newer), Some(Path::new("/r2")));
        assert_eq!(w.repo_of(Path::new("/elsewhere")), None);
        assert_eq!(w.worktree_at(Path::new("/r2-newer/src/deep")).map(|x| x.path.clone()), Some(newer.clone()), "a folder inside a worktree");
        assert_eq!(w.worktree_at(Path::new("/r2-newer-not")), None, "by path components, not by text");
        w.hold_order = true;
        w.touched(&older, t(300));
        assert_eq!(paths(&w), [newer.clone(), older.clone(), unaged.clone()], "the order holds while the pointer is in the section");
        w.hold_order = false;
        assert_eq!(paths(&w), [older.clone(), newer.clone(), unaged.clone()], "and resorts once it leaves");
        let tied = Age { changed: Some(t(300)), capped: false, lagging: false };
        w.set_age(&newer, tied);
        assert_eq!(paths(&w), [older.clone(), newer.clone(), unaged.clone()], "a tie breaks by path");
        w.set_age(&older, Age { changed: Some(t(50)), capped: true, lagging: false });
        w.touched(&older, t(400));
        assert_eq!(w.ages[&older], Age { changed: Some(t(400)), capped: true, lagging: false }, "a touch moves the age and keeps capped");
    }

    #[test]
    fn a_repository_set_again_drops_the_state_of_rows_that_went() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
        let (stay, go) = (wt("/p-stay", "/p"), wt("/p-go", "/p"));
        let other = wt("/q-x", "/q");
        let mut w = Worktrees::new(vec![]);
        w.set_repo(Path::new("/p"), vec![stay.clone(), go.clone()]);
        w.set_repo(Path::new("/q"), vec![other.clone()]);
        for p in [&go.path, &other.path] {
            assert!(w.toggle(p));
            w.mark_loading(p);
            w.set_changes(p, Some(vec![(PathBuf::from("a"), Status::Added)]));
            w.set_age(p, Age { changed: Some(t), capped: false, lagging: false });
            w.stamps.insert(p.clone(), Stamp::default());
        }
        w.selected = Some(go.path.join("a"));
        w.set_repo(Path::new("/p"), vec![stay.clone()]);
        assert!(!w.expanded.contains(&go.path) && !w.changes.contains_key(&go.path) && !w.opened_at.contains_key(&go.path));
        assert!(!w.ages.contains_key(&go.path) && !w.stamps.contains_key(&go.path), "its age and stamp go too");
        assert_eq!(w.selected, None, "a selection on the gone worktree, or a change in it, is cleared");
        assert!(w.expanded.contains(&other.path) && w.changes.contains_key(&other.path) && w.ages.contains_key(&other.path), "another repository's rows keep theirs");
        w.selected = Some(stay.path.clone());
        w.set_repo(Path::new("/p"), vec![stay.clone()]);
        assert_eq!(w.selected, Some(stay.path.clone()), "a row still listed keeps the selection");
        assert_eq!(w.ordered().len(), 2);
    }

    #[test]
    fn a_stale_worktree_is_one_unchanged_for_a_week() {
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let day = Duration::from_secs(86_400);
        let mut w = Worktrees::new(vec![]);
        w.set_repo(Path::new("/p"), vec![wt("/p-a", "/p"), wt("/p-b", "/p")]);
        w.set_age(Path::new("/p-a"), Age { changed: Some(t0), capped: false, lagging: false });
        assert!(!w.is_stale(Path::new("/p-a"), t0 + 6 * day), "six days is not stale");
        assert!(w.is_stale(Path::new("/p-a"), t0 + 7 * day), "seven days is");
        assert!(!w.is_stale(Path::new("/p-b"), t0 + 70 * day), "no age, never stale");
    }

    #[test]
    fn a_new_model_starts_watching_with_nothing_held() {
        let w = Worktrees::new(vec![]);
        assert_eq!(w.watched, Watched::Watching);
        assert!(!w.hold_order && w.selected.is_none());
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

    fn git_in(dir: &Path, args: &[&str]) {
        let st = crate::explorer::fixture_git(dir).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap();
        assert!(st.success(), "git {args:?}");
    }

    #[test]
    fn listing_from_a_main_checkout_gives_its_side_worktrees_without_itself() {
        let _lock = crate::client::fork_lock();
        let (_dir, root) = repo();
        assert_eq!(list_side(&root).unwrap(), vec![], "no side worktrees yet");
        let side = root.parent().unwrap().join("side");
        create(&root, "side", &side).unwrap();
        let list = list_side(&root).unwrap();
        assert_eq!(list.len(), 1, "the main checkout is never listed: {list:?}");
        assert_eq!(list[0].path, side.canonicalize().unwrap());
        assert_eq!(list[0].repo, root, "repo is the main checkout, canonical");
        assert_eq!(list[0].branch.as_deref(), Some("side"));
        assert_eq!(list[0].admin, Some(root.join(".git/worktrees/side").canonicalize().unwrap()), "read from the worktree's .git file");
        assert!(!list[0].locked && !list[0].missing);
    }

    #[test]
    fn listing_from_inside_a_linked_worktree_gives_the_same_list() {
        let _lock = crate::client::fork_lock();
        let (_dir, root) = repo();
        let side = root.parent().unwrap().join("side");
        create(&root, "side", &side).unwrap();
        let from_side = list_side(&side).unwrap();
        assert_eq!(from_side, list_side(&root).unwrap());
        assert!(from_side.iter().any(|w| w.path == side.canonicalize().unwrap()), "the opened worktree itself is in it");
        assert!(from_side.iter().all(|w| w.path != root), "the main checkout is not");
        let err = create(&root, "bad..name", &root.parent().unwrap().join("bad")).unwrap_err();
        assert!(err.contains("bad..name"), "git's refusal: {err}");
        assert!(remove(&side, &root, true).is_err(), "git refuses to remove the main checkout");
        remove(&root, &side, true).unwrap();
    }

    #[test]
    fn a_stamp_moves_when_the_worktree_switches_branch_and_not_on_a_status() {
        let _lock = crate::client::fork_lock();
        let (_dir, root) = repo();
        let side = root.parent().unwrap().join("side");
        create(&root, "side", &side).unwrap();
        let admin = list_side(&root).unwrap()[0].admin.clone().expect("a linked worktree has an admin folder");
        let before = stamp(&admin);
        assert!(before.head.is_some() && before.gitdir.is_some(), "HEAD and gitdir are there: {before:?}");
        assert_eq!(before.locked, None, "not locked");
        git_in(&side, &["status"]);
        assert_eq!(stamp(&admin), before, "a status leaves it alone");
        std::thread::sleep(Duration::from_millis(20));
        git_in(&side, &["checkout", "-q", "-b", "other"]);
        let after = stamp(&admin);
        assert_ne!(after.head, before.head, "a branch switch rewrites HEAD");
        assert_eq!(stamp(&root.join(".git/worktrees/nothing")), Stamp::default(), "no folder, no times");
    }

    #[test]
    fn status_create_and_remove_round_trip_on_a_real_repository() {
        let _lock = crate::client::fork_lock();
        let (_dir, root) = repo();
        let wt = root.parent().unwrap().join(format!("{}-feat", root.file_name().unwrap().to_string_lossy()));
        create(&root, "feat", &wt).expect("a new branch and worktree");
        let list = list_side(&root).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].branch.as_deref(), Some("feat"));
        std::fs::write(wt.join("a.txt"), "changed\n").unwrap();
        assert_eq!(run_status(&wt).unwrap(), vec![(PathBuf::from("a.txt"), Status::Modified)]);
        let err = remove(&root, &wt, false).unwrap_err();
        assert!(err.contains("modified") || err.contains("contains"), "git refuses a dirty worktree: {err}");
        remove(&root, &wt, true).expect("force");
        assert!(list_side(&root).unwrap().is_empty());
        assert!(!wt.exists());
        // An existing branch is checked out rather than created.
        create(&root, "feat", &wt).expect("the branch feat still exists");
        remove(&root, &wt, true).unwrap();
    }

    #[test]
    fn a_prunable_worktree_is_listed_as_missing_with_the_path_git_printed() {
        let _lock = crate::client::fork_lock();
        let (_dir, root) = repo();
        let wt = root.parent().unwrap().join(format!("{}-tmp", root.file_name().unwrap().to_string_lossy()));
        create(&root, "tmp", &wt).unwrap();
        std::fs::remove_dir_all(&wt).unwrap();
        let list = list_side(&root).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].missing);
        assert_eq!(list[0].path, wt, "the printed path, since there is nothing to canonicalise");
        assert_eq!(list[0].admin, None, "no .git file to read");
        remove(&root, &wt, true).expect("force removes a missing worktree");
        assert!(list_side(&root).unwrap().is_empty());
    }

    #[test]
    fn listing_outside_a_repository_is_git_s_error() {
        let _lock = crate::client::fork_lock();
        let dir = tempfile::tempdir().unwrap();
        let err = list_side(dir.path()).unwrap_err();
        assert!(err.contains("not a git repository"), "git's words: {err}");
    }
}
