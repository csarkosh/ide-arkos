// The Material wash under the Arkos window: Material 3 as a web workspace wears it. The title
// bar, the sidebar and the status bar are one frame in the theme's sidebar colour, and the title
// bar holds a search bar, a full pill a shade of the accent darker, round the title's words.
// Each pane is a card on the frame: its tab strip and its page in the theme's window colour,
// rounded at its four corners, the frame showing in the gap the manifest keeps between them; a
// hairline in the border colour under the strip, the pane's number on a round chip of its own hue
// (the theme's blue, green, yellow and red in turn, as tonal containers), and each active tab
// underlined in the accent, Material's tab indicator. The selected Explorer row is the
// navigation drawer's pill in the theme's selection colour, inset from the sidebar's edges; a
// hovered row or tab is a state layer of the text at 8%, with a ripple spreading from its centre
// as the hover comes. A pane whose Claude session works carries Material's indeterminate linear
// progress indicator along the bottom of its strip. Every container keeps the theme's dim text at
// 4.5:1 or better, and every colour is the theme's. The preamble (arkos.metal) is prepended
// before this is compiled, and declares ArkosInputs, ArkosBlock, ArkosVertex and the ARKOS_*
// indices used here.

// Material's state layer opacities: hover 8%, the ripple 12%.
constant float HOVER_LAYER = 0.08;
constant float RIPPLE_LAYER = 0.12;
// A card's corner radius, in points: the manifest's `pane_radius`, so the app's focus ring
// follows it.
constant float CARD_RADIUS = 14.0;
// The drawer's pill, in from the sidebar's edges.
constant float PILL_INSET = 8.0;
// The pane number's chip: its centre from the strip's left, its radius, and its container.
constant float CHIP_X = 12.0;
constant float CHIP_R = 9.0;
constant float CHIP_TONE = 0.28;
// The tab indicator: its height, and its inset from the tab's ends (the tab's own padding, so it
// runs under the label).
constant float INDICATOR_H = 3.0;
constant float INDICATOR_INSET = 10.0;
// The search bar: from past the traffic lights to clear of the bar's icons, a little in from the
// bar's top and bottom, the frame with a touch of the accent.
constant float SEARCH_LEFT = 76.0;
constant float SEARCH_RIGHT = 84.0;
constant float SEARCH_INSET = 3.0;
constant float SEARCH_TONE = 0.07;
// The progress indicator's height in points and its cycle in seconds.
constant float TRACK_H = 3.0;
constant float CYCLE = 2.0;
// Each pane's chip, by its place among the panes: the ANSI blue, green, yellow and red.
constant int PANE_HUES[4] = {4, 2, 3, 1};

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

// The least contrast a container keeps against the dim text drawn on it (WCAG AA).
constant float READABLE = 4.5;

// A tonal container of `pig` over `ground`: the pigment laid in at `a`, then moved back toward
// the ground just far enough that the theme's dim text keeps READABLE on it.
float3 container(constant ArkosInputs& u, float3 pig, float a, float3 ground) {
    float3 col = mix(ground, pig, a);
    float lt = rel_lum(u.colors[ARKOS_TEXT_DIM].rgb);
    float lc = rel_lum(col);
    float lp = rel_lum(ground);
    float f = 0.0;
    if (lt < lp) {
        float least = READABLE * (lt + 0.05) - 0.05;
        if (lc < least) { f = saturate((least - lc) / max(lp - lc, 1e-5)); }
    } else {
        float most = (lt + 0.05) / READABLE - 0.05;
        if (lc > most) { f = saturate((lc - most) / max(lc - lp, 1e-5)); }
    }
    return to_srgb(mix(to_linear(col), to_linear(ground), f));
}

float sd_round_box(float2 q, float2 hb, float r) {
    float2 d = abs(q) - hb + r;
    return length(max(d, 0.0)) + min(max(d.x, d.y), 0.0) - r;
}

// How much of a rounded box (x, y, w, h) with corner radius `radius` lies at p, its edge softened
// by half a point.
float round_box(float2 p, float4 r, float radius) {
    float2 hb = max(r.zw * 0.5, 0.0);
    float d = sd_round_box(p - (r.xy + hb), hb, min(radius, min(hb.x, hb.y)));
    return 1.0 - smoothstep(-0.5, 0.5, d);
}

// Material's emphasized decelerate curve, near enough: the bar's head and tail each run on it.
float ease(float t) {
    t = saturate(t);
    return 1.0 - pow(1.0 - t, 3.0);
}

// Where the indeterminate bar is at t seconds into its cycle, as fractions of the track: its
// tail and its head, the head leading on Material's curve and the tail following a beat later.
float2 bar_span(float t) {
    float phase = fract(t / CYCLE);
    float head = ease(phase / 0.7);
    float tail = ease((phase - 0.3) / 0.7);
    return float2(tail, head);
}

// How much of a one-point line lies at p along the edges of b the app no longer draws, softened
// by half a pixel so it reads crisp at any scale.
float edge_line(float2 p, ArkosBlock b, float scale) {
    uint edges = uint(b.edges);
    if (edges == 0u) {
        return 0.0;
    }
    float4 r = b.rect;
    float x0 = r.x, y0 = r.y, x1 = r.x + r.z, y1 = r.y + r.w;
    float d = 1e5;
    if (edges & ARKOS_EDGE_TOP) { d = min(d, max(abs(p.y - y0), max(x0 - p.x, p.x - x1))); }
    if (edges & ARKOS_EDGE_BOTTOM) { d = min(d, max(abs(p.y - y1), max(x0 - p.x, p.x - x1))); }
    if (edges & ARKOS_EDGE_LEFT) { d = min(d, max(abs(p.x - x0), max(y0 - p.y, p.y - y1))); }
    if (edges & ARKOS_EDGE_RIGHT) { d = min(d, max(abs(p.x - x1), max(y0 - p.y, p.y - y1))); }
    float soft = 0.5 / max(scale, 1.0);
    return 1.0 - smoothstep(0.5 - soft, 0.5 + soft, d);
}

fragment float4 arkos_wash(ArkosVertex in [[stage_in]], constant ArkosInputs& u [[buffer(0)]]) {
    float2 p = in.uv * u.size;
    float k = saturate(u.intensity);
    float3 frame = u.colors[ARKOS_SIDEBAR].rgb;
    float3 card = u.colors[ARKOS_WINDOW_BG].rgb;
    float3 primary = u.colors[ARKOS_FOCUS_RING].rgb;
    float3 ink = u.colors[ARKOS_TEXT].rgb;
    float3 col = frame;
    float lined = 0.0;
    int nb = min(int(u.block_count), 48);
    for (int i = 0; i < nb; i++) {
        ArkosBlock blk = u.blocks[i];
        int kind = int(blk.kind);
        float4 r = blk.rect;
        bool inside = p.x >= r.x && p.x <= r.x + r.z && p.y >= r.y && p.y <= r.y + r.w;
        if (kind == ARKOS_ROW_HIGHLIGHT_BLOCK || kind == ARKOS_TAB_HIGHLIGHT_BLOCK) {
            bool row = kind == ARKOS_ROW_HIGHLIGHT_BLOCK;
            // A row's pill is inset from the sidebar's edges; a tab's from the strip's top and
            // bottom. Both are full pills.
            float4 pr = row ? float4(r.x + PILL_INSET, r.y, r.z - 2.0 * PILL_INSET, r.w) : float4(r.x + 2.0, r.y + 4.0, r.z - 4.0, r.w - 8.0);
            float shape = round_box(p, pr, 1e4);
            if (shape <= 0.0) {
                continue;
            }
            int state = max(int(blk.ordinal), 0);
            float v = saturate(blk.value) * k;
            if (row && state >= 1) {
                col = mix(col, container(u, u.colors[ARKOS_SELECTION].rgb, 1.0, frame), shape * v);
            }
            if (state == 0 || state == 2) {
                col = mix(col, ink, shape * v * HOVER_LAYER);
            }
            // The ripple: from the centre, reaching the ends as the fade completes, and thinning
            // out as it goes (nothing at either end of the fade).
            float2 c = pr.xy + pr.zw * 0.5;
            float reach = length(pr.zw * 0.5) * saturate(blk.value);
            float ring = 1.0 - smoothstep(reach - 1.0, reach + 1.0, length(p - c));
            float bump = 4.0 * saturate(blk.value) * (1.0 - saturate(blk.value));
            col = mix(col, ink, shape * ring * bump * RIPPLE_LAYER * k);
            continue;
        }
        if (kind == ARKOS_TAB_BAR_BLOCK) {
            // The pane's card: this strip and the page laid out after it.
            float4 cr = r;
            if (i + 1 < nb && int(u.blocks[i + 1].kind) == ARKOS_PANE_BODY_BLOCK) {
                cr.w += u.blocks[i + 1].rect.w;
            }
            float on = round_box(p, cr, CARD_RADIUS);
            if (on > 0.0) {
                col = mix(col, card, on);
                if (inside) {
                    // The hairline under the strip.
                    float hair = 1.0 - smoothstep(0.0, 1.0, abs(p.y - (r.y + r.w - 0.5)));
                    col = mix(col, u.colors[ARKOS_BORDER].rgb, hair * on * k);
                    // The pane's number on its chip.
                    float3 hue = u.colors[ARKOS_ANSI + PANE_HUES[max(int(blk.ordinal), 0) % 4]].rgb;
                    float dc = length(p - float2(r.x + CHIP_X, r.y + r.w * 0.5)) - CHIP_R;
                    col = mix(col, container(u, hue, CHIP_TONE, card), (1.0 - smoothstep(-0.5, 0.5, dc)) * k);
                    // A busy pane's indicator along the bottom of its strip.
                    if (blk.busy > 0.5 && p.y >= r.y + r.w - TRACK_H) {
                        float2 span = bar_span(u.time);
                        float x = (p.x - r.x) / max(r.z, 1.0);
                        col = mix(col, container(u, primary, 0.25, card), k);
                        col = mix(col, primary, step(span.x, x) * step(x, span.y) * k);
                    }
                }
            }
            continue;
        }
        if (kind == ARKOS_ACTIVE_TAB_BLOCK) {
            // The tab indicator: under the label, its top corners rounded.
            float bottom = r.y + r.w;
            float4 ir = float4(r.x + INDICATOR_INSET, bottom - INDICATOR_H, r.z - 2.0 * INDICATOR_INSET, 2.0 * INDICATOR_H);
            float on = round_box(p, ir, INDICATOR_H) * step(p.y, bottom);
            col = mix(col, primary, on * k);
            continue;
        }
        if (!inside) {
            continue;
        }
        if (kind == ARKOS_TITLE_BAR_BLOCK) {
            float4 sr = float4(r.x + SEARCH_LEFT, r.y + SEARCH_INSET, r.z - SEARCH_LEFT - SEARCH_RIGHT, r.w - 2.0 * SEARCH_INSET);
            col = mix(col, container(u, primary, SEARCH_TONE, frame), round_box(p, sr, 1e4) * k);
        } else if (kind == ARKOS_MENU_BLOCK) {
            col = card;
        } else if (kind == ARKOS_TABLE_HEADER_BLOCK) {
            col = mix(col, container(u, primary, 0.1, card), k);
        }
    }
    // Only what sits inside a card or splits the sidebar keeps a line: tables, videos, menus and
    // the sidebar's divider (a block with no height, so looked for outside the loop above, which
    // skips what p is not inside). The cards themselves need none.
    for (int i = 0; i < nb; i++) {
        ArkosBlock blk = u.blocks[i];
        int kind = int(blk.kind);
        if (kind == ARKOS_TABLE_BLOCK || kind == ARKOS_TABLE_HEADER_BLOCK || kind == ARKOS_MEDIA_BLOCK || kind == ARKOS_MENU_BLOCK || kind == ARKOS_DIVIDER_BLOCK) {
            lined = max(lined, edge_line(p, blk, u.scale));
        }
    }
    col = mix(col, u.colors[ARKOS_BORDER].rgb, lined * k);
    return float4(col, 1.0);
}
