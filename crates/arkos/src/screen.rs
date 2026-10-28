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

/// Who holds the window's keyboard (its first responder) when a key arrives: gpui's view or a
/// view inside it; a page (a view inside a `WKWebView`); or nobody Arkos knows. The last is the
/// window itself, which AppKit makes the first responder when the view that had the keys is
/// removed from the window, or a view of neither kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeysHolder {
    Gpui,
    Page,
    Lost,
}

/// The Cmd chords a page keeps while it has the keyboard: its own editing. Every other Cmd
/// chord is Arkos's (web pane spec, ruling 9). `chord` is the key's lowercase character.
pub const PAGE_CHORDS: &[&str] = &["c", "v", "x", "a", "z"];

/// Whether the keys come back to gpui's view before this key is dispatched. Lost keys always
/// come back: a window that is its own first responder sends every key through its own
/// `keyDown:`, which runs the view hierarchy's `performKeyEquivalent:` once more, so gpui's view
/// hears each key twice (its guard against a duplicate covers the key down that follows a key
/// equivalent, not a second key equivalent). That was every key press typed twice, after a web
/// tab that held the keys was closed (2026-10-28, found by sampling the window). A page keeps
/// the keys except for a Cmd chord that is Arkos's own.
pub fn keys_return(holder: KeysHolder, cmd: bool, chord: &str) -> bool {
    match holder {
        KeysHolder::Lost => true,
        KeysHolder::Page => cmd && !PAGE_CHORDS.contains(&chord),
        KeysHolder::Gpui => false,
    }
}

#[cfg(target_os = "macos")]
pub use mac::*;

#[cfg(target_os = "macos")]
mod mac {
    use super::{keys_return, Frame, KeysHolder};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{MainThreadMarker, Message};
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSScreen, NSView, NSWindow, NSWindowOcclusionState};
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use objc2_web_kit::WKWebView;

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

    /// No part of the window can be seen: hidden as `window_hidden` says, or occluded, wholly
    /// behind other windows or on another Space. The wash draws nothing then (effects spec 4.4).
    pub fn window_unseen(window: &NSWindow) -> bool {
        window_hidden(window) || !window.occlusionState().contains(NSWindowOcclusionState::Visible)
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

    /// Who has `window`'s keyboard now, for `keys_return`: gpui's `view` or a view inside it, a
    /// view inside a `WKWebView`, or nobody Arkos knows (the window itself after the view that
    /// had the keys left the window, or a view of neither kind).
    pub fn keys_holder(window: &NSWindow, view: &NSView) -> KeysHolder {
        let Some(responder) = window.firstResponder() else { return KeysHolder::Lost };
        let Some(r) = responder.downcast_ref::<NSView>() else { return KeysHolder::Lost };
        if std::ptr::eq(r, view) || r.isDescendantOf(view) {
            return KeysHolder::Gpui;
        }
        let mut up = Some(r.retain());
        while let Some(v) = up {
            if v.downcast_ref::<WKWebView>().is_some() {
                return KeysHolder::Page;
            }
            // SAFETY: a live view's superview, read on the main thread and retained for the step.
            up = unsafe { v.superview() };
        }
        KeysHolder::Lost
    }

    /// A local monitor that looks at every key press before the window dispatches it and gives
    /// the keys back to gpui's `view` when `keys_return` says so. Two reasons it exists. A key
    /// equivalent does not pass through a focused `WKWebView` to gpui's view (web pane spec,
    /// ruling 9, found by the probe), so a Cmd chord of Arkos's own takes the keys back from a
    /// page, and then reaches gpui's keymap as any other. And a window left as its own first
    /// responder would hand every key to gpui's view twice (`keys_return`), so lost keys come
    /// back on the first key pressed. The monitor lives as long as the returned token; the
    /// window is its own, so a second window installs its own.
    pub fn install_key_monitor(window: Retained<NSWindow>, view: Retained<NSView>) -> Option<Retained<AnyObject>> {
        let block = block2::RcBlock::new(move |event: std::ptr::NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit hands a live event for the call; it is read, never kept.
            let ev: &NSEvent = unsafe { event.as_ref() };
            let holder = keys_holder(&window, &view);
            if holder != KeysHolder::Gpui {
                let cmd = ev.modifierFlags().contains(NSEventModifierFlags::Command);
                let chord = ev.charactersIgnoringModifiers().map(|s| s.to_string().to_ascii_lowercase()).unwrap_or_default();
                if keys_return(holder, cmd, &chord) {
                    window.makeFirstResponder(Some(&view));
                }
            }
            event.as_ptr()
        });
        // SAFETY: a local monitor on the main thread; the block is retained by AppKit through the
        // token and by `RcBlock` until both are gone.
        unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block) }
    }

    /// Gives `view` the keyboard: what a web view took on a click comes back to gpui. Also what
    /// a web view that is about to leave the window does while it still has them, or AppKit
    /// would make the window its own first responder (`keys_return` says what follows).
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

    #[test]
    fn lost_keys_come_back_to_gpui_on_any_key() {
        // The window is its own first responder: a plain letter, Enter and even a page's own
        // chord all bring the keys back, or gpui's view hears each of them twice.
        assert!(keys_return(KeysHolder::Lost, false, "a"));
        assert!(keys_return(KeysHolder::Lost, false, "\r"));
        assert!(keys_return(KeysHolder::Lost, true, "c"));
    }

    #[test]
    fn a_page_keeps_plain_typing_and_its_own_chords() {
        assert!(!keys_return(KeysHolder::Page, false, "a"));
        for chord in ["c", "v", "x", "a", "z"] {
            assert!(!keys_return(KeysHolder::Page, true, chord), "Cmd+{chord} is the page's");
        }
    }

    #[test]
    fn a_page_gives_up_the_chords_that_are_arkos_s_own() {
        assert!(keys_return(KeysHolder::Page, true, "w"));
        assert!(keys_return(KeysHolder::Page, true, "1"));
        assert!(keys_return(KeysHolder::Page, true, "b"));
    }

    #[test]
    fn gpui_s_own_view_is_left_alone() {
        assert!(!keys_return(KeysHolder::Gpui, false, "a"));
        assert!(!keys_return(KeysHolder::Gpui, true, "w"));
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
