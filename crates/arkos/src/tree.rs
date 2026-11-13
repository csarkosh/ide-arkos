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

/// The sessions pane's share of the Inbox's width when it has a side pane.
pub const INBOX_RATIO: f32 = 0.6;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    fn orientation(self) -> Orientation {
        match self {
            Side::Left | Side::Right => Orientation::Horizontal,
            Side::Top | Side::Bottom => Orientation::Vertical,
        }
    }

    /// Whether the new pane goes before the existing one in the split.
    fn new_pane_first(self) -> bool {
        matches!(self, Side::Left | Side::Top)
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
        Tree { root: Node::Leaf(pane), next_id: 3, focused: 1, maximized: None, names: Vec::new() }
    }

    /// One pane with no tabs: what a folder with no saved layout opens on, the empty screen.
    pub fn empty() -> Tree {
        let pane = Pane { id: 1, tabs: Vec::new(), active: 0, mru: Vec::new(), ids: Vec::new() };
        Tree { root: Node::Leaf(pane), next_id: 2, focused: 1, maximized: None, names: Vec::new() }
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

    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.root.find(id)
    }

    pub fn pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        self.root.find_mut(id)
    }

    pub fn pane_index(&self, id: PaneId) -> Option<usize> {
        self.panes().iter().position(|p| *p == id)
    }

    pub fn pane_at(&self, index: usize) -> Option<PaneId> {
        self.panes().get(index).copied()
    }

    pub fn find_session(&self, session: SessionId) -> Option<(PaneId, usize)> {
        for id in self.panes() {
            let pane = self.pane(id)?;
            if let Some(i) = pane.tabs.iter().position(|t| matches!(t, Tab::Terminal { session: s, .. } if *s == session)) {
                return Some((id, i));
            }
        }
        None
    }

    pub fn tabs(&self) -> Vec<(PaneId, usize, &Tab)> {
        let mut out = Vec::new();
        for id in self.panes() {
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
    pub fn focus(&mut self, pane: PaneId) {
        if self.pane(pane).is_none() {
            return;
        }
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

    /// Whether the tree has the Inbox layout's shape (layouts spec, "The Inbox layout"): one
    /// pane holding only terminal tabs, the sessions, alone or with a **side** to its right: one
    /// pane or several, none of them empty and none holding a terminal. The side belongs to the
    /// session shown, and the window swaps it as another is chosen (`inbox_take_side`).
    pub fn is_inbox(&self) -> bool {
        let terminals = |p: &Pane| p.tabs.iter().all(|t| matches!(t, Tab::Terminal { .. }));
        let others = |p: &Pane| !p.tabs.is_empty() && p.tabs.iter().all(|t| !matches!(t, Tab::Terminal { .. }));
        match &self.root {
            Node::Leaf(p) => terminals(p),
            Node::Split { orientation: Orientation::Horizontal, first, second, .. } => matches!(&**first, Node::Leaf(a) if terminals(a)) && second.panes().into_iter().all(others),
            Node::Split { .. } => false,
        }
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
                let Node::Leaf(sessions) = &**first else { return None };
                let side: Vec<PaneId> = second.panes().into_iter().map(|p| p.id).collect();
                Some((sessions.id, side.iter().copied().find(|id| *id == self.focused).or(side.first().copied())))
            }
        }
    }

    /// Lifts the Inbox's side out of the tree, leaving the sessions pane alone: the side as it
    /// is, panes, tabs, ids and all, and the share of the width the sessions had beside it. None
    /// when there is no side. The focus goes to the sessions when it was in the side.
    pub fn inbox_take_side(&mut self) -> Option<(Node, f32)> {
        if !self.is_inbox() || matches!(self.root, Node::Leaf(_)) {
            return None;
        }
        let Node::Split { ratio, first, second, .. } = std::mem::replace(&mut self.root, placeholder()) else { unreachable!("checked above") };
        self.root = *first;
        let sessions = self.panes()[0];
        if self.pane(self.focused).is_none() {
            self.focused = sessions;
        }
        if self.maximized.is_some_and(|m| self.pane(m).is_none()) {
            self.maximized = None;
        }
        Some((*second, ratio))
    }

    /// Puts `side` beside the sessions pane, which must be alone (the tree in the Inbox's shape
    /// with no side): the reverse of `inbox_take_side`. False, with nothing changed, otherwise.
    pub fn inbox_put_side(&mut self, side: Node, ratio: f32) -> bool {
        if !self.is_inbox() || !matches!(self.root, Node::Leaf(_)) {
            return false;
        }
        self.next_id = self.next_id.max(side.max_id() + 1);
        let id = self.fresh_id();
        let sessions = std::mem::replace(&mut self.root, placeholder());
        self.root = Node::Split { id, orientation: Orientation::Horizontal, ratio, first: Box::new(sessions), second: Box::new(side) };
        true
    }

    /// Adds every tab of `side` to the tree's own side, each with its id: to the pane a new tab
    /// goes to, or with no side yet, `side` itself becomes it.
    pub fn inbox_absorb(&mut self, side: Node, ratio: f32) {
        let Some((_, target)) = self.inbox_panes() else { return };
        let Some(target) = target else {
            self.inbox_put_side(side, ratio);
            return;
        };
        self.next_id = self.next_id.max(side.max_id() + 1);
        for pane in side.panes() {
            for (tab, id) in pane.tabs.iter().zip(&pane.ids) {
                if let Some(p) = self.pane_mut(target) {
                    p.tabs.push(tab.clone());
                    p.ids.push(*id);
                    p.mru.push(p.tabs.len() - 1);
                }
            }
        }
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

    /// Puts right a tree that is out of the Inbox's shape by a tab or two, without disturbing
    /// the rest: a terminal among the side's tabs joins the sessions (and is the one shown), a
    /// tab that is no terminal among the sessions goes to the side (to a new pane on the right
    /// when there is no side), and a pane left empty goes. True when the tree then has the shape.
    fn settle_inbox(&mut self) -> bool {
        let is_term = |t: &Tab| matches!(t, Tab::Terminal { .. });
        let Some(sessions) = self.panes().into_iter().find(|id| self.pane(*id).is_some_and(|p| p.tabs.iter().any(is_term))).or(self.panes().first().copied()) else { return false };
        let was_focused = self.focused;
        for pane in self.panes().into_iter().filter(|p| *p != sessions) {
            while let Some(idx) = self.pane(pane).and_then(|p| p.tabs.iter().position(is_term)) {
                self.move_tab(pane, idx, sessions, None);
            }
        }
        // The side pane a stray tab joins: the focused one when it is not the sessions.
        let mut target = Some(was_focused).filter(|p| *p != sessions && self.pane(*p).is_some()).or_else(|| self.panes().into_iter().find(|p| *p != sessions));
        let mut moved = false;
        while let Some(idx) = self.pane(sessions).and_then(|p| p.tabs.iter().position(|t| !is_term(t))) {
            moved = true;
            match target {
                Some(to) => {
                    self.move_tab(sessions, idx, to, None);
                }
                None => {
                    let Some((tab, id)) = self.remove_tab(sessions, idx) else { break };
                    target = self.split_with_tab(sessions, Side::Right, id, tab);
                    if let Node::Split { ratio, .. } = &mut self.root {
                        *ratio = INBOX_RATIO;
                    }
                }
            }
        }
        if moved {
            // The sessions pane shows the session it was on before the stray tab was its
            // active one: the most recently used of what is left.
            if let Some(p) = self.pane_mut(sessions) {
                p.active = p.mru.first().copied().filter(|i| *i < p.tabs.len()).unwrap_or(0);
            }
        }
        for pane in self.panes().into_iter().filter(|p| *p != sessions) {
            self.close_pane_if_empty(pane);
        }
        self.is_inbox()
    }

    /// Puts any tree in the Inbox's shape, as the layout is switched to: keeping every tab with
    /// its id and its name, every terminal tab in one pane, in reading order, and every other
    /// tab in one pane to its right
    /// (none when there is no such tab). Each pane shows the tab the user was on: the tab of its
    /// kind used last in the focused pane, else the first tab that was active in its pane. The focus stays on the kind of tab it was on. Pane ids are kept where they
    /// can be, so what is kept per pane stays with its tabs. True when the tree changed.
    pub fn gather_inbox(&mut self) -> bool {
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
            Node::Split { id, orientation: Orientation::Horizontal, ratio: INBOX_RATIO, first: Box::new(Node::Leaf(sessions)), second: Box::new(Node::Leaf(pane_of(side_id, others))) }
        } else {
            Node::Leaf(sessions)
        };
        self.focused = if on_other && has_side { side_id } else { sessions_id };
        self.maximized = None;
        true
    }

    /// Holds the Inbox's shape while the layout is on: a tab or two out of place is put right
    /// where the tree stands (`settle_inbox`), so a side of several panes keeps its arrangement,
    /// and anything further off is gathered afresh (`gather_inbox`). True when the tree changed.
    pub fn arrange_inbox(&mut self) -> bool {
        if self.is_inbox() {
            return false;
        }
        let mut settled = self.clone();
        if settled.settle_inbox() {
            *self = settled;
            return true;
        }
        self.gather_inbox()
    }

    /// The Grid's arrangement `grid`, kept aside while the Inbox was on, holding the tabs this
    /// tree holds now: a tab closed meanwhile is gone from it (and a pane left empty by that,
    /// unless it is the last), a tab that changed is as it is now, and a tab opened meanwhile is
    /// added to its focused pane. The names are this tree's. The pane of the tab the user is on
    /// now is focused, with that tab shown.
    pub fn regridded(&self, grid: &Tree) -> Tree {
        let now: Vec<(TabId, Tab)> = self.panes().into_iter().filter_map(|id| self.pane(id)).flat_map(|p| p.ids.iter().copied().zip(p.tabs.iter().cloned())).collect();
        let mut out = grid.clone();
        for pane in out.panes() {
            let mut idx = 0;
            while let Some(id) = out.tab_id(pane, idx) {
                match now.iter().find(|(tab_id, _)| *tab_id == id) {
                    Some((_, tab)) => {
                        if let Some(slot) = out.pane_mut(pane).and_then(|p| p.tabs.get_mut(idx)) {
                            *slot = tab.clone();
                        }
                        idx += 1;
                    }
                    None => {
                        out.remove_tab(pane, idx);
                    }
                }
            }
            out.close_pane_if_empty(pane);
        }
        let kept: Vec<TabId> = out.panes().into_iter().filter_map(|id| out.pane(id)).flat_map(|p| p.ids.clone()).collect();
        let target = out.focused;
        for (id, tab) in now.iter().filter(|(id, _)| !kept.contains(id)) {
            if let Some(p) = out.pane_mut(target) {
                p.tabs.push(tab.clone());
                p.ids.push(*id);
                p.mru.push(p.tabs.len() - 1);
            }
        }
        out.names = self.names.clone();
        out.next_id = out.next_id.max(self.next_id);
        out.maximized = None;
        // Back where the user is: the tab they are on now.
        let on = self.pane(self.focused).and_then(|p| p.ids.get(p.active).copied());
        if let Some((pane, idx)) = on.and_then(|id| out.panes().into_iter().find_map(|pane| out.pane(pane)?.ids.iter().position(|t| *t == id).map(|idx| (pane, idx)))) {
            out.activate(pane, idx);
        }
        out
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
        let mut panes = Vec::new();
        self.root.leaves(&mut panes);
        panes.into_iter().find_map(|id| self.pane(id)?.ids.iter().position(|t| *t == tab).map(|i| (id, i)))
    }

    /// Gives every tab an id when a file was written without them (or with a damaged list), and
    /// moves the id counter past every id in use.
    pub fn repair_ids(&mut self) {
        self.next_id = self.next_id.max(self.root.max_id() + 1);
        let mut panes = Vec::new();
        self.root.leaves(&mut panes);
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

    /// Removes a pane and its tabs. The last pane never closes.
    pub fn close_pane(&mut self, pane: PaneId) -> bool {
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
    fn the_inbox_arrangement_gathers_the_terminals_in_one_pane_and_the_rest_beside_it() {
        let doc = |p: &str| Tab::Viewer { path: p.into(), mode: ViewMode::Rendered, presenting: false, outline: false };
        // A grid: [term 1, a.md] | [term 2 (active), term 3] over [b.md].
        let mut t = Tree::new(term(1));
        let left = t.panes()[0];
        t.add_tab(left, doc("/p/a.md"));
        let right = t.split(left, Side::Right, term(2)).unwrap();
        t.add_tab(right, term(3));
        t.activate(right, 0);
        let below = t.split(right, Side::Bottom, doc("/p/b.md")).unwrap();
        t.set_tab_name(t.tab_id(right, 1).unwrap(), Some("Film".into()));
        t.focus(right);
        assert!(!t.is_inbox());
        let ids_before: Vec<TabId> = t.tabs().iter().map(|(pane, idx, _)| t.tab_id(*pane, *idx).unwrap()).collect();
        let grid = t.clone();
        assert!(t.gather_inbox());
        assert!(t.is_inbox());
        let (sessions, side) = t.inbox_panes().unwrap();
        let side = side.expect("two files make a side pane");
        assert_eq!(t.pane(sessions).unwrap().tabs, vec![term(1), term(2), term(3)]);
        assert_eq!(t.pane(side).unwrap().tabs, vec![doc("/p/a.md"), doc("/p/b.md")]);
        assert_eq!(t.pane(sessions).unwrap().active, 1, "the session the user was on");
        assert_eq!(t.focused, sessions);
        assert_eq!((sessions, side), (left, below), "pane ids are kept where they can be");
        let mut ids_after: Vec<TabId> = t.tabs().iter().map(|(pane, idx, _)| t.tab_id(*pane, *idx).unwrap()).collect();
        ids_after.sort();
        let mut sorted = ids_before.clone();
        sorted.sort();
        assert_eq!(ids_after, sorted, "every tab keeps its id");
        assert_eq!(t.tab_name(t.tab_id(sessions, 2).unwrap()), Some("Film"));
        assert!(!t.arrange_inbox(), "already arranged: nothing moves");
        // A terminal that lands among the files, or a file among the sessions, is put right,
        // and a file opened among the sessions leaves the session the user was on showing.
        t.add_tab(side, term(4));
        t.focus(sessions);
        t.add_tab(sessions, doc("/p/c.md"));
        assert!(t.arrange_inbox());
        let (sessions, side) = t.inbox_panes().unwrap();
        assert_eq!(t.pane(sessions).unwrap().tabs.len(), 4);
        let side_pane = t.pane(side.unwrap()).unwrap();
        assert_eq!((side_pane.tabs.len(), side_pane.tabs[side_pane.active].clone()), (3, doc("/p/c.md")), "the file just opened is the one shown");
        assert_eq!(t.focused, side.unwrap(), "and the keys are with it");
        // With no file left the sessions take the window, and with no session an empty pane waits.
        let mut alone = Tree::new(term(1));
        let first = alone.panes()[0];
        alone.split(first, Side::Bottom, term(2)).unwrap();
        assert!(alone.gather_inbox());
        assert_eq!(alone.inbox_panes(), Some((first, None)));
        let mut files = Tree::new(doc("/p/a.md"));
        assert!(files.gather_inbox());
        let (sessions, side) = files.inbox_panes().unwrap();
        assert!(files.pane(sessions).unwrap().tabs.is_empty() && files.pane(side.unwrap()).unwrap().tabs.len() == 1);
        assert_eq!(files.focused, side.unwrap(), "the focus stays on the file it was on");
        // Back to the grid as it was.
        assert_eq!(t.regridded(&grid).panes().len(), 3);
    }

    #[test]
    fn a_session_s_side_is_lifted_out_and_put_back_whole_and_a_stray_tab_is_put_right_in_place() {
        let doc = |p: &str| Tab::Viewer { path: p.into(), mode: ViewMode::Rendered, presenting: false, outline: false };
        // Sessions 1 and 2; a side of two panes, a.md over b.md.
        let mut t = Tree::new(term(1));
        let sessions = t.panes()[0];
        t.add_tab(sessions, term(2));
        t.activate(sessions, 0);
        let top = t.split(sessions, Side::Right, doc("/p/a.md")).unwrap();
        let bottom = t.split(top, Side::Bottom, doc("/p/b.md")).unwrap();
        assert!(t.is_inbox(), "a side may be several panes");
        t.focus(bottom);
        assert_eq!(t.inbox_panes(), Some((sessions, Some(bottom))), "a new tab joins the focused side pane");
        // A file opened among the sessions goes to that pane, and the session shown stays shown.
        t.focus(sessions);
        t.add_tab(sessions, doc("/p/c.md"));
        t.focus(bottom);
        assert!(t.arrange_inbox());
        assert_eq!(t.panes(), vec![sessions, top, bottom], "the side keeps its two panes");
        assert_eq!(t.pane(bottom).unwrap().tabs, vec![doc("/p/b.md"), doc("/p/c.md")]);
        assert_eq!(t.pane(sessions).unwrap().active, 0, "still on the session it was on");
        // A terminal opened in the side joins the sessions and is the one shown.
        t.add_tab(top, term(3));
        assert!(t.arrange_inbox());
        assert_eq!(t.pane(sessions).unwrap().tabs, vec![term(1), term(2), term(3)]);
        assert_eq!((t.pane(sessions).unwrap().active, t.focused), (2, sessions));
        assert_eq!(t.pane(top).unwrap().tabs, vec![doc("/p/a.md")]);
        // The side comes out whole, with the width the sessions had, and goes back as it was.
        let before = t.clone();
        let ids = t.pane(top).unwrap().ids.clone();
        let (side, ratio) = t.inbox_take_side().unwrap();
        assert_eq!(t.panes(), vec![sessions]);
        assert_eq!(side.tab_ids().len(), 3);
        assert!(t.inbox_take_side().is_none(), "nothing left to take");
        assert!(t.is_inbox());
        let mut back = t.clone();
        assert!(back.inbox_put_side(side.clone(), ratio));
        assert_eq!(back.panes(), before.panes());
        assert_eq!(back.pane(top).unwrap().ids, ids);
        assert!(!back.inbox_put_side(side.clone(), ratio), "one side at a time");
        // Another session's side, opened meanwhile, never shares an id with the one put away.
        t.add_tab(sessions, doc("/p/d.md"));
        t.arrange_inbox();
        let fresh: Vec<TabId> = t.panes().into_iter().flat_map(|p| t.pane(p).unwrap().ids.clone()).collect();
        assert!(side.tab_ids().iter().all(|id| !fresh.contains(id)));
        // Absorbed, the side's tabs join the pane a new tab goes to; split off, tabs leave as one pane.
        let d = t.inbox_panes().unwrap().1.unwrap();
        t.inbox_absorb(side.clone(), ratio);
        assert_eq!(t.pane(d).unwrap().tabs.len(), 4);
        let off = t.inbox_split_off(&side.tab_ids()).unwrap();
        assert_eq!(off.tab_ids(), side.tab_ids());
        assert_eq!(t.pane(d).unwrap().tabs, vec![doc("/p/d.md")]);
        assert!(t.inbox_split_off(&[9999]).is_none());
        let mut alone = Tree::new(term(1));
        alone.inbox_absorb(off, 0.6);
        assert_eq!(alone.panes().len(), 2, "with no side yet, the absorbed one becomes it");
    }

    #[test]
    fn the_grid_comes_back_as_it_was_less_what_closed_and_with_what_opened() {
        let doc = |p: &str| Tab::Viewer { path: p.into(), mode: ViewMode::Rendered, presenting: false, outline: false };
        let mut grid = Tree::new(term(1));
        let left = grid.panes()[0];
        let right = grid.split(left, Side::Right, term(2)).unwrap();
        grid.add_tab(right, doc("/p/a.md"));
        let below = grid.split(right, Side::Bottom, doc("/p/b.md")).unwrap();
        grid.focus(left);
        let mut inbox = grid.clone();
        inbox.gather_inbox();
        let (sessions, side) = inbox.inbox_panes().unwrap();
        let side = side.unwrap();
        // In the inbox: b.md closes (its pane was its own), a session and a file open, a tab is
        // renamed, and the user ends on session 2.
        let b = inbox.pane(side).unwrap().tabs.iter().position(|t| *t == doc("/p/b.md")).unwrap();
        inbox.close_tab(side, b);
        let new_term = inbox.add_tab(sessions, term(3));
        let new_term_id = inbox.tab_id(sessions, new_term).unwrap();
        inbox.add_tab(side, doc("/p/c.md"));
        inbox.set_tab_name(new_term_id, Some("New".into()));
        inbox.activate(sessions, 1);
        let back = inbox.regridded(&grid);
        assert_eq!(back.panes(), vec![left, right], "the pane whose only tab closed is gone");
        assert!(back.pane(below).is_none());
        assert_eq!(back.pane(left).unwrap().tabs, vec![term(1), term(3), doc("/p/c.md")], "what opened joins the pane that had the focus");
        assert_eq!(back.pane(right).unwrap().tabs, vec![term(2), doc("/p/a.md")]);
        assert_eq!(back.tab_name(new_term_id), Some("New"));
        assert_eq!((back.focused, back.pane(right).unwrap().active), (right, 0), "on the session the user was on");
        // A fresh id never collides with one handed out in the inbox.
        let mut again = back.clone();
        let idx = again.add_tab(left, term(9));
        assert!(!inbox.tabs().iter().any(|(pane, i, _)| inbox.tab_id(*pane, *i) == again.tab_id(left, idx)));
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
}
