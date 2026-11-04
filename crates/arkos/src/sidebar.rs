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
use crate::worktrees::{Worktree, WtRow};
use crate::Root;
use gpui::{div, prelude::*, px, uniform_list, App, ClickEvent, Context, Div, FocusHandle, KeyDownEvent, Keystroke, MouseButton, MouseDownEvent, ScrollHandle, Stateful, UniformList, UniformListScrollHandle, Window};
use protocol::SessionId;
use crate::video::Fp;
use std::collections::HashMap;
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
    /// Cmd+B's slide while it runs: the width on screen eases to the new `visible_width`.
    pub slide: Option<motion::Slide>,
    /// Each section's rows fade in as it opens (they go at once as it closes, since a closing
    /// list would still take clicks).
    pub explorer_fade: Transient<()>,
    pub worktrees_fade: Transient<()>,
    /// The Outline section (structure spec, section 2): open or not, its rows' fade, its scroll,
    /// and the headings whose children it hides (the outline's own state, not the document's).
    pub outline_open: bool,
    pub outline_fade: Transient<()>,
    pub outline_scroll: ScrollHandle,
    pub outline_collapsed: std::collections::BTreeSet<crate::md_fold::FoldKey>,
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
    pub fn new(width: f32, collapsed: bool, explorer_open: bool, worktrees_open: bool, outline_open: bool, cx: &mut Context<Root>) -> Self {
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
            outline_open,
            outline_fade: fade(outline_open),
            outline_scroll: ScrollHandle::new(),
            outline_collapsed: Default::default(),
        }
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
        self.sliding(now) || self.explorer_fade.animating(now) || self.worktrees_fade.animating(now)
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

/// The column: the Explorer and Worktrees sections, each under a header that opens and closes
/// it, and the resize handle. `None` while collapsed, once its slide out has finished.
pub fn render(root: &mut Root, window: &mut Window, cx: &mut Context<Root>) -> Option<Div> {
    let style = root.effect_style();
    let now = root.frame_now;
    let sliding = root.sidebar.sliding(now);
    if root.sidebar.collapsed && !sliding {
        return None;
    }
    let explorer_o = reveal(&mut root.sidebar.explorer_fade, root.sidebar.explorer_open, now);
    let worktrees_o = reveal(&mut root.sidebar.worktrees_fade, root.sidebar.worktrees_open, now);
    let outline_o = reveal(&mut root.sidebar.outline_fade, root.sidebar.outline_open, now);
    let theme = root.theme.clone();
    let c = &theme.chrome;
    let width = root.sidebar.width;
    let family = root.ui_family();
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
    let explorer = div()
        .flex()
        .flex_col()
        .min_h_0()
        .when(explorer_open, |d| d.flex_1())
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
                root.link_menu.hide(now);
                root.md_menu.hide(now);
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
        .when(explorer_open, |d| d.child(render_rows(root, &theme, cx).opacity(explorer_o)));
    let outline_open = root.sidebar.outline_open;
    let outline = div()
        .flex_none()
        .flex()
        .flex_col()
        .border_t_1()
        .border_color(if style.ink_borders { c.border.alpha(0.0) } else { c.border.hsla() })
        .child(header("outline-header", "Outline", outline_open).on_click(cx.listener(|root, _ev: &ClickEvent, _window, cx| {
            root.sidebar.outline_open = !root.sidebar.outline_open;
            root.outline_hover = None;
            root.save(cx);
            cx.notify();
        })))
        .when(outline_open, |d| d.child(crate::outline::render_rows(root, &theme, cx).opacity(outline_o)));
    let worktrees_open = root.sidebar.worktrees_open;
    let worktrees = div()
        .flex_none()
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
        .child(explorer)
        .child(outline)
        .child(worktrees)
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
pub fn render_rows(root: &mut Root, theme: &Theme, cx: &mut Context<Root>) -> UniformList {
    let entries = root.explorer.rows();
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
                            .pl(px(10.0 + *depth as f32 * INDENT))
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
                    .pl(px(10.0 + row.depth as f32 * INDENT))
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
                            root.link_menu.hide(now);
                            root.md_menu.hide(now);
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
        Tab::Viewer { path: path.to_path_buf(), mode: ViewMode::default(), presenting: false }
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

/// The Worktrees rows: each worktree (a shimmer while its first `git status` runs) and, once
/// expanded and loaded, its changed files easing in. Requests the next frame only while
/// something moves and Reduce Motion is off.
fn render_worktree_rows(root: &mut Root, window: &mut Window, theme: &Theme, cx: &mut Context<Root>) -> Stateful<Div> {
    let style = root.effect_style();
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
        list = list.child(div().flex_none().h(px(ROW_H)).px(px(10.0)).flex().items_center().text_color(dim).child(why));
    }
    for (i, row) in rows.into_iter().enumerate() {
        // `flex_none`, or the capped column shrinks every row to fit and the rows overprint
        // each other; the list scrolls instead.
        let base = div()
            .id(("worktree-row", i))
            .relative()
            .overflow_hidden()
            .w_full()
            .flex_none()
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
                    .children(count.map(|(text, color)| div().flex_none().text_size(px(style.ui(11.0))).text_color(color).child(text)))
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
                                let now = Instant::now();
                                root.context_menu.hide(now);
                                root.link_menu.hide(now);
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
    let live = root.wt_menu.value().is_some();
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
                    if root.wt_menu.value().is_some_and(|m| root.worktrees.find(&m.0).is_none()) {
                        root.wt_menu.hide(Instant::now());
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
    /// document's) or a title bar popover is open closes it and nothing else.
    pub(crate) fn escape_closes_menu(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        if !escape_closes(&ks.key, self.context_menu.value().is_some() || self.wt_menu.value().is_some() || self.link_menu.value().is_some() || self.md_menu.value().is_some() || self.popover.value().is_some()) {
            return false;
        }
        let now = Instant::now();
        self.context_menu.hide(now);
        self.wt_menu.hide(now);
        self.link_menu.hide(now);
        self.md_menu.hide(now);
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
    /// prompt open, the .
    pub fn wt_key(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) {
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
    fn terminals_inside_counts_the_tabs_opened_in_a_worktree() {
        use crate::tree::Tab;
        let mut tree = Tree::new(Tab::Terminal { session: SessionId(1), cwd: None });
        let pane = tree.focused;
        tree.add_tab(pane, Tab::Terminal { session: SessionId(2), cwd: Some("/p-f".into()) });
        tree.add_tab(pane, Tab::Terminal { session: SessionId(3), cwd: Some("/p-f/sub".into()) });
        tree.add_tab(pane, Tab::Terminal { session: SessionId(4), cwd: Some("/p-feat".into()) });
        tree.add_tab(pane, Tab::Viewer { path: "/p-f/a.rs".into(), mode: ViewMode::default(), presenting: false });
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
        let a = tree.add_tab(pane, Tab::Viewer { path: "/p/a.rs".into(), mode: ViewMode::default(), presenting: false });
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
        assert_eq!(file_tab(Path::new("/p/a.rs")), Tab::Viewer { path: "/p/a.rs".into(), mode: ViewMode::default(), presenting: false });
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
}
