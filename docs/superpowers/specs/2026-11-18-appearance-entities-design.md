# Appearance: Color, Font and Effect as their own things

## Intent

Cyrus, 2026-11-18:

1. "Normalize the Color options in the "Custom" tab. Right now, color schemes are duplicated when
   multiple themes use the same scheme. Ex: material lavender and watercolor lavender color
   options are the same lavender color scheme."
2. "Drop the fonts that are not used in the preset themes. This editor should ship with every font
   that the preset theme uses." Asked whether to bundle the fonts macOS lacks: "I want them
   bundled so there's never a chance that a user receives a poor UX because they didn't have the
   font installed."
3. "make the "tokyo rain" theme a group option. I want to add "seattle rain", "frisco rain", and
   "london rain" options. Each new option should have their iconic cityline. For now, use the
   same color scheme for each. But I may want to vary it later."
4. "In the "Custom" tab's "Effect" option, make "runic" a group option covering all the city line
   graphic options and the Matrix City graphic option."
5. "Color, Effect, and Font should be first-class entities that preset themes use and custom
   allows the user to mix-and-match."

He approved the landmarks offered for the new skylines and approach A below ("approved, go with
your recommendation", twice).

This builds on `2026-11-13-appearance-presets-and-custom-design.md` (Presets and Custom, the
config, the commands), which stays true except where this says otherwise.

## What is there today

- A preset is one file in `crates/arkos/themes/` holding everything: about seventy colours in five
  tables, an optional `[font]` table and an optional `[effect]` table (`name`, and for runic a
  `scene`). Fourteen are built in.
- Custom's Color list is one row per theme file (`settings::color_rows`), so a colour scheme two
  presets share shows twice. Three pairs are identical in every colour: Material Solar and
  Composition Notebook, Material Lavender and Watercolor Lavender, Watercolor Peach and Solar
  Arabia. Custom's Font list already shows each font set once, named after the first theme that
  carries it. `custom.color` and `custom.font` store a theme's name.
- Arkos ships no font files. A theme names families, and a family not installed on the Mac is left
  out with a line in the log. The presets name DM Sans, Roboto Mono, Cascadia Code, Monaspace
  Radon and Departure Mono, which macOS does not have, and Avenir Next, Baskerville, Optima,
  PT Mono and Chalkboard SE, which it does.
- Runic draws one of two sidebar scenes, picked by the colour theme's `[effect] scene`: `city`, a
  rainy skyline with Tokyo Tower and the Skytree (Tokyo Rain), and `code`, the Matrix City's New
  York street (The Matrix). In Custom, runic over any colour but The Matrix's shows Tokyo.
- Tokyo Rain's file says it has Navy's colours exactly; it no longer does (22 lines differ). Its
  colours are its own.

## Goals

- A colour scheme, a font set and an effect (with, for runic, its scene) are each a thing of its
  own, with a name and a title. A preset names one of each. Custom picks one of each.
- Each list in Custom shows each thing once.
- Every font a preset uses is in the app, so a preset looks the same on every Mac.
- A Rain group of presets: Tokyo Rain, Seattle Rain, Frisco Rain and London Rain, one colour scheme
  (Rain), each with its own city's skyline.
- In Custom's Effect list, Runic is a group row: Tokyo, Seattle, San Francisco, London and Matrix
  City.

## Non-goals

- Changing any preset's look. Every built-in preset draws exactly as before the split (the same
  colours, fonts, effect and scene), checked by a test.
- New colour schemes. Seattle, Frisco and London Rain share Rain's colours for now; giving one its
  own later is a new colour file and a one-line change to its preset.
- An "Add font…" button (as before: a font set of one's own is a file in the fonts folder).
- Letting the user's own font files be bundled or installed by Arkos.

## Anti-goals

- Someone's saved look changing on upgrade: a Custom look made before this must show the same
  colours, fonts, effect and scene after it.
- A theme file someone wrote in today's all-in-one form failing to load.
- The app growing by more than about 10 MB for the fonts.
- A missing-font fallback ever showing for a built-in preset.

## Approaches

- **A. Each its own file, presets name them (chosen).** Colour schemes in a `colors/` folder,
  font sets in `fonts/`, effects in their folders as today; a preset is a few lines naming one of
  each. Custom lists each folder once. The most work, and the only one in which the three are
  real things to mix and match and a shared scheme cannot drift apart.
- B. Keep one file a preset and merge identical colours when listing. Smallest, and the
  duplicates go, but a scheme has no name of its own (it borrows a theme's), and editing one
  preset's colours silently splits it from its twin.
- C. Split out only the colours. Fixes the Color list but leaves fonts and effects tied to
  presets, half of what was asked.

## The model

### Colour schemes

A colour file is the five colour tables with a `name`, a `title` and an `appearance` (dark or
light), nothing else. Built in, in `crates/arkos/colors/`:

| Colour | Title | Was in |
| --- | --- | --- |
| navy | Navy | Material Navy |
| ocean | Ocean | Material Ocean |
| palenight | Palenight | Material Palenight |
| sublime | Sublime | Material Sublime |
| bright | Bright | Material Bright |
| solar | Solar | Material Solar, Composition Notebook |
| lavender | Lavender | Material Lavender, Watercolor Lavender |
| peach | Peach | Watercolor Peach, Solar Arabia |
| timo-purple | Timo Purple | Timo Purple |
| matrix | Matrix | The Matrix |
| rain | Rain | Tokyo Rain, and the three new Rain presets |

The contrast rules (`docs/themes.md`) apply to a colour file as they did to a theme. `arkos theme
contrast` takes a colour's name or file as well as a preset's.

### Font sets

A font file is today's `[font]` table (`prose`, `ui`, `terminal`, `grid`) with a `name` and a
`title`. Built in, in `crates/arkos/fonts/`:

| Font set | Title | Families | Was in |
| --- | --- | --- | --- |
| material | DM Sans + Roboto Mono | DM Sans (prose, ui), Roboto Mono (terminal) | the Material presets |
| lavender | Avenir Next + Cascadia Code | Avenir Next (prose, ui), Cascadia Code (terminal) | the Lavender presets |
| peach | Baskerville + Optima + PT Mono | Baskerville (prose), Optima (ui), PT Mono (terminal) | Watercolor Peach |
| notebook | Chalkboard + Monaspace Radon | Chalkboard SE (prose, ui), Monaspace Radon (terminal) | Composition Notebook |
| departure | Departure Mono | Departure Mono (all three), grid 5.5 | Timo Purple, Solar Arabia |

(The exact families and slots are copied from the theme files as they are; the table above is a
summary.) "Default" stays: no font set, the config's `[font] family`.

### Bundled fonts

The five families macOS lacks ship inside the app: DM Sans, Roboto Mono, Cascadia Code, Monaspace
Radon and Departure Mono, in the weights and styles Arkos draws (at least regular and bold, and
italic where the family has one), with each font's licence file beside it (all five are under the
SIL Open Font License or Apache 2.0, which allow it; the builder confirms each licence before
adding it). They are registered with the text system at launch, before the first frame, for the
app only (not installed on the Mac). A bundled family is used even when the Mac has a different
version installed, so the look is the same everywhere.

The five that come with macOS (Avenir Next, Baskerville, Optima, PT Mono, Chalkboard SE) are not
bundled. The builder confirms each is present on the oldest macOS Arkos supports; one that is not
is bundled too if its licence allows, or its font set swaps it for a bundled family, with a note
here.

A test holds that every family a built-in font set names is either bundled or one of the
confirmed macOS families, so a built-in preset can never fall back. The "missing font" path stays
for font sets of one's own.

Cyrus said "Drop the fonts that are not used in the preset themes": since nothing was bundled,
nothing is dropped; the Font list shows only the built-in sets, Default, and sets the user added.

### Effects and scenes

Effects stay folders. An effect may have scenes (runic's do), named in its `effect.toml`. Runic's
scenes become `tokyo`, `seattle`, `san-francisco`, `london` and `matrix-city`, titled Tokyo,
Seattle, San Francisco, London and Matrix City. The old names still load: `city` is `tokyo`, `code`
is `matrix-city`.

The scene is the effect's choice now, not the colour's: a preset names it with its effect, and
Custom picks it in the Effect list. A colour file has no `[effect]`.

### Presets

A preset file names its parts:

```toml
name = "seattle-rain"
title = "Seattle Rain"
appearance = "dark"
group = "Rain"
color = "rain"
font = "default"     # or a font set's name
[effect]
name = "runic"
scene = "seattle"
```

`group` and `group_default` work as today. The built-in presets are the fourteen of today,
rewritten in this form, plus Seattle Rain, Frisco Rain and London Rain. The Rain group (dark list)
holds the four, Tokyo Rain its default.

A preset file in today's all-in-one form (colour tables, `[font]`, `[effect]` inline) still loads,
as a preset with a colour scheme and font set of its own, named after it. Its colours show in
Custom's Color list under its title, unless they are identical to a scheme already listed, in
which case they are that scheme (and the same for its fonts).

### Folders of one's own

Application Support gains `colors/` and `fonts/` beside the presets folder and the effects
folder. "Add color…" takes a colour file, or a preset file in either form (its colours are copied
out into a colour file), and selects it. Each folder has its "Show … folder" button.

## Settings, Appearance

- **Presets:** as today, with the Rain group row in the dark list (Tokyo Rain ▾).
- **Custom, Color:** each colour scheme once, dark ones first, each marked dark or light, by its
  title.
- **Custom, Font:** "Default (family)", then each font set once by its title. A set with a family
  neither bundled nor on this Mac is marked and not clickable, as today.
- **Custom, Effect:** "None", then each effect. Runic is a group row ("Runic · Tokyo ▾") using the
  same group row and card the Presets lists use; its card lists the five scenes with a check on
  the current one. The group remembers its pick as the Presets groups do.

## The config

```toml
[custom]
color = "lavender"      # a colour scheme's name
font = "material"       # a font set's name, or "default"
effect = "runic"        # "none", or an effect's name
scene = "seattle"       # the effect's scene; unset is its first (runic: tokyo)
```

`custom.scene` is new and means nothing for an effect without scenes. Migration, done where the
config is read, so nothing is rewritten at launch unless the user changes something:

- `custom.color` naming a built-in preset reads as that preset's colour scheme
  (`composition-notebook` is `solar`, `arabia-solar` is `peach`, `tokyo-rain` is `rain`,
  `lavender-watercolor` is `lavender`, and so on); naming a preset of the user's own in the old
  form reads as that preset's own colours.
- `custom.font` naming a preset reads as that preset's font set, the same way.
- `custom.effect = "runic"` with no `custom.scene` reads its scene from what showed before: the
  colour's old `[effect] scene` (The Matrix's colours gave the Matrix City), else Tokyo. So an
  existing Custom look keeps its scene.
- Presets in `[theme] dark` and `light` are names as today and need nothing.

## The commands

- `arkos theme list|use|contrast|…` keep working on presets. `contrast` takes a colour too.
- New `arkos color list|use <name>` and `arkos font list|use <name|default>`, as `arkos effect`
  works: `use` puts Custom in force with that part, keeping the other two.
- `arkos effect use runic --scene seattle` (or `runic:seattle`), and `arkos effect list` shows an
  effect's scenes.
- `arkos settings set custom.scene <scene>`.
- `arkos theme current`, `color current`, `font current` and `effect current` say what is in force.

## The new skylines

The Tokyo scene is a rainy skyline: a far and a near layer of towers, lit windows, Tokyo Tower and
the Skytree drawn from a half-width profile (`tower_half`), storm clouds, a bolt and the rain. Each
new city keeps that scene (rain, clouds, layers, windows) and changes its landmarks, drawn the same
way, and the skyline's character where it is cheap to:

- **Seattle:** the Space Needle (a slender waisted stalk under a saucer and a spire), the Columbia
  Center (the tallest tower, stepped), and Mount Rainier faint and snow-capped behind the far layer.
- **San Francisco:** the Transamerica Pyramid (a tall thin pyramid), Salesforce Tower (a tall
  tower rounding to its top), and a Golden Gate tower pair at one edge with the cable's curve
  between, in the theme's red.
- **London:** the Elizabeth Tower (Big Ben: a square tower, a lit clock face, a spire), the Shard
  (a tall broken-tipped glass pyramid), the London Eye (a wheel of spokes on its legs) and the
  Gherkin (a rounded bullet), smaller.

Landmarks are lit as Tokyo's are (outline, a few lit points); where a landmark has an iconic colour
(the Golden Gate's red, Big Ben's lit clock) it takes the nearest of the theme's ANSI colours.
Each scene is held under the wash's GPU budget at 1200 by 800 (Tokyo's own time today plus at most
0.3 ms), measured with the effects bench, and is smooth on a 1x external screen.

## Stages

Four plans, each landing on its own:

1. **The entities.** Colour and font files, presets rewritten to name them, the duplicates merged,
   old-form presets still loading, the config migration, Custom's Color and Font lists, `arkos color`
   and `arkos font`. The test that every built-in preset draws exactly as before.
2. **The scene as the effect's choice.** Scenes renamed with their old names kept, `custom.scene`,
   Runic's group row in Custom's Effect list, `--scene`, the migration of a Custom runic look's
   scene.
3. **The three skylines and the Rain group.** Seattle, San Francisco and London in the wash; the
   three presets; the Rain group.
4. **The bundled fonts.** The five families in the app with their licences, registered at launch,
   and the test that no built-in font set can fall back.

## Constraints

- Colours are read from `root.theme` at paint and never cached (the Motion and colour rules in
  `CLAUDE.md` and `docs/development.md`); a switch through either look fades as today.
- The contrast matrix and `arkos theme contrast` cover every built-in colour scheme; never loosen a
  floor.
- Tests first, headless, named as sentences; `cargo check --workspace --all-targets` with no
  warnings. Builds with `nice -n 15`, never `--release`, one cargo at a time.
- `docs/themes.md` is split to describe colour files, font files and presets; `docs/guide.md` and
  `docs/agent-guide.md` are kept true; `docs/development.md` gains Status rows for what only a
  person at the screen can check (each new skyline, the Runic group row, a font set drawing the
  same on a Mac without the font installed).
- Tokyo Rain's stale "Navy's colours exactly" comment and the test named for it are corrected.

## Settled before planning

A builder read this design cold and asked these; the answers are my calls unless quoted.

- **The merge list is exactly three pairs**, Solar, Lavender and Peach; no other schemes are
  folded together. A merge is of colours only: Composition Notebook keeps its notebook effect and
  Chalkboard fonts, Solar Arabia its overworld effect and Departure Mono, as presets.
- **`appearance` belongs to the colour scheme.** A preset's is its colour's; a preset file may not
  state its own.
- **The full migration table.** `custom.color` naming a built-in preset (or an old name in
  `theme::RENAMED`, resolved first): navy → navy, material-ocean → ocean, material-palenight →
  palenight, material-sublime → sublime, material → bright, material-solar → solar,
  composition-notebook → solar, lavender-material → lavender, lavender-watercolor → lavender,
  peach-watercolor → peach, arabia-solar → peach, timo-purple → timo-purple, matrix → matrix,
  tokyo-rain → rain. `custom.font` naming a built-in preset: navy, material-ocean,
  material-palenight, material-sublime, material and material-solar → material;
  lavender-material and lavender-watercolor → lavender; peach-watercolor → peach;
  composition-notebook → notebook; timo-purple and arabia-solar → departure; matrix and
  tokyo-rain → default. A test walks every entry and checks the look drawn is the same before and
  after.
- **Scenes and the looks.** While Presets is in force, `custom.*` is ignored, as today. A preset
  draws its own effect and scene (The Matrix preset names runic with matrix-city, so it is
  unchanged). With no `[custom]` table, the first click on Custom writes what is on screen, now
  including the scene. A Custom look with `custom.effect = "runic"` and no scene takes the scene
  its colour's preset gave (The Matrix's colours give matrix-city, all others tokyo); a Custom
  look with `custom.effect = "theme"` (the colour's own effect) reads as the effect and scene of
  the preset its colour came from.
- **Fonts load through gpui's `TextSystem::add_fonts`**, which the pinned fork (v0.2.2-arkos.9)
  has; it looks a family up in the fonts added before the Mac's own, so a bundled family always
  wins. No fork change is needed.
- **No synthesised styles.** A family is drawn in the styles its files have; a set whose family has
  no italic shows its upright, as today. Static or variable files are both acceptable if gpui
  draws every weight Arkos asks for from them; the roughly 10 MB limit counts whatever is shipped.
- **The oldest macOS is 14.0** (`LSMinimumSystemVersion`); the five system families are confirmed
  against it.
- **A file of one's own with a built-in's name** shadows the built-in (the file wins), as a theme
  file with a built-in's name does today (`theme::list`); found reading the code while planning. The identical-colours check is
  exact, on every colour value.
- **The bench is the builder's to run**, before and after in the same session, as earlier effect
  work did; the Status rows carry a person's check on the screen.

## Left to the builder

- The data structures and where the split lives in `theme.rs`, `settings.rs`, `config.rs`.
- The font set names in files (the titles above are fixed), the weights bundled, and how the font
  bytes are embedded (in the binary or as app resources), within the size limit.
- Each landmark's exact proportions, judged from stills at the sidebar's size.
- Whether `runic:seattle` or `--scene` (or both) is the CLI form, as long as `effect list` shows
  the scenes.
