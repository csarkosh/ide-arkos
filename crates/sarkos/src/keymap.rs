//! The keyboard map (spec 4): VS Code's macOS defaults for the editor area, applied to panes
//! and terminal tabs. Every binding is global except ClearScrollback, which needs a terminal.

use crate::tree::Side;
use crate::Root;
use gpui::{actions, Context, Div, KeyBinding, Window};
use gpui::prelude::*;

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
        NewTerminal, ClearScrollback, FontBigger, FontSmaller
    ]
);

/// What a key does, in the window's terms.
#[derive(Debug, Clone, Copy, PartialEq)]
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
}

/// The spec's table (section 4), gpui keystroke syntax. Chords are space-separated.
pub const SPEC_ROWS: &[(&str, &str)] = &[
    ("cmd-1", "focus pane 1"), ("cmd-2", "focus pane 2"), ("cmd-3", "focus pane 3"), ("cmd-4", "focus pane 4"),
    ("cmd-5", "focus pane 5"), ("cmd-6", "focus pane 6"), ("cmd-7", "focus pane 7"), ("cmd-8", "focus pane 8"),
    ("cmd-9", "focus pane 9"),
    ("cmd-k cmd-left", "focus left"), ("cmd-k cmd-right", "focus right"), ("cmd-k cmd-up", "focus up"), ("cmd-k cmd-down", "focus down"),
    ("cmd-\\", "split right"), ("cmd-k cmd-\\", "split down"),
    ("cmd-k cmd-m", "maximize pane"),
    ("ctrl-cmd-left", "move tab to previous pane"), ("ctrl-cmd-right", "move tab to next pane"),
    ("alt-cmd-right", "next tab"), ("alt-cmd-left", "previous tab"), ("cmd-shift-]", "next tab"), ("cmd-shift-[", "previous tab"),
    ("ctrl-tab", "most recently used tab"),
    ("cmd-w", "close tab"), ("cmd-shift-t", "reopen closed tab"), ("cmd-k w", "close all in pane"),
    ("cmd-k cmd-shift-left", "move tab left"), ("cmd-k cmd-shift-right", "move tab right"),
    ("ctrl-shift-`", "new terminal tab"),
    ("cmd-k", "clear scrollback"),
    ("cmd-=", "font bigger"), ("cmd--", "font smaller"),
];

pub fn bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("cmd-1", FocusPane1, None),
        KeyBinding::new("cmd-2", FocusPane2, None),
        KeyBinding::new("cmd-3", FocusPane3, None),
        KeyBinding::new("cmd-4", FocusPane4, None),
        KeyBinding::new("cmd-5", FocusPane5, None),
        KeyBinding::new("cmd-6", FocusPane6, None),
        KeyBinding::new("cmd-7", FocusPane7, None),
        KeyBinding::new("cmd-8", FocusPane8, None),
        KeyBinding::new("cmd-9", FocusPane9, None),
        KeyBinding::new("cmd-k cmd-left", FocusLeft, None),
        KeyBinding::new("cmd-k cmd-right", FocusRight, None),
        KeyBinding::new("cmd-k cmd-up", FocusUp, None),
        KeyBinding::new("cmd-k cmd-down", FocusDown, None),
        KeyBinding::new("cmd-\\", SplitRight, None),
        KeyBinding::new("cmd-k cmd-\\", SplitDown, None),
        KeyBinding::new("cmd-k cmd-m", ToggleMaximize, None),
        KeyBinding::new("ctrl-cmd-left", MoveTabToPrevPane, None),
        KeyBinding::new("ctrl-cmd-right", MoveTabToNextPane, None),
        KeyBinding::new("alt-cmd-right", NextTab, None),
        KeyBinding::new("alt-cmd-left", PrevTab, None),
        KeyBinding::new("cmd-shift-]", NextTabAlt, None),
        KeyBinding::new("cmd-shift-[", PrevTabAlt, None),
        KeyBinding::new("ctrl-tab", MruTab, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("cmd-shift-t", ReopenClosedTab, None),
        KeyBinding::new("cmd-k w", CloseAllInPane, None),
        KeyBinding::new("cmd-k cmd-shift-left", MoveTabLeft, None),
        KeyBinding::new("cmd-k cmd-shift-right", MoveTabRight, None),
        KeyBinding::new("ctrl-shift-`", NewTerminal, None),
        // Only when a terminal has focus, so the Cmd+K chords above keep working elsewhere.
        KeyBinding::new("cmd-k", ClearScrollback, Some("Terminal")),
        KeyBinding::new("cmd-=", FontBigger, None),
        KeyBinding::new("cmd--", FontSmaller, None),
    ]
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

/// Registers every action on the window's root element.
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
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_spec_row_is_bound_once() {
        let b = bindings();
        for (keys, name) in SPEC_ROWS {
            let hits: Vec<_> = b.iter().filter(|kb| kb.keystrokes().iter().map(|k| k.unparse()).collect::<Vec<_>>().join(" ") == *keys).collect();
            assert_eq!(hits.len(), 1, "{keys} ({name}) bound {} times", hits.len());
        }
        assert_eq!(b.len(), SPEC_ROWS.len(), "a binding exists that the spec table does not list");
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
