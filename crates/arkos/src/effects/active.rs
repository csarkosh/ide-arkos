//! An effect while the window shows it (spec 4.3 and 4.4): its layer, its clock, the fade that
//! brings it in and takes it away, and the style the app's surfaces paint with meanwhile. Pure
//! apart from the layer it holds, so the style, the cadence and the fade are tested.

use std::time::{Duration, Instant};

use gpui::GlyphShader;

use super::blocks::{Mark, Scene, Shape};
use super::inputs::Inputs;
use super::layer::Layer;
use super::manifest::{Effect, Highlights, Manifest, MenuLook};
use crate::motion;
use crate::palette::Rgb;
use crate::theme::{self, Theme};
use crate::tree::{PaneId, Rect};

/// Whether an effect is coming or going.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    In,
    Out,
}

/// How the app paints its own surfaces over the wash: the alphas of its fills, whether it still
/// draws its borders and highlights, and how much larger its UI text is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// A terminal's or a document's paper.
    pub paper_alpha: f32,
    /// The title bar, the tab bars, the inactive tabs and the sidebar.
    pub chrome_alpha: f32,
    /// The active tab, a lighter patch on its bar.
    pub active_tab_alpha: f32,
    /// The window's own background and each pane's body: opaque without an effect and gone
    /// under one, so nothing but the wash shows between the surfaces.
    pub ground_alpha: f32,
    /// The effect draws the edges, so the app draws no borders of its own.
    pub ink_borders: bool,
    /// The effect paints the selected and hovered rows and the hovered tab.
    pub highlights: bool,
    /// Points added to the project window's UI text.
    pub ui_text_bump: f32,
    /// How the menus and popovers look (`chrome::menu_colors`).
    pub menus: MenuLook,
    /// The effect draws the menus' and popovers' edges from above (its wash's `arkos_overlay`),
    /// so the app draws no line of its own around them.
    pub menu_edges: bool,
}

impl Style {
    /// The app as it paints without an effect.
    pub fn plain() -> Style {
        Style { paper_alpha: 1.0, chrome_alpha: 1.0, active_tab_alpha: 1.0, ground_alpha: 1.0, ink_borders: false, highlights: false, ui_text_bump: 0.0, menus: MenuLook::App, menu_edges: false }
    }

    /// A UI text size of `pt` points as this style draws it.
    pub fn ui(&self, pt: f32) -> f32 {
        pt + self.ui_text_bump
    }
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

/// The style `fade` of the way from plain (0) to the manifest's own (1). The alphas move with
/// the fade; the borders, the highlights, the text size and the menus' look can only cut, so they are the
/// manifest's only once the fade is complete.
pub fn style_at(manifest: &Manifest, fade: f32) -> Style {
    let f = fade.clamp(0.0, 1.0);
    let full = f >= 1.0;
    Style {
        paper_alpha: lerp(1.0, manifest.paper_alpha.max(0.5), f),
        chrome_alpha: lerp(1.0, manifest.chrome_alpha, f),
        active_tab_alpha: lerp(1.0, manifest.active_tab_alpha, f),
        ground_alpha: 1.0 - f,
        ink_borders: full && manifest.ink_borders,
        highlights: full && manifest.highlights == Highlights::Effect,
        ui_text_bump: if full { manifest.ui_text_bump } else { 0.0 },
        menus: if full { manifest.menus } else { MenuLook::App },
        // Only the effect's wash knows whether it has a menu stage: `ActiveEffect::style`.
        menu_edges: false,
    }
}

/// How long the wash's timer waits between frames: the manifest's `frame_rate` at rest, sixty a
/// second while something moves (a highlight fading), so a short fade is more than a few frames.
pub fn cadence(frame_rate: u32, moving: bool) -> Duration {
    let rate = if moving { 60 } else { frame_rate.clamp(1, 60) };
    Duration::from_secs_f64(1.0 / rate as f64)
}

/// How long the wash's timer rests between looks while nothing of the wash can be seen: long
/// enough that a hidden, occluded or covered window costs next to nothing, short enough that
/// the wash comes back soon after the window does.
pub const REST: Duration = Duration::from_millis(500);

/// What the wash's timer waits after a tick, or `None` to stop it. While the window is not
/// `seen` (hidden, occluded or covered) it rests; under Reduce Motion with no highlight
/// `moving` and no `fading` it stops, and the next render starts it again; otherwise it keeps
/// the `cadence`.
pub fn next_wait(frame_rate: u32, seen: bool, moving: bool, fading: bool, reduce: bool) -> Option<Duration> {
    if !seen {
        Some(REST)
    } else if reduce && !moving && !fading {
        None
    } else {
        Some(cadence(frame_rate, moving))
    }
}

/// The most frames a second the text is redrawn at while a glyph shader moves: gpui redraws
/// every view for each, so it is kept well under the wash's top rate.
pub const GLYPH_RATE_CAP: u32 = 20;

/// What the text's timer waits after a frame, or `None` to stop it. The text is redrawn only
/// while the effect's glyph shader `motion` is on, the effect is `settled` (not fading in or
/// out), the window is `seen` and Reduce Motion is off; the wait is one frame at the manifest's
/// `frame_rate`, at most `GLYPH_RATE_CAP` a second.
pub fn glyph_wait(frame_rate: u32, motion: bool, settled: bool, seen: bool, reduce: bool) -> Option<Duration> {
    if motion && settled && seen && !reduce {
        Some(Duration::from_secs_f64(1.0 / frame_rate.clamp(1, GLYPH_RATE_CAP) as f64))
    } else {
        None
    }
}

/// What a frame's blocks are made from in the project window: the panes' content area, the
/// window's bars, the sidebar's divider when laid out, the panes, the shapes laid out inside
/// them and the highlights' strokes.
pub fn scene<'a>(content: Rect, divider_y: Option<f32>, panes: &'a [(PaneId, Rect)], shapes: &'a [Shape], strokes: &'a [(Mark, f32)]) -> Scene<'a> {
    Scene { content, title_bar_h: crate::chrome::TITLE_BAR_H, tab_bar_h: crate::pane_view::TAB_BAR_H, divider_y, panes, shapes, strokes }
}

/// Whether the GPU's time per frame is worth a line in the window log: the first reading, then
/// at most once a minute and only when it moved by more than a tenth.
pub fn should_log(last: Option<(Instant, f32)>, now: Instant, ms: f32) -> bool {
    match last {
        None => true,
        Some((at, was)) => now.duration_since(at) >= Duration::from_secs(60) && (ms - was).abs() > was * 0.1,
    }
}

/// The notice for an effect whose shader did not compile: the file and the line of the
/// compiler's first error, or its message as it is when it names none.
pub fn compile_notice(name: &str, file: &str, message: &str) -> String {
    let first = message.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("program_source:")?;
        let (line_no, rest) = rest.split_once(':')?;
        let (_column, rest) = rest.split_once(':')?;
        let text = rest.trim();
        let text = text.strip_prefix("error:").map(str::trim).unwrap_or(text);
        Some(format!("{file}:{line_no}: {text}"))
    });
    match first {
        Some(at) => format!("effect {name}: {at}"),
        None => format!("effect {name}: {}", message.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("could not be drawn")),
    }
}

/// The theme an effect with `text_contrast = c` shows: the text `c`, the dim text three quarters
/// of `c` and the terminal's foreground nine tenths of `c` of the way towards ink, black on a
/// light theme and white on a dark one. Every other colour is the theme's own.
pub fn contrasted(theme: &Theme, c: f32) -> Theme {
    let ink = if theme.is_light() { Rgb { r: 0, g: 0, b: 0 } } else { Rgb { r: 255, g: 255, b: 255 } };
    let c = c.clamp(0.0, 1.0);
    let mut out = theme.clone();
    out.chrome.text = theme::mix(theme.chrome.text, ink, c);
    out.chrome.text_dim = theme::mix(theme.chrome.text_dim, ink, 0.75 * c);
    out.terminal.foreground = theme::mix(theme.terminal.foreground, ink, 0.9 * c);
    out
}

/// Which of the app's windows a glyph shader is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    Project,
    // The aux windows never ask: they keep gpui's own glyphs. Named so the rule is tested.
    #[cfg_attr(not(test), allow(dead_code))]
    About,
    #[cfg_attr(not(test), allow(dead_code))]
    Settings,
    #[cfg_attr(not(test), allow(dead_code))]
    Keybindings,
    #[cfg_attr(not(test), allow(dead_code))]
    Chooser,
}

/// Where the glyph clock wraps, about an hour (1260 periods of a 2.2 radians a second ripple, so
/// a glyph shader moving at that rate does not jump at the wrap). No built-in glyph shader reads
/// the clock; it is there for an effect whose `glyph_motion` is true.
pub const GLYPH_CLOCK_WRAP: f64 = 1260.0 * std::f64::consts::TAU / 2.2;

/// Where a window's glyph stages are set: gpui's window, or a test's stand-in.
pub trait GlyphSink {
    fn set_glyph_shader(&mut self, shader: Option<GlyphShader>) -> Result<(), String>;
    /// Sets the seconds the glyph stages read at Metal buffer index 5 on their next draws.
    fn set_glyph_time(&mut self, seconds: f32);
}

impl GlyphSink for gpui::Window {
    fn set_glyph_shader(&mut self, shader: Option<GlyphShader>) -> Result<(), String> {
        gpui::Window::set_glyph_shader(self, shader)
    }
    fn set_glyph_time(&mut self, seconds: f32) {
        gpui::Window::set_glyph_shader_time(self, seconds);
    }
}

/// The line that tells a glyph shader the window's pixels per point, `ARKOS_SCALE`: a glyph
/// shader works in device pixels, so one that measures its ink in points looks the same on a 1x
/// screen as on a Retina one.
pub fn scale_line(scale: f32) -> String {
    format!("constant float ARKOS_SCALE = {scale:.2};")
}

/// An effect's glyph shader as a window of `scale` compiles it: the scale's line, then its own.
pub fn glyph_source(glyphs: &str, scale: f32) -> String {
    format!("{}\n{glyphs}", scale_line(scale))
}

/// The glyph shader a window of `kind` at `scale` draws with under `effect`: the effect's own,
/// only in the project window, only once it has faded in fully and until it starts to leave
/// (glyphs can only cut), and only when the effect has a glyphs file.
pub fn glyph_shader_for<L>(kind: WindowKind, effect: Option<&ActiveEffect<L>>, scale: f32) -> Option<GlyphShader> {
    let e = effect.filter(|e| kind == WindowKind::Project && e.settled())?;
    let glyphs = e.effect.glyphs.as_deref()?;
    Some(GlyphShader {
        source: glyph_source(glyphs, scale),
        vertex: "arkos_glyph_vertex".into(),
        fragment: "arkos_glyph_fragment".into(),
        emoji: emoji_stages(glyphs),
    })
}

/// Whether a tick looks at the effect at all: never while the window is hidden; while one of the
/// app's own menus or prompts covers the panes the wash rests, but a tick still runs while a menu
/// is open, so the menu stage draws its edges (`covered`: `Root::covered`).
pub fn ticks(hidden: bool, covered: bool, menus_open: bool) -> bool {
    !hidden && (!covered || menus_open)
}

/// Whether a wash has a menu stage, `arkos_overlay`, which draws the menus' and popovers' edges
/// in a layer above the panes.
pub fn has_overlay(wash: &str) -> bool {
    wash.contains("arkos_overlay(")
}

/// The emoji stages a glyphs file asks for: when it defines `arkos_emoji_fragment`, the window's
/// emoji are drawn with it (pictures never are), placed by its own `arkos_emoji_vertex` if it
/// has one, else by gpui's. A file without one leaves emoji as gpui draws them.
pub fn emoji_stages(glyphs: &str) -> Option<gpui::EmojiStages> {
    glyphs.contains("arkos_emoji_fragment(").then(|| gpui::EmojiStages {
        vertex: if glyphs.contains("arkos_emoji_vertex(") { "arkos_emoji_vertex" } else { "polychrome_sprite_vertex" }.into(),
        fragment: "arkos_emoji_fragment".into(),
    })
}

/// What a window reports drawing under it (`arkos effect current`): the effect's name and the
/// GPU's time per frame, `gpu_ms` of its layer, while it is on or coming in; nothing once it is
/// leaving.
pub fn reported<L>(effect: Option<&ActiveEffect<L>>, gpu_ms: impl Fn(&L) -> Option<f32>) -> Option<(String, Option<f32>)> {
    let e = effect.filter(|e| !e.leaving())?;
    Some((e.effect.manifest.name.clone(), gpu_ms(&e.layer)))
}

/// Sets `shader` on `window`. A shader the window refuses gives the notice to show, naming the
/// effect's glyphs file and the compiler's first error, and the window goes back to gpui's own
/// glyphs, so no earlier effect's shader is left drawing under this one.
pub fn apply_glyphs(window: &mut impl GlyphSink, effect: &Effect, shader: Option<GlyphShader>) -> Result<(), String> {
    let had_shader = shader.is_some();
    window.set_glyph_shader(shader).map_err(|message| {
        if had_shader {
            // gpui's own glyphs need no compiling, so this is not refused.
            let _ = window.set_glyph_shader(None);
        }
        let file = effect.manifest.glyphs.as_deref().unwrap_or("glyphs.metal");
        let first = message
            .lines()
            .map(str::trim)
            .find(|l| l.contains("error"))
            .or_else(|| message.lines().map(str::trim).find(|l| !l.is_empty()))
            .unwrap_or("could not be drawn");
        // The compiler's line numbers count gpui's own source first, so only the text is kept.
        let text = first.split_once("error:").map_or(first, |(_, rest)| rest.trim());
        format!("effect {}: {file}: {text}", effect.manifest.name)
    })
}

/// What puts an effect's layer in the window. The window's own puts a Metal layer under gpui's
/// view; a test's stands in for it.
pub trait Installer {
    type Layer;
    /// The effect's layer, or the compiler's message (its lines pointing into the wash file).
    fn install(&mut self, effect: &Effect) -> Result<Self::Layer, String>;
}

/// An effect on the window, or on its way in or out.
pub struct ActiveEffect<L = Layer> {
    pub effect: Effect,
    pub layer: L,
    /// When it was put on: the shader's clock starts here.
    pub started: Instant,
    /// What the layer last drew, so a still window under Reduce Motion draws nothing more.
    pub last_inputs: Option<Inputs>,
    /// What the menu stage last drew; `None` while its layer is hidden.
    pub last_overlay: Option<Inputs>,
    /// A fade under way: since when, and which way.
    pub fade: Option<(Instant, Direction)>,
    /// When the GPU's time per frame was last written to the window log, and what it was.
    pub logged: Option<(Instant, f32)>,
}

/// Installs `effect` and starts it fading in at `now`, or at once when `cut` (Reduce Motion, or
/// a window not yet drawn). A shader that does not compile gives the notice to show instead.
pub fn start<I: Installer>(effect: Effect, installer: &mut I, now: Instant, cut: bool) -> Result<ActiveEffect<I::Layer>, String> {
    match installer.install(&effect) {
        Ok(layer) => Ok(ActiveEffect { effect, layer, started: now, last_inputs: None, last_overlay: None, fade: if cut { None } else { Some((now, Direction::In)) }, logged: None }),
        Err(message) => Err(compile_notice(&effect.manifest.name, &effect.manifest.wash, &message)),
    }
}

/// When a fade towards `amount` (0 to 1 along its way) must have started for it to be there at
/// `now`, so a fade reversed midway continues from what is on screen.
fn since_for(now: Instant, amount: f32) -> Instant {
    // The fade's curve is smoothstep: find where it reaches `amount` by halving.
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if mid * mid * (3.0 - 2.0 * mid) < amount {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    now.checked_sub(motion::THEME_FADE.mul_f32(lo)).unwrap_or(now)
}

impl<L> ActiveEffect<L> {
    /// How far in the effect is at `now`: 0 plain, 1 fully on.
    pub fn amount(&self, now: Instant) -> f32 {
        match self.fade {
            None => 1.0,
            Some((since, Direction::In)) => motion::fade_progress(since, now, motion::THEME_FADE),
            Some((since, Direction::Out)) => 1.0 - motion::fade_progress(since, now, motion::THEME_FADE),
        }
    }

    /// Whether it is fully on, with no fade under way: what can only cut is the effect's.
    pub fn settled(&self) -> bool {
        self.fade.is_none()
    }

    /// Whether it is on its way out.
    pub fn leaving(&self) -> bool {
        matches!(self.fade, Some((_, Direction::Out)))
    }

    /// The app's style at `now`. On the way out, what can only cut (the borders, the
    /// highlights, the text size) is back to plain from the fade's start.
    pub fn style(&self, now: Instant) -> Style {
        let s = style_at(&self.effect.manifest, self.amount(now));
        if self.leaving() {
            Style { ink_borders: false, highlights: false, ui_text_bump: 0.0, menus: MenuLook::App, menu_edges: false, ..s }
        } else {
            // The menus' edges cut over with the rest once the effect is fully on.
            Style { menu_edges: self.amount(now) >= 1.0 && has_overlay(&self.effect.wash), ..s }
        }
    }

    /// Starts the fade out at `now`, from wherever a fade in had got to.
    pub fn leave(&mut self, now: Instant) {
        let at = self.amount(now);
        self.fade = Some((since_for(now, 1.0 - at), Direction::Out));
    }

    /// Turns a fade out back into a fade in at `now`, from wherever it had got to.
    pub fn come_back(&mut self, now: Instant) {
        let at = self.amount(now);
        self.fade = Some((since_for(now, at), Direction::In));
    }

    /// Ends a fade that is complete at `now` and says which one ended.
    pub fn step(&mut self, now: Instant) -> Option<Direction> {
        let (since, direction) = self.fade?;
        if motion::fade_progress(since, now, motion::THEME_FADE) < 1.0 {
            return None;
        }
        self.fade = None;
        Some(direction)
    }

    /// The shader's clock at `now`: seconds since the effect started, frozen under Reduce Motion.
    pub fn time(&self, now: Instant, reduce: bool) -> f32 {
        if reduce {
            0.0
        } else {
            now.duration_since(self.started).as_secs_f32()
        }
    }

    /// The clock the glyph shader reads: seconds since the effect started, wrapped in f64 before
    /// it is narrowed so an f32 keeps its sub-frame precision in a long session. Zero under
    /// Reduce Motion.
    pub fn glyph_time(&self, now: Instant, reduce: bool) -> f32 {
        if reduce {
            0.0
        } else {
            (now.duration_since(self.started).as_secs_f64() % GLYPH_CLOCK_WRAP) as f32
        }
    }

    /// Whether a frame with `inputs` is drawn: never while the window is hidden or covered, and
    /// under Reduce Motion only when the inputs differ from the last frame's.
    pub fn should_draw(&self, inputs: &Inputs, hidden: bool, covered: bool, reduce: bool) -> bool {
        !hidden && !covered && !(reduce && self.last_inputs.as_ref() == Some(inputs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::blocks::{self, Kind};
    use crate::effects::inputs;
    use crate::effects::manifest::{self, Effect, Highlights};
    use crate::motion::{test_lock, THEME_FADE};
    use crate::tree::{Rect, Side, Tab, Tree};
    use std::path::Path;
    use std::time::{Duration, Instant};

    #[test]
    fn the_text_redraws_only_while_a_moving_glyph_shader_is_on_and_seen() {
        assert_eq!(glyph_wait(30, false, true, true, false), None);
        assert_eq!(glyph_wait(30, true, false, true, false), None);
        assert_eq!(glyph_wait(30, true, true, false, false), None);
        assert_eq!(glyph_wait(30, true, true, true, true), None);
        assert_eq!(glyph_wait(30, true, true, true, false), Some(Duration::from_millis(50)));
        assert_eq!(glyph_wait(60, true, true, true, false), Some(Duration::from_millis(50)));
        assert_eq!(glyph_wait(12, true, true, true, false), Some(Duration::from_secs_f64(1.0 / 12.0)));
    }

    #[test]
    fn the_glyph_clock_stays_precise_to_a_millisecond_after_six_days_and_is_zero_under_reduce_motion() {
        let start = Instant::now();
        let e = on(watercolor(), start);
        let six_days = Duration::from_secs(6 * 24 * 3600) + Duration::from_millis(123);
        let got = e.glyph_time(start + six_days, false);
        let want = six_days.as_secs_f64() % GLYPH_CLOCK_WRAP;
        assert!(got >= 0.0 && (got as f64) < GLYPH_CLOCK_WRAP);
        assert!((got as f64 - want).abs() < 0.001, "{got} against {want}");
        assert_eq!(e.glyph_time(start + six_days, true), 0.0);
    }

    fn watercolor() -> Effect {
        manifest::load("watercolor", Path::new("/nonexistent/effects")).unwrap()
    }

    /// An effect already on, with a stand-in for its layer.
    fn on(effect: Effect, now: Instant) -> ActiveEffect<()> {
        ActiveEffect { effect, layer: (), started: now, last_inputs: None, last_overlay: None, fade: None, logged: None }
    }

    fn inputs_at(time: f32) -> inputs::Inputs {
        inputs::build(&crate::theme::builtin("peach-light").unwrap(), 1.0, (800.0, 600.0), 1.0, time, &[])
    }

    #[test]
    fn the_glyph_shader_is_told_the_window_s_scale_ahead_of_its_own_source() {
        let e = on(watercolor(), Instant::now());
        let glyphs = watercolor().glyphs.unwrap();
        let retina = glyph_shader_for(WindowKind::Project, Some(&e), 2.0).unwrap();
        assert_eq!(retina.source, format!("constant float ARKOS_SCALE = 2.00;\n{glyphs}"));
        let plain_screen = glyph_shader_for(WindowKind::Project, Some(&e), 1.0).unwrap();
        assert!(plain_screen.source.starts_with("constant float ARKOS_SCALE = 1.00;\n"));
        // The built-in measures its ink in points, so a 1x screen sees the look a 2x one does.
        assert!(glyphs.contains("ARKOS_SCALE"), "the built-in ink ignores the scale");
    }

    #[test]
    fn an_open_menu_keeps_the_menu_stage_ticking_while_the_wash_rests_under_it() {
        assert!(ticks(false, false, false), "a plain frame ticks");
        assert!(!ticks(true, false, false) && !ticks(true, true, true), "a hidden window never does");
        assert!(!ticks(false, true, false), "a prompt over the panes rests the wash");
        assert!(ticks(false, true, true), "an open menu is covered too, and its edges still draw");
        // The wash itself is not drawn while covered.
        let t0 = Instant::now();
        let e = on(watercolor(), t0);
        let inputs = super::super::inputs::build(&theme::builtin("peach-light").unwrap(), 1.0, (10.0, 10.0), 1.0, 0.0, &[]);
        assert!(!e.should_draw(&inputs, false, true, false));
    }

    #[test]
    fn an_effect_with_a_menu_stage_draws_the_menus_edges_once_it_is_fully_on() {
        let _lock = test_lock(false);
        assert!(has_overlay(&watercolor().wash));
        let space = crate::effects::BUILTIN.iter().find(|b| b.name == "space").unwrap();
        assert!(has_overlay(space.wash));
        assert!(!has_overlay("fragment float4 arkos_wash(ArkosVertex in [[stage_in]]) { return 1; }"));
        let t0 = Instant::now();
        assert!(on(watercolor(), t0).style(t0).menu_edges);
        assert!(!Style::plain().menu_edges);
        assert!(!style_at(&watercolor().manifest, 1.0).menu_edges, "the manifest alone does not know the wash");
        // Mid-fade, and from the start of a fade out, the app draws its own lines.
        let mut fading = start(watercolor(), &mut Fine, t0, false).unwrap();
        assert!(!fading.style(t0 + THEME_FADE / 2).menu_edges);
        fading.step(t0 + THEME_FADE);
        assert!(fading.style(t0 + THEME_FADE).menu_edges);
        fading.leave(t0 + THEME_FADE * 2);
        assert!(!fading.style(t0 + THEME_FADE * 2).menu_edges);
        // A wash without a menu stage leaves the menus' lines to the app.
        let mut plain = watercolor();
        plain.wash = plain.wash.replace("arkos_overlay(", "arkos_not_overlay(");
        assert!(!on(plain, t0).style(t0).menu_edges);
    }

    #[test]
    fn the_style_is_plain_at_zero_and_the_manifest_s_at_one() {
        let m = watercolor().manifest;
        assert_eq!(style_at(&m, 0.0), Style::plain());
        assert_eq!(
            style_at(&m, 1.0),
            Style { paper_alpha: 0.5, chrome_alpha: 0.0, active_tab_alpha: 0.55, ground_alpha: 0.0, ink_borders: true, highlights: true, ui_text_bump: 2.0, menus: MenuLook::Ink, menu_edges: false }
        );
        let half = style_at(&m, 0.5);
        assert_eq!((half.paper_alpha, half.chrome_alpha, half.ground_alpha), (0.75, 0.5, 0.5));
        assert!(half.active_tab_alpha > 0.55 && half.active_tab_alpha < 1.0);
        // What can only cut waits for the fade's end.
        assert!(!half.ink_borders && !half.highlights);
        assert_eq!(half.menus, MenuLook::App, "a menu's look can only cut, at the fade's end");
        assert_eq!(Style::plain().menus, MenuLook::App);
        assert_eq!(half.ui_text_bump, 0.0);
        assert_eq!(style_at(&m, 1.0).ui(12.0), 14.0);
        assert_eq!(Style::plain().ui(12.0), 12.0);
    }

    #[test]
    fn switching_to_none_restores_the_app_s_own_fills() {
        let _lock = test_lock(false);
        let mut app = watercolor();
        app.manifest.highlights = Highlights::App;
        // An effect that leaves the highlights to the app never takes the app's fills away.
        assert!(!style_at(&app.manifest, 1.0).highlights);
        assert_eq!(style_at(&app.manifest, 0.0), Style::plain());
        let t0 = Instant::now();
        let mut e = on(watercolor(), t0);
        assert!(e.style(t0).highlights);
        e.leave(t0);
        // The fills, the borders and the text size come back at the fade out's start.
        let s = e.style(t0);
        assert!(!s.highlights && !s.ink_borders);
        assert_eq!(s.ui_text_bump, 0.0);
        assert_eq!(s.paper_alpha, 0.5, "the alphas fade from where they were");
        assert!(e.leaving());
        assert_eq!(e.step(t0 + THEME_FADE / 2), None);
        assert_eq!(e.style(t0 + THEME_FADE), Style::plain());
        // The window drops the effect when its fade out ends.
        assert_eq!(e.step(t0 + THEME_FADE), Some(Direction::Out));
    }

    #[test]
    fn a_fade_reversed_midway_continues_from_where_it_was() {
        let _lock = test_lock(false);
        let t0 = Instant::now();
        let mut e = on(watercolor(), t0);
        e.fade = Some((t0, Direction::In));
        let mid = t0 + THEME_FADE / 4;
        let before = e.amount(mid);
        assert!(before > 0.0 && before < 0.5, "{before}");
        e.leave(mid);
        assert!((e.amount(mid) - before).abs() < 0.01, "{} then {before}", e.amount(mid));
        e.come_back(mid);
        assert!((e.amount(mid) - before).abs() < 0.01);
        assert!(!e.leaving());
        assert_eq!(e.step(mid + THEME_FADE), Some(Direction::In));
        assert_eq!(e.fade, None);
        assert_eq!(e.amount(mid + THEME_FADE), 1.0);
    }

    #[test]
    fn the_cadence_is_the_manifest_s_rate_at_rest_and_sixty_while_moving() {
        assert_eq!(cadence(30, false), Duration::from_secs_f64(1.0 / 30.0));
        assert_eq!(cadence(30, true), Duration::from_secs_f64(1.0 / 60.0));
        assert_eq!(cadence(10, true), Duration::from_secs_f64(1.0 / 60.0));
        assert_eq!(cadence(60, false), Duration::from_secs_f64(1.0 / 60.0));
    }

    #[test]
    fn the_timer_rests_while_nothing_can_be_drawn_and_stops_when_reduce_motion_holds_still() {
        // Hidden, occluded or covered: a slow look, whatever moves, until the window is seen.
        assert_eq!(next_wait(30, false, false, false, false), Some(REST));
        assert_eq!(next_wait(30, false, true, true, false), Some(REST));
        assert_eq!(next_wait(30, false, false, false, true), Some(REST));
        assert!(REST >= Duration::from_millis(250));
        // Seen and in motion: the manifest's rate, sixty while a highlight fades.
        assert_eq!(next_wait(30, true, false, false, false), Some(cadence(30, false)));
        assert_eq!(next_wait(30, true, true, false, false), Some(cadence(30, true)));
        // Under Reduce Motion with nothing moving the timer stops; a render starts it again.
        assert_eq!(next_wait(30, true, false, false, true), None);
        assert_eq!(next_wait(30, true, true, false, true), Some(cadence(30, true)));
        assert_eq!(next_wait(30, true, false, true, true), Some(cadence(30, false)), "a fade still ends");
    }

    #[test]
    fn under_reduce_motion_the_wash_draws_only_when_something_changed() {
        let t0 = Instant::now();
        let mut e = on(watercolor(), t0);
        let same = inputs_at(0.0);
        assert!(e.should_draw(&same, false, false, true), "the first frame always draws");
        e.last_inputs = Some(same.clone());
        assert!(!e.should_draw(&same, false, false, true));
        assert!(e.should_draw(&inputs_at(0.5), false, false, true), "something changed");
        assert!(e.should_draw(&same, false, false, false), "in motion it draws every tick");
        assert!(!e.should_draw(&inputs_at(0.5), true, false, false), "hidden");
        assert!(!e.should_draw(&inputs_at(0.5), false, true, false), "covered");
        // The clock is frozen, so a still window hands the shader the same inputs.
        assert_eq!(e.time(t0 + Duration::from_secs(3), true), 0.0);
        assert_eq!(e.time(t0 + Duration::from_secs(3), false), 3.0);
    }

    struct Garbage;
    impl Installer for Garbage {
        type Layer = ();
        fn install(&mut self, _effect: &Effect) -> Result<(), String> {
            Err("Compilation failed: \n\nprogram_source:7:3: error: garbage\nprogram_source:9:1: error: more".into())
        }
    }

    struct Fine;
    impl Installer for Fine {
        type Layer = ();
        fn install(&mut self, _effect: &Effect) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn a_shader_that_fails_to_compile_leaves_the_theme_plain_and_says_so() {
        let mut bad = watercolor();
        bad.manifest.name = "bad".into();
        bad.wash = "garbage".into();
        let t0 = Instant::now();
        // No effect comes of it, so the window's style stays plain; the notice names the file
        // and the line in it.
        let notice = start(bad, &mut Garbage, t0, false).err().expect("no effect");
        assert!(notice.starts_with("effect bad: wash.metal:"), "{notice}");
        assert_eq!(notice, "effect bad: wash.metal:7: garbage");
        assert_eq!(compile_notice("bad", "wash.metal", "no Metal device"), "effect bad: no Metal device");
        let e = start(watercolor(), &mut Fine, t0, false).unwrap();
        assert_eq!(e.fade, Some((t0, Direction::In)));
        assert_eq!(start(watercolor(), &mut Fine, t0, true).unwrap().fade, None, "a cut");
    }

    #[test]
    fn blocks_follow_the_layout_and_the_drawable_follows_the_view() {
        let mut tree = Tree::new(Tab::Terminal { session: protocol::SessionId(1), cwd: None });
        let first = tree.focused;
        tree.split(first, Side::Right, Tab::Terminal { session: protocol::SessionId(2), cwd: None }).unwrap();
        for content in [Rect { x: 240.0, y: 28.0, w: 960.0, h: 772.0 }, Rect { x: 240.0, y: 28.0, w: 660.0, h: 572.0 }] {
            let layout = tree.layout(content);
            let out = blocks::blocks(&scene(content, None, &layout.panes, &[], &[]), true);
            assert_eq!(out[0].rect.w, content.x + content.w, "the title bar spans the window");
            let bars: Vec<Rect> = out.iter().filter(|b| b.kind == Kind::TabBar).map(|b| b.rect).collect();
            let want: Vec<Rect> = layout.panes.iter().map(|(_, r)| Rect { h: crate::pane_view::TAB_BAR_H, ..*r }).collect();
            assert_eq!(bars, want);
        }
        // The drawable is the view's size in pixels at the manifest's scale, whatever the view.
        assert_eq!(super::super::layer::drawable_size((1200.0, 800.0), 2.0, 0.5), (1200.0, 800.0));
        assert_eq!(super::super::layer::drawable_size((901.0, 600.0), 2.0, 0.5), (901.0, 600.0));
        assert_eq!(super::super::layer::drawable_size((1200.0, 800.0), 1.0, 0.25), (300.0, 200.0));
    }

    /// Records what a window was asked to draw its glyphs with, or refuses a shader as a
    /// compiler would; going back to gpui's own glyphs is never refused.
    #[derive(Default)]
    struct Recorder {
        calls: Vec<Option<GlyphShader>>,
        refuse: Option<String>,
        times: Vec<f32>,
    }
    impl GlyphSink for Recorder {
        fn set_glyph_shader(&mut self, shader: Option<GlyphShader>) -> Result<(), String> {
            match (&self.refuse, &shader) {
                (Some(m), Some(_)) => Err(m.clone()),
                _ => {
                    self.calls.push(shader);
                    Ok(())
                }
            }
        }
        fn set_glyph_time(&mut self, seconds: f32) {
            self.times.push(seconds);
        }
    }

    #[test]
    fn a_window_is_handed_the_glyph_time_and_applying_ink_leaves_the_time_alone() {
        let _lock = test_lock(true);
        let mut window = Recorder::default();
        let a = start(watercolor(), &mut Fine, Instant::now(), true).unwrap();
        apply_glyphs(&mut window, &a.effect, glyph_shader_for(WindowKind::Project, Some(&a), 2.0)).unwrap();
        assert!(window.times.is_empty(), "setting the stages does not set the clock");
        window.set_glyph_time(1.5);
        window.set_glyph_time(1.55);
        assert_eq!(window.times, vec![1.5, 1.55]);
    }

    #[test]
    fn a_refused_glyph_shader_clears_the_previous_effect_s_ink() {
        let _lock = test_lock(true);
        let t0 = Instant::now();
        let mut window = Recorder::default();
        // Effect A is on, cut in under Reduce Motion, with its ink.
        let a = start(watercolor(), &mut Fine, t0, true).unwrap();
        let ink_a = glyph_shader_for(WindowKind::Project, Some(&a), 2.0);
        assert!(ink_a.is_some());
        apply_glyphs(&mut window, &a.effect, ink_a.clone()).unwrap();
        // B replaces A at once, and the window refuses B's glyph shader.
        let mut b_effect = watercolor();
        b_effect.manifest.name = "b".into();
        b_effect.glyphs = Some("garbage".into());
        let b = start(b_effect, &mut Fine, t0, true).unwrap();
        window.refuse = Some("Compilation failed: \n\nprogram_source:3:1: error: garbage\n".into());
        let notice = apply_glyphs(&mut window, &b.effect, glyph_shader_for(WindowKind::Project, Some(&b), 2.0)).unwrap_err();
        assert_eq!(notice, "effect b: glyphs.metal: garbage");
        // gpui's own glyphs come back, rather than A's ink staying under B's wash.
        assert_eq!(window.calls, vec![ink_a, None]);
        assert!(b.settled() && b.style(t0).paper_alpha > 0.0, "B stays on");
    }

    #[test]
    fn a_glyphs_file_that_defines_an_emoji_fragment_draws_the_emoji_too() {
        assert_eq!(emoji_stages("fragment float4 arkos_glyph_fragment(In input [[stage_in]]) {}"), None);
        assert_eq!(
            emoji_stages("fragment float4 arkos_emoji_fragment(PolychromeSpriteFragmentInput input [[stage_in]]) {}"),
            Some(gpui::EmojiStages { vertex: "polychrome_sprite_vertex".into(), fragment: "arkos_emoji_fragment".into() }),
            "without a vertex stage of its own, gpui's places the emoji"
        );
        assert_eq!(
            emoji_stages("vertex Out arkos_emoji_vertex(uint v [[vertex_id]]) {}\nfragment float4 arkos_emoji_fragment(In input [[stage_in]]) {}"),
            Some(gpui::EmojiStages { vertex: "arkos_emoji_vertex".into(), fragment: "arkos_emoji_fragment".into() })
        );
        // Watercolor inks its emoji, and space shows them on its holo display.
        let t0 = Instant::now();
        let ink = glyph_shader_for(WindowKind::Project, Some(&on(watercolor(), t0)), 2.0).unwrap();
        assert_eq!(ink.emoji.map(|e| e.fragment), Some("arkos_emoji_fragment".to_string()));
        let space = crate::effects::BUILTIN.iter().find(|b| b.name == "space").unwrap();
        assert_eq!(
            emoji_stages(space.glyphs.unwrap()),
            Some(gpui::EmojiStages { vertex: "arkos_emoji_vertex".into(), fragment: "arkos_emoji_fragment".into() })
        );
    }

    #[test]
    fn the_glyph_shader_is_applied_at_the_end_of_a_fade_in_and_removed_at_the_start_of_a_fade_out() {
        let _lock = test_lock(false);
        let t0 = Instant::now();
        let mut window = Recorder::default();
        let mut e = start(watercolor(), &mut Fine, t0, false).unwrap();
        // Midway in, the glyphs are still gpui's own: they can only cut, at the fade's end.
        assert_eq!(e.step(t0 + THEME_FADE / 2), None);
        assert_eq!(glyph_shader_for(WindowKind::Project, Some(&e), 2.0), None);
        assert_eq!(e.step(t0 + THEME_FADE), Some(Direction::In));
        apply_glyphs(&mut window, &e.effect, glyph_shader_for(WindowKind::Project, Some(&e), 2.0)).unwrap();
        let ink = GlyphShader {
            source: glyph_source(&watercolor().glyphs.unwrap(), 2.0),
            vertex: "arkos_glyph_vertex".into(),
            fragment: "arkos_glyph_fragment".into(),
            emoji: Some(gpui::EmojiStages { vertex: "arkos_emoji_vertex".into(), fragment: "arkos_emoji_fragment".into() }),
        };
        assert_eq!(window.calls, vec![Some(ink.clone())]);
        // The fade out's first frame already draws gpui's own.
        let t1 = t0 + THEME_FADE * 2;
        e.leave(t1);
        apply_glyphs(&mut window, &e.effect, glyph_shader_for(WindowKind::Project, Some(&e), 2.0)).unwrap();
        assert_eq!(window.calls, vec![Some(ink.clone()), None]);
        assert_eq!(glyph_shader_for::<()>(WindowKind::Project, None, 2.0), None, "no effect, no ink");
        // A cut in has the ink at once.
        let cut = start(watercolor(), &mut Fine, t0, true).unwrap();
        assert_eq!(glyph_shader_for(WindowKind::Project, Some(&cut), 2.0), Some(ink));
        // An effect without a glyphs file leaves gpui's own.
        let mut plain = watercolor();
        plain.glyphs = None;
        assert_eq!(glyph_shader_for(WindowKind::Project, Some(&on(plain, t0)), 2.0), None);
        // A shader the window refuses is a notice naming the file; the effect stays on.
        let mut refusing = Recorder { refuse: Some("Compilation failed: \n\nprogram_source:1290:3: error: unknown type 'flaot'\n".into()), ..Default::default() };
        let notice = apply_glyphs(&mut refusing, &cut.effect, glyph_shader_for(WindowKind::Project, Some(&cut), 2.0)).unwrap_err();
        assert_eq!(notice, "effect watercolor: glyphs.metal: unknown type 'flaot'");
        assert_eq!(cut.style(t0).paper_alpha, 0.5, "still on");
    }

    #[test]
    fn a_window_reports_its_effect_and_cost_until_the_effect_starts_to_leave() {
        let t0 = Instant::now();
        let mut e = on(watercolor(), t0);
        assert_eq!(reported(None::<&ActiveEffect<()>>, |_| Some(1.0)), None);
        assert_eq!(reported(Some(&e), |_| Some(2.5)), Some(("watercolor".to_string(), Some(2.5))));
        assert_eq!(reported(Some(&e), |_| None), Some(("watercolor".to_string(), None)), "not measured yet");
        e.leave(t0);
        assert_eq!(reported(Some(&e), |_| Some(2.5)), None, "on its way out it is no longer the window's");
    }

    #[test]
    fn the_glyph_shader_is_the_window_s_own() {
        let e = on(watercolor(), Instant::now());
        assert!(glyph_shader_for(WindowKind::Project, Some(&e), 2.0).is_some());
        for kind in [WindowKind::About, WindowKind::Settings, WindowKind::Keybindings, WindowKind::Chooser] {
            assert_eq!(glyph_shader_for(kind, Some(&e), 2.0), None, "{kind:?} keeps gpui's own glyphs");
        }
    }

    #[test]
    fn a_light_theme_s_text_moves_towards_black_and_a_dark_one_s_towards_white() {
        use crate::palette::Rgb;
        let peach = crate::theme::builtin("peach-light").unwrap();
        assert!(peach.is_light());
        let deep = contrasted(&peach, 0.6);
        let towards = |from: Rgb, to: u8, by: f32| {
            let c = |x: u8| (f32::from(x) + (f32::from(to) - f32::from(x)) * by).round() as u8;
            Rgb { r: c(from.r), g: c(from.g), b: c(from.b) }
        };
        assert_eq!(deep.chrome.text, towards(peach.chrome.text, 0, 0.6));
        assert_eq!(deep.chrome.text_dim, towards(peach.chrome.text_dim, 0, 0.75 * 0.6));
        assert_eq!(deep.terminal.foreground, towards(peach.terminal.foreground, 0, 0.9 * 0.6));
        // Nothing else moves: the backgrounds, the palette, the name.
        assert_eq!(deep.chrome.window_bg, peach.chrome.window_bg);
        assert_eq!(deep.terminal.black, peach.terminal.black);
        assert_eq!(deep.name, peach.name);
        let navy = crate::theme::builtin("navy").unwrap();
        assert!(!navy.is_light());
        let bright = contrasted(&navy, 1.0);
        assert_eq!(bright.chrome.text, Rgb { r: 255, g: 255, b: 255 });
        assert_eq!(bright.chrome.text_dim, towards(navy.chrome.text_dim, 255, 0.75));
        assert_eq!(bright.terminal.foreground, towards(navy.terminal.foreground, 255, 0.9));
    }

    #[test]
    fn zero_contrast_leaves_the_theme_as_it_was() {
        for name in ["peach-light", "navy"] {
            let t = crate::theme::builtin(name).unwrap();
            assert_eq!(contrasted(&t, 0.0), t);
        }
    }

    #[test]
    fn the_cost_is_logged_once_a_minute_while_it_changes_by_a_tenth() {
        let t0 = Instant::now();
        assert!(should_log(None, t0, 3.0));
        let last = Some((t0, 3.0));
        assert!(!should_log(last, t0 + Duration::from_secs(30), 5.0), "within the minute");
        assert!(!should_log(last, t0 + Duration::from_secs(61), 3.2), "less than a tenth");
        assert!(should_log(last, t0 + Duration::from_secs(61), 3.4));
    }
}
