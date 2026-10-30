# Arkos IDE: inline reveal in the rendered markdown view (milestone A1)

Date: 2026-10-30. Status: approved by Cyrus on 2026-10-30 ("looks good"). The first milestone of the Mark Sharp
programme (`2026-10-30-marksharp-ux-exploration.md`, section 7), under its rulings (section 9):
do what Mark Sharp does today. Builds on the markdown editing design
(`2026-10-15-markdown-editing-design.md`, shipped in 0.3.0) and Tier B
(`2026-10-16-markdown-rendering-tier-b-design.md`). Written against main at `1a775f6` (0.7.19).

## 1. Why this exists

In Rendered mode the block holding the caret turns into its source in the terminal font. Mark
Sharp never does that: the block stays rendered in the document's font, and only the markdown
element under the caret shows its syntax. The caret in a bold word shows the `**` around that word
and nothing else in the paragraph changes; the caret anywhere on a heading shows its `##`; a link
shows `[text](url)` when the caret is in it; a bullet stays a bullet and a quote's bar stays a
bar. "Format tags appear when you need them and hide when you don't." This milestone makes
Arkos's active block behave that way, keeping the text buffer as the single source of truth.

A consequence worth naming up front: because the active block is re-parsed on every keystroke
already, conversion while typing falls out of this design. Type `**x**` and the word is bold the
moment the second `*` lands, its markers staying visible until the caret leaves; type `- ` at a
line start and a bullet appears; type `# ` and the line is a heading with its hashes showing. The
"typing rules" milestone (A2) is left with the key behaviours (Enter, Backspace, Tab, Shift+Enter,
Cmd+B, Cmd+I, paste a URL over a selection, the context menu), not conversion.

## 2. Scope

| # | Item | Where |
|---|---|---|
| 1 | The inline element tree of a block with every marker's source range, hidden or revealable | `md_blocks.rs` |
| 2 | The display of an active block: which bytes show, what they show as, and the mapping between source offsets and display bytes, both ways | new `md_reveal.rs` |
| 3 | The revealed block element: the rendered text with markers, the caret, the selection, marked text, find hits; a press and a drag through the display mapping | `md_view.rs` |
| 4 | The caret over hidden markers: snapping on moves, Backspace at an item's or a quote line's text start | `md_reveal.rs`, `viewer.rs` |
| 5 | Up, Down, Cmd+Left, Cmd+Right by display row in the active block | `md_view.rs`, `viewer.rs` |
| 6 | A link's revealed destination follows on a plain click | `md_view.rs` |
| 7 | An image paragraph's definition under the picture while the caret is in it | `md_view.rs` |
| 8 | The input method's bounds, the caret reveal on scroll, the find bar's current hit, all through the revealed layout | `viewer.rs` |
| 9 | Docs: the guide's "Editing markdown", the Status rows, the keyboard table | `docs/guide.md`, `docs/development.md` |

In scope for reveal: paragraphs, headings, list items (nested lists included), block quotes. Out
of scope here, keeping today's raw active form until their own milestones: code blocks (A5, with
the code chrome), tables (A6), HTML blocks, front matter, `<details>` sections and rules. Also
out: every key rule of A2, the slash menu, folding, footnotes, math, presentation mode. No new
config key and no setting (ruling 2): the rendered font while editing is the one behaviour.

## 3. The model: markers and text

`pulldown-cmark`'s offset iterator gives every `Start` tag the source range of the whole element,
delimiters included (`**bold**`, `[text](url)`, `## Heading`, `- item`, `> quoted`), and every
text, code, break and HTML event its own range. From these `md_blocks` builds, beside the runs it
builds today, an element tree for the block:

- `Element { kind, span: Range<usize>, children: Vec<Node> }`, `Node` being an element or a leaf
  (`Text`, `Code`, `SoftBreak`, `HardBreak`, `Html`, `Image`), each with its source range.
- A marker is any byte of an element's span that no child covers. So `**` and `**` are the
  markers of a strong element, `[` and `](url)` of a link, `## ` of a heading, `- ` or `3. ` or
  `- [x] ` of an item, `> ` of each line of a quote, and the indent of a nested item or an item's
  continuation line.
- Markers are of two classes. Revealable: those of headings and of every inline element
  (emphasis, strong, strikethrough, code span, link, autolink, reference link, inline image,
  footnote reference once footnotes exist). Hidden: those of items and quotes, and item indents
  (ruling 1: a bullet stays a bullet, a bar stays a bar). The hard break's `\` or two spaces is
  revealable, as its own element around the newline.
- A code span's backticks and the matching closing run are its markers; its inner range is the
  code as today (`Event::Code` excludes nothing, so the ticks are counted off as now).
- A link's destination within its closing marker (`](` to `)`, or `<`..`>` for an autolink) is
  recorded as `dest: Range<usize>` on the element, so the view can make it clickable (section 6).

This tree is built in the same pass as the runs and cached with them in `Rendered` per block, so
it costs nothing on blocks that do not change. The existing `Run::source` ranges are unchanged;
`inline`'s callers see no difference.

## 4. The display of an active block (`md_reveal.rs`)

Pure functions over one block's source, its element tree and runs, and the set of "focus
offsets" (the caret's byte offset in the block; both ends when there is a selection; a find hit's
two ends when the find bar's current hit is in this block). They produce a `Display`:

- `segments: Vec<Segment>` in source order, each `{ source: Range<usize>, shown: Shown, style }`
  where `Shown` is `Verbatim` (the source bytes themselves), `Rendered(String)` (a run's text as
  the parser gives it: a soft break as one space, an entity or an escape decoded), or `Hidden`
  (zero display width).
- `text: String`, the concatenation of what shows, and `runs: Vec<TextRun>`-shaped styling
  for it, in the same form `styled_runs` makes today (so `StyledText::with_runs` draws it).
- `display_of(source: usize) -> usize` and `source_of(display: usize) -> usize`, monotone, from
  the segment table: linear inside a `Verbatim` segment, clamped inside a `Rendered` one, the
  segment's end for a `Hidden` one.

The rules that fill it:

1. The revealed set is, for each focus offset, the outermost inline element whose span contains
   the offset, inclusive at both ends, and the heading element when the block is a heading (so
   the hashes show with the caret anywhere on the heading, ruling 1). Inclusive ends mean the
   markers stay while the caret sits just after `**bold**`, and hide as soon as a character is
   typed past them. Nested elements inside a revealed one are revealed with it, since the outer
   markers are what make the inner text bold too.
2. Within a revealed element every byte shows verbatim, markers included, styled as the text it
   belongs to: the `**` of a strong element are bold, the backticks of a code span are in the
   terminal font on the code background, the `[` and `](url)` of a link are in the link colour,
   the `## ` of a heading at the heading's size and weight. The marker colour is the text colour,
   as Mark Sharp draws it; a dimmer shade is a tuning to try at the screen, not a setting.
3. A text run that holds a focus offset but sits in no element shows verbatim as well, so the
   caret is always in a `Verbatim` segment and its mapping is exact. The cost is that an escape
   (`\*`) or an entity (`&amp;`) in that run shows as written while the caret is in the run.
4. Everything else shows as the rendered view shows it today: runs by their text and style,
   revealable markers of other elements hidden, soft breaks as a space.
5. Hidden markers never show, whatever the focus. A focus offset inside a hidden marker is drawn
   at the marker's end.

For a `List` block the display is per item: only the item holding a focus offset gets segments;
the other items draw as today. For a `Quote` the whole quote is one text element as today, with
its `>` markers hidden per line. A `Heading` or `Paragraph` is one text element.

An image paragraph (`Inline::Image`, `Images`) keeps its picture, or its row of badges, and when
it holds a focus offset draws its source line verbatim under it in the paragraph font, where the
caret and the selection live (Mark Sharp 1.7.0: the definition shown inline under the image). The
same applies to an `HtmlImage` block, since it is a paragraph to the eye: its tag shows under it.

## 5. The revealed block element (`md_view.rs`)

`block_element` today builds a rendered row; `raw_row` builds the raw one. A third form,
`reveal_row`, draws an active block of a revealable kind:

- The row has the rendered row's padding, gap, font and size, so nothing moves sideways when a
  block becomes active; its height may change by a row when the markers push a wrap, as in Mark
  Sharp.
- The text is a `StyledText` from the `Display`'s text and runs, wrapped to the row's width. The
  selection, the current find hit and the other hits are painted as run backgrounds
  (`TextRun::background_color`, the theme's `find_match` and `find_current` as today's raw block
  uses them), the marked text of a composition as an underline, so no second layout is needed.
- The caret is a 2 px bar in `chrome.text` at `position_for_index(display_of(caret))` of the
  text's layout, painted by a canvas over the text after it is laid out (or by an element of the
  row's own, as `RawBlockElement` is; the plan picks). It is drawn only while the viewer has the
  focus, as the raw caret is.
- The layout is recorded on the viewer each frame as the raw blocks' are (`Viewer.raw_blocks`),
  as `RevealLayout { block index, lines, TextLayout, display mapping }`, so the caret reveal, the
  input method's bounds and the key handling of section 7 can read it.
- A press maps through the layout: the display byte under the point, `source_of` it, then
  `pos_of_offset` as today; Shift extends the selection; a drag moves the head the same way. A
  press on the row's padding lands on the nearest row's end as the raw block's does. The press
  path of a rendered row (`caret_at`) is unchanged; the `Parts` list gains a `Part::Reveal`
  carrying the mapping.
- Task boxes in a revealed list keep toggling on click; the revealed item's box too.
- Blocks the selection runs through but does not end in are drawn revealed with the selection
  painted and nothing else revealed.

Every colour is read from the theme at paint time, as the motion rule requires; nothing here
animates, so Reduce Motion changes nothing and `Root::any_motion` is untouched.

## 6. Links

In a revealed link the destination's bytes (`Element.dest`) are an `InteractiveText` click range:
a plain click on them follows the link (`follow_link`, as Cmd+click does), a plain click on the
link's text puts the caret there as on any text. Cmd+click follows the link from anywhere in it,
revealed or not, as today. This is Mark Sharp's two-click rule (0.0.9) and removes the trap where
a click to edit a link opened it.

## 7. Keys in a revealed block

- Left, Right, Option+Left, Option+Right: after the move, a caret inside a hidden marker goes on
  in the move's direction to the first visible offset (`md_reveal::snap`), so the caret is never
  seen standing still over a bullet. Cmd+Left and Cmd+Right go to the start and end of the
  caret's display row, and the start of a row that begins with a hidden marker is the first
  visible offset after it.
- Up and Down move by display row within the revealed block, keeping the caret's x as the goal
  (the layout's `position_for_index` and `index_for_position`); from the first or last display
  row they cross to the previous or next block as `navigate` does today. Shift extends.
- Backspace with the caret at the first visible offset of a list item removes the item's marker
  (and its task box), and at the first visible offset of a quote line removes that line's `> `,
  each one undoable edit (`md_reveal::unmark`). The line re-parses as text, which is Mark Sharp's
  "Backspace at the beginning of a list item deletes the list item". Backspace elsewhere is the
  grapheme delete it is today, so a heading's `#` can be deleted like any character and the line
  stops being a heading, as in Mark Sharp.
- Typing, Enter, Tab, Shift+Tab, Escape, Undo, Redo, Select All, Copy, Cut, Paste, Cmd+S are
  unchanged. Escape still conceals the active block until the caret is placed again.

The raw-block forms of these keys apply unchanged to the kinds that stay raw.

## 8. The viewer's plumbing

- `renders_raw` becomes a three-way `active_form(ix) -> Rendered | Revealed | Raw`: `Revealed`
  for an active block of kind `Paragraph`, `Heading`, `List` or `Quote` (and an image paragraph),
  `Raw` for the other active kinds, `Rendered` otherwise. The find bar's current hit makes its
  block active in the same way, with the hit's ends as the focus offsets.
- `reveal_in_list` takes the caret's y from the revealed layout when the active block has one,
  else from the raw layout as today.
- `bounds_for_range` (the input method's candidate window) maps the range through `display_of`
  and the revealed layout; `character_index_for_point` through `source_of`.
- The block cache keeps the element tree with the runs; the `Display` itself is made per frame
  for the active block only (one block's segments, microseconds), never cached across edits.
- Telemetry: no new feature; the existing markdown counters are unchanged.

## 9. Performance

The element tree is one more vector per cached block, built in the pass that builds the runs. The
`Display` is linear in the active block's source and is made once per frame for one block. The
5 MB gate of milestone 7 (typing keeps up, the frame after a keystroke under 8 ms) holds; the
plan carries a test that a 20 000 line document's active paragraph reveals in under 1 ms.

## 10. Tests

Headless, named as sentences, written first:

- `md_blocks`: the element tree of a fixture with every inline kind (strong, emphasis, nested
  strong in emphasis, strikethrough, code span with doubled ticks, link with a title, reference
  link, autolink, inline image, hard break by backslash and by spaces, inline HTML, an entity, an
  escape); markers are exactly the bytes no child covers; a heading's prefix and closing hashes
  are its markers; an item's marker, task box and indent, and each quote line's `>`, are hidden.
- `md_reveal`: the display of a paragraph with the caret outside every element (nothing
  revealed, soft break a space); inside `**bold**` (the markers show, bold); just after its
  closing marker (still revealed); one character further (hidden); inside the italic inside a
  strong (both revealed); in a link (the `[`, the `](url)`, and `dest` points at the url); in a
  plain run holding `\*` and `&amp;` (shown as written); a heading with the caret at its end
  (`## ` shows); an item with the caret at its text start (the marker hidden, the caret at the
  marker's end); a selection across two elements (both revealed, nothing between); `display_of`
  and `source_of` are monotone and inverse on every `Verbatim` byte.
- `md_reveal::snap`: Left from an item's first visible offset lands on the previous line's end,
  not inside the marker; Right from a line end steps over the next item's marker; Cmd+Left on a
  quote line lands after its `> `.
- `md_reveal::unmark`: Backspace at an item's text start removes `- `, `1. `, `- [ ] ` and keeps
  the text; at a quote line's text start removes `> `; elsewhere returns `None`; the edit undoes
  in one step.
- `viewer`: `active_form` per kind; the find hit's block is `Revealed` with the hit's ends as
  focus; Up and Down by display row cross blocks at the edges (with a fixed-width fake layout);
  the input method's range maps through the display.
- UI, by hand (section 11).

## 11. Docs

`docs/guide.md`, "Editing markdown", second paragraph becomes: in Rendered mode the block holding
the caret stays rendered; the markdown element under the caret shows its syntax (the `**` around a
bold word, a heading's `#`, a link's `[text](url)`) and the rest of the block does not; bullets and
quote bars never show their markers; click a link's text to edit it and its revealed address to
follow it, or Cmd+click anywhere in it; Backspace at the start of an item removes its bullet.
Code blocks, tables, HTML and front matter still turn into their source when the caret enters
them. The keyboard table gains "Backspace at the start of a list item or quote line: removes its
marker" and notes that Up and Down move by displayed row in a rendered block.

`docs/development.md` Status rows: click a bold word in a README: the paragraph stays in the
document font and `**` appears around that word only; press Right past the closing `**`: it hides;
click a heading: its `##` shows, click elsewhere: it hides; click a bullet item: the bullet stays
and the caret lands at the text; Backspace there: the bullet goes; type `**x**` in a paragraph:
`x` is bold as the second `*` lands; click a link's text: `[text](url)` shows and a plain click on
the url opens it; a long wrapped paragraph: Up and Down move by displayed row; a 5 MB file with
the caret in a paragraph: typing keeps up; with the find bar open the hit's block shows its
hit highlighted in the document font; a code block still turns raw; Reduce Motion changes nothing.

## 12. Decisions left to the plan

- Whether the caret is painted by a canvas over the `StyledText` or by an element of the row's
  own; the canvas is less code, the element is what the raw block does.
- The exact treatment of a soft break's range when it carries trailing spaces (one display space
  for the whole range, mapping the caret at either end of it).
- Whether a `List` block's other items can keep their cached text elements across frames while
  only the caret's item is rebuilt, or the block's row is rebuilt whole (fine for lists of
  ordinary length; measured for a 2 000 item list).
- The marker shade and whether markers take the weight of their text, judged at the screen
  against Mark Sharp.
