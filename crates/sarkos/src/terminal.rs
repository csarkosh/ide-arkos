//! The terminal state behind one tab: an alacritty grid fed by the daemon, plus scroll state.
//! `TerminalCore` has no GPUI dependency so its behaviour is unit tested; `Terminal` wraps it as
//! a GPUI entity and owns the socket side.
use crate::client::Client;
use crate::keys::keystroke_to_bytes;
use crate::layout::{CellKind, CellStyle};
use crate::palette::{Palette, NAVY};
use crate::scroll::ScrollAnimator;
use alacritty_terminal::event::{Event as TermEvent, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Boundary, Column, Direction, Line, Point};
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
use std::time::Instant;

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
    // Not read in milestone 1; kept for milestone 2's per-tab scroll position.
    #[allow(dead_code)]
    pub display_offset: usize,
    /// The line just above the viewport, present when scrolled into history.
    pub above: Option<Vec<(char, CellStyle, CellKind)>>,
    pub visible: Vec<Vec<(char, CellStyle, CellKind)>>,
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

pub struct TerminalCore {
    term: Term<Listener>,
    parser: Processor,
    listener: Listener,
    palette: Arc<Palette>,
    pub scroll: ScrollAnimator,
}

impl TerminalCore {
    pub fn new(cols: usize, rows: usize, scrollback: usize) -> Self {
        let listener = Listener::default();
        let config = Config { scrolling_history: scrollback, ..Config::default() };
        let term = Term::new(config, &TermSize::new(cols, rows), listener.clone());
        Self { term, parser: Processor::new(), listener, palette: Arc::new(NAVY), scroll: ScrollAnimator::new() }
    }

    pub fn apply_output(&mut self, bytes: &[u8]) -> Wakeup {
        let before = self.term.grid().display_offset();
        self.parser.advance(&mut self.term, bytes);
        self.flush_sync_if_due();
        // Alacritty raises display_offset as lines arrive while scrolled up; keep the animator
        // in step so the next frame does not pull the view down.
        let after = self.term.grid().display_offset();
        if after != before {
            let d = after as f32 - before as f32;
            self.scroll.current += d;
            self.scroll.target += d;
        }
        let mut w = Wakeup { repaint: !bytes.is_empty(), ..Default::default() };
        let events: Vec<TermEvent> = self.listener.events.lock().unwrap().drain(..).collect();
        for ev in events {
            match ev {
                TermEvent::Title(t) => w.title = Some(t),
                TermEvent::ResetTitle => w.title = Some(String::new()),
                TermEvent::Bell => w.bell = true,
                TermEvent::PtyWrite(s) => w.pty_write.extend_from_slice(s.as_bytes()),
                _ => {}
            }
        }
        self.scroll.clamp_max(self.history_size() as f32);
        w
    }

    /// Swaps the colours every cell resolves against at paint. Nothing is re-parsed.
    pub fn set_palette(&mut self, palette: Arc<Palette>) {
        self.palette = palette;
    }

    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// Applies chunks in order and merges their wakeups into one.
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
        out
    }

    /// Ends a synchronized update whose deadline has passed. Returns true when it flushed.
    pub fn flush_sync_if_due(&mut self) -> bool {
        match self.parser.sync_timeout().sync_timeout() {
            Some(deadline) if deadline <= Instant::now() => {
                self.parser.stop_sync(&mut self.term);
                true
            }
            _ => false,
        }
    }

    /// True while a synchronized update is open and its rows are held back.
    pub fn sync_pending(&self) -> bool {
        self.parser.sync_timeout().sync_timeout().is_some()
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.term.resize(TermSize::new(cols.max(2), rows.max(1)));
        self.scroll.clamp_max(self.history_size() as f32);
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

    pub fn rows_for_paint(&self) -> RowsForPaint {
        let off = self.display_offset() as i32;
        let screen = self.term.screen_lines() as i32;
        let visible = (0..screen).map(|i| self.row_cells(Line(i - off))).collect();
        let above = if off < self.history_size() as i32 { Some(self.row_cells(Line(-off - 1))) } else { None };
        RowsForPaint { display_offset: off as usize, above, visible }
    }
}

/// Drops every control character except tab, newline and carriage return (this removes ESC and
/// the C1 range), so pasted text can neither end the bracketed paste nor inject escape sequences.
pub(crate) fn sanitize_paste(text: &str) -> String {
    text.chars().filter(|&c| c == '\t' || c == '\n' || c == '\r' || !c.is_control()).collect()
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
        }
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

    pub fn handle_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) {
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
        if let Some(bytes) = keystroke_to_bytes(ks, self.core.app_cursor()) {
            self.client.send(Request::Input { session: self.session, bytes });
            // Typing jumps back to the live bottom, as every terminal does.
            self.core.scroll.jump_to(0.0);
            self.core.set_display_offset(0);
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
        self.client.send(Request::Input { session: self.session, bytes });
    }

    /// Positive lines scroll up into history.
    pub fn scroll_lines(&mut self, lines: f32, cx: &mut Context<Self>) {
        self.core.scroll.scroll_by(lines, self.core.history_size() as f32);
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
    fn resize_reflows_and_reports_modes() {
        let mut t = TerminalCore::new(20, 5, 100);
        t.apply_output(b"\x1b[?1h\x1b[?2004h");
        assert!(t.app_cursor());
        assert!(t.bracketed_paste());
        t.resize(40, 10);
        assert_eq!(t.rows_for_paint().visible.len(), 10);
        assert_eq!(t.rows_for_paint().visible[0].len(), 40);
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
}
