# Terminal selection and links Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Three terminal UX fixes Cyrus asked for on 2026-10-16: a right-click on a link opens a small menu with "Open Link" and "Copy Link"; copying from Claude Code tells its own line wraps from real line breaks and joins the wraps; a selection keeps its anchor while the person scrolls, and dragging past the pane's edge scrolls on its own.

**Architecture:** All three live in the terminal pane: `crates/arkos/src/terminal.rs` (the `Terminal` entity and its `core`), `terminal_element.rs` (the drawn element, its per-frame loop, the hovered link), `mouse.rs` (the press and drag tracker `Clicks`, pure), `pane_view.rs` (the listeners), with the menu drawn like the sidebar's context menu (`sidebar.rs::render_context_menu`, `motion.rs::Transient`). The copy re-flow is a new pure module `reflow.rs`. No change to the daemon or the protocol.

**Tech Stack:** Rust, gpui, alacritty_terminal 0.26 (selections in grid coordinates, history lines negative).

**Design (agreed in chat):**
1. The terminal already finds the link under the pointer (`Terminal::hovered_link`, set each frame by `terminal_element::hovered_link`) for the underline and Cmd+click (`pane_view.rs` around line 831). A right-click on a link opens a menu with "Open Link" then "Copy Link"; a right-click elsewhere does nothing new.
2. Claude Code wraps its own prose and writes each visual line with a real newline, so alacritty's `WRAPLINE` join (which already handles lines the terminal wrapped) does not apply. The copy re-flows: consecutive lines join when the first line's next word would not have fit the wrap width, where the wrap width is the longest line of the selection and joining is attempted only when that width is at least half the terminal's columns. A line that ends early is a break; blank lines stay; a continuation indent is dropped on the join; a line that starts a list item never joins onto the previous one.
3. `mouse::Clicks` keeps the press anchor as a viewport cell and only turns it into a grid point when the drag starts, so a wheel scroll between the press and the first drag move shifts the anchor. The anchor becomes a grid point at the press. Dragging past the pane's top or bottom edge scrolls the history in that direction each frame and extends the selection to the edge row, driven by the element's frame loop (which already reads `window.mouse_position()` each frame), until the button comes up or the pointer comes back.

## Global Constraints

- Build hygiene (binding on this laptop): every cargo call `nice -n 15 ~/.cargo/bin/cargo ... ` with the worktree's `.cargo/config.toml` (jobs 3, shared target dir) copied from the main checkout and never committed; no `--release`; one cargo process at a time (check `pgrep -x cargo` and wait if one runs: another repository's implementer may be building). Focused tests while iterating: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- <module>::`; once before committing `nice -n 15 ~/.cargo/bin/cargo test --workspace --no-fail-fast` and `nice -n 15 ~/.cargo/bin/cargo check --workspace --all-targets` (no warning from our crates). Known timing flakes under load, rerun alone and report both results: `concurrent_spawns_give_each_shell_only_its_own_terminal`, `the_grace_counts_from_the_relaunch_so_a_slow_relaunch_keeps_the_old_bundle`, `recovery::tests::a_death_rewrites_the_manifest_without_the_dirty_mark`, `continuous_reader_survives_a_flood`.
- Never launch the Arkos app binary; headless tests only. Pure logic goes in functions that tests call without a `Window`.
- No em-dashes anywhere. Comments are full sentences. Doc comments say what and why, in the file's existing voice.
- Nothing about the daemon, the protocol or the viewer changes. The sidebar's context menu keeps its behaviour.
- Commit trailers (after a blank line): `Co-Authored-By: Claude <model> <noreply@anthropic.com>` with the implementer's model name, and `Claude-Session: https://claude.ai/code/session_015xrVD54zbvkgksJUndxfpr`.

## Review Focus

1. A right-click with no link under the pointer opens nothing and changes nothing (no selection cleared, nothing sent to the program). Pinned in Task 1.
2. Two short hard-broken lines never join; a wrapped paragraph from Claude Code joins into one line; a bullet's continuation lines join into the bullet and lose their indent; a blank line stays; a line beginning with a list marker never joins onto the previous one. Pinned in Task 2.
3. After a press, a wheel scroll of N lines, then a drag, the selection starts at the text that was under the press, not N lines away. Pinned in Task 3.
4. Auto-scroll stops the moment the button comes up, the pointer returns inside the element, or history runs out; it never scrolls a terminal in mouse-report mode or on the alternate screen (where the wheel is the program's). Pinned in Task 3.

---

### Task 1: A right-click on a link

**Files:**
- Modify: `crates/arkos/src/terminal.rs` (nothing but, if needed, a `pub fn link_at_position(&self, position) -> Option<HoveredLink>` that returns `hovered_link` when the position is on it), `crates/arkos/src/pane_view.rs` (the terminal element's listeners gain `.on_mouse_down(MouseButton::Right, ...)`), `crates/arkos/src/main.rs` (`Root.link_menu: Transient<LinkMenu>`, hidden with the other menus at the click-away and Escape sites around lines 664, 1025, 1054, 2928, 3601, drawn beside `render_context_menu` at 3647), `crates/arkos/src/sidebar.rs` or a new `crates/arkos/src/link_menu.rs` (the menu: `pub struct LinkMenu { url: String, at: Point<Pixels> }`, `pub fn link_menu_items(url: &str) -> Vec<(&'static str, MenuAction)>` returning `[("Open Link", MenuAction::OpenUrl(url)), ("Copy Link", MenuAction::CopyText(url))]` in that order, `pub fn render_link_menu(root, cx) -> Option<Div>` built the way `render_context_menu` builds its card and rows so the two look the same; `MenuAction` gains `OpenUrl(String)` and `run_menu_action` runs it with `cx.open_url`).

**Interfaces:**
- Produces: the menu; `link_menu_items`.
- Consumes: `Terminal::hovered_link` (set each frame by the element at the pointer; a right-click's position is the pointer's, so the hovered link is the link under it).

- [ ] **Step 1: Tests** (RED): `link_menu_items("https://x")` is exactly the two labelled actions in order; `run_menu_action(OpenUrl)` is not unit-testable (gpui), so the test covers the items and, in `pane_view` or `main`, a pure helper `right_click_target(hovered: Option<&HoveredLink>) -> Option<String>` that gives the url only when a link is hovered (Review Focus 1).
- [ ] **Step 2: Implement.** The right-click listener: if `right_click_target` gives a url, `root.link_menu.show(LinkMenu { url, at: ev.position }, now)` and `cx.stop_propagation()`; else nothing. The menu hides on a click anywhere else, on Escape, and after an item runs, through the same code paths as `context_menu` (search `context_menu` in main.rs and mirror each site). Focus stays with the terminal.
- [ ] **Step 3: Run** the focused tests, then the workspace gates; **commit** `A right-click on a link in the terminal opens a menu: Open Link, Copy Link`.

### Task 2: Copying from Claude Code joins its wraps and keeps its breaks

**Files:**
- Create: `crates/arkos/src/reflow.rs`
- Modify: `crates/arkos/src/main.rs` (`mod reflow;`), `crates/arkos/src/pane_view.rs` (the copy site around line 803: `reflow::reflow(&text, t.core.columns())` before the clipboard write; the same for any other copy of terminal text, such as a copy-on-select if one exists; search `selection_text`)

**Interfaces:**
```rust
/// Joins the lines a program wrapped for the terminal's width and keeps the ones it broke on
/// purpose. `columns` is the terminal's width; the wrap width is the longest line of `text`.
pub fn reflow(text: &str, columns: usize) -> String;
```
Rules, in order, for each pair of consecutive lines `a` and `b` (after `a` is trimmed on the right; `b_t` is `b` without its leading spaces; widths in chars):
1. If the longest line of `text` is shorter than `columns / 2`, nothing joins: short lines are breaks.
2. `a` or `b_t` empty: no join (blank lines stay).
3. `b_t` starts a list item or a structural line (`- `, `* `, `+ `, `• `, `● `, `○ `, `◐ `, `├`, `└`, `│`, `>`, a digit sequence followed by `. ` or `) `, `#`, a fenced marker, or `[ ]`/`[x]`): no join.
4. `len(a) + 1 + len(first word of b_t) > wrap width`: join as `a + " " + b_t`; the joined line is then `a` for the next pair.
5. Otherwise: a break.
Lines keep their own trailing content; the result ends with a newline only if the input did. The function is pure and never panics on any input (empty, one line, Unicode).

- [ ] **Step 1: Tests** (RED), with real Claude Code output pasted from the shoots as fixtures in the test module: a wrapped paragraph (joins into one line); a bullet whose continuation lines are indented two spaces (joins into one bullet line without the indent); two bullets in a row (never joined); two short hard-broken lines (never joined, Review Focus 2); paragraphs separated by a blank line (stay separate); a code block of short lines (unchanged); a selection of a single line (unchanged); a line with a trailing space and a Unicode word (counted in chars); `columns` smaller than every line (rule 1 off, rule 4 applies).
- [ ] **Step 2: Implement** `reflow.rs` and wire the copy site(s).
- [ ] **Step 3: Run** the focused tests, then the workspace gates; **commit** `Copying from the terminal joins a program's own wraps and keeps its breaks`.

### Task 3: The anchor stays put, and a drag past the edge scrolls

**Files:**
- Modify: `crates/arkos/src/mouse.rs` (`Clicks::press` and `moved` take the row as a grid line `i32` (the viewport row minus the display offset at that moment) instead of a viewport row; the anchor is `(col, line)`; `Gesture::Start`, `Select` and `Extend` carry lines; the "left the pressed cell" test compares lines; the tests updated and one added: a press at viewport row 5 with offset 0, then a move at viewport row 5 with offset 3 (line 2) is a drag from line 5 to line 2), `crates/arkos/src/terminal.rs` (`mouse_pressed` and `mouse_moved` pass `row as i32 - display_offset as i32`; `selection_start`/`selection_update` gain line-based twins or take `Line` directly; the auto-scroll state: `pub drag_scroll: Option<f32>` lines per second signed, set by `mouse_moved` when the left button is down with a selection and the pointer is above or below the element (`point_at` clamps the row, so compare the position's y with the element's origin and height instead), cleared by `mouse_released`, `mouse_cancelled`, and by a move back inside; `pub fn drag_scroll_step(&mut self, dt: f32, mouse_x: f32)`: scrolls `display_offset` by `rate * dt` (accumulated fractionally, clamped to the history), extends the selection to the top or bottom viewport row at the pointer's column, and clears itself when history runs out; never active in `MouseMode::Report` or on the alternate screen), `crates/arkos/src/terminal_element.rs` (the frame loop calls `drag_scroll_step` while `drag_scroll` is set, and keeps requesting frames while it is), `crates/arkos/src/pane_view.rs` (nothing, unless the right-click listener from Task 1 needs the same cancel)

**Interfaces:**
```rust
/// The auto-scroll rate for a pointer at `y` against an element from `top` to `bottom`, in
/// lines per second: zero inside, growing with the distance outside (6 lines per second per
/// cell height, at most 60). Positive scrolls up into history.
pub fn drag_scroll_rate(y: f32, top: f32, bottom: f32, cell_h: f32) -> f32;
```

- [ ] **Step 1: Tests** (RED): `mouse.rs` gestures with lines (the scrolled-between-press-and-drag case, Review Focus 3); `drag_scroll_rate` inside, just outside, far outside, capped, both signs; a `terminal.rs` test on `core` that a selection started at line 5 then `set_display_offset(3)` then updated at viewport row 0 covers lines 5 to -3 (grid coordinates do not move); `drag_scroll_step` with a fake history: it moves the offset by the rate, stops at the end of history and clears, and does nothing in report mode (Review Focus 4).
- [ ] **Step 2: Implement.**
- [ ] **Step 3: Run** the focused tests, then the workspace gates; **commit** `A selection keeps its anchor while scrolling, and a drag past the edge scrolls`.
