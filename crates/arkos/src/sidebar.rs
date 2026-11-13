//! The left sidebar (spec 6): a fixed column, Cmd+B to toggle, resizable by its right edge,
//! holding two collapsible sections, the Explorer and Worktrees. Its state is saved with the
//! workspace; the config's `[sidebar]` gives a new folder its defaults.

use crate::effects::blocks::{self, HighlightState, Kind, Mark};
use crate::explorer;
use crate::gitstatus::Status;
use crate::keymap;
use crate::motion::{self, Transient};
use crate::line_edit::{self, Edit, LineEdit};
use crate::palette::Rgb;
use crate::theme::Theme;
use crate::sidebar;
use crate::tree::{PaneId, Rect, Tab, TabId, Tree, ViewMode};
use crate::worktrees::{wt_row_label, Worktree, WtRow};
use crate::wt_scan::wt_row_place;
use crate::Root;
use gpui::{div, prelude::*, px, uniform_list, App, ClickEvent, Context, Div, FocusHandle, KeyDownEvent, Keystroke, MouseButton, MouseDownEvent, ScrollHandle, Stateful, UniformList, UniformListScrollHandle, Window};
use protocol::SessionId;
use crate::video::Fp;
use std::collections::{HashMap, HashSet};
use futures::StreamExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

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
    /// Cmd+B's slide while it runs: the width on screen eases to the new `visible_width`.
    pub slide: Option<motion::Slide>,
    /// Each section's rows fade in as it opens (they go at once as it closes, since a closing
    /// list would still take clicks).
    pub explorer_fade: Transient<()>,
    pub worktrees_fade: Transient<()>,
    /// The Sessions section (`sessions_list`), above the Explorer: whether it shows its rows,
    /// and their fade as it opens.
    pub sessions_open: bool,
    pub sessions_fade: Transient<()>,
    /// In the Inbox layout, whether the column shows the files (the Explorer and Worktrees) in
    /// place of the session list. Not saved: the Inbox opens on its sessions.
    pub inbox_files: bool,
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
        // Sections open at launch are already there: no fade as the window first draws.
        let now = Instant::now();
        let fade = |open: bool| if open { Transient::shown((), now) } else { Transient::Hidden };
        Sidebar {
            width: clamp_width(width),
            collapsed,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            explorer_open,
            worktrees_open,
            wt_scroll: ScrollHandle::new(),
            slide: None,
            explorer_fade: fade(explorer_open),
            worktrees_fade: fade(worktrees_open),
            sessions_open: true,
            sessions_fade: fade(true),
            inbox_files: false,
        }
    }

    /// The same sidebar with its Sessions section as it was saved.
    pub fn with_sessions(mut self, open: bool) -> Self {
        self.sessions_open = open;
        if !open {
            self.sessions_fade = Transient::Hidden;
        }
        self
    }

    pub fn visible_width(&self) -> f32 {
        visible_width(self.width, self.collapsed)
    }

    /// Collapses or expands the column (Cmd+B), sliding from the width on screen now, so a
    /// slide turned around half way goes back from where it is.
    pub fn set_collapsed(&mut self, collapsed: bool, now: Instant) {
        if collapsed == self.collapsed {
            return;
        }
        let from = self.shown_width(now);
        self.collapsed = collapsed;
        self.slide = Some(motion::Slide::new(from, self.visible_width(), now));
    }

    /// The width on screen at `now`: easing while a slide runs, else `visible_width`.
    pub fn shown_width(&self, now: Instant) -> f32 {
        shown_width(self.slide, self.visible_width(), now)
    }

    pub fn sliding(&self, now: Instant) -> bool {
        self.slide.is_some_and(|s| s.animating(now))
    }

    /// Whether anything in the column eases at `now`.
    pub fn animating(&self, now: Instant) -> bool {
        self.sliding(now) || self.explorer_fade.animating(now) || self.worktrees_fade.animating(now) || self.sessions_fade.animating(now)
    }
}

/// The column's width on screen: the slide's while it runs, else the settled width (which a
/// drag of the edge may have changed since the slide began).
pub fn shown_width(slide: Option<motion::Slide>, settled: f32, now: Instant) -> f32 {
    match slide {
        Some(s) if s.animating(now) => s.at(now),
        _ => settled,
    }
}

/// Shows a section's rows with a fade as it opens; a closed section's rows go at once.
fn reveal(fade: &mut Transient<()>, open: bool, now: Instant) -> f32 {
    if open {
        fade.set(Some(()), now);
    } else {
        fade.clear();
    }
    fade.current(now).map_or(0.0, |(_, o)| o)
}

/// Heights for the Explorer and the Worktrees sections in a sidebar `total` tall, both headers
/// included, from the Explorer's content height (its rows, without its header): open Worktrees
/// take what the Explorer does not need, never less than a third of `total`; the Explorer never
/// less than its header plus one row. With Worktrees closed its header sits right under the
/// Explorer's last row, not at the sidebar's foot: the Explorer is as tall as its rows, up to
/// all but the header's row, and what is left below the header is empty. On a sidebar shorter
/// than `2 * ROW_H` the Explorer keeps its header and a row and Worktrees gets what is left,
/// which may be nothing.
pub fn sidebar_split(total: f32, explorer_rows_h: f32, explorer_open: bool, worktrees_open: bool) -> (f32, f32) {
    match (explorer_open, worktrees_open) {
        (true, true) => {
            let explorer = (ROW_H + explorer_rows_h).min(total - total / 3.0).max(2.0 * ROW_H);
            (explorer, total - explorer)
        }
        (true, false) => ((ROW_H + explorer_rows_h).max(2.0 * ROW_H).min((total - ROW_H).max(2.0 * ROW_H)), ROW_H),
        (false, true) => (ROW_H, total - ROW_H),
        (false, false) => (ROW_H, ROW_H),
    }
}

/// The Sessions section's height in a sidebar `total` tall, its header included: closed, or
/// with no session to list, the header alone; open, its rows up to two fifths of the sidebar,
/// past which they scroll, so the Explorer under it always keeps the larger part.
pub fn sessions_height(total: f32, rows_h: f32, open: bool) -> f32 {
    if !open || rows_h <= 0.0 {
        return ROW_H;
    }
    (ROW_H + rows_h).min((total * 0.4).max(ROW_H))
}

/// The column: the Sessions, Explorer and Worktrees sections, each under a header that opens and closes
/// it, and the resize handle. `None` while collapsed, once its slide out has finished.
pub fn render(root: &mut Root, window: &mut Window, cx: &mut Context<Root>) -> Option<Div> {
    let style = root.effect_style();
    let now = root.frame_now;
    let sliding = root.sidebar.sliding(now);
    // A hidden sidebar reports nothing stale.
    root.effect_sidebar_end = None;
    if root.sidebar.collapsed && !sliding {
        return None;
    }
    let explorer_o = reveal(&mut root.sidebar.explorer_fade, root.sidebar.explorer_open, now);
    let sessions_o = reveal(&mut root.sidebar.sessions_fade, root.sidebar.sessions_open, now);
    let worktrees_o = reveal(&mut root.sidebar.worktrees_fade, root.sidebar.worktrees_open, now);
    let theme = root.theme.clone();
    let c = &theme.chrome;
    let width = root.sidebar.width;
    let family = root.ui_family();
    let header = |id: &'static str, title: &str, open: bool| -> Stateful<Div> {
        div()
            .id(id)
            .h(px(ROW_H))
            .flex_none()
            .pl(px(10.0 + style.sidebar_pad))
            .pr(px(10.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .text_color(c.text_dim.hsla())
            .text_size(px(style.ui(11.0)))
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
    let worktrees_open = root.sidebar.worktrees_open;
    // The New File prompt is a row of the tree while it is open or fading out.
    let entries = root.explorer.rows();
    let tree_rows = entries.len() + usize::from(root.new_file.current(root.frame_now).is_some());
    // The sidebar's own height: the panes' area and the gap an effect keeps above and below it.
    let total_h = root.content.h + 2.0 * style.gap;
    // The Inbox layout: the column is the session list, or the files, chosen by a switch at
    // its top. The Grid: a Sessions section over the Explorer and Worktrees.
    let inbox = root.is_inbox();
    let files_view = !inbox || root.sidebar.inbox_files;
    let switch_h = if inbox { ROW_H + 6.0 } else { 0.0 };
    let switch = inbox.then(|| {
        let files = root.sidebar.inbox_files;
        let (on, off, hover) = (c.tab_active.hsla(), c.tab_inactive.alpha(0.0), c.tab_inactive.hsla());
        let pill = |id: &'static str, label: &'static str, current: bool| {
            div()
                .id(id)
                .flex_none()
                .px(px(10.0))
                .py(px(2.0))
                .rounded(px(999.0))
                .text_size(px(style.ui(12.0)))
                .text_color(if current { c.text.hsla() } else { c.text_dim.hsla() })
                .bg(if current { on } else { off })
                .when(!current, |d| d.cursor_pointer().hover(move |s| s.bg(hover)))
                .child(label)
        };
        div()
            .h(px(switch_h))
            .flex_none()
            .pl(px(8.0 + style.sidebar_pad))
            .pr(px(8.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .child(pill("inbox-sessions", "Sessions", !files).on_click(cx.listener(|root, _ev: &ClickEvent, _window, cx| {
                root.sidebar.inbox_files = false;
                root.sidebar_focused = false;
                root.wt_focused = false;
                cx.notify();
            })))
            .child(pill("inbox-files", "Files", files).on_click(cx.listener(|root, _ev: &ClickEvent, _window, cx| {
                root.sidebar.inbox_files = true;
                cx.notify();
            })))
            .child(div().flex_1())
            .child(pill("inbox-new", "+ New", false).on_click(cx.listener(|root, _ev: &ClickEvent, _window, cx| {
                // A new terminal joins the sessions pane, wherever the focus is.
                let pane = root.tree.inbox_panes().map_or(root.tree.focused, |(sessions, _)| sessions);
                root.new_terminal_tab(pane, cx);
            })))
    });
    if inbox && !files_view {
        // No Worktrees section to say where it starts.
        root.effect_divider = None;
    }
    // The sessions first: what they take is taken from the two sections under them.
    let sessions_open = inbox || root.sidebar.sessions_open;
    let session_groups = crate::sessions_list::grouped(crate::sessions_list::entries(root, crate::sessions::now_ms()));
    let calling = crate::sessions_list::calling(&session_groups);
    let sessions_h = match (inbox, files_view) {
        (true, true) => 0.0,
        (true, false) => (total_h - switch_h).max(0.0),
        (false, _) => sessions_height(total_h, crate::sessions_list::content_height(&session_groups, root.sessions_done_open), sessions_open),
    };
    let sessions = div()
        .flex()
        .flex_col()
        .flex_none()
        .h(px(sessions_h))
        .min_h_0()
        .when(!inbox, |d| d.border_b_1().border_color(if style.ink_borders { c.border.alpha(0.0) } else { c.border.hsla() }))
        .when(inbox && !files_view, |d| d.child(crate::sessions_list::render_filter(root, cx)))
        .when(inbox && session_groups.is_empty(), |d| d.child(div().px(px(12.0 + style.sidebar_pad)).py(px(8.0)).text_size(px(style.ui(12.0))).text_color(c.text_dim.hsla()).child("No sessions. \u{201c}+ New\u{201d} starts one.")))
        .when(!inbox, |d| d.child(
            header("sessions-header", "Sessions", sessions_open)
                // How many call for the user, said on the header so a closed section still tells.
                .when(calling > 0, |d| d.child(div().flex_none().text_color(theme.semantic.waiting.hsla()).child(calling.to_string())))
                .on_click(cx.listener(|root, _ev: &ClickEvent, _window, cx| {
                    root.sidebar.sessions_open = !root.sidebar.sessions_open;
                    root.save(cx);
                    cx.notify();
                })),
        ))
        .when(sessions_open && !session_groups.is_empty() && sessions_h > 0.0, |d| d.child(crate::sessions_list::render(root, width, cx).opacity(sessions_o)))
        // In the Inbox the list is the column: its names may reach the foot, and say so to an
        // effect that draws there.
        .when(inbox && !files_view, |d| d.child(sidebar_end_marker(cx)));
    let (explorer_h, worktrees_h) = sidebar_split(total_h - sessions_h - switch_h, tree_rows as f32 * ROW_H, explorer_open, worktrees_open);
    let explorer = div()
        .flex()
        .flex_col()
        .flex_none()
        .h(px(explorer_h))
        .min_h_0()
        .key_context("Explorer")
        .track_focus(&root.sidebar.focus)
        .on_key_down(cx.listener(|root, ev: &KeyDownEvent, _window, cx| root.tree_key(&ev.keystroke, cx)))
        // A right press on the section's empty space (the rows stop theirs) is the project
        // folder's menu: New File at the top of the tree.
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|root, ev: &MouseDownEvent, _window, cx| {
                cx.stop_propagation();
                let now = Instant::now();
                root.wt_menu.hide(now);
                root.session_menu.hide(now);
                root.link_menu.hide(now);
                root.md_menu.hide(now);
                root.view_menu.hide(now);
                root.context_menu.show(ContextMenu { path: root.project.clone(), is_dir: true, at: ev.position }, now);
                cx.notify();
            }),
        )
        .child(header("explorer-header", "Explorer", explorer_open).on_click(cx.listener(|root, _ev: &ClickEvent, _window, cx| {
            root.sidebar.explorer_open = !root.sidebar.explorer_open;
            if !root.sidebar.explorer_open {
                // A closed section cannot hold the keys or the prompt.
                root.sidebar_focused = false;
                root.new_file.hide(Instant::now());
            }
            root.save(cx);
            cx.notify();
        })))
        .when(explorer_open, |d| d.child(render_rows(root, entries, &theme, cx).opacity(explorer_o)));
    let worktrees = div()
        .flex_none()
        .h(px(worktrees_h))
        .min_h_0()
        .flex()
        .flex_col()
        .border_t_1()
        // An effect that inks the borders draws the divider; the section says where it starts.
        .border_color(if style.ink_borders { c.border.alpha(0.0) } else { c.border.hsla() })
        .child({
            let w_divider = cx.weak_entity();
            gpui::canvas(
                move |bounds, _window, cx| {
                    let _ = w_divider.update(cx, |root, _| root.effect_divider = Some(f32::from(bounds.origin.y)));
                },
                |_, _, _, _| {},
            )
            .w_full()
            .h(px(0.0))
        })
        .key_context("Worktrees")
        .track_focus(&root.wt_focus)
        .on_key_down(cx.listener(|root, ev: &KeyDownEvent, _window, cx| root.wt_key(&ev.keystroke, cx)))
        .child(
            header("worktrees-header", "Worktrees", worktrees_open)
                .on_click(cx.listener(|root, _ev: &ClickEvent, _window, cx| {
                    root.sidebar.worktrees_open = !root.sidebar.worktrees_open;
                    if !root.sidebar.worktrees_open {
                        // A closed section cannot hold the keys.
                        root.wt_focused = false;
                    }
                    root.save(cx);
                    cx.notify();
                })),
        )
        // Closed, the names end at the header's bottom; open, the rows' own marker reports it.
        .when(!worktrees_open, |d| d.child(sidebar_end_marker(cx)))
        .when(worktrees_open, |d| {
            d.child(render_worktree_rows(root, window, &theme, cx).opacity(worktrees_o))
        });
    let column = div()
        .relative()
        .w(px(width))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .bg(c.sidebar.alpha(style.chrome_alpha))
        .border_r_1()
        // An effect that inks the borders draws the sidebar's edge.
        .border_color(if style.ink_borders { c.border.alpha(0.0) } else { c.border.hsla() })
        .font_family(family)
        .text_size(px(style.ui(13.0)))
        .text_color(c.text.hsla())
        .children(switch)
        .when(sessions_h > 0.0 || !inbox, |d| d.child(sessions))
        .when(files_view, |d| d.child(explorer).child(worktrees))
        .child(handle);
    if !sliding {
        return Some(column);
    }
    // Sliding (Cmd+B): the column keeps its width and is clipped to the width on screen, so its
    // rows slide rather than reflow. Sliding out, it is only drawn: a cover takes the pointer.
    Some(
        div()
            .relative()
            .w(px(root.sidebar.shown_width(now)))
            .h_full()
            .flex_none()
            .overflow_hidden()
            .child(column)
            .when(root.sidebar.collapsed, |d| d.child(div().absolute().top_0().left_0().size_full().occlude())),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    /// The New File prompt in this folder (`Root::open_new_file_prompt`; the tree's menu runs it).
    NewFile(PathBuf),
    /// Moves the file or folder to the Trash (`Root::trash_path`; the tree's menu runs it).
    Trash(PathBuf),
    Reveal(PathBuf),
    CopyText(String),
    OpenDefault(PathBuf),
    /// A link from the terminal, opened in the default browser.
    OpenUrl(String),
}

/// The right-click menu for a tree entry (spec 6), and for the project folder itself (a press on
/// the tree's empty space). New File goes in a folder, or beside a file; Delete, last, moves the
/// entry to the Trash, and the project folder has none.
pub fn context_menu_items(path: &Path, is_dir: bool, project: &Path) -> Vec<(&'static str, MenuAction)> {
    if path == project {
        return vec![
            ("New File", MenuAction::NewFile(project.to_path_buf())),
            ("Reveal in Finder", MenuAction::Reveal(project.to_path_buf())),
            ("Copy path", MenuAction::CopyText(project.to_string_lossy().into_owned())),
        ];
    }
    let folder = if is_dir { path } else { path.parent().unwrap_or(project) };
    let relative = path.strip_prefix(project).unwrap_or(path).to_string_lossy().into_owned();
    vec![
        ("New File", MenuAction::NewFile(folder.to_path_buf())),
        ("Reveal in Finder", MenuAction::Reveal(path.to_path_buf())),
        ("Copy path", MenuAction::CopyText(path.to_string_lossy().into_owned())),
        ("Copy relative path", MenuAction::CopyText(relative)),
        ("Open in default app", MenuAction::OpenDefault(path.to_path_buf())),
        ("Delete", MenuAction::Trash(path.to_path_buf())),
    ]
}

pub struct ContextMenu {
    pub path: PathBuf,
    pub is_dir: bool,
    /// Window position of the click.
    pub at: gpui::Point<gpui::Pixels>,
}

pub fn run_menu_action(action: &MenuAction, cx: &mut App) {
    match action {
        // Need the root: the tree's menu runs them itself.
        MenuAction::NewFile(_) | MenuAction::Trash(_) => {}
        MenuAction::Reveal(p) => cx.reveal_path(p),
        MenuAction::CopyText(s) => cx.write_to_clipboard(gpui::ClipboardItem::new_string(s.clone())),
        MenuAction::OpenDefault(p) => cx.open_with_system(p),
        MenuAction::OpenUrl(u) => cx.open_url(u),
    }
}

/// The New File prompt (spec `2026-10-31-code-editor-and-new-file-design.md`, section 5): the
/// folder the file goes in and the name as typed, shown as a row of the tree.
pub struct NewFile {
    pub dir: PathBuf,
    pub name: LineEdit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewFileStep {
    Stay,
    Cancel,
    /// Enter on a name that is no file name: said in a notice, the prompt stays.
    Refused(&'static str),
    /// Enter on a name: the file to make.
    Create(PathBuf),
}

impl NewFile {
    pub fn new(dir: PathBuf) -> Self {
        NewFile { dir, name: LineEdit::new() }
    }

    pub fn key(&mut self, ks: &Keystroke) -> NewFileStep {
        match self.name.handle(ks) {
            Edit::Cancel => NewFileStep::Cancel,
            Edit::Submit | Edit::SubmitBack => match new_file_target(&self.dir, &self.name.text) {
                Ok(path) => NewFileStep::Create(path),
                Err(why) => NewFileStep::Refused(why),
            },
            _ => NewFileStep::Stay,
        }
    }

    /// Pasted text goes into the name, newlines and carriage returns dropped.
    pub fn paste(&mut self, text: &str) {
        self.name.insert_str(text);
    }
}

/// What a name typed into the prompt makes under `dir`: the name trimmed, `/` making folders on
/// the way; refused when empty, absolute, with an empty, `.` or `..` part, or with a character
/// macOS or a shell could not take in a name (a NUL, a colon).
pub fn new_file_target(dir: &Path, name: &str) -> Result<PathBuf, &'static str> {
    let name = name.trim();
    if name.is_empty() || name.starts_with('/') || name.contains(['\0', ':']) {
        return Err("a file name");
    }
    if name.split('/').any(|part| part.is_empty() || part == "." || part == "..") {
        return Err("a file name");
    }
    Ok(dir.join(name))
}

/// Makes the file, and the folders on the way; refuses a path that exists.
pub fn create_file(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::File::create_new(path).map(drop)
}

/// Moves a file or folder to the user's Trash, as Finder's Move to Trash does, so Put Back
/// restores it. Blocks while the move runs (a rename on the same volume): background executor.
pub fn move_to_trash(path: &Path) -> Result<(), String> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    NSFileManager::defaultManager().trashItemAtURL_resultingItemURL_error(&url, None).map_err(|e| e.localizedDescription().to_string())
}

/// What a failed create says: "already exists" for the common case, the error otherwise.
pub fn create_error(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::AlreadyExists => "already exists".into(),
        _ => e.to_string(),
    }
}

/// One row of the tree as drawn: an entry, or the New File prompt at its folder's children's depth.
enum Shown {
    Entry(explorer::Row),
    Prompt { depth: usize },
}

/// Where the prompt's row goes among the entries: under its folder's row (the folder is expanded
/// when the prompt opens), or first for the project folder. A folder no longer shown puts it first.
pub fn prompt_row(rows: &[explorer::Row], dir: &Path, project: &Path) -> (usize, usize) {
    if dir == project {
        return (0, 0);
    }
    rows.iter().position(|r| r.path == dir).map_or((0, 0), |i| (i + 1, rows[i].depth + 1))
}

/// The folders to expand and read again once `path` is made: from `dir` down to the file's own.
pub fn folders_to_show(dir: &Path, path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut p = path.parent();
    while let Some(d) = p {
        out.push(d.to_path_buf());
        if d == dir {
            break;
        }
        p = d.parent();
    }
    out.reverse();
    out
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
pub fn render_rows(root: &mut Root, entries: Vec<crate::explorer::Row>, theme: &Theme, cx: &mut Context<Root>) -> UniformList {
    // The New File prompt is a row of the tree while it is open or fading out.
    let prompt = root.new_file.current(root.frame_now).map(|(p, opacity)| {
        let (at, depth) = prompt_row(&entries, &p.dir, &root.project);
        (at, depth, p.name.clone(), opacity, root.new_file.value().is_some())
    });
    let rows: Rc<Vec<Shown>> = Rc::new({
        let mut shown: Vec<Shown> = entries.into_iter().map(Shown::Entry).collect();
        if let Some((at, depth, ..)) = &prompt {
            shown.insert((*at).min(shown.len()), Shown::Prompt { depth: *depth });
        }
        shown
    });
    let prompt_focus = root.new_file_focus.clone();
    let selected = root.explorer.selected.clone();
    let weak = cx.weak_entity();
    let family = root.ui_family();
    // The effect's room before each row, for a frame and a cursor it draws at the sidebar's edge.
    let pad = root.effect_style().sidebar_pad;
    let (highlights, hovered) = (root.effect_highlights(), root.effect_hover);
    let (selected_bg, hover_bg, dim, dot) = (theme.chrome.tab_active.hsla(), theme.chrome.tab_inactive.hsla(), theme.chrome.text_dim.hsla(), theme.tree.modified.hsla());
    let theme = theme.clone();
    let (drag_bg, drag_fg) = (theme.chrome.banner, theme.chrome.text);
    let count = rows.len();
    uniform_list("explorer-rows", count, move |range, _window, _cx| {
        rows[range]
            .iter()
            .map(|shown| {
                let row = match shown {
                    Shown::Entry(row) => row,
                    Shown::Prompt { depth } => {
                        let (_, _, edit, opacity, live) = prompt.as_ref().expect("a prompt row only with a prompt");
                        let w_press = weak.clone();
                        return div()
                            .id("new-file-row")
                            .w_full()
                            .h(px(ROW_H))
                            .pl(px(10.0 + pad + *depth as f32 * INDENT))
                            .pr(px(8.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .opacity(*opacity)
                            .child(div().flex_none().w(px(12.0)))
                            .child(div().min_w_0().flex_1().overflow_hidden().text_size(px(12.0)).child(line_edit::render(edit, "file name", &theme, family.clone(), 12.0)))
                            // The keys reach `tree_key` through the Explorer's listener; a press
                            // here keeps them in the tree (the root's press would close the prompt).
                            .when(*live, |d| {
                                d.track_focus(&prompt_focus).on_mouse_down(MouseButton::Left, move |_ev: &MouseDownEvent, _window, cx| {
                                    cx.stop_propagation();
                                    let _ = w_press.update(cx, |root, cx| {
                                        root.sidebar_focused = true;
                                        root.wt_focused = false;
                                        cx.notify();
                                    });
                                })
                            });
                    }
                };
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
                let key = hash_of(&path);
                let state = HighlightState::of(is_selected, hovered == Some(key)).filter(|_| highlights);
                div()
                    .id(("row", key))
                    .w_full()
                    .h(px(ROW_H))
                    .pl(px(10.0 + pad + row.depth as f32 * INDENT))
                    .pr(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    // Under an effect that paints the highlights, a selected or hovered row
                    // reports a mark and the effect fades its stroke; otherwise the app fills it.
                    .when(is_selected && !highlights, |d| d.bg(selected_bg))
                    .when(!highlights, |d| d.hover(move |s| s.bg(hover_bg)))
                    .when(highlights, |d| {
                        let w_hover = weak.clone();
                        d.on_hover(move |over: &bool, _window, cx| {
                            let _ = w_hover.update(cx, |root, cx| {
                                root.effect_hover = blocks::hover_after(root.effect_hover, key, *over);
                                cx.notify();
                            });
                        })
                    })
                    .when_some(state, |d, state| {
                        let w_mark = weak.clone();
                        d.relative().child(
                            gpui::canvas(
                                move |b, _window, cx| {
                                    let rect = Rect { x: f32::from(b.origin.x), y: f32::from(b.origin.y), w: f32::from(b.size.width), h: f32::from(b.size.height) };
                                    let _ = w_mark.update(cx, |root, _| root.effect_marks.push(Mark { key, rect, kind: Kind::RowHighlight, state }));
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        )
                    })
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
                            let now = Instant::now();
                            root.wt_menu.hide(now);
                            root.session_menu.hide(now);
                            root.link_menu.hide(now);
                            root.md_menu.hide(now);
                            root.view_menu.hide(now);
                            root.context_menu.show(ContextMenu { path: p_menu.clone(), is_dir, at: ev.position }, now);
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

/// Whether a key press is Escape with something to close.
pub(crate) fn escape_closes(key: &str, any_open: bool) -> bool {
    key == "escape" && any_open
}

/// The right-click menu, drawn over everything at the click. A mouse-down anywhere else closes
/// it (the root's listener); its own rows stop the event so their click lands.
pub fn render_context_menu(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let style = root.effect_style();
    let (menu, opacity) = root.context_menu.current(root.frame_now)?;
    // Drawn while it fades out too, but only the open menu occludes and takes clicks.
    let live = root.context_menu.value().is_some();
    let c = &root.theme.chrome;
    let mut m = div()
        .absolute()
        .left(menu.at.x)
        .top(menu.at.y - px(motion::lift(opacity, motion::MENU_LIFT)))
        .opacity(opacity)
        .min_w(px(200.0))
        .flex()
        .flex_col()
        .py(px(4.0))
        .map(|d| crate::chrome::menu_frame(d, &root.theme, &style))
        .font_family(root.ui_family())
        .text_size(px(style.ui(12.0)))
        .text_color(c.text.hsla())
        // Hover, the cursor and the wheel under the menu stop reacting while it is over them.
        .when(live, |m| m.occlude().on_any_mouse_down(|_: &MouseDownEvent, _window, cx| cx.stop_propagation()));
    let hover = crate::chrome::menu_colors(&root.theme, style.menus).hover;
    if style.menu_edges {
        m = m.child(crate::chrome::menu_report(0x12, opacity, cx));
    }
    let (effect_rows, hovered) = (style.menu_edges && style.highlights, root.effect_hover);
    for (i, (label, action)) in context_menu_items(&menu.path, menu.is_dir, &root.project).into_iter().enumerate() {
        m = m.child(div().id(("context-menu-row", i)).px(px(10.0)).py(px(4.0)).child(label).when(live, |row| {
            let key = blocks::menu_row_key(0x12, i);
            crate::chrome::menu_row_hover(row.cursor_pointer(), key, hovered == Some(key), effect_rows, hover, cx).on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                match &action {
                    MenuAction::NewFile(dir) => root.open_new_file_prompt(dir.clone(), cx),
                    MenuAction::Trash(path) => root.trash_path(path.clone(), cx),
                    other => run_menu_action(other, cx),
                }
                root.context_menu.hide(Instant::now());
                cx.notify();
            }))
        }));
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
    if let Some(idx) = tree.find_path(pane, path) {
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

/// The tab a file opens in, by the kind its name says (spec 9): a video as a video tab at its
/// start, anything else in the viewer.
pub fn file_tab(path: &Path) -> Tab {
    let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
    if crate::media::kind(&path.to_string_lossy()) == Some(crate::media::Media::Video) {
        Tab::Video { path: path.to_path_buf(), at: None, loop_range: None }
    } else if matches!(ext.as_deref(), Some("html" | "htm")) {
        // A page opens as a web tab (web pane spec, section 7); its source is a text file too,
        // which the viewer shows when the page is opened as text some other way.
        Tab::Browser { url: crate::web::file_url(path) }
    } else {
        Tab::Viewer { path: path.to_path_buf(), mode: ViewMode::default(), presenting: false, outline: false }
    }
}

/// The files the open video views show, each once, for the poll to fingerprint (spec 4).
pub fn video_paths(views: &HashMap<TabId, PathBuf>) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = views.values().cloned().collect();
    paths.sort();
    paths.dedup();
    paths
}

/// What one poll tells the video views: every view whose file the poll read gets that file's
/// fingerprint, changed or not, since the follow rule decides on two samples in a row. A view
/// made after the poll listed its paths waits for the next poll; it is not told its file is gone.
pub fn video_ticks(views: &[(TabId, PathBuf)], polled: &HashMap<PathBuf, Fp>) -> Vec<(TabId, Fp)> {
    views.iter().filter_map(|(tab, path)| polled.get(path).map(|fp| (*tab, *fp))).collect()
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
/// How long a changed row takes to fade in: Material 3's short3.
pub const EASE_IN: Duration = Duration::from_millis(150);

/// Where the shimmer's band is, 0 to 1 across the row, sweeping once per period.
pub fn shimmer_phase(elapsed: Duration) -> f32 {
    (elapsed.as_secs_f32() % SHIMMER_PERIOD.as_secs_f32()) / SHIMMER_PERIOD.as_secs_f32()
}

/// Opacity of a changed row `elapsed` after its worktree's changes landed: to 1 on Material 3's
/// standard decelerate curve, as it enters.
pub fn ease_in(elapsed: Duration) -> f32 {
    let t = (elapsed.as_secs_f32() / EASE_IN.as_secs_f32()).clamp(0.0, 1.0);
    if t >= 1.0 { 1.0 } else if t <= 0.0 { 0.0 } else { crate::motion::cubic_bezier(crate::motion::STANDARD_DECELERATE, t) }
}

/// The path the Worktrees selection holds for a row: the worktree's, or the changed file's.
pub fn row_path(row: &WtRow) -> PathBuf {
    match row {
        WtRow::Worktree { wt, .. } => wt.path.clone(),
        WtRow::Change { worktree, rel, .. } => worktree.join(rel),
    }
}

/// Up or Down through the rows' paths in `order` (worktrees and the changes of expanded ones,
/// as drawn). With none selected, Down takes the first and Up the last; at either end the
/// selection stays. A selection no row has but one row's path holds (a change of a worktree
/// collapsed since) counts as inside that row: Down goes past it and Up to it. A selection in
/// no row starts over as if none.
pub fn step_selection(order: &[&Path], selected: Option<&Path>, down: bool) -> Option<PathBuf> {
    let last = order.len().checked_sub(1)?;
    let at = selected.and_then(|s| order.iter().position(|w| s == *w).map(|i| (i, true)).or_else(|| order.iter().position(|w| s.starts_with(w)).map(|i| (i, false))));
    let i = match at {
        None if down => 0,
        None => last,
        Some((i, _)) if down => (i + 1).min(last),
        Some((i, true)) => i.saturating_sub(1),
        Some((i, false)) => i,
    };
    Some(order[i].to_path_buf())
}

/// How many terminal tabs were opened in `dir` or a folder inside it.
pub fn terminals_inside(tree: &Tree, dir: &Path) -> usize {
    tree.tabs().into_iter().filter(|(_, _, tab)| matches!(tab, Tab::Terminal { cwd: Some(c), .. } if c.starts_with(dir))).count()
}

/// A `git worktree list` read off the UI thread: the folder asked about, the side worktrees (or
/// git's words) and each listed worktree's admin stamp.
type Listed = (PathBuf, Listing, Vec<(PathBuf, crate::worktrees::Stamp)>);

/// What `git worktree list` said about a folder.
enum Listing {
    Listed(Vec<Worktree>),
    /// Git's words, for the section.
    Failed(String),
    /// It failed and the folder holds no `.git` any more: the repository went.
    Gone,
}

fn list_with_stamps(asked: PathBuf) -> Listed {
    match crate::worktrees::list_side(&asked) {
        Ok(list) => {
            let stamps = list.iter().filter_map(|w| w.admin.as_deref().map(|a| (w.path.clone(), crate::worktrees::stamp(a)))).collect();
            (asked, Listing::Listed(list), stamps)
        }
        Err(_) if std::fs::symlink_metadata(asked.join(".git")).is_err() => (asked, Listing::Gone, Vec::new()),
        Err(e) => (asked, Listing::Failed(e), Vec::new()),
    }
}

fn walk_age(path: PathBuf) -> (PathBuf, Option<crate::wt_scan::Newest>) {
    let newest = crate::wt_scan::newest_change(&path, crate::wt_scan::AGE_BUDGET);
    (path, newest)
}

/// One batch of stream events, repeats dropped: whether the kernel dropped events, whether the
/// open folder itself changed, and each changed folder once, in the order first seen.
#[derive(Default)]
struct WtBatch {
    must_scan: bool,
    root_changed: bool,
    changed: Vec<PathBuf>,
    seen: HashSet<PathBuf>,
}

impl WtBatch {
    fn add(&mut self, ev: crate::fsevents::Event) {
        match ev {
            crate::fsevents::Event::MustScan => self.must_scan = true,
            crate::fsevents::Event::RootChanged => self.root_changed = true,
            crate::fsevents::Event::Changed(p) => {
                if self.seen.insert(p.clone()) {
                    self.changed.push(p);
                }
            }
        }
    }
}

/// How many `git worktree list` runs or age walks run at once.
const WT_PARALLEL: usize = 4;

/// Runs `work` on each item on the background executor, `WT_PARALLEL` at a time, handing each
/// result to `apply` on the UI thread as it lands. Stops when `apply` says so (a newer pass) and
/// says false then, or when the window is gone.
async fn in_parallel<I: Send + 'static, T: Send + 'static>(this: &gpui::WeakEntity<Root>, cx: &mut gpui::AsyncApp, items: Vec<I>, work: fn(I) -> T, mut apply: impl FnMut(&mut Root, T, &mut Context<Root>) -> bool) -> bool {
    use futures::stream::FuturesUnordered;
    let mut items = items.into_iter();
    let mut running = FuturesUnordered::new();
    loop {
        while running.len() < WT_PARALLEL {
            let Some(item) = items.next() else { break };
            running.push(cx.background_executor().spawn(async move { work(item) }));
        }
        let Some(out) = running.next().await else { return true };
        if !this.update(cx, |root, cx| apply(root, out, cx)).unwrap_or(false) {
            return false;
        }
    }
}

/// The Worktrees section's line when the file-system watcher is not running: on a network volume,
/// when it could not start, or when the open folder itself went away; `None` while watching.
pub fn watched_notice(w: &crate::worktrees::Watched) -> Option<&'static str> {
    use crate::worktrees::Watched;
    match w {
        Watched::Watching => None,
        Watched::NotLocal => Some("not watched: the folder is on a network volume"),
        Watched::Failed(_) => Some("not watched: the file watcher could not start"),
        Watched::FolderGone => Some("the folder is gone"),
    }
}

/// The Worktrees section's one dim line, drawn after the rows: the watcher's notice when it is
/// not watching; else, with no repository listed, the first repository's error; else, with no
/// rows, `no worktrees`. A repository that failed while others listed is left to the command.
pub fn wt_section_line(watched: &crate::worktrees::Watched, none_listed: bool, first_error: Option<&str>, has_rows: bool) -> Option<String> {
    if let Some(notice) = watched_notice(watched) {
        return Some(notice.to_string());
    }
    match first_error {
        Some(why) if none_listed => Some(why.to_string()),
        _ => (!has_rows).then(|| "no worktrees".to_string()),
    }
}

/// The age cell: `age_text` of the age, with `+` after a capped walk ("at least this recent"),
/// `~` after a lagging one; empty with no age. A change stamped after `now` reads `now`.
pub fn wt_row_age(age: Option<&crate::worktrees::Age>, now: SystemTime) -> String {
    let Some(age) = age else { return String::new() };
    let Some(changed) = age.changed else { return String::new() };
    let mut text = crate::wt_scan::age_text(now.duration_since(changed).unwrap_or(Duration::ZERO));
    if age.capped {
        text.push('+');
    }
    if age.lagging {
        text.push('~');
    }
    text
}

/// The canonical home folder, read once, for the rows' place lines.
fn home_dir() -> &'static Path {
    static HOME: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    HOME.get_or_init(|| std::env::var_os("HOME").map(|h| crate::wt_scan::canonical(Path::new(&h))).unwrap_or_default())
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

/// The count a worktree row shows: its changes once its status has loaded, and nothing for a
/// clean one or one not loaded yet (the word "clean" is never written).
pub fn wt_count_text(dirty: Option<usize>) -> Option<String> {
    match dirty? {
        0 => None,
        1 => Some("1 change".to_string()),
        n => Some(format!("{n} changes")),
    }
}

/// The right-click menu of a worktree row (every row is a side worktree, so every row offers
/// removal). A dirty or unloaded worktree offers force as well, since the plain removal will be
/// refused. One with no files (missing, or bare) has nothing to open a terminal in, and only
/// force removes it.
pub fn worktree_menu_items(wt: &Worktree, dirty: Option<usize>) -> Vec<(&'static str, WtMenuAction)> {
    let mut v = Vec::new();
    if wt.has_files() {
        v.push(("Open terminal here", WtMenuAction::OpenTerminal(wt.path.clone())));
    }
    if wt.has_files() {
        v.push(("Remove worktree", WtMenuAction::Remove(wt.path.clone())));
    }
    if !wt.has_files() || dirty.map_or(true, |n| n > 0) {
        v.push(("Remove worktree, force", WtMenuAction::RemoveForce(wt.path.clone())));
    }
    v
}

/// The Worktrees rows: each worktree with its count of changes (a shimmer while an expanded
/// one's first `git status` runs) and, once expanded and loaded, its changed files easing in.
/// Requests the next frame only while something moves and Reduce Motion is off.
fn render_worktree_rows(root: &mut Root, window: &mut Window, theme: &Theme, cx: &mut Context<Root>) -> Stateful<Div> {
    let style = root.effect_style();
    root.sync_worktree_hold();
    let rows = root.worktrees.rows();
    let selected = root.worktrees.selected.clone();
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
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&root.sidebar.wt_scroll)
        // While the pointer is over the list its order holds, so a row does not move from under it.
        .on_hover(cx.listener(|root, hovered: &bool, _window, cx| {
            root.wt_hovered = *hovered;
            cx.notify();
        }));
    // Read once a frame; the ages tick every ten seconds (`tick_worktree_ages`), so the text is
    // made here from the stored time and never kept.
    let wall = SystemTime::now();
    let line = wt_section_line(&root.worktrees.watched, root.worktrees.by_repo.is_empty(), root.worktrees.errors.values().next().map(String::as_str), !rows.is_empty());
    let (open, home) = (root.wt_open.clone(), home_dir());
    for (i, row) in rows.into_iter().enumerate() {
        // `flex_none`, or the capped column shrinks every row to fit and the rows overprint
        // each other; the list scrolls instead.
        let is_selected = selected.as_deref() == Some(row_path(&row).as_path());
        // A worktree row has a second line, its place, at a smaller size.
        let h = match row {
            WtRow::Worktree { .. } => ROW_H + style.ui(14.0),
            WtRow::Change { .. } => ROW_H,
        };
        let base = div()
            .id(("worktree-row", i))
            .relative()
            .overflow_hidden()
            .w_full()
            .flex_none()
            .h(px(h))
            .pr(px(8.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .when(is_selected, |d| d.bg(selected_bg))
            .hover(|s| s.bg(hover_bg))
            .cursor_pointer();
        let el = match row {
            WtRow::Worktree { wt, age, expanded, loading, dirty } => {
                let band = loading.then(|| {
                    let phase = if still {
                        0.5
                    } else {
                        moving = true;
                        shimmer_phase(root.worktrees.loading_since.get(&wt.path).map_or(Duration::ZERO, |t| now.saturating_duration_since(*t)))
                    };
                    div().absolute().top_0().h_full().w(px(0.3 * width)).left(px(phase * 1.3 * width - 0.3 * width)).bg(c.tab_inactive.alpha(0.6))
                });
                // Only a row with changes expands, so only it has a chevron.
                let chevron = if !wt.has_files() || !dirty.is_some_and(|n| n > 0) {
                    ""
                } else if expanded {
                    "\u{25be}"
                } else {
                    "\u{25b8}"
                };
                let (name, branch) = wt_row_label(&wt);
                let name_color = if root.worktrees.is_stale(&wt.path, wall) { dim } else { c.text.hsla() };
                let age_text = wt_row_age(age.as_ref(), wall);
                let place = wt_row_place(&wt.path, &open, home);
                let count = wt_count_text(dirty);
                let (p_click, p_menu) = (wt.path.clone(), wt.path.clone());
                let has_menu = !worktree_menu_items(&wt, dirty).is_empty();
                base.pl(px(10.0 + style.sidebar_pad))
                    .children(band)
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .line_height(px(style.ui(18.0)))
                                    .child(div().flex_none().w(px(12.0)).text_color(dim).child(chevron))
                                    .child(div().flex_none().max_w(px(width * 0.6)).overflow_hidden().whitespace_nowrap().text_ellipsis().text_color(name_color).child(name))
                                    .child(div().min_w_0().flex_1().overflow_hidden().whitespace_nowrap().text_ellipsis().text_color(dim).children(branch))
                                    .when(wt.missing, |d| d.child(div().flex_none().text_color(theme.tree.deleted.hsla()).child("missing")))
                                    .children(count.map(|text| div().flex_none().text_size(px(style.ui(11.0))).text_color(theme.tree.modified.hsla()).child(text)))
                                    .when(!age_text.is_empty(), |d| d.child(div().flex_none().text_size(px(style.ui(12.0))).text_color(dim).child(age_text))),
                            )
                            // Under the name: past the chevron's 12 and the gap's 6. Set tight
                            // under its name, so the pair reads as one row with air before the next.
                            // A definite width (the sidebar's less the row's 10 and 8 and the 18),
                            // since gpui keeps a no-wrap text's first measure, taken without one,
                            // and a long path would be clipped instead of ending in an ellipsis.
                            .child(
                                div()
                                    .pl(px(18.0))
                                    .line_height(px(style.ui(14.0)))
                                    .child(div().w(px((width - 36.0).max(0.0))).overflow_hidden().whitespace_nowrap().text_ellipsis().text_size(px(style.ui(11.0))).text_color(dim).child(place)),
                            ),
                    )
                    .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                        root.worktrees.selected = Some(p_click.clone());
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
                                let now = Instant::now();
                                root.context_menu.hide(now);
                                root.link_menu.hide(now);
                                root.view_menu.hide(now);
                                root.wt_menu.show((p_menu.clone(), ev.position), now);
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
                base.pl(px(10.0 + style.sidebar_pad + 2.0 * INDENT))
                    .opacity(opacity)
                    .text_color(row_color(Some(status), false, theme).hsla())
                    .child(div().min_w_0().truncate().child(rel.to_string_lossy().into_owned()))
                    .on_click(cx.listener(move |root, ev: &ClickEvent, _window, cx| {
                        root.worktrees.selected = Some(path.clone());
                        root.open_viewer(&path, ev.click_count() >= 2, cx);
                    }))
            }
        };
        list = list.child(el);
    }
    // After the rows, so a row's child index stays its index in `rows()` for the scroll.
    if let Some(line) = line {
        list = list.child(div().flex_none().h(px(ROW_H)).pl(px(10.0 + style.sidebar_pad)).pr(px(10.0)).flex().items_center().text_color(dim).truncate().child(line));
    }
    // Last in the scroll, so its top is where the last name ends (below the sidebar when the list
    // overflows).
    list = list.child(sidebar_end_marker(cx));
    if moving {
        window.request_animation_frame();
    }
    list
}

/// A zero-height canvas that reports its top as where the sidebar's names end, for an effect's
/// sidebar foot.
fn sidebar_end_marker(cx: &mut Context<Root>) -> impl IntoElement {
    let weak = cx.weak_entity();
    gpui::canvas(
        move |bounds, _window, cx| {
            let _ = weak.update(cx, |root, _| root.effect_sidebar_end = Some(f32::from(bounds.origin.y)));
        },
        |_, _, _, _| {},
    )
    .w_full()
    .h(px(0.0))
    .flex_none()
}

/// The worktree row's right-click menu, drawn like the file tree's.
pub fn render_worktree_menu(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let style = root.effect_style();
    let ((path, at), opacity) = root.wt_menu.current(root.frame_now).map(|(m, o)| (m.clone(), o))?;
    let items = root.worktrees.find(&path).map(|wt| worktree_menu_items(wt, root.worktrees.dirty_count(&path))).unwrap_or_default();
    if items.is_empty() {
        // The worktree went away (or has nothing to offer): the menu goes too, at once, since
        // there is nothing left to draw.
        root.wt_menu.clear();
        return None;
    }
    // Drawn while it fades out too, but only the open menu occludes and takes clicks.
    let live = root.wt_menu.value().is_some() || root.session_menu.value().is_some();
    let c = &root.theme.chrome;
    let hover = crate::chrome::menu_colors(&root.theme, style.menus).hover;
    let mut m = div()
        .absolute()
        .left(at.x)
        .top(at.y - px(motion::lift(opacity, motion::MENU_LIFT)))
        .opacity(opacity)
        .min_w(px(200.0))
        .flex()
        .flex_col()
        .py(px(4.0))
        .map(|d| crate::chrome::menu_frame(d, &root.theme, &style))
        .font_family(root.ui_family())
        .text_size(px(style.ui(12.0)))
        .text_color(c.text.hsla())
        .when(live, |m| m.occlude().on_any_mouse_down(|_: &MouseDownEvent, _window, cx| cx.stop_propagation()));
    if style.menu_edges {
        m = m.child(crate::chrome::menu_report(0x13, opacity, cx));
    }
    let (effect_rows, hovered) = (style.menu_edges && style.highlights, root.effect_hover);
    for (i, (label, action)) in items.into_iter().enumerate() {
        m = m.child(div().id(("worktree-menu-row", i)).px(px(10.0)).py(px(4.0)).child(label).when(live, |row| {
            let key = blocks::menu_row_key(0x13, i);
            crate::chrome::menu_row_hover(row.cursor_pointer(), key, hovered == Some(key), effect_rows, hover, cx).on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                root.wt_menu.hide(Instant::now());
                match action.clone() {
                    WtMenuAction::OpenTerminal(p) => root.open_terminal_in(p, cx),
                    WtMenuAction::Remove(p) => root.remove_worktree(p, false, cx),
                    WtMenuAction::RemoveForce(p) => root.remove_worktree(p, true, cx),
                }
                cx.notify();
            }))
        }));
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

    /// The first load and the safety-net pass: find repositories, list each, age each; rows show as
    /// each stage lands; a result older than a newer pass is dropped (`worktrees_seq`). A
    /// repository the pass no longer finds is dropped once every list has landed, unless it is
    /// still a repository under the open folder (a nested one the stream registered, which the
    /// walk does not enter). The current rows stay up meanwhile.
    pub fn refresh_worktrees(&mut self, cx: &mut Context<Self>) {
        self.worktrees_seq += 1;
        let seq = self.worktrees_seq;
        self.wt_pass_at = Instant::now();
        self.wt_gen += 1;
        let gen = self.wt_gen;
        let open = self.wt_open.clone();
        cx.spawn(async move |this, cx| {
            let o = open.clone();
            let found = cx.background_executor().spawn(async move { crate::wt_scan::find_repos(&o, crate::wt_scan::DISCOVERY_BUDGET) }).await;
            let mut seen: HashSet<PathBuf> = HashSet::new();
            let listed = in_parallel(&this, cx, found.repos, list_with_stamps, |root, listed, cx| {
                if root.worktrees_seq != seq {
                    return false;
                }
                seen.insert(root.apply_worktree_list(gen, listed));
                root.after_worktree_lists(cx);
                true
            })
            .await;
            if !listed {
                return;
            }
            let Ok(Some(unseen)) = this.update(cx, |root, _| {
                (root.worktrees_seq == seq).then(|| root.worktrees.by_repo.keys().chain(root.worktrees.errors.keys()).filter(|k| !seen.contains(*k)).cloned().collect::<Vec<_>>())
            }) else {
                return;
            };
            let gone = cx.background_executor().spawn(async move { crate::worktrees::pass_drops(&unseen, &open, |k| k.join(".git").exists()) }).await;
            let Ok(Some(to_age)) = this.update(cx, |root, cx| {
                if root.worktrees_seq != seq {
                    return None;
                }
                for repo in &gone {
                    root.forget_repo(repo, gen);
                }
                root.after_worktree_lists(cx);
                root.update_worktree_stream(cx);
                Some(root.worktrees.all().filter(|w| w.has_files()).map(|w| w.path.clone()).collect::<Vec<_>>())
            }) else {
                return;
            };
            in_parallel(&this, cx, to_age, walk_age, |root, (path, newest), cx| {
                if root.worktrees_seq != seq {
                    return false;
                }
                root.apply_age(&path, newest);
                cx.notify();
                true
            })
            .await;
        })
        .detach();
    }

    /// Creates the stream over `wt_watch::stream_paths` (recreating with `since` when the set
    /// changes) and spawns the drain task; sets `worktrees.watched`. A folder on a network volume
    /// gets no stream, and the ten minute pass alone keeps its list.
    pub fn start_worktree_watch(&mut self, cx: &mut Context<Self>) {
        if let Some(mut rx) = self.wt_events.take() {
            cx.spawn(async move |this, cx| {
                // One batch a second at most (`fsevents::LATENCY`): everything waiting is taken
                // at once, repeats dropped (a set, since a burst can hold thousands of folders),
                // and handled in one update.
                while let Some(first) = rx.next().await {
                    let mut batch = WtBatch::default();
                    batch.add(first);
                    while let Ok(ev) = rx.try_recv() {
                        batch.add(ev);
                    }
                    let handled = this.update(cx, |root, cx| root.worktree_batch(batch, cx));
                    if handled.is_err() {
                        return;
                    }
                }
            })
            .detach();
        }
        if !crate::fsevents::local_volume(&self.wt_open) {
            self.stop_worktree_watch();
            self.worktrees.watched = crate::worktrees::Watched::NotLocal;
            self.mark_lagging(&[]);
            cx.notify();
            return;
        }
        let (paths, left_out) = crate::wt_watch::stream_paths(&self.wt_open, &self.worktrees);
        let since = self.wt_stream.as_ref().map(|s| s.latest_event_id());
        let filter = crate::wt_watch::stream_filter(self.wt_open.clone(), self.wt_home, paths[1..].to_vec());
        match crate::fsevents::Stream::start(&paths, since, filter, self.wt_tx.clone()) {
            Ok(stream) => {
                // The old stream, if any, stops here, after the new one took over from its id.
                self.wt_stream = Some(stream);
                self.wt_stream_paths = paths;
                self.worktrees.watched = crate::worktrees::Watched::Watching;
                self.mark_lagging(&left_out);
            }
            Err(e) => {
                self.stop_worktree_watch();
                self.worktrees.watched = crate::worktrees::Watched::Failed(e);
                self.mark_lagging(&[]);
            }
        }
        cx.notify();
    }

    /// Releases the stream (the window closed).
    pub fn stop_worktree_watch(&mut self) {
        self.wt_stream = None;
        self.wt_stream_paths.clear();
    }

    /// One event from the stream, by `wt_watch::classify`; git and walks run on the background executor.
    pub fn worktree_event(&mut self, event: crate::fsevents::Event, cx: &mut Context<Self>) {
        let mut batch = WtBatch::default();
        batch.add(event);
        self.worktree_batch(batch, cx);
    }

    /// One batch from the stream: the open folder's own news first, then the changed folders,
    /// already filtered in the stream's callback (`wt_watch::stream_filter`), classified and
    /// collapsed (`wt_watch::collapse`), so a burst of thousands of folders is one action per
    /// worktree, repository or folder.
    fn worktree_batch(&mut self, batch: WtBatch, cx: &mut Context<Self>) {
        use crate::wt_watch::Rule;
        if batch.root_changed {
            if self.wt_open.exists() {
                if self.worktrees.watched == crate::worktrees::Watched::FolderGone {
                    self.start_worktree_watch(cx);
                }
                self.refresh_worktrees(cx);
            } else {
                // A pass in flight would fill the list again.
                self.worktrees_seq += 1;
                self.worktrees.watched = crate::worktrees::Watched::FolderGone;
                self.wt_gen += 1;
                let repos: Vec<PathBuf> = self.worktrees.by_repo.keys().cloned().collect();
                for repo in &repos {
                    self.forget_repo(repo, self.wt_gen);
                }
                self.worktrees.errors.clear();
                self.after_worktree_lists(cx);
            }
        } else if batch.must_scan {
            self.refresh_worktrees(cx);
        }
        let open = &self.wt_open;
        let rules: Vec<_> = batch.changed.into_iter().map(|p| (crate::wt_watch::classify(&p, open, &self.worktrees), p)).collect();
        for (rule, dirs) in crate::wt_watch::collapse(rules) {
            match rule {
                Rule::RelistRepo(repo) => self.relist_repo(repo, cx),
                Rule::CheckStamp { repo, name } => self.check_worktree_stamp(repo, &name, cx),
                Rule::Touched(wt) => self.touch_worktree(wt, dirs, cx),
                Rule::Unknown(dir) => self.look_at_folder(dir, cx),
                Rule::Ignore => {}
            }
        }
    }

    /// Every 10 s from the slow tick: notify when the section is open and has rows (the age text
    /// is computed at paint); on the 10 minute mark, `refresh_worktrees`.
    pub fn tick_worktree_ages(&mut self, cx: &mut Context<Self>) {
        if crate::explorer_watch::full_pass_due(self.wt_pass_at, Instant::now()) {
            self.refresh_worktrees(cx);
        }
        if self.sidebar.worktrees_open && !self.sidebar.collapsed && self.worktrees.all().next().is_some() {
            cx.notify();
        }
    }

    /// The order holds while the pointer is over the list, or while the keyboard is in the
    /// section with a row selected.
    pub fn sync_worktree_hold(&mut self) {
        self.worktrees.hold_order = self.wt_hovered || (self.wt_focused && self.worktrees.selected.is_some());
    }

    /// `git worktree list` for one repository, with its stamps, off the UI thread; new worktrees
    /// get their age walk.
    fn relist_repo(&mut self, repo: PathBuf, cx: &mut Context<Self>) {
        self.wt_gen += 1;
        let gen = self.wt_gen;
        cx.spawn(async move |this, cx| {
            let listed = cx.background_executor().spawn(async move { list_with_stamps(repo) }).await;
            let _ = this.update(cx, |root, cx| root.listed_one(gen, listed, cx));
        })
        .detach();
    }

    /// A list from an event landed: apply it, follow with the stream, age the worktrees that have
    /// no age yet.
    fn listed_one(&mut self, gen: u64, listed: Listed, cx: &mut Context<Self>) {
        let repo = self.apply_worktree_list(gen, listed);
        self.after_worktree_lists(cx);
        self.update_worktree_stream(cx);
        let unaged: Vec<PathBuf> = self.worktrees.by_repo.get(&repo).into_iter().flatten()
            .filter(|w| w.has_files() && self.worktrees.ages.get(&w.path).and_then(|a| a.changed).is_none())
            .map(|w| w.path.clone())
            .collect();
        if unaged.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            in_parallel(&this, cx, unaged, walk_age, |root, (path, newest), cx| {
                if root.worktrees.find(&path).is_some() {
                    root.apply_age(&path, newest);
                    cx.notify();
                }
                true
            })
            .await;
        })
        .detach();
    }

    /// `<repo>/.git/worktrees/<name>` changed: the worktree's status is due a second from now
    /// whatever the stamp says, since a commit rewrites the index there without touching `HEAD`,
    /// `gitdir` or `locked`. The admin folder's stamp is read off the UI thread and the repository
    /// listed again only when it moved (a `git status` writing `index` moves nothing).
    fn check_worktree_stamp(&mut self, repo: PathBuf, name: &str, cx: &mut Context<Self>) {
        let Some(wt) = self.worktrees.by_repo.get(&repo).into_iter().flatten().find(|w| w.admin.as_deref().and_then(Path::file_name).is_some_and(|n| n == std::ffi::OsStr::new(name))) else {
            return;
        };
        let Some(admin) = wt.admin.clone() else { return };
        let stored = self.worktrees.stamps.get(&wt.path).copied();
        let path = wt.path.clone();
        self.schedule_worktree_status(&path, cx);
        cx.spawn(async move |this, cx| {
            let now = cx.background_executor().spawn(async move { crate::worktrees::stamp(&admin) }).await;
            if stored != Some(now) {
                let _ = this.update(cx, |root, cx| root.relist_repo(repo, cx));
            }
        })
        .detach();
    }

    /// Something changed in `dirs` inside the worktree `wt`: its age becomes now, unless the
    /// folder is gone (missing) or, after a minute's quiet, no entry in any of `dirs` newer than the
    /// age is anything but noise (`only_noise_newer`: `.DS_Store`, `.git`, a build or cache
    /// folder; a deletion alone leaves nothing newer and does not count). A worktree marked missing
    /// whose folder is back is found again and its repository listed. The stat and the
    /// `read_dir`s (stopping at the first folder with work in it) run off the UI thread. A
    /// worktree still there has its status due a second later, whether or not its age moved.
    fn touch_worktree(&mut self, wt: PathBuf, dirs: Vec<PathBuf>, cx: &mut Context<Self>) {
        let at = SystemTime::now();
        let since = match self.worktrees.ages.get(&wt).and_then(|a| a.changed) {
            Some(c) if at.duration_since(c).is_ok_and(|d| d < Duration::from_secs(60)) => None,
            c => Some(c.unwrap_or(SystemTime::UNIX_EPOCH)),
        };
        cx.spawn(async move |this, cx| {
            let w = wt.clone();
            let (gone, only_noise) = cx
                .background_executor()
                .spawn(async move {
                    let gone = std::fs::symlink_metadata(&w).is_err();
                    (gone, !gone && since.is_some_and(|s| dirs.iter().all(|d| crate::wt_watch::only_noise_newer(d, s))))
                })
                .await;
            let _ = this.update(cx, |root, cx| {
                let Some(known) = root.worktrees.find(&wt) else { return };
                let was_missing = known.missing;
                if gone {
                    if !was_missing {
                        root.worktrees.mark_missing(&wt);
                        root.after_worktree_lists(cx);
                        root.update_worktree_stream(cx);
                    }
                    return;
                }
                if root.worktrees.found_again(&wt) {
                    // Its admin folder and stamp come from git again.
                    if let Some(repo) = root.worktrees.repo_of(&wt).map(Path::to_path_buf) {
                        root.relist_repo(repo, cx);
                    }
                    root.update_worktree_stream(cx);
                }
                if !only_noise {
                    root.worktrees.touched(&wt, at);
                }
                root.schedule_worktree_status(&wt, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// A folder outside every listed worktree changed: one look for `.git` off the UI thread. A
    /// new one registers the repository (listed, stamped, aged); a known repository whose `.git`
    /// is gone is dropped.
    fn look_at_folder(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let known = self.worktrees.by_repo.contains_key(&dir);
        self.wt_gen += 1;
        let gen = self.wt_gen;
        cx.spawn(async move |this, cx| {
            let d = dir.clone();
            let seen = cx
                .background_executor()
                .spawn(async move {
                    let has_git = std::fs::symlink_metadata(d.join(".git")).is_ok();
                    match (has_git, known) {
                        (true, false) => Some(Some(list_with_stamps(d))),
                        (false, true) => Some(None),
                        _ => None,
                    }
                })
                .await;
            let _ = this.update(cx, |root, cx| match seen {
                Some(Some(listed)) => root.listed_one(gen, listed, cx),
                Some(None) => {
                    root.forget_repo(&dir, gen);
                    root.after_worktree_lists(cx);
                    root.update_worktree_stream(cx);
                }
                None => {}
            });
        })
        .detach();
    }

    /// Files a list under its repository (the main checkout git printed, or the folder asked
    /// about when it has no side worktrees) with its stamps, or git's words under the folder, or
    /// drops the repository when it went. A result older than the last list or drop applied to
    /// that repository is dropped. Returns the key.
    fn apply_worktree_list(&mut self, gen: u64, (asked, list, stamps): Listed) -> PathBuf {
        let newer = |root: &Self, key: &Path| !crate::worktrees::list_applies(gen, root.wt_list_gen.get(key).copied());
        match list {
            Listing::Listed(list) => {
                let repo = list.first().map_or_else(|| asked.clone(), |w| w.repo.clone());
                if newer(self, &repo) {
                    return repo;
                }
                self.wt_list_gen.insert(repo.clone(), gen);
                self.worktrees.errors.remove(&asked);
                self.worktrees.set_repo(&repo, list);
                self.worktrees.stamps.extend(stamps);
                repo
            }
            Listing::Failed(e) => {
                if !newer(self, &asked) {
                    self.worktrees.errors.insert(asked.clone(), e);
                }
                asked
            }
            Listing::Gone => {
                if !newer(self, &asked) {
                    self.forget_repo(&asked, gen);
                }
                asked
            }
        }
    }

    /// Drops a repository, its rows and its error. `gen` is the number of the look that found
    /// it gone, kept so a list started before it and landing after it is dropped.
    fn forget_repo(&mut self, repo: &Path, gen: u64) {
        self.worktrees.remove_repo(repo);
        self.worktrees.errors.remove(repo);
        // Never lower a number an event's list already raised.
        if crate::worktrees::list_applies(gen, self.wt_list_gen.get(repo).copied()) {
            self.wt_list_gen.insert(repo.to_path_buf(), gen);
        }
    }

    /// An age walk landed: merged with the age held (`Worktrees::merge_age`), so a change the
    /// stream reported while it ran is never moved back.
    fn apply_age(&mut self, path: &Path, newest: Option<crate::wt_scan::Newest>) {
        self.worktrees.merge_age(path, crate::worktrees::Age { changed: newest.map(|n| n.at), capped: newest.is_some_and(|n| n.capped), lagging: false });
    }

    /// After the lists changed: a menu on a worktree that went closes, newly listed worktrees
    /// get their status, and the section redraws.
    fn after_worktree_lists(&mut self, cx: &mut Context<Self>) {
        if self.wt_menu.value().is_some_and(|m| self.worktrees.find(&m.0).is_none()) {
            self.wt_menu.hide(Instant::now());
        }
        self.load_pending_worktrees(cx);
        cx.notify();
    }

    /// The stream follows the lists: a changed path set recreates it from the old one's event id;
    /// the worktrees left out of it are marked as lagging, and none when there is no stream.
    fn update_worktree_stream(&mut self, cx: &mut Context<Self>) {
        let (paths, left_out) = crate::wt_watch::stream_paths(&self.wt_open, &self.worktrees);
        match &self.wt_stream {
            Some(_) if paths != self.wt_stream_paths => self.start_worktree_watch(cx),
            Some(_) => self.mark_lagging(&left_out),
            // No stream: the section's notice says the folder is not watched, and no row lags.
            None => self.mark_lagging(&[]),
        }
    }

    /// `lagging` on exactly the worktrees in `left_out`.
    fn mark_lagging(&mut self, left_out: &[PathBuf]) {
        for (path, age) in self.worktrees.ages.iter_mut() {
            age.lagging = left_out.contains(path);
        }
        for path in left_out {
            self.worktrees.ages.entry(path.clone()).or_default().lagging = true;
        }
    }

    /// Runs `git status` for each worktree `Worktrees::to_load` names (not loaded yet, or due
    /// after a change), off the UI thread, and applies each result as it lands. A change seen
    /// while a run was in flight is picked up when it lands.
    pub fn load_pending_worktrees(&mut self, cx: &mut Context<Self>) {
        for path in self.worktrees.to_load() {
            self.worktrees.mark_loading(&path);
            let p = path.clone();
            cx.spawn(async move |this, cx| {
                let changes = cx.background_executor().spawn(async move { crate::gitstatus::run_status(&p) }).await;
                let _ = this.update(cx, |root, cx| {
                    root.worktrees.set_changes(&path, changes);
                    root.load_pending_worktrees(cx);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// The watcher saw a change in the worktree `path`: its status is due a debounce from now, a
    /// later change moving the time up to the cap (`Worktrees::status_due_at`), and a task at that
    /// time runs whatever is due by then. Several such tasks are harmless, since `due_now` hands
    /// each worktree out once.
    fn schedule_worktree_status(&mut self, path: &Path, cx: &mut Context<Self>) {
        let now = Instant::now();
        let due = self.worktrees.status_due_at(path, now + crate::worktrees::STATUS_DEBOUNCE);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(due.saturating_duration_since(now)).await;
            let _ = this.update(cx, |root, cx| {
                root.worktrees.due_now(Instant::now());
                root.load_pending_worktrees(cx);
            });
        })
        .detach();
    }

    /// Opens `path` in the focused pane as a preview (single click) or pinned (double click,
    /// Enter): a video as a video tab, paused, anything else in the viewer (`file_tab`). The
    /// tab's view is made and loaded when the pane first shows it.
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
                // Unsaved edits in the preview are written first (its first edit normally pins
                // it). One that cannot be written stays, and the file opens in a tab of its own.
                let viewer = self.tree.tab_id(pane, idx).and_then(|id| self.viewers.get(&id).cloned());
                let kept = viewer.and_then(|v| {
                    v.update(cx, |v, cx| match v.save_now() {
                        Ok(()) => None,
                        Err(e) => {
                            cx.notify();
                            Some(e)
                        }
                    })
                });
                if let Some(e) = kept {
                    self.show_notice(e, cx);
                    let tab = file_tab(path);
                    crate::telemetry::pane_open(crate::telemetry::kind_of(&tab), crate::telemetry::Opened::Explorer);
                    let idx = self.tree.add_tab(pane, tab);
                    self.preview_tab = if pin { None } else { self.tree.tab_id(pane, idx) };
                    idx
                } else {
                    // Same tab id, another file: the old view goes (a video's with its player), the
                    // render makes the new one.
                    if let Some(id) = self.tree.tab_id(pane, idx) {
                        self.viewers.remove(&id);
                        self.forget_outline(id);
                        // As in `close_tab`: paused first, so the sound and the display token end
                        // now even while something still holds the view.
                        if let Some(v) = self.videos.remove(&id) {
                            v.update(cx, |v, cx| v.pause(cx));
                        }
                    }
                    self.tree.replace_tab(pane, idx, file_tab(path));
                    idx
                }
            }
            OpenPlan::Add => {
                let tab = file_tab(path);
                crate::telemetry::pane_open(crate::telemetry::kind_of(&tab), crate::telemetry::Opened::Explorer);
                let idx = self.tree.add_tab(pane, tab);
                self.preview_tab = if pin { None } else { self.tree.tab_id(pane, idx) };
                idx
            }
        };
        self.tree.activate(pane, idx);
        self.explorer.selected = Some(path.to_path_buf());
        self.save(cx);
        cx.notify();
    }

    /// Escape while a right-click menu (the tree's, a worktree's, a terminal link's or a
    /// document's), a markdown tab's view menu or a title bar popover is open closes it and
    /// nothing else.
    pub(crate) fn escape_closes_menu(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        if !escape_closes(&ks.key, self.context_menu.value().is_some() || self.wt_menu.value().is_some() || self.session_menu.value().is_some() || self.link_menu.value().is_some() || self.md_menu.value().is_some() || self.view_menu.value().is_some() || self.popover.value().is_some()) {
            return false;
        }
        let now = Instant::now();
        self.context_menu.hide(now);
        self.wt_menu.hide(now);
        self.session_menu.hide(now);
        self.link_menu.hide(now);
        self.md_menu.hide(now);
        self.view_menu.hide(now);
        self.close_popovers(now);
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

    /// `git worktree remove` off the UI thread; git's refusal (changes, the main checkout) is a
    /// notice, and so are terminals left open in a removed one (they keep running).
    pub fn remove_worktree(&mut self, path: PathBuf, force: bool, cx: &mut Context<Self>) {
        let repo = self.worktrees.repo_of(&path).map_or_else(|| self.project.clone(), Path::to_path_buf);
        let root = repo.clone();
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
                root.relist_repo(repo, cx);
            });
        })
        .detach();
    }

    /// Keys in the Worktrees section: Up/Down move, Enter or Right expands a worktree or opens a
    /// change pinned, Space previews it, Left collapses, Escape returns to the pane.
    pub fn wt_key(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) {
        if ks.modifiers.platform || ks.modifiers.control || ks.modifiers.alt {
            return;
        }
        self.sync_worktree_hold();
        let rows = self.worktrees.rows();
        let sel = self.worktrees.selected.clone();
        match ks.key.as_str() {
            "down" | "up" => {
                let order: Vec<PathBuf> = rows.iter().map(row_path).collect();
                let order: Vec<&Path> = order.iter().map(PathBuf::as_path).collect();
                match step_selection(&order, sel.as_deref(), ks.key == "down") {
                    Some(p) => self.worktrees.selected = Some(p),
                    None => return,
                }
            }
            "enter" | "right" | "left" | "space" => {
                if let Some(row) = sel.as_deref().and_then(|s| rows.iter().find(|r| row_path(r) == s)) {
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
        let rows = self.worktrees.rows();
        if let Some(i) = self.worktrees.selected.as_deref().and_then(|s| rows.iter().position(|r| row_path(r) == s)) {
            self.sidebar.wt_scroll.scroll_to_item(i);
        }
        cx.notify();
    }

    /// The tree's Delete: the tabs on the file, or on anything in the folder, let their unsaved
    /// edits go and close (so nothing writes the file back), then the file or folder moves to the
    /// Trash off this thread and the tree forgets it. A notice says where it went, or why not.
    pub fn trash_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if path == self.project || !path.starts_with(&self.project) {
            return;
        }
        for id in self.tree.tabs_within(&path) {
            if let Some(v) = self.viewers.get(&id).cloned() {
                v.update(cx, |v, _| v.abandon_edits());
            }
            if let Some((pane, idx)) = self.tree.locate(id) {
                self.close_tab(pane, idx, cx);
            }
        }
        if self.new_file.value().is_some_and(|p| p.dir.starts_with(&path)) {
            self.new_file.hide(Instant::now());
        }
        let name = path.file_name().map_or_else(|| path.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
        cx.spawn(async move |this, cx| {
            let moved = {
                let path = path.clone();
                cx.background_executor().spawn(async move { move_to_trash(&path) }).await
            };
            let _ = this.update(cx, |root, cx| {
                match moved {
                    Ok(()) => {
                        root.explorer.forget(&path);
                        root.load_pending_dirs(cx);
                        root.refresh_status(cx);
                        root.save(cx);
                        root.show_notice(format!("Moved to the Trash: {name}"), cx);
                    }
                    Err(e) => root.show_notice(format!("Could not move {name} to the Trash: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Opens the New File prompt under `dir` (expanded, so the row has a place) and gives the tree
    /// the keys.
    pub fn open_new_file_prompt(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let now = Instant::now();
        if dir != self.project {
            self.explorer.expanded.insert(dir.clone());
            self.explorer.selected = Some(dir.clone());
            self.load_pending_dirs(cx);
        }
        self.new_file.show(NewFile::new(dir), now);
        self.sidebar_focused = true;
        self.wt_focused = false;
        self.save(cx);
        cx.notify();
    }

    /// A key while the New File prompt is open: typed into the name, Cmd+V pastes, Escape closes,
    /// Enter makes the file and opens it for editing with the keys in the pane. True when the
    /// prompt took the key.
    fn new_file_key(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(p) = self.new_file.value_mut() else { return false };
        let now = Instant::now();
        if ks.modifiers.platform && ks.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                p.paste(&text);
            }
            cx.notify();
            return true;
        }
        match p.key(ks) {
            NewFileStep::Stay => {}
            NewFileStep::Refused(why) => self.show_notice(why.to_string(), cx),
            NewFileStep::Cancel => {
                self.new_file.hide(now);
            }
            NewFileStep::Create(path) => {
                let dir = p.dir.clone();
                match create_file(&path) {
                    Ok(()) => {
                        self.new_file.hide(now);
                        for folder in folders_to_show(&dir, &path) {
                            if folder != self.project {
                                self.explorer.expanded.insert(folder.clone());
                            }
                            self.explorer.invalidate(&folder);
                        }
                        self.load_pending_dirs(cx);
                        self.open_viewer(&path, true, cx);
                    }
                    Err(e) => self.show_notice(create_error(&e), cx),
                }
            }
        }
        cx.notify();
        true
    }

    /// Keys in the file tree (ruling 7): Up/Down move, Right expands or enters, Left collapses or
    /// goes to the parent, Enter opens pinned, Space previews, Escape returns to the pane.
    pub fn tree_key(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) {
        if self.new_file_key(ks, cx) {
            return;
        }
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
    /// viewers' and video views' files.
    pub fn watched_paths(&self, cx: &App) -> Vec<PathBuf> {
        let mut v = vec![self.project.clone()];
        v.extend(self.explorer.expanded_dirs());
        v.extend(self.viewers.values().map(|viewer| viewer.read(cx).path.clone()));
        v.extend(video_paths(&self.video_files(cx)));
        v
    }

    /// The file each video view shows, by its tab.
    fn video_files(&self, cx: &App) -> HashMap<TabId, PathBuf> {
        self.videos.iter().map(|(tab, v)| (*tab, v.read(cx).model.path.clone())).collect()
    }

    /// A changed directory is read again, a changed viewer file reloaded. A video's file is left
    /// to `tick_videos`, which sees every poll, changed or not.
    pub fn paths_changed(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let videos: Vec<PathBuf> = video_paths(&self.video_files(cx));
        for p in paths {
            if !self.reload_viewers_for(p, cx) && !videos.contains(p) {
                self.explorer.invalidate(p);
            }
        }
        self.load_pending_dirs(cx);
    }

    /// Every poll (spec 4): each video view is ticked with its file's fingerprint from this poll,
    /// unchanged files included, so the follow rule sees a file that has held still for one poll
    /// and reloads it then. The poll is also the once a second look at whether the window is
    /// hidden or minimized, which draws nothing for `render` to notice.
    pub fn tick_videos(&mut self, polled: &HashMap<PathBuf, Fp>, cx: &mut Context<Self>) {
        let mut views: Vec<(TabId, PathBuf)> = self.video_files(cx).into_iter().collect();
        views.sort();
        for (tab, fp) in video_ticks(&views, polled) {
            if let Some(v) = self.videos.get(&tab).cloned() {
                v.update(cx, |v, cx| v.tick(fp, cx));
            }
        }
        self.update_video_activity(cx);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn worktrees_take_what_the_explorer_does_not_need_and_never_less_than_a_third() {
        let (e, w) = super::sidebar_split(900.0, 200.0, true, true);
        assert_eq!((e, w), (200.0 + super::ROW_H, 900.0 - 200.0 - super::ROW_H));
        assert_eq!(super::sidebar_split(900.0, 2000.0, true, true), (600.0, 300.0));
    }

    #[test]
    fn closed_worktrees_sit_under_the_explorer_s_last_row_and_a_closed_explorer_leaves_worktrees_the_sidebar() {
        let h = super::ROW_H;
        // A short tree: the Worktrees header right under its last row, the rest left empty.
        assert_eq!(super::sidebar_split(900.0, 200.0, true, false), (200.0 + h, h));
        // A tree longer than the sidebar scrolls above the header, which stays at the foot.
        assert_eq!(super::sidebar_split(900.0, 2000.0, true, false), (900.0 - h, h));
        // An empty tree keeps the Explorer a row under its header.
        assert_eq!(super::sidebar_split(900.0, 0.0, true, false), (2.0 * h, h));
        assert_eq!(super::sidebar_split(900.0, 200.0, false, true), (h, 900.0 - h));
        assert_eq!(super::sidebar_split(900.0, 200.0, false, false), (h, h));
    }

    #[test]
    fn a_short_sidebar_keeps_the_explorer_s_header() {
        let (e, w) = super::sidebar_split(60.0, 2000.0, true, true);
        assert_eq!((e, w), (2.0 * super::ROW_H, 60.0 - 2.0 * super::ROW_H));
    }

    #[test]
    fn the_sessions_section_is_its_rows_up_to_two_fifths_of_the_sidebar_and_a_header_when_closed_or_empty() {
        let h = super::ROW_H;
        assert_eq!(super::sessions_height(900.0, 100.0, true), h + 100.0);
        assert_eq!(super::sessions_height(900.0, 2000.0, true), 360.0, "past two fifths the rows scroll");
        assert_eq!(super::sessions_height(900.0, 2000.0, false), h);
        assert_eq!(super::sessions_height(900.0, 0.0, true), h, "no session: the header alone");
        assert_eq!(super::sessions_height(30.0, 2000.0, true), h, "never less than its header");
    }

    #[test]
    fn the_section_says_when_the_folder_is_not_watched() {
        use crate::worktrees::Watched;
        use super::watched_notice;
        assert_eq!(watched_notice(&Watched::Watching), None);
        assert_eq!(watched_notice(&Watched::NotLocal), Some("not watched: the folder is on a network volume"));
        assert_eq!(watched_notice(&Watched::Failed("FSEvents refused the stream".into())), Some("not watched: the file watcher could not start"));
        assert_eq!(watched_notice(&Watched::FolderGone), Some("the folder is gone"));
    }

    use super::*;
    use crate::tree::{Side, Tree};
    use protocol::SessionId;

    #[test]
    fn escape_with_nothing_open_is_not_taken() {
        assert!(!escape_closes("escape", false));
        assert!(escape_closes("escape", true));
        assert!(!escape_closes("a", true));
    }

    #[test]
    fn the_width_stays_between_the_limits() {
        assert_eq!(clamp_width(10.0), MIN_W);
        assert_eq!(clamp_width(260.0), 260.0);
        assert_eq!(clamp_width(9000.0), MAX_W);
        assert_eq!(clamp_width(f32::NAN), MIN_W, "a NaN from a bad file is the minimum, not a panic");
    }

    #[test]
    fn the_context_menu_lists_new_file_then_the_four_file_actions_then_delete() {
        let items = context_menu_items(Path::new("/p/src/a.rs"), false, Path::new("/p"));
        let labels: Vec<&str> = items.iter().map(|(l, _)| *l).collect();
        assert_eq!(labels, ["New File", "Reveal in Finder", "Copy path", "Copy relative path", "Open in default app", "Delete"]);
        assert_eq!(items[5].1, MenuAction::Trash("/p/src/a.rs".into()), "the file itself");
        assert_eq!(context_menu_items(Path::new("/p/src"), true, Path::new("/p"))[5].1, MenuAction::Trash("/p/src".into()), "a folder, whole");
        assert_eq!(items[0].1, MenuAction::NewFile("/p/src".into()), "beside a file");
        assert_eq!(items[2].1, MenuAction::CopyText("/p/src/a.rs".into()));
        assert_eq!(items[3].1, MenuAction::CopyText("src/a.rs".into()));
        assert_eq!(context_menu_items(Path::new("/p/src"), true, Path::new("/p"))[0].1, MenuAction::NewFile("/p/src".into()), "in a folder");
        assert_eq!(context_menu_items(Path::new("/elsewhere/f"), false, Path::new("/p"))[3].1, MenuAction::CopyText("/elsewhere/f".into()), "outside the project the full path");
        let root = context_menu_items(Path::new("/p"), true, Path::new("/p"));
        assert_eq!(root.iter().map(|(l, _)| *l).collect::<Vec<_>>(), ["New File", "Reveal in Finder", "Copy path"], "the project folder: no relative path, no default app, never Delete");
        assert_eq!(root[0].1, MenuAction::NewFile("/p".into()));
    }

    #[test]
    fn a_new_file_name_is_trimmed_may_make_folders_and_is_refused_when_it_is_no_name() {
        let dir = Path::new("/p/src");
        assert_eq!(new_file_target(dir, " main.rs "), Ok(PathBuf::from("/p/src/main.rs")));
        assert_eq!(new_file_target(dir, "a/b/c.toml"), Ok(PathBuf::from("/p/src/a/b/c.toml")), "folders on the way");
        for bad in ["", "   ", "/abs", "a//b", "./a", "../a", "a/..", ".", "..", "a:b", "a\0b"] {
            assert_eq!(new_file_target(dir, bad), Err("a file name"), "{bad:?}");
        }
        assert_eq!(new_file_target(dir, ".env"), Ok(PathBuf::from("/p/src/.env")), "a dot file is a name");
    }

    #[test]
    fn the_new_file_prompt_types_a_name_refuses_a_bad_one_cancels_and_asks_to_create() {
        use gpui::Keystroke;
        let key = |k: &str| Keystroke::parse(k).unwrap();
        let mut p = NewFile::new(PathBuf::from("/p/src"));
        assert_eq!(p.key(&key("enter")), NewFileStep::Refused("a file name"), "Enter on nothing");
        for c in "lib.rs".chars() {
            assert_eq!(p.key(&key(&format!("{c}->{c}"))), NewFileStep::Stay);
        }
        assert_eq!(p.name.text, "lib.rs");
        p.paste("x\ny");
        assert_eq!(p.name.text, "lib.rsxy", "a paste drops its line breaks");
        assert_eq!(p.key(&key("backspace")), NewFileStep::Stay);
        assert_eq!(p.key(&key("backspace")), NewFileStep::Stay);
        assert_eq!(p.key(&key("enter")), NewFileStep::Create(PathBuf::from("/p/src/lib.rs")));
        assert_eq!(p.key(&key("escape")), NewFileStep::Cancel);
    }

    #[test]
    fn create_file_makes_the_folders_on_the_way_and_refuses_an_existing_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b/c.txt");
        create_file(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "", "an empty file");
        let err = create_file(&path).unwrap_err();
        assert_eq!(create_error(&err), "already exists");
        assert_eq!(folders_to_show(dir.path(), &path), vec![dir.path().to_path_buf(), dir.path().join("a"), dir.path().join("a/b")], "the prompt's folder down to the file's");
        assert_eq!(folders_to_show(dir.path(), &dir.path().join("d.txt")), vec![dir.path().to_path_buf()]);
    }

    #[test]
    fn the_prompt_row_sits_under_its_folder_at_the_children_depth_or_first_for_the_project() {
        let row = |path: &str, depth: usize, is_dir: bool| explorer::Row { path: path.into(), name: String::new(), depth, is_dir, expanded: is_dir, ignored: false, loading: false, status: None, dirty: false };
        let rows = vec![row("/p/src", 0, true), row("/p/src/a.rs", 1, false), row("/p/x.md", 0, false)];
        assert_eq!(prompt_row(&rows, Path::new("/p/src"), Path::new("/p")), (1, 1));
        assert_eq!(prompt_row(&rows, Path::new("/p"), Path::new("/p")), (0, 0));
        assert_eq!(prompt_row(&rows, Path::new("/p/gone"), Path::new("/p")), (0, 0), "a folder not shown: first");
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
        assert!((mid - crate::motion::cubic_bezier(crate::motion::STANDARD_DECELERATE, 0.5)).abs() < 1e-6, "Material's standard decelerate: {mid}");
    }

    #[test]
    fn the_worktree_menu_offers_a_terminal_and_removal_for_every_row() {
        use crate::worktrees::Worktree;
        let feat = Worktree { path: "/p-f".into(), repo: "/p".into(), head: "2".into(), branch: Some("f".into()), missing: false, bare: false, locked: false, admin: None };
        let labels = |items: Vec<(&'static str, WtMenuAction)>| items.into_iter().map(|(l, _)| l).collect::<Vec<_>>();
        assert_eq!(labels(worktree_menu_items(&feat, Some(0))), ["Open terminal here", "Remove worktree"]);
        assert_eq!(labels(worktree_menu_items(&feat, Some(3))), ["Open terminal here", "Remove worktree", "Remove worktree, force"], "dirty: the plain remove will be refused, so force is offered");
        assert_eq!(labels(worktree_menu_items(&feat, None)), ["Open terminal here", "Remove worktree", "Remove worktree, force"], "not loaded: cannot tell, offer both");
        let gone = Worktree { missing: true, ..feat.clone() };
        assert_eq!(labels(worktree_menu_items(&gone, None)), ["Remove worktree, force"], "nothing to open; only force removes a missing one");
        let bare = Worktree { bare: true, branch: None, head: String::new(), ..feat.clone() };
        assert_eq!(labels(worktree_menu_items(&bare, None)), ["Remove worktree, force"], "a bare one: no files to open a terminal in, only force removes it");
        assert_eq!(worktree_menu_items(&feat, Some(0))[0].1, WtMenuAction::OpenTerminal("/p-f".into()));
        assert_eq!(worktree_menu_items(&feat, None)[2].1, WtMenuAction::RemoveForce("/p-f".into()));
    }

    #[test]
    fn the_row_text_names_the_count_and_never_clean() {
        assert_eq!(wt_count_text(None), None, "not loaded yet: nothing");
        assert_eq!(wt_count_text(Some(0)), None, "clean: nothing, never the word clean");
        assert_eq!(wt_count_text(Some(1)), Some("1 change".to_string()));
        assert_eq!(wt_count_text(Some(3)), Some("3 changes".to_string()));
    }

    #[test]
    fn terminals_inside_counts_the_tabs_opened_in_a_worktree() {
        use crate::tree::Tab;
        let mut tree = Tree::new(Tab::Terminal { session: SessionId(1), cwd: None });
        let pane = tree.focused;
        tree.add_tab(pane, Tab::Terminal { session: SessionId(2), cwd: Some("/p-f".into()) });
        tree.add_tab(pane, Tab::Terminal { session: SessionId(3), cwd: Some("/p-f/sub".into()) });
        tree.add_tab(pane, Tab::Terminal { session: SessionId(4), cwd: Some("/p-feat".into()) });
        tree.add_tab(pane, Tab::Viewer { path: "/p-f/a.rs".into(), mode: ViewMode::default(), presenting: false, outline: false });
        assert_eq!(terminals_inside(&tree, Path::new("/p-f")), 2, "by path components: /p-feat is not inside /p-f, a viewer is no terminal");
        assert_eq!(terminals_inside(&tree, Path::new("/elsewhere")), 0);
        assert_eq!(removed_notice(1), "worktree removed; 1 terminal was open in it");
        assert_eq!(removed_notice(3), "worktree removed; 3 terminals were open in it");
    }

    #[test]
    fn up_and_down_step_the_selected_path_through_every_row() {
        let order = [Path::new("/a"), Path::new("/b"), Path::new("/c")];
        let step = |sel: Option<&str>, down: bool| step_selection(&order, sel.map(Path::new), down);
        assert_eq!(step(None, true), Some("/a".into()), "none selected: Down takes the first");
        assert_eq!(step(None, false), Some("/c".into()), "and Up the last");
        assert_eq!(step(Some("/a"), true), Some("/b".into()));
        assert_eq!(step(Some("/c"), true), Some("/c".into()), "the end holds");
        assert_eq!(step(Some("/b"), false), Some("/a".into()));
        assert_eq!(step(Some("/a"), false), Some("/a".into()), "the start holds");
        assert_eq!(step(Some("/b/src/x.rs"), true), Some("/c".into()), "from a change no longer drawn, Down goes past its worktree");
        assert_eq!(step(Some("/b/src/x.rs"), false), Some("/b".into()), "and Up to its worktree");
        assert_eq!(step(Some("/gone"), true), Some("/a".into()), "a selection that went starts over");
        assert_eq!(step_selection(&[], Some(Path::new("/a")), true), None, "no worktrees, no selection");
        // The keys walk every drawn row: an expanded worktree's changes are reachable.
        use crate::gitstatus::Status;
        use crate::worktrees::{Worktree, Worktrees};
        let wt = |p: &str| Worktree { path: p.into(), repo: "/r".into(), head: "1".into(), branch: None, missing: false, bare: false, locked: false, admin: None };
        let mut w = Worktrees::new(vec![]);
        w.set_repo(Path::new("/r"), vec![wt("/r-a"), wt("/r-b")]);
        assert!(w.toggle(Path::new("/r-a")));
        w.mark_loading(Path::new("/r-a"));
        w.set_changes(Path::new("/r-a"), Some(vec![(PathBuf::from("x.rs"), Status::Modified), (PathBuf::from("y.rs"), Status::Added)]));
        let paths: Vec<PathBuf> = w.rows().iter().map(row_path).collect();
        let rows: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
        assert_eq!(step_selection(&rows, Some(Path::new("/r-a")), true), Some("/r-a/x.rs".into()), "Down from the worktree lands on its first change");
        assert_eq!(step_selection(&rows, Some(Path::new("/r-a/x.rs")), true), Some("/r-a/y.rs".into()));
        assert_eq!(step_selection(&rows, Some(Path::new("/r-a/y.rs")), true), Some("/r-b".into()), "then the next worktree");
        assert_eq!(step_selection(&rows, Some(Path::new("/r-a/x.rs")), false), Some("/r-a".into()), "Up from the change returns to the worktree");
    }

    #[test]
    fn a_collapsed_sidebar_takes_no_width() {
        assert_eq!(visible_width(300.0, false), 300.0);
        assert_eq!(visible_width(300.0, true), 0.0);
    }

    #[test]
    fn cmd_b_slides_the_width_on_screen_then_settles() {
        let _g = crate::motion::test_lock(false);
        let t0 = Instant::now();
        let out = Some(crate::motion::Slide::new(300.0, 0.0, t0));
        assert_eq!(shown_width(out, 0.0, t0), 300.0, "starts at the width on screen");
        let mid = shown_width(out, 0.0, t0 + crate::motion::SIDEBAR_SLIDE / 2);
        assert!(mid > 0.0 && mid < 150.0, "past half way at half time: {mid}");
        assert_eq!(shown_width(out, 0.0, t0 + crate::motion::SIDEBAR_SLIDE), 0.0);
        assert_eq!(shown_width(None, 280.0, t0), 280.0, "no slide: the settled width");
        assert_eq!(shown_width(Some(crate::motion::Slide::new(0.0, 300.0, t0)), 320.0, t0 + crate::motion::SIDEBAR_SLIDE), 320.0, "after the slide, an edge drag's width");
        let mut fade = Transient::Hidden;
        assert_eq!(reveal(&mut fade, true, t0), 0.0, "a section opening fades its rows in");
        assert_eq!(reveal(&mut fade, true, t0 + crate::motion::ENTER), 1.0);
        assert_eq!(reveal(&mut fade, false, t0 + crate::motion::ENTER), 0.0, "and closing drops them at once");
        assert!(!fade.animating(t0 + crate::motion::ENTER));
    }

    #[test]
    fn cmd_b_snaps_under_reduce_motion() {
        let _g = crate::motion::test_lock(true);
        let t0 = Instant::now();
        assert_eq!(shown_width(Some(crate::motion::Slide::new(300.0, 0.0, t0)), 0.0, t0), 0.0);
        let mut fade = Transient::Hidden;
        assert_eq!(reveal(&mut fade, true, t0), 1.0);
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
        let a = tree.add_tab(pane, Tab::Viewer { path: "/p/a.rs".into(), mode: ViewMode::default(), presenting: false, outline: false });
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

    #[test]
    fn a_video_in_the_tree_opens_as_a_video_tab_and_is_reused() {
        use crate::tree::Tab;
        let mut tree = Tree::new(Tab::Terminal { session: SessionId(1), cwd: None });
        let pane = tree.panes()[0];
        let cut = Path::new("/p/cut.mp4");
        assert_eq!(file_tab(cut), Tab::Video { path: cut.into(), at: None, loop_range: None }, "a video opens as a video tab");
        assert_eq!(file_tab(Path::new("/p/a.rs")), Tab::Viewer { path: "/p/a.rs".into(), mode: ViewMode::default(), presenting: false, outline: false });
        assert_eq!(file_tab(Path::new("/p/out/report.html")), Tab::Browser { url: "file:///p/out/report.html".into() }, "a page opens as a web tab");
        let v = tree.add_tab(pane, file_tab(cut));
        let v_id = tree.tab_id(pane, v).unwrap();
        // A single click and a double click on the same file both bring the open tab forward.
        assert_eq!(open_plan(&tree, pane, cut, None, false), OpenPlan::Activate(v));
        assert_eq!(open_plan(&tree, pane, cut, None, true), OpenPlan::Activate(v));
        // A video preview is replaced by the next previewed file like any other preview.
        assert_eq!(open_plan(&tree, pane, Path::new("/p/b.rs"), Some(v_id), false), OpenPlan::Replace(v));
    }

    #[test]
    fn watched_paths_include_every_video_tab() {
        let views: std::collections::HashMap<TabId, PathBuf> = [(7, PathBuf::from("/p/b.mov")), (3, PathBuf::from("/p/a.mp4")), (9, PathBuf::from("/p/a.mp4"))].into_iter().collect();
        assert_eq!(video_paths(&views), vec![PathBuf::from("/p/a.mp4"), PathBuf::from("/p/b.mov")], "each file once, two tabs on it or not");
        assert!(video_paths(&std::collections::HashMap::new()).is_empty());
    }

    #[test]
    fn every_video_is_ticked_each_poll_but_never_for_a_path_the_poll_did_not_read() {
        let views = vec![(1, PathBuf::from("/p/a.mp4")), (2, PathBuf::from("/p/gone.mp4")), (3, PathBuf::from("/p/new.mp4"))];
        let polled: std::collections::HashMap<PathBuf, crate::video::Fp> = [(PathBuf::from("/p/a.mp4"), Some((5, 10))), (PathBuf::from("/p/gone.mp4"), None)].into_iter().collect();
        // An unchanged file is ticked too (the follow rule needs its second, stable sample); a
        // missing file is ticked with no fingerprint; a view made after the poll read its paths
        // waits for the next poll rather than being told its file is gone.
        assert_eq!(video_ticks(&views, &polled), vec![(1, Some((5, 10))), (2, None)]);
    }

    #[test]
    fn the_age_cell_marks_a_capped_walk_and_a_lagging_one_and_is_blank_without_an_age() {
        use crate::worktrees::Age;
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let age = |secs: u64, capped: bool, lagging: bool| Age { changed: Some(now - Duration::from_secs(secs)), capped, lagging };
        assert_eq!(wt_row_age(Some(&age(4 * 60, false, false)), now), "4 min");
        assert_eq!(wt_row_age(Some(&age(4 * 60, true, false)), now), "4 min+");
        assert_eq!(wt_row_age(Some(&age(4 * 60, false, true)), now), "4 min~");
        assert_eq!(wt_row_age(Some(&age(4 * 60, true, true)), now), "4 min+~");
        assert_eq!(wt_row_age(None, now), "");
        assert_eq!(wt_row_age(Some(&Age { changed: None, capped: false, lagging: false }), now), "", "no change seen yet");
        assert_eq!(wt_row_age(Some(&Age { changed: Some(now + Duration::from_secs(90)), capped: false, lagging: false }), now), "now", "a clock ahead reads now");
    }

    #[test]
    fn the_section_line_says_why_it_is_not_watched_then_why_nothing_listed_then_that_there_is_nothing() {
        use crate::worktrees::Watched;
        let gone = Watched::FolderGone;
        assert_eq!(wt_section_line(&gone, true, Some("fatal: x"), false).as_deref(), Some("the folder is gone"), "the watcher's notice first");
        assert_eq!(wt_section_line(&Watched::NotLocal, false, None, true).as_deref(), Some("not watched: the folder is on a network volume"), "said under the rows too");
        assert_eq!(wt_section_line(&Watched::Watching, true, Some("fatal: x"), false).as_deref(), Some("fatal: x"), "nothing listed: the first error");
        assert_eq!(wt_section_line(&Watched::Watching, false, Some("fatal: x"), false).as_deref(), Some("no worktrees"), "a repository listed: its failed neighbour is not shown");
        assert_eq!(wt_section_line(&Watched::Watching, true, None, false).as_deref(), Some("no worktrees"), "no repositories at all");
        assert_eq!(wt_section_line(&Watched::Watching, false, Some("fatal: x"), true), None, "rows and watching: no line");
    }
}
