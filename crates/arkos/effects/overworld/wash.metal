// The overworld wash under the Arkos window: the night overworld of the handheld fantasy games, in
// chunky pixels, across the whole window. The window stands on a dithered night ground, seen in
// the gap the manifest keeps round the panes. Each pane wears a game menu's frame in that gap: an
// outer line and a dark inner one, their corners notched, the outer line in the focus colour on
// the pane that has the focus and carrying a slow glint while the pane's Claude session works. A
// pane whose session waits for the user hangs a small "continue" pennant from the right of its
// frame's foot. Each tab bar is a ledge of stone with the active tab a lit doorway in it, a lantern
// before its label. The title bar is a strip of sky: stars along it and one slow cloud, both clear
// of its words and icons. At the foot of the sidebar, below its last name, stands the scene: grass
// with a winding path, pines, low hills, a castle with lit windows and a pennant, and a sky with
// stars and a crescent moon (by day, on a light theme, a sun); a short foot cuts it from the top and under SCENE_MIN_H there is
// none. The selected row has the menu's cursor in its left padding. Behind a terminal the night
// goes on: a dusk at its top, a few faint stars and two lines of hills along its foot.
//
// What is behind text is kept out of its way. Behind a terminal or a document everything but the
// stars moves away from the text's colour (darker on a dark theme), and a star reaches the screen
// no further than STAR_LIMIT toward the dim text. A tab's paper covers the wash at PAPER (the
// manifest's paper_alpha), so the wash draws its night that much stronger. The ledge's joints are
// darker than its stone.
//
// Every colour is the theme's, read each frame. The art is mixed MUTE of the way toward the
// sidebar colour; the lit things, the castle's windows, the lantern's flame, the fireflies and
// the cursor, keep their full colour. An art pixel is ART_PX points square, counted from its
// block's origin, so at scale 1.0 its edges land on whole points and stay crisp. The clock's zero,
// the still frame under Reduce Motion, has every star bright, every window lit, every firefly
// glowing, the cursor and the pennant at their brightest and the glint at its frame's top left.
//
// The constants and the arithmetic of the layout and the timing are mirrored in Rust
// (`src/effects/overworld.rs`) for its tests; a change to one is made to both. The preamble
// (arkos.metal) is prepended before this is compiled, and declares ArkosInputs, ArkosBlock,
// ArkosVertex and the ARKOS_* indices used here.

// The art pixel, the scene's tallest and least heights, and the title stars' clearance, in points.
constant float ART_PX = 3.0;
// A frame's width in art pixels: a pane's in the gap round it, the sidebar's and the title bar's
// inside their own edges.
constant int FRAME_PX = 2;
constant float SCENE_MAX_H = 170.0;
constant float SCENE_MIN_H = 50.0;
constant float STAR_CLEAR = 12.0;

// A castle window dims every DIM_CYCLE seconds, give or take half of DIM_SLACK, for DIM_FOR
// seconds, and comes back; cycle 0 has none. DIM_DEPTH is how far it dims at the deepest.
constant float DIM_CYCLE = 5.0;
constant float DIM_SLACK = 2.0;
constant float DIM_FOR = 0.8;
constant float DIM_DEPTH = 0.7;
constant int CASTLE_WINDOWS = 5;
constant uint DIM_SALT = 0x85EBCA6Bu;

// The fireflies: how many, the slowest and quickest rise in seconds, and the share of the rise
// each fades in over, stays lit until, and is dark from.
constant int FIREFLIES = 6;
constant float FIREFLY_PERIOD_MIN = 8.0;
constant float FIREFLY_PERIOD_MAX = 11.0;
constant float FIREFLY_FADE_IN = 0.1;
constant float FIREFLY_FADE_FROM = 0.8;
constant float FIREFLY_FADE_TO = 0.95;
constant uint FIREFLY_SALT = 0x27D4EB2Fu;

// A star's twinkle: its shortest and longest period in seconds.
constant float TWINKLE_PERIOD_MIN = 2.0;
constant float TWINKLE_PERIOD_MAX = 6.0;
constant uint TWINKLE_SALT = 0x165667B1u;

// How far the art is pulled toward the sidebar colour.
constant float MUTE = 0.1;

// The scene, in art pixels: SCENE_ROWS rows from the top of the full scene (the band shows its
// lowest ones), laid out across the band's width, under HIGH_ROWS more of open sky with stars.
constant int SCENE_ROWS = 46;
constant int HIGH_ROWS = 10;
// Where the sidebar's names lie over the scene it is softened into the sidebar's colour: what
// leans away from the text's colour by SOFT_AWAY, and what leans toward it by the least of
// SOFT_STEPS that keeps the dim text SOFT_FLOOR to 1 against it (all the way when none does).
constant float SOFT_AWAY = 0.35;
constant float SOFT_FLOOR = 4.6;
constant float SOFT_STEPS[5] = {0.6, 0.75, 0.85, 0.92, 1.0};
// Once the names reach into the scene, all of it is softened, not only the part under them, so
// no line runs across the castle: fully once they reach SOFT_REACH points in.
constant float SOFT_REACH = 24.0;
// The sky's dither runs over its first SKY_ROWS rows; its stars live in the first STAR_ROWS, one
// in some of the cells of STAR_CELL art pixels, a few of them in the focus colour.
constant int SKY_ROWS = 26;
constant int STAR_ROWS = 12;
constant int2 STAR_CELL = int2(6, 4);
constant float STAR_CHANCE = 0.35;
constant float STAR_FOCUS = 0.3;
constant uint STAR_SALT = 0x2545F491u;
// The moon: its centre, in from the band's right and down from the top, its radius, and the bite
// a second disc takes out of it.
constant int2 MOON_AT = int2(9, 5);
constant float MOON_R2 = 16.0;
constant float2 MOON_BITE = float2(2.0, -1.0);
constant float MOON_BITE_R2 = 13.0;
// By day the sun's rays: one art pixel this far from its middle, up, down, left and right.
constant float SUN_RAY = 6.0;
// The hills: their base row, and how much darker than the sidebar they are.
constant int HILL_BASE = 26;
constant float HILL_STEP = 0.2;
// The castle, from the band's middle column: its body, towers and keep (left, right, top row;
// every part stands on row CASTLE_FOOT), the crenels' rows, the flag's pole (its top and bottom
// rows in the middle column) and pennant (its columns and row), and its lit windows in the order
// the dimming counts them.
constant int CASTLE_FOOT = 25;
constant float CASTLE_STEP = 0.5;
constant int3 CASTLE_PARTS[4] = {int3(-11, 10, 14), int3(-13, -9, 10), int3(8, 12, 10), int3(-3, 3, 6)};
constant int TOWER_CRENELS = 9;
constant int KEEP_CRENELS = 5;
constant int2 POLE = int2(2, 5);
constant int3 PENNANT = int3(1, 2, 2);
constant int2 WINDOWS[5] = {int2(-11, 14), int2(10, 14), int2(0, 10), int2(-6, 19), int2(5, 19)};
// By day, on a light theme, the scene is a desert kingdom of the old tales rather than a castle
// in the hills: a palace in the castle's place, its long hall under a great onion dome with a
// spire, a small dome on each wing and a minaret at each end, its windows WINDOWS_DAY; dunes of
// sand SAND_FAR and SAND_NEAR of the way from the sidebar's colour to the terminal's yellow for
// hills; sand SAND_LIGHT to SAND_DEEP for the ground; a palm for each pine; and a carpet riding
// the air where the bat flew. The art is the effect's own.
constant int2 WINDOWS_DAY[5] = {int2(-13, 14), int2(13, 14), int2(0, 14), int2(-8, 20), int2(8, 20)};
constant float DOME_TONE = 0.5;
constant float SAND_FAR = 0.18;
constant float SAND_NEAR = 0.28;
constant float SAND_LIGHT = 0.3;
constant float SAND_DEEP = 0.42;
// The pines: each one's column (from the left edge when positive, from the right when negative,
// -1 being the last column), its top row and its height. Its trunk is two rows under it.
constant int3 PINES[3] = {int3(3, 20, 11), int3(-4, 18, 13), int3(-11, 25, 8)};
constant float PINE_GREEN = 0.5;
constant float PINE_GREEN_HI = 0.6;
constant float PINE_STEP = 0.4;
constant float PINE_STEP_HI = 0.35;
constant float TRUNK_RED = 0.3;
constant float TRUNK_STEP = 0.45;
// The grass from GRASS_TOP down, dithered from light to dark, with a scatter of bright blades,
// and the path winding down through it from beside the castle's middle.
constant int GRASS_TOP = 32;
constant float GRASS_DARK = 0.5;
constant float GRASS_HI = 0.4;
constant float GRASS_LO = 0.65;
constant float GRASS_BLADES = 0.015;
constant uint GRASS_SALT = 0x61C88647u;
constant int PATH_FROM = -3;
constant float PATH_TONE = 0.35;
// Where each firefly rises, as a share of the band's width, give or take FIREFLY_WANDER columns a
// rise; it climbs FIREFLY_CLIMB rows from FIREFLY_FROM, swaying a column or so.
constant float FIREFLY_HOME[6] = {0.22, 0.6, 0.8, 0.1, 0.42, 0.92};
constant float FIREFLY_WANDER = 6.0;
constant int FIREFLY_FROM = 40;
constant float FIREFLY_CLIMB = 16.0;

// The lantern on an active tab: 3 art pixels wide and 4 tall, LANTERN_IN points in from the tab's
// left so it fits inside the 10 points of padding before the label, centred on the tab's height.
// Its body is the dim text pulled toward the sidebar, warmed by its flame. A tab narrower than
// LANTERN_MIN_W, a dragged slot that has dropped its padding, carries none.
constant float LANTERN_IN = 1.0;
constant float LANTERN_MIN_W = 30.0;
constant float LANTERN_TONE = 0.55;
constant float LANTERN_GLOW = 0.18;

// The title bar's sky, inside its one-line frame: dithered over its SKY_BAR_ROWS art rows from SKY_DEEP
// darker than the bar at the top to the bar's own colour. Its stars: none before
// TITLE_LEFT, the traffic lights' end; one in most of the cells of TITLE_CELL columns, on a row
// from 2 to 6 of the bar, inside the frame, the whole length of the bar. Within STAR_CLEAR of the
// bar's words and icons a star, a cloud, a shooting star and the bird are softened into the bar's
// colour as the scene is under the sidebar's names, so the words keep their contrast.
// The bar's words, its icons and the traffic lights each sit on a plate: the bar's own plain
// colour, with no sky, star or cloud on it, PLATE_PAD points wider than what it holds on each
// side, as tall as the inside of the frame, its corners notched. The lights' plate runs from
// the frame to an art pixel short of the words' plate, and is there only when the words start
// past LIGHTS_END, where the lights are (a full-screen window has none).
constant float PLATE_PAD = 9.0;
constant float LIGHTS_END = 60.0;
constant float TITLE_LEFT = 80.0;
constant int TITLE_CELL = 8;
constant float TITLE_STAR_CHANCE = 0.6;
constant uint TITLE_SALT = 0x9E3779B9u;
constant int SKY_BAR_ROWS = 10;
// The title bar's own frame is a single line, TITLE_FRAME_PX art pixels, so the traffic lights
// have room above and below; the manifest's title_grow makes the bar that much taller too.
constant int TITLE_FRAME_PX = 1;
constant float SKY_DEEP = 0.3;
// The cloud: CLOUD_W art pixels wide and 3 tall from row CLOUD_ROW, crossing the bar right of
// TITLE_LEFT at CLOUD_SPEED art pixels a second from CLOUD_START of the way along, and hidden
// within STAR_CLEAR of a group, as if it passed behind the words.
constant int CLOUD_W = 12;
constant int CLOUD_ROW = 2;
constant float CLOUD_SPEED = 4.0;
constant float CLOUD_START = 0.6;
constant float CLOUD_TONE = 0.5;
// A second, smaller cloud, CLOUD2_W wide and two rows tall from the row under CLOUD_ROW, on the
// first one's course CLOUD2_RATE as fast and CLOUD2_AHEAD seconds along it.
constant int CLOUD2_W = 8;
constant float CLOUD2_RATE = 0.6;
constant float CLOUD2_AHEAD = 150.0;
// A shooting star now and then: one every SHOOT_CYCLE seconds at a moment of its own within the
// cycle (none in the first, so the still frame has none), crossing SHOOT_RUN art pixels down and
// to the left in SHOOT_FOR seconds with SHOOT_TRAIL of them fading behind it.
constant float SHOOT_CYCLE = 7.0;
// The title bar's own come more often.
constant float TITLE_SHOOT_CYCLE = 4.0;
// A bird crosses the title's sky from right to left in BIRD_FOR seconds every BIRD_CYCLE, and a
// light runs the length of the bar's foot, along its frame's outer line, every TITLE_GLINT_CYCLE
// seconds at TITLE_GLINT_SPEED art pixels a second, TITLE_GLINT_LEN of them lit.
constant float BIRD_CYCLE = 9.0;
constant float BIRD_FOR = 6.0;
constant float TITLE_GLINT_CYCLE = 6.0;
constant float TITLE_GLINT_SPEED = 160.0;
constant float TITLE_GLINT_LEN = 12.0;
constant float TITLE_GLINT_LIFT = 0.6;
constant float SHOOT_FOR = 0.6;
constant float SHOOT_RUN = 26.0;
constant float SHOOT_TRAIL = 6.0;
constant uint SHOOT_SALT = 0xC2B2AE35u;
// The life of the scene, each a small loop of its own: the castle's pennant waves every
// PENNANT_WAVE seconds and its windows gutter like candles WINDOW_FLICKERS times a second, by up
// to WINDOW_GUTTER; a cloud drifts across the sky at SKY_CLOUD_SPEED art pixels a second on row
// SKY_CLOUD_ROW; the grass's bright blades lean a column and back every GRASS_SWAY seconds; and
// a bat crosses the sky in BAT_FOR seconds every BAT_CYCLE, flapping BAT_FLAPS times a second.
// A lantern's flame gutters LANTERN_FLICKERS times a second, and a spark crosses the focused
// pane's gem every GEM_CYCLE seconds, taking GEM_SPARK.
constant float PENNANT_WAVE = 0.6;
constant float WINDOW_FLICKERS = 4.0;
constant float WINDOW_GUTTER = 0.3;
constant float SKY_CLOUD_SPEED = 1.5;
constant int SKY_CLOUD_ROW = 13;
constant float GRASS_SWAY = 1.1;
constant float BAT_CYCLE = 13.0;
constant float BAT_FOR = 5.0;
constant float BAT_FLAPS = 6.0;
constant int BAT_ROW = 3;
constant uint BAT_SALT = 0x94D049BBu;
constant float LANTERN_FLICKERS = 5.0;
constant float GEM_CYCLE = 3.0;
constant float GEM_SPARK = 0.4;
// A table's header behind a document's paper: on a dark theme the background HEADER_TONE away
// from the text's colour; on a light one, where that would be white on near white, HEADER_LIGHT
// toward the text's instead, little enough that the header's own text keeps its contrast.
constant float HEADER_TONE = 0.22;
constant float HEADER_LIGHT = 0.07;

// The ground between the panes: a checker of two night tones, each this much darker than the
// sidebar.
constant float GROUND_A = 0.3;
constant float GROUND_B = 0.42;
// A frame, FRAME_PX art pixels wide: the outer line the border colour lifted FRAME_LIFT toward
// the dim text, the inner one FRAME_DARK darker than the sidebar.
constant float FRAME_LIFT = 0.35;
constant float FRAME_DARK = 0.55;
// A working pane's glint: GLINT_LEN art pixels of the outer line lifted up to GLINT_LIFT toward
// white, fading along its length, going round at GLINT_SPEED art pixels a second.
constant float GLINT_LEN = 6.0;
constant float GLINT_LIFT = 0.55;
constant float GLINT_SPEED = 12.0;
// A waiting pane's "continue" arrow, behind its terminal's paper: ARROW_W art pixels wide at its
// top and three rows tall, ARROW_IN in from the body's right and its foot, in the terminal's
// yellow, bobbing one art pixel down and back every ARROW_BOB seconds (extra-long2 each way).
constant int ARROW_W = 5;
constant int ARROW_IN = 4;
constant float ARROW_BOB = 0.8;

// The ledge: the banner colour, its top row lifted LEDGE_HI toward the dim text, its last row the
// frame's dark, and joints LEDGE_JOINT of the way to the sidebar's colour: one along row
// LEDGE_COURSE and an upright every LEDGE_BRICK columns, the lower course half a brick along.
// Near its right end, GEM_IN art pixels in (clear of the bar's + button), a gem four art pixels
// square from row GEM_ROW: dull stone-violet, the focus colour on the pane that has the focus.
// A bar narrower than GEM_MIN_W has none.
constant float LEDGE_HI = 0.25;
constant float LEDGE_JOINT = 0.6;
constant int LEDGE_COURSE = 4;
constant int LEDGE_BRICK = 10;
constant float LEDGE_HOVER = 0.1;
constant int GEM_IN = 11;
constant int GEM_ROW = 3;
constant float GEM_MIN_W = 120.0;
constant float GEM_DULL = 0.35;

// The menu cursor on the selected row: 3 art pixels wide and 5 tall, just inside the sidebar's
// frame, in the padding the manifest's sidebar_pad adds before every row's text. It bobs one art
// pixel toward the row and back every CURSOR_BOB seconds (long2 each way).
constant float CURSOR_BOB = 0.5;

// The night behind a terminal. PAPER is the manifest's paper_alpha: the wash draws each tone
// 1 / (1 - PAPER) times as far from the background as it should reach the screen. The dusk is the
// background DUSK_TONE away from the text over the first DUSK_ROWS rows, dithered out; the stars
// are one in some of the cells of NIGHT_CELL art pixels in the upper half, STAR_LIMIT toward the
// dim text at their brightest; the far hills are HILL_FAR and the near ones HILL_NEAR away from
// the text.
constant float PAPER = 0.6;
constant int DUSK_ROWS = 34;
constant float DUSK_TONE = 0.14;
constant int2 NIGHT_CELL = int2(14, 10);
constant float NIGHT_STAR_CHANCE = 0.3;
constant float STAR_LIMIT = 0.25;
constant uint NIGHT_SALT = 0x7F4A7C15u;
constant float HILL_FAR = 0.1;
constant float HILL_NEAR = 0.18;
constant float DUNE_FAR = 0.02;
constant float DUNE_NEAR = 0.035;

// A highlight's block is its row or tab inset by this much on each side (`blocks::stroke_block`);
// the fill covers the whole row or tab, as the app's does.
constant float2 STROKE_INSET = float2(3.0, 2.0);

// A hash of n in 0 to 1, in integer arithmetic (PCG's output step), the same as the app's
// `effects::hash::hash01` to the bit.
float hash01(uint n) {
    uint h = n * 747796405u + 2891336453u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    h ^= h >> 22u;
    return float(h >> 8u) * (1.0 / 16777216.0);
}

bool inside_rect(float2 p, float4 r) {
    return p.x >= r.x && p.x <= r.x + r.z && p.y >= r.y && p.y <= r.y + r.w;
}

// The band of the sidebar `side` the scene is drawn in: inside its frame, at most SCENE_MAX_H tall
// in whole art pixels, standing on the frame's foot; zero height in a sidebar with under
// SCENE_MIN_H of room inside its frame.
float4 scene_band(float4 side) {
    float in = float(FRAME_PX) * ART_PX;
    float room = side.w - 2.0 * in;
    if (room < SCENE_MIN_H || side.z <= 2.0 * in) {
        return float4(0.0);
    }
    float h = floor(min(room, SCENE_MAX_H) / ART_PX) * ART_PX;
    return float4(side.x + in, side.y + side.w - in - h, side.z - 2.0 * in, h);
}

// Whether p is on the plate pl: inside it and not in one of its four corner art pixels.
bool on_plate(float2 p, float4 pl) {
    if (pl.z <= 0.0 || !inside_rect(p, pl)) {
        return false;
    }
    float2 last = ceil(pl.zw / ART_PX) - 1.0;
    float2 q = floor((p - pl.xy) / ART_PX);
    return !((q.x == 0.0 || q.x >= last.x) && (q.y == 0.0 || q.y >= last.y));
}

// Whether p is on one of the title bar's plates: behind a group of its words or icons, or behind
// the traffic lights.
bool plated(float2 p, float4 bar, constant ArkosInputs& u, int nb) {
    float in = float(TITLE_FRAME_PX) * ART_PX;
    float first = 1e5;
    for (int i = 0; i < nb; i++) {
        if (int(u.blocks[i].kind) != ARKOS_TITLE_GROUP_BLOCK) {
            continue;
        }
        float4 g = u.blocks[i].rect;
        first = min(first, g.x);
        if (on_plate(p, float4(g.x - PLATE_PAD, bar.y + in, g.z + 2.0 * PLATE_PAD, bar.w - 2.0 * in))) {
            return true;
        }
    }
    float to = first - PLATE_PAD - ART_PX;
    return first < 1e4 && first > LIGHTS_END && on_plate(p, float4(bar.x + in, bar.y + in, to - bar.x - in, bar.w - 2.0 * in));
}

// Whether the art pixel whose top left is o keeps clear of the title bar's words and icons: it
// touches no title group grown by STAR_CLEAR. False in a bar with no groups laid out yet.
bool cell_clear(float2 o, constant ArkosInputs& u, int nb) {
    bool grouped = false;
    for (int i = 0; i < nb; i++) {
        if (int(u.blocks[i].kind) != ARKOS_TITLE_GROUP_BLOCK) {
            continue;
        }
        grouped = true;
        float4 r = u.blocks[i].rect;
        if (o.x + ART_PX >= r.x - STAR_CLEAR && o.x <= r.x + r.z + STAR_CLEAR && o.y + ART_PX >= r.y - STAR_CLEAR && o.y <= r.y + r.w + STAR_CLEAR) {
            return false;
        }
    }
    return grouped;
}

float dim_start(uint k) {
    return float(k) * DIM_CYCLE + (hash01(k ^ DIM_SALT) - 0.5) * DIM_SLACK;
}

// The castle window dimmed at t (-1 for none) in .x, and how deep, 0 to 1, in .y.
float2 dimmed_window(float t) {
    float k = floor((t + 0.5 * DIM_SLACK) / DIM_CYCLE);
    if (k < 1.0) {
        return float2(-1.0, 0.0);
    }
    uint n = uint(k);
    float since = t - dim_start(n);
    if (since < 0.0 || since >= DIM_FOR) {
        return float2(-1.0, 0.0);
    }
    float w = min(floor(hash01((n ^ DIM_SALT) + 1u) * float(CASTLE_WINDOWS)), float(CASTLE_WINDOWS - 1));
    return float2(w, sin(3.14159265 * since / DIM_FOR));
}

// Firefly i at t: how far through its rise, how lit, and which rise it is on.
float3 firefly(int i, float t) {
    uint id = uint(i) ^ FIREFLY_SALT;
    float period = FIREFLY_PERIOD_MIN + (FIREFLY_PERIOD_MAX - FIREFLY_PERIOD_MIN) * hash01(id);
    float phase = FIREFLY_FADE_IN + (FIREFLY_FADE_FROM - FIREFLY_FADE_IN) * (float(i) + 0.5) / float(FIREFLIES);
    float x = t / period + phase;
    float rise = x - floor(x);
    float alpha = 0.0;
    if (rise < FIREFLY_FADE_IN) {
        alpha = rise / FIREFLY_FADE_IN;
    } else if (rise < FIREFLY_FADE_FROM) {
        alpha = 1.0;
    } else if (rise < FIREFLY_FADE_TO) {
        alpha = 1.0 - (rise - FIREFLY_FADE_FROM) / (FIREFLY_FADE_TO - FIREFLY_FADE_FROM);
    }
    return float3(rise, alpha, floor(x));
}

// A star's brightness at t, 0.2 to 1, full at the clock's zero.
float twinkle(float seed, float t) {
    float period = TWINKLE_PERIOD_MIN + (TWINKLE_PERIOD_MAX - TWINKLE_PERIOD_MIN) * hash01(uint(seed) ^ TWINKLE_SALT);
    return 0.6 + 0.4 * cos(6.2831853 * t / period);
}

// How lit a shooting star is at art pixel (i, j) of a sky w wide whose stars fall through its
// first `rows` rows, one row down for every `slope` columns across: 1 at its head, fading along
// its trail, 0 elsewhere and whenever none is falling. One falls every `cycle` seconds.
float shooting(int i, int j, int w, int rows, float slope, float cycle, float t, uint salt) {
    float k = floor(t / cycle);
    if (k < 1.0) {
        return 0.0;
    }
    uint id = uint(k) ^ salt;
    float since = t - k * cycle - hash01(id) * (cycle - SHOOT_FOR);
    if (since < 0.0 || since >= SHOOT_FOR) {
        return 0.0;
    }
    float along = since / SHOOT_FOR * SHOOT_RUN;
    float2 head = float2(float(w) * (0.35 + 0.65 * hash01(id + 1u)) - along, float(rows) * 0.4 * hash01(id + 2u) + along / slope);
    float behind = float(i) - floor(head.x);
    if (behind < 0.0 || behind >= SHOOT_TRAIL || j != int(floor(head.y - behind / slope)) || j >= rows) {
        return 0.0;
    }
    return 1.0 - behind / SHOOT_TRAIL;
}

// The art's colours, from the theme.
float3 mute(constant ArkosInputs& u, float3 c) {
    return mix(c, u.colors[ARKOS_SIDEBAR].rgb, MUTE);
}

// A step darker than c: toward black on a dark theme, toward the theme's own ink on a light one,
// so a light theme's silhouettes keep its hue, a dusk on a pale sky.
float3 darker(constant ArkosInputs& u, float3 c, float by) {
    float3 toward = u.dark > 0.5 ? float3(0.0) : u.colors[ARKOS_TEXT].rgb;
    return mix(c, toward, by);
}

// Whether the art pixel at column d from the castle's middle and row j is the castle's stone.
bool castle_at(int d, int j) {
    if (j > CASTLE_FOOT) {
        return false;
    }
    for (int n = 0; n < 4; n++) {
        int3 part = CASTLE_PARTS[n];
        if (d >= part.x && d <= part.y && j >= part.z) {
            return true;
        }
    }
    // A crenel on every other column of each tower and of the keep, from its left.
    for (int n = 1; n < 4; n++) {
        int3 part = CASTLE_PARTS[n];
        if (j == (n == 3 ? KEEP_CRENELS : TOWER_CRENELS) && d >= part.x && d <= part.y && (d - part.x) % 2 == 0) {
            return true;
        }
    }
    return d == 0 && j >= POLE.x && j <= POLE.y;
}

// What the palace is at column d from its middle and row j: 0 nothing, 1 its stone, 2 a dome.
int palace_at(int d, int j) {
    if (j > CASTLE_FOOT) {
        return 0;
    }
    int a = abs(d);
    // The great dome over the middle hall: an onion, widest above its neck, and its spire.
    int bulge = j == 10 ? 3 : (j == 9 || j == 8) ? 5 : j == 7 ? 4 : j == 6 ? 3 : j == 5 ? 2 : j == 4 ? 1 : (j == 3 || j == 2) ? 0 : -1;
    if (j >= 2 && j <= 10 && a <= bulge) {
        return 2;
    }
    // A small dome on each wing, eight columns out.
    int wing = abs(a - 8);
    if ((j == 16 && wing <= 2) || (j == 15 && wing <= 2) || (j == 14 && wing <= 1) || (j == 13 && wing == 0)) {
        return 2;
    }
    // A minaret at each end: a thin shaft with a balcony, under a small dome and a spire.
    int end = abs(a - 13);
    if ((end <= 1 && j == 8) || (end == 0 && (j == 6 || j == 7))) {
        return 2;
    }
    if ((end <= 1 && j >= 9) || (end <= 2 && j == 12)) {
        return 1;
    }
    // The long hall and the taller middle one.
    return ((a <= 11 && j >= 17) || (a <= 4 && j >= 11)) ? 1 : 0;
}

// The scene's colour at art pixel (i, j): column i of w across the band, row j of the full scene.
float3 scene(constant ArkosInputs& u, int i, int j, int w, float t) {
    float3 sb = u.colors[ARKOS_SIDEBAR].rgb;
    float3 text = u.colors[ARKOS_TEXT].rgb;
    float3 green = u.colors[ARKOS_ANSI + 2].rgb;
    bool even = ((i + j) & 1) == 0;
    float3 col = sb;
    // The sky: plain at the top, dithered toward the banner colour lower down, then solid.
    if (j < SKY_ROWS && float(j) / float(SKY_ROWS) > (even ? 0.25 : 0.75)) {
        col = mute(u, u.colors[ARKOS_BANNER].rgb);
    }
    // The stars, each in a cell of its own.
    if (j < STAR_ROWS) {
        int2 cell = int2(i / STAR_CELL.x, j / STAR_CELL.y);
        uint id = (uint(cell.y) * 64u + uint(cell.x)) ^ STAR_SALT;
        int sx = cell.x * STAR_CELL.x + int(hash01(id + 1u) * float(STAR_CELL.x));
        int sy = cell.y * STAR_CELL.y + int(hash01(id + 2u) * float(STAR_CELL.y));
        if (i == sx && j == sy && hash01(id) < STAR_CHANCE) {
            float3 star = hash01(id + 3u) < STAR_FOCUS ? u.colors[ARKOS_FOCUS_RING].rgb : text;
            col = mix(col, mute(u, star), twinkle(float(cell.y * 64 + cell.x), t));
        }
    }
    // A cloud drifting across the sky, behind the moon and the castle.
    int span = w + CLOUD_W;
    int sc = i + CLOUD_W - int(fmod(0.3 * float(span) + t * SKY_CLOUD_SPEED, float(span)));
    int sr = j - SKY_CLOUD_ROW;
    if (sr >= 0 && sr < 3 && sc >= (sr == 0 ? 3 : 0) && sc < (sr == 0 ? CLOUD_W - 3 : CLOUD_W)) {
        col = mix(col, mute(u, u.colors[ARKOS_BANNER].rgb), 0.7);
    }
    // The crescent moon; by day, on a light theme, the sun in its place: a whole disc in the
    // focus colour, with four rays that come and go with the stars.
    float2 m = float2(float(i - (w - MOON_AT.x)), float(j - MOON_AT.y));
    if (u.dark > 0.5) {
        if (dot(m, m) <= MOON_R2 && dot(m - MOON_BITE, m - MOON_BITE) > MOON_BITE_R2) {
            col = mute(u, text);
        }
    } else if (dot(m, m) <= MOON_R2) {
        col = mute(u, u.colors[ARKOS_FOCUS_RING].rgb);
    } else if ((abs(m.x) == SUN_RAY && m.y == 0.0) || (abs(m.y) == SUN_RAY && m.x == 0.0)) {
        col = mix(col, mute(u, u.colors[ARKOS_FOCUS_RING].rgb), twinkle(77.0, t));
    }
    // The hills, the full width; by day two lines of long dunes.
    bool day = u.dark < 0.5;
    float3 sand = u.colors[ARKOS_ANSI + 3].rgb;
    float fi = float(i);
    if (day) {
        if (j >= HILL_BASE - int(rint(4.0 + 3.0 * sin(fi / 11.0 + 1.0)))) {
            col = mute(u, mix(sb, sand, SAND_FAR));
        }
        if (j >= HILL_BASE + 3 - int(rint(2.0 + 2.0 * sin(fi / 8.0 + 3.0)))) {
            col = mute(u, mix(sb, sand, SAND_NEAR));
        }
    } else if (j >= HILL_BASE - int(rint(5.0 + 2.0 * sin(fi / 7.0) + 1.5 * sin(fi / 3.0)))) {
        col = mute(u, darker(u, sb, HILL_STEP));
    }
    // The castle, centred, its windows lit in the focus colour and one now and then dimming. Its
    // walls span columns -13 to 12 from `mid`, so on an even band they sit on its very middle.
    int mid = w / 2;
    int d = i - mid;
    int built = day ? palace_at(d, j) : (castle_at(d, j) ? 1 : 0);
    if (built > 0) {
        float3 stone = mute(u, darker(u, sb, CASTLE_STEP));
        // A dome is the stone warmed with the focus colour.
        col = built == 2 ? mix(stone, mute(u, u.colors[ARKOS_FOCUS_RING].rgb), DOME_TONE) : stone;
        float2 dim = dimmed_window(t);
        for (int n = 0; n < CASTLE_WINDOWS; n++) {
            int2 at = day ? WINDOWS_DAY[n] : WINDOWS[n];
            if (d == at.x && j == at.y) {
                float depth = float(n) == dim.x ? dim.y * DIM_DEPTH : 0.0;
                // Candlelight: each window gutters a little on its own, steady in the first beat.
                float beat = floor(t * WINDOW_FLICKERS);
                float gutter = beat < 1.0 ? 0.0 : WINDOW_GUTTER * hash01(uint(beat) * 8u + uint(n) + DIM_SALT);
                col = mix(u.colors[ARKOS_FOCUS_RING].rgb, stone, max(depth, gutter));
            }
        }
        // The palace's gate: an arch in its middle hall.
        if (day && ((abs(d) <= 1 && j >= 22) || (d == 0 && j == 21))) {
            col = mute(u, darker(u, sb, CASTLE_STEP + 0.2));
        }
    }
    // The castle's pennant, its tip dipping a row and back as it waves.
    bool dipped = fmod(floor(t / PENNANT_WAVE), 2.0) > 0.5;
    if (!day && d >= PENNANT.x && d <= PENNANT.y && j == PENNANT.z + ((dipped && d == PENNANT.y) ? 1 : 0)) {
        col = mute(u, u.colors[ARKOS_ANSI + 5].rgb);
    }
    // The pines, near both edges, with a light row every third.
    for (int n = 0; n < 3; n++) {
        int3 pine = PINES[n];
        int a = pine.x >= 0 ? pine.x : w + pine.x;
        int row = j - pine.y;
        if (day) {
            // A palm: three rows of fronds spread from the top of a thin trunk.
            int dx = abs(i - a);
            bool frond = (row == 0 && (dx == 1 || dx == 2)) || (row == 1 && (dx == 0 || dx == 2 || dx == 3)) || (row == 2 && (dx == 0 || dx == 3));
            if (frond) {
                col = mute(u, darker(u, mix(sb, green, PINE_GREEN_HI), PINE_STEP_HI));
            } else if (dx == 0 && row >= 3 && row < pine.z + 2) {
                col = mute(u, darker(u, mix(sb, u.colors[ARKOS_ANSI + 1].rgb, TRUNK_RED), TRUNK_STEP));
            }
            continue;
        }
        if (row >= 0 && row < pine.z && abs(i - a) <= row / 2) {
            bool hi = row % 3 == 0;
            float3 needles = darker(u, mix(sb, green, hi ? PINE_GREEN_HI : PINE_GREEN), hi ? PINE_STEP_HI : PINE_STEP);
            col = mute(u, needles);
        } else if (i == a && row >= pine.z && row < pine.z + 2) {
            col = mute(u, darker(u, mix(sb, u.colors[ARKOS_ANSI + 1].rgb, TRUNK_RED), TRUNK_STEP));
        }
    }
    // The grass, light at its top and dark below, a few bright blades, and the path.
    if (j >= GRASS_TOP) {
        float3 ground = darker(u, sb, GRASS_DARK);
        float deep = float(j - GRASS_TOP) / float(SCENE_ROWS - GRASS_TOP) * 1.4;
        col = mute(u, mix(green, ground, deep > (even ? 0.25 : 0.75) ? GRASS_LO : GRASS_HI));
        if (day) {
            col = mute(u, mix(sb, sand, deep > (even ? 0.25 : 0.75) ? SAND_DEEP : SAND_LIGHT));
        }
        // The bright blades lean a column and back, each row in its own time.
        int lean = int(fmod(floor(t / GRASS_SWAY + hash01(uint(j) ^ GRASS_SALT)), 2.0));
        if (hash01((uint(j) * 1024u + uint(i - lean)) ^ GRASS_SALT) < GRASS_BLADES) {
            // By day a tuft of dry grass in the sand.
            col = mute(u, day ? mix(sand, green, 0.4) : green);
        }
        int from = mid + PATH_FROM + int(rint(3.0 * sin(float(j) / 5.0)));
        if (i >= from && i < from + 4 + (j - GRASS_TOP) / 5) {
            col = mute(u, mix(sb, u.colors[ARKOS_TEXT_DIM].rgb, PATH_TONE));
        }
    }
    // A bat crossing the sky now and then, its wings up and down: none in the first cycle.
    float bk = floor(t / BAT_CYCLE);
    float bs = t - bk * BAT_CYCLE;
    if (bk >= 1.0 && bs < BAT_FOR) {
        float across = bs / BAT_FOR;
        int ax = i - (int(across * float(w + 6)) - 3);
        int ay = j - (BAT_ROW + int(hash01(uint(bk) ^ BAT_SALT) * 8.0) + int(rint(2.0 * sin(across * 11.0))));
        bool up = fmod(floor(t * BAT_FLAPS), 2.0) < 0.5;
        if (day) {
            // A carpet riding the air, five art pixels long, its ends rippling in turn.
            if ((abs(ax) <= 1 && ay == 0) || (ax == -2 && ay == (up ? 0 : 1)) || (ax == 2 && ay == (up ? 1 : 0))) {
                col = mute(u, u.colors[ARKOS_ANSI + 5].rgb);
            }
        } else if (ax >= -1 && ax <= 1 && ay >= 0 && ay <= 1 && ((ax != 0) == (up ? ay == 0 : ay == 1))) {
            col = mute(u, darker(u, sb, CASTLE_STEP));
        }
    }
    // The fireflies, rising from the grass and fading before the top of their climb.
    for (int n = 0; n < FIREFLIES; n++) {
        float3 f = firefly(n, t);
        if (f.y <= 0.0) {
            continue;
        }
        float wander = (hash01((uint(f.z) * 4u + uint(n)) ^ FIREFLY_SALT) - 0.5) * FIREFLY_WANDER;
        int fx = int(floor(FIREFLY_HOME[n] * float(w) + wander)) + int(rint(sin(6.2831853 * 1.5 * f.x + float(n))));
        int fy = FIREFLY_FROM - int(f.x * FIREFLY_CLIMB);
        if (i == fx && j == fy) {
            col = mix(col, u.colors[ARKOS_ANSI + 3].rgb, f.y);
        }
    }
    return col;
}

// The lantern's colour at art pixel q of its 3 by 4, over the tab's colour `tab`, at t: its flame
// gutters, brighter and dimmer by turns with its glow on the glass, steady in the first beat.
float3 lantern(constant ArkosInputs& u, int2 q, float3 tab, float t, float seed) {
    float3 flame = u.colors[ARKOS_ANSI + 3].rgb;
    float beat = floor(t * LANTERN_FLICKERS);
    float lit = beat < 1.0 ? 0.5 : hash01(uint(beat) * 16u + uint(seed));
    if (q.x == 1 && q.y == 2) {
        return mix(flame, float3(1.0), 0.5 * (lit - 0.5));
    }
    if ((q.y == 0 && q.x == 1) || (q.y >= 1 && q.y <= 3 && q.x >= 0 && q.x <= 2)) {
        float3 body = mute(u, mix(u.colors[ARKOS_SIDEBAR].rgb, u.colors[ARKOS_TEXT_DIM].rgb, LANTERN_TONE));
        return q.y == 0 ? body : mix(body, flame, LANTERN_GLOW * (0.5 + lit));
    }
    return tab;
}

// c moved `by` away from the text's colour: toward black on a dark theme, toward white on a light
// one, so whatever is drawn in it behind text raises the text's contrast.
float3 away(constant ArkosInputs& u, float3 c, float by) {
    return mix(c, u.dark > 0.5 ? float3(0.0) : float3(1.0), by);
}

// The frames' outer line.
float3 frame_line(constant ArkosInputs& u) {
    return mix(u.colors[ARKOS_BORDER].rgb, u.colors[ARKOS_TEXT_DIM].rgb, FRAME_LIFT);
}

// The frames' inner line, the darkest night tone.
float3 frame_dark(constant ArkosInputs& u) {
    return darker(u, u.colors[ARKOS_SIDEBAR].rgb, FRAME_DARK);
}

// The ground at window point p.
float3 ground(constant ArkosInputs& u, float2 p) {
    int2 q = int2(floor(p / ART_PX));
    return darker(u, u.colors[ARKOS_SIDEBAR].rgb, ((q.x + q.y) & 1) == 0 ? GROUND_A : GROUND_B);
}

// Which ring of the frame round pane rect r the point p is in, each ART_PX wide, in .x: 0 inside
// the pane, 1 the inner line, 2 the outer line, 3 and up beyond the frame; and in .y whether p is
// on a corner of its ring (both its axes are that far out).
int2 frame_ring(float2 p, float4 r) {
    float2 out = max(max(r.xy - p, p - (r.xy + r.zw)), float2(0.0));
    int2 a = int2(ceil(out / ART_PX));
    int ring = max(a.x, a.y);
    return int2(ring, (a.x == ring && a.y == ring) ? 1 : 0);
}

// How far along the outer line of the frame round r the point p is, in art pixels from its top
// left corner, clockwise; and the whole way round in .y.
float2 frame_run(float2 p, float4 r) {
    float grow = (float(FRAME_PX) - 0.5) * ART_PX;
    float4 o = float4(r.xy - grow, r.zw + 2.0 * grow);
    float w = floor(o.z / ART_PX);
    float h = floor(o.w / ART_PX);
    float2 d = floor((p - o.xy) / ART_PX + 0.5);
    float s;
    if (p.y < r.y && p.x < r.x + r.z) {
        s = clamp(d.x, 0.0, w);
    } else if (p.x >= r.x + r.z && p.y < r.y + r.w) {
        s = w + clamp(d.y, 0.0, h);
    } else if (p.y >= r.y + r.w) {
        s = w + h + clamp(w - d.x, 0.0, w);
    } else {
        s = 2.0 * w + h + clamp(h - d.y, 0.0, h);
    }
    return float2(s, 2.0 * (w + h));
}

// How lit the glint is at `at` art pixels along a frame `around` art pixels round, at t: 1 at its
// head, fading to 0 GLINT_LEN behind it.
float glint(float at, float around, float t) {
    float head = fmod(t * GLINT_SPEED, around);
    float behind = fmod(head - at + around, around);
    return behind < GLINT_LEN ? 1.0 - behind / GLINT_LEN : 0.0;
}

// The cloud's left edge at t, in art pixels from the bar's left, for a bar w art pixels wide.
int cloud_at(int w, float t) {
    int from = int(ceil(TITLE_LEFT / ART_PX));
    float span = float(max(w - from + CLOUD_W, 1));
    return from - CLOUD_W + int(fmod(CLOUD_START * span + t * CLOUD_SPEED, span));
}

// Which ring of the frame inside rect r the point p is in, each ART_PX wide, in .x: 0 the outer
// line along r's edge, 1 the inner line, 2 and up inside the frame; and in .y whether p is on a
// corner of its ring.
int2 inner_ring(float2 p, float4 r) {
    float2 in = min(p - r.xy, r.xy + r.zw - p);
    int2 a = int2(floor(in / ART_PX));
    int ring = min(a.x, a.y);
    return int2(ring, (a.x == ring && a.y == ring) ? 1 : 0);
}

// The frame inside r at p over `under`: the outer line with its corners left to the ground, the
// inner one with its corners in the outer line's colour. `under` where p is inside the frame.
float3 boxed(constant ArkosInputs& u, float2 p, float4 r, float3 under) {
    int2 ring = inner_ring(p, r);
    if (ring.x >= FRAME_PX || ring.x < 0) {
        return under;
    }
    if (ring.x == 0) {
        return ring.y == 1 ? ground(u, p) : frame_line(u);
    }
    return ring.y == 1 ? frame_line(u) : frame_dark(u);
}

// A colour's relative luminance, as WCAG has it.
float luma(float3 c) {
    float3 lin = mix(c / 12.92, pow((c + 0.055) / 1.055, float3(2.4)), step(float3(0.04045), c));
    return dot(lin, float3(0.2126, 0.7152, 0.0722));
}

// The colour c of a piece of art where words lie over it on a surface of colour sb: softened
// into that colour, far enough that the dim text keeps its contrast (see SOFT_AWAY).
float3 soften(constant ArkosInputs& u, float3 c, float3 sb) {
    float text = luma(u.colors[ARKOS_TEXT_DIM].rgb);
    float base = luma(sb);
    float own = luma(c);
    if ((own - base) * (text - base) <= 0.0) {
        return mix(c, sb, SOFT_AWAY);
    }
    for (int n = 0; n < 5; n++) {
        float3 soft = mix(c, sb, SOFT_STEPS[n]);
        float l = luma(soft);
        if ((max(text, l) + 0.05) / (min(text, l) + 0.05) >= SOFT_FLOOR) {
            return soft;
        }
    }
    return sb;
}

// The open sky above the scene at art pixel (i, j) of its HIGH_ROWS: the sidebar's colour and a
// scatter of stars.
float3 high_sky(constant ArkosInputs& u, int i, int j, float t) {
    float3 col = u.colors[ARKOS_SIDEBAR].rgb;
    int2 cell = int2(i / STAR_CELL.x, j / STAR_CELL.y);
    uint id = (uint(cell.y) * 64u + uint(cell.x) + 4096u) ^ STAR_SALT;
    int sx = cell.x * STAR_CELL.x + int(hash01(id + 1u) * float(STAR_CELL.x));
    int sy = cell.y * STAR_CELL.y + int(hash01(id + 2u) * float(STAR_CELL.y));
    if (i == sx && j == sy && hash01(id) < STAR_CHANCE) {
        float3 star = hash01(id + 3u) < STAR_FOCUS ? u.colors[ARKOS_FOCUS_RING].rgb : u.colors[ARKOS_TEXT].rgb;
        col = mix(col, mute(u, star), twinkle(float(4096 + cell.y * 64 + cell.x), t));
    }
    return col;
}

// A tone that should reach the screen as `want` behind a pane's paper over `bg`: drawn
// 1 / (1 - PAPER) times as far from the background, since the paper covers the rest. Every tab
// has a paper at PAPER, a terminal's and a document's alike; an empty pane has none, and its
// night is that much stronger.
float3 through(float3 bg, float3 want) {
    return saturate(bg + (want - bg) / (1.0 - PAPER));
}

// The night behind a pane's body, as it should reach the screen, `at` points from the top left of
// what it is drawn on and `to_foot` points above its foot: the pane's body for a terminal, and a
// document's whole page, so its dusk is at the page's top, its hills at the page's end, and the
// plain night between scrolls by with the text. The stars keep to the first `star_rows` rows.
float3 night(constant ArkosInputs& u, float2 at, float to_foot, int star_rows, float seed, float t) {
    float3 bg = u.colors[ARKOS_TERMINAL_BG].rgb;
    float3 col = bg;
    int i = int(floor(at.x / ART_PX));
    int j = int(floor(at.y / ART_PX));
    // A two by two ordered dither, so the dusk thins out in four steps instead of ending in a band.
    float sieve = (float(((i & 1) << 1 | (j & 1)) ^ ((i & 1) * 3)) + 0.5) / 4.0;
    if (j < DUSK_ROWS && 1.0 - float(j) / float(DUSK_ROWS) > sieve) {
        col = away(u, bg, DUSK_TONE);
    }
    if (j < star_rows) {
        int2 cell = int2(i / NIGHT_CELL.x, j / NIGHT_CELL.y);
        uint id = (uint(cell.y) * 256u + uint(cell.x) + uint(seed) * 7919u) ^ NIGHT_SALT;
        int sx = cell.x * NIGHT_CELL.x + int(hash01(id + 1u) * float(NIGHT_CELL.x));
        int sy = cell.y * NIGHT_CELL.y + int(hash01(id + 2u) * float(NIGHT_CELL.y));
        if (i == sx && j == sy && hash01(id) < NIGHT_STAR_CHANCE) {
            col = mix(col, mix(bg, u.colors[ARKOS_TEXT_DIM].rgb, STAR_LIMIT), twinkle(float(id & 1023u), t));
        }
    }
    // The hills stand at most eleven rows tall, so only the foot works them out.
    int up = int(floor(to_foot / ART_PX));
    if (up < 12) {
        float fi = float(i) + seed * 3.0;
        // By night they are away from the text's colour; by day, where that would be white on
        // near white, dunes a touch toward it, little enough to cost the text almost nothing.
        bool day = u.dark < 0.5;
        float3 ink = u.colors[ARKOS_TEXT].rgb;
        if (up < int(rint(6.0 + 2.5 * sin(fi / 9.0) + 1.5 * sin(fi / 4.0)))) {
            col = day ? mix(bg, ink, DUNE_FAR) : away(u, bg, HILL_FAR);
        }
        if (up < int(rint(3.0 + 1.5 * sin(fi / 5.0 + 2.0)))) {
            col = day ? mix(bg, ink, DUNE_NEAR) : away(u, bg, HILL_NEAR);
        }
    }
    return col;
}

fragment float4 arkos_wash(ArkosVertex in [[stage_in]], constant ArkosInputs& u [[buffer(0)]]) {
    float2 p = in.uv * u.size;
    float k = saturate(u.intensity);
    float t = u.time;
    float3 plain = u.colors[ARKOS_WINDOW_BG].rgb;
    float3 col = mix(plain, ground(u, p), k);
    int nb = min(int(u.block_count), 48);
    bool on_active = false;
    // The focus of the pane whose tab bar p is in, for its active tab's line.
    float bar_focus = 0.0;
    // The pane body p is in, if any: its night is drawn once, after the loop has seen whether a
    // table's header lies over p.
    bool in_body = false;
    bool on_header = false;
    float4 body = float4(0.0);
    // The document's page behind that body, when it shows one: the body itself otherwise.
    float4 page = float4(0.0);
    bool paged = false;
    float body_seed = 0.0;
    bool body_waits = false;
    // The sidebar's rect as shown (it narrows as Cmd+B slides it out), zero width when none, and
    // where its names end: its foot block's top, or its own foot when the names run to it.
    float4 side = float4(0.0);
    float names_end = 0.0;
    for (int i = 0; i < min(nb, 6); i++) {
        int kind = int(u.blocks[i].kind);
        if (kind == ARKOS_SIDEBAR_BLOCK) {
            side = u.blocks[i].rect;
            names_end = side.y + side.w;
        } else if (kind == ARKOS_SIDEBAR_FOOT_BLOCK) {
            names_end = u.blocks[i].rect.y;
        }
    }
    // The blocks come in the order they are painted: the title bar, the sidebar and its foot, the
    // panes' tab bars and bodies, the shapes (each active tab among them), then the highlights.
    for (int i = 0; i < nb; i++) {
        int kind = int(u.blocks[i].kind);
        float4 r = u.blocks[i].rect;
        if (kind == ARKOS_ROW_HIGHLIGHT_BLOCK || kind == ARKOS_TAB_HIGHLIGHT_BLOCK) {
            // A selected row is filled in the active tab's colour and a hovered row in the
            // inactive tab's, as the app fills them (its hover wins over its selection); a hovered
            // tab is the ledge's stone a little lighter, and a hovered active tab stays as it is.
            // A row in the sidebar's column is cut at the sidebar's shown edge, since its mark
            // keeps the full column while the sidebar slides.
            float4 full = float4(r.xy - STROKE_INSET, r.zw + 2.0 * STROKE_INSET);
            bool in_side = side.z > 0.0 && full.x < side.x + side.z;
            float v = saturate(u.blocks[i].value);
            int state = int(u.blocks[i].ordinal);
            // The cursor on a selected row of the sidebar: a triangle pointing at the row from
            // just inside the sidebar's frame. A row's mark starts where its text does, past its
            // indent, so the cursor is placed from the sidebar, level with the mark.
            if (kind == ARKOS_ROW_HIGHLIGHT_BLOCK && in_side && state >= 1 && inside_rect(p, side)) {
                float bob = fmod(floor(t / CURSOR_BOB), 2.0);
                float2 o = float2(side.x + (float(FRAME_PX) + bob) * ART_PX, full.y + floor((full.w - 5.0 * ART_PX) * 0.5));
                int2 q = int2(floor((p - o) / ART_PX));
                if (q.x >= 0 && q.y >= 0 && q.y < 5 && q.x <= 2 - abs(q.y - 2)) {
                    col = mix(col, u.colors[ARKOS_ANSI + 3].rgb, v * k);
                    continue;
                }
            }
            if (!inside_rect(p, full) || (kind == ARKOS_TAB_HIGHLIGHT_BLOCK && on_active)) {
                continue;
            }
            if (kind == ARKOS_ROW_HIGHLIGHT_BLOCK && in_side && !inside_rect(p, side)) {
                continue;
            }
            if (kind == ARKOS_TAB_HIGHLIGHT_BLOCK) {
                col = mix(col, mix(col, u.colors[ARKOS_TEXT].rgb, LEDGE_HOVER), v);
                continue;
            }
            col = mix(col, u.colors[state == 1 ? ARKOS_TAB_ACTIVE : ARKOS_TAB_INACTIVE].rgb, v);
            continue;
        }
        if (kind == ARKOS_TAB_BAR_BLOCK) {
            // The pane's frame, in the gap round this strip and the body laid out after it.
            float4 pr = r;
            if (i + 1 < nb && int(u.blocks[i + 1].kind) == ARKOS_PANE_BODY_BLOCK) {
                pr.w += u.blocks[i + 1].rect.w;
            }
            int2 ring = frame_ring(p, pr);
            if (ring.x >= 1 && ring.x <= FRAME_PX) {
                float3 line = mix(frame_line(u), u.colors[ARKOS_FOCUS_RING].rgb, saturate(u.blocks[i].focus));
                // The outer line with its corners left to the ground, the inner one with its
                // corners in the outer line's colour: a notch.
                if (ring.x == FRAME_PX && ring.y == 0) {
                    float lift = 0.0;
                    if (u.blocks[i].busy > 0.5) {
                        float2 run = frame_run(p, pr);
                        lift = glint(run.x, run.y, t) * GLINT_LIFT;
                    }
                    col = mix(col, mix(line, float3(1.0), lift), k);
                } else if (ring.x < FRAME_PX) {
                    col = mix(col, ring.y == 1 ? line : frame_dark(u), k);
                }
            }
        }
        if (kind == ARKOS_PAGE_BLOCK) {
            // The page of the body p is in: as wide as it, and reaching at least across it.
            if (in_body && !paged && abs(r.x - body.x) < 1.0 && abs(r.z - body.z) < 1.0) {
                paged = true;
                page = r;
            }
            continue;
        }
        if (!inside_rect(p, r)) {
            continue;
        }
        if (kind == ARKOS_TITLE_BAR_BLOCK) {
            float3 bar = u.colors[ARKOS_TITLE_BAR].rgb;
            int2 q = int2(floor((p - r.xy) / ART_PX));
            col = bar;
            // Its frame, the panes' own, inside its edges.
            int2 edge = inner_ring(p, r);
            if (edge.x < TITLE_FRAME_PX) {
                // A single line, its corners left to the ground.
                float3 line = edge.y == 1 ? ground(u, p) : frame_line(u);
                // A light running the length of the bar's foot, along the frame's outer line: one
                // pass every TITLE_GLINT_CYCLE seconds, none in the first.
                float gk = floor(t / TITLE_GLINT_CYCLE);
                if (gk >= 1.0 && edge.x == 0 && edge.y == 0 && p.y >= r.y + r.w - ART_PX) {
                    float behind = (t - gk * TITLE_GLINT_CYCLE) * TITLE_GLINT_SPEED - floor((p.x - r.x) / ART_PX);
                    if (behind >= 0.0 && behind < TITLE_GLINT_LEN) {
                        line = mix(line, float3(1.0), TITLE_GLINT_LIFT * (1.0 - behind / TITLE_GLINT_LEN));
                    }
                }
                col = mix(bar, line, k);
                continue;
            }
            // A plate behind the words, the icons and the traffic lights: the bar's plain colour.
            if (plated(p, r, u, nb)) {
                continue;
            }
            // The sky, darkest at the top.
            if (1.0 - float(q.y) / float(SKY_BAR_ROWS) > (((q.x + q.y) & 1) == 0 ? 0.25 : 0.75)) {
                // A step darker than the bar on any theme, so the plates stand out from it: the
                // words, the icons and the lights are on their plates, never on the sky.
                col = mix(bar, darker(u, bar, SKY_DEEP), k);
            }
            int cell = q.x / TITLE_CELL;
            uint id = uint(cell) ^ TITLE_SALT;
            int2 star = int2(cell * TITLE_CELL + 2 + int(hash01(id + 1u) * float(TITLE_CELL - 4)), 2 + int(hash01(id + 2u) * 5.0));
            bool on_star = q.x == star.x && q.y == star.y && hash01(id) < TITLE_STAR_CHANCE;
            int cx = q.x - cloud_at(int(r.z / ART_PX), t);
            int cy = q.y - CLOUD_ROW;
            bool on_cloud = cy >= 0 && cy < 3 && cx >= (cy == 0 ? 3 : 0) && cx < (cy == 0 ? CLOUD_W - 3 : CLOUD_W);
            int tw = int(r.z / ART_PX);
            int c2 = q.x - cloud_at(tw, t * CLOUD2_RATE + CLOUD2_AHEAD);
            on_cloud = on_cloud || (cy >= 1 && cy < 3 && c2 >= (cy == 1 ? 2 : 0) && c2 < (cy == 1 ? CLOUD2_W - 2 : CLOUD2_W));
            float shot = shooting(q.x, q.y - TITLE_FRAME_PX, tw, SKY_BAR_ROWS - 2 * TITLE_FRAME_PX, 6.0, TITLE_SHOOT_CYCLE, t + 3.0, SHOOT_SALT + 1u);
            // A bird crossing from right to left now and then, its wings up and down: none in the
            // first cycle.
            bool on_bird = false;
            float bk = floor(t / BIRD_CYCLE);
            float bs = t - bk * BIRD_CYCLE;
            if (bk >= 1.0 && bs < BIRD_FOR) {
                float across = bs / BIRD_FOR;
                int ax = q.x - (tw + 3 - int(across * float(tw + 6)));
                int ay = q.y - (TITLE_FRAME_PX + 2 + int(rint(sin(across * 14.0))));
                bool up = fmod(floor(t * BAT_FLAPS), 2.0) < 0.5;
                on_bird = ax >= -1 && ax <= 1 && ay >= 0 && ay <= 1 && ((ax != 0) == (up ? ay == 0 : ay == 1));
            }
            if (!on_star && !on_cloud && !on_bird && shot <= 0.0) {
                continue;
            }
            float2 o = r.xy + float2(q) * ART_PX;
            if (o.x < TITLE_LEFT || o.x + ART_PX > r.x + r.z) {
                continue;
            }
            // The cloud, a star over it, the shooting star and the bird; near the bar's words
            // and icons all of it softened into the bar.
            float3 art = col;
            if (on_cloud) {
                art = mix(art, mix(bar, mute(u, u.colors[ARKOS_BANNER].rgb), CLOUD_TONE), 1.0);
            }
            if (on_star) {
                float3 c = hash01(id + 3u) < STAR_FOCUS ? u.colors[ARKOS_FOCUS_RING].rgb : u.colors[ARKOS_TEXT].rgb;
                art = mix(art, mute(u, c), twinkle(float(1000 + cell), t));
            }
            art = mix(art, mute(u, u.colors[ARKOS_TEXT].rgb), shot);
            if (on_bird) {
                art = mute(u, u.colors[ARKOS_TEXT_DIM].rgb);
            }
            if (!cell_clear(o, u, nb)) {
                art = soften(u, art, bar);
            }
            col = mix(col, art, k);
        } else if (kind == ARKOS_SIDEBAR_BLOCK) {
            float3 sb = u.colors[ARKOS_SIDEBAR].rgb;
            col = sb;
            // Its frame, inside its edges.
            if (inner_ring(p, r).x < FRAME_PX) {
                col = mix(sb, boxed(u, p, r, sb), k);
                continue;
            }
            // The scene on the frame's foot, under an open sky; softened where the names lie.
            float4 band = scene_band(r);
            if (band.w <= 0.0 || !inside_rect(p, band)) {
                continue;
            }
            int2 q = int2(floor((p - band.xy) / ART_PX));
            int j = q.y + SCENE_ROWS + HIGH_ROWS - int(band.w / ART_PX);
            int w = int(band.z / ART_PX);
            float3 art = j < HIGH_ROWS ? high_sky(u, q.x, j, t) : scene(u, q.x, j - HIGH_ROWS, w, t);
            art = mix(art, mute(u, u.colors[ARKOS_TEXT].rgb), shooting(q.x, j, w, HIGH_ROWS + STAR_ROWS, 2.0, SHOOT_CYCLE, t, SHOOT_SALT));
            // Under the names it is softened for their contrast; and once they reach into the
            // scene the rest of it is softened with them, so it dims as one.
            float reach = saturate((names_end - band.y) / SOFT_REACH);
            if (p.y < names_end) {
                art = soften(u, art, sb);
            } else if (reach > 0.0) {
                art = mix(art, soften(u, art, sb), reach);
            }
            col = mix(sb, art, k);
        } else if (kind == ARKOS_TAB_BAR_BLOCK) {
            // The ledge.
            bar_focus = saturate(u.blocks[i].focus);
            float3 stone = u.colors[ARKOS_BANNER].rgb;
            float3 joint = mix(stone, u.colors[ARKOS_SIDEBAR].rgb, LEDGE_JOINT);
            int2 q = int2(floor((p - r.xy) / ART_PX));
            int last = int(ceil(r.w / ART_PX)) - 1;
            float3 c = stone;
            if (q.y == 0) {
                c = mix(stone, u.colors[ARKOS_TEXT_DIM].rgb, LEDGE_HI);
            } else if (q.y >= last) {
                c = frame_dark(u);
            } else if (q.y == LEDGE_COURSE || (q.x + (q.y > LEDGE_COURSE ? LEDGE_BRICK / 2 : 0)) % LEDGE_BRICK == 0) {
                c = joint;
            }
            // The gem, near the ledge's right end.
            int2 g = int2(GEM_IN + 3 - int(floor((r.x + r.z - p.x) / ART_PX)), q.y - GEM_ROW);
            if (r.z >= GEM_MIN_W && g.x >= 0 && g.x < 4 && g.y >= 0 && g.y < 4 && !((g.x == 0 || g.x == 3) && (g.y == 0 || g.y == 3))) {
                float3 gem = mix(mix(stone, u.colors[ARKOS_TEXT_DIM].rgb, GEM_DULL), u.colors[ARKOS_FOCUS_RING].rgb, bar_focus);
                // Lit from the top left: a highlight on its first row and first column, a shade
                // on its lower half.
                c = g.y == 0 || (g.y == 1 && g.x == 0) ? mix(gem, float3(1.0), 0.6) : g.y >= 2 ? darker(u, gem, 0.4) : gem;
                // A spark crossing the lit gem from its foot to its top, now and then.
                float spark = fmod(t, GEM_CYCLE);
                if (bar_focus > 0.5 && t >= GEM_CYCLE && spark < GEM_SPARK && g.x == int(spark / GEM_SPARK * 4.0) && g.y == 3 - g.x) {
                    c = float3(1.0);
                }
            }
            col = mix(u.colors[ARKOS_TAB_BAR].rgb, c, k);
        } else if (kind == ARKOS_PANE_BODY_BLOCK) {
            in_body = true;
            body = r;
            body_seed = u.blocks[i].seed;
            body_waits = u.blocks[i].waiting > 0.5;
        } else if (kind == ARKOS_TABLE_HEADER_BLOCK) {
            on_header = true;
        } else if (kind == ARKOS_ACTIVE_TAB_BLOCK) {
            // The doorway: the body's colour, open to it below, with a line along its top in the
            // frame's colour, the focus colour on the focused pane.
            float3 tab = u.colors[ARKOS_TAB_ACTIVE].rgb;
            col = tab;
            on_active = true;
            if (p.y < r.y + ART_PX) {
                col = mix(tab, mix(frame_line(u), u.colors[ARKOS_FOCUS_RING].rgb, bar_focus), k);
                continue;
            }
            if (r.z < LANTERN_MIN_W) {
                continue;
            }
            float2 o = float2(r.x + LANTERN_IN, r.y + floor((r.w - 4.0 * ART_PX) * 0.5));
            int2 q = int2(floor((p - o) / ART_PX));
            col = mix(tab, lantern(u, q, tab, t, u.blocks[i].seed), k);
        }
    }
    if (in_body) {
        // The night, drawn strong enough to read through the tab's paper, and a waiting pane's
        // arrow in its corner.
        float3 bg = u.colors[ARKOS_TERMINAL_BG].rgb;
        // A terminal's night is the body's own; a document's is its page's, which scrolls.
        float4 on = paged ? float4(body.x, min(page.y, body.y), body.z, max(page.y + page.w, body.y + body.w) - min(page.y, body.y)) : body;
        float3 want = night(u, p - on.xy, on.y + on.w - p.y, int(ceil(body.w / ART_PX)) / 2, body_seed, t);
        if (body_waits) {
            int c = int(floor((body.x + body.z - p.x) / ART_PX)) - ARROW_IN;
            int row = ARROW_IN + 2 - int(floor((body.y + body.w - p.y) / ART_PX)) - int(fmod(floor(t / ARROW_BOB), 2.0));
            if (row >= 0 && row < 3 && c >= row && c < ARROW_W - row) {
                want = u.colors[ARKOS_ANSI + 3].rgb;
            }
        }
        // A table's header, whose own fill is clear over a paper that shows the wash.
        if (on_header) {
            want = u.dark > 0.5 ? away(u, bg, HEADER_TONE) : mix(bg, u.colors[ARKOS_TEXT].rgb, HEADER_LIGHT);
        }
        col = mix(bg, through(bg, want), k);
    }
    return float4(col, 1.0);
}
