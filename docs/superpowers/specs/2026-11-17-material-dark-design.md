# Material for the dark list

Cyrus, 2026-11-17: "change the 'Plain Navy' theme to a 'Material Navy' theme and give it a
Material effect instead. Then, I want to add a couple more popular dark theme options for
Material and make it a single select option like the Light theme's 'Material' and 'Watercolor'
single select group items", and "the theme selection to lower case all the theme names".

## Material Navy, Ocean and Palenight

The material effect draws the theme's sidebar colour as the frame and its window colour as the
cards. Material's dark surfaces stand up from a darker ground, so a dark Material theme keeps its
sidebar (and title bar and status bar) the darker and its window, tabs and terminal the lighter.

- **Material Navy** (`navy`, still the default dark theme): Navy's lifted navy set that way
  (frame #0b111b, cards #16202f), Google's dark blue #8ab4f8 for the accent, Navy's ANSI colours
  with the bright black and white lifted for the lighter cards, DM Sans and Roboto Mono as the
  light Material themes have. Tokyo Rain keeps the colours it was made in (Plain Navy's).
- **Material Ocean**: the Material Theme's Ocean palette, frame #090b10 and cards #0f111a, the
  aqua accent #84ffff.
- **Material Palenight**: the Material Theme's Palenight palette, frame #1b1e2b and cards #292d3e,
  the accent the palette's light purple #c792ea (its deeper one is under 4.5:1 on the cards).

- **Material Sublime**: Sublime Text's classic dark look, its Monokai colours (Cyrus: "the
  classic dark sublime theme as an option in the dark Material theme options"), frame #1e1f1c
  and cards #272822, the cyan accent #66d9ef; Monokai's pink lifted to #ff4d8d (its own is 3.9:1
  on the cards).

The palettes' text and comments are lifted until every colour clears docs/themes.md's rules, the
terminal text 11:1. They are the group "Material" with `appearance = "dark"`; Material Navy is the
dark row's default. A group is one row in each list it has members in, so each group's remembered
pick is kept by group and list ("Material dark", "Material light"; a pick written before by the
group's name alone still counts).

## Pane 0 a card

Cyrus, 2026-11-17: "the material theme's Pane 0 to have the same look as the Pane 1", then "Pane
0 can be darker than the Pane 1". The material effect turns `sidebar_margin` on (its margin the
gap, as a pane's), and the wash paints the sidebar's column as a card rounded as a pane's, its
colour half way from a pane's card to the frame; a selected row's pill lies on that card. The
sidebar's focus glow is rounded as a pane's ring is. Its text keeps its contrast: the card lies
between the frame and a pane's card, both checked against it.

## The keyboard's ring there

Cyrus, 2026-11-17: on Pane 0 the keyboard's indicator "does not match the theme. I want the
outline to match the background shape and the background should be a fainter version of the
currently-selected element". Where the effect paints the rows as pills on rounded cards
(`chrome::keys_ring_is_pill`: highlights painted and a pane radius, material's), the ring on a
session row or a group heading is a pill inset as a selected row's (8 points from the sidebar's
edges), outlined in the focus colour and filled with the selection colour at 0.45, laid behind
the row's text. Then ("the same ... in-focus element changes ... applied to the other effects too
like Runic and Overworld"): a manifest key, `keys_ring`, names the ring's shape for each effect:
material's `pill`; runic's `glow`, a glowing line round the row with a faint wash of the focus
colour, as its lit rows; overworld's `pixel`, a square two point frame filled with a fainter
active tab's colour, as its selected rows are filled; `outline`, the app's own, for the rest. Each is subdued ("too strong. It should look like the selected element's effect, but more subdued"): a faint line (0.35 to 0.45 of the focus colour), a light fill, a small glow.

## Lower case in the theme lists

Settings' preset lists, the group rows and their menus, and the custom look's Color list show
every theme's name in lower case ("material navy", "watercolor lavender ▾"). The titles keep
their capitals everywhere else: the palette, the docs, `arkos theme list`.
