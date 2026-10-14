//! The window chrome (spec 4): a top bar with the project name and a bottom status bar with the
//! project path, daemon status, session count and the count of sessions waiting on the user.

use gpui::{div, prelude::*, px, rgb, Div};
use std::path::Path;

pub const TOP_BAR_H: f32 = 30.0;
pub const STATUS_BAR_H: f32 = 24.0;
const BAR_BG: u32 = 0x0a0e14;
const FG: u32 = 0xd6dae0;
const DIM: u32 = 0x8a93a3;

pub fn status_text(sessions: usize, waiting: usize) -> String {
    let base = match sessions {
        0 => "no sessions".to_string(),
        1 => "1 session".to_string(),
        n => format!("{n} sessions"),
    };
    if waiting > 0 {
        format!("{base}, {waiting} waiting")
    } else {
        base
    }
}

pub fn top_bar(project_name: &str) -> Div {
    div()
        .h(px(TOP_BAR_H))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(BAR_BG))
        .text_color(rgb(FG))
        .font_family("Menlo")
        .text_size(px(13.0))
        .child(project_name.to_string())
}

pub fn status_bar(project: &Path, daemon: &str, sessions: usize, waiting: usize) -> Div {
    div()
        .h(px(STATUS_BAR_H))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .px(px(10.0))
        .bg(rgb(BAR_BG))
        .text_color(rgb(DIM))
        .font_family("Menlo")
        .text_size(px(12.0))
        .child(project.display().to_string())
        .child(div().flex().gap(px(16.0)).child(daemon.to_string()).child(status_text(sessions, waiting)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_text_counts_sessions_and_waiting() {
        assert_eq!(status_text(1, 0), "1 session");
        assert_eq!(status_text(4, 2), "4 sessions, 2 waiting");
        assert_eq!(status_text(0, 0), "no sessions");
    }
}
