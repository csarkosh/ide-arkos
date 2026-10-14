//! The working glyph's breathing (spec 5): a slow, soft, time-based pulse ticked at a low rate,
//! paused while the window is not active and held still under Reduce Motion (spec 7.1).

use crate::Root;
use gpui::{App, Entity};
use std::time::Duration;

/// One breath.
pub const CYCLE: Duration = Duration::from_secs(2);
/// How often a working tab asks for a repaint: ten frames a second is a few percent of one core
/// for a tab bar, and the eye cannot tell at this speed.
pub const TICK: Duration = Duration::from_millis(100);
/// While nothing pulses: a slow check for a tab that started working.
const IDLE_TICK: Duration = Duration::from_millis(500);

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

/// Repaints the window every `TICK` while a tab is working and the window is active, and
/// looks every `IDLE_TICK` otherwise. One task per window, for its whole life.
pub fn start(root: Entity<Root>, cx: &mut App) {
    cx.spawn(async move |cx| loop {
        // The window is gone when the update fails: the task ends with it.
        let Ok(pulsing) = root.update(cx, |root, cx| {
            let pulsing = root.window_active && !crate::motion::reduce_motion() && root.sessions.any_working();
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
    fn the_pulse_holds_still_under_reduce_motion() {
        for ms in [0, 300, 1000, 1700] {
            assert_eq!(alpha(Duration::from_millis(ms), true), 1.0);
        }
    }
}
