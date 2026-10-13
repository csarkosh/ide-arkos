//! Paints a `Terminal`'s visible rows at a fractional line offset (spec 3). Only the rows in the
//! viewport plus one above are shaped and painted, which is the virtualization.
use crate::layout::{build_runs, CellKind, CellStyle};
use crate::palette::{Rgb, NAVY};
use crate::terminal::{CellMetrics, Terminal};
use gpui::{
    fill, font, point, px, size, App, Bounds, ContentMask, Element, ElementId, Entity, FontWeight, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, LayoutId, Pixels, Rgba, SharedString, Style, TextRun, Window,
};
use std::time::Instant;

pub const FONT_FAMILY: &str = "Menlo";
pub const FONT_SIZE: f32 = 16.0;
pub const LINE_HEIGHT: f32 = 1.5;

pub struct TerminalElement {
    terminal: Entity<Terminal>,
}

impl TerminalElement {
    pub fn new(terminal: Entity<Terminal>) -> Self {
        Self { terminal }
    }
}

fn hsla(c: Rgb) -> Hsla {
    Rgba { r: c.r as f32 / 255.0, g: c.g as f32 / 255.0, b: c.b as f32 / 255.0, a: 1.0 }.into()
}

fn text_run(text: &str, style: &CellStyle) -> TextRun {
    let mut f = font(FONT_FAMILY);
    if style.bold {
        f.weight = FontWeight::BOLD;
    }
    if style.italic {
        f.style = gpui::FontStyle::Italic;
    }
    let mut color = hsla(style.fg);
    if style.dim {
        color.a = 0.6;
    }
    TextRun { len: text.len(), font: f, color, background_color: None, underline: None, strikethrough: None }
}

/// Measures the cell grid once per window scale. `W` is the widest ASCII glyph in a monospace font.
fn measure_cell(window: &mut Window) -> CellMetrics {
    let run = TextRun { len: 1, font: font(FONT_FAMILY), color: gpui::white(), background_color: None, underline: None, strikethrough: None };
    let layout = window.text_system().layout_line("W", px(FONT_SIZE), &[run], None);
    CellMetrics { width: f32::from(layout.width), height: FONT_SIZE * LINE_HEIGHT }
}

/// The cell grid that fits `bounds` at `cell`, as (cols, rows).
fn grid_in(bounds: Bounds<Pixels>, cell: CellMetrics) -> (usize, usize) {
    let cols = (f32::from(bounds.size.width) / cell.width).floor().max(2.0) as usize;
    let rows = (f32::from(bounds.size.height) / cell.height).floor().max(1.0) as usize;
    (cols, rows)
}

/// The grid a terminal element in `bounds` would size itself to. The window measures with this
/// before a terminal exists, so a replay can be parsed at the size it will be shown at.
pub fn grid_for(bounds: Bounds<Pixels>, window: &mut Window) -> (usize, usize) {
    grid_in(bounds, measure_cell(window))
}

/// Splits a scroll position into alacritty's integer display offset and the fraction of a line
/// drawn as a pixel offset. The animator can sit slightly below zero after a negative offset
/// delta, so it is clamped first; the fraction is always in [0, 1).
fn split_offset(current: f32) -> (usize, f32) {
    let cur = current.max(0.0);
    let off = cur.floor();
    (off as usize, (cur - off).clamp(0.0, 1.0 - f32::EPSILON))
}

/// Seconds since the last animated frame, or one nominal frame when animation is starting.
/// Capped so a stalled frame cannot leap the animation.
fn frame_dt(last: Option<Instant>, now: Instant) -> f32 {
    last.map(|l| now.duration_since(l).as_secs_f32()).unwrap_or(0.016).min(0.1)
}

impl IntoElement for TerminalElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

pub struct Prepaint {
    cell: CellMetrics,
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = gpui::relative(1.0).into();
        style.size.height = gpui::relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _state: &mut (), window: &mut Window, cx: &mut App) -> Prepaint {
        let cell = measure_cell(window);
        let (cols, rows) = grid_in(bounds, cell);
        self.terminal.update(cx, |t, _| {
            t.cell = Some(cell);
            t.ensure_size(cols, rows);
        });
        Prepaint { cell }
    }

    fn paint(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _state: &mut (), prepaint: &mut Prepaint, window: &mut Window, cx: &mut App) {
        let cell = prepaint.cell;
        let lh = px(cell.height);
        let cw = cell.width;
        window.paint_quad(fill(bounds, hsla(NAVY.bg)));

        // Advance the scroll animation by wall-clock time, then split into integer offset + fraction.
        let (rows, frac, cursor, animating) = self.terminal.update(cx, |t, _| {
            // vte only checks the synchronized update deadline when bytes arrive, so a program
            // killed inside one would freeze the grid. Releasing it here means this frame paints
            // the released rows; while one is still open, keep frames coming to check again.
            t.core.flush_sync_if_due();
            let sync_open = t.core.sync_pending();
            let now = Instant::now();
            let dt = frame_dt(t.last_frame, now);
            let animating = t.core.scroll.tick(dt);
            // Once settled, forget the frame time: the next scroll after idle starts from one
            // nominal frame instead of the capped gap, which would jump most of the distance.
            t.last_frame = if animating || sync_open { Some(now) } else { None };
            let (off, frac) = split_offset(t.core.scroll.current);
            t.core.set_display_offset(off);
            (t.core.rows_for_paint(), frac, t.core.cursor(), animating || sync_open)
        });

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            let origin = bounds.origin;
            let paint_row = |cells: &Vec<(char, CellStyle, CellKind)>, y: Pixels, window: &mut Window, cx: &mut App| {
                let runs = build_runs(cells.iter().copied());
                for run in &runs {
                    let x = origin.x + px(run.col as f32 * cw);
                    let w = px(if run.wide { 2.0 * cw } else { run.text.chars().count() as f32 * cw });
                    if run.style.bg != NAVY.bg {
                        window.paint_quad(fill(Bounds { origin: point(x, y), size: size(w, lh) }, hsla(run.style.bg)));
                    }
                    if run.text.trim().is_empty() {
                        continue;
                    }
                    let text: SharedString = run.text.clone().into();
                    let line = window.text_system().shape_line(text, px(FONT_SIZE), &[text_run(&run.text, &run.style)], None);
                    let _ = line.paint(point(x, y), lh, window, cx);
                    if run.style.underline {
                        let uy = y + lh - px(2.0);
                        window.paint_quad(fill(Bounds { origin: point(x, uy), size: size(w, px(1.0)) }, hsla(run.style.fg)));
                    }
                }
            };
            if let Some(above) = &rows.above {
                let y = origin.y + px((frac - 1.0) * cell.height);
                paint_row(above, y, window, cx);
            }
            for (i, row) in rows.visible.iter().enumerate() {
                let y = origin.y + px((i as f32 + frac) * cell.height);
                paint_row(row, y, window, cx);
            }
            if let Some((col, line)) = cursor {
                if frac == 0.0 {
                    let x = origin.x + px(col as f32 * cw);
                    let y = origin.y + px(line as f32 * cell.height);
                    let mut c = hsla(NAVY.cursor);
                    c.a = 0.85;
                    window.paint_quad(fill(Bounds { origin: point(x, y), size: size(px(cw), lh) }, c));
                }
            }
        });

        if animating {
            window.request_animation_frame();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{frame_dt, split_offset};
    use crate::scroll::ScrollAnimator;
    use std::time::{Duration, Instant};

    #[test]
    fn split_offset_clamps_negative_drift() {
        assert_eq!(split_offset(-0.003), (0, 0.0));
        assert_eq!(split_offset(0.0), (0, 0.0));
        let (off, frac) = split_offset(3.25);
        assert_eq!(off, 3);
        assert!((frac - 0.25).abs() < 1e-6);
    }

    #[test]
    fn first_frame_after_idle_does_not_snap() {
        let now = Instant::now();
        assert_eq!(frame_dt(None, now), 0.016);
        assert_eq!(frame_dt(Some(now - Duration::from_secs(5)), now), 0.1);
        // A wheel notch of three lines: starting from a nominal frame moves about a quarter of
        // the way, where the capped idle gap would have moved about 80%.
        let mut s = ScrollAnimator::new();
        s.scroll_by(3.0, 100.0);
        s.tick(frame_dt(None, now));
        assert!(s.current < 1.0, "first frame moved {} lines", s.current);
    }
}
