//! Renders the layout tree: absolutely positioned panes and the resize handles between them
//! (spec 4: a 4 px handle with a wider hit area, live ratio while dragging, double-click resets).

use crate::pane_view::render_pane;
use crate::theme::Theme;
use crate::tree::{Orientation, Rect, SplitId, HANDLE};
use crate::Root;
use gpui::{div, prelude::*, px, Context, Div, MouseButton, MouseDownEvent, Stateful, Window};

/// The hit area around a 4 px handle.
pub const HANDLE_HIT: f32 = 8.0;

/// Where the pointer sits along the split, as a ratio in [0, 1].
pub fn ratio_from_pointer(area: Rect, orientation: Orientation, x: f32, y: f32) -> f32 {
    let (pos, len) = match orientation {
        Orientation::Horizontal => (x - area.x, area.w - HANDLE),
        Orientation::Vertical => (y - area.y, area.h - HANDLE),
    };
    if len <= 0.0 {
        return 0.5;
    }
    (pos / len).clamp(0.0, 1.0)
}

/// The panes and handles for the current tree, positioned absolutely inside `root.content`.
/// Layout rects are in window pixels; the container starts at `content`'s origin, so each child
/// is placed relative to it.
pub fn render_tree(root: &mut Root, window: &mut Window, cx: &mut Context<Root>) -> Div {
    let layout = root.layout();
    root.pane_order = root.tree.panes();
    root.forget_closed_panes();
    let (ox, oy) = (root.content.x, root.content.y);
    let theme = root.theme.clone();
    let mut container = div().relative().size_full();
    for (pane, rect) in &layout.panes {
        container = container.child(
            div()
                .absolute()
                .left(px(rect.x - ox))
                .top(px(rect.y - oy))
                .w(px(rect.w))
                .h(px(rect.h))
                .overflow_hidden()
                .child(render_pane(root, *pane, *rect, window, cx)),
        );
    }
    for (split, orientation, rect, area) in &layout.handles {
        let local = Rect { x: rect.x - ox, y: rect.y - oy, ..*rect };
        container = container.child(render_handle(*split, *orientation, local, *area, &theme, cx));
    }
    container
}

/// `rect` is relative to the tree's container; `area` stays in window pixels, like the pointer.
fn render_handle(split: SplitId, orientation: Orientation, rect: Rect, area: Rect, theme: &Theme, cx: &mut Context<Root>) -> Stateful<Div> {
    let pad = (HANDLE_HIT - HANDLE) / 2.0;
    let (x, y, w, h) = match orientation {
        Orientation::Horizontal => (rect.x - pad, rect.y, HANDLE_HIT, rect.h),
        Orientation::Vertical => (rect.x, rect.y - pad, rect.w, HANDLE_HIT),
    };
    let stripe = match orientation {
        Orientation::Horizontal => div().absolute().left(px(pad)).top_0().w(px(HANDLE)).h_full(),
        Orientation::Vertical => div().absolute().top(px(pad)).left_0().h(px(HANDLE)).w_full(),
    };
    let hover = theme.chrome.split_handle_hover;
    let handle = div()
        .id(("handle", split))
        .absolute()
        .left(px(x))
        .top(px(y))
        .w(px(w))
        .h(px(h))
        .group("handle")
        .child(stripe.bg(theme.chrome.split_handle.hsla()).group_hover("handle", move |s| s.bg(hover.hsla())))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |root, ev: &MouseDownEvent, _window, cx| {
                if ev.click_count == 2 {
                    root.tree.reset_ratio(split);
                    root.resizing = None;
                    root.save(cx);
                } else {
                    root.resizing = Some((split, orientation, area));
                }
                cx.notify();
            }),
        );
    match orientation {
        Orientation::Horizontal => handle.cursor_col_resize(),
        Orientation::Vertical => handle.cursor_row_resize(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Orientation;

    #[test]
    fn ratio_follows_the_pointer_inside_the_split() {
        let area = Rect { x: 100.0, y: 0.0, w: 404.0, h: 300.0 };
        assert!((ratio_from_pointer(area, Orientation::Horizontal, 300.0, 0.0) - 0.5).abs() < 1e-3);
        assert!((ratio_from_pointer(area, Orientation::Horizontal, 100.0, 0.0) - 0.0).abs() < 1e-3);
        // Past either edge clamps instead of inverting the split.
        assert_eq!(ratio_from_pointer(area, Orientation::Horizontal, 9999.0, 0.0), 1.0);
        let tall = Rect { x: 0.0, y: 50.0, w: 100.0, h: 204.0 };
        assert!((ratio_from_pointer(tall, Orientation::Vertical, 0.0, 100.0) - 0.25).abs() < 1e-3);
    }
}
