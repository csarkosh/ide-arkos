//! A markdown document's right-click menu (spec `2026-10-30-typing-rules-design.md`, section 8),
//! drawn and dismissed like the terminal's (`link_menu`): Cut, Copy and Paste, a line, then Bold,
//! Italic, Strikethrough, Code and Link, each doing what its key does. gpui delivers a
//! Control+click as a right press, so it opens the menu too.

use crate::md_format::Format;
use crate::motion;
use crate::viewer::{KeyAction, Viewer};
use crate::Root;
use gpui::{div, prelude::*, px, ClickEvent, Context, Div, MouseDownEvent, Pixels, Point, WeakEntity};
use std::time::Instant;

/// The open menu: the document it acts on, whether something was selected when it opened, and
/// where the press was, in window pixels.
pub struct MdMenu {
    pub viewer: WeakEntity<Viewer>,
    pub selection: bool,
    pub at: Point<Pixels>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MdRow {
    Cut,
    Copy,
    Paste,
    Format(Format),
    Link,
}

/// The rows in order, each with whether it can run: Cut and Copy need a selection. `None` is the
/// line between the clipboard rows and the formats.
pub fn menu_rows(selection: bool) -> Vec<Option<(&'static str, MdRow, bool)>> {
    vec![
        Some(("Cut", MdRow::Cut, selection)),
        Some(("Copy", MdRow::Copy, selection)),
        Some(("Paste", MdRow::Paste, true)),
        None,
        Some(("Bold", MdRow::Format(Format::Bold), true)),
        Some(("Italic", MdRow::Format(Format::Italic), true)),
        Some(("Strikethrough", MdRow::Format(Format::Strike), true)),
        Some(("Code", MdRow::Format(Format::Code), true)),
        Some(("Link", MdRow::Link, true)),
    ]
}

fn run_row(row: MdRow, viewer: &WeakEntity<Viewer>, cx: &mut Context<Root>) {
    let Some(v) = viewer.upgrade() else { return };
    v.update(cx, |v, cx| match row {
        MdRow::Cut => v.act(KeyAction::Cut, cx),
        MdRow::Copy => v.act(KeyAction::Copy, cx),
        MdRow::Paste => v.act(KeyAction::Paste, cx),
        MdRow::Format(f) => v.format(f, cx),
        MdRow::Link => v.insert_link(cx),
    });
}

/// The menu, drawn over everything at the press like the terminal's. A mouse-down anywhere else
/// closes it (the root's listener); its own rows stop the event so their click lands, and a row
/// closes the menu once its action has run. A row that cannot run is dimmed and takes no click.
pub fn render_md_menu(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let (menu, opacity) = root.md_menu.current(root.frame_now)?;
    let live = root.md_menu.value().is_some();
    let style = root.effect_style();
    let c = &root.theme.chrome;
    let (text, dim, line) = (c.text.hsla(), c.text_dim.hsla(), c.border.hsla());
    let mut m = div()
        .absolute()
        .left(menu.at.x)
        .top(menu.at.y - px(motion::lift(opacity, motion::MENU_LIFT)))
        .opacity(opacity)
        .min_w(px(180.0))
        .flex()
        .flex_col()
        .py(px(4.0))
        .map(|d| crate::chrome::menu_frame(d, &root.theme, &style))
        .font_family(root.ui_family())
        .text_size(px(root.ui_size(12.0)))
        .text_color(text)
        .when(live, |m| m.occlude().on_any_mouse_down(|_: &MouseDownEvent, _window, cx| cx.stop_propagation()));
    let hover = crate::chrome::menu_colors(&root.theme, style.menus).hover;
    let (effect_rows, hovered) = (style.menu_edges && style.highlights, root.effect_hover);
    if style.menu_edges {
        m = m.child(crate::chrome::menu_report(0x12, opacity, cx));
    }
    for (i, row) in menu_rows(menu.selection).into_iter().enumerate() {
        let Some((label, action, enabled)) = row else {
            m = m.child(div().my(px(4.0)).h(px(1.0)).mx(px(8.0)).bg(line));
            continue;
        };
        let viewer = menu.viewer.clone();
        let el = div().id(("md-menu-row", i)).px(px(10.0)).py(px(4.0)).child(label);
        m = m.child(if !enabled {
            el.text_color(dim).into_any_element()
        } else {
            el.when(live, |row| {
                let key = crate::effects::blocks::menu_row_key(0x12, i);
                crate::chrome::menu_row_hover(row.cursor_pointer(), key, hovered == Some(key), effect_rows, hover, cx).on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                    run_row(action, &viewer, cx);
                    root.md_menu.hide(Instant::now());
                    cx.notify();
                }))
            })
            .into_any_element()
        });
    }
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_lists_the_clipboard_then_the_formats_and_dims_cut_and_copy_without_a_selection() {
        let labels: Vec<Option<&str>> = menu_rows(true).iter().map(|r| r.map(|(l, _, _)| l)).collect();
        assert_eq!(labels, vec![Some("Cut"), Some("Copy"), Some("Paste"), None, Some("Bold"), Some("Italic"), Some("Strikethrough"), Some("Code"), Some("Link")]);
        let enabled = |sel: bool| menu_rows(sel).iter().flatten().filter(|(_, _, on)| *on).map(|(l, _, _)| *l).collect::<Vec<_>>();
        assert_eq!(enabled(false), vec!["Paste", "Bold", "Italic", "Strikethrough", "Code", "Link"], "Cut and Copy need a selection");
        assert_eq!(enabled(true).len(), 8);
    }
}
