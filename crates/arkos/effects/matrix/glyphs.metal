// Glyphs as a matrix display: emissive phosphor green, haloed, still, and now and then one letter
// glitches. This is appended to gpui's own shader source (its sprite declarations come first),
// and stands in for its monochrome sprite stages.
//
// It starts from space's holo text (space/glyphs.metal) without its scanlines. Everything is
// measured in points: gpui hands a glyph shader device pixels, and ARKOS_SCALE (the window's
// pixels per point, declared by the window ahead of this source) turns one into the other. The
// quad is widened by REACH so the five point halo is not clipped at the glyph's box, and every
// sample stays inside the glyph's own tile so the halo never picks up a neighbouring glyph in the
// atlas.
//
// The idle glitch: the vertex stages read the clock (seconds since the effect started, exactly
// 0.0 under Reduce Motion) and cut it into slots of GLITCH_SLOT. For each glyph and slot a hash
// of the glyph's place (its bounds' origin in device pixels, stable while a letter stays put) and
// the slot says whether it glitches. A glitching glyph samples its tile mirrored, horizontally,
// vertically or both, and is lit GLITCH_FLASH times brighter, halo and all, for that one slot.
// Slot 0 glitches nothing, so a frozen clock shows still text. The arithmetic is mirrored in Rust
// (crates/arkos/src/effects/matrix.rs, glitch_key and glitches) and a change to one is made to
// both. Between glitches every letter is still and whole; nothing else here moves.
//
// Reading text, the rendered markdown's, is drawn calmly: the app marks it with the alpha
// ARKOS_CALM_ALPHA (declared by the window ahead of this source, as ARKOS_SCALE is), and a glyph
// whose alpha is within 0.002 of it, after any fade, is a soft pale green with no halo and no
// glitch. On a light theme (dark text) it is the dark text below, without the glitch.
//
// Emoji go on the same display (arkos_emoji_vertex and arkos_emoji_fragment, at the end): their
// own colours tinted toward the green, haloed, and glitching by the same rule.

// The letter's light: the theme's colour at full brightness.
constant float BRIGHTNESS = 1.0;
// How far the colour leans toward the green.
constant float TINT = 0.45;
// How much of the lean saturated (coloured ANSI) text is spared, so it keeps its own hue.
constant float KEEP = 0.15;
// The halo's strength.
constant float GLOW = 1.6;
// How far past the glyph's box the halo reaches, in points: its outer ring and a margin.
constant float REACH = 5.5;
// The length of a glitch, and of the slots the clock is cut into, in seconds.
constant float GLITCH_SLOT = 0.1;
// The chance a glyph glitches in a slot: about 3 a second across 3,000 glyphs.
constant float GLITCH_CHANCE = 0.0001;
// How much brighter a glitching glyph and its halo are lit.
constant float GLITCH_FLASH = 1.4;
// The phosphor green the text and its halo lean toward: soft rather than full phosphor, as the
// matrix theme's text is, so long reading is easy on the eyes.
constant float3 MATRIX_GREEN = float3(0.44, 0.8, 0.49);
// Dark text (a light theme with matrix chosen) leans toward a deep green instead.
constant float3 MATRIX_DEEP = float3(0.0, 0.4, 0.15);
// The soft pale green reading text leans toward.
constant float3 CALM_GREEN = float3(0.72, 0.86, 0.72);
// How far reading text leans toward it, so links and headings keep a hint of their own colour.
constant float CALM_TINT = 0.6;
// Reading text's light: a little under the display's, easier on the eyes.
constant float CALM_BRIGHTNESS = 0.9;

// The colour the greens above were made around: the matrix theme's focus ring. Under another
// theme each green is turned round the colour wheel by the distance from this hue to the hue of
// the theme's focus ring (ARKOS_ACCENT, declared by the window ahead of this source), keeping
// its saturation and brightness, so navy's text leans blue; a ring with under MIN_SATURATION
// has no hue to speak of and turns nothing. The wash carries the same arithmetic, mirrored in
// crates/arkos/src/effects/accent.rs; a change to one is made to all three.
constant float3 HOME = float3(0.494118, 0.839216, 0.419608);
constant float MIN_SATURATION = 0.2;

// Hue (0 to 1, a full turn), saturation and value.
float3 rgb_to_hsv(float3 c) {
  float4 K = float4(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
  float4 p = mix(float4(c.bg, K.wz), float4(c.gb, K.xy), step(c.b, c.g));
  float4 q = mix(float4(p.xyw, c.r), float4(c.r, p.yzx), step(p.x, c.r));
  float d = q.x - min(q.w, q.y);
  return float3(abs(q.z + (q.w - q.y) / (6.0 * d + 1e-10)), d / (q.x + 1e-10), q.x);
}

float3 hsv_to_rgb(float3 c) {
  float3 p = abs(fract(c.xxx + float3(1.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0);
  return c.z * mix(float3(1.0), saturate(p - 1.0), c.y);
}

float3 toward_accent(float3 c, float3 accent) {
  float3 a = rgb_to_hsv(accent);
  if (a.y < MIN_SATURATION) {
    return c;
  }
  float3 h = rgb_to_hsv(c);
  h.x = fract(h.x + a.x - rgb_to_hsv(HOME).x + 1.0);
  return hsv_to_rgb(h);
}

// Whether a glyph's colour alpha marks it as reading text: within 0.002 of the window's
// ARKOS_CALM_ALPHA, so a fade that scales the alpha a little still counts.
bool calm_text(float alpha) {
  return abs(alpha - ARKOS_CALM_ALPHA) < 0.002;
}

// A hash of n in 0 to 1, in integer arithmetic (PCG's output step) so the Rust mirror agrees to
// the bit: the top 24 bits, times a power of two, are exact in a float. The wash's own copy.
float hash01(uint n) {
    uint h = n * 747796405u + 2891336453u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    h ^= h >> 22u;
    return float(h >> 8u) * (1.0 / 16777216.0);
}

// The glyph's place for the hash: its bounds' origin in whole device pixels, never below zero.
uint2 glitch_origin(Bounds_ScaledPixels bounds) {
  return uint2(uint(max(bounds.origin.x, 0.0)), uint(max(bounds.origin.y, 0.0)));
}

// The hash input for a glyph at origin (device pixels) in slot, as the mirror's glitch_key.
uint glitch_key(uint2 origin, uint slot) {
  return (origin.x * 73856093u) ^ (origin.y * 19349663u) ^ (slot * 83492791u);
}

// How a glyph at origin draws in slot, as the mirror's glitches: its flash in x and its mirror,
// 1 or -1 an axis, in y and z. Mode 1 mirrors horizontally, 2 vertically, 3 both.
float3 glitch(uint2 origin, uint slot) {
  uint key = glitch_key(origin, slot);
  if (slot == 0u || hash01(key) >= GLITCH_CHANCE) {
    return float3(1.0);
  }
  uint mode = 1u + uint(hash01(key ^ 0x9E3779B9u) * 3.0);
  return float3(GLITCH_FLASH, (mode & 1u) != 0u ? -1.0 : 1.0, (mode & 2u) != 0u ? -1.0 : 1.0);
}

// A sample's place in the atlas, reflected inside the glyph's own tile on each mirrored axis,
// so a glitching glyph and its halo rings read the tile mirrored and no sample leaves it.
float2 glitch_uv(float2 uv, float4 rect, float2 mirror) {
  return float2(mirror.x < 0.0 ? rect.x + rect.z - uv.x : uv.x,
                mirror.y < 0.0 ? rect.y + rect.w - uv.y : uv.y);
}

struct MatrixVertexOutput {
  float4 position [[position]];
  float2 tile_position;
  float4 color [[flat]];
  float4 clip_distance;
  float4 tile_rect [[flat]];
  float flash [[flat]];
  float2 mirror [[flat]];
};

struct MatrixFragmentInput {
  float4 position [[position]];
  float2 tile_position;
  float4 color [[flat]];
  float4 clip_distance;
  float4 tile_rect [[flat]];
  float flash [[flat]];
  float2 mirror [[flat]];
};

vertex MatrixVertexOutput arkos_glyph_vertex(
    uint unit_vertex_id [[vertex_id]], uint sprite_id [[instance_id]],
    constant float2 *unit_vertices [[buffer(SpriteInputIndex_Vertices)]],
    constant MonochromeSprite *sprites [[buffer(SpriteInputIndex_Sprites)]],
    constant Size_DevicePixels *viewport_size
    [[buffer(SpriteInputIndex_ViewportSize)]],
    constant Size_DevicePixels *atlas_size
    [[buffer(SpriteInputIndex_AtlasTextureSize)]],
    constant float *time [[buffer(5)]]) {
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
  uint slot = uint(floor(*time / GLITCH_SLOT));
  // Reading text never glitches: no mirror and no flash.
  float3 g = calm_text(color.a) ? float3(1.0) : glitch(glitch_origin(sprite.bounds), slot);
  return MatrixVertexOutput{
      device_position,
      tile_position,
      color,
      {clip_distance.x, clip_distance.y, clip_distance.z, clip_distance.w},
      tile_rect,
      g.x,
      g.yz};
}

// The glyph's coverage at uv, read mirrored as the glitch says and nothing outside its own tile.
float matrix_tx(texture2d<float> atlas, sampler s, float2 uv, float4 rect, float2 mirror) {
  uv = glitch_uv(uv, rect, mirror);
  if (any(uv < rect.xy) || any(uv > rect.zw)) {
    return 0.0;
  }
  return atlas.sample(s, uv).a;
}

// The mean coverage on a ring of eight samples r points out. Its colour is the glyph's own, one
// colour a glyph, so only the coverage is carried. The offsets assume a tile's texels match its
// bounds' device pixels (an unscaled glyph), as watercolor's do.
float matrix_ring(texture2d<float> atlas, sampler s, float2 uv, float4 rect, float2 mirror, float2 texel, float r) {
  float acc = 0.0;
  for (int k = 0; k < 8; k++) {
    float an = float(k) * 0.785398 + 0.3;
    acc += matrix_tx(atlas, s, uv + float2(cos(an), sin(an)) * r * ARKOS_SCALE * texel, rect, mirror);
  }
  return acc / 8.0;
}

fragment float4 arkos_glyph_fragment(
    MatrixFragmentInput input [[stage_in]],
    constant MonochromeSprite *sprites [[buffer(SpriteInputIndex_Sprites)]],
    texture2d<float> atlas_texture [[texture(SpriteInputIndex_AtlasTexture)]]) {
  if (any(input.clip_distance < float4(0.0))) {
    return float4(0.0);
  }
  constexpr sampler smp(mag_filter::linear, min_filter::linear);
  float2 texel = 1.0 / float2(atlas_texture.get_width(), atlas_texture.get_height());
  float4 rect = input.tile_rect;
  float2 uv = input.tile_position;
  float2 mirror = input.mirror;

  float core = matrix_tx(atlas_texture, smp, uv, rect, mirror);
  float a = core;
  float3 col = input.color.rgb;

  // The display assumes light text on a dark ground. Dark text leans toward the deep green,
  // keeps its full brightness and draws no halo and no flash, so it stays readable on a light
  // theme (a light theme's text sits under 0.45). The matrix theme keeps its text above this,
  // so all of it glows. The terminal's decode freshens by the same rule (DARK_TEXT in
  // crates/arkos/src/effects/decode.rs): a change here is made there too.
  float luminance = dot(col, float3(0.2126, 0.7152, 0.0722));
  bool dark = luminance < 0.45;
  float3 green = toward_accent(MATRIX_GREEN, ARKOS_ACCENT);
  float3 hue = dark ? toward_accent(MATRIX_DEEP, ARKOS_ACCENT) : green;

  // Reading text on a dark ground: a soft pale green at a little less light, with no halo and
  // (from the vertex stage) no glitch, its coverage the glyph's own. Dark reading text goes on
  // as dark text, which has no halo and no flash already.
  bool calm = calm_text(input.color.a);
  if (calm && !dark) {
    float3 soft = mix(col, toward_accent(CALM_GREEN, ARKOS_ACCENT), CALM_TINT) * CALM_BRIGHTNESS;
    return float4(soft, core * input.color.a);
  }

  // The halo's three rings, skipped for dark text, which draws none.
  float g = 0.0;
  if (!dark) {
    g = matrix_ring(atlas_texture, smp, uv, rect, mirror, texel, 1.5) * 0.5
      + matrix_ring(atlas_texture, smp, uv, rect, mirror, texel, 3.0) * 0.3
      + matrix_ring(atlas_texture, smp, uv, rect, mirror, texel, 5.0) * 0.2;
  }
  float3 hcol = col;
  float sat = max(max(col.r, col.g), col.b) - min(min(col.r, col.g), col.b);

  // The tint: coloured output keeps as much of its own hue as KEEP says.
  float w = TINT * (1.0 - KEEP * clamp(sat * 2.5, 0.0, 1.0));
  col = mix(col, hue, w);
  float3 gcol = mix(hcol, green, max(w, 0.35));

  float light = dark ? 1.0 : BRIGHTNESS * input.flash;
  float halo = dark ? 0.0 : GLOW * input.flash;

  // The halo is a layer of its own colour over the window under the text, and the letter goes
  // over it; brightness scales the letter's colour, never its coverage.
  float glow_a = saturate(g * halo * 0.55) * (1.0 - a);
  float alpha = a + glow_a;
  // However the light falls, the stroke's core keeps at least 55% of its coverage.
  alpha = max(alpha, core * 0.55);
  float3 rgb = (col * light * a + gcol * glow_a) / max(alpha, 1e-4);
  return float4(rgb, alpha * input.color.a);
}

// Emoji on the matrix display. gpui draws emoji as colour sprites; these stand in for its colour
// sprite stages for emoji only. Each pixel starts from the emoji's own colour where a letter
// starts from its one colour; the rest is the letters' arithmetic, the glitch included.
struct MatrixEmojiVertexOutput {
  float4 position [[position]];
  float2 tile_position;
  float clip_distance [[clip_distance]][4];
  float4 tile_rect [[flat]];
  float opacity [[flat]];
  float flash [[flat]];
  float2 mirror [[flat]];
};

struct MatrixEmojiFragmentInput {
  float4 position [[position]];
  float2 tile_position;
  float4 tile_rect [[flat]];
  float opacity [[flat]];
  float flash [[flat]];
  float2 mirror [[flat]];
};

vertex MatrixEmojiVertexOutput arkos_emoji_vertex(
    uint unit_vertex_id [[vertex_id]], uint sprite_id [[instance_id]],
    constant float2 *unit_vertices [[buffer(SpriteInputIndex_Vertices)]],
    constant PolychromeSprite *sprites [[buffer(SpriteInputIndex_Sprites)]],
    constant Size_DevicePixels *viewport_size
    [[buffer(SpriteInputIndex_ViewportSize)]],
    constant Size_DevicePixels *atlas_size
    [[buffer(SpriteInputIndex_AtlasTextureSize)]],
    constant float *time [[buffer(5)]]) {
  float reach = REACH * ARKOS_SCALE;
  float2 unit_vertex = unit_vertices[unit_vertex_id];
  PolychromeSprite sprite = sprites[sprite_id];
  Bounds_ScaledPixels wide = sprite.bounds;
  wide.origin.x -= reach;
  wide.origin.y -= reach;
  wide.size.width += 2.0 * reach;
  wide.size.height += 2.0 * reach;
  float4 device_position = to_device_position(unit_vertex, wide, viewport_size);
  float4 clip_distance = distance_from_clip_rect(unit_vertex, wide, sprite.content_mask.bounds);
  float2 atlas = float2((float)atlas_size->width, (float)atlas_size->height);
  float2 tile_origin = float2(sprite.tile.bounds.origin.x, sprite.tile.bounds.origin.y);
  float2 tile_size = float2(sprite.tile.bounds.size.width, sprite.tile.bounds.size.height);
  float2 emoji_size = float2(sprite.bounds.size.width, sprite.bounds.size.height);
  float2 per_px = tile_size / max(emoji_size, float2(1.0));
  float2 local = unit_vertex * float2(wide.size.width, wide.size.height) - reach;
  float2 tile_position = (tile_origin + local * per_px) / atlas;
  float4 tile_rect = float4(tile_origin / atlas, (tile_origin + tile_size) / atlas);
  // The same glitch as a letter's, keyed on the emoji's own place.
  uint slot = uint(floor(*time / GLITCH_SLOT));
  float3 g = glitch(glitch_origin(sprite.bounds), slot);
  return MatrixEmojiVertexOutput{
      device_position,
      tile_position,
      {clip_distance.x, clip_distance.y, clip_distance.z, clip_distance.w},
      tile_rect,
      sprite.opacity,
      g.x,
      g.yz};
}

// The emoji's colour at uv, read mirrored as the glitch says and nothing outside its own tile.
float4 matrix_emoji_tx(texture2d<float> atlas, sampler s, float2 uv, float4 rect, float2 mirror) {
  uv = glitch_uv(uv, rect, mirror);
  if (any(uv < rect.xy) || any(uv > rect.zw)) {
    return float4(0.0);
  }
  return atlas.sample(s, uv);
}

// The mean colour, weighted by coverage, on a ring of eight samples r points out.
float3 matrix_emoji_ring_rgb(texture2d<float> atlas, sampler s, float2 uv, float4 rect, float2 mirror, float2 texel, float r) {
  float3 acc = float3(0.0);
  float w = 0.0;
  for (int k = 0; k < 8; k++) {
    float an = float(k) * 0.785398 + 0.3;
    float4 c = matrix_emoji_tx(atlas, s, uv + float2(cos(an), sin(an)) * r * ARKOS_SCALE * texel, rect, mirror);
    acc += c.rgb * c.a;
    w += c.a;
  }
  return acc / max(w, 1e-4);
}

fragment float4 arkos_emoji_fragment(
    MatrixEmojiFragmentInput input [[stage_in]],
    constant PolychromeSprite *sprites [[buffer(SpriteInputIndex_Sprites)]],
    texture2d<float> atlas_texture [[texture(SpriteInputIndex_AtlasTexture)]]) {
  constexpr sampler smp(mag_filter::linear, min_filter::linear);
  float2 texel = 1.0 / float2(atlas_texture.get_width(), atlas_texture.get_height());
  float4 rect = input.tile_rect;
  float2 uv = input.tile_position;
  float2 mirror = input.mirror;

  float4 core = matrix_emoji_tx(atlas_texture, smp, uv, rect, mirror);
  float a = core.a;
  float3 col = core.rgb;

  // The halo's three rings, as a letter's (matrix_ring reads coverage, which an emoji has too).
  float g = matrix_ring(atlas_texture, smp, uv, rect, mirror, texel, 1.5) * 0.5
          + matrix_ring(atlas_texture, smp, uv, rect, mirror, texel, 3.0) * 0.3
          + matrix_ring(atlas_texture, smp, uv, rect, mirror, texel, 5.0) * 0.2;
  // Past the emoji's edge there is no colour of its own: the halo takes the colour nearby.
  float3 hcol = a > 0.0 ? col : matrix_emoji_ring_rgb(atlas_texture, smp, uv, rect, mirror, texel, 3.0);
  float sat = max(max(col.r, col.g), col.b) - min(min(col.r, col.g), col.b);

  // The tint, the halo's colour and the light, as a letter's.
  float w = TINT * (1.0 - KEEP * clamp(sat * 2.5, 0.0, 1.0));
  float3 green = toward_accent(MATRIX_GREEN, ARKOS_ACCENT);
  col = mix(col, green, w);
  float3 gcol = mix(hcol, green, max(w, 0.35));
  float light = BRIGHTNESS * input.flash;

  float glow_a = saturate(g * GLOW * input.flash * 0.55) * (1.0 - a);
  float alpha = a + glow_a;
  float3 rgb = (col * light * a + gcol * glow_a) / max(alpha, 1e-4);
  return float4(rgb, alpha * input.opacity);
}
