//! Font sets: the fonts of a look as a file of their own, apart from its colours and its effect.
//! A font file holds `name`, `title`, `prose`, `ui`, `terminal` and `grid`. The built-in sets are
//! embedded from `crates/arkos/fonts/`; a file of the same name in the user's `fonts/` folder
//! shadows the built-in.

use crate::theme::{self, ThemeFont};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The fonts of a look, with the name a person reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontSet {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prose: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<f32>,
}

/// The embedded sets: (name, file text).
pub const BUILTIN: [(&str, &str); 5] = [
    ("material", include_str!("../fonts/material.toml")),
    ("lavender", include_str!("../fonts/lavender.toml")),
    ("peach", include_str!("../fonts/peach.toml")),
    ("notebook", include_str!("../fonts/notebook.toml")),
    ("departure", include_str!("../fonts/departure.toml")),
];

/// A set from its file's text. A `grid` must be from 1.0 to 24.0 points.
pub fn parse(text: &str) -> Result<FontSet, String> {
    let set: FontSet = toml::from_str(text).map_err(|e| e.message().lines().next().unwrap_or("invalid font set").to_string())?;
    if let Some(g) = set.grid {
        if !(1.0..=24.0).contains(&g) {
            return Err(format!("grid = {g} is out of range; use a size from 1.0 to 24.0"));
        }
    }
    Ok(set)
}

/// The built-in set `name`.
pub fn builtin(name: &str) -> Option<FontSet> {
    let text = BUILTIN.iter().find(|(n, _)| *n == name).map(|(_, t)| *t)?;
    // The embedded files are tested to parse; a failure here is a build error, not a user one.
    parse(text).ok()
}

/// The folder of the user's font files, beside the themes folder.
pub fn dir_of(themes_dir: &Path) -> PathBuf {
    themes_dir.with_file_name("fonts")
}

/// `<fonts folder>/<name>.toml` if it exists, else the built-in of that name. The error names
/// both places.
pub fn load(name: &str, themes_dir: &Path) -> Result<FontSet, String> {
    if !theme::is_bare_name(name) {
        return Err(format!("font set name {name:?} must be a bare file name"));
    }
    let file = dir_of(themes_dir).join(format!("{name}.toml"));
    match std::fs::read_to_string(&file) {
        Ok(text) => parse(&text).map_err(|e| format!("{}: {e}", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            builtin(name).ok_or_else(|| format!("font set {name:?} is not built in and {} does not exist", file.display()))
        }
        Err(e) => Err(format!("cannot read {}: {e}", file.display())),
    }
}

impl FontSet {
    /// The set as the font table of a theme.
    pub fn font(&self) -> ThemeFont {
        ThemeFont { prose: self.prose.clone(), ui: self.ui.clone(), terminal: self.terminal.clone(), grid: self.grid }
    }

    /// The name a person reads: the `title`, else the set's name in title case.
    pub fn title(&self) -> String {
        self.title.clone().unwrap_or_else(|| theme::title_case(&self.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_font_set_is_the_fonts_of_the_presets_it_came_from() {
        let pairs = [
            ("material", "navy"),
            ("material", "material-ocean"),
            ("material", "material-palenight"),
            ("material", "material-sublime"),
            ("material", "material"),
            ("material", "material-solar"),
            ("lavender", "lavender-watercolor"),
            ("lavender", "lavender-material"),
            ("peach", "peach-watercolor"),
            ("notebook", "composition-notebook"),
            ("departure", "timo-purple"),
            ("departure", "arabia-solar"),
        ];
        for (set, preset) in pairs {
            let s = builtin(set).unwrap_or_else(|| panic!("{set} is built in"));
            assert_eq!(s.font(), theme::builtin(preset).unwrap().font, "{set} has the fonts of {preset}");
        }
        assert_eq!(BUILTIN.len(), 5);
    }

    #[test]
    fn every_built_in_font_set_has_the_title_the_design_gives() {
        let titles = [
            ("material", "DM Sans + Roboto Mono"),
            ("lavender", "Avenir Next + Cascadia Code"),
            ("peach", "Baskerville + Optima + PT Mono"),
            ("notebook", "Chalkboard + Monaspace Radon"),
            ("departure", "Departure Mono"),
        ];
        for (name, title) in titles {
            assert_eq!(builtin(name).unwrap().title(), title);
        }
    }

    #[test]
    fn a_font_file_in_the_fonts_folder_shadows_the_built_in_of_its_name() {
        let dir = tempfile::tempdir().unwrap();
        let themes = dir.path().join("themes");
        std::fs::create_dir_all(&themes).unwrap();
        std::fs::create_dir_all(dir.path().join("fonts")).unwrap();
        let changed = BUILTIN[0].1.replace("Roboto Mono", "Menlo");
        assert_ne!(changed, BUILTIN[0].1);
        std::fs::write(dir.path().join("fonts").join("material.toml"), changed).unwrap();
        assert_eq!(load("material", &themes).unwrap().terminal.as_deref(), Some("Menlo"));
        assert_eq!(load("peach", &themes).unwrap(), builtin("peach").unwrap(), "a name with no file is the built-in");
        assert!(load("nowhere", &themes).is_err());
    }

    #[test]
    fn a_font_file_with_a_grid_out_of_range_is_refused() {
        let base = "name = \"x\"\nterminal = \"Menlo\"\n";
        assert!(parse(&format!("{base}grid = 0.5\n")).is_err());
        assert!(parse(&format!("{base}grid = 24.5\n")).is_err());
        assert!(parse(&format!("{base}grid = 5.5\n")).is_ok());
    }
}
