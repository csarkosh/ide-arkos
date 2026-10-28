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

### gpui fork

Arkos builds against gpui from its fork, https://github.com/csarkosh/gpui, pinned in the
workspace `Cargo.toml`'s `[patch.crates-io]` by tag (now `v0.2.2-arkos.2`). The fork is the
crates.io tarball of gpui 0.2.2 (tag `v0.2.2`) with two commits on top, the hooks the effects
use: the per-window glyph shader (`Window::set_glyph_shader`, `GlyphShader`,
`GLYPH_SHADER_PRELUDE`, `GLYPH_SHADER_BASE`; tag `v0.2.2-arkos.1`) and its clock
(`Window::set_glyph_shader_time`, one `float` at Metal buffer index 5 in both glyph stages; tag
`v0.2.2-arkos.2`). An upgrade goes as the fork's `ARKOS.md` says:
commit the next tarball as `v<version>`, rebase the patch commits onto it, tag
`v<version>-arkos.1`, push, point `[patch.crates-io]` at the new tag and run
`cargo update -p gpui`.

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

## Motion

Every transition in the window goes through `crates/arkos/src/motion.rs`, on `Instant`, with
one duration per kind: `ENTER` (120 ms, chrome appearing), `EXIT` (90 ms, chrome going),
`SIDEBAR_SLIDE` (150 ms, Cmd+B) and `THEME_FADE` (200 ms, a theme switch). Under macOS's Reduce
Motion every curve is at its end at once; nothing animates. The rules that keep this working as
features are added:

- A colour is read from `root.theme` at paint time and never cached in a view or an element
  built earlier. For 200 ms after a theme switch `root.theme` is a `theme::blend` of the old and
  the new theme, remade each frame; a view that keeps a colour from an earlier frame snaps or
  lags behind the rest of the window. A cache keyed on a colour (as `md_view`'s `CodeRuns` is on
  its text context) recomputes every frame of a fade rather than lagging, so key a cache on
  something stable and apply the colour when building from it. The one known exception is a
  drag chip: `TabDrag` and `FileDrag` in `drag.rs` capture their colours at drag start, so a
  theme switch during a drag leaves the chip in the old colours until the drop.
- Anything that derives data from the theme once, rather than reading it at paint (the
  highlighter's spans, a viewer's re-highlight, the terminal program's theme push, Claude Code's
  `CSI ? 997 n`), does so from the final theme at the end of the fade, in `Root::finish_theme`,
  never per frame. Per-frame work is limited to swapping what the next paint reads.
- A new animated thing adds its `animating(now)` to `Root::any_motion`, so the one
  `request_animation_frame` in `Root::render` serves every transition and an idle window still
  paints nothing. Nothing repaints the window on a timer to pick up changes: whoever changes
  what the window shows calls `cx.notify()` (a daemon event's tab state, `route_events`; a
  response's outcome, the connection pump). A frame reads only the entities it draws (a tab's
  label comes from `SessionManager::labels`, not its terminal), because gpui repaints a window
  whenever an entity it read changes. Read `now` once per frame (`Root::frame_now`); never call `Instant::now()` per
  element.
- A new surface that reads the palette or the theme (a window, a panel, a program told about
  the theme) is checked by hand with a theme switch: it must fade with the window, not jump.
- An effect (`crates/arkos/src/effects`) comes and goes over `THEME_FADE`: its surfaces' alphas
  fade through `Root::effect_style`, and what can only cut (the text size, the borders, the
  highlights, the glyph shader) switches at a fade in's end and a fade out's start. An effect's
  wash is drawn from its own timer at the manifest's rate, sixty while a highlight fades, never
  under Reduce Motion unless its inputs changed (its clock is frozen there), and never while the
  window is hidden, occluded or covered; gpui redraws nothing for it. While the window cannot be
  seen the timer builds nothing and looks again every half second (`effects::active::REST`);
  under Reduce Motion it stops once nothing moves, and `Root::render` starts it for one tick
  after each frame. A surface that paints a fill under
  the wash reads its alpha from `Root::effect_style` at paint time, and UI text sizes go
  through `Root::ui_size`.

## Releasing

Every release is signed with the Developer ID and notarized, so macOS opens the download without
the quarantine step and updates keep the app's permissions (the protected folders a tab was
allowed). macOS keys those permissions to the signature, so a release signed any other way makes
every Mac forget them: until 0.5.1 releases alternated between an ad-hoc workflow and the
notarized one, and each switch asked for Accessibility (then needed by the browser pane) again. That `release` workflow is retired (it only
refuses now), and a Developer ID copy's daemon refuses to stage an update its team did not sign
(`check_signer` in `crates/arkosd/src/updates/stage.rs`), so a wrongly signed release cannot cost
anyone their permissions. Two ways, the same steps:

- **`release-notarized` workflow**: `gh workflow run release-notarized -f version=0.3.1` (or the
  Actions tab; it runs only when you start it). It bumps the version, tags `v0.3.1`, builds on a
  macOS runner, signs and notarizes `Arkos IDE.app`, signs `manifest.json` with the release key,
  and publishes a GitHub Release; every running daemon sees it within five minutes. It needs the
  secrets `APPLE_CERT_P12` (base64 of a `.p12` holding the "Developer ID Application" identity
  and Apple's Developer ID G2 intermediate), `APPLE_CERT_PASSWORD`, `APPLE_API_KEY` (an App Store
  Connect API key, the `.p8`, base64), `APPLE_API_KEY_ID` and `APPLE_API_ISSUER`, besides
  `ARKOS_RELEASE_KEY` (make a pair with `scripts/release-key`, keep the secret there and in your
  password manager, and put the public half in `crates/protocol/src/release_key.rs`).
- **`packaging/release.sh 0.3.1`** on a Mac that holds the identity in its keychain and a
  notary profile `arkos-notary` (`xcrun notarytool store-credentials arkos-notary --key <.p8>
  --key-id <id> --issuer <id>`, once), with `ARKOS_RELEASE_KEY` in the environment.

After a release, bump the Homebrew cask (`csarkosh/homebrew-tap`, `Casks/arkos-ide.rb`: version
and the zip's sha256) and run csarko.sh's `apps/deploy.sh` so the download button names it.
`release-tool verify --public <key> manifest.json manifest.json.sig` checks a published manifest
by hand. `packaging/test.sh` runs the same packaging steps locally on a dev build.

`packaging/bundle.sh` signs with `CODESIGN_IDENTITY` when it is set (hardened runtime, timestamp)
and ad-hoc otherwise; `packaging/notarize.sh` submits the zip, staples the ticket and rewrites the
zip, and runs before `manifest.sh`, which records the final zip. The key, the certificate and the
`.p8` live outside the repository (`~/.config/arkos-release/`): back them up; a lost key means a
new identity and reset permissions on every Mac.

Both release paths also do what makes a build a release for telemetry ([what Arkos
sends](telemetry.md)). `bundle.sh` run with `ARKOS_CHANNEL=release` writes
`Contents/Resources/channel` saying `release` before signing, and only such a bundle can ever
upload; `packaging/test.sh`, the shoot and every dev build have no channel file and are `source`,
which never sends anything. On the maintainer's own Macs, an empty `~/.config/arkos/dogfood`
(`touch` it once per Mac; the daemon reads it when it next starts, after an update or a log-out) makes a release
say `dogfood` instead, and the admin dashboard (`admin.csarko.sh/ide-arkos/`) shows that use apart
from everyone else's; the collector also marks rows from the addresses it ignores `own`. The build passes `--remap-path-prefix` for the checkout (`/src`), the
Cargo home (`/cargo`) and the workspace's relative `crates/` paths (`/src/crates`), so a panic's
`file` reads `/src/crates/arkos/src/md_view.rs` and never a home folder (the toolchain's own files
already read `/rustc/<hash>`), and the release fails when the binaries were built without their
commit hash. The release profile's `split-debuginfo = "packed"` leaves `arkos.dSYM` and
`arkosd.dSYM` beside the binaries in `target/release` (cargo's are symlinks into `deps/`), and
`strip = "debuginfo"` takes the build machine's paths out of the binaries once dsymutil has run;
`packaging/dsym.sh` checks that each carries its binary's UUID and zips both to
`Arkos-IDE-<version>-dSYM.zip`, published with the release beside the app zip.

To symbolicate a crash report, download the dSYM zip of the report's `ver` from its GitHub Release
and unzip it. The report's `images` names the binary (`arkos` or `arkosd`) and its UUID, which
`dwarfdump --uuid arkos.dSYM` shows too: the report has the same 32 hex digits in lower case
without dashes, and a different one means another build's symbols. Each of `frames` is
`[image, offset]`: the offset of a return address from the binary's load address, in decimal.
`atos` takes the dSYM, the binary's link address `0x100000000` as the load address, and per
frame that address plus the offset minus one (a return address points just after its call):

```bash
unzip Arkos-IDE-0.5.0-dSYM.zip    # arkos.dSYM and arkosd.dSYM
for OFF in 1843200 1790466; do printf '0x%x ' $((0x100000000 + OFF - 1)); done |
  xargs atos -o arkos.dSYM -l 0x100000000
```

The first nine or so frames are the panic hook and std's panic machinery; the panic's own path
through Arkos starts after them, and the report's `file` and `line` say where it was raised. The
names come out in Rust's mangled form (`_RNv...`); `rustfilt` (`cargo install rustfilt`) turns
them back into Rust paths.

## Screenshots

The pictures in `docs/images/` (the README's and csarko.sh's) are taken of the real app by
`scripts/shoot.sh`: one scene per storyboard in `scripts/shoot/stories/` (the multi tasker, the
web developer, the blogger), each a dev build on a demo project of its own (`scripts/shoot/demo`)
with real Claude Code sessions given the prompts written there, captured once the tabs show the
states the storyboard asks for, and the hero (`hero-dark.png`) composed from
the scenes' stills with the multi tasker in front. Nothing in them is staged afterwards; change a
storyboard to change its picture, and `scripts/shoot.sh <scene>` redoes that scene alone
(`compose` only remakes the hero). The shoot runs a second arkosd of its own (`ARKOS_APP_SUPPORT`
names its state folder, under `~/Library/Caches/arkos-shoot`), so the Arkos in use is untouched,
and runs as an app of its own ("Arkos Shoot", signed with the Developer ID from your keychain, as
is the shoot's copy of Arkos), so macOS attributes its captures to it whatever shell starts it:
grant it Screen Recording once. Each scene spends a few minutes of your Claude Code account. Progress captures and the
panes' states land in `docs/images/progress/<scene>/` every 20 s while it runs, and a scene whose
Claude Code panes never start is ended after a minute.

The pictures are shot in the cloud: the `shoot` workflow runs `scripts/shoot.sh` on a GitHub
macOS runner, Claude Code signed in with the `CLAUDE_CODE_OAUTH_TOKEN` secret (from `claude
setup-token`). `gh workflow run shoot -f scenes="blogger"` (scenes space-separated; `probe`
checks the runner without Claude Code), then `gh run download <run id> -D <dir>` and copy the
stills it changed into `docs/images/`; the hero is recomposed on the runner with them. Only the
dark stills are taken (each storyboard has one `dark` shot) and the README shows those alone.

## Status

Session awareness and recovery after a restart: each tab shows what its Claude Code session is
doing (a breathing glyph while it works, a warm dot when it waits on you, a muted title when the
shell ended), the tab marks show which sessions wait, and with "Start at login" on, a restart
brings every session back on its own: the same folders, the history on screen under a divider, and
each Claude Code conversation resumed. The sidebar shows git status in the tree and the
worktrees, and the viewer marks a file's changes against HEAD. The browser pane shows a web page
inside the pane (a WKWebView, 0.7; before it a Chrome window was docked over the pane). Milestone 6a made the update path and the
launcher trustworthy: "Check for updates" waits for the check and says what it found, the chooser
opens before the daemon answers and reconnects, and a closed tab's minute is kept by the daemon.
Milestones 6b and 6c are next. Milestone telemetry added anonymous usage reports and crash reports.
Usage reports stay on your Mac until you turn them on (Settings > Privacy, or `arkos telemetry
on`). A crash report is sent only when you press Send in the dialog that shows it, or when you have
chosen Always. It also added an invitation strip on the second day, the update check once a day
through the project's own server, and published dSYMs for release builds. What is sent is in
[telemetry.md](telemetry.md). Milestone 9 added the video pane: a video is a tab of its own,
drawn in process through AVFoundation with a control strip (play, time, scrubber, loop, mute) and
keys while the pane is focused; it follows its file, reloading a new render within about two
seconds and holding the position, and a relaunch brings it back paused on its frame. The sidebar,
the markdown poster's Cmd+click and the palette's "Open file in pane…" open it, and agents get
`arkos open`, `arkos video` and `arkos panes` over a targeted channel to their own window
(protocol 8), found through the SessionStart hook's line, the skill and `arkos guide`.

Things only a person at the screen can check, in order of value:

- effects: Peach Light (`effect.name = "theme"`) shows the watercolor wash under the window,
  the ink line between Explorer and Worktrees, and strokes that fade in and out on the
  selected and hovered Explorer rows and the hovered tab; the window log says `effect
  watercolor: … ms per frame`
- effects: `arkos settings set effect.name none` fades the wash out over 200 ms and the plain
  theme is back (fills, borders, text size); `effect.name theme` brings it back with a fade
- effects: Navy with `effect.name = "watercolor"` reads navy (the wash takes the theme's colours)
- effects: with Reduce Motion on the wash is still and Activity Monitor shows the window idle; a
  hover still moves its stroke at once
- effects: a window wholly behind another app's window, or minimized, shows the process idle in
  Activity Monitor (no wash frames, a few wake-ups a second at most), and its wash moves again
  within half a second of coming back into view
- effects: a window covered by the palette or a menu draws no wash frames meanwhile; a resized
  window's wash follows its new size at once, with no stretched or offset blocks
- effects: a `wash.metal` in the effects folder that fails to compile shows a notice naming the
  file and its line, and the theme draws plain
- effects: under Peach Light the project window's text is ink (feathered, uneven, and deeper
  than the theme's own, cross-faded) from the fade in's end, and plain from a fade out's start;
  the About, Settings and Keybindings windows and the chooser keep gpui's plain glyphs
- effects: a `glyphs.metal` in the effects folder that fails to compile shows a notice
  `effect <name>: glyphs.metal: …`, and the wash stays on with plain glyphs
- effects: Settings, Appearance lists Effect under the themes ("Theme's choice (watercolor)"
  under Peach Light, None, watercolor, an invalid folder dimmed as "(invalid)"); a click switches
  the window with a fade, the Intensity stepper moves the wash's strength by a tenth, and "Show
  effects folder" creates and reveals the folder
- effects: with a window open, `arkos effect current` names its effect and a cost in
  milliseconds after a few seconds, and `arkos effect bench watercolor` benches at the window's
  size and theme
- effects: under Peach Light a video tab shows the wash beside the picture (no cream bars at its
  sides or above and below), an ink outline just outside the picture that follows it through a
  resize and a split, and the control strip clear over the wash; with `effect.name none` the bars
  come back in the window's colour
- effects: under Peach Light a rendered markdown table shows the wash through its cells (an
  apricot band over the header, a light wash over the body, the stripe kept), an ink outline and
  a line under the header from the wash, and inner lines in ink; scrolled half out of the pane,
  its outline stops at the pane's edge; with `effect.name none` the cream fills and grey lines
  come back
- video pane: a video opens from the sidebar and plays with sound; Space, the arrows, comma and
  period do what the Keyboard table says
- video pane: dragging the tab to another pane keeps it playing after the drop, and switching
  tabs pauses it (switching back resumes)
- video pane: re-rendering the file with ffmpeg reloads the tab within two seconds, holding the
  position, and the half-written file never shows; deleting the file shows "waiting for file" and
  restoring it recovers
- video pane: an ffmpeg re-render written over a cut while it plays reloads the tab within two
  seconds, holding the position and still playing
- video pane: a truncated file written over a playing cut (`head -c 100000 good.mp4 > cut.mp4`)
  leaves the old cut on screen with the strip saying "new file cannot be played yet", and the next
  good write loads
- video pane: a relaunch brings the tab back paused on the frame
- video pane: `arkos open` from a Claude Code tab splits a preview pane to the right the first
  time and reuses it the second; the keyboard focus stays in the Claude Code tab
- video pane: open a cut from the sidebar into the Claude Code tab's pane, switch back to the
  Claude Code tab, and have the agent `arkos open` it: the video moves, at its position, into a pane
  to the right, the Claude Code tab stays shown, and Space still types to Claude Code
- video pane: `arkos open <file> --at 0:12 --loop 0:12-0:18 --playing` loops the range with sound
- video pane, the fresh session demo: from a fresh Claude Code session in a window with the hooks
  installed (pointing at the dev binary, or after the release), "show me the animatic beside you
  and stop on the radio scene" ends with the video paused in a pane to the right, and the agent
  ran `arkos guide` first
- video pane: a 4K file plays without the window stuttering, and the GPU stays idle when every
  video is paused (Activity Monitor's GPU column)
- video pane, the strip: Space plays with sound; the scrubber moves and a click seeks; a drag
  scrubs showing frames and resumes playback if it was playing; comma and period step one frame;
  L loops (Shift+drag on the scrubber selects a range); M mutes; R reloads
- video pane: click the picture of a playing video: it pauses; click again: it plays; a click on
  the strip's scrubber still seeks
- video pane, the strip: it stays legible in a narrow pane, and the Unplayable text is legible
  over a poster
- video pane: a paused seek shows its frame within half a second on a 4K HEVC file, and a
  trackpad tap on the scrubber does not leave it stuck paused

- markdown, Rendered: a table draws as a grid and a click lands in the cell with the caret there;
  Tab, Shift+Tab, Up and Down move by cell; Tab in the last cell adds a row; leaving the block
  realigns the pipes and one undo restores them; a wide table wraps, and with "Scroll" in Settings
  (or `arkos settings set viewer.wide_tables scroll`) it scrolls sideways with Shift+wheel, while a
  vertical wheel still scrolls the list; a table over 500 rows shows as source
- markdown, Rendered: in a README whose table is followed by a blank line and reference links
  (`[ref]: https://...`), edit the table and leave it: only the table is realigned, the definitions
  stay as they were and every `[text][ref]` link still works; the grid shows no row for them
- a remote image shows "loading" and then the image; a plain click places the caret and Cmd+click
  opens it in Preview; a `.mov` shows its poster and Cmd+click opens a video tab beside the document; audio opens
  in QuickTime Player; a local `.webm` is shown in Finder; a badge row wraps in a narrow pane
- a `mermaid` block renders within two seconds and again after an edit; with Google Chrome
  missing, the source shows with a message; the docking check: start a slow render (a large
  diagram) and run `lsappinfo list | grep -B2 -A8 com.google.Chrome` to see how the headless child
  registers ("Foreground" is a regular app; "UIElement" or "BackgroundOnly" is not)
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
  comes back inside and when the button is released outside the pane; a right-click or a
  Control-release in the middle of a drag ends the drag and stops any auto-scroll; in a fullscreen
  `claude` with a long answer, drag from the last line up past the top edge: the conversation
  scrolls, the tint grows to the top and never covers the prompt box; release, Cmd+C, paste: the
  whole answer, no "Jump to bottom" text, no prompt box, no repeated header line; wheel after the
  release and the tint stays on its text; vim and `less` still do not scroll from a drag; a wheel
  notch while dragging moves the selection's end with the text
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
- in a new tab, `ps -o tty=,stat= -p $$` names a `ttys` and its state has `+`; `sleep 100` stops
  on Ctrl+C; `claude` with `/tui fullscreen` in that tab, then a pane split below it (or the tab
  dragged into a shorter pane): its input box stays at the bottom of the smaller pane, and comes
  back to full height when the split closes
- `pkill -x arkosd` with the window open: tabs say "reconnecting", and the bell says
  "arkosd reconnecting"; after ten
  seconds a banner appears, a new daemon starts, and each tab comes back as a new shell marked
  "(restarted)"
- "Update now" when nothing changed relaunches the window and every tab comes back attached
- push, `gh workflow run release-notarized -f version=X`, wait for the run, see "Update ready: X" in the
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
- "Theme: peach-light" in the palette, then a config reload: the config's theme wins
- a config naming `navy-light` (with no `navy-light.toml` of your own in the themes folder), then a launch: the bell shows "Navy Light was removed; your light theme is now Peach Light." once, the file now says `peach-light` with its comments intact, and the next launch shows nothing
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
- Settings: Dark theme or Mode switches the window and `config.toml` keeps its comments;
  "Add theme…" with a broken file shows the error and copies nothing, and with a file named
  like an installed theme asks "Replace the existing ..." first
- switch themes in Settings, with `arkos theme use`, and by flipping macOS appearance: the chrome
  and the terminals fade over 200 ms, the syntax colours in code viewers and markdown code blocks
  land at the end of the fade, and a second switch mid-fade starts from the colours on screen;
  with Reduce Motion on, the switch is instant
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
- `arkos theme list` shows `navy`, `timo-purple`, `peach-light` and `lavender-light` as built in; with Timo Purple
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
- web pane: Cmd+Shift+B, type `localhost:4173`, Enter: the page is drawn inside the pane under
  the bar; split the pane, drag the handle, toggle the sidebar, move and resize the window: the
  page follows with no gap and no flash
- web pane: switch to another tab in the pane and back: the page is where it was, scrolled as it
  was; open the palette over a web tab: the palette is on top of the page; Escape: the page is back
- web pane: `arkos open out/report.html` from a session, the page holding `<video>` elements that
  point at files beside it: the videos play with sound; edit the file: the page reloads within
  two seconds; `arkos open` of the same file again after an edit prints "reloaded"
- web pane: click into a page's text field and type: the page gets it; Cmd+1: the first pane takes
  the focus; click the tab bar: the pane's keys work again; Cmd+L: the address field has the cursor
- web pane: click into a page, then close its tab (its × or Cmd+W) and type into the terminal
  that takes its place: every key arrives once (the page handed the keys back as it went; without
  that, the window was its own first responder and each key reached the terminal twice)
- web pane: `arkos open http://localhost:1/`: the pane says "Could not connect to the server" with
  Retry; start the server, Retry: the page
- web pane: right-click a page, Inspect Element: the Web Inspector opens as its own window
- web pane: quit and relaunch: the web tab comes back on the page it was on, not the one it
  opened with
- web pane: a page that calls `window.open`: it opens in the same tab; back returns
- web pane: a page with a download link: the notice says downloads are not supported, and "open
  in your browser" opens the page in the default browser
- web pane: in a markdown document, Cmd+click a link to a local `.html` file: the page opens in a
  web tab in the pane to the right (or a new pane split off), and the keys stay with the document
- web pane: Reduce Motion on: nothing of the web pane changes, and every text field's cursor
  (the address field, the palette, a find bar) stops blinking and stays shown; off again: it blinks
  about twice a second, shown at once after every key or click
- web pane: in the address field, Option+Left and Option+Right stop at each `/`, `.`, `:` and `-`
  of `https://docs.rs/gpui-component`, Shift with them selects
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
  are each underlined; a URL longer than the pane printed by `echo` is underlined across both
  rows and Cmd+click opens the whole of it, and so is one pasted into Claude Code's input box,
  which it cuts two cells short of the pane; Cmd+C on that input box, or on a Claude Code
  message with a token wider than the pane, pastes the token unbroken, with no space at the cut
- with two panes side by side, the separator between them is a hairline and the focused pane's
  glow looks the same on its separator side as on its others; hover the separator: a wider
  stripe fades in and the panes do not move; drag it; move away: it fades out
- the focused pane's glow reaches all four edges, the bottom and right included; it breathes
  slowly while a tab in the window is working and, once the work stops, comes to rest at the
  end of its breath with no jump; click another pane: the glow cross-fades; Reduce Motion in
  System Settings: it rests; with the window in front and nothing working, the window draws no
  frames: `python3 scripts/frames.py <arkos pid>` reads about zero, and output in a tab that is
  not showing (`sleep 1; while :; do date; sleep 0.2; done` there) keeps it about zero too
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
- telemetry, the crash dialog (a release build only): its size is right, Show report resizes it,
  two restored windows show one dialog, Send with Always checked flips Settings to Always, and
  closing it without a choice puts the claims back
- telemetry, the invitation: on the second day the strip shows under the tab bar and keeps one
  line in a narrow pane; What Arkos sends in Settings opens a viewer tab
- telemetry, the release run: the first one leaves `target/release/*.dSYM` and publishes a dSYM zip
  with DWARF in it; a real panic in a release build symbolicates against that zip and reads
  `/src/...`
- telemetry, the collector: the first-party check row is seen in BigQuery, with `rd` right on the
  second day; the shoot's source build sends nothing
