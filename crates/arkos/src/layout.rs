//! Turns one row of terminal cells into text runs with a starting column, so the painter can
//! shape each run once and place it at an exact cell x. Pure: no GPUI, so it is unit tested.
use crate::palette::Rgb;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellStyle {
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub dim: bool,
    pub underline: bool,
    pub inverse: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    Normal,
    Wide,
    Spacer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub col: usize,
    pub text: String,
    pub style: CellStyle,
    pub wide: bool,
}

pub fn build_runs(cells: impl Iterator<Item = (char, CellStyle, CellKind)>) -> Vec<Run> {
    build_stamped_runs(cells.map(|cell| (cell, 0.0))).into_iter().map(|(run, _)| run).collect()
}

/// `build_runs` for cells that each carry a glyph stamp (`effects::paint`): a run also ends where
/// the stamp changes, and each run comes with its cells' stamp. A run is all text or all graphics
/// characters (`contrast::is_graphic`), which are drawn in the colour sent.
pub fn build_stamped_runs(cells: impl Iterator<Item = ((char, CellStyle, CellKind), f32)>) -> Vec<(Run, f32)> {
    let mut runs: Vec<(Run, f32)> = Vec::new();
    for (col, ((c, mut style, kind), stamp)) in cells.enumerate() {
        if kind == CellKind::Spacer {
            continue;
        }
        if style.inverse {
            std::mem::swap(&mut style.fg, &mut style.bg);
            style.inverse = false;
        }
        let wide = kind == CellKind::Wide;
        let extend = match runs.last() {
            Some((last, at)) => {
                !wide && !last.wide && last.style == style && *at == stamp && last.col + last.text.chars().count() == col && last.text.chars().last().map(crate::contrast::is_graphic) == Some(crate::contrast::is_graphic(c))
            }
            None => false,
        };
        if extend {
            runs.last_mut().unwrap().0.text.push(c);
        } else {
            runs.push((Run { col, text: c.to_string(), style, wide }, stamp));
        }
    }
    runs
}

/// One rectangle of a block element, in fractions of its cell (left, top, right, bottom), and the
/// share of the colour it is filled with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub alpha: f32,
}

const fn piece(x0: f32, y0: f32, x1: f32, y1: f32) -> Piece {
    Piece { x0, y0, x1, y1, alpha: 1.0 }
}

const fn shade(alpha: f32) -> Piece {
    Piece { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0, alpha }
}

const UL: Piece = piece(0.0, 0.0, 0.5, 0.5);
const UR: Piece = piece(0.5, 0.0, 1.0, 0.5);
const LL: Piece = piece(0.0, 0.5, 0.5, 1.0);
const LR: Piece = piece(0.5, 0.5, 1.0, 1.0);

const E: f32 = 0.125;

/// The block elements' rectangles, from U+2580.
const BLOCKS: [&[Piece]; 32] = [
    &[piece(0.0, 0.0, 1.0, 0.5)],
    &[piece(0.0, 1.0 - E, 1.0, 1.0)],
    &[piece(0.0, 1.0 - 2.0 * E, 1.0, 1.0)],
    &[piece(0.0, 1.0 - 3.0 * E, 1.0, 1.0)],
    &[piece(0.0, 0.5, 1.0, 1.0)],
    &[piece(0.0, 1.0 - 5.0 * E, 1.0, 1.0)],
    &[piece(0.0, 1.0 - 6.0 * E, 1.0, 1.0)],
    &[piece(0.0, 1.0 - 7.0 * E, 1.0, 1.0)],
    &[piece(0.0, 0.0, 1.0, 1.0)],
    &[piece(0.0, 0.0, 7.0 * E, 1.0)],
    &[piece(0.0, 0.0, 6.0 * E, 1.0)],
    &[piece(0.0, 0.0, 5.0 * E, 1.0)],
    &[piece(0.0, 0.0, 0.5, 1.0)],
    &[piece(0.0, 0.0, 3.0 * E, 1.0)],
    &[piece(0.0, 0.0, 2.0 * E, 1.0)],
    &[piece(0.0, 0.0, E, 1.0)],
    &[piece(0.5, 0.0, 1.0, 1.0)],
    &[shade(0.25)],
    &[shade(0.5)],
    &[shade(0.75)],
    &[piece(0.0, 0.0, 1.0, E)],
    &[piece(1.0 - E, 0.0, 1.0, 1.0)],
    &[LL],
    &[LR],
    &[UL],
    &[UL, LL, LR],
    &[UL, LR],
    &[UL, UR, LL],
    &[UL, UR, LR],
    &[UR],
    &[UR, LL],
    &[UR, LL, LR],
];

/// The rectangles a block element (U+2580 to U+259F: halves, eighths, shades and quadrants) is
/// drawn as, or None for any other character. The terminal draws these itself, as most
/// terminals do: many fonts lack them (Roboto Mono has none), a fallback font's are seldom the
/// cell's size, and a bar of them must meet its neighbours with no seam.
pub fn block_pieces(c: char) -> Option<&'static [Piece]> {
    let i = (c as u32).checked_sub(0x2580)? as usize;
    BLOCKS.get(i).copied()
}

/// Whether a run of `cells` shaped `width` points wide has strayed from its cells `cell` points
/// wide by half a point or more, as a run does when the font lacks a glyph and a fallback font of
/// another width draws it. Such a run is painted a character to each cell instead.
pub fn strays(width: f32, cells: usize, cell: f32) -> bool {
    (width - cells as f32 * cell).abs() >= 0.5
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::rgb;

    fn st(fg: u32, bg: u32) -> CellStyle {
        CellStyle { fg: rgb(fg), bg: rgb(bg), bold: false, italic: false, dim: false, underline: false, inverse: false }
    }

    #[test]
    fn a_stamped_run_ends_where_the_stamp_changes() {
        let a = st(0xffffff, 0x000000);
        let cells = [('o', 0.0), ('k', 2.5), ('!', 2.5)].map(|(c, s)| ((c, a, CellKind::Normal), s));
        let runs = build_stamped_runs(cells.into_iter());
        assert_eq!(runs, vec![(Run { col: 0, text: "o".into(), style: a, wide: false }, 0.0), (Run { col: 1, text: "k!".into(), style: a, wide: false }, 2.5)]);
    }

    #[test]
    fn merges_same_style_cells() {
        let a = st(0xffffff, 0x000000);
        let runs = build_runs(vec![('h', a, CellKind::Normal), ('i', a, CellKind::Normal)].into_iter());
        assert_eq!(runs, vec![Run { col: 0, text: "hi".into(), style: a, wide: false }]);
    }

    #[test]
    fn breaks_on_style_change_and_keeps_columns() {
        let a = st(0xffffff, 0x000000);
        let b = st(0xff0000, 0x000000);
        let runs = build_runs(vec![('a', a, CellKind::Normal), ('b', b, CellKind::Normal), ('c', b, CellKind::Normal)].into_iter());
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0], Run { col: 0, text: "a".into(), style: a, wide: false });
        assert_eq!(runs[1], Run { col: 1, text: "bc".into(), style: b, wide: false });
    }

    #[test]
    fn wide_chars_get_their_own_run_and_spacers_are_skipped() {
        let a = st(0xffffff, 0x000000);
        let runs = build_runs(vec![('x', a, CellKind::Normal), ('語', a, CellKind::Wide), (' ', a, CellKind::Spacer), ('y', a, CellKind::Normal)].into_iter());
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[1], Run { col: 1, text: "語".into(), style: a, wide: true });
        assert_eq!(runs[2].col, 3);
        assert_eq!(runs[2].text, "y");
    }

    #[test]
    fn a_run_ends_where_text_meets_a_graphics_character() {
        let a = st(0xffffff, 0x000000);
        let runs = build_runs("ab\u{2580}\u{2588}c".chars().map(|c| (c, a, CellKind::Normal)));
        let texts: Vec<&str> = runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, ["ab", "\u{2580}\u{2588}", "c"]);
    }

    #[test]
    fn inverse_swaps_colours_at_build_time() {
        let mut a = st(0x111111, 0x222222);
        a.inverse = true;
        let runs = build_runs(vec![('z', a, CellKind::Normal)].into_iter());
        assert_eq!(runs[0].style.fg, rgb(0x222222));
        assert_eq!(runs[0].style.bg, rgb(0x111111));
        assert!(!runs[0].style.inverse);
    }

    #[test]
    fn every_block_element_is_drawn_as_rectangles_of_its_cell() {
        for c in '\u{2580}'..='\u{259f}' {
            let pieces = block_pieces(c).unwrap_or_else(|| panic!("{c:?} has no rectangles"));
            assert!(!pieces.is_empty());
            for p in pieces {
                assert!(0.0 <= p.x0 && p.x0 < p.x1 && p.x1 <= 1.0 && 0.0 <= p.y0 && p.y0 < p.y1 && p.y1 <= 1.0, "{c:?} stays in its cell");
                assert!(p.alpha > 0.0 && p.alpha <= 1.0);
            }
        }
    }

    #[test]
    fn a_full_block_fills_its_cell_and_the_shades_fill_it_lighter() {
        let full = Piece { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0, alpha: 1.0 };
        assert_eq!(block_pieces('\u{2588}'), Some(&[full][..]));
        let shade = |c| block_pieces(c).unwrap()[0].alpha;
        assert!(shade('\u{2591}') < shade('\u{2592}') && shade('\u{2592}') < shade('\u{2593}') && shade('\u{2593}') < 1.0);
    }

    #[test]
    fn halves_eighths_and_quadrants_cover_the_parts_their_names_say() {
        let one = |c| block_pieces(c).unwrap()[0];
        assert_eq!((one('\u{2580}').y1, one('\u{2584}').y0), (0.5, 0.5), "upper and lower halves");
        assert_eq!((one('\u{258c}').x1, one('\u{2590}').x0), (0.5, 0.5), "left and right halves");
        assert_eq!(one('\u{2581}').y0, 0.875, "a lower eighth");
        assert_eq!(one('\u{258f}').x1, 0.125, "a left eighth");
        assert_eq!(block_pieces('\u{259a}').unwrap().len(), 2, "upper left and lower right quadrants");
        assert_eq!(block_pieces('\u{259f}').unwrap().len(), 3);
    }

    #[test]
    fn text_braille_and_box_drawing_are_left_to_the_font() {
        for c in ['a', '\u{2500}', '\u{2801}', '\u{e0b0}', '\u{1fb00}'] {
            assert_eq!(block_pieces(c), None, "{c:?}");
        }
    }

    #[test]
    fn a_run_is_placed_cell_by_cell_once_its_shaped_width_strays_from_its_cells() {
        assert!(!strays(80.0, 10, 8.0), "a run the font covers lines up with its cells");
        assert!(!strays(80.3, 10, 8.0), "under half a point is rounding");
        assert!(strays(120.0, 10, 8.0), "a fallback font's wider glyphs drift right");
        assert!(strays(60.0, 10, 8.0));
    }

    #[test]
    fn empty_row_gives_no_runs() {
        assert!(build_runs(std::iter::empty()).is_empty());
    }
}
