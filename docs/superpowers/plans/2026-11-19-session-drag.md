# Session Drag Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Drag a tab onto the Sessions list to make it a session of its own or join another, and drag a session row onto a pane to bring its tabs into the session on screen.

**Architecture:** The tree gains three page-to-page moves (pure, headless-tested). The Sessions list learns to list a page with no terminal. A pure function in `drag.rs` decides what a drop on the list does. `Root` glues them to gpui drags: a new `SessionDrag` payload from live rows, the rows and the list as drop targets for `TabDrag`, the panes as drop targets for `SessionDrag`, a lit row for a join and a placeholder row for a new session.

**Tech Stack:** Rust, gpui (csarkosh fork), the `arkos` crate.

**Spec:** `docs/superpowers/specs/2026-11-19-session-drag-design.md`

## Global Constraints

- Tests are headless (`cargo test -p arkos --bin arkos`), named as sentences, written first and watched to fail.
- Doc comments are plain prose with no em-dashes.
- Builds use `nice -n 15`, one cargo at a time, never `--release`.
- Finish with `nice -n 15 cargo check --workspace --all-targets` showing no warnings.
- Motion follows `docs/development.md` (Motion), through `crates/arkos/src/motion.rs`:
  - An opening uses `motion::ENTER` on a decelerate curve; a closing uses `motion::EXIT` on an accelerate curve.
  - A highlight uses `motion::highlight_in` and `motion::highlight_out`.
  - All of it is instant under Reduce Motion.
  - Each new animated thing joins `Root::any_motion`.
- Colours are read from `root.theme` at paint time.
- Ids are kept across every move: a `TabId` is never reissued, so `Tree::names` follow their tabs. A terminal keeps its `SessionId`, and no daemon message is sent.

## Review Focus

1. **A row rename or the list's keyboard cursor on a page that a merge closes.** `root.row_rename`, `root.session_cursor` and `root.held` must be cleared when they name a page that is gone, or Enter acts on nothing.
2. **A drag cancelled outside the window, or with Escape.** The join highlight and the placeholder must close. Nothing may stay lit, and no row may stay pushed down.
3. **A tab dragged out of a maximized pane, or a session merged into one.** The maximize must end or hold as a tab drop does today.
4. **A viewer with unsaved edits, or a playing video, moved to a page not shown.** It keeps its view and its edits, since the views are keyed by `TabId`; the tree tests pin that the id is kept.
5. **The dragged session's terminal exits mid-drag, so its page closes before the drop.** The drop must do nothing, and must not panic on a missing page.

---

### Task 1: The tree moves tabs between pages

**Files:**
- Modify: `crates/arkos/src/tree.rs` (new methods beside `merge_pane`, near line 1124; tests in its `mod tests`)

**Interfaces:**
- Consumes: the existing `Tree::locate`, `take_tab`, `close_pane_if_empty`, `close_page`, `page_of`, `page_focused`, `page_tabs`, `split_with_tab`, `fresh_id`, `Page`, and `Side`.
- Produces:
  - `pub enum Place { At(usize), End, Side(Side) }`: where a merged page's tabs land in a pane.
  - `pub fn move_tab_to_page(&mut self, tab: TabId, page: PageKey) -> bool`:
    - Moves the tab with id `tab` to the end of page `page`'s focused pane, as that pane's active tab.
    - The page shown, `self.focused` and `self.maximized` are unchanged. Unlike `move_tab`, it does not call `focus`, which would show the page.
    - The pane the tab left closes when it emptied, unless it was its page's last.
    - False, changing nothing, when the tab or the page is missing, or when `page` is the tab's own page.
  - `pub fn page_from_tabs(&mut self, tabs: &[TabId]) -> Option<PageKey>`:
    - Makes a new kept page, not shown, of one pane holding those tabs in the order given, ids kept. The first tab is active, and the new page's focused pane is its one pane.
    - Panes they left close when emptied, unless last on their page.
    - Returns the new page's key.
    - None, changing nothing:
      - when `tabs` is empty or any id is missing;
      - when the tabs are not all on one page;
      - when they are every tab of their page, since that would only remake the same session.
  - `pub fn merge_page(&mut self, from: PageKey, into: PaneId, place: Place) -> bool`:
    - Moves every tab of page `from`, in `page_tabs` order, into pane `into` on the page shown.
      - `At(i)` inserts them from index `i`.
      - `End` appends them.
      - `Side(s)` makes one new pane on that side of `into` holding them all.
    - The first moved tab becomes active, and the pane it landed in is focused.
    - Page `from` closes through `close_page(from, None)`, which drops its session name and keeps the tab names.
    - False, changing nothing, when `from` is the page shown, `from` is missing, or `into` is not on the page shown.

- [ ] **Step 1: Write the failing tests** in `tree.rs`'s `mod tests`. Use the existing `term(n)` helper, and `Tab::Viewer`/markdown tabs as other tests there build them.

```rust
#[test]
fn a_tab_joins_another_page_at_the_end_of_its_focused_pane_and_the_shown_page_stays() {
    let mut t = Tree::new(term(1));
    let first = t.page;
    let p1 = t.focused;
    t.add_tab(p1, term(2));
    let other_pane = t.blank_page();
    t.add_tab(other_pane, term(3));
    let other = t.page;
    t.show_page(first);
    let moving = t.tab_id(p1, 1).unwrap();
    t.set_tab_name(moving, Some("api".into()));
    assert!(t.move_tab_to_page(moving, other));
    assert_eq!(t.page, first, "the page shown stays");
    assert_eq!(t.page_focused(first), Some(p1));
    assert_eq!(t.locate(moving), Some((other_pane, 1)), "the end of the other page's focused pane");
    assert_eq!(t.pane(other_pane).unwrap().active, 1);
    assert_eq!(t.tab_name(moving), Some("api"), "the tab keeps its name");
    assert!(matches!(t.pane(other_pane).unwrap().tabs[1], Tab::Terminal { session: SessionId(2), .. }));
}

#[test]
fn a_tab_joining_its_own_page_changes_nothing() {
    let mut t = Tree::new(term(1));
    let id = t.tab_id(t.focused, 0).unwrap();
    let before = t.clone();
    assert!(!t.move_tab_to_page(id, t.page));
    assert_eq!(t, before);
}

#[test]
fn a_tab_made_into_a_page_keeps_its_id_and_name_and_the_shown_page_stays() {
    let mut t = Tree::new(term(1));
    let first = t.page;
    let p1 = t.focused;
    t.add_tab(p1, term(2));
    let id = t.tab_id(p1, 1).unwrap();
    t.set_tab_name(id, Some("api".into()));
    let made = t.page_from_tabs(&[id]).expect("a new page");
    assert_ne!(made, first);
    assert_eq!(t.page, first);
    assert_eq!(t.page_tabs(made).len(), 1);
    assert_eq!(t.page_of(t.locate(id).unwrap().0), Some(made));
    assert_eq!(t.tab_name(id), Some("api"));
    assert_eq!(t.page_tabs(first).len(), 1);
}

#[test]
fn the_shown_page_s_only_tabs_make_no_new_page() {
    let mut t = Tree::new(term(1));
    let id = t.tab_id(t.focused, 0).unwrap();
    let before = t.clone();
    assert_eq!(t.page_from_tabs(&[id]), None);
    assert_eq!(t, before);
}

#[test]
fn a_pane_emptied_by_a_move_closes_unless_it_is_its_page_s_last() {
    let mut t = Tree::new(term(1));
    let first = t.page;
    let p1 = t.focused;
    let p2 = t.split(p1, Side::Right, term(2)).unwrap();
    let other_pane = t.blank_page();
    t.add_tab(other_pane, term(3));
    let other = t.page;
    t.show_page(first);
    let id2 = t.tab_id(p2, 0).unwrap();
    assert!(t.move_tab_to_page(id2, other));
    assert!(t.pane(p2).is_none(), "the emptied pane closed");
    let id1 = t.tab_id(p1, 0).unwrap();
    assert!(t.move_tab_to_page(id1, other));
    assert!(t.pane(p1).is_some_and(|p| p.tabs.is_empty()), "the page's last pane stays, empty, for the caller to settle");
}

#[test]
fn a_merged_page_s_tabs_land_in_order_and_the_page_and_its_session_name_go() {
    let mut t = Tree::new(term(1));
    let first = t.page;
    let p1 = t.focused;
    let q1 = t.blank_page();
    t.add_tab(q1, term(2));
    let q2 = t.split(q1, Side::Right, term(3)).unwrap();
    let gone = t.page;
    t.set_session_name(gone, Some("old".into()));
    let (a, b) = (t.tab_id(q1, 0).unwrap(), t.tab_id(q2, 0).unwrap());
    t.set_tab_name(b, Some("kept".into()));
    t.show_page(first);
    assert!(t.merge_page(gone, p1, Place::At(0)));
    assert_eq!(t.pane(p1).unwrap().ids[..2], [a, b], "first pane's tabs, then the second's, from the drop");
    assert!(!t.page_keys().contains(&gone));
    assert_eq!(t.session_name(gone), None);
    assert_eq!(t.tab_name(b), Some("kept"));
    assert_eq!(t.pane(p1).unwrap().active, 0);
    assert_eq!((t.page, t.focused), (first, p1));
}

#[test]
fn a_merged_page_on_an_edge_makes_one_new_pane_holding_every_tab() {
    let mut t = Tree::new(term(1));
    let first = t.page;
    let p1 = t.focused;
    let q1 = t.blank_page();
    t.add_tab(q1, term(2));
    t.add_tab(q1, term(3));
    let gone = t.page;
    t.show_page(first);
    assert!(t.merge_page(gone, p1, Place::Side(Side::Right)));
    assert_eq!(t.page_panes(first).len(), 2);
    let new = t.focused;
    assert_ne!(new, p1);
    assert_eq!(t.pane(new).unwrap().tabs.len(), 2);
}

#[test]
fn merging_the_shown_page_into_itself_changes_nothing() {
    let mut t = Tree::new(term(1));
    let before = t.clone();
    assert!(!t.merge_page(t.page, t.focused, Place::End));
    assert_eq!(t, before);
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `nice -n 15 cargo test -p arkos --bin arkos tree::tests 2>&1 | tail -20`
Expected: a compile error that `move_tab_to_page`, `page_from_tabs`, `merge_page` and `Place` are not found.

- [ ] **Step 3: Implement `Place`, `move_tab_to_page`, `page_from_tabs` and `merge_page`** with the signatures above.
  - For a move into a kept page, use `remove_tab` plus a direct insert into `pane_mut` (as `move_tab`'s cross-pane branch does, without `self.focus`).
  - `page_from_tabs` pushes a `Page { key: fresh, root: Node::Leaf(Pane { .. }), focused: pane, maximized: None }` onto `self.pages`.
  - `merge_page` with `Side(s)` splits with the first tab (`split_with_tab`), then inserts the rest into the new pane.

- [ ] **Step 4: Run the tests and watch them pass**

Run: `nice -n 15 cargo test -p arkos --bin arkos tree:: 2>&1 | grep -E "test result|FAILED"`
Expected: `test result: ok`, every `tree::` test included.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/tree.rs
git commit -m "Tree: move a tab to another page, make a page from tabs, merge a page into a pane"
```

### Task 2: The Sessions list lists a page with no terminal

**Files:**
- Modify: `crates/arkos/src/sessions_list.rs`:
  - a new `quiet_entry` beside `page_entry` (line 130);
  - the `entries` loop (line 515);
  - `menu_items` (line 430) and `RowAction` (line 397);
  - tests from line 932.
- Modify: `crates/arkos/src/main.rs`:
  - `Root` gains `pub page_made: HashMap<tree::PageKey, u64>`, the moment (ms) a drop made each page, not saved;
  - the `RowAction::Close` arm in the row action match (near line 3086);
  - `Root::close_page_tabs`.

**Interfaces:**
- Consumes: `pane_view::tab_title(root, pane, idx) -> String`, `Tree::page_tabs`, `Tree::page_focused`.
- Produces:
  - `pub fn quiet_entry(page: PageKey, pane: PaneId, idx: usize, name: String, since: u64, reminder: Option<&Reminder>, now: u64) -> Entry`. Its fields:
    - `session: SessionId(0)` and `names: vec![name.clone()]`;
    - `count: 0`, `alive: true` and `attention: Attention::Quiet`;
    - `group` as `Reminder::listed(Attention::Quiet, now)` gives it, with `Quiet` and no reminder;
    - `line`: an empty `String`;
    - `scheduled_until` and `repeats` from the reminder, as `page_entry` does.
  - `entries` makes one for every page with tabs and no terminal:
    - `pane` and `idx` are the page's focused pane and that pane's active tab;
    - `name` is `row_name(tree.session_name(page), typing, &tab_title(root, first_pane, 0))`, where the first tab is `page_tabs(page)[0]`;
    - `since` is `root.page_made.get(&page).copied().unwrap_or(0)`.
  - `menu_items(can_move: bool, alive: bool, scheduled: bool, terminal: bool)`: with `terminal` false it returns only `("Rename", Do(Rename))` and `("Close session", Do(Close))`. Every caller passes `count > 0` for `terminal`.
  - `RowAction::Close`, and `Root::close_page_tabs(&mut self, page: tree::PageKey, cx)`:
    - closes each tab of the page as Cmd+W does (`close_tab` by locating its id), stopping at one that will not close;
    - then calls `settle_page(page, self.row_beside(page), cx)`.

- [ ] **Step 1: Write the failing tests** in `sessions_list.rs`'s tests.

```rust
#[test]
fn a_page_with_no_terminal_is_one_quiet_row_named_for_its_first_tab() {
    let e = quiet_entry(7, 3, 1, "report.md".into(), 1_000, None, 2_000);
    assert_eq!((e.page, e.pane, e.idx), (7, 3, 1));
    assert_eq!(e.name, "report.md");
    assert_eq!((e.attention, e.group), (Attention::Quiet, Attention::Quiet));
    assert_eq!((e.count, e.since, e.ended), (0, 1_000, None));
    assert!(e.alive);
}

#[test]
fn a_row_with_no_terminal_offers_rename_and_close_only() {
    let items: Vec<_> = menu_items(true, true, false, false).into_iter().map(|(label, _)| label).collect();
    assert_eq!(items, ["Rename", "Close session"]);
    let terminal: Vec<_> = menu_items(true, true, false, true).into_iter().map(|(label, _)| label).collect();
    assert_eq!(terminal, ["Move to", "Remind me in", "Rename", "End session"], "a terminal's row is as before");
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `nice -n 15 cargo test -p arkos --bin arkos sessions_list:: 2>&1 | tail -15`
Expected: a compile error that `quiet_entry` is not found and `menu_items` takes 3 arguments.

- [ ] **Step 3: Implement** `quiet_entry`, the `entries` branch, the 4-argument `menu_items` (update its callers), `RowAction::Close`, `Root::page_made` and `Root::close_page_tabs` as specified above.

- [ ] **Step 4: Run them and watch them pass**

Run: `nice -n 15 cargo test -p arkos --bin arkos sessions_list:: 2>&1 | grep -E "test result|FAILED"`
Expected: `test result: ok`.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/sessions_list.rs crates/arkos/src/main.rs
git commit -m "Sessions list: a page with no terminal is a Quiet row with Rename and Close"
```

### Task 3: What a drop does, decided in one place

**Files:**
- Modify: `crates/arkos/src/drag.rs`: new types beside `DropTarget` (line 139), and tests in its `mod tests`.

**Interfaces:**
- Produces:
  - `pub enum ListDrop { Join(PageKey), NewRow, Nothing }`.
  - `pub fn list_drop(tab_page: PageKey, over: Option<PageKey>, whole_of_page: bool) -> ListDrop`:
    - `over` is the live row under the pointer, and None for anywhere else in the list (an ended row included).
    - `whole_of_page` says the dragged tabs are every tab of their page.
    - Over another page's row, it is `Join(that page)`. Over its own page's row, `Nothing`.
    - Elsewhere, it is `Nothing` when `whole_of_page`, else `NewRow`.
  - `pub struct SessionDrag { pub page: PageKey, pub label: String, pub position: Point<Pixels>, pub bg: Rgb, pub fg: Rgb, pub family: SharedString }`, with `at(self, position) -> Self` and a `Render` impl drawing the same chip as `TabDrag`'s. Share the chip's drawing through a private fn both call.
  - `pub fn session_drop_lands(dragged: PageKey, shown: PageKey) -> bool`: false when they are the same page.

- [ ] **Step 1: Write the failing tests** in `drag.rs`'s tests.

```rust
#[test]
fn a_tab_over_another_session_s_row_joins_it_and_over_its_own_does_nothing() {
    assert_eq!(list_drop(1, Some(2), false), ListDrop::Join(2));
    assert_eq!(list_drop(1, Some(1), false), ListDrop::Nothing);
    assert_eq!(list_drop(1, Some(2), true), ListDrop::Join(2), "a whole session's last tab may still join another");
}

#[test]
fn a_tab_anywhere_else_in_the_list_makes_a_new_row_unless_it_is_its_whole_session() {
    assert_eq!(list_drop(1, None, false), ListDrop::NewRow);
    assert_eq!(list_drop(1, None, true), ListDrop::Nothing);
}

#[test]
fn a_session_dropped_on_its_own_panes_lands_nowhere() {
    assert!(!session_drop_lands(4, 4));
    assert!(session_drop_lands(4, 5));
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `nice -n 15 cargo test -p arkos --bin arkos drag:: 2>&1 | tail -10`
Expected: a compile error that `list_drop`, `ListDrop` and `session_drop_lands` are not found.

- [ ] **Step 3: Implement** them and `SessionDrag` as specified.

- [ ] **Step 4: Run them and watch them pass**

Run: `nice -n 15 cargo test -p arkos --bin arkos drag:: 2>&1 | grep -E "test result|FAILED"`
Expected: `test result: ok`.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/drag.rs
git commit -m "Drag: decide a drop on the Sessions list, and a session's drag payload"
```

### Task 4: Dragging a session row onto a pane

**Files:**
- Modify: `crates/arkos/src/sessions_list.rs`: the live row in `render` (line ~650) gains `.on_drag(SessionDrag { .. }, ..)`. Ended rows and a row being renamed get no drag.
- Modify: `crates/arkos/src/pane_view.rs`: beside each `TabDrag` drop handler (body near 550-567 and tab bar near 717-740), add `on_drag_move::<SessionDrag>` and `on_drop::<SessionDrag>`. They compute the same `DropTarget`, and set `root.drop_target` only when `session_drop_lands(d.page, root.tree.page)`.
- Modify: `crates/arkos/src/main.rs`: `Root::drop_session`.

**Interfaces:**
- Consumes: `drag::SessionDrag`, `drag::session_drop_lands`, `Tree::merge_page`, `Place`, and `DropTarget` as Task 1 and Task 3 define them.
- Produces `pub fn drop_session(&mut self, d: &drag::SessionDrag, target: drag::DropTarget, cx: &mut Context<Self>)`:
  - It clears `drop_target`, and does nothing unless `session_drop_lands` holds and `d.page` still exists.
  - It maps the target:
    - `TabBar { pane, at }` becomes `Place::At(at)`;
    - `Pane { Center }` becomes `Place::End`;
    - an edge zone becomes `Place::Side(..)`, as `place_pane` maps it.
  - It calls `merge_page`, then removes `self.reminders[&d.page]` and `self.page_made[&d.page]`.
  - It clears `held`, `row_rename` and `session_cursor` when they name `d.page`.
  - Then it calls `save` and `cx.notify()`.

- [ ] **Step 1:** No headless test fits the gpui glue. Its decisions are Task 1's and Task 3's tested functions. Implement `drop_session` and the handlers.
- [ ] **Step 2: Check it builds**

Run: `nice -n 15 cargo check -p arkos 2>&1 | grep -E "^(warning|error)" | head`
Expected: no output.

- [ ] **Step 3: Check it on screen in a sandbox build**
  1. Copy the dev binary to the session's scratch `sandbox-bin`, then run `./arkos sandbox stop` and `./arkos sandbox <worktree>`.
  2. Make two sessions. Drag the second's row onto the first's tab bar, middle and right edge.
  3. Confirm that the tabs land, the row goes and the tab names stay.
  4. Confirm that a drag of the shown row lights nothing.
- [ ] **Step 4: Commit**

```bash
git add crates/arkos/src/sessions_list.rs crates/arkos/src/pane_view.rs crates/arkos/src/main.rs
git commit -m "Sessions: a session row dragged onto a pane brings its tabs into the session shown"
```

### Task 5: Dragging a tab onto the Sessions list

**Files:**
- Modify: `crates/arkos/src/main.rs`:
  - `Root` gains `pub list_drop_row: motion::Transient<tree::PageKey>` (the lit row) and `pub list_drop_new: motion::Transient<String>` (the placeholder's label).
  - Both join `any_motion`.
  - New `Root::drop_on_list`.
- Modify: `crates/arkos/src/sessions_list.rs`:
  - Each live row gets `on_drag_move::<TabDrag>`, which sets `list_drop_row` when `list_drop(..)` is `Join(page)`, else hides it.
  - Each live row gets `on_drop::<TabDrag>`.
  - The lit row draws `crate::chrome::keys_ring(&theme, &style)` at the highlight fade's opacity.
- Modify: `crates/arkos/src/sidebar.rs`:
  - The sessions block (the list and the "+ New session" area, line ~315) gets `on_drag_move::<TabDrag>` and `on_drop::<TabDrag>` for the `NewRow` case. A move outside the block's bounds hides both transients, the pattern `pane_view` uses to clear `drop_target`.
  - The placeholder renders between the "+ New session" button and the list, outside the list's scroll: a dashed-bordered row of `ROW_H` times its open progress (`motion::enter_progress` with `ENTER` while showing, `motion::exit_progress` with `EXIT` while closing), holding the tab's label in `text_dim`.

**Interfaces:**
- Consumes: `drag::list_drop`, `ListDrop`, `Tree::move_tab_to_page`, `Tree::page_from_tabs`, `pane_view::tab_title`, `Root::settle_page`, `Root::page_made`.
- Produces `pub fn drop_on_list(&mut self, d: &drag::TabDrag, over: Option<tree::PageKey>, cx: &mut Context<Self>)`:
  - The dragged tabs are `d.tab`, or with `d.whole` every tab of `d.pane`, by id.
  - `tab_page` is their page. It decides with `list_drop(tab_page, over, whole_of_page)`.
  - `Join(p)`: it calls `move_tab_to_page` for each tab. When the page shown is left with no tab, it calls `settle_page(shown, Some(p), cx)`.
  - `NewRow`:
    1. It reads the name first: `tab_title` of the single tab when there is one tab, else none.
    2. It calls `page_from_tabs`.
    3. If the single tab has a user name (`tree.tab_name`), it calls `set_session_name(new, Some(that name))`.
    4. It records `page_made[new] = now_ms()`.
  - `Nothing` does nothing.
  - Every path hides both transients, clears `drop_target`, saves, and calls `cx.notify()`.

- [ ] **Step 1:** No headless test fits the gpui glue. Its decisions are Task 1's and Task 3's tested functions. Implement it as specified.
- [ ] **Step 2: Check it builds**

Run: `nice -n 15 cargo check -p arkos 2>&1 | grep -E "^(warning|error)" | head`
Expected: no output.

- [ ] **Step 3: Check it on screen in the sandbox**, under navy (`material`) and under one other effect:
  1. Drag a terminal tab over another row. Confirm the ring fades in, then drop: the tab joins and the screen stays.
  2. Drag a named markdown tab over empty list space. Confirm the placeholder opens with its name and the rows slide, then drop: a Quiet row with that name.
  3. Drag over your own row. Confirm nothing lights.
  4. Drag the only tab of a one-tab session over the list. Confirm no placeholder appears.
  5. Release outside the window. Confirm everything closes.
  6. Drag a session's last tab onto another row. Confirm that session closes and the joined one shows.
- [ ] **Step 4: Commit**

```bash
git add crates/arkos/src/main.rs crates/arkos/src/sessions_list.rs crates/arkos/src/sidebar.rs
git commit -m "Sessions: a tab dropped on a row joins that session, elsewhere in the list it becomes one"
```

### Task 6: Docs and the hand checks

**Files:**
- Modify: `docs/guide.md`:
  - In the Sessions list section (~6-70), say how a tab dropped on a row joins that session, and how a tab dropped elsewhere in the list makes a new row.
  - In Layouts (~101-123), say how a row dragged onto a pane brings its tabs, with the same zones as a tab.
- Modify: `docs/development.md`: a Status row "Session drag" for:
  - each effect's join ring;
  - the placeholder's slide;
  - a drag released outside the window;
  - a merge of a session with a reminder;
  - a no-terminal row's Close.

- [ ] **Step 1:** Write the two guide paragraphs and the Status row, in the guide's plain prose.
- [ ] **Step 2: Run the whole suite and the check**

Run: `nice -n 15 cargo test --workspace 2>&1 | grep -E "test result|FAILED|panicked" | tail -15` then `nice -n 15 cargo check --workspace --all-targets 2>&1 | grep -E "^(warning|error)" | grep -v "future version"`
Expected: every `test result: ok`, except the two known `arkosd` pty failures. The check prints nothing.

- [ ] **Step 3: Commit**

```bash
git add docs/guide.md docs/development.md
git commit -m "Docs: dragging between the Sessions list and the panes"
```
