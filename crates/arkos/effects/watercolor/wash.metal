// The watercolor wash under the Arkos window: paper, bubbles, and a wash with an ink outline for
// every block of the UI. The preamble (arkos.metal) is prepended before this is compiled, and
// declares ArkosInputs, ArkosBlock, ArkosVertex and the ARKOS_* indices used here.
//
// It works on any theme, a user's own included: every colour comes from the theme's (the paper,
// the ink, its four pigments), and the theme's mode decides how they meet. On a light theme the
// pigments are thinned with water and laid over the paper, darkening it as a wash does; on a dark
// one (u.dark) they are a glaze, the pigment mixed into the dark paper so it shows. A theme with
// fewer than four pigments of its own gets the rest as variations of them. And however strong a
// theme's colours, the paper under the UI's text never falls below 3:1 against it (WCAG's floor
// for large text and controls).

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

// A colour's hue turned by `deg` degrees about its own grey (in YIQ), its lightness kept.
float3 turn_hue(float3 c, float deg) {
    float y = dot(c, float3(0.299, 0.587, 0.114));
    float i = dot(c, float3(0.596, -0.274, -0.322));
    float q = dot(c, float3(0.211, -0.523, 0.312));
    float a = deg * 0.0174533;
    float i2 = i * cos(a) - q * sin(a);
    float q2 = i * sin(a) + q * cos(a);
    return saturate(float3(y + 0.956 * i2 + 0.621 * q2, y - 0.272 * i2 - 0.647 * q2, y - 1.106 * i2 + 1.703 * q2));
}

// The four pigments are the theme's own (ARKOS_PIGMENT): its focus colour and accents of its own
// family, which thin out over the paper (Peach Watercolor's burnt orange, ochre, brick and plum into
// peach, apricot, dusty rose and mauve; Lavender Watercolor's purples, rose and blue into lilacs). A
// theme with fewer of its own (u.pigment_count) gets the rest as its own turned a little either
// way round the colour wheel, so neighbouring panes still differ.
float3 pigment(constant ArkosInputs& u, int i) {
    int n = clamp(int(u.pigment_count), 1, 4);
    int k = clamp(i, 0, 3);
    if (k < n) {
        return u.colors[ARKOS_PIGMENT + k].rgb;
    }
    float3 base = u.colors[ARKOS_PIGMENT + k % n].rgb;
    float step = float(k / n) * 32.0;
    return turn_hue(base, (k % 2 == 0) ? step : -step);
}

// A pigment laid over `col` at strength `a`, as the theme's mode has it: thinned and multiplied
// onto light paper; on dark paper a glaze, the pigment mixed in toward the paper's own depth.
float3 wash_onto(constant ArkosInputs& u, float3 col, float3 pig, float a) {
    if (u.dark > 0.5) {
        float3 paper = u.colors[ARKOS_WINDOW_BG].rgb;
        float3 glaze = mix(paper, pig, 0.6);
        return mix(col, glaze, a);
    }
    return col * mix(float3(1.0), bright(pig), a);
}

// An sRGB channel in linear light, and a colour's relative luminance (as WCAG measures it).
float3 to_linear(float3 c) {
    return select(pow((c + 0.055) / 1.055, 2.4), c / 12.92, c <= 0.03928);
}
float3 to_srgb(float3 c) {
    return select(1.055 * pow(c, 1.0 / 2.4) - 0.055, c * 12.92, c <= 0.0030402);
}
float rel_lum(float3 c) {
    return dot(to_linear(c), float3(0.2126, 0.7152, 0.0722));
}

// The least contrast the paper keeps against the UI's text, whatever the theme's colours.
constant float READABLE = 3.0;

// `col` moved back toward the paper, in linear light, just far enough that the UI's text keeps
// READABLE against it (or as much as the theme's own paper has, when that is less).
float3 keep_readable(constant ArkosInputs& u, float3 col) {
    float3 paper = u.colors[ARKOS_WINDOW_BG].rgb;
    float lt = rel_lum(u.colors[ARKOS_TEXT].rgb);
    float lc = rel_lum(col);
    float lp = rel_lum(paper);
    float f = 0.0;
    if (lt < lp) {
        // Dark text: the paper may not get darker than this.
        float least = READABLE * (lt + 0.05) - 0.05;
        if (lc < least) { f = saturate((least - lc) / max(lp - lc, 1e-5)); }
    } else {
        // Light text: the paper may not get lighter than this.
        float most = (lt + 0.05) / READABLE - 0.05;
        if (lc > most) { f = saturate((lc - most) / max(lc - lp, 1e-5)); }
    }
    return to_srgb(mix(to_linear(col), to_linear(paper), f));
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
    // The sidebar a light wash, so the Explorer's names read on near-white paper.
    if (kind == ARKOS_SIDEBAR_BLOCK) { tint = 2; return 0.12; }
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

// Pane 0's gallery (Cyrus, 2026-11-17: "a set of popular art in watercolor that slowly & softly
// transitions through the set; 1 at a time"): the effect's images (gallery/CREDITS.md, public
// domain), one at a time in the empty space under the sidebar's last row, each held
// GALLERY_HOLD seconds and then bloomed into the next over GALLERY_BLOOM, wet in wet: the next
// painting spreads out from a few pools of pigment that grow and merge, a darker rim where its
// wet front runs, while the last dries back to the paper.
constant float GALLERY_HOLD = 50.0;
constant float GALLERY_BLOOM = 6.0;
constant int GALLERY_POOLS = 4;
// The painting's margin in the space, how tall it may be against its width, and the least room
// under the rows it needs (it fades in over GALLERY_FADE more).
constant float GALLERY_MARGIN = 14.0;
constant float GALLERY_TALL = 1.35;
constant float GALLERY_MIN_H = 90.0;
constant float GALLERY_FADE = 40.0;

// Which painting shows at t, the next, and how far the bloom into it has come (0 to 1).
float3 gallery_at(float t, int n) {
    float period = GALLERY_HOLD + GALLERY_BLOOM;
    float cycle = floor(max(t, 0.0) / period);
    float into = max(t, 0.0) - cycle * period;
    float shown = fmod(cycle, float(n));
    float next = fmod(cycle + 1.0, float(n));
    float bloom = saturate((into - GALLERY_HOLD) / GALLERY_BLOOM);
    return float3(shown, next, bloom * bloom * (3.0 - 2.0 * bloom));
}

// Painting i fitted in the space under the rows, `foot`, at its bottom, centred across: its
// origin in xy, its size in zw.
float4 gallery_rect(float4 foot, int i) {
    float2 sz = max(ARKOS_IMAGE_SIZE[i], float2(1.0));
    float2 box = max(foot.zw - 2.0 * GALLERY_MARGIN, float2(1.0));
    box.y = min(box.y, box.x * GALLERY_TALL);
    float2 wh = sz * min(box.x / sz.x, box.y / sz.y);
    return float4(foot.x + (foot.z - wh.x) * 0.5, foot.y + foot.w - GALLERY_MARGIN - wh.y, wh);
}

// Painting i at p, as a watercolour of it: its colours softened, pooled darker where they meet,
// with the paper's grain through them, and its edge left ragged as a wash's. The colour in rgb,
// how much of the paper it covers in a.
float4 painting_at(texture2d_array<float> images, float2 p, int i, float4 r, float px, float gran) {
    constexpr sampler s(filter::linear, mip_filter::linear, address::clamp_to_edge);
    float2 hb = r.zw * 0.5;
    // Well off the painting, the paper alone: the noise and the samples are spent on it only.
    float box = sd_box(p - (r.xy + hb), hb);
    if (box > 6.0) {
        return float4(0.0);
    }
    float d = box + (vnoise(p * 0.11 + float(i) * 7.3) - 0.5) * 7.0;
    if (d > 2.0) {
        return float4(0.0);
    }
    float2 uv = saturate((p - r.xy) / r.zw);
    // A level a little softer than the pane's pixels ask for.
    float lod = max(log2(512.0 / max(r.z * px, 1.0)), 0.0) + 0.6;
    // Its colours run a little into each other, as wet paint does, along the paper's grain.
    float2 run = (float2(vnoise(p * 0.06), vnoise(p * 0.06 + 3.1)) - 0.5) * (3.0 / max(r.zw, float2(1.0)));
    float3 fine = images.sample(s, uv + run, uint(i), level(lod)).rgb;
    float3 soft = images.sample(s, uv + run, uint(i), level(lod + 2.0)).rgb;
    float meet = saturate(length(fine - soft) * 2.2);
    float3 c = mix(fine, soft, 0.45);
    // Transparent pigment: the darks lifted toward the paper, a few flat tones as washes lie,
    // pooled darker where two colours meet, the paper's grain through them.
    c = mix(c, float3(1.0), 0.14);
    c = mix(c, floor(c * 7.0 + 0.5) / 7.0, 0.35);
    c *= (1.0 - 0.32 * meet) * (0.92 + 0.16 * gran);
    float cover = 1.0 - smoothstep(-1.5, 1.5, d);
    // The edge pools darker, as a wash dries at its rim.
    float rim = smoothstep(-6.0, 0.0, d) * cover;
    return float4(c * (1.0 - 0.18 * rim), cover);
}

// Where the bloom into the next painting has reached at p: 1 inside, 0 outside, the pools
// seeded by the painting's number across `area`.
float bloom_at(float2 p, float4 area, float next, float bloom) {
    float reach = 1e9;
    for (int j = 0; j < GALLERY_POOLS; j++) {
        float2 seed = area.xy + area.zw * float2(hash1(next * 13.1 + float(j) * 3.7), hash1(next * 7.9 + float(j) * 5.3));
        reach = min(reach, length(p - seed));
    }
    // Four pools leave no point much more than half the painting from one: the bloom reaches
    // its last corner as it ends.
    reach = reach / (0.55 * max(max(area.z, area.w), 1.0)) + (fbm(p * 0.03 + next) - 0.5) * 0.45;
    return smoothstep(reach - 0.08, reach + 0.08, bloom * 1.3);
}

// A painting laid on the paper at strength a, as the theme's mode has it: multiplied into light
// paper as a wash is; on dark paper glazed over it.
float3 painting_onto(constant ArkosInputs& u, float3 col, float3 c, float a) {
    if (u.dark > 0.5) {
        return mix(col, c, a * 0.85);
    }
    return col * mix(float3(1.0), c, a);
}

fragment float4 arkos_wash(ArkosVertex in [[stage_in]], constant ArkosInputs& u [[buffer(0)]], texture2d_array<float> images [[texture(0)]]) {
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

    // Paper with grain: specks of darker fibre on light paper, of lighter on dark.
    float3 paper = u.colors[ARKOS_WINDOW_BG].rgb;
    float grain = 0.012 * speck + 0.008 * gran;
    float3 col = u.dark > 0.5 ? paper + (grain - 0.01) * k : paper * mix(1.0, 0.992 + grain, k);

    // Bubbles behind everything, a fifth as strong over the sidebar, which stays light.
    float bubble_k = 1.0;
    for (int i = 0; i < min(int(u.block_count), 48); i++) {
        ArkosBlock sb = u.blocks[i];
        if (int(sb.kind) == ARKOS_SIDEBAR_BLOCK && p.x >= sb.rect.x && p.x <= sb.rect.x + sb.rect.z && p.y >= sb.rect.y && p.y <= sb.rect.y + sb.rect.w) {
            bubble_k = 0.2;
        }
    }
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
        float a = (0.32 * body + 0.28 * rim) * (0.65 + 0.5 * gran) * k * bubble_k;
        col = wash_onto(u, col, pigment(u, i % 4), clamp(a, 0.0, 0.9));
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
                col = wash_onto(u, col, pigment(u, tint), clamp(a, 0.0, 0.85));
            }
        }
        // Strokes add up where they cross, so the corners pool darker.
        inked += ink_for_block(p, blk, pen);
    }
    // The text over the paper stays readable whatever the theme's colours did to it; the ink
    // lines, which carry no text, are drawn over that as they are.
    col = keep_readable(u, col);
    // The gallery, after the paper is kept readable: no text is over the space it hangs in.
    if (ARKOS_IMAGE_COUNT > 0) {
        for (int i = 0; i < nb; i++) {
            ArkosBlock blk = u.blocks[i];
            float4 foot = blk.rect;
            if (int(blk.kind) != ARKOS_SIDEBAR_FOOT_BLOCK || p.x < foot.x || p.x > foot.x + foot.z || p.y < foot.y || p.y > foot.y + foot.w) {
                continue;
            }
            float show = smoothstep(GALLERY_MIN_H, GALLERY_MIN_H + GALLERY_FADE, foot.w) * k;
            if (show <= 0.0) {
                break;
            }
            float3 at = gallery_at(t, ARKOS_IMAGE_COUNT);
            int shown = int(at.x);
            int next = int(at.y);
            float4 r0 = gallery_rect(foot, shown);
            float4 now_p = painting_at(images, p, shown, r0, u.scale, gran);
            float reached = 0.0;
            float4 next_p = float4(0.0);
            if (at.z > 0.0) {
                float4 r1 = gallery_rect(foot, next);
                float4 area = float4(min(r0.xy, r1.xy), max(r0.xy + r0.zw, r1.xy + r1.zw) - min(r0.xy, r1.xy));
                reached = bloom_at(p, area, at.y, at.z);
                next_p = painting_at(images, p, next, r1, u.scale, gran);
                // The wet front runs darker where the bloom is still spreading.
                float front = reached * (1.0 - reached) * 4.0 * (1.0 - at.z);
                next_p.rgb *= 1.0 - 0.2 * front;
            }
            col = painting_onto(u, col, now_p.rgb, now_p.a * (1.0 - reached) * show);
            col = painting_onto(u, col, next_p.rgb, next_p.a * reached * show);
            break;
        }
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
