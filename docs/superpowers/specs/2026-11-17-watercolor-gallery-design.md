# Watercolor's gallery in Pane 0

Cyrus, 2026-11-17: proposals for Watercolor's Pane 0 graphic, "a set of popular art in watercolor
that slowly & softly transitions through the set; 1 at a time". He chose museum images painted by
the shader, the famous works, and a wet-in-wet bloom between them.

## The works

Eight public-domain works, each released by its museum under CC0 and shrunk to 384 points on its long
side in `crates/arkos/effects/watercolor/gallery/` with `CREDITS.md` (artist, work, date, source page,
licence; a test checks every image is credited): Hokusai, The Great Wave (Met); Monet, Water Lilies
(Agapanthus) (Cleveland); Van Gogh, Wheat Field with Cypresses (Met); Hiroshige, Sudden Shower over
Shin-Ohashi Bridge (Cleveland); Degas, The Dance Class (Met); Van Gogh, Irises (Met); Bruegel, The
Harvesters (Met); Vermeer, Young Woman with a Water Pitcher (Met). Starry Night (MoMA) and Impression,
Sunrise (Marmottan) are not open access; the Art Institute of Chicago (the Grande Jatte) sits behind a
bot check, so it was not used. Two of the Met's photographs carried black borders, trimmed.

## Images for effects

A manifest key, `images`: up to eight JPEG or PNG files in the effect's folder or a folder in it. They
are read with the effect (a built-in's are compiled in), copied with it by a template or an import,
decoded once when the effect is put on, each stretched to a 512 square slice of a mipmapped texture
array (`effects::images::texture`), and bound at `[[texture(0)]]` to every draw of the wash (the window,
the menus' backdrops, the stills and the bench); an effect with none gets a clear pixel, so a wash that
declares the array never reads an unbound texture. The preamble gains `ARKOS_IMAGE_COUNT` and
`ARKOS_IMAGE_SIZE[i]`, each image's own size, which the wash uses to undo the stretch.

## The gallery

- Where: the sidebar's foot, its empty space under the last row. The painting fits the space less a
  14 point margin, no taller than 1.35 times its width, at the space's bottom, centred across; it
  fades in once the space is 90 points tall, fully at 130.
- When: each painting is held 50 seconds, then blooms into the next over 6, in order, the last into
  the first (`gallery_at`, mirrored in `effects::watercolor` and checked against the shader text).
  Under Reduce Motion the clock holds at zero: the first painting, still.
- How it is painted: sampled a little softer than the pane's pixels ask, its colours run slightly
  along the paper's grain, softened, the darks lifted toward the paper as transparent pigment is, a
  few flat tones as washes lie, darker where two colours meet, the paper's grain through it, and a
  ragged edge that pools darker as a wash dries. It is laid on after the wash keeps the paper readable
  for the UI's text (no text is over the space), multiplied into light paper and glazed over dark.
- The bloom: four pools seeded by the next painting's number spread across both paintings' area,
  their edge rough with noise, reaching the last corner as the six seconds end; the next painting
  shows inside them, its wet front a little darker, and the last painting dries back to the paper as
  they reach it.
- Cost: only the space's pixels near the painting sample the images; benched at 4.0 ms a frame
  against 4.3 for Watercolor without the gallery on the same loaded machine (within its noise).

## The studio

Cyrus, 2026-11-17, after seeing the gallery: it "should blend better into the theme's colors", it
"feels a little too basic and still"; "a scene like a painter's private art studio with the painting
as the main center piece", "soft movements that adds liveliness to the scene without being
distracting", and "a painter's private West Village loft in NYC".

- The scene fills the space under the rows, at most 1.25 times as tall as it is wide, at the space's
  bottom; it fades in from 120 points tall to 170. Its edges melt into the sidebar's paper, so it has
  no frame. Everything is a wash of the theme's pigments or a natural colour taken toward them (wood,
  terracotta, leaves), with the pen's lines lighter than the panes'.
- The room: a steel factory window, three panes across and four down, onto brownstone rooftops with
  cornices, a water tower and a fire escape's rail, the sky's wash left bare where clouds pass; a
  sheer drape at its right; plaster with a ragged patch of exposed brick; wide floorboards with their
  seams and staggered joints; a pendant lamp with a warm glow; a side table with a palette dabbed in
  the theme's four pigments, a jar of brushes and a mug; a terracotta pot with a broad-leaved plant.
- The painting stands on the easel in the middle (its canvas at most half the scene wide and 0.4 of
  it tall, resting on the ledge at 0.32 of its height), toned a quarter of the way toward the theme's
  first pigment on the paper, on its own sheet a little lighter than the page, which hides the easel's
  mast and legs behind it, with a soft shadow on the wall.
- What moves, each slowly (the tests hold every period at five seconds or more): the lamp sways
  2 degrees on its cord every 9 seconds, the drape's hem stirs every 8, the leaves sway every 7, the
  steam curls up every 5, the light through the window breathes every 13 with nine motes of dust
  drifting down it, and the clouds take 150 seconds to pass.
- Cost: each group of the pen's lines is tested only near where it lies; benched within the loaded
  machine's noise of Watercolor without the gallery (about 0.2 to 0.5 ms).

## The backboard

Cyrus, 2026-11-17: "Add a backboard to support the painting. Make the painting's edges straighter."
The easel holds a board of light wood, its grain running across, one size for every painting (half
the scene wide, 0.4 of it tall, its foot on the ledge), so nothing jumps when the next blooms in; it
is solid, hiding the easel's mast and legs behind it, and casts the soft shadow on the wall. Each
painting is as large as fits on it less a 5 point margin, centred, with its own faint shadow on the
board and a strip of tape over each top corner. Its edges waver at most 0.8 points, a sheet cut by
hand, and pool darker only within 3 points of them.

## A gentler bloom

Cyrus, 2026-11-17: the transition was "too big and noticeable"; it should "blend better and
smoother into the next painting". The bloom takes 12 seconds, not 6. Each point of the board fades
from the last painting to the next over half of it, starting later the farther it lies from one of
the four pools, so a wide soft blend drifts out from them instead of a spreading edge; the darker
wet front is gone, the paint of both thins by up to 18 per cent at the bloom's middle so they never
stand at full strength together, and the tape cross-fades with the paintings it holds.

## The cat

Cyrus, 2026-11-17: "a cat that walks in scene for a bit, sits besides various things like the plant
/ painting / etc, and eventually walks off screen". Once every 150 seconds a cat strolls in from one
side along the front of the floor at 0.03 scene widths a second, its legs swinging in pairs once
every 0.9 seconds and its tail held up in a curve; it sits 12 seconds by two of its three spots in
turn (under the side table, by the easel, by the plant; which two and from which side are drawn
for each visit), upright with its tail curled round its feet and its tip twitching now and then,
then strolls off the other side and is away the rest of the cycle, a third of it or more. It is a
soft grey silhouette taken toward the theme's third pigment, with a faint shadow under it. Under
Reduce Motion the clock holds at zero, when it has not yet come in. `effects::watercolor` mirrors its
schedule: never a jump, two different spots a visit, away at least a quarter of the cycle, gone
off the scene at its end.
