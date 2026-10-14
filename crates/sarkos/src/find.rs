//! Find in a terminal (spec 4: Cmd+F). The state lives on the `Terminal`, the search runs in
//! `TerminalCore`, and the bar is drawn at the top right of the pane.
use crate::keymap::tab_cycle;
use crate::line_edit::{self, Edit, LineEdit};
use crate::terminal::{Match, Terminal};
use crate::theme::Theme;
use crate::Root;
use gpui::{div, prelude::*, px, ClickEvent, Context, Div, Entity, FocusHandle, KeyDownEvent, Keystroke, MouseButton, MouseDownEvent, SharedString};

pub struct FindState {
    pub query: LineEdit,
    pub matches: Vec<Match>,
    /// Index into `matches`; meaningless when empty.
    pub current: usize,
    pub focus: FocusHandle,
}

/// A new query points at the newest match: what was just printed is what one looks for.
pub fn initial_match(len: usize) -> usize {
    len.saturating_sub(1)
}

pub fn count_text(current: usize, len: usize) -> String {
    if len == 0 {
        "no matches".to_string()
    } else {
        format!("{} of {len}", current + 1)
    }
}

impl Terminal {
    pub fn open_find(&mut self, cx: &mut Context<Self>) {
        if self.find.is_none() {
            self.find = Some(FindState { query: LineEdit::new(), matches: Vec::new(), current: 0, focus: cx.focus_handle() });
        }
        cx.notify();
    }

    pub fn close_find(&mut self, cx: &mut Context<Self>) {
        self.find = None;
        cx.notify();
    }

    /// Re-runs the search for the current query, points at the newest match and scrolls to it.
    fn find_changed(&mut self, cx: &mut Context<Self>) {
        let Some(f) = self.find.as_mut() else { return };
        f.matches = self.core.search(&f.query.text);
        f.current = initial_match(f.matches.len());
        if let Some(m) = f.matches.get(f.current) {
            let line = m.start().line;
            self.core.reveal_line(line);
        }
        cx.notify();
    }

    /// Output changed the grid: search again, keep the position, do not scroll.
    pub fn refresh_find(&mut self) {
        if let Some(f) = self.find.as_mut() {
            f.matches = self.core.search(&f.query.text);
            f.current = f.current.min(initial_match(f.matches.len()));
        }
    }

    pub fn find_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(f) = self.find.as_mut() else { return };
        if f.matches.is_empty() {
            return;
        }
        f.current = tab_cycle(f.current, f.matches.len(), delta);
        let line = f.matches[f.current].start().line;
        self.core.reveal_line(line);
        cx.notify();
    }

    pub fn find_paste(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(f) = self.find.as_mut() {
            f.query.insert_str(text);
            self.find_changed(cx);
        }
    }

    /// Enter is next, Shift+Enter previous, Escape closes. Returns true when the bar closed.
    pub fn find_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(f) = self.find.as_mut() else { return true };
        match f.query.handle(ks) {
            Edit::Changed => self.find_changed(cx),
            Edit::Submit => self.find_step(1, cx),
            Edit::SubmitBack => self.find_step(-1, cx),
            Edit::Cancel => {
                self.close_find(cx);
                return true;
            }
            Edit::Up => self.find_step(-1, cx),
            Edit::Down => self.find_step(1, cx),
            Edit::Unchanged => {}
        }
        false
    }
}

/// The bar: query, count, close. Keys go to the terminal's `find_key`; Cmd+V pastes.
pub fn render(terminal: &Entity<Terminal>, theme: &Theme, family: SharedString, cx: &mut Context<Root>) -> Option<Div> {
    let (query, count, focus) = {
        let t = terminal.read(cx);
        let f = t.find.as_ref()?;
        (f.query.clone(), count_text(f.current, f.matches.len()), f.focus.clone())
    };
    let t_keys = terminal.clone();
    let t_close = terminal.clone();
    Some(
        div()
            .absolute()
            .top(px(6.0))
            .right(px(14.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .px(px(10.0))
            .py(px(5.0))
            .rounded(px(6.0))
            .bg(theme.chrome.banner.hsla())
            .border_1()
            .border_color(theme.chrome.border.hsla())
            .text_color(theme.chrome.text.hsla())
            .font_family(family.clone())
            .text_size(px(12.0))
            .track_focus(&focus)
            .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _window, cx| cx.stop_propagation())
            .on_key_down(cx.listener(move |_root, ev: &KeyDownEvent, _window, cx| {
                // The bar sits inside the terminal's element, whose key handler would otherwise
                // send the same key to the shell.
                cx.stop_propagation();
                let ks = &ev.keystroke;
                if ks.modifiers.platform && ks.key == "v" {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        t_keys.update(cx, |t, cx| t.find_paste(&text, cx));
                    }
                    return;
                }
                if ks.modifiers.platform {
                    return;
                }
                t_keys.update(cx, |t, cx| {
                    t.find_key(ks, cx);
                });
            }))
            .child(div().w(px(220.0)).child(line_edit::render(&query, "find", theme, family, 12.0)))
            .child(div().text_color(theme.chrome.text_dim.hsla()).child(count))
            .child(
                div()
                    .id(("find-close", terminal.entity_id()))
                    .text_color(theme.chrome.text_dim.hsla())
                    .hover(|s| s.text_color(theme.chrome.text.hsla()))
                    .cursor_pointer()
                    .child("×")
                    .on_click(cx.listener(move |_root, _ev: &ClickEvent, _window, cx| t_close.update(cx, |t, cx| t.close_find(cx)))),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_search_starts_at_the_newest_match() {
        assert_eq!(initial_match(0), 0);
        assert_eq!(initial_match(1), 0);
        assert_eq!(initial_match(5), 4);
        assert_eq!(count_text(0, 0), "no matches");
        assert_eq!(count_text(0, 1), "1 of 1");
        assert_eq!(count_text(4, 5), "5 of 5");
    }
}
