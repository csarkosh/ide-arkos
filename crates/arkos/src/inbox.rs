//! The Inbox layout's sides (layouts spec, "The Inbox layout"): the panes beside the sessions
//! belong to the session shown, and exist only within it. Choosing another session puts them
//! away whole and brings that session's own in their place. Pure over the tree, so it is tested;
//! the window keeps one `Sides` and calls it as each frame begins.
use crate::tree::{Node, Orientation, Side, SidePlace, Tab, TabId, Tree, INBOX_RATIO, INBOX_STACKED};
use std::collections::HashMap;

/// Each session's side while it is not the one shown, and which session the side in the tree
/// belongs to. A session is known by its terminal tab's id, which it keeps when its shell is
/// started again.
#[derive(Debug, Clone, PartialEq)]
pub struct Sides {
    /// The sides put away, by their session's tab.
    pub kept: HashMap<TabId, Node>,
    /// The session whose side is in the tree. None before the first frame in the layout.
    pub shown: Option<TabId>,
    /// The sessions pane's share of the width beside a side, and of the height stacked with
    /// one: one of each for every session, so the divider stays where the user put it as they
    /// go from one to another (Inbox stacking spec, section 1).
    pub ratio: f32,
    pub stacked: f32,
    /// Where the user last dragged a side to; until they have, a new side follows the window.
    pub place: Option<Side>,
    /// Whether the window is narrow (`tree::is_narrow`), set by the window as each frame begins:
    /// with no place chosen, a narrow window puts a new side on top, a wide one to the right.
    pub narrow: bool,
    /// Which session each tab that is no terminal belongs to, by tab. Kept through the Grid, so
    /// a return to the Inbox hands each session its tabs again.
    pub owner: HashMap<TabId, TabId>,
}

impl Default for Sides {
    fn default() -> Self {
        Sides { kept: HashMap::new(), shown: None, ratio: INBOX_RATIO, stacked: INBOX_STACKED, place: None, narrow: false, owner: HashMap::new() }
    }
}

/// The session the Inbox shows: the tab of the sessions pane's active terminal.
pub fn selected(tree: &Tree) -> Option<TabId> {
    let (sessions, _) = tree.inbox_panes()?;
    let p = tree.pane(sessions)?;
    matches!(p.tabs.get(p.active)?, Tab::Terminal { .. }).then(|| p.ids.get(p.active).copied()).flatten()
}

/// The tabs of the side that is in the tree.
fn side_tabs(tree: &Tree) -> Vec<TabId> {
    let Some((sessions, _)) = tree.inbox_panes() else { return Vec::new() };
    tree.panes().into_iter().filter(|p| *p != sessions).filter_map(|p| tree.pane(p)).flat_map(|p| p.ids.clone()).collect()
}

impl Sides {
    /// Where a side that comes into the tree goes: the user's place, else on top of the session
    /// on a narrow window and to its right on a wide one.
    pub fn new_side(&self) -> Side {
        self.place.unwrap_or(if self.narrow { Side::Top } else { Side::Right })
    }

    /// `side` with the divider kept for its orientation.
    pub fn place_at(&self, side: Side) -> SidePlace {
        SidePlace { side, share: if side.orientation() == Orientation::Horizontal { self.ratio } else { self.stacked } }
    }

    /// Where a side that comes into the tree goes, divider and all.
    pub fn side_place(&self) -> SidePlace {
        self.place_at(self.new_side())
    }

    /// Keeps the divider of a side as it stands, for its orientation.
    fn remember(&mut self, place: SidePlace) {
        if place.side.orientation() == Orientation::Horizontal {
            self.ratio = place.share;
        } else {
            self.stacked = place.share;
        }
    }

    /// The user dragged the side to `side` of the sessions: it moves there, and every side after
    /// it goes there too. False when there is no side to move.
    pub fn drag_to(&mut self, tree: &mut Tree, side: Side) -> bool {
        if tree.inbox_place().is_none() {
            return false;
        }
        self.place = Some(side);
        tree.inbox_set_place(self.place_at(side))
    }

    /// Puts the side in the tree away under the session it belongs to, and brings `to`'s in its
    /// place. With no session shown yet the side there is `to`'s already.
    pub fn swap(&mut self, tree: &mut Tree, to: Option<TabId>) {
        if self.shown == to {
            return;
        }
        if let Some(from) = self.shown {
            if let Some((side, place)) = tree.inbox_take_side() {
                self.remember(place);
                for tab in side.tab_ids() {
                    self.owner.insert(tab, from);
                }
                self.kept.insert(from, side);
            }
        }
        if let Some(side) = to.and_then(|s| self.kept.remove(&s)) {
            tree.inbox_absorb(side, self.side_place());
        }
        self.shown = to;
    }

    /// Holds the Inbox's shape and shows the chosen session's side, whatever changed the tree
    /// since the last frame. True when the tree changed.
    pub fn keep_shape(&mut self, tree: &mut Tree) -> bool {
        let mut changed = tree.arrange_inbox(self.side_place());
        if let Some(place) = tree.inbox_place() {
            self.remember(place);
        }
        let now = selected(tree);
        if now != self.shown {
            self.swap(tree, now);
            changed = true;
        }
        changed
    }

    /// The layout is switched to the Inbox: the tree is gathered into its shape, the Grid's
    /// arrangement returned to be kept aside, and each session is handed the tabs it owned, the
    /// rest staying with the session shown.
    pub fn enter(&mut self, tree: &mut Tree) -> Tree {
        let grid = tree.clone();
        tree.gather_inbox(self.side_place());
        let now = selected(tree);
        let sessions: Vec<TabId> = tree.inbox_panes().and_then(|(s, _)| tree.pane(s)).map(|p| p.ids.clone()).unwrap_or_default();
        self.kept.clear();
        for session in sessions.into_iter().filter(|s| Some(*s) != now) {
            let theirs: Vec<TabId> = self.owner.iter().filter(|(_, o)| **o == session).map(|(tab, _)| *tab).collect();
            if let Some(side) = tree.inbox_split_off(&theirs) {
                self.kept.insert(session, side);
            }
        }
        self.shown = now;
        grid
    }

    /// The layout is switched to the Grid: every session's tabs come back into the tree, each
    /// remembered as its session's, and the Grid's arrangement `grid` is restored around them
    /// (`Tree::regridded`); with none kept, the tree stays as it stands.
    pub fn leave(&mut self, tree: &mut Tree, grid: Option<Tree>) {
        if let Some(shown) = self.shown {
            for tab in side_tabs(tree) {
                self.owner.insert(tab, shown);
            }
        }
        let mut kept: Vec<(TabId, Node)> = self.kept.drain().collect();
        kept.sort_by_key(|(session, _)| *session);
        for (session, side) in kept {
            for tab in side.tab_ids() {
                self.owner.insert(tab, session);
            }
            tree.inbox_absorb(side, self.side_place());
        }
        self.shown = None;
        if let Some(grid) = grid {
            *tree = tree.regridded(&grid);
        }
    }

    /// Forgets the owners of tabs that are gone, and of sessions that are: `live` is every tab
    /// in the tree and in the sides put away.
    pub fn prune(&mut self, tree: &Tree) {
        let mut live: Vec<TabId> = tree.tabs().iter().filter_map(|(pane, idx, _)| tree.tab_id(*pane, *idx)).collect();
        live.extend(self.kept.values().flat_map(|side| side.tab_ids()));
        self.owner.retain(|tab, session| live.contains(tab) && live.contains(session));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{Side, ViewMode};
    use protocol::SessionId;

    fn term(n: u64) -> Tab {
        Tab::Terminal { session: SessionId(n), cwd: None }
    }

    fn doc(p: &str) -> Tab {
        Tab::Viewer { path: p.into(), mode: ViewMode::Rendered, presenting: false, outline: false }
    }

    fn side_docs(tree: &Tree) -> Vec<Tab> {
        let (sessions, _) = tree.inbox_panes().unwrap();
        tree.panes().into_iter().filter(|p| *p != sessions).flat_map(|p| tree.pane(p).unwrap().tabs.clone()).collect()
    }

    #[test]
    fn choosing_another_session_replaces_every_side_pane_with_that_session_s_own() {
        let mut tree = Tree::new(term(1));
        let sessions = tree.panes()[0];
        tree.add_tab(sessions, term(2));
        tree.activate(sessions, 0);
        let mut sides = Sides::default();
        assert!(sides.keep_shape(&mut tree), "the first frame notes the session shown");
        let (one, two) = (tree.tab_id(sessions, 0).unwrap(), tree.tab_id(sessions, 1).unwrap());
        assert_eq!(sides.shown, Some(one));
        // Session 1 opens two files, the second in a pane of its own.
        tree.add_tab(sessions, doc("/p/a.md"));
        sides.keep_shape(&mut tree);
        let a_pane = tree.inbox_panes().unwrap().1.unwrap();
        tree.split(a_pane, Side::Bottom, doc("/p/b.md")).unwrap();
        assert!(!sides.keep_shape(&mut tree), "a side of two panes is in shape");
        assert_eq!(tree.panes().len(), 3);
        // Choosing session 2: its side is empty, so the sessions take the window.
        tree.activate(sessions, 1);
        assert!(sides.keep_shape(&mut tree));
        assert_eq!(tree.panes(), vec![sessions]);
        assert_eq!((sides.shown, sides.kept.len()), (Some(two), 1));
        // A file opened now is session 2's.
        tree.add_tab(sessions, doc("/p/c.md"));
        sides.keep_shape(&mut tree);
        assert_eq!(side_docs(&tree), vec![doc("/p/c.md")]);
        assert_eq!(tree.pane(sessions).unwrap().active, 1, "still on session 2");
        // Back to session 1: both its panes, as they were, and session 2's file is put away.
        tree.activate(sessions, 0);
        assert!(sides.keep_shape(&mut tree));
        assert_eq!(tree.panes().len(), 3);
        assert_eq!(side_docs(&tree), vec![doc("/p/a.md"), doc("/p/b.md")]);
        assert_eq!(sides.kept.keys().copied().collect::<Vec<_>>(), vec![two]);
        assert!(!sides.keep_shape(&mut tree), "nothing more to do");
    }

    #[test]
    fn the_grid_and_back_keeps_each_session_s_tabs_its_own() {
        // In the Inbox: session 1 with a.md, session 2 with b.md.
        let mut tree = Tree::new(term(1));
        let sessions = tree.panes()[0];
        tree.add_tab(sessions, term(2));
        tree.activate(sessions, 0);
        let mut sides = Sides::default();
        sides.keep_shape(&mut tree);
        tree.add_tab(sessions, doc("/p/a.md"));
        sides.keep_shape(&mut tree);
        tree.activate(sessions, 1);
        sides.keep_shape(&mut tree);
        tree.add_tab(sessions, doc("/p/b.md"));
        sides.keep_shape(&mut tree);
        let (one, two) = (tree.tab_id(sessions, 0).unwrap(), tree.tab_id(sessions, 1).unwrap());
        // To the Grid with no arrangement kept: every tab is in the tree, none lost.
        sides.leave(&mut tree, None);
        let all: Vec<Tab> = tree.tabs().into_iter().map(|(_, _, t)| t.clone()).collect();
        assert!(all.contains(&doc("/p/a.md")) && all.contains(&doc("/p/b.md")) && all.len() == 4);
        assert!(sides.kept.is_empty() && sides.shown.is_none());
        // And back: session 2 is the one shown, with b.md; a.md waits with session 1.
        let grid = sides.enter(&mut tree);
        assert_eq!(grid.tabs().len(), 4, "the grid's arrangement is handed back to be kept");
        assert_eq!(sides.shown, Some(two));
        assert_eq!(side_docs(&tree), vec![doc("/p/b.md")]);
        assert_eq!(sides.kept.get(&one).map(|n| n.tab_ids().len()), Some(1));
        let sessions = tree.inbox_panes().unwrap().0;
        tree.activate(sessions, 0);
        sides.keep_shape(&mut tree);
        assert_eq!(side_docs(&tree), vec![doc("/p/a.md")]);
        // A closed tab's owner is forgotten.
        let side = tree.inbox_panes().unwrap().1.unwrap();
        tree.close_tab(side, 0);
        tree.close_pane_if_empty(side);
        sides.prune(&tree);
        assert_eq!(sides.owner.len(), 1, "only b.md, put away with session 2, is still owned");
    }
    #[test]
    fn a_side_comes_back_on_top_on_a_narrow_window_beside_on_a_wide_one_and_where_the_user_dragged_it_after() {
        // Sessions 1 and 2, each with a file.
        let mut tree = Tree::new(term(1));
        let sessions = tree.panes()[0];
        tree.add_tab(sessions, term(2));
        tree.activate(sessions, 0);
        let mut sides = Sides::default();
        sides.narrow = true;
        tree.add_tab(sessions, doc("/p/a.md"));
        tree.activate(sessions, 0);
        sides.keep_shape(&mut tree);
        assert_eq!(tree.inbox_place().map(|p| p.side), Some(Side::Top), "narrow: a new side on top");
        // To session 2, which opens a file of its own, and back: session 1's side comes back on top.
        tree.activate(sessions, 1);
        sides.keep_shape(&mut tree);
        tree.add_tab(sessions, doc("/p/b.md"));
        tree.activate(sessions, 1);
        sides.keep_shape(&mut tree);
        assert_eq!(tree.inbox_place().map(|p| p.side), Some(Side::Top));
        // The window widens: the side shown stays, the next one to come back goes to the right.
        sides.narrow = false;
        sides.keep_shape(&mut tree);
        assert_eq!(tree.inbox_place().map(|p| p.side), Some(Side::Top), "never moved while shown");
        tree.activate(sessions, 0);
        sides.keep_shape(&mut tree);
        assert_eq!(tree.inbox_place().map(|p| p.side), Some(Side::Right));
        // Dragged below: there it stays, and every side after it goes there, narrow or not.
        assert!(sides.drag_to(&mut tree, Side::Bottom));
        assert_eq!(tree.inbox_place().map(|p| p.side), Some(Side::Bottom));
        sides.narrow = true;
        tree.activate(sessions, 1);
        sides.keep_shape(&mut tree);
        assert_eq!(tree.inbox_place().map(|p| p.side), Some(Side::Bottom));
    }

    #[test]
    fn each_orientation_keeps_its_own_divider_for_every_session() {
        let mut tree = Tree::new(term(1));
        let sessions = tree.panes()[0];
        tree.add_tab(sessions, term(2));
        tree.add_tab(sessions, doc("/p/a.md"));
        tree.activate(sessions, 0);
        let mut sides = Sides::default();
        sides.keep_shape(&mut tree);
        assert_eq!(tree.inbox_place(), Some(SidePlace { side: Side::Right, share: INBOX_RATIO }));
        // The user moves the divider beside, then drags the side on top and moves that one.
        assert!(tree.inbox_set_place(SidePlace { side: Side::Right, share: 0.7 }));
        sides.keep_shape(&mut tree);
        assert!(sides.drag_to(&mut tree, Side::Top));
        assert_eq!(tree.inbox_place(), Some(SidePlace { side: Side::Top, share: INBOX_STACKED }));
        assert!(tree.inbox_set_place(SidePlace { side: Side::Top, share: 0.4 }));
        sides.keep_shape(&mut tree);
        // Back beside, the divider is where it was beside.
        assert!(sides.drag_to(&mut tree, Side::Left));
        assert_eq!(tree.inbox_place(), Some(SidePlace { side: Side::Left, share: 0.7 }));
        assert!((sides.stacked - 0.4).abs() < 1e-6, "stacked, as it was left: {}", sides.stacked);
    }
}
