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
    /// Unused since 2026-10-15 (a handle at rest paints nothing); kept so theme files that name
    /// it still load.
    pub split_handle: Rgb,
    /// The stripe over a handle under the pointer or being dragged.
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
#[cfg_attr(not(test), allow(dead_code))] // The extract-themes command calls it from Task 6 on.
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

/// Which theme name the config picks for the current appearance (spec 7.1: follow the system
/// by default, Navy for dark and Navy Light for light).
pub fn pick_name(mode: crate::config::AppearanceMode, system_is_dark: bool, cfg: &crate::config::ThemeConfig) -> &str {
    use crate::config::AppearanceMode::*;
    match (mode, system_is_dark) {
        (Dark, _) | (System, true) => &cfg.dark,
        (Light, _) | (System, false) => &cfg.light,
    }
}

/// A theme that fails to load keeps the current one and says so; the same theme is not re-applied.
pub fn keep_or_switch(current: &Theme, wanted: Result<Theme, String>) -> (Option<Theme>, Option<String>) {
    match wanted {
        Ok(t) if t == *current => (None, None),
        Ok(t) => (Some(t), None),
        Err(e) => (None, Some(format!("theme: {e}; keeping {}", current.name))),
    }
}

pub fn is_dark(appearance: gpui::WindowAppearance) -> bool {
    use gpui::WindowAppearance::*;
    matches!(appearance, Dark | VibrantDark)
}

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
        // The built-in Navy keeps milestone 1's ANSI colours; only the chrome and the terminal
        // background are sampled from the reference screenshot.
        assert_eq!(p.ansi, crate::palette::NAVY.ansi);
    }

    #[test]
    fn a_theme_file_must_be_complete_and_known() {
        let e = parse("name = \"x\"\n").unwrap_err();
        assert!(e.contains("chrome"), "{e}");
        let text = BUILTIN[0].1.replace("window_bg", "window_background");
        let e = parse(&text).unwrap_err();
        assert!(e.contains("window_background") || e.contains("window_bg"), "{e}");
        let bg = builtin("navy").unwrap().chrome.window_bg.hex();
        let text = BUILTIN[0].1.replace(&bg, &bg[..6]);
        assert!(parse(&text).is_err());
    }

    #[test]
    fn user_files_win_and_names_are_bare_stems() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load("navy", dir.path()).unwrap(), builtin("navy").unwrap(), "a missing folder falls back to the built-in");
        let mut text = BUILTIN[0].1.to_string();
        let bg = builtin("navy").unwrap().chrome.window_bg.hex();
        text = text.replace(&format!("window_bg = \"{bg}\""), "window_bg = \"#000000\"");
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
}
