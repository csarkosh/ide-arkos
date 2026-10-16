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
- `terminal.rs` tests use `TerminalCore` only (`Terminal::new` needs a `Context` and a `Client`); anything that must be tested lives on the core or in a pure function. Task 3 changes the `Clicks` and `Gesture` signatures, so the existing `mouse.rs` and `terminal.rs` tests are updated with it.
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
- Modify: `crates/arkos/src/terminal.rs` (`TerminalCore::link_at(&self, col, row) -> Option<HoveredLink>`: the link under a viewport cell, found with `links::links_in_row` on that row's characters, as the element does for the hovered link; `Terminal::link_at_position(&self, position) -> Option<HoveredLink>` through `point_at`), `crates/arkos/src/pane_view.rs` (the terminal element's listeners gain `.on_mouse_down(MouseButton::Right, ...)`; a left press with Control held on macOS arrives as a left click, and it is treated the same as a right-click when a link is under it, since that is the Mac convention), `crates/arkos/src/main.rs` (`Root.link_menu: Transient<LinkMenu>`, hidden and settled wherever `context_menu` is: the sites around lines 664, 1025, 1054, 2928 and 3601, plus `sidebar.rs::escape_closes_menu` (around 1096 to 1105, both the any-open test and the hides) and the tree and worktree right-click handlers (sidebar.rs around 389 to 395 and 868 to 876), which hide the other menus before showing theirs and now hide `link_menu` too; drawn beside `render_context_menu` at 3647), a new `crates/arkos/src/link_menu.rs` (`pub struct LinkMenu { url: String, at: Point<Pixels> }`, `pub fn link_menu_items(url: &str) -> Vec<(&'static str, MenuAction)>` returning `[("Open Link", MenuAction::OpenUrl(url)), ("Copy Link", MenuAction::CopyText(url))]` in that order, `pub fn render_link_menu(root, cx) -> Option<Div>` built the way `render_context_menu` builds its card and rows so the two look the same; `sidebar::MenuAction` gains `OpenUrl(String)` and `run_menu_action` runs it with `cx.open_url`).

**Interfaces:**
- Produces: the menu; `link_menu_items`; `TerminalCore::link_at`.
- The right-click listener resolves the link at `ev.position` through `link_at_position` (not the last paint's `hovered_link`). When there is one: hide `context_menu`, `wt_menu`, `tab_menu` and the popovers explicitly (the listener's `cx.stop_propagation()` skips the root's `on_any_mouse_down`, which would otherwise do it), then `root.link_menu.show(LinkMenu { url, at: ev.position }, now)`. When there is none: nothing, and no `stop_propagation`. `stop_propagation` on the right button never reaches the left-button listeners (gpui filters by button), so the terminal's gestures are untouched; a right-click focuses the terminal through `track_focus`, which is fine.

- [ ] **Step 1: Tests** (RED): `link_menu_items("https://x")` is exactly the two labelled actions in order; on a `TerminalCore` holding a URL in a row, `link_at` on a cell of the URL gives it, a cell off it gives `None`, and the selection is unchanged either way (Review Focus 1).
- [ ] **Step 2: Implement.** The menu hides on a click anywhere else, on Escape, and after an item runs, through the same code paths as `context_menu` (search `context_menu` in main.rs and sidebar.rs and mirror each site). Focus stays with the terminal.
- [ ] **Step 3: Run** the focused tests, then the workspace gates; **commit** `A right-click on a link in the terminal opens a menu: Open Link, Copy Link`.

### Task 2: Copying from Claude Code joins its wraps and keeps its breaks

**Files:**
- Create: `crates/arkos/src/reflow.rs`
- Modify: `crates/arkos/src/main.rs` (`mod reflow;`), `crates/arkos/src/terminal.rs` (`TerminalCore::copy_text(&self) -> Option<String>`: the selection's text through `reflow` with the terminal's columns and the selection's start column, unchanged for a Block selection, a Lines selection keeping its trailing newline; `TerminalCore::selection_text` stays as the raw text), `crates/arkos/src/pane_view.rs` (the one copy site around line 803 calls `copy_text`; a second binding, Cmd+Shift+C "Copy as shown", copies `selection_text` raw as the escape hatch for code), `crates/arkos/src/keymap.rs` and `docs/keybindings` (whatever lists the bindings for the Keybindings window and the guide): the new binding.

**Interfaces:**
```rust
/// Joins the lines a program wrapped for the terminal's width and keeps the ones it broke on
/// purpose. `columns` is the terminal's width, `first_col` the column the text starts at on
/// its first line (a selection that starts mid-row), both in cells. Widths are display widths
/// (`unicode-width`), so CJK and emoji count as the cells they take.
pub fn reflow(text: &str, columns: usize, first_col: usize) -> String;
```
Rules, in order, for each pair of consecutive physical lines `a` and `b` of the input (the output accumulates; the comparison is always between the input's own lines, never a joined one; `a` is trimmed on the right; `b_t` is `b` without its leading spaces; the first line's width counts `first_col` cells before it):
1. The wrap width is `W = min(widest line, columns)`. If `W < columns - 12`, nothing joins at all: the text never reached the terminal's width, so its lines are breaks (a 60-cell hard-broken line in a 100-column terminal joins nothing).
2. `a` or `b_t` empty: no join (blank lines stay).
3. No join when `b_t` starts a list item or a structural line (`- `, `* `, `+ `, `• `, `● `, `○ `, `◐ `, `⏺ `, `>`, `#`, a fenced marker, `[ ]`, `[x]`, a digit sequence followed by `. ` or `) `) or when `a` or `b_t` starts with a box-drawing character (U+2500 to U+259F: tables, Claude Code's frames).
4. `width(a) + 1 + width(first word of b_t) > W`: join. The join is `a + " " + b_t`, except when `a` fills `W` exactly and neither side of the break is a space, where the program broke a long word (a URL, a path) mid-word and the join is `a + b_t` with no space.
5. Otherwise: a break.
The result ends with a newline only if the input did. The function is pure and never panics on any input (empty, one line, Unicode).

- [ ] **Step 1: Tests** (RED), with real Claude Code output pasted from the shoots as fixtures in the test module: a wrapped paragraph of three lines (one line); a two-line paragraph (one line); a bullet whose continuation lines are indented two spaces (one bullet line without the indent); two bullets in a row (never joined); a wrapped paragraph followed by a short hard-broken line such as `Done.` (the paragraph joins, `Done.` stays on its own: the comparison is on physical lines); two short hard-broken lines (never joined, Review Focus 2); paragraphs separated by a blank line (separate); a Claude Code frame of box characters after a full-width line (unchanged); a code block of short lines (unchanged); a URL broken mid-word at the width (joined without a space); a selection of a single line (unchanged); a CJK line (counted in cells); a selection starting mid-row (`first_col` counted); `columns` smaller than every line (rule 1 off through the `min`, rule 4 applies).
- [ ] **Step 2: Implement** `reflow.rs`, `copy_text`, the copy site, the raw-copy binding.
- [ ] **Step 3: Run** the focused tests, then the workspace gates; **commit** `Copying from the terminal joins a program's own wraps and keeps its breaks`.

### Task 3: The anchor stays put, and a drag past the edge scrolls

**Files:**
- Modify: `crates/arkos/src/mouse.rs` (`Clicks::press(col, row, line, side, click_count, report)`: `pending` keeps the viewport cell `(col, row)` for the bytes a held click sends to a program (so `release` is unchanged), `anchor` keeps the grid cell `(col, line)` where `line = row - display_offset` at the press; `moved(col, line, side, left_down, selecting)`; `Gesture::Start`, `Select` and `Extend` carry lines; `pub fn held(&self) -> bool` (the anchor is set); the "left the pressed cell" test compares lines; tests updated, two added: a press at viewport row 5 with offset 0 then a move at viewport row 5 with offset 3 (line 2) is a drag from line 5 to line 2 (Review Focus 3); a press at row 5 with offset 3 in Report mode releases with bytes saying row 5), `crates/arkos/src/terminal.rs` (`mouse_pressed` and `mouse_moved` pass both the row and `row as i32 - display_offset as i32`; `TerminalCore::selection_start`/`selection_update` take a grid `Line`; after a local wheel scroll while `clicks.held()` and a selection exists, the selection end is re-extended to `point_at(position)` since a wheel sends no move event; `TerminalCore::drag_scroll_step(&mut self, rate: f32, dt: f32, col: usize) -> bool`: scrolls through `self.scroll.scroll_direct(rate * dt, history)` so the element's per-frame `set_display_offset(split_offset(scroll.current))` keeps it (no separate accumulator; `frac` gives the smooth motion), then after the offset is applied extends the selection to the top or bottom viewport row at `col`, returns `false` once history runs out in that direction, and does nothing (returns `false`) in `MouseMode::Report` or on the alternate screen; `Terminal` wraps it), `crates/arkos/src/terminal_element.rs` (the paint loop decides the auto-scroll: while `clicks.held()` with a selection and `window.mouse_position().y` outside `bounds`, it computes `drag_scroll_rate`, calls the step with the pointer's column, keeps `last_frame` set so `dt` is real, and requests another frame; the inner div's hover exit already notifies, which paints the first frame; `on_mouse_up_out` is capture-phase and already cancels anywhere in the window; nothing is armed from `mouse_moved`, since gpui's `on_mouse_move` fires only while the element is hovered)

**Interfaces:**
```rust
/// The auto-scroll rate for a pointer at `y` against an element from `top` to `bottom`, in
/// lines per second: zero inside, else 6 lines per second per cell of distance with the
/// distance rounded up to whole cells (so just outside is already 6), at most 60. Positive
/// scrolls up into history.
pub fn drag_scroll_rate(y: f32, top: f32, bottom: f32, cell_h: f32) -> f32;
```

- [ ] **Step 1: Tests** (RED): the `mouse.rs` gestures above; `drag_scroll_rate` inside, just outside (6), far outside, capped, both signs; a `TerminalCore` test that a selection started at line 5 then `set_display_offset(3)` then updated at viewport row 0 covers lines 5 to -3 (grid coordinates do not move); `drag_scroll_step` on a core with history: it moves the offset by the rate, stops at the end of history and returns `false`, and returns `false` in report mode (Review Focus 4).
- [ ] **Step 2: Implement.**
- [ ] **Step 3: Run** the focused tests, then the workspace gates; **commit** `A selection keeps its anchor while scrolling, and a drag past the edge scrolls`.
