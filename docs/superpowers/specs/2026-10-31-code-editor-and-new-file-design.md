# Arkos IDE: the code editor and New File

Date: 2026-10-31. Status: asked for by Cyrus ("start the work so I can start editing files now",
and "create new files by right-clicking directories or files in the EXPLORER pane and selecting a
new New File option"). Builds on the markdown editor (`2026-10-15-markdown-editing-design.md`) and
the read-only viewer (spec 6). Written against main at `8a3d3a1` (0.8.3).

## 1. Why this exists

Cyrus edits beside Claude Code sessions. Markdown has been editable since 0.3; every other file
has stayed a read-only viewer, so a one-line fix to a `.rs` or `.toml` meant another editor. The
markdown editor's core (`edit::Document`, the raw view, saving, conflicts, recovery) was built so
the code editor could reuse it; this milestone turns it on for every text file, and lets a file be
created from the tree.

## 2. Scope

| # | Item | Where |
|---|---|---|
| 1 | Any text file the user may write opens as an editable document in the raw view | `viewer.rs` (`read_file`, `apply`) |
| 2 | Syntax colours kept while typing: an incremental syntect highlight, from the edited line until the grammar's state agrees again | new `highlight::CodeHighlight`, `viewer.rs` (`changed`) |
| 3 | The raw view paints a code document from its spans, a markdown one from `raw_marks` as before | `viewer_element.rs` |
| 4 | Code keys: Enter keeps the line's indent, Tab types the file's indent unit; no markdown helpers (links on paste, the slash menu, Cmd+B and Cmd+I, the format rows of the menu) | `viewer.rs`, `pane_view.rs`, `md_menu.rs` |
| 5 | "New File" in the tree's right-click menu, on a file or a folder, and on the tree's empty space; an inline prompt for the name; the file opens for editing | `sidebar.rs`, `main.rs` |
| 6 | Guide, Status rows, keybinding table | `docs/guide.md`, `docs/development.md` |

Out of scope: New Folder, Rename and Delete in the tree (a later round); multiple carets;
language features (completion, diagnostics, go to definition); wrapping in the code view; soft
tabs that follow an `.editorconfig`.

## 3. Which files become documents

A viewer tab that is not markdown opens as an editable document when the file is read as text
(`classify` says `Text`), is strict UTF-8 with one kind of line ending, is under the highlight
limit (`[viewer] highlight_limit_bytes`, 5 MB by default), has no line over 8 KB, and the user
may write it (`access(W_OK)`). Otherwise it is shown as today: plain or coloured, read-only, with
the existing banner for a file over the limit or with a long line, and the markdown editor's
banner ("not UTF-8: shown read-only", "mixed line endings", "read-only file") for the rest.

An empty file is a document of one empty line. A file the grammars do not claim (no extension,
an unknown one) is a document too, coloured in the text colour: colours are not what makes a
file editable.

Markdown is unchanged: Rendered and Raw modes, the pill, the markdown keys.

## 4. The code editor

The raw view (`ViewerElement` painting a document: the caret, the selection, the find hits, the
sideways scroll, the input handler for the IME) is the code editor. There is no mode pill and no
`Markdown` key context: Cmd+B toggles the sidebar as everywhere else, Cmd+S saves, Shift+Cmd+D
shows the inline diff.

### 4.1 Colours while typing

`read_file` builds, on the background thread, a `CodeHighlight` beside the document: the grammar
`syntax_for` picked, syntect's `ParseState` and `HighlightState` at the end of every line, and
the spans. It lives in `Loaded.code` and then in `Viewer.code`; the spans stay in
`Content::Text.spans` as for a read-only file.

After an edit (`Viewer::changed`), the old and new lines are compared for their common prefix and
suffix (the lines before and after the edit), and `CodeHighlight::update` highlights again from
the first changed line: each line's spans and end states are made anew, and once past the inserted
lines it stops at the first line whose end state equals the state that line had before (the
usual case: the next line). An edit that changes the state for the rest of the file (opening a
block comment or a string) runs to its end. The work is bounded per pass (`REHIGHLIGHT_BUDGET`
lines) so a keystroke never stalls the frame; the rest is finished in a follow-up on the
foreground executor, with the lines not yet reached shown in their old colours until then. A
typical keystroke recolours one or two lines in microseconds; the CPU stays with the GPU rule of
the effects (`effects-gpu-first`): state once, frames only while something changes.

A theme change, or the grammars landing after the first load, asks every code viewer to load
again (`reload_all_viewers` now includes code documents). For a code document that is a
recolour, not a read (`Root::recolour_viewer`): the lines it shows are coloured on the background
thread with the highlighter as it is now, and the result lands only if they are still the lines
shown (else it is done again), so the text, caret, undo history and dirty state are untouched. A
forced load that finds a document where there was none when it started (the grammars landed
during the first read) recolours the same way instead of being dropped as an unchanged file.

The input method's field, the active block in markdown, is in code the selection's lines whole
(the caret's line alone with nothing selected), so an offset into it never counts the file.

### 4.2 Keys

`key_action` is the same table. What changes by file kind, decided by `is_markdown(path)`:

- Enter: in code, a line break followed by the current line's leading whitespace, so a block
  keeps its indent; markdown's list continuation and fence rule do not apply.
- Tab: in code, the file's indent unit (`indent_unit`: a tab when the file indents with tabs,
  else the smallest indent step the file uses, 2 to 8 spaces, 4 when it has none); Shift+Tab
  does nothing. Markdown's list nesting does not apply.
- Paste: plain text, normalised to `\n`. No link from a pasted address, no image file.
- The slash menu never opens; Cmd+B and Cmd+I are not the editor's.
- The right-click menu has Cut, Copy and Paste only.

Everything else (movement, selection, delete by word and line, undo and redo, Cmd+A, the find
bar, Cmd+S, the autosave a second after the last edit, the conflict banner, the recovery file on
a failed quit) is the markdown editor's and unchanged.

## 5. New File

### 5.1 The menu

The tree's right-click menu gains "New File" as its first row, on a folder (the file goes in it)
and on a file (the file goes beside it). A right-click on the Explorer's empty space, under the
last row, opens the menu for the project folder: "New File", "Reveal in Finder", "Copy path".

### 5.2 The prompt

Choosing it opens an inline row in the tree where the file will land: under the folder's row at
its children's depth (the folder expands if it was collapsed), or at the top of the tree for the
project folder. The row is a `LineEdit` in the terminal font with a file icon's width of indent,
empty, with the keys. The sidebar's selection moves to the folder.

- Enter creates the file: the name is trimmed; empty, absolute, with an empty, `.` or `..`
  part, or with a NUL or a `:` is refused with the notice "a file name"; a name with `/` makes
  the folders on the way (`create_dir_all`) and then the file (`File::create_new`); an existing
  path is refused with the notice "already exists", and an I/O error with its message. The
  notices are the bell's, since the tree's rows are one height and have no room under them. The
  row stays open on a refusal with the text kept.
- On success the prompt closes, the folders on the way expand, the tree reads them again at
  once (`invalidate` and `load_pending_dirs`), the new file is selected and opened pinned in the
  focused pane (`open_viewer(path, true)`), and the keys go to the pane so typing starts in the
  file.
- Escape closes the prompt and keeps the tree's keys. A press anywhere else closes it too (the
  root's mouse-down listener, as a menu's); the row itself stops its press.
- Cmd+V pastes the clipboard's text into the name, newlines dropped.

The prompt is a `Transient<NewFile>` on `Root` (`new_file`), with its own focus handle
(`new_file_focus`) that the focus routing prefers while the tree holds the keys and the prompt is
open. Its row fades in and out with the Motion rule (ENTER and EXIT), lifted a few points like the
worktree prompt.

## 6. Tests

Headless, written before the code:

- `highlight`: a Rust snippet's incremental update after an insert, a delete and a paste of
  several lines equals a full highlight; an edit opening a block comment recolours to the end;
  an edit inside one line stops at the next line; the budget leaves a `pending` line the next
  pass finishes.
- `viewer::read_file`: a `.rs` file gets a document and coloured spans; one with a `.weird`
  extension a document and plain spans; a file over the limit no document and `Plain::TooBig`;
  a non-UTF-8 `.rs` no document and `ReadOnlyWhy::NotUtf8`; an empty file a document of one line.
- `viewer::line_splice`: the common prefix, then the common suffix, of two line lists.
- `viewer`: `code_newline` keeps the indent; `indent_unit` on tab-indented, two-space,
  four-space and unindented files; Tab in code types the unit.
- `viewer_element::code_runs`: spans map onto a line with a tab expanded to four columns.
- `sidebar::context_menu_items`: "New File" first, aimed at the folder for a folder and the
  parent for a file; the root menu's three rows.
- `sidebar::new_file_target`: the name rules.
- `sidebar::NewFile::key`: Enter with a bad name is refused, Escape cancels, a good name asks
  to create; `create_file` makes the folders and refuses an existing path; `prompt_row` places
  the row.
- `md_menu::menu_rows`: a code document's menu is Cut, Copy, Paste.

## 7. Docs

`docs/guide.md`: "Sidebar and viewer" (the menu's New File, the prompt), a new "Editing code"
section after "Editing markdown" replacing "Every other file type stays a read-only viewer", the
keyboard rows that say "in a markdown file" now say "in a document". `docs/development.md`:
Status rows for the screen checks (typing in a `.rs` recolours at once; a `/*` typed near the top
of a long file recolours the rest within a frame or two; New File on a folder, a file and the
empty space; Escape and a click away cancel; the theme switch keeps the undo history).
