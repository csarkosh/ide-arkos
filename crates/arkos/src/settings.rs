//! The Settings window: every change written to config.toml in place through one
//! `config::set_values` and applied at once by whoever handed the window its `on_change` (the
//! project window's `reload_config`, or in the launch chooser the write alone). It shows what it
//! was last handed (`set_shown`), so an edit made in an editor shows here too.
use crate::chrome::{self, TITLE_BAR_H, UI_FONT};
use crate::config::{self, AppearanceMode, Config, EffectChoice, FontConfig, Look, PageWidth, StartAtLogin, WideTables};
use crate::theme::{self, Theme};
use crate::{aux, color_schemes, effects, font_sets, keymap, paths, telemetry_cli};
use ::telemetry::visits_app::{Crashes, Mode, Usage};
use gpui::{
    div, prelude::*, px, size, AnyElement, App, Bounds, ClipboardItem, Context, ElementId, FocusHandle, Hsla, KeyDownEvent, PathPromptOptions, SharedString,
    Stateful, TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions,
};
use std::path::{Path, PathBuf};
use objc2::rc::Retained;
use objc2_app_kit::NSWindow;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// Writes the edits to the config file and applies them; the window calls it for every change.
pub type OnChange = Rc<dyn Fn(&[(&[&str], toml_edit::Value)], &mut App)>;
/// The Updates section's "Update now".
pub type OnUpdate = Rc<dyn Fn(&mut App)>;

/// The window's title, which its own title bar draws.
const TITLE: &str = "Settings";
/// The window's opening size and its minimum. Each height is the content's plus the title bar.
const WIDTH: f32 = 560.0;
const HEIGHT: f32 = 480.0 + TITLE_BAR_H;
const MIN_WIDTH: f32 = 480.0;
const MIN_HEIGHT: f32 = 360.0 + TITLE_BAR_H;
/// The width of the section column.
const SECTIONS_W: f32 = 132.0;
/// The size stepper's range and the line height stepper's, with their increments.
const SIZE_RANGE: (f64, f64) = (10.0, 32.0);
const LINE_RANGE: (f64, f64) = (1.2, 2.0);
const SIZE_STEP: f64 = 1.0;
const LINE_STEP: f64 = 0.1;

/// Families that pass the advance check but draw symbols, and the font macOS draws missing glyphs
/// with: never offered as a text font.
const SYMBOL_FONTS: [&str; 5] = ["Wingdings", "Webdings", "Dingbats", "Symbol", "LastResort"];

/// The name fragments of the common fixed-pitch families, for `monospace_families`.
const MONO_HINTS: [&str; 26] = [
    "Mono", "Menlo", "Monaco", "Courier", "Code", "Consolas", "Hack", "Inconsolata", "Source Code", "IBM Plex Mono", "Fira Code", "JetBrains",
    "SF Mono", "Andale", "Ubuntu Mono", "Cascadia", "Iosevka", "Victor", "Hasklig", "Roboto Mono", "DejaVu Sans Mono", "Liberation Mono",
    "Noto Sans Mono", "PT Mono", "Space Mono", "Anonymous Pro",
];

/// The installed families whose advances for "i" and "W" agree, found once per run on the
/// background executor (loading every family takes a moment) and kept for later windows.
static FIXED_PITCH: OnceLock<Vec<String>> = OnceLock::new();
/// Set when the advance check starts, so a window opened while it runs does not start another.
static FIXED_PITCH_STARTED: AtomicBool = AtomicBool::new(false);

/// Whether this process's Settings window is the key window, set by its activation observer and
/// read by the project window (`Root::sync_pause`), whose effects keep moving while it is.
static SETTINGS_KEY: AtomicBool = AtomicBool::new(false);

/// Records whether the Settings window is the key window.
fn set_settings_key(key: bool) {
    SETTINGS_KEY.store(key, Ordering::Relaxed);
}

/// Whether this process's Settings window is the key window.
pub fn settings_is_key() -> bool {
    SETTINGS_KEY.load(Ordering::Relaxed)
}

/// Whether this window should start the advance check: true once per run, and never once its
/// result is in.
fn claim_fixed_pitch_check() -> bool {
    FIXED_PITCH.get().is_none() && !FIXED_PITCH_STARTED.swap(true, Ordering::SeqCst)
}

/// The corners of a pill and of a list row, and how far outside a control the keyboard's ring
/// is drawn.
const PILL_ROUND: f32 = 10.0;
const ROW_ROUND: f32 = 4.0;
const RING_OUT: f32 = 2.0;

/// What a control does when the keys press it.
type KeyAct = Rc<dyn Fn(&mut SettingsWindow, &mut Window, &mut Context<SettingsWindow>)>;

/// Where the Settings window's keys are: the sections' column, or the section's controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Zone {
    #[default]
    Sections,
    Controls,
}

/// The keys' place: the zone, the control ringed in the controls, and whether the ring shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Keys {
    pub zone: Zone,
    pub at: usize,
    pub shown: bool,
}

/// What a key does in the Settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsKey {
    /// The keys move: their new place, and the section shown.
    Move(Keys, usize),
    /// Press the control at this place.
    Press(usize),
    Close,
    Pass,
}

/// A key in the Settings window, as the IDE's keys go (Cyrus, 2026-11-17: "re-using existing
/// navigation rules"): in the sections' column Up and Down show the section before or after (as
/// the sidebar's switcher shows Sessions or Files), Right, Enter or Space go into its controls,
/// Escape closes the window; in the controls Up and Down move the ring through them (stopping at
/// either end), Enter or Space press the one ringed, Left or Escape go back to the column. The
/// first key that would move shows the ring where the keys are, and moves nothing.
pub fn settings_key(key: &str, keys: Keys, section: usize, sections: usize, stops: usize) -> SettingsKey {
    let moves = matches!(key, "up" | "down" | "left" | "right" | "enter" | "space");
    if !keys.shown && moves {
        return SettingsKey::Move(Keys { shown: true, ..keys }, section);
    }
    let at = keys.at.min(stops.saturating_sub(1));
    match (keys.zone, key) {
        (Zone::Sections, "up") => SettingsKey::Move(keys, section.saturating_sub(1)),
        (Zone::Sections, "down") => SettingsKey::Move(keys, (section + 1).min(sections - 1)),
        (Zone::Sections, "right" | "enter" | "space") if stops > 0 => SettingsKey::Move(Keys { zone: Zone::Controls, at: 0, shown: true }, section),
        (Zone::Sections, "escape") => SettingsKey::Close,
        (Zone::Controls, "up") => SettingsKey::Move(Keys { at: at.saturating_sub(1), ..keys }, section),
        (Zone::Controls, "down") => SettingsKey::Move(Keys { at: (at + 1).min(stops.saturating_sub(1)), ..keys }, section),
        (Zone::Controls, "enter" | "space") if stops > 0 => SettingsKey::Press(at),
        (Zone::Controls, "left" | "escape") => SettingsKey::Move(Keys { zone: Zone::Sections, ..keys }, section),
        _ => SettingsKey::Pass,
    }
}

/// The sections of the left column, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Appearance,
    Layout,
    Text,
    Startup,
    Updates,
    Privacy,
    Keyboard,
}

impl Section {
    const ALL: [Section; 7] = [Section::Appearance, Section::Layout, Section::Text, Section::Startup, Section::Updates, Section::Privacy, Section::Keyboard];

    fn label(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::Layout => "Layout",
            Section::Text => "Text",
            Section::Startup => "Startup",
            Section::Updates => "Updates",
            Section::Privacy => "Privacy",
            Section::Keyboard => "Keyboard",
        }
    }
}

/// What the window shows: the config in force, the config file's problem (which makes every
/// control read-only), why the last write was refused (the controls stay live), the version an
/// update would install, the text preset the project window uses (`None` before there is
/// one), and whether this Arkos is a sandbox.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub config: Config,
    pub error: Option<String>,
    pub write_error: Option<String>,
    pub update_ready: Option<String>,
    pub active_preset: Option<String>,
    pub sandbox: bool,
}

impl Shown {
    /// `self` with a refused write's reason, shown above the controls until the next push.
    pub fn with_write_error(self, e: String) -> Shown {
        Shown { write_error: Some(e), ..self }
    }

    /// Whether the controls take clicks: only a config file that does not load makes them
    /// read-only, since fixing the file is then the way forward.
    pub fn editable(&self) -> bool {
        self.error.is_none()
    }

    /// Whether Start at login, auto-update and "Update now" take clicks: as `editable`, but never in
    /// a sandbox, which writes no login item and never updates.
    pub fn machine_switches_live(&self) -> bool {
        self.editable() && !self.sandbox
    }
}

/// The Text section's presets: every name sorted, the chosen one, and its size and line height.
/// The chosen one is the project window's preset (`active`) when there is a project window, else
/// the config's default; a name that is not among the presets resolves as the project window
/// resolves it (`config::resolve_preset`), so the steppers edit the preset the terminals show.
pub fn preset_rows(cfg: &FontConfig, active: Option<&str>) -> (Vec<String>, String, f32, f32) {
    let names: Vec<String> = cfg.presets.keys().cloned().collect();
    let (chosen, p) = config::resolve_preset(active, &cfg.preset, &cfg.presets);
    (names, chosen, p.size, p.line_height)
}

/// The fixed-pitch families by name alone, sorted and without repeats: the fallback list, shown
/// until the advance check has run.
pub fn monospace_families(all: &[String]) -> Vec<String> {
    let mut out: Vec<String> = all.iter().filter(|n| offerable(n) && MONO_HINTS.iter().any(|h| n.contains(h))).cloned().collect();
    out.sort();
    out.dedup();
    out
}

/// Whether a family may be listed at all: not hidden (a leading dot) and not a symbol font.
fn offerable(name: &str) -> bool {
    !name.starts_with('.') && !SYMBOL_FONTS.iter().any(|s| name.contains(s))
}

/// `v` rounded to one decimal, so a stepped value written to the file reads 1.6, not
/// 1.600000023841858.
pub fn round_tenth(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// `v` rounded to four decimals: enough to undo an f32's noise (1.5499999523 back to 1.55)
/// without changing a value written by hand.
fn round_fine(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

/// One stepper press: `value` moved by `delta`, kept within `range`, rounded to a tenth. A value
/// outside the range is clamped by its first step.
fn step(value: f64, delta: f64, range: (f64, f64)) -> f64 {
    round_tenth((round_fine(value) + delta).clamp(range.0, range.1))
}

/// A stepper press on a preset: the new size and line height, written together. The one stepped
/// moves by `delta`; the other is written back as it is.
fn preset_step(size: f64, line_height: f64, stepping_size: bool, delta: f64) -> (f64, f64) {
    if stepping_size {
        (step(size, delta, SIZE_RANGE), round_fine(line_height))
    } else {
        (round_fine(size), step(line_height, delta, LINE_RANGE))
    }
}

/// The Updates section's line for a ready version.
fn ready_line(version: &str) -> String {
    format!("{version} is ready")
}

/// The row a configured name gets when it is not among `listed`: shown first, marked current,
/// never clickable.
fn not_installed(current: &str, listed: &[String]) -> Option<String> {
    (!listed.iter().any(|n| n == current)).then(|| format!("{current} (not installed)"))
}

/// The rows of the dark or the light theme list, as (index into `themes`, label): the themes of
/// that kind, every theme file that does not load (marked, so it is seen whichever list is
/// read), and the current pick when it is of the other kind, marked as such so it never
/// vanishes from its own list.
pub fn slot_rows(themes: &[(String, theme::Source, Option<theme::Appearance>)], slot: theme::Appearance, current: &str) -> Vec<(usize, String)> {
    themes
        .iter()
        .enumerate()
        .filter_map(|(ix, (name, source, kind))| match (source, kind) {
            (theme::Source::Invalid(_), _) | (_, None) => Some((ix, format!("{name} (invalid)"))),
            (_, Some(k)) if *k == slot => Some((ix, name.clone())),
            (_, Some(k)) if name == current => Some((ix, format!("{name} (a {} theme)", k.word()))),
            _ => None,
        })
        .collect()
}

/// A row of a preset list: one theme (its index in the list and its label), or a group of them
/// shown as one row with a choice of its members (each member's index and title), and the member
/// a click on the row picks: the current one when it is in the group, else the group's default,
/// else its first.
#[derive(Debug, Clone, PartialEq)]
pub enum PresetRow {
    One { ix: usize, label: String },
    Group { name: String, members: Vec<(usize, String)>, pick: usize },
}

/// A theme's name as Settings' theme lists show it: in lower case (Cyrus, 2026-11-17: "the
/// theme selection to lower case all the theme names"), "Material Navy" as "material navy". The
/// palette and the docs keep the titles as they are.
pub fn in_theme_list(name: &str) -> String {
    name.to_lowercase()
}

/// A group's row as one line of the list's text: the group, the member it shows, a caret.
pub fn group_line(group: &str, member: &str) -> String {
    format!("{group} {member} \u{25be}")
}

/// A preset group's row as one line of the list's text, from the title of the member it shows:
/// the title as it is, with the caret. A title starting with its group's word ("Watercolor
/// Lavender") already reads as group and member; one ending with it ("Seattle Rain") is the
/// member's own name, so the group's word is never put in front a second time.
pub fn preset_group_line(title: &str) -> String {
    format!("{title} \u{25be}")
}

/// A group member's name in its group's dropdown: its title without the group's word in front
/// ("Watercolor Lavender" is "Lavender" under Watercolor), else its whole title.
pub fn member_name(title: &str, group: &str) -> String {
    title.strip_prefix(group).map(str::trim_start).filter(|rest| !rest.is_empty()).unwrap_or(title).to_string()
}

/// `slot_rows` as a person reads them: each theme by its title, and the members of a group folded
/// into one row where its first member stood (Watercolor: Watercolor Lavender and Watercolor
/// Peach; Material: Material Bright, Material Solar and Material Lavender). A theme of the other
/// kind listed because it is current stays a row of its own, and so does a broken file.
/// Each group's last member in force, by group and slot (`pick_key`), so picking a theme of
/// another group (or none) leaves a group's row on what was chosen there before, and the dark
/// list's Material row and the light list's keep their own.
pub type GroupPicks = std::collections::BTreeMap<String, String>;

/// A group's key in the picks for the slot its row is in: "Material dark", "Watercolor light".
pub fn pick_key(group: &str, slot: theme::Appearance) -> String {
    format!("{group} {}", slot.word())
}

/// The group's remembered pick for the slot; a file written before the picks were kept by slot
/// has the group's name alone, which still counts.
fn picked<'a>(picks: &'a GroupPicks, group: &str, slot: theme::Appearance) -> Option<&'a String> {
    picks.get(&pick_key(group, slot)).or_else(|| picks.get(group))
}

/// Remembers `current`, in force in `slot`, as its group's pick there when it is a member of
/// one; true when that changed the picks (so they are worth writing).
pub fn remember_pick(picks: &mut GroupPicks, shown: &std::collections::BTreeMap<String, theme::Shown>, current: &str, slot: theme::Appearance) -> bool {
    let Some(group) = shown.get(current).and_then(|s| s.group.clone()) else { return false };
    let key = pick_key(&group, slot);
    if picks.get(&key).map(String::as_str) == Some(current) {
        return false;
    }
    picks.insert(key, current.to_string());
    true
}

/// The picks from their file: a line each, the group and the theme's name split by a tab; a
/// missing or unreadable file is no picks.
pub fn read_group_picks(path: &std::path::Path) -> GroupPicks {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(g, n)| (g.to_string(), n.to_string()))
        .collect()
}

/// Writes the picks to their file; a failure only costs the memory, so it is logged.
pub fn write_group_picks(path: &std::path::Path, picks: &GroupPicks) {
    let text: String = picks.iter().map(|(g, n)| format!("{g}\t{n}\n")).collect();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(path, text) {
        eprintln!("arkos: could not write {}: {e}", path.display());
    }
}

#[cfg(test)]
fn preset_list(themes: &[(String, theme::Source, Option<theme::Appearance>)], shown: &std::collections::BTreeMap<String, theme::Shown>, slot: theme::Appearance, current: &str) -> Vec<PresetRow> {
    preset_list_with(themes, shown, slot, current, &GroupPicks::new())
}

/// The slot's rows in alphabetical order (a group by its name, its members by theme), a current
/// pick of the other kind first and broken files last; a group folded into one whose pick is the
/// member in force, else the group's last pick (`picks`) while it is still installed, else its
/// default.
pub fn preset_list_with(themes: &[(String, theme::Source, Option<theme::Appearance>)], shown: &std::collections::BTreeMap<String, theme::Shown>, slot: theme::Appearance, current: &str, picks: &GroupPicks) -> Vec<PresetRow> {
    let title = |name: &str| shown.get(name).map_or_else(|| theme::title_case(name), |s| s.title.clone());
    let mut out: Vec<PresetRow> = Vec::new();
    for (ix, _) in slot_rows(themes, slot, current) {
        let (name, source, kind) = &themes[ix];
        let label = match (source, kind) {
            (theme::Source::Invalid(_), _) | (_, None) => format!("{} (invalid)", title(name)),
            (_, Some(k)) if *k != slot => format!("{} (a {} theme)", title(name), k.word()),
            _ => title(name),
        };
        let group = shown.get(name).and_then(|s| s.group.clone()).filter(|_| *kind == Some(slot) && !matches!(source, theme::Source::Invalid(_)));
        let Some(group) = group else {
            out.push(PresetRow::One { ix, label });
            continue;
        };
        // How strongly a member claims the row's pick: in force, then last picked, then default.
        let claim = |n: &str| {
            if n == current {
                3
            } else if picked(picks, &group, slot).is_some_and(|p| p == n) {
                2
            } else if shown.get(n).is_some_and(|s| s.group_default) {
                1
            } else {
                0
            }
        };
        match out.iter_mut().find(|r| matches!(r, PresetRow::Group { name: g, .. } if *g == group)) {
            Some(PresetRow::Group { members, pick, .. }) => {
                members.push((ix, label));
                if claim(name) > claim(&themes[*pick].0) {
                    *pick = ix;
                }
            }
            _ => out.push(PresetRow::Group { name: group, members: vec![(ix, label)], pick: ix }),
        }
    }
    // Where a row sorts: a current pick of the other kind first, broken files last, the rest
    // alphabetically by what the row reads.
    let place = |row: &PresetRow| -> (u8, String) {
        match row {
            PresetRow::One { ix, label } => {
                let (_, source, kind) = &themes[*ix];
                let rank = match (source, kind) {
                    (theme::Source::Invalid(_), _) | (_, None) => 2,
                    (_, Some(k)) if *k != slot => 0,
                    _ => 1,
                };
                (rank, label.to_lowercase())
            }
            PresetRow::Group { name, .. } => (1, name.to_lowercase()),
        }
    };
    for row in out.iter_mut() {
        if let PresetRow::Group { members, .. } = row {
            members.sort_by_key(|(_, label)| label.to_lowercase());
        }
    }
    out.sort_by_key(|row| place(row));
    out
}

/// One row of a list: its label, the value a click writes (`None`: not clickable), and whether
/// it is the choice in force.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub label: String,
    pub value: Option<String>,
    pub current: bool,
}

/// The custom look's Color list: every theme, the dark ones first, each saying which it is; a
/// file that does not load is marked and not clickable; a colour named that is not installed is
/// listed first, so the list always shows what is in force.
pub fn color_rows(colors: &[theme::ColorEntry], current: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    if !colors.iter().any(|e| e.name == current) {
        rows.push(Row { label: format!("{current} (not installed)"), value: None, current: true });
    }
    for slot in [Some(theme::Appearance::Dark), Some(theme::Appearance::Light), None] {
        for e in colors {
            let broken = matches!(e.source, theme::Source::Invalid(_)) || e.appearance.is_none();
            match (slot, broken) {
                (None, true) => rows.push(Row { label: format!("{} (invalid)", e.title), value: None, current: e.name == current }),
                (Some(k), false) if e.appearance == Some(k) => rows.push(Row { label: format!("{} ({})", e.title, k.word()), value: Some(e.name.clone()), current: e.name == current }),
                _ => {}
            }
        }
    }
    rows
}

/// A row of the custom look's Effect list: a plain row, or an effect with several scenes shown as
/// one group row with a choice of its scenes (as the Presets lists show Watercolor).
#[derive(Debug, Clone, PartialEq)]
pub enum EffectRow {
    One(Row),
    /// `members` are the scenes as (name, title); `pick` is the name of the one the row shows.
    Group { effect: String, title: String, members: Vec<(String, String)>, pick: String, current: bool },
}

/// The key a scene effect's last pick is remembered under in the group picks.
pub fn effect_pick_key(effect: &str) -> String {
    format!("effect:{effect}")
}

/// The edits a pick of `scene` under `effect` writes.
pub fn effect_pick_edits(effect: &str, scene: &str) -> Vec<Edit> {
    vec![edit(&["custom", "effect"], effect), edit(&["custom", "scene"], scene)]
}

/// Remembers `scene` as `effect`'s last pick; true when that changed the picks.
pub fn remember_scene_pick(picks: &mut GroupPicks, effect: &str, scene: &str) -> bool {
    picks.insert(effect_pick_key(effect), scene.to_string()).as_deref() != Some(scene)
}

/// The custom look's Effect list: "None", then every effect installed, built in or a folder; one
/// that does not load is marked and not clickable; one named that is not installed is listed
/// last. `choice` is the custom look's own, and `theme` is the colour's own effect (`own`).
/// An effect with two or more scenes (`manifests`, by effect: each scene's name and title) is one
/// group row titled by the effect; when it is the effect in force its row shows `own_scene` (an old
/// name resolved by `aliases`, as "effect/old" to the scene's own), else its first scene, and
/// otherwise the scene last picked (`picks`), else its first.
pub fn custom_effect_rows(choice: &EffectChoice, own: Option<&str>, own_scene: Option<&str>, listed: &[(String, effects::manifest::Source)], manifests: &[(String, Vec<(String, String)>)], aliases: &std::collections::BTreeMap<String, String>, picks: &GroupPicks) -> Vec<EffectRow> {
    let named: Option<&str> = match choice {
        EffectChoice::None => None,
        EffectChoice::Theme => own,
        EffectChoice::Named(name) => Some(name),
    };
    let mut rows = vec![EffectRow::One(Row { label: "None".into(), value: Some("none".into()), current: named.is_none() })];
    for (name, source) in listed {
        let current = named == Some(name.as_str());
        let scenes = manifests.iter().find(|(n, _)| n == name).map(|(_, s)| s).filter(|s| s.len() >= 2);
        rows.push(match (source, scenes) {
            (effects::manifest::Source::Invalid(_), _) => EffectRow::One(Row { label: format!("{name} (invalid)"), value: None, current }),
            (_, Some(members)) => {
                let has = |scene: &str| members.iter().any(|(n, _)| n == scene).then(|| scene.to_string());
                let remembered = picks.get(&effect_pick_key(name)).and_then(|p| has(p));
                // The row in force names the scene drawn: the one in force by its own name, else the
                // first, as the wash's scene number does. Only a row not in force remembers a pick.
                let pick = if current {
                    own_scene.map(|s| aliases.get(&format!("{name}/{s}")).map(String::as_str).unwrap_or(s)).and_then(has).unwrap_or_else(|| members[0].0.clone())
                } else {
                    remembered.unwrap_or_else(|| members[0].0.clone())
                };
                let mut title = name.chars();
                let title = title.next().map(|f| f.to_uppercase().chain(title).collect::<String>()).unwrap_or_default();
                EffectRow::Group { effect: name.clone(), title, members: members.clone(), pick, current }
            }
            _ => EffectRow::One(Row { label: name.clone(), value: Some(name.clone()), current }),
        });
    }
    if let Some(name) = named.filter(|n| !listed.iter().any(|(l, _)| l == n)) {
        rows.push(EffectRow::One(Row { label: format!("{name} (not installed)"), value: None, current: true }));
    }
    rows
}

/// Where an open group's card lives, so a Presets group and the Runic group never both count as
/// open: a Presets list's slot and its group, or an effect of Custom's Effect list.
#[derive(Debug, Clone, PartialEq)]
enum GroupKey {
    Presets(theme::Appearance, String),
    Effect(String),
}

/// The scenes of each effect that has two or more, as (name, title), read from its manifest.
fn effect_scenes(listed: &[(String, effects::manifest::Source)]) -> Vec<(String, Vec<(String, String)>)> {
    listed
        .iter()
        .filter(|(_, source)| !matches!(source, effects::manifest::Source::Invalid(_)))
        .filter_map(|(name, _)| {
            let m = effects::manifest::load(name, &effects::effects_dir()).ok()?.manifest;
            (m.scenes.len() >= 2).then(|| (name.clone(), m.scenes.iter().enumerate().map(|(i, s)| (s.clone(), m.scene_title(i))).collect()))
        })
        .collect()
}

/// The old scene names of the effects with scenes, as "effect/old" to the scene's own name.
fn scene_aliases(listed: &[(String, effects::manifest::Source)]) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for (name, source) in listed {
        if let (false, Ok(e)) = (matches!(source, effects::manifest::Source::Invalid(_)), effects::manifest::load(name, &effects::effects_dir())) {
            out.extend(e.manifest.scene_aliases.iter().map(|(old, new)| (format!("{name}/{old}"), new.clone())));
        }
    }
    out
}

/// One member of a group row's card: what it is called, whether it is in force, what a pick of it
/// writes, and the pick to remember (effect and scene) when it is chosen.
struct GroupMember {
    id: usize,
    title: String,
    current: bool,
    edits: Vec<Edit>,
    remember: Option<(String, String)>,
}

/// A group row and its card: the row's one line of text, whether the group is in force, and its
/// members. `anchor` numbers the row's and the card's element ids.
struct GroupSpec {
    list_id: &'static str,
    key: GroupKey,
    group: String,
    anchor: usize,
    line: String,
    is_current: bool,
    members: Vec<GroupMember>,
}

/// The families a font set names, each once, in the order prose, UI, terminal:
/// "Baskerville, Optima, PT Mono".
pub fn families(font: &theme::ThemeFont) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for f in [&font.prose, &font.ui, &font.terminal].into_iter().flatten() {
        if !out.contains(&f.as_str()) {
            out.push(f);
        }
    }
    out
}

/// The custom look's Font list: "Default" (the config's own family, `plain`), then each distinct
/// set of fonts the themes name, by its families, under the first theme that names it. `choice`
/// is the custom look's own (`custom.font`); with none the fonts in force are `shown`, those of
/// the theme showing. A set none of whose families `has` finds is marked and not clickable; a
/// theme named that is not installed is listed first, so the list always shows what is in force.
pub fn font_rows(fonts: &[theme::FontEntry], choice: Option<&str>, shown: &theme::ThemeFont, plain: &str, has: &dyn Fn(&str) -> bool) -> Vec<Row> {
    let none = theme::ThemeFont::default();
    let in_force: Option<&theme::ThemeFont> = match choice {
        None => Some(shown),
        Some(config::PLAIN_FONT) => Some(&none),
        Some(name) => fonts.iter().find(|e| e.name == name).map(|e| &e.font),
    };
    let mut rows = Vec::new();
    if let (None, Some(name)) = (in_force, choice) {
        rows.push(Row { label: format!("{name} (not installed)"), value: None, current: true });
    }
    rows.push(Row { label: format!("Default ({plain})"), value: Some(config::PLAIN_FONT.into()), current: in_force.is_some_and(|f| f.is_default()) });
    for e in fonts {
        if e.font.is_default() {
            continue;
        }
        let current = match choice {
            Some(name) => e.name == name,
            None => in_force == Some(&e.font),
        };
        rows.push(if families(&e.font).iter().any(|f| has(f)) {
            Row { label: e.title.clone(), value: Some(e.name.clone()), current }
        } else {
            Row { label: format!("{} (not installed)", e.title), value: None, current }
        });
    }
    rows
}

/// What `custom.font` says for the fonts of the theme showing: its name, or with no fonts of
/// its own, the plain ones' word.
fn font_word(shown: &str, plain_font: bool) -> String {
    if plain_font { config::PLAIN_FONT } else { shown }.to_string()
}

/// What a click on a Color row writes: the colour, and the first time, the fonts on screen as
/// the custom look's font (`shown` the theme showing, `plain_font` whether it names no fonts),
/// so a new colour changes the colours alone.
pub fn color_edits(cfg: &Config, color: &str, shown: &str, plain_font: bool) -> Vec<(Vec<&'static str>, String)> {
    let mut edits = vec![(vec!["custom", "color"], color.to_string())];
    if cfg.custom.font.is_none() {
        edits.push((vec!["custom", "font"], font_word(shown, plain_font)));
    }
    edits
}

/// What a click on a look's pill writes: the look itself, and for the custom look, the first
/// time, what is on screen as its colour, its font and its effect (`shown` the theme showing,
/// `plain_font` whether it names no fonts, `own` its own effect, `scene` the scene it shows when
/// that effect has scenes), so putting it in force changes
/// nothing until something is picked.
pub fn look_edits(cfg: &Config, which: Look, shown: &str, plain_font: bool, own: Option<&str>, scene: Option<&str>) -> Vec<(Vec<&'static str>, String)> {
    let mut edits = vec![(vec!["appearance", "look"], if which == Look::Presets { "presets" } else { "custom" }.to_string())];
    if which == Look::Custom {
        if cfg.custom.color.is_none() {
            edits.push((vec!["custom", "color"], shown.to_string()));
            // With a colour already chosen the theme showing may be a preset, not that colour:
            // its font is pinned when a colour is next picked (`color_edits`).
            if cfg.custom.font.is_none() {
                edits.push((vec!["custom", "font"], font_word(shown, plain_font)));
            }
        }
        if cfg.custom.effect.is_none() {
            // The scene pinned goes with the effect taken from the screen: an effect already
            // chosen keeps resolving its own scene (`look_scene`), which the preset showing must
            // not overwrite.
            // Under the presets the effect on screen is the preset's own; an older config's
            // `effect.name` says otherwise.
            let on_screen = match (cfg.look(), &cfg.effect.name) {
                (Look::Custom, EffectChoice::None) => "none".to_string(),
                (Look::Custom, EffectChoice::Named(name)) => name.clone(),
                _ => own.unwrap_or("none").to_string(),
            };
            edits.push((vec!["custom", "effect"], on_screen));
            if cfg.custom.scene.is_none() {
                if let Some(scene) = scene {
                    edits.push((vec!["custom", "scene"], scene.to_string()));
                }
            }
        }
    }
    edits
}

/// Which `[theme]` key an imported theme is selected under: the mode's, or with the mode System,
/// the one for what macOS shows now.
pub fn theme_key_for(mode: AppearanceMode, system_is_dark: bool) -> &'static str {
    match (mode, system_is_dark) {
        (AppearanceMode::Dark, _) | (AppearanceMode::System, true) => "dark",
        (AppearanceMode::Light, _) | (AppearanceMode::System, false) => "light",
    }
}

/// The installed families whose "i" and "W" advance the same, sorted. A family gpui cannot load
/// resolves to a fallback, which is told apart by its name and left out.
fn fixed_pitch(text: &gpui::TextSystem, all: &[String]) -> Vec<String> {
    let mut out: Vec<String> = all
        .iter()
        .filter(|name| offerable(name))
        .filter(|name| {
            let id = text.resolve_font(&gpui::font(name.to_string()));
            if text.get_font_for_id(id).is_none_or(|f| f.family.as_ref() != name.as_str()) {
                return false;
            }
            match (text.advance(id, px(16.0), 'i'), text.advance(id, px(16.0), 'W')) {
                (Ok(i), Ok(w)) => i.width == w.width,
                _ => false,
            }
        })
        .cloned()
        .collect();
    out.sort();
    out.dedup();
    out
}

/// The Settings window's view. One at most, kept in `aux::Aux`.
pub struct SettingsWindow {
    shown: Shown,
    section: Section,
    themes: Vec<(String, theme::Source, Option<theme::Appearance>)>,
    /// How each theme is shown: its title and its group (`theme::shown`).
    theme_shown: std::collections::BTreeMap<String, theme::Shown>,
    /// Each group's last member in force (`paths::group_picks_file`).
    group_picks: GroupPicks,
    /// The preset group whose members are listed open under its row, and in which list.
    open_group: Option<GroupKey>,
    /// The scenes each installed effect with two or more offers, by effect: name and title.
    effect_scenes: Vec<(String, Vec<(String, String)>)>,
    /// Those effects' old scene names, as "effect/old" to the scene's own name.
    scene_aliases: std::collections::BTreeMap<String, String>,
    /// When a click outside it last closed a group's menu: a click on the group's own row closes
    /// it that way first, and must not open it again.
    group_closed_at: Option<std::time::Instant>,
    /// The effects folder and the built-ins, as `effects::manifest::list` reads them.
    effects: Vec<(String, effects::manifest::Source)>,
    /// Every theme that loads with its fonts (`theme::fonts`), for the custom look's Font list.
    theme_fonts: Vec<theme::FontEntry>,
    theme_colors: Vec<theme::ColorEntry>,
    /// Every family installed, to mark a font set this Mac has none of.
    families: std::collections::HashSet<String>,
    fonts: Vec<String>,
    /// Why the last "Add …" failed, shown under the buttons.
    import_error: Option<String>,
    /// A picked theme file or effect folder whose name is already taken, that name and what it
    /// was picked as, while the window asks whether to replace it.
    pending_replace: Option<(PathBuf, String, Adding)>,
    /// The file picker is up.
    picking: bool,
    focus: FocusHandle,
    /// Where the keys are (Cyrus, 2026-11-17: the Settings window "must support keyboard
    /// navigation"): the sections' column or the section's controls, which control, and whether
    /// the ring shows yet (from the first key that moves it).
    keys: Keys,
    /// The section's controls the keys can reach, in the order they are drawn, each with what a
    /// press does: collected afresh by every render (`act`).
    stops: std::cell::RefCell<Vec<KeyAct>>,
    /// The controls' scroll, so the ringed control is kept in view.
    scroll: gpui::ScrollHandle,
    /// Set by a key that moves the ring; the ringed control scrolls itself into view once.
    follow: Rc<std::cell::Cell<bool>>,
    theme: Theme,
    on_change: OnChange,
    on_update: OnUpdate,
    /// The Privacy section's rows read from the folder, kept between renders.
    privacy: PrivacyCache,
}

/// What an "Add …" button of the Appearance section adds: a preset for the slot showing, a
/// colour for the custom look, or an effect for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adding {
    Preset,
    Color,
    Effect,
}

/// Opens the Settings window centred on the IDE window `over` (on the main display when there
/// is none yet, from the chooser) and gives it the keyboard. `fonts` is the list shown until the
/// advance check has run. `None` when gpui could not open it, which the log records.
pub fn open(cx: &mut App, over: Option<Retained<NSWindow>>, shown: Shown, theme: Theme, fonts: Vec<String>, on_change: OnChange, on_update: OnUpdate) -> Option<WindowHandle<SettingsWindow>> {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(WIDTH), px(HEIGHT)), cx))),
        // The window draws the IDE's own title bar in the native one's place.
        titlebar: Some(TitlebarOptions { title: Some(TITLE.into()), appears_transparent: true, traffic_light_position: None }),
        focus: true,
        is_resizable: true,
        window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
        ..Default::default()
    };
    let opened = cx.open_window(options, move |window, cx| {
        centre_over(window, over.as_deref());
        let view = cx.new(|cx| {
            // Foreground time in Settings counts while its window is the key window, and stops
            // when it closes.
            crate::telemetry::settings_focus(true);
            // Coming to the front reads the Privacy rows again: the daemon may have sent a report
            // meanwhile.
            cx.observe_window_activation(window, |w: &mut SettingsWindow, window, cx| {
                let active = window.is_window_active();
                crate::telemetry::settings_focus(active);
                // Settings in front keeps the project window's effects moving, so its preview
                // does: the project window renders again and reads this (`Root::sync_pause`).
                set_settings_key(active);
                cx.defer(aux::poke_root);
                if active {
                    w.privacy.forget();
                    cx.notify();
                }
            })
            .detach();
            cx.on_release(|_, cx| {
                crate::telemetry::settings_focus(false);
                set_settings_key(false);
                cx.defer(aux::poke_root);
            })
            .detach();
            let fonts = FIXED_PITCH.get().filter(|f| !f.is_empty()).cloned().unwrap_or(fonts);
            if claim_fixed_pitch_check() {
                let text = cx.text_system().clone();
                cx.spawn(async move |_, cx| {
                    let found = cx.background_executor().spawn(async move { fixed_pitch(&text, &text.all_font_names()) }).await;
                    let found = FIXED_PITCH.get_or_init(|| found).clone();
                    // An empty result means the check found nothing it could measure: keep the
                    // names. The result goes to the Settings window open when it lands, which
                    // may be a later one than the window that started the check.
                    if !found.is_empty() {
                        let _ = cx.update(|cx| {
                            let Some(handle) = cx.try_global::<aux::Aux>().and_then(|aux| aux.settings) else { return };
                            let _ = handle.update(cx, |w, _window, cx| {
                                w.fonts = found;
                                cx.notify();
                            });
                        });
                    }
                })
                .detach();
            }
            SettingsWindow {
                shown,
                section: Section::Appearance,
                themes: theme::kinds(&paths::themes_dir()),
                theme_shown: theme::shown(&paths::themes_dir()),
                group_picks: read_group_picks(&paths::group_picks_file()),
                open_group: None,
                effect_scenes: effect_scenes(&effects::manifest::list(&effects::effects_dir())),
                scene_aliases: scene_aliases(&effects::manifest::list(&effects::effects_dir())),
                group_closed_at: None,
                effects: effects::manifest::list(&effects::effects_dir()),
                theme_fonts: theme::font_sets(&paths::themes_dir()),
                theme_colors: theme::colors(&paths::themes_dir()),
                families: cx.text_system().all_font_names().into_iter().collect(),
                fonts,
                import_error: None,
                pending_replace: None,
                picking: false,
                focus: cx.focus_handle(),
                keys: Keys::default(),
                stops: Default::default(),
                scroll: gpui::ScrollHandle::new(),
                follow: Rc::new(std::cell::Cell::new(false)),
                theme,
                on_change,
                on_update,
                privacy: PrivacyCache::default(),
            }
        });
        window.focus(&view.read(cx).focus);
        view
    });
    match opened {
        Ok(handle) => Some(handle),
        Err(e) => {
            eprintln!("arkos: the Settings window did not open: {e:#}");
            None
        }
    }
}

/// Moves a window that has just opened (Settings or Keybindings) to the centre of the IDE window
/// `over`. It has not drawn yet, so it is first seen there.
pub fn centre_over(window: &Window, over: Option<&NSWindow>) {
    let Some(own) = crate::screen::ns_window_of(window) else { return };
    if let Some(over) = over {
        crate::screen::centre_on(&own, over);
    }
    // The screenshot shoot floats the IDE window (ARKOS_WINDOW_LEVEL=floating); a window opened
    // over it floats too, or it would open hidden behind it.
    if std::env::var_os("ARKOS_WINDOW_LEVEL").is_some_and(|v| v == "floating") {
        own.setLevel(objc2_app_kit::NSFloatingWindowLevel);
    }
}

/// An edit the window owns until the deferred call borrows it for `on_change`.
type Edit = (Vec<String>, toml_edit::Value);

fn edit(keys: &[&str], value: impl Into<toml_edit::Value>) -> Edit {
    (keys.iter().map(|k| k.to_string()).collect(), value.into())
}

/// The edit for the "Wide tables" row: `viewer.wide_tables` as the config spells it.
fn wide_tables_edit(mode: WideTables) -> Edit {
    edit(&["viewer", "wide_tables"], match mode {
        WideTables::Wrap => "wrap",
        WideTables::Scroll => "scroll",
    })
}

/// The edit for the "Page width" row: `viewer.page_width` as the config spells it.
fn page_width_edit(width: PageWidth) -> Edit {
    edit(&["viewer", "page_width"], match width {
        PageWidth::Full => "full",
        PageWidth::Letter => "letter",
    })
}

/// The Privacy section's rows (telemetry spec, section 3.4), named as `docs/telemetry.md` names
/// them.
const USAGE_ROW: &str = "Usage reports";
const CRASHES_ROW: &str = "Crash reports";
const WHAT_IS_SENT_ROW: &str = "What Arkos sends";
const LAST_REPORT_ROW: &str = "Last report";
const REPORTS_ROW: &str = "Reports";

/// The Usage reports row's choices, in order.
const USAGE_CHOICES: [(&str, Usage); 3] = [("Off", Usage::Off), ("Local only", Usage::Local), ("On", Usage::On)];
/// The Crash reports row's choices, in order.
const CRASH_CHOICES: [(&str, Crashes); 3] = [("Never", Crashes::Never), ("Ask each time", Crashes::Ask), ("Always", Crashes::Always)];

/// The edit for the "Usage reports" row: `telemetry.usage` as the config spells it. Written like
/// any other setting; the project window's reload hands a change to `on` to the Recorder, which
/// deletes the session reports queued before the consent (the opt-in rule), as the daemon does.
fn usage_edit(usage: Usage) -> Edit {
    edit(&["telemetry", "usage"], ::telemetry::usage_name(usage))
}

/// The edit for the "Crash reports" row: `telemetry.crashes` as the config spells it.
fn crashes_edit(crashes: Crashes) -> Edit {
    edit(&["telemetry", "crashes"], ::telemetry::crashes_name(crashes))
}

/// The line under Usage reports: what the mode means, or the rule that forces it, in the words
/// `arkos telemetry status` and the disclosure page use.
fn usage_line(mode: &Mode) -> String {
    telemetry_cli::mode_words(mode.reason, ::telemetry::usage_name(mode.usage))
}

/// The mode the shown config gives this app: a person's, so interactive. A config file that does
/// not load is telemetry `Off`, never the defaults.
fn shown_mode(shown: &Shown) -> Mode {
    let config = if shown.error.is_some() { Err(()) } else { Ok(&shown.config.telemetry) };
    ::telemetry::mode_for(config, true)
}

/// The session the daemon sealed last, `last-session.json` in the telemetry folder `dir`, when
/// there is one. Only a plain file counts: a folder or a link in its place is not the daemon's.
fn last_report(dir: &Path) -> Option<PathBuf> {
    let file = dir.join(telemetry_cli::LAST_SESSION);
    file.symlink_metadata().is_ok_and(|m| m.is_file()).then_some(file)
}

/// One line of the Reports list: the label it shows ("date  inc"), the `inc` a click copies,
/// and what became of the report.
#[derive(Clone, PartialEq, Eq, Debug)]
struct ReportLine {
    label: String,
    inc: String,
    status: &'static str,
}

/// The Reports list's lines: each crash report by date, newest first, with its `inc` and what
/// became of it (`crash_dialog::reports`).
fn report_lines(dir: &Path) -> Vec<ReportLine> {
    crate::crash_dialog::reports(dir).into_iter().map(|r| ReportLine { label: format!("{}  {}", r.date, r.inc), inc: r.inc, status: r.status }).collect()
}

/// What the Privacy section shows that is read from the telemetry folder and the environment:
/// the line under Usage reports, the last sealed session, and the Reports list.
#[derive(Clone, Debug, PartialEq)]
struct PrivacyRows {
    usage_line: String,
    last: Option<PathBuf>,
    lines: Vec<ReportLine>,
}

/// The Privacy rows as last read: read when the section is shown, and again only after `forget`
/// (a new config, the window coming to the front, a crash report decided), never per render.
#[derive(Default)]
struct PrivacyCache(Option<PrivacyRows>);

impl PrivacyCache {
    /// The rows, read from the telemetry folder `dir` and `shown` when there are none yet.
    fn get(&mut self, dir: &Path, shown: &Shown) -> &PrivacyRows {
        self.0.get_or_insert_with(|| PrivacyRows { usage_line: usage_line(&shown_mode(shown)), last: last_report(dir), lines: report_lines(dir) })
    }

    /// Something they are read from changed: the next showing reads them again.
    fn forget(&mut self) {
        self.0 = None;
    }
}

impl SettingsWindow {
    /// The telemetry folder changed under the Privacy section (a crash report was decided): its
    /// rows are read again.
    pub fn reports_changed(&mut self, cx: &mut Context<Self>) {
        self.privacy.forget();
        cx.notify();
    }

    /// Redraws the window in the project window's new theme.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    /// Shows a newly loaded config (or its error) and the themes and effects folders as they are
    /// now.
    pub fn set_shown(&mut self, shown: Shown, cx: &mut Context<Self>) {
        self.shown = shown;
        self.privacy.forget();
        self.themes = theme::kinds(&paths::themes_dir());
        self.theme_shown = theme::shown(&paths::themes_dir());
        self.theme_fonts = theme::font_sets(&paths::themes_dir());
        self.theme_colors = theme::colors(&paths::themes_dir());
        self.effects = effects::manifest::list(&effects::effects_dir());
        self.effect_scenes = effect_scenes(&self.effects);
        self.scene_aliases = scene_aliases(&self.effects);
        cx.notify();
    }

    /// Hands `edits` to `on_change` once this update is over, so the push back into this window
    /// that follows the write is not refused.
    fn change(&self, edits: Vec<Edit>, cx: &mut Context<Self>) {
        let on_change = self.on_change.clone();
        cx.defer(move |cx| {
            let keys: Vec<Vec<&str>> = edits.iter().map(|(k, _)| k.iter().map(String::as_str).collect()).collect();
            let borrowed: Vec<(&[&str], toml_edit::Value)> = keys.iter().zip(&edits).map(|(k, (_, v))| (k.as_slice(), v.clone())).collect();
            on_change(&borrowed, cx);
        });
    }

    /// "Add preset…", "Add color…" and "Add effect…": the native picker for one theme file, or
    /// for one effect's folder; what is chosen is imported.
    fn add(&mut self, adding: Adding, system_is_dark: bool, cx: &mut Context<Self>) {
        if self.picking {
            return;
        }
        self.picking = true;
        let folder = adding == Adding::Effect;
        let prompt = match adding {
            Adding::Preset => "Add preset",
            Adding::Color => "Add color",
            Adding::Effect => "Add effect",
        };
        let rx = cx.prompt_for_paths(PathPromptOptions { files: !folder, directories: folder, multiple: false, prompt: Some(prompt.into()) });
        cx.spawn(async move |view, cx| {
            let picked = rx.await;
            let _ = view.update(cx, |w, cx| {
                w.picking = false;
                match picked {
                    Ok(Ok(Some(paths))) => {
                        if let Some(src) = paths.into_iter().next() {
                            w.import(src, adding, false, system_is_dark, cx);
                        }
                    }
                    Ok(Err(e)) => w.import_error = Some(format!("cannot show the file picker: {e}")),
                    _ => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Copies a theme file into the themes folder, or an effect's folder into the effects
    /// folder, and selects it: a preset for the mode showing, a colour or an effect for the
    /// custom look. A name already there asks first.
    fn import(&mut self, src: PathBuf, adding: Adding, replace: bool, system_is_dark: bool, cx: &mut Context<Self>) {
        self.pending_replace = None;
        self.import_error = None;
        let (copied, named) = match adding {
            Adding::Effect => (effects::manifest::import_folder(&src, &effects::effects_dir(), replace), effects::manifest::load_folder(&src).map(|e| e.manifest.name)),
            Adding::Color => (color_schemes::import_file(&src, &paths::themes_dir(), replace).map(|(name, _)| name), theme::validate_file(&src).or_else(|e| color_schemes::parse(&std::fs::read_to_string(&src).unwrap_or_default()).map(|s| s.name).map_err(|_| e))),
            _ => (theme::import_file(&src, &paths::themes_dir(), replace).map(|(name, _)| name), theme::validate_file(&src)),
        };
        match copied {
            Ok(name) => {
                self.themes = theme::kinds(&paths::themes_dir());
                self.theme_shown = theme::shown(&paths::themes_dir());
                self.theme_fonts = theme::font_sets(&paths::themes_dir());
                self.theme_colors = theme::colors(&paths::themes_dir());
                self.effects = effects::manifest::list(&effects::effects_dir());
                self.effect_scenes = effect_scenes(&self.effects);
                self.scene_aliases = scene_aliases(&self.effects);
                let edits = match adding {
                    Adding::Preset => vec![edit(&["theme", theme_key_for(self.shown.config.appearance.mode, system_is_dark)], name.as_str())],
                    Adding::Color => color_edits(&self.shown.config, &name, &self.theme.name, self.theme.font.is_default()).into_iter().map(|(keys, value)| edit(&keys, value.as_str())).collect(),
                    Adding::Effect => vec![edit(&["custom", "effect"], name.as_str())],
                };
                self.change(edits, cx);
            }
            Err(e) if e == "exists" => match named {
                Ok(name) => self.pending_replace = Some((src, name, adding)),
                Err(e) => self.import_error = Some(e),
            },
            Err(e) => self.import_error = Some(e),
        }
        cx.notify();
    }
}

/// The theme's chrome colours, as the controls use them.
#[derive(Clone, Copy)]
pub struct Colors {
    pub text: Hsla,
    pub dim: Hsla,
    pub border: Hsla,
    pub accent: Hsla,
    pub pill: Hsla,
    /// A floating card's ground (a dropdown's menu): the window's own.
    pub card: Hsla,
}

impl Colors {
    /// The controls' colours in `theme`.
    pub fn of(theme: &Theme) -> Colors {
        let t = &theme.chrome;
        Colors { text: t.text.hsla(), dim: t.text_dim.hsla(), border: t.border.hsla(), accent: t.focus_ring.hsla(), pill: t.banner.hsla(), card: t.window_bg.hsla() }
    }
}

/// Opens the config file in the default text editor (`open -t`), off the main thread. The
/// Keyboard section's "Open config" and the Keybindings window's run it.
pub fn open_config_in_editor(cx: &App) {
    let path = paths::config_file();
    cx.background_executor()
        .spawn(async move {
            if let Err(e) = std::process::Command::new("open").arg("-t").arg(&path).spawn() {
                eprintln!("arkos: cannot open {}: {e}", path.display());
            }
        })
        .detach();
}

/// Scrolls `scroll` so a control at `b` (drawn at its scrolled place) shows whole in it.
fn keep_in_view(scroll: &gpui::ScrollHandle, b: gpui::Bounds<gpui::Pixels>, window: &mut Window) {
    let view = scroll.bounds();
    let mut offset = scroll.offset();
    if b.top() < view.top() {
        offset.y += view.top() - b.top();
    } else if b.bottom() > view.bottom() {
        offset.y -= b.bottom() - view.bottom();
    } else {
        return;
    }
    scroll.set_offset(offset);
    window.refresh();
}

/// A pill button, drawn like the terminal's "jump to bottom" pill. A selected one is outlined in
/// the accent colour; a disabled one is dimmed and the caller attaches no click handler.
pub fn pill(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool, enabled: bool, c: Colors) -> Stateful<gpui::Div> {
    let p = div()
        .id(id)
        .flex_none()
        .px(px(10.0))
        .py(px(3.0))
        .rounded(px(PILL_ROUND))
        .border_1()
        .text_size(px(12.0))
        .text_color(if enabled { c.text } else { c.dim })
        .child(label.into());
    let p = if selected { p.bg(c.pill).border_color(c.accent) } else { p.border_color(c.border) };
    if enabled {
        p.cursor_pointer()
    } else {
        p
    }
}

/// A list row: a check mark column, then the name.
fn list_row(id: impl Into<ElementId>, label: impl Into<SharedString>, current: bool, enabled: bool, c: Colors) -> Stateful<gpui::Div> {
    let r = div()
        .id(id)
        .flex()
        .gap(px(6.0))
        .px(px(6.0))
        .py(px(2.0))
        .rounded(px(ROW_ROUND))
        .text_color(if enabled { c.text } else { c.dim })
        .child(div().w(px(14.0)).flex_none().child(if current { "✓" } else { "" }))
        .child(label.into());
    if enabled && !current {
        r.cursor_pointer().hover(move |s| s.bg(c.pill))
    } else {
        r
    }
}

/// A control's label above it.

fn heading(text: &'static str, c: Colors) -> gpui::Div {
    div().pt(px(6.0)).text_color(c.dim).text_size(px(12.0)).child(text)
}

/// A number as the steppers show it, as it is in the file: "16", "1.55".
fn number(v: f64) -> String {
    round_fine(v).to_string()
}

impl SettingsWindow {
    /// `el` as a control the keys reach: a click runs `f`, and so does Enter on it when ringed.
    fn act(&self, el: Stateful<gpui::Div>, cx: &mut Context<Self>, f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static) -> Stateful<gpui::Div> {
        self.act_shaped(el, PILL_ROUND, cx, f)
    }

    /// `act` for a list row, its ring following the row's corners.
    fn act_row(&self, el: Stateful<gpui::Div>, cx: &mut Context<Self>, f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static) -> Stateful<gpui::Div> {
        self.act_shaped(el, ROW_ROUND, cx, f)
    }

    fn act_shaped(&self, el: Stateful<gpui::Div>, round: f32, cx: &mut Context<Self>, f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static) -> Stateful<gpui::Div> {
        let f: KeyAct = Rc::new(f);
        let g = f.clone();
        self.key_only(el.on_click(cx.listener(move |w, _, window, cx| g(w, window, cx))), round, f)
    }

    /// `el` as a stop of the keys whose press runs `f`, the click left as it is (a row that acts on
    /// the press). Ringed while the keys are on it, and kept in view after a key moves to it.
    fn key_only(&self, el: Stateful<gpui::Div>, round: f32, f: KeyAct) -> Stateful<gpui::Div> {
        let i = {
            let mut stops = self.stops.borrow_mut();
            stops.push(f);
            stops.len() - 1
        };
        if !(self.keys.shown && self.keys.zone == Zone::Controls && self.keys.at == i) {
            return el;
        }
        let (scroll, follow) = (self.scroll.clone(), self.follow.clone());
        // A halo a little outside the control, following its shape: a selected pill's own border
        // sits on its edge, so the keys' place never reads as the choice in force.
        let ring = div()
            .absolute()
            .top(px(-RING_OUT))
            .bottom(px(-RING_OUT))
            .left(px(-RING_OUT))
            .right(px(-RING_OUT))
            .rounded(px(round + RING_OUT))
            .border_1()
            .border_color(self.theme.chrome.focus_ring.hsla());
        el.relative().child(ring).child(
            gpui::canvas(
                move |b, window, _| {
                    if follow.replace(false) {
                        keep_in_view(&scroll, b, window);
                    }
                },
                |_, _, _, _| (),
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
    }

    /// A key in the window (`settings_key`).
    fn key_down(&mut self, ks: &gpui::Keystroke, window: &mut Window, cx: &mut Context<Self>) {
        if ks.modifiers.modified() {
            return;
        }
        let stops = self.stops.borrow().len();
        let section = Section::ALL.iter().position(|s| *s == self.section).unwrap_or(0);
        match settings_key(&ks.key, self.keys, section, Section::ALL.len(), stops) {
            SettingsKey::Move(keys, to) => {
                self.keys = keys;
                if to != section {
                    self.section = Section::ALL[to];
                    self.privacy.forget();
                    self.keys.at = 0;
                }
                self.follow.set(true);
            }
            SettingsKey::Press(at) => {
                let f = self.stops.borrow().get(at).cloned();
                if let Some(f) = f {
                    f(self, window, cx);
                }
            }
            SettingsKey::Close => {
                window.remove_window();
                return;
            }
            SettingsKey::Pass => return,
        }
        cx.notify();
    }

    /// A pill that shows `dir` in Finder, making it first when it is not there yet.
    fn folder_pill(&self, id: &'static str, label: &'static str, dir: PathBuf, enabled: bool, c: Colors, cx: &mut Context<Self>) -> Stateful<gpui::Div> {
        let p = pill(id, label, false, enabled, c);
        if !enabled {
            return p;
        }
        self.act(p, cx, move |_, _, cx| {
            if let Err(e) = std::fs::create_dir_all(&dir) {
                eprintln!("arkos: cannot create {}: {e}", dir.display());
            }
            cx.reveal_path(&dir);
        })
    }

    /// The Appearance section: the two looks as pills, the one in force selected, and under
    /// them that look's controls alone.
    fn appearance(&self, enabled: bool, c: Colors, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let cfg = &self.shown.config;
        let look = cfg.look();
        let mut looks = div().flex().gap(px(6.0));
        for (ix, (label, which)) in [("Presets", Look::Presets), ("Custom", Look::Custom)].into_iter().enumerate() {
            let current = look == which;
            let mut p = pill(("look", ix), label, current, enabled, c);
            if enabled && !current {
                let edits: Vec<Edit> = look_edits(cfg, which, &self.theme.name, self.theme.font.is_default(), self.theme.recommended_effect(), effects::scene::shown_scene(cfg, &self.theme, &effects::effects_dir()).as_deref()).into_iter().map(|(keys, value)| edit(&keys, value.as_str())).collect();
                p = self.act(p, cx, move |w, _, cx| w.change(edits.clone(), cx));
            }
            looks = looks.child(p);
        }
        let says = match look {
            Look::Presets => "A finished look for dark and one for light: colours, effect and fonts together.",
            Look::Custom => "Any colours in any fonts under any effect. One look, day and night.",
        };
        let mut out = vec![heading("Look", c).into_any_element(), looks.into_any_element(), div().pt(px(6.0)).text_color(c.dim).child(says).into_any_element()];
        out.extend(match look {
            Look::Presets => self.presets(enabled, c, cx),
            Look::Custom => self.custom(enabled, c, cx),
        });
        if let Some((src, name, adding)) = &self.pending_replace {
            let (src, adding) = (src.clone(), *adding);
            let (mut replace, mut keep) = (pill("replace", "Replace", false, enabled, c), pill("keep", "Keep", false, enabled, c));
            if enabled {
                replace = self.act(replace, cx, move |w, window, cx| w.import(src.clone(), adding, true, theme::is_dark(window.appearance()), cx));
                keep = self.act(keep, cx, |w, _, cx| {
                    w.pending_replace = None;
                    cx.notify();
                });
            }
            out.push(div().flex().items_center().gap(px(6.0)).child(format!("Replace the existing {name}?")).child(replace).child(keep).into_any_element());
        }
        if let Some(e) = &self.import_error {
            out.push(div().text_color(c.dim).child(e.clone()).into_any_element());
        }
        out
    }

    /// A pill that runs the picker for `adding`.
    fn add_pill(&self, id: &'static str, label: &'static str, adding: Adding, enabled: bool, c: Colors, cx: &mut Context<Self>) -> Stateful<gpui::Div> {
        let live = enabled && !self.picking;
        let mut p = pill(id, label, false, live, c);
        if live {
            p = self.act(p, cx, move |w, window, cx| w.add(adding, theme::is_dark(window.appearance()), cx));
        }
        p
    }

    /// The presets' controls: the Mode, the dark and the light preset, and adding one.
    fn presets(&self, enabled: bool, c: Colors, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let cfg = &self.shown.config;
        let mut out = vec![heading("Mode", c).into_any_element()];
        let mut modes = div().flex().gap(px(6.0));
        for (ix, (label, mode, value)) in [("System", AppearanceMode::System, "system"), ("Dark", AppearanceMode::Dark, "dark"), ("Light", AppearanceMode::Light, "light")].into_iter().enumerate() {
            let current = cfg.appearance.mode == mode;
            let mut p = pill(("mode", ix), label, current, enabled, c);
            if enabled && !current {
                p = self.act(p, cx, move |w, _, cx| w.change(vec![edit(&["appearance", "mode"], value)], cx));
            }
            modes = modes.child(p);
        }
        out.push(modes.into_any_element());
        // The dark and the light lists side by side, each half the width.
        let mut columns = div().flex().gap(px(16.0));
        for (key, slot, title, list_id, current) in [
            ("dark", theme::Appearance::Dark, "Dark preset", "theme-dark", &cfg.theme.dark),
            ("light", theme::Appearance::Light, "Light preset", "theme-light", &cfg.theme.light),
        ] {
            let mut list = div().flex_1().min_w_0().flex().flex_col().child(heading(title, c));
            let names: Vec<String> = self.themes.iter().map(|(n, _, _)| n.clone()).collect();
            if let Some(missing) = not_installed(current, &names) {
                list = list.child(list_row((list_id, usize::MAX), in_theme_list(&missing), true, enabled, c));
            }
            for entry in preset_list_with(&self.themes, &self.theme_shown, slot, current, &self.group_picks) {
                match entry {
                    PresetRow::One { ix, label } => {
                        let id = (list_id, ix);
                        let (name, source, kind) = &self.themes[ix];
                        let row = match (source, kind) {
                            (theme::Source::Invalid(_), _) | (_, None) => list_row(id, in_theme_list(&label), false, false, c),
                            _ => {
                                let is_current = name == current;
                                let mut row = list_row(id, in_theme_list(&label), is_current, enabled, c);
                                if enabled && !is_current {
                                    let name = name.clone();
                                    row = self.act_row(row, cx, move |w, _, cx| w.change(vec![edit(&["theme", key], name.as_str())], cx));
                                }
                                row
                            }
                        };
                        list = list.child(row);
                    }
                    PresetRow::Group { name: group, members, pick } => {
                        let is_current = members.iter().any(|(ix, _)| &self.themes[*ix].0 == current);
                        let pick_title = members.iter().find(|(ix, _)| *ix == pick).map(|(_, t)| t.clone()).unwrap_or_default();
                        let spec = GroupSpec {
                            list_id,
                            key: GroupKey::Presets(slot, group.clone()),
                            anchor: pick,
                            line: in_theme_list(&preset_group_line(&pick_title)),
                            is_current,
                            members: members
                                .iter()
                                .map(|(ix, title)| {
                                    let name = self.themes[*ix].0.clone();
                                    GroupMember { id: *ix, title: member_name(title, &group), current: &name == current, edits: vec![edit(&["theme", key], name.as_str())], remember: None }
                                })
                                .collect(),
                            group,
                        };
                        list = list.child(self.group_row(spec, enabled, c, cx));
                    }
                }
            }
            columns = columns.child(list);
        }
        out.push(columns.into_any_element());
        let add = self.add_pill("add-preset", "Add preset…", Adding::Preset, enabled, c, cx);
        out.push(div().pt(px(10.0)).flex().gap(px(6.0)).child(add).child(self.folder_pill("presets-folder", "Show presets folder", paths::themes_dir(), enabled, c, cx)).into_any_element());
        out
    }

    /// A group as one row of a list, the whole row a select read as one line of the list's own
    /// text ("watercolor lavender ▾", aligned with every other row): a click anywhere on it
    /// opens a small card of the members floating over the list, where a click picks one. The
    /// keys open and close the card, and its members follow it as stops. Used by the Presets
    /// lists and Custom's Effect list alike.
    fn group_row(&self, spec: GroupSpec, enabled: bool, c: Colors, cx: &mut Context<Self>) -> Stateful<gpui::Div> {
        let GroupSpec { list_id, key, group, anchor, line, is_current, members } = spec;
        let open = self.open_group.as_ref() == Some(&key);
        let mut row = list_row((list_id, 20_000 + anchor), line, is_current, enabled, c).relative();
        if enabled {
            // `list_row` gives a row not in force its hover already; gpui takes one hover
            // style an element, so only the row in force adds one here.
            if is_current {
                row = row.cursor_pointer().hover(move |s| s.bg(c.pill));
            }
            let (toggle, keyed) = (key.clone(), key.clone());
            row = row.on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |w, _, _, cx| {
                    cx.stop_propagation();
                    let just_closed = w.group_closed_at.is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(100));
                    w.open_group = if w.open_group.as_ref() == Some(&toggle) || just_closed { None } else { Some(toggle.clone()) };
                    cx.notify();
                }),
            );
            row = self.key_only(
                row,
                ROW_ROUND,
                Rc::new(move |w: &mut Self, _: &mut Window, cx: &mut Context<Self>| {
                    w.open_group = if w.open_group.as_ref() == Some(&keyed) { None } else { Some(keyed.clone()) };
                    cx.notify();
                }),
            );
        }
        if open {
            // The card: under the choice, drawn above the rows after it, its members by their
            // short names with a check on the one in force.
            let mut card = div()
                .id((list_id, 40_000 + anchor))
                .absolute()
                .top(px(22.0))
                .right(px(0.0))
                .min_w(px(140.0))
                .py(px(4.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(c.border)
                .bg(c.card)
                .shadow_md()
                .flex()
                .flex_col()
                .on_mouse_down_out(cx.listener(|w, _, _, cx| {
                    w.open_group = None;
                    w.group_closed_at = Some(std::time::Instant::now());
                    cx.notify();
                }));
            for m in members {
                let mut item = div()
                    .id((list_id, 30_000 + m.id))
                    .flex()
                    .gap(px(6.0))
                    .px(px(8.0))
                    .py(px(3.0))
                    .text_color(c.text)
                    .child(div().w(px(12.0)).flex_none().child(if m.current { "\u{2713}" } else { "" }))
                    .child(in_theme_list(&m.title));
                if !m.current {
                    let pick: Rc<dyn Fn(&mut Self, &mut Context<Self>)> = {
                        let (edits, remember) = (m.edits, m.remember);
                        Rc::new(move |w: &mut Self, cx: &mut Context<Self>| {
                            w.open_group = None;
                            if let Some((effect, scene)) = &remember {
                                if remember_scene_pick(&mut w.group_picks, effect, scene) {
                                    write_group_picks(&paths::group_picks_file(), &w.group_picks);
                                }
                            }
                            w.change(edits.clone(), cx);
                        })
                    };
                    let by_key = pick.clone();
                    item = item.cursor_pointer().hover(move |s| s.bg(c.pill)).on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |w, _, _, cx| {
                            cx.stop_propagation();
                            pick(w, cx);
                        }),
                    );
                    item = self.key_only(item, ROW_ROUND, Rc::new(move |w: &mut Self, _: &mut Window, cx: &mut Context<Self>| by_key(w, cx)));
                }
                card = card.child(item);
            }
            row = row.child(gpui::deferred(card).with_priority(1));
        }
        let _ = group;
        row
    }

    /// The custom look's Effect list under its heading: plain rows as the other lists have them,
    /// and an effect with several scenes as a group row.
    fn effect_list(&self, rows: Vec<EffectRow>, enabled: bool, c: Colors, cx: &mut Context<Self>) -> gpui::Div {
        let list_id = "custom-effect";
        let mut list = div().flex().flex_col().child(heading("Effect", c));
        for (ix, r) in rows.into_iter().enumerate() {
            match r {
                EffectRow::One(r) => {
                    let clickable = enabled && r.value.is_some();
                    let mut row = list_row((list_id, ix), r.label, r.current, clickable, c);
                    if let (true, false, Some(value)) = (clickable, r.current, r.value) {
                        let edits = vec![edit(&["custom", "effect"], value.as_str())];
                        row = self.act_row(row, cx, move |w, _, cx| w.change(edits.clone(), cx));
                    }
                    list = list.child(row);
                }
                EffectRow::Group { effect, title, members, pick, current } => {
                    let pick_title = members.iter().find(|(n, _)| *n == pick).map(|(_, t)| t.clone()).unwrap_or_default();
                    let spec = GroupSpec {
                        list_id,
                        key: GroupKey::Effect(effect.clone()),
                        anchor: ix,
                        line: in_theme_list(&group_line(&title, &pick_title)),
                        is_current: current,
                        members: members
                            .iter()
                            .enumerate()
                            .map(|(i, (scene, t))| GroupMember {
                                id: i,
                                title: member_name(t, &title),
                                current: current && *scene == pick,
                                edits: effect_pick_edits(&effect, scene),
                                remember: Some((effect.clone(), scene.clone())),
                            })
                            .collect(),
                        group: title,
                    };
                    list = list.child(self.group_row(spec, enabled, c, cx));
                }
            }
        }
        list
    }

    /// One of the custom look's lists under its heading: a click on a row that is not the one in
    /// force writes what `edits` gives for its value.
    #[allow(clippy::too_many_arguments)]
    fn rows_list(&self, title: &'static str, list_id: &'static str, rows: Vec<Row>, enabled: bool, c: Colors, cx: &mut Context<Self>, edits: impl Fn(&str) -> Vec<Edit>) -> gpui::Div {
        let mut list = div().flex().flex_col().child(heading(title, c));
        for (ix, r) in rows.into_iter().enumerate() {
            let clickable = enabled && r.value.is_some();
            let mut row = list_row((list_id, ix), r.label, r.current, clickable, c);
            if let (true, false, Some(value)) = (clickable, r.current, r.value) {
                let edits = edits(&value);
                row = self.act_row(row, cx, move |w, _, cx| w.change(edits.clone(), cx));
            }
            list = list.child(row);
        }
        list
    }

    /// The custom look's controls: the Color on the left, the Font and under it the Effect on
    /// the right, and adding a colour or an effect.
    fn custom(&self, enabled: bool, c: Colors, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let cfg = &self.shown.config;
        // The colour in force is the theme showing: the custom look's own, or with none of its
        // own, the preset the mode picks. A new colour pins the fonts on screen the first time.
        let (shown, plain_font) = (self.theme.name.clone(), self.theme.font.is_default());
        let pinned = cfg.clone();
        // `custom.color` may name a preset (a 0.8.54 config); the row checked is its scheme.
        let current = match cfg.custom.color.as_deref() {
            Some(name) => theme::current_color(name, &self.theme_colors, &paths::themes_dir()),
            None => self.theme.color.clone(),
        };
        let colors = color_rows(&self.theme_colors, &current).into_iter().map(|r| Row { label: in_theme_list(&r.label), ..r }).collect();
        let color = self.rows_list("Color", "custom-color", colors, enabled, c, cx, move |value| {
            color_edits(&pinned, value, &shown, plain_font).into_iter().map(|(keys, value)| edit(&keys, value.as_str())).collect()
        });
        // `custom.font` may name a preset (a 0.8.54 config); the row checked is its font set.
        let font_choice = match cfg.custom.font.as_deref() {
            Some(name) if name != config::PLAIN_FONT && !self.theme_fonts.iter().any(|e| e.name == name) => theme::font_for(Some(name), &self.theme, &paths::themes_dir())
                .ok()
                .and_then(|f| self.theme_fonts.iter().find(|e| e.font == f).map(|e| e.name.clone()))
                .or_else(|| Some(name.to_string())),
            other => other.map(str::to_string),
        };
        let font_rows = font_rows(&self.theme_fonts, font_choice.as_deref(), &self.theme.font, &cfg.font.family, &|f| self.families.contains(f));
        let font = self.rows_list("Font", "custom-font", font_rows, enabled, c, cx, |value| vec![edit(&["custom", "font"], value)]);
        let scene = effects::scene::look_scene(cfg, &self.theme);
        let effect_rows = custom_effect_rows(&cfg.effect_choice(), self.theme.recommended_effect(), scene.as_deref(), &self.effects, &self.effect_scenes, &self.scene_aliases, &self.group_picks);
        let effect = self.effect_list(effect_rows, enabled, c, cx);
        let columns = div().flex().gap(px(16.0)).child(color.flex_1().min_w_0()).child(div().flex_1().min_w_0().flex().flex_col().child(font).child(effect));
        let mut out = vec![columns.into_any_element()];
        let (color, effect) = (self.add_pill("add-color", "Add color…", Adding::Color, enabled, c, cx), self.add_pill("add-effect", "Add effect…", Adding::Effect, enabled, c, cx));
        out.push(div().pt(px(10.0)).flex().gap(px(6.0)).child(color).child(effect).into_any_element());
        out.push(
            div()
                .pt(px(6.0))
                .flex()
                .gap(px(6.0))
                .child(self.folder_pill("colors-folder", "Show colors folder", color_schemes::dir_of(&paths::themes_dir()), enabled, c, cx))
                .child(self.folder_pill("fonts-folder", "Show fonts folder", font_sets::dir_of(&paths::themes_dir()), enabled, c, cx))
                .child(self.folder_pill("effects-folder", "Show effects folder", effects::effects_dir(), enabled, c, cx))
                .into_any_element(),
        );
        out
    }

    fn text(&self, enabled: bool, c: Colors, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let font = &self.shown.config.font;
        let mut out = vec![heading("Font family", c).into_any_element()];
        let mut families = div().flex().flex_col();
        if let Some(missing) = not_installed(&font.family, &self.fonts) {
            families = families.child(list_row("font-missing", missing, true, enabled, c));
        }
        for (ix, name) in self.fonts.iter().enumerate() {
            let current = *name == font.family;
            let mut row = list_row(("font", ix), name.clone(), current, enabled, c);
            if enabled && !current {
                let name = name.clone();
                row = self.act_row(row, cx, move |w, _, cx| w.change(vec![edit(&["font", "family"], name.as_str())], cx));
            }
            families = families.child(row);
        }
        out.push(families.into_any_element());
        let (names, chosen, size, line_height) = preset_rows(font, self.shown.active_preset.as_deref());
        out.push(heading("Text preset", c).into_any_element());
        let mut presets = div().flex().flex_col();
        for (ix, name) in names.into_iter().enumerate() {
            let current = name == chosen;
            let mut row = list_row(("preset", ix), name.clone(), current, enabled, c);
            if enabled && !current {
                row = self.act_row(row, cx, move |w, _, cx| w.change(vec![edit(&["font", "preset"], name.as_str())], cx));
            }
            presets = presets.child(row);
        }
        out.push(presets.into_any_element());
        // Both steppers write the preset whole, since a preset needs its size and line height.
        let (size, line_height) = (f64::from(size), f64::from(line_height));
        let steppers = [("Size", "size-step", size, SIZE_STEP, true), ("Line height", "line-step", line_height, LINE_STEP, false)];
        for (title, id, value, increment, is_size) in steppers {
            out.push(heading(title, c).into_any_element());
            let mut press = |ix: usize, sign: &'static str, delta: f64| {
                let mut p = pill((id, ix), sign, false, enabled, c);
                if enabled {
                    let name = chosen.clone();
                    p = self.act(p, cx, move |w, _, cx| {
                        let (s, l) = preset_step(size, line_height, is_size, delta);
                        w.change(vec![edit(&["font", "presets", &name, "size"], s), edit(&["font", "presets", &name, "line_height"], l)], cx);
                    });
                }
                p
            };
            let (minus, plus) = (press(0, "−", -increment), press(1, "+", increment));
            let shown = div().min_w(px(28.0)).flex().justify_center().child(number(value));
            out.push(div().flex().items_center().gap(px(8.0)).child(minus).child(shown).child(plus).into_any_element());
        }
        out.push(heading("Wide tables", c).into_any_element());
        let mut modes = div().flex().gap(px(6.0));
        for (ix, (label, mode)) in [("Wrap", WideTables::Wrap), ("Scroll", WideTables::Scroll)].into_iter().enumerate() {
            let current = self.shown.config.viewer.wide_tables == mode;
            let mut p = pill(("wide-tables", ix), label, current, enabled, c);
            if enabled && !current {
                p = self.act(p, cx, move |w, _, cx| w.change(vec![wide_tables_edit(mode)], cx));
            }
            modes = modes.child(p);
        }
        out.push(modes.into_any_element());
        out.push(heading("Page width", c).into_any_element());
        let mut widths = div().flex().gap(px(6.0));
        for (ix, (label, width)) in [("Full", PageWidth::Full), ("Letter", PageWidth::Letter)].into_iter().enumerate() {
            let current = self.shown.config.viewer.page_width == width;
            let mut p = pill(("page-width", ix), label, current, enabled, c);
            if enabled && !current {
                p = self.act(p, cx, move |w, _, cx| w.change(vec![page_width_edit(width)], cx));
            }
            widths = widths.child(p);
        }
        out.push(widths.into_any_element());
        out
    }

    /// An On / Off switch writing `on` and `off` through `write`.
    fn switch(&self, id: &'static str, on: Option<bool>, enabled: bool, c: Colors, cx: &mut Context<Self>, write: fn(bool) -> Edit) -> AnyElement {
        let mut row = div().flex().gap(px(6.0));
        for (label, value) in [("On", true), ("Off", false)] {
            let current = on == Some(value);
            let mut p = pill((id, usize::from(value)), label, current || (on.is_none() && !value), enabled, c);
            if enabled && !current {
                p = self.act(p, cx, move |w, _, cx| w.change(vec![write(value)], cx));
            }
            row = row.child(p);
        }
        row.into_any_element()
    }

    /// The Layout section: where a file a session opens goes on its page (sessions hold grids
    /// spec, section 1; the Grid and Inbox choice went on 2026-11-16).
    fn layout(&self, enabled: bool, c: Colors, cx: &mut Context<Self>) -> Vec<AnyElement> {
        use crate::config::OpenPlacement;
        let cfg = &self.shown.config.layout;
        let says = |text: &'static str| div().pt(px(6.0)).text_color(c.dim).child(text).into_any_element();
        let mut out = vec![heading("A session's files open", c).into_any_element()];
        let mut places = div().flex().gap(px(6.0));
        for (ix, (label, value, which)) in [("Neighbor", "neighbor", OpenPlacement::Neighbor), ("In place", "inplace", OpenPlacement::Inplace)].into_iter().enumerate() {
            let current = cfg.placement == which;
            let mut p = pill(("layout-placement", ix), label, current, enabled, c);
            if enabled && !current {
                p = self.act(p, cx, move |w, _, cx| w.change(vec![edit(&["layout", "placement"], value)], cx));
            }
            places = places.child(p);
        }
        out.push(places.into_any_element());
        out.push(says(match cfg.placement {
            OpenPlacement::Inplace => "A file a Claude Code session opens is a new tab in that session's own pane.",
            OpenPlacement::Neighbor => "A file a Claude Code session opens goes to the pane on its right, else the one on its left, else a new pane split off to its right, on the session's own page.",
        }));
        out
    }

    fn startup(&self, c: Colors, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let enabled = self.shown.machine_switches_live();
        let value = self.shown.config.recovery.start_at_login;
        // While the config says "ask", the switch reads off and either press writes a value.
        let on = match value {
            StartAtLogin::On => Some(true),
            StartAtLogin::Off => Some(false),
            StartAtLogin::Ask => None,
        };
        let mut out = vec![heading("Start at login", c).into_any_element()];
        out.push(self.switch("login", on, enabled, c, cx, |on| edit(&["recovery", "start_at_login"], if on { "on" } else { "off" })));
        if self.shown.sandbox {
            out.push(div().text_color(c.dim).text_size(px(12.0)).child(crate::sandbox::REFUSAL).into_any_element());
        } else if value == StartAtLogin::Ask {
            out.push(div().text_color(c.dim).text_size(px(12.0)).child("set by config to ask").into_any_element());
        }
        out
    }

    fn updates(&self, c: Colors, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let enabled = self.shown.machine_switches_live();
        let mut out = vec![heading("Automatic updates", c).into_any_element()];
        out.push(self.switch("auto-update", Some(self.shown.config.update.auto), enabled, c, cx, |on| edit(&["update", "auto"], on)));
        if self.shown.sandbox {
            out.push(div().text_color(c.dim).text_size(px(12.0)).child(crate::sandbox::REFUSAL).into_any_element());
        }
        if let Some(version) = &self.shown.update_ready {
            let mut now = pill("update-now", "Update now", false, enabled, c);
            if enabled {
                let on_update = self.on_update.clone();
                now = self.act(now, cx, move |_, _, cx| {
                    let on_update = on_update.clone();
                    cx.defer(move |cx| on_update(cx));
                });
            }
            out.push(div().pt(px(8.0)).flex().items_center().gap(px(8.0)).child(ready_line(version)).child(now).into_any_element());
        }
        out
    }

    fn privacy(&mut self, enabled: bool, c: Colors, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let PrivacyRows { usage_line, last, lines } = self.privacy.get(&paths::telemetry_dir(), &self.shown).clone();
        let t = &self.shown.config.telemetry;
        let mut out = vec![heading(USAGE_ROW, c).into_any_element()];
        let mut usage = div().flex().gap(px(6.0));
        for (ix, (label, value)) in USAGE_CHOICES.into_iter().enumerate() {
            let current = t.usage == value;
            let mut p = pill(("usage", ix), label, current, enabled, c);
            if enabled && !current {
                p = self.act(p, cx, move |w, _, cx| w.change(vec![usage_edit(value)], cx));
            }
            usage = usage.child(p);
        }
        out.push(usage.into_any_element());
        out.push(div().text_color(c.dim).text_size(px(12.0)).child(usage_line).into_any_element());
        out.push(heading(CRASHES_ROW, c).into_any_element());
        let mut crashes = div().flex().gap(px(6.0));
        for (ix, (label, value)) in CRASH_CHOICES.into_iter().enumerate() {
            let current = t.crashes == value;
            let mut p = pill(("crashes", ix), label, current, enabled, c);
            if enabled && !current {
                p = self.act(p, cx, move |w, _, cx| w.change(vec![crashes_edit(value)], cx));
            }
            crashes = crashes.child(p);
        }
        out.push(crashes.into_any_element());
        // Both stay live while the config has a problem: they only read.
        let sends = self.act(pill("what-arkos-sends", WHAT_IS_SENT_ROW, false, true, c), cx, |_, _, cx| aux::open_disclosure_from_settings(cx));
        let mut report = pill("last-report", LAST_REPORT_ROW, false, last.is_some(), c);
        if let Some(file) = last.clone() {
            report = self.act(report, cx, move |_, _, cx| aux::open_last_report(cx, file.clone()));
        }
        out.push(div().pt(px(10.0)).flex().gap(px(6.0)).child(sends).child(report).into_any_element());
        if last.is_none() {
            out.push(div().text_color(c.dim).text_size(px(12.0)).child("no session sealed yet").into_any_element());
        }
        // Each crash report by date with its `inc`, a receipt the person can quote in an issue: a
        // click copies the `inc`.
        out.push(heading(REPORTS_ROW, c).into_any_element());
        let mono = SharedString::from(self.shown.config.font.family.clone());
        if lines.is_empty() {
            out.push(div().text_color(c.dim).text_size(px(12.0)).child("no crash reports").into_any_element());
        }
        for (ix, ReportLine { label, inc, status }) in lines.into_iter().enumerate() {
            let row = div()
                .id(("crash-report", ix))
                .flex()
                .gap(px(10.0))
                .text_size(px(12.0))
                .font_family(mono.clone())
                .cursor_pointer()
                .hover(move |s| s.bg(c.pill))
                .child(label)
                .child(div().text_color(c.dim).child(status));
            let row = self.act_row(row, cx, move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(inc.clone())));
            out.push(row.into_any_element());
        }
        out
    }

    fn keyboard(&self, c: Colors, cx: &mut Context<Self>) -> Vec<AnyElement> {
        // Live even while the config has a problem: opening the file is how it gets fixed.
        let open = self.act(pill("open-config", "Open config", false, true, c), cx, |_, _, cx| open_config_in_editor(cx));
        // `open_keybindings` defers its work itself, so this window is not being updated when the
        // Keybindings window reads the Root.
        let list = self.act(pill("open-keybindings", "Keybindings", false, true, c), cx, |_, _, cx| aux::open_keybindings(cx));
        vec![
            div().flex().items_center().gap(px(8.0)).child("Keybindings live in config.toml").child(open).into_any_element(),
            div().flex().items_center().gap(px(8.0)).child("Every binding is listed under Help, Keybindings").child(list).into_any_element(),
        ]
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A group member in force (from here, the palette or `arkos theme use`) becomes its
        // group's remembered pick.
        let (dark, light) = (self.shown.config.theme.dark.clone(), self.shown.config.theme.light.clone());
        let mut changed = false;
        for (current, slot) in [(dark, theme::Appearance::Dark), (light, theme::Appearance::Light)] {
            changed |= remember_pick(&mut self.group_picks, &self.theme_shown, &current, slot);
        }
        if changed {
            write_group_picks(&paths::group_picks_file(), &self.group_picks);
        }
        // The controls' stops are collected afresh as they are drawn.
        self.stops.borrow_mut().clear();
        let t = &self.theme.chrome;
        let c = Colors::of(&self.theme);
        let (bg, sidebar) = (t.window_bg.hsla(), t.sidebar.hsla());
        let section_ringed = self.keys.shown && self.keys.zone == Zone::Sections;
        let ring_c = t.focus_ring.hsla();
        let enabled = self.shown.editable();
        let mut sections = div().w(px(SECTIONS_W)).flex_none().flex().flex_col().gap(px(2.0)).p(px(10.0)).bg(sidebar).border_r_1().border_color(c.border);
        for (ix, section) in Section::ALL.into_iter().enumerate() {
            let current = section == self.section;
            let row = div()
                .id(("section", ix))
                .px(px(8.0))
                .py(px(4.0))
                .rounded(px(4.0))
                .cursor_pointer()
                .text_color(if current { c.text } else { c.dim })
                .when(current, |d| d.bg(c.pill))
                // The keyboard's ring on the section shown while the keys are in the column.
                .border_1()
                .border_color(if current && section_ringed { ring_c } else { gpui::transparent_black() })
                .on_click(cx.listener(move |w, _, _, cx| {
                    w.section = section;
                    w.keys = Keys::default();
                    // A section shown reads what it shows afresh.
                    w.privacy.forget();
                    cx.notify();
                }))
                .child(section.label());
            sections = sections.child(row);
        }
        let controls = match self.section {
            Section::Appearance => self.appearance(enabled, c, cx),
            Section::Layout => self.layout(enabled, c, cx),
            Section::Text => self.text(enabled, c, cx),
            Section::Startup => self.startup(c, cx),
            Section::Updates => self.updates(c, cx),
            Section::Privacy => self.privacy(enabled, c, cx),
            Section::Keyboard => self.keyboard(c, cx),
        };
        let banner = self.shown.error.clone().map(|e| div().flex_none().px(px(10.0)).py(px(6.0)).rounded(px(4.0)).bg(c.pill).text_color(c.text).child(e));
        // A refused write is said above the controls, which stay live: the next write may succeed.
        let write_error = self.shown.write_error.clone().map(|e| div().flex_none().pb(px(6.0)).text_color(c.dim).child(e));
        let content = div()
            .id("settings-controls")
            .track_scroll(&self.scroll)
            .flex_1()
            .min_w_0()
            .overflow_y_scroll()
            .p(px(16.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .children(banner)
            .children(write_error)
            .children(controls);
        div()
            .id("settings")
            .key_context("Settings")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|w, ev: &KeyDownEvent, window, cx| w.key_down(&ev.keystroke, window, cx)))
            // A press of the pointer hides the keyboard's ring; the next key shows it again.
            .capture_any_mouse_down(cx.listener(|w, _: &gpui::MouseDownEvent, _, cx| {
                if w.keys.shown {
                    w.keys.shown = false;
                    cx.notify();
                }
            }))
            // Cmd+W arrives as the close-tab action.
            .on_action(|_: &keymap::CloseTab, window, _| window.remove_window())
            .on_action(|_: &keymap::NewWindow, _, cx| aux::forward_new_window(cx))
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(c.text)
            .text_size(px(13.0))
            .font_family(SharedString::from(UI_FONT))
            .child(chrome::aux_title_bar(TITLE, &self.theme))
            .child(div().flex_1().min_h_0().flex().child(sections).child(content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppearanceMode, Config, DEFAULT_CONFIG};

    #[test]
    fn a_theme_list_holds_its_own_kind_the_broken_files_and_a_current_pick_of_the_other_kind_marked() {
        use theme::{Appearance::*, Source};
        let themes = vec![
            ("navy".to_string(), Source::BuiltIn, Some(Dark)),
            ("peach-watercolor".to_string(), Source::BuiltIn, Some(Light)),
            ("matrix".to_string(), Source::BuiltIn, Some(Dark)),
            ("broken".to_string(), Source::Invalid("bad".into()), None),
        ];
        let rows = |slot, current| slot_rows(&themes, slot, current);
        assert_eq!(rows(Dark, "navy"), vec![(0, "navy".to_string()), (2, "matrix".to_string()), (3, "broken (invalid)".to_string())]);
        assert_eq!(rows(Light, "peach-watercolor"), vec![(1, "peach-watercolor".to_string()), (3, "broken (invalid)".to_string())]);
        assert_eq!(rows(Light, "navy"), vec![(0, "navy (a dark theme)".to_string()), (1, "peach-watercolor".to_string()), (3, "broken (invalid)".to_string())]);
    }

    #[test]
    fn presets_show_their_titles_and_a_group_folds_into_one_row_picking_its_current_or_default() {
        use theme::{Appearance::*, Source, Shown};
        let themes = vec![
            ("navy".to_string(), Source::BuiltIn, Some(Dark)),
            ("peach-watercolor".to_string(), Source::BuiltIn, Some(Light)),
            ("lavender-watercolor".to_string(), Source::BuiltIn, Some(Light)),
            ("my-paper".to_string(), Source::File, Some(Light)),
        ];
        let shown: std::collections::BTreeMap<String, Shown> = [
            ("navy", Shown { title: "Plain Navy".into(), group: None, group_default: false }),
            ("peach-watercolor", Shown { title: "Watercolor Peach".into(), group: Some("Watercolor".into()), group_default: false }),
            ("lavender-watercolor", Shown { title: "Watercolor Lavender".into(), group: Some("Watercolor".into()), group_default: true }),
            ("my-paper", Shown { title: "My Paper".into(), group: None, group_default: false }),
        ]
        .into_iter()
        .map(|(n, s)| (n.to_string(), s))
        .collect();
        let group = |pick: usize| PresetRow::Group { name: "Watercolor".into(), members: vec![(2, "Watercolor Lavender".into()), (1, "Watercolor Peach".into())], pick };
        assert_eq!(preset_list(&themes, &shown, Light, "my-paper"), vec![PresetRow::One { ix: 3, label: "My Paper".into() }, group(2)], "the group's default, Lavender, is its pick");
        assert_eq!(preset_list(&themes, &shown, Light, "peach-watercolor")[1], group(1), "the current member is the pick");
        assert_eq!(preset_list(&themes, &shown, Dark, "navy"), vec![PresetRow::One { ix: 0, label: "Plain Navy".into() }]);
        assert_eq!(preset_list(&themes, &shown, Light, "navy")[0], PresetRow::One { ix: 0, label: "Plain Navy (a dark theme)".into() });
        assert_eq!(theme::title_case("my-dark-theme"), "My Dark Theme", "a user's theme without a title reads in capitalised words");
        let alpha = vec![
            ("zeta".to_string(), Source::File, Some(Light)),
            ("peach-watercolor".to_string(), Source::BuiltIn, Some(Light)),
            ("broken".to_string(), Source::Invalid("bad".into()), None),
            ("lavender-watercolor".to_string(), Source::BuiltIn, Some(Light)),
            ("alpha".to_string(), Source::File, Some(Light)),
            ("navy".to_string(), Source::BuiltIn, Some(Dark)),
        ];
        let labels: Vec<String> = preset_list(&alpha, &shown, Light, "navy")
            .into_iter()
            .map(|r| match r {
                PresetRow::One { label, .. } => label,
                PresetRow::Group { name, members, .. } => format!("{name}: {}", members.into_iter().map(|(_, l)| l).collect::<Vec<_>>().join(", ")),
            })
            .collect();
        assert_eq!(labels, ["Plain Navy (a dark theme)", "Alpha", "Watercolor: Watercolor Lavender, Watercolor Peach", "Zeta", "Broken (invalid)"], "alphabetical, the other kind's current pick first and broken files last");
        let peach: GroupPicks = [("Watercolor".to_string(), "peach-watercolor".to_string())].into_iter().collect();
        assert_eq!(preset_list_with(&themes, &shown, Light, "my-paper", &peach)[1], group(1), "another preset in force leaves the group on its last pick, Peach");
        let gone: GroupPicks = [("Watercolor".to_string(), "deleted-theme".to_string())].into_iter().collect();
        assert_eq!(preset_list_with(&themes, &shown, Light, "my-paper", &gone)[1], group(2), "a last pick no longer installed falls back to the default");
    }

    #[test]
    fn a_group_remembers_the_member_in_force_and_the_picks_survive_a_round_trip_through_their_file() {
        use theme::Shown;
        let shown: std::collections::BTreeMap<String, Shown> = [
            ("peach-watercolor", Shown { title: "Watercolor Peach".into(), group: Some("Watercolor".into()), group_default: false }),
            ("material", Shown { title: "Material Bright".into(), group: Some("Material".into()), group_default: false }),
            ("material-ocean", Shown { title: "Material Ocean".into(), group: Some("Material".into()), group_default: false }),
            ("tokyo-rain", Shown { title: "Tokyo Rain".into(), group: None, group_default: false }),
        ]
        .into_iter()
        .map(|(n, s)| (n.to_string(), s))
        .collect();
        use theme::Appearance::{Dark, Light};
        let mut picks = GroupPicks::new();
        assert!(remember_pick(&mut picks, &shown, "peach-watercolor", Light), "a member in force is remembered");
        assert!(!remember_pick(&mut picks, &shown, "peach-watercolor", Light), "nothing new to write");
        assert!(remember_pick(&mut picks, &shown, "material", Light));
        assert!(remember_pick(&mut picks, &shown, "material-ocean", Dark));
        assert!(!remember_pick(&mut picks, &shown, "tokyo-rain", Dark), "a theme in no group changes nothing");
        assert_eq!(picks.get(&pick_key("Watercolor", Light)).map(String::as_str), Some("peach-watercolor"), "picking Material left Watercolor alone");
        assert_eq!(picks.get(&pick_key("Material", Light)).map(String::as_str), Some("material"), "the dark Material pick left the light one alone");
        assert_eq!(picks.get(&pick_key("Material", Dark)).map(String::as_str), Some("material-ocean"));
        let old: GroupPicks = [("Material".to_string(), "material".to_string())].into_iter().collect();
        assert_eq!(picked(&old, "Material", Light).map(String::as_str), Some("material"), "a file from before the slots still counts");
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("group-picks");
        write_group_picks(&file, &picks);
        assert_eq!(read_group_picks(&file), picks);
        assert_eq!(read_group_picks(&dir.path().join("missing")), GroupPicks::new());
    }

    #[test]
    fn the_theme_lists_show_every_name_in_lower_case() {
        assert_eq!(in_theme_list("Material Navy"), "material navy");
        assert_eq!(in_theme_list(&group_line("Watercolor", "Lavender")), "watercolor lavender \u{25be}");
        assert_eq!(in_theme_list("Tokyo Rain (dark)"), "tokyo rain (dark)");
    }

    #[test]
    fn the_settings_keys_go_as_the_ide_s_do() {
        let k = |zone, at| Keys { zone, at, shown: true };
        let unseen = Keys::default();
        // The first key shows the ring and moves nothing; Escape still closes.
        assert_eq!(settings_key("down", unseen, 2, 7, 5), SettingsKey::Move(Keys { shown: true, ..unseen }, 2));
        assert_eq!(settings_key("escape", unseen, 2, 7, 5), SettingsKey::Close);
        // The column: Up and Down show the section before or after, stopping at the ends.
        assert_eq!(settings_key("down", k(Zone::Sections, 0), 2, 7, 5), SettingsKey::Move(k(Zone::Sections, 0), 3));
        assert_eq!(settings_key("up", k(Zone::Sections, 0), 0, 7, 5), SettingsKey::Move(k(Zone::Sections, 0), 0));
        assert_eq!(settings_key("down", k(Zone::Sections, 0), 6, 7, 5), SettingsKey::Move(k(Zone::Sections, 0), 6));
        for key in ["right", "enter", "space"] {
            assert_eq!(settings_key(key, k(Zone::Sections, 3), 2, 7, 5), SettingsKey::Move(k(Zone::Controls, 0), 2), "{key} goes into the controls");
        }
        assert_eq!(settings_key("right", k(Zone::Sections, 0), 2, 7, 0), SettingsKey::Pass, "a section with no control keeps the keys");
        // The controls: Up and Down, stopping at the ends; Enter or Space press; Left or Escape back.
        assert_eq!(settings_key("down", k(Zone::Controls, 1), 2, 7, 5), SettingsKey::Move(k(Zone::Controls, 2), 2));
        assert_eq!(settings_key("down", k(Zone::Controls, 4), 2, 7, 5), SettingsKey::Move(k(Zone::Controls, 4), 2));
        assert_eq!(settings_key("up", k(Zone::Controls, 0), 2, 7, 5), SettingsKey::Move(k(Zone::Controls, 0), 2));
        assert_eq!(settings_key("enter", k(Zone::Controls, 3), 2, 7, 5), SettingsKey::Press(3));
        assert_eq!(settings_key("space", k(Zone::Controls, 9), 2, 7, 5), SettingsKey::Press(4), "a place past the controls left by a press is the last");
        assert_eq!(settings_key("left", k(Zone::Controls, 3), 2, 7, 5), SettingsKey::Move(k(Zone::Sections, 3), 2));
        assert_eq!(settings_key("escape", k(Zone::Controls, 3), 2, 7, 5), SettingsKey::Move(k(Zone::Sections, 3), 2));
        assert_eq!(settings_key("a", k(Zone::Controls, 3), 2, 7, 5), SettingsKey::Pass);
    }

    #[test]
    fn a_group_s_row_reads_as_one_line_of_text() {
        assert_eq!(group_line("Watercolor", "Lavender"), "Watercolor Lavender \u{25be}");
    }

    #[test]
    fn a_preset_group_s_row_reads_as_its_member_s_title_once() {
        // Cyrus, 2026-11-19: "The group option for seattle rain says "rain seattle rain". It
        // should just be "seattle rain"." A title that starts with its group keeps the group's
        // word in front once; one that ends with it (or names it elsewhere) is shown as it is.
        assert_eq!(in_theme_list(&preset_group_line("Seattle Rain")), "seattle rain \u{25be}");
        assert_eq!(in_theme_list(&preset_group_line("Tokyo Rain")), "tokyo rain \u{25be}");
        assert_eq!(in_theme_list(&preset_group_line("Watercolor Lavender")), "watercolor lavender \u{25be}");
        assert_eq!(in_theme_list(&preset_group_line("Material Navy")), "material navy \u{25be}");
    }

    #[test]
    fn a_group_s_menu_names_its_members_without_the_group_s_word() {
        assert_eq!(member_name("Watercolor Lavender", "Watercolor"), "Lavender");
        assert_eq!(member_name("Material Solar", "Material"), "Solar");
        assert_eq!(member_name("Night Ink", "Watercolor"), "Night Ink", "a member named otherwise keeps its title");
        assert_eq!(member_name("Watercolor", "Watercolor"), "Watercolor");
    }

    #[test]
    fn page_width_edit_writes_the_viewer_key() {
        let (keys, value) = page_width_edit(PageWidth::Letter);
        assert_eq!(keys, vec!["viewer".to_string(), "page_width".to_string()]);
        assert_eq!((value.as_str(), page_width_edit(PageWidth::Full).1.as_str()), (Some("letter"), Some("full")));
    }

    #[test]
    fn wide_tables_edit_writes_the_viewer_key() {
        use crate::config::WideTables;
        let (keys, value) = wide_tables_edit(WideTables::Scroll);
        assert_eq!(keys, vec!["viewer".to_string(), "wide_tables".to_string()]);
        assert_eq!(value.as_str(), Some("scroll"));
        assert_eq!(wide_tables_edit(WideTables::Wrap).1.as_str(), Some("wrap"));
    }

    #[test]
    fn the_default_file_documents_wide_tables_and_still_parses() {
        assert!(DEFAULT_CONFIG.contains("# wide_tables = \"wrap\""));
        let cfg: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        assert_eq!(cfg.viewer.wide_tables, crate::config::WideTables::Wrap);
    }

    #[test]
    fn the_text_section_reads_the_presets_filters_fixed_pitch_families_and_rounds() {
        let cfg: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        let (names, chosen, size, lh) = preset_rows(&cfg.font, None);
        assert_eq!(names, vec!["comfortable", "compact", "relaxed"]);
        assert_eq!((chosen.as_str(), size, lh), ("comfortable", 16.0, 1.5));
        let fams = monospace_families(&["Helvetica".into(), "SF Mono".into(), "Menlo".into(), "Fira Code".into(), "JetBrains Mono".into(), "Menlo".into()]);
        assert_eq!(fams, vec!["Fira Code", "JetBrains Mono", "Menlo", "SF Mono"]);
        assert_eq!(round_tenth(1.6000000238), 1.6);
        assert_eq!(theme_key_for(AppearanceMode::System, true), "dark");
        assert_eq!(theme_key_for(AppearanceMode::System, false), "light");
        assert_eq!(theme_key_for(AppearanceMode::Light, true), "light");
    }

    #[test]
    fn the_custom_look_lists_every_colour_dark_first_and_every_effect_after_none() {
        use crate::config::EffectChoice;
        use crate::effects::manifest::Source;
        use crate::theme::{Appearance, Source as ThemeSource};
        let entry = |name: &str, title: &str, source: ThemeSource, appearance: Option<Appearance>| theme::ColorEntry { name: name.into(), title: title.into(), appearance, source };
        let themes = vec![
            entry("navy", "Material Navy", ThemeSource::BuiltIn, Some(Appearance::Dark)),
            entry("peach", "Solar Arabia", ThemeSource::BuiltIn, Some(Appearance::Light)),
            entry("rain", "Tokyo Rain", ThemeSource::BuiltIn, Some(Appearance::Dark)),
            entry("broken", "Broken", ThemeSource::Invalid("bad".into()), None),
        ];
        let row = |label: &str, value: Option<&str>, current: bool| Row { label: label.to_string(), value: value.map(str::to_string), current };
        assert_eq!(
            color_rows(&themes, "rain"),
            vec![row("Material Navy (dark)", Some("navy"), false), row("Tokyo Rain (dark)", Some("rain"), true), row("Solar Arabia (light)", Some("peach"), false), row("Broken (invalid)", None, false)]
        );
        // A colour that is not installed is still shown as the one in force.
        assert_eq!(color_rows(&themes, "gone")[0], row("gone (not installed)", None, true));
        let listed = vec![
            ("watercolor".to_string(), Source::BuiltIn),
            ("overworld".to_string(), Source::BuiltIn),
            ("neon".to_string(), Source::Folder("/e/neon".into())),
            ("broken".to_string(), Source::Invalid("scale must be 0.1 to 1.0".into())),
        ];
        let none = GroupPicks::new();
        let rows = |choice: &EffectChoice, own: Option<&str>| -> Vec<Row> {
            custom_effect_rows(choice, own, None, &listed, &[], &runic_aliases(), &none).into_iter().map(|r| match r { EffectRow::One(r) => r, g => panic!("{g:?}") }).collect()
        };
        assert_eq!(
            rows(&EffectChoice::Named("neon".into()), Some("overworld")),
            vec![row("None", Some("none"), false), row("watercolor", Some("watercolor"), false), row("overworld", Some("overworld"), false), row("neon", Some("neon"), true), row("broken (invalid)", None, false)]
        );
        assert!(rows(&EffectChoice::None, Some("overworld"))[0].current);
        // `theme` is the colour's own effect, or none when it has none.
        assert!(rows(&EffectChoice::Theme, Some("overworld"))[2].current);
        assert!(rows(&EffectChoice::Theme, None)[0].current);
        assert_eq!(rows(&EffectChoice::Named("gone".into()), None)[5], row("gone (not installed)", None, true));
    }

    fn runic_scenes() -> Vec<(String, Vec<(String, String)>)> {
        vec![("runic".into(), vec![("tokyo".into(), "Tokyo".into()), ("matrix-city".into(), "Matrix City".into())])]
    }

    fn runic_aliases() -> std::collections::BTreeMap<String, String> {
        [("runic/city".to_string(), "tokyo".to_string()), ("runic/code".to_string(), "matrix-city".to_string())].into()
    }

    fn effects_listed() -> Vec<(String, effects::manifest::Source)> {
        vec![("material".to_string(), effects::manifest::Source::BuiltIn), ("runic".to_string(), effects::manifest::Source::BuiltIn)]
    }

    #[test]
    fn the_dark_presets_list_shows_one_rain_row_with_tokyo_rain_its_default_and_remembers_its_pick() {
        use theme::Appearance::Dark;
        let themes: Vec<(String, theme::Source, Option<theme::Appearance>)> = theme::BUILTIN.iter().map(|(n, _)| (n.to_string(), theme::Source::BuiltIn, theme::builtin(n).map(|t| t.kind()))).collect();
        let shown: std::collections::BTreeMap<String, theme::Shown> = theme::BUILTIN
            .iter()
            .map(|(n, _)| {
                let t = theme::builtin(n).unwrap();
                (n.to_string(), theme::Shown { title: t.title.clone().unwrap_or_else(|| theme::title_case(n)), group: t.group.clone(), group_default: t.group_default })
            })
            .collect();
        let rain_rows = |current: &str, picks: &GroupPicks| -> Vec<PresetRow> { preset_list_with(&themes, &shown, Dark, current, picks).into_iter().filter(|r| matches!(r, PresetRow::Group { name, .. } if name == "Rain")).collect() };
        let ix = |n: &str| themes.iter().position(|(t, _, _)| t == n).unwrap();
        let rows = rain_rows("navy", &GroupPicks::new());
        assert_eq!(rows.len(), 1, "one Rain row in the dark list");
        let PresetRow::Group { members, pick, .. } = &rows[0] else { unreachable!() };
        assert_eq!(members.len(), 4);
        assert_eq!(*pick, ix("tokyo-rain"), "Tokyo Rain is the default");
        let mut picks = GroupPicks::new();
        assert!(remember_pick(&mut picks, &shown, "london-rain", Dark));
        let rows = rain_rows("navy", &picks);
        assert!(matches!(&rows[0], PresetRow::Group { pick, .. } if *pick == ix("london-rain")), "the row remembers London Rain");
        let rows = rain_rows("seattle-rain", &picks);
        assert!(matches!(&rows[0], PresetRow::Group { pick, .. } if *pick == ix("seattle-rain")), "the one in force is the pick");
    }

    #[test]
    fn runic_is_one_group_row_with_tokyo_and_matrix_city() {
        let rows = custom_effect_rows(&EffectChoice::None, None, None, &effects_listed(), &runic_scenes(), &runic_aliases(), &GroupPicks::new());
        assert_eq!(rows.len(), 3, "None, Material and one Runic row: {rows:?}");
        let members = vec![("tokyo".to_string(), "Tokyo".to_string()), ("matrix-city".to_string(), "Matrix City".to_string())];
        assert_eq!(rows[2], EffectRow::Group { effect: "runic".into(), title: "Runic".into(), members, pick: "tokyo".into(), current: false });
    }

    #[test]
    fn the_runic_row_shows_the_scene_in_force() {
        let rows = custom_effect_rows(&EffectChoice::Named("runic".into()), None, Some("matrix-city"), &effects_listed(), &runic_scenes(), &runic_aliases(), &GroupPicks::new());
        assert!(matches!(&rows[2], EffectRow::Group { pick, current: true, .. } if pick == "matrix-city"), "{:?}", rows[2]);
        let by_colour = custom_effect_rows(&EffectChoice::Theme, Some("runic"), Some("matrix-city"), &effects_listed(), &runic_scenes(), &runic_aliases(), &GroupPicks::new());
        assert!(matches!(&by_colour[2], EffectRow::Group { pick, current: true, .. } if pick == "matrix-city"), "the colour's own effect and scene count too");
    }

    #[test]
    fn picking_a_runic_scene_writes_the_effect_and_the_scene() {
        let edits = effect_pick_edits("runic", "matrix-city");
        let shown: Vec<(String, String)> = edits.iter().map(|(keys, v)| (keys.join("."), v.as_str().unwrap_or_default().to_string())).collect();
        assert_eq!(shown, vec![("custom.effect".to_string(), "runic".to_string()), ("custom.scene".to_string(), "matrix-city".to_string())]);
    }

    #[test]
    fn an_effect_without_scenes_stays_a_plain_row() {
        let rows = custom_effect_rows(&EffectChoice::Named("material".into()), None, Some("stray"), &effects_listed(), &runic_scenes(), &runic_aliases(), &GroupPicks::new());
        assert_eq!(rows[1], EffectRow::One(Row { label: "material".into(), value: Some("material".into()), current: true }));
        let one_scene = vec![("material".to_string(), vec![("only".to_string(), "Only".to_string())])];
        let rows = custom_effect_rows(&EffectChoice::None, None, None, &effects_listed(), &one_scene, &runic_aliases(), &GroupPicks::new());
        assert!(matches!(rows[1], EffectRow::One(_)), "one scene is not a choice");
    }

    #[test]
    fn the_runic_group_remembers_its_last_pick() {
        let mut picks = GroupPicks::new();
        assert!(remember_scene_pick(&mut picks, "runic", "matrix-city"));
        assert!(!remember_scene_pick(&mut picks, "runic", "matrix-city"), "nothing new to write");
        assert_eq!(picks.get("effect:runic").map(String::as_str), Some("matrix-city"));
        let rows = custom_effect_rows(&EffectChoice::None, None, None, &effects_listed(), &runic_scenes(), &runic_aliases(), &picks);
        assert!(matches!(&rows[2], EffectRow::Group { pick, current: false, .. } if pick == "matrix-city"), "{:?}", rows[2]);
        picks.insert("effect:runic".into(), "gone".into());
        let rows = custom_effect_rows(&EffectChoice::None, None, None, &effects_listed(), &runic_scenes(), &runic_aliases(), &picks);
        assert!(matches!(&rows[2], EffectRow::Group { pick, .. } if pick == "tokyo"), "a scene no longer listed falls back to the first");
    }

    #[test]
    fn the_runic_row_with_no_scene_in_force_names_the_first_scene_whatever_was_picked_before() {
        let mut picks = GroupPicks::new();
        picks.insert("effect:runic".into(), "matrix-city".into());
        let rows = custom_effect_rows(&EffectChoice::Named("runic".into()), None, None, &effects_listed(), &runic_scenes(), &runic_aliases(), &picks);
        assert!(matches!(&rows[2], EffectRow::Group { pick, current: true, .. } if pick == "tokyo"), "{:?}", rows[2]);
    }

    #[test]
    fn the_runic_row_resolves_an_old_scene_name_to_the_scene_drawn() {
        let mut picks = GroupPicks::new();
        picks.insert("effect:runic".into(), "tokyo".into());
        let rows = custom_effect_rows(&EffectChoice::Named("runic".into()), None, Some("code"), &effects_listed(), &runic_scenes(), &runic_aliases(), &picks);
        assert!(matches!(&rows[2], EffectRow::Group { pick, current: true, .. } if pick == "matrix-city"), "{:?}", rows[2]);
    }

    #[test]
    fn returning_to_a_custom_look_with_its_own_effect_and_no_scene_pins_no_scene() {
        use crate::config::{parse, Look};
        let no_scene = |e: &str| parse(&format!("[appearance]\nlook = \"presets\"\n[custom]\ncolor = \"matrix\"\neffect = \"{e}\"\n")).unwrap();
        // The preset on screen is Tokyo Rain, the custom effect is runic over the Matrix City.
        let edits = look_edits(&no_scene("runic"), Look::Custom, "tokyo-rain", false, Some("runic"), Some("tokyo"));
        assert!(edits.iter().all(|(k, _)| k[1] != "scene"), "{edits:?}");
        let edits = look_edits(&no_scene("runic"), Look::Custom, "matrix", false, Some("runic"), Some("matrix-city"));
        assert!(edits.iter().all(|(k, _)| k[1] != "scene"), "{edits:?}");
        let edits = look_edits(&no_scene("material"), Look::Custom, "matrix", false, Some("runic"), Some("matrix-city"));
        assert!(edits.iter().all(|(k, _)| k[1] != "scene"), "{edits:?}");
    }

    #[test]
    fn the_first_click_on_custom_pins_the_scene_showing() {
        use crate::config::Look;
        let presets = Config::default();
        let edits = look_edits(&presets, Look::Custom, "matrix", false, Some("runic"), Some("matrix-city"));
        assert!(edits.contains(&(vec!["custom", "scene"], "matrix-city".to_string())), "{edits:?}");
        let pinned = crate::config::parse("[appearance]\nlook = \"presets\"\n[custom]\ncolor = \"matrix\"\neffect = \"runic\"\nscene = \"tokyo\"\n").unwrap();
        assert!(look_edits(&pinned, Look::Custom, "matrix", false, Some("runic"), Some("matrix-city")).iter().all(|(k, _)| k[1] != "scene"), "a scene already chosen is kept");
    }

    #[test]
    fn putting_the_custom_look_in_force_starts_from_what_is_on_screen_and_never_loses_either_look() {
        use crate::config::{parse, Look};
        let own = |pairs: &[(&[&'static str], &str)]| -> Vec<(Vec<&'static str>, String)> { pairs.iter().map(|(k, v)| (k.to_vec(), v.to_string())).collect() };
        // From the presets, the first time: the preset showing and its own effect.
        let presets = Config::default();
        assert_eq!(
            look_edits(&presets, Look::Custom, "timo-purple", false, Some("overworld"), None),
            own(&[(&["appearance", "look"], "custom"), (&["custom", "color"], "timo-purple"), (&["custom", "font"], "timo-purple"), (&["custom", "effect"], "overworld")])
        );
        assert_eq!(look_edits(&presets, Look::Custom, "navy", true, None, None)[2..], own(&[(&["custom", "font"], "default"), (&["custom", "effect"], "none")]), "a preset with no fonts and no effect");
        // A custom look already chosen is put back as it was, and the presets as they were.
        let chosen = parse("[appearance]\nlook = \"presets\"\n[custom]\ncolor = \"matrix\"\neffect = \"material\"\n").unwrap();
        assert_eq!(look_edits(&chosen, Look::Custom, "timo-purple", false, Some("overworld"), None), own(&[(&["appearance", "look"], "custom")]));
        assert_eq!(look_edits(&chosen, Look::Presets, "matrix", true, Some("runic"), None), own(&[(&["appearance", "look"], "presets")]));
        // An older config's one effect over every theme becomes the custom look's effect.
        let old = parse("[effect]\nname = \"material\"\n").unwrap();
        assert_eq!(old.look(), Look::Custom);
        assert_eq!(look_edits(&old, Look::Custom, "navy", true, None, None), own(&[(&["appearance", "look"], "custom"), (&["custom", "color"], "navy"), (&["custom", "font"], "default"), (&["custom", "effect"], "material")]));
    }

    #[test]
    fn the_font_list_offers_the_plain_family_and_each_distinct_set_of_a_theme_s_fonts() {
        use crate::theme::ThemeFont;
        let row = |label: &str, value: Option<&str>, current: bool| Row { label: label.into(), value: value.map(str::to_string), current };
        let set = |prose: &str, ui: &str, terminal: &str| ThemeFont { prose: Some(prose.into()), ui: Some(ui.into()), terminal: Some(terminal.into()), grid: None };
        let (pixel, serif, hand) = (set("Departure Mono", "Departure Mono", "Departure Mono"), set("Baskerville", "Optima", "PT Mono"), set("Chalkboard SE", "Chalkboard SE", "Monaspace Radon"));
        let entry = |name: &str, title: &str, font: ThemeFont| theme::FontEntry { name: name.into(), title: title.into(), font };
        let fonts = vec![entry("pixel", "Pixel", pixel.clone()), entry("serif", "Serif", serif.clone()), entry("hand", "Hand", hand)];
        let has = |f: &str| !f.starts_with("Chalkboard") && !f.starts_with("Monaspace");
        // Each set is a row labelled by its title, and a set this Mac has none of is marked and
        // cannot be picked.
        assert_eq!(
            font_rows(&fonts, Some("pixel"), &serif, "Menlo", &has),
            vec![row("Default (Menlo)", Some("default"), false), row("Pixel", Some("pixel"), true), row("Serif", Some("serif"), false), row("Hand (not installed)", None, false)]
        );
        // `default` is the plain family.
        assert!(font_rows(&fonts, Some("default"), &serif, "Menlo", &has)[0].current);
        // With no font of its own the custom look draws in the fonts of the theme showing.
        assert!(font_rows(&fonts, None, &serif, "Menlo", &has)[2].current);
        assert_eq!(font_rows(&fonts, Some("gone"), &serif, "Menlo", &has)[0], row("gone (not installed)", None, true));
    }

    #[test]
    fn the_custom_colour_list_shows_lavender_once() {
        let colors = theme::colors(std::path::Path::new("/nonexistent"));
        let rows = color_rows(&colors, "navy");
        assert_eq!(rows.iter().filter(|r| r.label.starts_with("Lavender")).count(), 1);
        assert_eq!(rows.len(), 11);
    }

    #[test]
    fn the_custom_colour_row_checked_is_the_scheme_showing_for_an_old_preset_name() {
        let dir = std::path::Path::new("/nonexistent");
        let shown = theme::load("lavender-watercolor", dir).unwrap();
        let rows = color_rows(&theme::colors(dir), &shown.color);
        let checked: Vec<_> = rows.iter().filter(|r| r.current).collect();
        assert_eq!(checked.len(), 1);
        assert_eq!(checked[0].value.as_deref(), Some("lavender"));
    }

    #[test]
    fn the_custom_colour_row_checked_for_an_old_form_copy_of_a_built_in_is_its_scheme_and_none_is_not_installed() {
        let root = tempfile::tempdir().unwrap();
        let themes = root.path().join("themes");
        std::fs::create_dir_all(&themes).unwrap();
        std::fs::write(themes.join("lavender-watercolor.toml"), include_str!("../tests-data/presets-0.8.54/lavender-watercolor.toml")).unwrap();
        std::fs::write(themes.join("lavender-material.toml"), include_str!("../tests-data/presets-0.8.54/lavender-material.toml")).unwrap();
        let entries = theme::colors(&themes);
        let rows = color_rows(&entries, &theme::current_color("lavender-watercolor", &entries, &themes));
        assert!(!rows.iter().any(|r| r.label.contains("not installed")), "{rows:?}");
        let checked: Vec<_> = rows.iter().filter(|r| r.current).collect();
        assert_eq!(checked.len(), 1, "{rows:?}");
        assert!(checked[0].label.starts_with("Lavender"), "{checked:?}");
    }

    #[test]
    fn the_custom_font_list_labels_each_set_by_its_title() {
        let fonts = theme::font_sets(std::path::Path::new("/nonexistent"));
        let rows = font_rows(&fonts, None, &theme::ThemeFont::default(), "Menlo", &|_| true);
        let labels: Vec<&str> = rows.iter().skip(1).map(|r| r.label.as_str()).collect();
        let titles: Vec<&str> = fonts.iter().filter(|f| !f.font.is_default()).map(|f| f.title.as_str()).collect();
        assert_eq!(labels, titles);
    }

    #[test]
    fn adding_a_preset_as_a_colour_writes_its_colours_out_as_a_colour_file() {
        let root = tempfile::tempdir().unwrap();
        let themes = root.path().join("themes");
        std::fs::create_dir_all(&themes).unwrap();
        let src = root.path().join("src.toml");
        std::fs::write(&src, theme::BUILTIN.iter().find(|(n, _)| *n == "peach-watercolor").unwrap().1).unwrap();
        let (name, replaced) = crate::color_schemes::import_file(&src, &themes, false).unwrap();
        assert_eq!((name.as_str(), replaced), ("peach-watercolor", false));
        let written = crate::color_schemes::load("peach-watercolor", &themes).unwrap();
        assert!(written.same_colors(&theme::load("peach-watercolor", std::path::Path::new("/nonexistent")).unwrap()));
        assert!(crate::color_schemes::dir_of(&themes).join("peach-watercolor.toml").is_file());
        assert_eq!(crate::color_schemes::import_file(&src, &themes, false), Err("exists".to_string()));
        assert_eq!(crate::color_schemes::import_file(&src, &themes, true).map(|(_, r)| r), Ok(true));
    }

    #[test]
    fn adding_a_colour_file_copies_it_into_the_colors_folder() {
        let root = tempfile::tempdir().unwrap();
        let themes = root.path().join("themes");
        let src = root.path().join("mine.toml");
        let text = crate::color_schemes::BUILTIN.iter().find(|(n, _)| *n == "solar").unwrap().1.replace("name = \"solar\"", "name = \"mine\"");
        std::fs::write(&src, &text).unwrap();
        let (name, replaced) = crate::color_schemes::import_file(&src, &themes, false).unwrap();
        assert_eq!((name.as_str(), replaced), ("mine", false));
        assert_eq!(std::fs::read_to_string(crate::color_schemes::dir_of(&themes).join("mine.toml")).unwrap(), text);
    }

    #[test]
    fn a_new_colour_pins_the_fonts_on_screen_the_first_time_and_leaves_a_chosen_font_alone() {
        use crate::config::parse;
        let own = |pairs: &[(&[&'static str], &str)]| -> Vec<(Vec<&'static str>, String)> { pairs.iter().map(|(k, v)| (k.to_vec(), v.to_string())).collect() };
        let unpinned = parse("[appearance]\nlook = \"custom\"\n[custom]\ncolor = \"timo-purple\"\neffect = \"overworld\"\n").unwrap();
        assert_eq!(color_edits(&unpinned, "material", "timo-purple", false), own(&[(&["custom", "color"], "material"), (&["custom", "font"], "timo-purple")]));
        assert_eq!(color_edits(&unpinned, "material", "navy", true), own(&[(&["custom", "color"], "material"), (&["custom", "font"], "default")]));
        let pinned = parse("[appearance]\nlook = \"custom\"\n[custom]\ncolor = \"timo-purple\"\nfont = \"peach-watercolor\"\n").unwrap();
        assert_eq!(color_edits(&pinned, "material", "timo-purple", false), own(&[(&["custom", "color"], "material")]));
    }

    #[test]
    fn a_step_moves_by_its_increment_and_clamps_a_value_outside_the_range() {
        assert_eq!(step(16.0, 1.0, SIZE_RANGE), 17.0);
        assert_eq!(step(32.0, 1.0, SIZE_RANGE), 32.0);
        assert_eq!(step(40.0, -1.0, SIZE_RANGE), 32.0, "an out-of-range size is clamped on the first step");
        assert_eq!(step(6.0, 1.0, SIZE_RANGE), 10.0);
        assert_eq!(step(f64::from(1.5f32), 0.1, LINE_RANGE), 1.6);
        assert_eq!(step(f64::from(1.6f32), 0.1, LINE_RANGE), 1.7, "f32 noise does not reach the file");
        assert_eq!(step(2.0, 0.1, LINE_RANGE), 2.0);
        assert_eq!(step(1.0, 0.1, LINE_RANGE), 1.2);
        // The value not stepped is written back as it is, only its f32 noise undone.
        assert_eq!(preset_step(16.0, f64::from(1.55f32), true, SIZE_STEP), (17.0, 1.55));
        assert_eq!(preset_step(f64::from(16.5f32), f64::from(1.55f32), false, LINE_STEP), (16.5, 1.7));
        assert_eq!(preset_step(40.0, 1.5, true, -SIZE_STEP), (32.0, 1.5));
        assert_eq!(number(f64::from(1.55f32)), "1.55", "the steppers show a value as it is");
        assert_eq!(number(16.0), "16");
    }

    #[test]
    fn the_updates_line_names_the_version_and_a_missing_choice_is_listed_first() {
        assert_eq!(ready_line("0.2.0 (def5678)"), "0.2.0 (def5678) is ready");
        let listed = ["navy".to_string(), "peach-watercolor".to_string()];
        assert_eq!(not_installed("nvy", &listed), Some("nvy (not installed)".to_string()));
        assert_eq!(not_installed("navy", &listed), None);
    }

    #[test]
    fn a_missing_chosen_preset_reads_as_the_root_resolves_it() {
        let mut cfg: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        cfg.font.preset = "gone".into();
        cfg.font.presets.insert("aaa".into(), crate::config::TextPreset { size: 12.0, line_height: 1.2 });
        let (_, chosen, size, lh) = preset_rows(&cfg.font, None);
        // Comfortable, as `config::resolve_preset` picks it, not the first name in order.
        assert_eq!((chosen.as_str(), size, lh), ("comfortable", 16.0, 1.5));
        // With a project window, the rows show and step the preset that window uses.
        let cfg: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        let (_, chosen, size, lh) = preset_rows(&cfg.font, Some("compact"));
        assert_eq!((chosen.as_str(), size, lh), ("compact", 14.0, 1.3));
        // A window preset that is gone falls back to the default, as the Root resolves it.
        let (_, chosen, size, lh) = preset_rows(&cfg.font, Some("nope"));
        assert_eq!((chosen.as_str(), size, lh), ("comfortable", 16.0, 1.5));
    }

    #[test]
    fn a_refused_write_leaves_the_controls_live_and_only_a_load_error_makes_them_read_only() {
        let config: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        let shown = Shown { config, error: None, write_error: None, update_ready: None, active_preset: None, sandbox: false };
        assert!(shown.editable());
        let refused = shown.clone().with_write_error("cannot write config.toml: read-only".into());
        assert_eq!(refused.write_error.as_deref(), Some("cannot write config.toml: read-only"));
        assert!(refused.editable(), "a refused write keeps every control live");
        assert!(!Shown { error: Some("line 3: bad".into()), ..shown }.editable(), "a load error makes them read-only");
    }

    #[test]
    fn a_sandbox_leaves_the_login_and_update_switches_dead_and_every_other_control_live() {
        let config: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        let shown = Shown { config, error: None, write_error: None, update_ready: Some("0.2.0".into()), active_preset: None, sandbox: true };
        assert!(shown.editable(), "themes, text and effects are the sandbox's to change");
        assert!(!shown.machine_switches_live(), "start at login, auto-update and Update now do nothing in a sandbox");
        assert!(Shown { sandbox: false, ..shown.clone() }.machine_switches_live());
        assert!(!Shown { sandbox: false, error: Some("line 3: bad".into()), ..shown }.machine_switches_live(), "a load error still makes them read-only");
    }

    #[test]
    fn the_font_check_starts_once_per_process() {
        assert!(claim_fixed_pitch_check(), "the first Settings window starts the check");
        assert!(!claim_fixed_pitch_check(), "a window opened while it runs does not start a second");
    }

    #[test]
    fn symbol_and_hidden_fonts_are_not_offered_as_fixed_pitch() {
        for name in ["Wingdings 2", "Webdings", "Zapf Dingbats", "Symbol", "LastResort", ".SF NS Mono"] {
            assert!(!offerable(name), "{name}");
        }
        for name in ["Menlo", "Courier New", "PT Mono"] {
            assert!(offerable(name), "{name}");
        }
    }

    #[test]
    fn the_privacy_section_names_its_rows_and_their_choices() {
        assert!(Section::ALL.contains(&Section::Privacy));
        assert_eq!(Section::Privacy.label(), "Privacy");
        assert_eq!([USAGE_ROW, CRASHES_ROW, WHAT_IS_SENT_ROW, LAST_REPORT_ROW], ["Usage reports", "Crash reports", "What Arkos sends", "Last report"]);
        assert_eq!(USAGE_CHOICES.map(|(label, _)| label), ["Off", "Local only", "On"]);
        assert_eq!(CRASH_CHOICES.map(|(label, _)| label), ["Never", "Ask each time", "Always"]);
        // The page names the section and its rows as the window does.
        let page = include_str!("../../../docs/telemetry.md").split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(page.contains(&format!("in the {} section: {USAGE_ROW} and {CRASHES_ROW}", Section::Privacy.label())), "the page's list of switches");
        assert!(page.contains(&format!("Settings > {} > **{LAST_REPORT_ROW}**", Section::Privacy.label())), "the page's way to read the last report");
    }

    #[test]
    fn the_settings_window_s_focus_is_read_by_the_project_window() {
        set_settings_key(true);
        assert!(settings_is_key());
        set_settings_key(false);
        assert!(!settings_is_key());
    }

    #[test]
    fn the_privacy_rows_write_the_telemetry_keys() {
        use ::telemetry::visits_app::{Crashes, Usage};
        for ((_, usage), spelled) in USAGE_CHOICES.into_iter().zip(["off", "local", "on"]) {
            let (keys, value) = usage_edit(usage);
            assert_eq!(keys, vec!["telemetry".to_string(), "usage".to_string()]);
            assert_eq!(value.as_str(), Some(spelled));
            let text = config::apply_edits(DEFAULT_CONFIG, &[(&["telemetry", "usage"], value)]).unwrap();
            assert_eq!(config::parse(&text).unwrap().telemetry.usage, usage, "{spelled}");
        }
        for ((_, crashes), spelled) in CRASH_CHOICES.into_iter().zip(["never", "ask", "always"]) {
            let (keys, value) = crashes_edit(crashes);
            assert_eq!(keys, vec!["telemetry".to_string(), "crashes".to_string()]);
            assert_eq!(value.as_str(), Some(spelled));
            let text = config::apply_edits(DEFAULT_CONFIG, &[(&["telemetry", "crashes"], value)]).unwrap();
            assert_eq!(config::parse(&text).unwrap().telemetry.crashes, crashes, "{spelled}");
        }
        assert_eq!(USAGE_CHOICES.map(|(_, u)| u), [Usage::Off, Usage::Local, Usage::On]);
        assert_eq!(CRASH_CHOICES.map(|(_, c)| c), [Crashes::Never, Crashes::Ask, Crashes::Always]);
    }

    /// Turning usage on in Settings is the opt-in: the file it writes, applied by the project
    /// window, reaches the Recorder, which deletes the sessions queued before the consent.
    #[test]
    fn turning_usage_on_in_settings_clears_the_queued_sessions_through_the_recorder() {
        use ::telemetry::visits_app::{new_id, Channel, Usage};
        let dir = tempfile::tempdir().unwrap();
        let now = crate::telemetry::Now::system();
        let mut r = crate::telemetry::Recorder::new(dir.path(), ::telemetry::envelope(::telemetry::Bin::App), ::telemetry::allowlist().clone(), Usage::Local, now);
        let queue = dir.path().join("queue");
        std::fs::create_dir_all(&queue).unwrap();
        let (session, crash) = (queue.join(format!("session-{}.json", new_id())), queue.join(format!("crash-{}.json", new_id())));
        std::fs::write(&session, "{}").unwrap();
        std::fs::write(&crash, "{}").unwrap();
        let old: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        let (_, on) = usage_edit(Usage::On);
        let new = config::parse(&config::apply_edits(DEFAULT_CONFIG, &[(&["telemetry", "usage"], on)]).unwrap()).unwrap();
        let usage = crate::telemetry::mode_of(new.telemetry.usage, new.telemetry.crashes, Channel::Release, None, true).usage;
        assert!(r.config_changed(&old, &new, usage, now).is_some(), "a fresh journal");
        assert!(!session.exists(), "the sessions queued before the consent are gone");
        assert!(crash.exists(), "a crash report stays");
    }

    /// The line under Usage reports: what the mode means, or, when a rule forces it, why, in the
    /// words of the disclosure page.
    #[test]
    fn the_usage_line_says_what_the_mode_means_and_why() {
        use crate::telemetry::mode_of as mode;
        use ::telemetry::visits_app::{Channel, Crashes, Usage};
        let release = |usage| mode(usage, Crashes::Ask, Channel::Release, None, true);
        assert_eq!(usage_line(&release(Usage::Off)), "usage reports are off: nothing is recorded");
        assert_eq!(usage_line(&release(Usage::Local)), "usage reports are local: recorded on this Mac, never sent");
        assert_eq!(usage_line(&release(Usage::On)), "usage reports are on: recorded and sent");
        assert_eq!(usage_line(&mode(Usage::On, Crashes::Ask, Channel::Source, None, true)), "this copy was built from source and sends nothing");
        assert_eq!(usage_line(&mode(Usage::On, Crashes::Ask, Channel::Release, Some(("DO_NOT_TRACK", "1")), true)), "DO_NOT_TRACK is set: nothing is recorded and nothing is sent");
        assert_eq!(usage_line(&mode(Usage::On, Crashes::Ask, Channel::Release, Some(("ARKOS_TELEMETRY", "off")), true)), "ARKOS_TELEMETRY=off: nothing is recorded and nothing is sent");
        assert_eq!(usage_line(&mode(Usage::On, Crashes::Ask, Channel::Release, Some(("ARKOS_TELEMETRY", "log")), true)), "ARKOS_TELEMETRY=log: Arkos records as your settings say and sends nothing");
        assert_eq!(usage_line(&mode(Usage::On, Crashes::Ask, Channel::Release, Some(("CI", "1")), true)), "CI is set: Arkos records as your settings say and sends nothing");
        assert_eq!(usage_line(&mode(Usage::On, Crashes::Ask, Channel::Release, None, false)), "arkos run from a script, with no terminal, never sends and never asks");
        let broken = crate::telemetry::mode_of_broken_config();
        assert_eq!(usage_line(&broken), "the config file cannot be read, so nothing is recorded or sent for that run");
    }

    /// Last report opens `last-session.json` when the daemon has sealed one, and is disabled
    /// until then; a folder or a link in its place is no report.
    #[test]
    fn last_report_is_there_only_once_a_session_was_sealed() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(last_report(dir.path()), None, "no folder");
        let file = dir.path().join("last-session.json");
        std::fs::create_dir_all(&file).unwrap();
        assert_eq!(last_report(dir.path()), None, "a folder is not a report");
        std::fs::remove_dir(&file).unwrap();
        let elsewhere = dir.path().join("elsewhere.json");
        std::fs::write(&elsewhere, "{}").unwrap();
        std::os::unix::fs::symlink(&elsewhere, &file).unwrap();
        assert_eq!(last_report(dir.path()), None, "a link is not the daemon's file");
        std::fs::remove_file(&file).unwrap();
        std::fs::write(&file, "{}").unwrap();
        assert_eq!(last_report(dir.path()), Some(file));
    }

    /// The Privacy section reads the folder when it is shown and when something changed, not
    /// at every render: a receipt written meanwhile shows once the cache is forgotten.
    #[test]
    fn the_privacy_rows_are_read_once_until_something_changes() {
        let dir = tempfile::tempdir().unwrap();
        let shown = Shown { config: Config::default(), error: None, write_error: None, update_ready: None, active_preset: None, sandbox: false };
        let mut cache = PrivacyCache::default();
        assert_eq!(cache.get(dir.path(), &shown).lines, Vec::new());
        assert_eq!(cache.get(dir.path(), &shown).last, None);
        let inc = "0123456789abcdef0123456789abcdef";
        std::fs::write(dir.path().join(crate::crash_dialog::RECEIPTS), format!("{inc} 2026-10-01 declined\n")).unwrap();
        std::fs::write(dir.path().join(telemetry_cli::LAST_SESSION), "{}").unwrap();
        assert_eq!(cache.get(dir.path(), &shown).lines, Vec::new(), "a render reads nothing");
        cache.forget();
        let rows = cache.get(dir.path(), &shown);
        assert_eq!(rows.lines.len(), 1);
        assert_eq!(rows.last, Some(dir.path().join(telemetry_cli::LAST_SESSION)));
        assert_eq!(rows.usage_line, usage_line(&shown_mode(&shown)));
    }

    /// The Reports list is named as the page names it, and lists each receipt by date with its
    /// `inc`, a queued report as having left the queue once its file is gone, and what still
    /// waits.
    #[test]
    fn the_reports_list_shows_the_receipts_by_date_with_the_inc() {
        assert_eq!(REPORTS_ROW, "Reports");
        let page = include_str!("../../../docs/telemetry.md").split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(page.contains(&format!("shown under {REPORTS_ROW} in Settings")));
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(report_lines(dir.path()), Vec::new());
        let line = |date: &str, inc: &str, status| ReportLine { label: format!("{date}  {inc}"), inc: inc.to_string(), status };
        let (a, b) = ("0123456789abcdef0123456789abcdef", "fedcba9876543210fedcba9876543210");
        std::fs::write(dir.path().join(crate::crash_dialog::RECEIPTS), format!("{a} 2026-10-01 queued\n{b} 2026-10-02 declined\n")).unwrap();
        assert_eq!(report_lines(dir.path()), vec![line("2026-10-02", b, "declined"), line("2026-10-01", a, "left the queue")]);
        std::fs::create_dir_all(dir.path().join("queue")).unwrap();
        std::fs::write(dir.path().join("queue").join(format!("crash-{a}.json")), "{}").unwrap();
        assert_eq!(report_lines(dir.path())[1], line("2026-10-01", a, "queued"));
    }
}
