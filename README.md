# Sarkos IDE

A terminal layout manager for running many Claude Code sessions side by side: a grid of
terminal panes with tabs, a docked Chrome pane, a file tree and git worktree explorer, in a
window that stays light on memory, scrolls smoothly, and keeps every session alive in a
separate daemon so a frozen UI never loses work.

Written in Rust with GPUI. The design is in
[docs/superpowers/specs/2026-10-13-sarkos-design.md](docs/superpowers/specs/2026-10-13-sarkos-design.md).

## Status

Milestone 2a of 6: panes, tabs, drag and drop, the VS Code keyboard map, and a layout that
survives relaunch. Milestone 2b (config file, themes, text presets, command palette, find)
is next. The daemon now notices a shell exit even when a background job keeps the terminal
open, never signals a reaped process, and forgets dead sessions ten minutes after they end
once no window shows them.

Things only a person at the screen can check, in order of value:

- typing in each pane, and that Cmd combinations never reach the shell
- Cmd+1 to Cmd+9 numbering after splits and closes, left to right then top to bottom
- dragging a tab to each of the five drop zones, within a bar, and out of a pane
- dropping a tab onto its own position changing nothing
- dragging a split handle, the 160 by 80 px minimum, and double-click resetting it
- dragging a handle and releasing outside the window ending the drag
- Cmd+K alone clearing the scrollback while the Cmd+K chords still work
- the empty pane's animation settling, replaying on hover and after a pane re-empties, and holding still with Reduce Motion
- scroll direction and glide, and the jump-to-bottom pill
- Cmd+V bracketed paste into `claude`
- after `kill -9`: the layout, every session, replayed history, and typing
- a session that is gone from the daemon showing "(restarted)" on relaunch
- a session ended in a background tab marks its tab without stealing focus
- the window minimized for two minutes keeps its sessions: compare the `sarkos` pid before and
  after, and look for no "killing" line in `~/Library/Application Support/sarkos/sarkosd.log`
- `ps` after thirty seconds idle with four terminals

## Keyboard

The VS Code map, in a terminal-friendly form.

| Keys | Action |
| --- | --- |
| Cmd+1 | focus pane 1 |
| Cmd+2 | focus pane 2 |
| Cmd+3 | focus pane 3 |
| Cmd+4 | focus pane 4 |
| Cmd+5 | focus pane 5 |
| Cmd+6 | focus pane 6 |
| Cmd+7 | focus pane 7 |
| Cmd+8 | focus pane 8 |
| Cmd+9 | focus pane 9 |
| Cmd+K then Cmd+Left | focus left |
| Cmd+K then Cmd+Right | focus right |
| Cmd+K then Cmd+Up | focus up |
| Cmd+K then Cmd+Down | focus down |
| Cmd+\ | split right |
| Cmd+K then Cmd+\ | split down |
| Cmd+K then Cmd+M | maximize pane |
| Ctrl+Cmd+Left | move tab to previous pane |
| Ctrl+Cmd+Right | move tab to next pane |
| Option+Cmd+Right | next tab |
| Option+Cmd+Left | previous tab |
| Cmd+Shift+] | next tab |
| Cmd+Shift+[ | previous tab |
| Ctrl+Tab | most recently used tab |
| Cmd+W | close tab |
| Cmd+Shift+T | reopen closed tab |
| Cmd+K then W | close all in pane |
| Cmd+K then Cmd+Shift+Left | move tab left |
| Cmd+K then Cmd+Shift+Right | move tab right |
| Ctrl+Shift+` | new terminal tab |
| Cmd+K | clear scrollback |
| Cmd+= | font bigger |
| Cmd+- | font smaller |

Notes:

- A lone Cmd+K clears the scrollback after gpui's one-second pending-chord delay; the Cmd+K
  chords fire at once. Cmd+K alone does nothing in a busy or empty pane.
- Pane numbers are structural reading order: rows top to bottom, left to right within a row,
  computed as if every split were at half, so dragging a handle never renumbers a pane.
- Option+Left and Option+Right send the word-jump sequences ESC b and ESC f. Every other
  modified arrow, Home and End sends xterm's `ESC [ 1 ; m X`.
- Cmd+F, Cmd+,, Cmd+Shift+P, Cmd+B, Cmd+Shift+E and Ctrl+Shift+G are not bound until
  milestones 2b and 4.

## Build and run

```bash
cargo build --release
./target/release/sarkos        # starts sarkosd on demand, opens one terminal in the current folder
```

The daemon keeps running after the window closes; sessions survive the UI being killed. Logs:
`~/Library/Application Support/sarkos/sarkosd.log`. A protocol change between the window and the
daemon requires restarting the daemon; the window reports the mismatch in its status banner.
`pkill -x sarkosd` stops the daemon and ends every session.

## Tests

```bash
cargo test
```

## If you get headaches

The defaults (16 px Menlo, line height 1.5, a bright grey on navy, no warm tint) follow
[Check your eyes before your theme](https://csarko.sh/research/check-your-eyes-before-your-theme).
The same research puts a comprehensive eye examination that tests convergence and small
astigmatism ahead of any setting. Not medical advice.

## License

GPL-3.0. Modified versions must be released under the same license. See [LICENSE](LICENSE).
