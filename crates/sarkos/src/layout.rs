//! Turns one row of terminal cells into text runs with a starting column, so the painter can
//! shape each run once and place it at an exact cell x. Pure: no GPUI, so it is unit tested.
// Used from Task 10 onward.
#![allow(dead_code)]
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
    let mut runs: Vec<Run> = Vec::new();
    for (col, (c, mut style, kind)) in cells.enumerate() {
        if kind == CellKind::Spacer {
            continue;
        }
        if style.inverse {
            std::mem::swap(&mut style.fg, &mut style.bg);
            style.inverse = false;
        }
        let wide = kind == CellKind::Wide;
        let extend = match runs.last() {
            Some(last) => !wide && !last.wide && last.style == style && last.col + last.text.chars().count() == col,
            None => false,
        };
        if extend {
            runs.last_mut().unwrap().text.push(c);
        } else {
            runs.push(Run { col, text: c.to_string(), style, wide });
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::rgb;

    fn st(fg: u32, bg: u32) -> CellStyle {
        CellStyle { fg: rgb(fg), bg: rgb(bg), bold: false, italic: false, dim: false, underline: false, inverse: false }
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
    fn inverse_swaps_colours_at_build_time() {
        let mut a = st(0x111111, 0x222222);
        a.inverse = true;
        let runs = build_runs(vec![('z', a, CellKind::Normal)].into_iter());
        assert_eq!(runs[0].style.fg, rgb(0x222222));
        assert_eq!(runs[0].style.bg, rgb(0x111111));
        assert!(!runs[0].style.inverse);
    }

    #[test]
    fn empty_row_gives_no_runs() {
        assert!(build_runs(std::iter::empty()).is_empty());
    }
}
