mod app_nap;
mod ax;
mod chooser_sessions;
mod chrome;
mod client;
mod closed_stack;
mod command_palette;
mod config;
mod config_watch;
mod connection;
mod diff;
mod dock;
mod doctor;
mod drag;
mod empty_pane;
mod explorer;
mod explorer_watch;
mod find;
mod gitstatus;
mod highlight;
mod hook_cli;
mod idle;
mod keys;
mod keymap;
mod launch;
mod launch_link;
mod links;
mod launch_agent;
mod layout;
mod line_edit;
mod motion;
mod mouse;
mod palette;
mod pane_view;
mod paths;
mod pulse;
mod recent;
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
mod worktrees;

use futures::channel::oneshot;
use gpui::{
    div, prelude::*, px, size, App, Application, AsyncApp, Bounds, Context, Div, DragMoveEvent, Entity, FocusHandle,
    KeyDownEvent, Menu, MenuItem, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Window, WindowBounds, WindowOptions,
};
use protocol::{Event, Request, SessionId, StagedUpdate};
use sessions::{reconcile, Outcome, Reconcile, SessionManager};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use closed_stack::{Closed, ClosedStack, ClosedTab, CLOSE_GRACE, CLOSE_TICK, MAX_CLOSED};
use terminal::{Terminal, TerminalCore};
use sidebar::{after_close, after_drop, keeps_tree_focus};
use viewer::Viewer;
use tree::{Orientation, PaneId, Rect, Side, SplitId, Tab, TabId, Tree};
use workspace::{request_save, SharedSaveQueue, SidebarSaved, Workspace, FORMAT_VERSION};
use keymap::Quit;
use motion::Transient;

const BANNER_H: f32 = 28.0;

/// Starts every status that reports a `Response::Error`.
const ERROR_PREFIX: &str = "arkosd error: ";
/// The status until the daemon answers the session list.
const CONNECTING: &str = "connecting to arkosd";
/// How long a one-off notice stays up when no command replaces or clears it first.
const NOTICE_FOR: Duration = Duration::from_secs(6);
/// How often kills the daemon refused during an upgrade are sent again.
const KILL_RETRY: Duration = Duration::from_secs(1);

/// The status bar's word on the daemon, from the connection banner alone: empty once the daemon
/// answered, `CONNECTING` until then, an `ERROR_PREFIX` error or a disconnect reason otherwise.
/// What the bar leaves unsaid: all is well.
const CONNECTED_LABEL: &str = "arkosd connected";

fn daemon_label(status: &str) -> &'static str {
    if status.is_empty() {
        CONNECTED_LABEL
    } else if status == CONNECTING {
        "arkosd connecting"
    } else {
        "arkosd: see banner"
    }
}

/// `daemon_label`, except that while the window is reconnecting the bar says so at once.
fn daemon_label_with(status: &str, reconnecting: bool) -> &'static str {
    if reconnecting {
        "arkosd reconnecting"
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

/// Where a palette-driven update is: the daemon replacing itself (the window relaunches once it
/// reconnects), or the window asking to be relaunched and quitting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStage {
    /// The daemon swapping in the staged bundle (`ApplyUpdate` in flight); `Applied` moves on
    /// to "Update now".
    Applying,
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
    /// The file tree's right-click menu while it is open, and while it fades out.
    pub context_menu: Transient<sidebar::ContextMenu>,
    /// Bumped per `git status` run; only the newest run's result is applied.
    pub status_seq: u64,
    /// The `git status` run in flight, and whether another was asked for meanwhile.
    pub status_runs: explorer_watch::StatusRuns,
    /// The worktree explorer's model: the list, the expanded worktrees and their changes.
    pub worktrees: worktrees::Worktrees,
    /// Bumped per `git worktree list` run; only the newest run's result is applied.
    pub worktrees_seq: u64,
    /// Keyboard focus is in the worktree list (Ctrl+Shift+G). Focusing a pane or a tab hands
    /// it back, as for the file tree.
    pub wt_focused: bool,
    /// The create prompt under the Worktrees header while it is open; it holds the keys while
    /// `wt_focused`.
    pub wt_prompt: Transient<sidebar::Prompt>,
    /// The repository's filter drivers, read as the prompt opens; `Unknown` until the read lands.
    pub wt_prompt_drivers: sidebar::Drivers,
    /// A worktree row's right-click menu while it is open: the worktree and the click.
    pub wt_menu: Transient<(PathBuf, gpui::Point<gpui::Pixels>)>,
    /// The Worktrees section's focus handle, and the create prompt's.
    pub wt_focus: FocusHandle,
    pub wt_prompt_focus: FocusHandle,
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
    /// Each pane's drop tint as drawn: it follows `drop_target` a frame at a time, fading and
    /// gliding, so the logic above stays a plain `Option`.
    pub drop_tints: HashMap<PaneId, drag::Tint>,
    /// The focus glow of the focused pane, and of the one it just left while that fades.
    pub glow: motion::Fades<PaneId, ()>,
    /// The find bars as drawn, by terminal or viewer entity: the open bar, and a closed one
    /// while it fades out.
    pub find_bars: motion::Fades<gpui::EntityId, find::FindLook>,
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
    pub notice: Transient<String>,
    /// Bumped by every notice, so an older notice's timer leaves a newer notice up.
    pub notice_seq: u64,
    /// The command palette while it is open (Cmd+Shift+P), and while it fades out.
    pub palette: Transient<command_palette::Palette>,
    /// Each pane's tab strip scroll state, so the active tab can be kept in view.
    pub bar_scroll: HashMap<PaneId, gpui::ScrollHandle>,
    /// The pane whose tab overflow menu is open.
    pub tab_menu: Transient<PaneId>,
    /// The badge: "update ready: <version>" for the release the daemon staged, else for the
    /// binary at the launch path when it differs from this one (`refresh_update_badge`).
    pub update_ready: Option<String>,
    /// What `update::start` last found at the launch path; the staged release takes precedence.
    pub update_ready_on_disk: Option<String>,
    /// The release the daemon has downloaded, verified and staged (protocol 4).
    pub staged: Option<StagedUpdate>,
    /// Why the last update failed, from the daemon or a refused apply; offers "Retry update".
    pub update_error: Option<String>,
    /// The idle timer that applies a staged update is running (`start_idle_timer`).
    pub auto_apply_timer: bool,
    /// "updates unreachable for N days" (`update::unreachable_text`) from the last `UpdateStatus`.
    pub update_unreachable: Option<String>,
    /// The daemon's `last_error` last shown as a notice, so each one is shown once.
    pub update_error_noticed: Option<String>,
    /// The native folder picker is up: Cmd+O again does not open a second one.
    pub picking_folder: bool,
    /// The chooser "Open recent..." opened, while it may still be open: asking again brings it
    /// forward, and closing this window closes it.
    pub chooser: Option<gpui::WindowHandle<launch::Chooser>>,
    /// "Open recent..." is reading the list before its window exists.
    pub chooser_opening: bool,
    /// The last key, pointer move or click in this window: a staged update applies only once the
    /// window has been quiet for `[update] idle_after_secs`.
    pub last_input: Instant,
    /// The update "Update now" started, while it runs.
    pub updating: Option<UpdateStage>,
    /// Each browser tab's placeholder as last laid out, in window pixels, and the frame that drew
    /// it: where its docked Chrome window goes. Filled by the placeholder's `canvas` every frame.
    pub browser_rects: HashMap<TabId, (Bounds<gpui::Pixels>, Instant)>,
    /// Each docked browser tab's window title, its tab label; empty until something docks.
    pub browser_titles: HashMap<TabId, String>,
    /// The Accessibility API, through which Chrome windows are docked (spec 8). UI thread only:
    /// called from the dock tick and from event handlers, never from render.
    pub ax: std::rc::Rc<dyn ax::Ax>,
    /// Each browser tab's dock state; runtime only, never saved.
    pub docks: dock::Docks,
    /// The dock tick's loop is running (`start_dock_timer`); one at a time.
    pub dock_timer: bool,
    /// Chrome's windows and titles, read when the dock list opens.
    pub chrome_windows: Vec<(ax::AxWindow, String)>,
    /// The project window's `NSWindow`, from gpui's window handle at setup: where the
    /// placeholders are on screen and whether the window is minimized or hidden.
    pub ns_window: Option<objc2::rc::Retained<objc2_app_kit::NSWindow>>,
    /// The windows the palette's dock list offers, by row (`Command::DockChromeWindowAt`).
    pub dock_choices: Vec<ax::AxWindow>,
    /// Each pane's body as laid out by the last render, in window points: where a Chrome window
    /// dragged over arkos lands (`browser_drop`).
    pub pane_rects: Vec<(PaneId, Rect)>,
    /// The pane and zone a Chrome window dragged over arkos docks into if released now; the
    /// pane tints it as it tints a tab drag's.
    pub browser_drop: Option<(PaneId, drag::Zone)>,
    /// The Chrome window dragged over arkos (`Drag::In`), until the release.
    browser_drag_in: Option<ax::AxWindow>,
    /// A docked window held past the threshold (`Drag::Out`): the release lets it go.
    browser_drag_out: Option<(TabId, ax::AxWindow, ax::Rect)>,
    /// The browser tab being dragged by its arkos tab, until it lands: released outside the
    /// window, its Chrome window goes to the pointer and the tab closes.
    pub tab_drag_browser: Option<TabId>,
    /// The last tick's snapshot, which `detect_drag` compares against.
    last_snapshot: Option<dock::Snapshot>,
    /// macOS was asked for Accessibility access in this process; it is asked once.
    pub ax_prompted: bool,
    /// The URL prompt ("New browser tab") while it is open, and while it fades out. While open it
    /// holds the keys, through `url_prompt_focus`.
    pub url_prompt: Transient<pane_view::UrlPrompt>,
    pub url_prompt_focus: FocusHandle,
    /// The Show Tabs Vertically tip was dismissed ("Got it"); Task 3 also sets it on the first
    /// dock. Kept for this process only.
    pub vertical_tabs_tip_seen: bool,
    /// Whether this is the key window, read each frame; the pulse ticks only while it is.
    pub window_active: bool,
    /// Where the working glyph's pulse starts; every working tab shares its phase.
    pub pulse_epoch: Instant,
    /// When this frame is drawn: every transition in one render reads the same instant, and
    /// `any_motion` asks for the next frame with it.
    pub frame_now: Instant,
    /// The user is closing the window: the Quit action, or the red button
    /// (`windowShouldClose:`). Never set by macOS's own terminate or the update relaunch.
    pub user_closing: bool,
    /// The Goodbye went out: the Quit action sends it, and the close its quit makes must not
    /// send a second one.
    said_goodbye: bool,
}

/// The folder a worktree tab's fresh shell starts in: its worktree while that is a directory,
/// else `None`, the project.
fn shell_folder(cwd: Option<PathBuf>) -> Option<PathBuf> {
    cwd.filter(|d| d.is_dir())
}

/// Opens a Chrome window for a browser tab (`dock::chrome_open_args`), on the background
/// executor; arkos never waits for Chrome, and what `open` says goes to the log.
fn launch_chrome(url: Option<&str>, app_mode: bool, cx: &mut App) {
    let args = dock::chrome_open_args(url, app_mode);
    cx.background_executor()
        .spawn(async move {
            let status = std::process::Command::new("/usr/bin/open").args(&args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
            match status {
                Ok(s) if s.success() => {}
                Ok(s) => eprintln!("arkos: opening a Chrome window failed ({s}); is Google Chrome installed?"),
                Err(e) => eprintln!("arkos: cannot run /usr/bin/open: {e}"),
            }
        })
        .detach();
}

/// What docking says when Accessibility access is missing.
const DOCKING_NEEDS_ACCESS: &str = "docking a Chrome window needs Accessibility access: allow Arkos IDE in System Settings > Privacy & Security > Accessibility";

/// How often the idle timer asks whether a staged update may apply itself.
const IDLE_TICK: Duration = Duration::from_secs(1);
/// How long after the daemon accepted "Check for updates" the window asks what it found.
const CHECK_RESULT_AFTER: Duration = Duration::from_secs(3);
/// What the daemon's refusal of a second `ApplyUpdate` says (`server.rs`): another window's
/// apply is in flight or waits for its new window.
const ANOTHER_APPLY: &str = "waiting for a window";

/// What arkosd starts with when this window (or the login item) starts it; a running daemon
/// keeps its own settings.
fn daemon_args(config: &config::Config) -> Vec<String> {
    vec![
        "--watchdog-secs".to_string(),
        config.daemon.watchdog_secs.to_string(),
        "--replay-bytes".to_string(),
        config.daemon.replay_bytes.to_string(),
        "--check-every-secs".to_string(),
        config.update.check_every_secs.to_string(),
    ]
}

/// Whether the new window takes focus: not when the daemon started it (`ARKOS_RELAUNCH` set by
/// `watchdog::launch_ui`), so a relaunch never interrupts another app.
fn should_activate(relaunch_env: Option<&str>) -> bool {
    relaunch_env.map_or(true, |v| v.is_empty())
}

#[derive(Debug, PartialEq)]
enum AfterClose {
    Quit,
    CloseChooser,
    Stay,
}

/// What a window closing leads to: the last one quits; the project window gone with only its
/// "Open recent..." chooser left closes that chooser, so the quit follows.
fn after_window_closed(windows_left: usize, project_open: bool, chooser_open: bool) -> AfterClose {
    if windows_left == 0 {
        AfterClose::Quit
    } else if !project_open && chooser_open {
        AfterClose::CloseChooser
    } else {
        AfterClose::Stay
    }
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
        Tab::Terminal { session, .. } => Some(*session),
        Tab::Viewer { .. } | Tab::Browser { .. } => None,
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
        let (sidebar, explorer, worktrees) = match saved.as_ref().and_then(|w| w.sidebar.clone()) {
            Some(s) => (sidebar::Sidebar::new(s.width, s.collapsed, s.explorer_open, s.worktrees_open, cx), explorer::Explorer::new(project.clone(), s.expanded, s.show_ignored), worktrees::Worktrees::new(s.worktrees_expanded)),
            None => (sidebar::Sidebar::new(config.sidebar.width, config.sidebar.collapsed, true, true, cx), explorer::Explorer::new(project.clone(), Vec::new(), false), worktrees::Worktrees::new(Vec::new())),
        };
        // No saved layout: the empty screen, not a shell nobody asked for.
        let tree = saved.map(|ws| ws.tree).unwrap_or_else(Tree::empty);
        let mut sessions = SessionManager::new(client.clone(), Some(client), project.clone());
        let t = &config.terminal;
        sessions.set_defaults(t.scrollback, t.shell.clone(), t.nice);
        // Every keystroke counts as input for the idle rule. An interceptor runs before dispatch,
        // so keys a find bar or the Escape capture stops still count.
        let weak = cx.weak_entity();
        cx.intercept_keystrokes(move |_ev, _window, cx| {
            let _ = weak.update(cx, |root, _| root.last_input = Instant::now());
        })
        .detach();
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
            context_menu: Transient::Hidden,
            status_seq: 0,
            status_runs: explorer_watch::StatusRuns::default(),
            worktrees,
            worktrees_seq: 0,
            wt_focused: false,
            wt_prompt: Transient::Hidden,
            wt_prompt_drivers: sidebar::Drivers::Unknown,
            wt_menu: Transient::Hidden,
            wt_focus: cx.focus_handle(),
            wt_prompt_focus: cx.focus_handle(),
            font_size: preset.size,
            line_height: preset.line_height,
            text_preset,
            content: Rect { x: 0.0, y: 0.0, w: 1200.0, h: 800.0 },
            pane_order: Vec::new(),
            closed: ClosedStack::default(),
            drop_target: None,
            drop_tints: HashMap::new(),
            glow: motion::Fades::default(),
            find_bars: motion::Fades::default(),
            empty_anim: HashMap::new(),
            shell_name: shell_name(config.terminal.shell.as_deref(), std::env::var("SHELL").ok().as_deref()),
            config,
            theme: Arc::new(theme),
            highlighter: None,
            viewers: HashMap::new(),
            system_is_dark,
            config_error: None,
            theme_error: None,
            notice: Transient::Hidden,
            notice_seq: 0,
            palette: Transient::Hidden,
            bar_scroll: HashMap::new(),
            tab_menu: Transient::Hidden,
            update_ready: None,
            update_ready_on_disk: None,
            staged: None,
            update_error: None,
            auto_apply_timer: false,
            update_error_noticed: None,
            picking_folder: false,
            chooser: None,
            chooser_opening: false,
            update_unreachable: None,
            last_input: Instant::now(),
            updating: None,
            browser_rects: HashMap::new(),
            browser_titles: HashMap::new(),
            ax: std::rc::Rc::new(ax::RealAx::new()),
            docks: dock::Docks::default(),
            dock_timer: false,
            chrome_windows: Vec::new(),
            ns_window: None,
            dock_choices: Vec::new(),
            pane_rects: Vec::new(),
            browser_drop: None,
            browser_drag_in: None,
            browser_drag_out: None,
            tab_drag_browser: None,
            last_snapshot: None,
            ax_prompted: false,
            url_prompt: Transient::Hidden,
            url_prompt_focus: cx.focus_handle(),
            vertical_tabs_tip_seen: false,
            window_active: true,
            pulse_epoch: Instant::now(),
            frame_now: Instant::now(),
            user_closing: false,
            said_goodbye: false,
        }
    }

    /// Whether any transition is under way at `now`, so the next frame looks different.
    pub fn any_motion(&self, now: Instant) -> bool {
        self.tab_menu.animating(now)
            || self.context_menu.animating(now)
            || self.wt_menu.animating(now)
            || self.palette.animating(now)
            || self.notice.animating(now)
            || self.wt_prompt.animating(now)
            || self.url_prompt.animating(now)
            || self.drop_tints.values().any(|t| t.animating(now))
            || self.glow.animating(now)
            || self.find_bars.animating(now)
            || self.sidebar.animating(now)
    }

    /// Drops what finished fading out, and the per-key fades the frame drawn at `last_frame`
    /// did not draw.
    fn settle(&mut self, now: Instant, last_frame: Instant) {
        self.drop_tints.retain(|_, t| {
            t.fade.settle(now);
            !t.fade.is_hidden()
        });
        self.glow.settle(now, last_frame);
        self.find_bars.settle(now, last_frame);
        self.tab_menu.settle(now);
        self.context_menu.settle(now);
        self.wt_menu.settle(now);
        self.palette.settle(now);
        self.notice.settle(now);
        self.wt_prompt.settle(now);
        self.url_prompt.settle(now);
    }

    /// The theme the config and the current appearance name, loaded from the themes folder.
    pub fn resolve_theme(&self) -> Result<theme::Theme, String> {
        let name = theme::pick_name(self.config.appearance.mode, self.system_is_dark, &self.config.theme);
        theme::load(name, &paths::themes_dir())
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
            eprintln!("arkos: {e}");
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
            eprintln!("arkos: {e}");
        } else {
            eprintln!("arkos: config reloaded");
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
        // `[update] auto` may have just been turned on with a release already staged.
        self.start_idle_timer(cx);
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

    pub fn save(&mut self, cx: &mut App) {
        let sidebar = SidebarSaved { width: self.sidebar.width, collapsed: self.sidebar.collapsed, show_ignored: self.explorer.show_ignored, expanded: self.explorer.expanded_dirs(), worktrees_expanded: self.worktrees.expanded.iter().cloned().collect(), explorer_open: self.sidebar.explorer_open, worktrees_open: self.sidebar.worktrees_open };
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
        self.drop_tints.remove(&pane);
        self.glow.remove(&pane);
        if self.tab_menu.current(self.frame_now).is_some_and(|(p, _)| *p == pane) {
            // The pane is gone, and the menu with nothing to fade over.
            self.tab_menu.clear();
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
            .chain(self.drop_tints.keys())
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
            Some(Tab::Terminal { session, .. }) => self.sessions.terminals.get(session).map(|t| {
                let core = &t.read(cx).core;
                (core.columns(), core.screen_lines())
            }),
            _ => None,
        };
        grid_choice(self.grids.get(&pane).copied(), terminal)
    }

    pub fn active_terminal(&self, _cx: &App) -> Option<Entity<Terminal>> {
        let p = self.tree.pane(self.tree.focused)?;
        match p.tabs.get(p.active)? {
            Tab::Terminal { session, .. } => self.sessions.terminals.get(session).cloned(),
            Tab::Viewer { .. } | Tab::Browser { .. } => None,
        }
    }

    /// Focuses a pane, exiting another pane's maximize so the focused pane is on screen.
    pub fn focus_pane(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        self.sidebar_focused = false;
        self.wt_focused = false;
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
        self.wt_focused = false;
        self.tree.activate(pane, idx);
        self.acknowledge_focused(cx);
        if let Some(h) = self.bar_scroll.get(&pane) {
            h.scroll_to_item(idx);
        }
        self.save(cx);
        cx.notify();
    }

    pub fn toggle_tab_menu(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        let now = Instant::now();
        if self.tab_menu.value() == Some(&pane) {
            self.tab_menu.hide(now);
        } else {
            self.tab_menu.show(pane, now);
            self.focus_pane(pane, cx);
        }
        cx.notify();
    }

    pub fn close_tab_menu(&mut self, cx: &mut Context<Self>) {
        if self.tab_menu.hide(Instant::now()) {
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
        if let Tab::Terminal { session, cwd } = tab {
            if session != SessionId(0) {
                let ended = self.sessions.terminals.get(&session).map(|t| !t.read(cx).alive).unwrap_or(false);
                if ended {
                    // Nothing to bring back: end the session now instead of a minute from now.
                    eprintln!("arkos: closing ended session {}", session.0);
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
                for old in self.closed.push(ClosedTab { closed: Closed::Session(session, cwd), pane, closed_at: Instant::now() }) {
                    eprintln!("arkos: more than {MAX_CLOSED} closed tabs, ending session {}", old.0);
                    self.sessions.kill(old);
                }
            }
            // A shell still starting for this tab is killed when it arrives (`handle_outcome`).
            self.sessions.tab_closed(id);
        } else {
            let closed = match tab {
                Tab::Viewer { path } => Closed::Viewer(path),
                Tab::Browser { url, app_mode } => {
                    // Its window is released where it was last shown; arkos never closes it.
                    self.release_browser_tab(id, cx);
                    Closed::Browser { url, app_mode }
                }
                Tab::Terminal { .. } => unreachable!("handled above"),
            };
            for old in self.closed.push(ClosedTab { closed, pane, closed_at: Instant::now() }) {
                eprintln!("arkos: more than {MAX_CLOSED} closed tabs, ending session {}", old.0);
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
    /// ended meanwhile shows as ended. `cwd` is the worktree it was opened in, if any.
    fn reopen_session(&mut self, pane: PaneId, session: SessionId, cwd: Option<PathBuf>, cx: &mut Context<Self>) {
        let idx = self.tree.add_tab(pane, Tab::Terminal { session, cwd });
        self.tree.focus(pane);
        if let Some(id) = self.tree.tab_id(pane, idx) {
            eprintln!("arkos: reopening session {} in pane {pane}", session.0);
            self.sessions.attach(session, id, false);
        }
        self.save(cx);
        cx.notify();
    }

    /// A restore whose tab is gone. Ends the session unless something can still show it.
    fn restore_tab_gone(&mut self, session: SessionId, tab: TabId) {
        if self.tree.find_session(session).is_some() {
            // Closed and reopened meanwhile: the new tab's own restore owns the session.
            eprintln!("arkos: tab {tab} closed during its restore; session {} was reopened elsewhere", session.0);
        } else if self.closed.contains(session) {
            // Closed by the user: already detached, and killed when its grace runs out.
            eprintln!("arkos: tab {tab} closed during its restore; session {} waits for Cmd+Shift+T", session.0);
            self.sessions.abort_restore(session, tab);
        } else {
            eprintln!("arkos: tab {tab} closed during its restore, ending session {}", session.0);
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
        eprintln!("arkos: cannot show session {} in tab {tab} ({why}); starting a new shell there", session.0);
        self.sessions.detach(session);
        let cwd = self.tab_cwd(tab);
        self.tree.replace_tab(pane, idx, Tab::Terminal { session: SessionId(0), cwd: cwd.clone() });
        self.start_shell(tab, cwd, true);
        self.save(cx);
        cx.notify();
    }

    /// The worktree a terminal tab was opened in; `None` for the project or a viewer.
    fn tab_cwd(&self, tab: TabId) -> Option<PathBuf> {
        let (pane, idx) = self.tree.locate(tab)?;
        match self.tree.pane(pane)?.tabs.get(idx)? {
            Tab::Terminal { cwd, .. } => cwd.clone(),
            Tab::Viewer { .. } | Tab::Browser { .. } => None,
        }
    }

    /// Starts a fresh shell for `tab` in its folder: the project, or the worktree it was opened
    /// in. A worktree gone meanwhile gives the project, and the tab forgets it, so the next
    /// restart checks the shell against the folder it really started in.
    fn start_shell(&mut self, tab: TabId, cwd: Option<PathBuf>, note: bool) {
        let wanted = cwd.is_some();
        match shell_folder(cwd) {
            Some(dir) => self.sessions.create_in(tab, note, dir),
            None if wanted => {
                if let Some((pane, idx)) = self.tree.locate(tab) {
                    if let Some(Tab::Terminal { session, .. }) = self.tree.pane(pane).and_then(|p| p.tabs.get(idx)).cloned() {
                        self.tree.replace_tab(pane, idx, Tab::Terminal { session, cwd: None });
                    }
                }
                self.sessions.create(tab, note);
            }
            None => self.sessions.create(tab, note),
        }
    }

    /// Kills the sessions of closed tabs whose grace has run out.
    pub fn expire_closed(&mut self, now: Instant) {
        for session in self.closed.expired(now) {
            eprintln!("arkos: session {} was not reopened within {}s, ending it", session.0, CLOSE_GRACE.as_secs());
            self.sessions.kill(session);
        }
    }

    /// Kills every closed tab's session, for quitting: nothing could reopen them afterwards.
    /// Returns how many were killed.
    pub fn end_closed_tabs(&mut self) -> usize {
        let sessions = self.closed.drain_all();
        for session in &sessions {
            eprintln!("arkos: quitting, ending closed tab's session {}", session.0);
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
            eprintln!("arkos: {text}");
            self.show_notice(text, cx);
        }
    }

    /// Shows a one-off notice as its own banner and takes it down after `NOTICE_FOR`, unless a
    /// newer notice or a command has replaced or cleared it by then.
    pub fn show_notice(&mut self, text: String, cx: &mut Context<Self>) {
        self.notice_seq += 1;
        let seq = self.notice_seq;
        self.notice.show(text, Instant::now());
        cx.notify();
        cx.spawn(async move |root, cx| {
            cx.background_executor().timer(NOTICE_FOR).await;
            let _ = root.update(cx, |root, cx| {
                if root.notice_seq == seq && root.notice.hide(Instant::now()) {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Cmd+Shift+P: opens the palette with every action, preset and theme, or closes it.
    pub fn toggle_palette(&mut self, cx: &mut Context<Self>) {
        if self.palette.value().is_some() {
            self.close_palette(cx);
            return;
        }
        let themes_dir = paths::themes_dir();
        let items = command_palette::items(&self.config, &theme::names(&themes_dir), &self.text_preset, &self.theme.name, &themes_dir, self.update_ready.as_deref(), self.update_error.is_some(), self.config.recovery.start_at_login, launch_agent::installed(&launch_agent::agent_path()), &hook_cli::claude_settings_path(), self.explorer.show_ignored);
        self.palette.show(command_palette::Palette::new(items, cx.focus_handle()), Instant::now());
        cx.notify();
    }

    pub fn close_palette(&mut self, cx: &mut Context<Self>) {
        self.palette.hide(Instant::now());
        cx.notify();
    }

    /// A key typed while the palette has focus: edits the query, moves the selection, runs the
    /// selected command (Enter) or closes the palette (Escape). Cmd+V pastes into the query.
    pub fn palette_key(&mut self, ks: &gpui::Keystroke, window: &mut Window, cx: &mut Context<Self>) {
        if ks.modifiers.platform && ks.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                if let Some(p) = self.palette.value_mut() {
                    p.query.insert_str(&text);
                    p.refilter();
                    cx.notify();
                }
            }
            return;
        }
        let Some(p) = self.palette.value_mut() else { return };
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
        let cmd = self.palette.value().and_then(|p| p.selected_command());
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
        self.tab_drag_browser = None;
        if self.tree.locate(d.tab).is_some() {
            self.preview_tab = after_drop(self.preview_tab, d.tab);
        }
        if self.place_tab(d.tab, target) {
            self.save(cx);
        }
        cx.notify();
    }

    /// Moves the tab with id `tab` to `target`: onto a tab bar, into a pane (center), or into a
    /// new pane split off one (an edge zone). The source pane closes when this emptied it.
    /// False when nothing changed.
    fn place_tab(&mut self, tab: TabId, target: drag::DropTarget) -> bool {
        let Some((source, idx)) = self.tree.locate(tab) else { return false };
        match target {
            drag::DropTarget::TabBar { pane, at } => {
                // Back onto its own place, `move_tab` changes nothing.
                self.tree.move_tab(source, idx, pane, Some(at));
            }
            drag::DropTarget::Pane { pane, zone: drag::Zone::Center } => {
                if pane == source {
                    return false;
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
                    return false;
                }
                if let Some((id, tab)) = self.tree.take_tab(source, idx) {
                    self.tree.split_with_tab(pane, side, id, tab);
                }
            }
        }
        if self.tree.pane(source).map(|p| p.tabs.is_empty()).unwrap_or(false) {
            self.tree.close_pane(source);
        }
        true
    }

    pub fn new_terminal_tab(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        self.new_terminal_tab_in(pane, None, cx);
    }

    /// `new_terminal_tab`, with the shell in `cwd` (a worktree) while that folder exists.
    fn new_terminal_tab_in(&mut self, pane: PaneId, cwd: Option<PathBuf>, cx: &mut Context<Self>) {
        let idx = self.tree.add_tab(pane, Tab::Terminal { session: SessionId(0), cwd: cwd.clone() });
        self.tree.focus(pane);
        if let Some(id) = self.tree.tab_id(pane, idx) {
            self.start_shell(id, cwd, false);
        }
        self.save(cx);
        cx.notify();
    }

    /// A browser tab in `pane` (the focused pane when that one is gone) that opens `url` (`None`:
    /// a blank window), active and focused. Chrome opens a window for it, which docks.
    pub fn new_browser_tab(&mut self, pane: PaneId, url: Option<String>, app_mode: bool, cx: &mut Context<Self>) {
        self.add_browser_tab(pane, url, app_mode, true, cx);
    }

    /// `new_browser_tab`; with `launch` false nothing opens and the tab waits for a window.
    fn add_browser_tab(&mut self, pane: PaneId, url: Option<String>, app_mode: bool, launch: bool, cx: &mut Context<Self>) -> Option<TabId> {
        let pane = if self.tree.pane(pane).is_some() { pane } else { self.tree.focused };
        let idx = self.tree.add_tab(pane, Tab::Browser { url: url.clone(), app_mode });
        self.tree.focus(pane);
        let id = self.tree.tab_id(pane, idx);
        if let Some(id) = id {
            self.docks.add(id, launch, Instant::now());
            self.ask_for_access_once();
            // A tick now, before Chrome is asked: the windows already open are known as old,
            // and the one launched for this tab is new however soon it appears.
            self.dock_tick(cx);
            // Without access the launch waits for the grant (`Action::Launch`): a window opened
            // now would be learned as old when access comes, and never dock.
            if launch && !self.docks.launch_pending(id) {
                launch_chrome(url.as_deref(), app_mode, cx);
            }
        }
        // Saves, and scrolls the new tab into view.
        self.activate_tab(pane, idx, cx);
        id
    }

    /// Opens the URL prompt over the focused pane. `target` is the waiting browser tab to point
    /// at the URL; `None` opens a new tab. Already open, it keeps what was typed.
    pub fn open_url_prompt(&mut self, app_mode: bool, target: Option<TabId>, cx: &mut Context<Self>) {
        if self.url_prompt.value().is_none() {
            self.url_prompt.show(pane_view::UrlPrompt::new(app_mode, target), Instant::now());
        }
        self.sidebar_focused = false;
        self.wt_focused = false;
        cx.notify();
    }

    /// A key typed while the URL prompt has focus: edits the field (Cmd+V pastes), opens the URL
    /// on Enter (an empty field opens `[browser] url`), closes on Escape. Closing hands the keys
    /// back to the pane.
    pub fn url_prompt_key(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) {
        let configured = self.config.browser.url.clone();
        let Some(p) = self.url_prompt.value_mut() else { return };
        if ks.modifiers.platform && ks.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                p.edit.insert_str(&text);
                cx.notify();
            }
            return;
        }
        let (app_mode, target) = (p.app_mode, p.target);
        match p.key(ks, &configured) {
            pane_view::UrlStep::Stay => {}
            pane_view::UrlStep::Cancel => {
                self.url_prompt.hide(Instant::now());
            }
            pane_view::UrlStep::Open(url) => {
                self.url_prompt.hide(Instant::now());
                match target.and_then(|t| self.tree.locate(t)) {
                    Some((pane, idx)) => {
                        self.tree.replace_tab(pane, idx, Tab::Browser { url: url.clone(), app_mode });
                        self.tree.focus(pane);
                        if let Some(tab) = target.filter(|t| self.docks.docked_window(*t).is_none()) {
                            // The waiting tab docks the window launched for it (ticked first, as
                            // in `add_browser_tab`).
                            self.docks.add(tab, true, Instant::now());
                            self.dock_tick(cx);
                            if !self.docks.launch_pending(tab) {
                                launch_chrome(url.as_deref(), app_mode, cx);
                            }
                        }
                        self.activate_tab(pane, idx, cx);
                    }
                    None => self.new_browser_tab(self.tree.focused, url, app_mode, cx),
                }
            }
        }
        cx.notify();
    }

    /// Where a browser tab's placeholder was drawn this frame, from its `canvas`.
    pub fn report_browser_rect(&mut self, tab: TabId, bounds: Bounds<gpui::Pixels>) {
        self.browser_rects.insert(tab, (bounds, self.frame_now));
    }

    /// A click in a browser tab's placeholder (arkos covers its docked window): the window comes
    /// back up and takes the keys.
    pub fn browser_clicked(&mut self, tab: TabId, _cx: &mut Context<Self>) {
        if let Some(win) = self.docks.docked_window(tab).cloned() {
            let r = self.ax.focus(&win);
            self.ax_result(r);
        }
    }

    /// "Grant access" on the permission screen: macOS asks (its own dialog), then a tick reads
    /// the answer.
    pub fn request_accessibility(&mut self, cx: &mut Context<Self>) {
        self.ax_prompted = true;
        if !self.ax.trusted(true) {
            self.show_notice(DOCKING_NEEDS_ACCESS.into(), cx);
        }
        self.dock_tick(cx);
    }

    /// The first browser tab of the process asks macOS for Accessibility access (spec 8); later
    /// ones leave it to the permission screen's "Grant access".
    fn ask_for_access_once(&mut self) {
        if !self.ax_prompted {
            self.ax_prompted = true;
            self.ax.trusted(true);
        }
    }

    /// An AX write's result: refused for want of trust, the next tick re-reads the permission.
    /// A window gone is left to the next tick, which sees it missing.
    fn ax_result(&mut self, r: Result<(), ax::AxError>) {
        if r == Err(ax::AxError::NotTrusted) {
            self.docks.distrust();
        }
    }

    /// The browser tabs in reading order.
    fn browser_tabs(&self) -> Vec<TabId> {
        self.tree.tabs().into_iter().filter(|(_, _, t)| matches!(t, Tab::Browser { .. })).filter_map(|(p, i, _)| self.tree.tab_id(p, i)).collect()
    }

    /// Keeps `docks` to the tree's browser tabs, whatever added or removed them (a restored
    /// workspace, a pane closed with its tabs): a new one waits, a gone one releases its window.
    /// Starts the tick when a tab needs it.
    fn sync_docks(&mut self, cx: &mut Context<Self>) {
        let tabs = self.browser_tabs();
        let gone: Vec<TabId> = self.docks.tabs.keys().filter(|t| !tabs.contains(t)).copied().collect();
        for tab in gone {
            self.release_browser_tab(tab, cx);
        }
        let now = Instant::now();
        for tab in tabs {
            if !self.docks.tabs.contains_key(&tab) {
                self.docks.add(tab, false, now);
            }
        }
        self.start_dock_timer(cx);
    }

    /// A browser tab is going: its window, if docked, is left where it was last shown (a hidden
    /// one comes back from off-screen), and everything kept for the tab is dropped.
    fn release_browser_tab(&mut self, tab: TabId, cx: &mut Context<Self>) {
        let released = self.docks.remove(tab);
        self.browser_rects.remove(&tab);
        self.browser_titles.remove(&tab);
        if let Some((win, leave_at)) = released {
            self.apply_actions(vec![dock::Action::Undock { tab, win: Some(win), leave_at }], cx);
        }
    }

    /// Quitting: every parked window comes back where it was, so none is left off-screen.
    fn unpark_windows(&mut self) {
        for (win, rect) in self.docks.parked_windows() {
            if let Err(e) = self.ax.set_frame(&win, rect) {
                eprintln!("arkos: cannot put a hidden Chrome window back on screen: {e:?}");
            }
        }
    }

    /// The Chrome window for a tab whose launch waited for access: its URL from the tree.
    fn launch_for(&mut self, tab: TabId, cx: &mut Context<Self>) {
        let Some((pane, idx)) = self.tree.locate(tab) else { return };
        if let Some(Tab::Browser { url, app_mode }) = self.tree.pane(pane).and_then(|p| p.tabs.get(idx)) {
            let (url, app_mode) = (url.clone(), *app_mode);
            launch_chrome(url.as_deref(), app_mode, cx);
        }
    }

    /// Ticks the docking engine at its cadence (`Docks::cadence`): 1 Hz while tabs wait, 20 Hz
    /// while a window is docked or the mouse is down (an idle second is slept in 50 ms slices
    /// that check only the button, so a press ticks at once); it stops with the last browser tab and
    /// `sync_docks` or a new tab starts it again. One loop at a time.
    pub fn start_dock_timer(&mut self, cx: &mut Context<Self>) {
        if self.dock_timer || self.docks.tabs.is_empty() {
            return;
        }
        self.dock_timer = true;
        cx.spawn(async move |root, cx| loop {
            let wait = root.update(cx, |root, _| {
                let wait = root.docks.cadence(root.ax.mouse_down());
                if wait.is_none() {
                    root.dock_timer = false;
                }
                wait
            });
            let Ok(Some(wait)) = wait else { break };
            if wait <= dock::TRACK_POLL {
                cx.background_executor().timer(wait).await;
            } else {
                // Idle (tabs waiting): the AX work stays at `wait`, but the button is checked
                // every `TRACK_POLL` (a class-method read, no AX call), so a Chrome window
                // dragged over arkos is seen at once.
                let start = Instant::now();
                loop {
                    cx.background_executor().timer(dock::TRACK_POLL).await;
                    let Ok(down) = root.update(cx, |root, _| root.ax.mouse_down()) else { return };
                    if down || start.elapsed() >= wait {
                        break;
                    }
                }
            }
            if root.update(cx, |root, cx| root.dock_tick(cx)).is_err() {
                break;
            }
        })
        .detach();
    }

    /// One docking step on the UI thread: reads Chrome's windows and the placeholders, lets the
    /// engine decide, applies what it decided.
    pub fn dock_tick(&mut self, cx: &mut Context<Self>) {
        self.sync_docks(cx);
        if self.docks.tabs.is_empty() {
            self.last_snapshot = None;
            return;
        }
        let snapshot = self.dock_snapshot(Instant::now());
        let drag = match &self.last_snapshot {
            Some(prev) => self.docks.detect_drag(&snapshot, prev),
            None => dock::Drag::None,
        };
        self.follow_drag(drag, &snapshot, cx);
        if !snapshot.mouse_down {
            self.release_drags(&snapshot, cx);
        }
        let actions = self.docks.tick(&snapshot);
        self.apply_actions(actions, cx);
        if !snapshot.mouse_down && (self.browser_drag_in.is_some() || self.browser_drop.is_some()) {
            // After the tick: the new tab's placeholder is drawn in the next frame, and the tick
            // after it puts the window there.
            self.finish_browser_drop(cx);
        }
        self.last_snapshot = Some(snapshot);
        // A window the palette took from another tab leaves that tab's title behind.
        let before = self.browser_titles.len();
        let docks = &self.docks;
        self.browser_titles.retain(|t, _| docks.docked_window(*t).is_some());
        if self.browser_titles.len() != before {
            cx.notify();
        }
    }

    /// What the tick reads: the permission, Chrome's window list (`None` when it cannot be read
    /// now), and the frame and title of the docked windows only; the placeholders drawn in the
    /// last frame, in global coordinates; the arkos window's state.
    fn dock_snapshot(&mut self, now: Instant) -> dock::Snapshot {
        let ax = self.ax.clone();
        let trusted = ax.trusted(false);
        let chrome_pid = if trusted { ax.chrome_pid() } else { None };
        let listed = if !trusted {
            None
        } else {
            match chrome_pid {
                Some(pid) => ax.windows(pid).ok(),
                None => Some(Vec::new()),
            }
        };
        let docked: Vec<ax::AxWindow> = self.docks.tabs.keys().filter_map(|t| self.docks.docked_window(*t).cloned()).collect();
        // With the button down every window's frame is read: one may be dragged over arkos.
        let mouse_down = ax.mouse_down();
        let mut titles = Vec::new();
        let windows = listed.map(|all| {
            all.into_iter()
                .map(|w| {
                    let frame = if docked.contains(&w) {
                        titles.push((w.clone(), ax.title(&w).unwrap_or_default()));
                        ax.frame(&w)
                    } else if mouse_down {
                        ax.frame(&w)
                    } else {
                        None
                    };
                    (w, frame, false)
                })
                .collect()
        });
        let ns_window = self.ns_window.clone().or_else(ax::screen::arkos_window);
        let arkos = ns_window.map(|w| (ax::screen::content_rect_in_screen(&w), ax::screen::window_hidden(&w)));
        let placeholders = self
            .browser_tabs()
            .into_iter()
            .map(|tab| {
                // Drawn in the last frame: its tab is the active one in a pane on screen.
                let rect = self.browser_rects.get(&tab).filter(|(_, at)| *at == self.frame_now).zip(arkos.as_ref()).map(|((b, _), (content, _))| ax::Rect {
                    x: content.x + f64::from(f32::from(b.origin.x)),
                    y: content.y + f64::from(f32::from(b.origin.y)),
                    w: f64::from(f32::from(b.size.width)),
                    h: f64::from(f32::from(b.size.height)),
                });
                (tab, rect)
            })
            .collect();
        dock::Snapshot {
            trusted,
            chrome_pid,
            windows,
            titles,
            placeholders,
            any_working: self.sessions.any_working(&self.tree),
            mouse_down,
            window_active: self.window_active,
            window_hidden: arkos.is_some_and(|(_, hidden)| hidden),
            screens_right: ax::screen::screens_right(),
            arkos_frame: arkos.map(|(content, _)| content),
            pointer: ax.mouse_location(),
            now,
        }
    }

    /// Carries out the engine's decisions through `ax`. A window that docked or came back on
    /// screen is raised once it is in place, so it shows above arkos.
    fn apply_actions(&mut self, actions: Vec<dock::Action>, cx: &mut Context<Self>) {
        use dock::Action;
        let ax = self.ax.clone();
        let mut raise: Vec<ax::AxWindow> = Vec::new();
        let mut notify = false;
        for action in actions {
            match action {
                Action::Dock { win, .. } => {
                    self.vertical_tabs_tip_seen = true;
                    raise.push(win);
                    notify = true;
                }
                Action::Move { win, rect } | Action::Hide { win, to: rect } => {
                    let r = ax.set_frame(&win, rect);
                    self.ax_result(r);
                }
                Action::Show { win, rect } => {
                    let r = ax.set_frame(&win, rect);
                    self.ax_result(r);
                    raise.push(win);
                }
                Action::Raise(win) => raise.push(win),
                Action::Undock { tab, win, leave_at } => {
                    if let (Some(win), Some(rect)) = (win, leave_at) {
                        let r = ax.set_frame(&win, rect);
                        self.ax_result(r);
                    }
                    self.browser_titles.remove(&tab);
                    notify = true;
                }
                Action::Title { tab, title } => {
                    self.browser_titles.insert(tab, title);
                    notify = true;
                }
                Action::Permission(granted) => {
                    if !granted {
                        self.browser_titles.clear();
                    }
                    notify = true;
                }
                Action::Launch { tab } => self.launch_for(tab, cx),
            }
        }
        let mut raised: Vec<ax::AxWindow> = Vec::new();
        for win in raise {
            if !raised.contains(&win) {
                let r = ax.raise(&win);
                self.ax_result(r);
                raised.push(win);
            }
        }
        if notify {
            cx.notify();
        }
    }

    /// Chrome's windows with their titles, read now (an event handler).
    fn read_chrome_windows(&mut self) {
        let ax = self.ax.clone();
        let all = ax.chrome_pid().and_then(|pid| ax.windows(pid).ok()).unwrap_or_default();
        self.chrome_windows = all.into_iter().map(|w| { let t = ax.title(&w).unwrap_or_default(); (w, t) }).collect();
    }

    /// "Dock a Chrome window": the palette lists Chrome's windows not docked yet; the one picked
    /// docks (`DockChromeWindowAt`).
    fn open_dock_list(&mut self, cx: &mut Context<Self>) {
        self.ask_for_access_once();
        if !self.ax.trusted(false) {
            self.show_notice(DOCKING_NEEDS_ACCESS.into(), cx);
            return;
        }
        self.read_chrome_windows();
        let docks = &self.docks;
        let free: Vec<(ax::AxWindow, String)> = self.chrome_windows.iter().filter(|(w, _)| !docks.tabs.keys().any(|t| docks.docked_window(*t) == Some(w))).cloned().collect();
        if free.is_empty() {
            self.show_notice("no Chrome window to dock: open one in Chrome first".into(), cx);
            return;
        }
        let items = free
            .iter()
            .enumerate()
            .map(|(i, (_, title))| command_palette::Item {
                label: if title.is_empty() { "Untitled Chrome window".to_string() } else { title.clone() },
                detail: "Chrome window".to_string(),
                command: keymap::Command::DockChromeWindowAt(i),
            })
            .collect();
        self.dock_choices = free.into_iter().map(|(w, _)| w).collect();
        self.palette.show(command_palette::Palette::list(items, "pick a Chrome window to dock", cx.focus_handle()), Instant::now());
        cx.notify();
    }

    /// The tab a picked window docks into: the focused pane's active browser tab, else the first
    /// waiting browser tab in reading order, else a new browser tab in the focused pane.
    fn dock_target(&mut self, cx: &mut Context<Self>) -> Option<TabId> {
        let focused = self.tree.focused;
        let active = self.tree.pane(focused).and_then(|p| match p.tabs.get(p.active) {
            Some(Tab::Browser { .. }) => self.tree.tab_id(focused, p.active),
            _ => None,
        });
        let waiting = self.docks.waiting_tabs();
        active.or_else(|| self.browser_tabs().into_iter().find(|t| waiting.contains(t))).or_else(|| self.add_browser_tab(focused, None, false, false, cx))
    }

    /// Docks `win` into `tab` and puts it in place at once. A window `tab` had is left where it is.
    pub fn dock_window_into(&mut self, tab: TabId, win: ax::AxWindow, cx: &mut Context<Self>) {
        if let Some((pane, idx)) = self.tree.locate(tab) {
            self.activate_tab(pane, idx, cx);
        }
        self.docks.dock_by_choice(tab, win.clone());
        self.vertical_tabs_tip_seen = true;
        self.start_dock_timer(cx);
        // The placeholder is drawn in the next frame; the tick that follows moves the window.
        // Raised now, it is above arkos when it lands.
        let r = self.ax.raise(&win);
        self.ax_result(r);
        cx.notify();
    }

    /// "Undock Chrome window": the tab stays, waiting; its window stays where it is.
    pub fn undock_tab(&mut self, tab: TabId, cx: &mut Context<Self>) {
        let Some((win, leave_at)) = self.docks.undock(tab) else {
            self.show_notice("this tab has no docked Chrome window".into(), cx);
            return;
        };
        self.apply_actions(vec![dock::Action::Undock { tab, win: Some(win), leave_at }], cx);
    }

    /// A docked window dragged out (by its title bar past the threshold, or by its arkos tab out
    /// of the window): it is released at `rect` and its tab closes, reopenable like any closed
    /// browser tab. A tab with no docked window keeps its place.
    pub fn undock_at(&mut self, tab: TabId, rect: ax::Rect, cx: &mut Context<Self>) {
        let Some((win, _)) = self.docks.undock(tab) else { return };
        self.apply_actions(vec![dock::Action::Undock { tab, win: Some(win), leave_at: Some(rect) }], cx);
        if let Some((pane, idx)) = self.tree.locate(tab) {
            // Pushes `Closed::Browser`; the window is no longer docked, so nothing moves it back.
            self.close_tab(pane, idx, cx);
        }
    }

    /// A Chrome window dragged over arkos was released: over a pane it docks into a new browser
    /// tab there (center) or in a new pane split off it (an edge zone), the way a dropped tab
    /// lands; anywhere else nothing docks. The tint goes either way.
    pub fn finish_browser_drop(&mut self, cx: &mut Context<Self>) {
        let win = self.browser_drag_in.take();
        if self.browser_drop.is_some() {
            cx.notify();
        }
        let (Some(win), Some((pane, zone))) = (win, self.browser_drop.take()) else { return };
        if self.tree.pane(pane).is_none() {
            return;
        }
        let idx = self.tree.add_tab(pane, Tab::Browser { url: None, app_mode: false });
        let Some(tab) = self.tree.tab_id(pane, idx) else { return };
        if zone != drag::Zone::Center {
            // An empty pane cannot split off its only tab: the window docks in it instead.
            self.place_tab(tab, drag::DropTarget::Pane { pane, zone });
        }
        if let Some((pane, _)) = self.tree.locate(tab) {
            self.tree.focus(pane);
        }
        self.docks.add(tab, false, Instant::now());
        self.dock_window_into(tab, win, cx);
    }

    /// The dock tick's view of the user's hand: while the button is down, a window dragged over
    /// arkos aims the pane tint, a docked one held past the threshold is marked to go.
    fn follow_drag(&mut self, drag: dock::Drag, s: &dock::Snapshot, cx: &mut Context<Self>) {
        match drag {
            dock::Drag::In { win, pointer } => {
                let panes: Vec<(PaneId, ax::Rect)> = match s.arkos_frame {
                    Some(c) => self.pane_rects.iter().map(|(p, r)| (*p, ax::Rect { x: c.x + f64::from(r.x), y: c.y + f64::from(r.y), w: f64::from(r.w), h: f64::from(r.h) })).collect(),
                    None => Vec::new(),
                };
                let target = drag::zone_and_pane_for(&panes, pointer);
                self.browser_drag_in = Some(win);
                self.browser_drag_out = None;
                if self.browser_drop != target {
                    self.browser_drop = target;
                    cx.notify();
                }
            }
            dock::Drag::Out { tab, win, frame } => {
                self.browser_drag_out = Some((tab, win, frame));
                self.browser_drag_in = None;
                if self.browser_drop.take().is_some() {
                    cx.notify();
                }
            }
            // With the button up the drags are settled by the release (`release_drags`,
            // `finish_browser_drop`); with it down, nothing is being dragged now.
            dock::Drag::None if s.mouse_down => {
                self.browser_drag_in = None;
                self.browser_drag_out = None;
                if self.browser_drop.take().is_some() {
                    cx.notify();
                }
            }
            dock::Drag::None => {}
        }
    }

    /// The button came up: a docked window held past the threshold is released where it is, and
    /// a browser tab dragged out of the arkos window releases its window at the pointer, its
    /// current size kept. Before the engine's tick, which would put them back.
    fn release_drags(&mut self, s: &dock::Snapshot, cx: &mut Context<Self>) {
        let frame_of = |win: &ax::AxWindow| s.windows.iter().flatten().find(|(w, _, _)| w == win).and_then(|(_, f, _)| *f);
        if let Some((tab, win, frame)) = self.browser_drag_out.take() {
            self.undock_at(tab, frame_of(&win).unwrap_or(frame), cx);
        }
        if let Some(tab) = self.tab_drag_browser.take() {
            let (px, py) = s.pointer;
            let outside = s.arkos_frame.is_some_and(|a| !a.contains(px, py));
            if let (true, Some(win)) = (outside, self.docks.docked_window(tab).cloned()) {
                let size = frame_of(&win).or_else(|| self.docks.last_shown(tab)).unwrap_or(ax::Rect { x: 0.0, y: 0.0, w: 800.0, h: 600.0 });
                // The title bar under the pointer, as if the window had been carried there.
                self.undock_at(tab, ax::Rect { x: px - size.w / 2.0, y: py, ..size }, cx);
            }
        }
    }

    /// Splits the focused pane and starts a fresh shell in the new one.
    pub fn split_focused(&mut self, side: Side, cx: &mut Context<Self>) {
        let pane = self.tree.focused;
        if let Some(new_pane) = self.tree.split(pane, side, Tab::Terminal { session: SessionId(0), cwd: None }) {
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
        if self.notice.hide(Instant::now()) {
            cx.notify();
        }
        // A command on the focused pane (Cmd+F, Cmd+T, a split) takes the keys back from the
        // sidebar.
        if (self.sidebar_focused || self.wt_focused) && !keeps_tree_focus(&cmd) {
            self.sidebar_focused = false;
            self.wt_focused = false;
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
                // pinned, its file read again; a browser tab comes back waiting for a window.
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
                    // Its old window may still be open: the tab waits rather than launch another.
                    Closed::Browser { url, app_mode } => {
                        self.add_browser_tab(pane, url, app_mode, false, cx);
                    }
                    Closed::Session(session, cwd) if in_grace => self.reopen_session(pane, session, cwd, cx),
                    Closed::Session(session, cwd) => {
                        eprintln!("arkos: session {} closed more than {}s ago, ending it and opening a new shell", session.0, CLOSE_GRACE.as_secs());
                        self.sessions.kill(session);
                        // A worktree terminal reopens in its worktree, if it is still there.
                        self.new_terminal_tab_in(pane, cwd, cx);
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
            CheckForUpdates => {
                self.sessions.check_for_updates();
                self.show_notice("checking for updates".into(), cx);
            }
            RetryUpdate => {
                self.update_error = None;
                self.refresh_update_badge(cx);
                self.sessions.check_for_updates();
                // A release still staged gets another go once the window is idle.
                self.start_idle_timer(cx);
            }
            StartAtLogin => self.toggle_start_at_login(cx),
            InstallHooks => {
                let exe = update::launch_path().unwrap_or_else(|| PathBuf::from("arkos"));
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
                self.sidebar.set_collapsed(!self.sidebar.collapsed, Instant::now());
                // A hidden sidebar cannot hold the keys: they go back to the focused pane.
                if self.sidebar.collapsed {
                    self.sidebar_focused = false;
                    self.wt_focused = false;
                }
                // The panes change width: measurements taken before are stale, as on a resize.
                self.grids.clear();
                self.save(cx);
                cx.notify();
            }
            FocusFileTree => {
                if self.sidebar.collapsed {
                    self.sidebar.set_collapsed(false, Instant::now());
                    self.grids.clear();
                    self.save(cx);
                }
                if !self.sidebar.explorer_open {
                    self.sidebar.explorer_open = true;
                    self.save(cx);
                }
                self.sidebar_focused = true;
                self.wt_focused = false;
                cx.notify();
            }
            FocusWorktrees => {
                if self.sidebar.collapsed {
                    self.sidebar.set_collapsed(false, Instant::now());
                    self.grids.clear();
                    self.save(cx);
                }
                if !self.sidebar.worktrees_open {
                    self.sidebar.worktrees_open = true;
                    self.save(cx);
                }
                self.wt_focused = true;
                self.sidebar_focused = false;
                cx.notify();
            }
            ShowIgnoredFiles => {
                self.explorer.show_ignored = !self.explorer.show_ignored;
                self.save(cx);
                cx.notify();
            }
            ToggleInlineDiff => {
                if let Some(v) = self.active_viewer() {
                    v.update(cx, |v, cx| v.toggle_inline_diff(cx));
                }
            }
            OpenFolder => self.pick_folder(cx),
            OpenRecent => self.open_recent(cx),
            NewBrowserTab => self.open_url_prompt(self.config.browser.app_mode, None, cx),
            NewBrowserTabSinglePage => self.open_url_prompt(true, None, cx),
            DockChromeWindow => self.open_dock_list(cx),
            DockChromeWindowAt(i) => {
                let Some(win) = self.dock_choices.get(i).cloned() else { return };
                if let Some(tab) = self.dock_target(cx) {
                    self.dock_window_into(tab, win, cx);
                }
            }
            UndockChromeWindow => {
                let p = self.tree.pane(focused);
                match p.and_then(|p| self.tree.tab_id(focused, p.active)).filter(|t| self.docks.docked_window(*t).is_some()) {
                    Some(tab) => self.undock_tab(tab, cx),
                    None => self.show_notice("the focused tab has no docked Chrome window".into(), cx),
                }
            }
        }
        let _ = window;
    }

    /// The native folder picker; the folder chosen opens in a new window.
    fn pick_folder(&mut self, cx: &mut Context<Self>) {
        if self.picking_folder {
            return;
        }
        self.picking_folder = true;
        let rx = cx.prompt_for_paths(launch::picker_options());
        cx.spawn(async move |root, cx| {
            let picked = rx.await;
            let _ = root.update(cx, |root, cx| {
                root.picking_folder = false;
                match picked {
                    Ok(Ok(Some(paths))) => {
                        if let Some(folder) = paths.into_iter().next() {
                            root.open_elsewhere(folder, cx);
                        }
                    }
                    Ok(Err(e)) => root.show_notice(format!("cannot show the folder picker: {e}"), cx),
                    _ => {}
                }
            });
        })
        .detach();
    }

    /// The chooser; the folder chosen opens in a new window. One at a time: asking again brings
    /// the open one forward.
    fn open_recent(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.chooser {
            if handle.update(cx, |_, window, _| window.activate_window()).is_ok() {
                return;
            }
            self.chooser = None;
        }
        if self.chooser_opening {
            return;
        }
        self.chooser_opening = true;
        // Weak: a chooser left open after this window closes keeps nothing of it alive.
        let weak = cx.entity().downgrade();
        let open = Box::new(move |folder: PathBuf, cx: &mut App| {
            let _ = weak.update(cx, |root, cx| root.open_elsewhere(folder, cx));
        });
        let opening = launch::open_chooser(cx, false, true, self.config.clone(), (*self.theme).clone(), None, open);
        cx.spawn(async move |root, cx| {
            let handle = opening.await;
            let _ = root.update(cx, |root, _| {
                root.chooser_opening = false;
                root.chooser = handle;
            });
        })
        .detach();
    }

    /// Opens `folder` in a new window process, unless it is this window's own project.
    pub fn open_elsewhere(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        if launch::same_folder(&folder, &self.project) {
            self.show_notice("already open here".into(), cx);
            return;
        }
        if let Err(e) = launch::spawn_window(&folder) {
            let text = format!("cannot open {}: {e}", folder.display());
            eprintln!("arkos: {text}");
            self.show_notice(text, cx);
        }
    }

    /// The plist for this install: the sibling daemon, the config's daemon arguments, the log.
    fn agent_plist(&self) -> String {
        launch_agent::plist(&paths::daemon_exe(), &daemon_args(&self.config), &client::daemon_log(&paths::socket_path()))
    }

    /// Installs or removes the login item off the UI thread and says what happened. Only the
    /// app in `/Applications` (or `~/Applications`) writes it: a copy run from anywhere else
    /// leaves the file as it is, and says so when the user asked (`asked`).
    fn set_start_at_login(&mut self, on: bool, asked: bool, cx: &mut Context<Self>) {
        if on && !launch_agent::may_install_here() {
            if asked {
                self.show_notice("Start at login needs the app in /Applications or ~/Applications".into(), cx);
            } else {
                eprintln!("arkos: not running from /Applications: the login item is left as it is");
            }
            return;
        }
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
                self.set_start_at_login(on, true, cx);
            }
            _ => self.open_config(cx),
        }
    }

    /// Keeps the login item as the config says; with "ask" and nothing installed, says once per
    /// launch how to turn it on.
    pub fn apply_recovery(&mut self, at_launch: bool, cx: &mut Context<Self>) {
        let installed = launch_agent::installed(&launch_agent::agent_path());
        match launch_agent::desired(self.config.recovery.start_at_login, installed) {
            Some(on) => self.set_start_at_login(on, false, cx),
            None if at_launch && !installed => {
                self.show_notice("sessions can come back after a restart: run \"Start at login\" from the command palette (Cmd+Shift+P)".into(), cx)
            }
            None => {}
        }
    }

    /// A click on the title bar's "update ready" badge: apply the staged release, else update from disk.
    pub fn update_clicked(&mut self, cx: &mut Context<Self>) {
        if self.staged.is_some() {
            self.apply_staged(cx);
        } else {
            self.update_now(cx);
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
                "arkosd is older than this window: run pkill -x arkosd (ends every session) and open Arkos IDE again"
            } else {
                "arkosd is not connected; try again once it is"
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

    /// Shows the staged release in the badge when there is one, else what the launch path holds.
    pub fn refresh_update_badge(&mut self, cx: &mut Context<Self>) {
        self.update_ready = self.staged.as_ref().map(update::status_text_for).or_else(|| self.update_ready_on_disk.clone());
        cx.notify();
    }

    /// What is open that an update must not pull out from under the user.
    fn chrome_state(&self, cx: &App) -> idle::Chrome {
        idle::Chrome {
            palette: self.palette.value().is_some(),
            menu: self.tab_menu.value().is_some() || self.context_menu.value().is_some() || self.wt_menu.value().is_some(),
            prompt: self.wt_prompt.value().is_some() || self.url_prompt.value().is_some(),
            find: self.active_terminal(cx).is_some_and(|t| t.read(cx).find.is_some()) || self.active_viewer().is_some_and(|v| v.read(cx).find.is_some()),
            drag: self.drop_target.is_some() || self.browser_drop.is_some() || self.resizing.is_some() || self.sidebar_resizing,
        }
    }

    /// Another window applied an update: the bundle at the launch path is not this build and the
    /// daemon runs another build than this window too (equal in a dev build, which never
    /// relaunches by itself).
    fn on_disk_newer_daemon(&self) -> bool {
        self.update_ready_on_disk.is_some() && self.daemon_version.as_deref().is_some_and(|d| update::other_build(d, &crate::version_string()))
    }

    /// Checks every second whether the window may act on an update (`idle::may_apply`): applies
    /// a staged release, or relaunches when another window applied one (`idle::idle_action`),
    /// once. Stops when there is nothing to do or `[update] auto` is off; one at a time.
    pub fn start_idle_timer(&mut self, cx: &mut Context<Self>) {
        if self.auto_apply_timer {
            return;
        }
        self.auto_apply_timer = true;
        cx.spawn(async move |root, cx| loop {
            cx.background_executor().timer(IDLE_TICK).await;
            let more = root.update(cx, |root, cx| {
                // A failed update waits for "Retry update" or a newly staged release.
                let action = idle::idle_action(root.staged.is_some() && root.update_error.is_none(), root.on_disk_newer_daemon());
                if action == idle::IdleAction::Nothing || !root.config.update.auto {
                    root.auto_apply_timer = false;
                    return false;
                }
                let now = Instant::now();
                let chrome = root.chrome_state(cx);
                // Anything open restarts the quiet spell, so closing it is not followed by an update.
                root.last_input = idle::quiet_since(root.last_input, now, chrome);
                let idle_after = Duration::from_secs(root.config.update.idle_after_secs);
                if idle::may_apply(now, root.last_input, idle_after, chrome, root.updating.is_some(), !root.sessions.is_offline()) {
                    root.auto_apply_timer = false;
                    match action {
                        idle::IdleAction::Apply => root.apply_staged(cx),
                        idle::IdleAction::Relaunch => root.relaunch_now(cx),
                        idle::IdleAction::Nothing => {}
                    }
                    return false;
                }
                true
            });
            if !matches!(more, Ok(true)) {
                break;
            }
        })
        .detach();
    }

    /// What the daemon's `UpdateStatus` says, into the badge; its `last_error` is shown once as a
    /// notice (again whenever the user asked for the check: `requested`).
    fn note_update_status(&mut self, staged: Option<StagedUpdate>, last_error: Option<String>, unreachable_for_secs: Option<u64>, requested: bool, cx: &mut Context<Self>) {
        // Replaced by every status: the next one after a successful check clears it.
        self.update_unreachable = update::unreachable_text(unreachable_for_secs);
        // A failed attempt is not retried by itself: "Retry update" does that.
        let apply = staged.is_some() && last_error.is_none();
        if let Some(e) = &last_error {
            if requested || self.update_error_noticed.as_ref() != Some(e) {
                self.update_error_noticed = Some(e.clone());
                self.show_notice(if e.starts_with("update") { e.clone() } else { format!("update check failed: {e}") }, cx);
            }
        }
        self.staged = staged;
        self.update_error = last_error;
        self.refresh_update_badge(cx);
        if apply {
            self.start_idle_timer(cx);
        }
    }

    /// Asks the daemon to swap in the staged bundle; `Outcome::Applied` then runs "Update now".
    pub fn apply_staged(&mut self, cx: &mut Context<Self>) {
        let Some(s) = &self.staged else { return };
        if self.updating.is_some() {
            return;
        }
        let version = s.version.clone();
        self.updating = Some(UpdateStage::Applying);
        self.show_notice(format!("updating to {version}; sessions stay"), cx);
        self.sessions.apply_update(version);
    }

    /// A batch of daemon events: the window's own (`UpdateStaged`) first, then the sessions'.
    pub fn route_events(&mut self, events: &[Event], cx: &mut Context<Self>) {
        for ev in events {
            if let Event::UpdateStaged { update } = ev {
                self.staged = Some(update.clone());
                self.update_error = None;
                self.refresh_update_badge(cx);
                self.start_idle_timer(cx);
            }
        }
        self.sessions.route_events(events, cx);
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
                self.show_notice(format!("cannot relaunch: {what} is gone; quit and open Arkos IDE again"), cx);
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
        self.unpark_windows();
        let killed = self.end_closed_tabs();
        // The process may exit before the writer thread sends what is queued; wait for it, once
        // and briefly. The client is the current connection's, whichever reconnect that is.
        if (killed > 0 || goodbye) && !self.sessions.client.as_ref().is_some_and(|c| c.wait_sent(Duration::from_millis(500))) {
            eprintln!("arkos: could not send {killed} kill requests{} before quitting", if goodbye { " and the goodbye" } else { "" });
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
            self.show_notice("update interrupted: arkosd disconnected before relaunching this window; try again".into(), cx);
        }
        // The apply went unanswered; the reconnect's `UpdateStatus` says what is staged and the
        // idle rule decides again. `connected` never relaunches for it.
        if self.updating == Some(UpdateStage::Applying) {
            self.updating = None;
            self.show_notice("update interrupted; checking again".into(), cx);
        }
        // An error the daemon sent before hanging up (a protocol mismatch) says why; keep it.
        self.status = if self.status.starts_with(ERROR_PREFIX) { format!("{}; {reason}", self.status) } else { reason };
        let is_final = connection::is_final(&self.status);
        if !is_final {
            eprintln!("arkos: {}; reconnecting", self.status);
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
            Outcome::DaemonHello { version, protocol_max, .. } => {
                self.daemon_version = Some(version);
                // An older daemon answers an unknown request with an error: ask only protocol 4.
                if protocol_max >= 4 {
                    self.sessions.update_status();
                }
                // Another window's update may have upgraded the daemon: relaunch once idle.
                self.start_idle_timer(cx);
            }
            Outcome::UpdateStatus { staged, last_error, unreachable_for_secs } => self.note_update_status(staged, last_error, unreachable_for_secs, false, cx),
            Outcome::CheckStarted => {
                // The check runs on the daemon's poll thread; its result is there a moment later.
                let root = cx.entity();
                cx.spawn(async move |_, cx| {
                    cx.background_executor().timer(CHECK_RESULT_AFTER).await;
                    let _ = root.update(cx, |root, _| root.sessions.status_after_check());
                })
                .detach();
            }
            Outcome::Checked { staged, last_error, unreachable_for_secs } => {
                let nothing = staged.is_none() && last_error.is_none() && unreachable_for_secs.is_none_or(|s| s == 0);
                self.note_update_status(staged, last_error, unreachable_for_secs, true, cx);
                if nothing {
                    self.show_notice("no update".into(), cx);
                }
            }
            Outcome::Applied => {
                // The bundle at the launch path is the new one: "Update now" upgrades the daemon
                // and relaunches the window from it.
                self.updating = None;
                self.staged = None;
                self.refresh_update_badge(cx);
                self.update_now(cx);
            }
            // Another window's apply holds the daemon: no error, no badge; this window relaunches
            // once that one has upgraded the daemon (`idle::IdleAction::Relaunch`).
            Outcome::ApplyFailed(e) if e.contains(ANOTHER_APPLY) => {
                eprintln!("arkos: apply refused: {e}");
                self.updating = None;
                self.show_notice("another window is applying the update".into(), cx);
            }
            Outcome::ApplyFailed(e) => {
                eprintln!("arkos: update failed: {e}");
                self.updating = None;
                // The daemon reports this failure as its `last_error` in these words: shown once.
                self.update_error_noticed = Some(format!("update failed: {e}"));
                self.update_error = Some(e.clone());
                self.refresh_update_badge(cx);
                self.show_notice(format!("update failed: {e}"), cx);
            }
            Outcome::CheckFailed(e) => self.show_notice(e, cx),
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
                        Reconcile::Create { tab, cwd } => self.start_shell(tab, cwd, self.reconcile_with_note),
                    }
                }
                cx.notify();
            }
            Outcome::Created { tab, session, note } => {
                let Some((pane, idx)) = self.tree.locate(tab) else {
                    // The tab closed while its shell was starting: nobody will show this session.
                    eprintln!("arkos: tab closed before session {} started, ending it", session.0);
                    self.sessions.kill(session);
                    self.sessions.detach(session);
                    return;
                };
                // The tab keeps the folder its shell was started in.
                let cwd = self.tab_cwd(tab);
                self.tree.replace_tab(pane, idx, Tab::Terminal { session, cwd });
                if note {
                    self.sessions.notes.insert(session, sessions::Note::Restarted);
                }
                self.save(cx);
                self.sessions.attach(session, tab, note);
                cx.notify();
            }
            Outcome::Attached { session, tab, replay, alive, note: _ } => {
                eprintln!("arkos: attached to session {} ({} replay bytes)", session.0, replay.len());
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
                self.restart_tab(tab, session, &format!("arkosd could not attach it: {error}"), cx);
            }
            Outcome::UpgradeFailed(e) => {
                // The old daemon keeps running and serving; nothing else changes.
                eprintln!("arkos: update failed: {e}");
                self.updating = None;
                self.show_notice(format!("update failed: {e}"), cx);
            }
            Outcome::RelaunchAccepted => {
                // No Goodbye: the daemon reopens this project after a rollback only while the
                // folder is still in its window list.
                self.flush_for_quit(false);
                cx.quit();
            }
            Outcome::RelaunchRefused(e) => {
                eprintln!("arkos: relaunch refused: {e}");
                self.updating = None;
                self.show_notice(format!("cannot relaunch: {e}"), cx);
            }
            Outcome::Error(e) => {
                self.status = format!("{ERROR_PREFIX}{e}");
                eprintln!("arkos: {}", self.status);
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
    eprintln!("arkos: session {} replay parsed at {cols}x{rows} in {:?}", session.0, started.elapsed());
    root.update(cx, |root, cx| {
        if root.connection_generation != generation {
            eprintln!("arkos: session {} replay was parsed for a connection that has ended; dropping it", session.0);
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
        let now = Instant::now();
        let last_frame = std::mem::replace(&mut self.frame_now, now);
        self.settle(now, last_frame);
        let theme = self.theme.clone();
        let status = if banner_visible(self.reconnecting_since, now) { self.status.clone() } else { String::new() };
        // A notice fades in and out; it keeps its row until it has faded.
        let notice = self.notice.current(now).map_or((String::new(), 1.0), |(text, o)| (text.clone(), o));
        let banners: Vec<(String, f32)> = [
            (status, 1.0),
            (self.config_error.clone().unwrap_or_default(), 1.0),
            (self.theme_error.clone().unwrap_or_default(), 1.0),
            notice,
        ]
            .into_iter()
            .filter(|(b, _)| !b.is_empty())
            .collect();
        let banner_h = BANNER_H * banners.len() as f32;
        let top = chrome::TITLE_BAR_H;
        // The panes start right of the sidebar. A window shorter than its chrome would give a
        // negative height; lay out at zero instead.
        // While Cmd+B's slide runs, the panes follow the width on screen frame by frame (each
        // frame relays them out, as dragging the edge does; no terminal is made again).
        let side = self.sidebar.shown_width(now);
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
        let side_col = sidebar::render(self, window, cx);
        // Focus follows the tree: the focused pane's terminal or viewer (its find bar while that
        // is open), or the pane's own handle. The palette holds it while it is open; then the
        // URL prompt (over the focused pane, until Enter or Escape), the worktree prompt, the worktree list (Ctrl+Shift+G) and the file tree (Shift+Cmd+E)
        // while they hold the keys. An open prompt holds them only while the list does, so a
        // click on a pane takes them back without losing what was typed.
        let wanted = if let Some(f) = self.palette.value().and_then(|p| p.focus.clone()) {
            Some(f)
        } else if self.url_prompt.value().is_some() {
            Some(self.url_prompt_focus.clone())
        } else if self.wt_focused && self.wt_prompt.value().is_some() {
            Some(self.wt_prompt_focus.clone())
        } else if self.wt_focused {
            Some(self.wt_focus.clone())
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
        // A failed update says so until a release is staged again or "Retry update" runs.
        let update_text = match (&self.staged, &self.update_error, &self.update_unreachable) {
            (None, Some(_), _) => Some("update failed"),
            // Nothing staged and nothing failed, but the daemon has not reached GitHub for a day.
            (None, None, Some(u)) => Some(u.as_str()),
            _ => self.update_ready.as_deref(),
        };
        let items = chrome::status_items(update_text, (daemon != CONNECTED_LABEL).then_some(daemon), self.sessions.waiting_count(&self.tree));
        let title_bar = chrome::title_bar(&self.project, items, window.is_fullscreen(), &theme, Some(Box::new(cx.listener(|root, _, _, cx| root.update_clicked(cx)))));
        let family = self.family();
        let root_div = div()
            .relative()
            .size_full()
            .key_context("Root")
            .bg(theme.chrome.window_bg.hsla())
            .flex()
            .flex_col()
            .on_drag_move(cx.listener(|root, ev: &DragMoveEvent<drag::TabDrag>, _window, cx| {
                // Capture listeners run parent first, so this forgets the last target before
                // the bar, tab or pane under the pointer claims it; over none of them, none.
                root.drop_target = None;
                // A browser tab released outside the window releases its Chrome window there
                // (the dock tick sees the release: no mouse-up reaches us outside).
                let tab = ev.drag(cx).tab;
                let browser = root.tree.locate(tab).and_then(|(p, i)| root.tree.pane(p).and_then(|p| p.tabs.get(i))).is_some_and(|t| matches!(t, Tab::Browser { .. }));
                root.tab_drag_browser = browser.then_some(tab);
                cx.notify();
            }))
            .on_mouse_move(cx.listener(|root, ev: &MouseMoveEvent, _window, cx| {
                root.last_input = Instant::now();
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|root, _ev: &MouseDownEvent, _window, cx| {
                    root.close_tab_menu(cx);
                    // A press outside the URL prompt (it stops its own) cancels it, like a menu.
                    if root.url_prompt.hide(Instant::now()) {
                        cx.notify();
                    }
                }),
            )
            // A right-click menu (the tree's or a worktree's) closes on any press outside it
            // (its rows stop theirs), and on Escape wherever the keys are.
            .on_any_mouse_down(cx.listener(|root, _ev: &MouseDownEvent, _window, cx| {
                let now = Instant::now();
                root.last_input = now;
                if root.context_menu.hide(now) | root.wt_menu.hide(now) {
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
                    root.tab_drag_browser = None;
                    if root.drop_target.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(title_bar)
            .children(banners.into_iter().map(|(text, opacity)| banner_bar(text, opacity, family.clone(), &theme)))
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
            .children(sidebar::render_context_menu(self, cx))
            .children(sidebar::render_worktree_menu(self, cx));
        // One request per frame while anything eases; an idle window asks for none.
        if self.any_motion(now) {
            window.request_animation_frame();
        }
        keymap::attach(root_div, cx)
    }
}

/// The menu bar. Set again when the key bindings change so it shows the bound keys.
fn menus() -> Vec<Menu> {
    vec![
        Menu { name: "Arkos IDE".into(), items: vec![MenuItem::action("Quit", Quit)] },
        Menu {
            name: "File".into(),
            items: vec![MenuItem::action("Open Folder\u{2026}", keymap::OpenFolder), MenuItem::action("Open Recent\u{2026}", keymap::OpenRecent)],
        },
    ]
}

fn banner_bar(text: String, opacity: f32, family: gpui::SharedString, theme: &theme::Theme) -> Div {
    div()
        .opacity(opacity)
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
    format!("{} {} ({})", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"), env!("ARKOS_GIT_HASH"))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // The hook CLI: no window, no daemon start, no QoS. `claude` runs these on every event.
    if matches!(args.get(1).map(String::as_str), Some("notify") | Some("hooks")) {
        std::process::exit(hook_cli::run(&args[1..]));
    }
    // Read-only diagnostics for a failed update: no window, no daemon start, nothing written.
    if args.get(1).map(String::as_str) == Some("doctor") {
        std::process::exit(doctor::run());
    }
    // Spec 8: a one-off check of the Accessibility calls the browser pane makes, run by hand.
    if args.get(1).map(String::as_str) == Some("browser-probe") {
        std::process::exit(ax::probe::run());
    }
    if args.iter().any(|a| a == "--version") {
        println!("{}", version_string());
        return;
    }
    // Spec 2.5: the UI thread is user-interactive so macOS schedules it ahead of the heavy work
    // Claude Code's children do.
    let qos = unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0) };
    if qos != 0 {
        eprintln!("arkos: could not set user-interactive QoS (error {qos})");
    }
    motion::refresh();
    app_nap::hold();
    let config_path = paths::config_file();
    let (config, config_error) = match config::load(&config_path) {
        Ok(c) => (c, None),
        Err(e) => {
            eprintln!("arkos: {e}");
            (config::Config::default(), Some(e.to_string()))
        }
    };
    // Used only when this window starts the daemon; a running one keeps its own settings.
    let daemon_args = daemon_args(&config);
    let cwd = std::env::current_dir().expect("cwd");
    let relaunched = !should_activate(std::env::var("ARKOS_RELAUNCH").ok().as_deref());
    let plan = launch::startup_plan(&cwd, relaunched);
    let yes = |b: bool| if b { "yes" } else { "no" };
    eprintln!("{} starting in {} (relaunch: {}, chooser: {})", version_string(), cwd.display(), yes(relaunched), yes(plan.is_chooser()));
    // Connected before any window opens, the chooser included: a window the daemon relaunched
    // after an update confirms it with this Hello. A Dock launch has no folder yet, so its Hello
    // names none (the daemon keeps what it had), and a second Hello names the folder once one is
    // chosen. The chooser heartbeats meanwhile, so the watchdog leaves it alone.
    debug_assert!(plan.connects_first());
    let (client, events) = connect(&daemon_args);
    let client = Arc::new(client);
    let save_queue = SharedSaveQueue::default();

    Application::new().run(move |cx: &mut App| {
        // gpui installs no menu and no quit binding of its own; the Quit action is handled below,
        // once the window exists.
        cx.bind_keys(keymap::bindings_for(&config.keybindings));
        cx.set_menus(menus());
        let system_is_dark = theme::is_dark(cx.window_appearance());
        let name = theme::pick_name(config.appearance.mode, system_is_dark, &config.theme).to_string();
        let (theme, theme_error) = match theme::load(&name, &paths::themes_dir()) {
            Ok(t) => (t, None),
            Err(e) => {
                // A broken user navy-light.toml still gives a light window.
                let t = theme::builtin(&name).or_else(|| theme::builtin("navy")).expect("navy is built in");
                let banner = format!("theme: {e}; using built-in {}", t.name);
                eprintln!("arkos: {banner}");
                (t, Some(banner))
            }
        };
        let start = Startup { config, config_error, config_path, daemon_args, save_queue, theme, theme_error, system_is_dark };
        if plan.is_chooser() {
            // Until a folder is chosen, Quit and closing the chooser end the app; once the
            // project window is open, its own handlers below take over. Neither says Goodbye:
            // the chooser named no folder, so the daemon forgets none.
            let opened = std::rc::Rc::new(std::cell::Cell::new(false));
            // The chooser's heartbeat, until the project window's connection runs its own.
            let waiting = opened.clone();
            let beat_client = client.clone();
            cx.spawn(async move |cx| connection::heartbeat(beat_client, move |_| !waiting.get(), cx).await).detach();
            // A quit right after "Stop all" still delivers the kills: the writer gets a moment.
            let quit_opened = opened.clone();
            let quit_client = client.clone();
            cx.on_action(move |_: &Quit, cx| {
                if !quit_opened.get() {
                    quit_client.wait_sent(Duration::from_millis(500));
                    cx.quit();
                }
            });
            let closed_opened = opened.clone();
            let closed_client = client.clone();
            cx.on_window_closed(move |cx| {
                if !closed_opened.get() && cx.windows().is_empty() {
                    closed_client.wait_sent(Duration::from_millis(500));
                    cx.quit();
                }
            })
            .detach();
            let (config, theme) = (start.config.clone(), start.theme.clone());
            // The chooser's line to the daemon: the session counts and "Stop all". It holds the
            // connection's receiver until a folder is chosen, then the project window gets it.
            let link = launch_link::Link::new(client.clone(), events);
            let link_for_choice = link.clone();
            launch::open_chooser(
                cx,
                true,
                plan.focus(),
                config,
                theme,
                Some(link),
                Box::new(move |folder, cx| {
                    // The project is the working directory, as for a terminal launch: the
                    // daemon's Hello, its relaunches and the shells all start from it.
                    if let Err(e) = std::env::set_current_dir(&folder) {
                        eprintln!("arkos: cannot open {}: {e}", folder.display());
                        cx.quit();
                        return;
                    }
                    let project = std::env::current_dir().unwrap_or(folder);
                    opened.set(true);
                    // The same connection goes on: its second Hello names the project.
                    client.hello();
                    let finishing = link_for_choice.finish();
                    cx.spawn(async move |cx| {
                        let (backlog, events) = finishing.await;
                        let _ = cx.update(|cx| open_project_window(project, (client, events, backlog), start, cx));
                    })
                    .detach();
                }),
            )
            .detach();
            return;
        }
        open_project_window(cwd, (client, events, Vec::new()), start, cx);
    });
}

/// What `main` read before the app ran, for the project window.
struct Startup {
    config: config::Config,
    config_error: Option<String>,
    config_path: PathBuf,
    daemon_args: Vec<String>,
    save_queue: SharedSaveQueue,
    theme: theme::Theme,
    theme_error: Option<String>,
    system_is_dark: bool,
}

/// Connects to the daemon, starting it when none answers. Without one there is nothing to show.
fn connect(daemon_args: &[String]) -> (client::Client, futures::channel::mpsc::UnboundedReceiver<protocol::ServerMessage>) {
    match client::Client::connect_or_start(&paths::socket_path(), &paths::daemon_exe(), daemon_args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("arkos: cannot reach arkosd: {e:#}");
            std::process::exit(1);
        }
    }
}

/// Opens the window on `project` and starts everything that serves it.
fn open_project_window(
    project: PathBuf,
    // The connection, and what the launch chooser read from it before the window existed.
    (client, events, backlog): (Arc<client::Client>, futures::channel::mpsc::UnboundedReceiver<protocol::ServerMessage>, Vec<protocol::ServerMessage>),
    start: Startup,
    cx: &mut App,
) {
    let Startup { config, config_error, config_path, daemon_args, save_queue, theme, theme_error, system_is_dark } = start;
    // Synchronous on purpose: one small file read, done before the window opens, so it cannot
    // stall painting or input. Saves happen on the background executor.
    let saved = workspace::load(&project);
    // The chooser lists this folder next time.
    let recent_project = project.clone();
    cx.background_executor()
        .spawn(async move {
            if let Err(e) = recent::note(&paths::recent_file(), &recent_project, recent::now_unix()) {
                eprintln!("arkos: cannot note recent folder: {e}");
            }
        })
        .detach();
    {
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let client_for_view = client.clone();
        let view_queue = save_queue.clone();
        let view_project = project.clone();
        let view_saved = saved.clone();
        let (view_config, view_theme) = (config, theme);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    // The window draws its own bar in the title bar's space (chrome.rs); the
                    // traffic lights stay where macOS puts them.
                    titlebar: Some(gpui::TitlebarOptions { title: Some(ax::screen::WINDOW_TITLE.into()), appears_transparent: true, traffic_light_position: None }),
                    // One minimum pane with its border, tab bar and the window's bar.
                    window_min_size: Some(size(px(tree::MIN_PANE_W + 8.0), px(tree::MIN_PANE_H + pane_view::TAB_BAR_H + chrome::TITLE_BAR_H))),
                    // A window the daemon relaunched (an update, the watchdog) does not take the
                    // keyboard from the app in use.
                    focus: should_activate(std::env::var("ARKOS_RELAUNCH").ok().as_deref()),
                    ..Default::default()
                },
                move |window, cx| {
                    let view = cx.new(|cx| {
                        // Activating arkos covers the docked windows: the tick re-raises them at
                        // once (spec 8's accepted flash). Deactivating is ticked too, so the
                        // next activation is seen as one.
                        cx.observe_window_activation(window, |root: &mut Root, window, cx| {
                            root.window_active = window.is_window_active();
                            root.dock_tick(cx);
                        })
                        .detach();
                        let mut root = Root::new(client_for_view, view_project, view_saved, view_config, view_theme, system_is_dark, cx);
                        root.ns_window = ax::screen::ns_window_of(window);
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
            root.refresh_status(cx);
            root.refresh_worktrees(cx);
            // A restored workspace's browser tabs wait for windows; no prompt until one is made.
            root.sync_docks(cx);
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
        let project_window = window.window_id();
        cx.on_window_closed(move |cx| {
            let left = cx.windows().len();
            let project_open = cx.windows().iter().any(|w| w.window_id() == project_window);
            let chooser = close_root.read(cx).chooser.filter(|h| cx.windows().iter().any(|w| w.window_id() == h.window_id()));
            match after_window_closed(left, project_open, chooser.is_some()) {
                AfterClose::Quit => {
                    close_root.update(cx, |root, _| if says_goodbye(root.user_closing, left) { root.leaving() } else { root.flush_for_quit(false) });
                    cx.quit();
                }
                // Its own close comes back here with no window left, and quits as above.
                AfterClose::CloseChooser => {
                    if let Some(h) = chooser {
                        let _ = h.update(cx, |_, window, _| window.remove_window());
                    }
                }
                AfterClose::Stay => {}
            }
        })
        .detach();
        config_watch::start(root.clone(), config_path.clone(), cx);
        explorer_watch::start(root.clone(), cx);
        // The same client the root was built with; reconnects replace it from here on.
        connection::start(root.clone(), paths::socket_path(), paths::daemon_exe(), daemon_args.clone(), (client.clone(), events, backlog), cx);
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
    }
}

#[cfg(test)]
mod tests {
    use super::{after_window_closed, daemon_label, pick_grid, says_goodbye, AfterClose, shell_name, should_activate, CONNECTING, ERROR_PREFIX};

    #[test]
    fn a_window_the_daemon_relaunched_does_not_take_focus() {
        assert!(should_activate(None), "opened by the user");
        assert!(should_activate(Some("")));
        assert!(!should_activate(Some("1")), "ARKOS_RELAUNCH from watchdog::launch_ui");
    }

    #[test]
    fn launch_closing_the_project_window_closes_its_chooser_then_quits() {
        assert_eq!(after_window_closed(0, false, false), AfterClose::Quit);
        assert_eq!(after_window_closed(1, false, true), AfterClose::CloseChooser, "only the chooser is left");
        assert_eq!(after_window_closed(1, true, false), AfterClose::Stay, "the chooser closed; the project stays");
        assert_eq!(after_window_closed(2, true, true), AfterClose::Stay);
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
        assert_eq!(daemon_label(""), "arkosd connected");
        assert_eq!(daemon_label(CONNECTING), "arkosd connecting");
        assert_eq!(daemon_label(&format!("{ERROR_PREFIX}no such session")), "arkosd: see banner");
        assert_eq!(daemon_label("daemon connection ended"), "arkosd: see banner");
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
        assert_eq!(daemon_label_with("daemon closed the connection", true), "arkosd reconnecting");
        assert_eq!(daemon_label_with("", false), "arkosd connected");
    }

    #[test]
    fn the_focused_session_is_the_focused_panes_active_terminal_tab() {
        use super::{focused_session_of, Tab, Tree};
        use protocol::SessionId;
        let mut tree = Tree::new(Tab::Terminal { session: SessionId(5), cwd: None });
        let root = tree.panes()[0];
        assert_eq!(focused_session_of(&tree), Some(SessionId(5)));
        tree.add_tab(root, Tab::Viewer { path: "/f".into() });
        tree.activate(root, 1);
        assert_eq!(focused_session_of(&tree), None);
    }

    #[test]
    fn a_shell_starts_in_its_worktree_while_the_folder_exists() {
        use super::shell_folder;
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(shell_folder(Some(dir.path().to_path_buf())), Some(dir.path().to_path_buf()));
        assert_eq!(shell_folder(Some(dir.path().join("gone"))), None, "a removed worktree: the project");
        assert_eq!(shell_folder(None), None);
    }

    #[test]
    fn a_refused_window_says_so_on_its_tabs() {
        use super::{connection::is_final, disconnect_mark};
        let refused = format!("{ERROR_PREFIX}protocol version mismatch: daemon speaks 1 to 2, client 3");
        assert_eq!(disconnect_mark(is_final(&refused)), "daemon refused");
        assert_eq!(disconnect_mark(is_final("daemon closed the connection")), "reconnecting");
    }
}
