//! The keyboard map (spec 4): VS Code's macOS defaults for the editor area, applied to panes
//! and terminal tabs. Every binding has a name, which is its key in the config file's
//! `[keybindings]` table; `bindings_for` applies the overrides from there.

use crate::tree::Side;
use crate::Root;
use gpui::prelude::*;
use gpui::{actions, Context, Div, KeyBinding, Keystroke, Window};
use std::collections::BTreeMap;

actions!(
    sarkos,
    [
        FocusPane1, FocusPane2, FocusPane3, FocusPane4, FocusPane5, FocusPane6, FocusPane7, FocusPane8, FocusPane9,
        FocusLeft, FocusRight, FocusUp, FocusDown,
        SplitRight, SplitDown, ToggleMaximize,
        MoveTabToPrevPane, MoveTabToNextPane,
        NextTab, PrevTab, NextTabAlt, PrevTabAlt, MruTab,
        CloseTab, ReopenClosedTab, CloseAllInPane,
        MoveTabLeft, MoveTabRight,
        NewTerminal, ClearScrollback, FontBigger, FontSmaller,
        OpenConfig, Quit
    ]
);

/// What a key does, in the window's terms.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    FocusPane(usize),
    FocusDir(Side),
    Split(Side),
    ToggleMaximize,
    /// Move the active tab to the previous (-1) or next (+1) pane in reading order.
    MoveTabToPane(isize),
    CycleTab(isize),
    MruTab,
    CloseTab,
    ReopenClosedTab,
    CloseAllInPane,
    MoveTabWithin(isize),
    NewTerminal,
    ClearScrollback,
    FontDelta(f32),
    OpenConfig,
}

/// One row of the table: the config name, the default keystrokes (gpui syntax, chords space
/// separated) and the key context the binding needs.
pub struct ActionSpec {
    pub name: &'static str,
    pub keys: &'static str,
    pub context: Option<&'static str>,
}

const fn row(name: &'static str, keys: &'static str) -> ActionSpec {
    ActionSpec { name, keys, context: None }
}

/// The spec's table (section 4) plus open config and quit. The order is the registration order.
pub const ACTIONS: &[ActionSpec] = &[
    // First on purpose: gpui's bindings_for_input (keymap.rs:191-197) drops a pending chord
    // whose index is after the best exact match, so a later lone cmd-k would fire at once and
    // the Cmd+K chords would never complete. Terminal-only, so chords work elsewhere too.
    ActionSpec { name: "clear scrollback", keys: "cmd-k", context: Some("Terminal") },
    row("focus pane 1", "cmd-1"), row("focus pane 2", "cmd-2"), row("focus pane 3", "cmd-3"),
    row("focus pane 4", "cmd-4"), row("focus pane 5", "cmd-5"), row("focus pane 6", "cmd-6"),
    row("focus pane 7", "cmd-7"), row("focus pane 8", "cmd-8"), row("focus pane 9", "cmd-9"),
    row("focus left", "cmd-k cmd-left"), row("focus right", "cmd-k cmd-right"),
    row("focus up", "cmd-k cmd-up"), row("focus down", "cmd-k cmd-down"),
    row("split right", "cmd-\\"), row("split down", "cmd-k cmd-\\"),
    row("maximize pane", "cmd-k cmd-m"),
    row("move tab to previous pane", "ctrl-cmd-left"), row("move tab to next pane", "ctrl-cmd-right"),
    row("next tab", "alt-cmd-right"), row("previous tab", "alt-cmd-left"),
    row("next tab alt", "cmd-shift-]"), row("previous tab alt", "cmd-shift-["),
    row("most recently used tab", "ctrl-tab"),
    row("close tab", "cmd-w"), row("reopen closed tab", "cmd-shift-t"), row("close all in pane", "cmd-k w"),
    row("move tab left", "cmd-k cmd-shift-left"), row("move tab right", "cmd-k cmd-shift-right"),
    row("new terminal tab", "ctrl-shift-`"),
    row("font bigger", "cmd-="), row("font smaller", "cmd--"),
    row("open config", "cmd-,"),
    row("quit", "cmd-q"),
];

/// The binding for a named action. `keys` must already be validated: `KeyBinding::new` panics
/// on a keystroke it cannot parse.
fn binding(name: &str, keys: &str, context: Option<&str>) -> Option<KeyBinding> {
    Some(match name {
        "clear scrollback" => KeyBinding::new(keys, ClearScrollback, context),
        "focus pane 1" => KeyBinding::new(keys, FocusPane1, context),
        "focus pane 2" => KeyBinding::new(keys, FocusPane2, context),
        "focus pane 3" => KeyBinding::new(keys, FocusPane3, context),
        "focus pane 4" => KeyBinding::new(keys, FocusPane4, context),
        "focus pane 5" => KeyBinding::new(keys, FocusPane5, context),
        "focus pane 6" => KeyBinding::new(keys, FocusPane6, context),
        "focus pane 7" => KeyBinding::new(keys, FocusPane7, context),
        "focus pane 8" => KeyBinding::new(keys, FocusPane8, context),
        "focus pane 9" => KeyBinding::new(keys, FocusPane9, context),
        "focus left" => KeyBinding::new(keys, FocusLeft, context),
        "focus right" => KeyBinding::new(keys, FocusRight, context),
        "focus up" => KeyBinding::new(keys, FocusUp, context),
        "focus down" => KeyBinding::new(keys, FocusDown, context),
        "split right" => KeyBinding::new(keys, SplitRight, context),
        "split down" => KeyBinding::new(keys, SplitDown, context),
        "maximize pane" => KeyBinding::new(keys, ToggleMaximize, context),
        "move tab to previous pane" => KeyBinding::new(keys, MoveTabToPrevPane, context),
        "move tab to next pane" => KeyBinding::new(keys, MoveTabToNextPane, context),
        "next tab" => KeyBinding::new(keys, NextTab, context),
        "previous tab" => KeyBinding::new(keys, PrevTab, context),
        "next tab alt" => KeyBinding::new(keys, NextTabAlt, context),
        "previous tab alt" => KeyBinding::new(keys, PrevTabAlt, context),
        "most recently used tab" => KeyBinding::new(keys, MruTab, context),
        "close tab" => KeyBinding::new(keys, CloseTab, context),
        "reopen closed tab" => KeyBinding::new(keys, ReopenClosedTab, context),
        "close all in pane" => KeyBinding::new(keys, CloseAllInPane, context),
        "move tab left" => KeyBinding::new(keys, MoveTabLeft, context),
        "move tab right" => KeyBinding::new(keys, MoveTabRight, context),
        "new terminal tab" => KeyBinding::new(keys, NewTerminal, context),
        "font bigger" => KeyBinding::new(keys, FontBigger, context),
        "font smaller" => KeyBinding::new(keys, FontSmaller, context),
        "open config" => KeyBinding::new(keys, OpenConfig, context),
        "quit" => KeyBinding::new(keys, Quit, context),
        _ => return None,
    })
}

/// Whether `keys` is a keystroke or chord gpui can parse. Empty means "unbound" and is fine.
fn keystroke_error(keys: &str) -> Option<String> {
    for token in keys.split_whitespace() {
        match Keystroke::parse(token) {
            Err(e) => return Some(format!("cannot parse keystroke \"{token}\": {e}")),
            // gpui 0.2.2 splits only on '-', so "cmd+\\" parses as a single odd key instead of
            // failing. No named key contains '+', so a longer key that does is a wrong separator.
            Ok(k) if k.key.len() > 1 && k.key.contains('+') => {
                return Some(format!("cannot parse keystroke \"{token}\": join modifiers with '-', not '+'"));
            }
            Ok(_) => {}
        }
    }
    None
}

/// Every problem with an override table, in table order: an action name that is not in
/// `ACTIONS`, or keystrokes gpui cannot parse. Empty when the table is fine.
#[allow(dead_code)] // Task 2 validates the config with it
pub fn validate(overrides: &BTreeMap<String, String>) -> Vec<String> {
    let mut errors = Vec::new();
    for (name, keys) in overrides {
        if !ACTIONS.iter().any(|a| a.name == name) {
            errors.push(format!("unknown action \"{name}\" in [keybindings]"));
            continue;
        }
        if let Some(e) = keystroke_error(keys) {
            errors.push(format!("\"{name}\": {e}"));
        }
    }
    errors
}

/// The effective keystrokes for an action: the override if there is one, else the default.
/// `None` for an unknown action or one overridden to the empty string.
pub fn keys_for(name: &str, overrides: &BTreeMap<String, String>) -> Option<String> {
    let spec = ACTIONS.iter().find(|a| a.name == name)?;
    let keys = overrides.get(name).map(String::as_str).unwrap_or(spec.keys);
    if keys.trim().is_empty() {
        None
    } else {
        Some(keys.to_string())
    }
}

/// The defaults with `overrides` applied, in `ACTIONS` order. Entries `validate` would reject
/// keep their default keys; callers show `validate`'s messages and keep the last good bindings.
pub fn bindings_for(overrides: &BTreeMap<String, String>) -> Vec<KeyBinding> {
    ACTIONS
        .iter()
        .filter_map(|spec| {
            let keys = keys_for(spec.name, overrides)?;
            // A bad override keeps the default bound, so a typo never leaves an action keyless.
            let keys = if keystroke_error(&keys).is_some() { spec.keys.to_string() } else { keys };
            binding(spec.name, &keys, spec.context)
        })
        .collect()
}

pub fn bindings() -> Vec<KeyBinding> {
    bindings_for(&BTreeMap::new())
}

pub fn tab_cycle(active: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let n = len as isize;
    (((active as isize + delta) % n + n) % n) as usize
}

macro_rules! route {
    ($div:expr, $cx:expr, $($action:ty => $command:expr),* $(,)?) => {{
        let mut d = $div;
        $( d = d.on_action($cx.listener(|root: &mut Root, _: &$action, window: &mut Window, cx: &mut Context<Root>| root.run($command, window, cx))); )*
        d
    }};
}

/// Registers every action on the window's root element. Quit is handled by the app, not here.
pub fn attach(div: Div, cx: &mut Context<Root>) -> Div {
    route!(div, cx,
        FocusPane1 => Command::FocusPane(0), FocusPane2 => Command::FocusPane(1), FocusPane3 => Command::FocusPane(2),
        FocusPane4 => Command::FocusPane(3), FocusPane5 => Command::FocusPane(4), FocusPane6 => Command::FocusPane(5),
        FocusPane7 => Command::FocusPane(6), FocusPane8 => Command::FocusPane(7), FocusPane9 => Command::FocusPane(8),
        FocusLeft => Command::FocusDir(Side::Left), FocusRight => Command::FocusDir(Side::Right),
        FocusUp => Command::FocusDir(Side::Top), FocusDown => Command::FocusDir(Side::Bottom),
        SplitRight => Command::Split(Side::Right), SplitDown => Command::Split(Side::Bottom),
        ToggleMaximize => Command::ToggleMaximize,
        MoveTabToPrevPane => Command::MoveTabToPane(-1), MoveTabToNextPane => Command::MoveTabToPane(1),
        NextTab => Command::CycleTab(1), PrevTab => Command::CycleTab(-1),
        NextTabAlt => Command::CycleTab(1), PrevTabAlt => Command::CycleTab(-1),
        MruTab => Command::MruTab,
        CloseTab => Command::CloseTab, ReopenClosedTab => Command::ReopenClosedTab, CloseAllInPane => Command::CloseAllInPane,
        MoveTabLeft => Command::MoveTabWithin(-1), MoveTabRight => Command::MoveTabWithin(1),
        NewTerminal => Command::NewTerminal, ClearScrollback => Command::ClearScrollback,
        FontBigger => Command::FontDelta(1.0), FontSmaller => Command::FontDelta(-1.0),
        OpenConfig => Command::OpenConfig,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn keys_of(kb: &KeyBinding) -> String {
        kb.keystrokes().iter().map(|k| k.unparse()).collect::<Vec<_>>().join(" ")
    }

    /// The spec's table (section 4) plus the four this milestone adds. Names are the config keys.
    const SPEC_ROWS: &[(&str, &str, &str)] = &[
        ("cmd-k", "clear scrollback", "ClearScrollback"),
        ("cmd-1", "focus pane 1", "FocusPane1"), ("cmd-2", "focus pane 2", "FocusPane2"), ("cmd-3", "focus pane 3", "FocusPane3"),
        ("cmd-4", "focus pane 4", "FocusPane4"), ("cmd-5", "focus pane 5", "FocusPane5"), ("cmd-6", "focus pane 6", "FocusPane6"),
        ("cmd-7", "focus pane 7", "FocusPane7"), ("cmd-8", "focus pane 8", "FocusPane8"), ("cmd-9", "focus pane 9", "FocusPane9"),
        ("cmd-k cmd-left", "focus left", "FocusLeft"), ("cmd-k cmd-right", "focus right", "FocusRight"),
        ("cmd-k cmd-up", "focus up", "FocusUp"), ("cmd-k cmd-down", "focus down", "FocusDown"),
        ("cmd-\\", "split right", "SplitRight"), ("cmd-k cmd-\\", "split down", "SplitDown"),
        ("cmd-k cmd-m", "maximize pane", "ToggleMaximize"),
        ("ctrl-cmd-left", "move tab to previous pane", "MoveTabToPrevPane"), ("ctrl-cmd-right", "move tab to next pane", "MoveTabToNextPane"),
        ("alt-cmd-right", "next tab", "NextTab"), ("alt-cmd-left", "previous tab", "PrevTab"),
        ("cmd-shift-]", "next tab alt", "NextTabAlt"), ("cmd-shift-[", "previous tab alt", "PrevTabAlt"),
        ("ctrl-tab", "most recently used tab", "MruTab"),
        ("cmd-w", "close tab", "CloseTab"), ("cmd-shift-t", "reopen closed tab", "ReopenClosedTab"), ("cmd-k w", "close all in pane", "CloseAllInPane"),
        ("cmd-k cmd-shift-left", "move tab left", "MoveTabLeft"), ("cmd-k cmd-shift-right", "move tab right", "MoveTabRight"),
        ("ctrl-shift-`", "new terminal tab", "NewTerminal"),
        ("cmd-=", "font bigger", "FontBigger"), ("cmd--", "font smaller", "FontSmaller"),
        ("cmd-,", "open config", "OpenConfig"),
        ("cmd-q", "quit", "Quit"),
    ];

    #[test]
    fn the_table_is_the_spec_table() {
        assert_eq!(ACTIONS.len(), SPEC_ROWS.len(), "an action exists that the spec table does not list, or one is missing");
        for (keys, name, _) in SPEC_ROWS {
            let spec = ACTIONS.iter().find(|a| a.name == *name).unwrap_or_else(|| panic!("{name} missing from ACTIONS"));
            assert_eq!(spec.keys, *keys, "{name}");
        }
        assert_eq!(ACTIONS[0].name, "clear scrollback", "the lone cmd-k must be registered first (milestone 2a ruling 16)");
        assert_eq!(ACTIONS[0].context, Some("Terminal"));
    }

    #[test]
    fn every_row_runs_the_action_it_describes() {
        let b = bindings();
        assert_eq!(b.len(), SPEC_ROWS.len());
        for (keys, name, want) in SPEC_ROWS {
            let kb = b.iter().find(|kb| keys_of(kb) == *keys).unwrap_or_else(|| panic!("{keys} ({name}) unbound"));
            assert_eq!(kb.action().name(), format!("sarkos::{want}"), "{keys}");
        }
    }

    #[test]
    fn overrides_name_actions_and_keys_exactly() {
        let mut o = BTreeMap::new();
        o.insert("splitt right".to_string(), "cmd-\\".to_string());
        o.insert("split right".to_string(), "cmd+\\".to_string());
        let errors = validate(&o);
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(errors[0].contains("\"split right\"") && errors[0].contains("cmd+\\"), "{}", errors[0]);
        assert!(errors[1].contains("\"splitt right\""), "{}", errors[1]);
        // The defaults stay bound when validation fails: callers keep the last good set.
        let b = bindings_for(&o);
        assert!(b.iter().any(|kb| keys_of(kb) == "cmd-\\" && kb.action().name() == "sarkos::SplitRight"));

        let mut o = BTreeMap::new();
        o.insert("split right".to_string(), "cmd-d".to_string());
        o.insert("font bigger".to_string(), "".to_string());
        assert!(validate(&o).is_empty());
        let b = bindings_for(&o);
        assert_eq!(b.len(), ACTIONS.len() - 1, "an empty string unbinds");
        let split = b.iter().find(|kb| kb.action().name() == "sarkos::SplitRight").unwrap();
        assert_eq!(keys_of(split), "cmd-d");
        assert!(!b.iter().any(|kb| kb.action().name() == "sarkos::FontBigger"));
        assert_eq!(keys_for("split right", &o).as_deref(), Some("cmd-d"));
        assert_eq!(keys_for("font bigger", &o), None);
        assert_eq!(keys_for("font smaller", &o).as_deref(), Some("cmd--"));
        assert_eq!(keys_for("no such action", &o), None);
    }

    #[test]
    fn a_chord_override_keeps_cmd_k_pending() {
        use gpui::{KeyContext, Keymap, Keystroke};
        let mut o = BTreeMap::new();
        o.insert("maximize pane".to_string(), "cmd-k cmd-z".to_string());
        let keymap = Keymap::new(bindings_for(&o));
        let ctx = [KeyContext::parse("Root").unwrap(), KeyContext::parse("Terminal").unwrap()];
        let k = Keystroke::parse("cmd-k").unwrap();
        let (_, pending) = keymap.bindings_for_input(&[k.clone()], &ctx);
        assert!(pending, "a lone cmd-k must wait for a chord's second key");
        let (matched, pending) = keymap.bindings_for_input(&[k, Keystroke::parse("cmd-z").unwrap()], &ctx);
        assert!(!pending);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].action().name(), "sarkos::ToggleMaximize");
    }

    #[test]
    fn tab_cycling_wraps() {
        assert_eq!(tab_cycle(0, 3, 1), 1);
        assert_eq!(tab_cycle(2, 3, 1), 0);
        assert_eq!(tab_cycle(0, 3, -1), 2);
        assert_eq!(tab_cycle(0, 1, 1), 0);
        assert_eq!(tab_cycle(5, 0, 1), 0);
    }
}
