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
