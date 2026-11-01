//! The About window: the build, update and install details, the paths a bug report needs, and the
//! attribution. Everything it shows comes from one `Details`, which the clipboard text renders too.
use crate::chrome::{self, TITLE_BAR_H, UI_FONT};
use crate::theme::Theme;
use crate::{aux, client, keymap, paths, update};
use gpui::{
    canvas, div, fill, point, prelude::*, px, rgb, size, App, Bounds, ClipboardItem, Context, FocusHandle, KeyDownEvent, SharedString, TitlebarOptions, Window,
    WindowBounds, WindowHandle, WindowOptions,
};
use objc2_foundation::NSProcessInfo;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Where the donation line goes: Arkos IDE's door on donate.csarko.sh. The line is not drawn
/// while this is empty.
pub const DONATE_URL: &str = "https://donate.csarko.sh/arkos";
/// The donation line's words, after the tokens icon, as every project's line reads.
pub const DONATE_WORDS: &str = "Buy me tokens";
/// The name in the attribution line.
pub const AUTHOR: &str = "Cyrus Sarkosh";
/// Where the author's name links to.
pub const AUTHOR_URL: &str = "https://github.com/csarkosh";

/// The window's title, which its own title bar draws.
const TITLE: &str = "About Arkos IDE";
/// The window's size: fixed, since the window is not resizable. The height is the content's
/// 520 px plus the title bar.
const WIDTH: f32 = 420.0;
const HEIGHT: f32 = 520.0 + TITLE_BAR_H;
/// The width of the table's label column.
const LABEL_W: f32 = 112.0;
/// The footer's text size, which the tokens icon is as tall as.
const FOOTER_TEXT: f32 = 12.0;

/// The tokens icon's ten colours: a shaded red heart and the gold token at its lower right, the
/// palette of `donate/icon.mjs` in the csarko.sh repository, which draws the same icon for the
/// web.
pub const TOKENS_PALETTE: [(char, u32); 10] = [
    ('O', 0x6b1113), // heart outline
    ('m', 0xe5484d), // heart mid-tone
    ('s', 0xb32d33), // heart shadow
    ('h', 0xff7a80), // heart highlight
    ('H', 0xffd6d8), // heart top highlight
    ('g', 0x7a5600), // token outline
    ('G', 0xf0c040), // token mid-tone
    ('c', 0xc8962a), // token shadow
    ('l', 0xffe08a), // token highlight
    ('L', 0xfff7d6), // token top highlight
];

/// The tokens icon on its 32 by 22 grid, one letter a pixel in `TOKENS_PALETTE`'s colour and a
/// dot transparent: the same pixel map as `donate/icon.mjs`.
#[rustfmt::skip]
pub const TOKENS_PIXELS: [&str; 22] = [
    "................................",
    "........OOOO........OOOO........",
    ".......OmmmsO......OmmmsO.......",
    "......OhhmmmsO....OhhmmmsO......",
    ".....OhHHhmmmsO..OhHHhmmmsO.....",
    ".....OmhhmmmmmsOOmhhmmmmmsO.....",
    "....OmmmmmmmmmmmmmmmmmmmmssO....",
    "....OmmmmmmmmmmmmmmmmmmmmssO....",
    ".....OmmmmmmmmmmmmmmmmmmssO.....",
    ".....OmmmmmmmmmmmmmmmmmmssO.....",
    ".....Ommmmmmmmmmmmmmmmgggggg....",
    "......OmmmmmmmmmmmmmmgLlllllg...",
    ".......OmmmmmmmmmmmmgLGccccGGg..",
    "........OmmmmmmmmmmglGcGGGGlGcg.",
    ".........OmmmmmmmmsglcGGGGGGlcg.",
    "..........OmmmmmmssglcGGGGGGlcg.",
    "...........OmmmmsssglcGGGGGGlcg.",
    "............OmmssssglGcGGGGlGcg.",
    ".............OmsssO.gGGllllGcg..",
    "..............OssO...gccccccg...",
    "...............OO.....gggggg....",
    "................................",
];

/// The tokens icon as runs to paint, in reading order: (x, y, width, colour), each a 1-pixel-tall
/// stretch of one colour on the grid.
pub fn tokens_runs() -> Vec<(usize, usize, usize, u32)> {
    let colour = |letter: char| TOKENS_PALETTE.iter().find(|(l, _)| *l == letter).map(|(_, c)| *c);
    let mut runs = Vec::new();
    for (y, row) in TOKENS_PIXELS.iter().enumerate() {
        let row = row.as_bytes();
        let mut x = 0;
        while x < row.len() {
            let start = x;
            while x < row.len() && row[x] == row[start] {
                x += 1;
            }
            if let Some(c) = colour(row[start] as char) {
                runs.push((start, y, x - start, c));
            }
        }
    }
    runs
}

/// The tokens icon, as tall as the footer's text: each grid pixel a whole number of device
/// pixels (one on a Retina display, where the icon is 16 by 11 points), so the edges stay sharp.
fn tokens_icon(scale: f32) -> impl IntoElement {
    let unit = (FOOTER_TEXT * scale / TOKENS_PIXELS.len() as f32).round().max(1.0) / scale;
    let (w, h) = (unit * 32.0, unit * TOKENS_PIXELS.len() as f32);
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let snap = |v: f32| (v * scale).round() / scale;
            let x0 = snap(f32::from(bounds.origin.x));
            let y0 = snap(f32::from(bounds.origin.y) + (f32::from(bounds.size.height) - h) / 2.0);
            for (x, y, run, c) in tokens_runs() {
                let at = point(px(x0 + x as f32 * unit), px(y0 + y as f32 * unit));
                window.paint_quad(fill(Bounds { origin: at, size: size(px(run as f32 * unit), px(unit)) }, rgb(c)));
            }
        },
    )
    .flex_none()
    .w(px(w))
    .h(px(FOOTER_TEXT))
}

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
        format_date(t)
    }
}

/// The local date of `t` as "YYYY-MM-DD", as the table and the crash dialog write a day.
pub fn format_date(t: SystemTime) -> String {
    let at = local_tm(t);
    format!("{:04}-{:02}-{:02}", at.tm_year + 1900, at.tm_mon + 1, at.tm_mday)
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

    /// Redraws the window in the project window's new theme.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
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
        // The window draws the IDE's own title bar in the native one's place.
        titlebar: Some(TitlebarOptions { title: Some(TITLE.into()), appears_transparent: true, traffic_light_position: None }),
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        // The icon, a gap of 0.35 em, then the words in the footer's own size and colour.
        let scale = window.scale_factor();
        let donate = donation_visible(DONATE_URL).then(|| {
            div()
                .flex()
                .justify_center()
                .child(
                    div()
                        .id("about-donate")
                        .flex()
                        .items_center()
                        .gap(px(FOOTER_TEXT * 0.35))
                        .cursor_pointer()
                        .hover(|s| s.underline())
                        .on_click(|_, _, cx| cx.open_url(DONATE_URL))
                        .child(tokens_icon(scale))
                        .child(DONATE_WORDS),
                )
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
            .bg(bg)
            .text_color(text)
            .font_family(SharedString::from(UI_FONT))
            .child(chrome::aux_title_bar(TITLE, &self.theme))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .p(px(20.0))
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
                            .text_size(px(FOOTER_TEXT))
                            .text_color(dim)
                            .child(author)
                            .children(donate),
                    ),
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
    }

    #[test]
    fn the_donation_line_opens_the_arkos_door() {
        assert_eq!(DONATE_URL, "https://donate.csarko.sh/arkos");
        assert!(donation_visible(DONATE_URL));
        assert_eq!(DONATE_WORDS, "Buy me tokens");
    }

    #[test]
    fn the_tokens_icon_is_the_shared_32_by_22_pixel_map_in_its_ten_colours() {
        assert_eq!(TOKENS_PIXELS.len(), 22);
        assert!(TOKENS_PIXELS.iter().all(|row| row.len() == 32), "every row is 32 pixels");
        assert_eq!(TOKENS_PALETTE.len(), 10);
        for row in TOKENS_PIXELS {
            for letter in row.chars().filter(|&c| c != '.') {
                assert!(TOKENS_PALETTE.iter().any(|(l, _)| *l == letter), "{letter} has a colour");
            }
        }
        // The heart's outline and the token's mid-tone, as donate.csarko.sh/icon.svg draws them.
        assert!(TOKENS_PALETTE.contains(&('O', 0x6b1113)));
        assert!(TOKENS_PALETTE.contains(&('G', 0xf0c040)));
    }

    #[test]
    fn the_tokens_icon_paints_one_quad_for_each_run_of_a_colour() {
        let runs = tokens_runs();
        let painted: usize = runs.iter().map(|r| r.2).sum();
        let pixels = TOKENS_PIXELS.iter().flat_map(|row| row.chars()).filter(|&c| c != '.').count();
        assert_eq!(painted, pixels, "every pixel is painted once");
        assert_eq!(runs[0], (8, 1, 4, 0x6b1113), "the left lobe's outline opens the second row");
        assert!(runs.windows(2).all(|w| w[0].1 < w[1].1 || w[0].0 + w[0].2 <= w[1].0), "runs do not overlap");
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
