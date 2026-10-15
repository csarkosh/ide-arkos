//! What the mouse means to the program in the terminal: xterm's wheel and button reports, the
//! alternate-scroll arrows, or nothing (the view scrolls its own history). Pure, so it is tested.

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

#[cfg(test)]
mod tests {
    use super::*;

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
