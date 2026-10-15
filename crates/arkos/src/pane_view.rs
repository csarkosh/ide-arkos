//! One pane: its tab bar and the active tab's content (a terminal, a file viewer, a browser
//! tab's placeholder, a busy notice while a shell starts or a replay is parsed, or the empty
//! state), and the URL prompt over the focused pane.

use crate::chrome;
use crate::drag;
use crate::dock::DockState;
use crate::find;
use crate::keymap::Command;
use crate::line_edit::{self, Edit, LineEdit};
use crate::md_view;
use crate::motion;
use crate::pulse;
use crate::sessions::Note;
use crate::terminal::Terminal;
use crate::terminal_element::{grid_for, TerminalElement};
use crate::palette::Rgb;
use crate::theme::{Chrome, Theme};
use crate::tree::{Pane, PaneId, Rect, Tab, TabId, ViewMode};
use crate::viewer::{self, Content};
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

/// The overflow menu is worth a button once there is something to choose between.
pub fn show_overflow(tabs: usize) -> bool {
    tabs >= 2
}

/// (index, label, active) for the menu, in bar order.
pub fn menu_rows(p: &Pane, labels: &[String]) -> Vec<(usize, String, bool)> {
    labels.iter().enumerate().map(|(i, l)| (i, l.clone(), i == p.active)).collect()
}

/// The tab bar's height while a browser tab is active: room for the pane number and a dot per
/// tab, so the docked Chrome window gets the rest of the pane.
pub fn slim_bar_height() -> f32 {
    14.0
}

/// The tab bar's height for the pane's active tab: slim for a browser tab, `TAB_BAR_H` otherwise.
pub fn bar_height_for(active: Option<&Tab>) -> f32 {
    match active {
        Some(Tab::Browser { .. }) => slim_bar_height(),
        _ => TAB_BAR_H,
    }
}

/// The slim bar's dots: the active tab in the text colour, the others dim. (`tab_active` is a
/// background colour and all but vanishes on the bar.)
pub fn dot_colors(c: &Chrome) -> (Rgb, Rgb) {
    (c.text, c.text_dim)
}

/// What a browser tab's placeholder shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserScreen {
    /// arkos may not move other apps' windows yet: how to allow it.
    NoPermission,
    /// No Chrome window is docked: the URL it opens with, and whether a window was launched for it.
    Waiting { url: Option<String>, launched: bool },
    /// A Chrome window covers the placeholder; nothing to draw under it.
    Docked,
}

/// The placeholder's screen: without Accessibility access nothing can dock, so that comes
/// first; then whether a window is docked. `docked` is the dock state with its window elided.
pub fn browser_screen(trusted: bool, docked: Option<()>, url: Option<String>, launched: bool) -> BrowserScreen {
    match (trusted, docked) {
        (false, _) => BrowserScreen::NoPermission,
        (true, Some(())) => BrowserScreen::Docked,
        (true, None) => BrowserScreen::Waiting { url, launched },
    }
}

/// The host of an absolute URL, without user info or port; `None` when there is none.
fn url_host(url: &str) -> Option<&str> {
    let (_, rest) = url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if host_port.starts_with('[') {
        host_port.split_inclusive(']').next().unwrap_or(host_port)
    } else {
        host_port.split(':').next().unwrap_or(host_port)
    };
    (!host.is_empty()).then_some(host)
}

/// A browser tab's label: the docked window's title, else the URL's host, else "browser".
pub fn browser_label(title: Option<&str>, url: Option<&str>) -> String {
    match title.filter(|t| !t.is_empty()) {
        Some(t) => t.to_string(),
        None => url.and_then(url_host).unwrap_or("browser").to_string(),
    }
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

/// The URL prompt ("New browser tab"), a small box at the top of the focused pane. `target`
/// is the waiting browser tab whose "Open a URL..." opened it; `None` makes a new tab.
#[derive(Debug, Clone, PartialEq)]
pub struct UrlPrompt {
    pub edit: LineEdit,
    pub app_mode: bool,
    pub target: Option<TabId>,
}

/// What a key in the URL prompt leads to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlStep {
    Stay,
    Cancel,
    /// Enter: the URL to open (`prompt_url`).
    Open(Option<String>),
}

impl UrlPrompt {
    pub fn new(app_mode: bool, target: Option<TabId>) -> Self {
        UrlPrompt { edit: LineEdit::new(), app_mode, target }
    }

    /// One key; `configured` is `[browser] url`, taken when the field is empty.
    pub fn key(&mut self, ks: &Keystroke, configured: &str) -> UrlStep {
        match self.edit.handle(ks) {
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

/// Every tab's label and mark, in bar order; the bar and the overflow menu both show the labels.
/// A name the user gave the tab replaces the automatic label; the mark still follows the shell.
/// A viewer with unsaved edits ends its label in " •".
fn tab_labels(root: &Root, p: &Pane, cx: &Context<Root>) -> Vec<(String, TabMark)> {
    p.tabs
        .iter()
        .enumerate()
        .map(|(idx, tab)| {
            let (label, mark) = automatic_label(root, p, idx, tab, cx);
            let named = p.ids.get(idx).and_then(|id| root.tree.tab_name(*id)).map(str::to_string);
            let dirty = matches!(tab, Tab::Viewer { .. }) && p.ids.get(idx).and_then(|id| root.viewers.get(id)).is_some_and(|v| v.read(cx).dirty);
            let label = named.unwrap_or(label);
            (if dirty { format!("{label} •") } else { label }, mark)
        })
        .collect()
}

fn automatic_label(root: &Root, p: &Pane, idx: usize, tab: &Tab, cx: &Context<Root>) -> (String, TabMark) {
    match tab {
        Tab::Terminal { session, .. } => {
            let busy = p.ids.get(idx).and_then(|id| root.sessions.busy.get(id)).copied();
            let note = root.sessions.notes.get(session).copied();
            match root.sessions.terminals.get(session) {
                Some(t) => {
                    let t = t.read(cx);
                    let mark = TabMark::of(root.sessions.states.get(session).copied(), t.alive);
                    (tab_label(&t.title, &root.shell_name, t.alive, t.exit_code, note, busy), mark)
                }
                None => (tab_label("", &root.shell_name, true, None, note, busy.or(Some("starting shell"))), TabMark::None),
            }
        }
        Tab::Viewer { path, .. } => (path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), TabMark::None),
        // `browser_titles` holds a title only while a window is docked.
        Tab::Browser { url, .. } => (browser_label(p.ids.get(idx).and_then(|id| root.browser_titles.get(id)).map(String::as_str), url.as_deref()), TabMark::None),
    }
}

pub fn render_pane(root: &mut Root, pane: PaneId, rect: Rect, window: &mut Window, cx: &mut Context<Root>) -> Div {
    let focused = root.tree.focused == pane;
    let theme = root.theme.clone();
    let Some(p) = root.tree.pane(pane).cloned() else { return div() };
    // `pane_order` is computed once per render by `render_tree` (reading order, structural).
    let number = root.pane_order.iter().position(|id| *id == pane).map(|i| i + 1).unwrap_or(0);
    let tabs = tab_labels(root, &p, cx);
    let bar_h = bar_height_for(p.tabs.get(p.active));
    // The body under the bar, for a Chrome window dragged over arkos (`Root::browser_drop`).
    root.pane_rects.push((pane, Rect { x: rect.x, y: rect.y + bar_h, w: rect.w, h: rect.h - bar_h }));
    let bar = render_tab_bar(root, pane, &p, number, &tabs, bar_h, cx);
    let labels: Vec<String> = tabs.into_iter().map(|(label, _)| label).collect();
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
                render_terminal(terminal, pane, family, &theme, &mut root.find_bars, now, cx)
            }
            None => render_busy(root, pane, p.ids.get(p.active).copied(), cx),
        },
        Some(Tab::Viewer { path, .. }) => match p.ids.get(p.active).copied() {
            Some(tab) => render_viewer(root, pane, tab, path, cx),
            None => div(),
        },
        Some(Tab::Browser { url, app_mode }) => match p.ids.get(p.active).copied() {
            Some(tab) => render_browser(root, pane, tab, url.clone(), *app_mode, cx),
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
    // A Chrome window dragged over arkos (`browser_drop`) tints the same way.
    let zone = match root.drop_target {
        Some(drag::DropTarget::Pane { pane: p, zone }) if p == pane => Some(zone),
        _ => root.browser_drop.filter(|(p, _)| *p == pane).map(|(_, zone)| zone),
    };
    let painted = if zone.is_some() || root.drop_tints.contains_key(&pane) {
        let tint = root.drop_tints.entry(pane).or_default();
        tint.aim(zone, now);
        tint.paint(rect.w, rect.h - bar_h, now)
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
                root.tab_drag_browser = None;
                cx.notify();
            }
        }))
        .child(measure)
        .child(body)
        .children(overlay);
    let slim = bar_h < TAB_BAR_H;
    if root.tab_menu.value() == Some(&pane) && (!show_overflow(p.tabs.len()) || slim) {
        // Down to one tab, or the slim bar of a browser tab: the ⋯ button is gone, so its menu
        // goes too and does not come back with the next tab.
        root.tab_menu.hide(now);
    }
    // Drawn while it fades out too; only the open menu takes clicks.
    let live = root.tab_menu.value() == Some(&pane);
    let menu = root.tab_menu.current(now).filter(|(p, _)| **p == pane).map(|(_, opacity)| {
        let mut m = div()
            .id(("menu", pane))
            .absolute()
            .top(px(bar_h - motion::lift(opacity, motion::MENU_LIFT)))
            .right_0()
            .opacity(opacity)
            .min_w(px(180.0))
            .flex()
            .flex_col()
            .py(px(4.0))
            .bg(theme.chrome.banner.hsla())
            .border_1()
            .border_color(theme.chrome.border.hsla())
            .font_family(root.family())
            .text_size(px(12.0))
            .when(live, |m| m.on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _window, cx| cx.stop_propagation()));
        for (idx, label, active) in menu_rows(&p, &labels) {
            m = m.child(
                div()
                    .id(tab_id("menu-row", pane, idx))
                    .px(px(10.0))
                    .py(px(4.0))
                    .bg(if active { theme.chrome.tab_active.hsla() } else { theme.chrome.banner.hsla() })
                    .text_color(if active { theme.chrome.text.hsla() } else { theme.chrome.text_dim.hsla() })
                    .child(label)
                    .when(live, |row| {
                        row.cursor_pointer().on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                            root.close_tab_menu(cx);
                            root.activate_tab(pane, idx, cx);
                        }))
                    }),
            );
        }
        m
    });
    // The focused pane glows at its edges instead of wearing a hard line: four strips that fade
    // from the focus colour to nothing, drawn over the pane so they never move its content, and
    // anchored to the pane's own edges (the pane wears a 1 px border, so offsets from the layout
    // rect would land past the clip). When focus moves, the old pane's glow fades out as the new
    // one's fades in. The glow breathes slowly, brighter and a little wider at the peak.
    // A window not in front rests: it repaints once on deactivation, and a breath caught
    // mid-way would freeze there.
    let breath = if root.window_active { pulse::glow(now.saturating_duration_since(root.pulse_epoch), crate::motion::reduce_motion()) } else { 0.0 };
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
        [
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
        .bg(theme.chrome.window_bg.hsla())
        .border_1()
        .border_color(theme.chrome.border.hsla())
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |root, _ev: &MouseDownEvent, _window, cx| {
                root.focus_pane(pane, cx);
            }),
        )
        .child(bar)
        .child(content)
        .children(menu)
        .children(render_url_prompt(root, pane, bar_h, cx))
        .children(glow.into_iter().flatten())
}

/// The tab bar, `height` tall. Slim (a browser tab is active), it shows only the pane number and
/// a dot per tab, the active one lit; every tab still clicks, drags and takes drops.
fn render_tab_bar(root: &mut Root, pane: PaneId, p: &Pane, number: usize, tabs: &[(String, TabMark)], height: f32, cx: &mut Context<Root>) -> Stateful<Div> {
    let slim = height < TAB_BAR_H;
    let theme = root.theme.clone();
    let c = &theme.chrome;
    let (fg, dim) = (c.text, c.text_dim);
    // One phase for the whole bar, so every working tab breathes together.
    let alpha = pulse::alpha(root.pulse_epoch.elapsed(), crate::motion::reduce_motion());
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
        .bg(c.tab_bar.hsla())
        .text_color(fg.hsla())
        .font_family(root.family())
        .text_size(px(if slim { 10.0 } else { 12.0 }))
        .child(div().px(px(8.0)).text_color(dim.hsla()).child(format!("{number}")))
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
    let family = root.family();
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
            family: root.family(),
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
            // The tab's painted width, for the gap a reorder opens where it would land.
            .child(canvas(
                {
                    let weak = cx.weak_entity();
                    move |bounds, _window, cx| {
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
        let (pad_x, pad_all) = if slot_width.is_some() { (px(0.0), px(0.0)) } else { (px(10.0), px(8.0)) };
        if slim {
            // A dot per tab in a 22 px square to hit; the label rides along on a drag.
            let (lit, unlit) = dot_colors(c);
            strip = strip.child(tab.p(pad_all).child(div().size(px(6.0)).rounded_full().bg(if is_active { lit } else { unlit }.hsla())));
            continue;
        }
        strip = strip.child(
            tab.px(pad_x)
                .gap(if slot_width.is_some() { px(0.0) } else { px(6.0) })
                .bg(if is_active { c.tab_active } else { c.tab_inactive }.hsla())
                .text_color(text)
                .when_some(glyph, |el, g| el.child(g))
                .child(match renaming {
                    Some(edit) => div()
                        .key_context("TabRename")
                        .track_focus(&rename_focus)
                        .w(px(150.0))
                        .overflow_hidden()
                        .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _window, cx| cx.stop_propagation())
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
    if slim {
        // No titles, no +, no overflow menu: the height goes to the docked window.
        return bar.child(strip);
    }
    bar = bar.child(strip).child(
        div()
            .id(("plus", pane))
            .px(px(10.0))
            .text_color(dim.hsla())
            .hover(move |s| s.text_color(fg.hsla()))
            .cursor_pointer()
            .child("+")
            .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| root.new_terminal_tab(pane, cx))),
    );
    if show_overflow(p.tabs.len()) {
        bar = bar.child(
            div()
                .id(("more", pane))
                .px(px(8.0))
                .text_color(dim.hsla())
                .hover(move |s| s.text_color(fg.hsla()))
                .cursor_pointer()
                .child("\u{22ef}")
                .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _window, cx| cx.stop_propagation())
                .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| root.toggle_tab_menu(pane, cx))),
        );
    }
    bar
}

fn render_terminal(terminal: Entity<Terminal>, pane: PaneId, family: SharedString, theme: &Theme, find_bars: &mut motion::Fades<gpui::EntityId, find::FindLook>, now: std::time::Instant, cx: &mut Context<Root>) -> Div {
    let focus = terminal.read(cx).focus.clone();
    let (font_size, line_height) = {
        let t = terminal.read(cx);
        (t.font_size, t.line_height)
    };
    let t_keys = terminal.clone();
    let t_wheel = terminal.clone();
    let t_click = terminal.clone();
    let t_up = terminal.clone();
    let t_up_out = terminal.clone();
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
            // Cmd+C copies the selection and nothing else: Ctrl+C stays the interrupt.
            if ks.modifiers.platform && ks.key == "c" {
                if let Some(text) = t_keys.read(cx).core.selection_text() {
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
                // free for selection later.
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
        .on_mouse_up(MouseButton::Left, cx.listener(move |_root, ev: &MouseUpEvent, _window, cx| t_up.update(cx, |t, _| t.mouse_released(ev.position))))
        // The button came up outside the terminal: the program hears nothing of the press.
        .on_mouse_up_out(MouseButton::Left, cx.listener(move |_root, _ev: &MouseUpEvent, _window, cx| t_up_out.update(cx, |t, _| t.mouse_cancelled())))
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
                .on_hover(cx.listener(|_root, _hovered: &bool, _window, cx| cx.notify()))
                .child(TerminalElement::new(terminal.clone(), font_size, line_height, family.clone())),
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
                .bg(theme.chrome.banner.hsla())
                .text_color(theme.chrome.text.hsla())
                .font_family(family.clone())
                .text_size(px(12.0))
                .cursor_pointer()
                .child("jump to bottom")
                // The terminal's own mouse-down closes an open find bar; the pill is not a click
                // in the terminal. Stopping it also stops the pane's, so the pill focuses the pane.
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |root, _ev: &MouseDownEvent, _window, cx| {
                        root.focus_pane(pane, cx);
                        cx.stop_propagation();
                    }),
                )
                .on_click(cx.listener(move |_root, _ev: &ClickEvent, _window, cx| t.update(cx, |t, cx| t.jump_to_bottom(cx))))
        }))
        .children(find::render(&terminal, pane, theme, family, find_bars, now, cx))
}

/// The width of a markdown tab's mode pill: fixed, so the inline-diff chip can sit beside it.
pub const PILL_W: f32 = 104.0;

/// The mode pill's text: it names the mode a click switches to.
pub fn pill_label(mode: ViewMode) -> &'static str {
    match mode {
        ViewMode::Rendered => "show raw",
        ViewMode::Raw => "show rendered",
    }
}

/// What a viewer tab shows under its find bar.
enum Kind {
    Loading,
    Text { plain: Option<crate::viewer::Plain> },
    TooLarge(u64),
    Binary,
    Unreadable(String),
}

/// A viewer tab: the file's lines through `ViewerElement`, or a notice when there are none to show.
/// A markdown document takes the editing keys (`Viewer::key`) and nothing else scrolls on a key;
/// in any other file the arrows, Page Up and Down, Space, Home and End scroll. The wheel scrolls
/// smoothly. An editable markdown document shows rendered (`md_view`, a list that scrolls itself)
/// or raw, with a pill at the top right that switches between the two.
fn render_viewer(root: &mut Root, pane: PaneId, tab: TabId, path: &std::path::Path, cx: &mut Context<Root>) -> Div {
    let viewer = root.viewer_for(tab, path, cx);
    let theme = root.theme.clone();
    let family = root.family();
    let (focus, kind, font_size, has_changes, inline, find_open, mode, editable) = {
        let v = viewer.read(cx);
        let kind = match &v.content {
            _ if !v.loaded => Kind::Loading,
            Content::Text { .. } => Kind::Text { plain: v.plain },
            Content::TooLarge(n) => Kind::TooLarge(*n),
            Content::Binary => Kind::Binary,
            Content::Unreadable(e) => Kind::Unreadable(e.clone()),
        };
        // A markdown file shown read-only (`read_only` set) has no document: no pill, raw text.
        let editable = viewer::is_markdown(path) && v.doc.is_some() && v.read_only.is_none();
        (v.focus.clone(), kind, v.font_size, v.diff.as_ref().is_some_and(|d| d.has_changes()), v.inline_diff, v.find.is_some(), v.mode, editable)
    };
    let rendered = editable && mode == ViewMode::Rendered;
    let dim = theme.chrome.text_dim.hsla();
    let notice = |text: String| div().size_full().flex().items_center().justify_center().text_color(dim).font_family(family.clone()).text_size(px(font_size)).child(text);
    let body = match kind {
        Kind::Text { .. } if rendered => {
            let element = md_view::render(&viewer, &theme, SharedString::from(chrome::UI_FONT), family.clone(), font_size, root.highlighter.clone(), cx);
            div().size_full().child(element)
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
                RawColors { text: theme.chrome.text, dim: theme.chrome.text_dim, code: theme.tree.modified, link: theme.chrome.focus_ring },
            );
            div()
                .size_full()
                .flex()
                .flex_col()
                .children(plain.map(|why| div().flex_none().px(px(12.0)).py(px(4.0)).text_color(dim).font_family(family.clone()).text_size(px(12.0)).child(why.banner())))
                .child(div().flex_1().min_h_0().child(element))
        }
        Kind::Loading => notice("loading".into()),
        Kind::TooLarge(n) => notice(format!("{} MB, too large to show", n / (1024 * 1024))),
        Kind::Binary => notice("a binary file; nothing to show".into()),
        Kind::Unreadable(e) => notice(format!("cannot read this file: {e}")),
    };
    // The mode pill's top: where the find bar opens, below it when open.
    let pill_top = if find_open { 44.0 } else { 10.0 };
    // The inline diff's switch, at the top right where the find bar opens, below it when open; on
    // a markdown document, left of the mode pill at its top.
    let chip = has_changes.then(|| {
        let v_chip = viewer.clone();
        let (top, right) = if editable { (pill_top, 14.0 + PILL_W + 8.0) } else { (if find_open { 44.0 } else { 6.0 }, 14.0) };
        div()
            .id(("viewer-inline-diff", viewer.entity_id()))
            .absolute()
            .top(px(top))
            .right(px(right))
            .px(px(8.0))
            .py(px(3.0))
            .rounded(px(6.0))
            .bg(theme.chrome.banner.hsla())
            .border_1()
            .border_color(theme.chrome.border.hsla())
            .text_color(if inline { theme.chrome.text.hsla() } else { dim })
            .font_family(family.clone())
            .text_size(px(12.0))
            .cursor_pointer()
            .child("inline diff")
            // A press on the chip is not a click in the text below: stopping it keeps the caret
            // where it is, so the chip focuses the pane itself.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |root, _ev: &MouseDownEvent, _window, cx| {
                    root.focus_pane(pane, cx);
                    cx.stop_propagation();
                }),
            )
            .on_click(cx.listener(move |_root, _ev: &ClickEvent, _window, cx| v_chip.update(cx, |v, cx| v.toggle_inline_diff(cx))))
    });
    // The mode pill, drawn like the terminal's "jump to bottom" pill.
    let pill = editable.then(|| {
        div()
            .id(("viewer-mode-pill", viewer.entity_id()))
            .absolute()
            .top(px(pill_top))
            .right(px(14.0))
            .w(px(PILL_W))
            .flex()
            .justify_center()
            .py(px(4.0))
            .rounded(px(10.0))
            .bg(theme.chrome.banner.hsla())
            .text_color(theme.chrome.text.hsla())
            .font_family(family.clone())
            .text_size(px(12.0))
            .cursor_pointer()
            .child(pill_label(mode))
            // A press on the pill is not a click in the text below; stopping it also stops the
            // pane's, so the pill focuses the pane itself.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |root, _ev: &MouseDownEvent, _window, cx| {
                    root.focus_pane(pane, cx);
                    cx.stop_propagation();
                }),
            )
            .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| root.toggle_markdown_view_for(tab, cx)))
    });
    let (v_keys, v_wheel) = (viewer.clone(), viewer.clone());
    div()
        .size_full()
        .relative()
        .key_context("Viewer")
        .track_focus(&focus)
        .on_key_down(cx.listener(move |_root, ev: &KeyDownEvent, _window, cx| {
            let ks = &ev.keystroke;
            // A document's keys are the editor's; one it does not know goes on to the input
            // method (a space is text), never to the scroll keys below.
            if v_keys.read(cx).doc.is_some() {
                if v_keys.update(cx, |v, cx| v.key(ks, cx)) {
                    cx.stop_propagation();
                }
                return;
            }
            if ks.modifiers.platform || ks.modifiers.control || ks.modifiers.alt {
                return;
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
        // The rendered view's list scrolls itself.
        .when(!rendered, |d| {
            d.on_scroll_wheel(cx.listener(move |_root, ev: &ScrollWheelEvent, _window, cx| {
                v_wheel.update(cx, |v, cx| {
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
        .child(body)
        .children(chip)
        .children(pill)
        .children(viewer::find_bar(&viewer, pane, &theme, family, &mut root.find_bars, root.frame_now, cx))
}

/// A browser tab's placeholder: the rectangle a Chrome window docks onto, reported every frame
/// (a `canvas`, like the pane's grid measure; it paints nothing and asks for no frames), and the
/// screen `browser_screen` picks. A click anywhere in it is `browser_clicked`.
fn render_browser(root: &mut Root, pane: PaneId, tab: TabId, url: Option<String>, app_mode: bool, cx: &mut Context<Root>) -> Div {
    let focus = pane_focus(root, pane, cx);
    let theme = root.theme.clone();
    let family = root.family();
    let c = &theme.chrome;
    let weak = cx.weak_entity();
    let report = canvas(
        move |bounds, _window, cx| {
            let _ = weak.update(cx, |root, _| root.report_browser_rect(tab, bounds));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    let (text, dim) = (c.text.hsla(), c.text_dim.hsla());
    let (banner, border) = (c.banner.hsla(), c.border.hsla());
    let button = move |name: &'static str, label: &'static str| {
        div()
            .id(ElementId::NamedInteger(name.into(), tab))
            .px(px(12.0))
            .py(px(5.0))
            .rounded(px(6.0))
            .bg(banner)
            .border_1()
            .border_color(border)
            .text_color(text)
            .cursor_pointer()
            .hover(move |s| s.border_color(text))
            .child(label)
    };
    let column = || div().size_full().flex().flex_col().items_center().justify_center().gap(px(10.0)).px(px(24.0)).font_family(family.clone()).text_size(px(13.0)).text_color(dim);
    let heading = |t: &'static str| div().text_color(text).text_size(px(15.0)).child(t);
    let tip_seen = root.vertical_tabs_tip_seen;
    let state = root.docks.tabs.get(&tab);
    let trusted = root.docks.trusted() && !matches!(state, Some(DockState::NoPermission));
    let docked = matches!(state, Some(DockState::Docked { .. })).then_some(());
    // "The window it opened" only while that window may still dock (the launch grace).
    let launched = matches!(state, Some(DockState::Waiting { launched_at: Some(t), .. }) if root.frame_now.saturating_duration_since(*t) <= crate::dock::LAUNCH_GRACE);
    let screen = match browser_screen(trusted, docked, url, launched) {
        BrowserScreen::NoPermission => column()
            .child(heading("Docking a Chrome window needs Accessibility access"))
            .child(div().child("arkos positions the window over this pane; it never reads its contents."))
            .child(div().child("macOS asks once. You can take the access back in System Settings at any time."))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(10.0))
                    .child(button("browser-grant", "Grant access").on_click(cx.listener(|root, _ev: &ClickEvent, _window, cx| root.request_accessibility(cx))))
                    .child(button("browser-settings", "Open System Settings").on_click(|_ev: &ClickEvent, _window, cx| {
                        cx.open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
                    })),
            ),
        BrowserScreen::Waiting { url, launched } => column()
            .child(heading(if launched { "Waiting for the Chrome window it opened" } else { "Waiting for a Chrome window" }))
            .when_some(url, |d, u| d.child(div().child(if app_mode { format!("{u} (single page)") } else { u })))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(10.0))
                    .child(button("browser-dock", "Dock a Chrome window\u{2026}").on_click(cx.listener(|root, _ev: &ClickEvent, window, cx| root.run(Command::DockChromeWindow, window, cx))))
                    .child(button("browser-url", "Open a URL\u{2026}").on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| root.open_url_prompt(app_mode, Some(tab), cx)))),
            )
            .when(!tip_seen, |d| {
                d.child(
                    div()
                        .pt(px(8.0))
                        .flex()
                        .flex_row()
                        .gap(px(10.0))
                        .text_size(px(12.0))
                        .child("Tip: right-click Chrome's tab strip and choose Show Tabs Vertically to save 40 px.")
                        .child(
                            div()
                                .id(ElementId::NamedInteger("browser-tip".into(), tab))
                                .text_color(text)
                                .cursor_pointer()
                                .child("Got it")
                                .on_click(cx.listener(|root, _ev: &ClickEvent, _window, cx| {
                                    root.vertical_tabs_tip_seen = true;
                                    cx.notify();
                                })),
                        ),
                )
            }),
        BrowserScreen::Docked => div().size_full().bg(c.window_bg.hsla()),
    };
    div()
        .size_full()
        .relative()
        .track_focus(&focus)
        .on_mouse_down(MouseButton::Left, cx.listener(move |root, _ev: &MouseDownEvent, _window, cx| root.browser_clicked(tab, cx)))
        .child(report)
        .child(screen)
}

/// The URL prompt, at the top of the focused pane's content while it is open and while it fades
/// out. It holds the keys while open (`render` focuses it): Enter opens, Escape cancels.
fn render_url_prompt(root: &mut Root, pane: PaneId, bar_h: f32, cx: &mut Context<Root>) -> Option<Div> {
    if root.tree.focused != pane {
        return None;
    }
    let (p, opacity) = root.url_prompt.current(root.frame_now)?;
    let live = root.url_prompt.value().is_some();
    let theme = root.theme.clone();
    let family = root.family();
    let c = &theme.chrome;
    let hint = if p.app_mode { "Enter opens it as a single page; empty opens [browser] url; Escape cancels" } else { "Enter opens it; empty opens [browser] url; Escape cancels" };
    let field = line_edit::render(&p.edit, "a URL, like example.com", &theme, family.clone(), 13.0);
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
                    .child(div().text_size(px(11.0)).text_color(c.text_dim.hsla()).child(hint)),
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
        .child(div().absolute().top(px(8.0)).left(px(12.0)).text_color(dim.hsla()).font_family(root.family()).text_size(px(font_size)).child(text))
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
    fn the_pill_names_the_other_mode() {
        assert_eq!(pill_label(ViewMode::Rendered), "show raw");
        assert_eq!(pill_label(ViewMode::Raw), "show rendered");
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
    fn the_tab_bar_is_slim_only_while_a_browser_tab_is_active() {
        use protocol::SessionId;
        assert_eq!(bar_height_for(Some(&Tab::Browser { url: None, app_mode: false })), slim_bar_height());
        assert_eq!(bar_height_for(Some(&Tab::Terminal { session: SessionId(1), cwd: None })), TAB_BAR_H);
        assert_eq!(bar_height_for(Some(&Tab::Viewer { path: "/p/a.rs".into(), mode: crate::tree::ViewMode::default() })), TAB_BAR_H);
        assert_eq!(bar_height_for(None), TAB_BAR_H);
        assert!(slim_bar_height() < TAB_BAR_H);
        assert_eq!(slim_bar_height(), 14.0);
    }

    #[test]
    fn the_slim_bars_active_dot_stands_out_from_the_bar_in_both_built_in_themes() {
        use crate::palette::contrast;
        for name in ["navy", "navy-light"] {
            let c = crate::theme::builtin(name).unwrap().chrome;
            let (active, inactive) = dot_colors(&c);
            let (a, i) = (contrast(active, c.tab_bar), contrast(inactive, c.tab_bar));
            assert!(a >= 4.5, "{name}: the active dot is {a:.2}:1 on the bar");
            assert!(a > i, "{name}: the active dot ({a:.2}:1) must outshine the others ({i:.2}:1)");
        }
    }

    #[test]
    fn the_browser_screen_follows_permission_then_docking() {
        assert_eq!(browser_screen(false, None, None, false), BrowserScreen::NoPermission);
        assert_eq!(browser_screen(false, Some(()), None, false), BrowserScreen::NoPermission, "no permission outranks a stale dock");
        assert_eq!(browser_screen(true, None, Some("https://x".into()), true), BrowserScreen::Waiting { url: Some("https://x".into()), launched: true });
        assert_eq!(browser_screen(true, None, None, false), BrowserScreen::Waiting { url: None, launched: false });
        assert_eq!(browser_screen(true, Some(()), None, false), BrowserScreen::Docked);
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
        let mut p = UrlPrompt::new(true, None);
        assert_eq!(p.key(&key("enter"), ""), UrlStep::Open(None), "an empty field with no configured URL is a blank window");
        assert_eq!(p.key(&key("enter"), "example.org"), UrlStep::Open(Some("https://example.org".into())));
        p.edit.set("rust-lang.org");
        assert_eq!(p.key(&key("backspace"), ""), UrlStep::Stay);
        assert_eq!(p.edit.text, "rust-lang.or");
        assert_eq!(p.key(&key("enter"), "example.org"), UrlStep::Open(Some("https://rust-lang.or".into())), "what was typed wins");
        assert_eq!(p.key(&key("escape"), ""), UrlStep::Cancel);
        assert!(p.app_mode);
    }

    #[test]
    fn a_browser_tab_is_titled_by_its_window_else_its_host() {
        assert_eq!(browser_label(Some("Docs - Example"), Some("https://example.com/docs")), "Docs - Example");
        assert_eq!(browser_label(Some(""), Some("https://example.com/docs")), "example.com", "an empty title falls back");
        assert_eq!(browser_label(None, Some("https://example.com/docs?q=1")), "example.com");
        assert_eq!(browser_label(None, Some("http://me@localhost:3000/x")), "localhost");
        assert_eq!(browser_label(None, Some("https://[::1]:8080/")), "[::1]");
        assert_eq!(browser_label(None, Some("about:blank")), "browser");
        assert_eq!(browser_label(None, None), "browser");
    }

    #[test]
    fn overflow_menu_lists_every_tab_from_two() {
        use crate::tree::{Pane, Tab};
        use protocol::SessionId;
        assert!(!show_overflow(0) && !show_overflow(1) && show_overflow(2));
        let p = Pane { id: 1, tabs: vec![Tab::Terminal { session: SessionId(1), cwd: None }, Tab::Terminal { session: SessionId(2), cwd: None }], active: 1, mru: vec![1, 0], ids: vec![2, 3] };
        let rows = menu_rows(&p, &["zsh".into(), "vim".into()]);
        assert_eq!(rows, vec![(0, "zsh".to_string(), false), (1, "vim".to_string(), true)]);
    }
}
