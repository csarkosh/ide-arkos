//! An update's relaunch with the window on screen throughout (seamless update handoff spec). The
//! old window asks the daemon for its successor at once (`RelaunchHandoff`) and stays live; the
//! successor starts as an accessory app (no Dock tile, no menu bar) with its window transparent
//! and ignoring the mouse, on top of the old one at its place, so macOS keeps drawing it. Once its
//! terminals are restored and drawn it turns opaque and regular and says so (`HandoffReady`);
//! the daemon tells the old window (`Event::HandOff`), which hands the user's focus over when it
//! has it, leaves the screen and quits. Anything that goes wrong falls back to the relaunch of
//! before: the old window quits first and the daemon starts the new one.
//!
//! Checked on macOS 26.6, 2026-11-16 (a spike of two processes of one bundle, sampled through
//! the window list every millisecond): a window at alpha 0 counts as visible, so its display link
//! runs; there is never a moment with neither window opaque; and
//! `activateFromApplication:options:` from the old app makes the new one active with its window
//! key, where the new app's own `activate` after a `yieldActivation` did not.
use std::time::Duration;

/// How long the successor waits for its terminals before it shows itself anyway: a session that
/// never answers its attach must not keep the old window up for ever.
pub const READY_WAIT: Duration = Duration::from_secs(6);

/// How long the old window waits for its successor to be drawn before it relaunches the old way.
pub const OLD_WAIT: Duration = Duration::from_secs(20);

/// How long the successor keeps drawing with everything restored before it shows itself: a few
/// frames, so what the restore changed is laid out and on screen.
pub const SETTLE: Duration = Duration::from_millis(120);

/// How long the old window waits after handing the focus over before it leaves the screen: a few
/// frames, so the successor's opaque window is composited under it first.
pub const LEAVE_AFTER: Duration = Duration::from_millis(50);

/// The pid of the window this one takes over from, from `ARKOS_HANDOFF`, which the daemon sets
/// on an update's successor.
pub fn predecessor(var: Option<&str>) -> Option<u32> {
    var?.trim().parse().ok().filter(|p| *p > 0)
}

/// Whether the window can hand off rather than quit first: the daemon must speak the handoff,
/// and the window must be on screen as an ordinary window. A full-screen window has a desktop of
/// its own, which its successor could reach only through the full-screen animation, and a
/// minimized one is not on screen to keep: both relaunch the old way.
pub fn can_hand_off(daemon_protocol: u32, full_screen: bool, minimized: bool) -> bool {
    daemon_protocol >= protocol::HANDOFF_FROM && !full_screen && !minimized
}

/// How long the successor gives macOS to call its window visible. macOS draws nothing it does not
/// show (the window is on a desktop not in front, or covered), and a terminal restores only once
/// drawn; such a window is in nobody's sight either, so it takes over at once.
pub const OFF_SCREEN_AFTER: Duration = Duration::from_millis(500);

/// Whether the successor is ready to show itself: nothing left restoring for `settled` (None
/// while something still is) at least `SETTLE`; or off screen (`on_screen` false) past
/// `OFF_SCREEN_AFTER`; or the wait over.
pub fn ready(restoring: usize, settled: Option<Duration>, waited: Duration, on_screen: bool) -> bool {
    (restoring == 0 && settled.is_some_and(|s| s >= SETTLE)) || (!on_screen && waited >= OFF_SCREEN_AFTER) || waited >= READY_WAIT
}

#[cfg(target_os = "macos")]
pub mod mac {
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationOptions, NSApplicationActivationPolicy, NSRunningApplication, NSWindow, NSWindowAnimationBehavior};

    /// The successor's window as it starts: transparent, letting clicks through to the old
    /// window under it, and in front of it without taking the keyboard.
    pub fn start_hidden(window: &NSWindow) {
        window.setAnimationBehavior(NSWindowAnimationBehavior::None);
        window.setAlphaValue(0.0);
        window.setIgnoresMouseEvents(true);
        window.orderFrontRegardless();
    }

    /// The successor shows itself: opaque, taking clicks, and a regular app with its Dock tile
    /// and menu bar.
    pub fn reveal(window: &NSWindow) {
        window.setAlphaValue(1.0);
        window.setIgnoresMouseEvents(false);
        window.orderFrontRegardless();
        if let Some(mtm) = MainThreadMarker::new() {
            NSApplication::sharedApplication(mtm).setActivationPolicy(NSApplicationActivationPolicy::Regular);
        }
    }

    /// The old window's part: when this app is the one in use, it makes the successor (`pid`)
    /// the app in use, as only the active app may under macOS 14's cooperative activation. True
    /// when it asked.
    pub fn give_focus_to(pid: u32) -> bool {
        let Some(mtm) = MainThreadMarker::new() else { return false };
        if !NSApplication::sharedApplication(mtm).isActive() {
            return false;
        }
        let Some(next) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32) else { return false };
        let me = NSRunningApplication::currentApplication();
        next.activateFromApplication_options(&me, NSApplicationActivationOptions::empty())
    }

    /// The old window leaves the screen at once, with no closing animation.
    pub fn leave(window: &Retained<NSWindow>) {
        window.setAnimationBehavior(NSWindowAnimationBehavior::None);
        window.orderOut(None);
    }

    /// Whether macOS shows the window (`NSWindowOcclusionStateVisible`): on the desktop in front
    /// and not wholly covered. A transparent window counts as shown.
    pub fn on_screen(window: &NSWindow) -> bool {
        window.occlusionState().contains(objc2_app_kit::NSWindowOcclusionState::Visible)
    }

    /// Whether the window is full screen.
    pub fn is_full_screen(window: &NSWindow) -> bool {
        window.styleMask().contains(objc2_app_kit::NSWindowStyleMask::FullScreen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_successor_knows_its_predecessor_only_from_a_pid() {
        assert_eq!(predecessor(Some("4242")), Some(4242));
        assert_eq!(predecessor(Some(" 77\n")), Some(77));
        assert_eq!(predecessor(None), None, "an ordinary launch");
        assert_eq!(predecessor(Some("")), None);
        assert_eq!(predecessor(Some("0")), None);
        assert_eq!(predecessor(Some("me")), None);
    }

    #[test]
    fn only_an_ordinary_window_with_a_daemon_that_speaks_it_hands_off() {
        assert!(can_hand_off(protocol::HANDOFF_FROM, false, false));
        assert!(!can_hand_off(protocol::HANDOFF_FROM - 1, false, false), "an older daemon relaunches the old way");
        assert!(!can_hand_off(protocol::HANDOFF_FROM, true, false), "full screen");
        assert!(!can_hand_off(protocol::HANDOFF_FROM, false, true), "minimized");
    }

    #[test]
    fn the_successor_shows_itself_once_restored_and_drawn_or_after_the_wait() {
        let soon = Duration::from_millis(400);
        assert!(!ready(2, None, soon, true), "terminals still restoring");
        assert!(!ready(0, Some(Duration::from_millis(10)), soon, true), "restored, not yet drawn");
        assert!(ready(0, Some(SETTLE), soon, true));
        assert!(ready(3, None, READY_WAIT, true), "a session that never answers does not hold it");
    }

    #[test]
    fn a_successor_macos_does_not_show_takes_over_at_once() {
        // On a desktop not in front: macOS draws nothing, so nothing restores, and nobody sees it.
        assert!(!ready(1, None, Duration::from_millis(200), false), "macOS may not have said yet");
        assert!(ready(1, None, OFF_SCREEN_AFTER, false));
    }
}
