// The vertex stage the app supplies to every wash (the preamble's `ArkosVertex`): one triangle
// that covers the window, with `uv` running from (0, 0) at the top left to (1, 1) at the bottom
// right. Appended after the wash, so the compiler's line numbers in the wash stay its own.
vertex ArkosVertex arkos_vertex(uint vid [[vertex_id]]) {
    float2 p = float2(vid == 1 ? 3.0 : -1.0, vid == 2 ? 3.0 : -1.0);
    ArkosVertex o;
    o.pos = float4(p, 0.0, 1.0);
    o.uv = float2((p.x + 1.0) * 0.5, 1.0 - (p.y + 1.0) * 0.5);
    return o;
}

// The same triangle over a part of the window only: a menu's backdrop is rendered into an image
// of its own, and `area` (the menu's place as a share of the window: x, y, width, height) keeps
// `uv` in the window's terms, so the wash inside the menu is the wash that lies behind it.
vertex ArkosVertex arkos_vertex_area(uint vid [[vertex_id]], constant float4& area [[buffer(0)]]) {
    float2 p = float2(vid == 1 ? 3.0 : -1.0, vid == 2 ? 3.0 : -1.0);
    ArkosVertex o;
    o.pos = float4(p, 0.0, 1.0);
    float2 local = float2((p.x + 1.0) * 0.5, 1.0 - (p.y + 1.0) * 0.5);
    o.uv = area.xy + local * area.zw;
    return o;
}
