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
    /// Points added inside the title bar at both ends, so its words and icons sit clear of a
    /// shape the wash draws round them.
    pub title_pad: f32,
    /// Points added to the title bar's height, at its foot: its words, its icons and the traffic
    /// lights keep their places, with that much more of the bar under them.
    pub title_grow: f32,
    /// Points added before every row and header of the sidebar, so their text sits clear of a
    /// frame and a cursor the wash draws along the sidebar's left edge.
    pub sidebar_pad: f32,
    /// Points at the right end of a pane's bar that the wash draws in (overworld's gem): the
    /// Inbox's session header, which has no + button to fill them, keeps its buttons left of them.
    pub bar_end_clear: f32,
    /// The width in points of a gutter between the sidebar and the panes, for the wash to draw
    /// in (reported as a binder block); 0, none.
    pub binder: f32,
    /// Points kept clear round each pane: between the panes, and between the panes and the
    /// window's bars and edges, so the wash can draw them as cards on a ground; 0, none.
    pub gap: f32,
    /// Points kept between the panes and the window's bars and edges (the sidebar's margin too,
    /// with `sidebar_margin`); absent, `gap`.
    pub edge_gap: Option<f32>,
    /// The corner radius in points of the panes as the wash draws them; with one, the focused
    /// pane's glow is a ring rounded to match instead of the four straight strips.
    pub pane_radius: f32,
    /// The app hides its own 1 px borders and the wash draws the edges.
    pub ink_borders: bool,
    /// The sidebar keeps the `gap` round it as a pane does (on the window's side, above and below;
    /// the gutter to the panes is already the gap), so the wash frames it as a pane.
    pub sidebar_margin: bool,
    pub highlights: Highlights,
    /// How the app draws its menus and popovers while the effect is on.
    pub menus: MenuLook,
    /// How the keyboard's ring in the sidebar looks, to match how the effect draws a selected
    /// row.
    pub keys_ring: KeysRing,
    /// A terminal's or document's paper over the wash.
    pub paper_alpha: f32,
    /// A document's paper over the wash (rendered markdown and the viewer). Absent,
    /// `paper_alpha`.
    pub document_paper_alpha: Option<f32>,
    /// A menu's paper over the backdrop the effect paints for it, so its rows read over the
    /// effect. Absent, `paper_alpha`.
    pub menu_paper_alpha: Option<f32>,
    /// The bars and the sidebar over the wash.
    pub chrome_alpha: f32,
    /// The bars inside a tab's body over the wash: a grid's banner, a sheet strip, a video's
    /// bar and a web tab's address bar. Absent, `chrome_alpha`.
    pub body_bars_alpha: Option<f32>,
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
    /// The terminal paints changed cells in while the effect is settled on: it stamps them and
    /// its glyph shader paints the ink in from the stamp (`effects::paint`). Needs a glyphs file,
    /// and an effect paints or decodes, not both.
    pub terminal_paint: bool,
    /// The scenes the wash can draw, by name, in the order of the number it is handed
    /// (`ArkosInputs::scene`); a theme names one (`[effect] scene`). Empty, one scene, 0.
    pub scenes: Vec<String>,
    /// Images the wash samples (`[[texture(0)]]`, a texture array in this order, each image's
    /// size in `ARKOS_IMAGE_SIZE`): JPEG or PNG files in the effect's folder, or in a folder in
    /// it, at most `images::MAX_IMAGES`.
    pub images: Vec<String>,
}

impl Manifest {
    /// The number of the scene a theme names among this effect's `scenes`; 0 for none, or a
    /// name the effect does not list.
    pub fn scene_index(&self, name: Option<&str>) -> f32 {
        name.and_then(|n| self.scenes.iter().position(|s| s == n)).map_or(0.0, |i| i as f32)
    }

    /// The frames a second the text is redrawn at while its shader moves: the `glyph_rate` key,
    /// else the wash's `frame_rate` capped at `active::GLYPH_RATE_CAP`.
    pub fn glyph_rate(&self) -> u32 {
        self.glyph_rate.unwrap_or_else(|| self.frame_rate.min(crate::effects::active::GLYPH_RATE_CAP))
    }

    /// The bars inside a tab's body: the `body_bars_alpha` key, else `chrome_alpha`.
    pub fn body_bars_alpha(&self) -> f32 {
        self.body_bars_alpha.unwrap_or(self.chrome_alpha)
    }

    /// The paper a menu reads on: the `menu_paper_alpha` key, else `paper_alpha`.
    pub fn menu_paper_alpha(&self) -> f32 {
        self.menu_paper_alpha.unwrap_or(self.paper_alpha)
    }

    /// The paper a document reads on: the `document_paper_alpha` key, else `paper_alpha`.
    pub fn document_paper_alpha(&self) -> f32 {
        self.document_paper_alpha.unwrap_or(self.paper_alpha)
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
            title_pad: 0.0,
            title_grow: 0.0,
            sidebar_pad: 0.0,
            bar_end_clear: 0.0,
            binder: 0.0,
            gap: 0.0,
            edge_gap: None,
            pane_radius: 0.0,
            ink_borders: false,
            sidebar_margin: false,
            highlights: Highlights::App,
            menus: MenuLook::App,
            keys_ring: KeysRing::Outline,
            paper_alpha: 0.5,
            document_paper_alpha: None,
            menu_paper_alpha: None,
            chrome_alpha: 0.0,
            body_bars_alpha: None,
            active_tab_alpha: 0.55,
            text_contrast: 0.0,
            glyph_motion: false,
            glyph_rate: None,
            terminal_decode: false,
            terminal_paint: false,
            scenes: Vec::new(),
            images: Vec::new(),
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

/// How the keyboard's ring on a sidebar row or heading looks under an effect (Cyrus, 2026-11-17:
/// the in-focus element should match "the theme's effect"): `outline`, a thin line round the row
/// (the app's own, the default); `pill`, a pill inset as material's selected rows are, filled with
/// a fainter selection colour; `glow`, a glowing line round the row with a faint wash of the
/// focus colour (runic's lit rows); `pixel`, a square two point frame with a fainter fill of the
/// active tab's colour (overworld's selected rows); `wash`, a soft wash of the first pigment with
/// a feathered edge and no line (watercolor's washed rows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeysRing {
    #[default]
    Outline,
    Pill,
    Glow,
    Pixel,
    Wash,
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
    if m.images.len() > super::images::MAX_IMAGES {
        return Err(format!("images may list at most {} files (lists {})", super::images::MAX_IMAGES, m.images.len()));
    }
    for file in &m.images {
        let parts: Vec<&str> = file.split('/').collect();
        let ext = file.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
        if parts.len() > 2 || parts.iter().any(|p| !is_bare_name(p)) || !["jpg", "jpeg", "png"].contains(&ext.as_str()) {
            return Err(format!("images must be .jpg, .jpeg or .png files in the effect's folder or a folder in it (was {file:?})"));
        }
    }
    in_range("frame_rate", m.frame_rate, 1, 60)?;
    in_range("scale", m.scale, 0.1, 1.0)?;
    in_range("ui_text_bump", m.ui_text_bump, 0.0, 4.0)?;
    in_range("title_pad", m.title_pad, 0.0, 24.0)?;
    in_range("title_grow", m.title_grow, 0.0, 16.0)?;
    in_range("sidebar_pad", m.sidebar_pad, 0.0, 24.0)?;
    in_range("bar_end_clear", m.bar_end_clear, 0.0, 64.0)?;
    in_range("binder", m.binder, 0.0, 32.0)?;
    in_range("gap", m.gap, 0.0, 16.0)?;
    if let Some(e) = m.edge_gap {
        in_range("edge_gap", e, 0.0, 16.0)?;
    }
    in_range("pane_radius", m.pane_radius, 0.0, 24.0)?;
    in_range("paper_alpha", m.paper_alpha, 0.5, 1.0)?;
    if let Some(a) = m.document_paper_alpha {
        in_range("document_paper_alpha", a, 0.5, 1.0)?;
    }
    if let Some(a) = m.menu_paper_alpha {
        in_range("menu_paper_alpha", a, 0.5, 1.0)?;
    }
    in_range("chrome_alpha", m.chrome_alpha, 0.0, 1.0)?;
    if let Some(a) = m.body_bars_alpha {
        in_range("body_bars_alpha", a, 0.0, 1.0)?;
    }
    in_range("active_tab_alpha", m.active_tab_alpha, 0.0, 1.0)?;
    in_range("text_contrast", m.text_contrast, 0.0, 1.0)?;
    if let Some(r) = m.glyph_rate {
        in_range("glyph_rate", r, 1, 20)?;
    }
    if m.terminal_decode && !m.glyph_motion {
        return Err("terminal_decode = true requires glyph_motion = true".into());
    }
    if m.terminal_paint && m.glyphs.is_none() {
        return Err("terminal_paint = true requires a glyphs file".into());
    }
    if m.terminal_paint && m.terminal_decode {
        return Err("terminal_paint and terminal_decode cannot both be true".into());
    }
    Ok(m)
}

/// An effect ready to compile: its manifest and the text of its shaders.
#[derive(Debug, Clone, PartialEq)]
pub struct Effect {
    pub manifest: Manifest,
    pub wash: String,
    pub glyphs: Option<String>,
    /// The files the manifest's `images` names, as read, in its order.
    pub images: Vec<std::sync::Arc<[u8]>>,
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
    let images = manifest
        .images
        .iter()
        .map(|file| {
            let path = folder.join(file);
            let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            super::images::dimensions(&bytes).map_err(|e| format!("{}: not an image Arkos can read ({e})", path.display()))?;
            Ok(std::sync::Arc::<[u8]>::from(bytes))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Effect { manifest, wash, glyphs, images, source: Source::Folder(folder.to_path_buf()) })
}

fn load_builtin(name: &str) -> Option<Effect> {
    let b = BUILTIN.iter().find(|b| b.name == name)?;
    // The built-in manifests are tested to parse; a failure here is a build error, not a user one.
    let manifest = parse(b.manifest).ok()?;
    // Each image the manifest names, from the files compiled in beside it.
    let images = manifest.images.iter().map(|file| b.images.iter().find(|(n, _)| n == file).map(|(_, bytes)| std::sync::Arc::<[u8]>::from(*bytes))).collect::<Option<Vec<_>>>()?;
    Some(Effect { manifest, wash: b.wash.to_string(), glyphs: b.glyphs.map(str::to_string), images, source: Source::BuiltIn })
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

/// Copies the effect in the folder `src` into the effects folder `dir`, under its own name, and
/// returns that name. The folder must hold an effect that loads (its manifest naming the folder,
/// its shader files beside it). A name already in `dir` is `Err("exists")` unless `replace`.
/// Only the folder's own files are copied, not what lies in folders inside it.
pub fn import_folder(src: &Path, dir: &Path, replace: bool) -> Result<String, String> {
    let loaded = load_folder(src)?;
    let name = loaded.manifest.name.clone();
    let dest = dir.join(&name);
    if dest.exists() {
        if !replace {
            return Err("exists".into());
        }
        std::fs::remove_dir_all(&dest).map_err(|e| format!("cannot replace {}: {e}", dest.display()))?;
    }
    std::fs::create_dir_all(&dest).map_err(|e| format!("cannot create {}: {e}", dest.display()))?;
    let entries = std::fs::read_dir(src).map_err(|e| format!("cannot read {}: {e}", src.display()))?;
    for entry in entries.flatten() {
        let from = entry.path();
        if from.is_file() {
            std::fs::copy(&from, dest.join(entry.file_name())).map_err(|e| format!("cannot copy {}: {e}", from.display()))?;
        }
    }
    // The images in a folder of the effect's own.
    for file in loaded.manifest.images.iter().filter(|f| f.contains('/')) {
        let to = dest.join(file);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        std::fs::copy(src.join(file), &to).map_err(|e| format!("cannot copy {}: {e}", src.join(file).display()))?;
    }
    load_folder(&dest).map(|_| name)
}

/// Every effect by name: the built-ins first, then the folder's others sorted. A folder that
/// shadows a built-in is listed once, in the built-in's place, as a folder. A hidden built-in
/// (`BuiltIn::hidden`) is left out unless a folder shadows it.
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
        BUILTIN.iter().filter_map(|b| match from_folder(b.name) {
            Some(source) => Some((b.name.to_string(), source)),
            None => (!b.hidden).then(|| (b.name.to_string(), Source::BuiltIn)),
        }).collect();
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
    fn images_are_jpeg_or_png_files_in_the_folder_at_most_eight_and_load_with_it() {
        assert_eq!(parse(MINIMAL).unwrap().images, Vec::<String>::new());
        assert_eq!(parse(&with("images = [\"a.jpg\", \"gallery/b.png\"]")).unwrap().images, ["a.jpg", "gallery/b.png"]);
        for bad in ["../a.jpg", "/a.jpg", "a/b/c.jpg", "a.gif"] {
            assert!(parse(&with(&format!("images = [\"{bad}\"]"))).is_err(), "{bad}");
        }
        let nine: Vec<String> = (0..9).map(|i| format!("\"{i}.jpg\"")).collect();
        assert!(parse(&with(&format!("images = [{}]", nine.join(", ")))).unwrap_err().contains("at most 8"));
        // A folder's images are read with it; one that is not an image is refused, naming it.
        let dir = tempfile::tempdir().unwrap();
        let wc = load("watercolor", Path::new("/nonexistent")).unwrap();
        let f = folder(dir.path(), "pics", "name = \"pics\"\nwash = \"wash.metal\"\nimages = [\"gallery/one.jpg\"]\n", &["wash.metal"]);
        std::fs::create_dir_all(f.join("gallery")).unwrap();
        std::fs::write(f.join("gallery/one.jpg"), &*wc.images[0]).unwrap();
        let e = load_folder(&f).unwrap();
        assert_eq!(e.images.len(), 1);
        assert_eq!(&*e.images[0], &*wc.images[0]);
        std::fs::write(f.join("gallery/one.jpg"), b"text").unwrap();
        assert!(load_folder(&f).unwrap_err().contains("one.jpg: not an image"));
        // Imported, its images come along.
        std::fs::write(f.join("gallery/one.jpg"), &*wc.images[0]).unwrap();
        let effects = dir.path().join("effects");
        std::fs::create_dir_all(&effects).unwrap();
        assert_eq!(import_folder(&f, &effects, false).unwrap(), "pics");
        assert!(effects.join("pics/gallery/one.jpg").is_file());
    }

    #[test]
    fn body_bars_alpha_follows_chrome_alpha_unless_given_and_is_refused_out_of_range() {
        let m = parse(&with("chrome_alpha = 0.2")).unwrap();
        assert_eq!((m.body_bars_alpha, m.body_bars_alpha()), (None, 0.2));
        assert_eq!(parse(&with("body_bars_alpha = 1.0")).unwrap().body_bars_alpha(), 1.0);
        let e = parse(&with("body_bars_alpha = 1.5")).unwrap_err();
        assert!(e.contains("body_bars_alpha must be 0.0 to 1.0"), "{e}");
    }

    #[test]
    fn the_space_effect_is_hidden_from_the_list_but_still_loads_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let listed: Vec<String> = list(dir.path()).into_iter().map(|(n, _)| n).collect();
        assert!(!listed.iter().any(|n| n == "space"), "{listed:?}");
        assert!(listed.iter().any(|n| n == "watercolor") && listed.iter().any(|n| n == "material"));
        assert!(load("space", dir.path()).is_ok(), "a config naming space still draws it");
        assert!(BUILTIN.iter().filter(|b| b.hidden).map(|b| b.name).eq(["space"]), "space alone is hidden");
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
    fn the_title_padding_and_the_binder_default_to_none_and_the_notebook_asks_for_both() {
        let m = parse(MINIMAL).unwrap();
        assert_eq!((m.title_pad, m.binder), (0.0, 0.0));
        let b = BUILTIN.iter().find(|b| b.name == "notebook").unwrap();
        let m = parse(b.manifest).unwrap();
        assert!(m.title_pad > 0.0, "room around the title's words inside the cover's label");
        assert!(m.binder > 0.0, "a gutter for the spiral between the sidebar and the panes");
    }

    #[test]
    fn the_gap_defaults_to_none_is_refused_out_of_range_and_material_asks_for_one() {
        assert_eq!(parse(MINIMAL).unwrap().gap, 0.0);
        let e = parse(&with("gap = 20")).unwrap_err();
        assert!(e.contains("gap must be 0.0 to 16.0 (was 20.0)"), "{e}");
        let b = BUILTIN.iter().find(|b| b.name == "material").expect("material is built in");
        let m = parse(b.manifest).unwrap();
        assert!(m.gap > 0.0, "the panes float as cards on the frame");
        assert_eq!(m.pane_radius, 14.0, "the wash's CARD_RADIUS, so the focus ring follows the card");
        assert_eq!(parse(MINIMAL).unwrap().pane_radius, 0.0);
        let e = parse(&with("pane_radius = 30")).unwrap_err();
        assert!(e.contains("pane_radius must be 0.0 to 24.0 (was 30.0)"), "{e}");
        assert_eq!(m.binder, 0.0, "no binder: the gap is the gutter beside the sidebar");
        assert!(m.sidebar_margin && m.edge_gap.is_none(), "the sidebar is a card as the panes are, with their margins (the gap)");
        assert!(b.glyphs.is_none(), "the text stays as the font draws it");
    }

    #[test]
    fn a_value_out_of_range_is_refused_with_its_key_and_range() {
        let e = parse(&with("title_pad = 30")).unwrap_err();
        assert!(e.contains("title_pad must be 0.0 to 24.0 (was 30.0)"), "{e}");
        // The sidebar's padding: none unless asked for, and overworld asks for room for its
        // frame and its cursor.
        assert_eq!(parse(MINIMAL).unwrap().sidebar_pad, 0.0);
        assert!(!parse(MINIMAL).unwrap().sidebar_margin, "a sidebar keeps no margin unless asked");
        assert_eq!(parse(MINIMAL).unwrap().keys_ring, KeysRing::Outline);
        assert_eq!(parse(&with("keys_ring = \"pixel\"")).unwrap().keys_ring, KeysRing::Pixel);
        assert_eq!(parse(&with("keys_ring = \"wash\"")).unwrap().keys_ring, KeysRing::Wash);
        assert!(parse(&with("keys_ring = \"round\"")).is_err());
        assert_eq!(parse(&with("paper_alpha = 0.6")).unwrap().menu_paper_alpha(), 0.6, "a menu's paper is the terminal's unless told");
        assert_eq!(parse(&with("menu_paper_alpha = 0.9")).unwrap().menu_paper_alpha(), 0.9);
        assert!(parse(&with("menu_paper_alpha = 0.2")).unwrap_err().contains("menu_paper_alpha must be 0.5 to 1.0"));
        assert_eq!(parse(MINIMAL).unwrap().edge_gap, None, "the edges keep the gap unless told");
        assert_eq!(parse(&with("edge_gap = 11")).unwrap().edge_gap, Some(11.0));
        assert!(parse(&with("edge_gap = 20")).unwrap_err().contains("edge_gap must be 0.0 to 16.0"));
        assert!(parse(&with("sidebar_margin = true")).unwrap().sidebar_margin);
        assert_eq!(parse(&with("sidebar_pad = 12")).unwrap().sidebar_pad, 12.0);
        let e = parse(&with("sidebar_pad = 30")).unwrap_err();
        assert!(e.contains("sidebar_pad must be 0.0 to 24.0 (was 30.0)"), "{e}");
        for b in BUILTIN.iter() {
            assert_eq!(parse(b.manifest).unwrap().sidebar_pad > 0.0, b.name == "overworld" || b.name == "runic", "{}", b.name);
        }
        // The bar's right end kept clear: none unless asked for; overworld for its gem, runic for
        // its lamp.
        assert_eq!(parse(MINIMAL).unwrap().bar_end_clear, 0.0);
        let e = parse(&with("bar_end_clear = 80")).unwrap_err();
        assert!(e.contains("bar_end_clear must be 0.0 to 64.0 (was 80.0)"), "{e}");
        for b in BUILTIN.iter() {
            assert_eq!(parse(b.manifest).unwrap().bar_end_clear > 0.0, b.name == "overworld" || b.name == "runic", "{}", b.name);
        }
        // A taller title bar: none unless asked for, and only overworld asks.
        assert_eq!(parse(MINIMAL).unwrap().title_grow, 0.0);
        assert_eq!(parse(&with("title_grow = 6")).unwrap().title_grow, 6.0);
        let e = parse(&with("title_grow = 20")).unwrap_err();
        assert!(e.contains("title_grow must be 0.0 to 16.0 (was 20.0)"), "{e}");
        for b in BUILTIN.iter() {
            assert_eq!(parse(b.manifest).unwrap().title_grow > 0.0, b.name == "overworld", "{}", b.name);
        }
        let e = parse(&with("binder = -1")).unwrap_err();
        assert!(e.contains("binder must be 0.0 to 32.0 (was -1.0)"), "{e}");
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
    fn a_document_s_paper_defaults_to_the_terminal_s_and_is_refused_out_of_range_with_its_key() {
        let m = parse(MINIMAL).unwrap();
        assert_eq!(m.document_paper_alpha, None);
        assert_eq!(m.document_paper_alpha(), 0.5, "absent, the paper_alpha default");
        assert_eq!(parse(&with("paper_alpha = 0.7")).unwrap().document_paper_alpha(), 0.7, "absent, the effect's own paper_alpha");
        let m = parse(&with("paper_alpha = 0.7\ndocument_paper_alpha = 0.9")).unwrap();
        assert_eq!((m.paper_alpha, m.document_paper_alpha, m.document_paper_alpha()), (0.7, Some(0.9), 0.9));
        assert!(parse(&with("document_paper_alpha = 0.5")).is_ok());
        assert!(parse(&with("document_paper_alpha = 1.0")).is_ok());
        let e = parse(&with("document_paper_alpha = 0.4")).unwrap_err();
        assert!(e.contains("document_paper_alpha must be 0.5 to 1.0 (was 0.4)"), "{e}");
        let e = parse(&with("document_paper_alpha = 1.1")).unwrap_err();
        assert!(e.contains("document_paper_alpha must be 0.5 to 1.0 (was 1.1)"), "{e}");
        for b in BUILTIN.iter().filter(|b| b.name != "runic") {
            assert_eq!(parse(b.manifest).unwrap().document_paper_alpha, None, "{} reads on its terminal's paper", b.name);
        }
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
        for b in BUILTIN.iter().filter(|b| b.name != "runic") {
            assert!(!parse(b.manifest).unwrap().terminal_decode, "{} does not decode", b.name);
        }
    }

    #[test]
    fn a_manifest_may_ask_the_terminal_to_paint_changed_cells_in_only_with_a_glyphs_file_and_watercolor_notebook_and_overworld_do() {
        assert!(!parse(MINIMAL).unwrap().terminal_paint);
        assert!(parse(&with("glyphs = \"g.metal\"\nterminal_paint = true")).unwrap().terminal_paint);
        assert_eq!(parse(&with("terminal_paint = true")).unwrap_err(), "terminal_paint = true requires a glyphs file");
        let both = with("glyphs = \"g.metal\"\nglyph_motion = true\nterminal_decode = true\nterminal_paint = true");
        assert_eq!(parse(&both).unwrap_err(), "terminal_paint and terminal_decode cannot both be true");
        for b in BUILTIN.iter() {
            assert_eq!(parse(b.manifest).unwrap().terminal_paint, matches!(b.name, "watercolor" | "notebook" | "overworld"), "{}", b.name);
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
    fn the_built_in_overworld_parses_with_its_fixed_values() {
        let b = BUILTIN.iter().find(|b| b.name == "overworld").expect("overworld is built in");
        let m = parse(b.manifest).unwrap();
        assert_eq!(m.description, "A night overworld in pixel art across the window: panes framed like game menus on a night ground, stone tab bars, a sky for a title bar, a castle scene, and text typed in like dialogue.");
        assert_eq!((m.frame_rate, m.scale), (30, 1.0), "30 fps, so the chrome the wash paints keeps up with the app");
        assert!(!m.ink_borders && !b.hidden);
        assert_eq!(m.glyphs.as_deref(), Some("glyphs.metal"));
        assert!(m.terminal_paint && !m.glyph_motion && !m.terminal_decode, "typed in from the terminal's stamps, with no timer of its own");
        assert_eq!((m.highlights, m.menus), (Highlights::Effect, MenuLook::App));
        assert_eq!((m.paper_alpha, m.chrome_alpha, m.active_tab_alpha, m.text_contrast), (0.6, 0.0, 0.0, 0.0));
        assert_eq!(m.document_paper_alpha(), 0.6, "a document's paper is the terminal's: the night is behind both");
        assert_eq!((m.gap, m.pane_radius, m.binder), (15.0, 0.0, 0.0), "room round each pane for its frame");
        assert_eq!(m.body_bars_alpha(), 1.0, "the bars inside a tab's body keep their colour");
    }

    #[test]
    fn the_built_in_runic_parses_with_its_fixed_values() {
        let b = BUILTIN.iter().find(|b| b.name == "runic").expect("runic is built in");
        let m = parse(b.manifest).unwrap();
        assert_eq!((m.frame_rate, m.scale, m.ui_text_bump), (30, 0.5, 0.0), "a pixel a point on a Retina display: full size cost 11 to 14 ms a frame there");
        assert_eq!((m.gap, m.title_pad, m.sidebar_pad, m.bar_end_clear), (16.0, 8.0, 2.0, 48.0), "room for the frames, the plates, the chevron and the lamp left of the +");
        assert_eq!(m.scenes, ["city", "code"]);
        assert!(m.sidebar_margin, "the sidebar keeps a pane's margins, so it is framed as one");
        assert_eq!(m.menu_paper_alpha(), 0.92, "a menu's rows read over the rain, though the panes' paper lets the wash through");
        assert_eq!(m.edge_gap, Some(11.0), "6 points of ground between a frame and the window's edge, as between two frames");
        assert!(m.ink_borders && m.glyph_motion && m.terminal_decode);
        assert_eq!(m.highlights, Highlights::Effect);
        assert_eq!(m.menus, MenuLook::Glow);
        assert_eq!((m.paper_alpha, m.chrome_alpha, m.active_tab_alpha, m.text_contrast), (0.6, 0.0, 0.0, 0.0), "the wash draws a pane's body through its paper and paints the active tab");
        assert_eq!(m.document_paper_alpha, Some(0.6), "a document reads on the terminal's paper, which the wash draws through");
        assert_eq!(m.document_paper_alpha(), 0.6);
        assert_eq!(m.glyph_rate(), 10);
        assert_eq!(m.glyphs.as_deref(), Some("glyphs.metal"));
    }

    #[test]
    fn an_effect_s_folder_is_imported_under_its_own_name_and_asks_before_replacing() {
        let (from, to) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let src = folder(from.path(), "neon", "name = \"neon\"\nwash = \"wash.metal\"\n", &["wash.metal"]);
        assert_eq!(import_folder(&src, to.path(), false), Ok("neon".to_string()));
        assert!(load("neon", to.path()).is_ok() && to.path().join("neon/wash.metal").is_file());
        assert_eq!(import_folder(&src, to.path(), false), Err("exists".to_string()));
        assert_eq!(import_folder(&src, to.path(), true), Ok("neon".to_string()));
        // A folder that is no effect is refused and nothing is written.
        let bad = folder(from.path(), "bad", "name = \"other\"\nwash = \"wash.metal\"\n", &["wash.metal"]);
        assert!(import_folder(&bad, to.path(), false).is_err());
        assert!(!to.path().join("bad").exists() && !to.path().join("other").exists());
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
            vec![("watercolor".to_string(), Source::BuiltIn), ("runic".to_string(), Source::BuiltIn), ("material".to_string(), Source::BuiltIn), ("notebook".to_string(), Source::BuiltIn), ("overworld".to_string(), Source::BuiltIn)]
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
