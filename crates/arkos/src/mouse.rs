//! What the mouse means to the program in the terminal: xterm's wheel and button reports, the
//! alternate-scroll arrows, or nothing (the view scrolls its own history), and the left button
//! between a press and its release. Pure, so it is tested. UTF-8 mouse coordinates (`?1005`) are
//! not supported: a program that asks for them gets X10 bytes.
use alacritty_terminal::index::Side;
use alacritty_terminal::selection::SelectionType;

/// How a wheel or button event reaches the program, from the modes it has set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseMode {
    /// No reporting: the wheel scrolls the local history and clicks stay local.
    Local,
    /// The alternate screen with alternate scroll (`?1007`, on by default) and no mouse
    /// reporting: a wheel step is an arrow key, as in xterm; clicks stay local.
    Arrows { app_cursor: bool },
    /// The program asked for mouse reports (`?1000`, `?1002` or `?1003`); `?1006` picks the
    /// SGR encoding over the X10 bytes.
    Report { sgr: bool },
}

pub const LEFT: u8 = 0;
const WHEEL_UP: u8 = 64;
const WHEEL_DOWN: u8 = 65;
/// X10 reports a release as button 3, whatever was pressed.
const X10_RELEASE: u8 = 3;

/// Carries the fraction of a line a trackpad leaves behind, so slow swipes add up to whole lines.
#[derive(Debug, Default, Clone, Copy)]
pub struct WheelCarry(f32);

impl WheelCarry {
    /// Adds `delta` lines and returns the whole lines now due, keeping the remainder.
    pub fn whole_lines(&mut self, delta: f32) -> i32 {
        let total = self.0 + delta;
        let whole = total.trunc();
        self.0 = total - whole;
        whole as i32
    }

    pub fn reset(&mut self) {
        self.0 = 0.0;
    }
}

/// The bytes for `lines` wheel steps over cell (`col`, `row`), positive meaning up. Empty in
/// `Local`, where the view scrolls its own history instead.
pub fn wheel_bytes(mode: MouseMode, lines: i32, col: usize, row: usize) -> Vec<u8> {
    let up = lines > 0;
    let mut out = Vec::new();
    for _ in 0..lines.unsigned_abs() {
        match mode {
            MouseMode::Local => return out,
            MouseMode::Arrows { app_cursor } => {
                out.extend_from_slice(if app_cursor { b"\x1bO" } else { b"\x1b[" });
                out.push(if up { b'A' } else { b'B' });
            }
            MouseMode::Report { sgr } => out.extend(button_bytes(sgr, if up { WHEEL_UP } else { WHEEL_DOWN }, col, row, true)),
        }
    }
    out
}

/// One button report at cell (`col`, `row`), zero-based: SGR `ESC [ < b ; x ; y M` (`m` for a
/// release), or the X10 three bytes, whose coordinates stop at 223 cells.
pub fn button_bytes(sgr: bool, button: u8, col: usize, row: usize, press: bool) -> Vec<u8> {
    if sgr {
        return format!("\x1b[<{button};{};{}{}", col + 1, row + 1, if press { 'M' } else { 'm' }).into_bytes();
    }
    let coord = |n: usize| (n + 33).min(255) as u8;
    vec![0x1b, b'[', b'M', 32 + if press { button } else { X10_RELEASE }, coord(col), coord(row)]
}

/// The grid cell under a window point and the half of the cell the pointer is on. `origin` is
/// where the grid was painted, `cell` its cell size, `frac` the fraction of a row the rows were
/// painted down by (a smooth scroll in flight); the result clamps to the grid.
pub fn cell_under(x: f32, y: f32, origin: (f32, f32), cell: (f32, f32), frac: f32, cols: usize, rows: usize) -> (usize, usize, Side) {
    let col = ((x - origin.0) / cell.0).max(0.0);
    let row = ((y - origin.1) / cell.1 - frac).floor().max(0.0) as usize;
    let side = if col.fract() >= 0.5 { Side::Right } else { Side::Left };
    ((col.floor() as usize).min(cols.saturating_sub(1)), row.min(rows.saturating_sub(1)), side)
}

/// What a press or a move asks of the selection, decided from cells alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    Nothing,
    /// A single click: whatever was selected goes.
    Clear,
    /// A double or triple click: the word or the line at the cell.
    Select { ty: SelectionType, col: usize, row: usize, side: Side },
    /// The pointer left the pressed cell with the button down: characters from the anchor to here.
    Start { anchor: (usize, usize, Side), to: (usize, usize, Side) },
    /// The selection follows the pointer.
    Extend { col: usize, row: usize, side: Side },
}

/// The left button between a press and its release: a click the program hears at the release,
/// unless the press turns into a drag first, and the cell a drag starts from.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Clicks {
    pending: Option<(usize, usize)>,
    anchor: Option<(usize, usize)>,
}

impl Clicks {
    /// A left press at a cell. `report` says whether the program hears a single click: it reports
    /// the mouse, and this is not the click that focused the window.
    pub fn press(&mut self, col: usize, row: usize, side: Side, click_count: usize, report: bool) -> Gesture {
        self.anchor = Some((col, row));
        self.pending = None;
        match click_count {
            2 => Gesture::Select { ty: SelectionType::Semantic, col, row, side },
            n if n >= 3 => Gesture::Select { ty: SelectionType::Lines, col, row, side },
            _ => {
                if report {
                    self.pending = Some((col, row));
                }
                Gesture::Clear
            }
        }
    }

    /// A move to a cell; `left_down` is whether the left button is held, `selecting` whether a
    /// selection exists. Leaving the pressed cell makes the press a drag, which is no click.
    pub fn moved(&mut self, col: usize, row: usize, side: Side, left_down: bool, selecting: bool) -> Gesture {
        if !left_down {
            self.anchor = None;
            return Gesture::Nothing;
        }
        let Some((acol, arow)) = self.anchor else { return Gesture::Nothing };
        if selecting {
            return Gesture::Extend { col, row, side };
        }
        if (col, row) == (acol, arow) {
            return Gesture::Nothing;
        }
        self.pending = None;
        // The anchor's side faces the way the drag sets off, so the pressed cell is taken on a
        // straight drag; a drag that turns back past it leaves it out.
        let anchor_side = if (row, col) > (arow, acol) { Side::Left } else { Side::Right };
        Gesture::Start { anchor: (acol, arow, anchor_side), to: (col, row, side) }
    }

    /// A left release at a cell: the bytes of the held click, a press at its cell then a release
    /// here, when the program still reports the mouse.
    pub fn release(&mut self, col: usize, row: usize, mode: MouseMode) -> Option<Vec<u8>> {
        self.anchor = None;
        let (pcol, prow) = self.pending.take()?;
        let MouseMode::Report { sgr } = mode else { return None };
        let mut bytes = button_bytes(sgr, LEFT, pcol, prow, true);
        bytes.extend(button_bytes(sgr, LEFT, col, row, false));
        Some(bytes)
    }

    /// The press ends without a release here (outside the pane, or it was a Cmd+click on a
    /// link): the program hears nothing.
    pub fn cancel(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT: MouseMode = MouseMode::Report { sgr: true };

    #[test]
    fn cells_come_from_the_painted_origin_and_the_scroll_fraction() {
        let o = (10.0, 20.0);
        let c = (8.0, 16.0);
        assert_eq!(cell_under(10.0, 20.0, o, c, 0.0, 80, 24), (0, 0, Side::Left));
        assert_eq!(cell_under(10.0 + 8.0 * 2.5, 20.0 + 16.0 * 1.2, o, c, 0.0, 80, 24), (2, 1, Side::Right));
        // Rows painted half a row down: a point in the top half of row 1's slot is still row 0.
        assert_eq!(cell_under(10.0, 20.0 + 16.0 * 1.2, o, c, 0.5, 80, 24).1, 0);
        assert_eq!(cell_under(10.0, 20.0 + 16.0 * 1.7, o, c, 0.5, 80, 24).1, 1);
        // Outside the grid clamps to its edges.
        assert_eq!(cell_under(1000.0, 1000.0, o, c, 0.0, 80, 24), (79, 23, Side::Right));
        assert_eq!(cell_under(-5.0, -5.0, o, c, 0.0, 80, 24), (0, 0, Side::Left));
    }

    #[test]
    fn a_click_reaches_the_program_at_the_release() {
        let mut k = Clicks::default();
        assert_eq!(k.press(2, 3, Side::Left, 1, true), Gesture::Clear);
        assert_eq!(k.release(2, 3, REPORT), Some(b"\x1b[<0;3;4M\x1b[<0;3;4m".to_vec()));
        // Nothing is held after it.
        assert_eq!(k.release(2, 3, REPORT), None);
    }

    #[test]
    fn a_drag_is_a_selection_and_never_a_click() {
        let mut k = Clicks::default();
        k.press(2, 3, Side::Left, 1, true);
        assert_eq!(k.moved(2, 3, Side::Right, true, false), Gesture::Nothing, "still on the pressed cell");
        assert_eq!(k.moved(5, 3, Side::Right, true, false), Gesture::Start { anchor: (2, 3, Side::Left), to: (5, 3, Side::Right) });
        assert_eq!(k.moved(6, 3, Side::Left, true, true), Gesture::Extend { col: 6, row: 3, side: Side::Left });
        assert_eq!(k.release(6, 3, REPORT), None);
        // Backwards: the anchor's right side faces the drag.
        k.press(5, 3, Side::Left, 1, false);
        assert_eq!(k.moved(2, 3, Side::Left, true, false), Gesture::Start { anchor: (5, 3, Side::Right), to: (2, 3, Side::Left) });
        // Up a row, even to a later column, is backwards too.
        k.press(2, 3, Side::Left, 1, false);
        assert_eq!(k.moved(7, 2, Side::Left, true, false), Gesture::Start { anchor: (2, 3, Side::Right), to: (7, 2, Side::Left) });
    }

    #[test]
    fn double_and_triple_clicks_select_and_are_not_reported() {
        let mut k = Clicks::default();
        assert_eq!(k.press(2, 3, Side::Left, 2, true), Gesture::Select { ty: SelectionType::Semantic, col: 2, row: 3, side: Side::Left });
        assert_eq!(k.release(2, 3, REPORT), None);
        assert_eq!(k.press(2, 3, Side::Left, 3, true), Gesture::Select { ty: SelectionType::Lines, col: 2, row: 3, side: Side::Left });
        assert_eq!(k.release(2, 3, REPORT), None);
    }

    #[test]
    fn nothing_is_sent_without_a_held_click() {
        let mut k = Clicks::default();
        assert_eq!(k.release(0, 0, REPORT), None);
        // A press the program does not hear (no reporting, or the focusing click) holds nothing.
        k.press(2, 3, Side::Left, 1, false);
        assert_eq!(k.release(2, 3, REPORT), None);
        // A held click is dropped when the program stops reporting before the release.
        k.press(2, 3, Side::Left, 1, true);
        assert_eq!(k.release(2, 3, MouseMode::Local), None);
    }

    #[test]
    fn a_cancel_or_a_release_elsewhere_drops_the_held_click() {
        let mut k = Clicks::default();
        k.press(2, 3, Side::Left, 1, true);
        k.cancel();
        assert_eq!(k.release(2, 3, REPORT), None);
        assert_eq!(k.moved(5, 3, Side::Left, true, false), Gesture::Nothing, "no anchor after a cancel");
        // The button came up somewhere else: the drag is over.
        k.press(2, 3, Side::Left, 1, true);
        assert_eq!(k.moved(5, 3, Side::Left, false, false), Gesture::Nothing);
        assert_eq!(k.moved(6, 3, Side::Left, true, false), Gesture::Nothing);
    }

    #[test]
    fn local_mode_sends_nothing() {
        assert!(wheel_bytes(MouseMode::Local, 3, 0, 0).is_empty());
        assert!(wheel_bytes(MouseMode::Local, -3, 0, 0).is_empty());
    }

    #[test]
    fn alternate_scroll_is_one_arrow_per_line() {
        assert_eq!(wheel_bytes(MouseMode::Arrows { app_cursor: false }, 2, 0, 0), b"\x1b[A\x1b[A".to_vec());
        assert_eq!(wheel_bytes(MouseMode::Arrows { app_cursor: false }, -1, 0, 0), b"\x1b[B".to_vec());
        assert_eq!(wheel_bytes(MouseMode::Arrows { app_cursor: true }, 1, 0, 0), b"\x1bOA".to_vec());
    }

    #[test]
    fn sgr_wheel_reports_the_cell_one_based() {
        assert_eq!(wheel_bytes(MouseMode::Report { sgr: true }, 1, 4, 9), b"\x1b[<64;5;10M".to_vec());
        assert_eq!(wheel_bytes(MouseMode::Report { sgr: true }, -2, 0, 0), b"\x1b[<65;1;1M\x1b[<65;1;1M".to_vec());
    }

    #[test]
    fn x10_wheel_uses_the_three_byte_form() {
        assert_eq!(wheel_bytes(MouseMode::Report { sgr: false }, 1, 0, 0), vec![0x1b, b'[', b'M', 96, 33, 33]);
        assert_eq!(wheel_bytes(MouseMode::Report { sgr: false }, -1, 2, 3), vec![0x1b, b'[', b'M', 97, 35, 36]);
    }

    #[test]
    fn clicks_report_press_and_release() {
        assert_eq!(button_bytes(true, LEFT, 2, 3, true), b"\x1b[<0;3;4M".to_vec());
        assert_eq!(button_bytes(true, LEFT, 2, 3, false), b"\x1b[<0;3;4m".to_vec());
        assert_eq!(button_bytes(false, LEFT, 2, 3, true), vec![0x1b, b'[', b'M', 32, 35, 36]);
        assert_eq!(button_bytes(false, LEFT, 2, 3, false), vec![0x1b, b'[', b'M', 35, 35, 36]);
        // X10 coordinates are bytes: a far column stays a byte.
        assert_eq!(button_bytes(false, LEFT, 500, 0, true)[4], 255);
    }

    #[test]
    fn the_carry_turns_fractions_into_whole_lines() {
        let mut c = WheelCarry::default();
        assert_eq!(c.whole_lines(0.4), 0);
        assert_eq!(c.whole_lines(0.4), 0);
        assert_eq!(c.whole_lines(0.4), 1);
        assert_eq!(c.whole_lines(-0.6), 0);
        assert_eq!(c.whole_lines(-0.6), -1);
        assert_eq!(c.whole_lines(2.5), 2);
        c.reset();
        assert_eq!(c.whole_lines(0.9), 0);
    }
}
