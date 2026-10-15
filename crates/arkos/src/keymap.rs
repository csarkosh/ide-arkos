//! The keyboard map (spec 4): VS Code's macOS defaults for the editor area, applied to panes
//! and terminal tabs. Every binding has a name, which is its key in the config file's
//! `[keybindings]` table; `bindings_for` applies the overrides from there.

use crate::tree::Side;
use crate::Root;
use gpui::prelude::*;
use gpui::{actions, Context, Div, KeyBinding, Keystroke, Window};
use std::collections::BTreeMap;

actions!(
    arkos,
    [
        FocusPane1, FocusPane2, FocusPane3, FocusPane4, FocusPane5, FocusPane6, FocusPane7, FocusPane8, FocusPane9,
        FocusLeft, FocusRight, FocusUp, FocusDown,
        SplitRight, SplitDown, ToggleMaximize,
        MoveTabToPrevPane, MoveTabToNextPane,
        NextTab, PrevTab, NextTabAlt, PrevTabAlt, MruTab,
        CloseTab, ReopenClosedTab, CloseAllInPane,
        MoveTabLeft, MoveTabRight,
        NewTerminal, ClearToStart, FontBigger, FontSmaller,
        OpenConfig, CommandPalette, Find, UpdateNow, CheckForUpdates, Notifications, Doctor, RetryUpdate, StartAtLogin, InstallHooks,
        ToggleSidebar, FocusFileTree, ShowIgnoredFiles, FocusWorktrees, ToggleInlineDiff, OpenFolder, NewWindow,
        NewBrowserTab, NewBrowserTabSinglePage, DockChromeWindow, UndockChromeWindow, Quit
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
    /// Terminal.app's Cmd+K: drop the history and the screen, keep the cursor line at the top.
    ClearToStart,
    FontDelta(f32),
    OpenConfig,
    /// Switch every terminal to the named text preset (the palette runs it).
    TextPreset(String),
    /// Re-read config.toml now (the palette runs it; the watcher does the same on every save).
    ReloadConfig,
    /// Write the built-in themes into the themes folder (the palette runs it).
    ExtractThemes,
    /// Open the command palette, or close it when it is open.
    CommandPalette,
    /// Switch to the named theme for this run; the config file is the only place a theme is kept.
    SetTheme(String),
    /// Open the active terminal's find bar, or focus it when it is open.
    Find,
    /// Run the installed update: the daemon first when its binary changed, then this window.
    UpdateNow,
    /// Ask the daemon to check for a new release now.
    CheckForUpdates,
    /// Opens the bell's popover.
    Notifications,
    /// Opens the doctor's popover.
    Doctor,
    /// Forget the last update error and check again (the palette offers it only after one).
    RetryUpdate,
    /// Install or remove the login item that brings sessions back after a restart; with the
    /// config pinning it, open the config instead.
    StartAtLogin,
    /// Merge the Claude Code hook entries into its settings file.
    InstallHooks,
    /// Collapse or show the sidebar.
    ToggleSidebar,
    /// Put keyboard focus in the file tree.
    FocusFileTree,
    /// Flip whether ignored entries are listed, dimmed.
    ShowIgnoredFiles,
    /// Put keyboard focus in the worktree list.
    FocusWorktrees,
    /// Show or hide the focused viewer's deleted lines between its current ones.
    ToggleInlineDiff,
    /// Pick a folder with the native picker and open it in a new window.
    OpenFolder,
    /// Show the project chooser in its own window (the File menu's "New Window"); the folder
    /// chosen opens in a new window.
    NewWindow,
    /// Ask for a URL and open a browser tab for it in the focused pane.
    NewBrowserTab,
    /// `NewBrowserTab` with the window opened as a single page (Chrome's app mode).
    NewBrowserTabSinglePage,
    /// List Chrome's windows in the palette, to dock one into a browser tab.
    DockChromeWindow,
    /// Dock the dock list's row: `Root.dock_choices[i]` (the palette runs it).
    DockChromeWindowAt(usize),
    /// Release the focused browser tab's Chrome window; the tab stays, waiting.
    UndockChromeWindow,
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

/// The spec's table (section 4) plus open config, the command palette, find, the update commands
/// and quit. The order is the registration order. "update now", "check for updates" and "retry
/// update" have no default keys: they are palette commands unless the config binds them.
pub const ACTIONS: &[ActionSpec] = &[
    // First on purpose: gpui's bindings_for_input (keymap.rs:191-197) drops a pending chord
    // whose index is after the best exact match, so a later lone cmd-k would fire at once and
    // the Cmd+K chords would never complete. Terminal-only, so chords work elsewhere too.
    ActionSpec { name: "clear to start", keys: "cmd-k", context: Some("Terminal") },
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
    row("command palette", "cmd-shift-p"),
    row("find", "cmd-f"),
    row("update now", ""),
    row("check for updates", ""),
    row("notifications", ""),
    row("doctor", ""),
    row("retry update", ""),
    row("start at login", ""),
    row("install claude code hooks", ""),
    row("toggle sidebar", "cmd-b"),
    row("focus file tree", "cmd-shift-e"),
    row("show ignored files", ""),
    row("focus worktrees", "ctrl-shift-g"),
    ActionSpec { name: "toggle inline diff", keys: "cmd-shift-d", context: Some("Viewer") },
    row("open folder", "cmd-o"),
    row("new window", "cmd-n"),
    row("new browser tab", "cmd-shift-b"),
    row("new single-page browser tab", ""),
    row("dock a chrome window", ""),
    row("undock chrome window", ""),
    row("quit", "cmd-q"),
];

/// The US layout's shifted symbols. gpui on macOS reports a shifted symbol as the shifted
/// character with Shift cleared (Ctrl+Shift+` arrives as `ctrl-~`), while a shifted letter
/// keeps Shift as a modifier (`cmd-shift-p`). A binding is written the way it is read,
/// "ctrl-shift-`", and registered the way gpui sees it.
const SHIFTED: &[(&str, &str)] = &[
    ("`", "~"), ("1", "!"), ("2", "@"), ("3", "#"), ("4", "$"), ("5", "%"), ("6", "^"), ("7", "&"), ("8", "*"), ("9", "("), ("0", ")"),
    ("-", "_"), ("=", "+"), ("[", "{"), ("]", "}"), ("\\", "|"), (";", ":"), ("'", "\""), (",", "<"), (".", ">"), ("/", "?"),
];

/// `keys` as gpui reports them: each "shift-<symbol>" becomes the symbol's shifted character
/// without Shift. Letters, named keys and strokes gpui cannot parse are left as they are.
pub fn as_gpui(keys: &str) -> String {
    keys.split_whitespace()
        .map(|stroke| {
            let Ok(mut k) = Keystroke::parse(stroke) else { return stroke.to_string() };
            if k.modifiers.shift {
                if let Some((_, shifted)) = SHIFTED.iter().find(|(plain, _)| *plain == k.key) {
                    k.key = shifted.to_string();
                    k.modifiers.shift = false;
                }
            }
            k.unparse()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The binding for a named action. `keys` must already be validated: `KeyBinding::new` panics
/// on a keystroke it cannot parse.
fn binding(name: &str, keys: &str, context: Option<&str>) -> Option<KeyBinding> {
    let keys = &as_gpui(keys);
    Some(match name {
        "clear to start" => KeyBinding::new(keys, ClearToStart, context),
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
        "command palette" => KeyBinding::new(keys, CommandPalette, context),
        "find" => KeyBinding::new(keys, Find, context),
        "update now" => KeyBinding::new(keys, UpdateNow, context),
        "check for updates" => KeyBinding::new(keys, CheckForUpdates, context),
        "notifications" => KeyBinding::new(keys, Notifications, context),
        "doctor" => KeyBinding::new(keys, Doctor, context),
        "retry update" => KeyBinding::new(keys, RetryUpdate, context),
        "start at login" => KeyBinding::new(keys, StartAtLogin, context),
        "install claude code hooks" => KeyBinding::new(keys, InstallHooks, context),
        "toggle sidebar" => KeyBinding::new(keys, ToggleSidebar, context),
        "focus file tree" => KeyBinding::new(keys, FocusFileTree, context),
        "show ignored files" => KeyBinding::new(keys, ShowIgnoredFiles, context),
        "focus worktrees" => KeyBinding::new(keys, FocusWorktrees, context),
        "toggle inline diff" => KeyBinding::new(keys, ToggleInlineDiff, context),
        "open folder" => KeyBinding::new(keys, OpenFolder, context),
        "new window" => KeyBinding::new(keys, NewWindow, context),
        "new browser tab" => KeyBinding::new(keys, NewBrowserTab, context),
        "new single-page browser tab" => KeyBinding::new(keys, NewBrowserTabSinglePage, context),
        "dock a chrome window" => KeyBinding::new(keys, DockChromeWindow, context),
        "undock chrome window" => KeyBinding::new(keys, UndockChromeWindow, context),
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

/// Names an action used to go by, still accepted in `[keybindings]` so a config file written
/// before the rename keeps working: the old name and the one it means now.
const ALIASES: &[(&str, &str)] = &[("open recent", "new window")];

/// The current name of an action, from either its name or an old one.
pub fn canonical(name: &str) -> &str {
    ALIASES.iter().find(|(old, _)| *old == name).map_or(name, |(_, new)| *new)
}

/// Every problem with an override table, in table order: an action name that is not in
/// `ACTIONS` (under its current or an old name), or keystrokes gpui cannot parse. Empty when
/// the table is fine.
pub fn validate(overrides: &BTreeMap<String, String>) -> Vec<String> {
    let mut errors = Vec::new();
    for (name, keys) in overrides {
        let name = canonical(name);
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
    // An override under the action's old name counts too.
    let aliased = || ALIASES.iter().filter(|(_, new)| *new == name).find_map(|(old, _)| overrides.get(*old));
    let keys = overrides.get(name).or_else(aliased).map(String::as_str).unwrap_or(spec.keys);
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

#[cfg(test)]
pub fn bindings() -> Vec<KeyBinding> {
    bindings_for(&BTreeMap::new())
}

/// A keystroke the way the Mac writes it: `cmd-shift-p` is `Shift+Cmd+P` (Apple's menu order:
/// Control, Option, Shift, Command), chords join with " then ".
pub fn pretty_keys(keys: &str) -> String {
    keys.split_whitespace()
        .map(|stroke| {
            let Ok(k) = Keystroke::parse(stroke) else { return stroke.to_string() };
            let mut parts = Vec::new();
            if k.modifiers.control {
                parts.push("Ctrl".to_string());
            }
            if k.modifiers.alt {
                parts.push("Option".to_string());
            }
            // A shifted symbol reads as Shift plus the plain key, whichever way it was written.
            let plain = SHIFTED.iter().find(|(_, shifted)| *shifted == k.key).map(|(plain, _)| *plain);
            if k.modifiers.shift || plain.is_some() {
                parts.push("Shift".to_string());
            }
            if k.modifiers.platform {
                parts.push("Cmd".to_string());
            }
            let key = match plain.unwrap_or(k.key.as_str()) {
                "left" => "Left".to_string(),
                "right" => "Right".to_string(),
                "up" => "Up".to_string(),
                "down" => "Down".to_string(),
                "enter" => "Enter".to_string(),
                "tab" => "Tab".to_string(),
                "escape" => "Escape".to_string(),
                "backspace" => "Backspace".to_string(),
                "space" => "Space".to_string(),
                other => other.to_uppercase(),
            };
            parts.push(key);
            parts.join("+")
        })
        .collect::<Vec<_>>()
        .join(" then ")
}

/// An action name as a palette label: first letter upper-cased.
pub fn label(name: &str) -> String {
    let mut c = name.chars();
    match c.next() {
        Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// The command an action name runs, for the palette. gpui's `on_action` needs a concrete action
/// type per listener, so `attach` keeps its macro; this is the same mapping as a lookup. The
/// palette itself and quit are not listed.
pub fn command_for(name: &str) -> Option<Command> {
    Some(match name {
        "clear to start" => Command::ClearToStart,
        "focus pane 1" => Command::FocusPane(0),
        "focus pane 2" => Command::FocusPane(1),
        "focus pane 3" => Command::FocusPane(2),
        "focus pane 4" => Command::FocusPane(3),
        "focus pane 5" => Command::FocusPane(4),
        "focus pane 6" => Command::FocusPane(5),
        "focus pane 7" => Command::FocusPane(6),
        "focus pane 8" => Command::FocusPane(7),
        "focus pane 9" => Command::FocusPane(8),
        "focus left" => Command::FocusDir(Side::Left),
        "focus right" => Command::FocusDir(Side::Right),
        "focus up" => Command::FocusDir(Side::Top),
        "focus down" => Command::FocusDir(Side::Bottom),
        "split right" => Command::Split(Side::Right),
        "split down" => Command::Split(Side::Bottom),
        "maximize pane" => Command::ToggleMaximize,
        "move tab to previous pane" => Command::MoveTabToPane(-1),
        "move tab to next pane" => Command::MoveTabToPane(1),
        "next tab" | "next tab alt" => Command::CycleTab(1),
        "previous tab" | "previous tab alt" => Command::CycleTab(-1),
        "most recently used tab" => Command::MruTab,
        "close tab" => Command::CloseTab,
        "reopen closed tab" => Command::ReopenClosedTab,
        "close all in pane" => Command::CloseAllInPane,
        "move tab left" => Command::MoveTabWithin(-1),
        "move tab right" => Command::MoveTabWithin(1),
        "new terminal tab" => Command::NewTerminal,
        "font bigger" => Command::FontDelta(1.0),
        "font smaller" => Command::FontDelta(-1.0),
        "open config" => Command::OpenConfig,
        "find" => Command::Find,
        "update now" => Command::UpdateNow,
        "check for updates" => Command::CheckForUpdates,
        "notifications" => Command::Notifications,
        "doctor" => Command::Doctor,
        "retry update" => Command::RetryUpdate,
        "start at login" => Command::StartAtLogin,
        "install claude code hooks" => Command::InstallHooks,
        "toggle sidebar" => Command::ToggleSidebar,
        "focus file tree" => Command::FocusFileTree,
        "show ignored files" => Command::ShowIgnoredFiles,
        "focus worktrees" => Command::FocusWorktrees,
        "toggle inline diff" => Command::ToggleInlineDiff,
        "open folder" => Command::OpenFolder,
        "new window" => Command::NewWindow,
        "new browser tab" => Command::NewBrowserTab,
        "new single-page browser tab" => Command::NewBrowserTabSinglePage,
        "dock a chrome window" => Command::DockChromeWindow,
        "undock chrome window" => Command::UndockChromeWindow,
        _ => return None,
    })
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
        NewTerminal => Command::NewTerminal, ClearToStart => Command::ClearToStart,
        FontBigger => Command::FontDelta(1.0), FontSmaller => Command::FontDelta(-1.0),
        OpenConfig => Command::OpenConfig, CommandPalette => Command::CommandPalette, Find => Command::Find,
        UpdateNow => Command::UpdateNow, CheckForUpdates => Command::CheckForUpdates, Notifications => Command::Notifications, Doctor => Command::Doctor, RetryUpdate => Command::RetryUpdate, StartAtLogin => Command::StartAtLogin, InstallHooks => Command::InstallHooks,
        ToggleSidebar => Command::ToggleSidebar, FocusFileTree => Command::FocusFileTree, ShowIgnoredFiles => Command::ShowIgnoredFiles,
        FocusWorktrees => Command::FocusWorktrees, ToggleInlineDiff => Command::ToggleInlineDiff,
        OpenFolder => Command::OpenFolder, NewWindow => Command::NewWindow,
        NewBrowserTab => Command::NewBrowserTab, NewBrowserTabSinglePage => Command::NewBrowserTabSinglePage,
        DockChromeWindow => Command::DockChromeWindow, UndockChromeWindow => Command::UndockChromeWindow,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn keys_of(kb: &KeyBinding) -> String {
        kb.keystrokes().iter().map(|k| k.unparse()).collect::<Vec<_>>().join(" ")
    }

    /// The spec's table (section 4) plus the rows later milestones add. Names are the config
    /// keys; empty keys mean unbound by default.
    const SPEC_ROWS: &[(&str, &str, &str)] = &[
        ("cmd-k", "clear to start", "ClearToStart"),
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
        ("cmd-shift-p", "command palette", "CommandPalette"),
        ("cmd-f", "find", "Find"),
        ("", "update now", "UpdateNow"),
        ("", "check for updates", "CheckForUpdates"),
        ("", "notifications", "Notifications"),
        ("", "doctor", "Doctor"),
        ("", "retry update", "RetryUpdate"),
        ("", "start at login", "StartAtLogin"),
        ("", "install claude code hooks", "InstallHooks"),
        ("cmd-b", "toggle sidebar", "ToggleSidebar"),
        ("cmd-shift-e", "focus file tree", "FocusFileTree"),
        ("", "show ignored files", "ShowIgnoredFiles"),
        ("ctrl-shift-g", "focus worktrees", "FocusWorktrees"),
        ("cmd-shift-d", "toggle inline diff", "ToggleInlineDiff"),
        ("cmd-o", "open folder", "OpenFolder"),
        ("cmd-n", "new window", "NewWindow"),
        ("cmd-shift-b", "new browser tab", "NewBrowserTab"),
        ("", "new single-page browser tab", "NewBrowserTabSinglePage"),
        ("", "dock a chrome window", "DockChromeWindow"),
        ("", "undock chrome window", "UndockChromeWindow"),
        ("cmd-q", "quit", "Quit"),
    ];

    #[test]
    fn an_old_action_name_in_the_config_still_binds_the_renamed_action() {
        let mut o = BTreeMap::new();
        o.insert("open recent".to_string(), "cmd-shift-n".to_string());
        assert!(validate(&o).is_empty(), "the old name is not an unknown action");
        assert_eq!(canonical("open recent"), "new window");
        assert_eq!(canonical("new window"), "new window");
        assert_eq!(keys_for("new window", &o), Some("cmd-shift-n".to_string()));
        assert_eq!(keys_for("new window", &BTreeMap::new()), Some("cmd-n".to_string()));
        o.insert("new window".to_string(), "".to_string());
        assert_eq!(keys_for("new window", &o), None, "the current name wins over the old one");
    }

    #[test]
    fn the_table_is_the_spec_table() {
        assert_eq!(ACTIONS.len(), SPEC_ROWS.len(), "an action exists that the spec table does not list, or one is missing");
        for (keys, name, _) in SPEC_ROWS {
            let spec = ACTIONS.iter().find(|a| a.name == *name).unwrap_or_else(|| panic!("{name} missing from ACTIONS"));
            assert_eq!(spec.keys, *keys, "{name}");
        }
        assert_eq!(ACTIONS[0].name, "clear to start", "the lone cmd-k must be registered first (milestone 2a ruling 16)");
        assert_eq!(ACTIONS[0].context, Some("Terminal"));
        let inline = ACTIONS.iter().find(|a| a.name == "toggle inline diff").unwrap();
        assert_eq!(inline.context, Some("Viewer"), "Cmd+Shift+D acts on the focused viewer only");
    }

    #[test]
    fn every_row_runs_the_action_it_describes() {
        let b = bindings();
        assert_eq!(b.len(), SPEC_ROWS.iter().filter(|(keys, _, _)| !keys.is_empty()).count());
        // A row with empty keys is unbound by default: nothing to press.
        for (keys, name, want) in SPEC_ROWS.iter().filter(|(keys, _, _)| !keys.is_empty()) {
            // Registered as gpui reports them: a shifted symbol is its shifted character.
            let kb = b.iter().find(|kb| keys_of(kb) == as_gpui(keys)).unwrap_or_else(|| panic!("{keys} ({name}) unbound"));
            assert_eq!(kb.action().name(), format!("arkos::{want}"), "{keys}");
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
        assert!(b.iter().any(|kb| keys_of(kb) == "cmd-\\" && kb.action().name() == "arkos::SplitRight"));

        let mut o = BTreeMap::new();
        o.insert("split right".to_string(), "cmd-d".to_string());
        o.insert("font bigger".to_string(), "".to_string());
        assert!(validate(&o).is_empty());
        let b = bindings_for(&o);
        assert_eq!(b.len(), ACTIONS.len() - 12, "an empty string unbinds, and update now, check for updates, notifications, doctor, retry update, start at login, install hooks, show ignored files, new single-page browser tab, dock a chrome window and undock chrome window have no default keys");
        let split = b.iter().find(|kb| kb.action().name() == "arkos::SplitRight").unwrap();
        assert_eq!(keys_of(split), "cmd-d");
        assert!(!b.iter().any(|kb| kb.action().name() == "arkos::FontBigger"));
        assert_eq!(keys_for("split right", &o).as_deref(), Some("cmd-d"));
        assert_eq!(keys_for("font bigger", &o), None);
        assert_eq!(keys_for("font smaller", &o).as_deref(), Some("cmd--"));
        assert_eq!(keys_for("no such action", &o), None);
        assert_eq!(keys_for("update now", &BTreeMap::new()), None, "unbound by default");
        assert_eq!(bindings().len(), ACTIONS.len() - 11, "only update now, check for updates, notifications, doctor, retry update, start at login, install hooks, show ignored files, new single-page browser tab, dock a chrome window and undock chrome window are unbound by default");
        assert_eq!(command_for("new browser tab"), Some(Command::NewBrowserTab));
        assert_eq!(command_for("new single-page browser tab"), Some(Command::NewBrowserTabSinglePage));
        assert_eq!(command_for("dock a chrome window"), Some(Command::DockChromeWindow));
        assert_eq!(command_for("undock chrome window"), Some(Command::UndockChromeWindow));
        assert_eq!(command_for("notifications"), Some(Command::Notifications));
        assert_eq!(command_for("doctor"), Some(Command::Doctor));
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
        assert_eq!(matched[0].action().name(), "arkos::ToggleMaximize");
    }

    #[test]
    fn shifted_symbols_are_registered_as_gpui_reports_them() {
        // macOS gives the shifted character with Shift cleared for symbols, not for letters.
        assert_eq!(as_gpui("ctrl-shift-`"), "ctrl-~");
        assert_eq!(as_gpui("cmd-shift-]"), "cmd-}");
        assert_eq!(as_gpui("cmd-shift-["), "cmd-{");
        assert_eq!(as_gpui("cmd-shift-p"), "cmd-shift-p");
        assert_eq!(as_gpui("cmd-k cmd-shift-left"), "cmd-k cmd-shift-left");
        assert_eq!(as_gpui("ctrl-~"), "ctrl-~", "already the way gpui sees it");
        assert_eq!(as_gpui("cmd-shift-t"), "cmd-shift-t");
        // The new-terminal binding is what Ctrl+Shift+` sends, whichever way it is written.
        let b = bindings();
        assert!(b.iter().any(|kb| keys_of(kb) == "ctrl-~" && kb.action().name() == "arkos::NewTerminal"));
        let mut o = BTreeMap::new();
        o.insert("new terminal tab".to_string(), "ctrl-shift-`".to_string());
        assert!(bindings_for(&o).iter().any(|kb| keys_of(kb) == "ctrl-~" && kb.action().name() == "arkos::NewTerminal"));
        o.insert("new terminal tab".to_string(), "ctrl-~".to_string());
        assert!(bindings_for(&o).iter().any(|kb| keys_of(kb) == "ctrl-~" && kb.action().name() == "arkos::NewTerminal"));
    }

    #[test]
    fn keys_print_the_mac_way() {
        assert_eq!(pretty_keys("cmd-shift-p"), "Shift+Cmd+P");
        assert_eq!(pretty_keys("cmd-k cmd-\\"), "Cmd+K then Cmd+\\");
        assert_eq!(pretty_keys("ctrl-shift-`"), "Ctrl+Shift+`");
        assert_eq!(pretty_keys("ctrl-~"), "Ctrl+Shift+`", "the registered form reads the same");
        assert_eq!(pretty_keys("cmd-}"), "Shift+Cmd+]");
        assert_eq!(pretty_keys("alt-cmd-right"), "Option+Cmd+Right");
        assert_eq!(pretty_keys("cmd-k w"), "Cmd+K then W");
        assert_eq!(pretty_keys("cmd-,"), "Cmd+,");
        assert_eq!(pretty_keys("cmd-="), "Cmd+=");
        assert_eq!(pretty_keys("ctrl-tab"), "Ctrl+Tab");
        assert_eq!(label("focus pane 1"), "Focus pane 1");
    }

    #[test]
    fn every_action_has_a_palette_command_except_palette_and_quit() {
        for a in ACTIONS {
            let has = command_for(a.name).is_some();
            assert_eq!(has, a.name != "command palette" && a.name != "quit", "{}", a.name);
        }
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
