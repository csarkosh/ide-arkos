# Fullscreen drag-scroll Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In Claude Code's fullscreen renderer (the alternate screen with mouse reports), a drag held past the pane's top or bottom edge scrolls the conversation with wheel reports, and Cmd+C copies everything the drag passed over, chrome left out.

**Architecture:** A new pure module `crates/arkos/src/ledger.rs` holds the "record" of the spec: shift detection between two grids of row text, a baseline and a pending shift so split frames are never counted twice, lines that are provisional until they move, and the selection's two ends with the cell rules of alacritty's `Simple` selection. `TerminalCore` (`terminal.rs`) owns at most one `Ledger`, opens it at a press in `Report` mode on the alternate screen, feeds it every output batch from `apply_all`, answers `selection_active`, `selection_text` and `copy_text` from it while it is open, and rebuilds alacritty's selection as the tint after every change. The edge step (`drag_scroll_step`) gains a report branch that returns wheel bytes for `Terminal` to write. `mouse.rs`, `terminal_element.rs`, `pane_view.rs`, the daemon and the protocol are untouched.

**Tech Stack:** Rust 2021, alacritty_terminal 0.26 (`Selection`, `SelectionType::Simple`, `Side`, `Point`, `Line`, `Column`, `TermMode`), gpui (unchanged). No new dependencies.

**Spec:** `docs/superpowers/specs/2026-10-27-fullscreen-drag-scroll-design.md`.

## Global Constraints

- Machine rules (`CLAUDE.md`): every cargo call is `nice -n 15 cargo ...`; never `--release` on the laptop; never two cargo processes at once; a worktree needs its own copy of `.cargo/config.toml` (`cp .cargo/config.toml <worktree>/.cargo/config.toml`, the file is git-excluded) so it shares `target-dir` and `jobs = 3`; focused test filters while iterating, `cargo test --workspace` once per task; finish with `cargo check --workspace --all-targets` showing no warnings.
- Tests are headless, named as sentences, written before the code they check. Doc comments are plain prose, no em-dashes.
- Shift rule values (spec 4.2): a shift lines up at least **4** non-blank rows and at least **2** more than the runner-up; `k` is never 0 and at most the grid's height minus one.
- Repaint-beyond-recognition rule (spec 4.2, "Batches" step 5): no pending shift, no shift found, and more than **half** of the baseline's non-blank rows differ from the new grid closes the record.
- Edge rates are the shell's (`mouse::drag_scroll_rate`: 6 rows per second per cell of distance, at most 60); one whole row due is one wheel report.
- The record opens only when `mouse_mode()` is `MouseMode::Report { .. }`, the term is on the alternate screen, and the session is alive. `Arrows` and `Local` keep today's behaviour exactly (the tests of 2026-10-16 must keep passing unchanged).
- While a record is open it is the selection; alacritty's selection is only the tint and is never read for text or for `selection_active`.
- No timers: every step is driven by a batch of output, a gesture or a frame of the drag.

## Review Focus

1. A plain click on an option in Claude Code (press and release on one cell, no drag): the click still reaches the program at the release, no tint appears, and no record is left open (Task 4, `a_click_in_a_fullscreen_program_opens_and_closes_the_record_without_a_tint`).
2. CJK or emoji in a recorded row: the copy keeps each wide glyph whole and clipping by column never splits one (Task 3, `wide_glyphs_survive_the_copy_and_its_clipping`).
3. Claude Code exits to the shell while a record is open: the record closes without a panic, and a drag in the shell afterwards is today's grid selection with the history scrolling (Task 4, `leaving_the_alternate_screen_closes_the_record_and_the_shell_selects_as_before`).
4. Cmd+C right after a one-frame drag, before any later frame commits the pending shift: the text is right and has no prompt box or status rows in it (Task 2, `a_pending_shift_reads_like_a_committed_one_with_chrome_left_out`).
5. Mouse reporting turned off while a record is open (Claude Code hands the terminal to a subprocess): the edge step sends nothing, the record keeps following shifts, and the copy still works (Task 5, `reporting_turned_off_mid_record_stops_the_reports_but_keeps_the_record`).

## File structure

- `crates/arkos/src/ledger.rs` (new, pure, `mod ledger;` in `main.rs` after `mod layout;` alphabetically near `mod launch;`): `SPACER`, `MIN_MATCHED`, `MARGIN`, `row_text`, `Shift`, `find_shift`, `End`, `Outcome`, `Ledger` and its tests.
- `crates/arkos/src/terminal.rs` (modify): `TerminalCore` gains `ledger: Option<Ledger>` and `drag_carry: WheelCarry`; `rows_text`, `ledger_press`, `ledger_release`, `retint`; hooks in `apply_all`, `resize`, `clear_to_start`, `selection_start`, `selection_update`, `selection_clear`, `selection_active`, `selection_text`, `copy_text`; `DragStep` and the report branch of `drag_scroll_step`; `Terminal::mouse_pressed`, `mouse_released`, `mouse_cancelled`, `drag_scroll_step` wiring.
- `docs/guide.md`, `docs/development.md`, `docs/superpowers/specs/2026-10-13-arkos-design.md` (modify, Task 6).

## Rulings taken while planning

- Row text is one `char` per cell, a wide character's spacer cell as `ledger::SPACER` (`'\0'`), trailing spaces and spacers trimmed. Equality for shift detection compares these strings; the copy drops spacers on output. So a column index is a char index.
- The record's anchor side is derived when the range is read: `Left` when the anchor comes before the end in (line, column) order, `Right` otherwise, which mirrors `Clicks::moved`'s rule for a drag. A `Select` gesture (word, line) sets both ends explicitly with the sides alacritty's range implies (`Left` at the start, `Right` at the end, both inclusive).
- A band row written to a line that is already confirmed keeps it confirmed with the new text (the drag scrolled back over rows it had seen).
- The pending shift is read through: `offset()` includes it, a row in its block or band reads from the grid it was found on, a confirmed line elsewhere reads from the committed record, and a provisional line outside its block and band reads as absent, exactly as a commit would leave it.
- The tint is a `Simple` alacritty selection from the projected start cell (`Side::Left`) to the projected end cell (`Side::Right`), both inclusive, clamped to `moved_rows()`.
- `apply_output` stays the per-chunk path with no record work; the record hook is in `apply_all`, which is the only production entry (`Terminal::flush_output`). Tests that want the hook call `t.apply_all([bytes])`.

---

### Task 1: Shift detection

**Files:**
- Create: `crates/arkos/src/ledger.rs`
- Modify: `crates/arkos/src/main.rs` (add `mod ledger;`)

**Interfaces:**
- Produces:
  - `pub const SPACER: char = '\0';`
  - `pub const MIN_MATCHED: usize = 4;` and `pub const MARGIN: usize = 2;`
  - `pub fn row_text(cells: &[(char, crate::layout::CellStyle, crate::layout::CellKind)]) -> String`: one char per cell, `CellKind::Spacer` as `SPACER`, trailing `' '` and `SPACER` trimmed.
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct Shift { pub k: i32, pub block: std::ops::Range<usize>, pub band: std::ops::Range<usize> }`: `k > 0` means old row `i` is now at `i + k`; `block` and `band` are new-grid row ranges.
  - `pub fn find_shift(old: &[String], new: &[String]) -> Option<Shift>`.

- [ ] **Step 1: Write the failing tests** in `ledger.rs`'s `mod tests`, with a helper `fn grid(rows: &[&str]) -> Vec<String>`:

```rust
#[test]
fn a_scroll_is_the_shift_that_lines_up_the_most_rows_and_names_its_block_and_band() {
    // Ten content rows over a two-row prompt box; the program scrolled up into older text by 3.
    let old = grid(&["l20","l21","l22","l23","l24","l25","l26","l27","l28","l29","─────","❯"]);
    let new = grid(&["l17","l18","l19","l20","l21","l22","l23","l24","l25","l26","─────","❯"]);
    assert_eq!(find_shift(&old, &new), Some(Shift { k: 3, block: 3..10, band: 0..3 }));
    // Back down by 3: the band is below the block.
    assert_eq!(find_shift(&new, &old), Some(Shift { k: -3, block: 0..7, band: 7..10 }));
}

#[test]
fn a_blank_row_never_counts_as_a_match_but_moves_inside_the_block() {
    let old = grid(&["a","","b","c","","d","e","f","g","h"]);
    let new = grid(&["x","y","a","","b","c","","d","e","f"]);
    assert_eq!(find_shift(&old, &new), Some(Shift { k: 2, block: 2..10, band: 0..2 }));
}

#[test]
fn fewer_than_four_rows_or_a_thin_margin_is_no_shift() {
    let old = grid(&["a","b","c","d","e","f"]);
    assert_eq!(find_shift(&old, &grid(&["z","a","b","c","q","r"])), None, "three rows line up");
    assert_eq!(find_shift(&old, &old), None, "nothing moved");
    // Four identical rows line up equally well at k = 1 and k = 2: no margin, no shift.
    let same = grid(&["r","r","r","r","r","r"]);
    assert_eq!(find_shift(&same, &same), None);
}

#[test]
fn chrome_that_stays_put_and_a_sticky_header_are_outside_the_block() {
    // The header appears at row 0 as the view scrolls; the status row at the bottom never moves.
    let old = grid(&["l1","l2","l3","l4","l5","l6","l7","status"]);
    let new = grid(&["❯ header","l0","l1","l2","l3","l4","l5","status"]);
    assert_eq!(find_shift(&old, &new), Some(Shift { k: 2, block: 2..7, band: 0..2 }));
}

#[test]
fn the_band_is_clipped_to_the_grid() {
    let old = grid(&["a","b","c","d","e"]);
    let new = grid(&["b","c","d","e","f"]);
    assert_eq!(find_shift(&old, &new), Some(Shift { k: -1, block: 0..4, band: 4..5 }));
}

#[test]
fn row_text_is_one_char_per_cell_with_spacers_marked_and_the_tail_trimmed() {
    use crate::layout::{CellKind, CellStyle};
    let s = CellStyle::default();
    let cells = vec![('字', s, CellKind::Wide), (' ', s, CellKind::Spacer), ('a', s, CellKind::Normal), (' ', s, CellKind::Normal)];
    assert_eq!(row_text(&cells), "字\0a");
}
```

(If `CellStyle` has no `Default`, build one the way `terminal.rs`'s tests do.)

- [ ] **Step 2: Run the tests to verify they fail**

Run: `nice -n 15 cargo test -p arkos ledger::`
Expected: compile error, `ledger` not found.

- [ ] **Step 3: Implement `ledger.rs`** with the constants, `row_text`, `Shift` and `find_shift`. `find_shift`: for every `k` in `-(n-1)..=(n-1)` except 0, count rows `i` with `0 <= i + k < n`, `old[i]` non-empty and `old[i] == new[i + k]`; the best `k` wins when its count is at least `MIN_MATCHED` and at least `MARGIN` more than the next best count; `block` is `first matched j ..= last matched j` as a `Range`, `band` is `block.start - k .. block.start` for `k > 0` and `block.end .. block.end + |k|` for `k < 0`, both clipped to `0..n`. Module doc: what a record is for, in prose. Add `mod ledger;` to `main.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `nice -n 15 cargo test -p arkos ledger::`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/ledger.rs crates/arkos/src/main.rs
git commit -m "A program's scroll is found from two grids: the shift that lines up the most rows, its block and its band"
```

### Task 2: The record follows batches

**Files:**
- Modify: `crates/arkos/src/ledger.rs`

**Interfaces:**
- Consumes: `Shift`, `find_shift` (Task 1).
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct End { pub line: i32, pub col: usize, pub side: alacritty_terminal::index::Side }`
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Outcome { Open, Closed }`
  - `pub struct Ledger { .. }` with:
    - `pub fn open(rows: Vec<String>, anchor_line: i32, anchor_col: usize) -> Ledger`: offset 0, every row a provisional line, baseline the rows, no pending shift, no end, `moved_rows` the whole grid.
    - `pub fn output(&mut self, new: &[String]) -> Outcome`: the batch rules of spec 4.2, "Batches" (steps 1 to 5). The grid the pending shift was found on is kept inside the record, so the caller passes only `new`.
    - `pub fn offset(&self) -> i32`: committed offset plus the pending shift's `k`.
    - `pub fn moved_rows(&self) -> std::ops::Range<usize>`: block and band of the pending shift, else of the last committed one, else `0..rows`.
    - `pub fn line_of_row(&self, row: usize) -> i32` and `pub fn row_of_line(&self, line: i32) -> i32`.
    - `pub fn line_text(&self, line: i32) -> Option<&str>`: the read-through rule in "Rulings".
    - `pub fn rows(&self) -> usize`: the grid height at opening.

- [ ] **Step 1: Write the failing tests**, with a helper `fn scrolled(content: &[&str], top: usize, height: usize, chrome: &[&str]) -> Vec<String>` that builds a grid showing `content[top..top + height - chrome.len()]` over `chrome`, and `fn lines(l: &Ledger, from: i32, to: i32) -> Vec<Option<String>>`:

```rust
#[test]
fn a_committed_shift_renumbers_nothing_and_adds_the_band_as_provisional_lines() {
    let content: Vec<String> = (0..40).map(|i| format!("l{i}")).collect();
    let c: Vec<&str> = content.iter().map(String::as_str).collect();
    let mut l = Ledger::open(scrolled(&c, 30, 12, &["─","❯"]), 9, 0);
    // Frame 1: scrolled up by 3, pending. Frame 2: by 3 more; frame 1 commits.
    assert_eq!(l.output(&scrolled(&c, 27, 12, &["─","❯"])), Outcome::Open);
    assert_eq!(l.offset(), 3);
    assert_eq!(l.output(&scrolled(&c, 24, 12, &["─","❯"])), Outcome::Open);
    assert_eq!(l.offset(), 6);
    // Line 0 is still l30, the first row at opening; lines -6..-1 are the rows that came in.
    assert_eq!(l.line_text(0), Some("l30"));
    assert_eq!(l.line_text(-6), Some("l24"));
    assert_eq!(l.line_text(-1), Some("l29"));
    assert_eq!(l.row_of_line(-6), 0);
    assert_eq!(l.line_of_row(9), 3);
}

#[test]
fn chrome_from_the_opening_snapshot_is_dropped_at_the_first_commit() {
    let c: Vec<String> = (0..40).map(|i| format!("l{i}")).collect();
    let c: Vec<&str> = c.iter().map(String::as_str).collect();
    let mut l = Ledger::open(scrolled(&c, 30, 12, &["─","❯"]), 9, 0);
    assert_eq!(l.line_text(10), Some("─"), "provisional until something moves");
    l.output(&scrolled(&c, 27, 12, &["─","❯"]));
    l.output(&scrolled(&c, 24, 12, &["─","❯"]));
    assert_eq!(l.line_text(10), None);
    assert_eq!(l.line_text(11), None);
}

#[test]
fn a_sticky_header_that_arrives_in_a_band_is_dropped_when_it_stays_put() {
    // Ten content rows over a status row; the program scrolls up by two per frame and shows a
    // header row once the message's first row has scrolled off.
    let mut l = Ledger::open(grid(&["l1","l2","l3","l4","l5","l6","l7","l8","l9","l10","status"]), 9, 0);
    l.output(&grid(&["❯ header","l0","l1","l2","l3","l4","l5","l6","l7","l8","status"]));
    assert_eq!(l.line_text(-2), Some("❯ header"), "pending: the band is read from the grid");
    l.output(&grid(&["❯ header","l-2","l-1","l0","l1","l2","l3","l4","l5","l6","status"]));
    l.output(&grid(&["❯ header","l-4","l-3","l-2","l-1","l0","l1","l2","l3","l4","status"]));
    // The header stayed at row 0 while the text moved under it: gone from the record, and the
    // rows that came in are contiguous.
    let got: Vec<Option<String>> = lines(&l, -6, 1);
    let want: Vec<Option<String>> = vec![None, Some("l-4".into()), Some("l-3".into()), Some("l-2".into()), Some("l-1".into()), Some("l0".into()), Some("l1".into()), Some("l2".into())];
    assert_eq!(got, want);
}

#[test]
fn the_pressed_row_pushed_under_the_prompt_box_by_the_first_scroll_is_kept() {
    let mut l = Ledger::open(grid(&["l1","l2","l3","l4","l5","l6","l7","─","❯"]), 6, 0);
    l.output(&grid(&["l-2","l-1","l0","l1","l2","l3","l4","─","❯"]));
    l.output(&grid(&["l-5","l-4","l-3","l-2","l-1","l0","l1","─","❯"]));
    assert_eq!(l.line_text(6), Some("l7"), "it left the screen without moving with a shift");
    assert_eq!(l.line_text(7), None, "the prompt box stayed put");
}

#[test]
fn the_jump_to_bottom_overlay_never_reaches_the_record_on_a_drag_downward() {
    // The overlay sits on the last content row; a drag downward brings rows in under it.
    let mut l = Ledger::open(grid(&["l1","l2","l3","l4","l5","l6   Jump to bottom","status"]), 0, 0);
    l.output(&grid(&["l2","l3","l4","l5","l6","l7   Jump to bottom","status"]));
    assert_eq!(l.line_text(5), Some("l6"), "the band's row is the clean one above the overlay");
    l.output(&grid(&["l3","l4","l5","l6","l7","l8   Jump to bottom","status"]));
    assert_eq!(l.line_text(5), Some("l6"));
    assert_eq!(l.line_text(6), Some("l7"));
    assert_eq!(l.line_text(7), None, "the overlay row itself is not recorded yet");
}

#[test]
fn a_frame_split_across_two_batches_gives_the_same_record_as_the_frame_in_one() {
    let before = grid(&["l10","l11","l12","l13","l14","l15","l16","l17","l18","l19","status"]);
    let whole = grid(&["l7","l8","l9","l10","l11","l12","l13","l14","l15","l16","status"]);
    // The first read painted the top seven rows; the rest still show the old text.
    let half = grid(&["l7","l8","l9","l10","l11","l12","l13","l17","l18","l19","status"]);
    let next = grid(&["l4","l5","l6","l7","l8","l9","l10","l11","l12","l13","status"]);
    let mut split = Ledger::open(before.clone(), 9, 0);
    split.output(&half);
    split.output(&whole);
    split.output(&next);
    let mut one = Ledger::open(before, 9, 0);
    one.output(&whole);
    one.output(&next);
    assert_eq!(lines(&split, -6, 10), lines(&one, -6, 10));
    assert_eq!(split.offset(), one.offset());
}

#[test]
fn a_pending_shift_reads_like_a_committed_one_with_chrome_left_out() {
    let c: Vec<String> = (0..50).map(|i| format!("l{i}")).collect();
    let c: Vec<&str> = c.iter().map(String::as_str).collect();
    // Ten content rows (l30..l39) over a two-row prompt box.
    let mut l = Ledger::open(scrolled(&c, 30, 12, &["─","❯"]), 0, 0);
    // One frame down (newer text came in at the bottom) and nothing after it: still pending.
    l.output(&scrolled(&c, 33, 12, &["─","❯"]));
    assert_eq!(l.offset(), -3);
    assert_eq!(l.moved_rows(), 0..10);
    assert_eq!(l.line_text(9), Some("l39"));
    // The band's rows read from the grid the shift was found on.
    assert_eq!(lines(&l, 10, 12), vec![Some("l40".into()), Some("l41".into()), Some("l42".into())]);
    // The prompt box's rows are lines 13 and 14 now and were never recorded: absent. (The
    // snapshot's own prompt box lines, 10 and 11, are read through the band above.)
    assert_eq!(lines(&l, 13, 14), vec![None, None]);
}
```

```rust
#[test]
fn two_frames_in_consecutive_batches_commit_the_first() {
    let a = grid(&["l10","l11","l12","l13","l14","l15","status"]);
    let b = grid(&["l9","l10","l11","l12","l13","l14","status"]);
    let c = grid(&["l8","l9","l10","l11","l12","l13","status"]);
    let mut l = Ledger::open(a, 0, 0);
    l.output(&b);
    l.output(&c);
    assert_eq!(l.offset(), 2);
    assert_eq!(l.line_text(-1), Some("l9"));
    assert_eq!(l.line_text(-2), Some("l8"));
}

#[test]
fn a_repaint_in_place_leaves_the_record_alone_and_a_redraw_beyond_recognition_closes_it() {
    let a = grid(&["l1","l2","l3","l4","l5","l6","⠋ thinking","status"]);
    let mut l = Ledger::open(a, 0, 0);
    assert_eq!(l.output(&grid(&["l1","l2","l3","l4","l5","l6","⠙ thinking","status"])), Outcome::Open);
    assert_eq!(l.offset(), 0);
    assert_eq!(l.output(&grid(&["x1","x2","x3","x4","x5","l6","⠹ thinking","status"])), Outcome::Closed);
}

#[test]
fn scrolling_back_and_forth_keeps_the_rows_seen() {
    let a = grid(&["l10","l11","l12","l13","l14","l15","status"]);
    let b = grid(&["l8","l9","l10","l11","l12","l13","status"]);
    let mut l = Ledger::open(a.clone(), 0, 0);
    l.output(&b);
    l.output(&a);
    l.output(&b);
    l.output(&a);
    assert_eq!(l.offset(), 0);
    assert_eq!(l.line_text(0), Some("l10"));
    assert_eq!(l.line_text(-2), Some("l8"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `nice -n 15 cargo test -p arkos ledger::`
Expected: compile error, `Ledger` not found.

- [ ] **Step 3: Implement `End`, `Outcome` and `Ledger`** in `ledger.rs`. State: `offset: i32`, `lines: BTreeMap<i32, Entry { text: String, confirmed: bool }>`, `rows: usize`, `baseline: Vec<String>`, `pending: Option<Pending { shift: Shift, grid: Vec<String> }>`, `last_moved: Option<Range<usize>>`, `anchor: End` (side `Left` until the range is read, see Task 3), `end: Option<End>`. `output(new)`:

```text
d = find_shift(&baseline, new)
match (pending, d):
  (None, Some(d))                     -> pending = (d, new)
  (Some(p), Some(d)) if d.k == p.k    -> pending = (d, new)
  (Some(p), _)                        -> commit(p.shift, &p.grid); baseline = p.grid;
                                         match find_shift(&baseline, new) { Some(d2) -> pending = (d2, new), None -> return close_check(new) }
  (None, None)                        -> return close_check(new)
return Open
```

`commit(shift, grid)`, with `old` the baseline (the grid before the shift) and `grid` the grid after it: `offset += k`; each row in `block` writes `lines[row - offset]` confirmed from `grid[row]`; each row in `band` writes `lines[row - offset]` provisional unless already confirmed (then confirmed, new text); then every provisional line that stayed put is removed: with `r = line + (offset - k)` (its row before the shift), `0 <= r < rows` and `old[r] == text && grid[r] == text`; `last_moved = Some(block ∪ band)`. A provisional line whose row shows something else now is kept (spec 4.2, "Shifts", step 3). `close_check(new)`: `Closed` when more than half of the baseline's non-empty rows differ from `new` at the same index, else `Open`. `line_text` follows the read-through ruling.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `nice -n 15 cargo test -p arkos ledger::`
Expected: all ledger tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/ledger.rs
git commit -m "The record follows a program's scroll batch by batch: a baseline, a pending shift, provisional lines dropped when they stay put"
```

### Task 3: The record's selection: ends, text, projection

**Files:**
- Modify: `crates/arkos/src/ledger.rs`

**Interfaces:**
- Consumes: `Ledger`, `End` (Task 2).
- Produces, on `Ledger`:
  - `pub fn set_end(&mut self, end: End)`
  - `pub fn set_range(&mut self, start: End, end: End)`: both ends explicit (a word or a line).
  - `pub fn has_end(&self) -> bool`
  - `pub fn range(&self) -> Option<(End, End)>`: ordered by (line, col), then the `Simple` cell rules: a `Right` start moves one cell right (to column 0 of the next line past the last column, `columns` given as a parameter: make the signature `range(&self, columns: usize)`), a `Left` end moves one cell left (to the last column of the previous line from column 0); `None` when, after that, the start is past the end, or when there is no end. The anchor's side is `Left` when the anchor is not after the end in (line, col) order, else `Right`, unless `set_range` set it.
  - `pub fn text(&self, columns: usize) -> Option<String>`: lines from the range's start line to its end line, `line_text` of each, missing lines skipped, the first clipped from the start column and the last to the end column inclusive (char indices, `SPACER` chars dropped on output), joined by `'\n'`, no trailing newline; `None` when nothing is selected or every line is missing.
  - `pub fn first_col(&self, columns: usize) -> usize`: the range start's column (for `reflow_at`).
  - `pub fn projection(&self, columns: usize) -> Option<((usize, usize), (usize, usize))>`: the range's cells as (row, col) pairs on the grid, start clamped up to `(moved_rows().start, 0)` and end down to `(moved_rows().end - 1, columns - 1)`; `None` when the whole range is above or below `moved_rows()`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn the_text_runs_from_the_anchor_to_the_end_with_the_simple_cell_rules() {
    let mut l = Ledger::open(grid(&["alpha beta","gamma delta","epsilon","status"]), 2, 3);
    // Dragged up and left: the end is the start of the range and its left side keeps its cell;
    // the anchor is after it, so its side is Right and its cell is included.
    l.set_end(End { line: 0, col: 6, side: Side::Left });
    assert_eq!(l.text(20).as_deref(), Some("beta\ngamma delta\nepsi"));
    // Dragged down: the anchor's left side includes its cell, the end's right side includes its cell.
    let mut l = Ledger::open(grid(&["alpha beta","gamma delta","epsilon","status"]), 0, 6);
    l.set_end(End { line: 1, col: 4, side: Side::Right });
    assert_eq!(l.text(20).as_deref(), Some("beta\ngamma"));
    assert_eq!(l.first_col(20), 6);
    // Nothing yet, and a selection of no cells.
    let mut l = Ledger::open(grid(&["alpha beta"]), 0, 2);
    assert_eq!(l.text(20), None);
    l.set_end(End { line: 0, col: 2, side: Side::Left });
    assert_eq!(l.text(20), None);
}

#[test]
fn missing_lines_are_skipped_and_rows_past_the_text_read_as_blank() {
    let mut l = Ledger::open(grid(&["l1","l2","l3","l4","l5","l6","─","❯"]), 0, 0);
    l.output(&grid(&["l0","l1","l2","l3","l4","l5","─","❯"]));
    l.output(&grid(&["l-1","l0","l1","l2","l3","l4","─","❯"]));
    // The end is on the prompt box's row; its lines were dropped at the commit, so the copy
    // ends with the last line of text, whole (column 10 is past its end).
    l.set_end(End { line: 7, col: 10, side: Side::Right });
    assert_eq!(l.text(20).as_deref(), Some("l1\nl2\nl3\nl4\nl5\nl6"), "the prompt box lines were dropped");
}

#[test]
fn a_word_or_a_line_is_recorded_as_the_cells_it_covers() {
    let mut l = Ledger::open(grid(&["alpha beta","gamma","delta","eps","zeta","eta"]), 0, 7);
    l.set_range(End { line: 0, col: 6, side: Side::Left }, End { line: 0, col: 9, side: Side::Right });
    assert_eq!(l.text(20).as_deref(), Some("beta"));
    l.output(&grid(&["pre","alpha beta","gamma","delta","eps","zeta"]));
    assert_eq!(l.text(20).as_deref(), Some("beta"), "the word moved with the text");
    assert_eq!(l.projection(20), Some(((1, 6), (1, 9))));
}

#[test]
fn wide_glyphs_survive_the_copy_and_its_clipping() {
    let mut l = Ledger::open(vec!["字\0a字\0".to_string(), "b".to_string()], 0, 0);
    l.set_end(End { line: 0, col: 3, side: Side::Right });
    assert_eq!(l.text(20).as_deref(), Some("字a字"));
    l.set_end(End { line: 0, col: 1, side: Side::Right });
    assert_eq!(l.text(20).as_deref(), Some("字"), "the spacer cell adds nothing");
}

#[test]
fn the_tint_is_the_range_on_the_grid_clamped_to_the_rows_that_moved() {
    let c: Vec<String> = (0..40).map(|i| format!("l{i}")).collect();
    let c: Vec<&str> = c.iter().map(String::as_str).collect();
    let mut l = Ledger::open(scrolled(&c, 30, 12, &["─","❯"]), 9, 2);
    l.set_end(End { line: 0, col: 0, side: Side::Left });
    // The anchor is after the end, so its side is Right and its cell is in.
    assert_eq!(l.projection(20), Some(((0, 0), (9, 2))));
    l.output(&scrolled(&c, 27, 12, &["─","❯"]));
    l.output(&scrolled(&c, 24, 12, &["─","❯"]));
    l.set_end(End { line: -6, col: 0, side: Side::Left });
    // The anchor (line 9) is at row 15, under the prompt box: the tint stops at the last row that moved.
    assert_eq!(l.projection(20), Some(((0, 0), (9, 19))));
    l.set_range(End { line: -20, col: 0, side: Side::Left }, End { line: -15, col: 3, side: Side::Right });
    assert_eq!(l.projection(20), None, "entirely above the screen");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `nice -n 15 cargo test -p arkos ledger::`
Expected: compile errors for `set_end`, `text`, `projection`.

- [ ] **Step 3: Implement** `set_end`, `set_range`, `has_end`, `range`, `text`, `first_col`, `projection` on `Ledger`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `nice -n 15 cargo test -p arkos ledger::`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/ledger.rs
git commit -m "The record keeps the selection's ends and gives its text and its place on the grid"
```

### Task 4: The core opens, feeds and reads the record

**Files:**
- Modify: `crates/arkos/src/terminal.rs` (`TerminalCore` fields near line 92; `new` at 167; `apply_all` at 250; `resize` at 279; `clear_to_start` at 286; `selection_start` 429, `selection_update` 434, `selection_clear` 475, `selection_active` 480, `selection_text` 485, `copy_text` 493; `Terminal::mouse_pressed` 806, `mouse_released` 829, `mouse_cancelled` 840; tests from 976)

**Interfaces:**
- Consumes: `ledger::{Ledger, End, Outcome, row_text}` (Tasks 1 to 3).
- Produces, on `TerminalCore`:
  - `pub fn ledger_press(&mut self, col: usize, row: usize, alive: bool)`: drops any record; opens one when `alive`, `mouse_mode()` is `Report { .. }` and the term is on `TermMode::ALT_SCREEN`, with `rows_text()` and the anchor at (`row` as line, `col`).
  - `pub fn ledger_release(&mut self)`: drops the record when it has no end.
  - `pub fn record_open(&self) -> bool` (for tests and the edge step).
  - `fn rows_text(&self) -> Vec<String>`: `row_text` of each visible row at the current display offset.
  - `fn retint(&mut self)`: `term.selection` from `ledger.projection(columns)`: `Selection::new(SelectionType::Simple, Point::new(Line(start_row), Column(start_col)), Side::Left)` then `update(Point::new(Line(end_row), Column(end_col)), Side::Right)`; `None` when there is no projection.
  - Changed behaviour: `apply_all` takes `rows_text()` before the chunks when a record is open and after them calls `ledger.output(&new)`; `Closed` drops the record; the record is also dropped when the term is no longer on the alternate screen; then `retint()`. `resize` and `clear_to_start` drop the record. `selection_start` with a `Semantic` or `Lines` type sets the record's range from `term.selection.to_range` (start `Left`, end `Right`, lines are grid lines, `line_of_row` converts). `selection_update` sets the record's end (`line_of_row(line.0 + display_offset)`), then `retint`. `selection_clear` drops the record. `selection_active` is `has_end()` while a record is open. `selection_text` is `ledger.text(columns)` while a record is open. `copy_text` with a record open: `reflow_at(text, columns, first_col, cut_width)` with the input-box inset when the start line's row is on screen and `in_input_box(Line(row))`.
  - `Terminal::mouse_pressed` calls `core.ledger_press(col, row, self.alive)` before `clicks.press`; `mouse_released` and `mouse_cancelled` call `core.ledger_release()` after their current work.
- Test helpers (in `terminal.rs` tests): `fn fullscreen(cols: usize, rows: usize) -> TerminalCore` (applies `\x1b[?1049h\x1b[?1000h\x1b[?1006h`); `fn paint(t: &mut TerminalCore, rows: &[&str])` (one batch through `apply_all`, each row as `\x1b[{i+1};1H{text}\x1b[K`); `fn drag(t: &mut TerminalCore, from: (usize, usize), to: (usize, usize))` (`ledger_press`, then `selection_start(Simple)` from the anchor with the side `Clicks` would pick, then `selection_update`).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_record_opens_only_for_a_live_program_that_reports_the_mouse_on_the_alternate_screen() {
    let mut t = TerminalCore::new(20, 5, 100);
    t.ledger_press(0, 0, true);
    assert!(!t.record_open(), "the primary screen");
    t.apply_output(b"\x1b[?1049h");
    t.ledger_press(0, 0, true);
    assert!(!t.record_open(), "arrows mode");
    t.apply_output(b"\x1b[?1000h");
    t.ledger_press(0, 0, false);
    assert!(!t.record_open(), "an ended session");
    t.ledger_press(0, 0, true);
    assert!(t.record_open());
    assert!(!t.selection_active(), "an anchor alone selects nothing");
}

#[test]
fn a_click_in_a_fullscreen_program_opens_and_closes_the_record_without_a_tint() {
    let mut t = fullscreen(20, 5);
    paint(&mut t, &["one", "two", "three", "─", "❯"]);
    // A press, then the release: no end was ever set, so the record goes.
    t.ledger_press(1, 1, true);
    assert!(t.record_open());
    t.ledger_release();
    assert!(!t.record_open());
    assert!(t.selection_spans().is_empty());
    // The press of a single click clears the selection first, which also closes the record.
    t.ledger_press(1, 1, true);
    t.selection_clear();
    assert!(!t.record_open());
}

#[test]
fn a_selection_in_a_fullscreen_program_keeps_its_text_while_the_program_scrolls() {
    let mut t = fullscreen(20, 8);
    paint(&mut t, &["l10", "l11", "l12", "l13", "l14", "l15", "─", "❯"]);
    drag(&mut t, (0, 1), (2, 2));
    assert_eq!(t.selection_text().as_deref(), Some("l11\nl12"));
    paint(&mut t, &["l9", "l10", "l11", "l12", "l13", "l14", "─", "❯"]);
    paint(&mut t, &["l8", "l9", "l10", "l11", "l12", "l13", "─", "❯"]);
    assert_eq!(t.selection_text().as_deref(), Some("l11\nl12"));
    let spans = t.selection_spans();
    assert_eq!(spans.first().map(|s| s.row), Some(3), "the tint moved with the text");
    assert_eq!(spans.len(), 2);
}

#[test]
fn output_that_erases_a_tinted_row_does_not_lose_the_tint() {
    let mut t = fullscreen(20, 6);
    paint(&mut t, &["l10", "l11", "l12", "l13", "⠋ thinking", "❯"]);
    drag(&mut t, (0, 1), (2, 2));
    paint(&mut t, &["l10", "l11", "l12", "l13", "⠙ thinking", "❯"]);
    assert_eq!(t.selection_spans().len(), 2);
    assert!(t.selection_active());
}

#[test]
fn a_double_click_in_a_fullscreen_program_records_the_word() {
    let mut t = fullscreen(20, 10);
    paint(&mut t, &["alpha beta", "gamma", "delta", "eps", "zeta", "eta", "theta", "iota", "─", "❯"]);
    t.ledger_press(7, 0, true);
    t.selection_start(7, Line(0), Side::Left, SelectionType::Semantic);
    assert_eq!(t.selection_text().as_deref(), Some("beta"));
    paint(&mut t, &["pre", "alpha beta", "gamma", "delta", "eps", "zeta", "eta", "theta", "─", "❯"]);
    assert_eq!(t.selection_text().as_deref(), Some("beta"));
    assert_eq!(t.selection_spans().first().map(|s| (s.row, s.start, s.end)), Some((1, 6, 9)));
}

#[test]
fn leaving_the_alternate_screen_closes_the_record_and_the_shell_selects_as_before() {
    let mut t = fullscreen(20, 5);
    paint(&mut t, &["one", "two", "three", "─", "❯"]);
    drag(&mut t, (0, 0), (2, 1));
    t.apply_all([b"\x1b[?1000l\x1b[?1049l".as_slice()]);
    assert!(!t.record_open());
    t.apply_all([b"hello\r\nworld".as_slice()]);
    t.selection_start(0, Line(0), Side::Left, SelectionType::Simple);
    t.selection_update(4, Line(0), Side::Right);
    assert_eq!(t.selection_text().as_deref(), Some("hello"));
}

#[test]
fn a_resize_and_a_redraw_beyond_recognition_close_the_record() {
    let mut t = fullscreen(20, 6);
    paint(&mut t, &["l1", "l2", "l3", "l4", "l5", "❯"]);
    drag(&mut t, (0, 0), (2, 1));
    t.resize(30, 6);
    assert!(!t.record_open());
    let mut t = fullscreen(20, 6);
    paint(&mut t, &["l1", "l2", "l3", "l4", "l5", "❯"]);
    drag(&mut t, (0, 0), (2, 1));
    paint(&mut t, &["x1", "x2", "x3", "x4", "x5", "❯"]);
    assert!(!t.record_open());
}

#[test]
fn copying_from_a_record_joins_the_programs_wraps_as_the_grid_copy_does() {
    let mut t = fullscreen(30, 5);
    // The first row runs to within a word of the width: a wrap the program made, joined on copy.
    paint(&mut t, &["\u{25cf} The limiter is done and all", "  six tests pass.", "", "─", "❯"]);
    drag(&mut t, (0, 0), (16, 1));
    assert_eq!(t.selection_text().as_deref(), Some("\u{25cf} The limiter is done and all\n  six tests pass."));
    assert_eq!(t.copy_text().as_deref(), Some("\u{25cf} The limiter is done and all six tests pass."));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `nice -n 15 cargo test -p arkos terminal::tests::a_record_opens`
Expected: compile errors for `ledger_press`, `record_open`.

- [ ] **Step 3: Implement** the fields, `ledger_press`, `ledger_release`, `record_open`, `rows_text`, `retint`, and the hooks listed in Interfaces; wire `Terminal::mouse_pressed`, `mouse_released`, `mouse_cancelled`. `ledger_press` resets `drag_carry` (Task 5 uses it).

- [ ] **Step 4: Run the tests to verify they pass, and the old ones still do**

Run: `nice -n 15 cargo test -p arkos terminal::` then `nice -n 15 cargo test -p arkos mouse::`
Expected: all pass, the 2026-10-16 selection tests among them.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/terminal.rs
git commit -m "A selection in a program that reports the mouse on the alternate screen is a record that follows the program's scroll"
```

### Task 5: The edge step drives the program's scroll

**Files:**
- Modify: `crates/arkos/src/terminal.rs` (`TerminalCore::drag_scroll_step` at 450, `Terminal::drag_scroll_step` at 823, tests near 1347)

**Interfaces:**
- Consumes: `mouse::{wheel_bytes, WheelCarry}`, `Ledger::set_end`, `line_of_row` (Tasks 2 to 4).
- Produces:
  - `#[derive(Debug, Default, Clone, PartialEq, Eq)] pub struct DragStep { pub more: bool, pub pty_write: Vec<u8> }`
  - `TerminalCore::drag_scroll_step(&mut self, rate: f32, dt: f32, col: usize, anchor: Option<(usize, i32, Side)>, ended: bool) -> DragStep`. The local-history branch is unchanged and returns `pty_write` empty. New branch, taken when a record is open and `!ended` and `mouse_mode()` is `Report { sgr }` and on the alternate screen: `drag_carry.whole_lines(rate * dt)` rows due; `pty_write = wheel_bytes(mode, due, col, edge_row)` with `edge_row` 0 for `rate > 0` and `screen_lines() - 1` otherwise; the record's end set to `End { line: line_of_row(edge_row), col, side }` with the shell's sides (top row `Left`, bottom row `Right`); `retint`; `more: true`. A record open but reporting off (the program turned it off): no bytes, end still set, `more: true`. `Arrows` or `Local` on the alternate screen without a record: `DragStep::default()` as today. `rate == 0.0` is `DragStep::default()` as today.
  - `Terminal::drag_scroll_step(&mut self, rate: f32, dt: f32, col: usize) -> bool`: unchanged signature; writes `pty_write` as `Request::Input` when non-empty and `self.alive`, returns `more`. `terminal_element.rs` needs no change.

- [ ] **Step 1: Write the failing tests**; update the three existing drag tests (`a_drag_past_the_edge_scrolls_the_history...`, `..._never_scrolls_a_program_that_owns_the_mouse`, `an_ended_session_scrolls...`) to read `.more` and, where the program reports the mouse without a record, assert `DragStep::default()`.

```rust
#[test]
fn a_drag_past_the_top_edge_of_a_fullscreen_program_sends_wheel_reports_and_selects_to_the_edge() {
    let mut t = fullscreen(20, 9);
    paint(&mut t, &["l10", "l11", "l12", "l13", "l14", "l15", "l16", "─", "❯"]);
    // Pressed on the last line of the text, column 2.
    t.ledger_press(2, 6, true);
    // 6 rows per second for half a second: three reports at the pointer's column on the top row.
    let step = t.drag_scroll_step(6.0, 0.5, 0, None, false);
    assert!(step.more);
    assert_eq!(step.pty_write, b"\x1b[<64;1;1M".repeat(3));
    assert_eq!(t.selection_text().as_deref(), Some("l10\nl11\nl12\nl13\nl14\nl15\nl16"), "from the top row's left side to the pressed cell");
    // A fraction of a row is carried to the next frame.
    let step = t.drag_scroll_step(6.0, 0.1, 0, None, false);
    assert!(step.pty_write.is_empty() && step.more);
    let step = t.drag_scroll_step(6.0, 0.1, 0, None, false);
    assert_eq!(step.pty_write, b"\x1b[<64;1;1M");
    // The program scrolls two rows per frame; the end stays the top row, an older line each time.
    paint(&mut t, &["l8", "l9", "l10", "l11", "l12", "l13", "l14", "─", "❯"]);
    paint(&mut t, &["l6", "l7", "l8", "l9", "l10", "l11", "l12", "─", "❯"]);
    t.drag_scroll_step(6.0, 0.5, 0, None, false);
    assert_eq!(t.selection_text().as_deref(), Some("l6\nl7\nl8\nl9\nl10\nl11\nl12\nl13\nl14\nl15\nl16"));
    assert_eq!(t.selection_spans().len(), 7, "the tint covers the rows that moved, never the prompt box");
}

#[test]
fn a_drag_past_the_bottom_edge_sends_wheel_down_reports_in_the_programs_encoding() {
    let mut t = TerminalCore::new(20, 8, 100);
    t.apply_output(b"\x1b[?1049h\x1b[?1000h");
    paint(&mut t, &["l10", "l11", "l12", "l13", "l14", "l15", "─", "❯"]);
    t.ledger_press(0, 0, true);
    let step = t.drag_scroll_step(-12.0, 0.25, 3, None, false);
    assert_eq!(step.pty_write, mouse::button_bytes(false, 65, 3, 7, true).repeat(3));
    // Before anything has moved, the snapshot's prompt box lines are still in the record.
    assert_eq!(t.selection_text().as_deref(), Some("l10\nl11\nl12\nl13\nl14\nl15\n─\n❯"));
    // Once the program scrolls, they are dropped and the copy is the text alone.
    paint(&mut t, &["l11", "l12", "l13", "l14", "l15", "l16", "─", "❯"]);
    paint(&mut t, &["l12", "l13", "l14", "l15", "l16", "l17", "─", "❯"]);
    assert_eq!(t.selection_text().as_deref(), Some("l10\nl11\nl12\nl13\nl14\nl15\nl16\nl17"));
}

#[test]
fn reporting_turned_off_mid_record_stops_the_reports_but_keeps_the_record() {
    let mut t = fullscreen(20, 9);
    paint(&mut t, &["l10", "l11", "l12", "l13", "l14", "l15", "l16", "─", "❯"]);
    t.ledger_press(0, 6, true);
    t.apply_all([b"\x1b[?1000l".as_slice()]);
    assert!(t.record_open());
    let step = t.drag_scroll_step(6.0, 0.5, 0, None, false);
    assert!(step.more && step.pty_write.is_empty());
    paint(&mut t, &["l8", "l9", "l10", "l11", "l12", "l13", "l14", "─", "❯"]);
    paint(&mut t, &["l6", "l7", "l8", "l9", "l10", "l11", "l12", "─", "❯"]);
    assert_eq!(t.selection_text().as_deref(), Some("l10\nl11\nl12\nl13\nl14\nl15\nl16"));
}

#[test]
fn an_ended_session_sends_nothing_from_a_drag_past_the_edge() {
    let mut t = fullscreen(20, 9);
    paint(&mut t, &["l10", "l11", "l12", "l13", "l14", "l15", "l16", "─", "❯"]);
    t.ledger_press(0, 6, true);
    assert_eq!(t.drag_scroll_step(6.0, 0.5, 0, None, true), DragStep::default());
}
```

Note for the bottom-edge test's second assertion: before any shift `moved_rows()` is the whole grid, so the prompt box lines are in the range; decide the expected string from the rules (the snapshot's chrome lines are present and provisional, so `"l10\nl11\nl12\nl13\n─\n❯"`), and fix the assertion to that. Chrome leaves the copy at the first commit (Task 2's tests cover it).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `nice -n 15 cargo test -p arkos terminal::tests::a_drag_past`
Expected: compile errors, `DragStep` not found.

- [ ] **Step 3: Implement `DragStep`, the report branch, and `Terminal::drag_scroll_step`'s write.** Export `mouse::button_bytes` for the test if it is private (it is `pub` today).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `nice -n 15 cargo test -p arkos terminal::` then `nice -n 15 cargo test --workspace`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/terminal.rs
git commit -m "A drag held past the edge of a program that reports the mouse scrolls it with wheel reports and grows the record's selection"
```

### Task 6: Documentation, checks, hand check

**Files:**
- Modify: `docs/guide.md` (the selection item near line 371), `docs/development.md` (the status checklist row near line 282), `docs/superpowers/specs/2026-10-13-arkos-design.md` (section 3, after the sentence ending "never a report."), `docs/superpowers/specs/2026-10-27-fullscreen-drag-scroll-design.md` (status line).

- [ ] **Step 1: Guide.** In the selection item, replace "and dragging past the pane's top or bottom edge scrolls the history, except in programs that take the mouse or the whole screen, such as vim or `less`." with: dragging past the pane's top or bottom edge scrolls the history; in a program that reports the mouse, such as Claude Code's fullscreen renderer, it scrolls the program's view instead and Cmd+C copies everything the drag passed over, the sticky header, the "Jump to bottom" overlay and the prompt box left out, and the selection keeps its place on its text while the program scrolls; programs that take the whole screen without reporting the mouse (vim, `less`) do not scroll from a drag. Keep the following sentences.

- [ ] **Step 2: Status checklist.** In `development.md`, replace "no auto-scroll in Claude Code's fullscreen renderer, vim or `less`" with: in a fullscreen `claude` with a long answer, drag from the last line up past the top edge: the conversation scrolls, the tint grows to the top and never covers the prompt box; release, Cmd+C, paste: the whole answer, no "Jump to bottom" text, no prompt box, no repeated header line; wheel after the release and the tint stays on its text; vim and `less` still do not scroll from a drag.

- [ ] **Step 3: Spec pointers.** In `2026-10-13-arkos-design.md` section 3 add: "A drag past the pane's edge in a program that reports the mouse scrolls it with wheel reports and the selection is a record of the rows that passed (`2026-10-27-fullscreen-drag-scroll-design.md`, added 2026-10-27)." Set the new spec's status to "implemented 2026-10-27" once Task 5 is merged.

- [ ] **Step 4: Checks.**

Run: `nice -n 15 cargo test --workspace` then `nice -n 15 cargo check --workspace --all-targets`
Expected: every test passes; the check prints no warnings.

- [ ] **Step 5: Commit**

```bash
git add docs/guide.md docs/development.md docs/superpowers/specs/2026-10-13-arkos-design.md docs/superpowers/specs/2026-10-27-fullscreen-drag-scroll-design.md
git commit -m "The guide, the status checklist and the design spec say what a drag past the edge does in Claude Code's fullscreen renderer"
```

- [ ] **Step 6: Hand check (the user, at the screen).** Build a dev `arkos` (`nice -n 15 cargo build`), run it, open a fullscreen `claude` with a long answer, and run the new status row. What only a person can see: the scroll speed feels like the shell's, the tint never covers the prompt box, and the pasted text is whole.
