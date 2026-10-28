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

pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Whether the wash goes with the theme named: Peach Light carries it; `ARKOS_EFFECT=1` puts it
/// under any theme for a look.
pub fn wanted(theme_name: &str) -> bool {
    theme_name == "peach-light" || std::env::var_os("ARKOS_EFFECT").is_some()
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
}

const SHADER: &str = r#"
#include <metal_stdlib>
using namespace metal;

struct Uniforms {
    float time;
    float bubble_count;
    float2 size;
    float4 paper;
    float4 ink;
    float4 tints[4];
    float4 bubbles[16];
    float region_count;
    float3 pad;
    float4 rects[32];
    float4 metas[32];
};

struct VOut {
    float4 pos [[position]];
    float2 uv;
};

vertex VOut watercolor_vertex(uint vid [[vertex_id]]) {
    float2 p = float2(vid == 1 ? 3.0 : -1.0, vid == 2 ? 3.0 : -1.0);
    VOut o;
    o.pos = float4(p, 0.0, 1.0);
    o.uv = float2((p.x + 1.0) * 0.5, 1.0 - (p.y + 1.0) * 0.5);
    return o;
}

float hash1(float n) { return fract(sin(n) * 43758.5453123); }

float vnoise(float2 p) {
    float2 i = floor(p);
    float2 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    float n = i.x + i.y * 57.0;
    return mix(mix(hash1(n), hash1(n + 1.0), f.x), mix(hash1(n + 57.0), hash1(n + 58.0), f.x), f.y);
}

float fbm(float2 p) {
    float v = 0.0;
    float a = 0.5;
    for (int i = 0; i < 3; i++) {
        v += a * vnoise(p);
        p *= 2.03;
        a *= 0.5;
    }
    return v;
}

// A pigment pulled 40% of the way to its own grey: dustier, as a muted palette is.
float3 muted(float3 c) {
    float l = dot(c, float3(0.299, 0.587, 0.114));
    return mix(c, float3(l), 0.4);
}

float sd_box(float2 q, float2 hb) {
    float2 d = abs(q) - hb;
    return length(max(d, 0.0)) + min(max(d.x, d.y), 0.0);
}

// The distance from p to an ink line along one edge: a horizontal (or vertical) segment from
// a to b at `at`, overshooting its corners a little as a quick pen stroke does.
float ink_h(float2 p, float y, float x0, float x1, float wob, float over) {
    float dx = max(max(x0 - over - p.x, p.x - x1 - over), 0.0);
    return length(float2(dx, p.y - y - wob));
}
float ink_v(float2 p, float x, float y0, float y1, float wob, float over) {
    float dy = max(max(y0 - over - p.y, p.y - y1 - over), 0.0);
    return length(float2(p.x - x - wob, dy));
}

fragment float4 watercolor_fragment(VOut in [[stage_in]], constant Uniforms& u [[buffer(0)]]) {
    float2 p = in.uv * u.size;
    float t = u.time;

    // Shared fields, computed once per pixel and read by every block: the ragged bleed (wider
    // across than down, as a brush stroke runs), fine edge fray, granulation, and the pen's
    // wobble and weight. All drift very slowly, so the edges breathe.
    float bleed = fbm(float2(p.x * 0.018, p.y * 0.05) + t * 0.012);
    float fray = vnoise(p * 0.16 + t * 0.03);
    float gran = fbm(p * 0.045 - t * 0.008);
    float speck = vnoise(p * 0.9);
    float wob = (vnoise(p * 0.035 + t * 0.05) - 0.5) * 2.6;
    float weight = 0.55 + 0.55 * vnoise(p * 0.02 + 7.0);
    float gap = smoothstep(0.08, 0.2, vnoise(p * 0.028 + 3.0));

    // Paper with grain.
    float3 col = u.paper.rgb * (0.975 + 0.035 * speck + 0.02 * gran);

    // Bubbles behind everything.
    int nb = int(u.bubble_count);
    float2 bp = p / u.size.y;
    for (int i = 0; i < nb; i++) {
        float4 b = u.bubbles[i];
        float r = b.z;
        float dist = length(bp - b.xy) + (bleed - 0.5) * 0.3 * r;
        if (dist > r * 1.05) {
            continue;
        }
        float body = 1.0 - smoothstep(r * 0.5, r, dist);
        float rim = smoothstep(r * 0.75, r * 0.96, dist) * (1.0 - smoothstep(r * 0.96, r * 1.02, dist));
        float a = (0.24 * body + 0.22 * rim) * (0.65 + 0.5 * gran);
        col *= mix(float3(1.0), muted(u.tints[int(b.w)].rgb), clamp(a, 0.0, 0.9));
    }

    // Washes, then ink, for every block.
    int nr = int(u.region_count);
    float inked = 0.0;
    for (int i = 0; i < nr; i++) {
        float4 r = u.rects[i];
        float4 m = u.metas[i];
        float2 hb = r.zw * 0.5;
        float2 c = r.xy + hb;
        float seed = float(i) * 1.7;
        if (m.y > 0.0) {
            float d = sd_box(p - c, hb);
            if (d < 12.0) {
                // Close to the edge: a point or two past it, ragged only a little.
                float edge = d - 1.5 + (bleed - 0.5) * 8.0 + (fray - 0.5) * 2.0;
                float cover = 1.0 - smoothstep(-0.7, 0.7, edge);
                float pool = smoothstep(-5.0, -0.4, edge) * cover;
                float blotch = 0.72 + 0.5 * fract(gran + seed * 0.37);
                float a = m.y * (cover * blotch + 0.5 * pool);
                col *= mix(float3(1.0), muted(u.tints[int(m.x)].rgb), clamp(a, 0.0, 0.85));
            }
        }
        uint edges = uint(m.z);
        if (edges != 0u) {
            float x0 = r.x, y0 = r.y, x1 = r.x + r.z, y1 = r.y + r.w;
            float over = 3.0 + 5.0 * hash1(seed);
            float dmin = 1e5;
            if (edges & 1u) { dmin = min(dmin, ink_h(p, y0, x0, x1, wob, over)); }
            if (edges & 2u) { dmin = min(dmin, ink_v(p, x1, y0, y1, wob, over)); }
            if (edges & 4u) { dmin = min(dmin, ink_h(p, y1, x0, x1, wob, over)); }
            if (edges & 8u) { dmin = min(dmin, ink_v(p, x0, y0, y1, wob, over)); }
            float line = 1.0 - smoothstep(weight - 0.55, weight + 0.55, dmin);
            inked = max(inked, line * gap);
        }
    }
    col = mix(col, u.ink.rgb, inked * 0.82);
    return float4(col, 1.0);
}
"#;

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
            metas[i] = [r.tint as f32, r.strength, r.ink as f32, 0.0];
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
