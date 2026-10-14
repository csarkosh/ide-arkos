//! The window chrome (spec 4): a top bar with the project name and a bottom status bar with the
//! project path, a ready update, daemon status, session count and the count of sessions waiting
//! on the user.

use crate::theme::Theme;
use gpui::{div, prelude::*, px, Div, SharedString};
use std::path::Path;

pub const TOP_BAR_H: f32 = 30.0;
pub const STATUS_BAR_H: f32 = 24.0;

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

/// The right side of the status bar, left to right.
pub fn status_items(update: Option<&str>, daemon: &str, sessions: usize, waiting: usize) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(u) = update {
        v.push(u.to_string());
    }
    v.push(daemon.to_string());
    v.push(status_text(sessions, waiting));
    v
}

pub fn top_bar(project_name: &str, family: SharedString, theme: &Theme) -> Div {
    div()
        .h(px(TOP_BAR_H))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.chrome.title_bar.hsla())
        .text_color(theme.chrome.text.hsla())
        .font_family(family)
        .text_size(px(13.0))
        .child(project_name.to_string())
}

pub fn status_bar(project: &Path, update: Option<&str>, daemon: &str, sessions: usize, waiting: usize, family: SharedString, theme: &Theme) -> Div {
    div()
        .h(px(STATUS_BAR_H))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .px(px(10.0))
        .bg(theme.chrome.status_bar.hsla())
        .text_color(theme.chrome.text_dim.hsla())
        .font_family(family)
        .text_size(px(12.0))
        .child(project.display().to_string())
        .child(div().flex().gap(px(16.0)).children(status_items(update, daemon, sessions, waiting)))
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

    #[test]
    fn status_text_mentions_a_ready_update_first() {
        assert_eq!(status_items(Some("update ready: 0.2.0"), "sarkosd connected", 2, 0), vec!["update ready: 0.2.0", "sarkosd connected", "2 sessions"]);
        assert_eq!(status_items(None, "sarkosd connected", 1, 0), vec!["sarkosd connected", "1 session"]);
    }
}
