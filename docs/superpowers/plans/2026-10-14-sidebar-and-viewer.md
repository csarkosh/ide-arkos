# Sarkos IDE: Sidebar, file tree and the read-only viewer (milestone 4a). Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the window a left sidebar with a lazy file tree that respects `.gitignore`, and read-only viewer tabs that open from it with syntax highlighting, line numbers, find, and live reload, so a file can be read next to the agents working on it without leaving the window.

**Architecture:** The sidebar is a fixed left column laid out before the pane tree (the tree's content rect starts after it), with one collapsible section, Explorer, in this plan; milestone 4b adds Worktrees under it. The file tree is a pure model (`explorer.rs`: expanded set, loaded directories, visible rows) fed by background reads that run `ls`-style directory listing plus `git check-ignore` for the ignore marks, and a one-second poll of expanded directories' modification times in place of a file-system watcher. A viewer tab holds a `Viewer` entity: the file loaded once on the background executor, classified (text, too big, binary), split into lines and highlighted with `syntect` through a theme built from the config theme's viewer colours; a custom element paints only the visible lines with a line-number gutter, scrolls through the existing `ScrollAnimator`, and a small find bar searches the lines. Single click previews, double click pins (VS Code's preview tab), right click gives the four file actions, and a file dragged onto a terminal pastes its path.

**Tech Stack:** Rust stable, gpui 0.2.2 (exact pin), `syntect` 5 with the pure-Rust regex engine (`regex-fancy`), no `onig`; git through the `git` binary (no `git2`); the existing `ScrollAnimator`, `LineEdit`, `config_watch::fingerprint`. One new dependency: `syntect`.

**Spec:** `docs/superpowers/specs/2026-10-13-sarkos-design.md` section 6 (sidebar: the file tree and the viewer paragraphs; the worktree explorer paragraph and the viewer's diff gutters are milestone 4b), section 4 (viewer tabs, the sidebar keys, persistence of sidebar state), section 7 (tree and viewer colours, text presets apply to viewers) and section 9 (never block the UI thread; a viewer holds one file).

## Global Constraints

- macOS only. The window never blocks the UI thread on IO: directory reads, `git check-ignore`, file reads, highlighting and the polls run on the background executor; the UI thread applies results.
- `gpui = { version = "=0.2.2", features = ["runtime_shaders"] }` pinned exactly; never bump. If a gpui or syntect name in this plan does not compile, grep `~/.cargo/registry/src/*/gpui-0.2.2/src/` or `~/.cargo/registry/src/*/syntect-5*/src/` for it, fix the call, and note the change in the commit message. Do not guess.
- One new dependency, `syntect = { version = "5", default-features = false, features = ["default-syntaxes", "regex-fancy", "dump-load", "parsing"] }` in `crates/sarkos/Cargo.toml`, added in Task 4 and nowhere else. No other new crates.
- The tree hides `.git`, respects `.gitignore` by default with a toggle to show ignored entries, and reads a directory's children only when it is expanded. Files over the configured limit (default 5 MB) open without highlighting; binaries show a short notice. The viewer is read-only: nothing in this plan writes to a project file.
- The theme file format does not change: the tree uses `theme.tree.*`, the viewer `theme.viewer.*` and `theme.chrome.{window_bg, text, text_dim, sidebar, border}`. The workspace file gains an optional `sidebar` object; files without it load as before.
- Display name "Sarkos IDE"; lowercase `sarkos` elsewhere. No em-dashes in any copy or comment. Commit messages imperative, one line, body optional, ending with a blank line, `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_015xrVD54zbvkgksJUndxfpr`.
- Tests: `cargo test -p sarkos` stays headless (no window); a test that needs a git repository creates one in a temp dir with `git init`; the build stays warning-free for the project's crates; `cargo` is at `~/.cargo/bin/cargo`.

## Review Focus

1. **A folder with ten thousand entries.** Expanding `node_modules` must not freeze the window: the read is off the UI thread and the rows are plain divs in a scrolling column. Pinned by Task 2's `rows_are_sorted_dirs_first_and_case_insensitively` on the model (the read is background by construction) and by the README checklist.
2. **A file that changes while open.** Claude Code edits a file the viewer shows; within about a second the viewer shows the new content without losing the scroll position more than the file's new length forces. Pinned by Task 5's `a_changed_fingerprint_reloads_and_keeps_the_scroll_clamped`.
3. **A binary or huge file clicked by accident.** A 200 MB log or an image opens as a one-line notice, never a frozen window. Pinned by Task 4's `classify_text_too_big_and_binary`.
4. **A project that is not a git repository.** The tree still works, nothing is marked ignored, no error banner. Pinned by Task 2's `mark_ignored_does_nothing_outside_a_repository`.
5. **A single click on the file already previewed, then on another.** The preview tab is replaced, never duplicated; a pinned tab is never replaced. Pinned by Task 3's `a_preview_is_replaced_and_a_pin_is_kept`.

---

## File structure

```
crates/sarkos/
  Cargo.toml                   syntect (Task 4)
  src/sidebar.rs               NEW: the column: state (width, collapsed, show_ignored), the resize handle, section header, rendering of the Explorer section rows
  src/explorer.rs              NEW: the file tree model: entries, expanded set, loaded dirs, visible rows, selection moves, list_dir, mark_ignored (git check-ignore)
  src/explorer_watch.rs        NEW: the one-second poll of expanded directories and open viewer files
  src/highlight.rs             NEW: syntect wrapper: a Theme from ViewerColors, highlight(path, lines) -> spans
  src/viewer.rs                NEW: the Viewer entity: load/classify/split, find, scroll, reload
  src/viewer_element.rs        NEW: paints the visible lines with the gutter, scroll, find highlights
  src/workspace.rs             SidebarSaved in the workspace file
  src/config.rs                [viewer] highlight_limit_bytes; the [sidebar] comment
  src/keymap.rs                "toggle sidebar" cmd-b, "focus file tree" cmd-shift-e, "show ignored files" (no key)
  src/command_palette.rs       the three items
  src/pane_view.rs             viewer tabs render the viewer; file drop on a terminal; preview tabs dimmed
  src/drag.rs                  FileDrag
  src/main.rs                  Root: sidebar, explorer, viewers, preview_tab, context_menu, highlighter; layout; open_viewer; commands
  README.md                    Sidebar and viewer section, keyboard rows, checklist, config
```

Rulings the plan makes where the spec is silent (the executor records any further one in the ledger):

1. The "file system watcher" is a one-second poll of expanded directories' and open files' modification times, like the config watcher: no dependency, and the tree only shows expanded directories anyway. A real FSEvents watcher can replace it in milestone 6 if the poll shows up in the idle CPU measurement.
2. `.gitignore` is answered by `git check-ignore` once per expanded directory, on the background executor, instead of a gitignore-parsing crate: exact semantics, one dependency fewer. Outside a repository nothing is ignored.
3. Worktrees, git status colours, the folder dots, diff gutters and the inline diff are milestone 4b; this plan leaves the Worktrees section out rather than showing an empty header, and the viewer's gutter shows line numbers only.
4. A preview tab is shown with the dim text colour in the tab bar (gpui's tab labels have no italic path today); double click or an edit action pins it.
5. The viewer follows the text preset like a terminal (spec 7) and Cmd+= / Cmd+- change only the focused viewer, like a terminal.
6. Dragging a file onto a terminal pastes the path single-quoted when it needs quoting, followed by a space, through the terminal's bracketed paste.
7. Keyboard in the tree: Up/Down move the selection, Right expands or moves into a directory, Left collapses or moves to the parent, Enter opens pinned, Space previews, Escape returns focus to the focused pane.

---

### Task 1: The sidebar column, Cmd+B, the resize handle, persistence

**Files:**
- Create: `crates/sarkos/src/sidebar.rs`
- Modify: `crates/sarkos/src/workspace.rs`, `crates/sarkos/src/config.rs` (the `[sidebar]` comment only), `crates/sarkos/src/keymap.rs`, `crates/sarkos/src/command_palette.rs`, `crates/sarkos/src/main.rs`, `README.md` (keyboard rows)
- Test: unit tests in `sidebar.rs`, `workspace.rs`, `keymap.rs`, `command_palette.rs`

**Interfaces:**
- Consumes: `config.sidebar.{width, collapsed}`, `Root.content: Rect`, `split_view::render_tree` (reads `root.content.x` as its origin), `Root.resizing` handling in `render`'s `on_mouse_move` / `on_mouse_up`, `Workspace`, `keymap::ACTIONS`, `command_palette::items`.
- Produces: `sidebar::Sidebar { width: f32, collapsed: bool, show_ignored: bool, focus: FocusHandle }`, `sidebar::{MIN_W, MAX_W, HANDLE_W, clamp_width(f32) -> f32, visible_width(&Sidebar) -> f32}`, `sidebar::render(root, cx) -> Option<Div>` (the column, `None` when collapsed), `workspace::SidebarSaved { width, collapsed, show_ignored, expanded: Vec<PathBuf> }` and `Workspace.sidebar: Option<SidebarSaved>`, `Root.sidebar`, `Root.sidebar_resizing: bool`, `Command::{ToggleSidebar, FocusFileTree, ShowIgnoredFiles}`.

- [ ] **Step 1: Write the failing tests**

Create `crates/sarkos/src/sidebar.rs` with the module doc and the tests:

```rust
//! The left sidebar (spec 6): a fixed column, Cmd+B to toggle, resizable by its right edge,
//! holding the Explorer section (and Worktrees in milestone 4b). Its state is saved with the
//! workspace; the config's `[sidebar]` gives a new folder its defaults.

use crate::explorer::Row;
use crate::theme::Theme;
use crate::Root;
use gpui::{div, prelude::*, px, ClickEvent, Context, Div, FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent, ScrollHandle, SharedString};
use std::path::Path;

pub const MIN_W: f32 = 160.0;
pub const MAX_W: f32 = 600.0;
/// The resize handle at the column's right edge, with its invisible hit area.
pub const HANDLE_W: f32 = 4.0;
pub const HANDLE_HIT_W: f32 = 10.0;
pub const ROW_H: f32 = 24.0;
pub const INDENT: f32 = 14.0;

pub struct Sidebar {
    pub width: f32,
    pub collapsed: bool,
    pub show_ignored: bool,
    pub focus: FocusHandle,
    pub scroll: ScrollHandle,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_width_stays_between_the_limits() {
        assert_eq!(clamp_width(10.0), MIN_W);
        assert_eq!(clamp_width(260.0), 260.0);
        assert_eq!(clamp_width(9000.0), MAX_W);
        assert_eq!(clamp_width(f32::NAN), MIN_W, "a NaN from a bad file is the minimum, not a panic");
    }

    #[test]
    fn a_collapsed_sidebar_takes_no_width() {
        assert_eq!(visible_width(300.0, false), 300.0);
        assert_eq!(visible_width(300.0, true), 0.0);
    }
}
```

In `crates/sarkos/src/workspace.rs`'s tests:

```rust
    #[test]
    fn the_sidebar_state_round_trips_and_is_optional() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ws.json");
        let project = Path::new("/p");
        let mut ws = Workspace { version: FORMAT_VERSION, project: project.to_path_buf(), tree: tree_with_two(), text_preset: None, sidebar: None };
        save_to(&file, &ws).unwrap();
        assert_eq!(load_from(&file, project), Some(ws.clone()));
        ws.sidebar = Some(SidebarSaved { width: 300.0, collapsed: true, show_ignored: true, expanded: vec![PathBuf::from("/p/src")] });
        save_to(&file, &ws).unwrap();
        assert_eq!(load_from(&file, project), Some(ws));
        // A milestone 3 file has no sidebar object.
        std::fs::write(&file, r#"{ "version": 2, "project": "/p", "tree": { "root": { "Leaf": { "id": 1, "tabs": [], "active": 0 } }, "next_id": 2, "focused": 1, "maximized": null } }"#).unwrap();
        assert_eq!(load_from(&file, project).unwrap().sidebar, None);
    }
```

(Add `sidebar: None` to every other `Workspace` literal in `workspace.rs` and `main.rs`.) In `keymap.rs`'s `SPEC_ROWS` add `("cmd-b", "toggle sidebar", "ToggleSidebar")`, `("cmd-shift-e", "focus file tree", "FocusFileTree")` and `("", "show ignored files", "ShowIgnoredFiles")` after the "install claude code hooks" row, and change the two unbound-count assertions from `- 4` / `- 3` to `- 5` / `- 4`. In `command_palette.rs`'s `items_cover_actions_presets_themes_and_config` assert the three labels exist: `Toggle sidebar` with detail `Cmd+B`, `Focus file tree` with `Shift+Cmd+E` (whatever `pretty_keys` prints for `cmd-shift-e`; read the existing expectation for `cmd-shift-p` and match its style), and `Show ignored files` with detail `"off"` (the new `show_ignored: bool` parameter false) and `"on"` when true.

- [ ] **Step 2: Run them to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkos -- sidebar workspace keymap command_palette`
Expected: FAIL to compile (add `mod sidebar;` and `mod explorer;` to `main.rs`; `explorer::Row` does not exist yet: for this task create `crates/sarkos/src/explorer.rs` containing only the `Row` struct from Task 2's interface, `pub struct Row { pub path: PathBuf, pub name: String, pub depth: usize, pub is_dir: bool, pub expanded: bool, pub ignored: bool, pub loading: bool }`, which Task 2 fills in around).

- [ ] **Step 3: Implement the state, the file format, the actions**

`sidebar.rs`, above the tests:

```rust
pub fn clamp_width(w: f32) -> f32 {
    if w.is_nan() {
        return MIN_W;
    }
    w.clamp(MIN_W, MAX_W)
}

pub fn visible_width(width: f32, collapsed: bool) -> f32 {
    if collapsed {
        0.0
    } else {
        width
    }
}

impl Sidebar {
    pub fn new(width: f32, collapsed: bool, show_ignored: bool, cx: &mut Context<Root>) -> Self {
        Sidebar { width: clamp_width(width), collapsed, show_ignored, focus: cx.focus_handle(), scroll: ScrollHandle::new() }
    }

    pub fn visible_width(&self) -> f32 {
        visible_width(self.width, self.collapsed)
    }
}

/// The column: a header per section, the Explorer rows, and the resize handle. `None` while
/// collapsed. The rows come from Task 3's `render_rows`; until then the section is empty.
pub fn render(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    if root.sidebar.collapsed {
        return None;
    }
    let theme = root.theme.clone();
    let c = &theme.chrome;
    let width = root.sidebar.width;
    let family = root.family();
    let header = |title: &str| -> Div {
        div()
            .h(px(ROW_H))
            .px(px(10.0))
            .flex()
            .items_center()
            .text_color(c.text_dim.hsla())
            .text_size(px(11.0))
            .child(title.to_uppercase())
    };
    let handle = div()
        .id("sidebar-handle")
        .absolute()
        .top_0()
        .right(px(-(HANDLE_HIT_W - HANDLE_W) / 2.0))
        .w(px(HANDLE_HIT_W))
        .h_full()
        .cursor_col_resize()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|root, _ev: &MouseDownEvent, _window, cx| {
                root.sidebar_resizing = true;
                cx.notify();
            }),
        );
    Some(
        div()
            .id("sidebar")
            .relative()
            .w(px(width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(c.sidebar.hsla())
            .border_r_1()
            .border_color(c.border.hsla())
            .font_family(family)
            .text_size(px(13.0))
            .text_color(c.text.hsla())
            .key_context("Explorer")
            .track_focus(&root.sidebar.focus)
            .child(header("Explorer"))
            .child(crate::sidebar::render_rows(root, &theme, cx))
            .child(handle),
    )
}

/// The Explorer's rows. Task 3 replaces this body; Task 1 shows nothing.
pub fn render_rows(_root: &mut Root, _theme: &Theme, _cx: &mut Context<Root>) -> Div {
    div().flex_1().min_h_0()
}
```

(gpui: `cursor_col_resize` exists on `Styled`; if the name differs, grep `fn cursor_` in `styled.rs`.)

`workspace.rs`:

```rust
/// The sidebar as the user left it for this folder (spec 4: "sidebar width and collapsed
/// states" are saved). Absent in files from before milestone 4.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SidebarSaved {
    pub width: f32,
    pub collapsed: bool,
    pub show_ignored: bool,
    /// Expanded directories, absolute, so the tree reopens as it was.
    pub expanded: Vec<PathBuf>,
}
```

and `Workspace` gains `#[serde(default)] pub sidebar: Option<SidebarSaved>`.

`keymap.rs`: `actions!` gains `ToggleSidebar, FocusFileTree, ShowIgnoredFiles`; `Command` gains `ToggleSidebar` ("Collapse or show the sidebar"), `FocusFileTree` ("Put keyboard focus in the file tree"), `ShowIgnoredFiles` ("Flip whether ignored entries are listed, dimmed"); `ACTIONS` gains, after `row("install claude code hooks", "")`: `row("toggle sidebar", "cmd-b"), row("focus file tree", "cmd-shift-e"), row("show ignored files", "")`; `bindings_for`, `command_for` and `attach` get the three names. `config.rs`'s `[keybindings]` comment block gains the three lines in that order; the `[sidebar]` comment becomes `# Width in px and whether it starts collapsed, for a folder opened for the first time; each folder then remembers its own.` (drop "Milestone 4.").

`command_palette.rs`: `items` gains `show_ignored: bool` as its last parameter; `Command::ToggleSidebar` and `FocusFileTree` get the key detail like other actions; `Command::ShowIgnoredFiles` gets detail `"on"` or `"off"` from `show_ignored`.

`main.rs`:
- `Root` gains `pub sidebar: sidebar::Sidebar`, `pub sidebar_resizing: bool`. `Root::new` takes `cx: &mut Context<Root>` already? It is called inside `cx.new(|_| Root::new(..))`: change the closure to `cx.new(|cx| ...)` and pass `cx` so `Sidebar::new` can make a focus handle. The sidebar's initial state: the saved `ws.sidebar` when present, else `config.sidebar.width` / `config.sidebar.collapsed` and `show_ignored: false` (keep `saved` around long enough: read `saved.as_ref().and_then(|w| w.sidebar.clone())` before `saved` is consumed for the tree).
- `save()` fills `sidebar: Some(SidebarSaved { width: self.sidebar.width, collapsed: self.sidebar.collapsed, show_ignored: self.sidebar.show_ignored, expanded: Vec::new() })` (Task 2 fills `expanded`).
- `render`: `let side = self.sidebar.visible_width();` and the content rect becomes `x: side, w: (f32::from(size.width) - side).max(0.0)`; the body line becomes

```rust
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .children(sidebar::render(self, cx))
                    .child(div().flex_1().min_w_0().min_h_0().child(body)),
            )
```

(`sidebar::render` must run before `split_view::render_tree`? No: the tree reads `self.content`, which is computed before both; build the sidebar after `body`, since `render_tree` takes `&mut self` first. Order the lets so `body` is built, then `let side_col = sidebar::render(self, cx);`, then the div.)
- The existing `on_mouse_move` listener that handles `root.resizing` gains, before it: `if root.sidebar_resizing { root.sidebar.width = sidebar::clamp_width(f32::from(ev.position.x)); cx.notify(); return; }`; the `on_mouse_up` listener that clears `root.resizing` also clears `sidebar_resizing` (`if root.sidebar_resizing { root.sidebar_resizing = false; root.save(cx); }`).
- `run`: `ToggleSidebar => { self.sidebar.collapsed = !self.sidebar.collapsed; self.grids.clear(); self.save(cx); cx.notify(); }` (the panes change width: measurements taken before are stale, as on a window resize), `FocusFileTree => { if self.sidebar.collapsed { self.sidebar.collapsed = false; self.grids.clear(); } self.sidebar_focused = true; cx.notify(); }` where `pub sidebar_focused: bool` is a new `Root` field that `render`'s focus logic honours: when true, `wanted` is `Some(self.sidebar.focus.clone())` ahead of the pane's handle; any `focus_pane` / `activate_tab` sets it back to false (add `self.sidebar_focused = false;` at the top of both). `ShowIgnoredFiles => { self.sidebar.show_ignored = !self.sidebar.show_ignored; self.save(cx); cx.notify(); }`.
- `toggle_palette` passes `self.sidebar.show_ignored`.

README: the keyboard table gains `| Cmd+B | toggle the sidebar |` and `| Shift+Cmd+E | focus the file tree |` after the Cmd+F row, and the Notes line "Cmd+B, Shift+Cmd+E and Ctrl+Shift+G arrive with the sidebar in milestone 4." becomes "Ctrl+Shift+G arrives with the worktree explorer in milestone 4b."

- [ ] **Step 4: Run the tests and the build**

Run: `~/.cargo/bin/cargo test -p sarkos` then `~/.cargo/bin/cargo build --release 2>&1 | grep -i "^warning"`
Expected: PASS; no warnings for the project's crates. By hand if a screen is available: Cmd+B hides and shows an empty sidebar column; dragging its edge resizes it between 160 and 600 px; the width survives a relaunch of that folder.

- [ ] **Step 5: Commit**

```bash
git add crates/sarkos/src/sidebar.rs crates/sarkos/src/explorer.rs crates/sarkos/src/workspace.rs crates/sarkos/src/config.rs crates/sarkos/src/keymap.rs crates/sarkos/src/command_palette.rs crates/sarkos/src/main.rs README.md
git commit -m "Add the sidebar column: Cmd+B, a resizable edge, state saved with the workspace"
```

---

### Task 2: The file tree model: lazy directories, gitignore, rows, the poll

**Files:**
- Modify: `crates/sarkos/src/explorer.rs` (the model around Task 1's `Row`), `crates/sarkos/src/main.rs` (`Root.explorer`, the load task), `crates/sarkos/src/workspace.rs` (nothing: `expanded` is filled by `save`)
- Create: `crates/sarkos/src/explorer_watch.rs`
- Test: unit tests in `explorer.rs` (temp dirs, a temp git repo), `explorer_watch.rs`

**Interfaces:**
- Consumes: `config_watch::fingerprint(&Path) -> Option<(u128, u64)>` (works for directories: a directory's mtime changes when an entry is added or removed), `Root.sidebar.show_ignored`, `workspace::SidebarSaved.expanded`.
- Produces: `explorer::{EntryInfo { path, name, is_dir, ignored }, Row, Explorer, list_dir(&Path) -> io::Result<Vec<EntryInfo>>, mark_ignored(root: &Path, entries: &mut [EntryInfo]), read_dir_entries(root, dir) -> Vec<EntryInfo>}`, `Explorer::{new(root, expanded, show_ignored), rows(), toggle(dir) -> bool, set_loaded(dir, entries), dirs_to_load() -> Vec<PathBuf>, mark_loading(dir), invalidate(dir), expanded_dirs() -> Vec<PathBuf>, select_next(), select_prev(), selected: Option<PathBuf>}`, `Root::{explorer: Explorer, load_pending_dirs(cx)}`, `explorer_watch::start(root, cx)`.

- [ ] **Step 1: Write the failing tests**

In `explorer.rs`:

```rust
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
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkos -- explorer`
Expected: FAIL to compile.

- [ ] **Step 3: Implement the model**

`explorer.rs`:

```rust
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
/// once per expanded directory, off the UI thread.
pub fn mark_ignored(root: &Path, entries: &mut [EntryInfo]) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    if entries.is_empty() {
        return;
    }
    let Ok(mut child) = Command::new("git").arg("-C").arg(root).args(["check-ignore", "-z", "--stdin"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else {
        return;
    };
    {
        let Some(mut stdin) = child.stdin.take() else { return };
        for e in entries.iter() {
            let _ = stdin.write_all(e.path.as_os_str().as_encoded_bytes());
            let _ = stdin.write_all(b"\0");
        }
    }
    let Ok(out) = child.wait_with_output() else { return };
    // Exit 1 means nothing matched; 128 means not a repository. Either way the output says.
    let ignored: HashSet<&[u8]> = out.stdout.split(|b| *b == 0).filter(|s| !s.is_empty()).collect();
    for e in entries.iter_mut() {
        e.ignored = ignored.contains(e.path.as_os_str().as_encoded_bytes());
    }
}

/// What the window loads for one directory, on the background executor.
pub fn read_dir_entries(root: &Path, dir: &Path) -> Vec<EntryInfo> {
    match list_dir(dir) {
        Ok(mut entries) => {
            mark_ignored(root, &mut entries);
            entries
        }
        Err(e) => {
            eprintln!("sarkos: cannot read {}: {e}", dir.display());
            Vec::new()
        }
    }
}

pub struct Explorer {
    pub root: PathBuf,
    pub expanded: BTreeSet<PathBuf>,
    pub loaded: HashMap<PathBuf, Vec<EntryInfo>>,
    loading: HashSet<PathBuf>,
    pub show_ignored: bool,
    pub selected: Option<PathBuf>,
}

impl Explorer {
    pub fn new(root: PathBuf, expanded: Vec<PathBuf>, show_ignored: bool) -> Self {
        Explorer { root, expanded: expanded.into_iter().collect(), loaded: HashMap::new(), loading: HashSet::new(), show_ignored, selected: None }
    }

    /// Expanded directories whose children are not loaded and not being loaded; the root first.
    pub fn dirs_to_load(&self) -> Vec<PathBuf> {
        let mut want: Vec<PathBuf> = std::iter::once(self.root.clone()).chain(self.expanded.iter().cloned()).collect();
        want.dedup();
        want.retain(|d| !self.loaded.contains_key(d) && !self.loading.contains(d));
        want
    }

    pub fn mark_loading(&mut self, dir: &Path) {
        self.loading.insert(dir.to_path_buf());
    }

    pub fn set_loaded(&mut self, dir: &Path, entries: Vec<EntryInfo>) {
        self.loading.remove(dir);
        self.loaded.insert(dir.to_path_buf(), entries);
    }

    /// Forgets a directory's children so the next `dirs_to_load` reads them again (the poll saw
    /// it change). What is shown stays until the new read lands.
    pub fn invalidate(&mut self, dir: &Path) {
        self.loaded.remove(dir);
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
```

(Task 1 put `Row` in this file; replace that stub with the full file above.)

`explorer_watch.rs`:

```rust
//! The sidebar's "watcher" (spec 6, ruling 1): every second, the expanded directories' and the
//! open viewers' fingerprints are read on the background executor; a changed directory is read
//! again, a changed file reloaded. One stat per expanded directory per second.
use crate::config_watch::fingerprint;
use crate::Root;
use gpui::{App, Entity};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

pub const POLL: Duration = Duration::from_secs(1);

/// The paths whose fingerprint changed since `last`, and the new table.
pub fn changed(last: &HashMap<PathBuf, Option<(u128, u64)>>, now: &HashMap<PathBuf, Option<(u128, u64)>>) -> Vec<PathBuf> {
    now.iter().filter(|(p, f)| last.get(*p).is_some_and(|l| l != *f)).map(|(p, _)| p.clone()).collect()
}

pub fn start(root: Entity<Root>, cx: &mut App) {
    cx.spawn(async move |cx| {
        let mut last: HashMap<PathBuf, Option<(u128, u64)>> = HashMap::new();
        loop {
            cx.background_executor().timer(POLL).await;
            let Ok(paths) = root.update(cx, |root, _| root.watched_paths()) else { return };
            let now = cx
                .background_executor()
                .spawn(async move { paths.into_iter().map(|p| (p.clone(), fingerprint(&p))).collect::<HashMap<_, _>>() })
                .await;
            let changed = changed(&last, &now);
            last = now;
            if changed.is_empty() {
                continue;
            }
            if root.update(cx, |root, cx| root.paths_changed(&changed, cx)).is_err() {
                return;
            }
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_paths_seen_before_with_a_different_fingerprint_count_as_changed() {
        let mut last = HashMap::new();
        last.insert(PathBuf::from("/a"), Some((1, 1)));
        last.insert(PathBuf::from("/b"), Some((2, 2)));
        last.insert(PathBuf::from("/gone"), Some((3, 3)));
        let mut now = HashMap::new();
        now.insert(PathBuf::from("/a"), Some((1, 1)));
        now.insert(PathBuf::from("/b"), Some((9, 2)));
        now.insert(PathBuf::from("/new"), Some((4, 4)));
        now.insert(PathBuf::from("/deleted"), None);
        last.insert(PathBuf::from("/deleted"), Some((5, 5)));
        let mut c = changed(&last, &now);
        c.sort();
        assert_eq!(c, [PathBuf::from("/b"), PathBuf::from("/deleted")], "a new path is a baseline, not a change; a deleted one is a change");
    }
}
```

`main.rs`:
- `Root` gains `pub explorer: explorer::Explorer`, built in `new` from `project.clone()`, the saved `expanded` (or empty) and `sidebar.show_ignored`.
- `save()` fills `expanded: self.explorer.expanded_dirs()`.
- Methods:

```rust
    /// Reads every expanded directory that is not loaded yet, off the UI thread, and applies
    /// each result as it lands.
    pub fn load_pending_dirs(&mut self, cx: &mut Context<Self>) {
        for dir in self.explorer.dirs_to_load() {
            self.explorer.mark_loading(&dir);
            let root = self.project.clone();
            let this = cx.entity();
            let d = dir.clone();
            cx.spawn(async move |_, cx| {
                let entries = cx.background_executor().spawn(async move { explorer::read_dir_entries(&root, &d) }).await;
                let _ = this.update(cx, |root, cx| {
                    root.explorer.set_loaded(&dir, entries);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// What the poll fingerprints: the expanded directories (and the root), and the open
    /// viewers' files (Task 5 adds them).
    pub fn watched_paths(&self) -> Vec<PathBuf> {
        let mut v = vec![self.project.clone()];
        v.extend(self.explorer.expanded_dirs());
        v
    }

    pub fn paths_changed(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        for p in paths {
            self.explorer.invalidate(p);
        }
        self.load_pending_dirs(cx);
    }
```

`main` calls `explorer_watch::start(root.clone(), cx)` next to `config_watch::start`, and `root.update(cx, |root, cx| root.load_pending_dirs(cx))` once after the window opens. `ShowIgnoredFiles` sets `self.explorer.show_ignored` too (keep the two in step: make `Explorer.show_ignored` the one source and have `Sidebar.show_ignored` go: `save()` and the palette read `self.explorer.show_ignored`; adjust Task 1's field accordingly, the simplest is to remove `Sidebar.show_ignored` now and pass `explorer.show_ignored` everywhere).

- [ ] **Step 4: Run the tests**

Run: `~/.cargo/bin/cargo test -p sarkos -- explorer`
Expected: PASS (the git test needs `git` on PATH, which the daemon tests already assume).

- [ ] **Step 5: Commit**

```bash
git add crates/sarkos/src/explorer.rs crates/sarkos/src/explorer_watch.rs crates/sarkos/src/main.rs crates/sarkos/src/sidebar.rs crates/sarkos/src/command_palette.rs
git commit -m "Model the file tree: lazy directories, gitignore through git, visible rows, a one-second poll"
```

---

### Task 3: Tree rows: click, preview and pin, context menu, keyboard, file drag

**Files:**
- Modify: `crates/sarkos/src/sidebar.rs` (`render_rows`, the context menu), `crates/sarkos/src/drag.rs` (`FileDrag`), `crates/sarkos/src/pane_view.rs` (drop on a terminal; preview tabs dimmed), `crates/sarkos/src/main.rs` (`open_viewer`, `preview_tab`, `context_menu`, the tree's key handling), `crates/sarkos/src/tree.rs` (`find_viewer`)
- Test: unit tests in `main.rs` (the pure open decision), `tree.rs`, `drag.rs`, `sidebar.rs`

**Interfaces:**
- Consumes: Task 2's `Explorer` and `Row`; `Tree::{add_tab, replace_tab, activate, tab_id, locate, pane}`; `Terminal::paste`; `cx.reveal_path`, `cx.write_to_clipboard`, `ClipboardItem::new_string`.
- Produces: `Tree::find_viewer(pane, path) -> Option<usize>`, `main::OpenPlan { Activate(idx) | Replace(idx) | Add }` and `open_plan(tree, pane, path, preview_tab, pin) -> OpenPlan`, `Root::{preview_tab: Option<TabId>, context_menu: Option<ContextMenu>, open_viewer(path, pin, cx), tree_key(ks, cx)}`, `drag::{FileDrag, paste_text(&Path) -> String}`, `sidebar::{render_rows, render_context_menu}`. Task 5 fills the tab's content; until then a viewer tab shows the placeholder text already there.

- [ ] **Step 1: Write the failing tests**

In `tree.rs`'s tests:

```rust
    #[test]
    fn find_viewer_locates_a_tab_by_path_in_one_pane() {
        let mut t = Tree::new(Tab::Terminal { session: SessionId(1) });
        let root = t.panes()[0];
        t.add_tab(root, Tab::Viewer { path: "/p/a.rs".into() });
        let right = t.split(root, Side::Right, Tab::Viewer { path: "/p/b.rs".into() }).unwrap();
        assert_eq!(t.find_viewer(root, Path::new("/p/a.rs")), Some(1));
        assert_eq!(t.find_viewer(root, Path::new("/p/b.rs")), None, "another pane's tab does not count");
        assert_eq!(t.find_viewer(right, Path::new("/p/b.rs")), Some(0));
    }
```

In `main.rs`'s tests:

```rust
    #[test]
    fn a_preview_is_replaced_and_a_pin_is_kept() {
        use crate::tree::Tab;
        let mut tree = Tree::new(Tab::Terminal { session: SessionId(1) });
        let pane = tree.panes()[0];
        // Nothing open yet: a new tab.
        assert_eq!(open_plan(&tree, pane, Path::new("/p/a.rs"), None, false), OpenPlan::Add);
        let a = tree.add_tab(pane, Tab::Viewer { path: "/p/a.rs".into() });
        let a_id = tree.tab_id(pane, a).unwrap();
        // The same file again, previewed: just activate it.
        assert_eq!(open_plan(&tree, pane, Path::new("/p/a.rs"), Some(a_id), false), OpenPlan::Activate(a));
        // Another file while a preview is open in this pane: replace the preview.
        assert_eq!(open_plan(&tree, pane, Path::new("/p/b.rs"), Some(a_id), false), OpenPlan::Replace(a));
        // Pinned (double click): a new tab, the preview stays.
        assert_eq!(open_plan(&tree, pane, Path::new("/p/b.rs"), Some(a_id), true), OpenPlan::Add);
        // No preview in this pane (it was pinned or lives elsewhere): a new tab.
        assert_eq!(open_plan(&tree, pane, Path::new("/p/b.rs"), None, false), OpenPlan::Add);
        let other = tree.split(pane, Side::Right, Tab::Terminal { session: SessionId(2) }).unwrap();
        assert_eq!(open_plan(&tree, other, Path::new("/p/b.rs"), Some(a_id), false), OpenPlan::Add, "the preview is in another pane");
        // An already open pinned tab for the file: activate, whatever the mode.
        assert_eq!(open_plan(&tree, pane, Path::new("/p/a.rs"), None, true), OpenPlan::Activate(a));
    }
```

In `drag.rs`'s tests:

```rust
    #[test]
    fn a_dropped_path_is_quoted_only_when_it_needs_it() {
        assert_eq!(paste_text(Path::new("/Users/me/src/main.rs")), "/Users/me/src/main.rs ");
        assert_eq!(paste_text(Path::new("/Users/me/My Docs/a b.txt")), "'/Users/me/My Docs/a b.txt' ");
        assert_eq!(paste_text(Path::new("/x/it's.txt")), "'/x/it'\\''s.txt' ");
    }
```

In `sidebar.rs`'s tests:

```rust
    #[test]
    fn the_context_menu_lists_the_four_file_actions_in_order() {
        let items = context_menu_items(Path::new("/p/src/a.rs"), Path::new("/p"));
        let labels: Vec<&str> = items.iter().map(|(l, _)| *l).collect();
        assert_eq!(labels, ["Reveal in Finder", "Copy path", "Copy relative path", "Open in default app"]);
        assert_eq!(items[1].1, MenuAction::CopyText("/p/src/a.rs".into()));
        assert_eq!(items[2].1, MenuAction::CopyText("src/a.rs".into()));
        assert_eq!(context_menu_items(Path::new("/elsewhere/f"), Path::new("/p"))[2].1, MenuAction::CopyText("/elsewhere/f".into()), "outside the project the full path");
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkos -- find_viewer a_preview_is_replaced a_dropped_path context_menu`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`tree.rs`:

```rust
    /// The index of the viewer tab showing `path` in `pane`, if any.
    pub fn find_viewer(&self, pane: PaneId, path: &Path) -> Option<usize> {
        self.pane(pane)?.tabs.iter().position(|t| matches!(t, Tab::Viewer { path: p } if p == path))
    }
```

`main.rs`, near the top:

```rust
/// How a click in the tree opens a file in `pane` (spec 6, VS Code's preview tabs): the tab
/// already showing it is activated; else a preview tab in this pane is replaced, unless the
/// click pins; else a new tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenPlan {
    Activate(usize),
    Replace(usize),
    Add,
}

pub fn open_plan(tree: &Tree, pane: PaneId, path: &Path, preview_tab: Option<TabId>, pin: bool) -> OpenPlan {
    if let Some(idx) = tree.find_viewer(pane, path) {
        return OpenPlan::Activate(idx);
    }
    if !pin {
        if let Some(p) = preview_tab {
            if let Some((in_pane, idx)) = tree.locate(p) {
                if in_pane == pane {
                    return OpenPlan::Replace(idx);
                }
            }
        }
    }
    OpenPlan::Add
}
```

`Root` gains `pub preview_tab: Option<TabId>` (doc: "The tab a single click previews into; the next single click replaces it. `None` once pinned or closed.") and `pub context_menu: Option<sidebar::ContextMenu>`. Methods:

```rust
    /// Opens `path` in the focused pane as a preview (single click) or pinned (double click,
    /// Enter). Task 5 gives the tab its content; here the tab exists and is active.
    pub fn open_viewer(&mut self, path: &Path, pin: bool, cx: &mut Context<Self>) {
        self.sidebar_focused = false;
        let pane = self.tree.focused;
        let plan = open_plan(&self.tree, pane, path, self.preview_tab, pin);
        let idx = match plan {
            OpenPlan::Activate(idx) => {
                if pin {
                    if self.tree.tab_id(pane, idx) == self.preview_tab {
                        self.preview_tab = None;
                    }
                }
                idx
            }
            OpenPlan::Replace(idx) => {
                self.tree.replace_tab(pane, idx, Tab::Viewer { path: path.to_path_buf() });
                idx
            }
            OpenPlan::Add => {
                let idx = self.tree.add_tab(pane, Tab::Viewer { path: path.to_path_buf() });
                self.preview_tab = if pin { None } else { self.tree.tab_id(pane, idx) };
                idx
            }
        };
        self.tree.activate(pane, idx);
        self.explorer.selected = Some(path.to_path_buf());
        self.save(cx);
        cx.notify();
    }
```

`close_tab`: when the closed tab's id equals `preview_tab`, set `preview_tab = None`. `drop_tab` (a dragged tab is a deliberate placement): if the dragged tab is the preview, pin it (`preview_tab = None`).

`sidebar.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    Reveal(PathBuf),
    CopyText(String),
    OpenDefault(PathBuf),
}

/// The right-click menu for a tree entry (spec 6).
pub fn context_menu_items(path: &Path, project: &Path) -> Vec<(&'static str, MenuAction)> {
    let relative = path.strip_prefix(project).unwrap_or(path).to_string_lossy().into_owned();
    vec![
        ("Reveal in Finder", MenuAction::Reveal(path.to_path_buf())),
        ("Copy path", MenuAction::CopyText(path.to_string_lossy().into_owned())),
        ("Copy relative path", MenuAction::CopyText(relative)),
        ("Open in default app", MenuAction::OpenDefault(path.to_path_buf())),
    ]
}

pub struct ContextMenu {
    pub path: PathBuf,
    /// Window position of the click.
    pub at: gpui::Point<gpui::Pixels>,
}
```

`render_rows` (replacing Task 1's stub): a scrolling column (`div().id("explorer-rows").flex_1().min_h_0().overflow_y_scroll().track_scroll(&root.sidebar.scroll)`) with one child per `root.explorer.rows()`:

```rust
    for row in rows {
        let path = row.path.clone();
        let selected = root.explorer.selected.as_deref() == Some(path.as_path());
        let color = if row.ignored { theme.tree.ignored.hsla() } else { theme.chrome.text.hsla() };
        let chevron = if !row.is_dir { " " } else if row.loading { "\u{2026}" } else if row.expanded { "\u{25be}" } else { "\u{25b8}" };
        let p_click = path.clone();
        let p_menu = path.clone();
        let p_drag = path.clone();
        let is_dir = row.is_dir;
        column = column.child(
            div()
                .id(("row", hash_of(&path)))
                .h(px(ROW_H))
                .pl(px(10.0 + row.depth as f32 * INDENT))
                .pr(px(8.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .when(selected, |d| d.bg(theme.chrome.tab_active.hsla()))
                .hover(|s| s.bg(theme.chrome.tab_inactive.hsla()))
                .text_color(color)
                .cursor_pointer()
                .on_click(cx.listener(move |root, ev: &ClickEvent, _window, cx| {
                    if is_dir {
                        root.explorer.toggle(&p_click);
                        root.explorer.selected = Some(p_click.clone());
                        root.load_pending_dirs(cx);
                        root.save(cx);
                        cx.notify();
                    } else {
                        root.open_viewer(&p_click, ev.up.click_count >= 2, cx);
                    }
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |root, ev: &MouseDownEvent, _window, cx| {
                        root.context_menu = Some(ContextMenu { path: p_menu.clone(), at: ev.position });
                        cx.notify();
                    }),
                )
                .when(!is_dir, |d| {
                    d.on_drag(crate::drag::FileDrag { path: p_drag.clone(), position: Default::default(), bg: theme.chrome.banner, fg: theme.chrome.text, family: family.clone() }, |d: &crate::drag::FileDrag, position, _window, cx| cx.new(|_| d.clone().at(position)))
                })
                .child(div().w(px(12.0)).text_color(theme.chrome.text_dim.hsla()).child(chevron))
                .child(div().min_w_0().truncate().child(row.name.clone())),
        );
    }
```

(`hash_of` is the `fnv1a` from `paths.rs`, made `pub(crate)`, over the path bytes: an `ElementId::NamedInteger("row", n)`. `ClickEvent.up.click_count` is the double-click signal in gpui 0.2.2; if the field path differs, grep `click_count` in `interactive.rs`.) The column's key handling for the tree (`key_context("Explorer")` is on the sidebar div from Task 1) goes on that div: `.on_key_down(cx.listener(|root, ev: &KeyDownEvent, _w, cx| root.tree_key(&ev.keystroke, cx)))`, and in `main.rs`:

```rust
    /// Keys in the file tree (ruling 7): Up/Down move, Right expands or enters, Left collapses or
    /// goes to the parent, Enter opens pinned, Space previews, Escape returns to the pane.
    pub fn tree_key(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) {
        if ks.modifiers.platform || ks.modifiers.control || ks.modifiers.alt {
            return;
        }
        let rows = self.explorer.rows();
        let selected = self.explorer.selected.clone().and_then(|s| rows.iter().find(|r| r.path == s).cloned());
        match ks.key.as_str() {
            "down" => self.explorer.select_next(),
            "up" => self.explorer.select_prev(),
            "right" => match selected {
                Some(r) if r.is_dir && !r.expanded => {
                    self.explorer.toggle(&r.path);
                    self.load_pending_dirs(cx);
                }
                Some(r) if r.is_dir => self.explorer.select_next(),
                _ => {}
            },
            "left" => match selected {
                Some(r) if r.is_dir && r.expanded => {
                    self.explorer.toggle(&r.path);
                }
                Some(r) => {
                    if let Some(parent) = r.path.parent() {
                        if parent != self.project {
                            self.explorer.selected = Some(parent.to_path_buf());
                        }
                    }
                }
                None => {}
            },
            "enter" | "space" => match selected {
                Some(r) if r.is_dir => {
                    self.explorer.toggle(&r.path);
                    self.load_pending_dirs(cx);
                }
                Some(r) => {
                    let pin = ks.key == "enter";
                    self.open_viewer(&r.path, pin, cx);
                    if !pin {
                        self.sidebar_focused = true;
                    }
                    return;
                }
                None => self.explorer.select_next(),
            },
            "escape" => self.sidebar_focused = false,
            _ => return,
        }
        self.save(cx);
        cx.notify();
    }
```

The context menu renders at the root level (after the palette): `sidebar::render_context_menu(root, cx) -> Option<Div>`: an absolute div at `menu.at` with the four rows, each `on_click` running the action then clearing the menu:

```rust
pub fn run_menu_action(action: &MenuAction, cx: &mut Context<Root>) {
    match action {
        MenuAction::Reveal(p) => cx.reveal_path(p),
        MenuAction::CopyText(s) => cx.write_to_clipboard(gpui::ClipboardItem::new_string(s.clone())),
        MenuAction::OpenDefault(p) => {
            if let Err(e) = std::process::Command::new("open").arg(p).spawn() {
                eprintln!("sarkos: cannot open {}: {e}", p.display());
            }
        }
    }
}
```

The root's existing `on_mouse_down` (any button, the one that clears drop targets, or a new listener on the root div) sets `context_menu = None` when a click lands outside the menu (the menu's own rows stop propagation). Escape closes it too (handle in `tree_key` when `context_menu.is_some()` and in the palette-style key path: the simplest is the root-level key listener, if one exists; else the menu div takes focus on open with its own `track_focus` and `on_key_down`).

`drag.rs`:

```rust
/// A file dragged out of the tree: dropped on a terminal, its path is pasted (spec 6).
#[derive(Debug, Clone, PartialEq)]
pub struct FileDrag {
    pub path: PathBuf,
    pub position: Point<Pixels>,
    pub bg: Rgb,
    pub fg: Rgb,
    pub family: SharedString,
}

impl FileDrag {
    pub fn at(mut self, position: Point<Pixels>) -> Self {
        self.position = position;
        self
    }
}

impl Render for FileDrag {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let name = self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        div().pl(self.position.x + px(8.0)).pt(self.position.y + px(8.0)).child(
            div().px(px(10.0)).py(px(4.0)).bg(self.bg.hsla()).text_color(self.fg.hsla()).font_family(self.family.clone()).text_size(px(12.0)).opacity(0.9).child(name),
        )
    }
}

/// What a dropped file types into a terminal: the path, single-quoted when it has anything a
/// shell would read, then a space so the next word starts clean.
pub fn paste_text(path: &Path) -> String {
    let s = path.to_string_lossy();
    let plain = s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c));
    if plain {
        format!("{s} ")
    } else {
        format!("'{}' ", s.replace('\'', "'\\''"))
    }
}
```

`pane_view.rs`: `render_terminal`'s div gains `.on_drop(cx.listener(move |_root, d: &crate::drag::FileDrag, _window, cx| { t_drop.update(cx, |t, _| t.paste(&crate::drag::paste_text(&d.path))); }))` with `let t_drop = terminal.clone();`, and `render_busy` / `render_no_tabs` need nothing. In `render_tab_bar`, a tab whose id is `root.preview_tab` uses `dim` for its text colour even when active (ruling 4).

- [ ] **Step 4: Run the tests and the build**

Run: `~/.cargo/bin/cargo test -p sarkos` then the warning check.
Expected: PASS. By hand if a screen is available: clicking a file opens a dimmed viewer tab (its content is the milestone 4 placeholder until Task 5); clicking another replaces it; a double click pins; right click shows the four actions and "Copy path" lands on the clipboard; dragging a file onto a terminal pastes its path; Shift+Cmd+E then arrows and Enter open a file.

- [ ] **Step 5: Commit**

```bash
git add crates/sarkos/src/sidebar.rs crates/sarkos/src/drag.rs crates/sarkos/src/pane_view.rs crates/sarkos/src/main.rs crates/sarkos/src/tree.rs crates/sarkos/src/paths.rs
git commit -m "Draw the file tree: click to preview, double click to pin, a context menu, keys, file drag"
```

---

### Task 4: The viewer model and syntax highlighting

**Files:**
- Create: `crates/sarkos/src/viewer.rs`, `crates/sarkos/src/highlight.rs`
- Modify: `crates/sarkos/Cargo.toml` (syntect), `Cargo.lock`, `crates/sarkos/src/config.rs` (`[viewer] highlight_limit_bytes`), `crates/sarkos/src/main.rs` (`mod` lines, `Root.highlighter`)
- Test: unit tests in `viewer.rs`, `highlight.rs`, `config.rs`

**Interfaces:**
- Consumes: `theme::ViewerColors`, `theme::Chrome.text`, `palette::Rgb`, `scroll::ScrollAnimator`, `line_edit::LineEdit`, `config_watch::fingerprint`.
- Produces: `viewer::{Span { start, end, color, bold, italic }, Content::{Text { lines, spans }, TooBig(u64), Binary, Unreadable(String)}, Hit { line, start, end }, classify(bytes, limit) -> Classified, split_lines(&str) -> Vec<String>, find_hits(lines, needle) -> Vec<Hit>, Viewer, ViewerFind, Loaded}`, `highlight::{Highlighter::new(colors: &ViewerColors, text: Rgb), highlight(&self, path, lines) -> Vec<Vec<Span>>}`, `config.viewer.highlight_limit_bytes: usize` (default 5 MiB), `Root.highlighter: Option<Arc<Highlighter>>` (built on the background executor at launch).

- [ ] **Step 1: Write the failing tests**

`viewer.rs`:

```rust
//! A read-only viewer tab (spec 6): the file loaded once off the UI thread, classified (text,
//! too big to highlight, binary), split into lines and highlighted; a find over the lines; a
//! scroll position in lines; reloaded when the poll sees the file change.

use crate::line_edit::LineEdit;
use crate::palette::Rgb;
use crate::scroll::ScrollAnimator;
use gpui::{Context, FocusHandle};
use std::path::PathBuf;
use std::time::Instant;

/// Bytes a binary check looks at: a NUL in the first 8 KiB means not text.
pub const SNIFF: usize = 8 * 1024;
pub const MAX_HITS: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// Byte range within the line.
    pub start: usize,
    pub end: usize,
    pub color: Rgb,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    Text { lines: Vec<String>, spans: Vec<Vec<Span>> },
    TooBig(u64),
    Binary,
    Unreadable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classified {
    Text(String),
    TooBig,
    Binary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_text_too_big_and_binary() {
        assert_eq!(classify(b"hello\nworld\n", 100), Classified::Text("hello\nworld\n".into()));
        assert_eq!(classify(b"hello\nworld\n", 5), Classified::TooBig, "over the limit: no highlighting, plain lines");
        assert_eq!(classify(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR", 100), Classified::Binary);
        let mut big = vec![b'a'; SNIFF + 10];
        big[SNIFF + 5] = 0;
        assert!(matches!(classify(&big, 1 << 20), Classified::Text(_)), "a NUL past the sniff window is not checked");
        assert!(matches!(classify(&[0xff, 0xfe, b'a'], 100), Classified::Text(_)), "invalid UTF-8 is shown lossily, not called binary");
        assert_eq!(classify(b"", 0), Classified::Text(String::new()), "an empty file is text under any limit");
    }

    #[test]
    fn lines_split_on_lf_and_crlf_without_a_phantom_last_line() {
        assert_eq!(split_lines("a\nb\n"), ["a", "b"]);
        assert_eq!(split_lines("a\r\nb"), ["a", "b"]);
        assert_eq!(split_lines(""), Vec::<String>::new());
        assert_eq!(split_lines("\n"), [""]);
        assert_eq!(split_lines("one"), ["one"]);
    }

    #[test]
    fn find_is_case_insensitive_in_order_and_capped() {
        let lines: Vec<String> = ["Hello hello", "nothing", "HELLO"].iter().map(|s| s.to_string()).collect();
        let hits = find_hits(&lines, "hello");
        assert_eq!(hits, [Hit { line: 0, start: 0, end: 5 }, Hit { line: 0, start: 6, end: 11 }, Hit { line: 2, start: 0, end: 5 }]);
        assert!(find_hits(&lines, "").is_empty());
        let many: Vec<String> = vec!["aa".to_string(); MAX_HITS];
        assert_eq!(find_hits(&many, "a").len(), MAX_HITS, "capped");
        let unicode = vec!["caf\u{e9} CAF\u{c9}".to_string()];
        let h = find_hits(&unicode, "caf\u{e9}");
        assert_eq!(h.len(), 2);
        assert_eq!(&unicode[0][h[1].start..h[1].end], "CAF\u{c9}", "byte ranges land on char boundaries");
    }

    #[test]
    fn a_hit_is_stepped_through_with_wraparound() {
        assert_eq!(step(0, 3, 1), 1);
        assert_eq!(step(2, 3, 1), 0);
        assert_eq!(step(0, 3, -1), 2);
        assert_eq!(step(0, 0, 1), 0);
    }
}
```

`highlight.rs`:

```rust
//! Syntax colours for the viewer (spec 6): syntect's TextMate grammars, with a theme built from
//! the config theme's nine viewer colours so every file matches the window. The syntax set
//! loads once, off the UI thread, at launch.

use crate::palette::Rgb;
use crate::theme::ViewerColors;
use crate::viewer::Span;
use std::path::Path;
use syntect::highlighting::{Color, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings};
use syntect::parsing::SyntaxSet;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::rgb;

    fn colors() -> ViewerColors {
        ViewerColors {
            keyword: rgb(0xc9, 0xa4, 0xe8),
            string: rgb(0x8f, 0xd9, 0xa8),
            comment: rgb(0x7c, 0x87, 0x98),
            number: rgb(0xe8, 0xc9, 0x7a),
            function: rgb(0x8a, 0xb8, 0xf0),
            type_name: rgb(0x7f, 0xd4, 0xd4),
            constant: rgb(0xf0, 0xa8, 0xa8),
            operator: rgb(0xc8, 0xcc, 0xd4),
            punctuation: rgb(0xa8, 0xb0, 0xbc),
        }
    }

    #[test]
    fn rust_keywords_strings_and_comments_get_their_colours() {
        let h = Highlighter::new(&colors(), rgb(0xd6, 0xda, 0xe0));
        let lines: Vec<String> = ["fn main() {", "    let s = \"hi\"; // note", "}"].iter().map(|s| s.to_string()).collect();
        let spans = h.highlight(Path::new("/p/main.rs"), &lines);
        assert_eq!(spans.len(), 3);
        let at = |line: usize, byte: usize| spans[line].iter().find(|s| s.start <= byte && byte < s.end).map(|s| s.color).expect("covered");
        assert_eq!(at(0, 0), colors().keyword, "fn");
        assert_eq!(at(1, 13), colors().string, "the string literal");
        assert_eq!(at(1, 20), colors().comment, "the comment");
        assert_eq!(at(0, 3), colors().function, "main");
        // Every byte of every line is covered by exactly one span, in order.
        for (i, line) in lines.iter().enumerate() {
            let mut pos = 0;
            for s in &spans[i] {
                assert_eq!(s.start, pos, "line {i}: spans are contiguous");
                pos = s.end;
            }
            assert_eq!(pos, line.len(), "line {i}: spans cover the line");
        }
    }

    #[test]
    fn an_unknown_extension_is_one_plain_span_per_line() {
        let h = Highlighter::new(&colors(), rgb(1, 2, 3));
        let lines: Vec<String> = ["some text", ""].iter().map(|s| s.to_string()).collect();
        let spans = h.highlight(Path::new("/p/notes.unknownext"), &lines);
        assert_eq!(spans[0], vec![Span { start: 0, end: 9, color: rgb(1, 2, 3), bold: false, italic: false }]);
        assert_eq!(spans[1], Vec::<Span>::new(), "an empty line has no span");
    }

    #[test]
    fn the_theme_maps_the_nine_colours_onto_scopes() {
        let t = scope_theme(&colors(), rgb(9, 9, 9));
        assert_eq!(t.settings.foreground, Some(Color { r: 9, g: 9, b: 9, a: 255 }));
        let selectors: Vec<String> = t.scopes.iter().map(|i| i.scope.selectors.iter().map(|s| format!("{:?}", s)).collect::<Vec<_>>().join("|")).collect();
        assert_eq!(t.scopes.len(), 9);
        assert!(selectors[0].contains("keyword"), "{selectors:?}");
    }
}
```

`config.rs`'s tests:

```rust
    #[test]
    fn the_viewer_limit_defaults_to_five_mebibytes_and_must_be_positive() {
        assert_eq!(Config::default().viewer.highlight_limit_bytes, 5 * 1024 * 1024);
        let c: Config = toml::from_str("[viewer]\nhighlight_limit_bytes = 1000\n").unwrap();
        assert_eq!(c.viewer.highlight_limit_bytes, 1000);
        let zero: Config = toml::from_str("[viewer]\nhighlight_limit_bytes = 0\n").unwrap();
        assert!(zero.validate().unwrap_err().contains("viewer.highlight_limit_bytes"));
        assert!(toml::from_str::<Config>(DEFAULT_CONFIG).unwrap().validate().is_ok());
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkos -- viewer highlight viewer_limit`
Expected: FAIL to compile (add `mod viewer; mod highlight;` to `main.rs` and the `syntect` dependency to `crates/sarkos/Cargo.toml` as the constraints give it; `cargo` fetches it).

- [ ] **Step 3: Implement**

`viewer.rs`, above the tests:

```rust
pub fn classify(bytes: &[u8], limit: usize) -> Classified {
    if bytes[..bytes.len().min(SNIFF)].contains(&0) {
        return Classified::Binary;
    }
    if bytes.len() > limit {
        return Classified::TooBig;
    }
    Classified::Text(String::from_utf8_lossy(bytes).into_owned())
}

/// Lines without their terminators; a trailing newline adds no empty last line.
pub fn split_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    text.lines().map(str::to_string).collect()
}

/// Every case-insensitive occurrence, in reading order, as byte ranges, at most `MAX_HITS`.
pub fn find_hits(lines: &[String], needle: &str) -> Vec<Hit> {
    let mut out = Vec::new();
    if needle.is_empty() {
        return out;
    }
    let needle_lc = needle.to_lowercase();
    for (i, line) in lines.iter().enumerate() {
        // Lowercasing can change byte lengths (e.g. some scripts): search char by char instead.
        let chars: Vec<(usize, char)> = line.char_indices().collect();
        let n: Vec<char> = needle_lc.chars().collect();
        let mut c = 0;
        while c + n.len() <= chars.len() {
            let matches = (0..n.len()).all(|k| chars[c + k].1.to_lowercase().eq(n[k].to_lowercase()));
            if matches {
                let start = chars[c].0;
                let end = chars.get(c + n.len()).map(|(b, _)| *b).unwrap_or(line.len());
                out.push(Hit { line: i, start, end });
                if out.len() >= MAX_HITS {
                    return out;
                }
                c += n.len();
            } else {
                c += 1;
            }
        }
    }
    out
}

/// The next hit index, wrapping.
pub fn step(current: usize, len: usize, delta: isize) -> usize {
    crate::keymap::tab_cycle(current, len, delta)
}

pub struct ViewerFind {
    pub query: LineEdit,
    pub hits: Vec<Hit>,
    pub current: usize,
    pub focus: FocusHandle,
}

/// What the background load produced, applied on the UI thread.
pub struct Loaded {
    pub content: Content,
    pub fingerprint: Option<(u128, u64)>,
}

pub struct Viewer {
    pub path: PathBuf,
    pub content: Content,
    /// `None` until the first load lands.
    pub loaded: bool,
    pub scroll: ScrollAnimator,
    pub focus: FocusHandle,
    pub find: Option<ViewerFind>,
    pub font_size: f32,
    pub line_height: f32,
    pub last_frame: Option<Instant>,
    /// Rows the element showed at its last paint, for page keys and the scroll limit.
    pub rows_visible: usize,
    pub fingerprint: Option<(u128, u64)>,
}

impl Viewer {
    pub fn new(path: PathBuf, font_size: f32, line_height: f32, cx: &mut Context<Self>) -> Self {
        Viewer {
            path,
            content: Content::Text { lines: Vec::new(), spans: Vec::new() },
            loaded: false,
            scroll: ScrollAnimator::new(),
            focus: cx.focus_handle(),
            find: None,
            font_size,
            line_height,
            last_frame: None,
            rows_visible: 1,
            fingerprint: None,
        }
    }

    pub fn line_count(&self) -> usize {
        match &self.content {
            Content::Text { lines, .. } => lines.len(),
            _ => 0,
        }
    }

    /// The furthest the view can scroll: the last line at the top of the viewport.
    pub fn max_scroll(&self) -> f32 {
        self.line_count().saturating_sub(1) as f32
    }

    pub fn apply(&mut self, loaded: Loaded, cx: &mut Context<Self>) {
        self.content = loaded.content;
        self.fingerprint = loaded.fingerprint;
        self.loaded = true;
        self.scroll.clamp_max(self.max_scroll());
        if let Some(f) = &mut self.find {
            f.hits = match &self.content {
                Content::Text { lines, .. } => find_hits(lines, &f.query.text()),
                _ => Vec::new(),
            };
            f.current = f.current.min(f.hits.len().saturating_sub(1));
        }
        cx.notify();
    }

    pub fn scroll_lines(&mut self, lines: f32, cx: &mut Context<Self>) {
        self.scroll.scroll_by(lines, self.max_scroll());
        cx.notify();
    }

    pub fn open_find(&mut self, cx: &mut Context<Self>) {
        if self.find.is_none() {
            self.find = Some(ViewerFind { query: LineEdit::new(), hits: Vec::new(), current: 0, focus: cx.focus_handle() });
        }
        cx.notify();
    }

    pub fn close_find(&mut self, cx: &mut Context<Self>) {
        self.find = None;
        cx.notify();
    }

    /// Re-runs the search after the query changed and jumps to the first hit.
    pub fn refind(&mut self, cx: &mut Context<Self>) {
        let Some(f) = &mut self.find else { return };
        f.hits = match &self.content {
            Content::Text { lines, .. } => find_hits(lines, &f.query.text()),
            _ => Vec::new(),
        };
        f.current = 0;
        self.show_current_hit();
        cx.notify();
    }

    pub fn find_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(f) = &mut self.find {
            f.current = step(f.current, f.hits.len(), delta);
        }
        self.show_current_hit();
        cx.notify();
    }

    /// Scrolls so the current hit's line is in view, a third of the way down.
    fn show_current_hit(&mut self) {
        let Some(f) = &self.find else { return };
        let Some(h) = f.hits.get(f.current) else { return };
        let target = (h.line as f32 - (self.rows_visible as f32 / 3.0).floor()).max(0.0);
        self.scroll.target = target.min(self.max_scroll());
    }
}
```

(`LineEdit::text()` exists or `LineEdit.text` is a field: read `line_edit.rs` and use what is there; the find bar's key handling, `find_key`, mirrors the terminal's `find_key` in `find.rs`: Escape closes, Enter steps forward, Shift+Enter back, else `query.handle(ks)` then `refind` when it returned an edit; put it in Task 5 with the bar.)

`highlight.rs`:

```rust
fn color(c: Rgb) -> Color {
    Color { r: c.r, g: c.g, b: c.b, a: 255 }
}

fn item(selector: &str, c: Rgb) -> ThemeItem {
    ThemeItem {
        scope: ScopeSelectors::from_str(selector).expect("a valid scope selector"),
        style: StyleModifier { foreground: Some(color(c)), background: None, font_style: None },
    }
}

/// A syntect theme from the nine viewer colours (spec 7: "viewer: syntax colours mapped onto
/// the highlighter's scopes"). Later items win for a more specific scope, so operators and
/// numbers come after keywords and constants.
pub fn scope_theme(colors: &ViewerColors, text: Rgb) -> Theme {
    Theme {
        name: Some("sarkos".into()),
        author: None,
        settings: ThemeSettings { foreground: Some(color(text)), ..Default::default() },
        scopes: vec![
            item("keyword, storage.modifier", colors.keyword),
            item("string", colors.string),
            item("comment", colors.comment),
            item("constant", colors.constant),
            item("constant.numeric", colors.number),
            item("entity.name.function, support.function, variable.function", colors.function),
            item("entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, storage.type, support.type", colors.type_name),
            item("keyword.operator", colors.operator),
            item("punctuation", colors.punctuation),
        ],
    }
}

pub struct Highlighter {
    syntaxes: SyntaxSet,
    theme: Theme,
    text: Rgb,
}

impl Highlighter {
    /// Loads the default grammars (about 100 ms): call it on the background executor.
    pub fn new(colors: &ViewerColors, text: Rgb) -> Self {
        Highlighter { syntaxes: SyntaxSet::load_defaults_newlines(), theme: scope_theme(colors, text), text }
    }

    /// One span list per line, contiguous and covering the line; an empty line has none. A file
    /// no grammar claims gets one plain span per line.
    pub fn highlight(&self, path: &Path, lines: &[String]) -> Vec<Vec<Span>> {
        let syntax = self
            .syntaxes
            .find_syntax_for_file(path)
            .ok()
            .flatten()
            .or_else(|| lines.first().and_then(|l| self.syntaxes.find_syntax_by_first_line(l)));
        let Some(syntax) = syntax else {
            return lines.iter().map(|l| plain(l, self.text)).collect();
        };
        let mut h = syntect::easy::HighlightLines::new(syntax, &self.theme);
        let mut out = Vec::with_capacity(lines.len());
        for line in lines {
            // The grammars expect the newline; the spans are measured on the line without it.
            let with_nl = format!("{line}\n");
            let mut spans = Vec::new();
            let mut pos = 0;
            match h.highlight_line(&with_nl, &self.syntaxes) {
                Ok(regions) => {
                    for (style, text) in regions {
                        let text = text.strip_suffix('\n').unwrap_or(text);
                        if text.is_empty() {
                            continue;
                        }
                        let c = style.foreground;
                        spans.push(Span {
                            start: pos,
                            end: pos + text.len(),
                            color: crate::palette::rgb(c.r, c.g, c.b),
                            bold: style.font_style.contains(FontStyle::BOLD),
                            italic: style.font_style.contains(FontStyle::ITALIC),
                        });
                        pos += text.len();
                    }
                }
                Err(_) => spans = plain(line, self.text),
            }
            out.push(spans);
        }
        out
    }
}

fn plain(line: &str, text: Rgb) -> Vec<Span> {
    if line.is_empty() {
        Vec::new()
    } else {
        vec![Span { start: 0, end: line.len(), color: text, bold: false, italic: false }]
    }
}
```

(`palette::rgb` is the `Rgb` constructor the palette module already has; if it is named differently, use that. `ScopeSelectors::from_str` needs `use std::str::FromStr;`.)

`config.rs`: `pub struct ViewerConfig { pub highlight_limit_bytes: usize }` with `Default` 5 MiB, `#[serde(default, deny_unknown_fields)]`, `Config.viewer: ViewerConfig`, `validate` rejecting 0 with "viewer.highlight_limit_bytes must be at least 1", and in `DEFAULT_CONFIG` after `[sidebar]`:

```toml

[viewer]
# Files larger than this open in the viewer without syntax colours; binaries show a notice.
highlight_limit_bytes = 5242880
```

`main.rs`: `Root.highlighter: Option<Arc<highlight::Highlighter>>`, built at launch on the background executor from the theme's `viewer` colours and `chrome.text` and set on the root when ready; rebuilt whenever the theme changes (`set_theme`): a viewer open at that moment re-highlights on its next reload (Task 5 re-highlights on theme change explicitly).

- [ ] **Step 4: Run the tests**

Run: `~/.cargo/bin/cargo test -p sarkos -- viewer highlight config`
Expected: PASS. Then `~/.cargo/bin/cargo build --release 2>&1 | grep -i "^warning"`: nothing from the project's crates (new items unused until Task 5 need `#[allow(dead_code)]` with a comment naming Task 5, or `#[cfg_attr(not(test), expect(dead_code))]`).

- [ ] **Step 5: Commit**

```bash
git add crates/sarkos/Cargo.toml Cargo.lock crates/sarkos/src/viewer.rs crates/sarkos/src/highlight.rs crates/sarkos/src/config.rs crates/sarkos/src/main.rs
git commit -m "Add the viewer model and syntect highlighting through the theme's viewer colours"
```

---

### Task 5: The viewer element, find, reload, presets, README

**Files:**
- Create: `crates/sarkos/src/viewer_element.rs`
- Modify: `crates/sarkos/src/viewer.rs` (`find_key`, `paste`), `crates/sarkos/src/pane_view.rs` (render a viewer tab), `crates/sarkos/src/main.rs` (`Root.viewers`, load, reload, `Find` for viewers, font keys, text preset, theme change, `watched_paths`, `paths_changed`), `README.md`
- Test: unit tests in `viewer_element.rs` (pure layout maths), `viewer.rs`

**Interfaces:**
- Consumes: Task 4's `Viewer`, `Content`, `Hit`, `Highlighter`; Task 3's viewer tabs; `terminal_element`'s `measure_cell`-style shaping (`window.text_system().shape_line`), `ScrollAnimator`, `line_edit::render`, `config_watch::fingerprint`, `explorer_watch` (`watched_paths`, `paths_changed`).
- Produces: `viewer_element::{ViewerElement::new(viewer, family), gutter_width(line_count, cell_w) -> f32, visible_range(scroll, rows, len) -> Range<usize>}`, `Root::{viewers: HashMap<TabId, Entity<Viewer>>, viewer_for(tab) -> Option<Entity<Viewer>>, load_viewer(tab, cx), reload_viewers_for(path, cx)}`, `Viewer::{find_key, paste}`.

- [ ] **Step 1: Write the failing tests**

`viewer_element.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gutter_fits_the_widest_line_number_plus_padding() {
        assert_eq!(gutter_width(9, 10.0), 1.0 * 10.0 + GUTTER_PAD * 2.0);
        assert_eq!(gutter_width(10, 10.0), 2.0 * 10.0 + GUTTER_PAD * 2.0);
        assert_eq!(gutter_width(12345, 8.0), 5.0 * 8.0 + GUTTER_PAD * 2.0);
        assert_eq!(gutter_width(0, 8.0), 1.0 * 8.0 + GUTTER_PAD * 2.0, "an empty file still has a gutter");
    }

    #[test]
    fn the_visible_range_is_the_rows_from_the_scroll_line_plus_one_for_the_fraction() {
        assert_eq!(visible_range(0.0, 10, 100), 0..11);
        assert_eq!(visible_range(95.5, 10, 100), 95..100, "clamped at the end");
        assert_eq!(visible_range(0.0, 10, 3), 0..3);
        assert_eq!(visible_range(0.0, 10, 0), 0..0);
    }
}
```

`viewer.rs`'s tests:

```rust
    #[test]
    fn a_changed_fingerprint_reloads_and_keeps_the_scroll_clamped() {
        // The pure part of a reload: a shorter file pulls the scroll back; the find re-runs.
        let mut content = Content::Text { lines: (0..100).map(|i| format!("line {i}")).collect(), spans: vec![Vec::new(); 100] };
        let mut scroll = ScrollAnimator::new();
        scroll.jump_to(80.0);
        let shorter = Content::Text { lines: (0..20).map(|i| format!("line {i}")).collect(), spans: vec![Vec::new(); 20] };
        content = shorter;
        let max = match &content {
            Content::Text { lines, .. } => lines.len().saturating_sub(1) as f32,
            _ => 0.0,
        };
        scroll.clamp_max(max);
        assert_eq!(scroll.current, 19.0);
        let hits = match &content {
            Content::Text { lines, .. } => find_hits(lines, "line 1"),
            _ => Vec::new(),
        };
        assert_eq!(hits.len(), 11, "line 1 and line 10 to 19");
    }
```

(This pins the arithmetic `Viewer::apply` performs; `apply` itself needs a gpui context. If `Viewer::apply` can be exercised with `gpui::TestAppContext`, prefer that; otherwise this stands.)

- [ ] **Step 2: Run them to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkos -- viewer_element a_changed_fingerprint`
Expected: FAIL to compile.

- [ ] **Step 3: Implement the element**

`viewer_element.rs`:

```rust
//! Paints a `Viewer`'s visible lines (spec 6: virtualized, line numbers in a gutter) at a
//! fractional line offset, like the terminal element. Only the lines in the viewport are shaped.
use crate::palette::Rgb;
use crate::viewer::{Content, Viewer};
use gpui::{
    fill, font, point, px, size, App, Bounds, ContentMask, Element, ElementId, Entity, FontWeight, GlobalElementId, Hsla, InspectorElementId,
    IntoElement, LayoutId, Pixels, Rgba, SharedString, Style, TextRun, Window,
};
use std::ops::Range;
use std::time::Instant;

pub const GUTTER_PAD: f32 = 8.0;
const LEFT_PAD: f32 = 8.0;

pub fn gutter_width(line_count: usize, cell_w: f32) -> f32 {
    let digits = line_count.max(1).to_string().len() as f32;
    digits * cell_w + GUTTER_PAD * 2.0
}

/// The lines to paint: from the scrolled line, `rows` of them plus one for the fraction,
/// clamped to the file.
pub fn visible_range(scroll: f32, rows: usize, len: usize) -> Range<usize> {
    let first = (scroll.floor().max(0.0) as usize).min(len);
    let last = (first + rows + 1).min(len);
    first..last
}

fn hsla(c: Rgb) -> Hsla {
    Rgba { r: c.r as f32 / 255.0, g: c.g as f32 / 255.0, b: c.b as f32 / 255.0, a: 1.0 }.into()
}

pub struct ViewerElement {
    viewer: Entity<Viewer>,
    family: SharedString,
    bg: Rgb,
    gutter_fg: Rgb,
    text_fg: Rgb,
    find_match: Rgb,
    find_current: Rgb,
}

impl ViewerElement {
    pub fn new(viewer: Entity<Viewer>, family: SharedString, bg: Rgb, gutter_fg: Rgb, text_fg: Rgb, find_match: Rgb, find_current: Rgb) -> Self {
        Self { viewer, family, bg, gutter_fg, text_fg, find_match, find_current }
    }
}

pub struct Prepaint {
    cell_w: f32,
    line_h: f32,
}

impl Element for ViewerElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = gpui::relative(1.0).into();
        style.size.height = gpui::relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _state: &mut (), window: &mut Window, cx: &mut App) -> Prepaint {
        let (font_size, line_height) = {
            let v = self.viewer.read(cx);
            (v.font_size, v.line_height)
        };
        let run = TextRun { len: 1, font: font(self.family.clone()), color: gpui::white(), background_color: None, underline: None, strikethrough: None };
        let cell_w = f32::from(window.text_system().layout_line("W", px(font_size), &[run], None).width);
        let line_h = font_size * line_height;
        let rows = (f32::from(bounds.size.height) / line_h).floor().max(1.0) as usize;
        self.viewer.update(cx, |v, _| v.rows_visible = rows);
        Prepaint { cell_w, line_h }
    }

    fn paint(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _state: &mut (), prepaint: &mut Prepaint, window: &mut Window, cx: &mut App) {
        let (cell_w, line_h) = (prepaint.cell_w, prepaint.line_h);
        window.paint_quad(fill(bounds, hsla(self.bg)));
        let (first, lines, spans, frac, animating, hits, current, font_size) = self.viewer.update(cx, |v, _| {
            let now = Instant::now();
            let dt = v.last_frame.map(|l| now.duration_since(l).as_secs_f32().min(0.05)).unwrap_or(1.0 / 60.0);
            let animating = v.scroll.tick(dt);
            v.last_frame = if animating { Some(now) } else { None };
            let (lines, spans) = match &v.content {
                Content::Text { lines, spans } => (lines.clone(), spans.clone()),
                _ => (Vec::new(), Vec::new()),
            };
            let range = visible_range(v.scroll.current, v.rows_visible, lines.len());
            let frac = v.scroll.current - v.scroll.current.floor();
            let (hits, current) = v.find.as_ref().map(|f| (f.hits.clone(), f.current)).unwrap_or_default();
            (range.start, lines[range.clone()].to_vec(), spans[range].to_vec(), frac, animating, hits, current, v.font_size)
        });
        if animating {
            window.request_animation_frame();
        }
        let gutter = gutter_width(first + lines.len(), cell_w);
        let origin = bounds.origin;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for (i, line) in lines.iter().enumerate() {
                let n = first + i;
                let y = origin.y + px((i as f32 - frac) * line_h);
                // The line number, right-aligned in the gutter.
                let num: SharedString = format!("{}", n + 1).into();
                let num_w = num.len() as f32 * cell_w;
                let num_run = TextRun { len: num.len(), font: font(self.family.clone()), color: hsla(self.gutter_fg), background_color: None, underline: None, strikethrough: None };
                let shaped = window.text_system().shape_line(num.clone(), px(font_size), &[num_run], None);
                let _ = shaped.paint(point(origin.x + px(gutter - GUTTER_PAD - num_w), y), px(line_h), window, cx);
                if line.is_empty() {
                    continue;
                }
                let runs: Vec<TextRun> = spans[i]
                    .iter()
                    .map(|s| {
                        let mut f = font(self.family.clone());
                        if s.bold {
                            f.weight = FontWeight::BOLD;
                        }
                        if s.italic {
                            f.style = gpui::FontStyle::Italic;
                        }
                        TextRun { len: s.end - s.start, font: f, color: hsla(s.color), background_color: None, underline: None, strikethrough: None }
                    })
                    .collect();
                let runs = if runs.is_empty() {
                    vec![TextRun { len: line.len(), font: font(self.family.clone()), color: hsla(self.text_fg), background_color: None, underline: None, strikethrough: None }]
                } else {
                    runs
                };
                let text: SharedString = line.clone().into();
                let x = origin.x + px(gutter + LEFT_PAD);
                // Find hits on this line: quads under the text, measured in cells of the shaped line.
                for h in hits.iter().filter(|h| h.line == n) {
                    let shaped = window.text_system().shape_line(text.clone(), px(font_size), &runs, None);
                    let x0 = shaped.x_for_index(h.start);
                    let x1 = shaped.x_for_index(h.end);
                    let is_current = hits.get(current).is_some_and(|c| c == h);
                    let c = if is_current { self.find_current } else { self.find_match };
                    let mut col = hsla(c);
                    col.a = 0.45;
                    window.paint_quad(fill(Bounds { origin: point(x + x0, y), size: size(x1 - x0, px(line_h)) }, col));
                }
                let shaped = window.text_system().shape_line(text, px(font_size), &runs, None);
                let _ = shaped.paint(point(x, y), px(line_h), window, cx);
            }
        });
    }
}

impl IntoElement for ViewerElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
```

(`ShapedLine::x_for_index` exists in gpui 0.2.2's `text_system/line.rs`; if the name differs, grep `fn x_for_index`. Tabs in source files: replace `\t` by four spaces in `split_lines` before highlighting, since the shaped line treats a tab as one glyph; add that to `split_lines` and its test: `split_lines("a\tb")` is `["a    b"]`.)

`viewer.rs` gains the bar's key handling and paste, mirroring `find.rs`:

```rust
    /// Keys typed into the find bar: Escape closes, Enter and Shift+Enter step, the rest edits
    /// the query and searches again.
    pub fn find_key(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) {
        match ks.key.as_str() {
            "escape" => self.close_find(cx),
            "enter" => self.find_step(if ks.modifiers.shift { -1 } else { 1 }, cx),
            _ => {
                let edited = match &mut self.find {
                    Some(f) => !matches!(f.query.handle(ks), crate::line_edit::Edit::None),
                    None => false,
                };
                if edited {
                    self.refind(cx);
                }
            }
        }
    }

    pub fn find_paste(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(f) = &mut self.find {
            f.query.insert_str(text);
        }
        self.refind(cx);
    }
```

(`Edit` is the enum `LineEdit::handle` returns; check its variant for "nothing changed" in `line_edit.rs` and use that name.)

`pane_view.rs`: `Some(Tab::Viewer { path })` renders `render_viewer(root, pane, tab_id, path, cx)`: gets or creates the entity through `root.viewer_for(tab)` (creating it starts the load), then

```rust
    div()
        .size_full()
        .relative()
        .key_context("Viewer")
        .track_focus(&focus)
        .on_key_down(cx.listener(move |_root, ev: &KeyDownEvent, _window, cx| {
            let ks = &ev.keystroke;
            v_keys.update(cx, |v, cx| {
                let page = v.rows_visible as f32;
                match ks.key.as_str() {
                    "down" => v.scroll_lines(1.0, cx),
                    "up" => v.scroll_lines(-1.0, cx),
                    "pagedown" | "space" => v.scroll_lines(page, cx),
                    "pageup" => v.scroll_lines(-page, cx),
                    "home" => { v.scroll.jump_to(0.0); cx.notify(); }
                    "end" => { let m = v.max_scroll(); v.scroll.jump_to(m); cx.notify(); }
                    "escape" => v.close_find(cx),
                    _ => {}
                }
            });
        }))
        .on_scroll_wheel(cx.listener(move |_root, ev: &ScrollWheelEvent, _window, cx| {
            v_wheel.update(cx, |v, cx| {
                let line_h = v.font_size * v.line_height;
                // The wheel's y is positive when the content should move down: scroll up.
                let lines = match ev.delta {
                    ScrollDelta::Lines(p) => -p.y,
                    ScrollDelta::Pixels(p) => -f32::from(p.y) / line_h,
                };
                v.scroll_lines(lines, cx);
            });
        }))
        .on_mouse_down(MouseButton::Left, cx.listener(move |root, _ev: &MouseDownEvent, _window, cx| root.focus_pane(pane, cx)))
        .child(match &content_kind {
            Kind::Text => ViewerElement::new(viewer.clone(), family, theme.chrome.window_bg, theme.chrome.text_dim, theme.chrome.text, theme.semantic.find_match, theme.semantic.find_current).into_any_element(),
            Kind::Loading => notice("loading"),
            Kind::TooBig(n) => notice(&format!("{} MB, over the viewer's highlight limit; shown plain", n / (1024 * 1024))),
            Kind::Binary => notice("a binary file; nothing to show"),
            Kind::Unreadable(e) => notice(&format!("cannot read this file: {e}")),
        })
        .children(viewer_find_bar(&viewer, pane, &theme, family2, cx))
```

where a too-big file is still shown plain (the load gives `Content::Text` with plain spans and the notice is a one-line banner above the text rather than instead of it: show the text, with a small dim line at the top saying "over the highlight limit, shown plain"), `notice(text)` is a centred dim line, and `viewer_find_bar` is `find::render` adapted for a `Viewer` (same chrome: the `LineEdit`, the count `"{} of {}"` or "no matches", a × to close; put it in `viewer.rs` or a new `viewer_find.rs` under 80 lines).

`main.rs`:
- `Root.viewers: HashMap<TabId, Entity<Viewer>>`.
- `viewer_for(tab, path, cx) -> Entity<Viewer>`: existing, or `cx.new(|cx| Viewer::new(path, self.font_size, self.line_height, cx))` inserted, then `load_viewer(tab, cx)`.
- `load_viewer(tab, cx)`: reads the viewer's path and the highlighter; on the background executor: `fs::read` → `classify(bytes, limit)` → `split_lines` → spans (highlighted when `Text` under the limit and the highlighter is ready, else plain) → `Loaded { content, fingerprint: fingerprint(&path) }`; then `viewer.update(cx, |v, cx| v.apply(loaded, cx))`. `TooBig` loads the text plain: read it as `String::from_utf8_lossy` and give it plain spans; keep a `plain: bool` on the viewer so the banner shows.
- `watched_paths` adds every open viewer's path; `paths_changed` calls `load_viewer` for the tabs whose path changed (a viewer whose file was deleted shows `Unreadable`).
- `close_tab`: when a `Tab::Viewer` closes, `self.viewers.remove(&id)`; `forget_pane` nothing extra (viewers are keyed by tab). `replace_tab` in `open_viewer` (`OpenPlan::Replace`) removes the old entity for that tab id and creates a new one.
- `run`: `Find` opens the active viewer's find when the active tab is a viewer (`active_viewer(cx)` mirrors `active_terminal`), else the terminal's as today. `FontDelta(d)` applies to the focused viewer too (`v.font_size = (v.font_size + d).clamp(8.0, 48.0)`). `apply_text_preset` sets every viewer's `font_size` and `line_height` as it does for terminals. `set_theme` rebuilds the highlighter and reloads every open viewer (one background job per viewer).
- `render`'s focus logic: when the active tab of the focused pane is a viewer, `wanted` is its `find` bar's focus when open, else the viewer's focus.

README: a **Sidebar and viewer** section before **Keyboard**:

```markdown
## Sidebar and viewer

Cmd+B shows the sidebar: a file tree rooted at the folder, read one directory at a time as you
expand it, with `.git` hidden and anything `.gitignore` ignores left out (the palette's "Show
ignored files" lists them dimmed). It follows the file system within a second. A single click
opens a file in a preview tab in the focused pane, which the next single click replaces; a
double click, Enter in the tree, or dragging the tab pins it. Right click: Reveal in Finder,
Copy path, Copy relative path, Open in default app. Drag a file onto a terminal to paste its
path. Shift+Cmd+E puts the keyboard in the tree: arrows move and expand, Enter opens, Space
previews, Escape goes back to the pane.

The viewer is read-only: syntax colours from your theme's `[viewer]` section through syntect's
TextMate grammars, line numbers, smooth scrolling, Cmd+F to find, and a reload within a second
of the file changing on disk. Files over `[viewer] highlight_limit_bytes` (5 MB by default) open
plain; binaries show a notice. The text preset applies to viewers as to terminals, and Cmd+= and
Cmd+- resize the focused one. Git status colours, the worktree explorer and the diff gutters
come in milestone 4b.
```

The checklist (top of Status) gains: "- Cmd+B, expand `src`, click a file: a dimmed preview tab with colours and line numbers; click another file: the same tab shows it; double click: a second tab; edit the file in another app: the viewer follows within a second", "- expand `node_modules` or `target` in a big project: the window stays responsive", "- a PNG and a 50 MB log from the tree: a notice, and plain text, with the window responsive", "- Shift+Cmd+E, Down, Right, Down, Enter: a file opens from the keyboard", "- right click a file, Copy relative path, paste into a terminal". The Status paragraph's "Milestone 4 (the sidebar) is next" becomes "Milestone 4b (worktrees, git status in the tree, diff gutters) is next." The Config section lists `viewer`.

- [ ] **Step 4: Run everything**

Run: `~/.cargo/bin/cargo test --workspace`, `~/.cargo/bin/cargo build --release 2>&1 | grep -i "^warning"`, `grep -n "—" README.md crates/sarkos/src/*.rs`
Expected: all green; no project warnings; no em-dashes. By hand if a screen is available: the README checklist's new rows, in order.

- [ ] **Step 5: Commit**

```bash
git add crates/sarkos/src/viewer_element.rs crates/sarkos/src/viewer.rs crates/sarkos/src/pane_view.rs crates/sarkos/src/main.rs README.md
git commit -m "Show files in a viewer tab: highlighted, numbered, scrolling, with find and live reload"
```

---

## Self-review notes

- Spec 6, file tree paragraph: rooted at the folder (Task 2), lazy (Task 2), `.gitignore` with a toggle and `.git` hidden (Task 2), watcher (Task 2's poll, ruling 1), right click actions (Task 3), drag onto a terminal (Task 3), preview and pin (Task 3). Git status colours and folder dots: milestone 4b (ruling 3).
- Spec 6, viewer paragraph: read-only, loaded once, syntect, virtualized, line numbers (Tasks 4, 5); diff gutters and the inline diff: 4b; re-render on change (Task 5 through the poll); Cmd+F (Task 5); the size limit and binaries (Task 4).
- Spec 4: the sidebar keys (Task 1; Ctrl+Shift+G with 4b), persistence of sidebar width and collapsed state (Task 1), viewer tabs in the tree model (already there).
- Spec 7: tree and viewer colours used as named (Tasks 3, 4, 5); text presets apply to viewers (Task 5).
- Type consistency: `Row` fields are the same in Tasks 1, 2 and 3; `open_plan` and `OpenPlan` in Task 3 match `open_viewer`; `Content`, `Span`, `Hit` in Task 4 match the element in Task 5; `command_palette::items` gains exactly one parameter (`show_ignored`) in Task 1 and no other task changes its signature; `Sidebar.show_ignored` is replaced by `Explorer.show_ignored` in Task 2 (Task 1's field goes with it).
- Review Focus pins: 1 and 4 in Task 2, 5 in Task 3, 3 in Task 4, 2 in Task 5.
