# Inline reveal (milestone A1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In Rendered mode the block holding the caret stays rendered in the document font; only the markdown element under the caret (or under either end of the selection) shows its syntax, a heading shows its `#` whenever the caret is on it, and bullets, task boxes, quote bars and item indents never show theirs. Links edit on a first click and follow on a click of their revealed address. Up and Down move by displayed row inside the block. Backspace at the start of an item removes its bullet.

**Architecture:** `md_blocks` gains an element tree per block from `pulldown-cmark`'s offset events (every element's full span, delimiters included); a marker is any byte of a span no child covers, hidden when it is a line prefix (`> `, `- `, `1. `, `- [x] `, an indent) and revealable otherwise. A new pure module `md_reveal` turns the tree, the block's runs and the focus offsets into a `Display`: segments that show verbatim, rendered or not at all, with a monotone two-way map between source offsets and display bytes, plus the caret helpers (`snap` over hidden markers, `unmark` for Backspace, `vertical` for row moves). `md_view` gets a third row form, `reveal_row`, a `StyledText` from the display with the caret painted by a canvas reading its `TextLayout`, recorded per frame as the raw blocks are so the viewer's mouse, keys and input method map through it. The text buffer stays the single source of truth; nothing is rewritten on save.

**Tech Stack:** Rust, gpui 0.2.2 (the fork), `pulldown-cmark` offset iterator, `StyledText`/`InteractiveText`/`TextLayout` (`position_for_index`, `index_for_position`, `line_layout_for_index`).

**Spec:** `docs/superpowers/specs/2026-10-30-inline-reveal-design.md`

## Progress (2026-10-30)

Tasks 1 to 8 are done on branch `reveal`, each with its tests and commit; `cargo test -p arkos`
passes (1138) and `cargo check --workspace --all-targets` shows no warnings. Task 9 (the screen
pass and the 5 MB frame gate) is open: the session that built this could not capture the screen
or post input, so every Status row from Task 8 still wants a person at the screen before the merge.
Two `arkosd` pty tests fail inside that session's process tree (responsible-process checks); the
branch does not touch `arkosd`.

Where the build differs from the steps below, from what the parser actually reports:

- A code span is a node spanning its backticks with one text child, so its ticks are markers like
  `**`. A hard break is a leaf (its `\` or two spaces show when the caret is on it). An escape's
  backslash is a paragraph-level marker, revealed with the plain text around the caret.
- `NodeKind::Link` carries `dest: Option<Range>` and `url`; a collapsed or shortcut reference has
  no address to show. `Node` carries the leaf's rendered `text`, so the display needs no runs.
- `md_blocks::revealable` sends a block to its source when the tree cannot place all its text (a
  link definition) or it holds more than prose (a code block in an item, a list in a quote).
- `unmark` returns one replacement: an item's marker, or every `>` of a quoted paragraph, so the
  paragraph leaves the quote in one undo step instead of becoming a lazy continuation.
- Displayed rows come from the text layout's glyph positions grouped by height (`rows_by_y`), not
  from wrap boundaries; `md_reveal::vertical` and `row_edges` work on those rows.
- Task 7 also reveals an HTML block that is one `<img>` (`BlockTree::picture`).

## Global Constraints

- Worktree `~/Projects/ide-arkos/.claude/worktrees/reveal`, branch `reveal` off `main`. Copy `.cargo/config.toml` from the main checkout into the worktree first (it is excluded from git; it keeps the shared `target-dir` and three jobs). `nice -n 15 cargo test -p arkos <filter>`, `nice -n 15 cargo check --workspace --all-targets`; never `--release`; one cargo at a time.
- Tests first, headless, named as sentences; doc comments in plain prose, no em-dashes.
- Every colour is read from the theme at paint time; nothing animates here, so `Root::any_motion` is untouched and Reduce Motion changes nothing.
- The buffer is never rewritten by a reveal: `md_reveal` is pure and only `unmark` produces an edit, through `Viewer::edit`.
- Offsets: everything in `md_reveal` is in bytes of the block's text (its lines joined with `\n`), as `Run::source` is; `md_blocks::pos_of_offset` turns them into positions and the inverse is `offset_of_pos` (Task 1).
- Commit after each task with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. The caret inside a hidden marker (after an undo, a reload, Cmd+Left today): it must be drawn at the marker's end, a move must step over it, and typing must insert at the marker's end, never before the bullet (Task 2 `snap`, Task 5 the insert path). A char typed before `- ` would silently turn the item into a paragraph.
2. A soft break whose range the parser gives to a gap rather than a `SoftBreak` event (the `\n> ` between two quote lines, the `\n  ` of an item's continuation): the newline must still show as one space or two words run together (Task 1 fixture pins the ranges; Task 2 rule 4).
3. Revealing a block changes its height by a row when the markers push a wrap: the caret's row must stay in view after a click and after Up or Down into the block (Task 4 `reveal_in_list` reads the reveal layout).
4. The find bar: with the current hit in a paragraph the block is revealed with the hit's ends as focus and the hit painted, and Enter stepping to a hit in another block moves the reveal with it (Task 4).
5. A plain click on a revealed link's text must place the caret and not open the link, and a click on the revealed address must open it once, not also move the caret away first (Task 6: the click range covers only `dest`, the row's press handler sees the press flag as it does for task boxes).

---

### Task 1: The element tree and its markers (`md_blocks`)

**Files:**
- Modify: `crates/arkos/src/md_blocks.rs` (after `inline`, about line 524; tests at the end)

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind { Paragraph, Heading(u8), List, Item, Quote, Strong, Emphasis, Strike, Code, Link { dest: Range<usize> }, Image, Text, SoftBreak, HardBreak, Html, CodeBlock }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node { pub kind: NodeKind, pub span: Range<usize>, pub children: Vec<Node> }
/// The element tree of one block's text: the top-level nodes in source order.
pub fn elements(block_text: &str) -> Vec<Node>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerClass { Hidden, Revealable }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker { pub range: Range<usize>, pub class: MarkerClass, /// the index path of the node the marker belongs to
    pub owner: Vec<usize> }
/// Every byte of `text` no leaf covers, grouped into runs, each owned by the innermost node whose span holds it.
pub fn markers(text: &str, nodes: &[Node]) -> Vec<Marker>;
/// The byte offset in the block's text of a document position inside the block (the inverse of `pos_of_offset`).
pub fn offset_of_pos(block: &Block, lines: &[String], p: edit::Pos) -> usize;
pub fn line_prefix_len(line: &str) -> usize;  // `> `, `- `, `1. `, `- [x] `, indents, in any order markdown allows
```

- `MarkerClass::Hidden` when the marker's bytes, taken from their line start, lie within `line_prefix_len` of that line (so `- `, `> `, `  ` indents and `[ ] ` boxes are hidden; `## ` is not, since `#` is no prefix character); every other marker is `Revealable`. A newline inside any marker is left to the display (Task 2 shows it as a space).
- A `Link`'s `dest` is the byte range of the destination inside its closing marker: for `[t](u "title")` the `u`, for an autolink `<u>` the `u`, for a reference link `[t][r]` the `r`; found by locating `dest_url` (or the label) in the span's text after the last child.

- [ ] **Step 1: Pin the parser's ranges.** Write a test that prints nothing but asserts the ranges `Parser::new_ext(src, OPTIONS).into_offset_iter()` gives for these fixtures, so the rules below rest on facts: `"a **b** c"`, `"# H #"`, `"- a\n- b\n  - c"`, `"> q1\n> q2"`, `"- item\n  continued"`, `"a  \nb"`, `"a\\\nb"`, `"[t](u \"T\")"`, `"<https://x>"`, `"[t][r]\n\n[r]: https://x"`, `"![a](p.png) text"`, `"`` a ` b ``"`. Record where the `\n` of a quote's or an item's continuation lands (a `SoftBreak` event or a gap).

- [ ] **Step 2: Write the failing tests:**

```rust
#[test]
fn the_element_tree_spans_every_delimiter_and_its_markers_are_the_bytes_no_leaf_covers() {
    let src = "a **b _c_** `d` [e](https://x \"T\") ~~f~~ ![g](h.png) <https://y> i\\\nj";
    let nodes = elements(src);
    let strong = find(&nodes, |n| n.kind == NodeKind::Strong).unwrap();
    assert_eq!(&src[strong.span.clone()], "**b _c_**");
    assert_eq!(&src[find(&nodes, |n| n.kind == NodeKind::Emphasis).unwrap().span.clone()], "_c_");
    let link = find(&nodes, |n| matches!(n.kind, NodeKind::Link { .. })).unwrap();
    let NodeKind::Link { dest } = &link.kind else { unreachable!() };
    assert_eq!((&src[link.span.clone()], &src[dest.clone()]), ("[e](https://x \"T\")", "https://x"));
    let m = markers(src, &nodes);
    let shown: Vec<&str> = m.iter().map(|k| &src[k.range.clone()]).collect();
    assert!(shown.contains(&"**") && shown.contains(&"_") && shown.contains(&"`") && shown.contains(&"[") && shown.contains(&"](https://x \"T\")") && shown.contains(&"~~"));
    assert!(m.iter().all(|k| k.class == MarkerClass::Revealable), "nothing in a paragraph is a line prefix");
    assert!(m.iter().any(|k| &src[k.range.clone()] == "\\" || src[k.range.clone()].starts_with('\\')), "a hard break's backslash is a marker");
}

#[test]
fn headings_reveal_their_hashes_while_items_and_quotes_hide_their_prefixes() {
    let h = "## Title ##";
    let m = markers(h, &elements(h));
    assert_eq!(m.iter().map(|k| (&h[k.range.clone()], k.class)).collect::<Vec<_>>(), vec![("## ", MarkerClass::Revealable), (" ##", MarkerClass::Revealable)]);
    let l = "1. first\n2. second\n   - [x] done\n   - [ ] todo";
    let m = markers(l, &elements(l));
    assert!(m.iter().all(|k| k.class == MarkerClass::Hidden), "{m:?}");
    assert!(m.iter().any(|k| &l[k.range.clone()] == "- [x] "));
    let q = "> one\n> two";
    let m = markers(q, &elements(q));
    assert!(m.iter().all(|k| k.class == MarkerClass::Hidden));
    assert_eq!(m.iter().filter(|k| l_or(&q[k.range.clone()]).contains('>')).count(), 2, "each line's prefix");
    assert_eq!(line_prefix_len("  - [ ] x"), 8);
    assert_eq!(line_prefix_len("> > y"), 4);
    assert_eq!(line_prefix_len("## z"), 0);
}

#[test]
fn offset_of_pos_is_the_inverse_of_pos_of_offset() {
    let doc = lines("# T\n\npara one\npara two\n\n- a");
    let blocks = split(&doc);
    for b in &blocks {
        let text = crate::md_view::block_source(&doc, b);
        for off in 0..=text.len() {
            if !text.is_char_boundary(off) { continue; }
            let p = pos_of_offset(b, &doc, off);
            assert_eq!(pos_of_offset(b, &doc, offset_of_pos(b, &doc, p)), p);
        }
    }
}
```

- [ ] **Step 3: Run to verify they fail:** `nice -n 15 cargo test -p arkos md_blocks 2>&1 | tail -15`. Expected: `elements`, `markers`, `offset_of_pos`, `line_prefix_len` undefined.

- [ ] **Step 4: Implement.** `elements` walks the offset events with a stack: `Start(tag)` pushes a node with the event's range, `End` pops it into its parent; `Text`, `Code`, `SoftBreak`, `HardBreak`, `InlineHtml`/`Html` are leaves; `TaskListMarker` is dropped (its bytes become a hidden marker of the item); an image's inner text events are dropped so the whole `![..](..)` is one leaf. `markers` walks the tree, and for each node sorts its children by span and emits the gaps between `span.start`, the children and `span.end`, classed by `line_prefix_len` of the line the gap starts on. `offset_of_pos` sums line lengths plus one per newline from the block's first line. Keep `inline` and `Run` untouched.

- [ ] **Step 5: Run the tests, then** `nice -n 15 cargo check --workspace --all-targets`.

- [ ] **Step 6: Commit** `git commit -m "Markdown blocks: the element tree of a block, its markers and which are line prefixes"`

---

### Task 2: `md_reveal`: the display, the map, snap and unmark

**Files:**
- Create: `crates/arkos/src/md_reveal.rs`
- Modify: `crates/arkos/src/main.rs` (`mod md_reveal;` after `mod md_blocks;`)

**Interfaces:**

```rust
/// Where the user is in a block: the caret, the selection's ends, or a find hit's ends.
pub struct Focus { pub offsets: Vec<usize>, pub selection: Option<Range<usize>>, pub hits: Vec<(Range<usize>, bool)>, pub marked: Option<Range<usize>> }
pub enum Shown { Verbatim, Rendered(String), Hidden }
pub struct Segment { pub source: Range<usize>, pub shown: Shown, pub style: md_blocks::Style, /// the marker's node when the segment is a revealed marker
    pub marker: bool }
pub struct Display { pub segments: Vec<Segment>, pub text: String, /// each segment's byte range in `text`
    pub display: Vec<Range<usize>> }
impl Display {
    pub fn new(text: &str, nodes: &[md_blocks::Node], runs: &[md_blocks::Run], focus: &Focus) -> Display;
    /// For a `List` block: the display of the item whose span holds a focus offset, else `None`.
    pub fn for_item(text: &str, nodes: &[Node], item: &Node, runs: &[Run], focus: &Focus) -> Display;
    pub fn display_of(&self, source: usize) -> usize;   // monotone; a hidden byte maps to its segment's end
    pub fn source_of(&self, display: usize) -> usize;   // monotone; never inside a hidden marker
    /// The set of revealed node paths for `focus` (rule 1 of the spec).
    pub fn revealed(nodes: &[Node], focus: &[usize]) -> Vec<Vec<usize>>;
    /// The display ranges of the selection, each hit, and the marked text, for the row's run backgrounds and underline.
    pub fn ranges(&self, r: &Range<usize>) -> Range<usize>;
}
/// After a move in `dir`, a caret inside a hidden marker goes on to the first visible offset.
pub fn snap(markers: &[Marker], offset: usize, dir: Dir) -> usize;
/// Backspace at the first visible offset of an item or a quote line: the hidden prefix to delete, as a byte range of the block's text; `None` elsewhere.
pub fn unmark(text: &str, markers: &[Marker], offset: usize) -> Option<Range<usize>>;
/// Up or Down by displayed row: the display byte on the row above or below nearest `goal_x`, or `None` at the first or last row.
pub fn vertical(rows: &[Range<usize>], x_of: impl Fn(usize) -> f32, at: usize, dir: Dir, goal_x: f32) -> Option<usize>;
/// The start and end of the row holding `at`, the start skipping a hidden prefix.
pub fn row_edges(rows: &[Range<usize>], at: usize) -> (usize, usize);
```

The rules (spec section 4): the revealed set is, per focus offset, the outermost inline node whose span contains it inclusively, plus the `Heading` node when there is one, plus the leaf holding the offset when no node does; inside a revealed node every byte is `Verbatim` with the node path's style (bold inside `Strong`, italic inside `Emphasis`, code inside `Code` with the backticks, link colour inside `Link`); elsewhere a leaf shows `Rendered(run.text)` for the run with the same source range (a `SoftBreak` as `" "`, a `HardBreak` as `"\n"`), a `Revealable` marker is `Hidden`, a `Hidden` marker is `Hidden` except that a `\n` inside it is `Rendered(" ")` (one space per newline, so quote and item continuation lines read as one paragraph).

- [ ] **Step 1: Write the failing tests** (a `fixture(src, caret)` helper builds nodes, runs and markers through `md_blocks`):

```rust
#[test]
fn nothing_is_revealed_with_the_caret_outside_every_element_and_the_map_is_monotone() {
    let (d, src) = fixture("a **b** c\nd", 0);
    assert_eq!(d.text, "a b c d");
    for i in 0..src.len() { assert!(d.display_of(i) <= d.display_of(i + 1)); }
    assert_eq!(d.source_of(d.display_of(4)), 4, "a verbatim byte round-trips");
}

#[test]
fn the_caret_in_a_strong_shows_its_markers_bold_until_one_past_the_closing_one() {
    let (d, _) = fixture("a **b** c", 4);
    assert_eq!(d.text, "a **b** c");
    assert!(d.segments.iter().filter(|s| s.marker).all(|s| s.style.bold));
    assert_eq!(fixture("a **b** c", 7).0.text, "a **b** c", "just after the closing marker: still revealed");
    assert_eq!(fixture("a **b** c", 8).0.text, "a b c", "one further: hidden");
}

#[test]
fn a_nested_element_reveals_with_its_outer_one_and_a_plain_run_shows_its_escapes() {
    assert_eq!(fixture("**a _b_ c**", 5).0.text, "**a _b_ c**");
    assert_eq!(fixture("x \\* y &amp; z", 3).0.text, "x \\* y &amp; z");
    assert_eq!(fixture("x \\* y &amp; z", 0).0.text, "x \\* y &amp; z", "the run is the whole paragraph");
    assert_eq!(fixture("[e](https://x) f", 1).0.text, "[e](https://x) f");
}

#[test]
fn a_heading_shows_its_hashes_anywhere_on_it_and_an_item_never_shows_its_bullet() {
    assert_eq!(fixture("## Title", 8).0.text, "## Title");
    let (d, _) = fixture("- one\n- two", 2);
    assert_eq!(d.text, "one");  // for_item: the caret's item only
    assert_eq!(d.display_of(0), 0, "an offset in the hidden marker is drawn at its end");
    assert_eq!(d.source_of(0), 2, "and the start of the display is the first visible offset");
    let (q, _) = fixture("> one\n> two", 3);
    assert_eq!(q.text, "one two", "the continuation's newline is a space");
}

#[test]
fn a_selection_reveals_both_ends_and_nothing_between() {
    let (d, _) = fixture_sel("**a** b `c` d ~~e~~", 2..17);
    assert_eq!(d.text, "**a** b c d ~~e~~");
}

#[test]
fn snap_steps_over_hidden_markers_and_unmark_removes_a_prefix_only_at_the_text_start() {
    let (text, m) = marks("- one\n- two");
    assert_eq!(snap(&m, 6, Dir::Right), 8, "Right from one's end lands after two's marker");
    assert_eq!(snap(&m, 7, Dir::Left), 5, "Left into the marker goes to the previous line's end");
    assert_eq!(unmark(&text, &m, 8), Some(6..8));
    assert_eq!(unmark(&text, &m, 9), None);
    let (text, m) = marks("- [ ] a");
    assert_eq!(unmark(&text, &m, 6), Some(0..6));
    let (text, m) = marks("> q");
    assert_eq!(unmark(&text, &m, 2), Some(0..2));
    let (text, m) = marks("## h");
    assert_eq!(unmark(&text, &m, 3), None, "a heading's hashes are text to Backspace");
}

#[test]
fn vertical_moves_by_displayed_row_toward_the_goal_x_and_stops_at_the_edges() {
    let rows = [0..10, 11..20, 21..25];
    let x_of = |b: usize| (b - rows.iter().find(|r| r.contains(&b) || r.end == b).unwrap().start) as f32 * 8.0;
    assert_eq!(vertical(&rows, x_of, 3, Dir::Down, 24.0), Some(14));
    assert_eq!(vertical(&rows, x_of, 14, Dir::Down, 72.0), Some(25), "a short row: its end");
    assert_eq!(vertical(&rows, x_of, 3, Dir::Up, 24.0), None);
    assert_eq!(row_edges(&rows, 14), (11, 20));
}
```

- [ ] **Step 2: Run to verify they fail:** `nice -n 15 cargo test -p arkos md_reveal 2>&1 | tail -15`.

- [ ] **Step 3: Implement** in the order: `revealed`, segment emission (walk the tree depth-first emitting leaves and markers in source order, each with its node path's style), `text` and `display` from the segments, `display_of`/`source_of` by binary search on `segments[i].source`, `snap`, `unmark`, `vertical`, `row_edges`. `vertical` picks, on the target row, the byte whose `x_of` is nearest `goal_x` (the row's end when every byte is left of it).

- [ ] **Step 4: Add the benchmark test** `a_20_000_line_document_s_active_paragraph_reveals_in_time`: a 1 MB document, `elements` + `markers` + `Display::new` on its longest paragraph under 1 ms in a debug build with the test's own timing margin (as `a_megabyte_splits_in_time` does).

- [ ] **Step 5: Run, check, commit** `git commit -m "md_reveal: the display of an active block, its source map, snap, unmark and row moves"`

---

### Task 3: The reveal row in the rendered view

**Files:**
- Modify: `crates/arkos/src/md_view.rs` (`Rendered` gains `nodes` and `markers`, about line 126; `fill_cache` about line 636 builds them; `render`'s row closure about line 833; new `reveal_row`, `RevealLayout`; `Parts`/`Part` gain `Reveal`; `caret_at` about line 1108)
- Modify: `crates/arkos/src/viewer.rs` (`Viewer.reveal_blocks: Vec<md_view::RevealLayout>` beside `raw_blocks`, cleared where `raw_blocks` is; new `active_form`, replacing the bool of `renders_raw` for the row closure; `focus_for(ix) -> md_reveal::Focus`)

**Interfaces:**

```rust
pub enum ActiveForm { Rendered, Revealed, Raw }
impl Viewer {
    /// `Revealed` for an active Paragraph, Heading, List or Quote block (and an image paragraph), `Raw` for every other active kind, `Rendered` otherwise; the find bar's current hit counts as active.
    pub fn active_form(&self, ix: usize, focused: bool) -> ActiveForm;
    /// The focus offsets of block `ix` in its text: the caret and the selection's ends inside it, the find hits inside it with the current one marked, the marked text.
    pub fn focus_for(&self, ix: usize) -> md_reveal::Focus;
}
pub struct RevealLayout { pub block: usize, pub lines: Range<usize>, pub layout: TextLayout, pub display: Rc<md_reveal::Display>, pub line_h: f32 }
```

- [ ] **Step 1: Write the failing tests** in `viewer.rs`'s test module (the viewer's pure helpers; the row itself is a hand check):

```rust
#[test]
fn an_active_paragraph_is_revealed_a_code_block_stays_raw_and_the_find_hit_counts_as_active() {
    let (v, _) = viewer_with("para\n\n```\ncode\n```\n\n# head");
    // caret in the paragraph
    assert_eq!(v.active_form(0, true), ActiveForm::Revealed);
    assert_eq!(v.active_form(1, true), ActiveForm::Rendered);
    assert_eq!(v.active_form(0, false), ActiveForm::Rendered, "no focus: nothing active");
    let (v, _) = viewer_with_caret("para\n\n```\ncode\n```", Pos { line: 3, ch: 0 });
    assert_eq!(v.active_form(1, true), ActiveForm::Raw);
    let (v, _) = viewer_finding("one\n\ntwo", "two");
    assert_eq!(v.active_form(1, true), ActiveForm::Revealed);
    assert_eq!(v.focus_for(1).hits.len(), 1);
}

#[test]
fn the_focus_of_a_block_is_in_its_own_bytes() {
    let (v, _) = viewer_with_selection("a **b**\n\nc d", Pos { line: 0, ch: 3 }, Pos { line: 2, ch: 1 });
    assert_eq!(v.focus_for(0).offsets, vec![3]);
    assert_eq!(v.focus_for(2).offsets, vec![1]);
    assert_eq!(v.focus_for(0).selection, Some(3..7), "the selection runs to the block's end");
    assert_eq!(v.focus_for(2).selection, Some(0..1));
}
```

(`viewer_with*` helpers: build a `Viewer` through the existing test constructor used by `the_rows_and_the_cache_follow_the_blocks_through_an_edit_an_undo_and_a_reload`, place the caret, and for find call `open_find` and `find_paste`.)

- [ ] **Step 2: Run to verify they fail.**

- [ ] **Step 3: Implement the viewer side.** `active_form` reuses `renders_raw`'s conditions (caret placed, not a pending append, focused, the block within the selection's blocks, or holding the current hit) and maps the kind: `Paragraph | Heading | List | Quote => Revealed`, else `Raw`. `focus_for` converts positions with `md_blocks::offset_of_pos`, clipping the selection to the block.

- [ ] **Step 4: Implement the row.** In `fill_cache` build `nodes = md_blocks::elements(&text)` and `markers` with the pieces and keep them in `Rendered`. In the row closure replace `if raw { raw_row }` with a match on `active_form`: `Raw => raw_row`, `Revealed => reveal_row(ix, ..)`, `Rendered` as today. `reveal_row`:
  - fills the cache as the rendered row does (`fill`), reads the block's `Rendered`, builds `focus_for(ix)`;
  - for a `Paragraph`, `Heading` or `Quote` piece: `Display::new`; for a `List`: for each `Item` piece the item's `Node` (match by the first run's source offset) and `Display::for_item` for the one holding a focus offset, the others drawn through `item_element` as today;
  - the display's segments become one string and `TextRun`s through a new `display_runs(&Display, weight, color, ctx)` beside `styled_runs` (same font and colour rules; a `Verbatim` marker takes its segment's style; the selection's display range sets `background_color: ctx.selection`, hits `ctx.selection`/`ctx.find_current`, the marked range an underline in `ctx.caret`);
  - the text is a `StyledText` (an `InteractiveText` when the display holds a revealed link, Task 6); its `TextLayout` is cloned into a `RevealLayout` pushed on `Viewer.reveal_blocks` from a canvas's prepaint closure, and the same canvas's paint closure paints the caret: `layout.position_for_index(display.display_of(caret_offset))` when the viewer has the focus, with `element::paint_caret(window, x, y, layout.line_height(), ctx.caret)`;
  - the row keeps `block_element`'s padding, font, size, line height and press handlers; `caret_at` gains a `Part::Reveal(Rc<Display>)` arm: `index_near(layout, at)` then `display.source_of(byte)` then `pos_of_offset`.
  - The image paragraph form (Task 7) and the link click (Task 6) come later; for now an image paragraph is `Raw`.

- [ ] **Step 5: Hand check in the sandbox** (`nice -n 15 cargo build -p arkos -p arkosd`, `target/debug/arkos sandbox ~/Projects/ide-arkos`): open `docs/guide.md`, click a bold word: the paragraph keeps its font and `**` appears around that word; click a heading: `##` shows; click a bullet item: the bullet stays, the caret is at the text; Shift+click elsewhere: the selection paints; the caret paints and blinks as the raw one does (it does not).

- [ ] **Step 6: Check, commit** `git commit -m "Rendered view: the active paragraph, heading, item or quote stays rendered and reveals the element under the caret"`

---

### Task 4: Mouse, scroll and the input method through the reveal layout

**Files:**
- Modify: `crates/arkos/src/viewer.rs` (`pos_at_point` about line 1758, `point_of_pos` about line 1774, `reveal_in_list` about line 1617, `bounds_for_range` about line 2028, `character_index_for_point` about line 2036)
- Modify: `crates/arkos/src/md_view.rs` (`RevealLayout::pos_at`, `point_of`, `bottom`, mirroring `BlockLayout`'s)

- [ ] **Step 1: Write the failing tests:** `RevealLayout` cannot be built headless (a `TextLayout` needs a window), so the pure parts are tested: a `Layouts` helper that, given the raw and reveal blocks' line ranges and bottoms, picks the one for a y or a line (`layout_for_y`, `layout_for_line`), with tests that a reveal block between two raw ones is found by its lines and by its height, and that `reveal_in_list`'s caret y for a revealed block is `row * line_h` from its display rows (`caret_row_y(rows, display_byte, line_h)`).

- [ ] **Step 2: Implement.** `RevealLayout::pos_at(lines, at)`: `index_near(layout, at)` then `source_of` then `pos_of_offset`; `point_of(lines, p)`: `offset_of_pos` then `display_of` then `position_for_index`; `bottom()` from `layout.bounds()`. `pos_at_point` and `point_of_pos` search `reveal_blocks` and `raw_blocks` together (by line, else by y). `reveal_in_list` takes the caret's y from the reveal layout's `point_of` relative to its bounds when the active block has one. `bounds_for_range` takes `line_h` from the reveal layout when there is one. Clear `reveal_blocks` where `raw_blocks` is cleared in `render`.

- [ ] **Step 3: Hand check:** a long wrapped paragraph: click near its end, the view does not jump; press Enter at the end of a paragraph low on screen: the new line's caret is in view; Cmd+F, type a word in a later paragraph: that paragraph reveals in the document font with the hit highlighted, Enter steps on; a Japanese input method's candidate window sits under the caret in a revealed paragraph.

- [ ] **Step 4: Check, commit** `git commit -m "Rendered view: the mouse, the caret reveal and the input method map through revealed blocks"`

---

### Task 5: Keys in a revealed block

**Files:**
- Modify: `crates/arkos/src/viewer.rs` (`rendered_key` about line 1527; `run_key`'s `DeleteBack` path; `replace_text_in_range` and `replace_and_mark_text_in_range` insert path, about line 1946)
- Modify: `crates/arkos/src/md_view.rs` (`navigate` about line 2070 gains the row form)

- [ ] **Step 1: Write the failing tests** (pure, on `apply_action`-level helpers):

```rust
#[test]
fn in_a_revealed_item_left_and_right_step_over_the_bullet_and_typing_lands_after_it() {
    let doc = doc("- one\n- two");
    let c = move_snapped(&doc, &blocks(&doc), Caret::at(Pos { line: 1, ch: 2 }), Move::Left, false);
    assert_eq!(c.head, Pos { line: 0, ch: 5 });
    let c = move_snapped(&doc, &blocks(&doc), Caret::at(Pos { line: 0, ch: 5 }), Move::Right, false);
    assert_eq!(c.head, Pos { line: 1, ch: 2 });
    assert_eq!(insert_pos(&doc, &blocks(&doc), Pos { line: 1, ch: 0 }), Pos { line: 1, ch: 2 }, "typing with the caret in the marker inserts after it");
    assert_eq!(move_snapped(&doc, &blocks(&doc), Caret::at(Pos { line: 1, ch: 3 }), Move::LineStart, false).head, Pos { line: 1, ch: 2 });
}

#[test]
fn backspace_at_an_item_s_text_start_removes_its_marker_in_one_undoable_step() {
    let mut doc = doc("- [ ] a\n- b");
    let c = backspace_revealed(&mut doc, &blocks(&doc), Caret::at(Pos { line: 0, ch: 6 }));
    assert_eq!((doc.lines()[0].as_str(), c.head), ("a", Pos { line: 0, ch: 0 }));
    assert!(doc.undo().is_some());
    assert_eq!(doc.lines()[0], "- [ ] a");
    let mut doc = doc("- ab");
    backspace_revealed(&mut doc, &blocks(&doc), Caret::at(Pos { line: 0, ch: 3 }));
    assert_eq!(doc.lines()[0], "- b", "elsewhere it is a grapheme delete");
}

#[test]
fn up_and_down_in_a_revealed_block_go_by_displayed_row_and_cross_blocks_at_the_edges() {
    // rows and x_of as in md_reveal's test; navigate_rows(rows, x_of, display_at, dir) returns Some(display byte) or None (cross)
    ...
}
```

- [ ] **Step 2: Implement.**
  - `move_snapped`: after `doc.move_by` for `Left`, `Right`, `WordLeft`, `WordRight`, `LineStart`, `LineEnd`, if the head is in a revealed block (`active_form == Revealed` reasoning on kinds) and inside a hidden marker, `md_reveal::snap` in the move's direction (Left-ish moves snap left, Right-ish and `LineStart` snap right). Wire it into `rendered_key` for those moves (today they fall through to `apply_action`).
  - The insert path: in `replace_text_in_range` (and the marked variant), when the caret is in a hidden marker of a revealed block, move it to the marker's end first (`snap(.., Dir::Right)`), as its own caret move (`break_coalescing` already happens on placement).
  - `backspace_revealed`: in `run_key`, `DeleteBack(Grapheme)` with no selection in a revealed block asks `md_reveal::unmark`; `Some(range)` becomes `doc.replace_range(start, end, "")` through `self.edit`; else the usual delete.
  - Row moves: `rendered_key` for `Up`/`Down` reads the active block's `RevealLayout` (if the block is revealed and on screen): rows from `layout.line_layout_for_index` over the display (collect the wrapped rows as `element::rows_from_breaks` does for raw lines), `x_of` from `position_for_index`, the goal x kept on the viewer (`goal_x: Option<f32>`, cleared by every other key as `goal_col` is); `md_reveal::vertical` gives the display byte, `source_of` the offset, `pos_of_offset` the position; `None` falls through to `navigate`, which crosses blocks as today. `Cmd+Left`/`Cmd+Right` use `row_edges` the same way. When the block has no layout (scrolled away), the line-based moves apply as today.

- [ ] **Step 3: Hand check:** a wrapped paragraph: Up and Down walk the displayed rows and keep the x; Down from the last row goes to the next block; Left from an item's first letter lands at the end of the item above; Backspace there removes the bullet and the line reads as a paragraph; Undo brings the bullet back; type `**x**` in a paragraph: `x` is bold as the second `*` lands and the markers hide when the caret moves on; type `- ` at a line start: a bullet appears.

- [ ] **Step 4: Check, commit** `git commit -m "Rendered view: moves step over hidden markers, Backspace removes a bullet, Up and Down go by displayed row"`

---

### Task 6: Links: click to edit, click the address to follow

**Files:**
- Modify: `crates/arkos/src/md_view.rs` (`runs_text` about line 1207 and the reveal row's text; `block_element`'s press handler about line 990)

- [ ] **Step 1: Write the failing test:** `md_reveal`: `Display::link_dests(&self) -> Vec<(Range<usize>, String)>`, the display ranges and targets of every revealed link's `dest`; a test that `"[e](https://x) [f](https://y)"` with the caret in `e` yields one entry covering `https://x` and none for the hidden link.

- [ ] **Step 2: Implement.** In the reveal row, when `link_dests` is not empty, the text is an `InteractiveText` whose click ranges are those display ranges; its handler follows the link on a plain click too (not only Cmd), and sets the row's press flag (the `task_press` cell) on mouse down inside a dest range so the row's handler leaves the caret alone. Rendered rows keep today's Cmd+click-only rule, and Cmd+click in a revealed row still follows from anywhere in the link's display range.

- [ ] **Step 3: Hand check:** click a link's text: the caret lands in it and `[text](url)` shows, nothing opens; click the url: it opens once and the caret stays; Cmd+click a link in an inactive paragraph: it opens as before.

- [ ] **Step 4: Check, commit** `git commit -m "Rendered view: a click edits a link and a click on its revealed address follows it"`

---

### Task 7: Image paragraphs show their definition while active

**Files:**
- Modify: `crates/arkos/src/md_view.rs` (`active_form`'s kinds in `viewer.rs` include an active `Paragraph` whose piece is `Image`, `Images`, `Media` or an `Html` block that is an `HtmlImage`; `reveal_row` draws the picture as the rendered row does and under it a `Display` of the whole block text with every byte `Verbatim`)

- [ ] **Step 1: Write the failing test:** `md_reveal::Display::verbatim(text)` returns a display whose text is the source and whose map is the identity; `viewer::active_form` for a paragraph that is one image and for an `<img>` HTML block returns `Revealed`.

- [ ] **Step 2: Implement.** The picture element is `image_element`/`media_element`/the badge row as today (their Cmd+click still works); below it a text element from the verbatim display in the paragraph font and `ctx.dim`, with the caret, selection and press mapping of Task 3. For an `HtmlImage` block (`BlockKind::Html`) the same, so the tag shows under its image.

- [ ] **Step 3: Hand check:** click an image in a README: the picture stays and `![alt](path)` appears under it with the caret; edit the alt text; click away: the line goes. A badge row the same with the whole row's source.

- [ ] **Step 4: Check, commit** `git commit -m "Rendered view: an image paragraph shows its definition under the picture while the caret is in it"`

---

### Task 8: Docs, the keyboard tables and the Status rows

**Files:**
- Modify: `docs/guide.md:91-103` (the second paragraph of "Editing markdown"), `docs/guide.md:356` (the Arrows row: "Up and Down move by displayed row in a rendered block") and a new row "Delete | at the start of a list item or a quote line in a rendered block: removes its marker"
- Modify: `crates/arkos/src/keymap.rs:574` (`MARKDOWN_KEYS` gains `("Remove a list marker or quote mark (at the text start)", "Delete")` and `("Follow a revealed link's address", "click")`; adjust the keymap tests that count or list these rows)
- Modify: `docs/development.md` Status section (the markdown rows near line 545): the rows of the spec's section 11
- Modify: `crates/arkos/src/md_view.rs:1-24` (the module comment's second paragraph: the active block stays rendered and reveals the element under the caret; code, tables, HTML and front matter are raw)

- [ ] **Step 1: Write the guide paragraph:**

> In Rendered mode one block is active at a time: the one holding the caret. A paragraph, heading, list or quote stays as it reads while you edit it; the markdown element under the caret shows its syntax (the `**` around a bold word, a heading's `#`, a link's `[text](url)`) and the rest of the block does not. Bullets, task boxes and quote bars never show their markers, and Backspace at the start of an item removes its bullet. Type the syntax and it takes effect as you close it: `**x**` is bold the moment the second `*` lands. Click a link's text to edit it and its revealed address to follow it, or Cmd+click anywhere in it. Code blocks, tables, HTML and front matter still turn into their source when the caret enters them. Click another block, or press the arrows past the end of this one, and the active block moves; Up and Down move by displayed row. When the pane loses focus the active block renders again.

- [ ] **Step 2: Status rows** (append under the markdown rows): click a bold word in a README: the paragraph stays in the document font and `**` appears around that word only; press Right past the closing `**`: it hides; click a heading: its `##` shows, click elsewhere: it hides; click a bullet item: the bullet stays and the caret lands at the text; Backspace there: the bullet goes and Undo brings it back; type `**x**` in a paragraph: `x` is bold as the second `*` lands; type `- ` at a line start: a bullet appears; click a link's text: `[text](url)` shows and a plain click on the url opens it once; a long wrapped paragraph: Up and Down move by displayed row and Down from the last row reaches the next block; click an image: its `![alt](path)` shows under the picture; with the find bar open the hit's block shows the hit highlighted in the document font; a code block still turns raw; a 5 MB file with the caret in a paragraph: typing keeps up; Reduce Motion changes nothing.

- [ ] **Step 3:** `nice -n 15 cargo test -p arkos keymap`, then the whole crate: `nice -n 15 cargo test -p arkos 2>&1 | tail -5`, then `nice -n 15 cargo check --workspace --all-targets` with no warnings.

- [ ] **Step 4: Commit** `git commit -m "Docs: inline reveal in the guide, the keyboard tables and the Status rows"`

---

### Task 9: The screen pass and the merge

- [ ] **Step 1:** Walk every Status row of Task 8 in the sandbox on `docs/guide.md`, the Mark Sharp exploration doc (long paragraphs, tables, nested lists) and a README with badges, in Navy and in a light theme; judge the marker shade and weight against Mark Sharp's GIF frames (`marksharp-media/crops` in the session scratchpad, or the Marketplace page) and adjust `display_runs` if the markers need the dim colour (spec section 12).
- [ ] **Step 2:** The 5 MB gate: `docs/guide.md` concatenated to 5 MB, the caret in a paragraph near the end, typing keeps up and the frame after a keystroke stays under 8 ms (the existing gate's method in `docs/development.md`).
- [ ] **Step 3:** `git log --oneline main..reveal`, then merge `reveal` into `main` with a merge commit, and remove the worktree (`git worktree remove .claude/worktrees/reveal`).
