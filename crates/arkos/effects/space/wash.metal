// The space wash under the Arkos window: a night sky made from the theme's colours, with
// twinkling stars, a faint nebula, a slow planet and the odd shooting star; and for the UI's blocks, a glow line on every edge the app no longer draws, a
// soft glow fill on the highlights and a faint darkening under the bars. The preamble
// (arkos.metal) is prepended before this is compiled, and declares ArkosInputs, ArkosBlock,
// ArkosVertex and the ARKOS_* indices used here.
//
// Every event is a pure function of the clock, so Reduce Motion (which freezes it) shows a still
// picture. The clock's zero falls between two shooting stars. (An aurora was removed on
// 2026-10-28: Cyrus did not like it.)

// The pane edges' glow: the halo's strength and reach in points, and how much a passing bar
// brightens it; the line's own strength and how far it leans from the ring's blue to the star
// colour; and how much further a passing bar leans it. The leans are kept small so the line and
// the bars stay close to the halo's blue.
constant float EDGE_HALO = 0.75;
constant float EDGE_HALO_REACH = 2.0;
constant float EDGE_SHIMMER = 0.5;
constant float EDGE_CORE = 0.7;
constant float EDGE_CORE_WHITE = 0.15;
constant float EDGE_BAR_WHITE = 0.05;
// Where a bar passes the halo also glows brighter (EDGE_BAR_GLOW more, at full cover) and reaches
// further (EDGE_BAR_REACH more). Edges are looked for out to EDGE_REACH points, and the halo is
// faded to nothing over the last 3 points of that, so it never ends in a step.
constant float EDGE_BAR_GLOW = 1.5;
constant float EDGE_BAR_REACH = 0.5;
constant float EDGE_REACH = 12.0;

// The bars that sweep the edges, after Material UI's LinearProgress, variant "query": its long and
// short indeterminate bars (CSS left and right as fractions of the edge, from and to, each eased by
// its own cubic-bezier over a 1.26 s sweep, each 1.15 s behind the last), run from the far end, so
// right to left along a horizontal edge and bottom to top along a vertical one. A bar's ends are
// tapered inward, so a bar resting just off an edge lights none of it: each end over 40% of the
// bar's length, between QUERY_SOFT and QUERY_TAPER_MAX points, so the bar blends into the line.
// While a shimmer plays its bars pulse, 1.4 times a second between half and full brightness, each
// shimmer at a phase of its own.
//
// Unlike Material's, they come as shimmers that vary: one a 3.5 s slot, starting up to 2 s into
// it (1.5 to 5.5 s apart), each with its own speed (0.8 to 1.5 times Material's), its own number
// of bars (1 to 3, long and short alternating) and its own pick of edges (each edge a 55% chance).
// A shimmer can run on into the next slot, so the previous slot's is drawn too; none runs past
// the slot after that. Mirrored in crates/arkos/src/effects/space.rs (shimmers_at, shimmer_bar,
// edge_key, edge_shows), which tests it: a change here is made there too.
constant float QUERY_SWEEP = 1.26;
constant float QUERY_STEP = 1.15;
constant float2 QUERY_FROM[2] = {float2(-0.35, 1.0), float2(-2.0, 1.0)};
constant float2 QUERY_TO[2] = {float2(1.0, -0.9), float2(1.07, -0.08)};
constant float4 QUERY_EASE[2] = {float4(0.65, 0.815, 0.735, 0.395), float4(0.165, 0.84, 0.44, 1.0)};
constant float QUERY_SOFT = 3.0;
constant float QUERY_TAPER = 0.4;
constant float QUERY_TAPER_MAX = 60.0;
constant float SHIMMER_PULSE_LOW = 0.5;
constant float SHIMMER_PULSE_HZ = 1.4;
constant float SHIMMER_SLOT = 3.5;
constant float SHIMMER_SLACK = 2.0;
constant float SHIMMER_OFFSET = 0.1;
constant float SHIMMER_SPEED_MIN = 0.8;
constant float SHIMMER_SPEED_MAX = 1.5;
constant float SHIMMER_EDGE_CHANCE = 0.55;
constant uint SHIMMER_SALT = 0xC2B2AE35u;

// The event schedule. It is mirrored in crates/arkos/src/effects/space.rs (meteor_visible),
// which tests it: a change here is made there too.
constant float METEOR_CYCLE = 12.0;
constant float METEOR_SLACK = 4.0;
constant float METEOR_FLIGHT = 0.8;
constant float METEOR_OFFSET = 6.0;
constant uint METEOR_SALT = 0x9E3779B9u;

// The planets: one at a time, crossing over five minutes, then a minute of clear sky. The clock's
// zero finds one part way across, so a still picture has it.
constant float PLANET_CYCLE = 360.0;
constant float PLANET_CROSSING = 300.0;
constant float PLANET_OFFSET = 120.0;
constant uint PLANET_SALT = 0x85EBCA6Bu;

// A hash of n in 0 to 1, in integer arithmetic (PCG's output step) so the Rust mirror agrees to
// the bit: the top 24 bits, times a power of two, are exact in a float.
float hash01(uint n) {
    uint h = n * 747796405u + 2891336453u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    h ^= h >> 22u;
    return float(h >> 8u) * (1.0 / 16777216.0);
}

float hash2(float2 p) {
    return fract(sin(dot(p, float2(12.9898, 78.233))) * 43758.5453);
}

float hash1(float n) { return fract(sin(n) * 43758.5453123); }

float vnoise(float2 p) {
    float2 i = floor(p);
    float2 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    float n = i.x + i.y * 57.0;
    return mix(mix(hash1(n), hash1(n + 1.0), f.x), mix(hash1(n + 57.0), hash1(n + 58.0), f.x), f.y);
}

float fbm(float2 p) {
    float v = 0.0;
    float a = 0.5;
    for (int i = 0; i < 3; i++) {
        v += a * vnoise(p);
        p *= 2.03;
        a *= 0.5;
    }
    return v;
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

// One coordinate of a CSS cubic Bezier from (0, 0) to (1, 1), and the easing it makes: x found by
// sixteen halvings, then y read off. Its ends are exact, so a bar at rest sits exactly off the edge.
float bezier_coord(float a, float b, float s) {
    float u = 1.0 - s;
    return 3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s;
}
float css_ease(float4 c, float x) {
    if (x <= 0.0) { return 0.0; }
    if (x >= 1.0) { return 1.0; }
    float lo = 0.0, hi = 1.0;
    for (int i = 0; i < 16; i++) {
        float mid = 0.5 * (lo + hi);
        if (bezier_coord(c.x, c.z, mid) < x) { lo = mid; } else { hi = mid; }
    }
    return bezier_coord(c.y, c.w, 0.5 * (lo + hi));
}

// A bar of kind k (0 the long bar, 1 the short) x seconds into its sweep at speed 1: its start and
// end as fractions along the edge from where the bars come in.
float2 query_bar(int k, float x) {
    float e = css_ease(QUERY_EASE[k], saturate(x / QUERY_SWEEP));
    float2 lr = mix(QUERY_FROM[k], QUERY_TO[k], e);
    return float2(lr.x, 1.0 - lr.y);
}

// An edge's key, from the point it starts at and its side's bit, and whether it takes part in
// the shimmer with hash id.
uint edge_key(float x, float y, uint side) {
    uint ux = uint(max(x, 0.0) + 0.5);
    uint uy = uint(max(y, 0.0) + 0.5);
    return (ux * 73856093u) ^ (uy * 19349663u) ^ (side * 83492791u);
}
bool edge_shows(uint key, uint id) {
    return hash01(key ^ (id * 0x9E3779B1u)) < SHIMMER_EDGE_CHANCE;
}

// How much of a bar lies at s points along an edge span points long, its ends softened.
float bar_cover(float2 bar, float s, float span) {
    float a = bar.x * span;
    float b = bar.y * span;
    float taper = clamp(QUERY_TAPER * (b - a), QUERY_SOFT, QUERY_TAPER_MAX);
    return smoothstep(a, a + taper, s) * (1.0 - smoothstep(b - taper, b, s));
}

// How bright shimmer id's bars are since seconds after it began.
float shimmer_pulse(uint id, float since) {
    float wave = 0.5 + 0.5 * cos(6.2831853 * (SHIMMER_PULSE_HZ * since + hash01(id + 3u)));
    return SHIMMER_PULSE_LOW + (1.0 - SHIMMER_PULSE_LOW) * wave;
}

// The stars' three depth layers: the cell each star lives in (points), the chance a cell has
// one, its radius and its brightness. The far layer is dense, small and dim.
constant float STAR_CELL[3] = {19.0, 37.0, 71.0};
constant float STAR_CHANCE[3] = {0.82, 0.8, 0.78};
constant float STAR_RADIUS[3] = {0.8, 1.1, 1.5};
constant float STAR_BRIGHT[3] = {0.35, 0.6, 0.9};

// A highlight's strength at full fade, by its ordinal: hovered, selected, both (watercolor's).
constant float HIGHLIGHT_STRENGTH[3] = {0.2, 0.3, 0.36};

// The edge nearest a pixel so far: how far it is, its length, how far along it the pixel lies
// from where the bars come in, its key, and its block's fade.
struct EdgeHit {
    float d;
    float span;
    float along;
    uint key;
    float value;
};

EdgeHit no_edge() {
    EdgeHit h;
    h.d = 1e5;
    h.span = 1.0;
    h.along = 0.0;
    h.key = 0u;
    h.value = 1.0;
    return h;
}

// Takes blk's reported edges into h when one is nearer p; d is p's distance to blk's box.
void nearest_edge(float2 p, ArkosBlock blk, float d, thread EdgeHit& h) {
    uint edges = uint(blk.edges);
    if (edges != 0u && abs(d) < EDGE_REACH) {
        float4 r = blk.rect;
        float x0 = r.x, y0 = r.y, x1 = r.x + r.z, y1 = r.y + r.w;
        float ah = x1 - clamp(p.x, x0, x1);
        float av = y1 - clamp(p.y, y0, y1);
        float e;
        if (edges & ARKOS_EDGE_TOP) { e = seg_h(p, y0, x0, x1); if (e < h.d) { h.d = e; h.span = r.z; h.along = ah; h.key = edge_key(x0, y0, ARKOS_EDGE_TOP); h.value = blk.value; } }
        if (edges & ARKOS_EDGE_RIGHT) { e = seg_v(p, x1, y0, y1); if (e < h.d) { h.d = e; h.span = r.w; h.along = av; h.key = edge_key(x1, y0, ARKOS_EDGE_RIGHT); h.value = blk.value; } }
        if (edges & ARKOS_EDGE_BOTTOM) { e = seg_h(p, y1, x0, x1); if (e < h.d) { h.d = e; h.span = r.z; h.along = ah; h.key = edge_key(x0, y1, ARKOS_EDGE_BOTTOM); h.value = blk.value; } }
        if (edges & ARKOS_EDGE_LEFT) { e = seg_v(p, x0, y0, y1); if (e < h.d) { h.d = e; h.span = r.w; h.along = av; h.key = edge_key(x0, y0, ARKOS_EDGE_LEFT); h.value = blk.value; } }
    }
}

// How much of a query bar lies at the nearest edge's point at time t: the previous slot's
// shimmer (still finishing) and the current one's, each on its own pick of edges.
float bar_cover_at(float t, EdgeHit h) {
    float dline = h.d;
    float along = h.along;
    float span = h.span;
    float cover = 0.0;
    if (dline < EDGE_REACH) {
        float ts = t + SHIMMER_OFFSET;
        float now_slot = floor(ts / SHIMMER_SLOT);
        for (int back = 1; back >= 0; back--) {
            float sn = now_slot - float(back);
            if (sn < 0.0) { continue; }
            uint sid = uint(sn) ^ SHIMMER_SALT;
            float since = ts - sn * SHIMMER_SLOT - SHIMMER_SLACK * hash01(sid);
            if (since <= 0.0 || !edge_shows(h.key, sid)) { continue; }
            float speed = mix(SHIMMER_SPEED_MIN, SHIMMER_SPEED_MAX, hash01(sid + 1u));
            uint bars = 1u + min(uint(hash01(sid + 2u) * 3.0), 2u);
            float pulse = shimmer_pulse(sid, since);
            for (uint j = 0u; j < bars; j++) {
                float2 bar = query_bar(int(j % 2u), since * speed - float(j) * QUERY_STEP);
                cover = max(cover, bar_cover(bar, along, span) * pulse);
            }
        }
    }
    return cover;
}

// The glow line at the nearest edge: a bright core a little over a point wide and a soft halo a
// couple of points out, both brightened where a query bar passes (cover), the halo also wider
// there. The halo is laid in the ring's colour and the core over it in core_col; their strengths
// are before the user's intensity.
struct Glow {
    float halo;
    float core;
    float3 core_col;
};

Glow edge_glow(EdgeHit h, float cover, float3 ring, float3 star_c) {
    float dline = h.d;
    float shimmer = cover * EDGE_SHIMMER;
    Glow g;
    g.core = saturate((1.0 - smoothstep(0.5, 1.1, dline)) * (EDGE_CORE + 0.25 * shimmer));
    g.halo = exp(-dline / (EDGE_HALO_REACH * (1.0 + EDGE_BAR_REACH * cover))) * EDGE_HALO * (1.0 + EDGE_BAR_GLOW * cover)
        * (1.0 - smoothstep(EDGE_REACH - 3.0, EDGE_REACH, dline));
    float lean = EDGE_CORE_WHITE + EDGE_BAR_WHITE * shimmer;
    g.core_col = mix(ring, star_c, lean);
    return g;
}

fragment float4 arkos_wash(ArkosVertex in [[stage_in]], constant ArkosInputs& u [[buffer(0)]]) {
    float2 p = in.uv * u.size;
    float W = max(u.size.x, 1.0);
    float H = max(u.size.y, 1.0);
    float t = u.time;
    float k = saturate(u.intensity);

    float3 bg = u.colors[ARKOS_WINDOW_BG].rgb;
    float3 star_c = u.colors[ARKOS_ANSI + 15].rgb;
    float3 magenta = u.colors[ARKOS_ANSI + 5].rgb;
    float3 cyan = u.colors[ARKOS_ANSI + 6].rgb;

    // The sky: the window's background, slightly deepened toward the top.
    float3 col = bg * mix(1.0, 0.72 + 0.28 * smoothstep(0.0, H, p.y), k);

    // A faint nebula: two slow noise fields, one magenta, one cyan.
    float n1 = fbm(p * 0.0045 + float2(t * 0.004, 0.0));
    float n2 = fbm(p * 0.0065 + float2(5.2, 1.3) - float2(0.0, t * 0.003));
    col = mix(col, magenta, smoothstep(0.45, 0.85, n1) * 0.09 * k);
    col = mix(col, cyan, smoothstep(0.5, 0.9, n2) * 0.06 * k);

    // Stars: one cell's star at most per layer, kept clear of the cell's sides so no neighbour
    // needs looking at; each twinkles at its own rate, a period of four to ten seconds.
    for (int layer = 0; layer < 3; layer++) {
        float cell = STAR_CELL[layer];
        float2 g = floor(p / cell) + float(layer) * 131.0;
        float s = hash2(g);
        if (s > STAR_CHANCE[layer]) {
            float2 at = (floor(p / cell) + 0.2 + 0.6 * float2(hash2(g + 1.7), hash2(g + 2.9))) * cell;
            float d = length(p - at);
            float r = STAR_RADIUS[layer];
            if (d < r * 2.0) {
                float tw = 0.6 + 0.4 * sin(t * (0.63 + 0.94 * hash2(g + 5.3)) + s * 50.0);
                col = mix(col, star_c, smoothstep(r * 1.6, 0.0, d) * tw * STAR_BRIGHT[layer] * k);
            }
        }
    }

    // A planet: a small shaded disc with a lit limb, crossing over several minutes.
    float tp = t + PLANET_OFFSET;
    float pn = floor(tp / PLANET_CYCLE);
    float px = tp - pn * PLANET_CYCLE;
    if (px < PLANET_CROSSING) {
        uint id = uint(pn) ^ PLANET_SALT;
        float r = H * (0.02 + 0.02 * hash01(id));
        float f = px / PLANET_CROSSING;
        f = hash01(id + 1u) > 0.5 ? 1.0 - f : f;
        float2 c = float2(mix(-r * 1.5, W + r * 1.5, f), H * (0.16 + 0.5 * hash01(id + 2u)) + sin(f * 3.0) * H * 0.02);
        float2 d = (p - c) / r;
        float rr = length(d);
        if (rr < 1.6) {
            float pick = hash01(id + 3u);
            float3 pc = pick < 0.25 ? u.colors[ARKOS_ANSI + 4].rgb
                      : pick < 0.5 ? magenta
                      : pick < 0.75 ? u.colors[ARKOS_ANSI + 3].rgb
                      : cyan;
            float3 nrm = float3(d, sqrt(max(1.0 - rr * rr, 0.0)));
            float lit = saturate(dot(nrm, normalize(float3(-0.6, -0.5, 0.62))));
            float body = 1.0 - smoothstep(1.0 - 1.0 / r, 1.0, rr);
            float limb = smoothstep(0.72, 0.98, rr) * lit;
            float3 shade = pc * (0.12 + 0.62 * lit) + pc * limb * 0.45;
            // A faint haze of its own colour just past its edge.
            float haze = (1.0 - smoothstep(1.0, 1.6, rr)) * (1.0 - body) * 0.12;
            col = mix(col, pc, haze * k);
            col = mix(col, shade, body * 0.85 * k);
        }
    }

    // A shooting star: a short streak with a fading tail, fading in and out across its flight.
    float tm = t + METEOR_OFFSET;
    float mn = floor(tm / METEOR_CYCLE);
    uint mid = uint(mn) ^ METEOR_SALT;
    float mx = tm - mn * METEOR_CYCLE - METEOR_SLACK * hash01(mid);
    if (mx >= 0.0 && mx < METEOR_FLIGHT) {
        float f = mx / METEOR_FLIGHT;
        float side = hash01(mid + 3u) > 0.5 ? 1.0 : -1.0;
        float2 dir = normalize(float2(side * (0.8 + 0.6 * hash01(mid + 4u)), 0.4 + 0.3 * hash01(mid + 5u)));
        float2 start = float2(W * (0.2 + 0.6 * hash01(mid + 1u)), H * (0.05 + 0.35 * hash01(mid + 2u)));
        float travel = 0.3 * min(W, H) + 80.0;
        float2 head = start + dir * travel * f;
        float2 rel = p - head;
        float along = -dot(rel, dir);
        float across = abs(rel.x * dir.y - rel.y * dir.x);
        float tail = 70.0;
        if (along > -3.0 && along < tail && across < 4.0) {
            float behind = 1.0 - saturate(along / tail);
            float w = 0.5 + 0.6 * behind;
            float streak = exp(-across * across / (w * w)) * behind * behind * step(0.0, along);
            float head_glow = exp(-dot(rel, rel) / 2.5);
            float life = sin(f * 3.14159);
            col = mix(col, star_c, saturate((0.8 * streak + 0.6 * head_glow) * life * k));
        }
    }

    // The blocks: the bars darkened a little, the highlights glowing, every reported edge a line.
    int nb = min(int(u.block_count), 48);
    float dim = 0.0;
    float lift = 0.0;
    EdgeHit hit = no_edge();
    for (int i = 0; i < nb; i++) {
        ArkosBlock blk = u.blocks[i];
        float4 r = blk.rect;
        float2 hb = r.zw * 0.5;
        float d = sd_box(p - (r.xy + hb), hb);
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
    float3 ring = u.colors[ARKOS_FOCUS_RING].rgb;
    col = mix(col, bg * 0.8, dim * k);
    col = mix(col, ring, lift * k);
    // Every reported edge a glow line, with the query bars sweeping it.
    Glow g = edge_glow(hit, bar_cover_at(t, hit), ring, star_c);
    col = mix(col, ring, saturate(g.halo * k));
    col = mix(col, g.core_col, g.core * k);
    return float4(col, 1.0);
}

// The menu stage: the open menus' edges, drawn above the panes with the panes' own glow and
// bars, and nothing else. Its blocks are only the menus, each with its fade as its value. It
// returns colour multiplied by its alpha: the halo laid in the ring's colour and the core over it.
fragment float4 arkos_overlay(ArkosVertex in [[stage_in]], constant ArkosInputs& u [[buffer(0)]]) {
    float2 p = in.uv * u.size;
    float k = saturate(u.intensity);
    float3 ring = u.colors[ARKOS_FOCUS_RING].rgb;
    float3 star_c = u.colors[ARKOS_ANSI + 15].rgb;
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
    Glow g = edge_glow(hit, bar_cover_at(u.time, hit), ring, star_c);
    // The edges fade with their menu (blk.value, carried in the hit).
    float fade = k * hit.value;
    float ah = saturate(g.halo * fade);
    float ac = g.core * fade;
    float a = ac + ah * (1.0 - ac);
    return float4(g.core_col * ac + ring * ah * (1.0 - ac), a);
}
