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
use alacritty_terminal::index::Side;
use std::collections::BTreeMap;
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

/// One end of the selection, held as a record line so it stays on its text while the grid moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct End {
    pub line: i32,
    pub col: usize,
    pub side: Side,
}

/// Whether the record still follows the program after a batch of output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Open,
    Closed,
}

/// A recorded row. A provisional line has not yet moved with a shift, so it may be chrome that
/// stays put; a confirmed one has moved with the text and is kept.
#[derive(Debug, Clone)]
struct Entry {
    text: String,
    confirmed: bool,
}

/// The latest reading of the output since the last commit, and the grid it was found on.
#[derive(Debug, Clone)]
struct Pending {
    shift: Shift,
    grid: Vec<String>,
}

/// The rows that have passed through the grid, numbered in a fixed frame of lines.
///
/// Grid row `r` is record line `r - offset`. The offset starts at 0 and moves by `k` with every
/// shift the program makes, so a line keeps naming the same text while the grid scrolls under
/// it. Output arrives in batches and a frame may span several, so the record compares each
/// batch with its baseline (the grid at its last commit) and holds the shift found as pending
/// until a different reading shows that a new frame has begun; only then is it committed.
#[derive(Debug, Clone)]
pub struct Ledger {
    offset: i32,
    lines: BTreeMap<i32, Entry>,
    rows: usize,
    baseline: Vec<String>,
    pending: Option<Pending>,
    last_moved: Option<Range<usize>>,
    anchor: End,
    end: Option<End>,
}

impl Ledger {
    /// A record of the grid at a press: every row a provisional line, the press its anchor.
    pub fn open(rows: Vec<String>, anchor_line: i32, anchor_col: usize) -> Ledger {
        let lines = rows
            .iter()
            .enumerate()
            .map(|(r, text)| (r as i32, Entry { text: text.clone(), confirmed: false }))
            .collect();
        Ledger {
            offset: 0,
            lines,
            rows: rows.len(),
            baseline: rows,
            pending: None,
            last_moved: None,
            anchor: End { line: anchor_line, col: anchor_col, side: Side::Left },
            end: None,
        }
    }

    /// Follows one batch of output, `new` being the grid's rows after it was applied.
    pub fn output(&mut self, new: &[String]) -> Outcome {
        let found = find_shift(&self.baseline, new);
        match (self.pending.take(), found) {
            (None, Some(shift)) => self.pending = Some(Pending { shift, grid: new.to_vec() }),
            (None, None) => return self.close_check(new),
            (Some(p), Some(shift)) if shift.k == p.shift.k => {
                self.pending = Some(Pending { shift, grid: new.to_vec() })
            }
            (Some(p), _) => {
                self.commit(&p.shift, &p.grid);
                self.baseline = p.grid;
                match find_shift(&self.baseline, new) {
                    Some(shift) => self.pending = Some(Pending { shift, grid: new.to_vec() }),
                    None => return self.close_check(new),
                }
            }
        }
        Outcome::Open
    }

    /// The committed offset with the pending shift applied.
    pub fn offset(&self) -> i32 {
        self.offset + self.pending.as_ref().map_or(0, |p| p.shift.k)
    }

    /// The rows that moved with the text in the latest shift, pending or committed: its block
    /// and its band. The whole grid before any shift.
    pub fn moved_rows(&self) -> Range<usize> {
        match &self.pending {
            Some(p) => moved(&p.shift),
            None => self.last_moved.clone().unwrap_or(0..self.rows),
        }
    }

    /// The record line grid row `row` shows now.
    pub fn line_of_row(&self, row: usize) -> i32 {
        row as i32 - self.offset()
    }

    /// The grid row record line `line` is at now, which may be off the grid.
    pub fn row_of_line(&self, line: i32) -> i32 {
        line + self.offset()
    }

    /// A line's text as the record reads with the pending shift applied. Rows in the pending
    /// block and band read from the grid it was found on; a provisional line that the pending
    /// shift would drop on commit reads as absent; any other line reads what was recorded.
    pub fn line_text(&self, line: i32) -> Option<&str> {
        if let Some(p) = &self.pending {
            let row = self.row_of_line(line);
            if row >= 0 {
                let row = row as usize;
                if p.shift.block.contains(&row) || p.shift.band.contains(&row) {
                    return p.grid.get(row).map(String::as_str);
                }
            }
        }
        let entry = self.lines.get(&line)?;
        if !entry.confirmed {
            if let Some(p) = &self.pending {
                if stayed_put(&entry.text, line + self.offset, &self.baseline, &p.grid) {
                    return None;
                }
            }
        }
        Some(&entry.text)
    }

    /// The grid's height when the record opened.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Commits `shift`, found between the baseline and `grid`: the offset moves, the block is
    /// rewritten and confirmed, the band comes in as provisional lines, and every provisional
    /// line whose row showed its text both before and after the shift is dropped.
    fn commit(&mut self, shift: &Shift, grid: &[String]) {
        let before = self.offset;
        self.offset += shift.k;
        for row in shift.block.clone() {
            let line = row as i32 - self.offset;
            self.lines.insert(line, Entry { text: grid[row].clone(), confirmed: true });
        }
        for row in shift.band.clone() {
            let line = row as i32 - self.offset;
            let confirmed = self.lines.get(&line).is_some_and(|e| e.confirmed);
            self.lines.insert(line, Entry { text: grid[row].clone(), confirmed });
        }
        let old = &self.baseline;
        self.lines.retain(|line, e| e.confirmed || !stayed_put(&e.text, line + before, old, grid));
        self.last_moved = Some(moved(shift));
    }

    /// Closed when more than half of the baseline's non-empty rows show something else in `new`.
    fn close_check(&self, new: &[String]) -> Outcome {
        let filled = self.baseline.iter().enumerate().filter(|(_, t)| !t.is_empty());
        let (total, differ) = filled.fold((0, 0), |(total, differ), (r, t)| {
            (total + 1, differ + usize::from(new.get(r) != Some(t)))
        });
        if differ * 2 > total { Outcome::Closed } else { Outcome::Open }
    }
}

/// The block and the band of a shift as one range of rows; they are always adjacent.
fn moved(shift: &Shift) -> Range<usize> {
    shift.block.start.min(shift.band.start)..shift.block.end.max(shift.band.end)
}

/// Whether grid row `row` showed `text` both before a shift (`old`) and after it (`new`).
fn stayed_put(text: &str, row: i32, old: &[String], new: &[String]) -> bool {
    row >= 0
        && old.get(row as usize).is_some_and(|t| t == text)
        && new.get(row as usize).is_some_and(|t| t == text)
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

    fn scrolled(content: &[&str], top: usize, height: usize, chrome: &[&str]) -> Vec<String> {
        let shown = height - chrome.len();
        content[top..top + shown].iter().chain(chrome).map(|r| r.to_string()).collect()
    }

    fn lines(l: &Ledger, from: i32, to: i32) -> Vec<Option<String>> {
        (from..=to).map(|line| l.line_text(line).map(str::to_string)).collect()
    }

    #[test]
    fn a_committed_shift_renumbers_nothing_and_adds_the_band_as_provisional_lines() {
        let content: Vec<String> = (0..40).map(|i| format!("l{i}")).collect();
        let c: Vec<&str> = content.iter().map(String::as_str).collect();
        let mut l = Ledger::open(scrolled(&c, 30, 12, &["─","❯"]), 9, 0);
        // Frame 1: scrolled up by 3, pending. Frame 2: by 3 more; frame 1 commits.
        assert_eq!(l.output(&scrolled(&c, 27, 12, &["─","❯"])), Outcome::Open);
        assert_eq!(l.offset(), 3);
        assert_eq!(l.output(&scrolled(&c, 24, 12, &["─","❯"])), Outcome::Open);
        assert_eq!(l.offset(), 6);
        // Line 0 is still l30, the first row at opening; lines -6..-1 are the rows that came in.
        assert_eq!(l.line_text(0), Some("l30"));
        assert_eq!(l.line_text(-6), Some("l24"));
        assert_eq!(l.line_text(-1), Some("l29"));
        assert_eq!(l.row_of_line(-6), 0);
        assert_eq!(l.line_of_row(9), 3);
    }

    #[test]
    fn chrome_from_the_opening_snapshot_is_dropped_at_the_first_commit() {
        let c: Vec<String> = (0..40).map(|i| format!("l{i}")).collect();
        let c: Vec<&str> = c.iter().map(String::as_str).collect();
        let mut l = Ledger::open(scrolled(&c, 30, 12, &["─","❯"]), 9, 0);
        assert_eq!(l.line_text(10), Some("─"), "provisional until something moves");
        l.output(&scrolled(&c, 27, 12, &["─","❯"]));
        l.output(&scrolled(&c, 24, 12, &["─","❯"]));
        assert_eq!(l.line_text(10), None);
        assert_eq!(l.line_text(11), None);
    }

    #[test]
    fn a_sticky_header_that_arrives_in_a_band_is_dropped_when_it_stays_put() {
        // Ten content rows over a status row; the program scrolls up by two per frame and shows a
        // header row once the message's first row has scrolled off.
        let mut l = Ledger::open(grid(&["l1","l2","l3","l4","l5","l6","l7","l8","l9","l10","status"]), 9, 0);
        l.output(&grid(&["❯ header","l0","l1","l2","l3","l4","l5","l6","l7","l8","status"]));
        assert_eq!(l.line_text(-2), Some("❯ header"), "pending: the band is read from the grid");
        l.output(&grid(&["❯ header","l-2","l-1","l0","l1","l2","l3","l4","l5","l6","status"]));
        l.output(&grid(&["❯ header","l-4","l-3","l-2","l-1","l0","l1","l2","l3","l4","status"]));
        // The header stayed at row 0 while the text moved under it: gone from the record, and the
        // rows that came in are contiguous.
        let got: Vec<Option<String>> = lines(&l, -6, 1);
        let want: Vec<Option<String>> = vec![None, Some("l-4".into()), Some("l-3".into()), Some("l-2".into()), Some("l-1".into()), Some("l0".into()), Some("l1".into()), Some("l2".into())];
        assert_eq!(got, want);
    }

    #[test]
    fn the_pressed_row_pushed_under_the_prompt_box_by_the_first_scroll_is_kept() {
        let mut l = Ledger::open(grid(&["l1","l2","l3","l4","l5","l6","l7","─","❯"]), 6, 0);
        l.output(&grid(&["l-2","l-1","l0","l1","l2","l3","l4","─","❯"]));
        l.output(&grid(&["l-5","l-4","l-3","l-2","l-1","l0","l1","─","❯"]));
        assert_eq!(l.line_text(6), Some("l7"), "it left the screen without moving with a shift");
        assert_eq!(l.line_text(7), None, "the prompt box stayed put");
    }

    #[test]
    fn the_jump_to_bottom_overlay_never_reaches_the_record_on_a_drag_downward() {
        // The overlay sits on the last content row; a drag downward brings rows in under it.
        let mut l = Ledger::open(grid(&["l1","l2","l3","l4","l5","l6   Jump to bottom","status"]), 0, 0);
        l.output(&grid(&["l2","l3","l4","l5","l6","l7   Jump to bottom","status"]));
        assert_eq!(l.line_text(5), Some("l6"), "the band's row is the clean one above the overlay");
        l.output(&grid(&["l3","l4","l5","l6","l7","l8   Jump to bottom","status"]));
        assert_eq!(l.line_text(5), Some("l6"));
        assert_eq!(l.line_text(6), Some("l7"));
        assert_eq!(l.line_text(7), None, "the overlay row itself is not recorded yet");
    }

    #[test]
    fn a_frame_split_across_two_batches_gives_the_same_record_as_the_frame_in_one() {
        let before = grid(&["l10","l11","l12","l13","l14","l15","l16","l17","l18","l19","status"]);
        let whole = grid(&["l7","l8","l9","l10","l11","l12","l13","l14","l15","l16","status"]);
        // The first read painted the top seven rows; the rest still show the old text.
        let half = grid(&["l7","l8","l9","l10","l11","l12","l13","l17","l18","l19","status"]);
        let next = grid(&["l4","l5","l6","l7","l8","l9","l10","l11","l12","l13","status"]);
        let mut split = Ledger::open(before.clone(), 9, 0);
        split.output(&half);
        split.output(&whole);
        split.output(&next);
        let mut one = Ledger::open(before, 9, 0);
        one.output(&whole);
        one.output(&next);
        assert_eq!(lines(&split, -6, 10), lines(&one, -6, 10));
        assert_eq!(split.offset(), one.offset());
    }

    #[test]
    fn a_pending_shift_reads_like_a_committed_one_with_chrome_left_out() {
        let c: Vec<String> = (0..50).map(|i| format!("l{i}")).collect();
        let c: Vec<&str> = c.iter().map(String::as_str).collect();
        // Ten content rows (l30..l39) over a two-row prompt box.
        let mut l = Ledger::open(scrolled(&c, 30, 12, &["─","❯"]), 0, 0);
        // One frame down (newer text came in at the bottom) and nothing after it: still pending.
        l.output(&scrolled(&c, 33, 12, &["─","❯"]));
        assert_eq!(l.offset(), -3);
        assert_eq!(l.moved_rows(), 0..10);
        assert_eq!(l.line_text(9), Some("l39"));
        // The band's rows read from the grid the shift was found on.
        assert_eq!(lines(&l, 10, 12), vec![Some("l40".into()), Some("l41".into()), Some("l42".into())]);
        // The prompt box's rows are lines 13 and 14 now and were never recorded: absent. (The
        // snapshot's own prompt box lines, 10 and 11, are read through the band above.)
        assert_eq!(lines(&l, 13, 14), vec![None, None]);
    }

    #[test]
    fn two_frames_in_consecutive_batches_commit_the_first() {
        let a = grid(&["l10","l11","l12","l13","l14","l15","status"]);
        let b = grid(&["l9","l10","l11","l12","l13","l14","status"]);
        let c = grid(&["l8","l9","l10","l11","l12","l13","status"]);
        let mut l = Ledger::open(a, 0, 0);
        l.output(&b);
        l.output(&c);
        assert_eq!(l.offset(), 2);
        assert_eq!(l.line_text(-1), Some("l9"));
        assert_eq!(l.line_text(-2), Some("l8"));
    }

    #[test]
    fn a_repaint_in_place_leaves_the_record_alone_and_a_redraw_beyond_recognition_closes_it() {
        let a = grid(&["l1","l2","l3","l4","l5","l6","⠋ thinking","status"]);
        let mut l = Ledger::open(a, 0, 0);
        assert_eq!(l.output(&grid(&["l1","l2","l3","l4","l5","l6","⠙ thinking","status"])), Outcome::Open);
        assert_eq!(l.offset(), 0);
        assert_eq!(l.output(&grid(&["x1","x2","x3","x4","x5","l6","⠹ thinking","status"])), Outcome::Closed);
    }

    #[test]
    fn scrolling_back_and_forth_keeps_the_rows_seen() {
        let a = grid(&["l10","l11","l12","l13","l14","l15","status"]);
        let b = grid(&["l8","l9","l10","l11","l12","l13","status"]);
        let mut l = Ledger::open(a.clone(), 0, 0);
        l.output(&b);
        l.output(&a);
        l.output(&b);
        l.output(&a);
        assert_eq!(l.offset(), 0);
        assert_eq!(l.line_text(0), Some("l10"));
        assert_eq!(l.line_text(-2), Some("l8"));
    }
}
