// The matrix wash under the Arkos window: digital rain made from the theme's colours, faint green
// runes falling in columns over a flat ground; and for the UI's blocks, a glow line on every edge
// the app no longer draws, a soft green fill on the highlights and a faint darkening under the
// bars. The preamble (arkos.metal) is prepended before this is compiled, and declares
// ArkosInputs, ArkosBlock, ArkosVertex and the ARKOS_* indices used here.
//
// Every stream is a pure function of the clock, so Reduce Motion (which freezes it) shows still
// rain. The rain's constants are mirrored in crates/arkos/src/effects/matrix.rs, which tests
// them: a change here is made there too.

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
// Seconds a column waits between its streams, beyond their fall: long enough that about a third
// of the columns carry a stream on screen at any moment in a window 800 points tall.
constant float RAIN_GAP = 30.0;
// The rows a stream falls through in its cycle before its trail, whatever the window's height,
// so a resize never moves a column's phase; a window taller than this sees the streams fall on
// into the wait.
constant float RAIN_ROWS_PERIOD = 60.0;
// Seconds a rune shows before its cell picks another.
constant float RUNE_CHANGE = 0.5;
// The most the rain adds of its green to the ground, before the user's intensity.
constant float RAIN_ALPHA = 0.12;
// The head's extra brightness: how far its rune leans from the green toward white.
constant float RAIN_HEAD = 0.35;
// A rune's bit, in points: its 5 by 7 bits make a shape 10 by 14 points, centred in its cell.
constant float RUNE_BIT = 2.0;
// How soft a bit's edges are, as a share of the bit: its square fades out over this much.
constant float RUNE_SOFT = 0.25;

// The pane edges' glow, as space's without its sweeping bars: the halo's strength.
constant float EDGE_HALO = 0.75;
// How far the halo reaches, in points: its strength falls by e over this distance.
constant float EDGE_HALO_REACH = 2.0;
// The line's core, a little over a point wide: its strength.
constant float EDGE_CORE = 0.7;
// How far the core leans from the ring's colour toward the theme's bright white.
constant float EDGE_CORE_WHITE = 0.15;
// How far out edges are looked for, in points; the halo fades to nothing over the last 3 of it.
constant float EDGE_REACH = 12.0;

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

// The distance from p to a horizontal segment from x0 to x1 at y, and to a vertical one.
float seg_h(float2 p, float y, float x0, float x1) {
    float dx = max(max(x0 - p.x, p.x - x1), 0.0);
    return length(float2(dx, p.y - y));
}
float seg_v(float2 p, float x, float y0, float y1) {
    float dy = max(max(y0 - p.y, p.y - y1), 0.0);
    return length(float2(p.x - x, dy));
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

// The edge nearest a pixel so far: how far it is, and its block's fade.
struct EdgeHit {
    float d;
    float value;
};

EdgeHit no_edge() {
    EdgeHit h;
    h.d = 1e5;
    h.value = 1.0;
    return h;
}

// Takes blk's reported edges into h when one is nearer p; d is p's distance to blk's box.
void nearest_edge(float2 p, ArkosBlock blk, float d, thread EdgeHit& h) {
    uint edges = uint(blk.edges);
    if (edges != 0u && abs(d) < EDGE_REACH) {
        float4 r = blk.rect;
        float x0 = r.x, y0 = r.y, x1 = r.x + r.z, y1 = r.y + r.w;
        float e;
        if (edges & ARKOS_EDGE_TOP) { e = seg_h(p, y0, x0, x1); if (e < h.d) { h.d = e; h.value = blk.value; } }
        if (edges & ARKOS_EDGE_RIGHT) { e = seg_v(p, x1, y0, y1); if (e < h.d) { h.d = e; h.value = blk.value; } }
        if (edges & ARKOS_EDGE_BOTTOM) { e = seg_h(p, y1, x0, x1); if (e < h.d) { h.d = e; h.value = blk.value; } }
        if (edges & ARKOS_EDGE_LEFT) { e = seg_v(p, x0, y0, y1); if (e < h.d) { h.d = e; h.value = blk.value; } }
    }
}

// The glow line at the nearest edge: a bright core a little over a point wide and a soft halo a
// couple of points out. The halo is laid in the ring's colour and the core over it in core_col;
// their strengths are before the user's intensity.
struct Glow {
    float halo;
    float core;
    float3 core_col;
};

Glow edge_glow(EdgeHit h, float3 ring, float3 white_c) {
    float dline = h.d;
    Glow g;
    g.core = saturate((1.0 - smoothstep(0.5, 1.1, dline)) * EDGE_CORE);
    g.halo = exp(-dline / EDGE_HALO_REACH) * EDGE_HALO * (1.0 - smoothstep(EDGE_REACH - 3.0, EDGE_REACH, dline));
    g.core_col = mix(ring, white_c, EDGE_CORE_WHITE);
    return g;
}

// The theme's green: the terminal's text colour when it is greener than the focus ring (its
// green above its red and blue), else the ring, so another theme's rain reads in its own colour.
float3 theme_green(constant ArkosInputs& u) {
    float3 fg = u.colors[ARKOS_TERMINAL_FG].rgb;
    float3 ring = u.colors[ARKOS_FOCUS_RING].rgb;
    return fg.g - max(fg.r, fg.b) > ring.g - max(ring.r, ring.b) ? fg : ring;
}

fragment float4 arkos_wash(ArkosVertex in [[stage_in]], constant ArkosInputs& u [[buffer(0)]]) {
    float2 p = in.uv * u.size;
    float k = saturate(u.intensity);

    float3 bg = u.colors[ARKOS_WINDOW_BG].rgb;
    float3 ring = u.colors[ARKOS_FOCUS_RING].rgb;
    float3 white_c = u.colors[ARKOS_ANSI + 15].rgb;
    float3 green = theme_green(u);

    // The ground, flat, and the rain laid over it, so it shows on a light ground as on black.
    float4 r = rain(p, u.time, green);
    float3 col = mix(bg, r.rgb, r.a * RAIN_ALPHA * k);

    // The blocks: the bars darkened a little, the highlights glowing, every reported edge a line.
    int nb = min(int(u.block_count), 48);
    float dim = 0.0;
    float lift = 0.0;
    EdgeHit hit = no_edge();
    for (int i = 0; i < nb; i++) {
        ArkosBlock blk = u.blocks[i];
        float4 rect = blk.rect;
        float2 hb = rect.zw * 0.5;
        float d = sd_box(p - (rect.xy + hb), hb);
        int kind = int(blk.kind);
        if (kind == ARKOS_TITLE_BAR_BLOCK || kind == ARKOS_TAB_BAR_BLOCK || kind == ARKOS_SIDEBAR_BLOCK) {
            float amount = kind == ARKOS_SIDEBAR_BLOCK ? 0.35 : 0.42;
            dim = max(dim, (1.0 - smoothstep(-0.5, 0.5, d)) * amount);
        } else if (kind == ARKOS_ROW_HIGHLIGHT_BLOCK || kind == ARKOS_TAB_HIGHLIGHT_BLOCK) {
            if (d < 6.0) {
                float s = HIGHLIGHT_STRENGTH[min(max(int(blk.ordinal), 0), 2)] * blk.value;
                float fill = (1.0 - smoothstep(-1.5, 1.0, d)) + 0.35 * (1.0 - smoothstep(0.0, 6.0, d));
                lift = max(lift, s * saturate(fill));
            }
        }
        nearest_edge(p, blk, d, hit);
    }
    col = mix(col, bg * 0.8, dim * k);
    col = mix(col, green, lift * k);
    // Every reported edge a glow line in the ring's colour.
    Glow g = edge_glow(hit, ring, white_c);
    col = mix(col, ring, saturate(g.halo * k));
    col = mix(col, g.core_col, g.core * k);
    return float4(col, 1.0);
}

// The menu stage: the open menus' edges, drawn above the panes with the panes' own glow, and
// nothing else. Its blocks are only the menus, each with its fade as its value. It returns colour
// multiplied by its alpha: the halo laid in the ring's colour and the core over it.
fragment float4 arkos_overlay(ArkosVertex in [[stage_in]], constant ArkosInputs& u [[buffer(0)]]) {
    float2 p = in.uv * u.size;
    float k = saturate(u.intensity);
    float3 ring = u.colors[ARKOS_FOCUS_RING].rgb;
    float3 white_c = u.colors[ARKOS_ANSI + 15].rgb;
    int nb = min(int(u.block_count), 48);
    EdgeHit hit = no_edge();
    for (int i = 0; i < nb; i++) {
        ArkosBlock blk = u.blocks[i];
        float4 r = blk.rect;
        float2 hb = r.zw * 0.5;
        nearest_edge(p, blk, sd_box(p - (r.xy + hb), hb), hit);
    }
    if (hit.d >= EDGE_REACH) {
        return float4(0.0);
    }
    Glow g = edge_glow(hit, ring, white_c);
    // The edges fade with their menu (blk.value, carried in the hit).
    float fade = k * hit.value;
    float ah = saturate(g.halo * fade);
    float ac = g.core * fade;
    float a = ac + ah * (1.0 - ac);
    return float4(g.core_col * ac + ring * ah * (1.0 - ac), a);
}
