# A Click Keeps the Keys Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A mouse click never takes the keys or the focus glow from the pane being typed in; keyboard navigation is unchanged.

**Architecture:** The keys follow flags on `Root` (`sidebar_focused`, `session_list_focused`, `switcher_focused`, `wt_focused`, `header_ring`, `bar_ring`), turned into a focus handle each render (`main.rs`, the `wanted` chain). The fix removes the click paths that set those flags, routes a clicked file through the "A session's files open" setting, and carries the glow across a page switch so it never dips.

**Tech Stack:** Rust, gpui (csarkosh fork). Tests are plain `#[test]` functions on pure helpers; there is no headless `Root`.

**Spec:** `docs/superpowers/specs/2026-11-18-click-focus-design.md`

## Global Constraints

- Keyboard paths (Cmd+0, rings, Enter and Space in the tree, the bar's icons by keys) behave exactly as before.
- Tests are named as sentences and written before the code. Doc comments plain prose, no em-dashes.
- Builds: `nice -n 15`, never `--release`, one cargo at a time. Finish with `cargo check --workspace --all-targets` showing no warnings.

## Review Focus

- A click in Pane 0 while Pane 0 already has the keys (after Cmd+0): the keys stay in Pane 0. The Sessions pill puts them on the session list, the Files pill on the tree (Task 2 hand check).
- A Neighbor click when the file is already open in the neighbour pane activates that tab, not a second one (Task 1 test).
- A Neighbor click on a single-pane page splits once; a second click on another file goes into that new pane, not a second split (Task 1 test).
- Escape with a mouse-opened bell menu reaches the pane (Claude), not the menu; the menu closes on a click outside, as today (Task 3 hand check).
- The glow carried across a page switch to a pane that is not focused would light the wrong pane (Task 4 test).

---

### Task 1: A clicked file opens by the placement setting

**Files:**
- Modify: `crates/arkos/src/sidebar.rs` (`open_viewer` near line 1923, `open_plan` near 918, the tree row's click near 816, the worktree change row's click near 1345)
- Test: `crates/arkos/src/sidebar.rs` (`mod tests`)

**Interfaces:**
- Consumes: `direct::neighbor_step(tree: &Tree, from: PaneId, bounds: Rect) -> OpenStep`, `direct::OpenStep::{AddTo, SplitFrom}`, `config::OpenPlacement`.
- Produces: `pub fn click_target(tree: &Tree, placement: OpenPlacement, focused_is_terminal: bool, bounds: Rect) -> OpenStep` and `Root::open_clicked(&mut self, path: &Path, pin: bool, cx: &mut Context<Self>)`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_click_in_place_opens_the_file_in_the_focused_pane() {
    // Inplace, focused terminal: AddTo { pane: focused }.
}
#[test]
fn a_click_beside_a_terminal_opens_the_file_in_the_pane_to_its_right() {
    // Neighbor, two panes side by side, the left focused showing a terminal: AddTo { pane: right }.
}
#[test]
fn a_click_beside_a_lone_terminal_splits_off_a_pane_to_its_right() {
    // Neighbor, one pane: SplitFrom { pane: focused, side: Side::Right }.
}
#[test]
fn a_click_with_a_file_tab_focused_opens_it_there_whatever_the_setting() {
    // Neighbor, focused_is_terminal false: AddTo { pane: focused }.
}
#[test]
fn a_second_click_beside_a_terminal_goes_into_the_pane_the_first_split_off() {
    // Apply the first SplitFrom with tree.split, re-focus the terminal pane, plan again: AddTo { pane: new }.
}
```

Build trees the way `direct.rs`'s tests do (`bounds = Rect { x: 0.0, y: 0.0, w: 1200.0, h: 800.0 }`).

- [ ] **Step 2: Run them to see them fail**

Run: `nice -n 15 cargo test -p arkos --bin arkos sidebar::tests::a_click`
Expected: FAIL, `click_target` not found.

- [ ] **Step 3: Implement `click_target`, then `open_clicked`**

`click_target`: `Inplace` or not a terminal gives `AddTo { pane: tree.focused }`; else `neighbor_step(tree, tree.focused, bounds)`.

`open_clicked`: remember `before = self.tree.focused`; take the step from `click_target` (focused-is-terminal from `self.active_terminal(cx).is_some()`, placement `self.config.layout.placement`, bounds `self.content`). For `AddTo { pane }`, run the body of today's `open_viewer` against `pane` instead of the focused pane (split it into `open_viewer_in(&mut self, pane: PaneId, path, pin, cx)`; `open_viewer` calls it with the focused pane so the keyboard path is unchanged). For `SplitFrom { pane, side }`, `self.tree.split(pane, side, file_tab(path))`, with the preview tab set as `OpenPlan::Add` sets it and telemetry `Opened::Explorer`. Then `self.tree.focused = before`, with the sidebar flags left alone. The tree row's and the worktree change row's `on_click` call `open_clicked`. Enter and Space in `tree_key` and `wt_key` keep calling `open_viewer`.

- [ ] **Step 4: Run the tests**

Run: `nice -n 15 cargo test -p arkos --bin arkos sidebar::tests`
Expected: PASS.

- [ ] **Step 5: Commit** "A clicked file opens by the placement setting, the keys left where they were"

### Task 2: A press in Pane 0 leaves the keys where they are

**Files:**
- Modify: `crates/arkos/src/sidebar.rs` (the column's `on_mouse_down` near line 477; the Sessions and Files pills near 310)
- Modify: `docs/superpowers/specs/2026-11-16-chrome-keys-design.md` (section 2, "A click anywhere else takes the keys as before"), `docs/guide.md` (wherever clicks in the sidebar and the keys are described), `docs/development.md` Status (rows near 532 and 1169)

**Interfaces:**
- Produces: `pub fn pill_keys(had_keys: bool, files: bool) -> Option<Spot>`: `None` when Pane 0 did not have the keys, else `Some(Spot::Tree)` for Files or `Some(Spot::Sessions)` (using the existing `sidebar::Spot`).

- [ ] **Step 1: Write the failing test** `a_pill_click_keeps_the_keys_in_pane_0_only_when_it_had_them`, asserting the four cases of `pill_keys`.
- [ ] **Step 2: Run it; expect FAIL** (`pill_keys` not found).
- [ ] **Step 3: Implement.**
  - Delete the column's left `on_mouse_down` (the one calling `focus_sessions`).
  - The pills call `pill_keys(root.sidebar_has_keys(), files)`. `Some(spot)` puts the keys at that spot as `focus_sessions` does for it. `None` leaves every flag alone (today the Sessions pill clears `sidebar_focused` and `wt_focused`: keep that only in the `Some` case).
  - Group headings, session-group headings and right-press menus already set no flags; leave them.
- [ ] **Step 4: Run** `nice -n 15 cargo test -p arkos --bin arkos sidebar::tests`; expect PASS.
- [ ] **Step 5: Update the docs.**
  - Keys spec: a click no longer takes the keys; it cites this design.
  - Guide: how the keys and clicks work.
  - Status, new row: "click focus: type in Claude in Pane 1, click Files, a folder, the Explorer heading, Sessions, a group heading: the glow stays on Pane 1 and typing still reaches Claude; after Cmd+0, the same clicks keep the keys in Pane 0".
  - Status: amend the rows at 532 and 1169 if they now read wrong.
- [ ] **Step 6: Commit** "A press in Pane 0 no longer takes the keys"

### Task 3: The bell and the doctor opened by a click leave the keys

**Files:**
- Modify: `crates/arkos/src/main.rs` (`click_popover` near 1115, `close_popovers` near 1158, `bar_by_mouse` field near 396 and its uses at 971, 2631, 6445)

- [ ] **Step 1: Implement.** In `click_popover`, a click that opens a popover while `bar_ring` is `None` sets no flags. A ring already on the bar (reached by keys) keeps today's path. `bar_by_mouse` then has no writer, so remove it and its reads: the ring shown is `bar_ring.unwrap_or_default()`, and `close_popovers` no longer clears a click's ring. There is no pure helper worth testing here; the hand check covers it.
- [ ] **Step 2: Verify** `nice -n 15 cargo test -p arkos --bin arkos` passes and `cargo check -p arkos --all-targets` shows no warnings.
- [ ] **Step 3: Add a Status row.** "bar clicks: typing in Claude, click the bell: its menu opens, the glow stays on the pane, typing reaches Claude, a click on a row works it, Escape goes to Claude; the same for the doctor and Help; Cmd+0, Up to the bell, Enter: the menu takes the keys as before". Amend the title bar keys row near 1123 if needed.
- [ ] **Step 4: Commit** "A bar icon opened by a click leaves the keys in the pane"

### Task 4: The glow stays lit when a session row's click changes the page

**Files:**
- Modify: `crates/arkos/src/motion.rs` (`Fades`, near line 290), `crates/arkos/src/main.rs` (`show_row` near 2972)
- Test: `crates/arkos/src/motion.rs` (`mod tests`)

**Interfaces:**
- Produces: `impl<K: Hash + Eq, T> Fades<K, T> { pub fn carry(&mut self, from: &K, to: K) }`. It moves `from`'s entry to `to` when `to` has none, and does nothing otherwise.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_fade_carried_to_another_key_shows_there_at_the_opacity_it_had() {
    // sync(a, Some(()), t0); at t0 + 1 s (fully in) carry(&a, b);
    // sync(b, Some(()), t1) returns opacity 1.0, and opacity(&a, t1) == 0.0.
}
#[test]
fn a_fade_is_not_carried_onto_a_key_already_showing() {
    // b already fading in at 0.3: carry(&a, b) leaves b's opacity as it was.
}
```

- [ ] **Step 2: Run** `nice -n 15 cargo test -p arkos --bin arkos motion::tests::a_fade`; expect FAIL.
- [ ] **Step 3: Implement `carry`.** In `show_row`, take `before = self.tree.focused` before `show_page`. After `focus_pane`, when the new focused pane differs, call `self.glow.carry(&before, self.tree.focused)`. Only the focused pane's glow moves; no other pane lights.
- [ ] **Step 4: Run the tests; expect PASS.**
- [ ] **Step 5: Add a Status row.** "row click glow: typing in Claude in Pane 1, click another session's row: the glow stays lit on Pane 1 through the switch, never on Pane 0, never dimming".
- [ ] **Step 6: Commit** "The glow stays lit through a session row's page switch"

### Task 5: Finish

- [ ] Run `nice -n 15 cargo test --workspace`. Expected: all pass. Report the arkosd pty pair separately if they fail as on 2026-11-18.
- [ ] Run `nice -n 15 cargo check --workspace --all-targets`. Expected: no warnings.
- [ ] Build a sandbox app and hand-check the Status rows above in it before reporting. The user does the final check in their own window.

### Task 6: A session's name and a tab's name are their own

**Spec:** `docs/superpowers/specs/2026-11-18-session-names-design.md`

**Files:**
- Modify: `crates/arkos/src/tree.rs` (`Tree` near line 180: new fields; `set_tab_name` near 640; page removal)
- Modify: `crates/arkos/src/sessions_list.rs` (`entries` near 500, the row's name cell, `shown_name` callers)
- Modify: `crates/arkos/src/main.rs` (`RowAction::Rename` near 3074, the ended entry near 3304, `resume_ended` near 2430, where the layout loads and `inbox::into_pages` runs)
- Docs: `docs/guide.md` (renaming), a Status row.

**Interfaces:**
- Produces:
  - `Tree` fields `#[serde(default)] session_names: Vec<(PageKey, String)>` and `#[serde(default)] pub names_split: bool`.
  - `Tree::session_name(&self, page: PageKey) -> Option<&str>` and `Tree::set_session_name(&mut self, page: PageKey, name: Option<String>)` (trimmed, blank clears, as `set_tab_name`).
  - `Tree::split_names(&mut self)`: when `!names_split`, copies each page's first terminal's tab name to its session, then sets the flag.
  - `Root.row_rename: Option<RowRename { page: PageKey, edit: LineEdit }>` with a focus handle `row_rename_focus` in the `wanted` chain beside `tab_rename`.

- [ ] **Step 1: Write the failing tests** in `tree.rs`:
  - `a_session_s_name_is_its_own_and_a_tab_s_rename_leaves_it`
  - `a_session_s_name_is_saved_with_the_layout_and_goes_with_its_page`
  - `a_layout_from_before_copies_each_page_s_tab_name_to_its_session_once` (a second `split_names` after a session rename changes nothing)

  In `sessions_list.rs`, `a_row_shows_its_session_s_name_not_its_tab_s`, through the `shown_name` the row now calls with the session's name.
- [ ] **Step 2: Run them; expect FAIL.**
- [ ] **Step 3: Implement.**
  - `entries` takes the name from `tree.session_name(page)` and the typing from `row_rename` for that page.
  - `RowAction::Rename` opens `row_rename` on the row: it no longer activates or renames a tab.
  - The row's name cell draws `line_edit::render` while renaming, and its keys go through a `row_rename_key` that mirrors `tab_rename_key`.
  - A press elsewhere commits it, as `commit_tab_rename` is committed today (find that call).
  - Ended entries use the session's name. `resume_ended` sets the session's name, not the tab's. `split_names` runs on load.
- [ ] **Step 4: Run** `nice -n 15 cargo test -p arkos --bin arkos tree:: sessions_list::`; expect PASS.
- [ ] **Step 5: Docs.**
  - Guide: renaming a session or a tab.
  - Status row: "session names: right-click a row, Rename: the row's name becomes an editor, Enter keeps it; the tab keeps its own title; double-click the tab, rename it: the row keeps its name; a blank row name follows Claude's topic again".
- [ ] **Step 6: Commit** "A session's name and a tab's name are their own"
