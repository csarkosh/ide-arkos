//! SPIKE, throwaway: a watercolor sketch under the window's content.
//!
//! A `CAMetalLayer` in its own `NSView` sits below gpui's view in the window's content view and
//! runs a fragment shader at thirty frames a second from its own timer. gpui draws nothing for
//! it: the window is transparent and the theme's surfaces are painted with little or no alpha.
//! Each block of the UI (the title bar, the sidebar, every pane's tab bar and body) is handed to
//! the shader as a rectangle each frame, from the app's own layout model, and painted as a wash
//! that bleeds past its edges, with pigment pooled at the ragged rim and granulation inside,
//! under a hand-drawn ink outline that wobbles slowly. Soft bubbles drift behind everything.
//! The colours are the theme's, read every frame: the paper is the window background, the ink
//! the text colour, the pigments four of its accents. The shader is Metal source compiled at
//! launch (`ARKOS_EFFECT_SHADER=<file>` replaces it), the seed of user-loaded effects.

use crate::palette::Rgb;
use metal::foreign_types::{ForeignType, ForeignTypeRef};
use metal::{CommandQueue, CompileOptions, Device, MTLLoadAction, MTLPixelFormat, MTLPrimitiveType, MTLStoreAction, MetalLayer, RenderPassDescriptor, RenderPipelineDescriptor, RenderPipelineState};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, MainThreadMarker};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSWindow, NSWindowOrderingMode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Whether the wash is on, for the paint sites that give their surfaces some alpha.
static ACTIVE: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Where the sidebar's Worktrees section starts, in window points, as last laid out.
    static DIVIDER: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) };
}

/// A highlight's rectangle in window points: x, y, width, height.
pub type Rect4 = (f32, f32, f32, f32);

/// How strong the wash under an Explorer row is: selected, hovered, or both.
pub const SELECTED: f32 = 0.3;
pub const HOVERED: f32 = 0.2;
pub const SELECTED_HOVERED: f32 = 0.36;

/// One highlight's stroke as it fades: where it is, and the strength it is moving between.
struct Stroke {
    rect: Rect4,
    from: f32,
    to: f32,
    since: Instant,
}

impl Stroke {
    /// The strength now: rising over HIGHLIGHT_ENTER on Material 3's standard decelerate
    /// curve, falling over HIGHLIGHT_EXIT on its standard accelerate (instant under Reduce Motion).
    fn value(&self, now: Instant) -> f32 {
        if self.to >= self.from {
            self.from + (self.to - self.from) * crate::motion::highlight_in(self.since, now)
        } else {
            self.to + (self.from - self.to) * crate::motion::highlight_out(self.since, now)
        }
    }
}

/// A tab's key among the highlights, apart from the Explorer rows' path hashes.
pub fn tab_key(tab: u64) -> u64 {
    tab ^ 0xa5a5_0000_0000_0000
}

thread_local! {
    /// Highlights laid out this frame (the Explorer's selected and hovered rows): a key, the
    /// rectangle and the strength wanted.
    static MARKS: std::cell::RefCell<Vec<(u64, Rect4, f32)>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Every highlight still on the paper, fading in, at rest, or fading out.
    static STROKES: std::cell::RefCell<std::collections::HashMap<u64, Stroke>> = std::cell::RefCell::new(std::collections::HashMap::new());
    /// The Explorer row under the pointer.
    static HOVER: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Called at the start of the window's render: the frame's highlights are reported afresh.
pub fn begin_frame() {
    MARKS.with(|m| m.borrow_mut().clear());
}

/// Called by a highlighted element's layout with its key, bounds and the strength it wants.
pub fn mark(key: u64, rect: Rect4, strength: f32) {
    MARKS.with(|m| m.borrow_mut().push((key, rect, strength)));
}

/// The pointer entered (`true`) or left the Explorer row `key`.
pub fn set_hover(key: u64, hovered: bool) {
    HOVER.with(|h| {
        if hovered {
            h.set(Some(key));
        } else if h.get() == Some(key) {
            h.set(None);
        }
    });
}

/// The Explorer row under the pointer.
pub fn hovered() -> Option<u64> {
    HOVER.with(|h| h.get())
}

/// The highlights at `now`, each with its current strength, and whether any is still moving.
/// A highlight that stops being reported fades out from where it was last laid out.
pub fn strokes(now: Instant) -> (Vec<(Rect4, f32)>, bool) {
    let reported = MARKS.with(|m| m.borrow().clone());
    STROKES.with(|s| {
        let mut s = s.borrow_mut();
        let retarget = |stroke: &mut Stroke, to: f32| {
            if (stroke.to - to).abs() > f32::EPSILON {
                stroke.from = stroke.value(now);
                stroke.to = to;
                stroke.since = now;
            }
        };
        for (key, rect, want) in &reported {
            let stroke = s.entry(*key).or_insert(Stroke { rect: *rect, from: 0.0, to: 0.0, since: now });
            stroke.rect = *rect;
            retarget(stroke, *want);
        }
        for (key, stroke) in s.iter_mut() {
            if !reported.iter().any(|(k, _, _)| k == key) {
                retarget(stroke, 0.0);
            }
        }
        s.retain(|_, stroke| stroke.to > 0.0 || stroke.value(now) > 0.001);
        let moving = s.values().any(|stroke| (stroke.value(now) - stroke.to).abs() > 0.001);
        (s.values().map(|stroke| (stroke.rect, stroke.value(now))).collect(), moving)
    })
}

/// Called by the Worktrees section's layout with its top edge.
pub fn note_divider(y: f32) {
    DIVIDER.with(|d| d.set(Some(y)));
}

/// The sidebar's divider between Explorer and Worktrees, when it has been laid out.
pub fn divider() -> Option<f32> {
    DIVIDER.with(|d| d.get())
}

pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// How much larger the project window's UI text is under the wash, in points: room for the ink.
const UI_BUMP: f32 = 2.0;

/// A UI text size as the wash wants it.
pub fn ui(size: f32) -> f32 {
    if active() { size + UI_BUMP } else { size }
}

/// Whether the wash goes with the theme named: Peach Light carries it; `ARKOS_EFFECT=1` puts it
/// under any theme for a look.
pub fn wanted(theme_name: &str) -> bool {
    theme_name == "peach-light" || std::env::var_os("ARKOS_EFFECT").is_some()
}

/// The pen's ink: a deep sepia the text moves towards, so it holds its own against the washes.
const SEPIA: Rgb = Rgb { r: 0x24, g: 0x15, b: 0x0f };

fn toward(c: Rgb, to: Rgb, t: f32) -> Rgb {
    let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Rgb { r: m(c.r, to.r), g: m(c.g, to.g), b: m(c.b, to.b) }
}

/// The theme as the wash wants it: its text, dim text and terminal text deepened towards sepia
/// ink, for contrast over the pigments.
pub fn inked(mut theme: crate::theme::Theme) -> crate::theme::Theme {
    theme.chrome.text = toward(theme.chrome.text, SEPIA, 0.6);
    theme.chrome.text_dim = toward(theme.chrome.text_dim, SEPIA, 0.45);
    theme.terminal.foreground = toward(theme.terminal.foreground, SEPIA, 0.55);
    theme
}

/// The alpha of a terminal's paper over the wash (the text must stay easy to read), of the bars
/// and the sidebar (none: the wash is their colour), and of the active tab (a lighter patch).
pub const SURFACE_ALPHA: f32 = 0.5;
pub const CHROME_ALPHA: f32 = 0.0;
pub const ACTIVE_TAB_ALPHA: f32 = 0.55;

/// The wash's pixels per window pixel on each side: one per point on a Retina screen, enough
/// for the ink lines to stay crisp.
const RENDER_SCALE: f64 = 0.5;

const BUBBLES: usize = 14;
const MAX_REGIONS: usize = 32;

/// Which edges of a region carry an ink line.
pub const INK_TOP: u32 = 1;
pub const INK_RIGHT: u32 = 2;
pub const INK_BOTTOM: u32 = 4;
pub const INK_LEFT: u32 = 8;

/// A block of the UI, in window points from the top-left: its rectangle, which of the four
/// pigments, how strong the wash, and which edges are inked.
#[derive(Debug, Clone, Copy)]
pub struct Region {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub tint: u32,
    pub strength: f32,
    pub ink: u32,
    /// What picks the block's grain pattern and ink overshoot: fixed for the block, so a
    /// highlight coming and going never changes another block's look.
    pub seed: f32,
}

const SHADER: &str = include_str!("../shaders/watercolor.metal");

#[repr(C)]
struct Uniforms {
    time: f32,
    bubble_count: f32,
    size: [f32; 2],
    paper: [f32; 4],
    ink: [f32; 4],
    tints: [[f32; 4]; 4],
    bubbles: [[f32; 4]; 16],
    region_count: f32,
    pad: [f32; 3],
    rects: [[f32; 4]; MAX_REGIONS],
    metas: [[f32; 4]; MAX_REGIONS],
}

fn float4(c: Rgb) -> [f32; 4] {
    [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, 1.0]
}

fn hash(n: f32) -> f32 {
    (n.sin() * 43758.547).fract().abs()
}

/// Where each bubble is at `t` seconds, in units where the window is `aspect` wide and 1 high:
/// x, y, radius and which of the four tints. Each sways slowly and drifts slower still across,
/// wrapping with a margin so it leaves before it comes back.
fn bubbles(t: f32, aspect: f32) -> [[f32; 4]; 16] {
    let mut out = [[0.0; 4]; 16];
    for (i, b) in out.iter_mut().enumerate().take(BUBBLES) {
        let fi = i as f32;
        let (hx, hy) = (hash(fi * 7.31), hash(fi * 7.31 + 1.0));
        let (sx, sy) = (hash(fi * 3.17 + 11.0), hash(fi * 3.17 + 12.0));
        let speed = 0.05 + 0.07 * sx;
        let mut x = hx * aspect + (t * speed + fi).sin() * 0.08;
        let y = hy + (t * speed * 0.8 + fi * 1.7).cos() * 0.06;
        x = (x + t * 0.006 * (0.5 + sy)).rem_euclid(aspect + 0.5) - 0.25;
        *b = [x, y, 0.1 + 0.14 * sy, (i % 4) as f32];
    }
    out
}

pub struct Watercolor {
    view: Retained<NSView>,
    layer: MetalLayer,
    queue: CommandQueue,
    pipeline: RenderPipelineState,
    start: Instant,
    drawable: (f64, f64),
    frames: u32,
}

impl Watercolor {
    /// Puts the wash below gpui's `above` view in `window`. `None` when Metal or the shader
    /// fails, which is said on stderr.
    pub fn install(window: &NSWindow, above: &NSView) -> Option<Watercolor> {
        let mtm = MainThreadMarker::new()?;
        let content = window.contentView()?;
        let Some(device) = Device::system_default() else {
            eprintln!("arkos: effect: no Metal device");
            return None;
        };
        let source = match std::env::var_os("ARKOS_EFFECT_SHADER").map(std::fs::read_to_string) {
            Some(Ok(s)) => s,
            Some(Err(e)) => {
                eprintln!("arkos: effect: cannot read ARKOS_EFFECT_SHADER: {e}");
                return None;
            }
            None => SHADER.to_string(),
        };
        let library = match device.new_library_with_source(&source, &CompileOptions::new()) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("arkos: effect: shader did not compile: {e}");
                return None;
            }
        };
        let vs = library.get_function("watercolor_vertex", None).ok()?;
        let fs = library.get_function("watercolor_fragment", None).ok()?;
        let desc = RenderPipelineDescriptor::new();
        desc.set_vertex_function(Some(&vs));
        desc.set_fragment_function(Some(&fs));
        desc.color_attachments().object_at(0)?.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        let pipeline = match device.new_render_pipeline_state(&desc) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("arkos: effect: pipeline: {e}");
                return None;
            }
        };
        let layer = MetalLayer::new();
        layer.set_device(&device);
        layer.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        layer.set_opaque(true);
        layer.set_framebuffer_only(true);
        let view = NSView::initWithFrame(mtm.alloc(), content.bounds());
        view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
        view.setWantsLayer(true);
        // SAFETY: the layer pointer is a live CAMetalLayer, which the view retains from here on.
        unsafe {
            let _: () = msg_send![&*view, setLayer: layer.as_ptr() as *mut AnyObject];
        }
        content.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, Some(above));
        ACTIVE.store(true, Ordering::Relaxed);
        Some(Watercolor { view, layer, queue: device.new_command_queue(), pipeline, start: Instant::now(), drawable: (0.0, 0.0), frames: 0 })
    }

    /// Draws one frame: the theme's `paper`, `ink` and four pigments, under the UI's `regions`.
    pub fn draw(&mut self, paper: Rgb, ink: Rgb, tints: [Rgb; 4], regions: &[Region]) {
        let bounds = self.view.bounds();
        let (pw, ph) = (bounds.size.width, bounds.size.height);
        let scale = self.view.window().map(|w| w.backingScaleFactor()).unwrap_or(2.0) * RENDER_SCALE;
        let size = ((pw * scale).round(), (ph * scale).round());
        if size.0 < 1.0 || size.1 < 1.0 {
            return;
        }
        if size != self.drawable {
            self.layer.set_drawable_size(core_graphics_types::geometry::CGSize { width: size.0, height: size.1 });
            // The layer's own scale stays the screen's; the smaller drawable is stretched to it.
            unsafe {
                let _: () = msg_send![self.layer.as_ptr() as *mut AnyObject, setContentsScale: scale / RENDER_SCALE];
            }
            self.drawable = size;
        }
        let Some(drawable) = self.layer.next_drawable() else { return };
        let pass = RenderPassDescriptor::new();
        let Some(att) = pass.color_attachments().object_at(0) else { return };
        att.set_texture(Some(drawable.texture()));
        att.set_load_action(MTLLoadAction::Clear);
        att.set_store_action(MTLStoreAction::Store);
        let t = self.start.elapsed().as_secs_f32();
        let mut rects = [[0.0; 4]; MAX_REGIONS];
        let mut metas = [[0.0; 4]; MAX_REGIONS];
        let n = regions.len().min(MAX_REGIONS);
        for (i, r) in regions.iter().take(n).enumerate() {
            rects[i] = [r.x, r.y, r.w, r.h];
            metas[i] = [r.tint as f32, r.strength, r.ink as f32, r.seed];
        }
        let u = Uniforms {
            time: t,
            bubble_count: BUBBLES as f32,
            size: [pw as f32, ph as f32],
            paper: float4(paper),
            ink: float4(ink),
            tints: [float4(tints[0]), float4(tints[1]), float4(tints[2]), float4(tints[3])],
            bubbles: bubbles(t, (pw / ph) as f32),
            region_count: n as f32,
            pad: [0.0; 3],
            rects,
            metas,
        };
        let cmd = self.queue.new_command_buffer();
        let enc = cmd.new_render_command_encoder(pass);
        enc.set_render_pipeline_state(&self.pipeline);
        enc.set_fragment_bytes(0, std::mem::size_of::<Uniforms>() as u64, &u as *const Uniforms as *const std::ffi::c_void);
        enc.draw_primitives(MTLPrimitiveType::Triangle, 0, 3);
        enc.end_encoding();
        cmd.present_drawable(drawable);
        cmd.commit();
        // The spike's measurement: every sixtieth frame waits for the GPU and says how long the
        // frame took on it, so the cost is known whatever else the machine is drawing.
        self.frames += 1;
        if self.frames % 60 == 0 {
            cmd.wait_until_completed();
            // SAFETY: GPUStartTime and GPUEndTime are CFTimeInterval properties of a completed
            // command buffer, read on the main thread.
            let (start, end): (f64, f64) = unsafe {
                let b = cmd.as_ptr() as *mut AnyObject;
                (msg_send![b, GPUStartTime], msg_send![b, GPUEndTime])
            };
            eprintln!("arkos: effect: gpu {:.2} ms per frame at {}x{}, {} regions", (end - start) * 1000.0, size.0, size.1, n);
        }
    }
}

impl Drop for Watercolor {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Relaxed);
        self.view.removeFromSuperview();
    }
}
