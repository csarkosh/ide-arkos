//! An effect while the window shows it (spec 4.3 and 4.4): its layer, its clock, the fade that
//! brings it in and takes it away, and the style the app's surfaces paint with meanwhile. Pure
//! apart from the layer it holds, so the style, the cadence and the fade are tested.

use std::time::{Duration, Instant};

use gpui::GlyphShader;

use super::blocks::{Mark, Scene};
use super::inputs::Inputs;
use super::layer::Layer;
use super::manifest::{Effect, Highlights, Manifest};
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
}

impl Style {
    /// The app as it paints without an effect.
    pub fn plain() -> Style {
        Style { paper_alpha: 1.0, chrome_alpha: 1.0, active_tab_alpha: 1.0, ground_alpha: 1.0, ink_borders: false, highlights: false, ui_text_bump: 0.0 }
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
/// the fade; the borders, the highlights and the text size can only cut, so they are the
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
    }
}

/// How long the wash's timer waits between frames: the manifest's `frame_rate` at rest, sixty a
/// second while something moves (a highlight fading), so a short fade is more than a few frames.
pub fn cadence(frame_rate: u32, moving: bool) -> Duration {
    let rate = if moving { 60 } else { frame_rate.clamp(1, 60) };
    Duration::from_secs_f64(1.0 / rate as f64)
}

/// What a frame's blocks are made from in the project window: the panes' content area, the
/// window's bars, the sidebar's divider when laid out, the panes and the highlights' strokes.
pub fn scene<'a>(content: Rect, divider_y: Option<f32>, panes: &'a [(PaneId, Rect)], strokes: &'a [(Mark, f32)]) -> Scene<'a> {
    Scene { content, title_bar_h: crate::chrome::TITLE_BAR_H, tab_bar_h: crate::pane_view::TAB_BAR_H, divider_y, panes, strokes }
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

/// Where a window's glyph stages are set: gpui's window, or a test's stand-in.
pub trait GlyphSink {
    fn set_glyph_shader(&mut self, shader: Option<GlyphShader>) -> Result<(), String>;
}

impl GlyphSink for gpui::Window {
    fn set_glyph_shader(&mut self, shader: Option<GlyphShader>) -> Result<(), String> {
        gpui::Window::set_glyph_shader(self, shader)
    }
}

/// The glyph shader a window of `kind` draws with under `effect`: the effect's own, only in the
/// project window, only once it has faded in fully and until it starts to leave (glyphs can
/// only cut), and only when the effect has a glyphs file.
pub fn glyph_shader_for<L>(kind: WindowKind, effect: Option<&ActiveEffect<L>>) -> Option<GlyphShader> {
    let e = effect.filter(|e| kind == WindowKind::Project && e.settled())?;
    Some(GlyphShader {
        source: e.effect.glyphs.clone()?,
        vertex: "arkos_glyph_vertex".into(),
        fragment: "arkos_glyph_fragment".into(),
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
/// effect's glyphs file and the compiler's first error; the window keeps the glyphs it had.
pub fn apply_glyphs(window: &mut impl GlyphSink, effect: &Effect, shader: Option<GlyphShader>) -> Result<(), String> {
    window.set_glyph_shader(shader).map_err(|message| {
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
    /// A fade under way: since when, and which way.
    pub fade: Option<(Instant, Direction)>,
    /// When the GPU's time per frame was last written to the window log, and what it was.
    pub logged: Option<(Instant, f32)>,
}

/// Installs `effect` and starts it fading in at `now`, or at once when `cut` (Reduce Motion, or
/// a window not yet drawn). A shader that does not compile gives the notice to show instead.
pub fn start<I: Installer>(effect: Effect, installer: &mut I, now: Instant, cut: bool) -> Result<ActiveEffect<I::Layer>, String> {
    match installer.install(&effect) {
        Ok(layer) => Ok(ActiveEffect { effect, layer, started: now, last_inputs: None, fade: if cut { None } else { Some((now, Direction::In)) }, logged: None }),
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
            Style { ink_borders: false, highlights: false, ui_text_bump: 0.0, ..s }
        } else {
            s
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

    fn watercolor() -> Effect {
        manifest::load("watercolor", Path::new("/nonexistent/effects")).unwrap()
    }

    /// An effect already on, with a stand-in for its layer.
    fn on(effect: Effect, now: Instant) -> ActiveEffect<()> {
        ActiveEffect { effect, layer: (), started: now, last_inputs: None, fade: None, logged: None }
    }

    fn inputs_at(time: f32) -> inputs::Inputs {
        inputs::build(&crate::theme::builtin("peach-light").unwrap(), 1.0, (800.0, 600.0), 1.0, time, &[])
    }

    #[test]
    fn the_style_is_plain_at_zero_and_the_manifest_s_at_one() {
        let m = watercolor().manifest;
        assert_eq!(style_at(&m, 0.0), Style::plain());
        assert_eq!(
            style_at(&m, 1.0),
            Style { paper_alpha: 0.5, chrome_alpha: 0.0, active_tab_alpha: 0.55, ground_alpha: 0.0, ink_borders: true, highlights: true, ui_text_bump: 2.0 }
        );
        let half = style_at(&m, 0.5);
        assert_eq!((half.paper_alpha, half.chrome_alpha, half.ground_alpha), (0.75, 0.5, 0.5));
        assert!(half.active_tab_alpha > 0.55 && half.active_tab_alpha < 1.0);
        // What can only cut waits for the fade's end.
        assert!(!half.ink_borders && !half.highlights);
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
            let out = blocks::blocks(&scene(content, None, &layout.panes, &[]), true);
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

    /// Records what a window was asked to draw its glyphs with, or refuses as a compiler would.
    #[derive(Default)]
    struct Recorder {
        calls: Vec<Option<GlyphShader>>,
        refuse: Option<String>,
    }
    impl GlyphSink for Recorder {
        fn set_glyph_shader(&mut self, shader: Option<GlyphShader>) -> Result<(), String> {
            match &self.refuse {
                Some(m) => Err(m.clone()),
                None => {
                    self.calls.push(shader);
                    Ok(())
                }
            }
        }
    }

    #[test]
    fn the_glyph_shader_is_applied_at_the_end_of_a_fade_in_and_removed_at_the_start_of_a_fade_out() {
        let _lock = test_lock(false);
        let t0 = Instant::now();
        let mut window = Recorder::default();
        let mut e = start(watercolor(), &mut Fine, t0, false).unwrap();
        // Midway in, the glyphs are still gpui's own: they can only cut, at the fade's end.
        assert_eq!(e.step(t0 + THEME_FADE / 2), None);
        assert_eq!(glyph_shader_for(WindowKind::Project, Some(&e)), None);
        assert_eq!(e.step(t0 + THEME_FADE), Some(Direction::In));
        apply_glyphs(&mut window, &e.effect, glyph_shader_for(WindowKind::Project, Some(&e))).unwrap();
        let ink = GlyphShader {
            source: watercolor().glyphs.unwrap(),
            vertex: "arkos_glyph_vertex".into(),
            fragment: "arkos_glyph_fragment".into(),
        };
        assert_eq!(window.calls, vec![Some(ink.clone())]);
        // The fade out's first frame already draws gpui's own.
        let t1 = t0 + THEME_FADE * 2;
        e.leave(t1);
        apply_glyphs(&mut window, &e.effect, glyph_shader_for(WindowKind::Project, Some(&e))).unwrap();
        assert_eq!(window.calls, vec![Some(ink.clone()), None]);
        assert_eq!(glyph_shader_for::<()>(WindowKind::Project, None), None, "no effect, no ink");
        // A cut in has the ink at once.
        let cut = start(watercolor(), &mut Fine, t0, true).unwrap();
        assert_eq!(glyph_shader_for(WindowKind::Project, Some(&cut)), Some(ink));
        // An effect without a glyphs file leaves gpui's own.
        let mut plain = watercolor();
        plain.glyphs = None;
        assert_eq!(glyph_shader_for(WindowKind::Project, Some(&on(plain, t0))), None);
        // A shader the window refuses is a notice naming the file; the effect stays on.
        let mut refusing = Recorder { refuse: Some("Compilation failed: \n\nprogram_source:1290:3: error: unknown type 'flaot'\n".into()), ..Default::default() };
        let notice = apply_glyphs(&mut refusing, &cut.effect, glyph_shader_for(WindowKind::Project, Some(&cut))).unwrap_err();
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
        assert!(glyph_shader_for(WindowKind::Project, Some(&e)).is_some());
        for kind in [WindowKind::About, WindowKind::Settings, WindowKind::Keybindings, WindowKind::Chooser] {
            assert_eq!(glyph_shader_for(kind, Some(&e)), None, "{kind:?} keeps gpui's own glyphs");
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
