//! The terminal's right-click menu, drawn like the file tree's menu at the click. On the
//! selection it offers Copy and Paste; on a link, Open Link and Copy Link; on a link inside the
//! selection, all four. gpui delivers a Control+click as a right press, so it opens the menu too.
//! A right-click anywhere else opens nothing.

use crate::motion;
use crate::sidebar::{run_menu_action, MenuAction};
use crate::terminal::Terminal;
use crate::Root;
use gpui::{div, prelude::*, px, ClickEvent, Context, Div, MouseButton, MouseDownEvent, Pixels, Point, WeakEntity};
use std::time::Instant;

/// The open menu: the link it acts on, whether the press was on the selection, the terminal it
/// was opened on, and where the press was, in window pixels.
pub struct LinkMenu {
    pub url: Option<String>,
    pub selection: bool,
    pub terminal: WeakEntity<Terminal>,
    pub at: Point<Pixels>,
}

/// One row's action. Copy and Paste act on the terminal when the row is clicked, so Copy takes
/// the selection as Cmd+C would and Paste the clipboard as Cmd+V would.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuRow {
    Copy,
    Paste,
    Link(MenuAction),
}

/// The menu's rows, in order: on the selection Copy then Paste; on a link, open it first, as
/// the Cmd+click does, then copy it.
pub fn menu_items(url: Option<&str>, selection: bool) -> Vec<(&'static str, MenuRow)> {
    let mut rows = Vec::new();
    if selection {
        rows.push(("Copy", MenuRow::Copy));
        rows.push(("Paste", MenuRow::Paste));
    }
    if let Some(url) = url {
        rows.push(("Open Link", MenuRow::Link(MenuAction::OpenUrl(url.to_string()))));
        rows.push(("Copy Link", MenuRow::Link(MenuAction::CopyText(url.to_string()))));
    }
    rows
}

/// What a press on the terminal is for, once the Cmd+click on a link has had its turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressKind {
    /// The menu opens; the press goes no further.
    Menu,
    /// An ordinary left press: the selection and the program's mouse reports see it.
    Gesture,
    /// Nothing happens and the event is left alone.
    Nothing,
}

/// Decides a press from its button and whether a link or the selection is under it. A right
/// press on either opens the menu; gpui delivers a Control+click as a right press with Control
/// cleared, so that is how a Mac without a second button opens it too. Any other right press
/// does nothing at all, so it neither clears the selection nor reaches the program.
pub fn press_kind(button: MouseButton, link: bool, selection: bool) -> PressKind {
    match button {
        MouseButton::Right if link || selection => PressKind::Menu,
        MouseButton::Left => PressKind::Gesture,
        _ => PressKind::Nothing,
    }
}

/// Runs a row on the terminal the menu was opened on. Copy leaves the selection in place.
fn run_row(row: &MenuRow, terminal: &WeakEntity<Terminal>, cx: &mut Context<Root>) {
    match row {
        MenuRow::Link(action) => run_menu_action(action, cx),
        MenuRow::Copy => {
            if let Some(text) = terminal.upgrade().and_then(|t| t.read(cx).core.copy_text()) {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            }
        }
        MenuRow::Paste => {
            if let (Some(t), Some(text)) = (terminal.upgrade(), cx.read_from_clipboard().and_then(|item| item.text())) {
                t.update(cx, |t, _| t.paste(&text));
            }
        }
    }
}

/// The menu, drawn over everything at the press like the file tree's menu. A mouse-down
/// anywhere else closes it (the root's listener); its own rows stop the event so their click
/// lands, and a row closes the menu once its action has run.
pub fn render_link_menu(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let (menu, opacity) = root.link_menu.current(root.frame_now)?;
    // Drawn while it fades out too, but only the open menu occludes and takes clicks.
    let live = root.link_menu.value().is_some();
    let style = root.effect_style();
    let c = &root.theme.chrome;
    let mut m = div()
        .absolute()
        .left(menu.at.x)
        .top(menu.at.y - px(motion::lift(opacity, motion::MENU_LIFT)))
        .opacity(opacity)
        .min_w(px(200.0))
        .flex()
        .flex_col()
        .py(px(4.0))
        .map(|d| crate::chrome::menu_frame(d, &root.theme, &style))
        .font_family(root.ui_family())
        .text_size(px(root.ui_size(12.0)))
        .text_color(c.text.hsla())
        // Hover, the cursor and the wheel under the menu stop reacting while it is over them.
        .when(live, |m| m.occlude().on_any_mouse_down(|_: &MouseDownEvent, _window, cx| cx.stop_propagation()));
    let hover = crate::chrome::menu_colors(&root.theme, style.menus).hover;
    let (effect_rows, hovered) = (style.menu_edges && style.highlights, root.effect_hover);
    if style.menu_edges {
        m = m.child(crate::chrome::menu_report(0x11, opacity, cx));
    }
    for (i, (label, action)) in menu_items(menu.url.as_deref(), menu.selection).into_iter().enumerate() {
        let terminal = menu.terminal.clone();
        m = m.child(div().id(("link-menu-row", i)).px(px(10.0)).py(px(4.0)).child(label).when(live, |row| {
            let key = crate::effects::blocks::menu_row_key(0x11, i);
            crate::chrome::menu_row_hover(row.cursor_pointer(), key, hovered == Some(key), effect_rows, hover, cx).on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                run_row(&action, &terminal, cx);
                root.link_menu.hide(Instant::now());
                cx.notify();
            }))
        }));
    }
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_link_menu_opens_then_copies_in_that_order() {
        let open = MenuRow::Link(MenuAction::OpenUrl("https://x".into()));
        let copy_link = MenuRow::Link(MenuAction::CopyText("https://x".into()));
        assert_eq!(menu_items(Some("https://x"), false), vec![("Open Link", open), ("Copy Link", copy_link)]);
    }

    #[test]
    fn the_selection_menu_copies_then_pastes_and_adds_the_link_rows_on_a_link() {
        assert_eq!(menu_items(None, true), vec![("Copy", MenuRow::Copy), ("Paste", MenuRow::Paste)]);
        let rows: Vec<&str> = menu_items(Some("https://x"), true).into_iter().map(|(label, _)| label).collect();
        assert_eq!(rows, ["Copy", "Paste", "Open Link", "Copy Link"]);
    }

    #[test]
    fn a_right_press_on_a_link_or_the_selection_opens_the_menu_and_nothing_else_does() {
        // A Control+click reaches here as a right press: gpui turns it into one.
        assert_eq!(press_kind(MouseButton::Right, true, false), PressKind::Menu);
        assert_eq!(press_kind(MouseButton::Right, false, true), PressKind::Menu, "on the selection");
        assert_eq!(press_kind(MouseButton::Right, true, true), PressKind::Menu);
        assert_eq!(press_kind(MouseButton::Right, false, false), PressKind::Nothing, "off a link and the selection a right-click does nothing");
        assert_eq!(press_kind(MouseButton::Left, true, false), PressKind::Gesture, "a plain click on a link selects as before");
        assert_eq!(press_kind(MouseButton::Left, false, true), PressKind::Gesture, "a plain click on the selection clears it as before");
        assert_eq!(press_kind(MouseButton::Middle, true, true), PressKind::Nothing);
    }
}
