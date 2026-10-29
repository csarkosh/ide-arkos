//! The Settings window: every change written to config.toml in place through one
//! `config::set_values` and applied at once by whoever handed the window its `on_change` (the
//! project window's `reload_config`, or in the launch chooser the write alone). It shows what it
//! was last handed (`set_shown`), so an edit made in an editor shows here too.
use crate::chrome::{self, TITLE_BAR_H, UI_FONT};
use crate::config::{self, AppearanceMode, Config, EffectChoice, FontConfig, StartAtLogin, WideTables};
use crate::theme::{self, Theme};
use crate::{aux, effects, keymap, paths, telemetry_cli};
use ::telemetry::visits_app::{Crashes, Mode, Usage};
use gpui::{
    div, prelude::*, px, size, AnyElement, App, Bounds, ClipboardItem, Context, ElementId, FocusHandle, Hsla, KeyDownEvent, PathPromptOptions, SharedString,
    Stateful, TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions,
};
use std::path::{Path, PathBuf};
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
/// The effect's Intensity stepper: 0.0 to 1.0 by a tenth.
const INTENSITY_RANGE: (f64, f64) = (0.0, 1.0);
const INTENSITY_STEP: f64 = 0.1;

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

/// Whether this window should start the advance check: true once per run, and never once its
/// result is in.
fn claim_fixed_pitch_check() -> bool {
    FIXED_PITCH.get().is_none() && !FIXED_PITCH_STARTED.swap(true, Ordering::SeqCst)
}

/// The sections of the left column, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Appearance,
    Text,
    Startup,
    Updates,
    Privacy,
    Keyboard,
}

impl Section {
    const ALL: [Section; 6] = [Section::Appearance, Section::Text, Section::Startup, Section::Updates, Section::Privacy, Section::Keyboard];

    fn label(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
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

/// One row of the Effect list: its label, the `effect.name` a click writes (`None`: not
/// clickable), and whether it is the configured choice.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectRow {
    pub label: String,
    pub value: Option<String>,
    pub current: bool,
}

/// The Effect list: "Theme's choice" first, naming what the theme recommends, then "None", then
/// a configured effect that is not installed, then every effect `listed`, an invalid one shown
/// but not clickable.
pub fn effect_rows(choice: &EffectChoice, recommended: Option<&str>, listed: &[(String, effects::manifest::Source)]) -> Vec<EffectRow> {
    let row = |label: String, value: Option<&str>, current: bool| EffectRow { label, value: value.map(str::to_string), current };
    let mut rows = vec![
        row(format!("Theme's choice ({})", recommended.unwrap_or("none")), Some("theme"), *choice == EffectChoice::Theme),
        row("None".into(), Some("none"), *choice == EffectChoice::None),
    ];
    if let EffectChoice::Named(name) = choice {
        let names: Vec<String> = listed.iter().map(|(n, _)| n.clone()).collect();
        rows.extend(not_installed(name, &names).map(|label| row(label, None, true)));
    }
    for (name, source) in listed {
        rows.push(match source {
            effects::manifest::Source::Invalid(_) => row(format!("{name} (invalid)"), None, false),
            _ => row(name.clone(), Some(name), *choice == EffectChoice::Named(name.clone())),
        });
    }
    rows
}

/// The edit for the Intensity stepper.
fn intensity_edit(value: f64) -> Edit {
    edit(&["effect", "intensity"], value)
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
    themes: Vec<(String, theme::Source)>,
    /// The effects folder and the built-ins, as `effects::manifest::list` reads them.
    effects: Vec<(String, effects::manifest::Source)>,
    fonts: Vec<String>,
    /// Why the last "Add theme…" failed, shown under the button.
    import_error: Option<String>,
    /// A picked theme file whose name is already in the themes folder, and that name, while the
    /// window asks whether to replace it.
    pending_replace: Option<(PathBuf, String)>,
    /// The file picker is up.
    picking: bool,
    focus: FocusHandle,
    theme: Theme,
    on_change: OnChange,
    on_update: OnUpdate,
    /// The Privacy section's rows read from the folder, kept between renders.
    privacy: PrivacyCache,
}

/// Opens the Settings window centred on the main display and gives it the keyboard. `fonts` is
/// the list shown until the advance check has run. `None` when gpui could not open it, which the
/// log records.
pub fn open(cx: &mut App, shown: Shown, theme: Theme, fonts: Vec<String>, on_change: OnChange, on_update: OnUpdate) -> Option<WindowHandle<SettingsWindow>> {
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
        let view = cx.new(|cx| {
            // Foreground time in Settings counts while its window is the key window, and stops
            // when it closes.
            crate::telemetry::settings_focus(true);
            // Coming to the front reads the Privacy rows again: the daemon may have sent a report
            // meanwhile.
            cx.observe_window_activation(window, |w: &mut SettingsWindow, window, cx| {
                let active = window.is_window_active();
                crate::telemetry::settings_focus(active);
                if active {
                    w.privacy.forget();
                    cx.notify();
                }
            })
            .detach();
            cx.on_release(|_, _| crate::telemetry::settings_focus(false)).detach();
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
                themes: theme::list(&paths::themes_dir()),
                effects: effects::manifest::list(&effects::effects_dir()),
                fonts,
                import_error: None,
                pending_replace: None,
                picking: false,
                focus: cx.focus_handle(),
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
        self.themes = theme::list(&paths::themes_dir());
        self.effects = effects::manifest::list(&effects::effects_dir());
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

    /// "Add theme…": the native picker for one file; the file chosen is imported.
    fn add_theme(&mut self, system_is_dark: bool, cx: &mut Context<Self>) {
        if self.picking {
            return;
        }
        self.picking = true;
        let rx = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Add theme".into()) });
        cx.spawn(async move |view, cx| {
            let picked = rx.await;
            let _ = view.update(cx, |w, cx| {
                w.picking = false;
                match picked {
                    Ok(Ok(Some(paths))) => {
                        if let Some(src) = paths.into_iter().next() {
                            w.import(src, false, system_is_dark, cx);
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

    /// Copies a theme file into the themes folder and selects it for the current mode; a name
    /// already there asks first.
    fn import(&mut self, src: PathBuf, replace: bool, system_is_dark: bool, cx: &mut Context<Self>) {
        self.pending_replace = None;
        self.import_error = None;
        match theme::import_file(&src, &paths::themes_dir(), replace) {
            Ok((name, _)) => {
                self.themes = theme::list(&paths::themes_dir());
                let key = theme_key_for(self.shown.config.appearance.mode, system_is_dark);
                self.change(vec![edit(&["theme", key], name.as_str())], cx);
            }
            Err(e) if e == "exists" => match theme::validate_file(&src) {
                Ok(name) => self.pending_replace = Some((src, name)),
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
}

impl Colors {
    /// The controls' colours in `theme`.
    pub fn of(theme: &Theme) -> Colors {
        let t = &theme.chrome;
        Colors { text: t.text.hsla(), dim: t.text_dim.hsla(), border: t.border.hsla(), accent: t.focus_ring.hsla(), pill: t.banner.hsla() }
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

/// A pill button, drawn like the terminal's "jump to bottom" pill. A selected one is outlined in
/// the accent colour; a disabled one is dimmed and the caller attaches no click handler.
pub fn pill(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool, enabled: bool, c: Colors) -> Stateful<gpui::Div> {
    let p = div()
        .id(id)
        .flex_none()
        .px(px(10.0))
        .py(px(3.0))
        .rounded(px(10.0))
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
        .rounded(px(4.0))
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
    fn appearance(&self, enabled: bool, c: Colors, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let cfg = &self.shown.config;
        let mut out = vec![heading("Mode", c).into_any_element()];
        let mut modes = div().flex().gap(px(6.0));
        for (ix, (label, mode, value)) in [("System", AppearanceMode::System, "system"), ("Dark", AppearanceMode::Dark, "dark"), ("Light", AppearanceMode::Light, "light")].into_iter().enumerate() {
            let current = cfg.appearance.mode == mode;
            let mut p = pill(("mode", ix), label, current, enabled, c);
            if enabled && !current {
                p = p.on_click(cx.listener(move |w, _, _, cx| w.change(vec![edit(&["appearance", "mode"], value)], cx)));
            }
            modes = modes.child(p);
        }
        out.push(modes.into_any_element());
        for (key, title, list_id, current) in [("dark", "Dark theme", "theme-dark", &cfg.theme.dark), ("light", "Light theme", "theme-light", &cfg.theme.light)] {
            out.push(heading(title, c).into_any_element());
            let mut list = div().flex().flex_col();
            let names: Vec<String> = self.themes.iter().map(|(n, _)| n.clone()).collect();
            if let Some(missing) = not_installed(current, &names) {
                list = list.child(list_row((list_id, usize::MAX), missing, true, enabled, c));
            }
            for (ix, (name, source)) in self.themes.iter().enumerate() {
                let id = (list_id, ix);
                let row = match source {
                    theme::Source::Invalid(_) => list_row(id, format!("{name} (invalid)"), false, false, c),
                    _ => {
                        let is_current = name == current;
                        let mut row = list_row(id, name.clone(), is_current, enabled, c);
                        if enabled && !is_current {
                            let name = name.clone();
                            row = row.on_click(cx.listener(move |w, _, _, cx| w.change(vec![edit(&["theme", key], name.as_str())], cx)));
                        }
                        row
                    }
                };
                list = list.child(row);
            }
            out.push(list.into_any_element());
        }
        let mut add = pill("add-theme", "Add theme…", false, enabled && !self.picking, c);
        if enabled && !self.picking {
            add = add.on_click(cx.listener(|w, _, window, cx| w.add_theme(theme::is_dark(window.appearance()), cx)));
        }
        let mut folder = pill("themes-folder", "Show themes folder", false, enabled, c);
        if enabled {
            folder = folder.on_click(|_, _, cx| {
                let dir = paths::themes_dir();
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    eprintln!("arkos: cannot create {}: {e}", dir.display());
                }
                cx.reveal_path(&dir);
            });
        }
        out.push(div().pt(px(10.0)).flex().gap(px(6.0)).child(add).child(folder).into_any_element());
        if let Some((src, name)) = &self.pending_replace {
            let src = src.clone();
            let (mut replace, mut keep) = (pill("replace", "Replace", false, enabled, c), pill("keep", "Keep", false, enabled, c));
            if enabled {
                replace = replace.on_click(cx.listener(move |w, _, window, cx| w.import(src.clone(), true, theme::is_dark(window.appearance()), cx)));
                keep = keep.on_click(cx.listener(|w, _, _, cx| {
                    w.pending_replace = None;
                    cx.notify();
                }));
            }
            out.push(div().flex().items_center().gap(px(6.0)).child(format!("Replace the existing {name}?")).child(replace).child(keep).into_any_element());
        }
        if let Some(e) = &self.import_error {
            out.push(div().text_color(c.dim).child(e.clone()).into_any_element());
        }
        out.extend(self.effect_controls(enabled, c, cx));
        out
    }

    /// The Appearance section's effect controls, under the themes: the Effect list, the
    /// Intensity stepper and "Show effects folder".
    fn effect_controls(&self, enabled: bool, c: Colors, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let cfg = &self.shown.config.effect;
        let mut out = vec![heading("Effect", c).into_any_element()];
        let mut list = div().flex().flex_col();
        for (ix, r) in effect_rows(&cfg.name, self.theme.recommended_effect(), &self.effects).into_iter().enumerate() {
            let clickable = enabled && r.value.is_some();
            let mut row = list_row(("effect", ix), r.label, r.current, clickable, c);
            if let (true, false, Some(value)) = (clickable, r.current, r.value) {
                row = row.on_click(cx.listener(move |w, _, _, cx| w.change(vec![edit(&["effect", "name"], value.as_str())], cx)));
            }
            list = list.child(row);
        }
        out.push(list.into_any_element());
        out.push(heading("Intensity", c).into_any_element());
        let value = f64::from(cfg.intensity);
        let press = |ix: usize, sign: &'static str, delta: f64| {
            let mut p = pill(("intensity-step", ix), sign, false, enabled, c);
            if enabled {
                p = p.on_click(cx.listener(move |w, _, _, cx| w.change(vec![intensity_edit(step(value, delta, INTENSITY_RANGE))], cx)));
            }
            p
        };
        let (minus, plus) = (press(0, "−", -INTENSITY_STEP), press(1, "+", INTENSITY_STEP));
        let shown = div().min_w(px(28.0)).flex().justify_center().child(number(value));
        out.push(div().flex().items_center().gap(px(8.0)).child(minus).child(shown).child(plus).into_any_element());
        let mut folder = pill("effects-folder", "Show effects folder", false, enabled, c);
        if enabled {
            folder = folder.on_click(|_, _, cx| {
                let dir = effects::effects_dir();
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    eprintln!("arkos: cannot create {}: {e}", dir.display());
                }
                cx.reveal_path(&dir);
            });
        }
        out.push(div().pt(px(10.0)).flex().gap(px(6.0)).child(folder).into_any_element());
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
                row = row.on_click(cx.listener(move |w, _, _, cx| w.change(vec![edit(&["font", "family"], name.as_str())], cx)));
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
                row = row.on_click(cx.listener(move |w, _, _, cx| w.change(vec![edit(&["font", "preset"], name.as_str())], cx)));
            }
            presets = presets.child(row);
        }
        out.push(presets.into_any_element());
        // Both steppers write the preset whole, since a preset needs its size and line height.
        let (size, line_height) = (f64::from(size), f64::from(line_height));
        let steppers = [("Size", "size-step", size, SIZE_STEP, true), ("Line height", "line-step", line_height, LINE_STEP, false)];
        for (title, id, value, increment, is_size) in steppers {
            out.push(heading(title, c).into_any_element());
            let press = |ix: usize, sign: &'static str, delta: f64| {
                let mut p = pill((id, ix), sign, false, enabled, c);
                if enabled {
                    let name = chosen.clone();
                    p = p.on_click(cx.listener(move |w, _, _, cx| {
                        let (s, l) = preset_step(size, line_height, is_size, delta);
                        w.change(vec![edit(&["font", "presets", &name, "size"], s), edit(&["font", "presets", &name, "line_height"], l)], cx);
                    }));
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
                p = p.on_click(cx.listener(move |w, _, _, cx| w.change(vec![wide_tables_edit(mode)], cx)));
            }
            modes = modes.child(p);
        }
        out.push(modes.into_any_element());
        out
    }

    /// An On / Off switch writing `on` and `off` through `write`.
    fn switch(&self, id: &'static str, on: Option<bool>, enabled: bool, c: Colors, cx: &mut Context<Self>, write: fn(bool) -> Edit) -> AnyElement {
        let mut row = div().flex().gap(px(6.0));
        for (label, value) in [("On", true), ("Off", false)] {
            let current = on == Some(value);
            let mut p = pill((id, usize::from(value)), label, current || (on.is_none() && !value), enabled, c);
            if enabled && !current {
                p = p.on_click(cx.listener(move |w, _, _, cx| w.change(vec![write(value)], cx)));
            }
            row = row.child(p);
        }
        row.into_any_element()
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
                now = now.on_click(move |_, _, cx| {
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
                p = p.on_click(cx.listener(move |w, _, _, cx| w.change(vec![usage_edit(value)], cx)));
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
                p = p.on_click(cx.listener(move |w, _, _, cx| w.change(vec![crashes_edit(value)], cx)));
            }
            crashes = crashes.child(p);
        }
        out.push(crashes.into_any_element());
        // Both stay live while the config has a problem: they only read.
        let sends = pill("what-arkos-sends", WHAT_IS_SENT_ROW, false, true, c).on_click(|_, _, cx| aux::open_disclosure_from_settings(cx));
        let mut report = pill("last-report", LAST_REPORT_ROW, false, last.is_some(), c);
        if let Some(file) = last.clone() {
            report = report.on_click(move |_, _, cx| aux::open_last_report(cx, file.clone()));
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
                .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(inc.clone())))
                .child(label)
                .child(div().text_color(c.dim).child(status));
            out.push(row.into_any_element());
        }
        out
    }

    fn keyboard(&self, c: Colors) -> Vec<AnyElement> {
        // Live even while the config has a problem: opening the file is how it gets fixed.
        let open = pill("open-config", "Open config", false, true, c).on_click(|_, _, cx| open_config_in_editor(cx));
        // `open_keybindings` defers its work itself, so this window is not being updated when the
        // Keybindings window reads the Root.
        let list = pill("open-keybindings", "Keybindings", false, true, c).on_click(|_, _, cx| aux::open_keybindings(cx));
        vec![
            div().flex().items_center().gap(px(8.0)).child("Keybindings live in config.toml").child(open).into_any_element(),
            div().flex().items_center().gap(px(8.0)).child("Every binding is listed under Help, Keybindings").child(list).into_any_element(),
        ]
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = &self.theme.chrome;
        let c = Colors::of(&self.theme);
        let (bg, sidebar) = (t.window_bg.hsla(), t.sidebar.hsla());
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
                .on_click(cx.listener(move |w, _, _, cx| {
                    w.section = section;
                    // A section shown reads what it shows afresh.
                    w.privacy.forget();
                    cx.notify();
                }))
                .child(section.label());
            sections = sections.child(row);
        }
        let controls = match self.section {
            Section::Appearance => self.appearance(enabled, c, cx),
            Section::Text => self.text(enabled, c, cx),
            Section::Startup => self.startup(c, cx),
            Section::Updates => self.updates(c, cx),
            Section::Privacy => self.privacy(enabled, c, cx),
            Section::Keyboard => self.keyboard(c),
        };
        let banner = self.shown.error.clone().map(|e| div().flex_none().px(px(10.0)).py(px(6.0)).rounded(px(4.0)).bg(c.pill).text_color(c.text).child(e));
        // A refused write is said above the controls, which stay live: the next write may succeed.
        let write_error = self.shown.write_error.clone().map(|e| div().flex_none().pb(px(6.0)).text_color(c.dim).child(e));
        let content = div()
            .id("settings-controls")
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
            .on_key_down(|ev: &KeyDownEvent, window, _| {
                if ev.keystroke.key == "escape" && !ev.keystroke.modifiers.modified() {
                    window.remove_window();
                }
            })
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
    fn the_effect_list_offers_the_theme_s_choice_none_and_each_effect_and_marks_the_current_one() {
        use crate::config::EffectChoice;
        use crate::effects::manifest::Source;
        let listed = vec![
            ("watercolor".to_string(), Source::BuiltIn),
            ("neon".to_string(), Source::Folder("/e/neon".into())),
            ("broken".to_string(), Source::Invalid("scale must be 0.1 to 1.0".into())),
        ];
        let rows = |choice: &EffectChoice, recommended: Option<&str>| -> Vec<(String, Option<String>, bool)> {
            effect_rows(choice, recommended, &listed).into_iter().map(|r| (r.label, r.value, r.current)).collect()
        };
        let row = |label: &str, value: Option<&str>, current: bool| (label.to_string(), value.map(str::to_string), current);
        assert_eq!(
            rows(&EffectChoice::Theme, Some("watercolor")),
            vec![
                row("Theme's choice (watercolor)", Some("theme"), true),
                row("None", Some("none"), false),
                row("watercolor", Some("watercolor"), false),
                row("neon", Some("neon"), false),
                row("broken (invalid)", None, false),
            ]
        );
        let named = rows(&EffectChoice::Named("neon".into()), None);
        assert_eq!(named[0], row("Theme's choice (none)", Some("theme"), false));
        assert_eq!(named[3], row("neon", Some("neon"), true));
        assert_eq!(rows(&EffectChoice::None, None)[1], row("None", Some("none"), true));
        // A configured effect that is not installed is listed after None, current and not
        // clickable.
        let gone = rows(&EffectChoice::Named("gone".into()), None);
        assert_eq!(gone[2], row("gone (not installed)", None, true));
        assert_eq!(gone.len(), 6);
    }

    #[test]
    fn the_intensity_stepper_moves_by_a_tenth_within_zero_and_one() {
        assert_eq!(step(1.0, INTENSITY_STEP, INTENSITY_RANGE), 1.0);
        assert_eq!(step(f64::from(0.7f32), -INTENSITY_STEP, INTENSITY_RANGE), 0.6);
        assert_eq!(step(0.0, -INTENSITY_STEP, INTENSITY_RANGE), 0.0);
        assert_eq!(step(f64::from(0.25f32), INTENSITY_STEP, INTENSITY_RANGE), 0.4, "rounded to a tenth");
        let (keys, value) = intensity_edit(0.6);
        assert_eq!(keys, vec!["effect".to_string(), "intensity".to_string()]);
        assert_eq!(value.as_float(), Some(0.6));
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
        let listed = ["navy".to_string(), "peach-light".to_string()];
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

    /// Like the Wide tables row: each choice writes its key as the config spells it, and the
    /// edited default file reads back as that choice.
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
