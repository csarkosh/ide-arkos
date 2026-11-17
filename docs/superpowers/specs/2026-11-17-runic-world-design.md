# The runic world: Overworld's features for Tokyo Rain and The Matrix

Cyrus, 2026-11-17: "I want to work on the Tokyo Rain and Matrix themes graphics to have all the
features that the Overworld theme uses", shared "similar to how Timo Purple and Solar Arabia are
shared with Overworld". For Tokyo Rain's sidebar: "the Tokyo cityline with runic lightning and
rain and clouds"; for Matrix: "it has to be something matrix related".

## Shared, with a scene each

Both keep the `runic` effect. A theme names a scene in `[effect] scene` from the effect's
manifest `scenes` (`["city", "code"]`); the wash reads it as `ArkosInputs::scene`.

## Overworld's features, in the runic's light

| Overworld | Runic world |
| --- | --- |
| framed panes in a gap | a neon line with cut corners, 5 points out in a 16 point gap |
| focused frame in the focus colour | the same, wider and brighter (2 points, a 5.5 point halo, 1.3 times the light), and the sidebar's frame while it has the keys; just inside the focused pane's frame, flush against it and following its shape (the frame's own outline moved 3.2 points in, cut corners and all), the runic edges run in the focus colour: runes flipping along its top and foot, streams down its sides, a tear now and then, drawn through the body's paper |
| glint along a working pane's frame | a packet of light running round it |
| "continue" pennant on a waiting pane | a block cursor blinking on the frame's foot, in the waiting colour |
| stone ledges, the active tab a doorway with a line along its top, a lantern on it, a gem lit on the focused pane | a solid panel (top lit, foot shaded, seams, a trace of light), the active tab a doorway in the tab's colour with a neon line along its top, a red paper lantern (city) or a changing rune (code) on it, a lamp left of the bar's + (38 points in), lit on the focused pane |
| a sky for a title bar, words and icons on plates | storm clouds, words and icons on plates |
| a castle scene at the sidebar's foot, softened under the names | Tokyo at night in the rain (`city`) or a city of falling code (`code`), softened under the names |
| the menu's cursor on the selected row | a chevron |
| dusk at a pane's top, hills along its foot, drawn through the paper | a band of storm cloud (city) or falling code (code) at its top, two rows of skyline with lit windows at its foot, drawn through the paper (0.6) as overworld does, the rain at BODY_RAIN so as faint as before |
| text typed in | text decoding, as before |

`city`: storm clouds in three layers with no sky between them (an overcast over the whole band, a bank of large clouds drifting slowly, lighter clouds faster in front; each cloud its own size, height and speed, 2 to 11 points a second), drifting from half the sidebar's height (or the scene's top, if higher)
down into the skyline (Cyrus, 2026-11-17: "make the clouds continue to 50% height"), softened
under the names, the far and near skyline with sparse lit windows (a few warm, a few
flickering), Tokyo Tower in orange lattice with a red beacon, the Skytree pale with its decks,
slanting rain falling at 90 to 140 points a second; once a 9 second cycle, from the second on, a bolt of lightning made of a chain of
runes strikes a rooftop and lights the clouds: softly (Cyrus, 2026-11-17: "not as jarring"), rising
over 0.15 s to 0.7 and fading over the rest of 1.2 s, no flicker, the bolt pale in the rain's colour
and the sky glowing toward it rather than flashing white. `code`: towers of falling code, their roofs lit, a
phone booth on the street whose light rings twice every three seconds, and once a 26 second cycle,
from the second on, a black cat with a green rim and lit eyes crosses the street, then crosses it
again the same way while a glitch line sweeps the scene.

The clock's zero (Reduce Motion) has no bolt and no cat, and every light on.

## Panes flush with their frames

The room between a pane's own rect and its frame's line (5 points) is the pane's, cut at the
frame's corners: the strip fills it beside and above the tab bar, a first active tab reaches the
frame on its left, and the body's night (its band, its skyline, measured from the frame, and its
rain) fills it beside and below the body (Cyrus, 2026-11-17: "flush with the pane's borders and
match the shape too").

## No lines of runes in the window

The panes', the sidebar's and the title bar's own edges are their frames, strips and clouds, not
runes (Cyrus, 2026-11-17: "I don't like the effect that divides the app bar and the panes", and
the one between the sidebar and the first pane). Runes still run along a menu's outline and the
line between Explorer and Worktrees.

## Quiet rain

Behind a pane the rain is as faint as behind a source file (first by a paper covering 92% of it;
since the panes' top and foot came, the paper is 0.6 as overworld's and the wash draws the body
as it should reach the screen, the rain at BODY_RAIN of RAIN_ALPHA; Cyrus, 2026-11-17: "I like
Timo Purple's approach a lot more", "Overworld also has nice graphics at the top and bottom of
the panes"), and over the sidebar it is laid at the same ninth, in its own colour, its heads
never whitened (Cyrus, 2026-11-17).

## Cost

Half scale (`scale = 0.5`), as runic always drew: a pixel a point on a Retina display, which is
what the stills show. At full scale the window measured 11 to 14 ms a frame on a Retina display
(four times the pixels), far over the budget; half scale is a quarter of that. One walk of the
blocks a pixel, blocks read by reference, far blocks skipped.

## Tested

GPU: the focused pane's frame lit over another's, the sidebar's frame with the keys, the waiting
cursor in the waiting colour, the two scenes differing, and the dim text over the scene never
below what the rain alone leaves (the rain has always fallen behind the sidebar's names).
Stills for review: `ARKOS_STILLS=<dir> cargo test -p arkos effects::stills -- --ignored`.
