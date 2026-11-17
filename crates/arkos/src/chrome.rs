//! The window chrome (spec 4, revised 2026-10-14 for a small display): one bar in the native
//! title bar's space, drawn by the window over a transparent title bar. On the left, "Arkos IDE"
//! and the project path; on the right, two icons (a bell for news, a doctor for fixes), each
//! with a dot and a popover. The old top bar and status bar are gone, so every row of height
//! goes to the panes.

use crate::effects::active::Style;
use crate::effects::manifest::{KeysRing, MenuLook};
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
/// The least room in points between the title's words and its icons, besides the bar's gaps, so
/// a wash's labels behind the two groups (`Kind::TitleGroup`) stay apart.
pub const TITLE_GROUPS_GAP: f32 = 40.0;
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

/// Which popover hangs under the bar. Only one is open at a time. Each of the bar's icons is
/// named by one too, for the pointer and the keyboard's ring: `Settings` opens the Settings window
/// and never hangs a popover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Popover {
    #[default]
    Closed,
    Bell,
    Doctor,
    Help,
    Settings,
}

/// The bar's icons, left to right (Cyrus, 2026-11-17: "Notification, Doctor, Help, Settings").
pub const BAR_ICONS: [Popover; 4] = [Popover::Bell, Popover::Doctor, Popover::Help, Popover::Settings];

/// The rows of the Help popover: the menu bar's Help menu, in its order.
pub const HELP_ITEMS: [&str; 4] = ["Keyboard Navigation", "Keybindings", "Documentation", "What Arkos Sends"];

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
    Help,
    Settings,
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
        // A first-aid kit (Cyrus, 2026-11-17: "something indicating 'Medical' so the user gets
        // an impression that it fixes issues with the IDE"; a hospital before it did not read):
        // a case with rounded corners, its handle on top and a cross on its front.
        Icon::Doctor => vec![
            Move((3.5, 4.5)),
            Line((12.5, 4.5)),
            Cubic((14.5, 6.5), (13.6, 4.5), (14.5, 5.4)),
            Line((14.5, 12.0)),
            Cubic((12.5, 14.0), (14.5, 13.1), (13.6, 14.0)),
            Line((3.5, 14.0)),
            Cubic((1.5, 12.0), (2.4, 14.0), (1.5, 13.1)),
            Line((1.5, 6.5)),
            Cubic((3.5, 4.5), (1.5, 5.4), (2.4, 4.5)),
            Close,
            // The handle.
            Move((5.8, 4.5)),
            Line((5.8, 2.6)),
            Line((10.2, 2.6)),
            Line((10.2, 4.5)),
            // The cross.
            Move((8.0, 6.6)),
            Line((8.0, 11.9)),
            Move((5.35, 9.25)),
            Line((10.65, 9.25)),
        ],
        // A question mark in a ring.
        Icon::Help => {
            let mut v = ring_segs((8.0, 8.0), 6.5);
            v.extend([
                Move((5.9, 6.2)),
                Cubic((8.0, 4.2), (5.9, 5.0), (6.9, 4.2)),
                Cubic((10.1, 6.1), (9.2, 4.2), (10.1, 5.0)),
                Cubic((8.0, 8.4), (10.1, 7.3), (8.0, 7.4)),
                Line((8.0, 9.4)),
                Move((8.0, 11.3)),
                Line((8.0, 11.6)),
            ]);
            v
        }
        // A cog: eight teeth round a ring, a hole in its middle.
        Icon::Settings => {
            let (c, outer, inner) = ((8.0f32, 8.0f32), 6.6f32, 4.9f32);
            let at = |r: f32, deg: f32| {
                let a = deg.to_radians();
                (c.0 + r * a.cos(), c.1 + r * a.sin())
            };
            let mut v = Vec::new();
            for i in 0..8 {
                let a = i as f32 * 45.0;
                let pts = [at(inner, a - 14.0), at(outer, a - 9.0), at(outer, a + 9.0), at(inner, a + 14.0)];
                v.push(if i == 0 { Move(pts[0]) } else { Line(pts[0]) });
                v.extend(pts[1..].iter().map(|p| Line(*p)));
            }
            v.push(Close);
            v.extend(ring_segs(c, 2.2));
            v
        }
    }
}

/// A circle of radius `r` round `c` as four cubics.
fn ring_segs(c: P, r: f32) -> Vec<Seg> {
    use Seg::*;
    let k = 0.5523 * r;
    let (x, y) = c;
    vec![
        Move((x + r, y)),
        Cubic((x, y + r), (x + r, y + k), (x + k, y + r)),
        Cubic((x - r, y), (x - k, y + r), (x - r, y + k)),
        Cubic((x, y - r), (x - r, y - k), (x - k, y - r)),
        Cubic((x + r, y), (x + k, y - r), (x + r, y - k)),
        Close,
    ]
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
    /// The icon the keyboard's ring is on (Chrome keys spec, section 2), or `Closed`.
    pub ring: Popover,
}

/// What a key does while the title bar's icons have the keyboard: with no menu open, Left and
/// Right move the ring along the icons (`BAR_ICONS`), Enter or Space opens the ringed one's menu
/// (the cog, the Settings window), Down goes back to the Sessions/Files switcher, Escape back to the pane; with its menu
/// open, Up and Down move between the menu's buttons and Enter or Space presses one (Escape
/// closes the menu, as for every menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarKey {
    Ring(Popover),
    Open,
    Mark(isize),
    Press,
    Down,
    Leave,
    Pass,
}

pub fn bar_key(key: &str, menu_open: bool, ring: Popover) -> BarKey {
    let at = BAR_ICONS.iter().position(|p| *p == ring).unwrap_or(0);
    match (key, menu_open) {
        ("up", true) => BarKey::Mark(-1),
        ("down", true) => BarKey::Mark(1),
        ("enter" | "space", true) => BarKey::Press,
        ("left", false) => BarKey::Ring(BAR_ICONS[at.saturating_sub(1)]),
        ("right", false) => BarKey::Ring(BAR_ICONS[(at + 1).min(BAR_ICONS.len() - 1)]),
        ("enter" | "space", false) => BarKey::Open,
        ("down", false) => BarKey::Down,
        ("escape", false) => BarKey::Leave,
        _ => BarKey::Pass,
    }
}

/// What each icon's tooltip says: its menu's name and what is in it.
pub fn icon_tip(which: Popover) -> (&'static str, &'static str) {
    match which {
        Popover::Doctor => ("Doctor", "Checks what Arkos needs from Claude Code and macOS (smooth scrolling, its theme, the title hook, start at login), with a fix for each."),
        Popover::Help => ("Help", "Keyboard navigation, the keybindings, the documentation and what Arkos sends: the Help menu, here."),
        Popover::Settings => ("Settings", "Opens Settings: the theme and its effect, fonts, layouts, keybindings and updates."),
        _ => ("Notifications", "Updates, settings errors and what Arkos set up for you. A dot means something new, or an update waiting."),
    }
}

/// A tooltip card: a title and a line under it, a set width so a wrapped line stays in its box.
pub struct Tip {
    pub title: SharedString,
    pub text: SharedString,
    pub fg: gpui::Hsla,
    pub dim: gpui::Hsla,
    pub bg: gpui::Hsla,
    pub border: gpui::Hsla,
    pub family: SharedString,
    pub size: f32,
}

impl gpui::Render for Tip {
    fn render(&mut self, _window: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .w(px(240.0))
            .px(px(10.0))
            .py(px(6.0))
            .rounded(px(6.0))
            .bg(self.bg)
            .border_1()
            .border_color(self.border)
            .text_color(self.fg)
            .font_family(self.family.clone())
            .text_size(px(self.size))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(div().font_weight(FontWeight::BOLD).child(self.title.clone()))
            .child(div().text_color(self.dim).child(self.text.clone()))
    }
}

pub type IconClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
pub type IconHover = Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>;

/// How far a popover's right edge sits from the window's: under its own icon, counted from the
/// bar's right end (`BAR_ICONS`, the last at the end).
pub fn popover_right_inset(which: Popover) -> f32 {
    let at = BAR_ICONS.iter().position(|p| *p == which).unwrap_or(BAR_ICONS.len() - 1);
    BAR_RIGHT_PAD + (BAR_ICONS.len() - 1 - at) as f32 * (ICON_TARGET + ICON_GAP)
}

/// A popover's width, never more than the window less its right inset and a 12 px margin on the left.
pub fn popover_width(which: Popover, window_w: f32) -> f32 {
    let wanted: f32 = match which {
        Popover::Bell => 320.0,
        Popover::Help => 240.0,
        _ => 360.0,
    };
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
/// halo swells around it and it lightens towards the peak, away from the bar's colour. Its
/// tooltip shows only while its own menu is not open (Cyrus, 2026-11-17: no menu laid over
/// another).
#[allow(clippy::too_many_arguments)]
fn icon_button(id: &'static str, icon: Icon, dot: bool, breath: f32, lit: bool, open: bool, ringed: bool, wash: Option<Rgb>, tip: (&'static str, &'static str), c: &crate::theme::Chrome, family: SharedString, (on_click, on_hover): (IconClick, IconHover)) -> gpui::Stateful<Div> {
    let color = if lit || ringed { c.text.hsla() } else { c.text_dim.hsla() };
    let (fg, dim, bg, border) = (c.text.hsla(), c.text_dim.hsla(), c.banner.hsla(), c.border.hsla());
    div()
        .id(id)
        .size(px(ICON_TARGET))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .relative()
        // The keyboard's ring: always a border, clear unless ringed, so it never moves the icon.
        .rounded(px(6.0))
        .border_1()
        .border_color(if ringed && wash.is_none() { c.focus_ring.hsla() } else { gpui::transparent_black() })
        // Under watercolor the ring is its soft wash instead of the line.
        .when_some(wash.filter(|_| ringed), |d, p| d.child(wash_mark(p, 6.0)))
        .when(!open, |d| {
            d.tooltip(move |_window, cx| {
                let tip = Tip { title: tip.0.into(), text: tip.1.into(), fg, dim, bg, border, family: family.clone(), size: 12.0 };
                gpui::AnyView::from(cx.new(|_| tip))
            })
        })
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

/// The title bar's padding at its left and right ends: past the traffic lights (12 points from
/// the edge in full screen, where they are gone) and in from the right, each with the effect's
/// `title_pad` added.
pub fn bar_padding(fullscreen: bool, title_pad: f32) -> (f32, f32) {
    (if fullscreen { 12.0 } else { TRAFFIC_LIGHTS_W } + title_pad, BAR_RIGHT_PAD + title_pad)
}

/// The words the project window's bar shows around the project's name: the app's, then in a
/// sandbox instance the word `sandbox`, so the bar tells it from the real IDE.
pub fn title_words(sandbox: bool) -> Vec<&'static str> {
    if sandbox { vec!["Arkos IDE", "sandbox"] } else { vec!["Arkos IDE"] }
}

/// The bar. Dragging it moves the window and a double click zooms it, as the native bar did.
/// In full screen the traffic lights are gone, so the text starts at the edge.
/// `bar_font` is the theme's ui font when it names one this Mac has, else the system's; `style`
/// is the window's effect's.
/// `report`, while an effect is on, is the window to report the bar's two groups to (its words
/// and its icons, `Kind::TitleGroup`), so the wash can set a label behind each.
#[allow(clippy::too_many_arguments)]
pub fn title_bar(project: &Path, sandbox: bool, right: Right, fullscreen: bool, theme: &Theme, style: Style, bar_font: SharedString, on_bell: (IconClick, IconHover), on_doctor: (IconClick, IconHover), on_help: (IconClick, IconHover), on_settings: (IconClick, IconHover), report: Option<gpui::WeakEntity<crate::Root>>) -> Div {
    let c = &theme.chrome;
    let wash = keys_wash(theme, &style);
    let words = title_words(sandbox);
    let (left, right_pad) = bar_padding(fullscreen, style.title_pad);
    // A group reports where it is laid out from a canvas behind it.
    let group = |d: Div, key: u64| -> Div {
        let report = report.clone();
        d.relative().child(
            canvas(
                move |b, _, cx| {
                    if let Some(root) = &report {
                        let rect = crate::tree::Rect { x: f32::from(b.origin.x), y: f32::from(b.origin.y), w: f32::from(b.size.width), h: f32::from(b.size.height) };
                        let _ = root.update(cx, |root, _| root.effect_shapes.push(crate::effects::blocks::Shape { key, rect, kind: crate::effects::blocks::Kind::TitleGroup, pitch: 0.0 }));
                    }
                },
                |_, _, _, _| (),
            )
            .absolute()
            .inset_0(),
        )
    };
    drag_to_move(div())
        // An effect may make the bar taller at its foot: what it holds stays centred in the bar's
        // own height, level with the traffic lights, with the added room under it.
        .h(px(TITLE_BAR_H + style.title_grow))
        .pb(px(style.title_grow))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .pl(px(left))
        .pr(px(right_pad))
        .gap(px(10.0))
        .bg(c.title_bar.alpha(style.chrome_alpha))
        .text_color(c.text.hsla())
        .font_family(bar_font.clone())
        .text_size(px(style.ui(13.0)))
        // The words take only their own width (the path truncating when the bar is short), and
        // an empty stretch of at least `TITLE_GROUPS_GAP` keeps them apart from the icons.
        .child(group(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .min_w_0()
                .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).child(words[0]))
                .child(div().min_w_0().truncate().italic().font_weight(FontWeight::LIGHT).text_color(c.text_dim.hsla()).child(project.display().to_string()))
                .children(words.get(1).map(|word| div().flex_none().text_color(c.text_dim.hsla()).child(*word))),
            crate::effects::blocks::TITLE_WORDS_KEY,
        ))
        .child(div().flex_1().min_w(px(TITLE_GROUPS_GAP)))
        .child(group(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(ICON_GAP))
                .child(icon_button("bell", Icon::Bell, right.bell_dot, right.breath, right.open == Popover::Bell || right.hover == Popover::Bell, right.open == Popover::Bell, right.ring == Popover::Bell, wash, icon_tip(Popover::Bell), c, bar_font.clone(), on_bell))
                .child(icon_button("doctor", Icon::Doctor, right.doctor_dot, right.breath, right.open == Popover::Doctor || right.hover == Popover::Doctor, right.open == Popover::Doctor, right.ring == Popover::Doctor, wash, icon_tip(Popover::Doctor), c, bar_font.clone(), on_doctor))
                .child(icon_button("help", Icon::Help, false, right.breath, right.open == Popover::Help || right.hover == Popover::Help, right.open == Popover::Help, right.ring == Popover::Help, wash, icon_tip(Popover::Help), c, bar_font.clone(), on_help))
                .child(icon_button("settings", Icon::Settings, false, right.breath, right.hover == Popover::Settings, false, right.ring == Popover::Settings, wash, icon_tip(Popover::Settings), c, bar_font.clone(), on_settings)),
            crate::effects::blocks::TITLE_ICONS_KEY,
        ))
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

/// How far a selected row's pill is inset from the sidebar's edges under an effect that draws
/// the rows as pills (the material wash's PILL_INSET).
pub const PILL_INSET: f32 = 8.0;
/// How strongly the keyboard's ring shows, as a quieter version of a selected row (Cyrus,
/// 2026-11-17: "too strong. It should look like the selected element's effect, but more
/// subdued"): the pill's fill of the selection colour and its line; runic's wash of the focus
/// colour, its line and its glow; overworld's fill of the active tab's colour and its line.
pub const KEYS_PILL_FILL: f32 = 0.28;
pub const KEYS_PILL_LINE: f32 = 0.35;
pub const KEYS_GLOW_FILL: f32 = 0.06;
pub const KEYS_GLOW_LINE: f32 = 0.4;
pub const KEYS_GLOW_HALO: f32 = 0.18;
pub const KEYS_PIXEL_FILL: f32 = 0.4;
pub const KEYS_PIXEL_LINE: f32 = 0.45;
/// Watercolor's wash: the first pigment laid at about half a selected row's strength, its edge
/// feathered by a soft rim of the same pigment rather than drawn as a line.
pub const KEYS_WASH_FILL: f32 = 0.16;
pub const KEYS_WASH_RIM: f32 = 0.22;

/// The pigment of watercolor's soft in-focus wash, when the effect in force draws one (`wash`):
/// every in-focus mark then takes it, the switcher, the title bar's icons, a popover's rows and
/// the filter field as well as the rows (Cyrus, 2026-11-17: watercolor's indicator was "too sharp
/// for the theme and missing a soft background color"). `None` under every other look.
pub fn keys_wash(theme: &Theme, style: &Style) -> Option<Rgb> {
    (style.keys_ring == KeysRing::Wash).then(|| crate::effects::inputs::pigments(theme).0[0])
}

/// Watercolor's in-focus wash laid over an element (`relative`) with corners `round`: the pigment
/// at a fainter strength than a selected row's, its edge feathered by a soft rim, no line.
pub fn wash_mark(pigment: Rgb, round: f32) -> Div {
    div()
        .absolute()
        .inset_0()
        .rounded(px(round))
        .bg(pigment.alpha(KEYS_WASH_FILL))
        .shadow(vec![gpui::BoxShadow { color: pigment.alpha(KEYS_WASH_RIM), offset: point(px(0.0), px(0.0)), blur_radius: px(4.0), spread_radius: px(0.0) }])
}

/// Whether the keyboard's ring in the sidebar is drawn as a shape of its own behind the row's
/// text (a pill, a glow, a pixel frame), not the thin outline round the row.
pub fn keys_ring_is_shaped(style: &Style) -> bool {
    style.keys_ring != KeysRing::Outline
}

/// The keyboard's ring on a sidebar row or heading, laid behind its text (the row's first child,
/// the row `relative`), matching how the effect draws a selected row (Cyrus, 2026-11-17: "the
/// outline to match the background shape and the background should be a fainter version of the
/// currently-selected element", for every effect, and subdued): material's pill inset as its
/// selected rows, filled with a fainter selection colour; runic's soft glowing line round the row
/// with a faint wash of the focus colour; overworld's square frame filled with a fainter active
/// tab's colour, as its selected rows are filled; watercolor's soft wash of its first pigment, a
/// feathered edge and no line; else the thin outline round the row.
pub fn keys_ring(theme: &Theme, style: &Style) -> Div {
    keys_ring_inset(theme, style, PILL_INSET)
}

/// `keys_ring`, its pill inset `pill_inset` from the element's sides (a tab's ring sits closer to
/// its edges than a sidebar row's).
pub fn keys_ring_inset(theme: &Theme, style: &Style, pill_inset: f32) -> Div {
    let ring = theme.chrome.focus_ring;
    let base = div().absolute();
    match style.keys_ring {
        KeysRing::Pill => base
            .top_0()
            .bottom_0()
            .left(px(pill_inset))
            .right(px(pill_inset))
            .rounded(px(999.0))
            .border_1()
            .border_color(ring.alpha(KEYS_PILL_LINE))
            .bg(theme.terminal.selection.alpha(KEYS_PILL_FILL)),
        KeysRing::Glow => base
            .inset_0()
            .rounded(px(2.0))
            .border_1()
            .border_color(ring.alpha(KEYS_GLOW_LINE))
            .bg(ring.alpha(KEYS_GLOW_FILL))
            .shadow(vec![gpui::BoxShadow { color: ring.alpha(KEYS_GLOW_HALO), offset: point(px(0.0), px(0.0)), blur_radius: px(5.0), spread_radius: px(0.0) }]),
        KeysRing::Pixel => base.inset_0().border_1().border_color(ring.alpha(KEYS_PIXEL_LINE)).bg(theme.chrome.tab_active.alpha(KEYS_PIXEL_FILL)),
        KeysRing::Wash => wash_mark(crate::effects::inputs::pigments(theme).0[0], 6.0),
        KeysRing::Outline => base.inset_0().rounded(px(4.0)).border_1().border_color(ring.hsla()),
    }
}

/// The paper laid over an effect's backdrop in a menu: the window's colour at the effect's
/// menu paper (`Style::menu_paper_alpha`), as a terminal's or a document's paper lies over the wash.
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
    menu_row_hover_then(row, key, hovered, effect, hover, cx, |_, _, _| {})
}

/// `menu_row_hover` for a row that also acts on the pointer coming and going itself (`then`, run
/// after the effect is told): gpui takes one hover listener an element, so the row's own goes
/// here, never in a second `on_hover`.
pub fn menu_row_hover_then(row: gpui::Stateful<Div>, key: u64, hovered: bool, effect: bool, hover: gpui::Hsla, cx: &mut gpui::Context<crate::Root>, then: impl Fn(&bool, &mut gpui::Window, &mut gpui::App) + 'static) -> gpui::Stateful<Div> {
    use crate::effects::blocks::{hover_after, HighlightState, Kind, Mark};
    if !effect {
        return row.hover(move |s| s.bg(hover)).on_hover(then);
    }
    let (w_hover, w_mark) = (cx.weak_entity(), cx.weak_entity());
    row.on_hover(move |over: &bool, window, cx| {
        let _ = w_hover.update(cx, |root, cx| {
            root.effect_hover = hover_after(root.effect_hover, key, *over);
            cx.notify();
        });
        then(over, window, cx);
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
                let paper = menu_paper(&root.theme, root.effect_style().menu_paper_alpha);
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
        .top(px(TITLE_BAR_H + style.title_grow + 4.0) - px(crate::motion::lift(opacity, crate::motion::MENU_LIFT)))
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
    fn the_title_bar_s_padding_grows_by_the_effect_s_title_pad_on_both_sides() {
        assert_eq!(bar_padding(false, 0.0), (TRAFFIC_LIGHTS_W, BAR_RIGHT_PAD));
        assert_eq!(bar_padding(false, 14.0), (TRAFFIC_LIGHTS_W + 14.0, BAR_RIGHT_PAD + 14.0));
        assert_eq!(bar_padding(true, 0.0), (12.0, BAR_RIGHT_PAD), "no traffic lights in full screen");
        assert_eq!(bar_padding(true, 14.0), (26.0, BAR_RIGHT_PAD + 14.0));
    }

    #[test]
    fn the_title_bar_adds_the_word_sandbox_only_in_a_sandbox() {
        assert_eq!(title_words(false), ["Arkos IDE"]);
        assert_eq!(title_words(true), ["Arkos IDE", "sandbox"]);
    }

    #[test]
    fn the_app_s_menus_look_as_ever_and_an_effect_s_take_its_paper_ink_or_glow() {
        use crate::effects::manifest::MenuLook;
        let peach = crate::theme::builtin("peach-watercolor").unwrap();
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
        // Left and Right walk the icons in order, and stop at either end.
        assert_eq!(BAR_ICONS, [Popover::Bell, Popover::Doctor, Popover::Help, Popover::Settings]);
        assert_eq!(bar_key("right", false, Popover::Bell), BarKey::Ring(Popover::Doctor));
        assert_eq!(bar_key("right", false, Popover::Doctor), BarKey::Ring(Popover::Help));
        assert_eq!(bar_key("right", false, Popover::Help), BarKey::Ring(Popover::Settings));
        assert_eq!(bar_key("right", false, Popover::Settings), BarKey::Ring(Popover::Settings));
        assert_eq!(bar_key("left", false, Popover::Help), BarKey::Ring(Popover::Doctor));
        assert_eq!(bar_key("left", false, Popover::Bell), BarKey::Ring(Popover::Bell));
        let k = |key| bar_key(key, false, Popover::Bell);
        assert_eq!(k("enter"), BarKey::Open);
        assert_eq!(k("space"), BarKey::Open);
        assert_eq!(k("down"), BarKey::Down, "back to the switcher");
        assert_eq!(k("escape"), BarKey::Leave);
        let open = |key| bar_key(key, true, Popover::Bell);
        assert_eq!(open("down"), BarKey::Mark(1), "a menu open: its buttons");
        assert_eq!(open("up"), BarKey::Mark(-1));
        assert_eq!(open("enter"), BarKey::Press);
        assert_eq!(open("left"), BarKey::Pass);
        assert!(icon_tip(Popover::Bell).1.contains("update"), "the bell's tooltip says what it holds");
        assert!(icon_tip(Popover::Doctor).1.contains("fix"));
        assert!(icon_tip(Popover::Help).1.contains("Help menu") && icon_tip(Popover::Settings).1.contains("Settings"));
        assert_eq!(HELP_ITEMS, ["Keyboard Navigation", "Keybindings", "Documentation", "What Arkos Sends"]);
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
        assert_eq!(popover_right_inset(Popover::Settings), BAR_RIGHT_PAD, "the cog is the last icon");
        assert_eq!(popover_right_inset(Popover::Bell), BAR_RIGHT_PAD + 3.0 * (ICON_TARGET + ICON_GAP));
        assert_eq!(popover_width(Popover::Bell, 1200.0), 320.0);
        assert_eq!(popover_width(Popover::Doctor, 1200.0), 360.0);
        let step = ICON_TARGET + ICON_GAP;
        assert_eq!(popover_right_inset(Popover::Bell), BAR_RIGHT_PAD + 3.0 * step, "the bell is the first of four icons");
        assert_eq!(popover_right_inset(Popover::Doctor), BAR_RIGHT_PAD + 2.0 * step);
        assert_eq!(popover_right_inset(Popover::Help), BAR_RIGHT_PAD + step);
        assert_eq!(popover_width(Popover::Doctor, 300.0), 300.0 - BAR_RIGHT_PAD - 2.0 * step - 12.0, "a narrow window");
        assert_eq!(popover_width(Popover::Help, 1200.0), 240.0);
    }

    #[test]
    fn the_bell_and_the_doctor_icons_stay_in_a_16_px_box() {
        for icon in [Icon::Bell, Icon::Doctor, Icon::Help, Icon::Settings] {
            let pts = icon_points(icon);
            assert!(pts.len() >= 6, "{icon:?} has a shape");
            for (x, y) in pts {
                assert!((0.75..=15.25).contains(&x) && (0.75..=15.25).contains(&y), "{icon:?} point {x},{y} leaves room for a 1.5 px stroke");
            }
        }
    }

    #[test]
    fn every_in_focus_mark_takes_watercolor_s_wash_and_only_watercolor_s() {
        let style = |name: &str| {
            let m = crate::effects::manifest::load(name, std::path::Path::new("/nonexistent")).unwrap().manifest;
            crate::effects::active::style_at(&m, 1.0)
        };
        let theme = crate::theme::builtin("lavender-watercolor").unwrap();
        assert_eq!(keys_wash(&theme, &style("watercolor")), Some(crate::effects::inputs::pigments(&theme).0[0]));
        for other in ["material", "runic", "overworld", "notebook"] {
            assert_eq!(keys_wash(&theme, &style(other)), None, "{other}");
        }
        assert_eq!(keys_wash(&theme, &Style::plain()), None);
    }

    #[test]
    fn watercolor_s_keyboard_ring_is_a_soft_wash_of_its_first_pigment_with_no_line() {
        // Cyrus, 2026-11-17: the ring was "too sharp for the theme and missing a soft background
        // color".
        let theme = crate::theme::builtin("lavender-watercolor").unwrap();
        let m = crate::effects::manifest::load("watercolor", std::path::Path::new("/nonexistent")).unwrap().manifest;
        let style = crate::effects::active::style_at(&m, 1.0);
        let mut ring = keys_ring(&theme, &style);
        let s = ring.style();
        let line = [s.border_widths.top, s.border_widths.right, s.border_widths.bottom, s.border_widths.left];
        assert!(line.iter().all(Option::is_none), "no line: {line:?}");
        let pigment = crate::effects::inputs::pigments(&theme).0[0];
        assert_eq!(s.background.clone(), Some(pigment.alpha(KEYS_WASH_FILL).into()), "a wash of the first pigment");
        let rim = s.box_shadow.clone().expect("a soft rim");
        assert!(rim.iter().all(|r| r.blur_radius > px(0.0) && r.color == pigment.alpha(KEYS_WASH_RIM)), "{rim:?}");
        assert!(KEYS_WASH_FILL < 0.3, "fainter than a selected row's wash (HIGHLIGHT_STRENGTH 0.3)");
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "watercolor").unwrap().wash;
        assert!(wash.contains("constant float HIGHLIGHT_STRENGTH[3] = {0.2, 0.3, 0.36};"));
    }

    #[test]
    fn the_keyboard_s_ring_takes_each_effect_s_own_shape() {
        let style = |name: &str| {
            let m = crate::effects::manifest::load(name, std::path::Path::new("/nonexistent")).unwrap().manifest;
            crate::effects::active::style_at(&m, 1.0)
        };
        assert_eq!(style("material").keys_ring, KeysRing::Pill, "material's rows are pills on rounded cards");
        assert_eq!(style("runic").keys_ring, KeysRing::Glow, "runic's rows are lit");
        assert_eq!(style("overworld").keys_ring, KeysRing::Pixel, "overworld's rows are pixel fills");
        assert_eq!(style("watercolor").keys_ring, KeysRing::Wash, "watercolor's rows are washes");
        assert!(keys_ring_is_shaped(&style("runic")) && !keys_ring_is_shaped(&Style::plain()), "no effect: the outline");
        // Faded out, the app's own outline.
        let m = crate::effects::manifest::load("material", std::path::Path::new("/nonexistent")).unwrap().manifest;
        assert_eq!(crate::effects::active::style_at(&m, 0.0).keys_ring, KeysRing::Outline);
        let wash = crate::effects::BUILTIN.iter().find(|b| b.name == "material").unwrap().wash;
        assert!(wash.contains(&format!("constant float PILL_INSET = {PILL_INSET:?};")), "inset as the wash's pills are");
    }
}
