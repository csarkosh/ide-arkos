# Overworld, the whole window

## Intent

Timo Purple reads plain next to the other themes. Cyrus, 12 November: "it's too plain. If you look
at `navy-runic` or `peach-watercolor` or even `material`, they all look distinct and unique in
their own way. timo-purple does not. I like its current theme direction, but I don't find it
enjoyable as the other themes."

The reason is in what `overworld` leaves alone. Runic, watercolor, notebook and material each
redraw the window's structure (the pane edges, the tabs, the title bar, the selection) and give
the terminal's text an entrance (decode, pen, ballpoint). Overworld does neither: it is a small
muted picture at the foot of the sidebar, and everything else is two flat purples.

This design keeps the theme's direction (its purple, its cyan, the night overworld of the handheld
fantasy games) and takes it across the whole window. Cyrus chose it from
`docs/superpowers/brainstorm/timo-purple-look/directions.html`: step 3, "The whole window is the
world", with one change, the moving light on a working pane's frame made "less attention
grabbing" (the page shows the calmed one).

It replaces three decisions of `2026-11-04-overworld-effect-design.md`, each by Cyrus's choice of
step 3: the tab bodies no longer stay plain, the art is no longer muted to a backdrop, and the
effect now changes the layout (a gap round the panes) and draws the text's entrance.

## Goals

- **The window is built of game menu windows.** Each pane, and the title bar, wears a notched
  pixel frame on a dithered night ground. The focused pane's frame is in the focus colour.
- **The tab bar is a stone ledge.** The active tab is a lit doorway in it with its lantern.
- **The title bar is a strip of night sky**: stars along it and a slow cloud, clear of its words
  and icons.
- **The selected file has a pixel cursor** beside it, the menu cursor of those games.
- **The night reaches into the terminals**: a dithered dusk at the top of each one, a few faint
  stars, and a line of hills along its foot, behind the text.
- **Terminal text types in like a dialogue box**, each new character lit for a moment.
- **A working pane shows it quietly**: a short glint of the frame's own colour drifts round its
  frame. A pane whose session waits for the user shows the dialogue box's "continue" arrow in its
  corner.
- **The art is near full colour**, no longer pulled halfway to the sidebar's purple.

## Anti-goals (a failure even if it works)

- **Text that is harder to read.** Everything drawn behind terminal text is darker than the
  terminal's own background, except the stars, which are single art pixels kept faint. If a line
  of a session is harder to read than under no effect, the effect has failed.
- **Motion that pulls the eye.** Cyrus asked for the one moving thing near his work to be calmer.
  Nothing near text is bright, fast or larger than a few art pixels. The typing is over within a
  medium token.
- **Borrowed material.** As before: no sprites, characters, logos, fonts, maps or scenes from any
  real game. The style alone carries the nostalgia.
- **A slow window.** `arkos effect bench overworld` says `within budget`.

## Non-goals

- Documents keep their paper: rendered and raw markdown, the file viewer and the CSV grid are
  opaque inside their frames, as today (`document_paper_alpha = 1.0`). The night is behind
  terminals only. (My call. Reading tabs are where Cyrus turned art behind text down on
  4 November, and tables and the grid keep their own fills only over opaque paper.)
- The menus keep the app's look.
- The About, Settings and Keybindings windows and the launch chooser are untouched, as for any
  effect.
- No pane number gem. The mockup drew one in place of the pane's number; the number is useful and
  stays as the app draws it, on the ledge. (My call.)
- No sound.

## What is drawn

Everything is in art pixels of 3 points, snapped as today, and every colour is the theme's.

### The ground and the frames

- The manifest asks for `gap = 15`. Between two panes, and between the panes and the window's
  bars and edges, there are 15 points: 5 art pixels. `ink_borders` stays false: the app keeps its
  own 1 point lines, a pane's just inside its frame, so tables, grids, videos and the outline
  rail keep theirs with no further work.
- The ground in that space is a checker of two night colours, both darker than the sidebar.
- Each pane's frame is 2 art pixels wide, drawn in the gap just outside the pane: an outer line
  in a lighter purple (the border colour lifted toward the dim text) and an inner line in the
  darkest night colour. The corners are notched: the outer line's corner pixel is left to the
  ground. Two neighbouring panes' frames leave 1 art pixel of ground between them.
- The focused pane's frame is in the focus colour. The wash cannot tell which pane has the focus
  today, so the app tells it: the pane's blocks carry `focus`, the same 0 to 1 fade the app's own
  glow uses, so the frame's colour moves with it. The app's soft glow inside the pane stays.
- The title bar has one line in the frames' colour along its foot, since the window's own edge is
  its other three sides, and the sidebar one down its right side, in the gap beside it. Nothing
  is drawn inside the sidebar's left padding but the cursor.

### The tab bar

- A ledge of stone: the banner colour, a lighter course along its top, a dark line along its
  bottom, and brick joints a shade darker than the stone. The joints are darker than the stone,
  so a label over one has more contrast, not less.
- The active tab is the terminal's background colour, open to the body below, with a line in the
  focus colour along its top when its pane has the focus and in the frame's purple otherwise. Its
  lantern stays.
- A hovered tab is the stone a step lighter.

### The title bar

- A dithered sky, darkest at the top, with stars along its whole length except within
  `STAR_CLEAR` of a group of its words or icons and left of the traffic lights. (Today only a few
  cells at each end have one.)
- One small cloud drifts along it and is not drawn where it would come within `STAR_CLEAR` of a
  group.

### The sidebar

- The scene at its foot as today, with `MUTE` down from 0.45 to 0.1.
- The selected row keeps its fill and gains a cursor: a right-pointing triangle 3 art pixels wide
  and 5 tall in the terminal's yellow, in the row's 10 points of left padding (every row's text
  starts at least 10 points in, `sidebar.rs`). It does not move sideways, since there is no room
  to; it breathes slowly between full and four fifths brightness. A hovered row has no cursor.
  The cursor fades with the row's highlight.

### The terminals

- `paper_alpha` goes from 1.0 to a value that lets the wash through behind terminals (0.6 is the
  starting point); documents stay at 1.0.
- Behind a terminal the wash draws, over the terminal's background colour:
  - a dusk: the top 34 art pixels dithered from the sidebar's colour down to the background;
  - stars: a sparse scatter over the upper half, each one art pixel, twinkling slowly, drawn so
    that after the paper they are no lighter than the background mixed 25% toward the dim text;
  - hills: two rows of low hills along the foot, each a step darker than the background, 10 art
    pixels at their tallest.
- The wash draws these strong enough to read through the paper; the numbers above are what
  reaches the screen.

### Working and waiting

- A pane whose blocks are `busy` has a glint on its frame: 6 art pixels of the frame's outer line
  lifted toward white, fading along its length, going round at 12 art pixels a second. It never
  leaves the frame. (The mockup's first version had a white head and went three times as fast;
  Cyrus asked for it calmer.)
- A pane whose active tab is a terminal whose Claude session is waiting carries the "continue"
  arrow as a pennant: a down-pointing triangle 5 art pixels wide and 3 tall in the terminal's
  yellow, hanging from the foot of the pane's frame 4 art pixels in from its right. The app
  reports it as it reports busy: the pane's blocks carry `waiting`. A waiting session becomes
  idle when the user looks at it (`SessionManager::acknowledge`), so it shows only until then.
- Changed while building, from the mockup's arrow inside the body's corner: there it would be
  behind the terminal's paper, at 40% of its colour, and under any text that reached the corner,
  against the rule that nothing light is drawn behind text. On the frame it is at full colour
  and never under text. It has no room to bob there (the next pane's frame is an art pixel
  away), so it breathes between full and 60% brightness instead, 800 ms each way.

### The text

- The effect gains a glyph shader and sets `terminal_paint = true`, so the terminal stamps each
  changed cell with its start as it does for the watercolor pen: a line is typed within 400 ms
  (medium4) and at most four rows at once, with no new CPU work.
- A stamped glyph is not drawn before its start. From its start it is drawn whole, in its own
  colour lifted toward white with a touch of the focus colour, and settles to its own colour over
  150 ms (short3, linear, a colour change that does not move).
- Every other stamp the terminal sends (a change in place, the spinner's frames, a cell
  background) is drawn at once as the plain glyph or fill, with no ghost of what it replaced.
  A working mark (a tool's blinking dot) is one steady mark, as the terminal paints it for the
  watercolor pen: the terminal hides the blinking cell itself, so a shader cannot blink it.
- Text is otherwise exactly the font's: no bleed, no shadow, no glitch. Reading text and UI text
  are untouched.
- Under Reduce Motion the terminal stamps nothing (`paint_clock`), so text appears at once.

### Motion, for the record

| What | Period or length | Kind |
| --- | --- | --- |
| Stars, castle windows, fireflies | as today | ambient |
| Cloud | 2 art pixels a second: a 1200 point bar in about three minutes | ambient |
| Cursor's breath | 2 s | ambient |
| Frame glint | 12 art pixels a second | ambient |
| Waiting pennant | 800 ms (extra-long2) down, 800 ms up, in brightness | ambient |
| A character's flash | 150 ms (short3), linear | transition |
| A line typed in | within 400 ms (medium4), the pen's | transition |

Time 0, the still frame under Reduce Motion, has everything lit as today, the cursor at full
brightness, the pennant at its brightest and the glint at the frame's top left corner.

## Light themes

Any effect can be put on any theme. On a light theme the ground is a checker of two tones a step
darker than the sidebar toward the theme's ink, the frames are the border colour and the ink, the
dusk in a terminal runs from the sidebar's colour, and the hills and stars step toward the ink
rather than away from it, so everything behind text still raises its contrast or stays within the
star's limit. It must look intended on Peach Watercolor and Composition Notebook's colours and
pass the contrast checks.

## App changes

- `ArkosBlock` gains `focus` and `waiting`, taken from its three spare floats, on a pane's tab
  bar and body blocks: `focus` the pane's focus fade (0 to 1), `waiting` 1 while its active tab
  is a terminal whose Claude session waits. Every other block has 0. The contract
  (`arkos.metal`), the agent guide's section 9 and `blocks.rs` say so. The block stays 64 bytes.
- The wash runs at 30 frames a second, so a focus fade of 100 to 150 ms shows in three or four
  of its frames with no further work.
- With a `gap`, the title bar's and the sidebar's blocks stopped a gap short of the window's
  right and bottom edges, which only showed once an effect drew a ground that is not the
  sidebar's colour. `blocks::Scene` now carries the gap and both reach the edges; `material`'s
  search bar keeps its place (its `SEARCH_RIGHT` grew by its gap).
- Nothing else. `gap`, `terminal_paint`, `paper_alpha` and `glyphs` are existing manifest keys.

## Constraints from the project

- Shaders are files: `crates/arkos/effects/overworld/wash.metal` and a new `glyphs.metal`.
- The layout and timing arithmetic the tests pin is mirrored in `src/effects/overworld.rs`, and
  the test that compares the constants covers the new ones.
- GPU-first (Cyrus, 30 October): the typing uses the stamps the terminal already makes; the glint,
  the arrow, the cloud and the cursor are computed in the wash from its clock.
- Legibility is checked, not assumed:
  - a GPU test renders a terminal body and asserts every pixel behind the rows is no lighter than
    the star limit, and that everything but the stars is no lighter than the background;
  - the title bar's stars and cloud keep clear of its groups, and the scene never reaches above
    the sidebar's foot, as today;
  - `arkos theme contrast` exits 0 for `timo-purple`, `navy`, `peach-watercolor` and
    `composition-notebook` with `--effect overworld`, and the contrast matrix passes. No floor is
    loosened.
- `arkos effect bench overworld` says `within budget`.
- Docs: `docs/guide.md` and `docs/agent-guide.md` (the effect's description, the two new block
  fields), `docs/themes.md` if it describes the effect, and `docs/development.md`'s Status rows
  for what only a person at the screen can judge: the frames and ledge on Timo Purple and one
  light theme, the typing's feel, the glint's and the arrow's calm, a terminal's legibility over
  the night, Reduce Motion, and a window with four panes.
- Tests are headless, named as sentences and written first. `cargo check --workspace
  --all-targets` ends with no warnings.

## Left to the builder

- The exact pixels of the frame, the ledge, the cloud and the hills, following the mockup.
- The exact `paper_alpha` and the strengths the wash draws at to land on the limits above.
- How the wash stays within budget at full scale with the extra work per pixel.

## Second pass, 12 November

Cyrus looked at the build in a sandbox window: "It doesn't look complete based on the designs
you sent me." He asked for the mockup's gem, its arrow in the terminal's corner, its bobbing
cursor and its animations, and added: "I also don't want the castle fully hidden. Maybe just
softened into the background more so the text overlays with enough contrast." This pass
replaces the passages above where they differ.

- **The sidebar and the title bar are boxed** in the panes' own two-line frame, inside their own
  edges, as the mockup drew them. A new manifest key, `sidebar_pad` (0 to 24 points before every
  row and header of the sidebar; overworld asks for 12), keeps the names clear of the frame and
  the cursor. The title bar's stars and cloud keep to the five art rows inside its frame.
- **The gem** is back: four art pixels square near each tab bar's right end, clear of the +
  button, dull on most panes and the focus colour on the focused one. The pane's number stays.
- **The "continue" arrow** is in the waiting terminal's bottom right corner again, bobbing one
  art pixel every 800 ms, in place of the pennant on the frame. It is behind the terminal's
  paper, so it reaches the screen at 40% of the terminal's yellow, and text that reaches the
  corner is drawn over it. Cyrus chose this knowing both.
- **The cursor bobs** one art pixel toward its row and back every 500 ms, just inside the
  sidebar's frame, and no longer breathes.
- **The scene never hides.** It stands on the sidebar's frame under ten more art rows of open
  sky with stars, whatever the lists cover. Where names lie over it (above the sidebar's foot
  block) it is softened into the sidebar's colour: what leans away from the text's colour by a
  third, and what leans toward it by as much as keeps the dim text at 4.6:1 or better against
  it, worked out per pixel from the theme's colours, so it holds on any theme. A test renders a
  sidebar whose names run to its foot on six themes and checks the dim text keeps 4.5:1 on every
  pixel. The old rule (the scene shrinks, then hides, and no name sits on art) is gone.
- **The animations** were there but for the three above. A burst of captures confirmed the wash
  redraws and new terminal text arrives a few characters a frame, each lit as it lands.
- **Cost.** The first build read 3.4 to 10.4 ms a frame in a 1200 by 800 window. The pane
  bodies now find their terminal's rows in the wash's one pass over the blocks and work out the
  hills only at their foot.

## Third pass, 12 November

Cyrus, after the second pass in the sandbox: "the effects don't feel alive enough", "source
files like markdown and python files feel too plain", "is the font in the similar game bit style
as the rest of the IDE?" and "same with the text transitions".

- **More life**, all in the wash from its clock, none of it behind text in a tab:
  - stars twinkle deeper (down to a fifth) and quicker (2 to 6 s);
  - a shooting star every 7 s or so in the title sky and another over the castle, 0.6 s each;
  - a second, smaller cloud in the title bar and the first twice as fast; a cloud over the castle;
  - the castle's windows gutter like candles and one dims every 3 to 7 s; its pennant waves;
  - the grass's bright blades lean and return; six fireflies instead of three;
  - a bat crosses the scene's sky every 13 s, flapping;
  - each lantern's flame flickers; a spark crosses the focused pane's gem every 3 s.
  The first beat of each is steady, so the still frame under Reduce Motion has everything lit and
  nothing caught mid-way. The frame's glint keeps the calm Cyrus asked for.
- **The night is behind documents too.** `document_paper_alpha` is no longer set, so a markdown
  tab and a file viewer have the terminal's paper and the same dusk, stars and hills behind them.
  This reverses the first pass's non-goal. With a see-through paper the app clears a table's and
  a grid's own fills (as under every other effect), so the wash paints a markdown table's header
  as a band a step away from the text's colour; alternate rows are no longer tinted.
- **A pixel face.** Timo Purple names Departure Mono (SIL Open Font License) for its terminal,
  its chrome and its prose. A theme's font is used only when installed
  (`brew install --cask font-departure-mono`); the shoot's runner installs it. Its pixels are
  whole at a preset size of 16.5 or 22 on a Retina screen.
- **Bit-style text.** A typed-in character no longer fades from lit to plain. It resolves in
  three cuts over 200 ms (short4): blocks 3 points square in the lit colour, blocks 1.5 points
  square in the accent, then the font's glyph a little lit, then plain.

## Fourth pass, 12 November

Cyrus: "Can the font be made a little sharper? It's not easy on my eyes to read." And for
source files: the backdrop should follow the page's length, the top effect when scrolled to the
top, the bottom effect when scrolled to the bottom, both when the page does not fill the screen,
and "the empty in-between space" otherwise.

- **The page's night.** A document tells the wash where its whole page lies: a new block,
  `ARKOS_PAGE_BLOCK` (17), as wide as the pane's body, from the page's top (above the body once
  scrolled) to its end (below the body while more is left). Rendered and raw markdown and the
  file viewer report it (`Viewer::page_span`); a terminal and a grid do not, and keep the pane's
  own night. The wash draws a document's dusk, stars and hills in the page's coordinates, so
  they scroll with the text exactly as Cyrus described, part-way states included. The wash runs
  at 30 frames a second, so during a fast scroll the backdrop can trail the text by a frame.
- **Sharper text.** The glyph shader cuts every glyph's coverage between 0.3 and 0.7, so an edge
  is hard instead of a soft fringe. Departure Mono's pixels are whole only at multiples of 11 px
  (of 5.5 px on a Retina screen); at other sizes the edges are now hard but its strokes are not
  all the same width. A theme cannot choose a text size, so this pass does not snap sizes; a
  preset of 16.5 is pixel-perfect for the terminal and documents.

## Fifth pass, 12 November

Cyrus: "I'm happy with this", and asked for "a `arabia-solar` theme", the sandy look he saw
when the effect was shot on Peach Watercolor, with the "same persona as `timo-purple`".

- **Arabia Solar**, a tenth built-in theme: Peach Watercolor's colours exactly (the ones he
  saw), recommending `overworld`, set in Departure Mono. It is Timo Purple by day. The indie
  game dev's storyboard takes a second picture in it (`indie-game-dev-solar.png`), as the
  blogger's does in Lavender Watercolor.
- **By day.** On a light theme the scene's crescent moon is a sun: a whole disc in the focus
  colour with four rays that come and go. The title bar's sky is dithered away from the text's
  colour (lighter on a light theme), where it had been toward it and made the title's words
  harder to read. A table's header behind a document's paper takes a faint band toward the text
  on a light theme, where a band away from it would be white on near white.
- **A font's grid.** A theme's `[font]` table may give `grid`, the points its terminal font's
  own pixels are whole at. Under the theme every text size is drawn at the nearest multiple of
  it: the preset's size in terminals and viewers (`Root::apply_text_preset`, and again when a
  theme with another grid is put on), the chrome's labels (`Style::ui`), and a Cmd+= or Cmd+-
  nudge moves one step of it. The config's presets are not changed, and without the font
  installed nothing is snapped. Both pixel themes set 5.5, so a preset of 14 or 18 draws at
  16.5 and the chrome at 11. Rendered markdown's headings are sized from the body text and are
  not snapped.

## Sixth pass, 12 November

Cyrus, in the sandbox: "part of the castle does not get dimmed when i scroll all the way down"
(his screenshot: the strip of grass under the last worktree row at full colour, the rest
dimmed), "the app bar is not as alive as i'd like it", and of Arabia Solar: "more aladin
themed... the same effects. just aladin instead of high fantasy".

- **The scene dims as one.** Under the names it is softened as before; and once the names
  reach into the scene at all, the rest of it is softened with them, fully by 24 points in, so
  no line runs across it and nothing below the last name stays bright.
- **A livelier title bar.** Its stars run its whole length, denser, and no longer stop short of
  its words and icons: within `STAR_CLEAR` of them a star, a cloud, a shooting star and the
  bird are softened into the bar's colour by the same rule as the scene under the names, so the
  dim text keeps 4.5:1 (tested on four themes). A shooting star falls every 4 seconds, a bird
  crosses from right to left every 9, and a light runs the length of the bar's foot along its
  frame every 6. None of the new ones shows in the first cycle, so the still frame is calm.
- **Arabia Solar's world.** A theme cannot pass an effect anything but its colours, so the
  scene has two faces chosen by the theme's mode: by night (a dark theme) the castle in the
  hills, and by day (a light one) a desert kingdom: a palace with a great onion dome and spire,
  a small dome on each wing and a minaret at each end, its domes warmed with the focus colour;
  palms for pines; two lines of dunes for hills; sand with dry tufts and a trail for grass; and
  a carpet rippling across the sky where the bat flew. Behind text the foot of a terminal or a
  page shows two faint dunes, 2% and 3.5% toward the text's colour, where the night's hills
  would be white on near white. Everything else (frames, ledges, gems, lanterns, cursor, arrow,
  typed text) is the same effect. The art is original and takes only the motifs of the old
  tales: no character, building or design of any film.

## Seventh pass, 12 November

Cyrus: "make the text effects aladin themed too".

- **Sand into ink.** By day (a light theme) a typed-in character keeps the three cuts over
  200 ms, but tells the desert's story: a scatter of gold grains 1.5 points square, about a
  third of the glyph's, a grain above where they will lie; then three quarters of them in the
  theme's accent, half a grain above; then the whole glyph, settled and still warm; then ink.
  A grain is there or not by its place on the screen alone, so each cut holds still. By night
  the character still resolves from blocks. The window already tells a glyph shader whether the
  ground is light (`ARKOS_LIGHT_GROUND`), so nothing new is passed.

## Eighth pass, 12 November

Cyrus: "add a little more background padding behind the title, stop light buttons and icon
buttons in the app bar", and, with a screenshot of the Worktrees header at the sidebar's foot
over the palace: "the worktrees explorer drops to the bottom when the file explorer opens".

- **Plates in the title bar.** The bar's words, its icons and the traffic lights each sit on a
  plate: the bar's own plain colour, 9 points wider than what it holds on each side, as tall as
  the inside of the frame, corners notched, with no sky, star, cloud or bird on it. The lights'
  plate is drawn only when the words start past the lights (not in full screen). The manifest's
  `title_pad` moves the words and icons that far in from the frame. Beyond a plate, what
  comes within `STAR_CLEAR` of a group is still softened.
- **Closed Worktrees.** With the Worktrees section closed its header now sits right under the
  Explorer's last row instead of at the sidebar's foot, where it lay over the scene and dimmed
  all of it; a tree longer than the sidebar still scrolls above a header at the foot. This is
  the sidebar's layout under every theme. The split also used the panes' height for the
  sidebar's, which under an effect's gap is two gaps short: it now uses the sidebar's own.

Cyrus, on the eighth pass: "i dont see the title bar padding updated." On Arabia Solar the
plates were the bar's colour on a sky dithered toward white, so nothing told them apart. The
sky is again a step darker than the bar on every theme (the words are on plates now, never on
the sky), a test holds the sky's mean luminance apart from the plates' on four themes, and the
padding is 9 points (`PLATE_PAD`, `title_pad`).
