//! The friendly empty pane (spec 4): a greeting, the bindings you want next, and a slow,
//! low-contrast ambient animation: three dots that keep drifting along slow arcs for as long as
//! the pane is shown (the user asked for them never to stop). Reduce Motion holds them still.

use crate::motion::reduce_motion;
use crate::theme::Theme;
use crate::tree::PaneId;
use crate::Root;
use gpui::{div, prelude::*, px, Context, Div, FocusHandle, Window};
use std::time::Instant;

/// How far each dot wanders from its rest position, as a fraction of the pane.
pub const DRIFT: f32 = 0.06;

/// The four things a person wants next from an empty pane.
pub const BINDINGS: &[(&str, &str)] = &[
    ("Ctrl+Shift+`", "new terminal here"),
    ("Cmd+\\", "split right"),
    ("Cmd+1 to Cmd+9", "focus a pane"),
    ("Cmd+W", "close this pane"),
];

/// The hints to show. The last pane never closes, so it does not offer Cmd+W.
pub fn hints(last_pane: bool) -> Vec<(&'static str, &'static str)> {
    BINDINGS.iter().copied().filter(|(keys, _)| !(last_pane && *keys == "Cmd+W")).collect()
}

/// Three dots drifting along slow arcs around their rest positions: periods of ten to twenty
/// seconds, so the motion reads as ambient rather than busy. `None` puts them at rest (Reduce
/// Motion). Time-based, so a dropped frame changes nothing about where they are.
pub fn dot_positions(elapsed: Option<f32>, w: f32, h: f32) -> [(f32, f32); 3] {
    let rest = [(0.3, 0.35), (0.5, 0.3), (0.7, 0.35)];
    let mut out = [(0.0, 0.0); 3];
    for (i, (rx, ry)) in rest.iter().enumerate() {
        let phase = i as f32 * 2.1;
        let (dx, dy) = match elapsed {
            Some(t) => ((t * 0.5 + phase).sin() * DRIFT, (t * 0.35 + phase).cos() * DRIFT),
            None => (0.0, 0.0),
        };
        out[i] = (((rx + dx) * w * 100.0).round() / 100.0, ((ry + dy) * h * 100.0).round() / 100.0);
    }
    out
}

pub fn render(root: &mut Root, pane: PaneId, w: f32, h: f32, focus: FocusHandle, theme: &Theme, window: &mut Window, _cx: &mut Context<Root>) -> Div {
    let started = *root.empty_anim.entry(pane).or_insert_with(Instant::now);
    let elapsed = (!reduce_motion()).then(|| started.elapsed().as_secs_f32());
    if elapsed.is_some() {
        window.request_animation_frame();
    }
    let dots = dot_positions(elapsed, w, h);
    let last_pane = root.tree.panes().len() == 1;
    let mut view = div()
        .id(("empty", pane))
        .size_full()
        .relative()
        .track_focus(&focus)
;
    for (i, (x, y)) in dots.iter().enumerate() {
        let size = 18.0 + 4.0 * i as f32;
        view = view.child(div().absolute().left(px(x - size / 2.0)).top(px(y - size / 2.0)).size(px(size)).rounded_full().bg(theme.semantic.working.alpha(0.2)));
    }
    let mut hints = div().flex().flex_col().gap(px(6.0)).text_color(theme.chrome.text_dim.hsla()).font_family(root.ui_family()).text_size(px(crate::effect::ui(13.0)));
    for (keys, what) in self::hints(last_pane) {
        hints = hints.child(div().flex().flex_row().gap(px(12.0)).child(div().w(px(150.0)).text_color(theme.chrome.text.hsla()).child(keys)).child(what));
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
            .child(div().text_color(theme.chrome.text.hsla()).font_family(root.ui_family()).text_size(px(crate::effect::ui(15.0))).child("This pane is empty. Nothing is lost."))
            .child(hints),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_pane_does_not_offer_to_close() {
        assert_eq!(hints(false).len(), 4);
        assert!(hints(false).iter().any(|(k, _)| *k == "Cmd+W"));
        assert_eq!(hints(true).len(), 3);
        assert!(!hints(true).iter().any(|(k, _)| *k == "Cmd+W"));
    }

    #[test]
    fn dots_keep_drifting_and_hold_still_under_reduce_motion() {
        let at = |t: f32| dot_positions(Some(t), 400.0, 300.0);
        assert_ne!(at(0.0), at(1.5), "the dots move");
        assert_ne!(at(20.0), at(40.0), "and keep moving long after appearance");
        for t in [0.0, 7.0, 33.0, 600.0] {
            for (i, (x, y)) in at(t).iter().enumerate() {
                let (rx, ry) = [(0.3, 0.35), (0.5, 0.3), (0.7, 0.35)][i];
                assert!((x - rx * 400.0).abs() <= DRIFT * 400.0 + 0.01 && (y - ry * 300.0).abs() <= DRIFT * 300.0 + 0.01, "each dot stays near its rest spot at t={t}");
            }
        }
        let rest = dot_positions(None, 400.0, 300.0);
        assert_eq!(rest, [(120.0, 105.0), (200.0, 90.0), (280.0, 105.0)], "Reduce Motion: the rest positions, at any time");
    }
}
