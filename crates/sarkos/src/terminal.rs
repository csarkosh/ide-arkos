//! The terminal state behind one tab: an alacritty grid fed by the daemon, plus scroll state.
//! `TerminalCore` has no GPUI dependency so its behaviour is unit tested; `Terminal` wraps it as
//! a GPUI entity and owns the socket side.
// Not constructed until Task 10 wires it into the window.
#![allow(dead_code)]
use crate::client::Client;
use crate::keys::keystroke_to_bytes;
use crate::layout::{CellKind, CellStyle};
use crate::palette::{Palette, NAVY};
use crate::scroll::ScrollAnimator;
use alacritty_terminal::event::{Event as TermEvent, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::Line;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{CursorShape, Processor};
use gpui::{Context, FocusHandle, Keystroke};
use protocol::{Event, Request, SessionId};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

/// Collects alacritty events during one `advance` so they can be reported after it.
#[derive(Clone, Default)]
struct Listener {
    events: Rc<RefCell<VecDeque<TermEvent>>>,
}

impl EventListener for Listener {
    fn send_event(&self, event: TermEvent) {
        self.events.borrow_mut().push_back(event);
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
}

pub struct TerminalCore {
    term: Term<Listener>,
    parser: Processor,
    listener: Listener,
    palette: &'static Palette,
    pub scroll: ScrollAnimator,
}

impl TerminalCore {
    pub fn new(cols: usize, rows: usize, scrollback: usize) -> Self {
        let listener = Listener::default();
        let config = Config { scrolling_history: scrollback, ..Config::default() };
        let term = Term::new(config, &TermSize::new(cols, rows), listener.clone());
        Self { term, parser: Processor::new(), listener, palette: &NAVY, scroll: ScrollAnimator::new() }
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
        for ev in self.listener.events.borrow_mut().drain(..) {
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

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.term.resize(TermSize::new(cols.max(2), rows.max(1)));
        self.scroll.clamp_max(self.history_size() as f32);
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

/// Strips the bracketed-paste end marker so pasted text cannot escape the bracket.
pub(crate) fn sanitize_paste(text: &str) -> String {
    text.replace("\x1b[201~", "")
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
    pub cell: Option<CellMetrics>,
    pub bell_pending: bool,
    /// When the element last painted, for time-based scroll animation.
    pub last_frame: Option<Instant>,
}

impl Terminal {
    pub fn new(core: TerminalCore, session: SessionId, client: Arc<Client>, cx: &mut Context<Self>) -> Self {
        Self { core, session, client, focus: cx.focus_handle(), title: String::new(), alive: true, cell: None, bell_pending: false, last_frame: None }
    }

    /// Applies one daemon event. Returns true when a repaint is needed.
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
                Event::Exited { session, .. } if *session == self.session => {
                    self.flush_output(&mut pending);
                    self.alive = false;
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
        true
    }

    pub fn handle_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) {
        // Shift+PageUp/PageDown scroll locally; everything else goes to the shell.
        if ks.modifiers.shift && (ks.key == "pageup" || ks.key == "pagedown") {
            let page = self.core.screen_lines() as f32;
            self.scroll_lines(if ks.key == "pageup" { page } else { -page }, cx);
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
        assert!(!t.flush_sync_if_due());
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(t.flush_sync_if_due());
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
    fn paste_end_marker_is_stripped() {
        assert_eq!(sanitize_paste("a\x1b[201~b"), "ab");
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
}
