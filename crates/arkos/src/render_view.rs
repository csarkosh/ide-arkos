//! Diagrams and display math drawn in process (diagrams and math spec, section 1): a `WKWebView`
//! of the window's own, never seen, loads a page `mermaid::prepare` wrote (a local file whose
//! content security policy allows only the bundled scripts and the page's own), waits for the
//! page's title to say `ok <w> <h>` or `error <message>`, and takes a snapshot of the page at
//! `mermaid::SCALE` times its pixels, which `mermaid::finish` writes into the cache as a PNG.
//!
//! The view sits in the window's content view under gpui's own view, which covers it: WebKit
//! paints only a view it takes to be on screen, so it is neither hidden nor outside the window.
//! It keeps nothing between loads (a non-persistent data store) and draws no background, so a
//! picture is transparent where the page is. Everything here runs on the main thread; the
//! completion handlers only fill cells that the next `poll` reads. Nothing in `cargo test`
//! constructs a view.
#![cfg(target_os = "macos")]

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, MainThreadMarker};
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSView, NSWindow, NSWindowOrderingMode};
use objc2_foundation::{NSDictionary, NSError, NSNumber, NSString, NSURL};
use objc2_web_kit::{WKSnapshotConfiguration, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::screen::{self, Frame};

/// How long a page has to answer before its render is an error.
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// The view's width while a page lays out: the widest a diagram is drawn (`mermaid::MAX_W`).
const LAYOUT_W: f64 = 2000.0;

thread_local! {
    static VIEW: RefCell<Option<Retained<WKWebView>>> = const { RefCell::new(None) };
}

/// Adds the hidden view to `window`, under gpui's `view`, once per window (a window is a process).
pub fn install(window: &NSWindow, view: &NSView) {
    if available() {
        return;
    }
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some(content) = screen::content_view(window) else { return };
    // SAFETY: fresh objects on the main thread; every setter takes a value or a live object it
    // retains, and `drawsBackground` is a key the view answers to (WebKit reads it through KVC).
    let web = unsafe {
        let config = WKWebViewConfiguration::new(mtm);
        config.setWebsiteDataStore(&WKWebsiteDataStore::nonPersistentDataStore(mtm));
        let web = WKWebView::initWithFrame_configuration(mtm.alloc(), screen::ns_rect(Frame { x: 0.0, y: 0.0, w: LAYOUT_W, h: 600.0 }), &config);
        let no = NSNumber::new_bool(false);
        let key = NSString::from_str("drawsBackground");
        let _: () = msg_send![&*web, setValue: &*no, forKey: &*key];
        web
    };
    content.addSubview_positioned_relativeTo(&web, NSWindowOrderingMode::Below, Some(view));
    VIEW.with(|v| *v.borrow_mut() = Some(web));
}

/// Whether this window has the view, so it can draw.
pub fn available() -> bool {
    VIEW.with(|v| v.borrow().is_some())
}

/// One render under way: the page, when it started, the title the last question returned, the
/// size the page gave, and the snapshot's PNG once it is taken.
pub struct Render {
    page: PathBuf,
    started: Instant,
    title: Rc<RefCell<Option<String>>>,
    asking: Rc<Cell<bool>>,
    size: Option<(u32, u32)>,
    shot: Rc<RefCell<Option<Result<Vec<u8>, String>>>>,
    shooting: bool,
    /// A poll after the view took the page's size, so WebKit lays it out at that size first.
    settled: bool,
}

impl Render {
    /// Loads `page` (a file in `dir`, which the view may read) into the view.
    pub fn start(page: &Path, dir: &Path) -> Result<Render, String> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&page.to_string_lossy()));
        let root = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(&dir.to_string_lossy()), true);
        with_view(|web| {
            web.setFrame(screen::ns_rect(Frame { x: 0.0, y: 0.0, w: LAYOUT_W, h: 600.0 }));
            // SAFETY: the view is live and on the main thread; the URLs are value types.
            unsafe {
                let _ = web.loadFileURL_allowingReadAccessToURL(&url, &root);
            }
        })?;
        Ok(Render { page: page.to_path_buf(), started: Instant::now(), title: Rc::default(), asking: Rc::default(), size: None, shot: Rc::default(), shooting: false, settled: false })
    }

    /// The page this render loaded.
    pub fn page(&self) -> &Path {
        &self.page
    }

    /// Moves the render along: `None` while it goes on, else the picture's size in CSS pixels
    /// and its PNG, or the message to show.
    pub fn poll(&mut self) -> Option<Result<(u32, u32, Vec<u8>), String>> {
        if self.started.elapsed() > TIMEOUT {
            return Some(Err(format!("the diagram did not finish in {} seconds", TIMEOUT.as_secs())));
        }
        let Some((w, h)) = self.size else {
            if let Some(title) = self.title.borrow_mut().take() {
                match crate::mermaid::size_from_title(&title) {
                    Ok(Some((w, h))) => {
                        if let Err(e) = crate::mermaid::fits(w, h) {
                            return Some(Err(e));
                        }
                        self.size = Some((w, h));
                        let _ = with_view(|web| web.setFrame(screen::ns_rect(Frame { x: 0.0, y: 0.0, w: f64::from(w), h: f64::from(h) })));
                        return None;
                    }
                    Ok(None) => {}
                    Err(e) => return Some(Err(e)),
                }
            }
            if !self.asking.get() {
                self.ask_title();
            }
            return None;
        };
        if !self.settled {
            self.settled = true;
            return None;
        }
        if !self.shooting {
            self.shooting = true;
            self.snapshot(w);
            return None;
        }
        let shot = self.shot.borrow_mut().take()?;
        Some(shot.map(|png| (w, h, png)))
    }

    /// Asks the page for its title; the answer lands in `title`.
    fn ask_title(&self) {
        self.asking.set(true);
        let (title, asking) = (self.title.clone(), self.asking.clone());
        let block = RcBlock::new(move |result: *mut AnyObject, _error: *mut NSError| {
            // SAFETY: WebKit passes the script's value, an `NSString` for a string, or null.
            let text = unsafe { result.as_ref() }.and_then(|r| r.downcast_ref::<NSString>()).map(|s| s.to_string());
            *title.borrow_mut() = text;
            asking.set(false);
        });
        let _ = with_view(|web| {
            // SAFETY: the view is live and on the main thread; the block is retained by WebKit.
            unsafe { web.evaluateJavaScript_completionHandler(&NSString::from_str("document.title"), Some(&block)) }
        });
    }

    /// Takes the snapshot of the whole view (sized to the page) at `SCALE` times the pixels; the
    /// PNG, or why there is none, lands in `shot`.
    fn snapshot(&self, w: u32) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let shot = self.shot.clone();
        let backing = with_view(|web| web.window().map_or(2.0, |win| win.backingScaleFactor())).unwrap_or(2.0);
        let block = RcBlock::new(move |image: *mut NSImage, _error: *mut NSError| {
            // SAFETY: WebKit passes the snapshot, or null when it could not take one.
            let png = unsafe { image.as_ref() }.ok_or_else(|| "WebKit took no snapshot".to_string()).and_then(png_of);
            *shot.borrow_mut() = Some(png);
        });
        let _ = with_view(|web| {
            // SAFETY: a fresh configuration on the main thread; the view is live and retains the
            // block until it calls it.
            unsafe {
                let config = WKSnapshotConfiguration::new(mtm);
                config.setAfterScreenUpdates(true);
                // The width is in points: a picture of `SCALE` pixels a CSS pixel on any screen.
                let points = f64::from(w) * f64::from(crate::mermaid::SCALE) / backing.max(1.0);
                config.setSnapshotWidth(Some(&NSNumber::new_f64(points)));
                web.takeSnapshotWithConfiguration_completionHandler(Some(&config), &block);
            }
        });
    }
}

/// An image as PNG bytes, through its TIFF form and a bitmap.
fn png_of(image: &NSImage) -> Result<Vec<u8>, String> {
    let tiff = image.TIFFRepresentation().ok_or_else(|| "the snapshot has no bitmap".to_string())?;
    let rep = NSBitmapImageRep::imageRepWithData(&tiff).ok_or_else(|| "the snapshot cannot be read".to_string())?;
    // SAFETY: an empty dictionary is a valid property list for any file type.
    let data = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) };
    data.map(|d| d.to_vec()).ok_or_else(|| "the snapshot cannot be written as PNG".to_string())
}

/// Runs `f` with the view, or says there is none.
fn with_view<T>(f: impl FnOnce(&WKWebView) -> T) -> Result<T, String> {
    VIEW.with(|v| v.borrow().as_deref().map(f)).ok_or_else(|| "this window cannot draw diagrams".to_string())
}
