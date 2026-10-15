# Markdown Editing (milestone 7) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Markdown files open as an editable rendered document: the block under the caret shows its raw markdown, every other block renders, edits autosave, a pill switches the tab between rendered and raw, and the whole thing stays fast on any document size.

**Architecture:** Three new pure modules under the viewer: `edit.rs` (a line buffer with caret, selection, undo, movement, line-ending round trip; no markdown), `md_blocks.rs` (top-level blocks from `pulldown-cmark` with source ranges, inline runs with source offsets, list and task helpers, a per-line markdown colourer for raw text), and `md_view.rs` (the rendered view as one gpui `list()` of blocks, the active block as a raw editing element). `Viewer` gains a document, a mode, dirty state and saving; the sidebar watcher learns to skip the viewer's own writes; `Tab::Viewer` carries the mode. Text input goes through gpui's `EntityInputHandler` so IME and dead keys work.

**Tech Stack:** Rust 2021, gpui 0.2.2 (`list`, `ListState`, `StyledText`, `TextRun`, `InteractiveText`, `img`, `EntityInputHandler`, `ElementInputHandler`, `canvas`, custom `Element`), `pulldown-cmark` 0.13 (new dependency: options `ENABLE_TABLES | ENABLE_TASKLISTS | ENABLE_STRIKETHROUGH`), `unicode-segmentation` (already a dependency), syntect (already) for code blocks only.

**Spec:** `docs/superpowers/specs/2026-10-15-markdown-editing-design.md` (approved 2026-10-15; plan reviewed on 2026-10-15 and amended, see the rulings in the ledger; its section 1 rulings are binding: autosave plus Cmd+S, Tier A plus task checkboxes with tables read-only, no size cap, the caret's block raw and the rest rendered, nothing but markdown becomes editable; section 9 as amended: the toggle is a pill inside the pane like "jump to bottom"). Section 14 defaults taken: a whole list is one block; `.mdx` counts as markdown.

## Global Constraints

- Machine rules (the user's MacBook Air locked up from build load): every cargo call is `nice -n 15 ~/.cargo/bin/cargo ...`; the worktree's local `.cargo/config.toml` (`jobs = 3`, shared `target-dir`) stays; no `--release`; never two cargo processes at once; focused filters while iterating, the workspace suite once per task; the warning check is `cargo check --workspace --all-targets` and must print no warning from our crates. The benchmark test is `#[ignore]` and run by hand (`-- --ignored md_blocks::tests::a_megabyte_splits_in_time`), never in the suite, because timing under load is not a gate.
- Dead code between tasks: an item a later task first uses, and that a test in this task reaches, gets `#[cfg_attr(not(test), allow(dead_code))] // Used from Task N on.`; an item nothing reaches yet (no test either) gets `#[allow(dead_code)] // Used from Task N on.` (the crate's convention, theme.rs:159, palette.rs:72; `cargo check --all-targets` compiles the test target, where the `cfg_attr` form does not apply). The task that first uses it removes the attribute; after the last task `grep -rn "Used from Task" crates/arkos/src` prints nothing.
- Markdown tabs: a viewer whose path extension is `md`, `markdown` or `mdx` (case-insensitive). Every other viewer is unchanged and read-only.
- Texts (exact): the dirty tab label is the title plus " •"; the conflict banner "The file changed on disk" with buttons "Reload" and "Keep mine"; the save-error banner "cannot save {path}: {error}" with "Retry"; the read-only banners "not UTF-8: shown read-only" and "mixed line endings: shown read-only"; the pill "show raw" while rendered and "show rendered" while raw; keymap actions "toggle markdown view" (`cmd-shift-m`, context `Viewer`) and "save file" (`cmd-s`, context `Viewer`); palette labels "Toggle markdown view" and "Save file".
- Timings: autosave 1 s after the last edit; undo coalescing breaks after 700 ms, a newline, or a caret move.
- Pill geometry: the terminal's "jump to bottom" pill (pane_view.rs ~885): `absolute`, `top(px(10.0))`, `right(px(14.0))`, a fixed width `PILL_W = 104.0` with the text centred, `py(4)`, `rounded(10)`, `banner` background, `text` colour, the pane's family (`root.family()`, as the jump pill and the chip use) at 12 px, `cursor_pointer`, a left mouse-down that focuses the pane and stops propagation; while the find bar is open the pill sits at `top(px(44.0))`. On a markdown tab the inline-diff chip moves to `right(px(14.0 + PILL_W + 8.0))` at the pill's top; on other tabs the chip stays where it is.
- Rendered type: paragraphs in the UI font (`.SystemUIFont`, `chrome::UI_FONT` made `pub`) at `font_size * 1.1`, line height 1.5; headings at 1.9, 1.5, 1.25, 1.1, 1.0, 1.0 times that, semibold; code and the active block in the terminal family at `font_size`; inline code background `tab_inactive`; block quote bar 3 px `border`; rule 1 px `border`; links `focus_ring` colour underlined; the caret 2 px `text`; the selection `find_match` at 0.5 alpha.
- Line endings: the file's `\n` or `\r\n` and whether it ended with a newline are kept and restored on save; in memory lines have no terminators (as `viewer::split_lines` gives them), but the editor's buffer keeps tabs as tabs (it does not use `split_lines`, which turns tabs into spaces).
- Saving writes `.{name}.arkos-{pid}-{n}.tmp` beside the target (`n` from a static `AtomicU64`), copies the target's permissions, calls `sync_all`, and renames; a symlink target is written in place through the link; if the temp file cannot be created but the target is writable, the target is written in place. Before writing, the save compares the file's fingerprint with the one the viewer holds and refuses (raising a conflict) when another writer got there first; right after the rename, still on the background thread, it records the new fingerprint.
- Build profile: the root `Cargo.toml` gets `[profile.dev.package.pulldown-cmark] opt-level = 3` so test builds parse at speed; the benchmark prints its time and its 4 ms gate is judged once by the user in a release build.
- No em-dashes in any file. Comments are full sentences. Headless tests only: nothing opens a window, spawns `open`, or touches files outside a temp dir.

## Review Focus

1. A paste of 10 000 lines into a 20 000-line document: one undo step, the blocks spliced once, no jump to the top (Task 1 `a_paste_is_one_undo_step`, Task 2 `changed_blocks_names_the_edited_range`).
2. A file with `\r\n` endings and no trailing newline, edited and saved: byte-identical except the edit (Task 1 `line_endings_round_trip`, Task 3 `a_save_restores_the_file_s_line_endings`).
3. The user types again within a second of an autosave while the watcher's pass sees the viewer's own write: no reload, no conflict banner, no caret jump (Task 3 `the_disk_change_rule_ignores_our_own_write`).
4. An external edit while the buffer is dirty, then Reload while an autosave timer is pending: the timer must not write the dropped edits over the file (Task 3 `a_timer_after_reload_does_not_write`); then Cmd+S during a conflict keeps mine (Task 3 `the_conflict_buttons_do_what_they_say`).
5. Arrow Down on the last line of the last block, Arrow Up on the first line of the first block, and a click below the last block: no panic, the caret clamps, a click below appends a paragraph (Task 6 `block_navigation_clamps_at_the_ends`, `a_click_below_the_last_block_appends_a_paragraph`).

## File structure

- Create `crates/arkos/src/edit.rs`: `Pos`, `Caret`, `LineEnding`, `Document`, `Edit`, `Undo`; movement and edit primitives. Pure.
- Create `crates/arkos/src/md_blocks.rs`: `Block`, `BlockKind`, `split`, `block_at`, `changed_blocks`, `Inline`, `Run`, `Style`, `inline`, `source_offset_at`, `pos_of_offset`, `list_continuation`, `toggle_task`, `raw_marks`. Pure; depends on `pulldown-cmark`.
- Create `crates/arkos/src/md_view.rs`: `render` (the list), `render_block`, `RawBlockElement`, click-to-caret mapping, block navigation. gpui.
- Modify `crates/arkos/src/viewer.rs`: `is_markdown`, `Viewer.doc`, `mode`, `dirty`, `conflict`, `saving`, `save_seq`, `list_state`; `apply` takes the document the load built; `edit` (the viewer owns autosave and emits `ViewerEvent`), `save`, `disk_change`, `autosave_due`, `undo`, `redo`; `Root` subscribes and keeps the synchronous saves for close and quit. `ViewMode` lives in tree.rs.
- Modify `crates/arkos/src/tree.rs`: `Tab::Viewer { path, #[serde(default)] mode: ViewMode }` and every constructor.
- Modify `crates/arkos/src/pane_view.rs`: `render_viewer` picks raw or rendered, draws the pill, routes keys and input; `tab_labels` adds " •".
- Modify `crates/arkos/src/keymap.rs`, `command_palette.rs`, `config.rs` (DEFAULT_CONFIG), `main.rs` (`run` arms, `flush_for_quit`, `close_tab`), `chrome.rs` (`pub const UI_FONT`).
- Modify `Cargo.toml` (workspace dependency `pulldown-cmark = { version = "0.13", default-features = false }` and the dev profile override) and `crates/arkos/Cargo.toml` (`pulldown-cmark`, `unicode-segmentation = "1"`).
- Modify `README.md`: Status rows, keybinding table, an "Editing markdown" section.

---

### Task 1: The editing core (`edit.rs`)

**Files:**
- Create: `crates/arkos/src/edit.rs`
- Modify: `crates/arkos/src/main.rs` (`mod edit;`), `crates/arkos/Cargo.toml` (`unicode-segmentation = "1"`; it is in Cargo.lock only through gpui today), `Cargo.lock`

**Interfaces:**
- Produces (the module gets the dead-code attribute "Used from Task 3 on."):
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)] pub struct Pos { pub line: usize, pub ch: usize } // ch counts Unicode scalar values
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)] pub struct Caret { pub head: Pos, pub anchor: Pos, pub goal_col: Option<usize> } // goal_col: the display column Up and Down aim for
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum LineEnding { Lf, CrLf }
  pub struct Document { lines: Vec<String>, pub ending: LineEnding, pub trailing_newline: bool, undo: Vec<Step>, redo: Vec<Step>, last_typed: Option<Instant> }
  impl Document {
      pub fn from_text(text: &str) -> Document; // detects the ending from the first line break; an empty text is one empty line
      pub fn to_text(&self) -> String; // restores the ending and the trailing newline
      pub fn lines(&self) -> &[String];
      pub fn line_count(&self) -> usize;
      pub fn clamp(&self, p: Pos) -> Pos;
      pub fn text_in(&self, from: Pos, to: Pos) -> String;
      // Edits: each returns the new caret and records an undo step (see coalescing).
      pub fn insert(&mut self, caret: Caret, text: &str, now: Instant) -> Caret; // replaces the selection if any; "\n" splits lines
      pub fn delete_backward(&mut self, caret: Caret, unit: Unit, now: Instant) -> Caret; // Unit::{Grapheme, Word, LineStart}
      pub fn delete_forward(&mut self, caret: Caret, unit: Unit, now: Instant) -> Caret; // Unit::{Grapheme, Word, LineEnd}
      pub fn replace_range(&mut self, from: Pos, to: Pos, text: &str, now: Instant) -> Pos; // for IME and the task toggle; its own step
      pub fn undo(&mut self) -> Option<Caret>; pub fn redo(&mut self) -> Option<Caret>;
      pub fn break_coalescing(&mut self); // a caret move ends the current typing step
      // Movement (no edit): the new caret; `select` keeps the anchor.
      pub fn move_by(&self, caret: Caret, m: Move, select: bool) -> Caret; // Move::{Left, Right, WordLeft, WordRight, LineStart, LineEnd, Up, Down, DocStart, DocEnd, PageUp(usize), PageDown(usize)}
      pub fn select_all(&self) -> Caret;
  }
  pub fn word_boundary_left(line: &str, ch: usize) -> usize; pub fn word_boundary_right(line: &str, ch: usize) -> usize;
  pub fn grapheme_left(line: &str, ch: usize) -> usize; pub fn grapheme_right(line: &str, ch: usize) -> usize;
  ```
  Coalescing: an `insert` of text without a newline, within 700 ms of the previous coalescable insert and at the caret that insert left, extends that undo step; a newline, a delete, a paste (more than one grapheme, or any newline), or `break_coalescing` starts a new step. `redo` is cleared by any edit. Steps store the inverse edits, not snapshots.

- [ ] **Step 1: Failing tests** (in `edit.rs`'s test module)

```rust
fn doc(s: &str) -> Document { Document::from_text(s) }
fn at(line: usize, ch: usize) -> Caret { Caret { head: Pos { line, ch }, anchor: Pos { line, ch }, goal_col: None } }
fn t0() -> Instant { Instant::now() }

#[test]
fn line_endings_round_trip() {
    for (text, ending, trailing) in [("a\nb\n", LineEnding::Lf, true), ("a\r\nb", LineEnding::CrLf, false), ("", LineEnding::Lf, false), ("one", LineEnding::Lf, false), ("\n", LineEnding::Lf, true)] {
        let d = doc(text);
        assert_eq!((d.ending, d.trailing_newline), (ending, trailing), "{text:?}");
        assert_eq!(d.to_text(), text, "{text:?}");
    }
    assert_eq!(doc("").lines(), &[String::new()], "an empty document is one empty line");
    assert_eq!(doc("a\tb").lines(), &["a\tb".to_string()], "tabs stay tabs");
}

#[test]
fn inserting_and_deleting_graphemes_and_words() {
    let mut d = doc("héllo wörld");
    let c = d.insert(at(0, 5), ",", t0());
    assert_eq!((d.lines()[0].as_str(), c.head), ("héllo, wörld", Pos { line: 0, ch: 6 }));
    let c = d.delete_backward(c, Unit::Word, t0());
    assert_eq!((d.lines()[0].as_str(), c.head.ch), (", wörld", 0), "a word delete takes the word, not the punctuation after it");
    let mut d = doc("e\u{301}x"); // e with a combining accent, then x
    let c = d.delete_backward(at(0, 2), Unit::Grapheme, t0());
    assert_eq!((d.lines()[0].as_str(), c.head.ch), ("x", 0), "one grapheme, two scalars");
    let mut d = doc("ab\ncd");
    let c = d.delete_backward(at(1, 0), Unit::Grapheme, t0());
    assert_eq!((d.lines(), c.head), (&["abcd".to_string()][..], Pos { line: 0, ch: 2 }), "backspace at a line start joins");
    let c = d.insert(c, "\n\n", t0());
    assert_eq!((d.lines().len(), c.head), (3, Pos { line: 2, ch: 0 }));
    let c = d.delete_forward(c, Unit::LineEnd, t0());
    assert_eq!((d.lines()[2].as_str(), c.head.ch), ("", 0));
    let c = d.delete_backward(c, Unit::LineStart, t0());
    assert_eq!(c.head, Pos { line: 2, ch: 0 }, "nothing before the caret: no change, no panic");
}

#[test]
fn a_selection_is_replaced_by_what_is_typed() {
    let mut d = doc("abc\ndef");
    let sel = Caret { head: Pos { line: 1, ch: 1 }, anchor: Pos { line: 0, ch: 1 }, goal_col: None };
    assert_eq!(d.text_in(sel.anchor, sel.head), "bc\nd");
    let c = d.insert(sel, "X", t0());
    assert_eq!((d.lines(), c.head), (&["aXef".to_string()][..], Pos { line: 0, ch: 2 }));
    assert_eq!(c.anchor, c.head, "typing collapses the selection");
    assert_eq!(d.select_all(), Caret { head: Pos { line: 0, ch: 4 }, anchor: Pos { line: 0, ch: 0 }, goal_col: None });
}

#[test]
fn typing_coalesces_into_one_undo_step_until_a_pause_newline_or_move() {
    let mut d = doc("");
    let t = t0();
    let mut c = at(0, 0);
    for (i, ch) in ["h", "e", "y"].iter().enumerate() {
        c = d.insert(c, ch, t + Duration::from_millis(100 * i as u64));
    }
    c = d.insert(c, " ", t + Duration::from_millis(1200)); // a pause: a new step
    c = d.insert(c, "x", t + Duration::from_millis(1250));
    assert_eq!(d.lines()[0], "hey x");
    assert_eq!(d.undo().map(|c| c.head.ch), Some(3), "the second step is the space and the x");
    assert_eq!(d.lines()[0], "hey");
    assert_eq!(d.undo().map(|c| c.head.ch), Some(0));
    assert_eq!(d.lines()[0], "");
    assert_eq!(d.undo(), None);
    assert_eq!(d.redo().map(|c| c.head.ch), Some(3));
    assert_eq!(d.redo().map(|c| c.head.ch), Some(5));
    assert_eq!(d.redo(), None);
    let c2 = d.insert(c, "\n", t + Duration::from_millis(1300));
    let _ = d.insert(c2, "a", t + Duration::from_millis(1310));
    assert_eq!(d.lines().len(), 2);
    d.undo();
    assert_eq!(d.lines(), &["hey x".to_string(), String::new()][..], "the newline is its own step, the a another");
    d.break_coalescing();
    let c3 = d.insert(at(1, 0), "b", t + Duration::from_millis(1320));
    let _ = d.insert(c3, "c", t + Duration::from_millis(1330));
    assert_eq!(d.redo(), None, "an edit clears the redo stack");
}

#[test]
fn a_paste_is_one_undo_step() {
    let mut d = doc("x");
    let big: String = (0..10_000).map(|i| format!("line {i}\n")).collect();
    let c = d.insert(at(0, 1), &big, t0());
    assert_eq!(d.line_count(), 10_001);
    assert_eq!(c.head, Pos { line: 10_000, ch: 0 });
    d.undo();
    assert_eq!(d.lines(), &["x".to_string()][..]);
}

#[test]
fn movement_by_word_line_and_document_with_a_goal_column() {
    let d = doc("short\na much longer line here\n");
    let c = d.move_by(at(1, 10), Move::WordRight, false);
    assert_eq!(c.head.ch, 13, "to the end of 'longer'");
    let c = d.move_by(c, Move::WordLeft, false);
    assert_eq!(c.head.ch, 7, "to the start of 'longer'");
    let c = d.move_by(at(1, 10), Move::Up, false);
    assert_eq!((c.head, c.goal_col), (Pos { line: 0, ch: 5 }, Some(10)), "Up clamps to the short line and remembers the goal");
    let c = d.move_by(c, Move::Down, false);
    assert_eq!(c.head, Pos { line: 1, ch: 10 }, "Down comes back to the goal column");
    let c = d.move_by(c, Move::LineEnd, false);
    assert_eq!(c.head.ch, 23);
    let c = d.move_by(c, Move::LineStart, true);
    assert_eq!((c.head.ch, c.anchor.ch), (0, 23), "Shift keeps the anchor");
    let c = d.move_by(at(1, 3), Move::DocStart, false);
    assert_eq!(c.head, Pos { line: 0, ch: 0 });
    let c = d.move_by(c, Move::DocEnd, false);
    assert_eq!(c.head, Pos { line: 2, ch: 0 });
    let c = d.move_by(at(0, 0), Move::Left, false);
    assert_eq!(c.head, Pos { line: 0, ch: 0 }, "the document start stays put");
    let c = d.move_by(at(0, 5), Move::Right, false);
    assert_eq!(c.head, Pos { line: 1, ch: 0 }, "Right at a line end goes to the next line");
}
```

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- edit::`

- [ ] **Step 3: Implement**

Module doc: "A line buffer with one caret for the markdown editor and, later, the code editor: positions, selection, edits with undo, movement by grapheme, word, line and page. Knows nothing about markdown." Graphemes through `unicode_segmentation::UnicodeSegmentation::grapheme_indices`; words are runs of alphanumerics (`char::is_alphanumeric` or `_`), the macOS way: Option+Right stops at the end of the current word, Option+Left at its start, skipping punctuation and spaces. `Pos.ch` counts chars; converting to byte offsets happens inside the module (`byte_of(line, ch)`). Steps: `struct Step { edits: Vec<(Pos, Pos, String, String)>, caret_before: Caret, caret_after: Caret, coalescable: bool }` where each edit is "range, old text, new text"; undo applies the inverses in reverse order. Keep every function total: positions are clamped, never indexed raw.

- [ ] **Step 4: Run, check, commit**

Run the focused tests (6 passed), the warning check, then the workspace suite once.

(The movement test also covers `Move::PageUp(10)` and `Move::PageDown(10)`: they move by that many lines, clamped, keeping the goal column; otherwise the test build warns that the variants are never constructed.)

```bash
git add crates/arkos/src/edit.rs crates/arkos/src/main.rs crates/arkos/Cargo.toml Cargo.lock
git commit -m "An editing core: a line buffer with caret, selection, undo and movement"
```

---

### Task 2: Markdown blocks (`md_blocks.rs`)

**Files:**
- Modify: `Cargo.toml` (workspace dependency `pulldown-cmark = { version = "0.13", default-features = false }`; `[profile.dev.package.pulldown-cmark] opt-level = 3`), `crates/arkos/Cargo.toml` (`pulldown-cmark.workspace = true`), `Cargo.lock`
- Create: `crates/arkos/src/md_blocks.rs`
- Modify: `crates/arkos/src/main.rs` (`mod md_blocks;`)

**Interfaces:**
- Produces (attributes: `split`, `block_at`, `changed_blocks`, `raw_marks`, `pos_of_offset` "Used from Task 3 on."; `list_continuation`, `toggle_task` "Used from Task 4 on."; `inline`, `Inline`, `Run`, `Style`, `source_offset_at` "Used from Task 5 on."; all are reached by tests here, so the `cfg_attr` form):
  ```rust
  pub const OPTIONS: Options = ENABLE_TABLES | ENABLE_TASKLISTS | ENABLE_STRIKETHROUGH | ENABLE_YAML_STYLE_METADATA_BLOCKS;
  #[derive(Debug, Clone, PartialEq, Eq)] pub enum BlockKind { Paragraph, Heading(u8), List, CodeBlock { info: String }, Quote, Rule, Table, Html, Metadata, Blank }
  #[derive(Debug, Clone, PartialEq, Eq)] pub struct Block { pub lines: Range<usize>, pub kind: BlockKind }
  pub fn split(lines: &[String]) -> Vec<Block>; // block i covers its start up to block i+1's start; leading blank lines are a Blank block; every line in exactly one block
  pub fn block_at(blocks: &[Block], line: usize) -> usize;
  /// The blocks an edit replaced: the common prefix and suffix of the old and new structures leave `range` in the old list, replaced by `count` new blocks.
  pub fn changed_blocks(old: &[Block], new: &[Block]) -> (Range<usize>, usize);
  #[derive(Debug, Clone, PartialEq, Eq, Default)] pub struct Style { pub bold: bool, pub italic: bool, pub code: bool, pub strike: bool, pub html: bool, pub link: Option<String> }
  #[derive(Debug, Clone, PartialEq, Eq)] pub struct Run { pub text: String, pub style: Style, pub source: Range<usize> } // source: the byte range of the run's own text in the block's text (inside any markers)
  #[derive(Debug, Clone, PartialEq, Eq)] pub enum Inline { Para(Vec<Run>), Heading(u8, Vec<Run>), Item { depth: u8, marker: String, task: Option<bool>, runs: Vec<Run> }, Code { info: String, text: String }, Quote(Vec<Run>), Rule, Table(String), Html(String), Metadata(String), Image { alt: String, path: String } }
  pub fn inline(block_text: &str, kind: &BlockKind) -> Vec<Inline>; // SoftBreak becomes " ", HardBreak "\n", InlineHtml a run with style.html; Code's source range excludes the backticks
  pub fn source_offset_at(runs: &[Run], rendered_byte: usize) -> usize; // a byte index into the rendered string (the runs' texts joined) to a byte offset in the block's text; inside a run, linear within its source; between runs, the next run's start; past the end, the end of the last run's source
  pub fn pos_of_offset(block: &Block, lines: &[String], byte: usize) -> edit::Pos; // a byte offset in the block's text (lines joined with "\n") to a document position
  pub fn list_continuation(line: &str) -> Option<(String, bool)>;
  pub fn toggle_task(line: &str) -> Option<String>;
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Mark { Marker, Heading, Emphasis, Code, Link, Quote }
  pub fn raw_marks(line: &str) -> Vec<(Range<usize>, Mark)>; // may leave gaps; the painter fills them in the text colour
  ```

- [ ] **Step 1: Failing tests**

The six tests from the first plan draft stay (`every_line_lands_in_exactly_one_block_in_order` now expects a leading `Blank` block for a document that starts with blank lines, a `Metadata` block for `---\ntitle: x\n---` front matter, and `inline_runs_carry_styles_and_source_offsets` adds `("a\nb", Paragraph)` giving runs "a", " ", "b" and `("a  \nb")` giving "a", "\n", "b"), plus:

```rust
#[test]
fn changed_blocks_names_the_edited_range() {
    let a = split(&lines("# T\n\npara\n\n- x\n"));
    let b = split(&lines("# T\n\npara two\n\n- x\n"));
    assert_eq!(changed_blocks(&a, &b), (1..2, 1), "one block's text changed");
    let c = split(&lines("# T\n\npara\n\nnew para\n\n- x\n"));
    assert_eq!(changed_blocks(&a, &c), (2..2, 1), "a block inserted before the list");
    let d = split(&lines("# T\n\n- x\n"));
    assert_eq!(changed_blocks(&a, &d), (1..2, 0), "a block removed");
    assert_eq!(changed_blocks(&a, &a), (a.len()..a.len(), 0), "nothing changed");
    assert_eq!(changed_blocks(&[], &a), (0..0, a.len()));
}

#[test]
fn rendered_bytes_map_back_to_the_source_and_to_positions() {
    let runs = match &inline("a **bold** c", &BlockKind::Paragraph)[0] { Inline::Para(r) => r.clone(), _ => unreachable!() };
    // Rendered text "a bold c": bytes 0 'a', 2..6 "bold", 7 'c'.
    assert_eq!(source_offset_at(&runs, 0), 0);
    assert_eq!(source_offset_at(&runs, 2), 4, "inside the bold run: past the opening **");
    assert_eq!(source_offset_at(&runs, 5), 7);
    assert_eq!(source_offset_at(&runs, 7), 11, "the c after the closing **");
    assert_eq!(source_offset_at(&runs, 99), 12, "past the end: the block's end");
    let doc = lines("# T\n\nab\ncd\n");
    let blocks = split(&doc);
    assert_eq!(pos_of_offset(&blocks[1], &doc, 0), edit::Pos { line: 2, ch: 0 });
    assert_eq!(pos_of_offset(&blocks[1], &doc, 3), edit::Pos { line: 3, ch: 0 }, "past the first line's newline");
    assert_eq!(pos_of_offset(&blocks[1], &doc, 99), edit::Pos { line: 3, ch: 2 }, "clamped to the block's end");
}
```

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- md_blocks::`

- [ ] **Step 3: Implement**

`split` joins the lines with `\n`, runs `Parser::new_ext(text, OPTIONS).into_offset_iter()`, and records a start offset and kind for every depth-0 `Event::Start` (`Tag::Paragraph`, `Tag::Heading { level, .. }` with `level as u8`, `Tag::List(_)`, `Tag::CodeBlock(kind)`, `Tag::BlockQuote(_)`, `Tag::Table(_)`, `Tag::HtmlBlock`, `Tag::MetadataBlock(_)`) and for `Event::Rule` (not a start event). Block i spans from its start line to block i+1's start line; the first block starts at line 0, as a `Blank` block when the first start is later; a document whose last block is followed by blank lines keeps them in that block; an empty document is one `Blank` block. `changed_blocks` compares `Block` values for the common prefix and suffix.

`inline` parses the block's own text with `OPTIONS` and `into_offset_iter`, keeping a `Style` stack through `Start`/`End` of `Strong`, `Emphasis`, `Strikethrough`, `Link { dest_url, .. }`; `Event::Text(t)` gives a run over the event's range; `Event::Code(t)` a code run whose source range is the event range shrunk by the backticks on each side; `SoftBreak` a plain " " run with the break's range; `HardBreak` a "\n" run; `InlineHtml` a run with `style.html`. For a list, every `Tag::Item` opens an `Item` whose depth is the list nesting at that point, whose marker is read from the source line (`-`, `*`, `+`, or the number with its delimiter) and whose `task` comes from `Event::TaskListMarker` inside the item (or inside the item's paragraph in a loose list). A paragraph whose only content is one `Tag::Image` gives `Image`. `source_offset_at` walks the runs accumulating rendered bytes.

`list_continuation`, `toggle_task` and `raw_marks` are hand scanners over one line.

- [ ] **Step 4: Run, check, commit**

Run the focused tests (8 passed, 1 ignored); run the benchmark by hand once and put the printed time in the commit message; the warning check; the suite.

```bash
git add Cargo.toml Cargo.lock crates/arkos/Cargo.toml crates/arkos/src/md_blocks.rs crates/arkos/src/main.rs
git commit -m "Markdown blocks: top-level blocks with source ranges, inline runs, list helpers"
```

### Task 2b (contingent): incremental split

Only if the user's release-build run of `a_megabyte_splits_in_time` misses 4 ms. `split_from(old: &[Block], lines: &[String], edited: Range<usize>) -> Vec<Block>` re-parses from the start of the first block touching `edited` to the first later block boundary where the old and new structures agree, and reuses the rest; an unclosed fence or HTML block reflows to the end. Tests: equality with `split` on fixtures including an opened and a closed fence.

---

### Task 3: The document in the viewer, the mode, saving and external changes

**Files:**
- Modify: `crates/arkos/src/edit.rs` (`Document` stores `Arc<Vec<String>>`, edits through `Arc::make_mut`; `shared_lines() -> Arc<Vec<String>>`; `bom: bool`), `crates/arkos/src/viewer.rs`, `crates/arkos/src/tree.rs` (`ViewMode`, `Tab::Viewer`, `set_viewer_mode`; patterns needing `..` at tree.rs:510 and in tests at 728-729), `crates/arkos/src/chrome.rs` (`pub const UI_FONT`), the `Tab::Viewer { path }` constructors and patterns (sidebar.rs:1058, 1062; main.rs:1317; pane_view.rs:368, 400; tests at main.rs:3936, pane_view.rs:1309, sidebar.rs:1534, 1610, sessions.rs:695, 765), `crates/arkos/src/main.rs` (`close_tab` ~1282, `flush_for_quit` 2830 and its call sites 2850, 3043, 3806; `leaving` 2844), `crates/arkos/src/sidebar.rs` (`OpenPlan::Replace` at 1053), `crates/arkos/src/pane_view.rs` (`tab_labels`), `crates/arkos/src/diff.rs` (none; `diff_of` is called with `split_lines(&doc.to_text())`)

**Interfaces:**
- Consumes: Task 1, Task 2's `split`, `block_at`, `changed_blocks`, `raw_marks`.
- Produces:
  ```rust
  // tree.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)] pub enum ViewMode { #[default] Rendered, Raw }
  Tab::Viewer { path: PathBuf, #[serde(default)] mode: ViewMode }
  impl Tree { pub fn set_viewer_mode(&mut self, pane: PaneId, idx: usize, mode: ViewMode); }
  // viewer.rs
  pub fn is_markdown(path: &Path) -> bool;
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum ReadOnlyWhy { NotUtf8, MixedEndings }
  pub struct Loaded { ..., pub doc: Option<edit::Document>, pub read_only: Option<ReadOnlyWhy> } // doc built on the background thread for markdown files that are strict UTF-8 with one line ending (a BOM is stripped into Document.bom)
  pub struct Viewer { ..., pub doc: Option<edit::Document>, pub caret: edit::Caret, pub blocks: Vec<md_blocks::Block>, pub mode: ViewMode, pub dirty: bool, pub conflict: bool, pub saving: bool, pub save_again: bool, pub save_error: Option<String>, pub save_seq: u64, pub marked: Option<Range<edit::Pos>>, pub read_only: Option<ReadOnlyWhy> }
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum ViewerEvent { Edited, Saved, SaveFailed }
  impl EventEmitter<ViewerEvent> for Viewer {}
  impl Viewer {
      /// Runs an edit, refreshes `content.lines` (an Arc clone of the document's), splices `blocks` by `changed_blocks`, clears `diff` until the next save, sets dirty, bumps save_seq, schedules the autosave, emits Edited, notifies.
      pub fn edit(&mut self, f: impl FnOnce(&mut edit::Document, edit::Caret, Instant) -> edit::Caret, cx: &mut Context<Self>);
      pub fn undo(&mut self, cx); pub fn redo(&mut self, cx);
      pub fn set_mode(&mut self, mode: ViewMode, cx);
      pub fn save(&mut self, cx); // the background write with the pre-write fingerprint check; records the new fingerprint on the background thread; emits Saved or SaveFailed
      pub fn resolve_conflict(&mut self, keep_mine: bool, cx);
  }
  /// What a load that found the file changed should do.
  pub fn disk_change(disk: Option<(u128, u64)>, known: Option<(u128, u64)>, dirty: bool, saving: bool) -> DiskChange; // Ignore when disk == known or saving; Conflict when dirty; else Reload
  /// Whether the autosave timer that carried `carried` may write now.
  pub fn autosave_due(carried: u64, save_seq: u64, dirty: bool, conflict: bool, saving: bool, composing: bool) -> bool;
  pub fn save_text(path: &Path, text: &str, expect: Option<(u128, u64)>) -> Result<(u128, u64), SaveError>; // SaveError::{Changed, Io(io::Error)}: refuses when the file's fingerprint is not `expect`; returns the fingerprint after the write
  impl Root { pub fn active_viewer_tab(&self) -> Option<TabId>; pub fn save_dirty_viewers_now(&mut self, cx: &mut App) -> Vec<String>; /* errors */ pub fn toggle_markdown_view_for(&mut self, tab: TabId, cx); }
  ```

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn markdown_is_known_by_extension() { /* as in the first draft */ }

#[test]
fn a_save_restores_the_file_s_line_endings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("n.md");
    std::fs::write(&path, "\u{feff}# T\r\n\r\ntext").unwrap();
    let mut d = edit::Document::from_text(&std::fs::read_to_string(&path).unwrap());
    assert!(d.bom && d.lines()[0] == "# T", "the BOM is a flag, not text");
    d.insert(edit::Caret::default(), "X", std::time::Instant::now());
    let fp = crate::config_watch::fingerprint(&path);
    save_text(&path, &d.to_text(), fp).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), "\u{feff}X# T\r\n\r\ntext".as_bytes());
}

#[test]
fn a_save_is_atomic_keeps_permissions_follows_a_symlink_and_refuses_a_changed_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real.md");
    std::fs::write(&real, "old").unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = dir.path().join("link.md");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let fp = crate::config_watch::fingerprint(&link);
    let after = save_text(&link, "new", fp).unwrap();
    assert_eq!(std::fs::read_to_string(&real).unwrap(), "new", "written through the link");
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link is still a link");
    assert_eq!(Some(after), crate::config_watch::fingerprint(&link), "the returned fingerprint is the file's");
    std::thread::sleep(std::time::Duration::from_millis(20));
    let after2 = save_text(&real, "newer", Some(after)).unwrap();
    assert_eq!(std::fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o600, "permissions copied onto the renamed file");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2, "no temp file left behind");
    std::fs::write(&real, "someone else").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert!(matches!(save_text(&real, "mine", Some(after2)), Err(SaveError::Changed)), "another writer got there first");
    assert_eq!(std::fs::read_to_string(&real).unwrap(), "someone else", "and their file is untouched");
    assert!(save_text(&real, "forced", None).is_ok(), "no expectation: Keep mine");
}

#[test]
fn an_unwritable_target_leaves_the_file_intact() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("ro");
    std::fs::create_dir(&sub).unwrap();
    let path = sub.join("a.md");
    std::fs::write(&path, "keep").unwrap();
    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o500)).unwrap();
    let fp = crate::config_watch::fingerprint(&path);
    let err = save_text(&path, "lost?", fp);
    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(err, Err(SaveError::Io(_))), "{err:?}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep");
    assert_eq!(std::fs::read_dir(&sub).unwrap().count(), 1, "no temp file left behind");
}

#[test]
fn the_disk_change_rule_ignores_our_own_write() {
    let ours = Some((1, 10));
    assert_eq!(disk_change(ours, ours, true, false), DiskChange::Ignore, "our own write seen while dirty again");
    assert_eq!(disk_change(Some((2, 10)), ours, false, true), DiskChange::Ignore, "a save in flight: re-check when it lands");
    assert_eq!(disk_change(Some((2, 10)), ours, true, false), DiskChange::Conflict);
    assert_eq!(disk_change(Some((2, 10)), ours, false, false), DiskChange::Reload);
    assert_eq!(disk_change(None, ours, false, false), DiskChange::Reload, "the file went away: reload shows why");
}

#[test]
fn a_timer_after_reload_does_not_write() {
    assert!(autosave_due(3, 3, true, false, false, false));
    assert!(!autosave_due(3, 4, true, false, false, false), "a newer edit rescheduled");
    assert!(!autosave_due(3, 3, false, false, false, false), "nothing dirty: Reload bumped save_seq and cleared dirty");
    assert!(!autosave_due(3, 3, true, true, false, false), "a conflict waits for the user");
    assert!(!autosave_due(3, 3, true, false, true, false), "a save in flight: save_again instead");
    assert!(!autosave_due(3, 3, true, false, false, true), "an IME composition in progress");
}

#[test]
fn the_conflict_buttons_do_what_they_say() {
    // The pure state machine behind resolve_conflict: (conflict, dirty, save_seq) before and after.
    let s = ConflictState { conflict: true, dirty: true, save_seq: 7 };
    assert_eq!(s.resolve(true), (ConflictState { conflict: false, dirty: true, save_seq: 7 }, ConflictAction::SaveNow), "Keep mine saves at once, adopting the disk fingerprint");
    assert_eq!(s.resolve(false), (ConflictState { conflict: false, dirty: false, save_seq: 8 }, ConflictAction::Reload), "Reload drops the edits and bumps the sequence so a pending timer stays quiet");
}

#[test]
fn markdown_loads_as_a_document_only_when_it_is_clean_utf8_with_one_ending() {
    let dir = tempfile::tempdir().unwrap();
    let ok = dir.path().join("ok.md"); std::fs::write(&ok, "# a\n\tb\n").unwrap();
    let l = read_file(&ok, None, None, 1 << 20, false).unwrap();
    assert_eq!(l.doc.as_ref().unwrap().lines(), &["# a".to_string(), "\tb".to_string(), String::new()][..], "the document keeps its tab and its phantom last line");
    assert_eq!(l.read_only, None);
    let bad = dir.path().join("bad.md"); std::fs::write(&bad, b"# a\n\xff\n").unwrap();
    let l = read_file(&bad, None, None, 1 << 20, false).unwrap();
    assert!(l.doc.is_none()); assert_eq!(l.read_only, Some(ReadOnlyWhy::NotUtf8));
    let mixed = dir.path().join("mixed.md"); std::fs::write(&mixed, "a\r\nb\nc").unwrap();
    let l = read_file(&mixed, None, None, 1 << 20, false).unwrap();
    assert!(l.doc.is_none()); assert_eq!(l.read_only, Some(ReadOnlyWhy::MixedEndings));
    let rs = dir.path().join("x.rs"); std::fs::write(&rs, "fn main() {}\n").unwrap();
    assert!(read_file(&rs, None, None, 1 << 20, false).unwrap().doc.is_none(), "not markdown: no document");
}

// tree.rs
#[test]
fn a_viewer_tab_s_mode_round_trips_and_an_old_file_renders() { /* as in the first draft */ }
```

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- viewer:: tree:: edit::`

- [ ] **Step 3: Implement**

- `edit::Document`: `lines: Arc<Vec<String>>` with `Arc::make_mut` in every edit (a copy only while a find or diff snapshot holds the old Arc); `shared_lines()`; `bom: bool` set by `from_text` when the text starts with U+FEFF and written back by `to_text`. Task 1's tests still pass unchanged.
- `read_file`: for `is_markdown(path)` it skips syntect; if the bytes are strict UTF-8 (`std::str::from_utf8`) and the line endings are uniform (no `\r\n` mixed with a bare `\n`), it builds the `Document` on the background thread and sets `content.lines = doc.shared_lines()` (tabs kept, the phantom last line kept) with empty spans; else `read_only = Some(why)`, no document, the old plain path. `Loaded.doc` and `read_only` are `None` for every other file.
- `apply`: takes `doc`, `caret` clamped, `blocks = split(...)`, `dirty = false`, `conflict = false`; undo history cleared when the text differs from the old buffer; `read_only` stored (Task 7 shows its banner).
- `Root::load`'s landing closure: when `read_file` returns `Some(loaded)` for a viewer with `doc.is_some()`, apply `disk_change(loaded.fingerprint, v.fingerprint, v.dirty, v.saving)`: `Ignore` drops the load; `Conflict` sets `conflict = true` and drops it; `Reload` applies. `reload_all_viewers` skips viewers with a document (a theme change repaints them; nothing to recolour).
- `Viewer::edit`: as the interface says; `blocks` spliced with `changed_blocks` (Task 5 mirrors the splice onto the list state and the block cache); the autosave timer: `let seq = self.save_seq; cx.spawn(async move |this, cx| { cx.background_executor().timer(Duration::from_secs(1)).await; let _ = this.update(cx, |v, cx| if autosave_due(seq, v.save_seq, v.dirty, v.conflict, v.saving, v.marked.is_some()) { v.save(cx) }); }).detach();`.
- `Viewer::save`: `saving = true`; the text and `expect = self.fingerprint` go to the background executor with `save_text`; on `Ok(fp)`: `fingerprint = Some(fp)`, `dirty = save_seq != seq_at_start`, `saving = false`, `save_error = None`, then `diff_of(path, split_lines(&text))` on the background executor as `rediff_viewers` does, emit `Saved`; on `Err(Changed)`: `saving = false`, `conflict = true`; on `Err(Io(e))`: `saving = false`, `save_error = Some(format!("cannot save {}: {e}", path.display()))`, emit `SaveFailed`; if `save_again` was set meanwhile, save once more.
- `resolve_conflict(keep_mine)`: through `ConflictState::resolve`; `SaveNow` adopts the disk fingerprint (`fingerprint = config_watch::fingerprint(path)`) before saving; `Reload` bumps `save_seq`, clears `dirty` and `conflict`, and reloads.
- Root: `viewer_for` initialises `mode` from the tab and subscribes with `cx.subscribe(&v, |root, v, event, cx| ...)`: on `Edited`, pin the preview tab when it is this one (`preview_tab = None`) and `cx.notify()` (the " •" label); `sidebar.rs:1053` (`OpenPlan::Replace`) saves a dirty viewer synchronously before removing it; `close_tab`: a dirty viewer is saved synchronously (`save_text` on the UI thread; a 1 MB write is milliseconds); on `Err`, the tab stays open with the error banner; when `conflict` is set the tab stays open and nothing is written; `flush_for_quit(&mut self, goodbye: bool, cx: &mut App)` and `leaving(&mut self, cx: &mut App)` call `save_dirty_viewers_now(cx)` (the four call sites change, the closures at 3791 and 3806 take `cx`); `toggle_markdown_view_for(tab)` sets both the tree's and the viewer's mode and saves the layout; `active_viewer_tab()`.
- `tab_labels`: `" •"` for a dirty viewer.

- [ ] **Step 4: Run, check, commit**

Run the focused tests, the warning check (attributes: what Task 4 first uses keeps "Used from Task 4 on."), the suite.

```bash
git add Cargo.lock crates/arkos/src/edit.rs crates/arkos/src/viewer.rs crates/arkos/src/tree.rs crates/arkos/src/chrome.rs crates/arkos/src/sidebar.rs crates/arkos/src/main.rs crates/arkos/src/pane_view.rs crates/arkos/src/sessions.rs
git commit -m "A markdown viewer holds an editable document, a mode, and saves it safely"
```

---

### Task 4: Text input and the editing keys (raw mode becomes an editor)

**Files:**
- Modify: `crates/arkos/src/viewer.rs` (`impl EntityInputHandler for Viewer`, `key`, `KeyAction`, `key_action`, `apply_action`, `normalize_paste`, `utf16` helpers in edit.rs), `crates/arkos/src/viewer_element.rs` (free painting helpers: `paint_caret`, `paint_selection`, `paint_marked`, `paint_hits`, `display_col`/`col_to_x` with tab expansion; a horizontal scroll `scroll_x`; `raw_marks` colours at paint time), `crates/arkos/src/pane_view.rs` (`render_viewer`: key routing, no scroll fallback for documents), `crates/arkos/src/keymap.rs`, `command_palette.rs`, `config.rs` (DEFAULT_CONFIG), `main.rs` (`run` arms)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum KeyAction { Move(Move, bool), DeleteBack(Unit), DeleteFwd(Unit), Newline, Tab, ShiftTab, Undo, Redo, SelectAll, Copy, Cut, Paste, Save, Escape, ScrollHome, ScrollEnd }
  pub fn key_action(ks: &Keystroke) -> Option<KeyAction>; // pageup/pagedown give Move(PageUp/PageDown(rows), shift); home/end give ScrollHome/ScrollEnd (the view scrolls, the caret stays); space gives None (it is text)
  #[derive(Debug, Clone, PartialEq, Eq)] pub enum Outcome { Edited, Moved, Copy(String), Cut(String), Paste, Save, Escape, Scroll(KeyAction), Nothing }
  /// Applies an action to the document and caret; `blocks_before` is the block structure before this action (the fence rule reads it). Pure: no gpui.
  pub fn apply_action(doc: &mut edit::Document, caret: &mut edit::Caret, blocks_before: &[md_blocks::Block], action: KeyAction, rows: usize, now: Instant) -> Outcome;
  pub fn normalize_paste(text: &str) -> String; // "\r\n" and lone "\r" become "\n"
  impl Viewer { pub fn key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool; } // true when handled: the caller stops propagation
  impl edit::Document { pub fn pos_to_utf16(&self, from: Pos, p: Pos) -> usize; pub fn utf16_to_pos(&self, from: Pos, units: usize) -> Pos; } // offsets relative to `from` (the active block's first line)
  ```
  `EntityInputHandler for Viewer`: all eight methods (`text_for_range`, `selected_text_range`, `marked_text_range`, `unmark_text`, `replace_text_in_range`, `replace_and_mark_text_in_range`, `bounds_for_range`, `character_index_for_point`); the field is the active block's text in both modes (`block_at(blocks, caret.head.line)`), UTF-16 offsets relative to the block's first line. `replace_text_in_range(None | the selection, text)` calls `Document::insert` (so typing coalesces); an explicit range that is not the selection calls `replace_range`. Marked (IME) text is written with `replace_range` and records no undo step until it is committed by `replace_text_in_range`; `marked` is cleared by `unmark_text`.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn keystrokes_map_to_editor_actions_the_mac_way() {
    let k = |s: &str| key_action(&gpui::Keystroke::parse(s).unwrap());
    assert_eq!(k("left"), Some(KeyAction::Move(Move::Left, false)));
    assert_eq!(k("shift-right"), Some(KeyAction::Move(Move::Right, true)));
    assert_eq!(k("alt-left"), Some(KeyAction::Move(Move::WordLeft, false)));
    assert_eq!(k("cmd-left"), Some(KeyAction::Move(Move::LineStart, false)));
    assert_eq!(k("cmd-shift-right"), Some(KeyAction::Move(Move::LineEnd, true)));
    assert_eq!(k("cmd-up"), Some(KeyAction::Move(Move::DocStart, false)));
    assert_eq!(k("pagedown"), Some(KeyAction::Move(Move::PageDown(0), false)), "the rows are filled in by the caller");
    assert_eq!(k("home"), Some(KeyAction::ScrollHome));
    assert_eq!(k("backspace"), Some(KeyAction::DeleteBack(Unit::Grapheme)));
    assert_eq!(k("alt-backspace"), Some(KeyAction::DeleteBack(Unit::Word)));
    assert_eq!(k("cmd-backspace"), Some(KeyAction::DeleteBack(Unit::LineStart)));
    assert_eq!(k("delete"), Some(KeyAction::DeleteFwd(Unit::Grapheme)));
    assert_eq!(k("enter"), Some(KeyAction::Newline));
    assert_eq!(k("tab"), Some(KeyAction::Tab));
    assert_eq!(k("shift-tab"), Some(KeyAction::ShiftTab));
    assert_eq!(k("cmd-z"), Some(KeyAction::Undo));
    assert_eq!(k("cmd-shift-z"), Some(KeyAction::Redo));
    assert_eq!(k("cmd-a"), Some(KeyAction::SelectAll));
    assert_eq!(k("cmd-c"), Some(KeyAction::Copy));
    assert_eq!(k("cmd-v"), Some(KeyAction::Paste));
    assert_eq!(k("escape"), Some(KeyAction::Escape));
    assert_eq!(k("space"), None, "text comes through the input handler, not the key table");
    assert_eq!(k("a"), None);
    assert_eq!(k("cmd-k"), None, "unknown chords are left to the keymap");
}

#[test]
fn enter_continues_a_list_tab_indents_the_item_and_a_fence_closes_itself() {
    let now = Instant::now();
    let mut d = edit::Document::from_text("- one\n- two");
    let blocks = md_blocks::split(d.lines());
    let mut c = edit::Caret { head: Pos { line: 1, ch: 5 }, anchor: Pos { line: 1, ch: 5 }, goal_col: None };
    assert_eq!(apply_action(&mut d, &mut c, &blocks, KeyAction::Newline, 20, now), Outcome::Edited);
    assert_eq!(d.lines()[2], "- ");
    apply_action(&mut d, &mut c, &blocks, KeyAction::Newline, 20, now);
    assert_eq!(d.lines()[2], "", "Enter on an empty item ends the list");
    c.head = Pos { line: 1, ch: 0 }; c.anchor = c.head;
    apply_action(&mut d, &mut c, &blocks, KeyAction::Tab, 20, now);
    assert_eq!(d.lines()[1], "  - two");
    apply_action(&mut d, &mut c, &blocks, KeyAction::ShiftTab, 20, now);
    assert_eq!(d.lines()[1], "- two");
    c.head = Pos { line: 0, ch: 5 }; c.anchor = c.head;
    apply_action(&mut d, &mut c, &blocks, KeyAction::Tab, 20, now);
    assert_eq!((d.lines()[0].as_str(), c.head.ch), ("  - one", 7), "Tab anywhere in an item indents the item and carries the caret");
    // A fence: three backticks and Enter open it and close it when nothing below closes it.
    let mut d = edit::Document::from_text("```rust\n\nlater");
    let blocks = md_blocks::split(edit::Document::from_text("\n\nlater").lines()); // the structure before the backticks were typed
    let mut c = edit::Caret { head: Pos { line: 0, ch: 7 }, anchor: Pos { line: 0, ch: 7 }, goal_col: None };
    apply_action(&mut d, &mut c, &blocks, KeyAction::Newline, 20, now);
    assert_eq!(d.lines(), &["```rust".to_string(), String::new(), "```".to_string(), String::new(), "later".to_string()][..]);
    assert_eq!(c.head, Pos { line: 1, ch: 0 });
    let mut d = edit::Document::from_text("```\ncode\n```");
    let blocks = md_blocks::split(d.lines());
    let mut c = edit::Caret { head: Pos { line: 0, ch: 3 }, anchor: Pos { line: 0, ch: 3 }, goal_col: None };
    apply_action(&mut d, &mut c, &blocks, KeyAction::Newline, 20, now);
    assert_eq!(d.lines().len(), 4, "a fence that is already closed gets a plain newline");
    assert_eq!(normalize_paste("a\r\nb\rc\n"), "a\nb\nc\n");
}

#[test]
fn utf16_offsets_are_relative_to_the_active_block() {
    let d = edit::Document::from_text("# T\n\na😀b\nc");
    let from = Pos { line: 2, ch: 0 };
    assert_eq!(d.pos_to_utf16(from, Pos { line: 2, ch: 2 }), 3, "the emoji is two UTF-16 units");
    assert_eq!(d.utf16_to_pos(from, 3), Pos { line: 2, ch: 2 });
    assert_eq!(d.utf16_to_pos(from, 5), Pos { line: 3, ch: 0 }, "the line break counts one unit");
    assert_eq!(d.utf16_to_pos(from, 99), Pos { line: 3, ch: 1 }, "clamped");
}
```

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- viewer:: edit::`

- [ ] **Step 3: Implement**

- `apply_action`: `Newline` consults `md_blocks::list_continuation` on the caret's line (an empty item removes the marker); the fence rule: when the caret's line is a fence opener (three or more backticks or tildes, optionally an info string, at the line start) and `blocks_before` has no block from that line on that is a `CodeBlock` closed by a matching fence (that is, the fence the user typed is new), insert `"\n\n<fence>"` and put the caret on the empty line between; else a plain newline. `Tab`/`ShiftTab` on a list item line add or remove two leading spaces (the caret shifts with them), elsewhere `Tab` inserts four spaces. `Copy`/`Cut` return the selected text; `Paste` returns `Outcome::Paste` and the caller inserts `normalize_paste(clipboard)` through `edit`; `Save` and `Escape` return their outcomes; `ScrollHome`/`ScrollEnd` return `Scroll`.
- `Viewer::key`: `key_action` then `apply_action` inside `edit` for edits (so autosave and events fire), movement outside it (`break_coalescing`, then `caret = ...`); for `Paste` reads `cx.read_from_clipboard()`; `Copy`/`Cut` write the clipboard; `Save` calls `self.save(cx)` (Cmd+S during a conflict resolves it as keep mine); `Escape` closes the find bar when open, else collapses the selection; returns `false` for `None` so the keymap and the pane see the key.
- `render_viewer`: for a viewer with a document, `if v.key(ks, cx) { cx.stop_propagation(); }` and the old scroll keys do not run at all (`space`, `home`, `end` included); the raw view keeps the caret's row visible after every move and edit (`scroll` adjusted in `edit` and after moves) and follows the caret horizontally with `scroll_x` (the raw view does not wrap).
- `EntityInputHandler for Viewer` as the interface says; `bounds_for_range` and `character_index_for_point` use the element's last bounds (`Viewer.last_bounds`, written in prepaint) and the painting helpers' geometry; the handler is registered from `ViewerElement::paint` with `window.handle_input(&self.focus, ElementInputHandler::new(bounds, viewer.clone()), cx)` while the viewer has a document.
- `viewer_element.rs`: free functions `display_col(line, ch) -> usize` (tabs to four columns), `x_for_col`, `col_for_x` (through `closest_index_for_x` on the shaped expanded line, bytes to chars), `paint_selection`, `paint_caret` (2 px bar, `text`), `paint_marked` (underline), `paint_hits`; raw colours from `raw_marks` at paint time for the visible lines only, gaps in `text` (`Marker`, `Quote`: `text_dim`; `Heading`: `text` bold; `Emphasis`: `text` italic; `Code`: `tree.modified`; `Link`: `focus_ring`). A mouse-down places the caret (`col_for_x`); a drag selects; Shift+click extends.
- keymap.rs: `ToggleMarkdownView` (`cmd-shift-m`, `Viewer`) and `SaveFile` (`cmd-s`, `Viewer`) rows after "toggle inline diff"; `binding()`, `command_for`, `route!`, SPEC_ROWS (`("cmd-shift-m", "toggle markdown view", "ToggleMarkdownView")`, `("cmd-s", "save file", "SaveFile")`), the two DEFAULT_CONFIG comment lines; the unbound counts do not change (both have keys). `run`: `ToggleMarkdownView => if let Some(t) = self.active_viewer_tab() { self.toggle_markdown_view_for(t, cx) }`, `SaveFile => if let Some(v) = self.active_viewer() { v.update(cx, |v, cx| v.save(cx)) }`.

- [ ] **Step 4: Run, check, commit**

```bash
git add crates/arkos/src/viewer.rs crates/arkos/src/viewer_element.rs crates/arkos/src/pane_view.rs crates/arkos/src/keymap.rs crates/arkos/src/command_palette.rs crates/arkos/src/config.rs crates/arkos/src/main.rs crates/arkos/src/edit.rs
git commit -m "Raw markdown is an editor: text input, the Mac editing keys, Cmd+S"
```

---

### Task 5: The rendered view (`md_view.rs`), inactive blocks

**Files:**
- Create: `crates/arkos/src/md_view.rs`
- Modify: `crates/arkos/src/viewer.rs` (`list_state: ListState`, `block_cache: Vec<Option<Rendered>>` spliced with `blocks`), `crates/arkos/src/highlight.rs` (`highlight_token(info, lines)` via `find_syntax_by_token`), `crates/arkos/src/pane_view.rs` (`render_viewer` renders `md_view::render` in Rendered mode, the pill, the chip's new place on markdown tabs), `crates/arkos/src/main.rs` (`mod md_view;`)

**Interfaces:**
- Produces: `md_view::render(viewer: &Entity<Viewer>, theme: &Theme, ui_family, mono_family, font_size, cx: &mut App) -> AnyElement`; `md_view::block_element(...)`; `md_view::scale(kind) -> f32` (`PARA = 1.1`, `HEADING: [f32; 6] = [1.9, 1.5, 1.25, 1.1, 1.0, 1.0]`, returned as `PARA * HEADING[level - 1]` so the test's products match exactly); `md_view::resolve_image(base_dir, path) -> Option<PathBuf>`; `pane_view::pill_label(mode) -> &'static str`; `pane_view::PILL_W`.

- [ ] **Step 1: Failing tests** (as in the first draft: `the_type_scale_follows_the_spec` written against `PARA * HEADING[..]`, `images_resolve_against_the_file_s_directory`, `the_pill_names_the_other_mode`)

- [ ] **Step 2: Run to see them fail**

- [ ] **Step 3: Implement**

- `Viewer.list_state = ListState::new(blocks.len(), ListAlignment::Top, px(600.0))` (no `measure_all`); `edit` mirrors `changed_blocks`' `(range, count)` onto `list_state.splice(range, count)` and `block_cache` (never `reset`, which jumps to the top), then `list_state.scroll_to_reveal_item(active)`. A reload splices everything, then `scroll_to(ListOffset { item_ix: block_at(blocks, old_top_line), offset_in_item: px(0.) })`.
- `render`: `list(state, move |ix, window, cx| ...)` reading the viewer through its `Entity` (`render_item` gets `&mut App`): the cached `Rendered { inlines, code_spans }` for block `ix` filled on demand; `block_element` builds the elements: `StyledText::new(text).with_runs(runs)` per paragraph, heading (with 8 px of space above) and quote; `InteractiveText::new(id, styled).on_click(link_ranges, move |ix, window, cx| if window.modifiers().platform { open link ix })` where a link opens with `cx.open_url(url)` for `http(s)` and `cx.open_with_system(&resolved)` for a path; the block div keeps `let layout = styled.layout().clone()` for Task 6's caret placement. Items: a marker column plus runs; a task checkbox (12 px, clickable) runs `viewer.update(cx, |v, cx| v.edit(|d, c, now| { toggle the line with toggle_task through replace_range; c }, cx))`. Code: `highlight_token` spans cached in `block_cache`, `tab_inactive` background, 8 px padding, mono. Quote bar, rule, table and html as plain mono blocks, `Metadata` dim mono. Image: `img(path).max_w_full().with_fallback(move || alt box)`. Blank: 12 px.
- The pill and the chip: `pill_label`, `PILL_W = 104.0`, the geometry from Global Constraints; the pill's `on_click` runs `toggle_markdown_view_for(tab)`; on markdown tabs the chip moves left of the pill at the pill's top.
- Rendered mode scrolling is the list's; the outer `on_scroll_wheel` is attached only in raw mode; page keys in rendered mode use `list_state.viewport_bounds().size.height` for the row count and `scroll_by`.

- [ ] **Step 4: Run, check, commit**

```bash
git add crates/arkos/src/md_view.rs crates/arkos/src/viewer.rs crates/arkos/src/highlight.rs crates/arkos/src/pane_view.rs crates/arkos/src/main.rs
git commit -m "The rendered markdown view: a virtualised list of blocks, and the mode pill"
```

---

### Task 6: The active block, caret placement and block navigation

**Files:**
- Modify: `crates/arkos/src/md_view.rs` (`RawBlockElement`, `navigate`, `append_paragraph`, the click mapping, the input canvas), `crates/arkos/src/viewer.rs` (`active_block()`, find in rendered mode), `crates/arkos/src/viewer_element.rs` (the shared painting helpers take a wrapped line too)

**Interfaces:**
- Produces: `md_view::navigate(doc: &Document, blocks: &[Block], caret: Caret, dir: Dir, select: bool) -> Caret` (Down on the active block's last non-blank line goes to `(next.lines.start, 0)`; Up on its first line to the end of the previous block's last non-blank line; anything else `move_by`); `md_view::append_paragraph(doc, now) -> Caret`; `RawBlockElement` (the active block's lines shaped with `window.text_system().shape_text(text, size, runs, Some(wrap_width), None)`, an explicit height of the wrapped rows times the line height, only the rows inside the content mask shaped, caret, selection, marked text and find hits through the shared helpers; a mouse-down placing the caret through the `WrappedLine`'s index mapping, a drag selecting); the input handler registered from a `canvas` over the list's bounds (always painted, so typing reaches the document when the active block is scrolled away) with the field as Task 4 defines it; `Viewer::active_block()`.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn block_navigation_crosses_blocks_and_clamps_at_the_ends() {
    let d = edit::Document::from_text("# H\n\npara one\npara two\n\n- item");
    let blocks = split(d.lines());
    let at = |l, c| edit::Caret { head: Pos { line: l, ch: c }, anchor: Pos { line: l, ch: c }, goal_col: None };
    assert_eq!(navigate(&d, &blocks, at(0, 1), Dir::Up, false).head, Pos { line: 0, ch: 1 }, "the first line of the first block stays");
    assert_eq!(navigate(&d, &blocks, at(5, 2), Dir::Down, false).head, Pos { line: 5, ch: 2 }, "the last line of the last block stays");
    assert_eq!(navigate(&d, &blocks, at(3, 4), Dir::Down, false).head, Pos { line: 5, ch: 0 }, "Down from the paragraph's last text line lands at the start of the list block");
    assert_eq!(navigate(&d, &blocks, at(5, 2), Dir::Up, false).head, Pos { line: 3, ch: 8 }, "Up from the list's first line lands at the end of the paragraph's last text line");
    assert_eq!(navigate(&d, &blocks, at(2, 4), Dir::Down, false).head, Pos { line: 3, ch: 4 }, "inside a block: a plain line move");
    let c = navigate(&d, &blocks, at(3, 4), Dir::Down, true);
    assert_eq!((c.head, c.anchor), (Pos { line: 5, ch: 0 }, Pos { line: 3, ch: 4 }), "Shift keeps the anchor across blocks");
}

#[test]
fn a_click_below_the_last_block_appends_a_paragraph() { /* as in the first draft */ }
```

- [ ] **Step 2: Run to see them fail**

- [ ] **Step 3: Implement**

- Clicks: in the block div's `on_mouse_down`, `let ix = layout.index_for_position(ev.position).unwrap_or_else(|e| e)` (a byte index into the rendered string), then `source_offset_at(runs, ix)`, `pos_of_offset(block, lines, offset)`, `caret = ...`, `break_coalescing`. A part with no runs places the caret at the block's first character. The list's tail is one more item of fixed height (`line_h * 2`) whose mouse-down runs `append_paragraph`.
- The active block renders as `RawBlockElement`; a selection spanning blocks renders every touched block raw; while the find bar is open the current hit's block renders raw too, `show_current_hit` calls `scroll_to_reveal_item`, `edit` re-runs `search(false)` while the bar is open, and closing the bar reveals the caret's block.
- Up and Down go through `navigate` in rendered mode; every edit and move calls `scroll_to_reveal_item(active_block())`.

- [ ] **Step 4: Run, check, commit**

```bash
git add crates/arkos/src/md_view.rs crates/arkos/src/viewer.rs crates/arkos/src/viewer_element.rs
git commit -m "The active block is raw, clicks place the caret, arrows cross blocks"
```

---

### Task 7: Banners, README, and the last attributes

**Files:**
- Modify: `crates/arkos/src/pane_view.rs` (the conflict, save-error and read-only banners in `render_viewer`), `README.md`, any file still carrying a "Used from Task" attribute

- [ ] **Step 1: Banners**

Above the body: `conflict` shows "The file changed on disk" with "Reload" and "Keep mine" (`resolve_conflict(false | true)`); `save_error` shows its text with "Retry" (`save`); `read_only` shows "not UTF-8: shown read-only" or "mixed line endings: shown read-only" (and the pill is not drawn for such a tab). Same style as the plain-file banner, the buttons in `text` with an underline on hover. The "line longer than 8 KB" plain banner is not shown for a document (the document is editable regardless).

- [ ] **Step 2: README**

The four Status rows from the first draft (the fourth adds "a file that is not UTF-8 opens read-only with a banner"); keybinding table rows for "toggle markdown view" (Cmd+Shift+M), "save file" (Cmd+S) and the editor's keys (arrows with Option, Cmd and Shift; Delete variants; Cmd+Z and Shift+Cmd+Z; Cmd+A, C, X, V; Tab and Shift+Tab in a list; Enter in a list; Escape); an "Editing markdown" section: the two modes, the active block, autosave and Cmd+S, line endings and the BOM kept, the conflict banner, what is not editable yet (tables render read-only, other file types), images render when their path resolves, raw colour is Arkos's own light markdown colouring rather than a TextMate grammar.

- [ ] **Step 3: Gates**

`grep -rn "Used from Task" crates/arkos/src` prints nothing; no em-dash in README.md or `crates/arkos/src/*.rs`; the warning check and the suite pass.

```bash
git add README.md crates/arkos/src/pane_view.rs
git commit -m "Markdown editing: the banners and the README"
```

---

## Deferred, for the user to rule on (not in this milestone)

- Inline conversion while typing within the active block (Typora style).
- Editable tables, image insertion, media (Tier B); live syntax colour inside code blocks while editing; footnotes (Tier C).
- A blinking caret; spell check; multiple carets.
- One block per list item instead of one block per list (section 14): a 3000-item list rebuilds every item on each frame it is visible.
- Editing non-markdown files (the code editor milestone).
- Scroll position kept when toggling between Rendered and Raw; double- and triple-click selection; copying text from an inactive block with the mouse; Up and Down by visual row inside a wrapped paragraph; bare URLs as links; reference-style links (each block is parsed alone); Ctrl+A, Ctrl+E, Ctrl+K; a reopened tab's mode (`Closed::Viewer` holds only the path).
- Spec deviations taken in the plan, for the user to ratify: raw colouring is Arkos's own per-line colourer rather than syntect's Markdown grammar (syntect over a 5 MB file would take seconds and its stateful grammar cannot colour one changed line); the pill uses the pane's monospace family like the jump pill rather than the UI family.

## Self-review notes

- Spec section 3 (files and modes): Tasks 3, 5. Section 4 (core): Task 1. Section 5 (blocks and view): Tasks 2, 5, 6. Section 6 (keys and input): Task 4. Section 7 (saving): Task 3. Section 8 (external changes): Tasks 3, 7. Section 9 (the pill): Task 5. Section 10 (find): Tasks 4, 6. Section 11 (performance): the benchmark in Task 2, the virtualised list in Task 5, the deferred `make_mut`. Section 12 (tests): as named. Section 13 (README): Task 7.
- The editing core never imports markdown; the markdown helpers never touch gpui; the view never parses markdown itself. That is the seam the later code editor relies on.
