# Sarkos IDE: Worktrees, git status in the tree, diff gutters (milestone 4b). Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the sidebar git-aware: rows in the file tree carry their git status in the theme's pastels with a dot on folders that contain changes, a Worktrees section lists `git worktree list` and lazily shows each worktree's changed files with actions to open a terminal there, create one and remove one, and the viewer marks added, modified and deleted lines against the worktree's HEAD with an inline diff on Cmd+Shift+D.

**Architecture:** Every git read goes through `explorer::git_read(root)`; the two worktree actions through `explorer::git(root)` (fsmonitor and hooks off) on the background executor, sequenced so a stale result never lands over a newer one. `gitstatus.rs` parses `git status --porcelain=v2 -z` into a map of paths to statuses; the explorer keeps one for the project (refreshed at most once a second after a change the poll saw, and every ten seconds otherwise) and derives the dirty-folder set. `worktrees.rs` parses `git worktree list --porcelain`, keeps the expanded set and each expanded worktree's status, and wraps the three actions. `diff.rs` turns `git show HEAD:<path>` plus the viewer's lines into gutter marks and deleted-line positions through the `similar` crate; the viewer element paints the marks in the gutter and, with the inline diff on, interleaves deleted lines in the deleted colour. The sidebar gains a collapsible Worktrees section under the Explorer with a loading shimmer and a short ease-in for the changed rows, both time-based and off under Reduce Motion. Before any of that, `main.rs`'s sidebar and viewer code moves into their modules so the new code lands beside what it extends.

**Tech Stack:** Rust stable, gpui 0.2.2 (exact pin), git through the `git` binary, `similar` 2 for the line diff (one new dependency), the existing `uniform_list`, `LineEdit`, `ScrollAnimator`, `config_watch::fingerprint`.

**Spec:** `docs/superpowers/specs/2026-10-13-sarkos-design.md` section 6 (the file tree's git status and folder dots, the worktree explorer paragraph, the viewer's diff gutters and inline diff), section 4 (Ctrl+Shift+G), section 7 (tree colours), section 9 (never block the UI thread; a viewer holds one file).

## Global Constraints

- macOS only. The window never blocks the UI thread on IO: every git call, file read and diff runs on the background executor; the UI thread applies results. Every read-only git invocation goes through `explorer::git_read(root)`, which sets `-c core.fsmonitor=false -c core.hooksPath=/dev/null --no-optional-locks` and, after one `git config --get-regexp '^filter\.'` (a config read that runs nothing), `-c filter.<name>.clean= -c filter.<name>.process= -c filter.<name>.smudge=` for every filter driver the repository configures, so no program named in a crafted `.git/config` runs; `git status` adds `--ignore-submodules=all`; `git show` and `git diff` are additionally run with `--no-ext-diff` where they take it; no git command this plan adds reads `core.pager` (every call is non-interactive with piped stdout). The two worktree actions (`git worktree add`/`remove`, commands the user chose by name) use the plain `explorer::git(root)` (fsmonitor and hooks off) with their own arguments; `git_read` also passes `-c filter.<name>.required=false` so a required filter (git-lfs) does not fail the read, and refuses (returns nothing) when a driver name holds a character outside `[A-Za-z0-9_.-]`.
- `gpui = { version = "=0.2.2", features = ["runtime_shaders"] }` pinned exactly; never bump. If a gpui or `similar` name does not compile, grep `~/.cargo/registry/src/*/gpui-0.2.2/src/` or `~/.cargo/registry/src/*/similar-2*/src/` for it, fix the call, and note the change in the commit message.
- One new dependency, `similar = "2"` in `crates/sarkos/Cargo.toml`, added in Task 5 and nowhere else. No other new crates.
- Nothing in this plan writes to a project file except the two worktree actions the user chooses by name (`git worktree add`, `git worktree remove`), which run only from the sidebar's menu and prompt, never on a timer.
- The theme file format does not change: statuses use `theme.tree.{added, modified, deleted, untracked}`; the viewer's gutter uses the same four. The workspace file's `sidebar` object gains optional fields with defaults.
- Display name "Sarkos IDE"; lowercase `sarkos` elsewhere. No em-dashes in any copy or comment. Commit messages imperative, one line, body optional, ending with a blank line, `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_015xrVD54zbvkgksJUndxfpr`.
- Tests: `cargo test -p sarkos` stays headless; tests that need a repository make one in a temp dir with `git init` and take `client::fork_lock()` around anything that spawns a process; the build stays warning-free for the project's crates; `cargo` is at `~/.cargo/bin/cargo`.

## Review Focus

1. **A project that is not a git repository.** The tree shows no colours, the Worktrees section says "not a git repository" once, no banner, no repeated git spawns every ten seconds. Pinned by Task 2's `status_outside_a_repository_is_empty_and_marks_the_root_as_not_git` and Task 3's `worktrees_outside_a_repository_is_none`.
2. **A repository with tens of thousands of untracked files.** `git status` must not list each one (`--untracked-files=normal` collapses directories), and its run must be background and sequenced so a slow one never lands over a newer one. Pinned by Task 2's `an_untracked_directory_marks_the_directory_not_its_files` and `a_stale_status_result_is_dropped`.
3. **A file opened from a worktree, not the main checkout.** Its diff base is that worktree's HEAD, found from the file's own directory. Pinned by Task 5's `the_base_comes_from_the_files_own_worktree`.
4. **A worktree whose directory was deleted outside the IDE.** It lists as missing (git marks it prunable), cannot be expanded, and "Remove, force" removes it. Pinned by Task 3's `a_prunable_worktree_is_listed_as_missing`.
5. **Deleted lines before the first line, and a file with no changes.** The red triangle for a deletion at the top paints at line 1's top edge; a clean file shows no marks and the inline diff toggle does nothing. Pinned by Task 5's `marks_for_a_deletion_at_the_top_and_a_clean_file`.

---

## File structure

```
crates/sarkos/
  Cargo.toml                   similar (Task 5)
  src/sidebar.rs               Task 1: the Root sidebar methods move in; Task 4: the Worktrees section, shimmer, ease-in, prompt, per-worktree menu
  src/viewer.rs                Task 1: the Root viewer methods move in; Task 5: DiffMarks on the viewer, the inline toggle
  src/gitstatus.rs             NEW (Task 2): Status, parse_porcelain_v2, run_status, dirty_dirs
  src/explorer.rs              Task 2: status map on the model, Row.status and Row.dirty
  src/explorer_watch.rs        Task 2: the status refresh (debounced on change, slow timer)
  src/worktrees.rs             NEW (Task 3): Worktree, parse_worktree_list, list, create, remove, the model with expanded and changes
  src/diff.rs                  NEW (Task 5): Mark, DiffMarks, compute(base, lines), base_for(path) through git
  src/viewer_element.rs        Task 5: gutter marks and the inline deleted rows
  src/workspace.rs             Task 4: SidebarSaved.{explorer_open, worktrees_open, worktrees_expanded}
  src/keymap.rs                Task 4: "focus worktrees" ctrl-shift-g; Task 5: "toggle inline diff" cmd-shift-d (Viewer context)
  src/command_palette.rs       the two rows (through ACTIONS)
  src/main.rs                  Task 1 shrinks it; later tasks add fields and the command arms only
  README.md                    Tasks 4 and 5: the Sidebar and viewer section, keyboard rows, checklist
```

Rulings the plan makes where the spec is silent or its letter costs more than its intent (the executor records any further one in the ledger):

1. `git status` runs with `--untracked-files=normal --no-renames --ignored=no`: a directory of untracked files is one entry (marked untracked, its files inherit the colour when listed), a rename shows as a deletion and an addition, ignored entries keep coming from `check-ignore`.
2. The status is refreshed at most once a second after the poll sees a change, and every ten seconds otherwise; an expanded worktree's status refreshes on the same slow timer. Collapsed worktrees never run status.
3. Modified lines in the gutter use the tree's modified colour (amber in Navy), not the spec's blue: the theme format is fixed and blue is the focus ring's colour; the tree and the gutter then agree. Untracked files get no gutter marks (nothing to compare), like VS Code.
4. The diff base is `HEAD:<path relative to the file's worktree root>`, the root found with `git rev-parse --show-toplevel` run from the file's directory, so a file opened from another worktree diffs against that worktree. The diff recomputes whenever the viewer reloads and on the ten-second timer (HEAD may move).
5. Creating a worktree: the prompt asks for the branch, then the path (defaulting to `<project>-<branch>` beside the project); an existing branch is checked out, a new one is created with `-b`. Removing: refused with a notice when the worktree has changes, unless the user picks "Remove worktree, force". The main checkout is first and has no remove items.
6. The inline diff interleaves deleted lines as rows in the deleted colour with a faint tint, and the scroll position counts display rows; find hits are on current lines only.
7. The Worktrees section's loading shimmer is a dim band sweeping the row at 1.5 s per sweep, and changed rows fade in over 150 ms; both are static under Reduce Motion.

---

### Task 1: Move the sidebar and viewer code out of main.rs

**Files:**
- Modify: `crates/sarkos/src/main.rs`, `crates/sarkos/src/sidebar.rs`, `crates/sarkos/src/viewer.rs`
- Test: the existing suite (no behaviour change); `cargo test -p sarkos` green before and after

**Interfaces:**
- Consumes: the `impl Root` methods in `main.rs`: `load_pending_dirs`, `open_viewer`, `escape_closes_menu`, `tree_key`, `watched_paths`, `paths_changed`, `OpenPlan`, `open_plan`, `after_close`, `after_drop`, `keeps_tree_focus` (sidebar side); `build_highlighter`, `viewer_for`, `load_viewer`, `reload_viewers_for`, `reload_all_viewers`, `load`, `active_viewer`, `should_apply` (viewer side).
- Produces: the same names, now `impl Root` blocks in `sidebar.rs` and `viewer.rs` (Rust allows `impl Root` in any module of the crate), with their tests moved alongside. `main.rs` loses about 300 lines.

- [ ] **Step 1: Record the baseline**

Run: `~/.cargo/bin/cargo test -p sarkos 2>&1 | grep "test result"` and `wc -l crates/sarkos/src/main.rs`
Expected: the pass count (write it down) and about 2190 lines.

- [ ] **Step 2: Move the sidebar block**

Cut from `main.rs` the free functions `OpenPlan`, `open_plan`, `after_close`, `after_drop`, `keeps_tree_focus` and the `impl Root` methods `load_pending_dirs`, `open_viewer`, `escape_closes_menu`, `tree_key`, `watched_paths`, `paths_changed` (keep `watched_paths`'s viewer line: it reads `self.viewers`), and paste them into `sidebar.rs` as:

```rust
impl Root {
    // (the methods, unchanged, with their doc comments)
}
```

plus the free functions at module level, `pub` where `main.rs` still calls them (`keeps_tree_focus` from `run`, `after_close` from `close_tab`, `after_drop` from `drop_tab`). Move their tests (`a_preview_is_replaced_and_a_pin_is_kept`, `only_the_sidebar_commands_and_the_palette_keep_the_keys_in_the_tree`, the `after_close`/`after_drop` tests) into `sidebar.rs`'s `mod tests`. Fix imports in both files (`use crate::sidebar::{open_plan, OpenPlan, ...}` in `main.rs` where used; `use crate::Root;` already in `sidebar.rs`).

- [ ] **Step 3: Move the viewer block**

Cut from `main.rs` `should_apply` and the `impl Root` methods `build_highlighter`, `viewer_for`, `load_viewer`, `reload_viewers_for`, `reload_all_viewers`, `load`, `active_viewer`, and paste them into `viewer.rs` as an `impl Root` block (`use crate::Root;`, `use crate::tree::TabId;`, `use gpui::{App, Context, Entity};` as needed). Move `should_apply`'s test. Keep `Root::set_theme`'s call to `reload_all_viewers` and `main`'s `build_highlighter` call working (make the methods `pub(crate)`).

- [ ] **Step 4: Verify nothing changed**

Run: `~/.cargo/bin/cargo test -p sarkos 2>&1 | grep "test result"`, `~/.cargo/bin/cargo build --release 2>&1 | grep -i "^warning"`, `wc -l crates/sarkos/src/main.rs`
Expected: the same pass count as Step 1; no warnings for the project's crates; `main.rs` under 1900 lines. `git diff --stat` shows only moves (a `git diff --color-moved=zebra` review is the proof: every removed line reappears added).

- [ ] **Step 5: Commit**

```bash
git add crates/sarkos/src/main.rs crates/sarkos/src/sidebar.rs crates/sarkos/src/viewer.rs
git commit -m "Move the sidebar and viewer code out of main.rs into their modules"
```

---

### Task 2: Git status in the file tree

**Files:**
- Create: `crates/sarkos/src/gitstatus.rs`
- Modify: `crates/sarkos/src/explorer.rs` (`Explorer.status`, `Row.{status, dirty}`), `crates/sarkos/src/explorer_watch.rs` (the refresh schedule), `crates/sarkos/src/sidebar.rs` (row colours and the folder dot; `refresh_status` on `Root`), `crates/sarkos/src/main.rs` (`mod gitstatus;`, the `status_seq` field)
- Test: unit tests in `gitstatus.rs` (parser, a temp repository), `explorer.rs`, `explorer_watch.rs`

**Interfaces:**
- Consumes: `explorer::git(root) -> Command`, `Explorer`, `Row`, `theme.tree.*`, the poll in `explorer_watch`.
- Produces: `gitstatus::{Status::{Added, Modified, Deleted, Untracked, Conflicted}, parse_porcelain_v2(bytes: &[u8]) -> Vec<(PathBuf, Status)>, run_status(root: &Path) -> Option<Vec<(PathBuf, Status)>>, dirty_dirs(root, &[(PathBuf, Status)]) -> HashSet<PathBuf>}`, `Explorer::{set_status(entries), status_of(&Path) -> Option<Status>, is_dirty_dir(&Path) -> bool, not_git: bool}`, `Row.status: Option<Status>`, `Row.dirty: bool`, `Root::refresh_status(cx)`, `explorer_watch::STATUS_EVERY`.

- [ ] **Step 1: Write the failing tests**

Create `crates/sarkos/src/gitstatus.rs`:

```rust
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
    fn status_outside_a_repository_is_empty_and_marks_the_root_as_not_git() {
        let _lock = crate::client::fork_lock();
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(run_status(dir.path()), None);
    }
}
```

In `explorer.rs`'s tests:

```rust
    #[test]
    fn rows_carry_their_status_and_folders_with_changes_a_dot() {
        use crate::gitstatus::Status;
        let dir = project();
        let root = dir.path().to_path_buf();
        let mut ex = Explorer::new(root.clone(), vec![root.join("src")], false);
        ex.set_loaded(&root, list_dir(&root).unwrap());
        ex.set_loaded(&root.join("src"), list_dir(&root.join("src")).unwrap());
        ex.set_status(Some(vec![(PathBuf::from("src/main.rs"), Status::Modified), (PathBuf::from("a.txt"), Status::Untracked)]));
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
        ex.set_status(Some(vec![(PathBuf::from("src"), Status::Untracked)]));
        assert_eq!(ex.rows().iter().find(|r| r.name == "main.rs").unwrap().status, Some(Status::Untracked));
        ex.set_status(None);
        assert!(ex.not_git);
        assert!(ex.rows().iter().all(|r| r.status.is_none() && !r.dirty));
    }
```

In `explorer_watch.rs`'s tests:

```rust
    #[test]
    fn the_status_refresh_is_due_a_second_after_a_change_or_every_ten_seconds() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        assert!(!status_due(Some(t0), None, t0 + Duration::from_millis(500)), "no change, not ten seconds yet");
        assert!(status_due(Some(t0), Some(t0 + Duration::from_millis(200)), t0 + Duration::from_millis(1300)), "a change over a second ago");
        assert!(!status_due(Some(t0), Some(t0 + Duration::from_millis(900)), t0 + Duration::from_millis(1300)), "a change under a second ago waits");
        assert!(status_due(Some(t0), None, t0 + STATUS_EVERY + Duration::from_millis(1)), "the slow timer");
        assert!(status_due(None, None, t0), "never run: run now");
    }

    #[test]
    fn a_stale_status_result_is_dropped() {
        // The window bumps `status_seq` per run and applies a result only when it is the newest.
        assert!(status_result_applies(3, 3));
        assert!(!status_result_applies(2, 3), "an older run landing after a newer one started");
        assert!(!status_result_applies(4, 3), "a run the window does not know is ignored too");
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkos -- gitstatus explorer explorer_watch`
Expected: FAIL to compile (add `mod gitstatus;` to `main.rs`).

- [ ] **Step 3: Implement**

`gitstatus.rs`, above the tests:

```rust
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
            "u" => (Status::Conflicted, rest.splitn(11, ' ').nth(10).unwrap_or("")),
            "1" => {
                let mut f = rest.splitn(9, ' ');
                let xy = f.next().unwrap_or("..");
                let path = f.nth(7).unwrap_or("");
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

/// The repository's changes, relative to `root`. `None` outside a repository (or without git).
/// Untracked directories are one entry each; renames show as a deletion and an addition;
/// ignored entries are not listed (the tree asks `check-ignore` for those).
pub fn run_status(root: &Path) -> Option<Vec<(PathBuf, Status)>> {
    let out = git(root)
        .args(["status", "--porcelain=v2", "-z", "--untracked-files=normal", "--no-renames", "--ignored=no"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(parse_porcelain_v2(&out.stdout))
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
```

`explorer.rs`: `Explorer` gains `status: HashMap<PathBuf, Status>` (absolute paths), `dirty: HashSet<PathBuf>`, `pub not_git: bool`; `Row` gains `pub status: Option<Status>`, `pub dirty: bool`;

```rust
    /// The repository's changes, or `None` outside a repository.
    pub fn set_status(&mut self, entries: Option<Vec<(PathBuf, Status)>>) {
        match entries {
            None => {
                self.not_git = true;
                self.status.clear();
                self.dirty.clear();
            }
            Some(entries) => {
                self.not_git = false;
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
```

`push_rows` fills `status: self.status_of(&e.path)` and `dirty: e.is_dir && self.is_dirty_dir(&e.path)`.

`explorer_watch.rs`:

```rust
pub const STATUS_EVERY: Duration = Duration::from_secs(10);
const STATUS_DEBOUNCE: Duration = Duration::from_secs(1);

/// Whether to run `git status` now: never run yet, a change seen at least a second ago and not
/// yet reflected, or ten seconds since the last run.
pub fn status_due(last_run: Option<Instant>, changed_at: Option<Instant>, now: Instant) -> bool {
    match (last_run, changed_at) {
        (None, _) => true,
        (Some(run), Some(changed)) if changed > run => now.duration_since(changed) >= STATUS_DEBOUNCE,
        (Some(run), _) => now.duration_since(run) >= STATUS_EVERY,
    }
}
```

```rust
/// Whether a `git status` result belongs to the newest run the window started.
pub fn status_result_applies(seq: u64, current: u64) -> bool {
    seq == current
}
```

(`refresh_status` applies a result only when `status_result_applies(seq, root.status_seq)`.) Also, `start`'s loop keeps `last_run: Option<Instant>` and `changed_at: Option<Instant>` (set to `now` whenever `changed` is non-empty, plus whenever `root.explorer.not_git` is false and the project root itself changed); each tick, after the fingerprint step, `if status_due(last_run, changed_at, Instant::now()) { last_run = Some(Instant::now()); root.update(cx, |root, cx| root.refresh_status(cx)) }`. A project that is not a repository (`explorer.not_git` after the first run) still re-checks every `STATUS_EVERY` (the user may `git init`), which is one process per ten seconds.

`sidebar.rs` (`impl Root`):

```rust
    /// Runs `git status` for the project on the background executor; a result older than a
    /// newer run is dropped.
    pub fn refresh_status(&mut self, cx: &mut Context<Self>) {
        self.status_seq += 1;
        let seq = self.status_seq;
        let root = self.project.clone();
        cx.spawn(async move |this, cx| {
            let entries = cx.background_executor().spawn(async move { crate::gitstatus::run_status(&root) }).await;
            let _ = this.update(cx, |root, cx| {
                if crate::explorer_watch::status_result_applies(seq, root.status_seq) {
                    root.explorer.set_status(entries);
                    cx.notify();
                }
            });
        })
        .detach();
    }
```

(`Root.status_seq: u64`, new field.) `render_rows`: the row's text colour is `theme.tree.added/modified/deleted/untracked` by `row.status` (conflicted uses `deleted`), ignored stays `ignored`, else `text`; after the name, a dirty directory shows `div().flex_none().size(px(6.0)).rounded_full().bg(theme.tree.modified.hsla())`. A test for the colour choice as a pure `fn row_color(status, ignored, theme) -> Rgb` with the five cases.

- [ ] **Step 4: Run the tests**

Run: `~/.cargo/bin/cargo test -p sarkos -- gitstatus explorer sidebar`
Expected: PASS. Then `~/.cargo/bin/cargo build --release 2>&1 | grep -i "^warning"`: nothing from the project's crates.

- [ ] **Step 5: Commit**

```bash
git add crates/sarkos/src/gitstatus.rs crates/sarkos/src/explorer.rs crates/sarkos/src/explorer_watch.rs crates/sarkos/src/sidebar.rs crates/sarkos/src/main.rs
git commit -m "Colour the file tree by git status and dot the folders with changes"
```

---

### Task 3: The worktree model: list, expand with status, create, remove

**Files:**
- Create: `crates/sarkos/src/worktrees.rs`
- Modify: `crates/sarkos/src/main.rs` (`mod worktrees;`, `Root.worktrees`, the refresh hooks), `crates/sarkos/src/sessions.rs` (`create_in`), `crates/sarkos/src/workspace.rs` (`SidebarSaved.worktrees_expanded`)
- Test: unit tests in `worktrees.rs` (parser, a temp repository with a second worktree), `sessions.rs`, `workspace.rs`

**Interfaces:**
- Consumes: `explorer::git`, `gitstatus::{run_status, Status}`, `SessionManager::create`.
- Produces: `worktrees::{Worktree { path, head: String, branch: Option<String>, is_main: bool, missing: bool }, parse_worktree_list(&str) -> Vec<Worktree>, list(root) -> Option<Vec<Worktree>>, create(root, branch, path) -> Result<(), String>, remove(root, path, force) -> Result<(), String>, default_path(root, branch) -> PathBuf, Changes = Vec<(PathBuf, Status)>, Worktrees { list: Option<Vec<Worktree>>, not_git: bool, expanded: BTreeSet<PathBuf>, changes: HashMap<PathBuf, Changes>, loading: HashSet<PathBuf>, opened_at: HashMap<PathBuf, Instant>, selected: Option<WorktreeRow> }, Worktrees::{new(expanded), set_list(Option<Vec<Worktree>>), toggle(&Path) -> bool, set_changes(&Path, Option<Changes>), to_load() -> Vec<PathBuf>, mark_loading, rows() -> Vec<WtRow>, dirty_count(&Path) -> Option<usize>}`, `WtRow::{Worktree { wt: Worktree, expanded, loading, dirty: Option<usize> }, Change { worktree: PathBuf, rel: PathBuf, status }}`, `SessionManager::create_in(tab, note, cwd)`, `Root::{refresh_worktrees(cx), load_pending_worktrees(cx)}`.

- [ ] **Step 1: Write the failing tests**

Create `crates/sarkos/src/worktrees.rs`:

```rust
//! The worktree explorer's model (spec 6): `git worktree list` only, until a worktree is
//! expanded, which runs `git status` for it on the background executor; collapsing drops the
//! result. Three actions, each one git command the user chose by name. Pure apart from the
//! four functions that run git.

use crate::explorer::git;
use crate::gitstatus::{run_status, Status};
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
}

pub type Changes = Vec<(PathBuf, Status)>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_porcelain_list_is_parsed_with_main_first_branches_and_prunable() {
        let text = "worktree /Users/me/proj\nHEAD 0123456789abcdef0123456789abcdef01234567\nbranch refs/heads/main\n\n\
worktree /Users/me/proj-feature\nHEAD 89abcdef0123456789abcdef0123456789abcdef\nbranch refs/heads/feature/x\n\n\
worktree /Users/me/proj-detached\nHEAD aaaaaaaabbbbbbbbccccccccddddddddeeeeeeee\ndetached\n\n\
worktree /Users/me/gone\nHEAD bbbbbbbbccccccccddddddddeeeeeeeeffffffff\nbranch refs/heads/old\nprunable gitdir file points to non-existent location\n\n";
        let list = parse_worktree_list(text);
        assert_eq!(list.len(), 4);
        assert_eq!(list[0], Worktree { path: "/Users/me/proj".into(), head: "01234567".into(), branch: Some("main".into()), is_main: true, missing: false });
        assert_eq!(list[1].branch.as_deref(), Some("feature/x"));
        assert!(!list[1].is_main);
        assert_eq!(list[2].branch, None, "detached");
        assert!(list[3].missing, "prunable");
        assert!(parse_worktree_list("").is_empty());
    }

    #[test]
    fn default_path_sits_beside_the_project_named_after_the_branch() {
        assert_eq!(default_path(Path::new("/Users/me/proj"), "feature/x"), PathBuf::from("/Users/me/proj-feature-x"));
        assert_eq!(default_path(Path::new("/Users/me/proj"), "fix"), PathBuf::from("/Users/me/proj-fix"));
    }

    #[test]
    fn rows_list_worktrees_and_the_changes_of_the_expanded_ones() {
        let main = Worktree { path: "/p".into(), head: "1".into(), branch: Some("main".into()), is_main: true, missing: false };
        let feat = Worktree { path: "/p-f".into(), head: "2".into(), branch: Some("f".into()), is_main: false, missing: false };
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
        assert!(!w.toggle(Path::new("/p-f")), "collapsed");
        assert_eq!(w.rows().len(), 2);
        assert_eq!(w.dirty_count(Path::new("/p-f")), None, "collapsing drops the result");
        w.set_list(None);
        assert!(w.not_git);
        assert!(w.rows().is_empty());
    }

    #[test]
    fn a_missing_worktree_cannot_be_expanded() {
        let gone = Worktree { path: "/gone".into(), head: "3".into(), branch: None, is_main: false, missing: true };
        let mut w = Worktrees::new(vec![]);
        w.set_list(Some(vec![gone]));
        assert!(!w.toggle(Path::new("/gone")));
        assert!(w.to_load().is_empty());
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            let st = git(dir.path()).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap();
            assert!(st.success(), "git {args:?}");
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "t@example.com"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
        run(&["add", "a.txt"]);
        run(&["commit", "-q", "-m", "init"]);
        dir
    }

    #[test]
    fn list_create_status_and_remove_round_trip_on_a_real_repository() {
        let _lock = crate::client::fork_lock();
        let dir = repo();
        let root = dir.path().canonicalize().unwrap();
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
    fn a_prunable_worktree_is_listed_as_missing() {
        let _lock = crate::client::fork_lock();
        let dir = repo();
        let root = dir.path().canonicalize().unwrap();
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
```

In `sessions.rs`'s tests, next to `create_uses_the_configured_shell_and_nice`:

```rust
    #[test]
    fn create_in_starts_the_shell_in_the_given_folder() {
        let rec = Recorder::new();
        let mut m = SessionManager::new(rec.clone(), None, PathBuf::from("/p"));
        m.create_in(7, false, PathBuf::from("/elsewhere"));
        assert!(matches!(&rec.requests()[0], Request::CreateSession { cwd, .. } if cwd == Path::new("/elsewhere")));
    }
```

In `workspace.rs`'s `the_sidebar_state_round_trips_and_is_optional`, the `SidebarSaved` literal gains `worktrees_expanded: vec![PathBuf::from("/p-f")]`, and a file with a `sidebar` object lacking the field loads with an empty list (write one such JSON and assert).

- [ ] **Step 2: Run them to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkos -- worktrees create_in sidebar_state`
Expected: FAIL to compile (add `mod worktrees;`).

- [ ] **Step 3: Implement**

`worktrees.rs`:

```rust
/// `git worktree list --porcelain`: blank-line separated blocks of `worktree <path>`,
/// `HEAD <sha>`, `branch refs/heads/<name>` or `detached`, and optional `bare`, `locked`,
/// `prunable <reason>`. The first block is the main checkout.
pub fn parse_worktree_list(text: &str) -> Vec<Worktree> {
    let mut out = Vec::new();
    for block in text.split("\n\n").filter(|b| !b.trim().is_empty()) {
        let mut path = None;
        let mut head = String::new();
        let mut branch = None;
        let mut missing = false;
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(p));
            } else if let Some(h) = line.strip_prefix("HEAD ") {
                head = h.chars().take(8).collect();
            } else if let Some(b) = line.strip_prefix("branch ") {
                branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
            } else if line.starts_with("prunable") {
                missing = true;
            }
        }
        if let Some(path) = path {
            let is_main = out.is_empty();
            out.push(Worktree { path, head, branch, is_main, missing });
        }
    }
    out
}

/// `None` outside a repository.
pub fn list(root: &Path) -> Option<Vec<Worktree>> {
    let out = git(root).args(["worktree", "list", "--porcelain"]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
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
    git(root).args(["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

/// `git worktree add <path> <branch>`, creating the branch when it does not exist yet.
pub fn create(root: &Path, branch: &str, path: &Path) -> Result<(), String> {
    let path = path.to_string_lossy();
    if branch_exists(root, branch) {
        run(root, &["worktree", "add", &path, branch])
    } else {
        run(root, &["worktree", "add", "-b", branch, &path])
    }
}

/// `git worktree remove [--force] <path>`. Without force git refuses a worktree with changes;
/// the error is git's own text.
pub fn remove(root: &Path, path: &Path, force: bool) -> Result<(), String> {
    let path = path.to_string_lossy();
    if force {
        run(root, &["worktree", "remove", "--force", &path])
    } else {
        run(root, &["worktree", "remove", &path])
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
    /// When each expanded worktree's changes landed, for the ease-in.
    pub opened_at: HashMap<PathBuf, Instant>,
    /// The keyboard's row, by index into `rows()`.
    pub selected: Option<usize>,
}

impl Worktrees {
    pub fn new(expanded: Vec<PathBuf>) -> Self {
        Worktrees { list: None, not_git: false, expanded: expanded.into_iter().collect(), changes: HashMap::new(), loading: HashSet::new(), opened_at: HashMap::new(), selected: None }
    }

    pub fn set_list(&mut self, list: Option<Vec<Worktree>>) {
        self.not_git = list.is_none();
        self.list = list;
        if let Some(l) = &self.list {
            // A worktree that went away takes its state with it.
            let known: HashSet<&Path> = l.iter().map(|w| w.path.as_path()).collect();
            self.expanded.retain(|p| known.contains(p.as_path()));
            self.changes.retain(|p, _| known.contains(p.as_path()));
        }
    }

    pub fn find(&self, path: &Path) -> Option<&Worktree> {
        self.list.as_ref()?.iter().find(|w| w.path == path)
    }

    /// Expands or collapses; a missing worktree stays collapsed. Collapsing drops its changes.
    pub fn toggle(&mut self, path: &Path) -> bool {
        if self.expanded.remove(path) {
            self.changes.remove(path);
            self.loading.remove(path);
            self.opened_at.remove(path);
            return false;
        }
        if self.find(path).is_some_and(|w| w.missing) {
            return false;
        }
        self.expanded.insert(path.to_path_buf());
        true
    }

    pub fn to_load(&self) -> Vec<PathBuf> {
        self.expanded.iter().filter(|p| !self.changes.contains_key(*p) && !self.loading.contains(*p)).cloned().collect()
    }

    pub fn mark_loading(&mut self, path: &Path) {
        self.loading.insert(path.to_path_buf());
    }

    /// The status result for an expanded worktree; `None` when git failed (shown as no changes).
    pub fn set_changes(&mut self, path: &Path, changes: Option<Changes>) {
        self.loading.remove(path);
        if !self.expanded.contains(path) {
            return;
        }
        self.changes.insert(path.to_path_buf(), changes.unwrap_or_default());
        self.opened_at.entry(path.to_path_buf()).or_insert_with(Instant::now);
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
```

`sessions.rs`: `create_in(&mut self, tab: TabId, note: bool, cwd: PathBuf)` is `create` with the given cwd (refactor `create` to call `create_in(tab, note, self.project.clone())`).

`workspace.rs`: `SidebarSaved` gains `#[serde(default)] pub worktrees_expanded: Vec<PathBuf>`.

`main.rs` (`Root`): `pub worktrees: worktrees::Worktrees` built from the saved list; `save()` fills `worktrees_expanded: self.worktrees.expanded.iter().cloned().collect()`; `pub worktrees_seq: u64`;

```rust
    /// Runs `git worktree list` on the background executor.
    pub fn refresh_worktrees(&mut self, cx: &mut Context<Self>) {
        self.worktrees_seq += 1;
        let seq = self.worktrees_seq;
        let root = self.project.clone();
        cx.spawn(async move |this, cx| {
            let list = cx.background_executor().spawn(async move { worktrees::list(&root) }).await;
            let _ = this.update(cx, |root, cx| {
                if seq == root.worktrees_seq {
                    root.worktrees.set_list(list);
                    root.load_pending_worktrees(cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Runs `git status` for each expanded worktree whose changes are not loaded.
    pub fn load_pending_worktrees(&mut self, cx: &mut Context<Self>) {
        for path in self.worktrees.to_load() {
            self.worktrees.mark_loading(&path);
            let p = path.clone();
            cx.spawn(async move |this, cx| {
                let changes = cx.background_executor().spawn(async move { crate::gitstatus::run_status(&p) }).await;
                let _ = this.update(cx, |root, cx| {
                    root.worktrees.set_changes(&path, changes);
                    cx.notify();
                });
            })
            .detach();
        }
    }
```

`explorer_watch`'s status tick also calls `root.refresh_worktrees(cx)` on the slow timer and, for expanded worktrees, drops their `changes` (`worktrees.changes.clear()`, keeping `expanded`) so `load_pending_worktrees` reloads them, without touching `opened_at` (no second ease-in). `main` calls `refresh_worktrees` once after the window opens, next to the first `load_pending_dirs`.

- [ ] **Step 4: Run the tests**

Run: `~/.cargo/bin/cargo test -p sarkos -- worktrees sessions workspace`
Expected: PASS (the repository tests need `git` on PATH and create worktrees beside the temp dir, inside the temp root's parent: use a nested `tempdir` as the parent so nothing lands outside it). Warning check clean (items unused until Task 4 need an allowance naming Task 4).

- [ ] **Step 5: Commit**

```bash
git add crates/sarkos/src/worktrees.rs crates/sarkos/src/sessions.rs crates/sarkos/src/workspace.rs crates/sarkos/src/main.rs crates/sarkos/src/explorer_watch.rs
git commit -m "Model the worktree explorer: list, lazy status on expand, create and remove through git"
```

---

### Task 4: The Worktrees section: rows, shimmer, ease-in, actions, prompt, Ctrl+Shift+G

**Files:**
- Modify: `crates/sarkos/src/sidebar.rs` (two collapsible sections, the worktree rows, the per-worktree menu, the create prompt, `wt_key`), `crates/sarkos/src/keymap.rs` ("focus worktrees" ctrl-shift-g), `crates/sarkos/src/config.rs` (the comment line), `crates/sarkos/src/command_palette.rs` (through `ACTIONS`), `crates/sarkos/src/main.rs` (`Command::FocusWorktrees`, `Root.{wt_focused, wt_prompt}`, `sidebar_focused` handling), `crates/sarkos/src/workspace.rs` (`explorer_open`, `worktrees_open`), `README.md`
- Test: unit tests in `sidebar.rs` (pure: shimmer phase, ease, prompt state machine, menu items), `keymap.rs`, `workspace.rs`

**Interfaces:**
- Consumes: Task 3's `Worktrees`, `WtRow`, `create`, `remove`, `default_path`; `SessionManager::create_in`; `Root::open_viewer`; `motion::reduce_motion`; `LineEdit`.
- Produces: `sidebar::{shimmer_phase(elapsed) -> f32, ease_in(elapsed) -> f32, SHIMMER_PERIOD, EASE_IN, WtMenuAction::{OpenTerminal(PathBuf), Remove(PathBuf), RemoveForce(PathBuf)}, worktree_menu_items(&Worktree, dirty: Option<usize>) -> Vec<(&'static str, WtMenuAction)>, Prompt { branch: LineEdit, path: LineEdit, stage: Stage::{Branch, Path}, error: Option<String> }, Prompt::{new(), key(ks, root) -> PromptStep::{Stay, Cancel, Create { branch, path }}}}`, `Root::{wt_focused: bool, wt_prompt: Option<Prompt>, wt_key(ks, cx), open_terminal_in(path, cx), remove_worktree(path, force, cx), create_worktree(branch, path, cx)}`, `Command::FocusWorktrees`, `SidebarSaved.{explorer_open, worktrees_open}`.

- [ ] **Step 1: Write the failing tests**

In `sidebar.rs`'s tests:

```rust
    #[test]
    fn the_shimmer_sweeps_once_per_period_and_the_ease_in_settles() {
        use std::time::Duration;
        assert!((shimmer_phase(Duration::ZERO) - 0.0).abs() < 1e-5);
        assert!((shimmer_phase(SHIMMER_PERIOD / 2) - 0.5).abs() < 1e-5);
        assert!((shimmer_phase(SHIMMER_PERIOD + SHIMMER_PERIOD / 4) - 0.25).abs() < 1e-5, "periodic");
        assert_eq!(ease_in(Duration::ZERO), 0.0);
        assert_eq!(ease_in(EASE_IN), 1.0);
        assert_eq!(ease_in(EASE_IN * 3), 1.0, "settled");
        let mid = ease_in(EASE_IN / 2);
        assert!(mid > 0.5 && mid < 1.0, "ease-out: past half way at half time");
    }

    #[test]
    fn the_worktree_menu_offers_a_terminal_and_removal_but_never_for_the_main_checkout() {
        use crate::worktrees::Worktree;
        let main = Worktree { path: "/p".into(), head: "1".into(), branch: Some("main".into()), is_main: true, missing: false };
        let feat = Worktree { path: "/p-f".into(), head: "2".into(), branch: Some("f".into()), is_main: false, missing: false };
        let labels = |items: Vec<(&'static str, WtMenuAction)>| items.into_iter().map(|(l, _)| l).collect::<Vec<_>>();
        assert_eq!(labels(worktree_menu_items(&main, Some(0))), ["Open terminal here"]);
        assert_eq!(labels(worktree_menu_items(&feat, Some(0))), ["Open terminal here", "Remove worktree"]);
        assert_eq!(labels(worktree_menu_items(&feat, Some(3))), ["Open terminal here", "Remove worktree", "Remove worktree, force"], "dirty: the plain remove will be refused, so force is offered");
        assert_eq!(labels(worktree_menu_items(&feat, None)), ["Open terminal here", "Remove worktree", "Remove worktree, force"], "not loaded: cannot tell, offer both");
        let gone = Worktree { missing: true, ..feat.clone() };
        assert_eq!(labels(worktree_menu_items(&gone, None)), ["Remove worktree, force"], "nothing to open; only force removes a missing one");
    }

    #[test]
    fn the_create_prompt_asks_for_the_branch_then_the_path() {
        use gpui::Keystroke;
        let key = |k: &str| Keystroke::parse(k).unwrap();
        let root = std::path::Path::new("/Users/me/proj");
        let mut p = Prompt::new();
        assert_eq!(p.stage, Stage::Branch);
        assert_eq!(p.key(&key("enter"), root), PromptStep::Stay, "an empty branch does not advance");
        assert!(p.error.is_some());
        for c in "feat/x".chars() {
            p.key(&key(&c.to_string()), root);
        }
        assert_eq!(p.key(&key("enter"), root), PromptStep::Stay);
        assert_eq!(p.stage, Stage::Path);
        assert_eq!(p.path.text, "/Users/me/proj-feat-x", "the default path is filled in");
        assert_eq!(p.key(&key("enter"), root), PromptStep::Create { branch: "feat/x".into(), path: "/Users/me/proj-feat-x".into() });
        let mut q = Prompt::new();
        assert_eq!(q.key(&key("escape"), root), PromptStep::Cancel);
    }
```

(`Keystroke::parse` exists in gpui 0.2.2; `LineEdit::handle` takes a `&Keystroke` and types the key when it is a single character: check how `line_edit.rs` turns a keystroke into text (`ks.key_char` or `ks.key`) and feed the test accordingly.)

`keymap.rs`'s `SPEC_ROWS` gains `("ctrl-shift-g", "focus worktrees", "FocusWorktrees")` after "show ignored files" (the unbound counts stay: this one has keys). `workspace.rs`'s round-trip test adds `explorer_open: true, worktrees_open: false` to the literal and asserts both default to `true` on a file without them.

- [ ] **Step 2: Run them to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkos -- sidebar keymap workspace`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`sidebar.rs` additions:

```rust
pub const SHIMMER_PERIOD: Duration = Duration::from_millis(1500);
pub const EASE_IN: Duration = Duration::from_millis(150);

/// Where the shimmer's band is, 0 to 1 across the row, sweeping once per period.
pub fn shimmer_phase(elapsed: Duration) -> f32 {
    (elapsed.as_secs_f32() % SHIMMER_PERIOD.as_secs_f32()) / SHIMMER_PERIOD.as_secs_f32()
}

/// Opacity of a changed row `elapsed` after its worktree's changes landed: ease-out to 1.
pub fn ease_in(elapsed: Duration) -> f32 {
    let t = (elapsed.as_secs_f32() / EASE_IN.as_secs_f32()).clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WtMenuAction {
    OpenTerminal(PathBuf),
    Remove(PathBuf),
    RemoveForce(PathBuf),
}

/// The right-click menu of a worktree row. The main checkout is never removed; a dirty or
/// unloaded worktree offers force as well, since the plain removal will be refused.
pub fn worktree_menu_items(wt: &Worktree, dirty: Option<usize>) -> Vec<(&'static str, WtMenuAction)> {
    let mut v = Vec::new();
    if !wt.missing {
        v.push(("Open terminal here", WtMenuAction::OpenTerminal(wt.path.clone())));
    }
    if wt.is_main {
        return v;
    }
    if !wt.missing {
        v.push(("Remove worktree", WtMenuAction::Remove(wt.path.clone())));
    }
    if wt.missing || dirty.map_or(true, |n| n > 0) {
        v.push(("Remove worktree, force", WtMenuAction::RemoveForce(wt.path.clone())));
    }
    v
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Branch,
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptStep {
    Stay,
    Cancel,
    Create { branch: String, path: PathBuf },
}

/// The two-step prompt under the Worktrees header: a branch name, then the path (prefilled).
pub struct Prompt {
    pub branch: LineEdit,
    pub path: LineEdit,
    pub stage: Stage,
    pub error: Option<String>,
    pub focus: FocusHandle,
}

impl Prompt {
    pub fn new(cx: &mut Context<Root>) -> Self {
        Prompt { branch: LineEdit::new(), path: LineEdit::new(), stage: Stage::Branch, error: None, focus: cx.focus_handle() }
    }

    pub fn key(&mut self, ks: &Keystroke, root: &Path) -> PromptStep {
        let edit = match self.stage {
            Stage::Branch => &mut self.branch,
            Stage::Path => &mut self.path,
        };
        match edit.handle(ks) {
            Edit::Cancel => PromptStep::Cancel,
            Edit::Submit => match self.stage {
                Stage::Branch => {
                    let b = self.branch.text.trim().to_string();
                    if b.is_empty() || b.contains(char::is_whitespace) {
                        self.error = Some("a branch name, without spaces".into());
                        return PromptStep::Stay;
                    }
                    self.error = None;
                    self.path.set(&crate::worktrees::default_path(root, &b).to_string_lossy());
                    self.stage = Stage::Path;
                    PromptStep::Stay
                }
                Stage::Path => {
                    let p = self.path.text.trim();
                    if p.is_empty() {
                        self.error = Some("a path for the worktree".into());
                        return PromptStep::Stay;
                    }
                    PromptStep::Create { branch: self.branch.text.trim().to_string(), path: PathBuf::from(p) }
                }
            },
            _ => PromptStep::Stay,
        }
    }
}
```

(The test builds a `Prompt` without a context: give `Prompt` a `new_for_test()` under `#[cfg(test)]` that skips the focus handle by making `focus: Option<FocusHandle>`, or keep `focus` on `Root` instead (`Root.wt_prompt_focus: FocusHandle`, made once in `Root::new`), which is simpler: do that, and `Prompt::new()` takes nothing.)

`render` becomes two collapsible sections: the header gets a chevron and an `on_click` that flips `root.sidebar.explorer_open` / `root.sidebar.worktrees_open` (new `Sidebar` fields, saved as `SidebarSaved.{explorer_open, worktrees_open}` with `#[serde(default = "default_true")]`), and a "+" on the Worktrees header that opens the prompt (`root.wt_prompt = Some(Prompt::new()); root.wt_focused = true`). The Explorer's list is shown only when open (`.when(open, ..)`), and the two lists share the column: Explorer `flex_1`, Worktrees `flex_none` with a max height of 40% of the column (`.max_h(px(root.content.h * 0.4))`) scrolling its own rows (`overflow_y_scroll` with a second `ScrollHandle`: few rows, plain divs). The Worktrees rows:

- A worktree row: chevron (none when missing), the directory name, the branch in `text_dim` (or "detached <head>"), "missing" in `tree.deleted` when missing, and the dirty count on the right once loaded ("3 changes", `tree.modified`, or "clean" in `text_dim`). Click toggles (then `load_pending_worktrees` and save); right click opens the worktree menu (`Root.wt_menu: Option<(PathBuf, Point)>` rendered like the file menu through `worktree_menu_items`); while `loading`, the row's background carries the shimmer: a `div().absolute()` band 30% of the row's width at `left = phase * (1.3 * width) - 0.3 * width`, in `chrome.tab_inactive` at alpha 0.6, and `window.request_animation_frame()` while any worktree is loading and Reduce Motion is off (`shimmer_phase(Instant::now() - loading_since)`; keep `Worktrees.loading_since: HashMap<PathBuf, Instant>` set in `mark_loading`). Under Reduce Motion the band sits at the centre, static.
- A change row: indented, the relative path in the status colour (`row_color` from Task 2); click opens the viewer on `worktree.join(rel)` (preview; double click pins), with opacity `ease_in(now - opened_at)` while settling (request a frame while any `ease_in < 1.0` and Reduce Motion is off; under Reduce Motion, 1.0).
- The prompt, when open, under the header: two rows, "branch" then "path" (`line_edit::render`), the error in `tree.deleted` under them, Enter/Escape through `Prompt::key`.

`Root` methods (in `sidebar.rs`'s `impl Root`):

```rust
    pub fn open_terminal_in(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let pane = self.tree.focused;
        let idx = self.tree.add_tab(pane, Tab::Terminal { session: SessionId(0) });
        if let Some(id) = self.tree.tab_id(pane, idx) {
            self.sessions.create_in(id, false, dir);
        }
        self.save(cx);
        cx.notify();
    }

    pub fn create_worktree(&mut self, branch: String, path: PathBuf, cx: &mut Context<Self>) {
        let root = self.project.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { crate::worktrees::create(&root, &branch, &path) }).await;
            let _ = this.update(cx, |root, cx| {
                match result {
                    Ok(()) => root.wt_prompt = None,
                    Err(e) => {
                        if let Some(p) = &mut root.wt_prompt {
                            p.error = Some(e);
                        }
                    }
                }
                root.refresh_worktrees(cx);
            });
        })
        .detach();
    }

    pub fn remove_worktree(&mut self, path: PathBuf, force: bool, cx: &mut Context<Self>) {
        let root = self.project.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { crate::worktrees::remove(&root, &path, force) }).await;
            let _ = this.update(cx, |root, cx| {
                if let Err(e) = result {
                    root.show_notice(format!("cannot remove the worktree: {e}"), cx);
                }
                root.refresh_worktrees(cx);
            });
        })
        .detach();
    }

    /// Keys in the Worktrees section: Up/Down move, Enter or Right expands a worktree or opens a
    /// change pinned, Left collapses, Escape returns to the pane; with the prompt open, the keys
    /// go to it.
    pub fn wt_key(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.wt_prompt {
            match p.key(ks, &self.project.clone()) {
                PromptStep::Stay => {}
                PromptStep::Cancel => self.wt_prompt = None,
                PromptStep::Create { branch, path } => self.create_worktree(branch, path, cx),
            }
            cx.notify();
            return;
        }
        let rows = self.worktrees.rows();
        let sel = self.worktrees.selected;
        match ks.key.as_str() {
            "down" => self.worktrees.selected = Some(sel.map_or(0, |i| (i + 1).min(rows.len().saturating_sub(1)))),
            "up" => self.worktrees.selected = Some(sel.map_or(0, |i| i.saturating_sub(1))),
            "enter" | "right" | "left" | "space" => {
                if let Some(row) = sel.and_then(|i| rows.get(i)) {
                    match row {
                        WtRow::Worktree { wt, expanded, .. } => {
                            let want_open = ks.key != "left";
                            if *expanded != want_open {
                                self.worktrees.toggle(&wt.path);
                                self.load_pending_worktrees(cx);
                                self.save(cx);
                            }
                        }
                        WtRow::Change { worktree, rel, .. } => {
                            if ks.key == "enter" || ks.key == "space" {
                                let pin = ks.key == "enter";
                                self.open_viewer(&worktree.join(rel), pin, cx);
                                if !pin {
                                    self.wt_focused = true;
                                }
                                return;
                            }
                        }
                    }
                }
            }
            "escape" => self.wt_focused = false,
            _ => return,
        }
        cx.notify();
    }
```

`main.rs`: `Command::FocusWorktrees` ("Put keyboard focus in the worktree list") registered everywhere like `FocusFileTree`; `run`'s arm opens the sidebar and its Worktrees section if needed and sets `wt_focused = true` (`sidebar_focused = false`); `keeps_tree_focus` also keeps `FocusWorktrees`; the top-of-`run` clear resets `wt_focused` too, except for the same set; `render`'s focus arbitration: `wt_prompt` open → the prompt's focus (`Root.wt_prompt_focus`), else `wt_focused` → `Root.wt_focus` (a second `FocusHandle` on `Root`, made in `new`), else `sidebar_focused` → the tree, else as today; the Worktrees section div has `key_context("Worktrees")`, `track_focus(&root.wt_focus)` and `on_key_down(.. root.wt_key(..))`; the prompt div `track_focus(&root.wt_prompt_focus)` with the same key handler. `focus_pane` and `activate_tab` reset `wt_focused` as they do `sidebar_focused`. The config comment block gains `# "focus worktrees" = "ctrl-shift-g"` after "show ignored files".

README: in **Sidebar and viewer**, a new paragraph after the tree paragraph:

```markdown
Under the Explorer, Worktrees lists `git worktree list`: each worktree's folder, branch and, once
expanded, its changed files in the status colours with a count in the row (a collapsed worktree
never runs `git status`). Click a changed file to read it in the viewer. Right click a worktree:
Open terminal here, Remove worktree (refused while it has changes), Remove worktree, force. The
"+" on the header creates one: a branch name, then the path (prefilled beside the project);
an existing branch is checked out, a new one created. The main checkout is first and cannot be
removed. Ctrl+Shift+G puts the keyboard in the list. The tree's rows carry git status in the
theme's pastels (added, modified, untracked) and a folder with changes inside carries a dot.
```

The keyboard table gains `| Ctrl+Shift+G | focus the worktree list |`; the Notes line about Ctrl+Shift+G arriving in 4b goes. The checklist (top of Status) gains: "- in a repo with a change: the file's row is amber and its folders carry a dot; `git stash` clears them within ten seconds", "- Worktrees: expand one: a shimmer, then its changed files fade in; collapse and expand again: `git status` runs again (watch `ps`); + creates `../proj-feat` on a new branch; Remove on a dirty one is refused with git's words, force removes it", "- Ctrl+Shift+G, Down, Enter: a worktree expands from the keyboard".

- [ ] **Step 4: Run the tests and the build**

Run: `~/.cargo/bin/cargo test -p sarkos` then the warning check and `grep -n "—" README.md crates/sarkos/src/*.rs`.
Expected: PASS; clean. By hand if a screen is available: the checklist rows above.

- [ ] **Step 5: Commit**

```bash
git add crates/sarkos/src/sidebar.rs crates/sarkos/src/keymap.rs crates/sarkos/src/config.rs crates/sarkos/src/command_palette.rs crates/sarkos/src/main.rs crates/sarkos/src/workspace.rs crates/sarkos/src/worktrees.rs README.md
git commit -m "Draw the Worktrees section: lazy status with a shimmer, changed files easing in, actions and a create prompt"
```

---

### Task 5: Diff gutters and the inline diff in the viewer

**Files:**
- Create: `crates/sarkos/src/diff.rs`
- Modify: `crates/sarkos/Cargo.toml` (`similar`), `Cargo.lock`, `crates/sarkos/src/viewer.rs` (`Viewer.{diff, inline_diff}`, the load computing the diff, `display_rows`), `crates/sarkos/src/viewer_element.rs` (gutter marks, inline rows), `crates/sarkos/src/pane_view.rs` (the toggle chip), `crates/sarkos/src/keymap.rs` ("toggle inline diff" cmd-shift-d, Viewer context), `crates/sarkos/src/config.rs` (comment line), `crates/sarkos/src/main.rs` (`Command::ToggleInlineDiff`), `README.md`
- Test: unit tests in `diff.rs` (pure marks, a temp repository for the base), `viewer.rs`, `viewer_element.rs`

**Interfaces:**
- Consumes: `explorer::git`, `viewer::{Viewer, Content, Loaded}`, `read_file`, the element's `Frame`, `theme.tree.*`, `motion` (no animation here).
- Produces: `diff::{Mark::{Added, Modified}, DiffMarks { marks: Vec<Option<Mark>> (one per current line), deleted_before: Vec<(usize, Vec<String>)> (line index, the deleted lines that preceded it; index == lines.len() for a deletion at the end) }, compute(base: &str, lines: &[String]) -> DiffMarks, base_for(path: &Path) -> Option<String> (None when not in a repository, untracked, or HEAD has no such file), DisplayRow::{Line(usize), Deleted(usize /*deleted_before index*/, usize /*line within it*/)}, display_rows(line_count, &DiffMarks, inline: bool) -> Vec<DisplayRow>}`, `Viewer.{diff: Option<DiffMarks>, inline_diff: bool}`, `Viewer::toggle_inline_diff(cx)`, `Command::ToggleInlineDiff`.

- [ ] **Step 1: Write the failing tests**

Create `crates/sarkos/src/diff.rs`:

```rust
//! Diff gutters for the viewer (spec 6): the file against its worktree's HEAD, as a line diff
//! through `similar`, kept as one mark per current line plus the deleted lines and where they
//! were. The base comes from `git show HEAD:<path>` run from the file's own worktree.

use crate::explorer::git;
use similar::{ChangeTag, TextDiff};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Added,
    Modified,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiffMarks {
    /// One per current line.
    pub marks: Vec<Option<Mark>>,
    /// (index of the current line the deletion sits before, the deleted lines). An index equal
    /// to the line count means lines were deleted at the end.
    pub deleted_before: Vec<(usize, Vec<String>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayRow {
    Line(usize),
    /// (which deletion, which line of it)
    Deleted(usize, usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(str::to_string).collect()
    }

    #[test]
    fn added_modified_and_deleted_lines_are_marked() {
        let base = "a\nb\nc\nd\n";
        let now = lines("a\nB\nc\nd\ne\n");
        let d = compute(base, &now);
        assert_eq!(d.marks, vec![None, Some(Mark::Modified), None, None, Some(Mark::Added)]);
        assert!(d.deleted_before.is_empty());
        let now = lines("a\nd\n");
        let d = compute(base, &now);
        assert_eq!(d.marks, vec![None, None]);
        assert_eq!(d.deleted_before, vec![(1, vec!["b".to_string(), "c".to_string()])], "b and c were deleted before d");
    }

    #[test]
    fn marks_for_a_deletion_at_the_top_and_a_clean_file() {
        let base = "x\na\nb\n";
        let now = lines("a\nb\n");
        let d = compute(base, &now);
        assert_eq!(d.deleted_before, vec![(0, vec!["x".to_string()])], "a deletion before line 1");
        assert_eq!(d.marks, vec![None, None]);
        let clean = compute("a\nb\n", &lines("a\nb\n"));
        assert_eq!(clean, DiffMarks { marks: vec![None, None], deleted_before: vec![] });
        assert!(!clean.has_changes());
        let end = compute("a\nb\nc\n", &lines("a\nb\n"));
        assert_eq!(end.deleted_before, vec![(2, vec!["c".to_string()])], "deleted at the end: before index == line count");
    }

    #[test]
    fn display_rows_interleave_deleted_lines_only_when_inline() {
        let d = DiffMarks { marks: vec![None, None], deleted_before: vec![(0, vec!["x".into()]), (2, vec!["y".into(), "z".into()])] };
        assert_eq!(display_rows(2, &d, false), vec![DisplayRow::Line(0), DisplayRow::Line(1)]);
        assert_eq!(display_rows(2, &d, true), vec![DisplayRow::Deleted(0, 0), DisplayRow::Line(0), DisplayRow::Line(1), DisplayRow::Deleted(1, 0), DisplayRow::Deleted(1, 1)]);
        assert_eq!(display_rows(2, &DiffMarks::default(), true), vec![DisplayRow::Line(0), DisplayRow::Line(1)], "no diff: the plain rows");
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let run = |args: &[&str]| {
            let st = git(dir.path()).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap();
            assert!(st.success(), "git {args:?}");
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "t@example.com"]);
        run(&["config", "user.name", "t"]);
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/a.rs"), "one\ntwo\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "init"]);
        dir
    }

    #[test]
    fn the_base_is_head_for_a_tracked_file_and_none_otherwise() {
        let _lock = crate::client::fork_lock();
        let dir = repo();
        std::fs::write(dir.path().join("src/a.rs"), "one\nTWO\n").unwrap();
        assert_eq!(base_for(&dir.path().join("src/a.rs")).as_deref(), Some("one\ntwo\n"));
        std::fs::write(dir.path().join("src/new.rs"), "").unwrap();
        assert_eq!(base_for(&dir.path().join("src/new.rs")), None, "untracked");
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("f.txt"), "").unwrap();
        assert_eq!(base_for(&outside.path().join("f.txt")), None, "not a repository");
    }

    #[test]
    fn the_base_comes_from_the_files_own_worktree() {
        let _lock = crate::client::fork_lock();
        let dir = repo();
        let root = dir.path().canonicalize().unwrap();
        let wt = root.parent().unwrap().join(format!("{}-wt", root.file_name().unwrap().to_string_lossy()));
        crate::worktrees::create(&root, "wt", &wt).unwrap();
        // Commit a different a.rs in the worktree, so its HEAD differs from main's.
        std::fs::write(wt.join("src/a.rs"), "wt\n").unwrap();
        let run = |args: &[&str]| assert!(git(&wt).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap().success());
        run(&["config", "user.email", "t@example.com"]);
        run(&["config", "user.name", "t"]);
        run(&["commit", "-q", "-am", "wt"]);
        std::fs::write(wt.join("src/a.rs"), "wt\nmore\n").unwrap();
        assert_eq!(base_for(&wt.join("src/a.rs")).as_deref(), Some("wt\n"), "the worktree's HEAD, not main's");
        assert_eq!(base_for(&root.join("src/a.rs")).as_deref(), Some("one\ntwo\n"));
        crate::worktrees::remove(&root, &wt, true).unwrap();
    }
}
```

In `viewer_element.rs`'s tests:

```rust
    #[test]
    fn the_gutter_grows_by_the_mark_column_and_a_display_row_maps_back_to_its_line() {
        assert_eq!(gutter_width_with_marks(99, 10.0), gutter_width(99, 10.0) + MARK_W);
        let d = crate::diff::DiffMarks { marks: vec![None, Some(crate::diff::Mark::Added)], deleted_before: vec![(1, vec!["gone".into()])] };
        let rows = crate::diff::display_rows(2, &d, true);
        assert_eq!(rows, vec![crate::diff::DisplayRow::Line(0), crate::diff::DisplayRow::Deleted(0, 0), crate::diff::DisplayRow::Line(1)]);
        assert_eq!(visible_range(0.0, 10, rows.len()), 0..3);
    }
```

`keymap.rs`'s `SPEC_ROWS` gains `("cmd-shift-d", "toggle inline diff", "ToggleInlineDiff")` with context `Some("Viewer")` in `ACTIONS` (the row's context is checked by `the_table_is_the_spec_table` only by name; add the assertion that this row's context is `Some("Viewer")`, like the one for "clear to start").

- [ ] **Step 2: Run them to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkos -- diff viewer_element keymap`
Expected: FAIL to compile (add `mod diff;` and the `similar = "2"` dependency; cargo fetches it).

- [ ] **Step 3: Implement**

`diff.rs`, above the tests:

```rust
impl DiffMarks {
    pub fn has_changes(&self) -> bool {
        self.marks.iter().any(Option::is_some) || !self.deleted_before.is_empty()
    }
}

/// Line diff of `base` against the current `lines`. A replaced run marks its new lines modified
/// (the old ones are not kept: the gutter says "changed here"); a pure insertion marks added; a
/// pure deletion is recorded before the next current line.
pub fn compute(base: &str, lines: &[String]) -> DiffMarks {
    let now = lines.join("\n") + if lines.is_empty() { "" } else { "\n" };
    let diff = TextDiff::from_lines(base, &now);
    let mut marks = vec![None; lines.len()];
    let mut deleted_before = Vec::new();
    for op in diff.ops() {
        let (old_range, new_range) = (op.old_range(), op.new_range());
        match op.tag() {
            similar::DiffTag::Equal => {}
            similar::DiffTag::Insert => {
                for i in new_range {
                    marks[i] = Some(Mark::Added);
                }
            }
            similar::DiffTag::Delete => {
                let gone: Vec<String> = base.lines().skip(old_range.start).take(old_range.len()).map(str::to_string).collect();
                deleted_before.push((new_range.start, gone));
            }
            similar::DiffTag::Replace => {
                for i in new_range.clone() {
                    marks[i] = Some(Mark::Modified);
                }
                // More old lines than new: the surplus is a deletion before the line after.
                if old_range.len() > new_range.len() {
                    let gone: Vec<String> = base.lines().skip(old_range.start + new_range.len()).take(old_range.len() - new_range.len()).map(str::to_string).collect();
                    deleted_before.push((new_range.end, gone));
                }
            }
        }
    }
    let _ = ChangeTag::Equal;
    DiffMarks { marks, deleted_before }
}

/// The worktree root of `path`'s directory, through git, so a file in another worktree diffs
/// against that worktree's HEAD.
fn worktree_root(dir: &Path) -> Option<PathBuf> {
    let out = git(dir).args(["rev-parse", "--show-toplevel"]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
}

/// `HEAD:<relative path>` of the file's worktree; `None` outside a repository or for a file HEAD
/// does not have (untracked, or new). Never reads the working file. Background executor only.
pub fn base_for(path: &Path) -> Option<String> {
    let dir = path.parent()?;
    let root = worktree_root(dir)?;
    let rel = path.canonicalize().ok()?.strip_prefix(root.canonicalize().ok()?).ok()?.to_path_buf();
    let spec = format!("HEAD:{}", rel.to_string_lossy());
    let out = git(&root).args(["show", "--no-ext-diff", &spec]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The rows the element paints: every current line, with the deleted lines interleaved where
/// they were when `inline` is on.
pub fn display_rows(line_count: usize, d: &DiffMarks, inline: bool) -> Vec<DisplayRow> {
    let mut out = Vec::with_capacity(line_count + if inline { d.deleted_before.iter().map(|(_, l)| l.len()).sum() } else { 0 });
    let mut del = d.deleted_before.iter().enumerate().peekable();
    for i in 0..=line_count {
        if inline {
            while let Some((k, (before, lines))) = del.peek() {
                if *before == i {
                    for j in 0..lines.len() {
                        out.push(DisplayRow::Deleted(*k, j));
                    }
                    del.next();
                } else {
                    break;
                }
            }
        }
        if i < line_count {
            out.push(DisplayRow::Line(i));
        }
    }
    out
}
```

(The `similar` API: `TextDiff::from_lines(old, new)`, `diff.ops()` yielding `DiffOp` with `tag()`, `old_range()`, `new_range()`; `DiffTag::{Equal, Delete, Insert, Replace}`. Remove the `ChangeTag` import line if unused. If a name differs, grep the crate.)

`viewer.rs`: `Viewer` gains `pub diff: Option<DiffMarks>` and `pub inline_diff: bool`; `Loaded` gains `pub diff: Option<DiffMarks>`; `read_file` gains a `with_diff: bool` parameter: when the content is `Text` and not plain-for-size, it calls `diff::base_for(path)` and `diff::compute` (both on the background executor, where `read_file` already runs) and stores the result (`None` when there is no base or no changes). `apply` takes `diff` from `Loaded`, and clamps the scroll against `display_rows(..).len()` instead of the line count when `inline_diff` is on (`max_scroll` becomes the display row count minus one). `toggle_inline_diff(cx)` flips the flag when `diff` has changes, re-clamps and notifies. The ten-second timer (`explorer_watch`'s slow tick) calls `reload_viewers_for` on every open viewer with `if_changed = false` only for the diff: add `Root::rediff_viewers(cx)` that reloads viewers whose `diff.is_some()` or whose file is inside a repository (cheapest: reload all text viewers; the read is small and background).

`viewer_element.rs`: `pub const MARK_W: f32 = 6.0;` and `gutter_width_with_marks(line_count, cell_w) = gutter_width + MARK_W`; the element uses it when the viewer has a diff. The `Frame` carries `rows: Vec<DisplayRow>` (the visible slice of `display_rows`), `marks` for those lines, the deleted texts for the deleted rows, and the colours (`added`, `modified`, `deleted` from `theme.tree`, passed into `ViewerElement::new`). Painting, per visible display row:
- `Line(i)`: as today (number, text), plus in the mark column (the gutter's left `MARK_W` strip): a 3 px wide bar the row's height in `added` or `modified` when marked; and when a deletion is recorded before line `i` and the inline diff is off, a small red triangle at the line's top edge (three `paint_quad`s stacked 1, 2, 3 px wide, or a `fill` of a 6 by 3 px quad: a triangle is cosmetic; a 6 by 2 px bar in `deleted` at the top edge is acceptable and simpler; say which in the commit).
- `Deleted(k, j)`: no line number; the text in `deleted` on a faint `deleted` tint (alpha 0.12) across the row; find hits never land on it.
`visible_range` runs over the display rows; `rows_visible` and the scroll are in display rows while inline is on. The `..` keyboard (Home/End/Page) stays in rows.

`pane_view.rs`: when the viewer's `diff` has changes, a small chip at the top right (next to where the find bar sits; below it when the bar is open) reading "inline diff" that toggles on click (`cursor_pointer`, `chrome.banner` background, `text_dim` text, `text` when on). `run`'s `ToggleInlineDiff` arm calls `active_viewer(..).toggle_inline_diff(cx)`.

`keymap.rs`: `ToggleInlineDiff` with `ActionSpec { name: "toggle inline diff", keys: "cmd-shift-d", context: Some("Viewer") }` after "focus worktrees"; registered in all five places; `config.rs`'s comment gains `# "toggle inline diff" = "cmd-shift-d"`.

README: the viewer paragraph gains: "A file in a git repository shows its changes against the worktree's HEAD in the gutter: an added line a green bar, a changed line an amber one, a deletion a red mark at the top of the line that follows; Cmd+Shift+D (or the "inline diff" chip) shows the deleted lines in red between the current ones." The keyboard table gains `| Cmd+Shift+D | inline diff in the viewer |`. The checklist gains: "- edit a tracked file in another app while the viewer shows it: the gutter bars appear within a second; Cmd+Shift+D shows the deleted lines; `git commit` clears the bars within ten seconds; a file in a worktree diffs against that worktree's HEAD". The Status paragraph says milestone 5 (the browser pane) is next, and "Git status colours, the worktree explorer and the diff gutters come in milestone 4b" goes.

- [ ] **Step 4: Run everything**

Run: `~/.cargo/bin/cargo test --workspace`, the warning check, `grep -n "—" README.md crates/sarkos/src/*.rs`.
Expected: all green; no project warnings; no em-dashes.

- [ ] **Step 5: Commit**

```bash
git add crates/sarkos/Cargo.toml Cargo.lock crates/sarkos/src/diff.rs crates/sarkos/src/viewer.rs crates/sarkos/src/viewer_element.rs crates/sarkos/src/pane_view.rs crates/sarkos/src/keymap.rs crates/sarkos/src/config.rs crates/sarkos/src/main.rs crates/sarkos/src/explorer_watch.rs README.md
git commit -m "Mark added, changed and deleted lines in the viewer's gutter, with an inline diff on Cmd+Shift+D"
```

---

## Self-review notes

- Spec 6, file tree: git status in the pastels and folder dots (Task 2). Worktree explorer: list only until expanded (Task 3's `to_load`), status on expand with a shimmer and an ease-in (Task 4), collapsing drops the result (Task 3's `toggle`), refresh on the slow timer (Task 3's watch hook; the "on the watcher" part is covered by the explorer poll invalidating the project status, and worktrees refresh on the timer: ruling 2), dirty count once loaded, click opens the viewer in that worktree, the three actions with the dirty refusal and force, the main checkout first and unremovable (Tasks 3, 4). Viewer: line diff against HEAD, gutter marks, the Cmd+Shift+D inline diff (Task 5). Spec 4: Ctrl+Shift+G (Task 4).
- Type consistency: `Status` is `gitstatus::Status` everywhere; `Changes` is `Vec<(PathBuf, Status)>` in Tasks 3 and 4; `WtRow`'s variants match between Task 3's tests and Task 4's `wt_key`; `DiffMarks`/`DisplayRow` match between `diff.rs`, `viewer.rs` and the element; `create_in(tab, note, cwd)` as defined in Task 3 and used in Task 4; `refresh_status`/`refresh_worktrees`/`load_pending_worktrees` live on `Root` in `sidebar.rs`'s impl block (Task 1's move) and `main.rs`.
- Review Focus pins: 1 in Tasks 2 and 3, 2 in Task 2 (the stale-result test is `a_stale_status_result_is_dropped`: add it to Task 2's `explorer_watch` tests as a pure check of the `seq == status_seq` rule, `fn status_result_applies(seq, current) -> bool`), 3 and 5 in Task 5, 4 in Task 3.
- Rulings 1 to 7 are each applied in the task that owns the code; the executor records further ones in the ledger.
