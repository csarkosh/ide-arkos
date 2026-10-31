# Arkos IDE: watercolor paints new terminal text in

Date: 2026-10-30. Status: Cyrus picked the soak look (B of the three mock-ups), the terminal
only, and a gpui sprite field for each glyph's start time (2026-10-30). Watercolor's counterpart
to runic's decode. It works GPU first: the CPU stamps a changed cell once, the glyph shader
paints it, and the frames while ink is wet re-present the last scene instead of redrawing views.

## 1. The look

A cell the program writes soaks into the paper: ink shows first where its strokes are thickest
and spreads out to their edges over `PAINT_DUR` (0.26 s), with a wide bleed around the letter.
While it is wet the ink reads deeper and bluer and the bleed is strong; over `PAINT_DRY` (0.8 s)
it dries to watercolor's ink as it is today. A run of cells written at once paints left to
right, each cell `PAINT_STAGGER` (16 ms) after the one before, counted from the first changed
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

Built against the local gpui clone first; the tag is pushed only once Cyrus approves.
