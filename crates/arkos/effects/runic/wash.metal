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
// How much of the rain the sidebar carries: what a pane's paper (0.92) lets through of its rain
// and PANE_RAIN's more, so the Cmd+0 pane is as quiet as a source file.
constant float SIDE_RAIN = 0.11;
// How much more rain the pane bodies carry, as a share of RAIN_ALPHA laid again over them: a
// terminal's paper covers the wash (paper_alpha), so without it the rain all but vanishes there;
// a little, so it stays a quiet ground behind the text (Cyrus, 2026-11-17: "blend it better
// into the background so it's more subtle").
constant float PANE_RAIN = 0.4;
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
// frame's foot of one whose session waits. The tab bar's active tab is underlined in light and
// a lamp at the bar's right end is lit on the focused pane. The title bar is a strip of storm
// clouds, its words and icons on plates. The sidebar wears a frame, a chevron marks its selected
// row, and at its foot, below the names, stands the theme's scene (`ArkosInputs::scene`): 0 a
// city skyline in the rain, its towers' windows lit, runic lightning now and then; 1 a city of
// falling code with a phone booth's light ringing and, now and then, a black cat crossing the
// street twice. Behind a terminal, the far skyline along its foot. The clock's zero (Reduce
// Motion) has no bolt, no cat and every light on.

// The frame's line: how far outside a pane's edges (the gap is 16, so two neighbours' frames keep
// 6 points of ground between them), its width, its halo, its corner cut.
constant float FRAME_OUT = 5.0;
constant float FRAME_W = 1.3;
constant float FRAME_HALO = 4.0;
constant float FRAME_CHAMFER = 7.0;
// An unfocused frame's strength against a focused one's 1.
constant float FRAME_IDLE = 0.5;
// The packet of light round a working pane's frame: its length in points and its speed.
constant float PACKET_LEN = 70.0;
constant float PACKET_SPEED = 240.0;
// The waiting cursor: its size, how far in from the frame's right end, and its blink.
constant float2 WAIT_SIZE = float2(7.0, 10.0);
constant float WAIT_IN = 26.0;
constant float WAIT_BLINK = 1.1;
// The lamp at a tab bar's right end: its radius and how far in from the end its centre sits.
constant float LAMP_R = 2.6;
constant float LAMP_IN = 16.0;
// The sidebar's frame line, its inset from the sidebar's edges.
constant float SIDE_FRAME_IN = 3.5;
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
constant float BOLT_FOR = 0.45;
// The black cat: a pass across the street, then the same pass again, once a cycle.
constant float CAT_CYCLE = 26.0;
constant float CAT_PASS = 4.0;
// The phone booth's ring: two short rings, then quiet, every cycle.
constant float RING_CYCLE = 3.0;

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

// The bolt's envelope at t, 0 to 1 (two flickers and a fade), and its slot's key; 0 in the first
// cycle and between strikes.
float bolt_env(float t, thread uint& key) {
    float slot = floor(t / BOLT_CYCLE);
    key = uint(slot) * 2654435761u + 17u;
    float at = t - slot * BOLT_CYCLE - hash01(key) * (BOLT_CYCLE - BOLT_FOR - 1.0);
    if (slot < 1.0 || at < 0.0 || at >= BOLT_FOR) {
        return 0.0;
    }
    return at < 0.06 ? 1.0 : (at < 0.12 ? 0.25 : (at < 0.22 ? 0.9 : 1.0 - (at - 0.22) / (BOLT_FOR - 0.22)));
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
    // A bolt: its envelope this frame (two flickers), its slot's key and its path. The clouds
    // over the city are drawn over the whole sidebar's lower half (`storm_clouds`).
    uint key = 0u;
    float env = bolt_env(t, key);
    col = mix(col, mix(col, white_c, 0.35), env * 0.35 * (1.0 - glow));
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
            float s = 0.45 * (1.0 - smoothstep(0.3, 1.0, d)) + 0.6 * pow(max(0.0, 1.0 - d / 9.0), 2.0);
            float2 g = float2((q.x - bx + 3.0) / 1.2, fract(q.y / 9.0) * 9.0 / 1.2);
            float runes = 0.0;
            if (g.x >= 0.0 && g.x < 5.0 && g.y >= 0.5 && g.y < 7.5) {
                float2 b = floor(g - float2(0.0, 0.5));
                float2 f = fract(g - float2(0.0, 0.5));
                bool lit = hash2(float2(floor(q.y / 9.0) * 13.0 + b.x, b.y * 7.0 + float(key % 211u))) < 0.5;
                runes = lit && max(abs(f.x - 0.5), abs(f.y - 0.5)) < 0.42 ? 1.3 : 0.0;
            }
            col = light_on(col, mix(green, white_c, 0.7), (s + runes) * env, u.dark);
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

// Whether q (in the cat's own units, 1 a point at its size) is on the black cat: a body, a head
// with two ears, two legs and a tail curling up behind.
bool cat_at(float2 cq) {
    bool body = cq.x >= 0.0 && cq.x < 7.0 && cq.y >= 2.0 && cq.y < 5.0;
    bool head = cq.x >= 7.0 && cq.x < 10.5 && cq.y >= 0.0 && cq.y < 3.5 && !(cq.y < 1.0 && cq.x >= 8.2 && cq.x < 9.3);
    bool legs = cq.y >= 5.0 && cq.y < 6.5 && (abs(cq.x - 1.0) < 0.6 || abs(cq.x - 6.0) < 0.6);
    bool tail = cq.x >= -3.5 && cq.x < 0.0 && abs(cq.y - (3.0 - (cq.x + 3.5) * 0.7)) < 0.6;
    return body || head || legs || tail;
}

// The code city's scene at q: towers on the city's skyline whose insides are falling code, their
// outlines lit, a phone booth whose light rings, and a black cat crossing the street twice.
float3 code_scene(float2 q, float2 sz, float t, constant ArkosInputs& u, float3 green, float3 white_c, float3 sb) {
    float3 bg = u.colors[ARKOS_WINDOW_BG].rgb;
    float3 col = sb;
    float near_top = code_top(q.x, sz.y, 1);
    float far_top = code_top(q.x, sz.y, 0);
    bool in_far = q.y > far_top;
    bool in_near = q.y > near_top;
    if (in_far || in_near) {
        // The code falling inside: 5 by 7 point cells, a stream a column.
        float2 cell = floor(q / float2(5.0, 7.0));
        float speed = 4.0 + 6.0 * hash2(float2(cell.x, 2.0 + (in_near ? 1.0 : 0.0)));
        float head = floor(fract(t * speed / 40.0 + hash2(float2(cell.x, 9.0))) * 40.0);
        float behind = head - cell.y;
        float2 f = fract(q / float2(5.0, 7.0));
        float2 bit = floor(float2(f.x * 5.0, f.y * 7.0));
        float on = (bit.x >= 1.0 && bit.x <= 3.0 && bit.y >= 1.0 && bit.y <= 5.0 && hash2(cell * 7.0 + bit + floor(t / 0.4)) < 0.5) ? 1.0 : 0.0;
        float trail = (behind >= 0.0 && behind < 14.0) ? 1.0 - behind / 14.0 : 0.15;
        float3 base = in_near ? bg * 0.6 : mix(sb, bg, 0.5);
        float3 head_c = behind < 1.0 && behind >= 0.0 ? mix(green, white_c, 0.5) : green;
        col = mix(base, head_c, on * trail * (in_near ? 1.0 : 0.55));
        // The tower's outline.
        float edge = in_near ? (q.y - near_top < 1.0 ? 1.0 : 0.0) : (q.y - far_top < 1.0 ? 0.6 : 0.0);
        col = light_on(col, green, edge * 0.5, u.dark);
    }
    // The street.
    if (q.y > sz.y - 3.0) {
        col = mix(bg, green, 0.12);
    }
    // The phone booth, ringing: two short rings a cycle, then quiet.
    float2 booth = float2(sz.x * 0.62, sz.y - 3.0 - 22.0);
    float2 bq = q - booth;
    if (bq.x >= 0.0 && bq.x < 11.0 && bq.y >= 0.0 && bq.y < 22.0) {
        bool frame_px = bq.x < 1.0 || bq.x >= 10.0 || bq.y < 1.0 || bq.y >= 21.0 || abs(bq.y - 9.0) < 0.5;
        col = frame_px ? mix(green, white_c, 0.2) : mix(bg, green, 0.08);
    }
    float rt = fract(t / RING_CYCLE) * RING_CYCLE;
    float ring = (rt < 0.3 || (rt >= 0.5 && rt < 0.8)) ? 1.0 : 0.15;
    float ld = length(q - (booth + float2(5.5, -2.0)));
    if (ld < 2.5) {
        col = light_on(col, mix(green, white_c, 0.6), (1.0 - ld / 2.5) * ring, u.dark);
    }
    // The black cat: it crosses the street, and a moment later crosses it again the same way, a
    // glitch across the scene as it does. None in the first cycle.
    float cslot = floor(t / CAT_CYCLE);
    float ct = t - cslot * CAT_CYCLE;
    float pass = ct < CAT_PASS ? ct / CAT_PASS : ((ct >= CAT_PASS + 0.8 && ct < 2.0 * CAT_PASS + 0.8) ? (ct - CAT_PASS - 0.8) / CAT_PASS : -1.0);
    if (cslot >= 1.0 && pass >= 0.0) {
        float2 cat = float2(-18.0 + pass * (sz.x + 36.0), sz.y - 3.0 - 9.0);
        float2 cq = (q - cat) / 1.5;
        if (cat_at(cq)) {
            col = bg * 0.15;
            // Its eyes, two lit points in its head.
            if (abs(cq.y - 1.5) < 0.5 && (abs(cq.x - 8.0) < 0.45 || abs(cq.x - 9.3) < 0.45)) {
                col = mix(green, white_c, 0.5);
            }
        } else if (cat_at(cq + float2(0.7, 0.0)) || cat_at(cq - float2(0.7, 0.0)) || cat_at(cq + float2(0.0, 0.7)) || cat_at(cq - float2(0.0, 0.7))) {
            // A rim of green round it, so a black cat shows on a dark street.
            col = mix(col, green, 0.55);
        }
        if (ct >= CAT_PASS + 0.8 && abs(q.y - fract(t * 3.0) * sz.y) < 0.8) {
            col = light_on(col, green, 0.35, u.dark);
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
    float4 r = rain(p, u.time, green);

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
    // The ground, flat, and the rain laid over it, so it shows on a light ground as on black;
    // over the sidebar as faint as behind a pane's paper, in the rain's own colour (Cyrus,
    // 2026-11-17: "apply the same muted runic rain to the Cmd+0 pane").
    float in_side = side.z > 0.0 ? 1.0 - smoothstep(-0.5, 0.5, sd_box(p - (side.xy + side.zw * 0.5), side.zw * 0.5)) : 0.0;
    float3 col = mix(bg, mix(r.rgb, green, in_side), r.a * RAIN_ALPHA * mix(1.0, SIDE_RAIN, in_side) * k);
    col = mix(col, bg * 0.8, dim * k);
    // The tab strips carry the rain more strongly: TAB_RAIN more of it, laid over the first.
    col = mix(col, r.rgb, r.a * RAIN_ALPHA * TAB_RAIN * tab * k);
    // The pane bodies carry it more strongly still: PANE_RAIN more, under the panes' paper.
    col = mix(col, green, r.a * RAIN_ALPHA * PANE_RAIN * pane * k);
    col = mix(col, green, lift * k);

    // The world (see above): the sidebar's scene, frame and chevron; the title bar's clouds and
    // plates; the panes' far skyline, frames, packets, waiting cursors, underlines and lamps.
    float3 ring_c = u.colors[ARKOS_FOCUS_RING].rgb;
    float3 sb = u.colors[ARKOS_SIDEBAR].rgb;
    // The sidebar: its scene at its foot, softened under the names; its frame; its chevron.
    if (side.z > 1.0 && inside_box(p, side)) {
        float4 band = float4(side.x + SIDE_FRAME_IN + 2.0, max(side.y + side.w - SCENE_H, side.y + 40.0), side.z - 2.0 * (SIDE_FRAME_IN + 2.0), 0.0);
        band.w = side.y + side.w - SIDE_FRAME_IN - 2.0 - band.y;
        if (band.w >= SCENE_MIN_H && inside_box(p, band)) {
            float2 q = p - band.xy;
            // The scene's sky is what is under it, so its top melts into the sidebar.
            float3 art = u.scene > 0.5 ? code_scene(q, band.zw, u.time, u, green, white_c, col) : city_scene(q, band.zw, u.time, u, green, white_c, col);
            float under = p.y < names_end ? 1.0 - UNDER_NAMES : saturate(1.0 - (p.y - names_end) / SOFT_BUFFER) * (1.0 - UNDER_NAMES);
            art = mix(art, col, max(under, 1.0 - saturate(q.y / 60.0)) * (q.y < 60.0 ? 1.0 : 0.0) + under * (q.y >= 60.0 ? 1.0 : 0.0));
            col = mix(col, art, k);
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
        float4 fr = float4(side.x + SIDE_FRAME_IN, side.y + SIDE_FRAME_IN, side.z - 2.0 * SIDE_FRAME_IN, side.w - 2.0 * SIDE_FRAME_IN);
        float2 hb = fr.zw * 0.5;
        float d = abs(sd_chamfer(p - (fr.xy + hb), hb, FRAME_CHAMFER));
        if (d < FRAME_HALO) {
            float3 c = mix(green, ring_c, side_focus);
            col = light_on(col, c, line_light(d, FRAME_W, FRAME_HALO) * mix(FRAME_IDLE, 1.0, side_focus) * k, u.dark);
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
            float cx = side.x + SIDE_FRAME_IN + 4.0;
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
            // The far skyline along a terminal's foot, faint under its paper.
            float2 q = float2(p.x - r.x, p.y - (r.y + r.w - 50.0));
            if (q.y > 0.0 && q.y > tower_top(q.x, 50.0, 0)) {
                col = mix(col, mix(bg, sb, 0.5), 0.5 * k);
            }
        } else if (kind == ARKOS_ACTIVE_TAB_BLOCK && inside_box(p, float4(r.x, r.y + r.w - 2.0, r.z, 2.0))) {
            // The active tab's underline, in the focus colour on the focused pane.
            col = light_on(col, mix(green, ring_c, bar_focus), mix(0.5, 1.0, bar_focus) * k, u.dark);
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
            if (d < FRAME_HALO + 2.0) {
                float s = line_light(d, FRAME_W, FRAME_HALO) * mix(FRAME_IDLE, 1.0, focus);
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
