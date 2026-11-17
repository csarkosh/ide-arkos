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

    const CARD_W: f64 = 240.0;
    const CARD_H: f64 = 64.0;

    /// The veil's views, over everything in the window's content view: the dim over the whole
    /// window, and the card in it. Dropping it takes it away.
    pub struct Veil {
        view: Retained<NSBox>,
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
            let bar = NSProgressIndicator::initWithFrame(NSProgressIndicator::alloc(mtm), NSRect::new(NSPoint::new(24.0, 0.0), NSSize::new(CARD_W - 48.0, 6.0)));
            bar.setStyle(NSProgressIndicatorStyle::Bar);
            bar.setIndeterminate(true);
            bar.setControlSize(NSControlSize::Small);
            // The title above the bar, whichever way the card counts y (a plain view: up).
            let flipped = card.isFlipped();
            let (title_y, bar_y) = if flipped { (14.0, CARD_H - 22.0) } else { (CARD_H - 14.0 - t.height.ceil(), 16.0) };
            title.setFrame(NSRect::new(NSPoint::new((CARD_W - t.width.ceil()) / 2.0, title_y), NSSize::new(t.width.ceil(), t.height.ceil())));
            bar.setFrameOrigin(NSPoint::new(24.0, bar_y));
            card.addSubview(&title);
            card.addSubview(&bar);
            // SAFETY: the main thread; starting the bar's own animation.
            unsafe { bar.startAnimation(None) };
            let into: &NSView = &dim;
            into.addSubview(&card);
            content.addSubview_positioned_relativeTo(&dim, NSWindowOrderingMode::Above, None);
            Some(Veil { view: dim })
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
    fn only_an_update_s_window_starts_veiled() {
        assert!(starts_veiled(true, false), "the successor of a handoff");
        assert!(starts_veiled(false, true), "a window an update relaunched");
        assert!(!starts_veiled(false, false), "a window the user or the watchdog opened");
    }
}
