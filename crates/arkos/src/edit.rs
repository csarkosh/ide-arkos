//! A line buffer with one caret for the markdown editor and, later, the code editor: positions,
//! selection, edits with undo, movement by grapheme, word, line and page. Knows nothing about
//! markdown.
//!
//! Positions count Unicode scalar values (`Pos::ch`), never bytes, so a position stays valid
//! whatever the line holds; the conversion to a byte offset happens inside this module. Every
//! function is total: a position past the end is clamped, never indexed raw.
use std::sync::Arc;
use std::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation;

/// Typing within this long of the previous keystroke, at the place it left the caret, is one
/// undo step.
const COALESCE_WINDOW: Duration = Duration::from_millis(700);

/// A place in the document. `ch` counts Unicode scalar values from the line start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Pos {
    pub line: usize,
    pub ch: usize,
}

/// The caret is the head; the selection runs between the anchor and the head. When they are equal
/// nothing is selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Caret {
    pub head: Pos,
    pub anchor: Pos,
    /// The display column Up and Down aim for, so a trip through a short line does not lose the
    /// column the caret started in.
    pub goal_col: Option<usize>,
}

impl Caret {
    /// A caret at `p` with nothing selected.
    pub fn at(p: Pos) -> Caret {
        Caret { head: p, anchor: p, goal_col: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
}

/// How far a delete reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Grapheme,
    Word,
    LineStart,
    LineEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    Up,
    Down,
    DocStart,
    DocEnd,
    PageUp(usize),
    PageDown(usize),
}

/// One edit: the range it replaced (in the text before the edit), the old text, the new text.
type EditRecord = (Pos, Pos, String, String);

/// One undo step. It stores the edits themselves rather than a snapshot of the document, so a
/// paste into a large file costs the size of the paste.
struct Step {
    edits: Vec<EditRecord>,
    caret_before: Caret,
    caret_after: Caret,
    coalescable: bool,
    /// When typing last landed before this step, so taking the step back (`retract`) lets the
    /// next keystroke join the typing before it.
    typed_before: Option<Instant>,
}

pub struct Document {
    /// Shared with the viewer's content, the find and the diff. Every edit goes through
    /// `Arc::make_mut`, which copies the lines only while someone else holds them.
    lines: Arc<Vec<String>>,
    pub ending: LineEnding,
    /// Whether the file started with a byte order mark, restored by `to_text`. The mark is never
    /// part of the first line.
    pub bom: bool,
    /// Whether the file ended with a line break, restored by `to_text`.
    pub trailing_newline: bool,
    undo: Vec<Step>,
    redo: Vec<Step>,
    last_typed: Option<Instant>,
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// The byte offset of char `ch`, or the line length when `ch` is past the end.
fn byte_of(line: &str, ch: usize) -> usize {
    line.char_indices().nth(ch).map(|(b, _)| b).unwrap_or(line.len())
}

/// The char offsets where a grapheme starts, then the line length.
fn boundaries(line: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut ch = 0;
    for g in line.graphemes(true) {
        out.push(ch);
        ch += char_len(g);
    }
    out.push(ch);
    out
}

pub fn grapheme_left(line: &str, ch: usize) -> usize {
    boundaries(line).into_iter().rev().find(|&b| b < ch).unwrap_or(0)
}

pub fn grapheme_right(line: &str, ch: usize) -> usize {
    let b = boundaries(line);
    let len = *b.last().unwrap_or(&0);
    b.into_iter().find(|&b| b > ch).unwrap_or(len)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Where Option+Left lands: the start of the word before `ch`, skipping punctuation and spaces.
pub fn word_boundary_left(line: &str, ch: usize) -> usize {
    let chars: Vec<char> = line.chars().collect();
    let mut i = ch.min(chars.len());
    while i > 0 && !is_word(chars[i - 1]) {
        i -= 1;
    }
    while i > 0 && is_word(chars[i - 1]) {
        i -= 1;
    }
    i
}

/// Where Option+Right lands: the end of the word at or after `ch`, skipping punctuation and spaces.
pub fn word_boundary_right(line: &str, ch: usize) -> usize {
    let chars: Vec<char> = line.chars().collect();
    let mut i = ch.min(chars.len());
    while i < chars.len() && !is_word(chars[i]) {
        i += 1;
    }
    while i < chars.len() && is_word(chars[i]) {
        i += 1;
    }
    i
}

/// The display column of `ch`: graphemes before it, so a combining accent takes no column.
fn col_of(line: &str, ch: usize) -> usize {
    boundaries(line).iter().take_while(|&&b| b < ch).count()
}

/// The char offset of display column `col`, clamped to the line end.
fn ch_at_col(line: &str, col: usize) -> usize {
    let b = boundaries(line);
    b[col.min(b.len() - 1)]
}

/// Where the end of `text` lands when it starts at `from`.
fn end_of(from: Pos, text: &str) -> Pos {
    match text.rsplit_once('\n') {
        None => Pos { line: from.line, ch: from.ch + char_len(text) },
        Some((before, last)) => Pos { line: from.line + before.matches('\n').count() + 1, ch: char_len(last) },
    }
}

/// The file text of a document's lines: what `Document::to_text` gives, for a caller holding only
/// the shared lines (a background job that must not join them on the UI thread).
pub fn join_text(lines: &[String], ending: LineEnding, bom: bool, trailing_newline: bool) -> String {
    let eol = match ending {
        LineEnding::Lf => "\n",
        LineEnding::CrLf => "\r\n",
    };
    let mut out = if bom { String::from("\u{feff}") } else { String::new() };
    out.push_str(&lines.join(eol));
    // An empty last line already ends the text with a break; a typed-into one needs it added.
    if trailing_newline && !lines.last().is_some_and(|l| l.is_empty()) {
        out.push_str(eol);
    }
    out
}

impl Document {
    /// The ending is taken from the first line break; an empty text is one empty line. A leading
    /// byte order mark is set aside as `bom`.
    pub fn from_text(text: &str) -> Document {
        let (bom, text) = match text.strip_prefix('\u{feff}') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let ending = match text.find('\n') {
            Some(i) if text[..i].ends_with('\r') => LineEnding::CrLf,
            _ => LineEnding::Lf,
        };
        let trailing_newline = text.ends_with('\n');
        // A final line break leaves an empty last line, so the caret can sit after it.
        let lines = Arc::new(text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l).to_string()).collect());
        Document { lines, ending, bom, trailing_newline, undo: Vec::new(), redo: Vec::new(), last_typed: None }
    }

    pub fn to_text(&self) -> String {
        join_text(&self.lines, self.ending, self.bom, self.trailing_newline)
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// The lines as the viewer's content holds them: a cheap clone of the shared buffer.
    pub fn shared_lines(&self) -> Arc<Vec<String>> {
        self.lines.clone()
    }

    #[cfg_attr(not(test), allow(dead_code))] // Used from Task 5 on.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn clamp(&self, p: Pos) -> Pos {
        let line = p.line.min(self.lines.len() - 1);
        Pos { line, ch: p.ch.min(char_len(&self.lines[line])) }
    }

    /// The text between two positions, in either order, with `\n` between lines.
    pub fn text_in(&self, from: Pos, to: Pos) -> String {
        let (from, to) = self.ordered(from, to);
        if from.line == to.line {
            let l = &self.lines[from.line];
            return l[byte_of(l, from.ch)..byte_of(l, to.ch)].to_string();
        }
        let first = &self.lines[from.line];
        let last = &self.lines[to.line];
        let mut out = first[byte_of(first, from.ch)..].to_string();
        for l in &self.lines[from.line + 1..to.line] {
            out.push('\n');
            out.push_str(l);
        }
        out.push('\n');
        out.push_str(&last[..byte_of(last, to.ch)]);
        out
    }

    fn ordered(&self, a: Pos, b: Pos) -> (Pos, Pos) {
        let (a, b) = (self.clamp(a), self.clamp(b));
        if a <= b {
            (a, b)
        } else {
            (b, a)
        }
    }

    fn end_pos(&self) -> Pos {
        let line = self.lines.len() - 1;
        Pos { line, ch: char_len(&self.lines[line]) }
    }

    /// Replaces `[from, to)` with `text` and returns the text that was there. The one place the
    /// buffer changes.
    fn splice(&mut self, from: Pos, to: Pos, text: &str) -> String {
        let (from, to) = self.ordered(from, to);
        let old = self.text_in(from, to);
        let head = self.lines[from.line][..byte_of(&self.lines[from.line], from.ch)].to_string();
        let tail = self.lines[to.line][byte_of(&self.lines[to.line], to.ch)..].to_string();
        let mut new: Vec<String> = text.split('\n').map(str::to_string).collect();
        new[0].insert_str(0, &head);
        if let Some(last) = new.last_mut() {
            last.push_str(&tail);
        }
        Arc::make_mut(&mut self.lines).splice(from.line..=to.line, new);
        old
    }

    /// Applies one edit, records its step, clears redo, and returns the caret after it.
    fn commit(&mut self, from: Pos, to: Pos, text: &str, before: Caret, after: Caret, coalescable: bool, now: Instant) -> Caret {
        let (from, to) = self.ordered(from, to);
        let old = self.splice(from, to, text);
        let typed_before = self.last_typed;
        self.undo.push(Step { edits: vec![(from, to, old, text.to_string())], caret_before: before, caret_after: after, coalescable, typed_before });
        self.redo.clear();
        self.last_typed = coalescable.then_some(now);
        after
    }

    /// The selected range in document order, clamped.
    fn selection(&self, caret: Caret) -> (Pos, Pos) {
        self.ordered(caret.head, caret.anchor)
    }

    /// Replaces the selection, if any, with `text`; `\n` splits lines.
    pub fn insert(&mut self, caret: Caret, text: &str, now: Instant) -> Caret {
        let text = text.replace("\r\n", "\n");
        let (from, to) = self.selection(caret);
        let has_selection = from != to;
        if text.is_empty() && !has_selection {
            return Caret::at(self.clamp(caret.head));
        }
        // One keystroke is one grapheme; anything longer or with a line break is a paste or a
        // newline and gets its own step.
        let typed = !has_selection && !text.contains('\n') && text.graphemes(true).count() == 1;
        let extends = typed
            && self.last_typed.is_some_and(|t| now.saturating_duration_since(t) <= COALESCE_WINDOW)
            && self.undo.last().is_some_and(|s| s.coalescable && s.caret_after.head == from);
        if extends {
            self.splice(from, from, &text);
            let end = end_of(from, &text);
            let step = self.undo.last_mut().expect("checked above");
            if let Some(edit) = step.edits.last_mut() {
                edit.3.push_str(&text);
            }
            step.caret_after = Caret::at(end);
            self.redo.clear();
            self.last_typed = Some(now);
            return Caret::at(end);
        }
        self.commit(from, to, &text, caret, Caret::at(end_of(from, &text)), typed, now)
    }

    pub fn delete_backward(&mut self, caret: Caret, unit: Unit, now: Instant) -> Caret {
        let (from, to) = self.selection(caret);
        if from != to {
            return self.commit(from, to, "", caret, Caret::at(from), false, now);
        }
        let head = self.clamp(caret.head);
        let line = &self.lines[head.line];
        let (start, end) = if head.ch == 0 {
            match unit {
                Unit::LineStart | Unit::LineEnd => (head, head),
                _ if head.line == 0 => (head, head),
                _ => (Pos { line: head.line - 1, ch: char_len(&self.lines[head.line - 1]) }, head),
            }
        } else {
            match unit {
                Unit::Grapheme | Unit::LineEnd => (Pos { line: head.line, ch: grapheme_left(line, head.ch) }, head),
                Unit::LineStart => (Pos { line: head.line, ch: 0 }, head),
                Unit::Word => {
                    // The macOS way: the whitespace just before the caret goes, then the run
                    // before it, a run of word characters or, when punctuation precedes, a run of
                    // punctuation. Whitespace in general counts, tabs included.
                    let chars: Vec<char> = line.chars().collect();
                    let mut i = head.ch;
                    while i > 0 && chars[i - 1].is_whitespace() {
                        i -= 1;
                    }
                    if i > 0 {
                        let word = is_word(chars[i - 1]);
                        while i > 0 && !chars[i - 1].is_whitespace() && is_word(chars[i - 1]) == word {
                            i -= 1;
                        }
                    }
                    (Pos { line: head.line, ch: i }, head)
                }
            }
        };
        if start == end {
            return Caret::at(head);
        }
        self.commit(start, end, "", caret, Caret::at(start), false, now);
        Caret::at(start)
    }

    pub fn delete_forward(&mut self, caret: Caret, unit: Unit, now: Instant) -> Caret {
        let (from, to) = self.selection(caret);
        if from != to {
            return self.commit(from, to, "", caret, Caret::at(from), false, now);
        }
        let head = self.clamp(caret.head);
        let line = &self.lines[head.line];
        let len = char_len(line);
        let (start, end) = if head.ch >= len {
            match unit {
                Unit::LineStart | Unit::LineEnd => (head, head),
                _ if head.line + 1 >= self.lines.len() => (head, head),
                _ => (head, Pos { line: head.line + 1, ch: 0 }),
            }
        } else {
            match unit {
                Unit::Grapheme | Unit::LineStart => (head, Pos { line: head.line, ch: grapheme_right(line, head.ch) }),
                Unit::LineEnd => (head, Pos { line: head.line, ch: len }),
                Unit::Word => {
                    let chars: Vec<char> = line.chars().collect();
                    let end = word_boundary_right(line, head.ch);
                    let mut word_start = head.ch;
                    while word_start < end && !is_word(chars[word_start]) {
                        word_start += 1;
                    }
                    if chars[head.ch..word_start].iter().all(|c| c.is_whitespace()) {
                        word_start = head.ch;
                    }
                    if word_start == end {
                        (head, Pos { line: head.line, ch: end })
                    } else {
                        (Pos { line: head.line, ch: word_start }, Pos { line: head.line, ch: end })
                    }
                }
            }
        };
        if start == end {
            return Caret::at(head);
        }
        // The caret stays put, so redo must leave it there too.
        self.commit(start, end, "", caret, Caret::at(head), false, now);
        Caret::at(head)
    }

    /// Replaces a range as its own undo step, for the input method and the task toggle. Returns
    /// the end of the new text.
    pub fn replace_range(&mut self, from: Pos, to: Pos, text: &str, now: Instant) -> Pos {
        let (from, to) = self.ordered(from, to);
        let text = text.replace("\r\n", "\n");
        if from == to && text.is_empty() {
            return from;
        }
        self.commit(from, to, &text, Caret::at(from), Caret::at(end_of(from, &text)), false, now).head
    }

    pub fn undo(&mut self) -> Option<Caret> {
        let step = self.undo.pop()?;
        for (from, _, old, new) in step.edits.iter().rev() {
            self.splice(*from, end_of(*from, new), old);
        }
        let caret = step.caret_before;
        self.redo.push(step);
        self.last_typed = None;
        Some(caret)
    }

    /// Takes the last step back and forgets it: no redo, and the typing before it may continue.
    /// The input method's marked text is written as a step and retracted when the composition
    /// changes or is committed, so a composition leaves one step, the committed text's.
    pub fn retract(&mut self) -> Option<Caret> {
        let step = self.undo.pop()?;
        for (from, _, old, new) in step.edits.iter().rev() {
            self.splice(*from, end_of(*from, new), old);
        }
        self.last_typed = step.typed_before;
        Some(step.caret_before)
    }

    /// The UTF-16 code units from `from` to `p`, a line break counting one: the offsets an input
    /// method speaks in. A `p` before `from` is zero.
    pub fn pos_to_utf16(&self, from: Pos, p: Pos) -> usize {
        let (from, p) = (self.clamp(from), self.clamp(p));
        if p <= from {
            return 0;
        }
        let mut units = 0;
        for line in from.line..=p.line {
            let start = if line == from.line { from.ch } else { 0 };
            let l = &self.lines[line];
            let end = if line == p.line { p.ch } else { char_len(l) };
            units += l.chars().skip(start).take(end.saturating_sub(start)).map(char::len_utf16).sum::<usize>();
            if line != p.line {
                units += 1;
            }
        }
        units
    }

    /// The position `units` UTF-16 code units after `from`, a line break counting one; clamped to
    /// the document's end. An offset inside a surrogate pair rounds back to its char.
    pub fn utf16_to_pos(&self, from: Pos, units: usize) -> Pos {
        let from = self.clamp(from);
        let mut left = units;
        let (mut line, mut ch) = (from.line, from.ch);
        loop {
            for c in self.lines[line].chars().skip(ch) {
                let n = c.len_utf16();
                if left < n {
                    return Pos { line, ch };
                }
                left -= n;
                ch += 1;
            }
            if left == 0 || line + 1 >= self.lines.len() {
                return Pos { line, ch };
            }
            left -= 1;
            line += 1;
            ch = 0;
        }
    }

    pub fn redo(&mut self) -> Option<Caret> {
        let step = self.redo.pop()?;
        for (from, to, _, new) in &step.edits {
            self.splice(*from, *to, new);
        }
        let caret = step.caret_after;
        self.undo.push(step);
        self.last_typed = None;
        Some(caret)
    }

    /// A caret move ends the current typing step: the next keystroke starts a new one.
    pub fn break_coalescing(&mut self) {
        self.last_typed = None;
    }

    /// The caret after a move. `select` keeps the anchor so the move extends the selection;
    /// otherwise the caret collapses. Left and Right with a selection and no `select` collapse to
    /// the selection's edge, as every Mac text field does.
    pub fn move_by(&self, caret: Caret, m: Move, select: bool) -> Caret {
        let head = self.clamp(caret.head);
        let anchor = self.clamp(caret.anchor);
        let line = &self.lines[head.line];
        let (lo, hi) = self.selection(caret);
        if !select && lo != hi {
            match m {
                Move::Left => return Caret::at(lo),
                Move::Right => return Caret::at(hi),
                _ => {}
            }
        }
        let mut goal = None;
        let new = match m {
            Move::Left if head.ch == 0 => self.prev_line_end(head),
            Move::Left => Pos { line: head.line, ch: grapheme_left(line, head.ch) },
            Move::Right if head.ch >= char_len(line) => self.next_line_start(head),
            Move::Right => Pos { line: head.line, ch: grapheme_right(line, head.ch) },
            Move::WordLeft if head.ch == 0 => self.prev_line_end(head),
            Move::WordLeft => Pos { line: head.line, ch: word_boundary_left(line, head.ch) },
            Move::WordRight if head.ch >= char_len(line) => self.next_line_start(head),
            Move::WordRight => Pos { line: head.line, ch: word_boundary_right(line, head.ch) },
            Move::LineStart => Pos { line: head.line, ch: 0 },
            Move::LineEnd => Pos { line: head.line, ch: char_len(line) },
            Move::DocStart => Pos::default(),
            Move::DocEnd => self.end_pos(),
            Move::Up | Move::Down | Move::PageUp(_) | Move::PageDown(_) => {
                let col = caret.goal_col.unwrap_or_else(|| col_of(line, head.ch));
                goal = Some(col);
                let (up, n) = match m {
                    Move::Up => (true, 1),
                    Move::Down => (false, 1),
                    Move::PageUp(n) => (true, n),
                    Move::PageDown(n) => (false, n),
                    _ => unreachable!(),
                };
                let last = self.lines.len() - 1;
                if up && head.line == 0 {
                    // Up on the first line goes to the start, as on the Mac.
                    goal = None;
                    Pos::default()
                } else if !up && head.line == last {
                    goal = None;
                    self.end_pos()
                } else {
                    let target = if up { head.line.saturating_sub(n) } else { (head.line + n).min(last) };
                    Pos { line: target, ch: ch_at_col(&self.lines[target], col) }
                }
            }
        };
        Caret { head: new, anchor: if select { anchor } else { new }, goal_col: goal }
    }

    fn prev_line_end(&self, p: Pos) -> Pos {
        if p.line == 0 {
            p
        } else {
            Pos { line: p.line - 1, ch: char_len(&self.lines[p.line - 1]) }
        }
    }

    fn next_line_start(&self, p: Pos) -> Pos {
        if p.line + 1 >= self.lines.len() {
            p
        } else {
            Pos { line: p.line + 1, ch: 0 }
        }
    }

    pub fn select_all(&self) -> Caret {
        Caret { head: self.end_pos(), anchor: Pos::default(), goal_col: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn doc(s: &str) -> Document { Document::from_text(s) }
    fn at(line: usize, ch: usize) -> Caret { Caret { head: Pos { line, ch }, anchor: Pos { line, ch }, goal_col: None } }
    fn t0() -> Instant { Instant::now() }

    #[test]
    fn line_endings_round_trip() {
        for (text, ending, trailing) in [("a\nb\n", LineEnding::Lf, true), ("a\r\nb", LineEnding::CrLf, false), ("", LineEnding::Lf, false), ("one", LineEnding::Lf, false), ("\n", LineEnding::Lf, true)] {
            let d = doc(text);
            assert_eq!((d.ending, d.trailing_newline), (ending, trailing), "{text:?}");
            assert_eq!(d.to_text(), text, "{text:?}");
        }
        assert_eq!(doc("").lines(), &[String::new()], "an empty document is one empty line");
        assert_eq!(doc("a\tb").lines(), &["a\tb".to_string()], "tabs stay tabs");
    }

    #[test]
    fn inserting_and_deleting_graphemes_and_words() {
        let mut d = doc("héllo wörld");
        let c = d.insert(at(0, 5), ",", t0());
        assert_eq!((d.lines()[0].as_str(), c.head), ("héllo, wörld", Pos { line: 0, ch: 6 }));
        let c = d.delete_backward(at(0, 7), Unit::Word, t0());
        assert_eq!((d.lines()[0].as_str(), c.head.ch), ("héllowörld", 5), "the space and the punctuation run go");
        let c = d.delete_backward(c, Unit::Word, t0());
        assert_eq!((d.lines()[0].as_str(), c.head.ch), ("wörld", 0), "then the word");
        let mut d = doc("a, ");
        let c = d.delete_backward(at(0, 3), Unit::Word, t0());
        assert_eq!((d.lines()[0].as_str(), c.head.ch), ("a", 1));
        let mut d = doc("one, two");
        let c = d.delete_forward(at(0, 3), Unit::Word, t0());
        assert_eq!((d.lines()[0].as_str(), c.head.ch), ("one, ", 3), "the word goes; the caret stays before the punctuation");
        assert_eq!(d.undo().map(|c| c.head.ch), Some(3));
        assert_eq!(d.redo().map(|c| c.head.ch), Some(3), "redo leaves the caret where the delete did");
        let mut d = doc("e\u{301}x"); // e with a combining accent, then x
        let c = d.delete_backward(at(0, 2), Unit::Grapheme, t0());
        assert_eq!((d.lines()[0].as_str(), c.head.ch), ("x", 0), "one grapheme, two scalars");
        let mut d = doc("ab\ncd");
        let c = d.delete_backward(at(1, 0), Unit::Grapheme, t0());
        assert_eq!((d.lines(), c.head), (&["abcd".to_string()][..], Pos { line: 0, ch: 2 }), "backspace at a line start joins");
        let c = d.insert(c, "\n\n", t0());
        assert_eq!((d.lines().len(), c.head), (3, Pos { line: 2, ch: 0 }));
        let c = d.delete_forward(c, Unit::LineEnd, t0());
        assert_eq!((d.lines()[2].as_str(), c.head.ch), ("", 0));
        let c = d.delete_backward(c, Unit::LineStart, t0());
        assert_eq!(c.head, Pos { line: 2, ch: 0 }, "nothing before the caret: no change, no panic");
    }

    #[test]
    fn a_selection_is_replaced_by_what_is_typed() {
        let mut d = doc("abc\ndef");
        let sel = Caret { head: Pos { line: 1, ch: 1 }, anchor: Pos { line: 0, ch: 1 }, goal_col: None };
        assert_eq!(d.text_in(sel.anchor, sel.head), "bc\nd");
        let c = d.insert(sel, "X", t0());
        assert_eq!((d.lines(), c.head), (&["aXef".to_string()][..], Pos { line: 0, ch: 2 }));
        assert_eq!(c.anchor, c.head, "typing collapses the selection");
        assert_eq!(d.select_all(), Caret { head: Pos { line: 0, ch: 4 }, anchor: Pos { line: 0, ch: 0 }, goal_col: None });
    }

    #[test]
    fn typing_coalesces_into_one_undo_step_until_a_pause_newline_or_move() {
        let mut d = doc("");
        let t = t0();
        let mut c = at(0, 0);
        for (i, ch) in ["h", "e", "y"].iter().enumerate() {
            c = d.insert(c, ch, t + Duration::from_millis(100 * i as u64));
        }
        c = d.insert(c, " ", t + Duration::from_millis(1200)); // a pause: a new step
        c = d.insert(c, "x", t + Duration::from_millis(1250));
        assert_eq!(d.lines()[0], "hey x");
        assert_eq!(d.undo().map(|c| c.head.ch), Some(3), "the second step is the space and the x");
        assert_eq!(d.lines()[0], "hey");
        assert_eq!(d.undo().map(|c| c.head.ch), Some(0));
        assert_eq!(d.lines()[0], "");
        assert_eq!(d.undo(), None);
        assert_eq!(d.redo().map(|c| c.head.ch), Some(3));
        assert_eq!(d.redo().map(|c| c.head.ch), Some(5));
        assert_eq!(d.redo(), None);
        let c2 = d.insert(c, "\n", t + Duration::from_millis(1300));
        let _ = d.insert(c2, "a", t + Duration::from_millis(1310));
        assert_eq!(d.lines().len(), 2);
        d.undo();
        assert_eq!(d.lines(), &["hey x".to_string(), String::new()][..], "the newline is its own step, the a another");
        d.break_coalescing();
        let c3 = d.insert(at(1, 0), "b", t + Duration::from_millis(1320));
        let _ = d.insert(c3, "c", t + Duration::from_millis(1330));
        assert_eq!(d.redo(), None, "an edit clears the redo stack");
    }

    #[test]
    fn a_paste_is_one_undo_step() {
        let mut d = doc("x");
        let big: String = (0..10_000).map(|i| format!("line {i}\n")).collect();
        let c = d.insert(at(0, 1), &big, t0());
        assert_eq!(d.line_count(), 10_001);
        assert_eq!(c.head, Pos { line: 10_000, ch: 0 });
        d.undo();
        assert_eq!(d.lines(), &["x".to_string()][..]);
    }

    #[test]
    fn movement_by_word_line_and_document_with_a_goal_column() {
        let d = doc("short\na much longer line here\n");
        let c = d.move_by(at(1, 10), Move::WordRight, false);
        assert_eq!(c.head.ch, 13, "to the end of 'longer'");
        let c = d.move_by(c, Move::WordLeft, false);
        assert_eq!(c.head.ch, 7, "to the start of 'longer'");
        let c = d.move_by(at(1, 10), Move::Up, false);
        assert_eq!((c.head, c.goal_col), (Pos { line: 0, ch: 5 }, Some(10)), "Up clamps to the short line and remembers the goal");
        let c = d.move_by(c, Move::Down, false);
        assert_eq!(c.head, Pos { line: 1, ch: 10 }, "Down comes back to the goal column");
        let c = d.move_by(c, Move::LineEnd, false);
        assert_eq!(c.head.ch, 23);
        let c = d.move_by(c, Move::LineStart, true);
        assert_eq!((c.head.ch, c.anchor.ch), (0, 23), "Shift keeps the anchor");
        let c = d.move_by(at(1, 3), Move::DocStart, false);
        assert_eq!(c.head, Pos { line: 0, ch: 0 });
        let c = d.move_by(c, Move::DocEnd, false);
        assert_eq!(c.head, Pos { line: 2, ch: 0 });
        let c = d.move_by(at(0, 0), Move::Left, false);
        assert_eq!(c.head, Pos { line: 0, ch: 0 }, "the document start stays put");
        let c = d.move_by(at(0, 5), Move::Right, false);
        assert_eq!(c.head, Pos { line: 1, ch: 0 }, "Right at a line end goes to the next line");
        assert_eq!(d.move_by(at(1, 3), Move::PageUp(10), false).head, Pos { line: 0, ch: 3 });
        assert_eq!(d.move_by(at(0, 3), Move::PageDown(10), false).head.line, 2);
    }

    #[test]
    fn a_retracted_step_leaves_no_redo_and_the_typing_before_it_continues() {
        let mut d = doc("");
        let t = t0();
        let c = d.insert(at(0, 0), "a", t);
        // A composition's marked text, written and taken back as the input method changes it.
        let end = d.replace_range(c.head, c.head, "\u{b4}", t + Duration::from_millis(50));
        assert_eq!((d.lines()[0].as_str(), end.ch), ("a\u{b4}", 2));
        assert_eq!(d.retract().map(|c| c.head), Some(Pos { line: 0, ch: 1 }));
        assert_eq!(d.lines()[0], "a");
        assert_eq!(d.redo(), None, "a retracted step is not redone");
        let _ = d.insert(c, "\u{e9}", t + Duration::from_millis(100));
        assert_eq!(d.lines()[0], "a\u{e9}");
        d.undo();
        assert_eq!(d.lines()[0], "", "the committed text joined the typing before it");
    }

    #[test]
    fn replace_range_is_its_own_undoable_step() {
        let mut d = doc("- [ ] task");
        let end = d.replace_range(Pos { line: 0, ch: 3 }, Pos { line: 0, ch: 4 }, "x", t0());
        assert_eq!((d.lines()[0].as_str(), end), ("- [x] task", Pos { line: 0, ch: 4 }));
        d.undo();
        assert_eq!(d.lines()[0], "- [ ] task");
    }
}
