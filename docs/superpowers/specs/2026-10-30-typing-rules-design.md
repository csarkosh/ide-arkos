# Arkos IDE: typing rules in the markdown editor (milestone A2)

Date: 2026-10-30. Status: written under the standing ruling of the Mark Sharp programme
(`2026-10-30-marksharp-ux-exploration.md`, section 9: do what Mark Sharp does today, keep none of
its weaknesses); Cyrus asked to start A2 on 2026-10-30. Builds on A1, the inline reveal
(`2026-10-30-inline-reveal-design.md`, merged at `3225710`).

## 1. Why this exists

A1 made the rendered view read like Mark Sharp while you edit. A2 makes it type like Mark Sharp:
the keys that shape a document (Enter, Shift+Enter, Tab, Shift+Tab, Cmd+B, Cmd+I), the paste that
makes a link, and the menu on a right-click. The exploration's section 2.3 lists Mark Sharp's
rules; this design maps each to an edit of the markdown text, so the file stays exactly what was
typed, as in A1.

## 2. Scope

| # | Item | Where |
|---|---|---|
| 1 | Enter by block: a new paragraph in a paragraph or heading, a new paragraph inside a quote, the next item in a list (as today), on an empty nested item one level out | `viewer.rs` (`newline`), `md_blocks.rs` |
| 2 | Shift+Enter: a hard line break in a paragraph, heading or item; out of a quote; out of a code block from its last line | `viewer.rs` (`key_action`, a new `KeyAction::BreakLine`) |
| 3 | Tab and Shift+Tab in a list item by levels: one level deeper than the item above at most, back to the parent's column, from anywhere in the item | `viewer.rs` (`indent_item`) |
| 4 | Cmd+B and Cmd+I: bold and italic on the selection, the word under the caret, or the element the caret is in (toggled off) | `keymap.rs`, `config.rs`, `main.rs`, `md_format.rs` (new) |
| 5 | Paste a URL or a file path over a selection on one line: a link | `viewer.rs` (Paste) |
| 6 | The right-click menu in a markdown document: Cut, Copy, Paste, then Bold, Italic, Strikethrough, Code, Link | `md_menu.rs` (new), `main.rs`, `pane_view.rs` |
| 7 | Docs: the guide's editing section and keyboard table, the Keybindings window, the Status rows | `docs/guide.md`, `docs/development.md`, `keymap.rs` |

Every rule applies in Rendered mode. Raw mode keeps today's plain editor, except that Cmd+B, Cmd+I,
the paste-as-link and the menu work there too, since they edit text the same way in both.

Out of scope, with the reason: Cmd+K for a link (Arkos uses Cmd+K as a chord prefix, and Mark
Sharp has no such key); Highlight in the menu (GitHub markdown has no syntax for it); the selection
toolbar Mark Sharp shows on a drag (A2 has the right-click menu; the toolbar is judged at the
screen first, see section 10); slash commands (A3); list type conversion (A3, through the slash
menu); Up at the document's top before a leading code block (Arkos's raw code block already puts
the caret on the fence line, where Enter adds a line above).

## 3. Enter

In Rendered mode, by the kind of the block holding the caret (with nothing selected; a selection
is replaced by the break, as typing replaces it):

- Paragraph or heading: a new paragraph, so a blank line goes in: `\n\n` at the caret. At a
  heading's end the next paragraph is plain text, never a heading. In the middle the block splits
  in two, the text after the caret starting the new paragraph. A heading split in its middle keeps
  its `#` on the first part only.
- List: as today, the next item with the same marker (the next number, `- [ ]` for a task), and
  on an empty item: out of the list when the item is top-level (as today), one level out when it
  is nested (new: the item's indent goes back to its parent's column, the marker stays).
- Quote: a new paragraph inside the quote, `\n>\n> ` at the caret (one `>` per nesting level the
  line has), so the text after the caret starts a new paragraph in the same quote. Enter on an
  empty quote line (only its `>`) ends the quote: the `>` goes and the line is a plain paragraph.
- Code block, table, HTML, front matter (the raw blocks): a plain line break, as today, and the
  fence rule (three backticks then Enter close themselves) stays.

## 4. Shift+Enter

A new action: `key_action` gives `KeyAction::BreakLine` for Shift+Enter (today it is Enter).

- Paragraph, heading or list item: a hard line break, the backslash form Mark Sharp writes for new
  breaks: `\` then a line break, then the continuation indent (an item's content column), so the
  text after the caret goes on the next line of the same paragraph or item.
- Quote: out of the quote. On its last line, a new plain paragraph after the quote (`\n\n` after
  the line's end, the caret there). In the middle, the quote splits there: the rest of the line and
  the lines after stay quoted, a plain empty paragraph between, the caret in it.
- Code block, on its last code line: out of the block: a new paragraph after the closing fence, the
  caret there. Elsewhere in a code block, a plain line break, as Enter.
- Raw mode and the other raw blocks: a plain line break.

## 5. Tab and Shift+Tab in a list item

Today Tab inserts two spaces before the item's marker and Shift+Tab removes two. Markdown nests an
item under the one above only when its marker sits within that item's content column (2 for `- `,
3 for `1. `, 4 for `10. `), and Mark Sharp allows one level deeper than the previous sibling at
most. So:

- Tab on an item line, or on a continuation line of an item (the caret anywhere in the item): the
  item's marker moves to the content column of the nearest item above it with a smaller or equal
  indent, when that is deeper than where it is; otherwise nothing happens (the item is already one
  level under the one above, or it is the list's first item). The item's continuation lines move
  with it by the same amount.
- Shift+Tab, anywhere in the item: the marker moves back to the column of the item it is nested in
  (the nearest item above with a smaller indent); a top-level item does not move. Continuation lines
  move with it.
- In a table block, Tab and Shift+Tab keep moving between cells (Tier B), first.

## 6. Cmd+B and Cmd+I

Two keymap actions, "bold" (`cmd-b`) and "italic" (`cmd-i`), in a new key context `Markdown` that
the viewer pane adds to `Viewer` for a markdown document only, so in a code viewer and everywhere
else Cmd+B still toggles the sidebar. Both rebindable and listed in the Keybindings window and the
palette. A new pure module `md_format` computes the edit; the delimiter is `**` for bold and `_`
for italic (Mark Sharp's guide types `_` for italics), `~~` for strikethrough and `` ` `` for code
(the menu's two other formats).

- A selection on one line: when it is exactly the text of an element of that format, or exactly
  the element with its delimiters, the delimiters go (toggled off); otherwise the selection is
  wrapped, and stays selected inside the delimiters. Spaces at the selection's ends stay outside
  the delimiters, since `** word**` is not bold in markdown.
- A selection over more than one line: nothing happens (Mark Sharp's rule).
- No selection, the caret in an element of that format: the element's delimiters go.
- No selection, the caret in a word: the word is wrapped, the caret keeps its place in the word.
- No selection, the caret between words: the empty pair goes in (`****`, `__`) with the caret
  between, so what is typed next is in the format.

Each is one undoable edit. In Rendered mode the element under the caret reveals as A1 draws it, so
the markers are seen as they go in.

## 7. Paste as a link

Cmd+V with a selection on one line and a clipboard holding one line that is a web address (`http`,
`https`, `mailto`) or a path to a file or folder that exists (absolute, `~`, or relative to the
document's folder): the selection becomes `[selection](target)`, the target in `<...>` when it holds
a space or a parenthesis. A path inside the document's folder is written relative to it. One
undoable edit; the caret goes after the link. Anything else pastes as today.

## 8. The right-click menu

A right-click (or Control+click) in a markdown document opens a menu at the press, drawn and
dismissed like the terminal's (`link_menu.rs`): Cut, Copy, Paste, a line, Bold, Italic,
Strikethrough, Code, Link. A press outside the current selection first moves the caret there, as a
left press does, so the formats act on the word clicked. Cut and Copy need a selection and are
dimmed without one. Link wraps the selection (or the word under the caret) as `[text]()` with the
caret between the parentheses, or `[text](address)` when the clipboard holds an address as in
section 7. Each row is the same edit as its key; the menu fades in and out on the motion constants
(ENTER, EXIT) like the terminal's, and joins `Root::any_motion` through the same `Transient`.

## 9. Tests

Headless, named as sentences, written first: Enter by block (paragraph middle and end, heading
end, quote middle, empty quote line, empty nested item, a code block keeps a plain break); Shift+
Enter (paragraph, item continuation indent, quote last line and middle, code block last line);
Tab and Shift+Tab levels (under `- `, under `1. `, a second Tab refused, continuation lines move,
Shift+Tab from a continuation line, a top-level item stays); `md_format` (wrap a word, wrap a
selection with spaces at its ends, toggle off by selection and by caret, an empty pair, a
multi-line selection refused, each format's delimiter); paste as a link (address, relative path,
path with a space, multi-line clipboard refused, no selection refused); the menu's rows and which
are dimmed; the keymap's two new actions, their context and the Keybindings rows.

## 10. The screen pass

Run on the local shoot (A1's method): Enter in a paragraph and a heading, Shift+Enter in a
paragraph and in an item, Enter twice in a quote, Tab and Shift+Tab on a nested item under a
numbered item, Cmd+B on a word and on a selection, Cmd+I toggled off, a right-click on a word and
each format row, a URL pasted over a word. The selection toolbar is judged here: if the right-click
menu alone feels short of Mark Sharp's, a toolbar milestone follows.
