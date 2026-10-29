//! An effect's folder: its manifest, `effect.toml`, and the shaders beside it (spec 4.1).
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::BUILTIN;
use crate::theme::is_bare_name;

/// An effect's `effect.toml`. Every key but `name` and `wash` has a default.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    pub description: String,
    /// The wash shader's file, beside the manifest.
    pub wash: String,
    /// The glyph shader's file, beside the manifest; absent, gpui draws text as always.
    pub glyphs: Option<String>,
    /// The wash's frames per second while anything moves.
    pub frame_rate: u32,
    /// The wash's pixels per window pixel, each side.
    pub scale: f32,
    /// Points added to the project window's UI text.
    pub ui_text_bump: f32,
    /// The app hides its own 1 px borders and the wash draws the edges.
    pub ink_borders: bool,
    pub highlights: Highlights,
    /// How the app draws its menus and popovers while the effect is on.
    pub menus: MenuLook,
    /// A terminal's or document's paper over the wash.
    pub paper_alpha: f32,
    /// The bars and the sidebar over the wash.
    pub chrome_alpha: f32,
    /// The active tab over the wash.
    pub active_tab_alpha: f32,
    /// How far the text moves towards ink (black on a light theme, white on a dark one) while
    /// the effect is on.
    pub text_contrast: f32,
    /// The glyph shader moves with time, so the text is redrawn on a timer while the effect is
    /// on (at most `active::GLYPH_RATE_CAP` frames a second).
    pub glyph_motion: bool,
    /// The frames a second the text is redrawn at while `glyph_motion` is on. Absent, the
    /// wash's rate capped at `active::GLYPH_RATE_CAP`.
    pub glyph_rate: Option<u32>,
    /// The terminal decodes changed cells and brightens fresh ones while the effect is settled
    /// on. Needs `glyph_motion`.
    pub terminal_decode: bool,
}

impl Manifest {
    /// The frames a second the text is redrawn at while its shader moves: the `glyph_rate` key,
    /// else the wash's `frame_rate` capped at `active::GLYPH_RATE_CAP`.
    pub fn glyph_rate(&self) -> u32 {
        self.glyph_rate.unwrap_or_else(|| self.frame_rate.min(crate::effects::active::GLYPH_RATE_CAP))
    }
}

impl Default for Manifest {
    fn default() -> Self {
        Manifest {
            name: String::new(),
            description: String::new(),
            wash: String::new(),
            glyphs: None,
            frame_rate: 30,
            scale: 0.5,
            ui_text_bump: 0.0,
            ink_borders: false,
            highlights: Highlights::App,
            menus: MenuLook::App,
            paper_alpha: 0.5,
            chrome_alpha: 0.0,
            active_tab_alpha: 0.55,
            text_contrast: 0.0,
            glyph_motion: false,
            glyph_rate: None,
            terminal_decode: false,
        }
    }
}

/// Who paints the selected and hovered rows and the hovered tab: the wash, from the highlight
/// blocks, or the app with its own fills (and no highlight blocks reported).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Highlights {
    Effect,
    #[default]
    App,
}

/// How the app draws its menus and popovers under an effect. The wash is painted under the
/// window, so it cannot reach a menu over the panes; the app draws the menu to match instead:
/// `ink` a card of the window's paper with an ink line and the hovered row in thinned pigment
/// (watercolor's), `glow` a card of the window's ground with a glowing line in the focus ring's
/// colour and the hovered row lit (space's), `app` the app's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MenuLook {
    Ink,
    Glow,
    #[default]
    App,
}

/// Refuses `value` outside `lo..=hi`, naming the key and the range.
fn in_range<T: PartialOrd + std::fmt::Debug>(key: &str, value: T, lo: T, hi: T) -> Result<(), String> {
    if lo <= value && value <= hi {
        Ok(())
    } else {
        Err(format!("{key} must be {lo:?} to {hi:?} (was {value:?})"))
    }
}

/// Reads a manifest and checks it: the required keys, each value's range, and shader files that
/// are bare names beside the manifest.
pub fn parse(text: &str) -> Result<Manifest, String> {
    let m: Manifest =
        toml::from_str(text).map_err(|e| e.message().lines().next().unwrap_or("invalid manifest").to_string())?;
    if m.name.is_empty() {
        return Err("name is required".into());
    }
    if m.wash.is_empty() {
        return Err("wash is required".into());
    }
    for (key, file) in [("wash", Some(&m.wash)), ("glyphs", m.glyphs.as_ref())] {
        if let Some(file) = file.filter(|f| !is_bare_name(f)) {
            return Err(format!("{key} must be a file beside effect.toml (was {file:?})"));
        }
    }
    in_range("frame_rate", m.frame_rate, 1, 60)?;
    in_range("scale", m.scale, 0.1, 1.0)?;
    in_range("ui_text_bump", m.ui_text_bump, 0.0, 4.0)?;
    in_range("paper_alpha", m.paper_alpha, 0.5, 1.0)?;
    in_range("chrome_alpha", m.chrome_alpha, 0.0, 1.0)?;
    in_range("active_tab_alpha", m.active_tab_alpha, 0.0, 1.0)?;
    in_range("text_contrast", m.text_contrast, 0.0, 1.0)?;
    if let Some(r) = m.glyph_rate {
        in_range("glyph_rate", r, 1, 20)?;
    }
    if m.terminal_decode && !m.glyph_motion {
        return Err("terminal_decode = true requires glyph_motion = true".into());
    }
    Ok(m)
}

/// An effect ready to compile: its manifest and the text of its shaders.
#[derive(Debug, Clone, PartialEq)]
pub struct Effect {
    pub manifest: Manifest,
    pub wash: String,
    pub glyphs: Option<String>,
    pub source: Source,
}

/// Where a listed effect comes from. A folder that cannot be loaded is `Invalid` with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    BuiltIn,
    Folder(PathBuf),
    Invalid(String),
}

/// Reads the effect in `folder`: its manifest and the shaders it names. An effect's name is its
/// folder's, so a manifest naming another is refused. Errors name the file.
pub fn load_folder(folder: &Path) -> Result<Effect, String> {
    let read = |file: &str| -> Result<String, String> {
        let path = folder.join(file);
        std::fs::read_to_string(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => format!("{}: not found", path.display()),
            _ => format!("cannot read {}: {e}", path.display()),
        })
    };
    let at = |e: String| format!("{}: {e}", folder.join("effect.toml").display());
    let manifest = parse(&read("effect.toml")?).map_err(at)?;
    let own = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if manifest.name != own {
        return Err(at(format!("name '{}' does not match its folder '{own}'", manifest.name)));
    }
    let wash = read(&manifest.wash)?;
    let glyphs = manifest.glyphs.as_deref().map(read).transpose()?;
    Ok(Effect { manifest, wash, glyphs, source: Source::Folder(folder.to_path_buf()) })
}

fn load_builtin(name: &str) -> Option<Effect> {
    let b = BUILTIN.iter().find(|b| b.name == name)?;
    // The built-in manifests are tested to parse; a failure here is a build error, not a user one.
    let manifest = parse(b.manifest).ok()?;
    Some(Effect { manifest, wash: b.wash.to_string(), glyphs: b.glyphs.map(str::to_string), source: Source::BuiltIn })
}

/// The effect named: `dir/<name>/` if that folder exists, else the built-in of that name.
pub fn load(name: &str, dir: &Path) -> Result<Effect, String> {
    if !is_bare_name(name) {
        return Err(format!("effect name {name:?} must be a bare folder name"));
    }
    let folder = dir.join(name);
    if folder.is_dir() {
        return load_folder(&folder);
    }
    load_builtin(name).ok_or_else(|| format!("effect {name:?} is not built in and {} does not exist", folder.display()))
}

/// Every effect by name: the built-ins first, then the folder's others sorted. A folder that
/// shadows a built-in is listed once, in the built-in's place, as a folder.
pub fn list(dir: &Path) -> Vec<(String, Source)> {
    let from_folder = |name: &str| -> Option<Source> {
        let folder = dir.join(name);
        if !folder.is_dir() {
            return None;
        }
        Some(match load_folder(&folder) {
            Ok(e) => e.source,
            Err(e) => Source::Invalid(e),
        })
    };
    let mut out: Vec<(String, Source)> =
        BUILTIN.iter().map(|b| (b.name.to_string(), from_folder(b.name).unwrap_or(Source::BuiltIn))).collect();
    let mut others: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.') && !BUILTIN.iter().any(|b| b.name == n))
        .collect();
    others.sort();
    out.extend(others.into_iter().filter_map(|n| from_folder(&n).map(|s| (n, s))));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::BUILTIN;

    const MINIMAL: &str = "name = \"plain\"\nwash = \"wash.metal\"\n";

    fn with(line: &str) -> String {
        format!("{MINIMAL}{line}\n")
    }

    fn folder(dir: &Path, name: &str, manifest: &str, files: &[&str]) -> PathBuf {
        let f = dir.join(name);
        std::fs::create_dir_all(&f).unwrap();
        std::fs::write(f.join("effect.toml"), manifest).unwrap();
        for file in files {
            std::fs::write(f.join(file), format!("// {file} of {name}\n")).unwrap();
        }
        f
    }

    #[test]
    fn a_manifest_with_only_a_name_and_a_wash_takes_every_default() {
        let m = parse(MINIMAL).unwrap();
        assert_eq!(m.name, "plain");
        assert_eq!(m.wash, "wash.metal");
        assert_eq!(m.description, "");
        assert_eq!(m.glyphs, None);
        assert_eq!(m.frame_rate, 30);
        assert_eq!(m.scale, 0.5);
        assert_eq!(m.ui_text_bump, 0.0);
        assert!(!m.ink_borders);
        assert_eq!(m.highlights, Highlights::App);
        assert_eq!(m.paper_alpha, 0.5);
        assert_eq!(m.chrome_alpha, 0.0);
        assert_eq!(m.active_tab_alpha, 0.55);
        assert_eq!(m.text_contrast, 0.0);
    }

    #[test]
    fn a_value_out_of_range_is_refused_with_its_key_and_range() {
        let e = parse(&with("frame_rate = 90")).unwrap_err();
        assert!(e.contains("frame_rate must be 1 to 60 (was 90)"), "{e}");
        let e = parse(&with("scale = 0.05")).unwrap_err();
        assert!(e.contains("scale must be 0.1 to 1.0 (was 0.05)"), "{e}");
        let e = parse(&with("ui_text_bump = 5")).unwrap_err();
        assert!(e.contains("ui_text_bump must be 0.0 to 4.0 (was 5.0)"), "{e}");
        let e = parse(&with("paper_alpha = 0.3")).unwrap_err();
        assert!(e.contains("paper_alpha must be 0.5 to 1.0"), "{e}");
        let e = parse(&with("chrome_alpha = 1.5")).unwrap_err();
        assert!(e.contains("chrome_alpha must be 0.0 to 1.0"), "{e}");
        let e = parse(&with("active_tab_alpha = -0.1")).unwrap_err();
        assert!(e.contains("active_tab_alpha must be 0.0 to 1.0"), "{e}");
        let e = parse(&with("text_contrast = 1.2")).unwrap_err();
        assert!(e.contains("text_contrast must be 0.0 to 1.0 (was 1.2)"), "{e}");
        assert!(parse(&with("frame_rate = 60\nscale = 1.0\nui_text_bump = 4\npaper_alpha = 1.0")).is_ok());
    }

    #[test]
    fn an_unknown_key_and_a_missing_name_are_refused() {
        let e = parse(&with("sparkle = true")).unwrap_err();
        assert!(e.contains("sparkle"), "{e}");
        let e = parse("wash = \"wash.metal\"\n").unwrap_err();
        assert!(e.contains("name"), "{e}");
        let e = parse("name = \"plain\"\n").unwrap_err();
        assert!(e.contains("wash"), "{e}");
        let e = parse(&with("highlights = \"sometimes\"")).unwrap_err();
        assert!(e.contains("sometimes"), "{e}");
    }

    #[test]
    fn a_manifest_may_set_the_text_s_own_frame_rate_and_it_defaults_to_the_capped_wash_rate() {
        assert_eq!(parse(MINIMAL).unwrap().glyph_rate, None);
        assert_eq!(parse(MINIMAL).unwrap().glyph_rate(), 20, "frame_rate 30 capped at 20");
        assert_eq!(parse(&with("frame_rate = 12")).unwrap().glyph_rate(), 12);
        assert_eq!(parse(&with("glyph_rate = 10")).unwrap().glyph_rate(), 10);
        let e = parse(&with("glyph_rate = 30")).unwrap_err();
        assert!(e.contains("glyph_rate must be 1 to 20 (was 30)"), "{e}");
        assert!(parse(&with("glyph_rate = 0")).is_err());
    }

    #[test]
    fn a_manifest_may_ask_the_terminal_to_decode_changed_cells_only_with_moving_glyphs() {
        assert!(!parse(MINIMAL).unwrap().terminal_decode);
        assert!(parse(&with("glyph_motion = true\nterminal_decode = true")).unwrap().terminal_decode);
        let e = parse(&with("terminal_decode = true")).unwrap_err();
        assert_eq!(e, "terminal_decode = true requires glyph_motion = true");
        for b in BUILTIN.iter().filter(|b| b.name != "matrix") {
            assert!(!parse(b.manifest).unwrap().terminal_decode, "{} does not decode", b.name);
        }
    }

    #[test]
    fn a_manifest_may_say_its_glyphs_move_and_watercolor_s_do_not() {
        assert!(!parse(MINIMAL).unwrap().glyph_motion);
        assert!(parse(&with("glyph_motion = true")).unwrap().glyph_motion);
        let b = BUILTIN.iter().find(|b| b.name == "watercolor").unwrap();
        assert!(!parse(b.manifest).unwrap().glyph_motion);
    }

    #[test]
    fn the_built_in_watercolor_parses_and_names_both_shaders() {
        let b = BUILTIN.iter().find(|b| b.name == "watercolor").expect("watercolor is built in");
        let m = parse(b.manifest).unwrap();
        assert_eq!(m.name, "watercolor");
        assert_eq!(m.wash, "wash.metal");
        assert_eq!(m.glyphs.as_deref(), Some("glyphs.metal"));
        assert_eq!(m.highlights, Highlights::Effect);
        assert!(m.ink_borders);
        assert_eq!(m.ui_text_bump, 2.0);
        assert_eq!(m.text_contrast, 0.6);
        assert!(b.wash.contains("fragment float4 arkos_wash("));
        let glyphs = b.glyphs.expect("watercolor draws its own glyphs");
        assert!(glyphs.contains("arkos_glyph_vertex(") && glyphs.contains("arkos_glyph_fragment("));
        let e = load("watercolor", Path::new("/nonexistent/effects")).unwrap();
        assert_eq!(e.source, Source::BuiltIn);
        assert_eq!(e.wash, b.wash);
    }

    #[test]
    fn watercolor_takes_its_pigments_from_the_theme_s_own_four() {
        let wash = BUILTIN.iter().find(|b| b.name == "watercolor").unwrap().wash;
        assert!(wash.contains("return u.colors[ARKOS_PIGMENT + k].rgb;"), "the theme's own first");
        assert!(wash.contains("int n = clamp(int(u.pigment_count), 1, 4);"), "then variations of them");
        assert!(wash.contains("if (u.dark > 0.5)"), "the theme's mode decides how they are laid");
        assert!(wash.contains("col = keep_readable(u, col);"), "the text stays readable");
    }

    #[test]
    fn watercolor_washes_a_menu_s_backdrop_as_it_does_a_pane() {
        let wash = BUILTIN.iter().find(|b| b.name == "watercolor").unwrap().wash;
        assert!(wash.contains("if (kind == ARKOS_MENU_BLOCK) { return 0.14; }"), "a menu gets a pane body's light wash");
    }

    #[test]
    fn watercolor_inks_the_menus_edges_with_the_panes_own_line() {
        let wash = BUILTIN.iter().find(|b| b.name == "watercolor").unwrap().wash;
        let start = wash.find("fragment float4 arkos_overlay(").expect("watercolor inks the menus' edges");
        assert!(wash[start..].contains("ink_for_block("), "the menu stage draws the panes' ink line");
        assert!(wash[..start].matches("ink_for_block(").count() >= 2, "ink_for_block is shared with the wash");
    }

    #[test]
    fn an_effect_chooses_how_its_menus_look_and_the_app_s_own_is_the_default() {
        let base = "name = \"x\"\nwash = \"wash.metal\"\n";
        assert_eq!(parse(base).unwrap().menus, MenuLook::App);
        assert_eq!(parse(&format!("{base}menus = \"ink\"\n")).unwrap().menus, MenuLook::Ink);
        assert_eq!(parse(&format!("{base}menus = \"glow\"\n")).unwrap().menus, MenuLook::Glow);
        assert_eq!(parse(&format!("{base}menus = \"app\"\n")).unwrap().menus, MenuLook::App);
        assert!(parse(&format!("{base}menus = \"sparkly\"\n")).is_err());
        let watercolor = BUILTIN.iter().find(|b| b.name == "watercolor").unwrap();
        assert_eq!(parse(watercolor.manifest).unwrap().menus, MenuLook::Ink, "watercolor's menus are inked paper");
    }

    #[test]
    fn the_built_in_space_parses_with_its_fixed_values() {
        let b = BUILTIN.iter().find(|b| b.name == "space").expect("space is built in");
        let m = parse(b.manifest).unwrap();
        assert_eq!(m.name, "space");
        assert!(!m.description.is_empty());
        assert_eq!(m.wash, "wash.metal");
        assert_eq!(m.glyphs.as_deref(), Some("glyphs.metal"));
        assert_eq!(m.frame_rate, 30);
        assert_eq!(m.scale, 0.5);
        assert_eq!(m.ui_text_bump, 0.0);
        assert!(m.ink_borders);
        assert_eq!(m.highlights, Highlights::Effect);
        assert_eq!(m.menus, MenuLook::Glow, "space's menus glow");
        assert_eq!(m.paper_alpha, 0.8);
        assert_eq!(m.chrome_alpha, 0.0);
        assert_eq!(m.active_tab_alpha, 0.5);
        assert_eq!(m.text_contrast, 0.0);
        assert!(!m.glyph_motion, "the holo text is still: moving text hurt Cyrus's eyes");
        assert!(b.wash.contains("fragment float4 arkos_wash("));
        let e = load("space", Path::new("/nonexistent/effects")).unwrap();
        assert_eq!(e.source, Source::BuiltIn);
        assert_eq!(e.glyphs.as_deref(), b.glyphs);
    }

    #[test]
    fn a_folder_in_the_effects_dir_is_listed_and_shadows_a_built_in() {
        let dir = tempfile::tempdir().unwrap();
        let mine = folder(dir.path(), "watercolor", "name = \"watercolor\"\nwash = \"wash.metal\"\n", &["wash.metal"]);
        folder(dir.path(), "neon", "name = \"neon\"\nwash = \"wash.metal\"\n", &["wash.metal"]);
        let listed = list(dir.path());
        let named: Vec<&(String, Source)> = listed.iter().filter(|(n, _)| n == "watercolor").collect();
        assert_eq!(named.len(), 1, "{listed:?}");
        assert_eq!(named[0].1, Source::Folder(mine));
        assert!(listed.iter().any(|(n, s)| n == "neon" && matches!(s, Source::Folder(_))), "{listed:?}");
        assert_eq!(
            list(Path::new("/nonexistent/effects")),
            vec![("watercolor".to_string(), Source::BuiltIn), ("space".to_string(), Source::BuiltIn)]
        );
    }

    #[test]
    fn a_folder_with_a_bad_manifest_or_a_missing_shader_is_invalid_and_says_why() {
        let dir = tempfile::tempdir().unwrap();
        folder(dir.path(), "broken", "name = \"broken\"\nwash = \"wash.metal\"\nframe_rate = 0\n", &["wash.metal"]);
        folder(dir.path(), "hollow", "name = \"hollow\"\nwash = \"wash.metal\"\n", &[]);
        folder(dir.path(), "half", "name = \"half\"\nwash = \"wash.metal\"\nglyphs = \"glyphs.metal\"\n", &["wash.metal"]);
        std::fs::create_dir_all(dir.path().join("empty")).unwrap();
        let listed = list(dir.path());
        let why = |name: &str| match listed.iter().find(|(n, _)| n == name) {
            Some((_, Source::Invalid(e))) => e.clone(),
            other => panic!("{name} should be invalid, was {other:?}"),
        };
        assert!(why("broken").contains("effect.toml") && why("broken").contains("frame_rate"), "{}", why("broken"));
        assert!(why("hollow").contains("wash.metal: not found"), "{}", why("hollow"));
        assert!(why("half").contains("glyphs.metal: not found"), "{}", why("half"));
        assert!(why("empty").contains("effect.toml: not found"), "{}", why("empty"));
        assert!(load("hollow", dir.path()).unwrap_err().contains("wash.metal: not found"));
    }

    #[test]
    fn a_folder_whose_manifest_names_another_effect_is_invalid_for_list_and_load() {
        let dir = tempfile::tempdir().unwrap();
        folder(dir.path(), "foo", "name = \"bar\"\nwash = \"wash.metal\"\n", &["wash.metal"]);
        let listed = list(dir.path());
        match listed.iter().find(|(n, _)| n == "foo") {
            Some((_, Source::Invalid(e))) => assert!(e.contains("name 'bar' does not match its folder 'foo'"), "{e}"),
            other => panic!("foo should be invalid, was {other:?}"),
        }
        assert!(load("foo", dir.path()).unwrap_err().contains("name 'bar' does not match its folder 'foo'"));
        assert!(load_folder(&dir.path().join("foo")).is_err());
    }

    #[test]
    fn load_reads_the_shader_files_beside_the_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let f = folder(
            dir.path(),
            "ink",
            "name = \"ink\"\nwash = \"wash.metal\"\nglyphs = \"glyphs.metal\"\n",
            &["wash.metal", "glyphs.metal"],
        );
        let e = load("ink", dir.path()).unwrap();
        assert_eq!(e.manifest.name, "ink");
        assert_eq!(e.wash, "// wash.metal of ink\n");
        assert_eq!(e.glyphs.as_deref(), Some("// glyphs.metal of ink\n"));
        assert_eq!(e.source, Source::Folder(f));
        assert!(load("../ink", dir.path()).is_err());
        assert!(load("nowhere", dir.path()).unwrap_err().contains("nowhere"));
    }
}
