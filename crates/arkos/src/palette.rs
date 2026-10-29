//! Colours and the terminal palette. The window draws from the active theme (`theme.rs`);
//! `NAVY` is milestone 1's palette, kept as the default a `TerminalCore` starts with (the
//! window gives every core the theme's palette) and as the unit tests' reference. Values follow
//! spec 7.1: a lifted navy background, a bright grey foreground, and ANSI colours that each
//! clear 4.5:1 luminance contrast on the background.
use alacritty_terminal::vte::ansi::{Color, NamedColor};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

pub const fn rgb(hex: u32) -> Rgb {
    Rgb { r: ((hex >> 16) & 0xff) as u8, g: ((hex >> 8) & 0xff) as u8, b: (hex & 0xff) as u8 }
}

impl Rgb {
    /// `#rrggbb`, either case. Anything else is an error naming the text.
    pub fn parse(s: &str) -> Result<Rgb, String> {
        let hex = s.strip_prefix('#').ok_or_else(|| format!("colour {s:?} must look like #rrggbb"))?;
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("colour {s:?} must look like #rrggbb"));
        }
        let v = u32::from_str_radix(hex, 16).map_err(|_| format!("colour {s:?} must look like #rrggbb"))?;
        Ok(rgb(v))
    }

    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    pub fn gpui(self) -> gpui::Rgba {
        gpui::Rgba { r: self.r as f32 / 255.0, g: self.g as f32 / 255.0, b: self.b as f32 / 255.0, a: 1.0 }
    }

    pub fn hsla(self) -> gpui::Hsla {
        self.gpui().into()
    }

    pub fn alpha(self, a: f32) -> gpui::Hsla {
        let mut c = self.hsla();
        c.a = a;
        c
    }
}

impl serde::Serialize for Rgb {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.hex())
    }
}

impl<'de> serde::Deserialize<'de> for Rgb {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Rgb::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// WCAG relative luminance, 0 (black) to 1 (white).
pub fn luminance(c: Rgb) -> f64 {
    fn ch(v: u8) -> f64 {
        let v = v as f64 / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    }
    0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
}

/// WCAG contrast ratio, 1 to 21.
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (l1, l2) = (luminance(a), luminance(b));
    let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}

fn to_linear(v: u8) -> f32 {
    let v = v as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn from_linear(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

/// A colour as Oklab [L, a, b] (Bjorn Ottosson's matrices).
pub fn to_oklab(c: Rgb) -> [f32; 3] {
    let (r, g, b) = (to_linear(c.r), to_linear(c.g), to_linear(c.b));
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

fn from_oklab(lab: [f32; 3]) -> Rgb {
    let [ll, a, b] = lab;
    let l = (ll + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let m = (ll - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let s = (ll - 0.0894841775 * a - 1.2914855480 * b).powi(3);
    Rgb {
        r: from_linear(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
        g: from_linear(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
        b: from_linear(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s),
    }
}

#[cfg(test)]
fn oklab_dist(a: Rgb, b: Rgb) -> f32 {
    let (a, b) = (to_oklab(a), to_oklab(b));
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Blends two colours in Oklab, so the blend keeps even lightness. `t` of 0 gives `a`, 1 gives `b`.
pub fn oklab_mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let (a, b) = (to_oklab(a), to_oklab(b));
    from_oklab([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t])
}

/// Colours 16 to 255 derived from a palette's background, foreground and six hues. Filled on
/// first use and left out of equality; a clone starts empty so it derives from its own colours.
#[derive(Debug, Default)]
pub struct Derived(std::sync::OnceLock<Box<[Rgb; 240]>>);

impl Clone for Derived {
    fn clone(&self) -> Self {
        Derived::default()
    }
}

impl PartialEq for Derived {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Derived {}

/// A terminal palette. The 256-colour cube and grey ramp are derived from `bg`, `fg` and `ansi`
/// the first time `indexed` needs them and cached. Those three must not be changed after that
/// first call; build a new palette instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub bg: Rgb,
    pub fg: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
    pub find_match: Rgb,
    pub find_current: Rgb,
    pub ansi: [Rgb; 16],
    pub derived: Derived,
}

pub const NAVY: Palette = Palette {
    bg: rgb(0x11161f),
    fg: rgb(0xd6dae0),
    cursor: rgb(0xd6dae0),
    selection: rgb(0x2a3a55),
    find_match: rgb(0x3a4556),
    find_current: rgb(0x6b5420),
    ansi: [
        rgb(0x1b2230), // black
        rgb(0xf08c8c), // red
        rgb(0x8fd9a8), // green
        rgb(0xe8c97a), // yellow
        rgb(0x8ab8f0), // blue
        rgb(0xc9a4e8), // magenta
        rgb(0x7fd4d4), // cyan
        rgb(0xc8ccd4), // white
        rgb(0x7c8798), // bright black: used for dimmed text, so it must stay readable
        rgb(0xf7a8a8), // bright red
        rgb(0xa8e6bd), // bright green
        rgb(0xf0d898), // bright yellow
        rgb(0xa8ccf5), // bright blue
        rgb(0xd9bcf0), // bright magenta
        rgb(0x9ee2e2), // bright cyan
        rgb(0xe8ebf0), // bright white
    ],
    derived: Derived(std::sync::OnceLock::new()),
};

impl Palette {
    pub fn indexed(&self, i: u8) -> Rgb {
        match i {
            0..=15 => self.ansi[i as usize],
            _ => self.derived.0.get_or_init(|| Box::new(self.derive_256()))[i as usize - 16],
        }
    }

    /// The 6x6x6 cube as a trilinear blend in Oklab of eight corners (the background, the
    /// foreground and the theme's six hues), then a 24-step grey ramp strictly between them.
    fn derive_256(&self) -> [Rgb; 240] {
        let hue = |i: usize| to_oklab(self.ansi[i]);
        let (k, w) = (to_oklab(self.bg), to_oklab(self.fg));
        // Corner at (r, g, b) in {0,1}: black, blue, green, cyan, red, magenta, yellow, white.
        let corner = |r: usize, g: usize, b: usize| match (r, g, b) {
            (0, 0, 0) => k,
            (0, 0, 1) => hue(4),
            (0, 1, 0) => hue(2),
            (0, 1, 1) => hue(6),
            (1, 0, 0) => hue(1),
            (1, 0, 1) => hue(5),
            (1, 1, 0) => hue(3),
            _ => w,
        };
        let mut out = [self.bg; 240];
        for i in 0..216usize {
            let t = [(i / 36) as f32 / 5.0, ((i % 36) / 6) as f32 / 5.0, (i % 6) as f32 / 5.0];
            let mut lab = [0.0f32; 3];
            for c in 0..8usize {
                let (r, g, b) = (c >> 2, (c >> 1) & 1, c & 1);
                let wt = [r, g, b].iter().zip(t).map(|(&bit, t)| if bit == 1 { t } else { 1.0 - t }).product::<f32>();
                let v = corner(r, g, b);
                for ch in 0..3 {
                    lab[ch] += wt * v[ch];
                }
            }
            out[i] = from_oklab(lab);
        }
        for step in 0..24usize {
            out[216 + step] = oklab_mix(self.bg, self.fg, (step + 1) as f32 / 25.0);
        }
        out
    }

    pub fn resolve(&self, c: &Color) -> Rgb {
        match c {
            Color::Spec(c) => Rgb { r: c.r, g: c.g, b: c.b },
            Color::Indexed(i) => self.indexed(*i),
            Color::Named(n) => match n {
                NamedColor::Foreground | NamedColor::BrightForeground => self.fg,
                NamedColor::Background => self.bg,
                NamedColor::Cursor => self.cursor,
                NamedColor::DimForeground => self.ansi[7],
                NamedColor::Black | NamedColor::DimBlack => self.ansi[0],
                NamedColor::Red | NamedColor::DimRed => self.ansi[1],
                NamedColor::Green | NamedColor::DimGreen => self.ansi[2],
                NamedColor::Yellow | NamedColor::DimYellow => self.ansi[3],
                NamedColor::Blue | NamedColor::DimBlue => self.ansi[4],
                NamedColor::Magenta | NamedColor::DimMagenta => self.ansi[5],
                NamedColor::Cyan | NamedColor::DimCyan => self.ansi[6],
                NamedColor::White | NamedColor::DimWhite => self.ansi[7],
                NamedColor::BrightBlack => self.ansi[8],
                NamedColor::BrightRed => self.ansi[9],
                NamedColor::BrightGreen => self.ansi[10],
                NamedColor::BrightYellow => self.ansi[11],
                NamedColor::BrightBlue => self.ansi[12],
                NamedColor::BrightMagenta => self.ansi[13],
                NamedColor::BrightCyan => self.ansi[14],
                NamedColor::BrightWhite => self.ansi[15],
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreground_is_bright_not_dim() {
        let c = contrast(NAVY.fg, NAVY.bg);
        assert!(c >= 11.0, "fg/bg contrast {c:.1} should be about 12:1");
    }

    #[test]
    fn every_ansi_colour_except_black_clears_4_5_to_1() {
        for (i, c) in NAVY.ansi.iter().enumerate() {
            if i == 0 {
                continue; // black is a background by convention
            }
            let r = contrast(*c, NAVY.bg);
            assert!(r >= 4.5, "ansi {i} has contrast {r:.2}");
        }
    }

    fn themes() -> Vec<Palette> {
        vec![NAVY, crate::theme::builtin("navy").unwrap().palette(), crate::theme::builtin("peach-light").unwrap().palette()]
    }

    fn l_of(c: Rgb) -> f32 {
        to_oklab(c)[0]
    }

    #[test]
    fn the_cube_s_corners_are_the_theme_s_own_colours() {
        for p in themes() {
            let near = |a: Rgb, b: Rgb| {
                let d = |x: u8, y: u8| (x as i32 - y as i32).abs();
                d(a.r, b.r) <= 1 && d(a.g, b.g) <= 1 && d(a.b, b.b) <= 1
            };
            assert!(near(p.indexed(16), p.bg), "000 is the background");
            assert!(near(p.indexed(231), p.fg), "555 is the foreground");
            assert!(near(p.indexed(196), p.ansi[1]), "500 is red");
            assert!(near(p.indexed(46), p.ansi[2]), "050 is green");
            assert!(near(p.indexed(21), p.ansi[4]), "005 is blue");
            assert!(near(p.indexed(226), p.ansi[3]), "550 is yellow");
            assert!(near(p.indexed(201), p.ansi[5]), "505 is magenta");
            assert!(near(p.indexed(51), p.ansi[6]), "055 is cyan");
            assert_eq!(p.indexed(1), p.ansi[1]);
        }
    }

    #[test]
    fn the_cube_s_low_steps_are_tints_of_the_background_toward_their_hue() {
        for p in themes() {
            let (idx, green) = (22u8, p.ansi[2]); // 0,1,0
            let c = p.indexed(idx);
            let (dc, dg) = (oklab_dist(c, p.bg), oklab_dist(c, green));
            assert!(dc > 0.0, "not the background itself");
            assert!(dc < dg, "index 22 is nearer the background ({dc:.3}) than green ({dg:.3})");
        }
    }

    #[test]
    fn the_grey_ramp_runs_evenly_from_just_off_the_background_to_just_short_of_the_foreground() {
        for p in themes() {
            assert_ne!(p.indexed(232), p.bg);
            assert_ne!(p.indexed(255), p.fg);
            let dir = (l_of(p.fg) - l_of(p.bg)).signum();
            let mut prev = l_of(p.bg);
            for i in 232..=255u8 {
                let l = l_of(p.indexed(i));
                assert!((l - prev) * dir > 0.0, "step {i} moves from the background toward the foreground");
                prev = l;
            }
            assert!((l_of(p.fg) - prev) * dir > 0.0);
            let d: Vec<f32> = (232..255u8).map(|i| oklab_dist(p.indexed(i), p.indexed(i + 1))).collect();
            let mean = d.iter().sum::<f32>() / d.len() as f32;
            for (n, x) in d.iter().enumerate() {
                assert!((x - mean).abs() < 0.004, "step {} is {x:.4} against a mean of {mean:.4}", 232 + n);
            }
        }
    }

    #[test]
    fn a_cloned_palette_with_a_new_background_derives_its_own_cube() {
        let p = NAVY;
        let _ = p.indexed(100);
        let mut q = p.clone();
        q.bg = rgb(0xfff4e8);
        assert_ne!(q.indexed(16), p.indexed(16));
    }

    #[test]
    fn resolves_named_spec_and_indexed() {
        assert_eq!(NAVY.resolve(&Color::Named(NamedColor::Foreground)), NAVY.fg);
        assert_eq!(NAVY.resolve(&Color::Named(NamedColor::Background)), NAVY.bg);
        assert_eq!(NAVY.resolve(&Color::Named(NamedColor::Red)), NAVY.ansi[1]);
        assert_eq!(NAVY.resolve(&Color::Named(NamedColor::BrightBlue)), NAVY.ansi[12]);
        assert_eq!(NAVY.resolve(&Color::Spec(alacritty_terminal::vte::ansi::Rgb { r: 1, g: 2, b: 3 })), Rgb { r: 1, g: 2, b: 3 });
        assert_eq!(NAVY.resolve(&Color::Indexed(196)), NAVY.indexed(196));
        assert_eq!(NAVY.resolve(&Color::Indexed(5)), NAVY.ansi[5]);
    }

    #[test]
    fn rgb_parses_and_prints_hex() {
        assert_eq!(Rgb::parse("#11161f"), Ok(rgb(0x11161f)));
        assert_eq!(Rgb::parse("#D6DAE0"), Ok(rgb(0xd6dae0)));
        assert!(Rgb::parse("11161f").is_err(), "the hash is required");
        assert!(Rgb::parse("#11161").is_err());
        assert!(Rgb::parse("#gg161f").is_err());
        assert_eq!(rgb(0x11161f).hex(), "#11161f");
        let json = serde_json::to_string(&rgb(0x0a0e14)).unwrap();
        assert_eq!(json, "\"#0a0e14\"");
        assert_eq!(serde_json::from_str::<Rgb>("\"#0A0E14\"").unwrap(), rgb(0x0a0e14));
        assert!(serde_json::from_str::<Rgb>("\"navy\"").is_err());
        let c = rgb(0xff0000).gpui();
        assert!((c.r - 1.0).abs() < 1e-6 && c.g == 0.0 && c.b == 0.0 && c.a == 1.0);
        assert!((rgb(0xff0000).alpha(0.5).a - 0.5).abs() < 1e-6);
    }
}
