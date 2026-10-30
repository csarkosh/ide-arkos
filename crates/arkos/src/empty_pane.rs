//! The friendly empty pane (spec 4): a greeting, the bindings you want next, and a slow,
//! low-contrast ambient animation: three dots that keep drifting along slow arcs for as long as
//! the pane is shown (the user asked for them never to stop). Reduce Motion holds them still, and
//! so does a window whose effects are paused while it is not in focus (`Root::effects_paused`),
//! where they hold where they were.

use crate::effects::active::Clock;
use crate::motion::reduce_motion;
use crate::theme::Theme;
use crate::tree::PaneId;
use crate::Root;
use gpui::{div, prelude::*, px, Context, Div, FocusHandle, Window};
use std::time::{Duration, Instant};

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

/// Whether the dots ask for another frame: only while they move, neither held by Reduce Motion
/// nor by a `paused` window (one whose effects are paused while it is not in focus).
pub fn asks_for_frames(reduce: bool, paused: bool) -> bool {
    !reduce && !paused
}

/// The seconds the dots are placed at, from the time their clock has run: `None`, at rest,
/// under Reduce Motion. A paused clock holds, so a paused window's dots stay where they were.
pub fn dot_time(reduce: bool, elapsed: Duration) -> Option<f32> {
    (!reduce).then(|| elapsed.as_secs_f32())
}

pub fn render(root: &mut Root, pane: PaneId, w: f32, h: f32, focus: FocusHandle, theme: &Theme, window: &mut Window, _cx: &mut Context<Root>) -> Div {
    let now = Instant::now();
    let paused = root.effects_paused;
    let clock = *root.empty_anim.entry(pane).or_insert_with(|| Clock::starting(now, paused));
    let reduce = reduce_motion();
    let elapsed = dot_time(reduce, clock.elapsed(now));
    if asks_for_frames(reduce, paused) {
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
    let mut hints = div().flex().flex_col().gap(px(6.0)).text_color(theme.chrome.text_dim.hsla()).font_family(root.ui_family()).text_size(px(root.ui_size(13.0)));
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
            .child(div().text_color(theme.chrome.text.hsla()).font_family(root.ui_family()).text_size(px(root.ui_size(15.0))).child("This pane is empty. Nothing is lost."))
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

    #[test]
    fn a_paused_window_s_dots_hold_where_they_are_and_ask_for_no_frames() {
        assert!(asks_for_frames(false, false), "running: a frame after each");
        assert!(!asks_for_frames(false, true), "paused: none");
        assert!(!asks_for_frames(true, false), "Reduce Motion: none");
        // Paused, the dots are where their frozen clock leaves them, not at their rest spots.
        assert_eq!(dot_time(false, std::time::Duration::from_millis(7500)), Some(7.5));
        assert_eq!(dot_time(true, std::time::Duration::from_millis(7500)), None, "Reduce Motion holds them at rest");
        assert_ne!(dot_positions(dot_time(false, std::time::Duration::from_millis(7500)), 400.0, 300.0), dot_positions(None, 400.0, 300.0));
    }
}
