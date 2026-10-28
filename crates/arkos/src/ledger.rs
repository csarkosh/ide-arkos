//! Finds how far a program scrolled its own screen between two snapshots of the grid.
//!
//! A program on the alternate screen (Claude Code's fullscreen renderer) scrolls itself by
//! redrawing, so the terminal keeps no history of it. A record follows rows as the program moves
//! them, and that needs one answer from two grids: by how many rows did the content shift, which
//! rows moved with it (the block), and which rows are new (the band). This module is that answer
//! and nothing else; it works on rows as strings so it is pure and unit tested.
// Nothing calls this yet; the record that uses it arrives in a later step, which removes this line.
#![allow(dead_code)]

use crate::layout::{CellKind, CellStyle};
use std::ops::Range;

/// Stands in for the second half of a wide character, so columns stay one char per cell.
pub const SPACER: char = '\0';
/// The fewest rows that must line up before a shift is believed.
pub const MIN_MATCHED: usize = 4;
/// How many more rows the best shift must line up than the runner-up.
pub const MARGIN: usize = 2;

/// One char per cell, spacers marked, trailing blanks and spacers trimmed.
pub fn row_text(cells: &[(char, CellStyle, CellKind)]) -> String {
    let mut text: String = cells
        .iter()
        .map(|(c, _, kind)| if *kind == CellKind::Spacer { SPACER } else { *c })
        .collect();
    let kept = text.trim_end_matches([' ', SPACER]).len();
    text.truncate(kept);
    text
}

/// A vertical shift of the program's content. A positive `k` means old row `i` is now at
/// `i + k`. `block` and `band` are row ranges in the new grid: the rows that moved, and the rows
/// that the shift uncovered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shift {
    pub k: i32,
    pub block: Range<usize>,
    pub band: Range<usize>,
}

/// The shift that lines up the most non-blank rows, if it is clearly better than the rest.
pub fn find_shift(old: &[String], new: &[String]) -> Option<Shift> {
    let n = old.len().min(new.len());
    if n < 2 {
        return None;
    }
    let reach = n as i32 - 1;
    let counts: Vec<(i32, usize)> = (-reach..=reach)
        .filter(|k| *k != 0)
        .map(|k| (k, matched_rows(old, new, n, k).count()))
        .collect();
    let (best_k, best) = counts.iter().copied().max_by_key(|(_, c)| *c)?;
    let runner_up = counts
        .iter()
        .filter(|(k, _)| *k != best_k)
        .map(|(_, c)| *c)
        .max()
        .unwrap_or(0);
    if best < MIN_MATCHED || best < runner_up + MARGIN {
        return None;
    }
    let mut rows = matched_rows(old, new, n, best_k);
    let first = rows.next()?;
    let last = rows.last().unwrap_or(first);
    let block = first..last + 1;
    let band = if best_k > 0 {
        block.start.saturating_sub(best_k as usize)..block.start
    } else {
        block.end..(block.end + best_k.unsigned_abs() as usize).min(n)
    };
    Some(Shift { k: best_k, block, band })
}

/// The new-grid rows `j` where a non-blank old row `j - k` is found unchanged.
fn matched_rows<'a>(
    old: &'a [String],
    new: &'a [String],
    n: usize,
    k: i32,
) -> impl Iterator<Item = usize> + 'a {
    (0..n).filter_map(move |i| {
        let j = i as i32 + k;
        (j >= 0 && (j as usize) < n && !old[i].is_empty() && old[i] == new[j as usize])
            .then_some(j as usize)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(rows: &[&str]) -> Vec<String> {
        rows.iter().map(|r| r.to_string()).collect()
    }

    #[test]
    fn a_scroll_is_the_shift_that_lines_up_the_most_rows_and_names_its_block_and_band() {
        // Ten content rows over a two-row prompt box; the program scrolled up into older text by 3.
        let old = grid(&["l20","l21","l22","l23","l24","l25","l26","l27","l28","l29","─────","❯"]);
        let new = grid(&["l17","l18","l19","l20","l21","l22","l23","l24","l25","l26","─────","❯"]);
        assert_eq!(find_shift(&old, &new), Some(Shift { k: 3, block: 3..10, band: 0..3 }));
        // Back down by 3: the band is below the block.
        assert_eq!(find_shift(&new, &old), Some(Shift { k: -3, block: 0..7, band: 7..10 }));
    }

    #[test]
    fn a_blank_row_never_counts_as_a_match_but_moves_inside_the_block() {
        let old = grid(&["a","","b","c","","d","e","f","g","h"]);
        let new = grid(&["x","y","a","","b","c","","d","e","f"]);
        assert_eq!(find_shift(&old, &new), Some(Shift { k: 2, block: 2..10, band: 0..2 }));
    }

    #[test]
    fn fewer_than_four_rows_or_a_thin_margin_is_no_shift() {
        let old = grid(&["a","b","c","d","e","f"]);
        assert_eq!(find_shift(&old, &grid(&["z","a","b","c","q","r"])), None, "three rows line up");
        assert_eq!(find_shift(&old, &old), None, "nothing moved");
        // Four identical rows line up equally well at k = 1 and k = 2: no margin, no shift.
        let same = grid(&["r","r","r","r","r","r"]);
        assert_eq!(find_shift(&same, &same), None);
    }

    #[test]
    fn chrome_that_stays_put_and_a_sticky_header_are_outside_the_block() {
        // The header appears at row 0 as the view scrolls; the status row at the bottom never moves.
        let old = grid(&["l1","l2","l3","l4","l5","l6","l7","status"]);
        let new = grid(&["❯ header","l0","l1","l2","l3","l4","l5","status"]);
        assert_eq!(find_shift(&old, &new), Some(Shift { k: 2, block: 2..7, band: 0..2 }));
    }

    #[test]
    fn the_band_is_clipped_to_the_grid() {
        let old = grid(&["a","b","c","d","e"]);
        let new = grid(&["b","c","d","e","f"]);
        assert_eq!(find_shift(&old, &new), Some(Shift { k: -1, block: 0..4, band: 4..5 }));
    }

    #[test]
    fn row_text_is_one_char_per_cell_with_spacers_marked_and_the_tail_trimmed() {
        use crate::layout::{CellKind, CellStyle};
        use crate::palette::Rgb;
        let rgb = Rgb { r: 0, g: 0, b: 0 };
        let s = CellStyle { fg: rgb, bg: rgb, bold: false, italic: false, dim: false, underline: false, inverse: false };
        let cells = vec![('字', s, CellKind::Wide), (' ', s, CellKind::Spacer), ('a', s, CellKind::Normal), (' ', s, CellKind::Normal)];
        assert_eq!(row_text(&cells), "字\0a");
    }
}
