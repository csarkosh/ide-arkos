// The watercolor wash under the Arkos window (src/effect.rs): paper, bubbles, and a wash with an
// ink outline for every block of the UI. Compiled at launch from this source.
#include <metal_stdlib>
using namespace metal;

struct Uniforms {
    float time;
    float bubble_count;
    float2 size;
    float4 paper;
    float4 ink;
    float4 tints[4];
    float4 bubbles[16];
    float region_count;
    float3 pad;
    float4 rects[32];
    float4 metas[32];
};

struct VOut {
    float4 pos [[position]];
    float2 uv;
};

vertex VOut watercolor_vertex(uint vid [[vertex_id]]) {
    float2 p = float2(vid == 1 ? 3.0 : -1.0, vid == 2 ? 3.0 : -1.0);
    VOut o;
    o.pos = float4(p, 0.0, 1.0);
    o.uv = float2((p.x + 1.0) * 0.5, 1.0 - (p.y + 1.0) * 0.5);
    return o;
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

// A pigment as it looks thinned with water: the theme's deep accent lifted 40% of the way to
// white, then pushed away from its own grey so it stays clear rather than dusty (ochre reads as
// golden apricot, plum as rosy lavender).
float3 bright(float3 c) {
    float3 lifted = 1.0 - (1.0 - c) * 0.6;
    float l = dot(lifted, float3(0.299, 0.587, 0.114));
    return saturate(mix(float3(l), lifted, 1.55));
}

float sd_box(float2 q, float2 hb) {
    float2 d = abs(q) - hb;
    return length(max(d, 0.0)) + min(max(d.x, d.y), 0.0);
}

// The distance from p to an ink line along one edge: a horizontal (or vertical) segment from
// a to b at `at`, overshooting its corners a little as a quick pen stroke does.
float ink_h(float2 p, float y, float x0, float x1, float wob, float over) {
    float dx = max(max(x0 - over - p.x, p.x - x1 - over), 0.0);
    return length(float2(dx, p.y - y - wob));
}
float ink_v(float2 p, float x, float y0, float y1, float wob, float over) {
    float dy = max(max(y0 - over - p.y, p.y - y1 - over), 0.0);
    return length(float2(p.x - x - wob, dy));
}

fragment float4 watercolor_fragment(VOut in [[stage_in]], constant Uniforms& u [[buffer(0)]]) {
    float2 p = in.uv * u.size;
    float t = u.time;

    // Shared fields, computed once per pixel and read by every block: the ragged bleed (wider
    // across than down, as a brush stroke runs), fine edge fray, granulation, and the pen's
    // wobble and weight. All drift very slowly, so the edges breathe.
    float bleed = fbm(float2(p.x * 0.018, p.y * 0.05) + t * 0.012);
    float fray = vnoise(p * 0.16 + t * 0.03);
    float gran = fbm(p * 0.045 - t * 0.008);
    float speck = vnoise(p * 0.9);
    float wob = (vnoise(p * 0.035 + t * 0.05) - 0.5) * 2.6;
    float weight = 0.55 + 0.55 * vnoise(p * 0.02 + 7.0);
    float gap = smoothstep(0.08, 0.2, vnoise(p * 0.028 + 3.0));

    // Paper with grain.
    float3 col = u.paper.rgb * (0.992 + 0.012 * speck + 0.008 * gran);

    // Bubbles behind everything.
    int nb = int(u.bubble_count);
    float2 bp = p / u.size.y;
    for (int i = 0; i < nb; i++) {
        float4 b = u.bubbles[i];
        float r = b.z;
        float dist = length(bp - b.xy) + (bleed - 0.5) * 0.3 * r;
        if (dist > r * 1.05) {
            continue;
        }
        float body = 1.0 - smoothstep(r * 0.5, r, dist);
        float rim = smoothstep(r * 0.75, r * 0.96, dist) * (1.0 - smoothstep(r * 0.96, r * 1.02, dist));
        float a = (0.32 * body + 0.28 * rim) * (0.65 + 0.5 * gran);
        col *= mix(float3(1.0), bright(u.tints[int(b.w)].rgb), clamp(a, 0.0, 0.9));
    }

    // Washes, then ink, for every block.
    int nr = int(u.region_count);
    float inked = 0.0;
    for (int i = 0; i < nr; i++) {
        float4 r = u.rects[i];
        float4 m = u.metas[i];
        float2 hb = r.zw * 0.5;
        float2 c = r.xy + hb;
        // The block's own seed (stable whatever else is on the paper): its grain and overshoot.
        float seed = m.w * 1.7;
        if (m.y > 0.0) {
            float d = sd_box(p - c, hb);
            if (d < 12.0) {
                // Close to the edge: a point or two past it, ragged only a little.
                float edge = d - 1.5 + (bleed - 0.5) * 8.0 + (fray - 0.5) * 2.0;
                float cover = 1.0 - smoothstep(-0.7, 0.7, edge);
                float pool = smoothstep(-5.0, -0.4, edge) * cover;
                float blotch = 0.72 + 0.5 * fract(gran + seed * 0.37);
                float a = m.y * (cover * blotch + 0.5 * pool);
                col *= mix(float3(1.0), bright(u.tints[int(m.x)].rgb), clamp(a, 0.0, 0.85));
            }
        }
        uint edges = uint(m.z);
        if (edges != 0u) {
            float x0 = r.x, y0 = r.y, x1 = r.x + r.z, y1 = r.y + r.w;
            float over = 3.0 + 5.0 * hash1(seed);
            float dmin = 1e5;
            if (edges & 1u) { dmin = min(dmin, ink_h(p, y0, x0, x1, wob, over)); }
            if (edges & 2u) { dmin = min(dmin, ink_v(p, x1, y0, y1, wob, over)); }
            if (edges & 4u) { dmin = min(dmin, ink_h(p, y1, x0, x1, wob, over)); }
            if (edges & 8u) { dmin = min(dmin, ink_v(p, x0, y0, y1, wob, over)); }
            // A crisp core with a thin feathered bleed beside it, as ink spreads into damp paper.
            float core = 1.0 - smoothstep(weight - 0.3, weight + 0.3, dmin);
            float halo = (1.0 - smoothstep(weight, weight + 2.2, dmin)) * 0.22;
            // Strokes add up where they cross, so the corners pool darker.
            inked += max(core, halo) * gap;
        }
    }
    // Dry-brush: the pen skips a little on the paper's tooth, more on lighter strokes.
    float tooth = 0.78 + 0.22 * smoothstep(0.25, 0.75, speck);
    col = mix(col, u.ink.rgb, clamp(inked * tooth, 0.0, 1.0) * 0.96);
    return float4(col, 1.0);
}
