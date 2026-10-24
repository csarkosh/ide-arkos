//! The AppKit side of the project window, for the web pane (web pane spec, section 3.2): the
//! `NSWindow` behind gpui's window, whether it is hidden, its content view's height and
//! flippedness, and the arithmetic that turns a gpui element's bounds into the frame of a child
//! `NSView`. Main thread only: off it every answer is the empty one. The arithmetic is pure and
//! tested; nothing in `cargo test` touches a window.

/// A child view's frame in the content view's coordinates, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Where a web view goes for a tab whose content element was laid out at `bounds` (gpui points,
/// origin top-left of the content view: x, y, width, height), under a bar `bar_h` tall at the top
/// of that element, in a content view `content_h` tall. AppKit's default content view has its
/// origin at the bottom-left with y up, so the rectangle is flipped unless the view says it is
/// flipped. A bar taller than the element leaves a zero-height frame, never a negative one.
pub fn appkit_frame(bounds: (f64, f64, f64, f64), bar_h: f64, content_h: f64, flipped: bool) -> Frame {
    let (x, y, w, h) = bounds;
    let top = y + bar_h;
    let h = (h - bar_h).max(0.0);
    let y = if flipped { top } else { content_h - top - h };
    Frame { x, y, w: w.max(0.0), h }
}

#[cfg(target_os = "macos")]
pub use mac::*;

#[cfg(target_os = "macos")]
mod mac {
    use super::Frame;
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSScreen, NSView, NSWindow};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    /// The menu bar's height in points, from the main display (every display carries one of the
    /// same height). Zero off the main thread or without a display.
    pub fn menu_bar_height() -> f64 {
        let Some(mtm) = MainThreadMarker::new() else { return 0.0 };
        let Some(screen) = NSScreen::mainScreen(mtm).or_else(|| NSScreen::screens(mtm).firstObject()) else { return 0.0 };
        let (f, v) = (screen.frame(), screen.visibleFrame());
        ((f.origin.y + f.size.height) - (v.origin.y + v.size.height)).max(0.0)
    }

    /// The window is minimized to the Dock or hidden (Cmd+H, or not on screen at all).
    pub fn window_hidden(window: &NSWindow) -> bool {
        window.isMiniaturized() || !window.isVisible()
    }

    /// The `NSWindow` behind a gpui window, from its AppKit window handle (the content view).
    pub fn ns_window_of(window: &impl raw_window_handle::HasWindowHandle) -> Option<Retained<NSWindow>> {
        let handle = window.window_handle().ok()?;
        let raw_window_handle::RawWindowHandle::AppKit(appkit) = handle.as_raw() else { return None };
        // SAFETY: an AppKit handle's `ns_view` is the window's live `NSView`, valid while the
        // window is; `window()` retains the `NSWindow` it returns.
        let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
        view.window()
    }

    /// The project window's title; the process's other windows (the "New Window" chooser, a
    /// folder picker) have their own.
    pub const WINDOW_TITLE: &str = "Arkos IDE";

    /// The content view's height in points, and whether its y axis points down. (0, false)
    /// without a content view.
    pub fn content_geometry(window: &NSWindow) -> (f64, bool) {
        match window.contentView() {
            Some(view) => (view.bounds().size.height, view.isFlipped()),
            None => (0.0, false),
        }
    }

    /// The window's content view, to host a child view.
    pub fn content_view(window: &NSWindow) -> Option<Retained<NSView>> {
        window.contentView()
    }

    /// A `Frame` as an `NSRect`.
    pub fn ns_rect(f: Frame) -> NSRect {
        NSRect::new(NSPoint::new(f.x, f.y), NSSize::new(f.w, f.h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_element_under_a_bar_flips_against_the_content_height() {
        // A 300 by 200 element at (10, 20) in a 600 high content view, with a 28 px bar: the page
        // starts 48 from the top and is 172 tall; AppKit measures from the bottom.
        assert_eq!(appkit_frame((10.0, 20.0, 300.0, 200.0), 28.0, 600.0, false), Frame { x: 10.0, y: 380.0, w: 300.0, h: 172.0 });
    }

    #[test]
    fn a_flipped_content_view_keeps_the_top_left_origin() {
        assert_eq!(appkit_frame((10.0, 20.0, 300.0, 200.0), 28.0, 600.0, true), Frame { x: 10.0, y: 48.0, w: 300.0, h: 172.0 });
    }

    #[test]
    fn a_bar_taller_than_the_element_leaves_no_page_and_nothing_negative() {
        let f = appkit_frame((0.0, 0.0, 100.0, 20.0), 28.0, 600.0, false);
        assert_eq!((f.w, f.h), (100.0, 0.0));
        assert_eq!(f.y, 600.0 - 28.0, "the (empty) page sits where it would start");
        assert_eq!(appkit_frame((0.0, 0.0, -5.0, 50.0), 28.0, 600.0, false).w, 0.0);
    }
}
