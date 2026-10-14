mod app_nap;
mod chrome;
mod client;
mod closed_stack;
mod config;
mod drag;
mod empty_pane;
mod keys;
mod keymap;
mod layout;
mod motion;
mod palette;
mod pane_view;
mod paths;
mod scroll;
mod split_view;
mod sessions;
mod terminal;
mod terminal_element;
mod theme;
mod tree;
mod workspace;

use futures::channel::mpsc::TryRecvError;
use futures::channel::oneshot;
use futures::StreamExt;
use gpui::{
    div, prelude::*, px, size, App, Application, AsyncApp, Bounds, Context, Div, DragMoveEvent, Entity, FocusHandle,
    Menu, MenuItem, MouseButton, MouseMoveEvent, MouseUpEvent, Window, WindowBounds, WindowOptions,
};
use protocol::{Event, Request, Response, ServerMessage, SessionId};
use sessions::{reconcile, Outcome, Reconcile, SessionManager, SCROLLBACK};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::task::Poll;
use std::time::{Duration, Instant};
use closed_stack::{ClosedStack, ClosedTab, CLOSE_GRACE, CLOSE_TICK, MAX_CLOSED};
use terminal::{Terminal, TerminalCore};
use tree::{Orientation, PaneId, Rect, Side, SplitId, Tab, TabId, Tree};
use workspace::{request_save, SharedSaveQueue, Workspace, FORMAT_VERSION};
use keymap::Quit;

const BANNER_H: f32 = 28.0;
/// Output bytes applied in one batch before the event loop yields to the frame loop.
const BATCH_BUDGET: usize = 1 << 20;


/// Starts every status that reports a `Response::Error`.
const ERROR_PREFIX: &str = "sarkosd error: ";
/// The status until the daemon answers the session list.
const CONNECTING: &str = "connecting to sarkosd";

pub struct Root {
    pub tree: Tree,
    pub sessions: SessionManager,
    pub save_queue: SharedSaveQueue,
    pub project: PathBuf,
    /// Connection-level banner (errors, disconnects). Empty when all is well.
    pub status: String,
    /// The grid each pane last measured while it had no terminal to measure itself.
    pub grids: HashMap<PaneId, (usize, usize)>,
    pub grid_waiters: HashMap<PaneId, Vec<oneshot::Sender<(usize, usize)>>>,
    /// Focus handles for panes that show no terminal (busy or empty).
    pub pane_focus: HashMap<PaneId, FocusHandle>,
    /// A split handle being dragged: the split, its orientation, and the split's bounds.
    pub resizing: Option<(SplitId, Orientation, Rect)>,
    /// The text size new terminals start at, and that panes without a terminal measure with.
    /// Cmd+= and Cmd+- change the focused terminal's own size, not this.
    pub font_size: f32,
    pub line_height: f32,
    /// The name of the text preset in use; saved with the workspace.
    pub text_preset: String,
    /// The area the tree is laid out in, in window pixels, from the last render.
    pub content: Rect,
    /// Pane ids in reading order (the tab bar numbers), computed once per render.
    pub pane_order: Vec<PaneId>,
    /// Terminal tabs closed in the last minute, whose detached sessions Cmd+Shift+T brings back.
    pub closed: ClosedStack,
    /// Where a dragged tab would land if released now. Cleared on drop, on a release anywhere
    /// else, and on the first move after a release outside the window.
    pub drop_target: Option<drag::DropTarget>,
    /// When each empty pane's animation started; settled panes keep an old instant.
    pub empty_anim: HashMap<PaneId, std::time::Instant>,
    /// The basename of `$SHELL`, shown on a tab whose program set no title.
    pub shell_name: String,
    /// The last good config; `apply_config` (Task 6) replaces it.
    pub config: config::Config,
    pub theme: Arc<theme::Theme>,
    /// What macOS reports; `appearance.mode = "system"` follows it.
    pub system_is_dark: bool,
    /// The config or theme problem shown under the connection banner; `None` when all is well.
    /// Kept apart from `status`, which belongs to the connection and clears when it answers.
    pub config_error: Option<String>,
}

/// The name a tab shows before its program sets a title: the basename of `$SHELL`, else "shell".
fn shell_name(shell: Option<&str>) -> String {
    shell
        .and_then(|s| std::path::Path::new(s).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "shell".into())
}

impl Root {
    fn new(client: Arc<client::Client>, project: PathBuf, saved: Option<Workspace>, config: config::Config, theme: theme::Theme, system_is_dark: bool) -> Self {
        // A fresh workspace starts with one pane holding a placeholder session that the launch
        // replaces with a real shell (reconcile sees it is not in the daemon's list).
        let (text_preset, preset) = config::resolve_preset(saved.as_ref().and_then(|w| w.text_preset.as_deref()), &config.font.preset, &config.font.presets);
        let tree = saved.map(|ws| ws.tree).unwrap_or_else(|| Tree::new(Tab::Terminal { session: SessionId(0) }));
        Self {
            tree,
            sessions: SessionManager::new(client.clone(), Some(client), project.clone()),
            save_queue: SharedSaveQueue::default(),
            project,
            status: CONNECTING.into(),
            grids: HashMap::new(),
            grid_waiters: HashMap::new(),
            pane_focus: HashMap::new(),
            resizing: None,
            font_size: preset.size,
            line_height: preset.line_height,
            text_preset,
            content: Rect { x: 0.0, y: 0.0, w: 1200.0, h: 800.0 },
            pane_order: Vec::new(),
            closed: ClosedStack::default(),
            drop_target: None,
            empty_anim: HashMap::new(),
            shell_name: shell_name(std::env::var("SHELL").ok().as_deref()),
            config,
            theme: Arc::new(theme),
            system_is_dark,
            config_error: None,
        }
    }

    /// The theme the config and the current appearance name, loaded from the themes folder.
    pub fn resolve_theme(&self) -> Result<theme::Theme, String> {
        let name = theme::pick_name(self.config.appearance.mode, self.system_is_dark, &self.config.theme);
        theme::load(name, &paths::themes_dir())
    }

    pub fn set_theme(&mut self, theme: theme::Theme, cx: &mut Context<Self>) {
        let palette = Arc::new(theme.palette());
        self.theme = Arc::new(theme);
        for t in self.sessions.terminals.values() {
            t.update(cx, |t, cx| {
                t.core.set_palette(palette.clone());
                cx.notify();
            });
        }
        cx.notify();
    }

    /// macOS switched between light and dark. In "system" mode the theme follows; a theme that
    /// fails to load leaves the current one and says why in the banner.
    pub fn set_system_appearance(&mut self, dark: bool, cx: &mut Context<Self>) {
        self.system_is_dark = dark;
        if self.config.appearance.mode != config::AppearanceMode::System {
            return;
        }
        match self.resolve_theme() {
            Ok(t) if t != *self.theme => self.set_theme(t, cx),
            Ok(_) => {}
            Err(e) => {
                self.config_error = Some(format!("theme: {e}; keeping {}", self.theme.name));
                eprintln!("sarkos: theme: {e}");
                cx.notify();
            }
        }
    }

    pub fn family(&self) -> gpui::SharedString {
        self.config.font.family.clone().into()
    }

    /// Switches every terminal to a preset (spec 7). Per-terminal nudges from Cmd+= are reset.
    pub fn apply_text_preset(&mut self, name: &str, cx: &mut Context<Self>) -> bool {
        let Some(p) = self.config.font.presets.get(name).copied() else { return false };
        self.text_preset = name.to_string();
        self.font_size = p.size;
        self.line_height = p.line_height;
        for t in self.sessions.terminals.values() {
            t.update(cx, |t, cx| {
                t.font_size = p.size;
                t.line_height = p.line_height;
                cx.notify();
            });
        }
        // Panes re-measure at the new cell size before any restore trusts a grid.
        self.grids.clear();
        self.save(cx);
        cx.notify();
        true
    }

    pub fn layout(&self) -> tree::Layout {
        self.tree.layout(self.content)
    }

    pub fn save(&mut self, cx: &mut App) {
        let ws = Workspace { version: FORMAT_VERSION, project: self.project.clone(), tree: self.tree.clone(), text_preset: Some(self.text_preset.clone()) };
        request_save(&self.save_queue, ws, cx);
    }

    /// Drops everything kept per pane. A restore waiting on the pane's grid then looks for its
    /// tab again: in another pane it waits there, and closed it gives up.
    pub fn forget_pane(&mut self, pane: PaneId) {
        self.grids.remove(&pane);
        self.grid_waiters.remove(&pane);
        self.pane_focus.remove(&pane);
        self.empty_anim.remove(&pane);
    }

    /// Forgets per-pane state for panes no longer in the tree, whatever removed them.
    fn forget_closed_panes(&mut self) {
        let gone: Vec<PaneId> = self
            .grids
            .keys()
            .chain(self.grid_waiters.keys())
            .chain(self.pane_focus.keys())
            .chain(self.empty_anim.keys())
            .copied()
            .filter(|p| !self.pane_order.contains(p))
            .collect();
        for pane in gone {
            self.forget_pane(pane);
        }
    }

    pub fn report_grid(&mut self, pane: PaneId, grid: (usize, usize)) {
        self.grids.insert(pane, grid);
        for tx in self.grid_waiters.remove(&pane).unwrap_or_default() {
            let _ = tx.send(grid);
        }
        // A hidden pane (another is maximized) never measures; its restores take this grid.
        let visible: Vec<PaneId> = self.layout().panes.iter().map(|(p, _)| *p).collect();
        let hidden: Vec<PaneId> = self.grid_waiters.keys().copied().filter(|p| !visible.contains(p)).collect();
        for p in hidden {
            for tx in self.grid_waiters.remove(&p).unwrap_or_default() {
                let _ = tx.send(grid);
            }
        }
    }

    /// The grid a terminal in `pane` would get: the pane's current terminal's, else the last
    /// measurement of the pane's placeholder.
    fn pane_grid(&self, pane: PaneId, cx: &App) -> Option<(usize, usize)> {
        self.tree.pane(pane)?;
        let visible = self.layout().panes.iter().any(|(p, _)| *p == pane);
        let own = self.own_grid(pane, cx);
        let maximized = self.tree.maximized.filter(|m| *m != pane).and_then(|m| self.own_grid(m, cx));
        pick_grid(visible, own, maximized, self.grids.values().next().copied())
    }

    fn own_grid(&self, pane: PaneId, cx: &App) -> Option<(usize, usize)> {
        let p = self.tree.pane(pane)?;
        if let Some(Tab::Terminal { session }) = p.tabs.get(p.active) {
            if let Some(t) = self.sessions.terminals.get(session) {
                let core = &t.read(cx).core;
                return Some((core.columns(), core.screen_lines()));
            }
        }
        self.grids.get(&pane).copied()
    }

    pub fn active_terminal(&self, _cx: &App) -> Option<Entity<Terminal>> {
        let p = self.tree.pane(self.tree.focused)?;
        match p.tabs.get(p.active)? {
            Tab::Terminal { session } => self.sessions.terminals.get(session).cloned(),
            Tab::Viewer { .. } => None,
        }
    }

    /// Focuses a pane, exiting another pane's maximize so the focused pane is on screen.
    pub fn focus_pane(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        if self.tree.pane(pane).is_none() {
            return;
        }
        let maximized = self.tree.maximized;
        self.tree.focus(pane);
        if self.tree.maximized != maximized {
            self.save(cx);
        }
        cx.notify();
    }

    pub fn activate_tab(&mut self, pane: PaneId, idx: usize, cx: &mut Context<Self>) {
        self.tree.activate(pane, idx);
        self.save(cx);
        cx.notify();
    }

    /// Closes a tab; the pane stays (spec 4). A terminal's session is detached at once and killed
    /// only after `CLOSE_GRACE`, so Cmd+Shift+T can bring the same shell back.
    pub fn close_tab(&mut self, pane: PaneId, idx: usize, cx: &mut Context<Self>) {
        let Some(id) = self.tree.tab_id(pane, idx) else { return };
        let Some(tab) = self.tree.close_tab(pane, idx) else { return };
        if let Tab::Terminal { session } = tab {
            if session != SessionId(0) {
                if self.sessions.is_restoring(session) {
                    self.sessions.abort_restore(session, id);
                }
                self.sessions.detach(session);
                for old in self.closed.push(ClosedTab { session, pane, closed_at: Instant::now() }) {
                    eprintln!("sarkos: more than {MAX_CLOSED} closed tabs, ending session {}", old.0);
                    self.sessions.kill(old);
                }
            }
            // A shell still starting for this tab is killed when it arrives (`handle_outcome`).
            self.sessions.tab_closed(id);
        }
        self.save(cx);
        cx.notify();
    }

    /// Puts a closed tab's session back in `pane` and reattaches it. The daemon kept it; one that
    /// ended meanwhile shows as ended.
    fn reopen_session(&mut self, pane: PaneId, session: SessionId, cx: &mut Context<Self>) {
        let idx = self.tree.add_tab(pane, Tab::Terminal { session });
        self.tree.focus(pane);
        if let Some(id) = self.tree.tab_id(pane, idx) {
            eprintln!("sarkos: reopening session {} in pane {pane}", session.0);
            self.sessions.attach(session, id, false);
        }
        self.save(cx);
        cx.notify();
    }

    /// A restore whose tab is gone. Ends the session unless something can still show it.
    fn restore_tab_gone(&mut self, session: SessionId, tab: TabId) {
        if self.tree.find_session(session).is_some() {
            // Closed and reopened meanwhile: the new tab's own restore owns the session.
            eprintln!("sarkos: tab {tab} closed during its restore; session {} was reopened elsewhere", session.0);
        } else if self.closed.contains(session) {
            // Closed by the user: already detached, and killed when its grace runs out.
            eprintln!("sarkos: tab {tab} closed during its restore; session {} waits for Cmd+Shift+T", session.0);
            self.sessions.abort_restore(session, tab);
        } else {
            eprintln!("sarkos: tab {tab} closed during its restore, ending session {}", session.0);
            self.sessions.kill(session);
            self.sessions.detach(session);
            self.sessions.abort_restore(session, tab);
        }
    }

    /// A tab whose session could not be shown becomes a fresh shell with the "(restarted)" note,
    /// so no tab is left dead. `why` is logged.
    fn restart_tab(&mut self, tab: TabId, session: SessionId, why: &str, cx: &mut Context<Self>) {
        let Some((pane, idx)) = self.tree.locate(tab) else {
            // Decides itself whether to abort: a reopened tab's restore may own the session now.
            self.restore_tab_gone(session, tab);
            return;
        };
        self.sessions.abort_restore(session, tab);
        eprintln!("sarkos: cannot show session {} in tab {tab} ({why}); starting a new shell there", session.0);
        self.sessions.detach(session);
        self.tree.replace_tab(pane, idx, Tab::Terminal { session: SessionId(0) });
        self.sessions.create(tab, true);
        self.save(cx);
        cx.notify();
    }

    /// Kills the sessions of closed tabs whose grace has run out.
    pub fn expire_closed(&mut self, now: Instant) {
        for session in self.closed.expired(now) {
            eprintln!("sarkos: session {} was not reopened within {}s, ending it", session.0, CLOSE_GRACE.as_secs());
            self.sessions.kill(session);
        }
    }

    /// Kills every closed tab's session, for quitting: nothing could reopen them afterwards.
    /// Returns how many were killed.
    pub fn end_closed_tabs(&mut self) -> usize {
        let sessions = self.closed.drain_all();
        for session in &sessions {
            eprintln!("sarkos: quitting, ending closed tab's session {}", session.0);
            self.sessions.kill(*session);
        }
        sessions.len()
    }

    /// Cmd+, (spec 7): opens the config file in the default text editor. Task 2 creates the file
    /// at launch, so it exists by the time this runs.
    pub fn open_config(&mut self, cx: &mut Context<Self>) {
        let path = paths::config_file();
        if let Err(e) = std::process::Command::new("open").arg("-t").arg(&path).spawn() {
            self.status = format!("cannot open {}: {e}", path.display());
            eprintln!("sarkos: {}", self.status);
            cx.notify();
        }
    }

    /// Lands a dragged tab. The tab is found by id, so a drag that outlived a move of its tab
    /// still lands the right one, and one whose tab closed does nothing. The source pane closes
    /// only when the drag emptied it, and never when it is the last pane (spec 4).
    pub fn drop_tab(&mut self, d: &drag::TabDrag, target: drag::DropTarget, cx: &mut Context<Self>) {
        self.drop_target = None;
        let Some((source, idx)) = self.tree.locate(d.tab) else {
            cx.notify();
            return;
        };
        match target {
            drag::DropTarget::TabBar { pane, at } => {
                // Back onto its own place, `move_tab` changes nothing.
                self.tree.move_tab(source, idx, pane, Some(at));
            }
            drag::DropTarget::Pane { pane, zone: drag::Zone::Center } => {
                if pane == source {
                    cx.notify();
                    return;
                }
                self.tree.move_tab(source, idx, pane, None);
            }
            drag::DropTarget::Pane { pane, zone } => {
                let side = match zone {
                    drag::Zone::Left => Side::Left,
                    drag::Zone::Right => Side::Right,
                    drag::Zone::Top => Side::Top,
                    drag::Zone::Bottom => Side::Bottom,
                    drag::Zone::Center => unreachable!(),
                };
                let lone = self.tree.pane(source).map(|p| p.tabs.len()).unwrap_or(0) == 1;
                if self.tree.pane(pane).is_none() || (pane == source && lone) {
                    cx.notify();
                    return;
                }
                if let Some((id, tab)) = self.tree.take_tab(source, idx) {
                    self.tree.split_with_tab(pane, side, id, tab);
                }
            }
        }
        if self.tree.pane(source).map(|p| p.tabs.is_empty()).unwrap_or(false) {
            self.tree.close_pane(source);
        }
        self.save(cx);
        cx.notify();
    }

    pub fn new_terminal_tab(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        let idx = self.tree.add_tab(pane, Tab::Terminal { session: SessionId(0) });
        self.tree.focus(pane);
        if let Some(id) = self.tree.tab_id(pane, idx) {
            self.sessions.create(id, false);
        }
        self.save(cx);
        cx.notify();
    }

    /// Splits the focused pane and starts a fresh shell in the new one.
    pub fn split_focused(&mut self, side: Side, cx: &mut Context<Self>) {
        let pane = self.tree.focused;
        if let Some(new_pane) = self.tree.split(pane, side, Tab::Terminal { session: SessionId(0) }) {
            if let Some(id) = self.tree.tab_id(new_pane, 0) {
                self.sessions.create(id, false);
            }
            self.save(cx);
            cx.notify();
        }
    }

    /// Executes one keyboard command.
    pub fn run(&mut self, cmd: keymap::Command, window: &mut Window, cx: &mut Context<Self>) {
        use keymap::Command::*;
        let focused = self.tree.focused;
        match cmd {
            FocusPane(i) => {
                if let Some(p) = self.tree.pane_at(i) {
                    self.focus_pane(p, cx);
                }
            }
            FocusDir(side) => {
                if let Some(p) = self.tree.neighbor(focused, side, self.content) {
                    self.focus_pane(p, cx);
                }
            }
            Split(side) => self.split_focused(side, cx),
            ToggleMaximize => {
                self.tree.toggle_maximize(focused);
                self.save(cx);
                cx.notify();
            }
            MoveTabToPane(delta) => {
                let panes = self.tree.panes();
                let Some(i) = self.tree.pane_index(focused) else { return };
                let target = i as isize + delta;
                if target < 0 || target as usize >= panes.len() {
                    return;
                }
                let to = panes[target as usize];
                let Some(p) = self.tree.pane(focused) else { return };
                if p.tabs.is_empty() {
                    return;
                }
                let idx = p.active;
                if self.tree.move_tab(focused, idx, to, None) {
                    self.focus_pane(to, cx);
                    self.save(cx);
                    cx.notify();
                }
            }
            CycleTab(delta) => {
                if let Some(p) = self.tree.pane(focused) {
                    let next = keymap::tab_cycle(p.active, p.tabs.len(), delta);
                    self.activate_tab(focused, next, cx);
                }
            }
            MruTab => {
                if let Some(next) = self.tree.mru_next(focused) {
                    self.activate_tab(focused, next, cx);
                }
            }
            CloseTab => {
                let Some(p) = self.tree.pane(focused) else { return };
                if p.tabs.is_empty() {
                    // Cmd+W on an empty pane closes the pane; the last pane never closes.
                    if self.tree.close_pane(focused) {
                        self.save(cx);
                        cx.notify();
                    }
                    return;
                }
                let idx = p.active;
                self.close_tab(focused, idx, cx);
            }
            ReopenClosedTab => {
                // Within the grace the same session comes back where it was closed; after it, or
                // with nothing closed, a fresh shell opens there instead.
                let Some(closed) = self.closed.pop() else {
                    self.new_terminal_tab(focused, cx);
                    return;
                };
                let pane = if self.tree.pane(closed.pane).is_some() { closed.pane } else { focused };
                if closed.in_grace(Instant::now()) {
                    self.reopen_session(pane, closed.session, cx);
                } else {
                    eprintln!("sarkos: session {} closed more than {}s ago, ending it and opening a new shell", closed.session.0, CLOSE_GRACE.as_secs());
                    self.sessions.kill(closed.session);
                    self.new_terminal_tab(pane, cx);
                }
            }
            CloseAllInPane => {
                let n = self.tree.pane(focused).map(|p| p.tabs.len()).unwrap_or(0);
                for _ in 0..n {
                    self.close_tab(focused, 0, cx);
                }
            }
            MoveTabWithin(delta) => {
                let Some(p) = self.tree.pane(focused) else { return };
                if self.tree.move_tab_within(focused, p.active, delta) {
                    self.save(cx);
                    cx.notify();
                }
            }
            NewTerminal => self.new_terminal_tab(focused, cx),
            ClearScrollback => {
                if let Some(t) = self.active_terminal(cx) {
                    t.update(cx, |t, cx| {
                        t.core.clear_scrollback();
                        cx.notify();
                    });
                }
            }
            FontDelta(d) => {
                // Only the focused terminal changes (spec 7); its element resizes the grid and
                // the PTY on the next paint. Not saved: milestone 2b's presets will be.
                if let Some(t) = self.active_terminal(cx) {
                    t.update(cx, |t, cx| {
                        t.font_size = (t.font_size + d).clamp(10.0, 32.0);
                        cx.notify();
                    });
                    cx.notify();
                }
            }
            OpenConfig => self.open_config(cx),
            TextPreset(name) => {
                self.apply_text_preset(&name, cx);
            }
        }
        let _ = window;
    }

    /// Applies what the daemon answered.
    pub fn handle_outcome(&mut self, outcome: Outcome, saved_layout: bool, cx: &mut Context<Self>) {
        match outcome {
            Outcome::None => {}
            Outcome::Sessions(list) => {
                if self.status == CONNECTING {
                    // The banner goes away, so every pane grows: forget measurements taken under
                    // it so restores wait for the next frame's grid.
                    self.status.clear();
                    self.grids.clear();
                }
                for step in reconcile(&self.tree, &list, &self.project) {
                    match step {
                        Reconcile::Attach { tab, session } => self.sessions.attach(session, tab, false),
                        Reconcile::Create { tab } => self.sessions.create(tab, saved_layout),
                    }
                }
                cx.notify();
            }
            Outcome::Created { tab, session, note } => {
                let Some((pane, idx)) = self.tree.locate(tab) else {
                    // The tab closed while its shell was starting: nobody will show this session.
                    eprintln!("sarkos: tab closed before session {} started, ending it", session.0);
                    self.sessions.kill(session);
                    self.sessions.detach(session);
                    return;
                };
                self.tree.replace_tab(pane, idx, Tab::Terminal { session });
                if note {
                    self.sessions.notes.insert(session);
                }
                self.save(cx);
                self.sessions.attach(session, tab, note);
                cx.notify();
            }
            Outcome::Attached { session, tab, replay, alive, note: _ } => {
                eprintln!("sarkos: attached to session {} ({} replay bytes)", session.0, replay.len());
                self.sessions.begin_restore(session);
                let root = cx.entity();
                cx.spawn(async move |_, cx| {
                    if let Err(e) = restore(&root, session, tab, replay, alive, cx).await {
                        let why = format!("restoring it failed: {e:#}");
                        let _ = root.update(cx, |root, cx| {
                            root.restart_tab(tab, session, &why, cx);
                            cx.notify();
                        });
                    }
                })
                .detach();
            }
            Outcome::Killed { .. } => {}
            Outcome::AttachFailed { session, tab, error } => {
                self.restart_tab(tab, session, &format!("sarkosd could not attach it: {error}"), cx);
            }
            Outcome::Error(e) => {
                self.status = format!("{ERROR_PREFIX}{e}");
                eprintln!("sarkos: {}", self.status);
                cx.notify();
            }
        }
    }
}

/// Parses a replay off the main thread at the pane's grid, then installs the terminal.
/// The tab is followed by id, so it may move or close while the replay is parsed.
async fn restore(root: &Entity<Root>, session: SessionId, tab: TabId, replay: Vec<u8>, alive: bool, cx: &mut AsyncApp) -> anyhow::Result<()> {
    let (cols, rows) = loop {
        let known = root.update(cx, |root, cx| {
            let (pane, _) = root.tree.locate(tab).ok_or_else(|| anyhow::anyhow!("tab {tab} closed before its restore began"))?;
            anyhow::Ok(match root.pane_grid(pane, cx) {
                Some(grid) => Ok(grid),
                None => {
                    let (tx, rx) = oneshot::channel();
                    root.grid_waiters.entry(pane).or_default().push(tx);
                    cx.notify();
                    Err(rx)
                }
            })
        })??;
        match known {
            Ok(grid) => break grid,
            Err(rx) => match rx.await {
                Ok(grid) => break grid,
                // The pane went away; the tab may have moved to another one, so look again.
                Err(_) => continue,
            },
        }
    };
    let started = std::time::Instant::now();
    let (mut core, title) = cx
        .background_spawn(async move {
            let mut core = TerminalCore::new(cols, rows, SCROLLBACK);
            // The previous UI already answered any queries in the replay; keep only the title.
            let title = core.apply_output(&replay).title;
            (core, title)
        })
        .await;
    eprintln!("sarkos: session {} replay parsed at {cols}x{rows} in {:?}", session.0, started.elapsed());
    root.update(cx, |root, cx| {
        if root.tree.locate(tab).is_none() {
            root.restore_tab_gone(session, tab);
            cx.notify();
            return;
        }
        // The PTY still has the previous window's size. Setting this grid's size is a no-op when
        // they match and makes the program redraw when they differ.
        let client = root.sessions.transport.clone();
        client.send(Request::Resize { session, rows: rows as u16, cols: cols as u16 });
        // Cells resolve against the palette at paint, so it is set here rather than before the
        // parse: a theme switch while the replay parsed then cannot leave this terminal stale.
        core.set_palette(Arc::new(root.theme.palette()));
        let terminal = cx.new(|cx| {
            let mut t = Terminal::new(core, session, root.sessions.client.clone().expect("client"), cx);
            t.alive = alive;
            t.title = title.unwrap_or_default();
            t.font_size = root.font_size;
            t.line_height = root.line_height;
            t
        });
        root.sessions.finish_restore(session, terminal, cx);
        root.sessions.ready(tab);
        cx.notify();
    })
}

/// The grid a restore into a pane uses. A hidden pane (another is maximized) never renders, so
/// it would wait forever for its own measurement: it borrows the maximized pane's grid, else any
/// measured grid. A visible pane uses its own and waits (None) until it has one.
fn pick_grid(visible: bool, own: Option<(usize, usize)>, maximized: Option<(usize, usize)>, any: Option<(usize, usize)>) -> Option<(usize, usize)> {
    if visible {
        own
    } else {
        own.or(maximized).or(any)
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = window.viewport_size();
        let theme = self.theme.clone();
        let banners: Vec<String> = [self.status.clone(), self.config_error.clone().unwrap_or_default()].into_iter().filter(|b| !b.is_empty()).collect();
        let banner_h = BANNER_H * banners.len() as f32;
        let (top, bottom) = (chrome::TOP_BAR_H, chrome::STATUS_BAR_H);
        // A window shorter than its chrome would give a negative height; lay out at zero instead.
        let content = Rect {
            x: 0.0,
            y: top + banner_h,
            w: f32::from(size.width).max(0.0),
            h: (f32::from(size.height) - top - bottom - banner_h).max(0.0),
        };
        if content != self.content {
            // Measurements taken at the old size would parse a replay at the wrong grid.
            self.grids.clear();
            self.content = content;
        }
        // Built first: it creates the focus handles of panes that show no terminal.
        let body = split_view::render_tree(self, window, cx);
        // Focus follows the tree: the focused pane's terminal, or the pane's own handle.
        let wanted = match self.active_terminal(cx) {
            Some(t) => Some(t.read(cx).focus.clone()),
            None => self.pane_focus.get(&self.tree.focused).cloned(),
        };
        if let Some(f) = wanted {
            if !f.is_focused(window) {
                window.focus(&f);
            }
        }
        let project_name = self.project.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let sessions = self.sessions.terminals.values().filter(|t| t.read(cx).alive).count();
        let daemon = if self.status.is_empty() { "sarkosd connected" } else { "sarkosd: see banner" };
        let family = self.family();
        let status_bar = chrome::status_bar(&self.project, daemon, sessions, 0, family.clone(), &theme);
        let root_div = div()
            .size_full()
            .key_context("Root")
            .bg(theme.chrome.window_bg.hsla())
            .flex()
            .flex_col()
            .on_drag_move(cx.listener(|root, _ev: &DragMoveEvent<drag::TabDrag>, _window, cx| {
                // Capture listeners run parent first, so this forgets the last target before
                // the bar, tab or pane under the pointer claims it; over none of them, none.
                root.drop_target = None;
                cx.notify();
            }))
            .on_mouse_move(cx.listener(|root, ev: &MouseMoveEvent, _window, cx| {
                if root.drop_target.is_some() && ev.pressed_button != Some(MouseButton::Left) {
                    // A tab drag released outside the window, where no mouse-up reaches us.
                    root.drop_target = None;
                    cx.notify();
                }
                if root.resizing.is_some() && ev.pressed_button != Some(MouseButton::Left) {
                    // Released outside the window, where no mouse-up reaches us: end the drag.
                    root.resizing = None;
                    root.save(cx);
                    cx.notify();
                    return;
                }
                if let Some((split, orientation, area)) = root.resizing {
                    // `area` and the pointer are both in window pixels.
                    let ratio = split_view::ratio_from_pointer(area, orientation, f32::from(ev.position.x), f32::from(ev.position.y));
                    root.tree.set_ratio(split, ratio);
                    root.grids.clear();
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|root, _ev: &MouseUpEvent, _window, cx| {
                    if root.resizing.take().is_some() {
                        root.save(cx);
                    }
                    // A drop stops the event before it gets here; this is a release elsewhere.
                    if root.drop_target.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(chrome::top_bar(&project_name, family.clone(), &theme))
            .children(banners.into_iter().map(|text| banner_bar(text, family.clone(), &theme)))
            .child(div().flex_1().min_h_0().child(body))
            .child(status_bar);
        keymap::attach(root_div, cx)
    }
}

/// What quitting must finish first: closed tabs' sessions are killed (nothing could reopen them
/// later) and the pending layout save is written.
fn flush_on_quit(root: &Entity<Root>, queue: &SharedSaveQueue, client: &client::Client, cx: &mut App) {
    let killed = root.update(cx, |root, _| root.end_closed_tabs());
    // The process may exit before the writer thread sends the Kills; wait for them, briefly.
    if killed > 0 && !client.wait_sent(Duration::from_millis(500)) {
        eprintln!("sarkos: could not send {killed} kill requests before quitting");
    }
    workspace::flush_now(queue);
}

fn banner_bar(text: String, family: gpui::SharedString, theme: &theme::Theme) -> Div {
    div()
        .w_full()
        .h(px(BANNER_H))
        .flex_none()
        .px(px(8.0))
        .flex()
        .items_center()
        .bg(theme.chrome.banner.hsla())
        .text_color(theme.chrome.text.hsla())
        .font_family(family)
        .text_size(px(13.0))
        .child(text)
}

fn output_len(msg: &ServerMessage) -> usize {
    match msg {
        ServerMessage::Event(Event::Output { bytes, .. }) => bytes.len(),
        _ => 0,
    }
}

/// Returns Pending once. A channel with queued messages resolves `next()` immediately, so without
/// this a flood would keep the main thread in the event loop and starve painting and input.
async fn yield_now() {
    let mut yielded = false;
    futures::future::poll_fn(|cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") {
        println!("sarkos {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    // Spec 2.5: the UI thread is user-interactive so macOS schedules it ahead of the heavy work
    // Claude Code's children do.
    let qos = unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0) };
    if qos != 0 {
        eprintln!("sarkos: could not set user-interactive QoS (error {qos})");
    }
    app_nap::hold();
    let (client, mut events) = match client::Client::connect_or_start(&paths::socket_path(), &paths::daemon_exe()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("sarkos: cannot reach sarkosd: {e:#}");
            std::process::exit(1);
        }
    };
    let client = Arc::new(client);
    let project = std::env::current_dir().expect("cwd");
    // Synchronous on purpose: one small file read, done before the window opens, so it cannot
    // stall painting or input. Saves happen on the background executor.
    let saved = workspace::load(&project);
    let save_queue = SharedSaveQueue::default();
    let config_path = paths::config_file();
    let (config, config_error) = match config::load(&config_path) {
        Ok(c) => (c, None),
        Err(e) => {
            eprintln!("sarkos: {e}");
            (config::Config::default(), Some(e.to_string()))
        }
    };

    Application::new().run(move |cx: &mut App| {
        // gpui installs no menu and no quit binding of its own; the Quit action is handled below,
        // once the window exists.
        cx.bind_keys(keymap::bindings());
        cx.set_menus(vec![Menu { name: "Sarkos IDE".into(), items: vec![MenuItem::action("Quit", Quit)] }]);
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let client_for_view = client.clone();
        let view_queue = save_queue.clone();
        let saved_layout = saved.is_some();
        let view_project = project.clone();
        let view_saved = saved.clone();
        let system_is_dark = theme::is_dark(cx.window_appearance());
        let name = theme::pick_name(config.appearance.mode, system_is_dark, &config.theme).to_string();
        let (theme, theme_error) = match theme::load(&name, &paths::themes_dir()) {
            Ok(t) => (t, None),
            Err(e) => {
                eprintln!("sarkos: theme: {e}; using navy");
                (theme::builtin("navy").expect("navy is built in"), Some(format!("theme: {e}")))
            }
        };
        let (view_config, view_theme) = (config, theme);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions { title: Some("Sarkos IDE".into()), ..Default::default() }),
                    // One minimum pane with its border, tab bar and the window's chrome.
                    window_min_size: Some(size(
                        px(tree::MIN_PANE_W + 8.0),
                        px(tree::MIN_PANE_H + pane_view::TAB_BAR_H + chrome::TOP_BAR_H + chrome::STATUS_BAR_H),
                    )),
                    ..Default::default()
                },
                move |window, cx| {
                    let view = cx.new(|_| {
                        let mut root = Root::new(client_for_view, view_project, view_saved, view_config, view_theme, system_is_dark);
                        root.save_queue = view_queue;
                        root
                    });
                    let observed = view.clone();
                    window
                        .observe_window_appearance(move |window, cx| {
                            let dark = theme::is_dark(window.appearance());
                            observed.update(cx, |root, cx| root.set_system_appearance(dark, cx));
                        })
                        .detach();
                    view
                },
            )
            .expect("open window");
        let root = window.entity(cx).expect("root entity");
        // The config's problem first: a bad config file also explains a theme that did not load.
        let launch_error = config_error.or(theme_error);
        root.update(cx, |root, cx| {
            root.config_error = launch_error;
            cx.notify();
        });
        let (quit_root, quit_queue, quit_client) = (root.clone(), save_queue.clone(), client.clone());
        cx.on_action(move |_: &Quit, cx| {
            flush_on_quit(&quit_root, &quit_queue, &quit_client, cx);
            cx.quit();
        });
        // gpui keeps running without windows; quit with the last one so the heartbeat stops and
        // no windowless process lingers.
        let (close_root, close_queue, close_client) = (root.clone(), save_queue.clone(), client.clone());
        cx.on_window_closed(move |cx| {
            if cx.windows().is_empty() {
                flush_on_quit(&close_root, &close_queue, &close_client, cx);
                cx.quit();
            }
        })
        .detach();

        // Closed tabs' sessions end once their grace runs out.
        let tick_root = root.clone();
        cx.spawn(async move |cx| loop {
            cx.background_executor().timer(CLOSE_TICK).await;
            if tick_root.update(cx, |root, _| root.expire_closed(Instant::now())).is_err() {
                break;
            }
        })
        .detach();

        // Heartbeat from the main thread, once a second (spec 2.4).
        let hb_client = client.clone();
        cx.spawn(async move |cx| {
            let mut seq = 0u64;
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                hb_client.send(Request::Heartbeat { seq });
                seq += 1;
            }
        })
        .detach();

        // Ask for the daemon's sessions; the answer reconciles the saved layout against them.
        root.update(cx, |root, _| root.sessions.list());
        let client_for_task = client.clone();
        cx.spawn(async move |cx| {
            let mut closed = false;
            // Whether the daemon has answered anything, for the hint below.
            let mut answered = false;
            while !closed {
                let Some(first) = events.next().await else { break };
                // Take everything already queued, up to the byte budget, so a flood becomes one
                // grid update and one notify per terminal per batch instead of one per message.
                let mut bytes = output_len(&first);
                let mut queue = VecDeque::from([first]);
                while bytes <= BATCH_BUDGET {
                    match events.try_recv() {
                        Ok(msg) => {
                            bytes += output_len(&msg);
                            queue.push_back(msg);
                        }
                        Err(TryRecvError::Closed) => {
                            closed = true;
                            break;
                        }
                        Err(TryRecvError::Empty) => break,
                    }
                }
                let Ok(()) = root.update(cx, |root, cx| {
                    let mut pending: Vec<Event> = Vec::new();
                    for msg in queue.drain(..) {
                        match msg {
                            ServerMessage::Event(ev) => pending.push(ev),
                            ServerMessage::Response { id, response } => {
                                answered = true;
                                // Events that came before this response are applied first, keeping order.
                                if !pending.is_empty() {
                                    root.sessions.route_events(&pending, cx);
                                    pending.clear();
                                }
                                let is_error = matches!(response, Response::Error(_));
                                let outcome = root.sessions.handle_response(id, response);
                                root.handle_outcome(outcome, saved_layout, cx);
                                // A later success means the error was transient.
                                if !is_error && root.status.starts_with(ERROR_PREFIX) {
                                    root.status.clear();
                                    cx.notify();
                                }
                            }
                        }
                    }
                    if !pending.is_empty() {
                        root.sessions.route_events(&pending, cx);
                    }
                }) else {
                    return;
                };
                // The batch's output is applied: let the reader thread pull more from the socket.
                client_for_task.consumed_output(bytes);
                yield_now().await;
            }
            let mut reason = client_for_task.disconnect_reason().unwrap_or_else(|| "daemon connection ended".into());
            if !answered {
                // A daemon built before the protocol version existed cannot decode this UI's
                // Hello and hangs up without a word.
                reason.push_str("; if sarkosd is from an older build, restart it (pkill -x sarkosd), which ends every session");
            }
            eprintln!("sarkos: {reason}");
            let _ = root.update(cx, |root, cx| {
                // An error the daemon sent before hanging up (a protocol mismatch) says why; keep it.
                root.status = if root.status.starts_with(ERROR_PREFIX) { format!("{}; {reason}", root.status) } else { reason };
                cx.notify();
            });
        })
        .detach();
    });
}

#[cfg(test)]
mod tests {
    use super::{pick_grid, shell_name};

    #[test]
    fn shell_name_is_the_basename_of_shell() {
        assert_eq!(shell_name(Some("/bin/zsh")), "zsh");
        assert_eq!(shell_name(Some("/opt/homebrew/bin/fish")), "fish");
        assert_eq!(shell_name(Some("")), "shell");
        assert_eq!(shell_name(None), "shell");
    }

    #[test]
    fn hidden_panes_borrow_a_measured_grid() {
        // A visible pane uses its own grid or waits for one.
        assert_eq!(pick_grid(true, Some((80, 24)), Some((100, 30)), None), Some((80, 24)));
        assert_eq!(pick_grid(true, None, Some((100, 30)), Some((90, 20))), None);
        // A hidden pane takes the maximized pane's grid, else any measured grid.
        assert_eq!(pick_grid(false, None, Some((100, 30)), Some((90, 20))), Some((100, 30)));
        assert_eq!(pick_grid(false, None, None, Some((90, 20))), Some((90, 20)));
        assert_eq!(pick_grid(false, None, None, None), None);
    }
}
