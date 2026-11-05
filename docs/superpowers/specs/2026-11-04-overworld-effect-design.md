# The `overworld` effect for Timo Purple

## Intent

Timo Purple is a strong violet theme with cyan accents. Cyrus wants it to be more fun to work
in. The model is the fantasy adventures of the Game Boy Color and Game Boy Advance years, the
games people now in their late twenties and early thirties grew up with. The theme's purple stays
the star. The game feel is nostalgic pixel art around the edges of the window, never in the way
of the work.

The look is a night overworld drawn in chunky pixels:
- a starry sky and a crescent moon;
- a far castle with a few lit windows;
- pines, grass and a winding path.

The colours are muted toward the theme's own purples, so the scene reads like a painted backdrop
rather than a bright game screen.

Cyrus chose this from mockups in the brainstorm (direction "A2, Softer"):
- `docs/superpowers/brainstorm/52431-1793848018/content/overworld-v2.html` shows the chosen
  level and the brighter one he turned down.
- `docs/superpowers/brainstorm/52431-1793848018/content/documents-a2.html` shows it around a
  rendered markdown tab and a CSV grid, which he approved as reading well.

The mockups are hand-drawn with canvas and HTML. They fix the composition and the strength, not
the exact pixels.

## Goals

- A new built-in effect, `overworld`, which Timo Purple recommends. Its "Theme's choice" becomes
  `overworld`, so the theme brings the scene with it.
- **The scene lives only in the window's chrome:** the sidebar, the title bar and the tab bars.
- **The tab bodies stay plain Timo Purple.** That covers terminals, rendered and raw markdown,
  file viewers and CSV grids. A tab's content looks as it does with no effect.
- **Nothing moves behind text.** Every name, label and line of content reads exactly as well as
  it does today.

## Anti-goals (a failure even if it works)

- **Hurting legibility.** If the art makes a file name, tab label or line of a document harder to
  read, the effect has failed. Cyrus rejected the first, brighter mockups for exactly this.
- **Strong or bright art.** The approved level is the muted one. Full Game Boy saturation was
  shown and turned down.
- **Borrowed material.** No sprites, characters, logos, fonts, music, maps or recognisable scenes
  from any real game. Every piece of art is original, and the nostalgia comes from the style
  alone: chunky pixels, limited colours, ordered dithering, a night overworld.
- **Busy motion.** Twinkles and flickers are slow and small. Nothing scrolls, flashes or draws
  the eye from the work.

## Non-goals

- No glyph shader. The text is drawn as the font draws it, with no ink or glitches.
- No change to the layout. That means no `gap`, `binder` or `pane_radius`: the panes sit where
  they do today.
- No change to the About, Settings or Keybindings windows, or to the launch chooser. Effects
  never reach those.
- No app changes beyond two: the bars inside tab bodies keep their colour (under "My calls"),
  and the app reports where the sidebar's names end (under Cyrus's answer).
- No sound.

## What Cyrus decided

### The sidebar

- **The scene's position.** It sits at the foot of the sidebar, below the file list. From bottom
  to top:
  - grass with a winding path;
  - pines on either side;
  - low hills;
  - a castle silhouette with a few windows lit in the focus colour (cyan in Timo Purple) and a
    small pennant;
  - a band of dithered sky with stars and a crescent moon.
- **Above the scene.** The sidebar is its plain colour, with no stars behind the Explorer or
  Worktrees names.

### The title bar

- A few stars at its two ends only, clear of the title's words and icons.

### The tab bars

- **The bars themselves** are the theme's own plain colour.
- **The active tab** carries a small lit lantern at its left, before its label. The app draws no
  line on an active tab today; the cyan at the top of a pane is the focused pane's glow, and that
  stays as it is. The 1 pixel cyan line on every active tab in the mockups is not added (my
  call).
- **Inactive tabs** are drawn as today.

### Strength

- Every colour of the art is pulled toward the sidebar colour. In the mockup, each art colour is
  mixed 45% of the way toward the sidebar colour.
- Only the castle windows, the lantern's flame and the fireflies keep their full colour. These are
  the lit things in a night scene.

### The bodies

- **Paper.** Terminal and document paper is opaque over the wash, so no art shows through.
- **Everything inside a tab** keeps its own colours: markdown headings, links, tables, code
  blocks, the outline rail, the grid's header, row numbers and alternate rows, and the `⋯` view
  button.

## My calls (Cyrus can overrule any of these)

### The name

- The effect is named `overworld`.
- Its description is: "A night overworld in muted pixel art at the foot of the sidebar: stars,
  a castle, pines and fireflies."

### The pixel size

- An art pixel is 3 points square, as in the mockups.
- The art pixels are snapped to the window's pixels so their edges stay crisp. No art pixel is
  ever blurred or split. The wash's drawable is stretched with a linear filter
  (`effects/layer.rs:308`), so crisp pixels need `scale = 1.0`, as `material` and `notebook` use.

### The lantern

- The lantern needs the active tab block, which is reported only with `highlights = "effect"`
  (`pane_view.rs:770`). So the wash paints the selected and hovered rows and the hovered tab, as
  plainly as the app does today.
- A tab has 10 points of padding before its optional working dot and its label
  (`pane_view.rs:697-705`), so a lantern up to 3 art pixels wide fits there with no layout
  change. Every pane's active tab gets one.

### Motion

- **Stars** twinkle slowly. Each is seeded, with a period of several seconds.
- **The castle windows.** Now and then one dims and comes back, roughly once every 10 to 20
  seconds somewhere in the scene.
- **Fireflies.** Two or three at a time rise slowly from the grass and fade out within the
  scene.
- **The lantern** glows softly and steadily, with at most a faint flicker.
- **Pausing.** All motion stops under Reduce Motion, which shows a still frame with everything
  lit. It also pauses like any effect when the window is unfocused and `run_unfocused` is off.
- **The still frame.** Under Reduce Motion the wash's clock is exactly 0
  (`effects/active.rs:534-540`), so time 0 must be the frame with everything lit. A pause only
  holds the clock where it was.
- **Frame rate.** The art moves slowly, so the wash runs at a low `frame_rate`. 10 is a starting
  point, and the builder may tune it.

### Bars inside tab bodies

- The art needs `chrome_alpha = 0` so the sidebar and bars show the wash. Today that alpha also
  reaches four bars inside tab bodies: the banners (`pane_view.rs:1397`), the spreadsheet's sheet
  strip (`pane_view.rs:1102`), the video bar (`video_element.rs:303`) and the web tab's address
  bar (`web_element.rs:233`). At 0 they would lose their own colour, which breaks "a tab's content
  looks as it does with no effect".
- A small app change makes those four bars ignore `chrome_alpha` while the effect asks for it: a
  new manifest key, `body_bars_alpha` (0.0 to 1.0, default `chrome_alpha` so every existing effect
  is unchanged), which `overworld` sets to 1.0. The builder may pick another mechanism with the
  same result.

### Menus

- Menus keep the app's own look (`menus = "app"`).

### Light themes

- **The palette.** Any effect can be used with any theme, so `overworld` takes every colour from
  the theme:
  - the sky from the sidebar colour;
  - the silhouettes a step darker than it;
  - the grass from the terminal's green;
  - the windows from the focus colour;
  - the lantern and fireflies from the terminal's yellow.
- **On a light theme** it becomes a muted dusk scene on a pale sidebar. It must still pass the
  contrast checks and look intended, not broken.

### Narrow and hidden sidebars

- **Width.** The sidebar can be dragged to any width. The scene is laid out from the sidebar's
  width:
  - the castle is centred;
  - pines stand near both edges;
  - the hills and grass span the full width.
- **Hidden sidebar.** When the sidebar is hidden there is no scene, only the title bar's stars.
- **Height.** The scene is at most about 140 points tall, and never taller than the empty space
  under the sidebar's last name (see Cyrus's answer below).

## What only Cyrus could answer, and his answer

**When the sidebar's names reach the scene, the scene shrinks, then hides** (chosen from
`docs/superpowers/brainstorm/52431-1793848018/content/overlap.html`).

- **Why it comes up.** The Worktrees section always holds at least the bottom third of the
  sidebar, and with Worktrees closed the Explorer's rows run to its foot (`sidebar.rs:142-152`).
  So names often sit where the scene would be.
- **The rule.** The scene fills only the space between the last visible name and the sidebar's
  bottom.
  - With room for the whole scene (about 140 points), it shows whole.
  - With less, it is cut from the top: the sky goes first, then the castle's top, the ground last.
  - With under about 50 points of room, there is no scene at all.
  - Text never sits on art.
- **The app change it needs.** No block says today where the sidebar's last row ends, so the app
  reports one: a new block (for example `ARKOS_SIDEBAR_FOOT_BLOCK`) covering the sidebar's empty
  space below its last visible row, updated as the lists grow, scroll, open and close. It is
  reported whenever an effect is on, so any effect may use it. The builder picks the exact name
  and documents it in agent guide section 9 beside the other blocks.
- **No animation.** The scene follows that space as the layout changes, the way text moves with
  its list, with no transition of its own (my call).

## Constraints from the project

- **Effects contract.** The effect is a folder `crates/arkos/effects/overworld/` holding
  `effect.toml` and `wash.metal`, registered in `BUILTIN` in `crates/arkos/src/effects/mod.rs`.
- **Shaders as files.** Every shader lives in its own `.metal` file, never in a Rust string.
- **Cost.** `arkos effect bench overworld` must say `within budget` (under 4 ms per frame).
- **Legibility is checked, not assumed.** `arkos theme contrast --effect` applies only the
  effect's `text_contrast` and never renders the wash, so it cannot see art behind a label. The
  plan therefore pins legibility in a headless test of the layout: the scene's band never reaches
  above where the sidebar's names end (see Cyrus's answer), and the title bar's stars
  keep clear of its word and icon groups. The hand checks in the Status list cover the look.
- **Contrast.** The following must each exit 0, and no contrast floor is loosened to pass:
  - `arkos theme contrast timo-purple --effect overworld`;
  - `arkos theme contrast peach-watercolor --effect overworld`;
  - `arkos theme contrast navy --effect overworld`;
  - the contrast matrix in the tests.
- **Colours** are read from the theme each frame (`u.colors`), never baked into the shader.
- **The theme.** Timo Purple's file gains an `[effect]` table naming `overworld`, as Matrix's
  names `runic`.
- **Docs to update.** `docs/guide.md` and `docs/agent-guide.md` list the effects and each theme's
  recommendation, and both say Timo Purple draws plain today. `docs/development.md`'s Status
  checklist gains rows for what only a person at the screen can judge:
  - the scene's look on Timo Purple and on one light theme;
  - legibility of the sidebar and tab labels;
  - the motion's pace;
  - Reduce Motion;
  - a narrow sidebar, a hidden sidebar and a very long file list.
- **Tests and checks.** Tests are headless and named as sentences. Finish with
  `cargo check --workspace --all-targets` showing no warnings.

## Left to the builder

- The exact pixel art: shapes, star layout and dithering pattern. It follows the mockups'
  composition and the A2 strength.
- How the scene is computed in the shader: tiles, signed-distance shapes or a small pixel map
  per object. It must stay crisp and within budget.
- The wash's `frame_rate`, within budget, with the art crisp at a 3 point art pixel on 1x and
  Retina screens.
- Whether the scene re-centres as the sidebar slides shut on Cmd+B or holds its layout until it
  is gone.
