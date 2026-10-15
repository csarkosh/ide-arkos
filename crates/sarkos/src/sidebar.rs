//! The left sidebar (spec 6): a fixed column, Cmd+B to toggle, resizable by its right edge,
//! holding the Explorer section (and Worktrees in milestone 4b). Its state is saved with the
//! workspace; the config's `[sidebar]` gives a new folder its defaults.

use crate::explorer;
use crate::gitstatus::Status;
use crate::keymap;
use crate::palette::Rgb;
use crate::theme::Theme;
use crate::sidebar;
use crate::tree::{PaneId, Tab, TabId, Tree};
use crate::Root;
use gpui::{div, prelude::*, px, uniform_list, App, ClickEvent, Context, Div, FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent, UniformList, UniformListScrollHandle};
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub const MIN_W: f32 = 160.0;
pub const MAX_W: f32 = 600.0;
/// The resize handle at the column's right edge, with its invisible hit area.
pub const HANDLE_W: f32 = 4.0;
pub const HANDLE_HIT_W: f32 = 10.0;
pub const ROW_H: f32 = 24.0;
/// How far each tree level is indented; the Explorer's rows read it.
pub const INDENT: f32 = 14.0;

pub struct Sidebar {
    pub width: f32,
    pub collapsed: bool,
    pub focus: FocusHandle,
    pub scroll: UniformListScrollHandle,
}

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
    pub fn new(width: f32, collapsed: bool, cx: &mut Context<Root>) -> Self {
        Sidebar { width: clamp_width(width), collapsed, focus: cx.focus_handle(), scroll: UniformListScrollHandle::new() }
    }

    pub fn visible_width(&self) -> f32 {
        visible_width(self.width, self.collapsed)
    }
}

/// The column: a header per section, the Explorer rows, and the resize handle. `None` while
/// collapsed.
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
            .on_key_down(cx.listener(|root, ev: &KeyDownEvent, _window, cx| root.tree_key(&ev.keystroke, cx)))
            .child(header("Explorer"))
            .child(crate::sidebar::render_rows(root, &theme, cx))
            .child(handle),
    )
}

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

pub fn run_menu_action(action: &MenuAction, cx: &mut App) {
    match action {
        MenuAction::Reveal(p) => cx.reveal_path(p),
        MenuAction::CopyText(s) => cx.write_to_clipboard(gpui::ClipboardItem::new_string(s.clone())),
        MenuAction::OpenDefault(p) => cx.open_with_system(p),
    }
}

/// A row's element id: stable across renders for the same path, so hover and clicks stay put.
fn hash_of(path: &Path) -> u64 {
    crate::paths::fnv1a(path.as_os_str().as_encoded_bytes())
}

/// A row's text colour: an ignored entry stays ignored (one inside an untracked directory
/// included), else its git status (a conflict reads as deleted), else plain text.
pub fn row_color(status: Option<Status>, ignored: bool, theme: &Theme) -> Rgb {
    let t = &theme.tree;
    if ignored {
        return t.ignored;
    }
    match status {
        Some(Status::Added) => t.added,
        Some(Status::Modified) => t.modified,
        Some(Status::Deleted | Status::Conflicted) => t.deleted,
        Some(Status::Untracked) => t.untracked,
        None => theme.chrome.text,
    }
}

/// The Explorer's rows: a virtualized list of the visible entries, only the rows on screen
/// built each frame. A click on a directory toggles it; on a file it previews (a double click
/// pins); a right click opens the menu; a file drags out to a terminal.
pub fn render_rows(root: &mut Root, theme: &Theme, cx: &mut Context<Root>) -> UniformList {
    let rows = Rc::new(root.explorer.rows());
    let selected = root.explorer.selected.clone();
    let weak = cx.weak_entity();
    let family = root.family();
    let (selected_bg, hover_bg, dim, dot) = (theme.chrome.tab_active.hsla(), theme.chrome.tab_inactive.hsla(), theme.chrome.text_dim.hsla(), theme.tree.modified.hsla());
    let theme = theme.clone();
    let (drag_bg, drag_fg) = (theme.chrome.banner, theme.chrome.text);
    let count = rows.len();
    uniform_list("explorer-rows", count, move |range, _window, _cx| {
        rows[range]
            .iter()
            .map(|row| {
                let path = row.path.clone();
                let is_selected = selected.as_deref() == Some(path.as_path());
                let chevron = if !row.is_dir {
                    " "
                } else if row.loading {
                    "\u{2026}"
                } else if row.expanded {
                    "\u{25be}"
                } else {
                    "\u{25b8}"
                };
                let (w_click, w_menu) = (weak.clone(), weak.clone());
                let p_click = path.clone();
                let p_menu = path.clone();
                let is_dir = row.is_dir;
                div()
                    .id(("row", hash_of(&path)))
                    .w_full()
                    .h(px(ROW_H))
                    .pl(px(10.0 + row.depth as f32 * INDENT))
                    .pr(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .when(is_selected, |d| d.bg(selected_bg))
                    .hover(|s| s.bg(hover_bg))
                    .text_color(row_color(row.status, row.ignored, &theme).hsla())
                    .cursor_pointer()
                    .on_click(move |ev: &ClickEvent, _window, cx| {
                        let _ = w_click.update(cx, |root, cx| {
                            if is_dir {
                                root.explorer.toggle(&p_click);
                                root.explorer.selected = Some(p_click.clone());
                                root.load_pending_dirs(cx);
                                root.save(cx);
                                cx.notify();
                            } else {
                                root.open_viewer(&p_click, ev.click_count() >= 2, cx);
                            }
                        });
                    })
                    .on_mouse_down(MouseButton::Right, move |ev: &MouseDownEvent, _window, cx| {
                        // The root's mouse-down closes an open menu; this one just opened.
                        cx.stop_propagation();
                        let _ = w_menu.update(cx, |root, cx| {
                            root.context_menu = Some(ContextMenu { path: p_menu.clone(), at: ev.position });
                            cx.notify();
                        });
                    })
                    .when(!is_dir, |d| {
                        let value = crate::drag::FileDrag { path: path.clone(), position: Default::default(), bg: drag_bg, fg: drag_fg, family: family.clone() };
                        d.on_drag(value, |d: &crate::drag::FileDrag, position, _window, cx| cx.new(|_| d.clone().at(position)))
                    })
                    .child(div().flex_none().w(px(12.0)).text_color(dim).child(chevron))
                    .child(div().min_w_0().truncate().child(row.name.clone()))
                    .when(row.dirty, |d| d.child(div().flex_none().size(px(6.0)).rounded_full().bg(dot)))
            })
            .collect::<Vec<_>>()
    })
    .flex_1()
    .min_h_0()
    .track_scroll(root.sidebar.scroll.clone())
}

/// The right-click menu, drawn over everything at the click. A mouse-down anywhere else closes
/// it (the root's listener); its own rows stop the event so their click lands.
pub fn render_context_menu(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let menu = root.context_menu.as_ref()?;
    let c = &root.theme.chrome;
    let mut m = div()
        .absolute()
        .left(menu.at.x)
        .top(menu.at.y)
        .min_w(px(200.0))
        .flex()
        .flex_col()
        .py(px(4.0))
        .bg(c.banner.hsla())
        .border_1()
        .border_color(c.border.hsla())
        .font_family(root.family())
        .text_size(px(12.0))
        .text_color(c.text.hsla())
        // Hover, the cursor and the wheel under the menu stop reacting while it is over them.
        .occlude()
        .on_any_mouse_down(|_: &MouseDownEvent, _window, cx| cx.stop_propagation());
    let hover = c.tab_active.hsla();
    for (i, (label, action)) in context_menu_items(&menu.path, &root.project).into_iter().enumerate() {
        m = m.child(
            div()
                .id(("context-menu-row", i))
                .px(px(10.0))
                .py(px(4.0))
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .child(label)
                .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                    run_menu_action(&action, cx);
                    root.context_menu = None;
                    cx.notify();
                })),
        );
    }
    Some(m)
}

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

/// The preview tab after a tab closed: none if the preview was the one closed.
pub fn after_close(preview_tab: Option<TabId>, closed: TabId) -> Option<TabId> {
    preview_tab.filter(|p| *p != closed)
}

/// The preview tab after a tab was dragged: a dragged tab is a deliberate placement, so a
/// dragged preview stops being one.
pub fn after_drop(preview_tab: Option<TabId>, dragged: TabId) -> Option<TabId> {
    preview_tab.filter(|p| *p != dragged)
}

/// Whether a command leaves the keys in the file tree (Shift+Cmd+E). Every other command acts
/// on the focused pane, so the keys go back to it.
pub fn keeps_tree_focus(cmd: &keymap::Command) -> bool {
    use keymap::Command::*;
    matches!(cmd, ToggleSidebar | FocusFileTree | ShowIgnoredFiles | CommandPalette)
}

impl Root {
    /// Reads every expanded directory that is not loaded yet, off the UI thread, and applies
    /// each result as it lands.
    pub fn load_pending_dirs(&mut self, cx: &mut Context<Self>) {
        for dir in self.explorer.dirs_to_load() {
            self.explorer.mark_loading(&dir);
            let root = self.project.clone();
            let d = dir.clone();
            cx.spawn(async move |this, cx| {
                let entries = cx.background_executor().spawn(async move { explorer::read_dir_entries(&root, &d) }).await;
                let _ = this.update(cx, |root, cx| {
                    match entries {
                        Some(entries) => root.explorer.set_loaded(&dir, entries),
                        None => root.explorer.set_unreadable(&dir),
                    }
                    // A change the poll saw while this read was in flight is read again now.
                    root.load_pending_dirs(cx);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// Runs `git status` for the project on the background executor, one run at a time: asked
    /// while one is in flight, it runs once more when that one lands. A result older than a
    /// newer run is dropped (a backstop: runs do not overlap).
    pub fn refresh_status(&mut self, cx: &mut Context<Self>) {
        if self.status_runs.request() == crate::explorer_watch::StatusAction::Defer {
            return;
        }
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
                if root.status_runs.landed() {
                    root.refresh_status(cx);
                }
            });
        })
        .detach();
    }

    /// Runs `git worktree list` on the background executor. A result older than a newer run
    /// is dropped.
    pub fn refresh_worktrees(&mut self, cx: &mut Context<Self>) {
        self.worktrees_seq += 1;
        let seq = self.worktrees_seq;
        let root = self.project.clone();
        cx.spawn(async move |this, cx| {
            let list = cx.background_executor().spawn(async move { crate::worktrees::list(&root) }).await;
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

    /// Runs `git status` for each expanded worktree whose changes are not loaded, off the UI
    /// thread, and applies each result as it lands.
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

    /// Opens `path` in the focused pane as a preview (single click) or pinned (double click,
    /// Enter). The tab's viewer is made and loaded when the pane first shows it.
    pub fn open_viewer(&mut self, path: &Path, pin: bool, cx: &mut Context<Self>) {
        self.sidebar_focused = false;
        let pane = self.tree.focused;
        let plan = open_plan(&self.tree, pane, path, self.preview_tab, pin);
        let idx = match plan {
            OpenPlan::Activate(idx) => {
                if pin && self.tree.tab_id(pane, idx) == self.preview_tab {
                    self.preview_tab = None;
                }
                idx
            }
            OpenPlan::Replace(idx) => {
                // Same tab id, another file: the old viewer goes, the render makes the new one.
                if let Some(id) = self.tree.tab_id(pane, idx) {
                    self.viewers.remove(&id);
                }
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

    /// Escape while the tree's right-click menu is open closes the menu and nothing else.
    pub(crate) fn escape_closes_menu(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        if ks.key != "escape" || self.context_menu.is_none() {
            return false;
        }
        self.context_menu = None;
        cx.notify();
        true
    }

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
        // Keep the selection on screen when the keys move it past the column's edge.
        let rows = self.explorer.rows();
        if let Some(i) = self.explorer.selected.as_ref().and_then(|s| rows.iter().position(|r| &r.path == s)) {
            // Top for a row above the first one fully shown, Bottom otherwise: either scrolls
            // only as far as the row (a row already in view does not move the list).
            let top = -f32::from(self.sidebar.scroll.0.borrow().base_handle.offset().y);
            let strategy = if (i as f32) * sidebar::ROW_H < top + sidebar::ROW_H { gpui::ScrollStrategy::Top } else { gpui::ScrollStrategy::Bottom };
            self.sidebar.scroll.scroll_to_item(i, strategy);
        }
        self.save(cx);
        cx.notify();
    }

    /// What the poll fingerprints: the expanded directories (and the root), and the open
    /// viewers' files.
    pub fn watched_paths(&self, cx: &App) -> Vec<PathBuf> {
        let mut v = vec![self.project.clone()];
        v.extend(self.explorer.expanded_dirs());
        v.extend(self.viewers.values().map(|viewer| viewer.read(cx).path.clone()));
        v
    }

    /// A changed directory is read again, a changed viewer file reloaded.
    pub fn paths_changed(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        for p in paths {
            if !self.reload_viewers_for(p, cx) {
                self.explorer.invalidate(p);
            }
        }
        self.load_pending_dirs(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{Side, Tree};
    use protocol::SessionId;

    #[test]
    fn the_width_stays_between_the_limits() {
        assert_eq!(clamp_width(10.0), MIN_W);
        assert_eq!(clamp_width(260.0), 260.0);
        assert_eq!(clamp_width(9000.0), MAX_W);
        assert_eq!(clamp_width(f32::NAN), MIN_W, "a NaN from a bad file is the minimum, not a panic");
    }

    #[test]
    fn the_context_menu_lists_the_four_file_actions_in_order() {
        let items = context_menu_items(Path::new("/p/src/a.rs"), Path::new("/p"));
        let labels: Vec<&str> = items.iter().map(|(l, _)| *l).collect();
        assert_eq!(labels, ["Reveal in Finder", "Copy path", "Copy relative path", "Open in default app"]);
        assert_eq!(items[1].1, MenuAction::CopyText("/p/src/a.rs".into()));
        assert_eq!(items[2].1, MenuAction::CopyText("src/a.rs".into()));
        assert_eq!(context_menu_items(Path::new("/elsewhere/f"), Path::new("/p"))[2].1, MenuAction::CopyText("/elsewhere/f".into()), "outside the project the full path");
    }

    #[test]
    fn a_row_is_coloured_ignored_first_then_by_its_git_status_then_plain() {
        use crate::gitstatus::Status;
        let theme = crate::theme::builtin("navy").unwrap();
        let t = &theme.tree;
        assert_eq!(row_color(Some(Status::Added), false, &theme), t.added);
        assert_eq!(row_color(Some(Status::Modified), false, &theme), t.modified);
        assert_eq!(row_color(Some(Status::Deleted), false, &theme), t.deleted);
        assert_eq!(row_color(Some(Status::Untracked), false, &theme), t.untracked);
        assert_eq!(row_color(Some(Status::Conflicted), false, &theme), t.deleted, "a conflict reads as deleted");
        assert_eq!(row_color(None, true, &theme), t.ignored);
        assert_eq!(row_color(Some(Status::Untracked), true, &theme), t.ignored, "an ignored file inside an untracked directory is ignored");
        assert_eq!(row_color(None, false, &theme), theme.chrome.text);
    }

    #[test]
    fn a_collapsed_sidebar_takes_no_width() {
        assert_eq!(visible_width(300.0, false), 300.0);
        assert_eq!(visible_width(300.0, true), 0.0);
    }

    #[test]
    fn closing_or_dragging_the_preview_tab_ends_the_preview_and_no_other_tab_does() {
        assert_eq!(after_close(Some(3), 3), None, "the preview closed");
        assert_eq!(after_close(Some(3), 4), Some(3), "another tab closed");
        assert_eq!(after_close(None, 4), None);
        assert_eq!(after_drop(Some(3), 3), None, "a dragged preview is placed on purpose: pinned");
        assert_eq!(after_drop(Some(3), 4), Some(3));
        assert_eq!(after_drop(None, 3), None);
    }

    #[test]
    fn only_the_sidebar_commands_and_the_palette_keep_the_keys_in_the_tree() {
        use crate::keymap::Command;
        for cmd in [Command::ToggleSidebar, Command::FocusFileTree, Command::ShowIgnoredFiles, Command::CommandPalette] {
            assert!(keeps_tree_focus(&cmd), "{cmd:?}");
        }
        for cmd in [Command::Find, Command::NewTerminal, Command::FontDelta(1.0)] {
            assert!(!keeps_tree_focus(&cmd), "{cmd:?}: the keys go back to the focused pane");
        }
    }

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
}
