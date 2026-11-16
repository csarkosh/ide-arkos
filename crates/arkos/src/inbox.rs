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

    /// The layout is switched to the Inbox: the tree is gathered into its shape, the Grid's
    /// arrangement returned to be kept aside, and each session is handed the tabs it owned, the
    /// rest staying with the session shown. The side goes where the Inbox puts sides, even when
    /// the Grid already had the shape (a session beside its files), so the Inbox is its own.
    pub fn enter(&mut self, tree: &mut Tree) -> Tree {
        let grid = tree.clone();
        tree.gather_inbox(self.side_place());
        tree.inbox_set_place(self.side_place());
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

}

/// Makes a tree from before pages into pages (sessions hold grids spec, section 6): a Grid's
/// arrangement is gathered as switching to the Inbox did, each session handed the tabs it owned;
/// then each terminal is a page of its own with its side beside it. A tree in pages is left be.
pub fn into_pages(tree: &mut Tree, mut sides: Sides) {
    if tree.paged {
        return;
    }
    if !tree.is_inbox() {
        sides.enter(tree);
    }
    let shown = selected(tree);
    let focus = tree.focused;
    let mut by_session: HashMap<TabId, (Node, SidePlace)> = HashMap::new();
    if let (Some(s), Some((side, place))) = (shown, tree.inbox_take_side()) {
        sides.remember(place);
        by_session.insert(s, (side, place));
    }
    for (session, side) in std::mem::take(&mut sides.kept) {
        by_session.entry(session).or_insert((side, sides.side_place()));
    }
    tree.into_pages(by_session, shown, focus);
}

/// The reminders of a file from before pages, whose put-off times were in each terminal's marks:
/// each page gets the earliest of its terminals', one-time, and the marks lose theirs.
pub fn reminders_from_marks(tree: &Tree, marks: &mut HashMap<protocol::SessionId, crate::attention::Marks>) -> Vec<(crate::tree::PageKey, crate::reminder::Reminder)> {
    let mut out = Vec::new();
    for page in tree.page_keys() {
        let at = tree
            .page_tabs(page)
            .into_iter()
            .filter_map(|(_, _, tab)| match tab {
                Tab::Terminal { session, .. } => marks.get_mut(session).and_then(|m| m.later_until.take()),
                _ => None,
            })
            .min();
        if let Some(at) = at {
            out.push((page, crate::reminder::Reminder::new(at, crate::reminder::Repeat::Never)));
        }
    }
    for m in marks.values_mut() {
        m.later_until = None;
    }
    out
}

#[cfg(test)]
mod tests {

    #[test]
    fn an_old_put_off_time_becomes_its_page_s_one_time_reminder() {
        let mut tree = Tree::new(term(1));
        let p = tree.panes()[0];
        tree.split(p, Side::Right, term(2));
        let page = tree.page;
        let mut marks: HashMap<protocol::SessionId, crate::attention::Marks> = HashMap::new();
        marks.insert(SessionId(1), crate::attention::Marks { later_until: Some(900), ..Default::default() });
        marks.insert(SessionId(2), crate::attention::Marks { later_until: Some(500), ..Default::default() });
        let got = reminders_from_marks(&tree, &mut marks);
        assert_eq!(got, vec![(page, crate::reminder::Reminder::new(500, crate::reminder::Repeat::Never))]);
        assert!(marks.values().all(|m| m.later_until.is_none()));
    }


    #[test]
    fn an_old_inbox_becomes_a_page_per_session_each_with_its_own_side() {
        // The old shape: the sessions pane, the shown session's side beside it, the other's kept.
        let mut tree = Tree::new(term(1));
        let sessions = tree.panes()[0];
        tree.add_tab(sessions, term(2));
        tree.activate(sessions, 1);
        tree.split(sessions, Side::Right, doc("/p/two.md")).unwrap();
        assert!(tree.is_inbox());
        let mut sides = Sides::default();
        let one = tree.tab_id(sessions, 0).unwrap();
        sides.kept.insert(one, Node::Leaf(crate::tree::Pane { id: 90, tabs: vec![doc("/p/one.md")], active: 0, mru: vec![0], ids: vec![91] }));
        sides.shown = Some(tree.tab_id(sessions, 1).unwrap());
        let focus = tree.focused;
        let two = tree.tab_id(sessions, 1).unwrap();
        let mut old: Tree = serde_json::from_str(&serde_json::to_string(&tree).unwrap().replace("\"paged\":true", "\"paged\":false")).unwrap();
        assert!(!old.paged);
        into_pages(&mut old, sides);
        assert!(old.paged);
        assert_eq!(old.page_keys().len(), 2, "a page for each session");
        assert_eq!(old.page, two, "the session shown is shown, keyed by its terminal");
        assert_eq!(old.panes().len(), 2, "its terminal and its side");
        assert_eq!(old.focused, focus, "focused where it was");
        let tabs = |t: &Tree, key| t.page_tabs(key).into_iter().map(|(_, _, tab)| tab.clone()).collect::<Vec<_>>();
        assert_eq!(tabs(&old, two), vec![term(2), doc("/p/two.md")]);
        let one = old.page_keys().into_iter().find(|k| *k != two).unwrap();
        assert_eq!(tabs(&old, one), vec![term(1), doc("/p/one.md")]);
        // Ids stay unique: a new tab draws past every page's.
        let p = old.panes()[0];
        old.add_tab(p, doc("/p/new.md"));
        let mut ids: Vec<u64> = old.tabs().iter().map(|(pane, idx, _)| old.tab_id(*pane, *idx).unwrap()).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn an_old_grid_becomes_a_page_per_terminal_with_the_tabs_it_owned() {
        let mut tree = Tree::new(term(1));
        let left = tree.panes()[0];
        let right = tree.split(left, Side::Right, term(2)).unwrap();
        tree.add_tab(right, doc("/p/b.md"));
        tree.add_tab(left, doc("/p/a.md"));
        let mut sides = Sides::default();
        let a = tree.tab_id(left, 1).unwrap();
        let first = tree.tab_id(left, 0).unwrap();
        sides.owner.insert(a, first);
        tree.focus(left);
        tree.activate(left, 0);
        let mut old: Tree = serde_json::from_str(&serde_json::to_string(&tree).unwrap().replace("\"paged\":true", "\"paged\":false")).unwrap();
        into_pages(&mut old, sides);
        assert_eq!(old.page_keys().len(), 2);
        let all: Vec<Tab> = old.page_keys().into_iter().flat_map(|k| old.page_tabs(k).into_iter().map(|(_, _, t)| t.clone()).collect::<Vec<_>>()).collect();
        assert_eq!(all.len(), 4, "no tab is lost");
        let mine = old.page_tabs(first).into_iter().map(|(_, _, t)| t.clone()).collect::<Vec<_>>();
        assert!(mine.contains(&doc("/p/a.md")), "the file it owned stays with it: {mine:?}");
    }

    #[test]
    fn a_tree_in_pages_is_left_as_it_is() {
        let mut tree = Tree::new(term(1));
        let p = tree.panes()[0];
        tree.split(p, Side::Right, term(2));
        let before = tree.clone();
        into_pages(&mut tree, Sides::default());
        assert_eq!(tree, before, "two terminals side by side are one session's grid now");
    }

    use super::*;
    use crate::tree::{Side, ViewMode};
    use protocol::SessionId;

    fn term(n: u64) -> Tab {
        Tab::Terminal { session: SessionId(n), cwd: None }
    }

    fn doc(p: &str) -> Tab {
        Tab::Viewer { path: p.into(), mode: ViewMode::Rendered, presenting: false, outline: false }
    }
}
