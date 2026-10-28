# Drag-selecting past the edge in a fullscreen program

**Date:** 2026-10-27. **Status:** implemented 2026-10-27. **Extends** section 3 of
`2026-10-13-arkos-design.md` (mouse selection and reporting) and the selection item of
`docs/guide.md`, which both say that dragging past the pane's edge scrolls the history "except
in programs that take the mouse or the whole screen". After this milestone that exception is
narrowed to programs that do not report the mouse (vim, `less`): Claude Code's fullscreen
renderer gets the same drag past the edge a shell has, with a copy that covers everything the
drag passed over.

## 1. Why this exists

Dragging a selection past the top or bottom edge of a terminal scrolls its history and extends
the selection as it goes. Arkos does this for a shell's own history
(`TerminalCore::drag_scroll_step`), but not in Claude Code's fullscreen renderer, which is where
Cyrus does nearly all of his reading. The 2026-10-27 recording shows the problem: a drag from the
last line of an answer up past the top edge selects one screen of text and stops there, so
copying a long answer takes several copy, paste, scroll rounds.

The reason is structural, not a missing branch. Claude Code's fullscreen renderer runs on the
alternate screen with SGR mouse reporting (`?1049`, `?1000`, `?1002`, `?1003`, `?1006`; checked
against 2.1.312 on 2026-10-27). The conversation's history lives inside Claude Code; Arkos's
grid holds one screen, and there is no history behind it to scroll. Sending Claude Code the
wheel during the drag would scroll its view, but the rows the drag started on would leave the
grid and Cmd+C would still give one screen, a different one. That is worse than today: it looks
right and copies the wrong text.

So the drag has to do two things at once: drive the program's scroll with the wheel reports it
asked for, and remember every row that passes through the grid while the selection lives, so the
copy is the text the drag passed over and not whatever happens to be on screen when Cmd+C comes.

A spike on 2026-10-27 (a pseudo-terminal running `claude`, wheel reports sent to it, every byte
it wrote replayed through `TerminalCore`) showed this is tractable. Claude Code repaints by
rewriting changed cells, with no synchronized-update bracket, and after every wheel report the
grid settled within 30 ms. Comparing the grid before a report with the grid after, the shift that
lines up the most non-blank rows matched 28 to 30 of 40 rows; the runner-up matched 1. Each SGR
wheel report moved the content exactly one row. The rows that never match (the prompt box, the
status rows, the sticky header that shows the message the view is inside, the "Jump to bottom"
overlay on the last content row) fall out of the count on their own.

## 2. What the user sees

In a Claude Code tab in fullscreen mode, with a long answer on screen:

- Press on the last line of the answer and drag up past the top edge. The conversation scrolls
  up, faster the further the pointer is above the pane (the same rates as a shell: 6 rows per
  second per cell of distance, at most 60). The tint grows from the top edge down; once the
  pressed line has scrolled off the bottom, the tint covers the whole conversation area and
  nothing of the prompt box or the status rows under it.
- Bring the pointer back inside and the scroll stops; the tint ends at the row under the
  pointer. Release the button, and the selection stays.
- Cmd+C pastes everything from the pressed cell to the released one, oldest row first, with
  Claude Code's own line wraps joined as they are today (`reflow`). The sticky header and the
  "Jump to bottom (click)" overlay are not in it, nor is the prompt box. Shift+Cmd+C pastes the
  same rows as shown, unjoined.
- Wheel the conversation after the release: the tint follows its text, and Cmd+C still gives
  the whole selection, including rows that have scrolled off the screen by then.
- The same drag downward, from the top of an answer past the bottom edge, scrolls the
  conversation down and selects to the pointer.
- A drag past the edge in vim or `less` still does nothing, as today.

## 3. Goals, non-goals, anti-goals

Goals:

- One drag past the edge selects an answer of any length in Claude Code's fullscreen renderer,
  and one Cmd+C copies it whole.
- A selection made on the alternate screen keeps its place on its text while the program
  scrolls, as a shell's does while the history scrolls.
- No change for a shell, for a program that does not report the mouse, or for the daemon.

Non-goals:

- Programs on the alternate screen that take the wheel as arrow keys (vim, `less`, `MouseMode::
  Arrows`). An arrow key moves vim's cursor rather than its view, and `less` is read in one
  screen at a time. The exception for them stays as documented.
- Text that changes in place after it was recorded (an answer still streaming into a row the
  drag has already passed). The copy gives the row as it was last seen moving.
- Block selections. They stay a rectangle of grid cells, as today.
- Selecting across a program's full repaint (a resize, Claude Code redrawing its transcript
  after Ctrl+O). The record closes then (section 4.2, "Closing") and the selection becomes a
  plain grid selection, as today.

Anti-goals:

- A copy that silently gives one screen when the user saw the whole conversation tinted. Where
  the record cannot follow the program, the tint must show it (it stays on the grid rows while
  the text moves) rather than pretend.
- Any wheel or click reaching a program that did not ask for mouse reports, and any bytes
  reaching an ended session.
- A frame loop that runs while nothing is held. The drag past the edge paints frames only
  while the button is down and the pointer is outside the pane, as the shell's does.

## 4. How it works

### 4.1 Who scrolls

`drag_scroll_step` today returns false on the alternate screen or when the program reports the
mouse. It gains a branch for `MouseMode::Report` on the alternate screen of a live session: the
rate times the frame's time is added to a `WheelCarry`, and each whole row due becomes one SGR
(or X10, whichever the program asked for) wheel report at the pointer's column and the edge row,
the same bytes `Terminal::wheel` sends. The core returns the bytes and `Terminal` writes them to
the session, as `Wakeup::pty_write` is written today. The step returns true while the drag is
held past the edge; the core cannot tell when the program's history runs out, and a program
that ignores the reports just gets a few per second until the pointer comes back in or the
button comes up. `MouseMode::Arrows` and `Local` on the alternate screen keep returning false.

An ended session is `Local` for this as for the wheel: nothing is sent, and since the alternate
screen of an ended session has no history either, nothing scrolls.

Claude Code scrolls one row per report when `wheelScrollAccelerationEnabled` is false, which the
doctor already sets. With acceleration on, a report may scroll several rows; the record below
measures what actually moved, so the selection stays right and only the speed differs.

### 4.2 The record

While a selection may exist on the alternate screen, `TerminalCore` keeps a **record** of the
rows that have passed through the grid: `ledger.rs`, a pure module with its own tests. It is
the only new state.

- **Lines.** The record numbers rows in a fixed frame: a grid row `r` is record line `r -
  offset`, where `offset` starts at 0 and changes with every shift the program makes. Lines
  carry the row's text (its characters, trailing blanks trimmed, exactly what `rows_for_paint`
  shows for the cells).
- **Opening.** A left press on the alternate screen of a live session whose `mouse_mode` is
  `Report` opens a record with `offset` 0 and every grid row as a line, all provisional (see
  below). The press's cell is the record's anchor; the record has no end yet, so no selection
  exists until a gesture or an edge step gives it one. A press anywhere else (the primary
  screen, `Arrows` or `Local` mode, an ended session) opens nothing, and those cases keep
  today's grid selection unchanged. A program that turns reporting off while a record is open
  (Claude Code handing the terminal to a subprocess) leaves the record following shifts; only
  the edge step, which checks the mode each frame, stops sending.
- **Shifts.** A shift is found by comparing two grids as rows of text. The shift `k` (not zero,
  in either direction, at most the grid's height minus one) that lines up the most non-blank
  rows (`old[i] == new[i + k]`) is the program's scroll, when it lines up at least four rows and
  at least two more than the runner-up; otherwise there is no shift. A shift also has to move
  at least half of the grid's rows (its block and band together); a smaller one is a pane
  scrolling inside a program that splits the screen, and the record treats it as a repaint in
  place (added 2026-10-27 after review). A shift names its **block**, the contiguous range of
  new rows from the first matched row to the last (blank rows inside it are content, such as a
  paragraph gap, and move with it), and its **band**, the `|k|` rows next to the block on the
  side the text came in from (above it for `k > 0`, below for `k < 0`), clipped to the grid.
  Committing a shift does three things, in order:
  1. `offset += k`, so every existing line keeps naming the row it was recorded from.
  2. Every line in the block is rewritten from the grid and confirmed (a row that was under the
     "Jump to bottom" overlay comes out clean once it moves). Each band row is written as a new
     provisional line.
  3. Every provisional line that stayed put is dropped: the grid row it was recorded at (its
     line plus the offset before this shift) is on the grid and shows its text both before and
     after the shift. These are rows that did not move with the text: the prompt box and the
     status rows from the opening snapshot, a sticky header that arrived in a band when its
     message's first row scrolled off (it then stays put while the rows under it keep moving).
     A provisional line whose row now shows something else is kept: it is text that left the
     screen before it had moved with a shift, such as the pressed row pushed under the prompt
     box by the first scroll, or a row that scrolled off the top on a drag downward.
  A line that has moved with a shift once is confirmed and is never dropped. The "Jump to
  bottom" overlay never reaches the record on a drag downward: it sits on the last content row,
  which is always one past the band, and a row is recorded only once it has moved up from under
  it. A chrome row whose text changes every frame (a spinner, a timer) is neither dropped nor
  overwritten until the band reaches its line, so a drag downward during a running turn can copy
  one such stale line; accepted.
- **Batches.** Output reaches the grid in batches (`apply_all`: the daemon sends one event per
  pty read, the window applies whatever events landed together), and one of Claude Code's
  frames can span several. The spike saw a scroll's frame of about 360 bytes arrive in one read
  in 21 of 22 steps and in three reads over 27 ms in the other (the step where the sticky header
  and the overlay first appeared); frames for a wider, taller screen of prose are bigger and
  will split more often. A half-painted grid looks like a complete one whose unpainted rows are
  chrome, so no single comparison can tell them apart, and comparing each batch with the one
  before would count a split frame twice. The record therefore keeps a **baseline**, the rows'
  text as of its last commit (the opening snapshot at first), and a **pending** shift, the
  latest interpretation of everything since, revisable until the next frame begins. On every
  batch, with `old` the rows before applying it and `new` after:
  1. `d` is the shift found between the baseline and `new`.
  2. With no pending shift, `d` becomes the pending shift (or nothing).
  3. With a pending shift and `d` of the same `k`, `d` replaces it (a frame completing, or a
     spinner repainting after it).
  4. With a pending shift and `d` of another `k` or no shift at all, a new frame has begun: the
     pending shift is committed against `old` (which is the grid it was found on), the baseline
     becomes `old`, and the shift between `old` and `new` becomes the pending shift, or nothing.
  5. When there is now no pending shift and no `d`, and more than half of the baseline's
     non-blank rows differ from `new`, the program has repainted beyond recognition (a transcript
     redraw, a resize it handled itself): the record closes. Fewer changed rows are a repaint in
     place (a spinner, an answer still streaming into its last rows) and leave the record alone;
     the baseline is not moved, so a shift found later still lines up with it.
  A pending shift counts: the tint, the copy and `selection_active` all read the committed
  record with the pending shift applied. The last frame of a drag may stay pending for good,
  which is fine. Comparing against the baseline rather than the previous batch is what makes a
  split frame safe: its first half gives a pending shift with a small block, its second half the
  same `k` with the whole block, and the next frame's different `k` commits that. A frame whose
  first read lines up fewer than four rows gives no pending shift and nothing moves until its
  next read, which finds the whole shift against the unchanged baseline.
- **Selection.** The record keeps the selection's two ends as (line, column, side), the anchor
  from the press and the end from the latest gesture or edge step. Gestures on the alternate
  screen set them: a drag's `Start` and `Extend` set the end from the pointer's row (`row -
  offset`); a double or triple click's `Select` sets both ends from the range alacritty expands
  the gesture to, so a word or a line is recorded as the cells it covers; an edge step sets the
  end to the edge row at the pointer's column, top row left side or bottom row right side as the
  shell's does. A shift leaves the ends alone, because lines do not move; the grid moves under
  them. While a record is open it is the selection: `selection_active` (which `Terminal::
  mouse_moved` passes to `Clicks::moved` to choose `Extend` over `Start`), `selection_text` and
  `copy_text` read the record, and `Clicks::anchor` is not consulted.
- **The tint.** After every gesture, edge step and batch of output, alacritty's selection is
  rebuilt as a `Simple` selection over the record's range projected onto the grid (line +
  offset), clamped to the rows that moved with the text in the last shift (the block plus the
  band; the whole grid before any shift). The clamp keeps the prompt box and the status rows
  untinted once the anchor has scrolled under them, and a range entirely off screen leaves no
  tint. Alacritty drops its own selection when output erases a row of it (`clear_line`,
  `clear_screen`); rebuilding after every batch is what keeps the tint through that, and
  alacritty's own drops are never a closing event. Alacritty's selection is only the tint while
  a record is open: `selection_spans` reads it as today, and nothing else does.
- **The copy.** `selection_text` with a record open is the record's lines from the lower end to
  the higher, in line order, missing lines skipped, the first and last clipped to their columns
  by the same cell rules as alacritty's `Simple` selection (`Selection::to_range` and
  `selection_to_string`): a row's text, a newline between rows, no trailing newline on the last.
  `copy_text` re-flows it with `reflow_at` as today, `first_col` the start's column and the
  input-box inset when the start's line is on screen in the input box. Shift+Cmd+C ("copy as
  shown") takes `selection_text` unjoined, as today.
- **Closing.** The record closes (is dropped) when the selection is cleared (a plain click; a new
  press elsewhere opens a fresh one), on a release or a cancel with no selection made (a click
  the program heard), on a resize, on the repaint-beyond-recognition rule above, and when the
  program leaves the alternate screen (alacritty clears the selection with the screen swap, as
  it does today, so nothing outlives Claude Code's exit). Closing for any other reason leaves
  alacritty's selection as it is, a plain grid selection from then on.

A press, a wheel of the program's view, then a drag: the record opened at the press has followed
the shifts, so the anchor names the text that was under the press, matching the shell's
behaviour in the status checklist.

### 4.3 What is unchanged

The local history path of `drag_scroll_step`, `Terminal::wheel`, `Clicks` and every gesture on
the primary screen or on an ended session are untouched. `Clicks::anchor` is not consulted while
a record is open; the record's anchor replaces it. The daemon, the protocol and the pane view
are untouched: the pane already hears presses, moves, releases and the paint loop already runs
the edge step from the pointer's position each frame.

### 4.4 Cost

A record is at most the grid's rows plus what scrolled past, as strings, plus a baseline of one
grid; a 10,000-row drag is a few hundred kilobytes. Shift detection runs per output batch while
a record is open: for a 40 by 100 grid that is at most 78 candidate shifts of 40 row
comparisons, well under a millisecond, and a selection in Claude Code is open for seconds. A
record is open only between a press and the selection's clearing, so an idle tab pays nothing.

## 5. Decisions made here

- Report-mode programs only; Arrows-mode programs keep today's exception (section 3). Reporting
  turned off mid-record leaves the record following shifts.
- The record opens at the press, not at the first edge step, so a wheel between the press and
  the drag is followed (section 4.2).
- Four matched rows and a margin of two over the runner-up decide a shift. Shifts are found
  against a baseline held since the last commit and stay pending until the next frame begins,
  so a frame split across pty reads is never counted twice (section 4.2, "Batches"). No timer
  is involved: every step is driven by a batch of output, a gesture or a frame of the drag.
- A repaint in place leaves the record alone; a repaint that changes more than half of the
  non-blank rows without a shift closes it. A stale record shows as a tint that stays on grid
  rows while the text moves (the anti-goal in section 3 is met by the tint, not by giving up).
- While a record is open it is the selection; alacritty's selection is only its tint, rebuilt
  after every batch, and alacritty dropping it is never a closing event.
- The tint is clamped to the rows that moved last, so chrome is never tinted by a scrolled-off
  anchor.
- Lines are text, not cells: the copy needs text, and the tint comes from the grid.

Residual risk, accepted: a frame whose first read lines up fewer than four rows and whose
second read arrives together with the next frame is read as one larger shift, and a provisional
line or two from the first read can be dropped. Nothing confirmed is ever lost, and the next
drag starts clean.

## 6. Left to the builder

- The exact shape of `ledger.rs` (its types, how a shift's block and band are returned, how a
  pending shift is held), as long as shift detection, the baseline and pending rules, the
  provisional rule and the range-to-text function are pure and tested against a grid-sized
  fixture. Tests replay the spike's shapes: a sticky header arriving in a band and being dropped
  at the next shift; an overlay row coming out clean once it moves; a frame split into two
  batches giving the same record as the frame in one; two frames in consecutive batches
  committing the first; a transcript redraw closing the record.
- Where the "before" rows are taken in `apply_all` (a copy of the rows' text when a record is
  open, nothing otherwise).
- Whether alacritty's selection is created at the press or only once the record has an end (a
  plain click in Claude Code need not show a one-cell tint).
- How `TerminalCore::drag_scroll_step` returns the report bytes (a return type carrying both
  the bytes and whether to keep stepping, or a `pty_write` field on the core drained by
  `Terminal`).

## 7. Documentation

`docs/guide.md`'s selection item loses "or the whole screen" from its exception and says that in
a program that reports the mouse (Claude Code's fullscreen renderer) the drag past the edge
scrolls the program's view and Cmd+C copies everything the drag passed over. `docs/development.md`'s
status checklist replaces "no auto-scroll in Claude Code's fullscreen renderer, vim or `less`"
with a row for this: drag from the last line of a long answer up past the top edge, release,
Cmd+C, paste: the whole answer, no "Jump to bottom" text, no prompt box; wheel after the release
and the tint stays on its text; vim and `less` still do not scroll. Section 3 of
`2026-10-13-arkos-design.md` gets a one-line pointer here.
