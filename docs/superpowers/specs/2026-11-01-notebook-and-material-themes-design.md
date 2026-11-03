# Two light themes: Composition Notebook and Material

**Status:** exploration, 2026-11-01. Cyrus asked to "explore 2 new light themes:
`composition-notebook`, `material`. `material` should resemble Google's material look.
`composition-notebook` should resemble a compositional notebook with handwriting effects."
Both are built as candidates to look at on screen; the choices below that are mine are marked
"(my call)". Nothing here is released until he has seen them. On the first stills he asked for less
yellow in the notebook's paper (now a near-neutral off-white, `#f8f6f0`) and for more colour in
Material, which felt boring: the bars, sidebar, banner and find marks moved to Material's
primary, secondary, tertiary, green and amber containers, and the effect gives each pane's tab
bar a tonal container of its own pigment and its page a faint tint of that hue.

## Why two themes and two effects

A theme is colours and fonts; a look is also what moves and what is drawn around the text. Peach
Light became a watercolor sketch only with its effect. Each of these themes recommends an effect
of its own, written to the effect contract so it suits any theme, and each can be worn plain.

## 1. Material

### The theme (`material`)

Material 3's baseline light colour scheme, mapped to the theme keys:

| Role | Colour | Keys |
| --- | --- | --- |
| surface | `#fef7ff` | `window_bg`, `tab_active`, `terminal.background` |
| surface container low / container / high | `#f7f2fa` / `#f3edf7` / `#ece6f0` | `sidebar` / `title_bar`, `status_bar` / `tab_bar`, `tab_inactive` |
| outline variant, outline | `#cac4d0`, `#79747e` | `border`, `split_handle`; `split_handle_hover` |
| primary | `#6750a4` | `focus_ring`, `cursor`, `working`, `magenta`, `keyword`, `untracked` |
| on surface, on surface variant | `#1d1b20`, `#49454f` | `text`, `foreground`, `black`, `bright_white`; `text_dim`, `operator` |
| primary container, secondary container, tertiary container, error container | `#eaddff`, `#e8def8`, `#ffd8e4`, `#f9dedc` | `banner`, `selection` and `find_match`, `find_current`, `error_banner` |
| error, Google blue, teal, green, amber, yellow at tone 40 | `#b3261e`, `#0b57d0`, `#006a6a`, `#386a20`, `#8b5000`, `#6d5e00` | the ANSI hues, the tree and the viewer |

Each colour is held to the theme rules by the tests and `arkos theme contrast material`; a tone
that fell short on the surface was deepened until it cleared 4.5:1, and the bright ANSI set is
the same hues a step lighter, still clear. Fonts: Roboto (ui, prose) and Roboto Mono (terminal),
from `brew install --cask font-roboto font-roboto-mono`; a Mac without them shows the system
font, as any theme's missing font does. (My call: Roboto rather than Google Sans, which has no
cask.)

### The effect (`material`)

Flat and tonal, read entirely from the theme's colours, so it is Material under Navy too:

- The wash paints each block in the theme's own colour for it (title bar, sidebar, tab bar,
  pane body), the ground in `window_bg`, and 1 px dividers in `border` on every edge the app
  hands it (`ink_borders = true`, `scale = 1.0` so the lines are crisp).
- Highlights are the effect's (`highlights = "effect"`). A selected Explorer row is Material's
  navigation drawer active indicator: a full-height pill in the theme's `selection` colour. A
  hovered row is a state layer, the text colour at 8%, in the same pill; hovered and selected,
  both. A hovered tab is the state layer on its rectangle. As a highlight fades in, a ripple
  spreads from the row's centre to its ends and thins out, Material's touch ripple with no
  pointer position (the contract has none).
- A busy pane (its active tab's Claude session working) carries Material's indeterminate linear
  progress indicator along the bottom of its tab bar: a 4 px track in `selection` and a bar in
  `focus_ring` that sweeps across on Material's curve, about every two seconds.
- No glyph shader (Roboto stays crisp), menus the app's own, `text_contrast` 0, no text bump.
  Nothing moves but a ripple and a busy pane's indicator, so the cost is well under budget.

## 2. Composition Notebook

### The theme (`composition-notebook`)

- Pages: cream `#f7f3e8` for the window, the pages and the terminal; the inside cover a shade
  deeper for the sidebar; tab bars deeper again.
- Ink: a blue-black ballpoint `#1e2a44` for text and the terminal; pencil grey `#4a5468` for
  dim text. The ANSI set is a pencil case: red, green, blue, purple and teal pens, an ochre for
  yellow (a yellow pen cannot clear 4.5:1 on cream), pencil greys for white and bright black.
- Marks: the margin's red `#b4302a` is the focus ring and the cursor; the selection is a pale
  blue highlighter; a find match a yellow one and the current match orange; the banner a
  sticky-note yellow.
- Fonts: Chalkboard SE for the chrome and the prose (my call: rounder and more legible at small
  sizes than Bradley Hand or Noteworthy, both also installed with macOS); Monaspace Radon for
  the terminal, GitHub's handwritten monospace (`brew install --cask font-monaspace`). Candidate
  B for the terminal is Comic Mono (`font-comic-mono`). Both are shown.

### The effect (`notebook`)

- **The cover.** The title bar is the marbled black-and-white cover, drawn from a thresholded
  noise field, with the cover's white label over it: a rounded rectangle with a thin black
  border from past the traffic lights to the bar's right end, inset from its top and bottom,
  holding the title's words and the bell and doctor icons. The strip below the lowest block
  (the status bar) is the same marble faded to a pale mottle, under its text.
- **The pages.** A pane body is a cream page with paper fibre, a red margin line a few points
  in from its left edge, and blue rules. On a terminal the rules sit under each row of text:
  the contract gains, in the padding `ArkosBlock` already carries, `row_pitch` and `row_top`
  (the pitch of a pane body's text rows in points and the y of its first row, set for a pane
  whose active tab is a terminal, 0 otherwise). A markdown or viewer tab, whose lines vary, is
  plain paper with the margin line only.
- **The sidebar** is the inside cover: plain paper in the theme's sidebar colour.
- **Edges** are pencil lines: `ink_borders = true`, the line in the dim text's colour with the
  pen's wobble.
- **Highlights** (`highlights = "effect"`) are a yellow highlighter stroke in the theme's
  `find_match` colour, ragged at both ends, lighter for a hovered row, fuller for the selected
  one; the hovered tab the same.
- **The handwriting** (the glyph shader): every glyph sits a little off its baseline, by a hash
  of its place, so a line reads hand-set; strokes take a graphite grain and a soft edge, keeping
  the 55% core floor, with no bleed. New terminal text is written in by the pen
  (`terminal_paint = true`, the paint-in watercolor built): wet ink reads a shade darker and
  dries to the theme's ink; a change in place soaks in as a crossfade; a cell background is a
  highlighter stroke; emoji keep their colours with the same jitter.
- Menus `ink`, `paper_alpha` 0.5, `chrome_alpha` 0 (the cover and the pages are the wash's),
  `scale` 1.0 (1 px rules), `frame_rate` 30. Nothing moves but the highlights and the pen.

## 3. Shared

- Both themes are built in (`theme::BUILTIN` grows to eight) and both effects (`effects::BUILTIN`
  to five); the docs that count them change. Each theme recommends its effect.
- The contrast matrix covers them as it covers every built-in, plain and under each effect.
- Docs: `docs/themes.md`, the user guide's theme and effect paragraphs, the agent guide's
  section 9 opening and contract text, `docs/development.md` Status rows for what only the
  screen shows (the cover, the rules under the rows, the pill and the ripple, the progress bar).

## Third pass: the notebook's app bar and binder

Cyrus kept the label across the whole title bar and asked for more room round its words and
icons, and for a spiral binder between the sidebar and the panes. Two manifest keys, both cutting
in once the effect has faded in, as `ui_text_bump` does:

- `title_pad` (0 to 24 points): added to the title bar's padding at both ends. The notebook asks
  for 14; its label runs from just past the traffic lights to the bar's own right padding, three
  points in from top and bottom, so the words and icons sit 14 points inside its ends.
- `binder` (0 to 32 points): a gutter between the sidebar and the panes, only while the sidebar
  shows. The panes' content area starts past it (`split_view::content_area`), and the scene
  reports it as a binder block (`ARKOS_BINDER_BLOCK = 13`, no edges); the sidebar block ends where
  it starts. The notebook asks for 22 and draws a turn of wire every 11 points: out of a hole
  punched by the sidebar's edge, arching over the fold, into a hole by the panes' a little lower,
  with a soft shadow, the pages shading into the fold.


- Bundling fonts. A theme names a family; the user installs it.
- Material cards with gaps and rounded corners around the panes: the layout is not an effect's.
- The status bar as a block of its own.
- Doodles in the margins, or a glyph shader that reshapes letters: the font is the handwriting.

## Material, fourth pass: a Google-style workspace

Cyrus found Material not "Googley" and asked for the look of a Google IDE without any of its
branding: no logos, no product names, no proprietary fonts. Material 3's baseline purple is the
Android default; Google's own web workspaces wear a different Material. This pass follows those:

- **Palette.** A cool pale grey frame (`#f0f4f9`) for the title bar, the sidebar and the status
  bar; white for the cards and the documents; a deep blue accent (`#0b57d0`); a light blue
  selection (`#c2e7ff`) that is also the drawer's pill; near black text (`#1f1f1f`) and a warm
  grey dim (`#444746`); ANSI hues at tone 40.
- **Type.** DM Sans for the chrome and the prose, an open geometric sans in the same spirit as
  the proprietary face Google uses; Roboto Mono for the terminal.
- **Cards.** A new manifest key, `gap` (0 to 16 points), keeps that much frame round every pane:
  between panes (`split_view::spaced` moves each pane's inner sides in, leaving the handles and
  the tree's layout alone), under the bars and at the window's edges (`content_area`), and beside
  the sidebar unless a `binder` is that gutter. Material asks for 8. `pane_radius` (0 to 24)
  tells the app the cards' corner radius: the focused pane's glow becomes a two point ring
  rounded to match, and a terminal's or document's paper rounds its bottom corners, so neither
  shows square on the frame. Material asks for 14, the wash's `CARD_RADIUS`.
- **Tabs.** A new block, `ARKOS_ACTIVE_TAB_BLOCK` (14), is each pane's active tab where its tab
  bar lays it out, reported while the effect paints the highlights. Material underlines it in the
  accent under the label (Material's tab indicator) and draws no active tab fill
  (`active_tab_alpha = 0`).
- **The rest of the wash.** A search-bar pill round the title's words (`title_pad = 10`); the
  pane's number on a round chip of its own hue (the ANSI blue, green, yellow and red as tonal
  containers); a hairline under each tab strip; the selected row a full pill across the sidebar,
  inset from its edges and the same width at any indent; the busy pane's progress bar along the
  bottom of its strip. No lines between cards.
- **Hover.** A quiet state layer of the text, 5%, with no ripple: Cyrus found Material's 8% with
  a ripple too loud. The ripple is kept for a row being selected, a press, as Material has it.
- **The pane number and the terminal's block bars.** On Cyrus's screen the chip sat three points
  from the card's edge, under the focus ring, and Claude Code's context bar (`█░░░░░░░░░ 6%`)
  was unreadable: Roboto Mono has no block elements, and the fallback font's were wider than a
  cell, so the run drifted over its own percentage. On a rounded card the number now sits in an
  18 point slot 10 points in (`pane_view::number_slot`), the chip filling it. The terminal draws
  the block elements (U+2580 to U+259F) itself as rectangles of their cells, snapped to the
  pixel grid so a bar has no seams, the shades as the colour at a quarter, a half and three
  quarters; and a run whose shaped width strays from its cells (another glyph from a fallback
  font) is painted a character to each cell. This holds for every theme and font.
- **A margin inside the card.** A terminal's text ran to the card's edge (Claude Code's `●`
  touched it). On a rounded card the terminal now sits 2 points in from the card's left and right
  sides (`pane_view::terminal_inset`), as Cyrus asked; he then had it doubled to 4 points.

- **The number in the chip's middle.** Cyrus saw the digit off the chip's centre. Two causes:
  the wash placed the chip from the tab bar block's corner, while the app lays the strip out
  inside the pane's one point border (`PANE_BORDER` now moves the chip with it); and flexbox
  centres a line box, not the digit's ink. The number is now painted with its ink centred on its
  slot: across by its glyphs' own bounds, down by the font's cap height
  (`pane_view::centred_origin`).

## The notebook's app bar, in two

Cyrus asked for the label behind the title bar's contents to be split in two, so more of the
cover shows. The bar now reports its two groups as they are laid out (`ARKOS_TITLE_GROUP_BLOCK`,
15, no edges): its words, which take only their own width, and its icons, with at least 40
points between them (`chrome::TITLE_GROUPS_GAP`), the path truncating first. The notebook sets a
label behind each, reaching 14 points past the group's ends, with the marble between; with no
group reported it falls back to the one label across the bar.

## Themes named by colour and effect

Cyrus asked to hide the `space` effect, rename `lavender-light` to `lavender-watercolor` and
`peach-light` to `peach-material`, and add `lavender-material`: a built-in's name is its colours
and the effect it recommends.

- `space` stays built in but `BuiltIn::hidden`: Settings and `arkos effect list` leave it out
  (unless a folder of that name shadows it), and a config, `arkos effect use space` or a theme
  naming it still draw it.
- `peach-material` is Peach's colours and fonts recommending `material` (my reading of the new
  name; it recommended watercolor before). It stays the default light theme.
- `lavender-watercolor` is the old `lavender-light` unchanged; `lavender-material` is its colours
  and fonts recommending `material` (the wash reads the frame from `sidebar` and the cards from
  `window_bg`, so no colour changes were needed; contrast clear).
- The old names still work (`theme::RENAMED`, `theme::current_name`): a built-in lookup, `arkos
  theme contrast` and `template` take them, `arkos theme use` saves the new name, and at launch a
  config naming one is rewritten to the new name with its comments kept and no notice, unless the
  user has a theme file of that old name, which stays theirs.
- The README's blogger still shows the watercolor still, now captioned as Peach's colours under
  watercolor; a reshoot of that story would draw Material.

## For Cyrus to decide

- The terminal font for the notebook: Monaspace Radon (A) or Comic Mono (B).
- Which, if either, to ship, and whether the Material theme should take Google Sans when a
  user has it.
