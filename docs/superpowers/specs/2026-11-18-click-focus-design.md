# A click keeps the keys where you type

Approved by Cyrus, 2026-11-18.

## Why

Cyrus, 2026-11-18: "I like the current behavior when I use keyboard navigation. However, it makes
the mouse click experience awkward. When I click on files or open the Sessions pane in Pane 0, I
expect to still be able to type in my Claude session. When I click on a notification, setting,
doctor, etc, I don't expect my focus to change." And: "when I click on a session row in Pane 0,
the focus highlight briefly appears in Pane 0 before jumping to Pane 1. This jump is jarring. The
focus should remain on Pane 1 the entire time so it feels smooth."

## The rule

A mouse click never takes the keys, or the focus glow, from the pane being typed in. The keyboard
works as the Chrome keys design (2026-11-16) says, unchanged: Cmd+0, the rings, the title bar's
icons reached by keys.

- A press in Pane 0 no longer hands it the keys. That press was also the glow's detour: it lit
  Pane 0 before a session row's click moved the keys to Pane 1. The Sessions/Files switcher, the
  folders, the Explorer and Worktrees headings, a session group's heading and a right press's menu
  all leave the keys where they were.
- A session row's click shows its session and hands it the keys, as before (Cyrus, 2026-11-17).
  The glow stays on the pane the whole time, also when the page changes under it.
- A file's click opens it as the setting "A session's files open" says. Neighbor: when the
  focused pane shows a terminal, the file goes beside it (the pane to its right, else its left,
  else a new pane split off to its right: `direct::neighbor_step`), and the keys stay in the
  terminal; a focused pane showing anything else takes the file itself. In place: the focused
  pane takes it, as today. Single click previews, double click pins, as before, in whichever pane
  the file lands.
- The bell and the doctor opened by a click open their menus without the keys; the mouse works
  them. Opened from the keyboard's ring, they take the keys as now. Settings is its own window.
- What has to be typed in still takes the keys: the New File prompt and a tab's rename.

## Approaches

1. Stop the clicks moving the keys (chosen): drop the column's press handler and the click paths'
   flag changes, and route a clicked file through the open placement. Small, keeps every keyboard
   path as it is.
2. Keep the keys moving but hide the ring and the glow after a click (the web's focus-visible).
   Rejected: typing would still go to the sidebar, which is what Cyrus objects to.

## Left to the builder

Which pane a Neighbor click picks when a preview tab of the file is already open elsewhere
(reuse that tab, as `plan_open_in` with `reuse` does). Tests first, one per bullet above.
