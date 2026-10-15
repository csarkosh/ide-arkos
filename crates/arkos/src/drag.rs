//! Dragging a tab (spec 4): a ghost chip follows the pointer; the hovered tab bar shows where
//! it would land; the hovered pane shows five drop zones (center, left, right, top, bottom).
//! Dragging a file out of the tree (spec 6): dropped on a terminal, its path is typed there.

use crate::palette::Rgb;
use crate::tree::{PaneId, TabId};
use gpui::{div, prelude::*, px, Bounds, Context, Pixels, Point, Render, SharedString, Window};
use crate::motion::{self, Transient};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// What a tab drag carries. `tab` is the tab's stable id: the drop re-locates the tab by it, so
/// `pane` and `idx` (where the drag started) only describe the start.
#[derive(Debug, Clone, PartialEq)]
pub struct TabDrag {
    pub pane: PaneId,
    pub idx: usize,
    pub tab: TabId,
    pub label: String,
    pub position: Point<Pixels>,
    /// The chip's colours, from the theme where the drag started.
    pub bg: Rgb,
    pub fg: Rgb,
    pub family: SharedString,
}

impl TabDrag {
    pub fn at(mut self, position: Point<Pixels>) -> Self {
        self.position = position;
        self
    }
}

/// The ghost chip gpui draws under the pointer while dragging.
impl Render for TabDrag {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().pl(self.position.x + px(8.0)).pt(self.position.y + px(8.0)).child(
            div()
                .px(px(10.0))
                .py(px(4.0))
                .bg(self.bg.hsla())
                .text_color(self.fg.hsla())
                .font_family(self.family.clone())
                .text_size(px(12.0))
                .opacity(0.9)
                .child(self.label.clone()),
        )
    }
}

/// A file dragged out of the tree: dropped on a terminal, its path is pasted (spec 6).
#[derive(Debug, Clone, PartialEq)]
pub struct FileDrag {
    pub path: PathBuf,
    pub position: Point<Pixels>,
    pub bg: Rgb,
    pub fg: Rgb,
    pub family: SharedString,
}

impl FileDrag {
    pub fn at(mut self, position: Point<Pixels>) -> Self {
        self.position = position;
        self
    }
}

impl Render for FileDrag {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let name = self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        div().pl(self.position.x + px(8.0)).pt(self.position.y + px(8.0)).child(
            div()
                .px(px(10.0))
                .py(px(4.0))
                .bg(self.bg.hsla())
                .text_color(self.fg.hsla())
                .font_family(self.family.clone())
                .text_size(px(12.0))
                .opacity(0.9)
                .child(name),
        )
    }
}

/// What a dropped file types into a terminal: the path, single-quoted when it has anything a
/// shell would read, then a space so the next word starts clean. `None` when the path holds a
/// control character (the daemon's `typeable` rule): a newline would run what follows it, an
/// escape would reach the terminal, and quoting stops neither.
pub fn paste_text(path: &Path) -> Option<String> {
    if path.as_os_str().as_encoded_bytes().iter().any(|b| *b < 0x20 || *b == 0x7f) {
        return None;
    }
    let s = path.to_string_lossy();
    let plain = s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c));
    Some(if plain { format!("{s} ") } else { format!("'{}' ", s.replace('\'', "'\\''")) })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Center,
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropTarget {
    TabBar { pane: PaneId, at: usize },
    Pane { pane: PaneId, zone: Zone },
}

/// The inner half of the pane (by each axis) is the center zone; outside it, the nearest edge.
pub fn zone_for(bounds: Bounds<Pixels>, pos: Point<Pixels>) -> Zone {
    let x = f32::from(pos.x - bounds.origin.x) / f32::from(bounds.size.width).max(1.0);
    let y = f32::from(pos.y - bounds.origin.y) / f32::from(bounds.size.height).max(1.0);
    if (0.25..=0.75).contains(&x) && (0.25..=0.75).contains(&y) {
        return Zone::Center;
    }
    let candidates = [(x, Zone::Left), (1.0 - x, Zone::Right), (y, Zone::Top), (1.0 - y, Zone::Bottom)];
    candidates.iter().fold((f32::MAX, Zone::Center), |best, (d, z)| if *d < best.0 { (*d, *z) } else { best }).1
}

/// The pane under a global `pointer` and its zone; `panes` are the panes' bodies in global
/// coordinates (a Chrome window dragged over arkos, spec 8).
pub fn zone_and_pane_for(panes: &[(PaneId, crate::ax::Rect)], pointer: (f64, f64)) -> Option<(PaneId, Zone)> {
    let (x, y) = pointer;
    let (pane, r) = panes.iter().find(|(_, r)| r.contains(x, y))?;
    let bounds = Bounds { origin: gpui::point(gpui::px(r.x as f32), gpui::px(r.y as f32)), size: gpui::size(gpui::px(r.w as f32), gpui::px(r.h as f32)) };
    Some((*pane, zone_for(bounds, gpui::point(gpui::px(x as f32), gpui::px(y as f32)))))
}

/// Hovering the left half of tab `idx` inserts before it, the right half after it.
pub fn insert_index(tab_bounds: Bounds<Pixels>, pos: Point<Pixels>, idx: usize) -> usize {
    let mid = tab_bounds.origin.x + tab_bounds.size.width / 2.0;
    if pos.x < mid {
        idx
    } else {
        idx + 1
    }
}

/// The rectangle a zone highlights inside a pane's bounds, as (x, y, w, h) relative to the pane.
pub fn zone_rect(zone: Zone, w: f32, h: f32) -> (f32, f32, f32, f32) {
    match zone {
        Zone::Center => (0.0, 0.0, w, h),
        Zone::Left => (0.0, 0.0, w / 2.0, h),
        Zone::Right => (w / 2.0, 0.0, w / 2.0, h),
        Zone::Top => (0.0, 0.0, w, h / 2.0),
        Zone::Bottom => (0.0, h / 2.0, w, h / 2.0),
    }
}

/// A rectangle inside a pane's body: (x, y, w, h).
pub type ZoneRect = (f32, f32, f32, f32);

/// `a` moved `t` of the way to `b`, every edge alike.
pub fn lerp_rect(a: ZoneRect, b: ZoneRect, t: f32) -> ZoneRect {
    let l = |x: f32, y: f32| x + (y - x) * t;
    (l(a.0, b.0), l(a.1, b.1), l(a.2, b.2), l(a.3, b.3))
}

/// One pane's drop tint: it fades in and out with the hovered zone, and between zones its
/// rectangle glides from where it was last painted. It only ever glides from its own pane's
/// rectangle, and only while the tint is still on screen: a tint that faded away starts afresh.
#[derive(Default)]
pub struct Tint {
    pub fade: Transient<Zone>,
    /// Where the glide started, and when.
    glide: Option<(ZoneRect, Instant)>,
    /// The rectangle the last frame painted.
    last: Option<ZoneRect>,
}

impl Tint {
    /// Points the tint at `zone`, or takes it down with `None`.
    pub fn aim(&mut self, zone: Option<Zone>, now: Instant) {
        if let Some(z) = zone {
            match self.fade.current(now).map(|(shown, _)| *shown) {
                Some(shown) if shown != z => self.glide = self.last.map(|r| (r, now)),
                Some(_) => {}
                None => {
                    self.glide = None;
                    self.last = None;
                }
            }
        }
        self.fade.set(zone, now);
    }

    /// The rectangle to paint in a body `w` by `h`, and its opacity; `None` when nothing shows.
    /// Fading out, it stays where it was, whatever the drop did to the pane under it.
    pub fn paint(&mut self, w: f32, h: f32, now: Instant) -> Option<(ZoneRect, f32)> {
        let (zone, opacity) = self.fade.current(now)?;
        let to = zone_rect(*zone, w, h);
        let rect = match (self.fade.value(), self.glide) {
            (None, _) => self.last.unwrap_or(to),
            (Some(_), Some((from, since))) => lerp_rect(from, to, motion::enter_progress(since, now, motion::ENTER)),
            (Some(_), None) => to,
        };
        self.last = Some(rect);
        Some((rect, opacity))
    }

    pub fn animating(&self, now: Instant) -> bool {
        self.fade.animating(now) || (self.fade.value().is_some() && self.glide.is_some_and(|(_, since)| motion::enter_progress(since, now, motion::ENTER) < 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size, Bounds};

    fn b(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds { origin: point(px(x), px(y)), size: size(px(w), px(h)) }
    }

    #[test]
    fn zones_split_the_pane_into_center_and_four_edges() {
        let pane = b(100.0, 100.0, 400.0, 200.0);
        assert_eq!(zone_for(pane, point(px(300.0), px(200.0))), Zone::Center);
        assert_eq!(zone_for(pane, point(px(110.0), px(200.0))), Zone::Left);
        assert_eq!(zone_for(pane, point(px(490.0), px(200.0))), Zone::Right);
        assert_eq!(zone_for(pane, point(px(300.0), px(105.0))), Zone::Top);
        assert_eq!(zone_for(pane, point(px(300.0), px(295.0))), Zone::Bottom);
        // Corners go to the nearer edge by distance.
        assert_eq!(zone_for(pane, point(px(104.0), px(110.0))), Zone::Left);
    }

    #[test]
    fn a_dropped_path_is_quoted_only_when_it_needs_it() {
        assert_eq!(paste_text(Path::new("/Users/me/src/main.rs")).as_deref(), Some("/Users/me/src/main.rs "));
        assert_eq!(paste_text(Path::new("/Users/me/My Docs/a b.txt")).as_deref(), Some("'/Users/me/My Docs/a b.txt' "));
        assert_eq!(paste_text(Path::new("/x/it's.txt")).as_deref(), Some("'/x/it'\\''s.txt' "));
    }

    #[test]
    fn a_dropped_path_with_a_control_character_is_not_pasted() {
        assert_eq!(paste_text(Path::new("/x/a\nrm -rf ~")), None, "a newline would run the rest");
        assert_eq!(paste_text(Path::new("/x/\u{1b}]0;title\u{7}")), None, "an escape sequence would reach the terminal");
        assert_eq!(paste_text(Path::new("/x/del\u{7f}")), None);
    }

    #[test]
    fn a_rectangle_moves_part_of_the_way() {
        let a = (0.0, 0.0, 100.0, 50.0);
        let b = (50.0, 0.0, 50.0, 100.0);
        assert_eq!(lerp_rect(a, b, 0.0), a);
        assert_eq!(lerp_rect(a, b, 0.5), (25.0, 0.0, 75.0, 75.0));
        assert_eq!(lerp_rect(a, b, 1.0), b);
    }

    #[test]
    fn the_tint_fades_in_glides_between_zones_and_never_from_a_stale_rect() {
        let _g = motion::test_lock(false);
        let (w, h) = (400.0, 200.0);
        let t0 = Instant::now();
        let mut t = Tint::default();
        t.aim(Some(Zone::Center), t0);
        assert_eq!(t.paint(w, h, t0), Some(((0.0, 0.0, 400.0, 200.0), 0.0)), "a fresh tint fades in where it is, no glide");
        let t1 = t0 + motion::ENTER;
        assert_eq!(t.paint(w, h, t1), Some(((0.0, 0.0, 400.0, 200.0), 1.0)));
        assert!(!t.animating(t1));
        // To the right half: from the painted rectangle, three quarters there at half time.
        t.aim(Some(Zone::Right), t1);
        assert!(t.animating(t1));
        assert_eq!(t.paint(w, h, t1).unwrap().0, (0.0, 0.0, 400.0, 200.0));
        let (r, o) = t.paint(w, h, t1 + motion::ENTER / 2).unwrap();
        assert_eq!(o, 1.0, "the opacity stays up while it glides");
        assert!((r.0 - 150.0).abs() < 0.5 && (r.2 - 250.0).abs() < 0.5, "{r:?}");
        let t2 = t1 + motion::ENTER;
        assert_eq!(t.paint(w, h, t2).unwrap().0, (200.0, 0.0, 200.0, 200.0));
        // A drop: it fades where it was, even though the pane under it changed size.
        t.aim(None, t2);
        let (r, o) = t.paint(w / 2.0, h, t2 + motion::EXIT / 2).unwrap();
        assert_eq!(r, (200.0, 0.0, 200.0, 200.0));
        assert!(o > 0.0 && o < 1.0);
        let t3 = t2 + motion::EXIT;
        assert!(t.paint(w, h, t3).is_none());
        assert!(!t.animating(t3));
        // Hovered again later: it starts at the new zone, not from the old rectangle.
        t.aim(Some(Zone::Left), t3 + motion::EXIT);
        assert_eq!(t.paint(w, h, t3 + motion::EXIT).unwrap().0, (0.0, 0.0, 200.0, 200.0));
    }

    #[test]
    fn the_tint_snaps_under_reduce_motion() {
        let _g = motion::test_lock(true);
        let t0 = Instant::now();
        let mut t = Tint::default();
        t.aim(Some(Zone::Center), t0);
        assert_eq!(t.paint(400.0, 200.0, t0), Some(((0.0, 0.0, 400.0, 200.0), 1.0)));
        t.aim(Some(Zone::Top), t0);
        assert_eq!(t.paint(400.0, 200.0, t0), Some(((0.0, 0.0, 400.0, 100.0), 1.0)));
        assert!(!t.animating(t0));
        t.aim(None, t0);
        assert!(t.paint(400.0, 200.0, t0).is_none());
    }

    #[test]
    fn insert_index_depends_on_which_half_of_the_tab_is_hovered() {
        let tab = b(200.0, 0.0, 100.0, 28.0);
        assert_eq!(insert_index(tab, point(px(210.0), px(10.0)), 3), 3);
        assert_eq!(insert_index(tab, point(px(290.0), px(10.0)), 3), 4);
    }

    #[test]
    fn the_pane_and_zone_under_a_global_pointer() {
        let panes = [(1u64, crate::ax::Rect { x: 100.0, y: 100.0, w: 400.0, h: 200.0 }), (2u64, crate::ax::Rect { x: 500.0, y: 100.0, w: 400.0, h: 200.0 })];
        assert_eq!(zone_and_pane_for(&panes, (300.0, 200.0)), Some((1u64, Zone::Center)));
        assert_eq!(zone_and_pane_for(&panes, (490.0, 200.0)), Some((1u64, Zone::Right)));
        assert_eq!(zone_and_pane_for(&panes, (510.0, 200.0)), Some((2u64, Zone::Left)));
        assert_eq!(zone_and_pane_for(&panes, (700.0, 295.0)), Some((2u64, Zone::Bottom)));
        assert_eq!(zone_and_pane_for(&panes, (700.0, 50.0)), None, "above the panes");
        assert_eq!(zone_and_pane_for(&panes, (950.0, 200.0)), None, "right of them");
    }
}
