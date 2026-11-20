# Dragging between the Sessions list and the panes

Asked by Cyrus on 2026-11-19. Approved in conversation the same day, pending this write-up.

## Why

Cyrus had one session holding several Claude Code sessions as tabs. Over time it became too
cumbersome to manage, and he would rather have had each one as its own row in the Sessions list
(pane 0). Today nothing moves a tab from one session to another. A tab moves only between the
panes of the session on screen, and session rows cannot be dragged. This feature makes both
directions one drag:

- **Split a crowded session apart:** drag a tab onto the Sessions list and it becomes a row of
  its own.
- **Combine:** drag a row onto a pane and its tabs join the session on screen. Or drag a tab onto
  another session's row and it joins that session.

## Goals

1. **Any tab can be dropped on the Sessions list.** That covers a terminal, a markdown file, a
   file in the viewer, a grid, a video or a web page.
   - Dropped on another session's row, it joins that session.
   - Dropped anywhere else in the list, it becomes a new session with a row of its own.
2. **A session row can be dropped onto a pane** of the session on screen. All of its tabs move
   into the session on screen and its row goes.
3. **Tab names never change** in a move. A new row created from a tab takes that tab's name.
4. **A drop onto itself has no effect.** This covers a tab dropped on its own session's row, and
   a session row dropped onto its own panes. Nothing is refused or announced; nothing happens.
5. **While dragging, the highlight says which drop it is.** One look means "this joins that
   session", another means "this makes a new row", and no highlight means "nothing happens".

## Non-goals

- Reordering rows by drag. Row order stays derived: rows sit in their state's group, newest
  first.
- Dropping a session row onto another session's row (merging two sessions that are both off
  screen).
- Dragging between windows. gpui drags live in one window.
- Switching the sidebar from Files to Sessions when a drag hovers its tab header.
- A "Move to new session" menu item. It could come later, but it is not part of this design.

## Anti-goals

- **Any lost tab, session name or terminal.** A move never closes, restarts or detaches a terminal.
  Its daemon session is untouched, since pages live in the window.
- **A list that jumps about during a drag.** Only the new-row placeholder moves rows, and it uses
  the app's own slide.
- **A drop indicator that lies.** A drop that would do nothing shows nothing.

## The behaviour

### A tab dropped on the Sessions list

The tab being dragged is always on the session on screen (the shown page), since only its panes
are visible.

**Over another session's row: join.**
- That row lights up as the keyboard's ring lights a row, in the effect's `keys_ring` look
  (outline, pill, glow, pixel or wash). The join therefore reads right under every effect.
- When dropped, the tab is appended to the end of the tabs in that session's focused pane
  (`Page::focused`).
- The session on screen stays on screen, unless the tab was its last. Then that page closes and
  the session the tab joined is shown.

**Anywhere else in the list: a new row.** That means the gaps between rows, the group headings,
and the empty space below the last row.
- A dashed placeholder row opens just under **+ New session**, holding the tab's label. The rows
  below slide down to make room.
- When dropped, a new page is made whose one pane holds the tab. Its row appears in its state's
  group, as any row does, and the placeholder closes.
- When the drag leaves the list, the placeholder closes, and the rows slide back.

**Over the session's own row:** no highlight, and a drop does nothing. That is the row of the
shown page, the session the tab already belongs to.

**The new row's name.**
- A tab with a name the user gave it (`Tree::names`) gives the new session that name as its
  session name (`Tree::set_session_name`).
- An unnamed terminal tab leaves the session unnamed. Its row then follows the program's title,
  which is what the tab shows.
- An unnamed non-terminal tab: the row shows the tab's automatic label (the file's name, the
  page's title).

**The session left behind** keeps every other tab and keeps its row, even if only files remain.
When the moved tab was its last, it closes as an emptied page does today, and the new or joined
session is shown.

### A session row dropped onto a pane

- The rows that can be dragged are the live session rows. An ended row (a closed Claude session
  kept in the list) cannot be dragged. The shown session's row can be dragged, but no drop target
  takes it, since every visible pane is its own (goal 4).
- The drop zones are those of a tab drag (`drag::zone_for`, `drag::insert_index`), with the same
  highlights:
  - On a tab bar, the tabs are inserted where the tab drop would go.
  - In the middle of a pane, they are appended to that pane's tabs.
  - On a pane's edge, they make a new pane on that side, holding all of them.
- **Order:** the dragged session's panes in their Cmd order, and each pane's tabs in their order,
  flattened into the one place.
- **Its row goes,** with its session name (`Tree::close_page` drops it) and its reminder, if it
  had one. Tab names stay. The session on screen keeps its own name and its own reminder.
- **The ghost** is a chip, as a tab drag's is, carrying the row's shown name.

### Highlights and motion

- The join highlight fades in and out with the existing highlight fade.
- The placeholder's opening and closing, and the rows' slide, use `motion::Slide` with Material
  duration and easing tokens: entering decelerates; leaving accelerates and is a little quicker.
  All of it is instant under Reduce Motion, and each new animated thing joins `Root::any_motion`.
- Every colour is read from `root.theme` at paint time.

## Constraints

- Follow the repository's CLAUDE.md:
  - Headless tests, named as sentences and written first.
  - Plain prose comments with no em-dashes.
  - `cargo check --workspace --all-targets` clean.
  - Builds with `nice -n 15`, one cargo at a time, never `--release`.
- **Layout:** a moved tab's place, a new page and a closed page are saved with the window's
  layout through the existing save. The tab names (`names`) are keyed by `TabId`, and the session
  names by `PageKey`, so ids are kept across a move, never reissued.
- **Daemon:** no daemon protocol change. A terminal tab keeps its `SessionId`.
- **Docs:**
  - `docs/guide.md`, in the Sessions list and Layouts sections, says how to split and combine
    sessions by dragging.
  - `docs/development.md` gets a Status row, "Session drag", for the hand checks: each effect's
    join highlight, the placeholder's slide, and a drag cancelled outside the window.

## Approaches

1. **Chosen: extend the existing drag system.**
   - A new drag payload, `drag::SessionDrag`, is carried by a live session row.
   - The Sessions list and its rows become drop targets for `TabDrag`.
   - The panes become drop targets for `SessionDrag`.
   - New tree operations move tabs between pages.

   It reuses the tab drag's zones, ghost, highlights and slide, so it looks and behaves like what
   is already there.
2. **A "Move to new session" menu item and a "Move into…" submenu.** It is simpler to build, but
   it is not what was asked. It was set aside, and could come later.
3. **Any tab drop on the list makes a new row, wherever it lands.** It is simpler, but merging
   could then only happen by dragging a whole row onto a pane. Cyrus chose join-on-row.

## Decided by Cyrus

- Any kind of tab can be dropped on the list, not only terminals.
- A drop onto a session's own row or its own panes does nothing.
- A tab dropped on top of another session's row joins it. Elsewhere in the list, it makes a new
  row. The highlight shows which.
- Tab names are unchanged by any move. A new row takes the tab's name.

## My calls (correct any of them)

- **A row with no terminal goes under Quiet,** since nothing in it asks anything of the user.
- **The join look and the new-row look.**
  - A joined tab lands at the end of the session's focused pane.
  - The join highlight is the keyboard ring's look.
  - The new row is a dashed placeholder under **+ New session** that rows slide away from.
- **What happens when a session empties.** When the moved tab was the shown session's last, that
  session closes and the destination is shown. Otherwise the user stays where they are.
- **A merged session's reminder is dropped with its row.** The session on screen keeps its own.

## Settled after the builder check (my calls)

- **A pane dragged by its number** (`TabDrag.whole`) counts as its tabs dragged together.
  - Dropped on another session's row, its tabs all join that session's focused pane, in order.
  - Dropped elsewhere in the list, it becomes a new session holding that one pane.
  - Its session name is the tab's name when the pane holds one tab. With several tabs it is
    left unnamed, so the row follows its own rules (its first terminal's title, else its first
    tab's label).
  - "Its last tab" below then reads "the page's last tabs": the page closes when nothing is
    left in it.
- **An ended row** (a closed Claude session kept in the list) is not a session to join. A tab
  over it counts as "elsewhere in the list": the placeholder shows, the ended row is not lit,
  and a drop makes a new row.
- **The shown session's only tab, dropped elsewhere in the list,** would make the same session
  again. So it is a drop onto itself: there is no placeholder, and nothing happens. The same
  goes for a whole-pane drag of the shown session's only pane.
- **A row for a page with no terminal:**
  - Its `pane` and `idx` are its focused pane and that pane's active tab, with no lead terminal.
  - Its `since` is the moment it was made by the drop, and it is Quiet.
  - A click shows it, as any row.
  - Its menu offers Rename and Close only. Close closes its tabs and its page.
  - Keyboard cycling and `held` treat it as any row. It has no reminder badge unless one is set
    on its page.
- **When the shown session empties because its last tab joined another session,** it closes
  with its session name and its reminder, as a merged row does. The joined session is shown
  (`close_page` with it as `then`).
- **The dragged page closing mid-drag** (its terminal exited, say): the drop does nothing. A
  maximized pane takes a session drop as it takes a tab drop.
- **The placeholder** sits just under **+ New session**, above the list's scrolling rows, so it is
  seen however far the list is scrolled.
- **Hovering the sidebar's Files tab is not a drop target.**
- **A named terminal tab made into a new row keeps its tab name,** and the session takes the same
  name.
- **Off-screen views:** a tab joining a page that is not shown must keep its live view (a
  terminal's grid and scroll, a video's position) as a tab on any hidden page does. The plan
  starts by confirming in code that views are keyed by tab, not page.

## Left to the builder

- The new tree operations and their names, for:
  - taking a tab from the shown page into another page;
  - making a page from a tab;
  - moving a whole page's tabs into a place on the shown page.
- How a page with no terminal becomes an `Entry` in `sessions_list::entries`. The current
  `page_entry` returns None for one.
- Where the drop handlers sit in `sessions_list.rs` and `pane_view.rs`. How the placeholder is
  laid out within the list's scroll.
- The ghost chip's look for a session row, within the `TabDrag` chip's style.
