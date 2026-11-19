//! The built-in `runic` effect's rain, mirrored from its wash shader (`effects/runic/wash.metal`)
//! so its values and its streams' schedule can be tested without a GPU. The constants and the
//! arithmetic here are the shader's own; a change to one is made to both.

use super::hash::hash01;

/// The width of a column of rain, in points.
pub const RAIN_COL: f32 = 14.0;
/// The height of a row of rain, one rune's cell, in points.
pub const RAIN_ROW: f32 = 18.0;
/// The slowest a column's streams fall, in rows a second.
pub const RAIN_SPEED_MIN: f32 = 2.0;
/// The fastest a column's streams fall, in rows a second.
pub const RAIN_SPEED_MAX: f32 = 6.0;
/// The shortest trail a column's streams leave, in rows.
pub const RAIN_TRAIL_MIN: f32 = 8.0;
/// The longest trail a column's streams leave, in rows.
pub const RAIN_TRAIL_MAX: f32 = 24.0;
/// Seconds a column waits between its streams, beyond their fall: about three fifths of the columns
/// then carry a stream on screen at any moment in a window 800 points tall.
pub const RAIN_GAP: f32 = 6.0;
/// The rows a stream falls through in its cycle before its trail, whatever the window's height,
/// so a resize never moves a column's phase.
pub const RAIN_ROWS_PERIOD: f32 = 60.0;
/// Seconds a rune shows before its cell picks another.
pub const RUNE_CHANGE: f32 = 0.5;
/// The most the rain lays of its green over the ground, before the user's intensity.
pub const RAIN_ALPHA: f32 = 0.22;
/// The rain over a pane's body as it reaches the screen through the paper, as a share of
/// `RAIN_ALPHA`: as faint as over a source file. The tab strips are solid and carry none.
pub const BODY_RAIN: f32 = 0.11;
/// The paper the wash draws a pane's body through, the manifest's `paper_alpha`.
pub const PAPER: f32 = 0.6;
/// How far a stream's head rune leans from the green toward white.
pub const RAIN_HEAD: f32 = 0.35;

/// Signal tears a minute across a window of `D_EDGES` edges.
pub const D_PER_MINUTE: f32 = 6.0;
/// How long a tear lasts, in seconds, and the slots the tears' clock is cut into.
pub const D_DURATION: f32 = 0.12;
/// The edges the tears are shared among: a window of a pane or two reports about this many.
pub const D_EDGES: f32 = 12.0;

/// The key of a block's edge, from the block's seed and the edge's side (0 top, 1 right, 2
/// bottom, 3 left), as the shader's `nearest_edge` makes it: never from the block's place.
pub fn edge_key(seed: f32, side: u32) -> u32 {
    seed.to_bits().wrapping_mul(4).wrapping_add(side)
}

/// Whether the edge `key` tears in slot `slot` of the tears' clock (`D_DURATION` each), as the
/// shader's `edge_glow` decides it. Slot 0, where the clock stands under Reduce Motion, never does.
pub fn tears(key: u32, slot: u32) -> bool {
    let chance = D_PER_MINUTE / 60.0 * D_DURATION / D_EDGES;
    let tk = key.wrapping_mul(8) ^ slot.wrapping_mul(83_492_791);
    slot != 0 && hash01(tk) < chance
}

/// Column `column`'s stream at `t` seconds of the effect's clock: its head's row from the
/// window's top and its trail's length in rows, as the shader's `rain` works them out. The
/// window's height plays no part.
pub fn stream(column: u32, t: f32) -> (f32, f32) {
    let col = column.wrapping_mul(4);
    let speed = RAIN_SPEED_MIN + (RAIN_SPEED_MAX - RAIN_SPEED_MIN) * hash01(col);
    let trail = RAIN_TRAIL_MIN + (RAIN_TRAIL_MAX - RAIN_TRAIL_MIN) * hash01(col.wrapping_add(1));
    let period = RAIN_ROWS_PERIOD + trail + RAIN_GAP * speed;
    let x = t * speed / period + hash01(col.wrapping_add(2));
    (((x - x.floor()) * period).floor(), trail)
}

/// Whether column `column` shows any of its stream at `t` in a window `height` points tall: a
/// row the window shows lies less than a trail's length behind the head. The head is never above
/// the top, so the stream is on screen until its trail's end passes the window's last row.
pub fn on_screen(column: u32, t: f32, height: f32) -> bool {
    let (head, trail) = stream(column, t);
    let last_row = (height / RAIN_ROW).ceil() - 1.0;
    head - last_row < trail
}

/// The length of a glitch, and of the slots the glyph shader cuts its clock into, in seconds.
pub const GLITCH_SLOT: f32 = 0.1;
/// The chance a glyph glitches in a slot: about 3 a second across 3,000 glyphs.
pub const GLITCH_CHANCE: f32 = 0.0001;
/// How much brighter a glitching glyph and its halo are lit.
pub const GLITCH_FLASH: f32 = 1.4;

/// The hash input the glyph shader builds for a glyph whose bounds start at `origin_px` (device
/// pixels) in slot `slot` of its clock.
pub fn glitch_key(origin_px: (u32, u32), slot: u32) -> u32 {
    origin_px.0.wrapping_mul(73_856_093) ^ origin_px.1.wrapping_mul(19_349_663) ^ slot.wrapping_mul(83_492_791)
}

/// Whether the glyph at `origin_px` glitches in slot `slot`, and if so how it is mirrored: 1
/// horizontally, 2 vertically, 3 both. Slot 0, where the clock stands under Reduce Motion,
/// glitches nothing.
pub fn glitches(origin_px: (u32, u32), slot: u32) -> Option<u8> {
    let key = glitch_key(origin_px, slot);
    if slot == 0 || hash01(key) >= GLITCH_CHANCE {
        return None;
    }
    Some(1 + (hash01(key ^ 0x9E37_79B9) * 3.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::BUILTIN;

    fn runic() -> &'static crate::effects::BuiltIn {
        BUILTIN.iter().find(|b| b.name == "runic").expect("runic is built in")
    }

    #[test]
    fn the_wash_carries_the_rain_s_constants_and_names_this_mirror() {
        let wash = runic().wash;
        assert!(wash.contains("effects/runic.rs"), "the shader names its mirror");
        for (line, value) in [
            ("constant float RAIN_COL = 14.0;", RAIN_COL),
            ("constant float RAIN_ROW = 18.0;", RAIN_ROW),
            ("constant float RAIN_SPEED_MIN = 2.0;", RAIN_SPEED_MIN),
            ("constant float RAIN_SPEED_MAX = 6.0;", RAIN_SPEED_MAX),
            ("constant float RAIN_TRAIL_MIN = 8.0;", RAIN_TRAIL_MIN),
            ("constant float RAIN_TRAIL_MAX = 24.0;", RAIN_TRAIL_MAX),
            ("constant float RAIN_GAP = 6.0;", RAIN_GAP),
            ("constant float RAIN_ROWS_PERIOD = 60.0;", RAIN_ROWS_PERIOD),
            ("constant float RUNE_CHANGE = 0.5;", RUNE_CHANGE),
            ("constant float RAIN_ALPHA = 0.22;", RAIN_ALPHA),
            ("constant float BODY_RAIN = 0.11;", BODY_RAIN),
            ("constant float PAPER = 0.6;", PAPER),
            ("constant float RAIN_HEAD = 0.35;", RAIN_HEAD),
        ] {
            assert!(wash.contains(line), "wash.metal lacks {line}");
            assert!(line.ends_with(&format!(" = {value:?};")), "the mirror's value for {line} is {value:?}");
        }
        // The arithmetic `stream` mirrors.
        for line in [
            "uint col = uint(max(cell.x, 0.0)) * 4u;",
            "float speed = mix(RAIN_SPEED_MIN, RAIN_SPEED_MAX, hash01(col));",
            "float trail = mix(RAIN_TRAIL_MIN, RAIN_TRAIL_MAX, hash01(col + 1u));",
            "float period = RAIN_ROWS_PERIOD + trail + RAIN_GAP * speed;",
            "float head = floor(fract(t * speed / period + hash01(col + 2u)) * period);",
        ] {
            assert!(wash.contains(line), "wash.metal's rain lacks {line}");
        }
        // The theme's green is chosen from its ANSI green, its text and its focus ring.
        for line in [
            "u.colors[ARKOS_ANSI + 2].rgb",
            "u.colors[ARKOS_TERMINAL_FG].rgb",
            "u.colors[ARKOS_FOCUS_RING].rgb",
        ] {
            assert!(wash.contains(line), "wash.metal's theme_green lacks {line}");
        }
    }

    #[test]
    fn the_wash_carries_cyrus_s_tuned_edge_settings_line_for_line() {
        let wash = runic().wash;
        for line in [
            // The line.
            "constant float EDGE_INTENSITY = 0.33;",
            "constant float EDGE_CORE_WIDTH = 0.55;",
            "constant float EDGE_CORE = 0.46;",
            "constant float EDGE_HALO_REACH = 5.2;",
            "constant float EDGE_HALO = 0.5;",
            // A: rain on the vertical edges.
            "constant float A_SPEED = 222.0;",
            "constant float A_TRAIL = 110.0;",
            "constant float A_GAP = 3.0;",
            "constant float A_HEAD = 1.7;",
            "constant float A_RUNE = 7.0;",
            // B: the horizontal edges are code.
            "constant float B_RUNE = 6.0;",
            "constant float B_FLIP = 0.5;",
            "constant float B_DENSITY = 0.5;",
            "constant float B_STRENGTH = 0.8;",
            "constant float B_REPLACE = 0.6;",
            // D: signal tears.
            "constant float D_PER_MINUTE = 6.0;",
            "constant float D_DURATION = 0.12;",
            "constant float D_LENGTH = 40.0;",
            "constant float D_OFFSET = 2.0;",
            "constant float D_FLASH = 1.8;",
            // E: a busy pane runs hotter.
            "constant float E_SPEED = 2.0;",
            "constant float E_BRIGHT = 1.4;",
            // The background rain.
            "constant float RAIN_ALPHA = 0.22;",
            // The tab strips: undarkened, then painted solid by the world.
            "kind == ARKOS_TAB_BAR_BLOCK ? 0.0 : 0.42",
            // The pane bodies: drawn as they should reach the screen through the paper, the rain
            // at BODY_RAIN of RAIN_ALPHA.
            "want = mix(want, green, drop.a * RAIN_ALPHA * BODY_RAIN);",
            "col = mix(bgt, through(bgt, want), k);",
        ] {
            assert!(wash.contains(line), "wash.metal lacks {line}");
        }
        // The mirror's tear schedule is the shader's.
        for (name, value) in [("D_PER_MINUTE", D_PER_MINUTE), ("D_DURATION", D_DURATION), ("D_EDGES", D_EDGES)] {
            assert!(wash.contains(&format!("constant float {name} = {value:?};")), "the mirror's {name} is {value:?}");
        }
        for line in [
            "float chance = D_PER_MINUTE / 60.0 * D_DURATION / D_EDGES;",
            "uint tk = (h.key * 8u) ^ (dslot * 83492791u);",
            "if (dslot != 0u && hash01(tk) < chance) {",
        ] {
            assert!(wash.contains(line), "wash.metal's tears lack {line}");
        }
        // Packets (the mock-up's C) are not ported.
        assert!(!wash.contains("constant float C_"), "the packets came along");
        // On a dark ground the light is added as the mock-up adds it, clipping a channel at 1;
        // on a light one it is mixed in.
        for line in ["if (u.dark > 0.5) {", "col = min(col + e.rgb * e.a * k, 1.0);", "col = mix(col, e.rgb, saturate(e.a * k));"] {
            assert!(wash.contains(line), "wash.metal lays its edges without {line}");
        }
        // A's slot offsets and rune bits hash apart, as B's do.
        assert!(wash.contains("rune_bit(c, true, base + 2u, uint(slot), A_DENSITY)"), "A's rune bits share a hash");
        assert!(!wash.contains("rune_bit(c, true, base + 7u"), "A's rune bits share its slot offsets' hash");
    }

    #[test]
    fn the_wash_skips_edges_on_the_window_s_border_and_the_menu_stage_keeps_them() {
        let wash = runic().wash;
        assert!(wash.contains("constant float EDGE_BORDER = 1.5;"));
        assert!(wash.contains("nearest_edge(p, blk, d, hit, u.size);"), "the wash does not skip the border's edges");
        let overlay = &wash[wash.find("fragment float4 arkos_overlay(").unwrap()..];
        assert!(overlay.contains("hit, float2(0.0));"), "the menu stage skips a menu's edges on the border");
    }

    #[test]
    fn an_edge_s_key_comes_from_its_block_s_seed_and_side_never_its_index() {
        let wash = runic().wash;
        assert!(wash.contains("uint key = as_type<uint>(blk.seed) * 4u + side;"), "the edge key is not the seed's");
        let start = wash.find("void nearest_edge(").expect("the wash finds its edges");
        let end = start + wash[start..].find("\n}\n").unwrap();
        let signature = &wash[start..start + wash[start..].find(')').unwrap()];
        assert!(!signature.contains("int "), "nearest_edge is handed an index: {signature}");
        assert!(!wash[start..end].contains("float(i)"), "nearest_edge keys an edge by its index");
        // The mirror agrees, and a block keeps its edges' keys wherever it sits.
        assert_eq!(edge_key(20.0, 1), 20.0f32.to_bits().wrapping_mul(4) + 1);
        assert_ne!(edge_key(20.0, 1), edge_key(21.0, 1));
        assert_ne!(edge_key(20.0, 1), edge_key(20.0, 3));
        // The busy flag rides in the hit, from the block.
        assert!(wash.contains("blk.busy"), "the edges never read the busy flag");
    }

    #[test]
    fn about_six_tears_a_minute_across_a_window_and_none_on_a_frozen_clock() {
        // Twelve edges (D_EDGES) for ten minutes: about 60, within a factor of two.
        let keys: Vec<u32> = (0..6).flat_map(|i| [edge_key(10.0 + i as f32 * 2.0, 1), edge_key(11.0 + i as f32 * 2.0, 2)]).collect();
        assert_eq!(keys.len() as f32, D_EDGES);
        let slots = (600.0 / D_DURATION) as u32;
        let n: usize = (1..=slots).map(|slot| keys.iter().filter(|&&k| tears(k, slot)).count()).sum();
        assert!((30..=120).contains(&n), "{n} tears in ten minutes");
        // Slot 0, where the clock stands under Reduce Motion, tears nothing.
        assert!((0..2000u32).all(|k| !tears(k, 0)));
    }

    #[test]
    fn both_shaders_hash_with_the_rust_hash01_line_for_line() {
        // Each shader carries its own copy of `hash01` (crates/arkos/src/effects/hash.rs), so a
        // hand-copied drift in either shows here.
        for (file, src) in [("wash.metal", runic().wash), ("glyphs.metal", runic().glyphs.unwrap())] {
            for line in [
                "uint h = n * 747796405u + 2891336453u;",
                "h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;",
                "h ^= h >> 22u;",
                "return float(h >> 8u) * (1.0 / 16777216.0);",
            ] {
                assert!(src.contains(line), "{file}'s hash01 lacks {line}");
            }
        }
    }

    #[test]
    fn a_resize_never_moves_a_stream() {
        let wash = runic().wash;
        let start = wash.find("float4 rain(").expect("the wash has its rain");
        let end = start + wash[start..].find("\n}\n").unwrap();
        let rain = &wash[start..end];
        // The rain is given no window size, so a column's phase is the same at every height.
        assert!(rain.starts_with("float4 rain(float2 p, float t, float3 green)"), "{}", &rain[..60]);
        assert!(!rain.contains("u.size") && !rain.contains("ceil("), "the rain reads the window's height");
    }

    #[test]
    fn about_three_fifths_of_the_columns_carry_a_stream_on_screen_at_any_moment() {
        // The phase never depends on the window's height, but how much of each cycle a window
        // shows does: a taller window sees a stream for longer. The share is measured at 800
        // points, the spec's case, and stays in the band from 600 to 1000 points.
        for height in [600.0, 800.0, 1000.0] {
            let mut on = 0;
            for i in 0..50 {
                let t = i as f32 * 7.3;
                on += (0..200).filter(|&c| on_screen(c, t, height)).count();
            }
            let share = on as f32 / (200.0 * 50.0);
            assert!((0.42..=0.80).contains(&share), "at {height} points {share:.3} of the columns carry a stream");
            if height == 800.0 {
                assert!((share - 0.6125).abs() < 0.05, "at 800 points {share:.3}, not about three fifths");
            }
        }
        // A frozen clock still shows rain.
        assert!((0..200).any(|c| on_screen(c, 0.0, 800.0)));
    }

    #[test]
    fn the_menus_edges_are_drawn_with_the_panes_own_code() {
        let wash = runic().wash;
        let start = wash.find("fragment float4 arkos_overlay(").expect("runic draws the menus' edges");
        let overlay = &wash[start..];
        // The nearest edge and its glow come from the functions the panes use.
        for f in ["nearest_edge(", "edge_glow("] {
            assert!(overlay.contains(f), "the menu stage does not call {f}");
            assert!(wash[..start].matches(f).count() >= 2, "{f} is not shared with the wash");
        }
        assert!(overlay.contains("ARKOS_MENU_BLOCK") || overlay.contains("blk.value"), "the edges fade with the menu");
    }

    #[test]
    fn slot_zero_glitches_nothing_so_a_frozen_clock_shows_still_text() {
        for x in (0..3000u32).step_by(7) {
            assert_eq!(glitches((x * 11, x * 17), 0), None);
        }
    }

    #[test]
    fn about_three_glyphs_in_three_thousand_glitch_each_second() {
        // 3,000 glyph positions over 100 slots (10 seconds): expect about 30, within a factor of two.
        let mut n = 0;
        for slot in 1..=100u32 {
            for g in 0..3000u32 {
                if glitches(((g % 120) * 14, (g / 120) * 18), slot).is_some() {
                    n += 1;
                }
            }
        }
        assert!((15..=60).contains(&n), "{n} glitches in ten seconds");
    }

    #[test]
    fn a_glitch_picks_one_of_three_mirrors_and_the_same_one_every_time() {
        let found: Vec<u8> = (1..=2000u32).flat_map(|s| (0..200u32).filter_map(move |g| glitches((g * 14, 36), s))).collect();
        assert!(found.iter().all(|m| (1..=3).contains(m)));
        assert!([1u8, 2, 3].iter().all(|m| found.contains(m)), "{found:?}");
        assert_eq!(glitches((14, 36), 5), glitches((14, 36), 5));
    }

    #[test]
    fn the_glyph_shader_carries_the_glitch_s_constants_reads_the_clock_and_keeps_the_floors() {
        let g = runic().glyphs.unwrap();
        for line in [
            "constant float GLITCH_SLOT = 0.1;",
            "constant float GLITCH_CHANCE = 0.0001;",
            "constant float GLITCH_FLASH = 1.4;",
            "constant float TINT = 0.45;",
            "constant float KEEP = 0.15;",
            "constant float GLOW = 1.6;",
            "constant float BRIGHTNESS = 1.0;",
            "float3(0.44, 0.8, 0.49)",
            "float3(0.0, 0.4, 0.15)",
            "bool dark = luminance < 0.45;",
            "core * 0.55",
            "ARKOS_SCALE",
            "[[buffer(5)]]",
            "effects/runic.rs",
        ] {
            assert!(g.contains(line), "glyphs.metal lacks {line}");
        }
        // The mirror's values are the shader's.
        for (name, value) in [("GLITCH_SLOT", GLITCH_SLOT), ("GLITCH_CHANCE", GLITCH_CHANCE), ("GLITCH_FLASH", GLITCH_FLASH)] {
            assert!(g.contains(&format!("constant float {name} = {value:?};")), "the mirror's {name} is {value:?}");
        }
        // The arithmetic `glitch_key` and `glitches` mirror.
        for line in [
            "return (origin.x * 73856093u) ^ (origin.y * 19349663u) ^ (slot * 83492791u);",
            "if (slot == 0u || hash01(key) >= GLITCH_CHANCE) {",
            "uint mode = 1u + uint(hash01(key ^ 0x9E3779B9u) * 3.0);",
        ] {
            assert!(g.contains(line), "glyphs.metal's glitch lacks {line}");
        }
        // The fresh text's dark rule is the shader's own.
        let dark = crate::effects::decode::DARK_TEXT;
        assert!(g.contains(&format!("bool dark = luminance < {dark:?};")), "the decode's DARK_TEXT is {dark:?}");
        assert!(!g.contains("SCANLINES"), "no scanlines on the runic display");
        let emoji = &g[g.find("arkos_emoji_vertex(").expect("runic draws its emoji")..];
        assert!(emoji.contains("arkos_emoji_fragment(") && emoji.contains("GLITCH_") && emoji.contains("[[buffer(5)]]"));
    }

    #[test]
    fn reading_text_is_a_soft_pale_green_with_no_halo_and_no_glitch() {
        let g = runic().glyphs.unwrap();
        for line in [
            "constant float3 CALM_GREEN = float3(0.72, 0.86, 0.72);",
            "constant float CALM_TINT = 0.6;",
            "constant float CALM_BRIGHTNESS = 0.9;",
            // The band the window's ARKOS_CALM_ALPHA marks reading text with, fades included.
            "return abs(alpha - ARKOS_CALM_ALPHA) < 0.002;",
        ] {
            assert!(g.contains(line), "glyphs.metal lacks {line}");
        }
        assert!(crate::effects::active::glyph_source(g, 2.0, crate::effects::accent::HOME, false).contains("constant float ARKOS_CALM_ALPHA = 0.995;"));
        // The vertex stage gives calm text no glitch: no mirror and no flash.
        let vertex = &g[g.find("vertex RunicVertexOutput arkos_glyph_vertex(").unwrap()..g.find("float runic_tx(").unwrap()];
        assert!(vertex.contains("float3 g = calm_text(color.a) ? float3(1.0) : glitch(glitch_origin(sprite.bounds), slot);"), "{vertex}");
        // The fragment draws calm light text before any halo ring is read, with the glyph's own
        // coverage times its alpha, and with no flash.
        let fragment = &g[g.find("fragment float4 arkos_glyph_fragment(").unwrap()..g.find("struct RunicEmojiVertexOutput").unwrap()];
        assert!(fragment.contains("bool calm = calm_text(input.color.a);"), "{fragment}");
        let branch = fragment.find("if (calm && !dark) {").expect("the calm branch");
        assert!(branch < fragment.find("runic_ring(").unwrap(), "the calm branch returns before the halo");
        let calm = &fragment[branch..branch + fragment[branch..].find('}').unwrap()];
        assert!(calm.contains("mix(col, toward_accent(CALM_GREEN, ARKOS_ACCENT), CALM_TINT) * CALM_BRIGHTNESS"), "{calm}");
        assert!(calm.contains("return float4(soft, core * input.color.a);"), "{calm}");
        assert!(!calm.contains("flash") && !calm.contains("GLOW"), "no flash and no halo: {calm}");
        // Emoji carry no mark and keep their glitch.
        let emoji = &g[g.find("arkos_emoji_vertex(").unwrap()..];
        assert!(!emoji.contains("calm_text("));
    }

    // The Matrix's city keeps the traffic laws (runic world spec, The Matrix's city): the road
    // cars' arithmetic, as wash.metal's `road_car_at` works it out from the clock, mirrored here.
    const GRID_Z: f32 = 1.6;
    const CROSS_EVERY: f32 = 4.0;
    const SIG_CYCLE: f32 = 12.0;
    const ROAD_GREEN: f32 = 6.5;
    const STOP_BACK: f32 = 0.55;
    const WALK_AT: f32 = 0.18;
    const WALK_W: f32 = 0.24;

    fn hash2(x: f32, y: f32) -> f32 {
        let ux = (x as i32 + 1_048_576) as u32;
        let uy = (y as i32 + 1_048_576) as u32;
        crate::effects::hash::hash01(ux.wrapping_mul(73_856_093) ^ uy.wrapping_mul(19_349_663))
    }

    const WRAP_BLOCKS: f32 = 4.0;
    const QUEUE_GAP: f32 = 0.3;
    const QUEUE_START: f32 = 0.25;

    fn road_car_at(dir: f32, key: (f32, f32), t: f32) -> f32 {
        let block = CROSS_EVERY * GRID_Z;
        let b = key.0 % WRAP_BLOCKS;
        let place = (key.0 / WRAP_BLOCKS).floor();
        let k = (t / SIG_CYCLE).floor();
        let dur = 6.5 + 2.0 * hash2(b * 7.0 + key.1, k);
        let x = ((t - k * SIG_CYCLE - place * QUEUE_START) / dur).clamp(0.0, 1.0);
        let j = b + dir * k;
        let stop_line = (if dir > 0.0 { j * block - STOP_BACK } else { j * block + GRID_Z + STOP_BACK }) - dir * place * QUEUE_GAP;
        stop_line + dir * block * x * x * (3.0 - 2.0 * x)
    }

    #[test]
    fn no_two_cars_in_a_lane_ever_come_within_a_car_s_length_moving_or_stopped() {
        // Cyrus, 2026-11-17: the cars must "not drive through each other or completely overlap
        // each other when stopped at a light". The city repeats every WRAP_BLOCKS blocks, so two
        // cars are as close as their places are, wrapped.
        let span = WRAP_BLOCKS * CROSS_EVERY * GRID_Z;
        let car = 0.25;
        for (lane, dir) in [(0.0f32, -1.0f32), (1.0, 1.0)] {
            let keys: Vec<(f32, f32)> = (0..24).map(|j| (j as f32, lane * 13.0 + 31.0)).collect();
            let mut t = 0.0f32;
            while t < 240.0 {
                let mut at: Vec<f32> = keys.iter().map(|k| road_car_at(dir, *k, t).rem_euclid(span)).collect();
                at.sort_by(f32::total_cmp);
                let closest = at.windows(2).map(|w| w[1] - w[0]).chain([at[0] + span - at[at.len() - 1]]).fold(f32::MAX, f32::min);
                assert!(closest >= car, "lane {lane} at {t}: two cars {closest} apart");
                t += 0.05;
            }
        }
    }

    const JAYWALK: f32 = 0.85;

    /// The cross streets' cyclists ride at 1.7 to 2.2 a second; a road cyclist rides in its bike
    /// lane this far out from the road's middle.
    const ROAD_BIKE_X: f32 = 0.78 * 0.9;

    fn road_bike_at(dir: f32, key: (f32, f32), t: f32) -> f32 {
        road_bike_at_p(dir, key, t, runic_constant("BIKE_GO"))
    }

    /// A road cyclist as wash.metal's `road_bike_at` places it, reaching its stop lines at `go`.
    fn road_bike_at_p(dir: f32, key: (f32, f32), t: f32, go: f32) -> f32 {
        let block = CROSS_EVERY * GRID_Z;
        let h = |d: f32| hash2(key.0 + d, key.1 + d);
        let blocks = if h(5.0) < 0.5 { 2.0 } else { 1.0 };
        let speed = blocks * block / SIG_CYCLE;
        let phase = go + runic_constant("BIKE_SPREAD") * h(2.0);
        let j0 = (h(3.0) * 8.0).floor();
        let stop0 = if dir > 0.0 { j0 * block - STOP_BACK } else { j0 * block + GRID_Z + STOP_BACK };
        stop0 + dir * speed * (t - phase)
    }

    /// A cross street's car across the road at a moment `sig` into the cycle, as the wash places
    /// it: the cross street's row m, its lane (dl 0 going +x, 1 going -x) and its place in the queue.
    fn cross_car_x(m: f32, dl: f32, qn: f32, sig: f32) -> (f32, bool) {
        let dir = if dl == 0.0 { 1.0 } else { -1.0 };
        let key = (m * 2.0 + dl, qn + 91.0);
        let v = 2.2 + 1.0 * hash2(key.0, key.1);
        let arrive = 3.0 + 3.0 * hash2(key.0 + 1.0, key.1 + 1.0);
        let edge = 0.9;
        let stop_x = -dir * (edge + 0.42 + qn * 0.5);
        let go = ROAD_GREEN + 0.5 + qn * 0.6;
        // Before its green it is behind its stop line, waiting or rolling up to it; only then does
        // it cross the road.
        if sig < go { (stop_x - dir * v * (arrive - sig).max(0.0), false) } else { (stop_x + dir * v * (sig - go), true) }
    }

    /// Whether anything at x across, in a cross street's lane band at a moment `sig` into the
    /// cycle, would be hit by one of that cross street's cars (any row's, as a wrapped view may
    /// show any): a moving car, 0.28 long, with a cyclist or a walker taking 0.12 either side. A
    /// car standing at its stop line hits nobody.
    fn cross_car_hits(x: f32, lane: f32, sig: f32) -> bool {
        (0..40).any(|row| {
            (0..2).any(|qn| {
                let (cx, moving) = cross_car_x(row as f32 * CROSS_EVERY, lane, qn as f32, sig);
                moving && (cx - x).abs() < 0.14 + 0.12
            })
        })
    }

    #[test]
    fn the_wash_s_traffic_is_the_one_these_laws_are_checked_on() {
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        for line in [
            "constant float GRID_Z = 1.6;",
            "constant int CROSS_EVERY = 4;",
            "constant float SIG_CYCLE = 12.0;",
            "constant float ROAD_GREEN = 6.5;",
            "constant float STOP_BACK = 0.55;",
            "constant float WALK_AT = 0.18;",
            "constant float WALK_W = 0.24;",
            "    float b = fmod(key.x, WRAP_BLOCKS);",
            "    float place = floor(key.x / WRAP_BLOCKS);",
            "    float dur = mix(6.5, 8.5, hash2(float2(b * 7.0 + key.y, k)));",
            "    float x = saturate((t - k * SIG_CYCLE - place * QUEUE_START) / dur);",
            "    float j = b + dir * k;",
            "    float stop_line = (dir > 0.0 ? j * block - STOP_BACK : j * block + GRID_Z + STOP_BACK) - dir * place * QUEUE_GAP;",
            "    return stop_line + dir * block * x * x * (3.0 - 2.0 * x);",
            "constant float WRAP_BLOCKS = 4.0;",
            "constant float QUEUE_GAP = 0.3;",
            "constant float QUEUE_START = 0.25;",
            "constant int ROAD_CARS = 24;",
            "            float z = wrap_view(road_car_at(dir, float2(float(j), float(lane) * 13.0 + 31.0), t) - run);",
            "    float blocks = hash2(key + 5.0) < 0.5 ? 2.0 : 1.0;",
            "    float phase = BIKE_GO + BIKE_SPREAD * hash2(key + 2.0);",
            "    return stop0 + dir * speed * (t - phase);",
            "constant float JAYWALK = 0.85;",
            "                float go = CROSS_BIKE_GO;",
            "                float v = mix(1.7, 2.2, hash2(key));",
            "                float2 key = float2(float(m) * 2.0 + float(dl), 131.0);",
            "                    float v = mix(2.2, 3.2, hash2(key));",
            "                    float arrive = mix(3.0, 6.0, hash2(key + 1.0));",
            "                    float stop_x = -dir * (edge + CROSS_STOP + float(qn) * 0.5);",
            "                    float go = cross_on + float(qn) * 0.6;",
            "                    float2 key = float2(float(m) * 2.0 + float(dl), float(qn) + 91.0);",
            "                float zl = zc + (dl == 0 ? 0.3 : 0.7) * GRID_Z;",
            "            float2 key = float2(float(j), float(lane) * 13.0 + 141.0);",
            "constant int ROAD_BIKES = 3;",
        ] {
            assert!(wash.contains(line), "wash.metal lacks {line}");
        }
    }

    #[test]
    fn a_road_car_is_in_an_intersection_only_on_the_road_s_green_and_stands_still_on_its_red() {
        let block = CROSS_EVERY * GRID_Z;
        // An intersection, with its crosswalks either side: from the near crosswalk's outer edge
        // to the far one's.
        let reach = WALK_AT + WALK_W * 0.5;
        let inside = |w: f32| {
            let local = w - (w / block).floor() * block;
            local < GRID_Z + reach || local > block - reach
        };
        // The road's cars, each as the wash places them (its cyclists break the law: below).
        let riders = [(0.0f32, -1.0f32), (1.0, 1.0)].into_iter().flat_map(|(lane, dir)| (0..24).map(move |j| (lane, dir, (j as f32, lane * 13.0 + 31.0), false)));
        for (lane, dir, key, bike) in riders {
            {
                let j = key.0;
                let kind = if bike { "cyclist" } else { "car" };
                let at = |t: f32| if bike { road_bike_at(dir, key, t) } else { road_car_at(dir, key, t) };
                let mut t = 0.0f32;
                while t < 600.0 {
                    let (w, next) = (at(t), at(t + 0.05));
                    let sig = t - (t / SIG_CYCLE).floor() * SIG_CYCLE;
                    let moving = (next - w).abs() > 1e-4;
                    if inside(w) && moving {
                        assert!(sig < ROAD_GREEN, "{kind} {j} of lane {lane} in an intersection at {t} ({sig} into the cycle, not the road's green)");
                    }
                    if sig >= ROAD_GREEN && sig + 0.05 < SIG_CYCLE {
                        assert!(!moving || !inside(next), "{kind} {j} of lane {lane} moves into an intersection on its red at {t}");
                    }
                    // A car waiting, or come to rest, is behind its stop line: never in an
                    // intersection.
                    if !moving {
                        assert!(!inside(w), "{kind} {j} of lane {lane} stands in an intersection at {t}");
                    }
                    t += 0.05;
                }
            }
        }
    }

    const AMBER: f32 = 1.0;

    /// A walker on the road's sidewalk, as wash.metal's `road_walker_at` places it: where it is
    /// along the road, and whether it is walking.
    fn road_walker_at(dir: f32, key: (f32, f32), t: f32) -> (f32, bool) {
        road_walker_at_p(dir, key, t, runic_constant("WALK_GO"))
    }

    /// A walker on the road's sidewalk, as wash.metal's `road_walker_at` places it, with the
    /// jaywalkers stepping off at `go`: where it is along the road, and whether it is moving
    /// (walking, or edging up to the curb).
    fn road_walker_at_p(dir: f32, key: (f32, f32), t: f32, go: f32) -> (f32, bool) {
        let block = CROSS_EVERY * GRID_Z;
        let cross = GRID_Z + 2.0 * WALK_AT;
        let h = |d: f32| hash2(key.0 + d, key.1 + d);
        let brisk = h(2.0) < 0.65;
        let v = if brisk { 0.54 + 0.06 * h(6.0) } else { 0.31 + 0.05 * h(6.0) };
        let laps = if brisk { 1.0 } else { 2.0 };
        let period = laps * SIG_CYCLE;
        let late = (ROAD_GREEN - AMBER).min(ROAD_GREEN - cross / v);
        let jay = h(8.0) < JAYWALK;
        let back = if jay { runic_constant("WAIT_DEPTH") * h(9.0) } else { 0.0 };
        let front = if jay { back * runic_constant("CREEP_KEEP") - runic_constant("GUTTER") * h(10.0) } else { 0.0 };
        let c = (h(3.0) * laps).floor() * SIG_CYCLE + if jay { go + runic_constant("JAY_SPREAD") * h(4.0) } else { late * h(4.0) };
        let k = ((t - c) / period).floor();
        let s = t - c - k * period;
        let j = (h(5.0) * 8.0).floor() + dir * k;
        let corner = if dir > 0.0 { j * block - WALK_AT } else { j * block + GRID_Z + WALK_AT };
        let dist = block - back + front;
        let walk = dist / v;
        let creep = walk.max(period - runic_constant("CREEP_T"));
        let (x, moving) = if s < walk {
            (v * s, true)
        } else if s < creep || back <= front {
            (dist, false)
        } else {
            (dist + (back - front) * (s - creep) / (period - creep), true)
        };
        (corner - dir * front + dir * x, moving)
    }

    /// A stroller on the road's sidewalk, as wash.metal's `road_stroller_at` places it: where it
    /// is along the road, and how much of it shows (it steps out of one door and into another).
    fn road_stroller_at(dir: f32, key: (f32, f32), t: f32) -> (f32, f32) {
        let block = CROSS_EVERY * GRID_Z;
        let span = block - GRID_Z - 2.0 * WALK_AT;
        let h = |d: f32| hash2(key.0 + d, key.1 + d);
        let v = 0.36 + 0.22 * h(2.0);
        let period = span / v + 1.0;
        let s = t + h(3.0) * period;
        let k = (s / period).floor();
        let lap = |d: f32| hash2(key.0 + k * 7.0 + d, key.1 + d);
        let len = span * (0.4 + 0.6 * lap(5.0));
        let a = lap(4.0) * (span - len);
        let dur = len / v;
        let into = s - k * period;
        let x = into.min(dur) * v;
        let smooth = |e0: f32, e1: f32, x: f32| {
            let u = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
            u * u * (3.0 - 2.0 * u)
        };
        let shown = smooth(0.0, 0.4, into) * (1.0 - smooth(dur - 0.4, dur, into));
        let j = (lap(6.0) * 8.0).floor();
        let base = j * block + GRID_Z + WALK_AT;
        (base + if dir > 0.0 { a + x } else { a + len - x }, shown)
    }

    /// The road's sidewalk people, one side: the walkers and the strollers.
    fn sidewalk(side: f32) -> (Vec<(f32, (f32, f32))>, Vec<(f32, (f32, f32))>) {
        let walkers = (0..PEOPLE).map(|j| {
            let key = (j as f32, side * 17.0 + 61.0);
            (if hash2(key.0 + 1.0, key.1 + 1.0) < 0.5 { 1.0 } else { -1.0 }, key)
        });
        let strollers = (0..STROLLERS).map(|j| {
            let key = (j as f32, side * 17.0 + 211.0);
            (if hash2(key.0 + 1.0, key.1 + 1.0) < 0.5 { 1.0 } else { -1.0 }, key)
        });
        (walkers.collect(), strollers.collect())
    }

    const PEOPLE: usize = 40;
    const STROLLERS: usize = 48;

    #[test]
    fn cyclists_run_the_reds_and_walkers_cross_against_the_light_but_no_car_ever_hits_one() {
        // Cyrus, 2026-11-17: "bicyclists never obey traffic laws ... as long as another car does
        // not hit them", "Same thing with pedestrians".
        let block = CROSS_EVERY * GRID_Z;
        // In a cross street's lane: its near lane at 0.3 of its width, its far one at 0.7; a car
        // there reaches 0.2 to either side, and the rider or walker 0.12 more.
        let lane_of = |w: f32| -> Option<f32> {
            let local = w - (w / block).floor() * block;
            [(0.0f32, 0.3f32), (1.0, 0.7)].into_iter().find(|(_, at)| (local - at * GRID_Z).abs() < 0.32).map(|(dl, _)| dl)
        };
        let (mut red_runs, mut jaywalks) = (0, 0);
        let cross_go = runic_constant("CROSS_BIKE_GO");
        let mut t = 0.0f32;
        while t < 360.0 {
            let sig = t - (t / SIG_CYCLE).floor() * SIG_CYCLE;
            for (lane, dir) in [(0.0f32, -1.0f32), (1.0, 1.0)] {
                for j in 0..3 {
                    let key = (j as f32, lane * 13.0 + 141.0);
                    let w = road_bike_at(dir, key, t);
                    let x = dir * 0.78 * 0.9;
                    if let Some(dl) = lane_of(w) {
                        assert!(!cross_car_hits(x, dl, sig) && !cross_bike_hits(x, dl, sig, cross_go), "cyclist {j} of lane {lane} hit at {t} ({sig} into the cycle)");
                        if sig >= ROAD_GREEN {
                            red_runs += 1;
                        }
                    }
                }
            }
            for side in [0.0f32, 1.0] {
                let (walkers, _) = sidewalk(side);
                for (dir, key) in walkers {
                    let (w, walking) = road_walker_at(dir, key, t);
                    if !walking {
                        continue;
                    }
                    let out = if side == 0.0 { -1.0 } else { 1.0 };
                    let x = out * (0.96 + (hash2(key.0 + 7.0, key.1 + 7.0) - 0.22) * runic_constant("WALKER_SPREAD"));
                    if let Some(dl) = lane_of(w) {
                        assert!(!cross_car_hits(x, dl, sig) && !cross_bike_hits(x, dl, sig, cross_go), "walker {key:?} hit at {t} ({sig} into the cycle)");
                        if sig >= ROAD_GREEN {
                            jaywalks += 1;
                        }
                    }
                }
            }
            t += 0.05;
        }
        assert!(red_runs > 100, "they do break the law: {red_runs}");
        // Cyrus, 2026-11-18: "NYC people are very aggressive jaywalkers". A walker's moments in a
        // cross street against the light were 22,894 before (three in five stepping off in the
        // first 0.8 s after the last car).
        assert!(jaywalks > 30_000, "walkers out against the light only {jaywalks} times");
    }

    /// A cross street's cyclist across the road at a moment `sig` into the cycle, as the wash
    /// places it, and whether it is moving over the road (before then it rolls up to its stop
    /// line from outside the road).
    fn cross_bike_x(m: f32, dl: f32, sig: f32, go: f32) -> (f32, bool) {
        let dir = if dl == 0.0 { 1.0 } else { -1.0 };
        let key = (m * 2.0 + dl, 131.0);
        let v = 1.7 + 0.5 * hash2(key.0, key.1);
        let stop_x = -dir * (0.9 + runic_constant("CROSS_STOP"));
        if sig < go { (stop_x, false) } else { (stop_x + dir * v * (sig - go), true) }
    }

    /// Whether anything at x across, in a cross street's lane band at `sig`, would be hit by one of
    /// that cross street's cyclists, set off at `go`: 0.14 either side of it, and 0.12 more.
    fn cross_bike_hits(x: f32, lane: f32, sig: f32, go: f32) -> bool {
        (0..40).any(|row| {
            let (cx, moving) = cross_bike_x(row as f32 * CROSS_EVERY, lane, sig, go);
            moving && (cx - x).abs() < 0.14 + 0.12
        })
    }

    /// Sig steps the searches below look at: a hundredth of a second, so no car (3.2 a second at
    /// most) moves past a person between two.
    const STEP: f32 = 0.01;
    const STEPS: usize = (SIG_CYCLE / STEP) as usize;

    /// The cross streets' cars and cyclists moving across, by sig step and lane: where they are.
    fn cross_traffic() -> Vec<[Vec<f32>; 2]> {
        let go = runic_constant("CROSS_BIKE_GO");
        (0..STEPS).map(|i| {
            let sig = i as f32 * STEP;
            [0usize, 1].map(|dl| {
                let mut at = Vec::new();
                for row in 0..40 {
                    let m = row as f32 * CROSS_EVERY;
                    for qn in 0..2 {
                        let (cx, moving) = cross_car_x(m, dl as f32, qn as f32, sig);
                        if moving {
                            at.push(cx);
                        }
                    }
                    let (bx, moving) = cross_bike_x(m, dl as f32, sig, go);
                    if moving {
                        at.push(bx);
                    }
                }
                at
            })
        }).collect()
    }

    /// The cross street lane a place along the road is in, if any: its near lane at 0.3 of its
    /// width, its far one at 0.7, a car there reaching 0.2 to either side and a person 0.12 more.
    fn lane_of(w: f32) -> Option<usize> {
        let block = CROSS_EVERY * GRID_Z;
        let local = w - (w / block).floor() * block;
        [(0usize, 0.3f32), (1, 0.7)].into_iter().find(|(_, at)| (local - at * GRID_Z).abs() < 0.32).map(|(dl, _)| dl)
    }

    fn hits(traffic: &[[Vec<f32>; 2]], n: usize, dl: usize, x: f32) -> bool {
        traffic[n % STEPS][dl].iter().any(|&c| (c - x).abs() < 0.14 + 0.12)
    }

    /// Whether every walker, its jaywalkers stepping off at `go`, keeps out of the cross streets'
    /// moving cars and cyclists, lane by lane. Two cycles say it all (the slow walkers take two).
    fn walkers_are_safe(go: f32, traffic: &[[Vec<f32>; 2]]) -> bool {
        let spread = runic_constant("WALKER_SPREAD");
        (0..2 * STEPS).all(|n| {
            let t = n as f32 * STEP;
            [0.0f32, 1.0].into_iter().all(|side| {
                let out = if side == 0.0 { -1.0 } else { 1.0 };
                sidewalk(side).0.into_iter().all(|(dir, key)| {
                    let (w, _) = road_walker_at_p(dir, key, t, go);
                    let x = out * (0.96 + (hash2(key.0 + 7.0, key.1 + 7.0) - 0.22) * spread);
                    lane_of(w).is_none_or(|dl| !hits(traffic, n, dl, x))
                })
            })
        })
    }

    /// Whether every road cyclist, at its stop line at `go`, keeps out of them.
    fn road_bikes_are_safe(go: f32, traffic: &[[Vec<f32>; 2]]) -> bool {
        (0..2 * STEPS).all(|n| {
            let t = n as f32 * STEP;
            [(0.0f32, -1.0f32), (1.0, 1.0)].into_iter().all(|(lane, dir)| {
                (0..3).all(|j| {
                    let w = road_bike_at_p(dir, (j as f32, lane * 13.0 + 141.0), t, go);
                    lane_of(w).is_none_or(|dl| !hits(traffic, n, dl, dir * ROAD_BIKE_X))
                })
            })
        })
    }

    /// The earliest of `from`, `from + STEP`, ... up to `to` at which `safe` holds.
    fn earliest(from: f32, to: f32, safe: impl Fn(f32) -> bool) -> Option<f32> {
        (0..=((to - from) / STEP).round() as usize).map(|i| from + i as f32 * STEP).find(|&g| safe(g))
    }

    /// The last moment in the cycle a moving cross car or cyclist is within reach of one of `xs`.
    fn passed(xs: &[f32], traffic: &[[Vec<f32>; 2]]) -> f32 {
        (0..STEPS).filter(|&n| xs.iter().any(|&x| (0..2).any(|dl| hits(traffic, n, dl, x)))).map(|n| n as f32 * STEP).fold(0.0, f32::max)
    }

    fn walker_xs() -> Vec<f32> {
        let spread = runic_constant("WALKER_SPREAD");
        (0..=20).flat_map(|i| {
            let x = 0.96 + (i as f32 / 20.0 - 0.22) * spread;
            [x, -x]
        }).collect()
    }

    #[test]
    fn jaywalkers_step_off_while_the_last_car_is_still_passing_and_are_never_in_its_lane() {
        // Cyrus, 2026-11-18: "walkers and cyclists should start walking before the cars have
        // passed. NYC people are crazy. They will time their walking so they're already crossing
        // the street as the car is passing."
        let traffic = cross_traffic();
        let go = runic_constant("WALK_GO");
        assert!(walkers_are_safe(go, &traffic), "a walker stepping off at {go} is hit");
        let first = earliest(6.5, go, |g| walkers_are_safe(g, &traffic)).unwrap();
        assert!(go - first < 0.15, "they could go at {first}, not {go}");
        let passed = passed(&walker_xs(), &traffic);
        assert!(go < passed - 0.2, "stepping off at {go}, after the traffic passed their crosswalk at {passed}");
    }

    #[test]
    fn road_cyclists_ride_into_a_cross_street_while_its_last_car_is_still_passing() {
        // Cyrus, 2026-11-18: "same thing with the bicyclists", then "before the cars have passed".
        let traffic = cross_traffic();
        let go = runic_constant("BIKE_GO");
        assert!(road_bikes_are_safe(go, &traffic), "a road cyclist at its stop line at {go} is hit");
        let first = earliest(6.5, go, |g| road_bikes_are_safe(g, &traffic)).unwrap();
        assert!(go - first < 0.15, "they could go at {first}, not {go}");
        let passed = passed(&[ROAD_BIKE_X, -ROAD_BIKE_X], &traffic);
        assert!(go < passed - 0.2, "at the stop line at {go}, after the traffic passed at {passed}");
        assert!(runic_constant("BIKE_SPREAD") <= 0.3);
    }

    /// Whether a cross street's cyclist set off at `go` meets nothing on its way over the road,
    /// lane by lane: no road car moving through an intersection in the lane it is crossing, no
    /// road cyclist in the bike lane it is crossing, no walker on the crosswalk it is crossing.
    /// The road's cars change their pace every cycle, so thirty cycles of them.
    fn a_cross_bike_set_off_then_meets_nothing(go: f32) -> bool {
        let block = CROSS_EVERY * GRID_Z;
        let reach = WALK_AT + WALK_W * 0.5;
        let inside = |w: f32| {
            let local = w - (w / block).floor() * block;
            local < GRID_Z + reach || local > block - reach
        };
        let in_street = |w: f32| lane_of(w).is_some();
        let car_x = 0.5 * 0.9;
        // Where the cross streets' cyclists are across, any of them, at each sig step.
        let bikes: Vec<Vec<f32>> = (0..STEPS).map(|n| {
            (0..40).flat_map(|row| (0..2).map(move |dl| (row, dl))).filter_map(|(row, dl)| {
                let (cx, moving) = cross_bike_x(row as f32 * CROSS_EVERY, dl as f32, n as f32 * STEP, go);
                (moving && cx.abs() < 1.6).then_some(cx)
            }).collect()
        }).collect();
        let near = |n: usize, x: f32, r: f32| bikes[n % STEPS].iter().any(|&b| (b - x).abs() < r);
        let walk_go = runic_constant("WALK_GO");
        let spread = runic_constant("WALKER_SPREAD");
        for n in 0..30 * STEPS {
            if bikes[n % STEPS].is_empty() {
                continue;
            }
            let t = n as f32 * STEP;
            for (lane, dir) in [(0.0f32, -1.0f32), (1.0, 1.0)] {
                if near(n, dir * car_x, 0.2 + 0.12) {
                    for j in 0..24 {
                        let key = (j as f32, lane * 13.0 + 31.0);
                        let (w, next) = (road_car_at(dir, key, t), road_car_at(dir, key, t + STEP));
                        if inside(w) && (next - w).abs() > 1e-4 {
                            return false;
                        }
                    }
                }
                if n < 2 * STEPS && near(n, dir * ROAD_BIKE_X, 0.26) && (0..3).any(|j| in_street(road_bike_at(dir, (j as f32, lane * 13.0 + 141.0), t))) {
                    return false;
                }
            }
            if n < 2 * STEPS {
                for side in [0.0f32, 1.0] {
                    let out = if side == 0.0 { -1.0 } else { 1.0 };
                    for (dir, key) in sidewalk(side).0 {
                        let (w, _) = road_walker_at_p(dir, key, t, walk_go);
                        let x = out * (0.96 + (hash2(key.0 + 7.0, key.1 + 7.0) - 0.22) * spread);
                        if in_street(w) && near(n, x, 0.26) {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    #[test]
    fn a_cross_street_cyclist_sets_off_as_soon_as_nothing_is_in_its_way_over_the_road() {
        // Cyrus, 2026-11-18: "same thing with the bicyclists", and "as long as they don't get hit
        // by a car or another bicycle".
        let go = runic_constant("CROSS_BIKE_GO");
        assert!(a_cross_bike_set_off_then_meets_nothing(go), "a cross cyclist set off at {go} meets something");
        let first = earliest(3.0, go, a_cross_bike_set_off_then_meets_nothing).unwrap();
        assert!(go - first < 0.15, "it could go at {first}, not {go}");
    }

    #[test]
    fn a_waiting_crowd_edges_up_to_the_curb_as_the_last_cars_pass() {
        // Cyrus, 2026-11-18: "The crowds also get closer to the car as the last few are passing
        // because they are impatient when waiting to cross the street." Each jaywalker's distance
        // back from its curb, standing early in the wait and in the moment it steps off.
        let block = CROSS_EVERY * GRID_Z;
        let go = runic_constant("WALK_GO");
        let (mut early, mut late, mut past_curb, mut n) = (0.0f32, 0.0f32, 0usize, 0usize);
        for side in [0.0f32, 1.0] {
            for (dir, key) in sidewalk(side).0 {
                if hash2(key.0 + 8.0, key.1 + 8.0) >= JAYWALK || hash2(key.0 + 2.0, key.1 + 2.0) >= 0.65 {
                    continue;
                }
                // A brisk one steps off at go plus its spread; its curb is the next street's edge.
                let c = go + runic_constant("JAY_SPREAD") * hash2(key.0 + 4.0, key.1 + 4.0);
                let curb = |w: f32| {
                    let local = w - (w / block).floor() * block;
                    if dir > 0.0 { block - local } else { local - GRID_Z }
                };
                let back = |t: f32| curb(road_walker_at(dir, key, t).0) - WALK_AT;
                early += back(c + SIG_CYCLE - runic_constant("CREEP_T") - 0.05);
                late += back(c + SIG_CYCLE - 0.01);
                past_curb += usize::from(back(c + SIG_CYCLE - 0.01) < 0.0);
                n += 1;
            }
        }
        let (early, late) = (early / n as f32, late / n as f32);
        assert!(late < early * 0.6, "they edge up from {early} back to only {late}");
        assert!(past_curb * 4 >= n, "a quarter or more off the curb: {past_curb} of {n}");
    }

    /// How the road's crossers wait and go (wash.metal's `road_crosser_at`).
    #[derive(Clone, Copy)]
    struct Crossing {
        spread: f32,
        depth: f32,
        across: f32,
        late_share: f32,
        late_go: f32,
        late_spread: f32,
        along: f32,
        first: f32,
        yield_to_bikes: bool,
    }

    impl Crossing {
        fn from_wash() -> Self {
            let c = runic_constant;
            Crossing { spread: c("CROSS_SPREAD"), depth: c("CROSS_WAIT_DEPTH"), across: c("CROSS_ACROSS"), late_share: c("LATE_SHARE"), late_go: c("LATE_GO"), late_spread: c("LATE_SPREAD"), along: c("WAIT_ALONG"), first: c("CROSS_FIRST"), yield_to_bikes: true }
        }

        /// As the wash had it before: everyone off within a tenth of a second of the green, from
        /// one spot at the corner, over six tenths of the crosswalk's width.
        fn before() -> Self {
            Crossing { spread: 0.1, depth: 0.0, across: 0.6, late_share: 0.0, late_go: 0.0, late_spread: 0.0, along: 0.0, first: ROAD_GREEN + 0.5, yield_to_bikes: false }
        }
    }

    /// wash.metal's `bike_wait`: how long a crosser at z along the road stands at the edge of
    /// the bike lane of the way s it is about to step into at ta, to be in it dur seconds with no
    /// road cyclist through its crosswalk.
    fn bike_wait(s: f32, z: f32, ta: f32, dur: f32) -> f32 {
        let block = CROSS_EVERY * GRID_Z;
        let span = WRAP_BLOCKS * block;
        let lane = if s > 0.0 { 1.0 } else { 0.0 };
        let (body, gap) = (runic_constant("PERSON_BODY"), runic_constant("NEAR_MISS"));
        let mut wait = 0.0f32;
        for _ in 0..2 {
            for j in 0..3 {
                let key = (j as f32, lane * 13.0 + 141.0);
                let h = |d: f32| hash2(key.0 + d, key.1 + d);
                let blocks = if h(5.0) < 0.5 { 2.0 } else { 1.0 };
                let speed = blocks * block / SIG_CYCLE;
                let phase = runic_constant("BIKE_GO") + runic_constant("BIKE_SPREAD") * h(2.0);
                let j0 = (h(3.0) * 8.0).floor();
                let stop0 = if s > 0.0 { j0 * block - STOP_BACK } else { j0 * block + GRID_Z + STOP_BACK };
                let base = phase + (z - stop0) / (s * speed);
                let per = span / speed;
                let half = (0.09 + body + gap) / speed;
                let from = ta + wait;
                let tk = base + ((from - half - base) / per).ceil() * per;
                if tk - half < from + dur {
                    wait = tk + half - ta;
                }
            }
        }
        wait
    }

    /// A road crosser as wash.metal's `road_crosser_at` places it, on the crosswalk at `cw`
    /// along the road: across, across the crosswalk's width, walking, and how much of it shows.
    fn road_crosser_at(key: (f32, f32), side: f32, cw: f32, t: f32, p: Crossing) -> (f32, f32, bool, f32) {
        let h = |d: f32| hash2(key.0 + d, key.1 + d);
        let edge = 0.9;
        let dir = if h(1.0) < 0.5 { 1.0 } else { -1.0 };
        let late = h(11.0) < p.late_share;
        let v = if late { 0.7 + 0.15 * h(2.0) } else { 0.46 + 0.14 * h(2.0) };
        let tt = t + (h(3.0) * 2.0).floor() * SIG_CYCLE;
        let tau = tt - (tt / (2.0 * SIG_CYCLE)).floor() * 2.0 * SIG_CYCLE;
        let back = p.depth * h(9.0);
        let over = (2.0 * (edge + 0.08) + back) / v;
        let tc = if late { p.late_go - p.late_spread * h(7.0) } else { p.first + (p.spread * h(7.0)).min(SIG_CYCLE - runic_constant("CROSS_CLEAR") - over - p.first) };
        let wait = if late { 0.3 * h(4.0) } else { 2.0 * h(4.0) };
        let start = -dir * (edge + 0.08 + back);
        let la = (0.5 + 2.5 * h(5.0)).min(v * (tc - wait));
        let t0 = tc - wait - la / v;
        let lb = 0.5 + 2.5 * h(6.0);
        let reach = 2.0 * (edge + 0.08) + back + lb;
        let across = (h(8.0) - 0.5) * WALK_W * p.across;
        let waiting = across + side * p.along * h(12.0);
        let smooth = |e0: f32, e1: f32, x: f32| {
            let u = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
            u * u * (3.0 - 2.0 * u)
        };
        if tau < t0 {
            return (0.0, waiting, false, 0.0);
        }
        if tau < tc {
            return (start - dir * la + dir * v * (tau - t0).min(la / v), waiting, tau - t0 < la / v, smooth(t0, t0 + 0.4, tau));
        }
        let hw = 0.03 + runic_constant("PERSON_BODY") + runic_constant("NEAR_MISS");
        let (d1, d2) = (edge + 0.08 + back - ROAD_BIKE_X - hw, edge + 0.08 + back + ROAD_BIKE_X - hw);
        let step_off = t - (tau - tc);
        let (w1, w2) = if p.yield_to_bikes {
            let w1 = bike_wait(-dir, cw + across, step_off + d1 / v, 2.0 * hw / v);
            (w1, bike_wait(dir, cw + across, step_off + d2 / v + w1, 2.0 * hw / v))
        } else {
            (0.0, 0.0)
        };
        let e = tau - tc;
        let (d, walking) = if e < d1 / v {
            (v * e, true)
        } else if e < d1 / v + w1 {
            (d1, false)
        } else if e < d2 / v + w1 {
            (v * (e - w1), true)
        } else if e < d2 / v + w1 + w2 {
            (d2, false)
        } else {
            (v * (e - w1 - w2), true)
        };
        if d > reach {
            return (0.0, across, false, 0.0);
        }
        let onto = waiting + (across - waiting) * smooth(0.0, runic_constant("CROSS_MERGE"), d);
        (start + dir * d, onto, walking, 1.0 - smooth(reach - 0.2, reach, d))
    }

    /// A crosser's reach past its middle every way: about half a body's width.
    fn body() -> f32 {
        runic_constant("PERSON_BODY")
    }

    fn near_a_crosswalk(local: f32) -> bool {
        let block = CROSS_EVERY * GRID_Z;
        local > block - 0.7 || (local > GRID_Z - 0.1 && local < GRID_Z + 0.7)
    }

    /// The road's moving cars near its crosswalks (moving: faster than a tenth of a camera height a
    /// second; one rolling the last of the way up to its stop line, its nose over the crosswalk's
    /// edge, has stopped for the people), folded onto the crossers' two cycles at a hundredth of a
    /// second over thirty cycles (the cars change their pace every cycle): each as (x across, half
    /// its width, where along its block, half its length).
    fn road_cars_at_crosswalks() -> Vec<Vec<(f32, f32, f32, f32)>> {
        let block = CROSS_EVERY * GRID_Z;
        let fold = 2 * STEPS;
        let mut at = vec![Vec::new(); fold];
        for n in 0..30 * STEPS {
            let t = n as f32 * STEP;
            for (lane, dir) in [(0.0f32, -1.0f32), (1.0, 1.0)] {
                for j in 0..24 {
                    let key = (j as f32, lane * 13.0 + 31.0);
                    let (w, next) = (road_car_at(dir, key, t), road_car_at(dir, key, t + STEP));
                    let local = w.rem_euclid(block);
                    if (next - w).abs() > 0.1 * STEP && near_a_crosswalk(local) {
                        at[n % fold].push((dir * 0.45, 0.15, local, 0.14));
                    }
                }
            }
        }
        at
    }

    /// Whether no road crosser, going as `p` has it, is ever in a moving road car's way (cars
    /// folded, any intersection) or a road cyclist's (each where it is, its images a view's span
    /// apart).
    fn crossers_are_safe(p: Crossing, cars: &[Vec<(f32, f32, f32, f32)>], with_bikes: bool) -> bool {
        let block = CROSS_EVERY * GRID_Z;
        let span = WRAP_BLOCKS * block;
        let body = body();
        (0..16).all(|street| {
            let m = street as f32 * CROSS_EVERY;
            (0..2).all(|w| {
                let side = if w == 0 { -1.0 } else { 1.0 };
                let cw = m * GRID_Z + if w == 0 { -WALK_AT } else { GRID_Z + WALK_AT };
                (0..12).all(|j| {
                    let key = (m * 3.0 + w as f32, j as f32 + 71.0);
                    (0..2 * STEPS).all(|n| {
                        let t = n as f32 * STEP;
                        let (x, across, _, shown) = road_crosser_at(key, side, cw, t, p);
                        if shown <= 0.0 {
                            return true;
                        }
                        let local = (cw + across).rem_euclid(block);
                        let car = cars[n].iter().any(|&(cx, hw, cz, hl)| (x - cx).abs() < hw + body && (local - cz).abs() < hl + body);
                        let bike = with_bikes && [(0.0f32, -1.0f32), (1.0, 1.0)].into_iter().any(|(lane, dir)| {
                            (x - dir * ROAD_BIKE_X).abs() < 0.03 + body && (0..3).any(|jb| {
                                let off = (road_bike_at(dir, (jb as f32, lane * 13.0 + 141.0), t) - (cw + across)).rem_euclid(span);
                                off.min(span - off) < 0.09 + body
                            })
                        });
                        !car && !bike
                    })
                })
            })
        })
    }

    #[test]
    fn road_crossers_are_never_in_a_road_car_s_or_cyclist_s_way() {
        // The road's cyclists never stop, a messenger at an intersection every six seconds, and
        // the crosswalks have people on them five seconds in twelve: no timing kept them apart
        // (every start from 0 to 11.5 s left 12 to 223 of 384 crossers in a cyclist's way). Cyrus,
        // 2026-11-18, asked who gives way: "Both, a near miss". A crosser stands at the edge of a
        // bike lane while a cyclist goes through, NEAR_MISS clear of it.
        assert!(crossers_are_safe(Crossing::from_wash(), &road_cars_at_crosswalks(), true));
    }

    #[test]
    fn late_crossers_hurry_over_in_front_of_the_traffic_as_late_as_is_safe() {
        // Cyrus, 2026-11-18: "pedestrians should start cross in front of oncoming traffic if they
        // know the light is about to turn red for them".
        let cars = road_cars_at_crosswalks();
        let p = Crossing::from_wash();
        let latest = (0..=120).map(|i| 14.0 - i as f32 * 0.05).find(|&g| crossers_are_safe(Crossing { late_go: g, ..p }, &cars, true));
        let latest = latest.expect("no time is safe");
        eprintln!("late crossers could go up to {latest}");
        assert!(p.late_go <= latest && latest - p.late_go < 0.15, "late crossers could go up to {latest}, not {}", p.late_go);
        assert!(p.late_share >= 0.25);
    }

    /// The share of the road's crossers out (waiting or crossing) standing on another: within a
    /// person's width across and along.
    fn crossers_stacked(p: Crossing) -> f32 {
        let (mut out, mut stacked) = (0usize, 0usize);
        for street in 0..10 {
            for w in 0..2 {
                let side = if w == 0 { -1.0 } else { 1.0 };
                let mut t = 0.0f32;
                while t < 2.0 * SIG_CYCLE {
                    let at: Vec<(f32, f32)> = (0..12).filter_map(|j| {
                        let m = street as f32 * CROSS_EVERY;
                        let cw = m * GRID_Z + if w == 0 { -WALK_AT } else { GRID_Z + WALK_AT };
                        let (x, across, _, shown) = road_crosser_at((m * 3.0 + w as f32, j as f32 + 71.0), side, cw, t, p);
                        (shown > 0.5).then_some((x, across))
                    }).collect();
                    for (i, a) in at.iter().enumerate() {
                        out += 1;
                        stacked += usize::from(at.iter().enumerate().any(|(k, b)| k != i && (a.0 - b.0).abs() < 0.06 && (a.1 - b.1).abs() < 0.06));
                    }
                    t += 0.25;
                }
            }
        }
        stacked as f32 / out.max(1) as f32
    }

    #[test]
    fn people_crossing_the_road_string_out_rather_than_stand_on_each_other() {
        // Cyrus, 2026-11-18: "walkers are still stacking on top of eachother when they are making
        // horizontal street crossings".
        let (before, now) = (crossers_stacked(Crossing::before()), crossers_stacked(Crossing::from_wash()));
        eprintln!("road crossers on another: {before} before, {now} now");
        // Twelve a sidewalk at each corner on a crosswalk a quarter of a camera height wide: some
        // pass close; none stands on another for long.
        assert!(now < 0.12 && now * 3.0 < before, "{now} of them on another, {before} before");
    }

    #[test]
    fn a_walker_crosses_a_cross_street_only_on_the_road_s_green_and_a_stroller_never_does() {
        let block = CROSS_EVERY * GRID_Z;
        // In a cross street: between its curbs.
        let in_street = |w: f32| w - (w / block).floor() * block < GRID_Z;
        for side in [0.0, 1.0] {
            let (walkers, strollers) = sidewalk(side);
            for (dir, key) in walkers {
                let mut t = 0.0f32;
                while t < 600.0 {
                    let (w, walking) = road_walker_at(dir, key, t);
                    let sig = t - (t / SIG_CYCLE).floor() * SIG_CYCLE;
                    // Those who wait for the light cross only on the road's green; the rest are
                    // checked against the cars below.
                    if in_street(w) && hash2(key.0 + 8.0, key.1 + 8.0) >= JAYWALK {
                        assert!(walking && sig < ROAD_GREEN, "walker {key:?} in a cross street at {t} ({sig} into the cycle)");
                    }
                    t += 0.05;
                }
            }
            for (dir, key) in strollers {
                let mut t = 0.0f32;
                while t < 600.0 {
                    let (w, shown) = road_stroller_at(dir, key, t);
                    let local = w - (w / block).floor() * block;
                    assert!(shown == 0.0 || (local >= GRID_Z + WALK_AT - 1e-3 && local <= block - WALK_AT + 1e-3), "stroller {key:?} off its block's sidewalk at {t}: {local}");
                    t += 0.05;
                }
            }
        }
    }

    #[test]
    fn people_waiting_at_a_corner_stand_apart_not_on_top_of_each_other() {
        // Cyrus, 2026-11-18: "people waiting to cross the street should be more spread-out in a
        // group. I don't want too many stacked directly on top of eachother". The view repeats
        // every WRAP_BLOCKS blocks, so two people that far apart stand on one spot on screen. On
        // top of another: within a person's width across and a body's depth along.
        let span = WRAP_BLOCKS * CROSS_EVERY * GRID_Z;
        let (mut standing, mut stacked) = (0usize, 0usize);
        for side in [0.0f32, 1.0] {
            let (walkers, _) = sidewalk(side);
            let mut t = 0.0f32;
            while t < 240.0 {
                let at: Vec<(f32, f32)> = walkers.iter().filter_map(|&(dir, key)| {
                    let (w, walking) = road_walker_at(dir, key, t);
                    let x = 0.96 + (hash2(key.0 + 7.0, key.1 + 7.0) - 0.22) * runic_constant("WALKER_SPREAD");
                    (!walking).then_some((w.rem_euclid(span), x))
                }).collect();
                for (i, a) in at.iter().enumerate() {
                    standing += 1;
                    if at.iter().enumerate().any(|(k, b)| k != i && (a.0 - b.0).abs() < 0.12 && (a.1 - b.1).abs() < 0.05) {
                        stacked += 1;
                    }
                }
                t += 0.25;
            }
        }
        let share = stacked as f32 / standing.max(1) as f32;
        eprintln!("{stacked} of {standing} standing people on top of another ({share})");
        // Before, everyone waited on the curb's line: 53 per cent of them on top of another.
        // Then 6 per cent; Cyrus, later the same day: "the crowds waiting to cross the street are
        // not spread out enough".
        assert!(share < 0.04, "{stacked} of {standing} standing people on top of another");
    }

    #[test]
    fn the_road_s_sidewalks_are_full_of_people_walking_between_the_corners() {
        // Cyrus, 2026-11-17: "more pedestrian traffic moving vertically", "distributed so the
        // pedestrian traffic looks fuller". Every block overlaid: its sidewalk between the
        // crosswalks in six stretches, each with people walking in it on average and hardly ever
        // none, and only a few people standing at the corners at once.
        let block = CROSS_EVERY * GRID_Z;
        let (lo, hi) = (GRID_Z + WALK_AT, block - WALK_AT);
        let (walkers, strollers) = sidewalk(0.0);
        let (mut moments, mut empty, mut most_standing) = (0usize, 0usize, 0usize);
        let mut total = [0usize; 6];
        let mut t = 0.0f32;
        while t < 240.0 {
            let mut bins = [0usize; 6];
            let mut standing = 0;
            let mut put = |w: f32| {
                let local = w - (w / block).floor() * block;
                if local >= lo && local < hi {
                    bins[(((local - lo) / (hi - lo)) * 6.0) as usize] += 1;
                }
            };
            for &(dir, key) in &walkers {
                let (w, walking) = road_walker_at(dir, key, t);
                if walking { put(w) } else { standing += 1 }
            }
            for &(dir, key) in &strollers {
                let (w, shown) = road_stroller_at(dir, key, t);
                if shown > 0.5 {
                    put(w);
                }
            }
            for (i, n) in bins.iter().enumerate() {
                total[i] += n;
                empty += usize::from(*n == 0);
            }
            moments += 1;
            most_standing = most_standing.max(standing);
            t += 0.25;
        }
        let means: Vec<f32> = total.iter().map(|&n| n as f32 / moments as f32).collect();
        let empty_share = empty as f32 / (moments * 6) as f32;
        eprintln!("stretches {means:?}, empty {empty_share}, most standing {most_standing} of {}", PEOPLE + STROLLERS);
        assert!(means.iter().all(|&m| m >= 4.0), "a thin stretch: {means:?}");
        // The crowd before (forty walkers all leaving the corners on the green and standing at the
        // next) averaged under three a stretch, a stretch empty a third of the time.
        assert!(empty_share < 0.05, "a stretch empty {empty_share} of the time");
        // The jaywalkers wait at the corners while a cross street's last car goes by and step off
        // behind it together (Cyrus, 2026-11-18: "they will cross the street as soon as the cars
        // have passed"): a few a corner then, never most of the sidewalk.
        assert!(most_standing * 5 <= (PEOPLE + STROLLERS) * 2, "{most_standing} people standing at once");
    }

    #[test]
    fn the_wash_s_sidewalks_are_the_ones_checked_here() {
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        for line in [
            "constant float AMBER = 1.0;",
            "constant int PEOPLE = 40;",
            "constant int STROLLERS = 48;",
            "    bool brisk = hash2(key + 2.0) < 0.65;",
            "    float v = brisk ? mix(0.54, 0.6, hash2(key + 6.0)) : mix(0.31, 0.36, hash2(key + 6.0));",
            "    float late = min(ROAD_GREEN - AMBER, ROAD_GREEN - cross / v);",
            "    float c = floor(hash2(key + 3.0) * laps) * SIG_CYCLE + (jay ? WALK_GO + JAY_SPREAD * hash2(key + 4.0) : late * hash2(key + 4.0));",
            "    bool jay = hash2(key + 8.0) < JAYWALK;",
            "    float back = jay ? WAIT_DEPTH * hash2(key + 9.0) : 0.0;",
            "    float front = jay ? back * CREEP_KEEP - GUTTER * hash2(key + 10.0) : 0.0;",
            "    float dist = block - back + front;",
            "    float creep = max(walk, period - CREEP_T);",
            "        x = dist + (back - front) * (s - creep) / (period - creep);",
            "    return float2(corner - dir * front + dir * x, moving ? 1.0 : 0.0);",
            "    float j = floor(hash2(key + 5.0) * 8.0) + dir * k;",
            "    float v = mix(0.36, 0.58, hash2(key + 2.0));",
            "    float period = span / v + 1.0;",
            "    float s = t + hash2(key + 3.0) * period;",
            "    float2 lap = key + float2(k * 7.0, 0.0);",
            "    float len = span * mix(0.4, 1.0, hash2(lap + 5.0));",
            "    float a = hash2(lap + 4.0) * (span - len);",
            "    float shown = smoothstep(0.0, 0.4, into) * (1.0 - smoothstep(dur - 0.4, dur, into));",
            "    float j = floor(hash2(lap + 6.0) * 8.0);",
            "            float2 key = float2(float(j), float(side) * 17.0 + 61.0);",
            "            float2 key = float2(float(j), float(side) * 17.0 + 211.0);",
        ] {
            assert!(wash.contains(line), "wash.metal lacks {line}");
        }
    }

    /// A shader constant's value, read from the runic wash's text.
    fn runic_constant(name: &str) -> f32 {
        // Read once: the mirrors call this in loops over minutes of simulated time.
        static CONSTANTS: std::sync::OnceLock<std::collections::HashMap<String, f32>> = std::sync::OnceLock::new();
        let constants = CONSTANTS.get_or_init(|| {
            let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
            wash.lines()
                .filter_map(|l| l.strip_prefix("constant float "))
                .filter_map(|l| l.split_once(" = ").map(|(n, v)| (n.to_string(), v.trim_end_matches(';').parse().ok())))
                .filter_map(|(n, v)| v.map(|v| (n, v)))
                .collect()
        });
        *constants.get(name).unwrap_or_else(|| panic!("no {name}"))
    }

    /// Where the food cart's line has person `i` on lot column `kx`, as the wash's `small_lot_at`
    /// works it out: across (x) and along from the lot's near edge (z), `step` of the way through
    /// stepping up and the first `leave` of the way through walking off with their food.
    fn cart_line_at(kx: i32, i: i32, step: f32, leave: f32) -> (f32, f32) {
        let xm = (kx as f32 + 0.5) * 0.9;
        let side = if kx < 0 { 1.0 } else { -1.0 };
        let wz = 0.5 * 1.6 + 0.1;
        let qx = xm + side * (runic_constant("CART_W") + runic_constant("CART_FRONT"));
        let qz = wz - 0.05 + (i as f32 - step) * runic_constant("CART_GAP") - leave * 0.5;
        (qx, qz)
    }

    #[test]
    fn the_food_cart_s_line_stays_on_the_sidewalk_by_the_road() {
        // Cyrus, 2026-11-18: "the line of people waiting for the food cart stretches into the
        // street. It should stay within the sidewalk."
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        for line in [
            "            float qx = xm + side * (CART_W + CART_FRONT);",
            "            float qz = wz - 0.05 + (float(i) - step) * CART_GAP - leave * 0.5;",
        ] {
            assert!(wash.contains(line), "wash.metal lacks {line}");
        }
        let curb = 0.9 - runic_constant("SIDEWALK_IN");
        let back = curb + runic_constant("SIDEWALK_W");
        for kx in [1, -2] {
            for i in 0..4 {
                for k in 0..=20 {
                    let step = k as f32 / 20.0;
                    let (x, _) = cart_line_at(kx, i, step, if i == 0 { step } else { 0.0 });
                    assert!(x.abs() > curb + 0.03 && x.abs() < back, "lot {kx}, person {i}: {x} off the sidewalk {curb}..{back}");
                }
            }
        }
    }

    #[test]
    fn the_food_cart_s_line_stays_in_its_lot_further_out() {
        for kx in (2..=6).chain(-7..=-3) {
            let x0 = kx as f32 * 0.9;
            for i in 0..4 {
                for k in 0..=20 {
                    let step = k as f32 / 20.0;
                    let (x, z) = cart_line_at(kx, i, step, if i == 0 { step } else { 0.0 });
                    assert!(x > x0 && x < x0 + 0.9, "lot {kx}, person {i}: {x} outside {x0}..");
                    assert!(z > 0.0 && z < 1.6, "lot {kx}, person {i}: {z} off the lot along the road");
                }
            }
        }
    }

    /// A sidewalk shed as the wash draws it on the tower at lot (kx, m): on the tower's road side,
    /// |x| of its posts' line and of the tower's face, its run along the road from the lot's near
    /// edge (z0 to z1), its deck's height and the top of any scaffolding (the deck's without).
    #[derive(Debug, Clone, Copy)]
    struct Shed {
        post_x: f32,
        face_x: f32,
        z0: f32,
        z1: f32,
        deck: f32,
        top: f32,
        roof: f32,
    }

    fn tower_built(kx: i32, m: i32) -> bool {
        // A cross street's row (every fourth) has no lots.
        m.rem_euclid(4) != 0 && hash2(kx as f32 + 17.0, m as f32 + 3.0) <= runic_constant("TOWER_ON")
    }

    fn shed_of(kx: i32, m: i32) -> Option<Shed> {
        let beside_road = kx == 1 || kx == -2;
        if !beside_road || !tower_built(kx, m) || hash2(kx as f32 + 61.0, m as f32 + 13.0) >= runic_constant("SHED_SHARE") {
            return None;
        }
        let scaffold = hash2(kx as f32 + 67.0, m as f32 + 19.0) < runic_constant("SCAFFOLD_SHARE");
        let tw = 0.9 * (0.5 + 0.35 * hash2(kx as f32 + 5.0, m as f32 + 11.0));
        let td = 1.6 * (0.35 + 0.35 * hash2(kx as f32 + 9.0, m as f32 + 2.0));
        let th = 0.6 + (runic_constant("TOWER_MAX") - 0.6) * hash2(kx as f32 + 4.0, m as f32 + 7.0).powf(1.6);
        let tx = (kx as f32 + 0.5) * 0.9;
        let deck = runic_constant("SHED_H");
        let top = if scaffold { (th - 0.05).min(deck + runic_constant("SCAFFOLD_FLOORS") * runic_constant("SCAFFOLD_FLOOR")) } else { deck };
        let z0 = 0.5 * 1.6 - 0.5 * td;
        Some(Shed { post_x: 0.9 - runic_constant("SIDEWALK_IN") + runic_constant("SHED_IN"), face_x: tx.abs() - 0.5 * tw, z0, z1: z0 + td, deck, top, roof: th })
    }

    #[test]
    fn about_four_in_ten_towers_beside_the_road_wear_a_sidewalk_shed() {
        // Cyrus, 2026-11-18: "the Matrix City needs some scaffolding alongside some buildings so
        // it feels more like NYC"; he chose sidewalk sheds, some with scaffolding above.
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        for line in [
            "                bool beside_road = kx == ROAD_CELLS || kx == -ROAD_CELLS - 1;",
            "                bool shed = beside_road && hash2(cell + float2(61.0, 13.0)) < SHED_SHARE;",
            "                bool scaffold = shed && hash2(cell + float2(67.0, 19.0)) < SCAFFOLD_SHARE;",
            "                float shed_top = scaffold ? min(th - 0.05, SHED_H + SCAFFOLD_FLOORS * SCAFFOLD_FLOOR) : SHED_H;",
            "                    float sgn = tx < 0.0 ? -1.0 : 1.0;",
            "                    float sx = sgn * (float(ROAD_CELLS) * GRID_X - SIDEWALK_IN + SHED_IN);",
            "                    float fx = tx - sgn * tw * 0.5;",
        ] {
            assert!(wash.contains(line), "wash.metal lacks {line}");
        }
        let (mut built, mut sheds) = (0, 0);
        for kx in [1, -2] {
            for m in 0..400 {
                built += usize::from(tower_built(kx, m));
                sheds += usize::from(shed_of(kx, m).is_some());
            }
        }
        let share = sheds as f32 / built as f32;
        assert!((0.3..0.55).contains(&share), "{sheds} of {built} towers beside the road");
        assert!((0..400).all(|m| [2, 3, 6, -3, -7].iter().all(|kx| shed_of(*kx, m).is_none())), "only beside the road");
    }

    #[test]
    fn a_shed_s_posts_stand_on_the_sidewalk_never_in_the_road() {
        let curb = 0.9 - runic_constant("SIDEWALK_IN");
        for kx in [1, -2] {
            for m in 0..400 {
                if let Some(s) = shed_of(kx, m) {
                    assert!(s.post_x > curb && s.post_x < s.face_x, "lot ({kx}, {m}): posts at {} between the curb {curb} and the face {}", s.post_x, s.face_x);
                    assert!(s.z0 > 0.0 && s.z1 < 1.6 && s.z0 < s.z1, "lot ({kx}, {m}): it runs the tower's depth within its lot");
                }
            }
        }
    }

    #[test]
    fn a_shed_on_the_road_s_left_stands_on_the_left_sidewalk() {
        // The shader's signed placement, as its pinned lines work it out: the posts and the face
        // on the tower's own side of the road, the posts between the curb and the face.
        let curb = 0.9 - runic_constant("SIDEWALK_IN");
        for kx in [1, -2] {
            let tx = (kx as f32 + 0.5) * 0.9;
            let sgn = if tx < 0.0 { -1.0 } else { 1.0 };
            for m in 0..400 {
                if let Some(s) = shed_of(kx, m) {
                    let tw = 0.9 * (0.5 + 0.35 * hash2(kx as f32 + 5.0, m as f32 + 11.0));
                    let (sx, fx) = (sgn * (curb + runic_constant("SHED_IN")), tx - sgn * tw * 0.5);
                    assert!(sx.signum() == tx.signum() && fx.signum() == tx.signum(), "lot ({kx}, {m}): across the road");
                    assert!((sx.abs() - s.post_x).abs() < 1e-5 && (fx.abs() - s.face_x).abs() < 1e-5);
                }
            }
        }
    }

    #[test]
    fn a_shed_s_pipes_stand_out_as_a_tower_s_edges_do() {
        // Cyrus, 2026-11-18: "The scaffolds and sheds are not visible enough." Their pipes are lit
        // as brightly as a tower's edges, in the same whitened green, the deck's boards and the
        // scaffolding's netting fill their planes, and more towers wear them.
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        assert!(wash.contains("float s = (1.0 - smoothstep(0.0, 1.0, edge)) * 0.8"), "a tower's edges at 0.8");
        assert!(runic_constant("SHED_LIGHT") >= 0.8, "the pipes as bright as a tower's edges");
        assert!(runic_constant("SHED_DECK_FILL") >= 0.3 && runic_constant("SCAFFOLD_NET") >= 0.2, "planes filled");
        assert!(runic_constant("SHED_SHARE") >= 0.4 && runic_constant("SCAFFOLD_SHARE") >= 0.5, "more of them");
        assert!(wash.contains("    return light_on(col, mix(green, white_c, 0.25), lit * seen, u.dark);"), "whitened as the towers' lines");
    }

    #[test]
    fn people_walk_under_a_shed_s_deck() {
        assert!(runic_constant("SHED_H") >= runic_constant("PERSON_H") + 0.04);
    }

    /// A tower's box as the tower loop works it out: (tw, td, th, tx, zf in its lot).
    fn tower_box(kx: i32, m: i32) -> (f32, f32, f32, f32, f32) {
        let (x, y) = (kx as f32, m as f32);
        let tw = 0.9 * (0.5 + 0.35 * hash2(x + 5.0, y + 11.0));
        let td = 1.6 * (0.35 + 0.35 * hash2(x + 9.0, y + 2.0));
        let th = 0.6 + (runic_constant("TOWER_MAX") - 0.6) * hash2(x + 4.0, y + 7.0).powf(1.6);
        (tw, td, th, (x + 0.5) * 0.9, 0.5 * 1.6 - 0.5 * td)
    }

    /// The water tower on lot (kx, m), as the tower loop places it: its x, its z in the lot and
    /// the roof it stands on.
    fn water_tower_of(kx: i32, m: i32) -> Option<(f32, f32, f32)> {
        if kx.abs() > 7 || (-1..1).contains(&kx) || !tower_built(kx, m) {
            return None;
        }
        let (tw, td, th, tx, zf) = tower_box(kx, m);
        let (x, y) = (kx as f32, m as f32);
        let r = runic_constant("WT_R");
        if th < runic_constant("WT_MIN") || th > runic_constant("WT_MAX") || hash2(x + 71.0, y + 23.0) >= runic_constant("WT_SHARE") {
            return None;
        }
        let wx = tx + (hash2(x + 73.0, y + 29.0) - 0.5) * (tw - 2.0 * r - 0.04);
        let wz = zf + r + 0.02 + hash2(x + 79.0, y + 31.0) * (td - 2.0 * r - 0.04);
        Some((wx, wz, th))
    }

    #[test]
    fn about_six_in_ten_middling_towers_wear_a_water_tower_and_no_tall_or_short_one() {
        // Cyrus, 2026-11-18, of what would make the city feel more like New York: "do 1, 2 and 3",
        // the first being rooftop water towers.
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        for line in [
            "                bool water = th >= WT_MIN && th <= WT_MAX && hash2(cell + float2(71.0, 23.0)) < WT_SHARE;",
            "                float wtx = tx + (hash2(cell + float2(73.0, 29.0)) - 0.5) * (tw - 2.0 * WT_R - 0.04);",
            "                float wtz = zf + WT_R + 0.02 + hash2(cell + float2(79.0, 31.0)) * (td - 2.0 * WT_R - 0.04);",
        ] {
            assert!(wash.contains(line), "wash.metal lacks {line}");
        }
        let (mut middling, mut worn) = (0, 0);
        for kx in (-7..-1).chain(1..7) {
            for m in 0..300 {
                if !tower_built(kx, m) {
                    continue;
                }
                let th = tower_box(kx, m).2;
                let w = water_tower_of(kx, m);
                if (runic_constant("WT_MIN")..=runic_constant("WT_MAX")).contains(&th) {
                    middling += 1;
                    worn += usize::from(w.is_some());
                } else {
                    assert!(w.is_none(), "lot ({kx}, {m}): a water tower on a tower {th} tall");
                }
            }
        }
        let share = worn as f32 / middling as f32;
        assert!((0.5..0.7).contains(&share), "{worn} of {middling} middling towers");
        // Manhattan's tallest towers hide theirs; a walk-up or a mid-rise shows it.
        assert!(runic_constant("WT_MIN") >= 0.8 && runic_constant("WT_MAX") <= 0.65 * runic_constant("TOWER_MAX"));
    }

    #[test]
    fn a_water_tower_stands_on_its_roof_never_over_the_edge() {
        let r = runic_constant("WT_R");
        for kx in (-7..-1).chain(1..7) {
            for m in 0..300 {
                if let Some((wx, wz, _)) = water_tower_of(kx, m) {
                    let (tw, td, _, tx, zf) = tower_box(kx, m);
                    assert!((wx - tx).abs() + r <= tw * 0.5, "lot ({kx}, {m}): over the side");
                    assert!(wz - r >= zf && wz + r <= zf + td, "lot ({kx}, {m}): over the front or back");
                }
            }
        }
        // A barrel on legs under a cone, drawn about a third of a narrow tower's width across so it
        // reads at the sidebar's size (at 0.09 across it was two points there).
        let h = runic_constant("WT_LEGS") + runic_constant("WT_TANK") + runic_constant("WT_CONE");
        assert!((0.25..0.35).contains(&h) && (0.15..0.2).contains(&(2.0 * r)));
    }

    #[test]
    fn a_water_tower_reads_as_a_tower_s_edges_do() {
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        let body = &wash[wash.find("float3 water_tower_at(").expect("a water_tower_at function")..];
        let body = &body[..body.find("\n}\n").unwrap()];
        assert!(body.contains("bg * 0.75"), "its tank filled as a tower's face is, hiding what is behind");
        assert!(body.contains("* 0.8 * seen"), "its outline lit as a tower's edges");
        assert!(wash.contains("col = water_tower_at("), "drawn in the tower loop");
    }

    /// Manhole k (two a block), as the wash's `steam_vent` places it: x across the road and z in
    /// the world, if it breathes steam.
    fn steam_vent(k: i32) -> Option<(f32, f32)> {
        let block = CROSS_EVERY * GRID_Z;
        let fj = k as f32;
        let j = (fj * 0.5).floor();
        let half = fj - 2.0 * j;
        if hash2(fj, 97.0) >= runic_constant("STEAM_SHARE") {
            return None;
        }
        let side = if hash2(fj, 101.0) < 0.5 { -1.0 } else { 1.0 };
        let x = side * (0.15 + 0.4 * hash2(fj, 83.0));
        let z = j * block + GRID_Z + WALK_AT + 0.3 + (half + hash2(fj, 89.0)) * 0.5 * (block - GRID_Z - 2.0 * WALK_AT - 0.6);
        Some((x, z))
    }

    #[test]
    fn most_blocks_breathe_steam_from_a_manhole_or_two() {
        // Cyrus, 2026-11-18, of what would make the city feel more like New York: "do 1, 2 and 3",
        // the second being steam from the manholes.
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        for line in [
            "    if (hash2(float2(fj, 97.0)) >= STEAM_SHARE) {",
            "    float side = hash2(float2(fj, 101.0)) < 0.5 ? -1.0 : 1.0;",
            "    float x = side * (0.15 + 0.4 * hash2(float2(fj, 83.0)));",
            "    float z = j * block + GRID_Z + WALK_AT + 0.3 + (half_k + hash2(float2(fj, 89.0))) * 0.5 * (block - GRID_Z - 2.0 * WALK_AT - 0.6);",
        ] {
            assert!(wash.contains(line), "wash.metal lacks {line}");
        }
        let n = (0..800).filter(|k| steam_vent(*k).is_some()).count();
        assert!((480..640).contains(&n), "{n} of 800 manholes");
        let none = (0..400).filter(|j| steam_vent(2 * j).is_none() && steam_vent(2 * j + 1).is_none()).count();
        assert!(none < 60, "{none} of 400 blocks without steam");
        assert!(wash.contains("col = steam_at("), "drawn in the city");
    }

    #[test]
    fn steam_rises_from_a_traffic_lane_never_a_crosswalk_or_a_bike_lane() {
        let block = CROSS_EVERY * GRID_Z;
        let bike = runic_constant("BIKE_LANE") * 0.9;
        let reach = runic_constant("STEAM_W1") * 0.5 + runic_constant("STEAM_DRIFT");
        for k in 0..800 {
            let j = k / 2;
            if let Some((x, z)) = steam_vent(k) {
                assert!(x.abs() + runic_constant("STEAM_W0") * 0.5 < bike - 0.05, "block {j}: its manhole by the bike lane");
                assert!(x.abs() + reach < 0.9, "block {j}: its plume over the sidewalk");
                let local = z - j as f32 * block;
                // The crosswalks: WALK_W wide, WALK_AT past either side of the cross street.
                let (near, far) = (GRID_Z + WALK_AT + WALK_W * 0.5, block - WALK_AT - WALK_W * 0.5);
                assert!(local - runic_constant("STEAM_W0") > near && local + runic_constant("STEAM_W0") < far, "block {j}: over a crosswalk");
            }
        }
    }

    #[test]
    fn steam_rises_about_two_stories_so_it_reads_at_the_sidebar_s_size() {
        // At three people tall (0.48) a plume halfway up the road was four points high.
        let (h, p) = (runic_constant("STEAM_H"), runic_constant("PERSON_H"));
        assert!(h >= 5.0 * p && h <= 6.5 * p, "{h} high");
        assert!(runic_constant("STEAM_LIGHT") >= 0.5, "seen at the sidebar's size");
    }

    /// The curb's line and a subway entrance's railing across the road's sidewalk: (inner, outer)
    /// |x|.
    fn entrance_rails() -> (f32, f32) {
        let ri = 0.9 - runic_constant("SIDEWALK_IN") + 0.01;
        (ri, ri + runic_constant("ENTRANCE_W"))
    }

    /// Whether lot (kx, m) has a subway entrance at its corner, as the wash's `entrance_at` works it
    /// out, and its railing's z span in the lot.
    fn entrance_at(kx: i32, m: i32) -> Option<(f32, f32)> {
        let corner = m.rem_euclid(4);
        if !(kx == 1 || kx == -2) || !(corner == 1 || corner == 3) || hash2(kx as f32 + 97.0, m as f32 + 37.0) >= runic_constant("ENTRANCE_SHARE") {
            return None;
        }
        if !tower_built(kx, m) {
            return None;
        }
        let (tw, _, _, tx, _) = tower_box(kx, m);
        if shed_of(kx, m).is_some() || tx.abs() - tw * 0.5 < entrance_rails().1 + 0.02 {
            return None;
        }
        let (at, len) = (runic_constant("ENTRANCE_AT"), runic_constant("ENTRANCE_LEN"));
        Some(if corner == 1 { (at, at + len) } else { (1.6 - at - len, 1.6 - at) })
    }

    /// Where a person whose place across the road's sidewalk is ax stands at world z zw on the
    /// road's side `side`, as the wash's `sidewalk_x` presses it aside beside an entrance.
    fn sidewalk_x(side: f32, ax: f32, zw: f32) -> f32 {
        let m = (zw / 1.6).floor();
        let local = zw - m * 1.6;
        let kx = if side > 0.0 { 1 } else { -2 };
        let Some((z0, z1)) = entrance_at(kx, m as i32) else { return side * ax };
        let ease = runic_constant("ENTRANCE_EASE");
        let smooth = |e0: f32, e1: f32, x: f32| {
            let u = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
            u * u * (3.0 - 2.0 * u)
        };
        let clear = runic_constant("ENTRANCE_CLEAR");
        let k = smooth(z0 - clear - ease, z0 - clear, local) * (1.0 - smooth(z1 + clear, z1 + clear + ease, local));
        let spread = runic_constant("WALKER_SPREAD");
        let (lo_w, hi_w) = (0.96 - 0.22 * spread, 0.96 + 0.78 * spread);
        let lo = entrance_rails().1 + runic_constant("ENTRANCE_CLEAR");
        let pressed = (lo + (ax - lo_w) * (hi_w - lo) / (hi_w - lo_w)).max(lo);
        side * (ax + (pressed - ax) * k)
    }

    #[test]
    fn about_a_third_of_the_road_s_corners_have_a_subway_entrance_never_under_a_shed() {
        // Cyrus, 2026-11-18, of what would make the city feel more like New York: "do 1, 2 and 3",
        // the third being subway entrances.
        let (mut corners, mut entrances) = (0, 0);
        for kx in [1, -2] {
            for m in 0..800 {
                if m % 4 == 1 || m % 4 == 3 {
                    corners += 1;
                    entrances += usize::from(entrance_at(kx, m).is_some());
                } else {
                    assert!(entrance_at(kx, m).is_none(), "lot ({kx}, {m}) is no corner");
                }
            }
        }
        let share = entrances as f32 / corners as f32;
        assert!((0.25..0.42).contains(&share), "{entrances} of {corners} corners");
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        for line in [
            "    if (hash2(float2(float(kx), float(m)) + float2(97.0, 37.0)) >= ENTRANCE_SHARE) {",
            "    bool shed = hash2(cell + float2(61.0, 13.0)) < SHED_SHARE;",
            "    return !(shed || abs(tx) - tw * 0.5 < entrance_out() + 0.02);",
        ] {
            assert!(wash.contains(line), "wash.metal lacks {line}");
        }
    }

    #[test]
    fn a_subway_entrance_stands_at_the_curb_clear_of_the_corner_s_crowd_and_its_tower() {
        let (ri, ro) = entrance_rails();
        let curb = 0.9 - runic_constant("SIDEWALK_IN");
        assert!(ri > curb && ro < curb + runic_constant("SIDEWALK_W") * 0.5, "at the curb: {ri}..{ro}");
        // The crossers wait up to WALK_AT + WALK_W / 2 + WAIT_ALONG from the cross street's curb.
        let crowd = WALK_AT + 0.5 * WALK_W + runic_constant("WAIT_ALONG");
        for kx in [1, -2] {
            for m in 0..800 {
                if let Some((z0, z1)) = entrance_at(kx, m) {
                    let from_curb = if m % 4 == 1 { z0 } else { 1.6 - z1 };
                    assert!(from_curb > crowd + 0.05, "lot ({kx}, {m}): in the corner's crowd");
                    let reach = runic_constant("ENTRANCE_EASE") + runic_constant("ENTRANCE_CLEAR");
                    assert!(z0 - reach > 0.0 && z1 + reach < 1.6, "lot ({kx}, {m}): its crowd's easing out of its lot");
                }
            }
        }
    }

    #[test]
    fn no_walker_or_stroller_ever_walks_through_a_subway_entrance() {
        let (_, ro) = entrance_rails();
        let clear = runic_constant("ENTRANCE_CLEAR");
        let edge = 0.9 - runic_constant("SIDEWALK_IN") + runic_constant("SIDEWALK_W");
        let spread = runic_constant("WALKER_SPREAD");
        let mut checked = 0;
        for side in [-1.0f32, 1.0] {
            let kx = if side > 0.0 { 1 } else { -2 };
            for m in 0..400 {
                let Some((z0, z1)) = entrance_at(kx, m) else { continue };
                checked += 1;
                for i in 0..=100 {
                    let h = i as f32 / 100.0;
                    for ax in [0.96 + (h - 0.22) * spread, 0.96 + (0.1 + 0.6 * h) * WALK_W] {
                        let mut z = z0 - 0.03;
                        while z <= z1 + 0.03 {
                            let x = sidewalk_x(side, ax, m as f32 * 1.6 + z).abs();
                            assert!(x >= ro + clear - 1e-4 && x < edge, "lot ({kx}, {m}): a person at {x} beside a railing out to {ro}");
                            z += 0.01;
                        }
                    }
                }
            }
        }
        assert!(checked > 50);
        // Away from an entrance, everyone keeps their own place.
        assert_eq!(sidewalk_x(1.0, 0.95, 0.1), 0.95);
    }

    #[test]
    fn the_city_s_data_lands_on_a_walker_where_it_is_drawn() {
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        assert_eq!(wash.matches("sidewalk_x(").count(), 6, "declared, defined, and used by walkers, strollers, data_end and small_end");
        assert!(!wash.contains("side * edge_x + side * (hash2(key + 7.0) - 0.22) * WALKER_SPREAD"), "data_end's own walker x is gone");
        assert!(!wash.contains("side * (road + 0.06) + side * (hash2(key + 7.0) - 0.22) * WALKER_SPREAD"), "small_end's own walker x is gone");
    }

    #[test]
    fn most_subway_globes_are_green_some_red() {
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        assert!((0.15..0.35).contains(&runic_constant("ENTRANCE_RED")), "exit only, now and then");
        assert!(wash.contains("bool red = hash2(cell + float2(107.0, 41.0)) < ENTRANCE_RED;"));
        assert!(wash.contains("col = entrance_draw("), "drawn in the lot loop");
        // The globes stand as tall as the lone lots' entrances' and the railing is waist high.
        assert!((runic_constant("ENTRANCE_LAMP") - 0.26).abs() < 1e-4 && (0.08..0.13).contains(&runic_constant("ENTRANCE_RAIL")));
    }

    #[test]
    fn a_plume_is_drawn_with_its_own_row_so_only_nearer_towers_cover_it() {
        // A plume drawn before the whole city was covered by farther towers' faces too (the
        // review of 2026-11-18): its upper half cut out in a distant building's shape.
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        let city = wash.find("    // The city, farthest row first").unwrap();
        let steam = wash.find("col = steam_at(").unwrap();
        let entrances = wash.find("col = entrance_draw(").unwrap();
        assert!(steam > city && steam > entrances, "drawn in the row loop, after the row's towers and entrances");
        assert!(wash.contains("            if (int(floor(vent.y / GRID_Z)) != m) {"), "each plume in the row holding its manhole");
    }

    #[test]
    fn a_subway_entrance_stands_only_beside_a_tower_never_in_an_empty_lot() {
        // An empty corner lot may be a park or a lone subway entrance of its own, which the crowd
        // pressed aside would walk through (the review of 2026-11-18).
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        assert!(wash.contains("    if (!lot_built(kx, m)) {\n        return false;\n    }"), "entrance_at refuses an empty lot");
        for kx in [1, -2] {
            for m in 0..800 {
                if entrance_at(kx, m).is_some() {
                    assert!(tower_built(kx, m), "lot ({kx}, {m}) is empty");
                }
            }
        }
    }

    #[test]
    fn about_half_the_sheds_have_scaffolding_never_above_the_roof() {
        let (mut sheds, mut scaffolds) = (0, 0);
        for kx in [1, -2] {
            for m in 0..400 {
                if let Some(s) = shed_of(kx, m) {
                    sheds += 1;
                    if s.top > s.deck {
                        scaffolds += 1;
                        assert!(s.top <= s.roof, "lot ({kx}, {m}): scaffolding to {} over a roof at {}", s.top, s.roof);
                    }
                }
            }
        }
        let share = scaffolds as f32 / sheds as f32;
        assert!((0.35..0.65).contains(&share), "{scaffolds} of {sheds} sheds");
    }

    #[test]
    fn every_walker_keeps_to_a_paved_sidewalk() {
        // Cyrus, 2026-11-17: "I want the sidewalks more prominent". The wash paves where the people
        // walk: along the road from just inside its edge, and along both sides of each cross street
        // from its curb.
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        let road_e = 1.0 * 0.9; // ROAD_CELLS * GRID_X
        assert!(wash.contains("constant int ROAD_CELLS = 1;") && wash.contains("constant float GRID_X = 0.9;"));
        let (inset, width) = (runic_constant("SIDEWALK_IN"), runic_constant("SIDEWALK_W"));
        let (lo, hi) = (road_e - inset, road_e - inset + width);
        // The road's walkers: edge_x plus (h - 0.22) * WALKER_SPREAD, h from 0 to 1; its strollers: edge_x
        // plus (0.1 + 0.6 h) * WALK_W.
        let edge_x = road_e + 0.06;
        for h in [0.0f32, 0.5, 1.0] {
            for x in [edge_x + (h - 0.22) * runic_constant("WALKER_SPREAD"), edge_x + (0.1 + 0.6 * h) * WALK_W] {
                assert!(x > lo && x < hi, "a walker at {x} off the road's sidewalk {lo}..{hi}");
            }
        }
        // The cross streets' walkers: WALK_AT from the curb, spread 0.3 WALK_W either way.
        for off in [-0.3f32, 0.0, 0.3] {
            let at = WALK_AT + off * WALK_W;
            assert!(at > 0.0 && at < width, "a cross street's walker {at} off its sidewalk 0..{width}");
        }
        assert!(runic_constant("SIDEWALK_FILL") >= 0.3 && runic_constant("SIDEWALK_CURB_LIGHT") >= 0.4, "prominent: a lit curb and a paved fill");
    }


    /// Transfer slot i as the wash's `data_slot` works it out: whether it is under way, and its
    /// travel's seconds.
    fn data_slot(i: usize, t: f32) -> (bool, f32) {
        let fi = i as f32;
        let (lo, hi) = (runic_constant("DATA_SLOT_MIN"), runic_constant("DATA_SLOT_MAX"));
        let period = lo + (hi - lo) * hash2(fi, 3.0);
        let off = hash2(fi, 5.0) * period;
        let k = ((t + off) / period).floor();
        let into = t + off - k * period;
        let seed = fi * 1013.0 + k;
        let travel = runic_constant("DATA_TRAVEL_MIN") + (runic_constant("DATA_TRAVEL_MAX") - runic_constant("DATA_TRAVEL_MIN")) * hash2(seed, 1.0);
        (into < travel * 1.6, travel)
    }

    #[test]
    fn the_city_s_data_is_a_steady_trickle_both_ways_to_every_kind_of_thing() {
        // Cyrus, 2026-11-17: all the rain is data, up and down, to and from the city's people,
        // cyclists, cars, food carts and buildings, a steady trickle (8 to 12 at once).
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        assert!(!wash.contains("code_rain(") && wash.contains("data_rain(p, top,"), "no ambient rain: only the data");
        let n = wash.lines().find(|l| l.starts_with("constant int DATA_STREAMS = ")).unwrap().rsplit("= ").next().unwrap().trim_end_matches(';').parse::<usize>().unwrap();
        let (mut sum, mut moments, mut least, mut most) = (0usize, 0usize, usize::MAX, 0usize);
        let mut t = 0.0f32;
        while t < 300.0 {
            let busy = (0..n).filter(|i| data_slot(*i, t).0).count();
            sum += busy;
            least = least.min(busy);
            most = most.max(busy);
            moments += 1;
            t += 0.25;
        }
        let mean = sum as f32 / moments as f32;
        // Some fall out of view or find no cart; the slots run a little over the 8 to 12 seen.
        assert!((8.0..=13.0).contains(&mean), "{mean} transfers on average");
        assert!(least >= 3 && most <= n, "never an empty sky: {least}..{most}");
        // Each kind takes a share of the slots, and the two directions about half each.
        let shares = [runic_constant("DATA_WALKER"), runic_constant("DATA_CAR") - runic_constant("DATA_WALKER"), runic_constant("DATA_BIKE") - runic_constant("DATA_CAR"), runic_constant("DATA_WINDOW") - runic_constant("DATA_BIKE"), 1.0 - runic_constant("DATA_WINDOW")];
        assert!(shares.iter().all(|s| *s >= 0.05), "{shares:?}");
        assert!(wash.contains("bool up = hash2(float2(slot.w, 3.0)) < 0.5;"), "up and down alike");
        // A transfer takes a few seconds: readable, not a blink.
        assert!(runic_constant("DATA_TRAVEL_MIN") >= 1.5 && runic_constant("DATA_TRAVEL_MAX") <= 5.0);
    }


    #[test]
    fn a_transfer_reads_moving_at_the_sidebar_s_size() {
        // Seen moving on 2026-11-17: at a 260 point sidebar the channel and a one-cell packet were
        // a few faint points, and the rain read as a few dotted columns over the roofs.
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        // Then, in the sandbox the same day: "the rain in the preview is too noisy right now". The
        // moving trails carry it; the channel is a hint, the glow soft, and the glyphs change slowly.
        assert!(runic_constant("DATA_CHANNEL") <= 0.25, "the channel only hints where the data goes");
        assert!((0.2..=0.35).contains(&runic_constant("DATA_HALO")), "the packet carries a soft glow");
        assert!(runic_constant("DATA_FLICKER") >= 1.0, "a glyph changes about once a second at most");
        assert!(!wash.contains("floor(t / 0.4)") && !wash.contains("floor(t / 0.3)"), "no faster glyph changes left");
        let rain = &wash[wash.find("float3 data_rain(").unwrap()..];
        let rain = &rain[..rain.find("\n}\n").unwrap()];
        assert!(rain.contains("DATA_HALO"), "drawn in data_rain");
        // The halo is drawn before the column's own cull, so it shows beside the column.
        assert!(rain.find("DATA_HALO").unwrap() < rain.find("abs(p.x - sx) > cw * 0.5").unwrap());
        // A transfer looks for a thing in view among several, so few slots draw nothing.
        assert!(rain.contains("n < DATA_TRIES") && runic_constant("DATA_TRIES") >= 5.0);
    }

    fn wash_int(name: &str) -> usize {
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        let line = wash.lines().find(|l| l.starts_with(&format!("constant int {name} = "))).unwrap_or_else(|| panic!("no {name}"));
        line.rsplit("= ").next().unwrap().trim_end_matches(';').parse().unwrap()
    }

    fn wrap_view(z: f32) -> f32 {
        let span = WRAP_BLOCKS * CROSS_EVERY * GRID_Z;
        let near = runic_constant("Z_NEAR");
        near + (z - near) - ((z - near) / span).floor() * span
    }

    /// The wash's `data_drizzle` slot `i` at `t`: how far through its run it is (0 to 1), its seed
    /// and when it began, or nothing while it rests.
    fn small_slot(i: usize, t: f32) -> Option<(f32, f32, f32)> {
        let fi = i as f32;
        let (lo, hi) = (runic_constant("DRIZZLE_PERIOD_MIN"), runic_constant("DRIZZLE_PERIOD_MAX"));
        let period = lo + (hi - lo) * hash2(fi, 47.0);
        let off = hash2(fi, 49.0) * period;
        let k = ((t + off) / period).floor();
        let into = t + off - k * period;
        let run = period * runic_constant("DRIZZLE_ON");
        (into <= run).then_some((into / run, fi * 733.0 + k, t - into))
    }

    /// The wash's `small_end` for the kinds the tests can follow: the walker, car or cyclist a
    /// small transfer joins (its kind, 0 to 2, and its depth in view at `t`), or a roof (3, with
    /// its depth); nothing when it is out of the nearer view. (Whether a roof's lot has a tower is
    /// the shader's to say, four lots in five.)
    fn small_end(seed: f32, side: f32, t: f32, t0: f32) -> Option<(usize, f32)> {
        let run = |t: f32| t * runic_constant("GRID_SPEED") * GRID_Z;
        let c = hash2(seed, 2.0);
        let pick = hash2(seed, 7.0);
        let (kind, z) = if c < runic_constant("SMALL_WALKER") {
            let key = ((pick * wash_int("PEOPLE") as f32).floor(), if side > 0.0 { 17.0 + 61.0 } else { 61.0 });
            let dir = if hash2(key.0 + 1.0, key.1 + 1.0) < 0.5 { 1.0 } else { -1.0 };
            (0, wrap_view(road_walker_at(dir, key, t).0 - run(t)))
        } else if c < runic_constant("SMALL_CAR") {
            let key = ((pick * wash_int("ROAD_CARS") as f32).floor(), if side > 0.0 { 13.0 + 31.0 } else { 31.0 });
            (1, wrap_view(road_car_at(side, key, t) - run(t)))
        } else if c < runic_constant("SMALL_BIKE") {
            let key = ((pick * wash_int("ROAD_BIKES") as f32).floor(), if side > 0.0 { 13.0 + 141.0 } else { 141.0 });
            (2, wrap_view(road_bike_at(side, key, t) - run(t)))
        } else {
            // A roof chosen when the transfer began, in the nearer view then; it comes on with
            // the street after.
            let near = runic_constant("Z_NEAR");
            let m = ((run(t0) + near + 1.5 + pick * (runic_constant("Z_FAR") * runic_constant("DRIZZLE_FAR") - 2.0)) / GRID_Z).floor();
            (3, m * GRID_Z - run(t) + 0.5 * GRID_Z)
        };
        (z > runic_constant("Z_NEAR") + 0.4 && z < runic_constant("Z_FAR") * runic_constant("DRIZZLE_FAR")).then_some((kind, z))
    }

    #[test]
    fn the_sky_is_full_of_small_transfers_each_tied_to_a_walker_a_car_a_cyclist_or_a_roof() {
        // Cyrus, 2026-11-17, after seeing the city move: "there should be many small ones so it
        // feels like the matrix with everyones connected devices to the cloud"; then "the
        // uploading and downloading rain should be tied to people, cars, bicycles, or tops of
        // buildings. And it should be a little slower. It looks random to me."
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        let rain = &wash[wash.find("float3 data_rain(").unwrap()..];
        assert!(rain[..rain.find("\n}\n").unwrap()].contains("data_drizzle(p, top,"), "drawn with the city's data");
        let drizzle = &wash[wash.find("float3 data_drizzle(").unwrap()..];
        let drizzle = &drizzle[..drizzle.find("\n}\n").unwrap()];
        assert!(drizzle.contains("small_end(") && !wash.contains("drizzle_end("), "each follows a thing, not the ground under a column");
        assert!(drizzle.contains("floor(t / DATA_FLICKER)"), "its dashes change as slowly as the glyphs");
        assert!(runic_constant("DRIZZLE_W") <= 4.0, "small: a few points a column");
        assert!(runic_constant("DRIZZLE_LIGHT") <= 0.33, "many, so each is quiet");
        // Cyrus, 2026-11-17, in the sandbox: "make the rain slightly dimmer in the graphic".
        assert!(runic_constant("DATA_LIGHT") <= 0.82, "the big transfers a little dimmer too");
        assert!(rain[..rain.find("\n}\n").unwrap()].matches("DATA_LIGHT").count() >= 3, "their trails, glow and flicker all dimmed");
        // Slower: a small transfer takes a few seconds to cross the sky.
        assert!(runic_constant("DRIZZLE_PERIOD_MIN") * runic_constant("DRIZZLE_ON") >= 2.5);
        let n = wash_int("DRIZZLE_STREAMS");
        let tries = runic_constant("DRIZZLE_TRIES") as usize;
        let (mut sum, mut moments, mut least) = (0usize, 0usize, usize::MAX);
        let mut kinds = [0usize; 4];
        let mut t = 0.0f32;
        while t < 120.0 {
            let mut running = 0;
            for i in 0..n {
                let Some((_, seed, t0)) = small_slot(i, t) else { continue };
                let side = if hash2(seed, 6.0) < 0.5 { -1.0 } else { 1.0 };
                if let Some((kind, _)) = (0..tries).find_map(|k| small_end(seed * 4.0 + k as f32, side, t, t0)) {
                    running += 1;
                    kinds[kind] += 1;
                }
            }
            sum += running;
            least = least.min(running);
            moments += 1;
            t += 0.25;
        }
        let mean = sum as f32 / moments as f32;
        assert!(mean >= 20.0, "many at once: {mean} on average");
        assert!(least >= 10, "never a thin sky: {least}");
        assert!(kinds.iter().all(|k| *k > 0), "walkers, cars, cyclists and roofs all connected: {kinds:?}");
        // A roof is chosen when its transfer begins and then held: through a transfer it comes
        // on with the street, a steady pace, never a jump to another building.
        let mut seen_roof = false;
        let mut t = 0.0f32;
        while t < 60.0 && !seen_roof {
            for i in 0..n {
                let Some((_, seed, t0)) = small_slot(i, t) else { continue };
                if hash2(seed * 4.0, 2.0) < runic_constant("SMALL_BIKE") { continue }
                let Some((3, z0)) = small_end(seed * 4.0, 1.0, t, t0) else { continue };
                let Some((3, z1)) = small_end(seed * 4.0, 1.0, t + 0.1, t0) else { continue };
                let step = runic_constant("GRID_SPEED") * GRID_Z * 0.1;
                assert!((z0 - z1 - step).abs() < 1e-3, "slot {i} at {t}: roof {z0} then {z1}");
                seen_roof = true;
            }
            t += 0.5;
        }
        assert!(seen_roof);
    }

    #[test]
    fn the_big_transfers_end_on_a_roof_held_from_their_start_and_take_their_time() {
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        assert!(wash.contains("float4 data_end(float seed, float t, float t0)"), "the building is chosen when the transfer begins");
        assert!(wash.contains("return float4(tx, zc + 0.5 * GRID_Z, th, 1.0);") && !wash.contains("th * mix(0.3, 0.85, h2)"), "the top of the building, not a window");
        assert!(runic_constant("DATA_TRAVEL_MIN") >= 3.5 && runic_constant("DATA_TRAVEL_MAX") <= 5.0, "slower");
    }

    #[test]
    fn the_train_is_drawn_in_hidden_lines_as_under_the_street() {
        // Seen moving on 2026-11-17: crisp and solid, it read as a tram on the median. A drawing
        // shows what lies under a surface in dashed lines.
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        assert!(runic_constant("SUB_DIM") <= 0.32, "dimmer");
        assert!(runic_constant("SUB_DASH") > 0.0);
        let seen = &wash[wash.find("float3 subway_seen(").unwrap()..];
        let seen = &seen[..seen.find("\n}\n").unwrap()];
        // Its outline is dashed along the train, so the dashes travel with it.
        assert!(seen.contains("fract(along / SUB_DASH)"), "the outline dashed along the train");
    }

    /// The train as the wash's `subway_at` places it: its front's and back's depths, and whether
    /// it runs.
    fn subway_at(t: f32) -> (f32, f32, bool) {
        let (cars, car, gap) = (5.0f32, runic_constant("SUB_CAR"), runic_constant("SUB_GAP"));
        let (speed, period) = (runic_constant("SUB_SPEED"), runic_constant("SUB_PERIOD"));
        let (z_near, z_far) = (0.7f32, 30.0f32);
        let len = cars * car + (cars - 1.0) * gap;
        let k = (t / period).floor();
        let u = t - k * period;
        if u * speed > z_far + len - z_near + 2.0 {
            return (0.0, 0.0, false);
        }
        let coming = k % 2.0 < 0.5;
        let front = if coming { z_far + 1.0 - u * speed } else { z_near - 1.0 + u * speed };
        (front, if coming { front + len } else { front - len }, true)
    }

    #[test]
    fn the_subway_runs_underground_faster_than_the_traffic_and_never_stops() {
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        assert!(wash.contains("constant int SUB_CARS = 5;") && wash.contains("constant float Z_NEAR = 0.7;") && wash.contains("constant float Z_FAR = 30.0;"));
        // Underground: its roof below the street, the tunnel's roof above it, drawn dim and under
        // the street's grid.
        assert!(runic_constant("SUB_ROOF") > 0.0 && runic_constant("SUB_TRAIN_TOP") > runic_constant("SUB_ROOF") && runic_constant("SUB_FLOOR") > runic_constant("SUB_TRAIN_TOP"));
        assert!(runic_constant("SUB_DIM") <= 0.45, "dimmer than the street's lights");
        let scene = &wash[wash.find("float3 code_scene(").unwrap()..];
        assert!(scene.find("subway_seen(").unwrap() < scene.find("float lines = max(grid_line(").unwrap(), "the street's grid over it");
        // Faster: a road car averages a block (6.4) a signal cycle (12 s).
        assert!(runic_constant("SUB_SPEED") >= 4.0 * 6.4 / 12.0, "several times the cars' pace");
        // Never stopping: through every pass its front moves at its speed every moment.
        let mut t = 0.0f32;
        while t < 120.0 {
            let (a, _, on_a) = subway_at(t);
            let (b, _, on_b) = subway_at(t + 0.05);
            if on_a && on_b && (t / runic_constant("SUB_PERIOD")).floor() == ((t + 0.05) / runic_constant("SUB_PERIOD")).floor() {
                assert!(((b - a).abs() - runic_constant("SUB_SPEED") * 0.05).abs() < 1e-3, "at {t} it moved {}", (b - a).abs());
            }
            t += 0.05;
        }
        // It comes on and goes away in turn, and is seen every period.
        let (f0, b0, _) = subway_at(1.0);
        let (f1, b1, _) = subway_at(runic_constant("SUB_PERIOD") + 1.0);
        assert!(b0 > f0 && b1 < f1, "one pass comes on, the next goes away");
    }

}