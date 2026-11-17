//! The update's veil (update veil spec, reworked by the update heads-up spec): while an update
//! swaps the window, a light dim over it and a small card in the middle, "Updating Arkos IDE"
//! over a thin moving bar, so the swap happens behind something calm (Cyrus, 2026-11-16: the
//! full-window frost was "jarring", and "too fast"). The old window fades it in before it asks
//! for its successor; the successor starts under the same veil and fades it out once its
//! terminals are back. It is AppKit's own views laid over gpui's view, as a web tab's view is, so
//! nothing in gpui draws it; its opacity follows Material's tokens through `motion`.

use std::time::{Duration, Instant};

/// How long the veil takes to come in: Material's long2, on the standard decelerate curve.
pub const ENTER: Duration = Duration::from_millis(500);

/// How long it takes to go: long1, on the standard accelerate curve, a little quicker than it
/// came.
pub const EXIT: Duration = Duration::from_millis(450);

/// How long the old window holds the card up, in full, before it asks for its successor: long
/// enough to read it.
pub const READ_HOLD: Duration = Duration::from_millis(600);

/// How long the successor keeps the veil after it showed itself, so the old window has left
/// from under it and the card has been seen before it lifts.
pub const HOLD_AFTER_SWAP: Duration = Duration::from_millis(700);

/// How long a window started by an update's relaunch (without a handoff) waits for its terminals
/// before it lifts the veil anyway.
pub const RESTORE_WAIT: Duration = Duration::from_secs(4);

/// How dark the dim over the window is at full strength: the terminals stay readable under it.
pub const DIM: f64 = 0.22;

pub const TITLE: &str = "Updating Arkos IDE";

/// Where an update is, for the card's bar and its line (Cyrus, 2026-11-16: "I want actual
/// progress indication with metrics visible and updating as the update runs"). The old window
/// shows the first two, the new one the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The daemon swaps in the new build.
    Installing,
    /// The new window is starting.
    Starting,
    /// The new window's visible terminals coming back, `done` of `total`.
    Restoring { done: usize, total: usize },
    Done,
}

/// How full the bar is at `step`: the install a fifth, the new window's start to two fifths,
/// the sessions the rest, one by one.
pub fn fraction(step: Step) -> f64 {
    match step {
        Step::Installing => 0.2,
        Step::Starting => 0.4,
        Step::Restoring { total: 0, .. } => 0.95,
        Step::Restoring { done, total } => 0.45 + 0.5 * (done.min(total) as f64 / total as f64),
        Step::Done => 1.0,
    }
}

/// The card's line under the bar: the step, its count, and the seconds since the update began.
pub fn status(step: Step, version: Option<&str>, elapsed: Duration) -> String {
    let what = match (step, version) {
        (Step::Installing, Some(v)) => format!("Installing {v}"),
        (Step::Installing, None) => "Installing".to_string(),
        (Step::Starting, _) => "Starting the new window".to_string(),
        (Step::Restoring { done, total }, _) => format!("Restoring sessions {} of {total}", done.min(total)),
        (Step::Done, _) => "Done".to_string(),
    };
    format!("{what} \u{b7} {:.1} s", elapsed.as_secs_f64())
}

/// The note the old window leaves for the new one, so the new card's clock goes on from when the
/// update began: milliseconds since the epoch.
pub const STARTED_FILE: &str = "update-started";

/// How old a note may be and still be this update's.
pub const STARTED_FRESH: Duration = Duration::from_secs(120);

/// When the update began, from the note's text and the time now (both milliseconds since the
/// epoch); `None` for a note that does not parse or is stale.
pub fn started_ago(note: &str, now_ms: u64) -> Option<Duration> {
    let at: u64 = note.trim().parse().ok()?;
    let ago = Duration::from_millis(now_ms.checked_sub(at)?);
    (ago <= STARTED_FRESH).then_some(ago)
}

/// The veil's opacity at `now` while it comes in from `since` (true) or goes (false).
pub fn opacity(entering: bool, since: Instant, now: Instant) -> f32 {
    if entering {
        crate::motion::enter_progress(since, now, ENTER)
    } else {
        crate::motion::exit_progress(since, now, EXIT)
    }
}

/// Whether a window starts under the veil: an update's successor (a handoff) or a window an
/// update relaunched (it says "Updated to X"). A window the watchdog or the user opened does not.
pub fn starts_veiled(handoff: bool, updated: bool) -> bool {
    handoff || updated
}

#[cfg(target_os = "macos")]
pub use mac::Veil;

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{msg_send, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSAutoresizingMaskOptions, NSBox, NSBoxType, NSColor, NSControlSize, NSFont, NSFontWeightSemibold, NSProgressIndicator, NSProgressIndicatorStyle, NSTextField, NSTitlePosition,
        NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow, NSWindowOrderingMode,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

    const CARD_W: f64 = 280.0;
    const CARD_H: f64 = 86.0;

    /// The veil's views, over everything in the window's content view: the dim over the whole
    /// window, and the card in it. Dropping it takes it away.
    pub struct Veil {
        view: Retained<NSBox>,
        bar: Retained<NSProgressIndicator>,
        line: Retained<NSTextField>,
    }

    impl Veil {
        /// Lays the veil over `window` at `opacity`. `None` without a content view or off the
        /// main thread.
        pub fn over(window: &NSWindow, opacity: f32) -> Option<Veil> {
            let mtm = MainThreadMarker::new()?;
            let content = window.contentView()?;
            let bounds = content.bounds();
            // The dim: a plain filled box over the whole window.
            let dim = NSBox::initWithFrame(NSBox::alloc(mtm), bounds);
            dim.setBoxType(NSBoxType::Custom);
            dim.setTitlePosition(NSTitlePosition::NoTitle);
            dim.setBorderWidth(0.0);
            dim.setContentViewMargins(NSSize::new(0.0, 0.0));
            dim.setFillColor(&NSColor::colorWithCalibratedWhite_alpha(0.0, super::DIM));
            dim.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
            dim.setAlphaValue(f64::from(opacity));
            // The card: the system's popover material, rounded, centred.
            let card_frame = NSRect::new(NSPoint::new((bounds.size.width - CARD_W) / 2.0, (bounds.size.height - CARD_H) / 2.0), NSSize::new(CARD_W, CARD_H));
            let card = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), card_frame);
            card.setMaterial(NSVisualEffectMaterial::Popover);
            card.setBlendingMode(NSVisualEffectBlendingMode::WithinWindow);
            card.setState(NSVisualEffectState::Active);
            card.setWantsLayer(true);
            // SAFETY: the card's own backing layer, on the main thread; a corner radius and a
            // mask are plain properties of a CALayer.
            unsafe {
                let layer: Option<Retained<AnyObject>> = msg_send![&card, layer];
                if let Some(layer) = layer {
                    let _: () = msg_send![&layer, setCornerRadius: 12.0f64];
                    let _: () = msg_send![&layer, setMasksToBounds: true];
                }
            }
            card.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin | NSAutoresizingMaskOptions::ViewMaxXMargin | NSAutoresizingMaskOptions::ViewMinYMargin | NSAutoresizingMaskOptions::ViewMaxYMargin);
            // SAFETY: the main thread; the system font at the weight AppKit defines.
            let font = unsafe { NSFont::systemFontOfSize_weight(13.0, NSFontWeightSemibold) };
            let title = NSTextField::labelWithString(&NSString::from_str(super::TITLE), mtm);
            title.setFont(Some(&font));
            title.setTextColor(Some(&NSColor::labelColor()));
            title.sizeToFit();
            let t = title.frame().size;
            // A bar that fills as the update goes, never an endless one, and a line under it with
            // the step, its count and the seconds (`super::status`).
            let bar = NSProgressIndicator::initWithFrame(NSProgressIndicator::alloc(mtm), NSRect::new(NSPoint::new(24.0, 0.0), NSSize::new(CARD_W - 48.0, 8.0)));
            bar.setStyle(NSProgressIndicatorStyle::Bar);
            bar.setIndeterminate(false);
            bar.setControlSize(NSControlSize::Small);
            bar.setMinValue(0.0);
            bar.setMaxValue(1.0);
            bar.setDoubleValue(0.0);
            // SAFETY: the main thread; the system's monospaced digits keep the seconds from jiggling.
            let small = unsafe { NSFont::monospacedDigitSystemFontOfSize_weight(11.0, objc2_app_kit::NSFontWeightRegular) };
            let line = NSTextField::labelWithString(&NSString::from_str(" "), mtm);
            line.setFont(Some(&small));
            line.setTextColor(Some(&NSColor::secondaryLabelColor()));
            line.setAlignment(objc2_app_kit::NSTextAlignment::Center);
            line.sizeToFit();
            let lh = line.frame().size.height.ceil();
            // Top to bottom: the title, the bar, the line, whichever way the card counts y.
            let flipped = card.isFlipped();
            let th = t.height.ceil();
            let (title_y, bar_y, line_y) = if flipped { (12.0, 12.0 + th + 10.0, 12.0 + th + 10.0 + 8.0 + 8.0) } else { (CARD_H - 12.0 - th, CARD_H - 12.0 - th - 10.0 - 8.0, 12.0) };
            title.setFrame(NSRect::new(NSPoint::new((CARD_W - t.width.ceil()) / 2.0, title_y), NSSize::new(t.width.ceil(), th)));
            bar.setFrameOrigin(NSPoint::new(24.0, bar_y));
            line.setFrame(NSRect::new(NSPoint::new(12.0, line_y), NSSize::new(CARD_W - 24.0, lh)));
            card.addSubview(&title);
            card.addSubview(&bar);
            card.addSubview(&line);
            let into: &NSView = &dim;
            into.addSubview(&card);
            content.addSubview_positioned_relativeTo(&dim, NSWindowOrderingMode::Above, None);
            Some(Veil { view: dim, bar, line })
        }

        /// The bar's fill, 0 to 1, and the line under it.
        pub fn set_progress(&self, fraction: f64, text: &str) {
            self.bar.setDoubleValue(fraction.clamp(0.0, 1.0));
            self.line.setStringValue(&NSString::from_str(text));
        }

        pub fn set_opacity(&self, opacity: f32) {
            self.view.setAlphaValue(f64::from(opacity.clamp(0.0, 1.0)));
        }
    }

    impl Drop for Veil {
        fn drop(&mut self) {
            self.view.removeFromSuperview();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_veil_comes_in_on_material_s_tokens_and_goes_a_little_quicker() {
        let _lock = crate::motion::test_lock(false);
        assert_eq!(ENTER, Duration::from_millis(500), "long2");
        assert_eq!(EXIT, Duration::from_millis(450), "long1");
        assert!(EXIT < ENTER);
        let t0 = Instant::now();
        assert_eq!(opacity(true, t0, t0), 0.0);
        assert_eq!(opacity(true, t0, t0 + ENTER), 1.0);
        assert_eq!(opacity(false, t0, t0), 1.0);
        assert_eq!(opacity(false, t0, t0 + EXIT), 0.0);
        assert!(opacity(true, t0, t0 + ENTER / 2) > 0.5, "decelerate: most of the way in by half time");
    }

    #[test]
    fn reduce_motion_puts_the_veil_up_and_away_at_once() {
        let _lock = crate::motion::test_lock(true);
        let t0 = Instant::now();
        assert_eq!(opacity(true, t0, t0), 1.0);
        assert_eq!(opacity(false, t0, t0), 0.0);
    }

    #[test]
    fn the_bar_fills_step_by_step_and_session_by_session() {
        assert_eq!(fraction(Step::Installing), 0.2);
        assert_eq!(fraction(Step::Starting), 0.4);
        assert_eq!(fraction(Step::Restoring { done: 0, total: 4 }), 0.45);
        assert_eq!(fraction(Step::Restoring { done: 2, total: 4 }), 0.7);
        assert_eq!(fraction(Step::Restoring { done: 4, total: 4 }), 0.95);
        assert_eq!(fraction(Step::Restoring { done: 0, total: 0 }), 0.95, "nothing to restore");
        assert_eq!(fraction(Step::Done), 1.0);
        let steps = [Step::Installing, Step::Starting, Step::Restoring { done: 0, total: 3 }, Step::Restoring { done: 3, total: 3 }, Step::Done];
        assert!(steps.windows(2).all(|w| fraction(w[0]) < fraction(w[1])), "never goes back");
    }

    #[test]
    fn the_line_says_the_step_its_count_and_the_seconds() {
        let t = Duration::from_millis(1_430);
        assert_eq!(status(Step::Installing, Some("0.8.44"), t), "Installing 0.8.44 \u{b7} 1.4 s");
        assert_eq!(status(Step::Starting, None, t), "Starting the new window \u{b7} 1.4 s");
        assert_eq!(status(Step::Restoring { done: 3, total: 7 }, None, t), "Restoring sessions 3 of 7 \u{b7} 1.4 s");
        assert_eq!(status(Step::Done, None, Duration::from_secs(2)), "Done \u{b7} 2.0 s");
    }

    #[test]
    fn the_new_window_s_clock_goes_on_from_the_old_one_s_note() {
        assert_eq!(started_ago("1000", 2500), Some(Duration::from_millis(1500)));
        assert_eq!(started_ago(" 1000\n", 1000), Some(Duration::ZERO));
        assert_eq!(started_ago("1000", 1000 + 121_000), None, "stale");
        assert_eq!(started_ago("soon", 2000), None);
        assert_eq!(started_ago("3000", 2000), None, "a clock that went back");
    }

    #[test]
    fn only_an_update_s_window_starts_veiled() {
        assert!(starts_veiled(true, false), "the successor of a handoff");
        assert!(starts_veiled(false, true), "a window an update relaunched");
        assert!(!starts_veiled(false, false), "a window the user or the watchdog opened");
    }
}
