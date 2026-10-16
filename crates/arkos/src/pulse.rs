//! The working glyph's breathing (spec 5): a slow, soft, time-based pulse ticked at a low rate,
//! paused while the window is not active and held still under Reduce Motion (spec 7.1).

use crate::Root;
use gpui::{App, WeakEntity};
use std::time::Duration;

/// One breath.
pub const CYCLE: Duration = Duration::from_secs(2);
/// How often the active window repaints for its motion (the focus glow's breath, the working
/// glyphs): ten frames a second, which the eye cannot tell at these speeds. A repaint is the
/// whole window, every terminal's visible rows included, so this is the window's idle cost
/// while it is in front; the idle check in docs/development.md notes the figure.
pub const TICK: Duration = Duration::from_millis(100);
/// While the window is not in front, or Reduce Motion is on: a slow check for either changing.
const IDLE_TICK: Duration = Duration::from_millis(500);

/// The focused pane's glow breathes this slowly: three seconds in, three out.
pub const GLOW_CYCLE: Duration = Duration::from_secs(6);

/// The glyph's opacity at `elapsed` into the pulse: a cosine between 0.35 and 1.0, so it never
/// vanishes. Reduce Motion holds it at full.
pub fn alpha(elapsed: Duration, reduce_motion: bool) -> f32 {
    if reduce_motion {
        return 1.0;
    }
    let t = (elapsed.as_secs_f32() % CYCLE.as_secs_f32()) / CYCLE.as_secs_f32();
    let wave = 0.5 - 0.5 * (t * std::f32::consts::TAU).cos();
    0.35 + 0.65 * wave
}

/// The focus glow's breath at `elapsed`: 0 at rest, 1 at the peak, a cosine over `GLOW_CYCLE`.
/// Reduce Motion holds it at rest.
pub fn glow(elapsed: Duration, reduce_motion: bool) -> f32 {
    if reduce_motion {
        return 0.0;
    }
    let t = (elapsed.as_secs_f32() % GLOW_CYCLE.as_secs_f32()) / GLOW_CYCLE.as_secs_f32();
    0.5 - 0.5 * (t * std::f32::consts::TAU).cos()
}

/// Repaints the window every `TICK` while it is active (the focused pane's glow breathes the
/// whole time, and the working glyphs ride the same ticks), and looks every `IDLE_TICK`
/// otherwise. One task per window, for its whole life.
pub fn start(root: WeakEntity<Root>, cx: &mut App) {
    cx.spawn(async move |cx| loop {
        // The window is gone when the update fails: the task ends with it.
        let Ok(pulsing) = root.update(cx, |root, cx| {
            let pulsing = root.window_active && !crate::motion::reduce_motion();
            if pulsing {
                cx.notify();
            }
            pulsing
        }) else {
            return;
        };
        cx.background_executor().timer(if pulsing { TICK } else { IDLE_TICK }).await;
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pulse_breathes_between_a_third_and_full_over_two_seconds() {
        assert!((alpha(Duration::ZERO, false) - 0.35).abs() < 1e-4);
        assert!((alpha(Duration::from_secs(1), false) - 1.0).abs() < 1e-4);
        assert!((alpha(Duration::from_secs(2), false) - 0.35).abs() < 1e-4, "periodic");
        assert!((alpha(Duration::from_millis(500), false) - 0.675).abs() < 1e-3);
        let a = alpha(Duration::from_millis(100), false);
        let b = alpha(Duration::from_millis(200), false);
        assert!(b > a && b - a < 0.1, "a tick moves it by little");
    }

    #[test]
    fn the_glow_breathes_slowly_from_rest_to_peak() {
        assert!(glow(Duration::ZERO, false).abs() < 1e-4);
        assert!((glow(Duration::from_secs(3), false) - 1.0).abs() < 1e-4);
        assert!(glow(Duration::from_secs(6), false).abs() < 1e-4, "periodic");
        assert!((glow(Duration::from_millis(1500), false) - 0.5).abs() < 1e-3);
        let a = glow(Duration::from_millis(1500), false);
        let b = glow(Duration::from_millis(1600), false);
        assert!(b > a && b - a < 0.06, "a tick moves it by little, even at the steepest point");
        for ms in [0, 1500, 3000] {
            assert_eq!(glow(Duration::from_millis(ms), true), 0.0, "Reduce Motion rests it");
        }
    }

    #[test]
    fn the_pulse_holds_still_under_reduce_motion() {
        for ms in [0, 300, 1000, 1700] {
            assert_eq!(alpha(Duration::from_millis(ms), true), 1.0);
        }
    }
}
