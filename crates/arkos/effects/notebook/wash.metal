// The notebook wash under the Arkos window: a composition notebook. The title bar is the
// marbled black-and-white cover with its white label (the theme's paper, bordered in its ink)
// holding the title's words and icons; the strip under the lowest block (the status bar) is the
// cover faded to a pale mottle. The sidebar is the inside cover, a spiral binds it to the panes
// where the app leaves a gutter (ARKOS_BINDER_BLOCK), the tab bars the pages' top
// edge, and each pane body a cream page with paper fibre, a red margin line (the theme's focus
// colour, thinned) and, where a terminal reports its rows (ARKOS_ROWS_BLOCK), a blue rule under
// each row in the theme's own blue. Edges are pencil lines in the dim text's colour, a little
// wobbly. The selected and hovered Explorer rows and the hovered tab are a highlighter stroke in
// the theme's find-match colour, ragged at both ends. The preamble (arkos.metal) is prepended
// before this is compiled, and declares ArkosInputs, ArkosBlock, ArkosVertex and the ARKOS_*
// indices used here. Every colour but the cover's black and white is the theme's, and the paper
// under the UI's text keeps 3:1 against it.

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
    for (int i = 0; i < 4; i++) {
        v += a * vnoise(p);
        p *= 2.07;
        a *= 0.5;
    }
    return v;
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

float3 keep_readable(constant ArkosInputs& u, float3 col) {
    float3 paper = u.colors[ARKOS_WINDOW_BG].rgb;
    float lt = rel_lum(u.colors[ARKOS_TEXT].rgb);
    float lc = rel_lum(col);
    float lp = rel_lum(paper);
    float f = 0.0;
    if (lt < lp) {
        float least = READABLE * (lt + 0.05) - 0.05;
        if (lc < least) { f = saturate((least - lc) / max(lp - lc, 1e-5)); }
    } else {
        float most = (lt + 0.05) / READABLE - 0.05;
        if (lc > most) { f = saturate((lc - most) / max(lc - lp, 1e-5)); }
    }
    return to_srgb(mix(to_linear(col), to_linear(paper), f));
}

// The cover's marble at p: 1 where the white blotches are, 0 on the black ground, with the
// blotches' soft, broken edges.
float marble(float2 p) {
    float m = fbm(p * 0.09) + 0.25 * vnoise(p * 0.5);
    return smoothstep(0.52, 0.6, m);
}

// Where the cover's label sits in the title bar: from just past the traffic lights to the bar's
// own right padding, a little in from its top and bottom. The manifest's `title_pad` sets the
// bar's words and icons that far inside both ends, so they have room inside the label.
constant float LABEL_LEFT = 80.0;
constant float LABEL_RIGHT = 12.0;
constant float LABEL_INSET = 3.0;
constant float LABEL_RADIUS = 5.0;

float sd_round_box(float2 q, float2 hb, float r) {
    float2 d = abs(q) - hb + r;
    return length(max(d, 0.0)) + min(max(d.x, d.y), 0.0) - r;
}

// The title bar as the cover: marble, then the label with its border. `col` comes in as the
// paper and goes out as the cover at p.
float3 cover(constant ArkosInputs& u, float2 p, ArkosBlock b, float3 col, float k) {
    float4 r = b.rect;
    float3 black = float3(0.08, 0.08, 0.09);
    float3 white = float3(0.93, 0.93, 0.9);
    float3 cov = mix(black, white, marble(p));
    float2 hb = float2((r.z - LABEL_LEFT - LABEL_RIGHT) * 0.5, (r.w - 2.0 * LABEL_INSET) * 0.5);
    float2 c = float2(r.x + LABEL_LEFT + hb.x, r.y + LABEL_INSET + hb.y);
    float d = sd_round_box(p - c, hb, LABEL_RADIUS);
    float inside = 1.0 - smoothstep(-0.5, 0.5, d);
    float border = (1.0 - smoothstep(0.8, 1.6, abs(d))) ;
    float3 label = mix(col, u.colors[ARKOS_TEXT].rgb, border * 0.9);
    return mix(col, mix(cov, label, inside), k);
}

// Paper fibre: a faint speckle, darker on light paper and lighter on dark.
float3 fibre(constant ArkosInputs& u, float3 paper, float2 p, float k) {
    float g = (vnoise(p * 0.9) - 0.5) * 0.03 + (vnoise(p * 0.05) - 0.5) * 0.02;
    return u.dark > 0.5 ? paper + g * k : paper * (1.0 + g * k);
}

// The binder: a spiral of wire in the gutter between the sidebar (the left page) and the panes
// (the right), each turn rising out of a hole punched by one page's edge, arching over the fold
// and down into a hole by the other's, a little lower, as a coil runs. The pages shade into the
// fold between them, and each turn throws a soft shadow.
constant float COIL_PITCH = 11.0;
constant float HOLE_IN = 4.5;
constant float HOLE_R = 2.0;
constant float COIL_RISE = 3.5;
constant float COIL_SLANT = 2.5;
constant float WIRE_R = 0.95;

// A turn of wire across the binder, centred on its left hole at (x0, cy): its distance from p,
// signed (negative above the wire), and how far along it p lies.
float2 coil(float2 p, float x0, float x1, float cy) {
    float span = max(x1 - x0, 1.0);
    float s = saturate((p.x - x0) / span);
    float y = cy + COIL_SLANT * s - COIL_RISE * 4.0 * s * (1.0 - s);
    float slope = (COIL_SLANT - COIL_RISE * 4.0 * (1.0 - 2.0 * s)) / span;
    float across = (p.y - y) / sqrt(1.0 + slope * slope);
    float past = max(max(x0 - p.x, p.x - x1), 0.0);
    return float2(past > 0.0 ? 1e5 : across, s);
}

float3 binder(constant ArkosInputs& u, float2 p, ArkosBlock b, float3 left, float3 right, float k) {
    float4 r = b.rect;
    float t = saturate((p.x - r.x) / max(r.z, 1.0));
    float3 col = mix(left, right, smoothstep(0.4, 0.6, t));
    float fold = exp(-pow((t - 0.5) / 0.17, 2.0));
    col *= 1.0 - 0.2 * fold * k;
    float3 black = float3(0.08, 0.08, 0.09);
    float x0 = r.x + HOLE_IN;
    float x1 = r.x + r.z - HOLE_IN;
    // The turns that can reach p: the one whose holes are above it and the next.
    float n0 = floor((p.y - r.y - COIL_PITCH * 0.5) / COIL_PITCH);
    float last = floor((r.w - COIL_PITCH) / COIL_PITCH);
    for (int j = 0; j < 2; j++) {
        float n = n0 + float(j);
        if (n < 0.0 || n > last) { continue; }
        float cy = r.y + COIL_PITCH * (n + 0.5);
        float d = min(length(p - float2(x0, cy)), length(p - float2(x1, cy + COIL_SLANT)));
        col = mix(col, mix(col, black, 0.75), (1.0 - smoothstep(HOLE_R - 0.5, HOLE_R + 0.5, d)) * k);
        float shade = coil(p - float2(0.0, 1.6), x0, x1, cy).x;
        col *= 1.0 - 0.22 * (1.0 - smoothstep(WIRE_R, WIRE_R + 1.4, abs(shade))) * k;
    }
    for (int j = 0; j < 2; j++) {
        float n = n0 + float(j);
        if (n < 0.0 || n > last) { continue; }
        float cy = r.y + COIL_PITCH * (n + 0.5);
        float2 c = coil(p, x0, x1, cy);
        float on = 1.0 - smoothstep(WIRE_R - 0.35, WIRE_R + 0.35, abs(c.x));
        float v = clamp(c.x / WIRE_R, -1.0, 1.0);
        float shine = exp(-pow((v + 0.35) / 0.4, 2.0));
        float3 metal = mix(float3(0.3, 0.31, 0.33), float3(0.9, 0.9, 0.92), shine);
        col = mix(col, metal, on * k);
    }
    return col;
}

// The pen at a point: its wobble and the paper's tooth under it.
struct Pen {
    float wob;
    float tooth;
};

Pen pen_at(float2 p) {
    Pen pen;
    pen.wob = (vnoise(p * 0.04 + 3.0) - 0.5) * 1.6;
    pen.tooth = 0.7 + 0.3 * vnoise(p * 0.8);
    return pen;
}

float line_h(float2 p, float y, float x0, float x1, float wob) {
    float dx = max(max(x0 - p.x, p.x - x1), 0.0);
    return length(float2(dx, p.y - y - wob));
}
float line_v(float2 p, float x, float y0, float y1, float wob) {
    float dy = max(max(y0 - p.y, p.y - y1), 0.0);
    return length(float2(p.x - x - wob, dy));
}

// How much pencil lies at p along the edges of blk the app no longer draws.
float pencil_for_block(float2 p, ArkosBlock blk, Pen pen) {
    uint edges = uint(blk.edges);
    if (edges == 0u) {
        return 0.0;
    }
    float4 r = blk.rect;
    float x0 = r.x, y0 = r.y, x1 = r.x + r.z, y1 = r.y + r.w;
    float dmin = 1e5;
    if (edges & ARKOS_EDGE_TOP) { dmin = min(dmin, line_h(p, y0, x0, x1, pen.wob)); }
    if (edges & ARKOS_EDGE_RIGHT) { dmin = min(dmin, line_v(p, x1, y0, y1, pen.wob)); }
    if (edges & ARKOS_EDGE_BOTTOM) { dmin = min(dmin, line_h(p, y1, x0, x1, pen.wob)); }
    if (edges & ARKOS_EDGE_LEFT) { dmin = min(dmin, line_v(p, x0, y0, y1, pen.wob)); }
    return (1.0 - smoothstep(0.4, 1.1, dmin)) * pen.tooth;
}

// A highlighter stroke over a highlight's block: the whole of it, its ends ragged by the block's
// seed and the marker's streaks, as a line run over with a marker is.
float highlighter(float2 p, ArkosBlock b) {
    float4 r = b.rect;
    float seed = b.seed * 2.3;
    float over_l = 2.0 + 4.0 * hash1(seed);
    float over_r = 2.0 + 4.0 * hash1(seed + 1.0);
    float x0 = r.x - over_l + (vnoise(float2(p.y * 0.3, seed)) - 0.5) * 5.0;
    float x1 = r.x + r.z + over_r + (vnoise(float2(p.y * 0.3, seed + 9.0)) - 0.5) * 5.0;
    float across = smoothstep(x0 - 1.5, x0 + 1.5, p.x) * (1.0 - smoothstep(x1 - 1.5, x1 + 1.5, p.x));
    float down = smoothstep(r.y + 0.5, r.y + 2.0, p.y) * (1.0 - smoothstep(r.y + r.w - 2.0, r.y + r.w - 0.5, p.y));
    float streak = 0.82 + 0.18 * vnoise(float2(p.x * 0.08, p.y * 0.6) + seed);
    return across * down * streak;
}

// A highlight's strength at full fade, by its ordinal: hovered, selected, both.
constant float HIGHLIGHT_STRENGTH[3] = {0.45, 0.8, 0.9};

fragment float4 arkos_wash(ArkosVertex in [[stage_in]], constant ArkosInputs& u [[buffer(0)]]) {
    float2 p = in.uv * u.size;
    float k = saturate(u.intensity);
    float3 paper = u.colors[ARKOS_WINDOW_BG].rgb;
    float3 pencil = u.colors[ARKOS_TEXT_DIM].rgb;
    Pen pen = pen_at(p);
    int nb = min(int(u.block_count), 48);

    // The ground, wherever no surface is: the cover, faded to a pale mottle under the status
    // bar. Found first, so the marble is worked out only where it shows.
    bool covered = false;
    for (int i = 0; i < nb && !covered; i++) {
        ArkosBlock blk = u.blocks[i];
        int kind = int(blk.kind);
        float4 r = blk.rect;
        bool surface = kind == ARKOS_TITLE_BAR_BLOCK || kind == ARKOS_SIDEBAR_BLOCK || kind == ARKOS_BINDER_BLOCK || kind == ARKOS_TAB_BAR_BLOCK || kind == ARKOS_PANE_BODY_BLOCK || kind == ARKOS_MENU_BLOCK;
        covered = surface && p.x >= r.x && p.x <= r.x + r.z && p.y >= r.y && p.y <= r.y + r.w;
    }
    float3 black = float3(0.08, 0.08, 0.09);
    float3 white = float3(0.93, 0.93, 0.9);
    float3 col = paper;
    if (!covered) {
        col = keep_readable(u, mix(paper, mix(black, white, marble(p)), 0.16 * k));
    }

    float pencilled = 0.0;
    float3 hl = u.colors[ARKOS_FIND_MATCH].rgb;
    for (int i = 0; i < nb; i++) {
        ArkosBlock blk = u.blocks[i];
        int kind = int(blk.kind);
        float4 r = blk.rect;
        bool inside = p.x >= r.x && p.x <= r.x + r.z && p.y >= r.y && p.y <= r.y + r.w;
        if (kind == ARKOS_ROW_HIGHLIGHT_BLOCK || kind == ARKOS_TAB_HIGHLIGHT_BLOCK) {
            // A marker is transparent: it multiplies the page. It reaches a little past its block.
            if (p.x < r.x - 8.0 || p.x > r.x + r.z + 8.0 || p.y < r.y || p.y > r.y + r.w) {
                continue;
            }
            float a = highlighter(p, blk) * HIGHLIGHT_STRENGTH[min(max(int(blk.ordinal), 0), 2)] * saturate(blk.value) * k;
            col = col * mix(float3(1.0), hl, a);
            continue;
        }
        if (inside) {
            if (kind == ARKOS_TITLE_BAR_BLOCK) {
                col = cover(u, p, blk, fibre(u, paper, p, k), k);
            } else if (kind == ARKOS_SIDEBAR_BLOCK) {
                col = fibre(u, mix(paper, u.colors[ARKOS_SIDEBAR].rgb, k), p, k);
            } else if (kind == ARKOS_BINDER_BLOCK) {
                col = binder(u, p, blk, fibre(u, mix(paper, u.colors[ARKOS_SIDEBAR].rgb, k), p, k), fibre(u, paper, p, k), k);
            } else if (kind == ARKOS_TAB_BAR_BLOCK || kind == ARKOS_TABLE_HEADER_BLOCK) {
                col = fibre(u, mix(paper, u.colors[ARKOS_TAB_BAR].rgb, k), p, k);
            } else if (kind == ARKOS_PANE_BODY_BLOCK || kind == ARKOS_MENU_BLOCK) {
                col = fibre(u, paper, p, k);
                // The margin: a red line a few points in from the page's left edge.
                float m = abs(p.x - (r.x + 6.0) - pen.wob * 0.3);
                float on = (1.0 - smoothstep(0.4, 1.0, m)) * 0.55 * k;
                col = mix(col, u.colors[ARKOS_FOCUS_RING].rgb, on);
            } else if (kind == ARKOS_ROWS_BLOCK && blk.pitch > 2.0) {
                // A rule under each row, in the theme's blue thinned on the paper.
                float dy = fmod(p.y - r.y, blk.pitch);
                float d = abs(dy - (blk.pitch - 0.75));
                float on = (1.0 - smoothstep(0.35, 0.9, d)) * 0.5 * k;
                float3 blue = u.colors[ARKOS_ANSI + 4].rgb;
                col = mix(col, mix(paper, blue, u.dark > 0.5 ? 0.6 : 0.55), on);
            }
        }
        pencilled += pencil_for_block(p, blk, pen);
    }
    col = mix(col, pencil, clamp(pencilled, 0.0, 1.0) * 0.8 * k);
    return float4(col, 1.0);
}

// The menu stage: the open menus' edges in pencil, drawn above the panes, and nothing else. Its
// blocks are only the menus, each with its fade as its value. It returns colour multiplied by
// its alpha.
fragment float4 arkos_overlay(ArkosVertex in [[stage_in]], constant ArkosInputs& u [[buffer(0)]]) {
    float2 p = in.uv * u.size;
    float k = saturate(u.intensity);
    Pen pen = pen_at(p);
    int nb = min(int(u.block_count), 48);
    float pencilled = 0.0;
    for (int i = 0; i < nb; i++) {
        ArkosBlock blk = u.blocks[i];
        pencilled += pencil_for_block(p, blk, pen) * blk.value;
    }
    float a = clamp(pencilled, 0.0, 1.0) * 0.8 * k;
    return float4(u.colors[ARKOS_TEXT_DIM].rgb * a, a);
}
