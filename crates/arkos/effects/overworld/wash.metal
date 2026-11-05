// The overworld wash under the Arkos window: a night overworld of the handheld fantasy games, in
// chunky pixels, drawn only where nothing is written. At the foot of the sidebar, in the empty
// space below its last name, a scene stands on the sidebar's bottom: grass with a winding path,
// pines near both edges, low hills, a castle with lit windows and a pennant, and a dithered sky
// with stars and a crescent moon. When the space is short the scene is cut from the top, the sky
// going first; under SCENE_MIN_H of room there is none. A few stars sit in the title bar clear of
// its words and icons, and each pane's active tab carries a small lantern in the padding before
// its label. Everything else is the theme's own plain colour: the bars, the tabs, and the pane
// bodies, where nothing is drawn. The selected and hovered rows and the hovered tab are filled as
// the app fills them without an effect.
//
// Every colour is the theme's, read each frame. The art is muted, mixed MUTE of the way toward the
// sidebar colour, so it reads as a painted backdrop; only the lit things, the castle's windows, the
// lantern's flame and the fireflies, keep their full colour. An art pixel is ART_PX points square,
// counted from its band's origin, so at scale 1.0 its edges land on whole points and stay crisp.
// The clock's zero, the still frame under Reduce Motion, has every star bright, every window lit
// and every firefly glowing.
//
// The constants and the arithmetic of the layout and the timing are mirrored in Rust
// (`src/effects/overworld.rs`) for its tests; a change to one is made to both. The preamble
// (arkos.metal) is prepended before this is compiled, and declares ArkosInputs, ArkosBlock,
// ArkosVertex and the ARKOS_* indices used here.

// The art pixel, the scene's tallest and least heights, and the title stars' clearance, in points.
constant float ART_PX = 3.0;
constant float SCENE_MAX_H = 140.0;
constant float SCENE_MIN_H = 50.0;
constant float STAR_CLEAR = 12.0;

// A castle window dims every DIM_CYCLE seconds, give or take half of DIM_SLACK, for DIM_FOR
// seconds, and comes back; cycle 0 has none. DIM_DEPTH is how far it dims at the deepest.
constant float DIM_CYCLE = 15.0;
constant float DIM_SLACK = 5.0;
constant float DIM_FOR = 0.8;
constant float DIM_DEPTH = 0.7;
constant int CASTLE_WINDOWS = 5;
constant uint DIM_SALT = 0x85EBCA6Bu;

// The fireflies: how many, the slowest and quickest rise in seconds, and the share of the rise
// each fades in over, stays lit until, and is dark from.
constant int FIREFLIES = 3;
constant float FIREFLY_PERIOD_MIN = 8.0;
constant float FIREFLY_PERIOD_MAX = 11.0;
constant float FIREFLY_FADE_IN = 0.1;
constant float FIREFLY_FADE_FROM = 0.8;
constant float FIREFLY_FADE_TO = 0.95;
constant uint FIREFLY_SALT = 0x27D4EB2Fu;

// A star's twinkle: its shortest and longest period in seconds.
constant float TWINKLE_PERIOD_MIN = 4.0;
constant float TWINKLE_PERIOD_MAX = 9.0;
constant uint TWINKLE_SALT = 0x165667B1u;

// How far the art is pulled toward the sidebar colour.
constant float MUTE = 0.45;

// The scene, in art pixels: SCENE_ROWS rows from the top of the full scene (the band shows its
// lowest ones), laid out across the band's width.
constant int SCENE_ROWS = 46;
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
constant float FIREFLY_HOME[3] = {0.22, 0.6, 0.8};
constant float FIREFLY_WANDER = 6.0;
constant int FIREFLY_FROM = 40;
constant float FIREFLY_CLIMB = 16.0;

// The lantern on an active tab: 3 art pixels wide and 4 tall, LANTERN_IN points in from the tab's
// left so it fits inside the 10 points of padding before the label, centred on the tab's height.
// Its body is the dim text pulled toward the sidebar, warmed by its flame.
constant float LANTERN_IN = 1.0;
constant float LANTERN_TONE = 0.55;
constant float LANTERN_GLOW = 0.18;

// The title bar's stars: none past the traffic lights' end (TITLE_LEFT), one in some of the cells
// of TITLE_CELL columns, on a row from 1 to 7 of the bar.
constant float TITLE_LEFT = 80.0;
constant int TITLE_CELL = 12;
constant float TITLE_STAR_CHANCE = 0.5;
constant uint TITLE_SALT = 0x9E3779B9u;

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

// The band at the bottom of the sidebar's foot the scene is drawn in: at most SCENE_MAX_H tall in
// whole art pixels, on the foot's bottom; zero height under SCENE_MIN_H of room.
float4 scene_band(float4 foot) {
    if (foot.w < SCENE_MIN_H) {
        return float4(0.0);
    }
    float h = floor(min(foot.w, SCENE_MAX_H) / ART_PX) * ART_PX;
    return float4(foot.x, foot.y + foot.w - h, foot.z, h);
}

// Whether a title bar star may be drawn at p: outside every title group grown by STAR_CLEAR.
bool star_clear(float2 p, constant ArkosInputs& u, int nb) {
    for (int i = 0; i < nb; i++) {
        ArkosBlock g = u.blocks[i];
        if (int(g.kind) != ARKOS_TITLE_GROUP_BLOCK) {
            continue;
        }
        float4 r = g.rect;
        if (p.x >= r.x - STAR_CLEAR && p.x <= r.x + r.z + STAR_CLEAR && p.y >= r.y - STAR_CLEAR && p.y <= r.y + r.w + STAR_CLEAR) {
            return false;
        }
    }
    return true;
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

// A star's brightness at t, 0.5 to 1, full at the clock's zero.
float twinkle(float seed, float t) {
    float period = TWINKLE_PERIOD_MIN + (TWINKLE_PERIOD_MAX - TWINKLE_PERIOD_MIN) * hash01(uint(seed) ^ TWINKLE_SALT);
    return 0.75 + 0.25 * cos(6.2831853 * t / period);
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
    // The crescent moon.
    float2 m = float2(float(i - (w - MOON_AT.x)), float(j - MOON_AT.y));
    if (dot(m, m) <= MOON_R2 && dot(m - MOON_BITE, m - MOON_BITE) > MOON_BITE_R2) {
        col = mute(u, text);
    }
    // The hills, the full width.
    float fi = float(i);
    int hill = int(rint(5.0 + 2.0 * sin(fi / 7.0) + 1.5 * sin(fi / 3.0)));
    if (j >= HILL_BASE - hill) {
        col = mute(u, darker(u, sb, HILL_STEP));
    }
    // The castle, centred, its windows lit in the focus colour and one now and then dimming.
    int d = i - (w - 1) / 2;
    if (castle_at(d, j)) {
        float3 stone = mute(u, darker(u, sb, CASTLE_STEP));
        col = stone;
        float2 dim = dimmed_window(t);
        for (int n = 0; n < CASTLE_WINDOWS; n++) {
            if (d == WINDOWS[n].x && j == WINDOWS[n].y) {
                float depth = float(n) == dim.x ? dim.y * DIM_DEPTH : 0.0;
                col = mix(u.colors[ARKOS_FOCUS_RING].rgb, stone, depth);
            }
        }
    }
    if (j == PENNANT.z && d >= PENNANT.x && d <= PENNANT.y) {
        col = mute(u, u.colors[ARKOS_ANSI + 5].rgb);
    }
    // The pines, near both edges, with a light row every third.
    for (int n = 0; n < 3; n++) {
        int3 pine = PINES[n];
        int a = pine.x >= 0 ? pine.x : w + pine.x;
        int row = j - pine.y;
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
        if (hash01((uint(j) * 1024u + uint(i)) ^ GRASS_SALT) < GRASS_BLADES) {
            col = mute(u, green);
        }
        int from = (w - 1) / 2 + PATH_FROM + int(rint(3.0 * sin(float(j) / 5.0)));
        if (i >= from && i < from + 4 + (j - GRASS_TOP) / 5) {
            col = mute(u, mix(sb, u.colors[ARKOS_TEXT_DIM].rgb, PATH_TONE));
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

// The lantern's colour at art pixel q of its 3 by 4, over the tab's colour `tab`.
float3 lantern(constant ArkosInputs& u, int2 q, float3 tab) {
    float3 flame = u.colors[ARKOS_ANSI + 3].rgb;
    if (q.x == 1 && q.y == 2) {
        return flame;
    }
    if ((q.y == 0 && q.x == 1) || (q.y >= 1 && q.y <= 3 && q.x >= 0 && q.x <= 2)) {
        float3 body = mute(u, mix(u.colors[ARKOS_SIDEBAR].rgb, u.colors[ARKOS_TEXT_DIM].rgb, LANTERN_TONE));
        return q.y == 0 ? body : mix(body, flame, LANTERN_GLOW);
    }
    return tab;
}

fragment float4 arkos_wash(ArkosVertex in [[stage_in]], constant ArkosInputs& u [[buffer(0)]]) {
    float2 p = in.uv * u.size;
    float k = saturate(u.intensity);
    float t = u.time;
    float3 col = u.colors[ARKOS_WINDOW_BG].rgb;
    int nb = min(int(u.block_count), 48);
    bool on_active = false;
    // The blocks come in the order they are painted: the title bar, the sidebar and its foot, the
    // panes' tab bars and bodies, the shapes (each active tab among them), then the highlights.
    for (int i = 0; i < nb; i++) {
        ArkosBlock blk = u.blocks[i];
        int kind = int(blk.kind);
        float4 r = blk.rect;
        if (kind == ARKOS_ROW_HIGHLIGHT_BLOCK || kind == ARKOS_TAB_HIGHLIGHT_BLOCK) {
            // Filled as the app fills them: a selected row in the active tab's colour, a hovered
            // row or tab in the inactive tab's (the app's hover wins over its selection). A hovered
            // active tab stays the active tab.
            float4 full = float4(r.xy - STROKE_INSET, r.zw + 2.0 * STROKE_INSET);
            if (!inside_rect(p, full) || (kind == ARKOS_TAB_HIGHLIGHT_BLOCK && on_active)) {
                continue;
            }
            bool selected = kind == ARKOS_ROW_HIGHLIGHT_BLOCK && int(blk.ordinal) == 1;
            float3 fill = u.colors[selected ? ARKOS_TAB_ACTIVE : ARKOS_TAB_INACTIVE].rgb;
            col = mix(col, fill, saturate(blk.value));
            continue;
        }
        if (!inside_rect(p, r)) {
            continue;
        }
        if (kind == ARKOS_TITLE_BAR_BLOCK) {
            col = u.colors[ARKOS_TITLE_BAR].rgb;
            // A star in some of the bar's cells, only where it keeps clear of every group.
            int2 q = int2(floor((p - r.xy) / ART_PX));
            int cell = q.x / TITLE_CELL;
            uint id = uint(cell) ^ TITLE_SALT;
            int2 star = int2(cell * TITLE_CELL + 2 + int(hash01(id + 1u) * float(TITLE_CELL - 4)), 1 + int(hash01(id + 2u) * 7.0));
            if (q.x != star.x || q.y != star.y || hash01(id) >= TITLE_STAR_CHANCE) {
                continue;
            }
            float2 o = r.xy + float2(star) * ART_PX;
            bool grouped = false;
            for (int g = 0; g < nb; g++) {
                grouped = grouped || int(u.blocks[g].kind) == ARKOS_TITLE_GROUP_BLOCK;
            }
            bool clear = grouped && o.x >= TITLE_LEFT && o.x + ART_PX <= r.x + r.z && star_clear(o, u, nb) && star_clear(o + float2(ART_PX, 0.0), u, nb) && star_clear(o + float2(0.0, ART_PX), u, nb) && star_clear(o + ART_PX, u, nb);
            if (clear) {
                float3 c = hash01(id + 3u) < STAR_FOCUS ? u.colors[ARKOS_FOCUS_RING].rgb : u.colors[ARKOS_TEXT].rgb;
                col = mix(col, mute(u, c), twinkle(float(1000 + cell), t) * k);
            }
        } else if (kind == ARKOS_SIDEBAR_BLOCK) {
            col = u.colors[ARKOS_SIDEBAR].rgb;
        } else if (kind == ARKOS_SIDEBAR_FOOT_BLOCK) {
            float4 band = scene_band(r);
            if (band.w <= 0.0 || !inside_rect(p, band)) {
                continue;
            }
            int2 q = int2(floor((p - band.xy) / ART_PX));
            int rows = int(band.w / ART_PX);
            int w = int(band.z / ART_PX);
            col = mix(col, scene(u, q.x, q.y + SCENE_ROWS - rows, w, t), k);
        } else if (kind == ARKOS_TAB_BAR_BLOCK) {
            col = u.colors[ARKOS_TAB_BAR].rgb;
        } else if (kind == ARKOS_PANE_BODY_BLOCK) {
            col = u.colors[ARKOS_WINDOW_BG].rgb;
        } else if (kind == ARKOS_ACTIVE_TAB_BLOCK) {
            float3 tab = u.colors[ARKOS_TAB_ACTIVE].rgb;
            float2 o = float2(r.x + LANTERN_IN, r.y + floor((r.w - 4.0 * ART_PX) * 0.5));
            int2 q = int2(floor((p - o) / ART_PX));
            col = mix(tab, lantern(u, q, tab), k);
            on_active = true;
        }
    }
    return float4(col, 1.0);
}
