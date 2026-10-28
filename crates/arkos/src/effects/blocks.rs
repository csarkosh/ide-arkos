//! The blocks of the UI a frame hands the wash shader (spec 4.4): the title bar, the sidebar and
//! its divider, each pane's tab bar and body, and the highlights that fade (spec 4.6). Pure over
//! the layout and the highlight registry, so which blocks, which edges and which seeds are tested.

use crate::tree::{PaneId, Rect};

/// What a block of the UI is, as the preamble's `ARKOS_*_BLOCK` numbers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    TitleBar = 1,
    Sidebar = 2,
    Divider = 3,
    TabBar = 4,
    PaneBody = 5,
    RowHighlight = 6,
    TabHighlight = 7,
}

/// The edges of a block the app no longer draws when the effect inks the borders, a bit each.
pub const EDGE_TOP: u32 = 1;
pub const EDGE_RIGHT: u32 = 2;
pub const EDGE_BOTTOM: u32 = 4;
pub const EDGE_LEFT: u32 = 8;

/// One block as the shader sees it, in window points from the top-left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Block {
    pub rect: Rect,
    pub kind: Kind,
    pub edges: u32,
    /// Fixed for the block's life, so one block coming and going never changes another's look.
    pub seed: f32,
    /// A highlight's fade from 0 to 1; 1 for every other kind.
    pub value: f32,
    /// A pane's place among the panes for its tab bar and body, a highlight's state for a
    /// highlight, else 0.
    pub ordinal: f32,
}

/// Whether a highlight is under the pointer, selected, or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightState {
    Hovered = 0,
    Selected = 1,
    Both = 2,
}

impl HighlightState {
    /// The state of an element selected and hovered as given; `None` when it is neither.
    pub fn of(selected: bool, hovered: bool) -> Option<HighlightState> {
        match (selected, hovered) {
            (true, true) => Some(HighlightState::Both),
            (true, false) => Some(HighlightState::Selected),
            (false, true) => Some(HighlightState::Hovered),
            (false, false) => None,
        }
    }
}

/// A highlight an element reported this frame: its key, where it was laid out, what it is and
/// its state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mark {
    pub key: u64,
    pub rect: Rect,
    pub kind: Kind,
    pub state: HighlightState,
}

/// A tab's key among the highlights, apart from the Explorer rows' path hashes.
pub fn tab_key(tab: u64) -> u64 {
    tab ^ 0xa5a5_0000_0000_0000
}

/// The element under the pointer after the one whose mark is `key` reports the pointer
/// entering (`over`) or leaving it: a leave clears the hover only while it is still that
/// element's, since the next element's enter may come first.
pub fn hover_after(hover: Option<u64>, key: u64, over: bool) -> Option<u64> {
    if over {
        Some(key)
    } else if hover == Some(key) {
        None
    } else {
        hover
    }
}

/// What the blocks are made from: the content area right of the sidebar and below the title bar,
/// the bars' heights, the sidebar's divider when laid out, the panes from the layout, and the
/// highlights with their fades.
pub struct Scene<'a> {
    pub content: Rect,
    pub title_bar_h: f32,
    pub tab_bar_h: f32,
    pub divider_y: Option<f32>,
    pub panes: &'a [(PaneId, Rect)],
    pub strokes: &'a [(Mark, f32)],
}

/// The blocks of `scene`, in the order the shader paints them. Without `ink_borders` no block
/// reports an edge, since the app still draws its own borders.
pub fn blocks(scene: &Scene, ink_borders: bool) -> Vec<Block> {
    let edges = |bits: u32| if ink_borders { bits } else { 0 };
    let c = scene.content;
    let bar = scene.title_bar_h;
    let bottom = c.y + c.h;
    let mut out = vec![Block {
        rect: Rect { x: 0.0, y: 0.0, w: c.x + c.w, h: bar },
        kind: Kind::TitleBar,
        edges: edges(EDGE_BOTTOM),
        seed: 1.0,
        value: 1.0,
        ordinal: 0.0,
    }];
    if c.x > 1.0 {
        out.push(Block {
            rect: Rect { x: 0.0, y: bar, w: c.x, h: (bottom - bar).max(0.0) },
            kind: Kind::Sidebar,
            edges: edges(EDGE_RIGHT),
            seed: 2.0,
            value: 1.0,
            ordinal: 0.0,
        });
        // The line between Explorer and Worktrees: a block with no height.
        if let Some(y) = scene.divider_y.filter(|y| *y > bar && *y < bottom) {
            out.push(Block {
                rect: Rect { x: 0.0, y, w: c.x, h: 0.0 },
                kind: Kind::Divider,
                edges: edges(EDGE_TOP),
                seed: 3.0,
                value: 1.0,
                ordinal: 0.0,
            });
        }
    }
    let tab = scene.tab_bar_h;
    for (i, (pane, r)) in scene.panes.iter().enumerate() {
        // Seeded by the pane itself, not its place in this frame's list.
        let seed = 10.0 + (*pane % 997) as f32 * 2.0;
        out.push(Block {
            rect: Rect { x: r.x, y: r.y, w: r.w, h: tab },
            kind: Kind::TabBar,
            edges: edges(EDGE_TOP | EDGE_RIGHT | EDGE_BOTTOM | EDGE_LEFT),
            seed,
            value: 1.0,
            ordinal: i as f32,
        });
        out.push(Block {
            rect: Rect { x: r.x, y: r.y + tab, w: r.w, h: (r.h - tab).max(0.0) },
            kind: Kind::PaneBody,
            edges: edges(EDGE_RIGHT | EDGE_BOTTOM | EDGE_LEFT),
            seed: seed + 1.0,
            value: 1.0,
            ordinal: i as f32,
        });
    }
    // The highlights last: past the cap, a transient stroke goes before a pane does.
    for (mark, value) in scene.strokes {
        let r = mark.rect;
        out.push(Block {
            rect: Rect { x: r.x + 3.0, y: r.y + 2.0, w: (r.w - 6.0).max(0.0), h: (r.h - 4.0).max(0.0) },
            kind: mark.kind,
            edges: 0,
            // Seeded by its key, not its place, so a row that scrolls keeps its grain.
            seed: 4.0 + (mark.key % 997) as f32 * 0.37,
            value: *value,
            ordinal: mark.state as u32 as f32,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    fn scene<'a>(content: Rect, divider_y: Option<f32>, panes: &'a [(PaneId, Rect)], strokes: &'a [(Mark, f32)]) -> Scene<'a> {
        Scene { content, title_bar_h: 38.0, tab_bar_h: 30.0, divider_y, panes, strokes }
    }

    fn kinds(blocks: &[Block]) -> Vec<Kind> {
        blocks.iter().map(|b| b.kind).collect()
    }

    #[test]
    fn one_pane_without_a_sidebar_gives_a_title_bar_a_tab_bar_and_a_body() {
        let panes = [(7, rect(0.0, 38.0, 800.0, 562.0))];
        let out = blocks(&scene(rect(0.0, 38.0, 800.0, 562.0), Some(300.0), &panes, &[]), true);
        assert_eq!(kinds(&out), vec![Kind::TitleBar, Kind::TabBar, Kind::PaneBody]);
        assert_eq!(out[0].rect, rect(0.0, 0.0, 800.0, 38.0));
        assert_eq!(out[1].rect, rect(0.0, 38.0, 800.0, 30.0));
        assert_eq!(out[2].rect, rect(0.0, 68.0, 800.0, 532.0));
        assert!(out.iter().all(|b| b.value == 1.0));
        assert_eq!((out[1].ordinal, out[2].ordinal), (0.0, 0.0));
    }

    #[test]
    fn a_sidebar_adds_its_block_and_its_divider_when_laid_out() {
        let panes = [(1, rect(240.0, 38.0, 560.0, 562.0)), (2, rect(800.0, 38.0, 400.0, 562.0))];
        let content = rect(240.0, 38.0, 960.0, 562.0);
        let out = blocks(&scene(content, Some(400.0), &panes, &[]), true);
        assert_eq!(kinds(&out), vec![Kind::TitleBar, Kind::Sidebar, Kind::Divider, Kind::TabBar, Kind::PaneBody, Kind::TabBar, Kind::PaneBody]);
        assert_eq!(out[0].rect, rect(0.0, 0.0, 1200.0, 38.0));
        assert_eq!(out[1].rect, rect(0.0, 38.0, 240.0, 562.0));
        assert_eq!(out[2].rect, rect(0.0, 400.0, 240.0, 0.0));
        assert_eq!((out[5].ordinal, out[6].ordinal), (1.0, 1.0));
        // Not laid out yet, or outside the sidebar: no divider.
        assert_eq!(kinds(&blocks(&scene(content, None, &panes, &[]), true))[..3], [Kind::TitleBar, Kind::Sidebar, Kind::TabBar]);
        assert_eq!(kinds(&blocks(&scene(content, Some(900.0), &panes, &[]), true))[..3], [Kind::TitleBar, Kind::Sidebar, Kind::TabBar]);
    }

    #[test]
    fn edges_are_reported_only_when_the_effect_inks_the_borders() {
        let panes = [(1, rect(240.0, 38.0, 960.0, 562.0))];
        let s = scene(rect(240.0, 38.0, 960.0, 562.0), Some(400.0), &panes, &[]);
        let inked: Vec<u32> = blocks(&s, true).iter().map(|b| b.edges).collect();
        assert_eq!(
            inked,
            vec![EDGE_BOTTOM, EDGE_RIGHT, EDGE_TOP, EDGE_TOP | EDGE_RIGHT | EDGE_BOTTOM | EDGE_LEFT, EDGE_RIGHT | EDGE_BOTTOM | EDGE_LEFT]
        );
        assert!(blocks(&s, false).iter().all(|b| b.edges == 0));
    }

    #[test]
    fn a_pane_s_seed_comes_from_its_id_not_its_place() {
        let left = rect(0.0, 38.0, 400.0, 562.0);
        let right = rect(400.0, 38.0, 400.0, 562.0);
        let content = rect(0.0, 38.0, 800.0, 562.0);
        let before = blocks(&scene(content, None, &[(5, left), (9, right)], &[]), true);
        let after = blocks(&scene(content, None, &[(9, left), (5, right)], &[]), true);
        let seeds_of = |out: &[Block], at: Rect| -> (f32, f32) {
            let bar = out.iter().find(|b| b.kind == Kind::TabBar && b.rect.x == at.x).unwrap();
            let body = out.iter().find(|b| b.kind == Kind::PaneBody && b.rect.x == at.x).unwrap();
            (bar.seed, body.seed)
        };
        // Pane 5 moved from the left slot to the right one and kept its seeds.
        assert_eq!(seeds_of(&before, left), seeds_of(&after, right));
        assert_eq!(seeds_of(&before, left), (20.0, 21.0));
        assert_ne!(seeds_of(&before, left), seeds_of(&before, right));
        // Its ordinal is its place, which did change.
        let bar = after.iter().find(|b| b.kind == Kind::TabBar && b.rect.x == right.x).unwrap();
        assert_eq!(bar.ordinal, 1.0);
    }

    #[test]
    fn highlights_become_blocks_with_their_fade_and_state() {
        let row = Mark { key: 1, rect: rect(0.0, 100.0, 240.0, 22.0), kind: Kind::RowHighlight, state: HighlightState::Both };
        let tab = Mark { key: tab_key(3), rect: rect(300.0, 38.0, 120.0, 30.0), kind: Kind::TabHighlight, state: HighlightState::Hovered };
        let strokes = [(row, 0.5), (tab, 1.0)];
        let panes = [(1, rect(240.0, 38.0, 960.0, 562.0))];
        let out = blocks(&scene(rect(240.0, 38.0, 960.0, 562.0), None, &panes, &strokes), true);
        let row_block = out.iter().find(|b| b.kind == Kind::RowHighlight).unwrap();
        assert_eq!(row_block.rect, rect(3.0, 102.0, 234.0, 18.0));
        assert_eq!(row_block.value, 0.5);
        assert_eq!(row_block.ordinal, 2.0);
        assert_eq!(row_block.edges, 0);
        // Seeded by its key, so a row that scrolls keeps its grain.
        assert_eq!(row_block.seed, 4.0 + (1 % 997) as f32 * 0.37);
        let scrolled = Mark { rect: rect(0.0, 140.0, 240.0, 22.0), ..row };
        let moved = blocks(&scene(rect(240.0, 38.0, 960.0, 562.0), None, &panes, &[(scrolled, 0.5)]), true);
        assert_eq!(moved.iter().find(|b| b.kind == Kind::RowHighlight).unwrap().seed, row_block.seed);
        // The panes come before the highlights, so the cap drops a transient stroke first.
        assert_eq!(kinds(&out), vec![Kind::TitleBar, Kind::Sidebar, Kind::TabBar, Kind::PaneBody, Kind::RowHighlight, Kind::TabHighlight]);
        let tab_block = out.iter().find(|b| b.kind == Kind::TabHighlight).unwrap();
        assert_eq!((tab_block.value, tab_block.ordinal), (1.0, 0.0));
        assert_eq!(tab_block.rect, rect(303.0, 40.0, 114.0, 26.0));
        assert_eq!(HighlightState::of(true, false), Some(HighlightState::Selected));
        assert_eq!(HighlightState::of(false, false), None);
    }

    #[test]
    fn a_leave_clears_the_hover_only_when_it_is_still_that_element_s() {
        assert_eq!(hover_after(None, 1, true), Some(1));
        assert_eq!(hover_after(Some(1), 2, true), Some(2));
        // Row 1's leave arrives after row 2's enter: row 2 stays hovered.
        assert_eq!(hover_after(Some(2), 1, false), Some(2));
        assert_eq!(hover_after(Some(2), 2, false), None);
    }
}
