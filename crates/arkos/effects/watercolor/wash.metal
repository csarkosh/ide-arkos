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
// Twelve seconds (Cyrus, 2026-11-17: the six second bloom was "too big and noticeable"; it should
// "blend better and smoother into the next painting").
constant float GALLERY_BLOOM = 12.0;
// How much the paint thins at the bloom's middle, so the two paintings never stand at full
// strength together.
constant float GALLERY_THIN = 0.18;
constant int GALLERY_POOLS = 4;

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

// The painter's studio the gallery hangs in (Cyrus, 2026-11-17: "a scene like a painter's private
// art studio with the painting as the main center piece", "a painter's private West Village loft
// in NYC", with "soft movements that adds liveliness to the scene without being distracting"),
// drawn in the space under the sidebar's rows in the theme's own pigments and ink, its edges
// melting into the sidebar's paper: a steel factory window onto brownstone rooftops, a water tower
// and a fire escape's rail, with clouds passing; a sheer drape; exposed brick where the plaster
// has come away; wide floorboards; the easel in the middle with the painting on it; a side table
// with a palette dabbed in the theme's pigments, a jar of brushes and a steaming mug; a potted
// plant; a pendant lamp on its cord. What moves, each slowly: the lamp sways on its cord, the drape
// stirs, the plant's leaves sway, the mug's steam rises, the light through the window breathes
// with dust drifting in it, and the clouds pass.
constant float STUDIO_TALL = 1.25;
constant float STUDIO_MIN_H = 120.0;
constant float STUDIO_FADE = 50.0;
// How far a painting is toned toward the theme's colours.
constant float STUDIO_TONE = 0.25;
// The soft motions: seconds a cycle, and how far.
constant float LAMP_SWAY_S = 9.0;
constant float LAMP_SWAY = 0.035;
constant float DRAPE_SWAY_S = 8.0;
constant float LEAF_SWAY_S = 7.0;
constant float STEAM_RISE_S = 5.0;
constant float BEAM_BREATH_S = 13.0;
constant float CLOUD_PASS_S = 150.0;
constant int MOTES = 9;
// The canvas on the easel: its centre across, how wide and tall it may be, and where its foot
// rests on the easel's ledge, as shares of the scene.
constant float CANVAS_X = 0.58;
constant float CANVAS_W = 0.5;
constant float CANVAS_H = 0.4;
constant float CANVAS_FOOT = 0.32;
constant float FLOOR_Y = 0.17;
// The board's margin round the painting, in points.
constant float BOARD_PAD = 5.0;
// How far a painting's edge wavers from straight, in points: a sheet cut by hand.
constant float PAINTING_WAVER = 0.8;

// The scene's frame: its bottom left in points, its width and height.
struct Studio {
    float2 o;
    float w;
    float h;
};

Studio studio_in(float4 foot) {
    Studio st;
    st.w = foot.z - 8.0;
    st.h = min(foot.w - 6.0, st.w * STUDIO_TALL);
    st.o = float2(foot.x + 4.0, foot.y + foot.w - 4.0);
    return st;
}

// A point of the scene, from its share across and up, and back.
float2 sp(Studio st, float2 n) {
    return float2(st.o.x + n.x * st.w, st.o.y - n.y * st.h);
}
float2 sn(Studio st, float2 p) {
    return float2((p.x - st.o.x) / st.w, (st.o.y - p.y) / st.h);
}

float wc_seg(float2 p, float2 a, float2 b) {
    float2 pa = p - a;
    float2 ba = b - a;
    float h = saturate(dot(pa, ba) / max(dot(ba, ba), 1e-4));
    return length(pa - ba * h);
}

// A pen stroke from a to b, `w` points wide, wobbling as the pen does.
float stroke(float2 p, float2 a, float2 b, Pen pen, float w) {
    float d = wc_seg(p + float2(pen.wob, -pen.wob) * 0.3, a, b);
    return 1.0 - smoothstep(w - 0.3, w + 0.5, d);
}

// Painting i on the easel: its origin in xy, its size in zw, in points.
// The backboard on the easel (Cyrus, 2026-11-17: "Add a backboard to support the painting"): one
// size for every painting, so nothing jumps when the next blooms in, its foot on the easel's
// ledge at CANVAS_FOOT: its origin in xy, its size in zw.
float4 board_rect(Studio st) {
    float2 wh = float2(st.w * CANVAS_W, st.h * CANVAS_H);
    float2 foot_pt = sp(st, float2(CANVAS_X, CANVAS_FOOT));
    return float4(foot_pt.x - wh.x * 0.5, foot_pt.y - wh.y, wh);
}

// Painting i taped to the board: as large as fits inside it less BOARD_PAD round, centred on it.
float4 canvas_rect(Studio st, int i) {
    float2 sz = max(ARKOS_IMAGE_SIZE[i], float2(1.0));
    float4 b = board_rect(st);
    float2 box = max(b.zw - 2.0 * BOARD_PAD, float2(1.0));
    float2 wh = sz * min(box.x / sz.x, box.y / sz.y);
    return float4(b.xy + (b.zw - wh) * 0.5, wh);
}

// Painting i at p, as a watercolour of it: its colours softened, pooled darker where they meet,
// with the paper's grain through them, and its edge left ragged as a wash's. The colour in rgb,
// how much of the paper it covers in a.
float4 painting_at(texture2d_array<float> images, float2 p, int i, float4 r, float px, float gran, float3 tone) {
    constexpr sampler s(filter::linear, mip_filter::linear, address::clamp_to_edge);
    float2 hb = r.zw * 0.5;
    // Well off the painting, the paper alone: the noise and the samples are spent on it only.
    float box = sd_box(p - (r.xy + hb), hb);
    if (box > 6.0) {
        return float4(0.0);
    }
    float d = box + (vnoise(p * 0.11 + float(i) * 7.3) - 0.5) * 2.0 * PAINTING_WAVER;
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
    // Toned toward the theme's own colours (Cyrus, 2026-11-17: Pane 0 "should blend better into
    // the theme's colors"), its lights and darks kept.
    float3 lum_w = float3(0.299, 0.587, 0.114);
    float3 toned = saturate(tone * (dot(c, lum_w) / max(dot(tone, lum_w), 0.05)));
    c = mix(c, toned, STUDIO_TONE);
    float cover = 1.0 - smoothstep(-0.6, 0.6, d);
    // The edge pools a little darker, as a wash dries at its rim.
    float rim = smoothstep(-3.0, 0.0, d) * cover;
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
    // Four pools leave no point much more than half the painting from one. Each point fades over
    // half the bloom, starting later the farther it is from a pool: a wide, soft blend drifting out
    // from them, not a front.
    reach = saturate(reach / (0.55 * max(max(area.z, area.w), 1.0)) + (fbm(p * 0.03 + next) - 0.5) * 0.25);
    return smoothstep(reach * 0.5, reach * 0.5 + 0.5, bloom);
}

// A painting laid at strength a: its own sheet of watercolour paper, a little lighter than the
// page, covering what is behind it (the easel's mast), its washes on it; on dark paper the sheet
// glows less.
float3 painting_onto(constant ArkosInputs& u, float3 col, float3 c, float a) {
    float3 paper = u.colors[ARKOS_WINDOW_BG].rgb;
    float3 sheet = u.dark > 0.5 ? mix(paper, float3(1.0), 0.75) * 0.9 : mix(paper, float3(1.0), 0.5);
    return mix(col, sheet * c, a);
}

// The studio's cat (Cyrus, 2026-11-17: "a cat that walks in scene for a bit, sits besides various
// things like the plant / painting / etc, and eventually walks off screen"): once every CAT_CYCLE
// seconds it strolls in from one side along the front of the floor at CAT_SPEED scene widths a
// second, sits CAT_SIT seconds beside two of its spots in turn (under the side table, by the easel,
// by the plant), then strolls off the other side, and is gone for the rest of the cycle. Under
// Reduce Motion the clock holds at zero, where it has not yet come in.
constant float CAT_CYCLE = 150.0;
constant float CAT_SPEED = 0.03;
constant float CAT_SIT = 12.0;
constant float CAT_STRIDE_S = 0.9;
constant float CAT_Y = 0.05;
constant float CAT_SPOT_TABLE = 0.17;
constant float CAT_SPOT_EASEL = 0.38;
constant float CAT_SPOT_PLANT = 0.76;

// Where the cat is at t: its place across the scene (x), whether it is away (0), walking (1) or
// sitting (2) (y), which way it faces (z, 1 to the right), and how far through a stride it is (w).
float4 cat_at(float t) {
    float k = floor(max(t, 0.0) / CAT_CYCLE);
    float tau = max(t, 0.0) - k * CAT_CYCLE;
    float dir = hash1(k * 3.1 + 0.7) < 0.5 ? 1.0 : -1.0;
    float pick = floor(hash1(k * 5.7 + 1.3) * 3.0);
    float a = pick == 2.0 ? CAT_SPOT_EASEL : CAT_SPOT_TABLE;
    float b = pick == 0.0 ? CAT_SPOT_EASEL : CAT_SPOT_PLANT;
    if (dir < 0.0) {
        float swap = a;
        a = b;
        b = swap;
    }
    float x0 = dir > 0.0 ? -0.12 : 1.12;
    float x3 = dir > 0.0 ? 1.12 : -0.12;
    float e1 = abs(a - x0) / CAT_SPEED;
    float e2 = e1 + CAT_SIT;
    float e3 = e2 + abs(b - a) / CAT_SPEED;
    float e4 = e3 + CAT_SIT;
    float e5 = e4 + abs(x3 - b) / CAT_SPEED;
    if (tau < e1) {
        return float4(x0 + dir * tau * CAT_SPEED, 1.0, dir, fract(tau / CAT_STRIDE_S));
    }
    if (tau < e2) {
        return float4(a, 2.0, dir, 0.0);
    }
    if (tau < e3) {
        return float4(a + dir * (tau - e2) * CAT_SPEED, 1.0, dir, fract((tau - e2) / CAT_STRIDE_S));
    }
    if (tau < e4) {
        return float4(b, 2.0, dir, 0.0);
    }
    if (tau < e5) {
        return float4(b + dir * (tau - e4) * CAT_SPEED, 1.0, dir, fract((tau - e4) / CAT_STRIDE_S));
    }
    return float4(x3, 0.0, dir, 0.0);
}

// A soft ellipse's coverage at q, in the cat's own units.
float cat_blob(float2 q, float2 c, float2 r) {
    return 1.0 - smoothstep(0.85, 1.15, length((q - c) / r));
}

// A limb or the tail, `w` units thick, from a to b.
float cat_limb(float2 q, float2 a, float2 b, float w) {
    return 1.0 - smoothstep(w * 0.5 - 0.35, w * 0.5 + 0.35, wc_seg(q, a, b));
}

// How much of the cat covers q: q in its own units, up positive and its head to +x.
float cat_cover(float2 q, float state, float stride, float t) {
    float c = 0.0;
    if (state > 1.5) {
        // Sitting: upright, its tail curled round its feet, the tip twitching now and then.
        c = max(c, cat_blob(q, float2(0.0, 6.0), float2(4.2, 5.8)));
        c = max(c, cat_blob(q, float2(1.2, 13.4), float2(3.1, 3.0)));
        c = max(c, cat_limb(q, float2(0.4, 15.6), float2(-0.2, 17.8), 1.4));
        c = max(c, cat_limb(q, float2(2.3, 15.7), float2(3.0, 17.8), 1.4));
        c = max(c, cat_limb(q, float2(2.0, 4.0), float2(2.3, 0.2), 1.3));
        c = max(c, cat_limb(q, float2(3.3, 4.0), float2(3.7, 0.2), 1.3));
        float tip = 0.7 * sin(t * 0.9) * smoothstep(0.6, 1.0, sin(t * 0.23));
        c = max(c, cat_limb(q, float2(-3.6, 1.2), float2(-0.8, 0.4), 1.6));
        c = max(c, cat_limb(q, float2(-0.8, 0.4), float2(3.6, 0.5), 1.5));
        c = max(c, cat_limb(q, float2(3.6, 0.5), float2(5.6, 1.6 + tip), 1.3));
    } else {
        // Walking: its legs swing in pairs, its tail held up in a gentle curve.
        float sw = sin(stride * 6.2831853);
        c = max(c, cat_blob(q, float2(0.0, 5.6), float2(7.6, 3.6)));
        c = max(c, cat_blob(q, float2(7.8, 8.2), float2(3.0, 2.9)));
        c = max(c, cat_limb(q, float2(6.7, 10.4), float2(6.2, 12.6), 1.4));
        c = max(c, cat_limb(q, float2(8.8, 10.5), float2(9.5, 12.5), 1.4));
        c = max(c, cat_limb(q, float2(-5.0, 3.5), float2(-5.0 + 1.6 * sw, 0.2), 1.3));
        c = max(c, cat_limb(q, float2(-3.4, 3.5), float2(-3.4 - 1.6 * sw, 0.2), 1.3));
        c = max(c, cat_limb(q, float2(3.6, 3.5), float2(3.6 - 1.6 * sw, 0.2), 1.3));
        c = max(c, cat_limb(q, float2(5.2, 3.5), float2(5.2 + 1.6 * sw, 0.2), 1.3));
        float wag = 0.8 * sin(t * 1.1);
        c = max(c, cat_limb(q, float2(-7.0, 6.4), float2(-9.8, 8.8), 1.3));
        c = max(c, cat_limb(q, float2(-9.8, 8.8), float2(-10.8 + wag * 0.4, 12.2 + wag * 0.3), 1.2));
    }
    return c;
}

// The studio at p (see `Studio`): the room, then the painting on the easel, then what stands
// before it, and the pen's lines over all of it.
float3 studio(constant ArkosInputs& u, texture2d_array<float> images, float2 p, float4 foot, float t, float gran, float bleed, Pen pen, float3 col, float k) {
    Studio st = studio_in(foot);
    float show = smoothstep(STUDIO_MIN_H, STUDIO_MIN_H + STUDIO_FADE, st.h) * k;
    float2 n = sn(st, p);
    if (show <= 0.0 || n.x < -0.06 || n.x > 1.06 || n.y < -0.03 || n.y > 1.1) {
        return col;
    }
    float tau = 6.2831853;
    float3 pig0 = pigment(u, 0);
    float3 pig1 = pigment(u, 1);
    float3 pig2 = pigment(u, 2);
    float3 pig3 = pigment(u, 3);
    float3 paper = u.colors[ARKOS_WINDOW_BG].rgb;
    float3 warm = u.dark > 0.5 ? mix(paper, float3(1.0, 0.86, 0.62), 0.35) : float3(1.0, 0.97, 0.9);
    // Wood (the floor, the water tower) and leaves, each a natural colour taken toward the
    // theme's own.
    float3 wood = mix(float3(0.76, 0.6, 0.44), pig1, 0.3);
    float3 leaf_c = mix(float3(0.36, 0.6, 0.38), pig2, 0.22);
    // The room's edges melt into the sidebar's paper: no frame round it.
    float melt = smoothstep(-0.03, 0.09, n.x) * (1.0 - smoothstep(0.91, 1.03, n.x)) * (1.0 - smoothstep(0.86, 1.05, n.y)) * show;
    float floor_y = FLOOR_Y + (bleed - 0.5) * 0.015;
    float ink = 0.0;

    // The window's place, and inside it.
    float2 w0 = float2(0.05, 0.42);
    float2 w1 = float2(0.4, 0.86);
    bool in_window = n.x > w0.x && n.x < w1.x && n.y > w0.y && n.y < w1.y;

    // The wall: plaster in a light wash, and a ragged patch of brick where it has come away.
    if (n.y > floor_y && !in_window) {
        col = wash_onto(u, col, pig2, 0.1 * melt * (0.8 + 0.4 * gran));
        float patch = smoothstep(0.0, 0.1, 0.3 - length((n - float2(0.8, 0.66)) * float2(1.0, 0.85)) + (fbm(p * 0.05) - 0.5) * 0.2);
        if (patch > 0.0) {
            float2 b = float2(n.x * st.w / 9.0, n.y * st.h / 4.0);
            float row = floor(b.y);
            float bx = b.x + 0.5 * fmod(row, 2.0);
            float mortar = max(1.0 - smoothstep(0.0, 0.14, fract(b.y)), 1.0 - smoothstep(0.0, 0.07, fract(bx)));
            float tone = 0.7 + 0.6 * hash1(floor(bx) * 7.1 + row * 13.3);
            col = wash_onto(u, col, pig1, 0.24 * patch * melt * tone * (1.0 - 0.75 * mortar));
        }
    }
    if (in_window) {
        float2 wn = (n - w0) / (w1 - w0);
        // The sky, the paper left bare where the clouds pass.
        float cloud = smoothstep(0.5, 0.7, fbm(float2(wn.x * 2.2 + t / CLOUD_PASS_S * 2.0, wn.y * 5.0) + 4.0));
        col = wash_onto(u, col, pig2, (0.16 - 0.12 * cloud) * melt * (0.6 + 0.4 * wn.y));
        // Brownstone rooftops across the street, their cornices, and a water tower on one.
        float bcol = floor(wn.x * 5.0);
        float roof = 0.2 + 0.16 * hash1(bcol * 3.7 + 1.0);
        float tower_x = (wn.x - 0.66) / 0.12;
        float tank = step(abs(tower_x), 1.0) * step(roof + 0.12, wn.y) * step(wn.y, roof + 0.27);
        float cone = step(roof + 0.27, wn.y) * step(abs(tower_x), 1.0 - (wn.y - roof - 0.27) / 0.08);
        float legs = step(roof, wn.y) * step(wn.y, roof + 0.12) * step(0.85, abs(tower_x)) * step(abs(tower_x), 1.0);
        roof = bcol == 3.0 ? 0.24 : roof;
        if (wn.y < roof) {
            col = wash_onto(u, col, pig0, 0.3 * melt * (0.8 + 0.3 * gran));
        } else if (wn.y < roof + 0.025) {
            col = wash_onto(u, col, pig0, 0.4 * melt);
        }
        if (tank + cone + legs > 0.0) {
            col = wash_onto(u, col, wood, 0.45 * melt);
        }
        // The fire escape's rail outside, low across the window.
        float rail = 1.0 - smoothstep(0.0, 0.012, abs(wn.y - 0.14));
        float bars = (1.0 - smoothstep(0.0, 0.02, abs(fract(wn.x * 9.0) - 0.5))) * step(wn.y, 0.14);
        ink = max(ink, 0.45 * max(rail, bars * 0.8));
    }
    // The window's steel frame: its outline, three panes across and four down, and its sill.
    // Each group of lines below is drawn only near where it lies.
    if (n.x > w0.x - 0.05 && n.x < w1.x + 0.05 && n.y > w0.y - 0.05 && n.y < w1.y + 0.03) {
        float2 a = sp(st, w0);
        float2 b = sp(st, w1);
        ink = max(ink, stroke(p, a, float2(b.x, a.y), pen, 1.0));
        ink = max(ink, stroke(p, float2(a.x, b.y), b, pen, 1.0));
        ink = max(ink, stroke(p, a, float2(a.x, b.y), pen, 1.0));
        ink = max(ink, stroke(p, float2(b.x, a.y), b, pen, 1.0));
        for (int c = 1; c < 3; c++) {
            float x = mix(a.x, b.x, float(c) / 3.0);
            ink = max(ink, stroke(p, float2(x, a.y), float2(x, b.y), pen, 0.6));
        }
        for (int r = 1; r < 4; r++) {
            float y = mix(a.y, b.y, float(r) / 4.0);
            ink = max(ink, stroke(p, float2(a.x, y), float2(b.x, y), pen, 0.6));
        }
        ink = max(ink, stroke(p, a + float2(-3.0, 2.0), float2(b.x + 3.0, a.y + 2.0), pen, 0.9));
    }
    // A sheer drape hung at the window's right, stirring at its hem.
    {
        float sway = sin(t * tau / DRAPE_SWAY_S + n.y * 9.0) * 0.012 * saturate((w1.y - n.y) / 0.4);
        float left = w1.x - 0.03 + sway;
        float right = w1.x + 0.05 + sway * 1.4;
        if (n.y > w0.y - 0.06 && n.y < w1.y + 0.02 && n.x > left && n.x < right) {
            float folds = 0.75 + 0.25 * sin((n.x - left) / (right - left) * 18.0 + sway * 40.0);
            col = wash_onto(u, col, pig0, 0.13 * folds * melt);
        }
    }
    // The light through the window, falling across the room to the floor: it breathes, and dust
    // drifts in it.
    float2 beam_o = (w0 + w1) * 0.5;
    float2 beam_d = normalize(float2(0.62, -0.78));
    float2 rel = n - beam_o;
    float along = dot(rel, beam_d);
    float across = dot(rel, float2(-beam_d.y, beam_d.x));
    float beam = (1.0 - smoothstep(0.1, 0.16, abs(across))) * smoothstep(0.0, 0.1, along) * (1.0 - smoothstep(0.5, 0.75, along));
    float breath = 0.75 + 0.25 * sin(t * tau / BEAM_BREATH_S);
    col = mix(col, warm, beam * 0.14 * breath * melt);

    // The floor: wide boards in a warm wash, their seams, and the skirting where it meets the wall.
    if (n.y < floor_y) {
        col = wash_onto(u, col, wood, 0.26 * melt * (0.8 + 0.4 * gran));
        // Two seams along the boards, and their butt joints staggered.
        float y_pt = n.y * st.h;
        float rows = floor_y * st.h / 3.0;
        float seam = 1.0 - smoothstep(0.0, 0.9, abs(y_pt - rows * floor(y_pt / rows + 0.5)));
        float board = floor(y_pt / rows);
        float joint = smoothstep(0.486, 0.5, abs(fract(n.x * 2.5 + board * 0.37) - 0.5));
        ink = max(ink, 0.25 * max(seam, joint * 0.7));
    }
    ink = max(ink, stroke(p, sp(st, float2(-0.02, floor_y)), sp(st, float2(1.02, floor_y)), pen, 0.9));

    // The easel and the painting on it.
    float3 at = gallery_at(t, ARKOS_IMAGE_COUNT);
    int shown = int(at.x);
    int next = int(at.y);
    float4 r0 = canvas_rect(st, shown);
    float4 bd = board_rect(st);
    float2 top = float2(bd.x + bd.z * 0.5, bd.y - 8.0);
    float2 ledge_l = float2(bd.x - 4.0, bd.y + bd.w + 1.5);
    float2 ledge_r = float2(bd.x + bd.z + 4.0, bd.y + bd.w + 1.5);
    float2 foot_l = sp(st, float2(CANVAS_X - 0.17, floor_y - 0.06));
    float2 foot_r = sp(st, float2(CANVAS_X + 0.17, floor_y - 0.06));
    float2 foot_b = sp(st, float2(CANVAS_X + 0.04, floor_y + 0.02));
    // A soft shadow the board casts on the wall behind it.
    float shade = 1.0 - smoothstep(0.0, 6.0, sd_box(p - (bd.xy + bd.zw * 0.5 + float2(4.0, 3.0)), bd.zw * 0.5));
    col = wash_onto(u, col, pig0, 0.12 * shade * melt);
    ink = max(ink, stroke(p, top, foot_b, pen, 0.9));
    ink = max(ink, stroke(p, float2(top.x - 3.0, top.y), foot_l, pen, 1.0));
    ink = max(ink, stroke(p, float2(top.x + 3.0, top.y), foot_r, pen, 1.0));
    ink = max(ink, stroke(p, top + float2(-5.0, 0.0), top + float2(5.0, 0.0), pen, 1.0));
    // The room's lines go down first, so the board covers the easel's mast and legs behind it.
    float3 ink_c = u.colors[ARKOS_TERMINAL_FG].rgb;
    col = mix(col, ink_c, saturate(ink) * 0.62 * ink_tooth(pen) * melt);
    ink = 0.0;
    // The backboard: light wood with its grain running across, outlined, and the painting's own
    // faint shadow on it.
    float on_board = 1.0 - smoothstep(-0.5, 0.5, sd_box(p - (bd.xy + bd.zw * 0.5), bd.zw * 0.5));
    if (on_board > 0.0) {
        float grain = vnoise(float2(p.x * 0.04, p.y * 0.6)) * 0.12;
        // Solid: the easel's mast and legs behind it do not show through.
        float3 board_c = mix(paper, wood, 0.42 + grain);
        col = mix(col, u.dark > 0.5 ? board_c : board_c * mix(float3(1.0), bright(pig2), 0.06), on_board);
        float sheet_shade = 1.0 - smoothstep(0.0, 3.0, sd_box(p - (r0.xy + r0.zw * 0.5 + float2(1.5, 1.5)), r0.zw * 0.5));
        col = wash_onto(u, col, wood, 0.3 * sheet_shade * on_board * melt);
    }
    ink = max(ink, stroke(p, bd.xy, float2(bd.x + bd.z, bd.y), pen, 0.8));
    ink = max(ink, stroke(p, bd.xy, float2(bd.x, bd.y + bd.w), pen, 0.8));
    ink = max(ink, stroke(p, float2(bd.x + bd.z, bd.y), bd.xy + bd.zw, pen, 0.8));
    float4 now_p = painting_at(images, p, shown, r0, u.scale, gran, mix(paper, pig0, 0.4));
    float reached = 0.0;
    float4 next_p = float4(0.0);
    if (at.z > 0.0) {
        float4 r1 = canvas_rect(st, next);
        float4 area = float4(min(r0.xy, r1.xy), max(r0.xy + r0.zw, r1.xy + r1.zw) - min(r0.xy, r1.xy));
        reached = bloom_at(p, area, at.y, at.z);
        next_p = painting_at(images, p, next, r1, u.scale, gran, mix(paper, pig0, 0.4));
    }
    float thin = 1.0 - GALLERY_THIN * sin(3.14159 * at.z);
    col = painting_onto(u, col, now_p.rgb, now_p.a * (1.0 - reached) * show * thin);
    col = painting_onto(u, col, next_p.rgb, next_p.a * reached * show * thin);
    // Two strips of tape hold the painting's top corners to the board.
    // The tape cross-fades with the paintings it holds.
    for (int c = 0; c < 4; c++) {
        float4 held = c < 2 ? r0 : canvas_rect(st, next);
        float weight = c < 2 ? 1.0 - at.z : at.z;
        if (weight <= 0.0) {
            continue;
        }
        float2 corner = float2(c % 2 == 0 ? held.x : held.x + held.z, held.y);
        float2 q = p - corner;
        float ang = c % 2 == 0 ? 0.75 : -0.75;
        float2 local = float2(q.x * cos(ang) + q.y * sin(ang), -q.x * sin(ang) + q.y * cos(ang));
        float tape = 1.0 - smoothstep(-0.4, 0.4, sd_box(local, float2(5.0, 1.8)));
        col = mix(col, mix(paper, pig0, 0.3), tape * 0.75 * melt * weight);
    }
    ink = max(ink, stroke(p, ledge_l, ledge_r, pen, 1.3));

    // The side table: its top and legs, a palette dabbed in the theme's pigments, a jar of
    // brushes, a mug with its steam.
    float table_y = 0.27;
    if (n.x < 0.34 && n.y < 0.52) {
    ink = max(ink, stroke(p, sp(st, float2(0.02, table_y)), sp(st, float2(0.3, table_y)), pen, 1.1));
    ink = max(ink, stroke(p, sp(st, float2(0.05, table_y)), sp(st, float2(0.05, -0.02)), pen, 0.8));
    ink = max(ink, stroke(p, sp(st, float2(0.27, table_y)), sp(st, float2(0.27, -0.02)), pen, 0.8));
    {
        float2 pc = sp(st, float2(0.12, table_y)) + float2(0.0, -2.5);
        float2 pr = float2(0.07 * st.w, 3.2);
        float pal = length((p - pc) / pr);
        if (pal < 1.0) {
            col = wash_onto(u, col, pig3, 0.22 * melt);
            for (int d = 0; d < 4; d++) {
                float2 dc = pc + float2((float(d) - 1.5) * pr.x * 0.42, (d % 2 == 0 ? -0.8 : 0.8));
                float dab = 1.0 - smoothstep(1.0, 2.0, length(p - dc));
                col = wash_onto(u, col, pigment(u, d), 0.75 * dab * melt);
            }
        }
        ink = max(ink, 0.7 * (1.0 - smoothstep(0.0, 1.0, abs(pal - 1.0) * pr.y)));
        // The jar, and three brushes standing in it.
        float2 j0 = sp(st, float2(0.19, table_y));
        float2 j1 = sp(st, float2(0.25, table_y + 0.07));
        if (p.x > j0.x && p.x < j1.x && p.y < j0.y && p.y > j1.y) {
            col = wash_onto(u, col, pig2, 0.22 * melt);
        }
        ink = max(ink, stroke(p, float2(j0.x, j1.y), float2(j0.x, j0.y), pen, 0.7));
        ink = max(ink, stroke(p, float2(j1.x, j1.y), float2(j1.x, j0.y), pen, 0.7));
        for (int b = 0; b < 3; b++) {
            float2 base = float2(mix(j0.x, j1.x, 0.3 + 0.2 * float(b)), j1.y + 2.0);
            float2 tip = base + float2((float(b) - 1.0) * 0.03 * st.w, -0.13 * st.h);
            ink = max(ink, stroke(p, base, tip, pen, 0.55));
            float bristle = 1.0 - smoothstep(0.8, 1.8, length(p - tip));
            col = wash_onto(u, col, pigment(u, b + 1), 0.7 * bristle * melt);
        }
        // The mug, and its steam rising in two soft curls.
        float2 m0 = sp(st, float2(0.06, table_y));
        float2 m1 = sp(st, float2(0.1, table_y + 0.045));
        if (p.x > m0.x && p.x < m1.x && p.y < m0.y && p.y > m1.y) {
            col = wash_onto(u, col, pig1, 0.28 * melt);
        }
        ink = max(ink, stroke(p, float2(m1.x, mix(m0.y, m1.y, 0.3)), float2(m1.x + 2.5, mix(m0.y, m1.y, 0.55)), pen, 0.6));
        float rise = (n.y - (table_y + 0.05)) / 0.13;
        if (rise > 0.0 && rise < 1.0) {
            for (int c = 0; c < 2; c++) {
                float x = (m0.x + m1.x) * 0.5 + (float(c) - 0.5) * 3.0 + sin(rise * 7.0 - t * tau / STEAM_RISE_S + float(c) * 2.0) * 2.2 * rise;
                float curl = (1.0 - smoothstep(0.3, 1.2, abs(p.x - x))) * sin(rise * 3.14159) * (1.0 - rise);
                ink = max(ink, 0.3 * curl);
            }
        }
    }
    }

    // The plant on the floor at the right: its pot, and its broad leaves swaying a little.
    if (n.x > 0.74 && n.y < 0.5) {
        float2 pot0 = sp(st, float2(0.84, floor_y - 0.1));
        float2 pot1 = sp(st, float2(0.96, floor_y + 0.04));
        if (p.x > pot0.x && p.x < pot1.x && p.y < pot0.y && p.y > pot1.y) {
            col = wash_onto(u, col, mix(float3(0.74, 0.46, 0.36), pig1, 0.3), 0.42 * melt);
        }
        ink = max(ink, stroke(p, float2(pot0.x, pot1.y), float2(pot1.x, pot1.y), pen, 0.8));
        float2 stem = sp(st, float2(0.9, floor_y + 0.04));
        for (int l = 0; l < 6; l++) {
            float fl = float(l);
            float ang = -1.1 + fl * 0.45 + sin(t * tau / LEAF_SWAY_S + fl * 1.3) * 0.06;
            float len = (0.1 + 0.05 * hash1(fl * 3.1)) * st.h;
            float2 dir = float2(sin(ang), -cos(ang));
            float2 tip = stem + dir * len;
            float2 mid = (stem + tip) * 0.5 + dir * len * 0.15;
            float2 q = p - mid;
            float2 local = float2(dot(q, float2(-dir.y, dir.x)), dot(q, dir));
            float leaf = length(local / float2(len * 0.2, len * 0.42));
            if (leaf < 1.0) {
                col = wash_onto(u, col, leaf_c, 0.5 * (1.0 - leaf * 0.4) * melt);
            }
            ink = max(ink, 0.5 * stroke(p, stem, tip, pen, 0.45));
        }
    }

    // The pendant lamp on its cord, swaying a little, and its warm glow.
    if (n.x > 0.5 && n.x < 0.95 && n.y > 0.55) {
        float sway = LAMP_SWAY * sin(t * tau / LAMP_SWAY_S);
        float2 hang = sp(st, float2(0.72, 1.04));
        float cord = 0.2 * st.h;
        float2 bulb = hang + float2(sin(sway), cos(sway)) * cord;
        ink = max(ink, stroke(p, hang, bulb, pen, 0.45));
        float2 down = normalize(bulb - hang);
        float2 side = float2(-down.y, down.x);
        float2 s0 = bulb - side * 3.0;
        float2 s1 = bulb + side * 3.0;
        float2 s2 = bulb + down * 7.0 - side * 8.0;
        float2 s3 = bulb + down * 7.0 + side * 8.0;
        ink = max(ink, stroke(p, s0, s2, pen, 0.8));
        ink = max(ink, stroke(p, s1, s3, pen, 0.8));
        ink = max(ink, stroke(p, s2, s3, pen, 0.8));
        float glow = exp(-dot(p - (bulb + down * 9.0), p - (bulb + down * 9.0)) / (2.0 * 14.0 * 14.0));
        col = mix(col, warm, glow * 0.35 * melt);
        col = wash_onto(u, col, pig3, (1.0 - smoothstep(0.0, 1.0, wc_seg(p, (s0 + s1) * 0.5, (s2 + s3) * 0.5) - 2.0)) * 0.25 * melt);
    }

    // The cat, when it is in: a soft grey silhouette taken toward the theme, a faint shadow under it.
    {
        float4 cat = cat_at(t);
        if (cat.y > 0.5) {
            float unit = clamp(st.w / 190.0, 0.95, 1.6);
            float2 base = sp(st, float2(cat.x, CAT_Y));
            float2 d = p - base;
            if (abs(d.x) < 16.0 * unit && d.y > -20.0 * unit && d.y < 3.0) {
                float2 q = float2(d.x * cat.z, -d.y) / unit;
                float shadow = 1.0 - smoothstep(0.6, 1.0, length(float2(q.x / 9.0, (q.y + 0.3) / 1.4)));
                col = wash_onto(u, col, pig0, 0.18 * shadow * melt);
                float cover = cat_cover(q, cat.y, cat.w, t);
                float3 fur = mix(u.colors[ARKOS_TERMINAL_FG].rgb, pig2, 0.4);
                col = mix(col, fur, cover * 0.82 * melt);
            }
        }
    }

    // Dust drifting in the light, each mote slowly down the beam and swaying across it.
    for (int m = 0; m < (beam > 0.0 ? MOTES : 0); m++) {
        float fm = float(m);
        float a = fract(hash1(fm * 5.3) + t * 0.004 * (0.6 + hash1(fm * 2.9)));
        float c = (hash1(fm * 8.7) - 0.5) * 0.2 + sin(t * 0.25 + fm) * 0.015;
        float2 mn = beam_o + beam_d * (0.1 + a * 0.55) + float2(-beam_d.y, beam_d.x) * c;
        float mote = 1.0 - smoothstep(0.4, 1.3, length(p - sp(st, mn)));
        float twinkle = 0.55 + 0.45 * sin(t * 0.7 + fm * 2.3);
        col = mix(col, warm, mote * twinkle * 0.6 * melt * smoothstep(0.0, 0.15, a) * (1.0 - smoothstep(0.8, 1.0, a)));
    }

    // The pen over what stands before the canvas, lighter than the panes' own lines.
    return mix(col, ink_c, saturate(ink) * 0.62 * ink_tooth(pen) * melt);
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
            col = studio(u, images, p, foot, t, gran, bleed, pen, col, k);
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
