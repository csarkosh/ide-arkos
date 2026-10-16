# Markdown rendering Tier B (milestone 9)

Status: plan for review. Spec: `docs/superpowers/specs/2026-10-16-markdown-rendering-tier-b-design.md`
(reviewed by Cyrus on 2026-10-16; its nine rulings bind). Written against main at `7f48b5e`.
Merges without a release; ships with milestones 8b and the analytics work in one release.

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
- No em-dashes in any file. Comments are full sentences. Headless tests only: no test opens a
  window, runs curl, qlmanage, Chrome or `open`, or touches the real Application Support; every
  process invocation goes through an argument builder that tests check as data.
- **Where things run.** Fetches, thumbnails and diagram renders run on the background executor
  (`cx.background_executor().spawn`, as `viewer.rs` ~917 and ~1017 do) and land through
  `viewer.update`; the UI thread never blocks on a process. Each has a cache under
  `paths::app_support().join("cache")` (`images/`, `posters/`, `mermaid/`), created on demand; a
  launch sweep (`cache::sweep(dir, max_age)`, run once from `main()` on the background executor)
  removes entries older than 30 days in `images/` and `mermaid/` and 90 days in `posters/`.
- **Tables** (`crates/arkos/src/md_table.rs`, new): `pub struct Table { pub align: Vec<Align>,
  pub header: Vec<String>, pub rows: Vec<Vec<String>> }`, `pub enum Align { Left, Center, Right }`;
  `pub fn parse(source: &str) -> Option<Table>` per spec section 3 (a leading and trailing pipe
  optional, `\|` a literal pipe, short rows padded, long rows truncated, `None` on a missing or
  malformed delimiter row); `pub fn realign(source: &str) -> Option<String>` (cells padded to
  the column width by display width (`unicode-width`, already a dependency: check; else char
  count), the delimiter row keeps its colons and stretches its dashes to the width (at least three),
  one space inside each pipe, a leading and trailing pipe always, lines joined with `\n` and the
  source's trailing newline kept; idempotent; `None` when `parse` fails). Navigation over the
  block's source lines and a caret `(line, col)` relative to the block: `pub struct CellPos {
  pub row: usize, pub cell: usize }` where `row` counts source lines (the delimiter row is row 1
  and is never a destination); `pub fn cell_at(source, line, col) -> Option<CellPos>`; `pub fn
  cell_start(source, pos) -> Option<(usize, usize)>` (the line and the column of the cell's first
  text character, or of the position after the pipe when the cell is empty); `pub fn next_cell`,
  `prev_cell`, `cell_above`, `cell_below(source, pos) -> Option<CellPos>` (across rows, skipping
  the delimiter row; `None` past the ends); `pub fn appended_row(source) -> String` (the text to
  append: `\n| ` plus ` | ` joins plus ` |` for the header's cell count). Rendering (`md_view.rs`):
  `Piece::Table` carries the parsed `Table` (or the source when `parse` fails, drawn as today):
  a grid of rows of cells; the header row semibold on `chrome.tab_inactive`; body rows alternate
  `window_bg` and `chrome.text` at 4 percent; 1 px `chrome.border` hairlines; 6 px by 10 px cell
  padding; alignment per column; cell text through the existing inline runs (an image inline shows
  its alt text). Widths: `pub fn column_widths(natural: &[f32], view: f32, mode: WideTables) ->
  Vec<f32>` (pure, tested): each column its widest cell's natural width capped at 45 percent of
  the view; in `Wrap` mode, when the sum exceeds the view, shrink proportionally to the view and
  cells wrap; in `Scroll` mode keep the capped natural widths and the grid scrolls sideways inside
  the block (`overflow_x_scroll` on the grid's container with a `ScrollHandle` per block kept in
  the viewer's `block_cache` entry; Shift+wheel and the horizontal axis scroll it; the block's
  height is the grid's height, measured as other blocks are). Natural widths come from the
  text system (`shape_line` of each cell's plain text at the body font size) at measure time.
  A click on a cell: the grid's cell `on_mouse_down` carries `(row, cell)` and the x offset within
  the cell; the viewer maps it to the source position through `cell_start` plus the inline hit
  test the paragraphs use (`code_pos`-style index for the cell's text) and makes the block raw with
  the caret there.
- **Table editing** (`viewer.rs`): in `apply_action` (~2259), when `blocks_before[block_at(caret)]`
  is a `Table`, `Tab` and `ShiftTab` move to `next_cell`/`prev_cell` (the caret at `cell_start`),
  `Tab` from the last cell of the last row inserts `appended_row` at the block's end as one
  undoable edit and lands in the new row's first cell; `Move(Line up/down)` (the existing Up and
  Down) moves to `cell_above`/`cell_below`; at the first or last row they fall through to the
  existing line move, which leaves the block. The viewer records `table_edited: bool` while a
  table block is active (set by any edit inside it); when the active block changes (the one place
  the viewer notices the caret leaving a block: find where `active_block()` is compared to the
  previous one for the raw-to-rendered flip and the `rendered` re-split) and the block was a
  table that was edited, `realign` runs on the block's source and, when it returns a changed
  string, `edit_elsewhere` replaces the block's lines with it as one undo step (the caret stays
  where the user clicked). A broken table is left as typed. `Enter` is unchanged.
- **Remote images** (`crates/arkos/src/remote_image.rs`, new): `pub fn is_remote(path) -> bool`
  (`http://` or `https://`, case-insensitive); `pub fn cache_path(dir, url) -> PathBuf`
  (`<dir>/<sha256 hex of url>` plus the url's extension when it is png, jpg, jpeg, gif, webp or
  svg, else no extension); `pub fn curl_args(url, dest) -> Vec<String>` (`--fail --silent
  --show-error --location --max-redirs 5 --proto =http,https --max-time 15 --max-filesize
  10485760 --output <dest> -- <url>`; no cookies, no referer by default in curl); the fetch:
  `Viewer.remote: HashMap<String, Remote>` with `Remote::{Fetching, Ready(PathBuf), Failed}`; the
  first render of a remote image not in the map checks the cache file (ready at once when it
  exists), else queues a fetch; at most four fetches in flight per viewer (a `VecDeque` of
  pending urls); a fetch runs `/usr/bin/curl` on the background executor through
  `std::process::Command` with the args above into `<dest>.part` then renames; the result lands
  through `viewer.update` and `cx.notify()`. The image element: `Ready` draws `img(path)` at
  most the view's width; `Fetching` the alt box with the alt text and "loading"; `Failed` the
  alt text and the url. A click on a rendered image (local or ready remote) opens the file with
  `cx.open_with_system(&path)`. Always on (ruling 7).
- **Media** (`crates/arkos/src/media.rs`, new): `pub enum Media { Video, Audio }`; `pub fn kind(path)
  -> Option<Media>` by extension (`mp4 mov m4v webm` video; `mp3 m4a wav aac` audio;
  case-insensitive; remote urls give `None`); `pub fn poster_path(dir, file, mtime) -> PathBuf`
  (sha256 of the path and the mtime's unix seconds, `.png`); `pub fn qlmanage_args(file, out_dir)
  -> Vec<String>` (`-t -s 1024 -o <out_dir> -- <file>`; qlmanage names the output `<file
  name>.png` in `out_dir`, so the fetch renames it to `poster_path`); `pub fn quicktime_args(file)
  -> Vec<String>` (`-a "QuickTime Player" -- <file>` for `/usr/bin/open`). The viewer keeps
  `posters: HashMap<PathBuf, Poster::{Making, Ready(PathBuf), Failed}>` with the same background
  pattern as remote images (one at a time). Rendering: a video paragraph draws the poster at most
  the view's width with a centred play glyph (a 40 px disc at `chrome.text` 60 percent with a
  triangle in `window_bg`); `Making` or `Failed` draws the alt box (the alt text, or the file
  name); a click runs `open` with `quicktime_args`. Audio draws a one-line box: a speaker glyph
  (`\u{1F50A}` in the UI font), the file name and its size (`metadata().len()` formatted as KB or
  MB); a click opens with the system. A remote video or audio url draws a link (the alt text
  with the url) that opens in the browser on click.
- **Mermaid** (`crates/arkos/src/mermaid.rs`, new): a fenced block with info string `mermaid`
  (exactly, after trimming) is a diagram block. `pub fn page(source: &str, theme: &MermaidTheme,
  max_width_px: u32) -> String` builds the HTML: a `<script>` tag loading `./mermaid.min.js`
  (written beside the page), `mermaid.initialize({ startOnLoad: false, theme: 'base',
  themeVariables: { background: 'transparent', primaryColor, primaryTextColor, lineColor,
  secondaryColor, tertiaryColor, fontFamily } })`, the source inside `<pre class="mermaid">`
  HTML-escaped, a script that renders, sets `document.title` to `ok <width> <height>` or `error
  <first line of the message>`, and sizes the body to the SVG; `pub struct MermaidTheme {
  primary, text, line, secondary, tertiary: String (hex), font: String }` from the chrome
  colours (`tab_inactive`, `text`, `text`, `banner`, `window_bg`) and the UI font; `pub fn
  cache_key(source, theme_name, scale) -> String` (sha256 hex); `pub fn chrome_app() ->
  Option<PathBuf>` (`/Applications/Google Chrome.app` or `~/Applications/Google Chrome.app`,
  whichever exists; the binary at `Contents/MacOS/Google Chrome`); `pub fn dump_args(chrome, profile,
  page) -> Vec<String>` (`--headless=new --disable-gpu --no-first-run --user-data-dir=<profile>
  --dump-dom <file url>`), `pub fn shot_args(chrome, profile, page, out, w, h) -> Vec<String>`
  (`--headless=new --disable-gpu --hide-scrollbars --no-first-run --force-device-scale-factor=2
  --user-data-dir=<profile> --window-size=<w>,<h> --screenshot=<out> <file url>`); `pub fn
  size_from_dom(dom: &str) -> Result<(u32, u32), String>` reads the title (`ok w h`, or the error
  text). The render: on the background executor, one at a time per viewer, never for the active
  block: write `mermaid.min.js` to `<cache>/mermaid/mermaid-<version>.min.js` once (from
  `include_bytes!("../assets/mermaid.min.js")`, vendored at a pinned version by
  `scripts/vendor-mermaid.sh`, recorded in `docs/development.md`), write the page to
  `<cache>/mermaid/<key>.html`, run the dump pass, parse the size, run the screenshot pass into
  `<key>.png`, delete the page; the result lands through `viewer.update`. Rendering states per
  block (`Viewer.diagrams: HashMap<String, Diagram::{Rendering, Ready(PathBuf), Error(String),
  NoChrome}>` keyed by the cache key): `Ready` draws the PNG at most the view's width (the PNG is
  2x, drawn at half its pixel size); `Rendering` draws the source in the code-block style with
  "rendering" in the info line; `Error` the source with the error line in `tree.deleted` under the
  info line; `NoChrome` the source with "diagram: install Google Chrome to render" in the info line
  (the check runs once per viewer). A cache hit at first render skips the process. The profile
  dir is `<cache>/mermaid/profile`, created once; `--user-data-dir` keeps Chrome off the user's
  profile.
- **HTML** (`md_blocks.rs`, `md_view.rs`): `pub fn html_image(block: &str) -> Option<HtmlImage {
  src: String, width: Option<u32> }>` recognises a block that is one `<img ...>` tag (attributes
  in any order, single or double quotes, optional `/>`); `pub fn html_details(block: &str) ->
  Option<Details { summary: String, inner: String }>` recognises a block starting with `<details`
  and ending with `</details>` whose `<summary>` (optional; "Details" when absent) is on the
  first lines, `inner` being the lines between the summary and the closing tag. `Inline::HtmlImage`
  renders as an image by the image rules (width honoured when smaller than the view);
  `Inline::Details { summary, blocks: Vec<(BlockKind, Vec<Inline>)> }` renders a row with a
  disclosure triangle (`\u{25b8}` closed, `\u{25be}` open) and the summary in `chrome.text`, then,
  when open, the inner blocks rendered read-only through the same block renderer (nested tables,
  lists and code included; no nested `<details>`); a click on the row toggles; a click on the
  content makes the whole block raw. The open state: `Viewer.open_details: BTreeSet<(String,
  usize)>` (summary, index among the file's details blocks in order), toggled by the click, saved
  into the workspace file as `Tab::Viewer { open_details: Vec<(String, usize)> }` (serde default
  empty) through the existing tab save path, restored at open.
- **Config and Settings**: `ViewerConfig` gains `wide_tables: WideTables` (`#[serde(rename_all =
  "lowercase")] pub enum WideTables { #[default] Wrap, Scroll }`); DEFAULT_CONFIG gains the
  commented key under `[viewer]` with one line of explanation; `agent_cli::SETTABLE` gains
  `viewer.wide_tables` (values `wrap` | `scroll`, typed as a string, validated); the Settings
  window's Text section gains, after the steppers, a heading "Wide tables" with segments "Wrap"
  and "Scroll" writing `["viewer", "wide_tables"]`; the viewer reads `config.viewer.wide_tables`
  through `md_view::render`'s arguments (add it next to `highlight_limit`) so a config reload
  applies at the next frame.
- **Docs**: `docs/guide.md` "Editing markdown" gains tables (the grid, the four keys, the
  re-alignment and its undo, the new row from Tab, Wrap or Scroll), remote images (always on, what
  is fetched, the cache), video and audio, Mermaid (Chrome required, the cache, nothing leaves the
  machine), `<img>` and `<details>` (the remembered state); `docs/agent-guide.md` gains one
  sentence that `viewer.wide_tables` is settable; `docs/development.md` gains the Status rows from
  spec section 8 and the vendored mermaid version; the Keybindings window's Viewer section gains
  the four table keys in `keymap::MARKDOWN_KEYS` ("Next cell" Tab, "Previous cell" Shift+Tab,
  "Cell above" Up, "Cell below" Down, each noted "in a table").

## Review Focus

1. A table with `\|` in a cell and a missing trailing pipe parses, renders, realigns and parses
   again unchanged (Task 1).
2. Tab in the last cell of the last row appends a row and lands in it; undo removes the row;
   leaving the block realigns once and undo restores the typed text (Task 2).
3. A remote image's fetch cannot block the UI, cannot exceed 10 MB, follows no redirect to a
   non-http scheme, and a second render of the same url while fetching does not start a second
   curl (Task 3).
4. A Mermaid block that is active never launches Chrome; leaving it renders once; the same
   source and theme never renders twice (Task 5).
5. A `<details>` block's state is in the workspace file after a toggle and back after a relaunch
   with the same summary, and survives an unrelated edit above it (Task 6).

## File structure

- Create: `crates/arkos/src/md_table.rs`, `remote_image.rs`, `media.rs`, `mermaid.rs`, `cache.rs`
  (the sweep and the cache root), `crates/arkos/assets/mermaid.min.js` (vendored),
  `scripts/vendor-mermaid.sh`.
- Modify: `md_view.rs`, `md_blocks.rs`, `viewer.rs`, `tree.rs` (the `Tab::Viewer` field),
  `workspace.rs` (nothing beyond serde, verify), `config.rs`, `settings.rs`, `agent_cli.rs`,
  `keymap.rs` (`MARKDOWN_KEYS`), `main.rs` (the sweep at launch; the `WideTables` argument),
  `docs/guide.md`, `docs/agent-guide.md`, `docs/development.md`.

---

### Task 1: Table parsing, re-alignment, navigation, and the grid

**Files:** create `md_table.rs`; modify `md_view.rs` (`Piece::Table(TablePiece)`, the grid, `column_widths`, the cell click), `config.rs` (`WideTables` and the key; the Settings row is Task 7), `md_blocks.rs` (nothing: the block's source still arrives as `Inline::Table(String)`; `md_view` parses it).

**Interfaces:** as the Tables constraint states. `WideTables` lives in `config.rs` now (Task 7 wires Settings and the CLI). `md_view::render` gains `wide_tables: WideTables`.

- [ ] **Step 1: Failing tests** (`md_table.rs`)

```rust
#[test]
fn a_table_parses_with_optional_outer_pipes_escaped_pipes_and_ragged_rows() {
    let t = parse("| a | b |\n|:--|--:|\n| 1 | 2 |\nx | y \\| z | extra\n| only |\n").unwrap();
    assert_eq!(t.align, vec![Align::Left, Align::Right]);
    assert_eq!(t.header, vec!["a", "b"]);
    assert_eq!(t.rows, vec![vec!["1", "2"], vec!["x", "y | z"], vec!["only", ""]]);
    assert!(parse("| a |\n| 1 |\n").is_none(), "no delimiter row");
    assert!(parse("| a |\n|-x-|\n").is_none(), "a malformed delimiter row");
    assert_eq!(parse("a|b\n-|-\n").unwrap().header, vec!["a", "b"]);
}

#[test]
fn realign_pads_every_cell_keeps_the_colons_and_is_idempotent() {
    let src = "|a|bb|\n|:-|-:|\n|ccc|d|\n";
    let out = realign(src).unwrap();
    assert_eq!(out, "| a   | bb |\n|:----|---:|\n| ccc |  d |\n");
    assert_eq!(realign(&out).unwrap(), out);
    assert!(realign("| a |\n| 1 |\n").is_none());
    assert_eq!(realign("| a |\n|---|\n| 1 |").unwrap(), "| a |\n|---|\n| 1 |", "no trailing newline stays so");
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
- [ ] **Step 3: Implement** per the constraint. The grid is a `div().flex().flex_col()` of rows, each a `flex()` of cells with the computed widths (`w(px(..))`, `min_w_0`, wrapping text in Wrap mode; `whitespace_nowrap` in Scroll mode); the cell mouse-down carries `(row, cell)` and the local x (`ev.position.x - bounds.origin.x`, as the paragraph hit test does) to `viewer.update(|v, cx| v.click_table_cell(block, row, cell, x, cx))`, which Task 2 fills (here it makes the block raw with the caret at `cell_start`). Measuring natural widths uses the text system at the block's measure step (find where paragraphs are shaped for height). The Scroll mode's `ScrollHandle` lives in the `Rendered` entry.
- [ ] **Step 4: Run, check, commit**: `git commit -m "Tables render as a grid; md_table parses, realigns and walks cells"`.

---

### Task 2: Table editing: cell navigation, the new row, re-alignment on leave

**Files:** modify `viewer.rs` (`apply_action` ~2259; the active-block change; `table_edited`; `click_table_cell`), `keymap.rs` (`MARKDOWN_KEYS` gains the four rows).

- [ ] **Step 1: Failing tests** (`viewer.rs` tests, through `apply_action` with a document holding a table and `blocks_before` from `md_blocks::split`)

```rust
#[test]
fn tab_and_arrows_walk_the_cells_and_tab_at_the_end_adds_a_row() {
    let text = "intro\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
    let (mut d, mut c, blocks, now) = fixture(text, (2, 2)); // caret in "a"
    apply_action(&mut d, &mut c, &blocks, KeyAction::Tab, 20, now);
    assert_eq!((c.line, c.col), (2, 6), "the next cell's text");
    apply_action(&mut d, &mut c, &blocks, KeyAction::Tab, 20, now);
    assert_eq!((c.line, c.col), (4, 2), "over the delimiter row");
    apply_action(&mut d, &mut c, &blocks, KeyAction::Move(edit::Move::Up, false), 20, now);
    assert_eq!((c.line, c.col), (2, 2), "the cell above, not the delimiter row");
    apply_action(&mut d, &mut c, &blocks, KeyAction::ShiftTab, 20, now);
    assert_eq!((c.line, c.col), (0, 0).max((2, 2)).min((2, 2)), "Shift+Tab from the first cell stays");
    let (mut d, mut c, blocks, now) = fixture(text, (4, 6)); // last cell
    apply_action(&mut d, &mut c, &blocks, KeyAction::Tab, 20, now);
    assert_eq!(d.to_text(), "intro\n\n| a | b |\n|---|---|\n| 1 | 2 |\n|  |  |\n");
    assert_eq!((c.line, c.col), (5, 2));
    d.undo(now); // one step
    assert_eq!(d.to_text(), text);
    let (mut d, mut c, blocks, now) = fixture(text, (4, 2));
    let out = apply_action(&mut d, &mut c, &blocks, KeyAction::Move(edit::Move::Down, false), 20, now);
    assert_eq!(c.line, 5, "Down from the last row leaves the block as a line move");
    let _ = out;
}

#[test]
fn leaving_an_edited_table_realigns_once_as_one_undo_step() {
    // Through the viewer's pure helper: `realign_on_leave(doc, block_lines, edited) -> Option<edit>`
    let text = "|a|bb|\n|:-|-:|\n|ccc|d|\n";
    let mut d = edit::Document::from_text(text);
    assert!(realign_on_leave(&mut d, 0..3, false, now()).is_none(), "not edited: nothing");
    let changed = realign_on_leave(&mut d, 0..3, true, now()).unwrap();
    assert_eq!(d.to_text(), "| a   | bb |\n|:----|---:|\n| ccc |  d |\n");
    assert!(changed);
    assert!(realign_on_leave(&mut d, 0..3, true, now()).is_none(), "already aligned: no edit");
    d.undo(now());
    assert_eq!(d.to_text(), text);
}
```

(`fixture` is the existing helper the `apply_action` tests use; adapt the names to it.)

- [ ] **Step 2: Run to see them fail**: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- viewer::tests::tab_and viewer::tests::leaving`.
- [ ] **Step 3: Implement** per the Table editing constraint. `apply_action` checks the block kind
  before the list rules (a table block is never a list). `realign_on_leave` is called where the
  active block changes; `table_edited` is set in the edit path when the active block is a table
  and cleared when the active block changes. `click_table_cell` sets the caret from `cell_start`
  plus the hit column and makes the block raw. `MARKDOWN_KEYS` and its test.
- [ ] **Step 4: Run, check, commit**: `git commit -m "Tables edit as markdown: cells by Tab and arrows, a row from Tab at the end, pipes realigned on leave"`.

---

### Task 3: Remote images and the cache sweep

**Files:** create `remote_image.rs`, `cache.rs`; modify `md_view.rs` (`image_element` for remote urls, the click), `viewer.rs` (`remote`, the queue, the fetch task), `main.rs` (the sweep at launch).

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn remote_urls_get_a_cache_path_and_curl_arguments_with_the_limits() {
    assert!(is_remote("HTTPS://x.y/a.png") && is_remote("http://x/a") && !is_remote("img/a.png") && !is_remote("file:///a.png"));
    let p = cache_path(Path::new("/c"), "https://x.y/a.PNG?x=1");
    assert_eq!(p.extension().and_then(|e| e.to_str()), Some("png"));
    assert_eq!(p.file_stem().unwrap().len(), 64 + 0, "the sha256 hex");
    assert!(cache_path(Path::new("/c"), "https://x.y/a").extension().is_none());
    assert_ne!(cache_path(Path::new("/c"), "https://x.y/a"), cache_path(Path::new("/c"), "https://x.y/b"));
    let a = curl_args("https://x.y/a.png", Path::new("/c/dest.part"));
    assert_eq!(a, ["--fail", "--silent", "--show-error", "--location", "--max-redirs", "5", "--proto", "=http,https", "--proto-redir", "=http,https", "--max-time", "15", "--max-filesize", "10485760", "--output", "/c/dest.part", "--", "https://x.y/a.png"]);
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

// cache.rs
#[test]
fn the_sweep_removes_only_old_files() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("old"); let new = dir.path().join("new");
    std::fs::write(&old, "x").unwrap(); std::fs::write(&new, "x").unwrap();
    let past = SystemTime::now() - Duration::from_secs(40 * 86_400);
    filetime_set(&old, past); // use `File::set_modified` (std 1.75+) on the handle
    assert_eq!(sweep(dir.path(), Duration::from_secs(30 * 86_400)), 1);
    assert!(!old.exists() && new.exists());
    assert_eq!(sweep(&dir.path().join("absent"), Duration::from_secs(1)), 0, "a missing folder is fine");
}
```

- [ ] **Step 2: Run to see them fail**: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- remote_image:: cache::`.
- [ ] **Step 3: Implement** per the constraint. The fetch task: `cx.background_executor().spawn(async move { run curl; rename .part })` then `viewer.update(cx, |v, cx| { v.remote.insert(url, Ready or Failed); v.queue.done(&url); start the next; cx.notify() })`. `image_element` consults `ctx.viewer.read(cx).remote` for a remote path (the element builder has `cx` where the pieces are built: check `block_element`'s signature and thread the state through `BlockCtx` as a snapshot of `remote` for the frame). `main()` runs `cache::sweep_all()` once on the background executor after the window opens.
- [ ] **Step 4: Run, check, commit**: `git commit -m "Remote images render: a curl fetch into a cache, four at a time, swept after 30 days"`.

---

### Task 4: Video and audio

**Files:** create `media.rs`; modify `md_view.rs` (the poster element, the audio box, the clicks), `viewer.rs` (`posters`, the thumbnail task).

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn media_is_told_by_extension_and_the_poster_and_open_arguments_are_fixed() {
    assert_eq!(kind("clip.MOV"), Some(Media::Video));
    assert_eq!(kind("a/b.webm"), Some(Media::Video));
    assert_eq!(kind("song.m4a"), Some(Media::Audio));
    assert_eq!(kind("a.png"), None);
    assert_eq!(kind("https://x/a.mp4"), None, "remote media is a link");
    let p = poster_path(Path::new("/c"), Path::new("/v/clip.mov"), 1_800_000_000);
    assert_eq!(p.extension().and_then(|e| e.to_str()), Some("png"));
    assert_ne!(p, poster_path(Path::new("/c"), Path::new("/v/clip.mov"), 1_800_000_001), "the mtime is in the key");
    assert_eq!(qlmanage_args(Path::new("/v/clip.mov"), Path::new("/c/tmp")), ["-t", "-s", "1024", "-o", "/c/tmp", "--", "/v/clip.mov"]);
    assert_eq!(quicktime_args(Path::new("/v/clip.mov")), ["-a", "QuickTime Player", "--", "/v/clip.mov"]);
    assert_eq!(size_text(950), "950 B"); assert_eq!(size_text(2_500), "2.4 KB"); assert_eq!(size_text(5_300_000), "5.1 MB");
}
```

- [ ] **Step 2: Run to see it fail**: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- media::`.
- [ ] **Step 3: Implement** per the Media constraint, with the same background pattern as Task 3 (one thumbnail at a time; `qlmanage` writes `<name>.png` into a per-job temp dir under the cache, then the file is renamed to `poster_path`).
- [ ] **Step 4: Run, check, commit**: `git commit -m "Video shows a Quick Look poster that opens in QuickTime; audio shows a box that opens"`.

---

### Task 5: Mermaid through headless Chrome

**Files:** create `mermaid.rs`, `crates/arkos/assets/mermaid.min.js` (vendored by `scripts/vendor-mermaid.sh` from the pinned npm tarball; the version in `mermaid::VERSION` and in `docs/development.md`), `scripts/vendor-mermaid.sh`; modify `md_view.rs` (the diagram states), `viewer.rs` (`diagrams`, the render task, the active-block rule), `Cargo.toml`/`build.rs` if `include_bytes!` needs a path note.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn the_page_carries_the_theme_and_the_escaped_source_and_the_arguments_are_fixed() {
    let t = MermaidTheme { primary: "#2a0062".into(), text: "#f3edff".into(), line: "#f3edff".into(), secondary: "#3b1a86".into(), tertiary: "#330076".into(), font: "Helvetica".into() };
    let html = page("graph TD; A-->B & <C>", &t, 800);
    assert!(html.contains("primaryColor: '#2a0062'") && html.contains("fontFamily: 'Helvetica'"));
    assert!(html.contains("A--&gt;B &amp; &lt;C&gt;"), "escaped: {html}");
    assert!(html.contains("<script src=\"./mermaid.min.js\">"));
    assert!(html.contains("max-width: 800px"));
    assert_eq!(cache_key("a", "navy", 2), cache_key("a", "navy", 2));
    assert_ne!(cache_key("a", "navy", 2), cache_key("a", "navy-light", 2));
    let c = Path::new("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
    assert_eq!(dump_args(c, Path::new("/p"), Path::new("/k.html"))[..4], ["--headless=new", "--disable-gpu", "--no-first-run", "--user-data-dir=/p"]);
    let s = shot_args(c, Path::new("/p"), Path::new("/k.html"), Path::new("/k.png"), 640, 480);
    assert!(s.contains(&"--window-size=640,480".to_string()) && s.contains(&"--screenshot=/k.png".to_string()) && s.contains(&"--force-device-scale-factor=2".to_string()));
    assert_eq!(size_from_dom("<html><head><title>ok 640 480</title></head></html>"), Ok((640, 480)));
    assert_eq!(size_from_dom("<title>error Parse error on line 2</title>"), Err("Parse error on line 2".into()));
    assert!(size_from_dom("<title></title>").is_err());
    assert_eq!(is_diagram_info("mermaid"), true); assert_eq!(is_diagram_info(" mermaid "), true); assert_eq!(is_diagram_info("mermaidjs"), false);
}
```

- [ ] **Step 2: Run to see it fail**: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- mermaid::`.
- [ ] **Step 3: Implement** per the Mermaid constraint. The render task runs the two Chrome passes with `std::process::Command` on the background executor (a 20 second timeout each through `wait_timeout`-style polling or `try_wait` in a loop, killing on timeout); the viewer starts a render only when the block is not the active one and no render is running; the active-block change triggers a check of diagram blocks needing a render. `scripts/vendor-mermaid.sh` downloads the pinned `mermaid@<version>/dist/mermaid.min.js` from the npm registry tarball, checks its sha256 against the one recorded in the script, and writes the asset; the committed asset is the one in the repo (about 3 MB; one file).
- [ ] **Step 4: Run, check, commit**: `git commit -m "Mermaid blocks render through headless Chrome into a cache; the source shows meanwhile"`.

---

### Task 6: HTML images and details, with the remembered state

**Files:** modify `md_blocks.rs` (`html_image`, `html_details`, the two inlines), `md_view.rs` (the renderers), `viewer.rs` (`open_details`, the toggle, the save and restore), `tree.rs` (`Tab::Viewer { open_details }`), `workspace.rs` (a round-trip test).

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn an_html_block_that_is_one_img_or_a_details_is_recognised() {
    assert_eq!(html_image("<img src=\"a.png\" width=\"300\">"), Some(HtmlImage { src: "a.png".into(), width: Some(300) }));
    assert_eq!(html_image("<img width='3' src='http://x/a.png' />"), Some(HtmlImage { src: "http://x/a.png".into(), width: Some(3) }));
    assert!(html_image("<img src=\"a.png\"><p>x</p>").is_none(), "more than one tag");
    let d = html_details("<details>\n<summary>More</summary>\n\nSome *text*\n\n- a\n</details>").unwrap();
    assert_eq!(d.summary, "More");
    assert_eq!(d.inner, "\nSome *text*\n\n- a\n");
    assert_eq!(html_details("<details>\nplain\n</details>").unwrap().summary, "Details");
    assert!(html_details("<details>\nno end").is_none());
    let inl = inline("<details>\n<summary>S</summary>\n\n| a |\n|---|\n| 1 |\n</details>", &BlockKind::Html);
    assert!(matches!(&inl[0], Inline::Details { summary, blocks } if summary == "S" && blocks.len() == 1));
}

// workspace.rs
#[test]
fn a_viewer_tab_keeps_its_open_details() {
    let t = Tab::Viewer { path: "/a.md".into(), mode: ViewMode::Rendered, open_details: vec![("More".into(), 0)] };
    let text = serde_json::to_string(&t).unwrap();
    assert_eq!(serde_json::from_str::<Tab>(&text).unwrap(), t);
    let old = r#"{"Viewer":{"path":"/a.md"}}"#;
    assert!(matches!(serde_json::from_str::<Tab>(old).unwrap(), Tab::Viewer { open_details, .. } if open_details.is_empty()));
}
```

(Use the workspace file's actual format: check whether tabs are TOML or JSON in `workspace.rs` and write the round trip through the same serializer.)

- [ ] **Step 2: Run to see them fail**: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- md_blocks::tests::an_html workspace::`.
- [ ] **Step 3: Implement** per the HTML constraint. The details' inner markdown is split with `md_blocks::split` and each block's inlines built with `inline`, rendered through `block_element` recursively with `is_nested` so a nested details renders as source. The viewer's `open_details` is loaded from the tab at open (`Tab::Viewer`'s field reaches the viewer where `mode` does) and written back where `mode` is saved (`tree.rs` ~432). The index among details blocks is computed in document order at render time.
- [ ] **Step 4: Run, check, commit**: `git commit -m "HTML img and details render; a details block remembers whether it is open"`.

---

### Task 7: Settings, the CLI key, docs and the gates

**Files:** modify `settings.rs` (the "Wide tables" row), `agent_cli.rs` (`viewer.wide_tables` in `SETTABLE` and `typed_value`), `config.rs` (DEFAULT_CONFIG's comment), `docs/guide.md`, `docs/agent-guide.md`, `docs/development.md`.

- [ ] **Step 1: Failing tests**: `settable("viewer.wide_tables")`; `typed_value("viewer.wide_tables", "scroll")` is `Ok` and `"sideways"` is `Err`; the DEFAULT_CONFIG round trip still parses with the commented key; a settings test that the Text section's segment writes `["viewer","wide_tables"]` (through the existing segment-row test pattern if one exists, else the pure `edit` builder).
- [ ] **Step 2: Implement** and write the docs per the Docs constraint.
- [ ] **Step 3: Gates**: `grep -rn "Used from Task" crates/` empty; no em-dash in docs/*.md, README.md, `crates/*/src/**/*.rs`, `scripts/*.sh`; the warning check; the workspace suite. `git commit -m "Wide tables in Settings and the CLI; docs for Tier B"`.

---

## Deferred, for the user to rule on

- In-place cell editing (a spreadsheet feel) and column insert/delete commands.
- Remote video and audio playback; a built-in player.
- Mermaid rendered without Chrome (a Rust renderer does not exist); a Mermaid editor; theme
  variables beyond the five mapped.
- Nested `<details>`; other HTML (tables in HTML, `<video>` tags).
- Math.
- The 5 MB incremental splitter (milestone 7's Task 2b) stays gated on the hand check.

## Self-review notes

- Spec section 3: Tasks 1 and 2. Section 4: Task 3. Section 5: Task 4. Section 6: Task 5.
  Section 7: Task 6. Section 8: Tasks 1, 6 and 7. Section 9: the tests above.
- Deviations from the spec's letter: the cache sweep ages (30 days for images and diagrams, 90
  for posters) are the plan's choice; the mermaid page reports its size through the title and a
  dump pass rather than a fixed window; audio shows a glyph rather than a player.
