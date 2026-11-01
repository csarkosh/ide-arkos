# The shoot at two pixels a point, and theme sweeps instead of flashes

*2026-10-31. Cyrus, on the first recording from the cloud shoot as it played on the store page:
"this video in the page looks bad. low resolution. and i don't like the transitions", then "look
at the type of videos that Apple uses for its products. the video should look like that".*

## What Apple's product videos are

Measured on apple.com/iphone-18-pro on 2026-10-31 (the `large_2x.mp4` files the page plays at a
1404 px viewport): 2880 pixels across for a 1440 px layout, so two pixels a CSS pixel; H.264
4:2:0 tagged sRGB; 30 fps; 6 to 15 Mbit/s; 2.5 to 5 s long; no sound. Each clip is one calm,
continuous move (a slow push, a reveal, light spreading over the product) that settles into the
still the page then shows. Nothing cuts; nothing flashes.

The first Arkos recording was none of that: captured at one pixel a point on the runner's
1024 by 768 display (its only mode; `display.swift best` finds no HiDPI mode there), 0.56
Mbit/s, and four themes switched by the IDE's own 200 ms cross-fade, which on a 16 s loop reads
as a slideshow with a flash between slides.

## Two pixels a point: a virtual display

The runner's display cannot be switched to a 2x mode, but macOS can be given a display:
`scripts/shoot/vdisplay.m` (Objective-C, built with `clang` once a run into the shoot's cache,
as `record.swift` is with `swiftc`) creates a virtual display through CoreGraphics'
`CGVirtualDisplay` (private, the API DeskPad and BetterDisplay use; no entitlement outside the
sandbox), 1440 by 900 points at two pixels a point, and makes it the main display by putting it
at the origin and moving every other display to its right. It prints `ready <id> <w> <h> <left>
<top> <right> <bottom>` (the display and its visible area in points, as AppKit sees them) and
stays until it is told to stop, when the display goes away with it.

`arkos-shoot` starts it only when the main display draws one pixel a point (a runner; never on a
Retina Mac, whose display is already right and must not be rearranged under its user), before
the window's config and effects are written, and ends it last, after the window has gone. The
window then opens centred on it as before (gpui's primary display is the one at the origin), the
stills come out at 2x as `frame.py` has always assumed, and the recorder (`record.swift`) sizes
its capture by the window's display's pixel density instead of its points. A storyboard's
`[[effect]]` scale, which exists to give a 1x runner a Retina Mac's wash density, is left alone on
a 2x display, since the built-in manifests already draw for it.

## Sweeps: the transitions made in the cut, not in the IDE

The recording still shows the window through its themes for real, each theme held `hold_secs`
(now 6 by default), the switches made through the config as before. The loop is cut differently
(`scripts/shoot/loop.py`): each theme's steady stretch is taken from one second after its switch
(past the config poll and the IDE's fade) to a tenth of a second before the next, and the
stretches are joined by `fade_secs` (1.2) of a soft-edged sweep from left to right
(`transition = "sweep"`, ffmpeg's `smoothright`), the new theme washing over the window the way
light moves over Apple's products; `transition = "dissolve"` is a plain cross-fade. The loop's
seam is the same sweep from the last theme back into the first: the first stretch is used twice,
the output starting `fade_secs` into its first use and ending at the same frame of its second, so
the last frame leads into the first with no cut at all.

The file is scaled to at most `width` pixels across (2000 by default: the store page shows it up
to 1000 CSS pixels wide, so two pixels each), encoded H.264 High at CRF 19 with the slow preset,
tagged sRGB and BT.709 like Apple's, with its first frame as the poster.

## The opening clip (added the same evening)

Cyrus, on the sweeps loop: "I'm not happy with the video. I want it to be more cinematic. It
should also be a background video similar to the one that plays here at the top of the page:
https://store.google.com/product/pixel_11_pro". That hero is a 6 s, 2880 by 1600, 24 fps clip
that opens macro on the phone's camera bar, pulls back and straightens into the key art, holds,
and plays once (full bleed, `object-fit: cover`, the pill bar over its foot).

The Arkos version is `scripts/shoot/cinema.py`, driven by a `[[recording]]` with `cinema`: the
window recorded in its one theme (the raw, at two pixels a point, is the front window's live
texture), the camera starting tight on the tab bar and its working and waiting marks, seen from
the side with a rack of focus, pulling back on a cubic in-and-out into the three-window key art
(the two backs the scenes' stills, drifting less, sharpening as they come), on a dark lit ground
with the window mirrored under it, then holding. The store page plays it once behind the title
and keeps its last frame, which is also its poster and the share card. The themes loop stays
under "Who it's for".

## Not done

- The page still loops the clip rather than playing it once and holding, as Apple's pages do;
  that is a page decision for csarko.sh, not the shoot's.
- The stills of every scene also come out at 2x from the next run; the README's PNGs would
  grow about four times if they were replaced, which is Cyrus's call.
