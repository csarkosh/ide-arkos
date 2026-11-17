//! Help, Keyboard Navigation (Cyrus, 2026-11-16: "It should explain how the keyboard navigation
//! works and the keybindings. Because I'm struggling to use it right now."): a short page on how
//! to get around with Chrome's keys, written from the keys in force, so a key the config rebinds
//! reads as it is bound. The project window opens it in a viewer tab, rendered; the chooser and
//! Settings in the default app for Markdown.

use crate::keymap::{keys_for, pretty_keys};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "Keyboard Navigation.md";

/// The page, its keys read from `overrides` (the config's `[keybindings]`) over the defaults.
pub fn page(overrides: &BTreeMap<String, String>) -> String {
    let k = |name: &str| keys_for(name, overrides).map(|keys| format!("**{}**", pretty_keys(&keys))).unwrap_or_else(|| "(no key)".to_string());
    format!(
        "# Keyboard navigation

Arkos uses Chrome's keys: a number jumps to a place, Tab steps through tabs, the arrows move
through the session list, and the command palette runs everything else. These get you anywhere:

| Keys | Where it takes you |
| --- | --- |
| {sessions} | the session list |
| {p1} to {p8} | pane 1 to 8 of the session you are on |
| {last} | the last pane |
| {palette} | the command palette: any action, by name |

## Switching sessions

Press {sessions}. The sidebar opens on your sessions, every group unfolded, with a ring on the
session you are on.

1. Press Up or Down to move the ring, or type a few letters of a session's name to narrow the
   list.
2. Press Enter: that session fills the window, and the keys go to it.
3. Changed your mind? Escape clears what you typed; Escape again goes back where you were.

Without opening the list, {next_session} and {prev_session} go to the next and previous session
in the list's order.

## Inside a session

A session is a page of panes, each with its own tabs. Each pane's number is at the left end of
its tab bar.

- {p1} to {p8}: that pane; {last}: the last one.
- {next_ctrl} and {prev_ctrl} (or {next_tab} and {prev_tab}): the next and previous tab of the
  pane you are in.
- {new_tab}: a new tab. Pick Claude, Codex or Shell with Up, Down and Enter.
- {close}: close the tab; {reopen}: bring it back.
- {split}: split the pane in two, side by side.

## Everything else

{palette} opens the command palette. Type part of what you want (\"split down\", \"maximize\",
\"focus file tree\", \"next session that needs you\") and press Enter. Every action is there, with
its key when it has one.

Help, Keybindings lists every action and its keys. To give an action a key, run \"Open config\"
from the palette and set it under `[keybindings]`, for example
`\"focus file tree\" = \"cmd-shift-e\"`.
",
        sessions = k("focus sessions"),
        p1 = k("focus pane 1"),
        p8 = k("focus pane 8"),
        last = k("focus last pane"),
        palette = k("command palette"),
        next_session = k("next session"),
        prev_session = k("previous session"),
        next_ctrl = k("next tab ctrl"),
        prev_ctrl = k("previous tab ctrl"),
        next_tab = k("next tab"),
        prev_tab = k("previous tab"),
        new_tab = k("new tab"),
        close = k("close tab"),
        reopen = k("reopen closed tab"),
        split = k("split right"),
    )
}

/// Writes the page into `dir` (only when its text changed) and returns its path.
pub fn write_page(dir: &Path, overrides: &BTreeMap<String, String>) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(FILE_NAME);
    let text = page(overrides);
    if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
        std::fs::write(&path, text)?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_names_the_default_keys() {
        let p = page(&BTreeMap::new());
        for keys in ["**Cmd+0**", "**Cmd+1** to **Cmd+8**", "**Cmd+9**", "**Shift+Cmd+P**", "**Cmd+T**", "**Ctrl+Tab**", "**Option+Cmd+Down**"] {
            assert!(p.contains(keys), "{keys} in:\n{p}");
        }
        assert!(!p.contains("(no key)"), "every key the page names is bound by default");
    }

    #[test]
    fn a_rebound_key_reads_as_it_is_bound_and_an_unbound_one_says_so() {
        let mut o = BTreeMap::new();
        o.insert("focus sessions".to_string(), "cmd-shift-s".to_string());
        o.insert("new tab".to_string(), String::new());
        let p = page(&o);
        assert!(p.contains("Press **Shift+Cmd+S**."), "{p}");
        assert!(p.contains("- (no key): a new tab."), "{p}");
    }

    #[test]
    fn the_page_is_written_once_and_rewritten_only_when_it_changes() {
        let dir = std::env::temp_dir().join(format!("arkos-keyguide-{}", std::process::id()));
        let path = write_page(&dir, &BTreeMap::new()).unwrap();
        assert_eq!(path.file_name().unwrap(), FILE_NAME);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), page(&BTreeMap::new()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
