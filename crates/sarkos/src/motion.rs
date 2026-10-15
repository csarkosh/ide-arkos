//! Honours macOS's Reduce Motion setting (spec 7.1: the UI adds no motion the user turned off).
//! Read through `defaults`, which needs no extra dependency, at launch and on every config
//! reload (`refresh` runs on the background executor; it spawns a process). The transitions of
//! transient chrome live here too: their durations, curves and holders, all on `Instant`.
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static REDUCE: AtomicBool = AtomicBool::new(false);

pub fn reduce_motion() -> bool {
    REDUCE.load(Ordering::Relaxed)
}

fn set(on: bool) {
    REDUCE.store(on, Ordering::Relaxed);
}

/// Re-reads the setting. Blocks on `defaults` for a few milliseconds.
pub fn refresh() {
    let out = std::process::Command::new("defaults").args(["read", "com.apple.universalaccess", "reduceMotion"]).output();
    set(match out {
        Ok(o) => parse_reduce_motion(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => false,
    });
}

/// `defaults` prints `1` when the setting is on, `0` or nothing (an error on stderr) when off.
pub fn parse_reduce_motion(stdout: &str) -> bool {
    stdout.trim() == "1"
}

/// How long chrome takes to appear (menus, the palette, notices, find bars, the drop preview).
pub const ENTER: Duration = Duration::from_millis(120);
/// How long it takes to go: a little quicker than it came.
pub const EXIT: Duration = Duration::from_millis(90);
/// How long Cmd+B takes to slide the sidebar in or out.
pub const SIDEBAR_SLIDE: Duration = Duration::from_millis(150);

/// How far `now` is through `dur` from `since`, 0 to 1.
fn fraction(since: Instant, now: Instant, dur: Duration) -> f32 {
    if dur.is_zero() {
        return 1.0;
    }
    (now.saturating_duration_since(since).as_secs_f32() / dur.as_secs_f32()).clamp(0.0, 1.0)
}

/// 0 to 1 over `dur` from `since`, easing out (fast, then settling). 1 at once under Reduce Motion.
pub fn enter_progress(since: Instant, now: Instant, dur: Duration) -> f32 {
    if reduce_motion() {
        return 1.0;
    }
    let t = fraction(since, now, dur);
    1.0 - (1.0 - t) * (1.0 - t)
}

/// 1 to 0 over `dur` from `since`, easing in (slow, then gone). 0 at once under Reduce Motion.
pub fn exit_progress(since: Instant, now: Instant, dur: Duration) -> f32 {
    if reduce_motion() {
        return 0.0;
    }
    let t = fraction(since, now, dur);
    (1.0 - t) * (1.0 - t)
}

/// How far above its resting place a menu starts as it enters, and a panel (the palette, a find
/// bar, the worktree prompt).
pub const MENU_LIFT: f32 = 4.0;
pub const PANEL_LIFT: f32 = 6.0;

/// How far above its resting place something at `opacity` is drawn: `distance` while invisible,
/// none once fully shown, so it settles downwards as it fades in and rises as it fades out.
pub fn lift(opacity: f32, distance: f32) -> f32 {
    (1.0 - opacity.clamp(0.0, 1.0)) * distance
}

/// `now` moved back by `elapsed`, so a curve started then is at the same point now.
fn back(now: Instant, elapsed: Duration) -> Instant {
    now.checked_sub(elapsed).unwrap_or(now)
}

/// Something that is shown, or still fading out after being hidden: the holder keeps the last
/// value mounted until the exit finishes, so callers never special-case the fade-out. Logic
/// reads `value()` (gone the moment it is hidden); drawing reads `current()` (the value and its
/// opacity, until the exit finishes).
pub enum Transient<T> {
    Hidden,
    Showing { value: T, since: Instant },
    Hiding { value: T, since: Instant },
}

impl<T> Default for Transient<T> {
    fn default() -> Self {
        Transient::Hidden
    }
}

impl<T> Transient<T> {
    /// Already shown and settled: what was open before the window first drew.
    pub fn shown(value: T, now: Instant) -> Self {
        Transient::Showing { value, since: back(now, ENTER) }
    }

    /// Shows `next`, or hides with `None`. From hidden, the enter starts; while shown, a new
    /// value replaces the old without entering again; while fading out, the enter resumes at the
    /// opacity on screen, so nothing jumps. Hiding a shown value starts the exit at its opacity.
    pub fn set(&mut self, next: Option<T>, now: Instant) {
        let state = std::mem::take(self);
        *self = match (state, next) {
            (Transient::Hidden, Some(value)) => Transient::Showing { value, since: now },
            (Transient::Showing { since, .. }, Some(value)) => Transient::Showing { value, since },
            (Transient::Hiding { since, .. }, Some(value)) => {
                let o = exit_progress(since, now, EXIT);
                // The enter's progress p = 1 - (1 - t)^2 equals the opacity at t = 1 - sqrt(1 - o).
                let t = 1.0 - (1.0 - o).max(0.0).sqrt();
                Transient::Showing { value, since: back(now, ENTER.mul_f32(t)) }
            }
            (Transient::Showing { value, since }, None) => {
                let p = enter_progress(since, now, ENTER);
                // The exit's opacity (1 - t)^2 equals p at t = 1 - sqrt(p).
                let t = 1.0 - p.max(0.0).sqrt();
                Transient::Hiding { value, since: back(now, EXIT.mul_f32(t)) }
            }
            (state, None) => state,
        };
    }

    pub fn show(&mut self, value: T, now: Instant) {
        self.set(Some(value), now);
    }

    /// Starts the exit. True when something was shown, as `Option::take().is_some()` said.
    pub fn hide(&mut self, now: Instant) -> bool {
        let shown = self.value().is_some();
        self.set(None, now);
        shown
    }

    /// Gone at once, with no exit: for a value that cannot be drawn any more.
    pub fn clear(&mut self) {
        *self = Transient::Hidden;
    }

    /// The value to draw and its opacity, 0 to 1; `None` once an exit finished.
    pub fn current(&self, now: Instant) -> Option<(&T, f32)> {
        match self {
            Transient::Hidden => None,
            Transient::Showing { value, since } => Some((value, enter_progress(*since, now, ENTER))),
            Transient::Hiding { value, since } => {
                let o = exit_progress(*since, now, EXIT);
                (o > 0.0).then_some((value, o))
            }
        }
    }

    /// The logical value: `None` while fading out.
    pub fn value(&self) -> Option<&T> {
        match self {
            Transient::Showing { value, .. } => Some(value),
            _ => None,
        }
    }

    pub fn value_mut(&mut self) -> Option<&mut T> {
        match self {
            Transient::Showing { value, .. } => Some(value),
            _ => None,
        }
    }

    /// Whether the next frame looks different: an enter or an exit under way.
    pub fn animating(&self, now: Instant) -> bool {
        if reduce_motion() {
            return false;
        }
        match self {
            Transient::Hidden => false,
            Transient::Showing { since, .. } => now.saturating_duration_since(*since) < ENTER,
            Transient::Hiding { since, .. } => now.saturating_duration_since(*since) < EXIT,
        }
    }

    /// Drops a value whose exit finished.
    pub fn settle(&mut self, now: Instant) {
        if matches!(self, Transient::Hiding { .. }) && self.current(now).is_none() {
            *self = Transient::Hidden;
        }
    }

    pub fn is_hidden(&self) -> bool {
        matches!(self, Transient::Hidden)
    }
}

/// A width easing from one value to another over `SIDEBAR_SLIDE` (Cmd+B). Turned around
/// mid-way, the next slide starts from `at(now)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slide {
    pub from: f32,
    pub to: f32,
    pub since: Instant,
}

impl Slide {
    pub fn new(from: f32, to: f32, since: Instant) -> Self {
        Slide { from, to, since }
    }

    /// Where the slide is at `now`; `to` at once under Reduce Motion.
    pub fn at(&self, now: Instant) -> f32 {
        let p = enter_progress(self.since, now, SIDEBAR_SLIDE);
        if p >= 1.0 {
            return self.to;
        }
        self.from + (self.to - self.from) * p
    }

    pub fn animating(&self, now: Instant) -> bool {
        !reduce_motion() && now.saturating_duration_since(self.since) < SIDEBAR_SLIDE
    }
}

/// Serialises the tests that read or flip the Reduce Motion flag, here and in other modules:
/// it is one process-wide atomic, and the test harness runs tests on several threads.
#[cfg(test)]
pub fn test_lock(reduce: bool) -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set(reduce);
    guard
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn parses_defaults_output_and_refresh_sets_the_flag() {
        let _g = test_lock(false);
        assert!(parse_reduce_motion("1\n"));
        assert!(!parse_reduce_motion("0\n"));
        assert!(!parse_reduce_motion(""));
        set(true);
        assert!(reduce_motion());
        set(false);
        assert!(!reduce_motion());
    }

    #[test]
    fn the_enter_eases_out_and_the_exit_eases_in() {
        let _g = test_lock(false);
        let t0 = Instant::now();
        assert_eq!(enter_progress(t0, t0, ENTER), 0.0);
        assert!(close(enter_progress(t0, t0 + ENTER / 2, ENTER), 0.75), "ease-out: three quarters at half time");
        assert_eq!(enter_progress(t0, t0 + ENTER, ENTER), 1.0);
        assert_eq!(enter_progress(t0, t0 + ENTER * 4, ENTER), 1.0, "settled");
        assert_eq!(exit_progress(t0, t0, EXIT), 1.0);
        assert!(close(exit_progress(t0, t0 + EXIT / 2, EXIT), 0.25), "ease-in: a quarter left at half time");
        assert_eq!(exit_progress(t0, t0 + EXIT, EXIT), 0.0);
        assert_eq!(exit_progress(t0, t0 + EXIT * 4, EXIT), 0.0);
        assert_eq!(enter_progress(t0 + MS(5), t0, ENTER), 0.0, "an instant from the future never panics");
    }

    #[test]
    fn a_transient_enters_holds_and_exits_with_its_last_value() {
        let _g = test_lock(false);
        let t0 = Instant::now();
        let mut h: Transient<u32> = Transient::default();
        assert!(h.current(t0).is_none() && h.value().is_none() && !h.animating(t0));
        h.set(Some(7), t0);
        assert_eq!(h.value(), Some(&7));
        assert_eq!(h.current(t0), Some((&7, 0.0)));
        assert!(h.animating(t0 + ENTER / 2));
        assert_eq!(h.current(t0 + ENTER), Some((&7, 1.0)));
        assert!(!h.animating(t0 + ENTER), "an idle holder asks for no frame");
        // A new value while shown replaces it without entering again.
        h.set(Some(8), t0 + ENTER * 2);
        assert_eq!(h.current(t0 + ENTER * 2), Some((&8, 1.0)));
        let t1 = t0 + ENTER * 3;
        assert!(h.hide(t1), "hiding a shown value says so");
        assert_eq!(h.value(), None, "the logical value goes at once");
        assert_eq!(h.current(t1), Some((&8, 1.0)), "the last value stays mounted while it fades");
        assert!(h.animating(t1 + EXIT / 2));
        assert!(!h.hide(t1 + MS(1)), "a fading value is not hidden twice");
        assert!(close(h.current(t1 + EXIT / 2).unwrap().1, 0.25));
        assert!(h.current(t1 + EXIT).is_none(), "gone once the exit finished");
        assert!(!h.animating(t1 + EXIT));
        h.settle(t1 + EXIT);
        assert!(matches!(h, Transient::Hidden));
    }

    #[test]
    fn reversing_mid_way_resumes_from_the_visible_opacity() {
        let _g = test_lock(false);
        let t0 = Instant::now();
        let mut h = Transient::default();
        h.set(Some('a'), t0);
        let mid = t0 + ENTER / 2;
        let seen = h.current(mid).unwrap().1;
        h.set(None, mid);
        assert!(close(h.current(mid).unwrap().1, seen), "an exit mid-enter starts where the enter was");
        let later = mid + EXIT / 3;
        let fading = h.current(later).unwrap().1;
        assert!(fading < seen);
        h.set(Some('a'), later);
        assert!(close(h.current(later).unwrap().1, fading), "showing again mid-exit does not jump");
        assert!(h.current(later + ENTER).unwrap().1 == 1.0);
        // Showing the same value again while shown keeps the enter's start.
        let mut g = Transient::default();
        g.set(Some(1), t0);
        g.set(Some(1), t0 + ENTER / 2);
        assert!(close(g.current(t0 + ENTER / 2).unwrap().1, 0.75));
        // An exit that already finished enters afresh.
        let mut f = Transient::default();
        f.set(Some(1), t0);
        f.hide(t0 + ENTER);
        f.set(Some(2), t0 + ENTER + EXIT * 2);
        assert_eq!(f.current(t0 + ENTER + EXIT * 2), Some((&2, 0.0)));
    }

    #[test]
    fn value_mut_reaches_only_a_shown_value_and_clear_snaps() {
        let _g = test_lock(false);
        let t0 = Instant::now();
        let mut h = Transient::default();
        h.set(Some(String::from("a")), t0);
        h.value_mut().unwrap().push('b');
        assert_eq!(h.value().map(String::as_str), Some("ab"));
        h.hide(t0);
        assert!(h.value_mut().is_none(), "a fading value is not edited");
        h.clear();
        assert!(h.current(t0).is_none() && !h.animating(t0));
        let s = Transient::shown(5, t0);
        assert_eq!(s.current(t0), Some((&5, 1.0)), "shown starts settled");
        assert!(!s.animating(t0));
    }

    #[test]
    fn reduce_motion_snaps_every_curve_and_holder() {
        let _g = test_lock(true);
        let t0 = Instant::now();
        assert_eq!(enter_progress(t0, t0, ENTER), 1.0);
        assert_eq!(exit_progress(t0, t0, EXIT), 0.0);
        let mut h = Transient::default();
        h.set(Some(1), t0);
        assert_eq!(h.current(t0), Some((&1, 1.0)));
        assert!(!h.animating(t0));
        h.hide(t0);
        assert!(h.current(t0).is_none(), "gone at once");
        assert!(!h.animating(t0));
        let s = Slide::new(300.0, 0.0, t0);
        assert_eq!(s.at(t0), 0.0);
        assert!(!s.animating(t0));
        set(false);
    }

    #[test]
    fn chrome_settles_downwards_as_it_fades_in() {
        assert_eq!(lift(0.0, MENU_LIFT), 4.0);
        assert_eq!(lift(0.5, PANEL_LIFT), 3.0);
        assert_eq!(lift(1.0, PANEL_LIFT), 0.0);
        assert_eq!(lift(1.5, MENU_LIFT), 0.0, "never below its place");
    }

    #[test]
    fn a_slide_eases_from_its_start_to_its_end() {
        let _g = test_lock(false);
        let t0 = Instant::now();
        let s = Slide::new(300.0, 0.0, t0);
        assert_eq!(s.at(t0), 300.0);
        assert!(close(s.at(t0 + SIDEBAR_SLIDE / 2), 75.0), "ease-out: three quarters of the way at half time");
        assert_eq!(s.at(t0 + SIDEBAR_SLIDE), 0.0);
        assert!(s.animating(t0 + SIDEBAR_SLIDE / 2));
        assert!(!s.animating(t0 + SIDEBAR_SLIDE));
        // Turned around mid-way, a slide starts from where the last one was.
        let mid = t0 + SIDEBAR_SLIDE / 2;
        let back = Slide::new(s.at(mid), 300.0, mid);
        assert!(close(back.at(mid), 75.0));
        assert_eq!(back.at(mid + SIDEBAR_SLIDE), 300.0);
    }
}
