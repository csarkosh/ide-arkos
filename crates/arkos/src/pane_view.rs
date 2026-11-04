//! One pane: its tab bar and the active tab's content (a terminal, a file viewer, a browser
//! tab's placeholder, a busy notice while a shell starts or a replay is parsed, or the empty
//! state), and the URL prompt over the focused pane.

use crate::effects::active::Style;
use crate::drag;
use crate::effects::blocks;
use crate::effects::decode;
use crate::find;
use crate::line_edit::{self, Edit, LineEdit};
use crate::link_menu::{press_kind, LinkMenu, PressKind};
use crate::md_view;
use crate::motion;
use crate::sessions::Note;
use crate::terminal::Terminal;
use crate::terminal_element::{grid_for, Ink, TerminalElement};
use crate::theme::Theme;
use crate::tree::{Pane, PaneId, Rect, Tab, TabId, ViewMode};
use crate::viewer::{self, Content};
use crate::grid_view::{GridColors, GridElement};
use crate::viewer_element::{MarkColors, RawColors, ViewerElement};
use crate::Root;
use gpui::{
    canvas, div, linear_color_stop, linear_gradient, prelude::*, px, ClickEvent, ClipboardItem, Context, Div, DragMoveEvent, ElementId, Entity, FocusHandle,
    KeyDownEvent, Keystroke, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ScrollDelta, ScrollWheelEvent, SharedString, Stateful, Window,
};

pub const TAB_BAR_H: f32 = 28.0;
/// How far the focused pane's glow reaches in from each edge, at rest.
pub const GLOW_W: f32 = 4.0;
/// The glow's opacity at the edge, at the peak of its breath; it fades to nothing by its reach.
pub const GLOW_ALPHA: f32 = 0.45;
/// The width in points of the focused pane's ring, under an effect that rounds the panes.
pub const FOCUS_RING_W: f32 = 2.0;

/// The radius of the bottom corners of a pane's paper (a terminal's, a document's) under `style`:
/// the card's, less the pane's one point border, so the paper's corners lie inside the card the
/// wash draws instead of showing square on the ground round it.
pub fn paper_radius(style: crate::effects::active::Style) -> f32 {
    (style.pane_radius - 1.0).max(0.0)
}
/// The margin in points between a terminal's text and its card's left and right sides on a
/// rounded card (`pane_radius`), so the text does not meet the card's edge.
pub const CARD_TEXT_INSET: f32 = 4.0;

pub fn terminal_inset(style: crate::effects::active::Style) -> f32 {
    if style.pane_radius > 0.0 { CARD_TEXT_INSET } else { 0.0 }
}

/// On a rounded card the pane number is centred in a slot this many points in from the card's
/// left edge and this wide, so an effect can draw a chip of that width behind it (Material's
/// sits exactly on the slot) clear of the card's corner and its focus ring.
pub const NUMBER_SLOT_INSET: f32 = 10.0;
pub const NUMBER_SLOT_W: f32 = 18.0;

/// Where to paint a line whose ink runs from `ink.0` to `ink.1` points past its origin across,
/// whose figures are `cap` points tall on the baseline and whose ascent is `ascent`, so its ink
/// is centred on `centre`: the origin, painted with a line height of its ascent and descent.
pub fn centred_origin(centre: (f32, f32), ink: (f32, f32), cap: f32, ascent: f32) -> (f32, f32) {
    let x = centre.0 - (ink.0 + ink.1) / 2.0;
    let baseline = centre.1 + cap / 2.0;
    (x, baseline - ascent)
}

/// Paints `text` in `bounds` with its ink centred there, on its cap height down and its glyphs'
/// own bounds across, so a number sits in the middle of a chip drawn on the same bounds rather
/// than where its line box would put it.
fn paint_centred(text: &gpui::SharedString, family: gpui::SharedString, size: f32, color: gpui::Hsla, bounds: gpui::Bounds<gpui::Pixels>, window: &mut gpui::Window, cx: &mut gpui::App) {
    let f = gpui::font(family);
    let run = gpui::TextRun { len: text.len(), font: f.clone(), color, background_color: None, underline: None, strikethrough: None };
    let ts = window.text_system().clone();
    let line = ts.shape_line(text.clone(), px(size), &[run], None);
    let cap = f32::from(ts.cap_height(ts.resolve_font(&f), px(size)));
    let mut ink = (f32::MAX, f32::MIN);
    for run in &line.runs {
        for g in &run.glyphs {
            let ch = text.get(g.index..).and_then(|s| s.chars().next()).unwrap_or('0');
            if let Ok(b) = ts.typographic_bounds(run.font_id, px(size), ch) {
                let x0 = f32::from(g.position.x + b.origin.x);
                ink = (ink.0.min(x0), ink.1.max(x0 + f32::from(b.size.width)));
            }
        }
    }
    if ink.0 > ink.1 {
        ink = (0.0, f32::from(line.width));
    }
    let centre = (f32::from(bounds.origin.x) + f32::from(bounds.size.width) / 2.0, f32::from(bounds.origin.y) + f32::from(bounds.size.height) / 2.0);
    let (x, y) = centred_origin(centre, ink, cap, f32::from(line.ascent));
    let _ = line.paint(gpui::point(px(x), px(y)), line.ascent + line.descent, window, cx);
}

/// Where the pane number sits under `style`: Some((inset, width)) on a rounded card, None for
/// the plain eight points of padding.
pub fn number_slot(style: crate::effects::active::Style) -> Option<(f32, f32)> {
    (style.pane_radius > 0.0).then_some((NUMBER_SLOT_INSET, NUMBER_SLOT_W))
}
/// The breath (`pulse::glow`): at rest the glow keeps this share of its opacity, and at the peak
/// it gains this much lightness and this share of its reach, so it reads as light at the edge.
const GLOW_BREATH_FLOOR: f32 = 0.7;
const GLOW_BREATH_LIGHT: f32 = 0.2;
const GLOW_BREATH_REACH: f32 = 0.25;

/// The glow's reach for a pane `w` by `h` at `breath` (0 at rest, 1 at the peak): `GLOW_W` plus
/// the breath's share, or less in a pane too small for it.
pub fn glow_width(w: f32, h: f32, breath: f32) -> f32 {
    (GLOW_W * (1.0 + GLOW_BREATH_REACH * breath)).min(w / 2.0).min(h / 2.0).max(0.0)
}

/// What a tab shows before its title (spec 5): a breathing glyph, a warm dot, nothing, or a
/// muted title for a shell that ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabMark {
    None,
    Working,
    Waiting,
    Dead,
}

impl TabMark {
    pub fn of(state: Option<protocol::ClaudeState>, alive: bool) -> TabMark {
        use protocol::ClaudeState::*;
        match (alive, state) {
            (false, _) => TabMark::Dead,
            (true, Some(Working)) => TabMark::Working,
            (true, Some(Waiting)) => TabMark::Waiting,
            _ => TabMark::None,
        }
    }
}

/// The text on a tab: the terminal's title (or `fallback`, the shell's name), then its state.
pub fn tab_label(title: &str, fallback: &str, alive: bool, exit_code: Option<i32>, note: Option<Note>, busy: Option<&str>) -> String {
    if let Some(b) = busy {
        return b.to_string();
    }
    let name = if title.is_empty() { fallback } else { title };
    if !alive {
        return match exit_code {
            Some(c) => format!("{name} (ended, exit {c})"),
            None => format!("{name} (ended)"),
        };
    }
    match note {
        Some(Note::Restarted) => format!("{name} (restarted)"),
        Some(Note::Resumed) => format!("{name} (resumed after restart)"),
        None => name.to_string(),
    }
}

/// gpui has no three-part element id; the pane goes in the name, the tab index in the number.
fn tab_id(kind: &str, pane: PaneId, idx: usize) -> ElementId {
    ElementId::NamedInteger(format!("{kind}-{pane}").into(), idx as u64)
}

/// The bar's fixed children after the tabs, by name. Only the + button: tabs are reached by
/// clicking them, by the bar scrolling to the active one, and by the keyboard.
pub fn bar_items() -> Vec<&'static str> {
    vec!["+"]
}

/// A URL as typed, with a scheme added when it has none: `http://` for this machine (a dev
/// server rarely has TLS), `https://` otherwise. `localhost:3000` has none: a scheme is only
/// what comes before `://`, or one of the few Chrome opens without it.
fn with_scheme(url: &str) -> String {
    let bare = ["about:", "chrome:", "data:", "file:"];
    if url.contains("://") || bare.iter().any(|s| url.starts_with(s)) {
        url.to_string()
    } else if is_local_host(url) {
        format!("http://{url}")
    } else {
        format!("https://{url}")
    }
}

/// Whether a scheme-less URL's host is this machine: `localhost`, `*.localhost`, `127.0.0.1`
/// or `[::1]`. The host runs up to the first `/`, its `:port` stripped (brackets kept for IPv6).
fn is_local_host(url: &str) -> bool {
    let authority = url.split('/').next().unwrap_or("");
    let host = match authority.strip_prefix('[') {
        Some(rest) => rest.split(']').next().map(|h| format!("[{h}]")).unwrap_or_default(),
        None => authority.split(':').next().unwrap_or("").to_string(),
    };
    let host = host.to_ascii_lowercase();
    host == "localhost" || host.ends_with(".localhost") || host == "127.0.0.1" || host == "[::1]"
}

/// What the URL prompt opens: what was typed, else `[browser] url`; `None` (both empty) is a
/// blank window.
pub fn prompt_url(typed: &str, configured: &str) -> Option<String> {
    let typed = typed.trim();
    let url = if typed.is_empty() { configured.trim() } else { typed };
    (!url.is_empty()).then(|| with_scheme(url))
}

/// What `typed` names in the path prompt: a path under the home folder for `~`, under the
/// project for a relative one, as it is for an absolute one; `None` when nothing was typed.
pub fn prompt_path(typed: &str, project: &std::path::Path, home: &std::path::Path) -> Option<std::path::PathBuf> {
    let typed = typed.trim();
    if typed.is_empty() {
        return None;
    }
    Some(match typed.strip_prefix('~') {
        Some("") => home.to_path_buf(),
        Some(rest) if rest.starts_with('/') => home.join(rest.trim_start_matches('/')),
        _ if typed.starts_with('/') => std::path::PathBuf::from(typed),
        _ => project.join(typed),
    })
}

/// What the prompt asks for: a URL for a web tab, or a file's path to open in the focused pane
/// ("Open file in pane...").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    Url,
    Path,
}

/// The URL prompt ("New browser tab"), a small box at the top of the focused pane; the same box
/// asks for a path when `kind` is `Path`. `target` is the web tab to point at the URL; `None`
/// makes a new tab.
#[derive(Debug, Clone, PartialEq)]
pub struct UrlPrompt {
    pub edit: LineEdit,
    pub kind: PromptKind,
    pub target: Option<TabId>,
    /// How the tab it opens is opened: the palette or the key that opened the prompt.
    pub via: crate::telemetry::Opened,
}

/// What a key in the URL prompt leads to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlStep {
    Stay,
    Cancel,
    /// Enter: the URL to open (`prompt_url`).
    Open(Option<String>),
    /// Enter in the path prompt: what was typed, trimmed (`prompt_path` reads it).
    OpenPath(String),
}

impl UrlPrompt {
    pub fn new(kind: PromptKind, target: Option<TabId>) -> Self {
        UrlPrompt { edit: LineEdit::new(), kind, target, via: crate::telemetry::current_via() }
    }

    /// One key; `configured` is `[browser] url`, taken when a URL field is empty. Enter on an
    /// empty path opens nothing and keeps the prompt.
    pub fn key(&mut self, ks: &Keystroke, configured: &str) -> UrlStep {
        match self.edit.handle(ks) {
            Edit::Submit | Edit::SubmitBack if self.kind == PromptKind::Path => match self.edit.text.trim() {
                "" => UrlStep::Stay,
                typed => UrlStep::OpenPath(typed.to_string()),
            },
            Edit::Submit | Edit::SubmitBack => UrlStep::Open(prompt_url(&self.edit.text, configured)),
            Edit::Cancel => UrlStep::Cancel,
            _ => UrlStep::Stay,
        }
    }
}

/// A tab dragged within its own bar: its slot closes behind it and a gap the tab's width opens
/// where it would land, gliding between slots as the pointer crosses the tabs. A drop lands the
/// tab in the gap (nothing moves); a cancel closes the gap and opens the slot again. Under
/// Reduce Motion every slide lands at once.
pub struct Reorder {
    pub pane: PaneId,
    pub tab: TabId,
    width: f32,
    at: Option<usize>,
    /// Gaps by the slot they sit before (one per slot the pointer visited, the old ones closing).
    gaps: Vec<(usize, motion::Slide)>,
    /// The dragged tab's own slot, closing at the start and opening again on a cancel.
    slot: motion::Slide,
    ending: bool,
}

impl Reorder {
    pub fn start(pane: PaneId, tab: TabId, width: f32, now: std::time::Instant) -> Self {
        Reorder { pane, tab, width, at: None, gaps: Vec::new(), slot: motion::Slide::new(width, 0.0, now), ending: false }
    }

    /// Points the gap at the slot the tab would land before (`None`: the pointer is elsewhere).
    pub fn aim(&mut self, at: Option<usize>, now: std::time::Instant) {
        if self.ending || at == self.at {
            return;
        }
        for (slot, s) in self.gaps.iter_mut() {
            if Some(*slot) != at {
                *s = motion::Slide::new(s.at(now), 0.0, now);
            }
        }
        if let Some(a) = at {
            match self.gaps.iter_mut().find(|(s, _)| *s == a) {
                Some((_, s)) => *s = motion::Slide::new(s.at(now), self.width, now),
                None => self.gaps.push((a, motion::Slide::new(0.0, self.width, now))),
            }
        }
        self.at = at;
    }

    /// A cancel: every gap closes and the slot opens again; `done` once that has played.
    pub fn end(&mut self, now: std::time::Instant) {
        self.ending = true;
        self.at = None;
        for (_, s) in self.gaps.iter_mut() {
            *s = motion::Slide::new(s.at(now), 0.0, now);
        }
        self.slot = motion::Slide::new(self.slot.at(now), self.width, now);
    }

    /// The gap drawn before `slot` this frame.
    pub fn gap_before(&self, slot: usize, now: std::time::Instant) -> f32 {
        self.gaps.iter().filter(|(s, _)| *s == slot).map(|(_, g)| g.at(now)).sum()
    }

    /// The dragged tab's slot width this frame.
    pub fn slot_width(&self, now: std::time::Instant) -> f32 {
        self.slot.at(now)
    }

    pub fn animating(&self, now: std::time::Instant) -> bool {
        self.slot.animating(now) || self.gaps.iter().any(|(_, g)| g.animating(now))
    }

    pub fn done(&self, now: std::time::Instant) -> bool {
        self.ending && !self.animating(now)
    }

    /// Whether a cancel has ended it (its slides may still be playing).
    pub fn ended(&self) -> bool {
        self.ending
    }

    /// The slot the gap sits before, in the bar with the dragged tab still in it.
    pub fn at(&self) -> Option<usize> {
        self.at
    }
}

/// The slot a tab dragged along its own bar lands in, from the pointer's `x` in the bar's
/// content: `widths` are every tab's in bar order, `dragged` the tab's index, `current` the slot
/// the gap sits before now. The other tabs are laid out as they are drawn (the gap, the dragged
/// tab's width, at `current`), and the slot is the first whose tab's midpoint lies past `x`,
/// so a pointer resting in the gap keeps it where it is. Slots count the dragged tab's own
/// position, as `Tree::move_tab` does; `dragged` means it stays.
pub fn reorder_slot(widths: &[f32], dragged: usize, current: Option<usize>, x: f32) -> usize {
    let gap = widths.get(dragged).copied().unwrap_or(0.0);
    let others: Vec<f32> = widths.iter().enumerate().filter(|(i, _)| *i != dragged).map(|(_, w)| *w).collect();
    let gap_at = current.map(|c| if c > dragged { c - 1 } else { c }.min(others.len()));
    let mut pos = 0.0;
    let mut slot = others.len();
    for (i, w) in others.iter().enumerate() {
        if gap_at == Some(i) {
            pos += gap;
        }
        if x < pos + w / 2.0 {
            slot = i;
            break;
        }
        pos += w;
    }
    if slot <= dragged { slot } else { slot + 1 }
}

/// A tab the user is renaming in place (double-click on its title): the editor shows where the
/// label was; Enter keeps the name, Escape drops the edit, a click elsewhere keeps it.
pub struct TabRename {
    pub pane: PaneId,
    pub tab: TabId,
    pub edit: LineEdit,
}

/// Every tab's label and mark, in bar order; the bar shows the labels.
/// A name the user gave the tab replaces the automatic label; the mark still follows the shell.
/// A viewer with unsaved edits ends its label in " •".
fn tab_labels(root: &Root, p: &Pane, cx: &Context<Root>) -> Vec<(String, TabMark)> {
    p.tabs
        .iter()
        .enumerate()
        .map(|(idx, tab)| {
            let (label, mark) = automatic_label(root, p, idx, tab);
            let named = p.ids.get(idx).and_then(|id| root.tree.tab_name(*id)).map(str::to_string);
            let dirty = matches!(tab, Tab::Viewer { .. }) && p.ids.get(idx).and_then(|id| root.viewers.get(id)).is_some_and(|v| v.read(cx).dirty);
            let label = named.unwrap_or(label);
            (if dirty { format!("{label} •") } else { label }, mark)
        })
        .collect()
}

/// The title a tab shows in its bar, without the unsaved mark: the name the user gave it, else
/// its automatic label (for a terminal the program's title, never what the terminal shows).
/// `arkos panes` lists it.
pub fn tab_title(root: &Root, pane: PaneId, idx: usize) -> String {
    let Some(p) = root.tree.pane(pane) else { return String::new() };
    let Some(tab) = p.tabs.get(idx) else { return String::new() };
    match p.ids.get(idx).and_then(|id| root.tree.tab_name(*id)) {
        Some(name) => name.to_string(),
        None => automatic_label(root, p, idx, tab).0,
    }
}

fn automatic_label(root: &Root, p: &Pane, idx: usize, tab: &Tab) -> (String, TabMark) {
    match tab {
        Tab::Terminal { session, .. } => {
            let busy = p.ids.get(idx).and_then(|id| root.sessions.busy.get(id)).copied();
            let note = root.sessions.notes.get(session).copied();
            // The label kept beside the terminal, never the terminal itself (`TermLabel`).
            match root.sessions.labels.get(session) {
                Some(t) => {
                    let mark = TabMark::of(root.sessions.states.get(session).copied(), t.alive);
                    (tab_label(&t.title, &root.shell_name, t.alive, t.exit_code, note, busy), mark)
                }
                None => (tab_label("", &root.shell_name, true, None, note, busy.or(Some("starting shell"))), TabMark::None),
            }
        }
        Tab::Viewer { path, .. } | Tab::Video { path, .. } => (path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), TabMark::None),
        Tab::Browser { url } => (p.ids.get(idx).and_then(|id| root.web_titles.get(id)).cloned().unwrap_or_else(|| crate::web::title_for(None, url)), TabMark::None),
    }
}

/// How the terminal decodes: under an effect that decodes it, longer while its session works and
/// at once with Reduce Motion or while the window is `paused` (so output appears plainly and no
/// cell is left mid-scramble); None without one.
pub fn decode_timing(style_decode: bool, state: Option<protocol::ClaudeState>, reduce: bool, paused: bool) -> Option<decode::Timing> {
    style_decode.then(|| decode::timing(state == Some(protocol::ClaudeState::Working), reduce || paused))
}

pub fn render_pane(root: &mut Root, pane: PaneId, rect: Rect, window: &mut Window, cx: &mut Context<Root>) -> Div {
    let style = root.effect_style();
    let focused = root.tree.focused == pane;
    let theme = root.theme.clone();
    let Some(p) = root.tree.pane(pane).cloned() else { return div() };
    // `pane_order` is computed once per render by `render_tree` (reading order, structural).
    let number = root.pane_order.iter().position(|id| *id == pane).map(|i| i + 1).unwrap_or(0);
    let tabs = tab_labels(root, &p, cx);
    let bar_h = TAB_BAR_H;
    // The invitation hangs under the first pane's tab bar, above the body, which gives up its
    // height: a terminal measures the grid it is left.
    let strip = (root.invite_pane == Some(pane)).then(|| render_invitation(root, cx));
    let body_top = bar_h + if strip.is_some() { INVITE_H } else { 0.0 };
    let bar = render_tab_bar(root, pane, &p, number, &tabs, bar_h, cx);
    if !p.tabs.is_empty() {
        // A pane that shows tabs forgets its empty-state animation, so emptying it again replays.
        root.empty_anim.remove(&pane);
    }
    let body = match p.tabs.get(p.active) {
        None => render_no_tabs(root, pane, rect, window, cx),
        Some(Tab::Terminal { session, .. }) => match root.sessions.terminals.get(session).cloned() {
            Some(terminal) => {
                let family = root.family();
                let now = root.frame_now;
                let (reduce, paused) = (motion::reduce_motion(), root.effects_paused);
                let decode = decode_timing(style.terminal_decode, root.sessions.states.get(session).copied(), reduce, paused).map(|t| t.toward(theme.chrome.focus_ring).on_ground(theme.is_light()));
                let ink = decode.map(Ink::Decode).or_else(|| crate::effects::paint::paint_clock(style.terminal_paint, reduce, paused, root.glyph_now).map(Ink::Paint));
                render_terminal(terminal, pane, family, &theme, style, ink, root.draws, &mut root.find_bars, now, cx)
            }
            None => render_busy(root, pane, p.ids.get(p.active).copied(), cx),
        },
        Some(Tab::Viewer { path, .. }) => match p.ids.get(p.active).copied() {
            Some(tab) => render_viewer(root, pane, tab, path, Rect { h: rect.h - body_top, ..rect }, cx),
            None => div(),
        },
        Some(Tab::Browser { url }) => match p.ids.get(p.active).copied() {
            Some(tab) => render_browser(root, pane, tab, url.clone(), cx),
            None => div(),
        },
        Some(Tab::Video { path, .. }) => match p.ids.get(p.active).copied() {
            Some(tab) => render_video(root, pane, tab, path, cx),
            None => div(),
        },
    };
    // Every pane measures the grid a terminal would get, whatever it shows, so a restore into
    // any of its tabs always gets a size (it paints nothing).
    let weak = cx.weak_entity();
    let (font_size, line_height) = (root.font_size, root.line_height);
    let family = root.config.font.family.clone();
    let measure = canvas(
        move |bounds, window, cx| {
            let grid = grid_for(bounds, window, font_size, line_height, &family);
            let _ = weak.update(cx, |root, _| root.report_grid(pane, grid));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    // The five drop zones of a dragged tab: the hovered one is tinted. The tint fades in and
    // out and glides between zones; the drop itself reads `drop_target`, never the tint.
    let now = root.frame_now;
    let zone = match root.drop_target {
        Some(drag::DropTarget::Pane { pane: p, zone }) if p == pane => Some(zone),
        _ => None,
    };
    let painted = if zone.is_some() || root.drop_tints.contains_key(&pane) {
        let tint = root.drop_tints.entry(pane).or_default();
        tint.aim(zone, now);
        tint.paint(rect.w, rect.h - body_top, now)
    } else {
        None
    };
    let overlay = painted.map(|((x, y, w, h), opacity)| div().absolute().left(px(x)).top(px(y)).w(px(w)).h(px(h)).opacity(opacity).bg(theme.chrome.focus_ring.alpha(0.4)));
    let content = div()
        .id(("body", pane))
        .flex_1()
        .min_h_0()
        .relative()
        .on_drag_move(cx.listener(move |root, ev: &DragMoveEvent<drag::TabDrag>, _window, cx| {
            // gpui calls every drag-move listener, hovered or not.
            if ev.bounds.contains(&ev.event.position) {
                let zone = drag::zone_for(ev.bounds, ev.event.position);
                root.drop_target = Some(drag::DropTarget::Pane { pane, zone });
                cx.notify();
            }
        }))
        .on_drop(cx.listener(move |root, d: &drag::TabDrag, _window, cx| match root.drop_target {
            Some(target @ drag::DropTarget::Pane { pane: p, .. }) if p == pane => root.drop_tab(d, target, cx),
            _ => {
                root.drop_target = None;
                cx.notify();
            }
        }))
        .child(measure)
        .child(body)
        .children(overlay);
    // The focused pane glows at its edges instead of wearing a hard line: four strips that fade
    // from the focus colour to nothing, drawn over the pane so they never move its content, and
    // anchored to the pane's own edges (the pane wears a 1 px border, so offsets from the layout
    // rect would land past the clip). When focus moves, the old pane's glow fades out as the new
    // one's fades in. The glow breathes slowly, brighter and a little wider at the peak.
    // A window not in front rests: it repaints once on deactivation, and a breath caught
    // mid-way would freeze there.
    let breath = if root.window_active { root.breath.glow(now, crate::motion::reduce_motion()) } else { 0.0 };
    let glow = root.glow.sync(pane, focused.then_some(()), now).map(|(_, opacity)| {
        let g = glow_width(rect.w, rect.h, breath);
        let mut from = theme.chrome.focus_ring.alpha(GLOW_ALPHA * (GLOW_BREATH_FLOOR + (1.0 - GLOW_BREATH_FLOOR) * breath));
        // Light at the peak: away from the window's background, so a light theme breathes too.
        let lift = GLOW_BREATH_LIGHT * breath;
        from.l = if theme.chrome.window_bg.hsla().l > 0.5 { (from.l - lift).max(0.0) } else { (from.l + lift).min(1.0) };
        let mut to = from;
        to.a = 0.0;
        // The angle is CSS's: the direction the colour fades towards (180 is downwards).
        let strip = |angle: f32| div().absolute().opacity(opacity).bg(linear_gradient(angle, linear_color_stop(from, 0.0), linear_color_stop(to, 1.0)));
        let z = px(0.0);
        // A pane the effect draws as a rounded card wears a ring of the same rounding instead,
        // as its focus: straight strips would run past the card's corners onto the ground.
        if style.pane_radius > 0.0 {
            let mut ring = from;
            ring.a = (ring.a * 2.0).min(1.0);
            return vec![div().absolute().inset_0().opacity(opacity).rounded(px(style.pane_radius)).border(px(FOCUS_RING_W)).border_color(ring)];
        }
        vec![
            strip(180.0).top(z).left(z).right(z).h(px(g)),
            strip(0.0).bottom(z).left(z).right(z).h(px(g)),
            strip(90.0).left(z).top(z).bottom(z).w(px(g)),
            strip(270.0).right(z).top(z).bottom(z).w(px(g)),
        ]
    });
    div()
        .size_full()
        .relative()
        .flex()
        .flex_col()
        .bg(theme.chrome.window_bg.alpha(style.ground_alpha))
        .border_1()
        // An effect that inks the borders draws the pane's outline itself.
        .border_color(if style.ink_borders { theme.chrome.border.alpha(0.0) } else { theme.chrome.border.hsla() })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |root, _ev: &MouseDownEvent, _window, cx| {
                root.focus_pane(pane, cx);
            }),
        )
        .child(bar)
        .children(strip)
        .child(content)
        .children(render_url_prompt(root, pane, bar_h, cx))
        .children(glow.into_iter().flatten())
}

/// The invitation strip's height: one line, whatever the pane's width. A narrow pane cuts the
/// words short and keeps the buttons, so the body below never gives up more than this.
pub const INVITE_H: f32 = 24.0;

/// The invitation to turn usage reports on (telemetry spec, section 3.3), drawn as a viewer
/// tab's banners are (dim words, buttons as text in the text colour) on the window banners'
/// background, so it reads as the app's own and not as a terminal's output. Turn on and Not now
/// answer it for good; What is sent opens the disclosure page in a viewer tab and leaves it up.
fn render_invitation(root: &mut Root, cx: &mut Context<Root>) -> Div {
    let style = root.effect_style();
    let theme = root.theme.clone();
    let c = &theme.chrome;
    let text = c.text.hsla();
    let button = |id: &'static str, label: &'static str| div().id(id).flex_none().text_color(text).cursor_pointer().hover(|s| s.underline()).child(label);
    div()
        .flex_none()
        .h(px(INVITE_H))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .px(px(12.0))
        .bg(c.banner.hsla())
        .text_color(c.text_dim.hsla())
        .font_family(root.ui_family())
        .text_size(px(style.ui(12.0)))
        .child(div().flex_1().min_w_0().truncate().child(crate::telemetry::INVITATION))
        .child(button("invite-turn-on", crate::telemetry::TURN_ON).on_click(cx.listener(|root, _: &ClickEvent, _window, cx| root.answer_invitation(crate::telemetry::Answer::TurnOn, cx))))
        .child(button("invite-not-now", crate::telemetry::NOT_NOW).on_click(cx.listener(|root, _: &ClickEvent, _window, cx| root.answer_invitation(crate::telemetry::Answer::NotNow, cx))))
        .child(button("invite-what-is-sent", crate::telemetry::WHAT_IS_SENT).on_click(cx.listener(|root, _: &ClickEvent, _window, cx| root.open_disclosure(cx))))
}

/// The tab bar, `height` tall: the pane number, a tab per entry and the +;
/// every tab clicks, drags and takes drops.
fn render_tab_bar(root: &mut Root, pane: PaneId, p: &Pane, number: usize, tabs: &[(String, TabMark)], height: f32, cx: &mut Context<Root>) -> Stateful<Div> {
    let style = root.effect_style();
    let theme = root.theme.clone();
    let c = &theme.chrome;
    let (fg, dim) = (c.text, c.text_dim);
    let (highlights, hovered) = (root.effect_highlights(), root.effect_hover);
    // One phase for the whole bar, so every working tab breathes together.
    let alpha = root.breath.alpha(root.frame_now, crate::motion::reduce_motion());
    let mut muted = dim.hsla();
    muted.a *= 0.6;
    let mut bar = div()
        .id(("bar", pane))
        .h(px(height))
        .w_full()
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .bg(c.tab_bar.alpha(style.chrome_alpha))
        .text_color(fg.hsla())
        .font_family(root.ui_family())
        .text_size(px(style.ui(12.0)))
        .child(match number_slot(style) {
            // Painted with its ink centred on the slot, which is the chip's place.
            Some((inset, width)) => {
                let (label, family, size, color): (gpui::SharedString, _, _, _) = (format!("{number}").into(), root.ui_family(), style.ui(12.0), dim.hsla());
                div().flex_none().ml(px(inset)).mr(px(6.0)).w(px(width)).h_full().child(gpui::canvas(|_, _, _| (), move |b, _, window, cx| paint_centred(&label, family.clone(), size, color, b, window, cx)).size_full())
            }
            None => div().px(px(8.0)).text_color(dim.hsla()).child(format!("{number}")),
        })
        .on_drag_move(cx.listener(move |root, ev: &DragMoveEvent<drag::TabDrag>, _window, cx| {
            // Off every tab, the bar appends. The bar's listener runs before its tabs', which
            // then claim the hover with an exact insertion point.
            if ev.bounds.contains(&ev.event.position) {
                let (from, tab) = { let d = ev.drag(cx); (d.pane, d.tab) };
                let at = if from == pane {
                    root.begin_reorder(pane, ev.drag(cx));
                    root.reorder_slot_for(pane, tab, ev.event.position)
                } else {
                    usize::MAX
                };
                root.drop_target = Some(drag::DropTarget::TabBar { pane, at });
                cx.notify();
            }
        }))
        .on_drop(cx.listener(move |root, d: &drag::TabDrag, _window, cx| {
            let target = match root.drop_target {
                Some(t @ drag::DropTarget::TabBar { pane: p, .. }) if p == pane => t,
                // `move_tab` clamps, so this appends.
                _ => drag::DropTarget::TabBar { pane, at: usize::MAX },
            };
            root.drop_tab(d, target, cx);
        }));
    let handle = root.bar_scroll.entry(pane).or_default().clone();
    let rename = root.tab_rename.as_ref().filter(|r| r.pane == pane).map(|r| (r.tab, r.edit.clone()));
    let rename_focus = root.tab_rename_focus.clone();
    let family = root.ui_family();
    let mut strip = div().id(("tabs", pane)).flex_1().min_w_0().h_full().flex().flex_row().overflow_x_scroll().track_scroll(&handle);
    // A tab dragged within this bar: the gap follows the drop target's insertion point.
    let now = root.frame_now;
    let at = match root.drop_target {
        Some(drag::DropTarget::TabBar { pane: target, at }) if target == pane => Some(at.min(p.tabs.len())),
        _ => None,
    };
    if let Some(r) = root.reorder.as_mut().filter(|r| r.pane == pane) {
        r.aim(at, now);
    }
    let reorder = root.reorder.as_ref().filter(|r| r.pane == pane);
    let gaps: Vec<f32> = (0..=p.tabs.len()).map(|slot| reorder.map_or(0.0, |r| r.gap_before(slot, now))).collect();
    let collapsed = reorder.map(|r| (r.tab, r.slot_width(now)));
    for (idx, (label, mark)) in tabs.iter().enumerate() {
        let (label, mark) = (label.clone(), *mark);
        if gaps[idx] > 0.5 {
            strip = strip.child(div().flex_none().h_full().w(px(gaps[idx])));
        }
        let tab_id_for_width = p.ids.get(idx).copied();
        let slot_width = collapsed.filter(|(tab, _)| Some(*tab) == tab_id_for_width).map(|(_, w)| w);
        // The title under edit: an editor in its place, holding the keys until Enter or Escape.
        let renaming = rename.as_ref().filter(|(tab, _)| p.ids.get(idx) == Some(tab)).map(|(_, edit)| edit.clone());
        let is_active = idx == p.active;
        // A preview tab stays dim even when active, so it reads as temporary (ruling 4).
        let is_preview = root.preview_tab.is_some() && p.ids.get(idx).copied() == root.preview_tab;
        let glyph = match mark {
            TabMark::Working => {
                let mut c = theme.semantic.working.hsla();
                c.a = alpha;
                Some(div().size(px(7.0)).rounded_full().bg(c))
            }
            TabMark::Waiting => Some(div().size(px(7.0)).rounded_full().bg(theme.semantic.waiting.hsla())),
            TabMark::None | TabMark::Dead => None,
        };
        let text = match mark {
            TabMark::Dead => muted,
            _ if is_active && !is_preview => fg.hsla(),
            _ => dim.hsla(),
        };
        let drag_value = p.ids.get(idx).map(|id| drag::TabDrag {
            pane,
            idx,
            tab: *id,
            label: label.clone(),
            position: Default::default(),
            bg: c.banner,
            fg,
            family: root.ui_family(),
        });
        let tab = div()
            .id(tab_id("tab", pane, idx))
            .h_full()
            .flex()
            .items_center()
            .cursor_pointer()
            // One click activates; a double click renames in place.
            .on_click(cx.listener(move |root, ev: &ClickEvent, window, cx| {
                if ev.click_count() == 2 {
                    root.begin_tab_rename(pane, idx, window, cx);
                } else {
                    root.activate_tab(pane, idx, cx);
                }
            }))
            .when_some(drag_value, |el, value| {
                el.on_drag(value, |d: &drag::TabDrag, position, _window, cx| cx.new(|_| d.clone().at(position)))
            })
            .on_drag_move(cx.listener(move |root, ev: &DragMoveEvent<drag::TabDrag>, _window, cx| {
                // A tab from this bar is placed by the bar itself (the gap moves the tabs).
                if ev.bounds.contains(&ev.event.position) && ev.drag(cx).pane != pane {
                    let at = drag::insert_index(ev.bounds, ev.event.position, idx);
                    root.drop_target = Some(drag::DropTarget::TabBar { pane, at });
                    cx.notify();
                }
            }))
            // Under an effect that paints the highlights, a hovered tab is a stroke the effect
            // fades in and out like an Explorer row's.
            .when_some(tab_id_for_width.filter(|_| highlights), |el, id| {
                let w_hover = cx.weak_entity();
                el.on_hover(move |over: &bool, _window, cx| {
                    let _ = w_hover.update(cx, |root, cx| {
                        root.effect_hover = blocks::hover_after(root.effect_hover, blocks::tab_key(id), *over);
                        cx.notify();
                    });
                })
            })
            // The tab's painted width, for the gap a reorder opens where it would land.
            .child(canvas(
                {
                    let weak = cx.weak_entity();
                    move |bounds, _window, cx| {
                        // The hovered tab reports where its stroke goes.
                        if let Some(key) = tab_id_for_width.map(blocks::tab_key).filter(|key| highlights && hovered == Some(*key)) {
                            let rect = Rect { x: f32::from(bounds.origin.x), y: f32::from(bounds.origin.y), w: f32::from(bounds.size.width), h: f32::from(bounds.size.height) };
                            let _ = weak.update(cx, |root, _| {
                                root.effect_marks.push(blocks::Mark { key, rect, kind: blocks::Kind::TabHighlight, state: blocks::HighlightState::Hovered });
                            });
                        }
                        // The active tab reports where it is, so the wash can mark it.
                        if let Some(id) = tab_id_for_width.filter(|_| highlights && is_active) {
                            let rect = Rect { x: f32::from(bounds.origin.x), y: f32::from(bounds.origin.y), w: f32::from(bounds.size.width), h: f32::from(bounds.size.height) };
                            let _ = weak.update(cx, |root, _| {
                                root.effect_shapes.push(blocks::Shape { key: blocks::tab_key(id), rect, kind: blocks::Kind::ActiveTab, pitch: 0.0 });
                            });
                        }
                        // Not while collapsed: the width on record is the tab's own.
                        if let Some(id) = tab_id_for_width.filter(|_| slot_width.is_none()) {
                            let _ = weak.update(cx, |root, _| {
                                root.tab_widths.insert(id, f32::from(bounds.size.width));
                            });
                        }
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full())
            .when_some(slot_width, |el, w| el.flex_none().w(px(w)).overflow_hidden());
        // The collapsed slot has no padding either: padding would hold it open.
        let pad_x = if slot_width.is_some() { px(0.0) } else { px(10.0) };
        strip = strip.child(
            tab.px(pad_x)
                .gap(if slot_width.is_some() { px(0.0) } else { px(6.0) })
                // Under an effect the wash is the bar's colour; the active tab is a lighter patch on it.
                .bg(if is_active { c.tab_active.alpha(style.active_tab_alpha) } else { c.tab_inactive.alpha(style.chrome_alpha) })
                .text_color(text)
                .when_some(glyph, |el, g| el.child(g))
                .child(match renaming {
                    Some(edit) => div()
                        .key_context("TabRename")
                        .track_focus(&rename_focus)
                        .w(px(150.0))
                        .overflow_hidden()
                        // A press in the field is not a press on the tab; it still closes an open menu,
                        // as the window's press would have.
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|root, _: &MouseDownEvent, _window, cx| {
                                let now = std::time::Instant::now();
                                root.md_menu.hide(now);
                                root.view_menu.hide(now);
                                cx.stop_propagation();
                            }),
                        )
                        .on_key_down(cx.listener(|root, ev: &KeyDownEvent, _window, cx| root.tab_rename_key(&ev.keystroke, cx)))
                        .child(line_edit::render(&edit, "name", &theme, family.clone(), 12.0))
                        .into_any_element(),
                    None => div().child(label).into_any_element(),
                })
                .child(
                    div()
                        .id(tab_id("close", pane, idx))
                        .text_color(dim.hsla())
                        .hover(move |s| s.text_color(fg.hsla()))
                        .child("×")
                        .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                            // The tab underneath would otherwise activate the index the close just freed.
                            cx.stop_propagation();
                            root.close_tab(pane, idx, cx);
                        })),
                ),
        );
    }
    // A gap past the last tab: the dragged tab would land at the end.
    if gaps[p.tabs.len()] > 0.5 {
        strip = strip.child(div().flex_none().h_full().w(px(gaps[p.tabs.len()])));
    }
    bar = bar.child(strip);
    for item in bar_items() {
        bar = match item {
            "+" => bar.child(
                div()
                    .id(("plus", pane))
                    .px(px(10.0))
                    .text_color(dim.hsla())
                    .hover(move |s| s.text_color(fg.hsla()))
                    .cursor_pointer()
                    .child("+")
                    .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| root.new_terminal_tab(pane, cx))),
            ),
            // `bar_items` names only "+": a name the bar does not draw adds nothing.
            _ => bar,
        };
    }
    bar
}

/// Opens the terminal's menu when the press asks for it (see `link_menu::press_kind`) and
/// returns whether it did: a right press on a link or on the selection, which is also what gpui
/// delivers for a Control+click. The link and the selection are the ones under the press now,
/// not the ones the last paint saw. The
/// event stops here, which skips the root's mouse-downs, so what they would close is closed here
/// instead: the other menus and the popovers (one floating card at a time), the URL prompt, and
/// a tab rename, which keeps what was typed. A press that does not open the menu is left alone
/// for the caller.
fn open_link_menu(root: &mut Root, terminal: &Entity<Terminal>, ev: &MouseDownEvent, cx: &mut Context<Root>) -> bool {
    // Only a press that could open the menu looks for a link or the selection.
    let (link, selection) = if ev.button == MouseButton::Right {
        let t = terminal.read(cx);
        (t.link_at_position(ev.position), t.selection_at_position(ev.position))
    } else {
        (None, false)
    };
    if press_kind(ev.button, link.is_some(), selection) != PressKind::Menu {
        return false;
    }
    cx.stop_propagation();
    let now = std::time::Instant::now();
    root.last_input = now;
    root.context_menu.hide(now);
    root.wt_menu.hide(now);
    root.view_menu.hide(now);
    root.close_popovers(now);
    root.url_prompt.hide(now);
    root.commit_tab_rename(cx);
    root.link_menu.show(LinkMenu { url: link.map(|l| l.url), selection, terminal: terminal.downgrade(), at: ev.position }, now);
    cx.notify();
    true
}

fn render_terminal(terminal: Entity<Terminal>, pane: PaneId, family: SharedString, theme: &Theme, style: Style, ink: Option<Ink>, draw: u64, find_bars: &mut motion::Fades<gpui::EntityId, find::FindLook>, now: std::time::Instant, cx: &mut Context<Root>) -> Div {
    let focus = terminal.read(cx).focus.clone();
    let (font_size, line_height) = {
        let t = terminal.read(cx);
        (t.font_size, t.line_height)
    };
    let t_keys = terminal.clone();
    let t_wheel = terminal.clone();
    let t_click = terminal.clone();
    let t_menu = terminal.clone();
    let t_up = terminal.clone();
    let t_up_out = terminal.clone();
    let t_rup = terminal.clone();
    let t_rup_out = terminal.clone();
    let t_move = terminal.clone();
    let t_drop = terminal.clone();
    let scrolled_up = terminal.read(cx).core.scrolled_up();
    let jump_id = ElementId::NamedInteger("jump".into(), terminal.read(cx).session.0 as u64);
    div()
        .size_full()
        .relative()
        .key_context("Terminal")
        .track_focus(&focus)
        .on_key_down(cx.listener(move |root, ev: &KeyDownEvent, _window, cx| {
            let ks = &ev.keystroke;
            // Shift+Cmd+C is "copy as shown", an action that never gets here while it is bound;
            // rebound elsewhere, it does nothing rather than Cmd+C's re-flowed copy.
            if ks.modifiers.platform && ks.modifiers.shift && ks.key == "c" {
                return;
            }
            // Cmd+C copies the selection and nothing else: Ctrl+C stays the interrupt. The lines
            // a program wrapped for this width come out joined (`TerminalCore::copy_text`).
            if ks.modifiers.platform && ks.key == "c" {
                if let Some(text) = t_keys.read(cx).core.copy_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                return;
            }
            if ks.modifiers.platform && ks.key == "v" {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    t_keys.update(cx, |t, _| t.paste(&text));
                }
                return;
            }
            if ks.modifiers.platform {
                // Cmd combinations are actions (Task 6) or nothing; never terminal bytes.
                return;
            }
            let session = t_keys.read(cx).session;
            if root.sessions.notes.remove(&session).is_some() | root.sessions.acknowledge(session) {
                cx.notify();
            }
            let option_as_meta = root.config.terminal.option_as_meta;
            t_keys.update(cx, |t, cx| t.handle_key(ks, option_as_meta, cx));
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_root, ev: &MouseDownEvent, _window, cx| {
                // Cmd+click on a link opens it (Terminal.app's gesture); the plain click stays
                // free for selection later. A Control+click never gets here: gpui delivers it as
                // a right press, which the listener below takes.
                if ev.modifiers.platform {
                    if let Some(link) = t_click.read(cx).hovered_link.clone() {
                        cx.open_url(&link.url);
                        t_click.update(cx, |t, _| t.mouse_cancelled());
                        return;
                    }
                }
                // A click back into the terminal closes the find bar.
                t_click.update(cx, |t, cx| {
                    if t.find.is_some() {
                        t.close_find(cx);
                    }
                    // One click clears the selection, two select a word, three a line; a program
                    // reporting the mouse hears a single click at the release.
                    t.mouse_pressed(ev.position, ev.click_count, ev.first_mouse, cx);
                });
            }),
        )
        // A right-click on a link or the selection opens the menu, and so does a Control+click, which gpui
        // delivers as a right press; anywhere else it does nothing and the event goes on to the
        // root, which closes whatever menu was open.
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |root, ev: &MouseDownEvent, _window, cx| {
                open_link_menu(root, &t_menu, ev, cx);
            }),
        )
        .on_mouse_up(MouseButton::Left, cx.listener(move |_root, ev: &MouseUpEvent, _window, cx| t_up.update(cx, |t, _| t.mouse_released(ev.position))))
        // The button came up outside the terminal: the program hears nothing of the press.
        .on_mouse_up_out(MouseButton::Left, cx.listener(move |_root, _ev: &MouseUpEvent, _window, cx| t_up_out.update(cx, |t, _| t.mouse_cancelled())))
        // gpui reports a left release made with Control held as a right release, and a
        // right-click in the middle of a drag leaves the left press held, so a right release
        // anywhere ends the drag too; otherwise auto-scroll could run with the button up.
        .on_mouse_up(MouseButton::Right, cx.listener(move |_root, _ev: &MouseUpEvent, _window, cx| t_rup.update(cx, |t, _| t.mouse_cancelled())))
        .on_mouse_up_out(MouseButton::Right, cx.listener(move |_root, _ev: &MouseUpEvent, _window, cx| t_rup_out.update(cx, |t, _| t.mouse_cancelled())))
        // A file dragged from the tree types its path at the prompt.
        .on_drop(cx.listener(move |_root, d: &crate::drag::FileDrag, _window, cx| {
            match crate::drag::paste_text(&d.path) {
                Some(text) => t_drop.update(cx, |t, _| t.paste(&text)),
                None => eprintln!("arkos: not pasting a path with a control character"),
            }
        }))
        // The element finds the link under the pointer when it paints; a move over the
        // terminal asks for that paint (the inner div below asks on enter and exit). A move
        // with the button down extends the selection.
        .on_mouse_move(cx.listener(move |_root, ev: &MouseMoveEvent, _window, cx| {
            t_move.update(cx, |t, cx| t.mouse_moved(ev.position, ev.pressed_button == Some(MouseButton::Left), cx));
            cx.notify();
        }))
        .on_scroll_wheel(cx.listener(move |_root, ev: &ScrollWheelEvent, _window, cx| {
            t_wheel.update(cx, |t, cx| {
                let cell_h = t.cell.map(|c| c.height).unwrap_or(24.0);
                // Positive wheel y means the content moves down, which is scrolling up into history.
                // macOS turns a Shift+wheel on a mouse into a horizontal step, so Shift reads x
                // when y is still.
                let (x, y, precise) = match ev.delta {
                    ScrollDelta::Lines(p) => (p.x, p.y, false),
                    ScrollDelta::Pixels(p) => (f32::from(p.x) / cell_h, f32::from(p.y) / cell_h, true),
                };
                let lines = if ev.modifiers.shift && y == 0.0 { x } else { y };
                t.wheel(lines, precise, ev.modifiers.shift, ev.position, cx);
            });
        }))
        .child(
            div()
                .id(("term-hover", pane))
                .size_full()
                .px(px(terminal_inset(style)))
                .on_hover(cx.listener(|_root, _hovered: &bool, _window, cx| cx.notify()))
                .child(TerminalElement::new(terminal.clone(), font_size, line_height, family.clone(), style.paper_alpha, ink, draw, (style.paper_alpha < 1.0).then(|| cx.weak_entity())).paper_radius(paper_radius(style))),
        )
        .children(scrolled_up.then(|| {
            let t = terminal.clone();
            div()
                .id(jump_id)
                .absolute()
                .bottom(px(10.0))
                .right(px(14.0))
                .px(px(10.0))
                .py(px(4.0))
                .rounded(px(10.0))
                .bg(pill_bg(&theme))
                .border_1()
                .border_color(pill_border(&theme))
                .text_color(theme.chrome.text.hsla())
                .font_family(family.clone())
                .text_size(px(style.ui(12.0)))
                .cursor_pointer()
                .child("jump to bottom")
                // The terminal's own mouse-down closes an open find bar; the pill is not a click
                // in the terminal. Stopping it also stops the pane's, so the pill focuses the pane.
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |root, _ev: &MouseDownEvent, _window, cx| pill_press(root, pane, cx)),
                )
                .on_click(cx.listener(move |_root, _ev: &ClickEvent, _window, cx| t.update(cx, |t, cx| t.jump_to_bottom(cx))))
        }))
        .children(find::render(&terminal, pane, theme, style, family, find_bars, now, cx))
}

/// A press on a pill or chip floating over a pane's text: not a click in the text below, so it
/// stops there (the pane's own press would move the caret or close the find bar), which also
/// stops the window's press; the menus that press would close are closed here, and the pill
/// focuses the pane itself.
fn pill_press(root: &mut Root, pane: PaneId, cx: &mut Context<Root>) {
    let now = std::time::Instant::now();
    root.md_menu.hide(now);
    root.view_menu.hide(now);
    root.focus_pane(pane, cx);
    cx.stop_propagation();
}

/// The width of a delimited file's mode pill: fixed, so the inline-diff chip can sit beside it.
pub const PILL_W: f32 = 104.0;
/// The width of the inline-diff chip: fixed, so a tab's banners can keep clear of it.
pub const CHIP_W: f32 = 100.0;

/// The strip above a grid: the caps' banner at its left and the mode pill at its right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Strip {
    pub banner: Option<String>,
    pub pill: bool,
}

/// The key context of a viewer tab: `Markdown` adds the formatting keys for a markdown document,
/// `Sheets` the sheet keys for a workbook of more than one sheet (which has no document, so the
/// two never meet).
pub fn viewer_key_context(has_doc: bool, markdown: bool, sheets: usize) -> &'static str {
    if has_doc && markdown {
        "Viewer Markdown"
    } else if sheets > 1 {
        "Viewer Sheets"
    } else {
        "Viewer"
    }
}

/// The strip of sheet tabs under a workbook's grid: the tab bar's family and ground, the active
/// sheet underlined in the focus ring, a long strip scrolling sideways. A click shows the sheet.
fn sheet_strip(viewer: &gpui::Entity<viewer::Viewer>, names: &[String], active: usize, theme: &Theme, style: Style, family: gpui::SharedString, cx: &mut Context<Root>) -> Stateful<Div> {
    let c = &theme.chrome;
    let mut strip = div()
        .id(("sheet-strip", viewer.entity_id()))
        .flex_none()
        .w_full()
        .flex()
        .flex_row()
        .overflow_x_scroll()
        .bg(c.tab_bar.alpha(style.chrome_alpha))
        .font_family(family)
        .text_size(px(style.ui(12.0)));
    for (i, name) in names.iter().enumerate() {
        let v = viewer.clone();
        let on = i == active;
        strip = strip.child(
            div()
                .id(("sheet-tab", i))
                .flex_none()
                .px(px(10.0))
                .py(px(5.0))
                .border_b_2()
                .border_color(if on { c.focus_ring.hsla() } else { gpui::transparent_black() })
                .text_color(if on { c.text.hsla() } else { c.text_dim.hsla() })
                .cursor_pointer()
                .child(name.clone())
                .on_click(cx.listener(move |_root, _ev: &ClickEvent, _window, cx| v.update(cx, |v, cx| v.set_sheet(i, cx)))),
        );
    }
    strip
}

/// What the strip above a grid holds: a delimited file's always has the pill that switches it to
/// its text, a workbook's only the banner, and a workbook with nothing dropped has no strip.
pub fn grid_strip(delimited: bool, banner: Option<String>) -> Option<Strip> {
    (delimited || banner.is_some()).then_some(Strip { banner, pill: delimited })
}

/// How far from the pane's right edge a viewer tab's banners keep their text, clear of what is
/// drawn over the top right: a markdown tab's view button (`button`), or a raw CSV's mode pill
/// (`pill`) and another file's inline-diff chip (`chip`), the chip left of the pill.
pub fn banner_right_pad(button: bool, pill: bool, chip: bool) -> f32 {
    if button {
        return crate::view_menu::RIGHT + crate::view_menu::BUTTON + 8.0;
    }
    match (pill, chip) {
        (true, true) => 14.0 + PILL_W + 8.0 + CHIP_W + 8.0,
        (true, false) => 14.0 + PILL_W + 8.0,
        (false, true) => 14.0 + CHIP_W + 8.0,
        (false, false) => 12.0,
    }
}

/// The overlay pills ("jump to bottom", a markdown tab's `⋯`) float over text: a translucent banner
/// colour with a faint edge, so what lies under them still shows through. (gpui 0.2 blurs only
/// a whole window's backdrop, not an element's, so this is transparency alone.)
fn pill_bg(theme: &Theme) -> gpui::Hsla {
    let mut c = theme.chrome.banner.hsla();
    c.a = 0.72;
    c
}

fn pill_border(theme: &Theme) -> gpui::Hsla {
    let mut c = theme.chrome.border.hsla();
    c.a = 0.6;
    c
}

/// What a viewer tab shows under its find bar.
enum Kind {
    Loading,
    Text { plain: Option<crate::viewer::Plain> },
    /// The grid of a book's active sheet, with the banner for what its caps dropped.
    Grid { banner: Option<String> },
    TooLarge(u64),
    Binary,
    Unreadable(String),
}

/// A viewer tab: the file's lines through `ViewerElement`, or a notice when there are none to show.
/// A document (markdown, or any text file the user may write) takes the editing keys
/// (`Viewer::key`) and nothing else scrolls on a key; in a read-only file the arrows, Page Up and
/// Down, Space, Home and End scroll. The wheel scrolls smoothly. A markdown document shows
/// rendered (`md_view`, a list that scrolls itself) or raw, with a `⋯` button at the top right
/// whose menu (`view_menu`) switches between the two and holds its other view toggles; a code
/// document shows raw with its syntax colours. A delimited file shows its grid in Rendered mode,
/// with a pill in the strip above it that switches to its text, and a workbook its grid always,
/// with no pill; while a grid shows, the keys scroll it.
/// `area` is the body's: the pane's left edge and width in window points, the height under the
/// tab bar.
fn render_viewer(root: &mut Root, pane: PaneId, tab: TabId, path: &std::path::Path, area: Rect, cx: &mut Context<Root>) -> Div {
    let style = root.effect_style();
    let viewer = root.viewer_for(tab, path, cx);
    let theme = root.theme.clone();
    let family = root.family();
    let presenting = viewer.read(cx).presenting;
    let markdown = viewer::is_markdown(path);
    let (focus, kind, font_size, has_changes, inline, find_open, mode, editable, has_doc, conflict, save_error, read_only, pill_shown, delimited, sheet_count, outline_on, has_heads) = {
        let v = viewer.read(cx);
        let kind = match &v.content {
            _ if !v.loaded => Kind::Loading,
            // A book shows its grid before its content: a workbook's is `Binary`.
            _ if v.grid_shown() => Kind::Grid { banner: v.sheet().and_then(crate::grid::banner) },
            Content::Text { .. } => Kind::Text { plain: v.plain },
            Content::TooLarge(n) => Kind::TooLarge(*n),
            Content::Binary => Kind::Binary,
            Content::Unreadable(e) => Kind::Unreadable(e.clone()),
        };
        // A file shown read-only (`read_only` set) has no document: raw text, no caret.
        let editable = v.doc.is_some() && v.read_only.is_none();
        let pill_shown = viewer::pill_shown(path, editable, v.book.as_ref());
        let delimited = v.book.as_ref().is_some_and(|b| matches!(b.source, crate::grid::Source::Delimited(_)));
        (v.focus.clone(), kind, v.font_size, v.diff.as_ref().is_some_and(|d| d.has_changes()), v.inline_diff, v.find.is_some(), v.mode, editable, v.doc.is_some(), v.conflict, v.save_error.clone(), v.read_only, pill_shown, delimited, v.sheet_count(), v.outline, !v.heads.is_empty())
    };
    // The rendered page and presentation are markdown's; a delimited file's pill switches to its
    // grid instead.
    let rendered = editable && markdown && mode == ViewMode::Rendered;
    let dim = theme.chrome.text_dim.hsla();
    // A notice wraps to the pane's width, centred, so a long reason reads whole in a narrow pane.
    let notice = |text: String| div().size_full().flex().items_center().justify_center().px(px(16.0)).text_color(dim).font_family(family.clone()).text_size(px(font_size)).child(div().max_w_full().text_center().child(text));
    let kind_is_text = matches!(kind, Kind::Text { .. });
    // A markdown document's outline rail, under the banners at the body's left edge, the
    // document to its right; in a narrow tab its open panel lies over the document.
    let outline = if markdown && has_doc && kind_is_text {
        let banners = usize::from(conflict) + usize::from(save_error.is_some()) + usize::from(read_only.is_some());
        let height = area.h - banners as f32 * (8.0 + style.ui(12.0) * 1.3);
        crate::outline_rail::render(root, &viewer, pane, tab, &theme, style, area.x, area.w, height, cx)
    } else {
        crate::outline_rail::Painted { rail: None, over: None }
    };
    let has_rail = outline.rail.is_some();
    let (kind_is_grid, grid_banner) = match &kind {
        Kind::Grid { banner } => (true, banner.clone()),
        _ => (false, None),
    };
    let body = match kind {
        Kind::Grid { .. } => {
            let element = GridElement::new(viewer.clone(), root.prose_family(), family.clone(), GridColors::new(&theme, style), style.ground_alpha, (style.ground_alpha < 1.0).then(|| cx.weak_entity()))
                .paper_alpha(style.document_paper_alpha)
                .paper_radius(paper_radius(style));
            // A workbook of several sheets has its tabs under the grid, which shrinks by their height.
            let sheets = { let v = viewer.read(cx); (v.sheet_count() > 1).then(|| (v.book.iter().flat_map(|b| b.sheets.iter().map(|s| s.name.clone())).collect::<Vec<_>>(), v.sheet)) };
            match sheets {
                Some((names, active)) => div().size_full().flex().flex_col().child(div().flex_1().min_h_0().child(element)).child(sheet_strip(&viewer, &names, active, &theme, style, root.ui_family(), cx)),
                None => div().size_full().child(element),
            }
        }
        Kind::Text { .. } if rendered => {
            let element = md_view::render(&viewer, &theme, root.prose_family(), family.clone(), font_size, root.highlighter.clone(), root.config.viewer.highlight_limit_bytes, root.config.viewer.wide_tables, root.config.viewer.page_width, style, cx.weak_entity(), cx);
            // Beside the rail the paper's bottom left corner is the rail's.
            div()
                .size_full()
                .rounded_br(px(paper_radius(style)))
                .when(!has_rail, |d| d.rounded_bl(px(paper_radius(style))))
                .bg(theme.chrome.window_bg.alpha(style.document_paper_alpha))
                .child(element)
        }
        Kind::Text { plain } => {
            let element = ViewerElement::new(
                viewer.clone(),
                family.clone(),
                theme.chrome.window_bg,
                theme.chrome.text_dim,
                theme.chrome.text,
                theme.semantic.find_match,
                theme.semantic.find_current,
                MarkColors { added: theme.tree.added, modified: theme.tree.modified, deleted: theme.tree.deleted },
                RawColors::opaque(theme.chrome.text, theme.chrome.text_dim, theme.tree.modified, theme.chrome.focus_ring),
            )
            .paper_alpha(style.document_paper_alpha)
            .paper_radius(paper_radius(style))
            .paper_left(!has_rail);
            div()
                .size_full()
                .flex()
                .flex_col()
                .children(plain.filter(|_| !has_doc).map(|why| div().flex_none().px(px(12.0)).py(px(4.0)).text_color(dim).font_family(family.clone()).text_size(px(style.ui(12.0))).child(why.banner())))
                .child(div().flex_1().min_h_0().child(element))
        }
        Kind::Loading => notice("loading".into()),
        Kind::TooLarge(n) => notice(format!("{} MB, too large to show", n / (1024 * 1024))),
        Kind::Binary => notice("a binary file; nothing to show".into()),
        Kind::Unreadable(e) => notice(format!("cannot read this file: {e}")),
    };
    let body = match outline {
        crate::outline_rail::Painted { rail: Some(rail), over } => div()
            .size_full()
            .relative()
            .flex()
            .flex_row()
            .child(rail.rounded_bl(px(paper_radius(style))))
            .child(div().flex_1().min_w_0().h_full().child(body))
            .children(over),
        _ => body,
    };
    // The banners above a text body: the file changed under unsaved edits, a save failed, or the
    // file is shown read-only. They share the plain-file banner's style; the buttons are text.
    let button = |id: &'static str, label: &'static str, on_click: Box<dyn Fn(&mut viewer::Viewer, &mut Context<viewer::Viewer>)>| {
        let v = viewer.clone();
        div()
            .id((id, viewer.entity_id()))
            .text_color(theme.chrome.text.hsla())
            .cursor_pointer()
            .hover(|s| s.underline())
            .child(label)
            .on_click(cx.listener(move |_root, _ev: &ClickEvent, _window, cx| v.update(cx, |v, cx| on_click(v, cx))))
    };
    // A markdown document's view toggles sit behind one button at the top right, in both views
    // and presenting (it is the way out); another file with changes has its inline-diff chip
    // there. While the grid shows, nothing floats over it: its pill sits in the strip above it.
    let view_button = markdown && has_doc && kind_is_text;
    let chip_shown = has_changes && !kind_is_grid && !view_button;
    // A delimited file's pill: in the strip over its grid, floating at the top right over its text.
    let csv_pill = pill_shown && !view_button;
    let right_pad = banner_right_pad(view_button, csv_pill && !kind_is_grid, chip_shown);
    let banner_row = || div().flex_none().flex().flex_row().gap(px(10.0)).pl(px(12.0)).pr(px(right_pad)).py(px(4.0)).text_color(dim).font_family(family.clone()).text_size(px(style.ui(12.0)));
    let mut banners: Vec<Div> = Vec::new();
    // A delimited document's grid shows the conflict and a failed save as its text does.
    if kind_is_text || kind_is_grid {
        if conflict {
            banners.push(
                banner_row()
                    .child("The file changed on disk")
                    .child(button("viewer-conflict-reload", "Reload", Box::new(|v, cx| v.resolve_conflict(false, cx))))
                    .child(button("viewer-conflict-keep", "Keep mine", Box::new(|v, cx| v.resolve_conflict(true, cx)))),
            );
        }
        if let Some(text) = save_error {
            banners.push(banner_row().child(text).child(button("viewer-save-retry", "Retry", Box::new(|v, cx| v.save(cx)))));
        }
    }
    if kind_is_text {
        if let Some(why) = read_only {
            let text = match why {
                viewer::ReadOnlyWhy::NotUtf8 => "not UTF-8: shown read-only",
                viewer::ReadOnlyWhy::MixedEndings => "mixed line endings: shown read-only",
                viewer::ReadOnlyWhy::NotWritable => "read-only file: shown read-only",
            };
            banners.push(banner_row().child(text));
        }
    }
    // The pill's top: where the find bar opens, below it when open.
    let pill_top = crate::view_menu::button_top(find_open);
    // A non-markdown file's inline diff switch, at the top right where the find bar opens, below
    // it when open; on a raw CSV, left of its pill at its top.
    let chip = chip_shown.then(|| {
        let v_chip = viewer.clone();
        let (top, right) = if csv_pill { (pill_top, 14.0 + PILL_W + 8.0) } else { (if find_open { 44.0 } else { 6.0 }, 14.0) };
        div()
            .id(("viewer-inline-diff", viewer.entity_id()))
            .absolute()
            .top(px(top))
            .right(px(right))
            .w(px(CHIP_W))
            .flex()
            .justify_center()
            .py(px(3.0))
            .rounded(px(6.0))
            .bg(theme.chrome.banner.hsla())
            .border_1()
            .border_color(theme.chrome.border.hsla())
            .text_color(if inline { theme.chrome.text.hsla() } else { dim })
            .font_family(family.clone())
            .text_size(px(style.ui(12.0)))
            .cursor_pointer()
            .child("inline diff")
            // A press on the chip is not a click in the text below: stopping it keeps the caret
            // where it is, so the chip focuses the pane itself.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |root, _ev: &MouseDownEvent, _window, cx| pill_press(root, pane, cx)),
            )
            .on_click(cx.listener(move |_root, _ev: &ClickEvent, _window, cx| v_chip.update(cx, |v, cx| v.toggle_inline_diff(cx))))
    });
    // A delimited file's mode pill, drawn like the terminal's "jump to bottom" pill.
    let pill = csv_pill.then(|| {
        div()
            .id(("viewer-mode-pill", viewer.entity_id()))
            .w(px(PILL_W))
            .flex()
            .justify_center()
            .py(px(4.0))
            .rounded(px(10.0))
            .bg(pill_bg(&theme))
            .border_1()
            .border_color(pill_border(&theme))
            .text_color(theme.chrome.text.hsla())
            .font_family(family.clone())
            .text_size(px(style.ui(12.0)))
            .cursor_pointer()
            .child(crate::view_menu::pill_label_for(mode, delimited))
            // A press on the pill is not a click in the grid below; stopping it also stops the
            // pane's, so the pill focuses the pane itself.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |root, _ev: &MouseDownEvent, _window, cx| pill_press(root, pane, cx)),
            )
            .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| root.toggle_markdown_view_for(tab, cx)))
    });
    // Over a grid the pill goes in the strip above it, with the caps' banner at the strip's left
    // cut with an ellipsis before the pill (`grid_strip`); over its text it floats at the top right.
    let (pill, strip_pill) = if kind_is_grid { (None, pill) } else { (pill.map(|p| p.absolute().top(px(pill_top)).right(px(14.0))), None) };
    if let Some(strip) = kind_is_grid.then(|| grid_strip(strip_pill.is_some(), grid_banner)).flatten() {
        let text = div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().children(strip.banner);
        banners.push(banner_row().items_center().bg(theme.chrome.banner.alpha(style.chrome_alpha)).child(text).children(strip_pill.map(|p| p.flex_none())));
    }
    let body = if banners.is_empty() { body } else { div().size_full().flex().flex_col().children(banners).child(div().flex_1().min_h_0().child(body)) };
    // The `⋯` button and its menu.
    let view_button_el = view_button.then(|| crate::view_menu::render_view_button(root, pane, tab, find_open, pill_bg(&theme), pill_border(&theme), cx));
    let view_menu_el = if view_button {
        let rows = crate::view_menu::items(mode, delimited, editable, has_changes, inline, presenting, outline_on, has_heads);
        crate::view_menu::render_view_menu(root, tab, &rows, find_open, cx)
    } else {
        None
    };
    let (v_keys, v_wheel, v_menu) = (viewer.clone(), viewer.clone(), viewer.downgrade());
    div()
        .size_full()
        .relative()
        // A markdown document adds `Markdown`, where Cmd+B and Cmd+I format text; in a code
        // document Cmd+B toggles the sidebar as everywhere else.
        .key_context(viewer_key_context(has_doc, markdown, sheet_count))
        .track_focus(&focus)
        .on_key_down(cx.listener(move |root, ev: &KeyDownEvent, _window, cx| {
            let ks = &ev.keystroke;
            // A document's keys are the editor's; one it does not know goes on to the input
            // method (a space is text), never to the scroll keys below. While the grid shows, a
            // delimited document's text is not on screen: the keys scroll the grid.
            // While the grid shows, its keys come first; one it does not take goes on below.
            if v_keys.read(cx).grid_shown() && v_keys.update(cx, |v, cx| v.grid_key(ks, cx)) {
                cx.stop_propagation();
                return;
            }
            if { let v = v_keys.read(cx); v.doc.is_some() && !v.grid_shown() } {
                if v_keys.update(cx, |v, cx| v.key(ks, cx)) {
                    cx.stop_propagation();
                }
                return;
            }
            if ks.modifiers.platform || ks.modifiers.control || ks.modifiers.alt {
                return;
            }
            // The keys that ease the view rest the glyph clock as the wheel does.
            if matches!(ks.key.as_str(), "down" | "up" | "pagedown" | "space" | "pageup") {
                root.last_scroll = Some(std::time::Instant::now());
            }
            v_keys.update(cx, |v, cx| {
                let page = v.rows_visible as f32;
                match ks.key.as_str() {
                    "down" => v.scroll_lines(1.0, cx),
                    "up" => v.scroll_lines(-1.0, cx),
                    "pagedown" | "space" => v.scroll_lines(page, cx),
                    "pageup" => v.scroll_lines(-page, cx),
                    "home" => {
                        v.scroll.jump_to(0.0);
                        cx.notify();
                    }
                    "end" => {
                        let m = v.max_scroll();
                        v.scroll.jump_to(m);
                        cx.notify();
                    }
                    "escape" => v.close_find(cx),
                    _ => {}
                }
            });
        }))
        // The rendered view's list scrolls itself; its wheel only rests the glyph clock here
        // (the list handles the wheel without stopping it, so both hear it).
        .when(rendered, |d| d.on_scroll_wheel(cx.listener(|root, _: &ScrollWheelEvent, _, _| root.last_scroll = Some(std::time::Instant::now()))))
        .when(!rendered, |d| {
            d.on_scroll_wheel(cx.listener(move |root, ev: &ScrollWheelEvent, _window, cx| {
                root.last_scroll = Some(std::time::Instant::now());
                v_wheel.update(cx, |v, cx| {
                    // The grid scrolls both ways: the wheel's y down by rows, its x sideways by
                    // points, swapped with Shift as a scrolled table's are.
                    if v.grid_shown() {
                        let row_h = v.grid_layout.as_ref().map_or(v.font_size * v.line_height, |l| l.row_h);
                        let (x, y, lines) = match ev.delta {
                            ScrollDelta::Lines(p) => (p.x, p.y, true),
                            ScrollDelta::Pixels(p) => (f32::from(p.x), f32::from(p.y), false),
                        };
                        let (x, y) = if ev.modifiers.shift { (y, x) } else { (x, y) };
                        let (rows, dx) = if lines { (-y, -x * row_h) } else { (-y / row_h, -x) };
                        v.grid_scroll_by(rows, dx, cx);
                        return;
                    }
                    let line_h = v.font_size * v.line_height;
                    // The wheel's y is positive when the content should move down: scroll up.
                    let lines = match ev.delta {
                        ScrollDelta::Lines(p) => -p.y,
                        ScrollDelta::Pixels(p) => -f32::from(p.y) / line_h,
                    };
                    v.scroll_lines(lines, cx);
                });
            }))
        })
        .on_mouse_down(MouseButton::Left, cx.listener(move |root, _ev: &MouseDownEvent, _window, cx| root.focus_pane(pane, cx)))
        // A right press (or a Control+click) in a markdown document opens its menu; the block
        // under it has already moved the caret there when the press was outside the selection.
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |root, ev: &MouseDownEvent, _window, cx| {
                // Presenting, the document has no menu: every row of it edits or needs a caret.
                // A shown grid has its own two-row menu: its text is not on screen.
                let Some(v) = v_menu.upgrade().filter(|v| (v.read(cx).doc.is_some() && !v.read(cx).presenting) || v.read(cx).grid_shown()) else { return };
                cx.stop_propagation();
                root.focus_pane(pane, cx);
                let now = std::time::Instant::now();
                root.last_input = now;
                root.context_menu.hide(now);
                root.wt_menu.hide(now);
                root.link_menu.hide(now);
                root.view_menu.hide(now);
                root.close_popovers(now);
                let grid = v.read(cx).grid_shown();
                let selection = if grid { v.read(cx).grid_sel.is_some() } else { v.read(cx).has_selection() };
                let table = !grid && { let v = v.read(cx); v.table_cell().is_some() && !v.table_edit };
                root.md_menu.show(crate::md_menu::MdMenu { viewer: v.downgrade(), selection, table, formats: markdown, grid, at: ev.position }, now);
                cx.notify();
            }),
        )
        .child(body)
        .children(chip)
        .children(pill)
        .children(view_button_el)
        .children(viewer::find_bar(&viewer, pane, &theme, family, &mut root.find_bars, root.frame_now, cx))
        .children(view_menu_el)
}

/// A video tab: the picture, the control strip and the video keys (`video_element`), its view
/// made the first time the tab is shown. A press anywhere in it focuses the pane.
fn render_video(root: &mut Root, pane: PaneId, tab: TabId, path: &std::path::Path, cx: &mut Context<Root>) -> Div {
    let view = root.video_for(tab, path, cx);
    let theme = root.theme.clone();
    let style = root.effect_style();
    let family = root.family();
    div()
        .size_full()
        .on_mouse_down(MouseButton::Left, cx.listener(move |root, _ev: &MouseDownEvent, _window, cx| root.focus_pane(pane, cx)))
        .child(crate::video_element::render(&view, &theme, style, family, true, Some(cx.weak_entity()), cx))
}

/// A web tab: its view, made the first time the tab is shown, drawn by `web_element` (the bar,
/// the screens and the rectangle the web view is placed over). A press anywhere in it focuses
/// the pane; a press on the page itself never reaches gpui (the web view takes it), so the
/// page's focus is read back each frame (`Root::update_web_visibility`).
fn render_browser(root: &mut Root, pane: PaneId, tab: TabId, url: String, cx: &mut Context<Root>) -> Div {
    let view = root.web_for(tab, &url, cx);
    let theme = root.theme.clone();
    let family = root.ui_family();
    div()
        .size_full()
        .on_mouse_down(MouseButton::Left, cx.listener(move |root, _ev: &MouseDownEvent, _window, cx| {
            root.focus_pane(pane, cx);
            root.keys_back_to_arkos();
        }))
        .child(crate::web_element::render(&view, pane, tab, &theme, root.effect_style(), family, cx))
}

/// The URL prompt, at the top of the focused pane's content while it is open and while it fades
/// out. It holds the keys while open (`render` focuses it): Enter opens, Escape cancels.
fn render_url_prompt(root: &mut Root, pane: PaneId, bar_h: f32, cx: &mut Context<Root>) -> Option<Div> {
    let style = root.effect_style();
    if root.tree.focused != pane {
        return None;
    }
    let (p, opacity) = root.url_prompt.current(root.frame_now)?;
    let live = root.url_prompt.value().is_some();
    let theme = root.theme.clone();
    let family = root.ui_family();
    let c = &theme.chrome;
    let (hint, placeholder) = match p.kind {
        PromptKind::Url => ("Enter opens it; empty opens [browser] url; Escape cancels", "a URL, like example.com"),
        PromptKind::Path => ("Enter opens it in this pane; a relative path is the project's; Escape cancels", "a file, like ~/Movies/cut.mp4"),
    };
    let field = line_edit::render(&p.edit, placeholder, &theme, family.clone(), 13.0);
    Some(
        div()
            .absolute()
            .top(px(bar_h + 8.0 - motion::lift(opacity, motion::PANEL_LIFT)))
            .left_0()
            .w_full()
            .flex()
            .justify_center()
            .opacity(opacity)
            .child(
                div()
                    .key_context("UrlPrompt")
                    .when(live, |d| {
                        d.track_focus(&root.url_prompt_focus)
                            .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _window, cx| cx.stop_propagation())
                            .on_key_down(cx.listener(|root, ev: &KeyDownEvent, _window, cx| root.url_prompt_key(&ev.keystroke, cx)))
                    })
                    .w(px(420.0))
                    .max_w_full()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .px(px(10.0))
                    .py(px(8.0))
                    .rounded(px(8.0))
                    .bg(c.banner.hsla())
                    .border_1()
                    .border_color(c.border.hsla())
                    .font_family(family)
                    .child(field)
                    .child(div().text_size(px(style.ui(11.0))).text_color(c.text_dim.hsla()).child(hint)),
            ),
    )
}

/// A tab whose shell is starting or whose replay is being parsed. The pane's grid is measured by
/// `render_pane`, so the replay is parsed at the size it will be shown at.
fn render_busy(root: &mut Root, pane: PaneId, tab: Option<TabId>, cx: &mut Context<Root>) -> Div {
    let text = tab.and_then(|id| root.sessions.busy.get(&id)).copied().unwrap_or("starting shell");
    let font_size = root.font_size;
    let dim = root.theme.chrome.text_dim;
    let focus = pane_focus(root, pane, cx);
    div()
        .size_full()
        .relative()
        .track_focus(&focus)
        .child(div().absolute().top(px(8.0)).left(px(12.0)).text_color(dim.hsla()).font_family(root.ui_family()).text_size(px(font_size)).child(text))
}

/// The friendly empty state (spec 4).
fn render_no_tabs(root: &mut Root, pane: PaneId, rect: Rect, window: &mut Window, cx: &mut Context<Root>) -> Div {
    let focus = pane_focus(root, pane, cx);
    let theme = root.theme.clone();
    crate::empty_pane::render(root, pane, rect.w, rect.h - TAB_BAR_H, focus, &theme, window, cx)
}

/// A pane without a terminal still needs something to focus so Cmd+N and typing land somewhere.
fn pane_focus(root: &mut Root, pane: PaneId, cx: &mut Context<Root>) -> FocusHandle {
    root.pane_focus.entry(pane).or_insert_with(|| cx.focus_handle()).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_context_adds_sheets_only_for_a_workbook_with_more_than_one_sheet() {
        assert_eq!(viewer_key_context(true, true, 0), "Viewer Markdown");
        assert_eq!(viewer_key_context(true, false, 0), "Viewer");
        assert_eq!(viewer_key_context(false, false, 1), "Viewer", "one sheet has nothing to switch to");
        assert_eq!(viewer_key_context(false, false, 2), "Viewer Sheets");
        assert_eq!(viewer_key_context(false, false, 0), "Viewer");
    }

    #[test]
    fn on_a_rounded_card_a_terminal_s_text_keeps_a_margin_from_the_card_s_sides() {
        let mut style = Style::plain();
        assert_eq!(terminal_inset(style), 0.0, "a square pane's text runs to its edge as ever");
        style.pane_radius = 14.0;
        assert_eq!(terminal_inset(style), 4.0);
    }

    #[test]
    fn the_pane_number_s_ink_is_centred_on_its_slot_by_its_cap_height_and_its_ink_width() {
        // A digit whose ink runs from 1 to 6 points past its origin, 9 points tall on the
        // baseline, in a line with a 12 point ascent, centred on (19, 15).
        let (x, y) = centred_origin((19.0, 15.0), (1.0, 6.0), 9.0, 12.0);
        assert_eq!(x + 1.0 + (6.0 - 1.0) / 2.0, 19.0, "the ink's middle on the centre across");
        let baseline = y + 12.0;
        assert_eq!(baseline - 9.0 / 2.0, 15.0, "the cap height's middle on the centre down");
    }

    #[test]
    fn on_a_rounded_card_the_pane_number_is_centred_in_a_slot_well_inside_the_card_s_edge() {
        let mut style = Style::plain();
        assert_eq!(number_slot(style), None, "the number keeps its plain padding on a square pane");
        style.pane_radius = 14.0;
        let (inset, width) = number_slot(style).unwrap();
        assert!(inset - FOCUS_RING_W >= 8.0, "clear of the focus ring by at least 8 points");
        assert_eq!((inset, width), (NUMBER_SLOT_INSET, NUMBER_SLOT_W));
    }

    #[test]
    fn a_pane_s_paper_rounds_its_bottom_corners_with_the_card_inside_the_pane_s_border() {
        let mut style = Style::plain();
        assert_eq!(paper_radius(style), 0.0, "square without an effect that rounds the panes");
        style.pane_radius = 14.0;
        assert_eq!(paper_radius(style), 13.0);
        style.pane_radius = 0.5;
        assert_eq!(paper_radius(style), 0.0);
    }

    #[test]
    fn the_terminal_decodes_under_a_decoding_effect_longer_while_its_session_works() {
        use protocol::ClaudeState::*;
        assert_eq!(decode_timing(false, Some(Working), false, false), None);
        assert_eq!(decode_timing(true, Some(Working), false, false), Some(decode::timing(true, false)));
        assert_eq!(decode_timing(true, Some(Idle), false, false), Some(decode::timing(false, false)));
        assert_eq!(decode_timing(true, None, false, false), Some(decode::timing(false, false)));
        assert_eq!(decode_timing(true, Some(Working), true, false), Some(decode::timing(true, true)));
    }

    #[test]
    fn a_paused_window_s_terminal_shows_its_output_plainly_with_no_decode() {
        use protocol::ClaudeState::*;
        let zero = decode::Timing { decode: std::time::Duration::ZERO, fresh: std::time::Duration::ZERO, white: decode::FRESH_WHITE, deep: decode::FRESH_DEEP };
        assert_eq!(decode_timing(true, Some(Working), false, true), Some(zero));
        assert_eq!(decode_timing(true, Some(Idle), false, true), Some(zero));
        assert_eq!(decode_timing(false, Some(Working), false, true), None);
    }

    #[test]
    fn a_grid_s_strip_holds_the_pill_for_a_csv_and_the_banner_when_rows_were_dropped() {
        let dropped = || Some("first 200,000 rows shown, 5 more in the file".to_string());
        assert_eq!(grid_strip(true, None), Some(Strip { banner: None, pill: true }), "a csv always has its pill");
        assert_eq!(grid_strip(true, dropped()), Some(Strip { banner: dropped(), pill: true }));
        assert_eq!(grid_strip(false, dropped()), Some(Strip { banner: dropped(), pill: false }), "a workbook has no pill");
        assert_eq!(grid_strip(false, None), None, "a workbook with nothing dropped has no strip");
    }

    #[test]
    fn a_markdown_tab_s_banners_keep_clear_of_the_view_button() {
        assert_eq!(banner_right_pad(true, false, false), crate::view_menu::RIGHT + crate::view_menu::BUTTON + 8.0);
        assert_eq!(banner_right_pad(false, false, true), CHIP_W + 8.0 + 14.0, "a code file with changes keeps its chip");
        assert_eq!(banner_right_pad(false, true, false), PILL_W + 8.0 + 14.0, "a raw csv keeps its pill");
        assert_eq!(banner_right_pad(false, true, true), PILL_W + 8.0 + 14.0 + CHIP_W + 8.0, "the chip sits left of a raw csv's pill");
        assert_eq!(banner_right_pad(false, false, false), 12.0, "nothing drawn over the banners: their usual padding");
    }

    #[test]
    fn a_reorder_opens_a_gap_where_the_tab_would_land_and_closes_it_on_a_cancel() {
        use std::time::{Duration, Instant};
        let _g = motion::test_lock(true);
        let now = Instant::now();
        let mut r = Reorder::start(1, 7, 80.0, now);
        assert_eq!(r.slot_width(now), 0.0, "the slot closes behind the tab");
        assert_eq!(r.gap_before(2, now), 0.0);
        r.aim(Some(2), now);
        assert_eq!((r.gap_before(1, now), r.gap_before(2, now)), (0.0, 80.0));
        r.aim(Some(3), now);
        assert_eq!((r.gap_before(2, now), r.gap_before(3, now)), (0.0, 80.0), "the gap moved to the next slot");
        r.aim(Some(3), now);
        assert_eq!(r.gap_before(3, now), 80.0, "aiming at the same slot changes nothing");
        r.aim(None, now);
        assert_eq!(r.gap_before(3, now), 0.0, "the pointer left the bar");
        assert!(!r.done(now));
        r.end(now);
        assert_eq!(r.slot_width(now), 80.0, "a cancel opens the slot again");
        assert!(r.done(now), "under Reduce Motion the slides land at once");
        r.aim(Some(1), now);
        assert_eq!(r.gap_before(1, now), 0.0, "nothing moves after the end");
        drop(_g);
        // With motion on, the gap grows over the slide rather than appearing.
        let _g = motion::test_lock(false);
        let t0 = Instant::now();
        let mut r = Reorder::start(1, 7, 80.0, t0);
        r.aim(Some(2), t0);
        let mid = r.gap_before(2, t0 + Duration::from_millis(40));
        assert!(mid > 0.0 && mid < 80.0, "part way: {mid}");
        assert!(r.animating(t0 + Duration::from_millis(40)));
        assert_eq!(r.gap_before(2, t0 + Duration::from_secs(1)), 80.0);
        assert!(!r.animating(t0 + Duration::from_secs(1)));
    }

    #[test]
    fn the_reorder_slot_follows_the_pointer_and_rests_in_the_gap() {
        let w = [100.0, 100.0, 100.0];
        // The first tab dragged right: past the middle of the next tab, it lands after it.
        assert_eq!(reorder_slot(&w, 0, None, 30.0), 0, "over its own place");
        assert_eq!(reorder_slot(&w, 0, Some(0), 149.0), 0, "grabbed off centre, the gap in its own place: stays");
        assert_eq!(reorder_slot(&w, 0, Some(0), 151.0), 2, "past the midpoint of the second tab");
        assert_eq!(reorder_slot(&w, 0, None, 149.0), 2, "with no gap open the second tab starts at 0");
        assert_eq!(reorder_slot(&w, 0, Some(2), 149.0), 2, "resting in the gap the slot opened");
        assert_eq!(reorder_slot(&w, 0, Some(2), 260.0), 3, "on to the end");
        assert_eq!(reorder_slot(&w, 0, Some(3), 260.0), 3);
        assert_eq!(reorder_slot(&w, 0, Some(3), 140.0), 2, "and back");
        // The middle tab dragged left and right.
        assert_eq!(reorder_slot(&w, 1, None, 30.0), 0);
        assert_eq!(reorder_slot(&w, 1, Some(0), 30.0), 0, "the gap now covers it");
        assert_eq!(reorder_slot(&w, 1, Some(0), 160.0), 1, "over the first tab's new place: stays");
        assert_eq!(reorder_slot(&w, 1, None, 250.0), 3);
        assert_eq!(reorder_slot(&w, 1, Some(3), 250.0), 3, "in the gap after the last tab");
        // Uneven widths.
        assert_eq!(reorder_slot(&[60.0, 200.0, 40.0], 2, None, 170.0), 2, "right of the wide tab's midpoint: stays after it");
        assert_eq!(reorder_slot(&[60.0, 200.0, 40.0], 2, None, 150.0), 1, "left of it: lands before the wide tab");
    }

    #[test]
    fn tab_labels_show_state_after_the_title() {
        assert_eq!(tab_label("vim", "zsh", true, None, None, None), "vim");
        assert_eq!(tab_label("", "shell", true, None, None, None), "shell");
        assert_eq!(tab_label("", "fish", true, None, None, None), "fish");
        assert_eq!(tab_label("", "zsh", false, Some(1), None, None), "zsh (ended, exit 1)");
        assert_eq!(tab_label("vim", "zsh", false, None, None, None), "vim (ended)");
        assert_eq!(tab_label("", "zsh", true, None, Some(Note::Restarted), None), "zsh (restarted)");
        assert_eq!(tab_label("", "zsh", true, None, Some(Note::Resumed), None), "zsh (resumed after restart)");
        assert_eq!(tab_label("", "zsh", true, None, None, Some("starting shell")), "starting shell");
    }

    #[test]
    fn a_tab_mark_is_the_state_unless_the_shell_ended() {
        use protocol::ClaudeState;
        assert_eq!(TabMark::of(Some(ClaudeState::Working), true), TabMark::Working);
        assert_eq!(TabMark::of(Some(ClaudeState::Waiting), true), TabMark::Waiting);
        assert_eq!(TabMark::of(Some(ClaudeState::Idle), true), TabMark::None);
        assert_eq!(TabMark::of(None, true), TabMark::None);
        assert_eq!(TabMark::of(Some(ClaudeState::Working), false), TabMark::Dead, "ended outranks a stale state");
    }

    #[test]
    fn the_glow_fits_a_tiny_pane_even_at_the_peak() {
        assert_eq!(glow_width(200.0, 100.0, 0.0), GLOW_W);
        assert_eq!(glow_width(200.0, 100.0, 1.0), 5.0, "a quarter more reach at the peak");
        assert_eq!(glow_width(10.0, 8.0, 0.0), 4.0, "never wider than half the pane");
        assert_eq!(glow_width(10.0, 8.0, 1.0), 4.0, "the breath cannot push it past half either");
        assert_eq!(glow_width(6.0, 4.0, 1.0), 2.0);
        assert_eq!(glow_width(0.0, 0.0, 1.0), 0.0);
    }

    #[test]
    fn the_url_prompt_adds_https_and_falls_back_to_the_configured_url() {
        assert_eq!(prompt_url("example.com", ""), Some("https://example.com".into()));
        assert_eq!(prompt_url("  localhost:3000/x ", ""), Some("http://localhost:3000/x".into()), "a port is not a scheme");
        assert_eq!(prompt_url("localhost:3000", ""), Some("http://localhost:3000".into()), "this machine: http");
        assert_eq!(prompt_url("127.0.0.1:8080/x", ""), Some("http://127.0.0.1:8080/x".into()));
        assert_eq!(prompt_url("[::1]:3000", ""), Some("http://[::1]:3000".into()));
        assert_eq!(prompt_url("app.localhost", ""), Some("http://app.localhost".into()));
        assert_eq!(prompt_url("localhost.example.com", ""), Some("https://localhost.example.com".into()), "only the host itself counts");
        assert_eq!(prompt_url("http://localhost:3000", ""), Some("http://localhost:3000".into()));
        assert_eq!(prompt_url("about:blank", ""), Some("about:blank".into()));
        assert_eq!(prompt_url("chrome://settings", ""), Some("chrome://settings".into()));
        assert_eq!(prompt_url("", "https://docs.rs"), Some("https://docs.rs".into()), "empty takes [browser] url");
        assert_eq!(prompt_url("   ", "docs.rs"), Some("https://docs.rs".into()), "the configured URL gets a scheme too");
        assert_eq!(prompt_url("", ""), None, "both empty: a blank window");
        assert_eq!(prompt_url("", "  "), None);
    }

    #[test]
    fn the_url_prompt_opens_on_enter_and_cancels_on_escape() {
        let key = |k: &str| gpui::Keystroke::parse(k).unwrap();
        let mut p = UrlPrompt::new(PromptKind::Url, None);
        assert_eq!(p.key(&key("enter"), ""), UrlStep::Open(None), "an empty field with no configured URL is a blank window");
        assert_eq!(p.key(&key("enter"), "example.org"), UrlStep::Open(Some("https://example.org".into())));
        p.edit.set("rust-lang.org");
        assert_eq!(p.key(&key("backspace"), ""), UrlStep::Stay);
        assert_eq!(p.edit.text, "rust-lang.or");
        assert_eq!(p.key(&key("enter"), "example.org"), UrlStep::Open(Some("https://rust-lang.or".into())), "what was typed wins");
        assert_eq!(p.key(&key("escape"), ""), UrlStep::Cancel);
        assert_eq!(p.kind, PromptKind::Url);
    }

    #[test]
    fn the_path_prompt_reads_home_and_the_project_and_opens_on_enter() {
        let (project, home) = (std::path::Path::new("/p/proj"), std::path::Path::new("/Users/me"));
        assert_eq!(prompt_path("/v/cut.mp4", project, home), Some("/v/cut.mp4".into()));
        assert_eq!(prompt_path("  ~/Movies/cut.mp4 ", project, home), Some("/Users/me/Movies/cut.mp4".into()));
        assert_eq!(prompt_path("~", project, home), Some("/Users/me".into()));
        assert_eq!(prompt_path("out/cut.mp4", project, home), Some("/p/proj/out/cut.mp4".into()), "a relative path is the project's");
        assert_eq!(prompt_path("   ", project, home), None);
        let key = |k: &str| gpui::Keystroke::parse(k).unwrap();
        let mut p = UrlPrompt::new(PromptKind::Path, None);
        assert_eq!(p.key(&key("enter"), ""), UrlStep::Stay, "an empty path opens nothing and the prompt stays");
        p.edit.set("/v/cut.mp4");
        assert_eq!(p.key(&key("enter"), "example.org"), UrlStep::OpenPath("/v/cut.mp4".into()), "a path is never given a scheme");
        assert_eq!(p.key(&key("escape"), ""), UrlStep::Cancel);
    }


    #[test]
    fn the_tab_bar_offers_no_overflow_button_at_any_tab_count() {
        assert_eq!(bar_items(), vec!["+"], "the bar's items do not depend on how many tabs it holds");
    }
}
