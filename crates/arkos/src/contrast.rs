//! The colour a terminal glyph is drawn in, from the colour the program sent and the background
//! under it, and the checks of every colour a program can send (contrast matrix spec,
//! 2026-11-01). Pure: the painter and the checks both call `draw`, so the checks test the painter.

use crate::palette::{contrast, Palette, Rgb};
use std::collections::HashSet;

/// The contrast every terminal glyph keeps with the background it is drawn on, whatever colour
/// the program asked for. A program can send colours meant for another ground, as RGB the
/// palette never sees: Claude Code under its stock dark theme draws a diff's context lines near
/// white, which vanish on a light theme's paper.
pub const TEXT_MIN_CONTRAST: f64 = 4.5;
/// The contrast a colour under `TEXT_MIN_CONTRAST` is moved to. It is wider than the floor
/// because under an effect a band is a wash that shows lighter than its colour, and text that
/// only just cleared the band's colour would not clear the wash.
pub const TEXT_MOVED_CONTRAST: f64 = 7.0;

/// `fg` as it is drawn on `bg`: itself when it has `TEXT_MIN_CONTRAST` with `bg` already, else
/// blended toward black or white, whichever reads better on `bg`, just far enough to reach
/// `TEXT_MOVED_CONTRAST`, or all the way when that is out of reach. The blend keeps its hue.
pub fn legible(fg: Rgb, bg: Rgb) -> Rgb {
    use crate::palette::contrast;
    if contrast(fg, bg) >= TEXT_MIN_CONTRAST {
        return fg;
    }
    let min = TEXT_MOVED_CONTRAST;
    let (black, white) = (Rgb { r: 0, g: 0, b: 0 }, Rgb { r: 255, g: 255, b: 255 });
    let ink = if contrast(black, bg) >= contrast(white, bg) { black } else { white };
    let at = |t: f32| {
        let mix = |f: u8, i: u8| (f32::from(f) + (f32::from(i) - f32::from(f)) * t).round() as u8;
        Rgb { r: mix(fg.r, ink.r), g: mix(fg.g, ink.g), b: mix(fg.b, ink.b) }
    };
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..12 {
        let mid = (lo + hi) / 2.0;
        if contrast(at(mid), bg) >= min { hi = mid } else { lo = mid }
    }
    at(hi)
}

/// How far faint text (SGR 2) fades toward its background.
pub const DIM_FADE: f32 = 0.4;
/// The contrast with its background faint text keeps however far it would fade, so a theme's
/// grey that is dim already (a hint, Claude Code's status lines) stays readable.
pub const DIM_MIN_CONTRAST: f64 = 3.0;

/// The alpha faint text of `fg` is drawn at over `bg`: `DIM_FADE` of the way to the background,
/// or as far as keeps `DIM_MIN_CONTRAST`; full for a colour under that contrast already.
pub fn dim_alpha(fg: Rgb, bg: Rgb) -> f32 {
    let at = |fade: f32| {
        let mix = |f: u8, b: u8| (f32::from(f) + (f32::from(b) - f32::from(f)) * fade).round() as u8;
        crate::palette::contrast(Rgb { r: mix(fg.r, bg.r), g: mix(fg.g, bg.g), b: mix(fg.b, bg.b) }, bg)
    };
    if at(DIM_FADE) >= DIM_MIN_CONTRAST {
        return 1.0 - DIM_FADE;
    }
    let (mut lo, mut hi) = (0.0, DIM_FADE);
    for _ in 0..10 {
        let mid = (lo + hi) / 2.0;
        if at(mid) >= DIM_MIN_CONTRAST { lo = mid } else { hi = mid }
    }
    1.0 - lo
}

/// How a glyph is drawn: its colour, its alpha over the background, and whether the floor had
/// to move the colour the program sent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drawn {
    pub color: Rgb,
    pub alpha: f32,
    pub moved: bool,
}

impl Drawn {
    /// The colour seen: `color` at `alpha` over `bg`.
    pub fn over(&self, bg: Rgb) -> Rgb {
        let mix = |c: u8, b: u8| (f32::from(c) * self.alpha + f32::from(b) * (1.0 - self.alpha)).round() as u8;
        Rgb { r: mix(self.color.r, bg.r), g: mix(self.color.g, bg.g), b: mix(self.color.b, bg.b) }
    }
}

/// How a glyph the program sent in `fg` on `bg` is drawn, faint (SGR 2) or not: the floor first
/// (`legible`), then faint text's fade (`dim_alpha`) from the colour the floor gave.
pub fn draw(fg: Rgb, bg: Rgb, dim: bool) -> Drawn {
    let color = legible(fg, bg);
    let alpha = if dim { dim_alpha(color, bg) } else { 1.0 };
    Drawn { color, alpha, moved: color != fg }
}

/// Whether `c` is a graphics character, a picture's pixel or a shape rather than a letter: block
/// elements and the legacy computing symbols (sextants), which terminal image viewers and Claude
/// Code's mascot draw with fg and bg as two pixels' colours; Braille, which plots and pictures
/// use as dots; and the powerline separators, whose fg is the next segment's bg on purpose.
/// The floor would break them, so they are drawn in the colour sent.
pub fn is_graphic(c: char) -> bool {
    matches!(c, '\u{2580}'..='\u{259f}' | '\u{2800}'..='\u{28ff}' | '\u{e0b0}'..='\u{e0d7}' | '\u{1fb00}'..='\u{1fbff}')
}

/// How a glyph of `c` is drawn: as `draw` for text; in the colour sent for a graphics character
/// (`is_graphic`), faint text's fade still applied.
pub fn draw_char(c: char, fg: Rgb, bg: Rgb, dim: bool) -> Drawn {
    if is_graphic(c) {
        return Drawn { color: fg, alpha: if dim { dim_alpha(fg, bg) } else { 1.0 }, moved: false };
    }
    draw(fg, bg, dim)
}

/// Claude Code's own colours, read from its binary by `scripts/claude-code-colours.py`: its six
/// stock themes, key by key as it spells them, and every colour its diff renderer draws.
const CLAUDE_CODE_COLOURS: &str = include_str!("../data/claude-code-colours.json");

/// Claude Code's own colours (`CLAUDE_CODE_COLOURS`).
pub struct Stock {
    /// The Claude Code version they were read from.
    pub version: String,
    /// Each stock theme's name and its keys with their values, sorted.
    pub themes: Vec<(String, Vec<(String, String)>)>,
    /// The RGB colours the diff renderer draws, as text and as bands.
    pub diff_rgb: Vec<Rgb>,
    /// The palette indexes the diff renderer draws.
    pub diff_indexed: Vec<u8>,
}

/// The stock colours, parsed once. The file is in the build and tested to parse.
pub fn stock() -> &'static Stock {
    static STOCK: std::sync::LazyLock<Stock> = std::sync::LazyLock::new(|| {
        let v: serde_json::Value = serde_json::from_str(CLAUDE_CODE_COLOURS).expect("claude-code-colours.json parses");
        let text = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_string();
        let themes = v["themes"]
            .as_object()
            .map(|o| o.iter().map(|(name, keys)| (name.clone(), keys.as_object().map(|k| k.iter().map(|(k, v)| (k.clone(), text(v))).collect()).unwrap_or_default())).collect())
            .unwrap_or_default();
        let list = |key: &str| v["diff"][key].as_array().cloned().unwrap_or_default();
        Stock {
            version: text(&v["claude_code"]),
            themes,
            diff_rgb: list("rgb").iter().filter_map(|c| Rgb::parse(c.as_str()?).ok()).collect(),
            diff_indexed: list("indexed").iter().filter_map(|i| u8::try_from(i.as_u64()?).ok()).collect(),
        }
    });
    &STOCK
}

/// A Claude Code colour value under `palette`: `rgb(r,g,b)` (spaces allowed), `#rrggbb`,
/// `ansi:<name>` or `ansi256(n)`. None for anything else.
pub fn resolve(value: &str, palette: &Palette) -> Option<Rgb> {
    let v = value.trim();
    if let Some(name) = v.strip_prefix("ansi:") {
        return crate::claude_theme::ANSI_NAMES.iter().position(|a| *a == name).map(|i| palette.indexed(i as u8));
    }
    if let Some(n) = v.strip_prefix("ansi256(").and_then(|r| r.strip_suffix(')')) {
        return n.trim().parse().ok().map(|i| palette.indexed(i));
    }
    if let Some(n) = v.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')) {
        let c: Vec<u8> = n.split(',').map(|x| x.trim().parse().ok()).collect::<Option<_>>()?;
        return match c[..] {
            [r, g, b] => Some(Rgb { r, g, b }),
            _ => None,
        };
    }
    Rgb::parse(v).ok()
}

/// Whether a Claude Code theme key paints a background (a band or a fill) rather than text.
pub fn is_background_key(key: &str) -> bool {
    key.starts_with("diff") || key.contains("Background") || key.contains("_background") || key.ends_with("Bg")
}

/// A pair the terminal drew under its floor: a renderer bug, never the theme's.
#[derive(Debug, Clone, PartialEq)]
pub struct Low {
    pub what: String,
    pub fg: Rgb,
    pub bg: Rgb,
    pub faint: bool,
    pub ratio: f64,
    pub needs: f64,
}

/// What the terminal does with every colour a program can send on a theme (`terminal_report`).
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalReport {
    /// The pairs drawn, each plain and faint.
    pub pairs: usize,
    /// The pairs whose foreground the floor moved to keep it readable.
    pub moved: usize,
    /// The pairs drawn under 4.5:1 (3:1 faint).
    pub low: Vec<Low>,
}

/// Draws every foreground a program can send on every background it can send, on `theme`, plain
/// and faint, as the terminal draws text (`draw`), and reports the pairs, how many the floor
/// moved and any drawn under 4.5:1 (3:1 faint). The program's colours: the theme's terminal
/// foreground and background (inverse text), its 256 palette colours, and Claude Code's stock
/// colours (`stock`). The backgrounds: the terminal's own, its selection and find highlights,
/// and the palette's colours as fills. Each stock theme's text is also drawn on its own bands,
/// and the diff renderer's colours on one another. Graphics characters (`is_graphic`) are drawn
/// in the colour sent and are not in it.
pub fn terminal_report(theme: &crate::theme::Theme) -> TerminalReport {
    report_with(theme, draw)
}

fn report_with(theme: &crate::theme::Theme, draw: impl Fn(Rgb, Rgb, bool) -> Drawn) -> TerminalReport {
    let p = theme.palette();
    let stock = stock();
    let term = &theme.terminal;
    let swatch = |i: u8| (p.indexed(i), if i < 16 { format!("ansi {i}") } else { format!("colour {i}") });
    let mut fgs: Vec<(Rgb, String)> = vec![(term.foreground, "terminal.foreground".into()), (term.background, "terminal.background as inverse text".into())];
    fgs.extend((0..=255u8).map(swatch));
    let mut bgs: Vec<(Rgb, String)> = vec![
        (term.background, "terminal.background".into()),
        (term.selection, "terminal.selection".into()),
        (theme.semantic.find_match, "semantic.find_match".into()),
        (theme.semantic.find_current, "semantic.find_current".into()),
    ];
    bgs.extend((0..=255u8).map(swatch));
    let mut own: Vec<(Vec<(Rgb, String)>, Vec<(Rgb, String)>)> = Vec::new();
    for (name, keys) in &stock.themes {
        let of = |back: bool| -> Vec<(Rgb, String)> { keys.iter().filter(|(k, _)| is_background_key(k) == back).filter_map(|(k, v)| Some((resolve(v, &p)?, format!("claude code {name}: {k} ({v})")))).collect() };
        let (text, bands) = (of(false), of(true));
        fgs.extend(text.iter().cloned());
        own.push((text, bands));
    }
    let mut diff: Vec<(Rgb, String)> = stock.diff_rgb.iter().map(|c| (*c, format!("claude code diff {}", c.hex()))).collect();
    diff.extend(stock.diff_indexed.iter().map(|i| (p.indexed(*i), format!("claude code diff colour {i}"))));
    fgs.extend(diff.iter().cloned());
    own.push((diff.clone(), diff));

    let mut seen: HashSet<(Rgb, Rgb)> = HashSet::new();
    let mut report = TerminalReport { pairs: 0, moved: 0, low: Vec::new() };
    let mut check = |fg: &(Rgb, String), bg: &(Rgb, String)| {
        if !seen.insert((fg.0, bg.0)) {
            return;
        }
        report.pairs += 1;
        for faint in [false, true] {
            let d = draw(fg.0, bg.0, faint);
            if d.moved && !faint {
                report.moved += 1;
            }
            let (ratio, needs) = (contrast(d.over(bg.0), bg.0), if faint { DIM_MIN_CONTRAST } else { TEXT_MIN_CONTRAST });
            // The drawn colour is rounded to whole channels, so a hair under the line is not low.
            if ratio < needs - 0.05 {
                let what = format!("{}{} on {}", if faint { "faint " } else { "" }, fg.1, bg.1);
                report.low.push(Low { what, fg: fg.0, bg: bg.0, faint, ratio, needs });
            }
        }
    };
    for fg in &fgs {
        for bg in &bgs {
            check(fg, bg);
        }
    }
    for (text, bands) in &own {
        for fg in text {
            for bg in bands {
                check(fg, bg);
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builtins() -> Vec<(&'static str, crate::theme::Theme)> {
        crate::theme::BUILTIN.iter().map(|(n, _)| (*n, crate::theme::builtin(n).unwrap())).collect()
    }

    /// Run by hand after a Claude Code update (`cargo test -- --ignored stock_colours`): it fails
    /// when the installed Claude Code draws colours the fixture does not hold, and says to run
    /// `scripts/claude-code-colours.py`, which writes them, then the tests again.
    #[test]
    #[ignore]
    fn the_stock_colours_are_those_of_the_installed_claude_code() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let out = std::process::Command::new("python3").args(["-I", "scripts/claude-code-colours.py", "--check"]).current_dir(root).output().unwrap();
        assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    }

    #[test]
    fn claude_code_s_stock_colours_are_its_six_themes_and_its_diff_renderer_s() {
        let s = stock();
        let names: Vec<&str> = s.themes.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["dark", "dark-ansi", "dark-daltonized", "light", "light-ansi", "light-daltonized"]);
        let p = crate::theme::builtin("navy").unwrap().palette();
        for (name, keys) in &s.themes {
            assert!(keys.iter().any(|(k, _)| k == "text") && keys.iter().any(|(k, _)| is_background_key(k)), "{name}");
            for (k, v) in keys {
                assert!(resolve(v, &p).is_some(), "{name}: {k} = {v}");
            }
        }
        assert!(s.diff_rgb.len() >= 20 && !s.diff_indexed.is_empty());
        assert!(s.diff_rgb.contains(&Rgb { r: 248, g: 248, b: 242 }), "the dark diff's text");
        assert_eq!(resolve("rgb(55, 55, 55)", &p), Some(Rgb { r: 55, g: 55, b: 55 }));
    }

    #[test]
    fn every_colour_a_program_can_send_is_drawn_readable_on_every_built_in_theme_plain_and_under_each_effect() {
        let mut deepenings: Vec<f32> = std::iter::once(0.0).chain(crate::effects::BUILTIN.iter().map(|b| crate::effects::manifest::parse(b.manifest).unwrap().text_contrast)).collect();
        deepenings.sort_by(f32::total_cmp);
        deepenings.dedup();
        // A theme a thread: the matrix is a few hundred thousand draws a theme.
        std::thread::scope(|scope| {
            for (name, t) in builtins() {
                let deepenings = &deepenings;
                scope.spawn(move || {
                    for c in deepenings {
                        let shown = crate::effects::active::contrasted(&t, *c);
                        let r = terminal_report(&shown);
                        assert!(r.pairs > 60_000, "{name}: {} pairs", r.pairs);
                        assert!(r.low.is_empty(), "{name} at text_contrast {c}: {:#?}", &r.low[..r.low.len().min(5)]);
                    }
                });
            }
        });
    }

    #[test]
    fn claude_code_s_stock_dark_text_is_moved_on_a_light_theme_and_its_own_text_is_not() {
        let t = crate::theme::builtin("lavender-light").unwrap();
        let paper = t.terminal.background;
        assert!(draw(Rgb { r: 248, g: 248, b: 242 }, paper, false).moved);
        assert!(!draw(t.terminal.foreground, paper, false).moved);
        assert!(terminal_report(&t).moved > 0);
    }

    #[test]
    fn a_terminal_drawing_the_colours_it_was_sent_fails_the_matrix() {
        let t = crate::theme::builtin("lavender-light").unwrap();
        let raw = |fg: Rgb, _bg: Rgb, _faint: bool| Drawn { color: fg, alpha: 1.0, moved: false };
        let r = report_with(&t, raw);
        assert!(r.low.iter().any(|l| l.what.starts_with("claude code dark: text") && l.what.ends_with("on terminal.background")), "{:#?}", &r.low[..r.low.len().min(5)]);
    }

    #[test]
    fn a_graphics_character_keeps_the_colour_sent_so_pictures_and_prompt_separators_stay_whole() {
        let (top, bottom) = (Rgb { r: 200, g: 120, b: 90 }, Rgb { r: 205, g: 124, b: 92 });
        for c in ['\u{2580}', '\u{2588}', '\u{259b}', '\u{2801}', '\u{e0b0}', '\u{1fb00}'] {
            assert!(is_graphic(c), "{c:?}");
            assert_eq!(draw_char(c, top, bottom, false), Drawn { color: top, alpha: 1.0, moved: false }, "{c:?}");
        }
        for c in ['a', '-', '\u{2500}', '\u{2502}', '\u{e0a0}'] {
            assert!(!is_graphic(c), "box lines and icons are read, so they keep the floor: {c:?}");
            assert!(draw_char(c, top, bottom, false).moved, "{c:?}");
        }
    }

    #[test]
    fn a_glyph_is_drawn_in_the_floor_s_colour_faded_as_faint_text_is_and_says_whether_it_moved() {
        use crate::palette::contrast;
        let paper = Rgb { r: 0xf9, g: 0xf7, b: 0xfb };
        let ink = Rgb { r: 0x2d, g: 0x23, b: 0x40 };
        assert_eq!(draw(ink, paper, false), Drawn { color: ink, alpha: 1.0, moved: false });
        assert_eq!(draw(ink, paper, true).alpha, 1.0 - DIM_FADE);
        let near_white = Rgb { r: 248, g: 248, b: 242 };
        let d = draw(near_white, paper, true);
        assert!(d.moved);
        assert_eq!(d.color, legible(near_white, paper));
        assert!(contrast(d.over(paper), paper) >= DIM_MIN_CONTRAST - 0.05, "{:?}", d.over(paper));
    }

    #[test]
    fn text_a_program_draws_too_close_to_its_background_is_moved_toward_ink_until_it_reads() {
        use crate::palette::{contrast, Rgb};
        // Claude Code's stock dark diff draws its context lines in a near white, sent as RGB, so
        // on a light theme's paper they all but vanish.
        let paper = Rgb { r: 0xf9, g: 0xf7, b: 0xfb };
        let near_white = Rgb { r: 248, g: 248, b: 242 };
        let shown = legible(near_white, paper);
        assert!(contrast(shown, paper) >= TEXT_MOVED_CONTRAST - 0.05, "{shown:?}");
        assert!(contrast(shown, paper) < TEXT_MOVED_CONTRAST + 0.5, "moved no further than it must: {shown:?}");
        // Its message band is a dark grey sent as RGB, under a light theme's dark text.
        let band = Rgb { r: 55, g: 55, b: 55 };
        let ink = Rgb { r: 0x2d, g: 0x23, b: 0x40 };
        let shown = legible(ink, band);
        assert!(contrast(shown, band) >= TEXT_MOVED_CONTRAST - 0.05, "{shown:?}");
        assert!(shown.r > ink.r && shown.g > ink.g && shown.b > ink.b, "lightened toward white on a dark band: {shown:?}");
    }

    #[test]
    fn text_that_reads_already_keeps_its_colour_and_moved_text_keeps_its_hue() {
        use crate::palette::Rgb;
        let paper = Rgb { r: 0xf9, g: 0xf7, b: 0xfb };
        let red = Rgb { r: 0xc2, g: 0x3c, b: 0x4e };
        assert_eq!(legible(red, paper), red, "a theme colour that clears 4.5:1");
        let pale_red = Rgb { r: 255, g: 168, b: 180 };
        let shown = legible(pale_red, paper);
        assert!(shown.r > shown.g && shown.r > shown.b, "still a red: {shown:?}");
    }

    #[test]
    fn a_faint_line_in_a_colour_meant_for_another_ground_still_reads_at_three_to_one() {
        use crate::palette::{contrast, Rgb};
        let paper = Rgb { r: 0xf9, g: 0xf7, b: 0xfb };
        let fg = legible(Rgb { r: 248, g: 248, b: 242 }, paper);
        let a = dim_alpha(fg, paper);
        let faded = |c: u8, b: u8| (f32::from(c) * a + f32::from(b) * (1.0 - a)).round() as u8;
        let shown = Rgb { r: faded(fg.r, paper.r), g: faded(fg.g, paper.g), b: faded(fg.b, paper.b) };
        assert!(contrast(shown, paper) >= DIM_MIN_CONTRAST - 0.05, "{shown:?}");
    }

    #[test]
    fn faint_text_fades_toward_its_background_but_keeps_three_to_one_contrast_with_it() {
        use crate::palette::Rgb;
        let navy = Rgb { r: 0x0e, g: 0x17, b: 0x23 };
        let white = Rgb { r: 0xc8, g: 0xcc, b: 0xd4 };
        assert_eq!(dim_alpha(white, navy), 1.0 - DIM_FADE, "bright text fades the whole way");
        let grey = Rgb { r: 0x7c, g: 0x87, b: 0x98 };
        let a = dim_alpha(grey, navy);
        assert!(a > 1.0 - DIM_FADE && a < 1.0, "{a}");
        let faded = |c: u8, b: u8| (f32::from(c) * a + f32::from(b) * (1.0 - a)).round() as u8;
        let shown = Rgb { r: faded(grey.r, navy.r), g: faded(grey.g, navy.g), b: faded(grey.b, navy.b) };
        assert!(crate::palette::contrast(shown, navy) >= DIM_MIN_CONTRAST - 0.05, "{shown:?}");
        let murk = Rgb { r: 0x30, g: 0x38, b: 0x44 };
        assert_eq!(dim_alpha(murk, navy), 1.0, "a colour already under 3:1 does not fade further");
    }
}
