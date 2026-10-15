# Arkos IDE Milestone 2b: Config, themes, text presets, palette and find. Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Arkos IDE its config file with live reload, the Navy and Navy Light themes, the three text presets, a command palette, find in the terminal, and the tab-bar overflow menu, plus the four small items milestone 2a parked, so that every colour, size and key in the window comes from a file Cyrus can edit while the app runs.

**Architecture:** `config.rs` parses `~/.config/arkos/config.toml` into a validated `Config` with line-numbered errors; `theme.rs` loads a flat colour file (two built-ins embedded) into a `Theme` that every view reads from `Root` and that each `TerminalCore` resolves cell colours against at paint time, so switching a theme repaints parsed history. A polling watcher reloads the file and `Root::apply_config` applies the parts that can change live (theme, fonts and presets, keybindings, terminal defaults); a bad edit becomes a banner and the last good config stays. The palette and the find bar share one pure `LineEdit`; find runs alacritty's own regex search with the query escaped.

**Tech Stack:** Rust stable (edition 2021), gpui 0.2.2 (exact pin, `runtime_shaders`), alacritty_terminal 0.26 (its `term::search::RegexSearch`), toml 0.8 (already in `Cargo.lock` through gpui), serde + serde_json, futures 0.3. No file-watcher crate: the watcher polls the file's mtime and size every 500 ms from the background executor, which costs one `stat` per half second and no new dependency, FSEvents thread or entitlement.

**Spec:** `docs/superpowers/specs/2026-10-13-arkos-design.md` (sections 3 "Text", 4 "Tabs", 4 keyboard rows "Clear scrollback / find / font size" and "Open config / command palette", 7, 7.1, 9, 10, 11 milestone 2, 12). Navy Light is listed under milestone 6 in section 11, but section 7.1's default appearance ("Follow the macOS appearance ... Navy for dark, Navy Light for light") needs it the moment appearance following ships, and the theme file format makes it one TOML file, so it lands here. The "size for my distance" helper and the break reminder stay in milestone 6. The sidebar (section 6) and browser (section 8) config sections are parsed and kept so the file format does not change in milestones 4 and 5, but nothing reads them yet.

## Global Constraints

- macOS only, Apple Silicon. No Linux or Windows code paths.
- `gpui = { version = "=0.2.2", features = ["runtime_shaders"] }` pinned exactly. Never bump inside this plan. If a gpui or alacritty_terminal name in this plan does not compile, the API moved: grep `~/.cargo/registry/src/*/gpui-0.2.2/src/` (its `examples/input.rs` is the reference for a hand-built text field, `examples/scrollable.rs` for `ScrollHandle`) or `~/.cargo/registry/src/*/alacritty_terminal-0.26.0/src/term/search.rs` for the method, fix the call, and note the change in the commit message. Do not guess.
- New dependency allowed: `toml = "0.8"` in `crates/arkos/Cargo.toml` (version 0.8.23 is already in the lockfile and the local registry). No `notify`, `regex`, `image` or `png` crate.
- Display name "Arkos IDE" in the window title and user-facing text; binaries, crates, the socket directory `~/Library/Application Support/arkos/` and log prefixes stay lowercase `arkos`.
- Config (spec 7): `~/.config/arkos/config.toml` (under `$XDG_CONFIG_HOME/arkos/` when that variable is set), created with commented defaults on first run, opened by Cmd+, in the default editor, watched and applied live; a bad edit shows a non-blocking banner with the line number and keeps the last good config; sections `font`, `terminal` (scrollback, shell, nice level), `keybindings`, `sidebar`, `browser` (default URL), `daemon` (watchdog timeout, replay buffer size), `theme` (name), `appearance` (follow system light/dark or fixed). Themes: one TOML file each in `~/.config/arkos/themes/`; built-ins embedded in the binary and extractable.
- Text (spec 7, 7.1): built-in presets verbatim: Compact 14 px / 1.3 ("below the research's comfort floor, so labelled as such"), Comfortable 16 px / 1.5 (default), Relaxed 18 px / 1.6. Users add their own presets in the config. Cmd+= and Cmd+- still nudge only the focused terminal's size (1 px within 10 to 32 px) without changing the preset. The active preset is saved with the workspace. Default font Menlo. Letter spacing: no option. No warm tint or blue-light mode.
- Colours (spec 7.1): every foreground colour in both built-in themes clears 4.5:1 luminance contrast against its background, the terminal foreground about 12:1 on Navy; no saturated low-luminance colours on the dark background. Appearance follows macOS by default: Navy for dark, Navy Light for light.
- Keyboard (spec 4): the bindings table in Task 1 is the spec's, verbatim, plus `cmd-,` (open config), `cmd-shift-p` (command palette), `cmd-f` (find) and `cmd-q` (quit). Every binding lives in the config file and can be changed, chords included. The lone `cmd-k` binding stays first in the generated list (gpui's pending-chord rule, milestone 2a ruling 16).
- Motion (spec 7.1): nothing new animates. The palette and find bar appear without transition. Reduce Motion is re-read on every config reload.
- Daemon: no protocol change (`PROTOCOL_VERSION` stays 1). The `[daemon]` section is passed as `--watchdog-secs` and `--replay-bytes` only when the window starts the daemon; the config file says so.
- Commit messages: imperative, one line, body optional. No em-dashes in any copy or comment. Every commit ends with a blank line, `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_015xrVD54zbvkgksJUndxfpr`.
- Tests: `cargo test -p arkos` must stay headless (no window; `gpui::Keystroke::parse` and `gpui::Keymap` are fine, they need no platform); the build stays warning-free for the project's crates; `cargo` is at `~/.cargo/bin/cargo`.

## Review Focus

1. **A typo in config.toml while the app runs.** A misspelled key, a string where a number belongs, or an unclosed quote must produce a banner naming the line, leave every setting as it was, and never close the window. Pinned by Task 1's `errors_carry_the_line_number` and `unknown_keys_are_rejected`, and Task 6's `bad_reload_keeps_the_last_good_config`.
2. **A theme name that does not exist.** `dark = "nvay"` or a deleted user theme must keep the current theme and say which name was not found. Pinned by Task 6's `missing_theme_keeps_the_current_one`.
3. **Switching themes with history on screen.** Cells parsed under Navy must repaint in Navy Light, including scrollback, because colours are resolved at paint time, not at parse time. Pinned by Task 4's `set_palette_recolours_parsed_cells`.
4. **Find with regex metacharacters.** Searching for `.`, `*`, `(` or `\` must match literally and never panic or match everything. Pinned by Task 9's `escape_makes_metacharacters_literal` and `search_is_literal_and_case_insensitive`.
5. **A keybinding override that names an action wrongly or uses a bad keystroke.** `"splitt right" = "cmd-\\"` or `"split right" = "cmd+\\"` must be rejected with the action named, and the defaults must stay bound. Pinned by Task 1's `overrides_name_actions_and_keys_exactly`.

---

## File structure

```
crates/arkos/
  Cargo.toml                   + toml = "0.8"
  themes/navy.toml             NEW: the built-in dark theme (embedded with include_str!)
  themes/navy-light.toml       NEW: the built-in light theme
  src/
    config.rs                  NEW: Config and its sections, DEFAULT_CONFIG text, parse with line numbers, load-or-create
    config_watch.rs            NEW: polls the config file's fingerprint and asks Root to reload
    theme.rs                   NEW: Theme (flat named colours), built-ins, user themes, extraction, Theme::palette()
    palette.rs                 Rgb gains serde ("#rrggbb"), gpui conversions; Palette gains selection and find colours
    paths.rs                   config_dir(), config_file(), themes_dir()
    keymap.rs                  ACTIONS table (name, default keys, context); bindings_for(overrides); validate; pretty_keys; new Commands
    motion.rs                  refresh() re-reads Reduce Motion
    line_edit.rs               NEW: pure single-line editor shared by the palette and find
    command_palette.rs         NEW: items, scoring, filtering, the overlay view
    find.rs                    NEW: FindState, the find bar view
    terminal.rs                TerminalCore: set_palette, search, reveal_line, highlights; Terminal: line_height, find
    terminal_element.rs        font family and line height as parameters; paints find highlights
    chrome.rs, pane_view.rs, split_view.rs, empty_pane.rs, drag.rs, main.rs
                               colours from the Theme; overflow menu; banners for config errors
    sessions.rs                scrollback, shell and nice from config; silent "no session" on Kill
    workspace.rs               v3: text_preset
    main.rs                    Root: config, theme, presets, palette, config reload; main loads config before the window
  README.md                    milestone 2b status, keyboard rows, config section, human checklist
docs/superpowers/specs/2026-10-13-arkos-design.md   three corrections from milestone 1
scripts/sample-png.py          NEW: stdlib-only PNG colour sampler for docs/reference/
```

Responsibilities stay one per file. `config.rs`, `theme.rs`, `line_edit.rs`, `command_palette.rs` (its scoring) and `keymap.rs` (its tables) have no window in their tested surface. Views never read the config directly: `Root` applies it and holds what views need.

---

### Task 1: The actions table, keybinding overrides, Cmd+, and the config paths

**Files:**
- Modify: `crates/arkos/src/keymap.rs` (whole file), `crates/arkos/src/paths.rs` (append), `crates/arkos/src/main.rs` (`actions!`, `cx.bind_keys`, `Command::OpenConfig` arm)
- Test: unit tests in `keymap.rs` and `paths.rs`

**Interfaces:**
- Consumes: `Root::run(Command, ..)` in `main.rs:377`, `Root::status`.
- Produces:
  - `keymap::ActionSpec { name: &'static str, keys: &'static str, context: Option<&'static str> }` and `keymap::ACTIONS: &[ActionSpec]`, the spec's table plus `open config` (`cmd-,`) and `quit` (`cmd-q`), with `clear scrollback` (`cmd-k`, context `Terminal`) first.
  - `keymap::validate(overrides: &BTreeMap<String, String>) -> Vec<String>`: one message per unknown action name or unparsable keystroke; empty when fine.
  - `keymap::bindings_for(overrides: &BTreeMap<String, String>) -> Vec<KeyBinding>`: the defaults with overrides applied, in `ACTIONS` order; `""` unbinds; unknown names and bad keystrokes are skipped (callers validate first).
  - `keymap::bindings() -> Vec<KeyBinding>`: `bindings_for` with no overrides (kept for `main.rs` until Task 5).
  - `keymap::keys_for(name: &str, overrides: &BTreeMap<String, String>) -> Option<String>`: the effective keystrokes of an action, `None` when unbound or unknown.
  - `keymap::Quit` (moved from `main.rs`) and `keymap::OpenConfig` actions; `Command::OpenConfig`.
  - `paths::config_dir() -> PathBuf`, `paths::config_file() -> PathBuf` (`config.toml`), `paths::themes_dir() -> PathBuf` (`themes`), honouring a non-empty `$XDG_CONFIG_HOME`, else `~/.config/arkos`; `paths::config_dir_from(xdg: Option<&OsStr>, home: &Path) -> PathBuf` for tests.
  - `Root::open_config(&mut self, cx)`: runs `open -t <config file>`; a spawn failure goes in the status banner.

- [ ] **Step 1: Write the failing tests**

Replace the `tests` module at the bottom of `crates/arkos/src/keymap.rs` with:

```rust
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
        assert_eq!(b.len(), ACTIONS.len() - 1, "an empty string unbinds");
        let split = b.iter().find(|kb| kb.action().name() == "arkos::SplitRight").unwrap();
        assert_eq!(keys_of(split), "cmd-d");
        assert!(!b.iter().any(|kb| kb.action().name() == "arkos::FontBigger"));
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
        assert_eq!(matched[0].action().name(), "arkos::ToggleMaximize");
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
```

Append to the `tests` module in `crates/arkos/src/paths.rs`:

```rust
    #[test]
    fn config_dir_honours_xdg_config_home() {
        use std::ffi::OsStr;
        let home = Path::new("/Users/me");
        assert_eq!(config_dir_from(None, home), PathBuf::from("/Users/me/.config/arkos"));
        assert_eq!(config_dir_from(Some(OsStr::new("")), home), PathBuf::from("/Users/me/.config/arkos"));
        assert_eq!(config_dir_from(Some(OsStr::new("/tmp/xdg")), home), PathBuf::from("/tmp/xdg/arkos"));
        assert!(config_file().ends_with("arkos/config.toml"));
        assert!(themes_dir().ends_with("arkos/themes"));
    }
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `~/.cargo/bin/cargo test -p arkos keymap paths 2>&1 | tail -20`
Expected: compile errors (`ACTIONS`, `validate`, `bindings_for`, `keys_for`, `config_dir_from` not found).

- [ ] **Step 3: Rewrite keymap.rs around the table**

Replace everything above the `tests` module in `crates/arkos/src/keymap.rs` with:

```rust
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
        if let Err(e) = Keystroke::parse(token) {
            return Some(format!("cannot parse keystroke \"{token}\": {e}"));
        }
    }
    None
}

/// Every problem with an override table, in table order: an action name that is not in
/// `ACTIONS`, or keystrokes gpui cannot parse. Empty when the table is fine.
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
/// are skipped; callers show `validate`'s messages and keep the last good bindings.
pub fn bindings_for(overrides: &BTreeMap<String, String>) -> Vec<KeyBinding> {
    ACTIONS
        .iter()
        .filter_map(|spec| {
            let keys = keys_for(spec.name, overrides)?;
            if keystroke_error(&keys).is_some() {
                return None;
            }
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
```

`Command` is no longer `Copy` (later tasks add a `String` variant); the `route!` macro builds the value inside each closure, so nothing captures it.

- [ ] **Step 4: Add the config paths**

Append to `crates/arkos/src/paths.rs` above the `tests` module:

```rust
fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").expect("HOME is set"))
}

/// `$XDG_CONFIG_HOME/arkos` when that is set and non-empty, else `~/.config/arkos` (spec 7).
pub fn config_dir_from(xdg: Option<&std::ffi::OsStr>, home: &Path) -> PathBuf {
    match xdg {
        Some(x) if !x.is_empty() => PathBuf::from(x).join("arkos"),
        _ => home.join(".config").join("arkos"),
    }
}

pub fn config_dir() -> PathBuf {
    config_dir_from(std::env::var_os("XDG_CONFIG_HOME").as_deref(), &home())
}

pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn themes_dir() -> PathBuf {
    config_dir().join("themes")
}
```

and change `app_support()` to use `home()`:

```rust
fn app_support() -> PathBuf {
    home().join("Library/Application Support/arkos")
}
```

- [ ] **Step 5: Wire Quit and OpenConfig in main.rs**

In `crates/arkos/src/main.rs`:

1. Delete `actions!(arkos, [Quit]);` (line 49) and the `actions,` and `KeyBinding,` imports from the `gpui` use list if nothing else uses them (`KeyBinding` is no longer used there).
2. Add `use keymap::Quit;` after the `use workspace::...` line.
3. Replace the two lines `cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);` and `cx.bind_keys(keymap::bindings());` with the single `cx.bind_keys(keymap::bindings());` (Quit is in the table now).
4. In `Root::run`, add an arm after `FontDelta(d) => { ... }`:

```rust
            OpenConfig => self.open_config(cx),
```

5. Add to `impl Root`, after `end_closed_tabs`:

```rust
    /// Cmd+, (spec 7): opens the config file in the default text editor. Task 2 creates the file
    /// at launch, so it exists by the time this runs.
    pub fn open_config(&mut self, cx: &mut Context<Self>) {
        let path = paths::config_file();
        if let Err(e) = std::process::Command::new("open").arg("-t").arg(&path).spawn() {
            self.status = format!("cannot open {}: {e}", path.display());
            eprintln!("arkos: {}", self.status);
            cx.notify();
        }
    }
```

- [ ] **Step 6: Run the tests**

Run: `~/.cargo/bin/cargo test -p arkos 2>&1 | tail -15`
Expected: all pass, including the five keymap tests and `config_dir_honours_xdg_config_home`. `~/.cargo/bin/cargo build -p arkos 2>&1 | grep -c warning` prints 0.

- [ ] **Step 7: Commit**

```bash
git add crates/arkos/src/keymap.rs crates/arkos/src/paths.rs crates/arkos/src/main.rs
git commit -m "Name every key binding and accept overrides by name"
```

---

### Task 2: The config file: types, defaults, parsing with line numbers, create on first run

**Files:**
- Modify: `crates/arkos/Cargo.toml` (add `toml = "0.8"`), `crates/arkos/src/main.rs` (`mod config;`)
- Create: `crates/arkos/src/config.rs`
- Test: unit tests in `config.rs`

**Interfaces:**
- Consumes: `keymap::validate`, `paths::config_file`.
- Produces:
  - `config::Config { font: FontConfig, terminal: TerminalConfig, keybindings: BTreeMap<String, String>, sidebar: SidebarConfig, browser: BrowserConfig, daemon: DaemonConfig, theme: ThemeConfig, appearance: AppearanceConfig }`, `Default`, `Clone`, `PartialEq`, serde with `deny_unknown_fields` on every struct.
  - `config::FontConfig { family: String, preset: String, presets: BTreeMap<String, TextPreset> }`; `config::TextPreset { size: f32, line_height: f32 }`.
  - `config::TerminalConfig { scrollback: usize, shell: Option<String>, nice: i32 }`.
  - `config::SidebarConfig { width: f32, collapsed: bool }`, `config::BrowserConfig { url: String }` (parsed for milestones 4 and 5; `#[allow(dead_code)]`).
  - `config::DaemonConfig { watchdog_secs: u64, replay_bytes: usize }`.
  - `config::ThemeConfig { dark: String, light: String }`; `config::AppearanceConfig { mode: AppearanceMode }`; `config::AppearanceMode { System, Dark, Light }` (serde lowercase).
  - `config::ConfigError { line: Option<usize>, message: String }` with `Display` as `config.toml line N: message` or `config.toml: message`.
  - `config::parse(text: &str) -> Result<Config, ConfigError>`: TOML errors carry the 1-based line of the span's start; semantic errors (`Config::validate`) carry no line.
  - `config::Config::validate(&self) -> Result<(), String>`.
  - `config::load(path: &Path) -> Result<Config, ConfigError>`: missing file is written from `DEFAULT_CONFIG` (parents created) and the defaults returned; an unwritable path still returns the defaults and logs; unreadable or invalid content is an error.
  - `config::DEFAULT_CONFIG: &str`: the commented file written on first run; parses to `Config::default()`.
  - `config::BUILTIN_PRESETS: [(&str, TextPreset); 3]`: compact, comfortable, relaxed.

Why the `theme` section has two names where the spec says "theme (name)": section 7.1 makes following the macOS appearance the default, which needs a theme for each appearance. `dark` and `light` are those two; `appearance.mode = "dark"` or `"light"` picks one for good.

- [ ] **Step 1: Write the failing tests**

Create `crates/arkos/src/config.rs` with only a `tests` module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_file_parses_to_the_defaults() {
        let cfg = parse(DEFAULT_CONFIG).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.font.family, "Menlo");
        assert_eq!(cfg.font.preset, "comfortable");
        assert_eq!(cfg.font.presets["compact"], TextPreset { size: 14.0, line_height: 1.3 });
        assert_eq!(cfg.font.presets["comfortable"], TextPreset { size: 16.0, line_height: 1.5 });
        assert_eq!(cfg.font.presets["relaxed"], TextPreset { size: 18.0, line_height: 1.6 });
        assert_eq!(cfg.terminal.scrollback, 20_000);
        assert_eq!(cfg.terminal.shell, None);
        assert_eq!(cfg.terminal.nice, 0);
        assert!(cfg.keybindings.is_empty());
        assert_eq!(cfg.daemon.watchdog_secs, 30);
        assert_eq!(cfg.daemon.replay_bytes, 16 * 1024 * 1024);
        assert_eq!(cfg.theme.dark, "navy");
        assert_eq!(cfg.theme.light, "navy-light");
        assert_eq!(cfg.appearance.mode, AppearanceMode::System);
    }

    #[test]
    fn the_default_file_documents_every_action() {
        for a in crate::keymap::ACTIONS {
            // A TOML basic string writes one backslash as two.
            let line = format!("# \"{}\" = \"{}\"", a.name, a.keys.replace('\\', "\\\\"));
            assert!(DEFAULT_CONFIG.contains(&line), "DEFAULT_CONFIG lacks the line {line:?}");
        }
        assert!(DEFAULT_CONFIG.contains("csarko.sh/research/check-your-eyes-before-your-theme"));
        assert!(DEFAULT_CONFIG.contains("convergence"), "the eye-examination note (spec 7.1)");
    }

    #[test]
    fn errors_carry_the_line_number() {
        let text = "[font]\nfamily = \"Menlo\"\n\n[terminal]\nscrollback = \"lots\"\n";
        let e = parse(text).unwrap_err();
        assert_eq!(e.line, Some(5), "{e}");
        assert!(e.to_string().starts_with("config.toml line 5: "), "{e}");
        let e = parse("[font\nfamily = 1").unwrap_err();
        assert_eq!(e.line, Some(1), "{e}");
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let e = parse("[font]\nfamly = \"Menlo\"\n").unwrap_err();
        assert_eq!(e.line, Some(2), "{e}");
        assert!(e.message.contains("famly"), "{e}");
        let e = parse("[fonts]\nfamily = \"Menlo\"\n").unwrap_err();
        assert!(e.message.contains("fonts"), "{e}");
    }

    #[test]
    fn semantic_errors_name_the_setting() {
        let e = parse("[font]\npreset = \"huge\"\n").unwrap_err();
        assert_eq!(e.line, None);
        assert!(e.message.contains("huge"), "{e}");
        let e = parse("[font.presets.tiny]\nsize = 4.0\nline_height = 1.5\n").unwrap_err();
        assert!(e.message.contains("tiny") && e.message.contains("size"), "{e}");
        let e = parse("[font.presets.flat]\nsize = 16.0\nline_height = 0.9\n").unwrap_err();
        assert!(e.message.contains("flat") && e.message.contains("line_height"), "{e}");
        let e = parse("[terminal]\nnice = 40\n").unwrap_err();
        assert!(e.message.contains("nice"), "{e}");
        let e = parse("[terminal]\nscrollback = 0\n").unwrap_err();
        assert!(e.message.contains("scrollback"), "{e}");
        let e = parse("[daemon]\nwatchdog_secs = 1\n").unwrap_err();
        assert!(e.message.contains("watchdog_secs"), "{e}");
        let e = parse("[keybindings]\n\"splitt right\" = \"cmd-d\"\n").unwrap_err();
        assert!(e.message.contains("splitt right"), "{e}");
        let e = parse("[appearance]\nmode = \"auto\"\n").unwrap_err();
        assert_eq!(e.line, Some(2), "{e}");
    }

    #[test]
    fn a_user_preset_and_overrides_round_trip() {
        let text = "[font]\npreset = \"mine\"\n[font.presets.mine]\nsize = 17.0\nline_height = 1.45\n[keybindings]\n\"split right\" = \"cmd-d\"\n[terminal]\nshell = \"/bin/bash\"\n";
        let cfg = parse(text).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(cfg.font.preset, "mine");
        assert_eq!(cfg.font.presets["mine"], TextPreset { size: 17.0, line_height: 1.45 });
        assert_eq!(cfg.font.presets.len(), 4, "the built-ins stay");
        assert_eq!(cfg.keybindings["split right"], "cmd-d");
        assert_eq!(cfg.terminal.shell.as_deref(), Some("/bin/bash"));
        let back = toml::to_string(&cfg).unwrap();
        assert_eq!(parse(&back).unwrap(), cfg);
    }

    #[test]
    fn load_creates_the_file_once_and_reads_it_after() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("deep").join("config.toml");
        let cfg = load(&file).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(cfg, Config::default());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), DEFAULT_CONFIG);
        std::fs::write(&file, "[font]\nfamily = \"Monaco\"\n").unwrap();
        assert_eq!(load(&file).unwrap().font.family, "Monaco");
        std::fs::write(&file, "[font]\nfamily = \n").unwrap();
        let e = load(&file).unwrap_err();
        assert_eq!(e.line, Some(2), "{e}");
    }

    #[test]
    fn load_survives_an_unwritable_location() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        // The folder cannot be written: the defaults still come back so the window can open.
        let file = locked.join("config.toml");
        assert_eq!(load(&file).unwrap(), Config::default());
        assert!(!file.exists());
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
```

Add `mod config;` to `main.rs` (alphabetical, after `mod closed_stack;`) and `toml = "0.8"` under `[dependencies]` in `crates/arkos/Cargo.toml`.

- [ ] **Step 2: Run the tests to see them fail**

Run: `~/.cargo/bin/cargo test -p arkos config 2>&1 | tail -20`
Expected: compile errors (`parse`, `Config`, `DEFAULT_CONFIG` not found).

- [ ] **Step 3: Write the config module**

Put this above the `tests` module in `crates/arkos/src/config.rs`:

```rust
//! The config file (spec 7): `~/.config/arkos/config.toml`, created with commented defaults on
//! first run, parsed here with line-numbered errors. `config_watch.rs` notices edits and
//! `Root::apply_config` applies them; nothing here touches the window.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub font: FontConfig,
    pub terminal: TerminalConfig,
    /// Action name (see `keymap::ACTIONS`) to keystrokes in gpui syntax. `""` unbinds.
    pub keybindings: BTreeMap<String, String>,
    pub sidebar: SidebarConfig,
    pub browser: BrowserConfig,
    pub daemon: DaemonConfig,
    pub theme: ThemeConfig,
    pub appearance: AppearanceConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextPreset {
    pub size: f32,
    pub line_height: f32,
}

/// Spec 7's table, verbatim.
pub const BUILTIN_PRESETS: [(&str, TextPreset); 3] = [
    ("compact", TextPreset { size: 14.0, line_height: 1.3 }),
    ("comfortable", TextPreset { size: 16.0, line_height: 1.5 }),
    ("relaxed", TextPreset { size: 18.0, line_height: 1.6 }),
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FontConfig {
    pub family: String,
    /// The preset a workspace starts with; each workspace remembers the one it last used.
    pub preset: String,
    /// The built-ins plus the user's; a user entry with a built-in name replaces it.
    #[serde(deserialize_with = "merge_presets")]
    pub presets: BTreeMap<String, TextPreset>,
}

fn builtin_presets() -> BTreeMap<String, TextPreset> {
    BUILTIN_PRESETS.iter().map(|(n, p)| (n.to_string(), *p)).collect()
}

/// A `[font.presets.*]` table in the file adds to the built-ins instead of replacing them, so
/// one custom preset does not make "comfortable" disappear.
fn merge_presets<'de, D: serde::Deserializer<'de>>(d: D) -> Result<BTreeMap<String, TextPreset>, D::Error> {
    let user: BTreeMap<String, TextPreset> = BTreeMap::deserialize(d)?;
    let mut all = builtin_presets();
    all.extend(user);
    Ok(all)
}

impl Default for FontConfig {
    fn default() -> Self {
        Self { family: "Menlo".into(), preset: "comfortable".into(), presets: builtin_presets() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {
    pub scrollback: usize,
    /// The program each new tab runs; `None` means `$SHELL`.
    pub shell: Option<String>,
    pub nice: i32,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self { scrollback: 20_000, shell: None, nice: 0 }
    }
}

/// Milestone 4 reads this; parsed now so the file format does not change then.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SidebarConfig {
    pub width: f32,
    pub collapsed: bool,
}

impl Default for SidebarConfig {
    fn default() -> Self {
        Self { width: 260.0, collapsed: false }
    }
}

/// Milestone 5 reads this; parsed now so the file format does not change then.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BrowserConfig {
    pub url: String,
}

/// Passed to arkosd when the window starts it; a running daemon keeps its own values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DaemonConfig {
    pub watchdog_secs: u64,
    pub replay_bytes: usize,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self { watchdog_secs: 30, replay_bytes: 16 * 1024 * 1024 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeConfig {
    pub dark: String,
    pub light: String,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self { dark: "navy".into(), light: "navy-light".into() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppearanceMode {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppearanceConfig {
    pub mode: AppearanceMode,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font: FontConfig::default(),
            terminal: TerminalConfig::default(),
            keybindings: BTreeMap::new(),
            sidebar: SidebarConfig::default(),
            browser: BrowserConfig::default(),
            daemon: DaemonConfig::default(),
            theme: ThemeConfig::default(),
            appearance: AppearanceConfig::default(),
        }
    }
}

impl Config {
    /// Rules the types cannot express. The message names the setting so the banner is useful.
    pub fn validate(&self) -> Result<(), String> {
        if self.font.family.trim().is_empty() {
            return Err("font.family is empty".into());
        }
        if !self.font.presets.contains_key(&self.font.preset) {
            return Err(format!("font.preset \"{}\" is not one of the presets", self.font.preset));
        }
        for (name, p) in &self.font.presets {
            if !(8.0..=48.0).contains(&p.size) || !p.size.is_finite() {
                return Err(format!("font.presets.{name}.size {} must be 8 to 48", p.size));
            }
            if !(1.0..=2.5).contains(&p.line_height) || !p.line_height.is_finite() {
                return Err(format!("font.presets.{name}.line_height {} must be 1.0 to 2.5", p.line_height));
            }
        }
        if self.terminal.scrollback == 0 || self.terminal.scrollback > 1_000_000 {
            return Err(format!("terminal.scrollback {} must be 1 to 1000000", self.terminal.scrollback));
        }
        if !(-20..=20).contains(&self.terminal.nice) {
            return Err(format!("terminal.nice {} must be -20 to 20", self.terminal.nice));
        }
        if self.daemon.watchdog_secs < 5 {
            return Err(format!("daemon.watchdog_secs {} must be at least 5", self.daemon.watchdog_secs));
        }
        if self.daemon.replay_bytes == 0 {
            return Err("daemon.replay_bytes must be at least 1".into());
        }
        if self.theme.dark.trim().is_empty() || self.theme.light.trim().is_empty() {
            return Err("theme.dark and theme.light must name a theme".into());
        }
        let errors = crate::keymap::validate(&self.keybindings);
        if !errors.is_empty() {
            return Err(errors.join("; "));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    /// 1-based, for TOML errors; semantic errors have none.
    pub line: Option<usize>,
    pub message: String,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(n) => write!(f, "config.toml line {n}: {}", self.message),
            None => write!(f, "config.toml: {}", self.message),
        }
    }
}

impl std::error::Error for ConfigError {}

/// The 1-based line holding byte `offset` of `text`.
fn line_of(text: &str, offset: usize) -> usize {
    text.as_bytes().iter().take(offset.min(text.len())).filter(|b| **b == b'\n').count() + 1
}

pub fn parse(text: &str) -> Result<Config, ConfigError> {
    let cfg: Config = toml::from_str(text).map_err(|e| ConfigError {
        line: e.span().map(|s| line_of(text, s.start)),
        // toml's message already names the key and what it expected; keep its first line.
        message: e.message().lines().next().unwrap_or("invalid TOML").to_string(),
    })?;
    cfg.validate().map_err(|message| ConfigError { line: None, message })?;
    Ok(cfg)
}

/// Reads and parses `path`. A missing file is created from `DEFAULT_CONFIG` first; if that
/// fails (an unwritable location), the defaults still come back and the failure is logged, so
/// the window always opens.
pub fn load(path: &Path) -> Result<Config, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let written = path.parent().map(std::fs::create_dir_all).unwrap_or(Ok(())).and_then(|()| std::fs::write(path, DEFAULT_CONFIG));
            match written {
                Ok(()) => eprintln!("arkos: wrote default config to {}", path.display()),
                Err(e) => eprintln!("arkos: cannot write default config to {}: {e}", path.display()),
            }
            Ok(Config::default())
        }
        Err(e) => Err(ConfigError { line: None, message: format!("cannot read {}: {e}", path.display()) }),
    }
}

/// Written on first run. Every key is present with its default so an edit is a one-word change;
/// the comments carry spec 7.1's evidence notes, each a preference, never a health feature.
pub const DEFAULT_CONFIG: &str = r##"# Arkos IDE configuration. Watched while the app runs: a saved edit applies at once, and a bad
# edit shows a banner with the line number while the last good settings stay in force.
#
# The defaults follow "Check your eyes before your theme"
# (https://csarko.sh/research/check-your-eyes-before-your-theme). Each is a preference with its
# evidence status, not a health feature. If headaches persist, the literature puts a comprehensive
# eye examination that tests convergence and small astigmatism, not just acuity, ahead of any
# setting here. See a physician first for a headache that is sudden and severe, that comes with
# fever, a stiff neck, weakness, numbness, confusion or vision loss, that wakes you from sleep,
# or that is new after 50.

[font]
# Any monospace font installed on this Mac. No controlled study favours a named coding font.
family = "Menlo"
# The text preset new workspaces start with; each workspace remembers the one it last used.
# "Text preset" in the command palette (Cmd+Shift+P) switches live. Cmd+= and Cmd+- nudge the
# focused terminal only and do not change the preset.
preset = "comfortable"

# Size in px, line height as a multiple of the size. 16 px keeps x-height near 0.25 degrees at
# 60 cm on a laptop; reading speed collapses below 0.2 degrees. Line height 1.5 moves the row
# pitch off the peak of the pattern-glare curve; 1.0 to 1.2 is not offered. Add your own presets.
# "compact" is below the research's comfort floor; it is here for a 2x2 grid that needs columns.
[font.presets.compact]
size = 14.0
line_height = 1.3

[font.presets.comfortable]
size = 16.0
line_height = 1.5

[font.presets.relaxed]
size = 18.0
line_height = 1.6

[terminal]
# Lines of history kept per tab. Memory grows with it: about 3 MB per 1,000 lines at 124 columns.
scrollback = 20000
# The program each new tab runs. Unset means $SHELL.
# shell = "/bin/zsh"
# Priority for new shells, -20 (highest) to 20 (lowest). 0 is normal.
nice = 0

[keybindings]
# Action name = keystrokes, in gpui syntax: modifiers cmd, ctrl, alt and shift joined with "-"
# before the key, as in "cmd-shift-p"; chords are separated by a space, as in "cmd-k cmd-m".
# An empty string "" unbinds an action. The defaults are VS Code's for the editor area:
# "clear scrollback" = "cmd-k"
# "focus pane 1" = "cmd-1"
# "focus pane 2" = "cmd-2"
# "focus pane 3" = "cmd-3"
# "focus pane 4" = "cmd-4"
# "focus pane 5" = "cmd-5"
# "focus pane 6" = "cmd-6"
# "focus pane 7" = "cmd-7"
# "focus pane 8" = "cmd-8"
# "focus pane 9" = "cmd-9"
# "focus left" = "cmd-k cmd-left"
# "focus right" = "cmd-k cmd-right"
# "focus up" = "cmd-k cmd-up"
# "focus down" = "cmd-k cmd-down"
# "split right" = "cmd-\\"
# "split down" = "cmd-k cmd-\\"
# "maximize pane" = "cmd-k cmd-m"
# "move tab to previous pane" = "ctrl-cmd-left"
# "move tab to next pane" = "ctrl-cmd-right"
# "next tab" = "alt-cmd-right"
# "previous tab" = "alt-cmd-left"
# "next tab alt" = "cmd-shift-]"
# "previous tab alt" = "cmd-shift-["
# "most recently used tab" = "ctrl-tab"
# "close tab" = "cmd-w"
# "reopen closed tab" = "cmd-shift-t"
# "close all in pane" = "cmd-k w"
# "move tab left" = "cmd-k cmd-shift-left"
# "move tab right" = "cmd-k cmd-shift-right"
# "new terminal tab" = "ctrl-shift-`"
# "font bigger" = "cmd-="
# "font smaller" = "cmd--"
# "open config" = "cmd-,"
# "quit" = "cmd-q"

[sidebar]
# Milestone 4. Width in px and whether it starts collapsed.
width = 260.0
collapsed = false

[browser]
# Milestone 5. The URL a new browser pane opens. Empty opens a blank page.
url = ""

[daemon]
# Read when the window starts arkosd. A daemon that is already running keeps its values until
# `pkill -x arkosd`, which ends every session.
# Seconds without a heartbeat from the window before the daemon relaunches it.
watchdog_secs = 30
# Bytes of output kept per session, replayed when a window reattaches.
replay_bytes = 16777216

[theme]
# Theme files live in ~/.config/arkos/themes/<name>.toml. "navy" and "navy-light" are built in;
# "Extract built-in themes" in the command palette writes them there to copy from.
dark = "navy"
light = "navy-light"

[appearance]
# "system" follows macOS light and dark: the evidence points at matching the room, which the
# system setting already tracks. "dark" or "light" fixes one. No warm tint or blue-light mode is
# offered: both fail their controlled trials.
mode = "system"
"##;
```

Note the `r##"..."##` string: the file contains `"` and `#` on the same lines, so the delimiter is two hashes. The `# "split right" = "cmd-\\"` lines contain a doubled backslash in the generated file because a TOML basic string writes one backslash as two; inside the Rust raw string that is two backslash characters, as written above, which is why the test escapes `a.keys` before comparing.

- [ ] **Step 4: Run the tests**

Run: `~/.cargo/bin/cargo test -p arkos config 2>&1 | tail -15`
Expected: all eight pass. If `errors_carry_the_line_number` reports line 4 instead of 5 for the `scrollback = "lots"` case, toml placed the span at the value; both are on line 5, so inspect `e.span()` with a `dbg!` and adjust `line_of` only if the span start is before the newline.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/Cargo.toml Cargo.lock crates/arkos/src/config.rs crates/arkos/src/main.rs
git commit -m "Parse config.toml with line-numbered errors and write it on first run"
```

---

### Task 3: Themes: the file format, Navy and Navy Light, loading and extraction

**Files:**
- Create: `crates/arkos/src/theme.rs`, `crates/arkos/themes/navy.toml`, `crates/arkos/themes/navy-light.toml`, `scripts/sample-png.py`
- Modify: `crates/arkos/src/palette.rs` (serde and gpui conversions for `Rgb`, three new `Palette` fields, test helpers made `pub`), `crates/arkos/src/main.rs` (`mod theme;`)
- Test: unit tests in `theme.rs` and `palette.rs`

**Interfaces:**
- Consumes: `palette::{Rgb, Palette}`.
- Produces:
  - `palette::Rgb::parse(s: &str) -> Result<Rgb, String>` (`#rrggbb`, case-insensitive), `Rgb::hex(self) -> String`, `Rgb::gpui(self) -> gpui::Rgba`, `Rgb::hsla(self) -> gpui::Hsla`, `Rgb::alpha(self, a: f32) -> gpui::Hsla`; `Serialize`/`Deserialize` as the hex string.
  - `palette::Palette` gains `selection: Rgb`, `find_match: Rgb`, `find_current: Rgb`; `palette::NAVY` keeps its values and gains `selection = #2a3a55`, `find_match = #3a4556`, `find_current = #6b5420`.
  - `palette::{luminance, contrast}` become `pub` (used by theme tests).
  - `theme::Theme { name: String, chrome: Chrome, terminal: TerminalColors, semantic: Semantic, tree: TreeColors, viewer: ViewerColors }`, every field an `Rgb`, every struct `deny_unknown_fields`, `Clone`, `PartialEq`, `Debug`.
  - `theme::Chrome { window_bg, title_bar, status_bar, sidebar, tab_bar, tab_active, tab_inactive, border, split_handle, split_handle_hover, focus_ring, text, text_dim, banner }`.
  - `theme::TerminalColors { background, foreground, cursor, selection, black, red, green, yellow, blue, magenta, cyan, white, bright_black, bright_red, bright_green, bright_yellow, bright_blue, bright_magenta, bright_cyan, bright_white }`.
  - `theme::Semantic { working, waiting, bell, error_banner, find_match, find_current }`; `theme::TreeColors { added, modified, deleted, untracked, ignored }`; `theme::ViewerColors { keyword, string, comment, number, function, type_name, constant, operator, punctuation }`.
  - `theme::BUILTIN: [(&str, &str); 2]` (name, TOML text) for `navy` and `navy-light`; `theme::builtin(name: &str) -> Option<Theme>`; `theme::parse(text: &str) -> Result<Theme, String>`; `theme::load(name: &str, dir: &Path) -> Result<Theme, String>` (a user file `dir/<name>.toml` wins over a built-in; names with `/` or `..` are refused); `theme::names(dir: &Path) -> Vec<String>` (built-ins plus user files, sorted, deduplicated); `theme::extract(dir: &Path) -> std::io::Result<Vec<PathBuf>>` (writes each built-in that is not already there, returns what it wrote).
  - `Theme::palette(&self) -> Palette`; `Theme::is_light(&self) -> bool` (window background luminance above 0.5).

The spec says the Navy values are sampled from the VS Code screenshot in `docs/reference/`. Three 2x captures are there: `vscode-full.png` (the whole window without shadow, 2940x1846), `vscode-terminals.png` (two terminal panes with tab bars, 3032x2070, with a window shadow and rounded corners) and `vscode-empty.png`. Step 5 samples `vscode-full.png` for the chrome and `vscode-terminals.png` for the tab bar and terminal background.

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module of `crates/arkos/src/palette.rs`:

```rust
    #[test]
    fn rgb_parses_and_prints_hex() {
        assert_eq!(Rgb::parse("#11161f"), Ok(rgb(0x11161f)));
        assert_eq!(Rgb::parse("#D6DAE0"), Ok(rgb(0xd6dae0)));
        assert!(Rgb::parse("11161f").is_err(), "the hash is required");
        assert!(Rgb::parse("#11161").is_err());
        assert!(Rgb::parse("#gg161f").is_err());
        assert_eq!(rgb(0x11161f).hex(), "#11161f");
        let json = serde_json::to_string(&rgb(0x0a0e14)).unwrap();
        assert_eq!(json, "\"#0a0e14\"");
        assert_eq!(serde_json::from_str::<Rgb>("\"#0A0E14\"").unwrap(), rgb(0x0a0e14));
        assert!(serde_json::from_str::<Rgb>("\"navy\"").is_err());
        let c = rgb(0xff0000).gpui();
        assert!((c.r - 1.0).abs() < 1e-6 && c.g == 0.0 && c.b == 0.0 && c.a == 1.0);
        assert!((rgb(0xff0000).alpha(0.5).a - 0.5).abs() < 1e-6);
    }
```

Create `crates/arkos/src/theme.rs` with only this `tests` module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::{contrast, rgb};

    /// Every colour drawn as text or a mark, with the background it sits on. Black is a
    /// background by convention on a dark theme and a text colour on a light one.
    fn foregrounds(t: &Theme) -> Vec<(String, Rgb, Rgb)> {
        let mut v = vec![
            ("chrome.text on window_bg".to_string(), t.chrome.text, t.chrome.window_bg),
            ("chrome.text_dim on window_bg".to_string(), t.chrome.text_dim, t.chrome.window_bg),
            ("chrome.text on title_bar".to_string(), t.chrome.text, t.chrome.title_bar),
            ("chrome.text_dim on status_bar".to_string(), t.chrome.text_dim, t.chrome.status_bar),
            ("chrome.text_dim on tab_bar".to_string(), t.chrome.text_dim, t.chrome.tab_bar),
            ("chrome.text on banner".to_string(), t.chrome.text, t.chrome.banner),
            ("semantic.working".to_string(), t.semantic.working, t.terminal.background),
            ("semantic.waiting".to_string(), t.semantic.waiting, t.terminal.background),
            ("semantic.bell".to_string(), t.semantic.bell, t.terminal.background),
            ("tree.added".to_string(), t.tree.added, t.chrome.window_bg),
            ("tree.modified".to_string(), t.tree.modified, t.chrome.window_bg),
            ("tree.deleted".to_string(), t.tree.deleted, t.chrome.window_bg),
            ("tree.untracked".to_string(), t.tree.untracked, t.chrome.window_bg),
            ("tree.ignored".to_string(), t.tree.ignored, t.chrome.window_bg),
            ("viewer.keyword".to_string(), t.viewer.keyword, t.chrome.window_bg),
            ("viewer.string".to_string(), t.viewer.string, t.chrome.window_bg),
            ("viewer.comment".to_string(), t.viewer.comment, t.chrome.window_bg),
            ("viewer.number".to_string(), t.viewer.number, t.chrome.window_bg),
            ("viewer.function".to_string(), t.viewer.function, t.chrome.window_bg),
            ("viewer.type_name".to_string(), t.viewer.type_name, t.chrome.window_bg),
            ("viewer.constant".to_string(), t.viewer.constant, t.chrome.window_bg),
            ("viewer.operator".to_string(), t.viewer.operator, t.chrome.window_bg),
            ("viewer.punctuation".to_string(), t.viewer.punctuation, t.chrome.window_bg),
        ];
        let p = t.palette();
        for (i, c) in p.ansi.iter().enumerate() {
            if i == 0 && !t.is_light() {
                continue;
            }
            v.push((format!("ansi {i}"), *c, t.terminal.background));
        }
        v
    }

    #[test]
    fn both_builtins_parse_and_name_themselves() {
        for (name, text) in BUILTIN {
            let t = parse(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(t.name, *name);
            assert_eq!(builtin(name).as_ref(), Some(&t));
        }
        assert!(builtin("nvay").is_none());
        assert!(!builtin("navy").unwrap().is_light());
        assert!(builtin("navy-light").unwrap().is_light());
    }

    #[test]
    fn every_foreground_clears_4_5_to_1_and_the_terminal_text_12_to_1() {
        for (name, _) in BUILTIN {
            let t = builtin(name).unwrap();
            for (what, fg, bg) in foregrounds(&t) {
                let c = contrast(fg, bg);
                assert!(c >= 4.5, "{name}: {what} ({}) on {} is {c:.2}:1", fg.hex(), bg.hex());
            }
            let c = contrast(t.terminal.foreground, t.terminal.background);
            assert!(c >= 11.0, "{name}: terminal text is {c:.1}:1, spec 7.1 wants about 12:1");
            let c = contrast(t.chrome.text, t.chrome.window_bg);
            assert!(c >= 7.0, "{name}: chrome text is {c:.1}:1");
        }
    }

    #[test]
    fn the_palette_maps_the_sixteen_ansi_colours_in_order() {
        let t = builtin("navy").unwrap();
        let p = t.palette();
        assert_eq!(p.bg, t.terminal.background);
        assert_eq!(p.fg, t.terminal.foreground);
        assert_eq!(p.cursor, t.terminal.cursor);
        assert_eq!(p.selection, t.terminal.selection);
        assert_eq!(p.find_match, t.semantic.find_match);
        assert_eq!(p.find_current, t.semantic.find_current);
        let want = [
            t.terminal.black, t.terminal.red, t.terminal.green, t.terminal.yellow, t.terminal.blue, t.terminal.magenta, t.terminal.cyan, t.terminal.white,
            t.terminal.bright_black, t.terminal.bright_red, t.terminal.bright_green, t.terminal.bright_yellow, t.terminal.bright_blue,
            t.terminal.bright_magenta, t.terminal.bright_cyan, t.terminal.bright_white,
        ];
        assert_eq!(p.ansi, want);
        // The built-in Navy is the milestone 1 palette, so nothing on screen changes colour.
        assert_eq!(p.ansi, crate::palette::NAVY.ansi);
        assert_eq!(p.bg, crate::palette::NAVY.bg);
    }

    #[test]
    fn a_theme_file_must_be_complete_and_known() {
        let e = parse("name = \"x\"\n").unwrap_err();
        assert!(e.contains("chrome"), "{e}");
        let text = BUILTIN[0].1.replace("window_bg", "window_background");
        let e = parse(&text).unwrap_err();
        assert!(e.contains("window_background") || e.contains("window_bg"), "{e}");
        let text = BUILTIN[0].1.replace("#11161f", "#11161");
        assert!(parse(&text).is_err());
    }

    #[test]
    fn user_files_win_and_names_are_bare_stems() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load("navy", dir.path()).unwrap(), builtin("navy").unwrap(), "a missing folder falls back to the built-in");
        let mut text = BUILTIN[0].1.to_string();
        text = text.replace("window_bg = \"#11161f\"", "window_bg = \"#000000\"");
        std::fs::write(dir.path().join("navy.toml"), &text).unwrap();
        assert_eq!(load("navy", dir.path()).unwrap().chrome.window_bg, rgb(0x000000));
        std::fs::write(dir.path().join("mine.toml"), BUILTIN[1].1.replace("name = \"navy-light\"", "name = \"mine\"")).unwrap();
        assert_eq!(load("mine", dir.path()).unwrap().name, "mine");
        assert_eq!(names(dir.path()), vec!["mine".to_string(), "navy".to_string(), "navy-light".to_string()]);
        let e = load("nope", dir.path()).unwrap_err();
        assert!(e.contains("nope") && e.contains(&dir.path().display().to_string()), "{e}");
        assert!(load("../navy", dir.path()).is_err());
        assert!(load("a/b", dir.path()).is_err());
        std::fs::write(dir.path().join("broken.toml"), "name = 3").unwrap();
        let e = load("broken", dir.path()).unwrap_err();
        assert!(e.contains("broken.toml"), "{e}");
    }

    #[test]
    fn extract_writes_builtins_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("themes");
        let wrote = extract(&sub).unwrap();
        assert_eq!(wrote.len(), 2);
        assert_eq!(std::fs::read_to_string(sub.join("navy.toml")).unwrap(), BUILTIN[0].1);
        std::fs::write(sub.join("navy.toml"), "edited").unwrap();
        assert!(extract(&sub).unwrap().is_empty(), "nothing to write the second time");
        assert_eq!(std::fs::read_to_string(sub.join("navy.toml")).unwrap(), "edited");
    }
}
```

Add `mod theme;` to `main.rs` (after `mod terminal_element;`).

- [ ] **Step 2: Run the tests to see them fail**

Run: `~/.cargo/bin/cargo test -p arkos theme palette 2>&1 | tail -20`
Expected: compile errors.

- [ ] **Step 3: Extend palette.rs**

In `crates/arkos/src/palette.rs`:

1. Change the `Palette` struct and `NAVY`:

```rust
pub struct Palette {
    pub bg: Rgb,
    pub fg: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
    pub find_match: Rgb,
    pub find_current: Rgb,
    pub ansi: [Rgb; 16],
}

pub const NAVY: Palette = Palette {
    bg: rgb(0x11161f),
    fg: rgb(0xd6dae0),
    cursor: rgb(0xd6dae0),
    selection: rgb(0x2a3a55),
    find_match: rgb(0x3a4556),
    find_current: rgb(0x6b5420),
    ansi: [
        rgb(0x1b2230), rgb(0xf08c8c), rgb(0x8fd9a8), rgb(0xe8c97a), rgb(0x8ab8f0), rgb(0xc9a4e8), rgb(0x7fd4d4), rgb(0xc8ccd4),
        rgb(0x7c8798), rgb(0xf7a8a8), rgb(0xa8e6bd), rgb(0xf0d898), rgb(0xa8ccf5), rgb(0xd9bcf0), rgb(0x9ee2e2), rgb(0xe8ebf0),
    ],
};
```

2. Add after `pub const fn rgb(...)`:

```rust
impl Rgb {
    /// `#rrggbb`, either case. Anything else is an error naming the text.
    pub fn parse(s: &str) -> Result<Rgb, String> {
        let hex = s.strip_prefix('#').ok_or_else(|| format!("colour {s:?} must look like #rrggbb"))?;
        if hex.len() != 6 {
            return Err(format!("colour {s:?} must look like #rrggbb"));
        }
        let v = u32::from_str_radix(hex, 16).map_err(|_| format!("colour {s:?} must look like #rrggbb"))?;
        Ok(rgb(v))
    }

    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    pub fn gpui(self) -> gpui::Rgba {
        gpui::Rgba { r: self.r as f32 / 255.0, g: self.g as f32 / 255.0, b: self.b as f32 / 255.0, a: 1.0 }
    }

    pub fn hsla(self) -> gpui::Hsla {
        self.gpui().into()
    }

    pub fn alpha(self, a: f32) -> gpui::Hsla {
        let mut c = self.hsla();
        c.a = a;
        c
    }
}

impl serde::Serialize for Rgb {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.hex())
    }
}

impl<'de> serde::Deserialize<'de> for Rgb {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Rgb::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// WCAG relative luminance, 0 (black) to 1 (white).
pub fn luminance(c: Rgb) -> f64 {
    fn ch(v: u8) -> f64 {
        let v = v as f64 / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    }
    0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
}

/// WCAG contrast ratio, 1 to 21.
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (l1, l2) = (luminance(a), luminance(b));
    let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}
```

3. Delete the private `luminance` and `contrast` functions from the `tests` module (they now live above) and keep its `use super::*;`.

- [ ] **Step 4: Write the two theme files**

Create `crates/arkos/themes/navy.toml`:

```toml
# Navy: the default dark theme (spec 7, 7.1). A lifted navy, not black; a bright grey text, not
# a dim one; every colour at least 4.5:1 against its background. The chrome values are the
# spec's placeholders until docs/reference/ holds the VS Code screenshot to sample.
name = "navy"

[chrome]
window_bg = "#11161f"
title_bar = "#0a0e14"
status_bar = "#0a0e14"
sidebar = "#0d1117"
tab_bar = "#0d1117"
tab_active = "#11161f"
tab_inactive = "#0d1117"
border = "#1f2630"
split_handle = "#1f2630"
split_handle_hover = "#3a4556"
focus_ring = "#3a7bd5"
text = "#d6dae0"
text_dim = "#8a93a3"
banner = "#1a2130"

[terminal]
background = "#11161f"
foreground = "#d6dae0"
cursor = "#d6dae0"
selection = "#2a3a55"
black = "#1b2230"
red = "#f08c8c"
green = "#8fd9a8"
yellow = "#e8c97a"
blue = "#8ab8f0"
magenta = "#c9a4e8"
cyan = "#7fd4d4"
white = "#c8ccd4"
bright_black = "#7c8798"
bright_red = "#f7a8a8"
bright_green = "#a8e6bd"
bright_yellow = "#f0d898"
bright_blue = "#a8ccf5"
bright_magenta = "#d9bcf0"
bright_cyan = "#9ee2e2"
bright_white = "#e8ebf0"

[semantic]
working = "#8ab8f0"
waiting = "#e8a86a"
bell = "#e8c97a"
error_banner = "#3a2230"
find_match = "#3a4556"
find_current = "#6b5420"

[tree]
added = "#8fd9a8"
modified = "#e8c97a"
deleted = "#e8a0a8"
untracked = "#c9b8e8"
ignored = "#7c8798"

[viewer]
keyword = "#c9a4e8"
string = "#8fd9a8"
comment = "#7c8798"
number = "#e8c97a"
function = "#8ab8f0"
type_name = "#7fd4d4"
constant = "#f0a8a8"
operator = "#c8ccd4"
punctuation = "#a8b0bc"
```

Create `crates/arkos/themes/navy-light.toml`:

```toml
# Navy Light: the light counterpart with the same hue relationships (spec 7). Text is dark on a
# soft blue-white; the ANSI colours are deepened so each clears 4.5:1 on the light background.
name = "navy-light"

[chrome]
window_bg = "#f4f6fa"
title_bar = "#e6eaf2"
status_bar = "#e6eaf2"
sidebar = "#eceff5"
tab_bar = "#e6eaf2"
tab_active = "#f4f6fa"
tab_inactive = "#e6eaf2"
border = "#cfd6e0"
split_handle = "#cfd6e0"
split_handle_hover = "#aeb8c8"
focus_ring = "#2b6cb0"
text = "#1e2633"
text_dim = "#5b6472"
banner = "#dfe5ee"

[terminal]
background = "#f4f6fa"
foreground = "#1e2633"
cursor = "#1e2633"
selection = "#c7d7f0"
black = "#1b2230"
red = "#b3261e"
green = "#1f7a3a"
yellow = "#7a5a00"
blue = "#1d4ed8"
magenta = "#8b2fa3"
cyan = "#0b6e75"
white = "#5b6472"
bright_black = "#646b78"
bright_red = "#c42b2b"
bright_green = "#237f3f"
bright_yellow = "#8a6500"
bright_blue = "#2563eb"
bright_magenta = "#9d3bb5"
bright_cyan = "#0b6f77"
bright_white = "#4b5563"

[semantic]
working = "#1d4ed8"
waiting = "#a1480a"
bell = "#8a6500"
error_banner = "#f3d5d5"
find_match = "#dbe4f3"
find_current = "#f5e3a1"

[tree]
added = "#1f7a3a"
modified = "#8a6500"
deleted = "#b3261e"
untracked = "#6d3fb5"
ignored = "#646b78"

[viewer]
keyword = "#8b2fa3"
string = "#1f7a3a"
comment = "#646b78"
number = "#7a5a00"
function = "#1d4ed8"
type_name = "#0b6e75"
constant = "#b3261e"
operator = "#3b4452"
punctuation = "#5b6472"
```

If `every_foreground_clears_4_5_to_1_and_the_terminal_text_12_to_1` fails on one of these, darken that colour on the light theme (or brighten it on the dark theme) in steps of `#08` on each channel until it passes; keep the hue. Record the final value in the commit message.

- [ ] **Step 5: Sample the screenshot, when there is one**

Create `scripts/sample-png.py` (standard library only; run with `python3 -I`):

```python
#!/usr/bin/env python3
"""Prints the dominant colours of a PNG, overall and in the bands a VS Code window has, so the
Navy theme's chrome can be sampled from docs/reference/ (spec 7). Standard library only; a
Retina screenshot takes about a minute. Usage: python3 -I scripts/sample-png.py <file.png>"""
import struct
import sys
import zlib
from collections import Counter


def read_png(path):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    pos, idat, w, h, bpp = 8, [], 0, 0, 0
    while pos < len(data):
        length, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            w, h, depth, ctype, _, _, interlace = struct.unpack(">IIBBBBB", body)
            assert depth == 8 and ctype in (2, 6) and interlace == 0, f"unsupported PNG: depth {depth} type {ctype} interlace {interlace}"
            bpp = 3 if ctype == 2 else 4
        elif kind == b"IDAT":
            idat.append(body)
        elif kind == b"IEND":
            break
    raw = zlib.decompress(b"".join(idat))
    stride = w * bpp
    rows, prev, p = [], bytearray(stride), 0
    for _ in range(h):
        f = raw[p]
        line = bytearray(raw[p + 1:p + 1 + stride])
        p += 1 + stride
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            if f == 1:
                line[i] = (line[i] + a) & 255
            elif f == 2:
                line[i] = (line[i] + b) & 255
            elif f == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append(bytes(line))
        prev = line
    return w, h, bpp, rows


def top(rows, bpp, x0, x1, y0, y1, n=4):
    counts = Counter()
    for y in range(y0, y1):
        row = rows[y]
        for x in range(x0, x1):
            counts[row[x * bpp:x * bpp + 3]] += 1
    total = sum(counts.values()) or 1
    return [(f"#{px.hex()}", round(100 * k / total, 1)) for px, k in counts.most_common(n)]


w, h, bpp, rows = read_png(sys.argv[1])
print(f"{w}x{h}")
bands = [
    ("whole", (0, w, 0, h)),
    ("title bar (top 40)", (0, w, 0, min(40, h))),
    ("status bar (bottom 24)", (0, w, max(0, h - 24), h)),
    ("activity bar (left 48)", (0, min(48, w), 40, h - 24)),
    ("sidebar (48 to 300)", (48, min(300, w), 40, h - 24)),
    ("editor area", (300, w, 40, h - 24)),
]
for name, box in bands:
    print(f"{name:24}", top(rows, bpp, *box))
```

Then:

```bash
ls docs/reference/*.png 2>/dev/null
```

Run `python3 -I scripts/sample-png.py docs/reference/vscode-full.png` and the same for `vscode-terminals.png`. The captures are 2x, so double every band size in the script's `bands` list first (title bar 80, status bar 48, activity bar 96, sidebar 96 to 600); in `vscode-terminals.png` the window shadow and rounded corners add a border of transparent or near-black pixels, so ignore any colour whose share is under 2 percent there and read the tab bar from a band 80 px tall just under the title bar, and the terminal background from the editor area. Map the most common colour of each band onto `navy.toml`: editor area to `window_bg`, `tab_active` and `terminal.background`; title bar to `title_bar`; status bar to `status_bar`; sidebar to `sidebar`, `tab_bar` and `tab_inactive`; the second most common colour of the editor area, if it is a near-background grey, to `border` and `split_handle`. Then apply spec 7.1: if the sampled background's luminance (`palette::luminance`) is below that of `#11161f`, lift it to `#11161f`'s luminance by adding the same amount to each channel; keep the foreground at `#d6dae0` unless the sampled text is brighter. Re-run the theme tests. Write the sampled values and the lift, if any, in the commit message. Delete the comment line in `navy.toml` that calls the values placeholders.

Also take the file-tree pastels from `vscode-full.png` only by eye (the icons are too small to sample): the spec's hue names stand, the sampled values are the backgrounds.

- [ ] **Step 6: Write the theme module**

Put this above the `tests` module in `crates/arkos/src/theme.rs`:

```rust
//! Themes (spec 7): one flat set of named colours per TOML file. Two are built in and embedded;
//! user files in `~/.config/arkos/themes/` override or add to them. `Root` holds the active
//! `Theme`; views read it, and each terminal resolves cells against `Theme::palette` at paint.
use crate::palette::{luminance, Palette, Rgb};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub name: String,
    pub chrome: Chrome,
    pub terminal: TerminalColors,
    pub semantic: Semantic,
    pub tree: TreeColors,
    pub viewer: ViewerColors,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chrome {
    pub window_bg: Rgb,
    pub title_bar: Rgb,
    pub status_bar: Rgb,
    pub sidebar: Rgb,
    pub tab_bar: Rgb,
    pub tab_active: Rgb,
    pub tab_inactive: Rgb,
    pub border: Rgb,
    pub split_handle: Rgb,
    pub split_handle_hover: Rgb,
    pub focus_ring: Rgb,
    pub text: Rgb,
    pub text_dim: Rgb,
    pub banner: Rgb,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalColors {
    pub background: Rgb,
    pub foreground: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
    pub black: Rgb,
    pub red: Rgb,
    pub green: Rgb,
    pub yellow: Rgb,
    pub blue: Rgb,
    pub magenta: Rgb,
    pub cyan: Rgb,
    pub white: Rgb,
    pub bright_black: Rgb,
    pub bright_red: Rgb,
    pub bright_green: Rgb,
    pub bright_yellow: Rgb,
    pub bright_blue: Rgb,
    pub bright_magenta: Rgb,
    pub bright_cyan: Rgb,
    pub bright_white: Rgb,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Semantic {
    pub working: Rgb,
    pub waiting: Rgb,
    pub bell: Rgb,
    pub error_banner: Rgb,
    pub find_match: Rgb,
    pub find_current: Rgb,
}

/// Milestone 4's file tree. Parsed now so a theme file is complete from the start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeColors {
    pub added: Rgb,
    pub modified: Rgb,
    pub deleted: Rgb,
    pub untracked: Rgb,
    pub ignored: Rgb,
}

/// Milestone 4's viewer, mapped onto the highlighter's scopes then.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewerColors {
    pub keyword: Rgb,
    pub string: Rgb,
    pub comment: Rgb,
    pub number: Rgb,
    pub function: Rgb,
    pub type_name: Rgb,
    pub constant: Rgb,
    pub operator: Rgb,
    pub punctuation: Rgb,
}

/// The embedded themes: (name, file text). Extracted verbatim by `extract`.
pub const BUILTIN: [(&str, &str); 2] = [
    ("navy", include_str!("../themes/navy.toml")),
    ("navy-light", include_str!("../themes/navy-light.toml")),
];

pub fn parse(text: &str) -> Result<Theme, String> {
    toml::from_str(text).map_err(|e| e.message().lines().next().unwrap_or("invalid theme").to_string())
}

pub fn builtin(name: &str) -> Option<Theme> {
    let (_, text) = BUILTIN.iter().find(|(n, _)| *n == name)?;
    // The embedded files are tested to parse; a failure here is a build error, not a user one.
    parse(text).ok()
}

/// True for a name that is a bare file stem: no separators, no parent references, not empty.
fn is_bare_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && name != "." && name != ".."
}

/// `dir/<name>.toml` if it exists, else the built-in of that name. The error names both places.
pub fn load(name: &str, dir: &Path) -> Result<Theme, String> {
    if !is_bare_name(name) {
        return Err(format!("theme name {name:?} must be a bare file name"));
    }
    let file = dir.join(format!("{name}.toml"));
    match std::fs::read_to_string(&file) {
        Ok(text) => parse(&text).map_err(|e| format!("{}: {e}", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            builtin(name).ok_or_else(|| format!("theme {name:?} is not built in and {} does not exist", file.display()))
        }
        Err(e) => Err(format!("cannot read {}: {e}", file.display())),
    }
}

/// Built-in names plus every `*.toml` stem in `dir`, sorted and deduplicated.
pub fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = BUILTIN.iter().map(|(n, _)| n.to_string()).collect();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    out.push(stem.to_string());
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Writes each built-in theme into `dir` unless a file of that name is already there, so a user
/// can copy one to start their own. Returns the paths written.
pub fn extract(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let mut wrote = Vec::new();
    for (name, text) in BUILTIN {
        let file = dir.join(format!("{name}.toml"));
        if file.exists() {
            continue;
        }
        std::fs::write(&file, text)?;
        wrote.push(file);
    }
    Ok(wrote)
}

impl Theme {
    pub fn palette(&self) -> Palette {
        let t = &self.terminal;
        Palette {
            bg: t.background,
            fg: t.foreground,
            cursor: t.cursor,
            selection: t.selection,
            find_match: self.semantic.find_match,
            find_current: self.semantic.find_current,
            ansi: [
                t.black, t.red, t.green, t.yellow, t.blue, t.magenta, t.cyan, t.white,
                t.bright_black, t.bright_red, t.bright_green, t.bright_yellow, t.bright_blue, t.bright_magenta, t.bright_cyan, t.bright_white,
            ],
        }
    }

    /// A light theme has a bright window background; black is then a text colour.
    pub fn is_light(&self) -> bool {
        luminance(self.chrome.window_bg) > 0.5
    }
}
```

- [ ] **Step 7: Run the tests**

Run: `~/.cargo/bin/cargo test -p arkos theme palette 2>&1 | tail -20`
Expected: all pass. `~/.cargo/bin/cargo build -p arkos 2>&1 | grep warning` prints nothing (the `Palette` fields and theme functions unused until Task 4 are `pub`, which is not a warning for a binary crate's `pub` items in a private module only when they are used; if `dead_code` warnings appear for `selection`, `find_match`, `find_current`, `names`, `extract` or `is_light`, add `#[cfg_attr(not(test), allow(dead_code))]` to each with the comment `// Read from Task 4 on.` and remove the attribute in the task that uses it).

- [ ] **Step 8: Commit**

```bash
git add crates/arkos/src/theme.rs crates/arkos/src/palette.rs crates/arkos/src/main.rs crates/arkos/themes scripts/sample-png.py
git commit -m "Add the theme file format with Navy and Navy Light built in"
```

---

### Task 4: Every colour from the theme, appearance following, live palette swap

**Files:**
- Modify: `crates/arkos/src/main.rs`, `crates/arkos/src/chrome.rs`, `crates/arkos/src/pane_view.rs`, `crates/arkos/src/split_view.rs`, `crates/arkos/src/empty_pane.rs`, `crates/arkos/src/drag.rs`, `crates/arkos/src/terminal.rs`, `crates/arkos/src/terminal_element.rs`, `crates/arkos/src/theme.rs` (`pick_name`)
- Test: unit tests in `terminal.rs` and `theme.rs`

**Interfaces:**
- Consumes: `theme::{Theme, load, builtin}`, `config::{Config, AppearanceMode, load}`, `paths::{config_file, themes_dir}`, `palette::Rgb::{hsla, alpha}`.
- Produces:
  - `theme::pick_name(mode: AppearanceMode, system_is_dark: bool, cfg: &ThemeConfig) -> &str`.
  - `theme::is_dark(appearance: gpui::WindowAppearance) -> bool`.
  - `TerminalCore::set_palette(&mut self, palette: Arc<Palette>)` and `TerminalCore::palette(&self) -> &Palette`; the `palette` field becomes `Arc<Palette>` (still `NAVY` by default, so every existing test is unchanged).
  - `Root { config: Config, theme: Arc<Theme>, system_is_dark: bool, .. }`; `Root::new(client, project, saved, config: Config, theme: Theme, system_is_dark: bool)`.
  - `Root::set_theme(&mut self, theme: Theme, cx: &mut Context<Self>)`: stores it and gives every terminal the new palette.
  - `Root::resolve_theme(&self) -> Result<Theme, String>`: the theme the config and appearance name now, loaded.
  - `Root { config_error: Option<String> }`: the config or theme problem shown as a second banner under the connection status; `None` when all is well.
  - `Root::set_system_appearance(&mut self, dark: bool, cx)`: re-resolves when the mode is `System`; a load failure goes in `config_error` and the theme stays.
  - Every view function that drew a colour takes `&Theme` (`chrome::top_bar(name, &theme)`, `chrome::status_bar(.., &theme)`, `empty_pane::render(.., &theme, ..)`), or reads `root.theme`.
  - `drag::TabDrag` gains `bg: Rgb` and `fg: Rgb`, filled from the theme where the drag starts.
  - `TerminalElement::new(terminal, font_size)` reads the background from the terminal's palette.

The `restore` function in `main.rs` builds `TerminalCore`s: give each the root's palette right after `TerminalCore::new` (inside the `background_spawn`, pass an `Arc<Palette>` clone in).

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `crates/arkos/src/terminal.rs`:

```rust
    #[test]
    fn set_palette_recolours_parsed_cells() {
        use crate::palette::{rgb, Palette, NAVY};
        let mut core = TerminalCore::new(10, 2, 100);
        core.apply_output(b"\x1b[31mred\x1b[0m plain");
        let before = core.rows_for_paint();
        assert_eq!(before.visible[0][0].1.fg, NAVY.ansi[1]);
        assert_eq!(before.visible[0][4].1.fg, NAVY.fg);
        assert_eq!(before.visible[0][4].1.bg, NAVY.bg);
        let mut light = Palette { ..NAVY };
        light.bg = rgb(0xf4f6fa);
        light.fg = rgb(0x1e2633);
        light.ansi[1] = rgb(0xb3261e);
        core.set_palette(std::sync::Arc::new(light));
        // Nothing was re-parsed: the same cells resolve against the new palette at paint time.
        let after = core.rows_for_paint();
        assert_eq!(after.visible[0][0].1.fg, rgb(0xb3261e));
        assert_eq!(after.visible[0][4].1.fg, rgb(0x1e2633));
        assert_eq!(after.visible[0][4].1.bg, rgb(0xf4f6fa));
        assert_eq!(core.palette().bg, rgb(0xf4f6fa));
    }
```

`Palette` needs `Clone` for `Palette { ..NAVY }`: add `#[derive(Debug, Clone, PartialEq, Eq)]` to it in `palette.rs`.

Append to the `tests` module in `crates/arkos/src/theme.rs`:

```rust
    #[test]
    fn appearance_mode_picks_the_theme() {
        use crate::config::{AppearanceMode, ThemeConfig};
        let cfg = ThemeConfig { dark: "navy".into(), light: "navy-light".into() };
        assert_eq!(pick_name(AppearanceMode::System, true, &cfg), "navy");
        assert_eq!(pick_name(AppearanceMode::System, false, &cfg), "navy-light");
        assert_eq!(pick_name(AppearanceMode::Dark, false, &cfg), "navy");
        assert_eq!(pick_name(AppearanceMode::Light, true, &cfg), "navy-light");
        use gpui::WindowAppearance::*;
        assert!(is_dark(Dark) && is_dark(VibrantDark));
        assert!(!is_dark(Light) && !is_dark(VibrantLight));
    }
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `~/.cargo/bin/cargo test -p arkos set_palette appearance_mode 2>&1 | tail -20`
Expected: compile errors (`set_palette`, `pick_name`, `is_dark`).

- [ ] **Step 3: The pure parts**

In `theme.rs`, add above the tests:

```rust
/// Which theme name the config picks for the current appearance (spec 7.1: follow the system
/// by default, Navy for dark and Navy Light for light).
pub fn pick_name(mode: crate::config::AppearanceMode, system_is_dark: bool, cfg: &crate::config::ThemeConfig) -> &str {
    use crate::config::AppearanceMode::*;
    match (mode, system_is_dark) {
        (Dark, _) | (System, true) => &cfg.dark,
        (Light, _) | (System, false) => &cfg.light,
    }
}

pub fn is_dark(appearance: gpui::WindowAppearance) -> bool {
    use gpui::WindowAppearance::*;
    matches!(appearance, Dark | VibrantDark)
}
```

In `terminal.rs`:

```rust
pub struct TerminalCore {
    term: Term<Listener>,
    parser: Processor,
    listener: Listener,
    palette: Arc<Palette>,
    pub scroll: ScrollAnimator,
}
```

`TerminalCore::new` sets `palette: Arc::new(NAVY)` (import `NAVY` is already there; `Palette` now derives `Clone`). Add:

```rust
    /// Swaps the colours every cell resolves against at paint. Nothing is re-parsed.
    pub fn set_palette(&mut self, palette: Arc<Palette>) {
        self.palette = palette;
    }

    pub fn palette(&self) -> &Palette {
        &self.palette
    }
```

`style_of` already reads `self.palette.resolve(..)`, which works through the `Arc`.

- [ ] **Step 4: Root owns the config and theme**

In `main.rs`:

1. Add fields to `Root` (after `shell_name`):

```rust
    /// The last good config; `apply_config` (Task 6) replaces it.
    pub config: config::Config,
    pub theme: Arc<theme::Theme>,
    /// What macOS reports; `appearance.mode = "system"` follows it.
    pub system_is_dark: bool,
```

2. Change `Root::new` to `fn new(client: Arc<client::Client>, project: PathBuf, saved: Option<Workspace>, config: config::Config, theme: theme::Theme, system_is_dark: bool) -> Self` and initialise `config, theme: Arc::new(theme), system_is_dark`.

3. Add to `impl Root`:

```rust
    /// The theme the config and the current appearance name, loaded from the themes folder.
    pub fn resolve_theme(&self) -> Result<theme::Theme, String> {
        let name = theme::pick_name(self.config.appearance.mode, self.system_is_dark, &self.config.theme);
        theme::load(name, &paths::themes_dir())
    }

    pub fn set_theme(&mut self, theme: theme::Theme, cx: &mut Context<Self>) {
        let palette = Arc::new(theme.palette());
        self.theme = Arc::new(theme);
        for t in self.sessions.terminals.values() {
            t.update(cx, |t, cx| {
                t.core.set_palette(palette.clone());
                cx.notify();
            });
        }
        cx.notify();
    }

    /// macOS switched between light and dark. In "system" mode the theme follows; a theme that
    /// fails to load leaves the current one and says why in the banner.
    pub fn set_system_appearance(&mut self, dark: bool, cx: &mut Context<Self>) {
        self.system_is_dark = dark;
        if self.config.appearance.mode != config::AppearanceMode::System {
            return;
        }
        match self.resolve_theme() {
            Ok(t) if t != *self.theme => self.set_theme(t, cx),
            Ok(_) => {}
            Err(e) => {
                self.config_error = Some(format!("theme: {e}; keeping {}", self.theme.name));
                eprintln!("arkos: theme: {e}");
                cx.notify();
            }
        }
    }
```

4. Delete the constants `BG`, `FG`, `BANNER_BG` from `main.rs`. `banner_bar(text)` becomes `banner_bar(text: String, theme: &theme::Theme)` using `theme.chrome.banner.hsla()` and `theme.chrome.text.hsla()`; the root div's `.bg(rgb(BG))` becomes `.bg(self.theme.chrome.window_bg.hsla())`. `chrome::top_bar(&project_name)` becomes `chrome::top_bar(&project_name, &theme)` and `chrome::status_bar(&self.project, daemon, sessions, 0)` gains `&theme`, where `let theme = self.theme.clone();` is taken at the top of `render`.

5. In `main()`:

```rust
    let config_path = paths::config_file();
    let (config, config_error) = match config::load(&config_path) {
        Ok(c) => (c, None),
        Err(e) => {
            eprintln!("arkos: {e}");
            (config::Config::default(), Some(e.to_string()))
        }
    };
```

before `Application::new().run`, and inside the `run` closure, before `open_window`:

```rust
        let system_is_dark = theme::is_dark(cx.window_appearance());
        let name = theme::pick_name(config.appearance.mode, system_is_dark, &config.theme).to_string();
        let (theme, theme_error) = match theme::load(&name, &paths::themes_dir()) {
            Ok(t) => (t, None),
            Err(e) => {
                eprintln!("arkos: theme: {e}; using navy");
                (theme::builtin("navy").expect("navy is built in"), Some(format!("theme: {e}")))
            }
        };
```

Pass `config.clone()`, `theme`, `system_is_dark` to `Root::new` (bound as `view_config` and `view_theme` next to `view_project`, since the closure moves them). Add a field `pub config_error: Option<String>` to `Root` (initialised to `None`): the config and theme problems live there, not in `status`, because `status` is cleared when the daemon's session list arrives and belongs to the connection. After the root is built, store the launch error in it, the config error first: `root.update(cx, |root, cx| { root.config_error = config_error.or(theme_error); cx.notify(); })`. In `render`, show one `banner_bar` per non-empty text in `[self.status.clone(), self.config_error.clone().unwrap_or_default()]`, and subtract `BANNER_H` from the content height for each; the `banner_h` local becomes `BANNER_H * banners.len() as f32`.

6. Register the appearance observer, inside the `open_window` builder closure (it receives `window: &mut Window`):

```rust
                move |window, cx| {
                    let view = cx.new(|_| {
                        let mut root = Root::new(client_for_view, view_project, view_saved, view_config, view_theme, system_is_dark);
                        root.save_queue = view_queue;
                        root
                    });
                    let observed = view.clone();
                    window
                        .observe_window_appearance(move |window, cx| {
                            let dark = theme::is_dark(window.appearance());
                            observed.update(cx, |root, cx| root.set_system_appearance(dark, cx));
                        })
                        .detach();
                    view
                }
```

`observe_window_appearance` runs the callback once on registration (`activate()`), which is harmless: `set_system_appearance` with the same theme does nothing.

7. In `restore`, pass the palette in:

```rust
    let palette = root.read_with(cx, |root, _| Arc::new(root.theme.palette()))?;
    let (core, title) = cx
        .background_spawn(async move {
            let mut core = TerminalCore::new(cols, rows, SCROLLBACK);
            core.set_palette(palette);
            let title = core.apply_output(&replay).title;
            (core, title)
        })
        .await;
```

(`Entity::read_with` on an `AsyncApp` returns `Result`; if the name differs in gpui 0.2.2, use `root.update(cx, |root, _| Arc::new(root.theme.palette()))?`.)

- [ ] **Step 5: Views read the theme**

Replace every colour constant with the theme field in this table. Each function gains a `theme: &Theme` parameter or reads `root.theme` (clone the `Arc` into a local at the top of the function: `let theme = root.theme.clone();`, since `root` is borrowed mutably further down).

| File and constant | Theme field |
|---|---|
| `chrome.rs` `BAR_BG` (top bar) | `chrome.title_bar` |
| `chrome.rs` `BAR_BG` (status bar) | `chrome.status_bar` |
| `chrome.rs` `FG`, `DIM` | `chrome.text`, `chrome.text_dim` |
| `pane_view.rs` `BG` (pane body) | `chrome.window_bg` |
| `pane_view.rs` `BAR_BG`, `TAB_ACTIVE`, `TAB_INACTIVE` | `chrome.tab_bar`, `chrome.tab_active`, `chrome.tab_inactive` |
| `pane_view.rs` `FG`, `DIM` | `chrome.text`, `chrome.text_dim` |
| `pane_view.rs` `FOCUS_RING` (focused border), `BAR_BG` (unfocused border) | `chrome.focus_ring`, `chrome.border` |
| `pane_view.rs` `rgba(0x3a7bd566)` drop zone | `chrome.focus_ring.alpha(0.4)` |
| `pane_view.rs` `rgb(0x1a2130)` jump pill | `chrome.banner` |
| `split_view.rs` `HANDLE_COLOR`, `HANDLE_HOVER` | `chrome.split_handle`, `chrome.split_handle_hover` |
| `empty_pane.rs` `FG`, `DIM`, `DOT` | `chrome.text`, `chrome.text_dim`, `semantic.working` (dots keep alpha `0x33`: `semantic.working.alpha(0.2)`) |
| `drag.rs` `CHIP_BG`, `CHIP_FG` | `TabDrag.bg`, `TabDrag.fg` (set from `chrome.banner`, `chrome.text` in `render_tab_bar`) |
| `main.rs` `BG`, `FG`, `BANNER_BG` | `chrome.window_bg`, `chrome.text`, `chrome.banner` |
| `terminal_element.rs` `NAVY.bg` (fill and the `run.style.bg != NAVY.bg` check), `NAVY.cursor` | `t.core.palette().bg`, `.cursor`, read inside the `terminal.update` closure and returned with the rows |

`gpui::rgb(u32)` calls become `field.hsla()`; `gpui::rgba` calls become `field.alpha(a)`. After the edit, this must print only `palette.rs`, `theme.rs` and `layout.rs` test lines:

```bash
grep -n "0x[0-9a-f]\{6\}" crates/arkos/src/*.rs | grep -v "palette.rs\|theme.rs\|layout.rs"
```

`split_view::render_handle` takes `theme: &Theme` (the `group_hover` closure needs the hover colour by value: `let hover = theme.chrome.split_handle_hover;`). `empty_pane::render` gains `theme: &Theme` after `focus`. `render_tab_bar`'s `drag_value` sets `bg: theme.chrome.banner, fg: theme.chrome.text`.

- [ ] **Step 6: Run everything**

Run: `~/.cargo/bin/cargo test -p arkos 2>&1 | tail -15 && ~/.cargo/bin/cargo build -p arkos 2>&1 | grep -c warning`
Expected: all pass; `0`.

Then a launch check (the screen may be locked; it still proves the window opens and the theme loads):

```bash
~/.cargo/bin/cargo build --release 2>&1 | tail -2
(cd /tmp && /Users/csarko/Projects/ide-arkos/target/release/arkos > /tmp/arkos-2b-t4.log 2>&1 &)
sleep 6; grep -c "attached to session\|wrote default config" /tmp/arkos-2b-t4.log; pkill -x arkos; sleep 1; tail -5 /tmp/arkos-2b-t4.log
```

Expected: the log shows `wrote default config to /Users/csarko/.config/arkos/config.toml` on the very first launch and no `theme:` line.

- [ ] **Step 7: Commit**

```bash
git add crates/arkos/src
git commit -m "Draw every colour from the theme and follow the macOS appearance"
```

---

### Task 5: Text presets: font family and line height everywhere, saved with the workspace

**Files:**
- Modify: `crates/arkos/src/terminal_element.rs`, `crates/arkos/src/terminal.rs`, `crates/arkos/src/pane_view.rs`, `crates/arkos/src/workspace.rs`, `crates/arkos/src/main.rs`, `crates/arkos/src/keymap.rs` (`Command::TextPreset`), `crates/arkos/src/config.rs` (`resolve_preset`)
- Test: unit tests in `config.rs`, `workspace.rs`, `terminal_element.rs`

**Interfaces:**
- Consumes: `config::{TextPreset, BUILTIN_PRESETS}`, `Root::font_size`.
- Produces:
  - `config::resolve_preset(saved: Option<&str>, default: &str, presets: &BTreeMap<String, TextPreset>) -> (String, TextPreset)`: the saved name if it exists, else the config default, else `comfortable`, else the comfortable values under the default name.
  - `terminal_element::DEFAULT_LINE_HEIGHT: f32 = 1.5` (replaces `LINE_HEIGHT`); `terminal_element::cell_height(font_size: f32, line_height: f32) -> f32`; `terminal_element::grid_for(bounds, window, font_size: f32, line_height: f32, family: &str) -> (usize, usize)`; `TerminalElement::new(terminal, font_size: f32, line_height: f32, family: SharedString)`. `FONT_FAMILY` is deleted.
  - `Terminal { line_height: f32, .. }`, default `DEFAULT_LINE_HEIGHT`.
  - `Root { line_height: f32, text_preset: String, .. }`; `Root::family(&self) -> SharedString` (the config's font family); `Root::apply_text_preset(&mut self, name: &str, cx) -> bool`: sets the root and every terminal's size and line height, clears the grid cache, saves. `false` for an unknown name.
  - `Command::TextPreset(String)` runs `apply_text_preset`.
  - `workspace::Workspace { text_preset: Option<String> }` (`#[serde(default)]`), `FORMAT_VERSION = 3`.

Cmd+= and Cmd+- keep nudging only the focused terminal's `font_size` (its `line_height` stays the preset's), as spec 7 says. Applying a preset resets every terminal to the preset's values, nudges included.

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `config.rs`:

```rust
    #[test]
    fn a_workspace_preset_wins_then_the_config_default_then_comfortable() {
        let presets = Config::default().font.presets;
        assert_eq!(resolve_preset(Some("relaxed"), "comfortable", &presets).0, "relaxed");
        assert_eq!(resolve_preset(Some("gone"), "compact", &presets).0, "compact");
        assert_eq!(resolve_preset(None, "compact", &presets), ("compact".to_string(), TextPreset { size: 14.0, line_height: 1.3 }));
        assert_eq!(resolve_preset(None, "gone", &presets).0, "comfortable");
        let none = BTreeMap::new();
        assert_eq!(resolve_preset(None, "gone", &none), ("gone".to_string(), TextPreset { size: 16.0, line_height: 1.5 }));
    }
```

Append to the `tests` module in `workspace.rs`:

```rust
    #[test]
    fn the_text_preset_is_saved_and_a_v2_file_has_none() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ws.json");
        let project = Path::new("/p");
        let mut ws = Workspace { version: FORMAT_VERSION, project: project.to_path_buf(), tree: tree_with_two(), text_preset: Some("relaxed".into()) };
        save_to(&file, &ws).unwrap();
        assert_eq!(load_from(&file, project), Some(ws.clone()));
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("\"text_preset\": \"relaxed\""), "{text}");
        // A milestone 2a file has no preset: the config default applies.
        let v2 = text.replace("\"text_preset\": \"relaxed\",", "").replace("\"version\": 3", "\"version\": 2");
        std::fs::write(&file, v2).unwrap();
        ws.text_preset = None;
        ws.version = 2;
        assert_eq!(load_from(&file, project), Some(ws));
    }
```

Every other `Workspace { .. }` literal in `workspace.rs` and `main.rs` gains `text_preset: None` (or the real value in `Root::save`).

Append to the `tests` module in `terminal_element.rs`:

```rust
    #[test]
    fn cell_height_follows_the_preset() {
        use super::cell_height;
        assert_eq!(cell_height(16.0, 1.5), 24.0);
        assert_eq!(cell_height(14.0, 1.3), 18.2);
        assert_eq!(cell_height(18.0, 1.6), 28.8);
    }
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `~/.cargo/bin/cargo test -p arkos resolve_preset text_preset cell_height 2>&1 | tail -20`
Expected: compile errors.

- [ ] **Step 3: The pure parts**

In `config.rs` above the tests:

```rust
/// The preset a workspace uses: what it saved, else the config's default, else comfortable.
pub fn resolve_preset(saved: Option<&str>, default: &str, presets: &BTreeMap<String, TextPreset>) -> (String, TextPreset) {
    for name in saved.into_iter().chain([default, "comfortable"]) {
        if let Some(p) = presets.get(name) {
            return (name.to_string(), *p);
        }
    }
    (default.to_string(), BUILTIN_PRESETS[1].1)
}
```

In `workspace.rs`: `pub const FORMAT_VERSION: u32 = 3;` and

```rust
pub struct Workspace {
    pub version: u32,
    pub project: PathBuf,
    pub tree: Tree,
    /// The text preset this workspace last used (spec 7: a 2x2 grid and a single pane want
    /// different sizes). Absent in milestone 2a files.
    #[serde(default)]
    pub text_preset: Option<String>,
}
```

(and `text_preset: None` in the V1 migration literal).

In `terminal_element.rs`:

```rust
pub const DEFAULT_FONT_SIZE: f32 = 16.0;
pub const DEFAULT_LINE_HEIGHT: f32 = 1.5;

/// The height of one terminal row.
pub fn cell_height(font_size: f32, line_height: f32) -> f32 {
    font_size * line_height
}

pub struct TerminalElement {
    terminal: Entity<Terminal>,
    font_size: f32,
    line_height: f32,
    family: SharedString,
}

impl TerminalElement {
    pub fn new(terminal: Entity<Terminal>, font_size: f32, line_height: f32, family: SharedString) -> Self {
        Self { terminal, font_size, line_height, family }
    }
}

fn text_run(text: &str, style: &CellStyle, family: SharedString) -> TextRun {
    let mut f = font(family);
    if style.bold {
        f.weight = FontWeight::BOLD;
    }
    if style.italic {
        f.style = gpui::FontStyle::Italic;
    }
    let mut color = style.fg.hsla();
    if style.dim {
        color.a = 0.6;
    }
    TextRun { len: text.len(), font: f, color, background_color: None, underline: None, strikethrough: None }
}

fn measure_cell(window: &mut Window, font_size: f32, line_height: f32, family: SharedString) -> CellMetrics {
    let run = TextRun { len: 1, font: font(family), color: gpui::white(), background_color: None, underline: None, strikethrough: None };
    let layout = window.text_system().layout_line("W", px(font_size), &[run], None);
    CellMetrics { width: f32::from(layout.width), height: cell_height(font_size, line_height) }
}

pub fn grid_for(bounds: Bounds<Pixels>, window: &mut Window, font_size: f32, line_height: f32, family: &str) -> (usize, usize) {
    grid_in(bounds, measure_cell(window, font_size, line_height, family.to_string().into()))
}
```

`prepaint` calls `measure_cell(window, self.font_size, self.line_height, self.family.clone())`; `paint` passes `self.family.clone()` to `text_run`.

In `terminal.rs`, `Terminal` gains `pub line_height: f32` initialised to `crate::terminal_element::DEFAULT_LINE_HEIGHT`.

- [ ] **Step 4: Root applies presets**

In `main.rs`:

1. Fields `pub line_height: f32` and `pub text_preset: String` after `font_size`. In `Root::new`:

```rust
        let (text_preset, preset) = config::resolve_preset(saved.as_ref().and_then(|w| w.text_preset.as_deref()), &config.font.preset, &config.font.presets);
        /* ... */
        font_size: preset.size,
        line_height: preset.line_height,
        text_preset,
```

(`saved` is consumed for `tree` below; take the preset name before that.)

2. `Root::save` writes `text_preset: Some(self.text_preset.clone())`.

3. Add:

```rust
    pub fn family(&self) -> gpui::SharedString {
        self.config.font.family.clone().into()
    }

    /// Switches every terminal to a preset (spec 7). Per-terminal nudges from Cmd+= are reset.
    pub fn apply_text_preset(&mut self, name: &str, cx: &mut Context<Self>) -> bool {
        let Some(p) = self.config.font.presets.get(name).copied() else { return false };
        self.text_preset = name.to_string();
        self.font_size = p.size;
        self.line_height = p.line_height;
        for t in self.sessions.terminals.values() {
            t.update(cx, |t, cx| {
                t.font_size = p.size;
                t.line_height = p.line_height;
                cx.notify();
            });
        }
        // Panes re-measure at the new cell size before any restore trusts a grid.
        self.grids.clear();
        self.save(cx);
        cx.notify();
        true
    }
```

4. In `run`: `TextPreset(name) => { self.apply_text_preset(&name, cx); }`. Add `TextPreset(String)` to `Command` in `keymap.rs` (no key binding; the palette runs it in Task 8).

5. In `restore`, the new `Terminal` gets `t.line_height = root.line_height;` next to `t.font_size = root.font_size;`.

6. In `pane_view.rs`: `render_terminal(terminal, line_height_unused..)`: read `font_size` and `line_height` from the terminal entity, `family` from `root.family()` (pass `family: SharedString` into `render_terminal`); `TerminalElement::new(terminal.clone(), font_size, line_height, family)`. The measuring canvas in `render_pane` calls `grid_for(bounds, window, font_size, line_height, &family)` with `let line_height = root.line_height; let family = root.config.font.family.clone();` captured by the closure. `render_busy`, `render_tab_bar`, `render_no_tabs` and the jump pill use `.font_family(root.family())` instead of `"Menlo"`; so do `chrome.rs` (pass `family: SharedString`), `empty_pane.rs` and `banner_bar`.

- [ ] **Step 5: Run everything**

Run: `~/.cargo/bin/cargo test -p arkos 2>&1 | tail -15 && ~/.cargo/bin/cargo build -p arkos 2>&1 | grep -c warning`
Expected: all pass; `0`. `grep -rn "Menlo" crates/arkos/src` must hit only `config.rs`.

- [ ] **Step 6: Commit**

```bash
git add crates/arkos/src
git commit -m "Apply text presets to every terminal and save the active one"
```

---

### Task 6: Live reload: the watcher, apply_config, terminal defaults, daemon flags, Reduce Motion

**Files:**
- Create: `crates/arkos/src/config_watch.rs`
- Modify: `crates/arkos/src/main.rs`, `crates/arkos/src/config.rs` (`reload_decision`), `crates/arkos/src/theme.rs` (`keep_or_switch`), `crates/arkos/src/sessions.rs`, `crates/arkos/src/client.rs`, `crates/arkos/src/motion.rs`, `crates/arkos/src/keymap.rs` (`Command::ReloadConfig`, `Command::ExtractThemes`)
- Test: unit tests in `config_watch.rs`, `config.rs`, `theme.rs`, `sessions.rs`, `motion.rs`

**Interfaces:**
- Consumes: `config::load`, `keymap::bindings_for`, `Root::{set_theme, resolve_theme, apply_text_preset, config_error}`.
- Produces:
  - `config_watch::POLL: Duration = 500 ms`; `config_watch::fingerprint(path: &Path) -> Option<(u128, u64)>` (mtime in nanoseconds since the epoch, length; `None` when missing); `config_watch::start(root: Entity<Root>, path: PathBuf, cx: &mut App)`.
  - `config::reload_decision(current: &Config, loaded: Result<Config, ConfigError>) -> (Config, Option<String>)`: the config to use and the banner text.
  - `theme::keep_or_switch(current: &Theme, wanted: Result<Theme, String>) -> (Option<Theme>, Option<String>)`: `Some(theme)` when a different theme loaded, the banner text when loading failed.
  - `Root::reload_config(&mut self, cx)` and `Root::apply_config(&mut self, cfg: Config, cx)`.
  - `SessionManager { scrollback: usize, shell: Option<String>, nice: i32 }` used by `create` and by `restore`; `SessionManager::set_defaults(&mut self, scrollback: usize, shell: Option<String>, nice: i32)`. `sessions::SCROLLBACK` stays as the initial value.
  - `client::Client::connect_or_start(socket, daemon_exe, daemon_args: &[String])`.
  - `motion::refresh()` re-reads the setting into an `AtomicBool`; `motion::reduce_motion()` reads it.
  - `Command::ReloadConfig` and `Command::ExtractThemes` (palette-only, Task 8).

- [ ] **Step 1: Write the failing tests**

Create `crates/arkos/src/config_watch.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_changes_with_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.toml");
        assert_eq!(fingerprint(&file), None);
        std::fs::write(&file, "a").unwrap();
        let one = fingerprint(&file).expect("exists");
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&file, "bb").unwrap();
        let two = fingerprint(&file).expect("exists");
        assert_ne!(one, two);
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&file, "cc").unwrap();
        assert_ne!(fingerprint(&file), Some(two), "the same length at a later time is a change");
        std::fs::remove_file(&file).unwrap();
        assert_eq!(fingerprint(&file), None);
    }
}
```

Append to `config.rs` tests:

```rust
    #[test]
    fn bad_reload_keeps_the_last_good_config() {
        let mut good = Config::default();
        good.font.family = "Monaco".into();
        let (cfg, banner) = reload_decision(&good, parse("[font]\nfamily = \n"));
        assert_eq!(cfg, good);
        assert!(banner.as_deref().unwrap_or("").starts_with("config.toml line 2: "), "{banner:?}");
        let (cfg, banner) = reload_decision(&good, parse("[font]\nfamily = \"Menlo\"\n"));
        assert_eq!(cfg.font.family, "Menlo");
        assert_eq!(banner, None);
    }
```

Append to `theme.rs` tests:

```rust
    #[test]
    fn missing_theme_keeps_the_current_one() {
        let navy = builtin("navy").unwrap();
        let (switch, banner) = keep_or_switch(&navy, Err("theme \"nvay\" is not built in".into()));
        assert_eq!(switch, None);
        let banner = banner.unwrap();
        assert!(banner.contains("nvay") && banner.contains("keeping navy"), "{banner}");
        let (switch, banner) = keep_or_switch(&navy, Ok(navy.clone()));
        assert_eq!((switch, banner), (None, None), "the same theme is not re-applied");
        let light = builtin("navy-light").unwrap();
        assert_eq!(keep_or_switch(&navy, Ok(light.clone())), (Some(light), None));
    }
```

Append to `sessions.rs` tests (next to the existing `create` test that inspects `reqs`):

```rust
    #[test]
    fn create_uses_the_configured_shell_and_nice() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r.clone(), None, PathBuf::from("/p"));
        m.set_defaults(5_000, Some("/bin/bash".into()), 5);
        assert_eq!(m.scrollback, 5_000);
        m.create(1, false);
        match &r.requests()[0] {
            Request::CreateSession { shell, nice, .. } => {
                assert_eq!(shell.as_deref(), Some("/bin/bash"));
                assert_eq!(*nice, 5);
            }
            other => panic!("{other:?}"),
        }
    }
```

(`Recorder` is the test transport already in `sessions.rs`; `requests()` returns what was sent, in order.)

Replace the test in `motion.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults_output_and_refresh_sets_the_flag() {
        assert!(parse_reduce_motion("1\n"));
        assert!(!parse_reduce_motion("0\n"));
        assert!(!parse_reduce_motion(""));
        set(true);
        assert!(reduce_motion());
        set(false);
        assert!(!reduce_motion());
    }
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `~/.cargo/bin/cargo test -p arkos fingerprint reload keep_or_switch configured_shell motion 2>&1 | tail -20`
Expected: compile errors.

- [ ] **Step 3: The pure parts**

`config_watch.rs` above its tests:

```rust
//! Notices edits to the config file (spec 7: "watched and applied live"). Polls the file's
//! mtime and size twice a second from the background executor: one stat per half second, no
//! watcher thread, no new dependency. A deleted file counts as a change and is recreated with
//! the defaults by `config::load`.
use crate::Root;
use gpui::{App, Entity};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

pub const POLL: Duration = Duration::from_millis(500);

/// (mtime in nanoseconds since the epoch, length), or `None` when the file is missing.
pub fn fingerprint(path: &Path) -> Option<(u128, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos();
    Some((mtime, meta.len()))
}

/// Polls until the window is gone; every change asks `Root::reload_config`.
pub fn start(root: Entity<Root>, path: PathBuf, cx: &mut App) {
    cx.spawn(async move |cx| {
        let mut last = fingerprint(&path);
        loop {
            cx.background_executor().timer(POLL).await;
            let p = path.clone();
            let now = cx.background_executor().spawn(async move { fingerprint(&p) }).await;
            if now == last {
                continue;
            }
            last = now;
            if root.update(cx, |root, cx| root.reload_config(cx)).is_err() {
                break;
            }
        }
    })
    .detach();
}
```

`config.rs`:

```rust
/// A reload keeps the last good config when the new one does not parse (spec 7).
pub fn reload_decision(current: &Config, loaded: Result<Config, ConfigError>) -> (Config, Option<String>) {
    match loaded {
        Ok(cfg) => (cfg, None),
        Err(e) => (current.clone(), Some(e.to_string())),
    }
}
```

`theme.rs`:

```rust
/// A theme that fails to load keeps the current one and says so; the same theme is not re-applied.
pub fn keep_or_switch(current: &Theme, wanted: Result<Theme, String>) -> (Option<Theme>, Option<String>) {
    match wanted {
        Ok(t) if t == *current => (None, None),
        Ok(t) => (Some(t), None),
        Err(e) => (None, Some(format!("theme: {e}; keeping {}", current.name))),
    }
}
```

`motion.rs` (whole file above the tests):

```rust
//! Honours macOS's Reduce Motion setting (spec 7.1: the UI adds no motion the user turned off).
//! Read through `defaults`, which needs no extra dependency, at launch and on every config
//! reload (`refresh` runs on the background executor; it spawns a process).
use std::sync::atomic::{AtomicBool, Ordering};

static REDUCE: AtomicBool = AtomicBool::new(false);

pub fn reduce_motion() -> bool {
    REDUCE.load(Ordering::Relaxed)
}

fn set(on: bool) {
    REDUCE.store(on, Ordering::Relaxed);
}

/// Re-reads the setting. Blocks on `defaults` for a few milliseconds.
pub fn refresh() {
    let out = std::process::Command::new("defaults").args(["read", "com.apple.universalaccess", "reduceMotion"]).output();
    set(match out {
        Ok(o) => parse_reduce_motion(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => false,
    });
}

/// `defaults` prints `1` when the setting is on, `0` or nothing (an error on stderr) when off.
pub fn parse_reduce_motion(stdout: &str) -> bool {
    stdout.trim() == "1"
}
```

`sessions.rs`: three `pub` fields `scrollback: usize`, `shell: Option<String>`, `nice: i32` on `SessionManager`, initialised in `new` to `SCROLLBACK`, `None`, `0`; `create` sends `shell: self.shell.clone(), nice: self.nice`; add

```rust
    /// The `[terminal]` section, applied to shells started from now on (spec 7).
    pub fn set_defaults(&mut self, scrollback: usize, shell: Option<String>, nice: i32) {
        self.scrollback = scrollback;
        self.shell = shell;
        self.nice = nice;
    }
```

`restore` in `main.rs` uses `root.sessions.scrollback` instead of `SCROLLBACK` (read it in the same `read_with` as the palette).

`client.rs`: `connect_or_start(socket: &Path, daemon_exe: &Path, daemon_args: &[String])` adds `cmd.args(daemon_args)` after `--socket`. Update its existing tests to pass `&[]`.

- [ ] **Step 4: Root reloads and applies**

In `main.rs`, `impl Root`:

```rust
    /// The file changed (or was deleted and recreated): parse it, keep the last good config on
    /// an error, apply what changed. Called by the watcher and by the palette's "Reload config".
    pub fn reload_config(&mut self, cx: &mut Context<Self>) {
        let (cfg, banner) = config::reload_decision(&self.config, config::load(&paths::config_file()));
        self.config_error = banner;
        if let Some(e) = &self.config_error {
            eprintln!("arkos: {e}");
        } else {
            eprintln!("arkos: config reloaded");
            self.apply_config(cfg, cx);
        }
        cx.notify();
    }

    /// Applies every live setting. The `[daemon]` section only matters when the window starts
    /// the daemon; the file says so.
    pub fn apply_config(&mut self, cfg: config::Config, cx: &mut Context<Self>) {
        let old = std::mem::replace(&mut self.config, cfg);
        if old.keybindings != self.config.keybindings {
            cx.clear_key_bindings();
            cx.bind_keys(keymap::bindings_for(&self.config.keybindings));
        }
        let (switch, banner) = theme::keep_or_switch(&self.theme, self.resolve_theme());
        if let Some(t) = switch {
            self.set_theme(t, cx);
        }
        if banner.is_some() {
            self.config_error = banner;
        }
        if old.font != self.config.font {
            let (name, _) = config::resolve_preset(Some(&self.text_preset), &self.config.font.preset, &self.config.font.presets);
            self.apply_text_preset(&name, cx);
        }
        let t = &self.config.terminal;
        self.sessions.set_defaults(t.scrollback, t.shell.clone(), t.nice);
        cx.background_executor().spawn(async { motion::refresh() }).detach();
        cx.notify();
    }
```

`run` gains `ReloadConfig => self.reload_config(cx)` and:

```rust
            ExtractThemes => match theme::extract(&paths::themes_dir()) {
                Ok(wrote) if wrote.is_empty() => self.status = format!("built-in themes are already in {}", paths::themes_dir().display()),
                Ok(wrote) => self.status = format!("wrote {} theme files to {}", wrote.len(), paths::themes_dir().display()),
                Err(e) => self.status = format!("cannot extract themes: {e}"),
            },
```

(`status` is the right place: it is a one-off notice, and the next daemon response clears an error prefix only; a plain notice stays until the next one replaces it. Add `cx.notify()` after the match.)

In `main()`: `motion::refresh();` before `app_nap::hold();`; the daemon args:

```rust
    let daemon_args = vec![
        "--watchdog-secs".to_string(), config.daemon.watchdog_secs.to_string(),
        "--replay-bytes".to_string(), config.daemon.replay_bytes.to_string(),
    ];
    let (client, mut events) = match client::Client::connect_or_start(&paths::socket_path(), &paths::daemon_exe(), &daemon_args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("arkos: cannot reach arkosd: {e:#}");
            std::process::exit(1);
        }
    };
```

(`config` must be loaded before this, which it already is from Task 4; move the config block above the connect.) After the root is built and the quit handlers are registered: `config_watch::start(root.clone(), config_path.clone(), cx);`. Also, in `Root::new`, call `sessions.set_defaults(...)` from the config so the first shells use it, and bind keys from the config: `cx.bind_keys(keymap::bindings_for(&config.keybindings))` replaces `cx.bind_keys(keymap::bindings())`.

Add `mod config_watch;` to `main.rs`.

- [ ] **Step 5: Run everything, then a live reload check**

Run: `~/.cargo/bin/cargo test -p arkos 2>&1 | tail -15 && ~/.cargo/bin/cargo build -p arkos 2>&1 | grep -c warning`
Expected: all pass; `0`.

Then, with the release binary running from `/tmp` as in Task 4:

```bash
sed -i '' 's/^mode = "system"/mode = "light"/' ~/.config/arkos/config.toml; sleep 1.5
sed -i '' 's/^mode = "light"/mode = "sytem"/' ~/.config/arkos/config.toml; sleep 1.5
sed -i '' 's/^mode = "sytem"/mode = "system"/' ~/.config/arkos/config.toml; sleep 1.5
pkill -x arkos; sleep 1; grep -n "config reloaded\|config.toml line" /tmp/arkos-2b-t4.log
```

Expected: two `config reloaded` lines around one `config.toml line N:` line (the mode typo), in that order.

- [ ] **Step 6: Commit**

```bash
git add crates/arkos/src
git commit -m "Reload config.toml live and keep the last good one on a bad edit"
```

---

### Task 7: A single-line editor for the palette and the find bar

**Files:**
- Create: `crates/arkos/src/line_edit.rs`
- Modify: `crates/arkos/src/main.rs` (`mod line_edit;`)
- Test: unit tests in `line_edit.rs`

**Interfaces:**
- Consumes: `gpui::Keystroke`, `theme::Theme`.
- Produces:
  - `line_edit::LineEdit { text: String, cursor: usize }` (`cursor` counts chars), `LineEdit::new()`, `LineEdit::set(&mut self, text: &str)` (cursor to the end), `LineEdit::clear(&mut self)`, `LineEdit::insert_str(&mut self, s: &str)`.
  - `line_edit::Edit { Changed, Unchanged, Submit, SubmitBack, Cancel, Up, Down }`; `LineEdit::handle(&mut self, ks: &Keystroke) -> Edit`.
  - `line_edit::render(edit: &LineEdit, placeholder: &str, theme: &Theme, family: SharedString, size: f32) -> Div`: the text with a 1 px cursor bar, or the placeholder in `text_dim` when empty.

gpui 0.2.2 ships no text input element (its `examples/input.rs` builds one by hand), so this is the smallest one that feels right: insert, delete, move, word delete, line delete, submit, cancel.

- [ ] **Step 1: Write the failing tests**

Create `crates/arkos/src/line_edit.rs` with:

```rust
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
```

Add `mod line_edit;` to `main.rs` (after `mod layout;`).

- [ ] **Step 2: Run the tests to see them fail**

Run: `~/.cargo/bin/cargo test -p arkos line_edit 2>&1 | tail -20`
Expected: compile errors.

- [ ] **Step 3: Write the editor**

Above the tests in `line_edit.rs`:

```rust
//! One line of editable text with a cursor, for the command palette and the find bar. gpui
//! 0.2.2 has no input element, so this is the smallest hand-built one: the keys a Mac user
//! expects in a search field, nothing more. Pure, so every key is unit tested.
use crate::theme::Theme;
use gpui::{div, prelude::*, px, Div, Keystroke, SharedString};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineEdit {
    pub text: String,
    /// In chars, 0 to `text.chars().count()`.
    pub cursor: usize,
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
```

The "a"/"e" control arms are matched before the generic `m.control` early return, so Ctrl+A and Ctrl+E work like a shell; every other Ctrl combination is ignored, as the test `ctrl_with_named_key_is_not_a_control_byte` in `keys.rs` expects for terminals.

- [ ] **Step 4: Run the tests**

Run: `~/.cargo/bin/cargo test -p arkos line_edit 2>&1 | tail -15`
Expected: five pass. The `render` function is unused until Task 8; add `#[cfg_attr(not(test), allow(dead_code))]` above it with the comment `// Used from Task 8.` if the build warns, and remove it then.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/line_edit.rs crates/arkos/src/main.rs
git commit -m "Add a hand-built single-line editor for the palette and find"
```

---

### Task 8: The command palette

**Files:**
- Create: `crates/arkos/src/command_palette.rs`
- Modify: `crates/arkos/src/keymap.rs` (`CommandPalette` action and row, `pretty_keys`, `label`, `Command::{CommandPalette, SetTheme}`), `crates/arkos/src/main.rs`
- Test: unit tests in `command_palette.rs` and `keymap.rs`

**Interfaces:**
- Consumes: `line_edit::{LineEdit, Edit, render}`, `keymap::{ACTIONS, keys_for}`, `theme::names`, `Root::{config, apply_text_preset, set_theme, reload_config, run}`.
- Produces:
  - `keymap::ACTIONS` gains `("command palette", "cmd-shift-p")` after `open config`; action `CommandPalette`; `Command::CommandPalette` toggles the palette; `Command::SetTheme(String)` applies a theme for this run (the config file is the only place a theme is kept; the entry's detail says so).
  - `keymap::pretty_keys(keys: &str) -> String`: `"cmd-shift-p"` to `Shift+Cmd+P` (Apple's menu order: Control, Option, Shift, Command), chords joined with ` then `, `cmd-k cmd-\\` to `Cmd+K then Cmd+\`, `ctrl-shift-\`` to ``Ctrl+Shift+` ``, arrows and named keys capitalised (`Left`, `Enter`, `Tab`, `Escape`, `Backspace`, `Space`), single letters upper-cased, `alt` printed as `Option`.
  - `keymap::label(name: &str) -> String`: the action's name with its first letter upper-cased.
  - `command_palette::Item { label: String, detail: String, command: Command }`; `command_palette::items(cfg: &Config, themes: &[String], current_preset: &str, current_theme: &str) -> Vec<Item>`: every action in `ACTIONS` except `command palette` and `quit`, then `Text preset: <name>` for each preset (the current one marked `current` in the detail), then `Theme: <name>` for each name, then `Reload config`, `Extract built-in themes`.
  - `command_palette::score(query: &str, label: &str) -> Option<i32>`: case-insensitive subsequence match; higher is better; `None` when a query char is missing. Each matched char scores 1, plus 2 when it directly follows the previous match, plus 1 when it starts a word (index 0, or after a space or colon); an empty query scores 0 for everything. A contiguous run therefore beats the same letters scattered across word starts.
  - `keymap::command_for(name: &str) -> Option<Command>`: the `Command` an action name runs; `None` for `command palette` and `quit`, which the palette never lists.
  - `command_palette::filter(query: &str, items: &[Item]) -> Vec<usize>`: indices sorted by score descending, then by original order.
  - `command_palette::Palette { query: LineEdit, selected: usize, items: Vec<Item>, visible: Vec<usize>, focus: FocusHandle }`; `Palette::new(items, focus)`, `Palette::refilter(&mut self)`, `Palette::step(&mut self, delta: isize)`, `Palette::selected_command(&self) -> Option<Command>`; `command_palette::MAX_ROWS: usize = 12`.
  - `command_palette::render(root: &mut Root, cx: &mut Context<Root>) -> Option<Div>`: the overlay when open.
  - `Root { palette: Option<Palette> }`, `Root::toggle_palette(cx)`, `Root::palette_key(ks, cx)`.

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `keymap.rs`:

```rust
    #[test]
    fn keys_print_the_mac_way() {
        assert_eq!(pretty_keys("cmd-shift-p"), "Shift+Cmd+P");
        assert_eq!(pretty_keys("cmd-k cmd-\\"), "Cmd+K then Cmd+\\");
        assert_eq!(pretty_keys("ctrl-shift-`"), "Ctrl+Shift+`");
        assert_eq!(pretty_keys("alt-cmd-right"), "Option+Cmd+Right");
        assert_eq!(pretty_keys("cmd-k w"), "Cmd+K then W");
        assert_eq!(pretty_keys("cmd-,"), "Cmd+,");
        assert_eq!(pretty_keys("cmd-="), "Cmd+=");
        assert_eq!(pretty_keys("ctrl-tab"), "Ctrl+Tab");
        assert_eq!(label("focus pane 1"), "Focus pane 1");
    }
```

Create `crates/arkos/src/command_palette.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn labels(items: &[Item], order: &[usize]) -> Vec<String> {
        order.iter().map(|i| items[*i].label.clone()).collect()
    }

    #[test]
    fn items_cover_actions_presets_themes_and_config() {
        let cfg = Config::default();
        let items = items(&cfg, &["navy".into(), "navy-light".into()], "comfortable", "navy");
        let n_actions = crate::keymap::ACTIONS.len() - 2;
        assert_eq!(items.len(), n_actions + 3 + 2 + 2);
        assert!(!items.iter().any(|i| i.label == "Command palette" || i.label == "Quit"));
        let split = items.iter().find(|i| i.label == "Split right").unwrap();
        assert_eq!(split.detail, "Cmd+\\");
        assert_eq!(split.command, Command::Split(crate::tree::Side::Right));
        let preset = items.iter().find(|i| i.label == "Text preset: comfortable").unwrap();
        assert_eq!(preset.detail, "16 px, line height 1.5, current");
        assert_eq!(preset.command, Command::TextPreset("comfortable".into()));
        let compact = items.iter().find(|i| i.label == "Text preset: compact").unwrap();
        assert_eq!(compact.detail, "14 px, line height 1.3, below the research's comfort floor");
        let theme = items.iter().find(|i| i.label == "Theme: navy-light").unwrap();
        assert_eq!(theme.detail, "until relaunch; set [theme] in config.toml to keep");
        assert_eq!(theme.command, Command::SetTheme("navy-light".into()));
        assert_eq!(items.iter().find(|i| i.label == "Theme: navy").unwrap().detail, "current");
        assert!(items.iter().any(|i| i.command == Command::ReloadConfig));
        assert!(items.iter().any(|i| i.command == Command::ExtractThemes));
        let unbound = cfg_with("split right", "");
        let items = self::items(&unbound, &[], "comfortable", "navy");
        assert_eq!(items.iter().find(|i| i.label == "Split right").unwrap().detail, "");
    }

    fn cfg_with(name: &str, keys: &str) -> Config {
        let mut cfg = Config::default();
        cfg.keybindings.insert(name.into(), keys.into());
        cfg
    }

    #[test]
    fn scoring_prefers_word_starts_and_runs() {
        assert_eq!(score("", "anything"), Some(0));
        assert_eq!(score("x", "anything"), None);
        assert!(score("sr", "Split right").unwrap() > score("sr", "Reset ratio sr").unwrap_or(0));
        assert!(score("split", "Split right").unwrap() > score("split", "Spl it right").unwrap());
        assert!(score("SPLIT", "split right").is_some(), "case does not matter");
        assert_eq!(score("tp", "Text preset: relaxed"), Some(4), "both chars at word starts, not adjacent");
        assert_eq!(score("split", "Split right"), Some(14), "2 for the first char, 3 for each adjacent one");
    }

    #[test]
    fn filter_sorts_by_score_then_keeps_order() {
        let items: Vec<Item> = ["Focus pane 1", "Find", "Font bigger", "Split right"]
            .iter()
            .map(|l| Item { label: l.to_string(), detail: String::new(), command: Command::MruTab })
            .collect();
        assert_eq!(filter("", &items).len(), 4);
        assert_eq!(labels(&items, &filter("f", &items)), vec!["Focus pane 1", "Find", "Font bigger"]);
        assert_eq!(labels(&items, &filter("fb", &items)), vec!["Font bigger"]);
        assert_eq!(labels(&items, &filter("zzz", &items)), Vec::<String>::new());
    }

    #[test]
    fn stepping_wraps_within_the_visible_rows() {
        let items: Vec<Item> = (0..3).map(|i| Item { label: format!("Item {i}"), detail: String::new(), command: Command::MruTab }).collect();
        let mut p = Palette { query: crate::line_edit::LineEdit::new(), selected: 0, items, visible: vec![], focus: None };
        p.refilter();
        assert_eq!(p.visible, vec![0, 1, 2]);
        p.step(-1);
        assert_eq!(p.selected, 2);
        p.step(1);
        assert_eq!(p.selected, 0);
        p.query.set("2");
        p.refilter();
        assert_eq!(p.visible, vec![2]);
        assert_eq!(p.selected, 0, "a new filter starts at the top");
        assert_eq!(p.selected_command(), Some(Command::MruTab));
        p.query.set("nothing");
        p.refilter();
        assert_eq!(p.selected_command(), None);
    }
}
```

The `focus` field is `Option<FocusHandle>` so the pure tests can build a `Palette` without a window.

- [ ] **Step 2: Run the tests to see them fail**

Run: `~/.cargo/bin/cargo test -p arkos command_palette pretty 2>&1 | tail -20`
Expected: compile errors.

- [ ] **Step 3: keymap additions**

In `keymap.rs`: add `CommandPalette` to the `actions!` list; `Command` gains `CommandPalette`, `SetTheme(String)`, `ReloadConfig`, `ExtractThemes` (the last two from Task 6 if not already there); add `row("command palette", "cmd-shift-p"),` after the `open config` row; add `"command palette" => KeyBinding::new(keys, CommandPalette, context),` to `binding`; add `CommandPalette => Command::CommandPalette,` to `attach`. Update the test table `SPEC_ROWS` with `("cmd-shift-p", "command palette", "CommandPalette")` and `DEFAULT_CONFIG` with the line `# "command palette" = "cmd-shift-p"` (after `open config`). Add:

```rust
/// A keystroke the way the Mac writes it: `cmd-shift-p` is `Cmd+Shift+P`, chords join with
/// " then ".
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
            if k.modifiers.shift {
                parts.push("Shift".to_string());
            }
            if k.modifiers.platform {
                parts.push("Cmd".to_string());
            }
            let key = match k.key.as_str() {
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
```

The modifier order Control, Option, Shift, Command is the one Apple's menus use; the README's table (Task 11) switches to it too.

- [ ] **Step 4: The palette module**

Above the tests in `command_palette.rs`:

```rust
//! The command palette (spec 4: Cmd+Shift+P). Every action by name with its keys, the text
//! presets, the themes, and the config commands, filtered by a subsequence match. The scoring
//! and filtering are pure; `render` is the overlay.
use crate::config::Config;
use crate::keymap::{self, Command, ACTIONS};
use crate::line_edit::{self, Edit, LineEdit};
use crate::Root;
use gpui::{div, prelude::*, px, ClickEvent, Context, Div, FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent};

pub const MAX_ROWS: usize = 12;
const WIDTH: f32 = 560.0;
const TEXT: f32 = 13.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub label: String,
    /// Shown dimmed at the right: the keys, or a note.
    pub detail: String,
    pub command: Command,
}

/// Everything the palette offers, in display order.
pub fn items(cfg: &Config, themes: &[String], current_preset: &str, current_theme: &str) -> Vec<Item> {
    let mut out = Vec::new();
    for a in ACTIONS {
        if a.name == "command palette" || a.name == "quit" {
            continue;
        }
        let command = match keymap::command_for(a.name) {
            Some(c) => c,
            None => continue,
        };
        let detail = keymap::keys_for(a.name, &cfg.keybindings).map(|k| keymap::pretty_keys(&k)).unwrap_or_default();
        out.push(Item { label: keymap::label(a.name), detail, command });
    }
    for (name, p) in &cfg.font.presets {
        let mut detail = format!("{} px, line height {}", p.size, p.line_height);
        if name == current_preset {
            detail.push_str(", current");
        } else if name == "compact" {
            detail.push_str(", below the research's comfort floor");
        }
        out.push(Item { label: format!("Text preset: {name}"), detail, command: Command::TextPreset(name.clone()) });
    }
    for name in themes {
        let detail = if name == current_theme { "current".to_string() } else { "until relaunch; set [theme] in config.toml to keep".to_string() };
        out.push(Item { label: format!("Theme: {name}"), detail, command: Command::SetTheme(name.clone()) });
    }
    out.push(Item { label: "Reload config".into(), detail: String::new(), command: Command::ReloadConfig });
    out.push(Item { label: "Extract built-in themes".into(), detail: "to ~/.config/arkos/themes".into(), command: Command::ExtractThemes });
    out
}

/// Case-insensitive subsequence match. Each matched char scores 1, plus 2 right after the
/// previous match, plus 1 at a word start; `None` when a query char is not found in order.
pub fn score(query: &str, label: &str) -> Option<i32> {
    let label: Vec<char> = label.to_lowercase().chars().collect();
    let mut total = 0;
    let mut pos = 0usize;
    let mut last: Option<usize> = None;
    for q in query.to_lowercase().chars() {
        let found = (pos..label.len()).find(|&i| label[i] == q)?;
        let word_start = found == 0 || label[found - 1] == ' ' || label[found - 1] == ':';
        let adjacent = found > 0 && last == Some(found - 1);
        total += 1 + if adjacent { 2 } else { 0 } + if word_start { 1 } else { 0 };
        last = Some(found);
        pos = found + 1;
    }
    Some(total)
}

/// Indices of the items that match, best first; ties keep the original order.
pub fn filter(query: &str, items: &[Item]) -> Vec<usize> {
    let mut scored: Vec<(i32, usize)> = items.iter().enumerate().filter_map(|(i, it)| score(query, &it.label).map(|s| (s, i))).collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, i)| i).collect()
}

pub struct Palette {
    pub query: LineEdit,
    /// Index into `visible`.
    pub selected: usize,
    pub items: Vec<Item>,
    pub visible: Vec<usize>,
    /// None only in tests.
    pub focus: Option<FocusHandle>,
}

impl Palette {
    pub fn new(items: Vec<Item>, focus: FocusHandle) -> Self {
        let mut p = Self { query: LineEdit::new(), selected: 0, items, visible: vec![], focus: Some(focus) };
        p.refilter();
        p
    }

    pub fn refilter(&mut self) {
        self.visible = filter(&self.query.text, &self.items);
        self.selected = 0;
    }

    pub fn step(&mut self, delta: isize) {
        let n = self.visible.len().min(MAX_ROWS);
        if n == 0 {
            return;
        }
        let n = n as isize;
        self.selected = (((self.selected as isize + delta) % n + n) % n) as usize;
    }

    pub fn selected_command(&self) -> Option<Command> {
        self.visible.get(self.selected).map(|i| self.items[*i].command.clone())
    }
}

/// The overlay: a query line and up to `MAX_ROWS` rows, centred near the top. A click outside
/// closes it; the keys go through `Root::palette_key`.
pub fn render(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let theme = root.theme.clone();
    let family = root.family();
    let p = root.palette.as_ref()?;
    let focus = p.focus.clone()?;
    let mut rows = div().flex().flex_col().mt(px(6.0));
    for (row, idx) in p.visible.iter().take(MAX_ROWS).enumerate() {
        let item = &p.items[*idx];
        let selected = row == p.selected;
        rows = rows.child(
            div()
                .id(("palette-row", row))
                .flex()
                .flex_row()
                .justify_between()
                .px(px(10.0))
                .py(px(5.0))
                .rounded(px(4.0))
                .bg(if selected { theme.chrome.tab_active.hsla() } else { theme.chrome.banner.hsla() })
                .border_1()
                .border_color(if selected { theme.chrome.focus_ring.hsla() } else { theme.chrome.banner.hsla() })
                .cursor_pointer()
                .on_click(cx.listener(move |root, _ev: &ClickEvent, window, cx| {
                    if let Some(p) = root.palette.as_mut() {
                        p.selected = row;
                    }
                    root.run_palette_selection(window, cx);
                }))
                .child(item.label.clone())
                .child(div().text_color(theme.chrome.text_dim.hsla()).child(item.detail.clone())),
        );
    }
    if p.visible.is_empty() {
        rows = rows.child(div().px(px(10.0)).py(px(5.0)).text_color(theme.chrome.text_dim.hsla()).child("no matching command"));
    }
    let panel = div()
        .id("palette")
        .key_context("Palette")
        .track_focus(&focus)
        .w(px(WIDTH))
        .p(px(10.0))
        .rounded(px(8.0))
        .bg(theme.chrome.banner.hsla())
        .border_1()
        .border_color(theme.chrome.border.hsla())
        .text_color(theme.chrome.text.hsla())
        .font_family(family.clone())
        .text_size(px(TEXT))
        .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _window, cx| cx.stop_propagation())
        .on_key_down(cx.listener(|root, ev: &KeyDownEvent, window, cx| root.palette_key(&ev.keystroke, window, cx)))
        .child(div().px(px(10.0)).py(px(6.0)).child(line_edit::render(&p.query, "type a command", &theme, family, TEXT)))
        .child(rows);
    Some(
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .justify_center()
            .items_start()
            .pt(px(60.0))
            .on_mouse_down(MouseButton::Left, cx.listener(|root, _: &MouseDownEvent, _window, cx| root.close_palette(cx)))
            .child(panel),
    )
}
```

`keymap::command_for` is new in `keymap.rs`. gpui's `on_action` needs a concrete action type per listener, so `attach` keeps its macro; this is the same mapping as a lookup, for the palette:

```rust
/// The command an action name runs, for the palette. The palette itself and quit are not listed.
pub fn command_for(name: &str) -> Option<Command> {
    Some(match name {
        "clear scrollback" => Command::ClearScrollback,
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
        _ => return None,
    })
}
```

(`"find"` exists from Task 9; add that arm there if this task lands first. Two tables that must agree, so pin them with a test in `keymap.rs`:

```rust
    #[test]
    fn every_action_has_a_palette_command_except_palette_and_quit() {
        for a in ACTIONS {
            let has = command_for(a.name).is_some();
            assert_eq!(has, a.name != "command palette" && a.name != "quit", "{}", a.name);
        }
    }
```

- [ ] **Step 5: Root owns the palette**

In `main.rs`:

1. `pub palette: Option<command_palette::Palette>` on `Root`, `None` at start. `mod command_palette;`.
2. Methods:

```rust
    pub fn toggle_palette(&mut self, cx: &mut Context<Self>) {
        if self.palette.is_some() {
            self.close_palette(cx);
            return;
        }
        let items = command_palette::items(&self.config, &theme::names(&paths::themes_dir()), &self.text_preset, &self.theme.name);
        self.palette = Some(command_palette::Palette::new(items, cx.focus_handle()));
        cx.notify();
    }

    pub fn close_palette(&mut self, cx: &mut Context<Self>) {
        self.palette = None;
        cx.notify();
    }

    pub fn palette_key(&mut self, ks: &gpui::Keystroke, window: &mut Window, cx: &mut Context<Self>) {
        if ks.modifiers.platform && ks.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                if let Some(p) = self.palette.as_mut() {
                    p.query.insert_str(&text);
                    p.refilter();
                    cx.notify();
                }
            }
            return;
        }
        let Some(p) = self.palette.as_mut() else { return };
        match p.query.handle(ks) {
            line_edit::Edit::Changed => p.refilter(),
            line_edit::Edit::Up => p.step(-1),
            line_edit::Edit::Down => p.step(1),
            line_edit::Edit::Submit | line_edit::Edit::SubmitBack => {
                self.run_palette_selection(window, cx);
                return;
            }
            line_edit::Edit::Cancel => {
                self.close_palette(cx);
                return;
            }
            line_edit::Edit::Unchanged => {}
        }
        cx.notify();
    }

    pub fn run_palette_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cmd = self.palette.as_ref().and_then(|p| p.selected_command());
        self.close_palette(cx);
        if let Some(cmd) = cmd {
            self.run(cmd, window, cx);
        }
    }
```

3. `run` arms: `CommandPalette => self.toggle_palette(cx)`, and

```rust
            SetTheme(name) => match theme::load(&name, &paths::themes_dir()) {
                Ok(t) => self.set_theme(t, cx),
                Err(e) => {
                    self.config_error = Some(format!("theme: {e}"));
                    cx.notify();
                }
            },
```

4. In `render`: focus goes to the palette while it is open: `let wanted = if let Some(f) = self.palette.as_ref().and_then(|p| p.focus.clone()) { Some(f) } else { match self.active_terminal(cx) { ... } };`. The overlay is the last child of the root div: `.children(command_palette::render(self, cx))` after the status bar (it is absolutely positioned, so it paints over everything; the root div needs `.relative()`).

- [ ] **Step 6: Run everything**

Run: `~/.cargo/bin/cargo test -p arkos 2>&1 | tail -15 && ~/.cargo/bin/cargo build -p arkos 2>&1 | grep -c warning`
Expected: all pass; `0`.

- [ ] **Step 7: Commit**

```bash
git add crates/arkos/src
git commit -m "Add the command palette with every action, preset and theme"
```

---

### Task 9: Find in the terminal (Cmd+F)

**Files:**
- Create: `crates/arkos/src/find.rs`
- Modify: `crates/arkos/src/terminal.rs` (search, highlights, reveal, `find` field, refresh after output), `crates/arkos/src/terminal_element.rs` (paint highlights), `crates/arkos/src/pane_view.rs` (the find bar, click-to-close), `crates/arkos/src/keymap.rs` (`Find` action and row, `Command::Find`, `command_for`), `crates/arkos/src/config.rs` (`DEFAULT_CONFIG` comment line), `crates/arkos/src/main.rs` (`mod find;`, the `Find` arm, focus)
- Test: unit tests in `terminal.rs` and `find.rs`

**Interfaces:**
- Consumes: `line_edit::{LineEdit, Edit, render}`, `keymap::tab_cycle`, `palette::Palette::{find_match, find_current}`, alacritty's `Term::search_next` and `term::search::RegexSearch`.
- Produces:
  - `terminal::Match` (re-export of `alacritty_terminal::term::search::Match`, a `RangeInclusive<Point>` in grid coordinates where history lines are negative).
  - `terminal::escape_regex(s: &str) -> String`: every regex metacharacter backslash-escaped.
  - `terminal::MAX_MATCHES: usize = 10_000`.
  - `TerminalCore::search(&self, needle: &str) -> Vec<Match>`: case-insensitive literal matches from the top of history to the bottom of the screen, in order, at most `MAX_MATCHES`; empty for an empty needle.
  - `terminal::Highlight { row: usize, start: usize, end: usize, current: bool }` (viewport row, inclusive columns); `TerminalCore::highlights(&self, matches: &[Match], current: usize) -> Vec<Highlight>`: the parts of each match on screen at the current display offset.
  - `TerminalCore::reveal_line(&mut self, line: Line)`: the smallest scroll that puts `line` on screen; a visible line scrolls nothing.
  - `find::FindState { query: LineEdit, matches: Vec<Match>, current: usize, focus: FocusHandle }`; `Terminal { find: Option<FindState> }`; `Terminal::open_find(cx)` (opens, or refocuses when open), `Terminal::close_find(cx)`, `Terminal::find_key(&mut self, ks: &Keystroke, cx) -> bool` (true when the bar closed), `Terminal::find_paste(text, cx)`, `Terminal::find_step(delta: isize, cx)`.
  - `find::initial_match(len: usize) -> usize`: the newest match (`len - 1`, or 0).
  - `find::render(terminal: &Entity<Terminal>, theme: &Theme, family: SharedString, cx: &mut Context<Root>) -> Option<Div>`: the bar at the top right of a terminal whose find is open.
  - `keymap::ACTIONS` gains `("find", "cmd-f")` after `command palette`; action `Find`; `Command::Find`.

Rulings: the search is literal and case-insensitive (what Cmd+F means in every Mac app; regex can come later as a toggle). A new query selects the newest match, since the thing just printed is what one usually looks for, and scrolls to it. Output arriving while the bar is open re-runs the search (the grid changed under it) but does not scroll. Highlights are painted as translucent quads over the text so no run has to be re-shaped.

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `terminal.rs`:

```rust
    #[test]
    fn escape_makes_metacharacters_literal() {
        assert_eq!(escape_regex("a.b*c"), "a\\.b\\*c");
        assert_eq!(escape_regex("(x)|[y]{2}^$\\"), "\\(x\\)\\|\\[y\\]\\{2\\}\\^\\$\\\\");
        assert_eq!(escape_regex("plain words"), "plain words");
        let mut core = TerminalCore::new(20, 3, 100);
        core.apply_output(b"a.b ab a*b");
        let dots = core.search(".");
        assert_eq!(dots.len(), 1);
        assert_eq!((dots[0].start().line.0, dots[0].start().column.0), (0, 1));
        assert_eq!(core.search("*").len(), 1);
        assert_eq!(core.search("(").len(), 0, "no panic, no match");
        assert_eq!(core.search("\\").len(), 0);
        assert_eq!(core.search("").len(), 0);
    }

    #[test]
    fn search_is_literal_and_case_insensitive() {
        let mut core = TerminalCore::new(30, 3, 100);
        core.apply_output(b"Hello hello HELLO");
        let m = core.search("hello");
        let cols: Vec<usize> = m.iter().map(|m| m.start().column.0).collect();
        assert_eq!(cols, vec![0, 6, 12]);
        assert_eq!(m[0].end().column.0, 4, "the match covers the whole word");
        assert_eq!(core.search("HeLLo").len(), 3);
        assert_eq!(core.search(" hello").len(), 2, "a space is part of the query (blank cells are spaces too)");
        assert_eq!(core.search("xyz").len(), 0);
    }

    #[test]
    fn search_covers_history_in_order_and_stops_after_one_pass() {
        let mut core = TerminalCore::new(10, 2, 100);
        core.apply_output(b"ab\r\nab\r\nab\r\nab");
        assert_eq!(core.history_size(), 2);
        let m = core.search("ab");
        let lines: Vec<i32> = m.iter().map(|m| m.start().line.0).collect();
        assert_eq!(lines, vec![-2, -1, 0, 1], "oldest first, no wrap-around duplicates");
        let one = TerminalCore::new(10, 2, 100);
        assert!(one.search("ab").is_empty());
        let mut single = TerminalCore::new(10, 2, 100);
        single.apply_output(b"xx ab xx");
        assert_eq!(single.search("ab").len(), 1, "a lone match is found once, not forever");
    }

    #[test]
    fn highlights_follow_the_viewport_and_reveal_scrolls_the_least() {
        use alacritty_terminal::index::Line;
        let mut core = TerminalCore::new(10, 2, 100);
        core.apply_output(b"ab\r\nab\r\nab\r\nab");
        let m = core.search("ab");
        let h = core.highlights(&m, 3);
        assert_eq!(h.len(), 2, "only the two screen lines are visible at offset 0");
        assert_eq!(h[0], Highlight { row: 0, start: 0, end: 1, current: false });
        assert_eq!(h[1], Highlight { row: 1, start: 0, end: 1, current: true });
        core.reveal_line(Line(1));
        assert_eq!(core.display_offset(), 0, "a visible line scrolls nothing");
        core.reveal_line(Line(-2));
        assert_eq!(core.display_offset(), 2, "the oldest line comes to the top");
        let h = core.highlights(&m, 0);
        assert_eq!(h[0], Highlight { row: 0, start: 0, end: 1, current: true });
        assert_eq!(h[1], Highlight { row: 1, start: 0, end: 1, current: false });
        core.reveal_line(Line(1));
        assert_eq!(core.display_offset(), 0, "the newest line comes to the bottom");
        core.reveal_line(Line(-1));
        assert_eq!(core.display_offset(), 1, "the smallest scroll that shows it");
        assert_eq!(core.scroll.current, 1.0, "the animator lands with it, no glide");
    }
```

Create `crates/arkos/src/find.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_search_starts_at_the_newest_match() {
        assert_eq!(initial_match(0), 0);
        assert_eq!(initial_match(1), 0);
        assert_eq!(initial_match(5), 4);
        assert_eq!(count_text(0, 0), "no matches");
        assert_eq!(count_text(0, 1), "1 of 1");
        assert_eq!(count_text(4, 5), "5 of 5");
    }
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `~/.cargo/bin/cargo test -p arkos escape_makes search_is search_covers highlights_follow newest_match 2>&1 | tail -20`
Expected: compile errors.

- [ ] **Step 3: The search in TerminalCore**

In `terminal.rs`, add imports:

```rust
use alacritty_terminal::index::{Column, Direction, Line, Point, Side as GridSide};
use alacritty_terminal::term::search::RegexSearch;
pub use alacritty_terminal::term::search::Match;
```

(`Line` is already imported; merge.) Add:

```rust
pub const MAX_MATCHES: usize = 10_000;

/// Backslash-escapes every regex metacharacter so the query matches literally.
pub fn escape_regex(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A match's part on one viewport row, columns inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Highlight {
    pub row: usize,
    pub start: usize,
    pub end: usize,
    pub current: bool,
}
```

and in `impl TerminalCore`:

```rust
    /// Every literal, case-insensitive match of `needle`, oldest first, from the top of history
    /// to the bottom of the screen. alacritty's search wraps around the buffer, so the loop stops
    /// when a match comes back at or before the previous one.
    pub fn search(&self, needle: &str) -> Vec<Match> {
        if needle.is_empty() {
            return Vec::new();
        }
        let Ok(mut regex) = RegexSearch::new(&format!("(?i){}", escape_regex(needle))) else { return Vec::new() };
        let last_col = self.term.last_column();
        let end_of_grid = Point::new(self.term.bottommost_line(), last_col);
        let mut origin = Point::new(self.term.topmost_line(), Column(0));
        let mut out: Vec<Match> = Vec::new();
        while let Some(m) = self.term.search_next(&mut regex, origin, Direction::Right, GridSide::Left, None) {
            if out.last().map_or(false, |prev| *m.start() <= *prev.start()) {
                break;
            }
            let end = *m.end();
            out.push(m);
            if end >= end_of_grid || out.len() >= MAX_MATCHES {
                break;
            }
            origin = if end.column < last_col { Point::new(end.line, Column(end.column.0 + 1)) } else { Point::new(Line(end.line.0 + 1), Column(0)) };
        }
        out
    }

    /// The on-screen parts of `matches` at the current display offset. `current` is the index
    /// of the match the find bar points at.
    pub fn highlights(&self, matches: &[Match], current: usize) -> Vec<Highlight> {
        let off = self.display_offset() as i32;
        let screen = self.screen_lines() as i32;
        let cols = self.columns();
        let mut out = Vec::new();
        for (i, m) in matches.iter().enumerate() {
            let (s, e) = (m.start(), m.end());
            for line in s.line.0..=e.line.0 {
                let row = line + off;
                if row < 0 || row >= screen {
                    continue;
                }
                let start = if line == s.line.0 { s.column.0 } else { 0 };
                let end = if line == e.line.0 { e.column.0 } else { cols.saturating_sub(1) };
                out.push(Highlight { row: row as usize, start, end, current: i == current });
            }
        }
        out
    }

    /// Scrolls just enough to show `line` (history lines are negative). The animator lands with
    /// the grid, so there is no glide: a find jump should not animate across 10,000 lines.
    pub fn reveal_line(&mut self, line: Line) {
        let screen = self.screen_lines() as i32;
        let off = self.display_offset() as i32;
        let (top, bottom) = (-off, screen - 1 - off);
        let want = if line.0 < top {
            -line.0
        } else if line.0 > bottom {
            screen - 1 - line.0
        } else {
            off
        };
        let want = want.clamp(0, self.history_size() as i32) as usize;
        self.set_display_offset(want);
        self.scroll.jump_to(want as f32);
    }
```

If `Point` has no `new` in alacritty 0.26, build it as `Point { line, column }`. If `Term::last_column` is not in scope, it comes from the `Dimensions` trait (already imported). If `search_next` returns the match at the origin twice (the test `search_covers_history_in_order_and_stops_after_one_pass` would see a duplicate), the `<= prev.start()` guard already stops; if instead it skips the match at column 0 of the first line (the `escape_makes_metacharacters_literal` test's column assertion would fail), change the origin's side to `GridSide::Right` and the origin to one cell before the start: note which in the commit message.

- [ ] **Step 4: The find state on the terminal**

Above the tests in `find.rs`:

```rust
//! Find in a terminal (spec 4: Cmd+F). The state lives on the `Terminal`, the search runs in
//! `TerminalCore`, and the bar is drawn at the top right of the pane.
use crate::keymap::tab_cycle;
use crate::line_edit::{self, Edit, LineEdit};
use crate::terminal::{Match, Terminal};
use crate::theme::Theme;
use crate::Root;
use gpui::{div, prelude::*, px, ClickEvent, Context, Div, Entity, FocusHandle, KeyDownEvent, Keystroke, MouseButton, MouseDownEvent, SharedString};

pub struct FindState {
    pub query: LineEdit,
    pub matches: Vec<Match>,
    /// Index into `matches`; meaningless when empty.
    pub current: usize,
    pub focus: FocusHandle,
}

/// A new query points at the newest match: what was just printed is what one looks for.
pub fn initial_match(len: usize) -> usize {
    len.saturating_sub(1)
}

pub fn count_text(current: usize, len: usize) -> String {
    if len == 0 {
        "no matches".to_string()
    } else {
        format!("{} of {len}", current + 1)
    }
}

impl Terminal {
    pub fn open_find(&mut self, cx: &mut Context<Self>) {
        if self.find.is_none() {
            self.find = Some(FindState { query: LineEdit::new(), matches: Vec::new(), current: 0, focus: cx.focus_handle() });
        }
        cx.notify();
    }

    pub fn close_find(&mut self, cx: &mut Context<Self>) {
        self.find = None;
        cx.notify();
    }

    /// Re-runs the search for the current query, points at the newest match and scrolls to it.
    fn find_changed(&mut self, cx: &mut Context<Self>) {
        let Some(f) = self.find.as_mut() else { return };
        f.matches = self.core.search(&f.query.text);
        f.current = initial_match(f.matches.len());
        if let Some(m) = f.matches.get(f.current) {
            let line = m.start().line;
            self.core.reveal_line(line);
        }
        cx.notify();
    }

    /// Output changed the grid: search again, keep the position, do not scroll.
    pub fn refresh_find(&mut self) {
        if let Some(f) = self.find.as_mut() {
            f.matches = self.core.search(&f.query.text);
            f.current = f.current.min(initial_match(f.matches.len()));
        }
    }

    pub fn find_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(f) = self.find.as_mut() else { return };
        if f.matches.is_empty() {
            return;
        }
        f.current = tab_cycle(f.current, f.matches.len(), delta);
        let line = f.matches[f.current].start().line;
        self.core.reveal_line(line);
        cx.notify();
    }

    pub fn find_paste(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(f) = self.find.as_mut() {
            f.query.insert_str(text);
            self.find_changed(cx);
        }
    }

    /// Enter is next, Shift+Enter previous, Escape closes. Returns true when the bar closed.
    pub fn find_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(f) = self.find.as_mut() else { return true };
        match f.query.handle(ks) {
            Edit::Changed => self.find_changed(cx),
            Edit::Submit => self.find_step(1, cx),
            Edit::SubmitBack => self.find_step(-1, cx),
            Edit::Cancel => {
                self.close_find(cx);
                return true;
            }
            Edit::Up => self.find_step(-1, cx),
            Edit::Down => self.find_step(1, cx),
            Edit::Unchanged => {}
        }
        false
    }
}

/// The bar: query, count, close. Keys go to the terminal's `find_key`; Cmd+V pastes.
pub fn render(terminal: &Entity<Terminal>, theme: &Theme, family: SharedString, cx: &mut Context<Root>) -> Option<Div> {
    let (query, count, focus) = {
        let t = terminal.read(cx);
        let f = t.find.as_ref()?;
        (f.query.clone(), count_text(f.current, f.matches.len()), f.focus.clone())
    };
    let t_keys = terminal.clone();
    let t_close = terminal.clone();
    Some(
        div()
            .id(("find", terminal.entity_id()))
            .absolute()
            .top(px(6.0))
            .right(px(14.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .px(px(10.0))
            .py(px(5.0))
            .rounded(px(6.0))
            .bg(theme.chrome.banner.hsla())
            .border_1()
            .border_color(theme.chrome.border.hsla())
            .text_color(theme.chrome.text.hsla())
            .font_family(family.clone())
            .text_size(px(12.0))
            .track_focus(&focus)
            .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _window, cx| cx.stop_propagation())
            .on_key_down(cx.listener(move |_root, ev: &KeyDownEvent, _window, cx| {
                let ks = &ev.keystroke;
                if ks.modifiers.platform && ks.key == "v" {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        t_keys.update(cx, |t, cx| t.find_paste(&text, cx));
                    }
                    return;
                }
                if ks.modifiers.platform {
                    return;
                }
                t_keys.update(cx, |t, cx| {
                    t.find_key(ks, cx);
                });
            }))
            .child(div().w(px(220.0)).child(line_edit::render(&query, "find", theme, family, 12.0)))
            .child(div().text_color(theme.chrome.text_dim.hsla()).child(count))
            .child(
                div()
                    .id(("find-close", terminal.entity_id()))
                    .text_color(theme.chrome.text_dim.hsla())
                    .hover(|s| s.text_color(theme.chrome.text.hsla()))
                    .cursor_pointer()
                    .child("×")
                    .on_click(cx.listener(move |_root, _ev: &ClickEvent, _window, cx| t_close.update(cx, |t, cx| t.close_find(cx)))),
            ),
    )
}
```

`LineEdit` derives `Clone` (Task 7) so the query can be read out of the entity for rendering. `Entity::entity_id()` gives a stable id for the element; if the name differs, use `terminal.read(cx).session.0` as the integer part as the jump pill does.

In `terminal.rs`: `pub find: Option<crate::find::FindState>` on `Terminal`, `None` in `new`; at the end of `flush_output`, before `true`, add `self.refresh_find();`.

- [ ] **Step 5: Paint, bar, keys, focus**

`terminal_element.rs`: inside the `terminal.update` closure in `paint`, after `set_display_offset`, compute `let highlights = t.find.as_ref().map(|f| t.core.highlights(&f.matches, f.current)).unwrap_or_default();` and return it in the tuple; after the rows and the cursor are painted (inside `with_content_mask`):

```rust
            for h in &highlights {
                let x = origin.x + px(h.start as f32 * cw);
                let w = px((h.end + 1 - h.start) as f32 * cw);
                let y = origin.y + px((h.row as f32 + frac) * cell.height);
                let c = if h.current { palette.find_current } else { palette.find_match };
                window.paint_quad(fill(Bounds { origin: point(x, y), size: size(w, lh) }, c.alpha(0.45)));
            }
```

(`palette` is the `Palette` clone or the `bg`/`cursor`/`find_*` values returned from the update closure in Task 4.)

`pane_view.rs` `render_terminal`: after the jump pill, `.children(find::render(&terminal, &theme, family.clone(), cx))`; in the terminal div's mouse handling, add `.on_mouse_down(MouseButton::Left, cx.listener(move |_root, _ev: &MouseDownEvent, _window, cx| { t_click.update(cx, |t, cx| { if t.find.is_some() { t.close_find(cx); } }); }))` with `let t_click = terminal.clone();`, so a click back into the terminal closes the bar.

`keymap.rs`: `Find` in `actions!`, `Command::Find`, `row("find", "cmd-f")` after `command palette`, the `binding` arm, the `attach` arm `Find => Command::Find`, `command_for` arm, the test table row `("cmd-f", "find", "Find")`. `config.rs`: the line `# "find" = "cmd-f"` in `DEFAULT_CONFIG` after `command palette`.

`main.rs`: `mod find;`; in `run`:

```rust
            Find => {
                if let Some(t) = self.active_terminal(cx) {
                    t.update(cx, |t, cx| t.open_find(cx));
                }
            }
```

In `render`, the focus choice becomes: the palette's handle if open; else the active terminal's find handle if its `find` is open; else the terminal's handle; else the pane's.

- [ ] **Step 6: Run everything**

Run: `~/.cargo/bin/cargo test -p arkos 2>&1 | tail -15 && ~/.cargo/bin/cargo build -p arkos 2>&1 | grep -c warning`
Expected: all pass; `0`.

- [ ] **Step 7: Commit**

```bash
git add crates/arkos/src
git commit -m "Find in the terminal with Cmd+F, literal and case-insensitive"
```

---

### Task 10: The tab-bar overflow menu and a bar that keeps the active tab in view

**Files:**
- Modify: `crates/arkos/src/pane_view.rs`, `crates/arkos/src/main.rs`
- Test: unit tests in `pane_view.rs`

**Interfaces:**
- Consumes: `gpui::ScrollHandle` (`div.rs:3068`, `scroll_to_item`), `Root::activate_tab`.
- Produces:
  - `pane_view::show_overflow(tabs: usize) -> bool`: true from two tabs.
  - `pane_view::menu_rows(p: &Pane, labels: &[String]) -> Vec<(usize, String, bool)>`: (index, label, is_active) for every tab.
  - `Root { bar_scroll: HashMap<PaneId, ScrollHandle>, tab_menu: Option<PaneId> }`; `Root::forget_pane` drops the pane's handle; `Root::activate_tab` scrolls the bar to the tab; `Root::toggle_tab_menu(pane, cx)`, `Root::close_tab_menu(cx)`.

The spec puts "plus button and overflow menu at the right of the active pane". The bar becomes three parts: the pane number, a scrolling strip of tabs, and a fixed right group with `+` and, from two tabs, `⋯`. The menu lists every tab; a click activates it; a click anywhere else closes the menu.

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `pane_view.rs`:

```rust
    #[test]
    fn overflow_menu_lists_every_tab_from_two() {
        use crate::tree::{Pane, Tab};
        use protocol::SessionId;
        assert!(!show_overflow(0) && !show_overflow(1) && show_overflow(2));
        let p = Pane { id: 1, tabs: vec![Tab::Terminal { session: SessionId(1) }, Tab::Terminal { session: SessionId(2) }], active: 1, mru: vec![1, 0], ids: vec![2, 3] };
        let rows = menu_rows(&p, &["zsh".into(), "vim".into()]);
        assert_eq!(rows, vec![(0, "zsh".to_string(), false), (1, "vim".to_string(), true)]);
    }
```

- [ ] **Step 2: Run the test to see it fail**

Run: `~/.cargo/bin/cargo test -p arkos overflow_menu 2>&1 | tail -10`
Expected: compile errors.

- [ ] **Step 3: Implement**

In `pane_view.rs`:

```rust
/// The overflow menu is worth a button once there is something to choose between.
pub fn show_overflow(tabs: usize) -> bool {
    tabs >= 2
}

/// (index, label, active) for the menu, in bar order.
pub fn menu_rows(p: &Pane, labels: &[String]) -> Vec<(usize, String, bool)> {
    labels.iter().enumerate().map(|(i, l)| (i, l.clone(), i == p.active)).collect()
}
```

Restructure `render_tab_bar`:

1. Compute every tab's label first into `let labels: Vec<String>` (the existing per-tab `label` code, collected), then build the tab elements from `labels`.
2. The bar div keeps its id, height, background, drag listeners and the number child. After the number, add the strip: `div().id(("tabs", pane)).flex_1().min_w_0().h_full().flex().flex_row().overflow_x_scroll().track_scroll(&handle)` holding the tab children, where `let handle = root.bar_scroll.entry(pane).or_default().clone();` (`ScrollHandle: Default`).
3. After the strip, the fixed group: the existing `+` child, then when `show_overflow(p.tabs.len())`:

```rust
        .child(
            div()
                .id(("more", pane))
                .px(px(8.0))
                .text_color(theme.chrome.text_dim.hsla())
                .hover(|s| s.text_color(theme.chrome.text.hsla()))
                .cursor_pointer()
                .child("⋯")
                .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _window, cx| cx.stop_propagation())
                .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| root.toggle_tab_menu(pane, cx))),
        )
```

(the `stop_propagation` on mouse-down keeps the root's close-on-click from closing the menu the moment it opens).

4. In `render_pane`, after `.child(content)` on the outer pane div, add the menu when `root.tab_menu == Some(pane)`:

```rust
    let menu = (root.tab_menu == Some(pane)).then(|| {
        let mut m = div()
            .id(("menu", pane))
            .absolute()
            .top(px(TAB_BAR_H))
            .right_0()
            .min_w(px(180.0))
            .flex()
            .flex_col()
            .py(px(4.0))
            .bg(theme.chrome.banner.hsla())
            .border_1()
            .border_color(theme.chrome.border.hsla())
            .font_family(family.clone())
            .text_size(px(12.0))
            .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _window, cx| cx.stop_propagation());
        for (idx, label, active) in menu_rows(&p, &labels) {
            m = m.child(
                div()
                    .id(tab_id("menu-row", pane, idx))
                    .px(px(10.0))
                    .py(px(4.0))
                    .bg(if active { theme.chrome.tab_active.hsla() } else { theme.chrome.banner.hsla() })
                    .text_color(if active { theme.chrome.text.hsla() } else { theme.chrome.text_dim.hsla() })
                    .cursor_pointer()
                    .child(label)
                    .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                        root.close_tab_menu(cx);
                        root.activate_tab(pane, idx, cx);
                    })),
            );
        }
        m
    });
```

The outer pane div needs `.relative()` so the menu's `absolute` is relative to the pane, and `.children(menu)` after `.child(content)` so it paints over the body. `labels` must be computed once in `render_pane` and passed into `render_tab_bar` (change its signature to take `labels: &[String]`).

In `main.rs`:

```rust
    pub bar_scroll: HashMap<PaneId, gpui::ScrollHandle>,
    pub tab_menu: Option<PaneId>,
```

(`HashMap::new()` and `None` in `new`; `forget_pane` removes `bar_scroll` and clears `tab_menu` if it names the pane). `activate_tab` adds, before `self.save(cx)`:

```rust
        if let Some(h) = self.bar_scroll.get(&pane) {
            h.scroll_to_item(idx);
        }
```

and:

```rust
    pub fn toggle_tab_menu(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        self.tab_menu = if self.tab_menu == Some(pane) { None } else { Some(pane) };
        cx.notify();
    }

    pub fn close_tab_menu(&mut self, cx: &mut Context<Self>) {
        if self.tab_menu.take().is_some() {
            cx.notify();
        }
    }
```

The root div in `render` gains `.on_mouse_down(MouseButton::Left, cx.listener(|root, _ev: &MouseDownEvent, _window, cx| root.close_tab_menu(cx)))` (any click not stopped by the menu or its button closes it). Keyboard tab changes (`CycleTab`, `MruTab`) go through `activate_tab`, so they scroll the bar too.

- [ ] **Step 4: Run everything**

Run: `~/.cargo/bin/cargo test -p arkos 2>&1 | tail -15 && ~/.cargo/bin/cargo build -p arkos 2>&1 | grep -c warning`
Expected: all pass; `0`.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src
git commit -m "Add the tab-bar overflow menu and keep the active tab in view"
```

---

### Task 11: Milestone 2a carry-overs, the README, and three spec corrections

**Files:**
- Modify: `crates/arkos/src/sessions.rs`, `crates/arkos/src/main.rs`, `README.md`, `docs/superpowers/specs/2026-10-13-arkos-design.md`
- Test: unit tests in `sessions.rs` and `main.rs`

**Interfaces:**
- Produces:
  - `SessionManager::handle_response`: a `Kill` answered `Error("no session N")` is `Outcome::Killed` (logged, no banner): the daemon reaped it first, which is the outcome a kill wanted.
  - `Root::close_tab`: a tab whose shell has ended is killed at once and never enters the closed stack; Cmd+Shift+T has nothing to bring back from it.
  - `Root::own_grid` prefers the pane's canvas measurement (taken at the workspace size) over the terminal's current grid, so a restore into a pane whose terminal was nudged with Cmd+= parses at the size the restored tab will actually get.
  - README: milestone 2b status, the keyboard table in Apple's modifier order with the three new rows, a "Config" section, and the human checklist.
  - Spec: sections 2.1, 2.3 and 3 corrected to what the code does.

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `sessions.rs`:

```rust
    #[test]
    fn a_kill_of_a_reaped_session_is_a_quiet_success() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r.clone(), None, PathBuf::from("/p"));
        m.kill(SessionId(9));
        assert_eq!(m.handle_response(1, Response::Error("no session 9".into())), Outcome::Killed { session: SessionId(9) });
        m.kill(SessionId(10));
        assert_eq!(m.handle_response(2, Response::Error("permission denied".into())), Outcome::Error("permission denied".into()), "other errors still surface");
    }
```

Append to the `tests` module in `main.rs`:

```rust
    #[test]
    fn a_pane_measurement_outranks_its_terminals_grid() {
        use super::grid_choice;
        // The canvas measures at the workspace font; the terminal may have been nudged.
        assert_eq!(grid_choice(Some((100, 30)), Some((80, 24))), Some((100, 30)));
        assert_eq!(grid_choice(None, Some((80, 24))), Some((80, 24)));
        assert_eq!(grid_choice(None, None), None);
    }
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `~/.cargo/bin/cargo test -p arkos quiet_success outranks 2>&1 | tail -10`
Expected: the first fails (`Outcome::Error`), the second does not compile.

- [ ] **Step 3: Implement the three fixes**

`sessions.rs`, in `handle_response`'s `if let Response::Error(e) = &response` block, add before the `_ => {}` arm:

```rust
                Some(Pending::Kill { session }) if e.starts_with("no session") => {
                    // Reaped before the kill arrived: the session is gone, which is what we asked.
                    eprintln!("arkos: session {} was already gone when killed", session.0);
                    return Outcome::Killed { session };
                }
```

`main.rs`:

```rust
/// A pane's own measurement first (it is taken at the workspace font size), else the grid of
/// the terminal it shows (which Cmd+= may have nudged).
fn grid_choice(measured: Option<(usize, usize)>, terminal: Option<(usize, usize)>) -> Option<(usize, usize)> {
    measured.or(terminal)
}
```

and `own_grid` becomes:

```rust
    fn own_grid(&self, pane: PaneId, cx: &App) -> Option<(usize, usize)> {
        let p = self.tree.pane(pane)?;
        let terminal = match p.tabs.get(p.active) {
            Some(Tab::Terminal { session }) => self.sessions.terminals.get(session).map(|t| {
                let core = &t.read(cx).core;
                (core.columns(), core.screen_lines())
            }),
            _ => None,
        };
        grid_choice(self.grids.get(&pane).copied(), terminal)
    }
```

In `close_tab`, inside `if session != SessionId(0) {`, before the `is_restoring` check:

```rust
                let ended = self.sessions.terminals.get(&session).map(|t| !t.read(cx).alive).unwrap_or(false);
                if ended {
                    // Nothing to bring back: end the session now instead of a minute from now.
                    eprintln!("arkos: closing ended session {}", session.0);
                    self.sessions.kill(session);
                    self.sessions.detach(session);
                    self.sessions.tab_closed(id);
                    self.save(cx);
                    cx.notify();
                    return;
                }
```

- [ ] **Step 4: README**

In `README.md`:

1. Replace the Status section's first paragraph with:

```markdown
Milestone 2b of 6: a config file with live reload, the Navy and Navy Light themes following the
macOS appearance, three text presets, a command palette (Cmd+Shift+P), find (Cmd+F), and the
tab-bar overflow menu. Milestone 3 (Claude Code session awareness) is next.
```

2. Replace the human checklist with these items (the 2a items that still matter are kept):

```markdown
Things only a person at the screen can check, in order of value:

- editing `~/.config/arkos/config.toml` while the app runs: `preset = "relaxed"` resizes every
  terminal within a second; a typo shows a banner with the line number and nothing else changes;
  fixing it clears the banner
- System Settings > Appearance: Light switches to Navy Light and back, history included
- `mode = "dark"` pins Navy whatever the system says
- Cmd+Shift+P: typing filters, Up/Down and Enter run a command, Escape and a click outside close it
- "Text preset: compact" in the palette, then Cmd+= on one pane: only that pane grows; choosing
  a preset again resets it; the preset survives a relaunch of that folder
- "Theme: navy-light" in the palette, then a config reload: the config's theme wins
- "Extract built-in themes", then editing `~/.config/arkos/themes/navy.toml`'s `window_bg` and
  reloading: the window follows the file; renaming it to `mine.toml` and setting `dark = "mine"`
- `"split right" = "cmd-d"` under `[keybindings]`: Cmd+D splits, Cmd+\ does not; `""` unbinds
- Cmd+F in a pane with history: typing highlights every match and jumps to the newest; Enter
  and Shift+Enter step with the count; Escape or a click in the terminal closes it; `.` and `*`
  match literally; output arriving keeps the bar open
- a pane with eight tabs: the bar scrolls, Cmd+Option+Right keeps the active tab in view, and
  the ⋯ menu lists them all
- closing a tab whose shell has ended: `ps` shows it gone at once, not a minute later
- Cmd+, opens the config file in the default text editor
- typing in each pane, and that Cmd combinations never reach the shell
- dragging a tab to each of the five drop zones, within a bar, and out of a pane
- Cmd+K alone clearing the scrollback while the Cmd+K chords still work
- the empty pane's animation holding still after turning on Reduce Motion and saving the config
- after `kill -9`: the layout, every session, replayed history, the text preset, and typing
- the window minimized for two minutes keeps its sessions: compare the `arkos` pid before and
  after, and look for no "relaunching" line in `~/Library/Application Support/arkos/arkosd.log`
- `ps` after thirty seconds idle with four terminals
```

3. The keyboard table: rename the modifier order to Apple's (`Shift+Cmd+]`, `Option+Cmd+Right`, `Ctrl+Cmd+Left`, `Shift+Cmd+T`, `Ctrl+Shift+\``, `Cmd+K then Shift+Cmd+Left` and so on) and add three rows after `font smaller`:

```markdown
| Cmd+F | find in the terminal |
| Cmd+, | open config file |
| Shift+Cmd+P | command palette |
```

Delete the note "Cmd+F, Cmd+,, Cmd+Shift+P, Cmd+B, Cmd+Shift+E and Ctrl+Shift+G are not bound until milestones 2b and 4." and add: "Cmd+B, Shift+Cmd+E and Ctrl+Shift+G arrive with the sidebar in milestone 4. Every binding can be changed in the config file; see Config."

4. Add a section after Keyboard:

```markdown
## Config

`~/.config/arkos/config.toml` (or `$XDG_CONFIG_HOME/arkos/config.toml`) is written with
commented defaults on first run and opened by Cmd+, in your text editor. It is watched: a saved
edit applies within a second. A bad edit shows a banner with the line number and keeps the last
good settings. Sections: `font` (family, the default text preset, and the presets), `terminal`
(scrollback, shell, nice), `keybindings` (action name to keys, `""` unbinds), `theme` (`dark`
and `light`), `appearance` (`system`, `dark` or `light`), `daemon` (applies when the window starts
the daemon), and `sidebar` and `browser` for later milestones.

Themes are one TOML file each in `~/.config/arkos/themes/`; `navy` and `navy-light` are built
in, and "Extract built-in themes" in the palette writes them there to copy from. A theme chosen
from the palette lasts until relaunch; the config file decides otherwise. Text presets are
`compact` (14 px, 1.3), `comfortable` (16 px, 1.5) and `relaxed` (18 px, 1.6); add your own under
`[font.presets.<name>]`. Each folder remembers its preset.
```

- [ ] **Step 5: Spec corrections**

In `docs/superpowers/specs/2026-10-13-arkos-design.md`:

1. Section 2.1, the sentence "Per session it keeps: id, title, cwd, shell, pid, alive flag, last activity time, nice level, Claude state (section 5), and a **replay buffer**" becomes "Per session it keeps: id, title, cwd, pid, alive flag, last activity time, Claude state (section 5), and a **replay buffer**". Add after that paragraph: "The shell and nice level are fields of the CreateSession request, applied when the shell starts; they are not part of the session record."
2. Section 2.3, the SessionEvent row's payload "session id, one of: exited(code), title(string), state(ClaudeState), bell" becomes "session id, one of: exited(code), state(ClaudeState). Title changes and the bell are escape sequences in the output, which the UI's emulator parses; the daemon never parses output (section 2.1)."
3. Section 3, "typical sessions are far smaller because blank cells are not stored" becomes "alacritty stores every cell of every line, so memory follows columns times lines: about 3 MB per 1,000 lines at 124 columns, or about 60 MB for a full 20,000-line tab, which milestone 6 measures against the budget in section 9".

- [ ] **Step 6: Run everything, measure, commit**

Run: `~/.cargo/bin/cargo test --workspace 2>&1 | grep "test result" && ~/.cargo/bin/cargo build --release 2>&1 | grep -c warning`
Expected: every suite passes; `0`.

Memory, for the README's Status paragraph if it moved (release build, four terminals, thirty seconds idle, as in milestone 2a):

```bash
(cd /tmp && /Users/csarko/Projects/ide-arkos/target/release/arkos > /tmp/arkos-2b-final.log 2>&1 &); sleep 30
ps -o rss=,comm= -p "$(pgrep -x arkos)" "$(pgrep -x arkosd)"
pkill -x arkos
```

Expected: `arkos` under 120 MB, `arkosd` under 15 MB. If `arkos` grew by more than 5 MB over milestone 2a's 85 MB, note it in the commit message for milestone 6.

```bash
git add crates/arkos/src README.md docs/superpowers/specs/2026-10-13-arkos-design.md
git commit -m "Quiet kills of reaped sessions, immediate kill of ended tabs, README and spec fixes"
```

---

## Self-review notes

Spec coverage, section by section of what this plan claims:

- 4 "Tabs ... plus button and overflow menu at the right of the active pane": Task 10.
- 4 keyboard rows "find" and "Open config / command palette": Tasks 9, 1, 8. "Every binding lives in the config file and can be changed, chords included": Tasks 1, 2, 6.
- 7 "Config file": Tasks 2 (created with commented defaults, sections), 1 (Cmd+,), 6 (watched and applied live, banner with line number, last good config kept).
- 7 "Themes": Task 3 (one TOML file each, built-ins embedded and extractable, the flat colour sets), Task 4 (every colour drawn from it).
- 7 "Default theme (Navy)" and "Navy Light": Task 3, with the sampling step for the screenshot.
- 7 "Text presets": Task 5 (the table, user presets, Cmd+= still nudges, saved with the workspace), Task 8 (the palette command).
- 7.1 Appearance follows macOS: Task 4. Reduce Motion re-read: Task 6. Config comments carry the eye-examination note: Task 2.
- 10 testing: every pure part has unit tests; the window pieces are on the README checklist.
- Milestone 2a carry-overs (ledger): silent "no session" on Kill, immediate kill of ended tabs on close, restore grid from the pane's measurement, Reduce Motion on reload: Tasks 11, 11, 11, 6. Spec corrections from milestone 1: Task 11.

Not in this plan, on purpose: the "size for my distance" helper and the break reminder (milestone 6), the sidebar and browser sections' consumers (milestones 4 and 5), regex mode for find, selection and copy changes, the window minimum size versus banners (clamps keep it safe).

Type consistency checked across tasks: `Command` loses `Copy` in Task 1 and gains `TextPreset(String)` (5), `ReloadConfig`/`ExtractThemes` (6), `CommandPalette`/`SetTheme(String)` (8), `Find` (9); `Root::config_error` is introduced in Task 4 and used by 6, 8; `Palette.focus` is `Option<FocusHandle>` for the headless test in Task 8; `TerminalElement::new` takes `(terminal, font_size, line_height, family)` from Task 5 on; `grid_for` takes five arguments from Task 5 on; `keymap::keys_for` (Task 1) feeds `command_palette::items` (Task 8).
