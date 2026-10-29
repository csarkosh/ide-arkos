# Arkos IDE: the guide for coding agents

You are inside Arkos IDE when `ARKOS_SESSION_ID` is set in your environment: every shell the Arkos
daemon starts carries it. If you are Claude Code, Arkos's SessionStart hook placed this guide in
your own config folder as the skill `arkos-ide` (`$CLAUDE_CONFIG_DIR/skills`, else
`~/.claude/skills`), and `arkos guide` prints the same text. Read it all before you change anything.

## 1. What you can change, and what you cannot

Arkos is a macOS IDE of terminal panes, a file tree, a markdown editor and a video pane. Through
this guide you may show the user a video, a document or a web page in a pane of their window and
control a video there (play, pause, seek, step, loop, reload, mute, close), and you may change its theme (the
colours and the optional fonts), write and choose its effect (the shaders that paint the window,
section 8) and change the settings on a short allow-list. Nothing else is supported:
you may not run commands in its terminals or reach a session, and nothing here writes the files
you show. Do not edit the app bundle. Do not look for the built-in theme files: they are compiled in
and do not exist on disk. Keep the user's comments in `config.toml`; the commands below do that
for you, so never write the file by hand when a command exists.

## 2. Where things are

- The config: `~/.config/arkos/config.toml` (`$XDG_CONFIG_HOME/arkos/config.toml` when that is set).
- Theme files: `~/.config/arkos/themes/<name>.toml`. A file shadows a built-in of the same name.
- Effects: `~/.config/arkos/effects/<name>/`, a folder each. A folder shadows a built-in of the
  same name.
- The daemon's socket: `~/Library/Application Support/arkos/arkosd.sock`.
- The logs, in the same folder: `window.log`, `arkosd.log`, and `updates/journal.log`.
- The binary: inside an Arkos shell, `arkos` is on your PATH and `$ARKOS_BIN` holds its full
  path. When `arkos` is not on your PATH, call it as `"$ARKOS_BIN"`, or by its full path; the
  usual one is `/Applications/Arkos IDE.app/Contents/MacOS/arkos` (quote it: the name has a space).
- This guide as a file: `arkos guide --path` writes it to the Application Support folder above
  and prints the path.
- Your window: `ARKOS_SESSION_ID` also tells `arkos open`, `arkos video` and `arkos panes` which
  window is yours, the one that shows your session, so you never pass a window yourself.

Every theme and settings command that reads or writes files works with no window open and no
daemon running, so you can prepare a theme before Arkos is even started. `arkos open`,
`arkos video` and `arkos panes` act on a window, so they need one (exit 3 without).

## 3. Showing the user a file

Three commands put a file or a page in front of the user and drive it: `arkos open` shows a
video, a markdown file, any text file or a web page in a pane; `arkos video` controls a video tab; `arkos panes` lists
the window's panes so you can choose one. The forms, exactly as `arkos open`, `arkos video` and
`arkos panes` with no arguments print them:

```
arkos open <file | url> [--pane N | --split right|down|left|up] [--at TIME] [--paused | --playing]
                  [--loop A-B | --loop off] [--raw] [--new] [--wait] [--focus] [--window PID] [--json]
arkos video status [<file>] [--json]
arkos video play|pause [<file>] [--json]
arkos video seek <TIME> [<file>] [--json]
arkos video step [+N|-N] [<file>] [--json]
arkos video loop <A-B>|on|off [<file>] [--json]
arkos video reload [<file>] [--json]
arkos video close [<file>] [--json]
arkos video mute|unmute [<file>] [--json]
arkos panes [--all | --window PID] [--json]
```

Every `arkos video` form also takes `--window PID`. A file is a path relative to your working
folder, or absolute; the command makes it absolute and resolves symlinks.

### Which window

`--window PID` aims at that window; `arkos panes --all` lists every window with its pid. Without
it, the window is the one showing your session (from `ARKOS_SESSION_ID`); when your session is
shown in two windows, the one opened last. Outside Arkos (no `ARKOS_SESSION_ID`, a plain Terminal)
the one Arkos window is used, and with several the command exits 1 with
`several windows: pids 81234, 81302; pass --window`. A window found with `--window` is not
your session's, so there is no pane of yours in it: the rules below then anchor on its focused
pane, and `arkos panes` marks no tab `(you)`.

### `arkos open <file | url>`

What opens depends on the file:

- A video (`.mp4`, `.mov`, `.m4v`) opens as a video tab, paused at 0 unless you say otherwise.
- A markdown file opens in the editor, rendered; `--raw` shows its raw text.
- Any other text file opens in the read-only viewer, highlighted.
- A web address (`http://`, `https://`) or an `.html` file opens as a web tab: the page drawn in
  the pane with a bar above it. A local page may read the files beside it (everything under the
  project when it is inside the project, else its own folder), so `out/report.html` can show
  `out/clips/a.mp4`. The page follows its file: an edit reloads it within about two seconds, and
  `arkos open` of the same file or address again reloads it at once (`reloaded: true`). The video
  and markdown flags (`--at`, `--paused`, `--playing`, `--loop`, `--wait`, `--raw`) are refused
  with a page, exit 2. The answer says `opened` when the page has been asked for, not when it has
  loaded: a dev server that compiles on its first request takes as long as it takes; `arkos panes
  --json` and a second `arkos open` report `loading`, `ready` or `failed` with the reason (a server
  that is not up reads "Could not connect to the server").

These are refused before anything is sent, with exit 1 and a line saying what to do instead:

- a missing file: `no such file: {path}` (unless `--wait` on a video, below)
- an image: ``images are not shown in a pane yet; open it with `open {path}` (Preview)``
- audio (`.mp3`, `.m4a`, `.wav`, `.aac`):
  ``audio is not shown in a pane yet; play it with `open -a 'QuickTime Player' {path}` ``
- a `.webm`: ``a .webm is not shown in a pane; show it with `open -R {path}` (Finder)``
- a folder: `{path} is a folder`
- anything else that is not text: `{path} is not a text file; Arkos shows text, markdown and video`

Where the tab goes, the preview pane rule:

- With neither `--pane` nor `--split`, the tab goes to your preview pane. The first open from your
  session splits your own pane to the right and the new pane becomes your preview pane; every
  later open goes into it as a new tab while it exists. If the pane has been closed, the next open
  splits again. The window does not keep it across a relaunch. This gives you a place of your own
  beside you and never covers a pane the user is looking at.
- `--pane N`: into pane N, numbered as Cmd+1 to Cmd+9 number them and as `arkos panes` lists them.
  A pane that does not exist exits 1 with `pane {n} does not exist; this window has {count}`.
- `--split right|down|left|up`: your own pane is split on that side and the tab is the new pane's
  first tab.
- Without a session (outside Arkos, or with `--window`), your pane is the window's focused pane:
  the preview rule splits it to the right the first time and reuses that pane afterwards, and
  `--split` splits it.
- When your session's tab is not in the window (it was closed and is in its minute of grace), a
  preview or a split exits 3 with
  `your session is not shown in this window; pass --pane or --window`. `--pane N` still works.

A file already open in the window is never opened twice: the tab that shows it, of any kind, is
made the active tab of its pane wherever it is, and that rule wins over `--pane` and `--split`.
The one exception is your own pane, where showing it would cover you: a tab on the file there
(the user opened it beside your tab) moves into your preview pane, or into a new pane split off
yours to the right, which becomes your preview pane; a video keeps its position and its play
state through the move.
`--new` forces another tab, placed by the rules above. On a video tab reused this way `--at`,
`--paused`, `--playing` and `--loop` apply; a reused markdown tab switches to raw with `--raw`.

Focus stays with you. An open makes its tab the active one in its pane but never moves the
keyboard focus, not even when it splits a new pane: the user is typing to you, and a jump would
send their next Space to the video. `--focus` moves the focus to the tab, for when the user asks you
to hand it over ("put it in front of me").

The flags:

- `--pane N`: the pane, as above. Not with `--split`.
- `--split right|down|left|up`: split your pane on that side, as above.
- `--at TIME`: seek there once the video has loaded; past the end goes to the last frame. A new
  tab starts at 0 without it; a reused tab keeps its position.
- `--paused` / `--playing`: the state after the open. A new tab is paused without either (the
  user decides when sound starts); a reused tab keeps its state. Not both.
- `--loop A-B`: set the loop range and turn the loop on; `--loop off` clears it. A loop with
  `--playing` shows a scene on repeat.
- `--raw`: a markdown file as raw text instead of rendered.
- `--new`: another tab even when one already shows the file.
- `--wait`: let a video that does not exist yet open; the tab says "waiting for file" and loads it
  once it appears and stops changing. Only for a video; any other missing file is still refused.
- `--focus`: move the keyboard focus to the tab.
- `--window PID`: another window, from `arkos panes --all`.
- `--json`: one JSON value on stdout, below.

The command waits for a video to load (up to about two seconds), so its line carries the length.
A slower load reports `loading`; ask `arkos video status` a moment later. What it prints:

```
opened video ranger-radio-animatic.mp4 in pane 2 (new pane), 0:12.0 of 1:48.5, paused, 1920x1080
shown video ranger-radio-animatic.mp4 in pane 2, 0:12.0 of 1:48.5, playing, loop 0:12.0-0:18.0, 1920x1080
reloaded, shown video ranger-radio-animatic.mp4 in pane 2, 0:12.0 of 1:48.5, paused, 1920x1080
opened notes.md in pane 2, rendered
shown notes.md in pane 3, raw
```

`opened` is a new tab and `shown` a reused one; `(new pane)` says the open split a pane;
`reloaded, ` says this command found a reused video's file changed and loaded the new cut itself;
a tab that had already followed the file on its own (it does within about two seconds of a render
ending) prints `shown` without it. Either way, exit 0 with no `error` and the state `ready` means
the tab shows the file as it is now; `reloaded` only says who loaded it. A video's
words are its position and length, `playing` or `paused`, its loop, `muted` when it is, and its
size; a video that is not ready ends with its state instead (`loading`, `reloading`, `waiting`,
or `unplayable` with the reason in brackets, as in `unplayable (no video track)`), and a tab whose
new file could not be played ends with `new file cannot be played yet`. Any other text file
prints only `opened main.rs in pane 2`.

A video that cannot be played at all ends `unplayable (...)` and the command exits 1. When a
reused video's new file cannot be played (it is truncated, or not a valid file yet), the
old cut stays on screen, the line ends with `new file cannot be played yet` and the command exits
1. It exits 1 for as long as the tab shows a cut older than the file: write a file that plays (the
tab tries again on the next change) or run `arkos video reload`.

### `arkos video`

Which tab: `<file>` names the video tab on that path in the window (every one, when it is open in
two panes). With no file and one video tab in the window, that one. With several, the command
exits 1 and prints one line per tab, as `pane 2: ranger-radio-animatic.mp4`, so you name one;
`status` with no file lists them all instead. No video tab exits 1 with `no video tab open`, and a
file that no tab shows with `no video tab shows {path}`.

- `arkos video status`: the tab's state, changing nothing.
- `arkos video play` and `arkos video pause`. A play at the end starts again from 0.
- `arkos video seek <TIME>`: to that time, in the forms below; past the end goes to the last frame.
- `arkos video step [+N|-N]`: pause, then move N frames, forward or back (default `+1`).
- `arkos video loop <A-B>|on|off`: `A-B` sets the range and turns the loop on, `on` loops the
  whole file, `off` clears the loop.
- `arkos video reload`: load the file now, without waiting for a change, and wait for it to be
  ready or to fail. A failure exits 1 with `new file cannot be played yet` in the line; the old
  cut keeps showing.
- `arkos video close`: close the tab, as the user's close would (Shift+Cmd+T brings it back).
- `arkos video mute` and `arkos video unmute`.

A seek, a step or a loop on a video that is still loading waits up to about two seconds for the
load; if the answer still says `loading` or `reloading`, the seek, step or loop was not applied:
run it again once `status` says `ready`. The same holds for `--at` and `--loop` on an open.
Every command prints the tab's status line after acting, so you see what you changed:

```
ranger-radio-animatic.mp4 in pane 2: 0:12.0 of 1:48.5, paused, loop 0:12.0-0:18.0, 1920x1080, file changed 00:00:04 ago, ready
```

The state word at the end is `ready`, `loading`, `reloading`, `waiting` (the file is missing) or
`unplayable` with the reason in brackets. After a reload that failed it reads
`ready (new file cannot be played yet)`, the old cut still showing, and `status` keeps saying so
(and exits 0) until a later cut loads. `file changed 00:00:04 ago` is the file's age on disk,
hours, minutes and seconds.

A video plays only while it is the active tab of its pane and the window is not hidden or
minimized. When the user switches to another tab it pauses and remembers that it was playing, and
it resumes when they switch back; a play asked for meanwhile reads `paused` (`"playing": false`)
and starts when the tab is shown. The user moving to another app does not pause it.

### `arkos panes`

`arkos panes` prints the window's panes in Cmd+1 order, one line each:

```
pane 1 (focused): terminal claude (you), terminal zsh
pane 2: video ranger-radio-animatic.mp4 (active), markdown notes.md
```

`(you)` is your own tab, `(active)` the tab a pane shows, `(focused)` the pane with the keyboard
focus. A tab's kind is `terminal`, `markdown`, `text`, `browser` or `video`, and its title is the
title on the tab (the program, or the user's name for it), never what a terminal shows; a browser
tab's line carries its page's address after the title.
`--all` prints every window, each headed `window {pid} ({project})`; it cannot be given with
`--window` (exit 2).

### Times

`TIME` is seconds (`12`, `12.5`), `m:ss` (`0:12`), `m:ss.fff` (`1:05.5`) or `h:mm:ss[.fff]`
(`1:02:05`). After a colon the seconds are under 60, and so are the minutes when there are
hours. `A-B` is two times joined by `-`, the first before the second (`0:12-0:18`). A bad one
exits 1 with `not a time: "1:60"` or `the range must start before it ends: "5-2"`. The commands
print times as `m:ss.f`, with hours when the file is an hour or longer.

### Exit codes and JSON

Exit codes: 0 done; 1 refused or failed (a missing or unsupported file, a bad time or range, a pane
that does not exist, no such video tab, several to choose from, several windows, an
`arkos video reload` that failed or found the file unplayable, an `arkos open` whose video is
`unplayable` or whose reused tab still shows an older cut than the file, a window that reported an error, did not answer or is too old for the command); 2 usage
(the usage text is on stderr, ending `see: arkos guide`); 3 no window to aim at
(`no window running`, `session {id} is not shown in a window`, or
`your session is not shown in this window; pass --pane or --window`).

With `--json` the command prints one JSON value on stdout and nothing else, and an error is
`{ "error": text, "exit": n }` there (with the same exit code). Times are seconds.

- `arkos open`: `{ "window": pid, "pane": n, "tab": id, "kind": "video" | "markdown" | "text" |
  "browser", "reused": bool, "reloaded": bool, "web": { "url", "state": "blank" | "loading" |
  "ready" | "failed", "error"? } | null, "video": { "duration": seconds | null, "position": seconds,
  "playing": bool, "loop": [a, b] | null, "width", "height", "state": "ready" | "loading" |
  "reloading" | "waiting" | "unplayable", "error"? } | null }`. `video` is null for a document;
  `width` and `height` are 0 until the video has loaded. `error` is there only with something to
  say: the reason when `state` is `unplayable`, or `new file cannot be played yet` from a reload
  that failed until a later cut loads (`state` stays `ready`: the old cut is still showing).
  `reloaded` is true only when this command loaded the new cut; the tab having followed the file
  on its own leaves it false with the new cut on screen.
- `arkos video`: one tab is the same `video` object with `"window"`, `"pane"`, `"tab"`, `"path"`,
  `"muted"` and `"file_mtime"` (seconds since 1970, or null) added; several (a bare `status`) are
  `{ "window": pid, "videos": [...] }`, one such object each.
- `arkos panes`: `{ "window": pid, "project", "panes": [{ "n", "id", "focused", "tabs": [{ "id",
  "kind", "title", "active", "you"?, "path"?, "session"?, "url"? }] }] }`; `you` is there only on
  your tab, `path` on a document, a video or a local page, `session` on a terminal, `url` on a
  browser tab. `--all --json` is an array of
  these, one per window, with `{ "window": pid, "error" }` for a window that did not answer.

### A video tab follows its file

A video tab watches its file. When the file changes and then stays the same for a second, the tab
reloads it, so a new cut is on screen within about two seconds of the write finishing. The
position is kept (clamped to the new length), a playing video keeps playing and a paused one
stays on its frame; the strip under the picture says "reloading", then "new cut" for three
seconds. A file still being written (ffmpeg writes in place) is never loaded half way. A loop that
starts past the new end is cleared ("loop cleared"). A new file that cannot be played yet leaves
the old cut showing with "new file cannot be played yet", and the next change tries again. A file
that is deleted leaves the last frame dimmed with "waiting for file", and the tab loads it again
when it comes back.

So you never need to reopen a cut. When you want it now, or the tool that writes the file keeps
its modification time, the one word is `arkos video reload`. Running `arkos open` on the same video
again does the same: it reloads a changed file without waiting, then applies `--at`, `--loop` and
`--playing` to the new cut.

The user drives the tab with the strip under the picture (play, the time, a scrubber, loop, mute)
and, once they click the pane, the keys: Space plays or pauses, Left and Right move 5 seconds,
Shift+Left and Shift+Right 1 second, comma and period one frame, Home or 0 go to the start, L
loops, M mutes and R reloads.

### A worked example: the animatic loop

The user is iterating an animatic and wants the radio scene, 0:12 to 0:18:

```
arkos open out/ranger-radio/ranger-radio-animatic.mp4 --at 0:12 --paused   # first cut, the radio scene
python3 tools/cinema/animatic.py tools/cinema/ranger-radio.json              # the next cut
arkos open out/ranger-radio/ranger-radio-animatic.mp4 --at 0:12 --loop 0:12-0:18 --playing   # exit 0, no error: the new cut shows
arkos video status out/ranger-radio/ranger-radio-animatic.mp4 --json
```

A web page works the same way: `arkos open out/report.html` shows a report with its media beside
you, a rewrite of the file reloads it, and `arkos open http://localhost:4173/` shows the dev
server's page (when the server is not up yet the pane says so with a Retry button, and the same
command again once it is up shows the page).

The first `open` splits your pane and shows the cut paused on 0:12; tell the user it is in pane 2
(the number the line printed). The second `open` reuses that tab: it makes sure the tab shows the
new file, jumps to 0:12 and loops the scene with sound. The tab reloads on its own within about
two seconds of the file changing, so when the render finished a few seconds before, the tab has
already loaded the new cut and the second `open` prints `shown` without `reloaded`; when it had
not, the `open` loads it now and says `reloaded`. The confirmation that the new cut is the one on
screen is the second `open` exiting 0 with no `error` (state `ready`), whichever of the two it
printed, and `status` showing no `error`. If the answer has `"error": "new file cannot be played
yet"` (exit 1), the old cut is still showing: tell the user the render did not produce a playable
file.

## 4. The theme file

One TOML file: a top-level `name`, five required tables of `#rrggbb` colours, and an optional
`[font]` table. Every colour key is required; an unknown or missing key is an error.
`arkos theme template --json` prints the full schema as data; the summary:

- `name`: the name `[theme] dark` and `[theme] light` use. `add` and `use` store the file as
  `<name>.toml` whatever the source file is called; a file placed in the themes folder by hand
  should be named `<name>.toml`. Letters, digits, `-` and `_` are the recommended characters.
- `[chrome]`: surfaces `window_bg`, `title_bar`, `status_bar`, `sidebar`, `tab_bar`,
  `tab_active`, `tab_inactive`, `banner`; lines `border`, `split_handle`, `split_handle_hover`;
  the accent `focus_ring`; text `text` and `text_dim`.
- `[terminal]`: `background`, `foreground`, `cursor`, `selection`, then the sixteen ANSI colours
  `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white` and their `bright_` forms.
- `[semantic]`: tab marks `working`, `waiting`, `bell`; backgrounds `error_banner`,
  `find_match`, `find_current`.
- `[tree]`: git status text in the file tree: `added`, `modified`, `deleted`, `untracked`, `ignored`.
- `[viewer]`: syntax colours: `keyword`, `string`, `comment`, `number`, `function`, `type_name`,
  `constant`, `operator`, `punctuation`.
- `[font]` (optional): `prose`, `ui`, `terminal`, each an installed family name; `terminal` must
  be monospaced. A family that is not installed is left out.

The contrast rules, which `arkos theme contrast` checks pair by pair:

- `chrome.text` needs 7:1 on `window_bg` and 4.5:1 on `title_bar`, `banner` and `tab_inactive`.
- `terminal.foreground` needs 11:1 on `terminal.background` and 4.5:1 on `selection`,
  `semantic.find_match` and `semantic.find_current`.
- Every other text or mark colour needs 4.5:1 on the surface it sits on: `text_dim` on
  `window_bg`, `status_bar`, `tab_bar`, `banner`, `sidebar` and `tab_inactive`; the ANSI colours
  and the semantic marks on `terminal.background`; the tree and viewer colours on `window_bg`.
- `terminal.black` is checked only on a light theme, where it is a text colour.

The live reload: every window watches `config.toml` and the file of the theme the config names. A
saved edit applies within a second. A file that stops validating keeps the last good theme and the
window's banner names the file and the error until the next good save. While `config.toml` itself
is broken, a theme file edit waits: it applies once the config loads again.

## 5. The theme and settings commands

Exit codes, for every command: 0 done; 1 refused or failed (a bad file, an unknown name, a low
contrast pair, a window that reported an error, an old daemon); 2 usage (the usage text is on
stderr); 3 a window was needed and none is running. `--json` anywhere prints one JSON value on
stdout and nothing else. Errors are plain lines on stderr. Window lines read
`window {pid} ({project}): {text}`, `window {pid}: no answer` or
`window {pid}: too old for this command`.

- `arkos theme list [--json]`: every theme with its source. JSON
  `[{ "name", "source": "built-in" | "file" | "invalid", "error"?, "path"? }]`.
- `arkos theme validate <file> [--json]`: `ok: {name}`, or the error and exit 1. JSON
  `{ "ok": true, "name" }` or `{ "ok": false, "error" }`.
- `arkos theme template [<built-in>] [--json]`: a built-in's TOML (default `navy`); an unknown
  built-in exits 1. `--json` gives the schema instead:
  `{ "tables": [{ "name", "keys": [{ "key", "meaning", "contrast_with"?, "needs"? }] }] }`.
- `arkos theme contrast <file-or-name> [--json]`: one line per pair, `{what} {ratio}:1 ok` with
  the ratio to one decimal, or `{what} {ratio}:1 LOW (needs {needs})` with the ratio cut down to two
  decimals, as in `tree.ignored on window_bg 4.46:1 LOW (needs 4.5)`; exit 1 when any pair is low. An argument ending in
  `.toml` is a file. JSON `{ "name", "ok", "pairs": [{ "what", "fg", "bg", "ratio", "needs", "ok" }] }`.
- `arkos theme add <file> [--replace] [--json]`: copies the file into the themes folder:
  `added {name}` or `replaced {name}`; a name taken by a file or a built-in prints
  `exists: {name}; pass --replace` and exits 1. JSON `{ "name", "replaced" }`.
- `arkos theme use <name-or-file> [--dark|--light] [--replace] [--json]`: a `.toml` path is added
  first (`exists: {name}; pass --replace`, exit 1, when that name is already in the themes
  folder: running `use draft.toml` again after more edits needs `--replace`); the name must load (else `unknown theme: {name}` or the file's error, exit 1, nothing
  written); then `[theme] dark` or `light` is written (the key from the flag, else from
  `[appearance] mode`, else from the window's system appearance, else dark) and every window
  reloads. Lines: `window {pid} ({project}): {name} applied`; a window whose active key is the
  other one prints `window {pid} ({project}): saved for {key} mode; shows {theme} in {active} mode`
  (exit 0 unless it reports a theme or config error, which is printed instead and exits 1); a
  window that shows another theme in the written key's mode prints `shows {other}` and exits 1.
  No window: `saved; no window running`, exit 0. No window answered (all silent or too old):
  `saved; no window answered`, then their `no answer` / `too old for this command` lines, exit 0.
  `use` refuses a `config.toml` that does not load. JSON
  `{ "name", "key", "saved": true, "windows": [{ "pid", "project", "theme"?, "error"? }], "missing": [pids], "too_old": [pids] }`.
- `arkos theme preview <name-or-file> [--json]` and `arkos theme preview --end [--json]`: shows a
  theme in every window without writing anything; the TOML is validated first. Lines
  `window {pid} ({project}): previewing {name}` and `window {pid} ({project}): preview ended, {theme}`.
  Exit 3 with no window, 1 when none answered. JSON `{ "windows", "missing", "too_old" }`. It ends at
  `--end`, at the next config or theme reload, when the system appearance changes, or when the
  window closes. Ending one while the
  configured theme's file is broken keeps the preview's colours on screen with a theme error set.
- `arkos theme current [--json]`: `config: dark {d}, light {l}, mode {m}`, then per window
  `window {pid} ({project}): {theme} ({dark|light}, system is {dark|light}), preset {preset}, family {family}`,
  plus `, preview {name}`, `, config error: {text}` and `, theme error: {text}` when set. Exit 0
  with no window. JSON `{ "config": { "dark", "light", "mode" } | { "error" }, "windows": [...], "missing", "too_old" }`,
  each window `{ "pid", "project", "theme", "dark", "system_is_dark", "preset", "family", "preview", "config_error", "theme_error" }`.
- `arkos settings get [<key>] [--json]`: no key prints the config file verbatim, then the window
  lines as `current` does (JSON `{ "file", "windows" }`). A dotted key prints its value bare
  (JSON `{ "key", "value" }`). It reads the file only, so a key left at its default answers
  `no such key` (exit 1).
- `arkos settings set <key> <value> [--json]`: the keys are `appearance.mode`
  (system, dark, light), `theme.dark`, `theme.light`, `font.family`, `font.preset`,
  `font.presets.<name>.size` (8 to 48), `font.presets.<name>.line_height` (1.0 to 2.5),
  `effect.name` (theme, none, or an installed effect's name), `effect.intensity` (0.0 to 1.0),
  `viewer.wide_tables` (wrap, scroll), `update.auto` (true, false),
  `recovery.start_at_login` (ask, on, off), `telemetry.usage` (off, local, on) and
  `telemetry.crashes` (never, ask, always). Another key exits 2
  with the list. A bad value is refused before anything is written. Then every window reloads:
  `window {pid} ({project}): {key} applied` or its config error (exit 1), or, for
  `appearance.mode`, `theme.dark` and `theme.light`, its theme error (exit 1); `saved; no window running`
  and `saved; no window answered` as for `use`. Unlike `use`, it repairs a broken `config.toml`
  when the edit makes it load. JSON `{ "key", "value", "saved", "windows", "missing", "too_old" }`.
- `arkos config reload [--json]`: asks every window to read its config now:
  `window {pid} ({project}): reloaded, {theme}` or its errors. Exit 3: no window; 1: none answered.
  A reload, from this command or from `use` and `settings set`, recreates a missing `config.toml`
  with the defaults (the file watcher does the same); that is the one case where a window writes a
  file on this channel.
- `arkos guide [--path | --install]`: prints this guide; `--path` writes it to a file and prints
  the path; `--install` installs the skill into your Claude config folder (or prints
  `already current: {path}` and writes nothing when it is up to date) and prints a line to paste
  into `AGENTS.md` or `CLAUDE.md`:
  ``Arkos IDE: run `arkos guide` before showing the user a file or a web page in a pane or changing its theme or settings.``

## 6. The theme workflow

1. `arkos theme template > draft.toml` (or `arkos theme template peach-light` for a light start).
2. Set `name` to a new bare name and edit the colours.
3. `arkos theme validate draft.toml`, then `arkos theme contrast draft.toml`; fix every LOW pair
   and run it again until it exits 0.
4. `arkos theme preview draft.toml`, so the user sees it; then `arkos theme preview --end` if they
   do not want it.
5. `arkos theme use draft.toml` (add `--dark` or `--light` to choose the slot).
6. `arkos theme current` to confirm what every window shows.

To revert, `arkos theme use navy` (or `arkos theme use peach-light --light`). The user can also
pick any theme by hand in the Settings window.

## 7. A worked example: Timo Purple

The brief was an accent, `#00ffff`, and a background, `#330076`. The built-in `timo-purple` is
the result (`arkos theme template timo-purple` prints it). The choices, and why each passes:

- `window_bg` and `terminal.background` are the background `#330076`. The bars, sidebar and
  inactive tabs are a step darker, `#2a0062`, and `banner` a step lighter, `#3b1a86`, so the
  layout reads without borders.
- `chrome.text` and `terminal.foreground` are a near-white with a violet tint, `#f3edff`: 13.1:1
  on the background, clearing both 7:1 and 11:1, and 11.0:1 on the banner.
- `chrome.text_dim` is `#c4b6ea`: 8.0:1 on the background and 6.7:1 on the banner, its hardest
  surface.
- The accent goes where a mark is wanted: `focus_ring`, `cursor` and the ANSI `cyan` are
  `#00ffff`, 11.9:1 on the background.
- The ANSI colours are pastels, since saturated reds and blues sink into a deep purple: red
  `#ff8a9e` 6.7:1, blue `#8fb8ff` 7.5:1; `bright_black` `#9a86d0` sits close to the line at 4.8:1.
- `tree.ignored` and `viewer.comment` are the dimmest text, `#a08ed6`, at 5.2:1.
- `selection` `#1a6b45`, `find_match` `#15553a` and `find_current` `#1f7a52` are greens dark
  enough for the foreground to keep 5.7:1, 7.7:1 and 4.6:1 on them; the accent was too light to
  carry white text, so it is not used as a background.

## 8. Effects

An effect is a pair of Metal shaders that paint the project window: a wash drawn under every
surface, and optionally the text itself. Two are built in: Peach Light recommends `watercolor`
(still ink glyphs) and Navy recommends `space` (a night sky with glowing edges and still holo
text); the user may pick any effect for any theme, or none, in
Settings or through `arkos effect use`. An effect reads the theme's colours on every frame, so
write it to suit any theme rather than one.

What an effect can touch:

- The wash under everything: one fragment shader drawn under the whole window, handed the
  theme's colours and the blocks of the UI (the title bar, the sidebar and its divider, each
  pane's tab bar and body, each video's picture as fitted in its tab, each markdown table and
  its header row as far as they are seen, and, when the manifest asks for them, the Explorer's selected and hovered rows and the hovered tab with their fades).
  A video's picture (`ARKOS_MEDIA_BLOCK`) is opaque over the wash, so only what the wash draws
  outside it shows; with `ink_borders` it comes with all four edges, its rectangle a point and a
  half outside the picture so the outline is seen whole. A table (`ARKOS_TABLE_BLOCK`) has
  clear fills under an effect and comes with all four edges; its header
  (`ARKOS_TABLE_HEADER_BLOCK`) with the edge under it. The app draws the table's inner lines
  itself, in ink.
- The text, through the glyph shader: it replaces gpui's own stages for the project window's
  glyphs.
- The alphas of the app's surfaces over the wash: the paper of terminals and documents, the bars
  and the sidebar, the active tab. The window's background and the pane bodies go clear, so the
  wash shows wherever nothing is painted over it.
- The borders: with `ink_borders` the app stops drawing its 1 px borders and tells the wash which
  edges of each block it no longer draws.
- The UI text size, a few points larger.
- The text contrast: how far the text colours move towards ink while the effect is on.

What it cannot touch: anything else gpui draws (icons, the cursor, selections, images, the
terminal's cells one by one), the About, Settings and Keybindings windows and the launch chooser
(they keep the plain look), and the layout (no size or position changes).

### The folder

An effect is a folder in `~/.config/arkos/effects/` (`$XDG_CONFIG_HOME/arkos/effects/` when that
is set), named for the effect: `effect.toml`, the wash shader and an optional glyph shader beside
it. A folder named like a built-in replaces it. Every key but `name` and `wash` has the default
shown; a value out of range or an unknown key makes the effect invalid, and the error names the
key and the range.

| Key | Range | Default | What it does |
| --- | --- | --- | --- |
| `name` | the folder's name | required | A manifest that names another folder is invalid: `name 'bar' does not match its folder 'foo'`. |
| `description` | text | `""` | One sentence for the user. |
| `wash` | a file beside `effect.toml` | required | The wash shader, usually `wash.metal`. |
| `glyphs` | a file beside `effect.toml` | absent | The glyph shader; absent, gpui draws text as always. |
| `frame_rate` | 1 to 60 | 30 | The wash's frames per second while anything moves; sixty while a highlight fades. |
| `scale` | 0.1 to 1.0 | 0.5 | The wash's pixels per window pixel, each side; the drawable is stretched to the window. |
| `ui_text_bump` | 0 to 4 | 0 | Points added to the project window's UI text. |
| `ink_borders` | true, false | false | The app hides its own 1 px borders and the wash draws the edges. |
| `highlights` | `"effect"`, `"app"` | `"app"` | `"effect"`: the wash paints the selected and hovered rows and the hovered tab from the highlight blocks, and the app stops filling them. `"app"`: the app keeps its own fills and the wash gets no highlight blocks. |
| `menus` | `"ink"`, `"glow"`, `"app"` | `"app"` | How the menus and popovers look, since the wash, painted under the window, cannot reach a menu over the panes. `"ink"`: a card of the window's paper, a line in the text's colour, the hovered row in the thinned focus colour (watercolor's). `"glow"`: a card of the window's ground with a glowing line and the hovered row lit in the focus ring's colour (space's). `"app"`: the app's own. |
| `glyph_motion` | `true`, `false` | `false` | `true`: the glyph shader moves with time, so the app redraws the text up to 20 times a second while the effect is on and the window is seen (never under Reduce Motion), and the shader reads the seconds at Metal buffer index 5. `false`: the text is drawn only when something changes. |
| `paper_alpha` | 0.5 to 1.0 | 0.5 | A terminal's or a document's paper over the wash. It never goes below 0.5, so text keeps its ground. |
| `chrome_alpha` | 0.0 to 1.0 | 0.0 | The title bar, the tab bars, the inactive tabs and the sidebar over the wash. |
| `active_tab_alpha` | 0.0 to 1.0 | 0.55 | The active tab over the wash. |
| `text_contrast` | 0.0 to 1.0 | 0.0 | How far the text moves towards ink (black on a light theme, white on a dark one): `chrome.text` that far, `chrome.text_dim` three quarters and `terminal.foreground` nine tenths of the way. |

Changing effects fades the wash and the surfaces' alphas in and out over 200 ms (it cuts under
Reduce Motion); the glyph shader, the text size, the borders, the highlights, the menus and the text
contrast switch at the end of a fade in and at the start of a fade out.

### The contract

`arkos effect contract` prints the whole contract for the wash, `arkos.metal`, which is put
before your wash file when it is compiled. It declares the theme's colours by index
(`ARKOS_WINDOW_BG` to the sixteen ANSI colours from `ARKOS_ANSI`), the block kinds
(`ARKOS_TITLE_BAR_BLOCK` and the rest), the edge bits (`ARKOS_EDGE_TOP` and the rest),
`ArkosBlock`, `ArkosInputs` and `ArkosVertex`. Your wash supplies one function:

```metal
fragment float4 arkos_wash(ArkosVertex in [[stage_in]], constant ArkosInputs& u [[buffer(0)]]);
```

What it is handed: `u.time`, seconds since the effect started, frozen under Reduce Motion (the
wash then draws only when something changes); `u.intensity`, the user's slider from 0 to 1;
`u.size`, the window in points; `u.scale`, the drawable's pixels per point; `u.colors`, the
theme's colours, opaque; and `u.blocks`, the first `u.block_count` of up to 48 blocks, each a
rectangle in window points from the top left with its kind, the edges the app no longer draws, a
`seed` fixed for the block's life, a `value` (a highlight's fade from 0 to 1, else 1) and an
`ordinal` (a pane's place; a highlight's state, 0 hovered, 1 selected, 2 both). `in.uv` runs
from (0, 0) at the top left to (1, 1) at the bottom right. What the wash returns is the window's
opaque ground; the app's surfaces are drawn over it with the manifest's alphas. Derive every
shade from the theme's colours, so the effect reads right in a dark theme as in a light one.
Strengths are yours: a highlight block says its kind, fade and state, and the wash decides how
strongly to paint it.

A wash may also define a menu stage, `fragment float4 arkos_overlay(ArkosVertex in [[stage_in]],
constant ArkosInputs& u [[buffer(0)]])`. The wash is drawn under the window, so it cannot reach
a menu or popover open over the panes; the menu stage is drawn in a see-through layer above them
while one is open, so the menus' edges can be drawn as the panes' are, and the app then draws no
line of its own around them. Its blocks are only the open menus (`ARKOS_MENU_BLOCK`, all four
edges, `value` the menu's fade). It returns colour multiplied by its alpha and transparent black
wherever it draws nothing; it is drawn over the menus' text, so draw only at their edges. The
built-ins share their edge functions between `arkos_wash` and `arkos_overlay`, so a menu's line
is the panes' own. `arkos effect check` builds it when the wash defines it.

`arkos effect contract --glyphs` prints gpui's sprite declarations, which your glyph shader is
compiled after (gpui's whole shader source comes first, so it may use any of them): its glyph
stages, then its colour sprite stages. Then one line naming the two functions it supplies,
`arkos_glyph_vertex` and `arkos_glyph_fragment`, with their signatures; one naming the emoji
stages it may add, `arkos_emoji_fragment` and, to place emoji itself, `arkos_emoji_vertex`
(without that, gpui's `polychrome_sprite_vertex` places them); and a last line naming the
window's glyph clock: any of these functions may declare `constant float *time [[buffer(5)]]`
and read its seconds in `*time`. A glyphs file that defines `arkos_emoji_fragment` draws the
window's emoji with it; pictures are never drawn by it, and a file without one leaves emoji as
gpui draws them. Between the declarations
and the signatures it prints `ARKOS_SCALE`, the window's pixels per point (2 on a Retina screen,
1 on most others), which the window declares ahead of your source and rebuilds when it moves to
a screen of another scale. gpui hands a glyph shader device pixels, so measure
your ink in points through it, or a 1x screen sees every bleed and wobble twice as wide. The
built-in's `glyphs.metal` (from `arkos effect template`) is a worked one: it widens each glyph's
quad so the ink can bleed, measures everything in points, and keeps a stroke's core at least 55%
of its coverage so thin text stays legible. Keep to that floor in any glyph shader you write.
Its emoji stages, at the end of the file, give emoji the same hand while keeping their colours
and at least 80% of their coverage.

### The steps

1. `arkos effect template --as <name>` copies the built-in `watercolor` into the effects folder
   under the new name and prints the folder. Start from it even for a different look: it shows
   the noise fields, the blocks, the highlights and the glyphs at work.
2. Edit `effect.toml` and the shaders.
3. `arkos effect check <folder>` until it prints `ok: <name>`; a compile error names your file
   and line. Read its `highlights:` line: with `app` the wash gets no highlight blocks.
4. `arkos effect bench <name>` until it says `within budget`.
5. `arkos effect use <name>`, then `arkos effect current` to see what each window draws and what
   it costs.

A window loads an effect when it puts it on. To see an edit to the effect it already shows, run
`arkos effect use none`, wait a second for the fade, then `arkos effect use <name>` again. To
revert, `arkos effect use theme` (the theme's choice) or `arkos effect use none`. The window log
(`window.log`) says `effect {name}: {ms} ms per frame at {w}x{h}` once a minute while the cost
changes by more than a tenth. It can read higher than `bench` on a GPU with little else to do,
which slows its clock between the wash's frames; `bench` measures the work at the working clock.

### The budget

The costs, measured on a MacBook Air at 1200 by 800 points, Retina:

| Technique | GPU per frame | Of a 30 fps frame |
| --- | --- | --- |
| Per-pixel noise evaluated once per block (22 blocks, 3-octave fbm each), quarter scale | 8.5 ms | 25% |
| The same with the noise fields shared by every block, quarter scale | 0.64 ms | 2% |
| Shared fields plus ink outlines, half scale (the outlines need it) | 3.6 ms | 11% |
| Ink glyphs on top (gpui's own pass; only when text changes) | about 0.1 ms per repaint | |

The budget: stay under 4 ms per frame at half scale; `bench` says `within budget`, `heavy` from
4 to 8, `too heavy` above 8. Rules of thumb:

- Compute a noise field once per pixel and read it per block, never per block per pixel.
- Draw at the lowest `scale` the look allows (the ink needs 0.5, a soft wash 0.25).
- Keep `frame_rate` at 30 unless something fast moves.
- Place anything that moves on the CPU side of the contract when it is per block, and in the
  shader from `time` when it is per pixel.
- Pick every block's grain from `seed`, never from its index, or a highlight coming and going
  will reshape the panes.

Nothing turns a slow effect off on its own: the cost is reported and the choice is the user's.
Tell them what `bench` said.

### The effect commands

Exit codes, `--json` and the window lines are as in section 5. Every command works with no
window open; `bench` and `current` ask the running windows, and `use` asks them to reload.

- `arkos effect list [--json]`: every effect, the built-ins first, then the folders sorted:
  `{name} (built in)`, `{name} (folder)` or `{name} (folder, invalid: {error})`. JSON
  `[{ "name", "source": "built-in" | "folder" | "invalid", "path"?, "error"? }]`.
- `arkos effect contract [--glyphs]`: the wash's contract, or with `--glyphs` gpui's sprite
  declarations and the line naming the glyph shader's two functions. No JSON.
- `arkos effect template --as <new-name> [<built-in>]`: writes a copy of the built-in (default
  `watercolor`) to the effects folder as `<new-name>`, with the manifest's `name` set to it and
  its comments kept, and prints the folder's path. `exists: {path}` when that folder is there
  already, `unknown built-in effect: {name}`, or a name that is not a bare folder name: exit 1
  and nothing written. No JSON.
- `arkos effect check <folder> [--json]`: reads the manifest and the files it names, then
  compiles the wash against the contract and the glyph shader after gpui's source, as a window
  would. `ok: {name}` and `highlights: effect` or `highlights: app`, exit 0; or the error, exit 1.
  A compiler error reads `{file}:{line}:{column}: error: {text}` with the line in your own file
  (a note inside the contract reads `arkos.metal:{line}`, inside gpui's source `gpui:{line}`).
  JSON `{ "ok": true, "name", "highlights": "effect" | "app" }` or `{ "ok": false, "error" }`.
- `arkos effect bench <folder-or-name> [--seconds N] [--json]`: an argument with a `/` is a
  folder, anything else an effect's name. Draws the wash off-screen with the window's own
  pipeline at the manifest's `scale`, on a Retina target, for a second's warm-up and then N
  seconds (above 0, at most 60; default 3), frames back to back so the GPU stays at its working
  clock, and times every frame on the GPU. It draws a window as it opens: the
  title bar, the sidebar and one pane, with a selected Explorer row when the manifest says
  `highlights = "effect"`; every pane or highlight more adds its blocks' cost. The size and the theme are those of
  the first window that answers; without one, 1200 by 800 points and Peach Light. Prints
  `{name}: {ms} ms per frame at {w}x{h} ({share}% of a frame at {rate} fps): {verdict}`, the
  verdict `within budget`, `heavy` or `too heavy`. Exit 1 when the effect does not load or
  compile. JSON `{ "name", "ms", "size": [w, h], "theme", "frame_rate", "share", "verdict" }`,
  `share` in percent.
- `arkos effect use <name|none|theme> [--json]`: an effect's name must load (else its error,
  exit 1, nothing written); then it is `arkos settings set effect.name <value>`, with its lines,
  exit codes and JSON `{ "key": "effect.name", "value", "saved", "windows", "missing", "too_old" }`:
  `window {pid} ({project}): effect.name applied`, or `saved; no window running`.
- `arkos effect current [--json]`: `config: effect {theme|none|name}, intensity {i}`, then per
  window `window {pid} ({project}): {name}, {ms} ms per frame`, `{name}, not measured yet` (the
  cost is read every sixtieth frame), `none`, or `too old to report its effect`, then the
  `no answer` and `too old for this command` lines. Exit 0 with no window; 1 when the daemon is
  older than this command. JSON `{ "config": { "name", "intensity" } | { "error" }, "windows":
  [{ "pid", "project", "effect": name | null, "gpu_ms": ms | null, "size": [w, h] | null } |
  { "pid", "project", "error" }], "missing": [pids], "too_old": [pids] }`.

## 9. What Arkos sends

When the user asks what Arkos sends, whether it collects data, or how to turn that off, answer
from the disclosure page and not from memory. It is `docs/telemetry.md` in the Arkos source
(<https://github.com/csarkosh/ide-arkos/blob/main/docs/telemetry.md>); the app writes the copy
it ships to `~/Library/Application Support/arkos/telemetry/what-arkos-sends.md` when the user
opens Help > What Arkos Sends. `arkos telemetry status` prints the mode in force and why, and
`arkos telemetry view` shows what is waiting to be sent.

Never change the setting on your own: not with `arkos telemetry`, not with
`arkos settings set telemetry.usage` or `telemetry.crashes`, and not by editing the `[telemetry]`
table. Whether Arkos records or sends anything is the user's choice. Tell them the command and
let them run it, or run it only when they have asked you to in so many words.

## 10. Rules

For files in panes:

- Open a file in a pane when the user asks to see, watch or review it, and offer it when you have
  just made one.
- Prefer the preview pane, and name the pane number to the user.
- Never open the same file twice: `open` reuses its tab for you.
- Leave the tab open when you finish; the user closes it.
- Pause before you talk about a frame.
- Do not open files the user did not ask about.

For themes and settings:

- Validate before you preview, and preview before you use.
- Never leave a preview running: end it, or make it permanent with `use`.
- Never write `config.toml` by hand when a command exists; never edit the app bundle.
- Never turn usage or crash reports on or off unless the user asked you to.
- Prefer `--json` when you parse output, and branch on the exit code.
- When you finish, run `arkos theme current` and report its lines to the user.

For effects:

- Check, then bench, then use; never put on an effect that `check` refuses.
- Tell the user what `bench` said, in milliseconds and its verdict, before you put the effect on,
  and say so plainly when it is `heavy` or `too heavy`.
- Keep the text readable: leave `paper_alpha` at 0.5 or more under text, and keep a glyph's core
  at least 55% of its coverage.
- Never write into a built-in's place unless the user asked to replace it: template under a new
  name.
- When you finish, run `arkos effect current` and report its lines to the user.
