//! One pane: its tab bar and the active tab's content (a terminal, a busy notice while a shell
//! starts or a replay is parsed, or the empty state).

use crate::terminal::Terminal;
use crate::terminal_element::{grid_for, TerminalElement};
use crate::tree::{Pane, PaneId, Rect, Tab, TabId};
use crate::Root;
use gpui::{
    canvas, div, prelude::*, px, rgb, ClickEvent, Context, Div, ElementId, Entity, FocusHandle, KeyDownEvent, MouseButton,
    MouseDownEvent, ScrollDelta, ScrollWheelEvent, Window,
};

pub const TAB_BAR_H: f32 = 28.0;
const BG: u32 = 0x11161f;
const FG: u32 = 0xd6dae0;
const BAR_BG: u32 = 0x0d1117;
const TAB_ACTIVE: u32 = 0x11161f;
const TAB_INACTIVE: u32 = 0x0d1117;
const DIM: u32 = 0x8a93a3;
const FOCUS_RING: u32 = 0x3a7bd5;

/// The text on a tab: the terminal's title (or "shell"), then its state.
pub fn tab_label(title: &str, alive: bool, exit_code: Option<i32>, note: bool, busy: Option<&str>) -> String {
    if let Some(b) = busy {
        return b.to_string();
    }
    let name = if title.is_empty() { "shell" } else { title };
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

pub fn render_pane(root: &mut Root, pane: PaneId, _rect: Rect, _window: &mut Window, cx: &mut Context<Root>) -> Div {
    let focused = root.tree.focused == pane;
    let Some(p) = root.tree.pane(pane).cloned() else { return div() };
    // `pane_order` is computed once per render by `render_tree` (reading order, structural).
    let number = root.pane_order.iter().position(|id| *id == pane).map(|i| i + 1).unwrap_or(0);
    let bar = render_tab_bar(root, pane, &p, number, cx);
    let body = match p.tabs.get(p.active) {
        None => render_no_tabs(root, pane, cx),
        Some(Tab::Terminal { session }) => match root.sessions.terminals.get(session).cloned() {
            Some(terminal) => render_terminal(root, terminal, cx),
            None => render_busy(root, pane, p.ids.get(p.active).copied(), cx),
        },
        Some(Tab::Viewer { .. }) => div().size_full().child("viewer tabs arrive in milestone 4"),
    };
    // Every pane measures the grid a terminal would get, whatever it shows, so a restore into
    // any of its tabs always gets a size (it paints nothing).
    let weak = cx.weak_entity();
    let font_size = root.font_size;
    let measure = canvas(
        move |bounds, window, cx| {
            let grid = grid_for(bounds, window, font_size);
            let _ = weak.update(cx, |root, _| root.report_grid(pane, grid));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(rgb(BG))
        .border_1()
        .border_color(rgb(if focused { FOCUS_RING } else { BAR_BG }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |root, _ev: &MouseDownEvent, _window, cx| {
                root.focus_pane(pane, cx);
            }),
        )
        .child(bar)
        .child(div().flex_1().min_h_0().relative().child(measure).child(body))
}

fn render_tab_bar(root: &mut Root, pane: PaneId, p: &Pane, number: usize, cx: &mut Context<Root>) -> Div {
    let mut bar = div()
        .h(px(TAB_BAR_H))
        .w_full()
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .bg(rgb(BAR_BG))
        .text_color(rgb(FG))
        .font_family("Menlo")
        .text_size(px(12.0))
        .child(div().px(px(8.0)).text_color(rgb(DIM)).child(format!("{number}")));
    for (idx, tab) in p.tabs.iter().enumerate() {
        let label = match tab {
            Tab::Terminal { session } => {
                let busy = p.ids.get(idx).and_then(|id| root.sessions.busy.get(id)).copied();
                let note = root.sessions.notes.contains(session);
                match root.sessions.terminals.get(session) {
                    Some(t) => {
                        let t = t.read(cx);
                        tab_label(&t.title, t.alive, t.exit_code, note, busy)
                    }
                    None => tab_label("", true, None, note, busy.or(Some("starting shell"))),
                }
            }
            Tab::Viewer { path } => path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        };
        let is_active = idx == p.active;
        bar = bar.child(
            div()
                .id(tab_id("tab", pane, idx))
                .h_full()
                .px(px(10.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .bg(rgb(if is_active { TAB_ACTIVE } else { TAB_INACTIVE }))
                .text_color(rgb(if is_active { FG } else { DIM }))
                .cursor_pointer()
                .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| root.activate_tab(pane, idx, cx)))
                .child(label)
                .child(
                    div()
                        .id(tab_id("close", pane, idx))
                        .text_color(rgb(DIM))
                        .hover(|s| s.text_color(rgb(FG)))
                        .child("×")
                        .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                            // The tab underneath would otherwise activate the index the close just freed.
                            cx.stop_propagation();
                            root.close_tab(pane, idx, cx);
                        })),
                ),
        );
    }
    bar.child(
        div()
            .id(("plus", pane))
            .px(px(10.0))
            .text_color(rgb(DIM))
            .hover(|s| s.text_color(rgb(FG)))
            .cursor_pointer()
            .child("+")
            .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| root.new_terminal_tab(pane, cx))),
    )
}

fn render_terminal(root: &mut Root, terminal: Entity<Terminal>, cx: &mut Context<Root>) -> Div {
    let focus = terminal.read(cx).focus.clone();
    let font_size = root.font_size;
    let t_keys = terminal.clone();
    let t_wheel = terminal.clone();
    div()
        .size_full()
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
        .child(TerminalElement::new(terminal, font_size))
}

/// A tab whose shell is starting or whose replay is being parsed. The pane's grid is measured by
/// `render_pane`, so the replay is parsed at the size it will be shown at.
fn render_busy(root: &mut Root, pane: PaneId, tab: Option<TabId>, cx: &mut Context<Root>) -> Div {
    let text = tab.and_then(|id| root.sessions.busy.get(&id)).copied().unwrap_or("starting shell");
    let font_size = root.font_size;
    let focus = pane_focus(root, pane, cx);
    div()
        .size_full()
        .relative()
        .track_focus(&focus)
        .child(div().absolute().top(px(8.0)).left(px(12.0)).text_color(rgb(DIM)).font_family("Menlo").text_size(px(font_size)).child(text))
}

/// Placeholder until Task 8 brings the friendly empty state.
fn render_no_tabs(root: &mut Root, pane: PaneId, cx: &mut Context<Root>) -> Div {
    let focus = pane_focus(root, pane, cx);
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .track_focus(&focus)
        .text_color(rgb(DIM))
        .font_family("Menlo")
        .text_size(px(14.0))
        .child("No tabs. Press Ctrl+Shift+` for a new terminal.")
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
        assert_eq!(tab_label("zsh", true, None, false, None), "zsh");
        assert_eq!(tab_label("", true, None, false, None), "shell");
        assert_eq!(tab_label("zsh", false, Some(1), false, None), "zsh (ended, exit 1)");
        assert_eq!(tab_label("zsh", false, None, false, None), "zsh (ended)");
        assert_eq!(tab_label("zsh", true, None, true, None), "zsh (restarted)");
        assert_eq!(tab_label("", true, None, false, Some("starting shell")), "starting shell");
    }
}
