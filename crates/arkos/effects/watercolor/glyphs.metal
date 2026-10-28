// Glyphs as if written in ink on watercolor paper. This is appended to gpui's own shader source
// (its sprite declarations come first), and stands in for its monochrome sprite stages. The
// vertex stage widens each glyph's quad by INK_REACH device pixels so the ink can bleed past the
// stroke, and passes the glyph's own tile so samples never pick up a neighbouring glyph in the
// atlas. However dry the pen runs, a stroke's core keeps at least 55% of its coverage, so thin
// text stays legible.
constant float INK_REACH = 3.0;

struct MonochromeInkVertexOutput {
  float4 position [[position]];
  float2 tile_position;
  float4 color [[flat]];
  float4 clip_distance;
  float4 tile_rect [[flat]];
  float glyph_h [[flat]];
};

struct MonochromeInkFragmentInput {
  float4 position [[position]];
  float2 tile_position;
  float4 color [[flat]];
  float4 clip_distance;
  float4 tile_rect [[flat]];
  float glyph_h [[flat]];
};

vertex MonochromeInkVertexOutput arkos_glyph_vertex(
    uint unit_vertex_id [[vertex_id]], uint sprite_id [[instance_id]],
    constant float2 *unit_vertices [[buffer(SpriteInputIndex_Vertices)]],
    constant MonochromeSprite *sprites [[buffer(SpriteInputIndex_Sprites)]],
    constant Size_DevicePixels *viewport_size
    [[buffer(SpriteInputIndex_ViewportSize)]],
    constant Size_DevicePixels *atlas_size
    [[buffer(SpriteInputIndex_AtlasTextureSize)]]) {
  float2 unit_vertex = unit_vertices[unit_vertex_id];
  MonochromeSprite sprite = sprites[sprite_id];
  Bounds_ScaledPixels wide = sprite.bounds;
  wide.origin.x -= INK_REACH;
  wide.origin.y -= INK_REACH;
  wide.size.width += 2.0 * INK_REACH;
  wide.size.height += 2.0 * INK_REACH;
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
  float2 local = unit_vertex * float2(wide.size.width, wide.size.height) - INK_REACH;
  float2 tile_position = (tile_origin + local * per_px) / atlas;
  float4 tile_rect = float4(tile_origin / atlas, (tile_origin + tile_size) / atlas);
  float4 color = hsla_to_rgba(sprite.color);
  return MonochromeInkVertexOutput{
      device_position,
      tile_position,
      color,
      {clip_distance.x, clip_distance.y, clip_distance.z, clip_distance.w},
      tile_rect,
      float(sprite.bounds.size.height)};
}

float arkos_hash(float2 p) {
  return fract(sin(dot(p, float2(12.9898, 78.233))) * 43758.5453);
}

float arkos_noise(float2 p) {
  float2 i = floor(p);
  float2 f = fract(p);
  f = f * f * (3.0 - 2.0 * f);
  return mix(mix(arkos_hash(i), arkos_hash(i + float2(1.0, 0.0)), f.x),
             mix(arkos_hash(i + float2(0.0, 1.0)), arkos_hash(i + float2(1.0, 1.0)), f.x), f.y);
}

// The glyph's coverage at uv, nothing outside its own tile.
float ink_at(texture2d<float> atlas, sampler s, float2 uv, float4 rect) {
  if (any(uv < rect.xy) || any(uv > rect.zw)) {
    return 0.0;
  }
  return atlas.sample(s, uv).a;
}

fragment float4 arkos_glyph_fragment(
    MonochromeInkFragmentInput input [[stage_in]],
    constant MonochromeSprite *sprites [[buffer(SpriteInputIndex_Sprites)]],
    texture2d<float> atlas_texture [[texture(SpriteInputIndex_AtlasTexture)]]) {
  if (any(input.clip_distance < float4(0.0))) {
    return float4(0.0);
  }
  constexpr sampler smp(mag_filter::linear, min_filter::linear);
  float2 px = input.position.xy;
  float2 texel = 1.0 / float2(atlas_texture.get_width(), atlas_texture.get_height());
  float4 rect = input.tile_rect;

  // Small text (the UI's labels, a tab's title) would see the slow variations below span a whole
  // word and look plain. `small` grows from 1 for a glyph as tall as code text to 2.2 for the
  // smallest labels: the variations come that much finer and the bleed that much stronger,
  // so a label reads as painted as a page does. A period or a comma is clamped, not blown up.
  float small = clamp(22.0 / max(input.glyph_h, 1.0), 1.0, 2.2);

  // The hand: the stroke drifts by up to a pixel along smooth noise.
  float2 wob = (float2(arkos_noise(px * 0.07 * small), arkos_noise(px * 0.07 * small + 17.0)) - 0.5) * 1.8;
  float2 uv = input.tile_position + wob * texel;

  float core = ink_at(atlas_texture, smp, uv, rect);
  // Two rings of samples: a near one for the stroke's own edge, a far one for the bleed.
  float near = 0.0;
  float far = 0.0;
  for (int k = 0; k < 8; k++) {
    float ang = float(k) * 0.785398 + 0.3;
    float2 d = float2(cos(ang), sin(ang));
    near += ink_at(atlas_texture, smp, uv + d * 1.2 * texel, rect);
    far += ink_at(atlas_texture, smp, uv + d * 2.7 * texel, rect);
  }
  near *= 0.125;
  far *= 0.125;

  // Pigment pools at the inside of a stroke's edge, as wet ink dries darker at its rim. A thin
  // stroke is nearly all edge, so it pools a little less or it would only go darker.
  float pool = saturate((core - near) * 2.2) * core * mix(1.0, 0.7, (small - 1.0) / 1.2);
  // The bleed: ink feathers out into the paper, unevenly, more around a small glyph.
  float feather = arkos_noise(px * 0.18 * small + 5.0);
  float halo = max(near, far * 0.8) * (0.25 + 0.3 * feather) * (1.0 - core) * mix(1.0, 1.6, (small - 1.0) / 1.2);
  // The brush pen loads and runs dry: density varies across a word, or across a letter of a
  // label, and swings further on small text.
  float density = mix(0.68, 0.55, (small - 1.0) / 1.2) + mix(0.32, 0.45, (small - 1.0) / 1.2) * arkos_noise(px * 0.035 * small * small + 9.0);
  // The paper's tooth.
  float tooth = 0.86 + 0.14 * arkos_hash(floor(px * 0.7));

  float alpha = saturate(max(core * density * tooth, core * 0.55) + halo + pool * 0.3);
  float3 rgb = input.color.rgb;
  // Darker where it pooled, paler and warmer where it bled.
  rgb *= 1.0 - 0.4 * pool;
  float bled = saturate(halo / max(alpha, 1e-4));
  rgb = mix(rgb, rgb * float3(1.25, 1.1, 0.95) + 0.08, bled * 0.6);
  return float4(rgb, alpha * input.color.a);
}
