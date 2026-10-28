// The space wash under the Arkos window: a night sky made from the theme's colours, with
// twinkling stars, a faint nebula, a slow planet, an aurora that comes and goes and the odd
// shooting star; and for the UI's blocks, a glow line on every edge the app no longer draws, a
// soft glow fill on the highlights and a faint darkening under the bars. The preamble
// (arkos.metal) is prepended before this is compiled, and declares ArkosInputs, ArkosBlock,
// ArkosVertex and the ARKOS_* indices used here.
//
// Every event is a pure function of the clock, so Reduce Motion (which freezes it) shows a still
// picture. The clock's zero falls in an aurora's hold and between two shooting stars.

// The event schedule. It is mirrored in crates/arkos/src/effects/space.rs (aurora_level and
// meteor_visible), which tests it: a change here is made there too.
constant float AURORA_CYCLE = 190.0;
constant float AURORA_SLACK = 30.0;
constant float AURORA_RISE = 20.0;
constant float AURORA_HOLD = 60.0;
constant float AURORA_FADE = 20.0;
constant float AURORA_OFFSET = 50.0;
constant float METEOR_CYCLE = 30.0;
constant float METEOR_SLACK = 10.0;
constant float METEOR_FLIGHT = 0.8;
constant float METEOR_OFFSET = 15.0;
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

// The aurora's strength at t, 0 (absent) to 1 (its hold): rising, holding, fading, then absent
// for the rest of its cycle.
float aurora_level(float t) {
    float tau = t + AURORA_OFFSET;
    float n = floor(tau / AURORA_CYCLE);
    float x = tau - n * AURORA_CYCLE - AURORA_SLACK * hash01(uint(n));
    float end = AURORA_RISE + AURORA_HOLD;
    return smoothstep(0.0, AURORA_RISE, x) * (1.0 - smoothstep(end, end + AURORA_FADE, x));
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

// The stars' three depth layers: the cell each star lives in (points), the chance a cell has
// one, its radius and its brightness. The far layer is dense, small and dim.
constant float STAR_CELL[3] = {19.0, 37.0, 71.0};
constant float STAR_CHANCE[3] = {0.82, 0.8, 0.78};
constant float STAR_RADIUS[3] = {0.8, 1.1, 1.5};
constant float STAR_BRIGHT[3] = {0.35, 0.6, 0.9};

// A highlight's strength at full fade, by its ordinal: hovered, selected, both (watercolor's).
constant float HIGHLIGHT_STRENGTH[3] = {0.2, 0.3, 0.36};

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
    float3 green = u.colors[ARKOS_ANSI + 2].rgb;

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

    // The aurora: a soft curtain in the top quarter, green at its hem and cyan above it.
    float level = aurora_level(t) * k;
    float top = H * 0.25;
    if (level > 0.0 && p.y < top) {
        float fy = p.y / top;
        float fold = vnoise(float2(p.x * 0.004 + t * 0.01, t * 0.02));
        float rays = vnoise(float2(p.x * 0.03 + fold * 6.0, t * 0.05));
        float hem = 0.55 + 0.33 * vnoise(float2(p.x * 0.006 - t * 0.012, 3.0));
        float shape = (1.0 - smoothstep(hem - 0.1, hem, fy)) * mix(0.2, 1.0, saturate(fy / hem)) * smoothstep(0.0, 0.12, fy);
        float3 ac = mix(cyan, green, saturate(fy / hem));
        col = mix(col, ac, shape * (0.35 + 0.65 * rays) * level * 0.3);
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
    float dline = 1e5;
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
        uint edges = uint(blk.edges);
        if (edges != 0u && abs(d) < 8.0) {
            float x0 = r.x, y0 = r.y, x1 = r.x + r.z, y1 = r.y + r.w;
            if (edges & ARKOS_EDGE_TOP) { dline = min(dline, seg_h(p, y0, x0, x1)); }
            if (edges & ARKOS_EDGE_RIGHT) { dline = min(dline, seg_v(p, x1, y0, y1)); }
            if (edges & ARKOS_EDGE_BOTTOM) { dline = min(dline, seg_h(p, y1, x0, x1)); }
            if (edges & ARKOS_EDGE_LEFT) { dline = min(dline, seg_v(p, x0, y0, y1)); }
        }
    }
    float3 ring = u.colors[ARKOS_FOCUS_RING].rgb;
    col = mix(col, bg * 0.8, dim * k);
    col = mix(col, ring, lift * k);
    // The glow line: a bright core about a point wide, and a soft halo a few points out.
    float core = 1.0 - smoothstep(0.35, 0.8, dline);
    float halo = exp(-dline / 2.2) * 0.45;
    col = mix(col, ring, saturate(halo * k));
    col = mix(col, mix(ring, star_c, 0.35), core * 0.9 * k);
    return float4(col, 1.0);
}
