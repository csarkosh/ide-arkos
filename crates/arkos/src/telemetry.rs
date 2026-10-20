//! What the window records of its session (telemetry spec, section 2.2). The `Recorder` holds
//! this process's journal while usage is not `Off`, under a session id made by `new_id` at launch
//! (each window is its own process), and writes nothing else.
use crate::config::Config;
use crate::keymap::Command;
use crate::tree::{Tab, Tree};
use crate::viewer::Content;
use ::telemetry::visits_app::{new_id, Allowlist, DValue, Envelope, Event, Journal, Queue, Usage};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

/// Declares the values one `d` key may take: the enum, its wire spelling and every variant in
/// `ALL`, all from the one list, so the list a test walks cannot miss a variant.
macro_rules! values {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $wire:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub enum $name {
            $($(#[$vmeta])* $variant),+
        }

        impl $name {
            /// Every value, in the allowlist's order.
            #[cfg(test)]
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The value as the journal spells it.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $wire),+
                }
            }
        }
    };
}

values! {
    /// How this process started.
    Launch {
        Normal => "normal",
        Relaunch => "relaunch",
        Updated => "updated",
        // No call site yet: the daemon starts every window it launches the same way, so a window
        // cannot tell a recovery from a relaunch.
        #[allow(dead_code)]
        Recovered => "recovered",
    }
}
values! {
    /// What a tab shows.
    PaneKind { Terminal => "terminal", Viewer => "viewer", Browser => "browser", Video => "video" }
}
values! {
    /// How a tab was opened.
    Opened { Palette => "palette", Key => "key", Click => "click", Explorer => "explorer", Link => "link", Restore => "restore", Agent => "agent", Poster => "poster" }
}
values! {
    /// What kind of file the viewer showed.
    Format {
        Markdown => "markdown",
        Code => "code",
        Plain => "plain",
        // No call site yet: an image opened as a file is shown as a binary.
        #[allow(dead_code)]
        Image => "image",
        Video => "video",
        Binary => "binary",
        TooLarge => "too_large",
    }
}
values! {
    /// Whether the viewer showed a file rendered or as its text.
    Shown { Rendered => "rendered", Raw => "raw" }
}
values! {
    /// A markdown feature, recorded the first time it is used in a session.
    MdFeature { TableEdit => "table_edit", Mermaid => "mermaid", Details => "details", Checklist => "checklist", RemoteImage => "remote_image", Video => "video", InlineDiff => "inline_diff" }
}
values! {
    /// Which agent runs in a terminal.
    Agent { Claude => "claude", Other => "other" }
}
values! {
    /// Whether an agent started afresh or was resumed after a restart.
    AgentHow { New => "new", Resume => "resume" }
}
values! {
    /// Where the updater got to.
    UpdateStep { Staged => "staged", Applied => "applied", Failed => "failed", RolledBack => "rolled_back" }
}
values! {
    /// What the bell got a notice about.
    Notice { UpdateReady => "update_ready", Updated => "updated", Daemon => "daemon", ConfigError => "config_error", ThemeError => "theme_error" }
}
values! {
    /// Something that went wrong where the person would have seen it.
    Failure {
        DaemonUnreachable => "daemon_unreachable",
        DaemonRestarted => "daemon_restarted",
        SessionLost => "session_lost",
        ChromeMissing => "chrome_missing",
        MermaidFailed => "mermaid_failed",
        SaveFailed => "save_failed",
        VideoUnplayable => "video_unplayable",
    }
}
values! {
    /// Which command an agent ran against the window's panes.
    // No call site yet: `agent_pane` is called by Task 8.
    #[allow(dead_code)]
    AgentCommand {
        Open => "open",
        Status => "status",
        Play => "play",
        Pause => "pause",
        Seek => "seek",
        Step => "step",
        Loop => "loop",
        Reload => "reload",
        Close => "close",
        Mute => "mute",
        Panes => "panes",
    }
}
values! {
    /// A Help surface.
    Help {
        Keybindings => "keybindings",
        Documentation => "documentation",
        About => "about",
        Telemetry => "telemetry",
    }
}
values! {
    /// What foreground time is counted against.
    Surface { Terminal => "terminal", Viewer => "viewer", Browser => "browser", Video => "video", Settings => "settings" }
}

/// The moment a call happens: unix seconds for the journal, and the monotonic clock the focus
/// time is measured on, which does not run while the Mac sleeps.
#[derive(Clone, Copy, Debug)]
pub struct Now {
    pub unix: u64,
    pub at: Instant,
}

impl Now {
    /// The system's clocks.
    pub fn system() -> Now {
        Now { unix: unix_now(), at: Instant::now() }
    }
}

/// The wall clock in unix seconds, as every telemetry file and receipt records it; a clock before
/// 1970 is 0.
pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// How often the focus totals are flushed to the journal while the window runs.
pub const MARK_EVERY: Duration = Duration::from_secs(60);

/// This process's journal and what it needs to keep one: the folder, the envelope and allowlist
/// a new journal is opened with, the usage it records under, and the per-session bookkeeping of
/// the events written once.
pub struct Recorder {
    dir: PathBuf,
    env: Envelope,
    allow: Allowlist,
    usage: Usage,
    /// The session id while a journal is open; `None` under `Off` or when it could not open.
    sid: Option<String>,
    journal: Option<Journal>,
    focus: Focus,
    /// The markdown features recorded in this session.
    md: BTreeSet<MdFeature>,
    /// The terminals known to run an agent, whether its launch was recorded or came before this
    /// window; kept across a new journal, since the agent is still the one it was.
    agents: HashSet<u64>,
    /// The terminals whose agent first asked for input in this session.
    waiting: HashSet<u64>,
    /// `window_open` was recorded in this run: a run that never opened a project window (the
    /// launch chooser, quit) records no close. Kept across a new journal, since the window is
    /// still the one that opened.
    opened: bool,
    /// `window_close` is written: every quit path asks, and the first one writes it.
    closed: bool,
    /// The config did not load at launch: usage stays `Off` for the whole run, whatever a later
    /// config says, until the next launch.
    pinned_off: bool,
}

/// The focus clock: what the project window and the Settings window show, since when, and the
/// milliseconds each surface has not yet handed to the journal as a whole second.
struct Focus {
    project: Option<Surface>,
    settings: bool,
    since: Instant,
    carry: BTreeMap<Surface, u64>,
}

impl Focus {
    /// What the person is looking at: Settings while its window is the key window, else what the
    /// project window shows (nothing while it is in the background).
    fn current(&self) -> Option<Surface> {
        if self.settings {
            Some(Surface::Settings)
        } else {
            self.project
        }
    }
}

impl Recorder {
    /// A recorder for this process, with a journal opened under a fresh id unless usage is `Off`.
    pub fn new(dir: &Path, env: Envelope, allow: Allowlist, usage: Usage, now: Now) -> Recorder {
        let focus = Focus { project: None, settings: false, since: now.at, carry: BTreeMap::new() };
        let mut r = Recorder { dir: dir.to_path_buf(), env, allow, usage, sid: None, journal: None, focus, md: BTreeSet::new(), agents: HashSet::new(), waiting: HashSet::new(), opened: false, closed: false, pinned_off: false };
        if usage != Usage::Off {
            r.start(now);
        }
        r
    }

    /// A recorder for a run whose config did not load at launch: no journal, now or later in the
    /// run (a config error means telemetry `Off` for that run, never the defaults).
    pub fn after_config_error(dir: &Path, env: Envelope, allow: Allowlist, now: Now) -> Recorder {
        let mut r = Recorder::new(dir, env, allow, Usage::Off, now);
        r.pinned_off = true;
        r
    }

    /// The session id the daemon is told, while a journal is open.
    #[cfg(test)]
    pub fn sid(&self) -> Option<&str> {
        self.sid.as_deref()
    }

    /// `journal/<sid>.jsonl`.
    fn journal_path(&self, sid: &str) -> PathBuf {
        self.dir.join("journal").join(format!("{sid}.jsonl"))
    }

    /// Opens a journal under a fresh id and starts the session's bookkeeping afresh; returns the
    /// id, or `None` when the journal could not be opened, which records nothing and says nothing.
    fn start(&mut self, now: Now) -> Option<String> {
        self.journal = None;
        self.sid = None;
        self.md.clear();
        self.waiting.clear();
        self.closed = false;
        // Nothing counted before the new journal belongs to it.
        self.focus.since = now.at;
        self.focus.carry.clear();
        let sid = new_id();
        let journal = Journal::open(&self.dir, &sid, &self.env, self.allow.clone(), now.unix).ok()?;
        self.journal = Some(journal);
        self.sid = Some(sid.clone());
        Some(sid)
    }

    /// Closes the running journal and deletes its file.
    fn discard(&mut self) {
        self.journal = None;
        if let Some(sid) = self.sid.take() {
            let _ = std::fs::remove_file(self.journal_path(&sid));
        }
    }

    /// The usage this window now runs under, as the config watcher saw it. Turning it `On` is the
    /// opt-in: the sessions queued under `Local` are deleted, the running journal too, and a new
    /// one starts, so only what is recorded from the consent on can leave. `Off` deletes the
    /// running journal; `Local` after `Off` starts one. Returns the new session id the window
    /// must send, when there is one.
    pub fn set_usage(&mut self, usage: Usage, now: Now) -> Option<String> {
        if self.pinned_off {
            return None;
        }
        let old = std::mem::replace(&mut self.usage, usage);
        match (old, usage) {
            (old, new) if old == new => None,
            (_, Usage::Off) => {
                self.discard();
                None
            }
            (_, Usage::On) => {
                let _ = Queue::new(&self.dir).clear_sessions();
                self.discard();
                self.start(now)
            }
            (Usage::Off, Usage::Local) => self.start(now),
            (_, Usage::Local) => None,
        }
    }

    /// The window's socket to the daemon dropped. The daemon seals the journal as `lost` (a
    /// restart, an upgrade, a stalled window dropped), so the window always starts a fresh one
    /// under a new id and leaves the old file to the daemon; the reconnect says the new id.
    pub fn socket_lost(&mut self, now: Now) {
        if self.usage != Usage::Off {
            self.start(now);
        }
    }

    /// The session id a connection says after its Hello: the running one, so every attempt of
    /// the first connect keeps the launch's. Should its journal be gone all the same (sealed by
    /// the daemon), a fresh one's: that file must not be written again.
    pub fn connect_sid(&mut self, now: Now) -> Option<String> {
        if self.usage == Usage::Off {
            return None;
        }
        match &self.sid {
            Some(sid) if self.journal.is_some() && self.journal_path(sid).exists() => Some(sid.clone()),
            _ => self.start(now),
        }
    }

    /// A config the window applied: the usage it gives (the opt-in, `set_usage`, comes first, so
    /// a new journal starts with it), a `settings` event per key changed and a `theme` event per
    /// theme chosen. `Some` when the session id changed: the new journal's for the window to
    /// send, or `None` once usage is off and no journal is kept.
    pub fn config_changed(&mut self, old: &Config, new: &Config, usage: Usage, now: Now) -> Option<Option<String>> {
        let before = self.sid.clone();
        self.set_usage(usage, now);
        for key in changed_keys(old, new) {
            self.record(events::settings(&key), now);
        }
        for (before, after) in [(&old.theme.dark, &new.theme.dark), (&old.theme.light, &new.theme.light)] {
            if before != after {
                self.theme(is_builtin(after), now);
            }
        }
        (self.sid != before).then(|| self.sid.clone())
    }

    /// `config_changed`, with a new session id said to the daemon at once on `daemon`, the
    /// connection there is (the launch chooser's before the project window, the window's own
    /// after). With none yet, the connect says it (`connect_sid`).
    pub fn config_applied<D: Daemon + ?Sized>(&mut self, old: &Config, new: &Config, usage: Usage, now: Now, daemon: Option<&D>) {
        if let (Some(sid), Some(daemon)) = (self.config_changed(old, new, usage, now), daemon) {
            daemon.set_sid(sid);
        }
    }

    /// The config the project window opens on, read again because the file changed since
    /// launch, against the one the Recorder last took (`applied`). One that failed to load
    /// (`failed`) is the launch config `reload_decision` fell back to, not a choice: the Recorder
    /// keeps what it has, so an off chosen in the chooser is not undone.
    pub fn opening_config<D: Daemon + ?Sized>(&mut self, applied: &Config, reloaded: &Config, failed: bool, usage: Usage, now: Now, daemon: Option<&D>) {
        if !failed {
            self.config_applied(applied, reloaded, usage, now, daemon);
        }
    }

    /// Says this recorder's session id on `daemon` when the connection says another: the id it
    /// was made with may have been read before a change in the chooser restarted the journal.
    pub fn sync_sid<D: Daemon + ?Sized>(&self, daemon: &D) {
        if daemon.sid() != self.sid {
            daemon.set_sid(self.sid.clone());
        }
    }

    /// Writes one event; the journal refuses and counts what the allowlist does not list, and a
    /// failed write is lost without a word.
    fn record(&mut self, ev: Event, now: Now) {
        if let Some(j) = &mut self.journal {
            j.event(ev, now.unix);
        }
    }

    /// Hands the time since the last change to the surface that had it: whole seconds to the
    /// journal, the rest kept for the next time.
    fn credit(&mut self, at: Instant) {
        let since = std::mem::replace(&mut self.focus.since, at);
        let Some(surface) = self.focus.current() else { return };
        let ms = self.focus.carry.entry(surface).or_default();
        *ms = ms.saturating_add(at.saturating_duration_since(since).as_millis() as u64);
        let secs = *ms / 1000;
        *ms %= 1000;
        if let (Some(j), true) = (&mut self.journal, secs > 0) {
            j.focus(surface.as_str(), secs);
        }
    }

    /// What the project window shows now (`None` while it is in the background).
    pub fn project_focus(&mut self, surface: Option<Surface>, now: Now) {
        if self.focus.project != surface {
            self.credit(now.at);
            self.focus.project = surface;
        }
    }

    /// Whether the Settings window is the key window.
    pub fn settings_focus(&mut self, active: bool, now: Now) {
        if self.focus.settings != active {
            self.credit(now.at);
            self.focus.settings = active;
        }
    }

    /// Flushes the focus totals: once a minute and at quit.
    pub fn mark(&mut self, now: Now) {
        self.credit(now.at);
        if let Some(j) = &mut self.journal {
            let _ = j.mark(now.unix);
        }
    }

    pub fn launch(&mut self, how: Launch, now: Now) {
        self.record(events::launch(how), now);
    }

    pub fn window_open(&mut self, now: Now) {
        self.opened = true;
        self.record(events::window_open(), now);
    }

    /// The window is going: written once, whichever quit paths run, and only after a
    /// `window_open`.
    pub fn window_close(&mut self, now: Now) {
        if self.opened && !std::mem::replace(&mut self.closed, true) {
            self.record(events::window_close(), now);
        }
    }

    /// The window is going: `window_close` (once) and the final mark.
    pub fn quitting(&mut self, now: Now) {
        self.window_close(now);
        self.mark(now);
    }

    pub fn pane_open(&mut self, kind: PaneKind, how: Opened, now: Now) {
        self.record(events::pane_open(kind, how), now);
    }

    pub fn pane_close(&mut self, kind: PaneKind, now: Now) {
        self.record(events::pane_close(kind), now);
    }

    pub fn view(&mut self, format: Format, shown: Shown, now: Now) {
        self.record(events::view(format, shown), now);
    }

    /// The first use of a markdown feature in this session; later ones write nothing.
    pub fn md(&mut self, feature: MdFeature, now: Now) {
        if self.journal.is_some() && self.md.insert(feature) {
            self.record(events::md(feature), now);
        }
    }

    /// An agent started in `terminal`: recorded once per terminal.
    pub fn agent_launch(&mut self, terminal: u64, agent: Agent, how: AgentHow, now: Now) {
        if self.agents.insert(terminal) {
            self.record(events::agent_launch(agent, how), now);
        }
    }

    /// `terminal` ran its agent before this window knew of it: no launch is recorded for it.
    pub fn agent_known(&mut self, terminal: u64) {
        self.agents.insert(terminal);
    }

    /// The agent in `terminal` first asked for input in this session.
    pub fn agent_state_waiting(&mut self, terminal: u64, agent: Agent, now: Now) {
        if self.journal.is_some() && self.waiting.insert(terminal) {
            self.record(events::agent_state_waiting(agent), now);
        }
    }

    pub fn palette(&mut self, command: &Command, now: Now) {
        self.record(events::palette(command), now);
    }

    pub fn theme(&mut self, builtin: bool, now: Now) {
        self.record(events::theme(builtin), now);
    }

    pub fn update(&mut self, step: UpdateStep, now: Now) {
        self.record(events::update(step), now);
    }

    pub fn notice(&mut self, kind: Notice, now: Now) {
        self.record(events::notice(kind), now);
    }

    pub fn error(&mut self, kind: Failure, now: Now) {
        self.record(events::error(kind), now);
    }

    pub fn agent_pane(&mut self, command: AgentCommand, ok: bool, now: Now) {
        self.record(events::agent_pane(command, ok), now);
    }

    pub fn help(&mut self, what: Help, now: Now) {
        self.record(events::help(what), now);
    }
}

/// The events each helper writes, built apart from the journal so a test can walk every value.
mod events {
    use super::*;

    fn ev(n: &str, d: &[(&str, &str)]) -> Event {
        Event { n: n.into(), t: 0, d: d.iter().map(|(k, v)| (k.to_string(), DValue::Str(v.to_string()))).collect() }
    }

    pub fn launch(how: Launch) -> Event {
        ev("launch", &[("how", how.as_str())])
    }

    pub fn window_open() -> Event {
        ev("window_open", &[])
    }

    pub fn window_close() -> Event {
        ev("window_close", &[])
    }

    pub fn pane_open(kind: PaneKind, how: Opened) -> Event {
        ev("pane_open", &[("kind", kind.as_str()), ("how", how.as_str())])
    }

    pub fn pane_close(kind: PaneKind) -> Event {
        ev("pane_close", &[("kind", kind.as_str())])
    }

    pub fn view(format: Format, shown: Shown) -> Event {
        ev("view", &[("format", format.as_str()), ("mode", shown.as_str())])
    }

    pub fn md(feature: MdFeature) -> Event {
        ev("md", &[("feature", feature.as_str())])
    }

    pub fn agent_launch(agent: Agent, how: AgentHow) -> Event {
        ev("agent_launch", &[("agent", agent.as_str()), ("how", how.as_str())])
    }

    pub fn agent_state_waiting(agent: Agent) -> Event {
        ev("agent_state", &[("agent", agent.as_str()), ("state", "waiting")])
    }

    pub fn palette(command: &Command) -> Event {
        ev("palette", &[("command", command_name(command))])
    }

    /// A config key from `changed_keys`: a name the config's own fields give, never a value, and
    /// still checked against the allowlist by the journal.
    pub fn settings(key: &str) -> Event {
        ev("settings", &[("key", key)])
    }

    pub fn theme(builtin: bool) -> Event {
        Event { n: "theme".into(), t: 0, d: [("builtin".to_string(), DValue::Bool(builtin))].into() }
    }

    pub fn update(step: UpdateStep) -> Event {
        ev("update", &[("step", step.as_str())])
    }

    pub fn notice(kind: Notice) -> Event {
        ev("notice", &[("kind", kind.as_str())])
    }

    pub fn error(kind: Failure) -> Event {
        ev("error", &[("kind", kind.as_str())])
    }

    pub fn agent_pane(command: AgentCommand, ok: bool) -> Event {
        Event { n: "agent_pane".into(), t: 0, d: [("command".to_string(), DValue::Str(command.as_str().into())), ("ok".to_string(), DValue::Bool(ok))].into() }
    }

    pub fn help(what: Help) -> Event {
        ev("help", &[("what", what.as_str())])
    }
}

/// A `keymap::Command`'s name in snake case, as the allowlist lists the palette's commands; the
/// argument a command carries (a pane, a theme's name, a preset's) is never part of it.
pub fn command_name(command: &Command) -> &'static str {
    use Command::*;
    match command {
        FocusPane(_) => "focus_pane",
        FocusDir(_) => "focus_dir",
        Split(_) => "split",
        ToggleMaximize => "toggle_maximize",
        MoveTabToPane(_) => "move_tab_to_pane",
        CycleTab(_) => "cycle_tab",
        MruTab => "mru_tab",
        CloseTab => "close_tab",
        ReopenClosedTab => "reopen_closed_tab",
        CloseAllInPane => "close_all_in_pane",
        MoveTabWithin(_) => "move_tab_within",
        NewTerminal => "new_terminal",
        ClearToStart => "clear_to_start",
        FontDelta(_) => "font_delta",
        About => "about",
        OpenSettings => "open_settings",
        Keybindings => "keybindings",
        Documentation => "documentation",
        OpenConfig => "open_config",
        TextPreset(_) => "text_preset",
        ReloadConfig => "reload_config",
        ExtractThemes => "extract_themes",
        CommandPalette => "command_palette",
        SetTheme(_) => "set_theme",
        Find => "find",
        CopyAsShown => "copy_as_shown",
        UpdateNow => "update_now",
        CheckForUpdates => "check_for_updates",
        Notifications => "notifications",
        Doctor => "doctor",
        RetryUpdate => "retry_update",
        StartAtLogin => "start_at_login",
        InstallHooks => "install_hooks",
        InstallAgentGuide => "install_agent_guide",
        ToggleSidebar => "toggle_sidebar",
        FocusFileTree => "focus_file_tree",
        ShowIgnoredFiles => "show_ignored_files",
        FocusWorktrees => "focus_worktrees",
        ToggleInlineDiff => "toggle_inline_diff",
        ToggleMarkdownView => "toggle_markdown_view",
        SaveFile => "save_file",
        OpenFolder => "open_folder",
        NewWindow => "new_window",
        NewBrowserTab => "new_browser_tab",
        NewBrowserTabSinglePage => "new_browser_tab_single_page",
        DockChromeWindow => "dock_chrome_window",
        DockChromeWindowAt(_) => "dock_chrome_window_at",
        UndockChromeWindow => "undock_chrome_window",
        OpenFileInPane => "open_file_in_pane",
        VideoPlayPause => "video_play_pause",
        VideoBack => "video_back",
        VideoForward => "video_forward",
        VideoBackFine => "video_back_fine",
        VideoForwardFine => "video_forward_fine",
        VideoStepBack => "video_step_back",
        VideoStepForward => "video_step_forward",
        VideoStart => "video_start",
        VideoLoop => "video_loop",
        VideoMute => "video_mute",
        VideoReload => "video_reload",
    }
}

/// Whether a theme of this name is one Arkos ships; a theme is never named in an event.
fn is_builtin(name: &str) -> bool {
    crate::theme::BUILTIN.iter().any(|(n, _)| *n == name)
}

/// The config keys that differ between `old` and `new`, sorted, never their values: every
/// preset is the one key `font.presets` (a preset's name is the person's own) and the
/// `[keybindings]` table is `keybindings` (an action's name is a key there).
pub fn changed_keys(old: &Config, new: &Config) -> Vec<String> {
    match (toml::Value::try_from(old), toml::Value::try_from(new)) {
        (Ok(toml::Value::Table(old)), Ok(toml::Value::Table(new))) => changed_keys_of(&old, &new),
        _ => Vec::new(),
    }
}

/// `changed_keys` on the two configs as TOML tables. A key one side lacks (an unset shell) has
/// changed too.
fn changed_keys_of(old: &toml::Table, new: &toml::Table) -> Vec<String> {
    let empty = toml::Table::new();
    let mut keys = BTreeSet::new();
    for section in old.keys().chain(new.keys()) {
        let (a, b) = (old.get(section), new.get(section));
        if a == b {
            continue;
        }
        let tables = (a.and_then(toml::Value::as_table), b.and_then(toml::Value::as_table));
        if section == "keybindings" || tables == (None, None) {
            keys.insert(section.clone());
            continue;
        }
        let (a, b) = (tables.0.unwrap_or(&empty), tables.1.unwrap_or(&empty));
        for key in a.keys().chain(b.keys()).filter(|k| a.get(*k) != b.get(*k)) {
            keys.insert(if section == "font" && key == "presets" { "font.presets".to_string() } else { format!("{section}.{key}") });
        }
    }
    keys.into_iter().collect()
}

/// The format the viewer showed a file in, from what its load produced: markdown by its name,
/// binaries and files too large to read as such, text shown without colours for its size or a
/// long line (`plain`) as plain, and other text as code when a grammar other than Plain Text
/// coloured it (`coloured`, the highlighter's own rule), else plain. `None` for a file that could
/// not be read (nothing was shown) and for text whose colouring is not known yet (`coloured` is
/// `None` while the grammars load). An image or a video opened as a file is a binary; the two
/// formats name what the rendered markdown shows.
pub fn format_of(content: &Content, plain: bool, coloured: Option<bool>, path: &Path) -> Option<Format> {
    Some(match content {
        Content::Unreadable(_) => return None,
        Content::TooLarge(_) => Format::TooLarge,
        Content::Binary => Format::Binary,
        Content::Text { .. } if crate::viewer::is_markdown(path) => Format::Markdown,
        Content::Text { .. } if plain => Format::Plain,
        Content::Text { .. } => match coloured? {
            true => Format::Code,
            false => Format::Plain,
        },
    })
}

/// The view a viewer's load records, once a viewer (`viewed`): `None` when it was recorded
/// already, or when this load says nothing yet (unreadable, or code loaded before the grammars,
/// whose reload once they land records it); whether a markdown file is shown rendered.
pub fn next_view(viewed: &mut bool, content: &Content, plain: bool, coloured: Option<bool>, path: &Path, mode: crate::tree::ViewMode) -> Option<(Format, Shown)> {
    if *viewed {
        return None;
    }
    let format = format_of(content, plain, coloured, path)?;
    *viewed = true;
    let shown = if format == Format::Markdown && mode == crate::tree::ViewMode::Rendered { Shown::Rendered } else { Shown::Raw };
    Some((format, shown))
}

/// What a tab is, for `pane_open`, `pane_close` and the focus clock.
pub fn kind_of(tab: &Tab) -> PaneKind {
    match tab {
        Tab::Terminal { .. } => PaneKind::Terminal,
        Tab::Viewer { .. } => PaneKind::Viewer,
        Tab::Browser { .. } => PaneKind::Browser,
        Tab::Video { .. } => PaneKind::Video,
    }
}

impl Surface {
    /// A tab's surface.
    fn of(tab: &Tab) -> Surface {
        match kind_of(tab) {
            PaneKind::Terminal => Surface::Terminal,
            PaneKind::Viewer => Surface::Viewer,
            PaneKind::Browser => Surface::Browser,
            PaneKind::Video => Surface::Video,
        }
    }
}

impl Notice {
    /// The bell's kind of notice.
    fn of(kind: crate::notices::Kind) -> Notice {
        use crate::notices::Kind;
        match kind {
            Kind::UpdateReady => Notice::UpdateReady,
            Kind::Updated => Notice::Updated,
            Kind::Daemon => Notice::Daemon,
            Kind::ConfigError => Notice::ConfigError,
            Kind::ThemeError => Notice::ThemeError,
        }
    }
}

impl Agent {
    /// The agent an `AgentRecord`'s `kind` names.
    fn of(kind: &str) -> Agent {
        if kind == "claude" {
            Agent::Claude
        } else {
            Agent::Other
        }
    }
}

// The process's recorder and the calls the window makes on it. Each window is its own process,
// so there is one, set up by `init` before the first connect; until then (and in tests, which
// never call `init`) every call does nothing. A call never fails where the person could see it.

static RECORDER: Mutex<Option<Recorder>> = Mutex::new(None);

thread_local! {
    /// How the tab about to open is being opened, while a caller up the stack says so.
    static VIA: Cell<Option<Opened>> = const { Cell::new(None) };
}

fn with(f: impl FnOnce(&mut Recorder, Now)) {
    let mut guard = RECORDER.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(r) = guard.as_mut() {
        f(r, Now::system());
    }
}

/// The connection to the daemon as the recorder's calls use it: the window's `Client`, or a fake
/// in the tests.
pub trait Daemon {
    /// Says the window's session id now (`Client::set_sid`).
    fn set_sid(&self, sid: Option<String>);
    /// Says the session ended as a quit, on a quit that sends no Goodbye (`Client::session_quit`).
    fn session_quit(&self);
    /// The session id the connection says after each Hello (`Client::sid`).
    fn sid(&self) -> Option<String>;
}

impl Daemon for crate::client::Client {
    fn set_sid(&self, sid: Option<String>) {
        crate::client::Client::set_sid(self, sid);
    }

    fn session_quit(&self) {
        crate::client::Client::session_quit(self);
    }

    fn sid(&self) -> Option<String> {
        crate::client::Client::sid(self)
    }
}

/// Sets this process's recorder up under the usage its config (`Err` when it did not load) and
/// the environment give, with a journal unless that is `Off`, and records the launch. Called
/// once in `main`, before the first connect, which says the session id it made.
pub fn init(config: Result<&::telemetry::TelemetryConfig, ()>, how: Launch) {
    let now = Now::system();
    let (dir, env, allow) = (crate::paths::telemetry_dir(), ::telemetry::envelope(::telemetry::Bin::App), ::telemetry::allowlist().clone());
    let mut r = match config {
        Ok(config) => Recorder::new(&dir, env, allow, usage_of(Ok(config)), now),
        Err(()) => Recorder::after_config_error(&dir, env, allow, now),
    };
    r.launch(how, now);
    *RECORDER.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(r);
}

/// The usage `config` gives with the environment's say, for the app (a person's, so
/// interactive).
fn usage_of(config: Result<&::telemetry::TelemetryConfig, ()>) -> Usage {
    ::telemetry::mode_for(config, true).usage
}

/// The window's socket to the daemon dropped (`Recorder::socket_lost`).
pub fn socket_lost() {
    with(|r, now| r.socket_lost(now));
}

/// The session id a new connection says after its Hello (`Recorder::connect_sid`). Every connect
/// asks: the first and each reconnect.
pub fn connect_sid() -> Option<String> {
    let mut sid = None;
    with(|r, now| sid = r.connect_sid(now));
    sid
}

/// A config the window applied, a new session id said on `daemon` (`Recorder::config_applied`):
/// the project window's reloads, the config it opens on, and a change made in the launch
/// chooser's Settings.
pub fn config_applied<D: Daemon + ?Sized>(old: &Config, new: &Config, daemon: Option<&D>) {
    let usage = usage_of(Ok(&new.telemetry));
    with(|r, now| r.config_applied(old, new, usage, now, daemon));
}

/// What the project window shows for the focus clock: the focused pane's active tab while it is
/// the key window.
pub fn surface_of(tree: &Tree, active: bool) -> Option<Surface> {
    active.then(|| tree.pane(tree.focused).and_then(|p| p.tabs.get(p.active)).map(Surface::of)).flatten()
}

/// The project window's focus changed (`surface_of`).
pub fn window_focus(surface: Option<Surface>) {
    with(|r, now| r.project_focus(surface, now));
}

/// Whether the Settings window is the key window.
pub fn settings_focus(active: bool) {
    with(|r, now| r.settings_focus(active, now));
}

/// The minute's flush of the focus totals.
pub fn mark() {
    with(|r, now| r.mark(now));
}

/// The config the project window opens on, when the file changed since launch
/// (`Recorder::opening_config`): `failed` when it did not load.
pub fn opening_config<D: Daemon + ?Sized>(applied: &Config, reloaded: &Config, failed: bool, daemon: Option<&D>) {
    let usage = usage_of(Ok(&reloaded.telemetry));
    with(|r, now| r.opening_config(applied, reloaded, failed, usage, now, daemon));
}

/// The launch's connection is stored: it says the Recorder's session id if it differs
/// (`Recorder::sync_sid`).
pub fn sync_sid<D: Daemon + ?Sized>(daemon: &D) {
    with(|r, _| r.sync_sid(daemon));
}

/// The window is going: `window_close` (once) and the final mark, before the Goodbye on the
/// user's quit.
pub fn quitting() {
    with(|r, now| r.quitting(now));
}

/// The window is going on a quit that sends no Goodbye (the launch chooser's, macOS's terminate
/// at a logout or restart, the update relaunch): `quitting`, then `SessionQuit` on `daemon`, so
/// the daemon seals the session as a quit rather than as `lost`. The caller waits for the writer.
pub fn quit_without_goodbye<D: Daemon + ?Sized>(daemon: Option<&D>) {
    let mut guard = RECORDER.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    quit_unsaid(guard.as_mut(), Now::system(), daemon);
}

/// `quit_without_goodbye` on the process's recorder, when there is one: the final mark is
/// written before the quit is said, and the quit is said with no recorder too.
fn quit_unsaid<D: Daemon + ?Sized>(r: Option<&mut Recorder>, now: Now, daemon: Option<&D>) {
    if let Some(r) = r {
        r.quitting(now);
    }
    if let Some(daemon) = daemon {
        daemon.session_quit();
    }
}

pub fn window_open() {
    with(|r, now| r.window_open(now));
}

/// Says how the tabs opened while the returned guard lives are opened, unless a caller further
/// up already said: the palette and a key binding run the same commands, and a link opens a file
/// through the same call as the file tree.
#[must_use]
pub fn opened_via(how: Opened) -> Via {
    VIA.with(|via| match via.get() {
        Some(_) => Via(None),
        None => {
            via.set(Some(how));
            Via(Some(None))
        }
    })
}

/// Says again the origin a prompt or a picker kept from when it opened (`current_via`), over
/// whatever runs its submit (the palette's own list, a key in the prompt).
#[must_use]
pub fn resumed_via(how: Opened) -> Via {
    VIA.with(|via| Via(Some(via.replace(Some(how)))))
}

/// How a tab opened now would be opened: what a caller up the stack said, else a click. A prompt
/// or a picker keeps it when it opens.
pub fn current_via() -> Opened {
    VIA.with(|via| via.get()).unwrap_or(Opened::Click)
}

/// Puts back, when it goes, what was said before the guard said its own (`None` when it said
/// nothing).
pub struct Via(Option<Option<Opened>>);

impl Drop for Via {
    fn drop(&mut self) {
        if let Some(before) = self.0 {
            VIA.with(|via| via.set(before));
        }
    }
}

/// A tab opened: `how` unless a caller up the stack said otherwise (`opened_via`).
pub fn pane_open(kind: PaneKind, how: Opened) {
    let how = VIA.with(|via| via.get()).unwrap_or(how);
    with(|r, now| r.pane_open(kind, how, now));
}

/// The tabs a saved layout brought back, each opened by the restore.
pub fn restored(tree: &Tree) {
    with(|r, now| {
        for (_, _, tab) in tree.tabs() {
            r.pane_open(kind_of(tab), Opened::Restore, now);
        }
    });
}

pub fn pane_close(tab: &Tab) {
    with(|r, now| r.pane_close(kind_of(tab), now));
}

/// A viewer's load landed: the view, once a viewer, when its format is known (`next_view`).
pub fn viewed(viewed: &mut bool, content: &Content, plain: bool, coloured: Option<bool>, path: &Path, mode: crate::tree::ViewMode) {
    if let Some((format, shown)) = next_view(viewed, content, plain, coloured, path, mode) {
        with(|r, now| r.view(format, shown, now));
    }
}

pub fn md(feature: MdFeature) {
    with(|r, now| r.md(feature, now));
}

/// A session in the daemon's list runs an agent: a resumed one was started again by the
/// daemon's recovery; any other ran before this window knew of it.
pub fn agent_listed(terminal: u64, kind: &str, recovered: bool) {
    with(|r, now| if recovered { r.agent_launch(terminal, Agent::of(kind), AgentHow::Resume, now) } else { r.agent_known(terminal) });
}

/// A hook said what the agent in `terminal` is doing (the hooks are Claude Code's): the first
/// word from a terminal not known to run one is its launch, and the first `waiting` in a
/// session is recorded once.
pub fn agent_state(terminal: u64, state: protocol::ClaudeState) {
    with(|r, now| {
        r.agent_launch(terminal, Agent::Claude, AgentHow::New, now);
        if state == protocol::ClaudeState::Waiting {
            r.agent_state_waiting(terminal, Agent::Claude, now);
        }
    });
}

pub fn palette(command: &Command) {
    with(|r, now| r.palette(command, now));
}

/// A theme chosen by name (the palette's "Theme: ..."), recorded only as built in or not.
pub fn theme_named(name: &str) {
    let builtin = is_builtin(name);
    with(|r, now| r.theme(builtin, now));
}

pub fn update(step: UpdateStep) {
    with(|r, now| r.update(step, now));
}

pub fn notice(kind: crate::notices::Kind) {
    with(|r, now| r.notice(Notice::of(kind), now));
}

pub fn error(kind: Failure) {
    with(|r, now| r.error(kind, now));
}

/// A video tab showed its first frame.
pub fn video_shown() {
    with(|r, now| r.view(Format::Video, Shown::Rendered, now));
}

// No call site yet: Task 8 records an agent's `arkos open`, `arkos video` and `arkos panes`.
#[allow(dead_code)]
pub fn agent_pane(command: AgentCommand, ok: bool) {
    with(|r, now| r.agent_pane(command, ok, now));
}

pub fn help(what: Help) {
    with(|r, now| r.help(what, now));
}

// The invitation (telemetry spec, section 3.3): a strip under the first pane's tab bar that asks
// once, on a later day than the install, whether to turn usage reports on. Either answer writes
// `asked` in the telemetry folder, which every window reads before it draws the strip, so a
// second window never asks again once the first has had its answer.

/// The strip's words and its buttons, as `docs/telemetry.md` quotes them.
pub const INVITATION: &str = "Help improve Arkos: share anonymous usage reports. Nothing identifies you or your files.";
pub const TURN_ON: &str = "Turn on";
pub const NOT_NOW: &str = "Not now";
pub const WHAT_IS_SENT: &str = "What is sent";

/// The file in the telemetry folder that says the invitation was answered.
pub const ASKED: &str = "asked";

/// The two answers, each written to `asked` as a word so the file says which one it was.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Answer {
    TurnOn,
    NotNow,
}

/// Whether the invitation shows: on a later UTC day than the install (the second launch, since
/// each window is its own process and a counter could not say it), while the mode invites (a
/// release build, interactive, usage `local`, no rule of the environment), and only while no
/// window has had an answer.
pub fn invitation_due(mode: &::telemetry::visits_app::Mode, installed_day: u64, today: u64, asked: bool) -> bool {
    mode.invite && today > installed_day && !asked
}

/// How often a window showing the invitation looks for `asked` again, besides when it becomes the
/// key window: another window may have had its answer meanwhile.
pub const ASKED_EVERY: Duration = Duration::from_secs(30);

/// Whether the invitation shows in one window, kept so that a frame reads nothing: the mode is
/// the one the last config change set (`set_mode`), and `asked` is read the first time the strip
/// would show, then again only after a `recheck`.
pub struct Invitation {
    /// The install's UTC day and this launch's, while the strip can still show in this run.
    days: Option<(u64, u64)>,
    mode: ::telemetry::visits_app::Mode,
    /// `asked` as last read; `None` until the next frame that would show the strip reads it.
    asked: Option<bool>,
}

impl Invitation {
    /// The invitation for this run: `days` from the install and the launch (`None` when it can
    /// never show in this run), and the mode the config at launch gives.
    pub fn new(days: Option<(u64, u64)>, mode: ::telemetry::visits_app::Mode) -> Invitation {
        Invitation { days, mode, asked: None }
    }

    /// The config changed: the mode it gives now.
    pub fn set_mode(&mut self, mode: ::telemetry::visits_app::Mode) {
        self.mode = mode;
    }

    /// Reads `asked` again at the next frame the strip would show: on activation and every
    /// `ASKED_EVERY`.
    pub fn recheck(&mut self) {
        self.asked = None;
    }

    /// Whether the strip can still show in this run; once it cannot, nothing rechecks.
    pub fn possible(&self) -> bool {
        self.days.is_some()
    }

    /// Whether the strip shows this frame (`invitation_due`), with `asked` read by `look` only
    /// when nothing else hides it and no read since the last recheck says. Once it says the
    /// invitation was answered, this run never asks again.
    pub fn shows(&mut self, look: impl FnOnce() -> bool) -> bool {
        let Some((installed, today)) = self.days else { return false };
        if !invitation_due(&self.mode, installed, today, false) {
            return false;
        }
        let asked = *self.asked.get_or_insert_with(look);
        if asked {
            self.days = None;
        }
        !asked
    }

    /// This window had its answer.
    pub fn answered(&mut self) {
        self.days = None;
    }
}

/// `at` as days since 1970 in UTC; a clock before 1970 is day 0.
pub fn utc_day(at: SystemTime) -> u64 {
    at.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs() / 86_400)
}

/// Whether the invitation was answered: `asked` is in the telemetry folder `dir`.
pub fn asked(dir: &Path) -> bool {
    dir.join(ASKED).symlink_metadata().is_ok()
}

/// The mode for `usage` and `crashes` in a build of `channel`, with one environment variable set
/// (or none) and the session interactive or not: for the tests here, in Settings and in the
/// crash dialog.
#[cfg(test)]
pub fn mode_of(usage: Usage, crashes: ::telemetry::visits_app::Crashes, channel: ::telemetry::visits_app::Channel, var: Option<(&'static str, &'static str)>, interactive: bool) -> ::telemetry::visits_app::Mode {
    let config = ::telemetry::TelemetryConfig { usage, crashes };
    let env = move |k: &str| var.filter(|(name, _)| *name == k).map(|(_, v)| v.to_string());
    ::telemetry::mode_with(Ok::<_, ::telemetry::Error>(&config), &env, &channel, interactive)
}

/// The mode an interactive release build gets from a config file that did not load: for the
/// same tests.
#[cfg(test)]
pub fn mode_of_broken_config() -> ::telemetry::visits_app::Mode {
    ::telemetry::mode_with(Err::<&::telemetry::TelemetryConfig, _>("line 1: bad"), &|_: &str| None, &::telemetry::visits_app::Channel::Release, true)
}

/// Writes `asked` into the telemetry folder `dir`, creating the folder when it is missing.
pub fn answer_invitation(dir: &Path, answer: Answer) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let word = match answer {
        Answer::TurnOn => "on",
        Answer::NotNow => "not now",
    };
    std::fs::write(dir.join(ASKED), format!("{word}\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::telemetry::visits_app::{Arch, Channel, Crashes, Os};
    use std::time::Duration;

    fn env() -> Envelope {
        Envelope { app: "arkos".into(), ver: "0.9.0".into(), bin: "app".into(), os: Os::Macos, osv: 15, arch: Arch::Arm64, ch: Channel::Release }
    }

    /// A moment `secs` after `base`.
    fn at(base: Instant, secs: u64) -> Now {
        Now { unix: 1_792_141_200 + secs, at: base + Duration::from_secs(secs) }
    }

    fn recorder(dir: &Path, usage: Usage, now: Now) -> Recorder {
        Recorder::new(dir, env(), ::telemetry::allowlist().clone(), usage, now)
    }

    fn journal_path(dir: &Path, sid: &str) -> PathBuf {
        dir.join("journal").join(format!("{sid}.jsonl"))
    }

    /// The journal's lines, parsed.
    fn lines(dir: &Path, sid: &str) -> Vec<serde_json::Value> {
        std::fs::read_to_string(journal_path(dir, sid)).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    /// The events the journal holds, as (name, d).
    fn events_in(dir: &Path, sid: &str) -> Vec<(String, serde_json::Value)> {
        lines(dir, sid).into_iter().filter_map(|l| l.get("ev").map(|e| (e["n"].as_str().unwrap().to_string(), e.get("d").cloned().unwrap_or_default()))).collect()
    }

    /// The journal's last mark.
    fn last_mark(dir: &Path, sid: &str) -> serde_json::Value {
        lines(dir, sid).into_iter().filter_map(|l| l.get("mark").cloned()).last().expect("a mark")
    }

    fn admitted(ev: &Event) {
        assert_eq!(::telemetry::allowlist().check(ev), Ok(()), "{ev:?}");
    }

    /// Review Focus 2: every value every helper can write is one `telemetry/events.toml` lists.
    #[test]
    fn every_helper_writes_only_values_the_allowlist_lists() {
        for &how in Launch::ALL {
            admitted(&events::launch(how));
        }
        admitted(&events::window_open());
        admitted(&events::window_close());
        for &kind in PaneKind::ALL {
            for &how in Opened::ALL {
                admitted(&events::pane_open(kind, how));
            }
            admitted(&events::pane_close(kind));
        }
        for &format in Format::ALL {
            for &shown in Shown::ALL {
                admitted(&events::view(format, shown));
            }
        }
        for &feature in MdFeature::ALL {
            admitted(&events::md(feature));
        }
        for &agent in Agent::ALL {
            for &how in AgentHow::ALL {
                admitted(&events::agent_launch(agent, how));
            }
            admitted(&events::agent_state_waiting(agent));
        }
        for command in crate::config::tests::every_command() {
            admitted(&events::palette(&command));
        }
        for builtin in [true, false] {
            admitted(&events::theme(builtin));
        }
        for &step in UpdateStep::ALL {
            admitted(&events::update(step));
        }
        for &kind in Notice::ALL {
            admitted(&events::notice(kind));
        }
        for &kind in Failure::ALL {
            admitted(&events::error(kind));
        }
        for &what in Help::ALL {
            admitted(&events::help(what));
        }
        for &command in AgentCommand::ALL {
            for ok in [true, false] {
                admitted(&events::agent_pane(command, ok));
            }
        }
        for &surface in Surface::ALL {
            assert!(::telemetry::allowlist().allows_surface(surface.as_str()), "{surface:?}");
        }
        // Every key the config diff can name (each one the defaults hold, plus the two they leave
        // out) is listed.
        let none = Config { terminal: crate::config::TerminalConfig { shell: Some("/bin/zsh".into()), ..Config::default().terminal }, keybindings: [("Split right".to_string(), "cmd-d".to_string())].into(), ..Config::default() };
        let toml::Value::Table(all) = toml::Value::try_from(&none).unwrap() else { panic!("a table") };
        for key in changed_keys_of(&all, &toml::Table::new()) {
            admitted(&events::settings(&key));
        }
    }

    /// The names follow the variants, so a variant added to `Command` names itself; the
    /// allowlist test in `config.rs` then asks for it in `events.toml`.
    #[test]
    fn the_palette_names_each_command_in_snake_case() {
        for command in crate::config::tests::every_command() {
            let debug = format!("{command:?}");
            let mut snake = String::new();
            for (i, ch) in debug.chars().take_while(|ch| ch.is_alphanumeric()).enumerate() {
                if ch.is_uppercase() && i > 0 {
                    snake.push('_');
                }
                snake.push(ch.to_ascii_lowercase());
            }
            assert_eq!(command_name(&command), snake);
        }
    }

    /// Every helper through a real journal: each event lands as one line, and the mark counts
    /// nothing refused and nothing dropped.
    #[test]
    fn the_journal_takes_every_helper_without_a_refusal() {
        let dir = tempfile::tempdir().unwrap();
        let base = Instant::now();
        let mut r = recorder(dir.path(), Usage::Local, at(base, 0));
        let sid = r.sid().expect("a journal under local").to_string();
        let now = at(base, 1);
        r.launch(Launch::Updated, now);
        r.window_open(now);
        r.pane_open(PaneKind::Viewer, Opened::Explorer, now);
        r.pane_close(PaneKind::Browser, now);
        r.view(Format::TooLarge, Shown::Raw, now);
        r.md(MdFeature::InlineDiff, now);
        r.agent_launch(7, Agent::Claude, AgentHow::Resume, now);
        r.agent_state_waiting(7, Agent::Claude, now);
        r.palette(&Command::DockChromeWindowAt(3), now);
        r.theme(false, now);
        r.update(UpdateStep::RolledBack, now);
        r.notice(Notice::ThemeError, now);
        r.error(Failure::SaveFailed, now);
        r.help(Help::Keybindings, now);
        r.window_close(now);
        r.mark(at(base, 2));
        let names: Vec<String> = events_in(dir.path(), &sid).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["launch", "window_open", "pane_open", "pane_close", "view", "md", "agent_launch", "agent_state", "palette", "theme", "update", "notice", "error", "help", "window_close"]);
        let mark = last_mark(dir.path(), &sid);
        assert_eq!((mark["rejected"].as_u64(), mark["dropped"].as_u64()), (Some(0), Some(0)), "{mark}");
        let events = events_in(dir.path(), &sid);
        assert_eq!(events[2].1, serde_json::json!({ "kind": "viewer", "how": "explorer" }));
        assert_eq!(events[7].1, serde_json::json!({ "agent": "claude", "state": "waiting" }));
        assert_eq!(events[8].1, serde_json::json!({ "command": "dock_chrome_window_at" }), "the name, never the argument");
        assert_eq!(events[9].1, serde_json::json!({ "builtin": false }), "a boolean, as the allowlist says");
    }

    /// The window closes once in a journal, whichever quit paths run.
    #[test]
    fn the_window_closes_once() {
        let dir = tempfile::tempdir().unwrap();
        let base = Instant::now();
        let mut r = recorder(dir.path(), Usage::On, at(base, 0));
        let sid = r.sid().unwrap().to_string();
        r.window_open(at(base, 1));
        r.window_close(at(base, 1));
        r.window_close(at(base, 2));
        let closes = events_in(dir.path(), &sid).into_iter().filter(|(n, _)| n == "window_close").count();
        assert_eq!(closes, 1);
    }

    #[test]
    fn the_focus_clock_counts_each_surface_across_three_changes() {
        let dir = tempfile::tempdir().unwrap();
        let base = Instant::now();
        let mut r = recorder(dir.path(), Usage::Local, at(base, 0));
        let sid = r.sid().unwrap().to_string();
        // A terminal from 0 to 5, a viewer from 5 to 12, Settings from 12 to 20, the viewer
        // again from 20 to 30.
        r.project_focus(Some(Surface::Terminal), at(base, 0));
        r.project_focus(Some(Surface::Viewer), at(base, 5));
        r.settings_focus(true, at(base, 12));
        // The project window going inactive under Settings changes nothing that is counted.
        r.project_focus(None, at(base, 13));
        r.project_focus(Some(Surface::Viewer), at(base, 19));
        r.settings_focus(false, at(base, 20));
        r.mark(at(base, 30));
        let mark = last_mark(dir.path(), &sid);
        assert_eq!(mark["focus"], serde_json::json!({ "terminal": 5, "viewer": 17, "settings": 8 }), "{mark}");
        assert_eq!(mark["t"], 30);
        // A window in the background counts for nothing; the next mark keeps the totals.
        r.project_focus(None, at(base, 30));
        r.mark(at(base, 90));
        assert_eq!(last_mark(dir.path(), &sid)["focus"], serde_json::json!({ "terminal": 5, "viewer": 17, "settings": 8 }));
        // Parts of a second add up rather than vanish.
        for i in 0..4 {
            let t = Now { unix: 1_792_141_300, at: base + Duration::from_millis(100_000 + i * 500) };
            r.project_focus(Some(if i % 2 == 0 { Surface::Browser } else { Surface::Terminal }), t);
        }
        r.project_focus(None, Now { unix: 1_792_141_302, at: base + Duration::from_millis(102_000) });
        r.mark(at(base, 110));
        assert_eq!(last_mark(dir.path(), &sid)["focus"], serde_json::json!({ "terminal": 6, "viewer": 17, "settings": 8, "browser": 1 }));
    }

    #[test]
    fn markdown_features_are_recorded_once_a_session() {
        let dir = tempfile::tempdir().unwrap();
        let base = Instant::now();
        let mut r = recorder(dir.path(), Usage::Local, at(base, 0));
        let sid = r.sid().unwrap().to_string();
        for i in 0..3 {
            r.md(MdFeature::Mermaid, at(base, i));
            r.md(MdFeature::Checklist, at(base, i));
        }
        let features: Vec<serde_json::Value> = events_in(dir.path(), &sid).into_iter().filter(|(n, _)| n == "md").map(|(_, d)| d["feature"].clone()).collect();
        assert_eq!(features, [serde_json::json!("mermaid"), serde_json::json!("checklist")]);
    }

    #[test]
    fn an_agent_is_recorded_once_per_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let base = Instant::now();
        let mut r = recorder(dir.path(), Usage::Local, at(base, 0));
        let sid = r.sid().unwrap().to_string();
        // One the daemon's list already knew: no launch is recorded for it.
        r.agent_known(1);
        r.agent_launch(1, Agent::Claude, AgentHow::New, at(base, 1));
        r.agent_launch(2, Agent::Claude, AgentHow::New, at(base, 2));
        r.agent_launch(2, Agent::Claude, AgentHow::New, at(base, 3));
        r.agent_state_waiting(2, Agent::Claude, at(base, 4));
        r.agent_state_waiting(2, Agent::Claude, at(base, 5));
        r.agent_state_waiting(1, Agent::Claude, at(base, 6));
        let names: Vec<String> = events_in(dir.path(), &sid).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["agent_launch", "agent_state", "agent_state"]);
    }

    #[test]
    fn usage_off_writes_no_journal() {
        let dir = tempfile::tempdir().unwrap();
        let base = Instant::now();
        let mut r = recorder(dir.path(), Usage::Off, at(base, 0));
        assert_eq!(r.sid(), None);
        assert_eq!(r.connect_sid(at(base, 1)), None, "nothing for the daemon to seal");
        r.launch(Launch::Normal, at(base, 1));
        r.window_open(at(base, 1));
        r.project_focus(Some(Surface::Terminal), at(base, 1));
        r.md(MdFeature::Details, at(base, 2));
        r.mark(at(base, 70));
        r.window_close(at(base, 71));
        assert!(!dir.path().join("journal").exists(), "no folder, no file");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "nothing written at all");
    }

    #[test]
    fn the_settings_diff_names_the_keys_never_the_values() {
        let old = Config::default();
        assert_eq!(changed_keys(&old, &old), Vec::<String>::new());
        let mut new = old.clone();
        new.font.family = "Iosevka Secret".into();
        new.font.presets.insert("my-own-preset".into(), crate::config::TextPreset { size: 21.0, line_height: 1.4 });
        new.keybindings.insert("Split right".into(), "cmd-shift-d".into());
        new.terminal.shell = Some("/Users/someone/bin/fish".into());
        new.telemetry.usage = Usage::On;
        let keys = changed_keys(&old, &new);
        assert_eq!(keys, ["font.family", "font.presets", "keybindings", "telemetry.usage", "terminal.shell"]);
        for key in &keys {
            admitted(&events::settings(key));
            for secret in ["Iosevka", "my-own-preset", "Split", "someone", "fish"] {
                assert!(!key.contains(secret), "{key}");
            }
        }
        // Back again names the same keys.
        assert_eq!(changed_keys(&new, &old), keys);
    }

    #[test]
    fn a_settings_edit_records_its_keys_and_a_chosen_theme() {
        let dir = tempfile::tempdir().unwrap();
        let base = Instant::now();
        let mut r = recorder(dir.path(), Usage::Local, at(base, 0));
        let sid = r.sid().unwrap().to_string();
        let old = Config::default();
        let mut new = old.clone();
        new.viewer.wide_tables = match old.viewer.wide_tables {
            crate::config::WideTables::Wrap => crate::config::WideTables::Scroll,
            crate::config::WideTables::Scroll => crate::config::WideTables::Wrap,
        };
        new.theme.dark = "my-midnight".into();
        assert_eq!(r.config_changed(&old, &new, Usage::Local, at(base, 1)), None, "the usage did not change, nor the sid");
        let mut newer = new.clone();
        newer.theme.light = "peach-light".into();
        r.config_changed(&new, &newer, Usage::Local, at(base, 2));
        let events = events_in(dir.path(), &sid);
        assert_eq!(
            events,
            [
                ("settings".to_string(), serde_json::json!({ "key": "theme.dark" })),
                ("settings".to_string(), serde_json::json!({ "key": "viewer.wide_tables" })),
                ("theme".to_string(), serde_json::json!({ "builtin": false })),
                ("settings".to_string(), serde_json::json!({ "key": "theme.light" })),
                ("theme".to_string(), serde_json::json!({ "builtin": true })),
            ]
        );
    }

    /// The opt-in (spec, "Opt-in"): what was queued under local goes, the running journal goes,
    /// and a new one starts under a fresh id that the window then sends.
    #[test]
    fn a_usage_change_to_on_restarts_the_journal_under_a_fresh_sid() {
        let dir = tempfile::tempdir().unwrap();
        let base = Instant::now();
        let mut r = recorder(dir.path(), Usage::Local, at(base, 0));
        let old_sid = r.sid().unwrap().to_string();
        r.md(MdFeature::Mermaid, at(base, 1));
        let queue = dir.path().join("queue");
        std::fs::create_dir_all(&queue).unwrap();
        let (sealed, crash) = (queue.join(format!("session-{}.json", new_id())), queue.join(format!("crash-{}.json", new_id())));
        std::fs::write(&sealed, "{}").unwrap();
        std::fs::write(&crash, "{}").unwrap();
        let old = Config::default();
        let mut on = old.clone();
        on.telemetry.usage = Usage::On;
        let new_sid = r.config_changed(&old, &on, Usage::On, at(base, 2)).expect("the sid changed").expect("a fresh sid to send");
        assert_ne!(new_sid, old_sid);
        assert!(::telemetry::is_sid(&new_sid));
        assert_eq!(r.sid(), Some(new_sid.as_str()));
        assert!(!journal_path(dir.path(), &old_sid).exists(), "the journal recorded before the consent is gone");
        assert!(!sealed.exists(), "sessions sealed under local are cleared");
        assert!(crash.exists(), "a crash stays");
        assert_eq!(Queue::new(dir.path()).due(u64::MAX).len(), 1);
        // The new journal starts with the opt-in itself, and the session's features count afresh.
        r.md(MdFeature::Mermaid, at(base, 3));
        let events = events_in(dir.path(), &new_sid);
        assert_eq!(events, [("settings".to_string(), serde_json::json!({ "key": "telemetry.usage" })), ("md".to_string(), serde_json::json!({ "feature": "mermaid" }))]);
        // On to local keeps the journal and its id; off deletes it and names none; local again
        // starts another under a new id.
        let mut local = on.clone();
        local.telemetry.usage = Usage::Local;
        assert_eq!(r.config_changed(&on, &local, Usage::Local, at(base, 4)), None);
        assert!(journal_path(dir.path(), &new_sid).exists());
        let mut off = local.clone();
        off.telemetry.usage = Usage::Off;
        assert_eq!(r.config_changed(&local, &off, Usage::Off, at(base, 5)), Some(None), "the window names no session");
        assert!(!journal_path(dir.path(), &new_sid).exists(), "off records nothing, and keeps nothing it recorded");
        assert_eq!(r.sid(), None);
        let third = r.config_changed(&off, &local, Usage::Local, at(base, 6)).expect("the sid changed").expect("a journal again");
        assert!(journal_path(dir.path(), &third).exists());
        assert_ne!(third, new_sid);
    }

    /// A fake connection to the daemon: what the window told it, in order. A quit also notes the
    /// journal's last line at that moment, so a test can see what was written before it.
    #[derive(Default)]
    struct FakeDaemon {
        calls: std::cell::RefCell<Vec<String>>,
        journal: Option<PathBuf>,
        /// The session id the connection says, as `Client` keeps it.
        said: std::cell::RefCell<Option<String>>,
    }

    impl Daemon for FakeDaemon {
        fn set_sid(&self, sid: Option<String>) {
            self.calls.borrow_mut().push(format!("sid {}", sid.as_deref().unwrap_or("none")));
            *self.said.borrow_mut() = sid;
        }

        fn sid(&self) -> Option<String> {
            self.said.borrow().clone()
        }

        fn session_quit(&self) {
            let last = self.journal.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| t.lines().last().map(str::to_string));
            self.calls.borrow_mut().push(format!("quit after {}", last.unwrap_or_default()));
        }
    }

    /// A usage change made while the launch chooser is up (in its Settings, or from the command
    /// line before the project window reads the file again) reaches the Recorder and the daemon:
    /// a change to on restarts the journal under a new id and says it on the chooser's
    /// connection; the same config applied again by the project window changes nothing; a change
    /// to off deletes the journal and names no session.
    #[test]
    fn a_usage_change_before_the_project_window_restarts_the_journal_and_says_the_new_sid() {
        let dir = tempfile::tempdir().unwrap();
        let base = Instant::now();
        let mut r = recorder(dir.path(), Usage::Local, at(base, 0));
        let first = r.sid().unwrap().to_string();
        let daemon = FakeDaemon::default();
        let launched = Config::default();
        let mut on = launched.clone();
        on.telemetry.usage = Usage::On;
        r.config_applied(&launched, &on, Usage::On, at(base, 1), Some(&daemon));
        let second = r.sid().expect("a journal under on").to_string();
        assert_ne!(second, first);
        assert!(!journal_path(dir.path(), &first).exists(), "nothing recorded before the consent is kept");
        assert!(journal_path(dir.path(), &second).exists());
        assert_eq!(*daemon.calls.borrow(), [format!("sid {second}")], "said at once");
        // The project window opens on the config the chooser already applied.
        r.config_applied(&on, &on, Usage::On, at(base, 2), Some(&daemon));
        assert_eq!(daemon.calls.borrow().len(), 1, "nothing changed, nothing said");
        let mut off = on.clone();
        off.telemetry.usage = Usage::Off;
        r.config_applied(&on, &off, Usage::Off, at(base, 3), Some(&daemon));
        assert_eq!(r.sid(), None);
        assert!(!journal_path(dir.path(), &second).exists(), "off keeps nothing it recorded");
        assert_eq!(daemon.calls.borrow().last().map(String::as_str), Some("sid none"));
        // With no connection yet the journal still restarts; the connect says the new id.
        let mut local = off.clone();
        local.telemetry.usage = Usage::Local;
        r.config_applied::<FakeDaemon>(&off, &local, Usage::Local, at(base, 4), None);
        assert!(r.sid().is_some());
    }

    /// A quit that sends no Goodbye (the launch chooser's, a macOS logout or restart, the update
    /// relaunch) writes `window_close` and the final mark first, then says `SessionQuit`.
    #[test]
    fn a_quit_without_a_goodbye_says_session_quit_after_the_final_mark() {
        let dir = tempfile::tempdir().unwrap();
        let base = Instant::now();
        let mut r = recorder(dir.path(), Usage::On, at(base, 0));
        let sid = r.sid().unwrap().to_string();
        let daemon = FakeDaemon { journal: Some(journal_path(dir.path(), &sid)), ..FakeDaemon::default() };
        r.window_open(at(base, 1));
        quit_unsaid(Some(&mut r), at(base, 5), Some(&daemon));
        let calls = daemon.calls.borrow().clone();
        assert_eq!(calls.len(), 1, "{calls:?}");
        let last: serde_json::Value = serde_json::from_str(calls[0].strip_prefix("quit after ").unwrap()).unwrap();
        assert_eq!(last["mark"]["t"], 5, "the final mark was written before the quit was said: {last}");
        assert_eq!(events_in(dir.path(), &sid).last().map(|(n, _)| n.as_str()), Some("window_close"));
        // Without a recorder (a run under off, or before `init`) the quit is said all the same.
        let alone = FakeDaemon::default();
        quit_unsaid(None, at(base, 6), Some(&alone));
        assert_eq!(*alone.calls.borrow(), ["quit after ".to_string()]);
    }

    /// A run that never opened a project window (the launch chooser, quit) records no close:
    /// `window_close` follows only a recorded `window_open`.
    #[test]
    fn a_run_with_no_window_open_records_no_close() {
        let dir = tempfile::tempdir().unwrap();
        let base = Instant::now();
        let mut r = recorder(dir.path(), Usage::On, at(base, 0));
        let sid = r.sid().unwrap().to_string();
        r.launch(Launch::Normal, at(base, 0));
        r.quitting(at(base, 3));
        let names: Vec<String> = events_in(dir.path(), &sid).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["launch"], "no close without an open");
        assert!(lines(dir.path(), &sid).iter().any(|l| l.get("mark").is_some()), "the final mark is still written");
    }

    /// The project window opens on a config that failed to load: the Recorder keeps what the
    /// chooser applied (here off), rather than the launch config that `reload_decision` falls
    /// back to. A config that loads is applied as usual.
    #[test]
    fn a_config_that_fails_to_load_at_the_project_window_is_not_applied() {
        let dir = tempfile::tempdir().unwrap();
        let base = Instant::now();
        let mut r = recorder(dir.path(), Usage::Local, at(base, 0));
        let launched = Config::default();
        let mut off = launched.clone();
        off.telemetry.usage = Usage::Off;
        r.config_applied::<FakeDaemon>(&launched, &off, Usage::Off, at(base, 1), None);
        assert_eq!(r.sid(), None);
        let daemon = FakeDaemon::default();
        r.opening_config(&off, &launched, true, Usage::Local, at(base, 2), Some(&daemon));
        assert_eq!(r.sid(), None, "off stays off");
        assert!(daemon.calls.borrow().is_empty());
        assert!(!dir.path().join("journal").read_dir().is_ok_and(|mut d| d.next().is_some()), "no journal");
        r.opening_config(&off, &launched, false, Usage::Local, at(base, 3), Some(&daemon));
        assert!(r.sid().is_some(), "a config that loads is applied");
        assert_eq!(daemon.calls.borrow().len(), 1);
    }

    /// The connection the launch made says the Recorder's session id when it differs, as when
    /// usage changed in the chooser between the connect thread reading the id and the
    /// connection being stored; when they agree, nothing is said.
    #[test]
    fn a_stored_connection_says_the_recorders_sid_when_it_differs() {
        let dir = tempfile::tempdir().unwrap();
        let base = Instant::now();
        let mut r = recorder(dir.path(), Usage::Local, at(base, 0));
        let first = r.sid().unwrap().to_string();
        let daemon = FakeDaemon { said: std::cell::RefCell::new(Some(first.clone())), ..FakeDaemon::default() };
        r.sync_sid(&daemon);
        assert!(daemon.calls.borrow().is_empty(), "the same id: nothing said");
        let launched = Config::default();
        let mut on = launched.clone();
        on.telemetry.usage = Usage::On;
        r.config_applied::<FakeDaemon>(&launched, &on, Usage::On, at(base, 1), None);
        let second = r.sid().unwrap().to_string();
        r.sync_sid(&daemon);
        assert_eq!(*daemon.calls.borrow(), [format!("sid {second}")]);
        r.sync_sid(&daemon);
        assert_eq!(daemon.calls.borrow().len(), 1, "said once");
    }

    /// After a dropped socket the daemon sealed and deleted the journal: the reconnect opens a
    /// fresh one under a new id rather than write to the old one again.
    #[test]
    fn a_reconnect_with_the_journal_gone_opens_a_fresh_one_under_a_new_sid() {
        let dir = tempfile::tempdir().unwrap();
        let base = Instant::now();
        let mut r = recorder(dir.path(), Usage::Local, at(base, 0));
        let first = r.sid().unwrap().to_string();
        assert_eq!(r.connect_sid(at(base, 1)).as_deref(), Some(first.as_str()), "the journal is there: the same session goes on");
        r.md(MdFeature::Details, at(base, 2));
        std::fs::remove_file(journal_path(dir.path(), &first)).unwrap();
        let second = r.connect_sid(at(base, 3)).expect("a fresh journal");
        assert_ne!(second, first);
        assert!(!journal_path(dir.path(), &first).exists(), "the old file is never written again");
        assert_eq!(r.connect_sid(at(base, 4)).as_deref(), Some(second.as_str()), "every later attempt keeps the new one");
        r.md(MdFeature::Details, at(base, 5));
        assert_eq!(events_in(dir.path(), &second), [("md".to_string(), serde_json::json!({ "feature": "details" }))]);
    }

    /// A lost socket always starts a fresh journal (the daemon seals the old one as `lost`), even
    /// while the old file is still there; the reconnect then says the new id.
    #[test]
    fn a_lost_socket_starts_a_fresh_journal_while_the_old_file_is_still_there() {
        let dir = tempfile::tempdir().unwrap();
        let base = Instant::now();
        let mut r = recorder(dir.path(), Usage::Local, at(base, 0));
        let first = r.sid().unwrap().to_string();
        r.md(MdFeature::Details, at(base, 1));
        r.socket_lost(at(base, 2));
        let second = r.sid().expect("a fresh journal").to_string();
        assert_ne!(second, first);
        assert!(journal_path(dir.path(), &first).exists(), "the old file is the daemon's to seal");
        assert!(journal_path(dir.path(), &second).exists());
        assert_eq!(r.connect_sid(at(base, 3)).as_deref(), Some(second.as_str()), "each attempt says the new id");
        assert_eq!(r.connect_sid(at(base, 4)).as_deref(), Some(second.as_str()));
        r.md(MdFeature::Details, at(base, 5));
        assert_eq!(events_in(dir.path(), &second), [("md".to_string(), serde_json::json!({ "feature": "details" }))]);
        assert_eq!(events_in(dir.path(), &first).len(), 1, "nothing more goes to the old one");
        // Under off a lost socket opens nothing.
        let off = tempfile::tempdir().unwrap();
        let mut r = recorder(off.path(), Usage::Off, at(base, 0));
        r.socket_lost(at(base, 1));
        assert_eq!(r.sid(), None);
        assert!(!off.path().join("journal").exists());
    }

    /// A config error at launch keeps usage off for the whole run: a config fixed meanwhile opens
    /// no journal until the next launch.
    #[test]
    fn a_config_error_at_launch_keeps_usage_off_for_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let base = Instant::now();
        let mut r = Recorder::after_config_error(dir.path(), env(), ::telemetry::allowlist().clone(), at(base, 0));
        assert_eq!(r.sid(), None);
        let old = Config::default();
        let mut on = old.clone();
        on.telemetry.usage = Usage::On;
        assert_eq!(r.config_changed(&old, &on, Usage::On, at(base, 1)), None, "no journal, no sid");
        assert_eq!(r.set_usage(Usage::Local, at(base, 2)), None);
        assert_eq!(r.connect_sid(at(base, 3)), None);
        r.socket_lost(at(base, 4));
        r.launch(Launch::Normal, at(base, 5));
        assert_eq!(r.sid(), None);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "nothing written at all");
    }

    /// A prompt or a picker keeps the origin it opened under, and its submit says it again over
    /// whatever runs the submit.
    #[test]
    fn a_prompt_keeps_the_origin_it_opened_under() {
        assert_eq!(current_via(), Opened::Click, "no caller said: a click");
        let kept = {
            let _key = opened_via(Opened::Key);
            assert_eq!(current_via(), Opened::Key);
            let _inner = opened_via(Opened::Palette);
            assert_eq!(current_via(), Opened::Key, "the outermost caller says how");
            current_via()
        };
        assert_eq!(current_via(), Opened::Click, "said only while the guard lives");
        {
            let _palette = opened_via(Opened::Palette);
            let _resumed = resumed_via(kept);
            assert_eq!(current_via(), Opened::Key, "the kept origin wins over the submit's own");
        }
        assert_eq!(current_via(), Opened::Click);
    }

    /// The highlighter's own rule: a grammar other than Plain Text makes a file code.
    #[test]
    fn a_makefile_is_code_and_an_unknown_extension_is_plain() {
        let theme = crate::theme::builtin("navy").unwrap();
        let h = crate::highlight::Highlighter::new(&theme.viewer, theme.chrome.text);
        let dir = tempfile::tempdir().unwrap();
        for (name, want) in [("Makefile", Format::Code), ("main.rs", Format::Code), ("notes.unknownext", Format::Plain), ("notes.txt", Format::Plain)] {
            let path = dir.path().join(name);
            std::fs::write(&path, "all:\n\techo hi\n").unwrap();
            let loaded = crate::viewer::read_file(&path, None, Some(&h), 1 << 20, false).unwrap();
            assert_eq!(format_of(&loaded.content, loaded.plain.is_some(), loaded.coloured, &path), Some(want), "{name}");
        }
        // Before the grammars load the colouring is not known yet.
        let path = dir.path().join("Makefile");
        assert_eq!(crate::viewer::read_file(&path, None, None, 1 << 20, false).unwrap().coloured, None);
        // Markdown, binaries and text shown plain do not wait for the grammars.
        let md = dir.path().join("README.md");
        std::fs::write(&md, "# Title\n").unwrap();
        assert_eq!(crate::viewer::read_file(&md, None, None, 1 << 20, false).unwrap().coloured, Some(false));
    }

    /// A restored tab usually loads before the grammars: that load records nothing, and the load
    /// that colours it once they are ready records the view, once.
    #[test]
    fn a_viewer_records_its_format_once_the_grammars_have_loaded() {
        let theme = crate::theme::builtin("navy").unwrap();
        let h = crate::highlight::Highlighter::new(&theme.viewer, theme.chrome.text);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.rs");
        std::fs::write(&path, "fn main() {}\n").unwrap();
        let mut viewed = false;
        let early = crate::viewer::read_file(&path, None, None, 1 << 20, false).unwrap();
        assert_eq!(next_view(&mut viewed, &early.content, early.plain.is_some(), early.coloured, &path, crate::tree::ViewMode::Rendered), None, "not known yet");
        assert!(!viewed);
        let coloured = crate::viewer::read_file(&path, None, Some(&h), 1 << 20, false).unwrap();
        assert_eq!(next_view(&mut viewed, &coloured.content, coloured.plain.is_some(), coloured.coloured, &path, crate::tree::ViewMode::Rendered), Some((Format::Code, Shown::Raw)));
        assert!(viewed);
        assert_eq!(next_view(&mut viewed, &coloured.content, coloured.plain.is_some(), coloured.coloured, &path, crate::tree::ViewMode::Rendered), None, "once a viewer");
        // Markdown records at its first load, rendered or raw as the tab shows it.
        let md = dir.path().join("README.md");
        std::fs::write(&md, "# Title\n").unwrap();
        let loaded = crate::viewer::read_file(&md, None, None, 1 << 20, false).unwrap();
        let mut viewed = false;
        assert_eq!(next_view(&mut viewed, &loaded.content, loaded.plain.is_some(), loaded.coloured, &md, crate::tree::ViewMode::Rendered), Some((Format::Markdown, Shown::Rendered)));
    }

    #[test]
    fn the_viewer_names_the_format_it_showed() {
        use crate::viewer::Content;
        let text = Content::Text { lines: std::sync::Arc::new(vec!["fn main() {}".into()]), spans: Vec::new() };
        assert_eq!(format_of(&text, false, Some(false), Path::new("/p/README.md")), Some(Format::Markdown));
        assert_eq!(format_of(&text, false, Some(true), Path::new("/p/main.rs")), Some(Format::Code));
        assert_eq!(format_of(&text, true, Some(false), Path::new("/p/main.rs")), Some(Format::Plain), "shown without colours");
        assert_eq!(format_of(&text, false, Some(false), Path::new("/p/notes.txt")), Some(Format::Plain), "no grammar but Plain Text");
        assert_eq!(format_of(&text, false, None, Path::new("/p/main.rs")), None, "the grammars have not loaded yet");
        assert_eq!(format_of(&Content::Binary, false, Some(false), Path::new("/p/logo.png")), Some(Format::Binary));
        assert_eq!(format_of(&Content::TooLarge(1 << 30), false, Some(false), Path::new("/p/dump.sql")), Some(Format::TooLarge));
        assert_eq!(format_of(&Content::Unreadable("denied".into()), false, Some(false), Path::new("/p/x")), None, "nothing was shown");
    }

    #[test]
    fn a_tab_is_a_terminal_a_viewer_or_a_browser() {
        assert_eq!(kind_of(&Tab::Terminal { session: protocol::SessionId(1), cwd: None }), PaneKind::Terminal);
        assert_eq!(kind_of(&Tab::Viewer { path: "/p/a.md".into(), mode: crate::tree::ViewMode::Raw }), PaneKind::Viewer);
        assert_eq!(kind_of(&Tab::Browser { url: None, app_mode: true }), PaneKind::Browser);
        assert_eq!(kind_of(&Tab::Video { path: "/p/a.mp4".into(), at: None, loop_range: None }), PaneKind::Video);
    }

    /// Review Focus 5: the invitation shows on a later UTC day than the install, in a release
    /// build, interactive, with usage `local` and `asked` absent; take any one away and it hides.
    #[test]
    fn the_invitation_is_due_only_on_a_later_day_in_a_release_with_usage_local_and_never_once_asked() {
        // 2026-10-16, as days since 1970.
        let installed = 20_742;
        let yes = mode_of(Usage::Local, Crashes::Ask, Channel::Release, None, true);
        assert!(invitation_due(&yes, installed, installed + 1, false), "the second day, everything right");
        assert!(invitation_due(&yes, installed, installed + 30, false), "any later day");
        assert!(!invitation_due(&yes, installed, installed, false), "never on the install day");
        assert!(!invitation_due(&yes, installed, installed - 1, false), "a clock set back is not a later day");
        assert!(!invitation_due(&yes, installed, installed + 1, true), "never again once answered");
        let hidden: [(&str, ::telemetry::visits_app::Mode); 9] = [
            ("a source build", mode_of(Usage::Local, Crashes::Ask, Channel::Source, None, true)),
            ("not interactive", mode_of(Usage::Local, Crashes::Ask, Channel::Release, None, false)),
            ("usage on", mode_of(Usage::On, Crashes::Ask, Channel::Release, None, true)),
            ("usage off", mode_of(Usage::Off, Crashes::Ask, Channel::Release, None, true)),
            ("DO_NOT_TRACK", mode_of(Usage::Local, Crashes::Ask, Channel::Release, Some(("DO_NOT_TRACK", "1")), true)),
            ("ARKOS_TELEMETRY=off", mode_of(Usage::Local, Crashes::Ask, Channel::Release, Some(("ARKOS_TELEMETRY", "off")), true)),
            ("ARKOS_TELEMETRY=log", mode_of(Usage::Local, Crashes::Ask, Channel::Release, Some(("ARKOS_TELEMETRY", "log")), true)),
            ("CI", mode_of(Usage::Local, Crashes::Ask, Channel::Release, Some(("CI", "true")), true)),
            (
                "a config error",
                mode_of_broken_config(),
            ),
        ];
        for (why, mode) in &hidden {
            assert!(!invitation_due(mode, installed, installed + 1, false), "hidden: {why}");
        }
        let unrelated = mode_of(Usage::Local, Crashes::Ask, Channel::Release, Some(("HOME", "/Users/x")), true);
        assert!(invitation_due(&unrelated, installed, installed + 1, false), "an unrelated variable changes nothing");
    }

    /// No frame reads the folder: `asked` is read the first time the strip would show, then
    /// again only when the window asks for a recheck (on activation and every `ASKED_EVERY`), and
    /// the mode is the one the last config change set.
    #[test]
    fn the_invitation_reads_asked_once_and_again_only_on_a_recheck() {
        let installed = 20_742;
        let reads = Cell::new(0);
        let answered = Cell::new(false);
        let look = || {
            reads.set(reads.get() + 1);
            answered.get()
        };
        let mut inv = Invitation::new(Some((installed, installed + 1)), mode_of(Usage::Local, Crashes::Ask, Channel::Release, None, true));
        for _ in 0..100 {
            assert!(inv.shows(look));
        }
        assert_eq!(reads.get(), 1, "one read for a hundred frames");
        inv.recheck();
        assert!(inv.shows(look));
        assert_eq!(reads.get(), 2, "the activation's recheck reads again");
        // Another window has its answer: this one sees it at its next recheck, not before.
        answered.set(true);
        assert!(inv.shows(look));
        inv.recheck();
        assert!(!inv.shows(look));
        assert_eq!(reads.get(), 3);
        assert!(!inv.possible(), "never again in this run");
        inv.recheck();
        assert!(!inv.shows(look));
        assert_eq!(reads.get(), 3, "nothing is read once it cannot show");
        // A mode that does not invite hides the strip without reading the folder.
        let reads_before = reads.get();
        let mut inv = Invitation::new(Some((installed, installed + 1)), mode_of(Usage::On, Crashes::Ask, Channel::Release, None, true));
        inv.recheck();
        assert!(!inv.shows(look));
        assert_eq!(reads.get(), reads_before);
        answered.set(false);
        inv.set_mode(mode_of(Usage::Local, Crashes::Ask, Channel::Release, None, true));
        assert!(inv.shows(look), "the config changed back to local");
        // The install day, and either answer in this window, show nothing.
        assert!(!Invitation::new(None, mode_of(Usage::Local, Crashes::Ask, Channel::Release, None, true)).shows(look));
        inv.answered();
        assert!(!inv.shows(look));
        assert!(!inv.possible());
    }

    #[test]
    fn the_day_is_the_utc_day() {
        // 2026-10-16 09:00 UTC.
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_792_141_200);
        assert_eq!(utc_day(at), 20_742);
        assert_eq!(utc_day(at + Duration::from_secs(14 * 3600)), 20_742, "23:00 UTC the same day");
        assert_eq!(utc_day(at + Duration::from_secs(15 * 3600)), 20_743, "00:00 UTC the next day");
        assert_eq!(utc_day(SystemTime::UNIX_EPOCH - Duration::from_secs(5)), 0, "a clock before 1970");
    }

    /// Either button writes `asked`, creating the telemetry folder when it is missing, and a
    /// window that looks again finds it.
    #[test]
    fn an_answer_writes_asked_and_every_window_then_sees_it() {
        let dir = tempfile::tempdir().unwrap();
        let telemetry = dir.path().join("telemetry");
        assert!(!asked(&telemetry), "no folder yet");
        answer_invitation(&telemetry, Answer::NotNow).unwrap();
        assert!(asked(&telemetry));
        assert!(telemetry.join(ASKED).is_file());
        // Answering again (a second window's stale strip) is no error.
        answer_invitation(&telemetry, Answer::TurnOn).unwrap();
        assert!(asked(&telemetry));
    }

    /// The strip says what `docs/telemetry.md` says it says, with the buttons it names.
    #[test]
    fn the_invitation_speaks_in_the_words_of_the_page() {
        let page = include_str!("../../../docs/telemetry.md").split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(INVITATION, "Help improve Arkos: share anonymous usage reports. Nothing identifies you or your files.");
        assert!(page.contains(&format!("\"{INVITATION}\"")), "the page quotes the strip");
        for button in [TURN_ON, NOT_NOW, WHAT_IS_SENT] {
            assert!(page.contains(&format!("**{button}**")), "the page names {button}");
        }
        assert_eq!([TURN_ON, NOT_NOW, WHAT_IS_SENT], ["Turn on", "Not now", "What is sent"]);
    }
}
