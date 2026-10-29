// The watercolor wash under the Arkos window: paper, bubbles, and a wash with an ink outline for
// every block of the UI. The preamble (arkos.metal) is prepended before this is compiled, and
// declares ArkosInputs, ArkosBlock, ArkosVertex and the ARKOS_* indices used here.

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

// The four pigments are the theme's own (ARKOS_PIGMENT): its focus colour and accents of its own
// family, which thin out over the paper (Peach Light's burnt orange, ochre, brick and plum into
// peach, apricot, dusty rose and mauve; Lavender Light's purples, rose and blue into lilacs).
float3 pigment(constant ArkosInputs& u, int i) {
    return u.colors[ARKOS_PIGMENT + clamp(i, 0, 3)].rgb;
}

// Each pane its own pigment, by its place among the panes, as each card in a sketch gets its own
// colour.
constant int PANE_PIGMENTS[4] = {0, 1, 3, 2};

// A highlight's strength at full fade, by its ordinal: hovered, selected, both.
constant float HIGHLIGHT_STRENGTH[3] = {0.2, 0.3, 0.36};

// How strongly a block is washed, before its fade and the user's intensity, and with which
// pigment. A divider is an ink stroke with no wash.
float strength_of(ArkosBlock b, thread int& tint) {
    int kind = int(b.kind);
    int ordinal = max(int(b.ordinal), 0);
    tint = 0;
    if (kind == ARKOS_TITLE_BAR_BLOCK) { return 0.46; }
    if (kind == ARKOS_SIDEBAR_BLOCK) { tint = 2; return 0.3; }
    if (kind == ARKOS_TAB_BAR_BLOCK) { tint = PANE_PIGMENTS[ordinal % 4]; return 0.56; }
    if (kind == ARKOS_PANE_BODY_BLOCK) { tint = PANE_PIGMENTS[ordinal % 4]; return 0.14; }
    // A menu's backdrop: a pane body's light wash, in the first pigment.
    if (kind == ARKOS_MENU_BLOCK) { return 0.14; }
    // A table: a light wash over its body and an apricot band over its header, as a ruled
    // sheet in a sketchbook is tinted.
    if (kind == ARKOS_TABLE_BLOCK) { tint = 3; return 0.1; }
    if (kind == ARKOS_TABLE_HEADER_BLOCK) { tint = 1; return 0.34; }
    if (kind == ARKOS_ROW_HIGHLIGHT_BLOCK || kind == ARKOS_TAB_HIGHLIGHT_BLOCK) {
        return HIGHLIGHT_STRENGTH[min(ordinal, 2)];
    }
    return 0.0;
}

// Where bubble i is at t seconds, in units where the window is `aspect` wide and 1 high: its
// centre in xy and its radius in z. Each sways slowly and drifts slower still across, wrapping
// with a margin so it leaves before it comes back.
float3 bubble(int i, float t, float aspect) {
    float fi = float(i);
    float hx = hash1(fi * 7.31);
    float hy = hash1(fi * 7.31 + 1.0);
    float sx = hash1(fi * 3.17 + 11.0);
    float sy = hash1(fi * 3.17 + 12.0);
    float speed = 0.05 + 0.07 * sx;
    float x = hx * aspect + sin(t * speed + fi) * 0.08;
    float y = hy + cos(t * speed * 0.8 + fi * 1.7) * 0.06;
    float span = aspect + 0.5;
    x += t * 0.006 * (0.5 + sy);
    x = x - span * floor(x / span) - 0.25;
    return float3(x, y, 0.1 + 0.14 * sy);
}

constant int BUBBLES = 14;

float sd_box(float2 q, float2 hb) {
    float2 d = abs(q) - hb;
    return length(max(d, 0.0)) + min(max(d.x, d.y), 0.0);
}

// The distance from p to an ink line along one edge: a horizontal (or vertical) segment from
// x0 to x1 at y, overshooting its corners a little as a quick pen stroke does.
float ink_h(float2 p, float y, float x0, float x1, float wob, float over) {
    float dx = max(max(x0 - over - p.x, p.x - x1 - over), 0.0);
    return length(float2(dx, p.y - y - wob));
}
float ink_v(float2 p, float x, float y0, float y1, float wob, float over) {
    float dy = max(max(y0 - over - p.y, p.y - y1 - over), 0.0);
    return length(float2(p.x - x - wob, dy));
}

// The pen at a point: its wobble, its weight, where it skips (gap) and the paper's speck under it.
// All drift very slowly, so the edges breathe.
struct Pen {
    float wob;
    float weight;
    float gap;
    float speck;
};

Pen pen_at(float2 p, float t) {
    Pen pen;
    pen.speck = vnoise(p * 0.9);
    pen.wob = (vnoise(p * 0.035 + t * 0.05) - 0.5) * 2.6;
    pen.weight = 0.55 + 0.55 * vnoise(p * 0.02 + 7.0);
    pen.gap = smoothstep(0.08, 0.2, vnoise(p * 0.028 + 3.0));
    return pen;
}

// How much ink blk's reported edges lay at p: a crisp core with a thin feathered bleed beside
// it, as ink spreads into damp paper, each line overshooting its corners as a quick pen stroke
// does (by the block's own seed).
float ink_for_block(float2 p, ArkosBlock blk, Pen pen) {
    uint edges = uint(blk.edges);
    if (edges == 0u) {
        return 0.0;
    }
    float4 r = blk.rect;
    float seed = blk.seed * 1.7;
    float x0 = r.x, y0 = r.y, x1 = r.x + r.z, y1 = r.y + r.w;
    float over = 3.0 + 5.0 * hash1(seed);
    float dmin = 1e5;
    if (edges & ARKOS_EDGE_TOP) { dmin = min(dmin, ink_h(p, y0, x0, x1, pen.wob, over)); }
    if (edges & ARKOS_EDGE_RIGHT) { dmin = min(dmin, ink_v(p, x1, y0, y1, pen.wob, over)); }
    if (edges & ARKOS_EDGE_BOTTOM) { dmin = min(dmin, ink_h(p, y1, x0, x1, pen.wob, over)); }
    if (edges & ARKOS_EDGE_LEFT) { dmin = min(dmin, ink_v(p, x0, y0, y1, pen.wob, over)); }
    float core = 1.0 - smoothstep(pen.weight - 0.3, pen.weight + 0.3, dmin);
    float halo = (1.0 - smoothstep(pen.weight, pen.weight + 2.2, dmin)) * 0.22;
    return max(core, halo) * pen.gap;
}

// Dry-brush: the pen skips a little on the paper's tooth, more on lighter strokes.
float ink_tooth(Pen pen) {
    return 0.78 + 0.22 * smoothstep(0.25, 0.75, pen.speck);
}

fragment float4 arkos_wash(ArkosVertex in [[stage_in]], constant ArkosInputs& u [[buffer(0)]]) {
    float2 p = in.uv * u.size;
    float t = u.time;
    float k = saturate(u.intensity);

    // Shared fields, computed once per pixel and read by every block: the ragged bleed (wider
    // across than down, as a brush stroke runs), fine edge fray, granulation, and the pen's
    // wobble and weight. All drift very slowly, so the edges breathe.
    float bleed = fbm(float2(p.x * 0.018, p.y * 0.05) + t * 0.012);
    float fray = vnoise(p * 0.16 + t * 0.03);
    float gran = fbm(p * 0.045 - t * 0.008);
    Pen pen = pen_at(p, t);
    float speck = pen.speck;

    // Paper with grain.
    float3 col = u.colors[ARKOS_WINDOW_BG].rgb * mix(1.0, 0.992 + 0.012 * speck + 0.008 * gran, k);

    // Bubbles behind everything.
    float aspect = u.size.x / max(u.size.y, 1.0);
    float2 bp = p / max(u.size.y, 1.0);
    for (int i = 0; i < BUBBLES; i++) {
        float3 b = bubble(i, t, aspect);
        float r = b.z;
        float dist = length(bp - b.xy) + (bleed - 0.5) * 0.3 * r;
        if (dist > r * 1.05) {
            continue;
        }
        float body = 1.0 - smoothstep(r * 0.5, r, dist);
        float rim = smoothstep(r * 0.75, r * 0.96, dist) * (1.0 - smoothstep(r * 0.96, r * 1.02, dist));
        float a = (0.32 * body + 0.28 * rim) * (0.65 + 0.5 * gran) * k;
        col *= mix(float3(1.0), bright(pigment(u, i % 4)), clamp(a, 0.0, 0.9));
    }

    // Washes, then ink, for every block.
    int nb = min(int(u.block_count), 48);
    float inked = 0.0;
    for (int i = 0; i < nb; i++) {
        ArkosBlock blk = u.blocks[i];
        // A highlight's rect is already its stroke, inset from its row or tab by the app.
        float4 r = blk.rect;
        float2 hb = r.zw * 0.5;
        float2 c = r.xy + hb;
        // The block's own seed (stable whatever else is on the paper): its grain and overshoot.
        float seed = blk.seed * 1.7;
        int tint;
        float strength = strength_of(blk, tint) * blk.value * k;
        if (strength > 0.0) {
            float d = sd_box(p - c, hb);
            if (d < 12.0) {
                // Close to the edge: a point or two past it, ragged only a little.
                float edge = d - 1.5 + (bleed - 0.5) * 8.0 + (fray - 0.5) * 2.0;
                float cover = 1.0 - smoothstep(-0.7, 0.7, edge);
                float pool = smoothstep(-5.0, -0.4, edge) * cover;
                float blotch = 0.72 + 0.5 * fract(gran + seed * 0.37);
                float a = strength * (cover * blotch + 0.5 * pool);
                col *= mix(float3(1.0), bright(pigment(u, tint)), clamp(a, 0.0, 0.85));
            }
        }
        // Strokes add up where they cross, so the corners pool darker.
        inked += ink_for_block(p, blk, pen);
    }
    col = mix(col, u.colors[ARKOS_TERMINAL_FG].rgb, clamp(inked * ink_tooth(pen), 0.0, 1.0) * 0.96 * k);
    return float4(col, 1.0);
}

// The menu stage: the open menus' edges in the panes' own ink, drawn above the panes, and
// nothing else. Its blocks are only the menus, each with its fade as its value. It returns
// colour multiplied by its alpha.
fragment float4 arkos_overlay(ArkosVertex in [[stage_in]], constant ArkosInputs& u [[buffer(0)]]) {
    float2 p = in.uv * u.size;
    float k = saturate(u.intensity);
    Pen pen = pen_at(p, u.time);
    int nb = min(int(u.block_count), 48);
    float inked = 0.0;
    for (int i = 0; i < nb; i++) {
        ArkosBlock blk = u.blocks[i];
        // The ink fades with its menu.
        inked += ink_for_block(p, blk, pen) * blk.value;
    }
    float a = clamp(inked * ink_tooth(pen), 0.0, 1.0) * 0.96 * k;
    return float4(u.colors[ARKOS_TERMINAL_FG].rgb * a, a);
}
