// The overworld glyphs: the font's own text, with new terminal text typed in like the dialogue of
// a handheld adventure. A glyph whose sprite carries a stamp (the terminal's paint-in,
// effects/paint.rs: the start it works out for each glyph, a line typed within 400 ms, medium4, and
// a few rows at once) is not drawn before its start. From its start it resolves the way a sprite
// does on a small screen, in three steps of FLASH / 3 with no blending between them: first as
// chunky blocks BLOCK_BIG points square, lit; then as blocks BLOCK_SMALL square, in the theme's
// accent; then as the font's own glyph, still a little lit; and after FLASH seconds (200 ms,
// Material's short4) as plain text. The steps cut rather than fade, as a palette swap does. The
// lit colour is a white touched with the accent.
//
// By day, on a light theme, the same three cuts tell the desert's story instead of the screen's:
// the character gathers out of blown sand. First a scatter of grains GRAIN points square, about
// GRAIN_FEW of the glyph's, in gold and a grain above where they will lie; then most of them
// (GRAIN_MOST), in the theme's accent, half a grain above; then the whole glyph, settled and
// still warm; then plain ink. Each grain is there or not by its place alone, so the scatter is
// the same on every frame of a cut and the sand does not boil.
//
// The terminal's other stamps are drawn at once and plainly, since a dialogue box has no ink to
// soak or dry: a change in place (ARKOS_PAINT_SOAK, ARKOS_PAINT_FLIP_SOAK) is the new glyph, and
// the ghost of what it replaced (ARKOS_PAINT_FADE, ARKOS_PAINT_FLIP_FADE) is nothing; a working
// mark is the one steady glyph the terminal paints for it (ARKOS_PAINT_DOODLE) with its blinking
// cell hidden (ARKOS_PAINT_HIDE); a cell background (ARKOS_PAINT_WASH and the three after it) is
// its band, filled. A glyph with no stamp, all the text outside a terminal and all of it under
// Reduce Motion, is the font's glyph with its edge drawn hard (EDGE_FROM, EDGE_TO) and nothing
// more: no bleed, no shadow, no movement.
//
// gpui's sprite declarations come before this file, then the window's lines: ARKOS_SCALE,
// ARKOS_CALM_ALPHA, ARKOS_ACCENT, ARKOS_LIGHT_GROUND and the paint-in's constants.

// Every glyph's edge is drawn hard: its coverage is cut between EDGE_FROM and EDGE_TO, so a pixel
// face's pixels end where they end instead of in a soft fringe of antialiasing.
constant float EDGE_FROM = 0.3;
constant float EDGE_TO = 0.7;
constant float FLASH = 0.2;
constant float BLOCK_BIG = 3.0;
constant float BLOCK_SMALL = 1.5;
constant float FLASH_ACCENT = 0.3;
constant float GRAIN = 1.5;
constant float GRAIN_FEW = 0.35;
constant float GRAIN_MOST = 0.75;
constant float3 GOLD = float3(0.85, 0.6, 0.1);

struct OverworldGlyphVertexOutput {
  float4 position [[position]];
  float2 tile_position;
  float4 color [[flat]];
  float4 clip_distance;
  // The glyph's tile in the atlas, so a block samples only its own glyph.
  float4 tile_rect [[flat]];
  // The stamp's start on the glyph clock, and whether there is one.
  float2 paint [[flat]];
  // 0 a glyph typed in or unstamped, 1 one drawn at once, 2 one not drawn, 3 a background's band.
  float mode [[flat]];
};

struct OverworldGlyphFragmentInput {
  float4 position [[position]];
  float2 tile_position;
  float4 color [[flat]];
  float4 clip_distance;
  float4 tile_rect [[flat]];
  float2 paint [[flat]];
  float mode [[flat]];
};

vertex OverworldGlyphVertexOutput arkos_glyph_vertex(
    uint unit_vertex_id [[vertex_id]], uint sprite_id [[instance_id]],
    constant float2 *unit_vertices [[buffer(SpriteInputIndex_Vertices)]],
    constant MonochromeSprite *sprites [[buffer(SpriteInputIndex_Sprites)]],
    constant Size_DevicePixels *viewport_size
    [[buffer(SpriteInputIndex_ViewportSize)]],
    constant Size_DevicePixels *atlas_size
    [[buffer(SpriteInputIndex_AtlasTextureSize)]]) {
  float2 unit_vertex = unit_vertices[unit_vertex_id];
  MonochromeSprite sprite = sprites[sprite_id];
  float4 device_position =
      to_device_position_transformed(unit_vertex, sprite.bounds, sprite.transformation, viewport_size);
  float4 clip_distance = distance_from_clip_rect_transformed(unit_vertex, sprite.bounds,
                                                 sprite.content_mask.bounds, sprite.transformation);
  float2 tile_position = to_tile_position(unit_vertex, sprite.tile, atlas_size);
  float2 atlas = float2((float)atlas_size->width, (float)atlas_size->height);
  float2 tile_origin = float2(sprite.tile.bounds.origin.x, sprite.tile.bounds.origin.y);
  float2 tile_size = float2(sprite.tile.bounds.size.width, sprite.tile.bounds.size.height);
  int variant = int(round(sprite.stamp * ARKOS_PAINT_STEPS)) & (ARKOS_PAINT_VARIANTS - 1);
  bool stamped = sprite.stamp != 0.0;
  bool band = variant >= ARKOS_PAINT_WASH && variant < ARKOS_PAINT_WASH + 4;
  bool gone = variant == ARKOS_PAINT_HIDE || variant == ARKOS_PAINT_FADE || variant == ARKOS_PAINT_FLIP_FADE;
  bool at_once = variant == ARKOS_PAINT_SOAK || variant == ARKOS_PAINT_FLIP_SOAK || variant == ARKOS_PAINT_DOODLE;
  return OverworldGlyphVertexOutput{
      device_position,
      tile_position,
      hsla_to_rgba(sprite.color),
      {clip_distance.x, clip_distance.y, clip_distance.z, clip_distance.w},
      float4(tile_origin / atlas, (tile_origin + tile_size) / atlas),
      float2(sprite.stamp, stamped ? 1.0 : 0.0),
      !stamped ? 0.0 : band ? 3.0 : gone ? 2.0 : at_once ? 1.0 : 0.0};
}

fragment float4 arkos_glyph_fragment(
    OverworldGlyphFragmentInput input [[stage_in]],
    constant MonochromeSprite *sprites [[buffer(SpriteInputIndex_Sprites)]],
    texture2d<float> atlas_texture [[texture(SpriteInputIndex_AtlasTexture)]],
    constant float *time [[buffer(5)]]) {
  if (any(input.clip_distance < float4(0.0)) || (input.mode > 1.5 && input.mode < 2.5)) {
    return float4(0.0);
  }
  if (input.mode > 2.5) {
    return input.color;
  }
  constexpr sampler smp(mag_filter::linear, min_filter::linear);
  float coverage = smoothstep(EDGE_FROM, EDGE_TO, atlas_texture.sample(smp, input.tile_position).a);
  float3 rgb = input.color.rgb;
  if (input.mode < 0.5 && input.paint.y > 0.5) {
    // A stamp older than a minute, the clock's wrap among them, was typed long ago.
    float age = *time - input.paint.x;
    if (age > -60.0 && age < 0.0) {
      return float4(0.0);
    }
    if (age >= 0.0 && age < FLASH) {
      int stage = int(age / FLASH * 3.0);
      float2 per_px = float2(dfdx(input.tile_position.x), dfdy(input.tile_position.y));
      if (ARKOS_LIGHT_GROUND) {
        // By day: sand gathering into the character.
        if (stage < 2) {
          float grain = GRAIN * ARKOS_SCALE;
          float2 cell = floor(input.position.xy / grain);
          // The grain's own place in the glyph, lifted a grain, then half of one, above it.
          float2 to_middle = (cell + 0.5) * grain - input.position.xy + float2(0.0, (stage == 0 ? 1.0 : 0.5) * grain);
          float2 uv = input.tile_position + per_px * to_middle;
          bool inside = all(uv >= input.tile_rect.xy) && all(uv <= input.tile_rect.zw);
          float there = fract(sin(dot(cell, float2(12.9898, 78.233))) * 43758.5453);
          bool shown = there < (stage == 0 ? GRAIN_FEW : GRAIN_MOST);
          coverage = inside && shown ? step(0.3, atlas_texture.sample(smp, uv).a) : 0.0;
        }
        rgb = stage == 0 ? GOLD : stage == 1 ? ARKOS_ACCENT : mix(rgb, ARKOS_ACCENT, 0.45);
      } else {
        // By night: a sprite resolving on a small screen.
        float3 lit = mix(float3(1.0), ARKOS_ACCENT, FLASH_ACCENT);
        float block = (stage == 0 ? BLOCK_BIG : stage == 1 ? BLOCK_SMALL : 0.0) * ARKOS_SCALE;
        if (block > 0.0) {
          // One sample at the middle of the block this pixel is in, cut to on or off: the glyph
          // as a handful of chunky pixels. A block's middle outside the glyph's tile is empty.
          float2 to_middle = (floor(input.position.xy / block) + 0.5) * block - input.position.xy;
          float2 uv = input.tile_position + per_px * to_middle;
          bool inside = all(uv >= input.tile_rect.xy) && all(uv <= input.tile_rect.zw);
          coverage = inside ? step(0.3, atlas_texture.sample(smp, uv).a) : 0.0;
        }
        rgb = stage == 0 ? lit : stage == 1 ? mix(rgb, ARKOS_ACCENT, 0.6) : mix(rgb, lit, 0.3);
      }
    }
  }
  return float4(rgb, coverage * input.color.a);
}
