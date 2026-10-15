# Arkos IDE: markdown editing (milestone 7)

Date: 2026-10-15. Status: approved by Cyrus on 2026-10-15 ("Everything else looks good") with the toggle moved into the pane, section 9. Builds on the read-only viewer (milestone 4a,
`crates/arkos/src/viewer.rs`, `viewer_element.rs`, `highlight.rs`) and the sidebar watcher
(`explorer_watch.rs`). Written against main at `8f8de3e` (0.2.17).

## 1. Why this exists

Cyrus edits markdown beside Claude Code sessions and wants to do it without leaving Arkos. The
model is the MarkSharp extension for VS Code: you type into the rendered document, the file on
disk stays plain markdown, and the raw syntax shows only where you are typing. MarkSharp's
problem was speed; this one must be native and fast on any document size.

Rulings taken in chat on 2026-10-15:

- Save model: autosave about a second after the last edit, plus Cmd+S to save at once.
- Fidelity: Tier A (paragraphs, headings, emphasis, inline code, lists, code blocks, quotes,
  rules, links) plus task-list checkboxes; tables render read-only and are edited raw. Tier B
  (editable tables, images, media) follows in a later milestone once the core is proven.
- Size: no cap on the rendered view. Virtualised layout and block-level parsing make the cost
  per keystroke the size of one block, not the file. The viewer's existing refusal to read
  files over 64 MB stays, since that is about reading the file at all.
- Conversion: MarkSharp's. The block holding the caret shows its raw markdown; every other
  block renders; a block converts when the caret leaves it.
- Order: markdown first; editing other file types (the code editor) is a later milestone and
  is not in scope, but the editing core here must not rule it out.

## 2. Scope

| # | Item | Where |
|---|---|---|
| 1 | A text buffer with caret, selection, edits, undo and redo, independent of markdown | new `edit.rs` |
| 2 | Markdown blocks: splitting the buffer into top-level blocks and mapping offsets both ways | new `md_blocks.rs` |
| 3 | The rendered view: one virtualised row per block, the active block raw | new `md_view.rs`, `viewer.rs` |
| 4 | Keys and text input through gpui's input handler (IME, dead keys) | `md_view.rs`, `keymap.rs` |
| 5 | Saving: autosave, Cmd+S, atomic write, the watcher told | `viewer.rs`, `explorer_watch.rs` |
| 6 | External changes while editing: reload or ask | `viewer.rs` |
| 7 | The per-tab Rendered or Raw toggle, a pill at the top right of the pane; mode persisted | `pane_view.rs`, `tree.rs`, `workspace.rs` |
| 8 | Links, images, task checkboxes in the rendered view | `md_view.rs` |
| 9 | Find in both modes | `viewer.rs` |
| 10 | README and keybinding table | `README.md`, `keymap.rs`, `config.rs` |

Out of scope: editing any file that is not markdown (the viewer stays read-only for them);
inline conversion while typing within the active block (Typora style); table editing, image
insertion, media; spell check; collaborative or remote files; a markdown preview for files that
are not on disk.

## 3. Which files, and the two modes

A viewer tab whose path ends in `.md`, `.markdown` or `.mdx` is a markdown tab. It opens in
Rendered mode. Each markdown tab has its own mode, kept in the layout file:
`Tab::Viewer { path, #[serde(default)] mode: ViewMode }` with `ViewMode::{Rendered, Raw}`;
older files load as Rendered.

Raw mode shows the whole document as one editable raw block: monospace in the terminal font,
syntect's Markdown grammar for colour, the existing gutter with line numbers and diff marks.
Rendered mode shows the document as blocks (section 5). Both modes edit the same buffer, so the
mode is a view choice and never loses an edit. The find bar, Cmd+F, works in both.

A non-markdown viewer tab is unchanged: read-only, no toggle, no caret.

## 4. The editing core (`edit.rs`)

A plain text model, with no knowledge of markdown, so the later code editor can reuse it.

- `Buffer { lines: Vec<String> }` with positions as `(line, char)` in Unicode scalar values and
  grapheme-aware movement (`unicode-segmentation`, already a dependency). Line endings are
  `\n` in memory; the file's original ending (`\n` or `\r\n`) and whether it ended with a
  newline are remembered and restored on save.
- `Caret { head: Pos, anchor: Pos }`: a selection when they differ. One caret.
- Edits: insert text (newlines allowed), delete backward or forward by grapheme, by word, to
  line start or end, delete the selection, replace a range. Every edit returns the inverse for
  undo.
- Undo and redo: unlimited within the session; consecutive typed characters coalesce into one
  step until a pause of 700 ms, a newline, or a caret move; a paste or a delete-word is its own
  step. Redo clears on the next edit.
- Movement: by grapheme, by word (Option), to line start or end (Cmd+Left, Cmd+Right), to the
  document start or end (Cmd+Up, Cmd+Down), by line (Up, Down keep the target column as a
  display column), by page. Shift extends the selection from `anchor`. Cmd+A selects all.
- Clipboard: Cmd+C copies the selection as plain text; Cmd+X cuts; Cmd+V pastes, normalising
  `\r\n` to `\n`. These are the system clipboard (`cx.write_to_clipboard`,
  `cx.read_from_clipboard`), so text moves to and from the terminal and other apps.
- Markdown helpers live in `md_blocks.rs`, not here: list continuation on Enter and task
  toggling are edits built on the buffer's primitives.

The buffer is the single source of truth; the block structure (section 5) is derived from it.

## 5. Blocks and the rendered view (`md_blocks.rs`, `md_view.rs`)

### 5.1 Blocks

`pulldown-cmark` (a new dependency; the CommonMark parser Rust projects use, with tables, task
lists and strikethrough enabled) parses the buffer and yields events with source byte ranges.
`md_blocks::split` turns the top-level events into `Vec<Block>`, each a source line range and a
kind: paragraph, heading (level), list (the whole list, nested items included), code block
(fence info string), block quote, thematic rule, table, html block, blank run. Blank lines
between blocks belong to the block before them, so every line is in exactly one block and the
blocks concatenate back to the buffer.

Re-parsing: on every edit the document is re-split. The plan carries a benchmark test: a 1 MB
document (about 20 000 lines) must split in under 4 ms on this machine; if it does not, the
split becomes incremental (from the start of the edited block to the first boundary after it
that the old and new structures agree on) before this milestone ships. The inline parse of a
block (emphasis, links, code) happens only when that block is rendered, and the result is
cached per block until its source changes.

### 5.2 The view

`md_view` renders Rendered mode as one gpui `list()` whose items are the blocks, so only the
blocks on screen are laid out, whatever the document's size. Scrolling is the list's own.
Each inactive block renders:

- Paragraph: the UI font (the one the title bar uses) at the viewer's text size times 1.1,
  line height 1.5; bold, italic, strikethrough, inline code (terminal font, `tab_inactive`
  background), links (accent colour, underlined; Cmd+click opens the URL, or the file for a
  relative path, in the system's default app).
- Heading: the UI font, sizes 1.9, 1.5, 1.25, 1.1, 1.0, 1.0 times the paragraph size, semibold,
  a little space above.
- List: bullets ("•" for unordered, the number and a period for ordered) in the gutter column,
  nested lists indented by two bullet widths; a task item shows a checkbox, clickable: a click
  flips `[ ]` and `[x]` in the source, which is an undoable edit.
- Code block: the terminal font, `tab_inactive` background, 8 px padding, syntect colour by the
  fence's info string when a grammar matches, else plain.
- Block quote: a 3 px bar in `border` at the left, text dimmed.
- Thematic rule: a 1 px line in `border`.
- Table: the source lines in the terminal font, as a code block without colour (read-only
  rendering; editing happens by making it the active block, where it is raw anyway).
- HTML block: its source, dimmed.
- Image (`![alt](path)` alone in a paragraph): `img()` with the path resolved against the file's
  directory, at most the view's width, else the alt text in a dashed box.

The active block (the one holding the caret) renders raw: its source lines in the terminal
font with syntect's Markdown colour, the caret, the selection, and the same width as the
rendered blocks so nothing jumps sideways. When the caret leaves the block (a click elsewhere,
an arrow past its first or last line), the block re-renders and the new one goes raw. Arrow
Down from the last line of the active block puts the caret at the start of the next block;
Arrow Up from the first line, at the end of the previous one.

A click on a rendered block places the caret in the source: the rendered runs carry their
source offsets (pulldown-cmark gives them), so a click in a paragraph lands on the character
clicked; a click on a part with no source text (a bullet, the bar of a quote, an image) lands
at the block's first character. A click below the last block appends an empty paragraph.

### 5.3 Typing helpers

- Enter in a list item inserts a newline and the item's marker (next number for ordered lists,
  the same bullet otherwise, `- [ ]` for a task item), at the item's indent. Enter on an empty
  item removes the marker and ends the list.
- Tab inside a list item indents the item by two spaces; Shift+Tab removes two. Elsewhere Tab
  inserts four spaces (markdown has no use for a tab character).
- Typing three backticks at a line start and Enter opens a fence; the matching closing fence is
  added below the caret only when none follows in the block.
- Nothing else is "smart": no auto-pairing, no conversion while typing within a block.

## 6. Keys and text input

Text reaches the editor through gpui's `EntityInputHandler` (`window.handle_input` with the
viewer's focus handle): that is what gives IME composition, dead keys and marked text, which
`on_key_down` cannot. Non-text keys (arrows, Delete, Enter, Tab, the Cmd and Option chords of
section 4) go through `on_key_down` on the viewer, as the terminal does. Keymap actions, all in
the `Viewer` context: "toggle markdown view" (`cmd-shift-m`), "save file" (`cmd-s`). Cmd+Z,
Shift+Cmd+Z, Cmd+A, Cmd+C, Cmd+X and Cmd+V are handled in the editor's key handler, not the
keymap, as the terminal's Cmd+C is. The find bar's keys are unchanged.

The terminal's `Cmd+K` ("clear to start") and other `Terminal`-context bindings do not fire in
a viewer; existing `Viewer`-context bindings (`cmd-shift-d`) keep working.

## 7. Saving

- Dirty: the tab's label gets a trailing " •" (the mark glyphs are unchanged) from the first
  edit until the save lands.
- Autosave: 1 s after the last edit, on the background executor. Cmd+S saves at once. A tab
  closed with unsaved edits saves first; quitting waits for pending saves as it waits for the
  layout save today (`flush_for_quit`).
- The write is atomic: a temp file beside the target, the target's permissions copied, then
  `rename`, as the layout file is written. When the path is a symlink the file it points to is
  written in place instead (a rename would replace the link). The original line ending and the
  trailing-newline state are restored.
- The watcher: the viewer records the fingerprint (`config_watch::fingerprint`) right after its
  own write, so the sidebar watcher's next pass sees nothing new and does not reload. The diff
  against HEAD is recomputed after each save as it is after a reload today.
- A save error (permission, disk full) shows the viewer's banner with the error and "Retry";
  the edits stay in the buffer and the tab stays dirty.

## 8. External changes

The watcher reloads a viewer whose fingerprint changed. For a markdown tab:

- No unsaved edits: reload; the caret keeps its line number (clamped) and the active block is
  the one now holding that line; undo history is kept and applies to the new text only when
  the old text is unchanged, else it is cleared.
- Unsaved edits: no reload. A banner says "The file changed on disk" with "Reload" (drop the
  edits, load the file) and "Keep mine" (the next autosave writes the buffer over the file).
  Until one is chosen, autosave is paused for that tab; Cmd+S means "Keep mine".

## 9. The toggle

Inside the pane, not in the tab bar: a pill floating at the top right of the active markdown
tab's content, the same shape and style as the terminal's "jump to bottom" pill at the bottom
right (`absolute`, 10 px from the top edge and 14 px from the right, `px(10) py(4)`,
`rounded(10)`, `banner` background, `text` colour, the UI family at 12 px, pointer cursor; a
mouse-down on it focuses the pane and stops there, as the jump pill's does). Its text names
the view a click switches to: "show raw" while the tab renders, "show rendered" while it shows
raw. It is drawn for markdown tabs only, over both modes, and never while the find bar is
open in that corner's way (the find bar sits at the top right too, so the pill moves below it
while the bar is open). `cmd-shift-m` ("toggle markdown view", `Viewer` context) does the same.
Switching saves the layout; other markdown tabs keep their modes. The bar's slim form (a
browser tab in front) is unaffected, since a browser tab is never a markdown tab.

## 10. Find

Cmd+F opens the existing find bar over the buffer's lines. In Raw mode hits are highlighted as
today. In Rendered mode the block holding the current hit renders raw while the find bar is
open (so the hit is visible in the source) and the view scrolls to it; Enter and Shift+Enter
step through hits. Closing the find bar returns to the caret's block.

## 11. Performance

- Typing in a 1 MB document: the frame after a keystroke under 8 ms on this machine (split
  under 4 ms, the active block's inline parse under 1 ms, the list lays out only the visible
  blocks).
- Opening a 5 MB markdown file: first screen rendered within 150 ms of the load landing.
- No per-frame work while idle: the list re-lays out only blocks whose source changed.

## 12. Tests

- `edit.rs`: every edit and movement on a fixture with multi-byte and combining characters;
  undo coalescing (typing, pause, newline, caret move); redo cleared on edit; line-ending
  round trip (`\r\n` and missing trailing newline).
- `md_blocks.rs`: split on a fixture covering every block kind, blank-line ownership, the
  concatenation identity, offset mapping both ways, list continuation and task toggle edits,
  the 1 MB benchmark.
- Saving: atomic write with permissions, the symlink case (temp dir), fingerprint recorded,
  an unwritable target leaves the file intact and reports the error.
- External change: the two states and what each button does, on the pure state machine.
- Mode persistence: `Tab::Viewer` round trip, an old file loads as Rendered.
- Keymap: the two new actions, their names, counts and the README table.
- UI (rendered blocks, caret placement by click, the toggle) by hand checks listed in the
  README Status section.

## 13. README

Status rows: open a README in Arkos, it renders; click a paragraph, it turns raw with the
caret where you clicked; type, wait a second, `git diff` shows the change; press Enter in a list,
the next bullet appears; tick a task checkbox, the file shows `[x]`; the "show raw" pill at the top right of the pane, or Cmd+Shift+M, shows the raw file and
back; edit the file in another editor while a line is half typed, the banner offers
Reload and Keep mine; a 5 MB markdown file opens and scrolls without stutter; Reduce Motion
changes nothing here. Keyboard table rows for the two actions and the editor's keys. A section
"Editing markdown" explaining the two modes, autosave, and that other files stay read-only for
now.

## 14. Decisions left to the plan

- Whether a whole nested list as one block is acceptable for v1, or each top-level item should
  be its own block (better for long lists; more offset mapping). Default: whole list.
- The exact widths and type scale, tuned at the screen on the first rendered build.
- Whether `.mdx` counts (its JSX would render as paragraphs). Default: yes, as plain markdown.
