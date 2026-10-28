//! `arkos effect bench` (spec 4.7 and 4.8): the wash rendered off-screen into a texture with the
//! window's own pipeline, for some seconds at the manifest's rate, and the GPU's time per frame.

use std::time::{Duration, Instant};

use metal::{Device, MTLLoadAction, MTLPixelFormat, MTLPrimitiveType, MTLStorageMode, MTLStoreAction, MTLTextureUsage, RenderPassDescriptor, TextureDescriptor};

use super::blocks::{self, Block, HighlightState, Kind, Mark};
use super::manifest::{Effect, Highlights};
use super::{active, inputs, layer, PREAMBLE};
use crate::theme::Theme;
use crate::tree::Rect;

/// The screen the bench draws for: Retina, two pixels per point.
const BACKING: f64 = 2.0;
/// The sidebar's width in the window the bench lays out.
const SIDEBAR_W: f32 = 240.0;

/// The budget's bands, in GPU milliseconds per frame.
const WITHIN: f32 = 4.0;
const HEAVY: f32 = 8.0;

/// The GPU's mean milliseconds per frame for `effect` drawn over `theme` in a window of `size`
/// points on a Retina screen: the wash rendered into an off-screen texture with the pipeline the
/// window's layer uses, at the manifest's `scale` and `frame_rate`, for `seconds`. Every frame
/// waits for the GPU, so each is timed alone.
pub fn bench(effect: &Effect, theme: &Theme, size: (f32, f32), seconds: f32) -> Result<f32, String> {
    let device = Device::system_default().ok_or("no Metal device")?;
    let pipeline = layer::pipeline(&device, PREAMBLE, &effect.wash)?;
    let m = &effect.manifest;
    let (w, h) = layer::drawable_size((size.0 as f64, size.1 as f64), BACKING, m.scale as f64);
    if w < 1.0 || h < 1.0 {
        return Err(format!("{}x{} points draw nothing", size.0, size.1));
    }
    let desc = TextureDescriptor::new();
    desc.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    desc.set_width(w as u64);
    desc.set_height(h as u64);
    desc.set_usage(MTLTextureUsage::RenderTarget);
    desc.set_storage_mode(MTLStorageMode::Private);
    let target = device.new_texture(&desc);
    let queue = device.new_command_queue();
    let blocks = sample_blocks(size, effect);
    let rate = m.frame_rate.clamp(1, 60) as f32;
    let frames = (seconds * rate).ceil().max(1.0) as u32;
    let interval = Duration::from_secs_f32(1.0 / rate);
    let start = Instant::now();
    let mut total = 0.0;
    for i in 0..frames {
        let inputs = inputs::build(theme, 1.0, size, (BACKING * m.scale as f64) as f32, i as f32 / rate, &blocks);
        let ms = objc2::rc::autoreleasepool(|_| {
            let pass = RenderPassDescriptor::new();
            let att = pass.color_attachments().object_at(0).ok_or("no colour attachment")?;
            att.set_texture(Some(&target));
            att.set_load_action(MTLLoadAction::Clear);
            att.set_store_action(MTLStoreAction::Store);
            let cmd = queue.new_command_buffer();
            let enc = cmd.new_render_command_encoder(pass);
            enc.set_render_pipeline_state(&pipeline);
            enc.set_fragment_bytes(0, std::mem::size_of::<inputs::Inputs>() as u64, &inputs as *const inputs::Inputs as *const std::ffi::c_void);
            enc.draw_primitives(MTLPrimitiveType::Triangle, 0, 3);
            enc.end_encoding();
            cmd.commit();
            cmd.wait_until_completed();
            if cmd.status() == metal::MTLCommandBufferStatus::Error {
                return Err("the GPU refused a frame".to_string());
            }
            Ok(layer::gpu_ms(cmd))
        })?;
        total += ms;
        // Frames come at the manifest's rate, as the window's timer sends them.
        if let Some(wait) = (start + interval * (i + 1)).checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
    Ok(total / frames as f32)
}

/// The budget's verdict on `ms` per frame: under 4 within it, 4 to 8 heavy, above 8 too heavy.
pub fn verdict(ms: f32) -> &'static str {
    if ms < WITHIN {
        "within budget"
    } else if ms <= HEAVY {
        "heavy"
    } else {
        "too heavy"
    }
}

/// The blocks of a window of `size` points as it opens: the title bar, the sidebar with its
/// divider and one pane, and, when the effect paints the highlights, a selected Explorer row.
fn sample_blocks(size: (f32, f32), effect: &Effect) -> Vec<Block> {
    let top = crate::chrome::TITLE_BAR_H;
    let content = Rect { x: SIDEBAR_W, y: top, w: (size.0 - SIDEBAR_W).max(0.0), h: (size.1 - top).max(0.0) };
    let panes = [(1, content)];
    let mut strokes = Vec::new();
    if effect.manifest.highlights == Highlights::Effect {
        let row = Rect { x: 0.0, y: top + 60.0, w: SIDEBAR_W, h: 22.0 };
        strokes.push((Mark { key: 11, rect: row, kind: Kind::RowHighlight, state: HighlightState::Selected }, 1.0));
    }
    let scene = active::scene(content, Some(top + content.h * 0.6), &panes, &strokes);
    blocks::blocks(&scene, effect.manifest.ink_borders)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bench_draws_a_window_as_it_opens_with_a_selected_row_when_the_effect_paints_it() {
        let mut effect = super::super::manifest::load("watercolor", std::path::Path::new("/nonexistent/effects")).unwrap();
        let kinds = |e: &Effect| sample_blocks((1200.0, 800.0), e).iter().map(|b| b.kind).collect::<Vec<_>>();
        assert_eq!(kinds(&effect), vec![Kind::TitleBar, Kind::Sidebar, Kind::Divider, Kind::TabBar, Kind::PaneBody, Kind::RowHighlight]);
        effect.manifest.highlights = Highlights::App;
        assert_eq!(kinds(&effect), vec![Kind::TitleBar, Kind::Sidebar, Kind::Divider, Kind::TabBar, Kind::PaneBody]);
        let body = sample_blocks((1200.0, 800.0), &effect).into_iter().find(|b| b.kind == Kind::PaneBody).unwrap();
        assert_eq!(body.rect.x + body.rect.w, 1200.0, "the pane reaches the window's right edge");
    }

    #[test]
    fn the_verdict_bands_are_four_and_eight_milliseconds() {
        assert_eq!(verdict(0.6), "within budget");
        assert_eq!(verdict(3.99), "within budget");
        assert_eq!(verdict(4.0), "heavy");
        assert_eq!(verdict(8.0), "heavy");
        assert_eq!(verdict(8.01), "too heavy");
        assert_eq!(verdict(25.0), "too heavy");
    }
}
