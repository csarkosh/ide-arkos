//! One line of editable text with a cursor, for the command palette and the find bar. gpui
//! 0.2.2 has no input element, so this is the smallest hand-built one: the keys a Mac user
//! expects in a search field, nothing more. Pure, so every key is unit tested.
use crate::theme::Theme;
use gpui::{div, prelude::*, px, Div, Keystroke, SharedString};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))] // Used from Task 8.
pub struct LineEdit {
    pub text: String,
    /// In chars, 0 to `text.chars().count()`.
    pub cursor: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))] // Used from Task 8.
pub enum Edit {
    Changed,
    Unchanged,
    Submit,
    SubmitBack,
    Cancel,
    Up,
    Down,
}

#[cfg_attr(not(test), allow(dead_code))] // Used from Task 8.
impl LineEdit {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, text: &str) {
        self.text = text.to_string();
        self.cursor = self.text.chars().count();
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    fn byte_at(&self, chars: usize) -> usize {
        self.text.char_indices().nth(chars).map(|(b, _)| b).unwrap_or(self.text.len())
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// Inserts at the cursor. Newlines and other control characters are dropped: this is one line.
    pub fn insert_str(&mut self, s: &str) {
        let clean: String = s.chars().filter(|c| !c.is_control()).collect();
        if clean.is_empty() {
            return;
        }
        let at = self.byte_at(self.cursor);
        self.text.insert_str(at, &clean);
        self.cursor += clean.chars().count();
    }

    /// Removes the chars in `[from, to)` and puts the cursor at `from`.
    fn remove(&mut self, from: usize, to: usize) -> Edit {
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

    pub fn handle(&mut self, ks: &Keystroke) -> Edit {
        let m = &ks.modifiers;
        let key = ks.key.as_str();
        match (key, m.platform, m.control, m.alt, m.shift) {
            ("enter", false, false, false, false) => return Edit::Submit,
            ("enter", false, false, false, true) => return Edit::SubmitBack,
            ("escape", ..) => return Edit::Cancel,
            ("up", false, false, false, false) => return Edit::Up,
            ("down", false, false, false, false) => return Edit::Down,
            ("left", true, ..) | ("home", ..) => {
                self.cursor = 0;
                return Edit::Unchanged;
            }
            ("right", true, ..) | ("end", ..) => {
                self.cursor = self.len();
                return Edit::Unchanged;
            }
            ("a", false, true, ..) => {
                self.cursor = 0;
                return Edit::Unchanged;
            }
            ("e", false, true, ..) => {
                self.cursor = self.len();
                return Edit::Unchanged;
            }
            ("left", false, ..) => {
                self.cursor = self.cursor.saturating_sub(1);
                return Edit::Unchanged;
            }
            ("right", false, ..) => {
                self.cursor = (self.cursor + 1).min(self.len());
                return Edit::Unchanged;
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

/// The text with a 1 px cursor bar at the cursor, or `placeholder` dimmed when empty.
#[allow(dead_code)] // Used from Task 8; tests do not call it.
pub fn render(edit: &LineEdit, placeholder: &str, theme: &Theme, family: SharedString, size: f32) -> Div {
    let at = edit.byte_at(edit.cursor);
    let (before, after) = edit.text.split_at(at);
    let row = div().flex().flex_row().items_center().font_family(family).text_size(px(size)).text_color(theme.chrome.text.hsla());
    if edit.text.is_empty() {
        return row
            .child(div().w(px(1.0)).h(px(size * 1.2)).bg(theme.chrome.text.hsla()))
            .child(div().text_color(theme.chrome.text_dim.hsla()).child(placeholder.to_string()));
    }
    row.child(before.to_string()).child(div().w(px(1.0)).h(px(size * 1.2)).bg(theme.chrome.text.hsla())).child(after.to_string())
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
