# The window stays on screen through an update (proposal)

Cyrus, 2026-11-16: "Right now, it briefly disappears and reappears which makes it feels like it
crashed instead of a healthy auto-update. This can make users decide not to use it. Ideally, the
IDE looks like it remains open throughout the entire auto-update."

Status: built on branch `update-handoff` (2026-11-16) after the spike of section 4; section 6 says
what changed from the proposal and what was checked. Cyrus, the same day: "Run the spike, then
build it. The test should include preserving the user's text in Claude's user input box when an
update occurs. I NEVER want a situation where an update wipes-out the prompt that a user was
spending time writing-up. The unsubmitted prompt should appear in the new window."


## 1. Why it disappears today

Each window is its own process. An update runs in this order (`Root::relaunch_now`,
`Request::RelaunchClient`, the daemon's `ClientRecord::relaunch`):

1. The daemon swaps the bundle and execs itself onto the new `arkosd` (the window shows nothing
   of this: it reconnects in about 0.3 s).
2. The window asks the daemon to start the new binary **once this window disconnects**, flushes
   (unsaved edits, the workspace, its desktop; waits up to 0.5 s for the daemon to receive it)
   and quits. Its window leaves the screen at once.
3. The daemon starts the new `arkos` with `ARKOS_RELAUNCH=1`. It is a cold start: the process,
   Metal, fonts, the theme, the workspace, then each session attached and its replay parsed
   (about 0.1 s each for a 16 MB replay, from `window.log`).

Measured on 2026-11-16 with the installed 0.8.38 in a sandbox: about **300 ms** from the process
starting to its window being on screen, with one fresh session. A real update adds the replays
and the old window's flush, so nothing is on screen for roughly half a second to a second, and
the content arrives after the frame.

Making startup faster narrows the gap but can never close it: the old window is gone before the
new process exists.

## 2. The proposal: overlap, then hand off

The new window gets ready while the old one is still on screen, and the old one leaves only once
the new one is drawn in its place.

1. **Old window:** on the relaunch it flushes as now, stops saving the workspace, and stays on
   screen and live (its terminals keep updating). It sends `RelaunchClient { handoff: true }`.
2. **Daemon:** starts the new `arkos` at once (not on disconnect), with `ARKOS_RELAUNCH=1` and
   `ARKOS_HANDOFF=<old pid>`.
3. **New window:** starts as an accessory app (no Dock tile, no menu bar) with its window created
   hidden (gpui's `WindowOptions { show: false }`), on the old window's frame and desktop. It
   loads the workspace, attaches every session, parses the replays, and draws one full frame.
   Then it tells the daemon it is ready.
4. **The swap, within a frame:**
   - The new window is ordered in **directly below** the old one, with no animation.
   - The old window yields activation to the new process (macOS 14's cooperative activation,
     `yieldActivationToApplication:`) when it was frontmost, orders out with no animation, and
     exits.
   - The new process becomes a regular app (its Dock tile and menu bar), and takes key focus if
     the old window had it.

   Because the new window sits under the old one before the old one goes, there is no frame with
   neither on screen.
5. **If the new window never says it is ready** (10 s): the daemon ends it, and the old window
   stays. The rollback of today ("the previous app put back, your windows reopened") then
   happens with nothing disappearing.

What the user may still notice: a shader effect's animation jumps (the new process starts its
clock again), a blinking caret restarts, and the menu bar redraws once. Terminal content,
scroll positions, the page shown and the focused pane are the same, since they come from the
workspace and the replays.

## 3. What it touches

- Protocol: `RelaunchClient` gains `handoff`, plus a `Ready` request from the new window and a
  `HandOff` event to the old one; the protocol version goes up by one.
- Daemon: the immediate launch, the ready and timeout bookkeeping, and the update's confirm and
  rollback rules (today "a window not connected at the apply" confirms; the new window still
  does, but the old one now stays connected until the swap).
- Window: the hidden start, the accessory to regular switch, the swap, and "stop saving" once
  the handoff starts.
- The first update that ships this still goes the old way, because the window running then
  does not know how to hand off. Every update after it is seamless.

## 4. Spike first (half a day), on the installed app in a sandbox

- **Dock:** does an accessory second instance of the same bundle stay out of the Dock, and does
  its switch to regular as the old one exits make the tile blink? This matters when Arkos is not
  kept in the Dock.
- **Activation:** does the yield-then-activate hand focus over on macOS 26 with no flash of
  another app?
- **Ordering:** a hidden gpui window ordered in below another at the same frame, with no
  animation, then the top one ordered out: no blank frame (checked with a screen recording at
  60 fps).
- **Full-screen windows:** a full-screen window has a Space of its own, and putting a new
  window there means the full-screen animation. Likely answer: a full-screen window keeps
  today's relaunch.

## 5. Not chosen

- **A screenshot cover** (the old window saves a picture that something shows until the new
  window is up): the picture is frozen, animated themes stop, and a process still has to start
  before the picture can be shown, so a gap remains.
- **Faster startup alone:** worth doing, but it shortens the gap rather than removing it.

## 6. As built, and what was checked

The spike (two processes of one test bundle, the window list sampled every millisecond, since
Screen Recording was not granted to the session) settled section 4:

- A window at alpha 0 counts as visible to macOS, so its display link runs. The successor is put
  **in front** of the old window, transparent and ignoring the mouse, rather than under it: under
  a covering window macOS may stop drawing it.
- `yieldActivation` followed by the new app's own `activate` did not hand the focus over; the
  old app calling `activateFromApplication:options:` on the new one did. The old window does it
  only when its app is the one in use.
- An accessory app (set before it finishes launching) has no Dock tile; turning regular as it
  shows itself adds its tile as the old app's goes. gpui made every app regular at launch and
  creates its `NSApplication` inside `run`, so the fork gained two small hooks (`v0.2.2-arkos.8`,
  `v0.2.2-arkos.9`, `gpui::launch_as_accessory`).
- Full screen: not built; such a window, and a minimized one, relaunches the old way.

One rule was added once the real window ran: macOS draws nothing it does not show, so a
successor whose window is on a desktop not in front (or wholly covered) never restores its
terminals. Nobody can see it either, so when macOS has not called it visible within half a
second it takes over at once (`handoff::OFF_SCREEN_AFTER`); its terminals restore when the user
goes to that desktop.

Protocol 12 carries it: `RelaunchHandoff { exe, cwd }`, `HandoffReady { from }`,
`Event::HandOff { to }`, `Event::HandoffFailed { reason }`. The successor learns its predecessor
from `ARKOS_HANDOFF`. The old window saves everything as it asks, then saves nothing more (the
successor read that save and owns the file); it falls back to the old relaunch when the daemon
refuses, when the successor ends before it is drawn, or after 20 s.

**The prompt.** Claude Code runs in the daemon's pty and never stops, so a prompt typed and not
sent lives on in it; the successor's terminal is rebuilt from the session's replay, as every
relaunch has been. Checked in a dev sandbox with `claude` running, a draft typed into its input
box through the daemon's socket and never sent, and the relaunch `ARKOS_RELAUNCH_AFTER` makes:
the successor's log said "handoff check: session … shows the text 1 times on screen"
(`ARKOS_HANDOFF_CHECK`), on screen in 271 ms and 559 ms. What the handoff cannot protect is a
daemon that dies during its own exec: its shells, Claude Code included, end with it, and the
recovery resumes the conversation without the unsent prompt. That path is the daemon upgrade's,
unchanged here.

On screen (sandbox, sampled): no sample without an opaque window; the successor an accessory
until it showed itself; the desktop in front unchanged in every run sampled for it.
