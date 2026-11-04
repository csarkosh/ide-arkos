# Outline Rail and Worktrees Sizing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The sidebar loses its Outline section and the tab bar its `⋯`; a markdown tab gains an outline rail (dots when closed, a labelled panel when open, an overlay in a narrow tab) and one `⋯` menu for its view toggles; the Worktrees section fills the sidebar and shows each worktree's change count without a click.

**Architecture:** Two removals first (small, independent), then the Worktrees changes (a pure height rule and a status driver that runs `git status` on events, not timers), then the rail in three steps (model and keymap, paint, the `⋯` menu). The rail's layout, dot states and overlay choice are pure functions in a new `outline_rail.rs`, tested headless; the painting lives beside the viewer in `pane_view.rs`; the per-tab state is on `Tab::Viewer` and `Viewer`.

**Tech Stack:** Rust, gpui 0.2.2 (csarkosh fork), `md_fold` (headings, current section, preview), `motion` (tokens and `Transient`), `chrome::menu_frame`, the worktree driver from `sidebar.rs` and `wt_watch.rs`.

**Spec:** `docs/superpowers/specs/2026-11-04-outline-rail-and-worktrees-sizing-design.md`

## Global Constraints

- Tests headless (`cargo test --workspace`), named as sentences, written before the code they check. Doc comments plain prose; no em-dashes anywhere (code, docs, commits).
- `nice -n 15 cargo ...`, never `--release`, one cargo at a time; finish each task with `nice -n 15 cargo check --workspace --all-targets` showing no warnings.
- Every colour is read from `root.theme` at paint time and never cached. Motion: every new transition uses a `motion` token and curve (entering decelerate, leaving accelerate and quicker), is instant under Reduce Motion, and adds its `Transient` to `Root::any_motion`.
- Numbers from the spec: rail 26 px; panel 220 px; overlay below 520 px of tab width; dots at least 16 px apart; dot sizes 12 px (top level) and 8 px (one below); preview after 500 ms, six lines; Worktrees floor a third of the sidebar's height; status debounce 1 second per worktree.
- Copy from the spec: menu items "Show raw" / "Show rendered" (⇧⌘M), "Inline diff" (⇧⌘D), "Present" / "Edit" (⌥⌘P), "Outline" (⌥⌘O); the panel's label "OUTLINE"; the worktree count "1 change" / "N changes", never "clean".
- The keymap command is `toggle outline`, default `alt-cmd-o`, context `Viewer`, section `Markdown`; telemetry name `toggle_outline`.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` (or the model that ran, if the harness names it).

## Review Focus

1. A document with hundreds of headings: the collapsed rail must stay readable (top-level dots only, then scrolling), never a smear of dots. Pinned in Task 5 (`a_long_document_keeps_its_dots_apart`).
2. A pane too narrow for a rail (a 160 px split): the rail must give way rather than crush the text. Pinned in Task 5 (`a_very_narrow_tab_shows_no_rail`).
3. A short window (a 300 px tall sidebar) with a long file tree: the Explorer keeps at least its header and one row, the Worktrees floor yields. Pinned in Task 3 (`a_short_sidebar_keeps_the_explorer_s_header`).
4. A worktree with 500 changes: the count reads "500 changes", one status run, the expanded list scrolls. Pinned in Task 4 (`a_large_count_is_one_status_run`).
5. A read-only markdown tab (not UTF-8, not writable): the `⋯` menu has no Show raw item but keeps Present and Outline; the find bar open moves the menu down. Pinned in Task 7 (`a_read_only_tab_s_menu_has_no_mode_item`, `the_button_sits_under_an_open_find_bar`).

---

### Task 1: The sidebar's Outline section goes

**Files:**
- Delete: `crates/arkos/src/outline.rs`
- Modify: `crates/arkos/src/main.rs` (`mod outline;`, `outline_hover` at ~266 and ~705, `outline::render_preview` at ~4473, `Sidebar::new`'s `outline_open` argument at ~664, `SidebarSaved` at ~1690), `crates/arkos/src/sidebar.rs:54-57, 76-94, 158, 227-241` (fields, constructor, fade, the section), `crates/arkos/src/workspace.rs:150-160` (`outline_open` stays readable: keep the field with `#[serde(default = "default_true")]` and a comment that it is ignored, so an old file loads; stop writing it), `docs/guide.md:226-229`, `docs/development.md:799-802`

**Interfaces:**
- Produces: `Sidebar::new(width, collapsed, explorer_open, worktrees_open, cx)` (no `outline_open`); `md_fold::{outline, visible_rows, current, preview}` untouched for Task 5.

- [ ] **Step 1: Write the failing test** in `workspace.rs`'s tests: `a_saved_sidebar_without_an_outline_flag_loads_and_one_with_it_is_ignored` (a `SidebarSaved` JSON with `outline_open: false` deserialises; one without it deserialises; a serialised `SidebarSaved` carries no `outline_open` key).
- [ ] **Step 2: Run** `nice -n 15 cargo test -p arkos workspace::` and see it fail on the serialised key.
- [ ] **Step 3: Delete** the module, fields, the section's render block and the preview call; adjust `Sidebar::new` and its callers; mark `SidebarSaved::outline_open` `#[serde(default = "default_true", skip_serializing)]`. Rewrite `docs/guide.md`'s Outline paragraph into one sentence pointing at the rail ("The outline lives inside the markdown tab; see Editing markdown" until Task 6 writes the real paragraph) and drop the Status row at `docs/development.md:799-802` (Task 6 adds the rail's).
- [ ] **Step 4: Run** `nice -n 15 cargo test -p arkos`, `cargo check` clean, `grep -rn "outline_hover\|outline_open\|outline::" crates/arkos/src` finds only the ignored saved field.
- [ ] **Step 5: Commit** `The sidebar's Outline section goes; the outline moves into the tab`.

---

### Task 2: The tab bar's overflow button goes

**Files:**
- Modify: `crates/arkos/src/pane_view.rs:155-163` (`show_overflow`, `menu_rows`), `:531-575` (the menu), `:664` (doc comment), `:891-903` (the button), `:931`, `:1548` (hides), tests `:1907-1915`; `crates/arkos/src/main.rs:465, 787, 845, 1271, 1304, 1705-1707, 1841-1856` (`tab_menu`, `toggle_tab_menu`, `close_tab_menu`); `docs/development.md:488` (drop "the tab overflow menu" from the menus an effect draws); `docs/guide.md` wherever the tab bar's `⋯` is described (`grep -n "⋯\|overflow" docs/guide.md`).

- [ ] **Step 1: Write the failing test**: replace `overflow_menu_lists_every_tab_from_two` with `the_tab_bar_offers_no_overflow_button_at_any_tab_count`, over a new pure `pub fn bar_items(tabs: usize) -> Vec<&'static str>` in `pane_view.rs` that names the bar's fixed children after the tabs (today `["+"]` plus `"more"` from two tabs; after this task `["+"]` at every count): assert `bar_items(1) == bar_items(2) && bar_items(9) == vec!["+"]`. The bar's render then builds its fixed children from `bar_items`, so the test holds the bar to it.
- [ ] **Step 2: Run** and see it fail.
- [ ] **Step 3: Remove** the button, the menu, the two helpers, the `Transient`, its `any_motion`/`settle`/hide lines and the Root methods; update the docs.
- [ ] **Step 4: Run** the crate tests and `cargo check` (no warnings; an unused `menu_report` key or import left behind is a warning to remove).
- [ ] **Step 5: Commit** `The tab bar's overflow button and its menu go`.

---

### Task 3: The Worktrees section fills the sidebar

**Files:**
- Modify: `crates/arkos/src/sidebar.rs:190-262` (the Explorer `flex_1` and the Worktrees `flex_none`), `:980-1000` (`render_worktree_rows`'s `max_h(root.content.h * 0.4)` goes), `docs/guide.md` Worktrees paragraph (one sentence), `docs/development.md:715` Status row (one clause)

**Interfaces:**
- Produces:
  ```rust
  /// Heights for the Explorer and the Worktrees sections in a sidebar `total` tall, both headers
  /// included, from the Explorer's content height (its rows, without its header): open Worktrees
  /// take what the Explorer does not need, never less than a third of `total`; the Explorer never
  /// less than its header plus one row.
  pub fn sidebar_split(total: f32, explorer_rows_h: f32, explorer_open: bool, worktrees_open: bool) -> (f32, f32);
  ```
  `ROW_H` is the header height.

- [ ] **Step 1: Write the failing tests** in `sidebar.rs`'s tests:
  ```rust
  #[test]
  fn worktrees_take_what_the_explorer_does_not_need_and_never_less_than_a_third() {
      // total 900, explorer rows 200: explorer 200 + ROW_H, worktrees the rest
      // total 900, explorer rows 2000: worktrees 300, explorer 600
  }
  #[test]
  fn a_closed_section_leaves_the_other_the_whole_sidebar() { /* worktrees closed: explorer total, worktrees ROW_H; explorer closed: explorer ROW_H, worktrees total - ROW_H */ }
  #[test]
  fn a_short_sidebar_keeps_the_explorer_s_header() { /* total 120, explorer rows 2000, both open: explorer >= 2 * ROW_H, worktrees = total - explorer */ }
  ```
- [ ] **Step 2: Run** `nice -n 15 cargo test -p arkos sidebar::` and see them fail.
- [ ] **Step 3: Implement** `sidebar_split` and use it: the Explorer's content height is `rows.len() as f32 * ROW_H` (the count `render_rows` already computes); set the Explorer's and the Worktrees' `h` from the split instead of `flex_1`/`flex_none`, with `min_h_0` and `overflow_y_scroll` on each section's rows; delete the 40% cap. The sidebar's total height is the sidebar column's bounds (`root.content.h`, as the cap used).
- [ ] **Step 4: Run** tests, `cargo check` clean; a throwaway local shoot of `~/Projects` with the sidebar open at 760 px tall and a long tree expanded, looked at, deleted after.
- [ ] **Step 5: Commit** `The Worktrees section fills the sidebar below the Explorer, a third of it at least`.

---

### Task 4: Worktree status at a glance

**Files:**
- Modify: `crates/arkos/src/worktrees.rs` (`to_load` at ~434, `mark_all_stale` ~444 (goes), `set_changes` ~454, `toggle` ~420, `rows`), `crates/arkos/src/sidebar.rs` (`load_pending_worktrees` ~1646, `touch_worktree` ~1487, `check_worktree_stamp` ~1466, `listed_one` ~1439, the row paint at ~1050-1060, `tick_worktree_ages` ~1410), `crates/arkos/src/explorer_watch.rs:126-140` (the slow tick no longer calls `mark_all_stale`/`load_pending_worktrees`), `docs/guide.md` Worktrees paragraph, `docs/development.md:715` Status row

**Interfaces:**
- Produces, on `Worktrees`:
  ```rust
  /// Side worktrees with files whose status is not loaded or is due, and not loading: every listed one, expanded or not.
  pub fn to_load(&self) -> Vec<PathBuf>;
  /// A change seen in the worktree: its status is due `after` from now (the debounce; a later call moves the time).
  pub fn status_due_at(&mut self, path: &Path, at: Instant);
  /// Worktrees whose due time has passed, taken off the list.
  pub fn due_now(&mut self, now: Instant) -> Vec<PathBuf>;
  pub fn set_changes(&mut self, path: &Path, changes: Option<Changes>);  // an empty result on an expanded worktree collapses it
  pub fn dirty_count(&self, path: &Path) -> Option<usize>;
  pub fn toggle(&mut self, path: &Path) -> bool;  // refuses a clean worktree (count Some(0)) and an unloaded one keeps expanding as now
  ```
  `WtRow::Worktree { dirty }` stays `Option<usize>`; the paint shows `1 change` / `N changes` in `theme.tree.modified` for `Some(n > 0)`, nothing for `Some(0)` or `None`, and a chevron only for `Some(n > 0)`.

- [ ] **Step 1: Write the failing tests** in `worktrees.rs`:
  ```rust
  #[test]
  fn every_listed_worktree_loads_its_status_once_expanded_or_not() { /* set_repo with two: to_load has both; set_changes on one: to_load has the other */ }
  #[test]
  fn a_change_seen_runs_status_a_second_later_and_a_second_change_moves_it() { /* status_due_at(t0+1s) then (t0+1.5s): due_now(t0+1.2s) empty, due_now(t0+1.6s) == [path] once */ }
  #[test]
  fn a_clean_worktree_has_no_chevron_and_does_not_expand() { /* set_changes(Some(vec![])): toggle false, rows()' WtRow has dirty Some(0); expanded one turning clean collapses */ }
  #[test]
  fn a_large_count_is_one_status_run() { /* 500 entries: dirty_count Some(500); to_load empty after */ }
  ```
  In `sidebar.rs`: update `the_worktree_menu_offers_...` if it reads the count text; add `the_row_text_names_the_count_and_never_clean` for a pure `wt_count_text(dirty: Option<usize>) -> Option<String>` (`None` and `Some(0)` → `None`; `Some(1)` → `"1 change"`; `Some(3)` → `"3 changes"`).
- [ ] **Step 2: Run** and see them fail.
- [ ] **Step 3: Implement.** Model: `to_load` drops the `expanded` filter; `due: HashMap<PathBuf, Instant>` with `status_due_at`/`due_now`; `mark_all_stale` and `stale` go. Driver: `listed_one` and the first load call `load_pending_worktrees` after `set_repo` (they do for expanded rows; now for all); `touch_worktree` and `check_worktree_stamp` (whether or not the stamp moved) call `status_due_at(path, now + 1 s)` and then `cx.spawn` one task that sleeps a second and calls `due_now(now)` then `load_pending_worktrees` (several such tasks are harmless: `due_now` hands each worktree out once, when its time has passed); the slow tick in `explorer_watch` keeps only `tick_worktree_ages` and `rediff`. Paint: `wt_count_text`, chevron only when dirty, `toggle` guard. `run_status` already goes through `git_read` (`--no-optional-locks`).
- [ ] **Step 4: Run** tests, `cargo check`; a hand run in the sandbox on `~/Projects`: the counts appear on load, a `touch` in a worktree shows "1 change" within two seconds, a `git commit` there clears it, `ps` shows no git while idle.
- [ ] **Step 5: Docs.** Guide: the Worktrees paragraph says each row shows its changes at a glance, a clean row says nothing and does not expand. Status row: "a change in a worktree shows '1 change' within two seconds and a commit clears it; `ps` shows no git while nothing changes".
- [ ] **Step 6: Commit** `Worktree status at a glance: the count on the row, run on change, never on a timer`.

---

### Task 5: The outline rail's model, the tab's state and the command

**Files:**
- Create: `crates/arkos/src/outline_rail.rs`
- Modify: `crates/arkos/src/main.rs` (`mod outline_rail;`; the `Command` dispatch near `:2868` gains `ToggleOutline`), `crates/arkos/src/tree.rs:72-80, 455-466` (`Tab::Viewer` gains `outline: bool`, serde default false, skipped when false; `set_viewer_outline(pane, idx, on)`), `crates/arkos/src/viewer.rs` (`pub outline: bool` beside `presenting` at ~958; `pub fn top_block(&self) -> usize`; `pub fn reveal_heading(&mut self, i: usize, cx)`), `crates/arkos/src/keymap.rs:105-120, 268-279, 395-410, 728-736, 805, 868-876` (`ToggleOutline`, `"toggle outline"`, `alt-cmd-o`), `crates/arkos/src/telemetry.rs:664-672` (`ToggleOutline => "toggle_outline"`), `telemetry/events.toml` palette list (`"toggle_outline"` after `"toggle_presentation"`), `docs/telemetry.md` (regenerated: `nice -n 15 cargo test -p telemetry write_the_tables_into_the_page -- --ignored`), `crates/arkos/src/command_palette.rs` (the palette lists it like `TogglePresentation`)

**Interfaces:**
- Consumes: `md_fold::{Head, outline, OutlineRow, current, preview}`, `md_blocks::block_at`.
- Produces:
  ```rust
  pub const RAIL_W: f32 = 26.0;
  pub const PANEL_W: f32 = 220.0;
  pub const OVERLAY_BELOW: f32 = 520.0;
  pub const MIN_TAB_W: f32 = 200.0;   // narrower: no rail at all (Review Focus 2)
  pub const DOT_GAP: f32 = 16.0;
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum DotState { Read, Current, Ahead }
  #[derive(Debug, Clone, Copy, PartialEq)]
  pub struct Dot { pub head: usize, pub y: f32, pub size: f32, pub state: DotState }
  /// The collapsed rail's dots for `heads` in a rail `height` tall with the section `current` in
  /// view: the top two levels (the document's lowest level and the one below) evenly spaced from
  /// `top` down, at least DOT_GAP apart; when they do not fit, the top level alone; when that does
  /// not fit either, every top-level dot DOT_GAP apart from `top` (the rail scrolls, `scroll_to`
  /// says where). Sizes 12.0 and 8.0.
  pub fn dots(heads: &[md_fold::Head], current: Option<usize>, height: f32, top: f32) -> Vec<Dot>;
  /// The rail's scroll offset that keeps the current dot in view, 0 when everything fits.
  pub fn scroll_to(dots: &[Dot], current: Option<usize>, height: f32) -> f32;
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum Shape { None, Rail, Panel, Overlay }
  /// What a tab `width` wide shows: nothing under MIN_TAB_W or with no headings, the rail when
  /// collapsed, the panel beside the document when expanded and wide, the overlay when narrow.
  pub fn shape(width: f32, expanded: bool, has_headings: bool, presenting: bool) -> Shape;
  /// The expanded panel's rows: every heading with its depth and state.
  pub struct PanelRow { pub head: usize, pub depth: usize, pub state: DotState }
  pub fn panel_rows(heads: &[md_fold::Head], current: Option<usize>) -> Vec<PanelRow>;
  ```
  `Viewer::top_block()`: in the rendered view `list_state.logical_scroll_top().item_ix`; in the raw view the block of the first visible line (`md_blocks::block_at(&self.blocks, line)`, the line from the raw scroll the find bar already uses). `Viewer::reveal_heading(i)`: `reveal_block(heads[i].block)` in the rendered view; in the raw view scroll the editor so the heading's first line is at the top (the same call the find bar uses to show a hit).

- [ ] **Step 1: Write the failing tests** in `outline_rail.rs` (headings built by hand: `Head { block, level, text, nth }`):
  ```rust
  #[test]
  fn dots_mark_the_top_two_levels_read_current_and_ahead() { /* levels 1,2,2,1,3: four dots (no level 3); current = 2 → dots[0..2] Read? no: dots[0] Read, dots[1] Read, dots[2] Current, dots[3] Ahead; sizes 12,8,8,12 */ }
  #[test]
  fn dots_are_spaced_evenly_at_least_the_gap_apart() { /* 4 dots in 300 px: y increasing, min gap >= 16 */ }
  #[test]
  fn a_long_document_keeps_its_dots_apart() { /* 60 headings: 30 level 1, 30 level 2, height 300: only level-1 dots; 60 level 1 in 300: 60 dots 16 apart and scroll_to keeps the current one inside the height */ }
  #[test]
  fn the_shape_follows_width_state_and_headings() {
      assert_eq!(shape(800.0, false, true, false), Shape::Rail);
      assert_eq!(shape(800.0, true, true, false), Shape::Panel);
      assert_eq!(shape(500.0, true, true, false), Shape::Overlay);
      assert_eq!(shape(500.0, false, true, false), Shape::Rail);
      assert_eq!(shape(800.0, true, false, false), Shape::None);
      assert_eq!(shape(800.0, true, true, true), Shape::None, "presentation mode shows the document alone");
  }
  #[test]
  fn a_very_narrow_tab_shows_no_rail() { assert_eq!(shape(160.0, false, true, false), Shape::None); }
  #[test]
  fn panel_rows_carry_every_heading_with_its_depth() { /* levels 1,2,3,1: depths 0,1,2,0 */ }
  ```
  In `tree.rs`: `a_viewer_tab_s_outline_flag_round_trips_and_defaults_closed` (serialise with `outline: true` → the key is present; without → absent; an old JSON without it reads false; `set_viewer_outline` flips it). In `keymap.rs`: extend the defaults test (`:868-876`) with `("alt-cmd-o", "toggle outline", "ToggleOutline")` and the section test. In `viewer.rs`: `the_top_block_follows_the_raw_view_s_first_line` (a document of three headings, raw mode scrolled to the second heading's line: `top_block()` is that heading's block).
- [ ] **Step 2: Run** `nice -n 15 cargo test -p arkos outline_rail:: tree:: keymap:: viewer::tests::the_top_block` and see them fail.
- [ ] **Step 3: Implement** the module, the tab field and setter, the viewer's two methods and `outline` state (loaded from the tab on open, as `presenting` is), the command end to end (`Root::toggle_outline_for(tab, cx)` flips `Viewer::outline` and `tree.set_viewer_outline`, saves, notifies), telemetry name and events list, the palette entry, and regenerate `docs/telemetry.md`. Confirm `alt-cmd-o` is unbound (`grep -n '"alt-cmd-o"' crates/arkos/src/keymap.rs`); if bound, pick `alt-cmd-l` and say so in the report.
- [ ] **Step 4: Run** the crate tests, `cargo check` clean.
- [ ] **Step 5: Commit** `The outline rail's layout, the tab's outline state and the toggle outline command`.

---

### Task 6: Painting the rail, the panel and the overlay

**Files:**
- Modify: `crates/arkos/src/pane_view.rs:1232-1300` (`render_viewer`: wrap the document in a row with the rail), new functions `render_outline_rail`, `render_outline_panel`, `render_outline_overlay` in `crates/arkos/src/outline_rail.rs` (the module may hold gpui code beside its pure functions, as `sidebar.rs` does), `crates/arkos/src/main.rs` (`outline_anim: HashMap<TabId, Transient<bool>>`, one per markdown tab, `true` while open: the width ease and the overlay's fade read its opacity; iterated in `any_motion` and `settle`, removed with the tab; `outline_hover: Option<(TabId, usize, f32, Instant)>` for the preview), `crates/arkos/src/theme.rs` or wherever the contrast matrix test lives (`grep -n "fn contrast_matrix\|every_built_in_theme" crates/arkos/src/*.rs`): a mark pair `focus_ring` on `window_bg` at 3:1 for every built-in theme, if not already there), `docs/guide.md` (the rail's paragraph under Editing markdown), `docs/development.md` Status (the rail's rows)

**Interfaces:**
- Consumes: Task 5's functions and state; `Viewer::{heads, top_block, reveal_heading, outline, presenting, doc, blocks}`; `md_fold::preview`; `chrome::menu_frame`; `motion::{ENTER, EXIT, lift, reduce_motion, Transient}`.
- Produces: the three render functions, each `(root: &mut Root, viewer: &Entity<Viewer>, tab: TabId, theme: &Theme, width: f32, height: f32, cx) -> Option<Div>`; the collapsed rail's tooltip uses the app's tooltip helper (`grep -n "fn tooltip\|Tooltip" crates/arkos/src/*.rs`, the grid's cell tooltip in `grid_view.rs` is the model).

- [ ] **Step 1: Write the failing test** for the one pure piece here: `the_rail_s_width_eases_between_closed_and_open(progress: f32) -> f32` (`rail_width(0.0) == 26.0`, `rail_width(1.0) == 220.0`, monotonic), in `outline_rail.rs`. The painting is checked on screen (step 4).
- [ ] **Step 2: Run** and see it fail.
- [ ] **Step 3: Implement.** In `render_viewer`, for a markdown tab (`has_doc`) compute `shape(width, v.outline, !v.heads.is_empty(), presenting)` with the pane's width; `Rail`: a `div().flex_none().w(RAIL_W)` column left of the document holding the ☰ glyph (click → `toggle_outline_for`) and the dots from `dots(...)` drawn as absolute circles (`Read`: `chrome.text_dim`; `Current`: `chrome.focus_ring`; `Ahead`: hollow, border `chrome.text_dim` at alpha 0.5) with the stepper line behind; a dot's hover shows the heading's text in the tooltip; click → `reveal_heading`. `Panel`: a 220 px column with the "OUTLINE" label, « (toggle), and `panel_rows` as rows (indent 14 px per depth, the current row with a tinted pill `chrome.tab_active`), hover 500 ms → the preview beside the panel (`md_fold::preview`, in `menu_frame`), click → `reveal_heading`. `Overlay`: the rail stays, plus an absolute panel over the document with a scrim (`chrome.window_bg` at alpha 0.35) that closes on click; the overlay takes focus (its own `FocusHandle`; Up/Down move a highlighted row, Enter reveals and closes, Escape closes and refocuses the viewer). Motion: the rail-to-panel width runs through a `Transient` (ENTER decelerate opening, EXIT accelerate closing), the overlay's scrim and slide through the same; add to `any_motion` and `settle`. The ☰ and « glyphs are text in `chrome.text_dim`.
- [ ] **Step 4: Run** tests and `cargo check`; then a throwaway local shoot story on a copy of `docs/guide.md` in Material: three stills (collapsed; expanded wide; expanded in a 480 px pane) looked at and fixed before committing; the story and its folder deleted after. Also `arkos theme contrast` on `material`, `navy`, `matrix` and `peach-watercolor` still exits 0.
- [ ] **Step 5: Docs.** Guide, under Editing markdown: the rail (the dots and what they mean, hover and click, ☰ or ⌥⌘O to open, the panel and its preview, the overlay in a narrow pane with its keys, remembered per tab, not in presentation mode). Status: "the rail's dots move with the scroll; hover a dot: its heading; click: the document goes there; ☰: the panel eases open beside the text; narrow the pane under 520 px: it slides over the text with a scrim and Escape closes it; relaunch: the tab remembers; under each effect the rail sits on the tab's paper".
- [ ] **Step 6: Commit** `The outline rail: dots when closed, a panel beside the document when open, over it in a narrow tab`.

---

### Task 7: The markdown tab's `⋯` menu replaces the pills

**Files:**
- Create: `crates/arkos/src/view_menu.rs`
- Modify: `crates/arkos/src/pane_view.rs:1105-1107` (`PILL_W`, `CHIP_W` go), `:1171-1178` (`banner_right_pad` becomes the button's width plus margins), `:1196-1212` (`pill_label_for`, `present_label` become the menu's words), `:1350-1445` (the chip, the mode pill and the present pill go; the grid's strip keeps its pill), the tests `:1744-1763`; `crates/arkos/src/main.rs` (`view_menu: Transient<TabId>` in the fields, `any_motion`, `settle`, the mouse-down hide at ~845, `escape_closes_menu` in `sidebar.rs:1415`, hide on tab close); `docs/guide.md:129-132, 161, 165-169, 266-272` (the pill, the present pill and the chip become the menu); `docs/development.md` Status (one row: the menu)

**Interfaces:**
- Consumes: `Root::{toggle_markdown_view_for, toggle_presentation_for, toggle_outline_for}`, `Viewer::toggle_inline_diff`, `chrome::{menu_frame, menu_colors, menu_report}`, `motion::{lift, MENU_LIFT}`.
- Produces:
  ```rust
  pub const BUTTON: f32 = 26.0;
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum ViewAction { ToggleMode, ToggleInlineDiff, TogglePresent, ToggleOutline }
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct MenuItem { pub label: &'static str, pub keys: &'static str, pub checked: bool, pub action: ViewAction }
  /// The menu's rows for a tab's state: the mode item only when `editable`, the diff item only with
  /// `has_changes`, present and outline always; the words name what a click turns to.
  pub fn items(mode: ViewMode, delimited: bool, editable: bool, has_changes: bool, inline: bool, presenting: bool, outline: bool) -> Vec<MenuItem>;
  /// The button's top: under the find bar when it is open.
  pub fn button_top(find_open: bool) -> f32;   // 44.0 or 10.0, as the pill today
  pub fn render_view_button(..) -> Div;  pub fn render_view_menu(root: &mut Root, cx) -> Option<Div>;
  ```

- [ ] **Step 1: Write the failing tests** in `view_menu.rs`:
  ```rust
  #[test]
  fn the_menu_names_what_a_click_turns_to_with_its_keys() {
      let m = items(ViewMode::Rendered, false, true, true, false, false, false);
      assert_eq!(m.iter().map(|i| (i.label, i.keys, i.checked)).collect::<Vec<_>>(), vec![("Show raw", "⇧⌘M", false), ("Inline diff", "⇧⌘D", false), ("Present", "⌥⌘P", false), ("Outline", "⌥⌘O", false)]);
      let m = items(ViewMode::Raw, false, true, true, true, false, true);
      assert_eq!(m[0].label, "Show rendered"); assert!(m[1].checked); assert!(m[3].checked);
      assert_eq!(items(ViewMode::Raw, true, true, false, false, false, false)[0].label, "Show grid");
      assert_eq!(items(ViewMode::Rendered, false, true, false, false, true, false)[2].label, "Edit");
  }
  #[test]
  fn a_read_only_tab_s_menu_has_no_mode_item() { /* editable false: first item is Present (or Inline diff when has_changes) */ }
  #[test]
  fn the_button_sits_under_an_open_find_bar() { assert_eq!(button_top(true), 44.0); assert_eq!(button_top(false), 10.0); }
  ```
  Update `banner_right_pad`'s test to the button (`BUTTON + 8.0 + 14.0` with a button, `0.0` without) and delete the pill label tests that the menu tests replace.
- [ ] **Step 2: Run** and see them fail.
- [ ] **Step 3: Implement.** The button: a 26 px round `div` with `⋯` in `chrome.text_dim`, `pill_bg`/`pill_border` as the pill used, at `button_top` and right 14 px, on every markdown tab (not a grid, not a code file: `has_doc`), presentation mode included; click → `root.view_menu.show(tab, now)` (and focus the pane, stopping propagation as the pill did). The menu: drawn in `render_viewer` after the body like the old tab menu was in the bar (absolute, right 14 px, top `button_top + BUTTON + 4`, `menu_frame`, `menu_colors`, `menu_report` when `style.menu_edges`, `lift`), one row per `MenuItem` (label left, keys right in `chrome.text_dim`, a ✓ before a checked label), a click runs the action through the existing `Root` methods and hides the menu. Hidden by Escape (`escape_closes_menu`), by the window's mouse-down elsewhere (the `:845` list), by the tab closing, and by its own button. Remove the chip, the two pills and their constants; keep the grid strip's pill through `pill_label_for` (now in `view_menu.rs` as the mode item's words).
- [ ] **Step 4: Run** tests, `cargo check`; a throwaway local shoot of a markdown tab with changes: the button, then (with the shoot's click) the open menu; looked at, deleted after.
- [ ] **Step 5: Docs.** Guide: every "pill", "present pill" and "inline diff chip" mention becomes "the ⋯ menu at the tab's top right" with the items and their shortcuts; the CSV paragraph keeps its strip pill. Status: "⋯ on a markdown tab opens the menu under it; each item does its toggle and closes it; Escape and a click outside close it; with the find bar open it sits below the bar".
- [ ] **Step 6: Commit** `A markdown tab's view toggles live in one menu behind a single button`.
