//! Paints a `Viewer`'s visible lines (spec 6: virtualized, line numbers in a gutter) at a
//! fractional line offset, like the terminal element. Only the lines in the viewport are shaped.
//! A file with changes against HEAD gets a mark column at the gutter's left; with the inline
//! diff on, the deleted lines are rows of their own.
use crate::diff::{display_rows, DisplayRow, Mark};
use crate::palette::Rgb;
use crate::viewer::{Content, Hit, Span, Viewer};
use gpui::{
    fill, font, point, px, size, App, Bounds, ContentMask, Element, ElementId, Entity, FontWeight, GlobalElementId, Hsla, InspectorElementId,
    IntoElement, LayoutId, Pixels, Rgba, SharedString, Style, TextRun, Window,
};
use std::ops::Range;
use std::time::Instant;

pub const GUTTER_PAD: f32 = 8.0;
/// The mark column at the gutter's left, there only when the viewer has a diff.
pub const MARK_W: f32 = 6.0;
/// An added or changed line's bar, in the mark column.
const BAR_W: f32 = 3.0;
/// A deletion's mark, a bar across the mark column at the top of the line that follows.
const DELETED_H: f32 = 2.0;
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

/// The gutter with the mark column added.
pub fn gutter_width_with_marks(line_count: usize, cell_w: f32) -> f32 {
    gutter_width(line_count, cell_w) + MARK_W
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

/// The diff's colours, the theme's `tree.{added, modified, deleted}`.
pub struct MarkColors {
    pub added: Rgb,
    pub modified: Rgb,
    pub deleted: Rgb,
}

pub struct ViewerElement {
    viewer: Entity<Viewer>,
    family: SharedString,
    bg: Rgb,
    gutter_fg: Rgb,
    text_fg: Rgb,
    find_match: Rgb,
    find_current: Rgb,
    marks: MarkColors,
}

impl ViewerElement {
    pub fn new(viewer: Entity<Viewer>, family: SharedString, bg: Rgb, gutter_fg: Rgb, text_fg: Rgb, find_match: Rgb, find_current: Rgb, marks: MarkColors) -> Self {
        Self { viewer, family, bg, gutter_fg, text_fg, find_match, find_current, marks }
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

/// One visible display row, copied out of the viewer.
enum Row {
    /// A current line: its index, text, spans, mark, and whether lines were deleted just above
    /// it (marked only with the inline diff off).
    Line { n: usize, text: String, spans: Vec<Span>, mark: Option<Mark>, deleted_above: bool },
    /// A deleted line, with the inline diff on.
    Deleted(String),
}

/// What one frame paints, copied out of the viewer: only the visible rows and their hits.
struct Frame {
    line_count: usize,
    rows: Vec<Row>,
    /// The viewer has a diff: the gutter holds the mark column.
    has_diff: bool,
    /// Lines deleted at the end of the file: marked at the top of this row (under the last line).
    deleted_at_end: Option<usize>,
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
            let diff = v.diff.as_ref();
            let inline = v.inline_diff && diff.is_some();
            let range = visible_range(v.scroll.current, v.rows_visible, v.row_count());
            let shown: Vec<DisplayRow> = match diff {
                Some(d) if inline => display_rows(lines.len(), d, true)[range.clone()].to_vec(),
                _ => range.clone().map(DisplayRow::Line).collect(),
            };
            let deleted_before = |i: usize| diff.is_some_and(|d| d.deleted_before.iter().any(|(before, _)| *before == i));
            let rows: Vec<Row> = shown
                .iter()
                .map(|r| match *r {
                    DisplayRow::Line(n) => Row::Line {
                        n,
                        text: lines[n].clone(),
                        spans: spans.get(n).cloned().unwrap_or_default(),
                        mark: diff.and_then(|d| d.marks.get(n).copied().flatten()),
                        deleted_above: !inline && deleted_before(n),
                    },
                    DisplayRow::Deleted(k, j) => Row::Deleted(diff.and_then(|d| d.deleted_before.get(k)).and_then(|(_, l)| l.get(j)).cloned().unwrap_or_default()),
                })
                .collect();
            // Without the inline diff the rows are the lines: the end mark sits under the last one.
            let deleted_at_end = (!inline && range.end == lines.len() && deleted_before(lines.len())).then(|| lines.len() - range.start);
            // Hits are in reading order: the visible lines' are one run of them.
            let visible_lines = || shown.iter().filter_map(|r| if let DisplayRow::Line(n) = r { Some(*n) } else { None });
            let (lo_line, hi_line) = (visible_lines().next().unwrap_or(0), visible_lines().last().map_or(0, |n| n + 1));
            let (hits, current) = match &v.find {
                Some(f) => {
                    let lo = f.hits.partition_point(|h| h.line < lo_line);
                    let hi = f.hits.partition_point(|h| h.line < hi_line).max(lo);
                    (f.hits[lo..hi].to_vec(), f.hits.get(f.current).copied())
                }
                None => (Vec::new(), None),
            };
            Frame {
                line_count: lines.len(),
                rows,
                has_diff: diff.is_some(),
                deleted_at_end,
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
        let gutter = if frame.has_diff { gutter_width_with_marks(frame.line_count, cell_w) } else { gutter_width(frame.line_count, cell_w) };
        let origin = bounds.origin;
        let x = origin.x + px(gutter + LEFT_PAD);
        let row_y = |r: usize| origin.y + px((r as f32 - frame.frac) * line_h);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for (r, row) in frame.rows.iter().enumerate() {
                let y = row_y(r);
                let (n, line, spans, mark, deleted_above) = match row {
                    Row::Deleted(text) => {
                        // No number; the old text in the deletion colour on a faint wash of it.
                        let mut tint = hsla(self.marks.deleted);
                        tint.a = 0.12;
                        window.paint_quad(fill(Bounds { origin: point(origin.x, y), size: size(bounds.size.width, px(line_h)) }, tint));
                        if !text.is_empty() {
                            let (shown, cut) = paint_cut(text);
                            let run = self.run(shown.len(), self.marks.deleted, false, false);
                            let shaped = window.text_system().shape_line(shown.to_string().into(), font_size, &[run], None);
                            let _ = shaped.paint(point(x, y), px(line_h), window, cx);
                            if cut {
                                self.paint_more(x + shaped.width, y, line_h, font_size, window, cx);
                            }
                        }
                        continue;
                    }
                    Row::Line { n, text, spans, mark, deleted_above } => (*n, text, spans, *mark, *deleted_above),
                };
                if let Some(mark) = mark {
                    let color = match mark {
                        Mark::Added => self.marks.added,
                        Mark::Modified => self.marks.modified,
                    };
                    window.paint_quad(fill(Bounds { origin: point(origin.x + px(1.0), y), size: size(px(BAR_W), px(line_h)) }, hsla(color)));
                }
                if deleted_above {
                    self.paint_deleted_mark(origin.x, y, window);
                }
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
                let spans = Some(spans).filter(|s| s.last().map(|l| l.end) == Some(shown.len()));
                let runs: Vec<TextRun> = match spans {
                    Some(spans) => spans.iter().map(|s| self.run(s.end - s.start, s.color, s.bold, s.italic)).collect(),
                    None => vec![self.run(shown.len(), self.text_fg, false, false)],
                };
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
                    self.paint_more(x + shaped.width, y, line_h, font_size, window, cx);
                }
            }
            if let Some(r) = frame.deleted_at_end {
                self.paint_deleted_mark(origin.x, row_y(r), window);
            }
        });
    }
}

impl ViewerElement {
    /// A dim marker where the painted part of a cut line ends.
    fn paint_more(&self, x: Pixels, y: Pixels, line_h: f32, font_size: Pixels, window: &mut Window, cx: &mut App) {
        let more = window.text_system().shape_line("…".into(), font_size, &[self.run("…".len(), self.gutter_fg, false, false)], None);
        let _ = more.paint(point(x, y), px(line_h), window, cx);
    }

    /// Lines were deleted here: a short bar across the mark column at the row's top edge.
    fn paint_deleted_mark(&self, x: Pixels, y: Pixels, window: &mut Window) {
        window.paint_quad(fill(Bounds { origin: point(x, y), size: size(px(MARK_W), px(DELETED_H)) }, hsla(self.marks.deleted)));
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

    #[test]
    fn the_gutter_grows_by_the_mark_column_and_a_display_row_maps_back_to_its_line() {
        assert_eq!(gutter_width_with_marks(99, 10.0), gutter_width(99, 10.0) + MARK_W);
        let d = crate::diff::DiffMarks { marks: vec![None, Some(crate::diff::Mark::Added)], deleted_before: vec![(1, vec!["gone".into()])] };
        let rows = crate::diff::display_rows(2, &d, true);
        assert_eq!(rows, vec![crate::diff::DisplayRow::Line(0), crate::diff::DisplayRow::Deleted(0, 0), crate::diff::DisplayRow::Line(1)]);
        assert_eq!(visible_range(0.0, 10, rows.len()), 0..3);
    }
}
