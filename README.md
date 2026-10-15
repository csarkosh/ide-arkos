# Arkos IDE

A terminal layout manager for running many Claude Code sessions side by side: a grid of
terminal panes with tabs, a docked Chrome pane, a file tree and git worktree explorer, in a
window that stays light on memory, scrolls smoothly, and keeps every session alive in a
separate daemon so a frozen UI never loses work.

Written in Rust with GPUI. The design is in
[docs/superpowers/specs/2026-10-13-arkos-design.md](docs/superpowers/specs/2026-10-13-arkos-design.md).

## Status

Session awareness and recovery after a restart: each tab shows what its Claude Code session is
doing (a breathing glyph while it works, a warm dot when it waits on you, a muted title when the
shell ended), the title bar counts the sessions waiting, and with "Start at login" on, a restart
brings every session back on its own: the same folders, the history on screen under a divider, and
each Claude Code conversation resumed. The sidebar shows git status in the tree and the
worktrees, and the viewer marks a file's changes against HEAD. The browser pane docks a Chrome
window into a pane (Accessibility permission needed). Milestone 6a made the update path and the
launcher trustworthy: "Check for updates" waits for the check and says what it found, the chooser
opens before the daemon answers and reconnects, and a closed tab's minute is kept by the daemon.
Milestones 6b and 6c are next.

Things only a person at the screen can check, in order of value:

- `claude` in a tab with `/tui fullscreen`, a long answer: the wheel and the trackpad scroll the
  conversation; a question with options: clicking one picks it; Option+Delete at the prompt
  deletes the word before the cursor
- drag across terminal text: it tints; Cmd+C, then Cmd+V at a prompt pastes it; double click
  selects a word, triple click a line, a click clears it; a drag inside Claude Code's fullscreen
  selects without the program reacting; `less` on a long file: the wheel moves the file;
  Shift+wheel on a mouse, in a shell with history: wheel up shows older lines, not newer
- in Claude Code: Option+Enter and Shift+Enter each add a line to the prompt instead of sending
  it; Option+P opens the model picker; Esc still cancels; in zsh, Shift+Enter runs the line
- `claude` with `/tui default`, a long answer, then a two-finger scroll: the text follows the
  finger with no lag and keeps gliding when the fingers lift; a mouse wheel's notches still ease
- in a repo with a change: the file's row is amber and its folders carry a dot; `git stash`
  clears them within ten seconds
- Worktrees: expand one: a shimmer, then its changed files fade in; collapse and expand again:
  `git status` runs again (watch `ps`); + creates `../proj-feat` on a new branch; Remove on a
  dirty one is refused with git's words, force removes it
- Ctrl+Shift+G, Down, Enter: a worktree expands from the keyboard
- edit a tracked file in another app while the viewer shows it: the gutter bars appear within a
  second; Shift+Cmd+D shows the deleted lines; `git commit` clears the bars within ten seconds; a
  file in a worktree diffs against that worktree's HEAD
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
- "Start at login" with four tabs open, then `kill -9 $(pgrep -x arkos)` to end the window
  without a goodbye (not Cmd+Q, which says goodbye so nothing reopens; a window left open would
  make a second one), then `pkill -TERM arkosd`: the log shows "SIGTERM: writing every session's
  history"; `launchctl kickstart gui/$(id -u)/sh.csarko.arkosd` (what login does) brings one
  window back with every tab's history under the divider and `claude --resume` typed into the
  ones that ran Claude; the tabs say "(resumed after restart)"
- a real restart with the same four tabs: the window is back after login with the same result
- Cmd+Q a window, then the restart: that project does not reopen
- `start_at_login = "off"` in the config: the plist is gone within a second; `"on"`: it is back
- with four `claude` sessions running, `cargo build --release` a change into place, wait for
  "update ready" in the title bar (or relaunch), run "Update now" from the palette: the window
  reappears with every tab, every history and every agent still running; `arkosd`'s pid in
  `ps` is unchanged and its log shows "upgrading to" and "resumed N sessions" under a second apart
- `pkill -x arkosd` with the window open: tabs say "reconnecting", the title bar too; after ten
  seconds a banner appears, a new daemon starts, and each tab comes back as a new shell marked
  "(restarted)"
- "Update now" when nothing changed relaunches the window and every tab comes back attached
- push, `gh workflow run release -f version=X`, wait for the run, see "update ready: X" in the
  title bar within five minutes, stop typing for thirty seconds: the window relaunches with every
  shell and tab where it was; `arkos --version` in a tab prints X
- `[update] auto = false`: the badge stays and "Update now" applies it
- a relaunched window opens without taking focus from the app in use (verify by eye)
- editing `~/.config/arkos/config.toml` while the app runs: `preset = "relaxed"` resizes every
  terminal within a second; a typo shows a banner with the line number and nothing else changes;
  fixing it clears the banner
- System Settings > Appearance: Light switches to Navy Light and back, history included
- `mode = "dark"` pins Navy whatever the system says
- Shift+Cmd+P: typing filters, Up/Down and Enter run a command, Escape and a click outside close it
- "Text preset: compact" in the palette, then Cmd+= on one pane: only that pane grows; choosing
  a preset again resets it; the preset survives a relaunch of that folder
- "Theme: navy-light" in the palette, then a config reload: the config's theme wins
- "Extract built-in themes", then editing `~/.config/arkos/themes/navy.toml`'s `window_bg` and
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
- drag a tab across a pane's zones and on into another pane: the tint glides and fades,
  nothing pops; drop it: the tint fades where it was
- open and close the ⋯ menu, a right-click menu, the palette and a find bar: each fades in and
  out; a click while one fades out lands on what is under it
- Cmd+B: the sidebar slides out and back in, the panes following
- System Settings > Accessibility > Display > Reduce Motion on, then save the config so it is
  read again: everything above snaps
- Cmd+K alone clearing history and screen with the prompt line left at the top, while the Cmd+K chords still work
- the empty pane's animation holding still after turning on Reduce Motion and saving the config
- after `kill -9`: the layout, every session, replayed history, the text preset, and typing
- the window minimized for two minutes keeps its sessions: compare the `arkos` pid before and
  after, and look for no "relaunching" line in `~/Library/Application Support/arkos/arkosd.log`
- `ps` after thirty seconds idle with four terminals
- run `/Applications/Arkos IDE.app/Contents/MacOS/arkos browser-probe` once and grant access when
  asked: it moves a Chrome window for a few seconds and prints what it saw
- open a browser tab while Claude is working: the next Chrome window Claude opens lands in it
- drag a Chrome window into a pane; drag its title bar out again
- switch tabs away and back: the window hides and returns
- revoke Accessibility in System Settings: the pane shows the permission screen within a second
- open from the Dock: the chooser shows the five most recent folders; Show all; Open another
  folder; Escape quits; Cmd+O from a window opens a second window
- with two panes side by side, the separator between them is a hairline and the focused pane's
  glow looks the same on its separator side as on its others; hover the separator: a wider
  stripe fades in and the panes do not move; drag it; move away: it fades out
- the focused pane's glow reaches all four edges, the bottom and right included, and breathes
  slowly; click another pane: the glow cross-fades; Reduce Motion in System Settings: it rests;
  `ps` with four idle terminals and the window in front: the breath's ten repaints a second
  cost a few percent of one core, and none once another app is in front
- `pkill -x arkosd`, then open from the Dock: the chooser is up at once with "connecting to the
  daemon…" under the list, and the session counts arrive a moment later; `pkill -x arkosd` again
  with the chooser open: the footer says "reconnecting to the daemon…" and the counts come back
  with the next daemon
- `pkill -x arkosd`, move `/Applications/Arkos IDE.app/Contents/MacOS/arkosd` aside, open from
  the Dock: "Arkos cannot reach its daemon" with the error; "Open the log" opens `arkosd.log`;
  move the binary back and press "Try again": the chooser fills in and choosing a folder opens it
- the same broken daemon path, but choose a folder in the chooser before the failure window
  appears: the choice is kept behind it, and "Try again" with the binary back opens that folder
- with the Wi-Fi off, "Check for updates" from the palette: after a few seconds the notice says
  "updates unreachable: " with the daemon's words, not "no update"
- close a terminal tab, `kill -9 $(pgrep -x arkos)` at once, wait a little over a minute: `ps`
  shows the tab's shell gone, and the update journal says "session N in F: not reopened within
  60 s, ended"; a tab closed and reopened with Shift+Cmd+T within the minute keeps its shell
- open from the Dock with Claude running in two tabs of a project: its row says "2 sessions";
  Stop all, then Confirm: the row says "stopping…" and the count goes; `ps` shows the shells
  gone; a folder with no sessions shows nothing on the right; Delete on a selected row arms it
  and Escape disarms it without closing the chooser

## Session awareness

Claude Code tells the IDE what it is doing through its hooks. Run "Install Claude Code hooks"
from the command palette (or `arkos hooks install`), which merges six entries into
`~/.claude/settings.json` after writing a `.bak`, then start or restart `claude` in a tab:

| Claude Code event | The tab |
|---|---|
| a prompt is sent, a tool runs | a slow breathing glyph: working |
| Claude stops, or asks for permission or input | a warm dot: waiting on you |
| you click the tab or type into it | the dot clears |
| the shell exits | a muted title, with the exit code |

A terminal bell marks a tab waiting too, so a `claude` set to ring the bell works without hooks.
`arkos notify` is the hook side: it reads `ARKOS_SESSION_ID`, which every shell the daemon
starts carries, and exits 0 whatever happens, so a `claude` run outside Arkos IDE is unaffected.
`arkos hooks print` prints the entries for a hand paste. The entries name this binary by its
launch path; after moving it, install again. A Claude Code killed by a shutdown (its SessionEnd
hook says `other`) keeps its record, so the recovery after the restart resumes it.

## After a restart

A shutdown or restart ends every process; what becomes automatic is the recovery. The daemon
writes each session's history to `~/Library/Application Support/arkos/sessions/<id>.replay`
a couple of seconds after output stops (in full on SIGTERM, which a clean shutdown sends) and a
manifest of the sessions (folder, shell, the Claude Code session id from its SessionStart hook)
and of the project folders with a window open. "Start at login" from the palette installs
`~/Library/LaunchAgents/sh.csarko.arkosd.plist`, which runs `arkosd --recover` once at login:
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

Under the Explorer, Worktrees lists `git worktree list`: each worktree's folder, branch and, once
expanded, its changed files in the status colours with a count in the row (a collapsed worktree
never runs `git status`). Click a changed file to read it in the viewer. Right click a worktree:
Open terminal here, Remove worktree (refused while it has changes), Remove worktree, force. The
"+" on the header creates one: a branch name, then the path (prefilled beside the project);
an existing branch is checked out, a new one created. If the repository's own config sets
filter drivers (`git lfs install --local`, for one), creating a worktree names them and asks for
a second Enter, since the checkout runs them; a filter config with a driver name it cannot read
safely creates nothing. The main checkout is first and cannot be removed. Ctrl+Shift+G puts the
keyboard in the list. The tree's rows carry git status in the theme's pastels (added, modified,
untracked) and a folder with changes inside carries a dot.

The viewer is read-only: syntax colours from your theme's `[viewer]` section through syntect's
TextMate grammars, line numbers, smooth scrolling, Cmd+F to find, and a reload within a second
of the file changing on disk. Files over `[viewer] highlight_limit_bytes` (5 MB by default) open
plain, and above 64 MB a notice; binaries show a notice. The text preset applies to viewers as to
terminals, and Cmd+= and Cmd+- resize the focused one. A file in a git repository shows its
changes against the worktree's HEAD in the gutter: an added line a green bar, a changed line an
amber one, a deletion a red mark at the top of the line that follows; Shift+Cmd+D (or the "inline
diff" chip) shows the deleted lines in red between the current ones.

Menus, the palette, notices, find bars and the drop preview ease in over 120 ms and out over
90 ms; Reduce Motion turns that off. The drop preview glides between zones, the focus glow
cross-fades as focus moves and breathes slowly while the window is active (six seconds a
breath, a little brighter and wider at the peak; Reduce Motion rests it), a section's rows fade
in as it opens, and Cmd+B slides the sidebar
over 150 ms with the panes following. Something on its way out is only drawn: it takes no
keys, focus or clicks.

## Browser pane

A browser tab is a placeholder in a pane with a real Chrome window sitting on top of it. Arkos
does not render the web: it moves and sizes the Chrome window through the Accessibility API so
that it follows the placeholder as you split, drag a handle, toggle the sidebar or move the
window, hides it when the tab is not the active one (or arkos is minimized or hidden), and
raises it when you click the pane.

A window gets docked in one of four ways. While a Claude Code session is working, the next Chrome
window it opens lands in a browser tab on its own. The palette's "Dock a Chrome window…" lists
Chrome's windows and docks the one you pick into the focused pane. "New browser tab…" (Cmd+Shift+B) asks for a URL
and opens it in a new Chrome window that docks into the tab; "New single-page browser tab…"
does the same in Chrome's app mode (no tab strip, no address bar), as does `app_mode` in the
config. Or drag any Chrome window by its title bar into a pane and drop it.

It is released four ways: close the tab (the window stays where it is; arkos never closes it),
the palette's "Undock Chrome window", drag the tab out of the arkos window and drop it there, or
drag the window's title bar more than 40 px away from the placeholder. Dropping the tab in
another pane moves it, still docked. Chrome's own tab dragging is left alone.

Moving another app's window needs the Accessibility permission. Arkos asks once, when the first
browser pane needs it; until it is granted the pane explains what is missing and links to System
Settings. Revoking it later brings the explanation back within a second.

A pane holding a browser tab shows a slim 14 px tab bar: the pane number and one dot per tab, no
titles, since Chrome shows the page's own title. Dragging a tab still drops onto it. To get the
vertical space back, a tip (once per run) suggests right-clicking Chrome's tab strip and choosing
Show Tabs Vertically.

Only the URL and the app-mode flag are saved. After a relaunch a browser tab waits for a window;
arkos never tries to find the old one. A hidden tab's window waits off-screen, and quitting puts
it back; after a crash or a kill it stays there, and Chrome's Window > Zoom (or quitting and
reopening Chrome) brings it back on screen.

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
| Cmd+Shift+B | new browser tab: asks for a URL, opens it in a Chrome window docked into the tab |
| Cmd+K | clear to start: history and screen go, the prompt line moves to the top, as in Terminal.app |
| Cmd+= | font bigger |
| Cmd+- | font smaller |
| Cmd+F | find in the terminal or viewer |
| Cmd+C | copy the selected terminal text |
| Cmd+V | paste |
| Cmd+B | toggle the sidebar |
| Shift+Cmd+E | focus the file tree |
| Ctrl+Shift+G | focus the worktree list |
| Shift+Cmd+D | inline diff in the viewer |
| Cmd+O | open folder…: the native picker; the folder opens in a new window |
| palette | open recent…: the project chooser; the folder opens in a new window |
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
  The daemon keeps that minute, so a window that crashes or is killed within it still has the
  shell ended on time (the update journal says "not reopened within 60 s, ended"), and an update
  or a restart in the minute keeps the deadline; any window that attaches the session first
  cancels it. Quitting ends every closed tab's shell at once. A tab whose shell has already
  ended is not kept for Shift+Cmd+T, since nothing is left to end or bring back. A closed
  viewer tab comes back too, pinned, in the pane it was closed from.
- Option is Meta, as with Terminal.app's "Use Option as Meta Key": Option+a key sends ESC plus
  the key, so Claude Code's Option+Enter (newline), Option+P, Option+T, Option+O and Alt+B/F/D/Y
  all work. Option+Left and Option+Right send the word-jump sequences ESC b and ESC f; every
  other modified arrow, Home and End sends xterm's `ESC [ 1 ; m X`; Option+Delete sends ESC DEL,
  the word delete, and a modified forward delete sends `ESC [ 3 ; m ~`. Set
  `option_as_meta = false` under `[terminal]` to have Option compose characters (ß, π) instead.
- A program that turns on the kitty keyboard protocol (Claude Code does) gets its
  "disambiguate" encoding: Esc as `ESC [ 27 u`, and a modified Enter, Tab, Backspace or Space, a
  key with Option, or Ctrl+Shift with a key as `ESC [ code ; mods u`, so Shift+Enter is a newline
  there. Ctrl+letter keeps its control byte even then, so Ctrl+I, Ctrl+M and Ctrl+[ stay Tab,
  Enter and Esc. Everywhere else those keys keep their usual bytes: Shift+Enter in a shell is
  Enter. A program killed on the alternate screen can leave the protocol on; `reset` clears it.
- Drag across terminal text to select it (double click a word, triple click a line); Cmd+C
  copies it and a click clears it. A program that asks for mouse reports, such as Claude Code's
  fullscreen renderer, gets the wheel and single clicks, so its conversation scrolls and its
  options can be clicked; a drag still selects locally and the program never sees it, though the
  first click of a double click has already reached it. On the alternate screen without mouse
  reports (`less`, for one) the wheel sends arrow keys (xterm's alternate scroll, `?1007`, on by
  default), and Shift+wheel always scrolls Arkos's own history.
- A link in terminal output (anything starting `http://` or `https://`, up to the next space
  or quote) is underlined under the pointer, and Cmd+click opens it in your browser, as in
  Terminal.app. A plain click does nothing to it.
- A pane closes with its last tab and its neighbour takes the space; the last pane stays and
  shows the empty screen, which is also what a folder with no saved layout opens on.
- Every binding can be changed in the config file; see Config. A shifted symbol can be written
  as Shift plus the plain key (`ctrl-shift-\``) or as the character itself (`ctrl-~`): macOS
  reports the character, and both spellings are the same binding.

## Config

`~/.config/arkos/config.toml` (or `$XDG_CONFIG_HOME/arkos/config.toml`) is written with
commented defaults on first run and opened by Cmd+, in your text editor. It is watched: a saved
edit applies within a second. A bad edit shows a banner with the line number and keeps the last
good settings. Sections: `font` (family, the default text preset, and the presets), `terminal`
(scrollback, shell, nice, option_as_meta), `keybindings` (action name to keys, `""` unbinds), `theme` (`dark`
and `light`), `appearance` (`system`, `dark` or `light`), `daemon` (applies when the window starts
the daemon), `recovery` (`start_at_login`: `ask`, `on` or `off`), `update` (below), `sidebar` (its width and
whether it starts collapsed), `viewer` (`highlight_limit_bytes`), and `browser` (`url`, the page a new
browser tab opens when its prompt is left empty, and `app_mode`, which opens user URLs as a single
page).

```toml
[update]
# The daemon checks the release manifest every check_every_secs and downloads a newer, signed
# build in the background. With auto = true the window applies it once you have been idle for
# idle_after_secs (no typing, nothing open): the daemon swaps in place and the window relaunches
# with every shell still there. auto = false keeps the "update ready" badge and the palette's
# "Update now" instead.
auto = true
check_every_secs = 300
idle_after_secs = 30
```

`check_every_secs` is the daemon's: it reaches a running daemon only when the window starts it or
the login item is reinstalled ("Start at login" in the palette); a daemon already running keeps
the interval it started with. `auto` and `idle_after_secs` apply at once.

Themes are one TOML file each in `~/.config/arkos/themes/`; `navy` and `navy-light` are built
in, and "Extract built-in themes" in the palette writes them there to copy from. A theme chosen
from the palette lasts until the config reloads or the app relaunches; the config file decides
otherwise. Text presets are `compact` (14 px, 1.3), `comfortable` (16 px, 1.5) and `relaxed`
(18 px, 1.6); add your own under `[font.presets.<name>]`. Each folder remembers its preset.

## Updating

The IDE is installed as `/Applications/Arkos IDE.app` through the Homebrew cask (its text is in
`homebrew/arkos-ide.rb`, published in `csarkosh/homebrew-tap`), which holds both `arkos` and `arkosd`:

```
brew install --cask csarkosh/tap/arkos-ide
xattr -dr com.apple.quarantine "/Applications/Arkos IDE.app"
```

The second line is needed until the app is notarized: the bundle is ad-hoc signed, and Gatekeeper
refuses a quarantined ad-hoc app. The daemon's own updates never carry the attribute. The cask says `auto_updates`, so
`brew upgrade` leaves the app alone and the IDE updates itself.

Updates arrive by themselves: the daemon checks the release manifest every five minutes, downloads
and verifies a newer build, and the window applies it once you have been idle for thirty seconds
(no typing, nothing open): `arkosd` swaps in place and the window relaunches with every shell
still there. `[update] auto = false` keeps the badge and "Update now" instead.

"Check for updates" in the palette asks now and waits for that check (up to 30 seconds) before
saying one thing: "update ready: X" when it found a release (the badge appears too), "update
check failed: …" when the release itself was bad, "updates unreachable: …" with the daemon's
words when GitHub could not be reached, "the update check is still running" when 30 seconds were
not enough, and "no update" only when the check reached the manifest and found nothing newer.
"Retry update" says its outcome the same way. A daemon from before protocol 5 is asked once after
three seconds, and says "the daemon has not reached the update server yet" when it never has.

A release downloaded and staged before a daemon restart is kept: the restarted daemon adopts the
staged bundle (the journal says "adopted staged X from before the restart") instead of
downloading it again, and the badge stays.

A check that cannot reach GitHub (offline, a captive portal) is retried quietly at the next one;
when none has reached the signed manifest for over a day, the title bar says "updates
unreachable for N days" until one does.
When the daemon's last check or update failed for a reason that is the release's (a bad
signature, a download that does not match its manifest) or the swap's, the title bar shows the
reason once as a notice, the badge says "update failed", and the palette offers "Retry update".
If the new window does not come up within 20 seconds of its relaunch, or the daemon's own
upgrade onto the new build fails (the journal says "the upgrade onto X failed: …; the grace for
X starts now"), the daemon puts the previous app back 20 seconds later, reopens your windows,
and records the version in
`~/Library/Application Support/arkos/updates/rejected` so it is never downloaded again (a newer
release still is). To try a rejected version again, delete that file.

Sessions live in `arkosd`, so updating never ends one. After `cargo build --release` into the
binaries you run (a build from source), the title bar shows "update ready" within
five minutes (or at the next launch), and the palette's "Update now" does the rest: if the
daemon's binary changed, the daemon writes a handover file, execs the new binary in the same
process (same pid, the shells stay its children) and the window reconnects; then the window
asks the daemon to relaunch it from the new binary and quits. Every tab comes back with its
history. The daemon and window speak protocol 5; the daemon still serves protocol 1 to 4 windows,
and reads the previous handover formats and recovery manifest, so updating from 0.2.12 or the
update-in-place build is an ordinary "Update now". Upgrading from milestone 2b is the one
update that needs `pkill -x arkosd` (the old daemon knows nothing of handovers), which ends
every session.

Click the badge to update at once.

If the new daemon fails its check or the exec fails, the old daemon keeps running and the window
shows the error. If the window loses the daemon for ten seconds, it starts a new one and every
tab becomes a fresh shell with the "(restarted)" note.

A window newer than its daemon (one started from a new build while an older daemon still runs)
is refused and stops reconnecting, so "Update now" cannot reach that daemon. The fix is the
one-time path above: `pkill -x arkosd`, which ends every session, and open the window again.

### When an update goes wrong

Run `arkos doctor` (from the app: `"/Applications/Arkos IDE.app/Contents/MacOS/arkos" doctor`).
It only reads: it prints the installed app and its version, whether a previous (`.old`) or
half-swapped (`.new`) copy sits beside it, the running daemon's version and pid, the update
marker and the rejected versions, the last lines of the daemon's update journal and of
`window.log`, and any crash reports from the last day. The journal,
`~/Library/Application Support/arkos/updates/journal.log`, has one timestamped line per step of
each check, apply, relaunch, confirmation and rollback, including every Hello the daemon heard
while it waited for the new window. A check that finds the same thing as the one before is
counted rather than written again: "check: N more checks since HH:MM found X, still not newer
than Y" when the outcome changes, at an update or exit, and once a day (failed checks the same
way, "check: transient, N more times since HH:MM: …"), so the journal holds weeks.
`~/Library/Application Support/arkos/window.log` keeps the
output of every window the daemon launched. Both are what to attach to a bug report; both name
the folders your windows had open, so look them over first.

### Renamed from Sarkos

The IDE used to be called Sarkos. A machine that ran it keeps the old login item, daemon,
config and saved workspaces under the old name; move them over once (this ends the old
daemon's sessions):

1. `launchctl bootout gui/$(id -u)/sh.csarko.sarkosd && rm ~/Library/LaunchAgents/sh.csarko.sarkosd.plist`
2. `pkill -x sarkosd`
3. `mv ~/.config/sarkos ~/.config/arkos`
4. `mv "$HOME/Library/Application Support/sarkos/workspaces" "$HOME/Library/Application Support/arkos/"`
5. Open Arkos IDE and run "Install Claude Code hooks" and "Start at login" from the palette
   (Cmd+Shift+P).

## Releasing

Releases are built by the `release` workflow, which runs only when you start it:
`gh workflow run release -f version=0.2.1` (or the Actions tab). It bumps the version, tags
`v0.2.1`, builds on a macOS runner, assembles and ad-hoc signs `Arkos IDE.app`, signs
`manifest.json` with the release key (the repository secret `ARKOS_RELEASE_KEY`; make a pair with
`scripts/release-key`, keep the secret there and in your password manager, and put the public half
in `crates/protocol/src/release_key.rs`), and publishes a GitHub Release. Every running daemon
sees it within five minutes. `release-tool verify --public <key> manifest.json manifest.json.sig`
checks a published manifest by hand. `packaging/test.sh` runs the same packaging steps locally on
a dev build.

## Build and run

```bash
cargo build --release
./target/release/arkos        # starts arkosd on demand, opens one terminal in the current folder
```

This runs a build from source. The released app is `/Applications/Arkos IDE.app`, installed by
the Homebrew cask and kept current by the daemon (see Updating); `packaging/bundle.sh` is what
assembles it.

Opening from the Dock (or Finder, or Spotlight) asks which folder to open: a small chooser lists
the five most recent folders, newest first, with "Show all" for the rest and "Open another
folder…" for the native picker. Up, Down and Enter choose; Escape quits. The chooser opens at
once, before the daemon answers: until it does, the footer says "connecting to the daemon…" and
the session counts are missing, and a folder chosen meanwhile opens as soon as the connection is
there. If the daemon goes away while the chooser is open, it reconnects as a window does (the
footer says "reconnecting to the daemon…", and after ten seconds a new daemon is started). When
no daemon can be reached or started, a small window says "Arkos cannot reach its daemon" with
the error, and offers "Open the log" (`arkosd.log`), "Try again" and "Quit"; Escape quits. A
window relaunched after an update shows the same window if its connect fails. The chooser's text
follows the config's preset size and the system appearance. Each folder's row ends
with how many of its sessions the daemon is running (a tab closed within its minute and a
worktree tab both count) and "Stop all": one click, or Delete on the selected row, arms it as
"Stop N sessions? Confirm"; a second ends every one of them (a window open on that folder sees
its shells end); Escape or moving the selection takes it back. Those counts show at launch, not
in the chooser opened from a running window. From a running window,
Cmd+O ("Open folder…") and "Open recent…" in the palette open the folder in a second window;
the folder this window already shows gets the notice "already open here". The list lives in
`~/Library/Application Support/arkos/recent.json`.

The daemon keeps running after the window closes; sessions survive the UI being killed. Logs:
`~/Library/Application Support/arkos/arkosd.log`. A window newer than the daemon is refused
with a banner and stops reconnecting; `pkill -x arkosd` and a relaunch fix it (see Updating). A
window older than the daemon keeps working. `pkill -x arkosd` stops the daemon and ends every
session.

Upgrading from milestone 1: the window finds milestone 1's saved session and moves it to the
new workspace format on first launch. A daemon started by milestone 1 keeps running without
this milestone's lifecycle fixes, though; `pkill -x arkosd` restarts it (the window starts a
new one) and ends every session, so do it when nothing important is running.

## Tests

```bash
cargo test
sh packaging/test.sh   # a dev build bundled, a manifest signed with a throwaway key, verified
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
