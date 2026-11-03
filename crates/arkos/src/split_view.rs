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

/// The area the panes are laid out in, and the gutter's width between it and the sidebar: right
/// of a sidebar `side` points wide, below the window's bars (`top`), in a window `width` by
/// `height`, kept the effect's `gap` off the bars and the window's edges. Beside a shown sidebar
/// the gutter is the effect's `binder`, else its `gap`; with the sidebar hidden there is none and
/// the gap keeps the panes off the window's left edge. A window smaller than its chrome lays out
/// at zero.
pub fn content_area(side: f32, binder: f32, gap: f32, top: f32, width: f32, height: f32) -> (Rect, f32) {
    let shown = side > 1.0;
    let gutter = if !shown { 0.0 } else if binder > 0.0 { binder } else { gap };
    let x = if shown { side + gutter } else { side + gap };
    let y = top + gap;
    (Rect { x, y, w: (width - x - gap).max(0.0), h: (height - y - gap).max(0.0) }, gutter)
}

/// The panes as drawn with `gap` points between them: each side that faces another pane (not on
/// `area`'s edge) moves in by half of what the gap adds to the layout's own handle, so the
/// handles keep their places and the panes' order and neighbours stay the layout's.
pub fn spaced(panes: &[(crate::tree::PaneId, Rect)], area: Rect, gap: f32) -> Vec<(crate::tree::PaneId, Rect)> {
    let half = ((gap - HANDLE) / 2.0).max(0.0);
    if half == 0.0 {
        return panes.to_vec();
    }
    let inner = |at: f32, edge: f32| if (at - edge).abs() > 0.5 { half } else { 0.0 };
    panes
        .iter()
        .map(|(id, r)| {
            let (l, t) = (inner(r.x, area.x), inner(r.y, area.y));
            let (rt, b) = (inner(r.x + r.w, area.x + area.w), inner(r.y + r.h, area.y + area.h));
            (*id, Rect { x: r.x + l, y: r.y + t, w: (r.w - l - rt).max(0.0), h: (r.h - t - b).max(0.0) })
        })
        .collect()
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
    fn the_panes_start_past_the_binder_only_while_the_sidebar_shows() {
        let (area, binder) = content_area(240.0, 20.0, 0.0, 28.0, 1200.0, 800.0);
        assert_eq!(area, Rect { x: 260.0, y: 28.0, w: 940.0, h: 772.0 });
        assert_eq!(binder, 20.0);
        let (area, binder) = content_area(0.0, 20.0, 0.0, 28.0, 1200.0, 800.0);
        assert_eq!((area.x, area.w, binder), (0.0, 1200.0, 0.0), "no sidebar, no binder");
        let (area, binder) = content_area(240.0, 0.0, 0.0, 28.0, 1200.0, 800.0);
        assert_eq!((area.x, binder), (240.0, 0.0));
        let (area, _) = content_area(240.0, 20.0, 0.0, 900.0, 200.0, 800.0);
        assert_eq!((area.w, area.h), (0.0, 0.0), "a window smaller than its chrome lays out at zero");
    }

    #[test]
    fn a_gap_keeps_the_panes_off_every_edge_and_is_the_gutter_beside_a_sidebar_without_a_binder() {
        let (area, left) = content_area(240.0, 0.0, 8.0, 28.0, 1200.0, 800.0);
        assert_eq!(area, Rect { x: 248.0, y: 36.0, w: 944.0, h: 756.0 });
        assert_eq!(left, 8.0, "the gutter beside the sidebar");
        let (area, left) = content_area(0.0, 0.0, 8.0, 28.0, 1200.0, 800.0);
        assert_eq!((area.x, area.w, left), (8.0, 1184.0, 0.0), "no sidebar: off the window's edge");
        let (_, left) = content_area(240.0, 20.0, 8.0, 28.0, 1200.0, 800.0);
        assert_eq!(left, 20.0, "a binder is the gutter where there is one");
    }

    #[test]
    fn a_gap_moves_each_pane_s_inner_sides_apart_and_leaves_its_outer_sides_alone() {
        let area = Rect { x: 0.0, y: 0.0, w: 802.0, h: 600.0 };
        let half = (8.0 - HANDLE) / 2.0;
        let panes = [(1, Rect { x: 0.0, y: 0.0, w: 400.0, h: 600.0 }), (2, Rect { x: 402.0, y: 0.0, w: 400.0, h: 600.0 })];
        let out = spaced(&panes, area, 8.0);
        assert_eq!(out[0], (1, Rect { x: 0.0, y: 0.0, w: 400.0 - half, h: 600.0 }));
        assert_eq!(out[1], (2, Rect { x: 402.0 + half, y: 0.0, w: 400.0 - half, h: 600.0 }));
        assert_eq!(out[1].1.x - (out[0].1.x + out[0].1.w), 8.0, "the gap between them is the gap asked for");
        assert_eq!(spaced(&panes, area, 0.0), panes.to_vec(), "no gap, the layout's own");
        assert_eq!(spaced(&panes, area, HANDLE), panes.to_vec());
    }

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
