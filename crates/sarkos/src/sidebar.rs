//! The left sidebar (spec 6): a fixed column, Cmd+B to toggle, resizable by its right edge,
//! holding the Explorer section (and Worktrees in milestone 4b). Its state is saved with the
//! workspace; the config's `[sidebar]` gives a new folder its defaults.

use crate::theme::Theme;
use crate::Root;
use gpui::{div, prelude::*, px, Context, Div, FocusHandle, MouseButton, MouseDownEvent, ScrollHandle};

pub const MIN_W: f32 = 160.0;
pub const MAX_W: f32 = 600.0;
/// The resize handle at the column's right edge, with its invisible hit area.
pub const HANDLE_W: f32 = 4.0;
pub const HANDLE_HIT_W: f32 = 10.0;
pub const ROW_H: f32 = 24.0;
/// How far each tree level is indented; the Explorer's rows read it.
#[allow(dead_code)]
pub const INDENT: f32 = 14.0;

pub struct Sidebar {
    pub width: f32,
    pub collapsed: bool,
    pub show_ignored: bool,
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
