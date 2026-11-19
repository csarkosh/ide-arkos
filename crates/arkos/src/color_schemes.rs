//! Colour schemes: the colours of a look as a file of their own, apart from its fonts and its
//! effect. A scheme file holds `name`, `title`, `appearance` and the five colour tables of a
//! theme file (`chrome`, `terminal`, `semantic`, `tree`, `viewer`). The built-in schemes are
//! embedded from `crates/arkos/colors/`; a file of the same name in the user's `colors/` folder
//! shadows the built-in.

use crate::theme::{self, Appearance, Chrome, Semantic, TerminalColors, Theme, TreeColors, ViewerColors};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The colours of a look, with the name a person reads and whether it is dark or light.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorScheme {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<Appearance>,
    pub chrome: Chrome,
    pub terminal: TerminalColors,
    pub semantic: Semantic,
    pub tree: TreeColors,
    pub viewer: ViewerColors,
}

/// The embedded schemes: (name, file text).
pub const BUILTIN: [(&str, &str); 11] = [
    ("navy", include_str!("../colors/navy.toml")),
    ("ocean", include_str!("../colors/ocean.toml")),
    ("palenight", include_str!("../colors/palenight.toml")),
    ("sublime", include_str!("../colors/sublime.toml")),
    ("bright", include_str!("../colors/bright.toml")),
    ("solar", include_str!("../colors/solar.toml")),
    ("lavender", include_str!("../colors/lavender.toml")),
    ("peach", include_str!("../colors/peach.toml")),
    ("timo-purple", include_str!("../colors/timo-purple.toml")),
    ("matrix", include_str!("../colors/matrix.toml")),
    ("rain", include_str!("../colors/rain.toml")),
];

/// A scheme from its file's text. A declared `appearance` must agree with the window background,
/// as in a theme file.
pub fn parse(text: &str) -> Result<ColorScheme, String> {
    let scheme: ColorScheme =
        toml::from_str(text).map_err(|e| e.message().lines().next().unwrap_or("invalid colour scheme").to_string())?;
    if let Some(declared) = scheme.appearance {
        let by_background = if crate::palette::luminance(scheme.chrome.window_bg) > 0.5 { Appearance::Light } else { Appearance::Dark };
        if declared != by_background {
            return Err(format!(
                "appearance = \"{}\" but window_bg {} is {}; make one of them match the other",
                declared.word(),
                scheme.chrome.window_bg.hex(),
                by_background.word()
            ));
        }
    }
    Ok(scheme)
}

/// The built-in scheme `name`.
pub fn builtin(name: &str) -> Option<ColorScheme> {
    let text = BUILTIN.iter().find(|(n, _)| *n == name).map(|(_, t)| *t)?;
    // The embedded files are tested to parse; a failure here is a build error, not a user one.
    parse(text).ok()
}

/// The folder of the user's colour files, beside the themes folder.
pub fn dir_of(themes_dir: &Path) -> PathBuf {
    themes_dir.with_file_name("colors")
}

/// `<colors folder>/<name>.toml` if it exists, else the built-in of that name. The error names
/// both places.
pub fn load(name: &str, themes_dir: &Path) -> Result<ColorScheme, String> {
    if !theme::is_bare_name(name) {
        return Err(format!("colour scheme name {name:?} must be a bare file name"));
    }
    let file = dir_of(themes_dir).join(format!("{name}.toml"));
    match std::fs::read_to_string(&file) {
        Ok(text) => parse(&text).map_err(|e| format!("{}: {e}", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            builtin(name).ok_or_else(|| format!("colour scheme {name:?} is not built in and {} does not exist", file.display()))
        }
        Err(e) => Err(format!("cannot read {}: {e}", file.display())),
    }
}

impl ColorScheme {
    /// A theme of this scheme's colours alone: its name, title and appearance, no font, no
    /// effect and no group.
    pub fn to_theme(&self) -> Theme {
        Theme {
            name: self.name.clone(),
            title: self.title.clone(),
            group: None,
            group_default: false,
            appearance: self.appearance,
            chrome: self.chrome.clone(),
            terminal: self.terminal.clone(),
            semantic: self.semantic.clone(),
            tree: self.tree.clone(),
            viewer: self.viewer.clone(),
            font: theme::ThemeFont::default(),
            effect: None,
        }
    }

    /// Whether every colour of the theme is this scheme's.
    pub fn same_colors(&self, t: &Theme) -> bool {
        self.chrome == t.chrome && self.terminal == t.terminal && self.semantic == t.semantic && self.tree == t.tree && self.viewer == t.viewer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_colour_scheme_is_the_colours_of_the_presets_it_came_from() {
        let pairs = [
            ("navy", "navy"),
            ("ocean", "material-ocean"),
            ("palenight", "material-palenight"),
            ("sublime", "material-sublime"),
            ("bright", "material"),
            ("solar", "material-solar"),
            ("solar", "composition-notebook"),
            ("lavender", "lavender-watercolor"),
            ("lavender", "lavender-material"),
            ("peach", "peach-watercolor"),
            ("peach", "arabia-solar"),
            ("timo-purple", "timo-purple"),
            ("matrix", "matrix"),
            ("rain", "tokyo-rain"),
        ];
        for (scheme, preset) in pairs {
            let s = builtin(scheme).unwrap_or_else(|| panic!("{scheme} is built in"));
            let t = theme::builtin(preset).unwrap();
            assert!(s.same_colors(&t), "{scheme} has the colours of {preset}");
            assert_eq!(s.appearance, t.appearance, "{scheme} and {preset} agree on appearance");
        }
        assert_eq!(BUILTIN.len(), 11);
    }

    #[test]
    fn every_built_in_colour_scheme_meets_the_contrast_rules() {
        for (name, _) in BUILTIN {
            let (ok, rows) = theme::contrast_report(&builtin(name).unwrap().to_theme());
            assert!(ok, "{name} fails: {:?}", rows.iter().filter(|(p, r)| *r < p.needs).map(|(p, r)| (p.needs, *r)).collect::<Vec<_>>());
        }
    }

    #[test]
    fn a_colour_file_in_the_colors_folder_shadows_the_built_in_of_its_name() {
        let dir = tempfile::tempdir().unwrap();
        let themes = dir.path().join("themes");
        std::fs::create_dir_all(&themes).unwrap();
        std::fs::create_dir_all(dir.path().join("colors")).unwrap();
        let original = builtin("navy").unwrap();
        let changed = BUILTIN[0].1.replace(&original.chrome.border.hex(), "#123456");
        assert_ne!(changed, BUILTIN[0].1);
        std::fs::write(dir.path().join("colors").join("navy.toml"), changed).unwrap();
        assert_eq!(load("navy", &themes).unwrap().chrome.border.hex(), "#123456");
        assert_eq!(load("ocean", &themes).unwrap(), builtin("ocean").unwrap(), "a name with no file is the built-in");
        assert!(load("nowhere", &themes).is_err());
    }

    #[test]
    fn a_colour_file_with_a_font_or_effect_table_is_refused() {
        assert!(parse(&(BUILTIN[0].1.to_string() + "\n[font]\nprose = \"Georgia\"\n")).is_err());
        assert!(parse(&(BUILTIN[0].1.to_string() + "\n[effect]\nname = \"material\"\n")).is_err());
    }

    #[test]
    fn a_colour_file_whose_appearance_disagrees_with_its_background_is_refused() {
        let err = parse(&BUILTIN[0].1.replace("appearance = \"dark\"", "appearance = \"light\"")).unwrap_err();
        assert!(err.contains("window_bg"), "the error names the background: {err}");
    }
}
