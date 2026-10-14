//! Dragging a tab (spec 4): a ghost chip follows the pointer; the hovered tab bar shows where
//! it would land; the hovered pane shows five drop zones (center, left, right, top, bottom).
//! Dragging a file out of the tree (spec 6): dropped on a terminal, its path is typed there.

use crate::palette::Rgb;
use crate::tree::{PaneId, TabId};
use gpui::{div, prelude::*, px, Bounds, Context, Pixels, Point, Render, SharedString, Window};
use std::path::{Path, PathBuf};

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
/// shell would read, then a space so the next word starts clean.
pub fn paste_text(path: &Path) -> String {
    let s = path.to_string_lossy();
    let plain = s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c));
    if plain {
        format!("{s} ")
    } else {
        format!("'{}' ", s.replace('\'', "'\\''"))
    }
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
        assert_eq!(paste_text(Path::new("/Users/me/src/main.rs")), "/Users/me/src/main.rs ");
        assert_eq!(paste_text(Path::new("/Users/me/My Docs/a b.txt")), "'/Users/me/My Docs/a b.txt' ");
        assert_eq!(paste_text(Path::new("/x/it's.txt")), "'/x/it'\\''s.txt' ");
    }

    #[test]
    fn insert_index_depends_on_which_half_of_the_tab_is_hovered() {
        let tab = b(200.0, 0.0, 100.0, 28.0);
        assert_eq!(insert_index(tab, point(px(210.0), px(10.0)), 3), 3);
        assert_eq!(insert_index(tab, point(px(290.0), px(10.0)), 3), 4);
    }
}
