//! Renders the layout tree: absolutely positioned panes and the resize handles between them
//! (spec 4: a 2 px gap with a wider hit area and a stripe under the pointer, live ratio while
//! dragging, double-click resets).

use crate::pane_view::render_pane;
use crate::tree::{Orientation, Rect, SplitId, HANDLE};
use crate::Root;
use gpui::{div, prelude::*, px, Context, Div, MouseButton, MouseDownEvent, Stateful, Window};

/// The hit area around the 2 px gap.
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
    // A handle not drawn this frame (its pane maximized or closed) gets no leave event: forget
    // its hover, or its stripe would come up on its own when it is drawn again.
    root.handle_hovered.retain(|s| layout.handles.iter().any(|h| h.0 == *s));
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
        container = container.child(render_handle(root, *split, *orientation, local, *area, cx));
    }
    container
}

/// The stripe shown while the pointer is over a handle or drags it: wider than the gap, over the
/// neighbours' edges, so it is seen without a pane moving.
pub const HANDLE_STRIPE: f32 = 6.0;

/// `rect` is relative to the tree's container; `area` stays in window pixels, like the pointer.
/// At rest the gap shows nothing but the window background between the two pane borders, so
/// the focus glow meets the same hairline on every side; the stripe fades in on hover and stays
/// while dragging.
fn render_handle(root: &mut Root, split: SplitId, orientation: Orientation, rect: Rect, area: Rect, cx: &mut Context<Root>) -> Stateful<Div> {
    let pad = (HANDLE_HIT - HANDLE) / 2.0;
    let (x, y, w, h) = match orientation {
        Orientation::Horizontal => (rect.x - pad, rect.y, HANDLE_HIT, rect.h),
        Orientation::Vertical => (rect.x, rect.y - pad, rect.w, HANDLE_HIT),
    };
    let dragging = root.resizing.is_some_and(|(s, _, _)| s == split);
    let want = (dragging || root.handle_hovered.contains(&split)).then_some(());
    let colour = root.theme.chrome.split_handle_hover.hsla();
    let now = root.frame_now;
    let stripe = root.handle_fades.sync(split, want, now).map(|(_, opacity)| {
        let inset = (HANDLE_HIT - HANDLE_STRIPE) / 2.0;
        let d = match orientation {
            Orientation::Horizontal => div().absolute().left(px(inset)).top_0().w(px(HANDLE_STRIPE)).h_full(),
            Orientation::Vertical => div().absolute().top(px(inset)).left_0().h(px(HANDLE_STRIPE)).w_full(),
        };
        d.opacity(opacity).bg(colour)
    });
    let handle = div()
        .id(("handle", split))
        .absolute()
        .left(px(x))
        .top(px(y))
        .w(px(w))
        .h(px(h))
        .children(stripe)
        .on_hover(cx.listener(move |root, hovered: &bool, _window, cx| {
            if *hovered {
                root.handle_hovered.insert(split);
            } else {
                root.handle_hovered.remove(&split);
            }
            cx.notify();
        }))
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
        // The area holds the two panes and the gap between them.
        let area = Rect { x: 100.0, y: 0.0, w: 400.0 + HANDLE, h: 300.0 };
        assert!((ratio_from_pointer(area, Orientation::Horizontal, 300.0, 0.0) - 0.5).abs() < 1e-3);
        assert!((ratio_from_pointer(area, Orientation::Horizontal, 100.0, 0.0) - 0.0).abs() < 1e-3);
        // Past either edge clamps instead of inverting the split.
        assert_eq!(ratio_from_pointer(area, Orientation::Horizontal, 9999.0, 0.0), 1.0);
        let tall = Rect { x: 0.0, y: 50.0, w: 100.0, h: 200.0 + HANDLE };
        assert!((ratio_from_pointer(tall, Orientation::Vertical, 0.0, 100.0) - 0.25).abs() < 1e-3);
    }
}
