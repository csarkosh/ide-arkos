// The runic wash under the Arkos window: digital rain made from the theme's colours, faint green
// runes falling in columns over a flat ground; and for the UI's blocks, a live line on every edge
// the app no longer draws, a soft green fill on the highlights and a faint darkening under the
// bars. The preamble (arkos.metal) is prepended before this is compiled, and declares
// ArkosInputs, ArkosBlock, ArkosVertex and the ARKOS_* indices used here.
//
// Every stream is a pure function of the clock, so Reduce Motion (which freezes it) shows still
// rain. The rain's constants are mirrored in crates/arkos/src/effects/runic.rs, which tests
// them: a change here is made there too.
//
// The edges are Cyrus's tuned settings (2026-10-29), ported from the WebGL mock-up in
// arkos-lab/2026-10-29-matrix-edges/edges.html with the same speeds, sizes and rates; its sin
// hash is this file's hash01, so streams and tears land in other places than in the mock-up.

// The width of a column of rain, in points.
constant float RAIN_COL = 14.0;
// The height of a row of rain, one rune's cell, in points.
constant float RAIN_ROW = 18.0;
// The slowest a column's streams fall, in rows a second.
constant float RAIN_SPEED_MIN = 2.0;
// The fastest a column's streams fall, in rows a second.
constant float RAIN_SPEED_MAX = 6.0;
// The shortest trail a column's streams leave, in rows.
constant float RAIN_TRAIL_MIN = 8.0;
// The longest trail a column's streams leave, in rows.
constant float RAIN_TRAIL_MAX = 24.0;
// Seconds a column waits between its streams, beyond their fall: long enough that about three fifths
// of the columns carry a stream on screen at any moment in a window 800 points tall.
constant float RAIN_GAP = 6.0;
// The rows a stream falls through in its cycle before its trail, whatever the window's height,
// so a resize never moves a column's phase; a window taller than this sees the streams fall on
// into the wait.
constant float RAIN_ROWS_PERIOD = 60.0;
// Seconds a rune shows before its cell picks another.
constant float RUNE_CHANGE = 0.5;
// The most the rain adds of its green to the ground, before the user's intensity.
constant float RAIN_ALPHA = 0.22;
// The rain over a pane's body as it reaches the screen, as a share of RAIN_ALPHA: as faint as a
// source file's (Cyrus, 2026-11-17), the wash drawing it 1 / (1 - PAPER) times as strong under
// the paper. The tab strips are solid and carry none.
constant float BODY_RAIN = 0.11;
// How much of the rain the sidebar carries: as faint as a pane's, in the rain's own colour, so
// the Cmd+0 pane is as quiet as a source file.
constant float SIDE_RAIN = 0.11;
// The head's extra brightness: how far its rune leans from the green toward white.
constant float RAIN_HEAD = 0.35;
// A rune's bit, in points: its 5 by 7 bits make a shape 10 by 14 points, centred in its cell.
constant float RUNE_BIT = 2.0;
// How soft a bit's edges are, as a share of the bit: its square fades out over this much.
constant float RUNE_SOFT = 0.25;

// The edges' light as a whole, before the user's intensity: every part below is scaled by it.
constant float EDGE_INTENSITY = 0.33;
// The width of the line's core, in points: full strength within half of it each side.
constant float EDGE_CORE_WIDTH = 0.55;
// The core's strength.
constant float EDGE_CORE = 0.46;
// How far beyond its width the core fades out, in points.
constant float EDGE_CORE_SOFT = 0.9;
// How far the core leans from the green toward the theme's bright white.
constant float EDGE_CORE_WHITE = 0.3;
// How far the halo reaches, in points: it falls as the square of the distance's share of this.
constant float EDGE_HALO_REACH = 5.2;
// The halo's strength at the line.
constant float EDGE_HALO = 0.5;
// How far out edges are looked for, in points: the halo's reach and a tear's shift, rounded up.
constant float EDGE_REACH = 8.0;
// How much nearer a busy block's edge counts, in points, so a line it shares with an idle block
// (two panes side by side, a tab bar under the title bar) is the busy one's.
constant float EDGE_TIE = 1.0;
// How near the window's border an edge's line lies, in points, for the wash to skip it (the
// window's own outline is no pane's edge); the menu stage keeps every edge.
constant float EDGE_BORDER = 1.5;
// An edge's rune cell along a vertical edge, as a share of the rune's size.
constant float EDGE_RUNE_TALL = 1.15;
// An edge's rune across a vertical edge, as a share of its size (across a horizontal one it is
// the size itself).
constant float EDGE_RUNE_THIN = 0.6;
// An edge's rune cell along a horizontal edge, as a share of the rune's size.
constant float EDGE_RUNE_WIDE = 0.8;
// The gap at each end of a rune cell along the edge, as a share of the cell.
constant float EDGE_RUNE_MARGIN = 0.1;

// A: code rains down the vertical edges, one stream an edge, in points a second.
constant float A_SPEED = 222.0;
// The slowest an edge's stream falls, as a share of A_SPEED.
constant float A_SPEED_LOW = 0.6;
// The fastest an edge's stream falls, as a share of A_SPEED.
constant float A_SPEED_HIGH = 1.4;
// The stream's trail, in points: it fades to nothing over this behind the head.
constant float A_TRAIL = 110.0;
// Seconds an edge waits between its streams, beyond their fall.
constant float A_GAP = 3.0;
// The head cell's brightness, against the trail's 1 at the head.
constant float A_HEAD = 1.7;
// How far the head leans from the green toward the theme's bright white.
constant float A_HEAD_WHITE = 0.45;
// The size of the stream's runes, in points.
constant float A_RUNE = 7.0;
// Seconds a stream's rune shows before its cell picks another.
constant float A_FLIP = 0.12;
// The share of a stream's rune bits that are lit.
constant float A_DENSITY = 0.55;

// B: the horizontal edges' line is code, small upright runes this many points in size.
constant float B_RUNE = 6.0;
// Seconds a line's rune shows before its cell picks another.
constant float B_FLIP = 0.5;
// The share of a line's rune bits that are lit.
constant float B_DENSITY = 0.5;
// A lit bit's strength.
constant float B_STRENGTH = 0.8;
// How much of the core the runes replace on a horizontal edge.
constant float B_REPLACE = 0.6;

// D: signal tears a minute across a window of D_EDGES edges.
constant float D_PER_MINUTE = 6.0;
// How long a tear lasts, in seconds, and the slots the tears' clock is cut into.
constant float D_DURATION = 0.12;
// The edges the tears are shared among: a window of a pane or two reports about this many.
constant float D_EDGES = 12.0;
// The length of the stretch of edge a tear shifts, in points.
constant float D_LENGTH = 40.0;
// How far a tear shifts its stretch across the edge, in points, to one side or the other.
constant float D_OFFSET = 2.0;
// How much brighter a torn stretch is lit.
constant float D_FLASH = 1.8;

// E: a busy pane's edges (its Claude session works) stream this many times faster.
constant float E_SPEED = 2.0;
// And are lit this many times brighter.
constant float E_BRIGHT = 1.4;

// A highlight's strength at full fade, by its ordinal: hovered, selected, both (watercolor's).
constant float HIGHLIGHT_STRENGTH[3] = {0.2, 0.3, 0.36};

// A hash of n in 0 to 1, in integer arithmetic (PCG's output step) so the Rust mirror agrees to
// the bit: the top 24 bits, times a power of two, are exact in a float.
float hash01(uint n) {
    uint h = n * 747796405u + 2891336453u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    h ^= h >> 22u;
    return float(h >> 8u) * (1.0 / 16777216.0);
}

float sd_box(float2 q, float2 hb) {
    float2 d = abs(q) - hb;
    return length(max(d, 0.0)) + min(max(d.x, d.y), 0.0);
}

bool inside_box(float2 p, float4 r) {
    return p.x >= r.x && p.y >= r.y && p.x < r.x + r.z && p.y < r.y + r.w;
}

// How much of a rune's bit lies at local (points from its cell's top left) in the given cell at
// time t: the cell's 5 by 7 pattern is the bits of a hash of the cell and its rune's slot, 24
// bits from one hash and the last 11 from a second, so a pixel needs only the hash its bit is in.
// Each cell's slots are offset by a fraction of their own, so the runes do not all change at once.
float rune(float2 cell, float2 local, float t) {
    float2 grid = (local - 0.5 * (float2(RAIN_COL, RAIN_ROW) - RUNE_BIT * float2(5.0, 7.0))) / RUNE_BIT;
    if (grid.x < 0.0 || grid.y < 0.0 || grid.x >= 5.0 || grid.y >= 7.0) {
        return 0.0;
    }
    float2 bit = floor(grid);
    uint b = uint(bit.y) * 5u + uint(bit.x);
    float slot = floor(t / RUNE_CHANGE + fract(dot(cell, float2(0.618034, 0.381966))));
    uint key = (uint(cell.x) * 73856093u) ^ (uint(cell.y) * 19349663u) ^ (uint(slot) * 83492791u);
    uint half_key = b < 24u ? 0u : 1u;
    uint bits = uint(hash01(key * 2u + half_key) * 16777216.0);
    if (((bits >> (b % 24u)) & 1u) == 0u) {
        return 0.0;
    }
    float2 q = abs(fract(grid) - 0.5);
    return 1.0 - smoothstep(0.5 - RUNE_SOFT, 0.5, max(q.x, q.y));
}

// The rain at p (points) at time t, in the theme's green: its colour and its strength, 0 to 1,
// before RAIN_ALPHA and the user's intensity. Each column has its own speed, trail and phase from
// hash01 of its index. A stream's head falls a row at a time from the window's top with its trail
// behind it, through RAIN_ROWS_PERIOD rows and its trail's length, then on through the column's
// wait of RAIN_GAP seconds; the trail fades linearly to nothing behind the head, and the head
// leans toward white. The window's height plays no part, so a resize leaves every stream where
// it was.
float4 rain(float2 p, float t, float3 green) {
    float2 cell = floor(p / float2(RAIN_COL, RAIN_ROW));
    uint col = uint(max(cell.x, 0.0)) * 4u;
    float speed = mix(RAIN_SPEED_MIN, RAIN_SPEED_MAX, hash01(col));
    float trail = mix(RAIN_TRAIL_MIN, RAIN_TRAIL_MAX, hash01(col + 1u));
    // A stream's cycle in rows: its fall, its trail out after it, and the wait.
    float period = RAIN_ROWS_PERIOD + trail + RAIN_GAP * speed;
    float head = floor(fract(t * speed / period + hash01(col + 2u)) * period);
    float behind = head - cell.y;
    if (behind < 0.0 || behind >= trail) {
        return float4(0.0);
    }
    float lit = rune(cell, p - cell * float2(RAIN_COL, RAIN_ROW), t);
    float fade = 1.0 - behind / trail;
    float3 c = behind < 1.0 ? mix(green, float3(1.0), RAIN_HEAD) : green;
    return float4(c, lit * fade);
}

// The edge nearest a pixel so far, and where the pixel sits by it.
struct EdgeHit {
    float rank;    // what the edges are compared by: d, less EDGE_TIE for a busy block's edge
    float d;       // the distance to the edge's segment, its ends included, in points
    float across;  // the signed distance across the edge's line, positive outside its block
    float over;    // how far past the segment's nearer end the pixel lies along it (0 beside it)
    float along;   // the distance along the edge from its start (its top or its left), 0 to len
    float len;     // the edge's length
    float value;   // its block's fade
    float busy;    // its block's busy flag
    uint key;      // the edge's own key, from its block's seed and its side
    bool vert;     // whether the edge is vertical (a left or right side)
};

EdgeHit no_edge() {
    EdgeHit h;
    h.rank = 1e5;
    h.d = 1e5;
    h.across = 0.0;
    h.over = 0.0;
    h.along = 0.0;
    h.len = 1.0;
    h.value = 1.0;
    h.busy = 0.0;
    h.key = 0u;
    h.vert = false;
    return h;
}

// Takes blk's edge on side (0 top, 1 right, 2 bottom, 3 left) into h when it ranks nearer: the
// pixel lies across from its line and along from its start, and the edge is len long. Its key is
// the block's seed and the side, so a block keeps its edges' grain wherever it sits in the list.
void take_edge(thread EdgeHit& h, ArkosBlock blk, uint side, float across, float along, float len) {
    float over = max(max(-along, along - len), 0.0);
    float e = length(float2(across, over));
    float rank = e - blk.busy * EDGE_TIE;
    if (rank < h.rank) {
        uint key = as_type<uint>(blk.seed) * 4u + side;
        h.rank = rank;
        h.d = e;
        h.across = across;
        h.over = over;
        h.along = clamp(along, 0.0, len);
        h.len = len;
        h.value = blk.value;
        h.busy = blk.busy;
        h.key = key;
        h.vert = (side & 1u) == 1u;
    }
}

// Whether an edge's line at line lies on the window's border along an axis lim points long: within
// EDGE_BORDER of either end. A lim of 0 keeps every edge.
bool on_border(float line, float lim) {
    return lim > 0.0 && (line < EDGE_BORDER || line > lim - EDGE_BORDER);
}

// Takes blk's reported edges into h when one is nearer p; d is p's distance to blk's box. An edge
// whose line lies on the border of a window of size win is skipped, as the mock-up skips them; a
// win of 0 (the menu stage) keeps every edge.
void nearest_edge(float2 p, ArkosBlock blk, float d, thread EdgeHit& h, float2 win) {
    uint edges = uint(blk.edges);
    if (edges != 0u && abs(d) < EDGE_REACH) {
        float4 r = blk.rect;
        float x0 = r.x, y0 = r.y, x1 = r.x + r.z, y1 = r.y + r.w;
        if ((edges & ARKOS_EDGE_TOP) && !on_border(y0, win.y)) { take_edge(h, blk, 0u, y0 - p.y, p.x - x0, r.z); }
        if ((edges & ARKOS_EDGE_RIGHT) && !on_border(x1, win.x)) { take_edge(h, blk, 1u, p.x - x1, p.y - y0, r.w); }
        if ((edges & ARKOS_EDGE_BOTTOM) && !on_border(y1, win.y)) { take_edge(h, blk, 2u, p.y - y1, p.x - x0, r.z); }
        if ((edges & ARKOS_EDGE_LEFT) && !on_border(x0, win.x)) { take_edge(h, blk, 3u, x0 - p.x, p.y - y0, r.w); }
    }
}

// Where a pixel falls among an edge's rune cells of the given size: its cell along the edge from
// the edge's start, the cell's length, and the pixel's place in the cell's rune across the line
// (u) and along it (v), each 0 to 1 inside the rune.
struct RuneCell {
    float cell;
    float cell_len;
    float u;
    float v;
    bool inside;
};

RuneCell rune_cell(float across, float along, bool vert, float size) {
    RuneCell c;
    c.cell_len = size * (vert ? EDGE_RUNE_TALL : EDGE_RUNE_WIDE);
    float width = vert ? size * EDGE_RUNE_THIN : size;
    c.u = across / width + 0.5;
    c.cell = floor(along / c.cell_len);
    c.v = (fract(along / c.cell_len) - EDGE_RUNE_MARGIN) / (1.0 - 2.0 * EDGE_RUNE_MARGIN);
    c.inside = c.u >= 0.0 && c.u < 1.0 && c.v >= 0.0 && c.v < 1.0;
    return c;
}

// Whether the bit under the pixel of the upright rune in cell c is lit, 1 or 0: the rune is 3
// bits wide and 5 tall, standing up whichever way its edge runs, and each bit is lit by a hash
// of the edge's base key, the cell, the rune's slot and the bit, for the given share of bits.
float rune_bit(RuneCell c, bool vert, uint base, uint slot, float density) {
    float2 b = vert ? floor(float2(c.u * 3.0, c.v * 5.0)) : floor(float2(c.v * 3.0, (1.0 - c.u) * 5.0));
    uint bit = uint(b.y) * 3u + uint(b.x);
    uint k = base ^ (uint(c.cell) * 73856093u) ^ (slot * 19349663u) ^ (bit * 83492791u);
    return hash01(k) < density ? 1.0 : 0.0;
}

// The light the nearest edge casts at the pixel at time t: its colour (the theme's green, the
// core and a stream's head leaning toward white_c) and its strength, before the user's intensity.
// The line is a core and a halo; a tear (D) now and then shifts a stretch of it across and
// flashes it; a horizontal edge's line is runes that flip (B), its core mostly replaced; a stream
// of runes falls down a vertical edge (A); and a busy block's edges (E) stream faster and burn
// brighter. Everything is worked out once, for the nearest edge alone.
float4 edge_glow(EdgeHit h, float t, float3 green, float3 white_c) {
    if (h.d >= EDGE_REACH) {
        return float4(0.0);
    }
    float pace = mix(1.0, E_SPEED, h.busy);
    float bright = mix(1.0, E_BRIGHT, h.busy);
    // The edge's hashes, one role each: 2 A's rune bits, 3 B's slot offsets, 4 B's rune bits,
    // 5 A's speed, 6 A's phase, 7 A's slot offsets (the tears hash the slot in apart from these).
    uint base = h.key * 8u;

    // D: a stretch of the edge tears sideways and flashes, about D_PER_MINUTE a minute across
    // the window. Slot 0, where the clock stands under Reduce Motion, never tears.
    float shift = 0.0;
    float flash = 1.0;
    float chance = D_PER_MINUTE / 60.0 * D_DURATION / D_EDGES;
    uint dslot = uint(floor(t / D_DURATION));
    uint tk = (h.key * 8u) ^ (dslot * 83492791u);
    if (dslot != 0u && hash01(tk) < chance) {
        float centre = hash01(tk + 1u) * h.len;
        if (abs(h.along - centre) < D_LENGTH * 0.5) {
            shift = hash01(tk + 2u) > 0.5 ? D_OFFSET : -D_OFFSET;
            flash = D_FLASH;
        }
    }
    float across = h.across - shift;
    float dl = length(float2(across, h.over));

    // B: a horizontal edge's line is small runes flipping on their own clock.
    float b_lit = 0.0;
    float core_share = 1.0;
    if (!h.vert) {
        core_share = 1.0 - B_REPLACE;
        RuneCell c = rune_cell(across, h.along, false, B_RUNE);
        if (c.inside && h.over <= 0.0) {
            float slot = floor(t / B_FLIP + hash01((base + 3u) ^ (uint(c.cell) * 2654435761u)));
            b_lit = rune_bit(c, false, base + 4u, uint(slot), B_DENSITY) * B_STRENGTH;
        }
    }

    // A: one stream of runes falls down a vertical edge from its top, its trail fading behind a
    // brighter, whiter head, then the edge waits A_GAP seconds for the next.
    float a_glow = 0.0;
    float a_head = 0.0;
    if (h.vert) {
        float speed = A_SPEED * mix(A_SPEED_LOW, A_SPEED_HIGH, hash01(base + 5u)) * pace;
        float cycle = h.len + A_TRAIL + A_GAP * speed;
        float head = fract(t * speed / cycle + hash01(base + 6u)) * cycle;
        RuneCell c = rune_cell(across, h.along, true, A_RUNE);
        float behind = (floor(head / c.cell_len) - c.cell) * c.cell_len;
        if (behind >= 0.0 && behind < A_TRAIL && c.inside && h.over <= 0.0) {
            float slot = floor(t / A_FLIP + hash01((base + 7u) ^ (uint(c.cell) * 2654435761u)));
            float lit = rune_bit(c, true, base + 2u, uint(slot), A_DENSITY);
            a_head = behind < c.cell_len ? 1.0 : 0.0;
            a_glow = lit * (1.0 - behind / A_TRAIL) * mix(1.0, A_HEAD, a_head);
        }
    }

    // The line: a core, mostly replaced by the runes on a horizontal edge, and a halo.
    float core = (1.0 - smoothstep(EDGE_CORE_WIDTH * 0.5, EDGE_CORE_WIDTH * 0.5 + EDGE_CORE_SOFT, dl)) * EDGE_CORE * core_share;
    float fall = max(0.0, 1.0 - dl / EDGE_HALO_REACH);
    float halo = fall * fall * EDGE_HALO;

    float s = halo + b_lit + core + a_glow;
    if (s <= 0.0) {
        return float4(0.0);
    }
    float3 core_col = mix(green, white_c, EDGE_CORE_WHITE);
    float3 a_col = mix(green, white_c, A_HEAD_WHITE * a_head);
    float3 c = (green * (halo + b_lit) + core_col * core + a_col * a_glow) / s;
    return float4(c, s * EDGE_INTENSITY * bright * flash);
}

// How green a colour is: its green above the larger of its red and blue.
float greenness(float3 c) {
    return c.g - max(c.r, c.b);
}

// The colour the effect's greens were made around: the matrix theme's focus ring. Under another
// theme the green is turned round the colour wheel by the distance from this hue to the hue of
// the theme's focus ring, keeping its saturation and brightness, so navy's rain and edges are
// blue; a ring with under MIN_SATURATION has no hue to speak of and turns nothing. The glyph
// shader carries the same arithmetic, mirrored in crates/arkos/src/effects/accent.rs; a change
// to one is made to all three.
constant float3 HOME = float3(0.494118, 0.839216, 0.419608);
constant float MIN_SATURATION = 0.2;

// Hue (0 to 1, a full turn), saturation and value.
float3 rgb_to_hsv(float3 c) {
  float4 K = float4(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
  float4 p = mix(float4(c.bg, K.wz), float4(c.gb, K.xy), step(c.b, c.g));
  float4 q = mix(float4(p.xyw, c.r), float4(c.r, p.yzx), step(p.x, c.r));
  float d = q.x - min(q.w, q.y);
  return float3(abs(q.z + (q.w - q.y) / (6.0 * d + 1e-10)), d / (q.x + 1e-10), q.x);
}

float3 hsv_to_rgb(float3 c) {
  float3 p = abs(fract(c.xxx + float3(1.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0);
  return c.z * mix(float3(1.0), saturate(p - 1.0), c.y);
}

float3 toward_accent(float3 c, float3 accent) {
  float3 a = rgb_to_hsv(accent);
  if (a.y < MIN_SATURATION) {
    return c;
  }
  float3 h = rgb_to_hsv(c);
  h.x = fract(h.x + a.x - rgb_to_hsv(HOME).x + 1.0);
  h.y *= min(a.y / rgb_to_hsv(HOME).y, 1.0);
  return hsv_to_rgb(h);
}

// The theme's green: the greenest of its ANSI green, the terminal's text colour and the focus
// ring, the ANSI green on a tie, so another theme's rain reads in its own green.
float3 theme_green(constant ArkosInputs& u) {
    float3 best = u.colors[ARKOS_ANSI + 2].rgb;
    float3 fg = u.colors[ARKOS_TERMINAL_FG].rgb;
    float3 ring = u.colors[ARKOS_FOCUS_RING].rgb;
    best = greenness(fg) > greenness(best) ? fg : best;
    return greenness(ring) > greenness(best) ? ring : best;
}


// ---- The world: the overworld's features in the runic's light (runic world spec) ----
// Every pane wears a frame in the gap round it, a neon line with its corners cut, lit in the
// focus colour on the focused pane and on the sidebar while it has the keys; a packet of light
// runs round the frame of a pane whose Claude session works, and a block cursor blinks on the
// frame's foot of one whose session waits. Each tab bar is a solid panel, lit along its top and
// shaded along its foot, faint seams across it and a trace of light under its top; the active tab
// is a doorway in the tab's colour open to the body below, a neon line along its top (the focus
// colour on the focused pane) and a lamp on it (a red paper lantern in the city, a changing rune
// in the code), and a lamp at the bar's right end is lit on the focused pane. A pane's body is
// drawn as it should reach the screen through its paper (PAPER): its top a band of storm cloud
// (the city) or a drizzle of code (the code), its foot two rows of skyline with lit windows, and
// the rain over it as faint as over a source file. The title bar is a strip of storm
// clouds, its words and icons on plates. The sidebar wears a frame, a chevron marks its selected
// row, and at its foot, below the names, stands the theme's scene (`ArkosInputs::scene`): 0 a
// city skyline in the rain, its towers' windows lit, runic lightning now and then; 1 a digital
// horizon, a grid floor running to a line of light with a dense city of wireframe towers on it,
// both coming toward the viewer, code raining in its sky and falling onto its road. Behind a terminal, the far skyline along its
// foot. The clock's zero (Reduce Motion) has no bolt, nothing falling, every tower standing and
// every light on.

// The frame's line: how far outside a pane's edges (the gap is 16, so two neighbours' frames keep
// 6 points of ground between them), its width, its halo, its corner cut.
constant float FRAME_OUT = 5.0;
constant float FRAME_W = 1.3;
constant float FRAME_HALO = 4.0;
constant float FRAME_CHAMFER = 7.0;
// An unfocused frame's strength against a focused one's 1.
constant float FRAME_IDLE = 0.5;
// The focused pane's frame is drawn a little stronger: wider, with a wider halo and brighter
// (Cyrus, 2026-11-17: "a little stronger").
constant float FOCUS_FRAME_W = 2.0;
constant float FOCUS_FRAME_HALO = 5.5;
constant float FOCUS_FRAME_LIGHT = 1.3;
// Inside the focused pane's frame, flush against it and following its shape (its cut corners
// too), the runic edges run in the focus colour: runes flipping along its top and foot, streams
// falling down its sides, a tear now and then (Cyrus, 2026-11-17: "those runic border effects on
// the inside of that border", "keep them flush together", "follow the border's shape"). Their
// line lies INNER_IN inside the frame's line: half the focused frame's width and half a side
// stream's rune, so the runes meet the line; nothing of them is drawn on or past the line.
constant float INNER_IN = 3.2;
constant float INNER_LIGHT = 0.8;
// The packet of light round a working pane's frame: its length in points and its speed.
constant float PACKET_LEN = 70.0;
constant float PACKET_SPEED = 240.0;
// The waiting cursor: its size, how far in from the frame's right end, and its blink.
constant float2 WAIT_SIZE = float2(7.0, 10.0);
constant float WAIT_IN = 26.0;
constant float WAIT_BLINK = 1.1;
// The tab strip: its top row lifted toward the dim text, its foot shaded, a seam every
// STRIP_PANEL points, a trace of the theme's light STRIP_TRACE strong a point under its top, and a
// hovered tab STRIP_HOVER toward the text.
constant float STRIP_HI = 0.3;
constant float STRIP_FOOT = 0.45;
constant float STRIP_PANEL = 56.0;
constant float STRIP_SEAM = 0.55;
constant float STRIP_TRACE = 0.14;
constant float STRIP_HOVER = 0.1;
// The active tab's line along its top, its glow's reach below it, and its lamp: in from the tab's
// left, flickering this many times a second, drawn on a tab at least this wide.
constant float DOOR_LINE = 2.0;
constant float DOOR_GLOW = 5.0;
constant float TAB_LAMP_IN = 6.0;
constant float TAB_LAMP_FLICKERS = 5.0;
constant float TAB_LAMP_MIN_W = 30.0;
// A pane's body: the paper over it (the manifest's paper_alpha), the band at its top (its height
// and how far it is moved away from the text at its top), and the skyline at its foot (the far
// row's and the near row's height and their tones away from the text, the windows' strength).
constant float PAPER = 0.6;
constant float BODY_TOP_H = 40.0;
constant float BODY_DUSK = 0.22;
constant float FOOT_FAR_H = 36.0;
constant float FOOT_NEAR_H = 22.0;
constant float FOOT_FAR = 0.07;
constant float FOOT_NEAR = 0.45;
constant float FOOT_WINDOW = 0.32;
// The lamp at a tab bar's right end: its radius and how far in from the end its centre sits,
// clear of the bar's + button (about 27 points at its end), as overworld's gem is (Cyrus,
// 2026-11-17: the lamp at 16 sat on the +).
constant float LAMP_R = 2.6;
constant float LAMP_IN = 38.0;
// The scene at the sidebar's foot: its height, the least room it is drawn in, and how far below
// the last name it comes back to full strength.
constant float SCENE_H = 230.0;
constant float SCENE_MIN_H = 90.0;
constant float SOFT_BUFFER = 30.0;
// How much of the scene shows under the names (the rest is the sidebar's own colour): little
// enough that the dim text keeps 4.5:1 over a lit window (a GPU test holds it).
constant float UNDER_NAMES = 0.08;
// The clouds over the city: they reach up the sidebar to half its height (or the scene's top, if
// that is higher), thinning out over this many points at each end.
constant float CLOUD_FADE = 40.0;
// How many clouds in the bank behind and in front, and the slowest and fastest of their speeds across, in points a second.
constant int CLOUD_BACK = 10;
constant int CLOUD_FRONT = 9;
constant float CLOUD_SPEED_MIN = 2.0;
constant float CLOUD_SPEED_MAX = 11.0;
// How far down the scene the clouds reach, as a share of its height.
constant float CLOUD_FOOT = 0.42;
// Lightning: one bolt a cycle at most, lasting this long; none in the first cycle.
constant float BOLT_CYCLE = 9.0;
constant float BOLT_FOR = 1.2;
// The bolt comes up over BOLT_RISE and fades out over the rest of BOLT_FOR, at most BOLT_PEAK:
// soft, never a hard flicker (Cyrus, 2026-11-17: "softened so it is not as jarring").
constant float BOLT_RISE = 0.15;
constant float BOLT_PEAK = 0.7;
// The digital horizon (the code scene; Cyrus, 2026-11-17, picked over the city of code): the
// horizon's height as a share of the scene, the grid's spacing across and into it (in camera
// heights) and how many of its rows come toward the viewer a second.
constant float HZ_AT = 0.4;
constant float GRID_X = 0.9;
constant float GRID_Z = 1.6;
constant float GRID_SPEED = 0.45;
// The city on the grid: a tower on most cells either side of an open road (ROAD_CELLS wide),
// LANES cells a side, rows from just in front of the viewer to Z_FAR, standing still on the floor
// so they come toward and past the viewer at the floor's speed (Cyrus, 2026-11-17: "moving
// towards and past the user at the same speed as the floor", "so many buildings that it feels
// dense"); far ones fade up out of the horizon.
constant int LANES = 6;
constant int ROAD_CELLS = 1;
constant float TOWER_ON = 0.8;
// Trees in each lot of a park (and a lamp post).
constant int PARK_TREES = 3;
constant float Z_NEAR = 0.7;
constant float Z_FAR = 30.0;
// The tallest tower, in camera heights.
constant float TOWER_MAX = 4.8;
// City life (Cyrus, 2026-11-17: "some city life added between the buildings"): every
// CROSS_EVERY-th row is a cross street with no towers, ROAD_CARS cars a lane on the road (one
// lane coming on, one going away, their speeds over the floor's) and CROSS_CARS on each cross
// street, lit windows on the towers' faces near enough to show them, and DRONES lights drifting
// over the roofs.
constant int CROSS_EVERY = 4;
// Twenty-four a lane, so the road is as busy as the cross streets (Cyrus, 2026-11-17: "not enough
// vehicles moving vertically"), queued QUEUE_GAP apart behind each stop line (Cyrus, 2026-11-17:
// cars must "not drive through each other or completely overlap each other when stopped at a
// light"): each car has its own place in one block's queue, and the queue moves off on the green
// as a platoon, each car QUEUE_START after the one ahead at the same pace, so no two ever close up.
constant int ROAD_CARS = 24;
constant float QUEUE_GAP = 0.3;
constant float QUEUE_START = 0.25;
constant int CROSS_CARS = 2;
constant int DRONES = 3;
// The city's laws (Cyrus, 2026-11-17: traffic and people "should follow traffic and pedestrian
// laws"; "make the pedestrian traffic denser. It should feel like NYC"). Every intersection runs
// one signal cycle, SIG_CYCLE seconds from the clock's zero: the road green for ROAD_GREEN (its
// last AMBER amber), ALL_RED both ways, the cross streets green for CROSS_GREEN (its last AMBER
// amber), ALL_RED again. Cars keep to the right, stop at the stop lines behind the crosswalks
// and move only on their green; a road car goes one or two blocks a green and is clear of every
// intersection before it turns. People keep to the sidewalks and cross only on the crosswalks
// with their green: along the road with the road's, across it with the cross streets'. Every
// motion is locked to the cycle, so it is worked out from the clock alone. The city wraps every
// WRAP_BLOCKS blocks, so a wrapped car or person is still at a stop line or a corner.
constant float SIG_CYCLE = 12.0;
constant float ROAD_GREEN = 6.5;
constant float ALL_RED = 0.5;
constant float CROSS_GREEN = 4.5;
constant float AMBER = 1.0;
constant float WRAP_BLOCKS = 4.0;
// A stop line's distance before a cross street's edge (behind the crosswalk), and a cross car's
// before the road's edge; the crosswalks' place and width, out from a street's edge.
constant float STOP_BACK = 0.55;
constant float CROSS_STOP = 0.42;
constant float WALK_AT = 0.18;
constant float WALK_W = 0.24;
// New York's cyclists and walkers (Cyrus, 2026-11-17: "bicyclists never obey traffic laws. So
// allow them to break the traffic laws as long as another car does not hit them. They are crazy
// and will run a red light even if they barely avoid an accident", "Same thing with
// pedestrians"; 2026-11-18: "make the pedestrians and bicyclists jaywalk more often as long as
// they don't get hit by a car or another bicycle", "they will cross the street as soon as the cars
// have passed", "same thing with the bicyclists"; then "walkers and cyclists should start walking
// before the cars have passed ... They will time their walking so they're already crossing the
// street as the car is passing"). Each goes at the earliest moment that never puts it in a moving
// car's or cyclist's lane, lane by lane, found by `effects::runic`'s tests: a JAYWALK share of the
// walkers step off against the light at WALK_GO, within JAY_SPREAD of each other, while the cross
// street's last car is still passing their crosswalk; the road's cyclists reach the stop line at
// BIKE_GO, within BIKE_SPREAD, as that car goes by, and ride through the red; the cross streets'
// cyclists set off over the road at CROSS_BIKE_GO, ahead of their green, as soon as no road car,
// road cyclist or walker will be in the lane they are crossing.
constant float JAYWALK = 0.85;
constant float WALK_GO = 8.85;
constant float JAY_SPREAD = 0.3;
constant float WAIT_DEPTH = 1.6;
constant float CREEP_KEEP = 0.3;
constant float GUTTER = 0.3;
constant float CREEP_T = 2.0;
// How far apart across its sidewalk the road's walkers keep, nearly its whole width.
constant float WALKER_SPREAD = 0.3;
constant float BIKE_GO = 8.05;
constant float BIKE_SPREAD = 0.3;
constant float CROSS_BIKE_GO = 5.95;
// The road's crossers (`road_crosser_at`).
constant float CROSS_FIRST = 5.6;
constant float CROSS_SPREAD = 3.0;
constant float CROSS_WAIT_DEPTH = 0.2;
constant float CROSS_ACROSS = 1.0;
constant float WAIT_ALONG = 0.5;
constant float CROSS_MERGE = 0.3;
constant float CROSS_CLEAR = 0.3;
// A person's reach past its middle, and the room it leaves a cyclist going by.
constant float PERSON_BODY = 0.06;
constant float NEAR_MISS = 0.03;
constant float SWERVE = 0.04;
constant float LATE_SHARE = 0.3;
constant float LATE_GO = 9.9;
constant float LATE_SPREAD = 0.5;
// The sidewalks (Cyrus, 2026-11-17: "I want the sidewalks more prominent"): paved bands where the
// people walk, SIDEWALK_W wide, along the road from SIDEWALK_IN inside its edge and along both sides
// of every cross street from its curb; lighter than the floor at SIDEWALK_FILL, jointed, and a lit
// curb where each meets a street at SIDEWALK_CURB_LIGHT.
constant float SIDEWALK_IN = 0.02;
constant float SIDEWALK_W = 0.32;
constant float SIDEWALK_FILL = 0.45;
constant float SIDEWALK_CURB_LIGHT = 0.6;
// A food cart (Cyrus, 2026-11-18: its line "should stay within the sidewalk"): CART_W either side
// of its middle, its line CART_FRONT off its road side and running along the sidewalk from there,
// CART_GAP from one person to the next.
constant float CART_W = 0.16;
constant float CART_FRONT = 0.1;
constant float CART_GAP = 0.13;
// People: PEOPLE walkers and STROLLERS strollers on each of the road's sidewalks, CROSS_PEOPLE on
// each of a cross street's two,
// PERSON_H camera heights tall, their legs swinging STEPS times a second while they walk.
// Cyclists (Cyrus, 2026-11-17: "include cyclists on the street traffic too"): ROAD_BIKES each way
// in the road's bike lanes by the curbs, BIKE_LANE out from the middle, keeping the law as the cars
// do but slower, one block a green; one each way on every cross street, waiting in its bike lane
// at the stop line ahead of the cars and going first on the cross streets' green.
constant int ROAD_BIKES = 3;
constant float BIKE_LANE = 0.78;
constant int PEOPLE = 40;
constant int STROLLERS = 48;
constant int CROSS_PEOPLE = 12;
constant float PERSON_H = 0.16;
constant float STEPS = 2.2;
// The city's data (Cyrus, 2026-11-17: "the runic rain comes from users uploading and download
// data ... it goes to and originates from pedestrians, bicyclists, cars, food carts, and
// buildings"; he chose that all rain is data, that direction alone tells the two apart, and a
// steady trickle): DATA_STREAMS slots, each starting a transfer every DATA_SLOT_MIN to
// DATA_SLOT_MAX seconds between the sky and one thing in the city, a walker, a car, a cyclist, a
// tower's window or a food cart, following it as it moves. A download falls from the sky onto it,
// an upload rises from it into the sky: a column of glyph cells RAIN_COL_W wide in the world, its
// bright head crossing it in DATA_TRAVEL_MIN to DATA_TRAVEL_MAX seconds with DATA_TRAIL cells
// fading behind, and a flicker on the thing as a download lands or an upload leaves.
constant int DATA_STREAMS = 14;
constant float DATA_SLOT_MIN = 6.0;
constant float DATA_SLOT_MAX = 10.0;
constant float DATA_TRAVEL_MIN = 3.5;
constant float DATA_TRAVEL_MAX = 5.0;
constant float DATA_TRAIL = 14.0;
// How strongly the whole channel shows while its transfer runs, against the packet's 1.
constant float DATA_CHANNEL = 0.2;
// How often a glyph of the city's data changes, in seconds: slowly, so the moving trails carry the
// rain and the sky does not fizz (Cyrus, 2026-11-17: "the rain in the preview is too noisy").
constant float DATA_FLICKER = 1.2;
// The glow the packet's head carries, so a transfer reads moving at a sidebar's size, and its
// radius in column widths; and how many things in view a transfer looks among before it draws
// nothing.
constant float DATA_HALO = 0.3;
// The fourteen transfers' brightness, trails, heads and glows alike (Cyrus, 2026-11-17: "make the
// rain slightly dimmer in the graphic").
constant float DATA_LIGHT = 0.8;
constant float DATA_HALO_R = 0.8;
constant float DATA_TRIES = 5.0;
constant float RAIN_COL_W = 0.22;
// The subway (Cyrus, 2026-11-17: "a subway train ... it should appear dimmer because it runs
// underground and it should be obvious to the user that the train is underground. The subway train
// should move faster and without stop"): a tunnel under the road, its walls SUB_HALF to either side
// and its roof SUB_ROOF below the street, its floor SUB_FLOOR below; a train of SUB_CARS cars, each
// SUB_CAR long, every SUB_PERIOD seconds, coming on and going away in turn at SUB_SPEED world
// units a second without a stop, seen through the ground at SUB_DIM: under the street's grid, the
// tunnel's walls dashed.
constant float SUB_HALF = 0.36;
constant float SUB_ROOF = 0.18;
constant float SUB_FLOOR = 0.62;
constant float SUB_TRAIN_TOP = 0.24;
constant float SUB_TRAIN_HALF = 0.24;
constant int SUB_CARS = 5;
constant float SUB_CAR = 1.1;
constant float SUB_GAP = 0.08;
constant float SUB_SPEED = 4.0;
constant float SUB_PERIOD = 14.0;
constant float SUB_DIM = 0.3;
// The train's outline is drawn in hidden lines, as a drawing shows what lies under a surface:
// dashes SUB_DASH world units long along it, travelling with it.
constant float SUB_DASH = 0.22;

float hash2(float2 c) {
    uint ux = uint(int(c.x) + 1048576);
    uint uy = uint(int(c.y) + 1048576);
    return hash01(ux * 73856093u ^ uy * 19349663u);
}

float vnoise(float2 q) {
    float2 i = floor(q);
    float2 f = fract(q);
    float2 w = f * f * (3.0 - 2.0 * f);
    float a = hash2(i), b = hash2(i + float2(1.0, 0.0)), c = hash2(i + float2(0.0, 1.0)), d = hash2(i + float2(1.0, 1.0));
    return mix(mix(a, b, w.x), mix(c, d, w.x), w.y);
}

float fbm(float2 q) {
    return 0.55 * vnoise(q) + 0.3 * vnoise(q * 2.03 + 7.1) + 0.15 * vnoise(q * 4.01 + 3.3);
}

// A box with its corners cut by ch: its signed distance, as sd_box's.
float sd_chamfer(float2 q, float2 hb, float ch) {
    float2 a = abs(q);
    return max(sd_box(q, hb), (a.x + a.y - (hb.x + hb.y - ch)) * 0.70710678);
}

// How far round r's outline p lies, clockwise from its top left, in points.
float perimeter_at(float2 p, float4 r) {
    float2 q = p - r.xy;
    float w = r.z, h = r.w;
    float dt = abs(q.y), dr = abs(q.x - w), db = abs(q.y - h), dl = abs(q.x);
    float m = min(min(dt, dr), min(db, dl));
    if (m == dt) { return clamp(q.x, 0.0, w); }
    if (m == dr) { return w + clamp(q.y, 0.0, h); }
    if (m == db) { return w + h + (w - clamp(q.x, 0.0, w)); }
    return 2.0 * w + h + (h - clamp(q.y, 0.0, h));
}

// A light of colour c and strength s laid over col: added on a dark ground, mixed on a light one.
float3 light_on(float3 col, float3 c, float s, float dark) {
    return dark > 0.5 ? min(col + c * s, 1.0) : mix(col, c, saturate(s));
}

// A line's light at distance d: a core of width w and a halo out to reach.
float line_light(float d, float w, float reach) {
    float core = 1.0 - smoothstep(w * 0.5, w * 0.5 + 0.8, d);
    float fall = max(0.0, 1.0 - d / reach);
    return core + fall * fall * 0.35;
}

// The skyline's height at x (points into the band, band h points tall) for a layer: buildings of
// the layer's width with hashed heights. layer 0 is the far one, taller and paler; 1 the near.
float tower_top(float x, float h, int layer) {
    float w = layer == 0 ? 14.0 : 10.0;
    float cell = floor((x + (layer == 0 ? 4.0 : 0.0)) / w);
    float r = hash2(float2(cell, float(layer) * 31.0 + 5.0));
    return layer == 0 ? h * (1.0 - (0.26 + 0.24 * r)) : h * (1.0 - (0.1 + 0.2 * r));
}

// The code city's towers: taller, the near ones up to three quarters of the band.
float code_top(float x, float h, int layer) {
    float w = layer == 0 ? 15.0 : 11.0;
    float cell = floor((x + (layer == 0 ? 5.0 : 0.0)) / w);
    float r = hash2(float2(cell, float(layer) * 53.0 + 11.0));
    return layer == 0 ? h * (1.0 - (0.5 + 0.35 * r)) : h * (1.0 - (0.28 + 0.42 * r));
}

// Whether a lit window is at q inside a building of the layer (3 by 4 point panes on a grid).
float window_lit(float2 q, int layer, float t) {
    float2 g = floor(q / float2(5.0, 6.0));
    float2 f = fract(q / float2(5.0, 6.0));
    if (f.x > 0.4 || f.y > 0.34) {
        return 0.0;
    }
    float r = hash2(g + float2(float(layer) * 97.0, 13.0));
    float lit = r < (layer == 0 ? 0.05 : 0.09) ? 1.0 : 0.0;
    // A few flicker on and off.
    if (r > 0.97) {
        lit = fract(t * 0.13 + r * 7.0) < 0.5 ? 1.0 : 0.0;
    }
    return lit;
}

// A landmark's half width at depth r (0 at its top, 1 at its foot): Tokyo Tower's flared lattice
// or the Skytree's slender spire, decks wider.
float tower_half(float r, int which) {
    if (which == 0) {
        float hw = 1.2 + 12.0 * pow(r, 1.9);
        if (abs(r - 0.42) < 0.025 || abs(r - 0.62) < 0.02) { hw += 2.5; }
        return hw;
    }
    float hw = 0.9 + 3.2 * pow(r, 1.3);
    if (abs(r - 0.22) < 0.02) { hw += 3.0; }
    if (abs(r - 0.34) < 0.015) { hw += 2.0; }
    return hw;
}

// The bolt's x at depth y for this strike (a jagged path of six legs from x0), in points.
float bolt_x(float y, float y0, float y1, float x0, uint key) {
    float legs = 6.0;
    float f = saturate((y - y0) / max(y1 - y0, 1.0)) * legs;
    float i = floor(f);
    float a = i < 0.5 ? 0.0 : (hash01(key + uint(i)) - 0.5) * 22.0;
    float b = (hash01(key + uint(i) + 1u) - 0.5) * 22.0;
    return x0 + mix(a, b, fract(f));
}

// A colour moved `by` of the way away from the text: toward black on a dark theme, white on a
// light one.
float3 away(constant ArkosInputs& u, float3 c, float by) {
    return mix(c, u.dark > 0.5 ? float3(0.0) : float3(1.0), by);
}

// A tone that should reach the screen as `want` behind a pane's paper over `bg`: drawn
// 1 / (1 - PAPER) times as far from the background, since the paper covers the rest.
float3 through(float3 bg, float3 want) {
    return saturate(bg + (want - bg) / (1.0 - PAPER));
}

// A pane's body as it should reach the screen, `at` points from its top left and `to_foot` above
// its foot: the band at its top (storm cloud in the city, a drizzle of code in the code) and two
// rows of skyline along its foot, the near one's windows lit (warm and pale in the city, green in
// the code); the rain is laid over it by the caller.
float3 pane_night(constant ArkosInputs& u, float2 at, float to_foot, float seed, float t, float3 green) {
    float3 bg = u.colors[ARKOS_TERMINAL_BG].rgb;
    float3 dim_c = u.colors[ARKOS_TEXT_DIM].rgb;
    bool city = u.scene < 0.5;
    float3 want = bg;
    if (at.y < BODY_TOP_H) {
        float f = 1.0 - at.y / BODY_TOP_H;
        f = f * f;
        want = away(u, bg, BODY_DUSK * f);
        if (city) {
            float c = fbm(float2((at.x + seed * 37.0 + t * 4.0) / 42.0, at.y / 11.0));
            want = mix(want, dim_c, smoothstep(0.45, 0.8, c) * 0.15 * f);
        } else {
            // Short streaks of code falling down out of the strip, each column at its own pace,
            // their bits in 3 point cells.
            float colx = floor(at.x / 5.0);
            float h = hash2(float2(colx, seed));
            float len = 8.0 + 10.0 * hash2(float2(colx, 4.0));
            float pos = fract(t * (0.25 + 0.5 * h) + h * 7.0) * (BODY_TOP_H + len);
            float behind = pos - at.y;
            float2 bit = floor(float2(at.x, at.y - t * 0.0) / 3.0);
            if (h < 0.7 && behind >= 0.0 && behind < len && fract(at.x / 5.0) < 0.6 && hash2(bit + float2(floor(t * 4.0), colx)) < 0.7) {
                want = mix(want, green, 0.3 * (1.0 - behind / len) * saturate(1.2 - at.y / BODY_TOP_H));
            }
        }
    }
    float x = at.x + seed * 53.0;
    if (to_foot < FOOT_FAR_H) {
        float qy = FOOT_FAR_H - to_foot;
        float top = city ? tower_top(x, FOOT_FAR_H, 0) : code_top(x, FOOT_FAR_H, 0);
        if (qy > top) {
            want = mix(bg, dim_c, FOOT_FAR);
            if (window_lit(float2(x, qy - top), 0, t) > 0.5) {
                want = mix(want, city ? dim_c : green, FOOT_WINDOW * 0.6);
            }
        }
    }
    if (to_foot < FOOT_NEAR_H) {
        float qy = FOOT_NEAR_H - to_foot;
        float top = city ? tower_top(x + 7.0, FOOT_NEAR_H, 1) : code_top(x + 7.0, FOOT_NEAR_H, 1);
        if (qy > top) {
            want = away(u, bg, FOOT_NEAR);
            float2 wq = float2(x + 7.0, qy - top);
            if (window_lit(wq, 1, t) > 0.5) {
                float3 lamp = city ? (hash2(floor(wq / float2(5.0, 6.0)) + 3.0) < 0.6 ? u.colors[ARKOS_ANSI + 3].rgb : dim_c) : green;
                want = mix(want, lamp, FOOT_WINDOW);
            }
        }
    }
    return want;
}

// The lamp on an active tab, over the tab's colour `tab` at p, the tab's rect r: a red paper
// lantern with dark ribs and caps and a warm glow that gutters, in the city; a rune of green bits
// that changes now and then, in the code. Returns `tab` where neither is.
float3 tab_lamp(constant ArkosInputs& u, float2 p, float4 r, float t, float seed, float3 green, float3 tab) {
    float2 q = p - float2(r.x + TAB_LAMP_IN, r.y + r.w * 0.5 + 0.5);
    if (u.scene < 0.5) {
        float3 red = u.colors[ARKOS_ANSI + 1].rgb;
        float gutter = 0.85 + 0.15 * sin(t * TAB_LAMP_FLICKERS * 6.2832 + seed * 3.0) * sin(t * 2.3 + seed);
        float e = length(q / float2(3.2, 4.2));
        if (abs(q.x) < 2.0 && abs(q.y) >= 4.0 && abs(q.y) < 5.4) {
            return mix(tab, float3(0.0), 0.55);
        }
        if (e < 1.0) {
            float3 c = red * (0.8 + 0.2 * gutter);
            if (fract((q.y + 4.2) / 2.1) < 0.26) {
                c = mix(c, float3(0.0), 0.35);
            }
            return mix(c, u.colors[ARKOS_ANSI + 3].rgb, (1.0 - e) * 0.45 * gutter);
        }
        return light_on(tab, red, max(0.0, 1.0 - (e - 1.0) / 1.3) * 0.3 * gutter, u.dark);
    }
    float2 b = floor((q + float2(2.4, 4.0)) / 1.6);
    if (b.x < 0.0 || b.x > 2.0 || b.y < 0.0 || b.y > 4.0) {
        return tab;
    }
    float slot = floor(t / 0.7);
    float on = hash01(uint(seed * 977.0) ^ (uint(slot) * 2654435761u) ^ (uint(b.y * 3.0 + b.x) * 83492791u)) < 0.55 ? 1.0 : 0.0;
    return mix(tab, mix(green, float3(1.0), 0.2), on);
}

// The bolt's envelope at t, 0 to BOLT_PEAK (a soft rise and a long fade), and its slot's key; 0 in the first
// cycle and between strikes.
float bolt_env(float t, thread uint& key) {
    float slot = floor(t / BOLT_CYCLE);
    key = uint(slot) * 2654435761u + 17u;
    float at = t - slot * BOLT_CYCLE - hash01(key) * (BOLT_CYCLE - BOLT_FOR - 1.0);
    if (slot < 1.0 || at < 0.0 || at >= BOLT_FOR) {
        return 0.0;
    }
    float fall = 1.0 - smoothstep(BOLT_RISE, BOLT_FOR, at);
    return BOLT_PEAK * smoothstep(0.0, BOLT_RISE, at) * fall * fall;
}

// One layer of clouds at p: `count` soft ragged clouds from seed, each of its own size and height
// between top and bottom and its own speed between the two given, wrapping round the sidebar
// (x0, w wide); how much of p they cover, overlapping clouds building up rather than hiding.
float cloud_layer(float2 p, float top, float bottom, float x0, float w, float t, int count, uint seed, float2 size_lo, float2 size_hi, float speed_lo, float speed_hi) {
    float cloud = 0.0;
    for (int i = 0; i < count; i++) {
        uint id = seed + uint(i) * 7919u;
        float2 half_size = mix(size_lo, size_hi, float2(hash01(id), hash01(id + 1u)));
        float y = mix(top + half_size.y * 0.5, bottom - half_size.y * 0.5, hash01(id + 2u));
        if (abs(p.y - y) > half_size.y * 1.6) {
            continue;
        }
        float speed = mix(speed_lo, speed_hi, hash01(id + 3u));
        // Its place across, wrapping: it leaves on the right and comes back on the left.
        float span = w + 2.0 * half_size.x;
        float x = x0 - half_size.x + fract(hash01(id + 4u) + t * speed / span) * span;
        float2 q = (p - float2(x, y)) / half_size;
        // A ragged, puffy edge: the outline pushed in and out by noise of its own.
        float rag = (vnoise((p - float2(x, y)) / 11.0 + float2(float(i) * 13.0 + float(seed % 97u), 5.0)) - 0.5) * 0.45;
        // Flatter below than above, as a storm cloud's base is.
        float d = length(q) + rag + (q.y > 0.0 ? q.y * 0.35 : 0.0);
        float one = smoothstep(1.0, 0.5, d);
        cloud = cloud + one * (1.0 - cloud);
    }
    return cloud;
}

// The storm clouds over the city at p (window points), between top and bottom (thinning out over
// CLOUD_FADE at the top and into the skyline at the foot), across the sidebar from x0, w wide, lit
// by a bolt's envelope env, over col. Three layers, so no sky shows between them (Cyrus,
// 2026-11-17: "so full that there's no blank space behind them"): an overcast over the whole
// band, a bank of large clouds drifting slowly over it, and lighter clouds drifting faster in
// front, every cloud at its own speed.
float3 storm_clouds(float2 p, float top, float bottom, float x0, float w, float t, float env, constant ArkosInputs& u, float3 col) {
    if (p.y < top || p.y > bottom) {
        return col;
    }
    float3 dim_c = u.colors[ARKOS_TEXT_DIM].rgb;
    float lit = 0.25 * env;
    float ends = saturate((p.y - top) / CLOUD_FADE) * saturate((bottom - p.y) / CLOUD_FADE);
    // The overcast: everywhere, its tone moved a little by slow noise.
    float tone = fbm(float2((p.x + t * 2.0) / 80.0, p.y / 26.0));
    float3 out = mix(col, mix(col, dim_c, 0.10 + 0.06 * tone + lit), ends * 0.95);
    // The bank behind.
    float back = cloud_layer(p, top, bottom, x0, w, t, CLOUD_BACK, 31u, float2(48.0, 14.0), float2(96.0, 26.0), CLOUD_SPEED_MIN, CLOUD_SPEED_MIN * 2.5);
    out = mix(out, mix(col, dim_c, 0.19 + lit), saturate(back) * ends * 0.9);
    // The clouds in front.
    float front = cloud_layer(p, top, bottom, x0, w, t, CLOUD_FRONT, 977u, float2(30.0, 9.0), float2(66.0, 18.0), CLOUD_SPEED_MIN * 2.5, CLOUD_SPEED_MAX);
    out = mix(out, mix(col, dim_c, 0.28 + lit), saturate(front) * ends * 0.9);
    return out;
}

// The city's scene at q (points into the band, band sz), over the sky colour sky: clouds, a bolt
// now and then, the far and near skyline with Tokyo Tower and the Skytree, lit windows, rain.
float3 city_scene(float2 q, float2 sz, float t, constant ArkosInputs& u, float3 green, float3 white_c, float3 sb) {
    float3 bg = u.colors[ARKOS_WINDOW_BG].rgb;
    // The sky: what is under it (sb), lit low by the city's glow.
    float glow = saturate(q.y / sz.y);
    float3 col = mix(sb, mix(sb, green, 0.14), glow * glow);
    // A bolt: its envelope this frame (a soft rise and a long fade), its slot's key and its path.
    // The clouds over the city are drawn over the whole sidebar's lower half (`storm_clouds`). The
    // sky glows a little toward the rain's colour while it lasts, never flashes white.
    uint key = 0u;
    float env = bolt_env(t, key);
    col = mix(col, mix(col, mix(green, white_c, 0.3), 0.3), env * 0.25 * (1.0 - glow));
    // The bolt, from the clouds to a rooftop, its path drawn in runes.
    if (env > 0.0) {
        float x0 = sz.x * (0.18 + 0.64 * hash01(key + 9u));
        float y0 = sz.y * 0.16;
        float y1 = tower_top(x0, sz.y, 1) - 2.0;
        if (q.y >= y0 && q.y <= y1) {
            float bx = bolt_x(q.y, y0, y1, x0, key + 20u);
            float d = abs(q.x - bx);
            // The bolt is a chain of runes: 5 by 7 bits of 1.2 points, a rune every 9 points
            // down its path, each with its own pattern, over a thin bright core and a glow.
            float s = 0.4 * (1.0 - smoothstep(0.3, 1.0, d)) + 0.35 * pow(max(0.0, 1.0 - d / 9.0), 2.0);
            float2 g = float2((q.x - bx + 3.0) / 1.2, fract(q.y / 9.0) * 9.0 / 1.2);
            float runes = 0.0;
            if (g.x >= 0.0 && g.x < 5.0 && g.y >= 0.5 && g.y < 7.5) {
                float2 b = floor(g - float2(0.0, 0.5));
                float2 f = fract(g - float2(0.0, 0.5));
                bool lit = hash2(float2(floor(q.y / 9.0) * 13.0 + b.x, b.y * 7.0 + float(key % 211u))) < 0.5;
                runes = lit && max(abs(f.x - 0.5), abs(f.y - 0.5)) < 0.42 ? 0.9 : 0.0;
            }
            col = light_on(col, mix(green, white_c, 0.35), (s + runes) * env, u.dark);
        }
    }
    // The far skyline, then the landmarks among it, then the near skyline.
    float3 far_c = mix(mix(sb, bg, 0.6), green, 0.05);
    float3 near_c = bg * 0.55;
    float win = 0.0;
    if (q.y > tower_top(q.x, sz.y, 0)) {
        col = far_c;
        win = window_lit(q, 0, t) * 0.6;
    }
    // Tokyo Tower, a little left of the middle, and the Skytree to the right.
    for (int which = 0; which < 2; which++) {
        float cx = sz.x * (which == 0 ? 0.3 : 0.78);
        float ttop = sz.y * (which == 0 ? 0.2 : 0.02);
        float r = (q.y - ttop) / (sz.y - ttop);
        if (r >= 0.0 && r <= 1.0) {
            float hw = tower_half(r, which);
            float dx = abs(q.x - cx);
            if (dx <= hw) {
                col = mix(far_c, near_c, 0.4);
                // Tokyo Tower's lattice in orange (the theme's red and yellow), lit; the
                // Skytree's edges pale, its decks in the focus colour.
                float3 orange = mix(u.colors[ARKOS_ANSI + 1].rgb, u.colors[ARKOS_ANSI + 3].rgb, 0.45);
                float edge = dx > hw - 0.9 ? 1.0 : 0.0;
                if (which == 0) {
                    float lattice = (fract((q.x - cx + q.y) / 4.0) < 0.25 || fract((cx - q.x + q.y) / 4.0) < 0.25) ? 0.55 : 0.0;
                    col = mix(col, orange, max(lattice, edge * 0.85));
                } else {
                    bool deck = abs(r - 0.22) < 0.02 || abs(r - 0.34) < 0.015;
                    col = mix(col, deck ? mix(white_c, u.colors[ARKOS_FOCUS_RING].rgb, 0.5) : mix(white_c, green, 0.4), deck ? 0.45 : edge * 0.7);
                }
                win = 0.0;
            }
            // The beacon on its top, blinking red.
            float bd = length(q - float2(cx, ttop - 1.5));
            if (bd < 2.2) {
                float on = fract(t * 0.7) < 0.5 ? 1.0 : 0.25;
                col = light_on(col, u.colors[ARKOS_ANSI + 1].rgb, (1.0 - bd / 2.2) * on, u.dark);
            }
        }
    }
    if (q.y > tower_top(q.x, sz.y, 1)) {
        col = near_c;
        win = window_lit(q, 1, t);
    }
    // A window lit, now and then a warm one.
    float3 warm = mix(u.colors[ARKOS_ANSI + 3].rgb, white_c, 0.3);
    float3 win_c = hash2(floor(q / float2(5.0, 6.0)) + 3.0) < 0.3 ? warm : mix(green, white_c, 0.45);
    col = mix(col, win_c, win * 0.85);
    // The rain, slanting a little, over all of it.
    float rx = q.x + q.y * 0.18;
    float colx = floor(rx / 5.0);
    // Falling: a streak's place goes down the band as the clock goes on.
    float speed = 90.0 + 50.0 * hash2(float2(colx, 3.0));
    float pos = fract((q.y - t * speed) / 60.0 + hash2(float2(colx, 7.0))) * 60.0;
    float streak = (fract(rx / 5.0) < 0.2 && pos < 9.0 && hash2(float2(colx, 11.0)) < 0.5) ? (1.0 - pos / 9.0) : 0.0;
    col = mix(col, mix(u.colors[ARKOS_TEXT_DIM].rgb, white_c, 0.2), streak * 0.3);
    return col;
}

// The distance from p to the segment from a to b.
float seg_dist(float2 p, float2 a, float2 b) {
    float2 ab = b - a;
    float k = saturate(dot(p - a, ab) / max(dot(ab, ab), 1e-4));
    return length(p - a - ab * k);
}

float wrap_view(float z);
float road_car_at(float dir, float2 key, float t);
float road_bike_at(float dir, float2 key, float t);
float2 road_walker_at(float dir, float2 key, float t);
float4 road_crosser_at(float2 key, float side, float cw, float t);
float bike_swerve(float x, float zw, float t);
bool lot_built(int kx, int m);
bool lot_empty(int kx, int m);
bool lot_in_park(int kx, int m);

// What kind of thing a transfer joins the sky to, and the share of the slots each takes: a walker,
// a car, a cyclist, a tower's window or a food cart (its lot found among a few tries, else none).
constant float DATA_WALKER = 0.34;
constant float DATA_CAR = 0.56;
constant float DATA_BIKE = 0.66;
constant float DATA_WINDOW = 0.9;

// Transfer slot i at t: whether one is under way (x 1), how far through it is (y, 0 to 1 of its
// travel, on past 1 while its trail drains), its travel's seconds (z), and its seed (w).
float4 data_slot(int i, float t) {
    float fi = float(i);
    float period = mix(DATA_SLOT_MIN, DATA_SLOT_MAX, hash2(float2(fi, 3.0)));
    float off = hash2(float2(fi, 5.0)) * period;
    float k = floor((max(t, 0.0) + off) / period);
    float into = max(t, 0.0) + off - k * period;
    float seed = fi * 1013.0 + k;
    float travel = mix(DATA_TRAVEL_MIN, DATA_TRAVEL_MAX, hash2(float2(seed, 1.0)));
    // A transfer runs for its travel and its trail's draining, then the slot rests till its next.
    float busy = into < travel * 1.6 ? 1.0 : 0.0;
    return float4(busy, into / travel, travel, seed);
}

// The thing a transfer with this seed joins at t: across (x, world), its depth in view (y), its
// height (z, camera heights: where the stream meets it), and whether it is there (w 1). A building
// or a cart is chosen among the lots in view when the transfer began (t0) and held after, coming on
// with the street, so a transfer never jumps from one building to another.
float4 data_end(float seed, float t, float t0) {
    float run = t * GRID_SPEED * GRID_Z;
    float c = hash2(float2(seed, 2.0));
    float side = hash2(float2(seed, 6.0)) < 0.5 ? -1.0 : 1.0;
    float pick = hash2(float2(seed, 7.0));
    if (c < DATA_WALKER) {
        float2 key = float2(floor(pick * float(PEOPLE)), side > 0.0 ? 17.0 + 61.0 : 61.0);
        float dir = hash2(key + 1.0) < 0.5 ? 1.0 : -1.0;
        float2 at = road_walker_at(dir, key, t);
        float edge_x = float(ROAD_CELLS) * GRID_X + 0.06;
        float x = side * edge_x + side * (hash2(key + 7.0) - 0.22) * WALKER_SPREAD;
        return float4(x, wrap_view(at.x - run), PERSON_H, 1.0);
    }
    if (c < DATA_CAR) {
        float2 key = float2(floor(pick * float(ROAD_CARS)), side > 0.0 ? 13.0 + 31.0 : 31.0);
        float z = wrap_view(road_car_at(side, key, t) - run);
        return float4(side * 0.5 * float(ROAD_CELLS) * GRID_X, z, 0.1, 1.0);
    }
    if (c < DATA_BIKE) {
        float2 key = float2(floor(pick * float(ROAD_BIKES)), side > 0.0 ? 13.0 + 141.0 : 141.0);
        float z = wrap_view(road_bike_at(side, key, t) - run);
        return float4(side * BIKE_LANE * float(ROAD_CELLS) * GRID_X, z, 0.2, 1.0);
    }
    // A lot in the nearer part of the view when the transfer began: a tower's roof, or a food cart.
    bool cart = c >= DATA_WINDOW;
    int far_row = int(floor((t0 * GRID_SPEED * GRID_Z + Z_FAR) / GRID_Z));
    int rows = int(ceil((Z_FAR - Z_NEAR) / GRID_Z));
    for (int n = 0; n < 8; n++) {
        float h1 = hash2(float2(seed, 11.0 + float(n)));
        float h2 = hash2(float2(seed, 31.0 + float(n)));
        int m = far_row - int(float(rows) * mix(0.45, 0.95, h1));
        int kx = side > 0.0 ? ROAD_CELLS + int(h2 * float(LANES)) : -ROAD_CELLS - 1 - int(h2 * float(LANES));
        if (((m % CROSS_EVERY) + CROSS_EVERY) % CROSS_EVERY == 0) {
            continue;
        }
        float zc = float(m) * GRID_Z - run;
        float2 cell = float2(float(kx), float(m));
        if (!cart && lot_built(kx, m)) {
            float th = 0.6 + (TOWER_MAX - 0.6) * pow(hash2(cell + float2(4.0, 7.0)), 1.6);
            float tx = (float(kx) + 0.5) * GRID_X;
            return float4(tx, zc + 0.5 * GRID_Z, th, 1.0);
        }
        if (cart && lot_empty(kx, m) && !lot_in_park(kx, m) && int(floor(hash2(cell + float2(29.0, 7.0)) * 3.0)) == 1) {
            return float4((float(kx) + 0.5) * GRID_X, zc + 0.5 * GRID_Z + 0.1, 0.22, 1.0);
        }
    }
    return float4(0.0);
}

// Many small transfers (Cyrus, 2026-11-17: "there should be many small ones so it feels like the
// matrix with everyones connected devices to the cloud", then "the uploading and downloading rain
// should be tied to people, cars, bicycles, or tops of buildings. And it should be a little slower.
// It looks random to me."). DRIZZLE_STREAMS slots, each running a transfer for DRIZZLE_ON of its
// cycle of DRIZZLE_PERIOD_MIN to DRIZZLE_PERIOD_MAX seconds between the sky and one thing it
// follows: a walker (to SMALL_WALKER of the picks), a car (to SMALL_CAR), a cyclist (to
// SMALL_BIKE) or a building's roof, the thing in the nearer view (within DRIZZLE_FAR of it) among
// DRIZZLE_TRIES picks, else the slot rests. Each is a column DRIZZLE_W points wide: a lit dash a
// cell, a bright head and DRIZZLE_TRAIL cells fading behind it, down onto the thing for a download
// and up out of it for an upload, with a flicker on the thing as it lands or leaves.
constant int DRIZZLE_STREAMS = 36;
constant float DRIZZLE_W = 3.0;
constant float DRIZZLE_PERIOD_MIN = 4.5;
constant float DRIZZLE_PERIOD_MAX = 9.0;
constant float DRIZZLE_ON = 0.75;
constant float DRIZZLE_TRIES = 1.0;
constant float DRIZZLE_FAR = 0.8;
constant float SMALL_WALKER = 0.45;
constant float SMALL_CAR = 0.65;
constant float SMALL_BIKE = 0.75;
constant float DRIZZLE_TRAIL = 9.0;
constant float DRIZZLE_LIGHT = 0.32;

// The thing small transfer `seed` joins at t, begun at t0, on the road's `side` (-1 left, 1 right):
// across (x, world), its depth in view (y), the height it is met at (z), and whether it is in the
// nearer view (w 1). A roof is chosen among the rows in view at t0 and held after.
float4 small_end(float seed, float side, float t, float t0) {
    float run = t * GRID_SPEED * GRID_Z;
    float c = hash2(float2(seed, 2.0));
    float pick = hash2(float2(seed, 7.0));
    float road = float(ROAD_CELLS) * GRID_X;
    float4 e = float4(0.0);
    if (c < SMALL_WALKER) {
        float2 key = float2(floor(pick * float(PEOPLE)), side > 0.0 ? 17.0 + 61.0 : 61.0);
        float dir = hash2(key + 1.0) < 0.5 ? 1.0 : -1.0;
        float2 at = road_walker_at(dir, key, t);
        float x = side * (road + 0.06) + side * (hash2(key + 7.0) - 0.22) * WALKER_SPREAD;
        e = float4(x, wrap_view(at.x - run), PERSON_H, 1.0);
    } else if (c < SMALL_CAR) {
        float2 key = float2(floor(pick * float(ROAD_CARS)), side > 0.0 ? 13.0 + 31.0 : 31.0);
        e = float4(side * 0.5 * road, wrap_view(road_car_at(side, key, t) - run), 0.1, 1.0);
    } else if (c < SMALL_BIKE) {
        float2 key = float2(floor(pick * float(ROAD_BIKES)), side > 0.0 ? 13.0 + 141.0 : 141.0);
        e = float4(side * BIKE_LANE * road, wrap_view(road_bike_at(side, key, t) - run), 0.2, 1.0);
    } else {
        float m = floor((t0 * GRID_SPEED * GRID_Z + Z_NEAR + 1.5 + pick * (Z_FAR * DRIZZLE_FAR - 2.0)) / GRID_Z);
        int mi = int(m);
        int kx = side > 0.0 ? ROAD_CELLS + int(hash2(float2(seed, 9.0)) * 2.0) : -ROAD_CELLS - 1 - int(hash2(float2(seed, 9.0)) * 2.0);
        bool cross_row = ((mi % CROSS_EVERY) + CROSS_EVERY) % CROSS_EVERY == 0;
        if (cross_row || !lot_built(kx, mi)) {
            return float4(0.0);
        }
        float2 cell = float2(float(kx), m);
        float th = 0.6 + (TOWER_MAX - 0.6) * pow(hash2(cell + float2(4.0, 7.0)), 1.6);
        e = float4((float(kx) + 0.5) * GRID_X, m * GRID_Z - run + 0.5 * GRID_Z, th, 1.0);
    }
    e.w = (e.y > Z_NEAR + 0.4 && e.y < Z_FAR * DRIZZLE_FAR) ? 1.0 : 0.0;
    return e;
}

// The small transfers at p over col; top, hz, vx and f as data_rain's.
float3 data_drizzle(float2 p, float top, float hz, float vx, float f, float t, float3 green, float3 white_c, float3 col, constant ArkosInputs& u) {
    float3 head_c = mix(green, white_c, 0.55);
    float fade_top = saturate((p.y - top) / max((hz - top) * 0.4, 1.0));
    float ch = DRIZZLE_W * 1.5;
    for (int i = 0; i < DRIZZLE_STREAMS; i++) {
        float fi = float(i);
        float period = mix(DRIZZLE_PERIOD_MIN, DRIZZLE_PERIOD_MAX, hash2(float2(fi, 47.0)));
        float off = hash2(float2(fi, 49.0)) * period;
        float k = floor((max(t, 0.0) + off) / period);
        float into = max(t, 0.0) + off - k * period;
        float run = period * DRIZZLE_ON;
        if (into > run) {
            continue;
        }
        float seed = fi * 733.0 + k;
        // Its side of the road, for all its tries: a thing on one side is seen on that side of the
        // vanishing point, so a pixel on the other side passes the slot by without working out
        // where its thing is.
        float side = hash2(float2(seed, 6.0)) < 0.5 ? -1.0 : 1.0;
        if ((p.x - vx) * side < -DRIZZLE_W * 2.0) {
            continue;
        }
        float4 e = float4(0.0);
        for (float n = 0.0; n < DRIZZLE_TRIES; n += 1.0) {
            e = small_end(seed * 4.0 + n, side, t, t - into);
            if (e.w > 0.5) {
                break;
            }
        }
        if (e.w < 0.5) {
            continue;
        }
        float sx = vx + e.x * f / e.y;
        if (abs(p.x - sx) > DRIZZLE_W * 2.0) {
            continue;
        }
        float y_end = hz + f * (1.0 - e.z) / e.y;
        if (p.y > y_end + DRIZZLE_W * 2.0 || y_end <= top) {
            continue;
        }
        float len = max((y_end - top) / ch, 1.0);
        float r = (y_end - p.y) / ch;
        bool up = hash2(float2(seed, 3.0)) < 0.5;
        float travelled = (into / run) * (len + DRIZZLE_TRAIL);
        float head = up ? travelled : len - travelled;
        // The flicker on the thing as a download lands on it or an upload leaves it.
        float meet = up ? (1.0 - smoothstep(0.0, 1.5, travelled)) : smoothstep(len - 1.5, len, travelled) * (1.0 - smoothstep(len + 1.0, len + 3.0, travelled));
        float g = exp(-dot(p - float2(sx, y_end), p - float2(sx, y_end)) / (2.0 * DRIZZLE_W * DRIZZLE_W));
        col = light_on(col, head_c, g * meet * 0.5, u.dark);
        if (r < 0.0 || r > len || abs(p.x - sx) > DRIZZLE_W * 0.5) {
            continue;
        }
        float behind = up ? head - r : r - head;
        if (behind < 0.0 || behind > DRIZZLE_TRAIL) {
            continue;
        }
        // A lit dash a cell, changing as slowly as the glyphs.
        float2 fq = float2((p.x - sx) / DRIZZLE_W + 0.5, fract(r));
        float2 d = abs(fq - 0.5);
        float dash = 1.0 - smoothstep(0.22, 0.34, max(d.x * 1.4, d.y));
        dash *= hash2(float2(seed, floor(r)) * 3.0 + floor(t / DATA_FLICKER)) < 0.85 ? 1.0 : 0.3;
        float trail = 1.0 - behind / DRIZZLE_TRAIL;
        bool at_head = behind < 1.0;
        col = mix(col, at_head ? head_c : green, saturate(dash * trail * fade_top * DRIZZLE_LIGHT * (at_head ? 1.6 : 1.0)));
    }
    return col;
}

// The city's data at p (window points): every transfer under way, its column from the sky's top
// (top, where it fades in) to the thing it joins, over col. vx and hz are the vanishing point
// across and the horizon; f the scene's focal length, as the city's.
float3 data_rain(float2 p, float top, float hz, float vx, float f, float t, float3 green, float3 white_c, float3 col, constant ArkosInputs& u) {
    if (p.y < top) {
        return col;
    }
    col = data_drizzle(p, top, hz, vx, f, t, green, white_c, col, u);
    float fade_top = saturate((p.y - top) / max((hz - top) * 0.4, 1.0));
    float3 head_c = mix(green, white_c, 0.55);
    for (int i = 0; i < DATA_STREAMS; i++) {
        float4 slot = data_slot(i, t);
        if (slot.x < 0.5) {
            continue;
        }
        // A thing in the nearer part of the view: out of it, the transfer tries another or two.
        float4 e = float4(0.0);
        for (float n = 0.0; n < DATA_TRIES; n += 1.0) {
            e = data_end(slot.w * 4.0 + n, t, t - slot.y * slot.z);
            if (e.w > 0.5 && e.y > Z_NEAR + 0.4 && e.y < Z_FAR * 0.45) {
                break;
            }
            e.w = 0.0;
        }
        if (e.w < 0.5) {
            continue;
        }
        float sx = vx + e.x * f / e.y;
        float cw = clamp(RAIN_COL_W * f / e.y, 4.0, 9.0);
        if (abs(p.x - sx) > cw * 1.8) {
            continue;
        }
        float y_end = hz + f * (1.0 - e.z) / e.y;
        if (p.y > y_end + cw * 1.8) {
            continue;
        }
        float seen = (1.0 - smoothstep(Z_FAR * 0.32, Z_FAR * 0.45, e.y)) * smoothstep(Z_NEAR + 0.4, Z_NEAR + 1.0, e.y);
        // Its cells, counted up from the thing: a download's head comes down them, an upload's goes up.
        float ch = cw * 1.35;
        float len = max((y_end - top) / ch, 1.0);
        bool up = hash2(float2(slot.w, 3.0)) < 0.5;
        float travelled = slot.y * (len + DATA_TRAIL);
        float head = up ? travelled : len - travelled;
        float r = (y_end - p.y) / ch;
        // The flicker on the thing as a download lands on it or an upload leaves it.
        float meet = up ? (1.0 - smoothstep(0.0, 1.5, travelled)) : smoothstep(len - 1.5, len, travelled) * (1.0 - smoothstep(len + 2.0, len + 4.0, travelled));
        float glow = exp(-dot(p - float2(sx, y_end), p - float2(sx, y_end)) / (2.0 * cw * cw));
        col = light_on(col, head_c, glow * meet * 0.6 * seen * DATA_LIGHT, u.dark);
        // The packet's glow, round its head while the head is on the channel.
        if (head >= 0.0 && head <= len) {
            float2 hp = float2(sx, y_end - head * ch);
            float hr = cw * DATA_HALO_R;
            float halo = exp(-dot(p - hp, p - hp) / (2.0 * hr * hr));
            col = light_on(col, head_c, halo * DATA_HALO * seen * fade_top * DATA_LIGHT, u.dark);
        }
        if (r < 0.0 || r > len || abs(p.x - sx) > cw * 0.5) {
            continue;
        }
        float behind = up ? head - r : r - head;
        // The whole channel shows faintly while the transfer runs, so the packet reads as going to
        // or from its thing; the packet runs bright along it.
        float channel = DATA_CHANNEL * smoothstep(0.0, 0.08, slot.y) * (1.0 - smoothstep(1.35, 1.6, slot.y));
        bool in_packet = behind >= 0.0 && behind <= DATA_TRAIL;
        if (!in_packet && channel <= 0.0) {
            continue;
        }
        // A glyph in each cell, changing now and then, as the rain's.
        float2 fq = float2((p.x - sx) / cw + 0.5, fract(r));
        float cell_y = floor(r);
        float2 bit = floor(float2(fq.x * 5.0, fq.y * 7.0));
        float on = (bit.x >= 1.0 && bit.x <= 3.0 && bit.y >= 1.0 && bit.y <= 5.0 && hash2(float2(slot.w, cell_y) * 7.0 + bit + floor(t / DATA_FLICKER)) < 0.62) ? 1.0 : 0.0;
        // Too narrow for a glyph's bits to show (a far thing, a small pane): each cell a soft lit
        // dash instead, flickering as the glyphs change.
        if (cw < 7.0) {
            float2 d = abs(fq - 0.5);
            float dash = 1.0 - smoothstep(0.28, 0.4, max(d.x * 1.1, d.y));
            on = dash * (hash2(float2(slot.w, cell_y) * 3.0 + floor(t / DATA_FLICKER)) < 0.8 ? 1.0 : 0.35);
        }
        float trail = in_packet ? 1.0 - behind / DATA_TRAIL : 0.0;
        float lit = on * max(trail, channel) * seen * fade_top * DATA_LIGHT;
        bool at_head = in_packet && behind < 1.0;
        col = mix(col, at_head ? head_c : green, lit * (at_head ? 0.9 : 0.6));
    }
    return col;
}

// A person's light at p, standing at screen point foot, px_h points tall, its walk's phase from
// its key h at t (still while not `walking`): a head, a body, two arms and two legs that swing; far off (under 4 points), a
// dot of light at its height.
float person_light(float2 p, float2 foot, float px_h, float t, float h, bool walking) {
    if (p.y > foot.y + 1.0 || p.y < foot.y - px_h - 2.0 || abs(p.x - foot.x) > px_h * 0.4 + 2.0) {
        return 0.0;
    }
    float d;
    if (px_h < 4.0) {
        d = length(p - float2(foot.x, foot.y - px_h * 0.6)) - 0.4;
    } else {
        float2 uv = float2(p.x - foot.x, foot.y - p.y) / px_h;
        // A stride and an arm swing wide enough to read at a few points tall.
        // Standing (waiting at a corner), the legs together and the arms down.
        float swing = walking ? sin(t * STEPS * 6.2832 + h * 6.2832) * 0.17 : 0.03;
        float head = length(uv - float2(0.0, 0.88)) - 0.12;
        float body = seg_dist(uv, float2(0.0, 0.45), float2(0.0, 0.7)) - 0.085;
        float legs = min(seg_dist(uv, float2(0.0, 0.42), float2(swing, 0.0)), seg_dist(uv, float2(0.0, 0.42), float2(-swing, 0.0))) - 0.045;
        float arms = min(seg_dist(uv, float2(0.0, 0.68), float2(-swing * 0.9, 0.4)), seg_dist(uv, float2(0.0, 0.68), float2(swing * 0.9, 0.4))) - 0.035;
        d = min(min(head, body), min(legs, arms)) * px_h;
    }
    return 1.0 - smoothstep(0.0, 1.0, d);
}

// A depth in view brought into the city's wrapped span, Z_NEAR on: whole blocks at a time, so a
// stop line or a corner stays one.
float wrap_view(float z) {
    float span = WRAP_BLOCKS * float(CROSS_EVERY) * GRID_Z;
    return Z_NEAR + (z - Z_NEAR) - floor((z - Z_NEAR) / span) * span;
}

// Where a road car is along the road, in the world (camera heights from the city's origin), at
// t: it waits at a stop line, goes on the road's green, one or two blocks at its own pace with a
// gentle start and stop, and comes to the stop line there after the light has turned, clear of
// every intersection it crossed while it was green. `dir` is +1 going away, -1 coming on.
float road_car_at(float dir, float2 key, float t) {
    float block = float(CROSS_EVERY) * GRID_Z;
    // Its block among the city's WRAP_BLOCKS and its place in that block's queue, for good: the
    // lane's cars fill every block's queue alike, the first at the line, the others a car's length
    // apart behind it.
    float b = fmod(key.x, WRAP_BLOCKS);
    float place = floor(key.x / WRAP_BLOCKS);
    float k = floor(t / SIG_CYCLE);
    // The platoon's pace this green, the same for every car in it; each car moves off a moment
    // after the one ahead, so the gaps only open while they go and close back to the queue's.
    float dur = mix(6.5, 8.5, hash2(float2(b * 7.0 + key.y, k)));
    float x = saturate((t - k * SIG_CYCLE - place * QUEUE_START) / dur);
    float j = b + dir * k;
    float stop_line = (dir > 0.0 ? j * block - STOP_BACK : j * block + GRID_Z + STOP_BACK) - dir * place * QUEUE_GAP;
    return stop_line + dir * block * x * x * (3.0 - 2.0 * x);
}

float grid_line(float phase);

// Whether the city's cell (kx across, m along) has a tower: four in five of the lots beside the
// road do; the road and the cross streets have none.
bool lot_built(int kx, int m) {
    return hash2(float2(float(kx), float(m)) + float2(17.0, 3.0)) <= TOWER_ON;
}

// Whether a cell is a lot (beside the road, not on a cross street) with no tower.
bool lot_empty(int kx, int m) {
    bool road = kx >= -ROAD_CELLS && kx < ROAD_CELLS;
    bool out = kx < -(ROAD_CELLS + LANES) || kx >= ROAD_CELLS + LANES;
    bool cross = ((m % CROSS_EVERY) + CROSS_EVERY) % CROSS_EVERY == 0;
    return !road && !out && !cross && !lot_built(kx, m);
}

// Whether an empty lot is part of open ground big enough for a park: it touches another empty
// lot across or along (Cyrus, 2026-11-17: "if the blank space is large enough, I'd like
// something like a park").
bool lot_in_park(int kx, int m) {
    return lot_empty(kx - 1, m) || lot_empty(kx + 1, m) || lot_empty(kx, m - 1) || lot_empty(kx, m + 1);
}

// A park on the lot (kx, m), whose near edge is zc in view, at p over col: its grass (the ground
// tinted and hatched), PARK_TREES trees (a trunk and a canopy of light) and a lamp post lit warm,
// farthest first; f, cx and yh are the scene's projection.
float3 park_at(float2 p, int kx, int m, float zc, float cx, float yh, float f, float t, constant ArkosInputs& u, float3 green, float3 white_c, float3 col) {
    float x0 = float(kx) * GRID_X;
    float2 cell = float2(float(kx), float(m));
    float seen_far = 1.0 - smoothstep(Z_FAR * 0.55, Z_FAR, zc);
    // The grass: where p is on the ground inside the lot.
    if (p.y > yh) {
        float zg = f / (p.y - yh);
        float xg = (p.x - cx) * zg / f;
        if (zg > zc && zg < zc + GRID_Z && xg > x0 && xg < x0 + GRID_X && zg > Z_NEAR) {
            col = mix(col, mix(col, green, 0.35), 0.25 * seen_far);
            col = light_on(col, green, grid_line((xg + zg) / 0.12) * 0.08 * seen_far, u.dark);
        }
    }
    // Its trees and its lamp, farthest first (the larger depth drawn first).
    for (int i = PARK_TREES; i >= 0; i--) {
        float2 key = cell * 3.0 + float2(float(i), 5.0);
        float tz = zc + mix(0.2, 0.8, (float(i) + hash2(key)) / float(PARK_TREES + 1)) * GRID_Z;
        float tx = x0 + mix(0.2, 0.8, hash2(key + 1.0)) * GRID_X;
        if (tz < Z_NEAR) {
            continue;
        }
        float seen = (1.0 - smoothstep(Z_FAR * 0.55, Z_FAR, tz)) * smoothstep(Z_NEAR, Z_NEAR + 0.6, tz);
        float2 foot = float2(cx + tx * f / tz, yh + f / tz);
        float s = f / tz;
        if (i == PARK_TREES) {
            // The lamp post: a thin pole and a warm lamp.
            float2 lamp = float2(foot.x, yh + f * (1.0 - 0.32) / tz);
            float pole = seg_dist(p, foot, lamp);
            col = light_on(col, green, (1.0 - smoothstep(0.0, 0.7, pole)) * 0.3 * seen, u.dark);
            col = light_on(col, u.colors[ARKOS_ANSI + 3].rgb, (1.0 - smoothstep(0.02 * s, 0.07 * s + 1.0, length(p - lamp))) * 0.8 * seen, u.dark);
            continue;
        }
        float h = mix(0.32, 0.5, hash2(key + 2.0));
        float r = mix(0.14, 0.22, hash2(key + 3.0)) * s;
        float2 crown = float2(foot.x, yh + f * (1.0 - h) / tz);
        float trunk = seg_dist(p, foot, float2(foot.x, crown.y + r * 0.6));
        float d = length((p - crown) / float2(1.0, 1.15));
        // A canopy of light: a soft dark fill hiding what is behind, its rim lit, leaves in it.
        if (d < r) {
            col = mix(col, u.colors[ARKOS_WINDOW_BG].rgb * 0.8, 0.7 * seen);
            float leaves = hash2(floor((p - crown) / max(s * 0.04, 1.0)) + cell * 7.0) < 0.3 ? 0.35 : 0.0;
            col = light_on(col, green, leaves * seen, u.dark);
        }
        float rim = 1.0 - smoothstep(0.0, 1.0, abs(d - r));
        col = light_on(col, mix(green, white_c, 0.1), (rim * 0.7 + (1.0 - smoothstep(0.0, 0.8, trunk)) * 0.45) * seen, u.dark);
    }
    return col;
}

float person_light(float2 p, float2 foot, float px_h, float t, float h, bool walking);
float car_light(float2 p, float2 s, float r);

// A lone empty lot (too small for a park; Cyrus, 2026-11-17, chose these three): a subway
// entrance, a food cart or a basketball court, by the lot. Its near edge is zc in view; f, cx and
// yh are the scene's projection. Things in it are placed in the world (camera heights) and drawn
// farthest first.
float3 small_lot_at(float2 p, int kx, int m, float zc, float cx, float yh, float f, float t, constant ArkosInputs& u, float3 green, float3 white_c, float3 col) {
    float2 cell = float2(float(kx), float(m));
    float x0 = float(kx) * GRID_X;
    float xm = x0 + 0.5 * GRID_X;
    float zm = zc + 0.5 * GRID_Z;
    if (zc + GRID_Z < Z_NEAR) {
        return col;
    }
    float seen = (1.0 - smoothstep(Z_FAR * 0.55, Z_FAR, zm)) * smoothstep(Z_NEAR, Z_NEAR + 0.6, zm);
    float3 bg = u.colors[ARKOS_WINDOW_BG].rgb;
    float3 person_c = mix(green, white_c, 0.3);
    // The ground under p, if p is on it inside the lot: its place across and along.
    float zg = p.y > yh ? f / (p.y - yh) : -1.0;
    float xg = (p.x - cx) * zg / f;
    bool on_lot = zg > max(zc, Z_NEAR) && zg < zc + GRID_Z && xg > x0 && xg < x0 + GRID_X;
    // A point in the world (x across, height y, depth z) on the screen.
    #define AT(x, y, z) float2(cx + (x) * f / (z), yh + f * (1.0 - (y)) / (z))
    int kind = int(floor(hash2(cell + float2(29.0, 7.0)) * 3.0));
    if (kind == 0) {
        // A subway entrance: a stairwell going down, a railing round it, two green globes at its
        // head, and people going down into it and coming up out of it.
        float hx = 0.17, hz = 0.32;
        // The stair's side nearest the road is its head, where the globes stand.
        float side = kx < 0 ? 1.0 : -1.0;
        float sx = xm + side * 0.12;
        if (on_lot && abs(xg - sx) < hx && abs(zg - zm) < hz) {
            // The hole, its steps running down away from the head.
            float depth = (zg - (zm - hz)) / (2.0 * hz);
            col = mix(col, bg * 0.3, 0.85 * seen);
            col = light_on(col, green, grid_line(zg / 0.08) * 0.35 * (1.0 - depth) * seen, u.dark);
        }
        // The railing: a rail at waist height along three sides of the hole.
        float rail = 0.12;
        float rd = min(min(seg_dist(p, AT(sx - hx, rail, zm - hz), AT(sx - hx, rail, zm + hz)), seg_dist(p, AT(sx + hx, rail, zm - hz), AT(sx + hx, rail, zm + hz))), seg_dist(p, AT(sx - hx, rail, zm + hz), AT(sx + hx, rail, zm + hz)));
        col = light_on(col, green, (1.0 - smoothstep(0.0, 0.8, rd)) * 0.55 * seen, u.dark);
        // The globes, on short posts at the head's corners.
        for (int g = 0; g < 2; g++) {
            float gx = sx + (g == 0 ? -hx : hx);
            float2 foot = AT(gx, 0.0, zm - hz);
            float2 globe = AT(gx, 0.26, zm - hz);
            col = light_on(col, green, (1.0 - smoothstep(0.0, 0.7, seg_dist(p, foot, globe))) * 0.4 * seen, u.dark);
            col = light_on(col, mix(green, white_c, 0.45), car_light(p, globe, clamp(0.045 * f / (zm - hz), 0.8, 3.0)) * seen, u.dark);
        }
        // People: one going down, one coming up, each in its own turn.
        for (int i = 0; i < 2; i++) {
            float ph = fract(t / 7.0 + hash2(cell + float2(float(i), 41.0)));
            bool down = i == 0;
            // On the way to the head (or away from it), then on the stair, sinking (or rising).
            float a = down ? ph : 1.0 - ph;
            float along = a < 0.45 ? 0.0 : (a - 0.45) / 0.55;
            float wz = zm - hz - 0.25 + (a < 0.45 ? a / 0.45 * 0.25 : along * 2.0 * hz + 0.25);
            float sink = along * 0.22;
            float fade = 1.0 - smoothstep(0.6, 1.0, along);
            if (wz < Z_NEAR) {
                continue;
            }
            float2 foot = AT(sx, -sink, wz);
            col = light_on(col, person_c, person_light(p, foot, PERSON_H * f / wz, t, hash2(cell + float(i)), true) * 0.85 * seen * fade, u.dark);
        }
    } else if (kind == 1) {
        // A food cart under a striped umbrella, a queue at it: now and then the first walks off
        // with their food and the rest step up.
        float wz = zm + 0.1;
        float cw = CART_W;
        float3 warm = u.colors[ARKOS_ANSI + 3].rgb;
        // The queue at the cart's road side, running along the sidewalk (beside the road, the
        // road's own sidewalk; further out, inside the lot), farthest first.
        float side = kx < 0 ? 1.0 : -1.0;
        float ph = fract(t / 4.0 + hash2(cell + float2(3.0, 9.0)));
        float step = smoothstep(0.75, 1.0, ph);
        for (int i = 3; i >= 0; i--) {
            float leave = i == 0 ? step : 0.0;
            float qx = xm + side * (CART_W + CART_FRONT);
            float qz = wz - 0.05 + (float(i) - step) * CART_GAP - leave * 0.5;
            if (qz < Z_NEAR) {
                continue;
            }
            float2 foot = AT(qx, 0.0, qz);
            col = light_on(col, person_c, person_light(p, foot, PERSON_H * f / qz, t, hash2(cell + float(i) + 20.0), i == 0 ? step > 0.0 : step > 0.0 && step < 1.0) * 0.85 * seen * (1.0 - smoothstep(0.6, 1.0, leave)), u.dark);
        }
        // The cart: a box with its lit front, on two wheels.
        float2 c0 = AT(xm - cw, 0.2, wz), c1 = AT(xm + cw, 0.04, wz);
        bool in_cart = p.x > c0.x && p.x < c1.x && p.y > c0.y && p.y < c1.y;
        if (in_cart) {
            col = mix(col, bg * 0.6, 0.8 * seen);
            col = light_on(col, warm, (p.y < mix(c0.y, c1.y, 0.45) ? 0.45 : 0.15) * seen, u.dark);
        }
        float box = min(min(abs(p.x - c0.x), abs(p.x - c1.x)), min(abs(p.y - c0.y), abs(p.y - c1.y)));
        if (p.x > c0.x - 1.0 && p.x < c1.x + 1.0 && p.y > c0.y - 1.0 && p.y < c1.y + 1.0) {
            col = light_on(col, green, (1.0 - smoothstep(0.0, 0.8, box)) * 0.6 * seen, u.dark);
        }
        for (int w = 0; w < 2; w++) {
            float2 wheel = AT(xm + (w == 0 ? -0.1 : 0.1), 0.03, wz);
            col = light_on(col, green, (1.0 - smoothstep(0.0, 0.8, abs(length(p - wheel) - 0.03 * f / wz))) * 0.5 * seen, u.dark);
        }
        // The umbrella: a pole and a striped dome over the cart.
        float2 top = AT(xm, 0.42, wz);
        col = light_on(col, green, (1.0 - smoothstep(0.0, 0.7, seg_dist(p, AT(xm, 0.2, wz), top))) * 0.4 * seen, u.dark);
        float2 dq = (p - top) / (float2(0.24, 0.1) * f / wz);
        if (dq.y > 0.0 && dq.y < 1.0 && length(dq) < 1.0) {
            float stripe = fract(atan2(dq.y, dq.x) / 3.14159 * 5.0) < 0.5 ? 1.0 : 0.0;
            col = mix(col, mix(warm, white_c, stripe * 0.6) * 0.75, 0.75 * seen);
        }
    } else {
        // A basketball court: its lines on the ground, a fence rail round it, a hoop at each end
        // and players running up and down it.
        float hx = 0.36, hz = 0.7;
        if (on_lot && abs(xg - xm) < hx + 0.02 && abs(zg - zm) < hz + 0.02) {
            float2 c = float2(xg - xm, zg - zm);
            float w = max(fwidth(zg), 0.004) * 1.2;
            float lines = 0.0;
            lines = max(lines, 1.0 - smoothstep(0.0, w, abs(max(abs(c.x) - hx, abs(c.y) - hz))));
            lines = max(lines, 1.0 - smoothstep(0.0, w, abs(c.y)));
            lines = max(lines, 1.0 - smoothstep(0.0, w, abs(length(c) - 0.12)));
            float key = max(abs(c.x) - 0.1, abs(abs(c.y) - hz + 0.15) - 0.15);
            lines = max(lines, 1.0 - smoothstep(0.0, w, abs(key)));
            col = mix(col, mix(col, green, 0.15), 0.3 * seen);
            col = light_on(col, white_c, lines * 0.35 * seen, u.dark);
        }
        // The fence's top rail along its four sides.
        float fr = 0.22, fx = hx + 0.04, fz = hz + 0.04;
        float fence = min(min(seg_dist(p, AT(xm - fx, fr, max(zm - fz, Z_NEAR)), AT(xm - fx, fr, zm + fz)), seg_dist(p, AT(xm + fx, fr, max(zm - fz, Z_NEAR)), AT(xm + fx, fr, zm + fz))), seg_dist(p, AT(xm - fx, fr, zm + fz), AT(xm + fx, fr, zm + fz)));
        col = light_on(col, green, (1.0 - smoothstep(0.0, 0.8, fence)) * 0.35 * seen, u.dark);
        // The players, running up and down and side to side, far first.
        for (int i = 3; i >= 0; i--) {
            float hp = hash2(cell + float2(float(i), 51.0));
            float pz = zm + sin(t * (0.6 + 0.4 * hp) + hp * 6.28) * (hz - 0.15) * (0.6 + 0.4 * hash2(cell + float2(float(i), 52.0)));
            float px = xm + sin(t * (0.9 + 0.5 * hp) + hp * 3.0) * (hx - 0.08);
            if (pz < Z_NEAR) {
                continue;
            }
            float2 foot = AT(px, 0.0, pz);
            col = light_on(col, person_c, person_light(p, foot, PERSON_H * f / pz, t * 1.6, hp, true) * 0.85 * seen, u.dark);
        }
        // The hoops: a pole, a backboard and an orange rim at each end.
        for (int e = 0; e < 2; e++) {
            float ez = zm + (e == 0 ? -hz : hz);
            if (ez < Z_NEAR) {
                continue;
            }
            float2 foot = AT(xm, 0.0, ez);
            float2 board = AT(xm, 0.36, ez);
            col = light_on(col, green, (1.0 - smoothstep(0.0, 0.7, seg_dist(p, foot, board))) * 0.45 * seen, u.dark);
            float2 bh = float2(0.07, 0.05) * f / ez;
            float bd = max(abs(p.x - board.x) - bh.x, abs(p.y - board.y) - bh.y);
            col = light_on(col, white_c, (1.0 - smoothstep(0.0, 0.8, abs(bd))) * 0.45 * seen, u.dark);
            float2 rim = AT(xm, 0.3, ez + (e == 0 ? 0.05 : -0.05));
            float rr = length((p - rim) / float2(1.0, 0.4)) - 0.035 * f / ez;
            col = light_on(col, mix(u.colors[ARKOS_ANSI + 1].rgb, u.colors[ARKOS_ANSI + 3].rgb, 0.5), (1.0 - smoothstep(0.0, 0.8, abs(rr))) * 0.7 * seen, u.dark);
        }
    }
    #undef AT
    return col;
}

// Where a road cyclist is along the road at t: as a road car (`road_car_at`), but one block a
// green, slower, and setting off a little later; clear of the intersection it crosses before the
// light turns.
float road_bike_at(float dir, float2 key, float t) {
    float block = float(CROSS_EVERY) * GRID_Z;
    // Never stopping: a block a cycle, or two for a messenger, reaching each stop line on the
    // road's red just after the last cross car has crossed its lane, and riding on through.
    float blocks = hash2(key + 5.0) < 0.5 ? 2.0 : 1.0;
    float speed = blocks * block / SIG_CYCLE;
    float phase = BIKE_GO + BIKE_SPREAD * hash2(key + 2.0);
    float j0 = floor(hash2(key + 3.0) * 8.0);
    float stop0 = dir > 0.0 ? j0 * block - STOP_BACK : j0 * block + GRID_Z + STOP_BACK;
    return stop0 + dir * speed * (t - phase);
}

// One of a cross street's people crossing the road, keyed by its street, sidewalk and place in
// the crowd, at t: across (x), its place across the crosswalk's width (y), whether it is walking
// (z), and how much of it shows (w, 0 while it is not out). Out of a doorway, to its own spot in
// the group at the corner, up to CROSS_WAIT_DEPTH back from it, over the road, and into a doorway
// on the far side. Most step off over the first CROSS_SPREAD seconds of their green, each at its
// own moment, so the crowd strings out over the road instead of crossing as one; while it waits
// the group spreads up to WAIT_ALONG along the road's sidewalk beside the crosswalk (`side` is -1
// for the crosswalk before a cross street, +1 for the one after it, the way the sidewalk runs on),
// and each comes onto the crosswalk in its first CROSS_MERGE steps (Cyrus,
// 2026-11-18: "walkers are still stacking on top of eachother when they are making horizontal
// street crossings"). A LATE_SHARE of them come late and hurry over as their light is about to
// turn, in front of the road's traffic about to go, stepping off in the LATE_SPREAD seconds before
// LATE_GO, the latest that keeps them out of every road car's and cyclist's lane (Cyrus,
// 2026-11-18: "pedestrians should start cross in front of oncoming traffic if they know the light
// is about to turn red for them").
// How long a person crossing the road, its feet at z along the road, must stand at the edge of the
// bike lane of the way s it is about to step into at time ta, to be in that lane for dur seconds
// with no road cyclist through its crosswalk meanwhile: it waits for one to pass, by NEAR_MISS
// (Cyrus, 2026-11-18, of the cyclists and the people crossing: "Both, a near miss"). A cyclist
// keeps one pace, so it is at z every span / speed seconds, one of its images in the view each time.
float bike_wait(float s, float z, float ta, float dur) {
    float block = float(CROSS_EVERY) * GRID_Z;
    float span = WRAP_BLOCKS * block;
    float lane = s > 0.0 ? 1.0 : 0.0;
    float wait = 0.0;
    for (int look = 0; look < 2; look++) {
        for (int j = 0; j < ROAD_BIKES; j++) {
            float2 key = float2(float(j), lane * 13.0 + 141.0);
            float blocks = hash2(key + 5.0) < 0.5 ? 2.0 : 1.0;
            float speed = blocks * block / SIG_CYCLE;
            float phase = BIKE_GO + BIKE_SPREAD * hash2(key + 2.0);
            float j0 = floor(hash2(key + 3.0) * 8.0);
            float stop0 = s > 0.0 ? j0 * block - STOP_BACK : j0 * block + GRID_Z + STOP_BACK;
            float base = phase + (z - stop0) / (s * speed);
            float per = span / speed;
            float half_t = (0.09 + PERSON_BODY + NEAR_MISS) / speed;
            float from = ta + wait;
            float tk = base + ceil((from - half_t - base) / per) * per;
            if (tk - half_t < from + dur) {
                wait = tk + half_t - ta;
            }
        }
    }
    return wait;
}

// How far a road cyclist in its lane at x, at zw along the road, eases aside at t: away from
// anyone crossing who stands at the edge of its lane on the crosswalk it is going through, by up to
// SWERVE, the other half of a near miss (Cyrus, 2026-11-18: "Both, a near miss").
float bike_swerve(float x, float zw, float t) {
    float block = float(CROSS_EVERY) * GRID_Z;
    float base = floor(zw / block) * block;
    float off = 0.0;
    for (int k = 0; k < 2; k++) {
        float street = base + float(k) * block;
        for (int w = 0; w < 2; w++) {
            float cw = w == 0 ? street - WALK_AT : street + GRID_Z + WALK_AT;
            float near = 1.0 - smoothstep(0.1, 0.45, abs(zw - cw));
            if (near <= 0.0) {
                continue;
            }
            int m = int(round(street / GRID_Z));
            for (int j = 0; j < CROSS_PEOPLE; j++) {
                float4 at = road_crosser_at(float2(float(m) * 3.0 + float(w), float(j) + 71.0), w == 0 ? -1.0 : 1.0, cw, t);
                if (at.w > 0.0 && at.z <= 0.0 && abs(at.x - x) < 0.25) {
                    off += sign(x - at.x) * SWERVE * near;
                }
            }
        }
    }
    return clamp(off, -SWERVE, SWERVE);
}

float4 road_crosser_at(float2 key, float side, float cw, float t) {
    float edge = float(ROAD_CELLS) * GRID_X;
    float dir = hash2(key + 1.0) < 0.5 ? 1.0 : -1.0;
    bool late = hash2(key + 11.0) < LATE_SHARE;
    float v = late ? mix(0.7, 0.85, hash2(key + 2.0)) : mix(0.46, 0.6, hash2(key + 2.0));
    // A period of two cycles, starting on one or the other.
    float tt = t + floor(hash2(key + 3.0) * 2.0) * SIG_CYCLE;
    float tau = tt - floor(tt / (2.0 * SIG_CYCLE)) * 2.0 * SIG_CYCLE;
    float back = CROSS_WAIT_DEPTH * hash2(key + 9.0);
    // One on the green is over the road CROSS_CLEAR before the road's green, however slow.
    float over = (2.0 * (edge + 0.08) + back) / v;
    float tc = late ? LATE_GO - LATE_SPREAD * hash2(key + 7.0) : CROSS_FIRST + min(CROSS_SPREAD * hash2(key + 7.0), SIG_CYCLE - CROSS_CLEAR - over - CROSS_FIRST);
    float wait = late ? 0.3 * hash2(key + 4.0) : 2.0 * hash2(key + 4.0);
    float start = -dir * (edge + 0.08 + back);
    float la = min(mix(0.5, 3.0, hash2(key + 5.0)), v * (tc - wait));
    float t0 = tc - wait - la / v;
    float lb = mix(0.5, 3.0, hash2(key + 6.0));
    float reach = 2.0 * (edge + 0.08) + back + lb;
    float across = (hash2(key + 8.0) - 0.5) * WALK_W * CROSS_ACROSS;
    float waiting = across + side * WAIT_ALONG * hash2(key + 12.0);
    if (tau < t0) {
        return float4(0.0, waiting, 0.0, 0.0);
    }
    if (tau < tc) {
        float wx = start - dir * la + dir * v * min(tau - t0, la / v);
        return float4(wx, waiting, tau - t0 < la / v ? 1.0 : 0.0, smoothstep(t0, t0 + 0.4, tau));
    }
    // Over the road, standing at the edge of each bike lane while a cyclist goes through.
    float bx = BIKE_LANE * float(ROAD_CELLS) * GRID_X;
    float hw = 0.03 + PERSON_BODY + NEAR_MISS;
    float d1 = edge + 0.08 + back - bx - hw;
    float d2 = edge + 0.08 + back + bx - hw;
    float step_off = t - (tau - tc);
    float e = tau - tc;
    // A wait only matters once it has reached the lane: before then none is worked out.
    float w1 = e < d1 / v ? 0.0 : bike_wait(-dir, cw + across, step_off + d1 / v, 2.0 * hw / v);
    float w2 = e < d2 / v + w1 ? 0.0 : bike_wait(dir, cw + across, step_off + d2 / v + w1, 2.0 * hw / v);
    float d;
    bool walking = true;
    if (e < d1 / v) {
        d = v * e;
    } else if (e < d1 / v + w1) {
        d = d1;
        walking = false;
    } else if (e < d2 / v + w1) {
        d = v * (e - w1);
    } else if (e < d2 / v + w1 + w2) {
        d = d2;
        walking = false;
    } else {
        d = v * (e - w1 - w2);
    }
    if (d > reach) {
        return float4(0.0, across, 0.0, 0.0);
    }
    float onto = mix(waiting, across, smoothstep(0.0, CROSS_MERGE, d));
    return float4(start + dir * d, onto, walking ? 1.0 : 0.0, 1.0 - smoothstep(reach - 0.2, reach, d));
}

// Where a walker on the road's sidewalk is along the road, in the world, at t, and whether it is
// walking (x) or standing at a corner (y 0). It crosses a cross street only on the road's green,
// stepping off the curb early enough to be over before the green ends: a brisk one (most) a block
// every cycle, so it hardly stands, a slow one a block every two, standing a few seconds at a
// corner for the next green. Each steps off at its own time in the green, so a block's walkers
// spread out along it.
float2 road_walker_at(float dir, float2 key, float t) {
    float block = float(CROSS_EVERY) * GRID_Z;
    float cross = GRID_Z + 2.0 * WALK_AT;
    bool brisk = hash2(key + 2.0) < 0.65;
    float v = brisk ? mix(0.54, 0.6, hash2(key + 6.0)) : mix(0.31, 0.36, hash2(key + 6.0));
    float laps = brisk ? 1.0 : 2.0;
    float period = laps * SIG_CYCLE;
    float late = min(ROAD_GREEN - AMBER, ROAD_GREEN - cross / v);
    // Most cross against the light, timed to the cross street's last car; the rest wait.
    bool jay = hash2(key + 8.0) < JAYWALK;
    // Where it waits: a jaywalker anywhere in the group gathered up to WAIT_DEPTH back from the
    // curb, so they stand apart (Cyrus, 2026-11-18: "people waiting to cross the street should be
    // more spread-out in a group"); and where it steps off, edged up to CREEP_KEEP of that and as
    // much as GUTTER off the curb, in the CREEP_T seconds before (Cyrus, 2026-11-18: "The crowds
    // also get closer to the car as the last few are passing because they are impatient"). One
    // who keeps the law waits at the curb, its crossing timed from there.
    float back = jay ? WAIT_DEPTH * hash2(key + 9.0) : 0.0;
    float front = jay ? back * CREEP_KEEP - GUTTER * hash2(key + 10.0) : 0.0;
    float c = floor(hash2(key + 3.0) * laps) * SIG_CYCLE + (jay ? WALK_GO + JAY_SPREAD * hash2(key + 4.0) : late * hash2(key + 4.0));
    float k = floor((t - c) / period);
    float s = t - c - k * period;
    float j = floor(hash2(key + 5.0) * 8.0) + dir * k;
    float corner = dir > 0.0 ? j * block - WALK_AT : j * block + GRID_Z + WALK_AT;
    float dist = block - back + front;
    float walk = dist / v;
    float creep = max(walk, period - CREEP_T);
    float x;
    bool moving;
    if (s < walk) {
        x = v * s;
        moving = true;
    } else if (s < creep || back <= front) {
        x = dist;
        moving = false;
    } else {
        x = dist + (back - front) * (s - creep) / (period - creep);
        moving = true;
    }
    return float2(corner - dir * front + dir * x, moving ? 1.0 : 0.0);
}

// A stroller on the road's sidewalk (Cyrus, 2026-11-17: the people "distributed so the pedestrian
// traffic looks fuller"): out of a door, or round the corner from a cross street's sidewalk, and
// along its block into another door or round the far corner, never into a street,
// so it keeps no light and comes and goes at its own times, filling the sidewalk between the
// corners. Where it is along the road, in the world, at t (x), and how much of it shows (y), as
// it steps out of a door and in at another.
float2 road_stroller_at(float dir, float2 key, float t) {
    float block = float(CROSS_EVERY) * GRID_Z;
    float span = block - GRID_Z - 2.0 * WALK_AT;
    float v = mix(0.36, 0.58, hash2(key + 2.0));
    float period = span / v + 1.0;
    float s = t + hash2(key + 3.0) * period;
    float k = floor(s / period);
    float2 lap = key + float2(k * 7.0, 0.0);
    float len = span * mix(0.4, 1.0, hash2(lap + 5.0));
    float a = hash2(lap + 4.0) * (span - len);
    float dur = len / v;
    float into = s - k * period;
    float x = min(into, dur) * v;
    float shown = smoothstep(0.0, 0.4, into) * (1.0 - smoothstep(dur - 0.4, dur, into));
    float j = floor(hash2(lap + 6.0) * 8.0);
    float base = j * block + GRID_Z + WALK_AT;
    return float2(base + (dir > 0.0 ? a + x : a + len - x), shown);
}

// A cyclist's light at p seen end on (riding toward or away from the camera), at screen point
// foot, s points to a camera height: the bike a thin upright, the rider seated on it, pedalling
// while it moves, and its light (headlight or tail light, the caller's colour) at the bike's
// height. Returns the rider and bike's light and the lamp's apart.
float2 bike_end_on(float2 p, float2 foot, float s, float t, float h, bool moving) {
    float frame = seg_dist(p, foot, foot - float2(0.0, 0.1 * s));
    float rider = person_light(p, foot - float2(0.0, 0.06 * s), 0.13 * s, t * 1.4, h, moving);
    float lamp = car_light(p, foot - float2(0.0, 0.08 * s), clamp(0.03 * s, 0.6, 2.4));
    return float2(max(rider, 1.0 - smoothstep(0.0, 0.8, frame)), lamp);
}

// A cyclist's light at p seen side on (riding across, dir +1 to the right), its wheels' contact
// midpoint at screen point mid, s points to a camera height: two wheels, a frame, the rider
// seated and pedalling, a headlight ahead and a tail light behind (returned apart).
float3 bike_side_on(float2 p, float2 mid, float s, float dir, float t, float h) {
    float r = 0.05 * s;
    float2 w0 = mid + float2(-0.09 * s, -r);
    float2 w1 = mid + float2(0.09 * s, -r);
    float wheels = min(abs(length(p - w0) - r), abs(length(p - w1) - r));
    float2 seat = mid + float2(-dir * 0.02 * s, -0.12 * s);
    float2 bars = mid + float2(dir * 0.07 * s, -0.14 * s);
    float frame = min(min(seg_dist(p, w0, seat), seg_dist(p, seat, bars)), min(seg_dist(p, bars, w1), seg_dist(p, w0, mid - float2(0.0, r))));
    float body = min(wheels, frame);
    float rider = person_light(p, seat + float2(0.0, 0.02 * s), 0.12 * s, t * 1.4, h, true);
    float head = car_light(p, mid + float2(dir * 0.12 * s, -0.1 * s), clamp(0.025 * s, 0.6, 2.0));
    float tail = car_light(p, mid + float2(-dir * 0.12 * s, -0.09 * s), clamp(0.025 * s, 0.6, 2.0));
    return float3(max(rider, 1.0 - smoothstep(0.0, 0.8, body)), head, tail);
}

// A car's light at screen point s, r points across, at p: a soft round glow.
float car_light(float2 p, float2 s, float r) {
    return 1.0 - smoothstep(r * 0.5, r * 1.6, length(p - s));
}

// A line's light at a phase (lines at whole numbers), drawn about a pixel wide whatever the
// phase's rate across the screen: `fwidth` keeps far lines fine and near ones crisp.
float grid_line(float phase) {
    float d = abs(fract(phase - 0.5) - 0.5) / max(fwidth(phase), 1e-4);
    return 1.0 - smoothstep(0.0, 1.2, d);
}

// The code scene: a digital horizon. On the horizon a line of light; below it a floor of grid
// lines in perspective, its rows coming toward the viewer, and on it a dense city of wireframe
// towers either side of a road, coming on with it: traffic on the road and the cross streets,
// lit windows, drones over the roofs and people walking the sidewalks. (The sky's code rain is
// drawn by the caller.) The clock's zero (Reduce Motion) holds it all still.
// The train at t: the depth of its front and of its back in view (x, y), and whether it runs (z 1).
// A pass comes on toward the viewer, the next goes away, and so on, at SUB_SPEED, never stopping.
float3 subway_at(float t) {
    float len = float(SUB_CARS) * SUB_CAR + float(SUB_CARS - 1) * SUB_GAP;
    float k = floor(max(t, 0.0) / SUB_PERIOD);
    float u = max(t, 0.0) - k * SUB_PERIOD;
    float span = Z_FAR + len - Z_NEAR + 2.0;
    if (u * SUB_SPEED > span) {
        return float3(0.0);
    }
    bool coming = fmod(k, 2.0) < 0.5;
    // Coming on: its front from beyond the far end toward the viewer; going away: from behind the
    // viewer out to the far end.
    float front = coming ? Z_FAR + 1.0 - u * SUB_SPEED : Z_NEAR - len - 1.0 + u * SUB_SPEED + len;
    float back = coming ? front + len : front - len;
    return float3(front, back, 1.0);
}

// The subway seen through the ground at q: the tunnel's dashed walls and the train's roof with its
// lit windows and its headlight, all at SUB_DIM, over col (the floor before its grid).
float3 subway_seen(float2 q, float yh, float cx, float f, float t, float3 green, float3 white_c, float3 col, constant ArkosInputs& u) {
    float dy = q.y - yh;
    if (dy <= 0.5) {
        return col;
    }
    float dim = SUB_DIM;
    // The tunnel's walls: lines along it at its roof's height, dashed as the ground hides them.
    for (int w = 0; w < 2; w++) {
        float depth = w == 0 ? SUB_ROOF : SUB_FLOOR;
        float z = f * (1.0 + depth) / dy;
        if (z < Z_NEAR || z > Z_FAR) {
            continue;
        }
        float xw = (q.x - cx) * z / f;
        float wx = max(fwidth(xw), 1e-5);
        float wall = 1.0 - smoothstep(wx, 2.0 * wx, abs(abs(xw) - SUB_HALF));
        float dash = step(0.45, fract(z / 0.5 + t * GRID_SPEED * GRID_Z / 0.5));
        col = light_on(col, green, wall * dash * dim * (w == 0 ? 0.7 : 0.45), u.dark);
    }
    float3 tr = subway_at(t);
    if (tr.z < 0.5) {
        return col;
    }
    // The train's roof: SUB_TRAIN_TOP below the street, SUB_TRAIN_HALF to either side.
    float z = f * (1.0 + SUB_TRAIN_TOP) / dy;
    float xw = (q.x - cx) * z / f;
    float near_end = min(tr.x, tr.y), far_end = max(tr.x, tr.y);
    if (z < near_end || z > far_end || abs(xw) > SUB_TRAIN_HALF || z < Z_NEAR) {
        // Its headlight, a soft glow ahead of its front in the tunnel.
        float ahead = (tr.x < tr.y) ? tr.x - z : z - tr.x;
        float beam = (ahead > 0.0 && ahead < 2.5 && abs(xw) < SUB_HALF) ? (1.0 - ahead / 2.5) * (1.0 - abs(xw) / SUB_HALF) : 0.0;
        return light_on(col, mix(green, white_c, 0.5), beam * dim * 0.6, u.dark);
    }
    float along = (z - near_end);
    float pitch = SUB_CAR + SUB_GAP;
    float in_car = fract(along / pitch) * pitch;
    if (in_car > SUB_CAR) {
        return col;
    }
    // The car's roof, a faint shadow; its outline in hidden lines, soft and dashed along the train
    // and across its ends, the dashes travelling with it; two rows of windows down its sides.
    float wx = max(fwidth(xw), 1e-5);
    float wz = max(fwidth(z), 1e-5);
    float sides = 1.0 - smoothstep(wx, 3.0 * wx, SUB_TRAIN_HALF - abs(xw));
    float ends = 1.0 - smoothstep(wz, 3.0 * wz, min(in_car, SUB_CAR - in_car));
    float along_dash = step(0.45, fract(along / SUB_DASH));
    float across_dash = step(0.45, fract((xw + SUB_TRAIN_HALF) / (SUB_DASH * 0.5)));
    float edge = max(sides * along_dash, ends * across_dash);
    col = mix(col, green * 0.35, 0.3 * dim);
    col = light_on(col, green, edge * dim, u.dark);
    float win = (abs(abs(xw) - SUB_TRAIN_HALF * 0.7) < SUB_TRAIN_HALF * 0.14 && fract(in_car / 0.16) < 0.6 && in_car > 0.08 && in_car < SUB_CAR - 0.08) ? 1.0 : 0.0;
    return light_on(col, mix(green, white_c, 0.6), win * dim * 0.9, u.dark);
}

float3 code_scene(float2 q, float2 sz, float t, constant ArkosInputs& u, float3 green, float3 white_c, float3 sb) {
    float3 bg = u.colors[ARKOS_WINDOW_BG].rgb;
    float3 col = sb;
    float yh = sz.y * HZ_AT;
    float cx = sz.x * 0.5;
    float f = sz.x * 0.9;
    // The sky's rain is drawn over the whole sidebar's lower half by the caller (`code_rain`).
    if (q.y >= yh) {
        // The floor: darker than the sky, its grid's rows coming on.
        float dy = q.y - yh;
        float z = f / max(dy, 0.5);
        float xw = (q.x - cx) * z / f;
        col = mix(col, bg * 0.7, 0.6);
        // The subway under the street, seen through the ground before the street's grid is laid
        // over it.
        col = subway_seen(q, yh, cx, f, t, green, white_c, col, u);
        float near = saturate(dy / (sz.y - yh));
        float lines = max(grid_line(z / GRID_Z + t * GRID_SPEED), grid_line(xw / GRID_X + 0.5));
        col = light_on(col, green, lines * (0.18 + 0.5 * near), u.dark);
        // The crosswalks: zebra stripes over the road beside each cross street, and over each
        // cross street beside the road.
        float blk = float(CROSS_EVERY) * GRID_Z;
        float zw = z + t * GRID_SPEED * GRID_Z;
        float local = zw - floor(zw / blk) * blk;
        float road_e = float(ROAD_CELLS) * GRID_X;
        bool over_road = abs(xw) < road_e && ((local > blk - WALK_AT - WALK_W * 0.5 && local < blk - WALK_AT + WALK_W * 0.5) || (local > GRID_Z + WALK_AT - WALK_W * 0.5 && local < GRID_Z + WALK_AT + WALK_W * 0.5));
        bool over_cross = local < GRID_Z && abs(abs(xw) - road_e - 0.08) < WALK_W * 0.5;
        float stripe = over_road ? grid_line(xw / 0.16) : (over_cross ? grid_line(local / 0.16) : 0.0);
        col = light_on(col, green, stripe * 0.25 * near, u.dark);
        // The sidewalks: along the road outside its edge, and along both sides of each cross street
        // outside the road, paved lighter than the floor, with joints, and a lit curb.
        float ax = abs(xw);
        float curb_x = road_e - SIDEWALK_IN;
        bool off_cross = local >= GRID_Z;
        bool by_road = ax > curb_x && ax < curb_x + SIDEWALK_W;
        bool by_cross = ax > curb_x && (local < GRID_Z + SIDEWALK_W || local > blk - SIDEWALK_W);
        if (off_cross && (by_road || by_cross)) {
            // A soft wash of the city's green: the paving stands up off the dark floor.
            col = light_on(col, green, SIDEWALK_FILL * 0.32 * (0.55 + 0.45 * near), u.dark);
            float joints = max(grid_line(local / 0.16), grid_line(ax / 0.16));
            col = light_on(col, green, joints * 0.12 * near, u.dark);
        }
        float wx = max(fwidth(xw), 1e-5);
        float wz = max(fwidth(local), 1e-5);
        float curb_road = (1.0 - smoothstep(wx, 2.5 * wx, abs(ax - curb_x))) * (off_cross ? 1.0 : 0.0);
        float curb_cross = (1.0 - smoothstep(wz, 2.5 * wz, min(abs(local - GRID_Z), abs(local - blk)))) * (ax > curb_x ? 1.0 : 0.0);
        col = light_on(col, green, max(curb_road, curb_cross) * SIDEWALK_CURB_LIGHT * (0.3 + 0.7 * near), u.dark);
    }
    // The horizon's line, and its glow either side.
    float hd = abs(q.y - yh);
    col = light_on(col, green, (1.0 - smoothstep(0.0, 1.2, hd)) * 0.6 + exp(-hd / 4.0) * 0.18, u.dark);
    // The road's traffic, keeping to the right: coming on in the left lane (headlights), going
    // away in the right (tail lights), each car stopping and going with the road's signal.
    float3 head_c = mix(green, white_c, 0.6);
    float3 tail_c = mix(u.colors[ARKOS_ANSI + 1].rgb, green, 0.15);
    float floor_v = GRID_SPEED * GRID_Z;
    float run = t * floor_v;
    float block = float(CROSS_EVERY) * GRID_Z;
    float sig = t - floor(t / SIG_CYCLE) * SIG_CYCLE;
    float cross_on = ROAD_GREEN + ALL_RED;
    // Only near the road at p's depth (its lights sit just above the floor) can a car light p.
    float road_z = q.y > yh ? f * 0.92 / (q.y - yh) : Z_FAR;
    bool by_road = q.y > yh && abs(q.x - cx) < 0.6 * f / road_z + 4.0;
    // A cyclist rides out by the curb and stands a fifth of a camera height tall: the band its
    // lane makes out from the vanishing point, its rider's height and its lights' reach included.
    float bike_off = abs(q.x - cx) / max(q.y - yh, 1.0);
    bool by_bike = q.y > yh && bike_off > 0.45 && bike_off < 1.2 + 4.0 / max(q.y - yh, 1.0);
    for (int lane = 0; lane < ((by_road || by_bike) ? 2 : 0); lane++) {
        float dir = lane == 0 ? -1.0 : 1.0;
        for (int j = 0; j < (by_road ? ROAD_CARS : 0); j++) {
            float z = wrap_view(road_car_at(dir, float2(float(j), float(lane) * 13.0 + 31.0), t) - run);
            float lx = dir * 0.5 * float(ROAD_CELLS) * GRID_X;
            float r = clamp(0.05 * f / z, 0.6, 3.0);
            float seen = (1.0 - smoothstep(Z_FAR * 0.55, Z_FAR, z)) * smoothstep(Z_NEAR, Z_NEAR + 0.6, z);
            float y = yh + f * (1.0 - 0.08) / z;
            float gap = 0.12 * f / z;
            float x = cx + lx * f / z;
            float l = max(car_light(q, float2(x - gap, y), r), car_light(q, float2(x + gap, y), r));
            col = light_on(col, lane == 0 ? head_c : tail_c, l * seen * 0.9, u.dark);
        }
        // The cyclists in this way's bike lane, by the curb.
        for (int j = 0; j < (by_bike ? ROAD_BIKES : 0); j++) {
            float2 key = float2(float(j), float(lane) * 13.0 + 141.0);
            float w = road_bike_at(dir, key, t);
            bool moving = abs(road_bike_at(dir, key, t + 0.05) - w) > 1e-4;
            float z = wrap_view(w - run);
            float seen = (1.0 - smoothstep(Z_FAR * 0.55, Z_FAR, z)) * smoothstep(Z_NEAR, Z_NEAR + 0.6, z);
            float sc = f / z;
            float bike_x = dir * BIKE_LANE * float(ROAD_CELLS) * GRID_X + bike_swerve(dir * BIKE_LANE * float(ROAD_CELLS) * GRID_X, z + run, t);
            float2 foot = float2(cx + bike_x * sc, yh + sc);
            float2 b = bike_end_on(q, foot, sc, t, hash2(key + 7.0), moving);
            col = light_on(col, mix(green, white_c, 0.3), b.x * 0.8 * seen, u.dark);
            col = light_on(col, lane == 0 ? head_c : tail_c, b.y * seen, u.dark);
        }
    }
    // The city, farthest row first so nearer towers hide farther ones: each cell's tower stands
    // on the floor, which comes on at GRID_SPEED rows a second, so the camera has come `run` into
    // it; a row m stands at depth m * GRID_Z - run.
    int far_row = int(floor((run + Z_FAR) / GRID_Z));
    int rows = int(ceil((Z_FAR - Z_NEAR) / GRID_Z)) + 1;
    // The farthest a tower can stand and still reach p: below the horizon its foot must be at or
    // below p, above it its top (at most TOWER_MAX tall) must reach up to p.
    float reach = q.y > yh ? f / (q.y - yh) : (q.y < yh ? f * (TOWER_MAX - 1.0) / (yh - q.y) : Z_FAR);
    for (int r = 0; r < rows; r++) {
        int m = far_row - r;
        float zc = float(m) * GRID_Z - run;
        if (zc - GRID_Z > reach || zc + GRID_Z < Z_NEAR) {
            continue;
        }
        // A cross street: no towers on this row; its signals at the corners, its cars keeping to
        // the right and waiting at their stop lines for their green, and its people crossing the
        // road on the crosswalks with that green.
        if (((m % CROSS_EVERY) + CROSS_EVERY) % CROSS_EVERY == 0) {
            float zs = zc + 0.5 * GRID_Z;
            if (zc + GRID_Z + WALK_AT < Z_NEAR || zc > Z_FAR) {
                continue;
            }
            float seen = (1.0 - smoothstep(Z_FAR * 0.55, Z_FAR, zs)) * smoothstep(Z_NEAR, Z_NEAR + 0.6, zs);
            float half_w = float(ROAD_CELLS + LANES) * GRID_X;
            float edge = float(ROAD_CELLS) * GRID_X;
            // The signals: on the near corners, the road's; on the far ones, the cross street's.
            float3 red_c = u.colors[ARKOS_ANSI + 1].rgb;
            float3 amber_c = u.colors[ARKOS_ANSI + 3].rgb;
            float3 road_sig = sig < ROAD_GREEN - AMBER ? green : (sig < ROAD_GREEN ? amber_c : red_c);
            float3 cross_sig = (sig >= cross_on && sig < cross_on + CROSS_GREEN - AMBER) ? green : ((sig >= cross_on && sig < cross_on + CROSS_GREEN) ? amber_c : red_c);
            for (int c = 0; c < 4; c++) {
                float pz = c < 2 ? zc - 0.3 : zc + GRID_Z + 0.3;
                if (pz < Z_NEAR) {
                    continue;
                }
                float px = cx + ((c % 2 == 0) ? -1.0 : 1.0) * (edge + 0.15) * f / pz;
                float2 foot = float2(px, yh + f / pz);
                float2 lamp = float2(px, yh + f * (1.0 - 0.45) / pz);
                float pole = seg_dist(q, foot, lamp);
                col = light_on(col, green, (1.0 - smoothstep(0.0, 0.8, pole)) * 0.3 * seen, u.dark);
                col = light_on(col, c < 2 ? road_sig : cross_sig, car_light(q, lamp, clamp(0.045 * f / pz, 0.7, 2.6)) * seen, u.dark);
            }
            float r2 = clamp(0.05 * f / zs, 0.6, 3.0);
            for (int dl = 0; dl < 2; dl++) {
                // Going right (+x) keeps to the near lane, going left to the far one.
                float dir = dl == 0 ? 1.0 : -1.0;
                float zl = zc + (dl == 0 ? 0.3 : 0.7) * GRID_Z;
                if (zl < Z_NEAR) {
                    continue;
                }
                float y = yh + f * (1.0 - 0.08) / zl;
                if (abs(q.y - y) > r2 * 2.0) {
                    continue;
                }
                for (int qn = 0; qn < CROSS_CARS; qn++) {
                    float2 key = float2(float(m) * 2.0 + float(dl), float(qn) + 91.0);
                    float v = mix(2.2, 3.2, hash2(key));
                    float arrive = mix(3.0, 6.0, hash2(key + 1.0));
                    // Queued at the stop line, the first car nearest it; they go in turn.
                    float stop_x = -dir * (edge + CROSS_STOP + float(qn) * 0.5);
                    float go = cross_on + float(qn) * 0.6;
                    float wx = sig < go ? stop_x - dir * v * max(arrive - sig, 0.0) : stop_x + dir * v * (sig - go);
                    if (abs(wx) > half_w) {
                        continue;
                    }
                    float2 front = float2(cx + (wx + dir * 0.14) * f / zl, y);
                    float2 back = float2(cx + (wx - dir * 0.14) * f / zl, y);
                    col = light_on(col, head_c, car_light(q, front, r2) * seen * 0.9, u.dark);
                    col = light_on(col, tail_c, car_light(q, back, r2) * seen * 0.9, u.dark);
                }
            }
            // Its cyclists, one each way in its bike lanes by the curbs: waiting at the stop line
            // ahead of the cars, going first on the cross streets' green.
            for (int dl = 0; dl < 2; dl++) {
                float dir = dl == 0 ? 1.0 : -1.0;
                float zb = zc + (dl == 0 ? 0.12 : 0.88) * GRID_Z;
                if (zb < Z_NEAR) {
                    continue;
                }
                float sc = f / zb;
                float y = yh + sc;
                if (q.y > y + 1.0 || q.y < y - 0.3 * sc - 2.0) {
                    continue;
                }
                float2 key = float2(float(m) * 2.0 + float(dl), 131.0);
                float v = mix(1.7, 2.2, hash2(key));
                float arrive = mix(4.0, 6.5, hash2(key + 1.0));
                float stop_x = -dir * (edge + CROSS_STOP);
                float go = CROSS_BIKE_GO;
                float wx = sig < go ? stop_x - dir * v * max(arrive - sig, 0.0) : stop_x + dir * v * (sig - go);
                if (abs(wx) > half_w) {
                    continue;
                }
                float3 b = bike_side_on(q, float2(cx + wx * sc, y), sc, dir, t, hash2(key + 2.0));
                col = light_on(col, mix(green, white_c, 0.3), b.x * 0.8 * seen, u.dark);
                col = light_on(col, head_c, b.y * seen, u.dark);
                col = light_on(col, tail_c, b.z * seen, u.dark);
            }
            // People on its near and far sidewalks (beside the crosswalks over the road): out of a
            // doorway, to the corner, waiting for the cross streets' green, over the road on the
            // crosswalk, and into a doorway on the far side; a crowd, each at its own pace.
            for (int w = 0; w < 2; w++) {
                float zw = w == 0 ? zc - WALK_AT : zc + GRID_Z + WALK_AT;
                float fy = yh + f / zw;
                float ph = PERSON_H * f / zw;
                if (zw < Z_NEAR || q.y > fy + 2.0 + WALK_W * f / zw || q.y < fy - ph - 3.0 - WALK_W * f / zw) {
                    continue;
                }
                for (int j = 0; j < CROSS_PEOPLE; j++) {
                    float2 key = float2(float(m) * 3.0 + float(w), float(j) + 71.0);
                    float4 at = road_crosser_at(key, w == 0 ? -1.0 : 1.0, float(m) * GRID_Z + (w == 0 ? -WALK_AT : GRID_Z + WALK_AT), t);
                    if (at.w <= 0.0) {
                        continue;
                    }
                    float zj = zw + at.y;
                    float2 foot = float2(cx + at.x * f / zj, yh + f / zj);
                    col = light_on(col, mix(green, white_c, 0.3), person_light(q, foot, PERSON_H * f / zj, t, hash2(key), at.z > 0.0) * 0.85 * seen * at.w, u.dark);
                }
            }
            continue;
        }
        // Only the lane p lies in at this row's depth and its two neighbours can cover it (a
        // tower's back face leans toward the road, never past the next lane).
        int lane = int(floor((q.x - cx) * max(zc, Z_NEAR) / f / GRID_X));
        for (int dk = -1; dk <= 1; dk++) {
            {
                int kx = lane + dk;
                bool road = kx >= -ROAD_CELLS && kx < ROAD_CELLS;
                if (road || kx < -(ROAD_CELLS + LANES) || kx >= ROAD_CELLS + LANES) {
                    continue;
                }
                float2 cell = float2(float(kx), float(m));
                if (!lot_built(kx, m)) {
                    // An empty lot: where it touches another, the open ground is a park.
                    if (lot_in_park(kx, m)) {
                        col = park_at(q, kx, m, zc, cx, yh, f, t, u, green, white_c, col);
                    } else {
                        col = small_lot_at(q, kx, m, zc, cx, yh, f, t, u, green, white_c, col);
                    }
                    continue;
                }
                float tw = GRID_X * (0.5 + 0.35 * hash2(cell + float2(5.0, 11.0)));
                float td = GRID_Z * (0.35 + 0.35 * hash2(cell + float2(9.0, 2.0)));
                float th = 0.6 + (TOWER_MAX - 0.6) * pow(hash2(cell + float2(4.0, 7.0)), 1.6);
                float tx = (float(kx) + 0.5) * GRID_X;
                float zf = zc + 0.5 * GRID_Z - 0.5 * td;
                if (zf < Z_NEAR || zf > Z_FAR) {
                    continue;
                }
                float base = yh + f / zf;
                float top = yh + f * (1.0 - th) / zf;
                float x0 = cx + (tx - tw * 0.5) * f / zf;
                float x1 = cx + (tx + tw * 0.5) * f / zf;
                // Its back face, its depth further: drawn so it reads as a box in perspective.
                float bz = zf + td;
                float btop = yh + f * (1.0 - th) / bz;
                float bx0 = cx + (tx - tw * 0.5) * f / bz;
                float bx1 = cx + (tx + tw * 0.5) * f / bz;
                if (q.x < min(x0, bx0) - 1.0 || q.x > max(x1, bx1) + 1.0 || q.y < min(top, btop) - 1.0 || q.y > base + 0.5) {
                    continue;
                }
                // Far towers fade up out of the horizon, near ones as they pass.
                float seen = (1.0 - smoothstep(Z_FAR * 0.55, Z_FAR, zf)) * smoothstep(Z_NEAR, Z_NEAR + 0.6, zf);
                bool inside = q.x > x0 && q.x < x1 && q.y > top && q.y < base;
                if (inside) {
                    col = mix(col, bg * 0.75, 0.9 * seen);
                }
                float edge = min(min(abs(q.x - x0), abs(q.x - x1)), abs(q.y - top));
                float floors = grid_line((base - q.y) / (f * 0.5 / zf));
                float s = (1.0 - smoothstep(0.0, 1.0, edge)) * 0.8 + (inside ? floors * 0.3 : 0.0);
                // Lit windows on the face, where a window is a few points wide: a few in each
                // tower, some switching on and off now and then.
                if (inside && 0.2 * f / zf > 3.0) {
                    float2 w = float2((q.x - x0) / (x1 - x0) * tw / 0.2, (base - q.y) / (base - top) * th / 0.25);
                    float2 wc = floor(w);
                    float2 wf = fract(w);
                    float lit = hash2(wc + cell * 13.0 + floor(t * 0.15 + hash2(wc + cell) * 3.0) * 0.37);
                    if (wf.x > 0.25 && wf.x < 0.75 && wf.y > 0.3 && wf.y < 0.75 && lit < 0.2) {
                        col = light_on(col, mix(green, white_c, 0.35), 0.5 * seen, u.dark);
                    }
                }
                if (!inside) {
                    // The top's back edge and the edges running back to it, and the back's
                    // upright on the side away from the road.
                    float outer_x = tx < 0.0 ? bx0 : bx1;
                    float back = min(seg_dist(q, float2(bx0, btop), float2(bx1, btop)), min(seg_dist(q, float2(x0, top), float2(bx0, btop)), seg_dist(q, float2(x1, top), float2(bx1, btop))));
                    back = min(back, seg_dist(q, float2(outer_x, btop), float2(outer_x, yh + f / bz)));
                    s = max(s, (1.0 - smoothstep(0.0, 1.0, back)) * 0.5);
                }
                col = light_on(col, mix(green, white_c, 0.15), s * seen, u.dark);
            }
        }
    }
    // Drones drifting over the roofs, each blinking.
    for (int i = 0; i < DRONES; i++) {
        float fi = float(i);
        float3 w = float3(sin(t * 0.21 + fi * 2.1) * 3.5, 2.6 + 0.6 * sin(t * 0.5 + fi), 7.0 + 4.0 * sin(t * 0.13 + fi * 1.7));
        float2 sp = float2(cx + w.x * f / w.z, yh + f * (1.0 - w.y) / w.z);
        float blink = fract(t * 0.9 + fi * 0.37) < 0.5 ? 1.0 : 0.35;
        col = light_on(col, i == 1 ? tail_c : head_c, car_light(q, sp, 1.2) * blink * 0.9, u.dark);
    }
    // People on the road's sidewalks: a crowd walking either way, each at its own pace, crossing
    // the cross streets on their crosswalks with the road's green and waiting at the corner for
    // it, so a crowd gathers there on the red. A person stands below the horizon with its feet on
    // its sidewalk's line, so only a pixel in the band beside that line can be on one.
    float walk_dy = q.y - yh;
    float edge_x = float(ROAD_CELLS) * GRID_X + 0.06;
    float off = abs(q.x - cx);
    float slack = WALK_W + 0.4 * PERSON_H;
    bool by_walk = walk_dy > 0.0 && off > (edge_x - slack) * walk_dy - 3.0 && off < (edge_x + slack) * walk_dy / (1.0 - PERSON_H) + 3.0;
    // Only the sidewalk on p's side of the road can have a person on p.
    int near_side = q.x < cx ? 0 : 1;
    for (int side = near_side; side < (by_walk ? near_side + 1 : near_side); side++) {
        float sx = (side == 0 ? -1.0 : 1.0) * edge_x;
        float out_x = side == 0 ? -1.0 : 1.0;
        for (int j = 0; j < PEOPLE; j++) {
            float2 key = float2(float(j), float(side) * 17.0 + 61.0);
            float h = hash2(key);
            float dir = hash2(key + 1.0) < 0.5 ? 1.0 : -1.0;
            float2 at = road_walker_at(dir, key, t);
            float z = wrap_view(at.x - run);
            float seen = (1.0 - smoothstep(Z_FAR * 0.55, Z_FAR, z)) * smoothstep(Z_NEAR, Z_NEAR + 0.6, z);
            float2 foot = float2(cx + (sx + out_x * (hash2(key + 7.0) - 0.22) * WALKER_SPREAD) * f / z, yh + f / z);
            col = light_on(col, mix(green, white_c, 0.3), person_light(q, foot, PERSON_H * f / z, t, h, at.y > 0.0) * 0.85 * seen, u.dark);
        }
        for (int j = 0; j < STROLLERS; j++) {
            float2 key = float2(float(j), float(side) * 17.0 + 211.0);
            float dir = hash2(key + 1.0) < 0.5 ? 1.0 : -1.0;
            float2 at = road_stroller_at(dir, key, t);
            if (at.y <= 0.0) {
                continue;
            }
            float z = wrap_view(at.x - run);
            float seen = (1.0 - smoothstep(Z_FAR * 0.55, Z_FAR, z)) * smoothstep(Z_NEAR, Z_NEAR + 0.6, z) * at.y;
            // Out of a door: nearer the buildings than the walkers keep.
            float2 foot = float2(cx + (sx + out_x * (0.1 + 0.6 * hash2(key + 7.0)) * WALK_W) * f / z, yh + f / z);
            col = light_on(col, mix(green, white_c, 0.3), person_light(q, foot, PERSON_H * f / z, t, hash2(key), true) * 0.85 * seen, u.dark);
        }
    }
    return col;
}

fragment float4 arkos_wash(ArkosVertex in [[stage_in]], constant ArkosInputs& u [[buffer(0)]]) {
    float2 p = in.uv * u.size;
    float k = saturate(u.intensity);

    float3 bg = u.colors[ARKOS_WINDOW_BG].rgb;
    float3 white_c = u.colors[ARKOS_ANSI + 15].rgb;
    float3 green = toward_accent(theme_green(u), u.colors[ARKOS_FOCUS_RING].rgb);

    // The rain; laid over the flat ground once the blocks say where the sidebar is.
    float4 drop = rain(p, u.time, green);

    // The blocks: the bars darkened a little, the highlights glowing, every reported edge live.
    int nb = min(int(u.block_count), 48);
    float dim = 0.0;
    float lift = 0.0;
    // How much of a tab bar covers p, for the extra rain laid on the tab strips, and how much of
    // a pane body, for the extra rain laid under the panes' paper.
    float tab = 0.0;
    float pane = 0.0;
    EdgeHit hit = no_edge();
    // For the world below: the sidebar, its focus and where its names end, and the focus of the
    // tab bar p is in.
    float4 side = float4(0.0);
    float side_focus = 0.0;
    float names_end = 0.0;
    float bar_focus = 0.0;
    float tab_hover = 0.0;
    // The focused pane whose frame p is in, if any: its focus, its frame's rect, its tab bar's
    // block and its body.
    float in_focus = 0.0;
    // The pane whose frame's shape p is in, if any (its own rect), for an active tab that starts
    // its bar and so reaches the frame.
    float4 shape_pane = float4(0.0);
    float4 in_rect = float4(0.0);
    int in_block = 0;
    float4 in_body = float4(0.0);
    for (int i = 0; i < nb; i++) {
        constant ArkosBlock& blk = u.blocks[i];
        float4 rect = blk.rect;
        int bkind = int(blk.kind);
        if (bkind == ARKOS_SIDEBAR_BLOCK) {
            side = rect;
            side_focus = saturate(blk.focus);
            names_end = side.y + side.w;
        } else if (bkind == ARKOS_SIDEBAR_FOOT_BLOCK) {
            names_end = rect.y;
        } else if (bkind == ARKOS_TAB_BAR_BLOCK && inside_box(p, rect)) {
            bar_focus = saturate(blk.focus);
        }
        float2 hb = rect.zw * 0.5;
        int kind = int(blk.kind);
        // The sidebar's ground reaches its frame, FRAME_OUT round its column and cut at the
        // frame's corners, as a pane's does (its column keeps a pane's margins).
        float d = kind == ARKOS_SIDEBAR_BLOCK ? sd_chamfer(p - (rect.xy + hb), hb + FRAME_OUT, FRAME_CHAMFER) : sd_box(p - (rect.xy + hb), hb);
        if (kind == ARKOS_TITLE_BAR_BLOCK || kind == ARKOS_TAB_BAR_BLOCK || kind == ARKOS_SIDEBAR_BLOCK) {
            float inside = 1.0 - smoothstep(-0.5, 0.5, d);
            float amount = kind == ARKOS_SIDEBAR_BLOCK ? 0.35 : (kind == ARKOS_TAB_BAR_BLOCK ? 0.0 : 0.42);
            dim = max(dim, inside * amount);
            if (kind == ARKOS_TAB_BAR_BLOCK) {
                tab = max(tab, inside);
            }
        } else if (kind == ARKOS_PANE_BODY_BLOCK) {
            pane = max(pane, 1.0 - smoothstep(-0.5, 0.5, d));
        } else if (kind == ARKOS_TAB_HIGHLIGHT_BLOCK) {
            // A hovered tab lightens the strip it is on (the world below).
            if (d < 0.0) {
                tab_hover = max(tab_hover, saturate(blk.value));
            }
        } else if (kind == ARKOS_ROW_HIGHLIGHT_BLOCK) {
            if (d < 6.0) {
                float s = HIGHLIGHT_STRENGTH[min(max(int(blk.ordinal), 0), 2)] * blk.value;
                float fill = (1.0 - smoothstep(-1.5, 1.0, d)) + 0.35 * (1.0 - smoothstep(0.0, 6.0, d));
                lift = max(lift, s * saturate(fill));
            }
        }
        // A pane's and the sidebar's own edges are their frames (the world below), not runes,
        // and the title bar's storm clouds meet the frames with no line of runes between them
        // (Cyrus, 2026-11-17: "I don't like the effect that divides the app bar and the panes",
        // and the one between the sidebar and the first pane).
        if (kind != ARKOS_TAB_BAR_BLOCK && kind != ARKOS_PANE_BODY_BLOCK && kind != ARKOS_ACTIVE_TAB_BLOCK && kind != ARKOS_TITLE_BAR_BLOCK && kind != ARKOS_SIDEBAR_BLOCK) {
            nearest_edge(p, blk, d, hit, u.size);
        }
    }
    // The ground, flat, and the rain laid over it, so it shows on a light ground as on black;
    // over the sidebar as faint as behind a pane's paper, in the rain's own colour (Cyrus,
    // 2026-11-17: "apply the same muted runic rain to the Cmd+0 pane").
    float in_side = side.z > 0.0 ? 1.0 - smoothstep(-0.5, 0.5, sd_chamfer(p - (side.xy + side.zw * 0.5), side.zw * 0.5 + FRAME_OUT, FRAME_CHAMFER)) : 0.0;
    float3 col = mix(bg, mix(drop.rgb, green, in_side), drop.a * RAIN_ALPHA * mix(1.0, SIDE_RAIN, in_side) * k);
    col = mix(col, bg * 0.8, dim * k);
    col = mix(col, green, lift * k);

    // The world (see above): the sidebar's scene, frame and chevron; the title bar's clouds and
    // plates; the panes' far skyline, frames, packets, waiting cursors, underlines and lamps.
    float3 ring_c = u.colors[ARKOS_FOCUS_RING].rgb;
    // The sidebar: its scene at its foot, softened under the names; its frame; its chevron. Its
    // block is its column, inside a pane's margins, so it is framed as a pane is.
    if (side.z > 1.0 && inside_box(p, side)) {
        float4 band = float4(side.x + 2.0, max(side.y + side.w - SCENE_H, side.y + 40.0), side.z - 4.0, 0.0);
        band.w = side.y + side.w - 2.0 - band.y;
        if (band.w >= SCENE_MIN_H && inside_box(p, band)) {
            float2 q = p - band.xy;
            // The scene's sky is what is under it, so its top melts into the sidebar.
            // The wash is drawn at its manifest's half a pixel a point of the window: one pixel a
            // point on a Retina screen, half of one on a 1x screen, where each of its pixels is
            // two points each way and an edge moving through one shows bright, then dim, then
            // bright again (Cyrus, 2026-11-18: "The Matrix City still has the building edges
            // flashing on my external monitor. It's smooth on the native Macbook monitor."). So
            // under one pixel a point the scene is looked at once a point inside each pixel and the
            // looks are averaged: the city a Retina screen sees, in the 1x screen's pixels. Only
            // the scene: it moves; the frames and the rain over it do not, or not that finely.
            float3 art;
            if (u.scale < 0.75) {
                float o = 0.25 / u.scale;
                art = float3(0.0);
                for (int s = 0; s < 4; s++) {
                    float2 qs = q + float2((s & 1) == 0 ? -o : o, (s & 2) == 0 ? -o : o);
                    art += u.scene > 0.5 ? code_scene(qs, band.zw, u.time, u, green, white_c, col) : city_scene(qs, band.zw, u.time, u, green, white_c, col);
                }
                art *= 0.25;
            } else {
                art = u.scene > 0.5 ? code_scene(q, band.zw, u.time, u, green, white_c, col) : city_scene(q, band.zw, u.time, u, green, white_c, col);
            }
            float under = p.y < names_end ? 1.0 - UNDER_NAMES : saturate(1.0 - (p.y - names_end) / SOFT_BUFFER) * (1.0 - UNDER_NAMES);
            art = mix(art, col, max(under, 1.0 - saturate(q.y / 60.0)) * (q.y < 60.0 ? 1.0 : 0.0) + under * (q.y >= 60.0 ? 1.0 : 0.0));
            col = mix(col, art, k);
        }
        // The city's data, from half the sidebar's height (or the scene's top, if higher) down to
        // the things it joins, softened under the names as the scene is.
        if (u.scene > 0.5 && band.w >= SCENE_MIN_H && p.y < band.y + band.w) {
            float top = min(side.y + side.w * 0.5, band.y);
            float3 rained = data_rain(p, top, band.y + band.w * HZ_AT, band.x + band.z * 0.5, band.z * 0.9, u.time, green, white_c, col, u);
            float under = p.y < names_end ? 1.0 - UNDER_NAMES : saturate(1.0 - (p.y - names_end) / SOFT_BUFFER) * (1.0 - UNDER_NAMES);
            col = mix(col, mix(rained, col, under), k);
        }
        // The city's storm clouds, from half the sidebar's height (or the scene's top, if higher)
        // down into the scene, softened under the names as the scene is.
        if (u.scene < 0.5 && band.w >= SCENE_MIN_H) {
            float top = min(side.y + side.w * 0.5, band.y);
            float bottom = band.y + band.w * CLOUD_FOOT;
            uint bkey = 0u;
            float3 clouded = storm_clouds(p, top, bottom, side.x, side.z, u.time, bolt_env(u.time, bkey), u, col);
            float under = p.y < names_end ? 1.0 - UNDER_NAMES : saturate(1.0 - (p.y - names_end) / SOFT_BUFFER) * (1.0 - UNDER_NAMES);
            col = mix(col, mix(clouded, col, under), k);
        }
    }
    if (side.z > 1.0) {
        // The frame, FRAME_OUT round the column, as a pane's: stronger while it has the keys.
        float4 fr = float4(side.xy - FRAME_OUT, side.zw + 2.0 * FRAME_OUT);
        float2 hb = fr.zw * 0.5;
        float d = abs(sd_chamfer(p - (fr.xy + hb), hb, FRAME_CHAMFER));
        if (d < FOCUS_FRAME_HALO + 2.0) {
            float3 c = mix(green, ring_c, side_focus);
            float s = line_light(d, mix(FRAME_W, FOCUS_FRAME_W, side_focus), mix(FRAME_HALO, FOCUS_FRAME_HALO, side_focus)) * mix(FRAME_IDLE, FOCUS_FRAME_LIGHT, side_focus);
            col = light_on(col, c, s * k, u.dark);
        }
    }
    for (int i = 0; i < nb; i++) {
        constant ArkosBlock& blk = u.blocks[i];
        int kind = int(blk.kind);
        float4 r = blk.rect;
        // Nothing of a block reaches past its frame and halo, but the title bar's plates and a
        // pane's waiting cursor, both within its own reach: skip a block p is far from.
        if (p.x < r.x - FRAME_OUT - FRAME_HALO - 4.0 || p.y < r.y - FRAME_OUT - FRAME_HALO - 4.0 || p.x > r.x + r.z + FRAME_OUT + FRAME_HALO + 4.0 || p.y > r.y + r.w + FRAME_OUT + FRAME_HALO + WAIT_SIZE.y) {
            if (kind != ARKOS_TAB_BAR_BLOCK) {
                continue;
            }
        }
        if (kind == ARKOS_ROW_HIGHLIGHT_BLOCK && blk.ordinal >= 1.0 && side.z > 1.0 && r.x < side.x + side.z) {
            // The selected row's chevron in the sidebar's left padding.
            float cx = side.x + 4.0;
            float cy = r.y + r.w * 0.5;
            float2 a = p - float2(cx, cy);
            float d = abs(abs(a.y) - (3.5 - a.x));
            if (a.x >= -0.5 && a.x <= 4.0 && abs(a.y) <= 4.0) {
                float pulse = 0.75 + 0.25 * cos(u.time * 3.0);
                col = light_on(col, ring_c, (1.0 - smoothstep(0.4, 1.2, d)) * pulse * blk.value * k, u.dark);
            }
        } else if (kind == ARKOS_TITLE_BAR_BLOCK && inside_box(p, r)) {
            // Storm clouds along the bar, a plate behind its words and icons.
            bool plate = false;
            for (int j = 0; j < nb; j++) {
                if (int(u.blocks[j].kind) == ARKOS_TITLE_GROUP_BLOCK) {
                    float4 g = u.blocks[j].rect;
                    float2 ghb = g.zw * 0.5 + float2(5.0, 3.0);
                    float gd = sd_box(p - (g.xy + g.zw * 0.5), ghb - 3.0) - 3.0;
                    if (gd < 0.0) {
                        plate = true;
                        col = mix(col, u.colors[ARKOS_TITLE_BAR].rgb * 0.85, k);
                        col = light_on(col, green, (1.0 - smoothstep(0.0, 1.2, abs(gd + 0.6))) * 0.25 * k, u.dark);
                    }
                }
            }
            if (!plate) {
                float c = fbm(float2((p.x + u.time * 5.0) / 70.0, p.y / 12.0));
                col = mix(col, mix(col, u.colors[ARKOS_TEXT_DIM].rgb, 0.18), smoothstep(0.45, 0.75, c) * k);
            }
        } else if (kind == ARKOS_PANE_BODY_BLOCK && inside_box(p, r)) {
            // The body as it should reach the screen, drawn strong enough to read through the
            // paper: its top band, its skyline, and the rain as faint as over a source file.
            float3 bgt = u.colors[ARKOS_TERMINAL_BG].rgb;
            // Measured from the frame (FRAME_OUT out), as the room round it is.
            float3 want = pane_night(u, float2(p.x - r.x + FRAME_OUT, p.y - r.y), r.y + r.w + FRAME_OUT - p.y, blk.seed, u.time, green);
            want = mix(want, green, drop.a * RAIN_ALPHA * BODY_RAIN);
            col = mix(bgt, through(bgt, want), k);
            // A highlighted row in the body keeps its glow over it.
            col = mix(col, green, lift * k);
        } else if (kind == ARKOS_ACTIVE_TAB_BLOCK && (inside_box(p, r) || (abs(r.x - shape_pane.x) < 1.0 && shape_pane.z > 0.0 && p.x < r.x && p.x >= r.x - FRAME_OUT + 1.0 && p.y >= r.y && p.y < r.y + r.w))) {
            // A first tab reaches the frame on its left (the room is the pane's, `shape_pane`).
            // The doorway: the tab's colour, open to the body below, a neon line along its top
            // (the focus colour on the focused pane) glowing a little down into it, and its lamp.
            float3 tabc = u.colors[ARKOS_TAB_ACTIVE].rgb;
            float3 line = mix(mix(tabc, green, 0.7), ring_c, bar_focus);
            float yy = p.y - r.y;
            float3 c = tabc;
            if (yy < DOOR_LINE) {
                c = line;
            } else {
                c = light_on(c, line, max(0.0, 1.0 - (yy - DOOR_LINE) / DOOR_GLOW) * 0.22 * mix(0.6, 1.0, bar_focus), u.dark);
                if (r.z >= TAB_LAMP_MIN_W) {
                    c = tab_lamp(u, p, r, u.time, blk.seed, green, c);
                }
            }
            col = mix(col, c, k);
        } else if (kind == ARKOS_TAB_BAR_BLOCK) {
            // The pane: this bar and the body laid out after it.
            float4 pr = r;
            if (i + 1 < nb && int(u.blocks[i + 1].kind) == ARKOS_PANE_BODY_BLOCK) {
                pr.w += u.blocks[i + 1].rect.w;
            }
            // A pixel outside this pane's frame and halo, and past its waiting cursor, has
            // nothing of it.
            if (p.x < pr.x - FRAME_OUT - FRAME_HALO - 2.0 || p.x > pr.x + pr.z + FRAME_OUT + FRAME_HALO + 2.0 || p.y < pr.y - FRAME_OUT - FRAME_HALO - 2.0 || p.y > pr.y + pr.w + FRAME_OUT + WAIT_SIZE.y) {
                continue;
            }
            float focus = saturate(blk.focus);
            // The focused pane, for the runes inside its frame (after the world, over its body).
            float4 out_rect = float4(pr.xy - FRAME_OUT, pr.zw + 2.0 * FRAME_OUT);
            if (focus > in_focus && inside_box(p, out_rect)) {
                in_focus = focus;
                in_rect = out_rect;
                in_block = i;
                in_body = float4(pr.x, r.y + r.w, pr.z, pr.y + pr.w - (r.y + r.w));
            }
            // The pane reaches its frame: the room between the pane's own rect and the frame's
            // line, cut at the frame's corners, is filled with the strip beside and above the
            // tab bar and with the body's night beside and below the body (Cyrus, 2026-11-17:
            // "flush with the pane's borders and match the shape too").
            float2 out_c = out_rect.xy + out_rect.zw * 0.5;
            bool in_shape = sd_chamfer(p - out_c, out_rect.zw * 0.5, FRAME_CHAMFER) < 0.0;
            bool in_room = in_shape && !inside_box(p, pr);
            if (in_shape) {
                shape_pane = pr;
            }
            if (in_room && p.y >= r.y + r.w && i + 1 < nb && int(u.blocks[i + 1].kind) == ARKOS_PANE_BODY_BLOCK) {
                // The body's night, as the body's paper shows it (no paper here, so as it is).
                constant ArkosBlock& bb = u.blocks[i + 1];
                float4 b = bb.rect;
                float3 want = pane_night(u, float2(p.x - b.x + FRAME_OUT, p.y - b.y), b.y + b.w + FRAME_OUT - p.y, bb.seed, u.time, green);
                want = mix(want, green, drop.a * RAIN_ALPHA * BODY_RAIN);
                col = mix(col, want, k);
            }
            // The strip: a solid panel, from the frame's top to the tab bar's foot.
            if (inside_box(p, r) || (in_room && p.y < r.y + r.w)) {
                float3 stone = u.colors[ARKOS_BANNER].rgb;
                float yy = p.y - r.y;
                float3 c = stone;
                if (p.y < out_rect.y + 1.0) {
                    c = mix(stone, u.colors[ARKOS_TEXT_DIM].rgb, STRIP_HI);
                } else if (yy >= r.w - 1.0) {
                    c = away(u, stone, STRIP_FOOT);
                } else {
                    if (yy >= 0.0 && fmod(p.x - r.x + blk.seed * 17.0, STRIP_PANEL) < 1.0) {
                        c = mix(stone, u.colors[ARKOS_TAB_BAR].rgb, STRIP_SEAM);
                    }
                    if (yy >= 2.0 && yy < 3.0) {
                        c = mix(c, mix(green, ring_c, focus), STRIP_TRACE);
                    }
                }
                c = mix(c, u.colors[ARKOS_TEXT].rgb, STRIP_HOVER * tab_hover);
                col = mix(col, c, k);
            }
            // The lamp at the bar's right end.
            float ld = length(p - float2(r.x + r.z - LAMP_IN, r.y + r.w * 0.5));
            if (ld < LAMP_R + 3.0) {
                float blink = blk.busy > 0.5 ? (fract(u.time * 1.5) < 0.6 ? 1.0 : 0.35) : 1.0;
                float3 lc = mix(green * 0.5, ring_c, focus);
                float s = (1.0 - smoothstep(LAMP_R - 0.6, LAMP_R + 0.4, ld)) + 0.3 * max(0.0, 1.0 - ld / (LAMP_R + 3.0));
                col = light_on(col, lc, s * mix(0.4, 1.0, focus) * blink * k, u.dark);
            }
            // The frame in the gap, a packet round it while the pane works.
            float4 fr = float4(pr.xy - FRAME_OUT, pr.zw + 2.0 * FRAME_OUT);
            float2 hb = fr.zw * 0.5;
            float d = abs(sd_chamfer(p - (fr.xy + hb), hb, FRAME_CHAMFER));
            if (d < FOCUS_FRAME_HALO + 2.0) {
                float s = line_light(d, mix(FRAME_W, FOCUS_FRAME_W, focus), mix(FRAME_HALO, FOCUS_FRAME_HALO, focus)) * mix(FRAME_IDLE, FOCUS_FRAME_LIGHT, focus);
                float3 c = mix(green, ring_c, focus);
                if (blk.busy > 0.5) {
                    float per = 2.0 * (fr.z + fr.w);
                    float behind = fract(u.time * PACKET_SPEED / per) * per - perimeter_at(p, fr);
                    behind = behind < 0.0 ? behind + per : behind;
                    if (behind < PACKET_LEN) {
                        float g = 1.0 - behind / PACKET_LEN;
                        c = mix(c, white_c, 0.6 * g);
                        s += g * line_light(d, FRAME_W * 1.6, FRAME_HALO * 1.5);
                    }
                }
                col = light_on(col, c, s * k, u.dark);
            }
            // The waiting cursor, blinking on the frame's foot.
            if (blk.waiting > 0.5) {
                float2 o = float2(fr.x + fr.z - WAIT_IN, fr.y + fr.w - WAIT_SIZE.y * 0.5);
                if (inside_box(p, float4(o, WAIT_SIZE)) && fract(u.time / WAIT_BLINK) < 0.6) {
                    col = mix(col, u.colors[ARKOS_WAITING].rgb, k);
                }
            }
        }
    }
    // The nearest reported edge's light, laid over the rest. On a dark ground it is added, as
    // the mock-up adds it, so a busy head or a tear burns on toward white until each channel
    // clips; on a light ground added light would vanish, so the colour is mixed in instead.
    float4 e = edge_glow(hit, u.time, green, white_c);
    if (u.dark > 0.5) {
        col = min(col + e.rgb * e.a * k, 1.0);
    } else {
        col = mix(col, e.rgb, saturate(e.a * k));
    }
    // The runes inside the focused pane's frame, in the focus colour, along a line that is the
    // frame's own outline moved INNER_IN inward (its distance field offset, so its cut corners
    // keep their shape); drawn 1 / (1 - PAPER) as strong over its body so they read through the
    // paper as over its strip.
    if (in_focus > 0.0) {
        constant ArkosBlock& fb = u.blocks[in_block];
        float2 fc = in_rect.xy + in_rect.zw * 0.5;
        float2 fhb = in_rect.zw * 0.5;
        float frame_d = sd_chamfer(p - fc, fhb, FRAME_CHAMFER);
        if (frame_d < -FOCUS_FRAME_W * 0.5) {
            // Which side p is nearest decides how its runes run: streams down a side, flipping
            // runes along the top and foot; along is measured from that side's start.
            float2 q = abs(p - fc) - fhb;
            bool vert = q.x > q.y;
            uint side = vert ? (p.x > fc.x ? 1u : 3u) : (p.y > fc.y ? 2u : 0u);
            float len = vert ? in_rect.w : in_rect.z;
            EdgeHit fh = no_edge();
            fh.across = frame_d + INNER_IN;
            fh.d = abs(fh.across);
            fh.rank = fh.d;
            fh.along = clamp(vert ? p.y - in_rect.y : p.x - in_rect.x, 0.0, len);
            fh.len = len;
            fh.busy = fb.busy;
            fh.key = as_type<uint>(fb.seed + 101.0) * 4u + side;
            fh.vert = vert;
            float4 fe = edge_glow(fh, u.time, u.colors[ARKOS_FOCUS_RING].rgb, white_c);
            float a = fe.a * INNER_LIGHT * in_focus * (inside_box(p, in_body) ? 1.0 / (1.0 - PAPER) : 1.0);
            if (u.dark > 0.5) {
                col = min(col + fe.rgb * a * k, 1.0);
            } else {
                col = mix(col, fe.rgb, saturate(a * k));
            }
        }
    }
    return float4(col, 1.0);
}

// The menu stage: the open menus' edges, drawn above the panes with the panes' own edge light,
// and nothing else. Its blocks are only the menus, each with its fade as its value and never
// busy. It returns colour multiplied by its alpha.
fragment float4 arkos_overlay(ArkosVertex in [[stage_in]], constant ArkosInputs& u [[buffer(0)]]) {
    float2 p = in.uv * u.size;
    float k = saturate(u.intensity);
    float3 white_c = u.colors[ARKOS_ANSI + 15].rgb;
    float3 green = toward_accent(theme_green(u), u.colors[ARKOS_FOCUS_RING].rgb);
    int nb = min(int(u.block_count), 48);
    EdgeHit hit = no_edge();
    for (int i = 0; i < nb; i++) {
        ArkosBlock blk = u.blocks[i];
        float4 r = blk.rect;
        float2 hb = r.zw * 0.5;
        // A menu may sit against the window's border, and keeps every edge.
        nearest_edge(p, blk, sd_box(p - (r.xy + hb), hb), hit, float2(0.0));
    }
    if (hit.d >= EDGE_REACH) {
        return float4(0.0);
    }
    float4 e = edge_glow(hit, u.time, green, white_c);
    // The edges fade with their menu (blk.value, carried in the hit). The alpha is the light's
    // coverage; on a dark ground the colour is the light added, unclamped up to 1 a channel, as
    // the wash adds it, and on a light one the mixed colour.
    float strength = e.a * k * hit.value;
    float a = saturate(strength);
    float3 c = u.dark > 0.5 ? min(e.rgb * strength, 1.0) : e.rgb * a;
    return float4(c, a);
}
