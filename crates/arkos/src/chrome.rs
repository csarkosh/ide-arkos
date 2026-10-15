//! The window chrome (spec 4, revised 2026-10-14 for a small display): one bar in the native
//! title bar's space, drawn by the window over a transparent title bar. On the left, "Arkos IDE"
//! and the project path; on the right, two icons (a bell for news, a doctor for fixes), each
//! with a dot and a popover. The old top bar and status bar are gone, so every row of height
//! goes to the panes.

use crate::theme::Theme;
use gpui::{canvas, div, point, prelude::*, px, App, ClickEvent, Div, FontWeight, MouseButton, MouseDownEvent, PathBuilder, Pixels, Point, SharedString, Window};
use std::path::Path;

/// The native macOS title bar's height; the bar fills exactly that space.
pub const TITLE_BAR_H: f32 = 28.0;
/// Room for the traffic lights, which macOS keeps drawing where it always does.
const TRAFFIC_LIGHTS_W: f32 = 80.0;
/// The system UI font, by gpui's name for it: the bar looks like the title bar it replaces.
const UI_FONT: &str = ".SystemUIFont";

/// Width of one icon's click target, and the space between the two.
pub const ICON_TARGET: f32 = 24.0;
pub const ICON_GAP: f32 = 8.0;
/// Space between the last icon and the window's right edge.
pub const BAR_RIGHT_PAD: f32 = 12.0;
/// The icon is drawn in a box this wide and high, centred in its target.
const ICON_SIZE: f32 = 16.0;
const ICON_STROKE: f32 = 1.5;

/// Which popover hangs under the bar. Only one is open at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Popover {
    #[default]
    Closed,
    Bell,
    Doctor,
}

impl Popover {
    /// What a click on `which`'s icon leaves open: the same one closes, another one opens.
    pub fn toggled(self, which: Popover) -> Popover {
        if self == which {
            Popover::Closed
        } else {
            which
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Bell,
    Doctor,
}

type P = (f32, f32);

/// One step of an icon's outline, in a 16 px box with y pointing down. `Cubic` is the end
/// point first, then the two control points, as `PathBuilder::cubic_bezier_to` takes them.
#[derive(Clone, Copy)]
enum Seg {
    Move(P),
    Line(P),
    Cubic(P, P, P),
    Close,
}

fn icon_segs(icon: Icon) -> Vec<Seg> {
    use Seg::*;
    match icon {
        Icon::Bell => vec![
            Move((4.0, 5.33)),
            Cubic((8.0, 1.33), (4.0, 3.12), (5.79, 1.33)),
            Cubic((12.0, 5.33), (10.21, 1.33), (12.0, 3.12)),
            Cubic((14.0, 11.33), (12.0, 10.0), (14.0, 11.33)),
            Line((2.0, 11.33)),
            Cubic((4.0, 5.33), (2.0, 11.33), (4.0, 10.0)),
            Close,
            Move((6.87, 14.0)),
            Cubic((9.13, 14.0), (7.2, 14.8), (8.8, 14.8)),
        ],
        // A stethoscope: two ear tubes meeting in a U, one tube on to a round chest piece at
        // the lower right.
        Icon::Doctor => vec![
            Move((3.0, 1.2)),
            Line((4.0, 2.0)),
            Cubic((7.0, 9.0), (4.0, 6.0), (5.0, 9.0)),
            Move((11.0, 1.2)),
            Line((10.0, 2.0)),
            Cubic((7.0, 9.0), (10.0, 6.0), (9.0, 9.0)),
            Move((7.0, 9.0)),
            Cubic((10.6, 11.1), (7.0, 10.8), (9.0, 11.4)),
            Move((12.0, 10.5)),
            Cubic((14.0, 12.5), (13.1, 10.5), (14.0, 11.4)),
            Cubic((12.0, 14.5), (14.0, 13.6), (13.1, 14.5)),
            Cubic((10.0, 12.5), (10.9, 14.5), (10.0, 13.6)),
            Cubic((12.0, 10.5), (10.0, 11.4), (10.9, 10.5)),
            Close,
        ],
    }
}

/// Every anchor and control point of the icon, in the 16 px box.
#[cfg_attr(not(test), allow(dead_code))] // The icon test reads it; the path is built from the same table.
pub fn icon_points(icon: Icon) -> Vec<(f32, f32)> {
    let mut v = Vec::new();
    for seg in icon_segs(icon) {
        match seg {
            Seg::Move(p) | Seg::Line(p) => v.push(p),
            Seg::Cubic(to, a, b) => v.extend([to, a, b]),
            Seg::Close => {}
        }
    }
    v
}

/// The icon as a stroked path whose 16 px box starts at `origin`.
pub fn icon_path(icon: Icon, origin: Point<Pixels>, stroke: Pixels) -> gpui::Path<Pixels> {
    let at = |(x, y): P| point(origin.x + px(x), origin.y + px(y));
    let mut b = PathBuilder::stroke(stroke);
    for seg in icon_segs(icon) {
        match seg {
            Seg::Move(p) => b.move_to(at(p)),
            Seg::Line(p) => b.line_to(at(p)),
            Seg::Cubic(to, a, c) => b.cubic_bezier_to(at(to), at(a), at(c)),
            Seg::Close => b.close(),
        }
    }
    b.build().expect("a fixed path builds")
}

/// The right side of the bar: a dot per icon when there is something to see, which popover is
/// open, and which icon the pointer is over (`Closed` for none).
pub struct Right {
    pub bell_dot: bool,
    pub doctor_dot: bool,
    pub open: Popover,
    pub hover: Popover,
}

pub type IconClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
pub type IconHover = Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>;

/// How far a popover's right edge sits from the window's: under its own icon.
pub fn popover_right_inset(which: Popover) -> f32 {
    match which {
        Popover::Bell => BAR_RIGHT_PAD + ICON_TARGET + ICON_GAP,
        _ => BAR_RIGHT_PAD,
    }
}

/// A popover's width, never more than the window less its right inset and a 12 px margin on the left.
pub fn popover_width(which: Popover, window_w: f32) -> f32 {
    let wanted: f32 = if which == Popover::Bell { 320.0 } else { 360.0 };
    wanted.min(window_w - popover_right_inset(which) - 12.0)
}

/// One icon: a square target holding the 16 px drawing, lit while hovered or open, with a dot
/// at its top right when there is news.
fn icon_button(id: &'static str, icon: Icon, dot: bool, lit: bool, c: &crate::theme::Chrome, (on_click, on_hover): (IconClick, IconHover)) -> gpui::Stateful<Div> {
    let color = if lit { c.text.hsla() } else { c.text_dim.hsla() };
    div()
        .id(id)
        .size(px(ICON_TARGET))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .relative()
        // The bar's window drag must not start from a press on an icon.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(on_click)
        .on_hover(on_hover)
        .child(
            canvas(|_, _, _| (), move |bounds, _, window, _| window.paint_path(icon_path(icon, bounds.origin, px(ICON_STROKE)), color))
                .size(px(ICON_SIZE)),
        )
        .when(dot, |d| d.child(div().absolute().top(px(3.0)).right(px(3.0)).size(px(6.0)).rounded_full().bg(c.focus_ring.hsla())))
}

/// The bar. Dragging it moves the window and a double click zooms it, as the native bar did.
/// In full screen the traffic lights are gone, so the text starts at the edge.
pub fn title_bar(project: &Path, right: Right, fullscreen: bool, theme: &Theme, on_bell: (IconClick, IconHover), on_doctor: (IconClick, IconHover)) -> Div {
    let c = &theme.chrome;
    div()
        .h(px(TITLE_BAR_H))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .pl(px(if fullscreen { 12.0 } else { TRAFFIC_LIGHTS_W }))
        .pr(px(BAR_RIGHT_PAD))
        .gap(px(10.0))
        .bg(c.title_bar.hsla())
        .text_color(c.text.hsla())
        .font_family(SharedString::from(UI_FONT))
        .text_size(px(13.0))
        .on_mouse_down(MouseButton::Left, |ev: &MouseDownEvent, window, _cx| {
            if ev.click_count == 2 {
                window.titlebar_double_click();
            } else {
                window.start_window_move();
            }
        })
        .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).child("Arkos IDE"))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .italic()
                .font_weight(FontWeight::LIGHT)
                .text_color(c.text_dim.hsla())
                .child(project.display().to_string()),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(ICON_GAP))
                .child(icon_button("bell", Icon::Bell, right.bell_dot, right.open == Popover::Bell || right.hover == Popover::Bell, c, on_bell))
                .child(icon_button("doctor", Icon::Doctor, right.doctor_dot, right.open == Popover::Doctor || right.hover == Popover::Doctor, c, on_doctor)),
        )
}

/// The card a popover's rows go in, styled like the context menu and hung under its icon.
/// Only a live (open, not fading) card occludes and takes presses.
pub fn popover_card(which: Popover, window_w: f32, opacity: f32, live: bool, theme: &Theme, family: SharedString) -> Div {
    let c = &theme.chrome;
    div()
        .absolute()
        .top(px(TITLE_BAR_H + 4.0) - px(crate::motion::lift(opacity, crate::motion::MENU_LIFT)))
        .right(px(popover_right_inset(which)))
        .w(px(popover_width(which, window_w)))
        .opacity(opacity)
        .flex()
        .flex_col()
        .py(px(4.0))
        .rounded(px(6.0))
        .bg(c.banner.hsla())
        .border_1()
        .border_color(c.border.hsla())
        .font_family(family)
        .text_size(px(12.0))
        .text_color(c.text.hsla())
        .when(live, |m| m.occlude().on_any_mouse_down(|_: &MouseDownEvent, _window, cx| cx.stop_propagation()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_one_popover_closes_the_other() {
        assert_eq!(Popover::Closed.toggled(Popover::Bell), Popover::Bell);
        assert_eq!(Popover::Bell.toggled(Popover::Doctor), Popover::Doctor);
        assert_eq!(Popover::Doctor.toggled(Popover::Doctor), Popover::Closed);
    }

    #[test]
    fn the_popovers_hang_under_their_icons_and_fit_the_window() {
        assert_eq!(popover_right_inset(Popover::Doctor), BAR_RIGHT_PAD, "the doctor is the last icon");
        assert_eq!(popover_right_inset(Popover::Bell), BAR_RIGHT_PAD + ICON_TARGET + ICON_GAP);
        assert_eq!(popover_width(Popover::Bell, 1200.0), 320.0);
        assert_eq!(popover_width(Popover::Doctor, 1200.0), 360.0);
        assert_eq!(popover_width(Popover::Doctor, 300.0), 276.0, "a narrow window");
        assert_eq!(popover_width(Popover::Bell, 300.0), 244.0, "the bell sits further left, so it has less room");
    }

    #[test]
    fn the_bell_and_the_doctor_icons_stay_in_a_16_px_box() {
        for icon in [Icon::Bell, Icon::Doctor] {
            let pts = icon_points(icon);
            assert!(pts.len() >= 6, "{icon:?} has a shape");
            for (x, y) in pts {
                assert!((0.75..=15.25).contains(&x) && (0.75..=15.25).contains(&y), "{icon:?} point {x},{y} leaves room for a 1.5 px stroke");
            }
        }
    }
}
