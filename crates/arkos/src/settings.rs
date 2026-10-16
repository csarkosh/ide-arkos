//! The Settings window: every change written to config.toml in place and applied at once.
//! Filled in Task 4.
use gpui::{div, prelude::*, Context, Window};

/// The Settings window's root view. Filled in Task 4.
#[allow(dead_code)] // Used from Task 4 on.
pub struct SettingsWindow;

impl Render for SettingsWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
