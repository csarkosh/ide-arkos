//! The project chooser. A Dock, Finder or Spotlight launch starts with `/` as its working
//! directory: the window asks which folder to open instead, listing the most recent ones
//! (`recent.rs`) with a way to show them all and the native folder picker. "Open recent..." from
//! a running window shows the same chooser; the folder chosen there opens in a new process.
use crate::config::Config;
use crate::keymap::{self, OpenFolder};
use crate::recent::{self, Recent};
use crate::theme::Theme;
use crate::paths;
use gpui::{
    div, prelude::*, px, size, App, Bounds, ClickEvent, Context, FocusHandle, KeyDownEvent, PathPromptOptions, ScrollHandle, Task, Window,
    WindowBounds, WindowHandle, WindowOptions,
};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The recent folders shown before "Show all".
pub const TOP: usize = 5;
const WIDTH: f32 = 560.0;
const HEIGHT: f32 = 460.0;
const TEXT: f32 = 13.0;

/// Whether this launch should ask for a folder: exactly `/` is what macOS gives an app opened
/// from the Dock, Finder or Spotlight. A terminal launch keeps its working directory.
pub fn needs_chooser(cwd: &Path) -> bool {
    cwd == Path::new("/")
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

/// What the chooser does with the folder picked: at launch, open the project window here; from
/// a running window, open it in a new process.
pub type OnChoice = Box<dyn FnOnce(PathBuf, &mut App)>;

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
}

impl Chooser {
    fn rows(&self) -> Vec<Row> {
        rows(&self.recents, self.expanded)
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.selected = move_selection(self.rows().len(), self.selected, delta);
        self.scroll.scroll_to_item(self.selected);
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
        if let Some(f) = self.on_choice.take() {
            f(folder, cx);
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
            "escape" => window.remove_window(),
            _ => return,
        }
        cx.stop_propagation();
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
            list = list.child(
                div()
                    .id(("chooser-row", ix))
                    .flex_none()
                    .flex()
                    .flex_row()
                    .gap(px(12.0))
                    .justify_between()
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
                    .child(div().min_w_0().overflow_hidden().text_ellipsis().whitespace_nowrap().text_color(dim).child(detail)),
            );
        }
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
            .text_size(px(TEXT))
            .child(div().px(px(10.0)).text_color(dim).child("Recent"))
            .when(self.recents.is_empty(), |d| d.child(div().px(px(10.0)).py(px(6.0)).text_color(dim).child("No recent folders")))
            .child(list)
    }
}

/// Opens the chooser once the recent list is read (off the UI thread). `at_launch`: closing it
/// with nothing chosen quits (the app's window-closed observer sees no windows left). `focus`
/// is whether the window takes the keyboard, as for the project window. The task gives the
/// window's handle once it is open.
pub fn open_chooser(cx: &mut App, at_launch: bool, focus: bool, config: Config, theme: Theme, on_choice: OnChoice) -> Task<Option<WindowHandle<Chooser>>> {
    let file = paths::recent_file();
    cx.spawn(async move |cx| {
        let recents = cx.background_executor().spawn(async move { recent::visible(&recent::load(&file)) }).await;
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
                });
                window.focus(&view.read(cx).focus);
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
}
