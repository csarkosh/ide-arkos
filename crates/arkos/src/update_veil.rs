//! The update's veil (update veil spec): a blur over the whole window with "Updating Arkos IDE"
//! and "Your sessions keep running.", so the swap to the new window happens behind something
//! calm instead of all at once (Cyrus, 2026-11-16: "still jarring of how sudden it is"). The old
//! window fades it in before it asks for its successor; the successor starts under the same veil
//! and fades it out once its terminals are back. It is AppKit's own blur (`NSVisualEffectView`,
//! within the window) laid over gpui's view, as a web tab's view is, so nothing in gpui draws it;
//! its opacity follows Material's tokens through `motion`.

use std::time::{Duration, Instant};

/// How long the veil takes to come in: Material's medium2, on the standard decelerate curve.
pub const ENTER: Duration = Duration::from_millis(300);

/// How long it takes to go: medium1, on the standard accelerate curve, a little quicker than it
/// came.
pub const EXIT: Duration = Duration::from_millis(250);

/// How long the successor keeps the veil after it showed itself, so the old window has left
/// from under it before the blur lifts: the handoff's own wait to leave, and a few frames.
pub const HOLD_AFTER_SWAP: Duration = Duration::from_millis(200);

/// How long a window started by an update's relaunch (without a handoff) waits for its terminals
/// before it lifts the veil anyway.
pub const RESTORE_WAIT: Duration = Duration::from_secs(4);

pub const TITLE: &str = "Updating Arkos IDE";
pub const SUBTITLE: &str = "Your sessions keep running.";

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
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSAutoresizingMaskOptions, NSColor, NSFont, NSFontWeightRegular, NSFontWeightSemibold, NSTextField, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow,
        NSWindowOrderingMode,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

    /// The veil's view, over everything in the window's content view. Dropping it takes it away.
    pub struct Veil {
        view: Retained<NSVisualEffectView>,
    }

    impl Veil {
        /// Lays the veil over `window` at `opacity`. `None` without a content view or off the
        /// main thread.
        pub fn over(window: &NSWindow, opacity: f32) -> Option<Veil> {
            let mtm = MainThreadMarker::new()?;
            let content = window.contentView()?;
            let bounds = content.bounds();
            let view = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), bounds);
            view.setMaterial(NSVisualEffectMaterial::FullScreenUI);
            view.setBlendingMode(NSVisualEffectBlendingMode::WithinWindow);
            // Active whether or not the window is: an update runs while the user is away.
            view.setState(NSVisualEffectState::Active);
            view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
            view.setAlphaValue(f64::from(opacity));
            // SAFETY: the main thread; the fonts are the system's, at the weights AppKit defines.
            let (title_font, sub_font) = unsafe { (NSFont::systemFontOfSize_weight(22.0, NSFontWeightSemibold), NSFont::systemFontOfSize_weight(13.0, NSFontWeightRegular)) };
            let title = label(super::TITLE, &title_font, &NSColor::labelColor(), mtm);
            let sub = label(super::SUBTITLE, &sub_font, &NSColor::secondaryLabelColor(), mtm);
            let (t, s) = (title.frame().size, sub.frame().size);
            let (cx, cy) = (bounds.size.width / 2.0, bounds.size.height / 2.0);
            let gap = 6.0;
            // The title above the line under it, whichever way the content view counts y.
            let (title_y, sub_y) = if content.isFlipped() { (cy - t.height - gap / 2.0, cy + gap / 2.0) } else { (cy + gap / 2.0, cy - s.height - gap / 2.0) };
            title.setFrameOrigin(NSPoint::new(cx - t.width / 2.0, title_y));
            sub.setFrameOrigin(NSPoint::new(cx - s.width / 2.0, sub_y));
            // Kept centred as the window resizes.
            let centred = NSAutoresizingMaskOptions::ViewMinXMargin | NSAutoresizingMaskOptions::ViewMaxXMargin | NSAutoresizingMaskOptions::ViewMinYMargin | NSAutoresizingMaskOptions::ViewMaxYMargin;
            title.setAutoresizingMask(centred);
            sub.setAutoresizingMask(centred);
            view.addSubview(&title);
            view.addSubview(&sub);
            content.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Above, None);
            Some(Veil { view })
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

    /// A one-line label sized to its text.
    fn label(text: &str, font: &NSFont, color: &NSColor, mtm: MainThreadMarker) -> Retained<NSTextField> {
        let l = NSTextField::labelWithString(&NSString::from_str(text), mtm);
        l.setFont(Some(font));
        l.setTextColor(Some(color));
        l.sizeToFit();
        let f = l.frame();
        l.setFrame(NSRect::new(f.origin, NSSize::new(f.size.width.ceil(), f.size.height.ceil())));
        l
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_veil_comes_in_on_material_s_tokens_and_goes_a_little_quicker() {
        let _lock = crate::motion::test_lock(false);
        assert_eq!(ENTER, Duration::from_millis(300), "medium2");
        assert_eq!(EXIT, Duration::from_millis(250), "medium1");
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
    fn only_an_update_s_window_starts_veiled() {
        assert!(starts_veiled(true, false), "the successor of a handoff");
        assert!(starts_veiled(false, true), "a window an update relaunched");
        assert!(!starts_veiled(false, false), "a window the user or the watchdog opened");
    }
}
