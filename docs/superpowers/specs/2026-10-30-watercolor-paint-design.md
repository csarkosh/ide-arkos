# Arkos IDE: watercolor paints new terminal text in

Date: 2026-10-30. Status: Cyrus picked the soak look (B of the three mock-ups), the terminal
only, and a gpui sprite field for each glyph's start time (2026-10-30). On seeing the first build
he asked for each letter's lines to be stroked in rather than faded in, and slower (2026-10-31):
the soak became a pen. Trying it, he asked for it faster and for the lines of one output to
differ (2026-10-31): each run is written in a hand of its own, and lines cascade. He found a
counter ticking in place (`Step N`) jarring (2026-10-31): a change in place now crossfades. Watercolor's counterpart
to runic's decode. It works GPU first: the CPU stamps a changed cell once, the glyph shader
paints it, and the frames while ink is wet re-present the last scene instead of redrawing views.

## 1. The look

A cell the program writes is written in: a pen crosses its glyph over `PAINT_DUR` (0.08 s), left
to right and down each stroke, so a stem draws from its top as the pen reaches it. Each run is
written in one of 16 hands (`paint::hand`, picked by a hash of the run's characters, so a line
keeps its hand as it scrolls): pace 0.7 to 1.3 times the stagger, glyph time 0.8 to 1.25 times
`PAINT_DUR`, 50% to 75% of the path across, and a bead 0.6 to 1.2 strong. Runs written at one
instant start down the screen one after another (`PAINT_CASCADE`, 40 ms each, at most 0.2 s,
plus up to 0.1 s at random), the top one at once. The hand rides in the stamp itself: stamps sit
on a 1/128 s grid and the 32 steps inside a grid cell name the hand, exact in an f32 up to the
clock's wrap, and the shader reads the hands from a table in its header (`ARKOS_PAINT_HAND`).

A change in place is no pen. When a run replaced text (the decode remembers each changed cell's
old character, `Decode::was_at`) and the cells changed at its instant are at most
`PAINT_INLINE_SHARE` (40%) of the row's characters, the new text soaks in over `PAINT_INLINE`
(0.4 s), thickest strokes first with a soft edge, while the terminal paints a ghost of the old
text under it that lifts off over the same time: one crossfade. The soak and the fade are two
reserved variants (`PAINT_SOAK`, `PAINT_FADE`) past the 16 hands, with no stagger. A change made
while the screen scrolled (the decode matched a row moved to another place) replaced nothing:
the old line moved on, so the new one is written by the pen with no ghost. The pen's edge is narrow (`PEN_EDGE`), so ink is drawn on, not faded in,
and the stretch just behind it is a wet bead of fuller, bluer ink. A wide bleed follows the pen.
The atlas holds only coverage, not strokes, so the path is the same slant for every glyph rather
than each letter's own stroke order.
While it is wet the ink reads deeper and bluer and the bleed is strong; over `PAINT_DRY` (0.8 s)
it dries to watercolor's ink as it is today. A run of cells written at once paints left to
right, each cell `PAINT_STAGGER` (22 ms, times the hand's pace) after the one before, counted from the first changed
cell of its row, so appended text paints at once and a new line writes itself out.

What does not paint, as with runic's decode: the echo of the user's own typing, paste or scroll
(`USER_QUIET_MS`), a cell that became a space, the row above the screen, a resize or scroll that
resets the remembered grid, and anything while the effect fades in or out, is paused, or Reduce
Motion is on. Emoji are not stamped and appear as they do today.

## 2. The CPU's part: one stamp per changed run

- The manifest's new `terminal_paint = true` (watercolor) turns it on; `Style.terminal_paint`,
  like `terminal_decode`, only once the effect is fully in. An effect has one or the other.
- The terminal reuses `effects::decode::Decode` for what changed: its `observe` already marks a
  changed cell with the instant, keeps moved rows' marks and settles the user's own echo.
- Each row's cells marked at the same instant share one stamp: that instant on the glyph clock,
  less the stagger up to the row's first changed cell measured in points from the window's left
  edge (`PAINT_STAGGER_PER_PT`, the same constant the shader reads). Runs split where the stamp
  changes, so a row still shapes as a few runs, and each run is painted inside
  `Window::with_glyph_stamp`.
- A stamp of exactly 0 means dry (every other glyph in the window carries 0); a real one that
  lands on 0 is nudged off it.
- While any stamp is wet the terminal asks gpui to keep presenting the scene until the newest
  dries (`Window::present_glyphs_until`), and requests no animation frame for it.
- The glyph clock is no longer held at zero while the user scrolls under an effect without
  `glyph_motion`: only runic's place-keyed glitch needs that.

## 3. The GPU's part

`watercolor/glyphs.metal` reads `sprite.stamp`: zero draws as today. Otherwise the glyph's start
is the stamp plus its device x in points times `ARKOS_PAINT_STAGGER`; from the glyph clock it
works out the progress (0 to 1 over the stroke) and the wetness (1 to 0 over the drying), and
from the stroke's own samples (the core and its two rings, which the shader already takes) how
thick the ink is at this pixel, so thick parts show first. A stamp older than a minute, the
clock's wrap among them, is dry.

## 4. gpui (fork tag `v0.2.2-arkos.5`)

- `MonochromeSprite.stamp: f32` in place of its padding (the layout is unchanged), set from
  `Window::with_glyph_stamp(stamp, f)` for the glyphs painted inside `f`; 0 elsewhere. gpui's
  own stages ignore it.
- `Window::present_glyphs_until(deadline)`: until then, a frame with nothing to redraw presents
  the last scene again with the glyph clock moved on by the time since the app last set it.

Built against the local gpui clone first; Cyrus approved it and the tag was pushed on 2026-10-31.

## After 0.8.1 (2026-10-31)

Cyrus asked for four more things, built on the branch `paint-rows`:

- **Only the newest rows.** The pen writes only the `PAINT_ROWS` (4) rows changed most recently,
  the lowest on screen first among rows changed at one instant, as the decode scrambles only its
  newest few. Older text shows as written.
- **Working marks are doodled.** Claude Code's spinner frames (`· ✢ ✳ ✶ ✻ ✽`) and a tool's
  blinking dot (`⏺`, `●`) flip in place. The decode remembers a cell that flipped as one
  (`paint::doodle_mark`, with its style and when), and for `DOODLE_HOLD` (1 s) after the last flip
  the cell is drawn as nothing (`PAINT_HIDE`) while a steady mark is doodled under it
  (`PAINT_DOODLE`): a pen goes round it `DOODLE_TURNS` (0.9) times a second, the ink heavier where
  it has just been, the mark swaying with the hand, easing to plain ink as the hold runs out. A dot
  blinked off is held in its colour.
- **A hidden tab's output is already written.** A terminal the window drew without (its tab was
  hidden; `Root::draws` against `Terminal::painted_draw`) starts its decode again from the grid as
  it is, so neither the paint-in nor runic's decode plays over output that arrived unseen.
- **Backgrounds are washes.** Each stretch of one cell background colour is painted as a band the
  glyph shader inks: a rectangle SVG of the band's size (`icons::wash_path`, served by the asset
  source), stamped `PAINT_WASH`, which needs gpui's `paint_svg` to carry the glyph stamp
  (`v0.2.2-arkos.6`). The shader draws it in broad soft blooms, its ends pooled darker and bleeding
  raggedly, its top and bottom straight so rows of one colour meet without a seam, and, while its
  row is written, sweeps it in behind a ragged brush front over `WASH_TIME` (0.3 s), drying a shade
  lighter. A run that mostly overwrote text (`PAINT_OVERWRITE_SHARE`) now crossfades too.

