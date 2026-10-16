//! The About window: the build, update, install and path details, and the attribution.
//! Filled in Task 3b.
use gpui::{div, prelude::*, Context, Window};

/// The About window's root view. Filled in Task 3b.
#[allow(dead_code)] // Used from Task 3b on.
pub struct AboutWindow;

impl Render for AboutWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
