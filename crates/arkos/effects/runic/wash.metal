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
// How much more rain the tab strips carry, as a share of RAIN_ALPHA laid again over them.
constant float TAB_RAIN = 0.4;
// How much more rain the pane bodies carry, as a share of RAIN_ALPHA laid again over them: a
// terminal's paper covers the wash (paper_alpha), so without it the rain all but vanishes there.
constant float PANE_RAIN = 1.5;
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

fragment float4 arkos_wash(ArkosVertex in [[stage_in]], constant ArkosInputs& u [[buffer(0)]]) {
    float2 p = in.uv * u.size;
    float k = saturate(u.intensity);

    float3 bg = u.colors[ARKOS_WINDOW_BG].rgb;
    float3 white_c = u.colors[ARKOS_ANSI + 15].rgb;
    float3 green = toward_accent(theme_green(u), u.colors[ARKOS_FOCUS_RING].rgb);

    // The ground, flat, and the rain laid over it, so it shows on a light ground as on black.
    float4 r = rain(p, u.time, green);
    float3 col = mix(bg, r.rgb, r.a * RAIN_ALPHA * k);

    // The blocks: the bars darkened a little, the highlights glowing, every reported edge live.
    int nb = min(int(u.block_count), 48);
    float dim = 0.0;
    float lift = 0.0;
    // How much of a tab bar covers p, for the extra rain laid on the tab strips, and how much of
    // a pane body, for the extra rain laid under the panes' paper.
    float tab = 0.0;
    float pane = 0.0;
    EdgeHit hit = no_edge();
    for (int i = 0; i < nb; i++) {
        ArkosBlock blk = u.blocks[i];
        float4 rect = blk.rect;
        float2 hb = rect.zw * 0.5;
        float d = sd_box(p - (rect.xy + hb), hb);
        int kind = int(blk.kind);
        if (kind == ARKOS_TITLE_BAR_BLOCK || kind == ARKOS_TAB_BAR_BLOCK || kind == ARKOS_SIDEBAR_BLOCK) {
            float inside = 1.0 - smoothstep(-0.5, 0.5, d);
            float amount = kind == ARKOS_SIDEBAR_BLOCK ? 0.35 : (kind == ARKOS_TAB_BAR_BLOCK ? 0.0 : 0.42);
            dim = max(dim, inside * amount);
            if (kind == ARKOS_TAB_BAR_BLOCK) {
                tab = max(tab, inside);
            }
        } else if (kind == ARKOS_PANE_BODY_BLOCK) {
            pane = max(pane, 1.0 - smoothstep(-0.5, 0.5, d));
        } else if (kind == ARKOS_ROW_HIGHLIGHT_BLOCK || kind == ARKOS_TAB_HIGHLIGHT_BLOCK) {
            if (d < 6.0) {
                float s = HIGHLIGHT_STRENGTH[min(max(int(blk.ordinal), 0), 2)] * blk.value;
                float fill = (1.0 - smoothstep(-1.5, 1.0, d)) + 0.35 * (1.0 - smoothstep(0.0, 6.0, d));
                lift = max(lift, s * saturate(fill));
            }
        }
        nearest_edge(p, blk, d, hit, u.size);
    }
    col = mix(col, bg * 0.8, dim * k);
    // The tab strips carry the rain more strongly: TAB_RAIN more of it, laid over the first.
    col = mix(col, r.rgb, r.a * RAIN_ALPHA * TAB_RAIN * tab * k);
    // The pane bodies carry it more strongly still: PANE_RAIN more, under the panes' paper.
    col = mix(col, r.rgb, r.a * RAIN_ALPHA * PANE_RAIN * pane * k);
    col = mix(col, green, lift * k);
    // The nearest reported edge's light, laid over the rest. On a dark ground it is added, as
    // the mock-up adds it, so a busy head or a tear burns on toward white until each channel
    // clips; on a light ground added light would vanish, so the colour is mixed in instead.
    float4 e = edge_glow(hit, u.time, green, white_c);
    if (u.dark > 0.5) {
        col = min(col + e.rgb * e.a * k, 1.0);
    } else {
        col = mix(col, e.rgb, saturate(e.a * k));
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
