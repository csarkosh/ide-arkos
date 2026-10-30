# Arkos IDE: the slash menu in the markdown editor (milestone A3)

Date: 2026-10-30. Status: written under the standing ruling of the Mark Sharp programme
(`2026-10-30-marksharp-ux-exploration.md`, section 9); Cyrus asked to start A3 on 2026-10-30.
Builds on A1 (inline reveal) and A2 (typing rules), both merged at `24126c3`.

## 1. What it is

Typing `/` where a word could start (at a line's start or after a space) in the rendered view
opens a menu under the caret that inserts markdown, as Mark Sharp's does (exploration section 3.1):

- With nothing typed after the slash, the top level, in Mark Sharp's order: Bulleted List,
  Numbered List, Check List, Heading (a submenu, Heading 1 to 6, hint "/h{1,6} to set level"),
  Table (hint "/mxn to set size"), Code Block, Quote Block (a submenu: Note, Tip, Important,
  Warning, Caution), Divider, Mermaid Diagram (a submenu of eighteen templates), Image, Footnote,
  HTML Block (a submenu: Basic Div, Center Aligned Div, Resized Image), Equation Block.
- With text after the slash, a flat list of every entry, submenu entries included under their full
  names ("Heading 2", "Mermaid Diagram - Pie"), whose name or keywords hold the text, case aside
  (`/ul`, `/ol`, `/todo`, `/hr`, `/math`, `/citation`, `/picture`, `/h2` all land). `/3x4` offers
  "3x4 Table"; `/3` offers 3x1 to 3x5 tables (sizes up to 10). No match shows "No Results".
- Up and Down move the selection, Enter or Tab runs it, Right opens a submenu and Left closes it,
  Escape closes the menu and leaves the typed text. A click runs an entry; hovering selects it.
  A space, the caret leaving the slash's line or moving before the slash, or the slash deleted
  closes the menu.

## 2. What each entry inserts

The `/` and the text after it are replaced. Every edit is one undo step.

- Line formats, applied to the line the slash is on (its text kept): Bulleted List `- `, Numbered
  List `1. `, Check List `- [ ] ` (on a list item they convert the item's marker, which is Mark
  Sharp's list type conversion), Heading N `#`×N then a space (replacing any heading hashes).
- Blocks, put on the slash's line when the rest of it is empty, else in a new paragraph after it,
  always with a blank line before and after them when the lines around are not blank (a `---` under
  a paragraph would make it a heading, and a table cannot interrupt one):
  - Table m×n: a header of n cells (`Column 1` …), its delimiter row, and m-1 empty rows; the caret
    in the first header cell. The plain Table entry is 2×2.
  - Code Block: a fence with the caret inside. Equation Block: `$$` lines with the caret between.
  - Quote Block: `> ` and the caret; an alert: `> [!NOTE]` (TIP, IMPORTANT, WARNING, CAUTION) and a
    `> ` line for the caret.
  - Divider: `---`, then the caret in a new paragraph after it.
  - Mermaid Diagram: a `mermaid` fence with the caret inside; a template entry fills it with a
    small valid diagram of its kind (flowchart, sequence, class, state, entity relationship, user
    journey, gantt, pie, quadrant, requirement, git graph, mindmap, timeline, XY chart, block,
    packet, kanban, architecture) and puts the caret after the block, so it renders at once.
  - HTML Block: `<div>` lines with the caret inside; centred, `<div align="center">`; Resized
    Image, `<img src="" width="300">` with the caret in the `src`.
- Image: the system's file picker for an image; the chosen file goes in as `![name](path)`, the path
  relative to the document's folder when it is inside it. Cancelled, nothing changes.
- Footnote: `[^n]` at the slash (n one more than the document's highest footnote number), and a
  `[^n]: ` definition at the document's end, where the caret goes for its text. Footnotes, alerts
  and equations render in milestones A8 and A9; until then they show as their text.

## 3. The caret on a new empty line (an A1 fix)

Blank lines after a block belong to it, and the revealed block hid them, so a caret on one (after
Enter, before typing) was drawn at the end of the text above. When the caret is on a blank line
after the block's last text, the revealed block draws an empty row below its text with the caret on
it, and no other caret; a press on that row lands on that line. The slash menu anchors to this row.

## 4. Where it lives

- `md_slash.rs` (new, pure): the catalogue (names, keywords, hints, submenus), the filter, the
  table sizes, the mermaid templates, and each entry's edit as a function of the slash's line and
  its neighbours.
- `viewer.rs`: the menu's state (where the slash is, the query, the selection, an open submenu),
  opening on a typed `/`, closing by the rules of section 1, the keys while it is open, running an
  entry, the image picker.
- `slash_menu.rs` (new): the menu drawn over the window under the caret (a second panel for an open
  submenu), with the chrome's menu frame, rows with a hint column and a chevron for submenus, at most
  ten rows shown with the selection kept in view; it fades on the motion constants and joins
  `Root::any_motion`.
- `md_reveal.rs`, `md_view.rs`: section 3.

Out of scope: icons in the rows (text rows, as the rest of Arkos's menus), the code block's language
picker (A5), rendering footnotes, alerts and math (A8, A9), a slash menu in Raw mode (Raw is the
plain editor).

## 5. Tests and the screen pass

Headless: the catalogue order and submenus; the filter (`/h2`, `/ul`, `/todo`, `/pie`, `/3x4`,
`/3`, a miss); every entry's edit on an empty line, after text, on a list item (conversion), with
the blank lines around; the footnote number; the open and close rules; the empty-line caret. On the
local shoot: `/` on an empty line shows the menu under the caret; `/h2` Enter makes a heading; Down,
Right into Mermaid, a template; `/3x4`; `/todo` on a list item converts it; Escape leaves `/`.
