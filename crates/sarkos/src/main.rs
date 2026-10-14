mod app_nap;
mod chrome;
mod client;
mod closed_stack;
mod command_palette;
mod config;
mod config_watch;
mod connection;
mod drag;
mod empty_pane;
mod explorer;
mod explorer_watch;
mod find;
mod highlight;
mod hook_cli;
mod keys;
mod keymap;
mod links;
mod launch_agent;
mod layout;
mod line_edit;
mod motion;
mod palette;
mod pane_view;
mod paths;
mod pulse;
mod scroll;
mod split_view;
mod sessions;
mod sidebar;
mod terminal;
mod terminal_element;
mod theme;
mod tree;
mod update;
mod viewer;
mod viewer_element;
mod workspace;

use futures::channel::oneshot;
use gpui::{
    div, prelude::*, px, size, App, Application, AsyncApp, Bounds, Context, Div, DragMoveEvent, Entity, FocusHandle,
    KeyDownEvent, Menu, MenuItem, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Window, WindowBounds, WindowOptions,
};
use protocol::{Request, SessionId};
use sessions::{reconcile, Outcome, Reconcile, SessionManager};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use closed_stack::{Closed, ClosedStack, ClosedTab, CLOSE_GRACE, CLOSE_TICK, MAX_CLOSED};
use terminal::{Terminal, TerminalCore};
use viewer::Viewer;
use tree::{Orientation, PaneId, Rect, Side, SplitId, Tab, TabId, Tree};
use workspace::{request_save, SharedSaveQueue, SidebarSaved, Workspace, FORMAT_VERSION};
use keymap::Quit;

const BANNER_H: f32 = 28.0;

/// Starts every status that reports a `Response::Error`.
const ERROR_PREFIX: &str = "sarkosd error: ";
/// The status until the daemon answers the session list.
const CONNECTING: &str = "connecting to sarkosd";
/// How long a one-off notice stays up when no command replaces or clears it first.
const NOTICE_FOR: Duration = Duration::from_secs(6);
/// How often kills the daemon refused during an upgrade are sent again.
const KILL_RETRY: Duration = Duration::from_secs(1);

/// The status bar's word on the daemon, from the connection banner alone: empty once the daemon
/// answered, `CONNECTING` until then, an `ERROR_PREFIX` error or a disconnect reason otherwise.
/// What the bar leaves unsaid: all is well.
const CONNECTED_LABEL: &str = "sarkosd connected";

fn daemon_label(status: &str) -> &'static str {
    if status.is_empty() {
        CONNECTED_LABEL
    } else if status == CONNECTING {
        "sarkosd connecting"
    } else {
        "sarkosd: see banner"
    }
}

/// `daemon_label`, except that while the window is reconnecting the bar says so at once.
fn daemon_label_with(status: &str, reconnecting: bool) -> &'static str {
    if reconnecting {
        "sarkosd reconnecting"
    } else {
        daemon_label(status)
    }
}

/// The busy mark a terminal tab shows after a disconnect.
fn disconnect_mark(is_final: bool) -> &'static str {
    if is_final {
        "daemon refused"
    } else {
        "reconnecting"
    }
}

/// A reconnect's status waits `BANNER_AFTER` before it becomes a banner; the status bar says
/// "reconnecting" at once. Any other status shows at once.
fn banner_visible(reconnecting_since: Option<Instant>, now: Instant) -> bool {
    match reconnecting_since {
        Some(since) => now.duration_since(since) >= connection::BANNER_AFTER,
        None => true,
    }
}

/// How a click in the tree opens a file in `pane` (spec 6, VS Code's preview tabs): the tab
/// already showing it is activated; else a preview tab in this pane is replaced, unless the
/// click pins; else a new tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenPlan {
    Activate(usize),
    Replace(usize),
    Add,
}

pub fn open_plan(tree: &Tree, pane: PaneId, path: &Path, preview_tab: Option<TabId>, pin: bool) -> OpenPlan {
    if let Some(idx) = tree.find_viewer(pane, path) {
        return OpenPlan::Activate(idx);
    }
    if !pin {
        if let Some(p) = preview_tab {
            if let Some((in_pane, idx)) = tree.locate(p) {
                if in_pane == pane {
                    return OpenPlan::Replace(idx);
                }
            }
        }
    }
    OpenPlan::Add
}

/// The preview tab after a tab closed: none if the preview was the one closed.
pub fn after_close(preview_tab: Option<TabId>, closed: TabId) -> Option<TabId> {
    preview_tab.filter(|p| *p != closed)
}

/// The preview tab after a tab was dragged: a dragged tab is a deliberate placement, so a
/// dragged preview stops being one.
pub fn after_drop(preview_tab: Option<TabId>, dragged: TabId) -> Option<TabId> {
    preview_tab.filter(|p| *p != dragged)
}

/// Whether a background result numbered `seq` lands over the one numbered `applied`: only a
/// newer one does, so a slow, older result never replaces a newer one.
pub fn should_apply(seq: u64, applied: u64) -> bool {
    seq > applied
}

/// Whether a command leaves the keys in the file tree (Shift+Cmd+E). Every other command acts
/// on the focused pane, so the keys go back to it.
pub fn keeps_tree_focus(cmd: &keymap::Command) -> bool {
    use keymap::Command::*;
    matches!(cmd, ToggleSidebar | FocusFileTree | ShowIgnoredFiles | CommandPalette)
}

/// Where a palette-driven update is: the daemon replacing itself (the window relaunches once it
/// reconnects), or the window asking to be relaunched and quitting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStage {
    DaemonUpgrading,
    Relaunching,
}

pub struct Root {
    pub tree: Tree,
    pub sessions: SessionManager,
    pub save_queue: SharedSaveQueue,
    pub project: PathBuf,
    /// Connection-level banner (errors, disconnects). Empty when all is well.
    pub status: String,
    /// What the daemon said it runs, from a version 2 `Hello` answer. `None` for an old daemon.
    pub daemon_version: Option<String>,
    /// Bumped when a connection starts and when it ends, so tasks bound to an older one (its
    /// heartbeat, a restore parsed from its replay) know to stop.
    pub connection_generation: u64,
    /// When the connection ended, while the window is reconnecting; `None` while connected.
    pub reconnecting_since: Option<Instant>,
    /// Whether a session the daemon no longer runs comes back with the "(restarted)" note: a
    /// saved layout at launch, and always after a reconnect.
    pub reconcile_with_note: bool,
    /// The grid each pane last measured while it had no terminal to measure itself.
    pub grids: HashMap<PaneId, (usize, usize)>,
    pub grid_waiters: HashMap<PaneId, Vec<oneshot::Sender<(usize, usize)>>>,
    /// Focus handles for panes that show no terminal (busy or empty).
    pub pane_focus: HashMap<PaneId, FocusHandle>,
    /// A split handle being dragged: the split, its orientation, and the split's bounds.
    pub resizing: Option<(SplitId, Orientation, Rect)>,
    /// The left column (Cmd+B); its width and collapsed state are saved with the workspace.
    pub sidebar: sidebar::Sidebar,
    /// The sidebar's right edge is being dragged.
    pub sidebar_resizing: bool,
    /// Keyboard focus is in the file tree (Shift+Cmd+E) rather than the focused pane. Focusing
    /// a pane or a tab hands it back.
    pub sidebar_focused: bool,
    /// The file tree's model: the expanded directories, what is loaded, the selection.
    pub explorer: explorer::Explorer,
    /// The tab a single click previews into; the next single click replaces it. `None` once
    /// pinned or closed.
    pub preview_tab: Option<TabId>,
    /// The file tree's right-click menu while it is open.
    pub context_menu: Option<sidebar::ContextMenu>,
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
    /// Closed tabs Cmd+Shift+T brings back: terminals whose detached sessions wait out the
    /// grace, and viewers.
    pub closed: ClosedStack,
    /// Where a dragged tab would land if released now. Cleared on drop, on a release anywhere
    /// else, and on the first move after a release outside the window.
    pub drop_target: Option<drag::DropTarget>,
    /// When each empty pane's animation started; settled panes keep an old instant.
    pub empty_anim: HashMap<PaneId, std::time::Instant>,
    /// The basename of `$SHELL`, shown on a tab whose program set no title.
    pub shell_name: String,
    /// The last good config; `apply_config` replaces it.
    pub config: config::Config,
    pub theme: Arc<theme::Theme>,
    /// Syntax colours for viewer tabs in the theme's `viewer` colours. `None` until the grammars
    /// finish loading on the background executor after launch; a theme change rethemes it.
    pub highlighter: Option<Arc<highlight::Highlighter>>,
    /// Each viewer tab's file, loaded when the tab is first shown; dropped with the tab.
    pub viewers: HashMap<TabId, Entity<Viewer>>,
    /// What macOS reports; `appearance.mode = "system"` follows it.
    pub system_is_dark: bool,
    /// The config file's problem shown under the connection banner; `None` when all is well.
    /// Kept apart from `status`, which belongs to the connection and clears when it answers.
    pub config_error: Option<String>,
    /// The theme that did not load, shown under `config_error`. Set and cleared by whatever last
    /// tried to switch theme, so a config reload and a theme problem never clobber each other.
    pub theme_error: Option<String>,
    /// A one-off result (themes extracted, the editor failed to open), shown under `theme_error`
    /// until the next command or for `NOTICE_FOR`. Never in `status`, which is the connection's.
    pub notice: Option<String>,
    /// Bumped by every notice, so an older notice's timer leaves a newer notice up.
    pub notice_seq: u64,
    /// The command palette while it is open (Cmd+Shift+P).
    pub palette: Option<command_palette::Palette>,
    /// Each pane's tab strip scroll state, so the active tab can be kept in view.
    pub bar_scroll: HashMap<PaneId, gpui::ScrollHandle>,
    /// The pane whose tab overflow menu is open.
    pub tab_menu: Option<PaneId>,
    /// "update ready: <version>" while the binary at the launch path differs from this one.
    pub update_ready: Option<String>,
    /// The update "Update now" started, while it runs.
    pub updating: Option<UpdateStage>,
    /// Whether this is the key window, read each frame; the pulse ticks only while it is.
    pub window_active: bool,
    /// Where the working glyph's pulse starts; every working tab shares its phase.
    pub pulse_epoch: Instant,
    /// The user is closing the window: the Quit action, or the red button
    /// (`windowShouldClose:`). Never set by macOS's own terminate or the update relaunch.
    pub user_closing: bool,
    /// The Goodbye went out: the Quit action sends it, and the close its quit makes must not
    /// send a second one.
    said_goodbye: bool,
}

/// Whether a window closing tells the daemon the user is leaving, so a restart does not reopen
/// it: only the last window, and only when the user closed it.
pub fn says_goodbye(user_closing: bool, windows_left: usize) -> bool {
    user_closing && windows_left == 0
}

/// The session of the focused pane's active terminal tab.
fn focused_session_of(tree: &Tree) -> Option<SessionId> {
    let p = tree.pane(tree.focused)?;
    match p.tabs.get(p.active)? {
        Tab::Terminal { session } => Some(*session),
        Tab::Viewer { .. } => None,
    }
}

/// The name a tab shows before its program sets a title: the basename of the shell new tabs
/// run, which is `[terminal] shell` when set, else `$SHELL` (as the daemon picks it), else "shell".
fn shell_name(configured: Option<&str>, env: Option<&str>) -> String {
    configured
        .or(env)
        .and_then(|s| std::path::Path::new(s).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "shell".into())
}

impl Root {
    fn new(client: Arc<client::Client>, project: PathBuf, saved: Option<Workspace>, config: config::Config, theme: theme::Theme, system_is_dark: bool, cx: &mut Context<Self>) -> Self {
        let (text_preset, preset) = config::resolve_preset(saved.as_ref().and_then(|w| w.text_preset.as_deref()), &config.font.preset, &config.font.presets);
        let reconcile_with_note = saved.is_some();
        // The folder's own sidebar when it saved one, else the config's defaults.
        let (sidebar, explorer) = match saved.as_ref().and_then(|w| w.sidebar.clone()) {
            Some(s) => (sidebar::Sidebar::new(s.width, s.collapsed, cx), explorer::Explorer::new(project.clone(), s.expanded, s.show_ignored)),
            None => (sidebar::Sidebar::new(config.sidebar.width, config.sidebar.collapsed, cx), explorer::Explorer::new(project.clone(), Vec::new(), false)),
        };
        // No saved layout: the empty screen, not a shell nobody asked for.
        let tree = saved.map(|ws| ws.tree).unwrap_or_else(Tree::empty);
        let mut sessions = SessionManager::new(client.clone(), Some(client), project.clone());
        let t = &config.terminal;
        sessions.set_defaults(t.scrollback, t.shell.clone(), t.nice);
        Self {
            tree,
            sessions,
            save_queue: SharedSaveQueue::default(),
            project,
            status: CONNECTING.into(),
            daemon_version: None,
            connection_generation: 0,
            reconnecting_since: None,
            reconcile_with_note,
            grids: HashMap::new(),
            grid_waiters: HashMap::new(),
            pane_focus: HashMap::new(),
            resizing: None,
            sidebar,
            sidebar_resizing: false,
            sidebar_focused: false,
            explorer,
            preview_tab: None,
            context_menu: None,
            font_size: preset.size,
            line_height: preset.line_height,
            text_preset,
            content: Rect { x: 0.0, y: 0.0, w: 1200.0, h: 800.0 },
            pane_order: Vec::new(),
            closed: ClosedStack::default(),
            drop_target: None,
            empty_anim: HashMap::new(),
            shell_name: shell_name(config.terminal.shell.as_deref(), std::env::var("SHELL").ok().as_deref()),
            config,
            theme: Arc::new(theme),
            highlighter: None,
            viewers: HashMap::new(),
            system_is_dark,
            config_error: None,
            theme_error: None,
            notice: None,
            notice_seq: 0,
            palette: None,
            bar_scroll: HashMap::new(),
            tab_menu: None,
            update_ready: None,
            updating: None,
            window_active: true,
            pulse_epoch: Instant::now(),
            user_closing: false,
            said_goodbye: false,
        }
    }

    /// The theme the config and the current appearance name, loaded from the themes folder.
    pub fn resolve_theme(&self) -> Result<theme::Theme, String> {
        let name = theme::pick_name(self.config.appearance.mode, self.system_is_dark, &self.config.theme);
        theme::load(name, &paths::themes_dir())
    }

    /// Loads the grammars off the UI thread, once, at launch. The result takes the theme's
    /// colours at the moment it lands, so a theme change during the load is not lost.
    fn build_highlighter(&mut self, cx: &mut Context<Self>) {
        let (colors, text) = (self.theme.viewer.clone(), self.theme.chrome.text);
        cx.spawn(async move |this, cx| {
            let built = cx.background_executor().spawn(async move { highlight::Highlighter::new(&colors, text) }).await;
            let _ = this.update(cx, |root, cx| {
                root.highlighter = Some(Arc::new(built.with_theme(&root.theme.viewer, root.theme.chrome.text)));
                // Viewers opened before the grammars were ready are plain until now.
                root.reload_all_viewers(cx);
            });
        })
        .detach();
    }

    pub fn set_theme(&mut self, theme: theme::Theme, cx: &mut Context<Self>) {
        let palette = Arc::new(theme.palette());
        if let Some(h) = &self.highlighter {
            self.highlighter = Some(Arc::new(h.with_theme(&theme.viewer, theme.chrome.text)));
        }
        self.theme = Arc::new(theme);
        // Every open viewer re-highlights in the new colours.
        self.reload_all_viewers(cx);
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
        self.switch_theme(cx);
    }

    /// Loads the theme the config and appearance name and switches to it if it differs. A theme
    /// that fails to load keeps the current one; `theme_error` says so until a load succeeds.
    fn switch_theme(&mut self, cx: &mut Context<Self>) {
        self.switch_theme_to(self.resolve_theme(), cx);
    }

    /// Switches to `wanted` if it loaded and differs; a load error keeps the current theme and
    /// sets `theme_error`, a success clears it.
    fn switch_theme_to(&mut self, wanted: Result<theme::Theme, String>, cx: &mut Context<Self>) {
        let (switch, banner) = theme::keep_or_switch(&self.theme, wanted);
        if let Some(t) = switch {
            self.set_theme(t, cx);
        }
        if let Some(e) = &banner {
            eprintln!("sarkos: {e}");
        }
        self.theme_error = banner;
        cx.notify();
    }

    /// The file changed (or was deleted and recreated): parse it, keep the last good config on
    /// an error, apply what changed. Called by the watcher and by the palette's "Reload config".
    pub fn reload_config(&mut self, cx: &mut Context<Self>) {
        let (cfg, banner) = config::reload_decision(&self.config, config::load(&paths::config_file()));
        self.config_error = banner;
        if let Some(e) = &self.config_error {
            eprintln!("sarkos: {e}");
        } else {
            eprintln!("sarkos: config reloaded");
            self.apply_config(cfg, cx);
        }
        cx.notify();
    }

    /// Applies every live setting. The `[daemon]` section only matters when the window starts
    /// the daemon; the file says so.
    pub fn apply_config(&mut self, cfg: config::Config, cx: &mut Context<Self>) {
        let old = std::mem::replace(&mut self.config, cfg);
        if old.keybindings != self.config.keybindings {
            cx.clear_key_bindings();
            cx.bind_keys(keymap::bindings_for(&self.config.keybindings));
            // The menu bar shows (and on macOS answers) the keys bound when it was set.
            cx.set_menus(menus());
        }
        self.switch_theme(cx);
        if let Some(name) = config::preset_after_reload(&self.text_preset, &old.font, &self.config.font) {
            self.apply_text_preset(&name, cx);
        }
        if old.font.family != self.config.font.family {
            // The cell size changes with the family (`family()` reads the config; the
            // `cx.notify()` below re-renders): panes re-measure before a restore trusts a grid.
            self.grids.clear();
        }
        let t = &self.config.terminal;
        self.sessions.set_defaults(t.scrollback, t.shell.clone(), t.nice);
        self.shell_name = shell_name(t.shell.as_deref(), std::env::var("SHELL").ok().as_deref());
        cx.background_executor().spawn(async { motion::refresh() }).detach();
        self.apply_recovery(false, cx);
        cx.notify();
    }

    pub fn family(&self) -> gpui::SharedString {
        self.config.font.family.clone().into()
    }

    /// Switches every terminal and viewer to a preset (spec 7). Per-tab nudges from Cmd+= are reset.
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
        for v in self.viewers.values() {
            v.update(cx, |v, cx| {
                v.font_size = p.size;
                v.line_height = p.line_height;
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

    /// Reads every expanded directory that is not loaded yet, off the UI thread, and applies
    /// each result as it lands.
    pub fn load_pending_dirs(&mut self, cx: &mut Context<Self>) {
        for dir in self.explorer.dirs_to_load() {
            self.explorer.mark_loading(&dir);
            let root = self.project.clone();
            let d = dir.clone();
            cx.spawn(async move |this, cx| {
                let entries = cx.background_executor().spawn(async move { explorer::read_dir_entries(&root, &d) }).await;
                let _ = this.update(cx, |root, cx| {
                    match entries {
                        Some(entries) => root.explorer.set_loaded(&dir, entries),
                        None => root.explorer.set_unreadable(&dir),
                    }
                    // A change the poll saw while this read was in flight is read again now.
                    root.load_pending_dirs(cx);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// Opens `path` in the focused pane as a preview (single click) or pinned (double click,
    /// Enter). The tab's viewer is made and loaded when the pane first shows it.
    pub fn open_viewer(&mut self, path: &Path, pin: bool, cx: &mut Context<Self>) {
        self.sidebar_focused = false;
        let pane = self.tree.focused;
        let plan = open_plan(&self.tree, pane, path, self.preview_tab, pin);
        let idx = match plan {
            OpenPlan::Activate(idx) => {
                if pin && self.tree.tab_id(pane, idx) == self.preview_tab {
                    self.preview_tab = None;
                }
                idx
            }
            OpenPlan::Replace(idx) => {
                // Same tab id, another file: the old viewer goes, the render makes the new one.
                if let Some(id) = self.tree.tab_id(pane, idx) {
                    self.viewers.remove(&id);
                }
                self.tree.replace_tab(pane, idx, Tab::Viewer { path: path.to_path_buf() });
                idx
            }
            OpenPlan::Add => {
                let idx = self.tree.add_tab(pane, Tab::Viewer { path: path.to_path_buf() });
                self.preview_tab = if pin { None } else { self.tree.tab_id(pane, idx) };
                idx
            }
        };
        self.tree.activate(pane, idx);
        self.explorer.selected = Some(path.to_path_buf());
        self.save(cx);
        cx.notify();
    }

    /// Escape while the tree's right-click menu is open closes the menu and nothing else.
    fn escape_closes_menu(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        if ks.key != "escape" || self.context_menu.is_none() {
            return false;
        }
        self.context_menu = None;
        cx.notify();
        true
    }

    /// Keys in the file tree (ruling 7): Up/Down move, Right expands or enters, Left collapses or
    /// goes to the parent, Enter opens pinned, Space previews, Escape returns to the pane.
    pub fn tree_key(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) {
        if ks.modifiers.platform || ks.modifiers.control || ks.modifiers.alt {
            return;
        }
        let rows = self.explorer.rows();
        let selected = self.explorer.selected.clone().and_then(|s| rows.iter().find(|r| r.path == s).cloned());
        match ks.key.as_str() {
            "down" => self.explorer.select_next(),
            "up" => self.explorer.select_prev(),
            "right" => match selected {
                Some(r) if r.is_dir && !r.expanded => {
                    self.explorer.toggle(&r.path);
                    self.load_pending_dirs(cx);
                }
                Some(r) if r.is_dir => self.explorer.select_next(),
                _ => {}
            },
            "left" => match selected {
                Some(r) if r.is_dir && r.expanded => {
                    self.explorer.toggle(&r.path);
                }
                Some(r) => {
                    if let Some(parent) = r.path.parent() {
                        if parent != self.project {
                            self.explorer.selected = Some(parent.to_path_buf());
                        }
                    }
                }
                None => {}
            },
            "enter" | "space" => match selected {
                Some(r) if r.is_dir => {
                    self.explorer.toggle(&r.path);
                    self.load_pending_dirs(cx);
                }
                Some(r) => {
                    let pin = ks.key == "enter";
                    self.open_viewer(&r.path, pin, cx);
                    if !pin {
                        self.sidebar_focused = true;
                    }
                    return;
                }
                None => self.explorer.select_next(),
            },
            "escape" => self.sidebar_focused = false,
            _ => return,
        }
        // Keep the selection on screen when the keys move it past the column's edge.
        let rows = self.explorer.rows();
        if let Some(i) = self.explorer.selected.as_ref().and_then(|s| rows.iter().position(|r| &r.path == s)) {
            self.sidebar.scroll.scroll_to_item(i);
        }
        self.save(cx);
        cx.notify();
    }

    /// What the poll fingerprints: the expanded directories (and the root), and the open
    /// viewers' files.
    pub fn watched_paths(&self, cx: &App) -> Vec<PathBuf> {
        let mut v = vec![self.project.clone()];
        v.extend(self.explorer.expanded_dirs());
        v.extend(self.viewers.values().map(|viewer| viewer.read(cx).path.clone()));
        v
    }

    /// A changed directory is read again, a changed viewer file reloaded.
    pub fn paths_changed(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        for p in paths {
            if !self.reload_viewers_for(p, cx) {
                self.explorer.invalidate(p);
            }
        }
        self.load_pending_dirs(cx);
    }

    /// The viewer of a viewer tab, made (and its load started) the first time the tab is shown.
    pub fn viewer_for(&mut self, tab: TabId, path: &Path, cx: &mut Context<Self>) -> Entity<Viewer> {
        if let Some(v) = self.viewers.get(&tab) {
            return v.clone();
        }
        let (font_size, line_height) = (self.font_size, self.line_height);
        let v = cx.new(|cx| Viewer::new(path.to_path_buf(), font_size, line_height, cx));
        self.viewers.insert(tab, v.clone());
        self.load_viewer(tab, cx);
        v
    }

    /// Reads, classifies and highlights a viewer's file on the background executor and applies
    /// the result. A deleted or unreadable file shows why.
    pub fn load_viewer(&mut self, tab: TabId, cx: &mut Context<Self>) {
        self.load(tab, false, cx);
    }

    /// Reloads every viewer showing `path` whose file changed since it was loaded; returns
    /// whether any viewer shows it.
    pub fn reload_viewers_for(&mut self, path: &Path, cx: &mut Context<Self>) -> bool {
        let tabs: Vec<TabId> = self.viewers.iter().filter(|(_, v)| v.read(cx).path == path).map(|(t, _)| *t).collect();
        for &tab in &tabs {
            self.load(tab, true, cx);
        }
        !tabs.is_empty()
    }

    /// Reloads the viewers whose colours depend on the highlighter: plain text, binaries and
    /// files too large to show have nothing to recolour.
    fn reload_all_viewers(&mut self, cx: &mut Context<Self>) {
        let tabs: Vec<TabId> = self.viewers.iter().filter(|(_, v)| {
            let v = v.read(cx);
            v.plain.is_none() && matches!(v.content, viewer::Content::Text { .. })
        }).map(|(t, _)| *t).collect();
        for tab in tabs {
            self.load_viewer(tab, cx);
        }
    }

    /// One background load. With `if_changed`, a file whose fingerprint matches the loaded one
    /// is left alone (the poll's first look at a viewer's file can report a change it already shows).
    fn load(&mut self, tab: TabId, if_changed: bool, cx: &mut Context<Self>) {
        let Some(viewer) = self.viewers.get(&tab).cloned() else { return };
        let (path, known, seq) = viewer.update(cx, |v, _| {
            v.loads += 1;
            let known = if if_changed && v.loaded { v.fingerprint } else { None };
            (v.path.clone(), known, v.loads)
        });
        let highlighter = self.highlighter.clone();
        let limit = self.config.viewer.highlight_limit_bytes;
        let weak = viewer.downgrade();
        cx.spawn(async move |_, cx| {
            let loaded = cx.background_executor().spawn(async move { viewer::read_file(&path, known, highlighter.as_deref(), limit) }).await;
            let Some(loaded) = loaded else { return };
            let _ = weak.update(cx, |v, cx| {
                if should_apply(seq, v.applied) {
                    v.applied = seq;
                    v.apply(loaded, cx);
                }
            });
        })
        .detach();
    }

    pub fn save(&mut self, cx: &mut App) {
        let sidebar = SidebarSaved { width: self.sidebar.width, collapsed: self.sidebar.collapsed, show_ignored: self.explorer.show_ignored, expanded: self.explorer.expanded_dirs() };
        let ws = Workspace { version: FORMAT_VERSION, project: self.project.clone(), tree: self.tree.clone(), text_preset: Some(self.text_preset.clone()), sidebar: Some(sidebar) };
        request_save(&self.save_queue, ws, cx);
    }

    /// Drops everything kept per pane. A restore waiting on the pane's grid then looks for its
    /// tab again: in another pane it waits there, and closed it gives up.
    pub fn forget_pane(&mut self, pane: PaneId) {
        self.grids.remove(&pane);
        self.grid_waiters.remove(&pane);
        self.pane_focus.remove(&pane);
        self.empty_anim.remove(&pane);
        self.bar_scroll.remove(&pane);
        if self.tab_menu == Some(pane) {
            self.tab_menu = None;
        }
    }

    /// Forgets per-pane state for panes no longer in the tree, whatever removed them.
    fn forget_closed_panes(&mut self) {
        let gone: Vec<PaneId> = self
            .grids
            .keys()
            .chain(self.grid_waiters.keys())
            .chain(self.pane_focus.keys())
            .chain(self.empty_anim.keys())
            .chain(self.bar_scroll.keys())
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

    /// The grid a terminal in `pane` would get: the pane's own last measurement, else the grid of
    /// its current terminal.
    fn pane_grid(&self, pane: PaneId, cx: &App) -> Option<(usize, usize)> {
        self.tree.pane(pane)?;
        let visible = self.layout().panes.iter().any(|(p, _)| *p == pane);
        let own = self.own_grid(pane, cx);
        let maximized = self.tree.maximized.filter(|m| *m != pane).and_then(|m| self.own_grid(m, cx));
        pick_grid(visible, own, maximized, self.grids.values().next().copied())
    }

    fn own_grid(&self, pane: PaneId, cx: &App) -> Option<(usize, usize)> {
        let p = self.tree.pane(pane)?;
        let terminal = match p.tabs.get(p.active) {
            Some(Tab::Terminal { session }) => self.sessions.terminals.get(session).map(|t| {
                let core = &t.read(cx).core;
                (core.columns(), core.screen_lines())
            }),
            _ => None,
        };
        grid_choice(self.grids.get(&pane).copied(), terminal)
    }

    /// The focused pane's active viewer, once its tab has been shown.
    pub fn active_viewer(&self) -> Option<Entity<Viewer>> {
        let p = self.tree.pane(self.tree.focused)?;
        match p.tabs.get(p.active)? {
            Tab::Viewer { .. } => self.viewers.get(p.ids.get(p.active)?).cloned(),
            Tab::Terminal { .. } => None,
        }
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
        self.sidebar_focused = false;
        if self.tree.pane(pane).is_none() {
            return;
        }
        let maximized = self.tree.maximized;
        self.tree.focus(pane);
        self.acknowledge_focused(cx);
        if self.tree.maximized != maximized {
            self.save(cx);
        }
        cx.notify();
    }

    /// The session of the focused pane's active terminal tab.
    pub fn focused_session(&self) -> Option<SessionId> {
        focused_session_of(&self.tree)
    }

    /// Clears the waiting dot of the focused tab (the user is looking at it).
    pub fn acknowledge_focused(&mut self, cx: &mut Context<Self>) {
        if let Some(s) = self.focused_session() {
            if self.sessions.acknowledge(s) {
                cx.notify();
            }
        }
    }

    pub fn activate_tab(&mut self, pane: PaneId, idx: usize, cx: &mut Context<Self>) {
        self.sidebar_focused = false;
        self.tree.activate(pane, idx);
        self.acknowledge_focused(cx);
        if let Some(h) = self.bar_scroll.get(&pane) {
            h.scroll_to_item(idx);
        }
        self.save(cx);
        cx.notify();
    }

    pub fn toggle_tab_menu(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        if self.tab_menu == Some(pane) {
            self.tab_menu = None;
        } else {
            self.tab_menu = Some(pane);
            self.focus_pane(pane, cx);
        }
        cx.notify();
    }

    pub fn close_tab_menu(&mut self, cx: &mut Context<Self>) {
        if self.tab_menu.take().is_some() {
            cx.notify();
        }
    }

    /// Closes a tab; the pane stays (spec 4). A terminal's session is detached at once and killed
    /// only after `CLOSE_GRACE`, so Cmd+Shift+T can bring the same shell back. A session whose
    /// shell has ended is killed at once and never enters the closed stack.
    pub fn close_tab(&mut self, pane: PaneId, idx: usize, cx: &mut Context<Self>) {
        let Some(id) = self.tree.tab_id(pane, idx) else { return };
        let Some(tab) = self.tree.close_tab(pane, idx) else { return };
        self.preview_tab = after_close(self.preview_tab, id);
        self.viewers.remove(&id);
        if let Tab::Terminal { session } = tab {
            if session != SessionId(0) {
                let ended = self.sessions.terminals.get(&session).map(|t| !t.read(cx).alive).unwrap_or(false);
                if ended {
                    // Nothing to bring back: end the session now instead of a minute from now.
                    eprintln!("sarkos: closing ended session {}", session.0);
                    self.sessions.kill(session);
                    self.sessions.detach(session);
                    self.sessions.tab_closed(id);
                    self.close_emptied_pane(pane);
                    self.save(cx);
                    cx.notify();
                    return;
                }
                if self.sessions.is_restoring(session) {
                    self.sessions.abort_restore(session, id);
                }
                self.sessions.detach(session);
                for old in self.closed.push(ClosedTab { closed: Closed::Session(session), pane, closed_at: Instant::now() }) {
                    eprintln!("sarkos: more than {MAX_CLOSED} closed tabs, ending session {}", old.0);
                    self.sessions.kill(old);
                }
            }
            // A shell still starting for this tab is killed when it arrives (`handle_outcome`).
            self.sessions.tab_closed(id);
        } else if let Tab::Viewer { path } = tab {
            for old in self.closed.push(ClosedTab { closed: Closed::Viewer(path), pane, closed_at: Instant::now() }) {
                eprintln!("sarkos: more than {MAX_CLOSED} closed tabs, ending session {}", old.0);
                self.sessions.kill(old);
            }
        }
        self.close_emptied_pane(pane);
        self.save(cx);
        cx.notify();
    }

    /// A pane whose last tab just closed goes with it, its space to its neighbour; the last pane
    /// stays and shows the empty screen.
    fn close_emptied_pane(&mut self, pane: PaneId) {
        if self.tree.close_pane_if_empty(pane) {
            self.forget_pane(pane);
        }
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
            let text = format!("cannot open {}: {e}", path.display());
            eprintln!("sarkos: {text}");
            self.show_notice(text, cx);
        }
    }

    /// Shows a one-off notice as its own banner and takes it down after `NOTICE_FOR`, unless a
    /// newer notice or a command has replaced or cleared it by then.
    pub fn show_notice(&mut self, text: String, cx: &mut Context<Self>) {
        self.notice_seq += 1;
        let seq = self.notice_seq;
        self.notice = Some(text);
        cx.notify();
        cx.spawn(async move |root, cx| {
            cx.background_executor().timer(NOTICE_FOR).await;
            let _ = root.update(cx, |root, cx| {
                if root.notice_seq == seq && root.notice.take().is_some() {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Cmd+Shift+P: opens the palette with every action, preset and theme, or closes it.
    pub fn toggle_palette(&mut self, cx: &mut Context<Self>) {
        if self.palette.is_some() {
            self.close_palette(cx);
            return;
        }
        let themes_dir = paths::themes_dir();
        let items = command_palette::items(&self.config, &theme::names(&themes_dir), &self.text_preset, &self.theme.name, &themes_dir, self.update_ready.as_deref(), self.config.recovery.start_at_login, launch_agent::installed(&launch_agent::agent_path()), &hook_cli::claude_settings_path(), self.explorer.show_ignored);
        self.palette = Some(command_palette::Palette::new(items, cx.focus_handle()));
        cx.notify();
    }

    pub fn close_palette(&mut self, cx: &mut Context<Self>) {
        self.palette = None;
        cx.notify();
    }

    /// A key typed while the palette has focus: edits the query, moves the selection, runs the
    /// selected command (Enter) or closes the palette (Escape). Cmd+V pastes into the query.
    pub fn palette_key(&mut self, ks: &gpui::Keystroke, window: &mut Window, cx: &mut Context<Self>) {
        if ks.modifiers.platform && ks.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                if let Some(p) = self.palette.as_mut() {
                    p.query.insert_str(&text);
                    p.refilter();
                    cx.notify();
                }
            }
            return;
        }
        let Some(p) = self.palette.as_mut() else { return };
        match p.query.handle(ks) {
            line_edit::Edit::Changed => p.refilter(),
            line_edit::Edit::Up => p.step(-1),
            line_edit::Edit::Down => p.step(1),
            line_edit::Edit::Submit | line_edit::Edit::SubmitBack => {
                self.run_palette_selection(window, cx);
                return;
            }
            line_edit::Edit::Cancel => {
                self.close_palette(cx);
                return;
            }
            line_edit::Edit::Unchanged => {}
        }
        cx.notify();
    }

    /// Closes the palette and runs the selected command, if any row is visible.
    pub fn run_palette_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cmd = self.palette.as_ref().and_then(|p| p.selected_command());
        self.close_palette(cx);
        if let Some(cmd) = cmd {
            self.run(cmd, window, cx);
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
        self.preview_tab = after_drop(self.preview_tab, d.tab);
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
        // The user has moved on: the last notice goes (a command may put up a new one).
        if self.notice.take().is_some() {
            cx.notify();
        }
        // A command on the focused pane (Cmd+F, Cmd+T, a split) takes the keys back from the tree.
        if self.sidebar_focused && !keeps_tree_focus(&cmd) {
            self.sidebar_focused = false;
            cx.notify();
        }
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
                // with nothing closed, a fresh shell opens there instead. A viewer comes back
                // pinned, its file read again.
                let Some(closed) = self.closed.pop() else {
                    self.new_terminal_tab(focused, cx);
                    return;
                };
                let pane = if self.tree.pane(closed.pane).is_some() { closed.pane } else { focused };
                let in_grace = closed.in_grace(Instant::now());
                match closed.closed {
                    Closed::Viewer(path) => {
                        self.tree.focus(pane);
                        self.open_viewer(&path, true, cx);
                    }
                    Closed::Session(session) if in_grace => self.reopen_session(pane, session, cx),
                    Closed::Session(session) => {
                        eprintln!("sarkos: session {} closed more than {}s ago, ending it and opening a new shell", session.0, CLOSE_GRACE.as_secs());
                        self.sessions.kill(session);
                        self.new_terminal_tab(pane, cx);
                    }
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
            ClearToStart => {
                if let Some(t) = self.active_terminal(cx) {
                    t.update(cx, |t, cx| {
                        t.core.clear_to_start();
                        // An open find bar's count and highlights follow on the next frame.
                        t.mark_find_stale();
                        cx.notify();
                    });
                }
            }
            FontDelta(d) => {
                // Only the focused terminal changes (spec 7); its element resizes the grid and
                // the PTY on the next paint. Nudges are not saved, and a preset change resets them.
                if let Some(t) = self.active_terminal(cx) {
                    t.update(cx, |t, cx| {
                        t.font_size = (t.font_size + d).clamp(10.0, 32.0);
                        cx.notify();
                    });
                    cx.notify();
                } else if let Some(v) = self.active_viewer() {
                    v.update(cx, |v, cx| {
                        v.font_size = (v.font_size + d).clamp(8.0, 48.0);
                        cx.notify();
                    });
                    cx.notify();
                }
            }
            OpenConfig => self.open_config(cx),
            TextPreset(name) => {
                self.apply_text_preset(&name, cx);
            }
            ReloadConfig => self.reload_config(cx),
            ExtractThemes => {
                let text = match theme::extract(&paths::themes_dir()) {
                    Ok(wrote) if wrote.is_empty() => format!("built-in themes are already in {}", paths::themes_dir().display()),
                    Ok(wrote) => format!("wrote {} theme files to {}", wrote.len(), paths::themes_dir().display()),
                    Err(e) => format!("cannot extract themes: {e}"),
                };
                self.show_notice(text, cx);
            }
            CommandPalette => self.toggle_palette(cx),
            Find => {
                if let Some(v) = self.active_viewer() {
                    v.update(cx, |v, cx| v.open_find(cx));
                } else if let Some(t) = self.active_terminal(cx) {
                    t.update(cx, |t, cx| t.open_find(cx));
                }
            }
            SetTheme(name) => self.switch_theme_to(theme::load(&name, &paths::themes_dir()), cx),
            UpdateNow => self.update_now(cx),
            StartAtLogin => self.toggle_start_at_login(cx),
            InstallHooks => {
                let exe = update::launch_path().unwrap_or_else(|| PathBuf::from("sarkos"));
                let root = cx.entity();
                cx.spawn(async move |_, cx| {
                    let result = cx.background_executor().spawn(async move { hook_cli::install_hooks(&exe) }).await;
                    let text = match result {
                        Ok((path, added, replaced)) => format!("installed {added} hook entries in {} (replaced {replaced}); restart claude for them to take effect", path.display()),
                        Err(e) => format!("cannot install hooks: {e}"),
                    };
                    let _ = root.update(cx, |root, cx| root.show_notice(text, cx));
                })
                .detach();
            }
            ToggleSidebar => {
                self.sidebar.collapsed = !self.sidebar.collapsed;
                // A hidden tree cannot hold the keys: they go back to the focused pane.
                if self.sidebar.collapsed {
                    self.sidebar_focused = false;
                }
                // The panes change width: measurements taken before are stale, as on a resize.
                self.grids.clear();
                self.save(cx);
                cx.notify();
            }
            FocusFileTree => {
                if self.sidebar.collapsed {
                    self.sidebar.collapsed = false;
                    self.grids.clear();
                    self.save(cx);
                }
                self.sidebar_focused = true;
                cx.notify();
            }
            ShowIgnoredFiles => {
                self.explorer.show_ignored = !self.explorer.show_ignored;
                self.save(cx);
                cx.notify();
            }
        }
        let _ = window;
    }

    /// The plist for this install: the sibling daemon, the config's daemon arguments, the log.
    fn agent_plist(&self) -> String {
        let args = vec![
            "--watchdog-secs".to_string(),
            self.config.daemon.watchdog_secs.to_string(),
            "--replay-bytes".to_string(),
            self.config.daemon.replay_bytes.to_string(),
        ];
        launch_agent::plist(&paths::daemon_exe(), &args, &client::daemon_log(&paths::socket_path()))
    }

    /// Installs or removes the login item off the UI thread and says what happened.
    fn set_start_at_login(&mut self, on: bool, cx: &mut Context<Self>) {
        let path = launch_agent::agent_path();
        let contents = self.agent_plist();
        let root = cx.entity();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { if on { launch_agent::install(&path, &contents) } else { launch_agent::remove(&path) } })
                .await;
            // A reload that changed nothing stays quiet: no notice replaces what is showing.
            let text = match (on, result) {
                (_, Ok(false)) => return,
                (true, Ok(true)) => "start at login is on: after a restart, sessions come back on their own".to_string(),
                (false, Ok(true)) => "start at login is off".to_string(),
                (_, Err(e)) => format!("start at login: {e}"),
            };
            let _ = root.update(cx, |root, cx| root.show_notice(text, cx));
        })
        .detach();
    }

    /// The palette's "Start at login": flips the login item while the config says "ask"; when
    /// the config pins it, opens the config instead.
    pub fn toggle_start_at_login(&mut self, cx: &mut Context<Self>) {
        match self.config.recovery.start_at_login {
            config::StartAtLogin::Ask => {
                let on = !launch_agent::installed(&launch_agent::agent_path());
                self.set_start_at_login(on, cx);
            }
            _ => self.open_config(cx),
        }
    }

    /// Keeps the login item as the config says; with "ask" and nothing installed, says once per
    /// launch how to turn it on.
    pub fn apply_recovery(&mut self, at_launch: bool, cx: &mut Context<Self>) {
        let installed = launch_agent::installed(&launch_agent::agent_path());
        match launch_agent::desired(self.config.recovery.start_at_login, installed) {
            Some(on) => self.set_start_at_login(on, cx),
            None if at_launch && !installed => {
                self.show_notice("sessions can come back after a restart: run \"Start at login\" from the command palette (Cmd+Shift+P)".into(), cx)
            }
            None => {}
        }
    }

    /// The palette's "Update now": the daemon first when its binary changed, then this window.
    pub fn update_now(&mut self, cx: &mut Context<Self>) {
        if self.updating.is_some() {
            self.show_notice("an update is already running".into(), cx);
            return;
        }
        if self.sessions.is_offline() {
            // A refused window never reconnects, so the daemon cannot be asked to update itself.
            let text = if connection::is_final(&self.status) {
                "sarkosd is older than this window: run pkill -x sarkosd (ends every session) and open Sarkos IDE again"
            } else {
                "sarkosd is not connected; try again once it is"
            };
            self.show_notice(text.into(), cx);
            return;
        }
        let Some(exe) = update::launch_path() else {
            self.show_notice("cannot find this window's own binary".into(), cx);
            return;
        };
        let daemon_exe = update::sibling_daemon(&exe);
        let root = cx.entity();
        cx.spawn(async move |_, cx| {
            let d = daemon_exe.clone();
            let installed = cx.background_executor().spawn(async move { update::installed_version(&d) }).await;
            let _ = root.update(cx, |root, cx| {
                // A second "Update now" got here first, or the connection went meanwhile.
                if root.updating.is_some() || root.sessions.is_offline() {
                    return;
                }
                match installed {
                    None => root.show_notice(format!("cannot run {} --version", daemon_exe.display()), cx),
                    Some(installed) if root.daemon_version.as_deref().map_or(true, |r| update::differs(r, &installed)) => {
                        root.updating = Some(UpdateStage::DaemonUpgrading);
                        root.show_notice("updating the daemon; sessions stay".into(), cx);
                        root.sessions.upgrade(daemon_exe.clone());
                    }
                    Some(_) => root.relaunch_now(cx),
                }
            });
        })
        .detach();
    }

    /// Asks the daemon to start the installed window in this project once we are gone. The window
    /// quits when the daemon accepts (`Outcome::RelaunchAccepted`) and stays when it refuses.
    pub fn relaunch_now(&mut self, cx: &mut Context<Self>) {
        // Quitting with nothing to relaunch would leave no window: a binary launched from a
        // Homebrew Cellar path is gone once `brew cleanup` removes that version.
        let exe = match update::launch_path() {
            Some(exe) if exe.is_file() => exe,
            other => {
                self.updating = None;
                let what = other.map_or_else(|| "this window's binary".to_string(), |p| p.display().to_string());
                self.show_notice(format!("cannot relaunch: {what} is gone; quit and open Sarkos IDE again"), cx);
                return;
            }
        };
        self.updating = Some(UpdateStage::Relaunching);
        self.sessions.relaunch(exe);
    }

    /// What quitting must finish first: closed tabs' sessions are killed (nothing could reopen
    /// them later) and the pending layout save is written. `goodbye` is whether a Goodbye was
    /// queued just before, which the same wait covers.
    pub fn flush_for_quit(&mut self, goodbye: bool) {
        let killed = self.end_closed_tabs();
        // The process may exit before the writer thread sends what is queued; wait for it, once
        // and briefly. The client is the current connection's, whichever reconnect that is.
        if (killed > 0 || goodbye) && !self.sessions.client.as_ref().is_some_and(|c| c.wait_sent(Duration::from_millis(500))) {
            eprintln!("sarkos: could not send {killed} kill requests{} before quitting", if goodbye { " and the goodbye" } else { "" });
        }
        workspace::flush_now(&self.save_queue);
    }

    /// The user is quitting: tell the daemon not to reopen this window after a restart, then
    /// the flush every quit does. Not for the relaunch an update makes (`RelaunchAccepted`) nor
    /// macOS's terminate (`says_goodbye`). Says it once.
    pub fn leaving(&mut self) {
        if self.said_goodbye {
            return;
        }
        self.said_goodbye = true;
        self.sessions.goodbye();
        self.flush_for_quit(true);
    }

    /// A connection is up (the first one, or a reconnect): its session list reconciles the tree.
    pub fn connected(&mut self, client: Arc<client::Client>, cx: &mut Context<Self>) {
        self.sessions.set_client(client);
        // Every connection after the first is a reconnect: a session the daemon lost meanwhile
        // comes back as a fresh shell with the "(restarted)" note, as a relaunch does.
        if self.connection_generation > 0 {
            self.reconcile_with_note = true;
        }
        self.connection_generation += 1;
        self.reconnecting_since = None;
        self.status = CONNECTING.into();
        self.grids.clear();
        self.sessions.list();
        // The daemon's exec ended the last connection: it runs the new binary, now this window.
        if self.updating == Some(UpdateStage::DaemonUpgrading) {
            self.relaunch_now(cx);
        }
        cx.notify();
    }

    /// The connection ended. Every terminal tab waits for the reconnect; the daemon kept the
    /// sessions, so the next attach replays them. A refused protocol is final: no reconnect
    /// follows, and the banner says why at once.
    pub fn disconnected(&mut self, reason: String, cx: &mut Context<Self>) {
        // A restore still parsing this connection's replay must not install a terminal bound to
        // the dead client: the reconnect attaches the session again.
        self.connection_generation += 1;
        self.sessions.disconnect();
        // The relaunch request went with the connection, unanswered: keep the window.
        if self.updating == Some(UpdateStage::Relaunching) {
            self.updating = None;
            self.show_notice("update interrupted: sarkosd disconnected before relaunching this window; try again".into(), cx);
        }
        // An error the daemon sent before hanging up (a protocol mismatch) says why; keep it.
        self.status = if self.status.starts_with(ERROR_PREFIX) { format!("{}; {reason}", self.status) } else { reason };
        let is_final = connection::is_final(&self.status);
        if !is_final {
            eprintln!("sarkos: {}; reconnecting", self.status);
        }
        let mark = disconnect_mark(is_final);
        for (pane, idx, tab) in self.tree.tabs() {
            if let Tab::Terminal { .. } = tab {
                if let Some(id) = self.tree.tab_id(pane, idx) {
                    self.sessions.busy.insert(id, mark);
                }
            }
        }
        self.reconnecting_since = if is_final { None } else { Some(Instant::now()) };
        cx.notify();
    }

    /// Applies what the daemon answered.
    pub fn handle_outcome(&mut self, outcome: Outcome, cx: &mut Context<Self>) {
        match outcome {
            Outcome::None => {}
            Outcome::DaemonHello { version, .. } => {
                self.daemon_version = Some(version);
            }
            Outcome::Sessions(list) => {
                if self.status == CONNECTING {
                    // The banner goes away, so every pane grows: forget measurements taken under
                    // it so restores wait for the next frame's grid.
                    self.status.clear();
                    self.grids.clear();
                }
                self.sessions.note_list(&list);
                let infos: Vec<_> = list.iter().map(|d| d.info.clone()).collect();
                for step in reconcile(&self.tree, &infos, &self.project) {
                    match step {
                        Reconcile::Attach { tab, session } => self.sessions.attach(session, tab, false),
                        Reconcile::Create { tab } => self.sessions.create(tab, self.reconcile_with_note),
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
                    self.sessions.notes.insert(session, sessions::Note::Restarted);
                }
                self.save(cx);
                self.sessions.attach(session, tab, note);
                cx.notify();
            }
            Outcome::Attached { session, tab, replay, alive, note: _ } => {
                eprintln!("sarkos: attached to session {} ({} replay bytes)", session.0, replay.len());
                self.sessions.begin_restore(session);
                let root = cx.entity();
                let generation = self.connection_generation;
                cx.spawn(async move |_, cx| {
                    if let Err(e) = restore(&root, session, tab, replay, alive, generation, cx).await {
                        let why = format!("restoring it failed: {e:#}");
                        let _ = root.update(cx, |root, cx| {
                            // After a reconnect the new connection's own attach owns the tab.
                            if root.connection_generation == generation {
                                root.restart_tab(tab, session, &why, cx);
                                cx.notify();
                            }
                        });
                    }
                })
                .detach();
            }
            Outcome::Killed { .. } => {}
            Outcome::AttachFailed { session, tab, error } => {
                self.restart_tab(tab, session, &format!("sarkosd could not attach it: {error}"), cx);
            }
            Outcome::UpgradeFailed(e) => {
                // The old daemon keeps running and serving; nothing else changes.
                eprintln!("sarkos: update failed: {e}");
                self.updating = None;
                self.show_notice(format!("update failed: {e}"), cx);
            }
            Outcome::RelaunchAccepted => {
                self.flush_for_quit(false);
                cx.quit();
            }
            Outcome::RelaunchRefused(e) => {
                eprintln!("sarkos: relaunch refused: {e}");
                self.updating = None;
                self.show_notice(format!("cannot relaunch: {e}"), cx);
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
/// A restore that outlives its connection (`generation`) installs nothing: the reconnect attaches
/// the session again.
async fn restore(root: &Entity<Root>, session: SessionId, tab: TabId, replay: Vec<u8>, alive: bool, generation: u64, cx: &mut AsyncApp) -> anyhow::Result<()> {
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
    let scrollback = root.read_with(cx, |root, _| root.sessions.scrollback)?;
    let started = std::time::Instant::now();
    let (mut core, title) = cx
        .background_spawn(async move {
            let mut core = TerminalCore::new(cols, rows, scrollback);
            // The previous UI already answered any queries in the replay; keep only the title.
            let title = core.apply_output(&replay).title;
            (core, title)
        })
        .await;
    eprintln!("sarkos: session {} replay parsed at {cols}x{rows} in {:?}", session.0, started.elapsed());
    root.update(cx, |root, cx| {
        if root.connection_generation != generation {
            eprintln!("sarkos: session {} replay was parsed for a connection that has ended; dropping it", session.0);
            return;
        }
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

/// A pane's own measurement first (it is taken at the workspace font size), else the grid of
/// the terminal it shows (which Cmd+= may have nudged).
fn grid_choice(measured: Option<(usize, usize)>, terminal: Option<(usize, usize)>) -> Option<(usize, usize)> {
    measured.or(terminal)
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
        self.window_active = window.is_window_active();
        let theme = self.theme.clone();
        let status = if banner_visible(self.reconnecting_since, Instant::now()) { self.status.clone() } else { String::new() };
        let banners: Vec<String> = [
            status,
            self.config_error.clone().unwrap_or_default(),
            self.theme_error.clone().unwrap_or_default(),
            self.notice.clone().unwrap_or_default(),
        ]
            .into_iter()
            .filter(|b| !b.is_empty())
            .collect();
        let banner_h = BANNER_H * banners.len() as f32;
        let top = chrome::TITLE_BAR_H;
        // The panes start right of the sidebar. A window shorter than its chrome would give a
        // negative height; lay out at zero instead.
        let side = self.sidebar.visible_width();
        let content = Rect {
            x: side,
            y: top + banner_h,
            w: (f32::from(size.width) - side).max(0.0),
            h: (f32::from(size.height) - top - banner_h).max(0.0),
        };
        if content != self.content {
            // Measurements taken at the old size would parse a replay at the wrong grid.
            self.grids.clear();
            self.content = content;
        }
        // Built first: it creates the focus handles of panes that show no terminal.
        let body = split_view::render_tree(self, window, cx);
        let side_col = sidebar::render(self, cx);
        // Focus follows the tree: the focused pane's terminal or viewer (its find bar while that
        // is open), or the pane's own handle. The palette holds it while it is open, the file
        // tree after Shift+Cmd+E.
        let wanted = if let Some(f) = self.palette.as_ref().and_then(|p| p.focus.clone()) {
            Some(f)
        } else if self.sidebar_focused {
            Some(self.sidebar.focus.clone())
        } else {
            match (self.active_terminal(cx), self.active_viewer()) {
                (Some(t), _) => {
                    let t = t.read(cx);
                    Some(t.find.as_ref().map_or_else(|| t.focus.clone(), |f| f.focus.clone()))
                }
                (None, Some(v)) => {
                    let v = v.read(cx);
                    Some(v.find.as_ref().map_or_else(|| v.focus.clone(), |f| f.focus.clone()))
                }
                (None, None) => self.pane_focus.get(&self.tree.focused).cloned(),
            }
        };
        if let Some(f) = wanted {
            if !f.is_focused(window) {
                window.focus(&f);
            }
        }
        let daemon = daemon_label_with(&self.status, self.reconnecting_since.is_some());
        let items = chrome::status_items(self.update_ready.as_deref(), (daemon != CONNECTED_LABEL).then_some(daemon), self.sessions.waiting_count(&self.tree));
        let title_bar = chrome::title_bar(&self.project, items, window.is_fullscreen(), &theme);
        let family = self.family();
        let root_div = div()
            .relative()
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
                if root.sidebar_resizing {
                    if ev.pressed_button != Some(MouseButton::Left) {
                        // Released outside the window, where no mouse-up reaches us: end the drag.
                        root.sidebar_resizing = false;
                        root.save(cx);
                    } else {
                        // The column starts at the window's left edge, so the pointer is its width.
                        root.sidebar.width = sidebar::clamp_width(f32::from(ev.position.x));
                    }
                    cx.notify();
                    return;
                }
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
            .on_mouse_down(MouseButton::Left, cx.listener(|root, _ev: &MouseDownEvent, _window, cx| root.close_tab_menu(cx)))
            // The tree's menu closes on any press outside it (its rows stop theirs), and on
            // Escape wherever the keys are.
            .on_any_mouse_down(cx.listener(|root, _ev: &MouseDownEvent, _window, cx| {
                if root.context_menu.take().is_some() {
                    cx.notify();
                }
            }))
            .capture_key_down(cx.listener(|root, ev: &KeyDownEvent, _window, cx| {
                if root.escape_closes_menu(&ev.keystroke, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|root, _ev: &MouseUpEvent, _window, cx| {
                    if root.resizing.take().is_some() {
                        root.save(cx);
                    }
                    if root.sidebar_resizing {
                        root.sidebar_resizing = false;
                        root.save(cx);
                    }
                    // A drop stops the event before it gets here; this is a release elsewhere.
                    if root.drop_target.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(title_bar)
            .children(banners.into_iter().map(|text| banner_bar(text, family.clone(), &theme)))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .children(side_col)
                    .child(div().flex_1().min_w_0().min_h_0().child(body)),
            )
            .children(command_palette::render(self, cx))
            .children(sidebar::render_context_menu(self, cx));
        keymap::attach(root_div, cx)
    }
}

/// The menu bar. Set again when the key bindings change so it shows the bound keys.
fn menus() -> Vec<Menu> {
    vec![Menu { name: "Sarkos IDE".into(), items: vec![MenuItem::action("Quit", Quit)] }]
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

/// `<name> <crate version> (<short git hash>)`, the same shape for both binaries.
pub fn version_string() -> String {
    format!("{} {} ({})", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"), env!("SARKOS_GIT_HASH"))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // The hook CLI: no window, no daemon start, no QoS. `claude` runs these on every event.
    if matches!(args.get(1).map(String::as_str), Some("notify") | Some("hooks")) {
        std::process::exit(hook_cli::run(&args[1..]));
    }
    if args.iter().any(|a| a == "--version") {
        println!("{}", version_string());
        return;
    }
    // Spec 2.5: the UI thread is user-interactive so macOS schedules it ahead of the heavy work
    // Claude Code's children do.
    let qos = unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0) };
    if qos != 0 {
        eprintln!("sarkos: could not set user-interactive QoS (error {qos})");
    }
    motion::refresh();
    app_nap::hold();
    let config_path = paths::config_file();
    let (config, config_error) = match config::load(&config_path) {
        Ok(c) => (c, None),
        Err(e) => {
            eprintln!("sarkos: {e}");
            (config::Config::default(), Some(e.to_string()))
        }
    };
    // Used only when this window starts the daemon; a running one keeps its own settings.
    let daemon_args = vec![
        "--watchdog-secs".to_string(),
        config.daemon.watchdog_secs.to_string(),
        "--replay-bytes".to_string(),
        config.daemon.replay_bytes.to_string(),
    ];
    let (client, events) = match client::Client::connect_or_start(&paths::socket_path(), &paths::daemon_exe(), &daemon_args) {
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

    Application::new().run(move |cx: &mut App| {
        // gpui installs no menu and no quit binding of its own; the Quit action is handled below,
        // once the window exists.
        cx.bind_keys(keymap::bindings_for(&config.keybindings));
        cx.set_menus(menus());
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let client_for_view = client.clone();
        let view_queue = save_queue.clone();
        let view_project = project.clone();
        let view_saved = saved.clone();
        let system_is_dark = theme::is_dark(cx.window_appearance());
        let name = theme::pick_name(config.appearance.mode, system_is_dark, &config.theme).to_string();
        let (theme, theme_error) = match theme::load(&name, &paths::themes_dir()) {
            Ok(t) => (t, None),
            Err(e) => {
                // A broken user navy-light.toml still gives a light window.
                let t = theme::builtin(&name).or_else(|| theme::builtin("navy")).expect("navy is built in");
                let banner = format!("theme: {e}; using built-in {}", t.name);
                eprintln!("sarkos: {banner}");
                (t, Some(banner))
            }
        };
        let (view_config, view_theme) = (config, theme);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    // The window draws its own bar in the title bar's space (chrome.rs); the
                    // traffic lights stay where macOS puts them.
                    titlebar: Some(gpui::TitlebarOptions { title: Some("Sarkos IDE".into()), appears_transparent: true, traffic_light_position: None }),
                    // One minimum pane with its border, tab bar and the window's bar.
                    window_min_size: Some(size(px(tree::MIN_PANE_W + 8.0), px(tree::MIN_PANE_H + pane_view::TAB_BAR_H + chrome::TITLE_BAR_H))),
                    ..Default::default()
                },
                move |window, cx| {
                    let view = cx.new(|cx| {
                        let mut root = Root::new(client_for_view, view_project, view_saved, view_config, view_theme, system_is_dark, cx);
                        root.save_queue = view_queue;
                        root.build_highlighter(cx);
                        root
                    });
                    // The red button asks first; a close during macOS's terminate does not.
                    let closing = view.clone();
                    window.on_window_should_close(cx, move |_, cx| {
                        closing.update(cx, |root, _| root.user_closing = true);
                        true
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
        root.update(cx, |root, cx| {
            root.config_error = config_error;
            root.theme_error = theme_error;
            root.apply_recovery(true, cx);
            root.load_pending_dirs(cx);
            cx.notify();
        });
        let quit_root = root.clone();
        cx.on_action(move |_: &Quit, cx| {
            // Goodbye here, while everything runs; the close this quit makes finds it said.
            quit_root.update(cx, |root, _| {
                root.user_closing = true;
                root.leaving();
            });
            cx.quit();
        });
        // gpui keeps running without windows; quit with the last one so the heartbeat stops and
        // no windowless process lingers. macOS closes the window itself on terminate (logout,
        // shutdown): that close flushes but says no Goodbye, so a restart reopens the window.
        let close_root = root.clone();
        cx.on_window_closed(move |cx| {
            let left = cx.windows().len();
            if left == 0 {
                close_root.update(cx, |root, _| if says_goodbye(root.user_closing, left) { root.leaving() } else { root.flush_for_quit(false) });
                cx.quit();
            }
        })
        .detach();
        config_watch::start(root.clone(), config_path.clone(), cx);
        explorer_watch::start(root.clone(), cx);
        // The same client the root was built with; reconnects replace it from here on.
        connection::start(root.clone(), paths::socket_path(), paths::daemon_exe(), daemon_args.clone(), (client.clone(), events), cx);
        update::start(root.clone(), cx);
        pulse::start(root.downgrade(), cx);

        // Closed tabs' sessions end once their grace runs out.
        let tick_root = root.clone();
        cx.spawn(async move |cx| loop {
            cx.background_executor().timer(CLOSE_TICK).await;
            if tick_root.update(cx, |root, _| root.expire_closed(Instant::now())).is_err() {
                break;
            }
        })
        .detach();
        // A kill refused while the daemon held the upgrade slot goes out again: on the reconnect
        // a finished upgrade brings, or from here when the upgrade failed and none follows.
        let retry_root = root.clone();
        cx.spawn(async move |cx| loop {
            cx.background_executor().timer(KILL_RETRY).await;
            if retry_root.update(cx, |root, _| root.sessions.resend_kills()).is_err() {
                break;
            }
        })
        .detach();
    });
}

#[cfg(test)]
mod tests {
    use super::{after_close, after_drop, daemon_label, keeps_tree_focus, open_plan, pick_grid, says_goodbye, shell_name, should_apply, OpenPlan, Side, Tree, CONNECTING, ERROR_PREFIX};
    use protocol::SessionId;
    use std::path::Path;

    #[test]
    fn closing_or_dragging_the_preview_tab_ends_the_preview_and_no_other_tab_does() {
        assert_eq!(after_close(Some(3), 3), None, "the preview closed");
        assert_eq!(after_close(Some(3), 4), Some(3), "another tab closed");
        assert_eq!(after_close(None, 4), None);
        assert_eq!(after_drop(Some(3), 3), None, "a dragged preview is placed on purpose: pinned");
        assert_eq!(after_drop(Some(3), 4), Some(3));
        assert_eq!(after_drop(None, 3), None);
    }

    #[test]
    fn only_a_result_newer_than_the_applied_one_lands() {
        assert!(should_apply(1, 0), "the first load");
        assert!(should_apply(5, 3), "newer than what is shown");
        assert!(!should_apply(3, 3), "already shown");
        assert!(!should_apply(2, 3), "an older load finishing late");
    }

    #[test]
    fn only_the_sidebar_commands_and_the_palette_keep_the_keys_in_the_tree() {
        use crate::keymap::Command;
        for cmd in [Command::ToggleSidebar, Command::FocusFileTree, Command::ShowIgnoredFiles, Command::CommandPalette] {
            assert!(keeps_tree_focus(&cmd), "{cmd:?}");
        }
        for cmd in [Command::Find, Command::NewTerminal, Command::FontDelta(1.0)] {
            assert!(!keeps_tree_focus(&cmd), "{cmd:?}: the keys go back to the focused pane");
        }
    }

    #[test]
    fn a_preview_is_replaced_and_a_pin_is_kept() {
        use crate::tree::Tab;
        let mut tree = Tree::new(Tab::Terminal { session: SessionId(1) });
        let pane = tree.panes()[0];
        // Nothing open yet: a new tab.
        assert_eq!(open_plan(&tree, pane, Path::new("/p/a.rs"), None, false), OpenPlan::Add);
        let a = tree.add_tab(pane, Tab::Viewer { path: "/p/a.rs".into() });
        let a_id = tree.tab_id(pane, a).unwrap();
        // The same file again, previewed: just activate it.
        assert_eq!(open_plan(&tree, pane, Path::new("/p/a.rs"), Some(a_id), false), OpenPlan::Activate(a));
        // Another file while a preview is open in this pane: replace the preview.
        assert_eq!(open_plan(&tree, pane, Path::new("/p/b.rs"), Some(a_id), false), OpenPlan::Replace(a));
        // Pinned (double click): a new tab, the preview stays.
        assert_eq!(open_plan(&tree, pane, Path::new("/p/b.rs"), Some(a_id), true), OpenPlan::Add);
        // No preview in this pane (it was pinned or lives elsewhere): a new tab.
        assert_eq!(open_plan(&tree, pane, Path::new("/p/b.rs"), None, false), OpenPlan::Add);
        let other = tree.split(pane, Side::Right, Tab::Terminal { session: SessionId(2) }).unwrap();
        assert_eq!(open_plan(&tree, other, Path::new("/p/b.rs"), Some(a_id), false), OpenPlan::Add, "the preview is in another pane");
        // An already open pinned tab for the file: activate, whatever the mode.
        assert_eq!(open_plan(&tree, pane, Path::new("/p/a.rs"), None, true), OpenPlan::Activate(a));
    }

    #[test]
    fn only_the_users_close_of_the_last_window_says_goodbye() {
        assert!(says_goodbye(true, 0), "Quit, or the red button on the last window");
        assert!(!says_goodbye(false, 0), "macOS's terminate at logout or shutdown, or the update relaunch");
        assert!(!says_goodbye(true, 1), "a window is still open");
        assert!(!says_goodbye(false, 1));
    }

    #[test]
    fn the_daemon_label_follows_the_connection_not_notices() {
        assert_eq!(daemon_label(""), "sarkosd connected");
        assert_eq!(daemon_label(CONNECTING), "sarkosd connecting");
        assert_eq!(daemon_label(&format!("{ERROR_PREFIX}no such session")), "sarkosd: see banner");
        assert_eq!(daemon_label("daemon connection ended"), "sarkosd: see banner");
    }

    #[test]
    fn shell_name_is_the_basename_of_shell() {
        assert_eq!(shell_name(None, Some("/bin/zsh")), "zsh");
        assert_eq!(shell_name(None, Some("/opt/homebrew/bin/fish")), "fish");
        assert_eq!(shell_name(None, Some("")), "shell");
        assert_eq!(shell_name(None, None), "shell");
    }

    #[test]
    fn the_configured_shell_names_tabs_before_shell() {
        assert_eq!(shell_name(Some("/bin/bash"), Some("/bin/zsh")), "bash");
        assert_eq!(shell_name(Some("/bin/bash"), None), "bash");
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

    #[test]
    fn a_pane_measurement_outranks_its_terminals_grid() {
        use super::grid_choice;
        // The canvas measures at the workspace font; the terminal may have been nudged.
        assert_eq!(grid_choice(Some((100, 30)), Some((80, 24))), Some((100, 30)));
        assert_eq!(grid_choice(None, Some((80, 24))), Some((80, 24)));
        assert_eq!(grid_choice(None, None), None);
    }

    #[test]
    fn a_disconnect_banner_waits_ten_seconds_but_the_label_says_so_at_once() {
        use super::{banner_visible, daemon_label_with};
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        assert!(!banner_visible(Some(t0), t0 + Duration::from_secs(3)));
        assert!(banner_visible(Some(t0), t0 + Duration::from_secs(10)));
        assert!(banner_visible(None, t0), "a status that is not a reconnect shows at once");
        assert_eq!(daemon_label_with("daemon closed the connection", true), "sarkosd reconnecting");
        assert_eq!(daemon_label_with("", false), "sarkosd connected");
    }

    #[test]
    fn the_focused_session_is_the_focused_panes_active_terminal_tab() {
        use super::{focused_session_of, Tab, Tree};
        use protocol::SessionId;
        let mut tree = Tree::new(Tab::Terminal { session: SessionId(5) });
        let root = tree.panes()[0];
        assert_eq!(focused_session_of(&tree), Some(SessionId(5)));
        tree.add_tab(root, Tab::Viewer { path: "/f".into() });
        tree.activate(root, 1);
        assert_eq!(focused_session_of(&tree), None);
    }

    #[test]
    fn a_refused_window_says_so_on_its_tabs() {
        use super::{connection::is_final, disconnect_mark};
        let refused = format!("{ERROR_PREFIX}protocol version mismatch: daemon speaks 1 to 2, client 3");
        assert_eq!(disconnect_mark(is_final(&refused)), "daemon refused");
        assert_eq!(disconnect_mark(is_final("daemon closed the connection")), "reconnecting");
    }
}
