# arkos: a terminal layout manager for Claude Code sessions

**Status:** design approved in conversation on 2026-10-13; this document is the written spec.
**Name:** Arkos IDE (display name; UI binary `arkos`, daemon binary `arkosd`). Repository: `csarkosh/ide-arkos`. License: GPL-3.0.

## 1. Why this exists

Cyrus runs several Claude Code sessions at once in VS Code (Cursor), laid out as a grid of
terminal panes. Those sessions drive CPU and GPU heavy work (local model inference for
asset generation). On a 16 GB M4 Mac with RAM at 93 percent, VS Code froze and never
recovered; the only way back was to kill it, which killed the terminals and the Claude Code
sessions inside them. VS Code's terminal scrolling is also jumpy and gets worse as scrollback
grows.

arkos does one job: lay out terminal panes for multiple Claude Code sessions, plus a
browser pane and a small sidebar, in a window that is light on memory, smooth to scroll,
easy to tinker with, and that can never lose a session because its UI froze.

### 1.1 Requirements (as stated, with decisions)

| Requirement | Decision |
|---|---|
| Layout like the VS Code screenshot: draggable panes forming 1:2, 2x2, 3x2 grids; resizable both ways; tabs per pane | Binary split tree, VS Code drop zones, tabs within panes (section 4) |
| Same colour scheme as the screenshot; more presets later | Theme files, default sampled from the screenshot (section 7) |
| Minimal memory and CPU | Budgets in section 9; no web engine, no JIT, bounded scrollback, virtualized rendering |
| Terminals only, for Claude Code sessions | Tabs hold a terminal or a read-only file viewer; no editor |
| Must self-recover when CPU/GPU is pegged; never lose sessions | Sessions live in a separate daemon; a daemon-side watchdog relaunches a stuck UI (section 2) |
| Embedded browser like Cursor's, to watch Claude work | A real Chrome window docked over a pane via the Accessibility API (section 8) |
| Smooth scrolling that stays smooth in long sessions; virtualization is fine | Fractional scroll position, eased animation, visible rows only (section 3) |
| Later additions during design | Tabs within panes; a left sidebar with a file tree and a lazy git worktree explorer; a read-only viewer with diff gutters; Claude Code session state icons on tabs; VS Code keybindings; friendly empty states; headache-proof defaults from the research (section 7.1) |

### 1.2 Non-goals

No text editing, no search across files, no git staging or committing, no extensions, no
Linux or Windows support in the first version, no embedded Chromium (CEF), no WKWebView.

### 1.3 Language and stack

Rust end to end. Reasons: `alacritty_terminal` (0.25.x) is the best available terminal
emulation library and the heavy lifting is terminal emulation and the daemon; footprint is
decided by architecture, not language; one language keeps tinkering cheap. The UI uses GPUI
(the framework Zed is built on, published on crates.io at 0.2.x, pre-1.0, pinned to an exact
version) for its Metal renderer, CoreText text shaping with full font fallback (emoji, braille
spinners, box drawing all render without configuration), flex layout, scroll containers and
virtualized lists. Zed's own terminal view wraps `alacritty_terminal` in GPUI and is the
reference for that integration.

Rejected: winit plus wgpu with a hand-rolled toolkit (lightest, but months of toolkit work
before any UX); egui (fastest to prototype, but no system font fallback, weaker Retina text);
Swift/AppKit shell over a Rust core (best native integration, but a second language and an
FFI seam exactly where tinkering happens); WKWebView via `wry` (Safari engine quirks, and
Claude Code's browser tools drive real Chrome, so it would show nothing of Claude's work);
CEF (hundreds of MB, the Electron footprint being escaped).

Known risk accepted: all three UI stacks render through Metal. When inference saturates the
GPU, frames queue behind that work and may lag. They cannot deadlock the app, and the
watchdog restarts a stalled UI without touching sessions. A CPU software renderer is a
possible later escape hatch but is not designed in.

## 2. Process architecture

Two binaries in one Cargo workspace.

### 2.1 `arkosd`, the daemon

Headless, no GPUI dependency. Owns every pseudo-terminal (`portable-pty`) and the shell
inside it. Listens on a Unix domain socket at
`~/Library/Application Support/arkos/arkosd.sock`. The UI launches it on demand if it is
not running; it keeps running when the UI exits. Idle cost: a few MB, zero CPU.

Per session it keeps: id, title, cwd, pid, alive flag, last activity time, Claude state
(section 5), and a **replay buffer**: a ring of the raw output bytes, default 16 MB per
session, configurable. On attach the daemon sends the buffer, then streams live bytes, so a
reattached UI rebuilds the screen and scrollback exactly. Nothing survives daemon death or a
reboot.

The shell and nice level are fields of the CreateSession request, applied when the shell
starts; they are not part of the session record.

Robustness rules: never allocate per output byte beyond the ring buffer; a shell exiting is
reported, not fatal; a malformed client message disconnects that client only; the socket file
is recreated on start and a stale socket from a crashed daemon is detected (connect fails)
and replaced.

### 2.2 `arkos`, the UI

The GPUI app. Connects to the socket, lists sessions, reattaches those named in the
workspace file, renders. Holds no process state that matters and can be force-quit at any
time. Its main thread runs at user-interactive quality of service.

### 2.3 Protocol

Length-prefixed binary frames (u32 length, then a `serde`-encoded message via `postcard`).
Requests and replies carry a request id.

| Message | Direction | Payload |
|---|---|---|
| CreateSession | UI to daemon | cwd, shell, env, nice level, rows, cols. Reply: session id |
| Attach | UI to daemon | session id. Reply: replay buffer, then a stream of Output frames |
| Detach | UI to daemon | session id |
| Input | UI to daemon | session id, bytes |
| Resize | UI to daemon | session id, rows, cols |
| Kill | UI to daemon | session id |
| ListSessions | UI to daemon | none. Reply: the per-session record above |
| Heartbeat | UI to daemon | monotonic counter |
| SetState | hook CLI to daemon | session id, Claude state (section 5) |
| Output | daemon to UI | session id, bytes |
| SessionEvent | daemon to UI | session id, one of: exited(code), state(ClaudeState). Title changes and the bell are escape sequences in the output, which the UI's emulator parses; the daemon never parses output (section 2.1). |

The `arkos` binary doubles as the hook CLI: `arkos notify --state <working|waiting|idle>`
reads the session id from `ARKOS_SESSION_ID` in its environment and sends SetState.

### 2.4 Watchdog

The UI sends a Heartbeat from its main thread every second. If the daemon sees none for the
configured window (default 30 s) while the UI process is still alive, it kills and relaunches
the UI. The relaunched UI reattaches every session from the workspace file. Sessions are never
touched. This holds regardless of what the renderer or the GPU is doing.

A silent window is not always a hung one: while the Mac sleeps, and through Power Nap's dark
wakes, macOS runs the daemon but schedules no window. So:

- **Sleep.** A scan where the wall clock moved more than a few seconds further than the
  monotonic clock (the system slept), or whose own gap is far longer than its interval (the
  daemon was not scheduled), gives every window a fresh heartbeat clock and relaunches nothing.
- **Away.** No window is killed while the screen is locked, the main display is asleep, or the
  window server cannot say (no session). The daemon log says so once per absence. The first
  scan after the person is back gives every window a fresh clock and relaunches nothing.
- **Backoff.** A window relaunched for silence that goes silent again waits 2 minutes from that
  relaunch, then 8, before the next. A scan that finds it heartbeating ten minutes after its
  last relaunch forgets the count. The count is kept per window binary and folder in the
  daemon, across reconnects; a daemon restart forgets it.
- **Stop.** Silent again after three relaunches in a row, the window is left as it is, and one
  line in the daemon log and the update journal says to quit it and open Arkos again. A fresh
  count starts only once every stopped window of that binary and folder is gone and another
  one opens; a healthy sibling never undoes a stop.

### 2.5 Scheduling

UI main thread: user-interactive QoS. Each session can be spawned at a configurable nice
level (per-tab option, default 0), so a heavy Claude Code job and its children yield to the
UI when the system is pegged.

## 3. The terminal pane

**Emulation.** One `alacritty_terminal` instance per terminal tab, in the UI process, fed by
the daemon stream on a background task. Scrollback is a bounded ring, default 20,000 lines,
configurable up to whatever memory allows (a full 100,000 line buffer at 200 columns can
approach 1 GB in the worst case; alacritty stores every cell of every line, so memory follows
columns times lines: about 3 MB per 1,000 lines at 124 columns, or about 60 MB for a full
20,000-line tab, which milestone 6 measures against the budget in section 9).

**Rendering.** Only the rows in the viewport are drawn. Scroll position is a floating point
line offset. Wheel and trackpad input update a target; each frame moves the position toward
it with a short ease-out, so scrolling glides, and rows are drawn at the fractional pixel
offset. New output does not pull the view down while scrolled up; a small "jump to bottom"
affordance appears. Box-drawing glyphs are drawn by the renderer to fill the full cell, so the
taller line height (section 7) leaves no gaps in Claude Code's frames.

**Frame pacing.** Output is coalesced: the emulator applies all bytes that arrived, then the
pane requests one repaint. At most one frame per display refresh under a flood of output. Late
frames draw the latest state; animations are time-based, not frame-based.

**Text.** CoreText via GPUI. Default Menlo at 16 px with line height 1.5 (section 7.1).

**Input.** Keystrokes go to the daemon as bytes; bracketed paste is supported (Claude Code
relies on it for multi-line input). Option is Meta by default (`[terminal] option_as_meta`,
Terminal.app's "Use Option as Meta Key"): Option+key sends ESC plus the key, Option+Enter ESC CR.
A program that sets the kitty keyboard protocol's disambiguate flag gets Esc and the modified
Enter, Tab, Backspace, Space, Option+key and Ctrl+Shift+key as `CSI code ; mods u`; everything
else keeps its legacy bytes, and the flags leave with the alternate screen (added 2026-10-15). Mouse selection and copy are local (a drag, a double click for
a word, a triple click for a line; Cmd+C); copy uses the system clipboard. A program that turns
on mouse reporting gets the wheel and single clicks at the cell under the pointer, in the SGR or
X10 form it asked for; a drag is a local selection and never a report. On the alternate screen
without reporting the wheel sends arrow keys (xterm's alternate scroll, `?1007`, on by default);
Shift+wheel always scrolls the local history (added 2026-10-15). BEL marks the tab (section 5). A link in the output (`http://` or `https://` to the
next space or quote, trailing punctuation left out) is underlined under the pointer and opened
by Cmd+click, Terminal.app's gesture, so a plain click stays free for selection (added 2026-10-14).

## 4. Layout, tabs, drag and drop

**Model.** A binary split tree. Internal node: orientation (horizontal or vertical) and a
ratio. Leaf: a pane with an ordered list of tabs and an active index. The screenshot layout is
a vertical split at about 0.5 whose right child is a horizontal split at 0.5. A tab is either a
terminal (session id) or a viewer (path, worktree root).

**Resizing.** Every split edge is a 2 px gap with an 8 px invisible hit area; at rest it shows
only the window background between the pane borders, and while the pointer is over it or drags
it a 6 px stripe fades in over the neighbours' edges, so the panes never move for it (2 px and
the stripe from 2026-10-15; before, a 4 px painted handle). Dragging
updates the ratio live; double-click resets to equal. A minimum pane size (default 160 px wide,
80 px tall) stops a pane being squeezed below readable.

**Tabs.** Per-pane tab bar as in the screenshot: icon, title, close button; plus button and
overflow menu at the right of the active pane. Title defaults to the shell name and follows the
terminal title escape sequence. Tabs reorder by drag within a pane.

**Drag and drop between panes.** Dragging a tab out of its bar shows a drop overlay with five
zones on the hovered pane: center (move into pane), left, right, top, bottom (split that pane
and place the tab in the new pane on that side). VS Code's gesture exactly.

**Empty state.** A pane closes with its last tab (Cmd+W, the tab's close button, "close all in
pane", or its last tab dragged into another pane), its space going to its neighbour. The last pane
in the workspace never closes: it shows a friendly screen instead: a short greeting, the three or
four bindings you'd want next, and a small ambient animation: three dots that keep drifting
slowly for as long as the pane is shown (changed 2026-10-15 from "settle after six seconds" at
the user's request). It honours the system Reduce Motion setting, which holds them still. A folder opened with no saved layout starts on that screen rather
than on a shell nobody asked for (revised 2026-10-14; before, an emptied pane stayed open and a
new folder started one shell).

**Choosing a folder.** An `arkos` launched from the Dock, Finder or Spotlight (working directory
`/`) first shows a small chooser: the five most recent folders, "Show all" for the rest, and the
native folder picker; Escape quits. From a window, "Open folder…" (Cmd+O) and "Open recent…"
open the chosen folder in a new window process (added 2026-10-15). At launch each folder's row
ends with its live session count (the sessions its workspace names plus any whose shell runs in
the folder) and "Stop all", which arms on the first ask and kills on the second; the chooser
holds the connection's receiver until a folder is chosen and the window takes it over (added
2026-10-15).

**Keyboard.** VS Code's macOS defaults for terminals in the editor area. Every binding lives in
the config file and can be changed, chords included.

| Action | Binding | VS Code equivalent |
|---|---|---|
| Focus pane N (left to right, top to bottom) | Cmd+1 … Cmd+9 | focus editor group N |
| Focus pane in a direction | Cmd+K, Cmd+Left/Right/Up/Down | focus left/right/above/below group |
| Split right / split down | Cmd+\ / Cmd+K, Cmd+\ | split editor / split editor orthogonal |
| Maximize pane (toggle) | Cmd+K, Cmd+M | toggle maximize editor group |
| Move active tab to previous/next pane | Ctrl+Cmd+Left/Right | move editor into previous/next group |
| Next / previous tab | Cmd+Option+Right/Left, also Cmd+Shift+] / [ | next/previous editor |
| Cycle tabs, most recently used | Ctrl+Tab | quick open previous editor |
| Close tab / reopen closed / close all in pane | Cmd+W / Cmd+Shift+T / Cmd+K, W | same |
| Move tab left/right in the bar | Cmd+K, Cmd+Shift+Left/Right | move editor left/right |
| New terminal tab in focused pane | Ctrl+Shift+` | closest match: VS Code's new terminal opens in the panel, and "new terminal in editor area" has no default |
| Clear scrollback / find / font size | Cmd+K / Cmd+F / Cmd+= and Cmd+- | same, when terminal focused |
| Toggle sidebar / focus file tree / focus worktrees | Cmd+B / Cmd+Shift+E / Ctrl+Shift+G | sidebar / explorer / source control (borrowed) |
| Open config / command palette | Cmd+, / Cmd+Shift+P | settings / palette |

The browser pane is a pane, so Cmd+1 to 9 reaches it. No bell-jump or browser-focus
shortcut exists (considered and dropped).

**Persistence.** Layout tree, tab order, active tabs, split ratios, each tab's session id or
viewer path, sidebar width and collapsed states, and the browser URL are written to
`~/Library/Application Support/arkos/workspaces/<hash-of-project-path>.json` on every
change, debounced. On launch the app reads it, reattaches each tab to its session and
rebuilds the layout. A missing session reopens as a fresh shell in the same cwd with a note in
the tab. One window is one workspace (project folder); another folder is another window.

**Window chrome.** Top bar with the project name (as in the screenshot). Bottom status bar:
project path on the left; on the right, daemon status, session count, and the count of
sessions waiting on the user (section 5), and the break nudge when due (section 7.1). Sidebar on the left.

## 5. Claude Code session awareness

Claude Code's hooks run shell commands on lifecycle events. The daemon sets
`ARKOS_SESSION_ID` in every shell it spawns; hook commands inherit it, so
`arkos notify --state …` reaches the daemon over the socket and names the right session.

| Claude Code event | State sent |
|---|---|
| UserPromptSubmit | working |
| PreToolUse | working |
| Notification (permission or idle prompt) | waiting |
| Stop | waiting |

Tab states and icons: **working**, a small glyph with a slow, soft breathing pulse (about a
2 s cycle, ticked at a low rate so a working tab costs a few percent of one core at most, and
paused when the window is not visible); **waiting**, a solid dot in the theme's warm accent that
stays until the tab is focused; **idle**, no icon; **dead**, shell exited, muted title. A terminal
BEL also sets waiting, so the feature degrades gracefully without hooks (Claude Code can be
set to ring the bell when it finishes).

Setup is one palette command, "Install Claude Code hooks", that shows the exact change to
`~/.claude/settings.json` before writing it, and is idempotent.

## 6. Sidebar

A fixed left column, Cmd+B to toggle, default 260 px, resizable by its edge. Two sections
stacked vertically, each collapsible with a header: Explorer, then Worktrees.

**File tree.** Rooted at the project folder. Lazy: a directory's children are read on expand.
Respects `.gitignore` by default (toggle to show ignored), hides `.git`. A file system watcher
updates expanded directories, debounced. Rows carry git status in the theme's pastel set
(section 7); parent folders with changes carry a dot. Right click: reveal in Finder, copy path,
copy relative path, open in default app. Dragging a file onto a terminal pastes its path.
Single click opens a preview viewer tab in the focused pane that the next single click
replaces; double click pins it (VS Code's preview behaviour).

**Worktree explorer.** Runs `git worktree list` only: the list shows names and branches,
collapsed. Expanding a worktree runs `git status` for that worktree on a background thread, with
a soft inline loading shimmer in the row, then animates the changed files open with a short
ease; collapsing drops the results. An expanded worktree refreshes on the watcher and a slow
timer; collapsed ones never run status. A dirty count shows only once loaded. Clicking a changed
file opens the viewer for that file in that worktree. Actions: open a terminal tab in the
worktree's directory; create a worktree from a branch name with a prompt for the path; remove a
worktree, refusing if dirty unless force is ticked. The main checkout is first and cannot be
removed.

**Viewer tab.** Read-only. Loads the file once, highlights with `syntect` (TextMate grammars,
so colours match VS Code's), virtualized, line numbers in a gutter. Against the worktree's HEAD
it computes a line diff and marks the gutter: green bar for added, blue bar for modified, red
triangle where lines were deleted. A toolbar toggle (Cmd+Shift+D while focused) expands to an
inline diff showing deleted lines in red between the current ones. Re-renders when the watcher
sees the file change. Cmd+F finds in the file. Files over 5 MB (configurable) open without
highlighting; binaries show a short notice.

## 7. Config and themes

**Config file.** `~/.config/arkos/config.toml`, created with commented defaults on first run,
opened by Cmd+, in the default editor. Watched and applied live. A bad edit shows a
non-blocking banner with the line number and keeps the last good config. Sections: `font`,
`terminal` (scrollback, shell, nice level), `keybindings`, `sidebar`, `browser` (default URL),
`daemon` (watchdog timeout, replay buffer size), `theme` (name), `appearance` (follow system
light/dark or fixed).

**Themes.** One TOML file each in `~/.config/arkos/themes/`; built-ins are embedded in the
binary and extractable. A theme is a flat set of named colours:

- chrome: window background, title bar, status bar, sidebar, tab bar, active tab, inactive
  tab, border, split handle, focus ring;
- terminal: background, foreground, cursor, selection, the 16 ANSI colours;
- semantic: working pulse, waiting dot, bell, error banner;
- tree: pastel added, modified, deleted, untracked, ignored;
- viewer: syntax colours mapped onto the highlighter's scopes.

**Default theme ("Navy", from the screenshot, tuned by the research).** The general
scheme stays the screenshot's dark navy; the exact values are chosen after the eye-comfort
research doc is written, so that contrast, foreground brightness and the accent colours
follow what the evidence supports (section 7.1). Starting points: deep navy background near `#0d1117`; tab
bar and status bar a darker band near `#0a0e14`; borders near `#1f2630`; foreground a light
grey near `#d6dae0` (about 12:1 against the background; high luminance, not dim, per section 7.1).
Pastels for the tree: soft mint (added), soft amber (modified), dusty rose (deleted), muted
lavender (untracked), grey (ignored). Waiting dot: a warm accent. Exact hex values are
sampled from the screenshot once it is copied into `docs/reference/` (the shell cannot read
the Desktop); the values above are eyeballed placeholders until then. A second built-in,
"Navy Light", is a soft light counterpart with the same hue relationships.

**Text presets.** Font size and line height are switched the same way themes are. A text
preset is a named pair in the config, and the palette command "Text preset" cycles or picks
one, applying it live to every terminal and viewer and re-measuring the cell grid. Built-ins:

| Preset | Size | Line height | Intent |
|---|---|---|---|
| Compact | 14 px | 1.3 | More columns when a 2x2 grid is open; below the research's comfort floor, so labelled as such |
| Comfortable (default) | 16 px | 1.5 | The section 7.1 defaults |
| Relaxed | 18 px | 1.6 | Closer to the 0.25 degree x-height target at 60 cm on a laptop screen |

Users add their own presets in the config. Cmd+= and Cmd+- still nudge the size of the
focused terminal without changing the preset, and the "size for my distance" helper writes a
custom preset. The active preset is saved with the workspace, since a 2x2 grid and a single
pane want different sizes.

### 7.1 Headache-proof defaults (from the research doc)

The research (published on csarko.sh/research as "Check your eyes before your theme") found
that no display setting has ever been tested with headache as an outcome, and that the
variables which move measured outcomes are luminance reaching the eye, angular text size, line
pitch relative to x-height, and flicker. "Dark versus light" is a proxy for the first and
nothing else. arkos therefore ships these defaults, each labelled in the config as a
preference with its evidence status, never as a health feature:

| Default | Value | Why |
|---|---|---|
| Font size | **16 px** (configurable; a "size for my distance" palette command converts viewing distance and display DPI to the size that puts x-height at 0.25 degrees) | Reading speed collapses below 0.2 degrees of x-height, and the dark-theme legibility penalty grows as text shrinks. 12 to 13 px on a 14-inch MacBook Pro at 60 cm is well below that. 16 px is the compromise between that finding and keeping 80 columns in a half-width pane; the helper lets the user go larger. |
| Line height | **1.5** (range 1.4 to 1.6; 1.0 to 1.2 is not offered as a preset) | Rows of text at a 14 px, 1.2 pitch land near 3 cycles per degree, the peak of the pattern-glare curve. 1.5 brings the x-height to line-pitch ratio to about 35 percent and moves the row grating off the peak; reading speed plateaus by 1.2 to 1.5x so there is no cost. Box-drawing glyphs are stretched to fill the cell (section 3). |
| Foreground luminance | A **light grey, not dim**, about `#d6dae0`, on the navy, about 12:1 | Legibility favours high luminance; a dim dark theme with small text is the worst configuration the literature identifies. Pure white on pure black versus a softened palette is untested, so the softening is modest and labelled a preference. |
| Background | The screenshot's navy, lifted slightly from near-black to about `#11161f` | Keeps the scheme; avoids pure black. Exact values are sampled from the screenshot in milestone 2. |
| ANSI and syntax colours | Every colour carries **luminance contrast of at least 4.5:1** against the background, preferably higher; no saturated low-luminance colours on the dark background | Images with high colour contrast and little luminance contrast are rated uncomfortable and reported to induce headaches in hypersensitive observers. The tree's pastels satisfy this by construction: high luminance, low saturation. |
| Letter spacing | Font default; no tracking option | Both tighter and wider spacing slow skilled readers. |
| Appearance | **Follow the macOS appearance** by default; Navy for dark, Navy Light for light; no health claim for either | Fatigue trials between polarities are null; the bad cell is a screen that mismatches the room's brightness. Following the system setting tracks the user's own room choice. |
| Warm tint, blue-light mode | **None shipped** | Blue-light filtering fails a double-masked RCT and a Cochrane review; software warm tints have no eye-strain trial, and the warmest setting was worst in the one lab study. |
| Refresh rate | Nothing claimed | No evidence for static text. |
| Break reminder | **On by default: a quiet status-bar nudge after 60 minutes of activity suggesting a 5-minute break**, user-adjustable to 30 on, 5 off, or off. Never a modal, never a 20-second pop-up | Hourly 5-minute breaks have field-trial support; the 20-20-20 rule's 20-second glances were null in their only controlled test. |
| Motion | The working pulse, the focus glow's breath (six seconds, 70 to 100 percent of a 0.45 alpha, brightened at the user's request, 4 px at rest; added 2026-10-15) and the empty-state animation are slow, low-contrast, and honour Reduce Motion | Flicker and high-contrast modulation are the environmental factors with headache evidence; the UI must not add any. |
| Font family | Menlo by default, a one-line config change | No controlled study exists for any named coding font. The one monospaced face in Wilkins' 20-font analysis was the least striped, which is encouraging but unnamed. Judge Menlo against taller-x-height faces by eye. |

The config file's generated comments carry a short, non-diagnostic note: if headaches persist,
the literature puts a comprehensive eye examination that tests convergence and small
astigmatism, not just acuity, ahead of any setting, and lists the red flags that mean a
physician first. The README links the research doc.

## 8. Browser pane (Chrome docking)

A browser tab's content area is a placeholder rectangle; a real Chrome window is kept
positioned exactly over it, so it reads as embedded.

**Which window.** Claude Code's browser tools bring their own windows. The Claude in Chrome
extension works inside an "MCP tab group" and, when none exists, creates a new Chrome window
to hold it; the Chrome DevTools MCP launches its own Chrome instance. So Claude already has a
default window, and arkos docks that one rather than launching a window and hoping Claude
uses it. Nothing has to be said to Claude. The rule: while any session in the workspace is in
the **working** state (section 5) and a browser pane exists with nothing docked, the first new
Chrome window to appear is docked automatically. A palette command, "Dock a Chrome window",
lists open Chrome windows for the keyboard-only case, and "Undock" releases one. A browser pane can
also be opened by the user with a URL, in which case arkos launches a normal Chrome window
(not app mode, so the tab strip stays and tabs Claude adds remain visible) in the existing
profile and docks it.

**The unit is a window, never a Chrome tab.** A docked Chrome window keeps its own tab strip
at the top of the pane with every tab it had; switching tabs happens in Chrome, with Chrome's
shortcuts when that window has focus. arkos has one browser tab per docked window, titled
after the active Chrome tab (the window title). Exploding a window's tabs into separate arkos
tabs was rejected: it would need one Chrome window per tab, tabs cannot be moved between
windows through the Accessibility API, and it would fight Chrome's tab groups, which is also
where Claude's tool keeps its tabs.

**Vertical space.** A normal Chrome window spends about 80 px on its tab strip and
toolbar. Two mitigations: the browser pane's first-run screen recommends Chrome's native
"Show Tabs Vertically" (a right-click option on the tab strip in current stable Chrome), which
removes the tab strip row and leaves only the toolbar; and when a browser tab is active, the
arkos tab bar for that pane collapses to a slim strip, since Chrome's toolbar already shows a
title. Stock Chrome offers no supported way to hide the toolbar in a windowed mode, and arkos
does not try. For a window the user opens by URL, the prompt offers "open as a single page",
which launches Chrome in app mode (thin title bar, no toolbar, one tab); windows Claude's tools
create are never app mode.

**Dock by dragging a window in.** Any independent Chrome window can be dragged into arkos.
While a Chrome window is being moved by the user (an AX window-moved notification with the
mouse button down, read from a global pointer monitor, which the Accessibility permission
already allows), and the pointer is over the arkos window, arkos shows the same five-zone
drop overlay used for tab drags (section 4). Releasing over a zone creates a browser tab in
that pane, or splits the pane, and docks the window there. Releasing elsewhere leaves the
window alone.

**Undock by dragging out.** Two symmetric gestures release a docked window: dragging the
browser tab out of its arkos tab bar and dropping it outside the arkos window, or dragging
the Chrome window's own title bar (which stays visible at the top of the pane) so that the
window leaves the pane rectangle by more than a threshold. Either way the Chrome window
becomes independent again at the drop position, the browser tab closes, and nothing in Chrome
changes. Dragging a Chrome tab out of a docked window's tab strip is Chrome's own gesture and
makes a new independent window as usual; the auto-dock rule ignores windows that appear while
the mouse button is down, so a user's drag-out is never re-captured.

**Identification.** Chrome windows are found through the Accessibility API (the Chrome
application's window list; titles are the active tab's title). The docked window is tracked by
its AX element, not its title, so tab changes do not lose it. Window creation is detected by
polling the window list at a low rate while a browser pane is waiting, and stopped otherwise.

**Tracking.** Whenever the placeholder's screen rectangle changes (split drag, sidebar toggle,
window move or resize), arkos writes the Chrome window's `AXPosition` and `AXSize`. When the
browser tab is not active in its pane, or the arkos window is minimized or hidden, the Chrome
window is hidden. Closing the tab undocks the window and leaves it where it is; it does not
close Claude's window.

**Focus and stacking.** Clicking the placeholder raises and focuses the Chrome window.
Clicking elsewhere in arkos raises the arkos window and immediately re-raises the Chrome
window above it. A brief flash during that re-raise is the accepted seam.

**Permissions.** The first browser pane triggers the one-time Accessibility permission prompt,
with an explanation screen. Without permission the pane shows instructions instead.

**Rulings, 2026-10-15** (taken while planning the implementation):

1. Hiding a docked window moves it off every display, to x = primary width + 64 at its current y and size, and remembers where it was; showing it moves it back onto the placeholder. `AXMinimized` would animate to the Dock on every tab switch and app hiding would hide all of Chrome.
2. Drag-in detection polls the Chrome windows' frames (20 Hz while the mouse button is down and a browser pane exists) instead of using an `AXObserver`: a frame that changes between two ticks while the button is down is a drag. The same loop serves tracking.
3. Geometry comes from AppKit (the arkos `NSWindow`'s content rect in screen coordinates, flipped against the primary display's height), not from gpui's `window_bounds()`.
4. The Accessibility prompt for a development binary can only be answered at the screen, so `arkos browser-probe` is run once by the user to confirm rulings 1 and 3 and the raise behaviour. The engine is pure and tested with a fake.
5. A window asked for by URL is launched with `open -na "Google Chrome" --args --new-window <url>` (or `--app=<url>` for a single page); the first new Chrome window within 10 s docks into that tab regardless of the working state, after which the tab waits like any other.
6. Persisted state is the URL and the app-mode flag only. After a relaunch a browser tab waits; it never looks for the old window by title.
7. The slim tab bar is 14 px: the pane number and one dot per tab, no titles. It stays a drop target for tab drags.

## 9. Resilience and performance budgets

- Never block the UI thread: socket reads, PTY parsing, git, file reads and watchers run on
  background tasks; the UI thread applies parsed state and draws. Channels are bounded;
  producers coalesce rather than queue.
- Degrade under load, then recover: late frames draw current state; animations are
  time-based; the watchdog covers a stuck UI.
- Memory budgets, measured in milestone 6: UI idle with four terminals under **120 MB**
  resident; daemon under **15 MB**. Scrollback is the only unbounded input and is capped per
  tab. The tree holds only expanded directories. A viewer holds one file; background viewer
  tabs drop highlighting state.
- Daemon rules as in section 2.1.

## 10. Testing

- Daemon protocol: unit tests for framing and every message type; an integration test that
  spawns a real shell, writes a command, reads output, detaches, reattaches, and confirms the
  replay buffer reproduces the screen.
- Layout tree: property tests for split, close, move and ratio operations; the tree stays
  valid and every pane stays reachable by Cmd+1 to 9 numbering.
- Terminal rendering: headless snapshot tests of grid to glyph layout for fixed byte
  sequences, including box drawing and wide characters.
- Smooth scroll: a deterministic test of the animation curve against time; a manual check
  with a 100,000 line scrollback.
- Watchdog: an integration test freezes a fake client's heartbeat and confirms the daemon
  relaunches it with sessions intact.
- Chrome docking and the Claude Code hooks: manual checklist (they depend on other apps).

## 11. Milestones

1. **Daemon plus one terminal.** Daemon, protocol, a GPUI window with one terminal tab,
   smooth scrolling, reattach after a UI kill. Proves GPUI with the emulator, and session
   survival.
2. **Layout and tabs.** Split tree, tabs, drag and drop, keyboard map, workspace
   persistence, the Navy theme sampled from the screenshot, config with live reload.
3. **Session awareness.** Claude Code hooks, tab state icons with the slow pulse, status bar
   count, empty states.
4. **Sidebar.** File tree, lazy worktree explorer, the viewer with diff gutters.
5. **Browser pane.** Chrome docking through the Accessibility API.
6. **Polish.** Navy Light, the "size for my distance" helper, the break reminder, watchdog
   hardening, memory measurement against budgets, whatever the first weeks of daily use turn up.

Each milestone ends with Cyrus using it for real work before the next starts.

## 12. Repository layout

```
ide-arkos/
  Cargo.toml            workspace
  crates/
    protocol/           message types, framing (shared)
    arkosd/            the daemon
    arkos/             the GPUI app and the `notify` hook CLI
  docs/
    superpowers/specs/  this spec and later ones
    superpowers/plans/  implementation plans
    reference/          the VS Code screenshot the theme is sampled from
```
