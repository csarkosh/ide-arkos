//! The friendly empty pane (spec 4): a greeting, the bindings you want next, and a slow,
//! low-contrast ambient animation that plays on appearance and on hover, then settles so an idle
//! empty pane costs no frames.

use crate::motion::reduce_motion;
use crate::tree::PaneId;
use crate::Root;
use gpui::{div, prelude::*, px, rgb, rgba, Context, Div, FocusHandle, Window};
use std::time::Instant;

pub const ANIM_SECS: f32 = 6.0;
const FG: u32 = 0xd6dae0;
const DIM: u32 = 0x8a93a3;
const DOT: u32 = 0x3a7bd5;

/// The four things a person wants next from an empty pane.
pub const BINDINGS: &[(&str, &str)] = &[
    ("Ctrl+Shift+`", "new terminal here"),
    ("Cmd+\\", "split right"),
    ("Cmd+1 to Cmd+9", "focus a pane"),
    ("Cmd+W", "close this pane"),
];

/// Three dots that drift along slow arcs and ease into their rest positions by `ANIM_SECS`.
/// The curves are time-based, so a dropped frame changes nothing about where they end.
pub fn dot_positions(elapsed: f32, w: f32, h: f32) -> [(f32, f32); 3] {
    let t = (elapsed / ANIM_SECS).clamp(0.0, 1.0);
    // Ease-out: fast at first, settling smoothly.
    let e = 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t);
    let rest = [(0.3, 0.35), (0.5, 0.3), (0.7, 0.35)];
    let mut out = [(0.0, 0.0); 3];
    for (i, (rx, ry)) in rest.iter().enumerate() {
        let phase = i as f32 * 2.1;
        let drift = (1.0 - e) * 0.08;
        let x = rx + drift * (t * 3.0 + phase).sin();
        let y = ry + drift * (t * 2.0 + phase).cos();
        out[i] = ((x * w * 100.0).round() / 100.0, (y * h * 100.0).round() / 100.0);
    }
    out
}

pub fn render(root: &mut Root, pane: PaneId, w: f32, h: f32, focus: FocusHandle, window: &mut Window, cx: &mut Context<Root>) -> Div {
    let started = *root.empty_anim.entry(pane).or_insert_with(Instant::now);
    let elapsed = if reduce_motion() { ANIM_SECS } else { started.elapsed().as_secs_f32() };
    let animating = elapsed < ANIM_SECS;
    if animating {
        window.request_animation_frame();
    }
    let dots = dot_positions(elapsed, w, h);
    let mut view = div()
        .id(("empty", pane))
        .size_full()
        .relative()
        .track_focus(&focus)
        .on_hover(cx.listener(move |root, hovered: &bool, _window, cx| {
            if *hovered && !reduce_motion() {
                root.empty_anim.insert(pane, Instant::now());
                cx.notify();
            }
        }));
    for (i, (x, y)) in dots.iter().enumerate() {
        let size = 18.0 + 4.0 * i as f32;
        view = view.child(div().absolute().left(px(x - size / 2.0)).top(px(y - size / 2.0)).size(px(size)).rounded_full().bg(rgba((DOT << 8) | 0x33)));
    }
    let mut hints = div().flex().flex_col().gap(px(6.0)).text_color(rgb(DIM)).font_family("Menlo").text_size(px(13.0));
    for (keys, what) in BINDINGS {
        hints = hints.child(div().flex().flex_row().gap(px(12.0)).child(div().w(px(150.0)).text_color(rgb(FG)).child(*keys)).child(*what));
    }
    div().size_full().child(view.child(
        div()
            .absolute()
            .left_0()
            .top(px(h * 0.5))
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(14.0))
            .child(div().text_color(rgb(FG)).font_family("Menlo").text_size(px(15.0)).child("This pane is empty. Nothing is lost."))
            .child(hints),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_move_then_settle() {
        let a = dot_positions(0.0, 400.0, 300.0);
        let b = dot_positions(1.5, 400.0, 300.0);
        assert_ne!(a, b, "the animation moves");
        let end = dot_positions(ANIM_SECS, 400.0, 300.0);
        let later = dot_positions(ANIM_SECS + 10.0, 400.0, 300.0);
        assert_eq!(end, later, "after ANIM_SECS the frame is static");
        for (x, y) in end {
            assert!((0.0..=400.0).contains(&x) && (0.0..=300.0).contains(&y));
        }
    }
}
