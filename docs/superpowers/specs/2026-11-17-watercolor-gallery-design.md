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
