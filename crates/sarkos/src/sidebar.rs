//! The left sidebar (spec 6): a fixed column, Cmd+B to toggle, resizable by its right edge,
//! holding two collapsible sections, the Explorer and Worktrees. Its state is saved with the
//! workspace; the config's `[sidebar]` gives a new folder its defaults.

use crate::explorer;
use crate::gitstatus::Status;
use crate::keymap;
use crate::line_edit::{self, Edit, LineEdit};
use crate::palette::Rgb;
use crate::theme::Theme;
use crate::sidebar;
use crate::tree::{PaneId, Tab, TabId, Tree};
use crate::worktrees::{Worktree, WtRow};
use crate::Root;
use gpui::{div, prelude::*, px, uniform_list, App, ClickEvent, Context, Div, FocusHandle, KeyDownEvent, Keystroke, MouseButton, MouseDownEvent, ScrollHandle, Stateful, UniformList, UniformListScrollHandle, Window};
use protocol::SessionId;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

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
    /// Whether each section shows its rows; the headers always show.
    pub explorer_open: bool,
    pub worktrees_open: bool,
    /// The Worktrees section's own scroll: few rows, plain divs.
    pub wt_scroll: ScrollHandle,
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
    pub fn new(width: f32, collapsed: bool, explorer_open: bool, worktrees_open: bool, cx: &mut Context<Root>) -> Self {
        Sidebar { width: clamp_width(width), collapsed, focus: cx.focus_handle(), scroll: UniformListScrollHandle::new(), explorer_open, worktrees_open, wt_scroll: ScrollHandle::new() }
    }

    pub fn visible_width(&self) -> f32 {
        visible_width(self.width, self.collapsed)
    }
}

/// The column: the Explorer and Worktrees sections, each under a header that opens and closes
/// it, and the resize handle. `None` while collapsed.
pub fn render(root: &mut Root, window: &mut Window, cx: &mut Context<Root>) -> Option<Div> {
    if root.sidebar.collapsed {
        return None;
    }
    let theme = root.theme.clone();
    let c = &theme.chrome;
    let width = root.sidebar.width;
    let family = root.family();
    let header = |id: &'static str, title: &str, open: bool| -> Stateful<Div> {
        div()
            .id(id)
            .h(px(ROW_H))
            .flex_none()
            .px(px(10.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .text_color(c.text_dim.hsla())
            .text_size(px(11.0))
            .cursor_pointer()
            .child(div().flex_none().w(px(12.0)).child(if open { "\u{25be}" } else { "\u{25b8}" }))
            .child(div().flex_1().child(title.to_uppercase()))
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
    let explorer_open = root.sidebar.explorer_open;
    let explorer = div()
        .flex()
        .flex_col()
        .min_h_0()
        .when(explorer_open, |d| d.flex_1())
        .key_context("Explorer")
        .track_focus(&root.sidebar.focus)
        .on_key_down(cx.listener(|root, ev: &KeyDownEvent, _window, cx| root.tree_key(&ev.keystroke, cx)))
        .child(header("explorer-header", "Explorer", explorer_open).on_click(cx.listener(|root, _ev: &ClickEvent, _window, cx| {
            root.sidebar.explorer_open = !root.sidebar.explorer_open;
            if !root.sidebar.explorer_open {
                root.sidebar_focused = false;
            }
            root.save(cx);
            cx.notify();
        })))
        .when(explorer_open, |d| d.child(render_rows(root, &theme, cx)));
    let worktrees_open = root.sidebar.worktrees_open;
    let plus = div()
        .id("worktree-create")
        .flex_none()
        .px(px(4.0))
        .text_size(px(13.0))
        .cursor_pointer()
        .hover(|s| s.text_color(c.text.hsla()))
        .child("+")
        .on_click(cx.listener(|root, _ev: &ClickEvent, _window, cx| {
            // The header under it would close the section on the same click.
            cx.stop_propagation();
            root.sidebar.worktrees_open = true;
            // A prompt already open keeps what was typed; it just takes the keys.
            if root.wt_prompt.is_none() {
                root.wt_prompt = Some(Prompt::new());
                root.read_prompt_drivers(cx);
            }
            root.wt_focused = true;
            root.sidebar_focused = false;
            root.save(cx);
            cx.notify();
        }));
    let worktrees = div()
        .flex_none()
        .flex()
        .flex_col()
        .border_t_1()
        .border_color(c.border.hsla())
        .key_context("Worktrees")
        .track_focus(&root.wt_focus)
        .on_key_down(cx.listener(|root, ev: &KeyDownEvent, _window, cx| root.wt_key(&ev.keystroke, cx)))
        .child(
            header("worktrees-header", "Worktrees", worktrees_open)
                .on_click(cx.listener(|root, _ev: &ClickEvent, _window, cx| {
                    root.sidebar.worktrees_open = !root.sidebar.worktrees_open;
                    if !root.sidebar.worktrees_open {
                        // A closed section cannot hold the keys or a prompt.
                        root.wt_focused = false;
                        root.wt_prompt = None;
                    }
                    root.save(cx);
                    cx.notify();
                }))
                .child(plus),
        )
        .when(worktrees_open, |d| d.children(render_prompt(root, &theme, cx)).child(render_worktree_rows(root, window, &theme, cx)));
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
            .child(explorer)
            .child(worktrees)
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
                            root.wt_menu = None;
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

/// Whether a command leaves the keys in the sidebar (Shift+Cmd+E, Ctrl+Shift+G). Every other
/// command acts on the focused pane, so the keys go back to it.
pub fn keeps_tree_focus(cmd: &keymap::Command) -> bool {
    use keymap::Command::*;
    matches!(cmd, ToggleSidebar | FocusFileTree | FocusWorktrees | ShowIgnoredFiles | CommandPalette)
}

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

/// The keyboard's row after the rows changed: still on a row, or none when there are none.
pub fn clamp_selection(selected: Option<usize>, rows: usize) -> Option<usize> {
    selected.filter(|_| rows > 0).map(|i| i.min(rows - 1))
}

/// How many terminal tabs were opened in `dir` or a folder inside it.
pub fn terminals_inside(tree: &Tree, dir: &Path) -> usize {
    tree.tabs().into_iter().filter(|(_, _, tab)| matches!(tab, Tab::Terminal { cwd: Some(c), .. } if c.starts_with(dir))).count()
}

/// The notice after a removal that left terminals open in the removed worktree.
pub fn removed_notice(terminals: usize) -> String {
    if terminals == 1 {
        "worktree removed; 1 terminal was open in it".to_string()
    } else {
        format!("worktree removed; {terminals} terminals were open in it")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WtMenuAction {
    OpenTerminal(PathBuf),
    Remove(PathBuf),
    RemoveForce(PathBuf),
}

/// The right-click menu of a worktree row. The main checkout is never removed; a dirty or
/// unloaded worktree offers force as well, since the plain removal will be refused. One with
/// no files (missing, or a bare main checkout) has nothing to open a terminal in, and only
/// force removes it.
pub fn worktree_menu_items(wt: &Worktree, dirty: Option<usize>) -> Vec<(&'static str, WtMenuAction)> {
    let mut v = Vec::new();
    if wt.has_files() {
        v.push(("Open terminal here", WtMenuAction::OpenTerminal(wt.path.clone())));
    }
    if wt.is_main {
        return v;
    }
    if wt.has_files() {
        v.push(("Remove worktree", WtMenuAction::Remove(wt.path.clone())));
    }
    if !wt.has_files() || dirty.map_or(true, |n| n > 0) {
        v.push(("Remove worktree, force", WtMenuAction::RemoveForce(wt.path.clone())));
    }
    v
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Branch,
    Path,
    /// The repository configures filter drivers, which the checkout runs: named, and a second
    /// Enter creates.
    Confirm,
}

/// The repository's filter drivers as the create prompt knows them. Only `Known` creates.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Drivers {
    /// The read has not landed.
    #[default]
    Unknown,
    /// The drivers the repository's own config sets; empty when none.
    Known(Vec<String>),
    /// The config could not be read, or names a driver `read_filter_args` refuses.
    Refused,
}

impl Drivers {
    /// From `explorer::filter_drivers`.
    pub fn from_read(read: Option<Vec<String>>) -> Self {
        read.map_or(Drivers::Refused, Drivers::Known)
    }
}

pub const CHECKING_FILTERS: &str = "checking the repository's filter config\u{2026}";
pub const REFUSED_FILTERS: &str = "cannot create: unusual filter config";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptStep {
    Stay,
    Cancel,
    Create { branch: String, path: PathBuf },
}

/// The two-step prompt under the Worktrees header: a branch name, then the path (prefilled),
/// and, when the repository configures filter drivers, a confirmation naming them. Its focus
/// handle is `Root.wt_prompt_focus`.
pub struct Prompt {
    pub branch: LineEdit,
    pub path: LineEdit,
    pub stage: Stage,
    pub error: Option<String>,
    /// `git worktree add` is running: every key but Escape waits for it.
    pub creating: bool,
    /// The filter drivers the confirmation names.
    pub drivers: Vec<String>,
    /// Enter on the path came before the drivers were read: said, and nothing created.
    pub checking: bool,
}

impl Prompt {
    pub fn new() -> Self {
        Prompt { branch: LineEdit::new(), path: LineEdit::new(), stage: Stage::Branch, error: None, creating: false, drivers: Vec::new(), checking: false }
    }

    /// A key, with `drivers` the repository's filter drivers as known now. Fails closed: Enter on
    /// the path creates only with `Drivers::Known`, at once when empty, after a confirmation
    /// naming them otherwise; while the read is out it says so (Enter again once it lands, in
    /// milliseconds), and a refused config never creates.
    pub fn key(&mut self, ks: &Keystroke, root: &Path, drivers: &Drivers) -> PromptStep {
        if self.creating {
            // Escape closes the prompt; the running git finishes on its own and the list's
            // refresh shows what it did.
            return if ks.key == "escape" { PromptStep::Cancel } else { PromptStep::Stay };
        }
        self.checking = false;
        let edit = match self.stage {
            Stage::Branch => &mut self.branch,
            Stage::Path => &mut self.path,
            Stage::Confirm => {
                return match ks.key.as_str() {
                    "escape" => PromptStep::Cancel,
                    "enter" if !ks.modifiers.modified() => {
                        self.creating = true;
                        PromptStep::Create { branch: self.branch.text.trim().to_string(), path: PathBuf::from(self.path.text.trim()) }
                    }
                    _ => PromptStep::Stay,
                };
            }
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
                    let path = PathBuf::from(p);
                    self.error = None;
                    match drivers {
                        Drivers::Unknown => {
                            self.checking = true;
                            PromptStep::Stay
                        }
                        Drivers::Refused => {
                            self.error = Some(REFUSED_FILTERS.into());
                            PromptStep::Stay
                        }
                        Drivers::Known(names) if !names.is_empty() => {
                            self.drivers = names.clone();
                            self.stage = Stage::Confirm;
                            PromptStep::Stay
                        }
                        Drivers::Known(_) => {
                            self.creating = true;
                            PromptStep::Create { branch: self.branch.text.trim().to_string(), path }
                        }
                    }
                }
                // Its keys were answered above.
                Stage::Confirm => PromptStep::Stay,
            },
            _ => PromptStep::Stay,
        }
    }

    /// Pasted text goes into the field being edited, newlines and carriage returns dropped;
    /// nothing while git creates or the confirmation is up.
    pub fn paste(&mut self, text: &str) {
        match self.stage {
            _ if self.creating => {}
            Stage::Branch => self.branch.insert_str(text),
            Stage::Path => self.path.insert_str(text),
            Stage::Confirm => {}
        }
    }

    /// The dim line under the fields: the filter config is being read, or the confirmation
    /// naming the drivers, until git starts.
    pub fn note(&self) -> Option<String> {
        if self.creating {
            None
        } else if self.checking {
            Some(CHECKING_FILTERS.to_string())
        } else {
            (self.stage == Stage::Confirm).then(|| format!("creating runs this repository's filter driver(s): {}. Enter again to continue", self.drivers.join(", ")))
        }
    }
}

/// Where a failed `git worktree add` is said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateFailure {
    /// Under the prompt that is waiting for it, which stays open.
    InPrompt,
    /// In a notice: the prompt was closed (or closed and opened afresh) while git ran.
    Notice,
}

pub fn create_failure_target(prompt_open_and_creating: bool) -> CreateFailure {
    if prompt_open_and_creating {
        CreateFailure::InPrompt
    } else {
        CreateFailure::Notice
    }
}

/// The create prompt under the Worktrees header: the branch, then the path, and git's or the
/// prompt's own complaint under them.
fn render_prompt(root: &mut Root, theme: &Theme, cx: &mut Context<Root>) -> Option<Div> {
    let p = root.wt_prompt.as_ref()?;
    let family = root.family();
    let label = |text: &'static str| div().flex_none().w(px(44.0)).text_color(theme.chrome.text_dim.hsla()).child(text);
    let field = |edit: &LineEdit, placeholder: &str, active: bool| -> Div {
        if active {
            line_edit::render(edit, placeholder, theme, family.clone(), 12.0)
        } else {
            div().text_size(px(12.0)).text_color(theme.chrome.text_dim.hsla()).child(edit.text.clone())
        }
    };
    let row = |l: Div, f: Div| div().h(px(ROW_H)).px(px(10.0)).flex().items_center().gap(px(6.0)).child(l).child(div().min_w_0().flex_1().overflow_hidden().child(f));
    Some(
        div()
            .flex_none()
            .flex()
            .flex_col()
            .text_size(px(12.0))
            .bg(theme.chrome.tab_inactive.hsla())
            // Keys reach `wt_key` through the section's listener; a click here takes them back.
            .track_focus(&root.wt_prompt_focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|root, _ev: &MouseDownEvent, _window, cx| {
                    root.wt_focused = true;
                    root.sidebar_focused = false;
                    cx.notify();
                }),
            )
            .child(row(label("branch"), field(&p.branch, "feature/name", p.stage == Stage::Branch)))
            .when(p.stage != Stage::Branch, |d| d.child(row(label("path"), field(&p.path, "", p.stage == Stage::Path))))
            .when_some(p.note(), |d, t| d.child(div().px(px(10.0)).pb(px(4.0)).text_color(theme.chrome.text_dim.hsla()).child(t)))
            .when(p.creating, |d| d.child(div().px(px(10.0)).pb(px(4.0)).text_color(theme.chrome.text_dim.hsla()).child("creating\u{2026}")))
            .when_some(p.error.clone().filter(|_| !p.creating), |d, e| d.child(div().px(px(10.0)).pb(px(4.0)).text_color(theme.tree.deleted.hsla()).child(e))),
    )
}

/// The Worktrees rows: each worktree (a shimmer while its first `git status` runs) and, once
/// expanded and loaded, its changed files easing in. Requests the next frame only while
/// something moves and Reduce Motion is off.
fn render_worktree_rows(root: &mut Root, window: &mut Window, theme: &Theme, cx: &mut Context<Root>) -> Stateful<Div> {
    let rows = root.worktrees.rows();
    root.worktrees.selected = clamp_selection(root.worktrees.selected, rows.len());
    let selected = root.worktrees.selected;
    let still = crate::motion::reduce_motion();
    let now = Instant::now();
    let width = root.sidebar.width;
    let c = &theme.chrome;
    let (selected_bg, hover_bg, dim) = (c.tab_active.hsla(), c.tab_inactive.hsla(), c.text_dim.hsla());
    let mut moving = false;
    let mut list = div()
        .id("worktree-rows")
        .flex()
        .flex_col()
        .max_h(px(root.content.h * 0.4))
        .overflow_y_scroll()
        .track_scroll(&root.sidebar.wt_scroll);
    if root.worktrees.not_git {
        // A repository whose filter config turned git status off says that instead.
        let why = root.explorer.status_off.unwrap_or("not a git repository");
        list = list.child(div().h(px(ROW_H)).px(px(10.0)).flex().items_center().text_color(dim).child(why));
    }
    for (i, row) in rows.into_iter().enumerate() {
        let base = div()
            .id(("worktree-row", i))
            .relative()
            .overflow_hidden()
            .w_full()
            .h(px(ROW_H))
            .pr(px(8.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .when(selected == Some(i), |d| d.bg(selected_bg))
            .hover(|s| s.bg(hover_bg))
            .cursor_pointer();
        let el = match row {
            WtRow::Worktree { wt, expanded, loading, dirty } => {
                let band = loading.then(|| {
                    let phase = if still {
                        0.5
                    } else {
                        moving = true;
                        shimmer_phase(root.worktrees.loading_since.get(&wt.path).map_or(Duration::ZERO, |t| now.saturating_duration_since(*t)))
                    };
                    div().absolute().top_0().h_full().w(px(0.3 * width)).left(px(phase * 1.3 * width - 0.3 * width)).bg(c.tab_inactive.alpha(0.6))
                });
                let chevron = if !wt.has_files() {
                    ""
                } else if expanded {
                    "\u{25be}"
                } else {
                    "\u{25b8}"
                };
                let name = wt.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| wt.path.to_string_lossy().into_owned());
                let branch = match (&wt.branch, wt.bare) {
                    (Some(b), _) => b.clone(),
                    (None, true) => "bare".to_string(),
                    (None, false) => format!("detached {}", wt.head),
                };
                let count = dirty.map(|n| match n {
                    0 => ("clean".to_string(), dim),
                    1 => ("1 change".to_string(), theme.tree.modified.hsla()),
                    n => (format!("{n} changes"), theme.tree.modified.hsla()),
                });
                let (p_click, p_menu) = (wt.path.clone(), wt.path.clone());
                let has_menu = !worktree_menu_items(&wt, dirty).is_empty();
                base.pl(px(10.0))
                    .children(band)
                    .child(div().flex_none().w(px(12.0)).text_color(dim).child(chevron))
                    .child(div().flex_none().max_w(px(width * 0.5)).truncate().child(name))
                    .child(div().min_w_0().flex_1().truncate().text_color(dim).child(branch))
                    .when(wt.missing, |d| d.child(div().flex_none().text_color(theme.tree.deleted.hsla()).child("missing")))
                    .children(count.map(|(text, color)| div().flex_none().text_size(px(11.0)).text_color(color).child(text)))
                    .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                        root.worktrees.selected = Some(i);
                        root.worktrees.toggle(&p_click);
                        root.load_pending_worktrees(cx);
                        root.save(cx);
                        cx.notify();
                    }))
                    .when(has_menu, |d| {
                        d.on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |root, ev: &MouseDownEvent, _window, cx| {
                                // The root's mouse-down closes an open menu; this one just opened.
                                cx.stop_propagation();
                                root.context_menu = None;
                                root.wt_menu = Some((p_menu.clone(), ev.position));
                                cx.notify();
                            }),
                        )
                    })
            }
            WtRow::Change { worktree, rel, status } => {
                let opacity = if still {
                    1.0
                } else {
                    let o = root.worktrees.opened_at.get(&worktree).map_or(1.0, |t| ease_in(now.saturating_duration_since(*t)));
                    moving |= o < 1.0;
                    o
                };
                let path = worktree.join(&rel);
                base.pl(px(10.0 + 2.0 * INDENT))
                    .opacity(opacity)
                    .text_color(row_color(Some(status), false, theme).hsla())
                    .child(div().min_w_0().truncate().child(rel.to_string_lossy().into_owned()))
                    .on_click(cx.listener(move |root, ev: &ClickEvent, _window, cx| {
                        root.worktrees.selected = Some(i);
                        root.open_viewer(&path, ev.click_count() >= 2, cx);
                    }))
            }
        };
        list = list.child(el);
    }
    if moving {
        window.request_animation_frame();
    }
    list
}

/// The worktree row's right-click menu, drawn like the file tree's.
pub fn render_worktree_menu(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let (path, at) = root.wt_menu.clone()?;
    let items = root.worktrees.find(&path).map(|wt| worktree_menu_items(wt, root.worktrees.dirty_count(&path))).unwrap_or_default();
    if items.is_empty() {
        // The worktree went away (or has nothing to offer): the menu goes too.
        root.wt_menu = None;
        return None;
    }
    let c = &root.theme.chrome;
    let hover = c.tab_active.hsla();
    let mut m = div()
        .absolute()
        .left(at.x)
        .top(at.y)
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
        .occlude()
        .on_any_mouse_down(|_: &MouseDownEvent, _window, cx| cx.stop_propagation());
    for (i, (label, action)) in items.into_iter().enumerate() {
        m = m.child(
            div()
                .id(("worktree-menu-row", i))
                .px(px(10.0))
                .py(px(4.0))
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .child(label)
                .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                    root.wt_menu = None;
                    match action.clone() {
                        WtMenuAction::OpenTerminal(p) => root.open_terminal_in(p, cx),
                        WtMenuAction::Remove(p) => root.remove_worktree(p, false, cx),
                        WtMenuAction::RemoveForce(p) => root.remove_worktree(p, true, cx),
                    }
                    cx.notify();
                })),
        );
    }
    Some(m)
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
            let entries = cx.background_executor().spawn(async move { crate::gitstatus::read_status(&root) }).await;
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
                    if root.wt_menu.as_ref().is_some_and(|m| root.worktrees.find(&m.0).is_none()) {
                        root.wt_menu = None;
                    }
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
        self.wt_focused = false;
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

    /// Escape while a right-click menu (the tree's or a worktree's) is open closes the menu and
    /// nothing else.
    pub(crate) fn escape_closes_menu(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        if ks.key != "escape" || (self.context_menu.is_none() && self.wt_menu.is_none()) {
            return false;
        }
        self.context_menu = None;
        self.wt_menu = None;
        cx.notify();
        true
    }

    /// A new terminal tab in the focused pane whose shell starts in `dir` (a worktree). The tab
    /// remembers the folder, so a restart reattaches the session.
    pub fn open_terminal_in(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let pane = self.tree.focused;
        let idx = self.tree.add_tab(pane, Tab::Terminal { session: SessionId(0), cwd: Some(dir.clone()) });
        if let Some(id) = self.tree.tab_id(pane, idx) {
            self.sessions.create_in(id, false, dir);
        }
        // The keys go to the new shell.
        self.sidebar_focused = false;
        self.wt_focused = false;
        self.save(cx);
        cx.notify();
    }

    /// Reads the repository's filter drivers for the create prompt, off the UI thread.
    /// `Drivers::Unknown` until the read lands.
    pub fn read_prompt_drivers(&mut self, cx: &mut Context<Self>) {
        self.wt_prompt_drivers = Drivers::Unknown;
        let root = self.project.clone();
        cx.spawn(async move |this, cx| {
            let read = cx.background_executor().spawn(async move { explorer::filter_drivers(&root) }).await;
            let _ = this.update(cx, |root, cx| {
                root.wt_prompt_drivers = Drivers::from_read(read);
                cx.notify();
            });
        })
        .detach();
    }

    /// `git worktree add` off the UI thread. Success closes the prompt; a failure shows git's
    /// words in it and keeps it open, or in a notice when the prompt was closed meanwhile. The
    /// list is read again either way.
    pub fn create_worktree(&mut self, branch: String, path: PathBuf, cx: &mut Context<Self>) {
        let root = self.project.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { crate::worktrees::create(&root, &branch, &path) }).await;
            let _ = this.update(cx, |root, cx| {
                match result {
                    Ok(()) => root.wt_prompt = None,
                    Err(e) => match create_failure_target(root.wt_prompt.as_ref().is_some_and(|p| p.creating)) {
                        CreateFailure::InPrompt => {
                            if let Some(p) = &mut root.wt_prompt {
                                p.error = Some(e);
                                p.creating = false;
                                // Back to the path, which the error is most likely about.
                                if p.stage == Stage::Confirm {
                                    p.stage = Stage::Path;
                                }
                            }
                        }
                        CreateFailure::Notice => root.show_notice(format!("cannot create the worktree: {e}"), cx),
                    },
                }
                root.refresh_worktrees(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// `git worktree remove` off the UI thread; git's refusal (changes, the main checkout) is a
    /// notice, and so are terminals left open in a removed one (they keep running).
    pub fn remove_worktree(&mut self, path: PathBuf, force: bool, cx: &mut Context<Self>) {
        let root = self.project.clone();
        let removed = path.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { crate::worktrees::remove(&root, &path, force) }).await;
            let _ = this.update(cx, |root, cx| {
                match result {
                    Err(e) => root.show_notice(format!("cannot remove the worktree: {e}"), cx),
                    Ok(()) => match terminals_inside(&root.tree, &removed) {
                        0 => {}
                        n => root.show_notice(removed_notice(n), cx),
                    },
                }
                root.refresh_worktrees(cx);
            });
        })
        .detach();
    }

    /// Keys in the Worktrees section: Up/Down move, Enter or Right expands a worktree or opens a
    /// change pinned, Space previews it, Left collapses, Escape returns to the pane; with the
    /// prompt open, the keys go to it (Cmd+V pastes).
    pub fn wt_key(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) {
        let drivers = self.wt_prompt_drivers.clone();
        if let Some(p) = &mut self.wt_prompt {
            if ks.modifiers.platform && ks.key == "v" {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    p.paste(&text);
                }
                cx.notify();
                return;
            }
            match p.key(ks, &self.project.clone(), &drivers) {
                PromptStep::Stay => {}
                PromptStep::Cancel => {
                    self.wt_prompt = None;
                    self.wt_focused = false;
                }
                PromptStep::Create { branch, path } => self.create_worktree(branch, path, cx),
            }
            cx.notify();
            return;
        }
        if ks.modifiers.platform || ks.modifiers.control || ks.modifiers.alt {
            return;
        }
        let rows = self.worktrees.rows();
        let sel = clamp_selection(self.worktrees.selected, rows.len());
        match ks.key.as_str() {
            "down" if !rows.is_empty() => self.worktrees.selected = Some(sel.map_or(0, |i| (i + 1).min(rows.len() - 1))),
            "up" if !rows.is_empty() => self.worktrees.selected = Some(sel.map_or(0, |i| i.saturating_sub(1))),
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
        if let Some(i) = self.worktrees.selected {
            self.sidebar.wt_scroll.scroll_to_item(i);
        }
        cx.notify();
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
        let main = Worktree { path: "/p".into(), head: "1".into(), branch: Some("main".into()), is_main: true, missing: false, bare: false };
        let feat = Worktree { path: "/p-f".into(), head: "2".into(), branch: Some("f".into()), is_main: false, missing: false, bare: false };
        let labels = |items: Vec<(&'static str, WtMenuAction)>| items.into_iter().map(|(l, _)| l).collect::<Vec<_>>();
        assert_eq!(labels(worktree_menu_items(&main, Some(0))), ["Open terminal here"]);
        assert_eq!(labels(worktree_menu_items(&feat, Some(0))), ["Open terminal here", "Remove worktree"]);
        assert_eq!(labels(worktree_menu_items(&feat, Some(3))), ["Open terminal here", "Remove worktree", "Remove worktree, force"], "dirty: the plain remove will be refused, so force is offered");
        assert_eq!(labels(worktree_menu_items(&feat, None)), ["Open terminal here", "Remove worktree", "Remove worktree, force"], "not loaded: cannot tell, offer both");
        let gone = Worktree { missing: true, ..feat.clone() };
        assert_eq!(labels(worktree_menu_items(&gone, None)), ["Remove worktree, force"], "nothing to open; only force removes a missing one");
        let bare = Worktree { bare: true, branch: None, head: String::new(), ..main.clone() };
        assert!(worktree_menu_items(&bare, None).is_empty(), "a bare main checkout: no files to open a terminal in, and never removed");
        assert_eq!(worktree_menu_items(&feat, Some(0))[0].1, WtMenuAction::OpenTerminal("/p-f".into()));
        assert_eq!(worktree_menu_items(&feat, None)[2].1, WtMenuAction::RemoveForce("/p-f".into()));
    }

    #[test]
    fn the_create_prompt_asks_for_the_branch_then_the_path() {
        use gpui::Keystroke;
        let key = |k: &str| Keystroke::parse(k).unwrap();
        let root = std::path::Path::new("/Users/me/proj");
        let none = Drivers::Known(vec![]);
        let mut p = Prompt::new();
        assert_eq!(p.stage, Stage::Branch);
        assert_eq!(p.key(&key("enter"), root, &none), PromptStep::Stay, "an empty branch does not advance");
        assert!(p.error.is_some());
        // "c->c" is gpui's test syntax for a key that types its character.
        for c in "feat/x".chars() {
            p.key(&key(&format!("{c}->{c}")), root, &none);
        }
        assert_eq!(p.branch.text, "feat/x");
        assert_eq!(p.key(&key("enter"), root, &none), PromptStep::Stay);
        assert_eq!(p.stage, Stage::Path);
        assert!(p.error.is_none(), "a good branch clears the error");
        assert_eq!(p.path.text, "/Users/me/proj-feat-x", "the default path is filled in");
        assert_eq!(p.key(&key("enter"), root, &none), PromptStep::Create { branch: "feat/x".into(), path: "/Users/me/proj-feat-x".into() });
        assert!(p.creating);
        assert_eq!(p.key(&key("enter"), root, &none), PromptStep::Stay, "a second Enter while git works runs nothing");
        assert_eq!(p.key(&key("z->z"), root, &none), PromptStep::Stay);
        assert_eq!(p.path.text, "/Users/me/proj-feat-x", "and types nothing");
        assert!(p.creating);
        assert_eq!(p.key(&key("escape"), root, &none), PromptStep::Cancel, "Escape still closes it");
        let mut q = Prompt::new();
        assert_eq!(q.key(&key("escape"), root, &none), PromptStep::Cancel);
        for c in "a b".chars() {
            q.key(&key(&format!("{c}->{c}")), root, &none);
        }
        assert_eq!(q.key(&key("enter"), root, &none), PromptStep::Stay, "a space is not a branch name");
        assert_eq!((q.stage, q.error.is_some()), (Stage::Branch, true));
    }

    #[test]
    fn with_filter_drivers_configured_the_prompt_names_them_and_a_second_enter_creates() {
        use gpui::Keystroke;
        let key = |k: &str| Keystroke::parse(k).unwrap();
        let root = std::path::Path::new("/Users/me/proj");
        let lfs = Drivers::Known(vec!["lfs".to_string()]);
        let to_path = |p: &mut Prompt| {
            for c in "feat".chars() {
                p.key(&key(&format!("{c}->{c}")), root, &Drivers::Unknown);
            }
            assert_eq!(p.key(&key("enter"), root, &Drivers::Unknown), PromptStep::Stay);
            assert_eq!(p.stage, Stage::Path);
            assert_eq!(p.note(), None);
        };
        let mut p = Prompt::new();
        to_path(&mut p);
        assert_eq!(p.key(&key("enter"), root, &lfs), PromptStep::Stay, "the first Enter on the path names the drivers");
        assert_eq!(p.stage, Stage::Confirm);
        assert!(!p.creating);
        assert_eq!(p.note().as_deref(), Some("creating runs this repository's filter driver(s): lfs. Enter again to continue"));
        assert_eq!(p.key(&key("z->z"), root, &lfs), PromptStep::Stay, "a key is not a confirmation");
        assert_eq!(p.path.text, "/Users/me/proj-feat", "and types nothing");
        assert_eq!(p.key(&key("enter"), root, &lfs), PromptStep::Create { branch: "feat".into(), path: "/Users/me/proj-feat".into() });
        assert!(p.creating);
        assert_eq!(p.note(), None, "git is running: the line gives way to \"creating\"");
        // Escape at the confirmation cancels.
        let mut q = Prompt::new();
        to_path(&mut q);
        q.key(&key("enter"), root, &Drivers::Known(vec!["lfs".to_string(), "x".to_string()]));
        assert_eq!(q.note().as_deref(), Some("creating runs this repository's filter driver(s): lfs, x. Enter again to continue"));
        assert_eq!(q.key(&key("escape"), root, &lfs), PromptStep::Cancel);
        // No drivers: one Enter creates, as before.
        let mut r = Prompt::new();
        to_path(&mut r);
        assert_eq!(r.key(&key("enter"), root, &Drivers::Known(vec![])), PromptStep::Create { branch: "feat".into(), path: "/Users/me/proj-feat".into() });
    }

    #[test]
    fn the_prompt_creates_nothing_until_the_filter_config_is_known_and_never_when_refused() {
        use gpui::Keystroke;
        let key = |k: &str| Keystroke::parse(k).unwrap();
        let root = std::path::Path::new("/Users/me/proj");
        let mut p = Prompt::new();
        for c in "feat".chars() {
            p.key(&key(&format!("{c}->{c}")), root, &Drivers::Unknown);
        }
        p.key(&key("enter"), root, &Drivers::Unknown);
        // The read has not landed: said, and nothing created.
        assert_eq!(p.key(&key("enter"), root, &Drivers::Unknown), PromptStep::Stay);
        assert_eq!((p.stage, p.creating), (Stage::Path, false));
        assert_eq!(p.note().as_deref(), Some(CHECKING_FILTERS));
        assert_eq!(p.key(&key("enter"), root, &Drivers::Unknown), PromptStep::Stay, "still not known: still nothing");
        // It landed, with none: the next Enter creates.
        assert_eq!(p.key(&key("enter"), root, &Drivers::Known(vec![])), PromptStep::Create { branch: "feat".into(), path: "/Users/me/proj-feat".into() });
        // Refused: never creates, however often Enter comes.
        let mut q = Prompt::new();
        for c in "feat".chars() {
            q.key(&key(&format!("{c}->{c}")), root, &Drivers::Refused);
        }
        q.key(&key("enter"), root, &Drivers::Refused);
        for _ in 0..3 {
            assert_eq!(q.key(&key("enter"), root, &Drivers::Refused), PromptStep::Stay);
            assert_eq!(q.error.as_deref(), Some(REFUSED_FILTERS));
            assert!(!q.creating);
            assert_ne!(q.stage, Stage::Confirm);
        }
        assert_eq!(q.note(), None);
        assert_eq!(Drivers::from_read(None), Drivers::Refused, "an unreadable or unusual config");
        assert_eq!(Drivers::from_read(Some(vec![])), Drivers::Known(vec![]));
    }

    #[test]
    fn a_paste_goes_into_the_field_being_edited_on_one_line() {
        use gpui::Keystroke;
        let root = std::path::Path::new("/Users/me/proj");
        let mut p = Prompt::new();
        p.paste("feat/\r\nx\n");
        assert_eq!((p.branch.text.as_str(), p.branch.cursor), ("feat/x", 6));
        p.key(&Keystroke::parse("enter").unwrap(), root, &Drivers::Known(vec!["lfs".to_string()]));
        p.path.clear();
        p.paste("/tmp/wt\n");
        assert_eq!(p.path.text, "/tmp/wt");
        assert_eq!(p.branch.text, "feat/x", "the path stage pastes into the path");
        p.key(&Keystroke::parse("enter").unwrap(), root, &Drivers::Known(vec!["lfs".to_string()]));
        assert_eq!(p.stage, Stage::Confirm);
        p.paste("more");
        assert_eq!(p.path.text, "/tmp/wt", "nothing is pasted at the confirmation");
    }

    #[test]
    fn a_failed_create_shows_in_the_waiting_prompt_or_else_in_a_notice() {
        assert_eq!(create_failure_target(true), CreateFailure::InPrompt);
        assert_eq!(create_failure_target(false), CreateFailure::Notice, "closed, or reopened afresh, while git ran: not silent");
    }

    #[test]
    fn terminals_inside_counts_the_tabs_opened_in_a_worktree() {
        use crate::tree::Tab;
        let mut tree = Tree::new(Tab::Terminal { session: SessionId(1), cwd: None });
        let pane = tree.focused;
        tree.add_tab(pane, Tab::Terminal { session: SessionId(2), cwd: Some("/p-f".into()) });
        tree.add_tab(pane, Tab::Terminal { session: SessionId(3), cwd: Some("/p-f/sub".into()) });
        tree.add_tab(pane, Tab::Terminal { session: SessionId(4), cwd: Some("/p-feat".into()) });
        tree.add_tab(pane, Tab::Viewer { path: "/p-f/a.rs".into() });
        assert_eq!(terminals_inside(&tree, Path::new("/p-f")), 2, "by path components: /p-feat is not inside /p-f, a viewer is no terminal");
        assert_eq!(terminals_inside(&tree, Path::new("/elsewhere")), 0);
        assert_eq!(removed_notice(1), "worktree removed; 1 terminal was open in it");
        assert_eq!(removed_notice(3), "worktree removed; 3 terminals were open in it");
    }

    #[test]
    fn the_worktrees_selection_stays_on_a_row() {
        assert_eq!(clamp_selection(Some(5), 3), Some(2), "rows went away under it");
        assert_eq!(clamp_selection(Some(1), 3), Some(1));
        assert_eq!(clamp_selection(Some(0), 0), None, "no rows, no selection");
        assert_eq!(clamp_selection(None, 3), None);
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
        for cmd in [Command::ToggleSidebar, Command::FocusFileTree, Command::FocusWorktrees, Command::ShowIgnoredFiles, Command::CommandPalette] {
            assert!(keeps_tree_focus(&cmd), "{cmd:?}");
        }
        for cmd in [Command::Find, Command::NewTerminal, Command::FontDelta(1.0)] {
            assert!(!keeps_tree_focus(&cmd), "{cmd:?}: the keys go back to the focused pane");
        }
    }

    #[test]
    fn a_preview_is_replaced_and_a_pin_is_kept() {
        use crate::tree::Tab;
        let mut tree = Tree::new(Tab::Terminal { session: SessionId(1), cwd: None });
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
        let other = tree.split(pane, Side::Right, Tab::Terminal { session: SessionId(2), cwd: None }).unwrap();
        assert_eq!(open_plan(&tree, other, Path::new("/p/b.rs"), Some(a_id), false), OpenPlan::Add, "the preview is in another pane");
        // An already open pinned tab for the file: activate, whatever the mode.
        assert_eq!(open_plan(&tree, pane, Path::new("/p/a.rs"), None, true), OpenPlan::Activate(a));
    }
}
