//! Paints a `Terminal`'s visible rows at a fractional line offset (spec 3). Only the rows in the
//! viewport plus one above are shaped and painted, which is the virtualization.
use crate::layout::{build_runs, CellKind, CellStyle};
use crate::palette::Rgb;
use crate::terminal::{CellMetrics, Terminal};
use gpui::{
    fill, font, point, px, size, App, Bounds, ContentMask, Element, ElementId, Entity, FontWeight, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, LayoutId, Pixels, Rgba, SharedString, Style, TextRun, Window,
};
use std::time::Instant;

pub const DEFAULT_FONT_SIZE: f32 = 16.0;
pub const DEFAULT_LINE_HEIGHT: f32 = 1.5;

/// The height of one terminal row.
pub fn cell_height(font_size: f32, line_height: f32) -> f32 {
    font_size * line_height
}

pub struct TerminalElement {
    terminal: Entity<Terminal>,
    font_size: f32,
    line_height: f32,
    family: SharedString,
}

impl TerminalElement {
    pub fn new(terminal: Entity<Terminal>, font_size: f32, line_height: f32, family: SharedString) -> Self {
        Self { terminal, font_size, line_height, family }
    }
}

fn hsla(c: Rgb) -> Hsla {
    Rgba { r: c.r as f32 / 255.0, g: c.g as f32 / 255.0, b: c.b as f32 / 255.0, a: 1.0 }.into()
}

fn text_run(text: &str, style: &CellStyle, family: SharedString) -> TextRun {
    let mut f = font(family);
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
fn measure_cell(window: &mut Window, font_size: f32, line_height: f32, family: SharedString) -> CellMetrics {
    let run = TextRun { len: 1, font: font(family), color: gpui::white(), background_color: None, underline: None, strikethrough: None };
    let layout = window.text_system().layout_line("W", px(font_size), &[run], None);
    CellMetrics { width: f32::from(layout.width), height: cell_height(font_size, line_height) }
}

/// The cell grid that fits `bounds` at `cell`, as (cols, rows).
fn grid_in(bounds: Bounds<Pixels>, cell: CellMetrics) -> (usize, usize) {
    let cols = (f32::from(bounds.size.width) / cell.width).floor().max(2.0) as usize;
    let rows = (f32::from(bounds.size.height) / cell.height).floor().max(1.0) as usize;
    (cols, rows)
}

/// The grid a terminal element in `bounds` would size itself to. The window measures with this
/// before a terminal exists, so a replay can be parsed at the size it will be shown at.
pub fn grid_for(bounds: Bounds<Pixels>, window: &mut Window, font_size: f32, line_height: f32, family: &str) -> (usize, usize) {
    grid_in(bounds, measure_cell(window, font_size, line_height, family.to_string().into()))
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
        let cell = measure_cell(window, self.font_size, self.line_height, self.family.clone());
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
        // Advance the scroll animation by wall-clock time, then split into integer offset + fraction.
        // The colours come from the terminal's palette, which follows the theme.
        let (rows, frac, cursor, animating, bg, cursor_color) = self.terminal.update(cx, |t, _| {
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
            let palette = t.core.palette();
            (t.core.rows_for_paint(), frac, t.core.cursor(), animating || sync_open, palette.bg, palette.cursor)
        });
        window.paint_quad(fill(bounds, hsla(bg)));

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            let origin = bounds.origin;
            let paint_row = |cells: &Vec<(char, CellStyle, CellKind)>, y: Pixels, window: &mut Window, cx: &mut App| {
                let runs = build_runs(cells.iter().copied());
                for run in &runs {
                    let x = origin.x + px(run.col as f32 * cw);
                    let w = px(if run.wide { 2.0 * cw } else { run.text.chars().count() as f32 * cw });
                    if run.style.bg != bg {
                        window.paint_quad(fill(Bounds { origin: point(x, y), size: size(w, lh) }, hsla(run.style.bg)));
                    }
                    if run.text.trim().is_empty() {
                        continue;
                    }
                    let text: SharedString = run.text.clone().into();
                    let line = window.text_system().shape_line(text, px(self.font_size), &[text_run(&run.text, &run.style, self.family.clone())], None);
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
                    let mut c = hsla(cursor_color);
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

    #[test]
    fn cell_height_follows_the_preset() {
        use super::cell_height;
        // f32 products of the decimal line heights are not exact.
        for (size, lh, want) in [(16.0, 1.5, 24.0), (14.0, 1.3, 18.2), (18.0, 1.6, 28.8)] {
            assert!((cell_height(size, lh) - want).abs() < 1e-4, "{size} x {lh}");
        }
    }

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
