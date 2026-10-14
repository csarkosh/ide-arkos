# Sarkos IDE

A terminal layout manager for running many Claude Code sessions side by side: a grid of
terminal panes with tabs, a docked Chrome pane, a file tree and git worktree explorer, in a
window that stays light on memory, scrolls smoothly, and keeps every session alive in a
separate daemon so a frozen UI never loses work.

Written in Rust with GPUI. The design is in
[docs/superpowers/specs/2026-10-13-sarkos-design.md](docs/superpowers/specs/2026-10-13-sarkos-design.md).

## Status

Updating in place: the daemon replaces itself by exec with every shell still its child, the
window reconnects by itself, and the palette's "Update now" runs the whole update after
`brew upgrade` or a build into place. Milestone 3 (Claude Code session awareness, with recovery
after a reboot) is next.

Things only a person at the screen can check, in order of value:

- with four `claude` sessions running, `cargo build --release` a change into place, wait for
  "update ready" in the status bar (or relaunch), run "Update now" from the palette: the window
  reappears with every tab, every history and every agent still running; `sarkosd`'s pid in
  `ps` is unchanged and its log shows "upgrading to" and "resumed N sessions" under a second apart
- `pkill -x sarkosd` with the window open: tabs say "reconnecting", the status bar too; after ten
  seconds a banner appears, a new daemon starts, and each tab comes back as a new shell marked
  "(restarted)"
- "Update now" when nothing changed relaunches the window and every tab comes back attached
- editing `~/.config/sarkos/config.toml` while the app runs: `preset = "relaxed"` resizes every
  terminal within a second; a typo shows a banner with the line number and nothing else changes;
  fixing it clears the banner
- System Settings > Appearance: Light switches to Navy Light and back, history included
- `mode = "dark"` pins Navy whatever the system says
- Shift+Cmd+P: typing filters, Up/Down and Enter run a command, Escape and a click outside close it
- "Text preset: compact" in the palette, then Cmd+= on one pane: only that pane grows; choosing
  a preset again resets it; the preset survives a relaunch of that folder
- "Theme: navy-light" in the palette, then a config reload: the config's theme wins
- "Extract built-in themes", then editing `~/.config/sarkos/themes/navy.toml`'s `window_bg` and
  reloading: the window follows the file; renaming it to `mine.toml` and setting `dark = "mine"`
- `"split right" = "cmd-d"` under `[keybindings]`: Cmd+D splits, Cmd+\ does not; `""` unbinds
- Cmd+F in a pane with history: typing highlights every match and jumps to the newest; Enter
  and Shift+Enter step with the count; Escape or a click in the terminal closes it; `.` and `*`
  match literally; output arriving keeps the bar open
- a pane with eight tabs: the bar scrolls, Option+Cmd+Right keeps the active tab in view, and
  the ⋯ menu lists them all
- closing a tab whose shell has ended, then Shift+Cmd+T: a new shell opens
- Cmd+, opens the config file in the default text editor
- typing in each pane, and that Cmd combinations never reach the shell
- dragging a tab to each of the five drop zones, within a bar, and out of a pane
- Cmd+K alone clearing the scrollback while the Cmd+K chords still work
- the empty pane's animation holding still after turning on Reduce Motion and saving the config
- after `kill -9`: the layout, every session, replayed history, the text preset, and typing
- the window minimized for two minutes keeps its sessions: compare the `sarkos` pid before and
  after, and look for no "relaunching" line in `~/Library/Application Support/sarkos/sarkosd.log`
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
| Shift+Cmd+] | next tab |
| Shift+Cmd+[ | previous tab |
| Ctrl+Tab | most recently used tab |
| Cmd+W | close tab |
| Shift+Cmd+T | reopen closed tab |
| Cmd+K then W | close all in pane |
| Cmd+K then Shift+Cmd+Left | move tab left |
| Cmd+K then Shift+Cmd+Right | move tab right |
| Ctrl+Shift+` | new terminal tab |
| Cmd+K | clear scrollback |
| Cmd+= | font bigger |
| Cmd+- | font smaller |
| Cmd+F | find in the terminal |
| Cmd+, | open config file |
| Shift+Cmd+P | command palette |

Notes:

- A lone Cmd+K clears the scrollback after gpui's one-second pending-chord delay; the Cmd+K
  chords fire at once. Cmd+K alone does nothing in a busy or empty pane.
- Pane numbers are structural reading order: rows top to bottom, left to right within a row,
  computed as if every split were at half, so dragging a handle never renumbers a pane.
- Cmd+W (and the tab's ×, and Cmd+K then W) keeps a closed terminal's shell alive for a
  minute: Shift+Cmd+T within that minute brings the same shell back, history and all, in the
  pane it was closed from. After the minute the shell ends, and Shift+Cmd+T opens a new one.
  Quitting ends every closed tab's shell at once. A tab whose shell has already ended is not
  kept for Shift+Cmd+T, since nothing is left to end or bring back.
- Option+Left and Option+Right send the word-jump sequences ESC b and ESC f. Every other
  modified arrow, Home and End sends xterm's `ESC [ 1 ; m X`.
- Cmd+B, Shift+Cmd+E and Ctrl+Shift+G arrive with the sidebar in milestone 4. Every binding can
  be changed in the config file; see Config.

## Config

`~/.config/sarkos/config.toml` (or `$XDG_CONFIG_HOME/sarkos/config.toml`) is written with
commented defaults on first run and opened by Cmd+, in your text editor. It is watched: a saved
edit applies within a second. A bad edit shows a banner with the line number and keeps the last
good settings. Sections: `font` (family, the default text preset, and the presets), `terminal`
(scrollback, shell, nice), `keybindings` (action name to keys, `""` unbinds), `theme` (`dark`
and `light`), `appearance` (`system`, `dark` or `light`), `daemon` (applies when the window starts
the daemon), and `sidebar` and `browser` for later milestones.

Themes are one TOML file each in `~/.config/sarkos/themes/`; `navy` and `navy-light` are built
in, and "Extract built-in themes" in the palette writes them there to copy from. A theme chosen
from the palette lasts until the config reloads or the app relaunches; the config file decides
otherwise. Text presets are `compact` (14 px, 1.3), `comfortable` (16 px, 1.5) and `relaxed`
(18 px, 1.6); add your own under `[font.presets.<name>]`. Each folder remembers its preset.

## Updating

Sessions live in `sarkosd`, so updating never ends one. After `brew upgrade sarkos`, or
`cargo build --release` into the binaries you run, the status bar shows "update ready" within
five minutes (or at the next launch), and the palette's "Update now" does the rest: if the
daemon's binary changed, the daemon writes a handover file, execs the new binary in the same
process (same pid, the shells stay its children) and the window reconnects; then the window
asks the daemon to relaunch it from the new binary and quits. Every tab comes back with its
history. The daemon and window speak protocol 2; the daemon still serves a protocol 1 window
from milestone 2b. Upgrading *to* this version from milestone 2b is the one update that needs
`pkill -x sarkosd` (the old daemon knows nothing of handovers), which ends every session.

If the new daemon fails its check or the exec fails, the old daemon keeps running and the window
shows the error. If the window loses the daemon for ten seconds, it starts a new one and every
tab becomes a fresh shell with the "(restarted)" note.

A window newer than its daemon (one started from a new build while an older daemon still runs)
is refused and stops reconnecting, so "Update now" cannot reach that daemon. The fix is the
one-time path above: `pkill -x sarkosd`, which ends every session, and open the window again.

## Build and run

```bash
cargo build --release
./target/release/sarkos        # starts sarkosd on demand, opens one terminal in the current folder
```

The daemon keeps running after the window closes; sessions survive the UI being killed. Logs:
`~/Library/Application Support/sarkos/sarkosd.log`. A window newer than the daemon is refused
with a banner and stops reconnecting; `pkill -x sarkosd` and a relaunch fix it (see Updating). A
window older than the daemon keeps working. `pkill -x sarkosd` stops the daemon and ends every
session.

Upgrading from milestone 1: the window finds milestone 1's saved session and moves it to the
new workspace format on first launch. A daemon started by milestone 1 keeps running without
this milestone's lifecycle fixes, though; `pkill -x sarkosd` restarts it (the window starts a
new one) and ends every session, so do it when nothing important is running.

## Tests

```bash
cargo test
```

## If you get headaches

The defaults (16 px Menlo, line height 1.5, a bright grey on navy, no warm tint) follow
[Check your eyes before your theme](https://csarko.sh/research/check-your-eyes-before-your-theme).
The same research puts a comprehensive eye examination that tests convergence and small
astigmatism ahead of any setting. Not medical advice.

## Further reading

[Update the IDE, keep the shells](https://csarko.sh/research/update-the-ide-keep-the-shells):
the write-up of why updates kill sessions elsewhere and how this IDE avoids it.

[Bring the agents back after a reboot](https://csarko.sh/research/bring-the-agents-back-after-a-reboot):
the research behind milestone 3's recovery of Claude Code sessions after a reboot.

## License

GPL-3.0. Modified versions must be released under the same license. See [LICENSE](LICENSE).
