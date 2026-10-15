//! Paints a `Viewer`'s visible lines (spec 6: virtualized, line numbers in a gutter) at a
//! fractional line offset, like the terminal element. Only the lines in the viewport are shaped.
//! A file with changes against HEAD gets a mark column at the gutter's left; with the inline
//! diff on, the deleted lines are rows of their own.
//!
//! A markdown document is painted as raw text: its tabs drawn four columns wide, coloured from
//! `md_blocks::raw_marks`, with the caret, the selection and an input method's marked text, scrolled
//! sideways to follow the caret (it does not wrap). The painting helpers are free functions so the
//! rendered view's raw block paints the same way; that block wraps its lines, so the helpers that
//! measure a line also come in a version for a wrapped line.
use crate::diff::{display_rows, DisplayRow, Mark};
use crate::edit::{Caret, Pos};
use crate::md_blocks;
use crate::palette::Rgb;
use crate::viewer::{Content, Hit, Span, Viewer};
use gpui::{
    fill, font, point, px, size, App, Bounds, ContentMask, CursorStyle, DispatchPhase, Element, ElementId, ElementInputHandler, Entity, FontWeight,
    GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, Rgba, ShapedLine, SharedString, Style, TextRun, Window, WrappedLine,
};
use std::borrow::Cow;
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
/// The columns a tab takes in a document's raw text.
pub const TAB_COLS: usize = 4;
/// The caret's width.
const CARET_W: f32 = 2.0;

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

/// A line as it is drawn: each tab four spaces.
pub fn expand_tabs(line: &str) -> Cow<'_, str> {
    if line.contains('\t') {
        Cow::Owned(line.replace('\t', &" ".repeat(TAB_COLS)))
    } else {
        Cow::Borrowed(line)
    }
}

/// The display column of char `ch`: the chars before it, a tab counting four.
pub fn display_col(line: &str, ch: usize) -> usize {
    line.chars().take(ch).map(|c| if c == '\t' { TAB_COLS } else { 1 }).sum()
}

/// The char at display column `col`; a column inside a tab goes to the nearer side of it, and
/// one past the end to the line end.
pub fn ch_for_col(line: &str, col: usize) -> usize {
    let mut at = 0;
    for (i, c) in line.chars().enumerate() {
        let w = if c == '\t' { TAB_COLS } else { 1 };
        if col < at + w {
            return if (col - at) * 2 < w { i } else { i + 1 };
        }
        at += w;
    }
    line.chars().count()
}

/// Where byte `byte` of `line` lands in the line with its tabs expanded.
pub fn expanded_byte(line: &str, byte: usize) -> usize {
    line.as_bytes()[..byte.min(line.len())].iter().map(|&b| if b == b'\t' { TAB_COLS } else { 1 }).sum()
}

/// The x of display column `col` on a shaped line, from the line's start; past the shaped text,
/// its width.
pub fn x_for_col(shaped: &ShapedLine, expanded: &str, col: usize) -> Pixels {
    let byte = expanded.char_indices().nth(col).map_or(expanded.len(), |(b, _)| b);
    if byte >= shaped.len() {
        shaped.width
    } else {
        shaped.x_for_index(byte)
    }
}

/// The display column nearest `x` (from the line's start) on a shaped line.
pub fn col_for_x(shaped: &ShapedLine, expanded: &str, x: Pixels) -> usize {
    let len = shaped.len().min(expanded.len());
    if len == 0 || x <= px(0.0) {
        return 0;
    }
    let mut byte = shaped.closest_index_for_x(x).min(len);
    // Past the last glyph's start gpui answers the line end; the last char's start is nearer
    // while x is in its left half.
    if byte == len {
        if let Some((last, _)) = expanded[..len].char_indices().last() {
            let x0 = shaped.x_for_index(last);
            if x - x0 < shaped.width - x {
                byte = last;
            }
        }
    }
    while !expanded.is_char_boundary(byte) {
        byte -= 1;
    }
    expanded[..byte].chars().count()
}

/// How far the raw text scrolls sideways so the caret, `caret_x` from the text's start, stays
/// within `width`: unchanged while it is in view, else the least move that shows it.
pub fn follow_x(scroll_x: f32, caret_x: f32, width: f32) -> f32 {
    if caret_x < scroll_x {
        caret_x.max(0.0)
    } else if caret_x > scroll_x + width {
        caret_x - width
    } else {
        scroll_x
    }
}

/// The colours of raw markdown: plain text, markers and quote bars, code, links.
#[derive(Debug, Clone, Copy)]
pub struct RawColors {
    pub text: Rgb,
    pub dim: Rgb,
    pub code: Rgb,
    pub link: Rgb,
}

/// Where a document viewer's text was laid out at the last prepaint: what the input handler and
/// the mouse map between positions and pixels with.
#[derive(Debug, Clone)]
pub struct RawLayout {
    pub bounds: Bounds<Pixels>,
    /// The window x of column 0 before the sideways scroll.
    pub text_left: Pixels,
    pub line_h: f32,
    pub font_size: f32,
    pub family: SharedString,
    pub colors: RawColors,
}

/// One run of text in the terminal family.
fn text_run(family: &SharedString, len: usize, color: Rgb, bold: bool, italic: bool) -> TextRun {
    let mut f = font(family.clone());
    if bold {
        f.weight = FontWeight::BOLD;
    }
    if italic {
        f.style = gpui::FontStyle::Italic;
    }
    TextRun { len, font: f, color: hsla(color), background_color: None, underline: None, strikethrough: None }
}

/// The runs of a raw markdown line with its tabs expanded, up to `limit` expanded bytes: the
/// marks `raw_marks` finds in their colours, the gaps in the text colour.
pub fn raw_runs(line: &str, family: &SharedString, colors: &RawColors, limit: usize) -> Vec<TextRun> {
    let marks = md_blocks::raw_marks(line);
    let mut styled: Vec<(usize, Option<md_blocks::Mark>)> = Vec::new();
    let mut at = 0;
    for (b, c) in line.char_indices() {
        if at >= limit {
            break;
        }
        let w = if c == '\t' { TAB_COLS } else { c.len_utf8() }.min(limit - at);
        let mark = marks.iter().find(|(r, _)| r.contains(&b)).map(|(_, m)| *m);
        match styled.last_mut() {
            Some((len, m)) if *m == mark => *len += w,
            _ => styled.push((w, mark)),
        }
        at += w;
    }
    styled
        .into_iter()
        .map(|(len, mark)| {
            use md_blocks::Mark as M;
            let (color, bold, italic) = match mark {
                Some(M::Marker | M::Quote) => (colors.dim, false, false),
                Some(M::Heading) => (colors.text, true, false),
                Some(M::Emphasis) => (colors.text, false, true),
                Some(M::Code) => (colors.code, false, false),
                Some(M::Link) => (colors.link, false, false),
                None => (colors.text, false, false),
            };
            text_run(family, len, color, bold, italic)
        })
        .collect()
}

/// A raw markdown line shaped as it is painted: the expanded text that was shaped (cut like any
/// painted line) and the shaped line. An empty line shapes nothing.
pub fn shape_raw(window: &mut Window, line: &str, layout: &RawLayout) -> (String, ShapedLine) {
    let expanded = expand_tabs(line);
    let (shown, _) = paint_cut(&expanded);
    if shown.is_empty() {
        return (String::new(), ShapedLine::default());
    }
    let runs = raw_runs(line, &layout.family, &layout.colors, shown.len());
    let shaped = window.text_system().shape_line(shown.to_string().into(), px(layout.font_size), &runs, None);
    (shown.to_string(), shaped)
}

/// A raw markdown line shaped as the rendered view's raw block paints it: like `shape_raw`, but
/// wrapped at `wrap_width`. An empty line shapes to an empty wrapped line of one row.
pub fn shape_raw_wrapped(window: &mut Window, line: &str, layout: &RawLayout, wrap_width: Pixels) -> (String, WrappedLine) {
    let expanded = expand_tabs(line);
    let (shown, _) = paint_cut(&expanded);
    let runs = raw_runs(line, &layout.family, &layout.colors, shown.len());
    let shaped = window.text_system().shape_text(shown.to_string().into(), px(layout.font_size), &runs, Some(wrap_width), None).ok().and_then(|lines| lines.into_iter().next()).unwrap_or_default();
    (shown.to_string(), shaped)
}

/// Rows as byte ranges from the bytes where each wrapped row after the first begins, for a line
/// of `len` bytes.
pub fn rows_from_breaks(breaks: &[usize], len: usize) -> Vec<Range<usize>> {
    let mut out = Vec::with_capacity(breaks.len() + 1);
    let mut start = 0;
    for &b in breaks {
        let end = b.max(start);
        out.push(start..end);
        start = end;
    }
    out.push(start..len.max(start));
    out
}

/// The byte ranges of a wrapped line's rows, in order; one row when it did not wrap. A row begins
/// at the glyph of its wrap boundary.
pub fn wrapped_rows(wrapped: &WrappedLine) -> Vec<Range<usize>> {
    let layout = &wrapped.unwrapped_layout;
    let breaks: Vec<usize> = wrapped.wrap_boundaries().iter().map(|b| layout.runs.get(b.run_ix).and_then(|r| r.glyphs.get(b.glyph_ix)).map_or(0, |g| g.index)).collect();
    rows_from_breaks(&breaks, wrapped.len())
}

/// The row that byte `byte` is drawn on: a byte where a row starts is drawn at that row's start,
/// not at the end of the row above.
pub fn row_of(rows: &[Range<usize>], byte: usize) -> usize {
    rows.iter().rposition(|r| r.start <= byte).unwrap_or(0)
}

/// The x of `byte` on `row`, from the row's start; `x_of` is the x of a byte on the unwrapped
/// line.
fn x_on_row(x_of: &impl Fn(usize) -> Pixels, row: &Range<usize>, byte: usize) -> Pixels {
    x_of(byte.clamp(row.start, row.end)) - x_of(row.start)
}

/// Where byte `byte` is drawn on a line wrapped into `rows`, from the line's top left.
pub fn point_on_rows(rows: &[Range<usize>], x_of: impl Fn(usize) -> Pixels, byte: usize, line_h: Pixels) -> Point<Pixels> {
    let r = row_of(rows, byte);
    point(x_on_row(&x_of, &rows[r], byte), line_h * r as f32)
}

/// The pieces of the bytes `b0..b1` on a line wrapped into `rows`, one per row they cross:
/// `(x0, x1, y)` from the line's top left. Empty pieces are left out.
pub fn spans_on_rows(rows: &[Range<usize>], x_of: impl Fn(usize) -> Pixels, b0: usize, b1: usize, line_h: Pixels) -> Vec<(Pixels, Pixels, Pixels)> {
    rows.iter()
        .enumerate()
        .filter(|(_, r)| b0 <= r.end && r.start <= b1)
        .map(|(i, r)| (x_on_row(&x_of, r, b0), x_on_row(&x_of, r, b1), line_h * i as f32))
        .filter(|(x0, x1, _)| x1 > x0)
        .collect()
}

/// A press past the end of row `row` gets the row's end from gpui, which is the next row's first
/// byte and would put the caret at the start of the row below. On a row that is not the last
/// one this steps back one char, so the caret lands at the end of the row pressed.
pub fn back_off_row_end(rows: &[Range<usize>], row: usize, byte: usize, expanded: &str) -> usize {
    match rows.get(row) {
        Some(r) if row + 1 < rows.len() && byte == r.end && byte > r.start => {
            let at = byte.min(expanded.len());
            expanded[..expanded.floor_char_boundary(at)].char_indices().last().map_or(at, |(b, _)| b)
        }
        _ => byte,
    }
}

/// The x of a byte on a wrapped line before wrapping; past the shaped text, its width.
fn unwrapped_x(wrapped: &WrappedLine) -> impl Fn(usize) -> Pixels + '_ {
    let layout = &wrapped.unwrapped_layout;
    move |b| if b >= layout.len { layout.width } else { layout.x_for_index(b) }
}

/// The byte of display column `col` in `expanded`, clamped to the shaped length.
fn col_byte(expanded: &str, col: usize, len: usize) -> usize {
    expanded.char_indices().nth(col).map_or(expanded.len(), |(b, _)| b).min(len)
}

/// Where display column `col` of a wrapped line is drawn, from the line's top left: the x on its
/// row and the row's top.
pub fn wrapped_point_for_col(wrapped: &WrappedLine, expanded: &str, col: usize, line_h: Pixels) -> Point<Pixels> {
    point_on_rows(&wrapped_rows(wrapped), unwrapped_x(wrapped), col_byte(expanded, col, wrapped.len()), line_h)
}

/// The display column nearest a point given from a wrapped line's top left: a point above the
/// line is on its first row, one below on its last, one past a row's end at that end.
pub fn wrapped_col_for_point(wrapped: &WrappedLine, expanded: &str, at: Point<Pixels>, line_h: Pixels) -> usize {
    let rows = wrapped_rows(wrapped);
    let y = at.y.max(px(0.0)).min(line_h * rows.len() as f32 - px(1.0));
    let row = ((y / line_h).max(0.0) as usize).min(rows.len() - 1);
    let byte = match wrapped.closest_index_for_position(point(at.x.max(px(0.0)), y), line_h) {
        Ok(b) => b,
        Err(b) => back_off_row_end(&rows, row, b, expanded),
    };
    let mut byte = byte.min(expanded.len());
    while !expanded.is_char_boundary(byte) {
        byte -= 1;
    }
    expanded[..byte].chars().count()
}

/// The pieces of a wrapped line between display columns `c0` and `c1`, one per row they cross:
/// `(x0, x1, y)` from the line's top left, for the selection, find hits and marked text.
pub fn wrapped_spans(wrapped: &WrappedLine, expanded: &str, c0: usize, c1: usize, line_h: Pixels) -> Vec<(Pixels, Pixels, Pixels)> {
    let len = wrapped.len();
    spans_on_rows(&wrapped_rows(wrapped), unwrapped_x(wrapped), col_byte(expanded, c0, len), col_byte(expanded, c1, len), line_h)
}

/// The selection on one row, from `x0` to `x1`, in `color` at half strength.
pub fn paint_selection(window: &mut Window, x0: Pixels, x1: Pixels, y: Pixels, line_h: Pixels, color: Rgb) {
    if x1 <= x0 {
        return;
    }
    let mut c = hsla(color);
    c.a = 0.5;
    window.paint_quad(fill(Bounds { origin: point(x0, y), size: size(x1 - x0, line_h) }, c));
}

/// The caret: a bar two pixels wide, the row's height.
pub fn paint_caret(window: &mut Window, x: Pixels, y: Pixels, line_h: Pixels, color: Rgb) {
    window.paint_quad(fill(Bounds { origin: point(x, y), size: size(px(CARET_W), line_h) }, hsla(color)));
}

/// An input method's marked text: a line under it, at the row's foot.
pub fn paint_marked(window: &mut Window, x0: Pixels, x1: Pixels, y: Pixels, line_h: Pixels, color: Rgb) {
    if x1 <= x0 {
        return;
    }
    window.paint_quad(fill(Bounds { origin: point(x0, y + line_h - px(2.0)), size: size(x1 - x0, px(1.0)) }, hsla(color)));
}

/// Find hits on one row, `(x0, x1, current)` each, under the text.
pub fn paint_hits(window: &mut Window, hits: &[(Pixels, Pixels, bool)], y: Pixels, line_h: Pixels, find_match: Rgb, find_current: Rgb) {
    for &(x0, x1, current) in hits {
        let mut col = hsla(if current { find_current } else { find_match });
        col.a = 0.45;
        window.paint_quad(fill(Bounds { origin: point(x0, y), size: size(x1 - x0, line_h) }, col));
    }
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
    raw: RawColors,
}

impl ViewerElement {
    pub fn new(viewer: Entity<Viewer>, family: SharedString, bg: Rgb, gutter_fg: Rgb, text_fg: Rgb, find_match: Rgb, find_current: Rgb, marks: MarkColors, raw: RawColors) -> Self {
        Self { viewer, family, bg, gutter_fg, text_fg, find_match, find_current, marks, raw }
    }

    fn run(&self, len: usize, color: Rgb, bold: bool, italic: bool) -> TextRun {
        text_run(&self.family, len, color, bold, italic)
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
    /// The document's caret, its marked text and whether the caret shows (the viewer has focus);
    /// `None` for a file that is not a document.
    doc: Option<DocFrame>,
}

/// What a document adds to a frame.
struct DocFrame {
    caret: Caret,
    marked: Option<Range<Pos>>,
    focused: bool,
    scroll_x: f32,
    layout: RawLayout,
}

pub struct Prepaint {
    cell_w: f32,
    line_h: f32,
    /// A document's text area takes the mouse: the caret, the selection, the I-beam.
    hitbox: Option<Hitbox>,
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
        let (font_size, line_height, line_count, has_diff, caret_line) = {
            let v = self.viewer.read(cx);
            // The caret's line and char, for following the caret sideways.
            let caret_line = v.doc.as_ref().map(|d| {
                let head = d.clamp(v.caret.head);
                (d.lines()[head.line].clone(), head.ch)
            });
            (v.font_size, v.line_height, v.line_count(), v.diff.is_some(), caret_line)
        };
        let run = TextRun { len: 1, font: font(self.family.clone()), color: gpui::white(), background_color: None, underline: None, strikethrough: None };
        let cell_w = f32::from(window.text_system().layout_line("W", px(font_size), &[run], None).width);
        let line_h = font_size * line_height;
        let rows = (f32::from(bounds.size.height) / line_h).floor().max(1.0) as usize;
        let Some((line, ch)) = caret_line else {
            self.viewer.update(cx, |v, _| {
                v.rows_visible = rows;
                v.last_bounds = None;
            });
            return Prepaint { cell_w, line_h, hitbox: None };
        };
        let gutter = if has_diff { gutter_width_with_marks(line_count, cell_w) } else { gutter_width(line_count, cell_w) };
        let layout = RawLayout { bounds, text_left: bounds.origin.x + px(gutter + LEFT_PAD), line_h, font_size, family: self.family.clone(), colors: self.raw };
        let (expanded, shaped) = shape_raw(window, &line, &layout);
        let caret_x = f32::from(x_for_col(&shaped, &expanded, display_col(&line, ch)));
        // Two cells of room past the caret, so it never sits on the edge.
        let width = (f32::from(bounds.size.width) - gutter - LEFT_PAD - cell_w * 2.0).max(cell_w);
        self.viewer.update(cx, |v, _| {
            v.rows_visible = rows;
            v.scroll_x = follow_x(v.scroll_x, caret_x, width);
            v.last_bounds = Some(layout);
        });
        Prepaint { cell_w, line_h, hitbox: Some(window.insert_hitbox(bounds, HitboxBehavior::Normal)) }
    }

    fn paint(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _state: &mut (), prepaint: &mut Prepaint, window: &mut Window, cx: &mut App) {
        let (cell_w, line_h) = (prepaint.cell_w, prepaint.line_h);
        window.paint_quad(fill(bounds, hsla(self.bg)));
        let focused = self.viewer.read(cx).focus.is_focused(window);
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
                doc: match (&v.doc, &v.last_bounds) {
                    (Some(_), Some(layout)) => Some(DocFrame { caret: v.caret, marked: v.marked.clone(), focused, scroll_x: v.scroll_x, layout: layout.clone() }),
                    _ => None,
                },
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
                if let Some(doc) = &frame.doc {
                    self.paint_doc_line(doc, n, line, x, y, line_h, cell_w, gutter, &frame.hits, frame.current, window, cx);
                    continue;
                }
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
        if let Some(hitbox) = prepaint.hitbox.clone() {
            self.listen(hitbox, bounds, window, cx);
        }
    }
}

impl ViewerElement {
    /// One line of a document: the selection, the find hits, the coloured raw text, the marked
    /// text's underline and the caret, scrolled sideways by `scroll_x` and clipped to the text
    /// area right of the gutter.
    fn paint_doc_line(&self, doc: &DocFrame, n: usize, line: &str, x: Pixels, y: Pixels, line_h: f32, cell_w: f32, gutter: f32, hits: &[Hit], current: Option<Hit>, window: &mut Window, cx: &mut App) {
        let bounds = doc.layout.bounds;
        let text_area = Bounds::from_corners(point(bounds.origin.x + px(gutter), bounds.origin.y), bounds.bottom_right());
        let (tx, lh) = (x - px(doc.scroll_x), px(line_h));
        let (expanded, shaped) = shape_raw(window, line, &doc.layout);
        let at = |col: usize| tx + x_for_col(&shaped, &expanded, col);
        window.with_content_mask(Some(ContentMask { bounds: text_area }), |window| {
            let c = doc.caret;
            let (lo, hi) = if c.anchor <= c.head { (c.anchor, c.head) } else { (c.head, c.anchor) };
            if lo != hi && lo.line <= n && n <= hi.line {
                let c0 = if n == lo.line { display_col(line, lo.ch) } else { 0 };
                let c1 = if n == hi.line { display_col(line, hi.ch) } else { expanded.chars().count() };
                // A selection running on past the line's end shows its line break as half a cell.
                let more = if n < hi.line { px(cell_w / 2.0) } else { px(0.0) };
                paint_selection(window, at(c0), at(c1) + more, y, lh, self.find_match);
            }
            let row_hits: Vec<(Pixels, Pixels, bool)> = hits
                .iter()
                .filter(|h| h.line == n)
                .map(|h| {
                    let x_of = |byte: usize| {
                        let e = expanded_byte(line, byte);
                        tx + if e >= shaped.len() { shaped.width } else { shaped.x_for_index(e) }
                    };
                    (x_of(h.start), x_of(h.end), current == Some(*h))
                })
                .collect();
            paint_hits(window, &row_hits, y, lh, self.find_match, self.find_current);
            if !expanded.is_empty() {
                let _ = shaped.paint(point(tx, y), lh, window, cx);
                if expanded.len() < expand_tabs(line).len() {
                    self.paint_more(tx + shaped.width, y, line_h, px(doc.layout.font_size), window, cx);
                }
            }
            if let Some(m) = doc.marked.as_ref().filter(|m| m.start.line <= n && n <= m.end.line) {
                let c0 = if n == m.start.line { display_col(line, m.start.ch) } else { 0 };
                let c1 = if n == m.end.line { display_col(line, m.end.ch) } else { expanded.chars().count() };
                paint_marked(window, at(c0), at(c1), y, lh, self.text_fg);
            }
            if doc.focused && c.head.line == n {
                paint_caret(window, at(display_col(line, c.head.ch)), y, lh, self.text_fg);
            }
        });
    }

    /// A document takes text input while focused, and the mouse in its text: a press places the
    /// caret (Shift extends the selection), a drag selects.
    fn listen(&self, hitbox: Hitbox, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        window.set_cursor_style(CursorStyle::IBeam, &hitbox);
        let focus = self.viewer.read(cx).focus.clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.viewer.clone()), cx);
        let v = self.viewer.clone();
        window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && ev.button == MouseButton::Left && hitbox.is_hovered(window) {
                v.update(cx, |v, cx| v.mouse_down(ev.position, ev.modifiers.shift, window, cx));
            }
        });
        let v = self.viewer.clone();
        window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && ev.pressed_button == Some(MouseButton::Left) {
                v.update(cx, |v, cx| v.mouse_drag(ev.position, window, cx));
            }
        });
        let v = self.viewer.clone();
        window.on_mouse_event(move |ev: &MouseUpEvent, phase, _window, cx| {
            if phase == DispatchPhase::Bubble && ev.button == MouseButton::Left {
                v.update(cx, |v, _| v.mouse_up());
            }
        });
    }

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
    fn a_tab_is_four_columns_and_a_column_maps_back_to_its_char() {
        assert_eq!(expand_tabs("a\tb"), "a    b");
        assert_eq!(display_col("a\tb", 2), 5, "a, then the tab's four columns");
        assert_eq!(display_col("a\tb", 9), 6, "past the end: the line's width");
        assert_eq!(ch_for_col("a\tb", 2), 1, "the tab's left half is before it");
        assert_eq!(ch_for_col("a\tb", 3), 2, "its right half is after it");
        assert_eq!(ch_for_col("a\tb", 5), 2);
        assert_eq!(ch_for_col("a\tb", 40), 3, "past the end: the line end");
        assert_eq!(expanded_byte("\tx\u{e9}y", 1), 4);
        assert_eq!(expanded_byte("\tx\u{e9}y", 4), 7, "a two-byte char keeps its bytes");
    }

    #[test]
    fn the_raw_view_follows_the_caret_sideways_by_the_least_move() {
        assert_eq!(follow_x(0.0, 50.0, 100.0), 0.0, "in view: unchanged");
        assert_eq!(follow_x(0.0, 150.0, 100.0), 50.0, "off the right edge: just enough to show it");
        assert_eq!(follow_x(80.0, 20.0, 100.0), 20.0, "off the left edge: back to it");
        assert_eq!(follow_x(80.0, 0.0, 100.0), 0.0);
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

    #[test]
    fn wrapped_rows_place_bytes_spans_and_presses_past_a_row_end() {
        // A monospace line of 12 bytes, ten pixels a byte, wrapped before bytes 5 and 9.
        let rows = rows_from_breaks(&[5, 9], 12);
        assert_eq!(rows, vec![0..5, 5..9, 9..12]);
        assert_eq!(rows_from_breaks(&[], 3), vec![0..3], "an unwrapped line is one row");
        let x = |b: usize| px(b as f32 * 10.0);
        let lh = px(20.0);
        assert_eq!(point_on_rows(&rows, x, 4, lh), point(px(40.0), px(0.0)));
        assert_eq!(point_on_rows(&rows, x, 5, lh), point(px(0.0), px(20.0)), "a row's first byte is drawn at that row's start");
        assert_eq!(point_on_rows(&rows, x, 12, lh), point(px(30.0), px(40.0)), "the line's end, on the last row");
        assert_eq!(spans_on_rows(&rows, x, 3, 7, lh), vec![(px(30.0), px(50.0), px(0.0)), (px(0.0), px(20.0), px(20.0))]);
        assert_eq!(spans_on_rows(&rows, x, 5, 9, lh), vec![(px(0.0), px(40.0), px(20.0))], "exactly the middle row");
        let text = "abcd efghijk";
        assert_eq!(back_off_row_end(&rows, 0, 5, text), 4, "past the first row's end: its last char, not the next row's start");
        assert_eq!(back_off_row_end(&rows, 2, 12, text), 12, "the last row's end stays");
        assert_eq!(back_off_row_end(&rows, 1, 5, text), 5, "a row's start stays");
    }
}
