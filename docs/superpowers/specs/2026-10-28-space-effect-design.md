# Space effect, holo text and Navy Light's removal

Status: approved by Cyrus in chat, 2026-10-28. Builds on the effects design
(`2026-10-28-effects-design.md`) and everything shipped through 0.7.7.

## Intent

Navy gets an effect of its own, as Peach Light has watercolor: a night sky under the window that
is alive but calm (Cyrus: "more alive, but not too noisy that it's distracting"), with text
drawn as a holo display that moves very slightly so it looks alive. Navy Light goes: one dark
navy theme is the brand, and Peach Light is the light theme people should meet first.

## Goals

1. A built-in effect `space` (folder `crates/arkos/effects/space/`: `effect.toml`, `wash.metal`,
   `glyphs.metal`), listed by `arkos effect list` and Settings beside `watercolor`, and Navy's
   recommended effect (`[effect] name = "space"` (corrected: the theme schema's key is `name`, not `recommends`) in `navy.toml`, as Peach Light names
   watercolor).
2. The wash: a sky made from the theme's colours, and events, as below.
3. The glyph shader: the holo display at Cyrus's tuned values, with the shimmer moving.
4. A second gpui fork hook that hands the glyph shader a clock.
5. Navy Light removed as a built-in theme; Peach Light the default light theme.

## Non-goals

- No new block kinds or UI reports: the wash uses the blocks the app already sends (title bar,
  sidebar and divider, tab bars, pane bodies, highlights, media, tables).
- No sound, no interaction with the pointer, no per-pane differences in the sky.
- No change to watercolor's look. Watercolor's glyphs stay still (they ignore the clock).

## Anti-goals (a failure even if it works)

- Anything that pulls the eye while reading: an event more often than the rates below, a bright
  flash, motion inside the text bigger than the shimmer's few percent.
- Text harder to read than plain Navy: the holo values are Cyrus's own and are not to be
  "improved"; the pane keeps 80% of its fill.
- A window that costs noticeable CPU while idle and visible because the text now moves.

## The wash (`space/wash.metal`)

Everything derives from the theme's colours (`ARKOS_*` indices), so `space` chosen with another
theme still reads as that theme's sky.

- **Sky:** the window background, slightly deepened toward the top; a faint nebula haze from
  two slow noise fields tinted with the theme's magenta and cyan ANSI colours; stars in two or
  three depth layers, each star twinkling slowly at its own rate (a period of several seconds).
- **Shooting stars:** one at a time, every 20 to 40 seconds (the gap chosen per event from a
  hash of its index), a short bright streak with a fading tail crossing a part of the window in
  under a second.
- **Aurora:** a curtain along the top quarter of the window in the theme's green and cyan ANSI
  colours, rising over about 20 seconds, holding about a minute, fading over about 20, then
  absent for one to two minutes. Soft; it never covers more than the top quarter.
- **Planets:** one or two, small (a few percent of the window's height), shaded discs with a lit
  limb, drifting across over several minutes; at most one in view at a time is enough.
- **Pane edges (`ink_borders = true`):** where watercolor draws ink, a thin glow line in the
  theme's focus ring colour: a bright core about a point wide and a soft halo a few points out,
  on the edges each block reports. Media and tables get the same line.
- **Highlights (`highlights = "effect"`):** the selected and hovered rows and the hovered tab get a
  soft glow fill in the focus ring colour, with the strengths by state as watercolor's.
- **Washes per block:** the bars and the sidebar get a faint darkening so their text stays clear
  over the sky; pane bodies get nothing (the pane's own paper does that work).
- **Time:** every event is a pure function of `u.time`; Reduce Motion freezes the time as for any
  effect, so the sky is a still picture. All within the 4 ms budget at 1200x800 by
  `arkos effect bench space`.

Manifest (`space/effect.toml`): `frame_rate = 30`, `scale = 0.5`, `ui_text_bump = 0`,
`ink_borders = true`, `highlights = "effect"`, `paper_alpha = 0.8`, `chrome_alpha = 0.0`,
`active_tab_alpha = 0.5`, `text_contrast = 0.0`. The builder may move `chrome_alpha` and
`active_tab_alpha` if the bars' text reads poorly in the cloud still; everything else is fixed.

## The text (`space/glyphs.metal`)

The holo display from the mock-up (`/Volumes/CyrusExtSSD/arkos-lab/2026-10-28-space-text/
holo-motion-dim.html`), at Cyrus's tuner values, which are fixed:

    brightness 0.80, tint 0.25, keep 0.15, scanlines 0.08, glow 0.90, movement Shimmer at 1.00

As the mock-up's shader defines them: the colour moves toward cyan `(0.55, 0.95, 1.0)` by
`tint`, less for coloured text by `keep` (scaled by saturation); scanlines every 3 points of
smooth depth `scanlines`; the letter's light is `brightness` times the scanline; a halo from
three rings of samples (1.5, 3 and 5 points) at strength `glow`, tinted toward cyan; the shimmer
multiplies the light by `1 + 0.09 * sin(x * 0.07 - t * 2.2 + y * 0.015)` with x, y in points.
Everything is measured in points through `ARKOS_SCALE`, the quad widened enough for the 5 point
halo, and the samples kept inside the glyph's own tile as watercolor's are. The stroke's core
keeps at least 55% of its coverage (the guide's floor for any glyph shader).

## The clock (gpui fork, `v0.2.2-arkos.2`)

- `Window::set_glyph_shader_time(seconds: f32)`: stored by the Metal renderer and bound to the
  glyph pass's vertex and fragment stages at a new `SpriteInputIndex::GlyphTime = 5` as a
  `float` on every draw, whether or not an app shader is set. gpui's own stages ignore it; an
  app shader may declare `constant float *time [[buffer(5)]]` in either function.
- One commit on the fork, tagged `v0.2.2-arkos.2`, with a test that the value set is the value
  bound; `ARKOS.md` lists it. Arkos's `[patch.crates-io]` moves to the new tag.
- `arkos effect contract --glyphs` prints the buffer's line with the signatures, and `check`
  compiles a glyph shader that declares it.

## Redraws while the text moves

- A manifest key `glyph_motion` (bool, default false) says the glyph shader reads the clock.
  `space` sets it; watercolor does not.
- While an effect with `glyph_motion` is settled on and the project window is visible, Root asks
  for a redraw at the wash's own cadence capped at 20 frames a second (a slow ripple needs no
  more), and before each frame sets the glyph time to the effect's clock (`ActiveEffect::time`,
  which Reduce Motion freezes). Under Reduce Motion, while the window is hidden, covered or
  occluded, and while the effect fades, no redraws are asked for the text.
- Measured in a release build on the Air with Navy and `space` on, idle in front: the window's
  CPU goes in the development Status checklist. If it is over 10%, the builder halves the rate
  and reports both figures.

## Navy Light's removal

- `crates/arkos/themes/navy-light.toml` and its `BUILTIN` entry go. `ThemeConfig::default()`'s
  light theme is `peach-light`, and the config template's comment lists the built-ins as `navy`,
  `timo-purple`, `peach-light` and `lavender-light`.
- A config whose `theme.light` or `theme.dark` names `navy-light`, with no user theme file of
  that name (`~/.config/arkos/themes/navy-light.toml`, which "Extract built-in themes" may have
  written and which keeps working as any user theme), is rewritten to `peach-light` in place
  (`toml_edit`, keeping the user's comments) when the window loads its config, and the window
  shows one notice: "Navy Light was removed; your light theme is now Peach Light." Rewritten
  once, it never shows again.
- Tests, the shoot's default `ThemePair`, `docs/themes.md`, `docs/guide.md`,
  `docs/agent-guide.md` and `docs/development.md` follow; the agent guide's light-start example
  becomes `arkos theme template peach-light`.

## How it is seen

Built on a branch with tests. A cloud shoot (`gh workflow run shoot`) of the multitasker scene
in Navy gives a still; Cyrus also runs a dev build to watch the motion. Nothing merges until he
has looked.

## Left to the builder

The exact sky maths (noise, star placement, the aurora's shape, the planets' shading), the event
timings within the ranges above, the glow line's widths, and the test names. Calls I made on
Cyrus's behalf, open to his correction: the event timings, the 80% paper, the 20 fps cap, the
`glyph_motion` key, and the config rewrite for Navy Light.

## Rulings from the builder check (2026-10-28, my calls)

1. The halo is not clipped at the glyph's box: the quad is widened by the halo's reach (5 points
   plus a margin) and a pixel outside the box samples back into the glyph's own tile, as
   watercolor's bleed does. No padding patch; the fork stays hooks only.
2. The holo assumes light text on a dark ground. Where the glyph's colour is dark (luminance
   under 0.5, a light theme with `space` chosen), the shader tints toward a deep teal
   `(0.0, 0.35, 0.45)` by the same `tint`, does not dim (brightness 1), and draws no glow.
3. The mock-up's fragment shader (`holo-motion-dim.html`, the `FS` string, motion 5) is the
   reference and is ported line for line, in points; `brightness` scales the colour, never the
   coverage, so the 55% core floor holds. A copy of the page goes in the plan's workspace.
4. The fork's commit and its tag `v0.2.2-arkos.2` are pushed to github.com/csarkosh/gpui by the
   builder (the repository is Cyrus's and public, as for `.1`); the app pins the tag.
5. The text moves by a full window redraw (`cx.notify` from a timer) at up to 20 a second, as a
   playing video already redraws the window every frame. Measured afterwards as above.
6. The event schedule is offset so a frozen clock (Reduce Motion, which stops at the effect's
   start) shows the aurora at its hold and no shooting star mid-flight.
