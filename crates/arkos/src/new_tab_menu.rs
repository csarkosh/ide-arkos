//! The tab bar's "+" menu (new tab menu spec): Claude, Codex or Shell. Claude and Codex open a
//! terminal tab whose shell types the agent's command once it has started; Shell is the plain
//! terminal tab "+" opened before. Drawn and dismissed like the terminal's right-click menu
//! (`link_menu`).

use crate::tree::PaneId;
use crate::motion;
use crate::Root;
use gpui::{div, prelude::*, px, ClickEvent, Context, Div, MouseDownEvent, Pixels, Point};
use std::time::Instant;

/// The menu's width, in points: wide enough for "Shell" and its shortcut.
pub const WIDTH: f32 = 180.0;

/// The open menu: the pane whose "+" opened it, where the press was, in window pixels, and the
/// row the keys act on (Chrome keys spec, section 3).
pub struct NewTabMenu {
    pub pane: PaneId,
    pub at: Point<Pixels>,
    pub mark: usize,
}

/// What a key does while the menu is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKey {
    /// The mark moves to this row.
    Mark(usize),
    /// The row is opened.
    Open(NewTab),
    Close,
    /// Not the menu's: the key goes on as if the menu were not there.
    Pass,
}

/// A plain key on the open menu with `mark` on a row: Up and Down move the mark (stopping at
/// either end), Enter opens the marked row, Escape closes the menu.
pub fn menu_key(key: &str, mark: usize) -> MenuKey {
    let last = NewTab::ALL.len() - 1;
    match key {
        "up" => MenuKey::Mark(mark.saturating_sub(1)),
        "down" => MenuKey::Mark((mark + 1).min(last)),
        "enter" => MenuKey::Open(NewTab::ALL[mark.min(last)]),
        "escape" => MenuKey::Close,
        _ => MenuKey::Pass,
    }
}

/// What a row opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewTab {
    Claude,
    Codex,
    Shell,
}

impl NewTab {
    /// The rows, in order: the agents first, then the plain shell.
    pub const ALL: [NewTab; 3] = [NewTab::Claude, NewTab::Codex, NewTab::Shell];

    pub fn label(self) -> &'static str {
        match self {
            NewTab::Claude => "Claude",
            NewTab::Codex => "Codex",
            NewTab::Shell => "Shell",
        }
    }

    /// What the new tab's shell types once it has started: the agent's command and Enter, or
    /// nothing for a plain shell. Typed, not run in place of the shell, so quitting the agent
    /// leaves a prompt in the tab.
    pub fn input(self) -> Option<Vec<u8>> {
        match self {
            NewTab::Claude => Some(b"claude\r".to_vec()),
            NewTab::Codex => Some(b"codex\r".to_vec()),
            NewTab::Shell => None,
        }
    }
}

/// The menu's left edge for a press at `x`: its right edge a little past the press, so it hangs
/// under the "+" at the bar's right end, and never past the window's left edge.
pub fn left_for(x: f32, width: f32) -> f32 {
    (x + 12.0 - width).max(4.0)
}

/// The menu, drawn over everything under the "+". A mouse-down anywhere else closes it (the
/// root's listener); its rows stop the event so their click lands, and a row closes the menu
/// once it has opened its tab.
pub fn render_new_tab_menu(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let (menu, opacity) = root.new_tab_menu.current(root.frame_now)?;
    let live = root.new_tab_menu.value().is_some();
    let pane = menu.pane;
    let style = root.effect_style();
    let c = &root.theme.chrome;
    let dim = c.text_dim.hsla();
    let shell_keys = crate::keymap::keys_for("new terminal tab", &root.config.keybindings).map(|k| crate::keymap::pretty_keys(&k)).unwrap_or_default();
    let mut m = div()
        .absolute()
        .left(px(left_for(f32::from(menu.at.x), WIDTH)))
        .top(menu.at.y + px(14.0) - px(motion::lift(opacity, motion::MENU_LIFT)))
        .opacity(opacity)
        .w(px(WIDTH))
        .flex()
        .flex_col()
        .py(px(4.0))
        .map(|d| crate::chrome::menu_frame(d, &root.theme, &style))
        .font_family(root.ui_family())
        .text_size(px(root.ui_size(12.0)))
        .text_color(c.text.hsla())
        .when(live, |m| m.occlude().on_any_mouse_down(|_: &MouseDownEvent, _window, cx| cx.stop_propagation()));
    let hover = crate::chrome::menu_colors(&root.theme, style.menus).hover;
    let (effect_rows, hovered) = (style.menu_edges && style.highlights, root.effect_hover);
    if style.menu_edges {
        m = m.child(crate::chrome::menu_report(0x17, opacity, cx));
    }
    for (i, kind) in NewTab::ALL.into_iter().enumerate() {
        let keys = if kind == NewTab::Shell { shell_keys.clone() } else { String::new() };
        let row = div().id(("new-tab-row", i)).px(px(10.0)).py(px(4.0)).flex().justify_between().child(kind.label()).child(div().text_color(dim).child(keys));
        // The marked row (the keys') is lit as a hovered one is, and the mouse moves the mark.
        let marked = live && menu.mark == i;
        m = m.child(row.when(live, |row| {
            let key = crate::effects::blocks::menu_row_key(0x17, i);
            crate::chrome::menu_row_hover(row.cursor_pointer(), key, marked || hovered == Some(key), effect_rows, hover, cx)
                .on_hover(cx.listener(move |root, over: &bool, _window, cx| {
                    if let (true, Some(m)) = (*over, root.new_tab_menu.value_mut()) {
                        m.mark = i;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                    root.new_tab_menu.hide(Instant::now());
                    root.new_tab_of(pane, kind, cx);
                }))
        }));
    }
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_lists_claude_then_codex_then_the_shell() {
        let labels: Vec<&str> = NewTab::ALL.iter().map(|k| k.label()).collect();
        assert_eq!(labels, ["Claude", "Codex", "Shell"]);
    }

    #[test]
    fn an_agent_s_tab_types_its_command_and_a_shell_s_types_nothing() {
        assert_eq!(NewTab::Claude.input().as_deref(), Some(&b"claude\r"[..]));
        assert_eq!(NewTab::Codex.input().as_deref(), Some(&b"codex\r"[..]));
        assert_eq!(NewTab::Shell.input(), None, "the plain terminal tab of before");
    }

    #[test]
    fn the_arrows_move_the_mark_enter_opens_it_and_escape_closes() {
        assert_eq!(menu_key("down", 0), MenuKey::Mark(1));
        assert_eq!(menu_key("down", 2), MenuKey::Mark(2), "stops at Shell");
        assert_eq!(menu_key("up", 0), MenuKey::Mark(0), "stops at Claude");
        assert_eq!(menu_key("enter", 0), MenuKey::Open(NewTab::Claude), "Cmd+T then Enter: a Claude tab");
        assert_eq!(menu_key("enter", 1), MenuKey::Open(NewTab::Codex));
        assert_eq!(menu_key("escape", 1), MenuKey::Close);
        assert_eq!(menu_key("a", 0), MenuKey::Pass);
    }

    #[test]
    fn the_menu_hangs_under_the_plus_and_stays_inside_the_window() {
        assert_eq!(left_for(1000.0, WIDTH), 1000.0 + 12.0 - WIDTH, "its right edge just past the press");
        assert_eq!(left_for(40.0, WIDTH), 4.0, "a narrow pane at the window's left edge");
    }
}
