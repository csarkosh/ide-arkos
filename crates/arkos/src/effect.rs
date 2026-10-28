//! SPIKE, throwaway: a watercolor wash with soft bubbles drifting under the window's content.
//!
//! A `CAMetalLayer` in its own `NSView` sits below gpui's view in the window's content view and
//! runs a fragment shader at thirty frames a second from its own timer. gpui draws nothing for
//! it: the window is transparent and the theme's surfaces are painted with some alpha, so the
//! wash shows through the chrome and, faintly, through the terminals. The shader is Metal
//! source compiled at launch (`ARKOS_EFFECT_SHADER=<file>` replaces the built-in one), the
//! first step towards user-loaded effects. The colours are the theme's: the paper is the window
//! background, the pigments four of the theme's own tints, read on every frame. The bubbles'
//! places are worked out here each frame and handed to the shader, which keeps the per-pixel
//! work to two noise fields shared by every bubble.

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

/// The alpha of a terminal's or a pane's content over the wash, and of the bars and the sidebar.
pub const SURFACE_ALPHA: f32 = 0.74;
pub const CHROME_ALPHA: f32 = 0.5;

/// The wash's pixels per window pixel on each side (see `draw`).
const RENDER_SCALE: f64 = 0.25;

/// How many bubbles drift about.
const BUBBLES: usize = 22;

/// The built-in shader: a full-screen triangle, then paper grain and the bubbles the CPU placed,
/// multiplied onto the paper so overlaps deepen as wet layers do. One wobble field and one
/// pigment field serve every bubble.
const SHADER: &str = r#"
#include <metal_stdlib>
using namespace metal;

struct Uniforms {
    float time;
    float count;
    float2 resolution;
    float4 base;
    float4 tints[4];
    float4 bubbles[24];
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

fragment float4 watercolor_fragment(VOut in [[stage_in]], constant Uniforms& u [[buffer(0)]]) {
    float2 res = u.resolution;
    float2 px = in.uv * res;
    float aspect = res.x / res.y;
    float2 q = float2(in.uv.x * aspect, in.uv.y);
    float3 paper = u.base.rgb;

    // Paper grain: two scales of value noise, a few percent of brightness.
    float grain = vnoise(px * 0.9) * 0.5 + vnoise(px * 0.35) * 0.5;
    float3 col = paper * (0.985 + 0.03 * grain);

    // A wet edge for every bubble, and uneven pigment, from two slowly moving fields.
    float wobble = fbm(q * 6.0 + u.time * 0.03) - 0.5;
    float pigment = 0.6 + 0.4 * fbm(q * 3.0 - u.time * 0.02);

    int n = int(u.count);
    for (int i = 0; i < n; i++) {
        float4 b = u.bubbles[i];
        float r = b.z;
        float2 d = q - b.xy;
        float dist = length(d) + wobble * 0.25 * r;
        if (dist > r * 1.05) {
            continue;
        }
        float body = 1.0 - smoothstep(r * 0.55, r, dist);
        float rim = smoothstep(r * 0.78, r * 0.97, dist) * (1.0 - smoothstep(r * 0.97, r * 1.02, dist));
        float a = (0.22 * body + 0.2 * rim) * pigment;
        float3 tint = u.tints[int(b.w)].rgb;
        col *= mix(float3(1.0), tint, a);
    }
    return float4(col, 1.0);
}
"#;

#[repr(C)]
struct Uniforms {
    time: f32,
    count: f32,
    resolution: [f32; 2],
    base: [f32; 4],
    tints: [[f32; 4]; 4],
    bubbles: [[f32; 4]; 24],
}

fn float4(c: Rgb) -> [f32; 4] {
    [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0, 1.0]
}

fn hash(n: f32) -> f32 {
    (n.sin() * 43758.547).fract().abs()
}

/// Where each bubble is at `t` seconds, in units where the window is `aspect` wide and 1 high:
/// x, y, radius and which of the four tints. Each has its own slow sway and a slower drift
/// across, wrapping with a margin so it leaves before it comes back.
pub fn bubbles(t: f32, aspect: f32) -> [[f32; 4]; 24] {
    let mut out = [[0.0; 4]; 24];
    for (i, b) in out.iter_mut().enumerate().take(BUBBLES) {
        let fi = i as f32;
        let (hx, hy) = (hash(fi * 7.31), hash(fi * 7.31 + 1.0));
        let (sx, sy) = (hash(fi * 3.17 + 11.0), hash(fi * 3.17 + 12.0));
        let speed = 0.05 + 0.07 * sx;
        let mut x = hx * aspect + (t * speed + fi).sin() * 0.08;
        let y = hy + (t * speed * 0.8 + fi * 1.7).cos() * 0.06;
        x = (x + t * 0.006 * (0.5 + sy)).rem_euclid(aspect + 0.4) - 0.2;
        *b = [x, y, 0.07 + 0.12 * sy, (i % 4) as f32];
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

    /// Draws one frame with the theme's paper and pigments.
    pub fn draw(&mut self, paper: Rgb, tints: [Rgb; 4]) {
        let bounds = self.view.bounds();
        // The wash is soft, so it is drawn at a quarter of the window's pixels on each side and
        // scaled up by Core Animation: a sixteenth of the fragment work of a full-size pass.
        let scale = self.view.window().map(|w| w.backingScaleFactor()).unwrap_or(2.0) * RENDER_SCALE;
        let size = ((bounds.size.width * scale).round(), (bounds.size.height * scale).round());
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
        let u = Uniforms {
            time: t,
            count: BUBBLES as f32,
            resolution: [size.0 as f32, size.1 as f32],
            base: float4(paper),
            tints: [float4(tints[0]), float4(tints[1]), float4(tints[2]), float4(tints[3])],
            bubbles: bubbles(t, (size.0 / size.1) as f32),
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
            eprintln!("arkos: effect: gpu {:.2} ms per frame at {}x{}", (end - start) * 1000.0, size.0, size.1);
        }
    }
}

impl Drop for Watercolor {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Relaxed);
        self.view.removeFromSuperview();
    }
}
