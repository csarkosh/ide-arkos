//! `arkos effect bench` (spec 4.7 and 4.8): the wash rendered off-screen into a texture with the
//! window's own pipeline, for some seconds, and the GPU's time per frame.

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

/// How long the bench draws before it counts: the first frames build the pipeline's state and
/// fill the caches, and the GPU comes up to its working clock.
const WARM_UP: Duration = Duration::from_secs(1);

/// The budget's bands, in GPU milliseconds per frame.
const WITHIN: f32 = 4.0;
const HEAVY: f32 = 8.0;

/// The GPU's mean milliseconds per frame for `effect` drawn over `theme` in a window of `size`
/// points on a Retina screen: the wash rendered into an off-screen texture with the pipeline the
/// window's layer uses, at the manifest's `scale`, back to back for `seconds` after a second's
/// warm-up (`measure`). Every frame waits for the GPU, so each is timed alone.
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
    let scale = (BACKING * m.scale as f64) as f32;
    measure(WARM_UP, Duration::from_secs_f32(seconds), |i| {
        // The shader's clock runs as at the manifest's rate, whatever pace the frames come at.
        let inputs = inputs::build(theme, 1.0, size, scale, i as f32 / rate, &blocks);
        objc2::rc::autoreleasepool(|_| {
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
        })
    })
}

/// Draws frames back to back with `draw` (handed each frame's number, from 0) for `warm_up`
/// and then for `seconds`, and gives the mean of the GPU milliseconds `draw` returned for the
/// frames begun after the warm-up. Back to back, because a GPU left idle between frames clocks
/// down and the same work then reads several times longer.
fn measure(warm_up: Duration, seconds: Duration, mut draw: impl FnMut(u32) -> Result<f32, String>) -> Result<f32, String> {
    let start = Instant::now();
    let (mut total, mut counted) = (0.0, 0u32);
    for i in 0.. {
        let at = start.elapsed();
        if at >= warm_up + seconds && counted > 0 {
            break;
        }
        let ms = draw(i)?;
        if at >= warm_up {
            total += ms;
            counted += 1;
        }
    }
    Ok(total / counted as f32)
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
    let scene = active::scene(content, Some(top + content.h * 0.6), &panes, &[], &[], &strokes);
    blocks::blocks(&scene, effect.manifest.ink_borders)
}

/// One frame of `wash` over `theme` with `blocks` in a window of `size` points, one pixel a point,
/// read back as colours: what the wash paints, for the tests to measure.
#[cfg(test)]
pub fn render_frame(wash: &str, theme: &Theme, size: (u32, u32), time: f32, blocks: &[Block]) -> Vec<crate::palette::Rgb> {
    let device = Device::system_default().expect("a Metal device");
    let pipeline = layer::pipeline(&device, PREAMBLE, wash).unwrap();
    let desc = TextureDescriptor::new();
    desc.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    desc.set_width(size.0 as u64);
    desc.set_height(size.1 as u64);
    desc.set_usage(MTLTextureUsage::RenderTarget);
    desc.set_storage_mode(MTLStorageMode::Shared);
    let target = device.new_texture(&desc);
    let queue = device.new_command_queue();
    let inputs = inputs::build(theme, 1.0, (size.0 as f32, size.1 as f32), 1.0, time, blocks);
    let pass = RenderPassDescriptor::new();
    let att = pass.color_attachments().object_at(0).unwrap();
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
    let mut bytes = vec![0u8; (size.0 * size.1 * 4) as usize];
    target.get_bytes(bytes.as_mut_ptr() as *mut std::ffi::c_void, (size.0 * 4) as u64, metal::MTLRegion::new_2d(0, 0, size.0 as u64, size.1 as u64), 0);
    bytes.chunks(4).map(|p| crate::palette::Rgb { r: p[2], g: p[1], b: p[0] }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::palette::{contrast, Rgb};

    const W: u32 = 240;
    const H: u32 = 160;

    /// A small window: the title bar, two panes (tab bar and body each) and a selected and
    /// hovered row in the first, with no edges, so every pixel is wash a text sits on.
    fn small_window() -> Vec<Block> {
        let b = |x: f32, y: f32, w: f32, h: f32, kind: Kind, ordinal: f32| Block { rect: Rect { x, y, w, h }, kind, edges: 0, seed: x + y, value: 1.0, ordinal, busy: 0.0 };
        vec![
            b(0.0, 0.0, 240.0, 30.0, Kind::TitleBar, 0.0),
            b(0.0, 30.0, 120.0, 24.0, Kind::TabBar, 0.0),
            b(0.0, 54.0, 120.0, 106.0, Kind::PaneBody, 0.0),
            b(120.0, 30.0, 120.0, 24.0, Kind::TabBar, 1.0),
            b(120.0, 54.0, 120.0, 106.0, Kind::PaneBody, 1.0),
            b(10.0, 80.0, 100.0, 20.0, Kind::RowHighlight, HighlightState::Both as u32 as f32),
        ]
    }

    fn watercolor() -> &'static str {
        super::super::BUILTIN.iter().find(|b| b.name == "watercolor").unwrap().wash
    }

    /// The mean colour of the frame's pixels inside the rectangle, as floats 0 to 1.
    fn mean(frame: &[Rgb], x0: u32, y0: u32, x1: u32, y1: u32) -> [f32; 3] {
        let mut sum = [0.0f32; 3];
        let mut n = 0.0;
        for y in y0..y1 {
            for x in x0..x1 {
                let p = frame[(y * W + x) as usize];
                sum[0] += p.r as f32 / 255.0;
                sum[1] += p.g as f32 / 255.0;
                sum[2] += p.b as f32 / 255.0;
                n += 1.0;
            }
        }
        sum.map(|v| v / n)
    }

    /// The frame's pixels inside the rectangle, row by row.
    fn pixels(frame: &[Rgb], x0: u32, y0: u32, x1: u32, y1: u32) -> Vec<Rgb> {
        (y0..y1).flat_map(|y| (x0..x1).map(move |x| frame[(y * W + x) as usize])).collect()
    }

    fn apart(a: [f32; 3], b: [f32; 3]) -> f32 {
        (0..3).map(|i| (a[i] - b[i]).abs()).fold(0.0, f32::max)
    }

    /// Themes a user might load: a dark one with loud accents, and a light one whose focus
    /// colour is a deep, saturated blue that a careless wash would darken the paper with.
    fn loaded_themes() -> Vec<Theme> {
        let mut loud = crate::theme::builtin("navy").unwrap();
        loud.name = "loud-dark".into();
        loud.chrome.window_bg = Rgb { r: 16, g: 16, b: 20 };
        loud.chrome.focus_ring = Rgb { r: 255, g: 40, b: 200 };
        loud.terminal.magenta = Rgb { r: 255, g: 0, b: 255 };
        loud.terminal.red = Rgb { r: 255, g: 30, b: 60 };
        let mut deep = crate::theme::builtin("peach-light").unwrap();
        deep.name = "deep-light".into();
        deep.chrome.window_bg = Rgb { r: 250, g: 250, b: 252 };
        deep.chrome.focus_ring = Rgb { r: 10, g: 20, b: 140 };
        deep.terminal.blue = Rgb { r: 0, g: 0, b: 120 };
        deep.terminal.magenta = Rgb { r: 60, g: 0, b: 120 };
        vec![loud, deep]
    }

    #[test]
    fn watercolor_keeps_the_ui_text_readable_over_its_wash_on_any_theme() {
        if Device::system_default().is_none() {
            return;
        }
        let mut themes: Vec<Theme> = ["peach-light", "lavender-light", "navy", "timo-purple"].iter().map(|n| crate::theme::builtin(n).unwrap()).collect();
        themes.extend(loaded_themes());
        for theme in themes {
            let text = theme.chrome.text;
            // 3:1, WCAG's floor for large text and controls (Peach Light's approved title bar sits
            // near 4:1, so a stricter floor would wash its look out); the theme's own contrast
            // when that is less, since no wash can do better than the bare paper.
            let floor = contrast(text, theme.chrome.window_bg).min(3.0) - 0.1;
            for time in [0.0, 37.0] {
                let frame = render_frame(watercolor(), &theme, (W, H), time, &small_window());
                let worst = frame.iter().map(|p| contrast(text, *p)).fold(f64::MAX, f64::min);
                assert!(worst >= floor, "{}: text at {worst:.2}:1 over the wash (floor {floor:.2})", theme.name);
            }
        }
    }

    #[test]
    fn on_a_dark_theme_the_panes_wash_shows_and_each_pane_has_its_own() {
        if Device::system_default().is_none() {
            return;
        }
        for theme in [crate::theme::builtin("navy").unwrap(), loaded_themes().remove(0)] {
            let frame = render_frame(watercolor(), &theme, (W, H), 0.0, &small_window());
            let bg = theme.chrome.window_bg;
            let paper = [bg.r as f32 / 255.0, bg.g as f32 / 255.0, bg.b as f32 / 255.0];
            // The two panes' tab bars, away from the bubbles' and the highlight's places.
            let (left, right) = (mean(&frame, 4, 34, 116, 50), mean(&frame, 124, 34, 236, 50));
            assert!(apart(left, paper) > 0.03, "{}: the wash does not show on the dark paper: {left:?} vs {paper:?}", theme.name);
            assert!(apart(left, right) > 0.015, "{}: two panes washed alike: {left:?} vs {right:?}", theme.name);
        }
    }

    #[test]
    fn a_theme_with_a_family_of_one_still_gives_each_pane_its_own_tint() {
        if Device::system_default().is_none() {
            return;
        }
        let mut lonely = crate::theme::builtin("peach-light").unwrap();
        lonely.semantic.waiting = Rgb { r: 0, g: 140, b: 60 };
        lonely.terminal.red = Rgb { r: 30, g: 90, b: 200 };
        lonely.terminal.magenta = Rgb { r: 30, g: 90, b: 200 };
        lonely.terminal.yellow = Rgb { r: 30, g: 90, b: 200 };
        lonely.terminal.blue = Rgb { r: 30, g: 90, b: 200 };
        lonely.terminal.cyan = Rgb { r: 0, g: 150, b: 160 };
        lonely.terminal.green = Rgb { r: 0, g: 140, b: 60 };
        assert_eq!(inputs::pigments(&lonely).1, 1);
        let frame = render_frame(watercolor(), &lonely, (W, H), 0.0, &small_window());
        let (left, right) = (mean(&frame, 4, 34, 116, 50), mean(&frame, 124, 34, 236, 50));
        assert!(apart(left, right) > 0.01, "two panes washed alike: {left:?} vs {right:?}");
    }

    #[test]
    fn the_matrix_rain_is_faint_under_the_text() {
        if Device::system_default().is_none() {
            return;
        }
        let matrix = super::super::manifest::load("matrix", std::path::Path::new("/nonexistent")).unwrap();
        let theme = crate::theme::builtin("matrix").unwrap();
        // Inside a pane body, away from the edges' glow: the wash lays its rain twice there, at
        // RAIN_ALPHA and again at PANE_RAIN times it, so that it shows through a terminal's paper.
        // Through the paper (paper_alpha), what is seen of it is never more than RAIN_ALPHA of
        // the text green over the black ground, and it does show. The brightest rune is a head,
        // which leans RAIN_HEAD of the way toward white, with 2% for the bytes' rounding. The left
        // pane's body below its highlighted row and that row's soft spill (6 points out), over a
        // few moments, since about three fifths of the columns carry a stream at a time.
        let interior: Vec<Rgb> = [7.0, 23.0, 61.0]
            .iter()
            .flat_map(|&t| pixels(&render_frame(&matrix.wash, &theme, (W, H), t, &small_window()), 4, 108, 116, 156))
            .collect();
        let text = theme.terminal.foreground.g as f32 / 255.0;
        let head = text + (1.0 - text) * crate::effects::matrix::RAIN_HEAD;
        let max = interior.iter().map(|p| p.g as f32 / 255.0).fold(0.0, f32::max);
        let (rain, pane) = (crate::effects::matrix::RAIN_ALPHA, crate::effects::matrix::PANE_RAIN);
        let laid = 1.0 - (1.0 - rain) * (1.0 - rain * pane);
        assert!(max <= head * laid * 1.02, "the rain is too bright: green channel {max:.3}");
        let seen = max * (1.0 - matrix.manifest.paper_alpha);
        assert!(seen <= head * rain * 1.02, "the rain seen through the paper is too bright: green channel {seen:.3}");
        assert!(interior.iter().any(|p| p.g > 4), "the rain does not show at all");
    }

    #[test]
    fn a_busy_pane_s_edges_burn_brighter_than_an_idle_one_s() {
        if Device::system_default().is_none() {
            return;
        }
        let matrix = super::super::manifest::load("matrix", std::path::Path::new("/nonexistent")).unwrap();
        let theme = crate::theme::builtin("matrix").unwrap();
        let all = blocks::EDGE_TOP | blocks::EDGE_RIGHT | blocks::EDGE_BOTTOM | blocks::EDGE_LEFT;
        // Two panes apart, each inked on all four sides: their outlines' bands never meet.
        let pane = |x: f32, seed: f32, busy: f32, edges: u32| Block { rect: Rect { x, y: 16.0, w: 88.0, h: 128.0 }, kind: Kind::PaneBody, edges, seed, value: 1.0, ordinal: 0.0, busy };
        // The light an outline adds, in green summed over the band within 8 points of it: the
        // frame with the edges less the same frame without, so the rain under them cancels.
        let band = |with: &[Rgb], without: &[Rgb], x: u32| -> f32 {
            let (x0, x1, y0, y1) = (x - 8, x + 88 + 8, 8u32, 152u32);
            let mut sum = 0.0;
            for y in y0..y1 {
                for xx in x0..x1 {
                    let inner = xx >= x + 8 && xx < x + 80 && y >= 24 && y < 136;
                    if !inner {
                        let i = (y * W + xx) as usize;
                        sum += with[i].g as f32 - without[i].g as f32;
                    }
                }
            }
            sum
        };
        // Either pane busy in turn, so neither place nor grain decides it; over a few moments,
        // since a stream rides each vertical edge only now and then.
        for busy_left in [true, false] {
            let (bl, br) = if busy_left { (1.0, 0.0) } else { (0.0, 1.0) };
            let (mut left, mut right) = (0.0, 0.0);
            for t in [1.3, 4.7, 9.1, 15.5, 22.2, 31.9, 40.4, 57.0] {
                let lit = [pane(16.0, 20.0, bl, all), pane(136.0, 22.0, br, all)];
                let dark = [pane(16.0, 20.0, bl, 0), pane(136.0, 22.0, br, 0)];
                let with = render_frame(&matrix.wash, &theme, (W, H), t, &lit);
                let without = render_frame(&matrix.wash, &theme, (W, H), t, &dark);
                left += band(&with, &without, 16);
                right += band(&with, &without, 136);
            }
            assert!(left > 0.0 && right > 0.0, "the edges draw nothing: {left} {right}");
            let (busy, idle) = if busy_left { (left, right) } else { (right, left) };
            assert!(busy > idle * 1.15, "busy {busy:.0} is not brighter than idle {idle:.0} (busy on the left: {busy_left})");
        }
    }

    #[test]
    fn the_matrix_wash_draws_no_edge_on_the_window_s_own_border() {
        if Device::system_default().is_none() {
            return;
        }
        let matrix = super::super::manifest::load("matrix", std::path::Path::new("/nonexistent")).unwrap();
        let theme = crate::theme::builtin("matrix").unwrap();
        let all = blocks::EDGE_TOP | blocks::EDGE_RIGHT | blocks::EDGE_BOTTOM | blocks::EDGE_LEFT;
        // A pane whose right and bottom edges are the window's right and bottom.
        let pane = |edges: u32| [Block { rect: Rect { x: 16.0, y: 16.0, w: W as f32 - 16.0, h: H as f32 - 16.0 }, kind: Kind::PaneBody, edges, seed: 20.0, value: 1.0, ordinal: 0.0, busy: 1.0 }];
        let added = |with: &[Rgb], without: &[Rgb], x0: u32, x1: u32, y0: u32, y1: u32| -> i32 {
            pixels(with, x0, y0, x1, y1).iter().zip(pixels(without, x0, y0, x1, y1)).map(|(a, b)| (a.g as i32 - b.g as i32).abs()).sum()
        };
        for t in [1.3, 9.1, 22.2, 40.4] {
            let with = render_frame(&matrix.wash, &theme, (W, H), t, &pane(all));
            let without = render_frame(&matrix.wash, &theme, (W, H), t, &pane(0));
            // Along the right and bottom borders, away from the corners the left and top edges
            // light: nothing.
            assert_eq!(added(&with, &without, W - 4, W, 30, H - 4), 0, "an edge on the right border at {t}");
            assert_eq!(added(&with, &without, 30, W - 4, H - 4, H), 0, "an edge on the bottom border at {t}");
            // The left edge, inside the window, is lit.
            assert!(added(&with, &without, 12, 21, 30, H - 4) > 0, "the left edge draws nothing at {t}");
        }
    }

    // Run by hand (`cargo test -p arkos effects::bench -- --ignored`): the GPU time is only
    // meaningful on an idle machine, not inside the parallel suite.
    #[test]
    #[ignore]
    fn the_matrix_wash_draws_within_budget() {
        if Device::system_default().is_none() {
            return;
        }
        let matrix = super::super::manifest::load("matrix", std::path::Path::new("/nonexistent")).unwrap();
        let theme = crate::theme::builtin("matrix").unwrap();
        let ms = bench(&matrix, &theme, (1200.0, 800.0), 1.0).unwrap();
        // The figure, for the hand check's note (`-- --ignored --nocapture`).
        eprintln!("matrix wash: {ms:.3} ms per frame at 1200x800, scale {}", matrix.manifest.scale);
        assert_eq!(verdict(ms), "within budget", "{ms} ms");
    }

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
    fn the_frames_of_the_warm_up_are_drawn_but_not_counted() {
        let start = Instant::now();
        let mut drawn = Vec::new();
        // A frame drawn while the GPU warms up costs far more than one after.
        let ms = measure(Duration::from_millis(20), Duration::from_millis(30), |i| {
            drawn.push(i);
            std::thread::sleep(Duration::from_millis(1));
            Ok(if start.elapsed() < Duration::from_millis(20) { 100.0 } else { 2.0 })
        })
        .unwrap();
        assert_eq!(ms, 2.0, "only the frames after the warm-up count");
        assert!(drawn.len() > 20, "frames come back to back, not at a frame rate: {}", drawn.len());
        assert!(drawn.windows(2).all(|w| w[1] == w[0] + 1), "each frame its own number, for the clock");
        assert!(start.elapsed() < Duration::from_millis(200));
        assert_eq!(measure(Duration::ZERO, Duration::from_millis(5), |_| Err("refused".into())), Err("refused".to_string()));
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
