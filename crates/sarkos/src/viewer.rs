//! A read-only viewer tab (spec 6): the file loaded once off the UI thread, classified (text,
//! too big to highlight, binary), split into lines and highlighted; a find over the lines; a
//! scroll position in lines; reloaded when the poll sees the file change.

use crate::line_edit::LineEdit;
use crate::palette::Rgb;
use crate::scroll::ScrollAnimator;
use gpui::{Context, FocusHandle};
use std::path::PathBuf;
use std::time::Instant;

#[cfg_attr(not(test), expect(dead_code))] // Task 5's viewer tab uses it; until then only the tests do.
/// Bytes a binary check looks at: a NUL in the first 8 KiB means not text.
pub const SNIFF: usize = 8 * 1024;
#[cfg_attr(not(test), expect(dead_code))] // Task 5's viewer tab uses it; until then only the tests do.
pub const MAX_HITS: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// Byte range within the line.
    pub start: usize,
    pub end: usize,
    pub color: Rgb,
    pub bold: bool,
    pub italic: bool,
}

#[allow(dead_code)] // Task 5 opens viewer tabs with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    Text { lines: Vec<String>, spans: Vec<Vec<Span>> },
    TooBig(u64),
    Binary,
    Unreadable(String),
}

#[cfg_attr(not(test), expect(dead_code))] // Task 5's viewer tab uses it; until then only the tests do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classified {
    Text(String),
    TooBig,
    Binary,
}

#[cfg_attr(not(test), expect(dead_code))] // Task 5's viewer tab uses it; until then only the tests do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

#[cfg_attr(not(test), expect(dead_code))] // Task 5's viewer tab uses it; until then only the tests do.
pub fn classify(bytes: &[u8], limit: usize) -> Classified {
    if bytes[..bytes.len().min(SNIFF)].contains(&0) {
        return Classified::Binary;
    }
    if bytes.len() > limit {
        return Classified::TooBig;
    }
    Classified::Text(String::from_utf8_lossy(bytes).into_owned())
}

#[cfg_attr(not(test), expect(dead_code))] // Task 5's viewer tab uses it; until then only the tests do.
/// Lines without their terminators; a trailing newline adds no empty last line.
pub fn split_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    text.lines().map(str::to_string).collect()
}

#[cfg_attr(not(test), expect(dead_code))] // Task 5's viewer tab uses it; until then only the tests do.
/// Every case-insensitive occurrence, in reading order, as byte ranges, at most `MAX_HITS`.
pub fn find_hits(lines: &[String], needle: &str) -> Vec<Hit> {
    let mut out = Vec::new();
    if needle.is_empty() {
        return out;
    }
    let n: Vec<char> = needle.to_lowercase().chars().collect();
    for (i, line) in lines.iter().enumerate() {
        // Lowercasing can change byte lengths (e.g. some scripts): search char by char instead.
        let chars: Vec<(usize, char)> = line.char_indices().collect();
        let mut c = 0;
        while c + n.len() <= chars.len() {
            let matches = (0..n.len()).all(|k| chars[c + k].1.to_lowercase().eq(n[k].to_lowercase()));
            if matches {
                let start = chars[c].0;
                let end = chars.get(c + n.len()).map(|(b, _)| *b).unwrap_or(line.len());
                out.push(Hit { line: i, start, end });
                if out.len() >= MAX_HITS {
                    return out;
                }
                c += n.len();
            } else {
                c += 1;
            }
        }
    }
    out
}

#[cfg_attr(not(test), expect(dead_code))] // Task 5's viewer tab uses it; until then only the tests do.
/// The next hit index, wrapping.
pub fn step(current: usize, len: usize, delta: isize) -> usize {
    crate::keymap::tab_cycle(current, len, delta)
}

#[allow(dead_code)] // Task 5 opens viewer tabs with it.
pub struct ViewerFind {
    pub query: LineEdit,
    pub hits: Vec<Hit>,
    pub current: usize,
    pub focus: FocusHandle,
}

#[allow(dead_code)] // Task 5 opens viewer tabs with it.
/// What the background load produced, applied on the UI thread.
pub struct Loaded {
    pub content: Content,
    pub fingerprint: Option<(u128, u64)>,
}

#[allow(dead_code)] // Task 5 opens viewer tabs with it.
pub struct Viewer {
    pub path: PathBuf,
    pub content: Content,
    /// `false` until the first load lands.
    pub loaded: bool,
    pub scroll: ScrollAnimator,
    pub focus: FocusHandle,
    pub find: Option<ViewerFind>,
    pub font_size: f32,
    pub line_height: f32,
    pub last_frame: Option<Instant>,
    /// Rows the element showed at its last paint, for page keys and the scroll limit.
    pub rows_visible: usize,
    pub fingerprint: Option<(u128, u64)>,
}

#[allow(dead_code)] // Task 5 opens viewer tabs with it.
impl Viewer {
    pub fn new(path: PathBuf, font_size: f32, line_height: f32, cx: &mut Context<Self>) -> Self {
        Viewer {
            path,
            content: Content::Text { lines: Vec::new(), spans: Vec::new() },
            loaded: false,
            scroll: ScrollAnimator::new(),
            focus: cx.focus_handle(),
            find: None,
            font_size,
            line_height,
            last_frame: None,
            rows_visible: 1,
            fingerprint: None,
        }
    }

    pub fn line_count(&self) -> usize {
        match &self.content {
            Content::Text { lines, .. } => lines.len(),
            _ => 0,
        }
    }

    /// The furthest the view can scroll: the last line at the top of the viewport.
    pub fn max_scroll(&self) -> f32 {
        self.line_count().saturating_sub(1) as f32
    }

    pub fn apply(&mut self, loaded: Loaded, cx: &mut Context<Self>) {
        self.content = loaded.content;
        self.fingerprint = loaded.fingerprint;
        self.loaded = true;
        self.scroll.clamp_max(self.max_scroll());
        if let Some(f) = &mut self.find {
            f.hits = match &self.content {
                Content::Text { lines, .. } => find_hits(lines, &f.query.text),
                _ => Vec::new(),
            };
            f.current = f.current.min(f.hits.len().saturating_sub(1));
        }
        cx.notify();
    }

    pub fn scroll_lines(&mut self, lines: f32, cx: &mut Context<Self>) {
        self.scroll.scroll_by(lines, self.max_scroll());
        cx.notify();
    }

    pub fn open_find(&mut self, cx: &mut Context<Self>) {
        if self.find.is_none() {
            self.find = Some(ViewerFind { query: LineEdit::new(), hits: Vec::new(), current: 0, focus: cx.focus_handle() });
        }
        cx.notify();
    }

    pub fn close_find(&mut self, cx: &mut Context<Self>) {
        self.find = None;
        cx.notify();
    }

    /// Re-runs the search after the query changed and jumps to the first hit.
    pub fn refind(&mut self, cx: &mut Context<Self>) {
        let Some(f) = &mut self.find else { return };
        f.hits = match &self.content {
            Content::Text { lines, .. } => find_hits(lines, &f.query.text),
            _ => Vec::new(),
        };
        f.current = 0;
        self.show_current_hit();
        cx.notify();
    }

    pub fn find_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(f) = &mut self.find {
            f.current = step(f.current, f.hits.len(), delta);
        }
        self.show_current_hit();
        cx.notify();
    }

    /// Scrolls so the current hit's line is in view, a third of the way down.
    fn show_current_hit(&mut self) {
        let Some(f) = &self.find else { return };
        let Some(h) = f.hits.get(f.current) else { return };
        let target = (h.line as f32 - (self.rows_visible as f32 / 3.0).floor()).max(0.0);
        self.scroll.target = target.min(self.max_scroll());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_text_too_big_and_binary() {
        assert_eq!(classify(b"hello\nworld\n", 100), Classified::Text("hello\nworld\n".into()));
        assert_eq!(classify(b"hello\nworld\n", 5), Classified::TooBig, "over the limit: no highlighting, plain lines");
        assert_eq!(classify(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR", 100), Classified::Binary);
        let mut big = vec![b'a'; SNIFF + 10];
        big[SNIFF + 5] = 0;
        assert!(matches!(classify(&big, 1 << 20), Classified::Text(_)), "a NUL past the sniff window is not checked");
        assert!(matches!(classify(&[0xff, 0xfe, b'a'], 100), Classified::Text(_)), "invalid UTF-8 is shown lossily, not called binary");
        assert_eq!(classify(b"", 0), Classified::Text(String::new()), "an empty file is text under any limit");
    }

    #[test]
    fn lines_split_on_lf_and_crlf_without_a_phantom_last_line() {
        assert_eq!(split_lines("a\nb\n"), ["a", "b"]);
        assert_eq!(split_lines("a\r\nb"), ["a", "b"]);
        assert_eq!(split_lines(""), Vec::<String>::new());
        assert_eq!(split_lines("\n"), [""]);
        assert_eq!(split_lines("one"), ["one"]);
    }

    #[test]
    fn find_is_case_insensitive_in_order_and_capped() {
        let lines: Vec<String> = ["Hello hello", "nothing", "HELLO"].iter().map(|s| s.to_string()).collect();
        let hits = find_hits(&lines, "hello");
        assert_eq!(hits, [Hit { line: 0, start: 0, end: 5 }, Hit { line: 0, start: 6, end: 11 }, Hit { line: 2, start: 0, end: 5 }]);
        assert!(find_hits(&lines, "").is_empty());
        let many: Vec<String> = vec!["aa".to_string(); MAX_HITS];
        assert_eq!(find_hits(&many, "a").len(), MAX_HITS, "capped");
        let unicode = vec!["caf\u{e9} CAF\u{c9}".to_string()];
        let h = find_hits(&unicode, "caf\u{e9}");
        assert_eq!(h.len(), 2);
        assert_eq!(&unicode[0][h[1].start..h[1].end], "CAF\u{c9}", "byte ranges land on char boundaries");
    }

    #[test]
    fn a_hit_is_stepped_through_with_wraparound() {
        assert_eq!(step(0, 3, 1), 1);
        assert_eq!(step(2, 3, 1), 0);
        assert_eq!(step(0, 3, -1), 2);
        assert_eq!(step(0, 0, 1), 0);
    }
}
