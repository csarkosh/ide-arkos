//! The built-in `matrix` effect's rain, mirrored from its wash shader (`effects/matrix/wash.metal`)
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
/// Seconds a column waits between its streams, beyond their fall: about a third of the columns
/// then carry a stream on screen at any moment in a window 800 points tall.
pub const RAIN_GAP: f32 = 30.0;
/// The rows a stream falls through in its cycle before its trail, whatever the window's height,
/// so a resize never moves a column's phase.
pub const RAIN_ROWS_PERIOD: f32 = 60.0;
/// Seconds a rune shows before its cell picks another.
pub const RUNE_CHANGE: f32 = 0.5;
/// The most the rain lays of its green over the ground, before the user's intensity.
pub const RAIN_ALPHA: f32 = 0.12;

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

    fn matrix() -> &'static crate::effects::BuiltIn {
        BUILTIN.iter().find(|b| b.name == "matrix").expect("matrix is built in")
    }

    #[test]
    fn the_wash_carries_the_rain_s_constants_and_names_this_mirror() {
        let wash = matrix().wash;
        assert!(wash.contains("effects/matrix.rs"), "the shader names its mirror");
        for (line, value) in [
            ("constant float RAIN_COL = 14.0;", RAIN_COL),
            ("constant float RAIN_ROW = 18.0;", RAIN_ROW),
            ("constant float RAIN_SPEED_MIN = 2.0;", RAIN_SPEED_MIN),
            ("constant float RAIN_SPEED_MAX = 6.0;", RAIN_SPEED_MAX),
            ("constant float RAIN_TRAIL_MIN = 8.0;", RAIN_TRAIL_MIN),
            ("constant float RAIN_TRAIL_MAX = 24.0;", RAIN_TRAIL_MAX),
            ("constant float RAIN_GAP = 30.0;", RAIN_GAP),
            ("constant float RAIN_ROWS_PERIOD = 60.0;", RAIN_ROWS_PERIOD),
            ("constant float RUNE_CHANGE = 0.5;", RUNE_CHANGE),
            ("constant float RAIN_ALPHA = 0.12;", RAIN_ALPHA),
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
    fn both_shaders_hash_with_the_rust_hash01_line_for_line() {
        // Each shader carries its own copy of `hash01` (crates/arkos/src/effects/hash.rs), so a
        // hand-copied drift in either shows here.
        for (file, src) in [("wash.metal", matrix().wash), ("glyphs.metal", matrix().glyphs.unwrap())] {
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
        let wash = matrix().wash;
        let start = wash.find("float4 rain(").expect("the wash has its rain");
        let end = start + wash[start..].find("\n}\n").unwrap();
        let rain = &wash[start..end];
        // The rain is given no window size, so a column's phase is the same at every height.
        assert!(rain.starts_with("float4 rain(float2 p, float t, float3 green)"), "{}", &rain[..60]);
        assert!(!rain.contains("u.size") && !rain.contains("ceil("), "the rain reads the window's height");
    }

    #[test]
    fn about_a_third_of_the_columns_carry_a_stream_on_screen_at_any_moment() {
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
            assert!((0.25..=0.42).contains(&share), "at {height} points {share:.3} of the columns carry a stream");
            if height == 800.0 {
                assert!((share - 1.0 / 3.0).abs() < 0.04, "at 800 points {share:.3}, not about a third");
            }
        }
        // A frozen clock still shows rain.
        assert!((0..200).any(|c| on_screen(c, 0.0, 800.0)));
    }

    #[test]
    fn the_menus_edges_are_drawn_with_the_panes_own_code() {
        let wash = matrix().wash;
        let start = wash.find("fragment float4 arkos_overlay(").expect("matrix draws the menus' edges");
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
        let g = matrix().glyphs.unwrap();
        for line in [
            "constant float GLITCH_SLOT = 0.1;",
            "constant float GLITCH_CHANCE = 0.0001;",
            "constant float GLITCH_FLASH = 1.4;",
            "constant float TINT = 0.45;",
            "constant float KEEP = 0.15;",
            "constant float GLOW = 1.6;",
            "constant float BRIGHTNESS = 1.0;",
            "float3(0.35, 1.0, 0.45)",
            "float3(0.0, 0.4, 0.15)",
            "bool dark = luminance < 0.45;",
            "core * 0.55",
            "ARKOS_SCALE",
            "[[buffer(5)]]",
            "effects/matrix.rs",
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
        assert!(!g.contains("SCANLINES"), "no scanlines on the matrix display");
        let emoji = &g[g.find("arkos_emoji_vertex(").expect("matrix draws its emoji")..];
        assert!(emoji.contains("arkos_emoji_fragment(") && emoji.contains("GLITCH_") && emoji.contains("[[buffer(5)]]"));
    }
}
