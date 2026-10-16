//! The Keybindings window: every action with the keys in force, grouped by section. A stub until
//! the window is drawn, so the registry's handle has its type.
use gpui::{div, prelude::*, Context, Window};

/// The Keybindings window's view.
#[allow(dead_code)] // Used from Task 2 on.
pub struct KeybindingsWindow;

impl Render for KeybindingsWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
