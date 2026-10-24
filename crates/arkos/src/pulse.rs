//! The working glyph's breathing (spec 5): a slow, soft, time-based pulse ticked at a low rate,
//! paused while the window is not active and held still under Reduce Motion (spec 7.1).

use crate::Root;
use gpui::{App, WeakEntity};
use std::time::{Duration, Instant};

/// One breath.
pub const CYCLE: Duration = Duration::from_secs(2);
/// How often the active window repaints while it breathes (the working glyphs, with the focus
/// glow and the icon dots): ten frames a second, which the eye cannot tell at these speeds. A
/// repaint is the whole window, every terminal's visible rows included, which is why it
/// breathes only while something is working (`Breath`).
pub const TICK: Duration = Duration::from_millis(100);
/// While the window rests: a slow check, repainting nothing, for something to start working.
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

/// Whether the window breathes, and since when. Every repaint is the whole window, so the
/// breath runs only while something shown is working: a working tab's glyph breathes, and the
/// focus glow and the icon dots breathe along with it. When the work stops, the breath runs on
/// to the end of the glow's current cycle, so the glow comes to rest instead of jumping there,
/// and then the window repaints nothing until something changes.
#[derive(Debug, Clone, Copy, Default)]
pub struct Breath {
    /// When this breath began; `None` at rest.
    since: Option<Instant>,
    /// Set once the work has stopped: where the glow's current cycle ends.
    until: Option<Instant>,
}

impl Breath {
    /// One tick at `now`, with `wanted` saying whether anything shown is working. Returns
    /// whether to repaint for it.
    pub fn tick(&mut self, wanted: bool, now: Instant) -> bool {
        let Some(since) = self.since else {
            if wanted {
                self.since = Some(now);
            }
            return wanted;
        };
        if wanted {
            self.until = None;
            return true;
        }
        let until = *self.until.get_or_insert_with(|| {
            let cycle = GLOW_CYCLE.as_secs_f64();
            let cycles = (now.saturating_duration_since(since).as_secs_f64() / cycle).ceil();
            since + Duration::from_secs_f64(cycles * cycle)
        });
        if now >= until {
            self.stop();
        }
        // The last tick repaints too, so the glow lands at rest.
        true
    }

    /// Rests at once: the window is not in front, or Reduce Motion is on.
    pub fn stop(&mut self) {
        *self = Breath::default();
    }

    pub fn is_breathing(&self) -> bool {
        self.since.is_some()
    }

    /// The focus glow's breath at `now` (`glow`), 0 at rest.
    pub fn glow(&self, now: Instant, reduce_motion: bool) -> f32 {
        match self.since {
            Some(since) => glow(now.saturating_duration_since(since), reduce_motion),
            None => 0.0,
        }
    }

    /// The working glyph's opacity at `now` (`alpha`), full at rest. The breath starts at the
    /// top of the glyph's pulse, so a glyph already showing does not dim with a jump.
    pub fn alpha(&self, now: Instant, reduce_motion: bool) -> f32 {
        match self.since {
            Some(since) => alpha(now.saturating_duration_since(since) + CYCLE / 2, reduce_motion),
            None => 1.0,
        }
    }
}

/// Every `TICK` while the window is active and something it shows is working, repaints it for
/// the breath (`Breath`); otherwise looks every `IDLE_TICK` and repaints nothing. One task per
/// window, for its whole life.
pub fn start(root: WeakEntity<Root>, cx: &mut App) {
    cx.spawn(async move |cx| loop {
        // The window is gone when the update fails: the task ends with it.
        let Ok(breathing) = root.update(cx, |root, cx| {
            if !root.window_active || crate::motion::reduce_motion() {
                root.breath.stop();
                return false;
            }
            let wanted = root.sessions.any_working(&root.tree);
            if root.breath.tick(wanted, Instant::now()) {
                cx.notify();
            }
            root.breath.is_breathing()
        }) else {
            return;
        };
        cx.background_executor().timer(if breathing { TICK } else { IDLE_TICK }).await;
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

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    #[test]
    fn an_idle_window_does_not_breathe_or_repaint() {
        let t0 = Instant::now();
        let mut b = Breath::default();
        assert!(!b.tick(false, t0), "nothing working: no repaint");
        assert!(!b.tick(false, at(t0, 5000)));
        assert!(!b.is_breathing());
        assert_eq!(b.glow(at(t0, 3000), false), 0.0, "the glow rests");
        assert_eq!(b.alpha(at(t0, 3000), false), 1.0, "a glyph at rest is at full");
    }

    #[test]
    fn a_working_tab_starts_the_breath_from_rest() {
        let t0 = Instant::now();
        let mut b = Breath::default();
        assert!(b.tick(true, t0));
        assert!(b.is_breathing());
        assert!(b.glow(t0, false).abs() < 1e-4, "the glow starts at rest, with no jump");
        assert!((b.alpha(t0, false) - 1.0).abs() < 1e-4, "the glyph starts at full, with no jump");
        assert!((b.glow(at(t0, 3000), false) - 1.0).abs() < 1e-4, "and peaks half a breath later");
        assert!(b.tick(true, at(t0, 100)));
    }

    #[test]
    fn when_work_stops_the_breath_finishes_at_rest_and_then_repainting_stops() {
        let t0 = Instant::now();
        let mut b = Breath::default();
        b.tick(true, t0);
        assert!(b.tick(false, at(t0, 1000)), "mid-breath: keeps going");
        assert!(b.tick(false, at(t0, 5900)), "still easing back to rest");
        assert!(b.glow(at(t0, 5900), false) > 0.0);
        assert!(b.tick(false, at(t0, 6000)), "one last repaint at rest");
        assert!(!b.is_breathing());
        assert_eq!(b.glow(at(t0, 6050), false), 0.0);
        assert!(!b.tick(false, at(t0, 6100)), "then nothing");
    }

    #[test]
    fn work_resuming_while_the_breath_winds_down_keeps_its_phase() {
        let t0 = Instant::now();
        let mut b = Breath::default();
        b.tick(true, t0);
        b.tick(false, at(t0, 1000));
        assert!(b.tick(true, at(t0, 2000)));
        assert!((b.glow(at(t0, 2000), false) - glow(Duration::from_secs(2), false)).abs() < 1e-4, "no jump");
        assert!(b.tick(false, at(t0, 7000)), "the wind-down now runs to the end of this breath");
        assert!(b.tick(false, at(t0, 11900)));
        assert!(b.tick(false, at(t0, 12000)));
        assert!(!b.is_breathing());
    }

    #[test]
    fn stopping_rests_the_breath_at_once() {
        let t0 = Instant::now();
        let mut b = Breath::default();
        b.tick(true, t0);
        b.stop();
        assert!(!b.is_breathing());
        assert_eq!(b.glow(at(t0, 1500), false), 0.0);
        assert!(!b.tick(false, at(t0, 1600)));
    }

    #[test]
    fn a_breath_holds_still_under_reduce_motion() {
        let t0 = Instant::now();
        let mut b = Breath::default();
        b.tick(true, t0);
        assert_eq!(b.glow(at(t0, 3000), true), 0.0);
        assert_eq!(b.alpha(at(t0, 700), true), 1.0);
    }

    #[test]
    fn the_pulse_holds_still_under_reduce_motion() {
        for ms in [0, 300, 1000, 1700] {
            assert_eq!(alpha(Duration::from_millis(ms), true), 1.0);
        }
    }
}
