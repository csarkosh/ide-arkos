//! Paints a `Terminal`'s visible rows at a fractional line offset (spec 3). Only the rows in the
//! viewport plus one above are shaped and painted, which is the virtualization.
use crate::layout::{build_runs, CellKind, CellStyle};
use crate::palette::Rgb;
use crate::effects::decode::{Decode, Timing};
use crate::terminal::{CellMetrics, RowsForPaint, Terminal};
use crate::links::{span_at, Span};
use crate::terminal::HoveredLink;
use gpui::{
    fill, font, point, px, size, App, Bounds, ContentMask, CursorStyle, Element, ElementId, Entity, FontWeight, GlobalElementId, Hitbox,
    HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, Pixels, Rgba, SharedString, Style, TextRun, Window,
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
    /// The alpha of the terminal's paper: 1 without an effect, the effect's `paper_alpha` under one.
    paper_alpha: f32,
    /// How changed cells decode under an effect that decodes the terminal; None without one.
    decode: Option<Timing>,
}

impl TerminalElement {
    pub fn new(terminal: Entity<Terminal>, font_size: f32, line_height: f32, family: SharedString, paper_alpha: f32, decode: Option<Timing>) -> Self {
        Self { terminal, font_size, line_height, family, paper_alpha, decode }
    }
}

/// The visible rows as the decode draws them. With a timing, the decode state (made here when it
/// is missing) takes in the rows and each cell shows what the decode says; without one, the
/// state is dropped and None says to draw `rows.visible` as it is, so the rows are copied only
/// while an effect decodes. The row above the screen (`rows.above`) never decodes.
pub fn decoded_rows(
    decode: &mut Option<Decode>,
    rows: &RowsForPaint,
    timing: Option<Timing>,
    cols: usize,
    now: Instant,
) -> Option<Vec<Vec<(char, CellStyle, CellKind)>>> {
    let Some(timing) = timing else {
        *decode = None;
        return None;
    };
    let state = decode.get_or_insert_with(|| Decode::new(cols, rows.visible.len(), rows.display_offset));
    state.observe(&rows.visible, rows.display_offset, now);
    let shown = rows
        .visible
        .iter()
        .enumerate()
        .map(|(row, line)| {
            line.iter()
                .enumerate()
                .map(|(col, &cell)| {
                    let (ch, style) = state.shown(col, row, cell, now, timing);
                    (ch, style, cell.2)
                })
                .collect()
        })
        .collect();
    Some(shown)
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
    /// The element's area, for the pointer cursor over a link.
    hitbox: Hitbox,
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
            t.origin = Some((f32::from(bounds.origin.x), f32::from(bounds.origin.y)));
            t.ensure_size(cols, rows);
        });
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        Prepaint { cell, hitbox }
    }

    fn paint(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _state: &mut (), prepaint: &mut Prepaint, window: &mut Window, cx: &mut App) {
        let cell = prepaint.cell;
        let lh = px(cell.height);
        let cw = cell.width;
        // Advance the scroll animation by wall-clock time, then split into integer offset + fraction.
        // The colours come from the terminal's palette, which follows the theme.
        let mouse = window.mouse_position();
        let decode = self.decode;
        let (rows, visible, frac, cursor, animating, highlights, selection, palette, find_owed) = self.terminal.update(cx, |t, _| {
            // vte only checks the synchronized update deadline when bytes arrive, so a program
            // killed inside one would freeze the grid. Releasing it here means this frame paints
            // the released rows; while one is still open, keep frames coming to check again.
            t.core.flush_sync_if_due();
            let sync_open = t.core.sync_pending();
            let now = Instant::now();
            let dt = frame_dt(t.last_frame, now);
            // A press held with the pointer above or below the element scrolls the history that
            // way and drags the selection along. It is decided here, from the pointer's position
            // each frame, because the pane hears no moves once the pointer has left it. The step
            // runs before the tick and the split, so the rows, their fraction and the selection
            // painted below agree.
            let drag_scrolling = t.clicks.held() && {
                let (x, y) = (f32::from(mouse.x), f32::from(mouse.y));
                let (top, bottom) = (f32::from(bounds.top()), f32::from(bounds.bottom()));
                let rate = crate::mouse::drag_scroll_rate(y, top, bottom, cell.height);
                let origin = (f32::from(bounds.origin.x), top);
                let col = crate::mouse::cell_under(x, top, origin, (cell.width, cell.height), 0.0, t.core.columns(), t.core.screen_lines()).0;
                rate != 0.0 && t.drag_scroll_step(rate, dt, col)
            };
            let animating = t.core.scroll.tick(dt);
            // Once settled, forget the frame time: the next scroll after idle starts from one
            // nominal frame instead of the capped gap, which would jump most of the distance.
            t.last_frame = if animating || sync_open || drag_scrolling { Some(now) } else { None };
            let (off, frac) = split_offset(t.core.scroll.current);
            t.core.set_display_offset(off);
            t.frac = frac;
            let find_owed = t.refresh_find();
            let highlights = t.find.as_ref().map(|f| t.core.highlights(&f.matches, f.current)).unwrap_or_default();
            let palette = t.core.palette();
            // The decode observes once per paint; under a decoding effect the window's glyph
            // timer supplies the frames that carry a changed cell through its scramble.
            let rows = t.core.rows_for_paint();
            let visible = decoded_rows(&mut t.decode, &rows, decode, t.core.columns(), now);
            (rows, visible, frac, t.core.cursor(), animating || sync_open || drag_scrolling, highlights, t.core.selection_spans(), palette.clone(), find_owed)
        });
        let (bg, cursor_color) = (palette.bg, palette.cursor);
        // Under an effect the wash shows faintly through the paper.
        let mut paper = hsla(bg);
        paper.a = self.paper_alpha;
        window.paint_quad(fill(bounds, paper));
        // Every link in the visible rows, a URL wrapped across rows among them: underlined below,
        // and the one under the pointer opened by Cmd+click in the pane's mouse handler, which
        // reads it back from the terminal.
        let hovered = hovered_link(&rows.links, bounds, window.mouse_position(), cell, frac);
        if hovered.is_some() {
            window.set_cursor_style(CursorStyle::PointingHand, &prepaint.hitbox);
        }
        self.terminal.update(cx, |t, _| t.hovered_link = hovered.clone());

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
            for (i, row) in visible.as_ref().unwrap_or(&rows.visible).iter().enumerate() {
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
            // The selection: a translucent tint over the text, like the find matches below.
            for s in &selection {
                let x = origin.x + px(s.start as f32 * cw);
                let w = px((s.end + 1 - s.start) as f32 * cw);
                let y = origin.y + px((s.row as f32 + frac) * cell.height);
                window.paint_quad(fill(Bounds { origin: point(x, y), size: size(w, lh) }, palette.selection.alpha(0.4)));
            }
            // Find matches: translucent quads over the text, so no run is re-shaped.
            for h in &highlights {
                let x = origin.x + px(h.start as f32 * cw);
                let w = px((h.end + 1 - h.start) as f32 * cw);
                let y = origin.y + px((h.row as f32 + frac) * cell.height);
                let c = if h.current { palette.find_current } else { palette.find_match };
                window.paint_quad(fill(Bounds { origin: point(x, y), size: size(w, lh) }, c.alpha(0.45)));
            }
            // Every link wears an underline in its own colour, so it reads as a link before the
            // pointer finds it; the hovered one also gets the pointer cursor (above).
            for &Span { row, start, end, .. } in &rows.links {
                let x = origin.x + px(start as f32 * cw);
                let w = px((end - start) as f32 * cw);
                let y = origin.y + px((row as f32 + frac) * cell.height) + lh - px(2.0);
                let fg = rows.visible.get(row).and_then(|r| r.get(start)).map(|c| c.1.fg).unwrap_or(palette.fg);
                window.paint_quad(fill(Bounds { origin: point(x, y), size: size(w, px(1.0)) }, hsla(fg)));
            }
        });

        // A throttled find search runs in a later frame, even once the output has stopped.
        if animating || find_owed {
            window.request_animation_frame();
        }
    }
}

/// The link under `mouse` among `spans`, painted at `frac`, if any: the span under the pointer
/// with the whole URL, wrapped rows included.
pub fn hovered_link(spans: &[Span], bounds: Bounds<Pixels>, mouse: gpui::Point<Pixels>, cell: CellMetrics, frac: f32) -> Option<HoveredLink> {
    if !bounds.contains(&mouse) || cell.width <= 0.0 || cell.height <= 0.0 {
        return None;
    }
    let col = (f32::from(mouse.x - bounds.origin.x) / cell.width).floor();
    let row = (f32::from(mouse.y - bounds.origin.y) / cell.height - frac).floor();
    if col < 0.0 || row < 0.0 {
        return None;
    }
    let s = span_at(spans, row as usize, col as usize)?;
    Some(HoveredLink { row: s.row, start: s.start, end: s.end, url: s.url.clone() })
}

#[cfg(test)]
mod tests {
    use super::{frame_dt, hovered_link, split_offset};
    use crate::terminal::CellMetrics;
    use gpui::{point, px, size, Bounds};

    #[test]
    fn the_hovered_link_follows_the_pointer_cell_and_the_fraction() {
        let rows: Vec<Vec<char>> = ["plain", "see https://csarko.sh now"].iter().map(|r| r.chars().collect()).collect();
        let bounds = Bounds { origin: point(px(100.0), px(50.0)), size: size(px(800.0), px(600.0)) };
        let cell = CellMetrics { width: 10.0, height: 20.0 };
        // Row 1, column 4: the first link cell.
        let spans = crate::links::links_in_rows(&rows, &[false, false]);
        let at = |x: f32, y: f32, frac: f32| hovered_link(&spans, bounds, point(px(100.0 + x), px(50.0 + y)), cell, frac).map(|l| (l.row, l.start, l.end));
        assert_eq!(at(45.0, 25.0, 0.0), Some((1, 4, 21)));
        assert_eq!(at(35.0, 25.0, 0.0), None, "the space before it");
        assert_eq!(at(45.0, 5.0, 0.0), None, "row 0 has no link");
        assert_eq!(at(45.0, 45.0, 0.5), Some((1, 4, 21)), "a half-line scroll shifts the rows down");
        assert_eq!(hovered_link(&spans, bounds, point(px(10.0), px(10.0)), cell, 0.0), None, "outside the element");
    }

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
    fn the_rows_are_drawn_as_the_decode_says_and_the_state_goes_when_the_effect_does() {
        use super::decoded_rows;
        use crate::effects::decode;
        use crate::layout::{CellKind, CellStyle};
        use crate::palette::Rgb;
        use crate::terminal::RowsForPaint;
        let t0 = Instant::now();
        let plain = CellStyle { fg: Rgb { r: 74, g: 246, b: 38 }, bg: Rgb { r: 0, g: 0, b: 0 }, bold: false, italic: false, dim: false, underline: false, inverse: false };
        let rows = |s: &str| RowsForPaint { display_offset: 0, above: None, visible: vec![s.chars().map(|c| (c, plain, CellKind::Normal)).collect()], links: vec![] };
        let mut state = None;
        let t = Some(decode::timing(true, false));
        let first = decoded_rows(&mut state, &rows("   "), t, 3, t0).expect("a decoding effect draws its own rows");
        assert!(state.is_some());
        assert_eq!(first[0][0].0, ' ');
        let second = decoded_rows(&mut state, &rows("ok "), t, 3, t0 + Duration::from_millis(20)).unwrap();
        assert_ne!(second[0][0].0, 'o', "a changed cell scrambles");
        assert!(decoded_rows(&mut state, &rows("ok "), None, 3, t0 + Duration::from_millis(30)).is_none(), "no effect: the rows are drawn as they are");
        assert!(state.is_none(), "no effect: the state is dropped");

        // A wide character keeps its kind, and its spacer stays a spacer, while it decodes; the
        // row above the screen is never touched.
        let above = vec![('x', plain, CellKind::Normal), ('y', plain, CellKind::Normal)];
        let wide = |c: char| RowsForPaint {
            display_offset: 0,
            above: Some(above.clone()),
            visible: vec![vec![(c, plain, CellKind::Wide), (' ', plain, CellKind::Spacer)]],
            links: vec![],
        };
        decoded_rows(&mut state, &wide(' '), t, 2, t0);
        let fresh = wide('漢');
        let shown = decoded_rows(&mut state, &fresh, t, 2, t0 + Duration::from_millis(20)).unwrap();
        assert_ne!(shown[0][0].0, '漢', "the wide character scrambles");
        assert_eq!((shown[0][0].2, shown[0][1].2), (CellKind::Wide, CellKind::Spacer));
        assert_eq!(shown[0][1].0, ' ');
        assert_eq!(shown.len(), 1, "only the visible rows are decoded");
        assert_eq!(fresh.above.as_ref(), Some(&above), "the row above is left alone");
    }

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
