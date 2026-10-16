//! The About window: the build, update and install details, the paths a bug report needs, and the
//! attribution. Everything it shows comes from one `Details`, which the clipboard text renders too.
use crate::chrome::UI_FONT;
use crate::theme::Theme;
use crate::{aux, client, keymap, paths, update};
use gpui::{
    div, prelude::*, px, rgb, size, App, Bounds, ClipboardItem, Context, FocusHandle, KeyDownEvent, SharedString, TitlebarOptions, Window,
    WindowBounds, WindowHandle, WindowOptions,
};
use objc2_foundation::NSProcessInfo;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Where "Buy me more tokens" goes. Empty until Cyrus sets it, and the line is not drawn while it
/// is empty.
pub const DONATE_URL: &str = "";
/// The name in the attribution line.
pub const AUTHOR: &str = "Cyrus Sarkosh";
/// Where the author's name links to.
pub const AUTHOR_URL: &str = "https://github.com/csarkosh";

/// The window's size: fixed, since the window is not resizable.
const WIDTH: f32 = 420.0;
const HEIGHT: f32 = 520.0;
/// The width of the table's label column.
const LABEL_W: f32 = 112.0;
/// The colours of the donation line's two glyphs: Claude's orange spark and a red heart.
const SPARK: u32 = 0xd97757;
const HEART: u32 = 0xe5484d;

/// Whether the donation line is drawn: only when there is somewhere for it to go.
pub fn donation_visible(url: &str) -> bool {
    !url.trim().is_empty()
}

/// What the About window shows and "Copy details" copies, gathered by `Root::details` or, before
/// the project window exists, from what `main()` loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Details {
    /// The crate version, "0.3.1".
    pub version: String,
    /// The short git hash this build was made from.
    pub build_hash: String,
    /// The build commit's date, "YYYY-MM-DD" or "unknown".
    pub build_date: String,
    /// When the running build was put in place (`last_updated`); `None` when nothing says.
    pub last_updated: Option<SystemTime>,
    /// When this install first ran (`startup::installed_since`).
    pub first_installed: SystemTime,
    /// The daemon's version and its pid when known; `None` when not connected.
    pub daemon: Option<(String, Option<u32>)>,
    /// The protocol version this build speaks.
    pub protocol: u32,
    /// "{major}.{minor}.{patch}" (`macos_version`).
    pub macos: String,
    /// The CPU architecture, "arm64".
    pub arch: &'static str,
    /// The config file.
    pub config: PathBuf,
    /// The folder theme files are read from.
    pub themes: PathBuf,
    /// The Application Support folder: the socket, the logs, `updates/`.
    pub app_support: PathBuf,
    /// The window's own log.
    pub window_log: PathBuf,
    /// The daemon's log, next to its socket.
    pub daemon_log: PathBuf,
}

impl Details {
    /// This build's details, with what only the caller knows: when the install first ran, and the
    /// daemon's version and pid while connected.
    pub fn gather(first_installed: SystemTime, daemon: Option<(String, Option<u32>)>) -> Details {
        let app_support = paths::app_support();
        Details {
            version: env!("CARGO_PKG_VERSION").to_string(),
            build_hash: env!("ARKOS_GIT_HASH").to_string(),
            build_date: crate::build_date().to_string(),
            last_updated: last_updated(&app_support.join("updates"), update::launch_path().as_deref()),
            first_installed,
            daemon,
            protocol: protocol::PROTOCOL_VERSION,
            macos: macos_version(),
            arch: std::env::consts::ARCH,
            config: paths::config_file(),
            themes: paths::themes_dir(),
            window_log: app_support.join("window.log"),
            daemon_log: client::daemon_log(&paths::socket_path()),
            app_support,
        }
    }

    /// The table, label and value, in the order the window draws it. `now` decides whether a date
    /// reads "today HH:MM".
    pub fn rows(&self, now: SystemTime) -> Vec<(&'static str, String)> {
        // The Hello names the binary first ("arkosd 0.3.1 (abc1234)"); the row's label says it.
        let daemon = self.daemon.as_ref().map(|(v, pid)| (v.strip_prefix("arkosd ").unwrap_or(v), pid));
        let daemon = match daemon {
            None => "not connected".to_string(),
            Some((version, Some(pid))) => format!("{version}, pid {pid}"),
            Some((version, None)) => version.to_string(),
        };
        vec![
            ("Version", self.version.clone()),
            ("Build", format!("{}, {}", self.build_hash, self.build_date)),
            ("Last updated", self.last_updated.map_or_else(|| "unknown".to_string(), |t| format_when(t, now))),
            ("First installed", format_when(self.first_installed, now)),
            ("Daemon", daemon),
            ("Protocol", self.protocol.to_string()),
            ("macOS", format!("{} {}", self.macos, self.arch)),
            ("Config", self.config.display().to_string()),
            ("Themes", self.themes.display().to_string()),
            ("App Support", self.app_support.display().to_string()),
            ("Window log", self.window_log.display().to_string()),
            ("Daemon log", self.daemon_log.display().to_string()),
        ]
    }

    /// The table as plain text for a bug report: "Arkos IDE {version}", then one "label: value"
    /// line per row.
    pub fn clipboard_text(&self, now: SystemTime) -> String {
        let mut lines = vec![format!("Arkos IDE {}", self.version)];
        lines.extend(self.rows(now).into_iter().map(|(label, value)| format!("{label}: {value}")));
        lines.join("\n")
    }

    /// The path behind a path row, which a click reveals in Finder; `None` for the other rows.
    fn path_of(&self, label: &str) -> Option<&Path> {
        Some(match label {
            "Config" => &self.config,
            "Themes" => &self.themes,
            "App Support" => &self.app_support,
            "Window log" => &self.window_log,
            "Daemon log" => &self.daemon_log,
            _ => return None,
        })
    }
}

/// A date for the table: "today HH:MM" when `t` falls on `now`'s local day, else its local date as
/// "YYYY-MM-DD".
pub fn format_when(t: SystemTime, now: SystemTime) -> String {
    let (at, today) = (local_tm(t), local_tm(now));
    if (at.tm_year, at.tm_yday) == (today.tm_year, today.tm_yday) {
        format!("today {:02}:{:02}", at.tm_hour, at.tm_min)
    } else {
        format!("{:04}-{:02}-{:02}", at.tm_year + 1900, at.tm_mon + 1, at.tm_mday)
    }
}

/// The local civil time of `t`. A time before the epoch counts as the epoch.
fn local_tm(t: SystemTime) -> libc::tm {
    let secs = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or(Duration::ZERO).as_secs() as libc::time_t;
    // SAFETY: localtime_r writes only into the tm we hand it, and reads the time_t by pointer.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&secs, &mut tm);
        tm
    }
}

/// When the running build was put in place: the daemon's `applied.json` in `updates_dir` (its
/// `at_unix`) when it is there, else the executable's creation time (the moment this copy was put
/// in place), else its modification time; `None` when none of them is known.
pub fn last_updated(updates_dir: &Path, exe: Option<&Path>) -> Option<SystemTime> {
    let applied = std::fs::read(updates_dir.join("applied.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|v| v.get("at_unix")?.as_u64())
        .and_then(|secs| SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(secs)));
    if applied.is_some() {
        return applied;
    }
    let meta = std::fs::metadata(exe?).ok()?;
    meta.created().or_else(|_| meta.modified()).ok()
}

/// The running macOS version, "{major}.{minor}.{patch}".
pub fn macos_version() -> String {
    let v = NSProcessInfo::processInfo().operatingSystemVersion();
    format!("{}.{}.{}", v.majorVersion, v.minorVersion, v.patchVersion)
}

/// The About window's view. One at most, kept in `aux::Aux`.
pub struct AboutWindow {
    details: Details,
    theme: Theme,
    focus: FocusHandle,
}

impl AboutWindow {
    /// Replaces what the window shows, for an About asked for again while it is open: the details
    /// may have been gathered before the project window and its daemon connection existed.
    pub fn refresh(&mut self, details: Details, theme: Theme, cx: &mut Context<Self>) {
        self.details = details;
        self.theme = theme;
        cx.notify();
    }
}

/// Opens the About window centred on the main display and gives it the keyboard. `None` when
/// gpui could not open it, which the log records.
pub fn open(cx: &mut App, details: Details, theme: Theme) -> Option<WindowHandle<AboutWindow>> {
    let fixed = size(px(WIDTH), px(HEIGHT));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, fixed, cx))),
        titlebar: Some(TitlebarOptions { title: Some("About Arkos IDE".into()), appears_transparent: false, traffic_light_position: None }),
        focus: true,
        is_resizable: false,
        window_min_size: Some(fixed),
        ..Default::default()
    };
    let opened = cx.open_window(options, move |window, cx| {
        let view = cx.new(|cx| AboutWindow { details, theme, focus: cx.focus_handle() });
        window.focus(&view.read(cx).focus);
        view
    });
    match opened {
        Ok(handle) => Some(handle),
        Err(e) => {
            eprintln!("arkos: the About window did not open: {e:#}");
            None
        }
    }
}

impl Render for AboutWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = &self.theme.chrome;
        let (bg, text, dim, border, link, pill) = (c.window_bg.hsla(), c.text.hsla(), c.text_dim.hsla(), c.border.hsla(), c.focus_ring.hsla(), c.banner.hsla());
        let mut table = div().id("about-rows").flex_1().min_h_0().overflow_y_scroll().flex().flex_col().gap(px(5.0)).text_size(px(13.0));
        for (ix, (label, value)) in self.details.rows(SystemTime::now()).into_iter().enumerate() {
            // The themes folder may not exist before the first theme file; it is made first, as
            // Settings' "Show themes folder" does.
            let make = label == "Themes";
            let value = match self.details.path_of(label).map(Path::to_path_buf) {
                Some(path) => div()
                    .id(("about-path", ix))
                    .text_color(link)
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .on_click(move |_, _, cx| {
                        if make {
                            if let Err(e) = std::fs::create_dir_all(&path) {
                                eprintln!("arkos: cannot create {}: {e}", path.display());
                            }
                        }
                        cx.reveal_path(&path);
                    })
                    .child(value)
                    .into_any_element(),
                None => div().child(value).into_any_element(),
            };
            table = table.child(
                div()
                    .flex()
                    .gap(px(12.0))
                    .child(div().w(px(LABEL_W)).flex_none().text_color(dim).child(label))
                    .child(div().flex_1().min_w_0().overflow_hidden().child(value)),
            );
        }
        // Drawn like the terminal's "jump to bottom" pill.
        let copy = div()
            .id("about-copy")
            .flex_none()
            .px(px(10.0))
            .py(px(4.0))
            .rounded(px(10.0))
            .bg(pill)
            .text_color(text)
            .text_size(px(12.0))
            .cursor_pointer()
            .child("Copy details")
            .on_click(cx.listener(|about, _, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(about.details.clipboard_text(SystemTime::now())))));
        let author = div()
            .flex()
            .justify_center()
            .gap(px(4.0))
            .child("Made by")
            .child(div().id("about-author").text_color(link).cursor_pointer().hover(|s| s.underline()).on_click(|_, _, cx| cx.open_url(AUTHOR_URL)).child(AUTHOR));
        let donate = donation_visible(DONATE_URL).then(|| {
            div()
                .id("about-donate")
                .flex()
                .justify_center()
                .gap(px(4.0))
                .cursor_pointer()
                .hover(|s| s.underline())
                .on_click(|_, _, cx| cx.open_url(DONATE_URL))
                .child("Buy me more tokens")
                .child(div().text_color(rgb(SPARK)).child("✦"))
                .child(div().text_color(rgb(HEART)).child("❤"))
        });
        div()
            .id("about")
            .key_context("About")
            .track_focus(&self.focus)
            .on_key_down(|ev: &KeyDownEvent, window, _| {
                if ev.keystroke.key == "escape" && !ev.keystroke.modifiers.modified() {
                    window.remove_window();
                }
            })
            // Cmd+W arrives as the close-tab action.
            .on_action(|_: &keymap::CloseTab, window, _| window.remove_window())
            .on_action(|_: &keymap::NewWindow, _, cx| aux::forward_new_window(cx))
            .size_full()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .p(px(20.0))
            .bg(bg)
            .text_color(text)
            .font_family(SharedString::from(UI_FONT))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(20.0)).font_weight(gpui::FontWeight::SEMIBOLD).child("Arkos IDE"))
                    .child(div().text_size(px(13.0)).text_color(dim).child(self.details.version.clone())),
            )
            .child(table)
            .child(div().flex_none().flex().child(copy))
            .child(
                div()
                    .flex_none()
                    .pt(px(12.0))
                    .border_t_1()
                    .border_color(border)
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .text_size(px(12.0))
                    .text_color(dim)
                    .child(author)
                    .children(donate),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_render_with_nothing_known() {
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let now = t0 + Duration::from_secs(30 * 86_400);
        let d = Details { version: "0.3.1".into(), build_hash: "abc1234".into(), build_date: "2026-10-15".into(), last_updated: None, first_installed: t0, daemon: None, protocol: 5, macos: "26.1.0".into(), arch: "arm64", config: "/c/config.toml".into(), themes: "/c/themes".into(), app_support: "/a".into(), window_log: "/a/window.log".into(), daemon_log: "/a/arkosd.log".into() };
        let rows = d.rows(now);
        assert_eq!(rows.iter().map(|(k, _)| *k).collect::<Vec<_>>(), vec!["Version", "Build", "Last updated", "First installed", "Daemon", "Protocol", "macOS", "Config", "Themes", "App Support", "Window log", "Daemon log"]);
        let get = |l: &str| rows.iter().find(|(k, _)| *k == l).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(get("Version"), "0.3.1");
        assert_eq!(get("Build"), "abc1234, 2026-10-15");
        assert_eq!(get("Last updated"), "unknown");
        let first = get("First installed");
        assert_eq!(first, format_when(t0, now));
        assert!(first.len() == 10 && first.as_bytes()[4] == b'-' && first.as_bytes()[7] == b'-', "{first}");
        assert_eq!(get("Daemon"), "not connected");
        assert_eq!(get("Protocol"), "5");
        assert_eq!(get("macOS"), "26.1.0 arm64");
        assert_eq!(get("Config"), "/c/config.toml");
        let text = d.clipboard_text(now);
        assert!(text.starts_with("Arkos IDE 0.3.1\n"));
        assert_eq!(text.lines().count(), rows.len() + 1);
        assert!(text.contains("\nDaemon: not connected\n"));
        assert!(!text.contains("None"));
        let with = Details { daemon: Some(("0.3.1".into(), Some(4242))), ..d.clone() };
        assert_eq!(with.rows(now).iter().find(|(k, _)| *k == "Daemon").unwrap().1, "0.3.1, pid 4242");
        let hello = Details { daemon: Some(("arkosd 0.3.1 (abc1234)".into(), Some(4242))), ..d.clone() };
        assert_eq!(hello.rows(now).iter().find(|(k, _)| *k == "Daemon").unwrap().1, "0.3.1 (abc1234), pid 4242");
        assert!(hello.clipboard_text(now).contains("\nDaemon: 0.3.1 (abc1234), pid 4242\n"));
        let no_pid = Details { daemon: Some(("0.3.1".into(), None)), ..d };
        assert_eq!(no_pid.rows(now).iter().find(|(k, _)| *k == "Daemon").unwrap().1, "0.3.1");
    }

    #[test]
    fn dates_say_today_or_the_day_and_the_donation_line_hides_without_a_url() {
        let now = SystemTime::now();
        assert!(format_when(now, now).starts_with("today "));
        let s = format_when(now - Duration::from_secs(3 * 86_400), now);
        assert!(s.len() == 10 && s.as_bytes()[4] == b'-');
        assert!(!donation_visible(""));
        assert!(donation_visible("https://example.org/tip"));
        assert!(DONATE_URL.is_empty(), "ships hidden until Cyrus sets it");
    }

    #[test]
    fn last_updated_prefers_the_journal_then_the_executable() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("arkos");
        std::fs::write(&exe, "x").unwrap();
        assert!(last_updated(dir.path(), Some(&exe)).is_some(), "the executable's creation or modification time");
        assert_eq!(last_updated(dir.path(), None), None);
        std::fs::write(dir.path().join("applied.json"), r#"{"version":"0.3.0","at_unix":1800000000}"#).unwrap();
        assert_eq!(last_updated(dir.path(), Some(&exe)), Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)));
    }

    #[test]
    fn the_macos_version_is_three_numbers() {
        let v = macos_version();
        let parts: Vec<&str> = v.split('.').collect();
        assert_eq!(parts.len(), 3, "{v}");
        assert!(parts.iter().all(|p| p.parse::<u32>().is_ok()), "{v}");
    }
}
