//! The window's side of the targeted channel (spec sections 5 to 8): where an agent's `arkos
//! open` lands, which video tab an `arkos video` op means, and what `arkos panes` lists. The
//! placement rules are pure functions over the tree; `Root::handle_direct` applies them.

use crate::telemetry::{self, AgentCommand};
use crate::config::OpenPlacement;
use crate::tree::{PaneId, Rect, Side, Tab, TabId, Tree, ViewMode};
use crate::video::PlayerState;
use crate::Root;
use gpui::Context;
use protocol::{LoopChange, OpenKind, PaneAction, PaneOutcome, PaneRow, PanesReport, Placement, Request, SessionId, SplitSide, TabRow, VideoOp, VideoState};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long the window waits for a video to load before answering anyway: just under the
/// daemon's wait for the answer, so a slow load is reported as loading rather than lost.
const READY_WAIT: Duration = protocol::CUSTOMIZE_WAIT.saturating_sub(Duration::from_millis(300));
/// How often a waiting answer looks at the video again.
const READY_POLL: Duration = Duration::from_millis(50);

/// The answer to an open whose session has no tab in this window (closed in its grace) and
/// whose placement needs the session's pane.
const NOT_SHOWN: &str = "your session is not shown in this window; pass --pane or --window";

/// The answer when the tab an answer waited for closed before it was sent.
const TAB_CLOSED: &str = "the tab closed before it loaded";

/// What an open does to the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenStep {
    /// A tab already on the file: activate it where it is.
    Reuse { pane: PaneId, idx: usize },
    /// A new tab in this pane.
    AddTo { pane: PaneId },
    /// A new pane split off this one on `side`, with the new tab as its first.
    SplitFrom { pane: PaneId, side: Side },
}

/// Where an Open lands. `anchor` is the calling session's pane (None without one: the focused
/// pane). A plain open (`Here`) is a new tab in the anchor, `Pane(n)` a new tab in pane n and
/// `Split` a new pane split off the anchor. With `reuse`, a tab already on the file anywhere in
/// the window is shown where it is instead, whatever the placement asked (ruling 3, any kind):
/// the one in the pane the open aims at when that pane has one, else the first in reading order.
/// Nothing is ever moved.
#[cfg(test)]
pub fn plan_open(tree: &Tree, path: &Path, place: &Placement, reuse: bool, anchor: Option<PaneId>) -> Result<OpenStep, String> {
    plan_open_in(tree, path, place, reuse, anchor, OpenPlacement::Inplace, Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 })
}

/// `plan_open` under the config's `layout.placement`, which says where a plain open (`Here`)
/// lands: `Inplace` in the anchor, `Neighbor` beside it (`neighbor_step`; `bounds` is the
/// panes' area, to tell which pane is beside which).
pub fn plan_open_in(tree: &Tree, path: &Path, place: &Placement, reuse: bool, anchor: Option<PaneId>, placement: OpenPlacement, bounds: Rect) -> Result<OpenStep, String> {
    let from = anchor.unwrap_or(tree.focused);
    if reuse {
        let aimed = match *place {
            Placement::Pane(n) => (n as usize).checked_sub(1).and_then(|i| tree.pane_at(i)),
            Placement::Here | Placement::Split(_) => Some(from),
        };
        let on = tabs_on_file(tree, path);
        if let Some(&(pane, idx)) = on.iter().find(|(pane, _)| Some(*pane) == aimed).or(on.first()) {
            return Ok(OpenStep::Reuse { pane, idx });
        }
    }
    match *place {
        Placement::Here if placement == OpenPlacement::Neighbor => Ok(neighbor_step(tree, from, bounds)),
        Placement::Here => Ok(OpenStep::AddTo { pane: from }),
        Placement::Pane(n) => {
            let count = tree.panes().len();
            let pane = (n as usize).checked_sub(1).and_then(|i| tree.pane_at(i));
            pane.map(|pane| OpenStep::AddTo { pane }).ok_or_else(|| format!("pane {n} does not exist; this window has {count}"))
        }
        Placement::Split(side) => Ok(OpenStep::SplitFrom { pane: from, side: side_of(side) }),
    }
}

/// Where a plain open lands beside `from`: a new tab in the pane to its right, else in the one
/// to its left, else (a single pane, or panes only above and below) a new pane split off to its
/// right.
pub fn neighbor_step(tree: &Tree, from: PaneId, bounds: Rect) -> OpenStep {
    match tree.neighbor(from, Side::Right, bounds).or_else(|| tree.neighbor(from, Side::Left, bounds)) {
        Some(pane) => OpenStep::AddTo { pane },
        None => OpenStep::SplitFrom { pane: from, side: Side::Right },
    }
}

/// Refuses an open that needs the calling session's pane when the session has no tab in this
/// window (closed, in its minute of grace): a plain open or a split would otherwise land in
/// whichever pane has the focus. `--pane N` names its pane and still works.
pub fn refuse_unshown(session: Option<SessionId>, anchor: Option<PaneId>, place: &Placement) -> Result<(), String> {
    if session.is_some() && anchor.is_none() && matches!(place, Placement::Here | Placement::Split(_)) {
        return Err(NOT_SHOWN.to_string());
    }
    Ok(())
}

/// Every tab on `path`, in reading order: an exact match, else one whose path canonicalizes to
/// it (a tab opened from the sidebar under a symlinked folder such as `/tmp` keeps the path as
/// clicked, while the command line sends the canonical one). A web address (an `http(s)`
/// "path") matches the web tabs on that page (`web::same_url`).
fn tabs_on_file(tree: &Tree, path: &Path) -> Vec<(PaneId, usize)> {
    let text = path.to_string_lossy();
    if text.starts_with("http://") || text.starts_with("https://") {
        return tree.tabs().into_iter().filter(|(_, _, t)| matches!(t, Tab::Browser { url } if crate::web::same_url(url, &text))).map(|(pane, idx, _)| (pane, idx)).collect();
    }
    let exact = tree.tabs_on(path);
    if !exact.is_empty() {
        return exact;
    }
    tree.tabs()
        .into_iter()
        .filter(|(_, _, t)| tab_file(t).map(|p| std::fs::canonicalize(&p).unwrap_or(p)).is_some_and(|p| p == path))
        .map(|(pane, idx, _)| (pane, idx))
        .collect()
}

/// The file a viewer, video or local web tab shows.
fn tab_file(tab: &Tab) -> Option<PathBuf> {
    match tab {
        Tab::Viewer { path, .. } | Tab::Video { path, .. } => Some(path.clone()),
        Tab::Browser { url } => crate::web::file_path(url),
        Tab::Terminal { .. } => None,
    }
}

/// The pane an open leaves focused: where the focus was, unless the agent asked for the tab.
pub fn apply_focus(before: PaneId, new_pane: PaneId, focus: bool) -> PaneId {
    if focus { new_pane } else { before }
}

pub fn side_of(s: SplitSide) -> Side {
    match s {
        SplitSide::Left => Side::Left,
        SplitSide::Right => Side::Right,
        SplitSide::Top => Side::Top,
        SplitSide::Bottom => Side::Bottom,
    }
}

/// What `arkos panes` calls a tab: a viewer on a markdown file (as the viewer decides, so an
/// `.mdx` too) is "markdown", any other viewer "text".
pub fn kind_string(tab: &Tab) -> &'static str {
    match tab {
        Tab::Terminal { .. } => "terminal",
        Tab::Viewer { path, .. } if crate::viewer::is_markdown(path) => "markdown",
        Tab::Viewer { .. } => "text",
        Tab::Browser { .. } => "browser",
        Tab::Video { .. } => "video",
    }
}

/// The window's panes in Cmd+1 order with their tabs. `you` is the asking session; `titles`
/// gives a tab's shown title by pane and index.
pub fn panes_report(tree: &Tree, project: &Path, you: Option<SessionId>, titles: &dyn Fn(PaneId, usize) -> String) -> PanesReport {
    let panes = tree
        .panes()
        .into_iter()
        .enumerate()
        .filter_map(|(i, id)| {
            let p = tree.pane(id)?;
            let tabs = p
                .tabs
                .iter()
                .enumerate()
                .map(|(idx, tab)| {
                    let session = match tab {
                        Tab::Terminal { session, .. } => Some(*session),
                        _ => None,
                    };
                    TabRow {
                        id: p.ids.get(idx).copied().unwrap_or(0),
                        kind: kind_string(tab).to_string(),
                        title: titles(id, idx),
                        active: p.active == idx,
                        you: you.is_some() && session == you,
                        path: tab_file(tab),
                        session,
                        url: match tab {
                            Tab::Browser { url } => Some(url.clone()),
                            _ => None,
                        },
                    }
                })
                .collect();
            Some(PaneRow { n: i as u32 + 1, id, focused: tree.focused == id, tabs })
        })
        .collect();
    PanesReport { project: project.to_path_buf(), panes }
}

/// The one-based Cmd+1 number of `pane`, as `arkos panes` prints it (ruling 6).
pub fn pane_number(tree: &Tree, pane: PaneId) -> u32 {
    tree.pane_index(pane).map_or(0, |i| i as u32 + 1)
}

/// Which video tabs a Video op means: by path, the only one, or the error text of ruling 7.
/// Every video tab on the path is meant when it is open in more than one pane.
pub fn pick_videos(tree: &Tree, path: Option<&Path>, op_is_status: bool) -> Result<Vec<(PaneId, usize)>, String> {
    let videos: Vec<(PaneId, usize)> = tree.tabs().into_iter().filter(|(_, _, t)| matches!(t, Tab::Video { .. })).map(|(pane, idx, _)| (pane, idx)).collect();
    if videos.is_empty() {
        return Err("no video tab open".to_string());
    }
    if let Some(path) = path {
        let on: Vec<(PaneId, usize)> = tabs_on_file(tree, path).into_iter().filter(|t| videos.contains(t)).collect();
        return if on.is_empty() { Err(format!("no video tab shows {}", path.display())) } else { Ok(on) };
    }
    if videos.len() == 1 || op_is_status {
        return Ok(videos);
    }
    let lines: Vec<String> = videos
        .iter()
        .map(|(pane, idx)| {
            let name = tree.pane(*pane).and_then(|p| tab_file(p.tabs.get(*idx)?)).and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
            format!("pane {}: {name}", pane_number(tree, *pane))
        })
        .collect();
    Err(lines.join("\n"))
}

/// Whether a video op waits for the item to load before it is applied: a seek, a step or a loop
/// needs the loaded item (its length, its frames), and one made while it loads would be lost
/// (`ready` seeks back to where the load began, a whole-file loop has no length yet).
pub fn defer_op(state: &PlayerState, op: VideoOp) -> bool {
    matches!(state, PlayerState::Loading | PlayerState::Reloading) && matches!(op, VideoOp::Seek(_) | VideoOp::Step(_) | VideoOp::Loop(_))
}

/// The telemetry name of an action.
pub fn command_of(action: &PaneAction) -> AgentCommand {
    match action {
        PaneAction::Open { kind: OpenKind::Web, .. } => AgentCommand::OpenWeb,
        PaneAction::Open { .. } => AgentCommand::Open,
        PaneAction::Panes => AgentCommand::Panes,
        PaneAction::Video { op, .. } => match op {
            VideoOp::Status => AgentCommand::Status,
            VideoOp::Play => AgentCommand::Play,
            VideoOp::Pause => AgentCommand::Pause,
            VideoOp::Seek(_) => AgentCommand::Seek,
            VideoOp::Step(_) => AgentCommand::Step,
            VideoOp::Loop(_) => AgentCommand::Loop,
            VideoOp::Reload => AgentCommand::Reload,
            VideoOp::Close => AgentCommand::Close,
            VideoOp::Mute(_) => AgentCommand::Mute,
        },
    }
}

/// The tab an open makes. A video carries its start time and a set loop, so the player seeks
/// and loops as the file loads (clamped to its length then) even when the answer is sent first.
pub fn new_tab(path: &Path, kind: OpenKind, at: Option<f64>, loop_change: LoopChange) -> Tab {
    let path = path.to_path_buf();
    match kind {
        OpenKind::Video => {
            let loop_range = match loop_change {
                LoopChange::Set { start, end } if start < end => Some((start.max(0.0), end)),
                _ => None,
            };
            Tab::Video { path, at: at.map(|a| a.max(0.0)), loop_range }
        }
        OpenKind::Markdown { raw: true } => Tab::Viewer { path, mode: ViewMode::Raw, presenting: false, outline: false },
        OpenKind::Markdown { raw: false } | OpenKind::Text => Tab::Viewer { path, mode: ViewMode::Rendered, presenting: false, outline: false },
        OpenKind::Web => Tab::Browser { url: crate::web::web_url_of(&path) },
    }
}

/// What an open or a deferred op asks of a video once it has loaded: a loop, a time, a step,
/// playing or paused. The loop, the time and the step apply only to a loaded item; one that did
/// not load by the deadline (or cannot be played) is reported as it is.
struct Settle {
    tab: TabId,
    at: Option<f64>,
    step: Option<i32>,
    playing: Option<bool>,
    loop_change: LoopChange,
}

impl Settle {
    fn new(tab: TabId) -> Settle {
        Settle { tab, at: None, step: None, playing: None, loop_change: LoopChange::Keep }
    }

    fn is_empty(&self) -> bool {
        self.at.is_none() && self.step.is_none() && self.playing.is_none() && self.loop_change == LoopChange::Keep
    }
}

/// What the answer reports once it is sent.
enum Answer {
    Now(PaneOutcome),
    Opened { tab: TabId, reused: bool, reloaded: bool, new_pane: bool },
    /// The videos an op touched; a closed one's state as it was just before it closed.
    Videos(Vec<(TabId, Option<VideoState>)>),
}

/// A `Direct` whose answer waits for its videos to load.
struct Waiting {
    id: u64,
    command: AgentCommand,
    answer: Answer,
    /// The video tabs whose load the answer waits for.
    loads: Vec<TabId>,
    /// Applied once the videos have loaded.
    then: Vec<Settle>,
    /// One more look before answering, so a seek or a play just made shows in the state.
    tick: bool,
    deadline: Instant,
}

impl Root {
    /// An agent's `Event::Direct` (spec 8): an open placed by the rules of section 5, an op on a
    /// video tab, or the pane list. The answer goes to the daemon as `DirectDone`, after the
    /// video it opened or reloaded has loaded or `READY_WAIT` has passed; the UI thread never
    /// blocks. Nothing moves the keyboard focus unless the open asks for it.
    pub fn handle_direct(&mut self, id: u64, action: PaneAction, session: Option<SessionId>, cx: &mut Context<Self>) {
        let command = command_of(&action);
        let now = Instant::now();
        let answer = match action {
            PaneAction::Open { path, kind, place, at, playing, loop_change, reuse, wait_for_file, focus } => {
                self.direct_open(OpenArgs { path, kind, place, at, playing, loop_change, reuse, wait_for_file, focus, session }, cx)
            }
            PaneAction::Video { path, op } => self.direct_video(path.as_deref(), op, cx),
            PaneAction::Panes => {
                let tree = &self.tree;
                let titles = |pane: PaneId, idx: usize| crate::pane_view::tab_title(self, pane, idx);
                Ok((Answer::Now(PaneOutcome::Panes(panes_report(tree, &self.project, session, &titles))), Vec::new(), Vec::new(), false))
            }
        };
        match answer {
            Err(e) => self.answer_direct(id, command, Err(e)),
            Ok((answer, loads, then, tick)) => {
                let waiting = Waiting { id, command, answer, loads, then, tick, deadline: now + READY_WAIT };
                self.wait_direct(waiting, cx);
            }
        }
    }

    /// An agent's open. In the Inbox what a session opens is that session's own: when the
    /// session is not the one shown, its side is brought into the tree for the open and put
    /// away again after, so the user's own view is left as it was; with `--focus` the session
    /// becomes the one shown instead.
    fn direct_open(&mut self, a: OpenArgs, cx: &mut Context<Self>) -> Result<Prepared, String> {
        let at = a.session.filter(|_| self.is_inbox()).and_then(|s| self.tree.find_session(s));
        let owner = at.and_then(|(pane, idx)| self.tree.tab_id(pane, idx));
        let (shown, focused, focus) = (self.inbox.shown, self.tree.focused, a.focus);
        let elsewhere = owner.is_some() && owner != shown;
        if elsewhere {
            self.inbox.swap(&mut self.tree, owner);
        }
        let out = self.direct_open_here(a, cx);
        if elsewhere {
            match (focus && out.is_ok(), at) {
                (true, Some((pane, idx))) => self.tree.activate_quiet(pane, idx),
                _ => {
                    self.inbox.swap(&mut self.tree, shown);
                    if self.tree.pane(focused).is_some() {
                        self.tree.focused = focused;
                    }
                }
            }
            self.save(cx);
        }
        out
    }

    fn direct_open_here(&mut self, a: OpenArgs, cx: &mut Context<Self>) -> Result<Prepared, String> {
        let anchor = a.session.and_then(|s| self.tree.find_session(s)).map(|(pane, _)| pane);
        refuse_unshown(a.session, anchor, &a.place)?;
        // The command line checked; the file may have gone since.
        if !a.wait_for_file && !a.path.exists() {
            return Err(format!("no such file: {}", a.path.display()));
        }
        let step = plan_open_in(&self.tree, &a.path, &a.place, a.reuse, anchor, self.config.layout.placement, self.content)?;
        let before = self.tree.focused;
        let (pane, idx, reused, new_pane) = match step {
            OpenStep::Reuse { pane, idx } => {
                self.tree.activate_quiet(pane, idx);
                (pane, idx, true, false)
            }
            OpenStep::AddTo { pane } => {
                let tab = new_tab(&a.path, a.kind, a.at, a.loop_change);
                telemetry::pane_open(telemetry::kind_of(&tab), telemetry::Opened::Agent);
                (pane, self.tree.add_tab(pane, tab), false, false)
            }
            OpenStep::SplitFrom { pane, side } => {
                let tab = new_tab(&a.path, a.kind, a.at, a.loop_change);
                let kind = telemetry::kind_of(&tab);
                let new = self.tree.split(pane, side, tab).ok_or_else(|| format!("pane {} cannot be split", pane_number(&self.tree, pane)))?;
                telemetry::pane_open(kind, telemetry::Opened::Agent);
                (new, 0, false, true)
            }
        };
        // In the Inbox a session's files go to the side pane whatever the placement said: the
        // tree is regrouped at once, and the tab found again where it landed.
        let (pane, idx, new_pane) = if self.is_inbox() {
            let opened = self.tree.tab_id(pane, idx);
            let had_side = self.tree.inbox_panes().is_some_and(|(_, side)| side.is_some());
            self.tree.arrange_inbox();
            let at = opened.and_then(|tab| self.tree.panes().into_iter().find_map(|p| self.tree.pane(p)?.ids.iter().position(|t| *t == tab).map(|i| (p, i))));
            let (pane, idx) = at.unwrap_or((pane, idx));
            (pane, idx, !reused && !had_side && self.tree.inbox_panes().is_some_and(|(_, side)| side == Some(pane)))
        } else {
            (pane, idx, new_pane)
        };
        // Whichever layout is on, the tab is its session's: the Inbox hands it to that session.
        let opener = a.session.and_then(|s| self.tree.find_session(s)).and_then(|(p, i)| self.tree.tab_id(p, i));
        if let (false, Some(session), Some(tab)) = (reused, opener, self.tree.tab_id(pane, idx)) {
            self.inbox.owner.insert(tab, session);
        }
        // Opened on purpose: a preview tab (the sidebar's single click) it was is kept.
        if reused && self.tree.tab_id(pane, idx) == self.preview_tab {
            self.preview_tab = None;
        }
        // A split focused its new pane: the keys go back to where they were unless asked.
        self.tree.focused = apply_focus(before, pane, a.focus);
        let tab = self.tree.tab_id(pane, idx).ok_or("the tab did not open")?;
        let shown = self.tree.pane(pane).and_then(|p| p.tabs.get(idx)).cloned();
        if reused && a.kind == (OpenKind::Markdown { raw: true }) && matches!(shown, Some(Tab::Viewer { mode: ViewMode::Rendered, .. })) {
            self.tree.set_viewer_mode(pane, idx, ViewMode::Raw);
            if let Some(v) = self.viewers.get(&tab).cloned() {
                v.update(cx, |v, cx| v.set_mode(ViewMode::Raw, cx));
            }
        }
        if a.focus {
            // Saves, and scrolls the tab into view.
            self.activate_tab(pane, idx, cx);
        } else {
            if let Some(h) = self.bar_scroll.get(&pane) {
                h.scroll_to_item(idx);
            }
            self.save(cx);
            cx.notify();
        }
        if let Some(Tab::Browser { url }) = &shown {
            // A page already shown is loaded again (web pane spec, section 8): the agent says it
            // changed it. A local page reloads only when its file is newer than the one shown.
            let view = self.web_for(tab, url, cx);
            let reloaded = reused && view.update(cx, |v, cx| v.reload_if_changed(cx));
            return Ok((Answer::Opened { tab, reused, reloaded, new_pane }, Vec::new(), Vec::new(), false));
        }
        let Some(Tab::Video { path, .. }) = shown else {
            return Ok((Answer::Opened { tab, reused, reloaded: false, new_pane }, Vec::new(), Vec::new(), false));
        };
        let view = self.video_for(tab, &path, cx);
        // The agent says the file is done: a reused tab whose file moved since it loaded loads it
        // now, without the follow rule's stability wait.
        let fp = crate::config_watch::fingerprint(&path);
        let reloaded = reused && fp.is_some() && view.read(cx).model.shown_fp != fp;
        if reloaded {
            view.update(cx, |v, cx| v.reload_at(fp, cx));
        }
        // A new tab already carries its time and a set loop (`new_tab`).
        let then = if reused {
            Settle { at: a.at, playing: a.playing, loop_change: a.loop_change, ..Settle::new(tab) }
        } else {
            let loop_change = match a.loop_change {
                LoopChange::WholeFile => LoopChange::WholeFile,
                LoopChange::Keep | LoopChange::Set { .. } | LoopChange::Clear => LoopChange::Keep,
            };
            Settle { playing: a.playing, loop_change, ..Settle::new(tab) }
        };
        Ok((Answer::Opened { tab, reused, reloaded, new_pane }, vec![tab], Some(then).into_iter().filter(|s| !s.is_empty()).collect(), false))
    }

    fn direct_video(&mut self, path: Option<&Path>, op: VideoOp, cx: &mut Context<Self>) -> Result<Prepared, String> {
        let picked = pick_videos(&self.tree, path, op == VideoOp::Status)?;
        let tabs: Vec<TabId> = picked.iter().filter_map(|(pane, idx)| self.tree.tab_id(*pane, *idx)).collect();
        let mut touched = Vec::new();
        let mut loads = Vec::new();
        let mut then = Vec::new();
        for tab in tabs {
            let Some((pane, idx)) = self.tree.locate(tab) else { continue };
            let Some(Tab::Video { path, .. }) = self.tree.pane(pane).and_then(|p| p.tabs.get(idx)).cloned() else { continue };
            // A tab never shown yet gets its view now; its first load is waited for like a reload.
            let fresh = !self.videos.contains_key(&tab);
            let view = self.video_for(tab, &path, cx);
            if fresh || op == VideoOp::Reload {
                loads.push(tab);
            }
            if defer_op(&view.read(cx).model.state, op) {
                let settle = match op {
                    VideoOp::Seek(secs) => Settle { at: Some(secs), ..Settle::new(tab) },
                    VideoOp::Step(n) => Settle { step: Some(n), ..Settle::new(tab) },
                    VideoOp::Loop(change) => Settle { loop_change: change, ..Settle::new(tab) },
                    _ => Settle::new(tab),
                };
                then.push(settle);
                if !loads.contains(&tab) {
                    loads.push(tab);
                }
                touched.push((tab, None));
                continue;
            }
            match op {
                VideoOp::Status => {}
                VideoOp::Play => view.update(cx, |v, cx| v.play(cx)),
                VideoOp::Pause => view.update(cx, |v, cx| v.pause(cx)),
                VideoOp::Seek(secs) => view.update(cx, |v, cx| v.seek(secs, cx)),
                VideoOp::Step(n) => view.update(cx, |v, cx| v.step(n, cx)),
                VideoOp::Loop(change) => view.update(cx, |v, cx| v.set_loop(change, cx)),
                VideoOp::Reload => view.update(cx, |v, cx| v.reload_now(cx)),
                VideoOp::Mute(muted) => view.update(cx, |v, cx| v.set_muted(muted, cx)),
                VideoOp::Close => {
                    let state = view.read(cx).status(pane_number(&self.tree, pane), tab);
                    // As a hand close: the closed stack keeps it for Cmd+Shift+T.
                    self.close_tab(pane, idx, cx);
                    touched.push((tab, Some(state)));
                    loads.retain(|t| *t != tab);
                    continue;
                }
            }
            touched.push((tab, None));
        }
        let tick = !matches!(op, VideoOp::Status | VideoOp::Close);
        Ok((Answer::Videos(touched), loads, then, tick))
    }

    /// Answers at once when nothing is loading, else looks again every `READY_POLL` until it
    /// has loaded or the deadline passes.
    fn wait_direct(&mut self, mut waiting: Waiting, cx: &mut Context<Self>) {
        if self.poll_direct(&mut waiting, cx) {
            return;
        }
        cx.spawn(async move |root, cx| loop {
            cx.background_executor().timer(READY_POLL).await;
            match root.update(cx, |root, cx| root.poll_direct(&mut waiting, cx)) {
                Ok(false) => {}
                // Answered, or the window is gone.
                Ok(true) | Err(_) => break,
            }
        })
        .detach();
    }

    /// One look at a waiting answer: true once it has been sent.
    fn poll_direct(&mut self, w: &mut Waiting, cx: &mut Context<Self>) -> bool {
        let late = Instant::now() >= w.deadline;
        let loading = w.loads.iter().any(|t| self.videos.get(t).is_some_and(|v| matches!(v.read(cx).model.state, PlayerState::Loading | PlayerState::Reloading)));
        if loading && !late {
            return false;
        }
        for s in std::mem::take(&mut w.then) {
            if let Some(v) = self.videos.get(&s.tab).cloned() {
                v.update(cx, |v, cx| {
                    if v.model.state == PlayerState::Ready {
                        if s.loop_change != LoopChange::Keep {
                            v.set_loop(s.loop_change, cx);
                        }
                        if let Some(at) = s.at {
                            v.seek(at, cx);
                        }
                        if let Some(n) = s.step {
                            v.step(n, cx);
                        }
                    }
                    // The model keeps a play asked for while loading, and refuses one it cannot do.
                    match s.playing {
                        Some(true) => v.play(cx),
                        Some(false) => v.pause(cx),
                        None => {}
                    }
                });
                w.tick = true;
            }
        }
        if w.tick && !late {
            w.tick = false;
            return false;
        }
        let outcome = self.direct_outcome(w, cx);
        self.answer_direct(w.id, w.command, outcome);
        true
    }

    fn direct_outcome(&self, w: &Waiting, cx: &Context<Self>) -> Result<PaneOutcome, String> {
        match &w.answer {
            Answer::Now(outcome) => Ok(outcome.clone()),
            Answer::Opened { tab, reused, reloaded, new_pane } => {
                let (pane, idx) = self.tree.locate(*tab).ok_or(TAB_CLOSED)?;
                let shown = self.tree.pane(pane).and_then(|p| p.tabs.get(idx)).ok_or(TAB_CLOSED)?;
                let raw = matches!(shown, Tab::Viewer { mode: ViewMode::Raw, .. });
                let video = match shown {
                    Tab::Video { .. } => self.video_state(*tab, cx),
                    _ => None,
                };
                let web = match shown {
                    Tab::Browser { .. } => self.webs.get(tab).map(|v| v.read(cx).status()),
                    _ => None,
                };
                Ok(PaneOutcome::Opened { pane: pane_number(&self.tree, pane), tab: *tab, kind: kind_string(shown).to_string(), reused: *reused, reloaded: *reloaded, new_pane: *new_pane, raw, video, web })
            }
            Answer::Videos(touched) => videos_answer(touched.iter().map(|(tab, kept)| kept.clone().or_else(|| self.video_state(*tab, cx))).collect()),
        }
    }

    /// A video tab's state now, as the model reports it: a reload whose new file could not be
    /// played stays in the error until a later cut loads, the old item still showing.
    fn video_state(&self, tab: TabId, cx: &Context<Self>) -> Option<VideoState> {
        let (pane, _) = self.tree.locate(tab)?;
        Some(self.videos.get(&tab)?.read(cx).status(pane_number(&self.tree, pane), tab))
    }

    fn answer_direct(&self, id: u64, command: AgentCommand, outcome: Result<PaneOutcome, String>) {
        telemetry::agent_pane(command, outcome.is_ok());
        // The event came through the client, so there is one; the daemon matches the id.
        if let Some(client) = &self.sessions.client {
            client.send(Request::DirectDone { id, outcome });
        }
    }
}

/// An open's fields, with the asking session.
struct OpenArgs {
    path: PathBuf,
    kind: OpenKind,
    place: Placement,
    at: Option<f64>,
    playing: Option<bool>,
    loop_change: LoopChange,
    reuse: bool,
    wait_for_file: bool,
    focus: bool,
    session: Option<SessionId>,
}

/// The answer to a video op from the state of each tab it touched (None: closed since). With
/// none left there is nothing to report, which is an error as for an open, not an empty success.
fn videos_answer(states: Vec<Option<VideoState>>) -> Result<PaneOutcome, String> {
    let left: Vec<VideoState> = states.into_iter().flatten().collect();
    if left.is_empty() {
        return Err(TAB_CLOSED.to_string());
    }
    Ok(PaneOutcome::Videos(left))
}

/// What a handled action leaves to answer: the answer, the videos to wait for, what to apply
/// once they load, and whether to look once more before answering.
type Prepared = (Answer, Vec<TabId>, Vec<Settle>, bool);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::ViewMode;
    use std::path::PathBuf;

    fn terminal(n: u64) -> Tab {
        Tab::Terminal { session: SessionId(n), cwd: None }
    }

    fn viewer(p: &str) -> Tab {
        Tab::Viewer { path: p.into(), mode: ViewMode::default(), presenting: false, outline: false }
    }

    fn video(p: &str) -> Tab {
        Tab::Video { path: p.into(), at: None, loop_range: None }
    }

    /// Applies a step the way the window does, returning the pane the tab is in.
    fn apply(tree: &mut Tree, step: OpenStep, tab: Tab) -> PaneId {
        match step {
            OpenStep::Reuse { pane, idx } => {
                tree.activate_quiet(pane, idx);
                pane
            }
            OpenStep::AddTo { pane } => {
                tree.add_tab(pane, tab);
                pane
            }
            OpenStep::SplitFrom { pane, side } => tree.split(pane, side, tab).unwrap(),
        }
    }

    #[test]
    fn a_plain_open_is_a_new_active_tab_in_the_agents_own_pane() {
        // The agent runs in the left pane; the user last clicked the right one.
        let mut tree = Tree::new(terminal(1));
        let agent = tree.panes()[0];
        let other = tree.split(agent, Side::Right, terminal(2)).unwrap();
        assert_eq!(tree.focused, other);
        let before = tree.focused;
        let step = plan_open(&tree, Path::new("/p/plan.md"), &Placement::Here, true, Some(agent)).unwrap();
        assert_eq!(step, OpenStep::AddTo { pane: agent });
        let pane = apply(&mut tree, step, viewer("/p/plan.md"));
        assert_eq!(tree.panes(), vec![agent, other], "no pane is split off");
        let p = tree.pane(agent).unwrap();
        assert_eq!(p.tabs, vec![terminal(1), viewer("/p/plan.md")]);
        assert_eq!(p.active, 1, "the new tab is the one the pane shows");
        assert_eq!(apply_focus(before, pane, false), other, "the keys stay where they were");
        assert_eq!(apply_focus(before, pane, true), agent, "--focus hands them to the tab");
    }

    #[test]
    fn a_neighbor_open_goes_to_the_pane_on_the_right_then_the_left_and_splits_right_with_neither() {
        let bounds = Rect { x: 0.0, y: 0.0, w: 1200.0, h: 800.0 };
        let plan = |tree: &Tree, from: PaneId, place: Placement| plan_open_in(tree, Path::new("/p/a.md"), &place, true, Some(from), OpenPlacement::Neighbor, bounds).unwrap();
        // A single pane: a new pane to its right, and from then on that pane takes the tabs.
        let mut tree = Tree::new(terminal(1));
        let agent = tree.panes()[0];
        let step = plan(&tree, agent, Placement::Here);
        assert_eq!(step, OpenStep::SplitFrom { pane: agent, side: Side::Right });
        let right = apply(&mut tree, step, viewer("/p/a.md"));
        assert_eq!(plan_open_in(&tree, Path::new("/p/b.md"), &Placement::Here, true, Some(agent), OpenPlacement::Neighbor, bounds), Ok(OpenStep::AddTo { pane: right }));
        // From the rightmost pane the neighbour is the one on its left.
        assert_eq!(plan_open_in(&tree, Path::new("/p/b.md"), &Placement::Here, true, Some(right), OpenPlacement::Neighbor, bounds), Ok(OpenStep::AddTo { pane: agent }));
        // A file already open is shown where it is, and `--pane` and `--split` say their own.
        assert_eq!(plan(&tree, agent, Placement::Here), OpenStep::Reuse { pane: right, idx: 0 });
        assert_eq!(plan_open_in(&tree, Path::new("/p/b.md"), &Placement::Pane(1), true, Some(agent), OpenPlacement::Neighbor, bounds), Ok(OpenStep::AddTo { pane: agent }));
        assert_eq!(plan_open_in(&tree, Path::new("/p/b.md"), &Placement::Split(SplitSide::Bottom), true, Some(agent), OpenPlacement::Neighbor, bounds), Ok(OpenStep::SplitFrom { pane: agent, side: Side::Bottom }));
        // Panes only above and below have no neighbour beside them: a split to the right.
        let mut stacked = Tree::new(terminal(1));
        let top = stacked.panes()[0];
        stacked.split(top, Side::Bottom, terminal(2)).unwrap();
        assert_eq!(plan_open_in(&stacked, Path::new("/p/b.md"), &Placement::Here, true, Some(top), OpenPlacement::Neighbor, bounds), Ok(OpenStep::SplitFrom { pane: top, side: Side::Right }));
        // In place, the config's default, is the session's own pane as before.
        assert_eq!(plan_open_in(&tree, Path::new("/p/b.md"), &Placement::Here, true, Some(agent), OpenPlacement::Inplace, bounds), Ok(OpenStep::AddTo { pane: agent }));
    }

    #[test]
    fn a_plain_open_never_splits_however_often_it_runs() {
        let mut tree = Tree::new(terminal(1));
        let agent = tree.panes()[0];
        for f in ["/p/a.mp4", "/p/b.md", "/p/c.rs"] {
            let step = plan_open(&tree, Path::new(f), &Placement::Here, true, Some(agent)).unwrap();
            assert_eq!(step, OpenStep::AddTo { pane: agent });
            apply(&mut tree, step, viewer(f));
        }
        assert_eq!(tree.panes(), vec![agent]);
        assert_eq!(tree.pane(agent).unwrap().tabs.len(), 4);
        assert_eq!(tree.pane(agent).unwrap().active, 3);
    }

    #[test]
    fn without_a_session_a_plain_open_goes_into_the_focused_pane() {
        let mut tree = Tree::new(terminal(1));
        let first = tree.panes()[0];
        let second = tree.split(first, Side::Right, terminal(2)).unwrap();
        assert_eq!(tree.focused, second);
        assert_eq!(plan_open(&tree, Path::new("/p/a.mp4"), &Placement::Here, true, None), Ok(OpenStep::AddTo { pane: second }));
        assert_eq!(plan_open(&tree, Path::new("/p/a.mp4"), &Placement::Split(SplitSide::Left), true, None), Ok(OpenStep::SplitFrom { pane: second, side: Side::Left }));
        tree.focused = first;
        assert_eq!(plan_open(&tree, Path::new("/p/a.mp4"), &Placement::Here, true, None), Ok(OpenStep::AddTo { pane: first }));
    }

    #[test]
    fn a_session_not_shown_in_the_window_is_refused_for_its_own_pane_but_not_for_pane_n() {
        let tree = Tree::new(terminal(1));
        let pane = tree.panes()[0];
        let gone = Some(SessionId(9));
        assert_eq!(refuse_unshown(gone, None, &Placement::Here), Err(NOT_SHOWN.to_string()));
        assert_eq!(refuse_unshown(gone, None, &Placement::Split(SplitSide::Right)), Err(NOT_SHOWN.to_string()));
        assert_eq!(refuse_unshown(gone, None, &Placement::Pane(1)), Ok(()));
        assert_eq!(refuse_unshown(Some(SessionId(1)), Some(pane), &Placement::Here), Ok(()), "a session shown here");
        assert_eq!(refuse_unshown(None, None, &Placement::Here), Ok(()), "no session: the focused pane");
    }

    #[test]
    fn a_file_already_open_in_the_agents_pane_is_shown_there_and_not_moved() {
        // The user opened cut.mp4 from the sidebar into the agent's pane, then went back to the agent.
        let mut tree = Tree::new(terminal(1));
        let agent = tree.panes()[0];
        tree.add_tab(agent, video("/p/cut.mp4"));
        let id = tree.tab_id(agent, 1).unwrap();
        tree.activate_quiet(agent, 0);
        // Whatever the placement asked: the rule wins over --pane and --split.
        for place in [Placement::Here, Placement::Pane(1), Placement::Split(SplitSide::Right)] {
            assert_eq!(plan_open(&tree, Path::new("/p/cut.mp4"), &place, true, Some(agent)), Ok(OpenStep::Reuse { pane: agent, idx: 1 }), "{place:?}");
        }
        apply(&mut tree, OpenStep::Reuse { pane: agent, idx: 1 }, video("/p/cut.mp4"));
        assert_eq!(tree.panes(), vec![agent], "no pane is made");
        assert_eq!(tree.locate(id), Some((agent, 1)), "the same tab, where it was");
        assert_eq!(tree.pane(agent).unwrap().tabs, vec![terminal(1), video("/p/cut.mp4")]);
        assert_eq!(tree.pane(agent).unwrap().active, 1, "and it is the one the pane shows");
    }

    #[test]
    fn a_file_already_open_in_another_pane_is_shown_there() {
        let mut tree = Tree::new(terminal(1));
        let agent = tree.panes()[0];
        let other = tree.split(agent, Side::Right, viewer("/p/notes.rs")).unwrap();
        tree.add_tab(other, viewer("/p/a.md"));
        tree.activate_quiet(other, 0);
        tree.focused = agent;
        for place in [Placement::Here, Placement::Pane(1), Placement::Split(SplitSide::Bottom)] {
            assert_eq!(plan_open(&tree, Path::new("/p/a.md"), &place, true, Some(agent)), Ok(OpenStep::Reuse { pane: other, idx: 1 }), "{place:?}");
        }
        assert_eq!(plan_open(&tree, Path::new("/p/a.md"), &Placement::Here, true, None), Ok(OpenStep::Reuse { pane: other, idx: 1 }), "without a session too");
        apply(&mut tree, OpenStep::Reuse { pane: other, idx: 1 }, viewer("/p/a.md"));
        assert_eq!(tree.pane(other).unwrap().active, 1);
        assert_eq!(tree.focused, agent, "showing it there does not move the keys");
        // --new makes another tab, placed as though none were open.
        assert_eq!(plan_open(&tree, Path::new("/p/a.md"), &Placement::Here, false, Some(agent)), Ok(OpenStep::AddTo { pane: agent }));
        assert_eq!(plan_open(&tree, Path::new("/p/a.md"), &Placement::Split(SplitSide::Right), false, Some(agent)), Ok(OpenStep::SplitFrom { pane: agent, side: Side::Right }));
    }

    #[test]
    fn a_file_open_in_several_panes_is_shown_in_the_pane_the_open_aims_at() {
        let mut tree = Tree::new(terminal(1));
        let agent = tree.panes()[0];
        tree.add_tab(agent, video("/p/cut.mp4"));
        let other = tree.split(agent, Side::Right, video("/p/cut.mp4")).unwrap();
        let third = tree.split(other, Side::Bottom, terminal(2)).unwrap();
        let n = |p| pane_number(&tree, p);
        // The agent's own pane holds one: a plain open shows that one.
        assert_eq!(plan_open(&tree, Path::new("/p/cut.mp4"), &Placement::Here, true, Some(agent)), Ok(OpenStep::Reuse { pane: agent, idx: 1 }));
        // --pane N aims at the other pane, which holds one too.
        assert_eq!(plan_open(&tree, Path::new("/p/cut.mp4"), &Placement::Pane(n(other)), true, Some(agent)), Ok(OpenStep::Reuse { pane: other, idx: 0 }));
        // A pane with none on the file: the first in reading order.
        assert_eq!(plan_open(&tree, Path::new("/p/cut.mp4"), &Placement::Pane(n(third)), true, Some(agent)), Ok(OpenStep::Reuse { pane: agent, idx: 1 }));
        assert_eq!(plan_open(&tree, Path::new("/p/cut.mp4"), &Placement::Here, true, Some(third)), Ok(OpenStep::Reuse { pane: agent, idx: 1 }));
    }

    #[test]
    fn pane_n_in_and_out_of_range() {
        let mut tree = Tree::new(terminal(1));
        let first = tree.panes()[0];
        let second = tree.split(first, Side::Right, terminal(2)).unwrap();
        let plan = |place| plan_open(&tree, Path::new("/p/a.mp4"), &place, true, Some(first));
        assert_eq!(plan(Placement::Pane(1)), Ok(OpenStep::AddTo { pane: first }));
        assert_eq!(plan(Placement::Pane(2)), Ok(OpenStep::AddTo { pane: second }));
        assert_eq!(plan(Placement::Pane(3)), Err("pane 3 does not exist; this window has 2".to_string()));
        assert_eq!(plan(Placement::Pane(0)), Err("pane 0 does not exist; this window has 2".to_string()));
    }

    #[test]
    fn a_split_side_is_honoured_and_its_new_pane_does_not_take_the_keys() {
        let tree = Tree::new(terminal(1));
        let agent = tree.panes()[0];
        for (side, want) in [(SplitSide::Left, Side::Left), (SplitSide::Right, Side::Right), (SplitSide::Top, Side::Top), (SplitSide::Bottom, Side::Bottom)] {
            assert_eq!(side_of(side), want);
            let step = plan_open(&tree, Path::new("/p/a.mp4"), &Placement::Split(side), true, Some(agent)).unwrap();
            assert_eq!(step, OpenStep::SplitFrom { pane: agent, side: want });
        }
        let mut tree = Tree::new(terminal(1));
        let agent = tree.panes()[0];
        let before = tree.focused;
        let step = plan_open(&tree, Path::new("/p/cut.mp4"), &Placement::Split(SplitSide::Right), true, Some(agent)).unwrap();
        let new = apply(&mut tree, step, video("/p/cut.mp4"));
        // The split focused its new pane; the window puts it back unless --focus.
        assert_eq!(tree.focused, new);
        assert_eq!(apply_focus(before, new, false), agent);
        assert_eq!(apply_focus(before, new, true), new);
    }

    #[test]
    fn panes_report_numbers_panes_in_cmd_order_and_marks_you_active_and_focused() {
        let mut tree = Tree::new(terminal(1));
        let left = tree.panes()[0];
        tree.add_tab(left, terminal(2));
        let right = tree.split(left, Side::Right, video("/p/cut.mp4")).unwrap();
        tree.add_tab(right, viewer("/p/notes.md"));
        tree.add_tab(right, viewer("/p/main.rs"));
        tree.add_tab(right, Tab::Browser { url: String::new() });
        tree.activate_quiet(right, 0);
        tree.focused = left;
        let titles = |pane: PaneId, idx: usize| format!("t{pane}.{idx}");
        let report = panes_report(&tree, Path::new("/proj"), Some(SessionId(1)), &titles);
        assert_eq!(report.project, PathBuf::from("/proj"));
        assert_eq!(report.panes.len(), 2);
        let (p1, p2) = (&report.panes[0], &report.panes[1]);
        assert_eq!((p1.n, p1.id, p1.focused), (1, left, true));
        assert_eq!((p2.n, p2.id, p2.focused), (2, right, false));
        let row = |p: &PaneRow, i: usize| p.tabs[i].clone();
        assert_eq!(
            row(p1, 0),
            TabRow { id: tree.tab_id(left, 0).unwrap(), kind: "terminal".into(), title: format!("t{left}.0"), active: false, you: true, path: None, session: Some(SessionId(1)), url: None }
        );
        assert_eq!((row(p1, 1).active, row(p1, 1).you, row(p1, 1).session), (true, false, Some(SessionId(2))));
        assert_eq!((row(p2, 0).kind.as_str(), row(p2, 0).active, row(p2, 0).path), ("video", true, Some(PathBuf::from("/p/cut.mp4"))));
        assert_eq!((row(p2, 1).kind.as_str(), row(p2, 1).path), ("markdown", Some(PathBuf::from("/p/notes.md"))));
        assert_eq!((row(p2, 2).kind.as_str(), row(p2, 2).session), ("text", None));
        assert_eq!((row(p2, 3).kind.as_str(), row(p2, 3).path), ("browser", None));
        assert_eq!(row(p2, 3).title, format!("t{right}.3"));
        // Without a session no tab is "you".
        let report = panes_report(&tree, Path::new("/proj"), None, &titles);
        assert!(report.panes.iter().flat_map(|p| &p.tabs).all(|t| !t.you));
    }

    #[test]
    fn a_web_address_already_shown_is_reused_whatever_its_slash_or_fragment() {
        let mut tree = Tree::new(terminal(1));
        let left = tree.focused;
        let right = tree.split(left, Side::Right, Tab::Browser { url: "http://localhost:4173".into() }).unwrap();
        assert!(matches!(plan_open(&tree, Path::new("http://localhost:4173/"), &Placement::Here, true, Some(left)), Ok(OpenStep::Reuse { pane, idx: 0 }) if pane == right));
        assert!(matches!(plan_open(&tree, Path::new("http://localhost:4173/#top"), &Placement::Here, true, Some(left)), Ok(OpenStep::Reuse { pane, idx: 0 }) if pane == right));
        assert!(matches!(plan_open(&tree, Path::new("http://localhost:4173/other"), &Placement::Here, true, Some(left)), Ok(OpenStep::AddTo { pane }) if pane == left), "another page is a new tab");
        assert!(matches!(plan_open(&tree, Path::new("http://localhost:4173/"), &Placement::Here, false, Some(left)), Ok(OpenStep::AddTo { pane }) if pane == left), "--new");
        assert_eq!(new_tab(Path::new("https://a.dev/x"), OpenKind::Web, None, LoopChange::Keep), Tab::Browser { url: "https://a.dev/x".into() });
        assert_eq!(new_tab(Path::new("/p/out/r.html"), OpenKind::Web, None, LoopChange::Keep), Tab::Browser { url: "file:///p/out/r.html".into() });
        assert_eq!(kind_string(&Tab::Browser { url: String::new() }), "browser");
        let open = |kind| PaneAction::Open { path: "/p/a".into(), kind, place: Placement::Here, at: None, playing: None, loop_change: LoopChange::Keep, reuse: true, wait_for_file: false, focus: false };
        assert_eq!(command_of(&open(OpenKind::Web)), AgentCommand::OpenWeb, "a page is counted apart from a file");
        assert_eq!(command_of(&open(OpenKind::Text)), AgentCommand::Open);
        let report = panes_report(&tree, Path::new("/p"), None, &|_, _| "t".into());
        assert_eq!(report.panes[1].tabs[0].url.as_deref(), Some("http://localhost:4173"));
        assert_eq!(report.panes[1].tabs[0].path, None, "a remote page has no file");
    }

    #[test]
    fn a_new_tab_carries_its_kind_its_time_and_a_set_loop() {
        let p = Path::new("/p/cut.mp4");
        let set = LoopChange::Set { start: 12.0, end: 18.0 };
        assert_eq!(new_tab(p, OpenKind::Video, Some(12.0), set), Tab::Video { path: p.into(), at: Some(12.0), loop_range: Some((12.0, 18.0)) });
        // The whole file needs the length, so it waits for the load; an empty range sets nothing.
        assert_eq!(new_tab(p, OpenKind::Video, None, LoopChange::WholeFile), video("/p/cut.mp4"));
        assert_eq!(new_tab(p, OpenKind::Video, Some(-1.0), LoopChange::Set { start: 5.0, end: 5.0 }), Tab::Video { path: p.into(), at: Some(0.0), loop_range: None });
        let md = Path::new("/p/notes.md");
        assert_eq!(new_tab(md, OpenKind::Markdown { raw: true }, None, LoopChange::Keep), Tab::Viewer { path: md.into(), mode: ViewMode::Raw, presenting: false, outline: false });
        assert_eq!(new_tab(md, OpenKind::Markdown { raw: false }, None, LoopChange::Keep), viewer("/p/notes.md"));
        assert_eq!(new_tab(Path::new("/p/a.rs"), OpenKind::Text, None, LoopChange::Keep), viewer("/p/a.rs"));
    }

    #[test]
    fn a_seek_a_step_or_a_loop_waits_for_the_item_to_load() {
        let needs = [VideoOp::Seek(3.0), VideoOp::Step(1), VideoOp::Loop(LoopChange::WholeFile), VideoOp::Loop(LoopChange::Clear)];
        let never = [VideoOp::Status, VideoOp::Play, VideoOp::Pause, VideoOp::Reload, VideoOp::Close, VideoOp::Mute(true)];
        for state in [PlayerState::Loading, PlayerState::Reloading] {
            for op in needs {
                assert!(defer_op(&state, op), "{op:?} while {state:?}");
            }
            for op in never {
                assert!(!defer_op(&state, op), "{op:?} while {state:?}");
            }
        }
        for state in [PlayerState::Ready, PlayerState::Waiting, PlayerState::Unplayable("no".into())] {
            for op in needs.into_iter().chain(never) {
                assert!(!defer_op(&state, op), "{op:?} while {state:?}");
            }
        }
    }

    #[test]
    fn a_video_op_whose_tabs_all_closed_while_it_waited_is_an_error() {
        let v = |pane: u32| VideoState {
            pane,
            tab: 9,
            path: "/p/a.mp4".into(),
            duration: Some(10.0),
            position: 0.0,
            playing: false,
            muted: false,
            loop_range: None,
            width: 0,
            height: 0,
            state: "ready".into(),
            error: None,
            file_mtime: None,
        };
        assert_eq!(videos_answer(vec![None]), Err("the tab closed before it loaded".to_string()));
        assert_eq!(videos_answer(vec![]), Err("the tab closed before it loaded".to_string()));
        // One of two closed: the other is still reported.
        assert_eq!(videos_answer(vec![None, Some(v(2))]), Ok(PaneOutcome::Videos(vec![v(2)])));
    }

    #[test]
    fn pick_videos_follows_ruling_7() {
        let mut tree = Tree::new(terminal(1));
        let left = tree.panes()[0];
        assert_eq!(pick_videos(&tree, None, false), Err("no video tab open".to_string()));
        assert_eq!(pick_videos(&tree, None, true), Err("no video tab open".to_string()));
        let right = tree.split(left, Side::Right, video("/p/a.mp4")).unwrap();
        assert_eq!(pick_videos(&tree, None, false), Ok(vec![(right, 0)]));
        tree.add_tab(left, video("/p/b.mov"));
        assert_eq!(pick_videos(&tree, None, false), Err("pane 1: b.mov\npane 2: a.mp4".to_string()));
        assert_eq!(pick_videos(&tree, None, true), Ok(vec![(left, 1), (right, 0)]));
        assert_eq!(pick_videos(&tree, Some(Path::new("/p/a.mp4")), false), Ok(vec![(right, 0)]));
        assert_eq!(pick_videos(&tree, Some(Path::new("/p/c.mp4")), false), Err("no video tab shows /p/c.mp4".to_string()));
    }
}
