# Browser Pane (Chrome Docking) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A browser tab whose content area is a placeholder over which a real Chrome window is kept positioned through the macOS Accessibility API, docked automatically when Claude opens one, or by URL, by palette, or by dragging a window in; undocked by closing the tab or dragging out.

**Architecture:** `ax.rs` wraps the handful of Accessibility C functions (trust check, the Chrome application element, its windows, position and size, raise) behind an `Ax` trait with a fake for tests, plus the geometry that turns a placeholder's rectangle into global screen coordinates. `dock.rs` is a pure engine: each tick it gets a snapshot (Chrome windows, placeholders, mouse and window state) and returns actions; the window applies them through `Ax`. `Tab::Browser` is persisted with its URL; docking state is runtime only. Drag in and out reuse the five-zone overlay and the tab drag of milestone 2.

**Tech Stack:** Rust 2021, gpui 0.2.2 (unchanged), `objc2-app-kit` 0.3.2 (NSEvent, NSRunningApplication, NSApplication, NSWindow, NSScreen) and `objc2-core-foundation` 0.3.2 for the CF types the AX functions take, the ApplicationServices framework linked for `AXUIElement*` and `AXIsProcessTrustedWithOptions`; `open` to launch Chrome.

**Spec:** `docs/superpowers/specs/2026-10-13-arkos-design.md`, section 8 (with section 4's tab and drop rules, section 5's working state, section 7.1's motion rules).

## Global Constraints

- Machine rules (the user's MacBook Air locked up from build load): every cargo call is `nice -n 15 ~/.cargo/bin/cargo ...`; the worktree's local `.cargo/config.toml` (`jobs = 3`, shared `target-dir`) stays; no `--release` builds on the laptop; never two cargo processes at once; focused filters while iterating, the workspace suite once per task; the warning check is `cargo check --workspace --all-targets`.
- New dependencies, only these: `objc2-app-kit = { version = "0.3.2", features = ["NSEvent", "NSRunningApplication", "NSApplication", "NSWindow", "NSScreen", "NSResponder", "NSGraphics"] }` and `objc2-core-foundation = { version = "0.3.2" }` (both already in the registry from gpui's own tree; add the features the compiler asks for and no others); the ApplicationServices framework linked by a `#[link(name = "ApplicationServices", kind = "framework")]` block in `ax.rs`. No gpui bump. No AX bindings crate.
- The Accessibility API is called only on the UI thread (AX elements are not `Send`), from a timer tick or an event handler, never in a loop that blocks: at most one `AXWindows` read plus one position/size write per docked window per tick.
- Polling cadence (spec 8 "Identification", "Tracking"): 1 Hz while any browser tab waits for a window; 20 Hz while any docked window is visible or the mouse button is down with a browser tab present; nothing otherwise. Under Reduce Motion nothing changes (there is no animation).
- The docked window is identified by its AX element (`CFEqual`), never by its title. Tab titles follow the Chrome window's title.
- Nothing is sent to Chrome except window geometry, raise, and launching a window with a URL the user typed or configured. arkos never closes a Chrome window.
- The unit is a Chrome window, never a Chrome tab. One browser tab per docked window.
- No em-dashes in any file. Warning-free. Headless tests only: the `Ax` trait's fake stands in for the real API; nothing in `cargo test` touches Accessibility.

## Review Focus

1. Accessibility permission revoked while a window is docked: every AX call starts failing; the pane returns to the permission screen and the tracking stops, rather than logging an error 20 times a second (Task 3 `a_failing_ax_call_puts_the_tab_back_on_the_permission_screen`).
2. Chrome quits, or the user closes the docked window with Cmd+Shift+W: the element vanishes from the window list; the tab goes back to waiting with its URL kept, nothing panics (Task 3 `a_docked_window_that_vanishes_leaves_the_tab_waiting`).
3. Two browser tabs waiting when one Chrome window appears: exactly one docks, the first in reading order (Task 3 `one_new_window_docks_into_the_first_waiting_tab_only`).
4. The arkos window moves to another display or the sidebar toggles: the docked window follows within one tick, in global coordinates measured from the primary display's top-left (Task 1 `screen_rect_flips_y_against_the_primary_display` and Task 3 `a_moved_placeholder_moves_the_window`).
5. A window the user is dragging out of a docked window's tab strip (Chrome's own gesture) must not be re-captured by the auto-dock rule while the mouse is down (Task 3 `windows_that_appear_while_the_mouse_is_down_are_not_auto_docked`).

## File structure

- `crates/arkos/src/ax.rs` (new): the FFI block, `AxWindow` (a retained element with `PartialEq` by `CFEqual`), the `Ax` trait (`trusted(prompt)`, `chrome_pid()`, `windows(pid)`, `frame(&win)`, `set_frame(&win, rect)`, `raise(&win)`, `title(&win)`, `minimized(&win)`), `RealAx`, `FakeAx` (test), `Rect`, `screen_rect_of(window content rect in gpui points, NSWindow)`, the flip helper.
- `crates/arkos/src/dock.rs` (new): `DockState::{NoPermission, Waiting { since }, Docked { win, visible_rect: Option<Rect> }}`, `Snapshot`, `Action`, `tick(&mut Docks, &Snapshot) -> Vec<Action>`, the cadence helper `poll_every(&Docks, mouse_down) -> Option<Duration>`, the auto-dock rule, drag detection (Task 4).
- `crates/arkos/src/tree.rs`: `Tab::Browser { url: Option<String>, app_mode: bool }`. `workspace.rs`: round trip. `pane_view.rs`: the placeholder, the three screens, the slim tab bar. `main.rs`: `Root.docks`, the tick timer, the palette commands, the URL prompt, applying actions, focus re-raise. `keymap.rs`, `command_palette.rs`, `config.rs` (`[browser] url`, `app_mode`), `drag.rs` (Task 4), `README.md`, the spec.

## Rulings taken while planning

1. Hiding a docked window (tab inactive, arkos window minimized or hidden) moves it off every display, to `x = primary width + 64` at its current y and size, and remembers where it was; showing it moves it back onto the placeholder. `AXMinimized` would animate to the Dock on every tab switch and app hiding would hide all of Chrome. The probe (Task 1) checks macOS leaves an off-screen window alone.
2. Drag-in detection uses polling (20 Hz while the mouse button is down and a browser pane exists) of the Chrome windows' frames, not an `AXObserver`: a window whose frame changes between two ticks while the button is down is being dragged. No run-loop source to manage, and the same loop serves tracking.
3. Geometry comes from AppKit directly (the arkos `NSWindow`'s content rect converted to screen coordinates, flipped against the primary display's height), not from gpui's `window_bounds()`, whose macOS conversion is relative to the window's own screen. gpui supplies only the placeholder's rectangle inside the content view.
4. The Accessibility permission prompt for a development binary can only be answered by the user at the screen. Task 1 ships a probe (`arkos browser-probe`) that the user runs once; its results confirm rulings 1 and 3 and the raise behaviour. Tasks 2 to 5 do not wait for it: the engine is pure and tested with `FakeAx`.
5. A window the user asks for by URL is launched with `open -na "Google Chrome" --args --new-window <url>` (or `--app=<url>` for "single page"), and the first new Chrome window to appear within 10 s docks into that tab regardless of the working state; after 10 s the tab waits like any other.
6. Persisted state is the URL and the app-mode flag only. After a relaunch a browser tab waits; it never tries to find the old window by title.
7. The slim tab bar (spec 8 "Vertical space") is 14 px: the pane number and one dot per tab, no titles, the active browser tab's title shown by Chrome itself. It stays a drop target for tab drags.

---

### Task 1: The Accessibility layer and the probe

**Files:**
- Create: `crates/arkos/src/ax.rs`
- Modify: `crates/arkos/Cargo.toml` (the two dependencies), `Cargo.lock`, `crates/arkos/src/main.rs` (`mod ax;`, the `browser-probe` subcommand next to `hooks`/`notify`)
- Test: unit tests in `ax.rs` (geometry, the fake)

**Interfaces:**
- Produces: `ax::{Rect { x: f64, y: f64, w: f64, h: f64 }, Rect::{contains(x, y), distance_outside(&self, other) -> f64, offscreen(primary_w) -> Rect}, AxWindow (Clone, PartialEq, Debug; `id() -> usize` for logs), Ax trait { fn trusted(&self, prompt: bool) -> bool; fn chrome_pid(&self) -> Option<i32>; fn windows(&self, pid: i32) -> Vec<AxWindow>; fn frame(&self, w: &AxWindow) -> Option<Rect>; fn set_frame(&self, w: &AxWindow, r: Rect) -> Result<(), AxError>; fn raise(&self, w: &AxWindow) -> Result<(), AxError>; fn title(&self, w: &AxWindow) -> Option<String>; fn minimized(&self, w: &AxWindow) -> bool; fn mouse_down(&self) -> bool; fn mouse_location(&self) -> (f64, f64) /* global, top-left origin */ }, AxError::{NotTrusted, Gone, Other(i32)}, RealAx, FakeAx { windows: RefCell<Vec<FakeWindow>>, trusted, mouse_down, mouse, pid } with FakeAx::{add_window(title, rect) -> AxWindow, remove(&AxWindow), set_frame_outside(&AxWindow, Rect), calls() -> Vec<String>}, screen::{content_rect_in_screen(window: &NSWindow) -> Rect /* top-left origin, global */, flip_y(primary_h, appkit_rect) -> Rect, primary_height()}, probe::run() -> i32`.

- [ ] **Step 1: Write the failing tests**

In `crates/arkos/src/ax.rs`:

```rust
//! The macOS Accessibility API, as much of it as docking needs (spec 8): the trust check, the
//! Chrome application's windows, their frames, moving and raising them. Behind a trait so the
//! docking engine is tested with a fake; nothing in `cargo test` touches the real API.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
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
        assert_eq!(ax.windows(4242), vec![a.clone(), b.clone()]);
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
```

- [ ] **Step 2: Run them to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos -- ax::`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

The FFI (all of it; the C signatures are from `HIServices/AXUIElement.h` and `AXValue.h`):

```rust
use objc2_core_foundation::{CFArray, CFBoolean, CFDictionary, CFRetained, CFString, CFType, Type};
use std::cell::RefCell;
use std::ffi::c_void;

pub type AXError = i32;
pub const AX_SUCCESS: AXError = 0;
/// `kAXErrorAPIDisabled`: the process is not trusted.
pub const AX_API_DISABLED: AXError = -25211;
/// `kAXErrorInvalidUIElement`: the window is gone.
pub const AX_INVALID_ELEMENT: AXError = -25202;
const AX_VALUE_CGPOINT: u32 = 1;
const AX_VALUE_CGSIZE: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    static kAXTrustedCheckOptionPrompt: *const CFString;
    fn AXIsProcessTrustedWithOptions(options: *const CFDictionary) -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> *mut c_void;
    fn AXUIElementCopyAttributeValue(element: *mut c_void, attribute: *const CFString, value: *mut *mut c_void) -> AXError;
    fn AXUIElementSetAttributeValue(element: *mut c_void, attribute: *const CFString, value: *const c_void) -> AXError;
    fn AXUIElementPerformAction(element: *mut c_void, action: *const CFString) -> AXError;
    fn AXValueCreate(kind: u32, value: *const c_void) -> *mut c_void;
    fn AXValueGetValue(value: *mut c_void, kind: u32, out: *mut c_void) -> bool;
    fn CFRetain(cf: *const c_void) -> *const c_void;
    fn CFRelease(cf: *const c_void);
    fn CFEqual(a: *const c_void, b: *const c_void) -> bool;
    fn CFGetTypeID(cf: *const c_void) -> usize;
    fn CFArrayGetTypeID() -> usize;
}
```

`AxWindow`: a newtype over `*mut c_void` (the retained `AXUIElementRef`); `Clone` calls `CFRetain`, `Drop` calls `CFRelease`, `PartialEq` calls `CFEqual`, `Debug` prints the pointer; `id()` is the pointer as `usize`. Not `Send` (no `unsafe impl`). The real implementation:

- `trusted(prompt)`: a `CFDictionary` with `kAXTrustedCheckOptionPrompt => CFBoolean(prompt)` passed to `AXIsProcessTrustedWithOptions` (build the dictionary with `CFDictionary::from_slices` or the `objc2_core_foundation` constructor available; one key).
- `chrome_pid()`: `objc2_app_kit::NSRunningApplication::runningApplicationsWithBundleIdentifier(ns_string!("com.google.Chrome"))`, the first app's `processIdentifier()`.
- `windows(pid)`: `AXUIElementCreateApplication(pid)`, `AXUIElementCopyAttributeValue(app, "AXWindows", &mut out)`; if `out` is a `CFArray` (type id check), each element retained into an `AxWindow`; the app element released. A `kAXErrorAPIDisabled` result yields an empty list (the engine treats "no permission" through `trusted`, read every tick).
- `frame(w)`: `AXPosition` and `AXSize` through `AXValueGetValue` with the point and size kinds; `None` on any error.
- `set_frame(w, r)`: `AXValueCreate` a point and a size, `AXUIElementSetAttributeValue` for `AXPosition` then `AXSize` (set the size before the position when the window grows, so Chrome does not clamp the position against the old size: do size, then position, then size again when the first size write changed the size; the probe reports whether the second write was ever needed); map `AX_API_DISABLED` to `NotTrusted`, `AX_INVALID_ELEMENT` to `Gone`, the rest to `Other(code)`.
- `raise(w)`: `AXUIElementPerformAction(w, "AXRaise")`.
- `title(w)`: `AXTitle` as a `CFString` to `String`.
- `minimized(w)`: `AXMinimized` as a `CFBoolean`.
- `mouse_down()`: `objc2_app_kit::NSEvent::pressedMouseButtons() & 1 != 0`.
- `mouse_location()`: `NSEvent::mouseLocation()` flipped against `screen::primary_height()`.

`screen`: `primary_height()` is `NSScreen::screens(mtm)[0].frame().size.height` (the first screen is the primary one; `MainThreadMarker::new()` since this runs on the UI thread); `flip_y(primary_h, r)` returns `Rect { x: r.x, y: primary_h - r.y - r.h, w: r.w, h: r.h }`; `content_rect_in_screen(window)` takes the arkos `NSWindow` (found as `NSApplication::sharedApplication(mtm).windows()` entry whose `isMainWindow` or `isKeyWindow` holds, falling back to the first; one window per process), calls `contentView().frame()` converted with `window.convertRectToScreen(..)` and flips it. The placeholder's global rect is then `content.x + bounds.origin.x`, `content.y + bounds.origin.y` with the gpui bounds in points (gpui's `Bounds<Pixels>` inside the content view are points on macOS).

`FakeAx`: `RefCell<Vec<(AxWindowFake, String title, Rect, bool minimized)>>` where the fake's `AxWindow` is built from a counter (the real newtype has a `#[cfg(test)] fn fake(n: usize) -> AxWindow` that stores `n` as the pointer and skips retain/release for fakes: keep a `fake: bool` field); `calls()` records each mutating call as `"set_frame <id> <x>,<y> <w>x<h>"` or `"raise <id>"`.

The probe: `arkos browser-probe` in `main.rs`'s argument handling, implemented in `ax::probe::run()`: prints whether the process is trusted (prompting once), the Chrome pid and its windows with titles and frames; if a window exists: records its frame, moves it to `Rect { x: 100, y: 100, w: 800, h: 500 }` for 2 s, raises it while this terminal is the active app and asks "is Chrome now above the terminal? (y/n)", moves it off-screen (`offscreen`) for 2 s and asks whether it vanished, moves it back to the recorded frame, and prints "probe done" plus a summary line per check. Exit 0. The README's checklist (Task 5) tells the user to run it once from `/Applications/Arkos IDE.app/Contents/MacOS/arkos browser-probe` and to grant Accessibility access when asked. The results settle rulings 1 and 3 and the raise behaviour; the implementer of Task 3 reads them from the ledger if present and otherwise builds to the spec.

- [ ] **Step 4: Run the tests and the warning check**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos -- ax::` then `nice -n 15 ~/.cargo/bin/cargo check --workspace --all-targets 2>&1 | grep -i "^warning"`.
Expected: PASS; items unused until Task 3 carry `#[allow(dead_code)] // used from Task 3`.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/Cargo.toml Cargo.lock crates/arkos/src/ax.rs crates/arkos/src/main.rs
git commit -m "Wrap the Accessibility API for Chrome docking, with a fake for tests and a probe command"
```

---

### Task 2: The browser tab: model, persistence, placeholder, screens, slim bar

**Files:**
- Modify: `crates/arkos/src/tree.rs` (`Tab::Browser`), `crates/arkos/src/workspace.rs` (tests), `crates/arkos/src/pane_view.rs` (`render_browser`, the slim bar), `crates/arkos/src/main.rs` (`Command::NewBrowserTab`, `NewBrowserTabSinglePage`, the URL prompt, `Root.browser_rects`, `report_browser_rect`), `crates/arkos/src/keymap.rs`, `crates/arkos/src/command_palette.rs`, `crates/arkos/src/config.rs` (`[browser] url`, `app_mode`), `crates/arkos/src/closed_stack.rs` (`Closed::Browser { url, app_mode }`)
- Test: unit tests in `tree.rs`, `workspace.rs`, `pane_view.rs` (pure helpers), `keymap.rs`, `config.rs`, `closed_stack.rs`

**Interfaces:**
- Consumes: Task 1's `ax::Rect` (for the placeholder rectangle report only).
- Produces: `Tab::Browser { url: Option<String>, #[serde(default)] app_mode: bool }`; `pane_view::{BrowserScreen::{NoPermission, Waiting { url: Option<String>, launched: bool }, Docked}, slim_bar_height() -> f32 = 14.0, TAB_BAR_H unchanged for other tabs, bar_height_for(active: Option<&Tab>) -> f32}`; `Root.browser_rects: HashMap<TabId, (Bounds<Pixels>, Instant)>` filled by a `canvas` in the placeholder every frame; `Root::new_browser_tab(pane, url: Option<String>, app_mode: bool, cx)`; `Command::{NewBrowserTab, NewBrowserTabSinglePage}` ("new browser tab" cmd-shift-b, "new single-page browser tab" unbound); the URL prompt `UrlPrompt { edit: LineEdit, app_mode: bool }` reusing `line_edit::render`, Enter with an empty field opens the configured `[browser] url` (empty means a blank window), Escape cancels; `config::BrowserConfig { url: String, app_mode: bool }` (`app_mode` default false); `Closed::Browser { url, app_mode }` reopened by Cmd+Shift+T as a waiting tab.

- [ ] **Step 1: Write the failing tests**

`tree.rs` tests, next to the Terminal/Viewer ones:

```rust
    #[test]
    fn a_browser_tab_round_trips_and_an_old_file_without_app_mode_reads_false() {
        let t = Tab::Browser { url: Some("https://example.com".into()), app_mode: true };
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<Tab>(&json).unwrap(), t);
        let old: Tab = serde_json::from_str(r#"{"Browser":{"url":null}}"#).unwrap();
        assert_eq!(old, Tab::Browser { url: None, app_mode: false });
    }
```

`workspace.rs`: `round_trips_a_layout` gains a browser tab in the tree literal and asserts it survives.

`pane_view.rs` tests:

```rust
    #[test]
    fn the_tab_bar_is_slim_only_while_a_browser_tab_is_active() {
        assert_eq!(bar_height_for(Some(&Tab::Browser { url: None, app_mode: false })), slim_bar_height());
        assert_eq!(bar_height_for(Some(&Tab::Terminal { session: SessionId(1), cwd: None })), TAB_BAR_H);
        assert_eq!(bar_height_for(None), TAB_BAR_H);
        assert!(slim_bar_height() < TAB_BAR_H);
    }

    #[test]
    fn the_browser_screen_follows_permission_then_docking() {
        assert_eq!(browser_screen(false, None, None, false), BrowserScreen::NoPermission);
        assert_eq!(browser_screen(true, None, Some("https://x".into()), true), BrowserScreen::Waiting { url: Some("https://x".into()), launched: true });
        assert_eq!(browser_screen(true, Some(()), None, false), BrowserScreen::Docked);
    }
```

(`browser_screen(trusted: bool, docked: Option<()>, url: Option<String>, launched: bool) -> BrowserScreen` is the pure chooser the renderer calls; Task 3 passes the real dock state through it.)

`keymap.rs`'s `SPEC_ROWS` gains `("cmd-shift-b", "new browser tab", "NewBrowserTab")` and `("", "new single-page browser tab", "NewBrowserTabSinglePage")` (unbound count up by one; check `cmd-shift-b` is free: `grep cmd-shift-b` in the config comment must find nothing before this task). `config.rs`: `the_browser_table_defaults_and_accepts_app_mode` (`url` default `""`, `app_mode` default false, unknown keys refused). `closed_stack.rs`: a `Closed::Browser` entry round trips through `push`/`take` like a viewer.

- [ ] **Step 2: Run them to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos -- browser bar_height workspace keymap closed_stack`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`tree.rs`: the variant, appended; every exhaustive `match` on `Tab` across the crate gains an arm (`tab_labels` titles the tab from `Root.browser_titles.get(id)` when docked, else the URL's host or "browser"; `restart`/`reconcile` ignore it like viewers; `closed_stack` records it; `terminals_inside` ignores it).

`pane_view.rs`:
- `bar_height_for(active)`; `render_tab_bar` takes the height and, when slim, draws only the pane number and one 6 px dot per tab (`tab_active` for the active one, `text_dim` otherwise), keeps every drag listener, and skips titles and the overflow menu.
- `render_browser(root, pane, tab, url, app_mode, cx)`: a `div().size_full().relative()` with a `canvas` that reports its bounds each frame (`root.browser_rects.insert(tab, (bounds, now))`), then the screen by `browser_screen(..)`:
  - `NoPermission`: a centred column: "Docking a Chrome window needs Accessibility access", two lines of explanation ("arkos positions the window over this pane; it never reads its contents"), a button "Grant access" (`root.request_accessibility(cx)`: Task 3 wires `ax.trusted(true)`; here it calls a `Root` method that Task 3 fills, stubbed to a notice "coming in the next task") and a button "Open System Settings" (`cx.open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")`).
  - `Waiting`: "Waiting for a Chrome window" with the URL under it when set, and two buttons: "Dock a Chrome window…" (the palette command of Task 3; here `Command::DockChromeWindow` dispatched through `run`, which Task 3 implements; Task 2 registers the command as a no-op that shows a notice) and "Open a URL…" (the prompt). A first-run tip in `text_dim`: "Tip: right-click Chrome's tab strip and choose Show Tabs Vertically to save 40 px."
  - `Docked`: an empty area in `chrome.window_bg` (the Chrome window covers it).
- Clicking anywhere in the placeholder calls `root.browser_clicked(tab, cx)` (Task 3 raises the window; Task 2 stub does nothing).

`main.rs`: `Root.browser_rects`, `Root.browser_titles: HashMap<TabId, String>`, `Root.url_prompt: Option<UrlPrompt>` rendered like the worktree prompt (a small floating box at the top of the focused pane) with `key_context("UrlPrompt")`; `new_browser_tab(pane, url, app_mode, cx)` adds `Tab::Browser`, activates it, saves; `Command::NewBrowserTab` opens the prompt with `app_mode` from config; `NewBrowserTabSinglePage` the same with `app_mode = true`; closing a browser tab pushes `Closed::Browser`. `config.rs`: `BrowserConfig { url, app_mode }` with the comment block updated ("Milestone 5" text replaced by what it does). The palette items and keymap registration as in every earlier task.

- [ ] **Step 4: Run the tests**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos`, the warning check, `grep -n "—" crates/arkos/src/*.rs`.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/tree.rs crates/arkos/src/workspace.rs crates/arkos/src/pane_view.rs crates/arkos/src/main.rs crates/arkos/src/keymap.rs crates/arkos/src/command_palette.rs crates/arkos/src/config.rs crates/arkos/src/closed_stack.rs
git commit -m "Add the browser tab: persisted URL, placeholder with its screens, the slim tab bar, the URL prompt"
```

---

### Task 3: The docking engine: auto-dock, palette dock and undock, tracking, hide and show, focus, URL launch

**Files:**
- Create: `crates/arkos/src/dock.rs`
- Modify: `crates/arkos/src/main.rs` (`Root.docks`, `Root.ax: Rc<dyn Ax>`, the tick timer, `apply_actions`, `Command::{DockChromeWindow, UndockChromeWindow}`, the dock picker list in the palette, `browser_clicked`, `request_accessibility`, re-raise on activation, undock on tab close), `crates/arkos/src/pane_view.rs` (`browser_screen` fed from `docks`), `crates/arkos/src/keymap.rs`, `crates/arkos/src/command_palette.rs`
- Test: unit tests in `dock.rs` (the engine with `FakeAx` snapshots)

**Interfaces:**
- Consumes: Task 1's `Ax`, `AxWindow`, `Rect`, `FakeAx`, `AxError`; Task 2's `Tab::Browser`, `browser_rects`, `browser_titles`, `browser_screen`, `Command` stubs.
- Produces: `dock::{DockState::{NoPermission, Waiting { since: Instant, launched_at: Option<Instant> }, Docked { win: AxWindow, shown: Option<Rect> /* where it is when visible */, parked: Option<Rect> /* where it was before hiding */ }}, Docks { tabs: BTreeMap<TabId, DockState> /* reading order supplied by the caller */, known: Vec<AxWindow> /* last tick's Chrome windows */, last_mouse_down: bool }, Snapshot { trusted: bool, windows: Vec<(AxWindow, Option<Rect>, bool /*minimized*/)>, placeholders: Vec<(TabId, Option<Rect> /* None when the tab is not visible */)>, any_working: bool, mouse_down: bool, window_active: bool, window_hidden: bool /* minimized or hidden */, now: Instant }, Action::{Dock { tab: TabId, win: AxWindow }, Move { win: AxWindow, rect: Rect }, Hide { win: AxWindow, to: Rect }, Show { win: AxWindow, rect: Rect }, Raise(AxWindow), Undock { tab: TabId, win: Option<AxWindow>, leave_at: Option<Rect> }, Title { tab: TabId, title: String }, Permission(bool)}, Docks::{tick(&mut self, s: &Snapshot) -> Vec<Action>, cadence(&self, mouse_down: bool) -> Option<Duration>, add(tab, launched: bool, now), remove(tab) -> Option<AxWindow>, dock_by_choice(tab, win), undock(tab) -> Option<AxWindow>, docked_window(tab) -> Option<&AxWindow>, waiting_tabs() -> Vec<TabId>}`, `LAUNCH_GRACE: Duration = 10 s`, `OFFSCREEN_GAP: f64 = 64.0`, `Root::{dock_tick(cx), apply_actions(actions, cx), start_dock_timer(cx), request_accessibility(cx), browser_clicked(tab, cx), dock_window_into(tab, win, cx), undock_tab(tab, cx)}`, `Command::{DockChromeWindow, UndockChromeWindow}` ("dock a chrome window", "undock chrome window", both unbound).

- [ ] **Step 1: Write the failing tests**

`crates/arkos/src/dock.rs`:

```rust
//! The docking engine (spec 8): pure over a per-tick snapshot of Chrome's windows and the
//! browser tabs' placeholders, it decides what to dock, move, hide, show, raise or undock; the
//! window applies the actions through `Ax`. Nothing here calls the Accessibility API.

use crate::ax::{AxWindow, Rect};
use crate::tree::TabId;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub const LAUNCH_GRACE: Duration = Duration::from_secs(10);
pub const OFFSCREEN_GAP: f64 = 64.0;
pub const WAIT_POLL: Duration = Duration::from_secs(1);
pub const TRACK_POLL: Duration = Duration::from_millis(50);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ax::FakeAx;

    fn snap(ax: &FakeAx, placeholders: Vec<(TabId, Option<Rect>)>, now: Instant) -> Snapshot {
        let pid = ax.chrome_pid().unwrap();
        Snapshot {
            trusted: ax.trusted(false),
            windows: ax.windows(pid).into_iter().map(|w| { let f = ax.frame(&w); let m = ax.minimized(&w); (w, f, m) }).collect(),
            placeholders,
            any_working: false,
            mouse_down: ax.mouse_down(),
            window_active: true,
            window_hidden: false,
            now,
        }
    }
    const R: Rect = Rect { x: 300.0, y: 200.0, w: 800.0, h: 600.0 };
    const R2: Rect = Rect { x: 1200.0, y: 200.0, w: 500.0, h: 600.0 };

    #[test]
    fn a_waiting_tab_docks_the_first_new_window_while_a_session_works() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(TabId(1), false, t0);
        let _old = ax.add_window("Old", Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 });
        let mut s = snap(&ax, vec![(TabId(1), Some(R))], t0);
        s.any_working = true;
        assert_eq!(d.tick(&s), vec![], "a window that was already open is not docked");
        let new = ax.add_window("Claude", Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 });
        let mut s = snap(&ax, vec![(TabId(1), Some(R))], t0 + Duration::from_secs(1));
        s.any_working = true;
        let actions = d.tick(&s);
        assert_eq!(actions[0], Action::Dock { tab: TabId(1), win: new.clone() });
        assert!(actions.contains(&Action::Move { win: new.clone(), rect: R }));
        assert!(actions.contains(&Action::Title { tab: TabId(1), title: "Claude".into() }));
        assert_eq!(d.docked_window(TabId(1)), Some(&new));
        // Not working: a further new window is ignored.
        let other = ax.add_window("Other", Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 });
        d.add(TabId(2), false, t0);
        let s = snap(&ax, vec![(TabId(1), Some(R)), (TabId(2), Some(R2))], t0 + Duration::from_secs(2));
        assert!(!d.tick(&s).iter().any(|a| matches!(a, Action::Dock { .. })), "no working session: nothing auto-docks");
        let _ = other;
    }

    #[test]
    fn one_new_window_docks_into_the_first_waiting_tab_only() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(TabId(5), false, t0);
        d.add(TabId(2), false, t0);
        let mut s = snap(&ax, vec![(TabId(2), Some(R)), (TabId(5), Some(R2))], t0);
        s.any_working = true;
        d.tick(&s);
        let w = ax.add_window("New", Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 });
        let mut s = snap(&ax, vec![(TabId(2), Some(R)), (TabId(5), Some(R2))], t0 + Duration::from_secs(1));
        s.any_working = true;
        let docks: Vec<_> = d.tick(&s).into_iter().filter(|a| matches!(a, Action::Dock { .. })).collect();
        assert_eq!(docks, vec![Action::Dock { tab: TabId(2), win: w }], "reading order: the placeholders' order, not the tab ids");
        assert!(d.waiting_tabs().contains(&TabId(5)));
    }

    #[test]
    fn windows_that_appear_while_the_mouse_is_down_are_not_auto_docked() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(TabId(1), false, t0);
        let mut s = snap(&ax, vec![(TabId(1), Some(R))], t0);
        s.any_working = true;
        d.tick(&s);
        ax.set_mouse_down(true);
        let w = ax.add_window("Dragged out", Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 });
        let mut s = snap(&ax, vec![(TabId(1), Some(R))], t0 + Duration::from_secs(1));
        s.any_working = true;
        assert!(d.tick(&s).is_empty());
        ax.set_mouse_down(false);
        let mut s = snap(&ax, vec![(TabId(1), Some(R))], t0 + Duration::from_secs(2));
        s.any_working = true;
        assert!(d.tick(&s).is_empty(), "it is a known window now, never a new one");
        let _ = w;
    }

    #[test]
    fn a_launched_tab_docks_the_next_window_without_a_working_session_within_the_grace() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(TabId(1), true, t0);
        d.tick(&snap(&ax, vec![(TabId(1), Some(R))], t0));
        let w = ax.add_window("Example", Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 });
        let actions = d.tick(&snap(&ax, vec![(TabId(1), Some(R))], t0 + Duration::from_secs(3)));
        assert_eq!(actions[0], Action::Dock { tab: TabId(1), win: w });
        let mut d2 = Docks::default();
        d2.add(TabId(9), true, t0);
        d2.tick(&snap(&ax, vec![(TabId(9), Some(R))], t0));
        let _late = ax.add_window("Late", Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 });
        assert!(d2.tick(&snap(&ax, vec![(TabId(9), Some(R))], t0 + Duration::from_secs(11))).is_empty(), "past the grace it waits like any tab");
    }

    #[test]
    fn a_moved_placeholder_moves_the_window_and_hiding_parks_it_off_screen() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(TabId(1), false, t0);
        let w = ax.add_window("W", R);
        d.dock_by_choice(TabId(1), w.clone());
        assert!(d.tick(&snap(&ax, vec![(TabId(1), Some(R))], t0)).contains(&Action::Move { win: w.clone(), rect: R }));
        ax.set_frame(&w, R).unwrap();
        assert!(!d.tick(&snap(&ax, vec![(TabId(1), Some(R))], t0)).iter().any(|a| matches!(a, Action::Move { .. })), "in place: no write");
        let moved = Rect { x: 350.0, ..R };
        assert!(d.tick(&snap(&ax, vec![(TabId(1), Some(moved))], t0)).contains(&Action::Move { win: w.clone(), rect: moved }));
        // The tab goes inactive: hide (park off screen); it comes back: show at the placeholder.
        let mut s = snap(&ax, vec![(TabId(1), None)], t0);
        s.primary_width = 1512.0;
        let hide = d.tick(&s);
        assert_eq!(hide, vec![Action::Hide { win: w.clone(), to: Rect { x: 1512.0 + OFFSCREEN_GAP, y: 200.0, w: 800.0, h: 600.0 } }]);
        assert!(d.tick(&s).is_empty(), "hidden once");
        let show = d.tick(&snap(&ax, vec![(TabId(1), Some(R))], t0));
        assert_eq!(show, vec![Action::Show { win: w.clone(), rect: R }]);
        // The arkos window minimized hides it too.
        let mut s = snap(&ax, vec![(TabId(1), Some(R))], t0);
        s.window_hidden = true;
        s.primary_width = 1512.0;
        assert!(matches!(d.tick(&s)[0], Action::Hide { .. }));
    }

    #[test]
    fn a_docked_window_that_vanishes_leaves_the_tab_waiting() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(TabId(1), false, t0);
        let w = ax.add_window("W", R);
        d.dock_by_choice(TabId(1), w.clone());
        d.tick(&snap(&ax, vec![(TabId(1), Some(R))], t0));
        ax.remove(&w);
        let actions = d.tick(&snap(&ax, vec![(TabId(1), Some(R))], t0 + Duration::from_secs(1)));
        assert_eq!(actions, vec![Action::Undock { tab: TabId(1), win: None, leave_at: None }]);
        assert!(d.waiting_tabs().contains(&TabId(1)));
    }

    #[test]
    fn a_failing_ax_call_puts_the_tab_back_on_the_permission_screen() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(TabId(1), false, t0);
        let w = ax.add_window("W", R);
        d.dock_by_choice(TabId(1), w.clone());
        ax.set_trusted(false);
        let mut s = snap(&ax, vec![(TabId(1), Some(R))], t0);
        s.trusted = false;
        let actions = d.tick(&s);
        assert_eq!(actions, vec![Action::Permission(false)]);
        assert!(matches!(d.tabs[&TabId(1)], DockState::NoPermission));
        assert_eq!(d.cadence(false), Some(WAIT_POLL), "it keeps checking for the grant, slowly");
        ax.set_trusted(true);
        let mut s = snap(&ax, vec![(TabId(1), Some(R))], t0 + Duration::from_secs(1));
        s.trusted = true;
        assert_eq!(d.tick(&s), vec![Action::Permission(true)]);
        assert!(matches!(d.tabs[&TabId(1)], DockState::Waiting { .. }));
    }

    #[test]
    fn the_cadence_is_slow_while_waiting_fast_while_tracking_and_off_otherwise() {
        let t0 = Instant::now();
        let mut d = Docks::default();
        assert_eq!(d.cadence(false), None);
        d.add(TabId(1), false, t0);
        assert_eq!(d.cadence(false), Some(WAIT_POLL));
        assert_eq!(d.cadence(true), Some(TRACK_POLL), "mouse down with a browser tab: drag detection");
        let ax = FakeAx::new(true);
        let w = ax.add_window("W", R);
        d.dock_by_choice(TabId(1), w);
        assert_eq!(d.cadence(false), Some(TRACK_POLL));
        d.remove(TabId(1));
        assert_eq!(d.cadence(false), None);
    }

    #[test]
    fn closing_a_tab_undocks_and_leaves_the_window_where_it_was_shown() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(TabId(1), false, t0);
        let w = ax.add_window("W", R);
        d.dock_by_choice(TabId(1), w.clone());
        d.tick(&snap(&ax, vec![(TabId(1), Some(R))], t0));
        let mut s = snap(&ax, vec![(TabId(1), None)], t0);
        s.primary_width = 1512.0;
        d.tick(&s); // hidden now
        assert_eq!(d.undock(TabId(1)), Some(w.clone()));
        // The caller moves a hidden window back to where it was shown before the hide.
        assert_eq!(d.last_shown(TabId(1)), Some(R));
    }
}
```

(`FakeAx::{set_mouse_down, set_trusted}` are added to Task 1's fake by this task; `Snapshot.primary_width: f64` is a field with a default of 1512.0 in `snap`; add it to the struct.)

- [ ] **Step 2: Run them to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos -- dock::`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

The engine:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum DockState {
    NoPermission,
    Waiting { since: Instant, launched_at: Option<Instant> },
    Docked { win: AxWindow, shown: Option<Rect>, parked: Option<Rect> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Dock { tab: TabId, win: AxWindow },
    Move { win: AxWindow, rect: Rect },
    Hide { win: AxWindow, to: Rect },
    Show { win: AxWindow, rect: Rect },
    Raise(AxWindow),
    Undock { tab: TabId, win: Option<AxWindow>, leave_at: Option<Rect> },
    Title { tab: TabId, title: String },
    Permission(bool),
}

pub struct Snapshot {
    pub trusted: bool,
    pub windows: Vec<(AxWindow, Option<Rect>, bool)>,
    pub titles: Vec<(AxWindow, String)>,
    /// Browser tabs in reading order with their placeholder's global rect, `None` when the
    /// tab is not the active one in its pane.
    pub placeholders: Vec<(TabId, Option<Rect>)>,
    pub any_working: bool,
    pub mouse_down: bool,
    pub window_active: bool,
    pub window_hidden: bool,
    pub primary_width: f64,
    pub now: Instant,
}

#[derive(Default)]
pub struct Docks {
    pub tabs: BTreeMap<TabId, DockState>,
    known: Vec<AxWindow>,
    trusted: Option<bool>,
    last_shown: BTreeMap<TabId, Rect>,
}

impl Docks {
    pub fn tick(&mut self, s: &Snapshot) -> Vec<Action> {
        let mut out = Vec::new();
        // Permission first: a change flips every tab between NoPermission and Waiting.
        if self.trusted != Some(s.trusted) {
            self.trusted = Some(s.trusted);
            for state in self.tabs.values_mut() {
                *state = if s.trusted { DockState::Waiting { since: s.now, launched_at: None } } else { DockState::NoPermission };
            }
            out.push(Action::Permission(s.trusted));
            self.known = s.windows.iter().map(|(w, _, _)| w.clone()).collect();
            return out;
        }
        if !s.trusted {
            return out;
        }
        let current: Vec<AxWindow> = s.windows.iter().map(|(w, _, _)| w.clone()).collect();
        let new: Vec<AxWindow> = current.iter().filter(|w| !self.known.contains(w)).cloned().collect();
        // A docked window that is gone: back to waiting.
        for (tab, state) in self.tabs.iter_mut() {
            if let DockState::Docked { win, .. } = state {
                if !current.contains(win) {
                    out.push(Action::Undock { tab: *tab, win: None, leave_at: None });
                    *state = DockState::Waiting { since: s.now, launched_at: None };
                }
            }
        }
        // Auto-dock: one new window into the first waiting tab (reading order), when a session
        // works or that tab launched the window within the grace; never while the mouse is down.
        if !s.mouse_down {
            let docked: Vec<AxWindow> = self.tabs.values().filter_map(|st| match st { DockState::Docked { win, .. } => Some(win.clone()), _ => None }).collect();
            let mut fresh = new.iter().filter(|w| !docked.contains(w));
            for (tab, rect) in &s.placeholders {
                let Some(DockState::Waiting { launched_at, .. }) = self.tabs.get(tab) else { continue };
                let allowed = s.any_working || launched_at.is_some_and(|t| s.now.duration_since(t) <= LAUNCH_GRACE);
                if !allowed {
                    continue;
                }
                let Some(win) = fresh.next() else { break };
                self.tabs.insert(*tab, DockState::Docked { win: win.clone(), shown: None, parked: None });
                out.push(Action::Dock { tab: *tab, win: win.clone() });
                if let Some(r) = rect {
                    out.push(Action::Move { win: win.clone(), rect: *r });
                    self.set_shown(*tab, *r);
                }
                if let Some((_, title)) = s.titles.iter().find(|(w, _)| w == win) {
                    out.push(Action::Title { tab: *tab, title: title.clone() });
                }
            }
        }
        // Tracking, hiding and showing, titles.
        let placeholder_of = |tab: TabId| s.placeholders.iter().find(|(t, _)| *t == tab).and_then(|(_, r)| *r);
        let mut shown_updates = Vec::new();
        for (tab, state) in self.tabs.iter_mut() {
            let DockState::Docked { win, shown, parked } = state else { continue };
            let visible = !s.window_hidden && placeholder_of(*tab).is_some();
            let frame = s.windows.iter().find(|(w, _, _)| w == win).and_then(|(_, f, _)| *f);
            match (visible, placeholder_of(*tab)) {
                (true, Some(target)) => {
                    if parked.is_some() {
                        out.push(Action::Show { win: win.clone(), rect: target });
                        *parked = None;
                        *shown = Some(target);
                        shown_updates.push((*tab, target));
                    } else if frame != Some(target) && *shown != Some(target) || frame.is_some_and(|f| f != target) {
                        out.push(Action::Move { win: win.clone(), rect: target });
                        *shown = Some(target);
                        shown_updates.push((*tab, target));
                    }
                }
                _ => {
                    if parked.is_none() {
                        let from = frame.or(*shown).unwrap_or(Rect { x: 0.0, y: 0.0, w: 800.0, h: 600.0 });
                        let to = from.offscreen(s.primary_width);
                        out.push(Action::Hide { win: win.clone(), to });
                        *parked = Some(from);
                    }
                }
            }
            if let Some((_, title)) = s.titles.iter().find(|(w, _)| w == win) {
                out.push(Action::Title { tab: *tab, title: title.clone() });
            }
        }
        for (tab, r) in shown_updates {
            self.last_shown.insert(tab, r);
        }
        self.known = current;
        out.retain(|a| !matches!(a, Action::Title { .. }) || true);
        out
    }
    // add, remove, dock_by_choice, undock, docked_window, waiting_tabs, last_shown, cadence, set_shown as the tests use them.
}
```

The implementer tidies the `Move` condition into one readable predicate (`needs_move(frame, shown, target)`: move when the window's actual frame differs from the target) and drops the no-op `retain`; `Title` actions are emitted only when the title changed since the last tick (keep `titles: BTreeMap<TabId, String>` in `Docks`), so the window does not notify every tick. `cadence`: `None` with no tabs; `TRACK_POLL` when any tab is `Docked` or `mouse_down`; else `WAIT_POLL` (waiting or no permission).

`main.rs` wiring:
- `Root.ax: Rc<dyn Ax>` (`RealAx` in `new`; tests of `Root` logic do not exist, the engine is tested on its own), `Root.docks: Docks`, `Root.dock_timer: bool`.
- `start_dock_timer(cx)`: a spawned loop that, each iteration, reads `docks.cadence(ax.mouse_down())`; `None` ends the loop (restarted by `add`); else sleeps that long, then `dock_tick(cx)`.
- `dock_tick(cx)`: builds the `Snapshot` on the UI thread (`ax.trusted(false)`, `chrome_pid` then `windows`, `frame` and `minimized` and `title` per window: at most a dozen AX reads a tick; placeholders from `browser_rects` whose entry is from the current frame (`now - at < 1 s`) converted with `screen::content_rect_in_screen` plus the bounds; `any_working = sessions.any_working(&tree)`; `window_hidden` from the `NSWindow`'s `isMiniaturized() || !isVisible()`; `primary_width` from `NSScreen`), calls `docks.tick`, then `apply_actions`.
- `apply_actions`: `Dock` sets `docks` (already set) and saves nothing (runtime state); `Move`/`Show`/`Hide` call `ax.set_frame` (an `Err(NotTrusted)` forces `trusted=false` on the next tick by clearing `docks.trusted`; `Err(Gone)` is left to the next tick's vanish rule); `Raise` calls `ax.raise`; `Undock { win: Some(w), leave_at }` moves the window to `leave_at` when given; `Title` updates `browser_titles` and notifies; `Permission(_)` notifies (the screens change).
- `browser_clicked(tab)`: `Raise` for its docked window.
- Activation: in `render`, when `window_active` flips to true, push `Raise` for every docked visible window (the spec's accepted flash).
- Tab close (every path that removes a tab: `close_tab`, `close_all_in_pane`, drag-out in Task 4, the pane close): `docks.remove(tab)` returns the window; `Undock { win, leave_at: docks.last_shown(tab) }` applied so a hidden window comes back where it was shown. `browser_titles` and `browser_rects` entries removed.
- `Command::DockChromeWindow`: the palette lists the Chrome windows' titles (through a `PaletteList` mode like the theme list, if one exists; otherwise a numbered notice is not enough: use the existing palette's secondary list mechanism used for "switch theme"); choosing one calls `dock_window_into(tab = the active browser tab in the focused pane, or the first waiting tab, win)`; with no waiting tab it creates a browser tab in the focused pane first. `Command::UndockChromeWindow`: undocks the focused pane's active browser tab (the tab stays, waiting).
- `request_accessibility`: `ax.trusted(true)` (prompts once) then a `dock_tick`.
- URL launch (`new_browser_tab` with a URL): `std::process::Command::new("/usr/bin/open").args(["-na", "Google Chrome", "--args", "--new-window", &url])` (or `"--app=<url>"` for app mode) with stdio null, spawned on the background executor; `docks.add(tab, launched: true, now)`. A URL without a scheme gets `https://`.
- `pane_view::browser_screen` fed from `docks.tabs.get(tab)`.

- [ ] **Step 4: Run the tests**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos -- dock:: ax:: browser`, then the workspace suite once, the warning check, the em-dash grep.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/dock.rs crates/arkos/src/ax.rs crates/arkos/src/main.rs crates/arkos/src/pane_view.rs crates/arkos/src/keymap.rs crates/arkos/src/command_palette.rs
git commit -m "Dock Chrome windows: auto-dock while a session works, dock and undock by palette, track, hide, show, raise, launch by URL"
```

---

### Task 4: Drag a Chrome window in; drag a docked window or its tab out

**Files:**
- Modify: `crates/arkos/src/dock.rs` (`DragIn`, `DragOut` detection), `crates/arkos/src/drag.rs` (`DropTarget` reused), `crates/arkos/src/main.rs` (`browser_drop: Option<(PaneId, Zone)>`, the overlay, `finish_browser_drop`, the tab drag-out path), `crates/arkos/src/pane_view.rs` (the overlay for a browser drag reuses the tint)
- Test: unit tests in `dock.rs`, `drag.rs`

**Interfaces:**
- Consumes: Task 3's engine and timer; `drag::{Zone, zone_for, zone_rect}`; the tab drag's drop handling (`Root::drop_tab`) and its split logic.
- Produces: `dock::{Drag::{None, In { win: AxWindow, pointer: (f64, f64) }, Out { tab: TabId, win: AxWindow, frame: Rect }}, Docks::detect_drag(&mut self, s: &Snapshot, prev: &Snapshot) -> Drag, OUT_THRESHOLD: f64 = 40.0}`, `Root::{browser_drop, finish_browser_drop(cx), undock_at(tab, rect, cx)}`.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn a_chrome_window_moving_while_the_button_is_down_over_arkos_is_a_drag_in() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(TabId(1), false, t0);
        let w = ax.add_window("Free", Rect { x: 0.0, y: 0.0, w: 400.0, h: 300.0 });
        let prev = snap(&ax, vec![(TabId(1), Some(R))], t0);
        ax.set_mouse_down(true);
        ax.set_frame_outside(&w, Rect { x: 320.0, y: 240.0, w: 400.0, h: 300.0 });
        ax.set_mouse(400.0, 300.0);
        let mut now = snap(&ax, vec![(TabId(1), Some(R))], t0 + Duration::from_millis(50));
        now.arkos_frame = Some(Rect { x: 200.0, y: 100.0, w: 1200.0, h: 800.0 });
        assert_eq!(d.detect_drag(&now, &prev), Drag::In { win: w.clone(), pointer: (400.0, 300.0) });
        ax.set_mouse(2000.0, 300.0);
        let mut off = snap(&ax, vec![(TabId(1), Some(R))], t0 + Duration::from_millis(100));
        off.arkos_frame = Some(Rect { x: 200.0, y: 100.0, w: 1200.0, h: 800.0 });
        assert_eq!(d.detect_drag(&off, &now), Drag::None, "pointer outside the arkos window");
    }

    #[test]
    fn a_docked_window_pulled_past_the_threshold_is_a_drag_out() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(TabId(1), false, t0);
        let w = ax.add_window("W", R);
        d.dock_by_choice(TabId(1), w.clone());
        d.tick(&snap(&ax, vec![(TabId(1), Some(R))], t0));
        ax.set_mouse_down(true);
        ax.set_frame_outside(&w, Rect { x: R.x + 20.0, ..R });
        let prev = snap(&ax, vec![(TabId(1), Some(R))], t0);
        let now = snap(&ax, vec![(TabId(1), Some(R))], t0 + Duration::from_millis(50));
        assert_eq!(d.detect_drag(&now, &prev), Drag::None, "under the threshold: tracking will snap it back on release");
        ax.set_frame_outside(&w, Rect { x: R.x + 60.0, y: R.y + 10.0, ..R });
        let now2 = snap(&ax, vec![(TabId(1), Some(R))], t0 + Duration::from_millis(100));
        assert_eq!(d.detect_drag(&now2, &now), Drag::Out { tab: TabId(1), win: w.clone(), frame: Rect { x: R.x + 60.0, y: R.y + 10.0, ..R } });
    }

    #[test]
    fn tracking_pauses_while_the_user_holds_a_docked_window() {
        let ax = FakeAx::new(true);
        let t0 = Instant::now();
        let mut d = Docks::default();
        d.add(TabId(1), false, t0);
        let w = ax.add_window("W", R);
        d.dock_by_choice(TabId(1), w.clone());
        d.tick(&snap(&ax, vec![(TabId(1), Some(R))], t0));
        ax.set_mouse_down(true);
        ax.set_frame_outside(&w, Rect { x: R.x + 20.0, ..R });
        let s = snap(&ax, vec![(TabId(1), Some(R))], t0 + Duration::from_millis(50));
        assert!(d.tick(&s).iter().all(|a| !matches!(a, Action::Move { .. })), "no fighting the user's hand");
        ax.set_mouse_down(false);
        let s = snap(&ax, vec![(TabId(1), Some(R))], t0 + Duration::from_millis(100));
        assert!(d.tick(&s).contains(&Action::Move { win: w, rect: R }), "released under the threshold: it snaps back");
    }
```

(`Snapshot.arkos_frame: Option<Rect>` is added for the arkos window's global frame; `FakeAx::set_mouse(x, y)`.) In `drag.rs`: `zone_and_pane_for(panes: &[(PaneId, Rect)], pointer) -> Option<(PaneId, Zone)>` tested on two panes side by side.

- [ ] **Step 2: Run them to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos -- dock:: drag::`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`detect_drag(now, prev)`: with `now.mouse_down`: for a docked window whose frame moved between `prev` and `now` (the user holds its title bar, since tracking never moves it while the button is down): `Drag::Out` when `frame.distance_outside(&placeholder) > OUT_THRESHOLD`, else `None` (still held, under the threshold); for a window that is not docked and moved between the ticks, with the pointer inside `arkos_frame`: `Drag::In`. The `tick` skips `Move` for a docked window while `mouse_down` and its frame moved (the hand holds it), resuming on release.

`main.rs`: the dock timer, while the button is down, calls `detect_drag` each tick: `In` sets `browser_drop = zone_and_pane_for(pane rects in global coordinates, pointer)` and notifies (the pane overlay draws the tint for `browser_drop` exactly as for a tab drag: reuse the `Tint` holder of the transitions milestone keyed by pane); on the first tick with the button up after an `In`: if `browser_drop` is `Some((pane, zone))`, `finish_browser_drop`: a new `Tab::Browser { url: None, app_mode: false }` in that pane (or a split per zone using the same code path as dropping a tab into a zone), `docks.add` then `dock_by_choice(tab, win)`; else nothing. `Out { tab, win, frame }` on release: `undock_at(tab, frame)`: `docks.undock(tab)`, the window left at `frame`, the tab closed (`Closed::Browser` pushed). The tab-drag path: when a `Tab::Browser` is dragged out of the arkos window (the existing "drop outside" handling for tab drags, `drag.rs`'s drop target `None` at release with the pointer outside the window), `undock_at(tab, Rect at the pointer with the window's current size)`.

- [ ] **Step 4: Run the tests**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos`, the warning check, the em-dash grep.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/dock.rs crates/arkos/src/drag.rs crates/arkos/src/main.rs crates/arkos/src/pane_view.rs
git commit -m "Drag a Chrome window into a pane to dock it, and drag a docked window or its tab out to release it"
```

---

### Task 5: Docs, config comment, the probe in the checklist, spec amendments

**Files:**
- Modify: `README.md` (a "Browser pane" section, keyboard rows, the Status checklist, Config), `docs/superpowers/specs/2026-10-13-arkos-design.md` (rulings 1 to 7 as dated notes in section 8), `crates/arkos/src/config.rs` (the `[browser]` comment)

- [ ] **Step 1: Write**

README "Browser pane" section after "Sidebar and viewer": what a browser tab is (a placeholder a real Chrome window sits on), how one gets docked (automatically while a session works; "Dock a Chrome window" from the palette; a URL through "New browser tab", Cmd+Shift+B, with "single page" for app mode; dragging any Chrome window into a pane), how it is released (close the tab, "Undock", drag the tab out, drag the window's title bar out past 40 px), the Accessibility permission (one prompt; the pane explains; System Settings link), the slim tab bar, the Show Tabs Vertically tip, what is persisted (the URL, not the window). Keyboard rows: `Cmd+Shift+B` new browser tab. Config: `[browser] url`, `app_mode`. Checklist rows: "run `/Applications/Arkos IDE.app/Contents/MacOS/arkos browser-probe` once and grant access when asked: it moves a Chrome window for a few seconds and prints what it saw"; "open a browser tab while Claude is working: the next Chrome window Claude opens lands in it"; "drag a Chrome window into a pane; drag its title bar out again"; "switch tabs away and back: the window hides and returns"; "revoke Accessibility in System Settings: the pane shows the permission screen within a second". The spec: a dated note under section 8 listing rulings 1 to 7 of this plan.

- [ ] **Step 2: Check and commit**

Run: `grep -n "—" README.md docs/superpowers/specs/2026-10-13-arkos-design.md crates/arkos/src/config.rs` (empty), `nice -n 15 ~/.cargo/bin/cargo test -p arkos -- config` (the default-file test).

```bash
git add README.md docs/superpowers/specs/2026-10-13-arkos-design.md crates/arkos/src/config.rs
git commit -m "Document the browser pane: docking, undocking, the permission, the probe, the config"
```

---

## Self-review notes

- Spec 8 coverage: which window (auto-dock rule, palette dock, URL launch, app mode) Task 3; the unit is a window (one tab per window, titles from Chrome) Tasks 2 and 3; vertical space (slim bar, the tip, app mode for user windows only) Task 2 and ruling 7; drag in Task 4; drag out (tab out, title bar out past a threshold, Chrome's own tab drag not re-captured) Tasks 3 and 4; identification (AX element identity, polling while waiting) Tasks 1 and 3; tracking (follow the placeholder, hide when inactive or minimized, closing the tab leaves the window) Task 3; focus and stacking (click raises, activation re-raises) Task 3; permissions (prompt with an explanation, instructions without) Tasks 2 and 3. Spec 4: Cmd+1 to 9 already reach panes; persistence of the URL Task 2.
- Type consistency: `Rect` is `ax::Rect` everywhere; `AxWindow` equality by `CFEqual` (fake: by number); `Snapshot` fields `primary_width` and `arkos_frame` added in Tasks 3 and 4 respectively (Task 3's `snap` helper sets `primary_width`; Task 4 adds `arkos_frame` with `None` default in the helper); `Action` variants match between the engine and `apply_actions`; `TabId` is `tree::TabId`; `Closed::Browser` in Task 2 used by Task 4.
- Placeholder scan: none.
- Review Focus pins: 1 and 2 and 3 and 5 in Task 3's tests, 4 in Task 1 (`screen_rect_flips_y_against_the_primary_display`) and Task 3 (`a_moved_placeholder_moves_the_window...`).
- Rulings 1 to 7 are applied where the code lives; the probe's results, when the user runs it, may amend 1 and 3 in the ledger.
