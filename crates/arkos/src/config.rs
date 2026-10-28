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
    pub viewer: ViewerConfig,
    pub browser: BrowserConfig,
    pub daemon: DaemonConfig,
    pub theme: ThemeConfig,
    pub effect: EffectConfig,
    pub appearance: AppearanceConfig,
    pub recovery: RecoveryConfig,
    pub update: UpdateConfig,
    /// Usage and crash reports (telemetry spec, section 3.1): the type is the shared crate's,
    /// because the daemon reads this table too.
    pub telemetry: ::telemetry::TelemetryConfig,
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
    /// Option sends ESC plus the key (Terminal.app's "Use Option as Meta Key"), which Claude
    /// Code's Option shortcuts need; off, Option composes characters such as ß.
    pub option_as_meta: bool,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self { scrollback: 20_000, shell: None, nice: 0, option_as_meta: true }
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

/// The viewer's limits: a file larger than `highlight_limit_bytes` opens without syntax colours;
/// and how the rendered markdown view draws a table wider than the view (`wide_tables`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ViewerConfig {
    pub highlight_limit_bytes: usize,
    pub wide_tables: WideTables,
}

impl Default for ViewerConfig {
    fn default() -> Self {
        Self { highlight_limit_bytes: 5 * 1024 * 1024, wide_tables: WideTables::default() }
    }
}

/// A markdown table wider than the view: its columns shrink to fit and the cells wrap (`wrap`),
/// or they keep their widths and the table scrolls sideways inside its block (`scroll`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WideTables {
    #[default]
    Wrap,
    Scroll,
}

/// What "New browser tab" opens when its prompt is left empty.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BrowserConfig {
    /// Empty: a blank tab.
    pub url: String,
    /// Chrome's app mode, from 0.6's Chrome docking. Ignored since 0.7 (the web pane); kept so a
    /// config that sets it still reads.
    pub app_mode: bool,
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

/// Which effect to draw under the window: the theme's own recommendation, none, or an effect
/// by name. In the file it is the bare string `theme`, `none` or the name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum EffectChoice {
    #[default]
    Theme,
    None,
    Named(String),
}

impl Serialize for EffectChoice {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(match self {
            EffectChoice::Theme => "theme",
            EffectChoice::None => "none",
            EffectChoice::Named(n) => n,
        })
    }
}

impl<'de> Deserialize<'de> for EffectChoice {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        match name.as_str() {
            "theme" => Ok(EffectChoice::Theme),
            "none" => Ok(EffectChoice::None),
            n if crate::theme::is_bare_name(n) => Ok(EffectChoice::Named(name)),
            _ => Err(serde::de::Error::custom(format!("effect.name must be theme, none or an effect's bare name (was {name:?})"))),
        }
    }
}

/// `[effect]`: which effect is drawn and how strongly (1.0 is the effect as designed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EffectConfig {
    pub name: EffectChoice,
    pub intensity: f32,
}

impl Default for EffectConfig {
    fn default() -> Self {
        Self { name: EffectChoice::Theme, intensity: 1.0 }
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

/// Whether arkosd starts at login to bring sessions back after a restart (update-in-place
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

/// Automatic updates (auto-update spec): how often the daemon checks for a signed release (it
/// takes `check_every_secs` as an argument when the window starts it), and whether and when the
/// window applies a staged one by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UpdateConfig {
    pub auto: bool,
    pub check_every_secs: u64,
    pub idle_after_secs: u64,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self { auto: true, check_every_secs: 300, idle_after_secs: 30 }
    }
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
            effect: EffectConfig::default(),
            appearance: AppearanceConfig::default(),
            recovery: RecoveryConfig::default(),
            update: UpdateConfig::default(),
            telemetry: ::telemetry::TelemetryConfig::default(),
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
        if !(0.0..=1.0).contains(&self.effect.intensity) {
            return Err(format!("effect.intensity must be 0.0 to 1.0 (was {})", self.effect.intensity));
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
                eprintln!("arkos: wrote default config to {}", path.display());
                Ok(Config::default())
            }
            // Created between the read and the write, as an editor's unlink-then-create save
            // does: read what it wrote.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => read(path),
            Err(e) => {
                eprintln!("arkos: cannot write default config to {}: {e}", path.display());
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

/// Sets one value in the config file, keeping the file's comments (see `set_values`).
// The Settings window writes every change through `set_values`, one pair or two; this one-pair
// form is the tests' shorthand.
#[cfg_attr(not(test), allow(dead_code))]
pub fn set_value(path: &Path, keys: &[&str], value: toml_edit::Value) -> Result<(), String> {
    set_values(path, &[(keys, value)])
}

/// Applies `edits` (a key path and its new value each) to the config file in one write, through
/// `apply_edits`. The temp file is written beside the symlink-resolved target and renamed over
/// it, so a symlinked config stays a symlink. A file that does not parse is refused and left as
/// it was.
pub fn set_values(path: &Path, edits: &[(&[&str], toml_edit::Value)]) -> Result<(), String> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = std::fs::read_to_string(&target).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let edited = apply_edits(&text, edits)?;
    let mut tmp = target.clone().into_os_string();
    tmp.push(format!(".{}.tmp", std::process::id()));
    let tmp = std::path::PathBuf::from(tmp);
    let written = std::fs::write(&tmp, edited)
        .and_then(|()| match std::fs::metadata(&target) {
            Ok(meta) => std::fs::set_permissions(&tmp, meta.permissions()),
            Err(_) => Ok(()),
        })
        .and_then(|()| std::fs::rename(&tmp, &target));
    written.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("cannot write {}: {e}", target.display())
    })
}

/// Applies `edits` to a config file's `text` in memory and returns the new text. Comments, order,
/// formatting and a line's trailing comment are kept: the old value's decor is copied onto the
/// new one. A key goes at the end of its table, a missing table at the end of the file. The walk
/// goes through inline and dotted tables too; a path segment that is not a table is an error, as
/// is text that is not TOML. The result is not validated: `parse` it to check it would load.
pub fn apply_edits(text: &str, edits: &[(&[&str], toml_edit::Value)]) -> Result<String, String> {
    let mut doc = text.parse::<toml_edit::DocumentMut>().map_err(|e| e.to_string().lines().next().unwrap_or("parse error").to_string())?;
    for (keys, value) in edits {
        let Some((leaf, parents)) = keys.split_last() else { return Err("empty key path".to_string()) };
        let mut table: &mut dyn toml_edit::TableLike = doc.as_table_mut();
        for (i, key) in parents.iter().enumerate() {
            if table.get(key).is_none() {
                let mut created = toml_edit::Table::new();
                // Only the table that holds the value prints a header of its own.
                created.set_implicit(i + 1 < parents.len());
                table.insert(key, toml_edit::Item::Table(created));
            }
            table = table
                .get_mut(key)
                .and_then(|item| item.as_table_like_mut())
                .ok_or_else(|| format!("{} is not a table", keys[..=i].join(".")))?;
        }
        let mut value = value.clone();
        if let Some(old) = table.get(leaf).and_then(|item| item.as_value()) {
            *value.decor_mut() = old.decor().clone();
        }
        // Assigning through the existing item keeps the key's own decor (the comment lines above it).
        match table.get_mut(leaf) {
            Some(item) => *item = toml_edit::value(value),
            None => {
                table.insert(leaf, toml_edit::value(value));
            }
        }
    }
    Ok(doc.to_string())
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
# Option sends ESC plus the key (Terminal.app's "Use Option as Meta Key"), which Claude Code's
# Option shortcuts need. false: Option composes characters, so Option+S types ß.
option_as_meta = true

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
# "about" = ""
# "settings" = "cmd-,"
# "keybindings" = "cmd-/"
# "documentation" = ""
# "open config" = ""
# "command palette" = "cmd-shift-p"
# "find" = "cmd-f"
# "copy as shown" = "cmd-shift-c"
# "update now" = ""
# "check for updates" = ""
# "notifications" = ""
# "doctor" = ""
# "retry update" = ""
# "start at login" = ""
# "install claude code hooks" = ""
# "install agent guide" = ""
# "toggle sidebar" = "cmd-b"
# "focus file tree" = "cmd-shift-e"
# "show ignored files" = ""
# "focus worktrees" = "ctrl-shift-g"
# "toggle inline diff" = "cmd-shift-d"
# "toggle markdown view" = "cmd-shift-m"
# "save file" = "cmd-s"
# "open folder" = "cmd-o"
# "new window" = "cmd-n"
# "new browser tab" = "cmd-shift-b"
# "open file in pane" = ""
# "video play pause" = "space"
# "video back" = "left"
# "video forward" = "right"
# "video back fine" = "shift-left"
# "video forward fine" = "shift-right"
# "video step back" = ","
# "video step forward" = "."
# "video start" = "home"
# "video start alt" = "0"
# "video loop" = "l"
# "video mute" = "m"
# "video reload" = "r"
# In a web tab: back and forward in its history, reload, the address field, the page in the default
# browser, zoom back to 1 ("font bigger" and "font smaller" step the zoom there).
# "page back" = "cmd-["
# "page forward" = "cmd-]"
# "reload page" = "cmd-r"
# "focus address" = "cmd-l"
# "open in browser" = "cmd-shift-o"
# "zoom reset" = "cmd-0"
# "quit" = "cmd-q"

[sidebar]
# Width in px and whether it starts collapsed, for a folder opened for the first time; each folder then remembers its own.
width = 260.0
collapsed = false

[viewer]
# Files larger than this open in the viewer without syntax colours; binaries show a notice.
highlight_limit_bytes = 5242880
# A markdown table wider than the rendered view: "wrap" shrinks its columns and wraps the cells;
# "scroll" keeps the column widths and scrolls the table sideways (Shift+wheel).
# wide_tables = "wrap"

[browser]
# A browser tab shows a web page inside the pane. "New browser tab" (Cmd+Shift+B) asks for a
# URL; left empty it opens this one, and this empty opens a blank tab. A URL without a scheme
# gets https:// (http:// for this machine).
url = ""
# From 0.6's Chrome docking; ignored since 0.7 and kept so an older config still reads.
app_mode = false

[daemon]
# Read when the window starts arkosd. A daemon that is already running keeps its values until
# `pkill -x arkosd`, which ends every session.
# Seconds without a heartbeat from the window before the daemon relaunches it.
watchdog_secs = 30
# Bytes of output kept per session, replayed when a window reattaches.
replay_bytes = 16777216

[theme]
# Theme files live in ~/.config/arkos/themes/<name>.toml. "navy", "navy-light", "timo-purple", "peach-light" and "lavender-light" are built in;
# "Extract built-in themes" in the command palette writes them there to copy from.
dark = "navy"
light = "navy-light"

[effect]
# A shader drawn under the window's text. "theme" draws the one the theme was designed with (only
# peach-light has one, the watercolor), "none" draws nothing, or name an effect: a built-in, or a
# folder in ~/.config/arkos/effects/. intensity runs from 0.0 (invisible) to 1.0 (as designed).
name = "theme"
intensity = 1.0

[appearance]
# "system" follows macOS light and dark: the evidence points at matching the room, which the
# system setting already tracks. "dark" or "light" fixes one. No warm tint or blue-light mode is
# offered: both fail their controlled trials.
mode = "system"

[recovery]
# After a restart, arkosd started at login brings every session back: the same folders, the
# history on screen under a divider, and each Claude Code conversation resumed (never continued:
# you send the next message). Everything stays on this Mac. "ask" shows a notice at launch until
# "Start at login" runs from the command palette; "on" installs the login item
# (~/Library/LaunchAgents/sh.csarko.arkosd.plist) and keeps it; "off" removes it.
start_at_login = "ask"

[update]
# The daemon checks the release manifest every check_every_secs and downloads a newer, signed
# build in the background. With auto = true the window applies it once you have been idle for
# idle_after_secs (no typing, nothing open): the daemon swaps in place and the window relaunches
# with every shell still there. auto = false keeps the "update ready" badge and the palette's
# "Update now" instead.
auto = true
check_every_secs = 300
idle_after_secs = 30

[telemetry]
# Usage reports: which features you used and for how long, never a file, a folder, a name or
# anything typed, under an id that ends with the window. "local" records them on this Mac only;
# "on" also sends them; "off" records nothing. Help > What Arkos Sends shows every field.
# usage = "local"
# Crash reports: "ask" shows the report and asks before sending each one; "always" sends them
# without asking; "never" deletes them.
# crashes = "ask"
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
pub(crate) mod tests {
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
    fn a_config_from_before_option_as_meta_keeps_meta_on() {
        let cfg: Config = toml::from_str("[terminal]\nscrollback = 10\n").unwrap();
        assert!(cfg.terminal.option_as_meta);
        let off: Config = toml::from_str("[terminal]\noption_as_meta = false\n").unwrap();
        assert!(!off.terminal.option_as_meta);
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
        assert!(cfg.terminal.option_as_meta);
        assert!(cfg.keybindings.is_empty());
        assert_eq!(cfg.daemon.watchdog_secs, 30);
        assert_eq!(cfg.daemon.replay_bytes, 16 * 1024 * 1024);
        assert_eq!(cfg.theme.dark, "navy");
        assert_eq!(cfg.theme.light, "navy-light");
        assert_eq!(cfg.appearance.mode, AppearanceMode::System);
    }

    #[test]
    fn the_update_table_defaults_to_automatic_and_accepts_the_three_fields() {
        let c = parse("").unwrap();
        assert_eq!(c.update, UpdateConfig { auto: true, check_every_secs: 300, idle_after_secs: 30 });
        let c = parse("[update]\nauto = false\ncheck_every_secs = 60\nidle_after_secs = 5\n").unwrap();
        assert_eq!(c.update, UpdateConfig { auto: false, check_every_secs: 60, idle_after_secs: 5 });
        assert!(parse("[update]\nspeed = 1\n").is_err(), "unknown keys are refused like everywhere else");
    }

    #[test]
    fn the_browser_table_defaults_and_still_accepts_the_retired_app_mode() {
        let c = parse("").unwrap();
        assert_eq!(c.browser, BrowserConfig { url: String::new(), app_mode: false });
        let c = parse(DEFAULT_CONFIG).unwrap();
        assert_eq!(c.browser, BrowserConfig { url: String::new(), app_mode: false }, "the written defaults");
        let c = parse("[browser]\nurl = \"https://example.com\"\napp_mode = true\n").unwrap();
        assert_eq!(c.browser, BrowserConfig { url: "https://example.com".into(), app_mode: true });
        let c = parse("[browser]\nurl = \"https://example.com\"\n").unwrap();
        assert!(!c.browser.app_mode, "app_mode defaults to false");
        assert!(parse("[browser]\nhome = \"x\"\n").is_err(), "unknown keys are refused like everywhere else");
        assert!(parse("[browser]\napp_mode = \"yes\"\n").is_err(), "app_mode is a boolean");
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
        // The telemetry keys are written commented, at their defaults, so the file states no choice.
        assert!(DEFAULT_CONFIG.contains("\n[telemetry]\n"), "the telemetry table");
        assert!(DEFAULT_CONFIG.contains("\n# usage = \"local\"\n"), "usage, commented");
        assert!(DEFAULT_CONFIG.contains("\n# crashes = \"ask\"\n"), "crashes, commented");
    }

    #[test]
    fn the_telemetry_table_is_read_through_the_shared_type() {
        use ::telemetry::visits_app::{Crashes, Usage};
        use ::telemetry::TelemetryConfig;
        let defaults = TelemetryConfig { usage: Usage::Local, crashes: Crashes::Ask };
        assert_eq!(parse("").unwrap().telemetry, defaults);
        assert_eq!(parse(DEFAULT_CONFIG).unwrap().telemetry, defaults, "the written defaults");
        assert_eq!(::telemetry::parse_table(DEFAULT_CONFIG), Ok(defaults), "the daemon reads the same table");
        let c = parse("[telemetry]\nusage = \"on\"\ncrashes = \"always\"\n").unwrap();
        assert_eq!(c.telemetry, TelemetryConfig { usage: Usage::On, crashes: Crashes::Always });
        let e = parse("[telemetry]\nusage = \"on\"\nuploads = true\n").unwrap_err();
        assert_eq!(e.line, Some(3), "{e}");
        assert!(e.message.contains("uploads"), "unknown keys are refused like everywhere else: {e}");
        let e = parse("[telemetry]\ncrashes = \"sometimes\"\n").unwrap_err();
        assert_eq!(e.line, Some(2), "{e}");
        assert!(e.message.contains("sometimes"), "{e}");
        let back = toml::to_string(&c).unwrap();
        assert_eq!(parse(&back).unwrap(), c);
    }

    /// Every `Command` variant, with any value where it carries one.
    pub(crate) fn every_command() -> Vec<crate::keymap::Command> {
        use crate::keymap::Command::*;
        use crate::tree::Side;
        let all = vec![
            FocusPane(1), FocusDir(Side::Left), Split(Side::Right), ToggleMaximize, MoveTabToPane(1), CycleTab(1), MruTab, CloseTab,
            ReopenClosedTab, CloseAllInPane, MoveTabWithin(1), NewTerminal, ClearToStart, FontDelta(1.0), About, OpenSettings, Keybindings,
            Documentation, OpenConfig, TextPreset(String::new()), ReloadConfig, ExtractThemes, CommandPalette, SetTheme(String::new()), Find,
            CopyAsShown, UpdateNow, CheckForUpdates, Notifications, Doctor, RetryUpdate, StartAtLogin, InstallHooks, InstallAgentGuide,
            ToggleSidebar, FocusFileTree, ShowIgnoredFiles, FocusWorktrees, ToggleInlineDiff, ToggleMarkdownView, SaveFile, OpenFolder,
            NewWindow, NewBrowserTab, OpenFileInPane, VideoPlayPause, VideoBack, VideoForward, VideoBackFine, VideoForwardFine, VideoStepBack, VideoStepForward,
            VideoStart, VideoLoop, VideoMute, VideoReload, PageBack, PageForward, ReloadPage, FocusAddress, OpenInBrowser, ZoomReset,
        ];
        // A variant added to `Command` stops this match from compiling until it is listed above,
        // and then the test below asks for it in `telemetry/events.toml`.
        for c in &all {
            match c {
                FocusPane(_) | FocusDir(_) | Split(_) | ToggleMaximize | MoveTabToPane(_) | CycleTab(_) | MruTab | CloseTab | ReopenClosedTab
                | CloseAllInPane | MoveTabWithin(_) | NewTerminal | ClearToStart | FontDelta(_) | About | OpenSettings | Keybindings
                | Documentation | OpenConfig | TextPreset(_) | ReloadConfig | ExtractThemes | CommandPalette | SetTheme(_) | Find | CopyAsShown
                | UpdateNow | CheckForUpdates | Notifications | Doctor | RetryUpdate | StartAtLogin | InstallHooks | InstallAgentGuide
                | ToggleSidebar | FocusFileTree | ShowIgnoredFiles | FocusWorktrees | ToggleInlineDiff | ToggleMarkdownView | SaveFile
                | OpenFolder | NewWindow | NewBrowserTab | OpenFileInPane | VideoPlayPause | VideoBack | VideoForward | VideoBackFine | VideoForwardFine
                | VideoStepBack | VideoStepForward | VideoStart | VideoLoop | VideoMute | VideoReload | PageBack | PageForward | ReloadPage
                | FocusAddress | OpenInBrowser | ZoomReset => {}
            }
        }
        all
    }

    /// A variant's name in snake case, from its `Debug` form: `DockChromeWindowAt(0)` is
    /// `dock_chrome_window_at`.
    fn snake_name(c: &crate::keymap::Command) -> String {
        let debug = format!("{c:?}");
        let mut out = String::new();
        for (i, ch) in debug.chars().take_while(|ch| ch.is_alphanumeric()).enumerate() {
            if ch.is_uppercase() && i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        }
        out
    }

    /// One `d` key's listed values in the compiled allowlist file.
    fn allowlisted(event: &str, key: &str) -> std::collections::BTreeSet<String> {
        let file: toml::Table = toml::from_str(::telemetry::EVENTS_TOML).unwrap();
        file["events"][event][key].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn the_palette_event_names_exactly_the_commands() {
        use ::telemetry::visits_app::{DValue, Event};
        let names: std::collections::BTreeSet<String> = every_command().iter().map(snake_name).collect();
        assert_eq!(names.len(), every_command().len(), "no two variants share a name");
        assert_eq!(allowlisted("palette", "command"), names, "telemetry/events.toml's palette commands are Command's variants");
        for name in &names {
            let ev = Event { n: "palette".into(), t: 0, d: [("command".to_string(), DValue::Str(name.clone()))].into() };
            assert_eq!(::telemetry::allowlist().check(&ev), Ok(()), "{name}");
        }
    }

    #[test]
    fn the_settings_event_names_exactly_the_config_keys() {
        let toml::Value::Table(sections) = toml::Value::try_from(Config::default()).unwrap() else { panic!("a table") };
        let mut keys = std::collections::BTreeSet::new();
        for (section, table) in &sections {
            for key in table.as_table().unwrap().keys() {
                // A preset's name is the person's own, so every preset is one key.
                keys.insert(if section == "font" && key == "presets" { "font.presets".to_string() } else { format!("{section}.{key}") });
            }
        }
        // The two the defaults leave out: an unset shell, and the keybindings table, which is one
        // key because an action's name has spaces in it.
        keys.insert("terminal.shell".into());
        keys.insert("keybindings".into());
        assert_eq!(allowlisted("settings", "key"), keys, "telemetry/events.toml's settings keys are the config's");
    }

    #[test]
    fn the_effect_section_defaults_to_the_theme_s_choice_at_full_intensity() {
        let cfg = Config::default();
        assert_eq!(cfg.effect, EffectConfig { name: EffectChoice::Theme, intensity: 1.0 });
        assert_eq!(parse("").unwrap().effect, cfg.effect);
        assert_eq!(parse(DEFAULT_CONFIG).unwrap(), cfg, "the commented defaults say the same");
    }

    #[test]
    fn effect_name_reads_theme_none_or_a_name_and_writes_them_back_the_same() {
        for (text, want) in [("theme", EffectChoice::Theme), ("none", EffectChoice::None), ("watercolor", EffectChoice::Named("watercolor".into()))] {
            let cfg = parse(&format!("[effect]\nname = \"{text}\"\n")).unwrap();
            assert_eq!(cfg.effect.name, want);
            let back = toml::to_string(&cfg).unwrap();
            assert!(back.contains(&format!("name = \"{text}\"")), "{back}");
            assert_eq!(parse(&back).unwrap().effect.name, want);
        }
        let e = parse("[effect]\nname = \"\"\n").unwrap_err().to_string();
        assert!(e.contains("effect.name"), "{e}");
        let e = parse("[effect]\nname = \"../x\"\n").unwrap_err().to_string();
        assert!(e.contains("effect.name"), "{e}");
    }

    #[test]
    fn an_intensity_outside_zero_to_one_is_a_config_error_naming_the_key() {
        assert_eq!(parse("[effect]\nintensity = 0.4\n").unwrap().effect.intensity, 0.4);
        assert_eq!(parse("[effect]\nintensity = 0.0\n").unwrap().effect.intensity, 0.0);
        let e = parse("[effect]\nintensity = 1.5\n").unwrap_err().to_string();
        assert!(e.contains("effect.intensity must be 0.0 to 1.0 (was 1.5)"), "{e}");
        assert!(parse("[effect]\nintensity = -0.1\n").is_err());
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

    #[test]
    fn wide_tables_wrap_by_default_and_read_wrap_or_scroll() {
        assert_eq!(Config::default().viewer.wide_tables, WideTables::Wrap);
        let c: Config = toml::from_str("[viewer]\nwide_tables = \"scroll\"\n").unwrap();
        assert_eq!(c.viewer.wide_tables, WideTables::Scroll);
        let c: Config = toml::from_str("[viewer]\nwide_tables = \"wrap\"\n").unwrap();
        assert_eq!(c.viewer.wide_tables, WideTables::Wrap);
        assert!(toml::from_str::<Config>("[viewer]\nwide_tables = \"sideways\"\n").is_err());
    }

    #[test]
    fn apply_edits_changes_the_text_in_memory_and_keeps_its_comments() {
        let text = "# top\n[theme]\ndark = \"navy\" # d\n";
        let edits: [(&[&str], toml_edit::Value); 2] = [(&["theme", "dark"], "ocean".into()), (&["font", "presets", "big", "size"], 18.0.into())];
        let out = apply_edits(text, &edits).unwrap();
        assert!(out.starts_with("# top\n[theme]\ndark = \"ocean\" # d\n"), "{out}");
        assert!(out.contains("[font.presets.big]\nsize = 18.0\n"), "{out}");
        assert!(!out.contains("[font]\n"), "the parent tables stay implicit: {out}");
        assert!(apply_edits(text, &[(&["theme", "dark", "x"], 1.into())]).unwrap_err().contains("theme.dark"));
        assert!(apply_edits("[[", &[(&["a"], 1.into())]).is_err());
    }

    #[test]
    fn set_value_keeps_comments_and_adds_what_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "# top comment\n[font]\n# the family\nfamily = \"Menlo\" # trailing\npreset = \"comfortable\"\n\n[font.presets.comfortable]\nsize = 16.0\nline_height = 1.5\n\n[update]\nauto = true\n").unwrap();
        set_value(&path, &["font", "presets", "comfortable", "size"], toml_edit::Value::from(18.0)).unwrap();
        set_value(&path, &["appearance", "mode"], toml_edit::Value::from("dark")).unwrap();
        set_value(&path, &["font", "family"], toml_edit::Value::from("SF Mono")).unwrap();
        set_value(&path, &["update", "idle_after_secs"], toml_edit::Value::from(30)).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# top comment\n[font]\n# the family\nfamily = \"SF Mono\" # trailing\n"), "comments and a trailing comment survive: {text}");
        assert!(text.contains("size = 18.0\nline_height = 1.5\n"), "{text}");
        assert!(text.contains("[update]\nauto = true\nidle_after_secs = 30\n"), "a key lands at the end of its table: {text}");
        assert!(text.ends_with("[appearance]\nmode = \"dark\"\n"), "a missing table is appended: {text}");
        let parsed: Config = toml::from_str(&text).unwrap();
        assert_eq!(parsed.font.family, "SF Mono");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "no temp file left");
    }

    #[test]
    fn set_values_writes_a_preset_whole_and_refuses_a_bad_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[font]\npreset = \"big\"\n").unwrap();
        set_values(&path, &[(&["font", "presets", "big", "size"], toml_edit::Value::from(20.0)), (&["font", "presets", "big", "line_height"], toml_edit::Value::from(1.6))]).unwrap();
        let parsed: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed.font.presets["big"].size, 20.0);
        assert!(set_values(&path, &[(&["font", "preset", "x"], toml_edit::Value::from(1))]).is_err(), "a value is not a table");
    }

    #[test]
    fn set_value_refuses_a_file_it_cannot_parse_follows_a_symlink_and_round_trips_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[font\nfamily = 1\n").unwrap();
        let err = set_value(&path, &["font", "family"], toml_edit::Value::from("x")).unwrap_err();
        assert!(!err.is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[font\nfamily = 1\n", "left as it was");
        std::fs::write(&path, DEFAULT_CONFIG).unwrap();
        let link = dir.path().join("link.toml");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        set_value(&link, &["update", "auto"], toml_edit::Value::from(false)).unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link is still a link");
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches('#').count(), DEFAULT_CONFIG.matches('#').count(), "every comment survives");
        assert!(toml::from_str::<Config>(&text).is_ok());
    }
}
