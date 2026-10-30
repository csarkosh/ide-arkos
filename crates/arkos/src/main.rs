mod about;
mod agent_cli;
mod app_nap;
mod aux;
mod av_player;
mod cache;
mod chooser_sessions;
mod chrome;
mod claude_theme;
mod client;
mod closed_stack;
mod command_palette;
mod config;
mod config_watch;
mod connection;
mod crash_dialog;
mod diff;
mod direct;
mod doctor;
mod doctor_panel;
mod drag;
mod edit;
mod effects;
mod empty_pane;
mod explorer;
mod explorer_watch;
mod find;
mod gitstatus;
mod guide;
mod highlight;
mod hook_cli;
mod idle;
mod keybindings;
mod keys;
mod keymap;
mod launch;
mod launch_link;
mod link_menu;
mod md_menu;
mod slash_menu;
mod links;
mod launch_agent;
mod layout;
mod ledger;
mod line_edit;
mod md_blocks;
mod md_format;
mod md_reveal;
mod md_slash;
mod md_table;
mod md_view;
mod media;
mod mermaid;
mod motion;
mod mouse;
mod notices;
mod open_cli;
mod palette;
mod pane_view;
mod paths;
mod pulse;
mod recent;
mod reflow;
mod remote_image;
mod sandbox;
mod screen;
mod scroll;
mod settings;
mod split_view;
mod startup;
mod sessions;
mod sidebar;
mod telemetry;
mod telemetry_cli;
mod terminal;
mod terminal_element;
mod theme;
mod tree;
mod update;
mod update_check;
mod viewer;
mod video;
mod video_element;
mod video_view;
mod viewer_element;
mod web;
mod web_element;
mod web_view;
mod wk_web_view;
mod workspace;
mod worktrees;

use futures::channel::oneshot;
use gpui::{
    div, point, prelude::*, px, size, App, Application, AsyncApp, Bounds, Context, Div, DragMoveEvent, Entity, FocusHandle,
    KeyDownEvent, Menu, MenuItem, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Task, Window, WindowBounds, WindowHandle, WindowOptions,
};
use protocol::{CustomizeAction, CustomizeOutcome, Event, Request, SessionId, StagedUpdate};
use sessions::{reconcile, Outcome, Reconcile, SessionManager};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
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

/// `daemon_label`, except that while the window is reconnecting the bell says so at once.
fn daemon_label_with(status: &str, reconnecting: bool) -> &'static str {
    if reconnecting {
        "arkosd reconnecting"
    } else {
        daemon_label(status)
    }
}

/// Whether the window is connected to the daemon: no connection status standing and no reconnect
/// under way, the state `daemon_label_with` calls "arkosd connected".
fn daemon_connected(status: &str, reconnecting: bool) -> bool {
    status.is_empty() && !reconnecting
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

/// A theme switch half way: the window shows `theme::blend(from, to, p)` with `p` the
/// `motion::fade_progress` since `since`. `from` is whatever was on screen when the switch came,
/// a blend itself when a switch lands mid-fade, so nothing jumps.
pub struct ThemeFade {
    pub from: Arc<theme::Theme>,
    pub to: Arc<theme::Theme>,
    pub since: Instant,
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
    /// The newest protocol the daemon said it speaks (0 before its `Hello` answer).
    pub daemon_protocol_max: u32,
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
    /// Each agent session's preview pane (spec 5): where its plain `arkos open` goes while the
    /// pane lives. `None` is the key of an open sent without a session. Not saved: after a
    /// relaunch the first open splits again.
    pub preview_panes: HashMap<Option<SessionId>, PaneId>,
    /// The file tree's right-click menu while it is open, and while it fades out.
    pub context_menu: Transient<sidebar::ContextMenu>,
    /// A terminal link's right-click menu while it is open, and while it fades out.
    pub link_menu: Transient<link_menu::LinkMenu>,
    /// A markdown document's right-click menu while it is open, and while it fades out.
    pub md_menu: Transient<md_menu::MdMenu>,
    /// What the bell shows.
    pub notices: notices::Notices,
    /// The doctor's table as last probed.
    pub doctor_states: Vec<doctor_panel::Check>,
    /// The doctor row whose action is running off the UI thread; a second click waits for it.
    pub doctor_busy: Option<doctor_panel::CheckId>,
    /// True once the doctor turned the scroll-acceleration key off during this run.
    pub scroll_applied: bool,
    /// This Arkos is a sandbox (`ARKOS_SANDBOX`, read once at startup): it never writes the
    /// user's Claude Code settings, colours, hooks or skill, never writes the login item, never
    /// updates.
    pub sandbox: bool,
    /// When the bell was last opened before the opening now; notices newer than this are unread.
    pub bell_read_before: Option<SystemTime>,
    /// The title bar popover that is open (or fading out).
    pub popover: Transient<chrome::Popover>,
    /// Which title bar icon the pointer is over (`Closed`: none).
    pub icon_hover: chrome::Popover,
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
    /// The split handles under the pointer, and each handle's stripe while it fades in or out.
    pub handle_hovered: HashSet<SplitId>,
    pub handle_fades: motion::Fades<SplitId, ()>,
    /// The find bars as drawn, by terminal or viewer entity: the open bar, and a closed one
    /// while it fades out.
    pub find_bars: motion::Fades<gpui::EntityId, find::FindLook>,
    /// Each empty pane's animation clock, from when it started; it holds while the window's
    /// effects are paused (`effects_paused`).
    pub empty_anim: HashMap<PaneId, effects::active::Clock>,
    /// The basename of `$SHELL`, shown on a tab whose program set no title.
    pub shell_name: String,
    /// The last good config; `apply_config` replaces it.
    pub config: config::Config,
    /// The theme every view reads at paint. For `motion::THEME_FADE` after a switch it is a
    /// `theme::blend` of the old and the new, remade each frame by `step_theme_fade`, so no view
    /// may cache a colour; `theme_fade` holds the ends of that fade while it runs.
    pub theme: Arc<theme::Theme>,
    /// The theme cross-fade under way, if any: `theme` is between `from` and `to`.
    pub theme_fade: Option<ThemeFade>,
    /// The theme the config and the appearance resolve to. The window shows it as it is, or
    /// with its text deepened by the effect's `text_contrast` while an effect is settled on.
    base_theme: Arc<theme::Theme>,
    /// Whether a frame has been drawn: a theme set before the first one shows at once.
    pub painted: bool,
    /// The theme's fonts, less any family this Mac lacks (`ThemeFont::installed`).
    pub fonts: theme::ThemeFont,
    /// Syntax colours for viewer tabs in the theme's `viewer` colours. `None` until the grammars
    /// finish loading on the background executor after launch; a theme change rethemes it.
    pub highlighter: Option<Arc<highlight::Highlighter>>,
    /// Each viewer tab's file, loaded when the tab is first shown; dropped with the tab.
    pub viewers: HashMap<TabId, Entity<Viewer>>,
    /// Each video tab's player view, made when the tab is first shown; dropped with the tab,
    /// which releases its player at once.
    pub videos: HashMap<TabId, Entity<video_view::VideoView>>,
    /// Each web tab's view, made the first time the tab is shown (web pane spec, section 3.2).
    pub webs: HashMap<TabId, Entity<web_view::WebView>>,
    /// Each web tab's title as its view last reported it, for the tab label.
    pub web_titles: HashMap<TabId, String>,
    /// gpui's own `NSView` in the project window, which takes the keys back from a web view.
    pub ns_view: Option<objc2::rc::Retained<objc2_app_kit::NSView>>,
    /// The repaint scheduled for a text field's cursor to blink (`line_edit::take_blink_wake`).
    caret_wake: Option<Instant>,
    /// The key monitor (`screen::install_key_monitor`), alive while the window is.
    key_monitor: Option<objc2::rc::Retained<objc2::runtime::AnyObject>>,
    /// The effect under the window (effects spec 4.3), on, or on its way in or out.
    effect: Option<effects::active::ActiveEffect>,
    /// The glyph shader the project window is to draw with, set at its next frame (`render`
    /// holds the window); `None` when nothing has changed.
    glyphs_pending: Option<Option<gpui::GlyphShader>>,
    /// The window's pixels per point as the glyph shader was last built for, so a move to a
    /// screen of another scale builds it again.
    glyph_scale: f32,
    /// The wash's timer (`ensure_effect_timer`): `None` while it is stopped, else whether it
    /// rests (waits `effects::active::REST` because the wash could not be seen). It ends once
    /// `effect` is `None`, and under Reduce Motion or while `effects_paused` once nothing moves.
    effect_timer: Option<bool>,
    /// Counts the timers started, so a timer started afresh ends the one it replaces.
    effect_timer_gen: u64,
    /// Whether the text's timer (`ensure_glyph_timer`) runs. It stops itself when
    /// `effects::active::glyph_wait` says so, and `render` starts it again.
    glyph_timer: bool,
    /// Counts the text's timers started, so a timer started afresh ends the one it replaces.
    glyph_timer_gen: u64,
    /// Whether the window's background is transparent, as `render` last set it: while an effect
    /// is on, so the wash shows through.
    transparent: bool,
    /// Where the sidebar's Worktrees section starts, in window points, as last laid out: the
    /// divider the wash draws between it and the Explorer.
    pub effect_divider: Option<f32>,
    /// The highlights laid out this frame (spec 4.6): cleared at the start of each render and
    /// pushed by the Explorer rows' and the tabs' canvases while the effect paints highlights.
    pub effect_marks: Vec<effects::blocks::Mark>,
    /// The shapes inside the panes laid out this frame (a video's picture, a markdown table and
    /// its header): cleared at the start of each render and pushed as they are laid out, so the
    /// wash paints and outlines each.
    pub effect_shapes: Vec<effects::blocks::Shape>,
    /// The open menus and popovers as they reported themselves this frame, for the effect's
    /// menu stage (`chrome::menu_report`).
    pub effect_menus: Vec<effects::blocks::MenuShape>,
    /// The highlight strokes with their fades as the wash last drew them, for the menus'
    /// backdrops.
    pub effect_strokes_now: Vec<(effects::blocks::Mark, f32)>,
    /// The Explorer row or tab under the pointer, by its mark's key, set by their `on_hover`.
    pub effect_hover: Option<u64>,
    /// The highlights' fades, fed `effect_marks` each frame the wash draws.
    pub effect_strokes: effects::strokes::Strokes,
    /// What macOS reports; `appearance.mode = "system"` follows it.
    pub system_is_dark: bool,
    /// When this install first ran (`startup::installed_since`, read once in `main()`).
    pub installed: SystemTime,
    /// The invitation to turn usage reports on (telemetry spec, section 3.3): whether its strip
    /// shows, with the mode of the last config change and `asked` as last read, so no frame
    /// reads the folder. It never shows on the install day or in a source build, and not again
    /// once answered, here or in another window.
    pub invitation: telemetry::Invitation,
    /// The pane the invitation hangs under the tab bar of this frame: the first pane drawn, while
    /// the invitation is due.
    pub invite_pane: Option<PaneId>,
    /// The config file's problem shown under the connection banner; `None` when all is well.
    /// Kept apart from `status`, which belongs to the connection and clears when it answers.
    pub config_error: Option<String>,
    /// The theme that did not load, shown under `config_error`. Set and cleared by whatever last
    /// tried to switch theme, so a config reload and a theme problem never clobber each other.
    pub theme_error: Option<String>,
    /// The name of the theme an agent is previewing (`CustomizeAction::PreviewTheme`), shown
    /// in place of the configured one until the next config reload, `EndPreview` or macOS
    /// appearance change. The config is untouched.
    pub preview: Option<String>,
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
    /// The poll behind a check the user asked for; None when none runs.
    pub check_poll: Option<update_check::Poll>,
    /// The daemon's `last_error` last shown as a notice, so each one is shown once.
    pub update_error_noticed: Option<String>,
    /// The native folder picker is up: Cmd+O again does not open a second one.
    pub picking_folder: bool,
    /// The chooser "New Window" opened, while it may still be open: asking again brings it
    /// forward, and closing this window closes it.
    pub chooser: Option<gpui::WindowHandle<launch::Chooser>>,
    /// "New Window" is reading the list before its window exists.
    pub chooser_opening: bool,
    /// The last key, pointer move or click in this window: a staged update applies only once the
    /// window has been quiet for `[update] idle_after_secs`.
    pub last_input: Instant,
    /// The update "Update now" started, while it runs.
    pub updating: Option<UpdateStage>,
    /// Each browser tab's placeholder as last laid out, in window pixels, and the frame that drew
    /// it: where its web view goes. Filled by the tab's content `canvas` every frame.
    pub browser_rects: HashMap<TabId, (Bounds<gpui::Pixels>, Instant)>,
    /// The project window's `NSWindow`, from gpui's window handle at setup: where the
    /// placeholders are on screen and whether the window is minimized or hidden.
    pub ns_window: Option<objc2::rc::Retained<objc2_app_kit::NSWindow>>,
    /// Where the window is (display, frame), as last saved with the layout; a relaunch opens
    /// it there.
    pub window_saved: Option<workspace::WindowSaved>,
    /// The states of the `<details>` sections opened or closed in each markdown file, as saved
    /// with the layout: a viewer takes its file's when it is made, and a toggle in any of them
    /// updates it.
    pub details_open: BTreeMap<PathBuf, Vec<workspace::DetailsOpen>>,
    /// A tab dragged within its own bar: the slot and gap that slide (`pane_view::Reorder`).
    pub reorder: Option<pane_view::Reorder>,
    /// Each tab's painted width, from the last frame, for the gap a reorder opens.
    pub tab_widths: HashMap<TabId, f32>,
    /// The URL prompt ("New browser tab") while it is open, and while it fades out. While open it
    /// holds the keys, through `url_prompt_focus`.
    pub url_prompt: Transient<pane_view::UrlPrompt>,
    pub url_prompt_focus: FocusHandle,
    /// A tab being renamed in place (double-click on its title); its editor holds the keys.
    pub tab_rename: Option<pane_view::TabRename>,
    pub tab_rename_focus: FocusHandle,
    /// Whether this is the key window, read each frame; the pulse ticks only while it is.
    pub window_active: bool,
    /// Whether the window's effects and other decorative motion are paused, holding their last
    /// frame: "Use more battery to run effects while using other apps" is off and neither this
    /// window nor Settings is the key window (`sync_pause`). Reduce Motion is separate and wins.
    pub effects_paused: bool,
    /// The breath of the working glyphs, the focus glow and the icon dots (`pulse::Breath`):
    /// at rest unless something shown is working.
    pub breath: pulse::Breath,
    /// When this frame is drawn: every transition in one render reads the same instant, and
    /// `any_motion` asks for the next frame with it.
    pub frame_now: Instant,
    /// When the user last scrolled a document (the wheel, or a key that eases the view): the
    /// glyph clock rests meanwhile (`effects::active::glyph_clock`), as it does for each
    /// terminal's own `Terminal::last_scroll`. None before the first.
    pub last_scroll: Option<Instant>,
    /// What the focus clock was last told the window shows (`note_focus`).
    pub focus_surface: Option<telemetry::Surface>,
    /// The user is closing the window: the Quit action, or the red button
    /// (`windowShouldClose:`). Never set by macOS's own terminate or the update relaunch.
    pub user_closing: bool,
    /// The Goodbye went out: the Quit action sends it, and the close its quit makes must not
    /// send a second one.
    said_goodbye: bool,
    /// A quit without a Goodbye said how the session ended (`SessionQuit`): macOS's terminate
    /// runs the flush twice, and the second need not say it again.
    said_quit: bool,
    /// When a quit or a window close last stopped because a markdown tab's edits could not be
    /// saved: another within `FORCE_QUIT_WITHIN` goes ahead without them (`quit_forced`).
    pub last_quit_attempt: Option<Instant>,
}

/// The folder a worktree tab's fresh shell starts in: its worktree while that is a directory,
/// else `None`, the project.
fn shell_folder(cwd: Option<PathBuf>) -> Option<PathBuf> {
    cwd.filter(|d| d.is_dir())
}

/// How often the idle timer asks whether a staged update may apply itself.
const IDLE_TICK: Duration = Duration::from_secs(1);
/// How long after the daemon accepted "Check for updates" the window asks what it found.
const CHECK_RESULT_AFTER: Duration = Duration::from_secs(3);
/// What the daemon's refusal of a second `ApplyUpdate` says (`server.rs`): another window's
/// apply is in flight or waits for its new window.
const ANOTHER_APPLY: &str = "waiting for a window";

/// What arkosd starts with when this window (or the login item) starts it; a running daemon
/// keeps its own settings. A sandbox's daemon never checks for updates (`--check-every-secs 0`).
fn daemon_args(config: &config::Config, sandbox: bool) -> Vec<String> {
    let check_every_secs = if sandbox { 0 } else { config.update.check_every_secs };
    vec![
        "--watchdog-secs".to_string(),
        config.daemon.watchdog_secs.to_string(),
        "--replay-bytes".to_string(),
        config.daemon.replay_bytes.to_string(),
        "--check-every-secs".to_string(),
        check_every_secs.to_string(),
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
    /// Close the "New Window" chooser and the About, Settings and Keybindings windows; the quit
    /// follows.
    CloseAux,
    Stay,
}

/// How soon a second Cmd+Q (or close) after one stopped by unsaved edits quits without them.
const FORCE_QUIT_WITHIN: Duration = Duration::from_secs(10);

/// Whether a quit goes ahead although a markdown tab's edits cannot be saved: the user asked once
/// already, `FORCE_QUIT_WITHIN` ago at most.
fn quit_forced(last_attempt: Option<Instant>, now: Instant) -> bool {
    last_attempt.is_some_and(|t| now.saturating_duration_since(t) <= FORCE_QUIT_WITHIN)
}

/// What a window closing leads to: the last one quits; the project window gone with its "New
/// Window" chooser, About or Settings left (`aux_open`) closes those, so the quit follows.
fn after_window_closed(windows_left: usize, project_open: bool, aux_open: bool) -> AfterClose {
    if windows_left == 0 {
        AfterClose::Quit
    } else if !project_open && aux_open {
        AfterClose::CloseAux
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
        Tab::Viewer { .. } | Tab::Browser { .. } | Tab::Video { .. } => None,
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
    fn new(client: Arc<client::Client>, project: PathBuf, saved: Option<Workspace>, config: config::Config, theme: theme::Theme, system_is_dark: bool, installed: SystemTime, cx: &mut Context<Self>) -> Self {
        let (text_preset, preset) = config::resolve_preset(saved.as_ref().and_then(|w| w.text_preset.as_deref()), &config.font.preset, &config.font.presets);
        let invitation = telemetry::Invitation::new(invite_days(installed, SystemTime::now(), ::telemetry::channel().is_source()), ::telemetry::mode_for(Ok::<_, ()>(&config.telemetry), true));
        let reconcile_with_note = saved.is_some();
        let window_saved = saved.as_ref().and_then(|w| w.window.clone());
        let details_open = saved.as_ref().map(|w| w.details_open.clone()).unwrap_or_default();
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
            daemon_protocol_max: 0,
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
            preview_panes: HashMap::new(),
            context_menu: Transient::Hidden,
            link_menu: Transient::Hidden,
            md_menu: Transient::Hidden,
            notices: Default::default(),
            doctor_states: Vec::new(),
            doctor_busy: None,
            scroll_applied: false,
            sandbox: ::telemetry::is_sandbox(),
            popover: Transient::Hidden,
            bell_read_before: None,
            icon_hover: chrome::Popover::Closed,
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
            handle_hovered: HashSet::new(),
            handle_fades: motion::Fades::default(),
            find_bars: motion::Fades::default(),
            empty_anim: HashMap::new(),
            shell_name: shell_name(config.terminal.shell.as_deref(), std::env::var("SHELL").ok().as_deref()),
            config,
            fonts: installed_fonts(&theme, cx),
            base_theme: Arc::new(theme.clone()),
            theme: Arc::new(theme),
            theme_fade: None,
            painted: false,
            highlighter: None,
            viewers: HashMap::new(),
            videos: HashMap::new(),
            webs: HashMap::new(),
            web_titles: HashMap::new(),
            ns_view: None,
            key_monitor: None,
            effect: None,
            glyphs_pending: None,
            glyph_scale: 2.0,
            effect_timer: None,
            effect_timer_gen: 0,
            glyph_timer: false,
            glyph_timer_gen: 0,
            transparent: false,
            effect_divider: None,
            effect_marks: Vec::new(),
            effect_shapes: Vec::new(),
            effect_menus: Vec::new(),
            effect_strokes_now: Vec::new(),
            effect_hover: None,
            effect_strokes: effects::strokes::Strokes::default(),
            caret_wake: None,
            system_is_dark,
            installed,
            invitation,
            invite_pane: None,
            config_error: None,
            theme_error: None,
            preview: None,
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
            check_poll: None,
            last_input: Instant::now(),
            updating: None,
            browser_rects: HashMap::new(),
            ns_window: None,
            window_saved,
            details_open,
            reorder: None,
            tab_widths: HashMap::new(),
            url_prompt: Transient::Hidden,
            url_prompt_focus: cx.focus_handle(),
            tab_rename: None,
            tab_rename_focus: cx.focus_handle(),
            window_active: true,
            effects_paused: false,
            breath: pulse::Breath::default(),
            frame_now: Instant::now(),
            last_scroll: None,
            user_closing: false,
            said_goodbye: false,
            said_quit: false,
            focus_surface: None,
            last_quit_attempt: None,
        }
    }

    /// Opens the bell's popover, or closes it when it is the one open. Any right-click menu
    /// closes first: one floating card at a time.
    fn toggle_bell(&mut self, cx: &mut Context<Self>) {
        self.toggle_popover(chrome::Popover::Bell, cx);
    }

    /// Opens the doctor's popover, or closes it when it is the one open.
    fn toggle_doctor(&mut self, cx: &mut Context<Self>) {
        self.toggle_popover(chrome::Popover::Doctor, cx);
    }

    fn toggle_popover(&mut self, which: chrome::Popover, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.context_menu.hide(now);
        self.wt_menu.hide(now);
        self.link_menu.hide(now);
        self.md_menu.hide(now);
        self.tab_menu.hide(now);
        let next = self.popover.value().copied().unwrap_or_default().toggled(which);
        if next == chrome::Popover::Closed {
            self.close_popovers(now);
        } else {
            if self.popover.value() == Some(&chrome::Popover::Bell) && next != chrome::Popover::Bell {
                self.bell_closed();
            }
            if next == chrome::Popover::Doctor {
                self.probe_doctor();
            }
            if next == chrome::Popover::Bell {
                self.bell_read_before = self.notices.opened_at();
                self.notices.opened(SystemTime::now());
            }
            self.popover.show(next, now);
        }
        cx.notify();
    }

    /// Closes the popover; true when one was open. Closing the bell marks its notices read.
    pub fn close_popovers(&mut self, now: Instant) -> bool {
        let was_bell = self.popover.value() == Some(&chrome::Popover::Bell);
        let closed = self.popover.hide(now);
        if closed && was_bell {
            self.bell_closed();
        }
        closed
    }

    /// The bell is no longer showing: whatever arrived while it was open counts as read, and an
    /// Updated notice that was on show goes.
    fn bell_closed(&mut self) {
        self.notices.mark_read(SystemTime::now());
        self.notices.closed();
    }

    /// Derives the bell's notices from the state the bar used to show, every frame; idempotent
    /// because a notice with the same text keeps its time. No assignment site needs to know.
    fn sync_notices(&mut self, now: SystemTime) {
        match self.update_ready.as_deref().filter(|_| self.updating.is_none()) {
            Some(t) => self.notices.set(notices::Kind::UpdateReady, notices::update_ready_text(t), Some(notices::Action::UpdateNow), now),
            None => {
                self.notices.clear(notices::Kind::UpdateReady);
            }
        }
        let label = daemon_label_with(&self.status, self.reconnecting_since.is_some());
        // "see banner" is a stand-in for a banner the user may have missed, so show the text itself.
        let label = if label == "arkosd: see banner" { self.status.as_str() } else { label };
        match notices::daemon_notice(label, self.update_error.as_deref(), self.update_unreachable.as_deref()) {
            Some(t) => self.notices.set(notices::Kind::Daemon, t, None, now),
            None => {
                self.notices.clear(notices::Kind::Daemon);
            }
        }
        match &self.config_error {
            Some(e) => self.notices.set(notices::Kind::ConfigError, e.clone(), Some(notices::Action::OpenConfig), now),
            None => {
                self.notices.clear(notices::Kind::ConfigError);
            }
        }
        match &self.theme_error {
            Some(e) => self.notices.set(notices::Kind::ThemeError, e.clone(), Some(notices::Action::OpenTheme), now),
            None => {
                self.notices.clear(notices::Kind::ThemeError);
            }
        }
    }

    /// Seeds the notices a window starts with: "Updated to X" when this launch is an update.
    fn seed_notices(&mut self, updated: Option<String>) {
        if let Some(v) = updated {
            self.notices.set(notices::Kind::Updated, format!("Updated to {v}"), None, SystemTime::now());
        }
        self.probe_doctor();
    }

    /// At startup: Claude Code's Arkos theme file is brought up to date, and a stock or `auto`
    /// Claude Code theme becomes `custom:arkos`, whose colours are the terminal palette's, so
    /// Claude Code takes each Arkos theme, lines already drawn included; any other choice is
    /// left for the doctor's row to offer (`claude_theme::follow_at`). A sandbox writes neither.
    fn follow_theme_in_claude(&mut self, cx: &mut Context<Self>) {
        if self.sandbox {
            return;
        }
        let settings = hook_cli::claude_settings_path();
        let themes = claude_theme::themes_dir();
        let root = cx.entity();
        cx.spawn(async move |_, cx| {
            let done = cx.background_executor().spawn(async move { claude_theme::follow_at(&settings, &themes) }).await;
            let was = match done {
                Ok(d) => d.switched_from,
                Err(e) => {
                    eprintln!("arkos: Claude Code's Arkos theme: {e}");
                    None
                }
            };
            if let Some(was) = was {
                eprintln!("arkos: Claude Code's theme was {}; set to {}", was.as_deref().unwrap_or("unset"), claude_theme::SELECTED);
                let _ = root.update(cx, |root, cx| {
                    root.notices.set(notices::Kind::ClaudeColours, claude_theme::SWITCH_NOTICE, None, SystemTime::now());
                    root.probe_doctor();
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Runs what a notice's button (or its row) offers.
    fn run_notice_action(&mut self, action: notices::Action, cx: &mut Context<Self>) {
        match action {
            notices::Action::UpdateNow => self.update_clicked(cx),
            notices::Action::OpenConfig => self.open_config(cx),
            notices::Action::OpenTheme => self.open_theme(cx),
        }
    }

    /// Opens the configured theme's file in the default text editor, else the themes folder.
    pub fn open_theme(&mut self, cx: &mut Context<Self>) {
        let dir = paths::themes_dir();
        let file = self.theme_file();
        let mut cmd = std::process::Command::new("open");
        let target = if file.is_file() {
            cmd.arg("-t");
            file
        } else {
            dir
        };
        if let Err(e) = cmd.arg(&target).spawn() {
            let text = format!("cannot open {}: {e}", target.display());
            eprintln!("arkos: {text}");
            self.show_notice(text, cx);
        }
    }

    /// How many fixes the doctor has to offer; the dot on its icon shows when there are any.
    fn doctor_needs_fix(&self) -> usize {
        doctor_panel::needs_fix(&self.doctor_states)
    }

    /// Reads the doctor's table: files and flags only, no prompt, no launchctl. Run when the
    /// popover opens, when an action finishes, and once at startup so the dot is right.
    fn probe_doctor(&mut self) {
        let settings = hook_cli::claude_settings_path();
        let exe = update::launch_path().unwrap_or_else(|| PathBuf::from("arkos"));
        let skills = guide::skills_dir();
        let p = doctor_panel::Probe {
            settings: &settings,
            exe: &exe,
            start_at_login: self.config.recovery.start_at_login,
            login_item_installed: launch_agent::installed(&launch_agent::agent_path()),
            scroll_applied: self.scroll_applied,
            skill: guide::skill_state(&skills),
            claude_present: skills.parent().is_some_and(|d| d.is_dir()),
            skills: &skills,
        };
        self.doctor_states = doctor_panel::probe_all(&p, self.sandbox);
    }

    /// Runs what a doctor row's button offers. The file actions re-probe when they finish.
    fn doctor_action(&mut self, id: doctor_panel::CheckId, cx: &mut Context<Self>) {
        use doctor_panel::CheckId;
        if self.doctor_busy.is_some() {
            return;
        }
        self.doctor_busy = Some(id);
        match id {
            CheckId::WheelScroll | CheckId::ClaudeTheme if self.refused_in_sandbox(id) => {}
            CheckId::WheelScroll => {
                let path = hook_cli::claude_settings_path();
                let root = cx.entity();
                cx.spawn(async move |_, cx| {
                    let r = cx.background_executor().spawn(async move { hook_cli::set_wheel_scroll_acceleration_at(&path, false) }).await;
                    let _ = root.update(cx, |root, cx| {
                        root.doctor_busy = None;
                        match r {
                            Ok(()) => {
                                root.scroll_applied = true;
                                root.probe_doctor();
                            }
                            Err(e) => root.mark_doctor_broken(CheckId::WheelScroll, e),
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            CheckId::ClaudeTheme => {
                let path = hook_cli::claude_settings_path();
                let themes = claude_theme::themes_dir();
                let root = cx.entity();
                cx.spawn(async move |_, cx| {
                    let r = cx
                        .background_executor()
                        .spawn(async move { claude_theme::ensure_theme_file(&themes).and_then(|_| hook_cli::set_claude_theme_at(&path, claude_theme::SELECTED)) })
                        .await;
                    let _ = root.update(cx, |root, cx| {
                        root.doctor_busy = None;
                        match r {
                            Ok(()) => {
                                root.notices.set(notices::Kind::ClaudeColours, claude_theme::SWITCH_NOTICE, None, SystemTime::now());
                                root.probe_doctor();
                            }
                            Err(e) => root.mark_doctor_broken(CheckId::ClaudeTheme, e),
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
            CheckId::StartAtLogin => self.toggle_start_at_login(cx),
            CheckId::TitleHook => self.install_hooks(cx),
            CheckId::AgentGuide => self.install_agent_guide(cx),
        }
    }

    /// Re-probes the doctor, then shows `id`'s row as broken with `error` and no button: an action
    /// that failed once will fail again until the user fixes what the message names.
    fn mark_doctor_broken(&mut self, id: doctor_panel::CheckId, error: String) {
        self.probe_doctor();
        if let Some(c) = self.doctor_states.iter_mut().find(|c| c.id == id) {
            c.state = doctor_panel::State::Broken(error);
            c.button = None;
        }
    }

    /// Whether a sandbox refuses the action behind `id`'s doctor row (it would write the user's own
    /// setup); `id`'s row is no longer busy when it is. The row and the palette say why.
    fn refused_in_sandbox(&mut self, id: doctor_panel::CheckId) -> bool {
        if self.sandbox && self.doctor_busy == Some(id) {
            self.doctor_busy = None;
        }
        self.sandbox
    }

    /// Installs the Claude Code hooks off the UI thread and says what happened.
    fn install_hooks(&mut self, cx: &mut Context<Self>) {
        if self.refused_in_sandbox(doctor_panel::CheckId::TitleHook) {
            return;
        }
        let exe = update::launch_path().unwrap_or_else(|| PathBuf::from("arkos"));
        let root = cx.entity();
        cx.spawn(async move |_, cx| {
            let result = cx.background_executor().spawn(async move { hook_cli::install_hooks(&exe) }).await;
            let (text, error) = match result {
                Ok((path, added, replaced)) => (format!("installed {added} hook entries in {} (replaced {replaced}); restart claude for them to take effect", path.display()), None),
                Err(e) => (format!("cannot install hooks: {e}"), Some(e)),
            };
            let _ = root.update(cx, |root, cx| {
                root.doctor_busy = None;
                root.show_notice(text, cx);
                match error {
                    Some(e) => root.mark_doctor_broken(doctor_panel::CheckId::TitleHook, e),
                    None => root.probe_doctor(),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Installs the agent guide skill into this environment's Claude config off the UI thread and
    /// says what happened.
    fn install_agent_guide(&mut self, cx: &mut Context<Self>) {
        if self.refused_in_sandbox(doctor_panel::CheckId::AgentGuide) {
            return;
        }
        let root = cx.entity();
        cx.spawn(async move |_, cx| {
            let result = cx.background_executor().spawn(async move { guide::install_skill(&guide::skills_dir()) }).await;
            let (text, error) = match result {
                Ok(i) => {
                    let verb = if i.replaced { "updated" } else { "installed" };
                    (format!("{verb} the agent guide skill at {}; Claude Code loads it at its next start", i.path.display()), None)
                }
                Err(e) => (format!("cannot install the agent guide: {e}"), Some(e)),
            };
            let _ = root.update(cx, |root, cx| {
                root.doctor_busy = None;
                root.show_notice(text, cx);
                match error {
                    Some(e) => root.mark_doctor_broken(doctor_panel::CheckId::AgentGuide, e),
                    None => root.probe_doctor(),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Builds the doctor's report off the UI thread (it talks to the daemon and runs a
    /// subprocess) and puts it on the clipboard.
    fn copy_doctor_report(&mut self, cx: &mut Context<Self>) {
        let root = cx.entity();
        cx.spawn(async move |_, cx| {
            let text = cx.background_executor().spawn(async { doctor::report(&doctor::Where::real(), SystemTime::now()) }).await;
            let _ = root.update(cx, |root, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                root.show_notice("doctor report copied to the clipboard".into(), cx);
            });
        })
        .detach();
    }

    /// The open popover's card, drawn while it fades out too. The bell lists its notices; the
    /// doctor lists its checks, each with the button that fixes it.
    fn render_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let (which, opacity) = self.popover.current(self.frame_now)?;
        let which = *which;
        let live = self.popover.value().is_some();
        let w = f32::from(window.viewport_size().width);
        let card = chrome::popover_card(which, w, opacity, live, &self.theme, self.effect_style(), self.ui_family());
        let card = if self.effect_style().menu_edges { card.child(chrome::menu_report(0x15, opacity, cx)) } else { card };
        let c = &self.theme.chrome;
        let (text_c, dim_c) = (c.text.hsla(), c.text_dim.hsla());
        if which != chrome::Popover::Bell {
            let red = self.theme.terminal.red.hsla();
            let mut card = card;
            for (i, check) in self.doctor_states.iter().enumerate() {
                let (state_text, state_c) = match &check.state {
                    doctor_panel::State::Broken(t) => (t.clone(), red),
                    doctor_panel::State::Good(t) | doctor_panel::State::Fix(t) | doctor_panel::State::Pinned(t) => (t.clone(), dim_c),
                };
                let id = check.id;
                let mut row = div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(10.0))
                    .py(px(4.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_color(text_c).child(check.title))
                            .child(div().text_color(state_c).child(state_text)),
                    );
                if let Some(label) = check.button.filter(|_| self.doctor_busy != Some(id)) {
                    row = row.child(
                        div()
                            .id(("doctor-action", i))
                            .flex_none()
                            .text_color(text_c)
                            .cursor_pointer()
                            .hover(|s| s.underline())
                            .child(label)
                            .on_click(cx.listener(move |root, _, _, cx| {
                                root.doctor_action(id, cx);
                                cx.notify();
                            })),
                    );
                }
                card = card.child(row);
            }
            return Some(card.child(
                div().flex().flex_row().px(px(10.0)).py(px(4.0)).child(
                    div()
                        .id("doctor-copy-report")
                        .flex_none()
                        .text_color(text_c)
                        .cursor_pointer()
                        .hover(|s| s.underline())
                        .child("Copy report")
                        .on_click(cx.listener(|root, _, _, cx| {
                            root.copy_doctor_report(cx);
                        })),
                ),
            ));
        }
        let items = self.notices.items().to_vec();
        if items.is_empty() {
            return Some(card.child(div().px(px(10.0)).py(px(4.0)).text_color(dim_c).child("Nothing new")));
        }
        let now = SystemTime::now();
        let mut card = card;
        for (i, n) in items.iter().enumerate() {
            let unread = self.notices.unread_since(n, self.bell_read_before);
            let mut row = div()
                .id(("notice", i))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .px(px(10.0))
                .py(px(4.0))
                .child(div().flex_1().min_w_0().truncate().text_color(if unread { text_c } else { dim_c }).child(n.text.clone()))
                .child(div().flex_none().text_color(dim_c).child(notices::relative_time(n.at, now)));
            if let Some(action) = n.action {
                let label = match action {
                    notices::Action::UpdateNow => "Update now",
                    notices::Action::OpenConfig => "Open config",
                    notices::Action::OpenTheme => "Open theme",
                };
                row = row.child(
                    div()
                        .id(("notice-action", i))
                        .flex_none()
                        .text_color(text_c)
                        .cursor_pointer()
                        .hover(|s| s.underline())
                        .child(label)
                        .on_click(cx.listener(move |root, _, _, cx| {
                            // The update-ready row runs this action too; stopping here keeps it to one run.
                            cx.stop_propagation();
                            root.run_notice_action(action, cx);
                            root.close_popovers(Instant::now());
                            cx.notify();
                        })),
                );
                if n.kind == notices::Kind::UpdateReady {
                    row = row.cursor_pointer().on_click(cx.listener(move |root, _, _, cx| {
                        root.run_notice_action(action, cx);
                        root.close_popovers(Instant::now());
                        cx.notify();
                    }));
                }
            }
            card = card.child(row);
        }
        Some(card)
    }

    /// Whether any transition is under way at `now`, so the next frame looks different.
    pub fn any_motion(&self, now: Instant) -> bool {
        self.tab_menu.animating(now)
            || self.context_menu.animating(now)
            || self.link_menu.animating(now)
            || self.md_menu.animating(now)
            || self.popover.animating(now)
            || self.wt_menu.animating(now)
            || self.palette.animating(now)
            || self.notice.animating(now)
            || self.wt_prompt.animating(now)
            || self.url_prompt.animating(now)
            || self.drop_tints.values().any(|t| t.animating(now))
            || self.reorder.as_ref().is_some_and(|r| r.animating(now))
            || self.glow.animating(now)
            || self.handle_fades.animating(now)
            || self.find_bars.animating(now)
            || self.sidebar.animating(now)
            || self.theme_fade.is_some()
            || self.effect.as_ref().is_some_and(|e| e.fade.is_some())
    }

    /// Drops what finished fading out, and the per-key fades the frame drawn at `last_frame`
    /// did not draw.
    fn settle(&mut self, now: Instant, last_frame: Instant) {
        if self.reorder.as_ref().is_some_and(|r| r.done(now)) {
            self.reorder = None;
        }
        self.drop_tints.retain(|_, t| {
            t.fade.settle(now);
            !t.fade.is_hidden()
        });
        self.glow.settle(now, last_frame);
        self.handle_fades.settle(now, last_frame);
        self.find_bars.settle(now, last_frame);
        self.tab_menu.settle(now);
        self.context_menu.settle(now);
        self.link_menu.settle(now);
        self.md_menu.settle(now);
        self.popover.settle(now);
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

    /// Switches to `theme`. The fonts change at once (they cannot fade); the colours cross-fade
    /// from what is on screen over `motion::THEME_FADE`, frame by frame in `step_theme_fade`, and
    /// what is derived from the theme once (the highlighter, the viewers, the push a program
    /// asked for) waits for the fade's end. Under Reduce Motion, or before the first frame, the
    /// switch is instant. A theme equal to the one being shown or faded to does nothing. An
    /// effect settled on deepens its text (`effects::active::contrasted`).
    pub fn set_theme(&mut self, theme: theme::Theme, cx: &mut Context<Self>) {
        if *self.base_theme == theme {
            return;
        }
        let fonts = installed_fonts(&theme, cx);
        // A theme that brings its own terminal font changes the cell size (`family()` reads the
        // theme): panes re-measure before a restore trusts a grid, as a config change does.
        if fonts.terminal != self.fonts.terminal {
            self.grids.clear();
        }
        self.fonts = fonts;
        let shown = effects::active::contrasted(&theme, self.text_contrast());
        let accent_changed = theme.chrome.focus_ring != self.base_theme.chrome.focus_ring;
        self.base_theme = Arc::new(theme);
        // The glyph shader is told the theme's accent when it is compiled, so a new accent
        // compiles it again; the turn of its colours cuts rather than fades, as glyphs can only.
        if accent_changed && self.effect.as_ref().is_some_and(|e| e.settled()) {
            self.glyphs_pending = Some(self.glyph_shader_for(effects::active::WindowKind::Project));
        }
        self.fade_theme_to(shown, cx);
    }

    /// Cross-fades the window's colours to `theme` from what is on screen, or switches at once
    /// under Reduce Motion or before the first frame. A theme already shown or faded to does
    /// nothing.
    fn fade_theme_to(&mut self, theme: theme::Theme, cx: &mut Context<Self>) {
        if **self.target_theme() == theme {
            return;
        }
        let to = Arc::new(theme);
        if motion::reduce_motion() || !self.painted {
            self.finish_theme(to, cx);
            return;
        }
        // From the colours on screen (a blend, when a switch lands mid-fade). The name and fonts
        // are the target's from this frame on, so the state reported and the fonts drawn agree.
        let from = self.theme.clone();
        self.theme = Arc::new(theme::blend(&from, &to, 0.0));
        self.theme_fade = Some(ThemeFade { from, to, since: Instant::now() });
        cx.notify();
    }

    /// The theme the window is at or is fading to: what a switch compares against and what the
    /// state report names. `theme` itself is a blend while a fade runs.
    pub fn target_theme(&self) -> &Arc<theme::Theme> {
        fade_target(self.theme_fade.as_ref(), &self.theme)
    }

    /// One frame of the theme cross-fade: the blend at `now` for the window, the terminals (for
    /// painting only; no program is told) and the aux windows. At the end, the final theme lands.
    fn step_theme_fade(&mut self, now: Instant, cx: &mut Context<Self>) {
        let Some(fade) = &self.theme_fade else { return };
        let p = motion::fade_progress(fade.since, now, motion::THEME_FADE);
        if p >= 1.0 {
            let to = fade.to.clone();
            self.finish_theme(to, cx);
            return;
        }
        self.theme = Arc::new(theme::blend(&fade.from, &fade.to, p));
        let palette = Arc::new(self.theme.palette());
        for t in self.sessions.terminals.values() {
            t.update(cx, |t, cx| {
                t.core.set_palette_quiet(palette.clone());
                cx.notify();
            });
        }
        aux::push_theme(cx, &self.theme);
        // The colour behind a page, pushed once from the final theme (never the per-frame blend).
        let under = self.theme.chrome.window_bg;
        for v in self.webs.values() {
            v.update(cx, |v, _| v.set_under_color((under.r, under.g, under.b)));
        }
    }

    /// The final theme of a switch, and everything derived from a theme once: the highlighter is
    /// rethemed, every open viewer re-highlights, About, Settings and Keybindings follow, and a
    /// program that asked for theme pushes (Claude Code's Auto theme) hears the switch.
    fn finish_theme(&mut self, theme: Arc<theme::Theme>, cx: &mut Context<Self>) {
        self.theme_fade = None;
        if let Some(h) = &self.highlighter {
            self.highlighter = Some(Arc::new(h.with_theme(&theme.viewer, theme.chrome.text)));
        }
        self.theme = theme;
        aux::push_theme(cx, &self.theme);
        self.reload_all_viewers(cx);
        // A theme's recommendation may have changed with it (`effect.name = "theme"`).
        self.resolve_effect(cx);
        let palette = Arc::new(self.theme.palette());
        for t in self.sessions.terminals.values() {
            t.update(cx, |t, cx| {
                if let Some(push) = t.core.set_palette(palette.clone()) {
                    t.client.send(Request::Input { session: t.session, bytes: push.to_vec() });
                }
                cx.notify();
            });
        }
        cx.notify();
    }

    /// macOS reported its appearance. Only a switch between light and dark counts (see
    /// `appearance_step`). In "system" mode the theme follows; a theme that fails to load leaves
    /// the current one and says why in the banner. A preview ends here, in any mode, so the screen
    /// shows what the config names.
    pub fn set_system_appearance(&mut self, dark: bool, cx: &mut Context<Self>) {
        let (end_preview, switch) = appearance_step(self.system_is_dark, dark, self.config.appearance.mode, self.preview.is_some());
        self.system_is_dark = dark;
        if end_preview {
            self.preview = None;
        }
        if switch {
            self.switch_theme(cx);
        }
    }

    /// Loads the theme the config and appearance name and switches to it if it differs. A theme
    /// that fails to load keeps the current one; `theme_error` says so until a load succeeds.
    fn switch_theme(&mut self, cx: &mut Context<Self>) {
        self.switch_theme_to(self.resolve_theme(), cx);
    }

    /// Switches to `wanted` if it loaded and differs; a load error keeps the current theme and
    /// sets `theme_error`, a success clears it.
    fn switch_theme_to(&mut self, wanted: Result<theme::Theme, String>, cx: &mut Context<Self>) {
        // Against the configured theme, not the blend on screen (at the fade's first frame the
        // blend has the old colours under the new name, and a revert must still switch) nor the
        // one an effect deepened.
        let (switch, banner) = theme::keep_or_switch(&self.base_theme, wanted);
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
    /// an error, apply what changed. Called by the watcher (for the config file and the
    /// configured theme's file), by the palette's "Reload config" and by
    /// `CustomizeAction::ReloadConfig`. A preview ends here, whether or not the file loads.
    pub fn reload_config(&mut self, cx: &mut Context<Self>) {
        let previewing = self.preview.take().is_some();
        // The same migration as at launch: a file edited back to Navy Light moves off it.
        let retired = config::retire_navy_light_file(&paths::config_file(), &paths::themes_dir());
        if retired == config::Retired::Light {
            self.notices.set(notices::Kind::ThemeRetired, config::NAVY_LIGHT_NOTICE, None, SystemTime::now());
        }
        let (cfg, banner) = config::reload_decision(&self.config, config::load(&paths::config_file()));
        self.config_error = banner;
        if let Some(e) = &self.config_error {
            eprintln!("arkos: {e}");
            // The last good config still names a theme: show it, not the ended preview.
            if previewing {
                self.switch_theme(cx);
            }
        } else {
            eprintln!("arkos: config reloaded");
            self.apply_config(cfg, cx);
        }
        self.invitation_config_changed();
        self.probe_doctor();
        // The Settings window shows the reloaded file, or its problem.
        aux::push_settings(cx, self.settings_shown());
        cx.notify();
    }

    /// What the Settings window shows from this window's state: the badge's version, without its
    /// "update ready: " prefix, and the text preset this window uses.
    pub fn settings_shown(&self) -> settings::Shown {
        let update_ready = self.update_ready.as_deref().map(|t| update::ready_version(t).unwrap_or(t).to_string());
        settings::Shown {
            config: self.config.clone(),
            error: self.config_error.clone(),
            write_error: None,
            update_ready,
            active_preset: Some(self.text_preset.clone()),
            sandbox: self.sandbox,
        }
    }

    /// Applies every live setting. The `[daemon]` section only matters when the window starts
    /// the daemon; the file says so.
    pub fn apply_config(&mut self, cfg: config::Config, cx: &mut Context<Self>) {
        let old = std::mem::replace(&mut self.config, cfg);
        // The keys changed, the theme chosen, and a usage change: a new journal's id goes to the
        // daemon at once (on a reconnect, `telemetry::connect_sid` names it).
        telemetry::config_applied(&old, &self.config, self.sessions.client.as_deref());
        self.invitation_config_changed();
        if old.keybindings != self.config.keybindings {
            rebind(cx, &self.config.keybindings);
            // An open Keybindings window shows the saved override at once.
            aux::push_keybindings(cx, &self.config.keybindings);
        }
        self.switch_theme(cx);
        // In step with the theme's fade when both change; the intensity is read every frame.
        self.resolve_effect(cx);
        // `[effect] run_unfocused` may have changed: an unfocused window pauses or resumes now.
        self.sync_pause(Instant::now());
        if let Some(name) = config::preset_after_reload(&self.text_preset, &old.font, &self.config.font) {
            self.apply_text_preset(&name, cx);
        }
        if old.font.family != self.config.font.family && self.fonts.terminal.is_none() {
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

    /// The configured theme's file, `themes_dir/<name>.toml`, whether or not it exists. The
    /// watcher polls it; a preview or a failed load never moves it.
    pub fn theme_file(&self) -> PathBuf {
        theme_file_for(self.config.appearance.mode, self.system_is_dark, &self.config.theme, &paths::themes_dir())
    }

    /// What this window shows, for an agent's `Customize` (spec 5).
    pub fn report(&self) -> protocol::Report {
        protocol::Report {
            theme: self.target_theme().name.clone(),
            dark: !self.target_theme().is_light(),
            system_is_dark: self.system_is_dark,
            preset: self.text_preset.clone(),
            family: self.family().to_string(),
            preview: self.preview.clone(),
            config_error: self.config_error.clone(),
            theme_error: self.theme_error.clone(),
        }
    }

    /// Applies an agent's `Event::Customize` and answers the daemon with `CustomizeDone`. Never
    /// writes a file: a preview lives in this window only.
    fn customize(&mut self, id: u64, action: CustomizeAction, cx: &mut Context<Self>) {
        // Only a preview parses anything; the other actions cannot fail.
        let parsed = match &action {
            CustomizeAction::PreviewTheme { toml } => {
                let parsed = theme::parse(toml);
                if let Ok(t) = &parsed {
                    self.preview = Some(t.name.clone());
                    self.set_theme(t.clone(), cx);
                }
                parsed
            }
            CustomizeAction::EndPreview => {
                self.preview = None;
                self.switch_theme(cx);
                Err(String::new())
            }
            CustomizeAction::ReloadConfig => {
                self.reload_config(cx);
                Err(String::new())
            }
            CustomizeAction::ReportState => Err(String::new()),
        };
        let outcome = CustomizeOutcome {
            pid: std::process::id(),
            project: self.project.clone(),
            result: customize_outcome(&action, parsed, self.report()),
        };
        // The event came through the client, so there is one; the daemon matches the pid. A
        // daemon that speaks the detail is told what the window draws too.
        if let Some(client) = &self.sessions.client {
            if self.daemon_protocol_max >= protocol::EFFECT_REPORT_FROM {
                client.send(Request::CustomizeDoneDetailed { id, outcome, detail: self.window_detail() });
            } else {
                client.send(Request::CustomizeDone { id, outcome });
            }
        }
    }

    /// What the window draws, for `arkos effect current` and `bench`: its effect with the GPU's
    /// time per frame, and its content's size in points.
    fn window_detail(&self) -> protocol::WindowDetail {
        let size = self.ns_window.as_ref().and_then(|ns| ns.contentView()).map(|v| {
            let b = v.bounds();
            (b.size.width as f32, b.size.height as f32)
        });
        protocol::WindowDetail { effect: effects::active::reported(self.effect.as_ref(), |l| l.gpu_ms()), size }
    }

    /// The terminal font: the theme's when it names one, else the config's.
    pub fn family(&self) -> gpui::SharedString {
        self.fonts.terminal.clone().unwrap_or_else(|| self.config.font.family.clone()).into()
    }

    /// The font of the file tree, tabs, palette and banners: the theme's `ui` font when it
    /// names one, else the terminal font, as it always was.
    pub fn ui_family(&self) -> gpui::SharedString {
        self.fonts.ui.clone().map_or_else(|| self.family(), Into::into)
    }

    /// The rendered markdown's font: the theme's prose font when it names one that is
    /// installed, else the system's.
    pub fn prose_family(&self) -> gpui::SharedString {
        self.fonts.prose.clone().map_or(gpui::SharedString::from(chrome::UI_FONT), Into::into)
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

    /// Remembers where the window is, whenever that changes, so a relaunch opens it there. A
    /// full-screen or minimized window is no place to come back to and is not noted.
    fn note_window_placement(&mut self, window: &Window, cx: &mut App) {
        if window.is_fullscreen() || self.ns_window.as_ref().is_some_and(|ns| screen::window_hidden(ns)) {
            return;
        }
        let b = window.bounds();
        let (x, y, w, h) = (f64::from(f32::from(b.origin.x)), f64::from(f32::from(b.origin.y)), f64::from(f32::from(b.size.width)), f64::from(f32::from(b.size.height)));
        if self.window_saved.as_ref().is_some_and(|s| (s.x, s.y, s.w, s.h) == (x, y, w, h)) {
            return;
        }
        let Some(display) = window.display(cx).and_then(|d| d.uuid().ok()) else { return };
        self.window_saved = Some(workspace::WindowSaved { display: display.to_string(), x, y, w, h });
        self.save(cx);
    }

    pub fn save(&mut self, cx: &mut App) {
        let sidebar = SidebarSaved { width: self.sidebar.width, collapsed: self.sidebar.collapsed, show_ignored: self.explorer.show_ignored, expanded: self.explorer.expanded_dirs(), worktrees_expanded: self.worktrees.expanded.iter().cloned().collect(), explorer_open: self.sidebar.explorer_open, worktrees_open: self.sidebar.worktrees_open };
        let ws = Workspace { version: FORMAT_VERSION, project: self.project.clone(), tree: self.tree.clone(), text_preset: Some(self.text_preset.clone()), sidebar: Some(sidebar), window: self.window_saved.clone(), details_open: self.details_open.clone() };
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
            Tab::Viewer { .. } | Tab::Browser { .. } | Tab::Video { .. } => None,
        }
    }

    /// Pauses the effect's clock and the empty panes' at `now`, or resumes them, as the setting
    /// and the window's focus say (`effects::active::paused`). Settings in front counts as focus,
    /// so its effect preview keeps moving. Called where the focus or the config changes and on
    /// every render; pausing what is paused changes nothing.
    pub fn sync_pause(&mut self, now: Instant) {
        let focused = self.window_active || settings::settings_is_key();
        let paused = effects::active::paused(self.config.effect.run_unfocused, focused);
        self.effects_paused = paused;
        if let Some(e) = self.effect.as_mut() {
            e.set_paused(paused, now);
        }
        for clock in self.empty_anim.values_mut() {
            clock.set_paused(paused, now);
        }
    }

    /// Tells the focus clock what the window shows, when that changed since it was last told.
    fn note_focus(&mut self) {
        let surface = telemetry::surface_of(&self.tree, self.window_active);
        if surface != self.focus_surface {
            self.focus_surface = surface;
            telemetry::window_focus(surface);
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

    /// Closes a tab; the pane stays (spec 4). A terminal's session is detached at once and ended
    /// only after `CLOSE_GRACE`, so Cmd+Shift+T can bring the same shell back: by the daemon when
    /// it speaks protocol 5 (a window that crashes in the minute orphans nothing), else by this
    /// window. A session whose shell has ended is killed at once and never enters the closed
    /// stack. A viewer's unsaved edits are written first, on this thread; a tab whose file changed
    /// on disk, or whose write failed, stays open with its banner.
    pub fn close_tab(&mut self, pane: PaneId, idx: usize, cx: &mut Context<Self>) {
        let Some(id) = self.tree.tab_id(pane, idx) else { return };
        if let Some(viewer) = self.viewers.get(&id).cloned() {
            let kept = viewer.update(cx, |v, cx| match v.save_now() {
                Ok(()) => None,
                Err(e) => {
                    cx.notify();
                    Some(e)
                }
            });
            if let Some(e) = kept {
                // The banner says it too; the notice says why Cmd+W did nothing.
                self.show_notice(e, cx);
                return;
            }
        }
        // A video's place and loop go with it into the closed stack, so a reopen comes back there,
        // and it pauses now, so the sound and the display token end at once (spec 3.5: no grace)
        // even while something still holds the view.
        if let Some(video) = self.videos.get(&id).cloned() {
            let (at, loop_range) = video.read(cx).saved_state();
            self.tree.set_video_state(pane, idx, at, loop_range);
            video.update(cx, |v, cx| v.pause(cx));
        }
        let Some(tab) = self.tree.close_tab(pane, idx) else { return };
        telemetry::pane_close(&tab);
        self.preview_tab = after_close(self.preview_tab, id);
        self.viewers.remove(&id);
        // The only strong hold on the view: dropping it releases the player (`AvPlayer::drop`).
        self.videos.remove(&id);
        // The only strong hold on the web view: dropping it removes the WKWebView from the window.
        self.webs.remove(&id);
        self.web_titles.remove(&id);
        self.tab_widths.remove(&id);
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
                if self.daemon_protocol_max >= protocol::DETACH_FOR_GRACE_FROM {
                    // Marked daemon-timed when the daemon's `Ok` arrives (`Outcome::GraceKept`);
                    // until then, or if it refuses, the window's own minute applies.
                    self.sessions.detach_for_grace(session, CLOSE_GRACE.as_secs() as u32);
                } else {
                    self.sessions.detach(session);
                }
                for old in self.closed.push(ClosedTab { closed: Closed::Session(session, cwd), pane, closed_at: Instant::now(), daemon_timed: false }) {
                    eprintln!("arkos: more than {MAX_CLOSED} closed tabs, ending session {}", old.0);
                    self.sessions.kill(old);
                }
            }
            // A shell still starting for this tab is killed when it arrives (`handle_outcome`).
            self.sessions.tab_closed(id);
        } else {
            let closed = closed_entry(tab).expect("a terminal is handled above");
            for old in self.closed.push(ClosedTab { closed, pane, closed_at: Instant::now(), daemon_timed: false }) {
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
        telemetry::pane_open(telemetry::PaneKind::Terminal, telemetry::Opened::Click);
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
            Tab::Viewer { .. } | Tab::Browser { .. } | Tab::Video { .. } => None,
        }
    }

    /// Starts a fresh shell for `tab` in its folder: the project, or the worktree it was opened
    /// in. A worktree gone meanwhile gives the project, and the tab forgets it, so the next
    /// restart checks the shell against the folder it really started in.
    fn start_shell(&mut self, tab: TabId, cwd: Option<PathBuf>, note: bool) {
        // The note is the "(restarted)" one: the session the tab showed is gone.
        if note {
            telemetry::error(telemetry::Failure::SessionLost);
        }
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

    /// Kills the sessions of closed tabs whose grace has run out, except those the daemon ends
    /// itself; those just leave the stack.
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

    /// The palette's "Open config" (spec 7): opens the config file in the default text editor.
    /// Task 2 creates the file at launch, so it exists by the time this runs.
    pub fn open_config(&mut self, cx: &mut Context<Self>) {
        let path = paths::config_file();
        if let Err(e) = std::process::Command::new("open").arg("-t").arg(&path).spawn() {
            let text = format!("cannot open {}: {e}", path.display());
            eprintln!("arkos: {text}");
            self.show_notice(text, cx);
        }
    }

    /// What the About window shows, from this window's state: the daemon's version and pid while
    /// connected. The version stays from the last `Hello` after a disconnect, so the connection
    /// state decides.
    pub fn details(&self) -> about::Details {
        let connected = daemon_connected(&self.status, self.reconnecting_since.is_some());
        let daemon = self.daemon_version.clone().filter(|_| connected).map(|v| (v, self.sessions.client.as_ref().and_then(|c| c.peer_pid())));
        about::Details::gather(self.installed, daemon)
    }

    /// The Help menu's "What Arkos Sends" in the project window: writes the disclosure page into
    /// the telemetry folder and opens it pinned in the focused pane, as a link to a file would.
    /// A page that cannot be written is a notice.
    fn open_disclosure(&mut self, cx: &mut Context<Self>) {
        telemetry::help(telemetry::Help::Telemetry);
        match ::telemetry::disclosure::write_page(&paths::telemetry_dir()) {
            Ok(path) => {
                let _via = telemetry::opened_via(telemetry::Opened::Click);
                self.open_viewer(&path, true, cx);
            }
            Err(e) => self.show_notice(format!("cannot write the disclosure page: {e}"), cx),
        }
    }

    /// Whether the invitation shows this frame (`telemetry::Invitation::shows`). `asked` is read
    /// the first time the strip would show, then again when the window comes to the front and
    /// every `telemetry::ASKED_EVERY`, since another window may have had its answer meanwhile;
    /// once it is there, this run never asks again.
    fn invitation_shows(&mut self) -> bool {
        self.invitation.shows(|| telemetry::asked(&paths::telemetry_dir()))
    }

    /// The config, or its error, changed: the invitation takes the mode it gives now. A config
    /// that does not load is telemetry `Off`, which never invites.
    fn invitation_config_changed(&mut self) {
        let config = if self.config_error.is_some() { Err(()) } else { Ok(&self.config.telemetry) };
        self.invitation.set_mode(::telemetry::mode_for(config, true));
    }

    /// Looks for `asked` again at the next frame, while the strip shows: on activation and every
    /// `telemetry::ASKED_EVERY`. `false` once the invitation can no longer show in this run.
    fn recheck_invitation(&mut self, cx: &mut Context<Self>) -> bool {
        if self.invite_pane.is_some() {
            self.invitation.recheck();
            cx.notify();
        }
        self.invitation.possible()
    }

    /// The invitation's Turn on or Not now: writes `asked`, so no window asks again, and takes
    /// the strip down. Turn on writes `telemetry.usage = "on"` through the config writer and
    /// reloads, as the watcher would: the reload hands the change to the Recorder, which deletes
    /// the session reports queued before the consent, and the daemon does the same when it next
    /// reads the file.
    fn answer_invitation(&mut self, answer: telemetry::Answer, cx: &mut Context<Self>) {
        self.invitation.answered();
        self.invite_pane = None;
        if let Err(e) = telemetry::answer_invitation(&paths::telemetry_dir(), answer) {
            eprintln!("arkos: cannot write the invitation's answer: {e}");
        }
        if answer == telemetry::Answer::TurnOn {
            match config::set_values(&paths::config_file(), &[(&["telemetry", "usage"], "on".into())]) {
                Ok(()) => self.reload_config(cx),
                Err(e) => self.show_notice(format!("cannot turn usage reports on: {e}"), cx),
            }
        }
        cx.notify();
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
        self.close_popovers(Instant::now());
        let themes_dir = paths::themes_dir();
        let themes: Vec<(String, Option<theme::Appearance>)> = theme::kinds(&themes_dir).into_iter().map(|(n, _, k)| (n, k)).collect();
        let items = command_palette::items(&self.config, &themes, &self.text_preset, &self.theme.name, &themes_dir, self.update_ready.as_deref(), self.update_error.is_some(), self.config.recovery.start_at_login, launch_agent::installed(&launch_agent::agent_path()), &hook_cli::claude_settings_path(), self.explorer.show_ignored, self.notices.unread_count(), self.doctor_needs_fix(), self.sandbox);
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
            telemetry::palette(&cmd);
            let _via = telemetry::opened_via(telemetry::Opened::Palette);
            self.run(cmd, window, cx);
        }
    }

    /// The first move of a tab dragged within its own bar starts the reorder slide: its slot
    /// closes and a gap its width follows the insertion point. A drag from another pane has no
    /// slide; its tabs stay put until the drop.
    pub fn begin_reorder(&mut self, pane: PaneId, d: &drag::TabDrag) {
        if d.pane != pane || self.reorder.as_ref().is_some_and(|r| r.tab == d.tab && !r.ended()) {
            return;
        }
        let width = self.tab_widths.get(&d.tab).copied().unwrap_or(80.0);
        self.reorder = Some(pane_view::Reorder::start(pane, d.tab, width, Instant::now()));
    }

    /// The slot a tab dragged along its own bar would land in, from the pointer's place in the
    /// strip of tabs (its bounds and scroll come from the bar's scroll handle). The tabs'
    /// recorded widths decide, not what is under the pointer: the gap itself moves the tabs. At
    /// the start the gap sits in the tab's own place, so grabbing a tab off centre moves nothing.
    pub fn reorder_slot_for(&self, pane: PaneId, tab: TabId, pos: gpui::Point<gpui::Pixels>) -> usize {
        let Some(p) = self.tree.pane(pane) else { return usize::MAX };
        let Some(idx) = p.ids.iter().position(|i| *i == tab) else { return usize::MAX };
        let Some(strip) = self.bar_scroll.get(&pane) else { return usize::MAX };
        let widths: Vec<f32> = p.ids.iter().map(|id| self.tab_widths.get(id).copied().unwrap_or(80.0)).collect();
        let x = f32::from(pos.x - strip.bounds().origin.x - strip.offset().x);
        let current = self.reorder.as_ref().filter(|r| r.pane == pane && r.tab == tab).and_then(|r| r.at());
        pane_view::reorder_slot(&widths, idx, current.or(Some(idx)), x)
    }

    /// Lands a dragged tab. The tab is found by id, so a drag that outlived a move of its tab
    /// still lands the right one, and one whose tab closed does nothing. The source pane closes
    /// only when the drag emptied it, and never when it is the last pane (spec 4).
    pub fn drop_tab(&mut self, d: &drag::TabDrag, target: drag::DropTarget, cx: &mut Context<Self>) {
        self.drop_target = None;
        if self.tree.locate(d.tab).is_some() {
            self.preview_tab = after_drop(self.preview_tab, d.tab);
        }
        if self.place_tab(d.tab, target) {
            // The tab lands in the gap the reorder opened, which was its own width: nothing to slide.
            self.reorder = None;
            self.save(cx);
        } else if let Some(r) = self.reorder.as_mut() {
            // Nowhere to land: the gap closes and the slot opens again.
            r.end(Instant::now());
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
        telemetry::pane_open(telemetry::PaneKind::Terminal, telemetry::Opened::Click);
        self.tree.focus(pane);
        if let Some(id) = self.tree.tab_id(pane, idx) {
            self.start_shell(id, cwd, false);
        }
        self.save(cx);
        cx.notify();
    }

    /// A video tab's view, made the first time the tab is shown: it loads the file (or waits for
    /// it) and comes up paused where the tab was saved. Its pauses, seeks and loops are written
    /// into the tab and saved.
    pub fn video_for(&mut self, tab: TabId, path: &Path, cx: &mut Context<Self>) -> Entity<video_view::VideoView> {
        if let Some(v) = self.videos.get(&tab) {
            return v.clone();
        }
        let (at, loop_range) = match self.tree.locate(tab).and_then(|(pane, idx)| self.tree.pane(pane)?.tabs.get(idx)) {
            Some(Tab::Video { at, loop_range, .. }) => (*at, *loop_range),
            _ => (None, None),
        };
        let path = path.to_path_buf();
        let v = cx.new(|cx| video_view::VideoView::new(path, at, loop_range, Box::new(av_player::AvPlayer::new()), cx));
        cx.subscribe(&v, move |root, v, _event: &video_view::VideoEvent, cx| {
            // A view no longer the tab's (a preview whose tab now shows another file, paused as it
            // was let go) must not write its place into the tab's new file.
            if root.videos.get(&tab).map(|own| own.entity_id()) != Some(v.entity_id()) {
                return;
            }
            let (at, loop_range) = v.read(cx).saved_state();
            if let Some((pane, idx)) = root.tree.locate(tab) {
                root.tree.set_video_state(pane, idx, at, loop_range);
                root.save(cx);
            }
        })
        .detach();
        self.videos.insert(tab, v.clone());
        v
    }

    /// The focused pane's video, when its active tab is one.
    pub fn active_video(&self) -> Option<Entity<video_view::VideoView>> {
        let p = self.tree.pane(self.tree.focused)?;
        match p.tabs.get(p.active)? {
            Tab::Video { .. } => self.videos.get(p.ids.get(p.active)?).cloned(),
            Tab::Terminal { .. } | Tab::Viewer { .. } | Tab::Browser { .. } => None,
        }
    }

    /// Runs a video command on the focused pane's video; nothing when it shows something else.
    fn with_active_video(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut video_view::VideoView, &mut Context<video_view::VideoView>)) {
        if let Some(v) = self.active_video() {
            v.update(cx, f);
        }
    }

    /// A video plays only while it is the active tab of a pane on screen in a window that is not
    /// hidden or minimized (spec 3.5): every other video view is made inactive, which pauses it
    /// (and lets the display sleep) until it is shown again. It runs at the top of every render,
    /// which covers a tab switch, a pane closed and a maximize, and from the once a second poll
    /// (`tick_videos`) and the window's activation, since a hidden window draws nothing.
    pub fn update_video_activity(&mut self, cx: &mut Context<Self>) {
        let shown: Vec<PaneId> = self.layout().panes.into_iter().map(|(p, _)| p).collect();
        let hidden = self.ns_window.as_ref().is_some_and(|ns| screen::window_hidden(ns));
        for (tab, v) in &self.videos {
            let plays = video_plays(&self.tree, &shown, *tab, hidden);
            v.update(cx, |v, cx| v.set_inactive(!plays, cx));
        }
    }

    /// Opens `path` in `pane` as the kind its name says (`sidebar::file_tab`): a video as a video
    /// tab, else the viewer, pinned. A tab already on the file there is reused. The tab becomes
    /// the pane's active one without moving the focus (`Tree::activate_quiet`); a caller that
    /// wants the keys there focuses the pane itself. `how` is recorded for a new tab.
    pub fn open_path_in_pane(&mut self, path: &Path, pane: PaneId, how: telemetry::Opened, cx: &mut Context<Self>) -> (usize, bool) {
        let reused = self.tree.find_path(pane, path);
        let idx = match reused {
            Some(idx) => {
                // Opened on purpose: a preview it was is kept.
                if self.tree.tab_id(pane, idx) == self.preview_tab {
                    self.preview_tab = None;
                }
                idx
            }
            None => {
                let tab = sidebar::file_tab(path);
                telemetry::pane_open(telemetry::kind_of(&tab), how);
                self.tree.add_tab(pane, tab)
            }
        };
        self.tree.activate_quiet(pane, idx);
        if let Some(h) = self.bar_scroll.get(&pane) {
            h.scroll_to_item(idx);
        }
        self.save(cx);
        cx.notify();
        (idx, reused.is_some())
    }

    /// "Open file in pane...": `path` in the focused pane, pinned and focused
    /// (`open_path_in_pane`). A path that is not a file says so.
    pub fn open_file_in_pane(&mut self, path: &Path, cx: &mut Context<Self>) {
        let path = match std::fs::canonicalize(path) {
            Ok(p) if p.is_file() => p,
            _ => {
                self.show_notice(format!("no such file: {}", path.display()), cx);
                return;
            }
        };
        let pane = self.tree.focused;
        let (idx, _) = self.open_path_in_pane(&path, pane, telemetry::Opened::Palette, cx);
        self.tree.focus(pane);
        self.activate_tab(pane, idx, cx);
    }

    /// A poster's Cmd+click in the document shown by tab `doc` (spec 9): the video opens paused
    /// in the pane after the document's (`pane_after`), else in a new pane split to its right. A
    /// video tab already on the file in another pane is brought forward instead (one in the
    /// document's own pane would hide the document). The keys stay in the document either way;
    /// a maximize that would hide the video ends, since the click asked to see it.
    pub fn open_poster_video(&mut self, doc: TabId, path: &Path, cx: &mut Context<Self>) {
        self.open_beside_document(doc, path, telemetry::Opened::Poster, cx);
    }

    /// Opens `path` beside the document tab `doc`, as its kind says (`sidebar::file_tab`: a video
    /// tab, a web tab for a page): a tab on the file in another pane is shown again; else it goes
    /// into the pane after the document's, or a new pane split off it to the right. The keys stay
    /// with the document.
    pub fn open_beside_document(&mut self, doc: TabId, path: &Path, how: telemetry::Opened, cx: &mut Context<Self>) {
        let focused = self.tree.focused;
        let doc_pane = self.tree.locate(doc).map_or(focused, |(pane, _)| pane);
        let shows = |t: &Tab| match t {
            Tab::Video { path: p, .. } => p == path,
            Tab::Browser { url } => web::file_path(url).is_some_and(|p| p == path),
            Tab::Terminal { .. } | Tab::Viewer { .. } => false,
        };
        let existing = self.tree.tabs().into_iter().find(|(pane, _, t)| *pane != doc_pane && shows(t)).map(|(pane, idx, _)| (pane, idx));
        let target = existing.map(|(pane, _)| pane).or_else(|| pane_after(&self.tree, doc_pane));
        if self.tree.maximized.is_some() && self.tree.maximized != target {
            self.tree.maximized = None;
        }
        match (existing, target) {
            (Some((pane, idx)), _) => {
                self.tree.activate_quiet(pane, idx);
                self.save(cx);
                cx.notify();
            }
            (None, Some(pane)) => {
                self.open_path_in_pane(path, pane, how, cx);
            }
            (None, None) => {
                let tab = sidebar::file_tab(path);
                let kind = telemetry::kind_of(&tab);
                if self.tree.split(doc_pane, Side::Right, tab).is_some() {
                    telemetry::pane_open(kind, how);
                    // A split focuses its new pane; the keys go back to the document.
                    self.tree.focus(focused);
                }
                self.save(cx);
                cx.notify();
            }
        }
    }

    /// A web tab on `url` (empty: a blank tab) in `pane`, or in the focused pane when that one is
    /// gone; it becomes the pane's active tab and the pane takes the focus.
    pub fn new_browser_tab(&mut self, pane: PaneId, url: String, cx: &mut Context<Self>) {
        let pane = if self.tree.pane(pane).is_some() { pane } else { self.tree.focused };
        let idx = self.tree.add_tab(pane, Tab::Browser { url });
        telemetry::pane_open(telemetry::PaneKind::Browser, telemetry::Opened::Click);
        self.tree.focus(pane);
        // Saves, and scrolls the new tab into view.
        self.activate_tab(pane, idx, cx);
    }

    /// Opens the URL prompt over the focused pane, asking for what `kind` says. `target` is the
    /// waiting browser tab to point at the URL; `None` opens a new tab. Already open for the same
    /// kind, it keeps what was typed.
    pub fn open_url_prompt(&mut self, kind: pane_view::PromptKind, target: Option<TabId>, cx: &mut Context<Self>) {
        if self.url_prompt.value().map(|p| p.kind) != Some(kind) {
            self.url_prompt.show(pane_view::UrlPrompt::new(kind, target), Instant::now());
        }
        self.sidebar_focused = false;
        self.wt_focused = false;
        cx.notify();
    }

    /// Double-click on a tab's title: the title becomes an editor holding the current name (or
    /// nothing, when the tab still shows its automatic title).
    pub fn begin_tab_rename(&mut self, pane: PaneId, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tree.tab_id(pane, idx) else { return };
        if self.tree.pane(pane).is_some_and(|p| matches!(p.tabs.get(p.active), Some(Tab::Browser { .. }))) {
            return;
        }
        let mut edit = crate::line_edit::LineEdit::new();
        edit.set(self.tree.tab_name(tab).unwrap_or_default());
        self.tab_rename = Some(pane_view::TabRename { pane, tab, edit });
        window.focus(&self.tab_rename_focus);
        cx.notify();
    }

    /// A key in the rename editor: Enter keeps the name, Escape drops the edit, Cmd+V pastes.
    pub fn tab_rename_key(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) {
        let Some(r) = self.tab_rename.as_mut() else { return };
        if ks.modifiers.platform && ks.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                r.edit.insert_str(&text);
                cx.notify();
            }
            return;
        }
        match r.edit.handle(ks) {
            crate::line_edit::Edit::Submit | crate::line_edit::Edit::SubmitBack => self.commit_tab_rename(cx),
            crate::line_edit::Edit::Cancel => {
                self.tab_rename = None;
                cx.notify();
            }
            crate::line_edit::Edit::Changed => cx.notify(),
            _ => {}
        }
    }

    /// Enter, or a press elsewhere: the typed name is kept and saved with the layout; a blank
    /// one gives the tab its automatic title back.
    pub fn commit_tab_rename(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.tab_rename.take() else { return };
        if self.tree.locate(r.tab).is_none() {
            cx.notify();
            return;
        }
        self.tree.set_tab_name(r.tab, Some(r.edit.text.clone()));
        self.save(cx);
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
        let (target, via) = (p.target, p.via);
        match p.key(ks, &configured) {
            pane_view::UrlStep::OpenPath(typed) => {
                let _via = telemetry::resumed_via(via);
                self.url_prompt.hide(Instant::now());
                let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
                if let Some(path) = pane_view::prompt_path(&typed, &self.project, &home) {
                    self.open_file_in_pane(&path, cx);
                }
            }
            pane_view::UrlStep::Stay => {}
            pane_view::UrlStep::Cancel => {
                self.url_prompt.hide(Instant::now());
            }
            pane_view::UrlStep::Open(url) => {
                let _via = telemetry::resumed_via(via);
                self.url_prompt.hide(Instant::now());
                let url = url.unwrap_or_default();
                match target.and_then(|t| self.tree.locate(t)) {
                    Some((pane, idx)) => {
                        self.tree.replace_tab(pane, idx, Tab::Browser { url });
                        self.tree.focus(pane);
                        self.activate_tab(pane, idx, cx);
                    }
                    None => self.new_browser_tab(self.tree.focused, url, cx),
                }
            }
        }
        cx.notify();
    }

    /// Where a browser tab's placeholder was drawn this frame, from its `canvas`.
    pub fn report_browser_rect(&mut self, tab: TabId, bounds: Bounds<gpui::Pixels>) {
        self.browser_rects.insert(tab, (bounds, self.frame_now));
    }

    /// Splits the focused pane and starts a fresh shell in the new one.
    pub fn split_focused(&mut self, side: Side, cx: &mut Context<Self>) {
        let pane = self.tree.focused;
        if let Some(new_pane) = self.tree.split(pane, side, Tab::Terminal { session: SessionId(0), cwd: None }) {
            telemetry::pane_open(telemetry::PaneKind::Terminal, telemetry::Opened::Click);
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
                // pinned, its file read again; a web tab comes back on its page.
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
                    Closed::Browser { url } => {
                        self.new_browser_tab(pane, url, cx);
                    }
                    video @ Closed::Video { .. } => {
                        let Some(tab) = reopened_video(video) else { return };
                        let idx = self.tree.add_tab(pane, tab);
                        telemetry::pane_open(telemetry::PaneKind::Video, telemetry::Opened::Key);
                        self.tree.focus(pane);
                        self.activate_tab(pane, idx, cx);
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
            // In a web tab the page zooms a step instead (web pane spec, section 4).
            FontDelta(d) if self.active_web().is_some() => self.with_active_web(cx, move |v, cx| v.zoom(if d > 0.0 { 1 } else { -1 }, cx)),
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
                // The Settings window's Text section shows the preset this window uses.
                if self.apply_text_preset(&name, cx) {
                    aux::push_settings(cx, self.settings_shown());
                }
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
            CopyAsShown => {
                // The selection's rows as they are on screen: Cmd+C's re-flow is for prose.
                if let Some(text) = self.active_terminal(cx).and_then(|t| t.read(cx).core.selection_text()) {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                }
            }
            SetTheme(name) => {
                telemetry::theme_named(&name);
                self.switch_theme_to(theme::load(&name, &paths::themes_dir()), cx)
            }
            UpdateNow | CheckForUpdates | RetryUpdate if self.sandbox => {}
            UpdateNow => self.update_now(cx),
            CheckForUpdates => {
                self.sessions.check_for_updates();
                self.show_notice("checking for updates".into(), cx);
            }
            Notifications => self.toggle_bell(cx),
            Doctor => self.toggle_doctor(cx),
            RetryUpdate => {
                self.update_error = None;
                self.refresh_update_badge(cx);
                self.sessions.check_for_updates();
                // A release still staged gets another go once the window is idle.
                self.start_idle_timer(cx);
            }
            StartAtLogin => self.toggle_start_at_login(cx),
            InstallHooks => self.install_hooks(cx),
            InstallAgentGuide => self.install_agent_guide(cx),
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
            ToggleMarkdownView => {
                if let Some(t) = self.active_viewer_tab() {
                    self.toggle_markdown_view_for(t, cx);
                }
            }
            // Cmd+S reaches the keymap before the viewer's key handler, so it does what the
            // handler's Save does: during a conflict it keeps mine.
            SaveFile => {
                if let Some(v) = self.active_viewer() {
                    v.update(cx, |v, cx| v.save_command(cx));
                }
            }
            Bold | Italic => {
                if let Some(v) = self.active_viewer() {
                    let format = if matches!(cmd, Bold) { md_format::Format::Bold } else { md_format::Format::Italic };
                    v.update(cx, |v, cx| v.format(format, cx));
                }
            }
            OpenFolder => self.pick_folder(cx),
            NewWindow => self.new_window(cx),
            About => aux::open_about(cx),
            OpenSettings => aux::open_settings(cx),
            Keybindings => aux::open_keybindings(cx),
            Documentation => aux::open_documentation(cx),
            NewBrowserTab => self.open_url_prompt(pane_view::PromptKind::Url, None, cx),
            OpenFileInPane => self.open_url_prompt(pane_view::PromptKind::Path, None, cx),
            VideoPlayPause => self.with_active_video(cx, |v, cx| v.play_pause(cx)),
            VideoBack => self.with_active_video(cx, |v, cx| v.back(video_view::SKIP, cx)),
            VideoForward => self.with_active_video(cx, |v, cx| v.forward(video_view::SKIP, cx)),
            VideoBackFine => self.with_active_video(cx, |v, cx| v.back(video_view::SKIP_FINE, cx)),
            VideoForwardFine => self.with_active_video(cx, |v, cx| v.forward(video_view::SKIP_FINE, cx)),
            VideoStepBack => self.with_active_video(cx, |v, cx| v.step(-1, cx)),
            VideoStepForward => self.with_active_video(cx, |v, cx| v.step(1, cx)),
            VideoStart => self.with_active_video(cx, |v, cx| v.to_start(cx)),
            VideoLoop => self.with_active_video(cx, |v, cx| v.toggle_loop(cx)),
            VideoMute => self.with_active_video(cx, |v, cx| v.mute(cx)),
            VideoReload => self.with_active_video(cx, |v, cx| v.reload_now(cx)),
            PageBack => self.with_active_web(cx, |v, cx| v.back(cx)),
            PageForward => self.with_active_web(cx, |v, cx| v.forward(cx)),
            ReloadPage => self.with_active_web(cx, |v, cx| v.reload(cx)),
            OpenInBrowser => self.with_active_web(cx, |v, cx| v.open_outside(cx)),
            ZoomReset => self.with_active_web(cx, |v, cx| v.zoom(0, cx)),
            FocusAddress => {
                if let Some(v) = self.active_web() {
                    // The page may hold the keys: they come back to gpui before the field takes them.
                    self.keys_back_to_arkos();
                    let focus = v.read(cx).address_focus.clone();
                    window.focus(&focus);
                    v.update(cx, |v, cx| v.begin_edit(cx));
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

    /// "New Window" (the Arkos IDE menu, Cmd+N): the chooser in its own window, with the recent
    /// folders and the native picker; the folder chosen opens in a new window. One at a time:
    /// asking again brings the open one forward.
    fn new_window(&mut self, cx: &mut Context<Self>) {
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
        let open = Box::new(move |folder: PathBuf, _link, cx: &mut App| {
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
        launch_agent::plist(&paths::daemon_exe(), &daemon_args(&self.config, self.sandbox), &client::daemon_log(&paths::socket_path()))
    }

    /// Installs or removes the login item off the UI thread and says what happened. Only the
    /// app in `/Applications` (or `~/Applications`) writes it: a copy run from anywhere else
    /// leaves the file as it is, and says so when the user asked (`asked`). A sandbox never writes
    /// it and marks the doctor's row `not in a sandbox` when asked.
    fn set_start_at_login(&mut self, on: bool, asked: bool, cx: &mut Context<Self>) {
        if self.refused_in_sandbox(doctor_panel::CheckId::StartAtLogin) {
            if asked {
                self.mark_doctor_broken(doctor_panel::CheckId::StartAtLogin, sandbox::REFUSAL.into());
            }
            return;
        }
        if on && !launch_agent::may_install_here() {
            if self.doctor_busy == Some(doctor_panel::CheckId::StartAtLogin) {
                self.doctor_busy = None;
            }
            if asked {
                let text = "Start at login needs the app in /Applications or ~/Applications";
                self.show_notice(text.into(), cx);
                self.mark_doctor_broken(doctor_panel::CheckId::StartAtLogin, text.into());
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
            let (text, error) = match (on, result) {
                (_, Ok(false)) => (None, None),
                (true, Ok(true)) => (Some("start at login is on: after a restart, sessions come back on their own".to_string()), None),
                (false, Ok(true)) => (Some("start at login is off".to_string()), None),
                (_, Err(e)) => (Some(format!("start at login: {e}")), Some(e.to_string())),
            };
            let _ = root.update(cx, |root, cx| {
                if root.doctor_busy == Some(doctor_panel::CheckId::StartAtLogin) {
                    root.doctor_busy = None;
                }
                if let Some(text) = text {
                    root.show_notice(text, cx);
                }
                match error {
                    Some(e) => root.mark_doctor_broken(doctor_panel::CheckId::StartAtLogin, e),
                    None => root.probe_doctor(),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The palette's "Start at login": flips the login item while the config says "ask"; when
    /// the config pins it, opens the config instead. A sandbox does neither and marks the doctor's
    /// row `not in a sandbox`.
    pub fn toggle_start_at_login(&mut self, cx: &mut Context<Self>) {
        if self.refused_in_sandbox(doctor_panel::CheckId::StartAtLogin) {
            self.mark_doctor_broken(doctor_panel::CheckId::StartAtLogin, sandbox::REFUSAL.into());
            return;
        }
        match self.config.recovery.start_at_login {
            config::StartAtLogin::Ask => {
                let on = !launch_agent::installed(&launch_agent::agent_path());
                self.set_start_at_login(on, true, cx);
            }
            _ => {
                self.doctor_busy = None;
                self.open_config(cx)
            }
        }
    }

    /// Keeps the login item as the config says; with "ask" and nothing installed, says once per
    /// launch how to turn it on.
    pub fn apply_recovery(&mut self, at_launch: bool, cx: &mut Context<Self>) {
        let installed = launch_agent::installed(&launch_agent::agent_path());
        match launch_agent::desired(self.config.recovery.start_at_login, installed, self.sandbox) {
            Some(on) => self.set_start_at_login(on, false, cx),
            None if at_launch && !installed && !self.sandbox => {
                self.show_notice("sessions can come back after a restart: run \"Start at login\" from the command palette (Cmd+Shift+P)".into(), cx)
            }
            None => {}
        }
    }

    /// The bell's "Update now" (or its update-ready row): apply the staged release, else update from disk.
    pub fn update_clicked(&mut self, cx: &mut Context<Self>) {
        if self.sandbox {
            return;
        }
        if self.staged.is_some() {
            self.apply_staged(cx);
        } else {
            self.update_now(cx);
        }
    }

    /// The palette's "Update now": the daemon first when its binary changed, then this window.
    pub fn update_now(&mut self, cx: &mut Context<Self>) {
        if self.sandbox {
            return;
        }
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

    /// Reads the staged release into `update_ready` (the bell's update-ready notice) when there is
    /// one, else what the launch path holds.
    pub fn refresh_update_badge(&mut self, cx: &mut Context<Self>) {
        let ready = update_check::ready_text(self.staged.as_ref(), self.update_ready_on_disk.as_deref());
        if ready != self.update_ready {
            self.update_ready = ready;
            // The Settings window's "Update now" line follows the badge.
            aux::push_settings(cx, self.settings_shown());
        }
        cx.notify();
    }

    /// What is open that an update must not pull out from under the user.
    fn chrome_state(&self, cx: &App) -> idle::Chrome {
        idle::Chrome {
            palette: self.palette.value().is_some(),
            menu: self.tab_menu.value().is_some() || self.context_menu.value().is_some() || self.wt_menu.value().is_some() || self.link_menu.value().is_some() || self.md_menu.value().is_some() || self.popover.value().is_some(),
            prompt: self.wt_prompt.value().is_some() || self.url_prompt.value().is_some(),
            find: self.active_terminal(cx).is_some_and(|t| t.read(cx).find.is_some()) || self.active_viewer().is_some_and(|v| v.read(cx).find.is_some()),
            drag: self.drop_target.is_some() || self.resizing.is_some() || self.sidebar_resizing,
            unsaved: self.viewers.values().any(|v| {
                let v = v.read(cx);
                v.conflict || v.save_error.is_some()
            }),
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
    /// once. Stops when there is nothing to do or `[update] auto` is off (as it is in a sandbox);
    /// one at a time.
    pub fn start_idle_timer(&mut self, cx: &mut Context<Self>) {
        if self.auto_apply_timer || self.sandbox {
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

    /// Asks the daemon what the requested check left, shortly: every `POLL_EVERY_MS` for a daemon
    /// that reports the check's state, once after `CHECK_RESULT_AFTER` for a protocol 4 one.
    fn poll_check_after(&mut self, cx: &mut Context<Self>) {
        let detailed = self.daemon_protocol_max >= protocol::UPDATE_STATUS_DETAILED_FROM;
        let delay = if detailed { Duration::from_millis(update_check::POLL_EVERY_MS) } else { CHECK_RESULT_AFTER };
        let root = cx.entity();
        cx.spawn(async move |_, cx| {
            cx.background_executor().timer(delay).await;
            let _ = root.update(cx, |root, _| root.sessions.status_after_check(detailed));
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
                // The daemon's own words for a release it put back (`updates::apply`), once.
                if e.contains(" rolled back: ") && self.update_error_noticed.as_ref() != Some(e) {
                    telemetry::update(telemetry::UpdateStep::RolledBack);
                }
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

    /// A batch of daemon events: the window's own (`UpdateStaged`, `Customize`, `Direct`) first,
    /// then the sessions'.
    pub fn route_events(&mut self, events: &[Event], cx: &mut Context<Self>) {
        for ev in events {
            match ev {
                Event::UpdateStaged { update } => {
                    telemetry::update(telemetry::UpdateStep::Staged);
                    self.staged = Some(update.clone());
                    self.update_error = None;
                    // The manifest was reached.
                    self.update_unreachable = None;
                    self.refresh_update_badge(cx);
                    self.start_idle_timer(cx);
                }
                Event::Customize { id, action } => self.customize(*id, action.clone(), cx),
                Event::Direct { id, action, session } => self.handle_direct(*id, action.clone(), *session, cx),
                _ => {}
            }
        }
        if self.sessions.route_events(events, cx) {
            cx.notify();
        }
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
    /// them later), unsaved markdown edits are written, and the pending layout save is written.
    /// Edits that cannot be saved (a conflict, a failed write) go to the recovery folder instead,
    /// so no quit drops them. `goodbye` is whether a Goodbye was queued just before, which the
    /// same wait covers. A quit with no Goodbye at all (macOS's terminate, the update relaunch)
    /// says `SessionQuit` after the final mark instead, once, so the daemon seals the session as
    /// a quit; the same wait covers it.
    pub fn flush_for_quit(&mut self, goodbye: bool, cx: &mut App) {
        let quit_said = !goodbye && !self.said_goodbye && !std::mem::replace(&mut self.said_quit, true);
        if quit_said {
            telemetry::quit_without_goodbye(self.sessions.client.as_deref());
        } else {
            telemetry::quitting();
        }
        for (_, e) in self.save_dirty_viewers_now(cx) {
            eprintln!("arkos: quitting with unsaved edits: {e}");
        }
        self.recover_unsaved(cx);
        let killed = self.end_closed_tabs();
        // The process may exit before the writer thread sends what is queued; wait for it, once
        // and briefly. The client is the current connection's, whichever reconnect that is.
        if (killed > 0 || goodbye || quit_said) && !self.sessions.client.as_ref().is_some_and(|c| c.wait_sent(Duration::from_millis(500))) {
            let also = if goodbye {
                " and the goodbye"
            } else if quit_said {
                " and the session's end"
            } else {
                ""
            };
            eprintln!("arkos: could not send {killed} kill requests{also} before quitting");
        }
        workspace::flush_now(&self.save_queue);
    }

    /// The user asked to quit, or to close the window: every markdown tab's unsaved edits are
    /// written first. When some cannot be (a conflict, a failed save) the window stays, the first
    /// such tab comes to the front and a notice says why; `false` then. Asking again within
    /// `FORCE_QUIT_WITHIN` goes ahead, and the quit's flush puts those edits in the recovery folder.
    pub fn may_quit(&mut self, cx: &mut Context<Self>) -> bool {
        let now = Instant::now();
        let errors = self.save_dirty_viewers_now(cx);
        let Some((tab, problem)) = errors.first().cloned() else { return true };
        if quit_forced(self.last_quit_attempt, now) {
            return true;
        }
        self.last_quit_attempt = Some(now);
        if let Some((pane, idx)) = self.tree.locate(tab) {
            self.focus_pane(pane, cx);
            self.activate_tab(pane, idx, cx);
        }
        let more = match errors.len() {
            1 => String::new(),
            n => format!(" (and {} more)", n - 1),
        };
        self.show_notice(format!("{problem}{more}; press Cmd+Q again to quit without saving"), cx);
        false
    }

    /// The user is quitting: tell the daemon not to reopen this window after a restart, then
    /// the flush every quit does. Not for the relaunch an update makes (`RelaunchAccepted`) nor
    /// macOS's terminate (`says_goodbye`). Says it once.
    pub fn leaving(&mut self, cx: &mut App) {
        if self.said_goodbye {
            return;
        }
        self.said_goodbye = true;
        telemetry::quitting();
        self.sessions.goodbye();
        self.flush_for_quit(true, cx);
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
        // The daemon still runs the check; its result shows in the reconnect's status.
        if self.check_poll.take().is_some() {
            self.show_notice("the update check is still running".into(), cx);
        }
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
                self.daemon_protocol_max = protocol_max;
                // An older daemon answers an unknown request with an error: ask only protocol 4.
                if protocol_max >= protocol::UPDATE_STATUS_FROM {
                    self.sessions.update_status();
                }
                // Another window's update may have upgraded the daemon: relaunch once idle.
                self.start_idle_timer(cx);
            }
            Outcome::UpdateStatus { staged, last_error, unreachable_for_secs } => self.note_update_status(staged, last_error, unreachable_for_secs, false, cx),
            Outcome::CheckStarted => {
                // A second click while a check polls: the running chain answers for both.
                if update_check::Poll::begin(&mut self.check_poll, recent::now_unix()) {
                    self.poll_check_after(cx);
                }
            }
            Outcome::Checked(answer) => {
                let step = update_check::step(self.check_poll.as_mut(), &answer);
                // Only the answer that ends the check notices an error the user already saw.
                self.note_update_status(answer.staged.clone(), answer.last_error.clone(), answer.unreachable_for_secs, step.requested(), cx);
                match step {
                    update_check::Step::Again => self.poll_check_after(cx),
                    update_check::Step::Done(verdict) => {
                        self.check_poll = None;
                        if let Some(n) = update_check::notice(&verdict) {
                            self.show_notice(n, cx);
                        }
                    }
                    update_check::Step::NoPoll => {}
                }
            }
            Outcome::Applied => {
                telemetry::update(telemetry::UpdateStep::Applied);
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
                telemetry::update(telemetry::UpdateStep::Failed);
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
            Outcome::GraceKept { session } => self.closed.mark_daemon_timed(session),
            Outcome::AttachFailed { session, tab, error } => {
                self.restart_tab(tab, session, &format!("arkosd could not attach it: {error}"), cx);
            }
            Outcome::UpgradeFailed(e) => {
                // The old daemon keeps running and serving; nothing else changes.
                eprintln!("arkos: update failed: {e}");
                telemetry::update(telemetry::UpdateStep::Failed);
                self.updating = None;
                self.show_notice(format!("update failed: {e}"), cx);
            }
            Outcome::RelaunchAccepted => {
                // No Goodbye: the daemon reopens this project after a rollback only while the
                // folder is still in its window list.
                self.flush_for_quit(false, cx);
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

/// What the closed stack keeps for a closed tab: the file, the page or the video with its place.
/// A terminal's session is `close_tab`'s own (`None` here).
fn closed_entry(tab: Tab) -> Option<Closed> {
    match tab {
        Tab::Viewer { path, .. } => Some(Closed::Viewer(path)),
        Tab::Browser { url } => Some(Closed::Browser { url }),
        Tab::Video { path, at, loop_range } => Some(Closed::Video { path, at, loop_range }),
        Tab::Terminal { .. } => None,
    }
}

/// The tab a reopened video comes back as: its file, where it was and its loop. Anything else
/// reopens its own way.
fn reopened_video(closed: Closed) -> Option<Tab> {
    match closed {
        Closed::Video { path, at, loop_range } => Some(Tab::Video { path, at, loop_range }),
        Closed::Session(..) | Closed::Viewer(_) | Closed::Browser { .. } => None,
    }
}

/// Whether the video tab `tab` plays (spec 3.5): it is its pane's active tab, the pane is on
/// screen (`shown`: not hidden by another's maximize) and the window is not hidden or minimized.
impl Root {
    /// A web tab's view, made the first time the tab is shown: it loads the tab's page. Its
    /// navigations are written into the tab and saved; its title feeds the tab label.
    pub fn web_for(&mut self, tab: TabId, url: &str, cx: &mut Context<Self>) -> Entity<web_view::WebView> {
        if let Some(v) = self.webs.get(&tab) {
            return v.clone();
        }
        let web: Box<dyn web::Web> = match (self.ns_window.as_ref(), self.ns_view.as_ref()) {
            (Some(ns), Some(view)) => match wk_web_view::Host::new(ns, view) {
                Some(host) => Box::new(host),
                None => Box::new(web::NoWeb),
            },
            _ => Box::new(web::NoWeb),
        };
        let project = self.project.clone();
        let under = self.theme.chrome.window_bg;
        let v = cx.new(|cx| {
            let mut v = web_view::WebView::new(url.to_string(), project, web, cx);
            v.set_under_color((under.r, under.g, under.b));
            v
        });
        cx.subscribe(&v, move |root, v, event: &web_view::WebEvent, cx| {
            if root.webs.get(&tab).map(|own| own.entity_id()) != Some(v.entity_id()) {
                return;
            }
            match event {
                web_view::WebEvent::Save(url) => {
                    if root.tree.set_browser_url(tab, url) {
                        root.save(cx);
                    }
                }
                web_view::WebEvent::Title(title) => {
                    root.web_titles.insert(tab, title.clone());
                    cx.notify();
                }
            }
        })
        .detach();
        self.webs.insert(tab, v.clone());
        v
    }

    /// The focused pane's web tab, when its active tab is one.
    pub fn active_web(&self) -> Option<Entity<web_view::WebView>> {
        let p = self.tree.pane(self.tree.focused)?;
        match p.tabs.get(p.active)? {
            Tab::Browser { .. } => self.webs.get(p.ids.get(p.active)?).cloned(),
            Tab::Terminal { .. } | Tab::Viewer { .. } | Tab::Video { .. } => None,
        }
    }

    /// Repaints at `at` for a cursor's blink, unless a repaint already comes sooner.
    fn schedule_blink(&mut self, at: Instant, cx: &mut Context<Self>) {
        let now = Instant::now();
        if self.caret_wake.is_some_and(|w| w > now && w <= at) {
            return;
        }
        self.caret_wake = Some(at);
        cx.spawn(async move |root, cx| {
            cx.background_executor().timer(at.saturating_duration_since(Instant::now())).await;
            let _ = root.update(cx, |root, cx| {
                if root.caret_wake == Some(at) {
                    root.caret_wake = None;
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Runs a web command on the focused pane's web tab; nothing when it shows something else.
    fn with_active_web(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut web_view::WebView, &mut Context<web_view::WebView>)) {
        if let Some(v) = self.active_web() {
            v.update(cx, f);
        }
    }

    /// Something of Arkos is drawn over the panes, so no web view may show (web pane spec,
    /// section 3.2). A new overlay adds itself here, as a new animation adds itself to
    /// `any_motion`.
    pub fn covered(&self) -> bool {
        self.palette.value().is_some()
            || self.url_prompt.value().is_some()
            || self.tab_menu.value().is_some()
            || self.popover.value().is_some()
            || self.context_menu.value().is_some()
            || self.link_menu.value().is_some()
            || self.md_menu.value().is_some()
            || self.wt_prompt.value().is_some()
            || self.wt_menu.value().is_some()
            || self.drop_target.is_some()
            || self.reorder.is_some()
            || self.picking_folder
    }

    /// Where a web tab's content was laid out this frame, from its `canvas`: the view goes over
    /// it, under the bar, unless an overlay covers the panes, the window is hidden or the tab
    /// shows one of Arkos's own screens.
    pub fn place_web(&mut self, tab: TabId, bounds: Bounds<gpui::Pixels>, cx: &mut Context<Self>) {
        let Some(v) = self.webs.get(&tab).cloned() else { return };
        let hidden = self.ns_window.as_ref().is_some_and(|ns| screen::window_hidden(ns));
        let frame = if self.covered() || hidden || !v.read(cx).shows_page() {
            None
        } else {
            self.ns_window.as_ref().map(|ns| {
                let (h, flipped) = screen::content_geometry(ns);
                // Inset by the focus glow's reach on the left, right and bottom, so the pane's
                // glow stays visible around the page (the bar is above it).
                let (w, hh) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                let g = pane_view::glow_width(w, hh, 1.0);
                let b = (f64::from(f32::from(bounds.origin.x) + g), f64::from(f32::from(bounds.origin.y)), f64::from((w - 2.0 * g).max(0.0)), f64::from((hh - g).max(0.0)));
                screen::appkit_frame(b, 0.0, h, flipped)
            })
        };
        v.update(cx, |v, _| v.place(frame));
    }

    /// At the top of every render: a web view whose tab is not on screen (not the active tab of
    /// a shown pane, an overlay open, the window hidden) is hidden now; the ones on screen are
    /// placed by their canvas as it paints. A page that took the keyboard with a click (gpui saw
    /// no press) makes its pane the focused one.
    pub fn update_web_visibility(&mut self, cx: &mut Context<Self>) {
        if self.webs.is_empty() {
            return;
        }
        let shown: Vec<PaneId> = self.layout().panes.into_iter().map(|(p, _)| p).collect();
        let hidden = self.ns_window.as_ref().is_some_and(|ns| screen::window_hidden(ns));
        let covered = self.covered();
        let mut focus = None;
        for (tab, v) in &self.webs {
            let on_screen = video_plays(&self.tree, &shown, *tab, hidden) && !covered && v.read(cx).shows_page();
            if !on_screen {
                v.update(cx, |v, _| v.place(None));
            } else if v.read(cx).has_keys() {
                if let Some((pane, _)) = self.tree.locate(*tab) {
                    if self.tree.focused != pane {
                        focus = Some(pane);
                    }
                }
            }
        }
        if let Some(pane) = focus {
            self.sidebar_focused = false;
            self.wt_focused = false;
            self.tree.focus(pane);
        }
    }

    /// The style the app paints its surfaces with this frame: plain without an effect, the
    /// manifest's with one, and a blend of the two while it fades.
    pub fn effect_style(&self) -> effects::active::Style {
        self.effect.as_ref().map_or(effects::active::Style::plain(), |e| e.style(self.frame_now))
    }

    /// Whether the effect paints the highlights (its manifest's `highlights = "effect"`, once it
    /// has faded in), so the Explorer rows and tabs report marks instead of the app filling them.
    pub fn effect_highlights(&self) -> bool {
        self.effect_style().highlights
    }

    /// A UI text size of `pt` points as the project window draws it under its effect.
    pub fn ui_size(&self, pt: f32) -> f32 {
        self.effect_style().ui(pt)
    }

    /// Puts on the effect the config and the theme name, or takes it off.
    pub fn resolve_effect(&mut self, cx: &mut Context<Self>) {
        let name = effects::resolve(&self.config.effect.name, &self.base_theme);
        self.set_effect(name, cx);
    }

    /// Puts on the effect `name` with a fade in, or with `None` starts the current one's fade
    /// out, at whose end it is dropped (`step_effect_fade`). Both cut under Reduce Motion or
    /// before the first frame. An effect that is missing or whose shader does not compile is a
    /// notice, and the window stays plain.
    pub fn set_effect(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        let now = Instant::now();
        let cut = motion::reduce_motion() || !self.painted;
        let Some(name) = name else {
            if self.effect.as_ref().is_some_and(|e| !e.leaving()) {
                if cut {
                    self.effect = None;
                } else if let Some(e) = self.effect.as_mut() {
                    e.leave(now);
                }
                self.effect_fade_settled(cx);
                cx.notify();
            }
            return;
        };
        if let Some(e) = self.effect.as_mut().filter(|e| e.effect.manifest.name == name) {
            if e.leaving() {
                e.come_back(now);
                cx.notify();
            }
            return;
        }
        // Another effect goes at once, its glyphs and text with it; the new one fades in from
        // plain.
        if self.effect.take().is_some() {
            self.effect_fade_settled(cx);
        }
        let (Some(ns), Some(view)) = (self.ns_window.clone(), self.ns_view.clone()) else { return };
        let effect = match effects::manifest::load(&name, &effects::effects_dir()) {
            Ok(e) => e,
            Err(e) => {
                self.show_notice(format!("effect {name}: {e}"), cx);
                return;
            }
        };
        match effects::active::start(effect, &mut WindowInstaller { ns, view }, now, cut) {
            Ok(mut active) => {
                // Put on while the window is paused, it holds its first frame until focus.
                active.set_paused(self.effects_paused, now);
                self.effect = Some(active);
                if cut {
                    self.effect_fade_settled(cx);
                }
                self.effect_tick(now);
                self.ensure_effect_timer(cx);
            }
            Err(notice) => self.show_notice(notice, cx),
        }
        cx.notify();
    }

    /// What can only cut switches here: at the end of a fade in and at the start of a fade
    /// out (the text size follows `effect_style` by itself). The project window's glyph shader
    /// is set or cleared at its next frame, and its theme fades to the configured one with the
    /// text deepened by the effect's `text_contrast`, or back.
    fn effect_fade_settled(&mut self, cx: &mut Context<Self>) {
        self.glyphs_pending = Some(self.glyph_shader_for(effects::active::WindowKind::Project));
        let shown = effects::active::contrasted(&self.base_theme, self.text_contrast());
        self.fade_theme_to(shown, cx);
        cx.notify();
    }

    /// The glyph shader a window of `kind` draws with: the effect's, in the project window
    /// only, told the configured theme's focus ring as its accent. The aux windows never ask,
    /// so they keep gpui's own glyphs.
    pub fn glyph_shader_for(&self, kind: effects::active::WindowKind) -> Option<gpui::GlyphShader> {
        effects::active::glyph_shader_for(kind, self.effect.as_ref(), self.glyph_scale, self.base_theme.chrome.focus_ring)
    }

    /// How far the text is deepened: the effect's `text_contrast` while it is settled on, else
    /// nothing.
    fn text_contrast(&self) -> f32 {
        self.effect.as_ref().filter(|e| e.settled()).map_or(0.0, |e| e.effect.manifest.text_contrast)
    }

    /// Ends an effect's fade that is complete at `now`: at a fade in's end what can only cut
    /// switches; at a fade out's end the effect is dropped, and `render` makes the window opaque
    /// again. Called by `render` and by the wash's timer, since gpui draws no frames for a window
    /// macOS has hidden behind others, and a fade must end all the same (within the timer's
    /// rest while the window cannot be seen).
    fn step_effect_fade(&mut self, now: Instant, cx: &mut Context<Self>) {
        let Some(e) = self.effect.as_mut() else { return };
        match e.step(now) {
            Some(effects::active::Direction::In) => self.effect_fade_settled(cx),
            Some(effects::active::Direction::Out) => self.effect = None,
            None => return,
        }
        cx.notify();
    }

    /// Starts the wash's timer unless it keeps its cadence already: a frame at the manifest's
    /// rate (sixty a second while a highlight fades) until the effect is gone. A timer that
    /// rests is replaced, so its first tick comes now rather than at the end of its rest. Called
    /// by `render`, which runs on every change of what the wash reads, so a timer stopped under
    /// Reduce Motion or while the window is paused out of focus ticks once after each such
    /// frame, and runs on only while something moves.
    fn ensure_effect_timer(&mut self, cx: &mut Context<Self>) {
        if self.effect.is_none() || self.effect_timer == Some(false) {
            return;
        }
        self.effect_timer_gen += 1;
        let generation = self.effect_timer_gen;
        self.effect_timer = Some(false);
        cx.spawn(async move |root, cx| loop {
            let wait = root.update(cx, |root, cx| {
                if root.effect_timer_gen != generation {
                    return None;
                }
                let now = Instant::now();
                root.step_effect_fade(now, cx);
                let seen = root.effect_tick(now);
                // The open menus repaint with the wash, which a paused window holds still.
                if seen.is_some() && !root.effect_menus.is_empty() && !root.effects_paused {
                    cx.notify();
                }
                // The text's own timer stops while the window cannot be seen and only `render`
                // restarts it; uncovering the window brings no render by itself, so this tick does.
                if seen.is_some() {
                    root.ensure_glyph_timer(cx);
                }
                let paused = root.effects_paused;
                let wait = root.effect.as_ref().and_then(|e| {
                    effects::active::next_wait(e.effect.manifest.frame_rate, seen.is_some(), seen == Some(true), !e.settled(), motion::reduce_motion(), paused)
                });
                root.effect_timer = wait.map(|_| seen.is_none());
                wait
            });
            match wait {
                Ok(Some(d)) => cx.background_executor().timer(d).await,
                _ => break,
            }
        })
        .detach();
    }

    /// What the text's timer waits before the next frame, or `None` when the text need not be
    /// redrawn: see `effects::active::glyph_wait`.
    fn glyph_wait(&self) -> Option<std::time::Duration> {
        let e = self.effect.as_ref()?;
        // As the wash, the text moves on with a menu or a prompt open; an unseen window rests it,
        // and so does one paused while it is not in focus (`effects_paused`).
        let seen = !self.ns_window.as_ref().is_some_and(|ns| screen::window_unseen(ns));
        effects::active::glyph_wait(e.effect.manifest.glyph_rate(), e.effect.manifest.glyph_motion, e.settled(), seen, motion::reduce_motion(), self.effects_paused)
    }

    /// Starts the text's timer unless it runs: while the effect's glyphs move it asks for a
    /// frame (`cx.notify()`) after each wait, so the text is redrawn at the glyph rate. It
    /// stops itself once `glyph_wait` is `None`; `render` starts it again when needed.
    fn ensure_glyph_timer(&mut self, cx: &mut Context<Self>) {
        if self.glyph_timer || self.glyph_wait().is_none() {
            return;
        }
        self.glyph_timer_gen += 1;
        let generation = self.glyph_timer_gen;
        self.glyph_timer = true;
        cx.spawn(async move |root, cx| loop {
            let Ok(wait) = root.update(cx, |root, _| {
                if root.glyph_timer_gen != generation {
                    return None;
                }
                let wait = root.glyph_wait();
                root.glyph_timer = wait.is_some();
                wait
            }) else {
                break;
            };
            let Some(d) = wait else { break };
            cx.background_executor().timer(d).await;
            let more = root.update(cx, |root, cx| {
                if root.glyph_timer_gen == generation {
                    cx.notify();
                }
            });
            if more.is_err() {
                break;
            }
        })
        .detach();
    }

    /// One frame of the wash at `now`: the blocks from the current layout, the divider and the
    /// highlights' strokes, with the theme as it is now (a theme fade reads the blend). Nothing
    /// is built or drawn while the window is hidden, occluded or covered, and `None` says so;
    /// under Reduce Motion, or while the window is paused out of focus (`effects_paused`, whose
    /// clock holds), nothing is drawn when nothing changed. `Some(true)` while a highlight's
    /// stroke fades, so the next frame comes sooner (the surfaces' own fade is gpui's frames,
    /// through `any_motion`).
    pub fn effect_tick(&mut self, now: Instant) -> Option<bool> {
        self.effect.as_ref()?;
        let hidden = self.ns_window.as_ref().is_some_and(|ns| screen::window_unseen(ns));
        let reduce = motion::reduce_motion();
        let paused = self.effects_paused;
        // A window that cannot be seen rests the wash. With a menu or a prompt open the user is
        // still at work in it; a window out of focus holds still only when the setting says so.
        if hidden {
            return None;
        }
        let layout = self.layout();
        let (strokes, strokes_moving) = self.effect_strokes.sync(&self.effect_marks, now);
        // The panes whose Claude session works: an effect may show them busy.
        let busy = self.sessions.working_panes(&self.tree);
        let e = self.effect.as_mut()?;
        let style = e.style(now);
        let scene = effects::active::scene(self.content, self.effect_divider, &layout.panes, &busy, &self.effect_shapes, &strokes);
        self.effect_strokes_now = strokes.clone();
        let blocks = effects::blocks::blocks(&scene, style.ink_borders);
        let (size, scale) = e.layer.metrics();
        let inputs = effects::inputs::build(&self.theme, self.config.effect.intensity, size, scale, e.time(now, reduce), &blocks);
        if e.should_draw(&inputs, hidden, reduce, paused) {
            e.layer.draw(&inputs);
            e.last_inputs = Some(inputs);
            if let Some(ms) = e.layer.gpu_ms().filter(|ms| effects::active::should_log(e.logged, now, *ms)) {
                eprintln!("arkos: effect {}: {ms:.2} ms per frame at {}x{}", e.effect.manifest.name, size.0.round(), size.1.round());
                e.logged = Some((now, ms));
            }
        }
        // The menu stage: the open menus' edges above the panes, or its layer hidden.
        let menus_fading = self.effect_menus.iter().any(|m| m.opacity < 1.0);
        if style.menu_edges && !self.effect_menus.is_empty() {
            let blocks = effects::blocks::menu_blocks(&self.effect_menus);
            let overlay = effects::inputs::build(&self.theme, self.config.effect.intensity, size, scale, e.time(now, reduce), &blocks);
            if !((reduce || paused) && e.last_overlay.as_ref() == Some(&overlay)) {
                e.layer.draw_overlay(Some(&overlay));
                e.last_overlay = Some(overlay);
            }
        } else if e.last_overlay.take().is_some() {
            e.layer.draw_overlay(None);
        }
        let open: Vec<u64> = self.effect_menus.iter().map(|m| m.key).collect();
        e.layer.keep_backdrops(&open);
        Some(strokes_moving || menus_fading)
    }

    /// The backdrop of the open menu `key` at `area`, faded to `opacity`: the effect's wash over
    /// the menu's place with the highlight strokes inside it, for the menu to show behind its
    /// text. `None` unless the effect paints the menus. A paused window's clock holds, so the
    /// backdrop holds with the wash.
    pub fn menu_backdrop(&mut self, key: u64, area: Rect, opacity: f32) -> Option<core_video::pixel_buffer::CVPixelBuffer> {
        let reduce = motion::reduce_motion();
        let now = Instant::now();
        let intensity = self.config.effect.intensity;
        let e = self.effect.as_mut()?;
        if !e.style(now).menu_edges {
            return None;
        }
        let menu = effects::blocks::MenuShape { key, rect: area, opacity };
        let blocks = effects::blocks::backdrop_blocks(&menu, &self.effect_strokes_now);
        let (size, scale) = e.layer.metrics();
        let inputs = effects::inputs::build(&self.theme, intensity, size, scale, e.time(now, reduce), &blocks);
        e.layer.backdrop(key, area, opacity, &inputs)
    }

    /// The keys come back to gpui from a web view: before a field of Arkos's own takes them.
    pub fn keys_back_to_arkos(&self) {
        if let (Some(ns), Some(view)) = (&self.ns_window, &self.ns_view) {
            screen::make_first_responder(ns, view);
        }
    }

}

/// Puts an effect's wash under gpui's view in the project window.
struct WindowInstaller {
    ns: objc2::rc::Retained<objc2_app_kit::NSWindow>,
    view: objc2::rc::Retained<objc2_app_kit::NSView>,
}

impl effects::active::Installer for WindowInstaller {
    type Layer = effects::layer::Layer;
    fn install(&mut self, effect: &effects::manifest::Effect) -> Result<effects::layer::Layer, String> {
        effects::layer::Layer::install(&self.ns, &self.view, effects::PREAMBLE, &effect.wash, effect.manifest.scale)
    }
}

fn video_plays(tree: &Tree, shown: &[PaneId], tab: TabId, window_hidden: bool) -> bool {
    !window_hidden && tree.locate(tab).is_some_and(|(pane, idx)| shown.contains(&pane) && tree.pane(pane).is_some_and(|p| p.active == idx))
}

/// The pane after `pane` in reading order, where a poster's video opens (spec 9); `None` when
/// it is the last, and the video splits it instead.
fn pane_after(tree: &Tree, pane: PaneId) -> Option<PaneId> {
    let panes = tree.panes();
    let i = panes.iter().position(|p| *p == pane)?;
    panes.get(i + 1).copied()
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = window.viewport_size();
        // The highlights report themselves again as this frame lays out.
        self.effect_marks.clear();
        self.effect_shapes.clear();
        self.effect_menus.clear();
        self.window_active = window.is_window_active();
        self.note_focus();
        let now = Instant::now();
        // Before the clocks are read: a change of focus or of Settings' focus re-renders.
        self.sync_pause(now);
        let last_frame = std::mem::replace(&mut self.frame_now, now);
        self.painted = true;
        self.settle(now, last_frame);
        // The theme's blend for this frame, before any element reads `self.theme`.
        self.step_theme_fade(now, cx);
        self.step_effect_fade(now, cx);
        // The wash reads what this frame lays out; its timer ticks once the frame is done.
        self.ensure_effect_timer(cx);
        // A window moved to a screen of another scale has its ink rebuilt for it.
        let scale = window.scale_factor();
        if scale != self.glyph_scale {
            self.glyph_scale = scale;
            if self.effect.as_ref().is_some_and(|e| e.settled()) {
                self.glyphs_pending = Some(self.glyph_shader_for(effects::active::WindowKind::Project));
            }
        }
        // The seconds a moving glyph shader reads, wrapped in f64 (see
        // `GLYPH_CLOCK_WRAP`) so f32 keeps its sub-frame precision in a long session, and held
        // at zero while the user scrolls a document or a terminal (`glyph_clock`).
        let glyph_seconds = match &self.effect {
            Some(e) => {
                let terminals = self.sessions.terminals.values().map(|t| t.read(cx).last_scroll);
                let last_scroll = effects::active::latest_scroll(std::iter::once(self.last_scroll).chain(terminals));
                effects::active::glyph_clock(e.glyph_time(now, motion::reduce_motion()), last_scroll, now)
            }
            None => 0.0,
        };
        effects::active::GlyphSink::set_glyph_time(window, glyph_seconds);
        self.ensure_glyph_timer(cx);
        if let Some(shader) = self.glyphs_pending.take() {
            let applied = match &self.effect {
                Some(e) => effects::active::apply_glyphs(window, &e.effect, shader),
                None => effects::active::GlyphSink::set_glyph_shader(window, None),
            };
            if let Err(notice) = applied {
                eprintln!("arkos: {notice}");
                self.show_notice(notice, cx);
            }
        }
        // Transparent while an effect is on, so the wash under gpui's view shows through the
        // surfaces; at the fade's start they are still opaque, and at its end they are again.
        if self.effect.is_some() != self.transparent {
            self.transparent = self.effect.is_some();
            window.set_background_appearance(if self.transparent { gpui::WindowBackgroundAppearance::Transparent } else { gpui::WindowBackgroundAppearance::Opaque });
        }
        self.sync_notices(SystemTime::now());
        self.note_window_placement(window, cx);
        // A rename whose tab closed or moved has no editor to hold the keys; it ends, else the
        // keys would go nowhere.
        if self.tab_rename.as_ref().is_some_and(|r| !self.tree.pane(r.pane).is_some_and(|p| p.ids.contains(&r.tab))) {
            self.tab_rename = None;
        }
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
        // The invitation hangs under the first pane drawn, while it is due.
        self.invite_pane = if self.invitation_shows() { self.layout().panes.first().map(|(pane, _)| *pane) } else { None };
        // Only the video tabs on screen play; the rest pause until they are shown again.
        self.update_video_activity(cx);
        self.update_web_visibility(cx);
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
        } else if self.tab_rename.is_some() {
            Some(self.tab_rename_focus.clone())
        } else if self.wt_focused && self.wt_prompt.value().is_some() {
            Some(self.wt_prompt_focus.clone())
        } else if self.wt_focused {
            Some(self.wt_focus.clone())
        } else if self.sidebar_focused {
            Some(self.sidebar.focus.clone())
        } else {
            match (self.active_terminal(cx), self.active_viewer(), self.active_video()) {
                (Some(t), _, _) => {
                    let t = t.read(cx);
                    Some(t.find.as_ref().map_or_else(|| t.focus.clone(), |f| f.focus.clone()))
                }
                (None, Some(v), _) => {
                    let v = v.read(cx);
                    Some(v.find.as_ref().map_or_else(|| v.focus.clone(), |f| f.focus.clone()))
                }
                (None, None, Some(v)) => Some(v.read(cx).focus.clone()),
                (None, None, None) => match self.active_web() {
                    // A web tab: its address field while it is being edited, else its content.
                    Some(w) => {
                        let w = w.read(cx);
                        Some(if w.editing { w.address_focus.clone() } else { w.focus.clone() })
                    }
                    None => self.pane_focus.get(&self.tree.focused).cloned(),
                },
            }
        };
        // A page holds the window's keyboard after a click into it; whenever Arkos wants the keys
        // anywhere but that page's own content (a prompt, the palette, another pane), they come
        // back to gpui first, or the typing would go into the page.
        if let Some(f) = &wanted {
            let page = self.webs.values().find(|v| v.read(cx).has_keys()).map(|v| v.read(cx).focus.clone());
            if page.is_some_and(|p| p != *f) {
                self.keys_back_to_arkos();
            }
        }
        if let Some(f) = wanted {
            if !f.is_focused(window) {
                window.focus(&f);
            }
        }
        // The dots breathe with the focused pane's glow, on its clock; a window not in front rests.
        let breath = if self.window_active { self.breath.glow(now, crate::motion::reduce_motion()) } else { 0.0 };
        let right = chrome::Right { bell_dot: self.notices.dot(), doctor_dot: self.doctor_needs_fix() > 0, open: self.popover.value().copied().unwrap_or_default(), hover: self.icon_hover, breath };
        let hover = |which: chrome::Popover| {
            Box::new(cx.listener(move |root, on: &bool, _, cx| {
                let next = if *on { which } else { chrome::Popover::Closed };
                if root.icon_hover != next {
                    root.icon_hover = next;
                    cx.notify();
                }
            })) as chrome::IconHover
        };
        let style = self.effect_style();
        let title_bar = chrome::title_bar(
            &self.project,
            self.sandbox,
            right,
            window.is_fullscreen(),
            &theme,
            style,
            self.fonts.ui.clone().map_or(gpui::SharedString::from(chrome::UI_FONT), Into::into),
            (Box::new(cx.listener(|root, _, _, cx| root.toggle_bell(cx))), hover(chrome::Popover::Bell)),
            (Box::new(cx.listener(|root, _, _, cx| root.toggle_doctor(cx))), hover(chrome::Popover::Doctor)),
        );
        let family = self.ui_family();
        let root_div = div()
            .relative()
            .size_full()
            .key_context("Root")
            .bg(theme.chrome.window_bg.alpha(style.ground_alpha))
            .flex()
            .flex_col()
            .on_drag_move(cx.listener(|root, ev: &DragMoveEvent<drag::TabDrag>, _window, cx| {
                // Capture listeners run parent first, so this forgets the last target before
                // the bar, tab or pane under the pointer claims it; over none of them, none.
                root.drop_target = None;
                let _ = ev.drag(cx).tab;
                cx.notify();
            }))
            .on_mouse_move(cx.listener(|root, ev: &MouseMoveEvent, window, cx| {
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
                if ev.pressed_button != Some(MouseButton::Left) {
                    // A tab drag released outside the window, where no mouse-up reaches us: it
                    // ends here and in gpui, else a later click would drop it.
                    if root.drop_target.take().is_some() {
                        cx.notify();
                    }
                    if let Some(r) = root.reorder.as_mut().filter(|r| !r.ended()) {
                        r.end(Instant::now());
                        cx.notify();
                    }
                    if cx.has_active_drag() {
                        cx.stop_active_drag(window);
                    }
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
                    // A press outside the rename editor (it stops its own) keeps what was typed.
                    root.commit_tab_rename(cx);
                }),
            )
            // A right-click menu (the tree's, a worktree's or a terminal link's) or a popover (the
            // bell or the doctor) closes on any press outside it (its rows stop theirs), and on
            // Escape wherever the keys are.
            .on_any_mouse_down(cx.listener(|root, _ev: &MouseDownEvent, _window, cx| {
                let now = Instant::now();
                root.last_input = now;
                if root.context_menu.hide(now) | root.wt_menu.hide(now) | root.link_menu.hide(now) | root.md_menu.hide(now) | root.close_popovers(now) {
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
                        // The handle's stripe stays while dragging: a release outside its zone
                        // repaints so it fades out.
                        cx.notify();
                    }
                    if root.sidebar_resizing {
                        root.sidebar_resizing = false;
                        root.save(cx);
                    }
                    // A drop stops the event before it gets here; this is a release elsewhere.
                    // A release with no drop: the slot a reorder opened closes again.
                    if let Some(r) = root.reorder.as_mut() {
                        r.end(Instant::now());
                        cx.notify();
                    }
                    if root.drop_target.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(title_bar)
            .children(banners.into_iter().map(|(text, opacity)| banner_bar(text, opacity, family.clone(), &theme, style)))
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
            .children(link_menu::render_link_menu(self, cx))
            .children(md_menu::render_md_menu(self, cx))
            .children(slash_menu::render_slash_menu(self, cx))
            .children(self.render_popover(window, cx))
            .children(sidebar::render_worktree_menu(self, cx));
        // One request per frame while anything eases; an idle window asks for none.
        if self.any_motion(now) {
            window.request_animation_frame();
        }
        // A text field's cursor blinks: one repaint at its next change, never a frame loop, and
        // none while the window is in the background (its cursor rests where it was).
        if let Some(at) = line_edit::take_blink_wake() {
            if self.window_active {
                self.schedule_blink(at, cx);
            }
        }
        let root_div = root_div.on_action(cx.listener(|root: &mut Root, _: &keymap::WhatArkosSends, _window, cx| root.open_disclosure(cx)));
        keymap::attach(root_div, cx)
    }
}

/// One window's answer to a `Customize`: a preview whose TOML did not parse is its error, every
/// other action reports the state `after` it ran. `parsed` is read only for a preview.
pub fn customize_outcome(action: &CustomizeAction, parsed: Result<theme::Theme, String>, after: protocol::Report) -> Result<protocol::Report, String> {
    match (action, parsed) {
        (CustomizeAction::PreviewTheme { .. }, Err(e)) => Err(e),
        _ => Ok(after),
    }
}

/// `themes/<name>.toml` for the theme `cfg` names under `mode` and the system appearance.
pub fn theme_file_for(mode: config::AppearanceMode, system_is_dark: bool, cfg: &config::ThemeConfig, themes: &Path) -> PathBuf {
    themes.join(format!("{}.toml", theme::pick_name(mode, system_is_dark, cfg)))
}

/// What a reported system appearance asks of a window, as (end the preview, switch the theme).
/// A report in the shade the window already knows (a vibrant or high contrast variant of it) asks
/// for nothing, so it cannot end an agent's preview. A change of shade ends a preview in any mode
/// and switches the theme in "system" mode or when a preview ended.
pub fn appearance_step(was_dark: bool, dark: bool, mode: config::AppearanceMode, previewing: bool) -> (bool, bool) {
    if was_dark == dark {
        return (false, false);
    }
    (previewing, previewing || mode == config::AppearanceMode::System)
}

/// Replaces the key bindings with the defaults plus `keybindings`, and sets the menu bar again:
/// it shows (and on macOS answers) the keys bound when it was set.
fn rebind(cx: &mut App, keybindings: &BTreeMap<String, String>) {
    cx.clear_key_bindings();
    cx.bind_keys(keymap::bindings_for(keybindings));
    cx.set_menus(menus());
}

/// The menu bar: the Arkos IDE menu and the Help menu. Set again when the key bindings change so
/// it shows the bound keys. "New Window" is one item: the chooser offers the recent folders and
/// the native picker both. "What Arkos Sends" opens the telemetry disclosure (telemetry spec,
/// section 3.4).
fn menus() -> Vec<Menu> {
    vec![
        Menu {
            name: "Arkos IDE".into(),
            items: vec![
                MenuItem::action("About Arkos IDE", keymap::About),
                MenuItem::separator(),
                MenuItem::action("New Window", keymap::NewWindow),
                MenuItem::action("Settings", keymap::OpenSettings),
                MenuItem::action("Quit", Quit),
            ],
        },
        Menu {
            name: "Help".into(),
            items: vec![
                MenuItem::action("Keybindings", keymap::Keybindings),
                MenuItem::action("Documentation", keymap::Documentation),
                MenuItem::action("What Arkos Sends", keymap::WhatArkosSends),
            ],
        },
    ]
}

/// The install's UTC day and this launch's, for the invitation, when it can show in this run at
/// all: the days and the channel are fixed for the run, so a launch on the install day or a source
/// build never looks at the mode or at `asked` again.
fn invite_days(installed: SystemTime, now: SystemTime, source: bool) -> Option<(u64, u64)> {
    let days = (telemetry::utc_day(installed), telemetry::utc_day(now));
    (days.1 > days.0 && !source).then_some(days)
}

fn banner_bar(text: String, opacity: f32, family: gpui::SharedString, theme: &theme::Theme, style: effects::active::Style) -> Div {
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
        .text_size(px(style.ui(13.0)))
        .child(text)
}

/// The theme's fonts that this Mac has; each missing one is said once on stderr and its slot
/// falls back as if the theme had not named it.
/// The end of a theme fade, or the theme shown when none runs (`Root::target_theme`).
fn fade_target<'a>(fade: Option<&'a ThemeFade>, shown: &'a Arc<theme::Theme>) -> &'a Arc<theme::Theme> {
    fade.map_or(shown, |f| &f.to)
}

fn installed_fonts(theme: &theme::Theme, cx: &App) -> theme::ThemeFont {
    let families: std::collections::HashSet<String> = cx.text_system().all_font_names().into_iter().collect();
    let (fonts, missing) = theme.font.installed(&families);
    for name in missing {
        eprintln!("arkos: theme {}: the font {name:?} is not installed; using the default for that slot", theme.name);
    }
    fonts
}

/// `<name> <crate version> (<short git hash>)`, the same shape for both binaries.
pub fn version_string() -> String {
    format!("{} {} ({})", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"), env!("ARKOS_GIT_HASH"))
}

/// The date this build's commit was made, `YYYY-MM-DD` (or `unknown`), recorded by `build.rs`.
pub fn build_date() -> &'static str {
    env!("ARKOS_BUILD_DATE")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // The hook CLI: no window, no daemon start, no QoS. `claude` runs these on every event.
    if matches!(args.get(1).map(String::as_str), Some("notify") | Some("hooks")) {
        std::process::exit(hook_cli::run(&args[1..]));
    }
    // The agent theme interface: no window and no daemon start. Each command reads and writes
    // the files itself and asks running windows through the daemon when one is up. These come
    // before the folder argument below, so a folder named `config` opens as `arkos ./config`.
    match args.get(1).map(String::as_str) {
        Some("guide") => std::process::exit(guide::guide_cli(&args[2..])),
        Some("open") => std::process::exit(open_cli::open_cli(&args[2..])),
        Some("video") => std::process::exit(open_cli::video_cli(&args[2..])),
        Some("panes") => std::process::exit(open_cli::panes_cli(&args[2..])),
        Some("theme") => std::process::exit(agent_cli::theme_cli(&args[2..])),
        Some("effect") => std::process::exit(agent_cli::effect_cli(&args[2..])),
        Some("settings") => std::process::exit(agent_cli::settings_cli(&args[2..])),
        Some("config") => std::process::exit(agent_cli::config_cli(&args[2..])),
        Some("telemetry") => std::process::exit(telemetry_cli::telemetry_cli(&args[2..])),
        Some("sandbox") => std::process::exit(sandbox::sandbox_cli(&args[2..])),
        Some("--help") | Some("-h") | Some("help") => {
            print!("{}", agent_cli::USAGE);
            return;
        }
        _ => {}
    }
    // Read-only diagnostics for a failed update: no window, no daemon start, nothing written.
    if args.get(1).map(String::as_str) == Some("doctor") {
        std::process::exit(doctor::run());
    }
    if args.iter().any(|a| a == "--version") {
        println!("{}", version_string());
        return;
    }
    // The panic hook, once every command-line branch above has had its turn (a new one goes
    // above this line too), so a crash in a CLI run is never an app incident: from here on a
    // release build's panic becomes an incident for the crash dialog.
    ::telemetry::panic::install(::telemetry::envelope(::telemetry::Bin::App), env!("ARKOS_GIT_HASH"), &paths::telemetry_dir());
    // `arkos <folder>`: open that folder, as `cd` there and `arkos` would. A Launch Services
    // start (`open -a`, the screenshot shoot) has no working directory to speak of.
    if let Some(folder) = args.get(1).filter(|a| !a.starts_with('-')) {
        if let Err(e) = std::env::set_current_dir(folder) {
            eprintln!("arkos: cannot open {folder}: {e}");
            std::process::exit(2);
        }
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
    // Navy Light is no longer built in: a config naming it is moved to Peach Light (or Navy for
    // the dark slot) before it is parsed, and the window says so once when the light theme moved
    // (the file never names it again). Before the fingerprint, so the rewritten file is the one
    // fingerprinted.
    let theme_retired = config::retire_navy_light_file(&config_path, &paths::themes_dir()) == config::Retired::Light;
    // Taken before the load: the project window reads the file again when it changed since
    // (Settings opened from the chooser writes it), so a write landing during the load is not
    // missed; at worst it costs one extra reload.
    let fingerprint = config_watch::fingerprint(&config_path);
    let (config, config_error) = match config::load(&config_path) {
        Ok(c) => (c, None),
        Err(e) => {
            eprintln!("arkos: {e}");
            (config::Config::default(), Some(e.to_string()))
        }
    };
    // Used only when this window starts the daemon; a running one keeps its own settings.
    let daemon_args = daemon_args(&config, ::telemetry::is_sandbox());
    let cwd = std::env::current_dir().expect("cwd");
    let relaunched = !should_activate(std::env::var("ARKOS_RELAUNCH").ok().as_deref());
    let plan = launch::startup_plan(&cwd, relaunched);
    // Once per process: a synchronous read and write of one tiny file, before any window.
    let updated = startup::updated_from(&paths::last_version_file(), env!("CARGO_PKG_VERSION"));
    // The same: the first run's moment, written on the first run and read back on every other.
    let installed = startup::installed_since(&paths::installed_file(), SystemTime::now());
    // The usage journal opens before the first connect, which names its session; a config error
    // records nothing this run.
    telemetry::init(
        if config_error.is_some() { Err(()) } else { Ok(&config.telemetry) },
        if updated.is_some() { telemetry::Launch::Updated } else if relaunched { telemetry::Launch::Relaunch } else { telemetry::Launch::Normal },
    );
    let yes = |b: bool| if b { "yes" } else { "no" };
    eprintln!("{} starting in {} (relaunch: {}, chooser: {})", version_string(), cwd.display(), yes(relaunched), yes(plan.is_chooser()));
    // The connect runs off the main thread once the app runs, the chooser included: a window the
    // daemon relaunched after an update confirms it with the Hello the connection sends at once.
    // A Dock launch has no folder yet, so its Hello names none (the daemon keeps what it had),
    // and a second Hello names the folder once one is chosen. The chooser heartbeats meanwhile,
    // so the watchdog leaves it alone.
    debug_assert!(plan.connects_first());
    let save_queue = SharedSaveQueue::default();

    Application::new().run(move |cx: &mut App| {
        // gpui installs no menu and no quit binding of its own; the Quit action is handled below,
        // and by the project window once it exists.
        cx.bind_keys(keymap::bindings_for(&config.keybindings));
        cx.set_menus(menus());
        let system_is_dark = theme::is_dark(cx.window_appearance());
        let (theme, theme_error) = startup_theme(&config, system_is_dark);
        let (look_config, look_theme) = (config.clone(), theme.clone());
        aux::init(cx, aux::LaunchState { config: config.clone(), theme: theme.clone(), config_path: config_path.clone(), installed, fingerprint, client: None });
        let start = Startup { config, config_error, config_path, daemon_args: daemon_args.clone(), save_queue, theme, theme_error, system_is_dark, updated, theme_retired };
        let launch = Rc::new(Launch {
            plan,
            cwd,
            config: look_config,
            theme: look_theme,
            daemon_args,
            start: RefCell::new(Some(start)),
            chooser: Cell::new(None),
            client: RefCell::new(None),
            flushed: Cell::new(false),
            opened: Cell::new(false),
            retry: RefCell::new(startup::Retry::default()),
        });
        // Until the project window opens, Quit ends the app (and the closes below); from then on
        // its own handlers take over. None says Goodbye: no folder is open, so the daemon forgets
        // none. A quit right after "Stop all" still delivers the kills: the writer gets a moment.
        let quitting = launch.clone();
        cx.on_action(move |_: &Quit, cx| {
            if !quitting.opened.get() {
                quitting.flush();
                cx.quit();
            }
        });
        // About, Settings and the Help menu's items work in every state, so they are registered
        // once, here, for the whole run. The project window's own handlers run the same
        // functions, and an action one of them handles stops there: "What Arkos Sends" opens the
        // page in a viewer tab there, and from the chooser or Settings in the default app.
        cx.on_action(|_: &keymap::About, cx| aux::open_about(cx));
        cx.on_action(|_: &keymap::OpenSettings, cx| aux::open_settings(cx));
        cx.on_action(|_: &keymap::Keybindings, cx| aux::open_keybindings(cx));
        cx.on_action(|_: &keymap::Documentation, cx| aux::open_documentation(cx));
        cx.on_action(|_: &keymap::WhatArkosSends, cx| aux::open_disclosure(cx));
        // Closing the launch chooser quits, the failure window's red button too; "Try again"
        // closes the failure window as its connect starts, which is no reason to quit.
        let closing = launch.clone();
        cx.on_window_closed(move |cx| {
            let windows = cx.windows();
            // About, Settings and Keybindings never keep the app alive on their own.
            let left = startup::windows_without_aux(windows.len(), aux::aux_open(cx));
            // Still opening when its handle is not known yet: open, as long as any window is.
            let chooser_open = match closing.chooser.get() {
                Some(h) => windows.iter().any(|w| w.window_id() == h.window_id()),
                None => left > 0,
            };
            if startup::quits_on_close(closing.opened.get(), plan.is_chooser(), left, chooser_open, closing.retry.borrow().running()) {
                closing.flush();
                cx.quit();
            }
        })
        .detach();
        // macOS's own terminate (logout, shutdown, the Dock's Quit) closes the chooser without
        // the close above: the quit observer flushes it too. The project window has its own.
        let terminating = launch.clone();
        cx.on_app_quit(move |_| {
            if !terminating.opened.get() {
                terminating.flush();
            }
            std::future::ready(())
        })
        .detach();
        if !plan.is_chooser() {
            launch.connect(None, cx);
            return;
        }
        let choosing = launch.clone();
        let on_choice: launch::OnChoice = Box::new(move |folder, link, cx| {
            // The project is the working directory, as for a terminal launch: the daemon's
            // Hello, its relaunches and the shells all start from it.
            if let Err(e) = std::env::set_current_dir(&folder) {
                eprintln!("arkos: cannot open {}: {e}", folder.display());
                cx.quit();
                return;
            }
            let project = std::env::current_dir().unwrap_or(folder);
            // The chooser holds a choice until its link arrives, so there is always one here.
            let Some(link) = link else {
                eprintln!("arkos: no daemon connection to open {}", project.display());
                cx.quit();
                return;
            };
            let Some(start) = choosing.start.borrow_mut().take() else { return };
            choosing.opened.set(true);
            // The same connection goes on: its second Hello names the project.
            // The client comes back with the receiver it belongs to: a reconnect that was
            // under way may have replaced the one the link had.
            let finishing = link.finish();
            cx.spawn(async move |cx| {
                let (backlog, events, client) = finishing.await;
                client.hello();
                let _ = cx.update(|cx| open_project_window(project, (client, events, backlog), start, cx));
            })
            .detach();
        });
        // The chooser first, with no link yet: its counts and "Stop all" arrive with the
        // connection.
        let opening = launch::open_chooser(cx, true, plan.focus(), launch.config.clone(), launch.theme.clone(), None, on_choice);
        launch.connect(Some(opening), cx);
    });
}

/// The theme the config names for this appearance, at launch. One that does not load gives the
/// built-in of the same name, else navy, and the banner that says so.
fn startup_theme(config: &config::Config, system_is_dark: bool) -> (theme::Theme, Option<String>) {
    let name = theme::pick_name(config.appearance.mode, system_is_dark, &config.theme).to_string();
    match theme::load(&name, &paths::themes_dir()) {
        Ok(t) => (t, None),
        Err(e) => {
            // A broken user theme file named like a built-in still gives that built-in.
            let t = theme::builtin(&name).or_else(|| theme::builtin("navy")).expect("navy is built in");
            let banner = format!("theme: {e}; using built-in {}", t.name);
            eprintln!("arkos: {banner}");
            (t, Some(banner))
        }
    }
}

/// The start while no project window exists: the connect runs off the main thread, the launch
/// chooser (when the plan has one) is open meanwhile, and a failed connect opens the failure
/// window, whose "Try again" runs it again. The first connect and every retry share it.
struct Launch {
    plan: launch::Plan,
    cwd: PathBuf,
    /// The chooser's and the failure window's look; `start` has its own.
    config: config::Config,
    theme: theme::Theme,
    daemon_args: Vec<String>,
    /// Lives here while the connect runs; taken by whichever path opens the project window.
    start: RefCell<Option<Startup>>,
    /// The launch chooser, once it is open.
    chooser: Cell<Option<WindowHandle<launch::Chooser>>>,
    /// The connection once it exists, so a quit can flush what the chooser sent.
    client: RefCell<Option<Arc<client::Client>>>,
    /// A quit flushed already: Quit and the close it makes, then macOS's terminate, each ask.
    flushed: Cell<bool>,
    /// The project window is open or opening: the start's handlers stand down.
    opened: Cell<bool>,
    retry: RefCell<startup::Retry>,
}

impl Launch {
    /// Starts a connect, unless one runs. `opening` is the chooser still opening, on the first
    /// connect: the connect is not held back for it, only its outcome.
    fn connect(self: &Rc<Self>, opening: Option<Task<Option<WindowHandle<launch::Chooser>>>>, cx: &mut App) {
        if !self.retry.borrow_mut().begin() {
            return;
        }
        self.set_footer(Some(launch::FOOTER_CONNECTING), cx);
        let connecting = startup::connect_in_background(self.daemon_args.clone(), cx);
        let this = self.clone();
        cx.spawn(async move |cx| {
            if let Some(opening) = opening {
                this.chooser.set(opening.await);
            }
            let result = connecting.await;
            let _ = cx.update(|cx| {
                this.retry.borrow_mut().end();
                this.connected(result, cx);
            });
        })
        .detach();
    }

    /// A connect's outcome: the project window, or the chooser's link, or the failure window.
    fn connected(self: &Rc<Self>, result: Result<startup::Connected, String>, cx: &mut App) {
        let (client, events) = match result {
            Ok(connected) => connected,
            Err(e) => {
                eprintln!("arkos: cannot reach arkosd (attempt {}): {e}", self.retry.borrow().attempts());
                telemetry::error(telemetry::Failure::DaemonUnreachable);
                self.set_footer(None, cx);
                let this = self.clone();
                startup::open_failure_window(e, Rc::new(move |cx: &mut App| this.connect(None, cx)), &self.config, &self.theme, cx);
                return;
            }
        };
        let client = Arc::new(client);
        *self.client.borrow_mut() = Some(client.clone());
        // A usage change in the chooser's Settings says its new session id on this connection.
        aux::set_launch_client(cx, client.clone());
        // The id this connection said was read on the connect thread; a change in the chooser
        // since then restarted the journal, so it says the Recorder's own now.
        telemetry::sync_sid(&*client);
        if !self.plan.is_chooser() {
            let start = self.start.borrow_mut().take();
            if let Some(start) = start {
                self.opened.set(true);
                open_project_window(self.cwd.clone(), (client, events, Vec::new()), start, cx);
            }
            return;
        }
        // The link beats the heartbeat (the chooser's Hello named this pid, so the watchdog
        // expects it) until the project window's connection runs its own.
        let link = launch_link::Link::new(client, events, paths::socket_path(), paths::daemon_exe(), self.daemon_args.clone());
        let attached = self.chooser.get().map(|h| h.update(cx, |chooser, window, cx| chooser.attach_link(link, window, cx)));
        // The chooser is gone (its close quits, so only a race gets here): nothing is left to
        // show.
        if !matches!(attached, Some(Ok(()))) && !self.opened.get() {
            self.flush();
            cx.quit();
        }
    }

    fn set_footer(&self, text: Option<&str>, cx: &mut App) {
        if let Some(h) = self.chooser.get() {
            let _ = h.update(cx, |chooser, _, cx| chooser.set_footer(text.map(str::to_string), cx));
        }
    }

    /// Before a quit from the chooser, which says no Goodbye: the final mark and `SessionQuit`,
    /// so the daemon seals the chooser's session as a quit, then a moment for the writer to send
    /// them and what the chooser sent (a "Stop all"'s kills). Once a run.
    fn flush(&self) {
        if self.flushed.replace(true) {
            return;
        }
        let client = self.client.borrow().clone();
        telemetry::quit_without_goodbye(client.as_deref());
        if let Some(client) = client {
            client.wait_sent(Duration::from_millis(500));
        }
    }
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
    /// The version this launch updated from, as "Updated to X" for the bell.
    updated: Option<String>,
    /// The config named the removed Navy Light and was rewritten: the bell says so once.
    theme_retired: bool,
}

/// Where the project window opens: where it was, on its display, when that display is attached;
/// else centered on the main display at its size. Either way it is kept within the display (it
/// may be a smaller one now). 1200 by 800 centered with nothing saved.
fn open_bounds(saved: Option<&workspace::WindowSaved>, cx: &App) -> (Option<gpui::DisplayId>, Bounds<gpui::Pixels>) {
    let Some(s) = saved.filter(|s| s.w.is_finite() && s.h.is_finite() && s.w >= 200.0 && s.h >= 200.0) else {
        return (None, Bounds::centered(None, size(px(1200.), px(800.)), cx));
    };
    let found = cx.displays().into_iter().find(|d| d.uuid().ok().is_some_and(|u| u.to_string() == s.display));
    let display = found.as_ref().map(|d| d.id());
    let Some(screen) = found.or_else(|| cx.primary_display()).map(|d| d.bounds().size) else {
        return (None, Bounds::centered(None, size(px(s.w as f32), px(s.h as f32)), cx));
    };
    let (dw, dh) = (f64::from(f32::from(screen.width)), f64::from(f32::from(screen.height)));
    let top = screen::menu_bar_height();
    let (x, y, w, h) = match display {
        Some(_) => workspace::fit_window(s.x, s.y, s.w, s.h, dw, dh, top),
        None => workspace::fit_window((dw - s.w) / 2.0, (dh - s.h) / 2.0, s.w, s.h, dw, dh, top),
    };
    (display, Bounds::new(point(px(x as f32), px(y as f32)), size(px(w as f32), px(h as f32))))
}

/// Opens the window on `project` and starts everything that serves it.
fn open_project_window(
    project: PathBuf,
    // The connection, and what the launch chooser read from it before the window existed.
    (client, events, backlog): (Arc<client::Client>, futures::channel::mpsc::UnboundedReceiver<protocol::ServerMessage>, Vec<protocol::ServerMessage>),
    start: Startup,
    cx: &mut App,
) {
    let Startup { mut config, mut config_error, config_path, daemon_args, save_queue, mut theme, mut theme_error, system_is_dark, updated, mut theme_retired } = start;
    let now = config_watch::fingerprint(&config_path);
    // `main()` sets the registry up before any window, so a missing one is a wiring bug.
    let launch = cx.global::<aux::Aux>().launch.as_ref().expect("aux::init runs before any window");
    let (launched, installed) = (launch.fingerprint, launch.installed);
    // The file changed since `main()` read it (Settings, opened from the chooser, writes it): read
    // it again, and the theme it names, so that change is not lost.
    if now != launched {
        // The Recorder has seen what the chooser's Settings applied; a change made elsewhere
        // meanwhile (`arkos telemetry on`, an editor) reaches it here, a new session id with it.
        let applied = launch.config.clone();
        // Migrated like the first read, in case the file was edited back to Navy Light meanwhile.
        theme_retired |= config::retire_navy_light_file(&config_path, &paths::themes_dir()) == config::Retired::Light;
        let (cfg, banner) = config::reload_decision(&config, config::load(&config_path));
        telemetry::opening_config(&applied, &cfg, banner.is_some(), Some(&*client));
        if cfg.keybindings != config.keybindings {
            rebind(cx, &cfg.keybindings);
        }
        config = cfg;
        config_error = banner;
        (theme, theme_error) = startup_theme(&config, system_is_dark);
    }
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
        // The window comes back where it was: on its display when that is still attached, at
        // its size and place.
        let (display_id, bounds) = open_bounds(saved.as_ref().and_then(|s| s.window.as_ref()), cx);
        let client_for_view = client.clone();
        let view_queue = save_queue.clone();
        let view_project = project.clone();
        let view_saved = saved.clone();
        let (view_config, view_theme) = (config, theme);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    display_id,
                    // The window draws its own bar in the title bar's space (chrome.rs); the
                    // traffic lights stay where macOS puts them.
                    titlebar: Some(gpui::TitlebarOptions { title: Some(screen::window_title(::telemetry::is_sandbox()).into()), appears_transparent: true, traffic_light_position: None }),
                    // One minimum pane with its border, tab bar and the window's bar.
                    window_min_size: Some(size(px(tree::MIN_PANE_W + 8.0), px(tree::MIN_PANE_H + pane_view::TAB_BAR_H + chrome::TITLE_BAR_H))),
                    // A window the daemon relaunched (an update, the watchdog) does not take the
                    // keyboard from the app in use.
                    focus: should_activate(std::env::var("ARKOS_RELAUNCH").ok().as_deref()),
                    ..Default::default()
                },
                move |window, cx| {
                    let view = cx.new(|cx| {
                        cx.observe_window_activation(window, |root: &mut Root, window, cx| {
                            root.window_active = window.is_window_active();
                            root.note_focus();
                            // The effects pause or resume at the change itself, not the next frame.
                            root.sync_pause(Instant::now());
                            // A minimize deactivates first: a video pauses as soon as the window
                            // reads as hidden, here or on the next poll.
                            root.update_video_activity(cx);
                            // Another window may have had the invitation's answer meanwhile.
                            if root.window_active {
                                root.recheck_invitation(cx);
                            }
                            if !root.window_active && root.close_popovers(Instant::now()) {
                                cx.notify();
                            }
                        })
                        .detach();
                        let mut root = Root::new(client_for_view, view_project, view_saved, view_config, view_theme, system_is_dark, installed, cx);
                        root.ns_window = screen::ns_window_of(window);
                        root.ns_view = screen::ns_view_of(window);
                        if let (Some(ns), Some(view)) = (&root.ns_window, &root.ns_view) {
                            root.key_monitor = screen::install_key_monitor(ns.clone(), view.clone());
                        }
                        // The effect the config and the theme name, under gpui's view; the first
                        // frame draws it at once, as it does the theme.
                        root.resolve_effect(cx);
                        // ARKOS_WINDOW_LEVEL=floating keeps the window above the others without
                        // taking the keyboard: the screenshot shoot (scripts/shoot.sh) sets it,
                        // since macOS stops drawing a window another one covers entirely, and
                        // a capture of it would then show its last frame.
                        if std::env::var_os("ARKOS_WINDOW_LEVEL").is_some_and(|v| v == "floating") {
                            if let Some(ns) = &root.ns_window {
                                ns.setLevel(objc2_app_kit::NSFloatingWindowLevel);
                            }
                        }
                        root.save_queue = view_queue;
                        root.build_highlighter(cx);
                        root
                    });
                    // The red button asks first; a close during macOS's terminate does not.
                    let closing = view.clone();
                    // Unsaved edits that cannot be written keep the window open, once (`may_quit`).
                    window.on_window_should_close(cx, move |_, cx| {
                        closing.update(cx, |root, cx| {
                            let close = root.may_quit(cx);
                            root.user_closing |= close;
                            close
                        })
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
        telemetry::window_open();
        telemetry::restored(&root.read(cx).tree);
        // About and Settings take their details from the Root from now on.
        aux::set_root(cx, root.downgrade());
        // The caches' old entries go once a launch, on a thread of their own, now the window is up.
        std::thread::spawn(cache::sweep_all);
        root.update(cx, |root, cx| {
            root.config_error = config_error;
            root.invitation_config_changed();
            root.theme_error = theme_error;
            root.seed_notices(updated);
            if theme_retired {
                root.notices.set(notices::Kind::ThemeRetired, config::NAVY_LIGHT_NOTICE, None, SystemTime::now());
            }
            root.follow_theme_in_claude(cx);
            root.apply_recovery(true, cx);
            root.load_pending_dirs(cx);
            root.refresh_status(cx);
            root.refresh_worktrees(cx);
            cx.notify();
        });
        // A Settings, About or Keybindings window opened from the chooser shows this window's
        // state and theme from now on.
        let (shown, theme, overrides) = {
            let root = root.read(cx);
            (root.settings_shown(), root.theme.clone(), root.config.keybindings.clone())
        };
        aux::push_settings(cx, shown);
        aux::push_theme(cx, &theme);
        aux::push_keybindings(cx, &overrides);
        // The crash reports a panic left, now the window is up: shown in the crash dialog, queued
        // or deleted as `telemetry.crashes` says, or left waiting under any rule of the
        // environment. A command-line run never gets here. "Always send" applies through the
        // reload, as Settings' writes do.
        let (crashes, mode, mono) = {
            let root = root.read(cx);
            let config = if root.config_error.is_some() { Err(()) } else { Ok(&root.config.telemetry) };
            (root.config.telemetry.crashes, ::telemetry::mode_for(config, true), root.family())
        };
        let reloading = root.downgrade();
        let on_always: Rc<dyn Fn(&mut App)> = Rc::new(move |cx| {
            let _ = reloading.update(cx, |root, cx| root.reload_config(cx));
        });
        crash_dialog::at_launch(crashes, &mode, (*theme).clone(), mono, on_always, cx);
        let quit_root = root.clone();
        cx.on_action(move |_: &Quit, cx| {
            // Goodbye here, while everything runs; the close this quit makes finds it said. Unsaved
            // edits that cannot be written stop the first Cmd+Q (`may_quit`).
            let quit = quit_root.update(cx, |root, cx| {
                if !root.may_quit(cx) {
                    return false;
                }
                root.user_closing = true;
                root.leaving(cx);
                true
            });
            if quit {
                cx.quit();
            }
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
            match after_window_closed(left, project_open, chooser.is_some() || aux::aux_open(cx) > 0) {
                AfterClose::Quit => {
                    close_root.update(cx, |root, cx| if says_goodbye(root.user_closing, left) { root.leaving(cx) } else { root.flush_for_quit(false, cx) });
                    cx.quit();
                }
                // Their closes come back here, the last with no window left, and quit as above.
                AfterClose::CloseAux => {
                    aux::close_all(cx);
                    if let Some(h) = chooser {
                        let _ = h.update(cx, |_, window, _| window.remove_window());
                    }
                }
                AfterClose::Stay => {}
            }
        })
        .detach();
        // macOS's own terminate (logout, shutdown, the Dock's Quit, AppleScript) runs gpui's
        // shutdown, which clears the windows without `on_window_closed`: the quit observer is the
        // only hook it runs, so it flushes too. Every other quit flushed already, and a second
        // flush finds clean viewers and edits already recovered.
        let quit_flush = root.downgrade();
        cx.on_app_quit(move |cx| {
            let _ = quit_flush.update(cx, |root, cx| root.flush_for_quit(false, cx));
            std::future::ready(())
        })
        .detach();
        config_watch::start(root.clone(), config_path.clone(), cx);
        explorer_watch::start(root.clone(), cx);
        // The same client the root was built with; reconnects replace it from here on.
        connection::start(root.clone(), paths::socket_path(), paths::daemon_exe(), daemon_args.clone(), (client.clone(), events, backlog), cx);
        // A sandbox never updates, so it never looks for a newer install either.
        if !root.read(cx).sandbox {
            update::start(root.clone(), cx);
        }
        pulse::start(root.downgrade(), cx);

        // The focus totals reach the journal once a minute, and at quit (`flush_for_quit`).
        cx.spawn(async move |cx| loop {
            cx.background_executor().timer(telemetry::MARK_EVERY).await;
            telemetry::mark();
        })
        .detach();
        // While the invitation shows, `asked` is looked for again now and then, not every frame;
        // the loop ends once the invitation can no longer show in this run.
        let invite_root = root.clone();
        cx.spawn(async move |cx| loop {
            cx.background_executor().timer(telemetry::ASKED_EVERY).await;
            if !invite_root.update(cx, |root, cx| root.recheck_invitation(cx)).unwrap_or(false) {
                break;
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
    use super::{after_window_closed, closed_entry, daemon_args, daemon_label, menus, pane_after, pick_grid, quit_forced, reopened_video, says_goodbye, video_plays, AfterClose, shell_name, should_activate, Quit, CONNECTING, ERROR_PREFIX, FORCE_QUIT_WITHIN};
    use crate::keymap;
    use gpui::{Action, MenuItem};

    /// Checks one menu's items against `want`: `None` for a separator, else the label and action.
    fn assert_menu(menu: &gpui::Menu, name: &str, want: &[Option<(&str, &dyn Action)>]) {
        assert_eq!(menu.name.as_ref(), name);
        assert_eq!(menu.items.len(), want.len(), "{name}");
        for (ix, (item, want)) in menu.items.iter().zip(want).enumerate() {
            match (item, want) {
                (MenuItem::Separator, None) => {}
                (MenuItem::Action { name: label, action, .. }, Some((want_name, want_action))) => {
                    assert_eq!(label.as_ref(), *want_name, "{name} item {ix}");
                    assert!(action.partial_eq(*want_action), "{name} item {ix}: {} runs {}", want_name, action.name());
                }
                _ => panic!("{name} item {ix} is not the expected kind"),
            }
        }
    }

    #[test]
    fn the_menu_bar_is_the_arkos_ide_menu_then_the_help_menu() {
        let menus = menus();
        assert_eq!(menus.len(), 2);
        assert_menu(
            &menus[0],
            "Arkos IDE",
            &[Some(("About Arkos IDE", &keymap::About)), None, Some(("New Window", &keymap::NewWindow)), Some(("Settings", &keymap::OpenSettings)), Some(("Quit", &Quit))],
        );
        assert_menu(
            &menus[1],
            "Help",
            &[Some(("Keybindings", &keymap::Keybindings)), Some(("Documentation", &keymap::Documentation)), Some(("What Arkos Sends", &keymap::WhatArkosSends))],
        );
    }

    #[test]
    fn a_window_the_daemon_relaunched_does_not_take_focus() {
        assert!(should_activate(None), "opened by the user");
        assert!(should_activate(Some("")));
        assert!(!should_activate(Some("1")), "ARKOS_RELAUNCH from watchdog::launch_ui");
    }

    #[test]
    fn launch_closing_the_project_window_closes_its_chooser_then_quits() {
        assert_eq!(after_window_closed(0, false, false), AfterClose::Quit);
        assert_eq!(after_window_closed(1, false, true), AfterClose::CloseAux, "only the chooser is left");
        assert_eq!(after_window_closed(1, true, false), AfterClose::Stay, "the chooser closed; the project stays");
        assert_eq!(after_window_closed(2, true, true), AfterClose::Stay);
    }

    #[test]
    fn an_aux_window_left_alone_does_not_keep_the_app_alive() {
        assert_eq!(after_window_closed(0, false, false), AfterClose::Quit);
        assert_eq!(after_window_closed(1, false, true), AfterClose::CloseAux, "About stayed after the project window closed");
        assert_eq!(after_window_closed(2, false, true), AfterClose::CloseAux, "the chooser and About both");
        assert_eq!(after_window_closed(1, true, true), AfterClose::Stay, "a project window remains");
    }

    #[test]
    fn a_revert_that_lands_before_the_first_fade_frame_still_switches() {
        use super::{fade_target, ThemeFade};
        use crate::theme;
        use std::sync::Arc;
        use std::time::Instant;
        // A theme file edited in place keeps its name: the watcher fires for the write, then
        // again when the editor's rename restores the old text before the first fade frame.
        // Both files say nothing of their appearance, so each is classed by its background; the
        // blend takes the target's, as it takes its name.
        let mut plain = theme::builtin("navy").unwrap();
        plain.appearance = None;
        let navy = Arc::new(plain);
        let mut edited = theme::builtin("lavender-light").unwrap();
        edited.name = "navy".into();
        edited.appearance = None;
        edited.font = theme::ThemeFont::default(); // the blend takes the target's fonts, and navy names none
        edited.effect = navy.effect.clone(); // and its effect: the edited file keeps navy's space
        let light = Arc::new(edited);
        // The screen at the start of that fade: navy's colours under the edited theme's name.
        let shown = Arc::new(theme::blend(&navy, &light, 0.0));
        let fade = ThemeFade { from: navy.clone(), to: light.clone(), since: Instant::now() };
        assert_eq!(theme::keep_or_switch(&shown, Ok((*navy).clone())).0, None, "the blend alone would swallow the revert");
        let target = fade_target(Some(&fade), &shown);
        assert_eq!(**target, *light, "the target is the fade's end");
        assert_eq!(theme::keep_or_switch(target, Ok((*navy).clone())).0, Some((*navy).clone()), "so a revert to navy switches");
        assert_eq!(theme::keep_or_switch(target, Ok((*light).clone())).0, None, "and the theme being faded to is not applied twice");
        assert!(target.is_light(), "the report says light while navy is still on screen");
        assert_eq!(**fade_target(None, &shown), *shown, "with no fade, the theme shown is the target");
    }

    #[test]
    fn a_second_quit_within_ten_seconds_goes_ahead_without_the_unsaved_edits() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        assert!(!quit_forced(None, t0), "the first attempt stops");
        assert!(quit_forced(Some(t0), t0 + Duration::from_secs(3)));
        assert!(quit_forced(Some(t0), t0 + FORCE_QUIT_WITHIN), "at the limit");
        assert!(!quit_forced(Some(t0), t0 + FORCE_QUIT_WITHIN + Duration::from_millis(1)), "too late: it stops again");
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

    /// The invitation is looked for only on a later UTC day than the install, and never in a
    /// source build; the rest (the mode, `asked`) is `telemetry::Invitation`'s.
    #[test]
    fn the_invitation_is_never_looked_for_on_the_install_day_or_in_a_source_build() {
        use super::invite_days;
        use std::time::{Duration, SystemTime};
        // 2026-10-16 09:00 UTC, day 20742.
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_792_141_200);
        assert_eq!(invite_days(at, at + Duration::from_secs(14 * 3600), false), None, "later on the install day");
        assert_eq!(invite_days(at, at + Duration::from_secs(15 * 3600), false), Some((20_742, 20_743)), "the next UTC day");
        assert_eq!(invite_days(at, at + Duration::from_secs(15 * 3600), true), None, "a source build");
        assert_eq!(invite_days(at, at - Duration::from_secs(86_400), false), None, "a clock set back");
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
    fn the_daemon_counts_as_connected_only_with_no_status_and_no_reconnect() {
        use super::daemon_connected;
        assert!(daemon_connected("", false));
        assert!(!daemon_connected("", true));
        assert!(!daemon_connected("daemon closed the connection", false));
        assert!(!daemon_connected(super::CONNECTING, false));
    }

    #[test]
    fn the_focused_session_is_the_focused_panes_active_terminal_tab() {
        use super::{focused_session_of, Tab, Tree};
        use protocol::SessionId;
        let mut tree = Tree::new(Tab::Terminal { session: SessionId(5), cwd: None });
        let root = tree.panes()[0];
        assert_eq!(focused_session_of(&tree), Some(SessionId(5)));
        tree.add_tab(root, Tab::Viewer { path: "/f".into(), mode: crate::tree::ViewMode::default() });
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
    fn only_a_change_of_shade_ends_a_preview_or_switches_the_theme() {
        use super::appearance_step;
        use crate::config::AppearanceMode::{Dark, Light, System};
        for mode in [System, Dark, Light] {
            for previewing in [false, true] {
                assert_eq!(appearance_step(true, true, mode, previewing), (false, false), "a variant of dark: {mode:?} {previewing}");
                assert_eq!(appearance_step(false, false, mode, previewing), (false, false), "a variant of light: {mode:?} {previewing}");
            }
        }
        assert_eq!(appearance_step(true, false, System, false), (false, true), "system mode follows");
        assert_eq!(appearance_step(true, false, Dark, false), (false, false), "a fixed mode ignores it");
        assert_eq!(appearance_step(false, true, Light, true), (true, true), "a change of shade ends a preview in any mode");
        assert_eq!(appearance_step(false, true, System, true), (true, true));
    }

    #[test]
    fn a_bad_preview_reports_its_error_and_the_watched_file_is_the_configured_theme() {
        use super::{customize_outcome, theme_file_for};
        use crate::{config, theme};
        use protocol::{CustomizeAction, Report};
        use std::path::{Path, PathBuf};
        let after = Report { theme: "x".into(), preview: Some("x".into()), ..Report::default() };
        let bad = customize_outcome(&CustomizeAction::PreviewTheme { toml: "nope".into() }, Err("expected an equals".into()), after.clone());
        assert_eq!(bad, Err("expected an equals".into()));
        let good = customize_outcome(&CustomizeAction::PreviewTheme { toml: "...".into() }, Ok(theme::builtin("navy").unwrap()), after.clone());
        assert_eq!(good.unwrap().preview.as_deref(), Some("x"));
        assert!(customize_outcome(&CustomizeAction::ReportState, Err("unused".into()), after).is_ok(), "only a preview can fail");
        let cfg = config::ThemeConfig { dark: "timo-purple".into(), light: "lavender-light".into() };
        assert_eq!(theme_file_for(config::AppearanceMode::System, true, &cfg, Path::new("/t")), PathBuf::from("/t/timo-purple.toml"));
        assert_eq!(theme_file_for(config::AppearanceMode::Light, true, &cfg, Path::new("/t")), PathBuf::from("/t/lavender-light.toml"));
    }

    #[test]
    fn a_refused_window_says_so_on_its_tabs() {
        use super::{connection::is_final, disconnect_mark};
        let refused = format!("{ERROR_PREFIX}protocol version mismatch: daemon speaks 1 to 2, client 3");
        assert_eq!(disconnect_mark(is_final(&refused)), "daemon refused");
        assert_eq!(disconnect_mark(is_final("daemon closed the connection")), "reconnecting");
    }

    #[test]
    fn a_closed_video_reopens_with_its_position() {
        use crate::closed_stack::Closed;
        use crate::tree::Tab;
        let tab = Tab::Video { path: "/p/cut.mp4".into(), at: Some(12.0), loop_range: Some((2.0, 4.5)) };
        let closed = closed_entry(tab.clone()).unwrap();
        assert_eq!(closed, Closed::Video { path: "/p/cut.mp4".into(), at: Some(12.0), loop_range: Some((2.0, 4.5)) });
        assert_eq!(reopened_video(closed), Some(tab));
        assert_eq!(reopened_video(Closed::Viewer("/p/a.rs".into())), None, "a viewer reopens through the viewer's own path");
        assert_eq!(closed_entry(Tab::Terminal { session: protocol::SessionId(1), cwd: None }), None, "a session's close is its own");
    }

    #[test]
    fn a_video_plays_only_as_the_active_tab_of_a_shown_pane_in_a_shown_window() {
        use crate::tree::{Side, Tab, Tree};
        let video = |name: &str| Tab::Video { path: format!("/p/{name}.mp4").into(), at: None, loop_range: None };
        let mut t = Tree::new(video("a"));
        let left = t.focused;
        t.add_tab(left, video("b"));
        let right = t.split(left, Side::Right, video("c")).unwrap();
        let (a, b, c) = (t.tab_id(left, 0).unwrap(), t.tab_id(left, 1).unwrap(), t.tab_id(right, 0).unwrap());
        let both = [left, right];
        assert!(!video_plays(&t, &both, a, false), "a tab behind another in its pane");
        assert!(video_plays(&t, &both, b, false), "the active tab of a pane on screen");
        assert!(video_plays(&t, &both, c, false), "a pane without the focus plays too");
        assert!(!video_plays(&t, &[left], c, false), "a pane hidden by another's maximize");
        assert!(!video_plays(&t, &both, b, true), "a hidden or minimized window");
        assert!(!video_plays(&t, &both, 999, false), "a tab that has closed");
    }

    #[test]
    fn a_poster_opens_its_video_in_the_next_pane_or_splits() {
        use crate::tree::{Side, Tab, Tree};
        let term = |n| Tab::Terminal { session: protocol::SessionId(n), cwd: None };
        let mut t = Tree::new(term(1));
        let doc = t.focused;
        assert_eq!(pane_after(&t, doc), None, "the only pane: split");
        let below = t.split(doc, Side::Bottom, term(2)).unwrap();
        assert_eq!(pane_after(&t, doc), Some(below), "the next pane in reading order");
        assert_eq!(pane_after(&t, below), None, "the last pane: split");
    }

    #[test]
    fn a_sandbox_daemon_never_polls_for_updates() {
        let mut cfg = crate::config::Config::default();
        cfg.update.check_every_secs = 60;
        let poll = |args: &[String]| args.windows(2).find(|w| w[0] == "--check-every-secs").map(|w| w[1].clone());
        assert_eq!(poll(&daemon_args(&cfg, false)), Some("60".into()), "the config's value");
        assert_eq!(poll(&daemon_args(&cfg, true)), Some("0".into()), "no daily check in a sandbox");
        assert_eq!(daemon_args(&cfg, true).iter().filter(|a| *a == "--check-every-secs").count(), 1, "said once");
        assert_eq!(daemon_args(&cfg, true)[..4], daemon_args(&cfg, false)[..4], "the other arguments are the config's");
    }
}
