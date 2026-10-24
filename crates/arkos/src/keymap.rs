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
        About, OpenSettings, Keybindings, Documentation, OpenConfig, CommandPalette, Find, CopyAsShown, UpdateNow, CheckForUpdates, Notifications, Doctor, RetryUpdate, StartAtLogin, InstallHooks, InstallAgentGuide,
        ToggleSidebar, FocusFileTree, ShowIgnoredFiles, FocusWorktrees, ToggleInlineDiff, ToggleMarkdownView, SaveFile, OpenFolder, NewWindow,
        NewBrowserTab, Quit,
        OpenFileInPane,
        VideoPlayPause, VideoBack, VideoForward, VideoBackFine, VideoForwardFine, VideoStepBack, VideoStepForward,
        VideoStart, VideoLoop, VideoMute, VideoReload,
        PageBack, PageForward, ReloadPage, FocusAddress, OpenInBrowser, ZoomReset,
        // The Help menu's "What Arkos Sends" has no key and no palette entry, so it is handled
        // outside `Command`.
        WhatArkosSends
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
    /// Open the About window, or bring it forward when it is open.
    About,
    /// Open the Settings window, or bring it forward when it is open.
    OpenSettings,
    /// Open the Keybindings window, or bring it forward when it is open.
    Keybindings,
    /// Open the guide in the browser.
    Documentation,
    /// Open config.toml in the default text editor.
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
    /// Copy the focused terminal's selection with its rows as they are on screen, the way out
    /// when Cmd+C's re-flow should not apply (code, a table).
    CopyAsShown,
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
    /// Install the agent guide as a skill in this environment's Claude config folder.
    InstallAgentGuide,
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
    /// Flip the focused markdown tab between rendered and raw.
    ToggleMarkdownView,
    /// Write the focused markdown document now; during a conflict, keep mine.
    SaveFile,
    /// Pick a folder with the native picker and open it in a new window.
    OpenFolder,
    /// Show the project chooser in its own window (the "New Window" item in the Arkos IDE menu);
    /// the folder chosen opens in a new window.
    NewWindow,
    /// Ask for a URL and open a web tab for it in the focused pane.
    NewBrowserTab,
    /// Ask for a path and open that file in the focused pane, a video as a video tab.
    OpenFileInPane,
    /// The focused video tab's commands (spec 3.4): play or pause.
    VideoPlayPause,
    /// Five seconds back or forward.
    VideoBack,
    VideoForward,
    /// One second back or forward.
    VideoBackFine,
    VideoForwardFine,
    /// One frame back or forward, pausing first.
    VideoStepBack,
    VideoStepForward,
    /// Back to the start.
    VideoStart,
    /// Loop toggle; with a selection on the scrubber, loop that range.
    VideoLoop,
    VideoMute,
    /// Load the file again now, without waiting for a change.
    VideoReload,
    /// The web tab's history, back and forward (web pane spec, section 4).
    PageBack,
    PageForward,
    /// Reload the page, or retry a failed load.
    ReloadPage,
    /// Put the keys in the address field, the address selected.
    FocusAddress,
    /// The page in the default browser.
    OpenInBrowser,
    /// Page zoom back to 1; "font bigger" and "font smaller" step it in a web tab.
    ZoomReset,
}

/// Where an action is listed in the Keybindings window, by what it acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Window,
    Panes,
    Tabs,
    Terminal,
    Sidebar,
    Browser,
    Video,
    Markdown,
    Maintenance,
}

impl Section {
    /// Every section, in display order.
    pub const ALL: [Section; 9] = [
        Section::Window,
        Section::Panes,
        Section::Tabs,
        Section::Terminal,
        Section::Sidebar,
        Section::Browser,
        Section::Video,
        Section::Markdown,
        Section::Maintenance,
    ];

    /// The section's heading in the Keybindings window.
    pub fn title(self) -> &'static str {
        match self {
            Section::Window => "Window and app",
            Section::Panes => "Panes",
            Section::Tabs => "Tabs",
            Section::Terminal => "Terminal",
            Section::Sidebar => "Sidebar and files",
            Section::Browser => "Browser",
            Section::Video => "Video",
            Section::Markdown => "Viewer and markdown editing",
            Section::Maintenance => "Updates and maintenance",
        }
    }
}

/// One row of the table: the config name, the default keystrokes (gpui syntax, chords space
/// separated), the key context the binding needs and the section it is listed under.
pub struct ActionSpec {
    pub name: &'static str,
    pub keys: &'static str,
    pub context: Option<&'static str>,
    pub section: Section,
}

const fn row(section: Section, name: &'static str, keys: &'static str) -> ActionSpec {
    ActionSpec { name, keys, context: None, section }
}

/// The spec's table (section 4) plus about, settings, keybindings, documentation, open config,
/// the command palette, find, the update commands and quit. The order is the registration order. "update now", "check for
/// updates" and "retry update" have no default keys: they are palette commands unless the config
/// binds them.
pub const ACTIONS: &[ActionSpec] = &[
    // First on purpose: gpui's bindings_for_input (keymap.rs:191-197) drops a pending chord
    // whose index is after the best exact match, so a later lone cmd-k would fire at once and
    // the Cmd+K chords would never complete. Terminal-only, so chords work elsewhere too.
    ActionSpec { name: "clear to start", keys: "cmd-k", context: Some("Terminal"), section: Section::Terminal },
    row(Section::Panes, "focus pane 1", "cmd-1"), row(Section::Panes, "focus pane 2", "cmd-2"), row(Section::Panes, "focus pane 3", "cmd-3"),
    row(Section::Panes, "focus pane 4", "cmd-4"), row(Section::Panes, "focus pane 5", "cmd-5"), row(Section::Panes, "focus pane 6", "cmd-6"),
    row(Section::Panes, "focus pane 7", "cmd-7"), row(Section::Panes, "focus pane 8", "cmd-8"), row(Section::Panes, "focus pane 9", "cmd-9"),
    row(Section::Panes, "focus left", "cmd-k cmd-left"), row(Section::Panes, "focus right", "cmd-k cmd-right"),
    row(Section::Panes, "focus up", "cmd-k cmd-up"), row(Section::Panes, "focus down", "cmd-k cmd-down"),
    row(Section::Panes, "split right", "cmd-\\"), row(Section::Panes, "split down", "cmd-k cmd-\\"),
    row(Section::Panes, "maximize pane", "cmd-k cmd-m"),
    row(Section::Tabs, "move tab to previous pane", "ctrl-cmd-left"), row(Section::Tabs, "move tab to next pane", "ctrl-cmd-right"),
    row(Section::Tabs, "next tab", "alt-cmd-right"), row(Section::Tabs, "previous tab", "alt-cmd-left"),
    row(Section::Tabs, "next tab alt", "cmd-shift-]"), row(Section::Tabs, "previous tab alt", "cmd-shift-["),
    row(Section::Tabs, "most recently used tab", "ctrl-tab"),
    row(Section::Tabs, "close tab", "cmd-w"), row(Section::Tabs, "reopen closed tab", "cmd-shift-t"), row(Section::Tabs, "close all in pane", "cmd-k w"),
    row(Section::Tabs, "move tab left", "cmd-k cmd-shift-left"), row(Section::Tabs, "move tab right", "cmd-k cmd-shift-right"),
    row(Section::Tabs, "new terminal tab", "ctrl-shift-`"),
    row(Section::Terminal, "font bigger", "cmd-="), row(Section::Terminal, "font smaller", "cmd--"),
    // Before "open config" on purpose: gpui ranks bindings at the same depth later-first, so a
    // user's `"open config" = "cmd-,"` wins over Settings' default Cmd+,.
    row(Section::Window, "about", ""),
    row(Section::Window, "settings", "cmd-,"),
    row(Section::Window, "keybindings", "cmd-/"),
    row(Section::Window, "documentation", ""),
    row(Section::Window, "open config", ""),
    row(Section::Window, "command palette", "cmd-shift-p"),
    row(Section::Terminal, "find", "cmd-f"),
    // Terminal-only, so Shift+Cmd+C stays free in the viewer and the browser.
    ActionSpec { name: "copy as shown", keys: "cmd-shift-c", context: Some("Terminal"), section: Section::Terminal },
    row(Section::Maintenance, "update now", ""),
    row(Section::Maintenance, "check for updates", ""),
    row(Section::Maintenance, "notifications", ""),
    row(Section::Maintenance, "doctor", ""),
    row(Section::Maintenance, "retry update", ""),
    row(Section::Maintenance, "start at login", ""),
    row(Section::Maintenance, "install claude code hooks", ""),
    row(Section::Maintenance, "install agent guide", ""),
    row(Section::Sidebar, "toggle sidebar", "cmd-b"),
    row(Section::Sidebar, "focus file tree", "cmd-shift-e"),
    row(Section::Sidebar, "show ignored files", ""),
    row(Section::Sidebar, "focus worktrees", "ctrl-shift-g"),
    ActionSpec { name: "toggle inline diff", keys: "cmd-shift-d", context: Some("Viewer"), section: Section::Markdown },
    ActionSpec { name: "toggle markdown view", keys: "cmd-shift-m", context: Some("Viewer"), section: Section::Markdown },
    ActionSpec { name: "save file", keys: "cmd-s", context: Some("Viewer"), section: Section::Markdown },
    row(Section::Window, "open folder", "cmd-o"),
    row(Section::Window, "new window", "cmd-n"),
    row(Section::Browser, "new browser tab", "cmd-shift-b"),
    row(Section::Sidebar, "open file in pane", ""),
    browser("page back", "cmd-["),
    browser("page forward", "cmd-]"),
    browser("reload page", "cmd-r"),
    browser("focus address", "cmd-l"),
    browser("open in browser", "cmd-shift-o"),
    browser("zoom reset", "cmd-0"),
    // The video keys (spec 3.4) act only in a focused video tab: a terminal never sees this map.
    // "video start alt" is Home's second key, 0, as "next tab alt" is Cmd+Shift+].
    video("video play pause", "space"),
    video("video back", "left"),
    video("video forward", "right"),
    video("video back fine", "shift-left"),
    video("video forward fine", "shift-right"),
    video("video step back", ","),
    video("video step forward", "."),
    video("video start", "home"),
    video("video start alt", "0"),
    video("video loop", "l"),
    video("video mute", "m"),
    video("video reload", "r"),
    row(Section::Window, "quit", "cmd-q"),
];

/// A row that acts only in a focused video tab.
const fn video(name: &'static str, keys: &'static str) -> ActionSpec {
    ActionSpec { name, keys, context: Some("Video"), section: Section::Video }
}

/// A web tab's key (web pane spec, section 4): acts only in a focused web tab.
const fn browser(name: &'static str, keys: &'static str) -> ActionSpec {
    ActionSpec { name, keys, context: Some("Browser"), section: Section::Browser }
}

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
        "about" => KeyBinding::new(keys, About, context),
        "settings" => KeyBinding::new(keys, OpenSettings, context),
        "keybindings" => KeyBinding::new(keys, Keybindings, context),
        "documentation" => KeyBinding::new(keys, Documentation, context),
        "open config" => KeyBinding::new(keys, OpenConfig, context),
        "command palette" => KeyBinding::new(keys, CommandPalette, context),
        "find" => KeyBinding::new(keys, Find, context),
        "copy as shown" => KeyBinding::new(keys, CopyAsShown, context),
        "update now" => KeyBinding::new(keys, UpdateNow, context),
        "check for updates" => KeyBinding::new(keys, CheckForUpdates, context),
        "notifications" => KeyBinding::new(keys, Notifications, context),
        "doctor" => KeyBinding::new(keys, Doctor, context),
        "retry update" => KeyBinding::new(keys, RetryUpdate, context),
        "start at login" => KeyBinding::new(keys, StartAtLogin, context),
        "install claude code hooks" => KeyBinding::new(keys, InstallHooks, context),
        "install agent guide" => KeyBinding::new(keys, InstallAgentGuide, context),
        "toggle sidebar" => KeyBinding::new(keys, ToggleSidebar, context),
        "focus file tree" => KeyBinding::new(keys, FocusFileTree, context),
        "show ignored files" => KeyBinding::new(keys, ShowIgnoredFiles, context),
        "focus worktrees" => KeyBinding::new(keys, FocusWorktrees, context),
        "toggle inline diff" => KeyBinding::new(keys, ToggleInlineDiff, context),
        "toggle markdown view" => KeyBinding::new(keys, ToggleMarkdownView, context),
        "save file" => KeyBinding::new(keys, SaveFile, context),
        "open folder" => KeyBinding::new(keys, OpenFolder, context),
        "new window" => KeyBinding::new(keys, NewWindow, context),
        "new browser tab" => KeyBinding::new(keys, NewBrowserTab, context),
        "open file in pane" => KeyBinding::new(keys, OpenFileInPane, context),
        "video play pause" => KeyBinding::new(keys, VideoPlayPause, context),
        "video back" => KeyBinding::new(keys, VideoBack, context),
        "video forward" => KeyBinding::new(keys, VideoForward, context),
        "video back fine" => KeyBinding::new(keys, VideoBackFine, context),
        "video forward fine" => KeyBinding::new(keys, VideoForwardFine, context),
        "video step back" => KeyBinding::new(keys, VideoStepBack, context),
        "video step forward" => KeyBinding::new(keys, VideoStepForward, context),
        "video start" | "video start alt" => KeyBinding::new(keys, VideoStart, context),
        "video loop" => KeyBinding::new(keys, VideoLoop, context),
        "video mute" => KeyBinding::new(keys, VideoMute, context),
        "video reload" => KeyBinding::new(keys, VideoReload, context),
        "page back" => KeyBinding::new(keys, PageBack, context),
        "page forward" => KeyBinding::new(keys, PageForward, context),
        "reload page" => KeyBinding::new(keys, ReloadPage, context),
        "focus address" => KeyBinding::new(keys, FocusAddress, context),
        "open in browser" => KeyBinding::new(keys, OpenInBrowser, context),
        "zoom reset" => KeyBinding::new(keys, ZoomReset, context),
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

/// Names of actions that no longer exist (Chrome docking, removed with the web pane): a
/// `[keybindings]` entry for one is ignored rather than refused, so a config written for 0.6
/// still loads.
pub const RETIRED: &[&str] = &["new single-page browser tab", "dock a chrome window", "undock chrome window"];

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
        if RETIRED.contains(&name) {
            continue;
        }
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
                "home" => "Home".to_string(),
                "end" => "End".to_string(),
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

/// An action's display name, as the palette and the Keybindings window write it: the config name
/// with its first letter upper-cased, except where the product names keep their capitals, and the
/// commands that ask for more end in an ellipsis, as macOS writes them.
pub fn label_for(name: &str) -> String {
    match command_for(name) {
        Some(Command::About) => "About Arkos IDE".to_string(),
        Some(Command::InstallHooks) => "Install Claude Code hooks".to_string(),
        Some(Command::OpenFolder | Command::NewBrowserTab | Command::OpenFileInPane) => format!("{}\u{2026}", label(name)),
        _ if name.starts_with("video ") => format!("Video: {}", video_label(name)),
        _ => label(name),
    }
}

/// What a video action does, as the palette's "Video: ..." row says it.
fn video_label(name: &str) -> &str {
    match name {
        "video play pause" => "play or pause",
        "video back" => "back 5 seconds",
        "video forward" => "forward 5 seconds",
        "video back fine" => "back 1 second",
        "video forward fine" => "forward 1 second",
        "video step back" => "step back a frame",
        "video step forward" => "step forward a frame",
        "video start" => "to the start",
        "video start alt" => "to the start alt",
        other => other.strip_prefix("video ").unwrap_or(other),
    }
}

/// The document viewer's fixed keys, which no `[keybindings]` entry changes: the label and the
/// keys in the Mac spelling. Each is one `viewer::key_action` maps, except Cmd+click, which the
/// rendered view's links answer (`md_view`).
pub const MARKDOWN_KEYS: &[(&str, &str)] = &[
    ("Leave the block", "Escape"),
    ("Continue a list", "Enter"),
    ("Undo", "Cmd+Z"),
    ("Redo", "Shift+Cmd+Z"),
    ("Select all", "Cmd+A"),
    ("Copy", "Cmd+C"),
    ("Cut", "Cmd+X"),
    ("Paste", "Cmd+V"),
    ("Indent a list item", "Tab"),
    ("Outdent a list item", "Shift+Tab"),
    ("Next cell (in a table)", "Tab"),
    ("Previous cell (in a table)", "Shift+Tab"),
    ("Cell above (in a table)", "Up"),
    ("Cell below (in a table)", "Down"),
    ("Open a link", "Cmd+click"),
    ("Scroll to the top or end", "Home, End"),
];

/// One line of the Keybindings window: what the action is called and its keys in the Mac
/// spelling, `None` when nothing is bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRow {
    pub label: String,
    pub keys: Option<String>,
}

/// Every action with its keys in force under `overrides`, grouped by section in display order (a
/// section with no rows is left out), in `ACTIONS` order within a section, each labelled as the
/// palette labels it (`label_for`). The nine "focus pane N" rows read as one row while every one
/// of them keeps its default; the Markdown section lists the viewer's actions, then
/// `MARKDOWN_KEYS`.
pub fn grouped(overrides: &BTreeMap<String, String>) -> Vec<(Section, Vec<KeyRow>)> {
    let focus_pane = |a: &ActionSpec| a.name.starts_with("focus pane ");
    let collapse = ACTIONS.iter().filter(|a| focus_pane(a)).all(|a| keys_for(a.name, overrides).as_deref() == Some(a.keys));
    Section::ALL
        .iter()
        .map(|&section| {
            let mut rows = Vec::new();
            for a in ACTIONS.iter().filter(|a| a.section == section) {
                if collapse && focus_pane(a) {
                    // The first of the nine stands for them all; the rest add nothing.
                    if a.name == "focus pane 1" {
                        let keys = format!("{} to {}", pretty_keys("cmd-1"), pretty_keys("cmd-9"));
                        rows.push(KeyRow { label: "Focus pane 1 to 9".to_string(), keys: Some(keys) });
                    }
                    continue;
                }
                rows.push(KeyRow { label: label_for(a.name), keys: keys_for(a.name, overrides).map(|k| pretty_keys(&k)) });
            }
            if section == Section::Markdown {
                rows.extend(MARKDOWN_KEYS.iter().map(|(label, keys)| KeyRow { label: label.to_string(), keys: Some(keys.to_string()) }));
            }
            (section, rows)
        })
        .filter(|(_, rows)| !rows.is_empty())
        .collect()
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
        "about" => Command::About,
        "settings" => Command::OpenSettings,
        "keybindings" => Command::Keybindings,
        "documentation" => Command::Documentation,
        "open config" => Command::OpenConfig,
        "find" => Command::Find,
        "copy as shown" => Command::CopyAsShown,
        "update now" => Command::UpdateNow,
        "check for updates" => Command::CheckForUpdates,
        "notifications" => Command::Notifications,
        "doctor" => Command::Doctor,
        "retry update" => Command::RetryUpdate,
        "start at login" => Command::StartAtLogin,
        "install claude code hooks" => Command::InstallHooks,
        "install agent guide" => Command::InstallAgentGuide,
        "toggle sidebar" => Command::ToggleSidebar,
        "focus file tree" => Command::FocusFileTree,
        "show ignored files" => Command::ShowIgnoredFiles,
        "focus worktrees" => Command::FocusWorktrees,
        "toggle inline diff" => Command::ToggleInlineDiff,
        "toggle markdown view" => Command::ToggleMarkdownView,
        "save file" => Command::SaveFile,
        "open folder" => Command::OpenFolder,
        "new window" => Command::NewWindow,
        "new browser tab" => Command::NewBrowserTab,
        "open file in pane" => Command::OpenFileInPane,
        "video play pause" => Command::VideoPlayPause,
        "video back" => Command::VideoBack,
        "video forward" => Command::VideoForward,
        "video back fine" => Command::VideoBackFine,
        "video forward fine" => Command::VideoForwardFine,
        "video step back" => Command::VideoStepBack,
        "video step forward" => Command::VideoStepForward,
        "video start" | "video start alt" => Command::VideoStart,
        "video loop" => Command::VideoLoop,
        "video mute" => Command::VideoMute,
        "video reload" => Command::VideoReload,
        "page back" => Command::PageBack,
        "page forward" => Command::PageForward,
        "reload page" => Command::ReloadPage,
        "focus address" => Command::FocusAddress,
        "open in browser" => Command::OpenInBrowser,
        "zoom reset" => Command::ZoomReset,
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
        $( d = d.on_action($cx.listener(|root: &mut Root, _: &$action, window: &mut Window, cx: &mut Context<Root>| {
            let _via = crate::telemetry::opened_via(crate::telemetry::Opened::Key);
            root.run($command, window, cx)
        })); )*
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
        About => Command::About, OpenSettings => Command::OpenSettings,
        Keybindings => Command::Keybindings, Documentation => Command::Documentation,
        OpenConfig => Command::OpenConfig, CommandPalette => Command::CommandPalette, Find => Command::Find, CopyAsShown => Command::CopyAsShown,
        UpdateNow => Command::UpdateNow, CheckForUpdates => Command::CheckForUpdates, Notifications => Command::Notifications, Doctor => Command::Doctor, RetryUpdate => Command::RetryUpdate, StartAtLogin => Command::StartAtLogin, InstallHooks => Command::InstallHooks, InstallAgentGuide => Command::InstallAgentGuide,
        ToggleSidebar => Command::ToggleSidebar, FocusFileTree => Command::FocusFileTree, ShowIgnoredFiles => Command::ShowIgnoredFiles,
        FocusWorktrees => Command::FocusWorktrees, ToggleInlineDiff => Command::ToggleInlineDiff,
        ToggleMarkdownView => Command::ToggleMarkdownView, SaveFile => Command::SaveFile,
        OpenFolder => Command::OpenFolder, NewWindow => Command::NewWindow,
        NewBrowserTab => Command::NewBrowserTab,
        OpenFileInPane => Command::OpenFileInPane,
        VideoPlayPause => Command::VideoPlayPause, VideoBack => Command::VideoBack, VideoForward => Command::VideoForward,
        VideoBackFine => Command::VideoBackFine, VideoForwardFine => Command::VideoForwardFine,
        VideoStepBack => Command::VideoStepBack, VideoStepForward => Command::VideoStepForward,
        VideoStart => Command::VideoStart, VideoLoop => Command::VideoLoop, VideoMute => Command::VideoMute, VideoReload => Command::VideoReload,
        PageBack => Command::PageBack, PageForward => Command::PageForward, ReloadPage => Command::ReloadPage, FocusAddress => Command::FocusAddress,
        OpenInBrowser => Command::OpenInBrowser, ZoomReset => Command::ZoomReset,
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
        ("", "about", "About"),
        ("cmd-,", "settings", "OpenSettings"),
        ("cmd-/", "keybindings", "Keybindings"),
        ("", "documentation", "Documentation"),
        ("", "open config", "OpenConfig"),
        ("cmd-shift-p", "command palette", "CommandPalette"),
        ("cmd-f", "find", "Find"),
        ("cmd-shift-c", "copy as shown", "CopyAsShown"),
        ("", "update now", "UpdateNow"),
        ("", "check for updates", "CheckForUpdates"),
        ("", "notifications", "Notifications"),
        ("", "doctor", "Doctor"),
        ("", "retry update", "RetryUpdate"),
        ("", "start at login", "StartAtLogin"),
        ("", "install claude code hooks", "InstallHooks"),
        ("", "install agent guide", "InstallAgentGuide"),
        ("cmd-b", "toggle sidebar", "ToggleSidebar"),
        ("cmd-shift-e", "focus file tree", "FocusFileTree"),
        ("", "show ignored files", "ShowIgnoredFiles"),
        ("ctrl-shift-g", "focus worktrees", "FocusWorktrees"),
        ("cmd-shift-d", "toggle inline diff", "ToggleInlineDiff"),
        ("cmd-shift-m", "toggle markdown view", "ToggleMarkdownView"),
        ("cmd-s", "save file", "SaveFile"),
        ("cmd-o", "open folder", "OpenFolder"),
        ("cmd-n", "new window", "NewWindow"),
        ("cmd-shift-b", "new browser tab", "NewBrowserTab"),
        ("", "open file in pane", "OpenFileInPane"),
        ("space", "video play pause", "VideoPlayPause"),
        ("left", "video back", "VideoBack"), ("right", "video forward", "VideoForward"),
        ("shift-left", "video back fine", "VideoBackFine"), ("shift-right", "video forward fine", "VideoForwardFine"),
        (",", "video step back", "VideoStepBack"), (".", "video step forward", "VideoStepForward"),
        ("home", "video start", "VideoStart"), ("0", "video start alt", "VideoStart"),
        ("l", "video loop", "VideoLoop"), ("m", "video mute", "VideoMute"), ("r", "video reload", "VideoReload"),
        ("cmd-[", "page back", "PageBack"), ("cmd-]", "page forward", "PageForward"), ("cmd-r", "reload page", "ReloadPage"),
        ("cmd-l", "focus address", "FocusAddress"), ("cmd-shift-o", "open in browser", "OpenInBrowser"), ("cmd-0", "zoom reset", "ZoomReset"),
        ("cmd-q", "quit", "Quit"),
    ];

    #[test]
    fn a_retired_action_in_the_config_is_ignored_not_refused() {
        let mut o = BTreeMap::new();
        o.insert("dock a chrome window".to_string(), "cmd-shift-k".to_string());
        o.insert("undock chrome window".to_string(), "".to_string());
        assert!(validate(&o).is_empty(), "{:?}", validate(&o));
        assert_eq!(bindings_for(&o).len(), bindings().len(), "nothing is bound for a retired name");
    }

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
        for name in ["toggle markdown view", "save file"] {
            assert_eq!(ACTIONS.iter().find(|a| a.name == name).unwrap().context, Some("Viewer"), "{name} acts on the focused viewer only");
        }
        let copy = ACTIONS.iter().find(|a| a.name == "copy as shown").map(|a| (a.context, a.section));
        assert_eq!(copy, Some((Some("Terminal"), Section::Terminal)), "Shift+Cmd+C copies the focused terminal's selection only");
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
        assert_eq!(b.len(), ACTIONS.len() - 14, "an empty string unbinds, and about, documentation, open config, update now, check for updates, notifications, doctor, retry update, start at login, install hooks, install agent guide, show ignored files and open file in pane have no default keys");
        let split = b.iter().find(|kb| kb.action().name() == "arkos::SplitRight").unwrap();
        assert_eq!(keys_of(split), "cmd-d");
        assert!(!b.iter().any(|kb| kb.action().name() == "arkos::FontBigger"));
        assert_eq!(keys_for("split right", &o).as_deref(), Some("cmd-d"));
        assert_eq!(keys_for("font bigger", &o), None);
        assert_eq!(keys_for("font smaller", &o).as_deref(), Some("cmd--"));
        assert_eq!(keys_for("no such action", &o), None);
        assert_eq!(keys_for("update now", &BTreeMap::new()), None, "unbound by default");
        assert_eq!(bindings().len(), ACTIONS.len() - 13, "only about, documentation, open config, update now, check for updates, notifications, doctor, retry update, start at login, install hooks, install agent guide, show ignored files and open file in pane are unbound by default");
        assert_eq!(command_for("new browser tab"), Some(Command::NewBrowserTab));
        assert_eq!(command_for("dock a chrome window"), None, "a retired action is no command");
        assert_eq!(command_for("notifications"), Some(Command::Notifications));
        assert_eq!(command_for("doctor"), Some(Command::Doctor));
    }

    #[test]
    fn a_user_s_cmd_comma_override_beats_settings() {
        use gpui::{KeyContext, Keymap, Keystroke};
        let ctx = [KeyContext::parse("Root").unwrap(), KeyContext::parse("Terminal").unwrap()];
        let k = Keystroke::parse("cmd-,").unwrap();
        let keymap = Keymap::new(bindings_for(&BTreeMap::new()));
        let (matched, _) = keymap.bindings_for_input(&[k.clone()], &ctx);
        assert_eq!(matched[0].action().name(), "arkos::OpenSettings", "by default Cmd+, opens Settings");
        let mut o = BTreeMap::new();
        o.insert("open config".to_string(), "cmd-,".to_string());
        let keymap = Keymap::new(bindings_for(&o));
        let (matched, _) = keymap.bindings_for_input(&[k], &ctx);
        assert_eq!(matched[0].action().name(), "arkos::OpenConfig", "a user's override comes later in the list and wins");
        assert_eq!(keys_for("open config", &BTreeMap::new()), None);
        assert_eq!(keys_for("settings", &BTreeMap::new()), Some("cmd-,".to_string()));
        assert_eq!(command_for("about"), Some(Command::About));
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
    fn every_action_sits_in_one_section_and_the_rows_read_as_the_mac_writes_them() {
        let g = grouped(&BTreeMap::new());
        let order: Vec<Section> = g.iter().map(|(s, _)| *s).collect();
        assert_eq!(order, Section::ALL.to_vec(), "every section, in display order");
        let mut seen = 0;
        for (section, rows) in &g {
            for row in rows {
                // The viewer's fixed keys are not actions; the viewer's three actions are.
                if *section == Section::Markdown && MARKDOWN_KEYS.iter().any(|(label, _)| *label == row.label) {
                    continue;
                }
                seen += if row.label == "Focus pane 1 to 9" { 9 } else { 1 };
            }
        }
        assert_eq!(seen, ACTIONS.len(), "every action once; the nine focus rows through the collapsed row");
        let panes = &g.iter().find(|(s, _)| *s == Section::Panes).unwrap().1;
        assert_eq!(panes[0], KeyRow { label: "Focus pane 1 to 9".into(), keys: Some("Cmd+1 to Cmd+9".into()) });
        let window = &g.iter().find(|(s, _)| *s == Section::Window).unwrap().1;
        assert!(window.iter().any(|r| r.label == "Keybindings" && r.keys.as_deref() == Some("Cmd+/")));
        assert!(window.iter().any(|r| r.label == "Documentation" && r.keys.is_none()));
        assert!(window.iter().any(|r| r.label == "About Arkos IDE" && r.keys.is_none()), "the palette's label");
        let maint = &g.iter().find(|(s, _)| *s == Section::Maintenance).unwrap().1;
        assert!(maint.iter().any(|r| r.label == "Doctor" && r.keys.is_none()));
        let md = &g.iter().find(|(s, _)| *s == Section::Markdown).unwrap().1;
        let viewer: Vec<&str> = md[..3].iter().map(|r| r.label.as_str()).collect();
        assert_eq!(viewer, ["Toggle inline diff", "Toggle markdown view", "Save file"], "the viewer's actions come first");
        assert_eq!(md.len(), 3 + MARKDOWN_KEYS.len());
        assert_eq!(md[3], KeyRow { label: "Leave the block".into(), keys: Some("Escape".into()) });
        assert!(md[3..].iter().zip(MARKDOWN_KEYS).all(|(r, (label, keys))| r.label == *label && r.keys.as_deref() == Some(*keys)));
        let table: Vec<(&str, &str)> = MARKDOWN_KEYS.iter().copied().filter(|(label, _)| label.ends_with("(in a table)")).collect();
        assert_eq!(
            table,
            [("Next cell (in a table)", "Tab"), ("Previous cell (in a table)", "Shift+Tab"), ("Cell above (in a table)", "Up"), ("Cell below (in a table)", "Down")],
            "the table keys, in order"
        );
        let mut o = BTreeMap::new();
        o.insert("find".to_string(), "cmd-shift-f".to_string());
        o.insert("focus pane 3".to_string(), "cmd-shift-3".to_string());
        let g = grouped(&o);
        let term = &g.iter().find(|(s, _)| *s == Section::Terminal).unwrap().1;
        assert!(term.iter().any(|r| r.label == "Find" && r.keys.as_deref() == Some("Shift+Cmd+F")));
        let panes = &g.iter().find(|(s, _)| *s == Section::Panes).unwrap().1;
        assert_eq!(panes.iter().filter(|r| r.label.starts_with("Focus pane ")).count(), 9, "an override on one of the nine spreads them out");
        assert_eq!(keys_for("keybindings", &BTreeMap::new()), Some("cmd-/".to_string()));
        assert_eq!(command_for("documentation"), Some(Command::Documentation));
    }

    #[test]
    fn the_sections_carry_the_window_s_headings() {
        let titles: Vec<&str> = Section::ALL.iter().map(|s| s.title()).collect();
        assert_eq!(titles, ["Window and app", "Panes", "Tabs", "Terminal", "Sidebar and files", "Browser", "Video", "Viewer and markdown editing", "Updates and maintenance"]);
    }

    #[test]
    fn every_video_action_acts_on_the_focused_video_only() {
        use gpui::{KeyContext, Keymap, Keystroke};
        let videos: Vec<&ActionSpec> = ACTIONS.iter().filter(|a| a.name.starts_with("video ")).collect();
        assert_eq!(videos.len(), 12, "the spec's eleven actions and the second key of video start");
        for a in &videos {
            assert_eq!((a.context, a.section), (Some("Video"), Section::Video), "{}", a.name);
            assert!(command_for(a.name).is_some(), "{} runs a command", a.name);
            assert!(label_for(a.name).starts_with("Video: "), "{} reads as a video command in the palette", a.name);
        }
        assert!(ACTIONS.iter().filter(|a| a.section == Section::Video).all(|a| a.name.starts_with("video ")), "the Video heading lists only the video actions");
        assert_eq!(command_for("video start alt"), command_for("video start"), "0 goes to the start as Home does");
        let keymap = Keymap::new(bindings());
        let terminal = [KeyContext::parse("Root").unwrap(), KeyContext::parse("Terminal").unwrap()];
        let video = [KeyContext::parse("Root").unwrap(), KeyContext::parse("Video").unwrap()];
        for a in &videos {
            let k = Keystroke::parse(&as_gpui(a.keys)).unwrap();
            let (matched, _) = keymap.bindings_for_input(std::slice::from_ref(&k), &terminal);
            assert!(matched.is_empty(), "{} must reach a terminal as typing, not as {:?}", a.keys, matched.iter().map(|b| b.action().name()).collect::<Vec<_>>());
            let (matched, _) = keymap.bindings_for_input(&[k], &video);
            assert_eq!(matched.len(), 1, "{} acts in a focused video", a.keys);
        }
        assert_eq!(pretty_keys("home"), "Home");
        assert_eq!(pretty_keys("shift-left"), "Shift+Left");
        assert_eq!(pretty_keys(","), ",");
    }

    #[test]
    fn an_action_s_label_reads_as_the_palette_writes_it() {
        assert_eq!(label_for("about"), "About Arkos IDE");
        assert_eq!(label_for("install claude code hooks"), "Install Claude Code hooks");
        assert_eq!(label_for("install agent guide"), "Install agent guide");
        assert_eq!(label_for("open folder"), "Open folder\u{2026}");
        assert_eq!(label_for("new browser tab"), "New browser tab\u{2026}");
        assert_eq!(label_for("keybindings"), "Keybindings");
        assert_eq!(label_for("open file in pane"), "Open file in pane\u{2026}");
        assert_eq!(label_for("video play pause"), "Video: play or pause");
        assert_eq!(label_for("video back fine"), "Video: back 1 second");
        assert_eq!(label_for("video step forward"), "Video: step forward a frame");
        assert_eq!(label_for("quit"), "Quit", "an action the palette does not list still reads by its name");
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
