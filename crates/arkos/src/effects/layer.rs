//! The wash's Metal layer (spec 4.4): a `CAMetalLayer` in its own `NSView` below gpui's view in
//! the window's content view, drawn with the effect's fragment stage over a full-window
//! triangle; and, for a wash with a menu stage (`arkos_overlay`), a second, see-through
//! `CAMetalLayer` on top of gpui's own layer that draws the open menus' edges above the panes.
//! A sublayer takes no events, so the menus below it are clicked as ever. AppKit and Metal only;
//! what to draw and when is `active`'s and `Root`'s.

use super::inputs::Inputs;
use metal::foreign_types::{ForeignType, ForeignTypeRef};
use metal::{CommandBufferRef, CommandQueue, CompileOptions, Device, DeviceRef, MTLLoadAction, MTLPixelFormat, MTLPrimitiveType, MTLStoreAction, MetalLayer, RenderPassDescriptor, RenderPipelineDescriptor, RenderPipelineState};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, MainThreadMarker};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSWindow, NSWindowOrderingMode};

/// The vertex stage every wash is compiled with, appended after it.
const VERTEX: &str = include_str!("../../effects/vertex.metal");

/// The size in pixels of the drawable for a view of `points`, on a screen of `backing` pixels per
/// point, drawn at `scale` of the screen's pixels each side.
pub fn drawable_size(points: (f64, f64), backing: f64, scale: f64) -> (f64, f64) {
    ((points.0 * backing * scale).round(), (points.1 * backing * scale).round())
}

/// The Metal compiler's `message` for `format!("{preamble}\n{wash}")`, with every
/// `program_source:LINE:` inside the wash renumbered to the wash's own line, and one inside the
/// preamble named `arkos.metal:LINE:`, the contract's own file.
pub fn point_into_wash(message: &str, preamble: &str) -> String {
    point_into(message, preamble, "arkos.metal", "program_source")
}

/// The Metal compiler's `message` for `format!("{before}\n{source}")`, with every
/// `program_source:LINE:` written `{file}:LINE:` with the source's own line when it falls in
/// `source`, and `{before_name}:LINE:` when it falls in `before`.
pub fn point_into(message: &str, before: &str, before_name: &str, file: &str) -> String {
    const TAG: &str = "program_source:";
    let offset = before.matches('\n').count() + 1;
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(at) = rest.find(TAG) {
        out.push_str(&rest[..at]);
        let after = &rest[at + TAG.len()..];
        let digits = after.chars().take_while(char::is_ascii_digit).count();
        match after[..digits].parse::<usize>() {
            Ok(line) if line > offset => out.push_str(&format!("{file}:{}", line - offset)),
            Ok(line) => out.push_str(&format!("{before_name}:{line}")),
            Err(_) => out.push_str(TAG),
        }
        rest = &after[digits..];
    }
    out.push_str(rest);
    out
}

/// The wash's render pipeline on `device`: `wash` compiled after `preamble` with the app's vertex
/// stage, drawing into a BGRA8 target. The window's layer draws with it, `arkos effect bench`
/// renders with it and `arkos effect check` builds it. `Err` carries the compiler's message, its
/// lines pointing into the wash, or what else went wrong.
pub fn pipeline(device: &DeviceRef, preamble: &str, wash: &str) -> Result<RenderPipelineState, String> {
    pipeline_for(device, preamble, wash, "arkos_wash")
}

/// The pipeline of the wash's fragment function `fragment`: `arkos_wash`, or its menu stage
/// `arkos_overlay`.
pub fn pipeline_for(device: &DeviceRef, preamble: &str, wash: &str, fragment: &str) -> Result<RenderPipelineState, String> {
    let source = format!("{preamble}\n{wash}\n{VERTEX}");
    let library = device.new_library_with_source(&source, &CompileOptions::new()).map_err(|e| point_into_wash(&e, preamble))?;
    let vs = library.get_function("arkos_vertex", None)?;
    let fs = library.get_function(fragment, None).map_err(|_| format!("the wash has no fragment function named {fragment}"))?;
    let desc = RenderPipelineDescriptor::new();
    desc.set_vertex_function(Some(&vs));
    desc.set_fragment_function(Some(&fs));
    desc.color_attachments().object_at(0).ok_or("no colour attachment")?.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    device.new_render_pipeline_state(&desc)
}

/// The GPU's time in milliseconds for a command buffer that has completed.
pub fn gpu_ms(cmd: &CommandBufferRef) -> f32 {
    // SAFETY: GPUStartTime and GPUEndTime are CFTimeInterval properties of a command buffer,
    // read once it has completed.
    let (start, end): (f64, f64) = unsafe {
        let b = cmd.as_ptr() as *mut AnyObject;
        (msg_send![b, GPUStartTime], msg_send![b, GPUEndTime])
    };
    ((end - start) * 1000.0) as f32
}

/// The wash's layer in one window. Dropping it takes the view out of the window.
pub struct Layer {
    view: Retained<NSView>,
    layer: MetalLayer,
    queue: CommandQueue,
    pipeline: RenderPipelineState,
    /// The manifest's `scale`: the wash's pixels per screen pixel, each side.
    scale: f64,
    drawable: (f64, f64),
    frames: u32,
    gpu_ms: Option<f32>,
    /// The menu stage's layer, for a wash that has one.
    overlay: Option<Overlay>,
}

/// The menu stage's layer: a see-through `CAMetalLayer` added on top of gpui's own layer, hidden
/// while no menu is open.
struct Overlay {
    layer: MetalLayer,
    pipeline: RenderPipelineState,
    drawable: (f64, f64),
    shown: bool,
}

/// `kCALayerWidthSizable | kCALayerHeightSizable`: the layer follows its superlayer's size.
const LAYER_SIZABLE: u32 = 2 | 16;

impl Overlay {
    fn install(device: &DeviceRef, above: &NSView, pipeline: RenderPipelineState) -> Result<Overlay, String> {
        let layer = MetalLayer::new();
        layer.set_device(device);
        layer.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        layer.set_opaque(false);
        layer.set_framebuffer_only(true);
        // SAFETY: gpui's view is layer-backed with a live layer; the new layer is a live
        // CAMetalLayer, retained by its superlayer once added. Every call is on the main thread.
        unsafe {
            let host: *mut AnyObject = msg_send![above, layer];
            if host.is_null() {
                return Err("gpui's view has no layer".into());
            }
            let bounds: objc2_foundation::NSRect = msg_send![host, bounds];
            let me = layer.as_ptr() as *mut AnyObject;
            let _: () = msg_send![me, setFrame: bounds];
            let _: () = msg_send![me, setAutoresizingMask: LAYER_SIZABLE];
            let _: () = msg_send![me, setHidden: true];
            let _: () = msg_send![host, addSublayer: me];
        }
        Ok(Overlay { layer, pipeline, drawable: (0.0, 0.0), shown: false })
    }

    fn set_hidden(&mut self, hidden: bool) {
        // SAFETY: setHidden: is a CALayer property, set on the main thread.
        unsafe {
            let _: () = msg_send![self.layer.as_ptr() as *mut AnyObject, setHidden: hidden];
        }
        self.shown = !hidden;
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        // SAFETY: the layer is live; removing it from its superlayer releases that reference.
        unsafe {
            let _: () = msg_send![self.layer.as_ptr() as *mut AnyObject, removeFromSuperlayer];
        }
    }
}

impl Layer {
    /// Compiles `wash` after `preamble` with the app's vertex stage and puts its layer below
    /// gpui's `above` view in `window`, drawing at `scale` of the screen's pixels. `Err` carries
    /// the compiler's message, its lines pointing into the wash, or what else went wrong.
    pub fn install(window: &NSWindow, above: &NSView, preamble: &str, wash: &str, scale: f32) -> Result<Layer, String> {
        let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
        let content = window.contentView().ok_or("the window has no content view")?;
        let device = Device::system_default().ok_or("no Metal device")?;
        let pipeline = pipeline(&device, preamble, wash)?;
        let overlay = if crate::effects::active::has_overlay(wash) {
            Some(Overlay::install(&device, above, pipeline_for(&device, preamble, wash, "arkos_overlay")?)?)
        } else {
            None
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
        Ok(Layer { view, layer, queue: device.new_command_queue(), pipeline, scale: scale as f64, drawable: (0.0, 0.0), frames: 0, gpu_ms: None, overlay })
    }

    fn backing(&self) -> f64 {
        self.view.window().map(|w| w.backingScaleFactor()).unwrap_or(2.0)
    }

    /// The view's size in points and the drawable's pixels per point, as the next frame's
    /// inputs say them.
    pub fn metrics(&self) -> ((f32, f32), f32) {
        let b = self.view.bounds();
        ((b.size.width as f32, b.size.height as f32), (self.backing() * self.scale) as f32)
    }

    /// Draws one frame with `inputs`. The drawable follows the view's size at every frame.
    pub fn draw(&mut self, inputs: &Inputs) {
        let bounds = self.view.bounds();
        let backing = self.backing();
        let size = drawable_size((bounds.size.width, bounds.size.height), backing, self.scale);
        if size.0 < 1.0 || size.1 < 1.0 {
            return;
        }
        if size != self.drawable {
            self.layer.set_drawable_size(core_graphics_types::geometry::CGSize { width: size.0, height: size.1 });
            // The layer's own scale stays the screen's; the smaller drawable is stretched to it.
            // SAFETY: setContentsScale: is a CALayer property, set on the main thread.
            unsafe {
                let _: () = msg_send![self.layer.as_ptr() as *mut AnyObject, setContentsScale: backing];
            }
            self.drawable = size;
        }
        let Some(drawable) = self.layer.next_drawable() else { return };
        let pass = RenderPassDescriptor::new();
        let Some(att) = pass.color_attachments().object_at(0) else { return };
        att.set_texture(Some(drawable.texture()));
        att.set_load_action(MTLLoadAction::Clear);
        att.set_store_action(MTLStoreAction::Store);
        let cmd = self.queue.new_command_buffer();
        let enc = cmd.new_render_command_encoder(pass);
        enc.set_render_pipeline_state(&self.pipeline);
        enc.set_fragment_bytes(0, std::mem::size_of::<Inputs>() as u64, inputs as *const Inputs as *const std::ffi::c_void);
        enc.draw_primitives(MTLPrimitiveType::Triangle, 0, 3);
        enc.end_encoding();
        cmd.present_drawable(drawable);
        cmd.commit();
        // Every sixtieth frame waits for the GPU and reads how long the frame took on it, so the
        // cost is known whatever else the machine is drawing.
        self.frames += 1;
        if self.frames % 60 == 0 {
            cmd.wait_until_completed();
            let ms = gpu_ms(cmd);
            self.gpu_ms = Some(self.gpu_ms.map_or(ms, |avg| avg * 0.75 + ms * 0.25));
        }
    }

    /// Draws the menu stage with `inputs` (the open menus' blocks), or hides its layer for
    /// `None`. A wash without a menu stage has nothing to draw.
    pub fn draw_overlay(&mut self, inputs: Option<&Inputs>) {
        let backing = self.backing();
        let bounds = self.view.bounds();
        let size = drawable_size((bounds.size.width, bounds.size.height), backing, self.scale);
        let Some(overlay) = self.overlay.as_mut() else { return };
        let Some(inputs) = inputs else {
            if overlay.shown {
                overlay.set_hidden(true);
            }
            return;
        };
        if size.0 < 1.0 || size.1 < 1.0 {
            return;
        }
        if size != overlay.drawable {
            overlay.layer.set_drawable_size(core_graphics_types::geometry::CGSize { width: size.0, height: size.1 });
            // SAFETY: setContentsScale: is a CALayer property, set on the main thread.
            unsafe {
                let _: () = msg_send![overlay.layer.as_ptr() as *mut AnyObject, setContentsScale: backing];
            }
            overlay.drawable = size;
        }
        let Some(drawable) = overlay.layer.next_drawable() else { return };
        let pass = RenderPassDescriptor::new();
        let Some(att) = pass.color_attachments().object_at(0) else { return };
        att.set_texture(Some(drawable.texture()));
        att.set_load_action(MTLLoadAction::Clear);
        att.set_clear_color(metal::MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
        att.set_store_action(MTLStoreAction::Store);
        let cmd = self.queue.new_command_buffer();
        let enc = cmd.new_render_command_encoder(pass);
        enc.set_render_pipeline_state(&overlay.pipeline);
        enc.set_fragment_bytes(0, std::mem::size_of::<Inputs>() as u64, inputs as *const Inputs as *const std::ffi::c_void);
        enc.draw_primitives(MTLPrimitiveType::Triangle, 0, 3);
        enc.end_encoding();
        cmd.present_drawable(drawable);
        cmd.commit();
        if !overlay.shown {
            overlay.set_hidden(false);
        }
    }

    /// The GPU's time per frame in milliseconds, a running average of every sixtieth frame;
    /// `None` before the first reading.
    pub fn gpu_ms(&self) -> Option<f32> {
        self.gpu_ms
    }
}

impl Drop for Layer {
    fn drop(&mut self) {
        self.view.removeFromSuperview();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_compiler_s_lines_point_into_the_wash_file() {
        let preamble = "a\nb\nc\n";
        // The wash's first line follows the preamble and the blank line that joins them.
        let message = "Compilation failed: \n\nprogram_source:5:3: error: x\nprogram_source:12:1: note: y";
        assert_eq!(point_into_wash(message, preamble), "Compilation failed: \n\nprogram_source:1:3: error: x\nprogram_source:8:1: note: y");
        // A line before the wash is the preamble's own, in the contract's file.
        assert_eq!(point_into_wash("program_source:2:1: note: z", preamble), "arkos.metal:2:1: note: z");
        // Any source after any other, each line named for its own file.
        assert_eq!(point_into("program_source:6:2: error: x\nprogram_source:1:1: note: y", preamble, "gpui", "glyphs.metal"), "glyphs.metal:2:2: error: x\ngpui:1:1: note: y");
    }
}
