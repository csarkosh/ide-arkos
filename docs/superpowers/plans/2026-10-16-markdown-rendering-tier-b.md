# Markdown rendering Tier B (milestone 9)

Status: plan reviewed (opus, 2026-10-16, "With fixes") and amended; the rulings on the reviewer's
questions are in the Self-review notes, for Cyrus to confirm at approval. Spec:
`docs/superpowers/specs/2026-10-16-markdown-rendering-tier-b-design.md` (reviewed by Cyrus on
2026-10-16; its nine rulings bind). Written against main at `34d1c9b`. Merges without a release;
ships with milestone 8b and the analytics work in one release, after the Blogger reshoot (Task 8).

Goal: tables render as a grid and edit as markdown with cell navigation and re-alignment; remote
images render; video and audio show posters that open in QuickTime; Mermaid blocks render through
headless Chrome to cached PNGs; `<img>` and `<details>` HTML blocks render; wide tables wrap or
scroll by a Settings choice; `<details>` state survives a relaunch.

## Global Constraints

- Machine rules: every cargo call is `nice -n 15 ~/.cargo/bin/cargo ...`; the worktree's
  `.cargo/config.toml` stays; no `--release`; never two cargo processes at once; focused filters
  while iterating, the workspace suite once per task; `cargo check --workspace --all-targets`
  prints no warning from our crates, in the test build too. Known timing flakes under load:
  `concurrent_spawns_give_each_shell_only_its_own_terminal`,
  `the_grace_counts_from_the_relaunch_so_a_slow_relaunch_keeps_the_old_bundle`,
  `recovery::tests::a_death_rewrites_the_manifest_without_the_dirty_mark`,
  `continuous_reader_survives_a_flood`; rerun alone and report both results.
- Dead code between tasks: an item a later task first uses, reached by a test here, gets
  `#[cfg_attr(not(test), allow(dead_code))] // Used from Task N on.`; unreached items get
  `#[allow(dead_code)] // Used from Task N on.`; the task that first uses it removes the attribute;
  after Task 7 `grep -rn "Used from Task" crates/` prints nothing.
- No em-dashes in any file we write; the vendored `crates/arkos/assets/mermaid.min.js` is exempt
  (third-party text, kept with its MIT licence header). Comments are full sentences. Headless
  tests only: no test opens a window, runs curl, qlmanage, Chrome or `open`, or touches the real
  Application Support; every process invocation goes through an argument builder tests check as
  data. New modules get their `mod` lines in `main.rs` (~1-63).
- **Where things run.** Fetches, thumbnails and diagram renders run on the background executor
  (`cx.background_executor().spawn`, as `viewer.rs` ~917 and ~1017 do; blocking that thread on
  `Command::output` is fine, `md_view::follow_link` ~640 already does) and land through
  `viewer.update`. They start from `md_view::fill` (~268), which runs inside `viewer.update(cx,
  |v, _| ..)` for the rows the list draws (and only for rows that are not raw, which makes "never
  while the block is active" automatic); `block_element` takes `&Viewer` for the frame's state
  (no map cloned into `BlockCtx`). Limits are process-wide (a window is a process): at most four
  fetches in flight, one thumbnail, one diagram render, each through a `static` gate. Each has a
  cache under `paths::app_support().join("cache")` (`images/`, `posters/`, `mermaid/`), created on
  demand; `cache::sweep(dir, max_age) -> usize` removes plain files older than `max_age` whose
  name has one of the known suffixes (`png jpg jpeg gif webp svg html`), never directories; it is
  run once from `main()` on a `std::thread` after the window opens: 30 days for `images/` and
  `mermaid/`, 90 for `posters/`.
- **Tables** (`crates/arkos/src/md_table.rs`, new; `unicode-width = "0.2"` added to
  `crates/arkos/Cargo.toml`, already in the lock through other crates): `pub struct Table {
  pub align: Vec<Align>, pub header: Vec<String>, pub rows: Vec<Vec<String>> }`, `pub enum Align {
  Default, Left, Center, Right }` (`---` is Default, drawn left; `realign` writes back exactly the
  colons the source had); `pub fn parse(source: &str) -> Option<Table>` (a leading and trailing
  pipe optional, `\|` a literal pipe in a cell, short rows padded, long rows truncated to the
  header's count, the delimiter row every cell matching `:?-+:?` after trimming and the row holding
  at least one pipe or being a single `---` cell for a one-column table; `None` otherwise); cell
  text in `header` and `rows` is the source text with `\|` kept (the inline renderer unescapes
  when drawing); `pub fn realign(source: &str) -> Option<String>` (each cell padded to the
  column's display width (`unicode_width::UnicodeWidthStr::width`), padding on the right for
  Default and Left, on the left for Right, split for Center; the delimiter row keeps its colons and
  stretches its dashes to the width, at least three; one space inside each pipe; a leading and
  trailing pipe always; `\|` kept; lines joined with `\n`; the source's trailing newline kept or
  absent as it was; idempotent; `None` when `parse` fails). Positions are char indices (`Pos.ch`).
  `pub struct CellPos { pub row: usize, pub cell: usize }` where `row` is the source line index
  within the block (the delimiter row is 1 and never a destination); `pub fn cell_at(source, line,
  ch) -> Option<CellPos>` (`None` on the delimiter row and outside the table); `pub fn
  cell_start(source, pos) -> Option<(usize, usize)>`: the line and the char index of the cell's
  first non-space character, or, for an empty cell, the index after the pipe plus one space
  (clamped to the next pipe); `pub fn next_cell`, `prev_cell` (across the row's end, over the
  delimiter row; `None` past the ends), `cell_above`, `cell_below` (same cell index, skipping the
  delimiter row; `None` past the ends) `(source, pos) -> Option<CellPos>`; `pub fn
  appended_row(source) -> String` (`\n| ` plus ` | ` joins plus ` |` for the header's cell count:
  `|  |  |` for two columns, so each cell has a caret place after "| ").
  Rendering (`md_view.rs`): `Piece::Table(TablePiece::{Grid(Table, Widths), Source(SharedString)})`
  (`Source` when `parse` fails: today's mono box). The grid: rows of cells as `div().flex()` rows
  with `w(px(..))` and `min_w_0` cells at the prose font and size (`scale(Table)` 1.0 is the
  terminal size today: the grid uses the prose scale instead); the header row semibold on
  `chrome.tab_inactive`; body rows alternate `window_bg` and `chrome.text` at 4 percent; 1 px
  `chrome.border` hairlines; 6 px by 10 px cell padding; alignment per column; cell text through
  the inline runs (`\|` drawn as `|`; an image inline shows its alt text). Natural widths: measured
  in the `list(state, move |ix, window, cx| ..)` closure (~252), the one place with a `Window`,
  with `window.text_system().layout_line(..)` as `row_counts` does (~842), once per block and
  cached in `Rendered` keyed by (family, size); a cell's natural width is its text width plus
  20 px padding plus 1 px; the view width is `v.list_state.viewport_bounds()` minus `2 * SIDE`;
  only the first 200 rows are measured (later rows take the widths found); a table over 500 rows
  renders as `Source` (the 5 MB gate stays a hand check). `pub fn column_widths(natural: &[f32],
  view: f32, mode: WideTables) -> Vec<f32>` (pure): each column its natural width capped at 45
  percent of the view; `Wrap`: when the sum exceeds the view, shrink proportionally to the view
  and cells wrap; `Scroll`: keep the capped widths and the grid's container (`div().id(..)
  .overflow_x_scroll().track_scroll(&handle)` with `style().restrict_scroll_to_axis =
  Some(true)` so a vertical wheel still scrolls the list; the `ScrollHandle` in the `Rendered`
  entry, lost on an edit, which is fine) scrolls sideways; Shift+wheel arrives as horizontal delta
  on macOS already. A click on a cell: the cell's `on_mouse_down` sets the row's `task_press`-style
  flag (~418, ~718) so the row's own press does not place the caret, and calls `viewer.update(|v,
  cx| v.click_table_cell(block, row, cell, local_x, cx))`; the viewer maps it to the source
  position through `cell_start` plus the char offset the cell's text layout gives for `local_x`
  (the same index lookup the paragraphs use), places the caret (`place_caret`'s path, so
  `caret_placed` holds) and the block renders raw.
- **Table editing** (`viewer.rs`; in Rendered mode only, ruling A below): `key_inner` (~1262)
  passes `self.blocks.clone()` to `apply_action` for `Tab | ShiftTab` as it does for `Newline`
  (~1331), so `apply_action` (~2259) sees the block kind through `blocks_before` and
  `md_blocks::block_at`; when the caret's block is a `Table`: `Tab` moves to `next_cell` and
  `ShiftTab` to `prev_cell` (the caret at `cell_start`, selection cleared, `doc.break_coalescing()`
  called so the move is not merged into a typing step); on the delimiter row (`cell_at` is
  `None`) Tab moves to the first cell of the next row and Shift+Tab to the last cell of the row
  above, never inserting spaces; `Tab` from the last cell of the last row inserts `appended_row`
  at the end of the block's last non-blank line (not at `block.lines.end`, which is the blank line
  the block owns) as one undoable edit and lands in the new row's first cell. Up and Down are
  handled by `rendered_key` through `md_view::navigate` (viewer.rs ~1355, md_view.rs ~1039),
  not `apply_action`: `navigate` gains the table rule for `Move(Up | Down)` with `!select` when
  the caret's block is a `Table`: `cell_above`/`cell_below` to that cell's `cell_start`; at the
  first or last row it falls through to the existing line move, which leaves the block; shifted
  arrows (selection) are unchanged. Re-alignment on leave: the viewer keeps `table_entry:
  Option<(usize, String)>` (the table block's start line and its text when the caret entered it;
  undo and redo and `edit_elsewhere` do not touch it); a new `fn note_caret_moved(&mut self, cx)`
  called at the end of `key_inner` (~1262), `place_caret` (~1625), `append_paragraph` (~1500) and
  after undo and redo compares the caret's block with `table_entry`: when the caret is now outside
  that block and the block's current text (its lines up to the last non-blank, `pos_of_offset(..,
  usize::MAX).line`) differs from the entry snapshot, `realign` runs on it and, when it returns a
  different string, `edit_elsewhere` replaces those lines as one undo step (`replace_range` puts
  the undo caret at the table's first line, which is acceptable), then `table_entry` is cleared,
  or set to the new block when that is a table. Comparing with the entry snapshot (not an edited
  flag) means an undo that lands the caret back in the table and a leave again do not realign the
  restored text. A broken table (`realign` is `None`) is left as typed. `Enter` is unchanged.
  Tests build state inline as the existing `apply_action` tests do (viewer.rs ~2597:
  `edit::Document::from_text`, `md_blocks::split(d.lines())`, `edit::Caret::at(Pos { line, ch })`,
  `Instant::now()`; a caret's position is `c.head.line` and `c.head.ch`; `Document::undo()` takes
  no argument).
- **Remote images** (`crates/arkos/src/remote_image.rs`, new): `pub fn is_remote(path) -> bool`
  (`http://` or `https://`, case-insensitive); `pub fn cache_path(dir, url) -> PathBuf`
  (`<dir>/<sha256 hex of url>` with no extension at first); `pub fn curl_args(url, dest) ->
  Vec<String>` (`--disable --fail --silent --show-error --location --max-redirs 5 --proto
  =http,https --proto-redir =http,https --max-time 15 --max-filesize 10485760 --output <dest> --
  <url>`; `--disable` keeps a `~/.curlrc` from adding cookies or a referer, as the updater does);
  after the fetch the file's length is checked (over 10 MB: deleted, Failed) and its format
  sniffed from the first bytes (`image::guess_format` is what gpui uses; a small sniff of PNG,
  JPEG, GIF, WebP, else SVG) and the file renamed with that extension, so `open -a Preview` has a
  type; the part file is `<dest>.<pid>.part` (two windows can fetch the same url). `pub struct
  FetchQueue` (`want(url)` records a url once, `start_ready() -> Vec<String>` moves up to four
  minus in-flight urls to in-flight and returns them, `in_flight() -> usize`, `done(url)`); the
  gate is a process-wide `static` so the four-in-flight limit is per window. The viewer keeps
  `remote: HashMap<String, Remote::{Fetching, Ready(PathBuf), Failed}>`; `fill` checks the cache
  (`cache_path` with any of the known extensions present: Ready at once), else queues; the fetch
  task runs `/usr/bin/curl` with a wall-clock backstop (as `run_bounded` does: kill after 20 s)
  and lands through `viewer.update` with `cx.notify()`. The image element: `Ready` draws
  `img(path)` (gpui sniffs the format, so the extension is for `open` only) at most the view's
  width; `Fetching` the alt box with the alt text and "loading"; `Failed` the alt text and the
  url. Badge rows (ruling C): a paragraph holding only images and whitespace renders them in a
  row, each at its natural size capped at the view's width, wrapping as a flex row. Always on
  (ruling 7).
- **Clicks on media** (ruling B): a plain click on a rendered image, poster or badge places the
  caret in the block as any click does (the block goes raw, where the path is edited); Cmd+click
  opens it: an image with `/usr/bin/open -a Preview <absolute path>` (never a bare `open`, which
  could run a program: the viewer's own rule at md_view.rs ~555), a video with `open -a "QuickTime
  Player" <path>`, audio with `open <path>` (safe because `media::kind` admitted the extension), a
  remote video or audio url in the browser (`cx.open_url`). The hand cursor shows only with Cmd
  held, as links do. The `<details>` row's toggle uses the `task_press` pattern so the row's press
  does not place the caret; a click on the details' content places the caret (the block goes raw).
- **Media** (`crates/arkos/src/media.rs`, new): `pub enum Media { Video, Audio }`; `pub fn kind(path)
  -> Option<Media>` by extension, case-insensitive: `mp4 mov m4v` video, `mp3 m4a wav aac` audio
  (`webm` is neither: Quick Look cannot thumbnail it and QuickTime cannot play it; it renders as a
  link opened in the browser, ruling D); a remote url gives `None`. `pub fn poster_path(dir, file,
  mtime_secs) -> PathBuf` (`<dir>/<sha256 of path and mtime>.png`); `pub fn qlmanage_args(file,
  out_dir) -> Vec<String>` (`-t -s 1024 -o <out_dir> <file>`; no `--`, the paths are absolute);
  qlmanage writes `<out_dir>/<file name with its extension>.png` (`clip.mov.png`) and can exit 0
  without writing, so the task uses a per-job folder under `posters/` and succeeds only when
  exactly one `.png` is there, renaming it to `poster_path`; a 20 s backstop. `pub fn
  quicktime_args(file) -> Vec<String>` (`-a`, `QuickTime Player`, `<file>`). `pub fn
  size_text(bytes: u64) -> String` (`950 B`, `2.4 KB`, `5.1 MB`). The viewer keeps `posters:
  HashMap<PathBuf, Poster::{Making, Ready(PathBuf), Failed}>` and reads a file's size and mtime in
  `fill`, not per frame. Rendering: a video paragraph draws the poster at most the view's width
  with a centred play glyph (a 40 px disc at `chrome.text` 60 percent with a triangle in
  `window_bg`); `Making` or `Failed` the alt box; audio a one-line box with a speaker glyph
  (`\u{1F50A}`), the file name and its size.
- **Mermaid** (`crates/arkos/src/mermaid.rs`, new): a fenced block whose trimmed info string is
  exactly `mermaid` is a diagram block (`pub fn is_diagram_info`). `pub const VERSION: &str`
  (the vendored mermaid version); `pub const SCRIPT_NAME: &str = "mermaid-<VERSION>.min.js"`
  built from it; `pub fn page(source, theme: &MermaidTheme) -> String`: `<meta http-equiv=
  "Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline' file:;
  style-src 'unsafe-inline'; img-src data:">`, `<script src="./<SCRIPT_NAME>">`,
  `mermaid.initialize({ startOnLoad: false, theme: 'base', themeVariables: { background:
  'transparent', primaryColor, primaryTextColor, lineColor, secondaryColor, tertiaryColor,
  fontFamily } })` with `fontFamily` `-apple-system, system-ui` (the prose font is the system
  font; Chrome needs that spelling), the source HTML-escaped in `<pre class="mermaid">`, a script
  that `await`s `mermaid.run()`, then sets `document.title` to `ok <w> <h>` (the SVG's bounding
  box in CSS px, natural width capped at 2000) or `error <first line of the message>`, and sizes
  the body to it; the body and html backgrounds transparent. `pub struct MermaidTheme { primary,
  text, line, secondary, tertiary: String }` from the chrome colours (`tab_inactive`, `text`,
  `text`, `banner`, `window_bg`) as hex; `pub fn cache_key(source, theme: &MermaidTheme, scale)
  -> String` (sha256 over the source, the five hex values and the scale: a user theme edited in
  place keeps its name, so the key uses the values). `pub fn chrome_app() -> Option<PathBuf>`:
  `/Applications/Google Chrome.app` or `~/Applications/Google Chrome.app` (the browser pane opens
  Chrome by name through `open -na`; this is the one place a path is needed). Common flags
  (`pub fn common_args(profile) -> Vec<String>`): `--headless=new --disable-gpu --no-first-run
  --use-mock-keychain --disable-background-networking --disable-component-update --disable-sync
  --disable-default-apps --disable-extensions --no-default-browser-check
  --host-resolver-rules=MAP * ~NOTFOUND --user-data-dir=<profile> --virtual-time-budget=5000
  --timeout=15000`; `dump_args(chrome, profile, page)` adds `--dump-dom <file url>`;
  `shot_args(chrome, profile, page, out, w, h)` adds `--hide-scrollbars
  --force-device-scale-factor=2 --default-background-color=00000000 --window-size=<w>,<h>
  --screenshot=<out> <file url>` (the PNG is 2w by 2h). `pub fn size_from_dom(dom) ->
  Result<(u32, u32), String>` reads the title (HTML entities unescaped). The render task (one at
  a time per process through a `static` gate; the viewer starts it from `fill` for a non-raw
  diagram block with no `Ready` entry): write `<cache>/mermaid/<SCRIPT_NAME>` once from
  `include_bytes!("../assets/mermaid.min.js")` (`crates/arkos/assets/`, vendored by
  `scripts/vendor-mermaid.sh` from the pinned npm tarball with its sha256 recorded in the script),
  write the page to `<key>.<pid>.html`, run the dump pass with a per-process profile
  `<cache>/mermaid/profile-<pid>` (created per run, removed at the end of the run; the sweep
  removes `profile-*` folders whose pid is not alive), parse the size, run the screenshot pass into
  `<key>.<pid>.png`, rename to `<key>.png`, delete the page; land through `viewer.update`. States
  (`Viewer.diagrams: HashMap<String, Diagram::{Rendering, Ready { png: PathBuf, w: u32, h: u32 },
  Error(String), NoChrome}>` by cache key; a cache hit needs the size too, so `<key>.json` holds
  `{ "w", "h" }` beside the PNG): `Ready` draws `img(png).w(px(w)).max_w_full()` (a raster
  draws at its pixel size, so the 2x PNG is given its CSS width); `Rendering` the source in the
  code-block style with "rendering" in the info line; `Error` the source with the error line in
  `tree.deleted`; `NoChrome` the source with "diagram: install Google Chrome to render" (checked
  once per process). A hand check and a Status row: the headless child must not be grabbed by the
  browser pane's docking (`ax.rs` picks the lowest Chrome pid); if it is, exclude Arkos's own
  child pids there in the fix wave.
- **HTML** (`md_blocks.rs`, `md_view.rs`): CommonMark ends a type-6 HTML block (`<details>`,
  `<summary>`, `<p>`, `<div>`) at a blank line, and `split` (~39-95) takes each depth-1 event as
  a block, so `<details>` with blank lines inside arrives as several blocks. `split` therefore
  merges, after collecting: a block whose first trimmed line starts with `<details` (ASCII
  case-insensitive) absorbs every following block up to and including the first one containing
  `</details>` (counting nested `<details` so an inner one does not close the outer), into one
  block of the new `BlockKind::Details`; with no closing tag the blocks stay as they were. `pub fn
  html_details(block: &str) -> Option<Details { summary: String, inner: String }>`: the
  `<summary>` (optional, on the first line or the next; "Details" when absent; also when
  `<summary>` sits on the same line as `<details>`), `inner` the lines after it up to the closing
  tag. `pub fn html_image(block: &str) -> Option<HtmlImage { src: String, width: Option<u32>,
  center: bool }>`: a block that is one `<img ...>` tag (attributes in any order, single or
  double quotes, optional `/>`), optionally wrapped in `<p>`, `<div>` or `<a>` tags with no text
  (ruling E), `center` when a wrapper has `align="center"`. `Inline::HtmlImage` renders by the
  image rules (width honoured when smaller than the view; centred when `center`);
  `Inline::Details { summary, blocks: Vec<(BlockKind, Vec<Inline>)> }` (built by `split` and
  `inline` over `inner`, `Blank` blocks dropped) renders a row with a disclosure triangle
  (`\u{25b8}` closed, `\u{25be}` open) and the summary, then, when open, the inner blocks through
  a `piece_element` helper factored out of `block_element`'s piece loop (~383-409), read-only,
  with a nested `Details` rendered as source. The open state (ruling F: per file): `Workspace.
  details_open: BTreeMap<PathBuf, Vec<DetailsOpen { summary: String, index: usize }>>` (the
  workspace file is JSON, `workspace.rs` ~144; serde default, skipped when empty; no version
  bump since the structs have no `deny_unknown_fields`), `index` the block's position among the
  file's `Details` blocks in order (a cheap scan of `v.blocks`); the viewer holds
  `open_details: BTreeSet<(String, usize)>`, restored in `viewer_for` (~2340) from the workspace
  by path, toggled by the click, and saved through `ViewerEvent::DetailsToggled(Vec<DetailsOpen>)`
  handled in `viewer_for`'s subscription (~2350) with `Tree::set_details_open(path, list)`
  beside `set_viewer_mode` (tree.rs ~431) and `root.save(cx)`.
- **Config and Settings**: `ViewerConfig` gains `wide_tables: WideTables` (`#[derive(Debug, Clone,
  Copy, PartialEq, Eq, Default, Serialize, Deserialize)] #[serde(rename_all = "lowercase")] pub
  enum WideTables { #[default] Wrap, Scroll }`, as `AppearanceMode` ~153; `ViewerConfig`'s
  hand-written `Default` ~108 gains it); DEFAULT_CONFIG gains the commented key under `[viewer]`
  with one line of explanation; `agent_cli::SETTABLE` gains `viewer.wide_tables` with
  `one_of(key, raw, &["wrap", "scroll"])` in `typed_value` (~756); the Settings window's Text
  section gains, after the steppers, a heading "Wide tables" with segments "Wrap" and "Scroll"
  writing `["viewer", "wide_tables"]` through a pure `fn wide_tables_edit(mode: WideTables) ->
  Edit` built on `edit()` (~290) with a test; `md_view::render` is called from `pane_view.rs`
  (~994) and gains `wide_tables` next to `highlight_limit`.
- **Docs**: `docs/guide.md` "Editing markdown" gains tables (the grid, the four keys, the
  re-alignment and its undo, the new row from Tab, Wrap or Scroll, Rendered mode only), remote
  images (always on, what is fetched, the cache, badge rows), Cmd+click to open an image or
  poster, video and audio, Mermaid (Chrome required, the cache, the hardening that keeps it
  offline), `<img>` and `<details>` (the remembered state per file); `docs/guide.md` ~410 and
  `docs/agent-guide.md` ~121 list `viewer.wide_tables` among the settable keys;
  `docs/development.md` gains the Status rows from spec section 8, the Cmd+click rows, the
  docking hand check, and the vendored mermaid version; `keymap::MARKDOWN_KEYS` gains "Next cell
  (in a table)" Tab, "Previous cell (in a table)" Shift+Tab, "Cell above (in a table)" Up, "Cell
  below (in a table)" Down.

## Review Focus

1. A table with `\|` in a cell and a missing trailing pipe parses, renders, realigns and parses
   again unchanged, with the `\|` intact (Task 1).
2. Tab in the last cell of the last row appends a row and lands in it; undo removes the row;
   leaving the block realigns once; undo restores the typed text and leaving again does not
   realign (Task 2).
3. A remote image's fetch cannot block the UI, cannot exceed 10 MB, follows no redirect off
   http(s), cannot be given cookies by a `.curlrc`, and a second render of the same url while
   fetching does not start a second curl (Task 3).
4. A Mermaid block that is active never launches Chrome; leaving it renders once; the same source
   and theme values never render twice; two windows rendering at once use two profiles (Task 5).
5. A `<details>` block with blank lines inside is one block, its state is in the workspace file
   after a toggle, back after a relaunch in any tab on that file, and survives an edit above it
   (Task 6).
6. A plain click on an image places the caret; Cmd+click opens Preview with the absolute path
   and never a bare `open` (Task 3).

## File structure

- Create: `crates/arkos/src/md_table.rs`, `remote_image.rs`, `media.rs`, `mermaid.rs`, `cache.rs`,
  `crates/arkos/assets/mermaid.min.js` (vendored), `scripts/vendor-mermaid.sh`.
- Modify: `md_view.rs`, `md_blocks.rs`, `viewer.rs`, `tree.rs`, `workspace.rs`, `pane_view.rs`
  (the `render` call), `config.rs`, `settings.rs`, `agent_cli.rs`, `keymap.rs`, `main.rs` (`mod`
  lines, the sweep), `crates/arkos/Cargo.toml` (`unicode-width`), `docs/guide.md`,
  `docs/agent-guide.md`, `docs/development.md`.

---

### Task 1: Table parsing, re-alignment, navigation, and the grid

**Files:** create `md_table.rs`; modify `md_view.rs` (`Piece::Table(TablePiece)`, the grid, the
measurement in the list closure, `column_widths`, the cell click calling
`Viewer::click_table_cell`), `viewer.rs` (`click_table_cell`, which places the caret at
`cell_start` plus the hit offset through `place_caret`), `config.rs` (`WideTables` and the key),
`pane_view.rs` (passing `wide_tables`), `crates/arkos/Cargo.toml` (`unicode-width`).

- [ ] **Step 1: Failing tests** (`md_table.rs`)

```rust
#[test]
fn a_table_parses_with_optional_outer_pipes_escaped_pipes_and_ragged_rows() {
    let t = parse("| a | b |\n|:--|--:|\n| 1 | 2 |\nx | y \\| z | extra\n| only |\n").unwrap();
    assert_eq!(t.align, vec![Align::Left, Align::Right]);
    assert_eq!(t.header, vec!["a", "b"]);
    assert_eq!(t.rows, vec![vec!["1", "2"], vec!["x", "y \\| z"], vec!["only", ""]]);
    assert_eq!(parse("| a |\n|---|\n").unwrap().align, vec![Align::Default]);
    assert!(parse("| a |\n| 1 |\n").is_none(), "no delimiter row");
    assert!(parse("| a |\n|-x-|\n").is_none(), "a malformed delimiter row");
    assert!(parse("| a |\n|:|\n").is_none(), "a lone colon is malformed");
    assert_eq!(parse("a|b\n-|-\n").unwrap().header, vec!["a", "b"]);
}

#[test]
fn realign_pads_by_alignment_keeps_the_colons_and_escapes_and_is_idempotent() {
    let src = "|a|bb|c|\n|:-|-:|:-:|\n|ccc|d|e \\| f|\n";
    let out = realign(src).unwrap();
    assert_eq!(out, "| a   | bb |    c    |\n|:----|---:|:-------:|\n| ccc |  d | e \\| f |\n");
    assert_eq!(realign(&out).unwrap(), out);
    assert!(realign("| a |\n| 1 |\n").is_none());
    assert_eq!(realign("| a |\n|---|\n| 1 |").unwrap(), "| a |\n|---|\n| 1 |", "no colon added, no trailing newline added");
    assert_eq!(realign("| é |\n|---|\n| 日本 |\n").unwrap(), "| é    |\n|------|\n| 日本 |\n", "display width, not bytes");
}

#[test]
fn cells_are_found_and_walked_across_rows_skipping_the_delimiter() {
    let src = "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n";
    assert_eq!(cell_at(src, 0, 2), Some(CellPos { row: 0, cell: 0 }));
    assert_eq!(cell_at(src, 2, 6), Some(CellPos { row: 2, cell: 1 }));
    assert_eq!(cell_at(src, 1, 2), None, "the delimiter row is not a cell");
    assert_eq!(cell_start(src, CellPos { row: 2, cell: 1 }), Some((2, 6)));
    assert_eq!(next_cell(src, CellPos { row: 0, cell: 1 }), Some(CellPos { row: 2, cell: 0 }), "across the row's end, over the delimiter");
    assert_eq!(prev_cell(src, CellPos { row: 2, cell: 0 }), Some(CellPos { row: 0, cell: 1 }));
    assert_eq!(next_cell(src, CellPos { row: 3, cell: 1 }), None);
    assert_eq!(cell_below(src, CellPos { row: 0, cell: 1 }), Some(CellPos { row: 2, cell: 1 }));
    assert_eq!(cell_above(src, CellPos { row: 2, cell: 0 }), Some(CellPos { row: 0, cell: 0 }));
    assert_eq!(cell_above(src, CellPos { row: 0, cell: 0 }), None);
    assert_eq!(appended_row(src), "\n|  |  |");
    let empty = "| a |  |\n|---|---|\n";
    assert_eq!(cell_start(empty, CellPos { row: 0, cell: 1 }), Some((0, 6)), "an empty cell: after the pipe and its space");
}

// md_view.rs
#[test]
fn column_widths_cap_shrink_to_fit_in_wrap_mode_and_keep_natural_in_scroll_mode() {
    let w = column_widths(&[100.0, 300.0, 800.0], 1000.0, WideTables::Wrap);
    assert_eq!(w, vec![100.0, 300.0, 450.0], "the cap is 45 percent of the view");
    let w = column_widths(&[600.0, 600.0, 600.0], 1000.0, WideTables::Wrap);
    assert!((w.iter().sum::<f32>() - 1000.0).abs() < 0.01, "shrunk to fit: {w:?}");
    assert!((w[0] - w[1]).abs() < 0.01);
    let w = column_widths(&[600.0, 600.0, 600.0], 1000.0, WideTables::Scroll);
    assert_eq!(w, vec![450.0, 450.0, 450.0], "scroll keeps the capped natural widths");
}
```

- [ ] **Step 2: Run to see them fail**: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- md_table:: md_view::`.
- [ ] **Step 3: Implement** per the Tables constraint.
- [ ] **Step 4: Run, check, commit**: `git commit -m "Tables render as a grid; md_table parses, realigns and walks cells"`.

---

### Task 2: Table editing: cell navigation, the new row, re-alignment on leave

**Files:** modify `viewer.rs` (`key_inner` ~1262 and ~1331, `apply_action` ~2259, `table_entry`,
`note_caret_moved`, the calls after `place_caret` ~1625, `append_paragraph` ~1500, undo and
redo), `md_view.rs` (`navigate` ~1039), `keymap.rs` (`MARKDOWN_KEYS`).

- [ ] **Step 1: Failing tests** (`viewer.rs` tests, state built inline as the existing `apply_action` tests do)

```rust
#[test]
fn tab_walks_the_cells_and_tab_at_the_end_adds_a_row() {
    let text = "intro\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
    let mut d = edit::Document::from_text(text);
    let blocks = md_blocks::split(d.lines());
    let mut c = edit::Caret::at(edit::Pos { line: 2, ch: 2 }); // in "a"
    let now = Instant::now();
    apply_action(&mut d, &mut c, &blocks, KeyAction::Tab, 20, now);
    assert_eq!((c.head.line, c.head.ch), (2, 6), "the next cell's text");
    apply_action(&mut d, &mut c, &blocks, KeyAction::Tab, 20, now);
    assert_eq!((c.head.line, c.head.ch), (4, 2), "over the delimiter row");
    apply_action(&mut d, &mut c, &blocks, KeyAction::ShiftTab, 20, now);
    assert_eq!((c.head.line, c.head.ch), (2, 6));
    let mut c = edit::Caret::at(edit::Pos { line: 2, ch: 2 });
    apply_action(&mut d, &mut c, &blocks, KeyAction::ShiftTab, 20, now);
    assert_eq!((c.head.line, c.head.ch), (2, 2), "Shift+Tab from the first cell stays");
    let mut c = edit::Caret::at(edit::Pos { line: 3, ch: 2 }); // the delimiter row
    apply_action(&mut d, &mut c, &blocks, KeyAction::Tab, 20, now);
    assert_eq!((c.head.line, c.head.ch), (4, 2), "Tab on the delimiter row goes to the next row's first cell, no spaces inserted");
    assert_eq!(d.to_text(), text);
    let mut c = edit::Caret::at(edit::Pos { line: 4, ch: 6 }); // the last cell
    apply_action(&mut d, &mut c, &blocks, KeyAction::Tab, 20, now);
    assert_eq!(d.to_text(), "intro\n\n| a | b |\n|---|---|\n| 1 | 2 |\n|  |  |\n");
    assert_eq!((c.head.line, c.head.ch), (5, 2));
    d.undo();
    assert_eq!(d.to_text(), text);
}

// md_view.rs navigate tests
#[test]
fn up_and_down_move_by_cell_inside_a_table_and_leave_it_at_the_ends() {
    // navigate(doc, blocks, caret, Up/Down, select=false, rows) with the caret at (4, 6): Up lands at (2, 6);
    // at (2, 2) Up falls through to the line move and leaves the block (line 1); Down from (4, 2) lands
    // on line 5 (the blank line after the table); with select = true the table rule does not apply.
}

// viewer.rs: the leave helper
#[test]
fn leaving_an_edited_table_realigns_once_and_an_undo_is_not_realigned_again() {
    let text = "|a|bb|\n|:-|-:|\n|ccc|d|\n";
    let mut d = edit::Document::from_text(text);
    let now = Instant::now();
    assert!(!realign_on_leave(&mut d, 0..3, text, now), "unchanged since entry: nothing");
    let entered = "|a|b|\n|:-|-:|\n|ccc|d|\n"; // the text when the caret entered; the user typed a "b"
    assert!(realign_on_leave(&mut d, 0..3, entered, now));
    assert_eq!(d.to_text(), "| a   | bb |\n|:----|---:|\n| ccc |  d |\n");
    d.undo();
    assert_eq!(d.to_text(), text);
    assert!(!realign_on_leave(&mut d, 0..3, text, now), "after the undo the entry text is the restored text");
}
```

`realign_on_leave(doc, lines: Range<usize>, entry_text: &str, now) -> bool` is the pure core
`note_caret_moved` calls (true when it edited).

- [ ] **Step 2: Run to see them fail**: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- viewer::tests::tab_walks viewer::tests::leaving md_view::tests::up_and_down`.
- [ ] **Step 3: Implement** per the Table editing constraint. `MARKDOWN_KEYS` and its test.
- [ ] **Step 4: Run, check, commit**: `git commit -m "Tables edit as markdown: cells by Tab and arrows, a row from Tab at the end, pipes realigned on leave"`.

---

### Task 3: Remote images, badge rows, image clicks, and the cache sweep

**Files:** create `remote_image.rs`, `cache.rs`; modify `md_view.rs` (`image_element` for remote
urls, badge rows, the press pattern and Cmd+click opening Preview), `md_blocks.rs` (a paragraph
of only images becomes `Inline::Images(Vec<..>)`), `viewer.rs` (`remote`, the queue, the fetch
task started from `fill`), `main.rs` (the sweep).

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn remote_urls_get_a_cache_path_and_curl_arguments_with_the_limits() {
    assert!(is_remote("HTTPS://x.y/a.png") && is_remote("http://x/a") && !is_remote("img/a.png") && !is_remote("file:///a.png"));
    let p = cache_path(Path::new("/c"), "https://x.y/a.PNG?x=1");
    assert_eq!(p.file_name().unwrap().len(), 64, "the sha256 hex, no extension until sniffed");
    assert_ne!(cache_path(Path::new("/c"), "https://x.y/a"), cache_path(Path::new("/c"), "https://x.y/b"));
    let a = curl_args("https://x.y/a.png", Path::new("/c/dest.1.part"));
    assert_eq!(a, ["--disable", "--fail", "--silent", "--show-error", "--location", "--max-redirs", "5", "--proto", "=http,https", "--proto-redir", "=http,https", "--max-time", "15", "--max-filesize", "10485760", "--output", "/c/dest.1.part", "--", "https://x.y/a.png"]);
    assert_eq!(sniff_extension(b"\x89PNG\r\n\x1a\n...."), "png");
    assert_eq!(sniff_extension(b"\xff\xd8\xff\xe0"), "jpg");
    assert_eq!(sniff_extension(b"GIF89a"), "gif");
    assert_eq!(sniff_extension(b"RIFF....WEBPVP8 "), "webp");
    assert_eq!(sniff_extension(b"<svg xmlns"), "svg");
    assert_eq!(preview_args(Path::new("/c/abc.png")), ["-a", "Preview", "/c/abc.png"]);
}

#[test]
fn the_fetch_queue_holds_four_in_flight_and_never_repeats_a_url() {
    let mut q = FetchQueue::default();
    for i in 0..6 { q.want(format!("https://x/{i}")); }
    q.want("https://x/0".into());
    assert_eq!(q.start_ready().len(), 4);
    assert_eq!(q.in_flight(), 4);
    q.done("https://x/0");
    assert_eq!(q.start_ready(), vec!["https://x/4".to_string()]);
    assert!(q.start_ready().is_empty());
}

// md_blocks.rs
#[test]
fn a_paragraph_of_only_images_is_a_badge_row() {
    let inl = inline("![a](a.png) ![b](https://x/b.svg)\n![c](c.png)", &BlockKind::Paragraph);
    assert!(matches!(&inl[0], Inline::Images(v) if v.len() == 3));
    let inl = inline("![a](a.png) text", &BlockKind::Paragraph);
    assert!(!matches!(&inl[0], Inline::Images(_)), "text beside an image is a paragraph");
}

// cache.rs
#[test]
fn the_sweep_removes_only_old_known_files() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("old.png"); let new = dir.path().join("new.png"); let other = dir.path().join("keep.js");
    for p in [&old, &new, &other] { std::fs::write(p, "x").unwrap(); }
    std::fs::create_dir(dir.path().join("profile-1")).unwrap();
    let past = SystemTime::now() - Duration::from_secs(40 * 86_400);
    for p in [&old, &other] { std::fs::File::options().write(true).open(p).unwrap().set_modified(past).unwrap(); }
    assert_eq!(sweep(dir.path(), Duration::from_secs(30 * 86_400)), 1);
    assert!(!old.exists() && new.exists() && other.exists() && dir.path().join("profile-1").is_dir());
    assert_eq!(sweep(&dir.path().join("absent"), Duration::from_secs(1)), 0);
}
```

- [ ] **Step 2: Run to see them fail**: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- remote_image:: cache:: md_blocks::tests::a_paragraph_of_only`.
- [ ] **Step 3: Implement** per the Remote images, Clicks on media and Where things run constraints.
- [ ] **Step 4: Run, check, commit**: `git commit -m "Remote images and badge rows render through a curl cache; Cmd+click opens Preview"`.

---

### Task 4: Video and audio

**Files:** create `media.rs`; modify `md_view.rs` (the poster element, the audio box, the Cmd+click), `viewer.rs` (`posters`, the thumbnail task from `fill`).

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn media_is_told_by_extension_and_the_poster_and_open_arguments_are_fixed() {
    assert_eq!(kind("clip.MOV"), Some(Media::Video));
    assert_eq!(kind("a/b.m4v"), Some(Media::Video));
    assert_eq!(kind("a.webm"), None, "webm is a link");
    assert_eq!(kind("song.m4a"), Some(Media::Audio));
    assert_eq!(kind("a.png"), None);
    assert_eq!(kind("https://x/a.mp4"), None, "remote media is a link");
    let p = poster_path(Path::new("/c"), Path::new("/v/clip.mov"), 1_800_000_000);
    assert_eq!(p.extension().and_then(|e| e.to_str()), Some("png"));
    assert_ne!(p, poster_path(Path::new("/c"), Path::new("/v/clip.mov"), 1_800_000_001), "the mtime is in the key");
    assert_eq!(qlmanage_args(Path::new("/v/clip.mov"), Path::new("/c/job")), ["-t", "-s", "1024", "-o", "/c/job", "/v/clip.mov"]);
    assert_eq!(quicktime_args(Path::new("/v/clip.mov")), ["-a", "QuickTime Player", "/v/clip.mov"]);
    assert_eq!(size_text(950), "950 B"); assert_eq!(size_text(2_500), "2.4 KB"); assert_eq!(size_text(5_300_000), "5.1 MB");
    let dir = tempfile::tempdir().unwrap();
    assert!(thumbnail_in(dir.path()).is_none(), "no png: failed");
    std::fs::write(dir.path().join("clip.mov.png"), "x").unwrap();
    assert_eq!(thumbnail_in(dir.path()), Some(dir.path().join("clip.mov.png")));
}
```

- [ ] **Step 2: Run to see it fail**: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- media::`.
- [ ] **Step 3: Implement** per the Media constraint (the per-job folder, `thumbnail_in` deciding success, the 20 s backstop, one at a time per process).
- [ ] **Step 4: Run, check, commit**: `git commit -m "Video shows a Quick Look poster that opens in QuickTime; audio shows a box that opens"`.

---

### Task 5: Mermaid through headless Chrome

**Files:** create `mermaid.rs`, `crates/arkos/assets/mermaid.min.js`, `scripts/vendor-mermaid.sh`;
modify `md_view.rs` (the diagram states), `viewer.rs` (`diagrams`, the render task from `fill`),
`cache.rs` (the `profile-*` rule), `docs/development.md` (the version; Task 7 may also touch it).

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn the_page_carries_the_theme_the_csp_and_the_escaped_source_and_the_arguments_are_fixed() {
    let t = MermaidTheme { primary: "#2a0062".into(), text: "#f3edff".into(), line: "#f3edff".into(), secondary: "#3b1a86".into(), tertiary: "#330076".into() };
    let html = page("graph TD; A-->B & <C>", &t);
    assert!(html.contains("primaryColor: '#2a0062'") && html.contains("fontFamily: '-apple-system, system-ui'"));
    assert!(html.contains("A--&gt;B &amp; &lt;C&gt;"), "escaped: {html}");
    assert!(html.contains(&format!("<script src=\"./{SCRIPT_NAME}\">")));
    assert!(html.contains("Content-Security-Policy") && html.contains("default-src 'none'"));
    assert!(html.contains("await mermaid.run"));
    assert_eq!(cache_key("a", &t, 2), cache_key("a", &t, 2));
    let t2 = MermaidTheme { primary: "#000000".into(), ..t.clone() };
    assert_ne!(cache_key("a", &t, 2), cache_key("a", &t2, 2), "the values, not a name");
    let c = Path::new("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
    let common = common_args(Path::new("/p"));
    for flag in ["--headless=new", "--disable-gpu", "--no-first-run", "--use-mock-keychain", "--disable-background-networking", "--disable-component-update", "--disable-sync", "--disable-default-apps", "--disable-extensions", "--no-default-browser-check", "--host-resolver-rules=MAP * ~NOTFOUND", "--user-data-dir=/p", "--virtual-time-budget=5000", "--timeout=15000"] {
        assert!(common.contains(&flag.to_string()), "{flag}");
    }
    let d = dump_args(c, Path::new("/p"), Path::new("/k.html"));
    assert!(d.contains(&"--dump-dom".to_string()) && d.last().unwrap() == "file:///k.html");
    let s = shot_args(c, Path::new("/p"), Path::new("/k.html"), Path::new("/k.png"), 640, 480);
    for flag in ["--window-size=640,480", "--screenshot=/k.png", "--force-device-scale-factor=2", "--default-background-color=00000000", "--hide-scrollbars"] {
        assert!(s.contains(&flag.to_string()), "{flag}");
    }
    assert_eq!(size_from_dom("<html><head><title>ok 640 480</title></head></html>"), Ok((640, 480)));
    assert_eq!(size_from_dom("<title>error Parse error on line 2: &lt;x&gt;</title>"), Err("Parse error on line 2: <x>".into()));
    assert!(size_from_dom("<title></title>").is_err());
    assert!(is_diagram_info("mermaid") && is_diagram_info(" mermaid ") && !is_diagram_info("mermaidjs"));
    assert_eq!(SCRIPT_NAME, format!("mermaid-{VERSION}.min.js"));
    assert!(profile_dir(Path::new("/c"), 42).ends_with("profile-42"));
}
```

- [ ] **Step 2: Run to see it fail**: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- mermaid::`.
- [ ] **Step 3: Implement** per the Mermaid constraint. The render task runs the two Chrome
  passes with `std::process::Command` on the background executor with a 20 s backstop each
  (poll `try_wait`, kill on timeout); `scripts/vendor-mermaid.sh` downloads the pinned
  `mermaid@<VERSION>/dist/mermaid.min.js` from the npm registry tarball, checks its sha256
  against the one recorded in the script, and writes the asset (about 3 MB, committed once).
- [ ] **Step 4: Run, check, commit**: `git commit -m "Mermaid blocks render through headless Chrome into a cache; the source shows meanwhile"`.

---

### Task 6: HTML images and details, with the remembered state per file

**Files:** modify `md_blocks.rs` (`BlockKind::Details`, the merge in `split`, `html_image`,
`html_details`, the two inlines), `md_view.rs` (`piece_element`, the renderers, the toggle with
the press pattern), `viewer.rs` (`open_details`, `ViewerEvent::DetailsToggled`, the restore in
`viewer_for` ~2340 and the save in its subscription ~2350), `tree.rs` (`set_details_open`),
`workspace.rs` (`details_open`, `DetailsOpen`, the round-trip test).

- [ ] **Step 1: Failing tests**

```rust
// md_blocks.rs
#[test]
fn a_details_block_with_blank_lines_is_one_block_and_img_and_details_are_recognised() {
    let lines: Vec<String> = "<details>\n<summary>More</summary>\n\nSome *text*\n\n- a\n</details>\n\nafter\n".lines().map(String::from).collect();
    let blocks = split(&lines);
    assert_eq!(blocks.iter().map(|b| &b.kind).collect::<Vec<_>>()[0], &BlockKind::Details);
    assert_eq!(blocks.iter().filter(|b| b.kind == BlockKind::Details).count(), 1);
    let nested: Vec<String> = "<details><summary>O</summary>\n<details>\n<summary>I</summary>\nx\n</details>\n\ny\n</details>\n".lines().map(String::from).collect();
    assert_eq!(split(&nested).iter().filter(|b| b.kind == BlockKind::Details).count(), 1, "the inner details does not close the outer");
    let open: Vec<String> = "<details>\nno end\n\npara\n".lines().map(String::from).collect();
    assert!(split(&open).iter().all(|b| b.kind != BlockKind::Details), "no closing tag: the blocks stay as they were");
    assert_eq!(html_image("<img src=\"a.png\" width=\"300\">"), Some(HtmlImage { src: "a.png".into(), width: Some(300), center: false }));
    assert_eq!(html_image("<p align=\"center\"><img width='3' src='http://x/a.png' /></p>"), Some(HtmlImage { src: "http://x/a.png".into(), width: Some(3), center: true }));
    assert!(html_image("<img src=\"a.png\"><p>x</p>").is_none(), "text beside the tag");
    let d = html_details("<details>\n<summary>More</summary>\n\nSome *text*\n\n- a\n</details>").unwrap();
    assert_eq!((d.summary.as_str(), d.inner.as_str()), ("More", "\nSome *text*\n\n- a\n"));
    assert_eq!(html_details("<details><summary>Same line</summary>\nx\n</details>").unwrap().summary, "Same line");
    assert_eq!(html_details("<details>\nplain\n</details>").unwrap().summary, "Details");
    let inl = inline("<details>\n<summary>S</summary>\n\n| a |\n|---|\n| 1 |\n</details>", &BlockKind::Details);
    assert!(matches!(&inl[0], Inline::Details { summary, blocks } if summary == "S" && blocks.len() == 1), "the leading blank block is dropped");
}

// tree.rs, beside the viewer mode round trip (~757); the workspace file is JSON
#[test]
fn the_open_details_round_trip_per_file_and_an_old_file_has_none() {
    let mut w = Workspace::default();
    w.details_open.insert("/a.md".into(), vec![DetailsOpen { summary: "More".into(), index: 0 }]);
    let text = serde_json::to_string(&w).unwrap();
    assert_eq!(serde_json::from_str::<Workspace>(&text).unwrap().details_open, w.details_open);
    let old = serde_json::to_string(&Workspace::default()).unwrap();
    assert!(!old.contains("details_open"), "skipped when empty");
}
```

- [ ] **Step 2: Run to see them fail**: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- md_blocks::tests::a_details tree::tests::the_open_details`.
- [ ] **Step 3: Implement** per the HTML constraint. About fifteen `Tab::Viewer { path, mode }`
  literals exist (sidebar.rs ~1071/1079/1084, sessions.rs, pane_view.rs ~1475, tree.rs tests):
  the state lives in `Workspace.details_open`, not in `Tab`, so those literals do not change.
- [ ] **Step 4: Run, check, commit**: `git commit -m "HTML img and details render; a details block remembers whether it is open, per file"`.

---

### Task 7: Settings, the CLI key, docs and the gates

**Files:** modify `settings.rs` (the "Wide tables" row through `wide_tables_edit`), `agent_cli.rs`
(`viewer.wide_tables`), `config.rs` (DEFAULT_CONFIG's comment), `docs/guide.md`,
`docs/agent-guide.md`, `docs/development.md`.

- [ ] **Step 1: Failing tests**: `settable("viewer.wide_tables")`; `typed_value("viewer.wide_tables",
  "scroll")` is `Ok` and `"sideways"` is `Err`; `wide_tables_edit(WideTables::Scroll)` builds the
  edit for `["viewer", "wide_tables"]` with the value `scroll`; the DEFAULT_CONFIG round trip
  still parses with the commented key.
- [ ] **Step 2: Implement** and write the docs per the Docs constraint.
- [ ] **Step 3: Gates**: `grep -rn "Used from Task" crates/` empty; no em-dash in docs/*.md,
  README.md, `crates/*/src/**/*.rs`, `scripts/*.sh`; the warning check; the workspace suite.
  `git commit -m "Wide tables in Settings and the CLI; docs for Tier B"`.

---

### Task 8 (after the merge, by hand): the Blogger reshoot

Cyrus (2026-10-16): "Once the Markdown Editor is done, we'll need to reshoot The Blogger
screenshots. It has to showcase our markdown features well and be stylish enough that someone who
works a lot with markdown (like a tech product manager) would be excited to use it."

- The fixture `scripts/shoot/demo/blog` (posts and notes in a product person's voice) gains a
  launch plan with a status table (rendered as a grid), a Mermaid flow of the decision process, a
  screenshot image inline, a `<details>` block of open questions, task checkboxes and a short code
  snippet; the story `scripts/shoot/stories/blogger.toml` opens that post with the caret in a raw
  cell mid-edit and the grid, the diagram and the image all in one frame, in the peach-light look.
- Run `scripts/shoot.sh` for the blogger scene in both appearances, replace
  `docs/images/blogger-dark.png` and `blogger-light.png`, and show Cyrus the frames against the
  bar before they go into the docs.

---

## Deferred, for the user to rule on

- In-place cell editing (a spreadsheet feel) and column insert/delete commands.
- Remote video and audio playback; a built-in player; `.webm` beyond a browser link.
- Mermaid rendered without Chrome; a Mermaid editor; theme variables beyond the five mapped.
- Nested `<details>` rendering (it shows as source); other HTML (`<video>`, HTML tables).
- Math.
- The 5 MB incremental splitter (milestone 7's Task 2b) and tables over 500 rows (shown as
  source) stay gated on the hand check.

## Self-review notes

- Rulings on the plan reviewer's questions (Cyrus to confirm at approval): (A) the table keys and
  the re-alignment apply in Rendered mode only; (B) a plain click on an image or poster places the
  caret and Cmd+click opens it, as links work, which changes the spec's "a click opens" to keep the
  viewer's one rule for opening things; (C) badge rows (several images in one paragraph) render
  in a row; (D) `.webm` is a link opened in the browser; (E) an `<img>` wrapped in `<p>`, `<div>`
  or `<a>` renders, centred when the wrapper says so; (F) the `<details>` state is per file in the
  workspace, so any tab on that file remembers it; (G) Chrome runs with background networking
  disabled and a page CSP, which is what makes "nothing leaves the machine" true; Chrome stays the
  renderer until Cyrus says otherwise.
- Spec section 3: Tasks 1 and 2. Section 4: Task 3. Section 5: Task 4. Section 6: Task 5.
  Section 7: Task 6. Section 8: Tasks 1, 6 and 7. Section 9: the tests above. Task 8 is the
  reshoot Cyrus asked for on 2026-10-16.
- Deviations from the spec's letter: the cache ages (30, 30 and 90 days); `appended_row` gives
  `|  |  |` so each cell has a caret place; the 45 percent cap applies in Scroll mode too; four
  fetches and one render per process (a window is a process); a per-process throwaway profile;
  the plain-click rule (B) above; the spec's scope table still mentions an off switch that
  ruling 7 removed.
