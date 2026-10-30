//! The slash menu drawn over the window (spec `2026-10-30-slash-menu-design.md`): under the `/`
//! of the focused markdown document, the rows `md_slash` gives for the text typed after it, a hint
//! column and a chevron for a submenu, at most ten rows with the selection kept in view, and the
//! open submenu in a second panel beside its row. The menu follows typing, so it appears and goes
//! at once, as the caret does. A row takes the mouse: hovering selects it, a click runs it.

use crate::viewer::Viewer;
use crate::Root;
use gpui::{div, prelude::*, px, Context, Div, Entity, MouseDownEvent, Pixels};

const ROWS: usize = 10;
const WIDTH: f32 = 280.0;

/// The panel: its rows from `first`, the selected one marked, each running or opening on a click.
#[allow(clippy::too_many_arguments)]
fn panel(root: &Root, viewer: &Entity<Viewer>, rows: &[crate::md_slash::Entry], selected: usize, sub_of: Option<usize>, row_h: Pixels) -> Div {
    let style = root.effect_style();
    let c = &root.theme.chrome;
    let hover = crate::chrome::menu_colors(&root.theme, style.menus).hover;
    let dim = c.text_dim.hsla();
    let first = selected.saturating_sub(ROWS - 1);
    let mut p = div()
        .w(px(WIDTH))
        .flex()
        .flex_col()
        .py(px(4.0))
        .map(|d| crate::chrome::menu_frame(d, &root.theme, &style))
        .font_family(root.ui_family())
        .text_size(px(root.ui_size(12.0)))
        .text_color(c.text.hsla())
        .occlude()
        .on_any_mouse_down(|_: &MouseDownEvent, _window, cx| cx.stop_propagation());
    if rows.is_empty() {
        return p.child(div().px(px(10.0)).h(row_h).flex().items_center().text_color(dim).child("No Results"));
    }
    for (i, e) in rows.iter().enumerate().skip(first).take(ROWS) {
        let (v_hover, v_click) = (viewer.clone(), viewer.clone());
        let (cmd, has_sub) = (e.command, !e.submenu.is_empty() && sub_of.is_none());
        let row = div()
            .id(("slash-row", i + sub_of.map_or(0, |s| 1000 * (s + 1))))
            .px(px(10.0))
            .h(row_h)
            .flex()
            .flex_row()
            .items_center()
            .cursor_pointer()
            .when(i == selected, |r| r.bg(hover))
            .child(div().flex_1().child(e.label.clone()))
            .children(e.hint.map(|h| div().text_color(dim).pr(px(6.0)).child(h)))
            .when(has_sub, |r| r.child(div().text_color(dim).child("\u{203a}")))
            .on_hover(move |hovered, _window, cx| {
                if *hovered {
                    v_hover.update(cx, |v, cx| {
                        if let Some(s) = v.slash.as_mut() {
                            match sub_of {
                                Some(_) => s.sub_selected = i,
                                None => {
                                    s.selected = i;
                                    if s.sub.is_some_and(|o| o != i) {
                                        s.sub = None;
                                    }
                                }
                            }
                            cx.notify();
                        }
                    });
                }
            })
            .on_click(move |_ev, _window, cx| {
                v_click.update(cx, |v, cx| {
                    if has_sub {
                        if let Some(s) = v.slash.as_mut() {
                            s.selected = i;
                            s.sub = Some(i);
                            s.sub_selected = 0;
                        }
                        cx.notify();
                    } else {
                        v.run_slash(cmd, cx);
                    }
                });
            });
        p = p.child(row);
    }
    p
}

/// The focused document's slash menu, when it is open and its `/` was laid out in the last frame.
pub fn render_slash_menu(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let viewer = root.active_viewer()?;
    let v = viewer.read(cx);
    let s = v.slash.clone()?;
    let (at, line_h) = v.slash_anchor()?;
    let rows = s.rows();
    let row_h = px(root.ui_size(12.0) * 1.9);
    let main = panel(root, &viewer, &rows, s.selected, None, row_h);
    let sub = s.sub.and_then(|i| rows.get(i).map(|e| (i, e.submenu.clone()))).map(|(i, items)| {
        let top = row_h * (i - s.selected.saturating_sub(ROWS - 1)) as f32;
        div().pt(top).child(panel(root, &viewer, &items, s.sub_selected, Some(i), row_h))
    });
    Some(div().absolute().left(at.x).top(at.y + line_h + px(4.0)).flex().flex_row().items_start().gap(px(4.0)).child(main).children(sub))
}
