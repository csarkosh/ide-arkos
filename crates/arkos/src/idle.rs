//! When a staged update may apply itself (auto-update spec, section 6): the window has been
//! quiet for `idle_after`, nothing transient is open, nothing is in flight, the daemon is there,
//! and no markdown tab holds edits that could not be saved.
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Chrome {
    pub palette: bool,
    pub menu: bool,
    pub prompt: bool,
    pub find: bool,
    pub drag: bool,
    /// A markdown tab holds edits that are not on disk and will not get there by themselves: a
    /// conflict waits for the user, or the last save failed. An update would drop them.
    pub unsaved: bool,
}

impl Chrome {
    pub fn any(self) -> bool {
        self.palette || self.menu || self.prompt || self.find || self.drag
    }
}

pub fn may_apply(now: Instant, last_input: Instant, idle_after: Duration, chrome: Chrome, in_flight: bool, connected: bool) -> bool {
    if chrome.any() || chrome.unsaved || in_flight || !connected {
        return false;
    }
    now.saturating_duration_since(last_input) >= idle_after
}

/// What an idle window does about updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleAction {
    /// Ask the daemon to swap in the staged release (`apply_staged`).
    Apply,
    /// Another window applied it: the daemon runs the new build and the bundle on disk is new, so
    /// this window relaunches from it (`relaunch_now`).
    Relaunch,
    Nothing,
}

/// `staged`: the daemon has a release staged. `on_disk_newer_daemon`: the binary at the launch
/// path differs from this window and the daemon already runs another build than this window
/// (a dev build whose daemon is its own build never relaunches).
pub fn idle_action(staged: bool, on_disk_newer_daemon: bool) -> IdleAction {
    if staged {
        IdleAction::Apply
    } else if on_disk_newer_daemon {
        IdleAction::Relaunch
    } else {
        IdleAction::Nothing
    }
}

/// When the quiet spell starts: now while anything is open, so closing the palette, a menu or a
/// find bar (whose keys may never reach the window) is not followed by an update a second later.
pub fn quiet_since(last_input: Instant, now: Instant, chrome: Chrome) -> Instant {
    if chrome.any() { now } else { last_input }
}

/// How long the heads-up counts down before an update applies in a window that is in front
/// (update heads-up spec).
pub const COUNTDOWN: Duration = Duration::from_secs(10);

/// How long "Later" puts an update off.
pub const LATER: Duration = Duration::from_secs(60 * 60);

/// What the quiet window does now that an update may apply (`may_apply` holds), in the update
/// heads-up spec's terms (Cyrus, 2026-11-16: "It starts without warning"): a window not in front
/// updates at once, nobody watching it; one in front first counts down in a toast, which typing
/// calls off (the quiet spell starts again) and "Later" puts off for `LATER`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadsUp {
    Apply,
    /// Show the toast, counting down from now.
    Start,
    /// Keep counting (or keep waiting out a "Later").
    Wait,
    /// The user typed during the countdown: the toast goes, the quiet spell starts again.
    CallOff,
}

/// `countdown`: when the toast started, if it shows. `later_until`: a "Later" not yet over.
pub fn heads_up(in_front: bool, countdown: Option<Instant>, later_until: Option<Instant>, last_input: Instant, now: Instant) -> HeadsUp {
    if later_until.is_some_and(|t| now < t) {
        return HeadsUp::Wait;
    }
    match countdown {
        Some(since) if last_input > since => HeadsUp::CallOff,
        Some(since) if now.saturating_duration_since(since) >= COUNTDOWN => HeadsUp::Apply,
        Some(_) if !in_front => HeadsUp::Apply,
        Some(_) => HeadsUp::Wait,
        None if !in_front => HeadsUp::Apply,
        None => HeadsUp::Start,
    }
}

/// The whole seconds the toast says are left, never 0 while it shows.
pub fn seconds_left(since: Instant, now: Instant) -> u64 {
    COUNTDOWN.saturating_sub(now.saturating_duration_since(since)).as_secs_f64().ceil().max(1.0) as u64
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_window_in_front_counts_down_first_and_one_behind_updates_at_once() {
        use super::*;
        let t0 = Instant::now();
        let input = t0 - Duration::from_secs(40);
        assert_eq!(heads_up(false, None, None, input, t0), HeadsUp::Apply, "nobody is looking at it");
        assert_eq!(heads_up(true, None, None, input, t0), HeadsUp::Start, "a heads-up first");
        assert_eq!(heads_up(true, Some(t0), None, input, t0 + Duration::from_secs(3)), HeadsUp::Wait);
        assert_eq!(heads_up(true, Some(t0), None, input, t0 + COUNTDOWN), HeadsUp::Apply);
        assert_eq!(heads_up(false, Some(t0), None, input, t0 + Duration::from_secs(2)), HeadsUp::Apply, "the user went to another app");
        assert_eq!(heads_up(true, Some(t0), None, t0 + Duration::from_secs(1), t0 + Duration::from_secs(2)), HeadsUp::CallOff, "typing calls it off");
        assert_eq!(heads_up(true, None, Some(t0 + LATER), input, t0 + Duration::from_secs(60)), HeadsUp::Wait, "Later holds");
        assert_eq!(heads_up(false, None, Some(t0 + LATER), input, t0 + Duration::from_secs(60)), HeadsUp::Wait, "Later holds behind too");
        assert_eq!(heads_up(true, None, Some(t0 + LATER), input, t0 + LATER), HeadsUp::Start, "after Later, the heads-up again");
    }

    #[test]
    fn the_toast_counts_whole_seconds_down_to_one() {
        use super::*;
        let t0 = Instant::now();
        assert_eq!(seconds_left(t0, t0), 10);
        assert_eq!(seconds_left(t0, t0 + Duration::from_millis(100)), 10);
        assert_eq!(seconds_left(t0, t0 + Duration::from_millis(9_500)), 1);
        assert_eq!(seconds_left(t0, t0 + COUNTDOWN), 1);
    }

    use super::*;

    #[test]
    fn applies_only_after_a_quiet_spell_with_nothing_open() {
        let t0 = Instant::now();
        let after = Duration::from_secs(30);
        let quiet = Chrome::default();
        assert!(!may_apply(t0 + Duration::from_secs(29), t0, after, quiet, false, true), "not yet");
        assert!(may_apply(t0 + Duration::from_secs(30), t0, after, quiet, false, true));
        assert!(may_apply(t0 + Duration::from_secs(300), t0, after, quiet, false, true));
        for open in [Chrome { palette: true, ..quiet }, Chrome { menu: true, ..quiet }, Chrome { prompt: true, ..quiet }, Chrome { find: true, ..quiet }, Chrome { drag: true, ..quiet }] {
            assert!(!may_apply(t0 + Duration::from_secs(60), t0, after, open, false, true), "{open:?}");
        }
        assert!(!may_apply(t0 + Duration::from_secs(60), t0, after, quiet, true, true), "an update already running");
        let unsaved = Chrome { unsaved: true, ..quiet };
        assert!(!may_apply(t0 + Duration::from_secs(600), t0, after, unsaved, false, true), "a conflict or a failed save waits for the user");
        assert!(!unsaved.any(), "unsaved edits are not chrome: they do not restart the quiet spell");
        assert!(!may_apply(t0 + Duration::from_secs(60), t0, after, quiet, false, false), "offline");
        assert!(may_apply(t0, t0 + Duration::from_secs(5), Duration::ZERO, quiet, false, true), "a last input in the future (clock skew) counts as idle only when idle_after is zero");
    }

    #[test]
    fn open_chrome_restarts_the_quiet_spell() {
        let t0 = Instant::now();
        let after = Duration::from_secs(30);
        let now = t0 + Duration::from_secs(60);
        let open = Chrome { find: true, ..Chrome::default() };
        assert_eq!(quiet_since(t0, now, Chrome::default()), t0);
        let since = quiet_since(t0, now, open);
        assert_eq!(since, now, "a find bar open at this tick");
        // Closed on the next tick: the 30 seconds count from the last tick it was open.
        assert!(!may_apply(now + Duration::from_secs(1), since, after, Chrome::default(), false, true));
        assert!(may_apply(now + Duration::from_secs(30), since, after, Chrome::default(), false, true));
    }

    #[test]
    fn a_staged_release_applies_and_one_another_window_applied_relaunches() {
        assert_eq!(idle_action(true, false), IdleAction::Apply);
        assert_eq!(idle_action(true, true), IdleAction::Apply, "a staged release goes first");
        assert_eq!(idle_action(false, true), IdleAction::Relaunch);
        assert_eq!(idle_action(false, false), IdleAction::Nothing);
    }
}
