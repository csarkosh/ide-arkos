//! The web tab's real view (web pane spec, section 3.2): a `WKWebView` added to the project
//! window's content view as a sibling of gpui's own view, after it, so it draws above gpui's
//! Metal layer. gpui does not know it is there; `Root` places it every frame the tab is on
//! screen and hides it otherwise. The navigation and UI delegates are the two `define_class!`
//! classes of the pane; each writes a `Signal` into a cell the next `poll` drains and calls
//! nothing of gpui. Everything here runs on the UI thread, which is the main thread. Nothing in
//! `cargo test` constructs a view.
#![cfg(target_os = "macos")]

use crate::screen::{self, Frame};
use crate::web::{Poll, Signal, Web};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{NSColor, NSView, NSWindow, NSWorkspace};
use objc2_foundation::{NSError, NSNumber, NSObject, NSObjectProtocol, NSString, NSURLRequest, NSURL};
use objc2_web_kit::{
    WKAudiovisualMediaTypes, WKNavigation, WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKNavigationResponse,
    WKNavigationResponsePolicy, WKUIDelegate, WKWebView, WKWebViewConfiguration, WKWindowFeatures,
};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

/// `NSURLErrorCancelled`: a load that was stopped, by a newer load or by the page itself.
const URL_ERROR_CANCELLED: isize = -999;
/// WebKit's own "Frame load interrupted", what a download or a refused response ends with.
const WEBKIT_FRAME_LOAD_INTERRUPTED: isize = 102;

/// The signals the delegates leave for the next poll.
type Signals = Rc<RefCell<Vec<Signal>>>;

struct DelegateIvars {
    signals: Signals,
}

/// A URL's text, for a signal.
fn url_text(url: Option<Retained<NSURL>>) -> String {
    url.and_then(|u| u.absoluteString()).map(|s| s.to_string()).unwrap_or_default()
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; the class implements no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ArkosNavigationDelegate"]
    #[ivars = DelegateIvars]
    struct NavigationDelegate;

    unsafe impl NSObjectProtocol for NavigationDelegate {}

    unsafe impl WKNavigationDelegate for NavigationDelegate {
        #[unsafe(method(webView:didCommitNavigation:))]
        fn did_commit(&self, web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
            // SAFETY: the view is live for the call.
            let url = url_text(unsafe { web_view.URL() });
            self.ivars().signals.borrow_mut().push(Signal::Committed(url));
        }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn did_fail_provisional(&self, _web_view: &WKWebView, _navigation: Option<&WKNavigation>, error: &NSError) {
            self.ivars().signals.borrow_mut().push(failure(error));
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn did_fail(&self, _web_view: &WKWebView, _navigation: Option<&WKNavigation>, error: &NSError) {
            self.ivars().signals.borrow_mut().push(failure(error));
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn did_terminate(&self, _web_view: &WKWebView) {
            self.ivars().signals.borrow_mut().push(Signal::Terminated);
        }

        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide_action(&self, _web_view: &WKWebView, action: &WKNavigationAction, handler: &block2::DynBlock<dyn Fn(WKNavigationActionPolicy)>) {
            // SAFETY: the action is live for the call; the request's URL is read, not kept.
            let url = url_text(unsafe { action.request().URL() });
            let scheme = url.split_once(':').map(|(s, _)| s.to_ascii_lowercase()).unwrap_or_default();
            if matches!(scheme.as_str(), "http" | "https" | "file" | "about" | "blob" | "data" | "javascript") {
                handler.call((WKNavigationActionPolicy::Allow,));
            } else {
                // Another scheme (`mailto:`, an app's own): the system's handler, not the pane.
                if let Some(ns) = NSURL::URLWithString(&NSString::from_str(&url)) {
                    // SAFETY: a plain call on the shared workspace with a live URL.
                    let _ = NSWorkspace::sharedWorkspace().openURL(&ns);
                }
                self.ivars().signals.borrow_mut().push(Signal::OpenOutside(url));
                handler.call((WKNavigationActionPolicy::Cancel,));
            }
        }

        #[unsafe(method(webView:decidePolicyForNavigationResponse:decisionHandler:))]
        fn decide_response(&self, _web_view: &WKWebView, response: &WKNavigationResponse, handler: &block2::DynBlock<dyn Fn(WKNavigationResponsePolicy)>) {
            // SAFETY: the response is live for the call.
            if unsafe { response.canShowMIMEType() } {
                handler.call((WKNavigationResponsePolicy::Allow,));
            } else {
                self.ivars().signals.borrow_mut().push(Signal::Undisplayable);
                handler.call((WKNavigationResponsePolicy::Cancel,));
            }
        }
    }
);

define_class!(
    // SAFETY: NSObject has no subclassing requirements; the class implements no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ArkosUiDelegate"]
    #[ivars = DelegateIvars]
    struct UiDelegate;

    unsafe impl NSObjectProtocol for UiDelegate {}

    unsafe impl WKUIDelegate for UiDelegate {
        #[unsafe(method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
        fn create_web_view(&self, web_view: &WKWebView, _configuration: &WKWebViewConfiguration, action: &WKNavigationAction, _features: &WKWindowFeatures) -> Option<Retained<WKWebView>> {
            // One page per tab (ruling 3): a window the page opens loads in the same view.
            // SAFETY: the view and the action are live for the call.
            unsafe {
                let _ = web_view.loadRequest(&action.request());
            }
            None
        }
    }
);

fn failure(error: &NSError) -> Signal {
    let code = error.code() as isize;
    if code == URL_ERROR_CANCELLED || code == WEBKIT_FRAME_LOAD_INTERRUPTED {
        Signal::Cancelled
    } else {
        Signal::Failed(error.localizedDescription().to_string())
    }
}

impl NavigationDelegate {
    fn new(mtm: MainThreadMarker, signals: Signals) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars { signals });
        // SAFETY: NSObject's init on a fresh allocation with its ivars set.
        unsafe { msg_send![super(this), init] }
    }
}

impl UiDelegate {
    fn new(mtm: MainThreadMarker, signals: Signals) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars { signals });
        // SAFETY: NSObject's init on a fresh allocation with its ivars set.
        unsafe { msg_send![super(this), init] }
    }
}

/// A `WKWebView` in the window's content view. Dropping it removes the view from the window.
pub struct Host {
    view: Retained<WKWebView>,
    window: Retained<NSWindow>,
    /// The delegates, kept alive: the view holds them weakly.
    _navigation: Retained<NavigationDelegate>,
    _ui: Retained<UiDelegate>,
    signals: Signals,
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
        let signals: Signals = Rc::new(RefCell::new(Vec::new()));
        let navigation = NavigationDelegate::new(mtm, signals.clone());
        let ui = UiDelegate::new(mtm, signals.clone());
        // SAFETY: the delegates outlive the view (they are dropped with the host after it).
        unsafe {
            view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*navigation)));
            view.setUIDelegate(Some(ProtocolObject::from_ref(&*ui)));
            view.setAllowsBackForwardNavigationGestures(true);
        }
        view.setHidden(true);
        content.addSubview(&view);
        Some(Host { view, window: window.retain(), _navigation: navigation, _ui: ui, signals, placed: None })
    }
}

impl Web for Host {
    fn load(&mut self, url: &str, read_root: Option<&Path>) {
        let Some(ns) = NSURL::URLWithString(&NSString::from_str(url)) else { return };
        // SAFETY: the view is live and on the main thread; the URLs are value types.
        unsafe {
            match (crate::web::file_path(url), read_root) {
                (Some(_), Some(root)) => {
                    let root = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(&root.to_string_lossy()), true);
                    let _ = self.view.loadFileURL_allowingReadAccessToURL(&ns, &root);
                }
                _ => {
                    let _ = self.view.loadRequest(&NSURLRequest::requestWithURL(&ns));
                }
            }
        }
    }

    fn reload(&mut self) {
        // SAFETY: the view is live and on the main thread.
        unsafe {
            let _ = self.view.reload();
        }
    }

    fn stop(&mut self) {
        // SAFETY: as above.
        unsafe { self.view.stopLoading() };
    }

    fn back(&mut self) {
        // SAFETY: as above.
        unsafe {
            let _ = self.view.goBack();
        }
    }

    fn forward(&mut self) {
        // SAFETY: as above.
        unsafe {
            let _ = self.view.goForward();
        }
    }

    fn set_zoom(&mut self, zoom: f64) {
        // SAFETY: as above.
        unsafe { self.view.setPageZoom(zoom) };
    }

    fn place(&mut self, frame: Option<Frame>) {
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

    fn poll(&mut self) -> Poll {
        // SAFETY: the view is live and on the main thread; every read is a property.
        unsafe {
            Poll {
                loading: self.view.isLoading(),
                progress: self.view.estimatedProgress(),
                url: Some(url_text(self.view.URL())).filter(|s| !s.is_empty()),
                title: self.view.title().map(|t| t.to_string()),
                can_back: self.view.canGoBack(),
                can_forward: self.view.canGoForward(),
                signals: std::mem::take(&mut *self.signals.borrow_mut()),
            }
        }
    }

    fn set_under_color(&mut self, (r, g, b): (u8, u8, u8)) {
        let color = NSColor::colorWithSRGBRed_green_blue_alpha(f64::from(r) / 255.0, f64::from(g) / 255.0, f64::from(b) / 255.0, 1.0);
        // SAFETY: the view is live and on the main thread.
        unsafe { self.view.setUnderPageBackgroundColor(Some(&color)) };
    }

    fn has_keys(&self) -> bool {
        self.window.firstResponder().is_some_and(|r| {
            // A responder inside the view is one of its views (WebKit's own).
            let view: Option<&NSView> = r.downcast_ref();
            view.is_some_and(|v| v.isDescendantOf(&self.view))
        })
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        // SAFETY: the view was added to a superview by `new` and is removed once, here; the
        // delegates are cleared before they go.
        unsafe {
            self.view.stopLoading();
            self.view.setNavigationDelegate(None);
            self.view.setUIDelegate(None);
        }
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
        // `developerExtrasEnabled` is not in the public header; WebKit reads it through KVC.
        let prefs = config.preferences();
        let yes = NSNumber::new_bool(true);
        let key = NSString::from_str("developerExtrasEnabled");
        let _: () = msg_send![&*prefs, setValue: &*yes, forKey: &*key];
        config
    }
}
