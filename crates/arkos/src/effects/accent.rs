//! The runic effect's colours turned toward the theme's own: its greens were made around a
//! phosphor green, `HOME` (the matrix theme's focus ring until 2026-11-17), and under a theme each
//! is turned round the colour wheel by the distance from `HOME`'s hue to that theme's focus
//! ring's, keeping its brightness; and a ring less saturated than `HOME` softens them to match,
//! their saturation scaled by the ring's over `HOME`'s (never raised). Under navy the greens go
//! blue at full colour; under the matrix theme, whose ring is a soft green, they keep their hue and
//! lose some of their colour. A focus ring with too little colour to have a hue (under
//! `MIN_SATURATION`) turns nothing.
//!
//! The runic shaders (`effects/runic/wash.metal` and `effects/runic/glyphs.metal`) carry the
//! same arithmetic as `toward_accent`, and the decode turns its fresh colours with it; a change
//! to one is made to all three.

use crate::palette::Rgb;

/// The colour the runic effect's greens were made around: a phosphor green.
pub const HOME: Rgb = Rgb { r: 0x7e, g: 0xd6, b: 0x6b };
/// The least saturation a focus ring needs for its hue to turn anything.
pub const MIN_SATURATION: f32 = 0.2;

/// Hue (0 to 1, a full turn), saturation and value of a colour of channels 0 to 1, as the
/// shaders' `rgb_to_hsv`.
pub fn rgb_to_hsv([r, g, b]: [f32; 3]) -> [f32; 3] {
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    [h, if max > 0.0 { d / max } else { 0.0 }, max]
}

/// The colour of a hue, saturation and value, as the shaders' `hsv_to_rgb`.
pub fn hsv_to_rgb([h, s, v]: [f32; 3]) -> [f32; 3] {
    let k = |n: f32| {
        let k = (n + h * 6.0).rem_euclid(6.0);
        v - v * s * (k.min(4.0 - k).clamp(0.0, 1.0))
    };
    [k(5.0), k(3.0), k(1.0)]
}

/// `c` turned round the wheel by the distance from `HOME`'s hue to `accent`'s, its value kept and
/// its saturation scaled by `accent`'s over `HOME`'s when that is under 1; `c` itself when
/// `accent` has under `MIN_SATURATION`.
pub fn toward_accent(c: Rgb, accent: Rgb) -> Rgb {
    let f = |c: Rgb| [c.r, c.g, c.b].map(|v| f32::from(v) / 255.0);
    let a = rgb_to_hsv(f(accent));
    if a[1] < MIN_SATURATION {
        return c;
    }
    let mut h = rgb_to_hsv(f(c));
    let home = rgb_to_hsv(f(HOME));
    h[0] = (h[0] + a[0] - home[0] + 1.0).fract();
    h[1] *= (a[1] / home[1]).min(1.0);
    let [r, g, b] = hsv_to_rgb(h).map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8);
    Rgb { r, g, b }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Rgb, b: Rgb) -> bool {
        a.r.abs_diff(b.r) <= 1 && a.g.abs_diff(b.g) <= 1 && a.b.abs_diff(b.b) <= 1
    }

    /// The effect's greens: the glyphs' phosphor, deep and calm greens, and the decode's two
    /// fresh colours.
    fn greens() -> [Rgb; 5] {
        [
            Rgb { r: 112, g: 204, b: 125 },
            Rgb { r: 0, g: 102, b: 38 },
            Rgb { r: 184, g: 219, b: 184 },
            crate::effects::decode::FRESH_WHITE,
            crate::effects::decode::FRESH_DEEP,
        ]
    }

    #[test]
    fn under_home_itself_nothing_turns() {
        for c in greens() {
            assert!(close(toward_accent(c, HOME), c), "{c:?} turned to {:?}", toward_accent(c, HOME));
        }
    }

    #[test]
    fn under_the_matrix_theme_s_soft_ring_the_greens_keep_hue_and_light_and_lose_colour() {
        let ring = crate::theme::builtin("matrix").expect("matrix is built in").chrome.focus_ring;
        let f = |c: Rgb| rgb_to_hsv([c.r, c.g, c.b].map(|v| f32::from(v) / 255.0));
        let scale = f(ring)[1] / f(HOME)[1];
        assert!(scale < 0.8, "the matrix ring is softer than HOME ({scale})");
        for c in greens().into_iter().filter(|c| f(*c)[1] > 0.2) {
            let (was, now) = (f(c), f(toward_accent(c, ring)));
            assert!((now[2] - was[2]).abs() < 0.01, "{c:?} changed its light");
            assert!((now[1] - was[1] * scale).abs() < 0.02, "{c:?} saturation {} not {}", now[1], was[1] * scale);
            let turn = (now[0] - was[0] + 1.5).fract() - 0.5;
            assert!((turn - (f(ring)[0] - f(HOME)[0])).abs() < 0.02, "{c:?} turned {turn}");
        }
    }

    #[test]
    fn a_ring_as_saturated_as_home_keeps_every_green_s_colour() {
        let navy = crate::theme::builtin("navy").unwrap().chrome.focus_ring;
        let f = |c: Rgb| rgb_to_hsv([c.r, c.g, c.b].map(|v| f32::from(v) / 255.0));
        for c in greens() {
            assert!((f(toward_accent(c, navy))[1] - f(c)[1]).abs() < 0.02, "{c:?} lost colour under navy");
        }
    }

    #[test]
    fn under_navy_the_greens_turn_blue_and_keep_their_light() {
        let navy = crate::theme::builtin("navy").expect("navy is built in").chrome.focus_ring;
        for c in greens() {
            let t = toward_accent(c, navy);
            assert!(t.b >= t.g && t.b >= t.r, "{c:?} turned to {t:?}, not blue");
            let (max, min) = (|x: Rgb| x.r.max(x.g).max(x.b), |x: Rgb| x.r.min(x.g).min(x.b));
            assert!(max(t).abs_diff(max(c)) <= 1 && min(t).abs_diff(min(c)) <= 1, "{c:?} to {t:?} changed its light");
        }
    }

    #[test]
    fn a_grey_focus_ring_turns_nothing() {
        let grey = Rgb { r: 0x80, g: 0x84, b: 0x88 };
        for c in greens() {
            assert_eq!(toward_accent(c, grey), c);
        }
    }

    #[test]
    fn both_runic_shaders_turn_toward_the_accent_with_this_arithmetic() {
        let m = crate::effects::BUILTIN.iter().find(|b| b.name == "runic").expect("runic is built in");
        let (wash, glyphs) = (m.wash, m.glyphs.expect("runic has glyphs"));
        let home = format!("constant float3 HOME = {};", crate::effects::active::float3(HOME));
        let lines = [
            home.as_str(),
            &format!("constant float MIN_SATURATION = {MIN_SATURATION:?};"),
            "float3 toward_accent(float3 c, float3 accent) {",
            "  if (a.y < MIN_SATURATION) {",
            "  h.x = fract(h.x + a.x - rgb_to_hsv(HOME).x + 1.0);",
            "  h.y *= min(a.y / rgb_to_hsv(HOME).y, 1.0);",
        ];
        for line in lines {
            assert!(wash.contains(line), "wash.metal lacks {line}");
            assert!(glyphs.contains(line), "glyphs.metal lacks {line}");
        }
        // Every green the glyphs draw is turned, and the wash's green with the theme's ring.
        for line in ["toward_accent(RUNE_GREEN, ARKOS_ACCENT)", "toward_accent(RUNE_DEEP, ARKOS_ACCENT)", "toward_accent(CALM_GREEN, ARKOS_ACCENT)"] {
            assert!(glyphs.contains(line), "glyphs.metal lacks {line}");
        }
        assert!(wash.contains("toward_accent(theme_green(u), u.colors[ARKOS_FOCUS_RING].rgb)"));
    }
}
