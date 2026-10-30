# Matrix theme and effect

Status: design agreed with Cyrus in chat, 2026-10-29; implemented on branch matrix, awaiting
Cyrus's look. Builds on the effects design (`2026-10-28-effects-design.md`), the space effect
design (`2026-10-28-space-effect-design.md`) and everything shipped through 0.7.13.

## Intent

A theme and an effect that make Arkos feel like hacking into the Matrix. Cyrus's words: "a
black background with emissive green text running like I'm hacking into the matrix. Maybe some
faint green text running in the background as soft background noise." The text itself moves: it
scrambles while new output arrives or Claude is working, and barely at all when things are idle.

This is the second effect to move text. The first, space's shimmer, was removed because Cyrus
found it hard on the eyes when the text was idle. The balance between alive and readable is to
be found by eye, in a dev window, by iterating on the constants: every rate and strength in this
document is a starting point, named in one place, and expected to change.

## Goals

1. A built-in theme `matrix` (`crates/arkos/themes/matrix.toml`): a black ground and bright
   green text, passing the theme contrast rules, recommending the effect `matrix`.
2. A built-in effect `matrix` (`crates/arkos/effects/matrix/`: `effect.toml`, `wash.metal`,
   `glyphs.metal`), listed by `arkos effect list` and Settings beside `watercolor` and `space`.
3. The text, everywhere in the project window, drawn as emissive green with a soft halo, and an
   idle glitch: now and then one letter flashes and shows a different shape for a fraction of a
   second.
4. The terminal, on top of that: a cell whose character changes shows random characters for a
   few frames before settling, longer and busier while that tab's Claude session is Working.
   Freshly settled text starts brighter and cools to the normal green.
5. The wash: black, with faint columns of rune-like shapes falling at different speeds, dim
   enough to read as background noise; a thin green line on the pane edges and menus.
6. The scramble is an effect capability, not a theme feature: a new manifest key asks the
   terminal for it, as `glyph_motion` asks for the clock. Any effect may use it.

## Non-goals

- No real letter swaps in the UI text (sidebar, tabs, markdown, palette). Those glyphs are
  drawn by dozens of elements, and the glyph shader cannot fetch another letter from the atlas,
  so the UI's glitch is the shader's shape trick (below), not a character change.
- No new gpui fork patch. The hooks that exist (the glyph shader, its clock at buffer 5, the
  emoji stages) are enough.
- No scramble on web pages (WebKit draws them), on images, or on the aux windows.
- No sound; no reaction to the pointer; no change to watercolor or space.

## Anti-goals (a failure even if it works)

- Text that is hard to read at rest. The idle glitch is rare and brief; between glitches every
  letter is still and whole. If Cyrus finds the idle state tiring, the idle rate goes down or to
  zero before anything else changes.
- Scramble that hides what Claude said: a cell always settles on its real character within the
  decode time, and a cell whose character did not change is never touched by the decode.
- A window that costs noticeable CPU while idle and visible. The text's timer runs at a low
  rate (below), and the CPU is measured as the space design measured it.
- Motion under Reduce Motion: the clock freezes (still rain, no glitches) and the decode settles
  at once.
- Any of the floors the guide sets broken: the stroke's core keeps at least 55% of its coverage,
  the paper stays at or above 0.5, and the wash stays within the 4 ms budget at half scale.

## Constraints

- The rules in `CLAUDE.md` and `docs/development.md` hold, with those of the 2026-10-29 hand-off
  (`~/Desktop/ide.md`, outside the repo): `nice -n 15`, no `--release`, one cargo at a time,
  tests before code, headless tests named as sentences, `cargo check --workspace --all-targets`
  clean, doc comments without em-dashes, motion through `motion.rs`, and every shader in its own
  `.metal` file loaded with `include_str!`.
- Dev windows run as a separate instance, never against Cyrus's real config or daemon: `theme
  use` in a dev window would rewrite his Claude Code colours, and the daemon under the real
  Application Support holds his sessions. The way is `arkos sandbox`: `target/debug/arkos sandbox
  <project folder>` opens the dev build's own sandbox (`~/Library/Caches/arkos-sandbox-dev`, its
  config copied once from his, no Claude Code colours, hooks or skill written), and
  `target/debug/arkos sandbox theme use matrix`, `target/debug/arkos sandbox effect use matrix`
  and `target/debug/arkos sandbox effect use none` drive that instance alone;
  `target/debug/arkos sandbox stop` ends it. For a build older than the command, the fallback is
  this script, kept in the scratchpad (never the repo):

  ```sh
  #!/bin/zsh
  export XDG_CONFIG_HOME=/Users/csarko/Library/Caches/arkos-spike-config
  export ARKOS_APP_SUPPORT=/Users/csarko/Library/Caches/arkos-spike
  exec /Users/csarko/Projects/ide-arkos/target/debug/arkos "$@"
  ```

  `<script> <project folder>` opens a window; `<script> theme use matrix`, `<script> effect use
  matrix` and `<script> effect use none` drive that instance alone. The script's config is Lavender
  Light with watercolor forced on today; `theme use matrix` and `effect use theme` set it up.
- Shoots run on GitHub runners only.
- The branch is `matrix`, in the worktree `~/Projects/ide-arkos/.claude/worktrees/matrix`, off
  `9dc1c76 Release 0.7.13`.

## The theme (`crates/arkos/themes/matrix.toml`)

- `window_bg` and `terminal.background` are black (`#000000`). The bars, the sidebar and the
  inactive tabs are a very dark green-black (around `#060b07`), so the window still has shape
  with the effect off. `tab_active` is black.
- `terminal.foreground`, `chrome.text` and the focus ring are phosphor greens: a bright one for
  the text (around `#4af626`, which clears the 11:1 terminal rule and 7:1 chrome rule on black
  by a wide margin), a deeper one for `text_dim` (around `#2e9e3a`, at least 4.5:1), and the
  bright green as `focus_ring`. The cursor is the text green; the selection a dark green.
- The ANSI colours keep their meaning (red stays red, yellow yellow) so a program's output still
  reads, but each leans toward the phosphor family and sits at 4.5:1 or better on black.
  `semantic.working` is the bright green, `semantic.waiting` an amber, as in Navy's roles.
- Every colour drawn as text (`chrome.text`, `text_dim`, `terminal.foreground`, the sixteen ANSI
  colours, the `semantic`, `tree` and `viewer` colours) has an sRGB luminance (`0.2126 r + 0.7152
  g + 0.0722 b` of the components as the shader sees them) of at least 0.47, so none of it takes
  the glyph shader's dark-text branch (threshold 0.45, below) and all of it glows. A theme test
  asserts this for matrix (amended 2026-10-29, see the end).
- `[effect] name = "matrix"`.
- The exact values are the builder's, found with `arkos theme contrast matrix` until every pair
  passes. The theme test that holds every built-in to the rules covers it.
- The `BUILTIN` list, Settings, `docs/themes.md`, `docs/guide.md`, `docs/agent-guide.md` and the
  config template's list of built-ins name it.

## The effect's manifest (`matrix/effect.toml`)

`frame_rate = 30`, `scale = 0.5`, `ui_text_bump = 0`, `ink_borders = true`,
`highlights = "effect"`, `menus = "glow"`, `glyph_motion = true`, `paper_alpha = 0.65`,
`document_paper_alpha = 0.92` (amended 2026-10-29, see the end),
`chrome_alpha = 0.0`, `active_tab_alpha = 0.3`, `text_contrast = 0.0`, and the two new keys
below.

### New manifest keys (the contract grows by two keys)

| Key | Range | Default | What it does |
| --- | --- | --- | --- |
| `glyph_rate` | 1 to 20 | `frame_rate` capped at 20 | The frames a second the text is redrawn at while `glyph_motion` is on. Matrix sets 10: a glitch that lasts a tenth of a second needs no more, and a whole-window redraw at 20 a second was the space design's CPU worry. |
| `terminal_decode` | true, false | false | The terminal decodes changed cells (below) and brightens fresh ones while this effect is settled on. Off, the terminal draws as always. Requires `glyph_motion = true`: the decode has no timer of its own, so a manifest with decode on and motion off is invalid, with an error naming both keys. |

`glyph_wait` takes `glyph_rate` in place of the capped `frame_rate`. Both keys are documented
in the agent guide's manifest table (the only place the keys are listed; `arkos effect contract`
prints the shader contract, not the manifest). An unknown key still makes an effect invalid, so
an older Arkos refuses a folder that uses them, which is the existing behaviour for any new key.

## The text (`matrix/glyphs.metal`)

Starts from space's `glyphs.metal` (the quad widened by the halo's reach, samples kept inside
the glyph's own tile, everything measured in points through `ARKOS_SCALE`, the 55% core floor,
the emoji stages at the end) with these changes:

- **Colour.** The hue is a phosphor green, `(0.35, 1.0, 0.45)`. The tint is stronger than
  space's (around 0.45, keep 0.15), so coloured ANSI text still shows its colour but reads as
  green-cast. No scanlines. Brightness 1.0: the letter is the theme's colour at full light.
- **Halo.** Space's three rings at a glow around 1.6, in the glyph's own colour mixed toward the
  green. The halo scales with the glyph's luminance, so a fresh, brightened cell (below) glows
  more than a settled one with no other signal from the app.
- **Dark text.** As space, but with the threshold at 0.45 rather than 0.5: a glyph whose colour
  has luminance under 0.45 (a light theme with matrix chosen) leans toward a deep green `(0.0,
  0.4, 0.15)`, keeps its brightness and draws no halo. The shader knows only the glyph's colour,
  so the matrix theme keeps every text colour at 0.47 or more (theme section) and all of its
  text, coloured ANSI output included, takes the emissive branch (amended 2026-10-29, see the
  end).
- **The idle glitch.** The shader reads the clock (`constant float *time [[buffer(5)]]`) in the
  vertex stage. Time is cut into slots of `GLITCH_SLOT` = 0.1 s. For each glyph and slot a hash
  of the glyph's position (its bounds origin in device pixels, which is stable for a letter that
  stays put) and the slot number says whether it glitches, with probability `GLITCH_CHANCE`
  such that a window of about 3,000 visible glyphs shows roughly 3 glitches a second: ten slots
  a second, so `GLITCH_CHANCE` = 0.0001 per glyph per slot. A glitching glyph:
  - samples its tile mirrored, horizontally, vertically or both, picked by the same hash, so it
    shows a shape that is not its letter but has the letter's weight and size (the atlas holds
    nothing else it could safely show);
  - is lit at `GLITCH_FLASH` = 1.4 times its light, the halo with it;
  - lasts one slot. A second slot in a row for the same glyph is not prevented, but at this
    chance it is rare.
  - The hash is `hash01` from space's wash (PCG's output step, integer arithmetic), so a Rust
    mirror agrees to the bit.
- **Reading text is calm** (amended 2026-10-29, see the end). The rendered markdown view marks
  its text with the alpha `CALM_ALPHA` = 0.995 (`effects::active`), which the window declares
  to every glyph shader as `ARKOS_CALM_ALPHA`. A glyph whose alpha is within 0.002 of it (after
  any fade) leans toward a soft pale green `CALM_GREEN` = `(0.72, 0.86, 0.72)` by `CALM_TINT` =
  0.6, at `CALM_BRIGHTNESS` = 0.9, with no halo and no glitch (the vertex stage gives it no
  mirror and no flash). Dark reading text (a light theme) is the dark text above, without the
  glitch. Emoji carry no mark and stay on the display.
- **Emoji** go on the same display: their own colours tinted toward the green by the same
  tint, haloed, and glitching by the same rule.
- **Under Reduce Motion** the clock is frozen and no glyph timer runs, so whichever glyphs the
  frozen slot picks would glitch forever. The vertex stage therefore reads a second value the
  app already controls: the clock is frozen at exactly 0.0 under Reduce Motion (`ActiveEffect::
  time` starts there and never advances), and slot 0 glitches nothing.
- All the constants (`GLITCH_SLOT`, `GLITCH_CHANCE`, `GLITCH_FLASH`, the hue, tint, keep, glow
  and reach) sit at the top of the file with one comment each, so tuning is a one-line edit.

## The terminal's decode (app side, `terminal_decode`)

The terminal is the only text the app draws from a grid it owns, so it is the only text whose
letters can really change. The decode lives in a new module `crates/arkos/src/effects/decode.rs`
(the pure rules, tested) and is applied in `TerminalElement::paint` as the rows are built.

- **What the terminal remembers.** Per visible cell, the character last painted and the instant
  it last changed (`Decode` state on `Terminal`, sized to the screen, reset on resize and when
  the display offset changes, since scrolled history is not fresh output). It is kept only while
  `terminal_decode` is on; turning the effect off drops it.
- **What counts as a change.** A cell whose character differs from the one last painted, where
  the new character is not a space. A cell that became a space just clears. A cell whose
  character is the same is never touched, whatever happened around it.
- **Decoding.** For `DECODE_MS` after the change the cell is drawn with a random character
  instead of its own, a new one each frame, from a fixed string of ASCII letters, digits and
  symbols (`DECODE_CHARS`), picked by a hash of the cell's position, its change instant and the
  frame, so the frames differ but a test can predict them. Only ASCII, so the glyph comes from
  the terminal's own font at the cell's width. A wide character (`CellKind::Wide`) decodes as one
  ASCII character drawn in its two-column cell (the kind is kept); a spacer never decodes
  (amended 2026-10-29, see the end).
- **Working and idle.** The element is told whether the tab's session is `ClaudeState::Working`
  (pane_view has the state; it already derives the tab mark from it). Working:
  `DECODE_MS_WORKING` = 400 ms. Not working: `DECODE_MS_IDLE` = 120 ms. At `glyph_rate` 10 that
  is four frames of random characters, or one. The idle decode is what a prompt's redraw shows:
  a brief flicker where the text changed.
- **The user's own input and scrolling settle at once.** For `USER_QUIET_MS` = 400 ms after the
  user's own key press, paste, mouse report or scroll in a terminal (`Terminal::last_user_input`,
  every mode, local scrollback included), a cell that changes is settled at once: no scramble and
  no brightening, so the echo of typing and the redraw a fullscreen program (Claude Code) makes
  for the wheel look like the rest of the text. A cell already decoding when the window opens
  keeps decoding. While the user scrolls a terminal or a document (the wheel, a keyboard page
  scroll or a drag past an edge), the glyph clock rests at 0.0 for `SCROLL_QUIET_MS` = 400 ms
  (`effects::active::glyph_clock`), where slot 0
  glitches nothing, so moving glyphs do not re-roll their glitch each frame (amended 2026-10-29,
  see the end).
- **Fresh text.** For `FRESH_MS` = 1500 ms after a cell settles, its foreground is mixed toward
  white-green `(0.85, 1.0, 0.85)` by a share that falls from `FRESH_MIX` = 0.6 to 0 over that
  time; a foreground the shader draws as dark text deepens toward `FRESH_DEEP` instead, and an
  inverse cell's background (its text colour once swapped) is the one mixed (amended 2026-10-29,
  see the end). The shader's halo follows the brightness, so new output glows and cools. A cell whose
  colour is not the terminal's foreground (ANSI-coloured text) brightens the same way.
- **Frames.** The glyph timer at `glyph_rate` already redraws the window while the effect is
  settled and seen, so the decode and the cooling need no timer of their own. Between frames
  nothing is computed.
- **Reduce Motion.** `DECODE_MS` and `FRESH_MS` are 0: cells settle at once with their own
  colour.
- **Scrolling output.** A line that scrolls up is recognised and does not scramble again: a row
  whose text (trailing spaces left out) is not blank and was on screen before takes that
  remembered row's cells whole, each with its own change instant, so a line still scrambling
  keeps scrambling as it moves and a settled one stays settled. The remembered row at the same
  place is taken first, then the nearest below (output scrolls up), then the nearest above. A
  blank row is never taken for a moved one; any row that matches none is diffed cell by cell.
  At most `DECODE_ROWS` = 3 rows scramble at once, the newest: past that the oldest settle at
  once (their change instants set back by the decode time, so they keep cooling), and among
  rows that changed in the same frame the lowest on screen is the newest. While Claude streams,
  only the newest few lines scramble and the rest of the screen stays still (amended
  2026-10-30, see the end).
- All the constants (`DECODE_MS_WORKING`, `DECODE_MS_IDLE`, `FRESH_MS`, `FRESH_MIX`,
  `DECODE_CHARS`, `DECODE_ROWS`) live in `decode.rs` with one comment each.

## The wash (`matrix/wash.metal`)

Everything derives from the theme's colours, so matrix chosen with another theme reads in that
theme's green: the greenest of its `ANSI green`, its terminal text and its focus ring (amended
2026-10-29, see the end).

- **Ground.** `ARKOS_WINDOW_BG`, flat.
- **Rain.** The window is cut into columns `RAIN_COL` = 14 points wide and rows `RAIN_ROW` = 18
  points tall. Each column (by a hash of its index) has a speed between `RAIN_SPEED_MIN` = 2 and
  `RAIN_SPEED_MAX` = 6 rows a second, a trail length between 8 and 24 rows, a phase, and a gap
  of a few seconds between its streams, so at any moment about three fifths of the columns carry a
  stream (a third until the 2026-10-29 more-rain amendment). A stream is a head cell and a trail that fades to nothing behind it. Each cell in a
  stream shows a rune: a 5 by 7 bit pattern from a hash of the cell's position and a slow clock
  (`RUNE_CHANGE` = 0.5 s, so the runes flicker as the film's do), drawn as filled bits with soft
  edges, in the theme's green. The head is brighter and a touch whiter; the trail is the green
  at `RAIN_ALPHA` = 0.22 at most (0.15, and 0.12 before that, until Cyrus tuned the edges), times `u.intensity`, so
  the rain is noise under the text and never competes with it. There is no font in the wash, so
  these are drawn shapes, not letters.
- **Where it shows.** The rain covers the whole window. The terminal panes' paper at 0.65 hides most of
  it under the text; the bars and the sidebar (`chrome_alpha` 0) show it through their own faint
  darkening, as space darkens its bars; the tab strips are not darkened and carry a second,
  fainter layer of rain (`TAB_RAIN` = 0.4).
- **Edges (`ink_borders`).** Every edge a block reports is live, in the theme's green, all of
  its light scaled by `EDGE_INTENSITY` = 0.33 (and then by `u.intensity`): a core 0.55 points
  wide and a halo reaching 5.2 points; code falling down the vertical edges, one stream an edge
  of 7-point runes with a brighter, whiter head; the horizontal edges' line drawn as small
  upright 6-point runes flipping every 0.5 s, the core mostly replaced; now and then (about 6 a
  minute across the window) a 40-point stretch of an edge tears 2 points sideways and flashes
  for 0.12 s; and a pane whose active tab's Claude session works (the block's `busy` flag) has
  edges that stream twice as fast and burn 1.4 times brighter. The values are Cyrus's (see the
  amendment of 2026-10-29, "Cyrus tuned the edges"). Media and tables get the same edges. The
  same functions serve `arkos_overlay`, so the menus' edges are the panes' own (never busy).
- **Highlights (`highlights = "effect"`).** A soft green fill on the selected and hovered rows
  and the hovered tab, the strengths by state as watercolor's.
- **Menus (`menus = "glow"`).** The backdrop is the wash over the menu's place, as space's.
- **Time.** Every stream is a pure function of `u.time`; a frozen clock shows still rain.
- **Budget.** Within 4 ms at half scale by `arkos effect bench matrix`. The rain is one hash
  and one bit lookup per pixel; the edges are one pass over the blocks for the nearest edge,
  then the line, a tear's test and at most one rune bit (A's or B's) for that edge alone.
- All the constants sit at the top of the file with one comment each.

## Tests (written first, headless)

- Theme: matrix is a built-in, passes every contrast pair, recommends `matrix`.
- Manifest: `glyph_rate` and `terminal_decode` parse, default as the table says, and a value out
  of range names the key and the range; `terminal_decode` without `glyph_motion` is invalid with
  an error naming both; the built-in manifest sets them as this document says.
- Theme: every text colour of matrix has luminance 0.47 or more as the shader measures it.
- `glyph_wait` uses `glyph_rate`: at 10 the wait is 100 ms.
- Decode rules: a changed cell decodes for the working or idle time and then settles on its own
  character; an unchanged cell is untouched; a cell that became a space clears at once; a wide
  character decodes as one ASCII glyph in its cell and its spacer never; fresh dark text deepens
  and bright text whitens; an inverse cell freshens its background; the random character for a given cell,
  instant and frame is the predicted one and is ASCII; the fresh mix falls from `FRESH_MIX` to 0
  over `FRESH_MS`; Reduce Motion settles at once; a resize or a change of display offset resets
  the state.
- Glitch mirror: a Rust mirror of the shader's glitch picker (as `effects/space.rs` mirrors the
  wash's schedule) shows that slot 0 glitches nothing and that over 3,000 glyphs and 100 slots
  the rate is within a factor of two of 3 a second.
- Shaders: the three built-in effects compile through the existing check path, `arkos_overlay`
  included, and the glyph shader with its clock parameter.
- GPU: a `bench.rs` render test measures matrix's frame and asserts it under the budget, as the
  watercolor render tests do.

## How it is seen

1. Built in the worktree. `arkos effect check` prints `ok: matrix`; `arkos effect bench matrix`
   says `within budget`.
2. A dev window as a separate instance (`target/debug/arkos sandbox <project folder>`), with
   `theme use matrix` and `effect use matrix` in that instance's config, running a real Claude Code session. Cyrus looks at: the idle state for a
   few minutes (is the glitch tiring?), a prompt being typed, Claude streaming output, Claude
   Code's fullscreen renderer, the sidebar and a markdown tab, a menu.
3. Iterate on the constants from what he says. Each round is a one-line edit, `effect use none`,
   a second's fade, `effect use matrix`.
4. The window's CPU with matrix on, idle in front, goes in the Status list. No release build is
   made locally (the Air rule), so the figure comes from the released build after Cyrus's look
   and the release, as space's did; a debug figure from the dev window is noted beside it in the
   meantime. Over 10% on the release build, `glyph_rate` is halved in a follow-up and both
   figures are reported.
5. A cloud shoot of a scene in matrix for the README and the site, once the look is settled.
6. The Status list in `docs/development.md` gains the hand checks: the glitch at rest, the decode
   while Claude works and while idle, fresh text cooling, the rain's brightness, emoji, menus,
   a 256-colour app in matrix, Claude Code's colours after a restart in matrix.

Nothing merges until Cyrus has looked.

## Left to the builder

The exact theme values within the rules above; the rune bit patterns; the rain's hashing and
how streams are spaced; the edge line's exact widths; how the terminal's decode state is stored
and reset; the test names; whether `glyph_rate` is threaded through `ActiveEffect` or read from
the manifest at the call. Calls made on Cyrus's behalf, open to his correction once he sees the
build: every constant named above, the mirror trick for the UI glitch, `glyph_rate` as a new key
rather than a change to the cap, `terminal_decode` as a manifest key rather than a theme or
app setting, and ASCII only for the decode's random characters.

## Rulings from the builder check (2026-10-29, my calls)

1. The dev-window script is written into this document (Constraints), since the hand-off that
   held it lives outside the repo.
2. The glyph shader's dark-text threshold is 0.3, not space's 0.5, and the matrix theme keeps
   every text colour at luminance 0.35 or more, so coloured ANSI output glows rather than taking
   the dark branch. Space's own rule is unchanged.
3. `GLITCH_CHANCE` is 0.0001 per glyph per 0.1 s slot: the document had said 0.001, which is 30
   glitches a second on 3,000 glyphs, ten times the rate it names.
4. `terminal_decode` requires `glyph_motion`; a manifest with one and not the other is invalid.
   The decode never requests frames of its own.
5. The idle-CPU figure is measured on the released build, after Cyrus's look, as space's was;
   the halving rule applies then.

## Amendment (2026-10-29, builder check / review)

A wide character decodes as one ASCII character drawn in its two-column cell, not as two
characters over its two cells. The terminal paints a wide cell as one run two columns wide, so
the decode keeps the cell's kind (`CellKind::Wide`, then its `CellKind::Spacer`) and changes only
the character; the spacer never decodes. The decode section above says so.

## Amendment (2026-10-29, whole-branch review)

1. The glyph shader's dark-text threshold is 0.45, not 0.3. Peach Light and Lavender Light carry
   30 to 35 text colours between 0.33 and 0.44 that took the emissive branch at 0.3 and fell to
   2.6 to 3.5:1 on their paper. Matrix's lowest text colour (`text_dim`) is 0.498, so the theme
   test's floor is 0.47, a margin over the threshold. This supersedes ruling 2's figures.
2. Fresh text follows the shader's rule: a text colour under 0.45 (`DARK_TEXT` in `decode.rs`)
   deepens toward `FRESH_DEEP` `(0, 90, 30)`, so a light theme's text stays dark and readable on
   its paper (Peach Light's `#4a3028` had whitened to 2.2:1); any other whitens toward
   `FRESH_WHITE` as before.
3. An inverse cell is drawn with its colours swapped (`layout::build_runs`), so its fresh mix
   applies to `bg`, the colour its text is drawn in, and leaves `fg`, its block, alone.
4. The wash's theme green is the greenest (green above the larger of red and blue) of the ANSI
   green, the terminal's text and the focus ring, the ANSI green on a tie. Navy's and Timo
   Purple's rain now read in their ANSI greens, Peach Light's in its own green rather than brown;
   matrix keeps its text green, as before.

## Amendment (2026-10-29, Cyrus after the first look)

"In the terminal, use the subtle matrix effect on the text for the characters that the user types
and when the user scrolls up. Right now, it's too intense and slows the user down."

1. The decode is for what the program writes. For 400 ms after the user's own key press, paste,
   mouse report or scroll (`USER_QUIET_MS` in `decode.rs`), a changed cell settles at once with its
   own character and colour; a cell already decoding keeps decoding. The typed text gets only the
   glyph shader's idle glitch, like the rest of the text.
2. While the user scrolls, the glyph clock is held at 0.0 for 400 ms (`SCROLL_QUIET_MS` in
   `active.rs`), so no glyph glitches while the text moves. A terminal stamps its own
   `last_scroll` for the wheel (every mode), Shift+PageUp/PageDown and a drag past an edge; the
   window stamps its own for a document's wheel (raw or rendered) and its scroll keys; the clock
   rests on the newest of them (`latest_scroll`). No shader changed: slot 0 already glitches
   nothing.

## Amendment (2026-10-29, Cyrus tuned the edges)

Cyrus tuned the pane edges in a WebGL mock-up (`arkos-lab/2026-10-29-matrix-edges/edges.html`
on the external SSD) and chose these settings "for now". The wash ports the mock-up's fragment
shader to Metal in points with the same speeds, sizes and rates; the mock-up's `sin` hash is the
wash's `hash01`, so streams and tears land in other places.

1. The line: the theme's green (the mock-up's `#4af626`), intensity 0.33 (`EDGE_INTENSITY`,
   apart from the user's `u.intensity`, which still scales everything), core width 0.55 pt,
   core strength 0.46, halo reach 5.2 pt, halo strength 0.5.
2. Background rain: `RAIN_ALPHA` 0.15.
3. A, rain on the edges: on, 222 pt/s, trail 110 pt, gap 3 s, one stream an edge, head
   brightness 1.7, as runes 7 pt in size, vertical edges only.
4. B, the line is code: on, horizontal edges, runes 6 pt in size, flipping every 0.5 s, half
   the bits lit, strength 0.8, replacing 0.6 of the core.
5. C, data packets: off, and not ported.
6. D, signal tears: on, 6 a minute (shared among `D_EDGES` = 12 edges, a window of a pane or
   two), 0.12 s long, a 40 pt stretch shifted 2 pt, flashed 1.8 times. None on a frozen clock.
7. E, a busy pane runs hotter: on, the streams twice as fast and the edges 1.4 times brighter on
   a pane whose active tab is a terminal whose Claude session is `Working`. The app marks such a
   pane's tab bar and body blocks with the preamble's new `busy` flag (`ArkosBlock`'s first
   float of padding; the block stays 64 bytes). Where two blocks share a line, the busy one's
   edge wins it (`EDGE_TIE`).
8. The wash keeps `frame_rate = 30`. At 30 fps an idle stream moves about 4 to 10 pt a frame
   (222 pt/s times 0.6 to 1.4) and a busy one 9 to 21 pt, so busy panes are where choppiness
   would show; it is judged in the look.
9. On a dark ground the edges' light is added to the wash, as the mock-up adds it, so a busy
   head or a tear burns on toward white until a channel clips; on a light ground it is mixed in,
   since added light would vanish there. The menu stage does the same in its premultiplied
   colour.
10. As in the mock-up, an edge whose line lies within 1.5 pt of the window's border
    (`EDGE_BORDER`) is skipped by the wash: the window's own outline carries no line, rain or
    runes. The menu stage keeps every edge.

## Amendment (2026-10-29, Cyrus: markdown is for reading)

"markdown needs a softer color / effect on the eyes. it's primarily for reading. so make it
easier for that."

1. Calm text is a general capability. The glyph shader sees only each glyph's colour, so the
   app marks reading text through its colour's alpha: `CALM_ALPHA` = 0.995 in
   `effects::active`. The rendered markdown view (`md_view::md_colors`, and the code spans'
   colours in `code_runs`) multiplies every text colour's alpha by it: the body, the dimmed
   text, links, headings, code, a task box's tick, a diagram's error line and the raw blocks'
   markdown colours (`RawColors.alpha`). Nothing else is marked: not the raw markdown editor,
   the file viewer, the terminal or the UI. Without an effect, or under one that ignores the
   mark, the text is 0.5% transparent, which nobody can see.
2. The window declares `constant float ARKOS_CALM_ALPHA = 0.995;` ahead of every glyph shader,
   beside `ARKOS_SCALE` (`active::glyph_header`), `arkos effect check` compiles with the same
   lines, and `arkos effect contract --glyphs` prints it. Since fades scale the alpha too, a
   shader tests a band: calm when `abs(input.color.a - ARKOS_CALM_ALPHA) < 0.002`.
3. Matrix draws calm light text a soft pale green (the text section's calm rule), with no halo
   and no glitch; its alpha is the glyph's coverage times `input.color.a`, so the 55% core floor
   holds. Calm dark text on a light theme is the dark branch without the glitch.
4. A reading paper: the manifest key `document_paper_alpha` (0.5 to 1.0, default the effect's
   `paper_alpha`) is the paper of the markdown and file viewer tabs (`Style::
   document_paper_alpha`); the terminal and web pages keep `paper_alpha`. Matrix sets 0.92, so
   the rain barely shows behind reading text.

## Amendment (2026-10-29, Cyrus: more rain)

After seeing the rain live, Cyrus asked for more of it, and then for less of it behind the tab
headers:

1. The gap between a column's streams: `RAIN_GAP` 30 s to 6 s, so about three fifths of the
   columns carry a stream on screen at once (0.61 at 800 points; 0.50 at 600, 0.72 at 1000).
2. The rain: `RAIN_ALPHA` 0.15 to 0.22.
3. The terminal paper: `paper_alpha` 0.8 to 0.65. `document_paper_alpha` stays 0.92.
4. The tab strips are no longer darkened (the tab bars' darkening 0.42 to 0), and carry a second
   layer of rain: `TAB_RAIN` = 0.4 of `RAIN_ALPHA` laid again over them (first tried at 1.0, too strong).
5. The active tab over the wash: `active_tab_alpha` 0.5 to 0.3 (first tried at 0.2).

## Amendment (2026-10-30, Cyrus: only a few lines scramble)

"Update the matrix text effect that makes the terminal text scrambled. Shorten the number of lines
that get scrambled. It should just be a few of the most-recent lines."

1. Moved lines do not scramble. Before the cell diff, `Decode::observe` recognises a row that
   moved: a new row whose text (trailing spaces left out) is not blank and equals a remembered
   row's takes that row's cells whole, change instants included, so a line already scrambling
   keeps scrambling as it scrolls up and a settled one stays settled. The remembered row at the
   same place wins, then the nearest below (output scrolls up), then the nearest above. Blank rows
   are never matched. A row that matches nothing is diffed cell by cell as before, and the quiet
   window still marks nothing.
2. At most `DECODE_ROWS` = 3 of the newest lines scramble at once. After the diff, when more rows
   than that have a cell still scrambling (`observe` takes the decode time in force), the oldest
   settle: a row's age is its newest scrambling cell's change, and among rows that changed in the
   same frame the lowest on screen is the newest. A settled cell's change instant is set back by
   the decode time, so it stops scrambling and keeps its fresh brightening, cooling from there.
3. This replaces the "Scrolling output" plan of a cap on the share of cells decoding: the
   decode section above says so.
