//! The Keybindings window (Help, Keybindings, or Cmd+/): every action with the keys in force,
//! grouped by section so a binding is easy to find. It is read-only; the footer says where the
//! bindings are changed and opens the config file.
use crate::aux;
use crate::chrome::{self, TITLE_BAR_H, UI_FONT};
use crate::keymap::{self, KeyRow, Section};
use crate::settings::{self, Colors};
use crate::theme::Theme;
use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, FocusHandle, FontWeight, KeyDownEvent, SharedString, TitlebarOptions, Window, WindowBounds,
    WindowHandle, WindowOptions,
};

/// The window's title, which its own title bar draws.
const TITLE: &str = "Keybindings";
/// The window's opening size and its minimum. Each height is the content's plus the title bar.
const WIDTH: f32 = 520.0;
const HEIGHT: f32 = 640.0 + TITLE_BAR_H;
const MIN_WIDTH: f32 = 420.0;
const MIN_HEIGHT: f32 = 400.0 + TITLE_BAR_H;

/// The footer's line, next to its "Open config" pill.
pub fn footer_text() -> &'static str {
    "Change them under [keybindings] in config.toml"
}

/// A row's keys as the window draws them, and whether they are dimmed: an unbound action reads
/// "none" in the dimmed colour.
pub fn keys_text(row: &KeyRow) -> (String, bool) {
    match &row.keys {
        Some(keys) => (keys.clone(), false),
        None => ("none".to_string(), true),
    }
}

/// The Keybindings window's view. One at most, kept in `aux::Aux`.
pub struct KeybindingsWindow {
    rows: Vec<(Section, Vec<KeyRow>)>,
    theme: Theme,
    /// The panes' monospace family, which the keys column is drawn in.
    mono: SharedString,
    focus: FocusHandle,
}

impl KeybindingsWindow {
    /// Shows the rows for the bindings in force now, after a config change or a second Cmd+/.
    pub fn set_rows(&mut self, rows: Vec<(Section, Vec<KeyRow>)>, cx: &mut Context<Self>) {
        self.rows = rows;
        cx.notify();
    }

    /// Redraws the window in the project window's new theme.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }
}

/// Opens the Keybindings window centred on the main display and gives it the keyboard. `None`
/// when gpui could not open it, which the log records.
pub fn open(cx: &mut App, rows: Vec<(Section, Vec<KeyRow>)>, theme: Theme, mono: SharedString) -> Option<WindowHandle<KeybindingsWindow>> {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(WIDTH), px(HEIGHT)), cx))),
        // The window draws the IDE's own title bar in the native one's place.
        titlebar: Some(TitlebarOptions { title: Some(TITLE.into()), appears_transparent: true, traffic_light_position: None }),
        focus: true,
        is_resizable: true,
        window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
        ..Default::default()
    };
    let opened = cx.open_window(options, move |window, cx| {
        let view = cx.new(|cx| KeybindingsWindow { rows, theme, mono, focus: cx.focus_handle() });
        window.focus(&view.read(cx).focus);
        view
    });
    match opened {
        Ok(handle) => Some(handle),
        Err(e) => {
            eprintln!("arkos: the Keybindings window did not open: {e:#}");
            None
        }
    }
}

impl Render for KeybindingsWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let c = Colors::of(&self.theme);
        let bg = self.theme.chrome.window_bg.hsla();
        let mut list = div().id("keybindings-rows").flex_1().min_h_0().overflow_y_scroll().flex().flex_col().px(px(20.0)).pb(px(16.0));
        for (section, rows) in &self.rows {
            list = list.child(
                div()
                    .flex_none()
                    .pt(px(16.0))
                    .pb(px(4.0))
                    .mb(px(4.0))
                    .border_b_1()
                    .border_color(c.border)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(section.title()),
            );
            for row in rows {
                let (keys, dimmed) = keys_text(row);
                list = list.child(
                    div()
                        .flex_none()
                        .flex()
                        .gap(px(12.0))
                        .py(px(2.0))
                        .child(div().flex_1().min_w_0().truncate().child(row.label.clone()))
                        .child(div().flex_none().font_family(self.mono.clone()).text_color(if dimmed { c.dim } else { c.text }).child(keys)),
                );
            }
        }
        // Live whatever state the config is in: opening the file is how a binding changes.
        let open = settings::pill("keybindings-open-config", "Open config", false, true, c).on_click(|_, _, cx| settings::open_config_in_editor(cx));
        let footer = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(20.0))
            .py(px(10.0))
            .border_t_1()
            .border_color(c.border)
            .child(div().flex_1().min_w_0().truncate().child(footer_text()))
            .child(open);
        div()
            .id("keybindings")
            .key_context("Keybindings")
            .track_focus(&self.focus)
            .on_key_down(|ev: &KeyDownEvent, window, _| {
                if ev.keystroke.key == "escape" && !ev.keystroke.modifiers.modified() {
                    window.remove_window();
                }
            })
            // Cmd+W arrives as the close-tab action.
            .on_action(|_: &keymap::CloseTab, window, _| window.remove_window())
            .on_action(|_: &keymap::NewWindow, _, cx| aux::forward_new_window(cx))
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(c.text)
            .text_size(px(13.0))
            .font_family(SharedString::from(UI_FONT))
            .child(chrome::aux_title_bar(TITLE, &self.theme))
            .child(list)
            .child(footer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_footer_names_the_config_section_and_an_unbound_row_reads_none_dimmed() {
        assert_eq!(footer_text(), "Change them under [keybindings] in config.toml");
        assert_eq!(keys_text(&KeyRow { label: "Save".into(), keys: Some("Cmd+S".into()) }), ("Cmd+S".to_string(), false));
        assert_eq!(keys_text(&KeyRow { label: "Doctor".into(), keys: None }), ("none".to_string(), true));
    }
}
