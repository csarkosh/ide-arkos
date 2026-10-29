//! The blocks of the UI a frame hands the wash shader (spec 4.4): the title bar, the sidebar and
//! its divider, each pane's tab bar and body, the shapes inside panes (a video's picture, a
//! markdown table and its header), and the highlights that fade (spec 4.6). Pure over
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
    /// A video's picture as fitted in its tab, opaque over the wash.
    Media = 8,
    /// A markdown table, its fills clear under an effect.
    Table = 9,
    /// A markdown table's header row.
    TableHeader = 10,
    /// An open menu or popover, drawn above the panes by the effect's menu stage.
    Menu = 11,
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

/// Something inside a pane that reports itself to the wash as it is laid out: a video's picture
/// (`Kind::Media`), a markdown table (`Kind::Table`) or its header row (`Kind::TableHeader`),
/// keyed by what draws it so its grain stays put as it moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    pub key: u64,
    pub rect: Rect,
    pub kind: Kind,
}

/// An open menu or popover as it reported itself this frame: what draws it, where, and how far
/// in it has faded (its edges fade with it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MenuShape {
    pub key: u64,
    pub rect: Rect,
    pub opacity: f32,
}

/// The blocks the effect's menu stage draws: each open menu with all four edges, its fade as its
/// value, and a seed from what draws it so its grain stays put as it moves.
pub fn menu_blocks(menus: &[MenuShape]) -> Vec<Block> {
    menus
        .iter()
        .map(|m| Block {
            rect: m.rect,
            kind: Kind::Menu,
            edges: EDGE_TOP | EDGE_RIGHT | EDGE_BOTTOM | EDGE_LEFT,
            seed: 50.0 + (m.key % 997) as f32 * 0.61,
            value: m.opacity,
            ordinal: 0.0,
        })
        .collect()
}

/// How far a video's outline sits outside its picture, in points: about the ink's own weight, so
/// the line is seen whole beside the picture rather than half under it.
const MEDIA_OUTSET: f32 = 1.5;

/// What the blocks are made from: the content area right of the sidebar and below the title bar,
/// the bars' heights, the sidebar's divider when laid out, the panes from the layout, the shapes
/// laid out inside them, and the highlights with their fades.
pub struct Scene<'a> {
    pub content: Rect,
    pub title_bar_h: f32,
    pub tab_bar_h: f32,
    pub divider_y: Option<f32>,
    pub panes: &'a [(PaneId, Rect)],
    pub shapes: &'a [Shape],
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
    for shape in scene.shapes {
        let (r, all) = (shape.rect, EDGE_TOP | EDGE_RIGHT | EDGE_BOTTOM | EDGE_LEFT);
        let (rect, bits) = match shape.kind {
            // A video's picture is opaque over the wash, so its outline is drawn just outside it.
            Kind::Media => (Rect { x: r.x - MEDIA_OUTSET, y: r.y - MEDIA_OUTSET, w: r.w + 2.0 * MEDIA_OUTSET, h: r.h + 2.0 * MEDIA_OUTSET }, all),
            // A header is set off by the line under it; the table's outline draws the rest.
            Kind::TableHeader => (r, EDGE_BOTTOM),
            _ => (r, all),
        };
        out.push(Block {
            rect,
            kind: shape.kind,
            edges: edges(bits),
            seed: 30.0 + (shape.key % 997) as f32 * 0.53 + shape.kind as u32 as f32 * 0.29,
            value: 1.0,
            ordinal: 0.0,
        });
    }
    // The highlights last: past the cap, a transient stroke goes before a pane does.
    for (mark, value) in scene.strokes {
        out.push(stroke_block(mark, *value));
    }
    out
}

/// A highlight's block: its stroke, inset from its row or tab, at its fade.
fn stroke_block(mark: &Mark, value: f32) -> Block {
    let r = mark.rect;
    Block {
        rect: Rect { x: r.x + 3.0, y: r.y + 2.0, w: (r.w - 6.0).max(0.0), h: (r.h - 4.0).max(0.0) },
        kind: mark.kind,
        edges: 0,
        // Seeded by its key, not its place, so a row that scrolls keeps its grain.
        seed: 4.0 + (mark.key % 997) as f32 * 0.37,
        value,
        ordinal: mark.state as u32 as f32,
    }
}

/// The blocks a menu's backdrop is washed from: the menu itself (no edges, which the menu stage
/// draws; unfaded, as the backdrop fades with the menu as a whole) and the highlight strokes whose
/// middle lies inside it, as the wash would paint them.
pub fn backdrop_blocks(menu: &MenuShape, strokes: &[(Mark, f32)]) -> Vec<Block> {
    let m = menu.rect;
    let inside = |r: Rect| {
        let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        cx >= m.x && cx <= m.x + m.w && cy >= m.y && cy <= m.y + m.h
    };
    let own = Block { rect: m, kind: Kind::Menu, edges: 0, seed: 50.0 + (menu.key % 997) as f32 * 0.61, value: 1.0, ordinal: 0.0 };
    std::iter::once(own).chain(strokes.iter().filter(|(mark, _)| inside(mark.rect)).map(|(mark, v)| stroke_block(mark, *v))).collect()
}

/// A menu row's key among the highlights: the menu's key and the row's place, apart from the
/// Explorer rows' path hashes and the tabs' keys.
pub fn menu_row_key(menu: u64, row: usize) -> u64 {
    0x5c5c_0000_0000_0000 ^ (menu << 16) ^ row as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    fn scene<'a>(content: Rect, divider_y: Option<f32>, panes: &'a [(PaneId, Rect)], strokes: &'a [(Mark, f32)]) -> Scene<'a> {
        Scene { content, title_bar_h: 38.0, tab_bar_h: 30.0, divider_y, panes, shapes: &[], strokes }
    }

    #[test]
    fn a_menu_s_backdrop_is_its_own_block_and_the_hover_strokes_inside_it() {
        let menu = MenuShape { key: 0x11, rect: rect(100.0, 80.0, 200.0, 100.0), opacity: 0.5 };
        let inside = Mark { key: menu_row_key(0x11, 1), rect: rect(100.0, 104.0, 200.0, 24.0), kind: Kind::RowHighlight, state: HighlightState::Hovered };
        let outside = Mark { key: 7, rect: rect(0.0, 300.0, 240.0, 22.0), kind: Kind::RowHighlight, state: HighlightState::Selected };
        let out = backdrop_blocks(&menu, &[(inside, 0.6), (outside, 1.0)]);
        assert_eq!(kinds(&out), vec![Kind::Menu, Kind::RowHighlight], "only the strokes inside the menu");
        // The menu block draws no edges here (the menu stage draws those) and is not faded: the
        // backdrop as a whole fades with the menu.
        assert_eq!((out[0].rect, out[0].edges, out[0].value), (menu.rect, 0, 1.0));
        // A stroke is inset from its row as in the wash, keeps its fade and its state.
        assert_eq!(out[1].rect, rect(103.0, 106.0, 194.0, 20.0));
        assert_eq!((out[1].value, out[1].ordinal), (0.6, HighlightState::Hovered as u32 as f32));
        // The same stroke gives the same block in the wash and in the backdrop.
        let panes = [];
        let in_wash = blocks(&Scene { content: rect(0.0, 0.0, 800.0, 600.0), title_bar_h: 38.0, tab_bar_h: 30.0, divider_y: None, panes: &panes, shapes: &[], strokes: &[(inside, 0.6)] }, true);
        assert_eq!(in_wash.last(), out.last());
    }

    #[test]
    fn menu_rows_have_keys_of_their_own() {
        let keys: Vec<u64> = (0..4).flat_map(|row| [menu_row_key(0x11, row), menu_row_key(0x12, row)]).collect();
        for (i, a) in keys.iter().enumerate() {
            assert!(!keys[i + 1..].contains(a), "{a:x} repeats");
            assert_ne!(*a, tab_key(*a), "apart from the tabs' keys");
        }
        assert_eq!(menu_row_key(0x11, 2), menu_row_key(0x11, 2));
    }

    #[test]
    fn each_open_menu_is_a_menu_block_with_all_four_edges_and_its_fade() {
        assert!(menu_blocks(&[]).is_empty());
        let menus = [MenuShape { key: 9, rect: rect(100.0, 80.0, 200.0, 60.0), opacity: 0.4 }, MenuShape { key: 3, rect: rect(10.0, 10.0, 180.0, 120.0), opacity: 1.0 }];
        let out = menu_blocks(&menus);
        assert_eq!(kinds(&out), vec![Kind::Menu, Kind::Menu]);
        assert_eq!(out[0].rect, menus[0].rect);
        assert_eq!(out[0].edges, EDGE_TOP | EDGE_RIGHT | EDGE_BOTTOM | EDGE_LEFT);
        assert_eq!((out[0].value, out[1].value), (0.4, 1.0), "a menu's edges fade with it");
        // A menu keeps its grain as it moves and fades.
        let moved = menu_blocks(&[MenuShape { key: 9, rect: rect(140.0, 90.0, 200.0, 60.0), opacity: 1.0 }]);
        assert_eq!(moved[0].seed, out[0].seed);
        assert_ne!(out[0].seed, out[1].seed);
        assert_eq!(Kind::Menu as i32, 11, "the preamble's ARKOS_MENU_BLOCK");
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
    fn the_built_in_wash_paints_a_highlight_s_rect_as_given_without_insetting_it_again() {
        // The inset is made once, here in `blocks`; the preamble promises a block's rect is what
        // the shader paints, and a template copied from the built-in inherits what it does.
        let wash = crate::effects::manifest::load("watercolor", std::path::Path::new("/nonexistent/effects")).unwrap().wash;
        assert!(!wash.contains("r.x + 3.0"), "the wash insets a highlight a second time");
        assert!(!wash.contains("r = float4("), "the wash reshapes a block's rect");
    }

    #[test]
    fn a_video_s_picture_is_a_media_block_inked_on_all_four_sides_just_outside_the_picture() {
        let panes = [(1, rect(0.0, 38.0, 800.0, 562.0))];
        let frames = [Shape { key: 42, rect: rect(200.0, 68.0, 400.0, 500.0), kind: Kind::Media }];
        let row = Mark { key: 1, rect: rect(0.0, 100.0, 240.0, 22.0), kind: Kind::RowHighlight, state: HighlightState::Hovered };
        let strokes = [(row, 1.0)];
        let inked = blocks(&Scene { shapes: &frames, ..scene(rect(0.0, 38.0, 800.0, 562.0), None, &panes, &strokes) }, true);
        // After the panes and before the highlights, so the cap still drops a stroke first.
        assert_eq!(kinds(&inked), vec![Kind::TitleBar, Kind::TabBar, Kind::PaneBody, Kind::Media, Kind::RowHighlight]);
        let media = inked.iter().find(|b| b.kind == Kind::Media).unwrap();
        // The picture is opaque over the wash, so the line sits just outside it to be seen whole.
        assert_eq!(media.rect, rect(198.5, 66.5, 403.0, 503.0));
        assert_eq!(media.edges, EDGE_TOP | EDGE_RIGHT | EDGE_BOTTOM | EDGE_LEFT);
        assert_eq!((media.value, media.ordinal), (1.0, 0.0));
        // Seeded by its key, so a picture that moves keeps its grain.
        let moved_frames = [Shape { rect: rect(10.0, 68.0, 400.0, 500.0), ..frames[0] }];
        let moved = blocks(&Scene { shapes: &moved_frames, ..scene(rect(0.0, 38.0, 800.0, 562.0), None, &panes, &[]) }, true);
        assert_eq!(moved.iter().find(|b| b.kind == Kind::Media).unwrap().seed, media.seed);
        // Without inked borders the block is still reported, with no edges.
        let plain = blocks(&Scene { shapes: &frames, ..scene(rect(0.0, 38.0, 800.0, 562.0), None, &panes, &[]) }, false);
        assert_eq!(plain.iter().find(|b| b.kind == Kind::Media).unwrap().edges, 0);
    }

    #[test]
    fn a_table_is_inked_round_its_outline_and_its_header_under_its_last_line() {
        let panes = [(1, rect(0.0, 38.0, 800.0, 562.0))];
        let shapes = [
            Shape { key: 7, rect: rect(20.0, 200.0, 576.0, 200.0), kind: Kind::Table },
            Shape { key: 7, rect: rect(20.0, 200.0, 576.0, 40.0), kind: Kind::TableHeader },
        ];
        let inked = blocks(&Scene { shapes: &shapes, ..scene(rect(0.0, 38.0, 800.0, 562.0), None, &panes, &[]) }, true);
        assert_eq!(kinds(&inked), vec![Kind::TitleBar, Kind::TabBar, Kind::PaneBody, Kind::Table, Kind::TableHeader]);
        let (table, header) = (inked[3], inked[4]);
        // A table's fills are clear under an effect, so its outline is drawn on its edge.
        assert_eq!(table.rect, shapes[0].rect);
        assert_eq!(table.edges, EDGE_TOP | EDGE_RIGHT | EDGE_BOTTOM | EDGE_LEFT);
        assert_eq!((header.rect, header.edges), (shapes[1].rect, EDGE_BOTTOM));
        assert_ne!(table.seed, header.seed, "the header has its own grain");
        let plain = blocks(&Scene { shapes: &shapes, ..scene(rect(0.0, 38.0, 800.0, 562.0), None, &panes, &[]) }, false);
        assert!(plain.iter().all(|b| b.edges == 0));
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
