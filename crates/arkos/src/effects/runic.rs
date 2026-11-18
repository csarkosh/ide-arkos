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
    const STOP_BACK: f32 = 0.4;
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

    const CLEAR_AFTER: f32 = 9.4;
    const JAYWALK: f32 = 0.6;

    fn road_bike_at(dir: f32, key: (f32, f32), t: f32) -> f32 {
        let block = CROSS_EVERY * GRID_Z;
        let h = |d: f32| hash2(key.0 + d, key.1 + d);
        let blocks = if h(5.0) < 0.5 { 2.0 } else { 1.0 };
        let speed = blocks * block / SIG_CYCLE;
        let phase = CLEAR_AFTER + h(2.0);
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
            "constant float STOP_BACK = 0.4;",
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
            "    float phase = CLEAR_AFTER + hash2(key + 2.0);",
            "    return stop0 + dir * speed * (t - phase);",
            "constant float CLEAR_AFTER = 9.4;",
            "constant float JAYWALK = 0.6;",
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
        let block = CROSS_EVERY * GRID_Z;
        let cross = GRID_Z + 2.0 * WALK_AT;
        let h = |d: f32| hash2(key.0 + d, key.1 + d);
        let brisk = h(2.0) < 0.65;
        let v = if brisk { 0.54 + 0.06 * h(6.0) } else { 0.31 + 0.05 * h(6.0) };
        let laps = if brisk { 1.0 } else { 2.0 };
        let period = laps * SIG_CYCLE;
        let late = (ROAD_GREEN - AMBER).min(ROAD_GREEN - cross / v);
        let jay = h(8.0) < JAYWALK;
        let c = (h(3.0) * laps).floor() * SIG_CYCLE + if jay { CLEAR_AFTER + 0.8 * h(4.0) } else { late * h(4.0) };
        let k = ((t - c) / period).floor();
        let x = (v * (t - c - k * period)).min(block);
        let j = (h(5.0) * 8.0).floor() + dir * k;
        let corner = if dir > 0.0 { j * block - WALK_AT } else { j * block + GRID_Z + WALK_AT };
        (corner + dir * x, x < block)
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
        let mut t = 0.0f32;
        while t < 360.0 {
            let sig = t - (t / SIG_CYCLE).floor() * SIG_CYCLE;
            for (lane, dir) in [(0.0f32, -1.0f32), (1.0, 1.0)] {
                for j in 0..3 {
                    let key = (j as f32, lane * 13.0 + 141.0);
                    let w = road_bike_at(dir, key, t);
                    let x = dir * 0.78 * 0.9;
                    if let Some(dl) = lane_of(w) {
                        assert!(!cross_car_hits(x, dl, sig), "cyclist {j} of lane {lane} hit at {t} ({sig} into the cycle)");
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
                    let x = out * (0.96 + (hash2(key.0 + 7.0, key.1 + 7.0) - 0.3) * WALK_W);
                    if let Some(dl) = lane_of(w) {
                        assert!(!cross_car_hits(x, dl, sig), "walker {key:?} hit at {t} ({sig} into the cycle)");
                        if sig >= ROAD_GREEN {
                            jaywalks += 1;
                        }
                    }
                }
            }
            t += 0.05;
        }
        assert!(red_runs > 100 && jaywalks > 1000, "they do break the law: {red_runs} {jaywalks}");
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
        assert!(most_standing * 4 <= PEOPLE + STROLLERS, "{most_standing} people standing at once");
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
            "    float c = floor(hash2(key + 3.0) * laps) * SIG_CYCLE + (jay ? CLEAR_AFTER + 0.8 * hash2(key + 4.0) : late * hash2(key + 4.0));",
            "    bool jay = hash2(key + 8.0) < JAYWALK;",
            "    float x = min(v * (t - c - k * period), block);",
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
        // The road's walkers: edge_x plus (h - 0.3) * WALK_W, h from 0 to 1; its strollers: edge_x
        // plus (0.1 + 0.6 h) * WALK_W.
        let edge_x = road_e + 0.06;
        for h in [0.0f32, 0.5, 1.0] {
            for x in [edge_x + (h - 0.3) * WALK_W, edge_x + (0.1 + 0.6 * h) * WALK_W] {
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

    /// The wash's `data_drizzle` slot for small column `ci` at `t`: how far through its run it is
    /// (0 to 1) and its seed, or nothing while it rests.
    fn drizzle_slot(ci: f32, t: f32) -> Option<(f32, f32)> {
        let (lo, hi) = (runic_constant("DRIZZLE_PERIOD_MIN"), runic_constant("DRIZZLE_PERIOD_MAX"));
        let period = lo + (hi - lo) * hash2(ci, 41.0);
        let off = hash2(ci, 43.0) * period;
        let k = ((t + off) / period).floor();
        let into = t + off - k * period;
        let run = period * runic_constant("DRIZZLE_ON");
        (into <= run).then_some((into / run, ci * 389.0 + k))
    }

    /// The wash's `drizzle_end`: where small column `ci` of a scene with focal length `f` lands for
    /// `seed`, as the ground line under it (across, world), its depth and its kind (0 a sidewalk, 1
    /// a bike lane, 2 a car lane), or nothing when that line is out of view under it.
    fn drizzle_end(ci: f32, seed: f32, f: f32) -> Option<(f32, f32, usize)> {
        let dx = ((ci + 0.5) * runic_constant("DRIZZLE_W")).abs();
        let pick = hash2(seed, 2.0);
        let road = runic_constant("GRID_X");
        let (wx, kind) = if pick < runic_constant("DRIZZLE_WALK") {
            (road + 0.06 + (hash2(seed, 5.0) - 0.3) * runic_constant("WALK_W"), 0)
        } else if pick < runic_constant("DRIZZLE_BIKE") {
            (runic_constant("BIKE_LANE") * road, 1)
        } else {
            (0.5 * road, 2)
        };
        let z = wx * f / dx.max(1e-3);
        (z >= runic_constant("Z_NEAR") + 0.4 && z <= runic_constant("Z_FAR") * 0.6).then_some((wx, z, kind))
    }

    #[test]
    fn the_sky_is_full_of_small_transfers_to_the_devices_on_the_ground() {
        // Cyrus, 2026-11-17, after seeing the city move: "there's not enough runic rain yet. there
        // should be many small ones so it feels like the matrix with everyones connected devices to
        // the cloud".
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").unwrap().wash;
        let rain = &wash[wash.find("float3 data_rain(").unwrap()..];
        assert!(rain[..rain.find("\n}\n").unwrap()].contains("data_drizzle(p, top,"), "drawn with the city's data");
        assert!(runic_constant("DRIZZLE_W") <= 4.0, "small: a few points a column");
        assert!(runic_constant("DRIZZLE_LIGHT") <= 0.45, "many, so each is quiet");
        // Cyrus, 2026-11-17, in the sandbox: "make the rain slightly dimmer in the graphic".
        assert!(runic_constant("DRIZZLE_LIGHT") <= 0.33 && runic_constant("DATA_LIGHT") <= 0.82, "a little dimmer, both layers");
        let rain = &wash[wash.find("float3 data_rain(").unwrap()..];
        let rain = &rain[..rain.find("\n}\n").unwrap()];
        assert!(rain.matches("DATA_LIGHT").count() >= 3, "the trails, the glow and the flicker on the thing all dimmed");
        let drizzle = &wash[wash.find("float3 data_drizzle(").unwrap()..];
        assert!(drizzle[..drizzle.find("\n}\n").unwrap()].contains("floor(t / DATA_FLICKER)"), "its dashes change as slowly as the glyphs");
        // ROAD_CELLS is 1 in the wash, which the mirror takes as read.
        assert!(wash.contains("constant int ROAD_CELLS = 1;"));
        // A sidebar of 260 points: the scene 256 wide, its focal length 0.9 of that.
        let (width, f) = (256.0f32, 0.9 * 256.0);
        let half = (width / 2.0 / runic_constant("DRIZZLE_W")).floor() as i32;
        let (mut sum, mut moments, mut least) = (0usize, 0usize, usize::MAX);
        let mut kinds = [0usize; 3];
        let mut t = 0.0f32;
        while t < 120.0 {
            let mut running = 0;
            for ci in -half..half {
                let ci = ci as f32;
                let Some((_, seed)) = drizzle_slot(ci, t) else { continue };
                let Some((wx, z, kind)) = drizzle_end(ci, seed, f) else { continue };
                // It lands under its own column: the line's screen x there is the column's.
                let cx = (ci + 0.5) * runic_constant("DRIZZLE_W");
                assert!((cx.signum() * wx * f / z - cx).abs() < 1e-2, "column {ci} lands at {}", wx * f / z);
                running += 1;
                kinds[kind] += 1;
            }
            sum += running;
            least = least.min(running);
            moments += 1;
            t += 0.25;
        }
        let mean = sum as f32 / moments as f32;
        assert!(mean >= 25.0, "many at once: {mean} on average");
        assert!(least >= 12, "never a thin sky: {least}");
        assert!(kinds.iter().all(|k| *k > 0), "walkers, cyclists and cars all connected: {kinds:?}");
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