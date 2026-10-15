//! The macOS Accessibility API, as much of it as docking needs (spec 8): the trust check, the
//! Chrome application's windows, their frames, moving and raising them. Behind a trait so the
//! docking engine is tested with a fake; nothing in `cargo test` touches the real API.

use objc2_app_kit::{NSApplicationActivationOptions, NSEvent, NSRunningApplication};
use objc2_core_foundation::{CFArray, CFBoolean, CFDictionary, CFRetained, CFString, CFType, CGPoint, CGSize};
use objc2_foundation::ns_string;
#[cfg(test)]
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::NonNull;

/// The C `AXError`, an `int32_t` enum.
pub type AXError = i32;
pub const AX_SUCCESS: AXError = 0;
/// `kAXErrorAPIDisabled`: the process is not trusted.
pub const AX_API_DISABLED: AXError = -25211;
/// `kAXErrorInvalidUIElement`: the window is gone.
pub const AX_INVALID_ELEMENT: AXError = -25202;
/// `kAXValueTypeCGPoint` and `kAXValueTypeCGSize` (`AXValueType` is a `uint32_t` enum).
const AX_VALUE_CGPOINT: u32 = 1;
const AX_VALUE_CGSIZE: u32 = 2;
/// `kAXErrorNoValue`: the attribute has no value.
const AX_NO_VALUE: AXError = -25212;

// From `HIServices/AXUIElement.h` and `AXValue.h`. `AXUIElementRef` and `AXValueRef` are
// CoreFoundation objects, typed here as `CFType`; ownership follows the Create/Copy rule and is
// handed to `CFRetained` at once.
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    static kAXTrustedCheckOptionPrompt: *const CFString;
    // `Boolean` is an `unsigned char`: read as `u8`, compared with zero.
    fn AXIsProcessTrustedWithOptions(options: *const CFDictionary) -> u8;
    fn AXUIElementCreateApplication(pid: i32) -> *mut CFType;
    fn AXUIElementCreateSystemWide() -> *mut CFType;
    fn AXUIElementSetMessagingTimeout(element: *const CFType, seconds: f32) -> AXError;
    fn AXUIElementCopyAttributeValue(element: *const CFType, attribute: *const CFString, value: *mut *mut CFType) -> AXError;
    fn AXUIElementSetAttributeValue(element: *const CFType, attribute: *const CFString, value: *const CFType) -> AXError;
    fn AXUIElementPerformAction(element: *const CFType, action: *const CFString) -> AXError;
    fn AXValueCreate(kind: u32, value: *const c_void) -> *mut CFType;
    fn AXValueGetValue(value: *const CFType, kind: u32, out: *mut c_void) -> u8;
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// How far right of the rightmost display a hidden window is parked (ruling 1).
pub const OFFSCREEN_GAP: f64 = 64.0;

impl Rect {
    /// Half-open: the left and top edges are inside, the right and bottom ones are not.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    /// The largest distance any edge of `other` lies from the same edge of `self`: zero when
    /// they coincide, the drag distance when a window was moved.
    pub fn distance_outside(&self, other: &Rect) -> f64 {
        let left = (other.x - self.x).abs();
        let top = (other.y - self.y).abs();
        let right = (other.x + other.w - (self.x + self.w)).abs();
        let bottom = (other.y + other.h - (self.y + self.h)).abs();
        left.max(top).max(right).max(bottom)
    }

    /// The same rectangle parked right of every display (ruling 1), keeping its y and size.
    /// `screens_right` is the right edge of the union of the displays.
    pub fn offscreen(&self, screens_right: f64) -> Rect {
        Rect { x: screens_right + OFFSCREEN_GAP, ..*self }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxError {
    /// `kAXErrorAPIDisabled`: Accessibility access is not granted.
    NotTrusted,
    /// `kAXErrorInvalidUIElement`: the window closed.
    Gone,
    Other(i32),
}

impl AxError {
    fn from_code(code: AXError) -> Result<(), AxError> {
        match code {
            AX_SUCCESS => Ok(()),
            AX_API_DISABLED => Err(AxError::NotTrusted),
            AX_INVALID_ELEMENT => Err(AxError::Gone),
            other => Err(AxError::Other(other)),
        }
    }
}

/// A window of another application. Equality is the element's identity (`CFEqual`), never its
/// title. Not `Send`: AX elements are used on the UI thread only.
#[derive(Clone)]
pub struct AxWindow(Element);

#[derive(Clone)]
enum Element {
    /// A retained `AXUIElementRef`: cloning retains it, dropping releases it.
    Real(CFRetained<CFType>),
    /// `FakeAx`'s windows: a number, never retained or released.
    #[cfg(test)]
    Fake(usize),
}

impl AxWindow {
    /// A number for logs: the element's address, or the fake's counter.
    pub fn id(&self) -> usize {
        match &self.0 {
            Element::Real(e) => CFRetained::as_ptr(e).as_ptr() as usize,
            #[cfg(test)]
            Element::Fake(n) => *n,
        }
    }

    #[cfg(test)]
    fn fake(n: usize) -> AxWindow {
        AxWindow(Element::Fake(n))
    }

    fn element(&self) -> Option<&CFType> {
        match &self.0 {
            Element::Real(e) => Some(e),
            #[cfg(test)]
            Element::Fake(_) => None,
        }
    }
}

impl PartialEq for AxWindow {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            // `CFType`'s `PartialEq` is `CFEqual`.
            (Element::Real(a), Element::Real(b)) => **a == **b,
            #[cfg(test)]
            (Element::Fake(a), Element::Fake(b)) => a == b,
            #[cfg(test)]
            _ => false,
        }
    }
}

impl std::fmt::Debug for AxWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AxWindow({:#x})", self.id())
    }
}

/// The Accessibility calls docking makes. UI thread only; every call returns at once.
pub trait Ax {
    /// Whether this process may use the API; `prompt` shows the system's request once.
    fn trusted(&self, prompt: bool) -> bool;
    fn chrome_pid(&self) -> Option<i32>;
    /// Chrome's windows; an error when the list cannot be read now (not trusted, Chrome busy),
    /// which is not the same as no windows.
    fn windows(&self, pid: i32) -> Result<Vec<AxWindow>, AxError>;
    /// Global, top-left origin, points. `None` when the window is gone or access is missing.
    fn frame(&self, w: &AxWindow) -> Option<Rect>;
    fn set_frame(&self, w: &AxWindow, r: Rect) -> Result<(), AxError>;
    fn raise(&self, w: &AxWindow) -> Result<(), AxError>;
    /// Raises the window and makes Chrome the active app, so the window takes the keys.
    fn focus(&self, w: &AxWindow) -> Result<(), AxError>;
    fn title(&self, w: &AxWindow) -> Option<String>;
    fn minimized(&self, w: &AxWindow) -> bool;
    fn mouse_down(&self) -> bool;
    /// Global, top-left origin.
    fn mouse_location(&self) -> (f64, f64);
}

/// The real API.
#[derive(Default)]
pub struct RealAx {
    /// How often `set_frame` had to write the size a second time because the first write did not
    /// take (Chrome clamped it against the old position). The probe reports it.
    second_size_writes: std::cell::Cell<u32>,
}

impl RealAx {
    pub fn new() -> RealAx {
        // Every AX call blocks the UI thread until Chrome answers, 6 s by default when Chrome
        // hangs. A quarter second at most, set on the system-wide element (the global default):
        // a call that times out fails, and the engine takes a failed read as nothing changed.
        // SAFETY: a Create function; `owned` takes its one retain and releases it after the call.
        if let Some(system) = owned(unsafe { AXUIElementCreateSystemWide() }) {
            // SAFETY: `system` is a live AXUIElementRef for the duration of the call.
            unsafe { AXUIElementSetMessagingTimeout(&*system, 0.25) };
        }
        RealAx::default()
    }

    pub fn second_size_writes(&self) -> u32 {
        self.second_size_writes.get()
    }
}

/// Takes ownership of a pointer returned by a Create or Copy function.
fn owned(ptr: *mut CFType) -> Option<CFRetained<CFType>> {
    // SAFETY: the pointer came from a Create/Copy function, so this process owns one retain,
    // which `CFRetained` releases exactly once.
    NonNull::new(ptr).map(|p| unsafe { CFRetained::from_raw(p) })
}

fn copy_attribute(element: &CFType, name: &str) -> Result<CFRetained<CFType>, AxError> {
    let attribute = CFString::from_str(name);
    let mut out: *mut CFType = std::ptr::null_mut();
    // SAFETY: `element` is a live AX element, `attribute` a live string, `out` a valid slot.
    let code = unsafe { AXUIElementCopyAttributeValue(element, &*attribute, &mut out) };
    // On failure `out` stays null; on success it is owned.
    let value = owned(out);
    AxError::from_code(code)?;
    value.ok_or(AxError::Other(AX_NO_VALUE))
}

fn set_attribute(element: &CFType, name: &str, value: &CFType) -> Result<(), AxError> {
    let attribute = CFString::from_str(name);
    // SAFETY: all three are live CoreFoundation objects; the call does not take ownership.
    AxError::from_code(unsafe { AXUIElementSetAttributeValue(element, &*attribute, value) })
}

fn ax_value<T: Copy>(kind: u32, value: &T) -> Option<CFRetained<CFType>> {
    // SAFETY: `kind` names the struct `value` points to (`CGPoint` or `CGSize`); the value is copied.
    owned(unsafe { AXValueCreate(kind, value as *const T as *const c_void) })
}

fn read_value<T: Copy + Default>(element: &CFType, name: &str, kind: u32) -> Option<T> {
    let value = copy_attribute(element, name).ok()?;
    let mut out = T::default();
    // SAFETY: `out` is the struct `kind` names; the call fails (zero) when the value is another kind.
    (unsafe { AXValueGetValue(&*value, kind, &mut out as *mut T as *mut c_void) } != 0).then_some(out)
}

fn read_size(element: &CFType) -> Option<CGSize> {
    read_value::<CGSize>(element, "AXSize", AX_VALUE_CGSIZE)
}

fn write_size(element: &CFType, r: Rect) -> Result<(), AxError> {
    let size = ax_value(AX_VALUE_CGSIZE, &CGSize { width: r.w, height: r.h }).ok_or(AxError::Other(AX_NO_VALUE))?;
    set_attribute(element, "AXSize", &size)
}

fn write_position(element: &CFType, r: Rect) -> Result<(), AxError> {
    let point = ax_value(AX_VALUE_CGPOINT, &CGPoint { x: r.x, y: r.y }).ok_or(AxError::Other(AX_NO_VALUE))?;
    set_attribute(element, "AXPosition", &point)
}

fn same_size(s: CGSize, r: Rect) -> bool {
    (s.width - r.w).abs() < 0.5 && (s.height - r.h).abs() < 0.5
}

impl Ax for RealAx {
    fn trusted(&self, prompt: bool) -> bool {
        // SAFETY: a constant `CFStringRef` exported by the framework, valid for the process.
        let Some(key) = (unsafe { kAXTrustedCheckOptionPrompt.as_ref() }) else { return false };
        let options = CFDictionary::<CFString, CFBoolean>::from_slices(&[key], &[CFBoolean::new(prompt)]);
        let options: &CFDictionary = (*options).as_ref();
        // SAFETY: a valid dictionary with the one documented key.
        unsafe { AXIsProcessTrustedWithOptions(options) != 0 }
    }

    fn chrome_pid(&self) -> Option<i32> {
        // The stable channel only. `open -na` briefly starts a second process that hands the
        // window to the running one and exits: the one that finished launching first (the
        // lowest pid among the launched ones) is Chrome.
        let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(ns_string!("com.google.Chrome"));
        let all: Vec<_> = apps.iter().collect();
        let launched = all.iter().filter(|a| a.isFinishedLaunching()).map(|a| a.processIdentifier()).min();
        launched.or_else(|| all.iter().map(|a| a.processIdentifier()).min())
    }

    fn windows(&self, pid: i32) -> Result<Vec<AxWindow>, AxError> {
        // SAFETY: creates an owned element for `pid`; it is released when `app` drops.
        let app = owned(unsafe { AXUIElementCreateApplication(pid) }).ok_or(AxError::Other(AX_NO_VALUE))?;
        let value = match copy_attribute(&app, "AXWindows") {
            Ok(v) => v,
            // No value: an app with no windows.
            Err(AxError::Other(AX_NO_VALUE)) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let array = value.downcast_ref::<CFArray>().ok_or(AxError::Other(AX_NO_VALUE))?;
        // SAFETY: `AXWindows` is an array of AX elements, all CoreFoundation objects; each one
        // is retained into its `AxWindow`.
        let array: &CFArray<CFType> = unsafe { array.cast_unchecked() };
        Ok(array.iter().map(|e| AxWindow(Element::Real(e))).collect())
    }

    fn frame(&self, w: &AxWindow) -> Option<Rect> {
        let element = w.element()?;
        let point = read_value::<CGPoint>(element, "AXPosition", AX_VALUE_CGPOINT)?;
        let size = read_size(element)?;
        Some(Rect { x: point.x, y: point.y, w: size.width, h: size.height })
    }

    /// Size, then position, then the size once more when the first write did not take: a window
    /// growing toward a screen edge has its size clamped against its old position.
    fn set_frame(&self, w: &AxWindow, r: Rect) -> Result<(), AxError> {
        let element = w.element().ok_or(AxError::Gone)?;
        let resize = read_size(element).is_none_or(|s| !same_size(s, r));
        if resize {
            write_size(element, r)?;
        }
        write_position(element, r)?;
        if resize && read_size(element).is_some_and(|s| !same_size(s, r)) {
            self.second_size_writes.set(self.second_size_writes.get() + 1);
            write_size(element, r)?;
        }
        Ok(())
    }

    fn raise(&self, w: &AxWindow) -> Result<(), AxError> {
        let element = w.element().ok_or(AxError::Gone)?;
        let action = CFString::from_str("AXRaise");
        // SAFETY: a live element and a live string.
        AxError::from_code(unsafe { AXUIElementPerformAction(element, &*action) })
    }

    fn focus(&self, w: &AxWindow) -> Result<(), AxError> {
        self.raise(w)?;
        // Without "all windows": only the raised window (now Chrome's main one) comes forward.
        let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(ns_string!("com.google.Chrome"));
        if let Some(app) = apps.firstObject() {
            app.activateWithOptions(NSApplicationActivationOptions(0));
        }
        Ok(())
    }

    fn title(&self, w: &AxWindow) -> Option<String> {
        let value = copy_attribute(w.element()?, "AXTitle").ok()?;
        value.downcast_ref::<CFString>().map(|s| s.to_string())
    }

    fn minimized(&self, w: &AxWindow) -> bool {
        let Some(element) = w.element() else { return false };
        copy_attribute(element, "AXMinimized")
            .ok()
            .and_then(|v| v.downcast_ref::<CFBoolean>().map(CFBoolean::as_bool))
            .unwrap_or(false)
    }

    fn mouse_down(&self) -> bool {
        NSEvent::pressedMouseButtons() & 1 != 0
    }

    fn mouse_location(&self) -> (f64, f64) {
        let p = NSEvent::mouseLocation();
        (p.x, screen::primary_height() - p.y)
    }
}

/// Screen geometry from AppKit (ruling 3): global rectangles with a top-left origin, the
/// coordinate space AX positions use. Main thread only: off it every answer is the empty one.
pub mod screen {
    use super::Rect;
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSScreen, NSView, NSWindow};

    /// An AppKit rectangle (origin bottom-left of the primary display, y up) in AX terms
    /// (origin top-left of the primary display, y down).
    pub fn flip_y(primary_h: f64, r: Rect) -> Rect {
        Rect { x: r.x, y: primary_h - r.y - r.h, w: r.w, h: r.h }
    }

    /// The primary display's height in points (the first screen is the one with the menu bar).
    /// Zero off the main thread or without a display.
    pub fn primary_height() -> f64 {
        primary_frame().map_or(0.0, |r| r.h)
    }

    /// The primary display's frame, AppKit coordinates (its origin is always 0,0).
    pub fn primary_frame() -> Option<Rect> {
        let mtm = MainThreadMarker::new()?;
        let screens = NSScreen::screens(mtm);
        let f = screens.firstObject()?.frame();
        Some(Rect { x: f.origin.x, y: f.origin.y, w: f.size.width, h: f.size.height })
    }

    /// The right edge of the union of every display, in points (x is the same in AppKit and AX
    /// terms): a window parked past it shows on no display.
    pub fn screens_right() -> f64 {
        let Some(mtm) = MainThreadMarker::new() else { return 0.0 };
        NSScreen::screens(mtm).iter().map(|s| { let f = s.frame(); f.origin.x + f.size.width }).fold(0.0, f64::max)
    }

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

    /// The project window's title; the process's other windows (the "Open recent..." chooser, a
    /// folder picker) have their own.
    pub const WINDOW_TITLE: &str = "Arkos IDE";

    /// The arkos project window when its handle was not available at setup (`ns_window_of`):
    /// the one titled `WINDOW_TITLE`, else the main or key window, else
    /// the first one.
    pub fn arkos_window() -> Option<Retained<NSWindow>> {
        let mtm = MainThreadMarker::new()?;
        let windows = NSApplication::sharedApplication(mtm).windows();
        let all: Vec<_> = windows.iter().collect();
        all.iter()
            .find(|w| w.title().to_string() == WINDOW_TITLE)
            .or_else(|| all.iter().find(|w| w.isMainWindow() || w.isKeyWindow()))
            .or(all.first())
            .cloned()
    }

    /// The window's content view in global, top-left coordinates. A gpui element's bounds
    /// (points, relative to the content view) add to its origin.
    pub fn content_rect_in_screen(window: &NSWindow) -> Rect {
        let content = match window.contentView() {
            Some(view) => window.convertRectToScreen(view.frame()),
            None => window.frame(),
        };
        let r = Rect { x: content.origin.x, y: content.origin.y, w: content.size.width, h: content.size.height };
        flip_y(primary_height(), r)
    }
}

/// A stand-in for the API with the same answers, for the docking engine's tests.
#[cfg(test)]
pub struct FakeAx {
    pub windows: RefCell<Vec<FakeWindow>>,
    pub trusted: Cell<bool>,
    pub mouse_down: Cell<bool>,
    pub mouse: Cell<(f64, f64)>,
    pub pid: Cell<Option<i32>>,
    /// The window list cannot be read (Chrome busy).
    pub windows_fail: Cell<bool>,
    calls: RefCell<Vec<String>>,
    next: Cell<usize>,
}

#[cfg(test)]
pub struct FakeWindow {
    pub window: AxWindow,
    pub title: String,
    pub rect: Rect,
    pub minimized: bool,
}

/// Calls that reach the API are recorded, failed ones too (`Gone`: the real call was made and
/// answered so); a call refused for want of trust is not, since the engine stops calling once the
/// permission is gone. `title` and `minimized` answer like reads without trust: nothing.
#[cfg(test)]
impl FakeAx {
    /// Chrome runs as pid 4242, with no windows yet.
    pub fn new(trusted: bool) -> FakeAx {
        FakeAx {
            windows: RefCell::new(Vec::new()),
            trusted: Cell::new(trusted),
            mouse_down: Cell::new(false),
            mouse: Cell::new((0.0, 0.0)),
            pid: Cell::new(Some(4242)),
            windows_fail: Cell::new(false),
            calls: RefCell::new(Vec::new()),
            next: Cell::new(1),
        }
    }

    pub fn add_window(&self, title: &str, rect: Rect) -> AxWindow {
        let n = self.next.get();
        self.next.set(n + 1);
        let window = AxWindow::fake(n);
        self.windows.borrow_mut().push(FakeWindow { window: window.clone(), title: title.to_string(), rect, minimized: false });
        window
    }

    /// The window closes.
    pub fn remove(&self, w: &AxWindow) {
        self.windows.borrow_mut().retain(|f| f.window != *w);
    }

    pub fn set_trusted(&self, trusted: bool) {
        self.trusted.set(trusted);
    }

    /// Chrome quits (`None`) or another Chrome process takes over; the windows stay listed.
    pub fn set_pid(&self, pid: Option<i32>) {
        self.pid.set(pid);
    }

    pub fn set_windows_fail(&self, fail: bool) {
        self.windows_fail.set(fail);
    }

    pub fn set_mouse_down(&self, down: bool) {
        self.mouse_down.set(down);
    }

    /// Where the pointer is, global top-left coordinates.
    pub fn set_mouse(&self, x: f64, y: f64) {
        self.mouse.set((x, y));
    }

    /// The window's active Chrome tab changes.
    pub fn set_title(&self, w: &AxWindow, title: &str) {
        self.with(w, |f| f.title = title.to_string());
    }

    /// The user moves or resizes the window: not recorded as a call. With the button down the
    /// user's hand carries it, so the pointer goes along, onto its title bar.
    pub fn set_frame_outside(&self, w: &AxWindow, r: Rect) {
        if let Some(f) = self.windows.borrow_mut().iter_mut().find(|f| f.window == *w) {
            f.rect = r;
        }
        if self.mouse_down.get() {
            self.mouse.set((r.x + r.w / 2.0, r.y + 10.0));
        }
    }

    /// Every mutating call so far: `"set_frame <id> <x>,<y> <w>x<h>"`, `"raise <id>"` or `"focus <id>"`.
    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    fn with<T>(&self, w: &AxWindow, f: impl FnOnce(&mut FakeWindow) -> T) -> Option<T> {
        self.windows.borrow_mut().iter_mut().find(|x| x.window == *w).map(f)
    }

    fn mutate(&self, w: &AxWindow, call: String, f: impl FnOnce(&mut FakeWindow)) -> Result<(), AxError> {
        if !self.trusted.get() {
            return Err(AxError::NotTrusted);
        }
        self.calls.borrow_mut().push(call);
        self.with(w, f).ok_or(AxError::Gone)
    }
}

#[cfg(test)]
impl Ax for FakeAx {
    fn trusted(&self, _prompt: bool) -> bool {
        self.trusted.get()
    }
    fn chrome_pid(&self) -> Option<i32> {
        self.pid.get()
    }
    fn windows(&self, pid: i32) -> Result<Vec<AxWindow>, AxError> {
        if !self.trusted.get() {
            return Err(AxError::NotTrusted);
        }
        if self.windows_fail.get() {
            return Err(AxError::Other(-25204)); // kAXErrorCannotComplete
        }
        if self.pid.get() != Some(pid) {
            return Ok(Vec::new());
        }
        Ok(self.windows.borrow().iter().map(|f| f.window.clone()).collect())
    }
    fn frame(&self, w: &AxWindow) -> Option<Rect> {
        if !self.trusted.get() {
            return None;
        }
        self.with(w, |f| f.rect)
    }
    fn set_frame(&self, w: &AxWindow, r: Rect) -> Result<(), AxError> {
        let call = format!("set_frame {} {},{} {}x{}", w.id(), r.x, r.y, r.w, r.h);
        self.mutate(w, call, |f| f.rect = r)
    }
    fn raise(&self, w: &AxWindow) -> Result<(), AxError> {
        self.mutate(w, format!("raise {}", w.id()), |_| {})
    }
    fn focus(&self, w: &AxWindow) -> Result<(), AxError> {
        self.mutate(w, format!("focus {}", w.id()), |_| {})
    }
    fn title(&self, w: &AxWindow) -> Option<String> {
        if !self.trusted.get() {
            return None;
        }
        self.with(w, |f| f.title.clone())
    }
    fn minimized(&self, w: &AxWindow) -> bool {
        self.trusted.get() && self.with(w, |f| f.minimized).unwrap_or(false)
    }
    fn mouse_down(&self) -> bool {
        self.mouse_down.get()
    }
    fn mouse_location(&self) -> (f64, f64) {
        self.mouse.get()
    }
}

/// `arkos browser-probe`: run once by hand, at the screen, to confirm rulings 1 and 3 and how
/// `AXRaise` behaves while another app is active. Never run by `cargo test`.
pub mod probe {
    use super::{screen, Ax, RealAx, Rect};
    use std::io::{BufRead, Write};
    use std::time::Duration;

    const PAUSE: Duration = Duration::from_secs(2);

    fn ask(question: &str) -> Option<bool> {
        print!("{question} (y/n) ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).ok()?;
        match line.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => Some(true),
            "n" | "no" => Some(false),
            _ => None,
        }
    }

    fn show(r: Option<Rect>) -> String {
        match r {
            Some(r) => format!("{},{} {}x{}", r.x, r.y, r.w, r.h),
            None => "unknown".to_string(),
        }
    }

    fn answer(a: Option<bool>) -> &'static str {
        match a {
            Some(true) => "yes",
            Some(false) => "no",
            None => "no answer",
        }
    }

    /// Prints what it finds and the summary; always exits 0.
    pub fn run() -> i32 {
        let ax = RealAx::new();
        let trusted = ax.trusted(true);
        println!("accessibility trusted: {trusted}");
        if !trusted {
            println!("grant access in System Settings > Privacy & Security > Accessibility, then run this again");
            println!("run from a terminal, the grant goes to that terminal app, not Arkos IDE; the Arkos IDE app gets its own prompt from its first browser tab");
            return 0;
        }
        let primary = screen::primary_frame();
        println!("primary display: {}", show(primary));
        let Some(pid) = ax.chrome_pid() else {
            println!("Google Chrome is not running: open a window and run this again");
            return 0;
        };
        println!("chrome pid: {pid}");
        let windows = match ax.windows(pid) {
            Ok(w) => w,
            Err(e) => {
                println!("cannot read Chrome's windows: {e:?}; run this again");
                return 0;
            }
        };
        println!("chrome windows: {}", windows.len());
        for w in &windows {
            println!(
                "  {:?} {:?} frame {} minimized {}",
                w,
                ax.title(w).unwrap_or_default(),
                show(ax.frame(w)),
                ax.minimized(w)
            );
        }
        let Some((window, original)) = windows.iter().find(|w| !ax.minimized(w)).and_then(|w| Some((w, ax.frame(w)?))) else {
            println!("no window to move: open a Chrome window that is not minimized and run this again");
            return 0;
        };
        println!("using {:?}, frame {}", window, show(Some(original)));

        let target = Rect { x: 100.0, y: 100.0, w: 800.0, h: 500.0 };
        let moved = ax.set_frame(window, target);
        std::thread::sleep(PAUSE);
        let landed = ax.frame(window);
        println!("moved to {}: {:?}, reads back {}", show(Some(target)), moved, show(landed));

        let raised = ax.raise(window);
        println!("raised: {raised:?}");
        let above = ask("is the Chrome window now above this terminal?");

        println!("watch every display: the window goes off-screen for {} seconds", PAUSE.as_secs() * 2);
        let parked = target.offscreen(screen::screens_right());
        let hidden = ax.set_frame(window, parked);
        std::thread::sleep(PAUSE);
        let parked_at = ax.frame(window);
        std::thread::sleep(PAUSE);
        println!("moved off-screen to {}: {:?}, reads back {}", show(Some(parked)), hidden, show(parked_at));

        // Back first, so the window is not left lost while the question waits for an answer.
        let restored = ax.set_frame(window, original);
        let back = ax.frame(window);
        println!("restored to {}: {:?}, reads back {}", show(Some(original)), restored, show(back));
        let vanished = ask("while it was off-screen, had the Chrome window vanished from every display?");

        println!("probe done");
        println!("summary:");
        println!("  trusted: yes");
        println!("  move to 100,100 800x500 landed exactly (ruling 3): {}", landed.is_some_and(|r| r.distance_outside(&target) < 1.0));
        println!("  raise put Chrome above the active terminal: {}", answer(above));
        println!("  off-screen frame left alone by macOS (ruling 1): {}", parked_at.is_some_and(|r| r.distance_outside(&parked) < 1.0));
        println!("  off-screen window vanished: {}", answer(vanished));
        println!("  restored: {}", back.is_some_and(|r| r.distance_outside(&original) < 1.0));
        println!("  second size writes needed: {}", ax.second_size_writes());
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rects_contain_measure_distance_and_move_off_screen() {
        let r = Rect { x: 100.0, y: 50.0, w: 400.0, h: 300.0 };
        assert!(r.contains(100.0, 50.0) && r.contains(499.0, 349.0) && !r.contains(500.0, 50.0));
        assert_eq!(r.distance_outside(&r), 0.0);
        assert_eq!(r.distance_outside(&Rect { x: 160.0, y: 50.0, w: 400.0, h: 300.0 }), 60.0, "shifted right by 60");
        assert_eq!(r.distance_outside(&Rect { x: 100.0, y: 10.0, w: 400.0, h: 300.0 }), 40.0, "up by 40");
        assert_eq!(r.offscreen(1512.0), Rect { x: 1512.0 + 64.0, y: 50.0, w: 400.0, h: 300.0 });
    }

    #[test]
    fn screen_rect_flips_y_against_the_primary_display() {
        // AppKit: origin bottom-left of the primary display, y up. AX: top-left, y down.
        let appkit = Rect { x: 200.0, y: 100.0, w: 800.0, h: 600.0 }; // bottom edge 100 above the bottom
        let flipped = screen::flip_y(982.0, appkit);
        assert_eq!(flipped, Rect { x: 200.0, y: 982.0 - 100.0 - 600.0, w: 800.0, h: 600.0 });
        // A window on a display to the left of the primary keeps a negative x.
        assert_eq!(screen::flip_y(982.0, Rect { x: -1440.0, y: 0.0, w: 1440.0, h: 900.0 }).x, -1440.0);
    }

    #[test]
    fn the_fake_records_calls_and_answers_like_the_real_thing() {
        let ax = FakeAx::new(true);
        assert_eq!(ax.chrome_pid(), Some(4242));
        let a = ax.add_window("Claude", Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 });
        let b = ax.add_window("Docs", Rect { x: 5.0, y: 5.0, w: 10.0, h: 10.0 });
        assert_ne!(a, b);
        assert_eq!(ax.windows(4242), Ok(vec![a.clone(), b.clone()]));
        assert_eq!(ax.title(&a).as_deref(), Some("Claude"));
        ax.set_frame(&a, Rect { x: 1.0, y: 2.0, w: 3.0, h: 4.0 }).unwrap();
        assert_eq!(ax.frame(&a), Some(Rect { x: 1.0, y: 2.0, w: 3.0, h: 4.0 }));
        ax.remove(&a);
        assert_eq!(ax.frame(&a), None);
        assert_eq!(ax.set_frame(&a, Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }), Err(AxError::Gone));
        assert!(ax.calls().iter().any(|c| c.starts_with("set_frame")));
        let untrusted = FakeAx::new(false);
        assert!(!untrusted.trusted(false));
        assert_eq!(untrusted.set_frame(&b, Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }), Err(AxError::NotTrusted));
    }
}
