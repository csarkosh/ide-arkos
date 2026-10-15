//! One pane: its tab bar and the active tab's content (a terminal, a file viewer, a browser
//! tab's placeholder, a busy notice while a shell starts or a replay is parsed, or the empty
//! state), and the URL prompt over the focused pane.

use crate::drag;
use crate::dock::DockState;
use crate::find;
use crate::keymap::Command;
use crate::line_edit::{self, Edit, LineEdit};
use crate::motion;
use crate::pulse;
use crate::sessions::Note;
use crate::terminal::Terminal;
use crate::terminal_element::{grid_for, TerminalElement};
use crate::palette::Rgb;
use crate::theme::{Chrome, Theme};
use crate::tree::{Pane, PaneId, Rect, Tab, TabId};
use crate::viewer::{self, Content};
use crate::viewer_element::{MarkColors, ViewerElement};
use crate::Root;
use gpui::{
    canvas, div, linear_color_stop, linear_gradient, prelude::*, px, ClickEvent, Context, Div, DragMoveEvent, ElementId, Entity, FocusHandle,
    KeyDownEvent, Keystroke, MouseButton, MouseDownEvent, MouseMoveEvent, ScrollDelta, ScrollWheelEvent, SharedString, Stateful, Window,
};

pub const TAB_BAR_H: f32 = 28.0;
/// How far the focused pane's glow reaches in from each edge.
pub const GLOW_W: f32 = 6.0;
/// The glow's opacity at the edge; it fades to nothing by `GLOW_W`.
pub const GLOW_ALPHA: f32 = 0.3;

/// The four strips of the focus glow for a pane `w` by `h`: (x, y, width, height, gradient
/// angle), the angle being CSS's, the direction the colour fades towards (180 is downwards).
pub fn glow_strips(w: f32, h: f32) -> [(f32, f32, f32, f32, f32); 4] {
    let g = GLOW_W.min(w / 2.0).min(h / 2.0).max(0.0);
    [(0.0, 0.0, w, g, 180.0), (0.0, h - g, w, g, 0.0), (0.0, 0.0, g, h, 90.0), (w - g, 0.0, g, h, 270.0)]
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

/// A URL as typed, with `https://` added when it has no scheme. `localhost:3000` has none: a
/// scheme is only what comes before `://`, or one of the few Chrome opens without it.
fn with_scheme(url: &str) -> String {
    let bare = ["about:", "chrome:", "data:", "file:"];
    if url.contains("://") || bare.iter().any(|s| url.starts_with(s)) {
        url.to_string()
    } else {
        format!("https://{url}")
    }
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

/// Every tab's label and mark, in bar order; the bar and the overflow menu both show the labels.
fn tab_labels(root: &Root, p: &Pane, cx: &Context<Root>) -> Vec<(String, TabMark)> {
    p.tabs
        .iter()
        .enumerate()
        .map(|(idx, tab)| match tab {
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
            Tab::Viewer { path } => (path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), TabMark::None),
            // `browser_titles` holds a title only while a window is docked.
            Tab::Browser { url, .. } => (browser_label(p.ids.get(idx).and_then(|id| root.browser_titles.get(id)).map(String::as_str), url.as_deref()), TabMark::None),
        })
        .collect()
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
        Some(Tab::Viewer { path }) => match p.ids.get(p.active).copied() {
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
    // from the focus colour to nothing, drawn over the pane so they never move its content.
    // When focus moves, the old pane's glow fades out as the new one's fades in.
    let glow = root.glow.sync(pane, focused.then_some(()), now).map(|(_, opacity)| {
        let from = theme.chrome.focus_ring.alpha(GLOW_ALPHA);
        let mut to = from;
        to.a = 0.0;
        glow_strips(rect.w, rect.h).map(|(x, y, w, h, angle)| {
            div().absolute().left(px(x)).top(px(y)).w(px(w)).h(px(h)).opacity(opacity).bg(linear_gradient(angle, linear_color_stop(from, 0.0), linear_color_stop(to, 1.0)))
        })
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
                root.drop_target = Some(drag::DropTarget::TabBar { pane, at: usize::MAX });
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
    let mut strip = div().id(("tabs", pane)).flex_1().min_w_0().h_full().flex().flex_row().overflow_x_scroll().track_scroll(&handle);
    for (idx, (label, mark)) in tabs.iter().enumerate() {
        let (label, mark) = (label.clone(), *mark);
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
            .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| root.activate_tab(pane, idx, cx)))
            .when_some(drag_value, |el, value| {
                el.on_drag(value, |d: &drag::TabDrag, position, _window, cx| cx.new(|_| d.clone().at(position)))
            })
            .on_drag_move(cx.listener(move |root, ev: &DragMoveEvent<drag::TabDrag>, _window, cx| {
                if ev.bounds.contains(&ev.event.position) {
                    let at = drag::insert_index(ev.bounds, ev.event.position, idx);
                    root.drop_target = Some(drag::DropTarget::TabBar { pane, at });
                    cx.notify();
                }
            }));
        if slim {
            // A dot per tab in a 22 px square to hit; the label rides along on a drag.
            let (lit, unlit) = dot_colors(c);
            strip = strip.child(tab.p(px(8.0)).child(div().size(px(6.0)).rounded_full().bg(if is_active { lit } else { unlit }.hsla())));
            continue;
        }
        strip = strip.child(
            tab.px(px(10.0))
                .gap(px(6.0))
                .bg(if is_active { c.tab_active } else { c.tab_inactive }.hsla())
                .text_color(text)
                .when_some(glyph, |el, g| el.child(g))
                .child(label)
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
            t_keys.update(cx, |t, cx| t.handle_key(ks, cx));
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_root, ev: &MouseDownEvent, _window, cx| {
                // Cmd+click on a link opens it (Terminal.app's gesture); the plain click stays
                // free for selection later.
                if ev.modifiers.platform {
                    if let Some(link) = t_click.read(cx).hovered_link.clone() {
                        cx.open_url(&link.url);
                        return;
                    }
                }
                // A click back into the terminal closes the find bar.
                t_click.update(cx, |t, cx| {
                    if t.find.is_some() {
                        t.close_find(cx);
                    }
                });
            }),
        )
        // A file dragged from the tree types its path at the prompt.
        .on_drop(cx.listener(move |_root, d: &crate::drag::FileDrag, _window, cx| {
            match crate::drag::paste_text(&d.path) {
                Some(text) => t_drop.update(cx, |t, _| t.paste(&text)),
                None => eprintln!("arkos: not pasting a path with a control character"),
            }
        }))
        // The element finds the link under the pointer when it paints; a move over the
        // terminal asks for that paint (the inner div below asks on enter and exit).
        .on_mouse_move(cx.listener(|_root, _ev: &MouseMoveEvent, _window, cx| cx.notify()))
        .on_scroll_wheel(cx.listener(move |_root, ev: &ScrollWheelEvent, _window, cx| {
            t_wheel.update(cx, |t, cx| {
                let cell_h = t.cell.map(|c| c.height).unwrap_or(24.0);
                // Positive wheel y means the content moves down, which is scrolling up into history.
                let lines = match ev.delta {
                    ScrollDelta::Lines(p) => p.y,
                    ScrollDelta::Pixels(p) => f32::from(p.y) / cell_h,
                };
                t.scroll_lines(lines, cx);
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

/// What a viewer tab shows under its find bar.
enum Kind {
    Loading,
    Text { plain: Option<crate::viewer::Plain> },
    TooLarge(u64),
    Binary,
    Unreadable(String),
}

/// A viewer tab: the file's lines through `ViewerElement`, or a notice when there are none to show.
/// Arrows, Page Up and Down, Space, Home and End scroll; the wheel scrolls smoothly.
fn render_viewer(root: &mut Root, pane: PaneId, tab: TabId, path: &std::path::Path, cx: &mut Context<Root>) -> Div {
    let viewer = root.viewer_for(tab, path, cx);
    let theme = root.theme.clone();
    let family = root.family();
    let (focus, kind, font_size, has_changes, inline, find_open) = {
        let v = viewer.read(cx);
        let kind = match &v.content {
            _ if !v.loaded => Kind::Loading,
            Content::Text { .. } => Kind::Text { plain: v.plain },
            Content::TooLarge(n) => Kind::TooLarge(*n),
            Content::Binary => Kind::Binary,
            Content::Unreadable(e) => Kind::Unreadable(e.clone()),
        };
        (v.focus.clone(), kind, v.font_size, v.diff.as_ref().is_some_and(|d| d.has_changes()), v.inline_diff, v.find.is_some())
    };
    let dim = theme.chrome.text_dim.hsla();
    let notice = |text: String| div().size_full().flex().items_center().justify_center().text_color(dim).font_family(family.clone()).text_size(px(font_size)).child(text);
    let body = match kind {
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
    // The inline diff's switch, at the top right where the find bar opens, below it when open.
    let chip = has_changes.then(|| {
        let v_chip = viewer.clone();
        div()
            .id(("viewer-inline-diff", viewer.entity_id()))
            .absolute()
            .top(px(if find_open { 44.0 } else { 6.0 }))
            .right(px(14.0))
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
            .on_click(cx.listener(move |_root, _ev: &ClickEvent, _window, cx| v_chip.update(cx, |v, cx| v.toggle_inline_diff(cx))))
    });
    let (v_keys, v_wheel) = (viewer.clone(), viewer.clone());
    div()
        .size_full()
        .relative()
        .key_context("Viewer")
        .track_focus(&focus)
        .on_key_down(cx.listener(move |_root, ev: &KeyDownEvent, _window, cx| {
            let ks = &ev.keystroke;
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
        .on_scroll_wheel(cx.listener(move |_root, ev: &ScrollWheelEvent, _window, cx| {
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
        .on_mouse_down(MouseButton::Left, cx.listener(move |root, _ev: &MouseDownEvent, _window, cx| root.focus_pane(pane, cx)))
        .child(body)
        .children(chip)
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
    fn the_glow_hugs_every_edge_and_fits_a_tiny_pane() {
        let [top, bottom, left, right] = glow_strips(200.0, 100.0);
        assert_eq!(top, (0.0, 0.0, 200.0, GLOW_W, 180.0));
        assert_eq!(bottom, (0.0, 100.0 - GLOW_W, 200.0, GLOW_W, 0.0));
        assert_eq!(left, (0.0, 0.0, GLOW_W, 100.0, 90.0));
        assert_eq!(right, (200.0 - GLOW_W, 0.0, GLOW_W, 100.0, 270.0));
        let [top, ..] = glow_strips(10.0, 8.0);
        assert_eq!(top.3, 4.0, "never wider than half the pane");
        assert!(glow_strips(0.0, 0.0).iter().all(|s| s.2 >= 0.0 && s.3 >= 0.0));
    }

    #[test]
    fn the_tab_bar_is_slim_only_while_a_browser_tab_is_active() {
        use protocol::SessionId;
        assert_eq!(bar_height_for(Some(&Tab::Browser { url: None, app_mode: false })), slim_bar_height());
        assert_eq!(bar_height_for(Some(&Tab::Terminal { session: SessionId(1), cwd: None })), TAB_BAR_H);
        assert_eq!(bar_height_for(Some(&Tab::Viewer { path: "/p/a.rs".into() })), TAB_BAR_H);
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
        assert_eq!(prompt_url("  localhost:3000/x ", ""), Some("https://localhost:3000/x".into()), "a port is not a scheme");
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
