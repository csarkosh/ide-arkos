//! Maps a GPUI keystroke to the bytes a terminal expects (xterm conventions), with the kitty
//! keyboard protocol's "disambiguate escape codes" level when the program has asked for it.
use gpui::Keystroke;

/// What the program and the config have set; decides how a keystroke is encoded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyMode {
    /// DECCKM: arrows as SS3 (`ESC O A`) instead of CSI.
    pub app_cursor: bool,
    /// The program set the kitty keyboard protocol's "disambiguate escape codes" flag: Esc and
    /// the modified keys whose legacy bytes are ambiguous go as `CSI code ; mods u`.
    pub kitty: bool,
    /// Option sends ESC plus the key, Terminal.app's "Use Option as Meta Key"; off, Option
    /// composes characters (Option+S types ß).
    pub option_as_meta: bool,
}

fn csi_u(code: u32, mods: u8) -> Option<Vec<u8>> {
    Some(if mods > 1 { format!("\x1b[{code};{mods}u") } else { format!("\x1b[{code}u") }.into_bytes())
}

pub fn keystroke_to_bytes(k: &Keystroke, mode: KeyMode) -> Option<Vec<u8>> {
    let m = &k.modifiers;
    if m.platform {
        return None;
    }
    let key = k.key.as_str();
    let mods = 1 + m.shift as u8 + 2 * m.alt as u8 + 4 * m.control as u8;
    // Option+Left/Right alone keep the shell word-jump sequences from milestone 1.
    if m.alt && !m.shift && !m.control {
        match key {
            "left" => return Some(b"\x1bb".to_vec()),
            "right" => return Some(b"\x1bf".to_vec()),
            _ => {}
        }
    }
    if mods > 1 {
        let suffix = match key {
            "up" => Some('A'),
            "down" => Some('B'),
            "right" => Some('C'),
            "left" => Some('D'),
            "home" => Some('H'),
            "end" => Some('F'),
            _ => None,
        };
        if let Some(c) = suffix {
            return Some(format!("\x1b[1;{mods}{c}").into_bytes());
        }
    }
    if mode.kitty {
        // The keys whose legacy bytes a program cannot tell apart: Esc from an escape sequence,
        // a modified Enter, Tab, Backspace or Space from the plain key. Shift+Space is text and
        // stays a space. Ctrl+letter keeps its control byte, which every program reads either
        // way (so Ctrl+I, Ctrl+M and Ctrl+[ stay Tab, Enter and Esc, as in a legacy terminal).
        let code = match key {
            "escape" => Some(27),
            "enter" if mods > 1 => Some(13),
            "tab" if mods > 1 => Some(9),
            "backspace" if mods > 1 => Some(127),
            "space" if m.control || (m.alt && mode.option_as_meta) => Some(32),
            _ => None,
        };
        if let Some(code) = code {
            return csi_u(code, mods);
        }
        // A character with Option as Meta, or with Ctrl and Shift, has no legacy byte at all.
        // Option composing a character is text, handled below.
        if key.chars().count() == 1 && ((m.alt && mode.option_as_meta) || (m.control && m.shift)) {
            return csi_u(key.chars().next()? as u32, mods);
        }
    }
    let arrow = |csi: &str, ss3: &str| -> Vec<u8> {
        if mode.app_cursor { format!("\x1bO{ss3}").into_bytes() } else { format!("\x1b[{csi}").into_bytes() }
    };
    let bytes = match key {
        // Option+Enter is Meta+Return, the newline in Claude Code and readline.
        "enter" if m.alt && mode.option_as_meta => vec![0x1b, b'\r'],
        "enter" => b"\r".to_vec(),
        // Option+Delete is readline's backward-kill-word (ESC DEL), as in Terminal.app.
        "backspace" if m.alt => vec![0x1b, 0x7f],
        "backspace" => vec![0x7f],
        "tab" if m.shift => b"\x1b[Z".to_vec(),
        "tab" => b"\t".to_vec(),
        "escape" => vec![0x1b],
        "delete" if mods > 1 => format!("\x1b[3;{mods}~").into_bytes(),
        "delete" => b"\x1b[3~".to_vec(),
        "space" if m.control => vec![0x00],
        "space" => b" ".to_vec(),
        "up" => arrow("A", "A"),
        "down" => arrow("B", "B"),
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
                // Ctrl+Option+key: Meta on the control byte, as xterm's metaSendsEscape.
                if m.alt && mode.option_as_meta {
                    return Some(vec![0x1b, code]);
                }
                return Some(vec![code]);
            }
            // Option as Meta: ESC plus the key the layout gives without Option; Shift keeps
            // its case, so Option+Shift+P is ESC P.
            if m.alt && mode.option_as_meta && key.chars().count() == 1 {
                let c = key.chars().next()?;
                let mut v = vec![0x1b];
                if m.shift {
                    v.extend(c.to_uppercase().to_string().as_bytes());
                } else {
                    v.extend(c.to_string().as_bytes());
                }
                return Some(v);
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
    use gpui::Modifiers;

    fn ks(key: &str, key_char: Option<&str>, m: Modifiers) -> Keystroke {
        Keystroke { modifiers: m, key: key.to_string(), key_char: key_char.map(|s| s.to_string()) }
    }
    fn none() -> Modifiers { Modifiers::default() }
    fn ctrl() -> Modifiers { Modifiers { control: true, ..Modifiers::default() } }
    fn alt() -> Modifiers { Modifiers { alt: true, ..Modifiers::default() } }
    fn shift() -> Modifiers { Modifiers { shift: true, ..Modifiers::default() } }
    fn cmd() -> Modifiers { Modifiers { platform: true, ..Modifiers::default() } }
    fn alt_shift() -> Modifiers { Modifiers { alt: true, shift: true, ..Modifiers::default() } }
    fn ctrl_shift() -> Modifiers { Modifiers { control: true, shift: true, ..Modifiers::default() } }

    /// The default: Option is Meta, no kitty protocol, normal cursor keys.
    fn meta() -> KeyMode { KeyMode { option_as_meta: true, ..KeyMode::default() } }
    fn app() -> KeyMode { KeyMode { app_cursor: true, ..meta() } }
    fn compose() -> KeyMode { KeyMode { option_as_meta: false, ..KeyMode::default() } }
    fn kitty() -> KeyMode { KeyMode { kitty: true, ..meta() } }

    #[test]
    fn printable_uses_key_char() {
        assert_eq!(keystroke_to_bytes(&ks("a", Some("a"), none()), meta()), Some(b"a".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("a", Some("A"), shift()), meta()), Some(b"A".to_vec()));
    }

    #[test]
    fn option_as_meta_sends_escape_plus_the_key() {
        // The layout would compose π and ∏; Meta sends the key the finger is on.
        assert_eq!(keystroke_to_bytes(&ks("p", Some("π"), alt()), meta()), Some(b"\x1bp".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("p", Some("∏"), alt_shift()), meta()), Some(b"\x1bP".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("enter", None, alt()), meta()), Some(vec![0x1b, b'\r']), "Option+Enter is Meta+Return");
        // GPUI reports an alt keystroke with no key_char when the layout yields nothing special.
        assert_eq!(keystroke_to_bytes(&ks("x", None, alt()), meta()), Some(b"\x1bx".to_vec()));
    }

    #[test]
    fn option_composes_characters_when_meta_is_off() {
        assert_eq!(keystroke_to_bytes(&ks("s", Some("ß"), alt()), compose()), Some("ß".as_bytes().to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("enter", None, alt()), compose()), Some(b"\r".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("x", None, alt()), compose()), Some(b"\x1bx".to_vec()));
        // The word keys do not depend on the setting.
        assert_eq!(keystroke_to_bytes(&ks("left", None, alt()), compose()), Some(b"\x1bb".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("backspace", None, alt()), compose()), Some(vec![0x1b, 0x7f]));
    }

    #[test]
    fn kitty_disambiguates_escape_and_the_modified_editing_keys() {
        assert_eq!(keystroke_to_bytes(&ks("escape", None, none()), kitty()), Some(b"\x1b[27u".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("enter", None, shift()), kitty()), Some(b"\x1b[13;2u".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("enter", None, alt()), kitty()), Some(b"\x1b[13;3u".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("enter", None, ctrl()), kitty()), Some(b"\x1b[13;5u".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("tab", None, shift()), kitty()), Some(b"\x1b[9;2u".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("backspace", None, ctrl()), kitty()), Some(b"\x1b[127;5u".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("space", None, ctrl()), kitty()), Some(b"\x1b[32;5u".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("backspace", None, alt()), kitty()), Some(b"\x1b[127;3u".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("p", Some("π"), alt()), kitty()), Some(b"\x1b[112;3u".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("a", None, ctrl_shift()), kitty()), Some(b"\x1b[97;6u".to_vec()));
        // Shift+Space is text, so it stays a space: Shift is often still held after a capital.
        assert_eq!(keystroke_to_bytes(&ks("space", Some(" "), shift()), kitty()), Some(b" ".to_vec()));
        // With Option composing, Option+S is still the character, kitty or not.
        let compose_kitty = KeyMode { kitty: true, ..compose() };
        assert_eq!(keystroke_to_bytes(&ks("s", Some("ß"), alt()), compose_kitty), Some("ß".as_bytes().to_vec()));
        // The plain keys and Ctrl+letter keep their legacy bytes.
        assert_eq!(keystroke_to_bytes(&ks("enter", None, none()), kitty()), Some(b"\r".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("tab", None, none()), kitty()), Some(b"\t".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("backspace", None, none()), kitty()), Some(vec![0x7f]));
        assert_eq!(keystroke_to_bytes(&ks("a", Some("a"), none()), kitty()), Some(b"a".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("c", None, ctrl()), kitty()), Some(vec![0x03]));
        assert_eq!(keystroke_to_bytes(&ks("up", None, none()), kitty()), Some(b"\x1b[A".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("up", None, alt()), kitty()), Some(b"\x1b[1;3A".to_vec()));
    }

    #[test]
    fn without_kitty_shift_enter_is_a_plain_return() {
        assert_eq!(keystroke_to_bytes(&ks("enter", None, shift()), meta()), Some(b"\r".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("escape", None, none()), meta()), Some(vec![0x1b]));
    }

    #[test]
    fn control_letters_are_control_codes() {
        assert_eq!(keystroke_to_bytes(&ks("c", None, ctrl()), meta()), Some(vec![0x03]));
        assert_eq!(keystroke_to_bytes(&ks("d", None, ctrl()), meta()), Some(vec![0x04]));
        assert_eq!(keystroke_to_bytes(&ks("[", None, ctrl()), meta()), Some(vec![0x1b]));
        // Ctrl+Option+C is Meta on the control byte; composing drops the Meta.
        let ctrl_alt = Modifiers { control: true, alt: true, ..Modifiers::default() };
        assert_eq!(keystroke_to_bytes(&ks("c", None, ctrl_alt), meta()), Some(vec![0x1b, 0x03]));
        assert_eq!(keystroke_to_bytes(&ks("c", None, ctrl_alt), compose()), Some(vec![0x03]));
    }

    #[test]
    fn editing_keys() {
        assert_eq!(keystroke_to_bytes(&ks("enter", None, none()), meta()), Some(b"\r".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("backspace", None, none()), meta()), Some(vec![0x7f]));
        assert_eq!(keystroke_to_bytes(&ks("tab", None, none()), meta()), Some(b"\t".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("tab", None, shift()), meta()), Some(b"\x1b[Z".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("escape", None, none()), meta()), Some(vec![0x1b]));
        assert_eq!(keystroke_to_bytes(&ks("delete", None, none()), meta()), Some(b"\x1b[3~".to_vec()));
    }

    #[test]
    fn option_delete_kills_the_word_before_the_cursor() {
        // Option+Delete (the backspace key) is readline's backward-kill-word: ESC DEL.
        assert_eq!(keystroke_to_bytes(&ks("backspace", None, alt()), meta()), Some(vec![0x1b, 0x7f]));
        // Option+forward-delete, and any other modified forward delete, is xterm's ESC [ 3 ; m ~.
        assert_eq!(keystroke_to_bytes(&ks("delete", None, alt()), meta()), Some(b"\x1b[3;3~".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("delete", None, shift()), meta()), Some(b"\x1b[3;2~".to_vec()));
    }

    #[test]
    fn arrows_follow_cursor_mode() {
        assert_eq!(keystroke_to_bytes(&ks("up", None, none()), meta()), Some(b"\x1b[A".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("up", None, none()), app()), Some(b"\x1bOA".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("left", None, alt()), meta()), Some(b"\x1bb".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("right", None, alt()), meta()), Some(b"\x1bf".to_vec()));
    }

    #[test]
    fn modified_arrows_use_xterm_parameters() {
        assert_eq!(keystroke_to_bytes(&ks("up", None, shift()), meta()), Some(b"\x1b[1;2A".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("left", None, ctrl()), meta()), Some(b"\x1b[1;5D".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("up", None, alt()), meta()), Some(b"\x1b[1;3A".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("right", None, alt_shift()), meta()), Some(b"\x1b[1;4C".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("end", None, shift()), meta()), Some(b"\x1b[1;2F".to_vec()));
        // Modifiers win over application cursor mode, as in xterm.
        assert_eq!(keystroke_to_bytes(&ks("up", None, shift()), app()), Some(b"\x1b[1;2A".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("up", None, none()), app()), Some(b"\x1bOA".to_vec()));
    }

    #[test]
    fn navigation_keys() {
        assert_eq!(keystroke_to_bytes(&ks("home", None, none()), meta()), Some(b"\x1b[H".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("end", None, none()), meta()), Some(b"\x1b[F".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("pageup", None, none()), meta()), Some(b"\x1b[5~".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("pagedown", None, none()), meta()), Some(b"\x1b[6~".to_vec()));
    }

    #[test]
    fn command_combos_are_not_terminal_input() {
        assert_eq!(keystroke_to_bytes(&ks("v", None, cmd()), meta()), None);
        assert_eq!(keystroke_to_bytes(&ks("q", None, cmd()), meta()), None);
    }

    #[test]
    fn ctrl_with_named_key_is_not_a_control_byte() {
        assert_eq!(keystroke_to_bytes(&ks("f1", None, ctrl()), meta()), None);
        assert_eq!(keystroke_to_bytes(&ks("insert", None, ctrl()), meta()), None);
    }
}
