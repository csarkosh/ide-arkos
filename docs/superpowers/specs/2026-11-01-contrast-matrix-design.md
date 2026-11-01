# Contrast matrix: every colour a terminal can draw, checked

**Status:** approved by Cyrus, 2026-11-01 ("build layer one and include the telemetry changes
too"), after Claude Code's stock dark diff drew near-white context lines on Lavender Light.

## Why

Every contrast check Arkos had started from colours Arkos chose: the theme file's pairs, the
derived palette, the Claude Code theme's tokens on its bands. None fed in a colour a program
sends. Claude Code on its stock dark theme sends its diff and message colours as RGB, which the
palette never sees, and nothing caught the near-white text it drew on a light theme. The
renderer now holds every glyph to 4.5:1 (`legible`); this keeps it so, and lets a theme or
effect author see the same checks.

## What

- **One function draws a cell's colour.** `contrast::drawn(fg, bg, dim)` is the colour the
  terminal paints a glyph: the floor (`legible`), then faint text's fade, composited over `bg`.
  `terminal_element::text_run` and the checks call it, so the checks test the painter.
- **The Claude Code pairs leave the tests.** `claude_theme::contrast_pairs(theme)` returns the
  rules the tests held the built-ins to (every text token on the background; the message text
  and `ansi:whiteBright` on every band; dim text on the bands it is drawn on at 3:1; inverse
  text on every text colour used as a fill), as `theme::Pair`s. The tests call it for the
  built-ins, and `arkos theme contrast` adds them to its rows, so a theme of one's own is held to
  them too.
- **The matrix.** `contrast::terminal_report(theme)` draws every foreground a program can send
  on every background it can send, plain and faint: the theme's terminal colours, the 256
  colours of its palette, and Claude Code's stock colours (all six stock themes and its diff
  renderer's own, from `crates/arkos/data/claude-code-colours.json`). It counts the pairs, those
  the floor had to move, and any drawn under 4.5:1 (3:1 faint), which can only be a renderer
  bug. `arkos theme contrast` prints its summary, `terminal: {n} pairs drawn, all clear`, and a LOW
  line per failure; the JSON adds how many the floor moved.
- **Effects.** An effect's `text_contrast` deepens the text. `arkos theme contrast <theme>
  --effect <name|none>` checks the theme as that effect shows it; without the flag, as the
  theme's own recommended effect shows it. The effect workflow in the agent guide gains the step.
- **The fixture.** `scripts/claude-code-colours.py` reads an installed Claude Code and writes
  the fixture (`--check` compares instead). An ignored test runs the check, so a Claude Code
  update that changes its colours shows up when someone runs it.
- **Telemetry.** A new event, `text_moved`, with `ground` (`light`, `dark`) and `on` (`paper`,
  `band`): the renderer had to move a program's colour to keep it readable, once per report for
  each pair of values. It says how often programs draw in colours meant for another ground,
  never which program or what text.

## Not in this

The surfaces table shared by painters and checks (the viewer's overlays, hover rows) and the
measured screen check (a shoot scene sampling glyphs over washes) are the next layers.
