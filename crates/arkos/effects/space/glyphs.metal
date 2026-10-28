// Glyphs as a holo display: tinted toward cyan, scanlined, haloed, with a faint ripple of light
// running along the lines. This is appended to gpui's own shader source (its sprite declarations
// come first), and stands in for its monochrome sprite stages.
//
// It is the mock-up's fragment shader (holo-motion-dim.html, motion 5, "Shimmer") ported line
// for line, at Cyrus's tuned values, which are fixed. Everything is measured in points: gpui
// hands a glyph shader device pixels, and ARKOS_SCALE (the window's pixels per point, declared
// by the window ahead of this source) turns one into the other. The quad is widened by REACH so
// the five point halo is not clipped at the glyph's box, and every sample stays inside the
// glyph's own tile so the halo never picks up a neighbouring glyph in the atlas.
//
// The clock is the window's effect clock, at buffer 5; Reduce Motion freezes it.

constant float BRIGHTNESS = 0.80;
constant float TINT = 0.25;
constant float KEEP = 0.15;
constant float SCANLINES = 0.08;
constant float GLOW = 0.90;
constant float SHIMMER = 1.00;
// How far past the glyph's box the halo reaches, in points: its outer ring and a margin.
constant float REACH = 5.5;

constant float3 HOLO_CYAN = float3(0.55, 0.95, 1.0);
// Dark text (a light theme with space chosen) leans toward a deep teal instead.
constant float3 HOLO_TEAL = float3(0.0, 0.35, 0.45);

struct HoloVertexOutput {
  float4 position [[position]];
  float2 tile_position;
  float4 color [[flat]];
  float4 clip_distance;
  float4 tile_rect [[flat]];
};

struct HoloFragmentInput {
  float4 position [[position]];
  float2 tile_position;
  float4 color [[flat]];
  float4 clip_distance;
  float4 tile_rect [[flat]];
};

vertex HoloVertexOutput arkos_glyph_vertex(
    uint unit_vertex_id [[vertex_id]], uint sprite_id [[instance_id]],
    constant float2 *unit_vertices [[buffer(SpriteInputIndex_Vertices)]],
    constant MonochromeSprite *sprites [[buffer(SpriteInputIndex_Sprites)]],
    constant Size_DevicePixels *viewport_size
    [[buffer(SpriteInputIndex_ViewportSize)]],
    constant Size_DevicePixels *atlas_size
    [[buffer(SpriteInputIndex_AtlasTextureSize)]]) {
  float reach = REACH * ARKOS_SCALE;
  float2 unit_vertex = unit_vertices[unit_vertex_id];
  MonochromeSprite sprite = sprites[sprite_id];
  Bounds_ScaledPixels wide = sprite.bounds;
  wide.origin.x -= reach;
  wide.origin.y -= reach;
  wide.size.width += 2.0 * reach;
  wide.size.height += 2.0 * reach;
  float4 device_position =
      to_device_position_transformed(unit_vertex, wide, sprite.transformation, viewport_size);
  float4 clip_distance = distance_from_clip_rect_transformed(unit_vertex, wide,
                                                 sprite.content_mask.bounds, sprite.transformation);
  float2 atlas = float2((float)atlas_size->width, (float)atlas_size->height);
  float2 tile_origin = float2(sprite.tile.bounds.origin.x, sprite.tile.bounds.origin.y);
  float2 tile_size = float2(sprite.tile.bounds.size.width, sprite.tile.bounds.size.height);
  float2 glyph_size = float2(sprite.bounds.size.width, sprite.bounds.size.height);
  // Texels per device pixel (1 unless the glyph is scaled), so the reach maps into the atlas.
  float2 per_px = tile_size / max(glyph_size, float2(1.0));
  float2 local = unit_vertex * float2(wide.size.width, wide.size.height) - reach;
  float2 tile_position = (tile_origin + local * per_px) / atlas;
  float4 tile_rect = float4(tile_origin / atlas, (tile_origin + tile_size) / atlas);
  float4 color = hsla_to_rgba(sprite.color);
  return HoloVertexOutput{
      device_position,
      tile_position,
      color,
      {clip_distance.x, clip_distance.y, clip_distance.z, clip_distance.w},
      tile_rect};
}

// The glyph's coverage at uv, nothing outside its own tile (the mock-up's tx(p).a).
float holo_tx(texture2d<float> atlas, sampler s, float2 uv, float4 rect) {
  if (any(uv < rect.xy) || any(uv > rect.zw)) {
    return 0.0;
  }
  return atlas.sample(s, uv).a;
}

// The mean coverage on a ring of eight samples r points out (the mock-up's ring(p, r)). Its
// colour is the glyph's own, one colour a glyph, so only the coverage is carried. The offsets
// assume a tile's texels match its bounds' device pixels (an unscaled glyph), as watercolor's do.
float holo_ring(texture2d<float> atlas, sampler s, float2 uv, float4 rect, float2 texel, float r) {
  float acc = 0.0;
  for (int k = 0; k < 8; k++) {
    float an = float(k) * 0.785398 + 0.3;
    acc += holo_tx(atlas, s, uv + float2(cos(an), sin(an)) * r * ARKOS_SCALE * texel, rect);
  }
  return acc / 8.0;
}

fragment float4 arkos_glyph_fragment(
    HoloFragmentInput input [[stage_in]],
    constant MonochromeSprite *sprites [[buffer(SpriteInputIndex_Sprites)]],
    texture2d<float> atlas_texture [[texture(SpriteInputIndex_AtlasTexture)]],
    constant float *time [[buffer(5)]]) {
  if (any(input.clip_distance < float4(0.0))) {
    return float4(0.0);
  }
  constexpr sampler smp(mag_filter::linear, min_filter::linear);
  float t = *time;
  // The pixel's place in points.
  float2 q = input.position.xy / ARKOS_SCALE;
  float2 texel = 1.0 / float2(atlas_texture.get_width(), atlas_texture.get_height());
  float4 rect = input.tile_rect;
  float2 uv = input.tile_position;

  float core = holo_tx(atlas_texture, smp, uv, rect);
  float a = core;
  float3 col = input.color.rgb;

  // The holo assumes light text on a dark ground. Dark text leans toward deep teal instead of
  // cyan, keeps its full brightness and draws no glow, so it stays readable on a light theme.
  float luminance = dot(col, float3(0.2126, 0.7152, 0.0722));
  bool dark = luminance < 0.5;
  float3 hue = dark ? HOLO_TEAL : HOLO_CYAN;

  // The halo's three rings, skipped for dark text, which draws none.
  float g = 0.0;
  if (!dark) {
    g = holo_ring(atlas_texture, smp, uv, rect, texel, 1.5) * 0.5
      + holo_ring(atlas_texture, smp, uv, rect, texel, 3.0) * 0.3
      + holo_ring(atlas_texture, smp, uv, rect, texel, 5.0) * 0.2;
  }
  float3 hcol = col;
  float sat = max(max(col.r, col.g), col.b) - min(min(col.r, col.g), col.b);

  // The tint: coloured output keeps as much of its own hue as KEEP says.
  float w = TINT * (1.0 - KEEP * clamp(sat * 2.5, 0.0, 1.0));
  col = mix(col, hue, w);
  float3 gcol = mix(hcol, HOLO_CYAN, max(w, 0.35));

  // Scanlines every three points, smooth.
  float phase = q.y / 3.0;
  float line = 1.0 - SCANLINES * 0.5 * (1.0 + cos(6.2832 * phase));

  float light = (dark ? 1.0 : BRIGHTNESS) * line;
  float halo = dark ? 0.0 : GLOW;
  // The shimmer: a faint ripple of light running along the lines.
  light *= 1.0 + 0.09 * SHIMMER * sin(q.x * 0.07 - t * 2.2 + q.y * 0.015); // 2.2 is in step with GLYPH_CLOCK_WRAP (effects/active.rs)

  // The mock-up adds the halo to the ground and lays the letter over it:
  //   c = mix(base + gcol * g * halo * 0.55, col * light, a).
  // Here the ground is the window under the text, so the halo is a layer of its own colour and
  // the letter goes over it; brightness scales the letter's colour, never its coverage.
  float glow_a = saturate(g * halo * 0.55) * (1.0 - a);
  float alpha = a + glow_a;
  // However the light falls, the stroke's core keeps at least 55% of its coverage.
  alpha = max(alpha, core * 0.55);
  float3 rgb = (col * light * a + gcol * glow_a) / max(alpha, 1e-4);
  return float4(rgb, alpha * input.color.a);
}
