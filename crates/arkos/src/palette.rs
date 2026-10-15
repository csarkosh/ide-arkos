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
#[cfg_attr(not(test), allow(dead_code))] // Only the contrast tests here, in theme.rs and in pane_view.rs read it.
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (l1, l2) = (luminance(a), luminance(b));
    let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub bg: Rgb,
    pub fg: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
    pub find_match: Rgb,
    pub find_current: Rgb,
    pub ansi: [Rgb; 16],
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
};

impl Palette {
    pub fn indexed(&self, i: u8) -> Rgb {
        match i {
            0..=15 => self.ansi[i as usize],
            16..=231 => {
                let i = i - 16;
                let steps = [0u8, 95, 135, 175, 215, 255];
                Rgb { r: steps[(i / 36) as usize], g: steps[((i % 36) / 6) as usize], b: steps[(i % 6) as usize] }
            }
            232..=255 => {
                let v = 8 + (i - 232) * 10;
                Rgb { r: v, g: v, b: v }
            }
        }
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

    #[test]
    fn indexed_cube_and_grey_ramp() {
        assert_eq!(NAVY.indexed(16), rgb(0x000000));
        assert_eq!(NAVY.indexed(231), rgb(0xffffff));
        assert_eq!(NAVY.indexed(196), rgb(0xff0000));
        assert_eq!(NAVY.indexed(232), rgb(0x080808));
        assert_eq!(NAVY.indexed(255), rgb(0xeeeeee));
        assert_eq!(NAVY.indexed(1), NAVY.ansi[1]);
    }

    #[test]
    fn resolves_named_spec_and_indexed() {
        assert_eq!(NAVY.resolve(&Color::Named(NamedColor::Foreground)), NAVY.fg);
        assert_eq!(NAVY.resolve(&Color::Named(NamedColor::Background)), NAVY.bg);
        assert_eq!(NAVY.resolve(&Color::Named(NamedColor::Red)), NAVY.ansi[1]);
        assert_eq!(NAVY.resolve(&Color::Named(NamedColor::BrightBlue)), NAVY.ansi[12]);
        assert_eq!(NAVY.resolve(&Color::Spec(alacritty_terminal::vte::ansi::Rgb { r: 1, g: 2, b: 3 })), Rgb { r: 1, g: 2, b: 3 });
        assert_eq!(NAVY.resolve(&Color::Indexed(196)), rgb(0xff0000));
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
