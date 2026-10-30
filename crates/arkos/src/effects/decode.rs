//! The terminal's decode: a cell whose character just changed shows random ASCII for a few
//! frames before it settles, and freshly settled text starts brighter and cools. Pure rules, no
//! drawing: the terminal asks what each cell shows and paints that.
use std::time::{Duration, Instant};

use super::hash::hash01;
use crate::layout::{CellKind, CellStyle};
use crate::palette::Rgb;
use crate::theme::{luminance, mix};

/// How long a changed cell scrambles while the agent is working, in milliseconds.
pub const DECODE_MS_WORKING: u64 = 400;
/// How long a changed cell scrambles when the agent is idle, in milliseconds.
pub const DECODE_MS_IDLE: u64 = 120;
/// How long freshly settled text takes to cool back to its own colour, in milliseconds.
pub const FRESH_MS: u64 = 1500;
/// How far toward `FRESH_WHITE` (or `FRESH_DEEP`) a cell's text colour starts when it settles.
pub const FRESH_MIX: f32 = 0.6;
/// The characters a scrambling cell draws from.
pub const DECODE_CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789@#$%&*+=<>/\\|";
/// The colour fresh text is brightened toward.
pub const FRESH_WHITE: Rgb = Rgb { r: 217, g: 255, b: 217 };
/// The colour fresh dark text (a light theme's) deepens toward, so it stays dark on its paper.
pub const FRESH_DEEP: Rgb = Rgb { r: 0, g: 90, b: 30 };
/// The luminance (`crate::theme::luminance`) under which the matrix glyph shader draws text as
/// dark, its `bool dark = luminance < 0.45;`, and fresh text deepens rather than whitens.
pub const DARK_TEXT: f32 = 0.45;
/// The step of the frame index that picks the random character, in milliseconds.
pub const TICK_MS: u64 = 50;

/// How long a cell scrambles and how long it stays brightened after it changes.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Timing {
    pub decode: Duration,
    pub fresh: Duration,
}

/// The timing for the agent's state; with Reduce Motion both are zero, so a cell settles at once.
pub fn timing(working: bool, reduce_motion: bool) -> Timing {
    if reduce_motion {
        return Timing { decode: Duration::ZERO, fresh: Duration::ZERO };
    }
    let ms = if working { DECODE_MS_WORKING } else { DECODE_MS_IDLE };
    Timing { decode: Duration::from_millis(ms), fresh: Duration::from_millis(FRESH_MS) }
}

/// What the decode remembers of one cell: its real character and when that last changed, in
/// nanoseconds since the state's epoch.
#[derive(Clone, Copy)]
struct Seen {
    ch: char,
    changed: Option<u64>,
}

/// The remembered grid of one terminal.
pub struct Decode {
    epoch: Instant,
    cols: usize,
    rows: usize,
    display_offset: usize,
    /// False until the first grid arrives: that one is the baseline, not a change.
    seeded: bool,
    cells: Vec<Seen>,
}

impl Decode {
    /// A state for a grid of this size with no cell marked changed.
    pub fn new(cols: usize, rows: usize, display_offset: usize) -> Decode {
        Decode {
            epoch: Instant::now(),
            cols,
            rows,
            display_offset,
            seeded: false,
            cells: vec![Seen { ch: ' ', changed: None }; cols * rows],
        }
    }

    /// Takes in the grid as it is now. A new size or display offset resets to this grid with no
    /// cell changed, as does the very first grid; otherwise each cell whose character differs is
    /// marked changed at `now`, or cleared when it became a space. The first row sets the width:
    /// a shorter row reads as spaces past its end, and a longer one's extra cells are left out.
    pub fn observe(&mut self, rows: &[Vec<(char, CellStyle, CellKind)>], display_offset: usize, now: Instant) {
        let cols = rows.first().map_or(0, |r| r.len());
        let reset = !self.seeded || rows.len() != self.rows || cols != self.cols || display_offset != self.display_offset;
        if reset {
            self.rows = rows.len();
            self.cols = cols;
            self.display_offset = display_offset;
            self.seeded = true;
            self.cells = rows
                .iter()
                .flat_map(|r| (0..cols).map(move |c| Seen { ch: r.get(c).map_or(' ', |x| x.0), changed: None }))
                .collect();
            return;
        }
        let nanos = now.saturating_duration_since(self.epoch).as_nanos() as u64;
        for (row, line) in rows.iter().enumerate() {
            for (col, cell) in line.iter().enumerate().take(self.cols) {
                let Some(seen) = self.cells.get_mut(row * self.cols + col) else {
                    continue;
                };
                if seen.ch != cell.0 {
                    seen.ch = cell.0;
                    seen.changed = if cell.0 == ' ' { None } else { Some(nanos) };
                }
            }
        }
    }

    /// What this cell shows now: its own character and style, or a random character while it
    /// decodes, with the colour its text is drawn in brightened while it is fresh.
    pub fn shown(
        &self,
        col: usize,
        row: usize,
        cell: (char, CellStyle, CellKind),
        now: Instant,
        t: Timing,
    ) -> (char, CellStyle) {
        let own = (cell.0, cell.1);
        if col >= self.cols || row >= self.rows || cell.2 == CellKind::Spacer {
            return own;
        }
        let Some(changed) = self.cells.get(row * self.cols + col).and_then(|s| s.changed) else {
            return own;
        };
        let at = self.epoch + Duration::from_nanos(changed);
        let since = now.saturating_duration_since(at);
        let mut ch = cell.0;
        if since < t.decode && ch != ' ' {
            ch = random_char(col, row, changed, since.as_millis() as u64 / TICK_MS);
        }
        let mut style = cell.1;
        if since < t.fresh {
            // The text is drawn in `bg` once `layout::build_runs` swaps an inverse cell's colours.
            let text = if style.inverse { &mut style.bg } else { &mut style.fg };
            *text = freshened(*text, fresh_mix(since, t.fresh));
        }
        (ch, style)
    }
}

/// A text colour `share` of the way to fresh: toward `FRESH_DEEP` when the glyph shader draws it
/// as dark text (under `DARK_TEXT`), so a light theme's text stays dark, else toward `FRESH_WHITE`.
fn freshened(text: Rgb, share: f32) -> Rgb {
    let target = if luminance(text) < DARK_TEXT { FRESH_DEEP } else { FRESH_WHITE };
    mix(text, target, share)
}

/// The random character for a cell: a pure function of the cell, the instant it changed and the
/// frame, so a frame drawn twice agrees with itself.
pub fn random_char(col: usize, row: usize, changed_nanos: u64, frame: u64) -> char {
    let mut h = (col as u32).wrapping_mul(0x9E37_79B1);
    h = (h ^ (row as u32)).wrapping_mul(0x85EB_CA6B);
    h = (h ^ (changed_nanos as u32) ^ ((changed_nanos >> 32) as u32)).wrapping_mul(0xC2B2_AE35);
    h = (h ^ (frame as u32) ^ ((frame >> 32) as u32)).wrapping_mul(0x27D4_EB2F);
    let chars = DECODE_CHARS.as_bytes();
    let i = ((hash01(h ^ (h >> 15)) * chars.len() as f32) as usize).min(chars.len() - 1);
    chars[i] as char
}

/// How far toward its fresh colour fresh text is, falling from `FRESH_MIX` to nothing over `fresh`.
pub fn fresh_mix(since: Duration, fresh: Duration) -> f32 {
    if fresh.is_zero() {
        return 0.0;
    }
    (FRESH_MIX * (1.0 - since.as_secs_f32() / fresh.as_secs_f32())).clamp(0.0, FRESH_MIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{CellKind, CellStyle};
    use crate::palette::Rgb;
    use std::time::{Duration, Instant};

    fn plain() -> CellStyle { CellStyle { fg: Rgb { r: 74, g: 246, b: 38 }, bg: Rgb { r: 0, g: 0, b: 0 }, bold: false, italic: false, dim: false, underline: false, inverse: false } }
    fn cell(c: char) -> (char, CellStyle, CellKind) { (c, plain(), CellKind::Normal) }
    fn grid(s: &[&str]) -> Vec<Vec<(char, CellStyle, CellKind)>> { s.iter().map(|r| r.chars().map(cell).collect()).collect() }

    #[test]
    fn working_decodes_longer_than_idle_and_reduce_motion_settles_at_once() {
        assert_eq!(timing(true, false), Timing { decode: Duration::from_millis(400), fresh: Duration::from_millis(1500) });
        assert_eq!(timing(false, false).decode, Duration::from_millis(120));
        assert_eq!(timing(true, true), Timing { decode: Duration::ZERO, fresh: Duration::ZERO });
    }

    #[test]
    fn a_changed_cell_shows_random_characters_then_settles_on_its_own() {
        let t0 = Instant::now();
        let mut d = Decode::new(3, 1, 0);
        d.observe(&grid(&["   "]), 0, t0);
        d.observe(&grid(&["ok "]), 0, t0);
        let t = timing(true, false);
        let (c, _) = d.shown(0, 0, cell('o'), t0 + Duration::from_millis(100), t);
        assert_ne!(c, 'o'); assert!(c.is_ascii() && DECODE_CHARS.contains(c));
        let (c2, _) = d.shown(0, 0, cell('o'), t0 + Duration::from_millis(160), t);
        assert!(DECODE_CHARS.contains(c2), "every frame draws from DECODE_CHARS");
        assert_eq!(d.shown(0, 0, cell('o'), t0 + Duration::from_millis(401), t).0, 'o');
        // The cell that stayed a space is untouched.
        assert_eq!(d.shown(2, 0, cell(' '), t0 + Duration::from_millis(100), t).0, ' ');
    }

    #[test]
    fn the_first_grid_over_existing_text_is_a_baseline_and_decodes_nothing() {
        let t0 = Instant::now();
        let mut d = Decode::new(3, 2, 0);
        let text = grid(&["abc", "def"]);
        d.observe(&text, 0, t0);
        let t = timing(true, false);
        for (row, line) in text.iter().enumerate() {
            for (col, &c) in line.iter().enumerate() {
                assert_eq!(d.shown(col, row, c, t0 + Duration::from_millis(10), t), (c.0, c.1), "cell {col},{row}");
            }
        }
    }

    #[test]
    fn a_ragged_grid_never_panics_and_a_short_row_reads_as_spaces() {
        let t0 = Instant::now();
        let mut d = Decode::new(3, 2, 0);
        d.observe(&grid(&["abc", "d"]), 0, t0);
        d.observe(&grid(&["abc", "dxyzw"]), 0, t0);
        let t = timing(true, false);
        let (c, _) = d.shown(1, 1, cell('x'), t0 + Duration::from_millis(10), t);
        assert_ne!(c, 'x', "past the short row's end was a space, so the x is a change");
        assert_eq!(d.shown(4, 1, cell('w'), t0 + Duration::from_millis(10), t).0, 'w', "past the width: its own");
        d.observe(&grid(&["abc"]), 0, t0);
        d.observe(&grid(&["abc", ""]), 0, t0);
    }

    #[test]
    fn an_unchanged_cell_is_never_touched_and_a_cleared_one_clears_at_once() {
        let t0 = Instant::now();
        let mut d = Decode::new(2, 1, 0);
        d.observe(&grid(&["ab"]), 0, t0);
        d.observe(&grid(&["a "]), 0, t0 + Duration::from_millis(10));
        let t = timing(true, false);
        assert_eq!(d.shown(0, 0, cell('a'), t0 + Duration::from_millis(20), t).0, 'a');
        assert_eq!(d.shown(0, 0, cell('a'), t0 + Duration::from_millis(20), t).1, plain(), "not fresh: its colour is its own");
        assert_eq!(d.shown(1, 0, cell(' '), t0 + Duration::from_millis(20), t).0, ' ');
    }

    #[test]
    fn a_wide_character_decodes_as_one_ascii_glyph_in_its_cell_and_its_spacer_never() {
        let t0 = Instant::now();
        let mut d = Decode::new(2, 1, 0);
        d.observe(&grid(&["  "]), 0, t0);
        let wide = vec![vec![('漢', plain(), CellKind::Wide), (' ', plain(), CellKind::Spacer)]];
        d.observe(&wide, 0, t0);
        let t = timing(false, false);
        let (c, _) = d.shown(0, 0, wide[0][0], t0 + Duration::from_millis(50), t);
        assert!(c.is_ascii() && c != '漢');
        assert_eq!(d.shown(1, 0, wide[0][1], t0 + Duration::from_millis(50), t).0, ' ');
        assert_eq!(d.shown(0, 0, wide[0][0], t0 + Duration::from_millis(121), t).0, '漢');
    }

    #[test]
    fn fresh_text_starts_brighter_and_cools_over_a_second_and_a_half() {
        assert!((fresh_mix(Duration::ZERO, Duration::from_millis(1500)) - 0.6).abs() < 1e-6);
        assert!((fresh_mix(Duration::from_millis(750), Duration::from_millis(1500)) - 0.3).abs() < 1e-6);
        assert_eq!(fresh_mix(Duration::from_millis(1500), Duration::from_millis(1500)), 0.0);
        assert_eq!(fresh_mix(Duration::ZERO, Duration::ZERO), 0.0, "Reduce Motion: never brightened");
        let t0 = Instant::now();
        let mut d = Decode::new(1, 1, 0);
        d.observe(&grid(&[" "]), 0, t0);
        d.observe(&grid(&["x"]), 0, t0);
        let t = timing(false, false);
        let own = plain();
        let (_, s) = d.shown(0, 0, ('x', own, CellKind::Normal), t0 + Duration::from_millis(121), t);
        assert_eq!(s.fg, crate::theme::mix(own.fg, FRESH_WHITE, fresh_mix(Duration::from_millis(121), t.fresh)));
        assert_eq!(s.bg, own.bg, "only the foreground brightens");
        assert_eq!(d.shown(0, 0, ('x', own, CellKind::Normal), t0 + Duration::from_millis(1600), t).1, own);
    }

    /// A cell that changed from a space at `t0` and is drawn with `style` at 121 ms, past the
    /// idle decode, and the fresh share it is drawn with then.
    fn fresh_at_121_ms(style: CellStyle) -> (CellStyle, f32) {
        let t0 = Instant::now();
        let mut d = Decode::new(1, 1, 0);
        d.observe(&grid(&[" "]), 0, t0);
        d.observe(&grid(&["x"]), 0, t0);
        let t = timing(false, false);
        let (_, s) = d.shown(0, 0, ('x', style, CellKind::Normal), t0 + Duration::from_millis(121), t);
        (s, fresh_mix(Duration::from_millis(121), t.fresh))
    }

    #[test]
    fn fresh_dark_text_deepens_toward_the_deep_green_and_bright_text_whitens() {
        // Peach Light's foreground, under the shader's dark-text threshold: whitened it would
        // fall to about 2:1 on its paper and take the emissive branch.
        let dark = CellStyle { fg: Rgb { r: 0x4a, g: 0x30, b: 0x28 }, bg: Rgb { r: 0xff, g: 0xf6, b: 0xef }, ..plain() };
        assert!(crate::theme::luminance(dark.fg) < DARK_TEXT);
        let (s, share) = fresh_at_121_ms(dark);
        assert_eq!(s.fg, crate::theme::mix(dark.fg, FRESH_DEEP, share));
        assert!(crate::theme::luminance(s.fg) < DARK_TEXT, "fresh dark text stays on the dark branch");
        let bright = plain();
        assert!(crate::theme::luminance(bright.fg) >= DARK_TEXT);
        let (s, share) = fresh_at_121_ms(bright);
        assert_eq!(s.fg, crate::theme::mix(bright.fg, FRESH_WHITE, share));
    }

    #[test]
    fn a_fresh_inverse_cell_brightens_its_background_which_its_text_is_drawn_in() {
        let inverse = CellStyle { inverse: true, ..plain() };
        let (s, share) = fresh_at_121_ms(inverse);
        assert_eq!(s.fg, inverse.fg, "the block, drawn in fg once swapped, keeps its colour");
        // The background is black, so the text drawn in it is dark and deepens.
        assert_eq!(s.bg, crate::theme::mix(inverse.bg, FRESH_DEEP, share));
        let bright_text = CellStyle { inverse: true, bg: Rgb { r: 200, g: 255, b: 200 }, ..plain() };
        let (s, share) = fresh_at_121_ms(bright_text);
        assert_eq!(s.bg, crate::theme::mix(bright_text.bg, FRESH_WHITE, share));
        assert_eq!(s.fg, bright_text.fg);
        let normal = plain();
        let (s, share) = fresh_at_121_ms(normal);
        assert_eq!(s.fg, crate::theme::mix(normal.fg, FRESH_WHITE, share));
        assert_eq!(s.bg, normal.bg, "a normal cell's background is untouched");
    }

    #[test]
    fn a_resize_or_a_scroll_into_history_resets_the_state_without_marking_cells_changed() {
        let t0 = Instant::now();
        let mut d = Decode::new(2, 1, 0);
        d.observe(&grid(&["ab"]), 0, t0);
        d.observe(&grid(&["abc"]), 0, t0);               // wider: a fresh grid
        let t = timing(true, false);
        assert_eq!(d.shown(2, 0, cell('c'), t0 + Duration::from_millis(10), t).0, 'c');
        d.observe(&grid(&["xyz"]), 3, t0);               // scrolled into history: never fresh
        assert_eq!(d.shown(0, 0, cell('x'), t0 + Duration::from_millis(10), t).0, 'x');
        d.observe(&grid(&["abc"]), 0, t0);               // back to the live screen: no flash
        assert_eq!(d.shown(0, 0, cell('a'), t0 + Duration::from_millis(10), t).0, 'a');
        // Out-of-range cells (a grid shrunk between observe and shown) are drawn as their own.
        assert_eq!(d.shown(9, 9, cell('q'), t0, t).0, 'q');
    }

    #[test]
    fn the_random_character_is_ascii_and_predicted_by_the_cell_the_instant_and_the_frame() {
        let a = random_char(3, 4, 123_456_789, 2);
        assert_eq!(a, random_char(3, 4, 123_456_789, 2));
        assert!(DECODE_CHARS.contains(a));
        let frames: std::collections::HashSet<char> = (0..40).map(|f| random_char(3, 4, 123_456_789, f)).collect();
        assert!(frames.len() > 5, "the frames vary");
    }
}
