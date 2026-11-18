//! The terminal state behind one tab: an alacritty grid fed by the daemon, plus scroll state.
//! `TerminalCore` has no GPUI dependency so its behaviour is unit tested; `Terminal` wraps it as
//! a GPUI entity and owns the socket side.
use crate::client::Client;
use crate::effects::decode::USER_QUIET_MS;
use crate::keys::{keystroke_to_bytes, KeyMode};
use crate::layout::{CellKind, CellStyle};
use crate::ledger::{self, End, Ledger, Outcome};
use crate::mouse::{self, Clicks, Gesture, MouseMode, WheelCarry};
use crate::palette::{Palette, NAVY};
use crate::scroll::ScrollAnimator;
use alacritty_terminal::event::{Event as TermEvent, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Boundary, Column, Direction, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
pub use alacritty_terminal::term::search::Match;
use alacritty_terminal::term::search::RegexSearch;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{CursorShape, Processor};
use gpui::{Context, FocusHandle, Keystroke};
use protocol::{Event, Request, SessionId};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Collects alacritty events during one `advance` so they can be reported after it. Shared
/// through `Arc<Mutex>` so the core is `Send` and a replay can be parsed off the main thread;
/// the lock is never contended.
#[derive(Clone, Default)]
struct Listener {
    events: Arc<Mutex<VecDeque<TermEvent>>>,
}

impl EventListener for Listener {
    fn send_event(&self, event: TermEvent) {
        self.events.lock().unwrap().push_back(event);
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Wakeup {
    pub repaint: bool,
    pub title: Option<String>,
    pub bell: bool,
    pub pty_write: Vec<u8>,
}

pub struct RowsForPaint {
    pub display_offset: usize,
    /// The line just above the viewport, present when scrolled into history.
    pub above: Option<Vec<(char, CellStyle, CellKind)>>,
    pub visible: Vec<Vec<(char, CellStyle, CellKind)>>,
    /// Every link in the visible rows, one span per row it takes, URLs wrapped across rows and
    /// across the viewport's edges included.
    pub links: Vec<crate::links::Span>,
}

/// The rows `TerminalCore::pinned_frame` holds at the pane's foot, and the cursor in them as
/// (column, row of these).
#[derive(Debug, Clone, PartialEq)]
pub struct PinnedFrame {
    pub rows: Vec<Vec<(char, CellStyle, CellKind)>>,
    pub cursor: Option<(usize, usize)>,
}

/// Claude Code's input box among a screen's rows, as (the first row, the last row): from the
/// rule over its prompt to the last row with anything on it, so the status lines under the box
/// come with it. The box is the last row that opens with the prompt `\u{276f}` and hangs under
/// a rule; a past prompt in the conversation opens with the same mark under no rule.
pub(crate) fn input_frame(rows: &[Vec<char>]) -> Option<(usize, usize)> {
    let written = |row: &Vec<char>| row.iter().any(|c| !c.is_whitespace());
    let rule = |row: &Vec<char>| {
        let dashes = row.iter().filter(|c| **c == '\u{2500}').count();
        dashes >= 8 && row.iter().all(|c| *c == '\u{2500}' || c.is_whitespace())
    };
    let last = rows.iter().rposition(written)?;
    let prompt = (1..=last).rev().find(|&i| rows[i].first() == Some(&'\u{276f}') && rule(&rows[i - 1]))?;
    Some((prompt - 1, last))
}

/// Whether a key sent to the program brings the view back to the live bottom. Every key does,
/// as in any terminal, except while Claude Code's input box is pinned at the pane's foot: what
/// is typed shows there, so the place in the history is kept, and Enter goes to the bottom,
/// where the answer will be.
pub(crate) fn key_returns_to_bottom(pinned: bool, key: &str) -> bool {
    !pinned || key == "enter"
}

pub const MAX_MATCHES: usize = 10_000;

/// Backslash-escapes every regex metacharacter so the query matches literally.
pub fn escape_regex(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A match's part on one viewport row, columns inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Highlight {
    pub row: usize,
    pub start: usize,
    pub end: usize,
    pub current: bool,
}

/// The selection's part on one viewport row, columns inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionSpan {
    pub row: usize,
    pub start: usize,
    pub end: usize,
}

/// One frame of a drag held past the pane's edge: whether another frame can do more, and the
/// bytes to send the program (its wheel reports), empty when it is sent nothing.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DragStep {
    pub more: bool,
    pub pty_write: Vec<u8>,
}

pub struct TerminalCore {
    term: Term<Listener>,
    parser: Processor,
    listener: Listener,
    palette: Arc<Palette>,
    pub scroll: ScrollAnimator,
    /// The program asked for theme-change pushes (`CSI ? 2031 h`, contour's colour palette
    /// update notifications; alacritty_terminal ignores the mode, so it is read here). Claude
    /// Code's Auto theme does, and then hears `CSI ? 997 ; 1 n` (dark) or `; 2 n` (light) at
    /// every Arkos theme switch, so its colours follow without a restart.
    theme_updates: bool,
    /// The end of the last chunk when it stopped inside a `CSI ?` sequence, read with the next.
    csi_tail: Vec<u8>,
    /// The record of a selection made in a program that reports the mouse on the alternate
    /// screen, open from the press until the selection is cleared (see `ledger::Ledger`). While
    /// it is open it is the selection, and alacritty's selection is only its tint.
    ledger: Option<Ledger>,
    /// The fraction of a line a drag held past the edge owes the program, reset at each press.
    drag_carry: WheelCarry,
    /// The agent in this terminal said it runs here (see `set_agent_redraws`).
    agent_redraws: bool,
    /// How many of the history's newest rows the program on the main screen printed at the
    /// width the grid has now: rows that scrolled off since the last change of width, and rows a
    /// shorter pane sent up. A lower bound (at its limit the history's size says nothing of what
    /// arrived), which is the safe side: see `resize_under_a_redrawing_program`.
    fresh_rows: usize,
    /// Resizes the redrawing program has not yet answered with a repaint, so the next erase of
    /// its screen is the answer and not something to keep (`before_the_program_erases_its_screen`).
    repaints_owed: usize,
    /// Output applied since the last resize or the last answer to one.
    since_resize: usize,
    /// How much of `SCREEN_ERASE` ends the output applied so far.
    erase_matched: usize,
}

/// Cursor home, then erase line: how Claude Code's classic renderer begins erasing its whole
/// screen, a row at a time, before it prints a frame from the top.
const SCREEN_ERASE: &[u8] = b"\x1b[H\x1b[2K";

/// A repaint this much output after a resize is not the resize's answer: the program answers at
/// once, or (its terminal's size unchanged, or two resizes heard as one) not at all.
const RESIZE_ANSWER_BYTES: usize = 16 * 1024;

/// `bytes` in slices, each after the first beginning with `mark`.
fn cut_before<'a>(bytes: &'a [u8], mark: &[u8]) -> Vec<&'a [u8]> {
    let mut slices = Vec::new();
    let mut from = 0;
    while let Some(at) = bytes.get(from + 1..).and_then(|rest| rest.windows(mark.len()).position(|w| w == mark)).map(|p| p + from + 1) {
        slices.push(&bytes[from..at]);
        from = at;
    }
    if from < bytes.len() {
        slices.push(&bytes[from..]);
    }
    slices
}

/// How much of `SCREEN_ERASE` ends the output once `byte` follows output that ended with
/// `matched` bytes of it.
fn erase_match_after(matched: usize, byte: u8) -> usize {
    let mut k = matched;
    loop {
        if SCREEN_ERASE[k] == byte {
            return k + 1;
        }
        if k == 0 {
            return 0;
        }
        // The longest shorter start of the sequence that the bytes seen so far also end with.
        k = (0..k).rev().find(|&n| SCREEN_ERASE[..n] == SCREEN_ERASE[k - n..k]).unwrap_or(0);
    }
}

/// The longest `CSI ?` sequence the theme scan waits for across chunks; anything longer is not
/// one it reads.
const CSI_TAIL_MAX: usize = 32;

/// What a `CSI ? … ` sequence in the output asked of the theme scan.
#[derive(Debug, Default, PartialEq, Eq)]
struct ThemeAsks {
    /// `Some(true)` when mode 2031 was set, `Some(false)` when reset, the last one winning.
    updates: Option<bool>,
    /// `CSI ? 996 n`: which theme is this, dark or light?
    query: bool,
}

/// Reads `CSI ? <params> h|l|n` sequences out of `tail` followed by `bytes`; returns what they
/// asked and the unfinished end to carry into the next chunk.
fn scan_theme_asks(tail: &[u8], bytes: &[u8]) -> (ThemeAsks, Vec<u8>) {
    let data: Vec<u8> = tail.iter().chain(bytes).copied().collect();
    let mut asks = ThemeAsks::default();
    let mut i = 0;
    while let Some(at) = data[i..].iter().position(|&b| b == 0x1b).map(|p| p + i) {
        let rest = &data[at..];
        if rest.len() < 3 {
            // ESC, or ESC [, at the very end: maybe the start of one.
            if rest == b"\x1b" || rest == b"\x1b[" {
                return (asks, rest.to_vec());
            }
            break;
        }
        if &rest[..3] != b"\x1b[?" {
            i = at + 1;
            continue;
        }
        let params_len = rest[3..].iter().take_while(|b| b.is_ascii_digit() || **b == b';').count();
        let Some(&end) = rest.get(3 + params_len) else {
            // Unfinished: carry it, unless it is too long to be one of ours.
            return (asks, if rest.len() <= CSI_TAIL_MAX { rest.to_vec() } else { Vec::new() });
        };
        let params = std::str::from_utf8(&rest[3..3 + params_len]).unwrap_or("");
        match end {
            b'h' | b'l' if params.split(';').any(|p| p == "2031") => asks.updates = Some(end == b'h'),
            b'n' if params == "996" => asks.query = true,
            _ => {}
        }
        i = at + 3 + params_len + 1;
    }
    (asks, Vec::new())
}

/// The theme-change push for a palette: `CSI ? 997 ; 1 n` on a dark background, `; 2 n` on a
/// light one (relative luminance under half is dark).
fn theme_report(palette: &Palette) -> &'static [u8] {
    let bg = palette.bg;
    let luma = 0.2126 * f32::from(bg.r) + 0.7152 * f32::from(bg.g) + 0.0722 * f32::from(bg.b);
    if luma < 128.0 {
        b"\x1b[?997;1n"
    } else {
        b"\x1b[?997;2n"
    }
}

impl TerminalCore {
    pub fn new(cols: usize, rows: usize, scrollback: usize) -> Self {
        let listener = Listener::default();
        // Kitty keyboard flags are tracked so a program that sets them (Claude Code does) gets
        // the key encoding it asked for; see `key_mode`.
        let config = Config { scrolling_history: scrollback, kitty_keyboard: true, ..Config::default() };
        let term = Term::new(config, &TermSize::new(cols, rows), listener.clone());
        Self { term, parser: Processor::new(), listener, palette: Arc::new(NAVY), scroll: ScrollAnimator::new(), theme_updates: false, csi_tail: Vec::new(), ledger: None, drag_carry: WheelCarry::default(), agent_redraws: false, fresh_rows: 0, repaints_owed: 0, since_resize: 0, erase_matched: 0 }
    }

    pub fn apply_output(&mut self, bytes: &[u8]) -> Wakeup {
        let before = self.term.grid().display_offset();
        let (asks, tail) = scan_theme_asks(&self.csi_tail, bytes);
        self.csi_tail = tail;
        if let Some(on) = asks.updates {
            self.theme_updates = on;
        }
        let history = self.term.grid().history_size();
        self.advance(bytes);
        self.flush_sync_if_due();
        self.fresh_rows = (self.fresh_rows + self.term.grid().history_size().saturating_sub(history)).min(self.term.grid().history_size());
        // Alacritty raises display_offset as lines arrive while scrolled up; keep the animator
        // in step so the next frame does not pull the view down.
        let after = self.term.grid().display_offset();
        if after != before {
            let d = after as f32 - before as f32;
            self.scroll.current += d;
            self.scroll.target += d;
        }
        let mut w = Wakeup { repaint: !bytes.is_empty(), ..Default::default() };
        if asks.query {
            w.pty_write.extend_from_slice(theme_report(&self.palette));
        }
        let events: Vec<TermEvent> = self.listener.events.lock().unwrap().drain(..).collect();
        for ev in events {
            match ev {
                TermEvent::Title(t) => w.title = Some(t),
                TermEvent::ResetTitle => w.title = Some(String::new()),
                TermEvent::Bell => w.bell = true,
                TermEvent::PtyWrite(s) => w.pty_write.extend_from_slice(s.as_bytes()),
                // OSC 4/10/11/12 queries ("what colour is the background?"): answered from the
                // palette, so a program that adapts to a light or dark terminal (Claude Code,
                // vim, bat) sees the theme it runs in rather than guessing dark.
                TermEvent::ColorRequest(index, format) => {
                    if let Some(c) = self.query_color(index) {
                        w.pty_write.extend_from_slice(format(c).as_bytes());
                    }
                }
                _ => {}
            }
        }
        self.scroll.clamp_max(self.history_size() as f32);
        w
    }

    /// The colour a program's OSC query asks about: 0 to 255 the indexed colours, 256 the
    /// foreground, 257 the background, 258 the cursor; nothing for the rest.
    fn query_color(&self, index: usize) -> Option<alacritty_terminal::vte::ansi::Rgb> {
        let c = match index {
            0..=255 => self.palette.indexed(index as u8),
            256 => self.palette.fg,
            257 => self.palette.bg,
            258 => self.palette.cursor,
            _ => return None,
        };
        Some(alacritty_terminal::vte::ansi::Rgb { r: c.r, g: c.g, b: c.b })
    }

    /// Swaps the colours every cell resolves against at paint. Nothing is re-parsed. Returns the
    /// theme-change push to write to the program when it asked for them (`theme_updates`).
    pub fn set_palette(&mut self, palette: Arc<Palette>) -> Option<&'static [u8]> {
        self.palette = palette;
        self.theme_updates.then(|| theme_report(&self.palette))
    }

    /// Swaps the colours for painting without telling the program: the theme cross-fade sets a
    /// blended palette every frame, and the program hears one `set_palette` at the fade's end.
    pub fn set_palette_quiet(&mut self, palette: Arc<Palette>) {
        self.palette = palette;
    }

    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// Applies chunks in order and merges their wakeups into one. The chunks are one batch for
    /// an open record: it reads the grid after them (see `follow_output`).
    pub fn apply_all<'a>(&mut self, chunks: impl IntoIterator<Item = &'a [u8]>) -> Wakeup {
        let mut out = Wakeup::default();
        for c in chunks {
            let w = self.apply_output(c);
            out.repaint |= w.repaint;
            if w.title.is_some() {
                out.title = w.title;
            }
            out.bell |= w.bell;
            out.pty_write.extend_from_slice(&w.pty_write);
        }
        self.follow_output();
        out
    }

    /// Ends a synchronized update whose deadline has passed. Returns true when it flushed. The
    /// held rows reach the grid outside any batch, so an open record follows them here, as
    /// `apply_all` has it follow a batch.
    pub fn flush_sync_if_due(&mut self) -> bool {
        match self.parser.sync_timeout().sync_timeout() {
            Some(deadline) if deadline <= Instant::now() => {
                self.parser.stop_sync(&mut self.term);
                self.follow_output();
                true
            }
            _ => false,
        }
    }

    /// True while a synchronized update is open and its rows are held back.
    pub fn sync_pending(&self) -> bool {
        self.parser.sync_timeout().sync_timeout().is_some()
    }

    /// Feeds output to the terminal. Under a program that redraws its own screen, the output is
    /// watched for the start of an erase of the whole screen, and the screen is kept first
    /// (`before_the_program_erases_its_screen`): everything up to the erase's last byte is
    /// parsed, then the rows are moved, then the erase runs on what is left.
    fn advance(&mut self, bytes: &[u8]) {
        if !self.program_redraws() {
            self.erase_matched = 0;
            self.parser.advance(&mut self.term, bytes);
            return;
        }
        let mut fed = 0;
        for (i, &byte) in bytes.iter().enumerate() {
            self.erase_matched = erase_match_after(self.erase_matched, byte);
            if self.erase_matched == SCREEN_ERASE.len() {
                self.erase_matched = 0;
                self.parser.advance(&mut self.term, &bytes[fed..i]);
                self.since_resize = self.since_resize.saturating_add(i - fed);
                fed = i;
                self.before_the_program_erases_its_screen();
            }
        }
        self.parser.advance(&mut self.term, &bytes[fed..]);
        self.since_resize = self.since_resize.saturating_add(bytes.len() - fed);
    }

    /// Claude Code's classic renderer erases its whole screen and prints a frame from the top
    /// at a resize, and also when it compacts and at `/clear`. At a resize it prints the same
    /// conversation again, so the erased rows are not kept (`repaints_owed`). Unasked, the new
    /// frame no longer holds what was on screen, and the erase would be the end of it: the last
    /// screenful before every compaction, gone from the history. So those rows go to the history
    /// first, as if the program had scrolled them off. A repaint for a reason unknown here (one
    /// that prints the same rows again) leaves that screenful in the history twice, never lost.
    fn before_the_program_erases_its_screen(&mut self) {
        if self.repaints_owed > 0 && self.since_resize <= RESIZE_ANSWER_BYTES {
            self.repaints_owed -= 1;
            self.since_resize = 0;
            return;
        }
        self.repaints_owed = 0;
        let Some(last) = self.last_written_row() else { return };
        let rows = self.term.screen_lines();
        self.term.grid_mut().scroll_up(&(Line(0)..Line(rows as i32)), last + 1);
        // The grid moved under alacritty's feet, so its selection bookkeeping did not run.
        self.term.selection = None;
        self.fresh_rows += last + 1;
    }

    /// The last screen row with anything on it.
    fn last_written_row(&self) -> Option<usize> {
        use alacritty_terminal::grid::GridCell;
        let grid = self.term.grid();
        (0..grid.screen_lines()).rev().find(|&l| (0..grid.columns()).any(|c| !grid[Line(l as i32)][Column(c)].is_empty()))
    }

    /// Resizes the grid. A shell's screen is reflowed with its history, as in any terminal. A
    /// program that redraws its own screen (`program_redraws`) gets its screen left to it.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        // The program redraws for the new size, which no shift describes.
        self.ledger = None;
        let (cols, rows) = (cols.max(2), rows.max(1));
        if (cols, rows) == (self.columns(), self.screen_lines()) {
            return;
        }
        if self.program_redraws() {
            self.repaints_owed += 1;
            self.since_resize = 0;
            self.resize_under_a_redrawing_program(cols, rows);
        } else {
            self.term.resize(TermSize::new(cols, rows));
            self.fresh_rows = 0;
        }
        self.scroll.clamp_max(self.history_size() as f32);
    }

    /// True while the program on the main screen redraws its own screen at a resize and reckons
    /// on a terminal that left that screen alone: Claude Code's classic renderer, known by the
    /// theme pushes it asks for (`CSI ? 2031 h`, on until it leaves) or by its own hooks
    /// (`set_agent_redraws`). On the alternate screen there is no history to keep whole.
    pub fn program_redraws(&self) -> bool {
        !self.term.mode().contains(TermMode::ALT_SCREEN) && (self.theme_updates || self.agent_redraws)
    }

    /// The agent's own word (its SessionStart and SessionEnd hooks) that it runs in this
    /// terminal: a replay that no longer holds the start of its output cannot say so.
    pub fn set_agent_redraws(&mut self, on: bool) {
        self.agent_redraws = on;
    }

    /// A resize under a program that erases every screen row and prints its screen again (see
    /// `tests::Classic` for what Claude Code's classic renderer does, measured). The history must
    /// stay one whole conversation, so the grid changes the way the program reckons it does:
    ///
    /// - Shorter: the rows that no longer fit go to the history, counted from the end of what
    ///   the program printed, not from the cursor (alacritty keeps the cursor's row and drops the
    ///   rows under it; the program prints only the last rows again, so the rows between were
    ///   lost).
    /// - Taller: rows come back from the history, which the program then prints over, but only
    ///   rows it printed at this width (`fresh_rows`). An older row may hold more text than the
    ///   row the program prints in its place, and that text would be gone; left in the history
    ///   it is at worst there twice.
    /// - Narrower or wider: the screen's rows stay row for row, cut or padded, and only the
    ///   history is reflowed. The program prints the same text again wrapped for the new width;
    ///   reflowing the screen first would push part of it into the history, to be printed twice.
    fn resize_under_a_redrawing_program(&mut self, cols: usize, rows: usize) {
        let (old_cols, old_rows) = (self.columns(), self.screen_lines());
        if rows < old_rows {
            let push = self.used_rows().saturating_sub(rows);
            if push > 0 {
                let grid = self.term.grid_mut();
                grid.scroll_up(&(Line(0)..Line(old_rows as i32)), push);
                grid.cursor.point.line = Line((grid.cursor.point.line.0 - push as i32).max(0));
                // The grid moved under alacritty's feet, so its selection bookkeeping did not run.
                self.term.selection = None;
                self.fresh_rows += push;
            }
            self.term.resize(TermSize::new(old_cols, rows));
        } else if rows > old_rows {
            let added = rows - old_rows;
            // alacritty takes back as many rows as it adds, while the history has them.
            let taken = added.min(self.history_size());
            let wanted = taken.min(self.fresh_rows);
            self.term.resize(TermSize::new(old_cols, rows));
            let back = taken - wanted;
            if back > 0 {
                let grid = self.term.grid_mut();
                grid.scroll_up(&(Line(0)..Line(rows as i32)), back);
                grid.cursor.point.line = Line((grid.cursor.point.line.0 - back as i32).max(0));
                self.term.selection = None;
            }
            self.fresh_rows -= wanted;
        }
        if cols != old_cols {
            self.resize_columns_keeping_the_screen(cols);
            self.fresh_rows = 0;
        }
    }

    /// The screen rows the program has printed through: one past the last row with anything on
    /// it, for the row its frame's last newline left the cursor on before it moved up to its
    /// prompt, and never less than through the cursor's row.
    fn used_rows(&self) -> usize {
        let grid = self.term.grid();
        let through_cursor = grid.cursor.point.line.0.max(0) as usize + 1;
        self.last_written_row().map_or(0, |l| l + 2).max(through_cursor).min(grid.screen_lines())
    }

    /// Changes the width, reflowing the history and keeping each screen row a row: the rows are
    /// taken off the screen while alacritty reflows (a blank screen has nothing to wrap, so the
    /// history's rows wrap or join above it and the screen stays where it is), then put back cut
    /// or padded to the new width, with the cursor on the row it was on.
    fn resize_columns_keeping_the_screen(&mut self, cols: usize) {
        use alacritty_terminal::term::cell::Cell;
        let (old_cols, rows) = (self.columns(), self.screen_lines());
        let grid = self.term.grid_mut();
        let cursor = grid.cursor.point;
        let saved: Vec<Vec<Cell>> = (0..rows)
            .map(|l| {
                let line = Line(l as i32);
                let row = (0..old_cols).map(|c| grid[line][Column(c)].clone()).collect();
                for c in 0..old_cols {
                    grid[line][Column(c)] = Cell::default();
                }
                row
            })
            .collect();
        // The history's last row no longer runs on into the screen's first.
        if grid.history_size() > 0 {
            grid[Line(-1)][Column(old_cols - 1)].flags.remove(Flags::WRAPLINE);
        }
        // A cursor past the new width would have its (blank) row wrapped like text.
        grid.cursor.point.column = Column(cursor.column.0.min(cols - 1));
        grid.cursor.input_needs_wrap = false;
        self.term.resize(TermSize::new(cols, rows));
        let grid = self.term.grid_mut();
        let kept = old_cols.min(cols);
        for (l, row) in saved.into_iter().enumerate() {
            let line = Line(l as i32);
            for (c, mut cell) in row.into_iter().take(kept).enumerate() {
                cell.flags.remove(Flags::WRAPLINE);
                grid[line][Column(c)] = cell;
            }
            // A wide character cut from its second half, or a spacer from its character.
            if kept < old_cols {
                let last = &mut grid[line][Column(kept - 1)];
                if last.flags.intersects(Flags::WIDE_CHAR | Flags::LEADING_WIDE_CHAR_SPACER) {
                    *last = Cell::default();
                }
            }
        }
        grid.cursor.point.line = cursor.line;
        grid.cursor.point.column = Column(cursor.column.0.min(cols - 1));
    }

    /// Parses a session's replay at the sizes it was written for (`protocol::replay`): the grid
    /// starts at the size the first bytes were written for and is resized at each mark, as the
    /// window open at the time resized its own. It ends at the size the session has; the caller
    /// resizes it to its pane. A replay with no marks is parsed at the size the grid has.
    pub fn apply_replay(&mut self, replay: &[u8]) -> Wakeup {
        use protocol::replay::Piece;
        let cut = protocol::replay::pieces(replay);
        if let Some((rows, cols)) = cut.start {
            // Nothing is on the grid yet, so this only sets its size.
            self.term.resize(TermSize::new((cols as usize).max(2), (rows as usize).max(1)));
        }
        let mut out = Wakeup::default();
        for piece in cut.pieces {
            match piece {
                // A chunk takes the last word on theme pushes for the whole of itself, which is
                // right for a read of the PTY and wrong for hours of output at once: the output
                // is cut before each such word, so the program is known as one that redraws from
                // where it said so until where it left.
                Piece::Output(bytes) => {
                    for slice in cut_before(bytes, b"\x1b[?2031") {
                        let w = self.apply_output(slice);
                        out.repaint |= w.repaint;
                        if w.title.is_some() {
                            out.title = w.title;
                        }
                        out.bell |= w.bell;
                        out.pty_write.extend_from_slice(&w.pty_write);
                    }
                }
                Piece::Resize((rows, cols)) => self.resize(cols as usize, rows as usize),
            }
        }
        out
    }

    /// Drops the history and lands at the live bottom. The screen itself is kept.
    /// Terminal.app's Cmd+K ("Clear to Start"): the history and the screen go, and the line the
    /// cursor is on moves to the top with whatever is typed on it. The shell is told nothing.
    pub fn clear_to_start(&mut self) {
        use alacritty_terminal::vte::ansi::{ClearMode, Handler};
        let rows = self.term.screen_lines() as i32;
        let row = self.term.grid().cursor.point.line.0.clamp(0, rows - 1) as usize;
        if row > 0 {
            // The rows above the cursor rotate into the history, which is dropped below.
            self.term.grid_mut().scroll_up(&(Line(0)..Line(rows)), row);
            self.term.grid_mut().cursor.point.line = Line(0);
        }
        // From the cursor to the end of the screen; what is left of the cursor stays.
        self.term.clear_screen(ClearMode::Below);
        self.term.grid_mut().clear_history();
        // The grid moved under alacritty's feet above, so its selection bookkeeping did not run.
        self.term.selection = None;
        self.ledger = None;
        self.set_display_offset(0);
        self.scroll.jump_to(0.0);
        self.scroll.clamp_max(0.0);
    }

    pub fn history_size(&self) -> usize {
        self.term.grid().history_size()
    }

    pub fn columns(&self) -> usize {
        self.term.columns()
    }

    pub fn screen_lines(&self) -> usize {
        self.term.screen_lines()
    }

    pub fn app_cursor(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }

    pub fn bracketed_paste(&self) -> bool {
        self.term.mode().contains(TermMode::BRACKETED_PASTE)
    }

    /// How keys are encoded for the program right now: its cursor-key mode, whether it set the
    /// kitty protocol's disambiguate flag, and the config's Option-as-Meta choice.
    pub fn key_mode(&self, option_as_meta: bool) -> KeyMode {
        let m = self.term.mode();
        KeyMode { app_cursor: m.contains(TermMode::APP_CURSOR), kitty: m.contains(TermMode::DISAMBIGUATE_ESC_CODES), option_as_meta }
    }

    /// What the program wants from the mouse right now (see `mouse::MouseMode`).
    pub fn mouse_mode(&self) -> MouseMode {
        let m = self.term.mode();
        if m.intersects(TermMode::MOUSE_MODE) {
            MouseMode::Report { sgr: m.contains(TermMode::SGR_MOUSE) }
        } else if m.contains(TermMode::ALT_SCREEN) && m.contains(TermMode::ALTERNATE_SCROLL) {
            MouseMode::Arrows { app_cursor: m.contains(TermMode::APP_CURSOR) }
        } else {
            MouseMode::Local
        }
    }

    /// Moves alacritty's integer display offset to `off` (clamped to history).
    /// True while the view shows history: either the grid is offset or the animator is heading
    /// there. Drives the jump-to-bottom affordance.
    pub fn scrolled_up(&self) -> bool {
        self.display_offset() > 0 || self.scroll.target > 0.5 || self.scroll.current > 0.5
    }

    pub fn set_display_offset(&mut self, off: usize) {
        let cur = self.term.grid().display_offset() as i32;
        let want = off.min(self.history_size()) as i32;
        if want != cur {
            self.term.scroll_display(Scroll::Delta(want - cur));
        }
    }

    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    /// Every literal, case-insensitive match of `needle`, oldest first, from the top of history
    /// to the bottom of the screen; at most `MAX_MATCHES`, the newest kept.
    pub fn search(&self, needle: &str) -> Vec<Match> {
        self.search_capped(needle, MAX_MATCHES)
    }

    /// `search` with a given cap. One pass from the bottom right of the grid towards the top
    /// left, so a common query stops after the newest `cap` matches instead of scanning the
    /// whole scrollback, and each step resumes just before the previous match (no overlaps).
    pub fn search_capped(&self, needle: &str, cap: usize) -> Vec<Match> {
        if needle.is_empty() || cap == 0 {
            return Vec::new();
        }
        let Ok(mut regex) = RegexSearch::new(&format!("(?i){}", escape_regex(needle))) else { return Vec::new() };
        let top_left = Point::new(self.term.topmost_line(), Column(0));
        let mut origin = Point::new(self.term.bottommost_line(), self.term.last_column());
        let mut out: Vec<Match> = Vec::new();
        while let Some(m) = self.term.regex_search_left(&mut regex, origin, top_left) {
            let start = self.term.expand_wide(*m.start(), Direction::Left);
            out.push(m);
            if start == top_left || out.len() >= cap {
                break;
            }
            origin = start.sub(&self.term, Boundary::Grid, 1);
        }
        out.reverse();
        out
    }

    /// The on-screen parts of `matches` at the current display offset. `current` is the index
    /// of the match the find bar points at.
    pub fn highlights(&self, matches: &[Match], current: usize) -> Vec<Highlight> {
        let off = self.display_offset() as i32;
        let screen = self.screen_lines() as i32;
        let cols = self.columns();
        let mut out = Vec::new();
        for (i, m) in matches.iter().enumerate() {
            let (s, e) = (m.start(), m.end());
            for line in s.line.0..=e.line.0 {
                let row = line + off;
                if row < 0 || row >= screen {
                    continue;
                }
                let start = if line == s.line.0 { s.column.0 } else { 0 };
                let end = if line == e.line.0 { e.column.0 } else { cols.saturating_sub(1) };
                out.push(Highlight { row: row as usize, start, end, current: i == current });
            }
        }
        out
    }

    /// A column and a grid line as a grid point, the column clamped to the grid.
    fn grid_point(&self, col: usize, line: Line) -> Point {
        Point::new(line, Column(col.min(self.columns().saturating_sub(1))))
    }

    /// The grid line shown at viewport row `row` at the current display offset (history lines
    /// are negative).
    pub fn line_at_row(&self, row: usize) -> i32 {
        row as i32 - self.display_offset() as i32
    }

    /// Starts a selection at a grid cell: characters from a drag, a word from a double click, a
    /// line from a triple. Alacritty owns it from here, so it follows its text as output scrolls.
    /// With a record open, a drag keeps the anchor the press gave the record, and a word or a
    /// line found by alacritty becomes the record's range.
    pub fn selection_start(&mut self, col: usize, line: Line, side: Side, ty: SelectionType) {
        if self.ledger.is_some() && ty == SelectionType::Simple {
            self.retint();
            return;
        }
        self.term.selection = Some(Selection::new(ty, self.grid_point(col, line), side));
        if self.ledger.is_none() {
            return;
        }
        let Some(range) = self.term.selection.as_ref().and_then(|s| s.to_range(&self.term)) else { return };
        let start = self.record_end(range.start.column.0, range.start.line, Side::Left);
        let end = self.record_end(range.end.column.0, range.end.line, Side::Right);
        if let Some(ledger) = self.ledger.as_mut() {
            ledger.set_range(start, end);
        }
        self.retint();
    }

    /// Moves the selection's end to a grid cell; with a record open, the record's end.
    pub fn selection_update(&mut self, col: usize, line: Line, side: Side) {
        if self.ledger.is_some() {
            let end = self.record_end(col, line, side);
            if let Some(ledger) = self.ledger.as_mut() {
                ledger.set_end(end);
            }
            self.retint();
            return;
        }
        let p = self.grid_point(col, line);
        if let Some(s) = self.term.selection.as_mut() {
            s.update(p, side);
        }
    }

    /// One frame of a drag held past the pane's edge at `rate` lines per second (positive is up),
    /// with the pointer over column `col`. Whatever moves, the selection is extended to the top
    /// row from its left side or to the bottom row to its right, so the pointer's cell is in it
    /// either way.
    ///
    /// With a record open on the alternate screen, the program scrolls itself: the frame owes it
    /// `rate * dt` rows, a fraction carried to the next frame, sent as wheel reports at the
    /// pointer's column on the edge row while the program still reports the mouse, and nothing
    /// once it has turned reporting off. The record's end becomes the line on the edge row, so
    /// as the program's output brings older or newer lines there the selection grows with them.
    /// Another frame can always do more.
    ///
    /// Otherwise the local history scrolls `rate * dt` lines with no glide and the grid moves to
    /// match. A drag that left the pane before its first move in it has no selection yet, so one
    /// starts from `anchor` (the press as a grid cell, its side facing the drag). Another frame
    /// can do more until history runs out that way. A program reporting the mouse without a
    /// record, or anything else on the alternate screen, owns the wheel, so nothing happens there
    /// and the step is empty.
    ///
    /// `ended` says the session is over: no program hears the mouse then, so its last reporting
    /// mode is ignored, as `Terminal::wheel` ignores it, and a record open on its alternate
    /// screen gets nothing.
    pub fn drag_scroll_step(&mut self, rate: f32, dt: f32, col: usize, anchor: Option<(usize, i32, Side)>, ended: bool) -> DragStep {
        let alt_screen = self.term.mode().contains(TermMode::ALT_SCREEN);
        let (row, side) = if rate > 0.0 { (0, Side::Left) } else { (self.screen_lines().saturating_sub(1), Side::Right) };
        if rate != 0.0 && !ended && alt_screen && self.ledger.is_some() {
            let due = self.drag_carry.whole_lines(rate * dt);
            let mode = self.mouse_mode();
            let pty_write = if matches!(mode, MouseMode::Report { .. }) { mouse::wheel_bytes(mode, due, col, row) } else { Vec::new() };
            if let Some(ledger) = self.ledger.as_mut() {
                let line = ledger.line_of_row(row);
                ledger.set_end(End { line, col, side });
            }
            self.retint();
            return DragStep { more: true, pty_write };
        }
        let reported = !ended && matches!(self.mouse_mode(), MouseMode::Report { .. });
        if rate == 0.0 || alt_screen || reported {
            return DragStep::default();
        }
        let history = self.history_size() as f32;
        self.scroll.scroll_direct(rate * dt, history);
        self.set_display_offset(self.scroll.current.max(0.0).floor() as usize);
        if !self.selection_active() {
            if let Some((acol, aline, aside)) = anchor {
                self.selection_start(acol, Line(aline), aside, SelectionType::Simple);
            }
        }
        self.selection_update(col, Line(self.line_at_row(row)), side);
        let more = if rate > 0.0 { self.scroll.current < history } else { self.scroll.current > 0.0 };
        DragStep { more, pty_write: Vec::new() }
    }

    /// Clears the selection, and with it any open record.
    pub fn selection_clear(&mut self) {
        self.term.selection = None;
        self.ledger = None;
    }

    /// True while a selection exists, even one that covers nothing yet. With a record open, once
    /// it has an end: the press alone selects nothing.
    pub fn selection_active(&self) -> bool {
        match &self.ledger {
            Some(ledger) => ledger.has_end(),
            None => self.term.selection.is_some(),
        }
    }

    /// The selected text, or None when nothing is selected. With a record open, the record's
    /// lines, so text the program has scrolled off the screen is still there.
    pub fn selection_text(&self) -> Option<String> {
        let text = match &self.ledger {
            Some(ledger) => ledger.text(self.columns()),
            None => self.term.selection_to_string(),
        };
        text.filter(|s| !s.is_empty())
    }

    /// The text Cmd+C copies: the selection with the lines a program wrapped for this width
    /// joined (`reflow`); in Claude Code's input box, which wraps two cells short of the pane,
    /// a token cut there is joined too. A block selection is a rectangle of cells, so it stays
    /// as it is.
    pub fn copy_text(&self) -> Option<String> {
        let text = self.selection_text()?;
        if let Some(ledger) = &self.ledger {
            let columns = self.columns();
            let first_col = ledger.first_col(columns);
            let input_box = ledger.range(columns).is_some_and(|(start, _)| {
                let row = ledger.row_of_line(start.line);
                (0..self.screen_lines() as i32).contains(&row) && self.in_input_box(Line(self.line_at_row(row as usize)))
            });
            let cut_width = columns - if input_box { crate::reflow::INPUT_BOX_INSET } else { 0 };
            return Some(crate::reflow::reflow_at(&text, columns, first_col, cut_width));
        }
        let selection = self.term.selection.as_ref()?;
        if selection.ty == SelectionType::Block {
            return Some(text);
        }
        let range = selection.to_range(&self.term);
        let first_col = range.as_ref().map_or(0, |r| r.start.column.0);
        let input_box = range.as_ref().is_some_and(|r| self.in_input_box(r.start.line));
        let cut_width = self.columns() - if input_box { crate::reflow::INPUT_BOX_INSET } else { 0 };
        Some(crate::reflow::reflow_at(&text, self.columns(), first_col, cut_width))
    }

    /// The selection's on-screen parts at the current display offset.
    pub fn selection_spans(&self) -> Vec<SelectionSpan> {
        let Some(r) = self.term.selection.as_ref().and_then(|s| s.to_range(&self.term)) else { return Vec::new() };
        let off = self.display_offset() as i32;
        let screen = self.screen_lines() as i32;
        let cols = self.columns();
        let mut out = Vec::new();
        for line in r.start.line.0..=r.end.line.0 {
            let row = line + off;
            if row < 0 || row >= screen {
                continue;
            }
            let start = if r.is_block || line == r.start.line.0 { r.start.column.0 } else { 0 };
            let end = if r.is_block || line == r.end.line.0 { r.end.column.0 } else { cols.saturating_sub(1) };
            out.push(SelectionSpan { row: row as usize, start, end });
        }
        out
    }

    /// Whether the viewport cell at `col`, `row` is tinted as part of the selection, which is
    /// where a right-click opens the selection's menu. A selection that copies nothing (the
    /// press before a drag) covers no cell.
    pub fn selection_covers(&self, col: usize, row: usize) -> bool {
        self.selection_text().is_some() && self.selection_spans().iter().any(|s| s.row == row && (s.start..=s.end).contains(&col))
    }

    /// Applies a gesture from `mouse::Clicks` to the selection: a clear, a word or a line, a
    /// drag's start, or its end following the pointer.
    pub fn apply_gesture(&mut self, gesture: Gesture) {
        match gesture {
            Gesture::Nothing => {}
            Gesture::Clear => self.selection_clear(),
            Gesture::Select { ty, col, line, side } => self.selection_start(col, Line(line), side, ty),
            Gesture::Start { anchor: (acol, aline, aside), to: (col, line, side) } => {
                self.selection_start(acol, Line(aline), aside, SelectionType::Simple);
                self.selection_update(col, Line(line), side);
            }
            Gesture::Extend { col, line, side } => self.selection_update(col, Line(line), side),
        }
    }

    /// A left press at viewport cell (`col`, `row`) as `clicks` reads it (see
    /// `mouse::Clicks::press`); `report` says whether the program hears a single click and
    /// `alive` whether the session is live. The press opens a record where one belongs
    /// (`ledger_press`): after a single click's clear, which would close it, and before a word or
    /// a line is selected, which sets its range.
    #[allow(clippy::too_many_arguments)]
    pub fn pressed(&mut self, clicks: &mut Clicks, col: usize, row: usize, side: Side, click_count: usize, report: bool, alive: bool) {
        let gesture = clicks.press(col, row, self.line_at_row(row), side, click_count, report);
        if gesture == Gesture::Clear {
            self.apply_gesture(gesture);
            self.ledger_press(col, row, alive);
        } else {
            self.ledger_press(col, row, alive);
            self.apply_gesture(gesture);
        }
    }

    /// A left release at viewport cell (`col`, `row`): the bytes of the click `clicks` held for
    /// the program, if any, and a record with no end goes (`ledger_release`).
    pub fn released(&mut self, clicks: &mut Clicks, col: usize, row: usize) -> Option<Vec<u8>> {
        let bytes = clicks.release(col, row, self.mouse_mode());
        self.ledger_release();
        bytes
    }

    // The record. A program that reports the mouse on the alternate screen scrolls by redrawing,
    // so alacritty's selection would stay on grid cells while the text moves on. A press there
    // opens a record (`ledger::Ledger`) that follows the program's scroll batch by batch; while
    // it is open it is the selection, and alacritty's selection is only its tint.

    /// A left press at viewport cell (`col`, `row`): drops any record, and opens one with the
    /// press as its anchor when the session is `alive` and its program reports the mouse on the
    /// alternate screen.
    pub fn ledger_press(&mut self, col: usize, row: usize, alive: bool) {
        self.ledger = None;
        self.drag_carry.reset();
        let reported = matches!(self.mouse_mode(), MouseMode::Report { .. });
        if alive && reported && self.term.mode().contains(TermMode::ALT_SCREEN) {
            self.ledger = Some(Ledger::open(self.rows_text(), row as i32, col));
        }
    }

    /// A release or a cancel: a record with no end was a click the program heard, so it goes.
    pub fn ledger_release(&mut self) {
        if self.ledger.as_ref().is_some_and(|l| !l.has_end()) {
            self.ledger = None;
        }
    }

    /// Whether a record is open.
    pub fn record_open(&self) -> bool {
        self.ledger.is_some()
    }

    /// The visible rows as the record reads them (`ledger::row_text`), at the display offset.
    fn rows_text(&self) -> Vec<String> {
        let off = self.display_offset() as i32;
        (0..self.screen_lines() as i32).map(|i| ledger::row_text(&self.row_cells(Line(i - off)))).collect()
    }

    /// A grid cell as one end of the record: the column clamped to the grid, the line the record
    /// names the cell's viewport row by. Only called with a record open.
    fn record_end(&self, col: usize, line: Line, side: Side) -> End {
        let p = self.grid_point(col, line);
        let row = (p.line.0 + self.display_offset() as i32).max(0) as usize;
        let line = self.ledger.as_ref().map_or(row as i32, |l| l.line_of_row(row));
        End { line, col: p.column.0, side }
    }

    /// Feeds an open record the grid after a batch of output. It closes when the program left
    /// the alternate screen or redrew beyond recognition; otherwise the tint is rebuilt, which
    /// also restores it where the output erased a tinted row.
    fn follow_output(&mut self) {
        if self.ledger.is_none() {
            return;
        }
        if !self.term.mode().contains(TermMode::ALT_SCREEN) {
            self.ledger = None;
            return;
        }
        let rows = self.rows_text();
        if self.ledger.as_mut().is_some_and(|l| l.output(&rows) == Outcome::Closed) {
            self.ledger = None;
            return;
        }
        self.retint();
    }

    /// Rebuilds alacritty's selection, the tint, from the open record's range projected onto
    /// the grid: none when the record has no end or none of its range is on the rows that moved
    /// with the text. Nothing changes without a record.
    fn retint(&mut self) {
        let Some(ledger) = &self.ledger else { return };
        self.term.selection = ledger.projection(self.columns()).map(|((start_row, start_col), (end_row, end_col))| {
            let start = Point::new(Line(start_row as i32), Column(start_col));
            let mut s = Selection::new(SelectionType::Simple, start, Side::Left);
            s.update(Point::new(Line(end_row as i32), Column(end_col)), Side::Right);
            s
        });
    }

    /// Scrolls just enough to show `line` (history lines are negative). The animator lands with
    /// the grid, so there is no glide: a find jump should not animate across 10,000 lines.
    pub fn reveal_line(&mut self, line: Line) {
        let screen = self.screen_lines() as i32;
        let off = self.display_offset() as i32;
        let (top, bottom) = (-off, screen - 1 - off);
        let want = if line.0 < top {
            -line.0
        } else if line.0 > bottom {
            screen - 1 - line.0
        } else {
            off
        };
        let want = want.clamp(0, self.history_size() as i32) as usize;
        self.set_display_offset(want);
        self.scroll.jump_to(want as f32);
    }

    /// Cursor as (col, viewport line), or None when scrolled so it is off screen.
    pub fn cursor(&self) -> Option<(usize, usize)> {
        let content = self.term.renderable_content();
        if content.cursor.shape == CursorShape::Hidden {
            return None;
        }
        let p = content.cursor.point;
        let line = p.line.0 + content.display_offset as i32;
        if line < 0 || line >= self.term.screen_lines() as i32 {
            return None;
        }
        Some((p.column.0, line as usize))
    }

    fn style_of(&self, cell: &alacritty_terminal::term::cell::Cell) -> CellStyle {
        let flags = cell.flags;
        CellStyle {
            fg: self.palette.resolve(&cell.fg),
            bg: self.palette.resolve(&cell.bg),
            bold: flags.intersects(Flags::BOLD),
            italic: flags.intersects(Flags::ITALIC),
            dim: flags.intersects(Flags::DIM),
            underline: flags.intersects(Flags::ALL_UNDERLINES),
            inverse: flags.intersects(Flags::INVERSE),
        }
    }

    fn row_cells(&self, line: Line) -> Vec<(char, CellStyle, CellKind)> {
        let row = &self.term.grid()[line];
        let cols = self.term.columns();
        (0..cols)
            .map(|c| {
                let cell = &row[alacritty_terminal::index::Column(c)];
                let kind = if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                    CellKind::Spacer
                } else if cell.flags.intersects(Flags::WIDE_CHAR) {
                    CellKind::Wide
                } else {
                    CellKind::Normal
                };
                let c = if cell.flags.intersects(Flags::HIDDEN) { ' ' } else { cell.c };
                (c, self.style_of(cell), kind)
            })
            .collect()
    }

    /// The link covering a viewport cell, found on the visible rows the way the element finds
    /// the hovered one, so a URL wrapped across rows is whole. Read from the grid at the time of
    /// the call, not from the last paint, so a right-click acts on what is under the press.
    /// Nothing changes: the selection stays as it was.
    pub fn link_at(&self, col: usize, row: usize) -> Option<HoveredLink> {
        if row >= self.screen_lines() {
            return None;
        }
        let spans = self.links_in_view();
        let s = crate::links::span_at(&spans, row, col)?;
        Some(HoveredLink { row, start: s.start, end: s.end, url: s.url.clone() })
    }

    /// Every link in the viewport, as spans on viewport rows. The scan takes in a few rows above
    /// and below the viewport (`links::LINK_CONTEXT`), so a URL that runs across its edge is
    /// whole on the rows that show.
    fn links_in_view(&self) -> Vec<crate::links::Span> {
        let off = self.display_offset() as i32;
        let screen = self.term.screen_lines() as i32;
        let context = crate::links::LINK_CONTEXT as i32;
        let top = (-off - context).max(-(self.history_size() as i32));
        let bottom = (screen - 1 - off + context).min(screen - 1);
        let lines: Vec<Line> = (top..=bottom).map(Line).collect();
        let rows: Vec<Vec<char>> = lines.iter().map(|l| self.row_chars(*l)).collect();
        let wrapped: Vec<bool> = lines.iter().map(|l| self.row_wraps(*l)).collect();
        crate::links::links_in_rows(&rows, &wrapped)
            .into_iter()
            .filter_map(|mut s| {
                let row = top + s.row as i32 + off;
                (0..screen).contains(&row).then(|| {
                    s.row = row as usize;
                    s
                })
            })
            .collect()
    }

    /// A row's characters, hidden cells as blanks.
    fn row_chars(&self, line: Line) -> Vec<char> {
        self.row_cells(line).iter().map(|c| c.0).collect()
    }

    /// Whether the terminal wrapped `line` onto the next one (its last cell carries the flag).
    fn row_wraps(&self, line: Line) -> bool {
        self.term.grid()[line][Column(self.term.columns() - 1)].flags.contains(Flags::WRAPLINE)
    }

    /// Whether `line` is in Claude Code's input box (see `links::in_input_box`), looking up to
    /// 64 rows up for its prompt.
    fn in_input_box(&self, line: Line) -> bool {
        let top = (line.0 - 63).max(-(self.history_size() as i32));
        let rows: Vec<Vec<char>> = (top..=line.0).map(|l| self.row_chars(Line(l))).collect();
        crate::links::in_input_box(&rows, rows.len() - 1)
    }

    /// Claude Code's input box and the lines under it, for painting fixed at the pane's foot
    /// while the view is scrolled up into the history (Cyrus, 2026-11-17: "keep the claude user
    /// input footer fixed to the bottom of the screen despite user scroll position"). Its classic
    /// renderer writes the box as the last rows of the screen, so a scroll takes it out of view
    /// with everything else; the terminal owns the scroll, so only the terminal can hold it.
    /// Nothing at the live bottom, for a shell, when the screen shows no input box (a question
    /// or a menu in its place), or when the box would take more than half the pane.
    pub fn pinned_frame(&self) -> Option<PinnedFrame> {
        if self.display_offset() == 0 || !self.program_redraws() {
            return None;
        }
        let screen = self.term.screen_lines();
        let cells: Vec<Vec<(char, CellStyle, CellKind)>> = (0..screen).map(|l| self.row_cells(Line(l as i32))).collect();
        let text: Vec<Vec<char>> = cells.iter().map(|row| row.iter().map(|c| c.0).collect()).collect();
        let (top, last) = input_frame(&text)?;
        if (last + 1 - top) * 2 > screen {
            return None;
        }
        let content = self.term.renderable_content();
        let at = content.cursor.point;
        let line = at.line.0.max(0) as usize;
        let cursor = (content.cursor.shape != CursorShape::Hidden && (top..=last).contains(&line)).then(|| (at.column.0, line - top));
        Some(PinnedFrame { rows: cells[top..=last].to_vec(), cursor })
    }

    pub fn rows_for_paint(&self) -> RowsForPaint {
        let off = self.display_offset() as i32;
        let screen = self.term.screen_lines() as i32;
        let visible = (0..screen).map(|i| self.row_cells(Line(i - off))).collect();
        let above = if off < self.history_size() as i32 { Some(self.row_cells(Line(-off - 1))) } else { None };
        RowsForPaint { display_offset: off as usize, above, visible, links: self.links_in_view() }
    }
}

/// Whether a window position is on the grid painted from `origin`: `cols` cells across and
/// `rows` down, nothing of the padding past them.
pub(crate) fn inside_grid(x: f32, y: f32, origin: (f32, f32), cell: CellMetrics, cols: usize, rows: usize) -> bool {
    let (right, bottom) = (origin.0 + cols as f32 * cell.width, origin.1 + rows as f32 * cell.height);
    x >= origin.0 && x < right && y >= origin.1 && y < bottom
}

/// Drops every control character except tab, newline and carriage return (this removes ESC and
/// the C1 range), so pasted text can neither end the bracketed paste nor inject escape sequences.
pub(crate) fn sanitize_paste(text: &str) -> String {
    text.chars().filter(|&c| c == '\t' || c == '\n' || c == '\r' || !c.is_control()).collect()
}

/// Whether the user's own input at `last` is recent enough at `now` that a changed cell settles
/// at once (within `USER_QUIET_MS`); never before the first input.
pub fn user_quiet_at(last: Option<Instant>, now: Instant) -> bool {
    last.is_some_and(|at| now.saturating_duration_since(at) < Duration::from_millis(USER_QUIET_MS))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellMetrics {
    pub width: f32,
    pub height: f32,
}

pub struct Terminal {
    pub core: TerminalCore,
    pub session: SessionId,
    pub client: Arc<Client>,
    pub focus: FocusHandle,
    pub title: String,
    pub alive: bool,
    /// The shell's exit code, once the session has ended and the daemon reported one.
    pub exit_code: Option<i32>,
    pub cell: Option<CellMetrics>,
    pub bell_pending: bool,
    /// When the element last painted, for time-based scroll animation.
    pub last_frame: Option<Instant>,
    /// This terminal's text size; Cmd+= and Cmd+- change only the focused terminal's (spec 7).
    pub font_size: f32,
    pub line_height: f32,
    /// The find bar's state while it is open (Cmd+F).
    pub find: Option<crate::find::FindState>,
    /// The link under the mouse at the last paint: underlined, and what Cmd+click opens.
    pub hovered_link: Option<HoveredLink>,
    /// Where the element last painted its top left, in window pixels: with `cell` and `frac`,
    /// what maps the pointer to a grid cell.
    pub origin: Option<(f32, f32)>,
    /// The fraction of a row the rows were painted down by at the last paint (a smooth scroll).
    pub frac: f32,
    /// The fraction of a wheel line owed to the program (see `mouse::WheelCarry`).
    pub wheel_carry: WheelCarry,
    /// The left button between a press and its release (see `mouse::Clicks`).
    pub clicks: Clicks,
    /// What the decode remembers of the grid while a decoding effect is on, kept across paints
    /// so a change shows against the grid painted before it; None without one.
    pub decode: Option<crate::effects::decode::Decode>,
    /// When the user last typed, pasted, clicked or scrolled here, so the decode leaves the
    /// echo of their own input alone (`user_quiet`); None before the first.
    pub last_user_input: Option<Instant>,
    /// When the view last moved under the user's hand: the wheel (every mode), a keyboard page
    /// scroll or a drag past an edge. The window's glyph clock rests meanwhile
    /// (`effects::active::glyph_clock`); None before the first.
    pub last_scroll: Option<Instant>,
    /// The window draw (`Root::draws`) this terminal was last painted in.
    pub painted_draw: u64,
}

/// A link under the pointer: its row in the visible grid, its columns, and the URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoveredLink {
    pub row: usize,
    pub start: usize,
    pub end: usize,
    pub url: String,
}

impl Terminal {
    pub fn new(core: TerminalCore, session: SessionId, client: Arc<Client>, cx: &mut Context<Self>) -> Self {
        Self {
            core,
            session,
            client,
            focus: cx.focus_handle(),
            title: String::new(),
            alive: true,
            exit_code: None,
            cell: None,
            bell_pending: false,
            last_frame: None,
            font_size: crate::terminal_element::DEFAULT_FONT_SIZE,
            line_height: crate::terminal_element::DEFAULT_LINE_HEIGHT,
            find: None,
            hovered_link: None,
            origin: None,
            frac: 0.0,
            wheel_carry: WheelCarry::default(),
            clicks: Clicks::default(),
            decode: None,
            last_user_input: None,
            last_scroll: None,
            painted_draw: 0,
        }
    }

    /// Whether the user acted here within `USER_QUIET_MS` of `now` (see `user_quiet_at`).
    pub fn user_quiet(&self, now: Instant) -> bool {
        user_quiet_at(self.last_user_input, now)
    }

    /// The grid cell under a window position and the half of the cell the pointer is on, from
    /// the last paint; None before the first paint.
    pub fn point_at(&self, position: gpui::Point<gpui::Pixels>) -> Option<(usize, usize, Side)> {
        let origin = self.origin?;
        let cell = self.cell?;
        Some(mouse::cell_under(f32::from(position.x), f32::from(position.y), origin, (cell.width, cell.height), self.frac, self.core.columns(), self.core.screen_lines()))
    }

    /// The grid cell (column, line) under a window position once the view rests at scroll
    /// position `at` (lines of history, as the animator counts them), and the half of the cell
    /// the pointer is on; None before the first paint. A wheel notch eases over a few frames, so
    /// this is the text the pointer will be over when it lands, not what was painted last.
    fn grid_cell_at(&self, position: gpui::Point<gpui::Pixels>, at: f32) -> Option<(usize, i32, Side)> {
        let (origin, cell) = (self.origin?, self.cell?);
        let at = at.max(0.0);
        let off = at.floor();
        let (col, row, side) = mouse::cell_under(f32::from(position.x), f32::from(position.y), origin, (cell.width, cell.height), at - off, self.core.columns(), self.core.screen_lines());
        Some((col, row as i32 - off as i32, side))
    }

    /// The link under a window position, if any (see `TerminalCore::link_at`); None before the
    /// first paint, and None off the grid, where `point_at` would clamp to the nearest cell: the
    /// padding past the last row or column shows no underline, so it opens no menu either.
    pub fn link_at_position(&self, position: gpui::Point<gpui::Pixels>) -> Option<HoveredLink> {
        let (origin, cell) = (self.origin?, self.cell?);
        if !inside_grid(f32::from(position.x), f32::from(position.y), origin, cell, self.core.columns(), self.core.screen_lines()) {
            return None;
        }
        let (col, row, _) = self.point_at(position)?;
        self.core.link_at(col, row)
    }

    /// Whether a window position is over the tinted selection; false before the first paint and
    /// in the padding off the grid, as for a link.
    pub fn selection_at_position(&self, position: gpui::Point<gpui::Pixels>) -> bool {
        let (Some(origin), Some(cell)) = (self.origin, self.cell) else { return false };
        if !inside_grid(f32::from(position.x), f32::from(position.y), origin, cell, self.core.columns(), self.core.screen_lines()) {
            return false;
        }
        self.point_at(position).is_some_and(|(col, row, _)| self.core.selection_covers(col, row))
    }

    /// A wheel step of `lines` (positive is up); `precise` is a trackpad's pixel step rather than
    /// a wheel notch. The local history scrolls unless the program reports the mouse or runs on
    /// the alternate screen, where the step goes to it as a report or an arrow key; Shift forces
    /// the local scroll, as in xterm, and so does an ended session.
    pub fn wheel(&mut self, lines: f32, precise: bool, shift: bool, position: gpui::Point<gpui::Pixels>, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.last_user_input = Some(now);
        self.last_scroll = Some(now);
        let mode = if shift || !self.alive { MouseMode::Local } else { self.core.mouse_mode() };
        if mode == MouseMode::Local {
            self.wheel_carry.reset();
            if precise {
                self.scroll_direct(lines, cx);
            } else {
                self.scroll_lines(lines, cx);
            }
            // A wheel sends no move, so a selection being dragged takes its end from the text
            // the pointer is over once the scroll lands.
            if self.clicks.held() && self.core.selection_active() {
                if let Some((col, line, side)) = self.grid_cell_at(position, self.core.scroll.target) {
                    self.core.selection_update(col, Line(line), side);
                }
            }
            return;
        }
        // The program's rows are the live ones: a view left in the history comes back first.
        if self.core.scrolled_up() {
            self.jump_to_bottom(cx);
        }
        let whole = self.wheel_carry.whole_lines(lines);
        if whole == 0 {
            return;
        }
        let (col, row, _) = self.point_at(position).unwrap_or((0, 0, Side::Left));
        let bytes = mouse::wheel_bytes(mode, whole, col, row);
        self.client.send(Request::Input { session: self.session, bytes });
    }

    /// A left press (see `mouse::Clicks::press`). The click that focused the window is never
    /// forwarded: it was meant for the window, not for the program. In a program that reports
    /// the mouse on the alternate screen the press opens a record (`TerminalCore::ledger_press`):
    /// after a single click's clear, which would close it, and before a word or a line is
    /// selected, which sets its range.
    pub fn mouse_pressed(&mut self, position: gpui::Point<gpui::Pixels>, click_count: usize, first_mouse: bool, cx: &mut Context<Self>) {
        let Some((col, row, side)) = self.point_at(position) else { return };
        let report = matches!(self.core.mouse_mode(), MouseMode::Report { .. }) && !first_mouse;
        self.core.pressed(&mut self.clicks, col, row, side, click_count, report, self.alive);
        cx.notify();
    }

    /// A move (see `mouse::Clicks::moved`); `left_down` is whether the left button is held.
    pub fn mouse_moved(&mut self, position: gpui::Point<gpui::Pixels>, left_down: bool, cx: &mut Context<Self>) {
        let Some((col, row, side)) = self.point_at(position) else { return };
        let gesture = self.clicks.moved(col, self.core.line_at_row(row), side, left_down, self.core.selection_active());
        self.apply(gesture, cx);
    }

    /// One frame of a held drag past the element's top or bottom edge at `rate` lines per
    /// second (see `TerminalCore::drag_scroll_step`), with the pointer over column `col`. The
    /// press is the anchor, its side facing the way the drag went. The wheel reports the step
    /// owes a live program are sent to it. Returns whether to keep scrolling.
    pub fn drag_scroll_step(&mut self, rate: f32, dt: f32, col: usize) -> bool {
        let side = if rate > 0.0 { Side::Right } else { Side::Left };
        let anchor = self.clicks.anchor().map(|(acol, aline)| (acol, aline, side));
        let step = self.core.drag_scroll_step(rate, dt, col, anchor, !self.alive);
        if step.more {
            // The press scrolled the pane, so it is a drag, and the program hears no click.
            self.clicks.dragged();
            self.last_scroll = Some(Instant::now());
        }
        if !step.pty_write.is_empty() && self.alive {
            self.last_user_input = Some(Instant::now());
            self.client.send(Request::Input { session: self.session, bytes: step.pty_write });
        }
        step.more
    }

    /// A left release over the terminal: a held click reaches a live program now.
    pub fn mouse_released(&mut self, position: gpui::Point<gpui::Pixels>) {
        let (col, row, _) = self.point_at(position).unwrap_or((0, 0, Side::Left));
        if let Some(bytes) = self.core.released(&mut self.clicks, col, row) {
            if self.alive {
                self.last_user_input = Some(Instant::now());
                self.client.send(Request::Input { session: self.session, bytes });
            }
        }
    }

    /// The left button came up somewhere else, a right button came up (gpui reports a left
    /// release made with Control held as a right release), or the press was a Cmd+click on a
    /// link: nothing is held for the program, and the drag is over.
    pub fn mouse_cancelled(&mut self) {
        self.clicks.cancel();
        self.core.ledger_release();
    }

    /// Applies a gesture to the core (`TerminalCore::apply_gesture`) and repaints unless it was
    /// nothing.
    fn apply(&mut self, gesture: Gesture, cx: &mut Context<Self>) {
        if gesture == Gesture::Nothing {
            return;
        }
        self.core.apply_gesture(gesture);
        cx.notify();
    }

    /// Applies one daemon event. Returns true when a repaint is needed.
    // Not called in milestone 1, where the window batches events through `handle_events`; kept
    // for milestone 2's per-tab event routing.
    #[allow(dead_code)]
    pub fn handle_event(&mut self, ev: &Event, cx: &mut Context<Self>) -> bool {
        self.handle_events(std::slice::from_ref(ev), cx)
    }

    /// Applies a batch of daemon events in order. Consecutive output is merged into one grid
    /// update, one input write, and at most one `cx.notify()`.
    pub fn handle_events(&mut self, events: &[Event], cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        let mut pending: Vec<&[u8]> = Vec::new();
        for ev in events {
            match ev {
                Event::Output { session, bytes } if *session == self.session => pending.push(bytes),
                Event::Exited { session, code } if *session == self.session => {
                    self.flush_output(&mut pending);
                    self.alive = false;
                    self.exit_code = *code;
                    changed = true;
                }
                _ => {}
            }
        }
        changed |= self.flush_output(&mut pending);
        if changed {
            cx.notify();
        }
        changed
    }

    fn flush_output(&mut self, pending: &mut Vec<&[u8]>) -> bool {
        if pending.is_empty() {
            return false;
        }
        let w = self.core.apply_all(pending.drain(..));
        if let Some(t) = w.title {
            self.title = t;
        }
        if w.bell {
            self.bell_pending = true;
        }
        if !w.pty_write.is_empty() {
            self.client.send(Request::Input { session: self.session, bytes: w.pty_write });
        }
        self.mark_find_stale();
        true
    }

    pub fn jump_to_bottom(&mut self, cx: &mut Context<Self>) {
        self.core.scroll.jump_to(0.0);
        self.core.set_display_offset(0);
        cx.notify();
    }

    pub fn handle_key(&mut self, ks: &Keystroke, option_as_meta: bool, cx: &mut Context<Self>) {
        // Shift+PageUp/PageDown scroll locally; everything else goes to the shell.
        if ks.modifiers.shift && (ks.key == "pageup" || ks.key == "pagedown") {
            let page = self.core.screen_lines() as f32;
            self.scroll_lines(if ks.key == "pageup" { page } else { -page }, cx);
            return;
        }
        // Local scrolling above still works on an ended session; nothing is sent to the daemon.
        if !self.alive {
            return;
        }
        if let Some(bytes) = keystroke_to_bytes(ks, self.core.key_mode(option_as_meta)) {
            self.last_user_input = Some(Instant::now());
            self.client.send(Request::Input { session: self.session, bytes });
            // Typing jumps back to the live bottom, as every terminal does, unless the input box
            // is pinned at the pane's foot (`key_returns_to_bottom`).
            if key_returns_to_bottom(self.core.pinned_frame().is_some(), &ks.key) {
                self.core.scroll.jump_to(0.0);
                self.core.set_display_offset(0);
            }
            cx.notify();
        }
    }

    pub fn paste(&mut self, text: &str) {
        if !self.alive {
            return;
        }
        let mut bytes = Vec::with_capacity(text.len() + 12);
        if self.core.bracketed_paste() {
            bytes.extend_from_slice(b"\x1b[200~");
            bytes.extend_from_slice(sanitize_paste(text).as_bytes());
            bytes.extend_from_slice(b"\x1b[201~");
        } else {
            bytes.extend_from_slice(text.as_bytes());
        }
        self.last_user_input = Some(Instant::now());
        self.client.send(Request::Input { session: self.session, bytes });
    }

    /// Positive lines scroll up into history, with the ease a wheel notch deserves.
    pub fn scroll_lines(&mut self, lines: f32, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.last_user_input = Some(now);
        self.last_scroll = Some(now);
        self.core.scroll.scroll_by(lines, self.core.history_size() as f32);
        cx.notify();
    }

    /// A trackpad's pixel step: the view follows the finger at once, since the trackpad already
    /// delivers its own glide and momentum.
    pub fn scroll_direct(&mut self, lines: f32, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.last_user_input = Some(now);
        self.last_scroll = Some(now);
        self.core.scroll.scroll_direct(lines, self.core.history_size() as f32);
        cx.notify();
    }

    /// Resizes the grid and the PTY when the pane's cell grid changes.
    pub fn ensure_size(&mut self, cols: usize, rows: usize) {
        let (cols, rows) = (cols.max(2), rows.max(1));
        if cols == self.core.columns() && rows == self.core.screen_lines() {
            return;
        }
        self.core.resize(cols, rows);
        self.client.send(Request::Resize { session: self.session, rows: rows as u16, cols: cols as u16 });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(row: &[(char, CellStyle, CellKind)]) -> String {
        row.iter().map(|(c, _, _)| *c).collect::<String>().trim_end().to_string()
    }

    #[test]
    fn the_user_is_at_work_for_400_ms_after_a_key_and_never_before_the_first() {
        let t0 = Instant::now();
        assert!(user_quiet_at(Some(t0), t0));
        assert!(user_quiet_at(Some(t0), t0 + Duration::from_millis(399)));
        assert!(!user_quiet_at(Some(t0), t0 + Duration::from_millis(400)));
        assert!(!user_quiet_at(Some(t0), t0 + Duration::from_secs(5)));
        assert!(!user_quiet_at(None, t0), "never set: not quiet");
    }

    #[test]
    fn apply_output_fills_the_grid() {
        let mut t = TerminalCore::new(20, 5, 100);
        let w = t.apply_output(b"hello\r\nworld");
        assert!(w.repaint);
        let rows = t.rows_for_paint();
        assert_eq!(text_of(&rows.visible[0]), "hello");
        assert_eq!(text_of(&rows.visible[1]), "world");
        assert_eq!(t.cursor(), Some((5, 1)));
    }

    #[test]
    fn title_and_bell_are_reported() {
        let mut t = TerminalCore::new(20, 5, 100);
        let w = t.apply_output(b"\x1b]0;my title\x07");
        assert_eq!(w.title.as_deref(), Some("my title"));
        let w = t.apply_output(b"\x07");
        assert!(w.bell);
    }

    #[test]
    fn apply_all_coalesces_wakeups() {
        let mut t = TerminalCore::new(20, 5, 100);
        let lines: Vec<Vec<u8>> = (0..1000).map(|i| format!("line {i}\r\n").into_bytes()).collect();
        let w = t.apply_all(lines.iter().map(|l| l.as_slice()));
        assert!(w.repaint);
        assert_eq!(t.history_size(), 100, "scrollback is capped");
        let mut chunks: Vec<&[u8]> = vec![b"a"; 10];
        chunks[2] = b"\x1b]0;t3\x07";
        chunks[6] = b"\x07";
        let w = t.apply_all(chunks);
        assert_eq!(w.title.as_deref(), Some("t3"));
        assert!(w.bell);
        assert!(!t.apply_all(Vec::<&[u8]>::new()).repaint);
    }

    #[test]
    fn sync_update_flushes_after_deadline() {
        let mut t = TerminalCore::new(20, 5, 100);
        t.apply_output(b"\x1b[?2026hhidden");
        assert_eq!(text_of(&t.rows_for_paint().visible[0]), "");
        assert!(t.sync_pending());
        assert!(!t.flush_sync_if_due());
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(t.flush_sync_if_due());
        assert!(!t.sync_pending());
        assert_eq!(text_of(&t.rows_for_paint().visible[0]), "hidden");
    }

    #[test]
    fn ensure_size_clamps_tiny_sizes() {
        let mut t = TerminalCore::new(20, 5, 100);
        t.resize(1, 0);
        assert_eq!((t.columns(), t.screen_lines()), (2, 1));
        t.resize(1, 0);
        assert_eq!((t.columns(), t.screen_lines()), (2, 1));
    }

    #[test]
    fn hidden_cursor_is_none() {
        let mut t = TerminalCore::new(20, 5, 100);
        t.apply_output(b"\x1b[?25l");
        assert_eq!(t.cursor(), None);
        t.apply_output(b"\x1b[?25h");
        assert!(t.cursor().is_some());
    }

    #[test]
    fn scrolled_up_tracks_the_animator_and_the_grid() {
        let mut t = TerminalCore::new(20, 3, 100);
        for i in 0..10 {
            t.apply_output(format!("l{i}\r\n").as_bytes());
        }
        assert!(!t.scrolled_up());
        t.scroll.scroll_by(2.0, t.history_size() as f32);
        assert!(t.scrolled_up(), "a scroll target above the bottom counts");
        t.scroll.jump_to(0.0);
        t.set_display_offset(0);
        assert!(!t.scrolled_up());
    }

    #[test]
    fn paste_end_marker_is_stripped() {
        assert_eq!(sanitize_paste("a\x1b[201~b"), "a[201~b");
        assert_eq!(sanitize_paste("\x1b[2\x1b[201~01~"), "[2[201~01~");
        assert_eq!(sanitize_paste("line1\r\n\tx"), "line1\r\n\tx");
    }

    #[test]
    fn replay_from_inside_osc_recovers() {
        let mut t = TerminalCore::new(20, 5, 100);
        let w = t.apply_output(b";cut title\x07echo ok\r\n");
        let rows = t.rows_for_paint();
        assert!(rows.visible.iter().any(|r| text_of(r).contains("echo ok")));
        assert_eq!(w.title, None);
    }

    #[test]
    fn scrolling_up_keeps_view_stable_while_output_arrives() {
        let mut t = TerminalCore::new(20, 5, 100);
        for i in 0..50 {
            t.apply_output(format!("l{i}\r\n").as_bytes());
        }
        t.scroll.jump_to(10.0);
        t.set_display_offset(t.scroll.current as usize);
        let before = text_of(&t.rows_for_paint().visible[0]);
        t.apply_output(b"more\r\nmore\r\n");
        t.set_display_offset(t.scroll.current as usize);
        let after = text_of(&t.rows_for_paint().visible[0]);
        assert_eq!(before, after, "new output must not yank the view down");
    }

    #[test]
    fn row_above_is_provided_when_scrolled() {
        let mut t = TerminalCore::new(20, 5, 100);
        for i in 0..20 {
            t.apply_output(format!("l{i}\r\n").as_bytes());
        }
        t.set_display_offset(3);
        let rows = t.rows_for_paint();
        let above = rows.above.expect("a row above the viewport");
        let top = text_of(&rows.visible[0]);
        // The row above is the line just before the visible top.
        let top_n: usize = top.trim_start_matches('l').parse().unwrap();
        assert_eq!(text_of(&above), format!("l{}", top_n - 1));
        t.set_display_offset(0);
        assert!(t.rows_for_paint().above.is_none() || t.history_size() > 0);
    }

    #[test]
    fn key_mode_follows_the_kitty_flags_per_screen() {
        let mut t = TerminalCore::new(20, 5, 100);
        assert!(!t.key_mode(true).kitty);
        assert!(t.key_mode(true).option_as_meta && !t.key_mode(false).option_as_meta);
        // The set form, then the push form Claude Code uses (flags 5: disambiguate and
        // alternate keys) and the pop.
        t.apply_output(b"\x1b[=1;1u");
        assert!(t.key_mode(true).kitty);
        t.apply_output(b"\x1b[=u");
        assert!(!t.key_mode(true).kitty, "an empty set clears the flags");
        t.apply_output(b"\x1b[>5u");
        assert!(t.key_mode(true).kitty);
        t.apply_output(b"\x1b[<u");
        assert!(!t.key_mode(true).kitty);
        // Flags pushed on the alternate screen leave with it, so a program that exits cleanly
        // leaves the shell's Esc alone (one that is killed does not; `reset` clears both).
        t.apply_output(b"\x1b[?1049h\x1b[>1u");
        assert!(t.key_mode(true).kitty);
        t.apply_output(b"\x1b[?1049l");
        assert!(!t.key_mode(true).kitty);
        t.apply_output(b"\x1b[?1h");
        assert!(t.key_mode(true).app_cursor);
    }

    #[test]
    fn mouse_mode_follows_the_programs_requests() {
        let mut t = TerminalCore::new(20, 5, 100);
        assert_eq!(t.mouse_mode(), MouseMode::Local);
        // The alternate screen alone: alternate scroll is on by default, so the wheel is arrows.
        t.apply_output(b"\x1b[?1049h");
        assert_eq!(t.mouse_mode(), MouseMode::Arrows { app_cursor: false });
        t.apply_output(b"\x1b[?1h");
        assert_eq!(t.mouse_mode(), MouseMode::Arrows { app_cursor: true });
        // Claude Code's fullscreen renderer: click reporting with the SGR encoding.
        t.apply_output(b"\x1b[?1000h\x1b[?1006h");
        assert_eq!(t.mouse_mode(), MouseMode::Report { sgr: true });
        t.apply_output(b"\x1b[?1006l");
        assert_eq!(t.mouse_mode(), MouseMode::Report { sgr: false });
        t.apply_output(b"\x1b[?1000l\x1b[?1007l");
        assert_eq!(t.mouse_mode(), MouseMode::Local);
        t.apply_output(b"\x1b[?1049l\x1b[?1007h");
        assert_eq!(t.mouse_mode(), MouseMode::Local, "alternate scroll means nothing on the primary screen");
    }

    #[test]
    fn a_right_click_is_on_the_selection_only_over_its_tinted_cells() {
        let mut t = TerminalCore::new(20, 3, 100);
        t.apply_output(b"hello world\r\nsecond line");
        assert!(!t.selection_covers(0, 0), "nothing selected");
        t.selection_start(0, Line(0), Side::Left, SelectionType::Simple);
        assert!(!t.selection_covers(0, 0), "a press alone selects nothing");
        t.selection_update(4, Line(0), Side::Right);
        assert!(t.selection_covers(0, 0) && t.selection_covers(4, 0));
        assert!(!t.selection_covers(5, 0), "past the end of the word");
        assert!(!t.selection_covers(0, 1), "the next row");
        // Across rows: the first row is tinted to its end, the last from its start.
        t.selection_start(6, Line(0), Side::Left, SelectionType::Simple);
        t.selection_update(5, Line(1), Side::Right);
        assert!(t.selection_covers(19, 0) && t.selection_covers(0, 1) && t.selection_covers(5, 1));
        assert!(!t.selection_covers(5, 0) && !t.selection_covers(6, 1));
        t.selection_clear();
        assert!(!t.selection_covers(6, 0));
    }

    #[test]
    fn a_drag_selects_text_for_the_clipboard() {
        let mut t = TerminalCore::new(20, 3, 100);
        t.apply_output(b"hello world\r\nsecond line");
        assert_eq!(t.selection_text(), None);
        t.selection_start(0, Line(0), Side::Left, SelectionType::Simple);
        t.selection_update(4, Line(0), Side::Right);
        assert_eq!(t.selection_text().as_deref(), Some("hello"));
        assert_eq!(t.selection_spans(), vec![SelectionSpan { row: 0, start: 0, end: 4 }]);
        // Across rows: the first row runs to its end, the last from its start.
        t.selection_start(6, Line(0), Side::Left, SelectionType::Simple);
        t.selection_update(5, Line(1), Side::Right);
        assert_eq!(t.selection_text().as_deref(), Some("world\nsecond"));
        assert_eq!(t.selection_spans(), vec![SelectionSpan { row: 0, start: 6, end: 19 }, SelectionSpan { row: 1, start: 0, end: 5 }]);
        t.selection_clear();
        assert_eq!(t.selection_text(), None);
        assert!(t.selection_spans().is_empty());
    }

    #[test]
    fn the_link_under_a_cell_runs_on_where_the_terminal_wrapped() {
        let mut t = TerminalCore::new(20, 3, 100);
        t.apply_output(b"see https://csarko.sh/abc/def now");
        let url = "https://csarko.sh/abc/def".to_string();
        assert_eq!(t.link_at(4, 0), Some(HoveredLink { row: 0, start: 4, end: 20, url: url.clone() }), "the first row's piece carries the whole URL");
        assert_eq!(t.link_at(3, 1), Some(HoveredLink { row: 1, start: 0, end: 9, url }), "so does the second");
        assert_eq!(t.link_at(10, 1), None, "the space after it");
        assert_eq!(t.rows_for_paint().links.iter().map(|s| (s.row, s.start, s.end)).collect::<Vec<_>>(), vec![(0, 4, 20), (1, 0, 9)]);
    }

    #[test]
    fn a_link_across_the_viewport_s_edge_is_whole_on_the_rows_that_show() {
        let mut t = TerminalCore::new(20, 3, 100);
        // Seven lines on a three-line screen: the URL takes the third and fourth.
        t.apply_output(b"a\r\nb\r\nsee https://csarko.sh/abc/def now\r\nmore\r\nlines\r\nx");
        // Scrolled so the URL's second row is the top row: its piece has no scheme of its own.
        t.set_display_offset(1);
        let url = "https://csarko.sh/abc/def".to_string();
        assert_eq!(t.link_at(3, 0), Some(HoveredLink { row: 0, start: 0, end: 9, url: url.clone() }));
        assert_eq!(t.link_at(3, 1), None, "the row under it");
        // Scrolled so the URL's first row is the bottom row: the URL still ends below.
        t.set_display_offset(4);
        assert_eq!(t.link_at(5, 2), Some(HoveredLink { row: 2, start: 4, end: 20, url }));
    }

    #[test]
    fn copying_from_the_input_box_joins_a_token_cut_two_cells_short() {
        let mut t = TerminalCore::new(40, 6, 100);
        // The input box as Claude Code draws it at 40 columns: a rule, the prompt row, two
        // continuation rows under a two-cell indent, a rule. The URL is cut at 38.
        let rows = ["\u{2500}".repeat(40), "\u{276f}\u{a0}YZ0123456789abcdef".into(), "  https://github.com/csarkosh/ide-arko".into(), "  s/actions/runs/39877328412".into(), "\u{2500}".repeat(40)];
        t.apply_output(rows.join("\r\n").as_bytes());
        t.selection_start(0, Line(1), Side::Left, SelectionType::Simple);
        t.selection_update(39, Line(3), Side::Right);
        assert_eq!(t.copy_text().as_deref(), Some("\u{276f}\u{a0}YZ0123456789abcdef https://github.com/csarkosh/ide-arkos/actions/runs/39877328412"));
        // The same rows under a bullet are a message, which cuts at the pane's width: the row
        // two cells short wrapped on a space.
        let mut m = TerminalCore::new(40, 6, 100);
        m.apply_output(b"\xe2\x97\x8f YZ0123456789abcdef\r\n  https://github.com/csarkosh/ide-arko\r\n  s/actions/runs/39877328412");
        m.selection_start(0, Line(0), Side::Left, SelectionType::Simple);
        m.selection_update(39, Line(2), Side::Right);
        assert_eq!(m.copy_text().as_deref(), Some("\u{25cf} YZ0123456789abcdef https://github.com/csarkosh/ide-arko s/actions/runs/39877328412"));
    }

    #[test]
    fn the_link_under_a_cell_is_found_and_the_selection_is_left_alone() {
        let mut t = TerminalCore::new(30, 3, 100);
        t.apply_output(b"see https://csarko.sh now\r\nplain text");
        t.selection_start(0, Line(1), Side::Left, SelectionType::Simple);
        t.selection_update(4, Line(1), Side::Right);
        let link = HoveredLink { row: 0, start: 4, end: 21, url: "https://csarko.sh".into() };
        assert_eq!(t.link_at(4, 0), Some(link.clone()), "the first cell of the URL");
        assert_eq!(t.link_at(20, 0), Some(link), "the last cell of the URL");
        assert_eq!(t.link_at(3, 0), None, "the space before it");
        assert_eq!(t.link_at(21, 0), None, "the space after it");
        assert_eq!(t.link_at(4, 1), None, "a row without a link");
        assert_eq!(t.link_at(4, 7), None, "a row below the screen");
        assert_eq!(t.selection_text().as_deref(), Some("plain"), "looking for a link never touches the selection");
    }

    #[test]
    fn only_a_position_on_the_grid_can_find_a_link() {
        // 10 columns of 8 by 3 rows of 16 from (100, 50): the grid ends at x 180 and y 98.
        let cell = CellMetrics { width: 8.0, height: 16.0 };
        let on = |x: f32, y: f32| inside_grid(x, y, (100.0, 50.0), cell, 10, 3);
        assert!(on(100.0, 50.0), "the top left corner");
        assert!(on(179.9, 97.9), "the last cell of the last row");
        assert!(!on(120.0, 98.0), "just below the last row, in the padding");
        assert!(!on(180.0, 60.0), "just right of the last column");
        assert!(!on(99.9, 60.0) && !on(120.0, 49.9), "left of and above the grid");
    }

    #[test]
    fn the_link_under_a_cell_follows_the_scrolled_view() {
        let mut t = TerminalCore::new(30, 2, 100);
        t.apply_output(b"https://a.io/x\r\none\r\ntwo\r\nthree");
        assert_eq!(t.link_at(2, 0), None, "the link is in history");
        t.set_display_offset(2);
        assert_eq!(t.link_at(2, 0).map(|l| l.url), Some("https://a.io/x".to_string()), "scrolled up, it is on the top row");
    }

    #[test]
    fn double_and_triple_clicks_select_a_word_and_a_line() {
        let mut t = TerminalCore::new(20, 3, 100);
        t.apply_output(b"hello world\r\nsecond line");
        t.selection_start(2, Line(0), Side::Left, SelectionType::Semantic);
        assert_eq!(t.selection_text().as_deref(), Some("hello"));
        t.selection_start(2, Line(1), Side::Left, SelectionType::Lines);
        assert_eq!(t.selection_text().as_deref(), Some("second line\n"));
    }

    /// Claude Code's way at 30 columns: each row ends with a real newline, the widest at 30.
    const WRAPPED: &[u8] = b"\xe2\x97\x8f The limiter is done and all\r\n  six tests pass.\r\n  Next I will wire it into the\r\n  server routes.";

    #[test]
    fn copying_joins_a_program_s_own_wraps_and_keeps_its_breaks() {
        let mut t = TerminalCore::new(30, 6, 100);
        t.apply_output(WRAPPED);
        assert_eq!(t.copy_text(), None);
        t.selection_start(0, Line(0), Side::Left, SelectionType::Simple);
        t.selection_update(15, Line(3), Side::Right);
        assert_eq!(t.copy_text().as_deref(), Some("\u{25cf} The limiter is done and all six tests pass.\n  Next I will wire it into the server routes."));
        // "Copy as shown" keeps the rows as they are on screen.
        assert_eq!(t.selection_text().as_deref(), Some("\u{25cf} The limiter is done and all\n  six tests pass.\n  Next I will wire it into the\n  server routes."));
    }

    #[test]
    fn copying_counts_the_column_the_selection_starts_at() {
        let mut t = TerminalCore::new(30, 6, 100);
        t.apply_output(WRAPPED);
        // From "is", 14 cells in: the row still ran to column 29, so its newline is a wrap.
        t.selection_start(14, Line(0), Side::Left, SelectionType::Simple);
        t.selection_update(15, Line(3), Side::Right);
        assert_eq!(t.copy_text().as_deref(), Some("is done and all six tests pass.\n  Next I will wire it into the server routes."));
    }

    #[test]
    fn a_block_selection_copies_as_shown_and_a_line_selection_keeps_its_newline() {
        let mut t = TerminalCore::new(30, 6, 100);
        t.apply_output(WRAPPED);
        t.selection_start(0, Line(0), Side::Left, SelectionType::Block);
        t.selection_update(29, Line(3), Side::Right);
        assert!(t.selection_text().is_some_and(|s| s.contains('\n')));
        assert_eq!(t.copy_text(), t.selection_text(), "a rectangle of cells is never re-flowed");
        t.selection_start(0, Line(0), Side::Left, SelectionType::Lines);
        t.selection_update(0, Line(1), Side::Right);
        assert_eq!(t.selection_text().as_deref(), Some("\u{25cf} The limiter is done and all\n  six tests pass.\n"));
        assert_eq!(t.copy_text().as_deref(), Some("\u{25cf} The limiter is done and all six tests pass.\n"));
    }

    #[test]
    fn a_selection_follows_its_text_into_history() {
        let mut t = TerminalCore::new(20, 2, 100);
        t.apply_output(b"hello world\r\nsecond");
        t.selection_start(0, Line(0), Side::Left, SelectionType::Simple);
        t.selection_update(4, Line(0), Side::Right);
        // One more line scrolls "hello" into history; the selection goes with it.
        t.apply_output(b"\r\nthird");
        assert_eq!(t.selection_text().as_deref(), Some("hello"));
        assert!(t.selection_spans().is_empty(), "off screen at the live bottom");
        t.set_display_offset(1);
        assert_eq!(t.selection_spans(), vec![SelectionSpan { row: 0, start: 0, end: 4 }]);
    }

    /// The selection's first and last cells as (line, column) pairs, history lines negative.
    fn selected_lines(t: &TerminalCore) -> ((i32, usize), (i32, usize)) {
        let r = t.term.selection.as_ref().and_then(|s| s.to_range(&t.term)).expect("a selection");
        ((r.start.line.0, r.start.column.0), (r.end.line.0, r.end.column.0))
    }

    fn with_history(cols: usize, rows: usize, lines: usize) -> TerminalCore {
        let mut t = TerminalCore::new(cols, rows, 100);
        for i in 0..lines {
            t.apply_output(format!("l{i}\r\n").as_bytes());
        }
        t
    }

    #[test]
    fn a_selection_is_held_in_grid_lines_so_scrolling_never_moves_its_start() {
        let mut t = with_history(20, 8, 50);
        // The anchor's right side, as `Clicks` gives a drag that sets off upwards.
        t.selection_start(0, Line(5), Side::Right, SelectionType::Simple);
        t.set_display_offset(3);
        // Viewport row 0 of a view scrolled up 3 lines is line -3.
        t.selection_update(0, Line(t.line_at_row(0)), Side::Left);
        assert_eq!(selected_lines(&t), ((-3, 0), (5, 0)));
    }

    #[test]
    fn a_drag_past_the_edge_scrolls_the_history_and_selects_to_the_edge_row() {
        let mut t = with_history(20, 5, 30);
        let history = t.history_size();
        assert!(history > 10, "enough history to scroll through: {history}");
        // Pressed at column 1 of line 2, and the pointer left over the top before any move in the
        // pane: the selection starts from the anchor and runs to the top row at the pointer.
        assert!(t.drag_scroll_step(6.0, 0.5, 3, Some((1, 2, Side::Right)), false).more);
        assert_eq!(t.scroll.current, 3.0);
        assert_eq!(t.display_offset(), 3, "the step moves the grid itself");
        assert_eq!(selected_lines(&t), ((-3, 3), (2, 1)));
        // The next frame extends the same selection; the anchor only starts a missing one.
        assert!(t.drag_scroll_step(6.0, 0.5, 0, Some((9, 4, Side::Right)), false).more);
        assert_eq!(t.display_offset(), 6);
        assert_eq!(selected_lines(&t), ((-6, 0), (2, 1)));
        // History runs out: the view stops at its top and the step says so.
        assert!(!t.drag_scroll_step(60.0, 1.0, 0, None, false).more);
        assert_eq!(t.display_offset(), history);
        assert_eq!(t.scroll.current, history as f32);
        assert!(!t.drag_scroll_step(6.0, 0.1, 0, None, false).more);
        assert_eq!(t.display_offset(), history);
        // Below the bottom scrolls back down, to the live view, and selects to the bottom row.
        assert!(t.drag_scroll_step(-6.0, 0.5, 4, None, false).more);
        assert_eq!(t.display_offset(), history - 3);
        assert!(!t.drag_scroll_step(-60.0, 1.0, 4, None, false).more);
        assert_eq!(t.display_offset(), 0);
        assert_eq!(t.scroll.current, 0.0);
        assert_eq!(selected_lines(&t).1, (4, 4));
    }

    #[test]
    fn a_drag_past_the_edge_never_scrolls_a_program_that_owns_the_mouse() {
        let still = |t: &TerminalCore| t.display_offset() == 0 && t.scroll.current == 0.0 && !t.selection_active();
        let mut t = with_history(20, 5, 30);
        assert_eq!(t.drag_scroll_step(0.0, 0.5, 0, Some((1, 2, Side::Right)), false), DragStep::default(), "a pointer inside is no drag past the edge");
        assert!(still(&t));
        // Mouse reporting: the wheel and the drag are the program's.
        t.apply_output(b"\x1b[?1000h");
        assert_eq!(t.drag_scroll_step(6.0, 0.5, 0, Some((1, 2, Side::Right)), false), DragStep::default());
        assert!(still(&t));
        // The alternate screen, with alternate scroll and without it.
        t.apply_output(b"\x1b[?1000l\x1b[?1049h");
        assert_eq!(t.drag_scroll_step(6.0, 0.5, 0, Some((1, 2, Side::Right)), false), DragStep::default());
        assert!(still(&t));
        t.apply_output(b"\x1b[?1007l");
        assert_eq!(t.mouse_mode(), MouseMode::Local);
        assert_eq!(t.drag_scroll_step(-6.0, 0.5, 0, Some((1, 2, Side::Right)), false), DragStep::default());
        assert!(still(&t));
    }

    #[test]
    fn an_ended_session_scrolls_its_history_on_a_drag_past_the_edge_even_in_report_mode() {
        // The program left mouse reporting on when it died; the wheel scrolls the history of an
        // ended session, and so does a drag past the edge.
        let mut t = with_history(20, 5, 30);
        t.apply_output(b"\x1b[?1000h");
        assert!(matches!(t.mouse_mode(), MouseMode::Report { .. }));
        assert!(t.drag_scroll_step(6.0, 0.5, 3, Some((1, 2, Side::Right)), true).more);
        assert_eq!(t.display_offset(), 3);
        assert_eq!(selected_lines(&t), ((-3, 3), (2, 1)));
        // The alternate screen has no history to scroll, ended or not.
        t.apply_output(b"\x1b[?1000l\x1b[?1049h");
        assert_eq!(t.drag_scroll_step(6.0, 0.5, 3, None, true), DragStep::default());
    }

    #[test]
    fn a_drag_past_the_top_edge_of_a_fullscreen_program_sends_wheel_reports_and_selects_to_the_edge() {
        let mut t = fullscreen(20, 9);
        paint(&mut t, &["l10", "l11", "l12", "l13", "l14", "l15", "l16", "─", "❯"]);
        // Pressed on the last line of the text, column 2.
        t.ledger_press(2, 6, true);
        // 6 rows per second for half a second: three reports at the pointer's column on the top row.
        let step = t.drag_scroll_step(6.0, 0.5, 0, None, false);
        assert!(step.more);
        assert_eq!(step.pty_write, b"\x1b[<64;1;1M".repeat(3));
        assert_eq!(t.selection_text().as_deref(), Some("l10\nl11\nl12\nl13\nl14\nl15\nl16"), "from the top row's left side to the pressed cell");
        // A fraction of a row is carried to the next frame.
        let step = t.drag_scroll_step(6.0, 0.1, 0, None, false);
        assert!(step.pty_write.is_empty() && step.more);
        let step = t.drag_scroll_step(6.0, 0.1, 0, None, false);
        assert_eq!(step.pty_write, b"\x1b[<64;1;1M");
        // The program scrolls two rows per frame; the end stays the top row, an older line each time.
        paint(&mut t, &["l8", "l9", "l10", "l11", "l12", "l13", "l14", "─", "❯"]);
        paint(&mut t, &["l6", "l7", "l8", "l9", "l10", "l11", "l12", "─", "❯"]);
        t.drag_scroll_step(6.0, 0.5, 0, None, false);
        assert_eq!(t.selection_text().as_deref(), Some("l6\nl7\nl8\nl9\nl10\nl11\nl12\nl13\nl14\nl15\nl16"));
        assert_eq!(t.selection_spans().len(), 7, "the tint covers the rows that moved, never the prompt box");
    }

    #[test]
    fn a_drag_past_the_bottom_edge_sends_wheel_down_reports_in_the_programs_encoding() {
        let mut t = TerminalCore::new(20, 8, 100);
        t.apply_output(b"\x1b[?1049h\x1b[?1000h");
        paint(&mut t, &["l10", "l11", "l12", "l13", "l14", "l15", "─", "❯"]);
        t.ledger_press(0, 0, true);
        let step = t.drag_scroll_step(-12.0, 0.25, 3, None, false);
        assert_eq!(step.pty_write, mouse::button_bytes(false, 65, 3, 7, true).repeat(3));
        // Before anything has moved, the snapshot's prompt box lines are still in the record.
        assert_eq!(t.selection_text().as_deref(), Some("l10\nl11\nl12\nl13\nl14\nl15\n─\n❯"));
        // Once the program scrolls, they are dropped and the copy is the text alone.
        paint(&mut t, &["l11", "l12", "l13", "l14", "l15", "l16", "─", "❯"]);
        paint(&mut t, &["l12", "l13", "l14", "l15", "l16", "l17", "─", "❯"]);
        assert_eq!(t.selection_text().as_deref(), Some("l10\nl11\nl12\nl13\nl14\nl15\nl16\nl17"));
    }

    #[test]
    fn reporting_turned_off_mid_record_stops_the_reports_but_keeps_the_record() {
        let mut t = fullscreen(20, 9);
        paint(&mut t, &["l10", "l11", "l12", "l13", "l14", "l15", "l16", "─", "❯"]);
        t.ledger_press(2, 6, true);
        t.apply_all([b"\x1b[?1000l".as_slice()]);
        assert!(t.record_open());
        let step = t.drag_scroll_step(6.0, 0.5, 0, None, false);
        assert!(step.more && step.pty_write.is_empty());
        paint(&mut t, &["l8", "l9", "l10", "l11", "l12", "l13", "l14", "─", "❯"]);
        paint(&mut t, &["l6", "l7", "l8", "l9", "l10", "l11", "l12", "─", "❯"]);
        assert_eq!(t.selection_text().as_deref(), Some("l10\nl11\nl12\nl13\nl14\nl15\nl16"));
    }

    #[test]
    fn an_ended_session_sends_nothing_from_a_drag_past_the_edge() {
        let mut t = fullscreen(20, 9);
        paint(&mut t, &["l10", "l11", "l12", "l13", "l14", "l15", "l16", "─", "❯"]);
        t.ledger_press(0, 6, true);
        assert_eq!(t.drag_scroll_step(6.0, 0.5, 0, None, true), DragStep::default());
    }

    #[test]
    fn resize_reflows_and_reports_modes() {
        let mut t = TerminalCore::new(20, 5, 100);
        t.apply_output(b"\x1b[?1h\x1b[?2004h");
        assert!(t.app_cursor());
        assert!(t.bracketed_paste());
        t.resize(40, 10);
        assert_eq!(t.rows_for_paint().visible.len(), 10);
        assert_eq!(t.rows_for_paint().visible[0].len(), 40);
    }

    /// A stand-in for Claude Code's classic renderer, written from a pty capture of 2.1.327
    /// (2026-11-17). It prints its conversation down the main screen, every row ending in a
    /// newline, and rests the cursor on its prompt. It answers a resize at once by homing the
    /// cursor, erasing every screen row and printing again: the rows it reckons are on screen
    /// (its frame's last rows, as many as the new height shows), wrapped for the new width, and
    /// never fewer than fill the screen when it has them. It reckons on a terminal that left
    /// those rows alone and sent the rows that no longer fit to its history.
    struct Classic {
        paragraphs: Vec<String>,
        cols: usize,
        rows: usize,
    }

    impl Classic {
        /// Texts of differing lengths and no spaces, so the rows of any wrapping join back
        /// into them.
        fn new(paragraphs: usize, cols: usize, rows: usize) -> Self {
            let paragraphs = (0..paragraphs).map(|i| format!("<{i:02}{}>", "abcdefghij".repeat(1 + i % 9))).collect();
            Self { paragraphs, cols, rows }
        }

        /// The frame at a width: every paragraph cut into rows, then the prompt box. Each row
        /// comes with the paragraph it is from and where in it the row starts.
        fn frame(&self, cols: usize) -> Vec<(usize, usize, String)> {
            let mut rows = Vec::new();
            for (p, text) in self.paragraphs.iter().enumerate() {
                let chars: Vec<char> = text.chars().collect();
                for (k, chunk) in chars.chunks(cols).enumerate() {
                    rows.push((p, k * cols, chunk.iter().collect()));
                }
            }
            let n = self.paragraphs.len();
            rows.push((n, 0, "─".repeat(cols)));
            rows.push((n + 1, 0, "❯".to_string()));
            rows.push((n + 2, 0, "─".repeat(cols)));
            rows
        }

        /// The rows, the cursor left a row under the last, then moved up onto the prompt.
        fn print(rows: &[(usize, usize, String)]) -> String {
            let mut s: String = rows.iter().map(|(_, _, text)| format!("{text}\r\n")).collect();
            s.push_str("\x1b[2A\x1b[2C");
            s
        }

        /// What it writes when it starts; it asks for theme pushes (`CSI ? 2031 h`) first.
        fn start(&self) -> Vec<u8> {
            format!("\x1b[?2031h{}", Self::print(&self.frame(self.cols))).into_bytes()
        }

        /// What it writes when told of a new size.
        fn resized(&mut self, cols: usize, rows: usize) -> Vec<u8> {
            let old = self.frame(self.cols);
            let top = old.len().saturating_sub(rows - 1);
            let new = self.frame(cols);
            let (paragraph, offset, _) = &old[top];
            let same_text = new.iter().rposition(|(p, o, _)| p == paragraph && o <= offset).expect("the row is in the frame");
            let start = same_text.min(new.len().saturating_sub(rows - 1));
            (self.cols, self.rows) = (cols, rows);
            format!("\x1b[2D\x1b[2B\x1b[H{}\x1b[H{}", "\x1b[2K\x1b[1B".repeat(rows), Self::print(&new[start..])).into_bytes()
        }

        /// What it writes when it compacts, or at `/clear`: the same erase as at a resize, with
        /// nothing asked of it, and then a frame that no longer holds the conversation.
        fn compacted(&mut self) -> Vec<u8> {
            self.paragraphs = vec!["<Conversation-compacted>".to_string()];
            format!("\x1b[2D\x1b[2B\x1b[H{}\x1b[H{}", "\x1b[2K\x1b[1B".repeat(self.rows), Self::print(&self.frame(self.cols))).into_bytes()
        }

        /// The whole conversation and the prompt box as one string, as the rows of a terminal
        /// that kept all of it once join up.
        fn whole(&self) -> String {
            self.frame(self.cols).into_iter().map(|(_, _, text)| text).collect()
        }
    }

    /// Every row of the history and the screen, oldest first, without trailing blanks.
    fn all_rows(t: &TerminalCore) -> Vec<String> {
        use alacritty_terminal::index::Column;
        let grid = t.term.grid();
        (-(grid.history_size() as i32)..grid.screen_lines() as i32)
            .map(|l| (0..grid.columns()).map(|c| grid[Line(l)][Column(c)].c).collect::<String>().trim_end().to_string())
            .collect()
    }

    /// A terminal with classic Claude Code started in it, and the program.
    fn classic(paragraphs: usize, cols: usize, rows: usize) -> (TerminalCore, Classic) {
        let program = Classic::new(paragraphs, cols, rows);
        let mut t = TerminalCore::new(cols, rows, 1000);
        t.apply_output(&program.start());
        (t, program)
    }

    /// The pane changes size: the terminal first, then the program's answer.
    fn resize_with(t: &mut TerminalCore, program: &mut Classic, cols: usize, rows: usize) {
        t.resize(cols, rows);
        t.apply_output(&program.resized(cols, rows));
    }

    #[test]
    fn the_stand_in_fills_a_terminal_as_classic_claude_code_does() {
        let (t, program) = classic(40, 80, 24);
        assert_eq!(all_rows(&t).concat(), program.whole());
        let rows = all_rows(&t);
        assert_eq!(rows[rows.len() - 3..], ["❯".to_string(), "─".repeat(80), String::new()], "the prompt box, then the row the frame's last newline left");
        assert_eq!(t.term.grid().cursor.point.line, Line(21), "the cursor rests on the prompt");
        assert!(t.program_redraws());
    }

    #[test]
    fn a_narrowed_pane_leaves_classic_claude_codes_screen_to_it_and_repeats_nothing_in_the_history() {
        let (mut t, mut program) = classic(40, 80, 24);
        let before = all_rows(&t);
        t.resize(60, 24);
        let after = all_rows(&t);
        let screen = |rows: &[String]| rows[rows.len() - 24..].to_vec();
        let cut: Vec<String> = screen(&before).iter().map(|r| r.chars().take(60).collect()).collect();
        assert_eq!(screen(&after), cut, "each screen row is still its own row, cut at the new width");
        assert_eq!(t.term.grid().cursor.point.line, Line(21), "the cursor is on the row it was on");
        t.apply_output(&program.resized(60, 24));
        assert_eq!(all_rows(&t).concat(), program.whole(), "nothing twice, nothing missing, no old rule left behind");
    }

    /// The program starts its reprint at the start of a row of the new wrapping, which can be a
    /// few characters before the text that was at the top of the screen: those few are in the
    /// history already, and no terminal can know it.
    #[test]
    fn a_pane_dragged_narrower_a_column_at_a_time_loses_nothing_and_repeats_less_than_a_row() {
        let (mut t, mut program) = classic(40, 80, 24);
        for cols in (60..80).rev() {
            resize_with(&mut t, &mut program, cols, 24);
        }
        let (all, whole) = (all_rows(&t).concat(), program.whole());
        let mut shown = all.chars();
        assert!(whole.chars().all(|c| shown.any(|s| s == c)), "all of the conversation is there, in order");
        let repeated = all.chars().count() - whole.chars().count();
        assert!(repeated < 60, "{repeated} characters twice over twenty resizes");
        assert_eq!(all.matches('❯').count(), 1, "one prompt box");
    }

    #[test]
    fn a_shortened_pane_sends_the_rows_that_no_longer_fit_to_the_history_and_loses_none() {
        let (mut t, mut program) = classic(40, 80, 24);
        let history = t.history_size();
        t.resize(80, 14);
        assert_eq!(t.history_size(), history + 10, "the ten rows the pane lost");
        assert_eq!(t.term.grid().cursor.point.line, Line(11), "the cursor is still on the prompt");
        t.apply_output(&program.resized(80, 14));
        assert_eq!(all_rows(&t).concat(), program.whole());
    }

    #[test]
    fn a_pane_shortened_and_made_tall_again_reads_as_it_did() {
        let (mut t, mut program) = classic(40, 80, 24);
        let before = all_rows(&t);
        resize_with(&mut t, &mut program, 80, 14);
        resize_with(&mut t, &mut program, 80, 24);
        assert_eq!(all_rows(&t), before);
    }

    #[test]
    fn a_conversation_shorter_than_its_pane_stays_at_the_top_when_the_pane_shortens() {
        let (mut t, mut program) = classic(3, 80, 24);
        let before = all_rows(&t).concat();
        resize_with(&mut t, &mut program, 80, 14);
        assert_eq!(t.history_size(), 0, "it all still fits, so nothing went to the history");
        assert_eq!(all_rows(&t).concat(), before);
    }

    #[test]
    fn a_widened_pane_loses_nothing() {
        let (mut t, mut program) = classic(40, 60, 24);
        resize_with(&mut t, &mut program, 80, 24);
        let all = all_rows(&t).concat();
        for paragraph in &program.paragraphs {
            assert!(all.contains(paragraph.as_str()), "{paragraph} is whole somewhere");
        }
        assert!(all.ends_with(&format!("{}❯{}", "─".repeat(80), "─".repeat(80))), "one prompt box, at the new width");
        assert_eq!(all.matches('❯').count(), 1);
    }

    /// Rows the terminal wrapped itself hold more text each than the rows the program would
    /// print, so taking as many of them back as the pane grew would take text the program does
    /// not print again.
    #[test]
    fn a_pane_made_taller_after_it_narrowed_takes_back_only_rows_printed_at_its_width() {
        let (mut t, mut program) = classic(40, 80, 14);
        resize_with(&mut t, &mut program, 60, 14);
        resize_with(&mut t, &mut program, 60, 40);
        let all = all_rows(&t).concat();
        for paragraph in &program.paragraphs {
            assert!(all.contains(paragraph.as_str()), "{paragraph} is whole somewhere");
        }
        assert_eq!(all.matches('❯').count(), 1);
    }

    #[test]
    fn a_screen_classic_claude_code_erases_unasked_goes_to_the_history_first() {
        let (mut t, mut program) = classic(40, 80, 24);
        let before = program.whole();
        t.apply_output(&program.compacted());
        assert_eq!(all_rows(&t).concat(), format!("{before}{}", program.whole()), "the conversation, its last screen included, then the new frame");
    }

    #[test]
    fn the_erase_is_seen_however_the_output_is_cut_into_reads() {
        for size in [1, 3, 7, 1024] {
            let (mut t, mut program) = classic(40, 80, 24);
            let before = program.whole();
            for chunk in program.compacted().chunks(size) {
                t.apply_output(chunk);
            }
            assert_eq!(all_rows(&t).concat(), format!("{before}{}", program.whole()), "reads of {size}");
        }
    }

    #[test]
    fn a_repaint_long_after_a_resize_is_not_its_answer() {
        let (mut t, mut program) = classic(40, 80, 24);
        // The program never answers this resize (its terminal's size did not change).
        t.resize(80, 23);
        t.resize(80, 24);
        let before = all_rows(&t).concat();
        t.apply_output("\x1b[s\x1b[u".repeat(4096).as_bytes());
        t.apply_output(&program.compacted());
        assert_eq!(all_rows(&t).concat(), format!("{before}{}", program.whole()));
    }

    #[test]
    fn a_shell_that_homes_and_erases_a_line_sends_nothing_to_the_history() {
        let mut t = TerminalCore::new(40, 5, 100);
        t.apply_output(b"one\r\ntwo\r\n\x1b[H\x1b[2Kthree");
        assert_eq!(t.history_size(), 0);
        assert_eq!(all_rows(&t)[..2], ["three".to_string(), "two".to_string()]);
    }

    #[test]
    fn scrolled_up_classic_claude_code_keeps_its_input_box_for_the_panes_foot() {
        // Cyrus, 2026-11-17: "is it possible to keep the claude user input footer fixed to the
        // bottom of the screen despite user scroll position?"
        let (mut t, _) = classic(40, 80, 24);
        assert!(t.pinned_frame().is_none(), "at the live bottom the box is where it is");
        t.set_display_offset(7);
        let frame = t.pinned_frame().expect("scrolled up, the box is pinned");
        let text: Vec<String> = frame.rows.iter().map(|r| text_of(r)).collect();
        assert_eq!(text, ["─".repeat(80), "❯".to_string(), "─".repeat(80)], "the box, and nothing of the conversation");
        assert_eq!(frame.cursor, Some((2, 1)), "the cursor in it, on the prompt's row");
        // The status lines under the box come with it.
        t.set_display_offset(0);
        t.apply_output(b"\x1b[2B\r  Opus 5.5 | project\r\n  accept edits on\x1b[4A\x1b[2C");
        t.set_display_offset(3);
        let text: Vec<String> = t.pinned_frame().unwrap().rows.iter().map(|r| text_of(r)).collect();
        assert_eq!(text[3..], ["  Opus 5.5 | project".to_string(), "  accept edits on".to_string()]);
    }

    #[test]
    fn nothing_is_pinned_for_a_shell_or_when_the_box_is_not_on_screen() {
        // A shell, scrolled up.
        let mut shell = TerminalCore::new(40, 5, 100);
        shell.apply_output("line\r\n".repeat(20).as_bytes());
        shell.set_display_offset(4);
        assert!(shell.pinned_frame().is_none());
        // Claude Code showing something else where its box was (a question, a menu).
        let (mut t, _) = classic(40, 80, 24);
        t.apply_output(format!("\x1b[1A\r\x1b[J{}\r\n  Allow this command?\r\n  1. Yes", "─".repeat(80)).as_bytes());
        t.set_display_offset(5);
        assert!(t.pinned_frame().is_none());
    }

    #[test]
    fn the_input_box_is_the_last_prompt_under_a_rule_and_all_that_follows() {
        let rows = |lines: &[&str]| -> Vec<Vec<char>> { lines.iter().map(|l| l.chars().collect()).collect() };
        let rule = "─".repeat(40);
        // A past prompt in the conversation opens with the same mark but hangs under no rule.
        assert_eq!(input_frame(&rows(&["❯ an old message", "", "an answer", &rule, "❯ typing", "  a second line", &rule, "  status", ""])), Some((3, 7)));
        assert_eq!(input_frame(&rows(&["❯ an old message", "", "an answer"])), None);
        assert_eq!(input_frame(&rows(&[&rule, "not a prompt", &rule])), None);
        assert_eq!(input_frame(&rows(&["──", "❯ a short dash is no rule"])), None);
        assert_eq!(input_frame(&rows(&[])), None);
    }

    #[test]
    fn a_box_taller_than_half_the_pane_is_not_pinned() {
        let (mut t, _) = classic(40, 80, 5);
        t.set_display_offset(4);
        assert!(t.pinned_frame().is_none(), "three rows of box in a five row pane would leave little to read");
        let (mut t, _) = classic(40, 80, 6);
        t.set_display_offset(4);
        assert!(t.pinned_frame().is_some(), "half the pane at most");
    }

    #[test]
    fn typing_keeps_the_place_while_the_box_is_pinned_and_enter_goes_to_the_bottom() {
        assert!(key_returns_to_bottom(false, "a"), "as every terminal does");
        assert!(!key_returns_to_bottom(true, "a") && !key_returns_to_bottom(true, "backspace"));
        assert!(key_returns_to_bottom(true, "enter"), "the answer will be at the bottom");
    }

    #[test]
    fn a_shells_screen_still_reflows_when_its_pane_narrows() {
        let mut t = TerminalCore::new(40, 5, 100);
        t.apply_output(format!("{}{}", "a".repeat(40), "b".repeat(20)).as_bytes());
        assert!(!t.program_redraws());
        t.resize(30, 5);
        let rows = all_rows(&t);
        assert_eq!(rows[..2], ["a".repeat(30), format!("{}{}", "a".repeat(10), "b".repeat(20))], "the line is wrapped, not cut");
    }

    #[test]
    fn a_program_on_the_alternate_screen_is_resized_as_before() {
        let mut t = fullscreen(40, 10);
        t.apply_output(b"\x1b[?2031h");
        assert!(!t.program_redraws(), "a full-screen program has no history to keep whole");
    }

    #[test]
    fn the_agents_own_word_marks_a_terminal_whose_program_redraws_its_screen() {
        let mut t = TerminalCore::new(80, 24, 100);
        assert!(!t.program_redraws());
        t.set_agent_redraws(true);
        assert!(t.program_redraws());
        t.set_agent_redraws(false);
        assert!(!t.program_redraws());
        // Classic Claude Code turns theme pushes off as it leaves.
        t.apply_output(b"\x1b[?2031h");
        assert!(t.program_redraws());
        t.apply_output(b"\x1b[?2031l");
        assert!(!t.program_redraws());
    }

    #[test]
    fn a_replay_is_parsed_at_the_sizes_its_marks_record() {
        use protocol::replay::{now_mark, resize_mark};
        // What a window that was open the whole time shows.
        let (mut live, mut program) = classic(40, 100, 30);
        let mut replay = program.start();
        for (cols, rows) in [(60, 30), (60, 14), (90, 20)] {
            replay.extend(resize_mark((program.rows as u16, program.cols as u16), (rows as u16, cols as u16)));
            let answer = program.resized(cols, rows);
            live.resize(cols, rows);
            live.apply_output(&answer);
            replay.extend(answer);
        }
        replay.extend(now_mark((20, 90)));
        // A window opened later, in a pane of another size, parsing the replay.
        let mut later = TerminalCore::new(72, 18, 1000);
        later.apply_replay(&replay);
        assert_eq!((later.columns(), later.screen_lines()), (90, 20), "it ends at the size the session has");
        assert_eq!(all_rows(&later), all_rows(&live));
    }

    /// A replay is hours of output in one piece: the program that redraws is known from where
    /// it asked for theme pushes to where it turned them off, not by the last word in the piece.
    #[test]
    fn a_replay_that_ends_after_claude_code_left_still_keeps_the_screens_it_erased() {
        let (_, mut program) = classic(40, 80, 24);
        let mut replay = program.start();
        let before = program.whole();
        replay.extend(program.compacted());
        let after = program.whole();
        replay.extend(b"\x1b[?2031l");
        let mut t = TerminalCore::new(80, 24, 1000);
        t.apply_replay(&replay);
        assert_eq!(all_rows(&t).concat(), format!("{before}{after}"));
        assert!(!t.program_redraws());
    }

    #[test]
    fn output_is_cut_before_each_mark_and_loses_no_byte() {
        let cut = |bytes: &'static [u8]| cut_before(bytes, b"ab");
        assert_eq!(cut(b"xxabyyabzz"), [&b"xx"[..], b"abyy", b"abzz"]);
        assert_eq!(cut(b"abxab"), [&b"abx"[..], b"ab"]);
        assert_eq!(cut(b"none"), [&b"none"[..]]);
        assert!(cut(b"").is_empty());
        assert_eq!(cut(b"a"), [&b"a"[..]]);
    }

    #[test]
    fn a_replay_without_marks_is_parsed_at_the_panes_size() {
        let mut t = TerminalCore::new(20, 5, 100);
        let w = t.apply_replay(b"\x1b]0;a title\x07hello");
        assert_eq!(w.title.as_deref(), Some("a title"));
        assert_eq!((t.columns(), t.screen_lines()), (20, 5));
        assert_eq!(all_rows(&t)[0], "hello");
    }

    #[test]
    fn replay_from_mid_sequence_recovers() {
        let mut t = TerminalCore::new(20, 5, 100);
        // Cut into the middle of an SGR sequence, then normal text.
        t.apply_output(b"1;32mgreen\x1b[0m plain");
        let rows = t.rows_for_paint();
        let line = text_of(&rows.visible[0]);
        assert_eq!(line, "1;32mgreen plain");
        let (_, style, _) = rows.visible[0][11];
        assert_eq!(rows.visible[0][11].0, 'p');
        assert_eq!(style.fg, NAVY.fg);
        assert!(!style.bold);
    }

    #[test]
    fn styles_come_through() {
        let mut t = TerminalCore::new(20, 5, 100);
        t.apply_output(b"\x1b[1;31mX\x1b[0m");
        let rows = t.rows_for_paint();
        let (c, style, _) = rows.visible[0][0];
        assert_eq!(c, 'X');
        assert!(style.bold);
        assert_eq!(style.fg, NAVY.ansi[1]);
    }

    /// A TUI's replay is cursor addressed for the PTY's size; parsed at the wrong width it lands
    /// in the wrong cells, which is why the window applies it at its measured grid.
    #[test]
    fn replay_with_wide_cursor_addressing_needs_the_real_width() {
        let replay = b"\x1b[2J\x1b[1;1Hleft\x1b[1;100Hright\x1b[3;120Hend";
        let mut wide = TerminalCore::new(124, 10, 100);
        wide.apply_output(replay);
        let rows = wide.rows_for_paint();
        let line0 = text_of(&rows.visible[0]);
        assert_eq!(&line0[..4], "left");
        assert_eq!(&line0[99..], "right");
        assert_eq!(&text_of(&rows.visible[2])[119..], "end");

        let mut narrow = TerminalCore::new(80, 10, 100);
        narrow.apply_output(replay);
        let rows = narrow.rows_for_paint();
        let line0 = text_of(&rows.visible[0]);
        assert_ne!(line0.get(99..), Some("right"), "80 columns cannot place column 100");
        assert_ne!(text_of(&rows.visible[2]).get(119..), Some("end"));
    }

    /// The replay is parsed on the background executor, so the core must move between threads.
    #[test]
    fn terminal_core_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<TerminalCore>();
    }

    #[test]
    fn clear_to_start_keeps_only_the_cursor_line_at_the_top() {
        let mut t = TerminalCore::new(20, 4, 100);
        for i in 0..10 {
            t.apply_output(format!("line{i}\r\n").as_bytes());
        }
        t.apply_output(b"prompt> ls");
        assert!(t.history_size() > 0);
        assert_eq!(t.cursor(), Some((10, 3)), "the prompt is on the last row (column, row)");
        t.scroll.jump_to(3.0);
        t.set_display_offset(3);
        t.clear_to_start();
        assert_eq!(t.history_size(), 0);
        assert_eq!(t.display_offset(), 0);
        assert!(t.scroll.is_at_bottom());
        let rows = t.rows_for_paint();
        assert_eq!(text_of(&rows.visible[0]), "prompt> ls");
        assert!(rows.visible[1..].iter().all(|r| text_of(r).trim().is_empty()), "nothing below the prompt");
        assert_eq!(t.cursor(), Some((10, 0)), "the cursor keeps its column on the top row");
        // Output after the clear lands where the shell expects it.
        t.apply_output(b"\r\nnext");
        assert_eq!(text_of(&t.rows_for_paint().visible[1]), "next");
    }

    #[test]
    fn set_palette_recolours_parsed_cells() {
        use crate::palette::{rgb, Palette, NAVY};
        let mut core = TerminalCore::new(10, 2, 100);
        core.apply_output(b"\x1b[31mred\x1b[0m plain");
        let before = core.rows_for_paint();
        assert_eq!(before.visible[0][0].1.fg, NAVY.ansi[1]);
        assert_eq!(before.visible[0][4].1.fg, NAVY.fg);
        assert_eq!(before.visible[0][4].1.bg, NAVY.bg);
        let mut light = Palette { ..NAVY };
        light.bg = rgb(0xf4f6fa);
        light.fg = rgb(0x1e2633);
        light.ansi[1] = rgb(0xb3261e);
        core.set_palette(std::sync::Arc::new(light));
        // Nothing was re-parsed: the same cells resolve against the new palette at paint time.
        let after = core.rows_for_paint();
        assert_eq!(after.visible[0][0].1.fg, rgb(0xb3261e));
        assert_eq!(after.visible[0][4].1.fg, rgb(0x1e2633));
        assert_eq!(after.visible[0][4].1.bg, rgb(0xf4f6fa));
        assert_eq!(core.palette().bg, rgb(0xf4f6fa));
    }

    #[test]
    fn a_background_colour_query_is_answered_from_the_palette() {
        use crate::palette::{rgb, Palette, NAVY};
        let mut core = TerminalCore::new(10, 2, 100);
        let w = core.apply_output(b"\x1b]11;?\x1b\\");
        let reply = String::from_utf8(w.pty_write).unwrap();
        assert_eq!(reply, "\x1b]11;rgb:1111/1616/1f1f\x1b\\", "navy's background, 16 bits a channel");
        let mut light = Palette { ..NAVY };
        light.bg = rgb(0xfff6ef);
        light.fg = rgb(0x3b2418);
        core.set_palette(std::sync::Arc::new(light));
        let w = core.apply_output(b"\x1b]10;?\x07\x1b]11;?\x07");
        let reply = String::from_utf8(w.pty_write).unwrap();
        assert!(reply.contains("rgb:3b3b/2424/1818") && reply.contains("rgb:ffff/f6f6/efef"), "{reply:?}");
        // An index with no colour gets no answer, and nothing else is written.
        let w = core.apply_output(b"\x1b]4;999;?\x07");
        assert!(w.pty_write.is_empty());
    }

    #[test]
    fn a_program_that_asks_for_theme_pushes_hears_each_switch() {
        use crate::palette::{rgb, Palette, NAVY};
        let mut core = TerminalCore::new(10, 2, 100);
        let mut light = Palette { ..NAVY };
        light.bg = rgb(0xf3effa);
        assert_eq!(core.set_palette(std::sync::Arc::new(light.clone())), None, "nobody asked yet");
        // The query alone is answered, from the palette in use.
        assert_eq!(core.apply_output(b"\x1b[?996n").pty_write, b"\x1b[?997;2n");
        // Mode 2031 set inside a list with another mode, split across two chunks.
        assert!(core.apply_output(b"text\x1b[?2004;20").pty_write.is_empty());
        core.apply_output(b"31h more");
        assert_eq!(core.set_palette(std::sync::Arc::new(Palette { ..NAVY })), Some(&b"\x1b[?997;1n"[..]), "navy is dark");
        assert_eq!(core.set_palette(std::sync::Arc::new(light.clone())), Some(&b"\x1b[?997;2n"[..]));
        // The fade's frames recolour the cells without a push; the final palette still pushes.
        core.set_palette_quiet(std::sync::Arc::new(Palette { ..NAVY }));
        assert_eq!(core.palette().bg, NAVY.bg, "a quiet swap recolours the paint");
        assert_eq!(core.set_palette(std::sync::Arc::new(light)), Some(&b"\x1b[?997;2n"[..]), "and the end of a fade is heard");
        // Reset: switches are silent again, and the grid saw only the text.
        core.apply_output(b"\x1b[?2031l");
        assert_eq!(core.set_palette(std::sync::Arc::new(Palette { ..NAVY })), None);
        assert!(text_of(&core.rows_for_paint().visible[0]).starts_with("text more"));
    }

    #[test]
    fn the_theme_scan_carries_only_unfinished_csi_sequences() {
        assert_eq!(scan_theme_asks(b"", b"plain \x1b[1mbold"), (ThemeAsks::default(), Vec::new()));
        assert_eq!(scan_theme_asks(b"", b"end\x1b").1, b"\x1b");
        assert_eq!(scan_theme_asks(b"", b"end\x1b[?99").1, b"\x1b[?99");
        assert_eq!(scan_theme_asks(b"\x1b[?99", b"6n").0, ThemeAsks { updates: None, query: true });
        assert_eq!(scan_theme_asks(b"", b"\x1b[?2031h\x1b[?2031l").0.updates, Some(false), "the last one wins");
        assert_eq!(scan_theme_asks(b"", b"\x1b[?12031h").0.updates, None, "only 2031 itself");
    }

    #[test]
    fn escape_makes_metacharacters_literal() {
        assert_eq!(escape_regex("a.b*c"), "a\\.b\\*c");
        assert_eq!(escape_regex("(x)|[y]{2}^$\\"), "\\(x\\)\\|\\[y\\]\\{2\\}\\^\\$\\\\");
        assert_eq!(escape_regex("plain words"), "plain words");
        let mut core = TerminalCore::new(20, 3, 100);
        core.apply_output(b"a.b ab a*b");
        let dots = core.search(".");
        assert_eq!(dots.len(), 1);
        assert_eq!((dots[0].start().line.0, dots[0].start().column.0), (0, 1));
        assert_eq!(core.search("*").len(), 1);
        assert_eq!(core.search("(").len(), 0, "no panic, no match");
        assert_eq!(core.search("\\").len(), 0);
        assert_eq!(core.search("").len(), 0);
    }

    #[test]
    fn search_is_literal_and_case_insensitive() {
        let mut core = TerminalCore::new(30, 3, 100);
        core.apply_output(b"Hello hello HELLO");
        let m = core.search("hello");
        let cols: Vec<usize> = m.iter().map(|m| m.start().column.0).collect();
        assert_eq!(cols, vec![0, 6, 12]);
        assert_eq!(m[0].end().column.0, 4, "the match covers the whole word");
        assert_eq!(core.search("HeLLo").len(), 3);
        assert_eq!(core.search(" hello").len(), 2, "a space is part of the query (blank cells are spaces too)");
        assert_eq!(core.search("xyz").len(), 0);
    }

    #[test]
    fn search_covers_history_in_order_and_stops_after_one_pass() {
        let mut core = TerminalCore::new(10, 2, 100);
        core.apply_output(b"ab\r\nab\r\nab\r\nab");
        assert_eq!(core.history_size(), 2);
        let m = core.search("ab");
        let lines: Vec<i32> = m.iter().map(|m| m.start().line.0).collect();
        assert_eq!(lines, vec![-2, -1, 0, 1], "oldest first, no wrap-around duplicates");
        let one = TerminalCore::new(10, 2, 100);
        assert!(one.search("ab").is_empty());
        let mut single = TerminalCore::new(10, 2, 100);
        single.apply_output(b"xx ab xx");
        assert_eq!(single.search("ab").len(), 1, "a lone match is found once, not forever");
    }

    #[test]
    fn highlights_follow_the_viewport_and_reveal_scrolls_the_least() {
        use alacritty_terminal::index::Line;
        let mut core = TerminalCore::new(10, 2, 100);
        core.apply_output(b"ab\r\nab\r\nab\r\nab");
        let m = core.search("ab");
        let h = core.highlights(&m, 3);
        assert_eq!(h.len(), 2, "only the two screen lines are visible at offset 0");
        assert_eq!(h[0], Highlight { row: 0, start: 0, end: 1, current: false });
        assert_eq!(h[1], Highlight { row: 1, start: 0, end: 1, current: true });
        core.reveal_line(Line(1));
        assert_eq!(core.display_offset(), 0, "a visible line scrolls nothing");
        core.reveal_line(Line(-2));
        assert_eq!(core.display_offset(), 2, "the oldest line comes to the top");
        let h = core.highlights(&m, 0);
        assert_eq!(h[0], Highlight { row: 0, start: 0, end: 1, current: true });
        assert_eq!(h[1], Highlight { row: 1, start: 0, end: 1, current: false });
        core.reveal_line(Line(1));
        assert_eq!(core.display_offset(), 0, "the newest line comes to the bottom");
        core.reveal_line(Line(-1));
        assert_eq!(core.display_offset(), 1, "the smallest scroll that shows it");
        assert_eq!(core.scroll.current, 1.0, "the animator lands with it, no glide");
    }

    #[test]
    fn the_cap_keeps_the_newest_matches() {
        let mut core = TerminalCore::new(10, 2, 100);
        core.apply_output(b"ab\r\nab\r\nab\r\nab\r\nab\r\nab");
        assert_eq!(core.history_size(), 4);
        let m = core.search_capped("ab", 3);
        let lines: Vec<i32> = m.iter().map(|m| m.start().line.0).collect();
        assert_eq!(lines, vec![-1, 0, 1], "the oldest matches are the ones dropped, oldest first");
        assert_eq!(core.search_capped("ab", 100).len(), 6);
        assert_eq!(core.search("ab").len(), 6);
    }

    #[test]
    fn matches_never_overlap_and_the_grid_corners_are_found() {
        let mut core = TerminalCore::new(10, 2, 100);
        core.apply_output(b"aaaa");
        let cols: Vec<(usize, usize)> = core.search("aa").iter().map(|m| (m.start().column.0, m.end().column.0)).collect();
        assert_eq!(cols, vec![(0, 1), (2, 3)]);
        let mut corners = TerminalCore::new(4, 2, 100);
        corners.apply_output(b"ab  \r\n  ab");
        let m = corners.search("ab");
        let ends: Vec<(i32, usize, i32, usize)> = m.iter().map(|m| (m.start().line.0, m.start().column.0, m.end().line.0, m.end().column.0)).collect();
        assert_eq!(ends, vec![(0, 0, 0, 1), (1, 2, 1, 3)], "the first and the last cell of the grid");
    }

    /// A fullscreen program that reports the mouse: the alternate screen, button reports, SGR.
    fn fullscreen(cols: usize, rows: usize) -> TerminalCore {
        let mut t = TerminalCore::new(cols, rows, 100);
        t.apply_output(b"\x1b[?1049h\x1b[?1000h\x1b[?1006h");
        t
    }

    /// Redraws every row in one batch, as a program's frame arrives.
    fn paint(t: &mut TerminalCore, rows: &[&str]) {
        let bytes: String = rows.iter().enumerate().map(|(i, text)| format!("\x1b[{};1H{text}\x1b[K", i + 1)).collect();
        t.apply_all([bytes.as_bytes()]);
    }

    /// A press at `from` and a drag to `to`, as `Terminal` drives the core (lines equal rows on
    /// the alternate screen).
    fn drag(t: &mut TerminalCore, from: (usize, usize), to: (usize, usize)) {
        t.ledger_press(from.0, from.1, true);
        t.selection_start(from.0, Line(from.1 as i32), Side::Left, SelectionType::Simple);
        t.selection_update(to.0, Line(to.1 as i32), Side::Right);
    }

    #[test]
    fn a_record_opens_only_for_a_live_program_that_reports_the_mouse_on_the_alternate_screen() {
        let mut t = TerminalCore::new(20, 5, 100);
        t.ledger_press(0, 0, true);
        assert!(!t.record_open(), "the primary screen");
        t.apply_output(b"\x1b[?1049h");
        t.ledger_press(0, 0, true);
        assert!(!t.record_open(), "arrows mode");
        t.apply_output(b"\x1b[?1000h");
        t.ledger_press(0, 0, false);
        assert!(!t.record_open(), "an ended session");
        t.ledger_press(0, 0, true);
        assert!(t.record_open());
        assert!(!t.selection_active(), "an anchor alone selects nothing");
    }

    #[test]
    fn a_click_in_a_fullscreen_program_opens_and_closes_the_record_without_a_tint() {
        let mut t = fullscreen(20, 5);
        paint(&mut t, &["one", "two", "three", "─", "❯"]);
        // A press, then the release: no end was ever set, so the record goes.
        t.ledger_press(1, 1, true);
        assert!(t.record_open());
        t.ledger_release();
        assert!(!t.record_open());
        assert!(t.selection_spans().is_empty());
        // The press of a single click clears the selection first, which also closes the record.
        t.ledger_press(1, 1, true);
        t.selection_clear();
        assert!(!t.record_open());
    }

    #[test]
    fn a_selection_in_a_fullscreen_program_keeps_its_text_while_the_program_scrolls() {
        let mut t = fullscreen(20, 8);
        paint(&mut t, &["l10", "l11", "l12", "l13", "l14", "l15", "─", "❯"]);
        drag(&mut t, (0, 1), (2, 2));
        assert_eq!(t.selection_text().as_deref(), Some("l11\nl12"));
        paint(&mut t, &["l9", "l10", "l11", "l12", "l13", "l14", "─", "❯"]);
        paint(&mut t, &["l8", "l9", "l10", "l11", "l12", "l13", "─", "❯"]);
        assert_eq!(t.selection_text().as_deref(), Some("l11\nl12"));
        let spans = t.selection_spans();
        assert_eq!(spans.first().map(|s| s.row), Some(3), "the tint moved with the text");
        assert_eq!(spans.len(), 2);
    }

    #[test]
    fn output_that_erases_a_tinted_row_does_not_lose_the_tint() {
        let mut t = fullscreen(20, 6);
        paint(&mut t, &["l10", "l11", "l12", "l13", "⠋ thinking", "❯"]);
        drag(&mut t, (0, 1), (2, 2));
        paint(&mut t, &["l10", "l11", "l12", "l13", "⠙ thinking", "❯"]);
        assert_eq!(t.selection_spans().len(), 2);
        assert!(t.selection_active());
    }

    #[test]
    fn a_double_click_in_a_fullscreen_program_records_the_word() {
        let mut t = fullscreen(20, 10);
        paint(&mut t, &["alpha beta", "gamma", "delta", "eps", "zeta", "eta", "theta", "iota", "─", "❯"]);
        t.ledger_press(7, 0, true);
        t.selection_start(7, Line(0), Side::Left, SelectionType::Semantic);
        assert_eq!(t.selection_text().as_deref(), Some("beta"));
        paint(&mut t, &["pre", "alpha beta", "gamma", "delta", "eps", "zeta", "eta", "theta", "─", "❯"]);
        assert_eq!(t.selection_text().as_deref(), Some("beta"));
        assert_eq!(t.selection_spans().first().map(|s| (s.row, s.start, s.end)), Some((1, 6, 9)));
    }

    #[test]
    fn leaving_the_alternate_screen_closes_the_record_and_the_shell_selects_as_before() {
        let mut t = fullscreen(20, 5);
        paint(&mut t, &["one", "two", "three", "─", "❯"]);
        drag(&mut t, (0, 0), (2, 1));
        t.apply_all([b"\x1b[?1000l\x1b[?1049l".as_slice()]);
        assert!(!t.record_open());
        t.apply_all([b"hello\r\nworld".as_slice()]);
        t.selection_start(0, Line(0), Side::Left, SelectionType::Simple);
        t.selection_update(4, Line(0), Side::Right);
        assert_eq!(t.selection_text().as_deref(), Some("hello"));
    }

    #[test]
    fn a_resize_and_a_redraw_beyond_recognition_close_the_record() {
        let mut t = fullscreen(20, 6);
        paint(&mut t, &["l1", "l2", "l3", "l4", "l5", "❯"]);
        drag(&mut t, (0, 0), (2, 1));
        t.resize(30, 6);
        assert!(!t.record_open());
        let mut t = fullscreen(20, 6);
        paint(&mut t, &["l1", "l2", "l3", "l4", "l5", "❯"]);
        drag(&mut t, (0, 0), (2, 1));
        paint(&mut t, &["x1", "x2", "x3", "x4", "x5", "❯"]);
        assert!(!t.record_open());
    }

    #[test]
    fn copying_from_a_record_joins_the_programs_wraps_as_the_grid_copy_does() {
        let mut t = fullscreen(30, 5);
        // The first row runs to within a word of the width: a wrap the program made, joined on copy.
        paint(&mut t, &["\u{25cf} The limiter is done and all", "  six tests pass.", "", "─", "❯"]);
        drag(&mut t, (0, 0), (16, 1));
        assert_eq!(t.selection_text().as_deref(), Some("\u{25cf} The limiter is done and all\n  six tests pass."));
        assert_eq!(t.copy_text().as_deref(), Some("\u{25cf} The limiter is done and all six tests pass."));
    }

    #[test]
    fn a_click_in_a_fullscreen_program_reaches_it_at_the_release_and_leaves_no_record() {
        let mut t = fullscreen(20, 5);
        paint(&mut t, &["one", "two", "three", "─", "❯"]);
        let mut clicks = mouse::Clicks::default();
        t.pressed(&mut clicks, 1, 1, Side::Left, 1, true, true);
        assert!(t.record_open(), "the press opened a record after the single click's clear");
        assert!(!t.selection_active());
        let mut want = mouse::button_bytes(true, 0, 1, 1, true);
        want.extend(mouse::button_bytes(true, 0, 1, 1, false));
        assert_eq!(t.released(&mut clicks, 1, 1), Some(want));
        assert!(!t.record_open());
        assert!(t.selection_spans().is_empty());
    }

    #[test]
    fn a_press_dragged_out_past_the_top_edge_and_back_sends_no_click() {
        let mut t = fullscreen(20, 9);
        paint(&mut t, &["l10", "l11", "l12", "l13", "l14", "l15", "l16", "─", "❯"]);
        let mut clicks = mouse::Clicks::default();
        // Pressed on the top row, then out past the top edge without crossing a cell boundary.
        t.pressed(&mut clicks, 2, 0, Side::Left, 1, true, true);
        let step = t.drag_scroll_step(6.0, 0.5, 2, None, false);
        assert!(step.more, "the edge step acted");
        // As `Terminal::drag_scroll_step` does when the step acts.
        clicks.dragged();
        // Back in over the pressed cell, and released there.
        let gesture = clicks.moved(2, 0, Side::Right, true, t.selection_active());
        assert_eq!(gesture, Gesture::Extend { col: 2, line: 0, side: Side::Right });
        t.apply_gesture(gesture);
        assert_eq!(t.released(&mut clicks, 2, 0), None);
        assert!(t.selection_text().is_some(), "the record still has its text");
    }

    #[test]
    fn a_double_click_in_a_fullscreen_program_records_the_word_through_the_press_path() {
        let mut t = fullscreen(20, 10);
        paint(&mut t, &["alpha beta", "gamma", "delta", "eps", "zeta", "eta", "theta", "iota", "─", "❯"]);
        let mut clicks = mouse::Clicks::default();
        t.pressed(&mut clicks, 7, 0, Side::Left, 2, true, true);
        assert_eq!(t.selection_text().as_deref(), Some("beta"));
    }

    #[test]
    fn a_synchronized_update_that_times_out_still_feeds_the_record() {
        // A frame held in a synchronized update that never ends: the record follows it when the
        // update times out, as it would at the end of a batch.
        let mut t = fullscreen(20, 8);
        paint(&mut t, &["l10", "l11", "l12", "l13", "l14", "l15", "─", "❯"]);
        drag(&mut t, (0, 1), (2, 2));
        let rows = ["l9", "l10", "l11", "l12", "l13", "l14", "─", "❯"];
        let frame: String = rows.iter().enumerate().map(|(i, text)| format!("\x1b[{};1H{text}\x1b[K", i + 1)).collect();
        t.apply_all([b"\x1b[?2026h".as_slice(), frame.as_bytes()]);
        // A program that leaves the alternate screen inside an update that times out.
        let mut gone = fullscreen(20, 5);
        paint(&mut gone, &["one", "two", "three", "─", "❯"]);
        drag(&mut gone, (0, 0), (2, 1));
        gone.apply_all([b"\x1b[?2026h\x1b[?1000l\x1b[?1049l".as_slice()]);
        assert!(gone.record_open(), "nothing has reached the grid yet");
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(t.flush_sync_if_due());
        assert_eq!(t.selection_spans().first().map(|s| s.row), Some(2), "the tint moved with the text");
        assert_eq!(t.selection_text().as_deref(), Some("l11\nl12"));
        assert!(gone.flush_sync_if_due());
        assert!(!gone.record_open());
    }
}
