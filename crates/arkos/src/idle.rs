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

/// How long "Later" puts the toast away before it asks again (the bell keeps its dot).
pub const LATER: Duration = Duration::from_secs(60 * 60);

/// What a window does about an update that is ready (update heads-up spec; Cyrus, 2026-11-16:
/// "Leave it open until the user either clicks update or later. open the toast regardless of
/// user activity"). A window not in front, quiet by the idle rule (`quiet`), updates by itself,
/// nobody watching it; otherwise the toast shows, and stays, until the user answers it, unless a
/// "Later" is not over yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdatePlan {
    ApplyQuietly,
    ShowToast,
    /// A "Later" is running: no toast; the bell's dot stays.
    Later,
}

pub fn update_plan(in_front: bool, quiet: bool, later_until: Option<Instant>, now: Instant) -> UpdatePlan {
    if !in_front && quiet {
        UpdatePlan::ApplyQuietly
    } else if later_until.is_some_and(|t| now < t) {
        UpdatePlan::Later
    } else {
        UpdatePlan::ShowToast
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_toast_shows_until_answered_and_a_window_behind_updates_quietly() {
        use super::*;
        let t0 = Instant::now();
        assert_eq!(update_plan(false, true, None, t0), UpdatePlan::ApplyQuietly, "nobody is looking at it");
        assert_eq!(update_plan(false, false, None, t0), UpdatePlan::ShowToast, "behind but busy: the toast waits there");
        assert_eq!(update_plan(true, true, None, t0), UpdatePlan::ShowToast, "in front: never by itself");
        assert_eq!(update_plan(true, false, None, t0), UpdatePlan::ShowToast, "whatever the user is doing");
        assert_eq!(update_plan(true, true, Some(t0 + LATER), t0), UpdatePlan::Later);
        assert_eq!(update_plan(true, true, Some(t0 + LATER), t0 + LATER), UpdatePlan::ShowToast, "after Later, it asks again");
        assert_eq!(update_plan(false, true, Some(t0 + LATER), t0), UpdatePlan::ApplyQuietly, "away, Later or not");
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
