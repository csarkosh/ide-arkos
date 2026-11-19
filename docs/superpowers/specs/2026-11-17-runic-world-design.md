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

## The city's data

Cyrus, 2026-11-17: "explore the idea that the runic rain comes from users uploading and download
data ... runic rain can move in the reverse direction. It also means it goes to and originates
from pedestrians, bicyclists, cars, food carts, and buildings." He chose that all the rain is data
(the ambient sheets went), that direction alone tells an upload from a download, and a steady
trickle. Fourteen slots each start a transfer every 5 to 9 seconds between the sky and one thing in
the city (a walker 34 per cent, a car 22, a cyclist 10, a tower's window 24, a food cart 10, its
lot found among a few tries), which it follows as it moves; out of the nearer part of the view it
tries another, up to five. While it runs, its whole channel from the sky's top to the thing shows
(0.5), and a packet runs along it, its head bright with a soft glow round it and 14 cells of trail
fading behind: down onto
the thing for a download, up out of it for an upload, crossing in 2.2 to 3.6 seconds, the thing
flickering as a download lands or an upload leaves. The glyphs are the rain's; a column too narrow
for them shows lit dashes. `effects::runic` mirrors the slots: eight to thirteen at once, never
fewer than three. Seen moving in a window (2026-11-17, a 260 point sidebar), the first cut read as
a few faint dotted columns over the roofs: a channel at 0.3 and a one-cell packet are a few points
of green at that size, and a transfer that found nothing in view among three tries drew nothing.
The packet's head gained a glow (`DATA_HALO`) and the tries went to five.

Then, in the sandbox: "there's not enough runic rain yet. there should be many small ones so it
feels like the matrix with everyones connected devices to the cloud", and of the brighter channel
tried first, "the rain in the preview is too noisy right now". Then: "the uploading and downloading
rain should be tied to people, cars, bicycles, or tops of buildings. And it should be a little
slower. It looks random to me." So a second layer runs beside the fourteen (`data_drizzle`): 36
slots, each running a transfer for three quarters of a 4.5 to 9 second cycle (3.4 to 6.8 seconds
across the sky), down onto or up out of one thing it follows: a walker (45 per cent), a car (20), a
cyclist (10) or a building's roof (25), within four fifths of the view, else the slot rests. Its
side of the road is fixed for the transfer, so a pixel on the other side of the vanishing point
passes it by without working out where its thing is: that, one try, and the farther reach keep
its cost near the fourteen's. `effects::runic` mirrors it: 20 or more at once on a 260 point
sidebar, never fewer than 10, every kind connected, and a roof held through its transfer. Each is
a column 3 points wide, a lit dash a cell, a bright head and 9 cells fading behind, at 0.32, with
a flicker on the thing as it lands or leaves. The first cut of this layer landed each column
wherever a sidewalk or a lane passed under it, which often fell between people and read as random.

The fourteen changed with it: a building's transfer ends on its roof, not a window partway up,
and its building is chosen among those in view when the transfer begins and held after (it was
chosen by where the camera was each moment, so a transfer jumped to another building every two
seconds or so); a transfer crosses in 3.5 to 5 seconds, not 2.2 to 3.6, in a 6 to 10 second cycle.
To keep it calm the glyphs and dashes change once every 1.2 seconds rather than every 0.3 to 0.4,
the fourteen's channel is a hint (0.2), their glow soft (0.3), and the whole of them at 0.8 of
their light (`DATA_LIGHT`, "make the rain slightly dimmer in the graphic").

## The subway

Cyrus, 2026-11-17: "a subway train ... it should appear dimmer because it runs underground and it
should be obvious to the user that the train is underground. The subway train should move faster
and without stop since it doesn't need to follow traffic laws." A tunnel runs under the road, seen
through the ground: its walls dashed lines 0.36 to either side, at its roof (0.18 below the street)
and its floor (0.62). Every 14 seconds a train of five cars, each 1.1 long, runs through it at 4
world units a second (a road car averages about 0.53), never stopping, one coming on and the next
going away: its roof 0.24 below the street, a faint shadow, its outline in hidden lines (soft dashes
0.22 long that travel with it, along its sides and across its ends, as a drawing shows what lies
under a surface), two rows of lit windows down its sides and a headlight glowing ahead of it in the
tunnel, all at 0.3 and drawn before the street's grid, which lies over it. `effects::runic` mirrors
its passes: never a stop, coming and going in turn. Seen moving in a window (2026-11-17), the first
cut, its outline solid and crisp at 0.38, read as a tram on the road's median; dashed and dimmer it
reads as under the street.

## New York's cyclists and walkers

Cyrus, 2026-11-17: "in NYC, bicyclists never obey traffic laws. So allow them to break the traffic
laws as long as another car does not hit them. They are crazy and will run a red light even if they
barely avoid an accident", and "Same thing with pedestrians". Every cross street's cars have
crossed the road's bike lanes and crosswalks by 9.4 seconds into the 12-second cycle and none comes
back till the next cross green. So a cyclist never stops: a block a cycle, or two for a messenger,
reaching each stop line just after the last cross car has passed (9.4 to 10.4 s, on the road's
red) and riding straight through. Sixty per cent of the walkers step off the curb against the light
in that same window, right behind the cars; the rest still wait for the road's green. Cars keep
every law. `effects::runic` checks every cyclist and walker against every cross street's moving
cars for six minutes: no one is ever hit, and the reds are run thousands of times.

Cyrus, 2026-11-18: "make the pedestrians and bicyclists jaywalk more often as long as they don't
get hit by a car or another bicycle. NYC people are very aggressive jaywalkers", then "they will
cross the street as soon as the cars have passed" and "same thing with the bicyclists". Everyone
now goes the moment their way is clear, and the moments are measured from the traffic in the tests
rather than chosen: 85 per cent of the walkers step off against the light at 9.1 s, together
within 0.3 s, right behind the last cross car and cyclist over their crosswalk (passed at 9.04);
the road's cyclists reach the stop line at 8.9 s and ride through the red behind the same cars; the
cross streets' cyclists set off over the road at 5.8 s, the first moment no road car is moving in
the intersection and no road cyclist or walker is where they cross, 1.2 s ahead of their own
green. The six-minute check now holds every walker and road cyclist against the cross streets'
cyclists as well as their cars, and nobody is hit. While the last car goes by a few jaywalkers
stand at each corner, up to 29 of a sidewalk's 88 people at once.

Cyrus, 2026-11-18: "people waiting to cross the street should be more spread-out in a group. I
don't want too many stacked directly on top of eachother." Every walker used to wait on the curb's
line, half of them on top of another (within a person's width across and a body's depth along, the
view's repeat every four blocks counted). Now a jaywalker waits anywhere up to 1.0 back from the
curb (WAIT_DEPTH; one who keeps the law waits at the curb, its crossing timed from there) and the
walkers use nearly the sidewalk's whole width (WALKER_SPREAD 0.3, was 0.24): 6 per cent overlap.

Cyrus, later on 2026-11-18: "walkers and cyclists should start walking before the cars have
passed. NYC people are crazy. They will time their walking so they're already crossing the street
as the car is passing. The crowds also get closer to the car as the last few are passing because
they are impatient when waiting to cross the street. Also, the crowds waiting to cross the street
are not spread out enough." Each step-off is now the earliest moment that never puts anyone in a
moving car's or cyclist's lane, lane by lane, searched at a hundredth of a second: walkers at
8.85 s (the last car clears their crosswalk at 9.05), the road's cyclists at their stop line at
8.2 s, the cross streets' cyclists at 5.8 s. A jaywalker waits up to 1.6 back from the curb
(WAIT_DEPTH) and in the last 2 s (CREEP_T) edges up to 0.3 of that (CREEP_KEEP), the front of the
crowd up to 0.3 off the curb (GUTTER), short of the cross street's near lane. Under 3 per cent of
the waiting people overlap another. The searches made the runic tests about ten times slower
(13 s, in parallel with the rest of the suite).

Then: "walkers are still stacking on top of eachother when they are making horizontal street
crossings. Also, pedestrians should start cross in front of oncoming traffic if they know the light
is about to turn red for them." The people crossing the road moved into `road_crosser_at`, with a
mirror in the tests that never had them before. They used to step off within a tenth of a second
of their green from one spot, over six tenths of the crosswalk: 39 per cent of them on another.
Now each waits at its own spot, up to 0.2 back from the corner (CROSS_WAIT_DEPTH) and up to 0.5
along the road's sidewalk beside the crosswalk (WAIT_ALONG), comes onto the crosswalk in its first
0.3 (CROSS_MERGE), uses its full width, and steps off at its own moment over 3 s (CROSS_SPREAD)
from 5.6 s (CROSS_FIRST), the earliest that keeps the crowd out of every road car (the road's cars
have cleared the intersections by then, 1.4 s before the walkers' green), each still over the road
0.3 s before the road's green (CROSS_CLEAR) however slow: 10 per cent on another. Three in ten
(LATE_SHARE) come late and hurry over at 0.7 to 0.85 a second, stepping off in the half second
before 10.05 s (LATE_GO), the latest that keeps them out of the road's cars as those get their green
at 12. The road's stop lines moved back from 0.4 to 0.55 before the cross streets (STOP_BACK): a
car rolling up to one stopped with its nose over the crosswalk's edge, among the people on it.

The road's cyclists and the people crossing the road: a cyclist never stops (a messenger passes
an intersection every 6 s) and the crosswalks have people on them five seconds in twelve, so no
timing kept them apart (every start from 0 to 11.5 s left 12 to 223 of 384 crossers in a cyclist's
way, as it had been since the cyclists came). Cyrus chose "Both, a near miss". A crosser about to
step into a bike lane works out, from the cyclists' steady pace, whether one will go through its
crosswalk while it is in the lane (`bike_wait`), and if so stands at the lane's edge until it has,
NEAR_MISS (0.03) clear of it; a cyclist going through a crosswalk where someone stands at its
lane's edge eases up to SWERVE (0.04) away from them (`bike_swerve`, drawing only). The tests now
hold every crosser against the road's cyclists as well as its cars, and nobody is in either's way;
the late crossers, held up now and then by a cyclist, step off by 9.9 s. It costs the wash about
0.6 ms a frame at 1200 by 800 under load (4.2 to 4.4 ms before, 4.9 to 5.0 after).

## Sidewalk sheds and scaffolding

Cyrus, 2026-11-18: "the Matrix City needs some scaffolding alongside some buildings so it feels
more like NYC." He chose sidewalk sheds, some with scaffolding above, over sheds alone or
scaffolding alone. About three in ten of the towers beside the road wear a sidewalk shed: a pipe
canopy over the road's sidewalk along the tower's road side, from its front face to its back,
its deck above a person's head, its posts standing on the sidewalk at the curb, never in the road.
About one in three of those also has scaffolding up the tower's road side: standards and a ledger
at each floor, a few floors up, never above the roof. The people on the sidewalk walk under the
deck. Only the towers beside the road have a sidewalk to shelter, so only they wear one, which
keeps the cost to a few segments per pixel near them. The deck has a faint fill of boards seen from above, so it reads as a
roof over the sidewalk rather than a fence along it. The wash measured 5.02 ms a frame at 1200 by
800 before and 4.92 ms after, within the bench's run-to-run noise.

Approaches: drawn with the tower that wears it (chosen), from its own lot and the tower's box, so
it can never stand apart from its building; or a layer of its own over the scene, rejected since
it would look up the tower's box a second time.

Then: "The scaffolds and sheds are not visible enough." Their pipes are now lit as brightly as a
tower's edges (SHED_LIGHT 0.85, in the towers' whitened green), the deck's boards fill at 0.32
(SHED_DECK_FILL) and the scaffolding wears netting over its face at 0.22 (SCAFFOLD_NET); and more
of them: 45 in 100 towers beside the road wear a shed (SHED_SHARE) and 55 in 100 of those have
scaffolding (SCAFFOLD_SHARE).

## The food cart's line keeps to the sidewalk

Cyrus, 2026-11-18: "the line of people waiting for the food cart stretches into the street. It
should stay within the sidewalk." The line ran from the cart toward the road, 0.13 a person: by the
road the cart stands at the sidewalk's back edge, so its third and fourth people stood past the
curb. Now the first stands at the cart's road side and the line runs along the sidewalk from
there, everyone at that one place across it: on the road's sidewalk beside the road, inside the lot
for a cart further out. Stepping up and the first walking off with their food go along it too.

## Water towers, steam and subway entrances

Cyrus, 2026-11-18, asked what else would make the city feel like New York and chose three of
the ideas offered: rooftop water towers, steam from manholes and subway entrances ("do 1, 2 and
3"), approving the design ("approved, go with your recommendation"). The sizes below are the
built ones; the approved design had them nearer life size, and stills at the sidebar's size showed
each too small to read, as the sheds had been.

Water towers. About 60 in 100 towers of middling height (0.9 to 3.0 tall; TOWER_MAX is 4.8) wear
one, as Manhattan's walk-ups and mid-rises do and its tallest towers do not: a barrel 0.16 across
on three legs 0.07 tall, its tank 0.15 tall with two hoops, under a cone roof 0.07 tall, standing
at a place on the roof drawn for the tower and never over its edge. It is drawn with its tower,
after the tower's own lines, its tank and roof filled dark so the lines behind them are hidden, its
outline lit as a tower's edges are; where a roof is above the eye the tower's front face hides the
legs' feet, and nearer towers still cover it. At 0.09 across (the first cut) it drew two points
wide in the sidebar.

Steam. Each half of a block between its crosswalks has a manhole breathing steam at 70 in 100, in
a traffic lane (0.15 to 0.55 from the road's middle, clear of the bike lanes): a soft plume 0.08
wide at the street, swelling to 0.45 at 0.9 high (about two stories) and drifting up to 0.1 aside,
so it never reaches the sidewalk, its noise rising through it and puffing harder and softer over a
few seconds, lit at 0.7. It is drawn with the row of the city its manhole is in, after that row's
towers (it stands inside the curb, clear of their faces) and after the road's traffic, so a car's
lights dim inside it and only nearer rows' towers cover it; drawn before the whole city at first,
farther towers' faces cut its upper half out. The first cut, three people tall with one
manhole a block, drew four points high halfway up the road and its density came out at 0.2 to 0.4
in a false-colour still. The orange and white Con Ed stacks were left out: one would stand in a
lane the cars drive through.

Subway entrances. About a third of the corners where a cross street meets the road have one (90 in
100 of the corners beside a tower with no sidewalk shed and its face at least 0.02 clear of the
railing; never beside an empty lot, which may be a park or hold a lone entrance of its own that the
crowd pressed aside would walk through), at the road's curb, 0.9 to 1.22 past the cross street's curb, clear of the
people waiting at the corner and the crossers stepping off it: the stair's dark hole with its steps
going down away from the opening, a railing 0.1 high round three sides, 0.09 wide from just inside
the curb, open toward the cross street, and a lamp post at each side of the opening under a globe
0.26 up (as tall as the lone lots' entrances'), green (open all hours) or for one in four red (exit
only). The design first put it at the buildings' side of the sidewalk; the walkers spread across
the whole sidewalk and the towers' faces stand at its back, so it went to the curb, where New
York's entrances mostly are. The road's walkers and strollers keep out of it (`sidewalk_x`): beside
an entrance their places across the sidewalk are pressed into the part past the railing, at least
0.03 clear of it, fully aside 0.03 before it starts and after it ends and easing in and out over
0.15, and the city's data lands on them where they are drawn. Drawn after its row's towers and
before the nearer rows.

The wash measured 4.76 ms a frame at 1200 by 800 on the Matrix theme before and 4.74 ms after.

## Seattle, San Francisco and London

Cyrus, 2026-11-18, asked for "seattle rain", "frisco rain" and "london rain", each with "their
iconic cityline". Runic's scenes are now `tokyo`, `matrix-city`, `seattle`, `san-francisco` and
`london`, and three presets draw them: Seattle Rain, Frisco Rain and London Rain, in one Rain
group with Tokyo Rain (its default). Each is the Rain colours with the runic effect and its scene,
and has no font of its own. Scene 1, the Matrix street, is drawn exactly as before; scenes 2, 3 and
4 never draw it, and Tokyo's two towers stay Tokyo's alone.

Each city is the sidebar foot's far skyline with its landmarks drawn among it (`seattle_at`,
`san_francisco_at` and `london_at` in `wash.metal`), lit as Tokyo's towers are: edges in the white
and green of the theme, a red beacon on the tallest, colours from the theme only.

- Seattle: the Space Needle at about 0.3 of the width (a spire, the cap, the saucer with its rim
  lit white, a stalk narrowing to the waist), the Columbia Center at about 0.74 (the tallest tower,
  stepped in near its top, its ledges lit), and Mount Rainier faint behind the far skyline
  (`rainier_at`: a low snow-capped peak with a ragged snow line and a faintly lit ridge).
- San Francisco: the Transamerica Pyramid at about 0.28 (a tall thin pyramid with its wings),
  Salesforce Tower at about 0.6 (rounding to its top, the crown a lattice of light that breathes),
  and at the right edge a Golden Gate tower pair with the deck, the cable's curve and its
  suspenders, drawn in the theme's red mixed a little toward its yellow.
- London: the Elizabeth Tower at about 0.17 (spire, belfry, clock stage and shaft) with a lit warm
  clock face and its hands, the Shard at about 0.63 (a glass pyramid broken into splinters at the
  tip, its facets faint), the London Eye at about 0.4 (its rim, spokes and pods turning slowly,
  and its legs) behind them, and the Gherkin at about 0.86 (a rounded bullet with spiral bands).

Cost, the runic wash at 1200 by 800 on the development MacBook Air (`effects::bench`, one session,
with the bench pointed at each preset in turn): Tokyo Rain 1.863 ms, Seattle Rain 1.751 ms, Frisco
Rain 1.755 ms and London Rain 1.771 ms, each the best of four runs. The runs on this machine swing
between about 1.8 and 4.7 ms from one to the next whichever scene is drawn (Tokyo Rain's own ran
1.86 to 4.73), so only the best of several is comparable; by that measure no city is more than
0.3 ms over Tokyo, and none costs more than it.

The review of 2026-11-19 found the Golden Gate's far tower past the band's right edge at every
sidebar width, Salesforce's crown and the Shard's tip in the band's faded top 60 points, Rainier's
flat foot showing where a far tower was low, and the London Eye running over its neighbours on a
narrow sidebar. The pair now stands at 0.8 and 0.95 of the band's width (inside it from 160 points
up), Salesforce's top at 0.16 of the band's height and the Shard's at 0.07 with its beacon above
its tip, Rainier's slopes run down to the band's foot behind both skylines, and the Eye's radius is
the smaller of 0.17 of the band's height and 0.12 of its width.

## Lower Manhattan down the street

Cyrus, 2026-11-19: "for the Matrix City, I want a cityline in Manhattan looking south at FiDi in
the background", approving the design offered. The street runs south: at its far end a hazy
silhouette of the Financial District stands on the horizon (`fidi_at`), drawn before the street's
own buildings so they cover it, in the far skyline's shade with its outlines lit faintly and a few
windows. Between the street's two rows only about 15 points either side of the vanishing point
are open sky, so the district is drawn there, as an avenue sees it: One World Trade Center in the
middle and tallest (its faceted taper to the roof at 0.55 of the sky's height, its spire to 0.66,
its beacon), the Woolworth Building's Gothic crown and 70 Pine's Art Deco setbacks to its left,
8 Spruce Street's rippled sides and 40 Wall Street's pyramid roof to its right, among plainer
towers. The design's data rain landing on its roofs was left out: the transfers choose among the
street's own buildings, and the far district is a few points wide.

Then, of Seattle Rain (Cyrus, 2026-11-19): "make the space needle more accurate to how it looks.
And balance the heights better with the Columbia Tower." The Needle is drawn from its silhouette,
its widths shares of its height: a thin spire over a small cap, the top house's roof sloping out to
its rim (the widest point, with the observation deck's lit band) and its underside sloping back in
more steeply, then its legs tapering in to a waist a third of the way down and splaying out wide
to the ground in an hourglass, sky between them, the 100 foot level's ring low on them. Its top
stands at 0.1 of the band and the Columbia Center's at 0.13 (it was 0.04, into the band's faded
top, far over the Needle), so the two stand about as tall, as they do from the city's south.

Then Cyrus sent a photograph of the skyline from Kerry Park as the one he likes, and Seattle was
recomposed to it: the Space Needle at the left (0.17 of the band) and in front, its top at 0.1;
downtown crowding just to its right (0.22 to 0.58), a band of glass towers with lit windows among
which the dark Columbia Center (0.31, its top at 0.2, about level with the Needle's top house,
stepped in near its top, its beacon), Two Union Square's notched top and 1201 Third Avenue's
pointed, lit crown stand out; and Mount Rainier broad and snowy on the right (its peak at 0.82 of
the width and 0.36 of the height, its flanks falling a third of the width either side, a shoulder
to its left, the upper part of what shows above the city under snow with a softly ragged line).
The towers show through between the Needle's legs.

Then (Cyrus, 2026-11-19): the towers between the Needle and Rainier "just looks like a bunch of
odd lines. Make it look cleaner". Each tower had been outlined on both sides, so wherever two
overlapped a stray vertical line stood in the crowd. Downtown is now one silhouette
(`downtown_top`): a run of lower buildings at half the band's height so no slot opens down to the
near skyline at any sidebar width, plain towers standing on it, and the Columbia Center (darker,
stepped in twice near its top, its beacon), Two Union Square's notch and 1201 Third Avenue's
pointed crown (lit warm) as steps and points of the same outline. Only the silhouette's own edge is
lit: along its roofs and down its outer sides.

The review of 2026-11-19 found that on a narrow sidebar each district landmark's lit sides ran
through One World Trade Center's body, and that, looking south, the Woolworth Building, 8 Spruce
Street, 70 Pine and 40 Wall Street, all east of One World Trade Center, belong on its left. The
district is now one silhouette (`fidi_top`, lit only along its outline, as Seattle's downtown is):
One World Trade Center just right of the vanishing point (0.025 of the width) and, stepping away to
its left, the Woolworth Building, 8 Spruce Street, 70 Pine and 40 Wall Street, among plainer
towers on both sides; One World Trade Center's facets drawn faint inside its body and its beacon
above its spire, never cut by the district's bounds.
