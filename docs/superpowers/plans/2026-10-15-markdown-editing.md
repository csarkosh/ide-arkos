# Markdown Editing (milestone 7) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Markdown files open as an editable rendered document: the block under the caret shows its raw markdown, every other block renders, edits autosave, a pill switches the tab between rendered and raw, and the whole thing stays fast on any document size.

**Architecture:** Three new pure modules under the viewer: `edit.rs` (a line buffer with caret, selection, undo, movement, line-ending round trip; no markdown), `md_blocks.rs` (top-level blocks from `pulldown-cmark` with source ranges, inline runs with source offsets, list and task helpers, a per-line markdown colourer for raw text), and `md_view.rs` (the rendered view as one gpui `list()` of blocks, the active block as a raw editing element). `Viewer` gains a document, a mode, dirty state and saving; the sidebar watcher learns to skip the viewer's own writes; `Tab::Viewer` carries the mode. Text input goes through gpui's `EntityInputHandler` so IME and dead keys work.

**Tech Stack:** Rust 2021, gpui 0.2.2 (`list`, `ListState`, `StyledText`, `TextRun`, `InteractiveText`, `img`, `EntityInputHandler`, `ElementInputHandler`, `canvas`, custom `Element`), `pulldown-cmark` 0.13 (new dependency: options `ENABLE_TABLES | ENABLE_TASKLISTS | ENABLE_STRIKETHROUGH`), `unicode-segmentation` (already a dependency), syntect (already) for code blocks only.

**Spec:** `docs/superpowers/specs/2026-10-15-markdown-editing-design.md` (approved 2026-10-15; its section 1 rulings are binding: autosave plus Cmd+S, Tier A plus task checkboxes with tables read-only, no size cap, the caret's block raw and the rest rendered, nothing but markdown becomes editable; section 9 as amended: the toggle is a pill inside the pane like "jump to bottom"). Section 14 defaults taken: a whole list is one block; `.mdx` counts as markdown.

## Global Constraints

- Machine rules (the user's MacBook Air locked up from build load): every cargo call is `nice -n 15 ~/.cargo/bin/cargo ...`; the worktree's local `.cargo/config.toml` (`jobs = 3`, shared `target-dir`) stays; no `--release`; never two cargo processes at once; focused filters while iterating, the workspace suite once per task; the warning check is `cargo check --workspace --all-targets` and must print no warning from our crates. The benchmark test is `#[ignore]` and run by hand (`-- --ignored md_blocks::tests::a_megabyte_splits_in_time`), never in the suite, because timing under load is not a gate.
- Dead code between tasks: an item a later task first uses gets `#[cfg_attr(not(test), allow(dead_code))] // Used from Task N on.` (the crate's convention, theme.rs:159, palette.rs:72); the task that first uses it removes the attribute; after the last task `grep -rn "Used from Task" crates/arkos/src` prints nothing.
- Markdown tabs: a viewer whose path extension is `md`, `markdown` or `mdx` (case-insensitive). Every other viewer is unchanged and read-only.
- Texts (exact): the dirty tab label is the title plus " •"; the conflict banner "The file changed on disk" with buttons "Reload" and "Keep mine"; the save-error banner "cannot save {path}: {error}" with "Retry"; the pill "show raw" while rendered and "show rendered" while raw; keymap actions "toggle markdown view" (`cmd-shift-m`, context `Viewer`) and "save file" (`cmd-s`, context `Viewer`); palette labels "Toggle markdown view" and "Save file".
- Timings: autosave 1 s after the last edit; undo coalescing breaks after 700 ms, a newline, or a caret move.
- Pill geometry: the terminal's "jump to bottom" pill (pane_view.rs ~885): `absolute`, `top(px(10.0))`, `right(px(14.0))`, `px(10) py(4)`, `rounded(10)`, `banner` background, `text` colour, the UI family at 12 px, `cursor_pointer`, a left mouse-down that focuses the pane and stops propagation; while the find bar is open the pill sits at `top(px(44.0))` (where the inline-diff chip goes then).
- Rendered type: paragraphs in the UI font (`.SystemUIFont`, `chrome::UI_FONT` made `pub`) at `font_size * 1.1`, line height 1.5; headings at 1.9, 1.5, 1.25, 1.1, 1.0, 1.0 times that, semibold; code and the active block in the terminal family at `font_size`; inline code background `tab_inactive`; block quote bar 3 px `border`; rule 1 px `border`; links `focus_ring` colour underlined; the caret 2 px `text`; the selection `find_match` at 0.5 alpha.
- Line endings: the file's `\n` or `\r\n` and whether it ended with a newline are kept and restored on save; in memory lines have no terminators (as `viewer::split_lines` gives them), but the editor's buffer keeps tabs as tabs (it does not use `split_lines`, which turns tabs into spaces).
- Saving writes a temp file beside the target, copies the target's permissions, and renames; a symlink target is written in place through the link. The viewer records `config_watch::fingerprint(path)` right after a write.
- No em-dashes in any file. Comments are full sentences. Headless tests only: nothing opens a window, spawns `open`, or touches files outside a temp dir.

## Review Focus

1. A paste of 10 000 lines into a 20 000-line document: one undo step, the active block recomputed once, the frame under budget (Task 1 `a_paste_is_one_undo_step`, Task 3 `an_edit_updates_the_blocks_once`).
2. A file with `\r\n` endings and no trailing newline, edited and saved: byte-identical except the edit (Task 1 `line_endings_round_trip`, Task 3 `a_save_restores_the_file_s_line_endings`).
3. The user types in a block while the sidebar watcher's one-second pass sees the viewer's own write land: no reload, no caret jump (Task 3 `the_viewer_s_own_write_is_not_an_external_change`).
4. An external edit while the buffer is dirty, then Cmd+S: "Keep mine" semantics, the file is overwritten, the banner goes (Task 3 `cmd_s_during_a_conflict_keeps_mine`).
5. Arrow Down on the last line of the last block, Arrow Up on the first line of the first block, and a click below the last block: no panic, the caret clamps, a click below appends a paragraph (Task 6 `block_navigation_clamps_at_the_ends`, `a_click_below_the_last_block_appends_a_paragraph`).

## File structure

- Create `crates/arkos/src/edit.rs`: `Pos`, `Caret`, `LineEnding`, `Document`, `Edit`, `Undo`; movement and edit primitives. Pure.
- Create `crates/arkos/src/md_blocks.rs`: `Block`, `BlockKind`, `split`, `block_at`, `Inline`, `inline_runs`, `list_continuation`, `toggle_task`, `image_only`, `raw_marks`. Pure; depends on `pulldown-cmark`.
- Create `crates/arkos/src/md_view.rs`: `render` (the list), `render_block`, `RawBlockElement`, click-to-caret mapping, block navigation. gpui.
- Modify `crates/arkos/src/viewer.rs`: `ViewMode`, `is_markdown`, `Viewer.doc`, `mode`, `dirty`, `conflict`, `save_seq`, `list_state`; `apply` builds the document; `save`, `on_external_change`, `edit`, `undo`, `redo`; the `Root` methods for autosave and the keys.
- Modify `crates/arkos/src/tree.rs`: `Tab::Viewer { path, #[serde(default)] mode: ViewMode }` and every constructor.
- Modify `crates/arkos/src/pane_view.rs`: `render_viewer` picks raw or rendered, draws the pill, routes keys and input; `tab_labels` adds " •".
- Modify `crates/arkos/src/keymap.rs`, `command_palette.rs`, `config.rs` (DEFAULT_CONFIG), `main.rs` (`run` arms, `flush_for_quit`, `close_tab`), `chrome.rs` (`pub const UI_FONT`).
- Modify `Cargo.toml` (workspace dependency `pulldown-cmark = { version = "0.13", default-features = false }`) and `crates/arkos/Cargo.toml`.
- Modify `README.md`: Status rows, keybinding table, an "Editing markdown" section.

---

### Task 1: The editing core (`edit.rs`)

**Files:**
- Create: `crates/arkos/src/edit.rs`
- Modify: `crates/arkos/src/main.rs` (`mod edit;`)

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

```bash
git add crates/arkos/src/edit.rs crates/arkos/src/main.rs
git commit -m "An editing core: a line buffer with caret, selection, undo and movement"
```

---

### Task 2: Markdown blocks (`md_blocks.rs`)

**Files:**
- Modify: `Cargo.toml` (workspace dependency), `crates/arkos/Cargo.toml` (`pulldown-cmark.workspace = true`)
- Create: `crates/arkos/src/md_blocks.rs`
- Modify: `crates/arkos/src/main.rs` (`mod md_blocks;`)

**Interfaces:**
- Produces (attribute "Used from Task 3 on." on the module's items until used):
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq)] pub enum BlockKind { Paragraph, Heading(u8), List, CodeBlock { info: String }, Quote, Rule, Table, Html, Blank }
  #[derive(Debug, Clone, PartialEq, Eq)] pub struct Block { pub lines: Range<usize>, pub kind: BlockKind } // lines are 0-based indices into the document; blank lines after a block belong to it
  pub fn split(lines: &[String]) -> Vec<Block>; // every line in exactly one block, in order, concatenating back to the document
  pub fn block_at(blocks: &[Block], line: usize) -> usize;
  #[derive(Debug, Clone, PartialEq, Eq, Default)] pub struct Style { pub bold: bool, pub italic: bool, pub code: bool, pub strike: bool, pub link: Option<String> }
  #[derive(Debug, Clone, PartialEq, Eq)] pub struct Run { pub text: String, pub style: Style, pub source: Range<usize> } // source: byte range in the block's text
  #[derive(Debug, Clone, PartialEq, Eq)] pub enum Inline { Para(Vec<Run>), Heading(u8, Vec<Run>), Item { depth: u8, marker: String, task: Option<bool>, runs: Vec<Run> }, Code { info: String, text: String }, Quote(Vec<Run>), Rule, Table(String), Html(String), Image { alt: String, path: String } }
  pub fn inline(block_text: &str, kind: &BlockKind) -> Vec<Inline>; // a list yields one Item per list item, in order; a paragraph holding only an image yields Image
  pub fn list_continuation(line: &str) -> Option<(String, bool)>; // (the marker to insert on Enter, the item was empty): "- " / "* " / "1. " incremented / "- [ ] "
  pub fn toggle_task(line: &str) -> Option<String>; // "[ ]" <-> "[x]" on a task item line
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Mark { Marker, Heading, Emphasis, Code, Link, Quote }
  pub fn raw_marks(line: &str) -> Vec<(Range<usize>, Mark)>; // a light per-line colourer for raw markdown; byte ranges
  ```

- [ ] **Step 1: Failing tests**

```rust
fn lines(s: &str) -> Vec<String> { s.split('\n').map(String::from).collect() }

#[test]
fn every_line_lands_in_exactly_one_block_in_order() {
    let doc = lines("# Title\n\nA paragraph\nover two lines.\n\n- one\n- two\n  - nested\n\n```rust\nfn x() {}\n```\n\n> quoted\n\n---\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n<div>raw</div>\n");
    let blocks = split(&doc);
    let kinds: Vec<&BlockKind> = blocks.iter().map(|b| &b.kind).collect();
    assert_eq!(kinds, vec![&BlockKind::Heading(1), &BlockKind::Paragraph, &BlockKind::List, &BlockKind::CodeBlock { info: "rust".into() }, &BlockKind::Quote, &BlockKind::Rule, &BlockKind::Table, &BlockKind::Html, &BlockKind::Blank]);
    let mut covered = 0;
    for b in &blocks {
        assert_eq!(b.lines.start, covered, "blocks are contiguous");
        covered = b.lines.end;
    }
    assert_eq!(covered, doc.len(), "and cover the document; the final empty line is a Blank block");
    assert_eq!(blocks[0].lines, 0..2, "the blank line after the heading belongs to it");
    assert_eq!(block_at(&blocks, 7), 2, "the nested item is in the list block");
    assert_eq!(block_at(&blocks, 999), blocks.len() - 1, "past the end: the last block");
    assert_eq!(split(&[]).len(), 0);
    assert_eq!(split(&lines("")), vec![Block { lines: 0..1, kind: BlockKind::Blank }]);
}

#[test]
fn inline_runs_carry_styles_and_source_offsets() {
    let runs = match &inline("a **bold** and `code` [link](https://x.y) ~~gone~~", &BlockKind::Paragraph)[0] { Inline::Para(r) => r.clone(), other => panic!("{other:?}") };
    let texts: Vec<(&str, bool, bool, bool, Option<&str>)> = runs.iter().map(|r| (r.text.as_str(), r.style.bold, r.style.code, r.style.strike, r.style.link.as_deref())).collect();
    assert_eq!(texts, vec![("a ", false, false, false, None), ("bold", true, false, false, None), (" and ", false, false, false, None), ("code", false, true, false, None), (" ", false, false, false, None), ("link", false, false, false, Some("https://x.y")), (" ", false, false, false, None), ("gone", false, false, true, None)]);
    assert_eq!(runs[1].source, 4..8, "the source range is the text inside the markers");
    assert_eq!(runs[5].source, 23..27);
}

#[test]
fn lists_yield_items_with_depth_markers_and_tasks() {
    let items = inline("1. first\n2. second\n   - [x] done\n   - [ ] todo", &BlockKind::List);
    let got: Vec<(u8, &str, Option<bool>, String)> = items.iter().map(|i| match i { Inline::Item { depth, marker, task, runs } => (*depth, marker.as_str(), *task, runs.iter().map(|r| r.text.clone()).collect()), other => panic!("{other:?}") }).collect();
    assert_eq!(got, vec![(0, "1.", None, "first".into()), (0, "2.", None, "second".into()), (1, "-", Some(true), "done".into()), (1, "-", Some(false), "todo".into())]);
}

#[test]
fn code_images_tables_and_html_pass_through() {
    assert_eq!(inline("```py\nprint(1)\n```", &BlockKind::CodeBlock { info: "py".into() }), vec![Inline::Code { info: "py".into(), text: "print(1)\n".into() }]);
    assert_eq!(inline("![alt text](img/a.png)", &BlockKind::Paragraph), vec![Inline::Image { alt: "alt text".into(), path: "img/a.png".into() }]);
    assert!(matches!(&inline("see ![a](b.png) here", &BlockKind::Paragraph)[0], Inline::Para(_)), "an image inside text is text");
    assert_eq!(inline("| a |\n|---|", &BlockKind::Table), vec![Inline::Table("| a |\n|---|".into())]);
    assert_eq!(inline("<b>x</b>", &BlockKind::Html), vec![Inline::Html("<b>x</b>".into())]);
}

#[test]
fn enter_continues_a_list_and_ends_an_empty_item() {
    assert_eq!(list_continuation("- item"), Some(("- ".into(), false)));
    assert_eq!(list_continuation("  * item"), Some(("  * ".into(), false)));
    assert_eq!(list_continuation("3. third"), Some(("4. ".into(), false)));
    assert_eq!(list_continuation("- [x] done"), Some(("- [ ] ".into(), false)));
    assert_eq!(list_continuation("- "), Some(("- ".into(), true)), "an empty item: Enter ends the list");
    assert_eq!(list_continuation("plain"), None);
    assert_eq!(toggle_task("- [ ] todo"), Some("- [x] todo".into()));
    assert_eq!(toggle_task("  1. [x] done"), Some("  1. [ ] done".into()));
    assert_eq!(toggle_task("- plain"), None);
}

#[test]
fn raw_marks_colour_the_syntax_a_line_shows() {
    assert_eq!(raw_marks("## Head"), vec![(0..2, Mark::Marker), (3..7, Mark::Heading)]);
    assert_eq!(raw_marks("- [ ] a **b** `c`"), vec![(0..5, Mark::Marker), (8..13, Mark::Emphasis), (14..17, Mark::Code)]);
    assert_eq!(raw_marks("> q"), vec![(0..1, Mark::Quote)]);
    assert_eq!(raw_marks("see [x](y)"), vec![(4..10, Mark::Link)]);
    assert_eq!(raw_marks("```rust"), vec![(0..7, Mark::Marker)]);
    assert!(raw_marks("plain words").is_empty());
}

#[test]
#[ignore] // Timing: run by hand with `-- --ignored`; the gate is 4 ms on the MacBook Air.
fn a_megabyte_splits_in_time() {
    let unit = "# Heading\n\nSome paragraph text that goes on for a while to look like prose.\n\n- a list item\n- another with **bold**\n\n```rust\nfn x() {}\n```\n\n";
    let mut text = String::new();
    while text.len() < 1 << 20 { text.push_str(unit); }
    let doc = lines(&text);
    let t = std::time::Instant::now();
    let blocks = split(&doc);
    let took = t.elapsed();
    assert!(blocks.len() > 1000);
    assert!(took < Duration::from_millis(4), "split took {took:?}");
}
```

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- md_blocks::` (and once the code exists, the benchmark by hand: `-- --ignored md_blocks::tests::a_megabyte_splits_in_time`).

- [ ] **Step 3: Implement**

`split` joins the lines with `\n` once, runs `pulldown_cmark::Parser::new_ext(text, options).into_offset_iter()`, and takes every `Event::Start` at nesting depth 0 with its byte range; byte offsets map to line indices through a prefix table of line starts. A block's `lines.end` extends over the blank lines that follow it (the next block starts on its first non-blank line); a document ending in blank lines gets a trailing `Blank` block, and an empty document is one `Blank` block over its one empty line. `BlockKind` from the tag: `Tag::Paragraph`, `Tag::Heading { level }`, `Tag::List(_)`, `Tag::CodeBlock(kind)` (the fence's info string, trimmed; indented code blocks have an empty info), `Tag::BlockQuote(_)`, `Event::Rule`, `Tag::Table(_)`, `Event::Html`/`Tag::HtmlBlock`.

`inline` parses the block's own text with the same options and `into_offset_iter`, folding `Start(Strong)`, `Emphasis`, `Strikethrough`, `Code`, `Link` into a `Style` stack and emitting a `Run` per `Text`/`Code` event with its source range (the range `pulldown-cmark` gives for the event). For a list, each `Tag::Item` becomes one `Item` with its depth (list nesting), its marker read from the source line (`-`, `*`, `+`, or the number with its delimiter) and `task` from `TaskListMarker`. A paragraph whose only content is one `Tag::Image` gives `Image`.

`list_continuation` and `toggle_task` are small hand parsers over the line's leading whitespace, marker and optional task box; `raw_marks` is a hand scanner too (heading markers, list and task markers, fences, quote markers, `**`/`*`/`_` pairs, backtick pairs, `[text](url)`), never pulldown-cmark, so it costs one pass over one line.

- [ ] **Step 4: Run, check, commit**

Run the focused tests (6 passed, 1 ignored), the benchmark by hand once (record the time in the commit message), the warning check, the suite.

```bash
git add Cargo.toml Cargo.lock crates/arkos/Cargo.toml crates/arkos/src/md_blocks.rs crates/arkos/src/main.rs
git commit -m "Markdown blocks: top-level blocks with source ranges, inline runs, list helpers"
```

---

### Task 3: The document in the viewer, the mode, saving and external changes

**Files:**
- Modify: `crates/arkos/src/viewer.rs` (`Viewer`, `apply`, the `Root` impl near the end), `crates/arkos/src/tree.rs` (`Tab::Viewer`), `crates/arkos/src/chrome.rs` (`pub const UI_FONT`), the `Tab::Viewer { path: .. }` constructors (sidebar.rs:1058, 1062; tests in main.rs:3936, pane_view.rs:1309, sidebar.rs:1534, 1610, sessions.rs:695, 765), `crates/arkos/src/main.rs` (`close_tab` saves, `flush_for_quit` saves, the autosave timer), `crates/arkos/src/pane_view.rs` (`tab_labels` adds " •"), `crates/arkos/src/config_watch.rs` (none; `fingerprint` is reused)

**Interfaces:**
- Consumes: Task 1's `Document`, Task 2's `split`/`block_at`.
- Produces:
  ```rust
  // tree.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)] pub enum ViewMode { #[default] Rendered, Raw }
  Tab::Viewer { path: PathBuf, #[serde(default)] mode: ViewMode }
  // viewer.rs
  pub fn is_markdown(path: &Path) -> bool;
  pub struct Viewer { ..., pub doc: Option<edit::Document>, pub caret: edit::Caret, pub blocks: Vec<md_blocks::Block>, pub mode: ViewMode, pub dirty: bool, pub conflict: bool, pub save_error: Option<String>, pub save_seq: u64, pub saved_seq: u64, pub own_fingerprint: Option<(u128, u64)> }
  impl Viewer {
      pub fn edit(&mut self, f: impl FnOnce(&mut edit::Document, edit::Caret, Instant) -> edit::Caret, cx) // runs an edit, refreshes `content.lines` and `blocks`, sets dirty, bumps save_seq, notifies
      pub fn undo(&mut self, cx); pub fn redo(&mut self, cx);
      pub fn set_mode(&mut self, mode: ViewMode, cx);
      pub fn text_for_save(&self) -> Option<(String, LineEnding)>;
      pub fn on_external_change(&mut self) -> ExternalChange; // Reload when clean, Ask when dirty
  }
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum ExternalChange { Reload, Ask }
  pub fn save_text(path: &Path, text: &str) -> std::io::Result<()>; // temp + permissions + rename; in place through a symlink
  impl Root { pub fn save_viewer(&mut self, tab: TabId, cx); fn schedule_autosave(&mut self, tab: TabId, cx); pub fn save_dirty_viewers_now(&mut self); pub fn resolve_conflict(&mut self, tab: TabId, keep_mine: bool, cx); }
  ```
  `content.lines` stays the `Arc<Vec<String>>` the find and the diff read; after an edit it is rebuilt from the document (`Arc::new(doc.lines().to_vec())`; the plan accepts this copy, measured in Task 3's benchmark note, and Task 5 or the final review may replace it with `Arc::make_mut` if it shows). `spans` for a markdown document come from `md_blocks::raw_marks` per line mapped to the theme's colours, not syntect.

- [ ] **Step 1: Failing tests** (viewer.rs and tree.rs test modules)

```rust
#[test]
fn markdown_is_known_by_extension() {
    for p in ["a.md", "b.MD", "c.markdown", "d.mdx"] { assert!(is_markdown(Path::new(p)), "{p}"); }
    for p in ["a.rs", "README", "x.md.bak", "y.txt"] { assert!(!is_markdown(Path::new(p)), "{p}"); }
}

#[test]
fn a_save_restores_the_file_s_line_endings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("n.md");
    std::fs::write(&path, "# T\r\n\r\ntext").unwrap();
    let mut d = edit::Document::from_text(&std::fs::read_to_string(&path).unwrap());
    d.insert(edit::Caret::default(), "X", std::time::Instant::now());
    save_text(&path, &d.to_text()).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"X# T\r\n\r\ntext");
}

#[test]
fn a_save_is_atomic_keeps_permissions_and_follows_a_symlink() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real.md");
    std::fs::write(&real, "old").unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = dir.path().join("link.md");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    save_text(&link, "new").unwrap();
    assert_eq!(std::fs::read_to_string(&real).unwrap(), "new", "written through the link");
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link is still a link");
    save_text(&real, "newer").unwrap();
    assert_eq!(std::fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o600, "permissions copied onto the renamed file");
    assert!(std::fs::read_dir(dir.path()).unwrap().count() == 2, "no temp file left behind");
    let err = save_text(&dir.path().join("missing").join("x.md"), "x").unwrap_err();
    assert!(!err.to_string().is_empty());
}

#[test]
fn the_viewer_s_own_write_is_not_an_external_change_and_a_dirty_buffer_asks() {
    // Pure state: after a save the recorded fingerprint equals the file's, so the watcher's
    // reload (which passes `known`) returns None from read_file.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.md");
    std::fs::write(&path, "one\n").unwrap();
    let fp = crate::config_watch::fingerprint(&path);
    assert!(read_file(&path, fp, None, 1 << 20, false).is_none(), "nothing changed: no reload");
    save_text(&path, "two\n").unwrap();
    let after = crate::config_watch::fingerprint(&path);
    assert!(read_file(&path, after, None, 1 << 20, false).is_none(), "the write we recorded is not a change");
    assert!(read_file(&path, fp, None, 1 << 20, false).is_some(), "a stale fingerprint is");
    assert_eq!(external_change(false), ExternalChange::Reload);
    assert_eq!(external_change(true), ExternalChange::Ask);
}

// tree.rs
#[test]
fn a_viewer_tab_s_mode_round_trips_and_an_old_file_renders() {
    let mut t = Tree::new(Tab::Viewer { path: "/p/a.md".into(), mode: ViewMode::Raw });
    let json = serde_json::to_string(&t).unwrap();
    let back: Tree = serde_json::from_str(&json).unwrap();
    assert_eq!(back.pane(back.panes()[0]).unwrap().tabs[0], Tab::Viewer { path: "/p/a.md".into(), mode: ViewMode::Raw });
    let old: Tree = serde_json::from_str(&json.replace(",\"mode\":\"Raw\"", "")).unwrap();
    assert_eq!(old.pane(old.panes()[0]).unwrap().tabs[0], Tab::Viewer { path: "/p/a.md".into(), mode: ViewMode::Rendered });
    t.set_viewer_mode(t.panes()[0], 0, ViewMode::Rendered);
    assert!(matches!(t.pane(t.panes()[0]).unwrap().tabs[0], Tab::Viewer { mode: ViewMode::Rendered, .. }));
}
```

(`external_change(dirty: bool) -> ExternalChange` is the pure rule `Viewer::on_external_change` uses; `Tree::set_viewer_mode(pane, idx, mode)` is a small setter.)

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- viewer:: tree::`

- [ ] **Step 3: Implement**

- `apply`: for a markdown path with `Content::Text`, build `doc = Document::from_text(&text)` (read_file must hand the text through for markdown: add `raw: Option<String>` to `Loaded`, filled only for markdown files, so the tab-to-spaces conversion in `split_lines` does not reach the editor), `blocks = split(doc.lines())`, `caret` clamped, `dirty = false`, `conflict = false`. A reload while `dirty` does not reach `apply`: `Root::load` checks `on_external_change()` first; `Ask` sets `conflict = true` and skips the load; `Reload` proceeds. The caret keeps its line (clamped) across a reload; undo history is cleared when the reloaded text differs from the buffer.
- `edit`: calls the closure with `now = Instant::now()`, then `content = Content::Text { lines: Arc::new(doc.lines().to_vec()), spans: raw_spans(doc.lines(), colours) }`, `blocks = split(...)`, `dirty = true`, `save_seq += 1`, `cx.notify()`. `Root` wraps it in `edit_viewer(tab, f, cx)` which also calls `schedule_autosave`.
- `schedule_autosave`: `cx.spawn` a 1 s timer carrying `save_seq`; when it fires, `save_viewer` only if the viewer's `save_seq` still equals the carried one and `conflict` is false. `save_viewer`: `text_for_save` on the UI thread, the write on the background executor, then on the UI thread `own_fingerprint = fingerprint(path)`, `viewer.fingerprint = own_fingerprint`, `saved_seq = seq`, `dirty = save_seq != saved_seq`, `save_error = None`; the diff is recomputed as `rediff_viewers` does for one viewer. On `Err(e)`: `save_error = Some(format!("cannot save {}: {e}", path.display()))`, the tab stays dirty.
- `close_tab` (main.rs ~1282, before `self.viewers.remove(&id)`): a dirty markdown viewer is saved synchronously on the UI thread (one small file write) so the tab's edits are not lost; `flush_for_quit` calls `save_dirty_viewers_now` the same way.
- `resolve_conflict(tab, keep_mine)`: `keep_mine` clears `conflict` and saves at once; else clears `conflict`, `dirty`, and reloads. Cmd+S during a conflict means keep mine (Task 4 wires the key).
- `tab_labels` (pane_view.rs ~360): a `Tab::Viewer` whose viewer is dirty gets `" •"` appended to its label.
- `save_text`: read the target's metadata; if it is a symlink (`symlink_metadata`), `std::fs::write` through it; else write `<file>.<pid>.tmp` in the parent, `set_permissions` from the target when it exists, `rename`; on any error remove the temp file and return the error.
- `tree.rs`: the enum change, `set_viewer_mode`, and every constructor gets `mode: ViewMode::default()` (or `Rendered`).

- [ ] **Step 4: Run, check, commit**

Run the focused tests, the warning check (Task 1's and 2's attributes come off where used; what Task 4 first uses keeps "Used from Task 4 on."), the suite.

```bash
git add Cargo.lock crates/arkos/src/viewer.rs crates/arkos/src/tree.rs crates/arkos/src/chrome.rs crates/arkos/src/sidebar.rs crates/arkos/src/main.rs crates/arkos/src/pane_view.rs crates/arkos/src/sessions.rs
git commit -m "A markdown viewer holds an editable document, a mode, and saves it"
```

---

### Task 4: Text input and the editing keys (raw mode becomes an editor)

**Files:**
- Modify: `crates/arkos/src/viewer.rs` (`impl EntityInputHandler for Viewer`, `key(&mut self, ks, now, cx) -> bool`), `crates/arkos/src/viewer_element.rs` (caret and selection painting, `raw_marks` colours, click to caret), `crates/arkos/src/pane_view.rs` (`render_viewer`: the key handler, `window.handle_input`, the raw body), `crates/arkos/src/keymap.rs` (two actions), `crates/arkos/src/command_palette.rs`, `crates/arkos/src/config.rs` (DEFAULT_CONFIG), `crates/arkos/src/main.rs` (`run` arms)

**Interfaces:**
- Produces: `viewer::key_action(ks: &Keystroke) -> Option<KeyAction>` (pure: the keystroke to editor action table, `KeyAction::{Move(Move, select), DeleteBack(Unit), DeleteFwd(Unit), Newline, Tab, ShiftTab, Undo, Redo, SelectAll, Copy, Cut, Paste, Save, Escape, Scroll(...)}`), `Viewer::key(&mut self, ks, cx) -> bool` applying it (list continuation on Enter via `md_blocks::list_continuation`, Tab as two spaces in a list item else four), the `EntityInputHandler` impl (the "field" is the whole document in Raw mode; UTF-16 offsets map through `edit::Document::utf16_to_pos` and back, added in this task), keymap `ToggleMarkdownView`, `SaveFile`.

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
    assert_eq!(k("a"), None, "plain text comes through the input handler, not the key table");
    assert_eq!(k("cmd-k"), None, "unknown chords are left to the keymap");
}

#[test]
fn enter_in_a_list_continues_it_and_tab_indents_the_item() {
    let mut v = test_viewer("- one\n- two");
    v.caret = edit::Caret { head: Pos { line: 1, ch: 5 }, anchor: Pos { line: 1, ch: 5 }, goal_col: None };
    v.apply_action(KeyAction::Newline, Instant::now());
    assert_eq!(v.doc.as_ref().unwrap().lines()[2], "- ");
    v.apply_action(KeyAction::Newline, Instant::now());
    assert_eq!(v.doc.as_ref().unwrap().lines()[2], "", "Enter on an empty item ends the list");
    v.caret.head = Pos { line: 1, ch: 0 }; v.caret.anchor = v.caret.head;
    v.apply_action(KeyAction::Tab, Instant::now());
    assert_eq!(v.doc.as_ref().unwrap().lines()[1], "  - two");
    v.apply_action(KeyAction::ShiftTab, Instant::now());
    assert_eq!(v.doc.as_ref().unwrap().lines()[1], "- two");
    v.caret.head = Pos { line: 0, ch: 5 }; v.caret.anchor = v.caret.head;
    v.apply_action(KeyAction::Tab, Instant::now());
    assert_eq!(v.doc.as_ref().unwrap().lines()[0], "  - one", "Tab anywhere in an item indents the item");
    assert!(v.dirty);
}

#[test]
fn utf16_offsets_round_trip_through_positions() {
    let d = edit::Document::from_text("a😀b\nc");
    assert_eq!(d.pos_to_utf16(Pos { line: 0, ch: 2 }), 3, "the emoji is two UTF-16 units");
    assert_eq!(d.utf16_to_pos(3), Pos { line: 0, ch: 2 });
    assert_eq!(d.utf16_to_pos(5), Pos { line: 1, ch: 0 }, "the line break counts one unit");
    assert_eq!(d.utf16_to_pos(99), Pos { line: 1, ch: 1 }, "clamped");
}
```

(`test_viewer(text)` builds a `Viewer` without a `Context`: give `Viewer` a `pub(crate) fn for_test(text: &str) -> Viewer` under `#[cfg(test)]` that fills the document and the focus with a dummy; if `FocusHandle` cannot be built without an App, make `focus` an `Option<FocusHandle>` for tests or keep `apply_action` as a free function over `(&mut Document, &mut Caret, &mut Vec<Block>)` and test that instead. Prefer the free function `apply_action(doc, caret, action, now) -> Outcome` so no gpui is needed in the test.)

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- viewer:: edit::`

- [ ] **Step 3: Implement**

- `key_action` as the table above; `apply_action` applies to the document and caret: `Newline` consults `md_blocks::list_continuation` on the caret's line (an empty item removes the marker instead of inserting a new one); `Tab`/`ShiftTab` on a list item line add or remove two leading spaces of that line (the caret shifts with them), elsewhere `Tab` inserts four spaces; `Copy`/`Cut`/`Paste` go through `cx.write_to_clipboard` / `cx.read_from_clipboard` in `Viewer::key` (the `Outcome` tells the caller to do the clipboard step); `Save` calls back into Root (`Outcome::Save`); `Escape` closes the find bar when open, else collapses the selection.
- `EntityInputHandler for Viewer`: `text_for_range` and `selected_text_range` over the document in UTF-16 (the whole document is the field in this task; Task 6 narrows it to the active block in rendered mode), `replace_text_in_range` runs `edit` with `replace_range`, `replace_and_mark_text_in_range` stores the marked range in `Viewer.marked: Option<Range<Pos>>` and draws it underlined, `unmark_text` clears it, `bounds_for_range` returns the caret line's rectangle from the element's last-known bounds (`Viewer.last_bounds` written in prepaint, like `rows_visible`).
- `pane_view::render_viewer`: for a markdown tab the key handler calls `viewer.key(ks, cx)` first and only falls back to the old scroll keys when it returns false; after the element is painted, `window.handle_input(&focus, ElementInputHandler::new(bounds, viewer.clone()), cx)` is registered from the element's `paint` (viewer_element.rs) when `viewer.doc.is_some()`. A mouse click in the raw view places the caret at the clicked line and column (shape the line and use `index_for_x`, as the terminal's selection does); a drag selects.
- `viewer_element.rs`: paint the selection behind the text, the caret as a 2 px bar at the caret's x (blinking is out of scope: steady), the marked IME text underlined; colours for `raw_marks` from the theme (`Marker`, `Quote`: `text_dim`; `Heading`: `text` bold; `Emphasis`: `text` italic; `Code`: `tree.modified`; `Link`: `focus_ring`).
- keymap.rs: actions `ToggleMarkdownView` (`cmd-shift-m`, context `Viewer`) and `SaveFile` (`cmd-s`, context `Viewer`), rows after "toggle inline diff", `binding()`, `command_for`, `route!`, SPEC_ROWS, the count assertions (both actions have default keys, so the unbound counts do not change but `SPEC_ROWS` and `ACTIONS` lengths do; the "every action documented" config test needs the two DEFAULT_CONFIG comment lines). `run`: `ToggleMarkdownView => self.toggle_markdown_view(cx)` (Task 5 draws the pill; this task flips `mode` and saves the layout), `SaveFile => if let Some(v) = self.active_viewer_tab() { self.save_viewer(v, cx) }`.

- [ ] **Step 4: Run, check, commit**

Run the focused tests, the keymap and palette tests, the warning check, the suite.

```bash
git add crates/arkos/src/viewer.rs crates/arkos/src/viewer_element.rs crates/arkos/src/pane_view.rs crates/arkos/src/keymap.rs crates/arkos/src/command_palette.rs crates/arkos/src/config.rs crates/arkos/src/main.rs crates/arkos/src/edit.rs
git commit -m "Raw markdown is an editor: text input, the Mac editing keys, Cmd+S"
```

---

### Task 5: The rendered view (`md_view.rs`), inactive blocks

**Files:**
- Create: `crates/arkos/src/md_view.rs`
- Modify: `crates/arkos/src/viewer.rs` (`list_state: ListState`, `block_cache: Vec<Option<Vec<Inline>>>`), `crates/arkos/src/pane_view.rs` (`render_viewer` renders `md_view::render` in Rendered mode and the pill), `crates/arkos/src/main.rs` (`mod md_view;`)

**Interfaces:**
- Consumes: `md_blocks::inline`, `Viewer.blocks`, `Viewer.mode`.
- Produces: `md_view::render(viewer: &Entity<Viewer>, pane: PaneId, theme: &Theme, ui_family, mono_family, font_size, cx) -> AnyElement` (the `list()`), `md_view::block_element(inlines: &[Inline], active: bool, theme, fonts, font_size, base_dir: &Path, on_task_toggle, on_link) -> AnyElement`, `md_view::scale(kind) -> f32` (the type scale, pure, tested), `md_view::resolve_image(base_dir, path) -> Option<PathBuf>` (pure, tested: relative paths against the file's directory, absolute kept, `http` URLs give None), `pane_view::mode_pill(...)`.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn the_type_scale_follows_the_spec() {
    assert_eq!(scale(&BlockKind::Paragraph), 1.1);
    assert_eq!(scale(&BlockKind::Heading(1)), 1.1 * 1.9);
    assert_eq!(scale(&BlockKind::Heading(3)), 1.1 * 1.25);
    assert_eq!(scale(&BlockKind::Heading(6)), 1.1);
    assert_eq!(scale(&BlockKind::CodeBlock { info: String::new() }), 1.0, "code keeps the terminal size");
}

#[test]
fn images_resolve_against_the_file_s_directory() {
    let base = Path::new("/p/docs");
    assert_eq!(resolve_image(base, "img/a.png"), Some(PathBuf::from("/p/docs/img/a.png")));
    assert_eq!(resolve_image(base, "/abs/b.png"), Some(PathBuf::from("/abs/b.png")));
    assert_eq!(resolve_image(base, "https://x.y/c.png"), None);
    assert_eq!(resolve_image(base, "../c.png"), Some(PathBuf::from("/p/c.png")), "normalised");
}

#[test]
fn the_pill_names_the_other_mode() {
    assert_eq!(pill_label(ViewMode::Rendered), "show raw");
    assert_eq!(pill_label(ViewMode::Raw), "show rendered");
}
```

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- md_view:: pane_view::`

- [ ] **Step 3: Implement**

- `Viewer.list_state = ListState::new(blocks.len(), ListAlignment::Top, px(600.0))`, reset to the new count whenever `blocks.len()` changes (in `edit` and `apply`; `splice` when only one block's count changed is a later refinement). `block_cache[i]` holds the block's `inline` result, invalidated for the blocks whose line range or text changed (compare the block's lines before and after the edit; the simple rule "invalidate every block from the edited one to the end" is acceptable in this task).
- `md_view::render`: `list(state, move |ix, window, cx| ...)` building `block_element` for block `ix` from the cache (filling it on demand), with `active = false` in this task (Task 6 swaps in the raw element). Paragraph and heading: `StyledText::new(text).with_runs(runs)` where a `TextRun` per `Run` carries the font (UI family; weight bold for `bold`, italic style for `italic`; the mono family with `tab_inactive` background for `code`; `focus_ring` colour with an underline for a link; a strikethrough for `strike`), wrapped in `InteractiveText` whose `on_click` on a link run opens it: `cx.open_url` for `http(s)`, else the resolved path with `open` through `std::process::Command` on the background executor. Items: a row with the marker column (bullet or number, `text_dim`, width two characters, indented by `depth * 2` characters) and the runs; a task item's checkbox is a 12 px square with a check mark glyph, clickable, calling `on_task_toggle(line)`, which runs `md_blocks::toggle_task` through `Viewer::edit` (an undoable edit). Code: a `div` with the `tab_inactive` background, 8 px padding, the mono font, the text coloured by syntect when `Highlighter` knows the info string (`highlight` on a path made from the info, `format!("x.{info}")`), else plain. Quote: a 3 px `border` bar and dimmed runs. Rule: a 1 px `border` line with 8 px of space around. Table and Html: the source in the mono font on the `tab_inactive` background, no colour. Image: `img(path)` with `max_w_full`, else the alt text in a dashed `border` box. Every block has 6 px of space below; a `Blank` block renders as 12 px of empty height.
- `pane_view::render_viewer`: `match mode` picks `md_view::render` or the existing raw element; the pill is drawn for markdown tabs in both modes, exactly in the "jump to bottom" style with `top(px(if find_open { 44.0 } else { 10.0 }))`, text `pill_label(mode)`, `on_click` running `root.toggle_markdown_view_for(tab, cx)` (flips the mode in the tree and the viewer, saves the layout). The inline-diff chip keeps its place; when both the chip and the pill show, the pill is first (rightmost) and the chip sits 8 px left of it (compute the chip's `right` from the pill's measured width through a `canvas` as Task 5 of milestone 6d did for tab widths, or simply stack them: chip at `right(px(14.0))`, pill at `right(px(14.0 + 110.0))`; take the stacking, it is fixed widths).
- `scroll`: in Rendered mode the wheel is the list's; the viewer's `scroll_lines` keys (`pagedown` and so on) call `list_state.scroll_by` the equivalent in pixels (`rows_visible * line_h`).

- [ ] **Step 4: Run, check, commit**

Run the focused tests, the warning check, the suite.

```bash
git add crates/arkos/src/md_view.rs crates/arkos/src/viewer.rs crates/arkos/src/pane_view.rs crates/arkos/src/main.rs
git commit -m "The rendered markdown view: a virtualised list of blocks, and the mode pill"
```

---

### Task 6: The active block, caret placement and block navigation

**Files:**
- Modify: `crates/arkos/src/md_view.rs` (`RawBlockElement`, click mapping, navigation), `crates/arkos/src/viewer.rs` (`active_block()`, the input handler's field in Rendered mode, `set_caret_from_click`), `crates/arkos/src/md_blocks.rs` (`source_offset_at(runs, text_index) -> usize`, pure)

**Interfaces:**
- Produces: `md_blocks::source_offset_at(runs: &[Run], rendered_index: usize) -> usize` (the byte offset in the block's text for a rendered character index; a rendered index inside a run maps linearly within the run's source range; between runs, the next run's start); `md_view::RawBlockElement` (the active block's lines in the mono font with `raw_marks` colours, caret and selection, a mouse-down placing the caret, a drag selecting, registering the input handler over its bounds); `Viewer::active_block(&self) -> usize` (`block_at(blocks, caret.head.line)`); navigation: `Move::Up` on the active block's first line goes to the previous block's last line, `Move::Down` on its last line to the next block's first line, both clamped at the document's ends (`edit::Document::move_by` already does the line arithmetic, since the document is one buffer; the view only needs to reveal the new block: `list_state.scroll_to_reveal_item(active_block())` after a caret move).

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn a_rendered_click_maps_back_to_the_source() {
    let runs = match &inline("a **bold** c", &BlockKind::Paragraph)[0] { Inline::Para(r) => r.clone(), _ => unreachable!() };
    // Rendered text is "a bold c": index 0 'a', 2..6 "bold", 7 'c'.
    assert_eq!(source_offset_at(&runs, 0), 0);
    assert_eq!(source_offset_at(&runs, 2), 4, "inside the bold run: past the opening **");
    assert_eq!(source_offset_at(&runs, 5), 7);
    assert_eq!(source_offset_at(&runs, 7), 11, "the c after the closing **");
    assert_eq!(source_offset_at(&runs, 99), 12, "past the end: the block's end");
}

#[test]
fn block_navigation_clamps_at_the_ends() {
    let d = edit::Document::from_text("# H\n\npara one\npara two\n\n- item");
    let blocks = split(d.lines());
    assert_eq!(block_at(&blocks, 0), 0);
    let c = d.move_by(edit::Caret { head: Pos { line: 0, ch: 1 }, anchor: Pos { line: 0, ch: 1 }, goal_col: None }, Move::Up, false);
    assert_eq!(c.head, Pos { line: 0, ch: 1 }, "the first line of the first block stays");
    let c = d.move_by(edit::Caret { head: Pos { line: 5, ch: 2 }, anchor: Pos { line: 5, ch: 2 }, goal_col: None }, Move::Down, false);
    assert_eq!(c.head, Pos { line: 5, ch: 2 }, "the last line of the last block stays");
    let c = d.move_by(edit::Caret { head: Pos { line: 3, ch: 4 }, anchor: Pos { line: 3, ch: 4 }, goal_col: None }, Move::Down, false);
    assert_eq!(block_at(&blocks, c.head.line), 2, "Down from the paragraph's last line lands in the list block (through its blank line)");
}

#[test]
fn a_click_below_the_last_block_appends_a_paragraph() {
    let mut d = edit::Document::from_text("one");
    let c = append_paragraph(&mut d, Instant::now());
    assert_eq!(d.lines(), &["one".to_string(), String::new(), String::new()][..]);
    assert_eq!(c.head, Pos { line: 2, ch: 0 });
    let d2 = edit::Document::from_text("one\n\n");
    let mut d2 = d2;
    let c2 = append_paragraph(&mut d2, Instant::now());
    assert_eq!(d2.lines().len(), 3, "a document already ending in a blank line gets no second one");
    assert_eq!(c2.head.line, 2);
}
```

(`append_paragraph(doc, now) -> Caret` lives in `md_view.rs` as a pure helper.)

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- md_view:: md_blocks::`

- [ ] **Step 3: Implement**

- `RawBlockElement`: a custom `Element` like `ViewerElement` (reuse its line shaping, caret and selection painting by extracting shared helpers into `viewer_element.rs` rather than copying), painting the active block's lines at the mono size; its `paint` registers `window.handle_input(&focus, ElementInputHandler::new(bounds, viewer.clone()), cx)`; in Rendered mode the input handler's "field" is the active block's text (UTF-16 offsets relative to the block's first line), so IME ranges stay small.
- Mouse: a mouse-down on an inactive block's `InteractiveText` gives the rendered character index (`InteractiveText::on_click` passes the index); `source_offset_at` maps it to the block's byte offset, converted to a `Pos` by `md_blocks::pos_of_offset(block, lines, offset)`; the caret moves there and the block becomes active on the next frame (it is simply `block_at(caret)`). A mouse-down on a part with no runs (a bullet, a quote bar, an image, a rule) places the caret at the block's first character. A mouse-down below the last block (the list's empty tail: a trailing `div` item of `flex_1` height registered as one more list row) runs `append_paragraph`.
- The caret's block leaving: nothing to do; the view derives the active block from the caret, and `edit` or a move calls `list_state.scroll_to_reveal_item(active)` so the active block stays on screen.
- Find in Rendered mode: while `find.is_some()`, the block holding the current hit renders raw too (so two blocks may be raw at once), and `show_current_hit` scrolls the list to it.

- [ ] **Step 4: Run, check, commit**

Run the focused tests, the warning check, the suite.

```bash
git add crates/arkos/src/md_view.rs crates/arkos/src/md_blocks.rs crates/arkos/src/viewer.rs crates/arkos/src/viewer_element.rs
git commit -m "The active block is raw, clicks place the caret, arrows cross blocks"
```

---

### Task 7: Banners, README, and the last attributes

**Files:**
- Modify: `crates/arkos/src/pane_view.rs` (the conflict and save-error banners in `render_viewer`), `README.md`, any file still carrying a "Used from Task" attribute

- [ ] **Step 1: Banners**

In `render_viewer`, above the body (where the plain-file banner goes): when `viewer.conflict`, a banner row "The file changed on disk" with two text buttons "Reload" and "Keep mine" calling `root.resolve_conflict(tab, false | true, cx)`; when `save_error` is Some, its text with "Retry" calling `root.save_viewer(tab, cx)`. Same style as the plain-file banner (`px(12) py(4)`, `text_dim`, 12 px) with the buttons in `text` and an underline on hover.

- [ ] **Step 2: README**

Status rows (after the New Window row):

```
- open a README in Arkos: it renders; click a paragraph: it turns raw with the caret where you
  clicked; type, wait a second: `git diff` shows the change and the tab's " •" goes; press Enter in
  a list: the next bullet appears; tick a task checkbox: the file shows `[x]`
- the "show raw" pill at the top right of the pane, or Cmd+Shift+M, shows the raw file and
  back; the choice survives a relaunch; another markdown tab keeps its own mode
- edit the file in another editor while a line is half typed: the banner offers Reload and
  Keep mine; Cmd+S means Keep mine; with nothing unsaved the file simply reloads
- a 5 MB markdown file opens and scrolls without stutter; typing in it keeps up; Reduce
  Motion changes nothing here; a `.rs` file is still read-only
```

Keybinding table rows for "toggle markdown view" (Cmd+Shift+M) and "save file" (Cmd+S), and an "Editing markdown" section under the viewer's section: the two modes, the active block, autosave and Cmd+S, line endings kept, the conflict banner, what is not editable yet (tables render read-only, other file types), and that images render when their path resolves.

- [ ] **Step 3: Attributes and the gates**

`grep -rn "Used from Task" crates/arkos/src` prints nothing; a `grep -c` for the em-dash character over README.md and `crates/arkos/src/*.rs` is 0; the warning check and the suite pass.

```bash
git add README.md crates/arkos/src/pane_view.rs
git commit -m "Markdown editing: the conflict and save banners, and the README"
```

---

## Deferred, for the user to rule on (not in this milestone)

- Inline conversion while typing within the active block (Typora style).
- Editable tables, image insertion, media (Tier B); live syntax colour inside code blocks while editing; footnotes (Tier C).
- A blinking caret; spell check; multiple carets.
- One block per list item instead of one block per list (section 14).
- `Arc::make_mut` on the shared lines instead of a copy per edit, if the copy shows in the frame budget.
- Editing non-markdown files (the code editor milestone).

## Self-review notes

- Spec section 3 (files and modes): Tasks 3, 5. Section 4 (core): Task 1. Section 5 (blocks and view): Tasks 2, 5, 6. Section 6 (keys and input): Task 4. Section 7 (saving): Task 3. Section 8 (external changes): Tasks 3, 7. Section 9 (the pill): Task 5. Section 10 (find): Tasks 4, 6. Section 11 (performance): the benchmark in Task 2, the virtualised list in Task 5, the deferred `make_mut`. Section 12 (tests): as named. Section 13 (README): Task 7.
- The editing core never imports markdown; the markdown helpers never touch gpui; the view never parses markdown itself. That is the seam the later code editor relies on.
