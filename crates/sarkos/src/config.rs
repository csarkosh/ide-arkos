//! The config file (spec 7): `~/.config/sarkos/config.toml`, created with commented defaults on
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
    pub viewer: ViewerConfig,
    pub browser: BrowserConfig,
    pub daemon: DaemonConfig,
    pub theme: ThemeConfig,
    pub appearance: AppearanceConfig,
    pub recovery: RecoveryConfig,
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

/// The sidebar's defaults for a folder opened the first time; after that the folder's
/// workspace file keeps its own.
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

/// The viewer's limits: a file larger than `highlight_limit_bytes` opens without syntax colours.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ViewerConfig {
    pub highlight_limit_bytes: usize,
}

impl Default for ViewerConfig {
    fn default() -> Self {
        Self { highlight_limit_bytes: 5 * 1024 * 1024 }
    }
}

/// Milestone 5 reads this; parsed now so the file format does not change then.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BrowserConfig {
    pub url: String,
}

/// Passed to sarkosd when the window starts it; a running daemon keeps its own values.
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

/// Whether sarkosd starts at login to bring sessions back after a restart (update-in-place
/// spec, section 11): "ask" until the user runs "Start at login" from the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StartAtLogin {
    #[default]
    Ask,
    On,
    Off,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RecoveryConfig {
    pub start_at_login: StartAtLogin,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font: FontConfig::default(),
            terminal: TerminalConfig::default(),
            keybindings: BTreeMap::new(),
            sidebar: SidebarConfig::default(),
            viewer: ViewerConfig::default(),
            browser: BrowserConfig::default(),
            daemon: DaemonConfig::default(),
            theme: ThemeConfig::default(),
            appearance: AppearanceConfig::default(),
            recovery: RecoveryConfig::default(),
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
        if self.viewer.highlight_limit_bytes == 0 {
            return Err("viewer.highlight_limit_bytes must be at least 1".into());
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
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => match write_default(path) {
            Ok(()) => {
                eprintln!("sarkos: wrote default config to {}", path.display());
                Ok(Config::default())
            }
            // Created between the read and the write, as an editor's unlink-then-create save
            // does: read what it wrote.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => read(path),
            Err(e) => {
                eprintln!("sarkos: cannot write default config to {}: {e}", path.display());
                Ok(Config::default())
            }
        },
        Err(e) => Err(read_error(path, e)),
    }
}

fn read(path: &Path) -> Result<Config, ConfigError> {
    std::fs::read_to_string(path).map_err(|e| read_error(path, e)).and_then(|text| parse(&text))
}

fn read_error(path: &Path, e: std::io::Error) -> ConfigError {
    ConfigError { line: None, message: format!("cannot read {}: {e}", path.display()) }
}

/// Writes `DEFAULT_CONFIG` only if no file is there, so it can never replace one.
fn write_default(path: &Path) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::OpenOptions::new().write(true).create_new(true).open(path)?.write_all(DEFAULT_CONFIG.as_bytes())
}

/// A reload keeps the last good config when the new one does not parse (spec 7).
pub fn reload_decision(current: &Config, loaded: Result<Config, ConfigError>) -> (Config, Option<String>) {
    match loaded {
        Ok(cfg) => (cfg, None),
        Err(e) => (current.clone(), Some(e.to_string())),
    }
}

/// Written on first run. Every key is present with its default so an edit is a one-word change;
/// the comments carry spec 7.1's evidence notes, each a preference, never a health feature.
pub const DEFAULT_CONFIG: &str = r##"# Sarkos IDE configuration. Watched while the app runs: a saved edit applies at once, and a bad
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
# "clear to start" = "cmd-k"
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
# "command palette" = "cmd-shift-p"
# "find" = "cmd-f"
# "update now" = ""
# "start at login" = ""
# "install claude code hooks" = ""
# "toggle sidebar" = "cmd-b"
# "focus file tree" = "cmd-shift-e"
# "show ignored files" = ""
# "quit" = "cmd-q"

[sidebar]
# Width in px and whether it starts collapsed, for a folder opened for the first time; each folder then remembers its own.
width = 260.0
collapsed = false

[viewer]
# Files larger than this open in the viewer without syntax colours; binaries show a notice.
highlight_limit_bytes = 5242880

[browser]
# Milestone 5. The URL a new browser pane opens. Empty opens a blank page.
url = ""

[daemon]
# Read when the window starts sarkosd. A daemon that is already running keeps its values until
# `pkill -x sarkosd`, which ends every session.
# Seconds without a heartbeat from the window before the daemon relaunches it.
watchdog_secs = 30
# Bytes of output kept per session, replayed when a window reattaches.
replay_bytes = 16777216

[theme]
# Theme files live in ~/.config/sarkos/themes/<name>.toml. "navy" and "navy-light" are built in;
# "Extract built-in themes" in the command palette writes them there to copy from.
dark = "navy"
light = "navy-light"

[appearance]
# "system" follows macOS light and dark: the evidence points at matching the room, which the
# system setting already tracks. "dark" or "light" fixes one. No warm tint or blue-light mode is
# offered: both fail their controlled trials.
mode = "system"

[recovery]
# After a restart, sarkosd started at login brings every session back: the same folders, the
# history on screen under a divider, and each Claude Code conversation resumed (never continued:
# you send the next message). Everything stays on this Mac. "ask" shows a notice at launch until
# "Start at login" runs from the command palette; "on" installs the login item
# (~/Library/LaunchAgents/sh.csarko.sarkosd.plist) and keeps it; "off" removes it.
start_at_login = "ask"
"##;

/// The preset a workspace uses: what it saved, else the config's default, else comfortable.
pub fn resolve_preset(saved: Option<&str>, default: &str, presets: &BTreeMap<String, TextPreset>) -> (String, TextPreset) {
    for name in saved.into_iter().chain([default, "comfortable"]) {
        if let Some(p) = presets.get(name) {
            return (name.to_string(), *p);
        }
    }
    (default.to_string(), BUILTIN_PRESETS[1].1)
}

/// The preset to apply after `[font]` changed from `old` to `new` while the workspace uses
/// `current`; `None` leaves every terminal as it is, nudges included. An edited default is
/// applied now; otherwise only a change to the current preset (new values, or gone, when the
/// default takes over) applies one.
pub fn preset_after_reload(current: &str, old: &FontConfig, new: &FontConfig) -> Option<String> {
    if new.preset != old.preset {
        return Some(resolve_preset(None, &new.preset, &new.presets).0);
    }
    if new.presets.contains_key(current) && old.presets.get(current) == new.presets.get(current) {
        return None;
    }
    Some(resolve_preset(Some(current), &new.preset, &new.presets).0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recovery_section_defaults_to_ask_and_parses_on_and_off() {
        let c = Config::default();
        assert_eq!(c.recovery.start_at_login, StartAtLogin::Ask);
        let on: Config = toml::from_str("[recovery]\nstart_at_login = \"on\"\n").unwrap();
        assert_eq!(on.recovery.start_at_login, StartAtLogin::On);
        let off: Config = toml::from_str("[recovery]\nstart_at_login = \"off\"\n").unwrap();
        assert_eq!(off.recovery.start_at_login, StartAtLogin::Off);
        assert!(toml::from_str::<Config>("[recovery]\nstart_at_login = true\n").is_err(), "a string, like appearance.mode");
        let parsed: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        assert_eq!(parsed.recovery.start_at_login, StartAtLogin::Ask, "the written defaults say ask");
    }

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

    #[test]
    fn a_reload_reapplies_a_preset_only_when_the_current_one_changed() {
        let old = FontConfig::default();
        // Only the family: no preset to apply, so per-terminal nudges survive.
        let family = FontConfig { family: "SF Mono".into(), ..FontConfig::default() };
        assert_eq!(preset_after_reload("relaxed", &old, &family), None);
        // Another preset changed or was added: the current one is untouched.
        let mut other = FontConfig::default();
        other.presets.insert("compact".into(), TextPreset { size: 13.0, line_height: 1.3 });
        other.presets.insert("big".into(), TextPreset { size: 24.0, line_height: 1.5 });
        assert_eq!(preset_after_reload("relaxed", &old, &other), None);
        // The current preset's values changed: it is applied again under the same name.
        let mut changed = FontConfig::default();
        changed.presets.insert("relaxed".into(), TextPreset { size: 20.0, line_height: 1.6 });
        assert_eq!(preset_after_reload("relaxed", &old, &changed).as_deref(), Some("relaxed"));
        // The current preset was renamed away: the config's default takes over.
        let mut with_big = FontConfig { preset: "compact".into(), ..FontConfig::default() };
        with_big.presets.insert("big".into(), TextPreset { size: 24.0, line_height: 1.5 });
        let renamed = FontConfig { preset: "compact".into(), ..FontConfig::default() };
        assert_eq!(preset_after_reload("big", &with_big, &renamed).as_deref(), Some("compact"));
        // The default itself was edited: every terminal follows it now.
        let relaxed = FontConfig { preset: "relaxed".into(), ..FontConfig::default() };
        assert_eq!(preset_after_reload("comfortable", &old, &relaxed).as_deref(), Some("relaxed"));
    }

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
    fn the_default_never_replaces_a_file_that_appeared() {
        // An editor's save can unlink then create the file; the watcher's load must not win.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.toml");
        std::fs::write(&file, "[font]\nfamily = \"Monaco\"\n").unwrap();
        assert_eq!(write_default(&file).unwrap_err().kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "[font]\nfamily = \"Monaco\"\n");
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

    #[test]
    fn the_viewer_limit_defaults_to_five_mebibytes_and_must_be_positive() {
        assert_eq!(Config::default().viewer.highlight_limit_bytes, 5 * 1024 * 1024);
        let c: Config = toml::from_str("[viewer]\nhighlight_limit_bytes = 1000\n").unwrap();
        assert_eq!(c.viewer.highlight_limit_bytes, 1000);
        let zero: Config = toml::from_str("[viewer]\nhighlight_limit_bytes = 0\n").unwrap();
        assert!(zero.validate().unwrap_err().contains("viewer.highlight_limit_bytes"));
        assert!(toml::from_str::<Config>(DEFAULT_CONFIG).unwrap().validate().is_ok());
    }
}
