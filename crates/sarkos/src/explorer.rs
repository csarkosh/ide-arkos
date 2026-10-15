//! The file tree's model (spec 6): rooted at the project folder, lazy (a directory's children
//! are read on expand), `.gitignore` respected through `git check-ignore`, `.git` hidden. Pure
//! apart from `list_dir`, `mark_ignored` and `read_dir_entries`, which the window runs on the
//! background executor.

use crate::gitstatus::Status;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryInfo {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub ignored: bool,
}

/// One visible row, from the root's children down through the expanded directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
    pub ignored: bool,
    /// Expanded, children not read yet.
    pub loading: bool,
    /// Its git status, or that of the untracked directory it is in.
    pub status: Option<Status>,
    /// A directory with a change somewhere inside: it carries a dot.
    pub dirty: bool,
}

/// A directory's entries: directories first, then files, each group case-insensitively by
/// name; `.git` left out. Nothing is marked ignored here.
pub fn list_dir(dir: &Path) -> io::Result<Vec<EntryInfo>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".git" {
            continue;
        }
        // Followed (`DirEntry::metadata` would not): a symlink to a directory lists as a
        // directory, a broken one as a file. Lazy loading bounds a symlink cycle to what the
        // user expands.
        let is_dir = std::fs::metadata(entry.path()).map(|m| m.is_dir()).unwrap_or(false);
        out.push(EntryInfo { path: entry.path(), name, is_dir, ignored: false });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

/// Settings every git call on the opened folder passes first: the folder's own `.git/config`
/// may name a program (`core.fsmonitor`, hooks) that git would otherwise run for a read-only
/// query.
pub const GIT_SAFE_ARGS: [&str; 4] = ["-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null"];

/// `git -C <root>` with `GIT_SAFE_ARGS`, for the commands the user chose by name (the worktree
/// actions). Every read-only query uses `git_read`.
pub fn git(root: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    cmd.args(GIT_SAFE_ARGS).arg("-C").arg(root);
    cmd
}

/// `git -C <dir>` for the tests' fixtures, blind to the user's global and system config (a
/// `commit.gpgsign` there would run gpg on the fixture's commit).
#[cfg(test)]
pub fn fixture_git(dir: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1").arg("-C").arg(dir);
    cmd
}

/// Every filter driver named in `git config --null --get-regexp '^filter\.'` output, read as
/// git reads them: each record is `key\nvalue` up to a NUL (the value may hold newlines), and a
/// key `filter.<name>.<var>` splits at its last dot, so a name may hold dots. A key with no
/// name is not a driver.
fn named_filter_drivers(out: &[u8]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for rec in out.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let key = rec.split(|b| *b == b'\n').next().unwrap_or_default();
        let Some(rest) = key.strip_prefix(b"filter.") else { continue };
        let Some(dot) = rest.iter().rposition(|b| *b == b'.') else { continue };
        names.insert(String::from_utf8_lossy(&rest[..dot]).into_owned());
    }
    names
}

/// The filter drivers `named_filter_drivers` reads, or `None` for a name that is empty or holds
/// anything outside `[A-Za-z0-9_.-]`: git might read an override for it differently (an `=`
/// ends `-c`'s key), so no query runs. A byte that is not UTF-8 reads as U+FFFD, refused too.
fn filter_driver_names(out: &[u8]) -> Option<BTreeSet<String>> {
    let names = named_filter_drivers(out);
    let usual = |n: &String| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    names.iter().all(usual).then_some(names)
}

/// The repository's `filter.*` config as `--null` records (empty when none is set), from one
/// `git config` read that runs nothing. `None` when git cannot read it.
fn read_filter_config(root: &Path) -> Option<Vec<u8>> {
    use std::process::Stdio;
    let out = git(root).args(["config", "--null", "--get-regexp", r"^filter\."]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    // Exit 1: no filter configured.
    match out.status.code() {
        Some(0) => Some(out.stdout),
        Some(1) => Some(Vec::new()),
        _ => None,
    }
}

/// Why a read-only git query gave nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoRead {
    /// No repository there, or git could not run or read its config.
    NotRepo,
    /// The repository configures a filter driver whose name an override might miss: nothing ran.
    Refused,
}

/// What the sidebar says in place of git status while `NoRead::Refused` holds.
pub const STATUS_OFF: &str = "git status off: unusual filter config";

/// The arguments every read-only query passes after `git -C <root>` and `GIT_SAFE_ARGS`:
/// `--no-optional-locks` (a status leaves `.git/index` alone, so the user's own `git add` never
/// meets its lock), and every filter driver the config names emptied and made optional:
/// re-hashing a touched file runs no program from the folder's config. Spawns one `git config`
/// read (it runs nothing), so it is called off the UI thread like the query itself; a caller
/// running two queries reads it once and builds both with `git_with`.
pub fn read_filter_args(root: &Path) -> Result<Vec<String>, NoRead> {
    let out = read_filter_config(root).ok_or(NoRead::NotRepo)?;
    let names = filter_driver_names(&out).ok_or(NoRead::Refused)?;
    let mut args = vec!["--no-optional-locks".to_string()];
    for name in names {
        for key in ["clean", "process", "smudge"] {
            args.push("-c".into());
            args.push(format!("filter.{name}.{key}="));
        }
        // An emptied driver git-lfs marks `required` would fail the query instead.
        args.push("-c".into());
        args.push(format!("filter.{name}.required=false"));
    }
    Ok(args)
}

/// `git -C <root>` with `GIT_SAFE_ARGS` and `args` from `read_filter_args`.
pub fn git_with(root: &Path, args: &[String]) -> std::process::Command {
    let mut cmd = git(root);
    cmd.args(args);
    cmd
}

/// `git -C <root>` for one read-only query: `read_filter_args` then `git_with`. `None`, and
/// nothing to run, when that read fails or names a driver it cannot safely override.
pub fn git_read(root: &Path) -> Option<std::process::Command> {
    read_filter_args(root).ok().map(|args| git_with(root, &args))
}

/// Marks the entries `.gitignore` ignores, by asking git from the repository root. Outside a
/// repository, without git, or with a filter config `git_read` refuses, nothing is marked. Two
/// processes per call (the config read, the query): the caller runs it once per expanded
/// directory, off the UI thread. Git refuses a path beyond a symlinked
/// directory (exit 128), so nothing inside one is marked.
pub fn mark_ignored(root: &Path, entries: &mut [EntryInfo]) {
    use std::io::Write;
    use std::process::Stdio;
    if entries.is_empty() {
        return;
    }
    let Some(mut cmd) = git_read(root) else { return };
    let Ok(mut child) = cmd.args(["check-ignore", "-z", "--stdin"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else {
        return;
    };
    let Some(mut stdin) = child.stdin.take() else { return };
    // Written from a thread while this one reads: git answers each path as it goes, and with
    // both pipes full a single thread writing everything first and git would wait on each other.
    let paths: Vec<Vec<u8>> = entries.iter().map(|e| e.path.as_os_str().as_encoded_bytes().to_vec()).collect();
    let writer = std::thread::spawn(move || {
        for p in paths {
            let _ = stdin.write_all(&p);
            let _ = stdin.write_all(b"\0");
        }
    });
    let out = child.wait_with_output();
    let _ = writer.join();
    let Ok(out) = out else { return };
    // Exit 1 means nothing matched; 128 means not a repository. Either way the output says.
    let ignored: HashSet<&[u8]> = out.stdout.split(|b| *b == 0).filter(|s| !s.is_empty()).collect();
    for e in entries.iter_mut() {
        e.ignored = ignored.contains(e.path.as_os_str().as_encoded_bytes());
    }
}

/// What the window loads for one directory, on the background executor. `None` (logged) when
/// the directory cannot be read, as when it was deleted.
pub fn read_dir_entries(root: &Path, dir: &Path) -> Option<Vec<EntryInfo>> {
    match list_dir(dir) {
        Ok(mut entries) => {
            mark_ignored(root, &mut entries);
            Some(entries)
        }
        Err(e) => {
            eprintln!("sarkos: cannot read {}: {e}", dir.display());
            None
        }
    }
}

pub struct Explorer {
    pub root: PathBuf,
    pub expanded: BTreeSet<PathBuf>,
    pub loaded: HashMap<PathBuf, Vec<EntryInfo>>,
    loading: HashSet<PathBuf>,
    /// Loaded directories the poll saw change: shown as they were until the re-read lands.
    stale: HashSet<PathBuf>,
    pub show_ignored: bool,
    pub selected: Option<PathBuf>,
    /// The last `git status`, by absolute path.
    status: HashMap<PathBuf, Status>,
    /// The directories with a change inside.
    dirty: HashSet<PathBuf>,
    /// The last `git status` found no repository.
    pub not_git: bool,
    /// Why git status is off in a repository (`STATUS_OFF`), shown where the worktrees would be.
    pub status_off: Option<&'static str>,
}

impl Explorer {
    /// `expanded` comes from the workspace file: an entry outside the project (or the root
    /// itself, which is implicit) is dropped rather than read and polled for nothing.
    pub fn new(root: PathBuf, expanded: Vec<PathBuf>, show_ignored: bool) -> Self {
        let expanded = expanded.into_iter().filter(|d| d != &root && d.starts_with(&root)).collect();
        Explorer {
            root,
            expanded,
            loaded: HashMap::new(),
            loading: HashSet::new(),
            stale: HashSet::new(),
            show_ignored,
            selected: None,
            status: HashMap::new(),
            dirty: HashSet::new(),
            not_git: false,
            status_off: None,
        }
    }

    /// Expanded directories whose children are not loaded, or stale, and not being loaded; the
    /// root first.
    pub fn dirs_to_load(&self) -> Vec<PathBuf> {
        let mut want: Vec<PathBuf> = std::iter::once(self.root.clone()).chain(self.expanded.iter().cloned()).collect();
        want.dedup();
        want.retain(|d| (!self.loaded.contains_key(d) || self.stale.contains(d)) && !self.loading.contains(d));
        want
    }

    /// The read starting now sees every change made before it.
    pub fn mark_loading(&mut self, dir: &Path) {
        self.stale.remove(dir);
        self.loading.insert(dir.to_path_buf());
    }

    /// A read landed. A directory invalidated while its read was in flight stays stale: the read
    /// may predate the change.
    pub fn set_loaded(&mut self, dir: &Path, entries: Vec<EntryInfo>) {
        if !self.loading.remove(dir) {
            self.stale.remove(dir);
        }
        self.loaded.insert(dir.to_path_buf(), entries);
    }

    /// A directory that could not be read: collapsed so it is neither saved nor retried, and
    /// read afresh if expanded again. The root cannot collapse; it shows empty until the poll
    /// sees it change.
    pub fn set_unreadable(&mut self, dir: &Path) {
        self.loading.remove(dir);
        self.stale.remove(dir);
        self.expanded.remove(dir);
        if dir == self.root {
            self.loaded.insert(dir.to_path_buf(), Vec::new());
        } else {
            self.loaded.remove(dir);
        }
    }

    /// Marks a directory's children stale so the next `dirs_to_load` reads them again (the poll
    /// saw it change). What is shown stays until the new read lands.
    pub fn invalidate(&mut self, dir: &Path) {
        self.stale.insert(dir.to_path_buf());
    }

    /// Expands a collapsed directory or collapses an expanded one; returns whether it is
    /// expanded now. Collapsing keeps what was loaded.
    pub fn toggle(&mut self, dir: &Path) -> bool {
        if self.expanded.remove(dir) {
            false
        } else {
            self.expanded.insert(dir.to_path_buf());
            true
        }
    }

    /// The expanded directories, for the workspace file. The root is implicit.
    pub fn expanded_dirs(&self) -> Vec<PathBuf> {
        self.expanded.iter().cloned().collect()
    }

    /// The repository's changes, or why there are none: outside a repository, or in one whose
    /// filter config `read_filter_args` refused (status off, said so in the sidebar).
    pub fn set_status(&mut self, entries: Result<Vec<(PathBuf, Status)>, NoRead>) {
        match entries {
            Err(why) => {
                self.not_git = why == NoRead::NotRepo;
                self.status_off = (why == NoRead::Refused).then_some(STATUS_OFF);
                self.status.clear();
                self.dirty.clear();
            }
            Ok(entries) => {
                self.not_git = false;
                self.status_off = None;
                self.dirty = crate::gitstatus::dirty_dirs(&self.root, &entries);
                self.status = entries.into_iter().map(|(rel, s)| (self.root.join(rel), s)).collect();
            }
        }
    }

    /// A path's own status, else the status of the nearest untracked ancestor (a file inside an
    /// untracked directory is untracked).
    pub fn status_of(&self, path: &Path) -> Option<Status> {
        if let Some(s) = self.status.get(path) {
            return Some(*s);
        }
        let mut p = path.parent();
        while let Some(parent) = p {
            if parent == self.root {
                break;
            }
            if self.status.get(parent) == Some(&Status::Untracked) {
                return Some(Status::Untracked);
            }
            p = parent.parent();
        }
        None
    }

    pub fn is_dirty_dir(&self, path: &Path) -> bool {
        self.dirty.contains(path)
    }

    pub fn rows(&self) -> Vec<Row> {
        let mut out = Vec::new();
        self.push_rows(&self.root, 0, &mut out);
        out
    }

    fn push_rows(&self, dir: &Path, depth: usize, out: &mut Vec<Row>) {
        let Some(entries) = self.loaded.get(dir) else { return };
        for e in entries {
            if e.ignored && !self.show_ignored {
                continue;
            }
            let expanded = e.is_dir && self.expanded.contains(&e.path);
            let loading = expanded && !self.loaded.contains_key(&e.path);
            let status = self.status_of(&e.path);
            let dirty = e.is_dir && self.is_dirty_dir(&e.path);
            out.push(Row { path: e.path.clone(), name: e.name.clone(), depth, is_dir: e.is_dir, expanded, ignored: e.ignored, loading, status, dirty });
            if expanded {
                self.push_rows(&e.path, depth + 1, out);
            }
        }
    }

    fn selected_index(&self, rows: &[Row]) -> Option<usize> {
        let s = self.selected.as_ref()?;
        rows.iter().position(|r| &r.path == s)
    }

    pub fn select_next(&mut self) {
        let rows = self.rows();
        if rows.is_empty() {
            return;
        }
        let i = match self.selected_index(&rows) {
            None => 0,
            Some(i) => (i + 1).min(rows.len() - 1),
        };
        self.selected = Some(rows[i].path.clone());
    }

    pub fn select_prev(&mut self) {
        let rows = self.rows();
        if rows.is_empty() {
            return;
        }
        let i = match self.selected_index(&rows) {
            None => 0,
            Some(i) => i.saturating_sub(1),
        };
        self.selected = Some(rows[i].path.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        fs::create_dir_all(p.join("src/deep")).unwrap();
        fs::create_dir_all(p.join(".git")).unwrap();
        fs::create_dir_all(p.join("target")).unwrap();
        fs::write(p.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(p.join("src/deep/x.rs"), "").unwrap();
        fs::write(p.join("README.md"), "").unwrap();
        fs::write(p.join("Cargo.toml"), "").unwrap();
        fs::write(p.join("a.txt"), "").unwrap();
        fs::write(p.join("B.txt"), "").unwrap();
        dir
    }

    #[test]
    fn rows_are_sorted_dirs_first_and_case_insensitively() {
        let dir = project();
        let entries = list_dir(dir.path()).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["src", "target", "a.txt", "B.txt", "Cargo.toml", "README.md"], "directories first, then files, both case-insensitively; .git hidden");
        assert!(entries.iter().all(|e| !e.ignored), "list_dir marks nothing; mark_ignored does");
        assert!(entries[0].is_dir && !entries[2].is_dir);
    }

    #[test]
    fn mark_ignored_does_nothing_outside_a_repository() {
        // Runs git: see FORK_LOCK.
        let _fork = crate::client::fork_lock();
        let dir = project();
        let mut entries = list_dir(dir.path()).unwrap();
        mark_ignored(dir.path(), &mut entries);
        assert!(entries.iter().all(|e| !e.ignored));
    }

    #[test]
    fn mark_ignored_follows_gitignore_in_a_repository() {
        // Runs git: see FORK_LOCK.
        let _fork = crate::client::fork_lock();
        let dir = project();
        let git = |args: &[&str]| {
            let st = fixture_git(dir.path()).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap();
            assert!(st.success(), "git {args:?}");
        };
        fs::remove_dir_all(dir.path().join(".git")).unwrap();
        git(&["init", "-q"]);
        fs::write(dir.path().join(".gitignore"), "target/\n*.txt\n").unwrap();
        let mut entries = list_dir(dir.path()).unwrap();
        mark_ignored(dir.path(), &mut entries);
        let ignored: Vec<&str> = entries.iter().filter(|e| e.ignored).map(|e| e.name.as_str()).collect();
        assert_eq!(ignored, ["target", "a.txt", "B.txt"]);
        // A subdirectory's entries are checked against the repository root's rules too.
        let mut sub = list_dir(&dir.path().join("src")).unwrap();
        mark_ignored(dir.path(), &mut sub);
        assert!(sub.iter().all(|e| !e.ignored));
    }

    #[test]
    fn a_symlinked_directory_lists_as_a_directory_and_its_children_list() {
        // Runs git: see FORK_LOCK.
        let _fork = crate::client::fork_lock();
        let dir = project();
        let root = dir.path().to_path_buf();
        std::os::unix::fs::symlink(root.join("src"), root.join("link")).unwrap();
        std::os::unix::fs::symlink(root.join("gone"), root.join("broken")).unwrap();
        let entries = list_dir(&root).unwrap();
        let link = entries.iter().find(|e| e.name == "link").unwrap();
        assert!(link.is_dir, "a symlink to a directory is a directory");
        assert!(!entries.iter().find(|e| e.name == "broken").unwrap().is_dir, "a broken symlink is a file");
        let mut ex = Explorer::new(root.clone(), vec![], false);
        ex.set_loaded(&root, entries);
        assert!(ex.toggle(&root.join("link")));
        assert_eq!(ex.dirs_to_load(), vec![root.join("link")]);
        let children = read_dir_entries(&root, &root.join("link")).expect("read through the link");
        let names: Vec<&str> = children.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["deep", "main.rs"]);
        ex.set_loaded(&root.join("link"), children);
        assert!(ex.rows().iter().any(|r| r.path == root.join("link/main.rs") && r.depth == 1));
    }

    #[test]
    fn filter_driver_names_are_read_as_git_reads_them_and_unusual_ones_refused() {
        // `--null` records: `key\nvalue\0`, the value possibly holding newlines.
        let out = b"filter.lfs.clean\ngit-lfs clean -- %f\0filter.lfs.process\ngit-lfs filter-process\0filter.lfs.required\ntrue\0filter.my.driver.smudge\nline\nfilter.a=b.clean\0filter.nokey\nv\0";
        let names: Vec<String> = filter_driver_names(out).expect("usual names").into_iter().collect();
        assert_eq!(names, ["lfs", "my.driver"], "a name may hold dots; a value's lines are not keys; a key without a subsection is not a driver");
        assert_eq!(filter_driver_names(b""), Some(BTreeSet::new()));
        for unusual in [&b"filter.a=b.clean\ncat\0"[..], b"filter.a b.clean\ncat\0", b"filter..clean\ncat\0", b"filter.\"q\".clean\ncat\0", "filter.\u{e9}.clean\ncat\0".as_bytes()] {
            assert_eq!(filter_driver_names(unusual), None, "{:?}: an override git might read differently", String::from_utf8_lossy(unusual));
        }
    }

    #[test]
    fn mark_ignored_runs_no_command_from_the_repository_config() {
        // Runs git: see FORK_LOCK.
        let _fork = crate::client::fork_lock();
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        fs::create_dir(&repo).unwrap();
        let git = |args: &[&str]| {
            let st = fixture_git(&repo).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap();
            assert!(st.success(), "git {args:?}");
        };
        git(&["init", "-q"]);
        fs::write(repo.join(".gitignore"), "*.log\n").unwrap();
        fs::write(repo.join("a.log"), "").unwrap();
        fs::write(repo.join("kept.rs"), "").unwrap();
        git(&["add", ".gitignore", "kept.rs"]);
        let marker = dir.path().join("ran");
        let script = dir.path().join("fsmonitor.sh");
        fs::write(&script, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        git(&["config", "core.fsmonitor", script.to_str().unwrap()]);
        git(&["update-index", "--fsmonitor"]);
        let _ = fs::remove_file(&marker);
        let mut entries = list_dir(&repo).unwrap();
        mark_ignored(&repo, &mut entries);
        assert!(!marker.exists(), "the repository's core.fsmonitor ran");
        let ignored: Vec<&str> = entries.iter().filter(|e| e.ignored).map(|e| e.name.as_str()).collect();
        assert_eq!(ignored, ["a.log"], "the marks are still right");
    }

    #[test]
    fn rows_follow_the_expanded_set_and_hide_ignored_unless_asked() {
        let dir = project();
        let root = dir.path().to_path_buf();
        let mut ex = Explorer::new(root.clone(), vec![], false);
        assert_eq!(ex.dirs_to_load(), vec![root.clone()], "the root is always wanted");
        let mut top = list_dir(&root).unwrap();
        top.iter_mut().find(|e| e.name == "target").unwrap().ignored = true;
        ex.set_loaded(&root, top);
        let names: Vec<(String, usize)> = ex.rows().iter().map(|r| (r.name.clone(), r.depth)).collect();
        assert_eq!(names, [("src".into(), 0), ("a.txt".into(), 0), ("B.txt".into(), 0), ("Cargo.toml".into(), 0), ("README.md".into(), 0)], "target is ignored and hidden");
        ex.show_ignored = true;
        assert!(ex.rows().iter().any(|r| r.name == "target" && r.ignored), "shown, marked, when asked");
        assert!(ex.toggle(&root.join("src")), "now expanded");
        assert_eq!(ex.dirs_to_load(), vec![root.join("src")]);
        assert!(ex.rows().iter().find(|r| r.name == "src").unwrap().loading, "expanded and not loaded yet");
        ex.set_loaded(&root.join("src"), list_dir(&root.join("src")).unwrap());
        let names: Vec<(String, usize)> = ex.rows().iter().map(|r| (r.name.clone(), r.depth)).collect();
        assert_eq!(&names[..3], [("src".into(), 0), ("deep".into(), 1), ("main.rs".into(), 1)]);
        assert!(!ex.toggle(&root.join("src")), "collapsed again");
        assert!(!ex.rows().iter().any(|r| r.name == "main.rs"));
        assert!(ex.toggle(&root.join("src")));
        assert!(ex.rows().iter().any(|r| r.name == "main.rs"), "collapsing keeps what was loaded");
        assert!(ex.dirs_to_load().is_empty());
        ex.invalidate(&root.join("src"));
        assert_eq!(ex.dirs_to_load(), vec![root.join("src")], "a changed directory is read again");
    }

    #[test]
    fn selection_moves_through_the_visible_rows_and_stops_at_the_ends() {
        let dir = project();
        let root = dir.path().to_path_buf();
        let mut ex = Explorer::new(root.clone(), vec![], false);
        ex.set_loaded(&root, list_dir(&root).unwrap());
        assert_eq!(ex.selected, None);
        ex.select_next();
        assert_eq!(ex.selected.as_deref(), Some(root.join("src").as_path()), "the first row");
        ex.select_prev();
        assert_eq!(ex.selected.as_deref(), Some(root.join("src").as_path()), "stays at the top");
        for _ in 0..20 {
            ex.select_next();
        }
        assert_eq!(ex.selected.as_deref(), Some(root.join("README.md").as_path()), "stays at the bottom");
        assert_eq!(ex.expanded_dirs(), Vec::<PathBuf>::new(), "the root is implicit");
    }

    #[test]
    fn mark_ignored_does_not_deadlock_on_many_ignored_entries() {
        // Runs git: see FORK_LOCK.
        let _fork = crate::client::fork_lock();
        let dir = tempfile::tempdir().unwrap();
        let st = fixture_git(dir.path()).args(["init", "-q"]).status().unwrap();
        assert!(st.success());
        fs::write(dir.path().join(".gitignore"), "*.log\n").unwrap();
        let logs = dir.path().join("logs");
        fs::create_dir(&logs).unwrap();
        for i in 0..3000 {
            fs::write(logs.join(format!("{i}.log")), "").unwrap();
        }
        let root = dir.path().to_path_buf();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut entries = list_dir(&logs).unwrap();
            mark_ignored(&root, &mut entries);
            let _ = tx.send(entries.iter().filter(|e| e.ignored).count());
        });
        let marked = rx.recv_timeout(std::time::Duration::from_secs(10)).expect("mark_ignored returns, git and the writer do not block each other");
        assert_eq!(marked, 3000);
    }

    #[test]
    fn an_invalidated_directory_keeps_its_rows_until_the_new_read_lands() {
        let dir = project();
        let root = dir.path().to_path_buf();
        let src = root.join("src");
        let mut ex = Explorer::new(root.clone(), vec![src.clone()], false);
        ex.set_loaded(&root, list_dir(&root).unwrap());
        ex.set_loaded(&src, list_dir(&src).unwrap());
        let before = ex.rows();
        ex.invalidate(&root);
        ex.invalidate(&src);
        assert_eq!(ex.rows(), before, "nothing vanishes while the re-read is pending");
        assert_eq!(ex.dirs_to_load(), vec![root.clone(), src.clone()]);
        ex.set_loaded(&root, list_dir(&root).unwrap());
        ex.set_loaded(&src, list_dir(&src).unwrap());
        assert!(ex.dirs_to_load().is_empty());
        // A change seen while the read is in flight is read again once it lands.
        ex.invalidate(&src);
        ex.mark_loading(&src);
        assert!(ex.dirs_to_load().is_empty(), "already being read");
        ex.invalidate(&src);
        ex.set_loaded(&src, list_dir(&src).unwrap());
        assert_eq!(ex.dirs_to_load(), vec![src.clone()], "the read may predate the change");
        ex.mark_loading(&src);
        ex.set_loaded(&src, list_dir(&src).unwrap());
        assert!(ex.dirs_to_load().is_empty());
    }

    #[test]
    fn an_unreadable_directory_is_collapsed_and_not_retried() {
        let dir = project();
        let root = dir.path().to_path_buf();
        let src = root.join("src");
        let mut ex = Explorer::new(root.clone(), vec![src.clone()], false);
        ex.set_loaded(&root, list_dir(&root).unwrap());
        ex.mark_loading(&src);
        assert_eq!(read_dir_entries(&root, &root.join("gone")), None);
        ex.set_unreadable(&src);
        assert!(ex.expanded_dirs().is_empty(), "not saved, not polled");
        assert!(ex.dirs_to_load().is_empty());
        assert!(ex.toggle(&src));
        assert_eq!(ex.dirs_to_load(), vec![src.clone()], "expanding it again reads it again");
        // The root cannot collapse: it shows empty until the poll sees it change.
        let mut ex = Explorer::new(root.clone(), vec![], false);
        ex.mark_loading(&root);
        ex.set_unreadable(&root);
        assert!(ex.rows().is_empty());
        assert!(ex.dirs_to_load().is_empty());
        ex.invalidate(&root);
        assert_eq!(ex.dirs_to_load(), vec![root]);
    }

    #[test]
    fn a_saved_expanded_directory_outside_the_project_is_dropped() {
        let root = PathBuf::from("/p");
        let ex = Explorer::new(root.clone(), vec![PathBuf::from("/elsewhere"), PathBuf::from("/p"), PathBuf::from("/p/src"), PathBuf::from("/pp")], false);
        assert_eq!(ex.expanded_dirs(), vec![PathBuf::from("/p/src")], "only directories under the root, not the root itself");
        assert_eq!(ex.dirs_to_load(), vec![root, PathBuf::from("/p/src")]);
    }

    #[test]
    fn rows_carry_their_status_and_folders_with_changes_a_dot() {
        use crate::gitstatus::Status;
        let dir = project();
        let root = dir.path().to_path_buf();
        let mut ex = Explorer::new(root.clone(), vec![root.join("src")], false);
        ex.set_loaded(&root, list_dir(&root).unwrap());
        ex.set_loaded(&root.join("src"), list_dir(&root.join("src")).unwrap());
        ex.set_status(Ok(vec![(PathBuf::from("src/main.rs"), Status::Modified), (PathBuf::from("a.txt"), Status::Untracked)]));
        assert!(!ex.not_git);
        let rows = ex.rows();
        let row = |name: &str| rows.iter().find(|r| r.name == name).unwrap();
        assert_eq!(row("main.rs").status, Some(Status::Modified));
        assert_eq!(row("a.txt").status, Some(Status::Untracked));
        assert_eq!(row("B.txt").status, None);
        assert!(row("src").dirty, "a folder with a change inside carries the dot");
        assert!(!row("deep").dirty);
        assert_eq!(ex.status_of(&root.join("src/main.rs")), Some(Status::Modified));
        // A file inside an untracked directory inherits untracked.
        ex.set_status(Ok(vec![(PathBuf::from("src"), Status::Untracked)]));
        assert_eq!(ex.rows().iter().find(|r| r.name == "main.rs").unwrap().status, Some(Status::Untracked));
        ex.set_status(Err(NoRead::NotRepo));
        assert!(ex.not_git);
        assert_eq!(ex.status_off, None);
        assert!(ex.rows().iter().all(|r| r.status.is_none() && !r.dirty));
        // A refused filter config is a repository with status off, said so; not "not a repository".
        ex.set_status(Err(NoRead::Refused));
        assert!(!ex.not_git);
        assert_eq!(ex.status_off, Some(STATUS_OFF));
        assert!(ex.rows().iter().all(|r| r.status.is_none() && !r.dirty));
        ex.set_status(Ok(vec![]));
        assert_eq!((ex.not_git, ex.status_off), (false, None), "a status that runs clears both");
    }
}
