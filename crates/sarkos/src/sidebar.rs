//! The left sidebar (spec 6): a fixed column, Cmd+B to toggle, resizable by its right edge,
//! holding the Explorer section (and Worktrees in milestone 4b). Its state is saved with the
//! workspace; the config's `[sidebar]` gives a new folder its defaults.

use crate::theme::Theme;
use crate::Root;
use gpui::{div, prelude::*, px, App, ClickEvent, Context, Div, FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent, ScrollHandle, Stateful};
use std::path::{Path, PathBuf};

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
    pub scroll: ScrollHandle,
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
        Sidebar { width: clamp_width(width), collapsed, focus: cx.focus_handle(), scroll: ScrollHandle::new() }
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

/// The Explorer's rows: a scrolling column of the visible entries. A click on a directory
/// toggles it; on a file it previews (a double click pins); a right click opens the menu; a
/// file drags out to a terminal.
pub fn render_rows(root: &mut Root, theme: &Theme, cx: &mut Context<Root>) -> Stateful<Div> {
    let family = root.family();
    let mut column = div().id("explorer-rows").flex_1().min_h_0().flex().flex_col().overflow_y_scroll().track_scroll(&root.sidebar.scroll);
    for row in root.explorer.rows() {
        let path = row.path.clone();
        let selected = root.explorer.selected.as_deref() == Some(path.as_path());
        let color = if row.ignored { theme.tree.ignored.hsla() } else { theme.chrome.text.hsla() };
        let chevron = if !row.is_dir {
            " "
        } else if row.loading {
            "\u{2026}"
        } else if row.expanded {
            "\u{25be}"
        } else {
            "\u{25b8}"
        };
        let p_click = path.clone();
        let p_menu = path.clone();
        let p_drag = path.clone();
        let is_dir = row.is_dir;
        column = column.child(
            div()
                .id(("row", hash_of(&path)))
                .flex_none()
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
                        root.open_viewer(&p_click, ev.click_count() >= 2, cx);
                    }
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |root, ev: &MouseDownEvent, _window, cx| {
                        root.context_menu = Some(ContextMenu { path: p_menu.clone(), at: ev.position });
                        // The root's mouse-down closes an open menu; this one just opened.
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .when(!is_dir, |d| {
                    let value = crate::drag::FileDrag { path: p_drag.clone(), position: Default::default(), bg: theme.chrome.banner, fg: theme.chrome.text, family: family.clone() };
                    d.on_drag(value, |d: &crate::drag::FileDrag, position, _window, cx| cx.new(|_| d.clone().at(position)))
                })
                .child(div().flex_none().w(px(12.0)).text_color(theme.chrome.text_dim.hsla()).child(chevron))
                .child(div().min_w_0().truncate().child(row.name.clone())),
        );
    }
    column
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
    fn the_context_menu_lists_the_four_file_actions_in_order() {
        let items = context_menu_items(Path::new("/p/src/a.rs"), Path::new("/p"));
        let labels: Vec<&str> = items.iter().map(|(l, _)| *l).collect();
        assert_eq!(labels, ["Reveal in Finder", "Copy path", "Copy relative path", "Open in default app"]);
        assert_eq!(items[1].1, MenuAction::CopyText("/p/src/a.rs".into()));
        assert_eq!(items[2].1, MenuAction::CopyText("src/a.rs".into()));
        assert_eq!(context_menu_items(Path::new("/elsewhere/f"), Path::new("/p"))[2].1, MenuAction::CopyText("/elsewhere/f".into()), "outside the project the full path");
    }

    #[test]
    fn a_collapsed_sidebar_takes_no_width() {
        assert_eq!(visible_width(300.0, false), 300.0);
        assert_eq!(visible_width(300.0, true), 0.0);
    }
}
