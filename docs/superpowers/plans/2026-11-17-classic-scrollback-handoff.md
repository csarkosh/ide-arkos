# Handoff (2026-11-17): scroll up through a whole Claude Code session, and The Matrix's city

Cyrus: "The main objective of this IDE is to be durable. This fails if I cannot scroll back in the
message history when Claude or this IDE is having some sort of issue." And: "From a user's
point-of-view, they just want to scroll up. They don't want a separate pane / tab opening up because
of technical issues." He approved: **prototype it in the sandbox**.

## Root cause (established, with evidence)

1. Both Claude config folders set `"tui": "fullscreen"` (`~/.claude3/settings.json:108`,
   `~/.claude/settings.json:160`). Fullscreen draws on the alternate screen (`ESC[?1049h`) and turns
   on mouse reports (`?1000h ?1002h ?1003h ?1006h`). Arkos then sends the wheel to Claude Code as
   reports (`crates/arkos/src/terminal.rs:1018`, `Terminal::wheel`); Shift+wheel scrolls Arkos's own
   history, which the alternate screen never fills. So in fullscreen the only scrollback is the one
   Claude Code holds in memory.
2. Claude Code's docs (code.claude.com/docs/en/fullscreen and /sessions): after a resume, "scrolling
   up stops at the `Conversation compacted` row, and your earlier messages stay on disk in the
   transcript file."
3. Session 0b8c66c2 (Arkos session 117526115109699624) was resumed by Arkos's recovery after the
   0.8.50 update (~19:49; Claude Code 2.1.327 had installed at 18:34) and compacted at 20:20, so it
   could only scroll back to 20:20. Evidence: `~/Library/Application Support/arkos/sessions/<id>.replay`
   (the daemon's saved output): one `?1049h` right after `── resumed after restart ──`, never left.
   All five live sessions show the same terminal modes; this one just compacted most recently.
4. Arkos's own durable layers are bypassed in fullscreen: the pane's history (`SCROLLBACK = 20_000`
   lines, `crates/arkos/src/sessions.rs:19`) only fills from the main screen, and the daemon's replay
   ring (`DEFAULT_REPLAY_BYTES` 16 MiB, `crates/arkosd/src/main.rs:18`, `ring.rs`) holds alternate-
   screen repaints; three sessions' rings are already full.

## Why fullscreen was chosen (do not regress this)

From the Oct 15 to 23 sessions (prompt history `~/.claude3/history.jsonl`, `~/.claude/history.jsonl`;
transcript `~/.claude/projects/-Users-csarko-Projects/c7f3ed54-98b5-4727-9f30-bc6e3acbd1de.jsonl`
around 2026-10-23T00:42Z): in the classic renderer (`/tui default`) the input box broke when a pane
was split or narrowed. Classic Claude Code erases its last frame with cursor-up and erase-line
counted at the old width; Arkos reflows the main screen on resize (alacritty `Term::resize`), so a
full-width border becomes two rows, the count falls short, and stale fragments stay above the new
frame. It was reproduced headlessly then (a three-line frame moved from 40 to 30 columns). Fullscreen
redraws the whole screen and has none of it. (A separate bug, shells without a controlling terminal,
was fixed in 0.6.2.) Scroll smoothness came from `wheelScrollAccelerationEnabled: false`, kept in both
settings files.

## The plan to prototype (sandbox first, nothing on main until Cyrus has tried it)

Goal: Claude Code in Arkos uses the classic renderer, so its output lands in Arkos's own scrollback
and the daemon's replay; the user scrolls up in place through the whole session, across compactions,
Claude Code restarts and Arkos updates; resizing a pane leaves no stale fragments.

1. **Classic for Arkos's Claude sessions.** Spawn sessions with
   `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1` (it takes precedence over the `tui` setting and
   `CLAUDE_CODE_NO_FLICKER`); env is built in `crates/arkosd/src/session.rs` near line 380. The user's
   settings files stay as they are. Decide whether this is a setting (default on) and document it.
2. **Resize without stale fragments.** Write the failing headless test first (the 40 to 30 column
   repro, then Claude Code's erase-and-redraw) in `terminal.rs` tests, beside
   `resize_reflows_and_reports_modes` (line ~1772); `TerminalCore::resize` is at line ~299. Candidate
   fix: reflow only the history, not the live screen rows a program is redrawing (or clear the rows
   below the cursor's frame on resize). Check against a real classic Claude Code in the sandbox:
   split, narrow, widen, and the input box must stay whole.
3. **Nothing clears the history.** Check whether classic Claude Code sends `ESC[3J` (erase saved
   lines) or `ESC[2J` on compaction, `/clear`, resume or Ctrl+L. If it does, decide with Cyrus whether
   Arkos ignores `3J` for these sessions (a durability rule) and test it.
4. **Restart and update survive.** After `arkos sandbox stop` and relaunch, the old conversation must
   sit above the `── resumed after restart ──` divider and be reachable by scrolling. Look at whether
   20,000 lines and 16 MiB are enough for a day-long session of classic output (classic redraws are
   heavy: measure bytes per hour from a real session) and raise or restructure them if not (for
   example, keep scrolled-off lines rather than raw bytes).
5. **Scrolling feels smooth** in the classic renderer: trackpad 1:1 in Arkos's own history (0.2.9
   work), no jitter.

Report to Cyrus with what he can see: before and after stills or a short shoot of a split pane and
of scrolling up past a compaction.

## Rules that bit this session

- Read `CLAUDE.md` and `docs/development.md` first. Tests written first, named as sentences;
  `nice -n 15 ~/.cargo/bin/cargo`, one at a time, never `--release`; finish with
  `cargo check --workspace --all-targets` and no warnings.
- Never run `target/debug/arkos` with no subcommand: it opens a window and blocks. Sandbox:
  `target/debug/arkos sandbox stop`, `(target/debug/arkos sandbox /Users/csarko/Projects/ide-arkos >/dev/null 2>&1 &)`;
  CLI against it with `ARKOS_CONFIG_DIR=$HOME/Library/Caches/arkos-sandbox-dev/config
  ARKOS_APP_SUPPORT=$HOME/Library/Caches/arkos-sandbox-dev/state`.
- Verify on screen yourself with a throwaway shoot story (`scripts/shoot/stories/zz-*.toml`,
  `OUT=<dir> scripts/shoot.sh <story>`, delete the story after); never hand the screen check back.
- Release only on "push to main and release" (release gate in memory: rebase on origin/main, ff,
  push, HEAD check, then `gh workflow run release-notarized`).

## The Matrix's Pane 0 city (built, unreleased, part of this handoff)

Cyrus's requests for the `runic` effect under the `matrix` theme (Pane 0's city), each done in
`crates/arkos/effects/runic/wash.metal` with Rust mirrors and tests in
`crates/arkos/src/effects/runic.rs`, documented in
`docs/superpowers/specs/2026-11-17-runic-world-design.md` (one section each) and the Matrix passage of
`docs/guide.md`. They are a stack of local branches on top of 0.8.50 (`e85c216`), each built on the
one before; `matrix-jaywalk` holds all four:

| Request (his words) | Branch, commit | What it does |
|---|---|---|
| "I want the sidewalks more prominent in the pane 0 graphics" | `matrix-sidewalks` 3122ed7 | Paved green-wash sidewalks with joints and a lit curb (SIDEWALK_* constants). |
| "the runic rain comes from users uploading and download data ... can move in the reverse direction ... goes to and originates from pedestrians, bicyclists, cars, food carts, and buildings." Chose: "All rain is data", "Direction only", "A steady trickle". | `matrix-data-rain` be21896 | Ambient rain removed. 14 slots each run a transfer every 5 to 9 s between the sky and a walker, car, cyclist, tower window or food cart, following it; channel shows faintly (DATA_CHANNEL 0.3), a bright packet runs down (download) or up (upload). Test: 8 to 13 at once. |
| "a subway train ... should appear dimmer because it runs underground and it should be obvious to the user that the train is underground. The subway train should move faster and without stop" | `matrix-subway` ea3480a | Dashed tunnel walls under the road; a five-car train every 14 s at 4 units/s (about 7x the cars), never stopping, alternating coming and going, drawn at SUB_DIM 0.38 before the street grid so the grid lies over it. |
| "in NYC, bicyclists never obey traffic laws. So allow them to break the traffic laws as long as another car does not hit them ... will run a red light even if they barely avoid an accident." Then: "Same thing with pedestrians." | `matrix-jaywalk` a2e4284 | Cyclists never stop: reach each stop line just after the last cross car clears (CLEAR_AFTER 9.4 s into the 12 s cycle) and ride through the red. JAYWALK 0.6 of walkers step off against the light in the same window. Cars keep every law. Test: no cyclist or walker is ever within reach of a moving cross car over six minutes; reds run thousands of times. |

State: all 257 `effects::` tests pass, no warnings, the runic wash is 3.0 ms a frame (bench), and
`arkos theme contrast matrix --effect runic` is clear. Checked only in off-screen stills
(`ARKOS_STILLS=<dir> ARKOS_STILLS_T=<t> ARKOS_STILLS_PAIRS=runic:matrix nice -n 15 cargo test -p arkos effects::stills::tests::stills -- --ignored`,
crop the Pane 0 city at (8,400,246,632) of the 1000x640 `matrix-sidebar.ppm`). Still to do:

- **Look at it moving in a real window** (sandbox with `theme use matrix`, or a throwaway shoot):
  the near-misses at the red, the train's speed, the rain's flow. Cyrus has not seen any of it yet.
- **Data rain density:** stills show only a few streams standing out at a moment though 8 to 13
  run; many are far or faint. If it reads sparse live, raise brightness or bias the targets nearer.
- Offer Cyrus a look in the sandbox, then release only on "push to main and release": rebase the
  stack (`matrix-jaywalk`) on origin/main and follow the release gate. If `classic-scrollback` lands
  first, rebase the stack on it.

## Other open work

- apps.csarko.sh still shows 0.8.49; the remote branch `reshoot-0849` was left on GitHub.
