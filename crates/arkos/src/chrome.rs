//! The window chrome (spec 4, revised 2026-10-14 for a small display): one bar in the native
//! title bar's space, drawn by the window over a transparent title bar. On the left, "Arkos IDE"
//! and the project path; on the right, two icons (a bell for news, a doctor for fixes), each
//! with a dot and a popover. The old top bar and status bar are gone, so every row of height
//! goes to the panes.

use crate::effects::active::Style;
use crate::effects::manifest::MenuLook;
use crate::palette::Rgb;
use crate::theme::Theme;
use gpui::{canvas, div, point, prelude::*, px, App, ClickEvent, Div, FontWeight, MouseButton, MouseDownEvent, PathBuilder, Pixels, Point, SharedString, Window};
use std::path::Path;

/// The native macOS title bar's height; the bar fills exactly that space.
pub const TITLE_BAR_H: f32 = 28.0;
/// Room for the traffic lights, which macOS keeps drawing where it always does.
const TRAFFIC_LIGHTS_W: f32 = 80.0;
/// The system UI font, by gpui's name for it: the bar looks like the title bar it replaces.
pub const UI_FONT: &str = ".SystemUIFont";

/// Width of one icon's click target, and the space between the two.
pub const ICON_TARGET: f32 = 24.0;
pub const ICON_GAP: f32 = 8.0;
/// Space between the last icon and the window's right edge.
pub const BAR_RIGHT_PAD: f32 = 12.0;
/// The icon is drawn in a box this wide and high, centred in its target.
const ICON_SIZE: f32 = 16.0;
const ICON_STROKE: f32 = 1.5;
/// An icon's dot: this wide, its top right this far in from the target's.
const DOT: f32 = 6.0;
const DOT_INSET: f32 = 3.0;
/// The dot breathes with the focused pane's glow: at the peak a halo of the focus colour this
/// wide and this opaque spreads around it, and the dot lightens a little (on a light bar it
/// darkens; the glow's full 0.2 turns a small dot muddy there, so the halo carries the breath).
const DOT_HALO: f32 = 14.0;
const DOT_HALO_ALPHA: f32 = 0.45;
const DOT_BREATH_LIGHT: f32 = 0.08;

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
        // A wrench on the diagonal: an open jaw at the top right, a rounded handle end at the
        // bottom left. The head's arcs are two cubics each (centre 10.3, 5.3; radius 3.36).
        Icon::Doctor => vec![
            Move((13.4, 4.1)),
            Cubic((12.63, 7.72), (13.88, 5.36), (13.6, 6.78)),
            Cubic((9.0, 8.4), (11.66, 8.66), (10.24, 8.93)),
            Line((4.2, 13.2)),
            Cubic((2.38, 13.22), (3.69, 13.71), (2.89, 13.73)),
            Cubic((2.4, 11.4), (1.87, 12.71), (1.89, 11.91)),
            Line((7.2, 6.6)),
            Cubic((7.92, 2.92), (6.67, 5.36), (6.97, 3.87)),
            Cubic((11.6, 2.2), (8.87, 1.97), (10.36, 1.67)),
            Line((9.8, 4.0)),
            Line((10.6, 5.4)),
            Line((12.0, 6.2)),
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
/// open, which icon the pointer is over (`Closed` for none), and the focus glow's breath
/// (`pulse::glow`, 0 at rest) the dots breathe with.
pub struct Right {
    pub bell_dot: bool,
    pub doctor_dot: bool,
    pub open: Popover,
    pub hover: Popover,
    pub breath: f32,
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

/// The dot's halo at `breath` (0 at rest, 1 at the peak): its width, from the dot's own to
/// `DOT_HALO`, and its opacity, from nothing to `DOT_HALO_ALPHA`.
pub fn dot_halo(breath: f32) -> (f32, f32) {
    let b = breath.clamp(0.0, 1.0);
    (DOT + (DOT_HALO - DOT) * b, DOT_HALO_ALPHA * b)
}

/// One icon: a square target holding the 16 px drawing, lit while hovered or open, with a dot
/// at its top right when there is news. The dot breathes as the focused pane's glow does: a
/// halo swells around it and it lightens towards the peak, away from the bar's colour.
fn icon_button(id: &'static str, icon: Icon, dot: bool, breath: f32, lit: bool, c: &crate::theme::Chrome, (on_click, on_hover): (IconClick, IconHover)) -> gpui::Stateful<Div> {
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
        .when(dot, |d| {
            let (halo, halo_alpha) = dot_halo(breath);
            // The halo shares the dot's centre, so it reaches past the dot on every side.
            let halo_inset = DOT_INSET + (DOT - halo) / 2.0;
            let mut fill = c.focus_ring.hsla();
            let lift = DOT_BREATH_LIGHT * breath.clamp(0.0, 1.0);
            fill.l = if c.title_bar.hsla().l > 0.5 { (fill.l - lift).max(0.0) } else { (fill.l + lift).min(1.0) };
            d.when(halo_alpha > 0.0, |d| d.child(div().absolute().top(px(halo_inset)).right(px(halo_inset)).size(px(halo)).rounded_full().bg(c.focus_ring.alpha(halo_alpha))))
                .child(div().absolute().top(px(DOT_INSET)).right(px(DOT_INSET)).size(px(DOT)).rounded_full().bg(fill))
        })
}

/// `bar` made to act as the native title bar did: a left press drags the window and a double
/// click zooms it.
fn drag_to_move(bar: Div) -> Div {
    bar.on_mouse_down(MouseButton::Left, |ev: &MouseDownEvent, window, _cx| {
        if ev.click_count == 2 {
            window.titlebar_double_click();
        } else {
            window.start_window_move();
        }
    })
}

/// The title bar of the About, Settings and Keybindings windows: the project window's bar with
/// the window's title in place of the project path and no icons, so every window the menu opens
/// looks like the IDE.
pub fn aux_title_bar(title: &str, theme: &Theme) -> Div {
    let c = &theme.chrome;
    drag_to_move(div())
        .h(px(TITLE_BAR_H))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .pl(px(TRAFFIC_LIGHTS_W))
        .pr(px(BAR_RIGHT_PAD))
        .bg(c.title_bar.hsla())
        .text_color(c.text.hsla())
        .font_family(SharedString::from(UI_FONT))
        .text_size(px(13.0))
        .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::SEMIBOLD).child(title.to_string()))
}

/// The bar. Dragging it moves the window and a double click zooms it, as the native bar did.
/// In full screen the traffic lights are gone, so the text starts at the edge.
/// `bar_font` is the theme's ui font when it names one this Mac has, else the system's; `style`
/// is the window's effect's.
pub fn title_bar(project: &Path, right: Right, fullscreen: bool, theme: &Theme, style: Style, bar_font: SharedString, on_bell: (IconClick, IconHover), on_doctor: (IconClick, IconHover)) -> Div {
    let c = &theme.chrome;
    drag_to_move(div())
        .h(px(TITLE_BAR_H))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .pl(px(if fullscreen { 12.0 } else { TRAFFIC_LIGHTS_W }))
        .pr(px(BAR_RIGHT_PAD))
        .gap(px(10.0))
        .bg(c.title_bar.alpha(style.chrome_alpha))
        .text_color(c.text.hsla())
        .font_family(bar_font)
        .text_size(px(style.ui(13.0)))
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
                .child(icon_button("bell", Icon::Bell, right.bell_dot, right.breath, right.open == Popover::Bell || right.hover == Popover::Bell, c, on_bell))
                .child(icon_button("doctor", Icon::Doctor, right.doctor_dot, right.breath, right.open == Popover::Doctor || right.hover == Popover::Doctor, c, on_doctor)),
        )
}

/// The card a popover's rows go in, styled like the context menu and hung under its icon.
/// Only a live (open, not fading) card occludes and takes presses.
/// A menu's colours: its card, its line, its hovered (or current) row, and the halo around it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MenuColors {
    pub bg: gpui::Hsla,
    pub border: gpui::Hsla,
    pub hover: gpui::Hsla,
    pub glow: Option<gpui::Hsla>,
}

/// The colours of a menu or popover in `look` (an effect's `menus`), from `theme`.
pub fn menu_colors(theme: &Theme, look: MenuLook) -> MenuColors {
    let c = &theme.chrome;
    match look {
        MenuLook::App => MenuColors { bg: c.banner.hsla(), border: c.border.hsla(), hover: c.tab_active.hsla(), glow: None },
        MenuLook::Ink => MenuColors { bg: c.window_bg.hsla(), border: c.text.alpha(0.55), hover: ink_hover(c.window_bg, c.focus_ring).hsla(), glow: None },
        MenuLook::Glow => MenuColors {
            bg: c.window_bg.hsla(),
            border: c.focus_ring.alpha(0.85),
            hover: c.focus_ring.alpha(0.22),
            glow: Some(c.focus_ring.alpha(0.45)),
        },
    }
}

/// The paper under a hovered row as watercolor paints a highlight: `paper` washed with `accent`
/// thinned as a pigment (lifted 40% of the way to white, then pushed from its own grey, the wash
/// shader's `bright`), at a hovered row's strength of 0.3.
pub fn ink_hover(paper: Rgb, accent: Rgb) -> Rgb {
    let f = |v: u8| v as f32 / 255.0;
    let lifted = [f(accent.r), f(accent.g), f(accent.b)].map(|v| 1.0 - (1.0 - v) * 0.6);
    let grey = lifted[0] * 0.299 + lifted[1] * 0.587 + lifted[2] * 0.114;
    let bright = lifted.map(|v| (grey + (v - grey) * 1.55).clamp(0.0, 1.0));
    let wash = |p: u8, b: f32| ((f(p) * (1.0 + (b - 1.0) * 0.3)).clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgb { r: wash(paper.r, bright[0]), g: wash(paper.g, bright[1]), b: wash(paper.b, bright[2]) }
}

/// A menu's line and halo: its own, or none when the effect draws its edges (`effect_edges`),
/// keeping the line's width so nothing inside moves.
pub fn menu_line(m: MenuColors, effect_edges: bool) -> (gpui::Hsla, Option<gpui::Hsla>) {
    if effect_edges {
        (gpui::transparent_black(), None)
    } else {
        (m.border, m.glow)
    }
}

/// A menu's own fill: its card's colour, or nothing when the effect paints its backdrop
/// (`effect_menus`), with paper over it (`menu_paper`) as a pane has.
pub fn menu_fill(m: MenuColors, effect_menus: bool) -> gpui::Hsla {
    if effect_menus {
        gpui::transparent_black()
    } else {
        m.bg
    }
}

/// The paper laid over an effect's backdrop in a menu: the window's colour at the effect's
/// `paper_alpha`, as a terminal's or a document's paper lies over the wash.
pub fn menu_paper(theme: &Theme, paper_alpha: f32) -> gpui::Hsla {
    theme.chrome.window_bg.alpha(paper_alpha)
}

/// A menu or popover's card under `style`: its fill, and its line and halo unless the effect
/// draws its edges.
pub fn menu_frame<E: Styled + gpui::prelude::FluentBuilder>(d: E, theme: &Theme, style: &Style) -> E {
    let m = menu_colors(theme, style.menus);
    let (border, glow) = menu_line(m, style.menu_edges);
    d.bg(menu_fill(m, style.menu_edges)).border_1().border_color(border).when_some(glow, |d, glow| {
        d.shadow(vec![gpui::BoxShadow { color: glow, offset: point(px(0.0), px(0.0)), blur_radius: px(10.0), spread_radius: px(0.0) }])
    })
}

/// A live menu row's hover: the app's fill when it paints the menus itself, or, when the effect
/// paints them (`effect`), the row tells the effect it is under the pointer and reports a stroke
/// while it is (`hovered`), which the effect fades into the menu's backdrop as it does an
/// Explorer row's.
pub fn menu_row_hover(row: gpui::Stateful<Div>, key: u64, hovered: bool, effect: bool, hover: gpui::Hsla, cx: &mut gpui::Context<crate::Root>) -> gpui::Stateful<Div> {
    use crate::effects::blocks::{hover_after, HighlightState, Kind, Mark};
    if !effect {
        return row.hover(move |s| s.bg(hover));
    }
    let (w_hover, w_mark) = (cx.weak_entity(), cx.weak_entity());
    row.on_hover(move |over: &bool, _window, cx| {
        let _ = w_hover.update(cx, |root, cx| {
            root.effect_hover = hover_after(root.effect_hover, key, *over);
            cx.notify();
        });
    })
    .when(hovered, |d| {
        d.relative().child(
            canvas(
                move |b, _, cx| {
                    let rect = crate::tree::Rect { x: f32::from(b.origin.x), y: f32::from(b.origin.y), w: f32::from(b.size.width), h: f32::from(b.size.height) };
                    let _ = w_mark.update(cx, |root, _| root.effect_marks.push(Mark { key, rect, kind: Kind::RowHighlight, state: HighlightState::Hovered }));
                },
                |_, _, _, _| (),
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
    })
}

/// What a menu adds to itself, first among its children, when the effect paints the menus: a
/// canvas under its rows that reports where the menu is and how far it has faded (for the menu
/// stage's edges), and paints the effect's backdrop for it with the paper over it.
pub fn menu_report(key: u64, opacity: f32, cx: &mut gpui::Context<crate::Root>) -> impl IntoElement {
    let (root, painter) = (cx.weak_entity(), cx.weak_entity());
    canvas(
        move |b, _, cx| {
            let rect = crate::tree::Rect { x: f32::from(b.origin.x), y: f32::from(b.origin.y), w: f32::from(b.size.width), h: f32::from(b.size.height) };
            let _ = root.update(cx, |root, _| root.effect_menus.push(crate::effects::blocks::MenuShape { key, rect, opacity }));
            rect
        },
        move |b, rect, window, cx| {
            let painted = painter.update(cx, |root, _| {
                let paper = menu_paper(&root.theme, root.effect_style().paper_alpha);
                (root.menu_backdrop(key, rect, opacity), paper)
            });
            if let Ok((backdrop, paper)) = painted {
                if let Some(image) = backdrop {
                    window.paint_surface(b, image);
                }
                window.paint_quad(gpui::fill(b, paper.opacity(opacity)));
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

pub fn popover_card(which: Popover, window_w: f32, opacity: f32, live: bool, theme: &Theme, style: Style, family: SharedString) -> Div {
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
        // The effect's edges are straight, so a card it draws has square corners.
        .when(!style.menu_edges, |d| d.rounded(px(6.0)))
        .map(|d| menu_frame(d, theme, &style))
        .font_family(family)
        .text_size(px(style.ui(12.0)))
        .text_color(c.text.hsla())
        .when(live, |m| m.occlude().on_any_mouse_down(|_: &MouseDownEvent, _window, cx| cx.stop_propagation()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_s_menus_look_as_ever_and_an_effect_s_take_its_paper_ink_or_glow() {
        use crate::effects::manifest::MenuLook;
        let peach = crate::theme::builtin("peach-light").unwrap();
        let navy = crate::theme::builtin("navy").unwrap();
        let app = menu_colors(&peach, MenuLook::App);
        let c = &peach.chrome;
        assert_eq!(app, MenuColors { bg: c.banner.hsla(), border: c.border.hsla(), hover: c.tab_active.hsla(), glow: None });

        // Ink: the window's paper, an ink line in the text's colour, the hovered row tinted with
        // the thinned pigment watercolor paints its highlights with, and the text still reads.
        let ink = menu_colors(&peach, MenuLook::Ink);
        assert_eq!(ink.bg, c.window_bg.hsla());
        assert_eq!(ink.border, c.text.alpha(0.55));
        assert_eq!(ink.glow, None);
        let hover = ink_hover(c.window_bg, c.focus_ring);
        assert_eq!(ink.hover, hover.hsla());
        assert_ne!(hover, c.window_bg, "the hovered row shows");
        assert!(crate::palette::contrast(c.text, hover) >= 4.5, "{}", crate::palette::contrast(c.text, hover));

        // Glow: the window's own ground, the focus ring's blue as the line and a soft halo, and
        // the hovered row lit in the same blue.
        let glow = menu_colors(&navy, MenuLook::Glow);
        let n = &navy.chrome;
        assert_eq!(glow.bg, n.window_bg.hsla());
        assert_eq!(glow.border, n.focus_ring.alpha(0.85));
        assert_eq!(glow.glow, Some(n.focus_ring.alpha(0.45)));
        assert_eq!(glow.hover, n.focus_ring.alpha(0.22));
    }

    #[test]
    fn a_menu_leaves_its_line_and_glow_to_an_effect_that_draws_its_edges() {
        let navy = crate::theme::builtin("navy").unwrap();
        let glow = menu_colors(&navy, MenuLook::Glow);
        assert_eq!(menu_line(glow, false), (glow.border, glow.glow));
        let (border, halo) = menu_line(glow, true);
        assert_eq!(border.a, 0.0, "the line is the effect's");
        assert_eq!(halo, None, "so is the glow");
        // The card's own fill gives way to the effect's backdrop, with paper over it as a pane has.
        assert_eq!(menu_fill(glow, false), glow.bg);
        assert_eq!(menu_fill(glow, true).a, 0.0);
        assert_eq!(menu_paper(&navy, 0.8), navy.chrome.window_bg.alpha(0.8));
    }

    #[test]
    fn opening_one_popover_closes_the_other() {
        assert_eq!(Popover::Closed.toggled(Popover::Bell), Popover::Bell);
        assert_eq!(Popover::Bell.toggled(Popover::Doctor), Popover::Doctor);
        assert_eq!(Popover::Doctor.toggled(Popover::Doctor), Popover::Closed);
    }

    #[test]
    fn the_dot_breathes_from_bare_to_a_full_halo() {
        assert_eq!(dot_halo(0.0), (DOT, 0.0), "at rest, or under Reduce Motion, the dot alone");
        assert_eq!(dot_halo(1.0), (DOT_HALO, DOT_HALO_ALPHA));
        let (w, a) = dot_halo(0.5);
        assert!(w > DOT && w < DOT_HALO && a > 0.0 && a < DOT_HALO_ALPHA);
        assert_eq!(dot_halo(2.0), dot_halo(1.0), "clamped");
        assert!(DOT_INSET + (DOT - DOT_HALO) / 2.0 >= -1.0, "the halo stays about the target's corner");
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
