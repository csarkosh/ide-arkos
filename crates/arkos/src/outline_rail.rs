//! The outline rail of a markdown tab (outline rail spec, section 2). The collapsed rail is a
//! stepper of dots at the tab's left edge, the expanded panel a labelled list beside the
//! document, or over it in a narrow tab. The layout rules come first and are pure (the headings
//! in, the dots, the shape and the panel's rows out); the paint follows them, every colour read
//! from the theme it is handed.

use crate::effects::active::Style;
use crate::md_fold::{self, Head};
use crate::motion;
use crate::theme::Theme;
use crate::tree::{PaneId, TabId};
use crate::viewer::Viewer;
use crate::Root;
use gpui::{div, point, prelude::*, px, ClickEvent, Context, Div, Entity, FontWeight, KeyDownEvent, MouseMoveEvent};
use std::time::Instant;

/// The collapsed rail's width.
pub const RAIL_W: f32 = 26.0;
/// The expanded panel's width.
pub const PANEL_W: f32 = 220.0;
/// Below this tab width the expanded panel lies over the document instead of beside it.
pub const OVERLAY_BELOW: f32 = 520.0;
/// Below this tab width there is no rail at all.
pub const MIN_TAB_W: f32 = 200.0;
/// The least distance between two dots' tops.
pub const DOT_GAP: f32 = 16.0;
/// A top-level heading's dot.
const TOP_SIZE: f32 = 12.0;
/// A dot for a heading one level below the top.
const SUB_SIZE: f32 = 8.0;

/// Where a section stands against the one in view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DotState {
    /// Above the section in view.
    Read,
    /// The section in view.
    Current,
    /// Below it.
    Ahead,
}

/// One dot of the collapsed rail: its heading (an index into the headings), the top of its slot
/// (a slot is as tall as a top-level dot, and a smaller dot is centred in it), its size and its
/// state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dot {
    pub head: usize,
    pub y: f32,
    pub size: f32,
    pub state: DotState,
}

/// The collapsed rail's dots for `heads` in a rail `height` tall with the section `current` in
/// view: the top two levels (the document's lowest level and the one below) evenly spaced from
/// `top` down, at least DOT_GAP apart; when they do not fit, the top level alone; when that does
/// not fit either, every top-level dot DOT_GAP apart from `top` (the rail scrolls, `scroll_to`
/// says where). Sizes 12.0 and 8.0. The current dot is the last one at or above the heading in
/// view, so a deeper section in view lights the dot of the section holding it.
pub fn dots(heads: &[Head], current: Option<usize>, height: f32, top: f32) -> Vec<Dot> {
    let Some(first) = heads.iter().map(|h| h.level).min() else { return Vec::new() };
    let second = heads.iter().map(|h| h.level).filter(|&l| l > first).min();
    let pick = |levels: &[u8]| -> Vec<usize> { heads.iter().enumerate().filter(|(_, h)| levels.contains(&h.level)).map(|(i, _)| i).collect() };
    // The even spacing of `n` dots, the last slot's bottom at the rail's bottom.
    let spacing = |n: usize| if n <= 1 { f32::INFINITY } else { (height - TOP_SIZE) / (n - 1) as f32 };
    let both = pick(&second.map_or(vec![first], |s| vec![first, s]));
    let (chosen, gap) = if spacing(both.len()) >= DOT_GAP {
        let g = spacing(both.len());
        (both, g)
    } else {
        let alone = pick(&[first]);
        let g = spacing(alone.len());
        (alone, if g >= DOT_GAP { g } else { DOT_GAP })
    };
    let lit = current.and_then(|c| chosen.iter().rposition(|&h| h <= c));
    chosen
        .iter()
        .enumerate()
        .map(|(k, &head)| {
            let state = match lit {
                Some(l) if k < l => DotState::Read,
                Some(l) if k == l => DotState::Current,
                _ => DotState::Ahead,
            };
            let size = if heads[head].level == first { TOP_SIZE } else { SUB_SIZE };
            Dot { head, y: top + k as f32 * if gap.is_finite() { gap } else { 0.0 }, size, state }
        })
        .collect()
}

/// The rail's scroll offset that keeps the current dot in view, 0 when everything fits. The
/// current dot is held in the middle of the rail where the dots allow, never scrolling past the
/// first or the last.
pub fn scroll_to(dots: &[Dot], current: Option<usize>, height: f32) -> f32 {
    let (Some(first), Some(last)) = (dots.first(), dots.last()) else { return 0.0 };
    let overflow = last.y + TOP_SIZE - (first.y + height);
    if overflow <= 0.0 {
        return 0.0;
    }
    let Some(cur) = current.and_then(|c| dots.iter().rev().find(|d| d.head <= c)) else { return 0.0 };
    (cur.y - first.y - (height - TOP_SIZE) / 2.0).clamp(0.0, overflow)
}

/// The rail's width `progress` of the way (0 to 1, the open transition's opacity) from the
/// collapsed rail to the open panel.
pub fn rail_width(progress: f32) -> f32 {
    RAIL_W + (PANEL_W - RAIL_W) * progress.clamp(0.0, 1.0)
}

/// What the tab's left edge shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// Nothing: the document takes the full width.
    None,
    /// The collapsed dot rail.
    Rail,
    /// The expanded panel beside the document.
    Panel,
    /// The expanded panel over the document, with a scrim over the rest.
    Overlay,
}

/// What a tab `width` wide shows: nothing under MIN_TAB_W or with no headings, the rail when
/// collapsed, the panel beside the document when expanded and wide, the overlay when narrow.
/// Presentation mode shows the document alone.
pub fn shape(width: f32, expanded: bool, has_headings: bool, presenting: bool) -> Shape {
    if presenting || !has_headings || width < MIN_TAB_W {
        Shape::None
    } else if expanded && width < OVERLAY_BELOW {
        Shape::Overlay
    } else if expanded {
        Shape::Panel
    } else {
        Shape::Rail
    }
}

/// One row of the expanded panel: its heading, how deep it sits (`md_fold::outline`) and its
/// state against the section in view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelRow {
    pub head: usize,
    pub depth: usize,
    pub state: DotState,
}

/// The expanded panel's rows: every heading with its depth and state.
pub fn panel_rows(heads: &[Head], current: Option<usize>) -> Vec<PanelRow> {
    md_fold::outline(heads)
        .into_iter()
        .map(|r| {
            let state = match current {
                Some(c) if r.head < c => DotState::Read,
                Some(c) if r.head == c => DotState::Current,
                _ => DotState::Ahead,
            };
            PanelRow { head: r.head, depth: r.depth, state }
        })
        .collect()
}

/// The ☰ glyph's strip at the rail's top; the dots start below it.
const GLYPH_H: f32 = 26.0;
/// Where the first dot's slot starts, and the room kept under the last.
const DOTS_TOP: f32 = 34.0;
const DOTS_BOTTOM: f32 = 10.0;
/// The stepper's line runs down the rail's middle, in the panel as in the rail.
const LINE_X: f32 = RAIL_W / 2.0;
/// The panel's header ("OUTLINE" and «) and its rows.
const HEADER_H: f32 = 28.0;
const ROW_H: f32 = 26.0;
/// How far a deeper heading's label moves in.
const INDENT: f32 = 14.0;
/// Where a label starts, right of the line and its dots.
const LABEL_X: f32 = 28.0;
/// The faint line's alpha: it joins the dots and means nothing itself.
const LINE_ALPHA: f32 = 0.5;
/// The current row's pill, and the tint of a row under the pointer or the overlay's keys: the
/// accent faintly over the paper (`tab_active` and `tab_inactive` are the paper itself in most
/// built-in themes, so they would tint nothing).
pub const PILL_ALPHA: f32 = 0.14;
pub const HOVER_ALPHA: f32 = 0.07;
/// The overlay's scrim over the rest of the tab: Material's scrim, black at 32 %, the same on a
/// light theme and a dark one (the paper's own colour over the paper would dim nothing).
const SCRIM_ALPHA: f32 = 0.32;

/// The rail's dot or the panel's row under the pointer: its tab and heading, where its card goes
/// (window points, left and the pointer's height), since when, and whether it shows the section's
/// preview (a panel row) or the heading alone (a dot's tooltip).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hover {
    pub tab: TabId,
    pub head: usize,
    pub x: f32,
    pub y: f32,
    pub since: Instant,
    pub preview: bool,
}

impl Hover {
    /// When the card shows: after the app's tooltip delay.
    fn shows_at(&self) -> Instant {
        self.since + crate::grid_view::TIP_DELAY
    }

    /// Whether the card is fading in at `now`.
    pub fn animating(&self, now: Instant) -> bool {
        !motion::reduce_motion() && now >= self.shows_at() && now.saturating_duration_since(self.shows_at()) < motion::ENTER
    }
}

/// How open `tab`'s rail is at `now`, 0 (the dots) to 1 (the panel). A tab drawn for the first time
/// with its panel open (a relaunch, a tab shown again) starts open, with no transition; one whose
/// panel was closed some other way than the toggle closes.
pub fn openness(root: &mut Root, tab: TabId, open: bool, now: Instant) -> f32 {
    // A closed rail with nothing fading keeps no entry (`Root::settle` drops a finished one).
    if !open && !root.outline_anim.contains_key(&tab) {
        return 0.0;
    }
    let anim = root.outline_anim.entry(tab).or_insert_with(|| if open { motion::Transient::shown(true, now) } else { motion::Transient::Hidden });
    match (open, anim.value().is_some()) {
        (true, false) => anim.show(true, now),
        (false, true) => {
            anim.hide(now);
        }
        _ => {}
    }
    anim.current(now).map_or(0.0, |(_, o)| o)
}

/// What the tab's left edge draws for `viewer`: `rail` the column beside the document (`None`:
/// nothing, the document takes the width), `over` the overlay's scrim and panel laid over the
/// whole body. `x` is the pane's left edge in window points, `width` and `height` the body's.
pub struct Painted {
    pub rail: Option<Div>,
    pub over: Option<Div>,
}

#[allow(clippy::too_many_arguments)]
pub fn render(root: &mut Root, viewer: &Entity<Viewer>, pane: PaneId, tab: TabId, theme: &Theme, style: Style, x: f32, width: f32, height: f32, cx: &mut Context<Root>) -> Painted {
    let (open, has_heads, presenting) = {
        let v = viewer.read(cx);
        (v.outline, !v.heads.is_empty(), v.presenting)
    };
    let now = root.frame_now;
    let o = openness(root, tab, open, now);
    // The panel is drawn while it opens, stays or closes: the shape of an open rail, at any
    // opacity above none.
    let drawn = shape(width, o > 0.0, has_heads, presenting);
    let overlay = drawn == Shape::Overlay;
    // The overlay holds the keys while it is open in the focused pane; closed or wide, it lets go.
    let holds = overlay && open && root.tree.focused == pane && height >= 12.0;
    match root.outline_cursor {
        Some((t, _)) if t == tab && !holds => root.outline_cursor = None,
        Some((t, _)) if t == tab => {}
        _ if holds => {
            let current = { let v = viewer.read(cx); md_fold::current(&v.heads, v.top_block()) };
            root.outline_cursor = Some((tab, current.unwrap_or(0)));
            // Opened with the row in view lit and in sight.
            root.outline_scroll.scroll_to_item(current.unwrap_or(0));
        }
        _ => {}
    }
    if drawn == Shape::None || height < 12.0 {
        return Painted { rail: None, over: None };
    }
    let paper = theme.chrome.window_bg.alpha(style.document_paper_alpha);
    let border = if style.ink_borders { theme.chrome.border.alpha(0.0) } else { theme.chrome.border.hsla() };
    let w = if overlay { RAIL_W } else { rail_width(o) };
    let mut column = div().flex_none().relative().h_full().w(px(w)).overflow_hidden().bg(paper).border_r_1().border_color(border);
    if overlay || o < 1.0 {
        column = column.child(render_outline_rail(viewer, tab, theme, style, x, height, cx).opacity(if overlay { 1.0 } else { 1.0 - o }));
    }
    if !overlay && o > 0.0 {
        column = column.child(render_outline_panel(root, viewer, pane, tab, theme, style, x, false, false, cx).opacity(o));
    }
    let over = overlay.then(|| render_outline_overlay(root, viewer, pane, tab, theme, style, x, o, open, holds, cx));
    Painted { rail: Some(column), over }
}

/// The collapsed rail: the ☰ glyph, then the stepper's dots for the top two levels, the section
/// in view filled in the accent, those above in the dim text colour, those below hollow. A dot
/// under the pointer shows its heading after the tooltip delay; a click goes there.
#[allow(clippy::too_many_arguments)]
pub fn render_outline_rail(viewer: &Entity<Viewer>, tab: TabId, theme: &Theme, style: Style, x: f32, height: f32, cx: &mut Context<Root>) -> Div {
    let c = &theme.chrome;
    let (heads, current) = {
        let v = viewer.read(cx);
        (v.heads.clone(), md_fold::current(&v.heads, v.top_block()))
    };
    let glyph = div()
        .id(("outline-open", tab))
        .absolute()
        .top_0()
        .left_0()
        .w(px(RAIL_W))
        .h(px(GLYPH_H))
        .flex()
        .items_center()
        .justify_center()
        .text_color(c.text_dim.hsla())
        .text_size(px(style.ui(13.0)))
        .cursor_pointer()
        .child("\u{2630}")
        .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| root.toggle_outline_for(tab, cx)));
    let rail = div().absolute().top_0().left_0().w(px(RAIL_W)).h_full().child(glyph);
    let area = height - DOTS_TOP - DOTS_BOTTOM;
    if area < 12.0 {
        return rail;
    }
    let dots = dots(&heads, current, area, 0.0);
    let scroll = scroll_to(&dots, current, area);
    let line = c.text_dim.alpha(LINE_ALPHA);
    let mut strip = div().id(("outline-dots", tab)).absolute().top(px(DOTS_TOP)).left_0().w(px(RAIL_W)).h(px(area)).overflow_hidden();
    // The line runs between the dots, never under them, so a hollow dot stays hollow on a paper
    // that lets the ground through.
    for pair in dots.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (from, to) = (a.y + 6.0 + a.size / 2.0 + 2.0, b.y + 6.0 - b.size / 2.0 - 2.0);
        if to > from {
            strip = strip.child(div().absolute().left(px(LINE_X - 1.0)).top(px(from - scroll)).w(px(2.0)).h(px(to - from)).rounded(px(1.0)).bg(line));
        }
    }
    for d in &dots {
        let (head, centre) = (d.head, d.y + 6.0 - scroll);
        let w_move = cx.weak_entity();
        let w_hover = cx.weak_entity();
        let v_click = viewer.clone();
        let hit = div()
            .id(("outline-dot", head))
            .absolute()
            .left_0()
            .top(px(centre - 8.0))
            .w(px(RAIL_W))
            .h(px(16.0))
            .cursor_pointer()
            .child(mark(d.state, d.size, LINE_X, 8.0, theme))
            .on_click(move |_ev: &ClickEvent, _window, cx| v_click.update(cx, |v, cx| v.reveal_heading(head, cx)))
            .on_mouse_move(move |ev: &MouseMoveEvent, _window, cx| {
                let y = f32::from(ev.position.y);
                let _ = w_move.update(cx, |root, cx| hover(root, Hover { tab, head, x: x + RAIL_W + 6.0, y, since: Instant::now(), preview: false }, cx));
            })
            .on_hover(move |over: &bool, _window, cx| {
                if !over {
                    let _ = w_hover.update(cx, |root, cx| unhover(root, tab, head, cx));
                }
            });
        strip = strip.child(hit);
    }
    rail.child(strip)
}

/// One dot `size` across, centred at (`cx`, `cy`) in its box: filled in the accent for the
/// section in view, in the dim text colour for one read, a ring in that colour for one ahead.
fn mark(state: DotState, size: f32, cx: f32, cy: f32, theme: &Theme) -> Div {
    let c = &theme.chrome;
    let d = div().absolute().left(px(cx - size / 2.0)).top(px(cy - size / 2.0)).size(px(size)).rounded_full();
    match state {
        DotState::Current => d.bg(c.focus_ring.hsla()),
        DotState::Read => d.bg(c.text_dim.hsla()),
        DotState::Ahead => d.border(px(if size >= 12.0 { 2.0 } else { 1.5 })).border_color(c.text_dim.hsla()),
    }
}

/// A dot's or a row's hover begins: its card shows once the delay is up, drawn again then.
fn hover(root: &mut Root, h: Hover, cx: &mut Context<Root>) {
    if root.outline_hover.is_some_and(|o| o.tab == h.tab && o.head == h.head && o.preview == h.preview) {
        return;
    }
    root.outline_hover = Some(h);
    cx.spawn(async move |root, cx| {
        cx.background_executor().timer(crate::grid_view::TIP_DELAY).await;
        let _ = root.update(cx, |_, cx| cx.notify());
    })
    .detach();
}

fn unhover(root: &mut Root, tab: TabId, head: usize, cx: &mut Context<Root>) {
    if root.outline_hover.is_some_and(|h| h.tab == tab && h.head == head) {
        root.outline_hover = None;
        cx.notify();
    }
}

/// The expanded panel, `PANEL_W` wide: "OUTLINE" and « at its top, then a row per heading on
/// the same stepper, indented by level, the section in view on a tinted pill in the text colour.
/// A row under the pointer for half a second previews its section beside the panel; a click goes
/// there, and in the overlay (`modal`) closes it. In the overlay the row the keys move is lit.
/// Only the overlay that `holds` the keys tracks the overlay's scroll handle, so two overlays
/// open in two panes never share it.
#[allow(clippy::too_many_arguments)]
pub fn render_outline_panel(root: &mut Root, viewer: &Entity<Viewer>, pane: PaneId, tab: TabId, theme: &Theme, style: Style, x: f32, modal: bool, holds: bool, cx: &mut Context<Root>) -> Div {
    let c = &theme.chrome;
    let (heads, current) = {
        let v = viewer.read(cx);
        (v.heads.clone(), md_fold::current(&v.heads, v.top_block()))
    };
    let rows = panel_rows(&heads, current);
    let lit = root.outline_cursor.filter(|(t, _)| modal && *t == tab).map(|(_, h)| h);
    let family = root.ui_family();
    let header = div()
        .flex_none()
        .h(px(HEADER_H))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .pl(px(12.0))
        .pr(px(4.0))
        .text_color(c.text_dim.hsla())
        .text_size(px(style.ui(10.5)))
        .child("OUTLINE")
        .child(
            div()
                .id(("outline-close", tab))
                .w(px(22.0))
                .h(px(20.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .text_size(px(style.ui(13.0)))
                .cursor_pointer()
                .hover(|s| s.bg(c.focus_ring.alpha(HOVER_ALPHA)))
                .child("\u{00ab}")
                .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                    root.toggle_outline_for(tab, cx);
                    root.focus_pane(pane, cx);
                })),
        );
    let mut list = div().id(("outline-rows", tab)).flex_1().min_h_0().flex().flex_col().overflow_y_scroll().pb(px(8.0));
    if holds {
        list = list.track_scroll(&root.outline_scroll);
    }
    let line = c.text_dim.alpha(LINE_ALPHA);
    let last = rows.len().saturating_sub(1);
    for (k, r) in rows.iter().enumerate() {
        let head = r.head;
        let size = match r.depth {
            0 => 12.0,
            1 => 8.0,
            _ => 6.0,
        };
        let here = r.state == DotState::Current;
        let mid = ROW_H / 2.0;
        let (w_move, w_hover) = (cx.weak_entity(), cx.weak_entity());
        let v_click = viewer.clone();
        let mut row = div()
            .id(("outline-row", head))
            .flex_none()
            .relative()
            .h(px(ROW_H))
            .flex()
            .flex_row()
            .items_center()
            .mr(px(8.0))
            .rounded_r(px(ROW_H / 2.0))
            .cursor_pointer()
            .when(here, |d| d.bg(c.focus_ring.alpha(PILL_ALPHA)))
            .when(!here && lit == Some(head), |d| d.bg(c.focus_ring.alpha(HOVER_ALPHA)))
            .when(!here, |d| d.hover(|s| s.bg(c.focus_ring.alpha(HOVER_ALPHA))));
        // The line, above and below the dot, joining the rows.
        if k > 0 {
            row = row.child(div().absolute().left(px(LINE_X - 1.0)).top_0().w(px(2.0)).h(px(mid - size / 2.0 - 2.0)).bg(line));
        }
        if k < last {
            row = row.child(div().absolute().left(px(LINE_X - 1.0)).top(px(mid + size / 2.0 + 2.0)).w(px(2.0)).h(px(mid - size / 2.0 - 2.0)).bg(line));
        }
        // A width of its own: gpui cuts a line with its ellipsis only where the width is known
        // when the text is measured.
        let indent = LABEL_X + INDENT * r.depth as f32;
        let label = div().flex_none().ml(px(indent)).w(px((PANEL_W - 16.0 - indent).max(0.0))).overflow_hidden().whitespace_nowrap().text_ellipsis().child(heads[head].text.clone());
        row = row
            .child(mark(r.state, size, LINE_X, mid, theme))
            .child(
                label
                    .text_color(if here { c.text.hsla() } else { c.text_dim.hsla() })
                    .text_size(px(style.ui(if r.depth == 0 { 12.5 } else { 12.0 })))
                    .when(here, |d| d.font_weight(FontWeight::SEMIBOLD)),
            )
            .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                v_click.update(cx, |v, cx| v.reveal_heading(head, cx));
                if modal {
                    root.toggle_outline_for(tab, cx);
                    root.focus_pane(pane, cx);
                }
            }))
            .on_mouse_move(move |ev: &MouseMoveEvent, _window, cx| {
                let y = f32::from(ev.position.y);
                let _ = w_move.update(cx, |root, cx| hover(root, Hover { tab, head, x: x + PANEL_W + 6.0, y, since: Instant::now(), preview: true }, cx));
            })
            .on_hover(move |over: &bool, _window, cx| {
                if !over {
                    let _ = w_hover.update(cx, |root, cx| unhover(root, tab, head, cx));
                }
            });
        list = list.child(row);
    }
    div().absolute().top_0().left_0().w(px(PANEL_W)).h_full().flex().flex_col().font_family(family).child(header).child(list)
}

/// The panel over the document in a narrow tab, sliding in from the left edge with a scrim over
/// the rest that fades with it. Open, it holds the keys: Up and Down move the lit row, Enter goes
/// to its heading and closes, Escape closes; a click on the scrim closes. Closing, it is drawn
/// only, taking no clicks.
#[allow(clippy::too_many_arguments)]
pub fn render_outline_overlay(root: &mut Root, viewer: &Entity<Viewer>, pane: PaneId, tab: TabId, theme: &Theme, style: Style, x: f32, o: f32, open: bool, holds: bool, cx: &mut Context<Root>) -> Div {
    let c = &theme.chrome;
    let panel = render_outline_panel(root, viewer, pane, tab, theme, style, x, true, holds, cx);
    let border = if style.ink_borders { c.border.alpha(0.0) } else { c.border.hsla() };
    let mut scrim = div().absolute().top_0().left_0().size_full().bg(gpui::black().opacity(SCRIM_ALPHA)).opacity(o);
    let mut sheet = div()
        .absolute()
        .top_0()
        .bottom_0()
        .left(px(-PANEL_W * (1.0 - o)))
        .w(px(PANEL_W))
        .bg(c.window_bg.hsla())
        .border_r_1()
        .border_color(border)
        .shadow(vec![gpui::BoxShadow { color: gpui::black().opacity(0.18 * o), offset: point(px(4.0), px(0.0)), blur_radius: px(18.0), spread_radius: px(0.0) }])
        .child(panel);
    if open {
        scrim = scrim.occlude().on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(move |root, _ev: &gpui::MouseDownEvent, _window, cx| {
                cx.stop_propagation();
                let now = std::time::Instant::now();
                root.view_menu.hide(now);
                root.md_menu.hide(now);
                root.toggle_outline_for(tab, cx);
                root.focus_pane(pane, cx);
            }),
        );
        sheet = sheet.occlude();
    }
    // Only the overlay holding the cursor tracks the one focus handle: another open in an
    // unfocused pane would otherwise take the keys of the one in front.
    if open && holds {
        let v_keys = viewer.clone();
        sheet = sheet.track_focus(&root.outline_focus).on_key_down(cx.listener(move |root, ev: &KeyDownEvent, _window, cx| {
            let m = &ev.keystroke.modifiers;
            if m.platform || m.control || m.alt {
                return;
            }
            cx.stop_propagation();
            let Some((t, lit)) = root.outline_cursor.filter(|(t, _)| *t == tab) else { return };
            let heads = v_keys.read(cx).heads.len();
            match ev.keystroke.key.as_str() {
                "up" | "down" if heads > 0 => {
                    let next = if ev.keystroke.key == "up" { lit.saturating_sub(1) } else { (lit + 1).min(heads - 1) };
                    root.outline_cursor = Some((t, next));
                    root.outline_scroll.scroll_to_item(next);
                    cx.notify();
                }
                "enter" => {
                    v_keys.update(cx, |v, cx| v.reveal_heading(lit, cx));
                    root.toggle_outline_for(tab, cx);
                    root.focus_pane(pane, cx);
                }
                "escape" => {
                    root.toggle_outline_for(tab, cx);
                    root.focus_pane(pane, cx);
                }
                _ => {}
            }
        }));
    }
    div().absolute().top_0().left_0().size_full().child(scrim).child(sheet)
}

/// The tooltip of the dot under the pointer, or the preview of the panel row under it, once the
/// delay is up, while its tab is on screen: the heading alone, or the heading and the first six
/// lines of its section, in the menu frame, at the pointer's height beside the rail or the panel.
pub fn render_hover(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let h = root.outline_hover?;
    // A hover whose tab left the screen is forgotten, or its card would come back at the old
    // pointer when the tab is shown again.
    let shown = root.tree.locate(h.tab).is_some_and(|(pane, idx)| root.tree.pane(pane).is_some_and(|p| p.active == idx));
    if !shown {
        root.outline_hover = None;
        return None;
    }
    let now = root.frame_now;
    if now < h.shows_at() {
        return None;
    }
    let viewer = root.viewers.get(&h.tab)?.clone();
    let v = viewer.read(cx);
    if v.outline != h.preview {
        return None;
    }
    let head = v.heads.get(h.head)?;
    let lines = if h.preview { md_fold::preview(&v.heads, h.head, &v.blocks, v.doc.as_ref()?.lines(), 6) } else { vec![head.text.clone()] };
    let o = motion::enter_progress(h.shows_at(), now, motion::ENTER);
    let style = root.effect_style();
    let theme = root.theme.clone();
    let c = &theme.chrome;
    let mut card = div()
        .absolute()
        .left(px(h.x))
        .top(px(h.y - 12.0 - motion::lift(o, motion::MENU_LIFT)))
        .max_w(px(360.0))
        .opacity(o)
        .flex()
        .flex_col()
        .gap(px(2.0))
        .px(px(if h.preview { 10.0 } else { 8.0 }))
        .py(px(if h.preview { 8.0 } else { 3.0 }))
        .rounded(px(6.0))
        .map(|d| crate::chrome::menu_frame(d, &theme, &style))
        .font_family(root.ui_family())
        .text_size(px(root.ui_size(12.0)))
        .text_color(c.text_dim.hsla());
    for (i, l) in lines.into_iter().enumerate() {
        card = card.child(div().overflow_hidden().whitespace_nowrap().text_ellipsis().when(i == 0, |d| d.text_color(c.text.hsla()).when(h.preview, |d| d.font_weight(FontWeight::SEMIBOLD))).child(l));
    }
    Some(card)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heads(levels: &[u8]) -> Vec<Head> {
        levels.iter().enumerate().map(|(i, &level)| Head { block: i * 2, level, text: format!("h{i}"), nth: 0 }).collect()
    }

    #[test]
    fn dots_mark_the_top_two_levels_read_current_and_ahead() {
        let d = dots(&heads(&[1, 2, 2, 1, 3]), Some(2), 300.0, 0.0);
        assert_eq!(d.iter().map(|d| d.head).collect::<Vec<_>>(), vec![0, 1, 2, 3], "no dot for the third level");
        assert_eq!(d.iter().map(|d| d.state).collect::<Vec<_>>(), vec![DotState::Read, DotState::Read, DotState::Current, DotState::Ahead]);
        assert_eq!(d.iter().map(|d| d.size).collect::<Vec<_>>(), vec![12.0, 8.0, 8.0, 12.0]);
        let deeper = dots(&heads(&[1, 2, 2, 1, 3]), Some(4), 300.0, 0.0);
        assert_eq!(deeper[3].state, DotState::Current, "a section in view without a dot lights the dot of the one holding it");
        assert!(dots(&heads(&[1, 2]), None, 300.0, 0.0).iter().all(|d| d.state == DotState::Ahead), "above the first heading everything is ahead");
        let shifted = dots(&heads(&[2, 3, 4]), None, 300.0, 0.0);
        assert_eq!(shifted.iter().map(|d| d.size).collect::<Vec<_>>(), vec![12.0, 8.0], "the document's own top two levels");
        assert!(dots(&[], None, 300.0, 0.0).is_empty());
    }

    #[test]
    fn dots_are_spaced_evenly_at_least_the_gap_apart() {
        let d = dots(&heads(&[1, 2, 2, 1]), None, 300.0, 30.0);
        assert_eq!(d.len(), 4);
        assert_eq!(d[0].y, 30.0, "from the top");
        let gaps: Vec<f32> = d.windows(2).map(|w| w[1].y - w[0].y).collect();
        assert!(gaps.iter().all(|&g| g >= DOT_GAP), "{gaps:?}");
        assert!(gaps.windows(2).all(|w| (w[0] - w[1]).abs() < 0.001), "evenly: {gaps:?}");
        assert!(d[3].y + 12.0 <= 330.0 + 0.001, "the last inside the rail");
    }

    #[test]
    fn a_long_document_keeps_its_dots_apart() {
        let mixed: Vec<u8> = (0..60).map(|i| if i % 2 == 0 { 1 } else { 2 }).collect();
        let d = dots(&heads(&mixed), Some(10), 300.0, 0.0);
        assert!(d.iter().all(|d| d.size == 12.0), "only the top level when both do not fit");
        assert_eq!(d.len(), 30);
        assert!(d.windows(2).all(|w| w[1].y - w[0].y >= DOT_GAP - 0.001));
        let long = heads(&[1; 60]);
        for current in [0, 30, 59] {
            let d = dots(&long, Some(current), 300.0, 0.0);
            assert_eq!(d.len(), 60);
            assert!(d.windows(2).all(|w| (w[1].y - w[0].y - DOT_GAP).abs() < 0.001), "the gap apart");
            let s = scroll_to(&d, Some(current), 300.0);
            let y = d[current].y - s;
            assert!(y >= 0.0 && y + 12.0 <= 300.0, "the current dot at {y} inside the rail (scroll {s})");
        }
        let short = dots(&heads(&[1, 1, 1]), Some(2), 300.0, 0.0);
        assert_eq!(scroll_to(&short, Some(2), 300.0), 0.0, "everything fits");
    }

    #[test]
    fn the_shape_follows_width_state_and_headings() {
        assert_eq!(shape(800.0, false, true, false), Shape::Rail);
        assert_eq!(shape(800.0, true, true, false), Shape::Panel);
        assert_eq!(shape(500.0, true, true, false), Shape::Overlay);
        assert_eq!(shape(500.0, false, true, false), Shape::Rail);
        assert_eq!(shape(800.0, true, false, false), Shape::None);
        assert_eq!(shape(800.0, true, true, true), Shape::None, "presentation mode shows the document alone");
    }

    #[test]
    fn a_very_narrow_tab_shows_no_rail() {
        assert_eq!(shape(160.0, false, true, false), Shape::None);
    }

    #[test]
    fn the_rail_s_width_eases_between_closed_and_open() {
        assert_eq!(rail_width(0.0), RAIL_W);
        assert_eq!(rail_width(1.0), PANEL_W);
        let steps: Vec<f32> = (0..=20).map(|i| rail_width(i as f32 / 20.0)).collect();
        assert!(steps.windows(2).all(|w| w[1] >= w[0]), "monotonic: {steps:?}");
        assert_eq!(rail_width(-1.0), RAIL_W, "clamped below");
        assert_eq!(rail_width(2.0), PANEL_W, "and above");
    }

    #[test]
    fn panel_rows_carry_every_heading_with_its_depth() {
        let rows = panel_rows(&heads(&[1, 2, 3, 1]), Some(1));
        assert_eq!(rows.iter().map(|r| r.depth).collect::<Vec<_>>(), vec![0, 1, 2, 0]);
        assert_eq!(rows.iter().map(|r| r.head).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
        assert_eq!(rows.iter().map(|r| r.state).collect::<Vec<_>>(), vec![DotState::Read, DotState::Current, DotState::Ahead, DotState::Ahead]);
    }
}
