# Arkos IDE: the video pane and `arkos open` (milestone 9)

**Date:** 2026-10-20
**Status:** draft for Cyrus's review
**Written against:** main at the 0.5.3 release (597aec6)
**Builds on:** the agent theme interface (`2026-10-16-agent-theme-interface-design.md`: the
`arkos` command line, the daemon's `Customize` relay, the guide and the skill), the viewer and
markdown editor (`2026-10-15-markdown-editing-design.md`: the fingerprint poll and the disk
conflict banner), the media helpers in `crates/arkos/src/media.rs` (Quick Look posters, the file
type check before an open).
**Rulings (Cyrus, 2026-10-20):** the video is drawn by Arkos itself, in process, never a docked
window ("I hate the docking approach"); agents get playback controls, not just open; an agent
must be able to reload the video in place and jump to the scene being worked on, because video
generation is iterated many times; `arkos open` also opens markdown, since the editor exists.

## 1. Why this exists

Cyrus generates video with coding agents: a shot list in JSON becomes stills, then an animatic
cut to time, then a better cut, many times an hour. Today the agent that finishes a cut runs
`open <file>.mp4`, which hands the file to QuickTime Player or whatever macOS has as the default
player: a separate window, in front of the IDE, that knows nothing about the next cut. Each
iteration means finding that window, closing it, and opening the new file, then scrubbing back to
the scene under discussion. The agent cannot help with any of it.

After this milestone a video is a tab in Arkos like a terminal or a document: it lives in a pane,
moves between panes, closes, reorders, renames, splits and survives a relaunch. An agent running
in an Arkos terminal can put a video in the pane beside itself with one command, and when the next
cut lands at the same path the tab reloads on its own, holding the position. The agent can also
say "jump to 0:12 and loop the scene", and can see from the command's output that the new cut is
the one on screen. The same command opens a markdown document in a pane, since the editor is
already there.

The test of the design: a fresh Claude Code session inside Arkos, asked "show me the animatic
beside you and stop on the radio scene", finds the command without being told, runs
`arkos open tools/cinema/out/ranger-radio/ranger-radio-animatic.mp4 --at 0:12 --paused`, and the
video appears in a pane next to the session, paused on that frame. The agent re-renders the file,
the tab reloads within two seconds, and the frame on screen is from the new cut.

## 2. What ships

1. **A video tab** (section 3): `Tab::Video`, drawn in process with AVFoundation decoding and
   gpui's `surface` element painting each frame; a control strip with play, pause, scrubbing, a
   loop range, mute and frame stepping; keyboard control while the pane has focus.
2. **The tab follows its file** (section 4): a change on disk reloads the video in place, keeping
   the position and whether it was playing; a file that vanishes and comes back is picked up.
3. **`arkos open`** (section 5): opens a video or a markdown (or any text) file in a pane of the
   window the agent runs in, by default a preview pane beside the agent's own, with `--at`,
   `--paused`, `--loop`, `--pane` and `--split`.
4. **`arkos video`** (section 6): `status`, `play`, `pause`, `seek`, `loop`, `step`, `reload`,
   `close`, aimed at a video tab by its path.
5. **`arkos panes`** (section 7): the window's panes in Cmd+1 order with their tabs, so an agent
   can choose a frame.
6. **A targeted channel through the daemon** (section 8): one window, found from the agent's
   `ARKOS_SESSION_ID`, rather than every window as `Customize` does.
7. **The window's own ways in** (section 9): a video in the sidebar opens the tab on a double
   click; Cmd+click on a poster in the markdown editor opens the tab beside the document.
8. **Discovery** (section 10): the hook line, the skill description, `arkos --help`, the guide
   and the doctor all name the new verb.
9. **Docs, telemetry rows and tests** (sections 11 to 13).

Out of scope, listed so nobody builds it by accident: remote URLs (the browser pane shows those;
the guide says so); audio files (they keep opening in QuickTime Player as today); an image tab
(`arkos open x.png` is refused with a line saying images are not shown yet; cheap to add later);
subtitles, chapters, playback rate, a volume slider (mute is enough beside the system volume);
agents writing frames out as PNGs (ffmpeg does that); and replacing the Chrome docking with an in
process web view, which Cyrus wants later and which this milestone's `Tab::Video` is the template
for (section 14).

## 3. The video tab

### 3.1 The model

`Tab::Video { path: PathBuf, #[serde(default)] at: Option<f64>, #[serde(default)] loop_range:
Option<(f64, f64)> }` in `tree.rs`, appended after `Browser`. `path` is absolute and canonical
(symlinks resolved at open time, as `media::checked_open_path` does for the poster's Cmd+click).
`at` is the position in seconds when the workspace was last saved, so a relaunch comes back on
the same frame, paused; `loop_range` is the loop when one is set. Neither is written on every
frame: `Root::save` clones the tree as it is, so the view pushes them into its `Tab::Video` entry
with a `Tree::set_video_state` setter (as the viewer's `mode` goes through `tree.set_viewer_mode`)
when the tab pauses, seeks by a command or the scrubber, sets or clears a loop, or is about to be
moved or closed, and `save` runs then. The closed stack gains `Closed::Video { path, at, loop_range }` and reopens
the tab paused at `at`.

A `Tab::Viewer` whose path is a video (`media::kind` says `Video`) is converted to `Tab::Video`
when the workspace loads, so an old workspace file that had an `.mp4` open as "binary" comes up as
a video. No format version bump: the new variant is appended and every field has a default.

### 3.2 Rendering

One `VideoView` entity per tab (as `Viewer` is per viewer tab), holding:

- An `AVPlayer` with an `AVPlayerItem` on an `AVURLAsset` of the file, `actionAtItemEnd` set to
  pause (or to seek to the loop start when a loop is set; section 3.4). AVFoundation plays the
  audio itself to the default output device; `muted` is the player's.
- An `AVPlayerItemVideoOutput` attached to the item, created with the pixel buffer attributes
  `kCVPixelBufferPixelFormatTypeKey = kCVPixelFormatType_420YpCbCr8BiPlanarFullRange` (`'420f'`),
  `kCVPixelBufferIOSurfacePropertiesKey = {}` and `kCVPixelBufferMetalCompatibilityKey = true`.
  gpui 0.2.2's Metal renderer asserts exactly that pixel format for a surface and reads the two
  planes through a `CVMetalTextureCache` (`metal_renderer.rs`, `draw_surfaces`), so the output
  does the conversion and Arkos never touches pixels.
- The last pixel buffer copied, as `core_video::pixel_buffer::CVPixelBuffer` (the type gpui's
  `surface()` takes). The output hands the buffer back under the copy rule, as the objc2
  binding's `Retained`; Arkos wraps its raw `CVPixelBufferRef` under the get rule (a reference
  of its own) and then drops the `Retained`, so the kept buffer has exactly one owner.

Each frame: while the player's rate is not zero, the view asks for an animation frame
(`window.request_animation_frame()`), and on paint it calls `hasNewPixelBufferForItemTime` with
the output's `itemTimeForHostTime(now)` and, when true, `copyPixelBufferForItemTime` to replace
the kept buffer. Paused, nothing is requested; a seek copies one frame for the new time (the
output is asked once more after `seekToTime` completes, through its completion handler posting
back to the UI thread with `cx.spawn` / a channel). The element is `surface(buffer)` with
`ObjectFit::Contain`, centred in the pane on `chrome.window_bg`, so letterboxing is the window's
colour. Before the first frame the pane shows the file's Quick Look poster when the media module
already has one, else "loading", over the same background.

Playing is bounded by the display's refresh through gpui's animation frames; a 24 fps file
repaints only when a new buffer exists, and a hidden or inactive tab (section 3.5) requests
nothing. App Nap is already off for the whole process (`app_nap.rs` holds one lifetime token with
`UserInitiatedAllowingIdleSystemSleep`), but that token does not stop the display from sleeping,
and a person watching a two minute cut without touching the keyboard must not lose the screen.
`app_nap.rs` gains a second, scoped token (`NSActivityIdleDisplaySleepDisabled`, what QuickTime
Player holds while it plays) that `VideoView` begins when its rate becomes non zero and ends when
it pauses, ends, is made inactive or is dropped; at most one such token is held per window at a
time, counted across views.

The pixel buffer attributes above are load bearing, not a nicety: gpui's
`create_texture_from_image(...).unwrap()` panics the window on a buffer that is not IOSurface
backed and Metal compatible. Two AVFoundation behaviours the builder budgets for: the video
output suspends itself after about a second without `hasNewPixelBufferForItemTime` calls, so a
paused view asks `requestNotificationOfMediaDataChangeWithAdvanceInterval` before the one frame
copy that follows a seek, a step or a reload; and `seekToTime_completionHandler` needs the
`block2` feature of the objc2 crates.

The crates: `objc2-av-foundation = "0.3.2"` (features `AVPlayer`, `AVPlayerItem`,
`AVPlayerItemOutput`, `AVAsset`, `AVAsynchronousKeyValueLoading`, `AVPlayerItemTrack` as the
builder finds needed), `objc2-core-media = "0.3.2"` (`CMTime`) and `objc2-core-video = "0.3.2"`
(the `CVPixelBuffer` reference type the output returns), all already at the objc2 0.3.2 line the
tree uses; and `core-video = "0.4.3"`, gpui's own CoreVideo crate, for the `surface()` type. All
behind `#[cfg(target_os = "macos")]` as `ax.rs` is; `cargo test` never creates a player (section
13).

### 3.3 The control strip

Below the picture, 28 px, always visible (no auto hiding: a pane can be short and a hidden strip
is a hunt), on `chrome.tab_bar` with `chrome.text` and `chrome.text_dim`:

- Play/pause (one button, the glyph swaps).
- Current time and duration as `m:ss.f / m:ss.f` (hours when the file is over an hour).
- The scrubber: a 4 px track in `chrome.border`, the played part in `chrome.text`, the loop range
  (when set) as a span in `chrome.focus_ring` at 40 percent behind it, a 10 px knob at the
  position. A click seeks; a drag seeks continuously (the output is asked for a frame on each
  change, so scrubbing shows frames; the player is paused for the drag and resumes after it when
  it was playing).
- Loop toggle (on when a range is set; off clears the range; on without a range loops the whole
  file).
- Mute toggle.
- A status word at the right in `text_dim`: nothing in the steady state; "reloading" while a new
  file is being loaded; "new cut" for three seconds after a reload (so the eye catches it);
  "waiting for file" while the file is missing (section 4); the reason when the file cannot be
  played ("no video track", "cannot play: <AVFoundation's localized description>").

Over the picture, centred, when the file cannot be played: the reason and a button "Open in
QuickTime Player" (`media::quicktime_args`), the same fallback the markdown poster has today.

The tab's title is the file name; the tab bar is the full height one (the slim bar stays for
browser tabs only); no tab mark. The pane number shows as for any pane, so the agent's
`--pane N` matches what the user sees.

### 3.4 Keys and the palette

With the video pane focused, in `keymap.rs` as actions with these defaults (all rebindable; the
Keybindings window lists them under a "Video" heading):

| Action | Key | Does |
|---|---|---|
| `video play pause` | Space | toggles |
| `video back` / `video forward` | Left / Right | 5 s |
| `video back fine` / `video forward fine` | Shift+Left / Shift+Right | 1 s |
| `video step back` / `video step forward` | , / . | one frame (`AVPlayerItem.stepByCount`), pausing first |
| `video start` | Home (and 0) | to 0 |
| `video loop` | L | loop toggle; with a selection on the scrubber (Shift+drag) sets that range |
| `video mute` | M | mute toggle |
| `video reload` | R | reload now (section 4) |

Space, the arrows and the letters are consumed only while a video tab has focus; terminals never
see this map. The palette gains the same entries plus "Open file in pane…", which asks for a path
(the browser tab's URL prompt reused) and opens it as `arkos open` would into the focused pane.

### 3.5 Pane behaviour

Everything a tab does today works unchanged because the tab is a `Tab` variant: drag between
panes and within a pane, Ctrl+Tab and the MRU, Cmd+1 to 9, close and reopen from the closed
stack, rename (the title shown is the rename, the file name otherwise), split, maximize, the ⋯
menu. The one new rule: a video plays only while it is the active tab of its pane and the window
is not hidden or minimized; switching away pauses it and remembers that it was playing;
switching back resumes. Closing the tab releases the player at once (no grace: there is no
process behind it). Two tabs on the same file are two players; `arkos open` avoids making a
second one (section 5), the sidebar's double click does too (section 9).

## 4. The tab follows its file

The fingerprint (`config_watch::fingerprint`, mtime and size) and the once a second poll over
open files (`explorer_watch.rs`, `POLL`, which runs `Root::watched_paths`, `paths_changed` and
`reload_viewers_for` in `sidebar.rs`) are reused: `watched_paths` gains every video tab's path
and `paths_changed` hands a changed video path to its `VideoView`. No thread of its own.

The decision, a pure function so it is tested: given the last fingerprint shown, the fingerprint
one poll ago and the fingerprint now, do nothing while the file is changing (the two most recent
differ: ffmpeg is still writing), reload when the file has been stable for one poll and differs
from the one shown, and enter "waiting for file" when the file is gone. `ffmpeg` writes the output
in place, so a half-written file is seen for a second or two; the stability rule skips it. A
tool that writes to a temp name and renames over the path is seen as one change.

The reload: a new `AVURLAsset` and `AVPlayerItem` for the same path, the video output moved to
it, `replaceCurrentItemWithPlayerItem`, then a seek to the old position clamped to the new
duration and the old rate restored (playing stays playing, paused stays paused on the nearest
frame). The loop range is clamped to the new duration; a range whose start is now past the end
is cleared and the status says "loop cleared". While the new item loads the strip says
"reloading" and the old frame stays on screen; a new item that fails to load (truncated, not yet
a valid file) is dropped, the old item keeps playing, the status says "new file cannot be played
yet", and the next change on disk tries again. On success the status says "new cut" for three
seconds and the Quick Look poster cache entry for the path is invalidated, so the markdown
editor's poster for the same file is remade on its next draw (`media::poster_path` already keys
on mtime, so this is free).

"Waiting for file": the last frame stays, dimmed to 50 percent, with the status word; the first
stable fingerprint after that reloads. A tab whose file is missing at open (allowed only with
`--wait`, section 5) shows "waiting for file" over the empty background.

`arkos video reload` (section 6) and the R key run the same reload at once, without waiting for
a change: for a tool that keeps the mtime, or to retry after "cannot be played yet".

## 5. `arkos open`

```
arkos open <file> [--pane N | --split right|down|left|up] [--at TIME] [--paused | --playing]
                  [--loop A-B | --loop off] [--raw] [--new] [--wait] [--window PID] [--json]
```

- `<file>`: relative to the shell's working directory or absolute; made absolute and canonical by
  the command. It must exist unless `--wait` (a typo must not open an empty tab). What opens
  depends on `media::kind` and the viewer's rules: a video (`mp4`, `mov`, `m4v`, and whatever
  `media::kind` calls `Video`) opens as `Tab::Video`; a markdown file opens as `Tab::Viewer`
  rendered, or raw with `--raw`; any other text file opens as `Tab::Viewer` (the read only
  viewer, highlighted). An image is refused: `images are not shown in a pane yet; open it with
  \`open <file>\` (Preview)`. Audio (`media::kind` says `Audio`), `webm` and other files `media::kind` does not know (it
  answers `None`; the viewer would call them binary), and a remote path (`media::is_link`) are
  refused with a line saying what to do instead (QuickTime Player, Finder, the browser pane). Exit 1 for each.
- **Where it goes** (my call; section 15 lists it): the target window is the one showing the
  agent's session (section 8). With `--pane N` the tab goes into pane N of that window (Cmd+1
  order, as `arkos panes` lists it; an N out of range exits 1 with the count). With `--split
  SIDE` the agent's own pane is split on that side and the tab is the new pane's first tab. With
  neither: **the preview pane.** The first open from a session splits the agent's pane to the
  right and remembers the new pane as that session's preview pane; later opens from the same
  session go into it as new tabs while it exists (the window keeps `preview_pane: HashMap<SessionId,
  PaneId>`, not persisted; after a relaunch the first open splits again). This gives an agent a
  place of its own beside it and never covers a pane the user is looking at. When the agent's
  session tab is not in the window (it was closed in its minute of grace), the command exits 3
  with "your session is not shown in this window; pass --pane or --window".
- **Focus stays with the agent.** An open from the command line activates the tab in its pane
  and never moves keyboard focus: the user is typing to the agent while it runs `arkos open` once
  per cut, and a focus jump would send the next Space or L to the video. The pane's keys (section
  3.4) work after a click on the pane, as for any pane. `--focus` moves focus to the new or reused
  tab for the cases where the agent is asked to hand over ("put it in front of me"). `arkos panes`
  reports the focused pane as it is.
- **Without a session** (`--window PID`, or `Only` from a plain Terminal) there is no agent pane to
  anchor the preview rule on: `Preview` then splits the window's focused pane to the right the
  first time and reuses that pane afterwards, under a no-session key in the same map. `--pane` and
  `--split` (which splits the focused pane) work as before.
- **Reuse:** a tab already open on the same canonical path in the target window is activated in
  its pane (focus unchanged) instead of opening a second one; one outside the agent's own pane is
  preferred, and one only in the agent's pane moves out with its tab id (into the preview pane, or
  a new split to the right that becomes it) so the agent's pane is never covered (call 12), and `--at`, `--paused`, `--playing` and
  `--loop` apply to it; this is how an agent's iteration loop is one command per cut. `--new`
  forces another tab. A reused tab whose file changed since it last loaded is reloaded first
  (the same stable rule, with the stability wait skipped since the agent says the file is done).
- `--at TIME`: seek there once the file is loaded. `TIME` is seconds (`12`, `12.5`), `m:ss`,
  `m:ss.fff` or `h:mm:ss[.fff]`. Past the end clamps to the last frame. Default for a new tab: 0;
  for a reused tab: leave the position.
- `--paused` / `--playing`: the state after the open. Default for a new tab: paused (the user
  decides when sound starts; the agent is often opening a frame to talk about); for a reused tab:
  unchanged. `--loop A-B` sets the range and turns the loop on; `--loop off` clears it. A loop
  with `--playing` is how an agent shows a scene on repeat.
- `--window PID` aims at another window (from `arkos panes --all`); the default is the agent's.
- Output: `opened video ranger-radio-animatic.mp4 in pane 2 (new pane), 0:12.0 of 1:48.5,
  paused, 1920x1080` or `shown … in pane 2` for a reused tab (`reloaded, ` prefixed when this
  command loaded the new cut; a tab that already followed the file says only `shown`, so the
  confirmation that the file as it is now is on screen is exit 0 with no `error`),
  or for a document `opened notes.md in pane 2, rendered`. JSON: `{ "window": pid, "pane": n,
  "tab": id, "kind": "video" | "markdown" | "text", "reused": bool, "reloaded": bool, "video":
  { "duration", "position", "playing", "loop": [a, b] | null, "width", "height", "state":
  "ready" | "loading" | "waiting" | "unplayable", "error"? } | null }`. The command waits up to
  the daemon's wait (section 8) for the item to be ready so the line carries the duration; a
  slower load prints `state: loading` and the agent asks `arkos video status` next.
- Exit codes as the rest of the toolkit: 0 done; 1 refused (missing or unsupported file, bad
  time, pane out of range, the window reported an error); 2 usage; 3 no window to aim at.

## 6. `arkos video`

```
arkos video status [<file>] [--json]
arkos video play|pause [<file>] [--json]
arkos video seek <TIME> [<file>] [--json]
arkos video step [+N|-N] [<file>] [--json]
arkos video loop <A-B>|on|off [<file>] [--json]
arkos video reload [<file>] [--json]
arkos video close [<file>] [--json]
arkos video mute|unmute [<file>] [--json]
```

- **Which tab:** `<file>` picks the video tab on that canonical path in the target window. With
  no file and exactly one video tab in the window, that one; with several, exit 1 and a line per
  tab (`pane 2: ranger-radio-animatic.mp4`) so the agent names one. `status` with no file and
  several tabs lists them all instead of failing. `--window PID` and `--json` everywhere.
- `status` prints `ranger-radio-animatic.mp4 in pane 2: 0:12.0 of 1:48.5, paused, loop 0:12.0-0:18.0,
  1920x1080, file changed 00:00:04 ago, ready` (the state words of section 3.3); JSON the `video`
  object of section 5 plus `pane`, `tab`, `path`, `muted`, `file_mtime`.
- `seek` takes the time forms of `--at`; `step` moves `N` frames (default +1) and pauses;
  `loop A-B` sets and turns on, `on` loops the whole file, `off` clears; `reload` reloads now and
  waits for ready or failure, printing the new duration or the error (exit 1 on failure, the old
  cut still showing); `close` closes the tab (it goes to the closed stack as a hand close would).
- Every command prints the `status` line after acting, so an agent sees the result it changed.

## 7. `arkos panes`

`arkos panes [--all] [--json]`: the target window's panes in Cmd+1 order, one line each: `pane 1
(focused): terminal claude (you), terminal zsh` then `pane 2: video ranger-radio-animatic.mp4
(active), markdown notes.md`. "(you)" marks the tab of the calling session; "(active)" the pane's
shown tab; "(focused)" the window's focused pane. `--all` prints every window, each headed
`window {pid} ({project})`. JSON: `{ "window": pid, "project", "panes": [{ "n", "id", "focused",
"tabs": [{ "id", "kind": "terminal" | "markdown" | "text" | "browser" | "video", "title",
"active", "you"?, "path"?, "session"? }] }] }`. Exit 3 with no window. Terminal titles are the
tab's shown title (the program name or the rename), never the terminal's contents.

## 8. The targeted channel

`Customize` broadcasts to every project window and that is right for a theme. Opening a file
must land in one window: the agent's.

- **Protocol** (`crates/protocol`, `PROTOCOL_VERSION` 7 to 8, `DIRECT_FROM: u32 = 8`; variants
  appended, never inserted): `Request::Direct { id: u64, window: WindowTarget, action:
  PaneAction }` from a tool client; `Event::Direct { id, action }` to the one window;
  `Request::DirectDone { id, outcome: PaneOutcome }` from it; `Response::Directed { pid: u32,
  outcome: Result<PaneOutcome, String> }` to the tool, or `Response::Error` with one of the
  texts below when no window can be aimed at.
- `WindowTarget` is `Session(SessionId)`, `Pid(u32)` or `Only` (the one connected project
  window; an error listing the pids when there are several or none). A session can be attached
  to more than one window (`Session::subscribe` keeps a map of subscribers: a second window on
  the same folder shows the same tab), so `Session` resolves to the most recently connected
  window among those attached, which is the one the user opened last; `arkos panes --all` shows
  the others and `--window` aims at one. The daemon test covers zero, one and two windows for
  `Session` as well as for `Only`. The daemon resolves it from its client records (`attached`, `ui_pid`);
  a window older than `DIRECT_FROM` answers `Error("window {pid} is too old for this command")`.
  The daemon waits `CUSTOMIZE_WAIT` (two seconds) for `DirectDone`, then answers `Error("window
  {pid}: no answer")`.
- `PaneAction` is `Open { path, kind: OpenKind (Video | Markdown { raw: bool } | Text), place:
  Placement (Preview | Pane(u32) | Split(Side)), at: Option<f64>, playing: Option<bool>,
  loop_range: LoopChange (Keep | Set(a, b) | WholeFile | Clear), reuse: bool, wait_for_file: bool
  }`, `Video { path: Option<PathBuf>, op: VideoOp (Status | Play | Pause | Seek(f64) | Step(i32)
  | Loop(LoopChange) | Reload | Close | Mute(bool)) }`, `Panes`. The command line does the file
  checks (existence, kind) before sending; the window checks again (the file may have gone).
- `PaneOutcome` is `Opened { pane: u32, tab: u64, kind, reused, reloaded, video:
  Option<VideoState> }`, `Videos(Vec<VideoState>)` (one for most ops; several for a bare
  `status`), `Panes(PanesReport)`, or the error text in the `Result`. `VideoState` is `{ pane,
  tab, path, duration, position, playing, muted, loop_range, width, height, state, error,
  file_mtime }`.
- **The command line** finds the session from `ARKOS_SESSION_ID` (the daemon put it in every
  shell it starts) and sends `WindowTarget::Session`; `--window` sends `Pid`; outside Arkos with
  no `--window`, `Only`, so a user at a plain Terminal with one Arkos window open can still use
  it. The connection is `hook_cli::send_one_with` as the theme commands use, with no Hello.
- **The window** handles `Event::Direct` on the UI thread in `Root`: `Open` runs the placement
  and reuse rules of section 5, creates the tab, waits for the video to be ready (a one shot
  channel the `VideoView` completes, with a deadline just under the daemon's) and answers;
  `Video` finds the `VideoView` and applies the op; `Panes` reads the tree. A window never
  writes a file on this channel other than its own workspace file, which it saves anyway.
- **The guide's promise changes** from "no opening files" to "you may open a video or a
  document in a pane and control a video; you may not run commands in its terminals or reach a
  session" (section 10).

## 9. The window's own ways in

- **Sidebar:** `open_viewer` on a path whose `media::kind` is `Video` makes (or reuses) a
  `Tab::Video` instead of a `Tab::Viewer`, with the same preview-on-single-click and
  pin-on-double-click behaviour the viewer has (a single click previews in the reusable preview
  tab, paused; a double click pins). The tree's file row keeps its icon.
- **Markdown editor:** Cmd+click on a video's poster opens a `Tab::Video` paused at 0 in the pane
  to the right of the document's pane when there is one, else splits right; a second Cmd+click
  activates the existing tab. The guide text "Cmd+click plays it in QuickTime Player" changes
  accordingly; QuickTime stays the fallback for a file Arkos cannot play and for audio.
- **Drag and drop** of a video file from Finder onto a pane is not in this milestone (gpui's
  external drop support would need checking; listed in section 14).

## 10. Discovery

An agent that has never heard of Arkos must find this in one step from each of the three layers
the theme interface established:

1. **The hook line** (`hook_cli::CONTEXT_LINES`, printed by the SessionStart hook inside Arkos):
   "This session runs inside Arkos IDE. To show the user a video or a document in a pane beside
   you, or to change Arkos's theme, fonts or settings, run `arkos guide` first and follow it."
   Two lines still; the second grows. The `{exe} guide` form for a binary off the PATH changes
   the same way.
2. **The skill** (`guide::SKILL_DESCRIPTION`): "Show a video or a markdown file in an Arkos IDE
   pane, control its playback, or customize Arkos: themes, fonts and settings. Use when the user
   asks to see, preview, play or watch a video or a document, or to change how Arkos looks or
   behaves." The stamp changes with the text, so the doctor reports the installed skill as
   outdated and offers the reinstall; the SessionStart hook's install path refreshes it on the
   next session start as it does today.
3. **The binary:** `arkos --help` gains `arkos open <file> …` second, after `guide`, then
   `arkos video <command> …` and `arkos panes`; `arkos open` and `arkos video` with no arguments
   print their usage with "see: arkos guide".

**The guide** (`docs/agent-guide.md`) gains, in this order: section 1's first paragraph rewritten
for the new allow-list; in section 2, that `ARKOS_SESSION_ID` also tells the commands which
window is yours; a new section "Showing the user a file" before the theme sections, with the
commands of sections 5 to 7 (exit codes, `--json` shapes), the time forms, the preview pane rule,
the follow-the-file rule and the one word reload; a worked example that is the video iteration
loop:

```
arkos open out/ranger-radio/ranger-radio-animatic.mp4 --at 0:12 --paused   # first cut, the radio scene
python3 tools/cinema/animatic.py tools/cinema/ranger-radio.json              # the next cut
arkos open out/ranger-radio/ranger-radio-animatic.mp4 --at 0:12 --loop 0:12-0:18 --playing
arkos video status out/ranger-radio/ranger-radio-animatic.mp4 --json
```

with the note that the second `open` reuses the tab, makes sure it shows the new file and jumps,
that the tab reloads on its own within two seconds anyway (so the line often says `shown` without
`reloaded`), and that exit 0 with no `error` is the confirmation; and rules: open a file in a
pane when the user asks to see, watch or review it, and offer it when you have just made one;
prefer the preview pane and name the pane number to the user; never open the same file twice
(`open` reuses for you); leave the tab open when you finish, the user closes it; pause before
you talk about a frame; do not open files the user did not ask about.

**The doctor** gains, in the agent guide section it has, a line when the installed skill's stamp
is older than the shipped guide ("the agent guide installed is from an older Arkos; agents will
not know about video panes"), with Apply running the install, as it does for a missing skill.

## 11. Documentation

- `docs/guide.md`: a "Video pane" section after "Browser pane" (what the tab is, the strip, the
  keys, that it follows its file, that an agent can open and drive it, the fallback to
  QuickTime); the "Images and media" paragraph's Cmd+click sentence updated; the Keyboard
  section's table gains the video rows; "Themes for agents" becomes "Agents" and mentions `arkos
  open`.
- `README.md`: the one line list at the top gains "a video pane", the agents bullet becomes "run
  `arkos guide` in an Arkos terminal for how to show the user a video or a document and how to
  change the theme and settings", and a sentence under "The blogger" or a fourth persona, "the
  director", if the shoot gains a picture (optional; the shoot's demo would need a small video,
  which `scripts/sample-png.py`'s approach can make with ffmpeg; the builder decides whether the
  picture is worth it in this milestone).
- `docs/development.md` Status: a paragraph for milestone 9 and these hand checks: a video opens
   from the sidebar and plays with sound; Space, arrows, comma and period do what the table says;
   dragging the tab to another pane keeps playing after the drop and switching tabs pauses it;
   re-rendering the file with ffmpeg reloads the tab within two seconds, holding the position,
   and the half-written file never shows; deleting the file shows "waiting for file" and
   restoring it recovers; a relaunch brings the tab back paused on the frame; `arkos open` from a
   Claude Code tab splits a preview pane to the right the first time and reuses it the second;
   `arkos open … --at 0:12 --loop 0:12-0:18 --playing` loops the range with sound; the fresh
   session demo of section 1; a 4K file plays without the window stuttering; the GPU stays idle
   when every video is paused (Activity Monitor's GPU column).
- `docs/telemetry.md` (section 12).

## 12. Telemetry

Following `telemetry/events.toml` and the disclosure page, with no name or path ever:

- `pane_open` and `pane_close` gain `kind: video`; `pane_open` gains `how: agent` for a tab an
  agent's `Open` made (markdown too) and `how: poster` for the editor's Cmd+click.
- `view` sends `format: video` (reserved today) when a video tab first shows a frame.
- `focus` gains the `video` kind.
- A new event `agent_pane` with `command: open, status, play, pause, seek, step, loop, reload,
  close, mute, panes` and `ok: true | false`: a `Direct` action ran, which one, and whether it
  succeeded. This says whether agents use the interface, which is the question this milestone
  asks.
- A new `error` kind `video_unplayable` when a file cannot be played, so a codec gap shows up.

The disclosure page gains these rows; the allowlist test that compares `events.toml` with the
page keeps them honest.

## 13. Tests

Headless, as always; nothing in `cargo test` creates an `AVPlayer` or needs a display.

- `tree.rs` / `workspace.rs`: `Tab::Video` round trips with and without `at` and `loop_range`;
  a version 3 file with a `Viewer` on an `.mp4` loads as a `Video`; the closed stack reopens a
  video paused at its position.
- The follow decision (section 4) as a pure function over three fingerprints: changing, stable
  and different, stable and same, gone, back.
- Position and loop clamping after a reload with a shorter and a longer duration.
- Time parsing: `12`, `12.5`, `0:12`, `1:02:03.250`, `-1`, `1:60`, `abc`.
- Placement (section 5) as a pure function over a `Tree`: preview pane made then reused, reused
  pane gone so a new split, `--pane N` in and out of range, `--split`, reuse of an open path,
  `--new`, the no-session anchor, and that the tree's focus is unchanged without `--focus`.
- `Tree::set_video_state` writes `at` and `loop_range` and nothing else; the saved JSON carries
  them.
- `arkos open`, `arkos video` and `arkos panes` against the fake daemon `agent_cli` already has:
  the human lines and the `--json` shapes for each outcome, the refusals (missing file, image,
  audio, bad time, pane out of range), `--window`, exit 3 when the session is not shown.
- The daemon: `Direct` resolution for `Session`, `Pid` and `Only` with zero, one and two windows
  and with a too old window; the two second cut; a late `DirectDone` is not an error.
- The protocol: the new messages round trip through postcard and JSON and sit at the end of
  their enums (the existing shape test extends).
- `hook_cli`: the new context lines; `guide.rs`: the stamp changes with the description; a test
  that the guide names every `arkos open`, `arkos video` and `arkos panes` subcommand and flag
  that the parser accepts (so the guide cannot fall behind the code).
- Telemetry: the new events pass the allowlist and the disclosure page lists them.
- `VideoView`'s state machine (ready, loading, reloading, waiting, unplayable; playing kept
  across a reload; paused while inactive and resumed) behind a `Player` trait with a fake, as
  `ax.rs` has `FakeAx`; the real AVFoundation player is the one implementation `cargo test`
  never constructs.

## 14. Deferred, for the next milestone's list

- An image tab (`arkos open x.png`), the cheapest follow-up: `img()` in a tab with fit and zoom.
- Audio files in the same tab (AVPlayer plays them; the picture would be the file name and the
  strip).
- Drag and drop of files from Finder onto a pane.
- `--at <heading>` for markdown, scrolling the editor to a heading.
- An agent reading a frame out (`arkos video frame --out f.png`); ffmpeg covers it today.
- Playback rate, a volume slider, subtitles.
- Rotated and anamorphic video: the surface paints the decoded pixel buffer as stored, so a track
  with a rotation in its metadata (phone footage) shows on its side and non-square pixels
  (anamorphic) show squeezed. The track's preferred transform and pixel aspect are to be applied
  when the frame is fitted.
- Replacing the Chrome docking with an in process web view: Cyrus's wish (2026-10-20). `WKWebView`
  cannot be painted through `surface()`; it would be an `NSView` child of the window placed over
  the pane's bounds and clipped by the window, which the gpui window's `NSView` allows but which
  gpui does not manage. The video tab's model (a `Tab` variant, a view entity, the targeted
  channel, the follow rules) is the template; the rendering is the open question to spike.

## 15. The calls I made (check these while reading)

1. The preview pane rule: first open splits the agent's pane to the right; later opens reuse
   that pane; `--pane` and `--split` override.
2. A new tab opens paused, at 0 unless `--at`; a reused tab keeps its state unless told.
3. `arkos open` reuses a tab on the same path by default; `--new` makes another.
4. Persisting the position and loop range in the workspace file, saved on pause, seek, loop and
   the usual saves, never per frame; a relaunch comes back paused on the frame.
5. The stability rule: reload only after one poll with no change, so a file ffmpeg is writing is
   never loaded half way.
6. A missing file is refused unless `--wait`.
7. The control strip is always visible; mute instead of a volume slider; frame stepping on comma
   and period.
8. Markdown opens through the existing Viewer tab unchanged, rendered unless `--raw`; any text
   file is allowed; images and audio are refused with a pointer.
9. `arkos panes` shows tab titles, never terminal contents.
10. Protocol 8 with a targeted request; the daemon and every window move together through the
    existing update path, as 0.4.0 did for `Customize`.
11. The skill description grows rather than a second skill: one skill, one guide, one stamp.
12. An agent's open never moves keyboard focus; `--focus` does. (The reviewer's first question.)
13. A session shown in two windows opens in the most recently connected one.
14. Without a session, the preview pane anchors on the window's focused pane.
15. A playing video keeps the display awake with a scoped activity token, as QuickTime does.
