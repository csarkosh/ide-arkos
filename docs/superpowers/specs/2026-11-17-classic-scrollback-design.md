# Scroll up through a whole Claude Code session (2026-11-17)

Cyrus: "The main objective of this IDE is to be durable. This fails if I cannot scroll back in the
message history when Claude or this IDE is having some sort of issue." And: "From a user's
point-of-view, they just want to scroll up. They don't want a separate pane / tab opening up because
of technical issues." The cause and the first plan are in
`docs/superpowers/plans/2026-11-17-classic-scrollback-handoff.md`. This is what was measured, what
was built on the branch `classic-scrollback`, and what is left for Cyrus to decide.

## What was measured

Claude Code 2.1.327, recorded through a pseudo-terminal with
`CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1` (a resumed, forked 143-row conversation, no prompt sent),
then the recorded bytes played through `TerminalCore` with each resize.

- **The env var forces the classic renderer** over `"tui": "fullscreen"` in the settings file.
- **Nothing clears the history, but a compaction erases the screen.** Start, resume, sixty
  resizes, `/compact`, `/clear` and exit sent no `ESC[2J` and no `ESC[3J`. But when it compacts,
  and at `/clear`, Claude Code erases its screen exactly as at a resize and prints a frame that no
  longer holds the conversation. In a recorded Haiku session (70 rows, `/compact`, `/clear`) the
  last four rows of the answer and the "Conversation compacted" line never reached the history:
  the classic renderer alone still loses the last screenful before every compaction.
- **A resize is not the old cursor-up erase.** The October bug (stale input-box fragments, because
  the erase counted lines at the old width) is gone from Claude Code itself: at every change of size
  it homes the cursor, erases every screen row (`ESC[2K ESC[1B` per row) and prints again. There is
  no debounce: a dragged divider is a repaint per column.
- **What it prints again assumes a terminal that left its screen alone.** It reckons its frame's last
  rows are on screen (as many as the new height shows), prints the text of those rows wrapped for
  the new width, and never fewer rows than fill the screen. So: narrower, it prints more rows than
  the screen has and the extra scroll into the history; shorter, it prints only the last rows and
  assumes the rest went to the history; taller or wider, it prints rows that are in the history
  already.
- **alacritty's resize does not match that**, and the damage is in the scrollback, not on screen:

  | Resize and back, 143 rows at 80 by 24 | Before | With this branch |
  |---|---|---|
  | narrow to 60 columns | 23 rows repeated | 14 repeated |
  | shorten to 14 rows (a split below) | **5 rows lost** | identical to before |
  | a divider dragged 20 columns, a column at a time | 176 rows repeated | 16 repeated |

  Narrower: the reflow pushes part of the old screen into the history, and Claude Code prints it
  again. Shorter: alacritty keeps only enough rows to hold the cursor (which rests on the prompt,
  five rows above the frame's end) and drops the rows under it; Claude Code prints only the last
  rows again, so the rows between were erased and never came back.
- **What is left is Claude Code's own.** The rows still repeated come from widening: it prints the
  last screenful again at the new width while the same text, wrapped narrower, is in the history.
  Playing the same bytes through a grid that never resizes leaves the same 15 to 17 rows, so no
  terminal avoids them without guessing which rows match.

## What was built

1. **Classic for every shell Arkos starts** (`arkosd`, `shell_env`): the env var is in every
   session's environment, new or recovered, before the session's own entries. The settings files
   are not touched.
2. **A resize leaves a redrawing program's screen to it** (`TerminalCore::resize`,
   `resize_under_a_redrawing_program`). When the program on the main screen is known to redraw
   (`program_redraws`):
   - shorter: the rows that no longer fit go to the history, counted from the end of what the
     program printed (`used_rows`), not from the cursor;
   - taller: rows come back from the history only if the program printed them at this width
     (`fresh_rows`, a lower bound), since an older row may hold more text than the row printed in
     its place; left in the history that text is at worst there twice, never gone;
   - narrower or wider: each screen row stays a row, cut or padded, and only the history reflows.

   A shell's screen reflows as before. The program is known by the theme pushes classic Claude Code
   asks for (`CSI ? 2031 h`, off again when it leaves) or by its hooks' agent record
   (`set_agent_redraws`), which a replay that has lost its start cannot say.
3. **A screen erased unasked is kept** (`before_the_program_erases_its_screen`). The start of
   the erase (`ESC[H ESC[2K`) is watched for in a redrawing program's output. When it answers a
   resize the rows are let go, since the same conversation is printed again. When nothing asked
   for it, the screen's rows go to the history first, as if the program had scrolled them off. The
   same recording then reads whole: all 70 rows once, `/compact`, "Conversation compacted",
   `/clear`, the new session.
4. **The replay remembers its sizes** (`protocol::replay`). A replay is parsed by every window that
   opens later (each update relaunches the window), and bytes written for one size parsed at
   another land in the wrong cells. The daemon writes a size mark (`OSC 7711 ; size ; ...`) into the
   replay at every resize and closes an attach's replay with the size the session has; the window
   parses the replay at those sizes (`apply_replay`), then resizes to its pane. A window older than
   the marks ignores them; a replay older than the marks is parsed as before.
5. **The shoot names its model** (`[scene] model`, default `sonnet`): a check never runs on the
   account's default model.

## Checked on screen

A throwaway shoot (Haiku, a 120-row answer, `"tui": "fullscreen"` in the settings file) in a real
window: Claude Code drew down the pane, not on the alternate screen. The pane was narrowed from 74
to 38 columns by its divider, widened to 82, and went through some forty changes of height; the
input box was whole in every still, with no fragment of an older frame. The session's saved
replay held 67 size marks; parsed again at its sizes it held all 120 rows of the answer.

Not checked on screen, and left for hands: scrolling up past a compaction in a window (checked
from recorded bytes instead, because the shoot's typed letters arrive in Arkos as "a", so
`/compact` and the find bar could not be driven), the feel of the scroll, and a relaunch.

## The input box held at the pane's foot

Cyrus, 2026-11-17: "in the classic /tui view, is it possible to keep the claude user input footer
fixed to the bottom of the screen despite user scroll position?" Claude Code cannot: its box is
the last rows it wrote, and the scroll is the terminal's. So the terminal holds it
(`TerminalCore::pinned_frame`): while the view is scrolled up under a redrawing program, the live
screen is searched for the box (the last row that opens with the prompt and hangs under a rule,
through the last row with anything on it, so the status lines come too), and the painter draws
those rows over the pane's last rows on plain paper, on whole rows whatever the scroll's
fraction, with the cursor where it is in the box. Typing then keeps the place in the history
(`key_returns_to_bottom`); Enter goes to the bottom. Nothing is held when the screen shows no
box (a question or a menu in its place) or when the box would take more than half the pane.
Checked in a window with a Haiku session: scrolled a page up, three characters typed, the box
and its status lines at the foot with the text in it. Left as it is for now: a click or a drag on
the held rows lands on the history under them, and the "jump to bottom" chip sits over the right
end of the status lines.

## One resize a drag

Cyrus, in the sandbox: "why is the indentation weird after I change the width back and forth?"
His session's replay (37 resizes: 84 columns down to 54 a column or two at a time, back up to 71)
parsed again shows why. Claude Code prints its screen again at every resize, starting at a row of
the new wrapping; the paragraph that straddles the top of the screen is then in the history in
its old wrapping and on screen in the new, so each step can leave a seam: a lone "today." or "for
today." under the paragraph's older copy. Thirty-seven steps left a stack of them. And rows
already in the history were written for a wider pane, so the terminal wraps them a character at
a time from column 0 ("mostly su" / "nny"), as any terminal does, where Claude Code wraps at
words under an indent.

So a redrawing program's resize waits until the pane has held one size for 150 ms
(`resize_settled`): a drag is one resize and at most one seam, not one a column. The grid keeps
its size meanwhile, so the text re-wraps when the drag rests, not under the pointer. A shell is
resized at once, as before. Left: the one seam a resize can still leave.

## The history wrapped at words

Cyrus: "re-wrap the history at word boundaries too". At a change of width under a redrawing
program the history no longer goes through alacritty's reflow. Its rows are taken out, put back
together into the lines they were written as, and wrapped for the new width by `rewrap`: after
the last space that fits, the later rows under the line's own indent (past a bullet, a mark or a
number that opens it), a token wider than the pane cut where the width falls, a band's fill cut at
the pane's edge and never a row of its own. Every row of a wrapped line but its last ends in a
`WRAPLINE` cell, a hidden space where the break stood for a space, and a later row's indent is
hidden spaces, so the lines can be put together again: narrowed and widened, a history shows the
rows it had, by any path of widths. The wrapped rows go back into the grid through the blank
screen (`resize_columns_keeping_the_screen`), and a view scrolled into the history stays on the
line it was on. A full history (20,000 rows of 120 columns) wraps in about 0.2 s in a debug
build, once a drag. His session's replay parsed again reads "mostly sunny," / "  with a" where
it read "mostly su" / "nny, with a".

Claude Code's own rows are joined first. It ends every row it wraps with a newline, so to a
terminal each is a line, and an answer written in a narrow pane stayed a narrow column in a wide
one (Cyrus's second screenshot). Rows not yet looked at are joined into the lines Claude Code
wrapped them from by the rule a copy uses (`reflow::join`): a row runs on into the next when the
next's first word would not have fitted after it, the next sits under the line's own indent, and
neither is a list item, a table or a rule. The test is against the pane's full width, which is
where Claude Code 2.1.327 wraps, at the width the rows were written for: the grid's, since every
change of width comes through here. A row that has been through is marked on its first cell
(`SETTLED`, a flag bit alacritty leaves unused) and not judged again, because the same rows
judged at another width would tell another tale. A word that fills its row is not run into an
address under it. His session's replay parsed again reads "⏺ I couldn't get a reliable forecast
for NYC today (Tuesday, November 18," across 76 columns where it stood in a column of 33.

Left: a break Claude Code meant is taken for a wrap when the row before it is nearly as wide as
the pane and the next sits under its indent (two short code lines after a long one); the text
is all there, the break is not. A paragraph across the top of the screen at a resize is two
lines in the history, since its rows were judged at different times. A copy across a wrapped row
has not been checked. And the rows Claude Code prints a second time when a pane widens are still
there twice.

## A screen of its own

Cyrus, 2026-11-18: "When a claude session begins or ends in /tui fullscreen, all the output on the
terminal is wiped clean for the Claude Code session. I want this behavior replicated. Without it,
already-ended Claude Code sessions don't reformat their output when its arkos pane is resized."
Claude Code sets `CSI ? 2031 h` as it starts (set, reset and set again within its first hundred
bytes, before it draws) and resets it as it quits, before it prints `Resume this session with:`.
At the set Arkos puts the shell's grid away, screen, history and cursor, and gives Claude Code an
empty one with a history of its own; at the reset the shell's grid comes back, reflowed to the
pane's size if it changed, and Claude Code's is dropped. The switch is made at the byte where the
sequence ends, within a read, so the replay of a session reads the same. An end heard on the
alternate screen waits until the program leaves it. Checked on Cyrus's own saved session: after
two runs of Claude Code the pane reads as it would after fullscreen, the prompt, `claude`, the
resume line and the next prompt. Another program on the main screen that sets mode 2031 would get
the same treatment; none of the shells in use do.

## A repaint of the same screen

Cyrus, 2026-11-18: "it double prints the first Claude session outputs". Claude Code also erases its
screen unasked in the middle of a turn and prints the same rows again: in his session the spinner
line under a running web search had wrapped on two rows and took one, and the whole screen was
printed again from a row higher. The rule that keeps an unasked erase's screen (for compactions
and `/clear`) kept that one too, so the screen was in the history twice. Now the screen is held at
the erase, not kept, while the frame after it is printed. If the frame's first written rows hold
the held screen's first four (allowing the frame to begin up to four rows higher) it was the same
screen: the held copy goes, and so do the rows the frame printed again above it, which were in the
history already (the history's oldest rows move up and leave blank rows at its very top). If not,
the held screen goes into the history above the frame, as before. A screen still held at the next
erase, at a resize or after 64 KiB of output is kept. Checked on his saved session (the question
once, the logo once) and on the recorded compaction (the screen before it kept).

## Known limits

- The frame on screen when a compaction or `/clear` erases it goes to the history with its
  prompt box and status lines, which read there as a divider.
- If Claude Code erases and reprints the same rows for a reason Arkos does not know of, that
  screenful is in the history twice. A dragged divider that widens a pane repeats the rows that
  stay on screen once per step of the drag, for the reason given next.

- Widening a pane (closing a split beside it) leaves up to a screenful of the conversation twice
  in the history, once as it was wrapped and once as printed again. A match on the text could
  drop the older copy; it is a guess, and a wrong guess drops a row, so it is not done.
- Rows Claude Code pads to the full width (your own messages, diff lines) wrap into a row and a
  blank band when the history is narrowed, and join again when it widens.
- At its first resize Claude Code erases whatever was above its frame on screen (the shell line
  that started it, or the end of a restored history when the resumed conversation is shorter than
  the pane). Once its conversation is taller than the pane everything above is in the history.
- A Claude Code that was killed leaves its agent record, so the shell after it has its screen rows
  cut, not wrapped, when the pane narrows, until the next session starts or ends there.
- A size mark is in the replay beside the program's output, so a program can print a forged
  one. A mark is believed only up to 1,000 rows and columns (`protocol::replay::MAX_ROWS`), so the
  worst a forged one does is misplace the replayed rows of the session that printed it; keeping
  the marks out of the stream (a list beside the ring) would close that too.
- 20,000 lines and 16 MiB are not yet measured against a day of classic output.

## For Cyrus to decide

- Whether the classic renderer is a setting (and its default), or simply how Arkos runs Claude Code.
- Whether to drop the older copy after a widening by matching text.
- The history's size, once a real day has been measured.
