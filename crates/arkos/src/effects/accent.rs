//! The matrix effect's colours turned toward the theme's own: its greens were made around the
//! matrix theme's focus ring, `HOME`, and under another theme each is turned round the colour
//! wheel by the distance from `HOME`'s hue to that theme's focus ring's, keeping its saturation
//! and its brightness. Under the matrix theme nothing turns; under navy the greens go blue. A
//! focus ring with too little colour to have a hue (under `MIN_SATURATION`) turns nothing.
//!
//! The matrix shaders (`effects/matrix/wash.metal` and `effects/matrix/glyphs.metal`) carry the
//! same arithmetic as `toward_accent`, and the decode turns its fresh colours with it; a change
//! to one is made to all three.

use crate::palette::Rgb;

/// The colour the matrix effect's greens were made around: the matrix theme's focus ring.
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

/// `c` turned round the wheel by the distance from `HOME`'s hue to `accent`'s, its saturation
/// and value kept; `c` itself when `accent` has under `MIN_SATURATION`.
pub fn toward_accent(c: Rgb, accent: Rgb) -> Rgb {
    let f = |c: Rgb| [c.r, c.g, c.b].map(|v| f32::from(v) / 255.0);
    let a = rgb_to_hsv(f(accent));
    if a[1] < MIN_SATURATION {
        return c;
    }
    let mut h = rgb_to_hsv(f(c));
    h[0] = (h[0] + a[0] - rgb_to_hsv(f(HOME))[0] + 1.0).fract();
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
    fn under_the_matrix_theme_nothing_turns() {
        let matrix = crate::theme::builtin("matrix").expect("matrix is built in");
        assert_eq!(matrix.chrome.focus_ring, HOME, "HOME is the matrix theme's focus ring");
        for c in greens() {
            assert!(close(toward_accent(c, HOME), c), "{c:?} turned to {:?}", toward_accent(c, HOME));
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
    fn both_matrix_shaders_turn_toward_the_accent_with_this_arithmetic() {
        let m = crate::effects::BUILTIN.iter().find(|b| b.name == "matrix").expect("matrix is built in");
        let (wash, glyphs) = (m.wash, m.glyphs.expect("matrix has glyphs"));
        let home = format!("constant float3 HOME = {};", crate::effects::active::float3(HOME));
        let lines = [
            home.as_str(),
            &format!("constant float MIN_SATURATION = {MIN_SATURATION:?};"),
            "float3 toward_accent(float3 c, float3 accent) {",
            "  if (a.y < MIN_SATURATION) {",
            "  h.x = fract(h.x + a.x - rgb_to_hsv(HOME).x + 1.0);",
        ];
        for line in lines {
            assert!(wash.contains(line), "wash.metal lacks {line}");
            assert!(glyphs.contains(line), "glyphs.metal lacks {line}");
        }
        // Every green the glyphs draw is turned, and the wash's green with the theme's ring.
        for line in ["toward_accent(MATRIX_GREEN, ARKOS_ACCENT)", "toward_accent(MATRIX_DEEP, ARKOS_ACCENT)", "toward_accent(CALM_GREEN, ARKOS_ACCENT)"] {
            assert!(glyphs.contains(line), "glyphs.metal lacks {line}");
        }
        assert!(wash.contains("toward_accent(theme_green(u), u.colors[ARKOS_FOCUS_RING].rgb)"));
    }
}
