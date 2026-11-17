//! The update's heads-up (update heads-up spec): in a window in front, before an update applies
//! by itself, a toast at the window's bottom right says a new version is ready and counts down,
//! with "Now" and "Later". Typing calls it off (`idle::heads_up`); a window not in front updates
//! with no toast, nobody watching it.

use crate::idle;
use crate::motion;
use crate::Root;
use gpui::{div, prelude::*, px, ClickEvent, Context, Div, MouseDownEvent};
use std::time::Instant;

/// The toast's first line: the version when the daemon named one.
pub fn title(version: Option<&str>) -> String {
    match version {
        Some(v) => format!("Arkos IDE {v} is ready"),
        None => "A new version of Arkos IDE is ready".to_string(),
    }
}

/// The toast's second line.
pub fn countdown_text(seconds: u64) -> String {
    format!("Updating in {seconds} s")
}

pub fn render(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let ((), opacity) = root.update_toast.current(root.frame_now).map(|(v, o)| (*v, o))?;
    let live = root.update_toast.value().is_some();
    let seconds = root.update_countdown.map_or(1, |since| idle::seconds_left(since, Instant::now()));
    let version = root.staged.as_ref().map(|s| s.version.clone());
    let style = root.effect_style();
    let c = &root.theme.chrome;
    let (text, dim, line) = (c.text.hsla(), c.text_dim.hsla(), c.border.hsla());
    let button = |id: &'static str, label: &'static str| {
        div().id(id).px(px(10.0)).py(px(3.0)).rounded(px(5.0)).border_1().border_color(line).text_color(text).cursor_pointer().hover(|s| s.opacity(0.8)).child(label)
    };
    Some(
        div()
            .absolute()
            .right(px(16.0))
            // Rising into place as it enters, as the panels do.
            .bottom(px(16.0 - motion::lift(opacity, motion::PANEL_LIFT)))
            .opacity(opacity)
            .w(px(300.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .px(px(12.0))
            .py(px(10.0))
            .map(|d| crate::chrome::menu_frame(d, &root.theme, &style))
            .font_family(root.ui_family())
            .text_size(px(root.ui_size(12.0)))
            .text_color(text)
            .when(live, |d| d.occlude().on_any_mouse_down(|_: &MouseDownEvent, _window, cx| cx.stop_propagation()))
            .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(title(version.as_deref())))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(div().flex_1().text_color(dim).child(countdown_text(seconds)))
                    .child(button("update-toast-later", "Later").on_click(cx.listener(|root, _: &ClickEvent, _window, cx| root.update_toast_later(cx))))
                    .child(button("update-toast-now", "Now").on_click(cx.listener(|root, _: &ClickEvent, _window, cx| root.update_toast_now(cx)))),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toast_names_the_version_and_counts_down() {
        assert_eq!(title(Some("0.8.44")), "Arkos IDE 0.8.44 is ready");
        assert_eq!(title(None), "A new version of Arkos IDE is ready");
        assert_eq!(countdown_text(7), "Updating in 7 s");
    }
}
