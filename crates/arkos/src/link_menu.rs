//! The terminal's right-click menu on a link: "Open Link" and "Copy Link", drawn like the file
//! tree's menu at the click. A right-click (or Control+click, the Mac's other right-click) off a
//! link opens nothing.

use crate::motion;
use crate::sidebar::{run_menu_action, MenuAction};
use crate::Root;
use gpui::{div, prelude::*, px, ClickEvent, Context, Div, Modifiers, MouseButton, MouseDownEvent, Pixels, Point};
use std::time::Instant;

/// The open menu: the link it acts on and where the press was, in window pixels.
pub struct LinkMenu {
    pub url: String,
    pub at: Point<Pixels>,
}

/// The menu's rows, in order: open the link first, as the Cmd+click does, then copy it.
pub fn link_menu_items(url: &str) -> Vec<(&'static str, MenuAction)> {
    vec![("Open Link", MenuAction::OpenUrl(url.to_string())), ("Copy Link", MenuAction::CopyText(url.to_string()))]
}

/// What a press on the terminal is for, once the Cmd+click on a link has had its turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressKind {
    /// The link menu opens; the press goes no further.
    LinkMenu,
    /// An ordinary left press: the selection and the program's mouse reports see it.
    Gesture,
    /// Nothing happens and the event is left alone.
    Nothing,
}

/// Decides a press from its button, its modifiers and whether a link is under it. A right press
/// on a link opens the menu, and so does a left press with Control held, which is how a Mac
/// without a second button right-clicks. A right press off a link does nothing at all, so it
/// neither clears the selection nor reaches the program.
pub fn press_kind(button: MouseButton, modifiers: Modifiers, link: bool) -> PressKind {
    match button {
        MouseButton::Right if link => PressKind::LinkMenu,
        MouseButton::Left if modifiers.control && link => PressKind::LinkMenu,
        MouseButton::Left => PressKind::Gesture,
        _ => PressKind::Nothing,
    }
}

/// The link menu, drawn over everything at the press like the file tree's menu. A mouse-down
/// anywhere else closes it (the root's listener); its own rows stop the event so their click
/// lands, and a row closes the menu once its action has run.
pub fn render_link_menu(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let (menu, opacity) = root.link_menu.current(root.frame_now)?;
    // Drawn while it fades out too, but only the open menu occludes and takes clicks.
    let live = root.link_menu.value().is_some();
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
        .bg(c.banner.hsla())
        .border_1()
        .border_color(c.border.hsla())
        .font_family(root.ui_family())
        .text_size(px(12.0))
        .text_color(c.text.hsla())
        // Hover, the cursor and the wheel under the menu stop reacting while it is over them.
        .when(live, |m| m.occlude().on_any_mouse_down(|_: &MouseDownEvent, _window, cx| cx.stop_propagation()));
    let hover = c.tab_active.hsla();
    for (i, (label, action)) in link_menu_items(&menu.url).into_iter().enumerate() {
        m = m.child(div().id(("link-menu-row", i)).px(px(10.0)).py(px(4.0)).child(label).when(live, |row| {
            row.cursor_pointer().hover(move |s| s.bg(hover)).on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                run_menu_action(&action, cx);
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
        assert_eq!(link_menu_items("https://x"), vec![("Open Link", MenuAction::OpenUrl("https://x".into())), ("Copy Link", MenuAction::CopyText("https://x".into()))]);
    }

    #[test]
    fn a_right_or_control_press_on_a_link_opens_the_menu_and_nothing_else_does() {
        let none = Modifiers::default();
        let control = Modifiers { control: true, ..Modifiers::default() };
        assert_eq!(press_kind(MouseButton::Right, none, true), PressKind::LinkMenu);
        assert_eq!(press_kind(MouseButton::Right, control, true), PressKind::LinkMenu);
        assert_eq!(press_kind(MouseButton::Left, control, true), PressKind::LinkMenu, "Control+click is the Mac's right-click");
        assert_eq!(press_kind(MouseButton::Right, none, false), PressKind::Nothing, "off a link a right-click does nothing");
        assert_eq!(press_kind(MouseButton::Left, none, true), PressKind::Gesture, "a plain click on a link selects as before");
        assert_eq!(press_kind(MouseButton::Left, none, false), PressKind::Gesture);
        assert_eq!(press_kind(MouseButton::Left, control, false), PressKind::Gesture, "Control+click off a link is an ordinary press");
        assert_eq!(press_kind(MouseButton::Middle, none, true), PressKind::Nothing);
    }
}
