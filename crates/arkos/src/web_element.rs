//! Draws a web tab (web pane spec, section 4): the bar above the page (back, forward, reload or
//! stop, the address, the progress hairline, open outside), the content rectangle the web view
//! is placed over (a `canvas` that reports its bounds to `Root` every frame it is drawn), and
//! the two screens Arkos draws itself, blank and failed. The bar's arithmetic
//! (`progress_width`) is pure and tested; the rest reads the view and the theme as they are at
//! this draw, never keeping a colour.

use crate::theme::Theme;
use crate::tree::{PaneId, TabId};
use crate::web::{LoadState, BAR_H};
use crate::web_view::WebView;
use crate::Root;
use gpui::{canvas, div, prelude::*, px, ClickEvent, Context, Div, Entity, KeyDownEvent, MouseButton, MouseDownEvent, SharedString};
use std::time::Instant;

/// The glyph buttons' width.
const BUTTON_W: f32 = 24.0;
/// The progress hairline's height, along the bar's bottom edge.
const PROGRESS_H: f32 = 2.0;

/// How wide the progress hairline is on a bar `bar_w` wide: the load's share of it, never past
/// the bar, nothing for a value that is not one.
pub fn progress_width(progress: f64, bar_w: f32) -> f32 {
    if !(bar_w > 0.0) || !progress.is_finite() {
        return 0.0;
    }
    (progress.max(0.0).min(1.0) * f64::from(bar_w)) as f32
}

pub fn render(view: &Entity<WebView>, pane: PaneId, tab: TabId, theme: &Theme, family: SharedString, cx: &mut Context<Root>) -> Div {
    let now = Instant::now();
    let more = view.update(cx, |v, cx| v.frame_step(now, cx));
    let v = view.read(cx);
    let c = &theme.chrome;
    let (text, dim, bg, banner, border, accent) = (c.text.hsla(), c.text_dim.hsla(), c.window_bg.hsla(), c.banner.hsla(), c.border.hsla(), c.focus_ring.hsla());
    let eid = view.entity_id();
    let (can_back, can_forward, loading) = (v.model.can_back, v.model.can_forward, v.model.loading());
    let progress = match v.model.state {
        LoadState::Loading { progress } => Some(progress),
        _ => None,
    };
    let state = v.model.state.clone();
    let shown_address = v.model.shown_address().to_string();
    let (focus, address_focus, editing) = (v.focus.clone(), v.address_focus.clone(), v.editing);
    let address = v.address.clone();
    let notice = v.notice.as_ref().filter(|(_, at)| now.saturating_duration_since(*at) < crate::web::NOTICE_FOR).map(|(t, _)| t.clone());
    let shows_page = v.shows_page();
    // The hairline moves while a load runs: one more frame each time.
    let pump = canvas(
        move |_bounds, window, _cx| {
            if more {
                window.request_animation_frame();
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_0();
    let button = |name: &'static str, glyph: &'static str, enabled: bool| {
        div()
            .id((name, eid))
            .flex_none()
            .w(px(BUTTON_W))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .text_color(if enabled { text } else { dim.opacity(0.5) })
            .when(enabled, |d| d.cursor_pointer().hover(move |s| s.bg(border)))
            .child(glyph)
    };
    let target = |f: fn(&mut WebView, &mut Context<WebView>)| {
        let weak = view.downgrade();
        cx.listener(move |_root, _ev: &ClickEvent, _window, cx| {
            cx.stop_propagation();
            let _ = weak.update(cx, |v, cx| f(v, cx));
        })
    };
    let back = button("web-back", "\u{2039}", can_back).on_click(target(|v, cx| v.back(cx)));
    let forward = button("web-forward", "\u{203a}", can_forward).on_click(target(|v, cx| v.forward(cx)));
    let reload = if loading {
        button("web-stop", "\u{00d7}", true).on_click(target(|v, cx| v.stop(cx)))
    } else {
        button("web-reload", "\u{21bb}", true).on_click(target(|v, cx| v.reload(cx)))
    };
    let outside = button("web-outside", "\u{2197}", !shown_address.is_empty()).on_click(target(|v, cx| v.open_outside(cx)));
    let field_weak = view.downgrade();
    let field = div()
        .id(("web-address", eid))
        .key_context("WebAddress")
        .track_focus(&address_focus)
        .flex_1()
        .min_w_0()
        .h(px(20.0))
        .px(px(8.0))
        .flex()
        .items_center()
        .rounded(px(5.0))
        .bg(bg)
        .border_1()
        .border_color(if editing { accent } else { border })
        .overflow_hidden()
        .on_mouse_down(MouseButton::Left, {
            let weak = view.downgrade();
            let address_focus = address_focus.clone();
            cx.listener(move |root, _ev: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                root.focus_pane(pane, cx);
                root.keys_back_to_arkos();
                window.focus(&address_focus);
                let _ = weak.update(cx, |v, cx| v.begin_edit(cx));
            })
        })
        .on_click({
            let weak = view.downgrade();
            cx.listener(move |_root, ev: &ClickEvent, _window, cx| {
                // A double click selects the whole address, as in a browser.
                if let ClickEvent::Mouse(m) = ev {
                    if m.up.click_count >= 2 {
                        let _ = weak.update(cx, |v, cx| v.select_address(cx));
                    }
                }
            })
        })
        .on_key_down(cx.listener(move |root, ev: &KeyDownEvent, window, cx| {
            cx.stop_propagation();
            let done = field_weak.update(cx, |v, cx| v.address_key(&ev.keystroke, cx)).unwrap_or(false);
            if done {
                // The pane has the keys again: a Browser-context key works at once.
                if let Some(f) = root.webs.get(&tab).map(|v| v.read(cx).focus.clone()) {
                    window.focus(&f);
                }
            }
        }))
        .child(crate::line_edit::render(&address, "an address, like localhost:4173 or example.com", theme, family.clone(), 12.0));
    let hairline = progress.map(|p| {
        div().absolute().left_0().bottom_0().h(px(PROGRESS_H)).bg(accent).w_full().child(
            canvas(
                move |bounds, _window, _cx| progress_width(p, f32::from(bounds.size.width)),
                move |bounds, w, window, _cx| {
                    let mut b = bounds;
                    b.size.width = px(w);
                    window.paint_quad(gpui::fill(b, accent));
                },
            )
            .size_full(),
        )
    });
    let bar = div()
        .flex_none()
        .h(px(BAR_H))
        .w_full()
        .relative()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .px(px(4.0))
        .bg(banner)
        .border_b_1()
        .border_color(border)
        .font_family(family.clone())
        .text_size(px(12.0))
        .text_color(text)
        .child(back)
        .child(forward)
        .child(reload)
        .child(field)
        .child(outside)
        .children(hairline);
    // The web view is placed over this rectangle every frame it is drawn (`Root::place_web`).
    let root_weak = cx.weak_entity();
    let report = canvas(
        move |bounds, _window, cx| {
            let _ = root_weak.update(cx, |root, cx| root.place_web(tab, bounds, cx));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    let screen = match (&state, shows_page) {
        (_, true) => None,
        (LoadState::Failed { reason }, _) => Some(
            column(dim, family.clone())
                .child(div().text_color(text).text_size(px(15.0)).child("Could not load"))
                .child(div().max_w(px(520.0)).text_center().child(shown_address.clone()))
                .child(div().max_w(px(520.0)).text_center().child(reason.clone()))
                .child(
                    div()
                        .id(("web-retry", eid))
                        .px(px(12.0))
                        .py(px(5.0))
                        .rounded(px(6.0))
                        .bg(banner)
                        .border_1()
                        .border_color(border)
                        .text_color(text)
                        .cursor_pointer()
                        .hover(move |s| s.border_color(text))
                        .child("Retry")
                        .on_click(target(|v, cx| v.reload(cx))),
                ),
        ),
        (LoadState::Blank, _) => Some(column(dim, family.clone()).child("Type an address, or `arkos open page.html` from a session")),
        (LoadState::Loading { .. } | LoadState::Ready, _) => Some(column(dim, family.clone()).child("loading")),
    };
    let content = div()
        .flex_1()
        .min_h_0()
        .relative()
        .bg(bg)
        .child(report)
        .children(screen)
        .children(notice.map(|n| {
            div()
                .absolute()
                .top(px(8.0))
                .left_0()
                .w_full()
                .flex()
                .justify_center()
                .child(div().px(px(10.0)).py(px(5.0)).rounded(px(6.0)).bg(banner).border_1().border_color(border).text_color(text).text_size(px(12.0)).child(n))
        }));
    div()
        .size_full()
        .flex()
        .flex_col()
        .key_context("Browser")
        .track_focus(&focus)
        .bg(bg)
        .child(pump)
        .child(bar)
        .child(content)
}

fn column(dim: gpui::Hsla, family: SharedString) -> Div {
    div().size_full().flex().flex_col().items_center().justify_center().gap(px(10.0)).px(px(24.0)).font_family(family).text_size(px(13.0)).text_color(dim)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hairline_is_the_loads_share_of_the_bar_and_never_past_it() {
        assert_eq!(progress_width(0.0, 400.0), 0.0);
        assert_eq!(progress_width(0.25, 400.0), 100.0);
        assert_eq!(progress_width(1.0, 400.0), 400.0);
        assert_eq!(progress_width(1.7, 400.0), 400.0);
        assert_eq!(progress_width(-0.3, 400.0), 0.0);
        assert_eq!(progress_width(f64::NAN, 400.0), 0.0);
        assert_eq!(progress_width(0.5, 0.0), 0.0);
    }
}
