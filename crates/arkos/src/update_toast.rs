//! The update's toast (update heads-up spec): as soon as an update is ready, a toast at the
//! window's bottom right says so, with "Later" and "Update", and stays until one is clicked
//! (`idle::update_plan`). "Later" puts it away for an hour; the bell keeps its dot and its
//! "Update now" all the while.

use crate::motion;
use crate::Root;
use gpui::{div, prelude::*, px, ClickEvent, Context, Div, MouseDownEvent};

/// The toast's first line: the version when the daemon named one.
pub fn title(version: Option<&str>) -> String {
    match version {
        Some(v) => format!("Arkos IDE {v} is ready"),
        None => "A new version of Arkos IDE is ready".to_string(),
    }
}

/// The toast's second line.
pub const LINE: &str = "Your sessions keep running through it.";

pub fn render(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let ((), opacity) = root.update_toast.current(root.frame_now).map(|(v, o)| (*v, o))?;
    let live = root.update_toast.value().is_some();
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
            // Under an effect that paints the menus, its backdrop first, under the words.
            .when(style.menu_edges, |d| d.child(crate::chrome::menu_report(0x1f, opacity, cx)))
            .font_family(root.ui_family())
            .text_size(px(root.ui_size(12.0)))
            .text_color(text)
            .when(live, |d| d.occlude().on_any_mouse_down(|_: &MouseDownEvent, _window, cx| cx.stop_propagation()))
            .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(title(version.as_deref())))
            .child(div().text_color(dim).child(LINE))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .child(button("update-toast-later", "Later").on_click(cx.listener(|root, _: &ClickEvent, _window, cx| root.update_toast_later(cx))))
                    .child(button("update-toast-now", "Update").on_click(cx.listener(|root, _: &ClickEvent, _window, cx| root.update_clicked(cx)))),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toast_names_the_version() {
        assert_eq!(title(Some("0.8.44")), "Arkos IDE 0.8.44 is ready");
        assert_eq!(title(None), "A new version of Arkos IDE is ready");
    }
}
