// Glyphs as if written in ink on watercolor paper. This is appended to gpui's own shader source
// (its sprite declarations come first), and stands in for its monochrome sprite stages. The
// vertex stage widens each glyph's quad by INK_REACH so the ink can bleed past the stroke, and
// passes the glyph's own tile so samples never pick up a neighbouring glyph in the atlas.
// However dry the pen runs, a stroke's core keeps at least 55% of its coverage, so thin text
// stays legible. Emoji get the same hand (arkos_emoji_vertex and arkos_emoji_fragment, at the
// end): they keep their own colours, drift with the stroke, take the paper's tooth, pool darker
// at their rim and bleed softly in their own colour; pictures are never drawn by these.
//
// New terminal text is written in: a glyph whose sprite carries a stamp (the terminal's paint-in,
// effects/paint.rs) is written in the hand the stamp names (its steps of 1/ARKOS_PAINT_STEPS,
// an entry of ARKOS_PAINT_HAND), so the lines of one output differ. It starts at the stamp plus
// its x in points times ARKOS_PAINT_STAGGER times the hand's pace, and a pen crosses it over
// ARKOS_PAINT_DUR times the hand's glyph time, left to right and down each stroke as a hand
// writes (how much across is the hand's too), with a crisp edge and a wet bead just behind it.
// Text that replaced text in place (a counter ticking) is no pen: it soaks in over its glyph time,
// thickest strokes first and softly, while a ghost of the text it replaced (ARKOS_PAINT_FADE,
// drawn under it by the terminal) lifts off the paper, so the change is one smooth crossfade. The ink it leaves is wetter, deeper and bluer, with
// a wide bleed, and dries to the ink above over ARKOS_PAINT_DRY. A glyph with no stamp (0), or
// one whose ink has dried, is drawn exactly as it is without one.
//
// gpui hands a glyph shader device pixels. Every distance and size below was drawn on a Retina
// screen, two pixels a point, so each is measured in those half points: PER_PX turns them into
// this screen's pixels (ARKOS_SCALE, the window's pixels per point, is declared by the window),
// and a 1x screen sees the same ink rather than one twice as heavy.
constant float PER_PX = ARKOS_SCALE / 2.0;
constant float INK_REACH = 3.0 * PER_PX;
// The width of the pen's edge, in path units (the glyph's box is 0 to 1): narrow, so the ink is
// drawn on rather than faded in.
constant float PEN_EDGE = 0.05;
// How far behind the pen its wet bead reaches, in path units.
constant float PEN_BEAD = 0.16;

struct MonochromeInkVertexOutput {
  float4 position [[position]];
  float2 tile_position;
  float4 color [[flat]];
  float4 clip_distance;
  float4 tile_rect [[flat]];
  float glyph_h [[flat]];
  float4 paint [[flat]];
  float mode [[flat]];
  float2 box;
};

struct MonochromeInkFragmentInput {
  float4 position [[position]];
  float2 tile_position;
  float4 color [[flat]];
  float4 clip_distance;
  float4 tile_rect [[flat]];
  float glyph_h [[flat]];
  float4 paint [[flat]];
  float mode [[flat]];
  float2 box;
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
  // The paint-in: the hand the stamp names, and the start, a cell further right a little later.
  // The paint carries the start, the glyph time (0 for no stamp), across and the bead.
  int variant = int(round(sprite.stamp * ARKOS_PAINT_STEPS)) & (ARKOS_PAINT_VARIANTS - 1);
  float4 hand = ARKOS_PAINT_HAND[variant];
  float start = sprite.stamp + float(sprite.bounds.origin.x) / ARKOS_SCALE * ARKOS_PAINT_STAGGER * hand.x;
  return MonochromeInkVertexOutput{
      device_position,
      tile_position,
      color,
      {clip_distance.x, clip_distance.y, clip_distance.z, clip_distance.w},
      tile_rect,
      float(sprite.bounds.size.height) / PER_PX,
      float4(start, sprite.stamp != 0.0 ? hand.y : 0.0, hand.z, hand.w),
      // 0 for the pen, 1 for a change in place soaking in, 2 for the text it replaced fading out.
      variant == ARKOS_PAINT_FADE ? 2.0 : (variant == ARKOS_PAINT_SOAK ? 1.0 : 0.0),
      // The pixel's place in the glyph's box, 0 to 1 across and down, past it in the reach.
      local / max(glyph_size, float2(1.0))};
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
    texture2d<float> atlas_texture [[texture(SpriteInputIndex_AtlasTexture)]],
    constant float *time [[buffer(5)]]) {
  if (any(input.clip_distance < float4(0.0))) {
    return float4(0.0);
  }
  constexpr sampler smp(mag_filter::linear, min_filter::linear);
  // The pixel's place in half points, so the noise below has the same grain on any screen.
  float2 px = input.position.xy / PER_PX;
  float2 texel = 1.0 / float2(atlas_texture.get_width(), atlas_texture.get_height());
  float4 rect = input.tile_rect;

  // Small text (the UI's labels, a tab's title) would see the slow variations below span a whole
  // word and look plain. `small` grows from 1 for a glyph as tall as code text to 2.2 for the
  // smallest labels: the variations come that much finer and the bleed that much stronger,
  // so a label reads as painted as a page does. A period or a comma is clamped, not blown up.
  float small = clamp(22.0 / max(input.glyph_h, 1.0), 1.0, 2.2);

  // The hand: the stroke drifts by up to a pixel along smooth noise.
  float2 wob = (float2(arkos_noise(px * 0.07 * small), arkos_noise(px * 0.07 * small + 17.0)) - 0.5) * 1.8;
  float2 uv = input.tile_position + wob * PER_PX * texel;

  float core = ink_at(atlas_texture, smp, uv, rect);
  // Two rings of samples: a near one for the stroke's own edge, a far one for the bleed.
  float near = 0.0;
  float far = 0.0;
  for (int k = 0; k < 8; k++) {
    float ang = float(k) * 0.785398 + 0.3;
    float2 d = float2(cos(ang), sin(ang));
    near += ink_at(atlas_texture, smp, uv + d * 1.2 * PER_PX * texel, rect);
    far += ink_at(atlas_texture, smp, uv + d * 2.7 * PER_PX * texel, rect);
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

  // The paint-in: how far the stroke has soaked in (0 to 1) and how wet it still is (1 to 0).
  // A stamp older than a minute, the clock's wrap among them, has long dried.
  float progress = 1.0;
  float wet = 0.0;
  if (input.paint.y > 0.0) {
    float age = *time - input.paint.x;
    float dur = ARKOS_PAINT_DUR * input.paint.y;
    if (age > -60.0) {
      progress = saturate(age / dur);
      wet = age > dur ? saturate(1.0 - (age - dur) / ARKOS_PAINT_DRY) : 1.0;
    }
  }
  // The pen: ink shows where it has passed, the edge a little uneven as a nib's is, and the stretch
  // just behind it is its wet bead. Once across, all of the glyph shows.
  float soak = 1.0;
  float bead = 0.0;
  float bleed = 0.0;
  float thick = saturate(core * 0.45 + near * 0.35 + far * 0.2) + (feather - 0.5) * 0.15;
  if (input.mode > 1.5 && input.paint.y > 0.0) {
    // The ghost of replaced text lifts off, its thin edges first, and is gone once the new text
    // has soaked in; it never reads wet.
    float lift = smoothstep(0.0, 1.0, progress);
    soak = progress >= 1.0 ? 0.0 : saturate((thick + 0.35 - lift * 1.35) * 3.0);
    wet = 0.0;
  } else if (input.mode > 0.5 && (progress < 1.0 || wet > 0.0)) {
    // A change in place soaks in, thickest strokes first, with a soft edge so it eases in.
    float seep = smoothstep(0.0, 1.0, progress);
    soak = progress >= 1.0 ? 1.0 : saturate((thick - (1.0 - seep * 1.35)) * 3.0);
    bleed = max(near, far * 0.9) * (1.0 - core) * (0.6 + 0.4 * feather) * 0.4 * wet * seep;
  } else if (progress < 1.0 || wet > 0.0) {
    float path = dot(saturate(input.box), float2(input.paint.z, 1.0 - input.paint.z)) + (feather - 0.5) * 0.06;
    float behind = progress * (1.0 + PEN_EDGE + PEN_BEAD) - path;
    soak = progress >= 1.0 ? 1.0 : saturate(behind / PEN_EDGE);
    bead = progress >= 1.0 ? 0.0 : saturate(soak * saturate(1.0 - (behind - PEN_EDGE) / PEN_BEAD) * input.paint.w);
    // Wet ink bleeds wide around the stroke behind the pen, and draws back as it dries.
    bleed = max(near, far * 0.9) * (1.0 - core) * (0.6 + 0.4 * feather) * 0.55 * wet * soak;
  }

  float alpha = saturate(max(core * density * tooth, core * 0.55) + halo + pool * 0.3);
  // The bead is fuller ink: the pen's stroke at full coverage where it has just been.
  alpha = saturate(mix(alpha, max(alpha, core), bead) * soak + bleed);
  float3 rgb = input.color.rgb;
  // Darker where it pooled, paler and warmer where it bled.
  rgb *= 1.0 - 0.4 * pool;
  float bled = saturate((halo * soak + bleed) / max(alpha, 1e-4));
  rgb = mix(rgb, rgb * float3(1.25, 1.1, 0.95) + 0.08, bled * 0.6);
  // Wet ink reads deeper and bluer: dark text toward a deep blue, light text toward a pale one.
  if (wet > 0.0) {
    bool dark = ARKOS_LIGHT_GROUND && dot(input.color.rgb, float3(0.2126, 0.7152, 0.0722)) < 0.45;
    float3 wet_rgb = dark ? float3(0.086, 0.118, 0.275) : mix(input.color.rgb, float3(0.70, 0.80, 1.0), 0.5);
    rgb = mix(rgb, wet_rgb, saturate(wet * 0.6 + bead * 0.3));
  }
  return float4(rgb, alpha * input.color.a);
}

// Emoji, in the same ink. gpui draws emoji as colour sprites; these stand in for its colour
// sprite stages for emoji only.
struct EmojiInkVertexOutput {
  float4 position [[position]];
  float2 tile_position;
  float clip_distance [[clip_distance]][4];
  float4 tile_rect [[flat]];
  float opacity [[flat]];
  float glyph_h [[flat]];
};

struct EmojiInkFragmentInput {
  float4 position [[position]];
  float2 tile_position;
  float4 tile_rect [[flat]];
  float opacity [[flat]];
  float glyph_h [[flat]];
};

vertex EmojiInkVertexOutput arkos_emoji_vertex(
    uint unit_vertex_id [[vertex_id]], uint sprite_id [[instance_id]],
    constant float2 *unit_vertices [[buffer(SpriteInputIndex_Vertices)]],
    constant PolychromeSprite *sprites [[buffer(SpriteInputIndex_Sprites)]],
    constant Size_DevicePixels *viewport_size
    [[buffer(SpriteInputIndex_ViewportSize)]],
    constant Size_DevicePixels *atlas_size
    [[buffer(SpriteInputIndex_AtlasTextureSize)]]) {
  float2 unit_vertex = unit_vertices[unit_vertex_id];
  PolychromeSprite sprite = sprites[sprite_id];
  // Widened as a glyph's quad is, so the bleed has room past the emoji's edge.
  Bounds_ScaledPixels wide = sprite.bounds;
  wide.origin.x -= INK_REACH;
  wide.origin.y -= INK_REACH;
  wide.size.width += 2.0 * INK_REACH;
  wide.size.height += 2.0 * INK_REACH;
  float4 device_position = to_device_position(unit_vertex, wide, viewport_size);
  float4 clip_distance = distance_from_clip_rect(unit_vertex, wide, sprite.content_mask.bounds);
  float2 atlas = float2((float)atlas_size->width, (float)atlas_size->height);
  float2 tile_origin = float2(sprite.tile.bounds.origin.x, sprite.tile.bounds.origin.y);
  float2 tile_size = float2(sprite.tile.bounds.size.width, sprite.tile.bounds.size.height);
  float2 emoji_size = float2(sprite.bounds.size.width, sprite.bounds.size.height);
  float2 per_px = tile_size / max(emoji_size, float2(1.0));
  float2 local = unit_vertex * float2(wide.size.width, wide.size.height) - INK_REACH;
  float2 tile_position = (tile_origin + local * per_px) / atlas;
  float4 tile_rect = float4(tile_origin / atlas, (tile_origin + tile_size) / atlas);
  return EmojiInkVertexOutput{
      device_position,
      tile_position,
      {clip_distance.x, clip_distance.y, clip_distance.z, clip_distance.w},
      tile_rect,
      sprite.opacity,
      float(sprite.bounds.size.height) / PER_PX};
}

// The emoji's colour at uv, nothing outside its own tile.
float4 emoji_at(texture2d<float> atlas, sampler s, float2 uv, float4 rect) {
  if (any(uv < rect.xy) || any(uv > rect.zw)) {
    return float4(0.0);
  }
  return atlas.sample(s, uv);
}

fragment float4 arkos_emoji_fragment(
    EmojiInkFragmentInput input [[stage_in]],
    constant PolychromeSprite *sprites [[buffer(SpriteInputIndex_Sprites)]],
    texture2d<float> atlas_texture [[texture(SpriteInputIndex_AtlasTexture)]]) {
  constexpr sampler smp(mag_filter::linear, min_filter::linear);
  float2 px = input.position.xy / PER_PX;
  float2 texel = 1.0 / float2(atlas_texture.get_width(), atlas_texture.get_height());
  float4 rect = input.tile_rect;
  float small = clamp(22.0 / max(input.glyph_h, 1.0), 1.0, 2.2);

  // The hand: a little less drift than a letter's, so the picture stays clear.
  float2 wob = (float2(arkos_noise(px * 0.07 * small), arkos_noise(px * 0.07 * small + 17.0)) - 0.5) * 1.4;
  float2 uv = input.tile_position + wob * PER_PX * texel;

  float4 core = emoji_at(atlas_texture, smp, uv, rect);
  // The same two rings as a letter's: the near one finds the rim, the far one feeds the bleed,
  // whose colour is the emoji's own nearby.
  float near = 0.0;
  float far = 0.0;
  float3 bleed_rgb = float3(0.0);
  for (int k = 0; k < 8; k++) {
    float ang = float(k) * 0.785398 + 0.3;
    float2 d = float2(cos(ang), sin(ang));
    float4 n = emoji_at(atlas_texture, smp, uv + d * 1.2 * PER_PX * texel, rect);
    float4 f = emoji_at(atlas_texture, smp, uv + d * 2.7 * PER_PX * texel, rect);
    near += n.a;
    far += f.a;
    bleed_rgb += n.rgb * n.a + f.rgb * f.a;
  }
  bleed_rgb /= max(near + far, 1e-4);
  near *= 0.125;
  far *= 0.125;

  // Pigment pools at the rim; ink feathers out unevenly; the pen's load varies a little (an
  // emoji keeps at least 80% of itself); the paper's tooth.
  float pool = saturate((core.a - near) * 2.2) * core.a * 0.8;
  float feather = arkos_noise(px * 0.18 * small + 5.0);
  float halo = max(near, far * 0.8) * (0.2 + 0.25 * feather) * (1.0 - core.a);
  float density = 0.85 + 0.15 * arkos_noise(px * 0.035 * small * small + 9.0);
  float tooth = 0.9 + 0.1 * arkos_hash(floor(px * 0.7));

  float body = core.a * max(density * tooth, 0.8);
  float alpha = saturate(body + halo);
  float3 inked = core.rgb * (1.0 - 0.3 * pool);
  // The bleed paler and warmer than the colour it came from, as a letter's is.
  float3 bled = mix(bleed_rgb, bleed_rgb * float3(1.25, 1.1, 0.95) + 0.08, 0.6);
  float3 rgb = mix(bled, inked, saturate(body / max(alpha, 1e-4)));
  return float4(rgb, alpha * input.opacity);
}
