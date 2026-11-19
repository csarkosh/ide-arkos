//! Themes (spec 7): one flat set of named colours per TOML file. Six are built in and embedded;
//! user files in `~/.config/arkos/themes/` override or add to them. `Root` holds the active
//! `Theme`; views read it, and each terminal resolves cells against `Theme::palette` at paint.
use crate::palette::{Palette, Rgb};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub name: String,
    /// Optional: the name a person reads (Settings, the palette): "Plain Navy" for `navy`.
    /// Absent, the name in capitalised words (`title_case`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Optional: the group Settings shows the theme under, one row with a choice of its members
    /// ("Watercolor": Watercolor Lavender and Watercolor Peach).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Whether the theme is its group's member a click on the group's row picks.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub group_default: bool,
    /// Whether this is a dark or a light theme, which decides the `[theme]` slot Settings lists
    /// it under. Every built-in says; a file from before 0.7.20 may not, and is then classed by
    /// its window background (`kind`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<Appearance>,
    pub chrome: Chrome,
    pub terminal: TerminalColors,
    pub semantic: Semantic,
    pub tree: TreeColors,
    pub viewer: ViewerColors,
    /// Optional: the fonts a look asks for. Absent in files from before 0.3.3.
    #[serde(default, skip_serializing_if = "ThemeFont::is_default")]
    pub font: ThemeFont,
    /// Optional: the effect the theme was designed with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<ThemeEffect>,
    /// The name of the colour scheme this theme draws: a new-form preset's `color`, an old-form
    /// file's own `name`, a scheme loaded alone its own name. Never in a file.
    #[serde(skip)]
    pub color: String,
}

/// A theme's `appearance`: a dark theme (a dark window, light text) or a light one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    Dark,
    Light,
}

impl Appearance {
    pub fn word(self) -> &'static str {
        match self {
            Appearance::Dark => "dark",
            Appearance::Light => "light",
        }
    }
}

/// The `[effect]` table of a theme file: the one effect the look was designed with. The user's
/// `effect.name` in the config may choose another.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeEffect {
    pub name: String,
    /// Which of the effect's scenes the theme shows, by the name its manifest lists (`scenes`):
    /// runic's `city` (Tokyo Rain) or `code` (Matrix). Absent, the effect's first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<String>,
}

impl Theme {
    /// The name of the effect this theme recommends, if it does.
    pub fn recommended_effect(&self) -> Option<&str> {
        self.effect.as_ref().map(|e| e.name.as_str())
    }
}

/// The fonts a theme may name, each a family installed on the Mac. `prose`: the rendered
/// markdown's text (headings, paragraphs, lists; unset, the system UI font). `ui`: the window's
/// chrome, the file tree, tabs, palette and banners (unset, the terminal font, as before; the
/// title bar the system font). `terminal`: the shells, the raw and code views and the
/// terminal's measurements (unset, `[font] family` in the config); it must be monospaced.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeFont {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prose: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    /// The size, in points, the font's own pixels are whole at: under the theme every text size
    /// is drawn at the nearest multiple of it, so a pixel face is sharp at any preset. It goes
    /// with the terminal font: without that installed, sizes are left as they are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<f32>,
}

/// One colour scheme as the Color list shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorEntry {
    pub name: String,
    pub title: String,
    pub appearance: Option<Appearance>,
    pub source: Source,
}

/// The `*.toml` files of `dir` that are not new-form presets, sorted by name, as (stem, theme).
/// These are the old all-in-one files, such as the ones `theme extract` wrote before 0.8.55.
fn old_form_presets(dir: &Path) -> Vec<(String, Theme)> {
    let mut stems: Vec<String> = names(dir).into_iter().filter(|n| dir.join(format!("{n}.toml")).is_file()).collect();
    stems.sort();
    stems
        .into_iter()
        .filter_map(|stem| {
            let text = std::fs::read_to_string(dir.join(format!("{stem}.toml"))).ok()?;
            let new_form = toml::from_str::<toml::Table>(&text).map(|t| t.contains_key("color")).unwrap_or(true);
            if new_form {
                return None;
            }
            parse_in(&text, Some(dir)).ok().map(|t| (stem, t))
        })
        .collect()
}

/// Every colour to show, one row a name, each row what that name loads (`load_color`): the
/// built-in schemes in order, the other colour files sorted, then each old-form preset file of
/// `themes_dir` whose colours match no row listed so far, named by its file name and titled as
/// the preset. A theme file named like a built-in scheme replaces its row when its colours differ
/// and leaves it when they do not.
pub fn colors(themes_dir: &Path) -> Vec<ColorEntry> {
    use crate::color_schemes as cs;
    let folder = cs::dir_of(themes_dir);
    let mut shown: Vec<(ColorEntry, Option<Theme>)> = Vec::new();
    let push_named = |name: &str, shown: &mut Vec<(ColorEntry, Option<Theme>)>| {
        let colour_file = folder.join(format!("{name}.toml")).is_file();
        let theme_file = themes_dir.join(format!("{name}.toml")).is_file();
        let scheme = cs::builtin(name);
        let row = match load_color(name, themes_dir) {
            Ok(t) => {
                let own = colour_file || (theme_file && !scheme.as_ref().is_some_and(|b| b.same_colors(&t)));
                let entry = match scheme.filter(|_| !own) {
                    Some(b) => ColorEntry { name: name.to_string(), title: b.title.clone().unwrap_or_else(|| title_case(name)), appearance: Some(b.appearance.unwrap_or_else(|| background_kind(b.chrome.window_bg))), source: Source::BuiltIn },
                    None => ColorEntry { name: name.to_string(), title: t.title.clone().unwrap_or_else(|| title_case(name)), appearance: Some(t.kind()), source: Source::File },
                };
                (entry, Some(t))
            }
            Err(e) => (ColorEntry { name: name.to_string(), title: title_case(name), appearance: None, source: Source::Invalid(e) }, None),
        };
        shown.push(row);
    };
    for (name, _) in cs::BUILTIN {
        push_named(name, &mut shown);
    }
    let mut stems: Vec<String> = std::fs::read_dir(&folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.path().file_stem().filter(|_| e.path().extension().and_then(|x| x.to_str()) == Some("toml")).and_then(|s| s.to_str()).map(String::from))
        .filter(|n| !cs::BUILTIN.iter().any(|(b, _)| b == n))
        .collect();
    stems.sort();
    for stem in stems {
        push_named(&stem, &mut shown);
    }
    for (stem, t) in old_form_presets(themes_dir) {
        if shown.iter().any(|(e, s)| e.name == stem || s.as_ref().is_some_and(|s| same_colors(s, &t))) {
            continue;
        }
        let entry = ColorEntry { title: t.title.clone().unwrap_or_else(|| title_case(&stem)), name: stem, appearance: Some(t.kind()), source: Source::File };
        shown.push((entry, Some(t)));
    }
    shown.into_iter().map(|(e, _)| e).collect()
}

/// One font set as the Font list shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct FontEntry {
    pub name: String,
    pub title: String,
    pub font: ThemeFont,
}

/// Every font set to show, each once: the built-in sets, the other font files sorted (one that
/// does not parse is left out), then the non-default fonts of each old-form preset file of
/// `themes_dir` that match no set listed so far, named and titled as the preset.
pub fn font_sets(themes_dir: &Path) -> Vec<FontEntry> {
    use crate::font_sets as fs;
    let folder = fs::dir_of(themes_dir);
    let mut out: Vec<FontEntry> = Vec::new();
    let add = |out: &mut Vec<FontEntry>, name: &str| {
        if let Ok(set) = fs::load(name, themes_dir) {
            out.push(FontEntry { name: name.to_string(), title: set.title.clone().unwrap_or_else(|| title_case(name)), font: set.font() });
        }
    };
    for (name, _) in fs::BUILTIN {
        add(&mut out, name);
    }
    let mut stems: Vec<String> = std::fs::read_dir(&folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.path().file_stem().filter(|_| e.path().extension().and_then(|x| x.to_str()) == Some("toml")).and_then(|s| s.to_str()).map(String::from))
        .filter(|n| !fs::BUILTIN.iter().any(|(b, _)| b == n))
        .collect();
    stems.sort();
    for stem in stems {
        add(&mut out, &stem);
    }
    for (_, t) in old_form_presets(themes_dir) {
        if t.font.is_default() || out.iter().any(|e| e.font == t.font) {
            continue;
        }
        out.push(FontEntry { title: t.title.clone().unwrap_or_else(|| title_case(&t.name)), name: t.name.clone(), font: t.font.clone() });
    }
    out
}

/// A text size of `size` points on a font's `grid`: the nearest multiple of it, and never under
/// one step. Without a grid, the size itself.
pub fn snap_to(grid: Option<f32>, size: f32) -> f32 {
    match grid {
        Some(g) if g > 0.0 => (size / g).round().max(1.0) * g,
        _ => size,
    }
}

/// The fonts a look draws in. `choice` is the custom look's font (`Config::font_choice`): no
/// choice gives the fonts of `theme`, the one showing; `config::PLAIN_FONT` none of a theme's; a
/// name gives, in this order, the font file of that name, the fonts of a theme file of that name
/// (old or new form, so an edited copy of a preset keeps its edits), the built-in font set, and
/// the fonts of the built-in preset, which is how a 0.8.54 config naming a preset reads. A name
/// that is none of these is the error.
pub fn font_for(choice: Option<&str>, theme: &Theme, themes_dir: &Path) -> Result<ThemeFont, String> {
    match choice {
        None => Ok(theme.font.clone()),
        Some(crate::config::PLAIN_FONT) => Ok(ThemeFont::default()),
        Some(name) => {
            // Your own font file, then your own theme file (old or new form), then the built-in
            // set, then a built-in preset.
            let own_font = crate::font_sets::dir_of(themes_dir).join(format!("{name}.toml")).is_file();
            let own_theme = is_bare_name(name) && themes_dir.join(format!("{name}.toml")).is_file();
            if own_font {
                return crate::font_sets::load(name, themes_dir).map(|s| s.font());
            }
            if own_theme {
                return load(name, themes_dir).map(|t| t.font);
            }
            match crate::font_sets::load(name, themes_dir) {
                Ok(set) => Ok(set.font()),
                Err(set_err) => load(name, themes_dir).map(|t| t.font).map_err(|preset_err| format!("{set_err}; {preset_err}")),
            }
        }
    }
}

/// Every theme that loads, of `themes_dir` and the built-ins in `list`'s order, with its fonts:
/// what a custom look's font may be picked from.
#[cfg(test)]
pub fn fonts(themes_dir: &Path) -> Vec<(String, ThemeFont)> {
    list(themes_dir).into_iter().filter_map(|(name, _)| load(&name, themes_dir).ok().map(|t| (name, t.font))).collect()
}

impl ThemeFont {
    pub fn is_default(&self) -> bool {
        *self == ThemeFont::default()
    }

    /// The same fonts with every family that is not installed left out, and those families'
    /// names: a theme asking for a font this Mac lacks falls back to what the slot shows
    /// without one, rather than to whatever macOS substitutes.
    pub fn installed(&self, families: &std::collections::HashSet<String>) -> (ThemeFont, Vec<String>) {
        let mut missing = Vec::new();
        let mut keep = |f: &Option<String>| match f {
            Some(name) if !families.contains(name) => {
                missing.push(name.clone());
                None
            }
            other => other.clone(),
        };
        let (prose, ui, terminal) = (keep(&self.prose), keep(&self.ui), keep(&self.terminal));
        let grid = self.grid.filter(|_| terminal.is_some());
        let fonts = ThemeFont { prose, ui, terminal, grid };
        (fonts, missing)
    }
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
pub const BUILTIN: [(&str, &str); 14] = [
    ("navy", include_str!("../themes/navy.toml")),
    ("timo-purple", include_str!("../themes/timo-purple.toml")),
    ("peach-watercolor", include_str!("../themes/peach-watercolor.toml")),
    ("lavender-watercolor", include_str!("../themes/lavender-watercolor.toml")),
    ("lavender-material", include_str!("../themes/lavender-material.toml")),
    ("matrix", include_str!("../themes/matrix.toml")),
    ("tokyo-rain", include_str!("../themes/tokyo-rain.toml")),
    ("composition-notebook", include_str!("../themes/composition-notebook.toml")),
    ("material", include_str!("../themes/material.toml")),
    ("arabia-solar", include_str!("../themes/arabia-solar.toml")),
    ("material-solar", include_str!("../themes/material-solar.toml")),
    ("material-ocean", include_str!("../themes/material-ocean.toml")),
    ("material-palenight", include_str!("../themes/material-palenight.toml")),
    ("material-sublime", include_str!("../themes/material-sublime.toml")),
];

/// The luminance a glyph shader sees: the sRGB dot product `0.2126 r + 0.7152 g + 0.0722 b` of
/// the components over 255, with no linearising. It is not the WCAG luminance the contrast
/// check uses. The runic shader draws a glyph under 0.45 as dark text, without a halo, and the
/// terminal's decode (`effects::decode::DARK_TEXT`) freshens such text toward a deep green.
pub fn luminance(c: Rgb) -> f32 {
    let ch = |v: u8| f32::from(v) / 255.0;
    0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
}

/// A theme from its file's text. A declared `appearance` must agree with the window background,
/// since the contrast rules and Settings' lists go by it.
pub fn parse(text: &str) -> Result<Theme, String> {
    parse_in(text, None)
}

/// A preset file in the new form: it names a colour scheme and a font set instead of holding
/// their tables.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetFile {
    name: String,
    title: Option<String>,
    group: Option<String>,
    #[serde(default)]
    group_default: bool,
    color: String,
    font: Option<String>,
    effect: Option<ThemeEffect>,
}

/// A theme from its file's text. A text with a top-level `color` key is a preset in the new form,
/// its colour scheme and font set looked up in the folders beside `themes_dir` and then among the
/// built-ins (only the built-ins when `themes_dir` is None). Any other text is the old all-in-one
/// form, a theme whose colour scheme is its own name.
pub fn parse_in(text: &str, themes_dir: Option<&Path>) -> Result<Theme, String> {
    let first_line = |e: toml::de::Error| e.message().lines().next().unwrap_or("invalid theme").to_string();
    let is_new_form = toml::from_str::<toml::Table>(text).map(|t| t.contains_key("color")).unwrap_or(false);
    if is_new_form {
        let preset: PresetFile = toml::from_str(text).map_err(first_line)?;
        let scheme = match themes_dir {
            Some(dir) => crate::color_schemes::load(&preset.color, dir)?,
            None => crate::color_schemes::builtin(&preset.color).ok_or_else(|| format!("colour scheme {:?} is not built in", preset.color))?,
        };
        let font = match preset.font.as_deref() {
            None | Some("default") => ThemeFont::default(),
            Some(set) => match themes_dir {
                Some(dir) => crate::font_sets::load(set, dir)?,
                None => crate::font_sets::builtin(set).ok_or_else(|| format!("font set {set:?} is not built in"))?,
            }
            .font(),
        };
        let mut theme = scheme.to_theme();
        theme.name = preset.name;
        theme.title = preset.title;
        theme.group = preset.group;
        theme.group_default = preset.group_default;
        theme.font = font;
        theme.effect = preset.effect;
        theme.color = preset.color;
        return Ok(theme);
    }
    let mut theme: Theme = toml::from_str(text).map_err(first_line)?;
    theme.color = theme.name.clone();
    old_form_checks(theme)
}

/// The checks an old-form theme gets: its appearance against its background, its grid in range.
fn old_form_checks(theme: Theme) -> Result<Theme, String> {
    if let Some(declared) = theme.appearance {
        let by_background = background_kind(theme.chrome.window_bg);
        if declared != by_background {
            return Err(format!(
                "appearance = \"{}\" but window_bg {} is {}; make one of them match the other",
                declared.word(),
                theme.chrome.window_bg.hex(),
                by_background.word()
            ));
        }
    }
    if let Some(grid) = theme.font.grid.filter(|g| !(1.0..=24.0).contains(g)) {
        return Err(format!("font.grid must be 1.0 to 24.0 (was {grid:?})"));
    }
    Ok(theme)
}

/// What a window background makes a theme: light when its luminance is over a half.
fn background_kind(window_bg: Rgb) -> Appearance {
    if crate::palette::luminance(window_bg) > 0.5 {
        Appearance::Light
    } else {
        Appearance::Dark
    }
}

/// Built-in themes that were renamed: (the old name, the name now). A config, a command or a
/// story written before the rename still names the old one.
pub const RENAMED: &[(&str, &str)] = &[("peach-light", "peach-watercolor"), ("peach-material", "peach-watercolor"), ("lavender-light", "lavender-watercolor"), ("navy-runic", "tokyo-rain")];

/// The theme's name now: a built-in's old name gives its new one, any other name itself.
pub fn current_name(name: &str) -> &str {
    RENAMED.iter().find(|(old, _)| *old == name).map_or(name, |(_, new)| new)
}

/// The file text of the built-in theme `name`, by its name now or an old one (`RENAMED`).
pub fn builtin_text(name: &str) -> Option<&'static str> {
    let name = current_name(name);
    BUILTIN.iter().find(|(n, _)| *n == name).map(|(_, t)| *t)
}

/// The built-in theme `name`, by its name now or an old one (`RENAMED`).
pub fn builtin(name: &str) -> Option<Theme> {
    let text = builtin_text(name)?;
    // The embedded files are tested to parse; a failure here is a build error, not a user one.
    parse(text).ok()
}

/// True for a name that is a bare file stem: no separators, no parent references, not empty.
pub(crate) fn is_bare_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && name != "." && name != ".."
}

/// `dir/<name>.toml` if it exists, else the built-in preset of that name, else the colour scheme
/// of that name as a theme of its colours alone. The error names the preset and the scheme.
pub fn load(name: &str, dir: &Path) -> Result<Theme, String> {
    if !is_bare_name(name) {
        return Err(format!("theme name {name:?} must be a bare file name"));
    }
    let file = dir.join(format!("{name}.toml"));
    match std::fs::read_to_string(&file) {
        Ok(text) => parse_in(&text, Some(dir)).map_err(|e| format!("{}: {e}", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => builtin(name).map(Ok).unwrap_or_else(|| {
            crate::color_schemes::load(name, dir).map(|s| s.to_theme()).map_err(|e| {
                // A colour file that is there but does not parse says why, with its path.
                if crate::color_schemes::dir_of(dir).join(format!("{name}.toml")).is_file() {
                    e
                } else {
                    format!("{name:?} is no theme (not built in and {} does not exist) and no colour scheme", file.display())
                }
            })
        }),
        Err(e) => Err(format!("cannot read {}: {e}", file.display())),
    }
}

/// The theme the config shows: the custom look's colour when it names one (`load_color`), else
/// the preset the mode and macOS's appearance pick (`load`).
pub fn load_look(cfg: &crate::config::Config, system_is_dark: bool, dir: &Path) -> Result<Theme, String> {
    let name = cfg.theme_name(system_is_dark);
    if cfg.look() == crate::config::Look::Custom && cfg.custom.color.is_some() {
        load_color(name, dir)
    } else {
        load(name, dir)
    }
}

/// Whether two themes draw the same colours (the five colour tables).
pub(crate) fn same_colors(a: &Theme, b: &Theme) -> bool {
    a.chrome == b.chrome && a.terminal == b.terminal && a.semantic == b.semantic && a.tree == b.tree && a.viewer == b.viewer
}

/// The theme a custom colour name gives: the colour file `colors/<name>.toml`, else the theme file
/// `themes/<name>.toml` (its colours, and its fonts and effect as a preset has them), else the
/// built-in colour scheme, else the built-in preset of that name (a 0.8.54 config names one). A
/// name that is both a built-in scheme and a built-in preset with the same colours gives the
/// preset, so its fonts are kept. The error is the first file's own when that file is broken.
pub fn load_color(name: &str, dir: &Path) -> Result<Theme, String> {
    use crate::color_schemes as cs;
    if !is_bare_name(name) {
        return Err(format!("theme name {name:?} must be a bare file name"));
    }
    if cs::dir_of(dir).join(format!("{name}.toml")).is_file() {
        return cs::load(name, dir).map(|s| s.to_theme());
    }
    if dir.join(format!("{name}.toml")).is_file() {
        return load(name, dir);
    }
    if let Some(scheme) = cs::builtin(name) {
        return Ok(builtin(name).filter(|p| scheme.same_colors(p)).unwrap_or_else(|| scheme.to_theme()));
    }
    load(name, dir)
}

/// The colour list's name for the colour a config names: the name itself when a row has it, else
/// the row that draws the colours the name loads (an old-form copy of a built-in preset, as
/// `theme extract` wrote before 0.8.55, is the scheme it copied), else the colour scheme the
/// theme names, so the list shows what is in force.
pub fn current_color(name: &str, entries: &[ColorEntry], dir: &Path) -> String {
    if entries.iter().any(|e| e.name == name) {
        return name.to_string();
    }
    let Ok(t) = load_color(name, dir) else { return name.to_string() };
    entries
        .iter()
        .find(|e| load_color(&e.name, dir).is_ok_and(|x| same_colors(&x, &t)))
        .map_or(t.color.clone(), |e| e.name.clone())
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

/// Parses the theme file at `path`. Ok is the theme's own `name`; a missing or unreadable file is
/// an error like any other.
pub fn validate_file(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    parse_in(&text, path.parent()).map(|t| t.name)
}

/// Where a listed theme comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    BuiltIn,
    File,
    Invalid(String),
}

/// Every theme by name: the built-ins first, then the folder's other files sorted. A file that
/// shadows a built-in is listed once, in the built-in's place, as a file.
pub fn list(dir: &Path) -> Vec<(String, Source)> {
    let from_file = |name: &str| -> Option<Source> {
        let file = dir.join(format!("{name}.toml"));
        if !file.is_file() {
            return None;
        }
        Some(match validate_file(&file) {
            Ok(_) => Source::File,
            Err(e) => Source::Invalid(e),
        })
    };
    let mut out: Vec<(String, Source)> =
        BUILTIN.iter().map(|(n, _)| (n.to_string(), from_file(n).unwrap_or(Source::BuiltIn))).collect();
    let mut stems: Vec<String> = names(dir).into_iter().filter(|n| !BUILTIN.iter().any(|(b, _)| b == n)).collect();
    stems.sort();
    out.extend(stems.into_iter().filter_map(|n| from_file(&n).map(|s| (n, s))));
    out
}

/// A theme id in capitalised words, the way a person reads it: `my-dark-theme` is "My Dark
/// Theme". A theme file's `title` says otherwise.
pub fn title_case(name: &str) -> String {
    name.split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// How Settings and the palette show a theme: its title (its own, else its id in capitalised
/// words), and the group it sits in with whether it is that group's default.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shown {
    pub title: String,
    pub group: Option<String>,
    pub group_default: bool,
}

/// `Shown` for every theme `list` gives: from the theme when it loads, else from its id.
pub fn shown(dir: &Path) -> std::collections::BTreeMap<String, Shown> {
    list(dir)
        .into_iter()
        .map(|(name, _)| {
            let t = load(&name, dir).ok();
            let s = Shown {
                title: t.as_ref().and_then(|t| t.title.clone()).unwrap_or_else(|| title_case(&name)),
                group: t.as_ref().and_then(|t| t.group.clone()),
                group_default: t.as_ref().is_some_and(|t| t.group_default),
            };
            (name, s)
        })
        .collect()
}

/// A theme's title by id, for a list without `shown`'s map: its own, else its id in capitalised
/// words.
pub fn title_of(name: &str, dir: &Path) -> String {
    load(name, dir).ok().and_then(|t| t.title).unwrap_or_else(|| title_case(name))
}

/// `list` with each theme's appearance: dark or light, or none for a file that does not load.
pub fn kinds(dir: &Path) -> Vec<(String, Source, Option<Appearance>)> {
    list(dir)
        .into_iter()
        .map(|(name, source)| {
            let kind = match source {
                Source::Invalid(_) => None,
                _ => load(&name, dir).ok().map(|t| t.kind()),
            };
            (name, source, kind)
        })
        .collect()
}

/// Copies the theme file `src` into `dir` under the name the theme itself declares. Only `.toml`
/// files are taken. Ok is the name and whether a file of that name was already there; without
/// `replace` an existing file is left alone and the error is "exists", as it is for a built-in's
/// name, which the copy would shadow.
pub fn import_file(src: &Path, dir: &Path, replace: bool) -> Result<(String, bool), String> {
    if src.extension().and_then(|e| e.to_str()) != Some("toml") {
        return Err("only .toml theme files can be added".into());
    }
    let text = std::fs::read_to_string(src).map_err(|e| format!("cannot read {}: {e}", src.display()))?;
    let name = parse_in(&text, Some(dir))?.name;
    if !is_bare_name(&name) || name.starts_with('.') {
        return Err(format!("theme name {name:?} must be a bare file name that does not start with a dot"));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let dest = dir.join(format!("{name}.toml"));
    let existed = dest.exists();
    let shadows_builtin = BUILTIN.iter().any(|(b, _)| *b == name);
    if (existed || shadows_builtin) && !replace {
        return Err("exists".into());
    }
    std::fs::write(&dest, text).map_err(|e| format!("cannot write {}: {e}", dest.display()))?;
    Ok((name, existed))
}

const THEME_USAGE: &str = "usage: arkos theme list [--json]\n       arkos theme validate <file> [--json]\n";

/// `arkos theme list` and `arkos theme validate` against an explicit folder and streams (the
/// other subcommands live in `agent_cli.rs`, which hands these two here). Returns the exit code:
/// 0 on success, 1 when `validate` finds a problem, 2 for a usage error. `--json` anywhere prints
/// one JSON value instead of lines.
pub fn theme_cli_to(args: &[String], dir: &Path, out: &mut dyn std::io::Write, err: &mut dyn std::io::Write) -> i32 {
    let json = args.iter().any(|a| a == "--json");
    let rest: Vec<&str> = args.iter().map(String::as_str).filter(|a| *a != "--json").collect();
    match rest.as_slice() {
        ["list"] if json => {
            let rows: Vec<serde_json::Value> = kinds(dir)
                .into_iter()
                .map(|(name, source, kind)| {
                    let path = dir.join(format!("{name}.toml")).display().to_string();
                    let kind = kind.map(Appearance::word);
                    match source {
                        Source::BuiltIn => serde_json::json!({ "name": name, "source": "built-in", "appearance": kind }),
                        Source::File => serde_json::json!({ "name": name, "source": "file", "appearance": kind, "path": path }),
                        Source::Invalid(e) => serde_json::json!({ "name": name, "source": "invalid", "error": e, "path": path }),
                    }
                })
                .collect();
            let _ = writeln!(out, "{}", serde_json::Value::Array(rows));
            0
        }
        ["list"] => {
            for (name, source, kind) in kinds(dir) {
                let kind = kind.map_or("", Appearance::word);
                let _ = match source {
                    Source::BuiltIn => writeln!(out, "{name} (built in, {kind})"),
                    Source::File => writeln!(out, "{name} (file, {kind})"),
                    Source::Invalid(e) => writeln!(out, "{name} (file, invalid: {e})"),
                };
            }
            0
        }
        ["validate", file] => {
            let result = validate_file(Path::new(file));
            if json {
                let v = match &result {
                    Ok(name) => serde_json::json!({ "ok": true, "name": name }),
                    Err(e) => serde_json::json!({ "ok": false, "error": e }),
                };
                let _ = writeln!(out, "{v}");
            }
            match result {
                Ok(name) => {
                    if !json {
                        let _ = writeln!(out, "ok: {name}");
                    }
                    0
                }
                Err(e) => {
                    if !json {
                        let _ = writeln!(err, "{e}");
                    }
                    1
                }
            }
        }
        _ => {
            let _ = err.write_all(THEME_USAGE.as_bytes());
            2
        }
    }
}

/// One colour drawn as text or a mark, the surface it sits on, and the contrast ratio it needs.
/// `what` reads `{table}.{key} on {surface}` (chrome surfaces are named bare, as `window_bg`), or
/// `ansi {i} on terminal.background` for the sixteen terminal colours.
#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    pub what: String,
    pub fg: Rgb,
    pub bg: Rgb,
    pub needs: f64,
}

/// Every pair the contrast check holds a theme to: 4.5:1 for text and marks, 7:1 for the chrome
/// text on the window, 11:1 for the terminal text. Black is a background by convention on a dark
/// theme and a text colour on a light one, so ANSI 0 is checked only on a light theme.
pub fn contrast_pairs(t: &Theme) -> Vec<Pair> {
    let (c, term) = (&t.chrome, &t.terminal);
    let mut v = Vec::new();
    let mut add = |what: String, fg: Rgb, bg: Rgb, needs: f64| v.push(Pair { what, fg, bg, needs });
    add("chrome.text on window_bg".into(), c.text, c.window_bg, 7.0);
    add("chrome.text_dim on window_bg".into(), c.text_dim, c.window_bg, 4.5);
    add("chrome.text on title_bar".into(), c.text, c.title_bar, 4.5);
    add("chrome.text_dim on status_bar".into(), c.text_dim, c.status_bar, 4.5);
    add("chrome.text_dim on tab_bar".into(), c.text_dim, c.tab_bar, 4.5);
    add("chrome.text on banner".into(), c.text, c.banner, 4.5);
    let s = &t.semantic;
    for (key, fg) in [("working", s.working), ("waiting", s.waiting), ("bell", s.bell)] {
        add(format!("semantic.{key} on terminal.background"), fg, term.background, 4.5);
    }
    let tr = &t.tree;
    for (key, fg) in [("added", tr.added), ("modified", tr.modified), ("deleted", tr.deleted), ("untracked", tr.untracked), ("ignored", tr.ignored)] {
        add(format!("tree.{key} on window_bg"), fg, c.window_bg, 4.5);
    }
    let vw = &t.viewer;
    let viewer = [
        ("keyword", vw.keyword),
        ("string", vw.string),
        ("comment", vw.comment),
        ("number", vw.number),
        ("function", vw.function),
        ("type_name", vw.type_name),
        ("constant", vw.constant),
        ("operator", vw.operator),
        ("punctuation", vw.punctuation),
    ];
    for (key, fg) in viewer {
        add(format!("viewer.{key} on window_bg"), fg, c.window_bg, 4.5);
    }
    add("terminal.foreground on terminal.background".into(), term.foreground, term.background, 11.0);
    for (i, fg) in t.palette().ansi.iter().enumerate() {
        if i == 0 && !t.is_light() {
            continue;
        }
        add(format!("ansi {i} on terminal.background"), *fg, term.background, 4.5);
    }
    // The other surfaces text sits on, first checked for Timo Purple in milestone 8a.
    add("chrome.text_dim on banner".into(), c.text_dim, c.banner, 4.5);
    add("chrome.text_dim on sidebar".into(), c.text_dim, c.sidebar, 4.5);
    add("chrome.text_dim on tab_inactive".into(), c.text_dim, c.tab_inactive, 4.5);
    add("chrome.text on tab_inactive".into(), c.text, c.tab_inactive, 4.5);
    add("terminal.foreground on terminal.selection".into(), term.foreground, term.selection, 4.5);
    add("terminal.foreground on semantic.find_match".into(), term.foreground, s.find_match, 4.5);
    add("terminal.foreground on semantic.find_current".into(), term.foreground, s.find_current, 4.5);
    v
}

/// Every pair of `contrast_pairs` with its ratio, and whether all of them reach what they need.
pub fn contrast_report(t: &Theme) -> (bool, Vec<(Pair, f64)>) {
    let rows: Vec<(Pair, f64)> = contrast_pairs(t)
        .into_iter()
        .map(|p| {
            let ratio = crate::palette::contrast(p.fg, p.bg);
            (p, ratio)
        })
        .collect();
    (rows.iter().all(|(p, r)| *r >= p.needs), rows)
}

/// One key of a theme file, for `arkos theme template --json`: what it paints and, for a colour
/// drawn as text or a mark, the surfaces it must contrast with (comma separated, named as in
/// `contrast_pairs`) and the strictest ratio it needs there.
pub struct KeyDoc {
    pub key: &'static str,
    pub meaning: &'static str,
    pub contrast_with: Option<&'static str>,
    pub needs: Option<f64>,
}

/// One table of a theme file; the top-level `name` is the table named "".
pub struct TableDoc {
    pub name: &'static str,
    pub keys: &'static [KeyDoc],
}

const fn surface(key: &'static str, meaning: &'static str) -> KeyDoc {
    KeyDoc { key, meaning, contrast_with: None, needs: None }
}

const fn ink(key: &'static str, meaning: &'static str, contrast_with: &'static str, needs: f64) -> KeyDoc {
    KeyDoc { key, meaning, contrast_with: Some(contrast_with), needs: Some(needs) }
}

const ANSI_BG: &str = "terminal.background";

const SCHEMA: &[TableDoc] = &[
    TableDoc { name: "", keys: &[surface("name", "The theme's own name, a bare file stem; stored as <name>.toml and named in [theme] dark or light"), surface("appearance", "dark or light: which [theme] slot Settings lists it under; it must agree with chrome.window_bg (optional, else read from it)"), surface("title", "Optional: the name Settings and the palette show (\"Plain Navy\"); absent, the name in capitalised words"), surface("group", "Optional: a group Settings lists the theme under, one row with a choice of its members (\"Watercolor\")"), surface("group_default", "Optional, true or false: whether the theme is the member a click on its group's row picks")] },
    TableDoc {
        name: "chrome",
        keys: &[
            surface("window_bg", "The window background behind everything"),
            surface("title_bar", "The title bar"),
            surface("status_bar", "The status bar at the bottom"),
            surface("sidebar", "The file tree and project sidebar"),
            surface("tab_bar", "The strip behind the tabs"),
            surface("tab_active", "The selected tab"),
            surface("tab_inactive", "The other tabs"),
            surface("border", "Lines between panes and panels (a divider, not text)"),
            surface("split_handle", "Unused (a handle at rest paints nothing); kept so files that name it still load"),
            surface("split_handle_hover", "The stripe over a split divider under the pointer or being dragged"),
            surface("focus_ring", "The outline and accent on the focused item"),
            ink("text", "Primary interface text (7:1 on window_bg, 4.5:1 on the others)", "window_bg, title_bar, banner, tab_inactive", 7.0),
            ink("text_dim", "Secondary text such as hints and paths", "window_bg, status_bar, tab_bar, banner, sidebar, tab_inactive", 4.5),
            surface("banner", "The background of notice banners"),
        ],
    },
    TableDoc {
        name: "terminal",
        keys: &[
            surface("background", "The terminal background"),
            ink(
                "foreground",
                "Default terminal text (11:1 on the background, 4.5:1 on the others)",
                "terminal.background, terminal.selection, semantic.find_match, semantic.find_current",
                11.0,
            ),
            surface("cursor", "The cursor block"),
            surface("selection", "The background of selected text"),
            ink("black", "ANSI colour 0 (checked only on a light theme, where it is a text colour)", ANSI_BG, 4.5),
            ink("red", "ANSI colour 1", ANSI_BG, 4.5),
            ink("green", "ANSI colour 2", ANSI_BG, 4.5),
            ink("yellow", "ANSI colour 3", ANSI_BG, 4.5),
            ink("blue", "ANSI colour 4", ANSI_BG, 4.5),
            ink("magenta", "ANSI colour 5", ANSI_BG, 4.5),
            ink("cyan", "ANSI colour 6", ANSI_BG, 4.5),
            ink("white", "ANSI colour 7", ANSI_BG, 4.5),
            ink("bright_black", "ANSI colour 8, also used for dimmed text", ANSI_BG, 4.5),
            ink("bright_red", "ANSI colour 9", ANSI_BG, 4.5),
            ink("bright_green", "ANSI colour 10", ANSI_BG, 4.5),
            ink("bright_yellow", "ANSI colour 11", ANSI_BG, 4.5),
            ink("bright_blue", "ANSI colour 12", ANSI_BG, 4.5),
            ink("bright_magenta", "ANSI colour 13", ANSI_BG, 4.5),
            ink("bright_cyan", "ANSI colour 14", ANSI_BG, 4.5),
            ink("bright_white", "ANSI colour 15", ANSI_BG, 4.5),
        ],
    },
    TableDoc {
        name: "semantic",
        keys: &[
            ink("working", "The mark on a tab whose agent is working", ANSI_BG, 4.5),
            ink("waiting", "The mark on a tab waiting for you", ANSI_BG, 4.5),
            ink("bell", "The mark on a tab that rang the bell", ANSI_BG, 4.5),
            surface("error_banner", "The background of an error banner"),
            surface("find_match", "The background of a search match in the terminal"),
            surface("find_current", "The background of the current search match"),
        ],
    },
    TableDoc {
        name: "tree",
        keys: &[
            ink("added", "Files added to the index, in the file tree", "window_bg", 4.5),
            ink("modified", "Files changed in the working tree", "window_bg", 4.5),
            ink("deleted", "Files removed", "window_bg", 4.5),
            ink("untracked", "Files git does not know yet", "window_bg", 4.5),
            ink("ignored", "Files matched by an ignore rule", "window_bg", 4.5),
        ],
    },
    TableDoc {
        name: "viewer",
        keys: &[
            ink("keyword", "Language keywords in the file viewer", "window_bg", 4.5),
            ink("string", "String literals", "window_bg", 4.5),
            ink("comment", "Comments", "window_bg", 4.5),
            ink("number", "Number literals", "window_bg", 4.5),
            ink("function", "Function names", "window_bg", 4.5),
            ink("type_name", "Type names", "window_bg", 4.5),
            ink("constant", "Constants", "window_bg", 4.5),
            ink("operator", "Operators", "window_bg", 4.5),
            ink("punctuation", "Brackets, commas and other punctuation", "window_bg", 4.5),
        ],
    },
    TableDoc {
        name: "font",
        keys: &[
            surface("prose", "Optional: the installed family for rendered markdown text; unset, the system UI font"),
            surface("ui", "Optional: the installed family for the window's chrome; unset, the terminal font"),
            surface("terminal", "Optional: the installed monospaced family for the shells and code views; unset, [font] family"),
            surface("grid", "Optional: the points the terminal font's own pixels are whole at (a pixel face); every text size is drawn at the nearest multiple of it"),
        ],
    },
    TableDoc {
        name: "effect",
        keys: &[
            surface("name", "Optional: the effect the theme was designed with, a built-in or an installed effect's folder name; the user's [effect] name in the config may choose another"),
            surface("scene", "Optional: which of the effect's scenes the theme shows, by a name its manifest lists in `scenes` (runic: \"tokyo\" or \"matrix-city\"); absent, the effect's first"),
        ],
    },
];

/// The theme file's tables and keys as data: the top-level `name` (a table named ""), then
/// chrome, terminal, semantic, tree, viewer and the optional font.
pub fn schema() -> &'static [TableDoc] {
    SCHEMA
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
            derived: Default::default(),
        }
    }

    /// A light theme has a bright window background; black is then a text colour.
    pub fn is_light(&self) -> bool {
        self.kind() == Appearance::Light
    }

    /// Dark or light: the theme's own `appearance`, or without one, its window background's.
    pub fn kind(&self) -> Appearance {
        self.appearance.unwrap_or_else(|| background_kind(self.chrome.window_bg))
    }
}

/// One colour `t` of the way from `a` to `b`, each channel rounded to the nearest step.
pub(crate) fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let channel = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round().clamp(0.0, 255.0) as u8;
    Rgb { r: channel(a.r, b.r), g: channel(a.g, b.g), b: channel(a.b, b.b) }
}

/// Mixes every colour field of two values of one struct, `t` of the way from `a` to `b`.
macro_rules! blend_fields {
    ($ty:ident, $a:expr, $b:expr, $t:expr; $($field:ident),+ $(,)?) => {
        $ty { $($field: mix($a.$field, $b.$field, $t),)+ }
    };
}

/// The theme `t` of the way from `from` to `to`: every colour interpolated per channel, with `t`
/// clamped to 0..1. The name and the fonts are `to`'s throughout (fonts cannot fade). The window
/// shows a run of these over `motion::THEME_FADE` after a switch, so nothing that reads the theme
/// may cache a colour, and anything derived from it once must wait for `t` to reach 1.
pub fn blend(from: &Theme, to: &Theme, t: f32) -> Theme {
    let t = if t.is_nan() { 1.0 } else { t.clamp(0.0, 1.0) };
    let (a, b) = (from, to);
    Theme {
        name: to.name.clone(),
        title: to.title.clone(),
        group: to.group.clone(),
        group_default: to.group_default,
        chrome: blend_fields!(Chrome, a.chrome, b.chrome, t;
            window_bg, title_bar, status_bar, sidebar, tab_bar, tab_active, tab_inactive, border,
            split_handle, split_handle_hover, focus_ring, text, text_dim, banner),
        terminal: blend_fields!(TerminalColors, a.terminal, b.terminal, t;
            background, foreground, cursor, selection, black, red, green, yellow, blue, magenta, cyan, white,
            bright_black, bright_red, bright_green, bright_yellow, bright_blue, bright_magenta, bright_cyan, bright_white),
        semantic: blend_fields!(Semantic, a.semantic, b.semantic, t; working, waiting, bell, error_banner, find_match, find_current),
        tree: blend_fields!(TreeColors, a.tree, b.tree, t; added, modified, deleted, untracked, ignored),
        viewer: blend_fields!(ViewerColors, a.viewer, b.viewer, t;
            keyword, string, comment, number, function, type_name, constant, operator, punctuation),
        font: to.font.clone(),
        effect: to.effect.clone(),
        appearance: to.appearance,
        color: to.color.clone(),
    }
}

/// Which theme name the config picks for the current appearance (spec 7.1: follow the system
/// by default, Navy for dark and Peach Watercolor for light).
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

    #[test]
    fn a_renamed_built_in_still_loads_by_its_old_name_and_is_listed_by_its_new_one() {
        assert_eq!(builtin("peach-light").unwrap().name, "peach-watercolor");
        assert_eq!(builtin("peach-material").unwrap().name, "peach-watercolor", "0.8.11's name for it");
        assert_eq!(builtin("lavender-light").unwrap().name, "lavender-watercolor");
        assert_eq!(builtin_text("peach-light"), builtin_text("peach-watercolor"));
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load("peach-light", dir.path()).unwrap().name, "peach-watercolor");
        let listed = names(dir.path());
        assert!(!listed.iter().any(|n| n == "peach-light" || n == "lavender-light"), "{listed:?}");
        std::fs::write(dir.path().join("peach-light.toml"), FIXTURES[0].1.replace("name = \"navy\"", "name = \"peach-light\"")).unwrap();
        assert_eq!(load("peach-light", dir.path()).unwrap().name, "peach-light", "a user's own file of the old name is theirs");
    }

    #[test]
    fn peach_and_lavender_each_wear_the_effect_their_names_say() {
        let effect = |n: &str| builtin(n).unwrap().recommended_effect().map(str::to_string);
        assert_eq!(effect("peach-watercolor").as_deref(), Some("watercolor"));
        assert_eq!(effect("lavender-material").as_deref(), Some("material"));
        assert_eq!(effect("lavender-watercolor").as_deref(), Some("watercolor"));
        let (a, b) = (builtin("lavender-material").unwrap(), builtin("lavender-watercolor").unwrap());
        assert_eq!((a.chrome, a.terminal, a.font), (b.chrome, b.terminal, b.font), "lavender-material is lavender's colours and fonts");
        assert_eq!(a.appearance, Some(Appearance::Light));
    }

    #[test]
    fn a_theme_may_name_a_prose_font() {
        let t = builtin("peach-watercolor").unwrap();
        assert_eq!(t.font.prose.as_deref(), Some("Baskerville"));
        assert_eq!(builtin("matrix").unwrap().font, ThemeFont::default(), "the others leave it to the system font");
        let text = FIXTURES.iter().find(|(n, _)| *n == "matrix").unwrap().1.to_string() + "\n[font]\nprose = \"Georgia\"\n";
        assert_eq!(parse(&text).unwrap().font.prose.as_deref(), Some("Georgia"));
        assert_eq!(t.font.ui.as_deref(), Some("Optima"));
        assert_eq!(t.font.terminal.as_deref(), Some("PT Mono"));
        assert!(parse(&(FIXTURES[0].1.to_string() + "\n[font]\nchrome = \"Menlo\"\n")).is_err(), "only prose, ui and terminal are fonts a theme may name");
    }

    #[test]
    fn navy_light_is_no_longer_built_in_and_peach_light_is_the_default_light_theme() {
        assert!(builtin("navy-light").is_none());
        assert!(!BUILTIN.iter().any(|(n, _)| *n == "navy-light"));
        assert_eq!(crate::config::ThemeConfig::default().light, "peach-watercolor");
        assert!(builtin("peach-watercolor").unwrap().is_light());
    }

    #[test]
    fn navy_the_default_recommends_material_and_lavender_watercolor_watercolor() {
        // Material Navy since 2026-11-17 (it drew plain as Plain Navy).
        assert_eq!(builtin("navy").unwrap().recommended_effect(), Some("material"));
        assert_eq!(builtin("lavender-watercolor").unwrap().recommended_effect(), Some("watercolor"));
        // A theme with no [effect] table recommends none, and any theme may recommend one: the
        // Matrix's file without its own table, given one.
        let bare = FIXTURES.iter().find(|(n, _)| *n == "matrix").unwrap().1.replace("[effect]\nname = \"runic\"\nscene = \"code\"\n", "");
        assert_eq!(parse(&bare).unwrap().recommended_effect(), None);
        let text = bare.clone() + "\n[effect]\nname = \"ink\"\n";
        assert_eq!(parse(&text).unwrap().recommended_effect(), Some("ink"));
        assert!(parse(&(bare + "\n[effect]\nname = \"a\"\nspeed = 2\n")).is_err(), "name is the only key");
    }

    #[test]
    fn a_font_s_grid_snaps_every_text_size_to_its_nearest_multiple() {
        // Departure Mono's pixels are whole every 5.5 points on a Retina screen.
        let g = Some(5.5);
        assert_eq!((snap_to(g, 14.0), snap_to(g, 18.0), snap_to(g, 16.5), snap_to(g, 20.0)), (16.5, 16.5, 16.5, 22.0), "the presets");
        assert_eq!((snap_to(g, 10.5), snap_to(g, 11.0), snap_to(g, 12.0), snap_to(g, 13.0)), (11.0, 11.0, 11.0, 11.0), "the chrome's sizes");
        assert_eq!(snap_to(g, 1.0), 5.5, "never under one step");
        assert_eq!((snap_to(None, 14.0), snap_to(Some(0.0), 14.0)), (14.0, 14.0), "no grid, no change");
        // The two pixel themes ask for it; no other built-in does.
        for (name, _) in BUILTIN {
            assert_eq!(builtin(name).unwrap().font.grid, matches!(name, "timo-purple" | "arabia-solar").then_some(5.5), "{name}");
        }
        let plain = FIXTURES.iter().find(|(n, _)| *n == "matrix").unwrap().1;
        assert_eq!(parse(&(plain.to_string() + "\n[font]\ngrid = 0.5\n")).unwrap_err(), "font.grid must be 1.0 to 24.0 (was 0.5)");
        assert_eq!(parse(&(plain.to_string() + "\n[font]\ngrid = 11\n")).unwrap().font.grid, Some(11.0));
    }

    #[test]
    fn a_custom_look_s_font_is_a_named_theme_s_whatever_colours_show() {
        let dir = tempfile::tempdir().unwrap();
        let (peach, material) = (load("peach-watercolor", dir.path()).unwrap(), load("material", dir.path()).unwrap());
        // No choice: the fonts of the theme showing, as under the presets.
        assert_eq!(font_for(None, &material, dir.path()).unwrap(), material.font);
        // A theme's name: its fonts over another theme's colours, the pixel face's grid with it.
        let pixel = font_for(Some("timo-purple"), &material, dir.path()).unwrap();
        assert_eq!((pixel.terminal.as_deref(), pixel.grid), (Some("Departure Mono"), Some(5.5)));
        assert_eq!(font_for(Some("peach-watercolor"), &peach, dir.path()).unwrap(), peach.font);
        // `default`: none of a theme's, so the config's family shows.
        assert_eq!(font_for(Some("default"), &peach, dir.path()).unwrap(), ThemeFont::default());
        assert!(font_for(Some("nope"), &peach, dir.path()).is_err());
    }

    #[test]
    fn every_theme_that_loads_is_listed_with_its_fonts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("broken.toml"), "name = \"broken\"\n").unwrap();
        let all = fonts(dir.path());
        assert_eq!(all.len(), BUILTIN.len(), "a file that does not load names no fonts");
        let of = |name: &str| all.iter().find(|(n, _)| n == name).map(|(_, f)| f.clone()).unwrap();
        assert_eq!(of("matrix"), ThemeFont::default());
        assert_eq!(of("navy"), of("material"), "Material Navy has Material's fonts");
        assert_eq!(of("timo-purple"), of("arabia-solar"));
        assert_eq!(of("peach-watercolor").prose.as_deref(), Some("Baskerville"));
    }

    #[test]
    fn a_font_that_is_not_installed_is_left_out() {
        let fonts = ThemeFont { prose: Some("Baskerville".into()), ui: Some("Optima".into()), terminal: Some("PT Mono".into()), grid: Some(5.5) };
        let have: std::collections::HashSet<String> = ["Baskerville", "Menlo"].iter().map(|s| s.to_string()).collect();
        let (kept, missing) = fonts.installed(&have);
        assert_eq!(kept, ThemeFont { prose: Some("Baskerville".into()), ui: None, terminal: None, grid: None }, "the grid is the terminal font's, and goes with it");
        let have_all: std::collections::HashSet<String> = ["Baskerville", "Optima", "PT Mono"].iter().map(|s| s.to_string()).collect();
        assert_eq!(fonts.installed(&have_all).0.grid, Some(5.5));
        assert_eq!(missing, vec!["Optima".to_string(), "PT Mono".to_string()]);
        assert_eq!(ThemeFont::default().installed(&have), (ThemeFont::default(), vec![]));
    }

    #[test]
    fn every_builtin_parses_and_names_itself() {
        for (name, text) in BUILTIN {
            let t = parse(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(t.name, *name);
            assert_eq!(builtin(name).as_ref(), Some(&t));
        }
        assert!(builtin("nvay").is_none());
        assert!(!builtin("navy").unwrap().is_light());
        assert!(builtin("peach-watercolor").unwrap().is_light());
        assert!(builtin("lavender-watercolor").unwrap().is_light());
        assert!(!builtin("matrix").unwrap().is_light());
    }

    #[test]
    fn matrix_is_built_in_soft_green_on_dark_green_grey_and_recommends_the_runic_effect() {
        let m = builtin("matrix").expect("matrix is built in");
        assert!(!m.is_light());
        // Softened 2026-11-17: off pure black, and the text a sage-mint rather than phosphor.
        assert_eq!(m.chrome.window_bg, Rgb { r: 0x0b, g: 0x10, b: 0x0d });
        assert_eq!(m.terminal.background, m.chrome.window_bg);
        assert_eq!(m.terminal.foreground, Rgb { r: 0xa9, g: 0xd1, b: 0xa6 });
        assert_eq!(m.recommended_effect(), Some("runic"));
        assert_eq!(BUILTIN.len(), 14);
    }

    #[test]
    fn tokyo_rain_draws_the_rain_colours_with_the_runic_city() {
        let rain = builtin("tokyo-rain").expect("tokyo-rain is built in");
        assert_eq!(rain.name, "tokyo-rain");
        assert_eq!(rain.color, "rain");
        assert!(crate::color_schemes::builtin("rain").unwrap().same_colors(&rain));
        assert_eq!(rain.font, ThemeFont::default(), "in the system's fonts");
        assert_eq!(rain.appearance, Some(Appearance::Dark));
        assert_eq!(rain.recommended_effect(), Some("runic"));
        assert_eq!(rain.effect.as_ref().and_then(|e| e.scene.as_deref()), Some("tokyo"));
        assert_eq!(builtin("navy").unwrap().recommended_effect(), Some("material"), "navy, the default, is Material Navy");
        // It was `navy-runic`: the old name still loads it, and a config naming it is moved.
        assert_eq!(current_name("navy-runic"), "tokyo-rain");
        assert_eq!(builtin("navy-runic").unwrap().name, "tokyo-rain");
        let dir = tempfile::tempdir().unwrap();
        let moved = crate::config::rename_themes("[theme]\ndark = \"navy-runic\"\nlight = \"peach-watercolor\"\n", dir.path()).unwrap();
        assert!(moved.contains("dark = \"tokyo-rain\""), "{moved}");
    }

    #[test]
    fn a_0_8_54_custom_colour_naming_a_preset_shows_that_preset_s_colour_scheme() {
        let dir = tempfile::tempdir().unwrap();
        let table = [
            ("navy", "navy"), ("material-ocean", "ocean"), ("material-palenight", "palenight"), ("material-sublime", "sublime"),
            ("material", "bright"), ("material-solar", "solar"), ("composition-notebook", "solar"), ("lavender-material", "lavender"),
            ("lavender-watercolor", "lavender"), ("peach-watercolor", "peach"), ("arabia-solar", "peach"), ("timo-purple", "timo-purple"),
            ("matrix", "matrix"), ("tokyo-rain", "rain"), ("navy-runic", "rain"), ("peach-light", "peach"), ("lavender-light", "lavender"),
        ];
        for (old, new) in table {
            let t = load(old, dir.path()).unwrap_or_else(|e| panic!("{old}: {e}"));
            assert_eq!(t.color, new, "{old}");
            assert!(crate::color_schemes::builtin(new).unwrap().same_colors(&t), "{old} draws the colours of {new}");
        }
    }

    #[test]
    fn a_0_8_54_custom_font_naming_a_preset_draws_that_preset_s_font_set() {
        let dir = tempfile::tempdir().unwrap();
        let showing = builtin("navy").unwrap();
        let table = [
            ("navy", "material"), ("material-ocean", "material"), ("material-palenight", "material"), ("material-sublime", "material"),
            ("material", "material"), ("material-solar", "material"), ("lavender-material", "lavender"), ("lavender-watercolor", "lavender"),
            ("peach-watercolor", "peach"), ("composition-notebook", "notebook"), ("timo-purple", "departure"), ("arabia-solar", "departure"),
        ];
        for (preset, set) in table {
            let got = font_for(Some(preset), &showing, dir.path()).unwrap();
            assert_eq!(got, crate::font_sets::builtin(set).unwrap().font(), "{preset}");
        }
        for preset in ["matrix", "tokyo-rain"] {
            assert_eq!(font_for(Some(preset), &showing, dir.path()).unwrap(), ThemeFont::default(), "{preset}");
        }
        assert!(font_for(Some("nowhere"), &showing, dir.path()).is_err());
    }

    #[test]
    fn a_colour_name_that_is_no_preset_loads_as_that_scheme_alone() {
        let dir = tempfile::tempdir().unwrap();
        let t = load("solar", dir.path()).unwrap();
        assert!(crate::color_schemes::builtin("solar").unwrap().same_colors(&t));
        assert_eq!((t.name.as_str(), t.color.as_str()), ("solar", "solar"));
        assert!(t.font.is_default());
        assert_eq!(t.effect, None);
        let e = load("nowhere", dir.path()).unwrap_err();
        assert!(e.contains("theme") && e.contains("colour scheme"), "{e}");
    }

    #[test]
    fn the_colour_list_shows_each_scheme_once() {
        let dir = tempfile::tempdir().unwrap();
        let names: Vec<String> = colors(dir.path()).into_iter().map(|c| c.name).collect();
        let want: Vec<String> = crate::color_schemes::BUILTIN.iter().map(|(n, _)| n.to_string()).collect();
        assert_eq!(names.len(), 11);
        assert_eq!(names, want);
    }

    #[test]
    fn extracted_copies_of_the_built_in_presets_are_not_listed_twice() {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in FIXTURES {
            std::fs::write(dir.path().join(format!("{name}.toml")), text).unwrap();
        }
        assert_eq!(colors(dir.path()).len(), 11);
        assert_eq!(font_sets(dir.path()).len(), 5);
        assert_eq!(load("lavender-watercolor", dir.path()).unwrap().font.prose.as_deref(), Some("Avenir Next"));
    }

    #[test]
    fn an_old_form_preset_with_colours_of_its_own_is_listed_under_its_title() {
        let dir = tempfile::tempdir().unwrap();
        let navy = FIXTURES[0].1;
        let text = navy.replace("name = \"navy\"", "name = \"mine\"").replace("title = \"Material Navy\"", "title = \"My Navy\"").replace("#16202f", "#16202e");
        assert_ne!(text, navy);
        std::fs::write(dir.path().join("mine.toml"), text).unwrap();
        let listed = colors(dir.path());
        assert_eq!(listed.len(), 12);
        let mine = listed.last().unwrap();
        assert_eq!((mine.name.as_str(), mine.title.as_str()), ("mine", "My Navy"));
        assert_eq!(mine.source, Source::File);
    }

    #[test]
    fn a_font_set_file_is_listed_by_its_title() {
        let root = tempfile::tempdir().unwrap();
        let themes = root.path().join("themes");
        std::fs::create_dir_all(&themes).unwrap();
        std::fs::create_dir_all(root.path().join("fonts")).unwrap();
        std::fs::write(root.path().join("fonts").join("mine.toml"), "name = \"mine\"\ntitle = \"My Fonts\"\nprose = \"Georgia\"\n").unwrap();
        let listed = font_sets(&themes);
        assert_eq!(listed.len(), 6);
        let mine = listed.last().unwrap();
        assert_eq!((mine.name.as_str(), mine.title.as_str()), ("mine", "My Fonts"));
        assert_eq!(mine.font.prose.as_deref(), Some("Georgia"));
    }

    /// The fourteen preset files as 0.8.54 shipped them, all-in-one.
    const FIXTURES: [(&str, &str); 14] = [
        ("navy", include_str!("../tests-data/presets-0.8.54/navy.toml")),
        ("timo-purple", include_str!("../tests-data/presets-0.8.54/timo-purple.toml")),
        ("peach-watercolor", include_str!("../tests-data/presets-0.8.54/peach-watercolor.toml")),
        ("lavender-watercolor", include_str!("../tests-data/presets-0.8.54/lavender-watercolor.toml")),
        ("lavender-material", include_str!("../tests-data/presets-0.8.54/lavender-material.toml")),
        ("matrix", include_str!("../tests-data/presets-0.8.54/matrix.toml")),
        ("tokyo-rain", include_str!("../tests-data/presets-0.8.54/tokyo-rain.toml")),
        ("composition-notebook", include_str!("../tests-data/presets-0.8.54/composition-notebook.toml")),
        ("material", include_str!("../tests-data/presets-0.8.54/material.toml")),
        ("arabia-solar", include_str!("../tests-data/presets-0.8.54/arabia-solar.toml")),
        ("material-solar", include_str!("../tests-data/presets-0.8.54/material-solar.toml")),
        ("material-ocean", include_str!("../tests-data/presets-0.8.54/material-ocean.toml")),
        ("material-palenight", include_str!("../tests-data/presets-0.8.54/material-palenight.toml")),
        ("material-sublime", include_str!("../tests-data/presets-0.8.54/material-sublime.toml")),
    ];

    #[test]
    fn every_built_in_preset_draws_exactly_as_it_did_in_0_8_54() {
        assert_eq!(FIXTURES.len(), BUILTIN.len());
        for (name, old_text) in FIXTURES {
            let old = toml::from_str::<Theme>(old_text).unwrap_or_else(|e| panic!("{name} fixture: {e}"));
            let new = builtin(name).unwrap_or_else(|| panic!("{name} does not load"));
            assert_eq!(new.chrome, old.chrome, "{name} chrome");
            assert_eq!(new.terminal, old.terminal, "{name} terminal");
            assert_eq!(new.semantic, old.semantic, "{name} semantic");
            assert_eq!(new.tree, old.tree, "{name} tree");
            assert_eq!(new.viewer, old.viewer, "{name} viewer");
            assert_eq!(new.font, old.font, "{name} font");
            assert_eq!(new.effect.as_ref().map(|e| e.name.as_str()), old.effect.as_ref().map(|e| e.name.as_str()), "{name} effect");
            let scene_of = |t: &Theme| {
                let e = t.effect.as_ref()?;
                let m = crate::effects::manifest::load(&e.name, std::path::Path::new("/nonexistent/effects")).ok()?;
                Some(m.manifest.scene_index(e.scene.as_deref()))
            };
            assert_eq!(scene_of(&new), scene_of(&old), "{name} scene");
            assert_eq!(new.kind(), old.kind(), "{name} kind");
            assert_eq!(new.appearance, old.appearance, "{name} appearance");
            assert_eq!(new.title, old.title, "{name} title");
            assert_eq!(new.group, old.group, "{name} group");
            assert_eq!(new.group_default, old.group_default, "{name} group_default");
            assert_eq!(parse(old_text).unwrap().chrome, old.chrome, "{name} still parses in the old form");
        }
    }

    #[test]
    fn every_built_in_preset_is_in_the_new_form() {
        for (name, text) in BUILTIN {
            let table: toml::Table = toml::from_str(text).unwrap();
            assert!(table.contains_key("color"), "{name} names its colour scheme");
            for old in ["chrome", "terminal", "semantic", "tree", "viewer", "appearance"] {
                assert!(!table.contains_key(old), "{name} still carries {old}");
            }
        }
    }

    #[test]
    fn a_preset_in_the_old_all_in_one_form_still_loads() {
        let dir = tempfile::tempdir().unwrap();
        let text = FIXTURES.iter().find(|(n, _)| *n == "lavender-watercolor").unwrap().1.replace("name = \"lavender-watercolor\"", "name = \"mine\"");
        std::fs::write(dir.path().join("mine.toml"), text).unwrap();
        let t = load("mine", dir.path()).unwrap();
        assert_eq!((t.name.as_str(), t.color.as_str()), ("mine", "mine"));
        assert_eq!(t.font.prose.as_deref(), Some("Avenir Next"));
    }

    #[test]
    fn a_preset_naming_a_colour_of_one_s_own_loads_it_from_the_colors_folder() {
        let root = tempfile::tempdir().unwrap();
        let themes = root.path().join("themes");
        let colors = root.path().join("colors");
        std::fs::create_dir_all(&themes).unwrap();
        std::fs::create_dir_all(&colors).unwrap();
        let mine = crate::color_schemes::BUILTIN[0].1.replace("name = \"navy\"", "name = \"mine\"");
        std::fs::write(colors.join("mine.toml"), mine).unwrap();
        std::fs::write(themes.join("dusk.toml"), "name = \"dusk\"\ncolor = \"mine\"\nfont = \"lavender\"\n").unwrap();
        let t = load("dusk", &themes).unwrap();
        assert_eq!((t.name.as_str(), t.color.as_str()), ("dusk", "mine"));
        assert!(crate::color_schemes::builtin("navy").unwrap().same_colors(&t));
        assert_eq!(t.font.prose.as_deref(), Some("Avenir Next"));
        assert_eq!(t.effect, None);
    }

    #[test]
    fn a_preset_naming_a_missing_colour_scheme_is_an_error_naming_it() {
        let root = tempfile::tempdir().unwrap();
        let themes = root.path().join("themes");
        std::fs::create_dir_all(&themes).unwrap();
        std::fs::write(themes.join("dusk.toml"), "name = \"dusk\"\ncolor = \"nowhere\"\n").unwrap();
        let e = load("dusk", &themes).unwrap_err();
        assert!(e.contains("nowhere"), "{e}");
        let e = validate_file(&themes.join("dusk.toml")).unwrap_err();
        assert!(e.contains("nowhere"), "{e}");
        std::fs::write(themes.join("dusk.toml"), "name = \"dusk\"\ncolor = \"navy\"\nfont = \"nofont\"\n").unwrap();
        assert!(load("dusk", &themes).unwrap_err().contains("nofont"));
    }

    #[test]
    fn a_new_form_preset_may_not_set_its_own_appearance() {
        let e = parse("name = \"x\"\ncolor = \"navy\"\nappearance = \"dark\"\n").unwrap_err();
        assert!(e.contains("appearance"), "{e}");
        assert!(parse("name = \"x\"\ncolor = \"navy\"\n[chrome]\nwindow_bg = \"#000000\"\n").is_err());
    }

    #[test]
    fn every_built_in_declares_whether_it_is_dark_or_light_and_its_background_agrees() {
        for (name, text) in BUILTIN {
            let t = parse(text).unwrap();
            let declared = t.appearance.unwrap_or_else(|| panic!("{name} declares no appearance"));
            assert_eq!(declared == Appearance::Light, crate::palette::luminance(t.chrome.window_bg) > 0.5, "{name}");
            assert_eq!(t.kind(), declared, "{name}");
        }
        let dark: Vec<&str> = BUILTIN.iter().filter(|(n, _)| builtin(n).unwrap().kind() == Appearance::Dark).map(|(n, _)| *n).collect();
        assert_eq!(dark, ["navy", "timo-purple", "matrix", "tokyo-rain", "material-ocean", "material-palenight", "material-sublime"]);
    }

    #[test]
    fn a_theme_declaring_the_wrong_appearance_is_refused_and_one_declaring_none_is_classed_by_its_background() {
        let navy = FIXTURES[0].1;
        assert!(navy.contains("appearance = \"dark\"\n"));
        let e = parse(&navy.replace("appearance = \"dark\"", "appearance = \"light\"")).unwrap_err();
        assert_eq!(e, "appearance = \"light\" but window_bg #16202f is dark; make one of them match the other");
        assert!(parse(&navy.replace("appearance = \"dark\"", "appearance = \"dim\"")).is_err());
        let none = parse(&navy.replace("appearance = \"dark\"\n", "")).unwrap();
        assert_eq!(none.appearance, None);
        assert_eq!(none.kind(), Appearance::Dark);
        let peach = parse(&FIXTURES[2].1.replace("appearance = \"light\"\n", "")).unwrap();
        assert_eq!(peach.kind(), Appearance::Light);
    }

    #[test]
    fn the_kinds_list_carries_each_theme_s_appearance_and_none_for_a_broken_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mine.toml"), FIXTURES[2].1.replace("name = \"peach-watercolor\"", "name = \"mine\"")).unwrap();
        std::fs::write(dir.path().join("broken.toml"), "name = \"broken\"\n").unwrap();
        let k = kinds(dir.path());
        let find = |n: &str| k.iter().find(|(name, _, _)| name == n).map(|(_, _, a)| *a).unwrap();
        assert_eq!(find("navy"), Some(Appearance::Dark));
        assert_eq!(find("lavender-watercolor"), Some(Appearance::Light));
        assert_eq!(find("mine"), Some(Appearance::Light));
        assert_eq!(find("broken"), None);
        assert_eq!(k.len(), list(dir.path()).len());
    }

    #[test]
    fn every_text_colour_of_matrix_is_bright_enough_for_the_glyph_shader_s_emissive_branch() {
        let m = builtin("matrix").unwrap();
        let (c, t, s, tr, v) = (&m.chrome, &m.terminal, &m.semantic, &m.tree, &m.viewer);
        let text = [c.text, c.text_dim, c.focus_ring, t.foreground, t.cursor,
            t.red, t.green, t.yellow, t.blue, t.magenta, t.cyan, t.white,
            t.bright_black, t.bright_red, t.bright_green, t.bright_yellow, t.bright_blue, t.bright_magenta, t.bright_cyan, t.bright_white,
            s.working, s.waiting, s.bell, tr.added, tr.modified, tr.deleted, tr.untracked, tr.ignored,
            v.keyword, v.string, v.comment, v.number, v.function, v.type_name, v.constant, v.operator, v.punctuation];
        for x in text {
            assert!(luminance(x) >= 0.47, "{} is {:.2}, under 0.47, too near the shader's 0.45", x.hex(), luminance(x));
        }
        assert!((luminance(Rgb { r: 255, g: 255, b: 255 }) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn every_contrast_pair_of_every_builtin_clears_what_it_needs() {
        for (name, _) in BUILTIN {
            let t = builtin(name).unwrap();
            for p in contrast_pairs(&t) {
                let c = contrast(p.fg, p.bg);
                assert!(c >= p.needs, "{name}: {} ({} on {}) is {c:.2}:1, needs {}", p.what, p.fg.hex(), p.bg.hex(), p.needs);
            }
        }
    }

    #[test]
    fn the_outline_rail_s_marks_and_labels_clear_their_floors_on_every_builtin() {
        // The stepper's line is drawn fainter and is not checked: it joins the dots and carries
        // nothing of its own. A hollow dot's ring is full `text_dim` so it holds 3:1.
        use crate::outline_rail::{HOVER_ALPHA, PILL_ALPHA};
        let over = |fg: Rgb, bg: Rgb, a: f32| crate::contrast::Drawn { color: fg, alpha: a, moved: false }.over(bg);
        let mut fails = Vec::new();
        for (name, _) in BUILTIN {
            let c = builtin(name).unwrap().chrome;
            let (pill, tint) = (over(c.focus_ring, c.window_bg, PILL_ALPHA), over(c.focus_ring, c.window_bg, HOVER_ALPHA));
            let pairs = [
                ("the current dot (chrome.focus_ring) on window_bg", contrast(c.focus_ring, c.window_bg), 3.0),
                ("a read dot or a hollow dot's ring (chrome.text_dim) on window_bg", contrast(c.text_dim, c.window_bg), 3.0),
                ("the current row's label (chrome.text) on its pill (focus_ring faint on window_bg)", contrast(c.text, pill), 4.5),
                ("the current dot (chrome.focus_ring) on its pill", contrast(c.focus_ring, pill), 3.0),
                ("a lit row's label (chrome.text_dim) on its tint", contrast(c.text_dim, tint), 4.5),
                ("a dot (chrome.text_dim) on a lit row's tint", contrast(c.text_dim, tint), 3.0),
            ];
            for (what, ratio, needs) in pairs {
                if ratio < needs {
                    fails.push(format!("{name}: {what} is {ratio:.2}:1, needs {needs}"));
                }
            }
        }
        assert!(fails.is_empty(), "{}", fails.join("\n"));
    }

    #[test]
    fn the_schema_names_the_surfaces_the_contrast_check_uses() {
        let doc = |key: &str| {
            let (table, key) = key.split_once('.').unwrap();
            schema().iter().find(|t| t.name == table).and_then(|t| t.keys.iter().find(|k| k.key == key)).unwrap_or_else(|| panic!("{table}.{key}"))
        };
        let pairs = contrast_pairs(&builtin("lavender-watercolor").unwrap());
        for p in pairs.iter().filter(|p| !p.what.starts_with("ansi ")) {
            let (key, on) = p.what.split_once(" on ").unwrap();
            let k = doc(key);
            assert!(k.contrast_with.unwrap().split(", ").any(|s| s == on), "{}", p.what);
            assert!(k.needs.unwrap() >= p.needs, "{}", p.what);
        }
        for t in schema() {
            for k in t.keys {
                for on in k.contrast_with.into_iter().flat_map(|c| c.split(", ")) {
                    let label = format!("{}.{} on {on}", t.name, k.key);
                    let ansi = t.name == "terminal" && on == "terminal.background" && k.key != "foreground";
                    assert!(ansi || pairs.iter().any(|p| p.what == label), "{label} is not checked");
                }
                assert_eq!(k.contrast_with.is_some(), k.needs.is_some(), "{}", k.key);
            }
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
        // The built-in Navy keeps milestone 1's ANSI colours, but for the bright black and white,
        // lifted to read on Material Navy's lighter cards.
        let mut kept = crate::palette::NAVY.ansi;
        (kept[8], kept[15]) = (rgb(0x8590a2), rgb(0xeef0f4));
        assert_eq!(p.ansi, kept);
    }

    #[test]
    fn a_theme_file_must_be_complete_and_known() {
        let e = parse("name = \"x\"\n").unwrap_err();
        assert!(e.contains("chrome"), "{e}");
        let text = FIXTURES[0].1.replace("window_bg", "window_background");
        let e = parse(&text).unwrap_err();
        assert!(e.contains("window_background") || e.contains("window_bg"), "{e}");
        let bg = builtin("navy").unwrap().chrome.window_bg.hex();
        let text = FIXTURES[0].1.replace(&bg, &bg[..6]);
        assert!(parse(&text).is_err());
    }

    #[test]
    fn user_files_win_and_names_are_bare_stems() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load("navy", dir.path()).unwrap(), builtin("navy").unwrap(), "a missing folder falls back to the built-in");
        let mut text = FIXTURES[0].1.to_string();
        let bg = builtin("navy").unwrap().chrome.window_bg.hex();
        text = text.replace(&format!("window_bg = \"{bg}\""), "window_bg = \"#000000\"");
        std::fs::write(dir.path().join("navy.toml"), &text).unwrap();
        assert_eq!(load("navy", dir.path()).unwrap().chrome.window_bg, rgb(0x000000));
        std::fs::write(dir.path().join("mine.toml"), FIXTURES[1].1.replace("name = \"timo-purple\"", "name = \"mine\"")).unwrap();
        assert_eq!(load("mine", dir.path()).unwrap().name, "mine");
        assert_eq!(names(dir.path()), vec!["arabia-solar".to_string(), "composition-notebook".to_string(), "lavender-material".to_string(), "lavender-watercolor".to_string(), "material".to_string(), "material-ocean".to_string(), "material-palenight".to_string(), "material-solar".to_string(), "material-sublime".to_string(), "matrix".to_string(), "mine".to_string(), "navy".to_string(), "peach-watercolor".to_string(), "timo-purple".to_string(), "tokyo-rain".to_string()]);
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
        assert_eq!(wrote.len(), 14);
        assert_eq!(std::fs::read_to_string(sub.join("navy.toml")).unwrap(), BUILTIN[0].1);
        std::fs::write(sub.join("navy.toml"), "edited").unwrap();
        assert!(extract(&sub).unwrap().is_empty(), "nothing to write the second time");
        assert_eq!(std::fs::read_to_string(sub.join("navy.toml")).unwrap(), "edited");
    }

    #[test]
    fn material_is_a_light_built_in_in_white_cards_on_a_cool_frame_with_a_blue_accent() {
        let material = builtin("material").expect("material is built in");
        assert!(material.is_light());
        assert_eq!(material.recommended_effect(), Some("material"));
        let font = &material.font;
        assert_eq!((font.ui.as_deref(), font.prose.as_deref(), font.terminal.as_deref()), (Some("DM Sans"), Some("DM Sans"), Some("Roboto Mono")));
        let c = &material.chrome;
        assert_eq!(c.window_bg, Rgb { r: 0xff, g: 0xff, b: 0xff }, "the cards are white");
        assert_eq!(material.terminal.background, c.window_bg, "a terminal's page is its card");
        assert_eq!(c.sidebar, Rgb { r: 0xf0, g: 0xf4, b: 0xf9 }, "the frame, a cool pale grey");
        assert_eq!((c.title_bar, c.status_bar), (c.sidebar, c.sidebar), "the bars are the frame");
        assert_eq!(c.focus_ring, Rgb { r: 0x0b, g: 0x57, b: 0xd0 }, "the accent is a deep blue");
    }

    #[test]
    fn composition_notebook_is_a_light_built_in_that_recommends_its_own_effect() {
        let notebook = builtin("composition-notebook").expect("composition-notebook is built in");
        assert!(notebook.is_light());
        assert_eq!(notebook.recommended_effect(), Some("notebook"));
        let font = &notebook.font;
        assert_eq!((font.ui.as_deref(), font.prose.as_deref(), font.terminal.as_deref()), (Some("Chalkboard SE"), Some("Chalkboard SE"), Some("Monaspace Radon")));
        assert_eq!(notebook.chrome.focus_ring, notebook.terminal.cursor, "the margin's red is the accent and the cursor");
    }

    #[test]
    fn material_solar_is_composition_notebook_s_colours_in_material_s_dress() {
        let (m, n) = (builtin("material-solar").unwrap(), builtin("composition-notebook").unwrap());
        assert_eq!((&m.chrome, &m.terminal, &m.semantic, &m.tree, &m.viewer), (&n.chrome, &n.terminal, &n.semantic, &n.tree, &n.viewer), "every colour is the notebook's");
        assert_eq!(m.recommended_effect(), Some("material"));
        assert_eq!(m.font, builtin("material").unwrap().font, "Material's fonts");
        assert_eq!((m.title.as_deref(), m.group.as_deref(), m.group_default), (Some("Material Solar"), Some("Material"), true));
    }

    #[test]
    fn the_built_ins_read_as_cyrus_named_them_and_the_two_groups_each_have_one_default() {
        let title = |n: &str| builtin(n).unwrap().title.unwrap_or_else(|| title_case(n));
        assert_eq!(title("navy"), "Material Navy");
        assert_eq!((title("material-ocean"), title("material-palenight")), ("Material Ocean".into(), "Material Palenight".into()));
        assert_eq!(title("matrix"), "The Matrix");
        assert_eq!(title("lavender-watercolor"), "Watercolor Lavender");
        assert_eq!(title("peach-watercolor"), "Watercolor Peach");
        assert_eq!(title("material"), "Material Bright");
        assert_eq!(title("lavender-material"), "Material Lavender");
        assert_eq!(title("tokyo-rain"), "Tokyo Rain", "the rest in capitalised words");
        // A group is one row in each list it has members in, so it has one default a list: the
        // dark Material row's is Material Navy, the light one's Material Solar.
        for (group, kind, default) in [("Watercolor", Appearance::Light, "lavender-watercolor"), ("Material", Appearance::Light, "material-solar"), ("Material", Appearance::Dark, "navy")] {
            let defaults: Vec<&str> = BUILTIN
                .iter()
                .map(|(n, _)| *n)
                .filter(|n| builtin(n).unwrap().group.as_deref() == Some(group) && builtin(n).unwrap().group_default && builtin(n).unwrap().kind() == kind)
                .collect();
            assert_eq!(defaults, [default], "{group} {kind:?}");
        }
        let dark_material: Vec<&str> = BUILTIN.iter().map(|(n, _)| *n).filter(|n| builtin(n).unwrap().group.as_deref() == Some("Material") && !builtin(n).unwrap().is_light()).collect();
        assert_eq!(dark_material, ["navy", "material-ocean", "material-palenight", "material-sublime"]);
        assert_eq!(title("material-sublime"), "Material Sublime");
    }

    #[test]
    fn timo_purple_recommends_the_overworld_effect() {
        assert_eq!(builtin("timo-purple").unwrap().recommended_effect(), Some("overworld"));
    }

    #[test]
    fn arabia_solar_is_peach_watercolor_s_colours_by_day_under_timo_purple_s_effect_and_font() {
        let (solar, peach, timo) = (builtin("arabia-solar").expect("ships"), builtin("peach-watercolor").unwrap(), builtin("timo-purple").unwrap());
        assert_eq!((solar.name.as_str(), solar.title.as_deref()), ("arabia-solar", Some("Solar Arabia")));
        assert!(solar.is_light());
        assert_eq!((&solar.chrome, &solar.terminal, &solar.semantic, &solar.tree, &solar.viewer), (&peach.chrome, &peach.terminal, &peach.semantic, &peach.tree, &peach.viewer));
        assert_eq!(solar.recommended_effect(), Some("overworld"));
        assert_eq!(solar.font, timo.font, "the same pixel face as Timo Purple");
    }

    #[test]
    fn timo_purple_is_built_in_and_its_accent_is_the_cyan() {
        // Its other surfaces (banner, sidebar, inactive tabs, selection, find) are checked for
        // every built-in through `contrast_pairs`.
        let t = builtin("timo-purple").expect("ships");
        assert_eq!(t.name, "timo-purple");
        assert_eq!(BUILTIN.len(), 14);
        assert_eq!(t.chrome.focus_ring, t.terminal.cyan, "the accent is the cyan");
        assert_eq!(t.terminal.cursor, t.terminal.cyan);
    }

    #[test]
    fn the_list_and_the_validator_name_every_theme() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mine.toml"), BUILTIN[0].1.replace("name = \"navy\"", "name = \"mine\"")).unwrap();
        std::fs::write(dir.path().join("broken.toml"), "name = \"broken\"\n[chrome]\nwindow_bg = 1\n").unwrap();
        std::fs::write(dir.path().join("navy.toml"), BUILTIN[0].1).unwrap();
        let l = list(dir.path());
        assert_eq!(l[0], ("navy".into(), Source::File), "a file shadows the built-in and is listed once");
        assert_eq!(l[1], ("timo-purple".into(), Source::BuiltIn));
        assert_eq!(l[2], ("peach-watercolor".into(), Source::BuiltIn));
        assert_eq!(l[3], ("lavender-watercolor".into(), Source::BuiltIn));
        assert_eq!(l[4], ("lavender-material".into(), Source::BuiltIn));
        assert_eq!(l[5], ("matrix".into(), Source::BuiltIn));
        assert_eq!(l[6], ("tokyo-rain".into(), Source::BuiltIn));
        assert_eq!(l[7], ("composition-notebook".into(), Source::BuiltIn));
        assert_eq!(l[8], ("material".into(), Source::BuiltIn));
        assert_eq!(l[9], ("arabia-solar".into(), Source::BuiltIn));
        assert_eq!(l[10], ("material-solar".into(), Source::BuiltIn));
        assert_eq!(l[11], ("material-ocean".into(), Source::BuiltIn));
        assert_eq!(l[12], ("material-palenight".into(), Source::BuiltIn));
        assert_eq!(l[13], ("material-sublime".into(), Source::BuiltIn));
        assert!(matches!(&l[14], (n, Source::Invalid(e)) if n == "broken" && !e.is_empty()));
        assert_eq!(l[15], ("mine".into(), Source::File));
        assert_eq!(validate_file(&dir.path().join("mine.toml")), Ok("mine".into()));
        assert!(validate_file(&dir.path().join("broken.toml")).is_err());
        assert!(validate_file(&dir.path().join("none.toml")).is_err());
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(theme_cli_to(&["list".into()], dir.path(), &mut out, &mut err), 0);
        let lines: Vec<&str> = std::str::from_utf8(&out).unwrap().lines().collect();
        assert_eq!(lines[0], "navy (file, dark)");
        assert_eq!(lines[1], "timo-purple (built in, dark)");
        assert_eq!(lines[2], "peach-watercolor (built in, light)");
        assert_eq!(lines[3], "lavender-watercolor (built in, light)");
        assert_eq!(lines[4], "lavender-material (built in, light)");
        assert_eq!(lines[5], "matrix (built in, dark)");
        assert_eq!(lines[6], "tokyo-rain (built in, dark)");
        assert_eq!(lines[7], "composition-notebook (built in, light)");
        assert_eq!(lines[8], "material (built in, light)");
        assert_eq!(lines[9], "arabia-solar (built in, light)");
        assert_eq!(lines[10], "material-solar (built in, light)");
        assert_eq!(lines[11], "material-ocean (built in, dark)");
        assert_eq!(lines[12], "material-palenight (built in, dark)");
        assert_eq!(lines[13], "material-sublime (built in, dark)");
        assert!(lines[14].starts_with("broken (file, invalid: "));
        assert_eq!(lines[15], "mine (file, dark)");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(theme_cli_to(&["validate".into(), dir.path().join("mine.toml").display().to_string()], dir.path(), &mut out, &mut err), 0);
        assert_eq!(std::str::from_utf8(&out).unwrap(), "ok: mine\n");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(theme_cli_to(&["validate".into(), dir.path().join("broken.toml").display().to_string()], dir.path(), &mut out, &mut err), 1);
        assert!(!err.is_empty());
        assert_eq!(theme_cli_to(&["frob".into()], dir.path(), &mut Vec::new(), &mut Vec::new()), 2);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(theme_cli_to(&["--json".into(), "list".into()], dir.path(), &mut out, &mut err), 0);
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v[0], serde_json::json!({ "name": "navy", "source": "file", "appearance": "dark", "path": dir.path().join("navy.toml").display().to_string() }));
        assert_eq!(v[1], serde_json::json!({ "name": "timo-purple", "source": "built-in", "appearance": "dark" }));
        assert_eq!(v[2]["appearance"], "light");
        assert_eq!((v[7]["name"].as_str(), v[8]["name"].as_str(), v[8]["appearance"].as_str()), (Some("composition-notebook"), Some("material"), Some("light")));
        assert_eq!((v[9]["name"].as_str(), v[9]["appearance"].as_str()), (Some("arabia-solar"), Some("light")));
        assert_eq!((v[10]["name"].as_str(), v[10]["appearance"].as_str()), (Some("material-solar"), Some("light")));
        assert_eq!((v[11]["name"].as_str(), v[12]["name"].as_str()), (Some("material-ocean"), Some("material-palenight")));
        assert_eq!(v[13]["name"].as_str(), Some("material-sublime"));
        assert_eq!((v[14]["source"].as_str(), v[14]["error"].is_string(), v[14].get("appearance")), (Some("invalid"), true, None));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(theme_cli_to(&["validate".into(), dir.path().join("broken.toml").display().to_string(), "--json".into()], dir.path(), &mut out, &mut err), 1);
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!((v["ok"].as_bool(), v["error"].is_string(), err.is_empty()), (Some(false), true, true));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(theme_cli_to(&["validate".into(), dir.path().join("mine.toml").display().to_string(), "--json".into()], dir.path(), &mut out, &mut err), 0);
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&out).unwrap(), serde_json::json!({ "ok": true, "name": "mine" }));
    }

    #[test]
    fn import_uses_the_theme_s_own_name_and_asks_before_replacing() {
        let src_dir = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("a").join("themes"); // created on demand
        let src = src_dir.path().join("whatever.toml");
        std::fs::write(&src, BUILTIN[0].1.replace("name = \"navy\"", "name = \"ocean\"")).unwrap();
        assert_eq!(import_file(&src, &dir, false), Ok(("ocean".into(), false)));
        assert!(dir.join("ocean.toml").is_file());
        assert_eq!(import_file(&src, &dir, false), Err("exists".into()));
        assert_eq!(import_file(&src, &dir, true), Ok(("ocean".into(), true)));
        // A theme named like a built-in would shadow it, so that asks too.
        for builtin in ["navy", "peach-watercolor", "timo-purple"] {
            std::fs::write(&src, BUILTIN[0].1.replace("name = \"navy\"", &format!("name = \"{builtin}\""))).unwrap();
            assert_eq!(import_file(&src, &dir, false), Err("exists".into()), "{builtin}");
            assert!(!dir.join(format!("{builtin}.toml")).exists(), "nothing written before the answer");
            assert_eq!(import_file(&src, &dir, true), Ok((builtin.to_string(), false)), "{builtin}");
        }
        std::fs::write(&src, BUILTIN[0].1.replace("name = \"navy\"", "name = \"../../x\"")).unwrap();
        assert!(import_file(&src, &dir, true).is_err(), "a name that leaves the folder");
        assert!(!dir.join("x.toml").exists() && !dir.parent().unwrap().join("x.toml").exists());
        assert!(!root.path().join("x.toml").exists(), "nothing lands two levels up");
        std::fs::write(&src, BUILTIN[0].1.replace("name = \"navy\"", "name = \".hidden\"")).unwrap();
        assert!(import_file(&src, &dir, true).is_err());
        std::fs::write(&src, "nope").unwrap();
        assert!(!import_file(&src, &dir, true).unwrap_err().is_empty());
        assert!(import_file(&src_dir.path().join("x.txt"), &dir, true).is_err(), "only .toml");
    }

    #[test]
    fn appearance_mode_picks_the_theme() {
        use crate::config::{AppearanceMode, ThemeConfig};
        let cfg = ThemeConfig { dark: "navy".into(), light: "peach-watercolor".into() };
        assert_eq!(pick_name(AppearanceMode::System, true, &cfg), "navy");
        assert_eq!(pick_name(AppearanceMode::System, false, &cfg), "peach-watercolor");
        assert_eq!(pick_name(AppearanceMode::Dark, false, &cfg), "navy");
        assert_eq!(pick_name(AppearanceMode::Light, true, &cfg), "peach-watercolor");
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
        let light = builtin("peach-watercolor").unwrap();
        assert_eq!(keep_or_switch(&navy, Ok(light.clone())), (Some(light), None));
    }

    #[test]
    fn a_blend_walks_every_colour_from_one_theme_to_the_other() {
        let from = builtin("navy").unwrap();
        let mut to = builtin("peach-watercolor").unwrap();
        to.font.ui = Some("Avenir".into());
        let start = blend(&from, &to, 0.0);
        assert_eq!(start.chrome, from.chrome);
        assert_eq!(start.terminal, from.terminal);
        assert_eq!(start.semantic, from.semantic);
        assert_eq!(start.tree, from.tree);
        assert_eq!(start.viewer, from.viewer);
        assert_eq!((start.name.as_str(), &start.font), ("peach-watercolor", &to.font), "the name and fonts are the target's from the first frame");
        let end = blend(&from, &to, 1.0);
        assert_eq!(end, to);
        assert_eq!(blend(&from, &to, -3.0).chrome, from.chrome, "t is clamped below");
        assert_eq!(blend(&from, &to, 7.0), to, "and above");
        // Every channel sits at its midpoint at t = 0.5, rounded to the nearest step.
        let mid = |a: u8, b: u8| ((f32::from(a) + f32::from(b)) / 2.0).round() as u8;
        let half = blend(&from, &to, 0.5);
        let (a, b, h) = (from.chrome.window_bg, to.chrome.window_bg, half.chrome.window_bg);
        assert_eq!(h, Rgb { r: mid(a.r, b.r), g: mid(a.g, b.g), b: mid(a.b, b.b) });
        let (a, b, h) = (from.terminal.bright_cyan, to.terminal.bright_cyan, half.terminal.bright_cyan);
        assert_eq!(h, Rgb { r: mid(a.r, b.r), g: mid(a.g, b.g), b: mid(a.b, b.b) });
        let (a, b, h) = (from.viewer.punctuation, to.viewer.punctuation, half.viewer.punctuation);
        assert_eq!(h, Rgb { r: mid(a.r, b.r), g: mid(a.g, b.g), b: mid(a.b, b.b) });
        // A single step: 0 to 255 at a quarter is 64, the nearest integer to 63.75.
        let mut black = from.clone();
        black.semantic.bell = rgb(0x000000);
        let mut white = from.clone();
        white.semantic.bell = rgb(0xffffff);
        assert_eq!(blend(&black, &white, 0.25).semantic.bell, rgb(0x404040));
    }

    /// A temp root with `themes/` and `colors/` and `fonts/` beside it.
    fn roots() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let themes = root.path().join("themes");
        for d in ["themes", "colors", "fonts"] {
            std::fs::create_dir_all(root.path().join(d)).unwrap();
        }
        (root, themes)
    }

    fn fixture(name: &str) -> &'static str {
        FIXTURES.iter().find(|(n, _)| *n == name).unwrap().1
    }

    fn assert_one_row_a_name(rows: &[ColorEntry]) {
        let mut names: Vec<&str> = rows.iter().map(|e| e.name.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), rows.len(), "no two rows share a value: {rows:?}");
    }

    #[test]
    fn an_old_form_theme_with_no_appearance_is_listed_by_its_background() {
        let (root, themes) = roots();
        let text = fixture("navy").replace("name = \"navy\"", "name = \"mine\"").replace("#16202f", "#16202e").replace("appearance = \"dark\"\n", "");
        assert!(!text.contains("appearance"), "the fixture says no appearance");
        std::fs::write(themes.join("mine.toml"), text).unwrap();
        let plain = crate::color_schemes::BUILTIN[0].1.replace("name = \"navy\"", "name = \"plain\"").replace("appearance = \"dark\"\n", "").replace("#16202f", "#16202d");
        std::fs::write(root.path().join("colors").join("plain.toml"), plain).unwrap();
        let rows = colors(&themes);
        let of = |n: &str| rows.iter().find(|e| e.name == n).unwrap_or_else(|| panic!("{n} is listed: {rows:?}")).appearance;
        assert_eq!(of("mine"), Some(Appearance::Dark));
        assert_eq!(of("plain"), Some(Appearance::Dark));
    }

    #[test]
    fn a_theme_file_with_a_built_in_scheme_s_name_and_other_colours_replaces_its_row() {
        let (_root, themes) = roots();
        let text = fixture("navy").replace("title = \"Material Navy\"", "title = \"Mine Navy\"").replace("#16202f", "#16202e");
        std::fs::write(themes.join("navy.toml"), text).unwrap();
        let rows = colors(&themes);
        assert_one_row_a_name(&rows);
        assert_eq!(rows.len(), 11);
        let navy = rows.iter().find(|e| e.name == "navy").unwrap();
        assert_eq!((navy.title.as_str(), &navy.source), ("Mine Navy", &Source::File));
        assert_eq!(load_color("navy", &themes).unwrap().chrome, parse(&std::fs::read_to_string(themes.join("navy.toml")).unwrap()).unwrap().chrome);
    }

    #[test]
    fn a_theme_file_with_a_built_in_scheme_s_name_and_the_same_colours_leaves_its_row() {
        let (_root, themes) = roots();
        std::fs::write(themes.join("navy.toml"), fixture("navy")).unwrap();
        let rows = colors(&themes);
        assert_one_row_a_name(&rows);
        let navy = rows.iter().find(|e| e.name == "navy").unwrap();
        assert_eq!((navy.title.as_str(), &navy.source), ("Navy", &Source::BuiltIn));
    }

    #[test]
    fn an_old_form_theme_is_listed_by_its_file_name_not_its_name_field() {
        let (_root, themes) = roots();
        let text = fixture("navy").replace("name = \"navy\"", "name = \"other\"").replace("#16202f", "#16202e");
        std::fs::write(themes.join("mine.toml"), text).unwrap();
        let rows = colors(&themes);
        assert_one_row_a_name(&rows);
        assert!(rows.iter().any(|e| e.name == "mine") && !rows.iter().any(|e| e.name == "other"), "{rows:?}");
        assert!(load_color("mine", &themes).is_ok());
    }

    #[test]
    fn a_custom_colour_name_is_a_colour_file_then_a_theme_file_then_a_built_in_scheme_then_a_preset() {
        let (root, themes) = roots();
        let navy = |hex: &str| fixture("navy").replace("#16202f", hex);
        std::fs::write(themes.join("navy.toml"), navy("#16202e")).unwrap();
        let mine = crate::color_schemes::BUILTIN[0].1.replace(&crate::color_schemes::builtin("navy").unwrap().chrome.border.hex(), "#123456");
        std::fs::write(root.path().join("colors").join("navy.toml"), mine).unwrap();
        assert_eq!(load_color("navy", &themes).unwrap().chrome.border.hex(), "#123456", "the colour file first");
        std::fs::remove_file(root.path().join("colors").join("navy.toml")).unwrap();
        assert_eq!(load_color("navy", &themes).unwrap().chrome.window_bg.hex(), "#16202e", "then the theme file");
        std::fs::remove_file(themes.join("navy.toml")).unwrap();
        assert!(crate::color_schemes::builtin("navy").unwrap().same_colors(&load_color("navy", &themes).unwrap()), "then the built-in scheme");
        // A preset of the name, as a 0.8.54 config names one, keeps its fonts.
        assert_eq!(load_color("lavender-watercolor", &themes).unwrap().font.prose.as_deref(), Some("Avenir Next"));
        assert_eq!(load_color("matrix", &themes).unwrap().font, builtin("matrix").unwrap().font, "a name that is a scheme and a preset keeps the preset's fonts");
        assert!(load_color("nowhere", &themes).is_err());
    }

    #[test]
    fn the_font_of_a_name_is_a_font_file_then_a_theme_file_then_a_built_in_set_then_a_preset() {
        let (root, themes) = roots();
        let navy = builtin("navy").unwrap();
        // An edited old-form copy of a preset keeps the user's font.
        let edited = fixture("material").replace("prose = \"DM Sans\"", "prose = \"Georgia\"");
        assert_ne!(edited, fixture("material"));
        std::fs::write(themes.join("material.toml"), edited).unwrap();
        assert_eq!(font_for(Some("material"), &navy, &themes).unwrap().prose.as_deref(), Some("Georgia"));
        std::fs::write(root.path().join("fonts").join("material.toml"), "name = \"material\"\nprose = \"Menlo\"\n").unwrap();
        assert_eq!(font_for(Some("material"), &navy, &themes).unwrap().prose.as_deref(), Some("Menlo"), "the font file first");
        std::fs::remove_file(root.path().join("fonts").join("material.toml")).unwrap();
        std::fs::remove_file(themes.join("material.toml")).unwrap();
        assert_eq!(font_for(Some("material"), &navy, &themes).unwrap().prose.as_deref(), Some("DM Sans"), "then the preset");
        assert!(font_for(Some("nowhere"), &navy, &themes).is_err());
    }

    #[test]
    fn a_colour_file_that_does_not_parse_says_why_with_its_path() {
        let (root, themes) = roots();
        let file = root.path().join("colors").join("broken.toml");
        std::fs::write(&file, "name = \"broken\"\nbogus = 1\n").unwrap();
        for e in [load("broken", &themes).unwrap_err(), load_color("broken", &themes).unwrap_err()] {
            assert!(e.contains(&file.display().to_string()) && e.contains("bogus"), "{e}");
        }
    }

    #[test]
    fn the_colour_row_of_an_old_form_copy_is_the_scheme_with_its_colours() {
        let (_root, themes) = roots();
        for (name, text) in FIXTURES {
            std::fs::write(themes.join(format!("{name}.toml")), text).unwrap();
        }
        let rows = colors(&themes);
        assert_eq!(current_color("lavender-watercolor", &rows, &themes), "lavender");
        assert_eq!(current_color("peach", &rows, &themes), "peach");
        assert_eq!(current_color("gone", &rows, &themes), "gone");
    }

    #[test]
    fn a_preset_naming_a_colour_of_one_s_own_is_accepted_wherever_a_preset_file_is_read() {
        let (root, themes) = roots();
        let mine = crate::color_schemes::BUILTIN[0].1.replace("name = \"navy\"", "name = \"mine\"");
        std::fs::write(root.path().join("colors").join("mine.toml"), mine).unwrap();
        let src = root.path().join("dusk.toml");
        std::fs::write(&src, "name = \"dusk\"\ncolor = \"mine\"\n").unwrap();
        assert_eq!(import_file(&src, &themes, false), Ok(("dusk".to_string(), false)));
    }
}
