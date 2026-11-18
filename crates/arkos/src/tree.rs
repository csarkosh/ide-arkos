//! The layout model (spec 4): a binary split tree of panes, each pane an ordered list of tabs.
//! Pure data with no gpui types, so every operation is unit and property tested.


use protocol::SessionId;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub type PaneId = u64;
pub type SplitId = u64;
/// A tab's identity for its whole life, across moves between panes and index shifts. Drawn from
/// the same counter as pane and split ids.
pub type TabId = u64;
/// An Inbox session's page (sessions hold grids spec, section 1): drawn from the same counter, or
/// for a page made from an older file, its first terminal's tab id. 0 is a new window's page.
pub type PageKey = u64;

/// The sessions pane's share of the Inbox's width when it has a side pane.
pub const INBOX_RATIO: f32 = 0.6;
/// The sessions pane's share of the height when the Inbox stacks its side with it, at first.
pub const INBOX_STACKED: f32 = 0.5;
/// A window whose panes' area is narrower than this, in points, is narrow (Inbox stacking spec,
/// section 2): a 13 or 14 inch MacBook with the sidebar shown is, a 16 inch or an external
/// display is not. There Neighbor and the Inbox put a new pane on top instead of to the right.
pub const NARROW_W: f32 = 1500.0;

/// Whether panes' area `w` points wide is narrow (`NARROW_W`). No width yet is not narrow.
pub fn is_narrow(w: f32) -> bool {
    w > 0.0 && w < NARROW_W
}

pub const MIN_PANE_W: f32 = 160.0;
pub const MIN_PANE_H: f32 = 80.0;
/// The gap between two panes at a split: a hairline, painted only while the pointer is over it
/// or drags it (`split_view`). The hit area is wider (the view decides).
pub const HANDLE: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
    /// Children side by side ("split right").
    Horizontal,
    /// Children stacked ("split down").
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub fn orientation(self) -> Orientation {
        match self {
            Side::Left | Side::Right => Orientation::Horizontal,
            Side::Top | Side::Bottom => Orientation::Vertical,
        }
    }

    /// Whether the new pane goes before the existing one in the split.
    pub fn new_pane_first(self) -> bool {
        matches!(self, Side::Left | Side::Top)
    }
}

/// Whether every tab of `p` is a terminal (an empty pane too).
fn all_terminals(p: &Pane) -> bool {
    p.tabs.iter().all(|t| matches!(t, Tab::Terminal { .. }))
}

/// Where the Inbox's side sits (Inbox stacking spec, section 1): on which side of the sessions
/// pane, and the sessions pane's share of the split, whichever of the two comes first in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SidePlace {
    pub side: Side,
    pub share: f32,
}

impl Default for SidePlace {
    fn default() -> Self {
        SidePlace { side: Side::Right, share: INBOX_RATIO }
    }
}

impl SidePlace {
    /// The split that holds the sessions pane and the side in this place.
    fn split(self, id: SplitId, sessions: Node, side: Node) -> Node {
        let (first, second, ratio) = if self.side.new_pane_first() { (side, sessions, 1.0 - self.share) } else { (sessions, side, self.share) };
        Node::Split { id, orientation: self.side.orientation(), ratio, first: Box::new(first), second: Box::new(second) }
    }
}

/// How a markdown viewer shows its document: rendered, with the block under the caret raw, or
/// all of it as raw text. Other viewers ignore it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ViewMode {
    #[default]
    Rendered,
    Raw,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Tab {
    /// `cwd` is the folder its shell was started in when that is not the project (a worktree):
    /// a restart reattaches the session only if the daemon says it lives there. Absent in files
    /// from before milestone 4b, which means the project.
    Terminal {
        session: SessionId,
        #[serde(default)]
        cwd: Option<PathBuf>,
    },
    /// Milestone 4's viewer; a markdown file is editable and keeps its mode. Absent in files from
    /// before milestone 7, which means rendered.
    Viewer {
        path: PathBuf,
        #[serde(default)]
        mode: ViewMode,
        /// Presentation mode is on (reading spec, section 4). Written only when it is.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        presenting: bool,
        /// The outline rail is expanded into its panel (outline rail spec, section 2). Written
        /// only when it is; a new tab starts collapsed.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        outline: bool,
    },
    /// A web tab (web pane spec, section 3.1): the page it shows, kept current as it navigates;
    /// empty is a blank tab. Files from 0.6 wrote `null` for a blank tab and an `app_mode` flag,
    /// which read as blank and nothing.
    Browser {
        #[serde(default, deserialize_with = "url_or_blank")]
        url: String,
    },
    /// A video file, played in the pane. Its saved position (seconds) and loop range come back
    /// with the window; absent means the start and no loop.
    Video {
        path: PathBuf,
        #[serde(default)]
        at: Option<f64>,
        #[serde(default)]
        loop_range: Option<(f64, f64)>,
    },
}

/// A web tab's URL as 0.6 wrote it (`null` for a blank tab) or as it is written now.
fn url_or_blank<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

/// The file a tab shows, if it shows one.
fn tab_path(tab: &Tab) -> Option<&Path> {
    match tab {
        Tab::Viewer { path, .. } | Tab::Video { path, .. } => Some(path),
        Tab::Terminal { .. } | Tab::Browser { .. } => None,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pane {
    pub id: PaneId,
    pub tabs: Vec<Tab>,
    pub active: usize,
    /// Tab indices, most recently activated first. Drives Ctrl+Tab.
    #[serde(default)]
    pub mru: Vec<usize>,
    /// Each tab's id, parallel to `tabs`. A file without them is repaired on load.
    #[serde(default)]
    pub ids: Vec<TabId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Node {
    Leaf(Pane),
    Split { id: SplitId, orientation: Orientation, ratio: f32, first: Box<Node>, second: Box<Node> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tree {
    root: Node,
    next_id: u64,
    pub focused: PaneId,
    pub maximized: Option<PaneId>,
    /// Names the user gave tabs (double-click on the title), by tab id so a name follows its
    /// tab between panes and dies with it. Absent in files from before 0.2.14.
    #[serde(default)]
    pub names: Vec<(TabId, String)>,
    /// The page shown (`root`): the session the window shows.
    #[serde(default)]
    pub page: PageKey,
    /// Every other session's page, as the user left it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pages: Vec<Page>,
    /// Whether the tree is in pages: false for a file from before them, which is made into
    /// pages on load (`inbox::into_pages`). A new tree is.
    #[serde(default)]
    pub paged: bool,
    /// Names the user gave sessions (a row's Rename in Pane 0), by page, apart from the tabs'
    /// own (Cyrus, 2026-11-18: renaming a session "should not rename the tab", and the other way
    /// round).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    session_names: Vec<(PageKey, String)>,
    /// Whether the session names are apart from the tabs': false for a file from before, whose
    /// sessions take their first terminal's tab name once on load (`split_names`). A new tree is.
    #[serde(default)]
    pub names_split: bool,
}

/// A session's page while another is shown: its panes as they were, and where its focus was.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub key: PageKey,
    root: Node,
    focused: PaneId,
    maximized: Option<PaneId>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layout {
    pub panes: Vec<(PaneId, Rect)>,
    /// (split, orientation, handle rect, the split's whole bounds).
    pub handles: Vec<(SplitId, Orientation, Rect, Rect)>,
}

/// The size panes are numbered at. Numbering must not jump while the window resizes, so it comes
/// from a layout at this fixed size instead of the live bounds.
const NOMINAL: Rect = Rect { x: 0.0, y: 0.0, w: 1200.0, h: 800.0 };

const MIN_RATIO: f32 = 0.05;
const MAX_RATIO: f32 = 0.95;

impl Node {
    fn leaves(&self, out: &mut Vec<PaneId>) {
        match self {
            Node::Leaf(p) => out.push(p.id),
            Node::Split { first, second, .. } => {
                first.leaves(out);
                second.leaves(out);
            }
        }
    }

    /// The largest pane, split or tab id in the subtree.
    /// Every pane under the node, in the tree's own order.
    fn panes(&self) -> Vec<&Pane> {
        match self {
            Node::Leaf(p) => vec![p],
            Node::Split { first, second, .. } => first.panes().into_iter().chain(second.panes()).collect(),
        }
    }

    /// The id of every tab under the node.
    pub fn tab_ids(&self) -> Vec<TabId> {
        self.panes().into_iter().flat_map(|p| p.ids.iter().copied()).collect()
    }

    fn max_id(&self) -> u64 {
        match self {
            Node::Leaf(p) => p.ids.iter().copied().chain([p.id]).max().unwrap_or(0),
            Node::Split { id, first, second, .. } => (*id).max(first.max_id()).max(second.max_id()),
        }
    }

    fn find(&self, id: PaneId) -> Option<&Pane> {
        match self {
            Node::Leaf(p) => (p.id == id).then_some(p),
            Node::Split { first, second, .. } => first.find(id).or_else(|| second.find(id)),
        }
    }

    fn find_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        match self {
            Node::Leaf(p) => (p.id == id).then_some(p),
            Node::Split { first, second, .. } => match first.find_mut(id) {
                Some(p) => Some(p),
                None => second.find_mut(id),
            },
        }
    }

    fn find_split_mut(&mut self, id: SplitId) -> Option<&mut Node> {
        if matches!(self, Node::Split { id: sid, .. } if *sid == id) {
            return Some(self);
        }
        match self {
            Node::Leaf(_) => None,
            Node::Split { first, second, .. } => match first.find_split_mut(id) {
                Some(n) => Some(n),
                None => second.find_split_mut(id),
            },
        }
    }

    fn find_split(&self, id: SplitId) -> Option<(Orientation, f32)> {
        match self {
            Node::Leaf(_) => None,
            Node::Split { id: sid, orientation, ratio, first, second } => {
                if *sid == id {
                    return Some((*orientation, *ratio));
                }
                first.find_split(id).or_else(|| second.find_split(id))
            }
        }
    }

    /// Replaces the leaf `id` with a split holding it and a new pane; returns whether it happened.
    fn split_leaf(&mut self, id: PaneId, side: Side, new_pane: Pane, split_id: SplitId) -> bool {
        match self {
            Node::Leaf(p) if p.id == id => {
                let existing = std::mem::replace(self, Node::Leaf(Pane { id: 0, tabs: vec![], active: 0, mru: vec![], ids: vec![] }));
                let new_leaf = Node::Leaf(new_pane);
                let (first, second) = if side.new_pane_first() { (new_leaf, existing) } else { (existing, new_leaf) };
                *self = Node::Split { id: split_id, orientation: side.orientation(), ratio: 0.5, first: Box::new(first), second: Box::new(second) };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { first, second, .. } => first.split_leaf(id, side, new_pane.clone(), split_id) || second.split_leaf(id, side, new_pane, split_id),
        }
    }

    /// Removes leaf `id`, promoting its sibling. Returns the sibling's first pane for focus.
    fn remove_leaf(&mut self, id: PaneId) -> Option<PaneId> {
        if let Node::Split { first, second, .. } = self {
            let (victim_first, victim_second) = (matches!(**first, Node::Leaf(ref p) if p.id == id), matches!(**second, Node::Leaf(ref p) if p.id == id));
            if victim_first || victim_second {
                let keep = if victim_first { std::mem::replace(&mut **second, placeholder()) } else { std::mem::replace(&mut **first, placeholder()) };
                *self = keep;
                let mut leaves = Vec::new();
                self.leaves(&mut leaves);
                return leaves.first().copied();
            }
            return first.remove_leaf(id).or_else(|| second.remove_leaf(id));
        }
        None
    }

    /// Lays the tree out in `bounds`. With `equal` every stored ratio is ignored and taken as 0.5.
    fn layout_into(&self, bounds: Rect, equal: bool, out: &mut Layout) {
        match self {
            Node::Leaf(p) => out.panes.push((p.id, bounds)),
            Node::Split { id, orientation, ratio, first, second } => {
                let ratio = if equal { 0.5 } else { *ratio };
                let (first_b, handle, second_b) = split_rects(bounds, *orientation, ratio, first, second);
                out.handles.push((*id, *orientation, handle, bounds));
                first.layout_into(first_b, equal, out);
                second.layout_into(second_b, equal, out);
            }
        }
    }

    /// The smallest extent this subtree needs along `orientation`.
    fn min_extent(&self, orientation: Orientation) -> f32 {
        match self {
            Node::Leaf(_) => match orientation {
                Orientation::Horizontal => MIN_PANE_W,
                Orientation::Vertical => MIN_PANE_H,
            },
            Node::Split { orientation: o, first, second, .. } => {
                let (a, b) = (first.min_extent(orientation), second.min_extent(orientation));
                if *o == orientation { a + b + HANDLE } else { a.max(b) }
            }
        }
    }
}

/// Orders panes by band (top edge, 0.5 px tolerance), then by left edge.
fn reading_order(panes: &mut [(PaneId, Rect)]) {
    panes.sort_by(|a, b| a.1.y.total_cmp(&b.1.y));
    let mut start = 0;
    while start < panes.len() {
        let band_y = panes[start].1.y;
        let mut end = start + 1;
        while end < panes.len() && panes[end].1.y - band_y <= 0.5 {
            end += 1;
        }
        panes[start..end].sort_by(|a, b| a.1.x.total_cmp(&b.1.x));
        start = end;
    }
}

fn placeholder() -> Node {
    Node::Leaf(Pane { id: 0, tabs: vec![], active: 0, mru: vec![], ids: vec![] })
}

/// Splits `bounds` along `orientation` at `ratio`, leaving a handle between the halves. Each half
/// keeps at least what its subtree needs when the whole has room for both.
fn split_rects(bounds: Rect, orientation: Orientation, ratio: f32, first: &Node, second: &Node) -> (Rect, Rect, Rect) {
    let (extent, min_first, min_second) = (
        match orientation {
            Orientation::Horizontal => bounds.w,
            Orientation::Vertical => bounds.h,
        },
        first.min_extent(orientation),
        second.min_extent(orientation),
    );
    // In a sliver narrower than two handles the handle shrinks so it never outgrows its split.
    // A negative or NaN extent (a window shorter than its chrome) lays out as zero.
    let extent = extent.max(0.0);
    let gap = HANDLE.min(extent / 2.0);
    let total = (extent - gap).max(0.0);
    let wanted = total * ratio;
    // Every clamp below has min <= max: the first because the minimums fit, the second because
    // `total` is never negative.
    let first_len = if total >= min_first + min_second { wanted.clamp(min_first, total - min_second) } else { wanted.clamp(0.0, total) };
    match orientation {
        Orientation::Horizontal => (
            Rect { x: bounds.x, y: bounds.y, w: first_len, h: bounds.h },
            Rect { x: bounds.x + first_len, y: bounds.y, w: gap, h: bounds.h },
            Rect { x: bounds.x + first_len + gap, y: bounds.y, w: total - first_len, h: bounds.h },
        ),
        Orientation::Vertical => (
            Rect { x: bounds.x, y: bounds.y, w: bounds.w, h: first_len },
            Rect { x: bounds.x, y: bounds.y + first_len, w: bounds.w, h: gap },
            Rect { x: bounds.x, y: bounds.y + first_len + gap, w: bounds.w, h: total - first_len },
        ),
    }
}

impl Tree {
    pub fn new(first: Tab) -> Tree {
        let pane = Pane { id: 1, tabs: vec![first], active: 0, mru: vec![0], ids: vec![2] };
        Tree { root: Node::Leaf(pane), next_id: 3, focused: 1, maximized: None, names: Vec::new(), page: 0, pages: Vec::new(), paged: true, session_names: Vec::new(), names_split: true }
    }

    /// One pane with no tabs: what a folder with no saved layout opens on, the empty screen.
    pub fn empty() -> Tree {
        let pane = Pane { id: 1, tabs: Vec::new(), active: 0, mru: Vec::new(), ids: Vec::new() };
        Tree { root: Node::Leaf(pane), next_id: 2, focused: 1, maximized: None, names: Vec::new(), page: 0, pages: Vec::new(), paged: true, session_names: Vec::new(), names_split: true }
    }

    /// Closes `pane` when it has no tabs left, unless it is the last one, which stays and shows
    /// the empty screen. Returns whether the pane went.
    pub fn close_pane_if_empty(&mut self, pane: PaneId) -> bool {
        let empty = self.pane(pane).is_some_and(|p| p.tabs.is_empty());
        empty && self.close_pane(pane)
    }

    fn fresh_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Pane ids in reading order: left to right, top to bottom. This is the Cmd+1 to Cmd+9 order.
    /// The tree laid out with every ratio taken as 0.5, panes in reading order. Numbering comes
    /// from this, so dragging a handle never renumbers panes.
    pub(crate) fn equal_ratio_layout(&self, bounds: Rect) -> Layout {
        let mut out = Layout::default();
        self.root.layout_into(bounds, true, &mut out);
        reading_order(&mut out.panes);
        out
    }

    pub fn panes(&self) -> Vec<PaneId> {
        self.equal_ratio_layout(NOMINAL).panes.into_iter().map(|(id, _)| id).collect()
    }

    /// The pane `id`, on whichever page it is.
    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.root.find(id).or_else(|| self.pages.iter().find_map(|p| p.root.find(id)))
    }

    pub fn pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        if self.root.find(id).is_some() {
            return self.root.find_mut(id);
        }
        self.pages.iter_mut().find_map(|p| p.root.find_mut(id))
    }

    pub fn pane_index(&self, id: PaneId) -> Option<usize> {
        self.panes().iter().position(|p| *p == id)
    }

    pub fn pane_at(&self, index: usize) -> Option<PaneId> {
        self.panes().get(index).copied()
    }

    /// Where the terminal of `session` is, on whichever page.
    pub fn find_session(&self, session: SessionId) -> Option<(PaneId, usize)> {
        for id in self.all_panes() {
            let pane = self.pane(id)?;
            if let Some(i) = pane.tabs.iter().position(|t| matches!(t, Tab::Terminal { session: s, .. } if *s == session)) {
                return Some((id, i));
            }
        }
        None
    }

    /// Every tab of every page: the shown page's first, in reading order.
    pub fn tabs(&self) -> Vec<(PaneId, usize, &Tab)> {
        let mut out = Vec::new();
        for id in self.all_panes() {
            if let Some(pane) = self.pane(id) {
                for (i, t) in pane.tabs.iter().enumerate() {
                    out.push((id, i, t));
                }
            }
        }
        out
    }

    pub fn split(&mut self, pane: PaneId, side: Side, tab: Tab) -> Option<PaneId> {
        self.split_inner(pane, side, None, tab)
    }

    /// Splits `pane` and puts an existing tab in the new pane, keeping the tab's id.
    pub fn split_with_tab(&mut self, pane: PaneId, side: Side, id: TabId, tab: Tab) -> Option<PaneId> {
        self.split_inner(pane, side, Some(id), tab)
    }

    fn split_inner(&mut self, pane: PaneId, side: Side, id: Option<TabId>, tab: Tab) -> Option<PaneId> {
        self.pane(pane)?;
        let new_id = self.fresh_id();
        let split_id = self.fresh_id();
        let tab_id = match id {
            Some(id) => id,
            None => self.fresh_id(),
        };
        let new_pane = Pane { id: new_id, tabs: vec![tab], active: 0, mru: vec![0], ids: vec![tab_id] };
        if !self.root.split_leaf(pane, side, new_pane, split_id) {
            return None;
        }
        self.focused = new_id;
        self.maximized = None;
        Some(new_id)
    }

    pub fn add_tab(&mut self, pane: PaneId, tab: Tab) -> usize {
        let id = self.fresh_id();
        let p = self.pane_mut(pane).expect("pane exists");
        p.tabs.push(tab);
        p.ids.push(id);
        let idx = p.tabs.len() - 1;
        p.active = idx;
        p.mru.retain(|i| *i != idx);
        p.mru.insert(0, idx);
        idx
    }

    /// Sets a viewer tab's mode; any other tab is left alone.
    pub fn set_viewer_mode(&mut self, pane: PaneId, idx: usize, mode: ViewMode) {
        if let Some(Tab::Viewer { mode: m, .. }) = self.pane_mut(pane).and_then(|p| p.tabs.get_mut(idx)) {
            *m = mode;
        }
    }

    /// Turns a viewer tab's presentation mode on or off; any other tab is left alone.
    pub fn set_viewer_presenting(&mut self, pane: PaneId, idx: usize, on: bool) {
        if let Some(Tab::Viewer { presenting, .. }) = self.pane_mut(pane).and_then(|p| p.tabs.get_mut(idx)) {
            *presenting = on;
        }
    }

    /// Expands or collapses a viewer tab's outline; any other tab is left alone.
    pub fn set_viewer_outline(&mut self, pane: PaneId, idx: usize, on: bool) {
        if let Some(Tab::Viewer { outline, .. }) = self.pane_mut(pane).and_then(|p| p.tabs.get_mut(idx)) {
            *outline = on;
        }
    }

    /// Writes a video tab's saved position and loop; any other tab is left alone.
    pub fn set_video_state(&mut self, pane: PaneId, idx: usize, at: Option<f64>, loop_range: Option<(f64, f64)>) {
        if let Some(Tab::Video { at: a, loop_range: l, .. }) = self.pane_mut(pane).and_then(|p| p.tabs.get_mut(idx)) {
            *a = at;
            *l = loop_range;
        }
    }

    /// A web tab's page changed (a navigation committed): the tab keeps the new URL.
    pub fn set_browser_url(&mut self, tab: TabId, url: &str) -> bool {
        let Some((pane, idx)) = self.locate(tab) else { return false };
        match self.pane_mut(pane).and_then(|p| p.tabs.get_mut(idx)) {
            Some(Tab::Browser { url: u }) if u != url => {
                *u = url.to_string();
                true
            }
            _ => false,
        }
    }

    /// Swaps a tab's content in place; the tab keeps its id.
    pub fn replace_tab(&mut self, pane: PaneId, idx: usize, tab: Tab) {
        if let Some(p) = self.pane_mut(pane) {
            if idx < p.tabs.len() {
                p.tabs[idx] = tab;
            }
        }
    }

    pub fn activate(&mut self, pane: PaneId, idx: usize) {
        self.activate_quiet(pane, idx);
        self.focus(pane);
    }

    /// Makes a tab its pane's active one (and the most recent) without moving the focus or
    /// leaving a maximize: for a tab opened on another's behalf (a poster's Cmd+click, an
    /// agent's open) while the keys stay where they are.
    pub fn activate_quiet(&mut self, pane: PaneId, idx: usize) {
        if let Some(p) = self.pane_mut(pane) {
            if idx < p.tabs.len() {
                p.active = idx;
                p.mru.retain(|i| *i != idx);
                p.mru.insert(0, idx);
            }
        }
    }

    /// Focuses a pane. Focusing a pane hidden by another's maximize exits the maximize, so the
    /// focused pane is always on screen and typing always lands somewhere visible.
    /// On a kept page, that page is shown first.
    pub fn focus(&mut self, pane: PaneId) {
        let Some(key) = self.page_of(pane) else { return };
        self.show_page(key);
        if self.maximized.is_some_and(|m| m != pane) {
            self.maximized = None;
        }
        self.focused = pane;
    }

    /// The tab Ctrl+Tab would switch to: the most recently used one that is not active.
    pub fn mru_next(&self, pane: PaneId) -> Option<usize> {
        let p = self.pane(pane)?;
        p.mru.iter().copied().find(|i| *i != p.active && *i < p.tabs.len())
    }

    /// Removes a tab; the pane stays even when it empties (the empty state).
    pub fn close_tab(&mut self, pane: PaneId, idx: usize) -> Option<Tab> {
        let (tab, id) = self.remove_tab(pane, idx)?;
        // A closed tab's name goes with it; a reopened one starts from its title again.
        self.names.retain(|(t, _)| *t != id);
        Some(tab)
    }

    /// The name the user gave a tab, if any; the automatic title shows otherwise.
    pub fn tab_name(&self, id: TabId) -> Option<&str> {
        self.names.iter().find(|(t, _)| *t == id).map(|(_, n)| n.as_str())
    }

    /// Sets a tab's name; blank clears it, so the automatic title shows again.
    pub fn set_tab_name(&mut self, id: TabId, name: Option<String>) {
        self.names.retain(|(t, _)| *t != id);
        if let Some(n) = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
            self.names.push((id, n));
        }
    }

    /// The name the user gave the session on page `page`, if any; its automatic name shows
    /// otherwise.
    pub fn session_name(&self, page: PageKey) -> Option<&str> {
        self.session_names.iter().find(|(p, _)| *p == page).map(|(_, n)| n.as_str())
    }

    /// Sets a session's name; blank clears it, so the automatic name shows again.
    pub fn set_session_name(&mut self, page: PageKey, name: Option<String>) {
        self.session_names.retain(|(p, _)| *p != page);
        if let Some(n) = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
            self.session_names.push((page, n));
        }
    }

    /// For a layout from before session names (`names_split` false): each page takes its first
    /// terminal's tab name as its session's, once, so no name the user gave changes. Names of
    /// pages no longer here go.
    pub fn split_names(&mut self) {
        let keys = self.page_keys();
        self.session_names.retain(|(p, _)| keys.contains(p));
        if self.names_split {
            return;
        }
        for page in keys {
            let first = self.page_tabs(page).into_iter().filter(|(_, _, t)| matches!(t, Tab::Terminal { .. })).filter_map(|(pane, idx, _)| self.tab_id(pane, idx)).min();
            if let Some(name) = first.and_then(|id| self.tab_name(id)).map(str::to_string) {
                if self.session_name(page).is_none() {
                    self.session_names.push((page, name));
                }
            }
        }
        self.names_split = true;
    }

    /// Removes a tab for moving elsewhere, with its id so the move keeps it. Same removal as
    /// `close_tab`; the name says the intent.
    pub fn take_tab(&mut self, pane: PaneId, idx: usize) -> Option<(TabId, Tab)> {
        self.remove_tab(pane, idx).map(|(tab, id)| (id, tab))
    }

    /// Removes a tab and returns it with its id, for moves that keep the id.
    fn remove_tab(&mut self, pane: PaneId, idx: usize) -> Option<(Tab, TabId)> {
        let p = self.pane_mut(pane)?;
        if idx >= p.tabs.len() {
            return None;
        }
        let tab = p.tabs.remove(idx);
        let id = p.ids.remove(idx);
        p.mru.retain(|i| *i != idx);
        for i in p.mru.iter_mut() {
            if *i > idx {
                *i -= 1;
            }
        }
        if p.tabs.is_empty() {
            p.active = 0;
        } else if p.active > idx || p.active == p.tabs.len() {
            p.active -= 1;
        }
        Some((tab, id))
    }

    /// Every page's key, the shown one's included, in the order they were made.
    pub fn page_keys(&self) -> Vec<PageKey> {
        let mut keys: Vec<PageKey> = self.pages.iter().map(|p| p.key).chain([self.page]).collect();
        keys.sort_unstable();
        keys
    }

    /// The page `pane` is on.
    pub fn page_of(&self, pane: PaneId) -> Option<PageKey> {
        if self.root.find(pane).is_some() {
            return Some(self.page);
        }
        self.pages.iter().find(|p| p.root.find(pane).is_some()).map(|p| p.key)
    }

    /// A page's root, the shown one's or a kept one's.
    fn page_root(&self, key: PageKey) -> Option<&Node> {
        if key == self.page {
            return Some(&self.root);
        }
        self.pages.iter().find(|p| p.key == key).map(|p| &p.root)
    }

    /// A page's panes in reading order.
    pub fn page_panes(&self, key: PageKey) -> Vec<PaneId> {
        let Some(root) = self.page_root(key) else { return Vec::new() };
        let mut out = Layout::default();
        root.layout_into(NOMINAL, true, &mut out);
        reading_order(&mut out.panes);
        out.panes.into_iter().map(|(id, _)| id).collect()
    }

    /// A page's tabs, pane by pane in reading order.
    pub fn page_tabs(&self, key: PageKey) -> Vec<(PaneId, usize, &Tab)> {
        let mut out = Vec::new();
        for id in self.page_panes(key) {
            if let Some(pane) = self.pane(id) {
                out.extend(pane.tabs.iter().enumerate().map(|(i, t)| (id, i, t)));
            }
        }
        out
    }

    /// The pane focused in a page: where the keys go when it is shown.
    pub fn page_focused(&self, key: PageKey) -> Option<PaneId> {
        if key == self.page {
            return Some(self.focused);
        }
        self.pages.iter().find(|p| p.key == key).map(|p| p.focused)
    }

    /// Shows page `key`, keeping the one shown as it is. False when there is no such page.
    /// The page shown goes when it has no tabs left, since nothing would list it.
    pub fn show_page(&mut self, key: PageKey) -> bool {
        self.swap_page(key, false)
    }

    fn swap_page(&mut self, key: PageKey, keep_empty: bool) -> bool {
        if key == self.page {
            return true;
        }
        let Some(at) = self.pages.iter().position(|p| p.key == key) else { return false };
        let next = self.pages.remove(at);
        self.hide_shown(keep_empty);
        self.root = next.root;
        self.focused = next.focused;
        self.maximized = next.maximized;
        self.page = next.key;
        true
    }

    /// Puts the page shown among the kept ones, unless it has no tabs and `keep_empty` is not
    /// asked for. The root is left for the caller to replace.
    fn hide_shown(&mut self, keep_empty: bool) {
        let empty = self.root.panes().iter().all(|p| p.tabs.is_empty());
        if empty && !keep_empty {
            return;
        }
        let root = std::mem::replace(&mut self.root, placeholder());
        self.pages.push(Page { key: self.page, root, focused: self.focused, maximized: self.maximized });
    }

    /// Starts a page holding `first` alone and shows it: a new session. Returns its pane.
    pub fn new_page(&mut self, first: Tab) -> PaneId {
        let pane = self.blank_page();
        self.add_tab(pane, first);
        pane
    }

    /// Starts a page of one pane with no tab yet and shows it, for a tab to be added to at once.
    /// The empty screen shown is used as it is.
    pub fn blank_page(&mut self) -> PaneId {
        if self.root.panes().iter().all(|p| p.tabs.is_empty()) {
            let pane = self.root.panes()[0].id;
            self.root = Node::Leaf(Pane { id: pane, tabs: Vec::new(), active: 0, mru: Vec::new(), ids: Vec::new() });
            self.focused = pane;
            self.maximized = None;
            return pane;
        }
        self.hide_shown(false);
        let (key, pane) = (self.fresh_id(), self.fresh_id());
        self.root = Node::Leaf(Pane { id: pane, tabs: Vec::new(), active: 0, mru: Vec::new(), ids: Vec::new() });
        self.page = key;
        self.focused = pane;
        self.maximized = None;
        pane
    }

    /// Shows page `key` for a moment's work on it, keeping the page shown even when it has no
    /// tabs, so `show_page_for` can bring it back.
    pub fn show_page_for(&mut self, key: PageKey) -> bool {
        self.swap_page(key, true)
    }

    /// Runs `f` with page `key` shown, then shows again the page that was, focused as it was:
    /// work on a session's page while the user looks at another, which nothing on screen sees.
    pub fn within<R>(&mut self, key: PageKey, f: impl FnOnce(&mut Tree) -> R) -> R {
        let was = self.page;
        self.swap_page(key, true);
        let out = f(self);
        self.swap_page(was, true);
        out
    }

    /// Closes page `key` and every tab on it. When it is the page shown, `then` is shown in its
    /// place, else the empty screen.
    pub fn close_page(&mut self, key: PageKey, then: Option<PageKey>) {
        self.session_names.retain(|(p, _)| *p != key);
        if key != self.page {
            self.pages.retain(|p| p.key != key);
            return;
        }
        let gone = self.page_tabs(key).iter().filter_map(|(pane, idx, _)| self.tab_id(*pane, *idx)).collect::<Vec<_>>();
        self.names.retain(|(t, _)| !gone.contains(t));
        let next = then.filter(|k| *k != key).and_then(|k| self.pages.iter().position(|p| p.key == k)).map(|at| self.pages.remove(at));
        match next {
            Some(p) => {
                self.root = p.root;
                self.focused = p.focused;
                self.maximized = p.maximized;
                self.page = p.key;
            }
            None => {
                let (k, pane) = (self.fresh_id(), self.fresh_id());
                self.root = Node::Leaf(Pane { id: pane, tabs: Vec::new(), active: 0, mru: Vec::new(), ids: Vec::new() });
                self.focused = pane;
                self.maximized = None;
                self.page = k;
            }
        }
    }

    /// Makes the Inbox's old shape into pages (sessions hold grids spec, section 6), the side
    /// already taken out (`inbox_take_side`), so the root is the sessions pane alone: each of
    /// its terminals becomes a page of its own, keyed by its tab, with its side from `sides`
    /// beside it where that side's place says. `shown`'s page is shown, focused on `focus` when
    /// that pane is in it, else on its terminal. A tree not in that shape is only marked paged.
    pub fn into_pages(&mut self, mut sides: std::collections::HashMap<TabId, (Node, SidePlace)>, shown: Option<TabId>, focus: PaneId) {
        self.paged = true;
        let Node::Leaf(sessions) = &self.root else { return };
        if sessions.tabs.is_empty() || !all_terminals(sessions) {
            return;
        }
        let sessions = sessions.clone();
        let top = sides.values().map(|(n, _)| n.max_id()).fold(self.root.max_id(), u64::max);
        self.next_id = self.next_id.max(top + 1);
        let shown = shown.filter(|s| sessions.ids.contains(s)).unwrap_or(sessions.ids[sessions.active.min(sessions.ids.len() - 1)]);
        let mut made = Vec::new();
        for (tab, id) in sessions.tabs.into_iter().zip(sessions.ids) {
            let pane_id = if id == shown { sessions.id } else { self.fresh_id() };
            let pane = Node::Leaf(Pane { id: pane_id, tabs: vec![tab], active: 0, mru: vec![0], ids: vec![id] });
            let root = match sides.remove(&id) {
                Some((side, place)) => {
                    let split = self.fresh_id();
                    place.split(split, pane, side)
                }
                None => pane,
            };
            let focused = if id == shown && root.find(focus).is_some() { focus } else { pane_id };
            made.push(Page { key: id, root, focused, maximized: None });
        }
        let at = made.iter().position(|p| p.key == shown).unwrap_or(0);
        let first = made.remove(at);
        self.root = first.root;
        self.focused = first.focused;
        self.maximized = None;
        self.page = first.key;
        self.pages = made;
    }

    /// Every pane of every page: the shown page's in reading order, then each kept page's.
    fn all_panes(&self) -> Vec<PaneId> {
        let mut out = self.panes();
        for p in &self.pages {
            out.extend(self.page_panes(p.key));
        }
        out
    }

    /// Whether the tree has the Inbox layout's shape (layouts spec, "The Inbox layout"; Inbox
    /// stacking spec, section 1): one pane holding only terminal tabs, the sessions, alone or
    /// with a **side** on any of its four sides: one pane or several, none of them empty and none
    /// holding a terminal. The side belongs to the session shown, and the window swaps it as
    /// another is chosen (`inbox_take_side`).
    pub fn is_inbox(&self) -> bool {
        match &self.root {
            Node::Leaf(p) => all_terminals(p),
            Node::Split { .. } => self.inbox_sessions_first().is_some(),
        }
    }

    /// With the root a split in the Inbox's shape, whether the sessions pane is its first child.
    fn inbox_sessions_first(&self) -> Option<bool> {
        let others = |n: &Node| n.panes().into_iter().all(|p| !p.tabs.is_empty() && p.tabs.iter().all(|t| !matches!(t, Tab::Terminal { .. })));
        let sessions = |n: &Node| matches!(n, Node::Leaf(a) if all_terminals(a));
        let Node::Split { first, second, .. } = &self.root else { return None };
        if sessions(first) && others(second) {
            Some(true)
        } else if sessions(second) && others(first) {
            Some(false)
        } else {
            None
        }
    }

    /// Where the Inbox's side sits, while the tree has the shape and a side.
    pub fn inbox_place(&self) -> Option<SidePlace> {
        let sessions_first = self.inbox_sessions_first()?;
        let Node::Split { orientation, ratio, .. } = &self.root else { return None };
        let side = match (orientation, sessions_first) {
            (Orientation::Horizontal, true) => Side::Right,
            (Orientation::Horizontal, false) => Side::Left,
            (Orientation::Vertical, true) => Side::Bottom,
            (Orientation::Vertical, false) => Side::Top,
        };
        Some(SidePlace { side, share: if sessions_first { *ratio } else { 1.0 - *ratio } })
    }

    /// The Inbox's panes while the tree has its shape: the sessions pane, and the side pane a
    /// new tab goes to when there is a side: the focused pane when it is one of the side's, else
    /// the side's first.
    pub fn inbox_panes(&self) -> Option<(PaneId, Option<PaneId>)> {
        if !self.is_inbox() {
            return None;
        }
        match &self.root {
            Node::Leaf(p) => Some((p.id, None)),
            Node::Split { first, second, .. } => {
                let (sessions, others) = if self.inbox_sessions_first()? { (first, second) } else { (second, first) };
                let Node::Leaf(sessions) = &**sessions else { return None };
                let side: Vec<PaneId> = others.panes().into_iter().map(|p| p.id).collect();
                Some((sessions.id, side.iter().copied().find(|id| *id == self.focused).or(side.first().copied())))
            }
        }
    }

    /// Lifts the Inbox's side out of the tree, leaving the sessions pane alone: the side as it
    /// is, panes, tabs, ids and all, and where it sat. None when there is no side. The focus goes
    /// to the sessions when it was in the side.
    pub fn inbox_take_side(&mut self) -> Option<(Node, SidePlace)> {
        let place = self.inbox_place()?;
        let sessions_first = !place.side.new_pane_first();
        let Node::Split { first, second, .. } = std::mem::replace(&mut self.root, placeholder()) else { unreachable!("checked above") };
        let (sessions, side) = if sessions_first { (first, second) } else { (second, first) };
        self.root = *sessions;
        let sessions = self.panes()[0];
        if self.pane(self.focused).is_none() {
            self.focused = sessions;
        }
        if self.maximized.is_some_and(|m| self.pane(m).is_none()) {
            self.maximized = None;
        }
        Some((*side, place))
    }

    /// Puts `side` by the sessions pane, in `place`, the sessions pane alone (the tree in the
    /// Inbox's shape with no side): the reverse of `inbox_take_side`. False, with nothing
    /// changed, otherwise.
    pub fn inbox_put_side(&mut self, side: Node, place: SidePlace) -> bool {
        if !self.is_inbox() || !matches!(self.root, Node::Leaf(_)) {
            return false;
        }
        self.next_id = self.next_id.max(side.max_id() + 1);
        let id = self.fresh_id();
        let sessions = std::mem::replace(&mut self.root, placeholder());
        self.root = place.split(id, sessions, side);
        true
    }

    /// Moves the Inbox's side to `place`, its panes and their arrangement as they are, the focus
    /// where it was. False when there is no side.
    pub fn inbox_set_place(&mut self, place: SidePlace) -> bool {
        let focused = self.focused;
        let Some((side, _)) = self.inbox_take_side() else { return false };
        self.inbox_put_side(side, place);
        if self.pane(focused).is_some() {
            self.focused = focused;
        }
        true
    }

    /// Takes the tabs with these ids out of the Inbox's side and returns them as one pane, to be
    /// kept as another session's side; a side pane left empty goes. None when it holds none.
    pub fn inbox_split_off(&mut self, ids: &[TabId]) -> Option<Node> {
        let (sessions, _) = self.inbox_panes()?;
        let mut taken: Vec<(Tab, TabId)> = Vec::new();
        for pane in self.panes().into_iter().filter(|p| *p != sessions) {
            let mut idx = 0;
            while let Some(id) = self.tab_id(pane, idx) {
                if ids.contains(&id) {
                    taken.extend(self.remove_tab(pane, idx));
                } else {
                    idx += 1;
                }
            }
            self.close_pane_if_empty(pane);
        }
        if taken.is_empty() {
            return None;
        }
        let id = self.fresh_id();
        let mru = (0..taken.len()).collect();
        Some(Node::Leaf(Pane { id, ids: taken.iter().map(|t| t.1).collect(), tabs: taken.into_iter().map(|t| t.0).collect(), active: 0, mru }))
    }

    /// Puts any tree in the Inbox's shape, as the layout is switched to: keeping every tab with
    /// its id and its name, every terminal tab in one pane, in reading order, and every other
    /// tab in one pane to its right
    /// (none when there is no such tab). Each pane shows the tab the user was on: the tab of its
    /// kind used last in the focused pane, else the first tab that was active in its pane. The focus stays on the kind of tab it was on. Pane ids are kept where they
    /// can be, so what is kept per pane stays with its tabs. True when the tree changed.
    pub fn gather_inbox(&mut self, place: SidePlace) -> bool {
        if self.is_inbox() {
            return false;
        }
        // (the tab, its id, how recently it was used in its pane with 0 the active tab, its pane)
        let (mut terms, mut others): (Vec<(Tab, TabId, usize, PaneId)>, Vec<(Tab, TabId, usize, PaneId)>) = (Vec::new(), Vec::new());
        for id in self.panes() {
            let Some(p) = self.pane(id) else { continue };
            for (i, (tab, tab_id)) in p.tabs.iter().zip(&p.ids).enumerate() {
                let rank = if i == p.active { 0 } else { p.mru.iter().position(|m| *m == i).map_or(usize::MAX, |at| at + 1) };
                let item = (tab.clone(), *tab_id, rank, id);
                if matches!(tab, Tab::Terminal { .. }) {
                    terms.push(item);
                } else {
                    others.push(item);
                }
            }
        }
        let focused = self.focused;
        let on_other = others.iter().any(|(_, _, rank, pane)| *rank == 0 && *pane == focused);
        let sessions_id = terms.first().map(|t| t.3).unwrap_or_else(|| self.panes()[0]);
        let side_id = others.iter().map(|t| t.3).find(|id| *id != sessions_id).unwrap_or_else(|| self.fresh_id());
        let pane_of = |id: PaneId, items: Vec<(Tab, TabId, usize, PaneId)>| {
            // The tab of this kind used last in the focused pane (so a file opened among the
            // sessions leaves the session the user was on showing), else the first that was its
            // pane's active tab.
            let in_focused = items.iter().enumerate().filter(|(_, t)| t.3 == focused).min_by_key(|(_, t)| t.2).map(|(i, _)| i);
            let active = in_focused.or_else(|| items.iter().position(|t| t.2 == 0)).unwrap_or(0);
            let mru: Vec<usize> = if items.is_empty() { Vec::new() } else { std::iter::once(active).chain((0..items.len()).filter(|i| *i != active)).collect() };
            Pane { id, ids: items.iter().map(|t| t.1).collect(), tabs: items.into_iter().map(|t| t.0).collect(), active, mru }
        };
        let has_side = !others.is_empty();
        let sessions = pane_of(sessions_id, terms);
        self.root = if has_side {
            let id = match &self.root {
                Node::Split { id, .. } => *id,
                Node::Leaf(_) => self.fresh_id(),
            };
            place.split(id, Node::Leaf(sessions), Node::Leaf(pane_of(side_id, others)))
        } else {
            Node::Leaf(sessions)
        };
        self.focused = if on_other && has_side { side_id } else { sessions_id };
        self.maximized = None;
        true
    }

    /// Moves pane `pane`, tabs and all, to the `side` of pane `to` (Inbox stacking spec, section
    /// 3): its old place goes to its sibling, and the two share `to`'s space half and half. The
    /// moved pane is focused. False, with nothing changed, onto itself or a pane that is not.
    pub fn move_pane(&mut self, pane: PaneId, to: PaneId, side: Side) -> bool {
        if pane == to || self.pane(to).is_none() {
            return false;
        }
        let Some(moved) = self.pane(pane).cloned() else { return false };
        let before = self.clone();
        if !self.close_pane(pane) {
            return false;
        }
        let split_id = self.fresh_id();
        if !self.root.split_leaf(to, side, moved, split_id) {
            *self = before;
            return false;
        }
        self.focused = pane;
        self.maximized = None;
        true
    }

    /// Moves every tab of pane `pane` into pane `to`, in order, at `at` (appended with None),
    /// and closes `pane`; `to` shows the tab `pane` showed. False onto itself or a missing pane.
    pub fn merge_pane(&mut self, pane: PaneId, to: PaneId, at: Option<usize>) -> bool {
        if pane == to || self.pane(to).is_none() {
            return false;
        }
        let Some(p) = self.pane(pane).cloned() else { return false };
        let start = at.unwrap_or(usize::MAX).min(self.pane(to).map_or(0, |t| t.tabs.len()));
        // Moved one by one: the first tab of `pane` to `start`, the next after it, and so on.
        for i in 0..p.tabs.len() {
            if !self.move_tab(pane, 0, to, Some(start + i)) {
                return false;
            }
        }
        if let Some(shown) = p.ids.get(p.active).copied() {
            if let Some(idx) = self.pane(to).and_then(|t| t.ids.iter().position(|id| *id == shown)) {
                self.activate(to, idx);
            }
        }
        if self.pane(pane).is_some_and(|q| q.tabs.is_empty()) {
            self.close_pane(pane);
        }
        self.focused = to;
        true
    }

    /// The id of the tab at `idx` in `pane`.
    pub fn tab_id(&self, pane: PaneId, idx: usize) -> Option<TabId> {
        self.pane(pane)?.ids.get(idx).copied()
    }

    /// The first tab in `pane` on `path`, whatever its kind.
    pub fn find_path(&self, pane: PaneId, path: &Path) -> Option<usize> {
        self.pane(pane)?.tabs.iter().position(|t| tab_path(t) == Some(path))
    }

    /// Every tab on `path` in the tree, in reading order.
    pub fn tabs_on(&self, path: &Path) -> Vec<(PaneId, usize)> {
        self.tabs().into_iter().filter(|(_, _, t)| tab_path(t) == Some(path)).map(|(pane, idx, _)| (pane, idx)).collect()
    }

    /// The tabs on `dir` or on anything inside it (a file or folder being deleted), as ids, since
    /// closing one moves the others' indices.
    pub fn tabs_within(&self, dir: &Path) -> Vec<TabId> {
        self.tabs()
            .into_iter()
            .filter(|(_, _, t)| tab_path(t).is_some_and(|p| p.starts_with(dir)))
            .filter_map(|(pane, idx, _)| self.tab_id(pane, idx))
            .collect()
    }

    /// Where a tab is now: its pane and index. `None` once it has closed.
    pub fn locate(&self, tab: TabId) -> Option<(PaneId, usize)> {
        self.all_panes()
            .into_iter().find_map(|id| self.pane(id)?.ids.iter().position(|t| *t == tab).map(|i| (id, i)))
    }

    /// Gives every tab an id when a file was written without them (or with a damaged list), and
    /// moves the id counter past every id in use.
    pub fn repair_ids(&mut self) {
        let top = self.pages.iter().map(|p| p.root.max_id().max(p.key)).fold(self.root.max_id().max(self.page), u64::max);
        self.next_id = self.next_id.max(top + 1);
        let panes = self.all_panes();
        let mut seen = std::collections::HashSet::new();
        for pane in panes {
            let (n, ids) = {
                let p = self.pane(pane).expect("listed pane exists");
                (p.tabs.len(), p.ids.clone())
            };
            let mut fixed = Vec::with_capacity(n);
            for i in 0..n {
                match ids.get(i) {
                    Some(id) if ids.len() == n && seen.insert(*id) => fixed.push(*id),
                    _ => {
                        let id = self.fresh_id();
                        seen.insert(id);
                        fixed.push(id);
                    }
                }
            }
            self.pane_mut(pane).expect("listed pane exists").ids = fixed;
        }
    }

    /// Removes a pane and its tabs. The last pane never closes. A kept page's pane closes in
    /// that page, which is not shown for it.
    pub fn close_pane(&mut self, pane: PaneId) -> bool {
        if let Some(key) = self.page_of(pane).filter(|k| *k != self.page) {
            return self.within(key, |t| t.close_pane(pane));
        }
        if matches!(self.root, Node::Leaf(_)) {
            return false;
        }
        let Some(next) = self.root.remove_leaf(pane) else { return false };
        if self.focused == pane {
            self.focused = next;
        }
        if self.maximized == Some(pane) {
            self.maximized = None;
        }
        true
    }

    /// Moves a tab to another pane (or another position in the same pane). `at` is the index to
    /// insert at in the target's list before removal is accounted for; `None` appends.
    pub fn move_tab(&mut self, from: PaneId, idx: usize, to: PaneId, at: Option<usize>) -> bool {
        let n_from = match self.pane(from) {
            Some(p) if idx < p.tabs.len() => p.tabs.len(),
            _ => return false,
        };
        if self.pane(to).is_none() {
            return false;
        }
        if from == to {
            let target = at.unwrap_or(n_from);
            let landing = if target > idx { target - 1 } else { target };
            if landing == idx {
                return true;
            }
            let (tab, id) = self.remove_tab(from, idx).expect("tab exists");
            let p = self.pane_mut(to).expect("pane exists");
            let landing = landing.min(p.tabs.len());
            p.tabs.insert(landing, tab);
            p.ids.insert(landing, id);
            for i in p.mru.iter_mut() {
                if *i >= landing {
                    *i += 1;
                }
            }
            p.active = landing;
            p.mru.retain(|i| *i != landing);
            p.mru.insert(0, landing);
            return true;
        }
        let (tab, id) = self.remove_tab(from, idx).expect("tab exists");
        let p = self.pane_mut(to).expect("pane exists");
        let landing = at.unwrap_or(p.tabs.len()).min(p.tabs.len());
        p.tabs.insert(landing, tab);
        p.ids.insert(landing, id);
        for i in p.mru.iter_mut() {
            if *i >= landing {
                *i += 1;
            }
        }
        p.active = landing;
        p.mru.retain(|i| *i != landing);
        p.mru.insert(0, landing);
        self.focus(to);
        true
    }

    /// Moves a tab left (negative) or right within its pane.
    pub fn move_tab_within(&mut self, pane: PaneId, idx: usize, delta: isize) -> bool {
        let n = match self.pane(pane) {
            Some(p) => p.tabs.len(),
            None => return false,
        };
        let target = idx as isize + delta;
        if idx >= n || target < 0 || target as usize >= n {
            return false;
        }
        let p = self.pane_mut(pane).expect("pane exists");
        let tab = p.tabs.remove(idx);
        p.tabs.insert(target as usize, tab);
        let id = p.ids.remove(idx);
        p.ids.insert(target as usize, id);
        let remap = |i: usize| -> usize {
            if i == idx {
                target as usize
            } else if delta > 0 && i > idx && i <= target as usize {
                i - 1
            } else if delta < 0 && i >= target as usize && i < idx {
                i + 1
            } else {
                i
            }
        };
        p.active = remap(p.active);
        for i in p.mru.iter_mut() {
            *i = remap(*i);
        }
        true
    }

    pub fn set_ratio(&mut self, split: SplitId, ratio: f32) {
        if !ratio.is_finite() {
            return;
        }
        if let Some(Node::Split { ratio: r, .. }) = self.root.find_split_mut(split) {
            *r = ratio.clamp(MIN_RATIO, MAX_RATIO);
        }
    }

    pub fn reset_ratio(&mut self, split: SplitId) {
        self.set_ratio(split, 0.5);
    }

    pub fn split_of(&self, split: SplitId) -> Option<(Orientation, f32)> {
        self.root.find_split(split)
    }

    pub fn toggle_maximize(&mut self, pane: PaneId) {
        if self.pane(pane).is_none() {
            return;
        }
        self.maximized = if self.maximized == Some(pane) { None } else { Some(pane) };
        self.focused = pane;
    }

    /// Pane and handle rectangles for `bounds`. A maximized pane takes everything.
    pub fn layout(&self, bounds: Rect) -> Layout {
        let mut out = Layout::default();
        if let Some(m) = self.maximized {
            if self.pane(m).is_some() {
                out.panes.push((m, bounds));
                return out;
            }
        }
        self.root.layout_into(bounds, false, &mut out);
        let rank: std::collections::HashMap<PaneId, usize> = self.panes().into_iter().enumerate().map(|(i, id)| (id, i)).collect();
        out.panes.sort_by_key(|(id, _)| rank[id]);
        out
    }

    /// The pane in direction `dir` from `pane`: the one across the gap with the most overlap along
    /// the other axis, nearest first.
    pub fn neighbor(&self, pane: PaneId, dir: Side, bounds: Rect) -> Option<PaneId> {
        let mut t = self.clone();
        t.maximized = None;
        let l = t.layout(bounds);
        let cur = l.panes.iter().find(|(id, _)| *id == pane)?.1;
        let mut best: Option<(PaneId, f32, f32)> = None;
        for (id, r) in &l.panes {
            if *id == pane {
                continue;
            }
            let (across, distance, overlap) = match dir {
                Side::Left => (r.x + r.w <= cur.x + HANDLE + 0.5, cur.x - (r.x + r.w), overlap(r.y, r.h, cur.y, cur.h)),
                Side::Right => (r.x >= cur.x + cur.w - HANDLE - 0.5, r.x - (cur.x + cur.w), overlap(r.y, r.h, cur.y, cur.h)),
                Side::Top => (r.y + r.h <= cur.y + HANDLE + 0.5, cur.y - (r.y + r.h), overlap(r.x, r.w, cur.x, cur.w)),
                Side::Bottom => (r.y >= cur.y + cur.h - HANDLE - 0.5, r.y - (cur.y + cur.h), overlap(r.x, r.w, cur.x, cur.w)),
            };
            if !across || overlap <= 0.0 {
                continue;
            }
            let better = match best {
                None => true,
                Some((_, d, o)) => distance < d - 0.5 || ((distance - d).abs() <= 0.5 && overlap > o),
            };
            if better {
                best = Some((*id, distance, overlap));
            }
        }
        best.map(|(id, _, _)| id)
    }
}

fn overlap(a: f32, aw: f32, b: f32, bw: f32) -> f32 {
    (a + aw).min(b + bw) - a.max(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_path_locates_a_tab_by_path_in_one_pane() {
        let mut t = Tree::new(Tab::Terminal { session: SessionId(1), cwd: None });
        let root = t.panes()[0];
        t.add_tab(root, Tab::Viewer { path: "/p/a.rs".into(), mode: ViewMode::default(), presenting: false, outline: false });
        let right = t.split(root, Side::Right, Tab::Viewer { path: "/p/b.rs".into(), mode: ViewMode::default(), presenting: false, outline: false }).unwrap();
        assert_eq!(t.find_path(root, Path::new("/p/a.rs")), Some(1));
        assert_eq!(t.find_path(root, Path::new("/p/b.rs")), None, "another pane's tab does not count");
        assert_eq!(t.find_path(right, Path::new("/p/b.rs")), Some(0));
    }

    #[test]
    fn set_video_state_writes_only_a_video_tab() {
        let mut t = Tree::new(Tab::Video { path: "/p/cut.mp4".into(), at: None, loop_range: None });
        let root = t.panes()[0];
        t.add_tab(root, Tab::Viewer { path: "/p/a.rs".into(), mode: ViewMode::default(), presenting: false, outline: false });
        t.set_video_state(root, 0, Some(12.5), Some((12.0, 18.0)));
        t.set_video_state(root, 1, Some(3.0), Some((1.0, 2.0)));
        let tabs = &t.pane(root).unwrap().tabs;
        assert_eq!(tabs[0], Tab::Video { path: "/p/cut.mp4".into(), at: Some(12.5), loop_range: Some((12.0, 18.0)) });
        assert_eq!(tabs[1], Tab::Viewer { path: "/p/a.rs".into(), mode: ViewMode::default(), presenting: false, outline: false });
    }

    #[test]
    fn find_path_and_tabs_on_see_every_kind() {
        let mut t = Tree::new(Tab::Viewer { path: "/p/cut.mp4".into(), mode: ViewMode::default(), presenting: false, outline: false });
        let root = t.panes()[0];
        t.add_tab(root, Tab::Video { path: "/p/cut.mp4".into(), at: None, loop_range: None });
        t.add_tab(root, Tab::Viewer { path: "/p/other.rs".into(), mode: ViewMode::default(), presenting: false, outline: false });
        let right = t.split(root, Side::Right, Tab::Video { path: "/p/cut.mp4".into(), at: None, loop_range: None }).unwrap();
        assert_eq!(t.find_path(root, Path::new("/p/cut.mp4")), Some(0));
        assert_eq!(t.find_path(root, Path::new("/p/none.mp4")), None);
        assert_eq!(t.tabs_on(Path::new("/p/cut.mp4")), vec![(root, 0), (root, 1), (right, 0)]);
    }

    #[test]
    fn tabs_within_finds_the_tabs_on_a_path_or_inside_it_and_no_others() {
        let mut t = Tree::new(Tab::Viewer { path: "/p/src/a.rs".into(), mode: ViewMode::default(), presenting: false, outline: false });
        let root = t.panes()[0];
        t.add_tab(root, Tab::Terminal { session: SessionId(1), cwd: Some("/p/src".into()) });
        t.add_tab(root, Tab::Viewer { path: "/p/srcs/b.rs".into(), mode: ViewMode::default(), presenting: false, outline: false });
        let right = t.split(root, Side::Right, Tab::Video { path: "/p/src/deep/c.mp4".into(), at: None, loop_range: None }).unwrap();
        let ids = |pairs: &[(PaneId, usize)]| pairs.iter().map(|&(p, i)| t.tab_id(p, i).unwrap()).collect::<Vec<_>>();
        assert_eq!(t.tabs_within(Path::new("/p/src")), ids(&[(root, 0), (right, 0)]), "a terminal there is kept, and srcs is another folder");
        assert_eq!(t.tabs_within(Path::new("/p/src/a.rs")), ids(&[(root, 0)]), "a file alone");
        assert!(t.tabs_within(Path::new("/q")).is_empty());
    }

    #[test]
    fn a_viewer_tab_s_mode_round_trips_and_an_old_file_renders() {
        let mut t = Tree::new(Tab::Viewer { path: "/p/a.md".into(), mode: ViewMode::Raw, presenting: false, outline: false });
        let json = serde_json::to_string(&t).unwrap();
        let back: Tree = serde_json::from_str(&json).unwrap();
        assert_eq!(back.pane(back.panes()[0]).unwrap().tabs[0], Tab::Viewer { path: "/p/a.md".into(), mode: ViewMode::Raw, presenting: false, outline: false });
        let old: Tree = serde_json::from_str(&json.replace(",\"mode\":\"Raw\"", "")).unwrap();
        assert_eq!(old.pane(old.panes()[0]).unwrap().tabs[0], Tab::Viewer { path: "/p/a.md".into(), mode: ViewMode::Rendered, presenting: false, outline: false });
        assert!(!json.contains("presenting"), "written only when on");
        t.set_viewer_presenting(t.panes()[0], 0, true);
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"presenting\":true"));
        let back: Tree = serde_json::from_str(&json).unwrap();
        assert!(matches!(back.pane(back.panes()[0]).unwrap().tabs[0], Tab::Viewer { presenting: true, .. }), "presentation mode comes back with the tab");
        t.set_viewer_presenting(t.panes()[0], 0, false);
        t.set_viewer_mode(t.panes()[0], 0, ViewMode::Rendered);
        assert!(matches!(t.pane(t.panes()[0]).unwrap().tabs[0], Tab::Viewer { mode: ViewMode::Rendered, .. }));
    }

    #[test]
    fn a_viewer_tab_s_outline_flag_round_trips_and_defaults_closed() {
        let mut t = Tree::new(Tab::Viewer { path: "/p/a.md".into(), mode: ViewMode::Rendered, presenting: false, outline: false });
        let json = serde_json::to_string(&t).unwrap();
        assert!(!json.contains("outline"), "written only when open");
        let old: Tree = serde_json::from_str(&json).unwrap();
        assert!(matches!(old.pane(old.panes()[0]).unwrap().tabs[0], Tab::Viewer { outline: false, .. }), "a file without it reads closed");
        t.set_viewer_outline(t.panes()[0], 0, true);
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"outline\":true"));
        let back: Tree = serde_json::from_str(&json).unwrap();
        assert!(matches!(back.pane(back.panes()[0]).unwrap().tabs[0], Tab::Viewer { outline: true, .. }), "an open outline comes back with the tab");
        t.set_viewer_outline(t.panes()[0], 0, false);
        assert!(matches!(t.pane(t.panes()[0]).unwrap().tabs[0], Tab::Viewer { outline: false, .. }));
    }

    #[test]
    fn a_0_6_browser_tab_reads_as_a_web_tab() {
        let blank: Tab = serde_json::from_str(r#"{"Browser":{"url":null,"app_mode":true}}"#).unwrap();
        assert_eq!(blank, Tab::Browser { url: String::new() }, "a blank 0.6 tab, its app mode dropped");
        let page: Tab = serde_json::from_str(r#"{"Browser":{"url":"https://x.dev","app_mode":false}}"#).unwrap();
        assert_eq!(page, Tab::Browser { url: "https://x.dev".into() });
        let missing: Tab = serde_json::from_str(r#"{"Browser":{}}"#).unwrap();
        assert_eq!(missing, Tab::Browser { url: String::new() });
        let t = Tab::Browser { url: "file:///p/a.html".into() };
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<Tab>(&json).unwrap(), t);
    }

    #[test]
    fn an_empty_tree_is_one_pane_with_no_tabs_that_takes_tabs() {
        let mut t = Tree::empty();
        assert_eq!(t.panes().len(), 1);
        let root = t.panes()[0];
        assert_eq!(t.focused, root);
        assert!(t.pane(root).unwrap().tabs.is_empty());
        assert!(t.pane(root).unwrap().ids.is_empty());
        let idx = t.add_tab(root, Tab::Terminal { session: SessionId(7), cwd: None });
        assert_eq!(idx, 0);
        assert_eq!(t.pane(root).unwrap().active, 0);
        assert!(t.tab_id(root, 0).is_some(), "the new tab gets a fresh id");
    }

    #[test]
    fn a_pane_closes_with_its_last_tab_unless_it_is_the_last_pane() {
        let mut t = Tree::new(Tab::Terminal { session: SessionId(1), cwd: None });
        let root = t.panes()[0];
        let right = t.split(root, Side::Right, Tab::Terminal { session: SessionId(2), cwd: None }).unwrap();
        assert!(!t.close_pane_if_empty(right), "a pane with a tab stays");
        t.close_tab(right, 0);
        assert!(t.close_pane_if_empty(right), "its last tab gone, the pane goes");
        assert_eq!(t.panes(), vec![root]);
        assert_eq!(t.focused, root, "focus moves to what is left");
        t.close_tab(root, 0);
        assert!(!t.close_pane_if_empty(root), "the last pane never closes");
        assert_eq!(t.panes(), vec![root]);
        assert!(t.pane(root).unwrap().tabs.is_empty(), "it shows the empty screen");
    }
    use proptest::prelude::*;

    fn term(n: u64) -> Tab {
        Tab::Terminal { session: SessionId(n), cwd: None }
    }

    fn bounds() -> Rect {
        Rect { x: 0.0, y: 0.0, w: 1200.0, h: 800.0 }
    }

    #[test]
    fn split_with_tab_moves_an_existing_tab_into_a_new_pane() {
        let mut t = Tree::new(term(1));
        let a = t.focused;
        t.add_tab(a, term(2));
        let moved = t.tab_id(a, 1).unwrap();
        let (id, tab) = t.take_tab(a, 1).unwrap();
        assert_eq!(id, moved);
        let b = t.split_with_tab(a, Side::Bottom, id, tab).unwrap();
        assert_eq!(t.pane(a).unwrap().tabs, vec![term(1)]);
        assert_eq!(t.pane(b).unwrap().tabs, vec![term(2)]);
        assert_eq!(t.panes(), vec![a, b]);
        // The moved tab keeps its id, so busy marks and pendings keyed by it still find it.
        assert_eq!(t.locate(moved), Some((b, 0)));
    }

    #[test]
    fn a_new_tree_has_one_pane_with_one_tab() {
        let t = Tree::new(term(1));
        assert_eq!(t.panes().len(), 1);
        let p = t.pane(t.focused).unwrap();
        assert_eq!(p.tabs, vec![term(1)]);
        assert_eq!(p.active, 0);
        assert_eq!(t.pane_index(t.focused), Some(0));
    }

    #[test]
    fn split_right_puts_the_new_pane_second_and_focuses_it() {
        let mut t = Tree::new(term(1));
        let left = t.focused;
        let right = t.split(left, Side::Right, term(2)).unwrap();
        assert_eq!(t.panes(), vec![left, right]);
        assert_eq!(t.focused, right);
        let l = t.layout(bounds());
        let (_, lr) = l.panes.iter().find(|(id, _)| *id == left).unwrap();
        let (_, rr) = l.panes.iter().find(|(id, _)| *id == right).unwrap();
        assert!(lr.x < rr.x && (lr.y - rr.y).abs() < 0.01);
        assert_eq!(l.handles.len(), 1);
        assert_eq!(l.handles[0].1, Orientation::Horizontal);
    }

    #[test]
    fn split_left_and_top_put_the_new_pane_first() {
        let mut t = Tree::new(term(1));
        let orig = t.focused;
        let left = t.split(orig, Side::Left, term(2)).unwrap();
        assert_eq!(t.panes(), vec![left, orig]);
        let top = t.split(orig, Side::Top, term(3)).unwrap();
        assert_eq!(t.panes(), vec![left, top, orig]);
        let l = t.layout(bounds());
        let r = |id| l.panes.iter().find(|(p, _)| *p == id).unwrap().1;
        assert!(r(top).y < r(orig).y);
        assert!(r(left).x < r(top).x);
    }

    #[test]
    fn screenshot_layout_numbers_left_then_top_right_then_bottom_right() {
        // Spec 4: a vertical split at 0.5 whose right child is a horizontal split at 0.5.
        let mut t = Tree::new(term(1));
        let left = t.focused;
        let right_top = t.split(left, Side::Right, term(2)).unwrap();
        let right_bottom = t.split(right_top, Side::Bottom, term(3)).unwrap();
        assert_eq!(t.panes(), vec![left, right_top, right_bottom]);
        assert_eq!(t.pane_at(1), Some(right_top));
        let l = t.layout(bounds());
        let r = |id| l.panes.iter().find(|(p, _)| *p == id).unwrap().1;
        assert!((r(left).w - (600.0 - HANDLE / 2.0)).abs() < 1.0);
        assert!((r(right_top).h - (400.0 - HANDLE / 2.0)).abs() < 1.0);
        assert_eq!(l.handles.len(), 2);
    }

    #[test]
    fn close_tab_keeps_an_empty_pane_open() {
        let mut t = Tree::new(term(1));
        let p = t.focused;
        assert_eq!(t.close_tab(p, 0), Some(term(1)));
        assert!(t.pane(p).unwrap().tabs.is_empty());
        assert_eq!(t.panes(), vec![p]);
    }

    #[test]
    fn close_tab_moves_active_to_the_neighbour() {
        let mut t = Tree::new(term(1));
        let p = t.focused;
        t.add_tab(p, term(2));
        t.add_tab(p, term(3));
        t.activate(p, 2);
        t.close_tab(p, 2);
        assert_eq!(t.pane(p).unwrap().active, 1);
        t.activate(p, 0);
        t.close_tab(p, 0);
        assert_eq!(t.pane(p).unwrap().active, 0);
        assert_eq!(t.pane(p).unwrap().tabs, vec![term(2)]);
    }

    #[test]
    fn last_pane_never_closes() {
        let mut t = Tree::new(term(1));
        let p = t.focused;
        t.close_tab(p, 0);
        assert!(!t.close_pane(p));
        assert_eq!(t.panes(), vec![p]);
    }

    #[test]
    fn close_pane_collapses_the_split_and_focuses_the_sibling() {
        let mut t = Tree::new(term(1));
        let a = t.focused;
        let b = t.split(a, Side::Right, term(2)).unwrap();
        let c = t.split(b, Side::Bottom, term(3)).unwrap();
        assert!(t.close_pane(b));
        assert_eq!(t.panes(), vec![a, c]);
        assert_eq!(t.focused, c);
        assert_eq!(t.layout(bounds()).handles.len(), 1);
        assert!(t.close_pane(c));
        assert_eq!(t.panes(), vec![a]);
        assert!(t.layout(bounds()).handles.is_empty());
    }

    #[test]
    fn a_session_s_name_is_its_own_and_a_tab_s_rename_leaves_it() {
        let mut t = Tree::new(Tab::Terminal { session: SessionId(1), cwd: None });
        let page = t.page;
        let id = t.tab_id(t.panes()[0], 0).unwrap();
        t.set_session_name(page, Some("  IDE  ".into()));
        assert_eq!(t.session_name(page), Some("IDE"), "trimmed");
        assert_eq!(t.tab_name(id), None, "the tab keeps its automatic title");
        t.set_tab_name(id, Some("build".into()));
        assert_eq!(t.session_name(page), Some("IDE"), "a tab's rename leaves the session's name");
        t.set_session_name(page, Some("  ".into()));
        assert_eq!(t.session_name(page), None, "blank gives the automatic name back");
        assert_eq!(t.tab_name(id), Some("build"), "and leaves the tab's");
    }

    #[test]
    fn a_session_s_name_is_saved_with_the_layout_and_goes_with_its_page() {
        let mut t = Tree::new(Tab::Terminal { session: SessionId(1), cwd: None });
        let first = t.page;
        t.set_session_name(first, Some("IDE".into()));
        let back: Tree = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(back.session_name(first), Some("IDE"));
        let pane = t.blank_page();
        t.add_tab(pane, Tab::Terminal { session: SessionId(2), cwd: None });
        let second = t.page;
        t.set_session_name(second, Some("Game".into()));
        t.close_page(first, None);
        t.close_page(second, None);
        assert_eq!((t.session_name(first), t.session_name(second)), (None, None));
    }

    #[test]
    fn a_layout_from_before_copies_each_page_s_tab_name_to_its_session_once() {
        let mut t = Tree::new(Tab::Terminal { session: SessionId(1), cwd: None });
        let page = t.page;
        let id = t.tab_id(t.panes()[0], 0).unwrap();
        t.set_tab_name(id, Some("IDE".into()));
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"names_split\":true"), "a new layout needs no copy");
        let mut old: Tree = serde_json::from_str(&json.replace("\"names_split\":true", "\"names_split\":false")).unwrap();
        old.split_names();
        assert_eq!((old.session_name(page), old.tab_name(id)), (Some("IDE"), Some("IDE")), "both keep the name");
        old.set_session_name(page, Some("Arkos".into()));
        old.split_names();
        assert_eq!(old.session_name(page), Some("Arkos"), "copied once");
    }

    #[test]
    fn a_tab_name_follows_its_tab_between_panes_and_dies_with_it() {
        let mut t = Tree::new(Tab::Terminal { session: SessionId(1), cwd: None });
        let root = t.panes()[0];
        let id = t.tab_id(root, 0).unwrap();
        assert_eq!(t.tab_name(id), None);
        t.set_tab_name(id, Some("  build  ".into()));
        assert_eq!(t.tab_name(id), Some("build"), "trimmed");
        // It is keyed by id: a move to another pane keeps it, and the saved form carries it.
        let right = t.split(root, Side::Right, Tab::Terminal { session: SessionId(2), cwd: None }).unwrap();
        assert!(t.move_tab(root, 0, right, None));
        assert_eq!(t.tab_name(id), Some("build"));
        let json = serde_json::to_string(&t).unwrap();
        let back: Tree = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tab_name(id), Some("build"));
        // Blank clears it; closing the tab drops it.
        t.set_tab_name(id, Some("   ".into()));
        assert_eq!(t.tab_name(id), None);
        t.set_tab_name(id, Some("again".into()));
        let idx = t.pane(right).unwrap().ids.iter().position(|i| *i == id).unwrap();
        t.close_tab(right, idx);
        assert_eq!(t.tab_name(id), None);
        assert!(t.names.is_empty());
        // A file from before names has none.
        let plain = serde_json::to_string(&Tree::new(Tab::Terminal { session: SessionId(9), cwd: None })).unwrap();
        assert!(plain.contains(",\"names\":[]"));
        let old: Tree = serde_json::from_str(&plain.replace(",\"names\":[]", "")).unwrap();
        assert!(old.names.is_empty());
    }

    #[test]
    fn move_tab_between_panes() {
        let mut t = Tree::new(term(1));
        let a = t.focused;
        let b = t.split(a, Side::Right, term(2)).unwrap();
        t.add_tab(a, term(3));
        assert!(t.move_tab(a, 1, b, Some(0)));
        assert_eq!(t.pane(a).unwrap().tabs, vec![term(1)]);
        assert_eq!(t.pane(b).unwrap().tabs, vec![term(3), term(2)]);
        assert_eq!(t.pane(b).unwrap().active, 0);
        // Without a position the tab goes last.
        assert!(t.move_tab(b, 0, a, None));
        assert_eq!(t.pane(a).unwrap().tabs, vec![term(1), term(3)]);
    }

    #[test]
    fn move_tab_within_a_pane_reorders() {
        let mut t = Tree::new(term(1));
        let p = t.focused;
        t.add_tab(p, term(2));
        t.add_tab(p, term(3));
        t.activate(p, 0);
        assert!(t.move_tab_within(p, 0, 1));
        assert_eq!(t.pane(p).unwrap().tabs, vec![term(2), term(1), term(3)]);
        assert_eq!(t.pane(p).unwrap().active, 1);
        assert!(!t.move_tab_within(p, 2, 1));
    }

    #[test]
    fn mru_cycles_most_recent_first() {
        let mut t = Tree::new(term(1));
        let p = t.focused;
        t.add_tab(p, term(2));
        t.add_tab(p, term(3));
        t.activate(p, 2);
        t.activate(p, 0);
        assert_eq!(t.mru_next(p), Some(2));
        t.close_tab(p, 2);
        assert_eq!(t.mru_next(p), Some(1));
    }

    #[test]
    fn ratio_respects_minimum_pane_size() {
        let mut t = Tree::new(term(1));
        let a = t.focused;
        let b = t.split(a, Side::Right, term(2)).unwrap();
        let split = t.layout(bounds()).handles[0].0;
        t.set_ratio(split, 0.01);
        let l = t.layout(bounds());
        let r = |id| l.panes.iter().find(|(p, _)| *p == id).unwrap().1;
        assert!(r(a).w >= MIN_PANE_W - 0.01);
        t.reset_ratio(split);
        assert_eq!(t.split_of(split).unwrap().1, 0.5);
        assert!(r(b).w > 0.0);
    }

    #[test]
    fn set_ratio_ignores_non_finite_ratios() {
        let mut t = Tree::new(term(1));
        let a = t.focused;
        t.split(a, Side::Right, term(2));
        let split = t.layout(bounds()).handles[0].0;
        t.set_ratio(split, 0.3);
        for r in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            t.set_ratio(split, r);
            assert_eq!(t.split_of(split).unwrap().1, 0.3);
        }
    }

    #[test]
    fn nested_same_axis_splits_keep_the_minimum_for_each_pane() {
        let mut t = Tree::new(term(1));
        let a = t.focused;
        let b = t.split(a, Side::Right, term(2)).unwrap();
        let c = t.split(b, Side::Right, term(3)).unwrap();
        let outer = t.layout(bounds()).handles[0].0;
        t.set_ratio(outer, 0.95);
        let l = t.layout(bounds());
        let r = |id| l.panes.iter().find(|(p, _)| *p == id).unwrap().1;
        assert!(r(b).w >= MIN_PANE_W - 0.01, "b is {}", r(b).w);
        assert!(r(c).w >= MIN_PANE_W - 0.01, "c is {}", r(c).w);
        assert!(r(a).w >= MIN_PANE_W - 0.01);
    }

    #[test]
    fn move_tab_to_end_of_the_same_pane() {
        let mut t = Tree::new(term(1));
        let p = t.focused;
        t.add_tab(p, term(2));
        t.add_tab(p, term(3));
        assert!(t.move_tab(p, 0, p, None));
        assert_eq!(t.pane(p).unwrap().tabs, vec![term(2), term(3), term(1)]);
        assert_eq!(t.pane(p).unwrap().active, 2);
    }

    #[test]
    fn maximize_gives_one_pane_the_whole_window() {
        let mut t = Tree::new(term(1));
        let a = t.focused;
        let b = t.split(a, Side::Right, term(2)).unwrap();
        t.toggle_maximize(b);
        let l = t.layout(bounds());
        assert_eq!(l.panes.len(), 1);
        assert_eq!(l.panes[0].0, b);
        assert!(l.handles.is_empty());
        assert_eq!(t.panes().len(), 2, "numbering still counts every pane");
        t.toggle_maximize(b);
        assert_eq!(t.layout(bounds()).panes.len(), 2);
    }

    #[test]
    fn a_quiet_activation_leaves_the_focus_and_the_maximize_alone() {
        let mut t = Tree::new(term(1));
        let a = t.focused;
        let b = t.split(a, Side::Right, term(2)).unwrap();
        t.add_tab(b, term(3));
        t.activate(b, 0);
        t.focus(a);
        t.toggle_maximize(a);
        t.activate_quiet(b, 1);
        assert_eq!((t.focused, t.maximized), (a, Some(a)), "the hidden pane is not shown and the keys stay put");
        let p = t.pane(b).unwrap();
        assert_eq!((p.active, p.mru.clone()), (1, vec![1, 0]));
        t.activate_quiet(b, 9);
        assert_eq!(t.pane(b).unwrap().active, 1, "an index past the end changes nothing");
    }

    #[test]
    fn focusing_another_pane_exits_maximize() {
        let mut t = Tree::new(term(1));
        let a = t.focused;
        let b = t.split(a, Side::Right, term(2)).unwrap();
        t.add_tab(b, term(3));
        t.toggle_maximize(b);
        // Focusing or activating within the maximized pane keeps it maximized.
        t.focus(b);
        t.activate(b, 0);
        assert_eq!(t.maximized, Some(b));
        // Focusing a hidden pane shows it.
        t.focus(a);
        assert_eq!((t.focused, t.maximized), (a, None));
        t.toggle_maximize(a);
        t.activate(b, 1);
        assert_eq!((t.focused, t.maximized), (b, None));
        // A tab moved to a hidden pane takes focus there, which shows it.
        t.toggle_maximize(b);
        assert!(t.move_tab(b, 0, a, None));
        assert_eq!((t.focused, t.maximized), (a, None));
        // A pane that does not exist changes nothing.
        t.toggle_maximize(a);
        t.focus(999);
        assert_eq!((t.focused, t.maximized), (a, Some(a)));
    }

    #[test]
    fn neighbor_follows_geometry() {
        let mut t = Tree::new(term(1));
        let left = t.focused;
        let right_top = t.split(left, Side::Right, term(2)).unwrap();
        let right_bottom = t.split(right_top, Side::Bottom, term(3)).unwrap();
        assert_eq!(t.neighbor(left, Side::Right, bounds()), Some(right_top));
        assert_eq!(t.neighbor(right_bottom, Side::Left, bounds()), Some(left));
        assert_eq!(t.neighbor(right_top, Side::Bottom, bounds()), Some(right_bottom));
        assert_eq!(t.neighbor(right_bottom, Side::Top, bounds()), Some(right_top));
        assert_eq!(t.neighbor(left, Side::Left, bounds()), None);
    }

    #[test]
    fn the_inbox_s_side_may_sit_on_any_side_of_the_sessions_and_keeps_its_place_through_take_and_put() {
        let doc = |p: &str| Tab::Viewer { path: p.into(), mode: ViewMode::Rendered, presenting: false, outline: false };
        for side in [Side::Left, Side::Right, Side::Top, Side::Bottom] {
            let mut t = Tree::new(term(1));
            let sessions = t.panes()[0];
            let files = t.split(sessions, side, doc("/p/a.md")).unwrap();
            assert!(t.is_inbox(), "{side:?}");
            assert_eq!(t.inbox_panes(), Some((sessions, Some(files))), "{side:?}");
            assert_eq!(t.inbox_place().map(|p| p.side), Some(side));
            let place = SidePlace { side, share: 0.7 };
            assert!(t.inbox_set_place(place));
            assert_eq!(t.inbox_place(), Some(place), "the sessions keep their share whichever comes first");
            let (taken, at) = t.inbox_take_side().unwrap();
            assert_eq!((t.panes(), at), (vec![sessions], place));
            assert!(t.inbox_put_side(taken, at));
            assert_eq!(t.inbox_place(), Some(place));
        }
        // Two sessions panes, or a terminal in the side, are not the shape.
        let mut t = Tree::new(term(1));
        let a = t.panes()[0];
        t.split(a, Side::Top, term(2)).unwrap();
        assert!(!t.is_inbox());
    }

    #[test]
    fn a_pane_dragged_to_an_edge_moves_there_whole_and_one_dropped_in_the_middle_joins() {
        let doc = |p: &str| Tab::Viewer { path: p.into(), mode: ViewMode::Rendered, presenting: false, outline: false };
        // left | right, right holding two files.
        let mut t = Tree::new(term(1));
        let left = t.panes()[0];
        let right = t.split(left, Side::Right, doc("/p/a.md")).unwrap();
        t.add_tab(right, doc("/p/b.md"));
        t.activate(right, 0);
        let ids = t.pane(right).unwrap().ids.clone();
        assert!(t.move_pane(right, left, Side::Top));
        assert_eq!(t.panes(), vec![right, left], "now on top");
        assert!(matches!(t.root, Node::Split { orientation: Orientation::Vertical, .. }));
        assert_eq!((t.pane(right).unwrap().ids.clone(), t.focused), (ids.clone(), right), "tabs, ids and all, and focused");
        assert!(!t.move_pane(right, right, Side::Left), "onto itself");
        assert!(!t.move_pane(right, 999, Side::Left));
        // Dropped in the middle of another, its tabs join it in order and it goes.
        assert!(t.merge_pane(right, left, None));
        assert_eq!(t.panes(), vec![left]);
        assert_eq!(t.pane(left).unwrap().tabs, vec![term(1), doc("/p/a.md"), doc("/p/b.md")]);
        assert_eq!(t.pane(left).unwrap().active, 1, "showing the tab the dragged pane showed");
        // Onto a tab bar, at a place in it.
        let mut u = Tree::new(term(1));
        let a = u.panes()[0];
        u.add_tab(a, term(2));
        let b = u.split(a, Side::Right, doc("/p/x.md")).unwrap();
        u.add_tab(b, doc("/p/y.md"));
        assert!(u.merge_pane(b, a, Some(1)));
        assert_eq!(u.pane(a).unwrap().tabs, vec![term(1), doc("/p/x.md"), doc("/p/y.md"), term(2)]);
    }

    #[test]
    fn a_window_is_narrow_under_1500_points_of_panes() {
        assert!(is_narrow(1230.0), "a 13 inch MacBook with the sidebar");
        assert!(!is_narrow(1700.0));
        assert!(!is_narrow(0.0), "no width yet is not narrow");
    }

    #[test]
    fn find_session_locates_a_tab() {
        let mut t = Tree::new(term(1));
        let a = t.focused;
        let b = t.split(a, Side::Right, term(2)).unwrap();
        assert_eq!(t.find_session(SessionId(2)), Some((b, 0)));
        assert_eq!(t.find_session(SessionId(9)), None);
        t.replace_tab(b, 0, term(9));
        assert_eq!(t.find_session(SessionId(9)), Some((b, 0)));
    }

    #[test]
    fn serializes_round_trip() {
        let mut t = Tree::new(term(1));
        let a = t.focused;
        t.split(a, Side::Bottom, term(2));
        let json = serde_json::to_string(&t).unwrap();
        let back: Tree = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn tab_ids_survive_moves_and_closes() {
        let mut t = Tree::new(term(1));
        let a = t.focused;
        let b = t.split(a, Side::Right, term(2)).unwrap();
        t.add_tab(a, term(3));
        t.add_tab(a, term(4));
        t.add_tab(a, term(5));
        let ids: Vec<TabId> = (0..4).map(|i| t.tab_id(a, i).unwrap()).collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 4);
        // Move tab 3 (index 2) to the other pane, then close the first tab: indices shift.
        assert!(t.move_tab(a, 2, b, Some(0)));
        let closed = ids[0];
        t.close_tab(a, 0);
        assert_eq!(t.locate(ids[2]), Some((b, 0)));
        assert_eq!(t.pane(b).unwrap().tabs[0], term(4));
        assert_eq!(t.locate(closed), None);
        assert_eq!(t.locate(ids[1]), Some((a, 0)));
        assert_eq!(t.locate(ids[3]), Some((a, 1)));
        // Replacing a tab's content keeps its id; reordering carries it.
        t.replace_tab(a, 0, term(9));
        assert_eq!(t.tab_id(a, 0), Some(ids[1]));
        assert!(t.move_tab_within(a, 0, 1));
        assert_eq!(t.locate(ids[1]), Some((a, 1)));
        check_invariants(&t);
    }

    #[test]
    fn repair_ids_fills_missing_ids_and_moves_the_counter_past_them() {
        let mut t = Tree::new(term(1));
        let a = t.focused;
        t.split(a, Side::Bottom, term(2));
        t.add_tab(a, term(3));
        // A file written before tab ids existed: strip them, as `#[serde(default)]` would.
        let json = serde_json::to_string(&t).unwrap().replace(r#""ids":"#, r#""old_ids":"#);
        let mut old: Tree = serde_json::from_str(&json).unwrap();
        assert!(old.pane(a).unwrap().ids.is_empty());
        old.repair_ids();
        check_invariants(&old);
        let max = old.tabs().iter().map(|(p, i, _)| old.tab_id(*p, *i).unwrap()).max().unwrap();
        let next = old.add_tab(a, term(4));
        assert!(old.tab_id(a, next).unwrap() > max, "fresh ids never collide with repaired ones");
        check_invariants(&old);
        // A sound tree is left as it is.
        let before = t.clone();
        t.repair_ids();
        assert_eq!(t, before);
    }

    #[derive(Debug, Clone)]
    enum Op {
        Split(usize, Side),
        CloseTab(usize, usize),
        ClosePane(usize),
        MoveTab(usize, usize, usize, Option<usize>),
        Ratio(usize, f32),
        Maximize(usize),
        Activate(usize, usize),
    }

    fn op() -> impl Strategy<Value = Op> {
        let side = prop_oneof![Just(Side::Left), Just(Side::Right), Just(Side::Top), Just(Side::Bottom)];
        prop_oneof![
            (0..8usize, side).prop_map(|(p, s)| Op::Split(p, s)),
            (0..8usize, 0..6usize).prop_map(|(p, i)| Op::CloseTab(p, i)),
            (0..8usize).prop_map(Op::ClosePane),
            (0..8usize, 0..6usize, 0..8usize, proptest::option::of(0..6usize)).prop_map(|(f, i, t, a)| Op::MoveTab(f, i, t, a)),
            (0..8usize, 0.0f32..1.0).prop_map(|(s, r)| Op::Ratio(s, r)),
            (0..8usize).prop_map(Op::Maximize),
            (0..8usize, 0..6usize).prop_map(|(p, i)| Op::Activate(p, i)),
        ]
    }

    /// Applies an op to whatever pane or split the indices name modulo what exists.
    fn apply(t: &mut Tree, op: &Op, next: &mut u64) {
        let panes = t.panes();
        let pick = |i: usize| panes[i % panes.len()];
        match *op {
            Op::Split(p, side) => {
                *next += 1;
                t.split(pick(p), side, term(*next));
            }
            Op::CloseTab(p, i) => {
                let p = pick(p);
                let n = t.pane(p).unwrap().tabs.len();
                if n > 0 {
                    t.close_tab(p, i % n);
                }
            }
            Op::ClosePane(p) => {
                t.close_pane(pick(p));
            }
            Op::MoveTab(f, i, to, at) => {
                let (f, to) = (pick(f), pick(to));
                let n = t.pane(f).unwrap().tabs.len();
                if n > 0 {
                    t.move_tab(f, i % n, to, at);
                }
            }
            Op::Ratio(s, r) => {
                let handles = t.layout(bounds()).handles;
                if !handles.is_empty() {
                    t.set_ratio(handles[s % handles.len()].0, r);
                }
            }
            Op::Maximize(p) => t.toggle_maximize(pick(p)),
            Op::Activate(p, i) => {
                let p = pick(p);
                let n = t.pane(p).unwrap().tabs.len();
                if n > 0 {
                    t.activate(p, i % n);
                }
            }
        }
    }

    fn check_invariants(t: &Tree) {
        let panes = t.panes();
        assert!(!panes.is_empty());
        let mut seen = std::collections::HashSet::new();
        let mut tab_ids = std::collections::HashSet::new();
        for p in &panes {
            assert!(seen.insert(*p), "duplicate pane id");
            let pane = t.pane(*p).expect("every listed pane resolves");
            assert_eq!(pane.ids.len(), pane.tabs.len(), "one id per tab");
            for id in &pane.ids {
                assert!(tab_ids.insert(*id), "duplicate tab id");
            }
            assert!(pane.tabs.is_empty() || pane.active < pane.tabs.len(), "active in range");
            for i in &pane.mru {
                assert!(*i < pane.tabs.len(), "mru in range");
            }
        }
        assert!(panes.contains(&t.focused), "focused pane exists");
        if let Some(m) = t.maximized {
            assert!(panes.contains(&m));
        }
        let l = t.layout(bounds());
        let shown: Vec<PaneId> = l.panes.iter().map(|(id, _)| *id).collect();
        match t.maximized {
            Some(m) => assert_eq!(shown, vec![m]),
            None => assert_eq!(shown, panes, "layout lists panes in reading order"),
        }
        for (i, (_, a)) in l.panes.iter().enumerate() {
            assert!(a.w >= 0.0 && a.h >= 0.0);
            for (_, b) in l.panes.iter().skip(i + 1) {
                let overlap = a.x < b.x + b.w - 0.01 && b.x < a.x + a.w - 0.01 && a.y < b.y + b.h - 0.01 && b.y < a.y + a.h - 0.01;
                assert!(!overlap, "panes overlap: {a:?} {b:?}");
            }
        }
        for (_, _, r, area) in &l.handles {
            assert!(r.w > 0.0 && r.h > 0.0);
            assert!(area.w >= r.w && area.h >= r.h);
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(300))]

        #[test]
        fn random_operations_keep_the_tree_valid(ops in proptest::collection::vec(op(), 0..40)) {
            let mut t = Tree::new(term(1));
            let mut next = 1;
            for op in &ops {
                apply(&mut t, op, &mut next);
                check_invariants(&t);
            }
        }

        #[test]
        fn panes_are_numbered_in_reading_order(ops in proptest::collection::vec(op(), 0..30)) {
            let mut t = Tree::new(term(1));
            let mut next = 1;
            for op in &ops {
                apply(&mut t, op, &mut next);
            }
            let eq = t.equal_ratio_layout(bounds());
            let rect = |id: PaneId| eq.panes.iter().find(|(p, _)| *p == id).unwrap().1;
            let panes = t.panes();
            prop_assert_eq!(panes.len(), eq.panes.len());
            // Reading order: the next pane is in a lower band, or in the same band further right.
            for w in panes.windows(2) {
                let (a, b) = (rect(w[0]), rect(w[1]));
                prop_assert!(b.y > a.y + 0.5 || ((a.y - b.y).abs() <= 0.5 && b.x > a.x), "{a:?} then {b:?}");
            }
        }

        #[test]
        fn ratio_changes_never_renumber_panes(ops in proptest::collection::vec(op(), 0..30), ratios in proptest::collection::vec((0..8usize, 0.0f32..1.0, proptest::bool::ANY), 0..20)) {
            let mut t = Tree::new(term(1));
            let mut next = 1;
            for op in &ops {
                apply(&mut t, op, &mut next);
            }
            t.maximized = None;
            let before = t.panes();
            for (s, r, reset) in ratios {
                let handles = t.layout(bounds()).handles;
                if handles.is_empty() {
                    continue;
                }
                let id = handles[s % handles.len()].0;
                if reset { t.reset_ratio(id) } else { t.set_ratio(id, r) }
                prop_assert_eq!(t.panes(), before.clone());
                let live: Vec<PaneId> = t.layout(bounds()).panes.iter().map(|(id, _)| *id).collect();
                prop_assert_eq!(live, before.clone());
            }
        }

        #[test]
        fn every_pane_is_reachable(ops in proptest::collection::vec(op(), 0..30)) {
            let mut t = Tree::new(term(1));
            let mut next = 1;
            for op in &ops {
                apply(&mut t, op, &mut next);
            }
            let panes = t.panes();
            for (i, p) in panes.iter().enumerate() {
                prop_assert_eq!(t.pane_at(i), Some(*p));
                prop_assert_eq!(t.pane_index(*p), Some(i));
            }
            prop_assert_eq!(t.pane_at(panes.len()), None);
        }

        #[test]
        fn layout_never_panics_at_tiny_bounds(ops in proptest::collection::vec(op(), 0..30), w in 0.0f32..=200.0, h in 0.0f32..=200.0) {
            let mut t = Tree::new(term(1));
            let mut next = 1;
            for op in &ops {
                apply(&mut t, op, &mut next);
            }
            for maximized in [t.maximized, None] {
                t.maximized = maximized;
                let l = t.layout(Rect { x: 0.0, y: 0.0, w, h });
                let rects: Vec<Rect> = l.panes.iter().map(|(_, r)| *r).chain(l.handles.iter().map(|(_, _, r, _)| *r)).collect();
                for r in rects {
                    prop_assert!(r.w.is_finite() && r.h.is_finite() && r.w >= 0.0 && r.h >= 0.0, "{r:?} in {w}x{h}");
                }
            }
            // The window clamps its content to zero, but a negative extent must not panic either.
            t.layout(Rect { x: 0.0, y: 0.0, w: -w, h: -h });
        }

        #[test]
        fn move_tab_onto_itself_is_a_no_op(ops in proptest::collection::vec(op(), 0..20), pick in 0..8usize, at in proptest::option::of(0..6usize)) {
            let mut t = Tree::new(term(1));
            let mut next = 1;
            for op in &ops {
                apply(&mut t, op, &mut next);
            }
            let panes = t.panes();
            let p = panes[pick % panes.len()];
            let n = t.pane(p).unwrap().tabs.len();
            if n > 0 {
                let idx = pick % n;
                let before = t.clone();
                let at = at.map(|a| a % (n + 1));
                // Same pane, same position (or the position a removal-then-insert lands on).
                let same_spot = match at { None => idx == n - 1, Some(a) => a == idx || a == idx + 1 };
                if same_spot {
                    t.move_tab(p, idx, p, at);
                    prop_assert_eq!(t, before);
                }
            }
        }
    }

    // Pages (sessions hold grids spec, section 1).

    fn doc(p: &str) -> Tab {
        Tab::Viewer { path: p.into(), mode: ViewMode::default(), presenting: false, outline: false }
    }

    #[test]
    fn a_new_page_shows_one_fresh_tab_and_keeps_the_page_before_as_it_was() {
        let mut t = Tree::new(term(1));
        let first = t.panes()[0];
        let right = t.split(first, Side::Right, doc("/p/a.md")).unwrap();
        let before = t.page;
        let pane = t.new_page(term(2));
        assert_eq!(t.panes(), vec![pane], "the new page is one pane");
        assert_eq!(t.pane(pane).unwrap().tabs, vec![term(2)]);
        assert_eq!(t.focused, pane);
        assert_ne!(t.page, before);
        assert_eq!(t.page_keys().len(), 2);
        assert!(t.show_page(before));
        assert_eq!(t.panes(), vec![first, right], "the page before comes back split as it was");
        assert_eq!(t.focused, right, "and focused where it was");
    }

    #[test]
    fn every_tab_of_every_page_is_found_while_only_the_shown_page_is_laid_out() {
        let mut t = Tree::new(term(1));
        let first = t.panes()[0];
        t.split(first, Side::Right, doc("/p/a.md")).unwrap();
        let kept = t.page;
        let pane = t.new_page(term(2));
        assert_eq!(t.panes(), vec![pane]);
        assert_eq!(t.layout(Rect { x: 0.0, y: 0.0, w: 800.0, h: 600.0 }).panes.len(), 1);
        assert_eq!(t.find_session(SessionId(1)), Some((first, 0)), "a kept page's terminal is found");
        assert_eq!(t.page_of(first), Some(kept));
        assert_eq!(t.page_of(pane), Some(t.page));
        assert_eq!(t.tabs().len(), 3, "the tabs of every page");
        assert_eq!(t.tabs_on(Path::new("/p/a.md")).len(), 1);
        let id = t.tab_id(first, 0).unwrap();
        assert_eq!(t.locate(id), Some((first, 0)));
        assert_eq!(t.page_tabs(kept).len(), 2);
        assert_eq!(t.page_tabs(t.page).len(), 1);
    }

    #[test]
    fn a_kept_page_s_tab_changes_in_place_without_showing_it() {
        let mut t = Tree::new(term(1));
        let first = t.panes()[0];
        let shown = t.new_page(term(2));
        t.replace_tab(first, 0, term(9));
        t.activate_quiet(first, 0);
        assert_eq!(t.focused, shown, "nothing moved on screen");
        assert_eq!(t.find_session(SessionId(9)), Some((first, 0)));
    }

    #[test]
    fn focusing_a_pane_of_a_kept_page_shows_that_page() {
        let mut t = Tree::new(term(1));
        let first = t.panes()[0];
        let kept = t.page;
        t.new_page(term(2));
        t.focus(first);
        assert_eq!(t.page, kept);
        assert_eq!(t.focused, first);
        assert_eq!(t.panes(), vec![first]);
    }

    #[test]
    fn a_page_left_with_no_tabs_is_not_kept_when_another_is_shown() {
        let mut t = Tree::new(term(1));
        let kept = t.page;
        let pane = t.new_page(term(2));
        t.close_tab(pane, 0);
        assert!(t.show_page(kept));
        assert_eq!(t.page_keys(), vec![kept], "the empty page went");
    }

    #[test]
    fn work_within_a_kept_page_leaves_the_shown_page_and_its_focus_alone() {
        let mut t = Tree::new(term(1));
        let first = t.panes()[0];
        let kept = t.page;
        let shown = t.new_page(term(2));
        let new = t.within(kept, |t| t.split(first, Side::Right, doc("/p/a.md"))).unwrap();
        assert_eq!(t.panes(), vec![shown]);
        assert_eq!(t.focused, shown);
        assert_eq!(t.page_of(new), Some(kept));
        assert!(t.show_page(kept));
        assert_eq!(t.panes(), vec![first, new]);
        // An empty page shown is dropped when another is made, but work within a kept page keeps it.
        let mut e = Tree::empty();
        e.new_page(term(4));
        assert_eq!(e.page_keys().len(), 1, "the empty page went");
        let mut f = Tree::new(term(5));
        let one = f.page;
        let p = f.new_page(term(6));
        f.close_tab(p, 0);
        let empty_key = f.page;
        f.within(one, |_| ());
        assert_eq!(f.page, empty_key, "within keeps an empty page shown");
    }

    #[test]
    fn closing_a_page_shows_the_one_named_and_a_kept_page_just_goes() {
        let mut t = Tree::new(term(1));
        let a = t.page;
        t.new_page(term(2));
        let b = t.page;
        t.new_page(term(3));
        let c = t.page;
        t.close_page(a, None);
        assert_eq!(t.page_keys(), vec![b, c]);
        assert_eq!(t.page, c);
        t.close_page(c, Some(b));
        assert_eq!(t.page, b);
        assert_eq!(t.find_session(SessionId(3)), None);
        t.close_page(b, None);
        assert_eq!(t.tabs().len(), 0, "the last page closed leaves the empty screen");
        assert_eq!(t.panes().len(), 1);
    }

    #[test]
    fn ids_stay_unique_across_pages_and_pages_are_saved() {
        let mut t = Tree::new(term(1));
        t.new_page(term(2));
        let p = t.panes()[0];
        t.add_tab(p, doc("/p/a.md"));
        let mut ids: Vec<u64> = t.tabs().iter().map(|(pane, idx, _)| t.tab_id(*pane, *idx).unwrap()).collect();
        ids.extend(t.page_keys().into_iter().filter(|k| *k != 0));
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
        let json = serde_json::to_string(&t).unwrap();
        let back: Tree = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
        assert!(back.paged);
        let old: Tree = serde_json::from_str(r#"{ "root": { "Leaf": { "id": 1, "tabs": [], "active": 0 } }, "next_id": 2, "focused": 1, "maximized": null }"#).unwrap();
        assert!(!old.paged, "a file from before pages is not in pages");
    }
}
