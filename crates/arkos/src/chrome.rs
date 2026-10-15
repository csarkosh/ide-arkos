//! The window chrome (spec 4, revised 2026-10-14 for a small display): one bar in the native
//! title bar's space, drawn by the window over a transparent title bar. On the left, "Arkos IDE"
//! and the project path; on the right, only what needs saying: a ready update, daemon trouble,
//! tabs waiting on the user. The old top bar and status bar are gone, so every row of height
//! goes to the panes.

use crate::theme::Theme;
use gpui::{div, prelude::*, px, App, ClickEvent, Div, FontWeight, MouseButton, MouseDownEvent, SharedString, Window};
use std::path::Path;

/// The native macOS title bar's height; the bar fills exactly that space.
pub const TITLE_BAR_H: f32 = 28.0;
/// Room for the traffic lights, which macOS keeps drawing where it always does.
const TRAFFIC_LIGHTS_W: f32 = 80.0;
/// The system UI font, by gpui's name for it: the bar looks like the title bar it replaces.
const UI_FONT: &str = ".SystemUIFont";

/// The right side of the bar, left to right: only exceptions. "connected" and the session
/// count say nothing the tabs do not, so they are left out and the bar is empty when all is
/// well. `daemon` is `None` while the connection is up.
pub fn status_items(update: Option<&str>, daemon: Option<&str>, waiting: usize) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(u) = update {
        v.push(u.to_string());
    }
    if let Some(d) = daemon {
        v.push(d.to_string());
    }
    if waiting > 0 {
        v.push(format!("{waiting} waiting"));
    }
    v
}

/// Whether a status item is the clickable "update ready" badge.
pub fn is_update_item(text: &str) -> bool {
    text.starts_with("update ready")
}

/// A click handler for the update badge.
pub type UpdateClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// The bar. Dragging it moves the window and a double click zooms it, as the native bar did.
/// In full screen the traffic lights are gone, so the text starts at the edge.
/// The "update ready" item runs `on_update` when clicked.
pub fn title_bar(project: &Path, items: Vec<String>, fullscreen: bool, theme: &Theme, on_update: Option<UpdateClick>) -> Div {
    let c = &theme.chrome;
    let hover = c.text.hsla();
    let mut on_update = on_update;
    let items: Vec<gpui::AnyElement> = items
        .into_iter()
        .map(|text| match on_update.take_if(|_| is_update_item(&text)) {
            Some(f) => div()
                .id("update-ready")
                .cursor_pointer()
                .hover(move |s| s.text_color(hover).underline())
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(f)
                .child(text)
                .into_any_element(),
            None => div().child(text).into_any_element(),
        })
        .collect();
    div()
        .h(px(TITLE_BAR_H))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .pl(px(if fullscreen { 12.0 } else { TRAFFIC_LIGHTS_W }))
        .pr(px(12.0))
        .gap(px(10.0))
        .bg(c.title_bar.hsla())
        .text_color(c.text.hsla())
        .font_family(SharedString::from(UI_FONT))
        .text_size(px(13.0))
        .on_mouse_down(MouseButton::Left, |ev: &MouseDownEvent, window, _cx| {
            if ev.click_count == 2 {
                window.titlebar_double_click();
            } else {
                window.start_window_move();
            }
        })
        .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).child("Arkos IDE"))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .italic()
                .font_weight(FontWeight::LIGHT)
                .text_color(c.text_dim.hsla())
                .child(project.display().to_string()),
        )
        .child(div().flex_none().flex().gap(px(16.0)).text_size(px(12.0)).text_color(c.text_dim.hsla()).children(items))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_says_nothing_while_all_is_well() {
        assert!(status_items(None, None, 0).is_empty());
    }

    #[test]
    fn only_the_ready_item_is_the_update_badge() {
        assert!(is_update_item("update ready: 0.2.0"));
        assert!(!is_update_item("update failed"));
        assert!(!is_update_item("2 waiting"));
    }

    #[test]
    fn the_bar_lists_exceptions_update_first() {
        assert_eq!(status_items(Some("update ready: 0.2.0"), Some("arkosd reconnecting"), 2), vec!["update ready: 0.2.0", "arkosd reconnecting", "2 waiting"]);
        assert_eq!(status_items(None, Some("arkosd connecting"), 0), vec!["arkosd connecting"]);
        assert_eq!(status_items(None, None, 1), vec!["1 waiting"]);
    }
}
