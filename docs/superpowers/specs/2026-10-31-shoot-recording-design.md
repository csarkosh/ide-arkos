# Shoot recordings: the window through its themes, as a loop

**Status:** approved by Cyrus, 2026-10-31 ("I want it to be an actual recording"), for the Arkos
IDE page on apps.csarko.sh, whose "Who it's for" section shows the IDE's themes.

## What

A storyboard may ask for a recording as well as its stills: the same real window, after the
shot, switched through a list of built-in themes, each held a few seconds, and back to the
first. The result is one short H.264 file that loops without a cut, so a page can play it with
`autoplay muted loop` and show what a still cannot: the theme cross-fade, the runic rain and
decode, the watercolor washes.

## How

- `[[recording]]` in a storyboard: `name`, the same `wait`, `changed`, `after_secs` and
  `timeout_secs` a shot has, `themes` (two or more built-in theme names), `hold_secs` (4 by
  default), `fps` (30), `seam_secs` (1) and `width` (1320). It runs after every `[[shot]]`.
- The window is recorded by its id through ScreenCaptureKit's recording output
  (`scripts/shoot/record.swift`, macOS 15 and later, compiled once a run with `swiftc`): the
  window alone, no shadow, no pointer, whatever lies over it. The tool switches themes by
  rewriting the shoot's config, which the window reloads and cross-fades (THEME_FADE), so the
  transitions are the IDE's own. A theme's `appearance` (from `crates/arkos/themes/<name>.toml`)
  sets the mode, so light themes draw as light.
- `scripts/shoot/loop.py` makes the loop with ffmpeg: one period (the holds together) long,
  the tail after the period (the return to the first theme) cross-faded into the opening
  seconds, re-timed at a constant rate and scaled to at most `width` across. It writes
  `docs/images/<scene>-<name>.mp4` and the first frame as `<scene>-<name>.jpg`.
- `[effect]` may now be given more than once (`[[effect]]`): a recording through themes with
  different effects wants each at the runner's scale.
- The shoot workflow keeps the recordings with the stills, and installs ffmpeg if the runner
  lacks it.

## Not in this

A recording of anything but theme changes (typing, clicks over time), sound, or a GIF. The
README stays stills: GitHub plays no video from a repository.
