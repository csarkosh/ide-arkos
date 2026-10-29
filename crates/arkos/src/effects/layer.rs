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
    stages(device, preamble, wash, "arkos_vertex", fragment, false)
}

/// The pipeline a menu's backdrop is rendered with: the wash over the menu's place alone, faded
/// with the menu. `arkos effect check` builds it as the window does.
pub fn backdrop_pipeline(device: &DeviceRef, preamble: &str, wash: &str) -> Result<RenderPipelineState, String> {
    stages(device, preamble, wash, "arkos_vertex_area", "arkos_wash", true)
}

/// The pipeline of the app's vertex function `vertex` and the wash's `fragment`. With `fade`,
/// the colour is written as it is and its alpha is the encoder's blend colour's (a menu's
/// backdrop fading with its menu).
fn stages(device: &DeviceRef, preamble: &str, wash: &str, vertex: &str, fragment: &str, fade: bool) -> Result<RenderPipelineState, String> {
    let source = format!("{preamble}\n{wash}\n{VERTEX}");
    let library = device.new_library_with_source(&source, &CompileOptions::new()).map_err(|e| point_into_wash(&e, preamble))?;
    let vs = library.get_function(vertex, None)?;
    let fs = library.get_function(fragment, None).map_err(|_| format!("the wash has no fragment function named {fragment}"))?;
    let desc = RenderPipelineDescriptor::new();
    desc.set_vertex_function(Some(&vs));
    desc.set_fragment_function(Some(&fs));
    let att = desc.color_attachments().object_at(0).ok_or("no colour attachment")?;
    att.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    if fade {
        att.set_blending_enabled(true);
        att.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
        att.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
        att.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
        att.set_destination_rgb_blend_factor(metal::MTLBlendFactor::Zero);
        att.set_source_alpha_blend_factor(metal::MTLBlendFactor::BlendAlpha);
        att.set_destination_alpha_blend_factor(metal::MTLBlendFactor::Zero);
    }
    device.new_render_pipeline_state(&desc)
}

/// The menus' backdrops: for each open menu, the wash rendered over the menu's place into an
/// IOSurface-backed BGRA image of its own, which the menu shows behind its text
/// (`Window::paint_surface`) under a paper as a pane's. Kept per menu and remade only when its
/// size changes.
struct Backdrops {
    pipeline: RenderPipelineState,
    cache: core_video::metal_texture_cache::CVMetalTextureCache,
    images: std::collections::HashMap<u64, core_video::pixel_buffer::CVPixelBuffer>,
}

impl Backdrops {
    fn new(device: &Device, preamble: &str, wash: &str) -> Result<Backdrops, String> {
        let pipeline = backdrop_pipeline(device, preamble, wash)?;
        let cache = core_video::metal_texture_cache::CVMetalTextureCache::new(None, device.clone(), None).map_err(|e| format!("no texture cache ({e})"))?;
        Ok(Backdrops { pipeline, cache, images: std::collections::HashMap::new() })
    }

    /// The image kept for `key`, `w` by `h` pixels: the one already there, or a new one.
    fn image(&mut self, key: u64, w: usize, h: usize) -> Option<core_video::pixel_buffer::CVPixelBuffer> {
        use core_foundation::base::TCFType;
        use core_video::pixel_buffer::{kCVPixelFormatType_32BGRA, CVPixelBuffer, CVPixelBufferKeys};
        if let Some(img) = self.images.get(&key).filter(|i| i.get_width() == w && i.get_height() == h) {
            return Some(img.clone());
        }
        let surface = core_foundation::dictionary::CFDictionary::<core_foundation::string::CFString, core_foundation::base::CFType>::from_CFType_pairs(&[]);
        let attributes = core_foundation::dictionary::CFDictionary::from_CFType_pairs(&[
            (core_foundation::string::CFString::from(CVPixelBufferKeys::IOSurfaceProperties), surface.as_CFType()),
            (core_foundation::string::CFString::from(CVPixelBufferKeys::MetalCompatibility), core_foundation::boolean::CFBoolean::true_value().as_CFType()),
        ]);
        let img = CVPixelBuffer::new(kCVPixelFormatType_32BGRA, w, h, Some(&attributes)).ok()?;
        self.images.insert(key, img.clone());
        Some(img)
    }

    /// Renders the wash with `inputs` over `area` (window points, the window `size` points) into
    /// `key`'s image at `px` pixels a point, faded to `opacity`, and returns the image once the
    /// GPU has drawn it.
    fn render(&mut self, queue: &CommandQueue, key: u64, area: crate::tree::Rect, size: (f32, f32), px: f32, opacity: f32, inputs: &Inputs) -> Option<core_video::pixel_buffer::CVPixelBuffer> {
        use core_foundation::base::TCFType;
        let (w, h) = (((area.w * px).round() as usize).max(1), ((area.h * px).round() as usize).max(1));
        let img = self.image(key, w, h)?;
        let cv_texture = self.cache.create_texture_from_image(img.as_concrete_TypeRef(), None, MTLPixelFormat::BGRA8Unorm, w, h, 0).ok()?;
        // Borrowed, as gpui borrows a video frame's: `CVMetalTextureGetTexture` follows the get
        // rule, so the texture lives as long as `cv_texture` and must not be released here.
        // (`CVMetalTexture::get_texture` wraps it as owned and over-releases it.)
        // SAFETY: the pointer is the live texture of `cv_texture`, which outlives every use below.
        let texture = unsafe {
            let raw = core_video::metal_texture::CVMetalTextureGetTexture(cv_texture.as_concrete_TypeRef());
            if raw.is_null() {
                return None;
            }
            metal::TextureRef::from_ptr(raw as *mut _)
        };
        let pass = RenderPassDescriptor::new();
        let att = pass.color_attachments().object_at(0)?;
        att.set_texture(Some(texture));
        att.set_load_action(MTLLoadAction::Clear);
        att.set_clear_color(metal::MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
        att.set_store_action(MTLStoreAction::Store);
        let share = [area.x / size.0.max(1.0), area.y / size.1.max(1.0), area.w / size.0.max(1.0), area.h / size.1.max(1.0)];
        let cmd = queue.new_command_buffer();
        let enc = cmd.new_render_command_encoder(pass);
        enc.set_render_pipeline_state(&self.pipeline);
        enc.set_blend_color(0.0, 0.0, 0.0, opacity.clamp(0.0, 1.0));
        enc.set_vertex_bytes(0, std::mem::size_of_val(&share) as u64, share.as_ptr() as *const std::ffi::c_void);
        enc.set_fragment_bytes(0, std::mem::size_of::<Inputs>() as u64, inputs as *const Inputs as *const std::ffi::c_void);
        enc.draw_primitives(MTLPrimitiveType::Triangle, 0, 3);
        enc.end_encoding();
        cmd.commit();
        // gpui samples the image on its own queue later in this frame: it must be drawn by then.
        cmd.wait_until_completed();
        Some(img)
    }
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
    /// The menus' backdrops, for a wash with a menu stage.
    backdrops: Option<Backdrops>,
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
        let (overlay, backdrops) = if crate::effects::active::has_overlay(wash) {
            (Some(Overlay::install(&device, above, pipeline_for(&device, preamble, wash, "arkos_overlay")?)?), Some(Backdrops::new(&device, preamble, wash)?))
        } else {
            (None, None)
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
        Ok(Layer { view, layer, queue: device.new_command_queue(), pipeline, scale: scale as f64, drawable: (0.0, 0.0), frames: 0, gpu_ms: None, overlay, backdrops })
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

    /// The backdrop of the menu `key` at `area` (window points), faded to `opacity`: the wash
    /// with `inputs` rendered over it at the wash's own pixels a point. `None` for a wash without
    /// a menu stage, or when the image cannot be made.
    pub fn backdrop(&mut self, key: u64, area: crate::tree::Rect, opacity: f32, inputs: &Inputs) -> Option<core_video::pixel_buffer::CVPixelBuffer> {
        let ((w, h), px) = self.metrics();
        let backdrops = self.backdrops.as_mut()?;
        backdrops.render(&self.queue, key, area, (w, h), px, opacity, inputs)
    }

    /// Lets go of the backdrops of menus no longer open.
    pub fn keep_backdrops(&mut self, open: &[u64]) {
        if let Some(b) = self.backdrops.as_mut() {
            b.images.retain(|key, _| open.contains(key));
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
    fn a_menu_s_backdrop_renders_frame_after_frame_into_the_same_image() {
        let Some(device) = Device::system_default() else { return };
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "watercolor").unwrap().wash;
        let mut backdrops = Backdrops::new(&device, crate::effects::PREAMBLE, wash).unwrap();
        let queue = device.new_command_queue();
        let theme = crate::theme::builtin("peach-light").unwrap();
        let area = crate::tree::Rect { x: 100.0, y: 80.0, w: 200.0, h: 120.0 };
        let menu = crate::effects::blocks::MenuShape { key: 0x11, rect: area, opacity: 1.0 };
        let inputs = crate::effects::inputs::build(&theme, 1.0, (800.0, 600.0), 1.0, 0.0, &crate::effects::blocks::backdrop_blocks(&menu, &[]));
        // Each frame draws into the menu's own image again; the texture borrowed from it must
        // still be alive on the second and later frames.
        let first = backdrops.render(&queue, 0x11, area, (800.0, 600.0), 1.0, 1.0, &inputs).unwrap();
        for _ in 0..3 {
            let again = backdrops.render(&queue, 0x11, area, (800.0, 600.0), 1.0, 0.5, &inputs).unwrap();
            assert_eq!((again.get_width(), again.get_height()), (200, 120));
        }
        assert_eq!((first.get_width(), first.get_height()), (200, 120));
        // A menu that grows gets a new image of its new size.
        let wider = crate::tree::Rect { w: 260.0, ..area };
        let grown = backdrops.render(&queue, 0x11, wider, (800.0, 600.0), 1.0, 1.0, &inputs).unwrap();
        assert_eq!(grown.get_width(), 260);
    }

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
