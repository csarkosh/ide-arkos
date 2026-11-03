//! Draws a book's active sheet as a grid (csv-grid spec, section 5): a row-number gutter down the
//! left and the header row across the top, both pinned, over a body that scrolls down by rows and
//! sideways by points. Only the rows in the pane are shaped and painted, so a sheet of 200,000
//! rows costs what thirty do. The cells take the markdown table's fonts, padding, fills and rules.
use crate::effects::active::Style;
use crate::effects::blocks::{Kind, Shape};
use crate::grid::{Cell, ColumnKind, Scope, Selection, Sheet};
use crate::md_view::{self, CELL_PAD_X, CELL_PAD_Y, COLUMN_CAP, MEASURED_ROWS, PARA};
use crate::theme::Theme;
use crate::viewer::Viewer;
use crate::viewer_element::GUTTER_PAD;
use crate::Root;
use gpui::{
    fill, font, point, px, quad, size, App, BorderStyle, Bounds, ContentMask, DispatchPhase, Element, ElementId, Entity, FontWeight, GlobalElementId, Hitbox, HitboxBehavior, Hsla, InspectorElementId,
    IntoElement, Keystroke, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ShapedLine, SharedString, TextRun, WeakEntity, Window,
};
use std::ops::{Range, RangeInclusive};
use std::time::{Duration, Instant};

/// A cell is cut here before it is shaped: past it nothing a column could show is left.
pub const MAX_CELL_CHARS: usize = 512;

/// Where the parts of the grid sit, in points from the element's top left, as the last prepaint
/// laid them out.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub gutter_w: f32,
    pub header_h: f32,
    pub row_h: f32,
    /// Each column's left edge from the body's left before the sideways scroll, and the right
    /// edge of the last: one more than the columns.
    pub cols: Vec<f32>,
    /// The element's size.
    pub view_w: f32,
    pub view_h: f32,
}

impl Layout {
    /// The width of the body, right of the gutter.
    pub fn body_w(&self) -> f32 {
        (self.view_w - self.gutter_w).max(0.0)
    }

    /// The height of the body, under the header.
    pub fn body_h(&self) -> f32 {
        (self.view_h - self.header_h).max(0.0)
    }

    /// The columns' widths added up.
    pub fn content_w(&self) -> f32 {
        self.cols.last().copied().unwrap_or(0.0)
    }

    /// The furthest the body scrolls sideways: the columns past the body's width.
    pub fn max_scroll_x(&self) -> f32 {
        (self.content_w() - self.body_w()).max(0.0)
    }
}

/// The drawn width of each column: its natural width, capped at `COLUMN_CAP` of a view `view_w`
/// wide, and never below `floor`.
pub fn column_widths(natural: &[f32], view_w: f32, floor: f32) -> Vec<f32> {
    natural.iter().map(|w| w.min(COLUMN_CAP * view_w).max(floor)).collect()
}

/// The body rows to paint: from the row at the scroll's top to the one at the body's bottom edge,
/// plus one, within the sheet's `rows`.
pub fn row_range(scroll_rows: f32, view_h: f32, row_h: f32, rows: usize) -> Range<usize> {
    let top = scroll_rows.max(0.0);
    let first = (top.floor() as usize).min(rows);
    // The last row in sight is the one under the body's bottom edge, `top + fit` rows down.
    let fit = if row_h > 0.0 { (view_h / row_h).max(0.0) } else { 0.0 };
    let last = (top + fit).floor() as usize;
    first..(last + 1).min(rows).max(first)
}

/// What a cell shows: its first line, cut at `MAX_CELL_CHARS` chars, and whether anything was
/// left out.
pub fn visible_prefix(text: &str) -> (&str, bool) {
    let line = text.find(['\n', '\r']).map_or(text, |end| &text[..end]);
    let broke = line.len() < text.len();
    match line.char_indices().nth(MAX_CELL_CHARS) {
        Some((end, _)) => (&line[..end], true),
        None => (line, broke),
    }
}

/// What is under a point of the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// The blank cell at the top left, where the gutter meets the header.
    Corner,
    /// A column's header cell.
    Header(usize),
    /// A body row's number, by its grid row (the first body row is 1).
    RowNumber(usize),
    Cell(Cell),
    /// Off the grid, or past its last row or column.
    Outside,
}

/// What is under `at`, a point from the element's top left, with the body scrolled `scroll_rows`
/// rows down and `scroll_x` points to the right.
pub fn hit(at: Point<f32>, layout: &Layout, scroll_rows: f32, scroll_x: f32, sheet: &Sheet) -> Hit {
    if at.x < 0.0 || at.y < 0.0 || at.x >= layout.view_w || at.y >= layout.view_h {
        return Hit::Outside;
    }
    let in_gutter = at.x < layout.gutter_w;
    let in_header = at.y < layout.header_h;
    // The column under the point, the header's or the body's: both scroll sideways together.
    let col = || {
        let x = at.x - layout.gutter_w + scroll_x;
        let c = layout.cols.partition_point(|&left| left <= x);
        (c >= 1 && c < layout.cols.len() && c <= sheet.cols()).then(|| c - 1)
    };
    // The grid row under the point: body row `i` is grid row `i + 1`.
    let row = || {
        let r = (scroll_rows.max(0.0) + (at.y - layout.header_h) / layout.row_h).floor() as usize;
        (r < sheet.rows.len()).then_some(r + 1)
    };
    match (in_gutter, in_header) {
        (true, true) => Hit::Corner,
        (false, true) => col().map_or(Hit::Outside, Hit::Header),
        (true, false) => row().map_or(Hit::Outside, Hit::RowNumber),
        (false, false) => match (row(), col()) {
            (Some(row), Some(col)) => Hit::Cell(Cell { row, col }),
            _ => Hit::Outside,
        },
    }
}

/// The gutter's width for row numbers up to `last_row_number`, as the text view's gutter is.
pub fn gutter_width(last_row_number: usize, cell_w: f32) -> f32 {
    crate::viewer_element::gutter_width(last_row_number, cell_w)
}

/// How long the pointer rests on a cut cell before its full text shows.
pub const TIP_DELAY: Duration = Duration::from_millis(500);
/// The tooltip wraps its text at this many characters and shows at most this many lines.
const TIP_WRAP: usize = 60;
const TIP_LINES: usize = 8;
/// The tooltip's padding, its distance from the pointer and its corners' radius.
const TIP_PAD_X: f32 = 8.0;
const TIP_PAD_Y: f32 = 5.0;
const TIP_OFFSET: f32 = 14.0;
const TIP_RADIUS: f32 = 6.0;
/// The ring round the head cell.
const RING_W: f32 = 2.0;
/// The alpha of a find hit's tint, the text view's.
const HIT_ALPHA: f32 = 0.45;

/// The tooltip's lines for a cell's text: its first `MAX_CELL_CHARS` chars, each of its lines
/// wrapped at `TIP_WRAP` characters (at a space when there is one), at most `TIP_LINES` of them,
/// the last ending in an ellipsis when more was left out.
pub fn tip_lines(text: &str) -> Vec<String> {
    let text = text.char_indices().nth(MAX_CELL_CHARS).map_or(text, |(end, _)| &text[..end]);
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let mut rest: Vec<char> = line.trim_end_matches('\r').chars().collect();
        loop {
            if rest.len() <= TIP_WRAP {
                out.push(rest.iter().collect());
                break;
            }
            // The last space within the width breaks the line; a word longer than it is cut.
            let at = rest[..=TIP_WRAP].iter().rposition(|&c| c == ' ').filter(|&i| i > 0).unwrap_or(TIP_WRAP);
            out.push(rest[..at].iter().collect::<String>().trim_end().to_string());
            rest.drain(..at);
            while rest.first() == Some(&' ') {
                rest.remove(0);
            }
        }
    }
    if out.len() > TIP_LINES {
        out.truncate(TIP_LINES);
        if let Some(last) = out.last_mut() {
            last.push('\u{2026}');
        }
    }
    out
}

/// What the column widths were measured at: the cells' family and size, the book they were
/// measured over and its sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct GridWidthsKey {
    pub family: SharedString,
    pub size: f32,
    pub book_gen: u64,
    pub sheet: usize,
}

/// The colours a grid paints in, read from the theme each frame: the markdown table's (its
/// header fill, paper, stripe, rules, accent and reading text), the text view's gutter and find
/// colours, and the pill's for the tooltip.
#[derive(Debug, Clone, Copy)]
pub struct GridColors {
    pub bg: Hsla,
    pub text: Hsla,
    pub gutter: Hsla,
    pub header: Hsla,
    pub paper: Hsla,
    pub tint: Hsla,
    pub rule: Hsla,
    /// The line under the header and the gutter's edge: clear when an effect inks the borders,
    /// as a table's outline is.
    pub outline: Hsla,
    pub link: Hsla,
    pub find_match: Hsla,
    pub find_current: Hsla,
    pub tip_bg: Hsla,
    pub tip_border: Hsla,
    pub tip_text: Hsla,
}

impl GridColors {
    pub fn new(theme: &Theme, style: Style) -> Self {
        let md = crate::md_view::md_colors(theme);
        let (rule, outline) = if style.ink_borders { (md.text.opacity(0.45), gpui::transparent_black()) } else { (theme.chrome.border.hsla(), theme.chrome.border.hsla()) };
        GridColors {
            bg: theme.chrome.window_bg.hsla(),
            text: md.text,
            gutter: theme.chrome.text_dim.hsla(),
            header: theme.chrome.tab_inactive.hsla(),
            paper: md.on_accent,
            tint: md.text.opacity(0.04),
            rule,
            outline,
            link: md.link,
            find_match: theme.semantic.find_match.hsla(),
            find_current: theme.semantic.find_current.hsla(),
            tip_bg: theme.chrome.banner.alpha(0.92),
            tip_border: theme.chrome.border.hsla(),
            tip_text: theme.chrome.text.hsla(),
        }
    }
}

pub struct GridElement {
    viewer: Entity<Viewer>,
    /// The cells' family (the prose font) and the gutter's (the terminal font).
    prose: SharedString,
    mono: SharedString,
    colors: GridColors,
    /// The window's ground under an effect (`Style::ground_alpha`), which the fills go with.
    ground: f32,
    /// Where the grid reports itself to the wash while an effect is on.
    shapes: Option<WeakEntity<Root>>,
    paper_alpha: f32,
    paper_radius: f32,
}

impl GridElement {
    pub fn new(viewer: Entity<Viewer>, prose: SharedString, mono: SharedString, colors: GridColors, ground: f32, shapes: Option<WeakEntity<Root>>) -> Self {
        Self { viewer, prose, mono, colors, ground, shapes, paper_alpha: 1.0, paper_radius: 0.0 }
    }

    /// The paper painted with `alpha`, so an effect's wash shows faintly through it.
    pub fn paper_alpha(mut self, alpha: f32) -> Self {
        self.paper_alpha = alpha;
        self
    }

    /// Rounds the paper's bottom corners by `radius` points.
    pub fn paper_radius(mut self, radius: f32) -> Self {
        self.paper_radius = radius;
        self
    }

    fn run(&self, family: &SharedString, len: usize, color: Hsla, weight: FontWeight) -> TextRun {
        let mut f = font(family.clone());
        f.weight = weight;
        TextRun { len, font: f, color, background_color: None, underline: None, strikethrough: None }
    }
}

/// One cell's text as a frame copies it: what shows, and whether anything was left out.
struct Text {
    shown: String,
    cut: bool,
}

/// What one frame paints, copied out of the viewer: the visible rows and columns only.
struct Frame {
    layout: Layout,
    scroll_rows: f32,
    scroll_x: f32,
    rows: Range<usize>,
    cols: Range<usize>,
    /// The visible columns' header cells and kinds, and the visible rows' cells, from `cols.start`.
    header: Vec<Text>,
    numeric: Vec<bool>,
    body: Vec<Vec<Text>>,
    first_row: usize,
    sel: Option<Selection>,
    sel_rect: Option<(RangeInclusive<usize>, RangeInclusive<usize>)>,
    /// The find's hits in sight, and whether each is the current one.
    hits: Vec<(Cell, bool)>,
    /// The cell the pointer has rested on long enough, and its text.
    tip: Option<(Cell, String)>,
    font_size: f32,
    animating: bool,
}

pub struct Prepaint {
    hitbox: Hitbox,
    layout: Option<Layout>,
}

impl Element for GridElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let mut style = gpui::Style::default();
        style.size.width = gpui::relative(1.0).into();
        style.size.height = gpui::relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    /// Measures the columns when the font, the size or the book changed, lays the grid out for
    /// these bounds and leaves the layout on the viewer for the mouse and the scroll limits.
    fn prepaint(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _state: &mut (), window: &mut Window, cx: &mut App) -> Prepaint {
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let (font_size, line_height) = {
            let v = self.viewer.read(cx);
            (v.font_size, v.line_height)
        };
        let size = font_size * PARA;
        let mono_run = self.run(&self.mono, 1, gpui::white(), FontWeight::NORMAL);
        let cell_w = f32::from(window.text_system().layout_line("W", px(font_size), &[mono_run], None).width);
        let measured = {
            let v = self.viewer.read(cx);
            let Some(sheet) = v.sheet() else {
                return Prepaint { hitbox, layout: None };
            };
            let key = GridWidthsKey { family: self.prose.clone(), size, book_gen: v.book_gen, sheet: v.sheet };
            let fresh = v.grid_widths.as_ref().is_some_and(|(k, w)| *k == key && w.len() == sheet.cols());
            (!fresh).then(|| (key, self.measure(sheet, size, window)))
        };
        let (view_w, view_h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let row_h = (size * line_height).ceil() + 2.0 * CELL_PAD_Y;
        let floor = 3.0 * cell_w + 2.0 * CELL_PAD_X;
        let layout = self.viewer.update(cx, |v, _| {
            if let Some(m) = measured {
                v.grid_widths = Some(m);
            }
            let natural = v.grid_widths.as_ref().map(|(_, w)| w.as_slice()).unwrap_or(&[]);
            let sheet = v.sheet()?;
            let last = (sheet.first_row + sheet.rows.len()).saturating_sub(1);
            let mut cols = Vec::with_capacity(natural.len() + 1);
            cols.push(0.0);
            for w in column_widths(natural, view_w, floor) {
                cols.push(cols.last().copied().unwrap_or(0.0) + w);
            }
            let layout = Layout { gutter_w: gutter_width(last, cell_w), header_h: row_h, row_h, cols, view_w, view_h };
            v.rows_visible = ((layout.body_h() / row_h).floor() as usize).max(1);
            v.grid_scroll_x = v.grid_scroll_x.clamp(0.0, layout.max_scroll_x());
            v.last_bounds = None;
            v.grid_layout = Some(layout.clone());
            Some(layout)
        });
        if let (Some(root), Some(l)) = (self.shapes.clone(), layout.as_ref()) {
            self.report_shapes(root, l, bounds, window, cx);
        }
        Prepaint { hitbox, layout }
    }

    fn paint(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _state: &mut (), prepaint: &mut Prepaint, window: &mut Window, cx: &mut App) {
        let mut paper = self.colors.bg;
        paper.a = self.paper_alpha;
        let r = px(self.paper_radius);
        window.paint_quad(fill(bounds, paper).corner_radii(gpui::Corners { top_left: px(0.0), top_right: px(0.0), bottom_right: r, bottom_left: r }));
        let Some(layout) = prepaint.layout.clone() else { return };
        let frame = self.viewer.update(cx, |v, _| {
            let now = Instant::now();
            let dt = v.last_frame.map(|l| now.duration_since(l).as_secs_f32().min(0.05)).unwrap_or(1.0 / 60.0);
            let animating = v.scroll.tick(dt);
            v.last_frame = if animating { Some(now) } else { None };
            let current = v.find.as_ref().and_then(|f| v.grid_hits.get(f.current)).copied();
            let tip_cell = v.grid_hover.filter(|(_, at)| at.elapsed() >= TIP_DELAY).map(|(c, _)| c);
            let sheet = v.sheet()?;
            Some(frame_of(sheet, layout, v.scroll.current, v.grid_scroll_x, v.grid_sel, &v.grid_hits, current, tip_cell, v.font_size, animating))
        });
        let Some(frame) = frame else { return };
        if frame.animating {
            window.request_animation_frame();
        }
        self.paint_grid(&frame, bounds, window, cx);
        self.listen(prepaint.hitbox.clone(), bounds, window, cx);
    }
}

/// Copies what a frame paints out of the sheet: the rows `row_range` finds and the columns that
/// meet the body, each cell cut by `visible_prefix`.
#[allow(clippy::too_many_arguments)]
fn frame_of(sheet: &Sheet, layout: Layout, scroll_rows: f32, scroll_x: f32, sel: Option<Selection>, hits: &[Cell], current: Option<Cell>, tip: Option<Cell>, font_size: f32, animating: bool) -> Frame {
    let rows = row_range(scroll_rows, layout.body_h(), layout.row_h, sheet.rows.len());
    let ncols = layout.cols.len().saturating_sub(1).min(sheet.cols());
    let first = layout.cols.partition_point(|&l| l <= scroll_x).saturating_sub(1).min(ncols);
    let last = layout.cols.partition_point(|&l| l < scroll_x + layout.body_w()).min(ncols).max(first);
    let cols = first..last;
    let text = |c: Cell| {
        let (shown, cut) = visible_prefix(sheet.text(c));
        Text { shown: shown.to_string(), cut }
    };
    let header = cols.clone().map(|col| text(Cell { row: 0, col })).collect();
    let numeric = cols.clone().map(|c| sheet.kinds.get(c) == Some(&ColumnKind::Number)).collect();
    let body = rows.clone().map(|r| cols.clone().map(|col| text(Cell { row: r + 1, col })).collect()).collect();
    let seen = |c: &Cell| (c.row == 0 || (rows.start < c.row && c.row <= rows.end)) && cols.contains(&c.col);
    let hits = hits.iter().filter(|c| seen(c)).map(|c| (*c, Some(*c) == current)).collect();
    let tip = tip.filter(seen).map(|c| (c, sheet.text(c).to_string()));
    Frame { layout, scroll_rows, scroll_x, rows, cols, header, numeric, body, first_row: sheet.first_row, sel, sel_rect: sel.map(|s| s.rect(sheet)), hits, tip, font_size, animating }
}

impl GridElement {
    /// Each column's natural width: its widest cell over the header (semibold) and the first
    /// `MEASURED_ROWS` rows, cut as it is drawn, plus the padding and the rule.
    fn measure(&self, sheet: &Sheet, size: f32, window: &mut Window) -> Vec<f32> {
        let mut natural = vec![0.0f32; sheet.cols()];
        for r in 0..=sheet.rows.len().min(MEASURED_ROWS) {
            let weight = if r == 0 { FontWeight::SEMIBOLD } else { FontWeight::NORMAL };
            for (col, w) in natural.iter_mut().enumerate() {
                let (shown, _) = visible_prefix(sheet.text(Cell { row: r, col }));
                let text_w = if shown.is_empty() {
                    0.0
                } else {
                    let run = self.run(&self.prose, shown.len(), gpui::white(), weight);
                    f32::from(window.text_system().layout_line(shown, px(size), &[run], None).width).ceil()
                };
                *w = w.max(text_w + 2.0 * CELL_PAD_X + 1.0);
            }
        }
        natural
    }

    /// Reports the painted grid to the wash as a table and its header row as a table's header,
    /// as a markdown table does.
    fn report_shapes(&self, root: WeakEntity<Root>, l: &Layout, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let (scroll_rows, scroll_x, rows) = {
            let v = self.viewer.read(cx);
            (v.scroll.current, v.grid_scroll_x, v.sheet().map_or(0, |s| s.rows.len()))
        };
        let w = (l.gutter_w + l.content_w() - scroll_x).min(l.view_w).max(0.0);
        let h = (l.header_h + (rows as f32 - scroll_rows).max(0.0) * l.row_h).min(l.view_h);
        let clip = window.content_mask().bounds;
        let key = self.viewer.entity_id().as_u64().wrapping_mul(1009).wrapping_add(GRID_KEY);
        let grid = md_view::seen(Bounds { origin: bounds.origin, size: size(px(w), px(h)) }, clip);
        let header = md_view::seen(Bounds { origin: bounds.origin, size: size(px(w), px(l.header_h)) }, clip);
        let _ = root.update(cx, |root, _| {
            for (rect, kind, key) in [(grid, Kind::Table, key), (header, Kind::TableHeader, key.wrapping_add(1))] {
                if let Some(rect) = rect {
                    root.effect_shapes.push(Shape { key, rect, kind, pitch: 0.0 });
                }
            }
        });
    }

    /// The body, clipped under the pinned header and gutter, then the header across the top, the
    /// gutter down the left, the corner, and last the tooltip over everything.
    fn paint_grid(&self, f: &Frame, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let l = &f.layout;
        let c = &self.colors;
        let o = bounds.origin;
        let size_px = px(f.font_size * PARA);
        let body_left = o.x + px(l.gutter_w);
        let body_top = o.y + px(l.header_h);
        let right = o.x + px((l.gutter_w + l.content_w() - f.scroll_x).min(l.view_w));
        let bottom = o.y + px((l.header_h + (f.rows.end as f32 - f.scroll_rows) * l.row_h).min(l.view_h));
        let col_x = |col: usize| body_left + px(l.cols[col] - f.scroll_x);
        let row_y = |r: usize| body_top + px((r as f32 - f.scroll_rows) * l.row_h);
        let body = Bounds::from_corners(point(body_left, body_top), bounds.bottom_right());
        let in_sel = |cell: Cell| f.sel_rect.as_ref().is_some_and(|(rs, cs)| rs.contains(&cell.row) && cs.contains(&cell.col));
        let mut tip_cut = false;
        window.with_content_mask(Some(ContentMask { bounds: body }), |window| {
            for (i, r) in f.rows.clone().enumerate() {
                let y = row_y(r);
                let fill_c = md_view::row_fill(r + 1, c.header, c.paper, c.tint, self.ground);
                window.paint_quad(fill(Bounds::from_corners(point(body_left, y), point(right, y + px(l.row_h))), fill_c));
                for (j, col) in f.cols.clone().enumerate() {
                    let cell = Cell { row: r + 1, col };
                    let cb = Bounds::from_corners(point(col_x(col), y), point(col_x(col + 1), y + px(l.row_h)));
                    let cut = self.paint_cell(cell, cb, &f.body[i][j], f.numeric[j], FontWeight::NORMAL, size_px, in_sel(cell), f, window, cx);
                    tip_cut |= cut && f.tip.as_ref().is_some_and(|(t, _)| *t == cell);
                }
                // The first row's top is the line under the header, painted with the outline.
                if r > 0 {
                    window.paint_quad(fill(Bounds { origin: point(body_left, y), size: size(right - body_left, px(1.0)) }, c.rule));
                }
            }
            for col in f.cols.clone().filter(|&col| col > 0) {
                window.paint_quad(fill(Bounds { origin: point(col_x(col), body_top), size: size(px(1.0), bottom - body_top) }, c.rule));
            }
        });
        // The header row: across the top, scrolled sideways with the body.
        let header = Bounds::from_corners(point(body_left, o.y), point(bounds.right(), body_top));
        window.with_content_mask(Some(ContentMask { bounds: header }), |window| {
            window.paint_quad(fill(Bounds::from_corners(point(body_left, o.y), point(right, body_top)), md_view::row_fill(0, c.header, c.paper, c.tint, self.ground)));
            for (j, col) in f.cols.clone().enumerate() {
                let cell = Cell { row: 0, col };
                let cb = Bounds::from_corners(point(col_x(col), o.y), point(col_x(col + 1), body_top));
                let cut = self.paint_cell(cell, cb, &f.header[j], f.numeric[j], FontWeight::SEMIBOLD, size_px, in_sel(cell), f, window, cx);
                tip_cut |= cut && f.tip.as_ref().is_some_and(|(t, _)| *t == cell);
                if col > 0 {
                    window.paint_quad(fill(Bounds { origin: point(col_x(col), o.y), size: size(px(1.0), px(l.header_h)) }, c.rule));
                }
            }
        });
        // The gutter: the row numbers, right-aligned in the text view's family and colour,
        // scrolled down with the body; a row selection tints its numbers too.
        let gutter = Bounds::from_corners(point(o.x, body_top), point(body_left, bounds.bottom()));
        let whole_rows = f.sel.is_some_and(|s| matches!(s.scope, Scope::Rows | Scope::All));
        window.with_content_mask(Some(ContentMask { bounds: gutter }), |window| {
            for r in f.rows.clone() {
                let y = row_y(r);
                let rb = Bounds::from_corners(point(o.x, y), point(body_left, y + px(l.row_h)));
                window.paint_quad(fill(rb, md_view::row_fill(r + 1, c.header, c.paper, c.tint, self.ground)));
                if whole_rows && in_sel(Cell { row: r + 1, col: 0 }) {
                    window.paint_quad(fill(rb, c.link.opacity(0.15)));
                }
                let num: SharedString = (f.first_row + r).to_string().into();
                let run = self.run(&self.mono, num.len(), c.gutter, FontWeight::NORMAL);
                let shaped = window.text_system().shape_line(num, px(f.font_size), &[run], None);
                let x = body_left - px(GUTTER_PAD) - shaped.width;
                let _ = shaped.paint(point(x, y), px(l.row_h), window, cx);
            }
        });
        // The corner, a plain fill, and the lines that pin the header and the gutter.
        let corner = Bounds::from_corners(o, point(body_left, body_top));
        window.paint_quad(fill(corner, md_view::row_fill(0, c.header, c.paper, c.tint, self.ground)));
        window.paint_quad(fill(Bounds { origin: point(o.x, body_top - px(1.0)), size: size(right - o.x, px(1.0)) }, c.outline));
        window.paint_quad(fill(Bounds { origin: point(body_left - px(1.0), o.y), size: size(px(1.0), bottom - o.y) }, c.outline));
        if let Some((_, text)) = f.tip.as_ref().filter(|_| tip_cut) {
            self.paint_tip(text, bounds, size_px, window, cx);
        }
    }

    /// One cell: the selection's tint, a find hit's, the text cut to the column with an ellipsis
    /// (right-aligned in a number column) and the head's ring. True when the text was cut.
    #[allow(clippy::too_many_arguments)]
    fn paint_cell(&self, cell: Cell, cb: Bounds<Pixels>, text: &Text, numeric: bool, weight: FontWeight, size_px: Pixels, selected: bool, f: &Frame, window: &mut Window, cx: &mut App) -> bool {
        let c = &self.colors;
        if selected {
            window.paint_quad(fill(cb, c.link.opacity(0.15)));
        }
        if let Some((_, current)) = f.hits.iter().find(|(h, _)| *h == cell) {
            window.paint_quad(fill(cb, (if *current { c.find_current } else { c.find_match }).opacity(HIT_ALPHA)));
        }
        let inner = f32::from(cb.size.width) - 2.0 * CELL_PAD_X - 1.0;
        let mut cut = text.cut;
        if !text.shown.is_empty() && inner > 0.0 {
            let run = |len| self.run(&self.prose, len, c.text, weight);
            let mut shaped = window.text_system().shape_line(text.shown.clone().into(), size_px, &[run(text.shown.len())], None);
            if text.cut || f32::from(shaped.width) > inner {
                cut = true;
                let ell = window.text_system().shape_line("\u{2026}".into(), size_px, &[run("\u{2026}".len())], None);
                let room = px((inner - f32::from(ell.width)).max(0.0));
                let end = if shaped.width <= room { text.shown.len() } else { shaped.index_for_x(room).unwrap_or(text.shown.len()) };
                let end = text.shown.floor_char_boundary(end);
                let kept = format!("{}\u{2026}", text.shown[..end].trim_end());
                shaped = window.text_system().shape_line(kept.clone().into(), size_px, &[run(kept.len())], None);
            }
            let x = if numeric { cb.right() - px(CELL_PAD_X + 1.0) - shaped.width } else { cb.left() + px(CELL_PAD_X + 1.0) };
            let lh = cb.size.height - px(2.0 * CELL_PAD_Y);
            let _ = shaped.paint(point(x, cb.top() + px(CELL_PAD_Y)), lh, window, cx);
        }
        if f.sel.is_some_and(|s| s.head == cell) {
            let ring = Bounds { origin: cb.origin + point(px(1.0), px(1.0)), size: cb.size - size(px(1.0), px(1.0)) };
            window.paint_quad(quad(ring, px(0.0), gpui::transparent_black(), px(RING_W), c.link, BorderStyle::Solid));
        }
        cut
    }

    /// The full text of a cut cell in a box at the pointer, in the pill's colours, kept inside
    /// the grid.
    fn paint_tip(&self, text: &str, bounds: Bounds<Pixels>, size_px: Pixels, window: &mut Window, cx: &mut App) {
        let c = &self.colors;
        let lines = tip_lines(text);
        let line_h = (f32::from(size_px) * 1.3).ceil();
        let shaped: Vec<ShapedLine> = lines
            .iter()
            .map(|t| {
                let run = self.run(&self.prose, t.len(), c.tip_text, FontWeight::NORMAL);
                window.text_system().shape_line(t.clone().into(), size_px, &[run], None)
            })
            .collect();
        let w = shaped.iter().map(|s| f32::from(s.width)).fold(0.0, f32::max) + 2.0 * TIP_PAD_X;
        let h = line_h * shaped.len() as f32 + 2.0 * TIP_PAD_Y;
        let at = window.mouse_position();
        let x = (at.x + px(TIP_OFFSET)).min(bounds.right() - px(w)).max(bounds.left());
        let y = (at.y + px(TIP_OFFSET)).min(bounds.bottom() - px(h)).max(bounds.top());
        let tip = Bounds { origin: point(x, y), size: size(px(w), px(h)) };
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(quad(tip, px(TIP_RADIUS), c.tip_bg, px(1.0), c.tip_border, BorderStyle::Solid));
            for (i, s) in shaped.iter().enumerate() {
                let _ = s.paint(point(x + px(TIP_PAD_X), y + px(TIP_PAD_Y + i as f32 * line_h)), px(line_h), window, cx);
            }
        });
    }

    /// The grid takes the mouse: a left press selects (Shift extends), a move with it held drags
    /// the selection, the release ends it, and a move without it is the tooltip's hover.
    fn listen(&self, hitbox: Hitbox, bounds: Bounds<Pixels>, window: &mut Window, _cx: &mut App) {
        let origin = bounds.origin;
        let local = move |p: Point<Pixels>| point(f32::from(p.x - origin.x), f32::from(p.y - origin.y));
        let v = self.viewer.clone();
        let hb = hitbox.clone();
        window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && ev.button == MouseButton::Left && hb.is_hovered(window) {
                v.update(cx, |v, cx| {
                    let hit = v.grid_hit(local(ev.position));
                    v.grid_press(hit, ev.modifiers.shift, cx);
                });
            }
        });
        // A right press selects the cell under it, if it is outside the selection, before the
        // pane opens its menu (a child's listener hears the press first on the way up).
        let v = self.viewer.clone();
        let hb = hitbox.clone();
        window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && ev.button == MouseButton::Right && hb.is_hovered(window) {
                v.update(cx, |v, cx| {
                    let hit = v.grid_hit(local(ev.position));
                    v.grid_right_press(hit, cx);
                });
            }
        });
        let v = self.viewer.clone();
        window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let over = hitbox.is_hovered(window);
            v.update(cx, |v, cx| {
                let hit = v.grid_hit(local(ev.position));
                if ev.pressed_button == Some(MouseButton::Left) {
                    v.grid_drag(hit, cx);
                }
                let cell = match hit {
                    Hit::Cell(c) if over => Some(c),
                    Hit::Header(col) if over => Some(Cell { row: 0, col }),
                    _ => None,
                };
                v.grid_hover_at(cell, cx);
            });
        });
        let v = self.viewer.clone();
        window.on_mouse_event(move |ev: &MouseUpEvent, phase, _window, cx| {
            if phase == DispatchPhase::Bubble && ev.button == MouseButton::Left {
                v.update(cx, |v, _| v.grid_release());
            }
        });
    }
}

/// Added to the viewer's key for the grid's shapes, apart from any markdown table's.
const GRID_KEY: u64 = 0x6772_6964;

impl IntoElement for GridElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

/// What a key does in the grid (spec 5, Selection).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridKey {
    /// An arrow: a one-cell selection a step from the head.
    Move(isize, isize),
    /// Shift and an arrow: the head moves, the anchor stays. With Cmd as well the step is
    /// `isize::MAX` or `isize::MIN`, which is the sheet's edge.
    Extend(isize, isize),
    /// Cmd and an arrow: the head jumps to the edge the sign points at.
    Edge(isize, isize),
    RowStart,
    RowEnd,
    PageUp,
    PageDown,
    SelectAll,
    Copy,
    Escape,
}

/// The grid key a keystroke is, `None` for anything else (Cmd+X and Cmd+V among them).
pub fn key_of(ks: &Keystroke) -> Option<GridKey> {
    let m = ks.modifiers;
    if m.control || m.alt {
        return None;
    }
    let (dr, dc) = match ks.key.as_str() {
        "up" => (-1, 0),
        "down" => (1, 0),
        "left" => (0, -1),
        "right" => (0, 1),
        _ => (0, 0),
    };
    if (dr, dc) != (0, 0) {
        let far = |d: isize| match d.signum() {
            -1 => isize::MIN,
            1 => isize::MAX,
            _ => 0,
        };
        return Some(match (m.platform, m.shift) {
            (false, false) => GridKey::Move(dr, dc),
            (false, true) => GridKey::Extend(dr, dc),
            (true, false) => GridKey::Edge(dr, dc),
            (true, true) => GridKey::Extend(far(dr), far(dc)),
        });
    }
    match (ks.key.as_str(), m.platform) {
        ("home", false) => Some(GridKey::RowStart),
        ("end", false) => Some(GridKey::RowEnd),
        ("pageup", false) => Some(GridKey::PageUp),
        ("pagedown", false) => Some(GridKey::PageDown),
        ("a", true) => Some(GridKey::SelectAll),
        ("c", true) => Some(GridKey::Copy),
        ("escape", false) => Some(GridKey::Escape),
        _ => None,
    }
}

/// Whether the grid takes the keystroke: its keys, and Cmd+X and Cmd+V, which do nothing in a
/// read-only grid and must not reach anything else.
pub fn handles(ks: &Keystroke) -> bool {
    key_of(ks).is_some() || (ks.modifiers.platform && matches!(ks.key.as_str(), "x" | "v"))
}

/// The selection after `key`. With nothing selected the movement keys select the first body cell
/// (the header is row 0 and is reached by the mouse or by Up). `page` is the rows a page moves.
/// Escape gives `None`, Copy the selection as it was.
pub fn apply_key(key: GridKey, sel: Option<&Selection>, sheet: &Sheet, page: usize) -> Option<Selection> {
    let last = Cell { row: sheet.rows.len(), col: sheet.cols().saturating_sub(1) };
    let step = |v: usize, d: isize, max: usize| match d {
        isize::MAX => max,
        isize::MIN => 0,
        d => v.saturating_add_signed(d).min(max),
    };
    let edge = |v: usize, d: isize, max: usize| match d.signum() {
        -1 => 0,
        1 => max,
        _ => v,
    };
    let page = isize::try_from(page.max(1)).unwrap_or(isize::MAX);
    match key {
        GridKey::Escape => None,
        GridKey::Copy => sel.copied(),
        GridKey::SelectAll => {
            let origin = Cell { row: 0, col: 0 };
            Some(Selection { anchor: origin, head: origin, scope: Scope::All })
        }
        // A page key with nothing selected is for the pane's scroll keys.
        GridKey::PageUp | GridKey::PageDown if sel.is_none() => None,
        _ => {
            let Some(s) = sel else { return Some(Selection::cell(Cell { row: 1, col: 0 }).clamp(sheet)) };
            let h = s.head;
            let to = |row, col| Cell { row, col };
            Some(match key {
                GridKey::Move(dr, dc) => s.move_by(dr, dc, sheet),
                GridKey::Extend(dr, dc) => s.extend(to(step(h.row, dr, last.row), step(h.col, dc, last.col))),
                GridKey::Edge(dr, dc) => Selection::cell(to(edge(h.row, dr, last.row), edge(h.col, dc, last.col))),
                GridKey::RowStart => Selection::cell(to(h.row, 0)),
                GridKey::RowEnd => Selection::cell(to(h.row, last.col)),
                GridKey::PageUp => s.move_by(-page, 0, sheet),
                GridKey::PageDown => s.move_by(page, 0, sheet),
                GridKey::Escape | GridKey::Copy | GridKey::SelectAll => unreachable!("handled above"),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Sheet;
    use crate::viewer_element::GUTTER_PAD;
    use gpui::point;

    fn sheet(cols: usize, rows: usize) -> Sheet {
        let header = (0..cols).map(|c| format!("h{c}")).collect();
        let body = (0..rows).map(|r| (0..cols).map(|c| format!("{r}-{c}")).collect()).collect();
        Sheet::cap("s".into(), header, body)
    }

    fn layout() -> Layout {
        // A gutter of 40, a header and rows of 20, three columns 100 wide, in a view 240 by 100:
        // the body is 200 wide and 80 tall.
        Layout { gutter_w: 40.0, header_h: 20.0, row_h: 20.0, cols: vec![0.0, 100.0, 200.0, 300.0], view_w: 240.0, view_h: 100.0 }
    }

    fn key(s: &str) -> Option<GridKey> {
        key_of(&gpui::Keystroke::parse(s).unwrap())
    }

    #[test]
    fn each_key_maps_to_its_grid_move() {
        assert_eq!(key("left"), Some(GridKey::Move(0, -1)));
        assert_eq!(key("right"), Some(GridKey::Move(0, 1)));
        assert_eq!(key("up"), Some(GridKey::Move(-1, 0)));
        assert_eq!(key("down"), Some(GridKey::Move(1, 0)));
        assert_eq!(key("shift-left"), Some(GridKey::Extend(0, -1)));
        assert_eq!(key("shift-down"), Some(GridKey::Extend(1, 0)));
        assert_eq!(key("cmd-up"), Some(GridKey::Edge(-1, 0)));
        assert_eq!(key("cmd-right"), Some(GridKey::Edge(0, 1)));
        assert_eq!(key("cmd-shift-down"), Some(GridKey::Extend(isize::MAX, 0)));
        assert_eq!(key("cmd-shift-left"), Some(GridKey::Extend(0, isize::MIN)));
        assert_eq!(key("home"), Some(GridKey::RowStart));
        assert_eq!(key("end"), Some(GridKey::RowEnd));
        assert_eq!(key("pageup"), Some(GridKey::PageUp));
        assert_eq!(key("pagedown"), Some(GridKey::PageDown));
        assert_eq!(key("cmd-a"), Some(GridKey::SelectAll));
        assert_eq!(key("cmd-c"), Some(GridKey::Copy));
        assert_eq!(key("escape"), Some(GridKey::Escape));
        assert_eq!(key("a"), None);
        assert_eq!(key("space"), None);
        assert_eq!(key("cmd-x"), None);
        assert_eq!(key("cmd-v"), None);
    }

    #[test]
    fn apply_key_moves_extends_jumps_to_edges_and_pages_within_the_sheet() {
        let s = sheet(3, 10);
        let c = |row, col| Cell { row, col };
        let at = |row, col| Some(Selection::cell(c(row, col)));
        let from = at(5, 1);
        assert_eq!(apply_key(GridKey::Move(1, 1), from.as_ref(), &s, 4), at(6, 2));
        assert_eq!(apply_key(GridKey::Move(0, 5), from.as_ref(), &s, 4), at(5, 2), "clamped to the last column");
        assert_eq!(apply_key(GridKey::Move(-9, 0), from.as_ref(), &s, 4), at(0, 1), "Up reaches the header");
        assert_eq!(apply_key(GridKey::Extend(2, 0), from.as_ref(), &s, 4), Some(Selection { anchor: c(5, 1), head: c(7, 1), scope: Scope::Cells }));
        assert_eq!(apply_key(GridKey::Edge(-1, 0), from.as_ref(), &s, 4), at(0, 1));
        assert_eq!(apply_key(GridKey::Edge(1, 0), from.as_ref(), &s, 4), at(10, 1));
        assert_eq!(apply_key(GridKey::Edge(0, -1), from.as_ref(), &s, 4), at(5, 0));
        assert_eq!(apply_key(GridKey::Edge(0, 1), from.as_ref(), &s, 4), at(5, 2));
        assert_eq!(apply_key(GridKey::Extend(isize::MAX, 0), from.as_ref(), &s, 4), Some(Selection { anchor: c(5, 1), head: c(10, 1), scope: Scope::Cells }));
        assert_eq!(apply_key(GridKey::Extend(0, isize::MIN), from.as_ref(), &s, 4), Some(Selection { anchor: c(5, 1), head: c(5, 0), scope: Scope::Cells }));
        assert_eq!(apply_key(GridKey::RowStart, from.as_ref(), &s, 4), at(5, 0));
        assert_eq!(apply_key(GridKey::RowEnd, from.as_ref(), &s, 4), at(5, 2));
        assert_eq!(apply_key(GridKey::PageDown, from.as_ref(), &s, 4), at(9, 1));
        assert_eq!(apply_key(GridKey::PageDown, at(8, 1).as_ref(), &s, 4), at(10, 1), "clamped to the last row");
        assert_eq!(apply_key(GridKey::PageUp, from.as_ref(), &s, 4), at(1, 1));
        let all = apply_key(GridKey::SelectAll, from.as_ref(), &s, 4).unwrap();
        assert_eq!((all.scope, all.anchor), (Scope::All, c(0, 0)));
        assert_eq!(apply_key(GridKey::Copy, from.as_ref(), &s, 4), from);
        assert_eq!(apply_key(GridKey::Escape, from.as_ref(), &s, 4), None);
    }

    #[test]
    fn the_first_arrow_without_a_selection_selects_the_first_body_cell() {
        let s = sheet(3, 10);
        let first = Some(Selection::cell(Cell { row: 1, col: 0 }));
        for k in [GridKey::Move(0, 1), GridKey::Extend(1, 0), GridKey::Edge(1, 0), GridKey::RowEnd, GridKey::RowStart] {
            assert_eq!(apply_key(k, None, &s, 4), first, "{k:?}");
        }
        assert_eq!(apply_key(GridKey::Escape, None, &s, 4), None);
        assert_eq!(apply_key(GridKey::Copy, None, &s, 4), None);    }

    #[test]
    fn a_page_key_without_a_selection_is_left_for_the_scroll_keys() {
        let s = sheet(3, 10);
        assert_eq!(apply_key(GridKey::PageDown, None, &s, 4), None);
        assert_eq!(apply_key(GridKey::PageUp, None, &s, 4), None);
    }

    #[test]
    fn column_widths_cap_at_45_percent_and_floor_at_three_characters() {
        let floor = 3.0 * 8.0 + 2.0 * CELL_PAD_X;
        let w = column_widths(&[50.0, 900.0, 10.0], 1000.0, floor);
        assert_eq!(w[0], 50.0, "a narrow column keeps its natural width");
        assert_eq!(w[1], 1000.0 * COLUMN_CAP, "a wide one stops at the cap");
        assert_eq!(w[2], floor, "an empty one is still three characters wide");
        // In a pane too small for the cap to hold three characters, the floor wins.
        let w = column_widths(&[500.0], 50.0, floor);
        assert_eq!(w, vec![floor]);
        assert!(column_widths(&[], 400.0, floor).is_empty());
    }

    #[test]
    fn row_range_covers_the_viewport_plus_one_and_stops_at_the_end() {
        assert_eq!(row_range(0.0, 100.0, 20.0, 1000), 0..6, "five rows fit, plus one");
        assert_eq!(row_range(10.5, 100.0, 20.0, 1000), 10..16, "a fractional scroll starts at its row");
        assert_eq!(row_range(998.0, 100.0, 20.0, 1000), 998..1000, "clamped at the last row");
        assert_eq!(row_range(0.0, 100.0, 20.0, 0), 0..0, "a sheet with only a header");
        assert_eq!(row_range(0.0, 0.0, 20.0, 10), 0..1, "a pane of no height still asks for one");
        assert_eq!(row_range(-3.0, 100.0, 20.0, 10), 0..6, "a scroll past the top starts at the first row");
        // A fractional scroll and a fractional fit together reach one row further down.
        assert_eq!(row_range(0.9, 90.0, 20.0, 1000), 0..6, "rows 0 to 5 are in sight from 0.9 to 5.4");
        assert_eq!(row_range(2.0, 100.0, 20.0, 1000), 2..8, "an exact fit: five rows from row 2, plus one");
        assert_eq!(row_range(0.9, 500.0, 32.0, 1000), 0..17, "a body 500 tall in rows of 32 at 0.9");
    }

    #[test]
    fn visible_prefix_takes_the_first_line_and_cuts_a_long_cell() {
        assert_eq!(visible_prefix("plain"), ("plain", false));
        assert_eq!(visible_prefix(""), ("", false));
        assert_eq!(visible_prefix("first\nsecond"), ("first", true), "a line break ends what shows");
        assert_eq!(visible_prefix("first\r\nsecond"), ("first", true));
        let long = "\u{e9}".repeat(MAX_CELL_CHARS + 10);
        let (shown, cut) = visible_prefix(&long);
        assert!(cut);
        assert_eq!(shown.chars().count(), MAX_CELL_CHARS, "cut by chars, on a char boundary");
        let exact = "a".repeat(MAX_CELL_CHARS);
        assert_eq!(visible_prefix(&exact), (exact.as_str(), false), "at the limit nothing is cut");
    }

    #[test]
    fn hit_finds_the_corner_the_header_the_gutter_a_cell_and_outside_under_scroll() {
        let s = sheet(3, 10);
        let l = layout();
        let at = |x: f32, y: f32| point(x, y);
        assert_eq!(hit(at(10.0, 10.0), &l, 0.0, 0.0, &s), Hit::Corner);
        assert_eq!(hit(at(50.0, 10.0), &l, 0.0, 0.0, &s), Hit::Header(0));
        assert_eq!(hit(at(150.0, 10.0), &l, 0.0, 0.0, &s), Hit::Header(1));
        assert_eq!(hit(at(10.0, 25.0), &l, 0.0, 0.0, &s), Hit::RowNumber(1), "the first body row is grid row 1");
        assert_eq!(hit(at(50.0, 25.0), &l, 0.0, 0.0, &s), Hit::Cell(Cell { row: 1, col: 0 }));
        assert_eq!(hit(at(150.0, 65.0), &l, 0.0, 0.0, &s), Hit::Cell(Cell { row: 3, col: 1 }));
        // Scrolled two and a half rows down and 120 points right: the header and the gutter stay.
        assert_eq!(hit(at(50.0, 10.0), &l, 2.5, 120.0, &s), Hit::Header(1), "the header moves sideways with the body");
        assert_eq!(hit(at(10.0, 25.0), &l, 2.5, 120.0, &s), Hit::RowNumber(3), "the gutter moves down with it");
        assert_eq!(hit(at(50.0, 25.0), &l, 2.5, 120.0, &s), Hit::Cell(Cell { row: 3, col: 1 }));
        assert_eq!(hit(at(50.0, 35.0), &l, 2.5, 120.0, &s), Hit::Cell(Cell { row: 4, col: 1 }), "past the half row");
        assert_eq!(hit(at(190.0, 10.0), &l, 0.0, 120.0, &s), Hit::Header(2));
        // Past the last column or row, or off the element.
        assert_eq!(hit(at(230.0, 30.0), &l, 0.0, 200.0, &s), Hit::Outside, "right of the last column");
        assert_eq!(hit(at(50.0, 90.0), &l, 9.0, 0.0, &s), Hit::Outside, "under the last row");
        assert_eq!(hit(at(10.0, 90.0), &l, 9.0, 0.0, &s), Hit::Outside, "the gutter under the last row");
        assert_eq!(hit(at(-1.0, 10.0), &l, 0.0, 0.0, &s), Hit::Outside);
        assert_eq!(hit(at(50.0, 100.0), &l, 0.0, 0.0, &s), Hit::Outside);
    }

    #[test]
    fn gutter_width_grows_with_the_last_row_number() {
        assert_eq!(gutter_width(9, 10.0), 10.0 + GUTTER_PAD * 2.0);
        assert_eq!(gutter_width(10, 10.0), 20.0 + GUTTER_PAD * 2.0);
        assert_eq!(gutter_width(200_001, 8.0), 6.0 * 8.0 + GUTTER_PAD * 2.0);
        assert!(gutter_width(1, 8.0) > 0.0, "a sheet with only a header still has a gutter");
    }

    #[test]
    fn a_tooltip_wraps_at_sixty_characters_and_stops_at_eight_lines() {
        assert_eq!(tip_lines("short"), vec!["short"]);
        assert_eq!(tip_lines("one\ntwo"), vec!["one", "two"], "a cell's own line breaks are kept");
        let words = "word ".repeat(30);
        let lines = tip_lines(words.trim_end());
        assert!(lines.iter().all(|l| l.chars().count() <= 60), "{lines:?}");
        assert!(lines.iter().all(|l| !l.starts_with(' ') && !l.ends_with(' ')), "broken at spaces");
        let long = "x".repeat(130);
        assert_eq!(tip_lines(&long).iter().map(|l| l.chars().count()).collect::<Vec<_>>(), vec![60, 60, 10], "a word longer than the width is cut");
        let many = (0..20).map(|i| i.to_string()).collect::<Vec<_>>().join("\n");
        let lines = tip_lines(&many);
        assert_eq!(lines.len(), 8);
        assert_eq!(lines[7], "7\u{2026}", "the last line says more was left out");
        let huge = "y".repeat(MAX_CELL_CHARS * 3);
        assert!(tip_lines(&huge).concat().chars().filter(|&c| c == 'y').count() <= MAX_CELL_CHARS);
    }
}
