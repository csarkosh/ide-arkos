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
    /// Optional: the fonts a look asks for. Absent in files from before 0.3.3.
    #[serde(default, skip_serializing_if = "ThemeFont::is_default")]
    pub font: ThemeFont,
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
}

impl ThemeFont {
    fn is_default(&self) -> bool {
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
        let fonts = ThemeFont { prose: keep(&self.prose), ui: keep(&self.ui), terminal: keep(&self.terminal) };
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
pub const BUILTIN: [(&str, &str); 5] = [
    ("navy", include_str!("../themes/navy.toml")),
    ("navy-light", include_str!("../themes/navy-light.toml")),
    ("timo-purple", include_str!("../themes/timo-purple.toml")),
    ("peach-light", include_str!("../themes/peach-light.toml")),
    ("lavender-light", include_str!("../themes/lavender-light.toml")),
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
pub(crate) fn is_bare_name(name: &str) -> bool {
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

/// Parses the theme file at `path`. Ok is the theme's own `name`; a missing or unreadable file is
/// an error like any other.
pub fn validate_file(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    parse(&text).map(|t| t.name)
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

/// Copies the theme file `src` into `dir` under the name the theme itself declares. Only `.toml`
/// files are taken. Ok is the name and whether a file of that name was already there; without
/// `replace` an existing file is left alone and the error is "exists", as it is for a built-in's
/// name, which the copy would shadow.
pub fn import_file(src: &Path, dir: &Path, replace: bool) -> Result<(String, bool), String> {
    if src.extension().and_then(|e| e.to_str()) != Some("toml") {
        return Err("only .toml theme files can be added".into());
    }
    let text = std::fs::read_to_string(src).map_err(|e| format!("cannot read {}: {e}", src.display()))?;
    let name = parse(&text)?.name;
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
            let rows: Vec<serde_json::Value> = list(dir)
                .into_iter()
                .map(|(name, source)| {
                    let path = dir.join(format!("{name}.toml")).display().to_string();
                    match source {
                        Source::BuiltIn => serde_json::json!({ "name": name, "source": "built-in" }),
                        Source::File => serde_json::json!({ "name": name, "source": "file", "path": path }),
                        Source::Invalid(e) => serde_json::json!({ "name": name, "source": "invalid", "error": e, "path": path }),
                    }
                })
                .collect();
            let _ = writeln!(out, "{}", serde_json::Value::Array(rows));
            0
        }
        ["list"] => {
            for (name, source) in list(dir) {
                let _ = match source {
                    Source::BuiltIn => writeln!(out, "{name} (built in)"),
                    Source::File => writeln!(out, "{name} (file)"),
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
    TableDoc { name: "", keys: &[surface("name", "The theme's own name, a bare file stem; stored as <name>.toml and named in [theme] dark or light")] },
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
        }
    }

    /// A light theme has a bright window background; black is then a text colour.
    pub fn is_light(&self) -> bool {
        luminance(self.chrome.window_bg) > 0.5
    }
}

/// One colour `t` of the way from `a` to `b`, each channel rounded to the nearest step.
fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
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

    #[test]
    fn a_theme_may_name_a_prose_font() {
        let t = builtin("peach-light").unwrap();
        assert_eq!(t.font.prose.as_deref(), Some("Baskerville"));
        assert_eq!(builtin("navy").unwrap().font, ThemeFont::default(), "the others leave it to the system font");
        let text = BUILTIN[0].1.to_string() + "\n[font]\nprose = \"Georgia\"\n";
        assert_eq!(parse(&text).unwrap().font.prose.as_deref(), Some("Georgia"));
        assert_eq!(t.font.ui.as_deref(), Some("Optima"));
        assert_eq!(t.font.terminal.as_deref(), Some("PT Mono"));
        assert!(parse(&(BUILTIN[0].1.to_string() + "\n[font]\nchrome = \"Menlo\"\n")).is_err(), "only prose, ui and terminal are fonts a theme may name");
    }

    #[test]
    fn a_font_that_is_not_installed_is_left_out() {
        let fonts = ThemeFont { prose: Some("Baskerville".into()), ui: Some("Optima".into()), terminal: Some("PT Mono".into()) };
        let have: std::collections::HashSet<String> = ["Baskerville", "Menlo"].iter().map(|s| s.to_string()).collect();
        let (kept, missing) = fonts.installed(&have);
        assert_eq!(kept, ThemeFont { prose: Some("Baskerville".into()), ui: None, terminal: None });
        assert_eq!(missing, vec!["Optima".to_string(), "PT Mono".to_string()]);
        assert_eq!(ThemeFont::default().installed(&have), (ThemeFont::default(), vec![]));
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
        assert!(builtin("peach-light").unwrap().is_light());
        assert!(builtin("lavender-light").unwrap().is_light());
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
    fn the_schema_names_the_surfaces_the_contrast_check_uses() {
        let doc = |key: &str| {
            let (table, key) = key.split_once('.').unwrap();
            schema().iter().find(|t| t.name == table).and_then(|t| t.keys.iter().find(|k| k.key == key)).unwrap_or_else(|| panic!("{table}.{key}"))
        };
        let pairs = contrast_pairs(&builtin("navy-light").unwrap());
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
        assert_eq!(names(dir.path()), vec!["lavender-light".to_string(), "mine".to_string(), "navy".to_string(), "navy-light".to_string(), "peach-light".to_string(), "timo-purple".to_string()]);
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
        assert_eq!(wrote.len(), 5);
        assert_eq!(std::fs::read_to_string(sub.join("navy.toml")).unwrap(), BUILTIN[0].1);
        std::fs::write(sub.join("navy.toml"), "edited").unwrap();
        assert!(extract(&sub).unwrap().is_empty(), "nothing to write the second time");
        assert_eq!(std::fs::read_to_string(sub.join("navy.toml")).unwrap(), "edited");
    }

    #[test]
    fn timo_purple_is_built_in_and_its_accent_is_the_cyan() {
        // Its other surfaces (banner, sidebar, inactive tabs, selection, find) are checked for
        // every built-in through `contrast_pairs`.
        let t = builtin("timo-purple").expect("ships");
        assert_eq!(t.name, "timo-purple");
        assert_eq!(BUILTIN.len(), 5);
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
        assert_eq!(l[1], ("navy-light".into(), Source::BuiltIn));
        assert_eq!(l[2], ("timo-purple".into(), Source::BuiltIn));
        assert_eq!(l[3], ("peach-light".into(), Source::BuiltIn));
        assert_eq!(l[4], ("lavender-light".into(), Source::BuiltIn));
        assert!(matches!(&l[5], (n, Source::Invalid(e)) if n == "broken" && !e.is_empty()));
        assert_eq!(l[6], ("mine".into(), Source::File));
        assert_eq!(validate_file(&dir.path().join("mine.toml")), Ok("mine".into()));
        assert!(validate_file(&dir.path().join("broken.toml")).is_err());
        assert!(validate_file(&dir.path().join("none.toml")).is_err());
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(theme_cli_to(&["list".into()], dir.path(), &mut out, &mut err), 0);
        let lines: Vec<&str> = std::str::from_utf8(&out).unwrap().lines().collect();
        assert_eq!(lines[0], "navy (file)");
        assert_eq!(lines[1], "navy-light (built in)");
        assert_eq!(lines[2], "timo-purple (built in)");
        assert_eq!(lines[3], "peach-light (built in)");
        assert_eq!(lines[4], "lavender-light (built in)");
        assert!(lines[5].starts_with("broken (file, invalid: "));
        assert_eq!(lines[6], "mine (file)");
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
        assert_eq!(v[0], serde_json::json!({ "name": "navy", "source": "file", "path": dir.path().join("navy.toml").display().to_string() }));
        assert_eq!(v[1], serde_json::json!({ "name": "navy-light", "source": "built-in" }));
        assert_eq!((v[5]["source"].as_str(), v[5]["error"].is_string()), (Some("invalid"), true));
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
        for builtin in ["navy", "navy-light", "timo-purple"] {
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

    #[test]
    fn a_blend_walks_every_colour_from_one_theme_to_the_other() {
        let from = builtin("navy").unwrap();
        let mut to = builtin("navy-light").unwrap();
        to.font.ui = Some("Avenir".into());
        let start = blend(&from, &to, 0.0);
        assert_eq!(start.chrome, from.chrome);
        assert_eq!(start.terminal, from.terminal);
        assert_eq!(start.semantic, from.semantic);
        assert_eq!(start.tree, from.tree);
        assert_eq!(start.viewer, from.viewer);
        assert_eq!((start.name.as_str(), &start.font), ("navy-light", &to.font), "the name and fonts are the target's from the first frame");
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
}
