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

/// What tells one key event from another: AppKit's timestamp for it, the key, the modifier
/// flags and the window it was sent to. A key held down repeats with a new timestamp each time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyPress {
    pub time: f64,
    pub code: u16,
    pub flags: usize,
    pub window: isize,
}

/// The same key event handed to the window a second time: after the Mac woke, a window was seen
/// to get every key press twice, so the second one is dropped before gpui hears it.
pub fn delivered_again(last: Option<KeyPress>, next: KeyPress) -> bool {
    last == Some(next)
}

#[cfg(target_os = "macos")]
pub use mac::*;

#[cfg(target_os = "macos")]
mod mac {
    use super::Frame;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{MainThreadMarker, Message};
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSScreen, NSView, NSWindow};
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

    /// gpui's own `NSView` in a gpui window, from its AppKit window handle: the view that takes
    /// the keys back from a web view (`make_first_responder`).
    pub fn ns_view_of(window: &impl raw_window_handle::HasWindowHandle) -> Option<Retained<NSView>> {
        let handle = window.window_handle().ok()?;
        let raw_window_handle::RawWindowHandle::AppKit(appkit) = handle.as_raw() else { return None };
        // SAFETY: an AppKit handle's `ns_view` is the window's live `NSView`, valid while the
        // window is; retained here for as long as the window keeps it.
        let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
        Some(view.retain())
    }

    /// The Cmd chords a page keeps while it has the keyboard: its own editing. Every other Cmd
    /// chord is Arkos's (web pane spec, ruling 9).
    const PAGE_CHORDS: &[&str] = &["c", "v", "x", "a", "z"];

    /// A key equivalent does not pass through a focused `WKWebView` to gpui's view (spec ruling
    /// 9, found by the probe), so a local monitor looks at every key press with Cmd held: when
    /// the window's first responder is not gpui's `view` (a page has the keys) and the chord is
    /// not the page's own, the keys come back to `view` before the event is dispatched, and the
    /// event then reaches gpui's keymap as any other. The monitor lives as long as the returned
    /// token; the window is its own, so a second window installs its own. The monitor also drops
    /// a key event handed over a second time (`delivered_again`), and says so once in the log.
    pub fn install_cmd_monitor(window: Retained<NSWindow>, view: Retained<NSView>) -> Option<Retained<AnyObject>> {
        let last = std::cell::Cell::new(None::<super::KeyPress>);
        let told = std::cell::Cell::new(false);
        let block = block2::RcBlock::new(move |event: std::ptr::NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit hands a live event for the call; it is read, never kept.
            let ev: &NSEvent = unsafe { event.as_ref() };
            let flags = ev.modifierFlags();
            let press = super::KeyPress { time: ev.timestamp(), code: ev.keyCode(), flags: flags.0, window: ev.windowNumber() };
            if super::delivered_again(last.replace(Some(press)), press) {
                if !told.replace(true) {
                    eprintln!("arkos: a key press was handed to the window twice; the second is dropped");
                }
                return std::ptr::null_mut();
            }
            if flags.contains(NSEventModifierFlags::Command) {
                let page_has_keys = window.firstResponder().is_some_and(|r| {
                    let r: Option<&NSView> = r.downcast_ref();
                    r.is_some_and(|r| !std::ptr::eq(r, &*view) && !r.isDescendantOf(&view))
                });
                let chord = ev.charactersIgnoringModifiers().map(|s| s.to_string().to_ascii_lowercase()).unwrap_or_default();
                if page_has_keys && !PAGE_CHORDS.contains(&chord.as_str()) {
                    window.makeFirstResponder(Some(&view));
                }
            }
            event.as_ptr()
        });
        // SAFETY: a local monitor on the main thread; the block is retained by AppKit through the
        // token and by `RcBlock` until both are gone.
        unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block) }
    }

    /// Gives `view` the keyboard: what a web view took on a click comes back to gpui.
    pub fn make_first_responder(window: &NSWindow, view: &NSView) {
        if window.firstResponder().is_none_or(|r| !std::ptr::eq(&*r as *const _ as *const NSView, view)) {
            window.makeFirstResponder(Some(view));
        }
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

    fn press(time: f64, code: u16) -> KeyPress {
        KeyPress { time, code, flags: 0, window: 7 }
    }

    #[test]
    fn a_key_event_handed_over_twice_is_dropped_the_second_time() {
        assert!(delivered_again(Some(press(10.5, 0)), press(10.5, 0)));
    }

    #[test]
    fn a_held_key_a_double_letter_and_the_first_press_all_come_through() {
        // A repeat and "ee" typed quickly are the same key at a later time.
        assert!(!delivered_again(Some(press(10.5, 14)), press(10.53, 14)));
        assert!(!delivered_again(Some(press(10.5, 14)), press(10.5, 15)), "another key at the same instant");
        assert!(!delivered_again(None, press(10.5, 14)));
        let other_window = KeyPress { window: 8, ..press(10.5, 14) };
        assert!(!delivered_again(Some(press(10.5, 14)), other_window));
    }

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
