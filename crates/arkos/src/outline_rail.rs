//! The outline rail of a markdown tab (outline rail spec, section 2): the layout rules under its
//! paint. The collapsed rail is a stepper of dots at the tab's left edge, the expanded panel a
//! labelled list beside the document, or over it in a narrow tab. Everything here is pure: the
//! headings in, the dots, the shape and the panel's rows out.
#![allow(dead_code)] // Task 6 of the outline rail paints it.

use crate::md_fold::{self, Head};

/// The collapsed rail's width.
pub const RAIL_W: f32 = 26.0;
/// The expanded panel's width.
pub const PANEL_W: f32 = 220.0;
/// Below this tab width the expanded panel lies over the document instead of beside it.
pub const OVERLAY_BELOW: f32 = 520.0;
/// Below this tab width there is no rail at all.
pub const MIN_TAB_W: f32 = 200.0;
/// The least distance between two dots' tops.
pub const DOT_GAP: f32 = 16.0;
/// A top-level heading's dot.
const TOP_SIZE: f32 = 12.0;
/// A dot for a heading one level below the top.
const SUB_SIZE: f32 = 8.0;

/// Where a section stands against the one in view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DotState {
    /// Above the section in view.
    Read,
    /// The section in view.
    Current,
    /// Below it.
    Ahead,
}

/// One dot of the collapsed rail: its heading (an index into the headings), the top of its slot
/// (a slot is as tall as a top-level dot, and a smaller dot is centred in it), its size and its
/// state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dot {
    pub head: usize,
    pub y: f32,
    pub size: f32,
    pub state: DotState,
}

/// The collapsed rail's dots for `heads` in a rail `height` tall with the section `current` in
/// view: the top two levels (the document's lowest level and the one below) evenly spaced from
/// `top` down, at least DOT_GAP apart; when they do not fit, the top level alone; when that does
/// not fit either, every top-level dot DOT_GAP apart from `top` (the rail scrolls, `scroll_to`
/// says where). Sizes 12.0 and 8.0. The current dot is the last one at or above the heading in
/// view, so a deeper section in view lights the dot of the section holding it.
pub fn dots(heads: &[Head], current: Option<usize>, height: f32, top: f32) -> Vec<Dot> {
    let Some(first) = heads.iter().map(|h| h.level).min() else { return Vec::new() };
    let second = heads.iter().map(|h| h.level).filter(|&l| l > first).min();
    let pick = |levels: &[u8]| -> Vec<usize> { heads.iter().enumerate().filter(|(_, h)| levels.contains(&h.level)).map(|(i, _)| i).collect() };
    // The even spacing of `n` dots, the last slot's bottom at the rail's bottom.
    let spacing = |n: usize| if n <= 1 { f32::INFINITY } else { (height - TOP_SIZE) / (n - 1) as f32 };
    let both = pick(&second.map_or(vec![first], |s| vec![first, s]));
    let (chosen, gap) = if spacing(both.len()) >= DOT_GAP {
        let g = spacing(both.len());
        (both, g)
    } else {
        let alone = pick(&[first]);
        let g = spacing(alone.len());
        (alone, if g >= DOT_GAP { g } else { DOT_GAP })
    };
    let lit = current.and_then(|c| chosen.iter().rposition(|&h| h <= c));
    chosen
        .iter()
        .enumerate()
        .map(|(k, &head)| {
            let state = match lit {
                Some(l) if k < l => DotState::Read,
                Some(l) if k == l => DotState::Current,
                _ => DotState::Ahead,
            };
            let size = if heads[head].level == first { TOP_SIZE } else { SUB_SIZE };
            Dot { head, y: top + k as f32 * if gap.is_finite() { gap } else { 0.0 }, size, state }
        })
        .collect()
}

/// The rail's scroll offset that keeps the current dot in view, 0 when everything fits. The
/// current dot is held in the middle of the rail where the dots allow, never scrolling past the
/// first or the last.
pub fn scroll_to(dots: &[Dot], current: Option<usize>, height: f32) -> f32 {
    let (Some(first), Some(last)) = (dots.first(), dots.last()) else { return 0.0 };
    let overflow = last.y + TOP_SIZE - (first.y + height);
    if overflow <= 0.0 {
        return 0.0;
    }
    let Some(cur) = current.and_then(|c| dots.iter().rev().find(|d| d.head <= c)) else { return 0.0 };
    (cur.y - first.y - (height - TOP_SIZE) / 2.0).clamp(0.0, overflow)
}

/// What the tab's left edge shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// Nothing: the document takes the full width.
    None,
    /// The collapsed dot rail.
    Rail,
    /// The expanded panel beside the document.
    Panel,
    /// The expanded panel over the document, with a scrim over the rest.
    Overlay,
}

/// What a tab `width` wide shows: nothing under MIN_TAB_W or with no headings, the rail when
/// collapsed, the panel beside the document when expanded and wide, the overlay when narrow.
/// Presentation mode shows the document alone.
pub fn shape(width: f32, expanded: bool, has_headings: bool, presenting: bool) -> Shape {
    if presenting || !has_headings || width < MIN_TAB_W {
        Shape::None
    } else if expanded && width < OVERLAY_BELOW {
        Shape::Overlay
    } else if expanded {
        Shape::Panel
    } else {
        Shape::Rail
    }
}

/// One row of the expanded panel: its heading, how deep it sits (`md_fold::outline`) and its
/// state against the section in view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelRow {
    pub head: usize,
    pub depth: usize,
    pub state: DotState,
}

/// The expanded panel's rows: every heading with its depth and state.
pub fn panel_rows(heads: &[Head], current: Option<usize>) -> Vec<PanelRow> {
    md_fold::outline(heads)
        .into_iter()
        .map(|r| {
            let state = match current {
                Some(c) if r.head < c => DotState::Read,
                Some(c) if r.head == c => DotState::Current,
                _ => DotState::Ahead,
            };
            PanelRow { head: r.head, depth: r.depth, state }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heads(levels: &[u8]) -> Vec<Head> {
        levels.iter().enumerate().map(|(i, &level)| Head { block: i * 2, level, text: format!("h{i}"), nth: 0 }).collect()
    }

    #[test]
    fn dots_mark_the_top_two_levels_read_current_and_ahead() {
        let d = dots(&heads(&[1, 2, 2, 1, 3]), Some(2), 300.0, 0.0);
        assert_eq!(d.iter().map(|d| d.head).collect::<Vec<_>>(), vec![0, 1, 2, 3], "no dot for the third level");
        assert_eq!(d.iter().map(|d| d.state).collect::<Vec<_>>(), vec![DotState::Read, DotState::Read, DotState::Current, DotState::Ahead]);
        assert_eq!(d.iter().map(|d| d.size).collect::<Vec<_>>(), vec![12.0, 8.0, 8.0, 12.0]);
        let deeper = dots(&heads(&[1, 2, 2, 1, 3]), Some(4), 300.0, 0.0);
        assert_eq!(deeper[3].state, DotState::Current, "a section in view without a dot lights the dot of the one holding it");
        assert!(dots(&heads(&[1, 2]), None, 300.0, 0.0).iter().all(|d| d.state == DotState::Ahead), "above the first heading everything is ahead");
        let shifted = dots(&heads(&[2, 3, 4]), None, 300.0, 0.0);
        assert_eq!(shifted.iter().map(|d| d.size).collect::<Vec<_>>(), vec![12.0, 8.0], "the document's own top two levels");
        assert!(dots(&[], None, 300.0, 0.0).is_empty());
    }

    #[test]
    fn dots_are_spaced_evenly_at_least_the_gap_apart() {
        let d = dots(&heads(&[1, 2, 2, 1]), None, 300.0, 30.0);
        assert_eq!(d.len(), 4);
        assert_eq!(d[0].y, 30.0, "from the top");
        let gaps: Vec<f32> = d.windows(2).map(|w| w[1].y - w[0].y).collect();
        assert!(gaps.iter().all(|&g| g >= DOT_GAP), "{gaps:?}");
        assert!(gaps.windows(2).all(|w| (w[0] - w[1]).abs() < 0.001), "evenly: {gaps:?}");
        assert!(d[3].y + 12.0 <= 330.0 + 0.001, "the last inside the rail");
    }

    #[test]
    fn a_long_document_keeps_its_dots_apart() {
        let mixed: Vec<u8> = (0..60).map(|i| if i % 2 == 0 { 1 } else { 2 }).collect();
        let d = dots(&heads(&mixed), Some(10), 300.0, 0.0);
        assert!(d.iter().all(|d| d.size == 12.0), "only the top level when both do not fit");
        assert_eq!(d.len(), 30);
        assert!(d.windows(2).all(|w| w[1].y - w[0].y >= DOT_GAP - 0.001));
        let long = heads(&[1; 60]);
        for current in [0, 30, 59] {
            let d = dots(&long, Some(current), 300.0, 0.0);
            assert_eq!(d.len(), 60);
            assert!(d.windows(2).all(|w| (w[1].y - w[0].y - DOT_GAP).abs() < 0.001), "the gap apart");
            let s = scroll_to(&d, Some(current), 300.0);
            let y = d[current].y - s;
            assert!(y >= 0.0 && y + 12.0 <= 300.0, "the current dot at {y} inside the rail (scroll {s})");
        }
        let short = dots(&heads(&[1, 1, 1]), Some(2), 300.0, 0.0);
        assert_eq!(scroll_to(&short, Some(2), 300.0), 0.0, "everything fits");
    }

    #[test]
    fn the_shape_follows_width_state_and_headings() {
        assert_eq!(shape(800.0, false, true, false), Shape::Rail);
        assert_eq!(shape(800.0, true, true, false), Shape::Panel);
        assert_eq!(shape(500.0, true, true, false), Shape::Overlay);
        assert_eq!(shape(500.0, false, true, false), Shape::Rail);
        assert_eq!(shape(800.0, true, false, false), Shape::None);
        assert_eq!(shape(800.0, true, true, true), Shape::None, "presentation mode shows the document alone");
    }

    #[test]
    fn a_very_narrow_tab_shows_no_rail() {
        assert_eq!(shape(160.0, false, true, false), Shape::None);
    }

    #[test]
    fn panel_rows_carry_every_heading_with_its_depth() {
        let rows = panel_rows(&heads(&[1, 2, 3, 1]), Some(1));
        assert_eq!(rows.iter().map(|r| r.depth).collect::<Vec<_>>(), vec![0, 1, 2, 0]);
        assert_eq!(rows.iter().map(|r| r.head).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
        assert_eq!(rows.iter().map(|r| r.state).collect::<Vec<_>>(), vec![DotState::Read, DotState::Current, DotState::Ahead, DotState::Ahead]);
    }
}
