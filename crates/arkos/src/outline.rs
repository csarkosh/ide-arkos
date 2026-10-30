//! The sidebar's Outline section (structure spec, section 2): the focused markdown document's
//! headings as a tree, the one in view highlighted as the document scrolls, a click scrolling to a
//! heading, a chevron collapsing a heading's children here, and a preview of a section beside the
//! sidebar after the pointer rests on its row for half a second.

use crate::md_fold;
use crate::sidebar::{INDENT, ROW_H};
use crate::theme::Theme;
use crate::Root;
use gpui::{div, prelude::*, px, ClickEvent, Context, Div, MouseMoveEvent, Stateful};
use std::time::{Duration, Instant};

/// How long the pointer rests on a row before its preview shows.
const PREVIEW_AFTER: Duration = Duration::from_millis(500);
/// The section's most height before it scrolls.
const MAX_H: f32 = 280.0;

/// The rows: the focused markdown document's outline, or "No headings".
pub fn render_rows(root: &mut Root, theme: &Theme, cx: &mut Context<Root>) -> Stateful<Div> {
    let c = &theme.chrome;
    let (selected_bg, hover_bg, dim) = (c.tab_active.hsla(), c.tab_inactive.hsla(), c.text_dim.hsla());
    let list = div().id("outline-rows").flex().flex_col().max_h(px(MAX_H)).overflow_y_scroll().track_scroll(&root.sidebar.outline_scroll);
    let viewer = root.active_viewer().filter(|v| v.read(cx).doc.is_some());
    let Some(viewer) = viewer else { return list.child(empty(dim)) };
    let v = viewer.read(cx);
    if v.heads.is_empty() {
        return list.child(empty(dim));
    }
    let heads = v.heads.clone();
    let current = md_fold::current(&heads, v.list_state.logical_scroll_top().item_ix);
    let rows = md_fold::outline(&heads);
    let collapsed: std::collections::BTreeSet<usize> = heads.iter().enumerate().filter(|(_, h)| root.sidebar.outline_collapsed.contains(&h.key())).map(|(i, _)| i).collect();
    let mut list = list;
    for r in md_fold::visible_rows(&rows, &collapsed) {
        let h = &heads[r.head];
        let (key, block, head, folded_here) = (h.key(), h.block, r.head, collapsed.contains(&r.head));
        let (v_click, w_move, w_hover, w_toggle) = (viewer.clone(), cx.weak_entity(), cx.weak_entity(), cx.weak_entity());
        let chevron = div()
            .id(("outline-chevron", r.head))
            .flex_none()
            .w(px(12.0))
            .text_color(dim)
            .when(r.has_children, |d| {
                d.cursor_pointer().child(if folded_here { "\u{25b8}" } else { "\u{25be}" }).on_click(move |_ev: &ClickEvent, _window, cx| {
                    cx.stop_propagation();
                    let _ = w_toggle.update(cx, |root, cx| {
                        if !root.sidebar.outline_collapsed.remove(&key) {
                            root.sidebar.outline_collapsed.insert(key.clone());
                        }
                        cx.notify();
                    });
                })
            });
        list = list.child(
            div()
                .id(("outline-row", r.head))
                .w_full()
                .h(px(ROW_H))
                .flex_none()
                .pl(px(10.0 + r.depth as f32 * INDENT))
                .pr(px(8.0))
                .flex()
                .items_center()
                .gap(px(4.0))
                .cursor_pointer()
                .when(current == Some(r.head), |d| d.bg(selected_bg))
                .hover(move |s| s.bg(hover_bg))
                .child(chevron)
                .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(h.text.clone()))
                .on_click(move |_ev: &ClickEvent, _window, cx| v_click.update(cx, |v, cx| v.reveal_block(block, cx)))
                .on_mouse_move(move |ev: &MouseMoveEvent, _window, cx| {
                    let y = f32::from(ev.position.y);
                    let _ = w_move.update(cx, |root, cx| {
                        if root.outline_hover.is_some_and(|(i, _, _)| i == head) {
                            return;
                        }
                        root.outline_hover = Some((head, y, Instant::now()));
                        // Drawn again when the half second is up, for the preview.
                        cx.spawn(async move |root, cx| {
                            cx.background_executor().timer(PREVIEW_AFTER).await;
                            let _ = root.update(cx, |_, cx| cx.notify());
                        })
                        .detach();
                    });
                })
                .on_hover(move |over: &bool, _window, cx| {
                    if !over {
                        let _ = w_hover.update(cx, |root, cx| {
                            if root.outline_hover.is_some_and(|(i, _, _)| i == head) {
                                root.outline_hover = None;
                                cx.notify();
                            }
                        });
                    }
                }),
        );
    }
    list
}

fn empty(dim: gpui::Hsla) -> Div {
    div().h(px(ROW_H)).px(px(10.0)).flex().items_center().text_color(dim).child("No headings")
}

/// The preview of the hovered row's section, beside the sidebar at the row's height, once the
/// pointer has rested there for half a second.
pub fn render_preview(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let (head, y, since) = root.outline_hover?;
    if since.elapsed() < PREVIEW_AFTER || !root.sidebar.outline_open || root.sidebar.collapsed {
        return None;
    }
    let viewer = root.active_viewer()?;
    let v = viewer.read(cx);
    let doc = v.doc.as_ref()?;
    v.heads.get(head)?;
    let lines = md_fold::preview(&v.heads, head, &v.blocks, doc.lines(), 6);
    let style = root.effect_style();
    let c = &root.theme.chrome;
    let mut panel = div()
        .absolute()
        .left(px(root.sidebar.width + 6.0))
        .top(px(y - 12.0))
        .max_w(px(360.0))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .px(px(10.0))
        .py(px(8.0))
        .map(|d| crate::chrome::menu_frame(d, &root.theme, &style))
        .font_family(root.ui_family())
        .text_size(px(root.ui_size(12.0)))
        .text_color(c.text_dim.hsla());
    for (i, l) in lines.into_iter().enumerate() {
        panel = panel.child(div().overflow_hidden().whitespace_nowrap().text_ellipsis().when(i == 0, |d| d.text_color(c.text.hsla()).font_weight(gpui::FontWeight::SEMIBOLD)).child(l));
    }
    Some(panel)
}
