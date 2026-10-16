# Developing Arkos IDE

Building it from source, the tests, cutting a release, the screenshot shoot, and the status
of each milestone with its manual checks. How to use it is in [guide.md](guide.md).

## Build and run

```bash
cargo build --release
./target/release/arkos        # starts arkosd on demand, opens one terminal in the current folder
./target/release/arkos ~/Projects/x   # the same, on that folder
```

This runs a build from source. The released app is `/Applications/Arkos IDE.app`, installed by
the Homebrew cask and kept current by the daemon (see [Updating](guide.md#updating)); `packaging/bundle.sh` is what
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
Cmd+O ("Open folder…") and Cmd+N ("New window", the chooser) open the folder in a second window;
the folder this window already shows gets the notice "already open here". The list lives in
`~/Library/Application Support/arkos/recent.json`.

The daemon keeps running after the window closes; sessions survive the UI being killed. Logs:
`~/Library/Application Support/arkos/arkosd.log`. A window newer than the daemon is refused
with a banner and stops reconnecting; `pkill -x arkosd` and a relaunch fix it (see [Updating](guide.md#updating)). A
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

The rendered view's Mermaid diagrams use mermaid 11.17.2, vendored as
`crates/arkos/assets/mermaid.min.js` and bundled into the binary. `scripts/vendor-mermaid.sh`
fetches it from the npm registry's tarball and checks it against the SHA-256 recorded there; to
move to a new release, change the version in that script and in `crates/arkos/src/mermaid.rs`
(`VERSION`), record the new hash, run the script and commit the file. The recorded SHA-256 for 11.17.2 is
`581ed7d74bd9048d0e3a91363927d72ef22942d7722546b27f7cc29e35390eb8`.

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

A release that macOS opens without the quarantine step, and whose updates keep the app's
permissions (Accessibility for the browser pane; an ad-hoc signature's identity is the binary's
hash, so macOS forgets them at every update), is signed with a Developer ID and notarized. Two
ways, the same steps:

- **`release-notarized` workflow**: `gh workflow run release-notarized -f version=0.3.1`. It
  needs the secrets `APPLE_CERT_P12` (base64 of a `.p12` holding the "Developer ID Application"
  identity and Apple's Developer ID G2 intermediate), `APPLE_CERT_PASSWORD`, `APPLE_API_KEY` (an
  App Store Connect API key, the `.p8`, base64), `APPLE_API_KEY_ID` and `APPLE_API_ISSUER`,
  besides `ARKOS_RELEASE_KEY`.
- **`packaging/release.sh 0.3.1`** on a Mac that holds the identity in its keychain and a
  notary profile `arkos-notary` (`xcrun notarytool store-credentials arkos-notary --key <.p8>
  --key-id <id> --issuer <id>`, once), with `ARKOS_RELEASE_KEY` in the environment.

`packaging/bundle.sh` signs with `CODESIGN_IDENTITY` when it is set (hardened runtime, timestamp)
and ad-hoc otherwise; `packaging/notarize.sh` submits the zip, staples the ticket and rewrites the
zip, and runs before `manifest.sh`, which records the final zip. The key, the certificate and the
`.p8` live outside the repository (`~/.config/arkos-release/`): back them up; a lost key means a
new identity and reset permissions on every Mac.

## Screenshots

The pictures in `docs/images/` (the README's and csarko.sh's) are taken of the real app by
`scripts/shoot.sh`: one scene per storyboard in `scripts/shoot/stories/` (the multi tasker, the
web developer, the blogger), each a dev build on a demo project of its own (`scripts/shoot/demo`)
with real Claude Code sessions given the prompts written there, captured once the tabs show the
states the storyboard asks for, and the hero (`hero-dark.png`, `hero-light.png`) composed from
the scenes' stills with the multi tasker in front. Nothing in them is staged afterwards; change a
storyboard to change its picture, and `scripts/shoot.sh <scene>` redoes that scene alone
(`compose` only remakes the hero). The shoot runs a second arkosd of its own (`ARKOS_APP_SUPPORT`
names its state folder, under `~/Library/Caches/arkos-shoot`), so the Arkos in use is untouched,
and runs as an app of its own ("Arkos Shoot", signed with the Developer ID from your keychain, as
is the shoot's copy of Arkos), so macOS attributes its captures to it whatever shell starts it:
grant it Screen Recording once, and the shoot's copy of Arkos Accessibility once (the browser
pane). Each scene spends a few minutes of your Claude Code account. Progress captures and the
panes' states land in `docs/images/progress/<scene>/` every 20 s while it runs, and a scene whose
Claude Code panes never start is ended after a minute.

## Status

Session awareness and recovery after a restart: each tab shows what its Claude Code session is
doing (a breathing glyph while it works, a warm dot when it waits on you, a muted title when the
shell ended), the tab marks show which sessions wait, and with "Start at login" on, a restart
brings every session back on its own: the same folders, the history on screen under a divider, and
each Claude Code conversation resumed. The sidebar shows git status in the tree and the
worktrees, and the viewer marks a file's changes against HEAD. The browser pane docks a Chrome
window into a pane (Accessibility permission needed). Milestone 6a made the update path and the
launcher trustworthy: "Check for updates" waits for the check and says what it found, the chooser
opens before the daemon answers and reconnects, and a closed tab's minute is kept by the daemon.
Milestones 6b and 6c are next.

Things only a person at the screen can check, in order of value:

- markdown, Rendered: a table draws as a grid and a click lands in the cell with the caret there;
  Tab, Shift+Tab, Up and Down move by cell; Tab in the last cell adds a row; leaving the block
  realigns the pipes and one undo restores them; a wide table wraps, and with "Scroll" in Settings
  (or `arkos settings set viewer.wide_tables scroll`) it scrolls sideways with Shift+wheel, while a
  vertical wheel still scrolls the list; a table over 500 rows shows as source
- markdown, Rendered: in a README whose table is followed by a blank line and reference links
  (`[ref]: https://...`), edit the table and leave it: only the table is realigned, the definitions
  stay as they were and every `[text][ref]` link still works; the grid shows no row for them
- a remote image shows "loading" and then the image; a plain click places the caret and Cmd+click
  opens it in Preview; a `.mov` shows its poster and Cmd+click opens QuickTime Player; audio opens
  in QuickTime Player; a local `.webm` is shown in Finder; a badge row wraps in a narrow pane
- a `mermaid` block renders within two seconds and again after an edit; with Google Chrome
  missing, the source shows with a message; the docking check: start a slow render (a large
  diagram) and run `lsappinfo list | grep -B2 -A8 com.google.Chrome` to see how the headless child
  registers ("Foreground" is a regular app; "UIElement" or "BackgroundOnly" is not), then quit the
  real Chrome and dock the browser pane during a render: the pane must not grab the headless child
- a `<details>` block opens and closes and is still open after a relaunch; `<details open>` starts
  open until you close it
- a 5 MB markdown file with a large table stays responsive (the milestone 7 gate)
- `claude` in a tab with `/tui fullscreen`, a long answer: the wheel and the trackpad scroll the
  conversation; a question with options: clicking one picks it; Option+Delete at the prompt
  deletes the word before the cursor
- drag across terminal text: it tints; Cmd+C, then Cmd+V at a prompt pastes it; double click
  selects a word, triple click a line, a click clears it; a drag inside Claude Code's fullscreen
  selects without the program reacting; `less` on a long file: the wheel moves the file;
  Shift+wheel on a mouse, in a shell with history: wheel up shows older lines, not newer
- terminal selection and links: a right-click on a link in Claude Code's output shows "Open Link" and
  "Copy Link" and nothing elsewhere, and Control+click on a link opens the same menu, while a
  right-click in the pane's padding opens nothing; the menu looks like the file tree's (the card,
  the hover, the fade) and closes on Escape, on a click elsewhere and after an item runs; Cmd+C on a
  wrapped Claude Code paragraph pastes one line while a bulleted list keeps its lines, and
  Cmd+Shift+C pastes it as shown; in a terminal 160 columns or wider, Claude Code's prose should
  reach within 12 cells of the width (the re-flow's margin; if it caps its width there, the margin
  in `reflow.rs` needs revisiting); press, wheel-scroll, then drag: the selection starts at the text
  that was under the press; drag past the pane's bottom edge, and above its top edge: the history
  scrolls and the selection grows until the button comes up; auto-scroll stops when the pointer
  comes back inside and when the button is released outside the pane; no auto-scroll in Claude
  Code's fullscreen renderer, vim or `less`; a wheel notch while dragging moves the selection's end
  with the text
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
- a worktree with 40 changes lists them readably, one per row, and the list scrolls
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
  appears when it stops, clicking the tab clears it; with Reduce Motion on, the glyph holds still
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
  "Update ready" on the bell (or relaunch), run "Update now" from the palette: the window
  reappears with every tab, every history and every agent still running; `arkosd`'s pid in
  `ps` is unchanged and its log shows "upgrading to" and "resumed N sessions" under a second apart
- `pkill -x arkosd` with the window open: tabs say "reconnecting", and the bell says
  "arkosd reconnecting"; after ten
  seconds a banner appears, a new daemon starts, and each tab comes back as a new shell marked
  "(restarted)"
- "Update now" when nothing changed relaunches the window and every tab comes back attached
- push, `gh workflow run release -f version=X`, wait for the run, see "Update ready: X" in the
  bell within five minutes, stop typing for thirty seconds: the window relaunches with every
  shell and tab where it was; `arkos --version` in a tab prints X
- `[update] auto = false`: the bell's dot stays and "Update now" applies it
- a relaunched window opens without taking focus from the app in use (verify by eye)
- the menu bar's first menu, "Arkos IDE": About Arkos IDE, a line, New Window, Settings, Quit, in
  that order, and no File menu; Cmd+N or "New Window" opens the chooser with
  the recent folders and "Open folder..."; picking one opens it in a new window; `"open recent"
  = "cmd-shift-n"` left in config.toml binds the chooser to that key with no config error
- open a README in Arkos: it renders; click a paragraph: it turns raw with the caret where you
  clicked; type, wait a second: `git diff` shows the change and the tab's " •" goes; press Enter in
  a list: the next bullet appears; tick a task checkbox: the file shows `[x]`
- the "show raw" pill at the top right of the pane, or Shift+Cmd+M, shows the raw file and
  back; the choice survives a relaunch; another markdown tab keeps its own mode
- edit the file in another editor while a line is half typed: the banner offers Reload and
  Keep mine; Cmd+S means Keep mine; with nothing unsaved the file simply reloads
- a 5 MB markdown file opens and scrolls without stutter; typing in it keeps up; Reduce
  Motion changes nothing here; a `.rs` file is still read-only; a file that is not UTF-8 opens
  read-only with a banner
- the title bar's right side shows a bell and a wrench (the doctor) and no text; with an
  update staged the bell wears a dot and its popover says "Update ready: X" with "Update now";
  after an update the bell says "Updated to X" once, gone after you open and close it; a
  daemon drop shows "arkosd reconnecting" there and the row goes when it is back
- with `wheelScrollAccelerationEnabled` absent from `~/.claude/settings.json` the doctor wears a
  dot; "Apply" writes the key and a `settings.json.bak`; a restarted `claude` scrolls smoothly
  in a fullscreen conversation; the doctor's Start at login and title hook rows match the
  palette's commands; "Copy report" puts `arkos doctor`'s text on the clipboard
- Escape, a click outside, the icon again, or switching to another app closes a popover; opening
  one closes the other; Reduce Motion shows them without the lift
- move the window to a second display and resize it, then "Update now" (or quit and open the
  folder again): the window comes back on that display, the same size, in the same place; with
  that display unplugged it opens centered on the main display at that size (macOS puts a new
  window on the current Space; an app cannot pick one)
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
- Cmd+, opens Settings; "Open config" in the palette opens the config file in the default text
  editor
- About: Version, Build and Protocol match `arkos --version`; the Config, Themes, App Support,
  Window log and Daemon log paths exist; "Copy details" then paste gives "Arkos IDE {version}"
  and one `label: value` line per row; the "Buy me more tokens" line is hidden while
  `DONATE_URL` is empty
- Settings: Dark theme or Mode switches the window at once and `config.toml` keeps its comments;
  "Add theme…" with a broken file shows the error and copies nothing, and with a file named
  like an installed theme asks "Replace the existing ..." first
- edit the configured theme's file (`~/.config/arkos/themes/<name>.toml`, change `window_bg`) while
  the window shows it: the colour follows within a second; save it with a typo: the banner names
  the error and the last good colours stay
- `arkos theme contrast` on a copy of `navy` with `tree.ignored` set close to `window_bg`: a
  `LOW` line and exit 1; fix the colour: every line `ok` and exit 0
- `arkos theme preview <file>` with two windows open: both show the draft and nothing in
  `config.toml` changes; `arkos theme preview --end` restores both
- `arkos theme use timo-purple`: `config.toml` keeps its comments with `[theme] dark` changed,
  and one `window {pid} ({project}): timo-purple applied` line per window
- `arkos settings set font.preset compact`: every terminal shrinks within a second, and the
  config's comments are still there
- a theme file with a syntax error in `~/.config/arkos/themes/`: `arkos doctor` lists it under
  `themes:`; with no skill installed the doctor's "Agent guide for Claude Code" row says so, and
  "Install" installs it
- `arkos guide` prints the guide; `arkos guide --path` prints a path to a readable copy
- with the hooks installed and no skill, a fresh `claude` in a tab, asked "change the theme to
  something warm": it runs `arkos guide` unprompted, makes and applies a theme, and ends with
  `arkos theme current` showing it; afterwards `skills/arkos-ide/SKILL.md` exists in that
  session's config folder (`CLAUDE_CONFIG_DIR`, else `~/.claude`)
- the same with the skill installed (`arkos guide --install`) and the hooks removed
- `arkos theme list` shows `navy`, `navy-light`, `timo-purple`, `peach-light` and `lavender-light` as built in; with Timo Purple
  chosen, the purple, cyan and green match the screenshot (check at the offsite demo)
- About open, close the project window with the red button: the app quits
- Settings opened from the chooser, a theme or text change, then a folder: the project window
  opens with the change in effect
- the menu bar's second menu, "Help": Keybindings with Cmd+/ beside it, then Documentation
- Cmd+/ (or Help, Keybindings) opens the Keybindings window with eight sections, "Window and app"
  first and "Updates and maintenance" last; Escape or Cmd+W closes it; Cmd+/ again brings the same
  window forward
- with the Keybindings window open, `"find" = "cmd-shift-f"` saved under `[keybindings]`: its Find
  row reads Shift+Cmd+F within a second; `"doctor" = ""` leaves Doctor reading "none"
- Help, Documentation: this README opens in the default browser
- Settings, Keyboard, "Keybindings": the Keybindings window opens
- About, Settings and Keybindings show the IDE's own title bar with the window's name and no
  native one; dragging the bar moves the window, a double click zooms Settings and Keybindings,
  and the bar follows a theme change with the rest of the window
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
- with three tabs in a pane, drag the first slowly to the right: its slot closes, the others
  slide left, and a gap its width glides along as the pointer crosses each tab; drop: the tab
  lands in the gap with no jump; drag again and release over the sidebar: the gap closes and
  the slot opens again; Reduce Motion: the same without the glide
- double-click a tab's title, type a name, Enter: the tab shows it; `claude` retitles its own
  window and the name stays; relaunch (Update now): the name is back; close the tab and
  Shift+Cmd+T: the automatic title is back
- a link in terminal output is underlined before the pointer reaches it; two links on one line
  are each underlined
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
