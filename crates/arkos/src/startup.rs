//! The process's start: the connect to the daemon runs off the main thread, so a window opens at
//! once, and a connect that fails shows a window saying so instead of a stderr line no Dock launch
//! shows.
use crate::client::{self, Client};
use crate::config::Config;
use crate::paths;
use crate::theme::Theme;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{div, prelude::*, px, size, App, Bounds, ClickEvent, Context, FocusHandle, KeyDownEvent, SharedString, Task, TitlebarOptions, Window, WindowBounds, WindowOptions};
use protocol::ServerMessage;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub const FAILURE_TITLE: &str = "Arkos cannot reach its daemon";
const WIDTH: f32 = 480.0;
const HEIGHT: f32 = 220.0;
const TEXT: f32 = 13.0;

/// What the failure window says: the error, where the log is, and that Try again reruns it.
pub fn failure_lines(error: &str, log: &Path) -> (String, String) {
    (FAILURE_TITLE.to_string(), format!("{error}\n\nThe daemon's log is {}.", log.display()))
}

/// Try again runs one connect at a time: a second press while one runs is ignored.
#[derive(Default)]
pub struct Retry {
    running: bool,
    attempts: u32,
}

impl Retry {
    pub fn begin(&mut self) -> bool {
        if self.running {
            return false;
        }
        self.running = true;
        self.attempts += 1;
        true
    }

    pub fn end(&mut self) {
        self.running = false;
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// Whether a connect is out: no window may be open meanwhile, and that is no reason to quit.
    pub fn running(&self) -> bool {
        self.running
    }
}

/// Whether a window closing before the project window is up ends the app. With the launch
/// chooser, its close does (the failure window alone has nothing to open); without one, the last
/// window's does, unless "Try again" closed it to start its connect.
pub fn quits_on_close(project_opened: bool, chooser_plan: bool, windows_left: usize, chooser_open: bool, retrying: bool) -> bool {
    if project_opened {
        false
    } else if chooser_plan {
        !chooser_open
    } else {
        windows_left == 0 && !retrying
    }
}

pub type Connected = (Client, UnboundedReceiver<ServerMessage>);

/// Connects to the daemon, starting it when none answers, off the main thread. The window
/// never waits on it: the chooser opens meanwhile, or the project window once it is done.
pub fn connect_in_background(daemon_args: Vec<String>, cx: &mut App) -> Task<Result<Connected, String>> {
    cx.background_executor()
        .spawn(async move { Client::connect_or_start(&paths::socket_path(), &paths::daemon_exe(), &daemon_args).map_err(|e| format!("{e:#}")) })
}

/// Reads the version the last launch recorded in `file`, records `current`, and says whether
/// this launch is an update: the recorded version was older. A missing or unreadable file, the
/// same version, or a newer one (a downgrade) gives None. The write goes through a temp file
/// and a rename, as the workspace file does, so a crash leaves the old record, not a torn one.
pub fn updated_from(file: &Path, current: &str) -> Option<String> {
    let previous = std::fs::read_to_string(file).ok().and_then(|s| parse_version(s.trim()));
    let now = parse_version(current);
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = file.with_extension("tmp");
    if std::fs::write(&tmp, format!("{current}\n")).and_then(|_| std::fs::rename(&tmp, file)).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    match (previous, now) {
        (Some(p), Some(n)) if p < n => Some(current.to_string()),
        _ => None,
    }
}

/// "major.minor.patch" as a tuple; anything else is None.
fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let mut it = s.split('.').map(|p| p.parse::<u64>().ok());
    match (it.next()?, it.next()?, it.next()?, it.next()) {
        (Some(a), Some(b), Some(c), None) => Some((a, b, c)),
        _ => None,
    }
}

/// The window a failed connect opens: the error, the daemon's log, and Open the log, Try again
/// and Quit. `retry` runs the connect again; this window closes as it starts. Closing the window
/// any other way quits, as Quit and Escape do: without a daemon there is nothing to show.
pub fn open_failure_window(error: String, retry: Rc<dyn Fn(&mut App)>, config: &Config, theme: &Theme, cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(WIDTH), px(HEIGHT)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions { title: Some("Arkos".into()), ..Default::default() }),
        ..Default::default()
    };
    let log = client::daemon_log(&paths::socket_path());
    let (family, theme) = (SharedString::from(config.font.family.clone()), theme.clone());
    let opened = cx.open_window(options, move |window, cx| {
        let view = cx.new(|cx| Failure { error, log, retry, focus: cx.focus_handle(), family, theme });
        window.focus(&view.read(cx).focus);
        // The red button: no daemon, no window, so the app ends with this one.
        window.on_window_should_close(cx, |_, cx| {
            cx.quit();
            true
        });
        view
    });
    if let Err(e) = opened {
        eprintln!("arkos: cannot open the daemon failure window: {e}");
        cx.quit();
    }
}

struct Failure {
    error: String,
    log: PathBuf,
    retry: Rc<dyn Fn(&mut App)>,
    focus: FocusHandle,
    family: SharedString,
    theme: Theme,
}

impl Failure {
    fn open_log(&self) {
        match std::process::Command::new("open").arg(&self.log).spawn() {
            // Reaped when it exits.
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(e) => eprintln!("arkos: cannot open {}: {e}", self.log.display()),
        }
    }

    fn try_again(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The connect starts first, so the window's close finds it running and does not quit.
        (self.retry)(cx);
        window.remove_window();
    }

    fn key(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if ev.keystroke.key == "escape" && !ev.keystroke.modifiers.modified() {
            cx.stop_propagation();
            cx.quit();
        }
    }
}

impl Render for Failure {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = &self.theme.chrome;
        let (text, dim, hover_bg, border, bg) = (c.text.hsla(), c.text_dim.hsla(), c.tab_inactive.hsla(), c.border.hsla(), c.window_bg.hsla());
        let (title, body) = failure_lines(&self.error, &self.log);
        let button = |id: &'static str, label: &'static str| {
            div().id(id).flex_none().px(px(10.0)).py(px(3.0)).rounded(px(4.0)).border_1().border_color(border).cursor_pointer().hover(|s| s.bg(hover_bg)).child(label)
        };
        div()
            .id("daemon-failure")
            .key_context("DaemonFailure")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| this.key(ev, window, cx)))
            .size_full()
            .flex()
            .flex_col()
            .p(px(16.0))
            .gap(px(10.0))
            .bg(bg)
            .text_color(text)
            .font_family(self.family.clone())
            .text_size(px(TEXT))
            .child(div().flex_none().font_weight(gpui::FontWeight::SEMIBOLD).child(title))
            .child(div().id("daemon-failure-body").flex_1().min_h_0().overflow_y_scroll().text_color(dim).child(body))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap(px(8.0))
                    .child(button("failure-log", "Open the log").on_click(cx.listener(|this, _: &ClickEvent, _window, _cx| this.open_log())))
                    .child(button("failure-retry", "Try again").on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.try_again(window, cx))))
                    .child(button("failure-quit", "Quit").on_click(cx.listener(|_, _: &ClickEvent, _window, cx| cx.quit()))),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_again_reruns_the_connect_once() {
        let mut r = Retry::default();
        assert!(r.begin());
        assert!(!r.begin(), "one at a time");
        r.end();
        assert!(r.begin());
        assert_eq!(r.attempts(), 2);
    }

    #[test]
    fn a_close_before_the_project_window_quits_only_when_nothing_is_left_to_open() {
        // The project plan: the failure window's red button or Quit, but not Try again.
        assert!(quits_on_close(false, false, 0, false, false));
        assert!(!quits_on_close(false, false, 0, false, true), "Try again's connect runs");
        // The chooser plan: the chooser's close quits even with the failure window up.
        assert!(quits_on_close(false, true, 1, false, false));
        assert!(quits_on_close(false, true, 0, false, true));
        assert!(!quits_on_close(false, true, 1, true, true), "Try again closed the failure window; the chooser stays");
        // Once the project window is up its own handler decides.
        assert!(!quits_on_close(true, false, 0, false, false));
        assert!(!quits_on_close(true, true, 0, false, false));
    }

    #[test]
    fn the_failure_window_names_the_log() {
        let (title, body) = failure_lines("started /x/arkosd but it never accepted a connection", Path::new("/l/arkosd.log"));
        assert_eq!(title, "Arkos cannot reach its daemon");
        assert!(body.contains("/l/arkosd.log") && body.starts_with("started /x/arkosd"));
    }

    #[test]
    fn an_older_recorded_version_means_this_launch_is_an_update() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("deep").join("last_version");
        assert_eq!(updated_from(&file, "0.2.14"), None, "no file: nothing to say");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "0.2.14\n", "and the file is written, parent created");
        assert_eq!(updated_from(&file, "0.2.14"), None, "same version: nothing");
        assert_eq!(updated_from(&file, "0.2.15"), Some("0.2.15".to_string()));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "0.2.15\n");
        assert_eq!(updated_from(&file, "0.10.0"), Some("0.10.0".to_string()), "semver, not string order");
    }

    #[test]
    fn a_downgrade_gives_no_notice() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("last_version");
        std::fs::write(&file, "0.3.0\n").unwrap();
        assert_eq!(updated_from(&file, "0.2.14"), None);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "0.2.14\n", "the running version is recorded anyway");
        std::fs::write(&file, "garbage").unwrap();
        assert_eq!(updated_from(&file, "0.2.14"), None, "an unreadable version is no version");
        assert!(!dir.path().join("last_version.tmp").exists(), "no temp file left behind");
    }
}
