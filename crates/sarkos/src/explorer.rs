//! The file tree's model (spec 6): rooted at the project folder, lazy (a directory's children
//! are read on expand), `.gitignore` respected through `git check-ignore`, `.git` hidden. Pure
//! apart from `list_dir`, `mark_ignored` and `read_dir_entries`, which the window runs on the
//! background executor.

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
        // A symlink to a directory lists as a directory; a broken one as a file.
        let is_dir = entry.metadata().map(|m| m.is_dir()).unwrap_or(false);
        out.push(EntryInfo { path: entry.path(), name, is_dir, ignored: false });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

/// Marks the entries `.gitignore` ignores, by asking git from the repository root. Outside a
/// repository, or without git, nothing is marked. One process per call: the caller runs it
/// once per expanded directory, off the UI thread. Git refuses a path beyond a symlinked
/// directory (exit 128), so nothing inside one is marked.
pub fn mark_ignored(root: &Path, entries: &mut [EntryInfo]) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    if entries.is_empty() {
        return;
    }
    let Ok(mut child) = Command::new("git").arg("-C").arg(root).args(["check-ignore", "-z", "--stdin"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else {
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
}

impl Explorer {
    /// `expanded` comes from the workspace file: an entry outside the project (or the root
    /// itself, which is implicit) is dropped rather than read and polled for nothing.
    pub fn new(root: PathBuf, expanded: Vec<PathBuf>, show_ignored: bool) -> Self {
        let expanded = expanded.into_iter().filter(|d| d != &root && d.starts_with(&root)).collect();
        Explorer { root, expanded, loaded: HashMap::new(), loading: HashSet::new(), stale: HashSet::new(), show_ignored, selected: None }
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
            out.push(Row { path: e.path.clone(), name: e.name.clone(), depth, is_dir: e.is_dir, expanded, ignored: e.ignored, loading });
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
        let dir = project();
        let mut entries = list_dir(dir.path()).unwrap();
        mark_ignored(dir.path(), &mut entries);
        assert!(entries.iter().all(|e| !e.ignored));
    }

    #[test]
    fn mark_ignored_follows_gitignore_in_a_repository() {
        let dir = project();
        let git = |args: &[&str]| {
            let st = std::process::Command::new("git").arg("-C").arg(dir.path()).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
        let st = std::process::Command::new("git").arg("-C").arg(dir.path()).args(["init", "-q"]).status().unwrap();
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
}
