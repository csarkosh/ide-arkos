# Arkos IDE: the Mark Sharp editing UX, explored

Date: 2026-10-30. Status: exploration with rulings; the design spec follows. Cyrus asked to
explore recreating the full Mark Sharp markdown reading and editing UX in Arkos's markdown
editor, and on 2026-10-30 ruled on the open questions: "go with what Mark Sharp does today"
(section 9). This document says what Mark Sharp actually does, where Arkos stands against it,
which pieces are the heart of the feel, and how the rest is reached without giving up what Arkos
already does better.

Sources: https://www.marksharp.co/, the docs repository (README, user guide, changelog 0.0.1 to
1.13.0, acknowledgements), the extension package 1.13.0 from the Marketplace (its `package.json`,
its stylesheet, the strings of its webview bundle; the source itself is not public), and the
feature-overview GIF frame by frame. Copies are in the session scratchpad under
`marksharp-src`, `marksharp-vsix` and `marksharp-media`.

## 1. What Mark Sharp is

A VS Code custom editor for `.md` and `.markdown` by Jonathan Yeung, version 1.13.0, about 34 000
installs, free with a 12 dollar perpetual "premium" tier. Technically it is a React webview around
Lexical (Meta's editor framework) with Fluent UI widgets, Prism for code colour, KaTeX for math
and Mermaid 11 for diagrams. The markdown file is imported into Lexical's tree when opened and
exported back to markdown on save. That one fact explains most of its character, good and bad:

- The document is always rendered. There is no raw mode of its own; "raw" is VS Code's editor,
  reached by `Mark Sharp: Switch Editor Mode` (Cmd+K Y), which keeps the caret position and the
  fold state across the switch, and tries to keep the same vertical screen position.
- The syntax is revealed per inline element, not per block. The caret inside a bold word shows
  the `**` around that word and nothing else in the paragraph changes. The caret in a heading
  shows its `##`. A click on a link places the caret in the link text and shows `[text](url)`; a
  second click on the now visible URL follows it. A footnote anchor shows `[^1]` when focused, an
  inline equation expands to its `$...$`, an image shows its markdown definition under the
  picture, and the frontmatter is a faint "Frontmatter" label that expands when clicked and hides
  when the caret leaves.
- The file's layout may change on save. The README says so in bold and recommends version control.
  Formatting settings (bullet marker, ordered delimiter and increment style, table alignment,
  hard line break style) default to "as typed", which keeps the imported style where it can.
- Edits from outside (an agent writing the file) are received live since 1.13.0, with a full
  re-render that the author admits flashes on tables and diagrams.

The GIF's own words for the feel: "Format tags appear when you need them and hide when you
don't."

## 2. The editing model in detail

### 2.1 Reveal and conceal

What is revealed depends on the selection's two ends only. Formatted text in the middle of a
selection stays rendered; the elements under the anchor and the focus are expanded. In
presentation mode nothing is ever expanded. Observed reveal rules, by element:

| Element | Caret inside it shows | Notes |
|---|---|---|
| Heading | `## ` prefix, in the heading's own size and weight | The hashes hide the moment the caret leaves; an empty `### ` stays selectable |
| Bold, italic, strikethrough, highlight, inline code | The delimiters around that one run | Nested formatting expands only the run the caret is in |
| Link | `[text](url)` with the url clickable | First click edits, second click (on the url) follows |
| Wikilink | `[[name\|alias]]` | Alias shown when not selected |
| Footnote anchor | `[^1]` | The key is editable and renames its reference; hover shows the note text |
| Inline equation | `$...$` source | Block equations the same with `$$` |
| Image | The `![alt](path)` line under the picture | Was a separate text box before 1.7.0 |
| Frontmatter | The YAML, expanded | A `title` field renders as an H1 and `created` as a date line under it |
| HTML block | Its source, on double click | Reverts to text when the edit no longer forms an HTML block |
| Code block | Never conceals; always source with chrome | See 3.4 |
| Table | Never conceals; a grid with a cell editor | See 3.5 |

### 2.2 Conversion while typing

Typing the syntax converts in place: `# ` to `###### ` at a line start, `**x**` and `_x_` as the
closing delimiter is typed, `- `, `* `, `1. `, `- [ ]`, `> `, three or more backticks or tildes
(the fence length is preserved on export), `---` or `***` for a rule, `|a|b|` then Space for a
table, `$$ ` for an equation block, `[^1]` with a matching `[^1]: ` line for a footnote,
`[!NOTE]` at the start of a quote for an alert. Adding a missing `]` to `[text(url)` completes
the link. Pasting a URL over a selection makes a link with the selection as the text (1.8.0), and
pasting a local file path does the same (1.12.0). Dollar signs only form an equation when the
characters next to them are not spaces, so "$1, $2" stays text.

### 2.3 Keys, by block

The list behaviour is explicitly modelled on Word. The rules, collected from the guide and the
changelog:

- Paragraph: Enter makes a new paragraph; Shift+Enter a hard line break inside it (exported as
  `\` or two spaces by the setting). Home and End go to the line's ends and never scroll.
- List: Enter makes the next item; Enter on an empty item outdents one level or leaves the list
  at the top level; Backspace at the start of an item deletes the item; Shift+Enter adds a line
  inside the item (so an item can hold a code block or a quote); Tab indents, but at most one
  level past the previous sibling, as markdown requires; Shift+Tab outdents from anywhere in the
  item; two adjacent lists of the same type merge; a slash command on an item converts the list's
  type; checkboxes toggle on click.
- Quote: Enter adds a line within the quote; Shift+Enter leaves it (or splits it when in the
  middle); Down on the last line makes a paragraph after it when none exists; Up on the first
  line of a quote at the document's top adds a line above.
- Code block: Enter is a newline; Shift+Enter on the last line leaves the block; Down on the last
  line leaves it too; a click on the block's border selects the whole block and Backspace or
  Delete removes it; a paste keeps its newlines.
- Document edges: Up at the very top no longer makes blank lines (1.9.0); instead Up on the first
  line of a leading code block, table or HTML block puts a caret in front of it, and Enter there
  adds a line above. A click below the last block puts the caret there, adding a paragraph when
  the document ends in a table, code block or diagram, but the file is not dirtied until typing.
- Formatting chords: Cmd+B and Cmd+I toggle bold and italic (nothing when the selection spans
  lines or an unformattable element). Cmd+F opens a find widget. Chord shortcuts of VS Code
  (Cmd+K ...) are swallowed so the trailing letter is not typed.
- Context menu: right click, or selecting text, shows a menu with Bold, Italic, Strikethrough,
  Highlight, Link, Copy, Cut, Paste (1.10.0).

### 2.4 Selection and clipboard

Select All works across tables and code blocks. Copying from a header and pasting does not make a
new header (1.8.2). Tables pasted from HTML become markdown tables. Copy and paste between Mark
Sharp, VS Code's editor and mail clients was reworked in 1.13.0. Per-tab caret position persists
when switching between Mark Sharp tabs.

## 3. The surface, area by area

### 3.1 Slash commands

Typing `/` at an empty place opens a menu under the caret; the filter is the text typed after the
slash, with keyword synonyms per entry, "No Results" when nothing matches, arrow keys and Enter
to pick, submenus on the right (a chevron) for Heading, Quote Block, Mermaid Diagram and HTML
Block, and a secondary hint column ("/h{1,6} to set level", "/mxn to set size"). The top level,
in order, as the bundle lists it:

Bulleted List, Numbered List, Check List, Heading (submenu Heading 1 to 6), Table (`/3x4` makes
a 3 by 4 table directly; `/3` offers 3x1 to 3x5), Code Block, Quote Block (submenu Note, Tip,
Important, Warning, Caution), Divider, Mermaid Diagram (submenu Flowchart, Sequence, Class,
State, Entity Relationship, User Journey, Gantt, Pie, Quadrant, Requirement, Git Graph, Mindmap,
Timeline, XY Chart, Block Diagram, Packet, Kanban, Architecture), Image, Footnote, HTML Block
(submenu Basic Div, Center Aligned Div, Resized Image), Equation Block.

Keywords, so `/ul`, `/ol`, `/todo`, `/hr`, `/math`, `/citation`, `/picture` all land.

### 3.2 Headings, outline and folding

- A chevron fades in (0.5 s) in a 28 px gutter to the left of a heading when the mouse is near
  it; a click folds the heading's children. Fold All and Unfold All commands exist. Folds sync
  with VS Code's editor on a mode switch.
- The "M# Outline" view in the Explorer lists the headings as a tree: click scrolls to the
  heading, hover shows a popup preview of the section, the heading in view is highlighted as you
  scroll, and sections with deeper children collapse.
- Draggable blocks (premium): a six-dot handle, 16 by 18 px, appears at the left of the block
  under the mouse; dragging it moves the block, with a 4 px blue target line.

### 3.3 Typography and layout

From the stylesheet: the system UI font stack, 14 px by default (6 to 100, two commands to step
it, independent of VS Code's zoom; a premium font family setting), line height 1.5, 28 px of
padding around the page plus the 20 px gutter, 80 vh of scroll past the end. H1 2.5 em, H2 1.8
em, H3 1.2 em, all weight 500. Quotes have a 4 px left bar, a tinted background, 16 px side
padding and 15 px text. Lists indent 32 px per level; a checkbox is 16 px square, a checked
item is struck through. A "letter" document width setting (1.11.0) centres a page-width column;
the default spans the pane. Footnotes sit in a sticky section at the bottom of the viewport when
they fit. Colours follow the VS Code theme in premium; the free tier has one light and one dark
theme. There is an RTL mode (status bar "LTR"/"RTL") that flips direction and alignment.

### 3.4 Code blocks

A header row with a language pill (a dropdown with "Search for a language..."), a Copy button at
the right that turns into a check mark for a moment, line numbers in a 52 px gutter (correct for
wrapped lines and multi-line highlight tokens), Menlo at 13 px, tab size 2, horizontal scroll.
Syntax colour for a limited set of languages (Prism). Undo inside code blocks, Home and End with
Shift, arrow keys across wrapped lines were all fixed in 1.12.x, which says how fiddly a code
block inside a WYSIWYG tree is.

### 3.5 Tables

Tables always render as a grid; editing them is premium. The cell model is a spreadsheet's, not a
text field's: typing into a selected cell replaces its text; double click or F2 enters the cell
with the caret after the text; Enter moves down a row and makes a new row from the last; Tab and
the arrows move between cells and out of the table at its edges; a click near the left edge
selects a row, near the top edge a column, the corner the whole table. Hovering the right or
bottom edge shows a "+" strip (a 0.2 s fade) that adds a column or row. A right-click menu
inserts and deletes rows and columns and sets the column alignment, which round-trips to the
delimiter row. Cells hold formatted text, links, wikilinks and images. Short rows are padded and
long rows truncated on import. On export, columns are aligned (or condensed by the setting),
with double-width characters counted properly. Wide tables wrap cell text and the table scrolls
sideways when it still does not fit.

### 3.6 Mermaid diagrams (premium)

A block with a header bar: a "Mermaid" pill, a display mode pill (Code, Diagram, Split; the
default is Diagram since 1.11.0 and the choice is saved per diagram), zoom and pan on the
rendered diagram, a Copy button and a link to the Mermaid docs page for the diagram's type.
Rendered in the webview, so it updates live in Split mode as the source is typed. Colours follow
the editor theme, or Mermaid's defaults by a setting, or a per-diagram frontmatter block. The
`:::mermaid` fence of Azure DevOps is accepted.

### 3.7 Images (premium for insertion)

Rendered inline, several on one line side by side when they fit, with their definition revealed
under the picture when focused. Insertion: paste from the clipboard, drag and drop (Shift must
be held before the drag starts, a VS Code webview limitation), the `/image` command or
`Mark Sharp: Insert Image` with a file picker. The file lands where VS Code's own
`markdown.copyFiles.destination` says, named `image-1.png`, `image-2.png` and so on.

### 3.8 Links, wikilinks, footnotes, alerts, math, frontmatter

- Links: `[text](url)`, raw `www.` and `http(s)://` URLs, relative file links, `#heading` anchors
  (lowercased, hyphenated) within and across files, angle-bracket destinations. A local link opens
  the file in VS Code and scrolls to the heading.
- Wikilinks: `[[file]]`, `[[file|alias]]`, `[[image.jpg]]`, resolved against the open workspace.
- Footnotes: `/footnote` inserts an auto-incremented anchor and its reference line at the end;
  the keys are sorted numerically, hover shows the note, breaking the anchor's pattern removes it.
- Alerts: `[!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, `[!CAUTION]` colour the quote and
  add an icon; text after the tag replaces the label.
- Math: KaTeX inline and block.
- Frontmatter: `---` on the first line starts it; collapsed to a faint label; `title` and
  `created` get a rendered header.

### 3.9 Presentation mode (premium) and the status bar

A read-only mode toggled from a status bar item ("M#: Edit Mode", turning yellow as
"M#: Presentation Mode") or a command: no reveal on click, single-click navigation on links and
footnotes, diagrams in Diagram mode, no table "+" strips, no drag handles, frontmatter hidden but
its title kept, HTML blocks rendered.

## 4. Where Arkos stands

Arkos's markdown editor (`md_view.rs`, `md_blocks.rs`, `md_table.rs`, `edit.rs`, `viewer.rs`,
about 12 000 lines with tests) was designed in October 2026 with Mark Sharp named as the model,
and it already has the parts Mark Sharp is weakest at: the text buffer is the single source of
truth, so a save never rewrites anything but what was typed; a 5 MB file opens and edits without
stutter; external edits reload without a flash; line endings and a BOM survive. What differs is
the unit of reveal. Arkos turns the whole block holding the caret into monospace source; Mark
Sharp keeps the block rendered and reveals the one element under the caret, in the rendered
font and size. The 2026-10-15 design ruled "inline conversion while typing within the active
block (Typora style)" out of scope on purpose, as a later step once the core was proven. The core
is proven. That ruling is the thing to revisit.

Feature by feature:

| Area | Mark Sharp | Arkos today | Gap |
|---|---|---|---|
| Reveal unit | The inline element under the caret, rendered font | The whole block, monospace | The heart of the difference |
| Conversion while typing | Live, per keystroke | None inside the active block (list continuation, fence close, Tab indent only) | Large |
| Raw view | VS Code's editor, Cmd+K Y, caret kept | Own Raw mode, Shift+Cmd+M, caret kept | Arkos is better here |
| Save model | Export on save, layout may change | Buffer written as is, autosave 1 s | Arkos is better |
| Lists | Word-like Enter, Backspace, Tab limit, merge, type conversion | Enter continues, empty Enter ends, Tab indents two spaces | Medium |
| Quotes, alerts | Nested blocks, Shift+Enter rules, five alert kinds with icons | A bar and dimmed text; alerts render as plain quotes | Medium |
| Code blocks | Language pill, line numbers, Copy, Shift+Enter exit, border select | Monospace box with syntect colour, the info line | Medium |
| Tables | Spreadsheet cell editing, "+" strips, context menu, alignment menu | Grid rendering, raw cell editing with Tab, Up, Down, re-align on leave | Medium to large |
| Mermaid | Code, Diagram, Split; zoom, pan; templates; live | Headless Chrome to PNG, cached, about two seconds, source while editing | Large (a different renderer) |
| Images | Inline reveal of the definition; paste, drag, picker | Local and remote images, `<img>`, badges, Cmd+click opens | Insertion missing |
| Video, audio | None | Poster, video pane, QuickTime | Arkos only |
| `<details>` | Not supported | Collapsible, state remembered | Arkos only |
| Links | Click edits, second click follows; wikilinks; `#anchors` | Cmd+click follows; link runs carry the target | Wikilinks and anchors missing |
| Footnotes | Auto-numbered, hover preview, sticky section | Rendered as text | Missing |
| Math | KaTeX inline and block | Excluded by ruling 6 of the Tier B design | Missing; back in scope (section 9) |
| Frontmatter | Faint label, expands on click, title and date header | Rendered as its source (`Metadata` block) | Medium |
| Slash commands | Full menu with submenus and keywords | None | Missing |
| Headings | Hover chevron folds, Fold All, outline view with scroll highlight and hover preview | None | Missing |
| Drag to reorder | Six-dot handle | None | Missing |
| Context menu, Cmd+B, Cmd+I | Yes | None in markdown (the terminal has a right-click menu) | Missing |
| Presentation mode | Read-only toggle | None (every rendered tab edits) | Missing, cheap |
| Typography | 14 px system font, 1.5, 28 px padding, letter width option, font size commands | The UI font at the viewer size, own scale | Close; letter width and size steps missing |
| Find | VS Code's find widget | Own find bar in both modes | Equal |
| RTL | Toggle | None | Low priority |
| External edits | Full re-render, flashes | Reload keeps the caret, no flash | Arkos is better |

## 5. What makes the feel, in order

If only part of this is built, this is the order in which it earns the "same UX" verdict:

1. Element-level reveal in the rendered font. Without it nothing else reads as Mark Sharp.
2. Conversion while typing (`**`, `_`, `` ` ``, `# `, `- `, `> `, fences, rules, tables), so the
   document is never "in source" while writing.
3. The list, quote and code block key rules of section 2.3, and Cmd+B, Cmd+I.
4. The slash menu.
5. Heading chevrons, folding and the outline.
6. Code block chrome and spreadsheet tables.
7. Image insertion by paste and drag.
8. Footnotes, alerts, wikilinks, frontmatter header, presentation mode.
9. Mermaid Split mode and math.

## 6. How element-level reveal can fit Arkos

Three ways to get there, with a recommendation.

### A. Keep the buffer, render the active block with markers spliced in (recommended)

Arkos's inline parse already gives every rendered run its source byte range inside the block
(`md_blocks::Run::source`), and a click already maps from a rendered run to a source position.
The missing step is the inverse: render the active block as rendered runs too, and for each
inline element whose source range contains the caret or a selection end, insert its delimiters as
dim runs in the terminal font (`**`, `_`, `` ` ``, `~~`, `[`, `](url)`, `![`, `#`) at their
source positions, so the caret has a place to stand on them and the user can type over them. The
caret's position is a source offset either way, which is the buffer's truth; only the layout of
the active block changes. A block whose source contains something the inline parser cannot place
(a raw HTML tag, a table, a code block, a reference definition) keeps today's monospace fallback,
so nothing is ever uneditable.

Conversion while typing is then free: the block is re-split and re-parsed per keystroke already
(the 4 ms budget of the editing design), so `**x**` renders bold the moment the second `*` lands
and the caret, being inside the element, keeps its markers visible until it leaves. Heading
reveal is the same rule applied to the `#` prefix: shown while the caret is on the heading's line,
in the heading's size.

What this keeps: the on-disk text is never normalised, undo stays a text undo, agents editing the
same file keep working, the 5 MB files keep working, and Raw mode stays the escape hatch. What it
does not give: Lexical's "the tree is the truth" invariants, which are exactly what produced Mark
Sharp's long list of skew bugs.

### B. A document tree with import and export, as Lexical does

A full rewrite of the editing core to a block and inline tree, serialised to markdown on save.
It would make the spreadsheet tables and drag-to-reorder natural, and cost every property in the
paragraph above. Not recommended.

### C. A web view running Lexical or Milkdown inside the pane

Arkos has an embedded WKWebView now (the browser pane). A Lexical editor in it would be Mark
Sharp itself, with its speed problem, its export normalisation, and a second text model that
Arkos's find, keymap, themes, effects and agent CLI would not reach. Not recommended, though it
would be the fastest demo.

## 7. Sketch of the milestones under option A

Each would get its own design and plan; the sizes are relative.

| # | Milestone | Scope | Size |
|---|---|---|---|
| A1 | Inline reveal | Paragraphs, headings, list items, quotes: rendered active block with markers for the element under the caret or a selection end; monospace fallback for the rest; Raw mode unchanged. Conversion while typing falls out of it, since the block re-parses per keystroke. Design: `2026-10-30-inline-reveal-design.md` | Large |
| A2 | Typing rules | The list, quote and code block key table of 2.3; Cmd+B, Cmd+I, Cmd+K for a link; paste URL over a selection; Shift+Enter hard break; the context menu | Medium |
| A3 | Slash menu | The catalogue of 3.1 with keywords and submenus, drawn like the command palette, inserting markdown text; table sizes by `/mxn` | Medium |
| A4 | Structure | Hover chevron folds with fold state in the workspace file, Fold All and Unfold All, an outline section in the sidebar with the current heading highlighted and a click that scrolls; drag handle to reorder blocks | Medium |
| A5 | Code block chrome | Language pill, line numbers, Copy, Shift+Enter to exit, border click selects | Small to medium |
| A6 | Tables as a spreadsheet | Cell-level caret with the grid staying rendered, typing replaces, F2 appends, Enter moves down, "+" strips, row and column selection, a context menu with insert, delete and alignment; today's re-align stays the writer | Large |
| A7 | Images and links | Paste and drop an image into a sibling folder with a numbered name; definition reveal; wikilinks; `#anchor` links within and across files; click-to-edit then click-to-follow (Cmd+click stays) | Medium |
| A8 | Reading niceties | Footnotes with hover and numbering, alerts with icons, frontmatter label with title and date, presentation mode (a read-only toggle in the pill), letter width, font size steps | Medium |
| A9 | Diagrams and math | Mermaid rendered in process (mermaid.js in a local, offline WKWebView page, replacing headless Chrome) with Code, Diagram and Split modes saved per diagram, zoom and pan; KaTeX inline and block through the same page, or a native math layout if one proves workable | Large |

## 8. Mark Sharp's weaknesses, which Arkos does not copy

The target is Mark Sharp's whole feature set and feel on Arkos's foundation. These are the places
where Mark Sharp is worse than Arkos already is, and copying them would lower the UX:

- Rewriting the file on save. Mark Sharp regenerates markdown from its tree, so bullets, numbers,
  blank lines, table spacing and line-break styles can come out different from what was written;
  its README warns of it. Arkos edits the text itself, so a save changes only what was typed and
  a `git diff` shows only the edit. Nothing is lost: the tidy tables Mark Sharp produces on export,
  Arkos produces when the caret leaves a table. Mark Sharp's formatting settings exist only
  because its export has to choose a style; Arkos needs none.
- Premium gating, the licensing server and the telemetry around it. Everything is unlocked.
- Webview constraints: Shift before dragging an image, chord shortcuts typing a stray letter, a
  white flash when opening in dark mode, "Lexical error" popups with a Report button. A native
  editor has none of these.
- A full re-render when the file changes on disk, which flashes tables and diagrams and slows on
  long files. Arkos reloads in place, keeping the caret and the scroll.
- The clipboard URL becoming a footnote's text, which the author himself calls weird.
- A separate editor for raw text. Arkos's Raw mode in the same tab, with the caret kept, is the
  better form of "Switch Editor Mode" and stays.

Where Arkos already goes past Mark Sharp (video posters and the video pane, collapsible
`<details>` with remembered state, 5 MB files, external edits without a flash) it keeps that too.

## 9. Rulings

Cyrus, 2026-10-30: for every open question, do what Mark Sharp does today. Applied:

1. Reveal scope: element-level for inline marks (the delimiters of the one run under the caret
   or a selection end). A heading shows its `#` prefix while the caret is anywhere on it. A list
   item's bullet stays a bullet and a quote's bar stays a bar; their `-`, `1.` and `>` are never
   shown in the rendered view (Raw mode shows everything, as before).
2. The active block keeps the rendered font and size while editing; the markers are drawn in it,
   dimmed. No monospace active block and no setting for one. Code blocks, tables and HTML blocks
   are the exceptions, as in Mark Sharp, because they are source by nature.
3. Order: Mark Sharp has no order to copy, so the order of section 5 stands: reveal, typing rules,
   slash menu, folding and outline first.
4. Math: in scope. KaTeX-style inline (`$x$`, only when the characters next to the dollars are
   not spaces) and block (`$$`) equations, revealed to their source when the caret enters them.
   This reverses ruling 6 of the 2026-10-16 Tier B design.
5. Mermaid: in scope as Mark Sharp has it: rendered in process with Code, Diagram and Split modes
   per diagram (Diagram the default, the choice remembered), zoom and pan, the templates of the
   slash menu, theme colours from the Arkos theme. Headless Chrome and the cached PNG go once
   the in-process renderer lands.
6. Outline: a section of the sidebar, as Mark Sharp's lives in VS Code's Explorer: the headings
   as a tree, a click scrolls, the heading in view is highlighted while scrolling, hover shows a
   preview of the section, deeper sections collapse.
7. Image drops: as VS Code's `markdown.copyFiles.destination` with Mark Sharp's naming. The
   default is beside the markdown file as `image.png`, `image-1.png`, `image-2.png`; a config
   key (`[viewer] image_destination`, a pattern with the document's folder and name in it) moves
   them, say into an `images` folder beside the file. Paste and drag and drop both work; Arkos
   needs no Shift.

Also from "what Mark Sharp does today", without a question asked: Cmd+B and Cmd+I format;
Shift+Enter is a hard line break; the context menu on a selection or right click offers Bold,
Italic, Strikethrough, Highlight, Link, Copy, Cut, Paste; a first click on a link edits it and a
click on the revealed url follows it (Cmd+click keeps following at once); presentation mode is a
read-only toggle that disables every reveal; footnotes, alerts, wikilinks, `#anchor` links, the
frontmatter label with its title and date, the code block language pill with line numbers and
Copy, spreadsheet table editing with the "+" strips and the context menu, and the six-dot drag
handle are all in scope. Section 8 stands: no rewriting of the file on save, no premium gating,
no Shift-before-drag, no clipboard-to-footnote, no full re-render on external edits, and Raw mode
in the same tab stays Arkos's form of "Switch Editor Mode".

The design spec for milestone A1 (inline reveal) is `2026-10-30-inline-reveal-design.md`.
