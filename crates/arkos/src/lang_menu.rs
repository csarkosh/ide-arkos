//! A code block's language list (code block spec, section 1), drawn over the window under the
//! block's pill: a search field showing what was typed ("Search for a language..." before
//! anything is), then the languages it matches, at most ten rows kept around the selection. A row
//! takes the mouse: hovering selects it, a click sets it.

use crate::Root;
use gpui::{div, prelude::*, px, Context, Div, MouseDownEvent};

const ROWS: usize = 10;

pub fn render_lang_menu(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let viewer = root.active_viewer()?;
    let v = viewer.read(cx);
    let l = v.lang.clone()?;
    let at = v.lang_at?;
    let langs = crate::md_code::filter_languages(&l.query);
    let style = root.effect_style();
    let c = &root.theme.chrome;
    let (dim, text) = (c.text_dim.hsla(), c.text.hsla());
    let hover = crate::chrome::menu_colors(&root.theme, style.menus).hover;
    let row_h = px(root.ui_size(12.0) * 1.9);
    let search = div().px(px(10.0)).h(row_h).flex().items_center().border_b_1().border_color(c.border.hsla()).map(|d| {
        if l.query.is_empty() {
            d.text_color(dim).child("Search for a language...")
        } else {
            d.child(l.query.clone()).child(div().ml(px(1.0)).w(px(1.5)).h(px(root.ui_size(12.0) * 1.2)).bg(text))
        }
    });
    let mut panel = div()
        .absolute()
        .left(at.x)
        .top(at.y + px(4.0))
        .w(px(220.0))
        .flex()
        .flex_col()
        .pb(px(4.0))
        .map(|d| crate::chrome::menu_frame(d, &root.theme, &style))
        .font_family(root.ui_family())
        .text_size(px(root.ui_size(12.0)))
        .text_color(text)
        .occlude()
        .on_any_mouse_down(|_: &MouseDownEvent, _window, cx| cx.stop_propagation())
        .child(search);
    if langs.is_empty() {
        return Some(panel.child(div().px(px(10.0)).h(row_h).flex().items_center().text_color(dim).child("No Results")));
    }
    let first = l.selected.saturating_sub(ROWS - 1);
    for (i, (id, name)) in langs.iter().enumerate().skip(first).take(ROWS) {
        let (v_hover, v_click, id) = (viewer.clone(), viewer.clone(), *id);
        panel = panel.child(
            div()
                .id(("lang-row", i))
                .px(px(10.0))
                .h(row_h)
                .flex()
                .items_center()
                .cursor_pointer()
                .when(i == l.selected, |r| r.bg(hover))
                .child(*name)
                .on_hover(move |over, _window, cx| {
                    if *over {
                        v_hover.update(cx, |v, cx| {
                            if let Some(l) = v.lang.as_mut() {
                                l.selected = i;
                                cx.notify();
                            }
                        });
                    }
                })
                .on_click(move |_ev, _window, cx| v_click.update(cx, |v, cx| v.set_language(id, cx))),
        );
    }
    Some(panel)
}
