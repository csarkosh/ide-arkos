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
and the sky glowing toward it rather than flashing white. `code` was first towers of falling code with a ringing phone booth and a
black cat crossing twice; Cyrus, 2026-11-17: "not impressed by the graphic in Pane 0 for The
Matrix", and he picked a digital horizon over a code waterfall, a corridor of doors and a rabbit
with two pills. Now: a faint code rain in the sky; a line of light on the horizon (40% down the
scene); a floor of grid lines in perspective, its rows coming toward the viewer; First four towers rose and folded back in turn; Cyrus, 2026-11-17: "the buildings to
look like they are moving towards and past the user at the same speed as the floor", "so many
buildings that it feels dense". Now a tower stands on four in five of the grid's cells, six lanes
either side of a road one cell wide each side of the middle, rows out to 30 camera heights; they
stand still on the floor, so they come on with it (the camera has run `t * GRID_SPEED * GRID_Z`
into the city) and pass, fading up out of the horizon far off; drawn farthest row first, each
face hiding what is behind it, its back edges drawn so it reads as a box. City life ("some city life added between the buildings"):
every fourth row is a cross street with no towers, two cars crossing it each way between the
blocks (headlight leading, tail light following); five cars a lane on the road, one lane coming
on at the car's speed and the floor's (headlights), one going away at the car's less the floor's
(tail lights); lit windows, about one in five, on the faces of towers near enough for a window to
be three points wide, some switching on and off every few seconds; three drones blinking as they
drift over the roofs. The streams of code that fell to the floor and spread a ripple went (Cyrus:
"remove the animation ... that drops and creates a disk"), and people walk the sidewalks instead:
seven a side just off the road's edge, each walking toward or away at its own pace over the
floor's, and three on each of a cross street's two sidewalks, crossing left or right at their
own pace, as the cars move ("the same horizontal and vertical movements as cars, but at
different rates and in non uniform directions"); 0.16 of the camera's height tall, in scale with
the towers ("more proportional with the buildings"); a head, a body, arms and legs swinging
twice a second, a dot of light where too far off for a figure. The sky's rain runs from half the sidebar's height (or the
scene's top) down to the horizon ("the runic rain in the background of the graphic to have its
height increased to half the pane size"), softened under the names. It comes toward the viewer with the city ("make the
rain look like it's moving towards the camera too"): six sheets of falling code at depths spread
over 24 camera heights from 2 out, each sliding toward the viewer at the floor's speed, so its
columns spread out from the vanishing point and its glyphs grow as it nears; a sheet fades in
far off and out as it passes, and a third of each sheet's columns carry a stream, so the sheets
show through each other.

The clock's zero (Reduce Motion) has no bolt, nothing falling, every tower standing and every
light on.

## The Matrix, softened

Cyrus, 2026-11-17: "the green text on black background is too intense for my eyes". The ground is
a very dark green-grey (#0b100d) instead of black, the text a sage-mint (#a9d1a6, 11.3:1) instead
of phosphor green, and the greens and the ANSI colours lose most of their saturation. The runic
effect softens its own greens with the theme: `toward_accent` now also scales their saturation by
the focus ring's over the green they were made around (`HOME`), never raising it, so under The
Matrix's soft ring the rain, the glows and the text's halo lose colour too, and under every other
built-in (whose rings are at least as saturated) nothing changes.

## Panes flush with their frames

The room between a pane's own rect and its frame's line (5 points) is the pane's, cut at the
frame's corners: the strip fills it beside and above the tab bar, a first active tab reaches the
frame on its left, and the body's night (its band, its skyline, measured from the frame, and its
rain) fills it beside and below the body (Cyrus, 2026-11-17: "flush with the pane's borders and
match the shape too").

## The sidebar keeps a pane's margins

The manifest's `sidebar_margin` has the app keep the edge margin round the sidebar's column, as
round a pane: from the window's edge, under the title bar and above the foot (the gutter to the
panes is the gap already). The sidebar block is that column, and the wash frames it as a pane, 5
points out, the ground reaching the frame and cut at its corners (Cyrus, 2026-11-17: "Pane 0 to
have the same margins around it as Pane 1"). Then, "decrease the space between Pane 0 and the
other panes" and "the margin between the panes and the window edges": a new key, `edge_gap`
(11 for runic), keeps the panes and the sidebar off the title bar and the window's edges, the gap
(16) staying between the panes and beside the sidebar; so 6 points of ground lie beyond every
frame, to the window's edge, under the title bar and between any two frames. Its rows' pad
(`sidebar_pad`) is 2, room for the chevron, the frame lying outside the column. A press anywhere
in the sidebar gives it the keys, and a click on a session shows it and keeps the keys in the list
(Cyrus: "clicking on Pane 0 or one of its sessions should cause it to go in focus").

## The city keeps the law

Cyrus, 2026-11-17: "the pedestrians and car traffic ... should follow traffic and pedestrian
laws", and "make the pedestrian traffic denser. It should feel like NYC". Every intersection runs
one signal cycle from the clock's zero, 12 s: the road green 6.5 s (its last second amber), red
both ways 0.5 s, the cross streets green 4.5 s (its last second amber), red both ways 0.5 s; a
lamp on a pole at each corner shows it, the road's on the near corners and the cross street's
on the far ones. Cars keep to the right: coming on in the road's left lane, going away in its
right; going right on a cross street in its near lane. A road car waits at its stop line (0.4
behind the cross street's edge, behind the crosswalk), goes on the green one or two blocks at its
own pace with a gentle start and stop, and comes to the next stop line after the light has
turned; its trips are timed so every intersection it crosses is crossed on the green (a test
mirrors its arithmetic and checks it over ten minutes). A cross car comes in from the side,
queues at its stop line, and goes in turn on the cross streets' green. Crosswalks are zebra
stripes, over the road beside each cross street and over each cross street beside the road.
People keep to the sidewalks. On each of the road's (Cyrus, 2026-11-17: "more pedestrian traffic
moving vertically", "distributed so the pedestrian traffic looks fuller"), forty walkers and
forty-eight strollers (forty walkers before, all stepping off on the green and standing at the
next corner, so the sidewalks filled in waves and emptied between them). A walker (`road_walker_at`)
crosses the cross streets with the road's green and waits at the corner otherwise (standing, its
legs together); most are brisk, a block every cycle, so they hardly stand, the rest a block every
two, and each steps off at its own time in the green, early enough to be over before it ends. A
stroller (`road_stroller_at`) comes out of a door or round the corner from a cross street's
sidewalk and walks along its block into another door or round the far corner, never into a
street, so it keeps no light and comes and goes at its own times, fading in and out at the door.
Every block overlaid, a sixth of its sidewalk has four to seven people walking in it on average
and is empty under five per cent of the time (under three, and a third of the time, before);
`runic.rs` checks both the laws and that fullness on a mirror of the arithmetic. Twelve on each of a cross street's two sidewalks, out of a doorway, to the
corner, over the road on the crosswalk with the cross streets' green, into a doorway on the far
side. Spread across the sidewalk's width, not single file. Every motion is locked to the cycle,
so it is worked out from the clock alone; the city wraps every four blocks, so whatever wraps is
still at a stop line or a corner.

## Busier road

Cyrus, 2026-11-17: "there's not enough vehicles moving vertically. Only horizontal movement across
the graphic looks like NYC's traffic density". Twenty-four cars a lane on the road (five before),
queued up to four deep behind a stop line a car's length (0.3) apart; only the first at the line
goes two blocks on a green, so none behind it is still crossing when the light turns (the law
test, mirroring them, caught the first try at that).

Cyrus, 2026-11-17, later: the cars must "not drive through each other or completely overlap each
other when stopped at a light". Each car drew its block, its place in the queue and its pace at
random, so two often shared a place, a faster one ran through a slower one ahead, and with the city
repeating every four blocks a car eight blocks on stood where another did. Now every car has its own
block of the four and its own place in that block's queue, six deep, 0.3 apart; each green the
queue goes one block on as a platoon at one pace (6.5 to 8.5 seconds, drawn for the block and the
cycle), each car 0.25 seconds after the one ahead, so the gaps only open while it goes and close back
to the queue's at the next line. A test mirrors the cars and finds no two in a lane ever within a
car's length (0.25), moving or stopped, besides the law test.

## Cyclists

Cyrus, 2026-11-17: "include cyclists on the street traffic too". Three each way ride the road's
bike lanes by the curbs (keeping to the right, as the cars), seen end on: the bike an upright,
the rider seated and pedalling while it moves, a headlight coming on or a tail light going away.
They keep the law as the road cars do but slower, one block a green (8.5 to 11.5 s, setting off
up to 0.8 s late), clear of the intersection they cross before the light turns; the law test
mirrors them too. On every cross street one rides each way in its bike lane by the curb, seen side
on (two wheels, a frame, the rider, a headlight ahead and a tail light behind), waiting at the
stop line ahead of the cars and going first on the cross streets' green. To pay for them, a pixel
tests only the sidewalk crowd on its own side of the road.

## Parks on open ground

Cyrus, 2026-11-17: "there's also blank space that is not occupied by streets or buildings. If the
blank space is large enough, I'd like something like a park". A lot with no tower (one in five)
that touches another such lot, across or along, is open ground big enough for a park: its grass
is the ground tinted and hatched, three trees stand on it (a trunk and a canopy of light, its
dark fill hiding what is behind it, leaves in it) and a lamp post lit warm, at places of its own,
drawn farthest first. A lone empty lot (too small for a park) holds, by the lot, one of
three Cyrus chose from four proposals (a parking lot was the fourth): a subway entrance (a
stairwell going down, its steps running away from the head beside the road, a railing round it,
two green globes on posts at its head, one person going down into it and one coming up, sinking
and fading on the stair), a food cart (a box with its lit front on two wheels under a striped
umbrella, a queue of four at it toward the sidewalk; every four seconds the first walks off and
the rest step up), or a basketball court (its lines on the ground: the boundary, the half-court
line, the centre circle and the keys; a fence rail round it; a hoop at each end, a pole, a
backboard and an orange rim; four players running up and down and side to side).

## Menus read over the rain

The panes' paper went to 0.6 so their top and foot show, and the menus, whose paper was the
panes', went see-through with it (Cyrus, 2026-11-17: the "Remind me in" menu "is not opaque
enough"). A new manifest key, `menu_paper_alpha` (absent, `paper_alpha`), gives the menus their
own: 0.92 for runic. The "Remind me in" and "Move to" card beside a session's menu had no backdrop
of its own at all, its fill clear under an effect that paints the menus; it reports itself as a
menu now (key 0x18), so it gets the backdrop and the paper as the menu beside it does.

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

## Sidewalks

Cyrus, 2026-11-17: "I want the sidewalks more prominent". The floor is paved where the people walk:
a band 0.32 wide along the road from just inside its edge, and 0.32 along both sides of every
cross street from its curb (the crosswalks over the road already continue them). The paving is a
soft wash of the city's green over the dark floor with faint joints, and a lit curb runs where
each sidewalk meets a street. A test holds every walker's and stroller's place inside the paving.
