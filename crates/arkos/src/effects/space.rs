//! The built-in `space` effect's event schedule, mirrored from its wash shader
//! (`effects/space/wash.metal`) so the timings can be tested without a GPU. The constants and the
//! arithmetic here are the shader's own; a change to one is made to both.

/// Seconds from one shooting star's cycle to the next; each starts up to `METEOR_SLACK` into its
/// cycle, so the gap between two is 20 to 40 seconds.
pub const METEOR_CYCLE: f32 = 30.0;
pub const METEOR_SLACK: f32 = 10.0;
/// How long a shooting star takes to cross.
pub const METEOR_FLIGHT: f32 = 0.8;
/// Where the clock's zero falls in the shooting stars' schedule: past the latest a first one
/// could have finished, so a frozen clock shows none mid-flight.
pub const METEOR_OFFSET: f32 = 15.0;

/// A salt that gives the shooting stars hashes of their own.
pub const METEOR_SALT: u32 = 0x9E37_79B9;

/// A hash of `n` in 0 to 1, in integer arithmetic (PCG's output step) so the shader and this
/// agree to the bit: the top 24 bits, times a power of two, are exact in a float.
pub fn hash01(n: u32) -> f32 {
    let mut h = n.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    h = ((h >> ((h >> 28) + 4)) ^ h).wrapping_mul(277_803_737);
    h ^= h >> 22;
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

/// Whether a shooting star is crossing at `t` seconds of the effect's clock.
pub fn meteor_visible(t: f32) -> bool {
    let tau = t + METEOR_OFFSET;
    let n = (tau / METEOR_CYCLE).floor();
    let x = tau - n * METEOR_CYCLE - METEOR_SLACK * hash01((n as u32) ^ METEOR_SALT);
    (0.0..METEOR_FLIGHT).contains(&x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::BUILTIN;

    fn space() -> &'static crate::effects::BuiltIn {
        BUILTIN.iter().find(|b| b.name == "space").expect("space is built in")
    }

    /// The runs of `t` from 0 to `end` in steps of `dt` where `on` holds, as (start, end) pairs.
    fn runs(end: f32, dt: f32, on: impl Fn(f32) -> bool) -> Vec<(f32, f32)> {
        let mut out = Vec::new();
        let mut began: Option<f32> = None;
        let steps = (end / dt) as u32;
        for i in 0..=steps {
            let t = i as f32 * dt;
            match (on(t), began) {
                (true, None) => began = Some(t),
                (false, Some(b)) => {
                    out.push((b, t));
                    began = None;
                }
                _ => {}
            }
        }
        out
    }

    #[test]
    fn a_frozen_clock_shows_no_shooting_star_mid_flight_and_they_come_every_20_to_40_seconds() {
        assert!(!meteor_visible(0.0));

        // A shooting star every 20 to 40 seconds, each across in under a second.
        let dt = 0.01;
        let stars = runs(3600.0, dt, meteor_visible);
        assert!(stars.len() >= 90, "{}", stars.len());
        for w in stars.windows(2) {
            let gap = w[1].0 - w[0].0;
            assert!((20.0 - dt..=40.0 + dt).contains(&gap), "meteor gap {gap}");
        }
        for &(a, b) in &stars {
            assert!(b - a < 1.0, "a crossing of {} s", b - a);
        }
    }

    #[test]
    fn the_wash_carries_the_schedule_s_constants_and_names_this_mirror() {
        let wash = space().wash;
        assert!(wash.contains("effects/space.rs"), "the shader names its mirror");
        assert!(!wash.to_lowercase().contains("aurora_"), "the aurora is gone");
        for line in [
            "constant float METEOR_CYCLE = 30.0;",
            "constant float METEOR_SLACK = 10.0;",
            "constant float METEOR_FLIGHT = 0.8;",
            "constant float METEOR_OFFSET = 15.0;",
            "constant uint METEOR_SALT = 0x9E3779B9u;",
        ] {
            assert!(wash.contains(line), "wash.metal lacks {line}");
        }
    }

    #[test]
    fn the_holo_text_is_measured_in_points_is_still_and_keeps_dark_text_readable() {
        let glyphs = space().glyphs.expect("space draws its own glyphs");
        assert!(glyphs.contains("arkos_glyph_vertex(") && glyphs.contains("arkos_glyph_fragment("));
        // Points through the window's scale, and no clock: the text never moves.
        assert!(glyphs.contains("ARKOS_SCALE"));
        assert!(!glyphs.contains("[[buffer(5)]]"), "the holo text reads no clock");
        // Cyrus's values, verbatim.
        for line in [
            "constant float BRIGHTNESS = 0.80;",
            "constant float TINT = 0.25;",
            "constant float KEEP = 0.15;",
            "constant float SCANLINES = 0.08;",
            "constant float GLOW = 0.90;",
            "constant float REACH = 5.5;",
        ] {
            assert!(glyphs.contains(line), "glyphs.metal lacks {line}");
        }
        // Dark text (a light theme with space) tints toward deep teal, undimmed and unglowing.
        assert!(glyphs.contains("bool dark = luminance < 0.5;"));
        assert!(glyphs.contains("float3(0.0, 0.35, 0.45)"));
        // The stroke's core keeps at least 55% of its coverage.
        assert!(glyphs.contains("core * 0.55"));
    }
}
