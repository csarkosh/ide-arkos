//! Paints a `Terminal`'s visible rows at a fractional line offset (spec 3). Only the rows in the
//! viewport plus one above are shaped and painted, which is the virtualization.
use crate::layout::{build_runs, build_stamped_runs, CellKind, CellStyle};
use crate::palette::Rgb;
use crate::effects::decode::{Decode, Timing};
use crate::effects::paint;
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
    /// What an effect does to the cells the program writes; None without one.
    ink: Option<Ink>,
    /// The window draw this paint is part of (`Root::draws`).
    draw: u64,
}

/// What an effect does to the cells the program writes: decodes them as runic does, with the
/// decode's timing, or paints them in as watercolor does, with the glyph clock at this frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Ink {
    Decode(Timing),
    Paint(f32),
}

impl TerminalElement {
    pub fn new(terminal: Entity<Terminal>, font_size: f32, line_height: f32, family: SharedString, paper_alpha: f32, ink: Option<Ink>, draw: u64) -> Self {
        Self { terminal, font_size, line_height, family, paper_alpha, ink, draw }
    }
}

/// The visible rows as the decode draws them. With a timing, the decode state (made here when it
/// is missing) takes in the rows and each cell shows what the decode says; without one, the
/// state is dropped and None says to draw `rows.visible` as it is, so the rows are copied only
/// while an effect decodes. While `quiet` (the user has just typed, pasted or scrolled) a cell
/// that changes settles at once (`Decode::observe`). The row above the screen (`rows.above`)
/// never decodes.
pub fn decoded_rows(
    decode: &mut Option<Decode>,
    rows: &RowsForPaint,
    timing: Option<Timing>,
    quiet: bool,
    cols: usize,
    now: Instant,
) -> Option<Vec<Vec<(char, CellStyle, CellKind)>>> {
    let Some(timing) = timing else {
        *decode = None;
        return None;
    };
    let state = decode.get_or_insert_with(|| Decode::new(cols, rows.visible.len(), rows.display_offset));
    state.observe(&rows.visible, rows.display_offset, now, quiet, timing.decode);
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

/// Each visible row's ghosts, painted under it: the characters its changed cells replaced, or the
/// steady mark a working mark is doodled as, each with its stamp and, for a mark, its style.
pub type Ghosts = Vec<Vec<(char, f32, Option<CellStyle>)>>;

/// The visible rows' glyph stamps under an effect that paints the terminal (`effects::paint`),
/// the ghosts of the characters changed cells replaced (each with its stamp, empty for a row with
/// none), and the instant the newest ink dries. The decode state (made here when it is missing) takes
/// in the rows as it does for the decode. Only the `PAINT_ROWS` rows changed most recently are
/// stamped; any other row, and a row with no wet ink, gets no stamps at all, so it is drawn as
/// ever. A working mark that flipped within `DOODLE_HOLD`, on any row, is hidden and its steady
/// mark doodled under it. `x` is a column's left edge in points from the window's left.
pub fn painted_rows(
    decode: &mut Option<Decode>,
    rows: &RowsForPaint,
    quiet: bool,
    cols: usize,
    x: impl Fn(usize) -> f32,
    clock: f32,
    now: Instant,
) -> (Vec<Vec<f32>>, Ghosts, Option<Instant>) {
    let state = decode.get_or_insert_with(|| Decode::new(cols, rows.visible.len(), rows.display_offset));
    state.observe(&rows.visible, rows.display_offset, now, quiet, std::time::Duration::ZERO);
    let mut dry = None;
    let mut order = std::collections::HashMap::new();
    let changes: Vec<Vec<Option<Instant>>> = rows.visible.iter().enumerate().map(|(row, line)| (0..line.len()).map(|col| state.changed_at(col, row)).collect()).collect();
    // The newest rows: by their latest change, the lowest on screen first at one instant.
    let mut newest: Vec<(Instant, usize)> = changes.iter().enumerate().filter_map(|(row, c)| c.iter().flatten().max().map(|&at| (at, row))).collect();
    newest.sort_unstable_by(|a, b| b.cmp(a));
    let writing: std::collections::HashSet<usize> = newest.iter().take(paint::PAINT_ROWS).map(|&(_, row)| row).collect();
    let (stamps, ghosts) = rows
        .visible
        .iter()
        .zip(changes)
        .enumerate()
        .map(|(row, (line, changed))| {
            let (mut stamps, mut ghosts) = if writing.contains(&row) && changed.iter().any(Option::is_some) {
                let chars: Vec<char> = line.iter().map(|(c, _, _)| *c).collect();
                let was: Vec<char> = (0..line.len()).map(|col| state.was_at(col, row)).collect();
                let (stamps, ghosts, until) = paint::row_stamps(&changed, &chars, &was, &x, clock, now, &mut order);
                dry = dry.max(until);
                match until {
                    None => (Vec::new(), Vec::new()),
                    Some(_) if ghosts.iter().all(|&g| g == 0.0) => (stamps, Vec::new()),
                    Some(_) => (stamps, was.into_iter().zip(ghosts).map(|(c, g)| (c, g, None)).collect()),
                }
            } else {
                (Vec::new(), Vec::new())
            };
            for (col, &(ch, _, _)) in line.iter().enumerate() {
                let Some((mark, style, at)) = state.mark_at(col, row) else { continue };
                let since = now.saturating_duration_since(at).as_secs_f32();
                if since >= paint::DOODLE_HOLD || !paint::holds_doodle(ch) {
                    continue;
                }
                if stamps.is_empty() {
                    stamps = vec![0.0; line.len()];
                }
                if ghosts.is_empty() {
                    ghosts = vec![(' ', 0.0, None); line.len()];
                }
                stamps[col] = paint::encode(clock, paint::PAINT_HIDE);
                ghosts[col] = (mark, paint::encode(clock - since, paint::PAINT_DOODLE), Some(style));
                dry = dry.max(Some(at + std::time::Duration::from_secs_f32(paint::DOODLE_HOLD)));
            }
            (stamps, ghosts)
        })
        .unzip();
    (stamps, ghosts, dry)
}

/// A stretch of one background colour across a row's cells.
#[derive(Debug, PartialEq)]
struct Band {
    col: usize,
    len: usize,
    color: Rgb,
}

/// A row's backgrounds as bands, from each cell's background: every stretch of one colour other
/// than the terminal's own `bg`.
fn bands(cells: impl Iterator<Item = Rgb>, bg: Rgb) -> Vec<Band> {
    let mut out: Vec<Band> = Vec::new();
    for (col, color) in cells.enumerate() {
        if color == bg {
            continue;
        }
        match out.last_mut() {
            Some(b) if b.color == color && b.col + b.len == col => b.len += 1,
            _ => out.push(Band { col, len: 1, color }),
        }
    }
    out
}

/// Whether a terminal last painted in window draw `painted` was left out of a draw since, so is
/// shown again in `draw`: the window draws every visible terminal each time it draws.
fn shown_again(painted: u64, draw: u64) -> bool {
    painted != 0 && draw > painted + 1
}

fn hsla(c: Rgb) -> Hsla {
    Rgba { r: c.r as f32 / 255.0, g: c.g as f32 / 255.0, b: c.b as f32 / 255.0, a: 1.0 }.into()
}

/// How far faint text (SGR 2) fades toward its background.
const DIM_FADE: f32 = 0.4;
/// The contrast with its background faint text keeps however far it would fade, so a theme's
/// grey that is dim already (a hint, Claude Code's status lines) stays readable.
const DIM_MIN_CONTRAST: f64 = 3.0;

/// The alpha faint text of `fg` is drawn at over `bg`: `DIM_FADE` of the way to the background,
/// or as far as keeps `DIM_MIN_CONTRAST`; full for a colour under that contrast already.
fn dim_alpha(fg: Rgb, bg: Rgb) -> f32 {
    let at = |fade: f32| {
        let mix = |f: u8, b: u8| (f32::from(f) + (f32::from(b) - f32::from(f)) * fade).round() as u8;
        crate::palette::contrast(Rgb { r: mix(fg.r, bg.r), g: mix(fg.g, bg.g), b: mix(fg.b, bg.b) }, bg)
    };
    if at(DIM_FADE) >= DIM_MIN_CONTRAST {
        return 1.0 - DIM_FADE;
    }
    let (mut lo, mut hi) = (0.0, DIM_FADE);
    for _ in 0..10 {
        let mid = (lo + hi) / 2.0;
        if at(mid) >= DIM_MIN_CONTRAST { lo = mid } else { hi = mid }
    }
    1.0 - lo
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
        color.a = dim_alpha(style.fg, style.bg);
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
        let ink = self.ink;
        let (rows, visible, (stamps, ghosts), dry, frac, cursor, animating, highlights, selection, palette, find_owed) = self.terminal.update(cx, |t, _| {
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
            // A terminal the window drew without (its tab was hidden) takes what arrived meanwhile
            // as already written: the decode starts again from the grid as it is now.
            if shown_again(t.painted_draw, self.draw) {
                t.decode = None;
            }
            t.painted_draw = self.draw;
            // The decode observes once per paint; under a decoding effect the window's glyph
            // timer supplies the frames that carry a changed cell through its scramble.
            let rows = t.core.rows_for_paint();
            // The echo of the user's own typing and the redraw of their own scroll settle at
            // once: the decode is for what the program writes.
            let quiet = t.user_quiet(now);
            let cols = t.core.columns();
            let (visible, stamps, dry) = match ink {
                Some(Ink::Paint(clock)) => {
                    let x = |col: usize| f32::from(bounds.origin.x) + col as f32 * cw;
                    let (stamps, ghosts, dry) = painted_rows(&mut t.decode, &rows, quiet, cols, x, clock, now);
                    (None, (stamps, ghosts), dry)
                }
                Some(Ink::Decode(timing)) => (decoded_rows(&mut t.decode, &rows, Some(timing), quiet, cols, now), (Vec::new(), Vec::new()), None),
                None => (decoded_rows(&mut t.decode, &rows, None, quiet, cols, now), (Vec::new(), Vec::new()), None),
            };
            (rows, visible, stamps, dry, frac, t.core.cursor(), animating || sync_open || drag_scrolling, highlights, t.core.selection_spans(), palette.clone(), find_owed)
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
            // A row with wet ink is painted in runs that each keep one glyph stamp, the shader
            // painting the ink in from it; any other row as ever.
            let washed = match self.ink {
                Some(Ink::Paint(clock)) => Some(clock),
                _ => None,
            };
            let paint_row = |cells: &Vec<(char, CellStyle, CellKind)>, stamps: &[f32], y: Pixels, window: &mut Window, cx: &mut App| {
                // Under watercolor a cell background is a wash: each stretch of one colour is
                // painted as a band the glyph shader inks, washed in as its row's text starts.
                if let Some(clock) = washed {
                    for band in bands(cells.iter().map(|c| c.1.bg), bg) {
                        let x = origin.x + px(band.col as f32 * cw);
                        let w = band.len as f32 * cw;
                        let stamp = stamps.get(band.col..band.col + band.len).and_then(|s| s.iter().copied().find(|&s| s != 0.0)).unwrap_or(0.0);
                        let stamp = paint::wash_stamp(stamp, f32::from(x), clock);
                        let path = crate::icons::wash_path(w.round().max(1.0) as u32, f32::from(lh).round().max(1.0) as u32);
                        let bounds = Bounds { origin: point(x, y), size: size(px(w), lh) };
                        let _ = window.with_glyph_stamp(stamp, |window| window.paint_svg(bounds, path, gpui::TransformationMatrix::unit(), hsla(band.color), cx));
                    }
                }
                let runs = if stamps.is_empty() {
                    build_runs(cells.iter().copied()).into_iter().map(|run| (run, 0.0)).collect()
                } else {
                    build_stamped_runs(cells.iter().copied().zip(stamps.iter().copied()))
                };
                for (run, stamp) in &runs {
                    let x = origin.x + px(run.col as f32 * cw);
                    let w = px(if run.wide { 2.0 * cw } else { run.text.chars().count() as f32 * cw });
                    if run.style.bg != bg && washed.is_none() {
                        window.paint_quad(fill(Bounds { origin: point(x, y), size: size(w, lh) }, hsla(run.style.bg)));
                    }
                    if run.text.trim().is_empty() {
                        continue;
                    }
                    let text: SharedString = run.text.clone().into();
                    let line = window.text_system().shape_line(text, px(self.font_size), &[text_run(&run.text, &run.style, self.family.clone())], None);
                    let _ = window.with_glyph_stamp(*stamp, |window| line.paint(point(x, y), lh, window, cx));
                    if run.style.underline {
                        let uy = y + lh - px(2.0);
                        window.paint_quad(fill(Bounds { origin: point(x, uy), size: size(w, px(1.0)) }, hsla(run.style.fg)));
                    }
                }
            };
            if let Some(above) = &rows.above {
                let y = origin.y + px((frac - 1.0) * cell.height);
                paint_row(above, &[], y, window, cx);
            }
            for (i, row) in visible.as_ref().unwrap_or(&rows.visible).iter().enumerate() {
                let y = origin.y + px((i as f32 + frac) * cell.height);
                // The characters this row's changed cells replaced fade out under the new ones, in
                // the new cells' style, and a doodled mark is drawn in its own; everything else in
                // the ghost row is a space.
                if let Some(ghosts) = ghosts.get(i).filter(|g| !g.is_empty()) {
                    let cells: Vec<(char, CellStyle, CellKind)> = row
                        .iter()
                        .zip(ghosts)
                        .map(|(&(_, style, kind), &(was, stamp, shown))| (if stamp == 0.0 { ' ' } else { was }, CellStyle { bg, ..shown.unwrap_or(style) }, kind))
                        .collect();
                    let stamps: Vec<f32> = ghosts.iter().map(|g| g.1).collect();
                    paint_row(&cells, &stamps, y, window, cx);
                }
                paint_row(row, stamps.get(i).map_or(&[][..], Vec::as_slice), y, window, cx);
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
        // Wet ink moves on the GPU alone: gpui presents this scene again, the glyph clock moving
        // on, until the newest ink is dry, and nothing is redrawn for it.
        if let Some(dry) = dry {
            window.present_glyphs_until(dry + paint::PAINT_MARGIN);
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
    use super::{bands, dim_alpha, frame_dt, hovered_link, shown_again, split_offset, Band, DIM_FADE, DIM_MIN_CONTRAST};
    use crate::terminal::CellMetrics;
    use gpui::{point, px, size, Bounds};

    #[test]
    fn a_row_s_backgrounds_are_bands_of_one_colour_each() {
        use crate::palette::Rgb;
        let (bg, red, dark) = (Rgb { r: 0, g: 0, b: 0 }, Rgb { r: 240, g: 200, b: 200 }, Rgb { r: 220, g: 150, b: 150 });
        let row = [bg, red, red, dark, dark, red, bg, bg, red];
        assert_eq!(
            bands(row.into_iter(), bg),
            vec![Band { col: 1, len: 2, color: red }, Band { col: 3, len: 2, color: dark }, Band { col: 5, len: 1, color: red }, Band { col: 8, len: 1, color: red }]
        );
        assert!(bands([bg, bg].into_iter(), bg).is_empty());
    }

    #[test]
    fn a_terminal_left_out_of_a_draw_is_shown_again_and_one_drawn_each_time_is_not() {
        assert!(!shown_again(0, 7), "the first paint has nothing to forget");
        assert!(!shown_again(6, 7), "drawn in the last draw: still on screen");
        assert!(!shown_again(7, 7), "painted twice in one draw");
        assert!(shown_again(5, 7), "the window drew once without it: its tab was hidden");
    }

    #[test]
    fn faint_text_fades_toward_its_background_but_keeps_three_to_one_contrast_with_it() {
        use crate::palette::Rgb;
        let navy = Rgb { r: 0x0e, g: 0x17, b: 0x23 };
        let white = Rgb { r: 0xc8, g: 0xcc, b: 0xd4 };
        assert_eq!(dim_alpha(white, navy), 1.0 - DIM_FADE, "bright text fades the whole way");
        let grey = Rgb { r: 0x7c, g: 0x87, b: 0x98 };
        let a = dim_alpha(grey, navy);
        assert!(a > 1.0 - DIM_FADE && a < 1.0, "{a}");
        let faded = |c: u8, b: u8| (f32::from(c) * a + f32::from(b) * (1.0 - a)).round() as u8;
        let shown = Rgb { r: faded(grey.r, navy.r), g: faded(grey.g, navy.g), b: faded(grey.b, navy.b) };
        assert!(crate::palette::contrast(shown, navy) >= DIM_MIN_CONTRAST - 0.05, "{shown:?}");
        let murk = Rgb { r: 0x30, g: 0x38, b: 0x44 };
        assert_eq!(dim_alpha(murk, navy), 1.0, "a colour already under 3:1 does not fade further");
    }
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
        let first = decoded_rows(&mut state, &rows("   "), t, false, 3, t0).expect("a decoding effect draws its own rows");
        assert!(state.is_some());
        assert_eq!(first[0][0].0, ' ');
        let second = decoded_rows(&mut state, &rows("ok "), t, false, 3, t0 + Duration::from_millis(20)).unwrap();
        assert_ne!(second[0][0].0, 'o', "a changed cell scrambles");
        assert!(decoded_rows(&mut state, &rows("ok "), None, false, 3, t0 + Duration::from_millis(30)).is_none(), "no effect: the rows are drawn as they are");
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
        decoded_rows(&mut state, &wide(' '), t, false, 2, t0);
        let fresh = wide('漢');
        let shown = decoded_rows(&mut state, &fresh, t, false, 2, t0 + Duration::from_millis(20)).unwrap();
        assert_ne!(shown[0][0].0, '漢', "the wide character scrambles");
        assert_eq!((shown[0][0].2, shown[0][1].2), (CellKind::Wide, CellKind::Spacer));
        assert_eq!(shown[0][1].0, ' ');
        assert_eq!(shown.len(), 1, "only the visible rows are decoded");
        assert_eq!(fresh.above.as_ref(), Some(&above), "the row above is left alone");
    }

    #[test]
    fn a_row_the_program_writes_is_stamped_until_its_ink_dries_and_the_user_s_own_echo_is_not() {
        use super::painted_rows;
        use crate::layout::{CellKind, CellStyle};
        use crate::terminal::RowsForPaint;
        use std::time::Duration;
        let style = CellStyle { fg: crate::palette::Rgb { r: 46, g: 43, b: 58 }, bg: crate::palette::Rgb { r: 0, g: 0, b: 0 }, bold: false, italic: false, dim: false, underline: false, inverse: false };
        let rows = |a: &str, b: &str| RowsForPaint { display_offset: 0, above: None, visible: [a, b].iter().map(|r| r.chars().map(|c| (c, style, CellKind::Normal)).collect()).collect(), links: vec![] };
        let x = |col: usize| col as f32 * 10.0;
        let t0 = Instant::now();
        let mut state = None;
        let none = (vec![vec![], vec![]], vec![vec![], vec![]], None);
        assert_eq!(painted_rows(&mut state, &rows("   ", "   "), false, 3, x, 5.0, t0), none, "the first grid is the baseline");
        let t1 = t0 + Duration::from_millis(20);
        let (stamps, ghosts, dry) = painted_rows(&mut state, &rows("ok ", "   "), false, 3, x, 5.02, t1);
        assert_eq!(ghosts, vec![vec![], vec![]], "new text replaced nothing");
        assert!(stamps[0][0] != 0.0 && stamps[0][0] == stamps[0][1] && stamps[0][2] == 0.0, "{stamps:?}");
        assert!(stamps[1].is_empty(), "an unchanged row is drawn as ever");
        assert!(dry.is_some_and(|d| d > t1));
        let t2 = t1 + Duration::from_secs(2);
        assert_eq!(painted_rows(&mut state, &rows("ok ", "   "), false, 3, x, 7.02, t2), none, "dry ink is drawn as ever");
        let typed = painted_rows(&mut state, &rows("ok ", "ls "), true, 3, x, 7.04, t2 + Duration::from_millis(20));
        assert_eq!(typed, none, "the user's own echo is not painted");
        let t3 = t2 + Duration::from_secs(1);
        let (_, ghosts, _) = painted_rows(&mut state, &rows("ok1", "ls "), false, 3, x, 8.04, t3);
        assert!(ghosts[0].is_empty(), "the 1 was written where nothing was");
        let t4 = t3 + Duration::from_secs(2);
        let (stamps, ghosts, _) = painted_rows(&mut state, &rows("ok2", "ls "), false, 3, x, 10.04, t4);
        assert!(stamps[0][2] != 0.0 && stamps[0][0] == 0.0, "{stamps:?}");
        assert_eq!(ghosts[0].iter().map(|g| g.0).collect::<String>(), "  1", "the replaced 1 fades out under the 2");
        assert!(ghosts[0][2].1 != 0.0 && ghosts[0][0].1 == 0.0, "{ghosts:?}");
    }

    #[test]
    fn only_the_newest_rows_are_written_by_the_pen_and_older_ones_show_as_written() {
        use super::painted_rows;
        use crate::effects::paint::PAINT_ROWS;
        use crate::layout::{CellKind, CellStyle};
        use crate::terminal::RowsForPaint;
        use std::time::Duration;
        let style = CellStyle { fg: crate::palette::Rgb { r: 46, g: 43, b: 58 }, bg: crate::palette::Rgb { r: 0, g: 0, b: 0 }, bold: false, italic: false, dim: false, underline: false, inverse: false };
        let grid = |lines: &[&str]| RowsForPaint { display_offset: 0, above: None, visible: lines.iter().map(|r| r.chars().map(|c| (c, style, CellKind::Normal)).collect()).collect(), links: vec![] };
        let x = |col: usize| col as f32 * 10.0;
        let t0 = Instant::now();
        let mut state = None;
        let blank = ["  "; 8];
        painted_rows(&mut state, &grid(&blank), false, 2, x, 5.0, t0);
        // A burst writes all eight rows at once: only the lowest few are written by the pen.
        let burst = ["a1", "b2", "c3", "d4", "e5", "f6", "g7", "h8"];
        let (stamps, _, dry) = painted_rows(&mut state, &grid(&burst), false, 2, x, 5.02, t0 + Duration::from_millis(20));
        let written: Vec<usize> = (0..8).filter(|&r| !stamps[r].is_empty()).collect();
        assert_eq!(written, (8 - PAINT_ROWS..8).collect::<Vec<_>>());
        assert!(dry.is_some());
        // Then one row changes on its own, and it takes a place from the oldest of those.
        let mut next = burst;
        next[0] = "z9";
        let (stamps, _, _) = painted_rows(&mut state, &grid(&next), false, 2, x, 5.1, t0 + Duration::from_millis(100));
        assert!(!stamps[0].is_empty(), "the newest change is written");
        assert_eq!((0..8).filter(|&r| !stamps[r].is_empty()).count(), PAINT_ROWS);
        assert!(stamps[8 - PAINT_ROWS].is_empty(), "the oldest of the burst's rows shows as written");
    }

    #[test]
    fn a_spinner_and_a_blinking_dot_are_hidden_and_doodled_as_one_steady_mark_under_them() {
        use super::painted_rows;
        use crate::effects::paint::{self, DOODLE_HOLD};
        use crate::layout::{CellKind, CellStyle};
        use crate::terminal::RowsForPaint;
        use std::time::Duration;
        let style = CellStyle { fg: crate::palette::Rgb { r: 46, g: 43, b: 58 }, bg: crate::palette::Rgb { r: 0, g: 0, b: 0 }, bold: false, italic: false, dim: false, underline: false, inverse: false };
        let green = CellStyle { fg: crate::palette::Rgb { r: 40, g: 160, b: 80 }, ..style };
        let grid = |a: &str, b: &str, dot: CellStyle| RowsForPaint {
            display_offset: 0,
            above: None,
            visible: vec![a.chars().map(|c| (c, style, CellKind::Normal)).collect(), b.chars().enumerate().map(|(i, c)| (c, if i == 0 { dot } else { style }, CellKind::Normal)).collect()],
            links: vec![],
        };
        let x = |col: usize| col as f32 * 10.0;
        let t0 = Instant::now();
        let mut state = None;
        painted_rows(&mut state, &grid("✢ Working", "⏺ Read", green), false, 9, x, 5.0, t0);
        // The spinner turns and the dot blinks off.
        let t1 = t0 + Duration::from_millis(120);
        let (stamps, ghosts, dry) = painted_rows(&mut state, &grid("✳ Working", "  Read", style), false, 9, x, 5.12, t1);
        let variant = |s: f32| ((s * 4096.0).round() as i32 & 31) as u32;
        assert_eq!(variant(stamps[0][0]), paint::PAINT_HIDE, "the frame itself is hidden");
        assert_eq!((ghosts[0][0].0, variant(ghosts[0][0].1)), ('✻', paint::PAINT_DOODLE), "a steady star is doodled under it");
        assert_eq!((ghosts[1][0].0, ghosts[1][0].2), ('⏺', Some(green)), "the dot blinked off is held, in its own colour");
        assert!(dry.is_some_and(|d| d >= t1 + Duration::from_secs_f32(DOODLE_HOLD) - Duration::from_millis(1)));
        // Once the spinner has stood still past the hold, it is drawn as it is.
        let t2 = t1 + Duration::from_secs(2);
        let (stamps, ghosts, _) = painted_rows(&mut state, &grid("✳ Working", "  Read", style), false, 9, x, 7.12, t2);
        assert!(stamps.iter().all(Vec::is_empty) && ghosts.iter().all(Vec::is_empty), "{stamps:?}");
    }

    #[test]
    fn a_cell_changed_while_the_user_is_at_work_is_drawn_as_its_own() {
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
        decoded_rows(&mut state, &rows("   "), t, false, 3, t0);
        let shown = decoded_rows(&mut state, &rows("ok "), t, true, 3, t0 + Duration::from_millis(20)).unwrap();
        assert_eq!(shown[0][0], ('o', plain, CellKind::Normal), "the typed o is its own, with its own colour");
        assert_eq!(shown[0][1], ('k', plain, CellKind::Normal));
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
