//! The Claude Code theme Arkos writes, `~/.claude/themes/arkos.json` (spec 2026-10-28, Claude
//! colours). Every colour in it names a slot of the terminal's palette rather than a fixed value,
//! so Claude Code in an Arkos tab takes the current Arkos theme, and a theme switch recolours the
//! lines already on screen too. Text-like keys name the sixteen ANSI slots 1 to 15, which every
//! Arkos theme holds to 4.5:1 on its background; background-like keys name the 256-colour cube
//! and grey ramp, which Arkos derives from the theme as faint tints of its background.

use std::path::{Path, PathBuf};

/// The file's slug, and so the name Claude Code's `theme` setting selects it by.
pub const SLUG: &str = "arkos";

/// Claude Code's `theme` value for this file.
pub const SELECTED: &str = "custom:arkos";

/// What a key paints, which decides the kind of colour it may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// A foreground colour on the terminal background: one of ANSI slots 1 to 15.
    Text,
    /// A band or fill under text: a derived 256-colour index near the background.
    Band,
    /// Text drawn on a coloured fill (a badge): the cube's first corner, the background itself.
    Inverse,
}

/// Every override key Claude Code documents (code.claude.com/docs/en/terminal-config, "Color
/// token reference"), what it paints and the colour Arkos gives it.
pub const KEYS: &[(&str, Role, &str)] = &[
    // Text and accent colours.
    ("claude", Role::Text, "ansi:redBright"),
    ("text", Role::Text, "ansi:whiteBright"),
    ("inverseText", Role::Inverse, "ansi256(16)"),
    ("inactive", Role::Text, "ansi:blackBright"),
    ("subtle", Role::Text, "ansi:blackBright"),
    ("suggestion", Role::Text, "ansi:blueBright"),
    ("permission", Role::Text, "ansi:blueBright"),
    ("remember", Role::Text, "ansi:blueBright"),
    // Status colours.
    ("success", Role::Text, "ansi:greenBright"),
    ("error", Role::Text, "ansi:redBright"),
    ("warning", Role::Text, "ansi:yellowBright"),
    ("merged", Role::Text, "ansi:magentaBright"),
    // Input box and mode indicators.
    ("promptBorder", Role::Text, "ansi:blackBright"),
    ("planMode", Role::Text, "ansi:cyan"),
    ("autoAccept", Role::Text, "ansi:magentaBright"),
    ("bashBorder", Role::Text, "ansi:magenta"),
    ("ide", Role::Text, "ansi:blue"),
    ("fastMode", Role::Text, "ansi:yellow"),
    ("effortUltra", Role::Text, "ansi:magentaBright"),
    // Diff rendering.
    ("diffAdded", Role::Band, "ansi256(22)"),
    ("diffRemoved", Role::Band, "ansi256(52)"),
    ("diffAddedDimmed", Role::Band, "ansi256(22)"),
    ("diffRemovedDimmed", Role::Band, "ansi256(52)"),
    ("diffAddedWord", Role::Band, "ansi256(28)"),
    ("diffRemovedWord", Role::Band, "ansi256(88)"),
    // Fullscreen mode, and the transcript's message backgrounds.
    ("userMessageBackground", Role::Band, "ansi256(234)"),
    ("userMessageBackgroundHover", Role::Band, "ansi256(235)"),
    ("bashMessageBackgroundColor", Role::Band, "ansi256(234)"),
    ("memoryBackgroundColor", Role::Band, "ansi256(234)"),
    ("selectionBg", Role::Band, "ansi256(17)"),
    ("composerSidebarBackground", Role::Band, "ansi256(233)"),
    // Usage meter and speaker labels.
    ("rate_limit_fill", Role::Text, "ansi:blueBright"),
    ("rate_limit_empty", Role::Text, "ansi:blackBright"),
    ("briefLabelYou", Role::Text, "ansi:blueBright"),
    ("briefLabelClaude", Role::Text, "ansi:redBright"),
    // Shimmer variants: the base key's hue in its other weight.
    ("claudeShimmer", Role::Text, "ansi:red"),
    ("warningShimmer", Role::Text, "ansi:yellow"),
    ("permissionShimmer", Role::Text, "ansi:blue"),
    ("promptBorderShimmer", Role::Text, "ansi:white"),
    ("inactiveShimmer", Role::Text, "ansi:white"),
    ("fastModeShimmer", Role::Text, "ansi:yellowBright"),
    // Subagent colours.
    ("red_FOR_SUBAGENTS_ONLY", Role::Text, "ansi:red"),
    ("blue_FOR_SUBAGENTS_ONLY", Role::Text, "ansi:blue"),
    ("green_FOR_SUBAGENTS_ONLY", Role::Text, "ansi:green"),
    ("yellow_FOR_SUBAGENTS_ONLY", Role::Text, "ansi:yellow"),
    ("purple_FOR_SUBAGENTS_ONLY", Role::Text, "ansi:magenta"),
    ("orange_FOR_SUBAGENTS_ONLY", Role::Text, "ansi:redBright"),
    ("pink_FOR_SUBAGENTS_ONLY", Role::Text, "ansi:magentaBright"),
    ("cyan_FOR_SUBAGENTS_ONLY", Role::Text, "ansi:cyan"),
    // The ultrathink rainbow.
    ("rainbow_red", Role::Text, "ansi:red"),
    ("rainbow_red_shimmer", Role::Text, "ansi:redBright"),
    ("rainbow_orange", Role::Text, "ansi:redBright"),
    ("rainbow_orange_shimmer", Role::Text, "ansi:yellowBright"),
    ("rainbow_yellow", Role::Text, "ansi:yellow"),
    ("rainbow_yellow_shimmer", Role::Text, "ansi:yellowBright"),
    ("rainbow_green", Role::Text, "ansi:green"),
    ("rainbow_green_shimmer", Role::Text, "ansi:greenBright"),
    ("rainbow_blue", Role::Text, "ansi:blue"),
    ("rainbow_blue_shimmer", Role::Text, "ansi:blueBright"),
    ("rainbow_indigo", Role::Text, "ansi:magenta"),
    ("rainbow_indigo_shimmer", Role::Text, "ansi:magentaBright"),
    ("rainbow_violet", Role::Text, "ansi:magentaBright"),
    ("rainbow_violet_shimmer", Role::Text, "ansi:white"),
];

/// The sixteen ANSI names Claude Code accepts after `ansi:`, in slot order.
#[cfg(test)]
pub const ANSI_NAMES: [&str; 16] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "blackBright",
    "redBright",
    "greenBright",
    "yellowBright",
    "blueBright",
    "magentaBright",
    "cyanBright",
    "whiteBright",
];

/// The palette index a colour value of `KEYS` names: `ansi:<name>` is its slot, `ansi256(n)` is n.
#[cfg(test)]
pub fn palette_index(value: &str) -> Option<u8> {
    if let Some(name) = value.strip_prefix("ansi:") {
        return ANSI_NAMES.iter().position(|n| *n == name).map(|i| i as u8);
    }
    value.strip_prefix("ansi256(")?.strip_suffix(')')?.parse().ok()
}

/// The theme file's content: `name` Arkos, `base` dark-ansi, and every documented key set.
pub fn arkos_theme_json() -> serde_json::Value {
    let overrides: serde_json::Map<String, serde_json::Value> = KEYS.iter().map(|(k, _, v)| (k.to_string(), serde_json::json!(v))).collect();
    serde_json::json!({ "name": "Arkos", "base": "dark-ansi", "overrides": overrides })
}

/// Claude Code's themes folder: `$CLAUDE_CONFIG_DIR/themes`, else `~/.claude/themes`, beside the
/// settings file `hook_cli::claude_settings_path` names.
pub fn themes_dir() -> PathBuf {
    crate::hook_cli::claude_settings_path().parent().map(|d| d.join("themes")).unwrap_or_else(|| PathBuf::from("themes"))
}

/// Writes `dir/arkos.json` when it is missing or its text differs from `arkos_theme_json`,
/// through a temporary file and a rename so Claude Code never reads half a file; true when it
/// wrote. A file that already holds the text is not touched, so Claude Code does not reload for
/// nothing. No other file in `dir` is read or written.
pub fn ensure_theme_file(dir: &Path) -> Result<bool, String> {
    let path = dir.join(format!("{SLUG}.json"));
    let text = serde_json::to_string_pretty(&arkos_theme_json()).map_err(|e| e.to_string())? + "\n";
    if std::fs::read_to_string(&path).is_ok_and(|t| t == text) {
        return Ok(false);
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let tmp = dir.join(format!("{SLUG}.json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, text).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(true)
}

/// What `follow_at` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Followed {
    /// The theme file was missing or out of date and has been written.
    pub wrote_file: bool,
    /// The settings' `theme` was switched to `SELECTED`, and what it was before (None: unset).
    pub switched_from: Option<Option<String>>,
}

/// Arkos's startup step for Claude Code's colours. Where Claude Code has a config folder (the
/// parent of `settings`), the theme file is brought up to date in `themes`; where its settings
/// file exists and names a stock or `auto` theme, the theme becomes `SELECTED`. Nothing is made
/// for a person without Claude Code, and any other theme is left to the doctor's row.
pub fn follow_at(settings: &Path, themes: &Path) -> Result<Followed, String> {
    let mut done = Followed { wrote_file: false, switched_from: None };
    if !settings.parent().is_some_and(|d| d.is_dir()) {
        return Ok(done);
    }
    done.wrote_file = ensure_theme_file(themes)?;
    if !settings.exists() {
        return Ok(done);
    }
    let theme = crate::hook_cli::claude_theme_at(settings)?;
    if crate::hook_cli::claude_theme_is_stock(theme.as_deref()) {
        crate::hook_cli::set_claude_theme_at(settings, SELECTED)?;
        done.switched_from = Some(theme);
    }
    Ok(done)
}

/// The notice for a switch: Claude Code watches its themes folder only if it existed when the
/// session started, so a session already running may not find the new theme until a restart.
pub const SWITCH_NOTICE: &str = "Claude Code now takes the Arkos colours; restart running sessions to see them";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::contrast;
    use crate::theme;

    fn builtins() -> Vec<(&'static str, theme::Theme)> {
        theme::BUILTIN.iter().map(|(n, _)| (*n, theme::builtin(n).unwrap())).collect()
    }

    #[test]
    fn every_text_like_key_names_an_ansi_slot_other_than_black() {
        for (key, role, value) in KEYS {
            if *role == Role::Text {
                let slot = value.strip_prefix("ansi:").and_then(|n| ANSI_NAMES.iter().position(|a| *a == n));
                assert!(matches!(slot, Some(1..=15)), "{key} = {value}");
            }
        }
    }

    #[test]
    fn every_slot_the_text_like_keys_name_reaches_4_5_to_1_on_each_built_in_theme() {
        for (name, t) in builtins() {
            let p = t.palette();
            for (key, role, value) in KEYS {
                if *role == Role::Text {
                    let c = p.indexed(palette_index(value).unwrap());
                    let r = contrast(c, t.terminal.background);
                    assert!(r >= 4.5, "{name}: {key} = {value} is {r:.2}:1 on the background");
                }
            }
        }
    }

    /// Main text must reach 4.5:1 on every band. Dim text is secondary, so it is held to the 3:1
    /// WCAG allows for secondary and large text, on the bands where Claude Code draws it; the
    /// changed-word highlights (28, 88) carry code text, not dim labels.
    #[test]
    fn main_text_keeps_4_5_to_1_on_every_band_and_dim_text_3_to_1_on_the_bands_it_is_drawn_on() {
        let mut low = Vec::new();
        for (name, t) in builtins() {
            let p = t.palette();
            let ratio = |slot: &str, band: u8| contrast(p.indexed(palette_index(slot).unwrap()), p.indexed(band));
            for band in [22u8, 52, 234, 235, 28, 88, 17] {
                let r = ratio("ansi:whiteBright", band);
                if r < 4.5 {
                    low.push(format!("{name}: main text on {band} is {r:.2}:1"));
                }
            }
            for band in [22u8, 52, 234, 235, 17] {
                let r = ratio("ansi:blackBright", band);
                if r < 3.0 {
                    low.push(format!("{name}: dim text on {band} is {r:.2}:1"));
                }
            }
        }
        assert!(low.is_empty(), "{low:#?}");
    }

    #[test]
    fn the_temporary_theme_file_is_named_for_the_process() {
        let dir = std::env::temp_dir().join(format!("arkos-tmpname-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(ensure_theme_file(&dir), Ok(true));
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["arkos.json".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_message_text_keeps_4_5_to_1_on_every_band_in_each_built_in_theme() {
        let text = palette_index(KEYS.iter().find(|(k, _, _)| *k == "text").unwrap().2).unwrap();
        let mut low = Vec::new();
        for (name, t) in builtins() {
            let p = t.palette();
            for (key, role, value) in KEYS {
                if *role == Role::Band {
                    assert!(value.starts_with("ansi256("), "{key} = {value} is a derived index");
                    let r = contrast(p.indexed(text), p.indexed(palette_index(value).unwrap()));
                    if r < 4.5 {
                        low.push(format!("{name}: text on {key} = {value} is {r:.2}:1"));
                    }
                }
            }
        }
        assert!(low.is_empty(), "{low:#?}");
    }

    #[test]
    fn inverse_text_reads_on_every_text_colour_used_as_a_fill() {
        let inverse = palette_index(KEYS.iter().find(|(_, r, _)| *r == Role::Inverse).unwrap().2).unwrap();
        for (name, t) in builtins() {
            let p = t.palette();
            for (key, role, value) in KEYS {
                if *role == Role::Text {
                    let r = contrast(p.indexed(inverse), p.indexed(palette_index(value).unwrap()));
                    assert!(r >= 4.5, "{name}: inverse text on {key} = {value} is {r:.2}:1");
                }
            }
        }
    }

    #[test]
    fn the_theme_sets_every_documented_key_once_on_the_dark_ansi_base() {
        let v = arkos_theme_json();
        assert_eq!((v["name"].as_str(), v["base"].as_str()), (Some("Arkos"), Some("dark-ansi")));
        assert_eq!(v["overrides"].as_object().unwrap().len(), KEYS.len(), "no key twice");
        assert_eq!(KEYS.len(), 63, "the documented list: 8 + 4 + 7 + 6 + 6 + 4 + 6 shimmers + 8 subagents + 14 rainbow");
        for (key, _, value) in KEYS {
            assert!(palette_index(value).is_some(), "{key} = {value} names a palette slot");
        }
    }

    #[test]
    fn the_guide_lists_every_key_with_its_colour() {
        let guide = include_str!("../../../docs/guide.md");
        for (key, _, value) in KEYS {
            assert!(guide.contains(&format!("| `{key}` | {} | `{value}` |", match KEYS.iter().find(|(k, _, _)| k == key).unwrap().1 {
                Role::Text => "text",
                Role::Band => "background",
                Role::Inverse => "text on a fill",
            })), "docs/guide.md has no row for {key}");
        }
    }

    #[test]
    fn the_theme_file_is_written_when_missing_or_different_and_left_alone_when_equal() {
        let dir = tempfile::tempdir().unwrap();
        let themes = dir.path().join("themes");
        std::fs::create_dir_all(&themes).unwrap();
        std::fs::write(themes.join("mine.json"), "{\"name\":\"Mine\"}").unwrap();
        let file = themes.join("arkos.json");
        assert_eq!(ensure_theme_file(&themes), Ok(true), "missing");
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v, arkos_theme_json());
        let before = std::fs::metadata(&file).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(ensure_theme_file(&themes), Ok(false), "equal");
        assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), before, "untouched");
        std::fs::write(&file, "{\"name\":\"Arkos\",\"base\":\"dark\"}").unwrap();
        assert_eq!(ensure_theme_file(&themes), Ok(true), "different");
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v, arkos_theme_json());
        assert_eq!(std::fs::read_to_string(themes.join("mine.json")).unwrap(), "{\"name\":\"Mine\"}", "another theme is not touched");
        let mut names: Vec<String> = std::fs::read_dir(&themes).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["arkos.json", "mine.json"], "no temporary file left");
        let fresh = dir.path().join("fresh/themes");
        assert_eq!(ensure_theme_file(&fresh), Ok(true), "the folder is made");
        assert!(fresh.join("arkos.json").is_file());
    }

    #[test]
    fn a_stock_or_auto_claude_theme_becomes_custom_arkos_and_any_other_is_left_alone() {
        for (was, switched) in [(None, true), (Some("dark"), true), (Some("light"), true), (Some("auto"), true), (Some("dark-daltonized"), false), (Some("light-ansi"), false), (Some("custom:mine"), false), (Some("custom:arkos"), false)] {
            let dir = tempfile::tempdir().unwrap();
            let settings = dir.path().join("settings.json");
            let themes = dir.path().join("themes");
            let text = match was {
                Some(t) => format!("{{\"theme\":\"{t}\",\"model\":\"opus\"}}"),
                None => "{\"model\":\"opus\"}".to_string(),
            };
            std::fs::write(&settings, &text).unwrap();
            let done = follow_at(&settings, &themes).unwrap();
            assert!(themes.join("arkos.json").is_file(), "{was:?}: the file is written either way");
            assert_eq!(done.switched_from, switched.then(|| was.map(str::to_string)), "{was:?}");
            let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
            assert_eq!(v["model"], "opus", "{was:?}: other keys kept");
            if switched {
                assert_eq!(v["theme"], SELECTED, "{was:?}");
            } else {
                assert_eq!(std::fs::read_to_string(&settings).unwrap(), text, "{was:?}: left alone");
            }
        }
    }

    #[test]
    fn nothing_is_written_where_claude_code_has_no_config_folder_and_no_settings_file_is_made() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join(".claude");
        let done = follow_at(&claude.join("settings.json"), &claude.join("themes")).unwrap();
        assert_eq!((done.wrote_file, done.switched_from), (false, None));
        assert!(!claude.exists());
        std::fs::create_dir_all(&claude).unwrap();
        let done = follow_at(&claude.join("settings.json"), &claude.join("themes")).unwrap();
        assert_eq!((done.wrote_file, done.switched_from), (true, None), "the theme file, but no settings file of Arkos's making");
        assert!(!claude.join("settings.json").exists());
    }
}
