# Arkos IDE: markdown rendering Tier B (milestone 9)

Date: 2026-10-16. Status: draft for Cyrus's review; the rulings below were taken in chat on
2026-10-16. Builds on the markdown editing design (`2026-10-15-markdown-editing-design.md`, shipped
in 0.3.0: Tier A rendering, click-to-caret raw blocks, autosave) and the agent theme interface
(`2026-10-16-agent-theme-interface-design.md`, merged for the same release). Written against main
at `e8d8642`. Ships with milestone 8b and the analytics milestone in one release.

## 1. Rulings taken in chat

1. Tables render as a grid; editing stays markdown: the table block goes raw when the caret enters
   it, cell values are edited as text, Tab and Shift+Tab move between cells, Up and Down move
   between rows by cell, and the pipes are re-aligned when the caret leaves.
2. `http(s)` image URLs render too, because that is the common case in real files.
3. Video renders as a poster frame with a play glyph; a click opens the file in QuickTime Player.
4. Mermaid diagrams render through the Google Chrome that Arkos already drives, headless, to a
   cached PNG; Cyrus will say if this needs a different renderer.
5. HTML blocks stay source except `<img>` (rendered as an image) and `<details>` (a collapsible
   section).
6. Math is excluded.

## 2. Scope

| # | Item | Where |
|---|------|-------|
| 1 | Table grid rendering and cell parsing | `md_table.rs` (new), `md_view.rs` |
| 2 | Table editing: cell navigation and re-alignment on leave | `viewer.rs`, `md_table.rs` |
| 3 | Remote images: fetch, cache, placeholder, the off switch | `remote_image.rs` (new), `md_view.rs`, `config.rs` |
| 4 | Video and audio: poster frames and open-on-click | `media.rs` (new), `md_view.rs` |
| 5 | Mermaid: headless Chrome to PNG, cached by hash | `mermaid.rs` (new), `md_view.rs` |
| 6 | HTML `<img>` and `<details>` | `md_blocks.rs`, `md_view.rs` |
| 7 | Docs and the Status rows | `docs/guide.md`, `docs/development.md`, `docs/agent-guide.md` (a line) |

Out of scope: in-place cell editing with a spreadsheet feel (cells edited through the raw block
instead); math; footnotes; remote video; a Mermaid editor; rendering any HTML beyond the two
cases; images inside table cells (they show as their alt text).

## 3. Tables

- **Parsing.** `md_table::parse(source: &str) -> Option<Table { align: Vec<Align>, header:
  Vec<String>, rows: Vec<Vec<String>> }>` over the block's source lines: the header row, the
  delimiter row (`---`, `:--`, `--:`, `:-:` give Left (default), Left, Right, Center), the body
  rows; a leading and trailing pipe optional; `\|` is a literal pipe inside a cell; a row with
  fewer cells is padded with empty cells and one with more is truncated to the header's count (as
  GitHub does). Cell text keeps its inline markdown, rendered through the existing inline parser
  (emphasis, code, links; an image inside a cell shows its alt text). `None` when the delimiter row
  is missing or malformed, and the block renders as Tier A does today (the source in a monospace
  box) so nothing is lost.
- **Grid.** A header row in `chrome.text` at semibold on `chrome.tab_inactive`, body rows
  alternating `window_bg` and a 4 percent tint of `chrome.text`, a 1 px `chrome.border` hairline
  between rows and columns, 6 px by 10 px cell padding, text aligned per column. Column widths:
  each column takes the widest cell's natural width, capped at 45 percent of the view's width;
  when the sum exceeds the view, columns shrink proportionally to fit and cells wrap; a table never
  scrolls sideways. The grid's total height is measured like any other block for the virtualised
  layout.
- **Caret and clicks.** A click on a cell makes the block raw with the caret at the matching
  position in the source: the click's cell index maps to the cell's text in the row's source line,
  and the x offset within the cell maps to a column within that text (the same hit-testing the
  rendered paragraphs use, applied per cell). The block renders raw in the terminal font as today.
- **Editing in the raw block.** Inside a table block (the active block's kind is `Table`):
  - Tab moves the caret to the start of the next cell's text (across the row's end into the next
    row's first cell); Shift+Tab to the previous; Tab from the last cell of the last row appends a
    new row of empty cells (`| | |` with the header's cell count), an undoable edit, and lands in
    its first cell.
  - Up and Down move to the same cell index in the row above or below, caret at the start of that
    cell's text (the delimiter row is skipped); at the first or last row they leave the block as
    arrows do today.
  - Enter inserts a newline as in any raw block (it splits the table; the user can undo).
  - Typing edits the source; no auto-alignment while typing.
- **Re-alignment on leave.** When the caret leaves a table block that was edited while active, the
  block's source is rewritten with the pipes aligned: every cell padded with spaces to the
  column's width (the delimiter row keeps its alignment colons and stretches its dashes), one
  space inside each pipe, a leading and trailing pipe always present. The rewrite is one undoable
  edit (undo restores the unaligned text) and does not run when the text is unchanged. Pure:
  `md_table::realign(source) -> Option<String>` (`None` when the table does not parse, so a broken
  table is left exactly as typed).
- **Keys that reach the viewer.** Tab and Shift+Tab already arrive as `KeyAction::Tab` and
  `ShiftTab` (list indent today); inside a table block they mean cells. Up and Down are
  `Move(Line)`; inside a table block they mean rows. The Keybindings window's "Viewer and
  markdown editing" section lists the four.

## 4. Remote images

- An image whose path starts with `http://` or `https://` is fetched through the system `curl`
  (as the updater does: TLS and proxies are macOS's business; no HTTP crate) into
  `<app support>/cache/images/<sha256 of the url>.<ext>` with a 10 MB cap, a 15 second timeout,
  no cookies and no referer, at most four fetches in flight per window; the alt box shows the alt
  text and "loading" until the file lands, then the image at most the view's width; a failed fetch
  shows the alt text and the url, retried only when the file is reopened. The cache is reused
  across sessions and entries older than 30 days are removed at launch.
- `[viewer] remote_images = true` in `config.toml` (default true, per the ruling); `false` shows
  the alt text and the url and fetches nothing. The agent guide and the user guide say what is
  fetched and when, since a markdown file can make Arkos contact any host it names.
- Local images are unchanged. A click on any rendered image opens it with the system (Preview for
  a local file; the cached file for a remote one).

## 5. Video and audio

- A paragraph holding only `![alt](path)` whose path ends in `.mp4`, `.mov`, `.m4v` or `.webm`
  (local only) renders a poster: `qlmanage -t -s 1024 -o <cache dir> <file>` (Quick Look's
  thumbnailer, part of macOS; no new dependency) run on the background executor once per file,
  cached at `<app support>/cache/posters/<sha256 of path and mtime>.png`; the poster at most the
  view's width with a centred play glyph (a triangle in `chrome.text` on a translucent disc); the
  alt text in a dashed box while the thumbnail runs or when it fails. A click opens the file in
  QuickTime Player through `open -a "QuickTime Player"`.
- `.mp3`, `.m4a`, `.wav`, `.aac` render a one-line box: a speaker glyph, the file name and its
  size; a click opens it with the system.
- Remote video and audio URLs render as a link (the alt text with the url), opened in the browser
  on click; nothing is fetched.

## 6. Mermaid diagrams

- A fenced code block whose info string is `mermaid` renders as a diagram. The renderer is Google
  Chrome, headless: `<Chrome bundle>/Contents/MacOS/Google Chrome --headless=new
  --disable-gpu --hide-scrollbars --force-device-scale-factor=2 --user-data-dir=<temp profile>
  --screenshot=<out.png> --window-size=W,H file://<page.html>` where `page.html` is written by
  Arkos into the cache folder with the block's source, `mermaid.min.js` (bundled into the binary
  with `include_bytes!`, written out once per version), a transparent background, and mermaid
  theme variables from the current theme (`background` none, `primaryColor` the chrome's
  `tab_inactive`, `primaryTextColor` and `lineColor` the chrome's `text`, the font the UI font);
  the page sizes the SVG to its natural width (capped at the view's width in CSS pixels) and
  reports its height through the document title, which a first `--dump-dom` pass reads so the
  screenshot's window matches the diagram (two Chrome runs per render, both under a second on a
  laptop; cached after). Chrome is found as the browser pane finds it (the `Google Chrome.app`
  bundle in `/Applications` or `~/Applications`).
- Cache: `<app support>/cache/mermaid/<sha256 of source, theme name and scale>.png`; the block
  shows the cached PNG at once, else its source in the code-block style with "rendering" in the
  info line until the PNG lands, then the image. A render error (mermaid's own parse error, taken
  from the page's title) shows the source with the error line in `tree.deleted`. Chrome absent:
  the source with "diagram: install Google Chrome to render" once in the info line.
- Renders run on the background executor, at most one at a time per window, and never for a
  block that is active (raw), so typing in a diagram does not launch Chrome per keystroke; the
  render starts when the caret leaves the block.
- Privacy: the page is a local file, Chrome runs with a throwaway profile under the cache folder,
  no network is needed, and nothing leaves the machine.

## 7. HTML blocks

- An HTML block that is a single `<img ...>` tag (with optional width and height attributes)
  renders as an image by the same rules as a markdown image (local or remote); `width` is
  honoured when smaller than the view.
- An HTML block that starts with `<details>` and ends with `</details>` renders as a collapsible
  section: the `<summary>` text (or "Details") on a row with a disclosure triangle, collapsed
  by default, a click toggles (view state per block, kept while the tab is open, not written to
  the file); the content between the summary and the closing tag is parsed as markdown blocks
  and rendered read-only inside the section; a click on that content makes the whole `<details>`
  block raw, as any block.
- Every other HTML block stays as it is: dimmed source.

## 8. Config, keys and docs

- `[viewer] remote_images` (bool, default true) is the one new key; `arkos settings set
  viewer.remote_images false` joins the allow-list (the Settings window gets a switch "Remote
  images" under a new Viewer line in the Text section: deferred unless Cyrus asks).
- No new keymap rows: Tab, Shift+Tab, Up and Down gain meanings inside a table block only.
- Docs: `docs/guide.md`'s "Editing markdown" section gains tables (the grid, the keys, the
  re-alignment), remote images and the off switch, video and audio, Mermaid and its Chrome
  requirement, `<img>` and `<details>`; `docs/development.md` Status rows: a table renders as a
  grid and a click lands in the cell; Tab, Shift+Tab, Up, Down move by cell; leaving the block
  realigns the pipes and undo restores them; Tab at the end adds a row; a remote image loads and
  shows "loading" first; `remote_images = false` shows the url instead; a `.mov` shows its poster
  and opens in QuickTime; a Mermaid block renders within two seconds and re-renders after an
  edit; the source shows with a message when Chrome is missing; a `<details>` block opens and
  closes; a 5 MB file with a large table stays responsive (the milestone 7 gate).

## 9. Tests

Headless: `md_table::parse` (leading and trailing pipes, escaped pipes, short and long rows, a
malformed delimiter), `realign` (idempotent, keeps alignment colons, `None` on a broken table),
the cell navigation helpers (`next_cell`, `prev_cell`, `cell_above`, `cell_below`, `append_row` as
pure functions over the block's source and a caret), the remote-image cache path and the url
rule, the `curl` argument builder, the poster and mermaid cache keys, the mermaid page builder
(the theme variables land in the HTML, the source is escaped), the Chrome argument builder, the
`<img>` and `<details>` recognisers and the details' inner split. Chrome, curl and qlmanage are
never run by tests; the renders are hand checks.

## 10. Open questions for Cyrus

1. Remote images on by default (per the ruling) with the off switch, or off until the first
   remote image is seen and a one-time notice offers to turn them on? The draft says on.
2. Should `<details>` remember its open state in the workspace file across relaunches? The draft
   says no (view state while the tab is open).
3. Column widths: the 45 percent cap and proportional shrink, or a horizontal scroll inside the
   table for very wide ones? The draft never scrolls sideways.
