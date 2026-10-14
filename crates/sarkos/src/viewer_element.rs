//! Paints a `Viewer`'s visible lines (spec 6: virtualized, line numbers in a gutter) at a
//! fractional line offset, like the terminal element. Only the lines in the viewport are shaped.
use crate::palette::Rgb;
use crate::viewer::{Content, Hit, Span, Viewer};
use gpui::{
    fill, font, point, px, size, App, Bounds, ContentMask, Element, ElementId, Entity, FontWeight, GlobalElementId, Hsla, InspectorElementId,
    IntoElement, LayoutId, Pixels, Rgba, SharedString, Style, TextRun, Window,
};
use std::ops::Range;
use std::time::Instant;

pub const GUTTER_PAD: f32 = 8.0;
const LEFT_PAD: f32 = 8.0;
/// The most of one line a frame shapes and paints; the rest is past any screen's right edge.
pub const MAX_PAINT_BYTES: usize = 16 * 1024;

/// The part of `line` to paint: at most `MAX_PAINT_BYTES`, cut back to a char boundary, and
/// whether anything was cut.
pub fn paint_cut(line: &str) -> (&str, bool) {
    if line.len() <= MAX_PAINT_BYTES {
        return (line, false);
    }
    (&line[..line.floor_char_boundary(MAX_PAINT_BYTES)], true)
}

pub fn gutter_width(line_count: usize, cell_w: f32) -> f32 {
    let digits = line_count.max(1).to_string().len() as f32;
    digits * cell_w + GUTTER_PAD * 2.0
}

/// The lines to paint: from the scrolled line, `rows` of them plus one for the fraction,
/// clamped to the file.
pub fn visible_range(scroll: f32, rows: usize, len: usize) -> Range<usize> {
    let first = (scroll.floor().max(0.0) as usize).min(len);
    let last = (first + rows + 1).min(len);
    first..last
}

fn hsla(c: Rgb) -> Hsla {
    Rgba { r: c.r as f32 / 255.0, g: c.g as f32 / 255.0, b: c.b as f32 / 255.0, a: 1.0 }.into()
}

pub struct ViewerElement {
    viewer: Entity<Viewer>,
    family: SharedString,
    bg: Rgb,
    gutter_fg: Rgb,
    text_fg: Rgb,
    find_match: Rgb,
    find_current: Rgb,
}

impl ViewerElement {
    pub fn new(viewer: Entity<Viewer>, family: SharedString, bg: Rgb, gutter_fg: Rgb, text_fg: Rgb, find_match: Rgb, find_current: Rgb) -> Self {
        Self { viewer, family, bg, gutter_fg, text_fg, find_match, find_current }
    }

    fn run(&self, len: usize, color: Rgb, bold: bool, italic: bool) -> TextRun {
        let mut f = font(self.family.clone());
        if bold {
            f.weight = FontWeight::BOLD;
        }
        if italic {
            f.style = gpui::FontStyle::Italic;
        }
        TextRun { len, font: f, color: hsla(color), background_color: None, underline: None, strikethrough: None }
    }
}

/// What one frame paints, copied out of the viewer: only the visible lines and their hits.
struct Frame {
    first: usize,
    line_count: usize,
    lines: Vec<String>,
    spans: Vec<Vec<Span>>,
    frac: f32,
    animating: bool,
    hits: Vec<Hit>,
    current: Option<Hit>,
    font_size: f32,
}

pub struct Prepaint {
    cell_w: f32,
    line_h: f32,
}

impl Element for ViewerElement {
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
        let (font_size, line_height) = {
            let v = self.viewer.read(cx);
            (v.font_size, v.line_height)
        };
        let run = TextRun { len: 1, font: font(self.family.clone()), color: gpui::white(), background_color: None, underline: None, strikethrough: None };
        let cell_w = f32::from(window.text_system().layout_line("W", px(font_size), &[run], None).width);
        let line_h = font_size * line_height;
        let rows = (f32::from(bounds.size.height) / line_h).floor().max(1.0) as usize;
        self.viewer.update(cx, |v, _| v.rows_visible = rows);
        Prepaint { cell_w, line_h }
    }

    fn paint(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _state: &mut (), prepaint: &mut Prepaint, window: &mut Window, cx: &mut App) {
        let (cell_w, line_h) = (prepaint.cell_w, prepaint.line_h);
        window.paint_quad(fill(bounds, hsla(self.bg)));
        let frame = self.viewer.update(cx, |v, _| {
            let now = Instant::now();
            let dt = v.last_frame.map(|l| now.duration_since(l).as_secs_f32().min(0.05)).unwrap_or(1.0 / 60.0);
            let animating = v.scroll.tick(dt);
            v.last_frame = if animating { Some(now) } else { None };
            let (lines, spans) = match &v.content {
                Content::Text { lines, spans } => (lines.as_slice(), spans.as_slice()),
                _ => (&[][..], &[][..]),
            };
            let range = visible_range(v.scroll.current, v.rows_visible, lines.len());
            // Hits are in reading order: the visible ones are one run of them.
            let (hits, current) = match &v.find {
                Some(f) => {
                    let lo = f.hits.partition_point(|h| h.line < range.start);
                    let hi = f.hits.partition_point(|h| h.line < range.end);
                    (f.hits[lo..hi].to_vec(), f.hits.get(f.current).copied())
                }
                None => (Vec::new(), None),
            };
            Frame {
                first: range.start,
                line_count: lines.len(),
                lines: lines[range.clone()].to_vec(),
                spans: spans.get(range).map(<[Vec<Span>]>::to_vec).unwrap_or_default(),
                frac: v.scroll.current - v.scroll.current.floor(),
                animating,
                hits,
                current,
                font_size: v.font_size,
            }
        });
        if frame.animating {
            window.request_animation_frame();
        }
        let font_size = px(frame.font_size);
        let gutter = gutter_width(frame.line_count, cell_w);
        let origin = bounds.origin;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for (i, line) in frame.lines.iter().enumerate() {
                let n = frame.first + i;
                let y = origin.y + px((i as f32 - frame.frac) * line_h);
                // The line number, right-aligned in the gutter.
                let num: SharedString = format!("{}", n + 1).into();
                let num_w = num.len() as f32 * cell_w;
                let num_run = self.run(num.len(), self.gutter_fg, false, false);
                let shaped = window.text_system().shape_line(num, font_size, &[num_run], None);
                let _ = shaped.paint(point(origin.x + px(gutter - GUTTER_PAD - num_w), y), px(line_h), window, cx);
                if line.is_empty() {
                    continue;
                }
                let (shown, cut) = paint_cut(line);
                // The spans cover the line; anything else (a plain file, a cut line) is one run.
                let spans = frame.spans.get(i).filter(|s| s.last().map(|l| l.end) == Some(shown.len()));
                let runs: Vec<TextRun> = match spans {
                    Some(spans) => spans.iter().map(|s| self.run(s.end - s.start, s.color, s.bold, s.italic)).collect(),
                    None => vec![self.run(shown.len(), self.text_fg, false, false)],
                };
                let x = origin.x + px(gutter + LEFT_PAD);
                let shaped = window.text_system().shape_line(shown.to_string().into(), font_size, &runs, None);
                // Find hits on this line: quads under the text, measured on the shaped line.
                for h in frame.hits.iter().filter(|h| h.line == n && h.start < shown.len()) {
                    let (x0, x1) = (shaped.x_for_index(h.start), shaped.x_for_index(h.end.min(shown.len())));
                    let mut col = hsla(if frame.current == Some(*h) { self.find_current } else { self.find_match });
                    col.a = 0.45;
                    window.paint_quad(fill(Bounds { origin: point(x + x0, y), size: size(x1 - x0, px(line_h)) }, col));
                }
                let _ = shaped.paint(point(x, y), px(line_h), window, cx);
                if cut {
                    // A dim marker where the painted part ends.
                    let more = window.text_system().shape_line("…".into(), font_size, &[self.run("…".len(), self.gutter_fg, false, false)], None);
                    let _ = more.paint(point(x + shaped.width, y), px(line_h), window, cx);
                }
            }
        });
    }
}

impl IntoElement for ViewerElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gutter_fits_the_widest_line_number_plus_padding() {
        assert_eq!(gutter_width(9, 10.0), 1.0 * 10.0 + GUTTER_PAD * 2.0);
        assert_eq!(gutter_width(10, 10.0), 2.0 * 10.0 + GUTTER_PAD * 2.0);
        assert_eq!(gutter_width(12345, 8.0), 5.0 * 8.0 + GUTTER_PAD * 2.0);
        assert_eq!(gutter_width(0, 8.0), 1.0 * 8.0 + GUTTER_PAD * 2.0, "an empty file still has a gutter");
    }

    #[test]
    fn a_long_line_is_cut_for_painting_on_a_char_boundary() {
        assert_eq!(paint_cut("short"), ("short", false));
        assert_eq!(paint_cut(""), ("", false));
        let exact = "a".repeat(MAX_PAINT_BYTES);
        assert_eq!(paint_cut(&exact), (exact.as_str(), false), "at the limit nothing is cut");
        let long = "a".repeat(MAX_PAINT_BYTES * 3);
        assert_eq!(paint_cut(&long), (&long[..MAX_PAINT_BYTES], true));
        // Three-byte chars: 16384 is one byte past a multiple of three, inside a char.
        let wide = "\u{20ac}".repeat(MAX_PAINT_BYTES);
        assert!(!wide.is_char_boundary(MAX_PAINT_BYTES));
        let (shown, cut) = paint_cut(&wide);
        assert!(cut);
        assert_eq!(shown.len(), MAX_PAINT_BYTES - 1, "back to the start of the char the limit splits");
        assert!(shown.ends_with('\u{20ac}'));
    }

    #[test]
    fn the_visible_range_is_the_rows_from_the_scroll_line_plus_one_for_the_fraction() {
        assert_eq!(visible_range(0.0, 10, 100), 0..11);
        assert_eq!(visible_range(95.5, 10, 100), 95..100, "clamped at the end");
        assert_eq!(visible_range(0.0, 10, 3), 0..3);
        assert_eq!(visible_range(0.0, 10, 0), 0..0);
    }
}
