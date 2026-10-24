//! One line of editable text with a cursor and a selection, for the command palette, the find
//! bar, the prompts and the web tab's address field. gpui 0.2.2 has no input element, so this is
//! the smallest hand-built one: the keys a Mac user expects in a text field (a selection with
//! Shift and the arrows, Cmd+A, typing over a selection), nothing more. Pure, so every key is
//! unit tested; the clipboard is the caller's.
use crate::theme::Theme;
use gpui::{div, prelude::*, px, Div, Keystroke, SharedString};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineEdit {
    pub text: String,
    /// In chars, 0 to `text.chars().count()`.
    pub cursor: usize,
    /// The selection's other end, in chars, while one is held; the cursor is the end that moves.
    pub anchor: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    Changed,
    Unchanged,
    Submit,
    SubmitBack,
    Cancel,
    Up,
    Down,
}

impl LineEdit {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, text: &str) {
        self.text = text.to_string();
        self.cursor = self.text.chars().count();
        self.anchor = None;
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.anchor = None;
    }

    /// Selects the whole text, the cursor at its end (Cmd+A, a click into an address field).
    pub fn select_all(&mut self) {
        self.cursor = self.len();
        self.anchor = (self.cursor > 0).then_some(0);
    }

    /// A click at char `at`: the cursor goes there and the selection goes, or, with `extend` (a
    /// drag, or Shift held), the selection runs from where it started to there.
    pub fn place(&mut self, at: usize, extend: bool) {
        let at = at.min(self.len());
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = at;
    }

    /// The char index of a byte offset into `text` (a shaped line's hit test answers in bytes),
    /// rounded down to the char it falls in.
    pub fn char_at_byte(&self, byte: usize) -> usize {
        self.text.char_indices().take_while(|(b, _)| *b < byte).count()
    }

    /// The selection as `[from, to)` in chars, when it is not empty.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        (a != self.cursor).then(|| (a.min(self.cursor), a.max(self.cursor)))
    }

    /// The selected text, empty without a selection.
    pub fn selected_text(&self) -> String {
        match self.selection() {
            Some((from, to)) => self.text.chars().skip(from).take(to - from).collect(),
            None => String::new(),
        }
    }

    /// Removes the selection, if any; true when there was one.
    pub fn delete_selection(&mut self) -> bool {
        match self.selection() {
            Some((from, to)) => {
                self.remove(from, to);
                true
            }
            None => {
                self.anchor = None;
                false
            }
        }
    }

    fn byte_at(&self, chars: usize) -> usize {
        self.text.char_indices().nth(chars).map(|(b, _)| b).unwrap_or(self.text.len())
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// Inserts at the cursor, over the selection when there is one. Newlines and other control
    /// characters are dropped: this is one line.
    pub fn insert_str(&mut self, s: &str) {
        let clean: String = s.chars().filter(|c| !c.is_control()).collect();
        if clean.is_empty() {
            return;
        }
        self.delete_selection();
        let at = self.byte_at(self.cursor);
        self.text.insert_str(at, &clean);
        self.cursor += clean.chars().count();
    }

    /// Removes the chars in `[from, to)` and puts the cursor at `from`.
    fn remove(&mut self, from: usize, to: usize) -> Edit {
        self.anchor = None;
        if from >= to {
            return Edit::Unchanged;
        }
        let (a, b) = (self.byte_at(from), self.byte_at(to));
        self.text.replace_range(a..b, "");
        self.cursor = from;
        Edit::Changed
    }

    /// The char index where the word before the cursor starts: skip spaces, then non-spaces.
    fn word_start(&self) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let mut i = self.cursor;
        while i > 0 && chars[i - 1] == ' ' {
            i -= 1;
        }
        while i > 0 && chars[i - 1] != ' ' {
            i -= 1;
        }
        i
    }

    /// Moves the cursor to `to`: with Shift the selection grows from where it was (or starts at
    /// the old cursor), without it the selection goes.
    fn go(&mut self, to: usize, shift: bool) -> Edit {
        if shift {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = to.min(self.len());
        Edit::Unchanged
    }

    pub fn handle(&mut self, ks: &Keystroke) -> Edit {
        let m = &ks.modifiers;
        let key = ks.key.as_str();
        match (key, m.platform, m.control, m.alt, m.shift) {
            ("enter", false, false, false, false) => return Edit::Submit,
            ("enter", false, false, false, true) => return Edit::SubmitBack,
            ("escape", ..) => return Edit::Cancel,
            ("up", false, false, false, false) => return Edit::Up,
            ("down", false, false, false, false) => return Edit::Down,
            ("a", true, false, false, false) => {
                self.select_all();
                return Edit::Unchanged;
            }
            ("left", true, ..) | ("home", ..) => return self.go(0, m.shift),
            ("right", true, ..) | ("end", ..) => return self.go(self.len(), m.shift),
            ("a", false, true, ..) => return self.go(0, m.shift),
            ("e", false, true, ..) => return self.go(self.len(), m.shift),
            ("left", false, _, _, false) => {
                // A selection collapses to its left end, as in every Mac field.
                let to = self.selection().map_or(self.cursor.saturating_sub(1), |(from, _)| from);
                return self.go(to, false);
            }
            ("right", false, _, _, false) => {
                let to = self.selection().map_or((self.cursor + 1).min(self.len()), |(_, to)| to);
                return self.go(to, false);
            }
            ("left", false, _, _, true) => return self.go(self.cursor.saturating_sub(1), true),
            ("right", false, _, _, true) => return self.go((self.cursor + 1).min(self.len()), true),
            ("backspace", ..) | ("delete", ..) if self.selection().is_some() => {
                self.delete_selection();
                return Edit::Changed;
            }
            ("backspace", true, ..) => return self.remove(0, self.cursor),
            ("backspace", false, _, true, _) => return self.remove(self.word_start(), self.cursor),
            ("backspace", ..) => return self.remove(self.cursor.saturating_sub(1), self.cursor),
            ("delete", ..) => return self.remove(self.cursor, (self.cursor + 1).min(self.len())),
            _ => {}
        }
        if m.platform || m.control {
            return Edit::Unchanged;
        }
        match ks.key_char.as_deref() {
            Some(ch) if !ch.is_empty() && !ch.chars().any(char::is_control) => {
                self.insert_str(ch);
                Edit::Changed
            }
            _ => Edit::Unchanged,
        }
    }
}

/// The text with a 1 px cursor bar at the cursor and the selection highlighted, or
/// `placeholder` dimmed when empty.
pub fn render(edit: &LineEdit, placeholder: &str, theme: &Theme, family: SharedString, size: f32) -> Div {
    let text = theme.chrome.text.hsla();
    let bar = || div().w(px(1.0)).h(px(size * 1.2)).bg(text);
    let row = div().flex().flex_row().items_center().font_family(family).text_size(px(size)).text_color(text);
    if edit.text.is_empty() {
        return row.child(bar()).child(div().text_color(theme.chrome.text_dim.hsla()).child(placeholder.to_string()));
    }
    let (from, to) = edit.selection().unwrap_or((edit.cursor, edit.cursor));
    let (a, b) = (edit.byte_at(from), edit.byte_at(to));
    let (before, selected, after) = (&edit.text[..a], &edit.text[a..b], &edit.text[b..]);
    let mut row = row.child(before.to_string());
    if edit.cursor == from {
        row = row.child(bar());
    }
    if !selected.is_empty() {
        row = row.child(div().bg(theme.chrome.focus_ring.hsla().opacity(0.35)).child(selected.to_string()));
    }
    if edit.cursor == to && from != to {
        row = row.child(bar());
    }
    row.child(after.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Keystroke, Modifiers};

    fn ks(key: &str, key_char: Option<&str>, m: Modifiers) -> Keystroke {
        Keystroke { modifiers: m, key: key.to_string(), key_char: key_char.map(|s| s.to_string()) }
    }
    fn none() -> Modifiers { Modifiers::default() }
    fn cmd() -> Modifiers { Modifiers { platform: true, ..Modifiers::default() } }
    fn alt() -> Modifiers { Modifiers { alt: true, ..Modifiers::default() } }
    fn ctrl() -> Modifiers { Modifiers { control: true, ..Modifiers::default() } }
    fn shift() -> Modifiers { Modifiers { shift: true, ..Modifiers::default() } }

    fn typed(e: &mut LineEdit, s: &str) {
        for c in s.chars() {
            let k = c.to_string();
            assert_eq!(e.handle(&ks(&k, Some(&k), none())), Edit::Changed);
        }
    }

    #[test]
    fn a_selection_grows_with_shift_collapses_with_an_arrow_and_is_typed_over() {
        let mut e = LineEdit::new();
        typed(&mut e, "localhost:4173");
        assert_eq!(e.handle(&ks("a", None, cmd())), Edit::Unchanged);
        assert_eq!(e.selection(), Some((0, 14)));
        assert_eq!(e.selected_text(), "localhost:4173");
        assert_eq!(e.handle(&ks("left", None, none())), Edit::Unchanged);
        assert_eq!((e.cursor, e.selection()), (0, None), "an arrow collapses to the selection's end on that side");
        assert_eq!(e.handle(&ks("right", None, shift())), Edit::Unchanged);
        assert_eq!(e.handle(&ks("right", None, shift())), Edit::Unchanged);
        assert_eq!(e.selected_text(), "lo");
        assert_eq!(e.handle(&ks("right", None, Modifiers { platform: true, shift: true, ..Modifiers::default() })), Edit::Unchanged);
        assert_eq!(e.selected_text(), "localhost:4173", "Cmd+Shift+Right extends to the end");
        assert_eq!(e.handle(&ks("x", Some("x"), none())), Edit::Changed);
        assert_eq!((e.text.as_str(), e.cursor, e.anchor), ("x", 1, None), "typing replaces the selection");
        typed(&mut e, "yz");
        e.select_all();
        assert_eq!(e.handle(&ks("backspace", None, none())), Edit::Changed);
        assert_eq!(e.text, "", "backspace removes the selection");
        typed(&mut e, "ab");
        assert_eq!(e.handle(&ks("left", None, shift())), Edit::Unchanged);
        assert_eq!(e.handle(&ks("end", None, none())), Edit::Unchanged);
        assert_eq!(e.selection(), None, "a plain move drops the selection");
        e.set("set");
        assert_eq!(e.selection(), None, "set clears it too");
        e.select_all();
        e.insert_str("v");
        assert_eq!(e.text, "v", "a paste replaces the selection");
    }

    #[test]
    fn a_click_places_the_cursor_and_drops_the_selection_and_a_drag_extends_it() {
        let mut e = LineEdit::new();
        e.set("https://example.com");
        e.select_all();
        e.place(8, false);
        assert_eq!((e.cursor, e.selection()), (8, None), "a click in a selected address deselects it");
        e.place(15, true);
        assert_eq!(e.selected_text(), "example", "a drag from there selects to the pointer");
        e.place(3, true);
        assert_eq!(e.selected_text(), "ps://", "back past where it started, the other way");
        e.place(99, false);
        assert_eq!(e.cursor, 19, "past the end is the end");
        let w = LineEdit { text: "héllo".into(), cursor: 0, anchor: None };
        assert_eq!((w.char_at_byte(0), w.char_at_byte(1), w.char_at_byte(3), w.char_at_byte(6)), (0, 1, 2, 5), "bytes to chars across a two-byte é");
    }

    #[test]
    fn typing_inserts_at_the_cursor() {
        let mut e = LineEdit::new();
        typed(&mut e, "héllo");
        assert_eq!((e.text.as_str(), e.cursor), ("héllo", 5));
        assert_eq!(e.handle(&ks("left", None, none())), Edit::Unchanged);
        assert_eq!(e.handle(&ks("left", None, none())), Edit::Unchanged);
        typed(&mut e, "X");
        assert_eq!((e.text.as_str(), e.cursor), ("hélXlo", 4));
        assert_eq!(e.handle(&ks("right", None, none())), Edit::Unchanged);
        assert_eq!(e.handle(&ks("right", None, none())), Edit::Unchanged);
        assert_eq!(e.handle(&ks("right", None, none())), Edit::Unchanged, "past the end stays put");
        assert_eq!(e.cursor, 6);
    }

    #[test]
    fn backspace_and_delete() {
        let mut e = LineEdit::new();
        typed(&mut e, "abc");
        assert_eq!(e.handle(&ks("backspace", None, none())), Edit::Changed);
        assert_eq!(e.text, "ab");
        e.handle(&ks("left", None, none()));
        e.handle(&ks("left", None, none()));
        assert_eq!(e.handle(&ks("backspace", None, none())), Edit::Unchanged, "nothing before the cursor");
        assert_eq!(e.handle(&ks("delete", None, none())), Edit::Changed);
        assert_eq!((e.text.as_str(), e.cursor), ("b", 0));
        assert_eq!(e.handle(&ks("delete", None, none())), Edit::Changed);
        assert_eq!(e.handle(&ks("delete", None, none())), Edit::Unchanged);
        assert_eq!(e.text, "");
    }

    #[test]
    fn word_and_line_deletes_and_jumps() {
        let mut e = LineEdit::new();
        typed(&mut e, "one two  three");
        assert_eq!(e.handle(&ks("backspace", None, alt())), Edit::Changed);
        assert_eq!(e.text, "one two  ");
        assert_eq!(e.handle(&ks("backspace", None, alt())), Edit::Changed);
        assert_eq!(e.text, "one ");
        assert_eq!(e.handle(&ks("left", None, cmd())), Edit::Unchanged);
        assert_eq!(e.cursor, 0);
        assert_eq!(e.handle(&ks("right", None, cmd())), Edit::Unchanged);
        assert_eq!(e.cursor, 4);
        assert_eq!(e.handle(&ks("a", None, ctrl())), Edit::Unchanged);
        assert_eq!(e.cursor, 0);
        assert_eq!(e.handle(&ks("e", None, ctrl())), Edit::Unchanged);
        assert_eq!(e.cursor, 4);
        assert_eq!(e.handle(&ks("backspace", None, cmd())), Edit::Changed);
        assert_eq!(e.text, "");
        assert_eq!(e.handle(&ks("backspace", None, cmd())), Edit::Unchanged);
    }

    #[test]
    fn submit_cancel_and_arrows_are_reported_not_typed() {
        let mut e = LineEdit::new();
        typed(&mut e, "q");
        assert_eq!(e.handle(&ks("enter", None, none())), Edit::Submit);
        assert_eq!(e.handle(&ks("enter", None, shift())), Edit::SubmitBack);
        assert_eq!(e.handle(&ks("escape", None, none())), Edit::Cancel);
        assert_eq!(e.handle(&ks("up", None, none())), Edit::Up);
        assert_eq!(e.handle(&ks("down", None, none())), Edit::Down);
        assert_eq!(e.handle(&ks("p", None, cmd())), Edit::Unchanged, "command combos are not text");
        assert_eq!(e.handle(&ks("tab", None, none())), Edit::Unchanged);
        assert_eq!(e.handle(&ks("f1", None, none())), Edit::Unchanged);
        assert_eq!(e.text, "q");
    }

    #[test]
    fn set_clear_and_insert() {
        let mut e = LineEdit::new();
        e.set("abc");
        assert_eq!((e.text.as_str(), e.cursor), ("abc", 3));
        e.handle(&ks("left", None, none()));
        e.insert_str("12");
        assert_eq!((e.text.as_str(), e.cursor), ("ab12c", 4));
        e.insert_str("x\ny");
        assert_eq!(e.text, "ab12xyc", "a pasted newline is dropped");
        e.clear();
        assert_eq!((e.text.as_str(), e.cursor), ("", 0));
    }
}
