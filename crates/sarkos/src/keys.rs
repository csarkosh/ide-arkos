//! Maps a GPUI keystroke to the bytes a terminal expects (xterm conventions).
// Used from Task 9 onward.
#![allow(dead_code)]
use gpui::{Keystroke, Modifiers};

pub fn keystroke_to_bytes(k: &Keystroke, app_cursor: bool) -> Option<Vec<u8>> {
    let m = &k.modifiers;
    if m.platform {
        return None;
    }
    let key = k.key.as_str();
    let arrow = |csi: &str, ss3: &str| -> Vec<u8> {
        if app_cursor { format!("\x1bO{ss3}").into_bytes() } else { format!("\x1b[{csi}").into_bytes() }
    };
    let bytes = match key {
        "enter" => b"\r".to_vec(),
        "backspace" => vec![0x7f],
        "tab" if m.shift => b"\x1b[Z".to_vec(),
        "tab" => b"\t".to_vec(),
        "escape" => vec![0x1b],
        "delete" => b"\x1b[3~".to_vec(),
        "space" if m.control => vec![0x00],
        "space" => b" ".to_vec(),
        "up" => arrow("A", "A"),
        "down" => arrow("B", "B"),
        "right" if m.alt => b"\x1bf".to_vec(),
        "left" if m.alt => b"\x1bb".to_vec(),
        "right" => arrow("C", "C"),
        "left" => arrow("D", "D"),
        "home" => b"\x1b[H".to_vec(),
        "end" => b"\x1b[F".to_vec(),
        "pageup" => b"\x1b[5~".to_vec(),
        "pagedown" => b"\x1b[6~".to_vec(),
        _ => {
            if m.control {
                if key.chars().count() != 1 {
                    return None;
                }
                let c = key.chars().next()?;
                let code = match c {
                    'a'..='z' => (c as u8) - b'a' + 1,
                    '[' => 0x1b,
                    '\\' => 0x1c,
                    ']' => 0x1d,
                    '^' | '6' => 0x1e,
                    '_' | '-' => 0x1f,
                    '?' => 0x7f,
                    _ => return None,
                };
                return Some(vec![code]);
            }
            if let Some(ch) = &k.key_char {
                if !ch.is_empty() {
                    return Some(ch.as_bytes().to_vec());
                }
            }
            if m.alt && key.chars().count() == 1 {
                let mut v = vec![0x1b];
                v.extend_from_slice(key.as_bytes());
                return Some(v);
            }
            if key.chars().count() == 1 {
                return Some(key.as_bytes().to_vec());
            }
            return None;
        }
    };
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ks(key: &str, key_char: Option<&str>, m: Modifiers) -> Keystroke {
        Keystroke { modifiers: m, key: key.to_string(), key_char: key_char.map(|s| s.to_string()) }
    }
    fn none() -> Modifiers { Modifiers::default() }
    fn ctrl() -> Modifiers { Modifiers { control: true, ..Modifiers::default() } }
    fn alt() -> Modifiers { Modifiers { alt: true, ..Modifiers::default() } }
    fn shift() -> Modifiers { Modifiers { shift: true, ..Modifiers::default() } }
    fn cmd() -> Modifiers { Modifiers { platform: true, ..Modifiers::default() } }

    #[test]
    fn printable_uses_key_char() {
        assert_eq!(keystroke_to_bytes(&ks("a", Some("a"), none()), false), Some(b"a".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("a", Some("A"), shift()), false), Some(b"A".to_vec()));
    }

    #[test]
    fn option_s_sends_eszett() {
        assert_eq!(keystroke_to_bytes(&ks("s", Some("ß"), alt()), false), Some("ß".as_bytes().to_vec()));
    }

    #[test]
    fn control_letters_are_control_codes() {
        assert_eq!(keystroke_to_bytes(&ks("c", None, ctrl()), false), Some(vec![0x03]));
        assert_eq!(keystroke_to_bytes(&ks("d", None, ctrl()), false), Some(vec![0x04]));
        assert_eq!(keystroke_to_bytes(&ks("[", None, ctrl()), false), Some(vec![0x1b]));
    }

    #[test]
    fn editing_keys() {
        assert_eq!(keystroke_to_bytes(&ks("enter", None, none()), false), Some(b"\r".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("backspace", None, none()), false), Some(vec![0x7f]));
        assert_eq!(keystroke_to_bytes(&ks("tab", None, none()), false), Some(b"\t".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("tab", None, shift()), false), Some(b"\x1b[Z".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("escape", None, none()), false), Some(vec![0x1b]));
        assert_eq!(keystroke_to_bytes(&ks("delete", None, none()), false), Some(b"\x1b[3~".to_vec()));
    }

    #[test]
    fn arrows_follow_cursor_mode() {
        assert_eq!(keystroke_to_bytes(&ks("up", None, none()), false), Some(b"\x1b[A".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("up", None, none()), true), Some(b"\x1bOA".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("left", None, alt()), false), Some(b"\x1bb".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("right", None, alt()), false), Some(b"\x1bf".to_vec()));
    }

    #[test]
    fn navigation_keys() {
        assert_eq!(keystroke_to_bytes(&ks("home", None, none()), false), Some(b"\x1b[H".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("end", None, none()), false), Some(b"\x1b[F".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("pageup", None, none()), false), Some(b"\x1b[5~".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("pagedown", None, none()), false), Some(b"\x1b[6~".to_vec()));
    }

    #[test]
    fn command_combos_are_not_terminal_input() {
        assert_eq!(keystroke_to_bytes(&ks("v", None, cmd()), false), None);
        assert_eq!(keystroke_to_bytes(&ks("q", None, cmd()), false), None);
    }

    #[test]
    fn ctrl_with_named_key_is_not_a_control_byte() {
        assert_eq!(keystroke_to_bytes(&ks("f1", None, ctrl()), false), None);
        assert_eq!(keystroke_to_bytes(&ks("insert", None, ctrl()), false), None);
    }

    #[test]
    fn option_with_plain_letter_sends_meta() {
        // GPUI reports an alt keystroke with no key_char when the layout yields nothing special.
        assert_eq!(keystroke_to_bytes(&ks("x", None, alt()), false), Some(b"\x1bx".to_vec()));
    }
}
