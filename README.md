# Sarkos IDE

A terminal layout manager for running many Claude Code sessions side by side: a grid of
terminal panes with tabs, a docked Chrome pane, a file tree and git worktree explorer, in a
window that stays light on memory, scrolls smoothly, and keeps every session alive in a
separate daemon so a frozen UI never loses work.

Written in Rust with GPUI. The design is in
[docs/superpowers/specs/2026-10-13-sarkos-design.md](docs/superpowers/specs/2026-10-13-sarkos-design.md).

## Status

Session awareness and recovery after a restart: each tab shows what its Claude Code session is
doing (a breathing glyph while it works, a warm dot when it waits on you, a muted title when the
shell ended), the title bar counts the sessions waiting, and with "Start at login" on, a restart
brings every session back on its own: the same folders, the history on screen under a divider, and
each Claude Code conversation resumed. Milestone 4b (worktrees, git status in the tree, diff gutters) is next.

Things only a person at the screen can check, in order of value:

- Cmd+B, expand `src`, click a file: a dimmed preview tab with colours and line numbers; click
  another file: the same tab shows it; double click: a second tab; edit the file in another app:
  the viewer follows within a second
- expand `node_modules` or `target` in a big project: the window stays responsive
- a PNG and a 50 MB log from the tree: a notice, and plain text, with the window responsive
- Shift+Cmd+E, Down, Right, Down, Enter: a file opens from the keyboard
- right click a file, Copy relative path, paste into a terminal
- "Install Claude Code hooks", then `claude` in a tab: the glyph breathes while it works, a dot
  appears when it stops, the title bar's right side says "1 waiting", clicking the tab clears
  both; with Reduce Motion on, the glyph holds still
- "Start at login" with four tabs open, then `kill -9 $(pgrep -x sarkos)` to end the window
  without a goodbye (not Cmd+Q, which says goodbye so nothing reopens; a window left open would
  make a second one), then `pkill -TERM sarkosd`: the log shows "SIGTERM: writing every session's
  history"; `launchctl kickstart gui/$(id -u)/sh.csarko.sarkosd` (what login does) brings one
  window back with every tab's history under the divider and `claude --resume` typed into the
  ones that ran Claude; the tabs say "(resumed after restart)"
- a real restart with the same four tabs: the window is back after login with the same result
- Cmd+Q a window, then the restart: that project does not reopen
- `start_at_login = "off"` in the config: the plist is gone within a second; `"on"`: it is back
- with four `claude` sessions running, `cargo build --release` a change into place, wait for
  "update ready" in the title bar (or relaunch), run "Update now" from the palette: the window
  reappears with every tab, every history and every agent still running; `sarkosd`'s pid in
  `ps` is unchanged and its log shows "upgrading to" and "resumed N sessions" under a second apart
- `pkill -x sarkosd` with the window open: tabs say "reconnecting", the title bar too; after ten
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
- Cmd+K alone clearing history and screen with the prompt line left at the top, while the Cmd+K chords still work
- the empty pane's animation holding still after turning on Reduce Motion and saving the config
- after `kill -9`: the layout, every session, replayed history, the text preset, and typing
- the window minimized for two minutes keeps its sessions: compare the `sarkos` pid before and
  after, and look for no "relaunching" line in `~/Library/Application Support/sarkos/sarkosd.log`
- `ps` after thirty seconds idle with four terminals

## Session awareness

Claude Code tells the IDE what it is doing through its hooks. Run "Install Claude Code hooks"
from the command palette (or `sarkos hooks install`), which merges six entries into
`~/.claude/settings.json` after writing a `.bak`, then start or restart `claude` in a tab:

| Claude Code event | The tab |
|---|---|
| a prompt is sent, a tool runs | a slow breathing glyph: working |
| Claude stops, or asks for permission or input | a warm dot: waiting on you |
| you click the tab or type into it | the dot clears |
| the shell exits | a muted title, with the exit code |

A terminal bell marks a tab waiting too, so a `claude` set to ring the bell works without hooks.
`sarkos notify` is the hook side: it reads `SARKOS_SESSION_ID`, which every shell the daemon
starts carries, and exits 0 whatever happens, so a `claude` run outside Sarkos IDE is unaffected.
`sarkos hooks print` prints the entries for a hand paste. The entries name this binary by its
launch path; after moving it, install again. A Claude Code killed by a shutdown (its SessionEnd
hook says `other`) keeps its record, so the recovery after the restart resumes it.

## After a restart

A shutdown or restart ends every process; what becomes automatic is the recovery. The daemon
writes each session's history to `~/Library/Application Support/sarkos/sessions/<id>.replay`
a couple of seconds after output stops (in full on SIGTERM, which a clean shutdown sends) and a
manifest of the sessions (folder, shell, the Claude Code session id from its SessionStart hook)
and of the project folders with a window open. "Start at login" from the palette installs
`~/Library/LaunchAgents/sh.csarko.sarkosd.plist`, which runs `sarkosd --recover` once at login:
it recreates each shell in its folder (the home folder when that is gone) with the old output
under a dim "resumed after restart" line, types `claude --resume <id>` into the shells that had
a Claude Code session, and opens a window for each project that had one; each tab says
"(resumed after restart)" until you type into it. A window you quit yourself stays closed.

What does not come back: whatever was executing at that moment (a half-finished tool call, a
running build, shell variables, background jobs). Claude Code's transcript records the
unfinished tool, and the agent continues from there once asked; recovery only resumes, it never
sends a message. Everything stays on this Mac. `[recovery] start_at_login` in the config pins
it on or off; "ask", the default, shows a one-line notice at launch until you decide. While the
login item is installed it follows the binary and the `[daemon]` settings: a moved binary or an
edited value rewrites it. The window never unloads the login item, since after a recovery the
running daemon is the login item's own process: a rewritten file is loaded at the next login, and
turning it off deletes the file but does not stop a daemon already running.

## Sidebar and viewer

Cmd+B shows the sidebar: a file tree rooted at the folder, read one directory at a time as you
expand it, with `.git` hidden and anything `.gitignore` ignores left out (the palette's "Show
ignored files" lists them dimmed). It follows the file system within a second. A single click
opens a file in a preview tab in the focused pane, which the next single click replaces; a
double click, Enter in the tree, or dragging the tab pins it. Right click: Reveal in Finder,
Copy path, Copy relative path, Open in default app. Drag a file onto a terminal to paste its
path. Shift+Cmd+E puts the keyboard in the tree: arrows move and expand, Enter opens, Space
previews, Escape goes back to the pane.

The viewer is read-only: syntax colours from your theme's `[viewer]` section through syntect's
TextMate grammars, line numbers, smooth scrolling, Cmd+F to find, and a reload within a second
of the file changing on disk. Files over `[viewer] highlight_limit_bytes` (5 MB by default) open
plain; binaries show a notice. The text preset applies to viewers as to terminals, and Cmd+= and
Cmd+- resize the focused one. Git status colours, the worktree explorer and the diff gutters
come in milestone 4b.

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
| Cmd+K | clear to start: history and screen go, the prompt line moves to the top, as in Terminal.app |
| Cmd+= | font bigger |
| Cmd+- | font smaller |
| Cmd+F | find in the terminal or viewer |
| Cmd+B | toggle the sidebar |
| Shift+Cmd+E | focus the file tree |
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
- A link in terminal output (anything starting `http://` or `https://`, up to the next space
  or quote) is underlined under the pointer, and Cmd+click opens it in your browser, as in
  Terminal.app. A plain click does nothing to it.
- A pane closes with its last tab and its neighbour takes the space; the last pane stays and
  shows the empty screen, which is also what a folder with no saved layout opens on.
- Ctrl+Shift+G arrives with the worktree explorer in milestone 4b. Every binding can be
  changed in the config file; see Config.

## Config

`~/.config/sarkos/config.toml` (or `$XDG_CONFIG_HOME/sarkos/config.toml`) is written with
commented defaults on first run and opened by Cmd+, in your text editor. It is watched: a saved
edit applies within a second. A bad edit shows a banner with the line number and keeps the last
good settings. Sections: `font` (family, the default text preset, and the presets), `terminal`
(scrollback, shell, nice), `keybindings` (action name to keys, `""` unbinds), `theme` (`dark`
and `light`), `appearance` (`system`, `dark` or `light`), `daemon` (applies when the window starts
the daemon), `recovery` (`start_at_login`: `ask`, `on` or `off`), `sidebar` (its width and whether it starts
collapsed), `viewer` (`highlight_limit_bytes`), and `browser` for a later milestone.

Themes are one TOML file each in `~/.config/sarkos/themes/`; `navy` and `navy-light` are built
in, and "Extract built-in themes" in the palette writes them there to copy from. A theme chosen
from the palette lasts until the config reloads or the app relaunches; the config file decides
otherwise. Text presets are `compact` (14 px, 1.3), `comfortable` (16 px, 1.5) and `relaxed`
(18 px, 1.6); add your own under `[font.presets.<name>]`. Each folder remembers its preset.

## Updating

Sessions live in `sarkosd`, so updating never ends one. After `brew upgrade sarkos`, or
`cargo build --release` into the binaries you run, the title bar shows "update ready" within
five minutes (or at the next launch), and the palette's "Update now" does the rest: if the
daemon's binary changed, the daemon writes a handover file, execs the new binary in the same
process (same pid, the shells stay its children) and the window reconnects; then the window
asks the daemon to relaunch it from the new binary and quits. Every tab comes back with its
history. The daemon and window speak protocol 3; the daemon still serves protocol 1 and 2 windows,
and reads the previous handover format, so updating from the update-in-place build is an ordinary
"Update now". Upgrading from milestone 2b is the one update that needs `pkill -x sarkosd` (the old
daemon knows nothing of handovers), which ends every session.

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
