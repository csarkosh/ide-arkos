//! The project chooser. A Dock, Finder or Spotlight launch starts with `/` as its working
//! directory: the window asks which folder to open instead, listing the most recent ones
//! (`recent.rs`) with a way to show them all and the native folder picker. "Open recent..." from
//! a running window shows the same chooser; the folder chosen there opens in a new process.
use crate::chooser_sessions::{self, LinkEvent};
use crate::config::{self, Config};
use crate::keymap::{self, OpenFolder};
use crate::launch_link::Link;
use crate::recent::{self, Recent};
use crate::theme::{self, Theme};
use crate::{paths, workspace};
use gpui::{
    div, prelude::*, px, size, App, Bounds, ClickEvent, Context, FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent, PathPromptOptions,
    ScrollHandle, Task, Window, WindowBounds, WindowHandle, WindowOptions,
};
use protocol::SessionId;
use std::collections::{HashMap, HashSet};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The recent folders shown before "Show all".
pub const TOP: usize = 5;
/// After "Stop all": the daemon escalates a kill to SIGKILL after a second, so the list is asked
/// again once that has passed.
const RELIST_AFTER_STOP: std::time::Duration = std::time::Duration::from_millis(1500);
const WIDTH: f32 = 560.0;
const HEIGHT: f32 = 460.0;
/// The launch chooser's footer until the daemon's connection exists.
pub const FOOTER_CONNECTING: &str = "connecting to the daemon\u{2026}";
pub const FOOTER_RECONNECTING: &str = "reconnecting to the daemon\u{2026}";

/// Whether this launch should ask for a folder: exactly `/` is what macOS gives an app opened
/// from the Dock, Finder or Spotlight. A terminal launch keeps its working directory.
pub fn needs_chooser(cwd: &Path) -> bool {
    cwd == Path::new("/")
}

/// What a process opens at start. Either way it connects to the daemon at once, off the main
/// thread and while the chooser opens, and says Hello as soon as the connection exists: a window
/// the daemon relaunched after an update confirms it with that Hello, even while the chooser
/// waits for a folder (the 2026-10-15 incident: a relaunch at `/` that waited silently in the
/// chooser was rolled back).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// The launch chooser; the project window follows on the same connection.
    Chooser { focus: bool },
    Project { focus: bool },
}

impl Plan {
    /// Always: the Hello is what tells the daemon a window came up. The connect no longer blocks
    /// the first window; the Hello still goes out the moment the connection exists.
    pub fn connects_first(self) -> bool {
        true
    }

    /// Whether the first window takes the keyboard: not when the daemon relaunched it (an update,
    /// the watchdog), since the user may be in another app.
    pub fn focus(self) -> bool {
        match self {
            Plan::Chooser { focus } | Plan::Project { focus } => focus,
        }
    }

    pub fn is_chooser(self) -> bool {
        matches!(self, Plan::Chooser { .. })
    }
}

/// The plan for a process started in `cwd`; `relaunched` is `ARKOS_RELAUNCH=1` (the daemon
/// started it). A relaunch at `/` shows the chooser too: the daemon has no folder for it.
pub fn startup_plan(cwd: &Path, relaunched: bool) -> Plan {
    let focus = !relaunched;
    if needs_chooser(cwd) {
        Plan::Chooser { focus }
    } else {
        Plan::Project { focus }
    }
}

/// The folder the Hello names: empty at `/` (the protocol's "keep the daemon's"), since the
/// chooser has no project yet and the daemon must not remember `/`.
pub fn hello_cwd(cwd: &Path) -> PathBuf {
    if needs_chooser(cwd) {
        PathBuf::new()
    } else {
        cwd.to_path_buf()
    }
}

/// `path` with the home folder written as `~`.
pub fn shorten_home(path: &str, home: &str) -> String {
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return path.to_string();
    }
    if path == home {
        return "~".to_string();
    }
    match path.strip_prefix(home) {
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_string(),
    }
}

/// Whether two paths name the same folder once symlinks are resolved.
pub fn same_folder(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    Folder(Recent),
    /// "Show all N folders".
    ShowAll(usize),
    ShowFewer,
    /// "Open another folder...": the native picker.
    Picker,
}

/// The chooser's rows: the `TOP` newest folders (all of them once `expanded`), the row that
/// expands or collapses the list when there are more, and the picker last.
pub fn rows(recents: &[Recent], expanded: bool) -> Vec<Row> {
    let more = recents.len() > TOP;
    let shown = if more && !expanded { TOP } else { recents.len() };
    let mut out: Vec<Row> = recents[..shown].iter().cloned().map(Row::Folder).collect();
    if more {
        out.push(if expanded { Row::ShowFewer } else { Row::ShowAll(recents.len()) });
    }
    out.push(Row::Picker);
    out
}

/// The selection after an arrow key: it stops at the first and last rows.
pub fn move_selection(len: usize, selected: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    (selected as isize + delta).clamp(0, len as isize - 1) as usize
}

/// The native picker: one folder.
pub fn picker_options() -> PathPromptOptions {
    PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Open".into()) }
}

/// The command that opens a window on `folder`: this binary in that folder, in its own session
/// with no stdio. `ARKOS_RELAUNCH` is cleared so the new window takes focus, even when this
/// window was itself relaunched by the daemon.
pub fn window_command(exe: &Path, folder: &Path) -> Command {
    let mut cmd = Command::new(exe);
    cmd.current_dir(folder).env_remove("ARKOS_RELAUNCH").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd
}

/// Opens `folder` in a new window process and reaps it when it exits.
pub fn spawn_window(folder: &Path) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let mut child = window_command(&exe, folder).spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// What the chooser does with the folder picked: at launch, open the project window here on the
/// link's connection (given with the folder); from a running window, which has no link, open it
/// in a new process.
pub type OnChoice = Box<dyn FnOnce(PathBuf, Option<Link>, &mut App)>;

pub struct Chooser {
    recents: Vec<Recent>,
    expanded: bool,
    selected: usize,
    focus: FocusHandle,
    scroll: ScrollHandle,
    config: Config,
    theme: Theme,
    home: String,
    /// Taken by the first choice; the window closes right after.
    on_choice: Option<OnChoice>,
    /// The native picker is up: Enter and Cmd+O do not open a second one.
    picking: bool,
    /// The daemon, at launch only: a chooser opened from a running window has no line of its own
    /// and shows no counts.
    link: Option<Link>,
    /// Each recent folder's saved sessions, read with the list.
    saved: HashMap<PathBuf, Vec<SessionId>>,
    /// Each recent folder's live sessions, from the daemon's last list; absent means none.
    live: HashMap<PathBuf, Vec<SessionId>>,
    /// The folder whose "Stop all" was asked once; the second ask stops its sessions.
    confirm: Option<PathBuf>,
    /// Folders whose kills are out and not yet all answered.
    stopping: HashSet<PathBuf>,
    /// A line under the list: what the daemon refused.
    notice: Option<String>,
    /// The last line: the launch chooser's wait for the daemon.
    footer: Option<String>,
    /// At launch, until the link arrives: a choice waits for it, since the project window
    /// needs the connection.
    waiting: bool,
    /// The folder chosen while `waiting`; it opens as the link arrives.
    chosen: Option<PathBuf>,
}

impl Chooser {
    fn rows(&self) -> Vec<Row> {
        rows(&self.recents, self.expanded)
    }

    fn live_of(&self, folder: &Path) -> &[SessionId] {
        self.live.get(folder).map(|v| v.as_slice()).unwrap_or(&[])
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.selected = move_selection(self.rows().len(), self.selected, delta);
        self.scroll.scroll_to_item(self.selected);
        self.confirm = None;
        cx.notify();
    }

    /// "Stop all" for a folder: the first ask arms the row, the second sends a Kill for each of
    /// its sessions. Nothing while its kills are out, or when nothing is left to stop.
    fn request_stop(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        if self.stopping.contains(&folder) {
            return;
        }
        if self.confirm.as_ref() == Some(&folder) {
            self.confirm = None;
            let sessions = self.live.remove(&folder).unwrap_or_default();
            if let (Some(link), false) = (&self.link, sessions.is_empty()) {
                link.stop(folder.clone(), &sessions);
                self.stopping.insert(folder);
                self.notice = None;
                // A Kill answers Ok before the shell has gone (it escalates to SIGKILL after a
                // second), so the count is dropped here and the daemon asked again once that
                // has passed: a refused session comes back.
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(RELIST_AFTER_STOP).await;
                    let _ = this.update(cx, |this, _| {
                        if let Some(link) = &this.link {
                            link.list();
                        }
                    });
                })
                .detach();
            }
        } else {
            self.confirm = Some(folder);
        }
        cx.notify();
    }

    /// The launch chooser's connection, once it exists: the counts follow, and a folder chosen
    /// meanwhile opens now.
    pub fn attach_link(&mut self, link: Link, window: &mut Window, cx: &mut Context<Self>) {
        self.waiting = false;
        self.footer = None;
        self.link = Some(link);
        if let Some(folder) = self.chosen.take() {
            // Not started: the receiver goes to the project window untouched.
            self.choose(folder, window, cx);
            return;
        }
        self.start_link(cx);
        cx.notify();
    }

    pub fn set_footer(&mut self, text: Option<String>, cx: &mut Context<Self>) {
        self.footer = text;
        cx.notify();
    }

    /// The daemon's answers reach the chooser while it lives; the list is asked for now.
    fn start_link(&mut self, cx: &mut Context<Self>) {
        let Some(link) = self.link.clone() else { return };
        let weak = cx.weak_entity();
        link.start(
            Box::new(move |ev, cx| {
                let _ = weak.update(cx, |chooser, cx| chooser.on_link(ev, cx));
            }),
            cx,
        );
    }

    /// macOS switched between light and dark: in `system` mode the chooser re-themes; a manually
    /// chosen theme stays.
    pub fn set_system_appearance(&mut self, dark: bool, cx: &mut Context<Self>) {
        if self.config.appearance.mode != config::AppearanceMode::System {
            return;
        }
        let name = theme::pick_name(self.config.appearance.mode, dark, &self.config.theme).to_string();
        if name == self.theme.name {
            return;
        }
        match theme::load(&name, &paths::themes_dir()) {
            Ok(t) => {
                self.theme = t;
                cx.notify();
            }
            Err(e) => eprintln!("arkos: chooser theme: {e}"),
        }
    }

    /// The daemon's answers, through the link.
    pub fn on_link(&mut self, ev: LinkEvent, cx: &mut Context<Self>) {
        match ev {
            LinkEvent::Sessions(list) => {
                if self.footer.as_deref() == Some(FOOTER_RECONNECTING) {
                    self.footer = None;
                }
                self.live = self
                    .recents
                    .iter()
                    .map(|r| {
                        let saved = self.saved.get(&r.path).map(|v| v.as_slice()).unwrap_or(&[]);
                        (r.path.clone(), chooser_sessions::sessions_of(&r.path, saved, &list))
                    })
                    .filter(|(_, v)| !v.is_empty())
                    .collect();
                // An armed row with nothing left to stop (a worktree listed as its own folder,
                // stopped through its project) is disarmed.
                if self.confirm.as_ref().is_some_and(|f| !self.live.contains_key(f)) {
                    self.confirm = None;
                }
            }
            LinkEvent::Stopped { folder, errors } => {
                self.stopping.remove(&folder);
                if let Some(first) = errors.first() {
                    let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| folder.display().to_string());
                    self.notice = Some(format!("{} of {name}'s sessions could not be stopped: {first}", errors.len()));
                }
            }
            LinkEvent::Lost => {
                self.live.clear();
                self.stopping.clear();
                self.confirm = None;
                self.footer = Some(FOOTER_RECONNECTING.to_string());
            }
        }
        cx.notify();
    }

    fn activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.rows().get(self.selected).cloned() else { return };
        match row {
            Row::Folder(r) => self.choose(r.path, window, cx),
            // The selection stays on the same row index: the sixth folder after "Show all".
            Row::ShowAll(_) => {
                self.expanded = true;
                cx.notify();
            }
            Row::ShowFewer => {
                self.expanded = false;
                self.selected = TOP;
                self.scroll.scroll_to_item(self.selected);
                cx.notify();
            }
            Row::Picker => self.pick(window, cx),
        }
    }

    fn choose(&mut self, folder: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        // No connection yet: the latest choice opens once there is one.
        if self.waiting {
            self.chosen = Some(folder);
            cx.notify();
            return;
        }
        // A kill still out would answer into the window's loop, which did not ask: a moment.
        if !self.stopping.is_empty() || self.link.as_ref().is_some_and(|l| l.kills_pending()) {
            self.notice = Some("a moment: sessions are still stopping".into());
            cx.notify();
            return;
        }
        // The link is the window's from here: the re-list timer finds none to ask.
        let link = self.link.take();
        if let Some(f) = self.on_choice.take() {
            f(folder, link, cx);
        }
        window.remove_window();
    }

    /// The native picker; a cancel leaves the chooser as it was.
    fn pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picking {
            return;
        }
        self.picking = true;
        let rx = cx.prompt_for_paths(picker_options());
        cx.spawn_in(window, async move |this, cx| {
            let picked = match rx.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Err(e)) => {
                    eprintln!("arkos: folder picker: {e}");
                    None
                }
                _ => None,
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.picking = false;
                match picked {
                    Some(folder) => this.choose(folder, window, cx),
                    None => window.focus(&this.focus),
                }
            });
        })
        .detach();
    }

    fn key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        if k.modifiers.modified() {
            return;
        }
        match k.key.as_str() {
            "up" => self.step(-1, cx),
            "down" => self.step(1, cx),
            "enter" => self.activate(window, cx),
            // Delete asks to stop the selected folder's sessions; a second Delete does it.
            "backspace" | "delete" => {
                if let Some(Row::Folder(r)) = self.rows().get(self.selected).cloned() {
                    if !self.live_of(&r.path).is_empty() {
                        self.request_stop(r.path, cx);
                    }
                }
            }
            // Esc first takes back an armed "Stop all"; with none armed it closes the chooser.
            "escape" => {
                if self.confirm.take().is_some() {
                    cx.notify();
                } else {
                    window.remove_window();
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// The right-hand side of a folder's row: its live session count and "Stop all", the armed
    /// confirmation, or the wait while its kills are out.
    fn trailing(&self, ix: usize, folder: &Path, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let c = &self.theme.chrome;
        let (dim, text, ring, hover_bg) = (c.text_dim.hsla(), c.text.hsla(), c.focus_ring.hsla(), c.tab_inactive.hsla());
        let n = self.live_of(folder).len();
        if self.stopping.contains(folder) {
            return Some(div().flex_none().text_color(dim).child("stopping\u{2026}").into_any_element());
        }
        let button = |label: String, id: &'static str, color: gpui::Hsla| {
            let path = folder.to_path_buf();
            div()
                .id((id, ix))
                .flex_none()
                .px(px(6.0))
                .rounded(px(3.0))
                .text_color(color)
                .cursor_pointer()
                .hover(|s| s.bg(hover_bg))
                // Not a click on the row: the row would open the folder.
                .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _window, cx| cx.stop_propagation())
                // A double click is not two asks: the second click of one lands on the Confirm
                // the first drew under the pointer.
                .on_click(cx.listener(move |this, ev: &ClickEvent, _window, cx| {
                    if ev.click_count() == 1 {
                        this.request_stop(path.clone(), cx);
                    }
                }))
                .child(label)
                .into_any_element()
        };
        if self.confirm.as_deref() == Some(folder) {
            let label = format!("Stop {}? Confirm", chooser_sessions::count_label(n).unwrap_or_else(|| "sessions".into()));
            return Some(button(label, "chooser-confirm", ring));
        }
        let label = chooser_sessions::count_label(n)?;
        Some(
            div()
                .flex_none()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(div().text_color(dim).child(label))
                .child(button("Stop all".into(), "chooser-stop", text))
                .into_any_element(),
        )
    }
}

impl Render for Chooser {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = &self.theme.chrome;
        let (text, dim, selected_bg, hover_bg, ring, bg) =
            (c.text.hsla(), c.text_dim.hsla(), c.tab_active.hsla(), c.tab_inactive.hsla(), c.focus_ring.hsla(), c.window_bg.hsla());
        let picker_keys = keymap::keys_for("open folder", &self.config.keybindings).map(|k| keymap::pretty_keys(&k)).unwrap_or_default();
        let mut list = div().id("chooser-rows").flex_1().min_h_0().flex().flex_col().overflow_y_scroll().track_scroll(&self.scroll);
        for (ix, row) in self.rows().into_iter().enumerate() {
            let selected = ix == self.selected;
            let (label, detail) = match &row {
                Row::Folder(r) => {
                    let name = r.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| r.path.display().to_string());
                    (name, shorten_home(&r.path.to_string_lossy(), &self.home))
                }
                Row::ShowAll(n) => (format!("Show all {n} folders"), String::new()),
                Row::ShowFewer => ("Show fewer".to_string(), String::new()),
                Row::Picker => ("Open another folder\u{2026}".to_string(), picker_keys.clone()),
            };
            let folder = matches!(row, Row::Folder(_));
            let trailing = match &row {
                Row::Folder(r) => self.trailing(ix, &r.path, cx),
                _ => None,
            };
            list = list.child(
                div()
                    .id(("chooser-row", ix))
                    .flex_none()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.0))
                    .px(px(10.0))
                    .py(px(6.0))
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(if selected { ring } else { bg })
                    .when(selected, |d| d.bg(selected_bg))
                    .when(!selected, |d| d.hover(|s| s.bg(hover_bg)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.selected = ix;
                        this.activate(window, cx);
                    }))
                    .child(div().flex_none().text_color(if folder { text } else { dim }).child(label))
                    .child(div().flex_1().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_right().text_color(dim).child(detail))
                    .children(trailing),
            );
        }
        let notice = self.notice.clone();
        let footer = self.footer.clone();
        div()
            .id("chooser")
            .key_context("Chooser")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| this.key(ev, window, cx)))
            .on_action(cx.listener(|this, _: &OpenFolder, window, cx| this.pick(window, cx)))
            .size_full()
            .flex()
            .flex_col()
            .p(px(12.0))
            .gap(px(6.0))
            .bg(bg)
            .text_color(text)
            .font_family(gpui::SharedString::from(self.config.font.family.clone()))
            .text_size(px(text_px(&self.config)))
            .child(div().px(px(10.0)).text_color(dim).child("Recent"))
            .when(self.recents.is_empty(), |d| d.child(div().px(px(10.0)).py(px(6.0)).text_color(dim).child("No recent folders")))
            .child(list)
            .children(notice.map(|n| div().flex_none().px(px(10.0)).text_color(dim).child(n)))
            .children(footer.map(|n| div().flex_none().px(px(10.0)).text_color(dim).child(n)))
    }
}

/// Opens the chooser once the recent list is read (off the UI thread). `at_launch`: closing it
/// with nothing chosen quits (the app's window-closed observer sees it gone). `focus`
/// is whether the window takes the keyboard, as for the project window. `link` is the daemon
/// line at launch, for the session counts; none from a running window, and none at launch
/// until the connection exists (`Chooser::attach_link`): the footer says it is coming. The task
/// gives the window's handle once it is open.
/// The chooser's text size: the config's default preset.
fn text_px(config: &Config) -> f32 {
    config::resolve_preset(None, &config.font.preset, &config.font.presets).1.size
}

pub fn open_chooser(cx: &mut App, at_launch: bool, focus: bool, config: Config, theme: Theme, link: Option<Link>, on_choice: OnChoice) -> Task<Option<WindowHandle<Chooser>>> {
    let file = paths::recent_file();
    // The saved sessions serve the counts, which only the launch chooser shows, link or not yet.
    let with_saved = at_launch;
    let waiting = at_launch && link.is_none();
    cx.spawn(async move |cx| {
        let (recents, saved) = cx
            .background_executor()
            .spawn(async move {
                let recents = recent::visible(&recent::load(&file));
                // Each folder's saved sessions, so a worktree tab counts for its project.
                let saved: HashMap<PathBuf, Vec<SessionId>> =
                    if with_saved { recents.iter().map(|r| (r.path.clone(), workspace::session_ids(&r.path))).collect() } else { HashMap::new() };
                (recents, saved)
            })
            .await;
        let opened = cx.update(|cx| {
            let bounds = Bounds::centered(None, size(px(WIDTH), px(HEIGHT)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions { title: Some("Open a project".into()), appears_transparent: false, traffic_light_position: None }),
                focus,
                ..Default::default()
            };
            cx.open_window(options, move |window, cx| {
                let view = cx.new(|cx| Chooser {
                    recents,
                    expanded: false,
                    selected: 0,
                    focus: cx.focus_handle(),
                    scroll: ScrollHandle::new(),
                    config,
                    theme,
                    home: std::env::var("HOME").unwrap_or_default(),
                    on_choice: Some(on_choice),
                    picking: false,
                    link,
                    saved,
                    live: HashMap::new(),
                    confirm: None,
                    stopping: HashSet::new(),
                    notice: None,
                    footer: waiting.then(|| FOOTER_CONNECTING.to_string()),
                    waiting,
                    chosen: None,
                });
                window.focus(&view.read(cx).focus);
                let observed = view.clone();
                window
                    .observe_window_appearance(move |window, cx| {
                        let dark = theme::is_dark(window.appearance());
                        observed.update(cx, |chooser, cx| chooser.set_system_appearance(dark, cx));
                    })
                    .detach();
                view.update(cx, |chooser, cx| chooser.start_link(cx));
                view
            })
        });
        match opened {
            Ok(Ok(handle)) => Some(handle),
            _ => {
                eprintln!("arkos: cannot open the project chooser");
                if at_launch {
                    let _ = cx.update(|cx| cx.quit());
                }
                None
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recents(n: usize) -> Vec<Recent> {
        (0..n).map(|i| Recent { path: PathBuf::from(format!("/p/{i}")), opened_unix: 100 - i as u64 }).collect()
    }

    fn shape(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                Row::Folder(r) => r.path.display().to_string(),
                Row::ShowAll(n) => format!("all {n}"),
                Row::ShowFewer => "fewer".into(),
                Row::Picker => "picker".into(),
            })
            .collect()
    }

    #[test]
    fn launch_needs_chooser_only_at_root() {
        assert!(needs_chooser(Path::new("/")));
        assert!(!needs_chooser(Path::new("/Users/me/p")));
        assert!(!needs_chooser(Path::new("")));
    }

    #[test]
    fn launch_startup_plan_connects_first_whatever_opens() {
        let cases = [
            ("/", false, Plan::Chooser { focus: true }),
            ("/", true, Plan::Chooser { focus: false }),
            ("/Users/me/project", false, Plan::Project { focus: true }),
            ("/Users/me/project", true, Plan::Project { focus: false }),
        ];
        for (cwd, relaunched, want) in cases {
            let plan = startup_plan(Path::new(cwd), relaunched);
            assert_eq!(plan, want, "{cwd} relaunched={relaunched}");
            assert!(plan.connects_first(), "the chooser connects too, off the main thread, so a relaunch confirms as soon as it can");
            assert_eq!(plan.focus(), !relaunched);
        }
        assert!(startup_plan(Path::new("/"), true).is_chooser());
    }

    #[test]
    fn launch_hello_names_no_folder_from_the_chooser() {
        assert_eq!(hello_cwd(Path::new("/")), PathBuf::new(), "empty: keep the daemon's");
        assert_eq!(hello_cwd(Path::new("/Users/me/p")), PathBuf::from("/Users/me/p"));
    }

    #[test]
    fn launch_rows_show_five_then_all() {
        assert_eq!(shape(&rows(&[], false)), vec!["picker"]);
        assert_eq!(shape(&rows(&[], true)), vec!["picker"]);
        assert_eq!(shape(&rows(&recents(3), false)), vec!["/p/0", "/p/1", "/p/2", "picker"]);
        assert_eq!(shape(&rows(&recents(3), true)), vec!["/p/0", "/p/1", "/p/2", "picker"], "nothing to expand");
        assert_eq!(shape(&rows(&recents(5), false)), vec!["/p/0", "/p/1", "/p/2", "/p/3", "/p/4", "picker"]);
        assert_eq!(shape(&rows(&recents(6), false)), vec!["/p/0", "/p/1", "/p/2", "/p/3", "/p/4", "all 6", "picker"]);
        assert_eq!(shape(&rows(&recents(6), true)), vec!["/p/0", "/p/1", "/p/2", "/p/3", "/p/4", "/p/5", "fewer", "picker"]);
        let few = rows(&recents(20), false);
        assert_eq!(few.len(), TOP + 2);
        assert_eq!(few[TOP], Row::ShowAll(20));
        let all = rows(&recents(20), true);
        assert_eq!(all.len(), 20 + 2);
        assert_eq!(shape(&all[19..]), vec!["/p/19", "fewer", "picker"]);
    }

    #[test]
    fn launch_selection_clamps() {
        assert_eq!(move_selection(4, 0, -1), 0);
        assert_eq!(move_selection(4, 0, 1), 1);
        assert_eq!(move_selection(4, 3, 1), 3);
        assert_eq!(move_selection(4, 2, -1), 1);
        assert_eq!(move_selection(0, 0, 1), 0);
    }

    #[test]
    fn launch_shortens_home() {
        assert_eq!(shorten_home("/Users/me/x", "/Users/me"), "~/x");
        assert_eq!(shorten_home("/Users/me/x", "/Users/me/"), "~/x");
        assert_eq!(shorten_home("/Users/me", "/Users/me"), "~");
        assert_eq!(shorten_home("/Users/meg/x", "/Users/me"), "/Users/meg/x", "a prefix that is not a folder");
        assert_eq!(shorten_home("/opt/x", "/Users/me"), "/opt/x");
        assert_eq!(shorten_home("/opt/x", ""), "/opt/x");
    }

    #[test]
    fn launch_same_folder_resolves_spellings() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        std::fs::create_dir_all(&a).unwrap();
        assert!(same_folder(&a, &dir.path().join("a/../a")));
        assert!(!same_folder(&a, dir.path()));
    }

    #[test]
    fn launch_new_window_is_detached_and_takes_focus() {
        let cmd = window_command(Path::new("/Applications/Arkos IDE.app/Contents/MacOS/arkos"), Path::new("/Users/me/p"));
        assert_eq!(cmd.get_current_dir(), Some(Path::new("/Users/me/p")));
        let relaunch: Vec<_> = cmd.get_envs().filter(|(k, _)| *k == "ARKOS_RELAUNCH").collect();
        assert_eq!(relaunch, vec![(std::ffi::OsStr::new("ARKOS_RELAUNCH"), None)], "removed, not inherited");
        assert_eq!(cmd.get_args().count(), 0);
    }

    #[test]
    fn the_chooser_picks_the_other_theme_name_when_the_appearance_flips() {
        let cfg = Config::default();
        let (dark, light) = (
            theme::pick_name(config::AppearanceMode::System, true, &cfg.theme).to_string(),
            theme::pick_name(config::AppearanceMode::System, false, &cfg.theme).to_string(),
        );
        assert_ne!(dark, light);
        assert_eq!(theme::pick_name(config::AppearanceMode::Dark, false, &cfg.theme), dark, "a fixed mode ignores the appearance");
    }

    #[test]
    fn the_chooser_text_follows_the_default_preset() {
        let cfg = Config::default();
        assert_eq!(text_px(&cfg), 16.0);
    }
}
