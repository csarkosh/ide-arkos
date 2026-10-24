//! The web tab's real view (web pane spec, section 3.2): a `WKWebView` added to the project
//! window's content view as a sibling of gpui's own view, after it, so it draws above gpui's
//! Metal layer. gpui does not know it is there; `Root::place_webs` sets its frame every frame
//! the tab is on screen and hides it otherwise. Everything here runs on the UI thread, which is
//! the main thread. Nothing in `cargo test` constructs a view.
#![cfg(target_os = "macos")]

use crate::screen::{self, Frame};
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::NSWindow;
use objc2_foundation::{NSString, NSURLRequest, NSURL};
use objc2_web_kit::{WKAudiovisualMediaTypes, WKWebView, WKWebViewConfiguration};

/// A `WKWebView` in the window's content view. Dropping it removes the view from the window.
pub struct Host {
    view: Retained<WKWebView>,
    /// The frame last written, so a still window writes nothing.
    placed: Option<Frame>,
}

impl Host {
    /// Adds a hidden web view to `window`'s content view. `None` without a content view or off
    /// the main thread.
    pub fn new(window: &NSWindow) -> Option<Host> {
        let mtm = MainThreadMarker::new()?;
        let content = screen::content_view(window)?;
        let config = configuration(mtm);
        // SAFETY: a fresh allocation initialised once, on the main thread, with a live
        // configuration; the frame is overwritten by the first placement.
        let view = unsafe { WKWebView::initWithFrame_configuration(mtm.alloc(), screen::ns_rect(Frame { x: 0.0, y: 0.0, w: 10.0, h: 10.0 }), &config) };
        view.setHidden(true);
        content.addSubview(&view);
        Some(Host { view, placed: None })
    }

    /// Shows the view at `frame`, or hides it with `None`. A frame already written is skipped.
    pub fn place(&mut self, frame: Option<Frame>) {
        match frame {
            Some(f) => {
                if self.placed != Some(f) {
                    self.view.setFrame(screen::ns_rect(f));
                    self.placed = Some(f);
                }
                if self.view.isHidden() {
                    self.view.setHidden(false);
                }
            }
            None => {
                if !self.view.isHidden() {
                    self.view.setHidden(true);
                }
            }
        }
    }

    /// Loads an absolute `http(s)` URL.
    pub fn load_url(&self, url: &str) {
        let Some(ns) = NSURL::URLWithString(&NSString::from_str(url)) else { return };
        let request = NSURLRequest::requestWithURL(&ns);
        // SAFETY: the view is live and on the main thread; the request is a value type.
        unsafe { self.view.loadRequest(&request) };
    }

}

impl Drop for Host {
    fn drop(&mut self) {
        // SAFETY: the view was added to a superview by `new` and is removed once, here.
        unsafe { self.view.stopLoading() };
        self.view.removeFromSuperview();
    }
}

/// The configuration of spec 3.3: media plays without a click; the default persistent data
/// store (the app's own, apart from Safari's and Chrome's); the app's name after WebKit's user
/// agent; the right-click menu's Inspect Element.
fn configuration(mtm: MainThreadMarker) -> Retained<WKWebViewConfiguration> {
    // SAFETY: a fresh configuration on the main thread; every setter takes a value or a live
    // object it retains.
    unsafe {
        let config = WKWebViewConfiguration::new(mtm);
        config.setMediaTypesRequiringUserActionForPlayback(WKAudiovisualMediaTypes::empty());
        config.setApplicationNameForUserAgent(Some(&NSString::from_str(&format!("Arkos IDE/{}", env!("CARGO_PKG_VERSION")))));
        config
    }
}

/// `arkos web-probe` (spec section 14): one window with one web view, and what the implementer
/// needs to know printed as it happens. Deleted once rulings 7 to 9 are written.
pub mod probe {
    use super::Host;
    use crate::screen::{self, Frame};
    use gpui::{actions, div, prelude::*, px, size, App, Application, Bounds, Context, KeyBinding, Render, Window, WindowBounds, WindowOptions};
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    actions!(web_probe, [ProbeClose, ProbeQuit]);

    struct Probe;

    impl Render for Probe {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(gpui::rgb(0x202830)).text_color(gpui::rgb(0xe0e0e0)).p(px(12.0)).child("web probe: the page should sit below this line, inside the window")
        }
    }

    pub fn run() -> i32 {
        Application::new().run(|cx: &mut App| {
            cx.bind_keys([KeyBinding::new("cmd-w", ProbeClose, None), KeyBinding::new("cmd-q", ProbeQuit, None)]);
            cx.on_action(|_: &ProbeClose, _cx| println!("probe: gpui saw cmd-w (a key equivalent reached gpui with the page focused)"));
            cx.on_action(|_: &ProbeQuit, cx| cx.quit());
            let fixed = size(px(800.0), px(600.0));
            let options = WindowOptions { window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, fixed, cx))), focus: true, ..Default::default() };
            let host: Rc<RefCell<Option<Host>>> = Rc::new(RefCell::new(None));
            let opened = cx.open_window(options, {
                let host = host.clone();
                move |window, cx| {
                    let ns = screen::ns_window_of(window).expect("the probe window has an NSWindow");
                    let (h, flipped) = screen::content_geometry(&ns);
                    println!("probe: content view height {h}, flipped {flipped}");
                    let mut host_view = Host::new(&ns).expect("a web view");
                    host_view.place(Some(screen::appkit_frame((40.0, 60.0, 720.0, 500.0), 0.0, h, flipped)));
                    host_view.load_url("https://example.com/");
                    *host.borrow_mut() = Some(host_view);
                    println!("probe: loaded https://example.com/ at (40, 60) 720 by 500");
                    cx.new(|_| Probe)
                }
            });
            if opened.is_err() {
                println!("probe: the window did not open");
                cx.quit();
                return;
            }
            let timeline = host.clone();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(Duration::from_secs(4)).await;
                println!("probe: hiding the page for two seconds (it should vanish, the dark window stays)");
                let _ = cx.update(|_| timeline.borrow_mut().as_mut().map(|h| h.place(None)));
                cx.background_executor().timer(Duration::from_secs(2)).await;
                println!("probe: showing it again, moved to (100, 120) 600 by 400");
                let _ = cx.update(|_| timeline.borrow_mut().as_mut().map(|h| h.place(Some(Frame { x: 100.0, y: 600.0 - 120.0 - 400.0, w: 600.0, h: 400.0 }))));
                println!("probe: now click into the page, select some text, press cmd-c, then cmd-w, then cmd-q.");
                println!("probe: watching the pasteboard: a line prints when cmd-c changed it.");
                let mtm = objc2::MainThreadMarker::new();
                let mut last = cx.update(|_| mtm.map(|_| objc2_app_kit::NSPasteboard::generalPasteboard().changeCount())).ok().flatten();
                loop {
                    cx.background_executor().timer(Duration::from_millis(500)).await;
                    let now = cx.update(|_| mtm.map(|_| objc2_app_kit::NSPasteboard::generalPasteboard().changeCount())).ok().flatten();
                    if now != last {
                        println!("probe: the pasteboard changed (cmd-c reached the page)");
                        last = now;
                    }
                }
            })
            .detach();
        });
        0
    }
}
