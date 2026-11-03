# Arkos IDE: tabular files as a grid (CSV, TSV and spreadsheets)

Date: 2026-11-03. Status: brainstormed with Cyrus on 2026-11-03; this is the written design,
awaiting their OK. Builds on the viewer (`viewer.rs`, `viewer_element.rs`), the markdown
tables (`md_table.rs`, `md_view.rs`; spec `2026-10-30-tables-design.md`) and the code editor
(spec `2026-10-31-code-editor-and-new-file-design.md`).

## 1. Why this exists

The data analyst's picture (`docs/images/data-analyst-dark.png`, story
`scripts/shoot/stories/data-analyst.toml`) has `data/signups.csv` in a pane 680 points wide and
430 high, beside Claude Code. The pane shows comma-separated text with line numbers, which no one
reads, while the report in the pane on the left shows the same numbers as a markdown table that
everyone reads. Cyrus, 2026-11-03: "The CSV is not legible for them at all. It should render
cleanly." And on what else it should do: "I just want rendering capabilities. If I ever need to
work with a CSV, I'd use Claude to edit it / make updates / present views."

So this is a viewer, not an editor. The analyst reads the data, looks something up, copies a
slice of it into a prompt, and asks Claude in the next pane to do the work. The files are the
ones people are handed: CSV, TSV and Excel workbooks, all three in the first cut (Cyrus:
"CSV/TSV and Excel together"). It has to read well in a small pane, since that is where it will
live.

## 2. Scope

Goals:

- A `.csv` or `.tsv` opens as a grid: aligned columns, the file's first row as a bold header
  that stays put while the rows scroll, zebra rows, row numbers down the left, figures
  right-aligned so a column of numbers lines up.
- A workbook (`.xlsx`, `.xlsm`, `.xlsb`, `.xls`, `.ods`) opens as the same grid, one sheet at a
  time, with a strip of sheet tabs when it has more than one.
- The grid follows the file: Claude rewrites it in the next pane and the grid updates, as the
  markdown render does today.
- The same Rendered and Raw modes markdown has, with the same pill: a CSV's raw mode is the code
  editor it opens in today. A workbook has no raw mode.
- Selection and copy: a cell, a dragged range, a column from its header, a row from its number,
  everything; Cmd+C copies tab-separated text that pastes cleanly into a prompt, Numbers or
  Sheets.
- Find (Cmd+F) over the cells.
- A 100,000-row export scrolls like a 30-row one: only the rows on screen are drawn.
- Works in a pane 400 points wide: nothing in the layout assumes a spreadsheet's width.

Non-goals, for this cut:

- Editing in the grid: no typing into cells, no sorting, filtering, formulas or column
  reordering. The raw mode edits a CSV as text; a workbook is read-only.
- Writing workbooks, or converting between formats.
- Charts, pivots, or any analysis. That is Claude's job in the next pane.
- Guessing whether a CSV has a header row. The first row is the header (section 3.2).
- A sheet picker for CSV, or multi-file views.

Anti-goals (what would make this a failure even if it works):

- A grid that is slower to open than the text it replaces, or that stalls the window on a big
  file. Parsing happens off the main thread, as loads do today; drawing is bounded by the pane.
- A grid that looks like a different app from the markdown table next to it. It takes the
  markdown table's colours, fonts and rules, so the analyst's two panes read as one IDE.
- Columns squeezed to illegibility to avoid a horizontal scroll. Readability wins; the grid
  scrolls sideways instead.

## 3. The model

### 3.1 A sheet

`grid.rs` (new, pure, tested like `md_table.rs`):

```rust
pub struct Sheet {
    pub name: String,            // the file's stem for CSV; the sheet's name for a workbook
    pub header: Vec<String>,     // as many as the widest row
    pub rows: Vec<Vec<String>>,  // every row as long as `header`
    pub kinds: Vec<ColumnKind>,  // one per column, from the body cells (3.4)
    pub first_row: usize,        // the number the first body row shows (3.5)
    pub truncated: Option<usize>,// rows dropped past the cap (3.6)
}
pub enum ColumnKind { Text, Number }
pub struct Book { pub sheets: Vec<Sheet>, pub source: Source }
pub enum Source { Delimited(Delimiter), Workbook }
```

A `Book` has at least one sheet. A CSV is a `Book` of one sheet; a workbook has one per visible
sheet, in the workbook's order (a hidden sheet is skipped; a workbook whose sheets are all hidden
or empty is one empty sheet named after the file).

### 3.2 Delimited text

`grid::parse(text: &str, delimiter: Delimiter) -> Sheet` reads RFC 4180: fields split on the
delimiter, a field in double quotes may hold the delimiter, doubled quotes (`""` is one `"`) and
line breaks; records end at `\n` or `\r\n`; a final line break adds no empty record. A quote in
the middle of an unquoted field is literal. A short record is padded with empty cells and a long
one widens the sheet: the column count is the widest record's, and a header shorter than that is
padded with empty names (shown blank). The first record is the header, always; a file of one
record is a header with no rows; an empty file is an empty sheet.

`Delimiter` is `Comma`, `Tab`, `Semicolon` or `Pipe`. `grid::sniff(text, path) -> Delimiter`:
`.tsv` is `Tab`; otherwise the four candidates are counted outside quotes over the first 20
records, and the one whose count is the same on every record and greater than zero wins; among
several such, the one with the most per record; with none, `Comma`. (Semicolon is what Excel
writes in locales where the comma is the decimal mark; the sniff is what makes those files open
right without a setting.) The parser is written here, not taken from the `csv` crate: it is
about eighty lines, pure, and the repository keeps its text handling in-tree and tested with
proptest (a round trip through a writer and the parser for any grid of random cells, including
cells with quotes, delimiters and line breaks).

Tabs are kept as tabs in cells: `split_lines` (the text view's) turns them into four spaces, and
the grid must parse the document's text before that, or the text as read. The builder makes sure
`Loaded.book` for a `.tsv` comes from the bytes, not from `Content::Text`'s lines.

**Encodings** (the builder check asked; this is the author's call). The grid decodes its bytes
itself, before the viewer's text path: a UTF-8 byte order mark is stripped (Excel's "CSV UTF-8"
writes one; it must never become part of the first header name); a UTF-16 byte order mark
decodes the file as UTF-16 through `encoding_rs`, which is already in the tree, so a UTF-16
export becomes a grid instead of the binary notice `read_capped`'s NUL sniff would give it (the
delimited read therefore runs before `read_capped`, as the workbook read does); anything else is
read as UTF-8 with lossy replacement, which is what the raw view shows. No guessing at
Windows-1252 or other legacy encodings: a wrong guess silently corrupts text, and a replacement
character is at least visible. Such a file's raw mode is what it is today: the plain read-only
text under the "not UTF-8" banner, no editor, the pill still shown.

### 3.3 Workbooks

`workbook.rs` (new): reads a file with `calamine` 0.38 (`open_workbook_auto`, which picks the
format by extension; MIT; pure Rust; its dependencies `zip`, `quick-xml`, `encoding_rs`,
`codepage`, `byteorder`, `log`, `serde` are small and `encoding_rs` is already in the tree) into
a `Book`. Its `dates` feature (which adds `chrono`) stays off; dates are converted here.

Each visible sheet's used range (`worksheet_range`) becomes a `Sheet`. The header is the
spreadsheet's column letters (`A`, `B`, ... `Z`, `AA`, ...) for the range's columns, since that
is what a spreadsheet's user expects, and the first row of the range is an ordinary row: a
workbook has a title row as often as a header row, and the letters are always right. Row numbers
are the spreadsheet's (`first_row` is the range's first row, 1-based), so "row 14" means the same
thing in Arkos and in Excel.

Cell text, from calamine's `Data`:

- `String`: as is. `Empty`: blank.
- `Int`: the integer. `Float`: an integer when it is one (`3.0` is `3`); otherwise up to ten
  significant digits with trailing zeros trimmed, never scientific notation under 1e15, the way
  Excel's General format reads. No thousands separators (the cell is copied into prompts and
  scripts).
- `Bool`: `TRUE` or `FALSE`.
- `DateTime`: converted from the serial by a pure function here, honouring the workbook's 1904
  date system when calamine reports it and the 1900 system's phantom leap day otherwise;
  `2026-10-26` when the time is midnight, else `2026-10-26 14:30`; a duration as `h:mm:ss`.
  `DateTimeIso` and `DurationIso`: the text as given.
- `Error`: the spreadsheet's error text (`#DIV/0!`, `#N/A`, ...).
- A formula cell shows its cached value, which is what `worksheet_range` returns; one without a
  cached value is blank. Merged cells keep their value in the top-left cell and blank elsewhere,
  as calamine gives them.

A workbook that fails to open (not a workbook, encrypted, corrupt) shows the viewer's unreadable
notice with calamine's message, and a load that panics in calamine must not take the app down:
the read runs under `catch_unwind` and reports a panic as an unreadable file.

### 3.4 Column kinds

A column is `Number` when every non-empty body cell parses as a number: an optional sign, digits
with optional thousands commas or a decimal point, an optional trailing `%`, an optional leading
currency sign (`$`, `€`, `£`), surrounding spaces trimmed. Otherwise `Text`. The header does not
count; an all-empty column is `Text`. In the view, a `Number` column is right-aligned (header
included), a `Text` one left, the way the delimiter row aligns a markdown table.

### 3.5 Row numbers

A CSV's rows are numbered from 2, the header being 1, so the number matches the raw view's line
number for the ordinary file with one record per line. A record with embedded line breaks puts
the two out of step; that is accepted (the raw view has no better answer). A workbook's rows show
the spreadsheet's numbers (3.3).

### 3.6 Limits

A delimited file is read within the viewer's existing limits: a file over `MAX_PLAIN_BYTES`
(64 MB) shows the existing too-large notice and never parses; one over the highlight limit
(5 MB) still becomes a grid (parsing is cheaper than highlighting). A sheet keeps at most
200,000 rows; the rest are dropped and `truncated` says how many, which the view shows in the
viewer's banner ("first 200,000 rows shown, 31,214 more in the file"). A workbook file over
64 MB shows the too-large notice unread.

### 3.7 Selection

`grid::Selection`, pure, tested:

```rust
pub struct Cell { pub row: usize, pub col: usize }  // row 0 is the header
pub struct Selection { pub anchor: Cell, pub head: Cell, pub scope: Scope }
pub enum Scope { Cells, Columns, Rows, All }
```

The rectangle is `anchor` to `head`, whichever way round. `Columns` spans every row of the
anchor's and head's columns, `Rows` every column of their rows, `All` the sheet. Methods: `rect()`
(the inclusive row and column ranges), `extend(to)` (moves `head`, keeps the scope),
`move_by(dr, dc, sheet)` (a new one-cell selection, clamped), `contains(cell)`.

`grid::copy_text(sheet, selection) -> String`: the rectangle's cells, tab-separated, one record
a line, `\n` endings, no trailing newline. A cell holding a tab, a line break or a double quote
is wrapped in double quotes with its quotes doubled, as Excel does when it copies; everything
else is verbatim. The header row is included when the rectangle includes row 0 (a column
selection from the header always does, which is what a prompt wants: the name with the values).

### 3.8 Find

`grid::find(sheet, needle) -> Vec<Cell>`: every cell whose text contains the needle, case
insensitive, in reading order (header first), at most `viewer::MAX_HITS`. The viewer's find bar
and its Enter and Shift+Enter stepping serve it (section 5).

## 4. Where it lives

`Loaded.book: Option<grid::Book>`, built on the background thread with the load. For a
delimited file (`grid::is_delimited(path)`: `.csv`, `.tsv`) it is parsed from the bytes beside
the document the load already builds; the file remains a text document with its highlight, so
raw mode is the code editor unchanged. For a workbook (`workbook::is_workbook(path)`) it is read
by `workbook.rs` before `classify` would call the zip binary; the content stays `Content::Binary`
and the element shows the book, never the binary notice, when `book` is `Some`.

`Viewer` keeps `book`, the active sheet index, the `Selection` (none to start), the grid's
scroll (rows down, points across; `ScrollAnimator` for the vertical as the text view uses) and
the find hits as cells. On `apply`, a new book replaces the old; the sheet index, the selection
and the scroll are kept and clamped, so a file Claude rewrites keeps the analyst's place.

Modes: `ViewMode::Rendered` shows the grid, `ViewMode::Raw` the code editor, through the same
`Tab::Viewer { mode }` the layout saves, and the same pill; `pill_label` reads "show raw" and
"show grid" for a delimited file. `toggle_markdown_view_for` and the "Toggle markdown view" key
apply to delimited files too (the action keeps its name). A workbook ignores the toggle and shows
no pill. `is_markdown` stays what it is; a new `viewer::renders(path)` (markdown or delimited)
is what the pill and the toggle ask, and the markdown-only parts (presentation mode, the
markdown keys, the slash menu, the outline) keep asking `is_markdown`.

Raw mode edits: switching from Raw to Rendered rebuilds the book from the document's text (on
the background thread, the pill showing the old grid until it lands); autosave and the watcher
then reload it from disk as any edit does. Nothing re-parses per keystroke.

A workbook is never a document: no save, no autosave, no dirty mark, no conflict dialog; the
watcher's reload replaces the book in place.

`grid_view.rs` (new) draws the grid; `viewer_element.rs` and `pane_view.rs` branch to it when
the viewer has a book and the mode is Rendered. Telemetry's `viewed` learns two formats
(`delimited`, `workbook`) added to `telemetry/events.toml`; a grid is reported with the existing
`rendered` mode and the code editor with `raw`, so `mode` gains no value. `docs/telemetry.md` is
regenerated from the events file as the development doc says, and the release notes get the
line every events change gets.

## 5. The view

**Layout.** A row-number gutter on the left (as wide as the largest number shown, in the gutter
colour and font the text view uses), the header row across the top, both pinned: the body scrolls
down and sideways beneath them and they stay. The top-left corner is a blank cell that selects
everything on click. Cells draw in the prose font at the paragraph size the markdown table uses,
with the markdown table's cell padding, rules between cells, header fill and zebra tint (through
`md_view::row_fill`, with the same ground and ink rules under an effect, and the grid reports
itself to the wash as a table does). Text is right-aligned in a `Number` column, left in `Text`.
Rows are one line high, always: a cell with a line break shows its first line and an ellipsis.

**Widths.** Each column's natural width is its widest cell over the header and the first 200
rows (`MEASURED_ROWS`), measured with the window's text system in the frame's font and size,
plus padding; the measure is cached by font and size as `md_view::Widths` is. A column is capped
at 45% of the grid's width (`COLUMN_CAP`) and floored at three characters. A cell wider than its
column is cut with an ellipsis; hovering it for a moment shows the full text in a tooltip, in the
banner colour over the border colour the pills use. When the columns exceed the grid's width,
the body scrolls sideways (Shift+wheel, a horizontal swipe, and the keys) under the pinned
gutter; columns never wrap, since wrapping would break the one-line rows that make 100,000 rows
cheap. `[viewer] wide_tables` does not apply.

**Virtualisation.** Only the rows in the pane are shaped and painted, from the scroll's first
row to the last that fits, plus one; a sheet of 200,000 rows costs what thirty do. The viewer's
`rows_visible` and `max_scroll` serve the grid as they serve the text, so Page Up and Down and
the scroll clamp work unchanged.

**Selection.** The selected rectangle is tinted with the markdown table's selection tint and the
head cell ringed in its ring colour. A click selects a cell; a drag, or Shift+click, extends to a
rectangle; a click on a header cell selects its column (Shift+click extends across columns), on
a row number its row, on the corner everything. Arrows move the one-cell selection, Shift+arrows
extend it, Cmd+arrows go to the sheet's edges, Home and End to the row's first and last column,
Page Up and Down scroll a page, Cmd+A selects all, Escape clears. The view scrolls to keep the
head cell in sight. Cmd+C copies `copy_text`; Cmd+X and Cmd+V do nothing (read-only). The
right-click menu offers Copy and Copy with header (the rectangle with row 0 added when it lacks
it), reusing `md_menu`'s look.

**Find.** Cmd+F opens the viewer's find bar as today; in grid mode the hits are cells, every hit
tinted with the find colour and the current one with the current-hit colour (the text view's
two), the view scrolled to the current one. Enter and Shift+Enter step. The count reads as today.
Closing the bar leaves the current hit selected.

**Sheet tabs.** A workbook with more than one sheet shows a strip under the grid with each
sheet's name as a tab, in the tab bar's colours and font at the small size; the active one
underlined with the accent, as pane tabs are. A click switches; Cmd+Option+] and Cmd+Option+[
go to the next and previous sheet (two new keymap actions, "next sheet" and "previous sheet", in
the viewer's section, bound in a key context the grid sets only while it shows a workbook with
more than one sheet, so no other tab sees them). Cmd+Shift+] and Cmd+Shift+[ stay the tab-cycle
keys everywhere, workbook tabs included. A strip wider than the pane scrolls sideways.
Each sheet keeps its own scroll and selection while the tab is open. A workbook of one sheet
shows no strip. The active sheet is not saved in the layout: a relaunch opens the first.

**Banners.** The viewer's existing banner shows "first 200,000 rows shown, N more in the file"
when a sheet is truncated, and for a workbook, "read-only: a spreadsheet" is not shown (there
is nothing to write; the tab simply has no editing).

**Small panes.** At 400 points wide the gutter, the header and at least two columns show; the
first column is never narrower than its floor; the pill sits where markdown's does; the sheet
strip stays one line and scrolls. Nothing is hidden at small widths; things scroll instead.

**Motion.** The scroll animates through `ScrollAnimator` as the text does. The tooltip and the
find tint appear instantly, nothing new animates, so `Root::any_motion` is unchanged. Should the
builder animate anything (a sheet switch, say), it uses the M3 tokens through `motion.rs`.

**Colour.** Every colour is one the markdown table or the text view already reads from the
theme at paint time; the grid adds no colour of its own, so the contrast matrix needs no new
pair. Should the builder need one (a tooltip's text, say), it goes through the matrix and
`arkos theme contrast` on all nine built-in themes, and never loosens a floor.

**Opening.** Nothing changes in how a file is opened: the sidebar, `arkos open`, the palette and
a markdown link open a `.csv` or `.xlsx` as a viewer tab as they do today; the tab now shows a
grid. A workbook's tab title is the file name, as any viewer tab's.

## 6. Tests

Headless, written first, named as sentences:

- `grid.rs`: parsing (quotes, doubled quotes, embedded line breaks, `\r\n`, a trailing line
  break, a quote mid-field, short and long records, the header padded, an empty file, one
  record); the sniff (a `.tsv`, a semicolon file, a comma file with semicolons inside quotes,
  a one-column file, ties); the proptest round trip; column kinds (currency, percent, thousands,
  a blank, a stray word); `Selection::rect`, `extend`, `move_by` clamped at the edges, each
  scope; `copy_text` (a cell, a rectangle, a column with its header, quoting of a tab, a line
  break and a quote); `find` ordering and its cap; the truncation count.
- `workbook.rs`: a fixture workbook read into a `Book`: sheet order, a hidden sheet skipped,
  column letters past `Z`, `first_row` from a range that starts on row 3, an integer float, a
  long float, a bool, a date at midnight and one with a time, a 1904-system date, a duration, an
  error cell, a formula's cached value, a merged cell, a corrupt file reported as unreadable.
  The fixtures (`crates/arkos/tests/fixtures/grid/*.xlsx`, `.xls`, `.ods`) are made by
  `scripts/make-grid-fixtures.py` from the standard library's `zipfile` and hand-written XML
  for the `.xlsx` and `.ods`; the `.xls` (a binary format no stdlib writes) is a small file
  checked in once with its origin recorded in the script's comment. The date conversion is
  tested alone against known serials (1, 59, 60, 61, 25569, 45956.5).
- `viewer.rs`: `renders` for each extension; a load of a `.csv` gives a document and a book;
  a `.tsv`'s tabs reach the cells; a UTF-8 BOM is not in the first header name; a UTF-16 file
  with a BOM becomes a grid; a Windows-1252 file becomes a grid with replacement characters and
  the "not UTF-8" read-only raw view; a workbook load gives `Content::Binary` with a book and no
  document; `apply` keeps and clamps the sheet index, selection and scroll; the raw-to-grid
  switch rebuilds the book from the edited text; the toggle is ignored for a workbook; the
  pill's label per mode and file.
- `grid_view.rs`: the pure layout: column widths from natural widths with the cap and floor;
  the row range from a scroll offset and a height; the cell under a point (gutter, header,
  corner, body, past the end); the key table (each key to its selection move); the truncation
  banner text.
- Keymap: the two new actions appear in the viewer's section and the settings' list.

The screen pass, on a throwaway local shoot try-story (not handed to Cyrus) in Material dark and
one light theme: `signups.csv` in a 680 by 430 pane reads as a grid with `visits` and `signups`
right-aligned; a drag, a column click and Cmd+C paste into a terminal as TSV; the raw pill and
back; a 150,000-row generated CSV scrolls smoothly and shows the truncation banner at 200,001;
a three-sheet `.xlsx` with its strip, a sheet switch, a date column reading as dates, a column
past `Z`; a 400-point-wide pane; the tooltip on a long cell; find stepping through hits; the
grid under the watercolor effect (the wash shows through the header and paper). Then the
data-analyst story's dark still is retaken on the cloud shoot (`gh workflow run shoot --ref
<branch> -f scenes="data-analyst"`) and replaces `docs/images/data-analyst-dark.png`; the hero
is recomposed with it.

## 7. Docs

- `docs/guide.md`, "Sidebar and viewer": a "Tables and spreadsheets" subsection: what opens as
  a grid, the header, the numbers, the pill, selection and copy, find, sheet tabs and their
  keys, the limits, and that editing is raw mode or Claude's.
- `docs/development.md`: the Status paragraph gains the grid; the checklist gains the rows a
  person must check (a workbook's dates against Excel's; the grid under each effect).
- `docs/telemetry.md`: the two formats.
- `docs/agent-guide.md`: `arkos open` on a `.csv` or `.xlsx` opens the grid (one line).
- `crates/arkos/Cargo.toml`: `calamine` with a comment saying why, as the other entries have.

## 8. Left to the builder

- The exact split of `grid_view.rs` between a gpui element and pure layout functions, as long
  as the layout is pure and tested.
- Whether the tooltip uses gpui's tooltip support or the viewer's own overlay.
- The sidebar icon for `.csv` and `.xlsx`, if `icons.rs` maps extensions (a small, optional
  nicety).
- How the right-click menu is wired (a `md_menu` variant or a small one of its own).
- Performance details: whether natural widths are measured on the main thread for 200 rows
  (as markdown does) or precomputed by character count off it. The measured number must be
  bounded by `MEASURED_ROWS` either way.
- The order of the plan's tasks, with the pure model first and the Excel reader last, so the
  CSV grid can be looked at on screen before the workbook work starts.
