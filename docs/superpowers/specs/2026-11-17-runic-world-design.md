# The runic world: Overworld's features for Tokyo Rain and The Matrix

Cyrus, 2026-11-17: "I want to work on the Tokyo Rain and Matrix themes graphics to have all the
features that the Overworld theme uses", shared "similar to how Timo Purple and Arabia Solar are
shared with Overworld". For Tokyo Rain's sidebar: "the Tokyo cityline with runic lightning and
rain and clouds"; for Matrix: "it has to be something matrix related".

## Shared, with a scene each

Both keep the `runic` effect. A theme names a scene in `[effect] scene` from the effect's
manifest `scenes` (`["city", "code"]`); the wash reads it as `ArkosInputs::scene`.

## Overworld's features, in the runic's light

| Overworld | Runic world |
| --- | --- |
| framed panes in a gap | a neon line with cut corners, 5 points out in a 16 point gap |
| focused frame in the focus colour | the same, and the sidebar's frame while it has the keys |
| glint along a working pane's frame | a packet of light running round it |
| "continue" pennant on a waiting pane | a block cursor blinking on the frame's foot, in the waiting colour |
| stone ledges, lantern on the active tab, gem lit on the focused pane | the active tab underlined in light, a lamp at the bar's right end lit on the focused pane |
| a sky for a title bar, words and icons on plates | storm clouds, words and icons on plates |
| a castle scene at the sidebar's foot, softened under the names | Tokyo at night in the rain (`city`) or a city of falling code (`code`), softened under the names |
| the menu's cursor on the selected row | a chevron |
| hills behind a terminal | the far skyline along its foot |
| text typed in | text decoding, as before |

`city`: storm clouds drifting, the far and near skyline with sparse lit windows (a few warm, a few
flickering), Tokyo Tower in orange lattice with a red beacon, the Skytree pale with its decks,
slanting rain; once a 9 second cycle, from the second on, a bolt of lightning made of a chain of
runes strikes a rooftop and lights the clouds. `code`: towers of falling code, their roofs lit, a
phone booth on the street whose light rings twice every three seconds, and once a 26 second cycle,
from the second on, a black cat with a green rim and lit eyes crosses the street, then crosses it
again the same way while a glitch line sweeps the scene.

The clock's zero (Reduce Motion) has no bolt and no cat, and every light on.

## Cost

Full resolution (`scale = 1.0`), as Overworld, so the lines and the windows stay crisp; one walk
of the blocks a pixel, blocks read by reference, far blocks skipped. Benched on a loaded Mac at
4.4 ms a frame at 1200 by 800 against the old shader's 3.1 to 3.5 ms under the same load (1.8 ms
on a quieter moment); the budget's 4 ms is for an idle machine.

## Tested

GPU: the focused pane's frame lit over another's, the sidebar's frame with the keys, the waiting
cursor in the waiting colour, the two scenes differing, and the dim text over the scene never
below what the rain alone leaves (the rain has always fallen behind the sidebar's names).
Stills for review: `ARKOS_STILLS=<dir> cargo test -p arkos effects::stills -- --ignored`.
