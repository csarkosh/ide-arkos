//! One pane: its tab bar and the active tab's content (a terminal, a busy notice while a shell
//! starts or a replay is parsed, or the empty state).

use crate::drag;
use crate::find;
use crate::terminal::Terminal;
use crate::terminal_element::{grid_for, TerminalElement};
use crate::theme::Theme;
use crate::tree::{Pane, PaneId, Rect, Tab, TabId};
use crate::Root;
use gpui::{
    canvas, div, prelude::*, px, ClickEvent, Context, Div, DragMoveEvent, ElementId, Entity, FocusHandle, KeyDownEvent, MouseButton,
    MouseDownEvent, ScrollDelta, ScrollWheelEvent, SharedString, Stateful, Window,
};

pub const TAB_BAR_H: f32 = 28.0;

/// The text on a tab: the terminal's title (or `fallback`, the shell's name), then its state.
pub fn tab_label(title: &str, fallback: &str, alive: bool, exit_code: Option<i32>, note: bool, busy: Option<&str>) -> String {
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
    if note {
        return format!("{name} (restarted)");
    }
    name.to_string()
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

/// Every tab's label, in bar order; the bar and the overflow menu both show these.
fn tab_labels(root: &Root, p: &Pane, cx: &Context<Root>) -> Vec<String> {
    p.tabs
        .iter()
        .enumerate()
        .map(|(idx, tab)| match tab {
            Tab::Terminal { session } => {
                let busy = p.ids.get(idx).and_then(|id| root.sessions.busy.get(id)).copied();
                let note = root.sessions.notes.contains(session);
                match root.sessions.terminals.get(session) {
                    Some(t) => {
                        let t = t.read(cx);
                        tab_label(&t.title, &root.shell_name, t.alive, t.exit_code, note, busy)
                    }
                    None => tab_label("", &root.shell_name, true, None, note, busy.or(Some("starting shell"))),
                }
            }
            Tab::Viewer { path } => path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        })
        .collect()
}

pub fn render_pane(root: &mut Root, pane: PaneId, rect: Rect, window: &mut Window, cx: &mut Context<Root>) -> Div {
    let focused = root.tree.focused == pane;
    let theme = root.theme.clone();
    let Some(p) = root.tree.pane(pane).cloned() else { return div() };
    // `pane_order` is computed once per render by `render_tree` (reading order, structural).
    let number = root.pane_order.iter().position(|id| *id == pane).map(|i| i + 1).unwrap_or(0);
    let labels = tab_labels(root, &p, cx);
    let bar = render_tab_bar(root, pane, &p, number, &labels, cx);
    if !p.tabs.is_empty() {
        // A pane that shows tabs forgets its empty-state animation, so emptying it again replays.
        root.empty_anim.remove(&pane);
    }
    let body = match p.tabs.get(p.active) {
        None => render_no_tabs(root, pane, rect, window, cx),
        Some(Tab::Terminal { session }) => match root.sessions.terminals.get(session).cloned() {
            Some(terminal) => render_terminal(terminal, root.family(), &theme, cx),
            None => render_busy(root, pane, p.ids.get(p.active).copied(), cx),
        },
        Some(Tab::Viewer { .. }) => div().size_full().child("viewer tabs arrive in milestone 4"),
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
    // The five drop zones of a dragged tab: the hovered one is tinted.
    let overlay = match root.drop_target {
        Some(drag::DropTarget::Pane { pane: p, zone }) if p == pane => {
            let (x, y, w, h) = drag::zone_rect(zone, rect.w, rect.h - TAB_BAR_H);
            Some(div().absolute().left(px(x)).top(px(y)).w(px(w)).h(px(h)).bg(theme.chrome.focus_ring.alpha(0.4)))
        }
        _ => None,
    };
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
    let menu = (root.tab_menu == Some(pane)).then(|| {
        let mut m = div()
            .id(("menu", pane))
            .absolute()
            .top(px(TAB_BAR_H))
            .right_0()
            .min_w(px(180.0))
            .flex()
            .flex_col()
            .py(px(4.0))
            .bg(theme.chrome.banner.hsla())
            .border_1()
            .border_color(theme.chrome.border.hsla())
            .font_family(root.family())
            .text_size(px(12.0))
            .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _window, cx| cx.stop_propagation());
        for (idx, label, active) in menu_rows(&p, &labels) {
            m = m.child(
                div()
                    .id(tab_id("menu-row", pane, idx))
                    .px(px(10.0))
                    .py(px(4.0))
                    .bg(if active { theme.chrome.tab_active.hsla() } else { theme.chrome.banner.hsla() })
                    .text_color(if active { theme.chrome.text.hsla() } else { theme.chrome.text_dim.hsla() })
                    .cursor_pointer()
                    .child(label)
                    .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                        root.close_tab_menu(cx);
                        root.activate_tab(pane, idx, cx);
                    })),
            );
        }
        m
    });
    div()
        .size_full()
        .relative()
        .flex()
        .flex_col()
        .bg(theme.chrome.window_bg.hsla())
        .border_1()
        .border_color(if focused { theme.chrome.focus_ring } else { theme.chrome.border }.hsla())
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |root, _ev: &MouseDownEvent, _window, cx| {
                root.focus_pane(pane, cx);
            }),
        )
        .child(bar)
        .child(content)
        .children(menu)
}

fn render_tab_bar(root: &mut Root, pane: PaneId, p: &Pane, number: usize, labels: &[String], cx: &mut Context<Root>) -> Stateful<Div> {
    let theme = root.theme.clone();
    let c = &theme.chrome;
    let (fg, dim) = (c.text, c.text_dim);
    let mut bar = div()
        .id(("bar", pane))
        .h(px(TAB_BAR_H))
        .w_full()
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .bg(c.tab_bar.hsla())
        .text_color(fg.hsla())
        .font_family(root.family())
        .text_size(px(12.0))
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
    for (idx, label) in labels.iter().enumerate() {
        let label = label.clone();
        let is_active = idx == p.active;
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
        strip = strip.child(
            div()
                .id(tab_id("tab", pane, idx))
                .h_full()
                .px(px(10.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .bg(if is_active { c.tab_active } else { c.tab_inactive }.hsla())
                .text_color(if is_active { fg } else { dim }.hsla())
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
                }))
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

fn render_terminal(terminal: Entity<Terminal>, family: SharedString, theme: &Theme, cx: &mut Context<Root>) -> Div {
    let focus = terminal.read(cx).focus.clone();
    let (font_size, line_height) = {
        let t = terminal.read(cx);
        (t.font_size, t.line_height)
    };
    let t_keys = terminal.clone();
    let t_wheel = terminal.clone();
    let t_click = terminal.clone();
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
            if root.sessions.notes.remove(&session) {
                cx.notify();
            }
            t_keys.update(cx, |t, cx| t.handle_key(ks, cx));
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_root, _ev: &MouseDownEvent, _window, cx| {
                // A click back into the terminal closes the find bar.
                t_click.update(cx, |t, cx| {
                    if t.find.is_some() {
                        t.close_find(cx);
                    }
                });
            }),
        )
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
        .child(TerminalElement::new(terminal.clone(), font_size, line_height, family.clone()))
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
                // in the terminal.
                .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _window, cx| cx.stop_propagation())
                .on_click(cx.listener(move |_root, _ev: &ClickEvent, _window, cx| t.update(cx, |t, cx| t.jump_to_bottom(cx))))
        }))
        .children(find::render(&terminal, theme, family, cx))
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
        assert_eq!(tab_label("vim", "zsh", true, None, false, None), "vim");
        assert_eq!(tab_label("", "shell", true, None, false, None), "shell");
        assert_eq!(tab_label("", "fish", true, None, false, None), "fish");
        assert_eq!(tab_label("", "zsh", false, Some(1), false, None), "zsh (ended, exit 1)");
        assert_eq!(tab_label("vim", "zsh", false, None, false, None), "vim (ended)");
        assert_eq!(tab_label("", "zsh", true, None, true, None), "zsh (restarted)");
        assert_eq!(tab_label("", "zsh", true, None, false, Some("starting shell")), "starting shell");
    }

    #[test]
    fn overflow_menu_lists_every_tab_from_two() {
        use crate::tree::{Pane, Tab};
        use protocol::SessionId;
        assert!(!show_overflow(0) && !show_overflow(1) && show_overflow(2));
        let p = Pane { id: 1, tabs: vec![Tab::Terminal { session: SessionId(1) }, Tab::Terminal { session: SessionId(2) }], active: 1, mru: vec![1, 0], ids: vec![2, 3] };
        let rows = menu_rows(&p, &["zsh".into(), "vim".into()]);
        assert_eq!(rows, vec![(0, "zsh".to_string(), false), (1, "vim".to_string(), true)]);
    }
}
