//! The auxiliary windows' registry: the About, Settings and Keybindings windows and the crash
//! dialog, one of each at most, and the project window's Root once it exists. A gpui global, so
//! the actions registered once in `main()` reach them in every state, the launch chooser's
//! included, and the close rules can count them.
use crate::about::{self, AboutWindow};
use crate::config::{self, Config};
use crate::crash_dialog::CrashDialog;
use crate::keybindings::{self, KeybindingsWindow};
use crate::settings::{self, SettingsWindow, Shown};
use crate::theme::Theme;
use crate::{keymap, launch, paths, Root};
use gpui::{App, SharedString, WeakEntity, WindowHandle};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::SystemTime;

/// What `main()` read before any window existed: what About, Settings and Keybindings show
/// until the project window's Root takes over, and the config file's fingerprint, so the project
/// window can tell a change made from the chooser.
pub struct LaunchState {
    pub config: Config,
    pub theme: Theme,
    pub config_path: PathBuf,
    /// When this install first ran (`startup::installed_since`).
    pub installed: SystemTime,
    /// `config_watch::fingerprint` of the config file as `main()` loaded it.
    pub fingerprint: Option<(u128, u64)>,
    /// The launch's connection to the daemon once it exists (`set_launch_client`), on which a
    /// change made in Settings before the project window says a new session id.
    pub client: Option<std::sync::Arc<crate::client::Client>>,
}

/// The registry. A handle stays here after its window closes; `aux_open` asks which are open.
#[derive(Default)]
pub struct Aux {
    pub about: Option<WindowHandle<AboutWindow>>,
    pub settings: Option<WindowHandle<SettingsWindow>>,
    pub keybindings: Option<WindowHandle<KeybindingsWindow>>,
    /// The crash dialog, opened at most once a run (`crash_dialog::at_launch`).
    pub crash: Option<WindowHandle<CrashDialog>>,
    /// The project window's Root, once it exists.
    pub root: Option<WeakEntity<Root>>,
    /// Set by `init` before any window opens.
    pub launch: Option<LaunchState>,
}

impl gpui::Global for Aux {}

/// Sets the registry up with what `main()` loaded. Called once, before any window opens.
pub fn init(cx: &mut App, launch: LaunchState) {
    cx.set_global(Aux { launch: Some(launch), ..Aux::default() });
}

/// Records the launch's connection to the daemon, for the Settings window's writes made before
/// the project window exists.
pub fn set_launch_client(cx: &mut App, client: std::sync::Arc<crate::client::Client>) {
    if !cx.has_global::<Aux>() {
        return;
    }
    if let Some(launch) = cx.global_mut::<Aux>().launch.as_mut() {
        launch.client = Some(client);
    }
}

/// Records the project window's Root, from which About, Settings and Keybindings take their
/// details from now on.
pub fn set_root(cx: &mut App, root: WeakEntity<Root>) {
    cx.global_mut::<Aux>().root = Some(root);
}

/// The About menu item and command: brings the About window forward with fresh details, or opens
/// it. The details come from the project window's Root when there is one, else from what `main()`
/// loaded, with no daemon. Deferred, because the Root's own command runs this while the Root is
/// being updated, and gpui refuses a second update or read of an entity meanwhile.
pub fn open_about(cx: &mut App) {
    crate::telemetry::help(crate::telemetry::Help::About);
    cx.defer(|cx| {
        let Some(aux) = cx.try_global::<Aux>() else { return };
        let handle = aux.about;
        let shown = match root(cx) {
            Some(root) => {
                let root = root.read(cx);
                Some((root.details(), (*root.theme).clone()))
            }
            // Before the project window there is no daemon connection to report.
            None => cx.global::<Aux>().launch.as_ref().map(|l| (about::Details::gather(l.installed, None), l.theme.clone())),
        };
        let Some((details, theme)) = shown else { return };
        // An open About takes the fresh details, so one opened from the chooser stops saying "not
        // connected" once the project window exists. Its window may have closed meanwhile.
        if let Some(h) = handle {
            let (d, t) = (details.clone(), theme.clone());
            let refreshed = h.update(cx, move |view, window, cx| {
                view.refresh(d, t, cx);
                window.activate_window();
            });
            if refreshed.is_ok() {
                return;
            }
        }
        let handle = about::open(cx, details, theme);
        cx.global_mut::<Aux>().about = handle;
    });
}

/// The Settings menu item and Cmd+,: brings the Settings window forward with what is in force
/// now, or opens it. The config comes from the project window's Root when there is one, else from
/// the file as it is now. Deferred, because the Root's own command runs this while the Root is
/// being updated, and gpui refuses a second update or read of an entity meanwhile.
pub fn open_settings(cx: &mut App) {
    cx.defer(|cx| {
        let Some(aux) = cx.try_global::<Aux>() else { return };
        let handle = aux.settings;
        let Some((shown, theme)) = settings_state(cx) else { return };
        if let Some(handle) = handle {
            // A window opened from the chooser shows the Root's state, and its theme, once there
            // is a Root.
            let (pushed, drawn_in) = (shown.clone(), theme.clone());
            if handle
                .update(cx, |w, window, cx| {
                    w.set_shown(pushed, cx);
                    w.set_theme(drawn_in, cx);
                    window.activate_window();
                })
                .is_ok()
            {
                return;
            }
        }
        let fonts = settings::monospace_families(&cx.text_system().all_font_names());
        let handle = settings::open(cx, shown, theme, fonts, Rc::new(apply_settings_edits), Rc::new(settings_update_now));
        cx.global_mut::<Aux>().settings = handle;
    });
}

/// What the Settings window shows and the theme it is drawn in: the Root's, or before the project
/// window exists, the file read again (Settings may have changed it) and the launch theme.
fn settings_state(cx: &App) -> Option<(Shown, Theme)> {
    if let Some(root) = root(cx) {
        let root = root.read(cx);
        return Some((root.settings_shown(), (*root.theme).clone()));
    }
    let launch = cx.try_global::<Aux>()?.launch.as_ref()?;
    let (config, error) = config::reload_decision(&launch.config, config::load(&launch.config_path));
    Some((Shown { config, error, write_error: None, update_ready: None, active_preset: None, sandbox: ::telemetry::is_sandbox() }, launch.theme.clone()))
}

/// The Keybindings menu item, Cmd+/ and Settings' "Keybindings": brings the Keybindings window
/// forward with the bindings in force now, or opens it. The bindings come from the project
/// window's Root when there is one, else from what `main()` loaded. Deferred, because the Root's
/// own command runs this while the Root is being updated, and gpui refuses a second update or
/// read of an entity meanwhile.
pub fn open_keybindings(cx: &mut App) {
    crate::telemetry::help(crate::telemetry::Help::Keybindings);
    cx.defer(|cx| {
        let Some(aux) = cx.try_global::<Aux>() else { return };
        let handle = aux.keybindings;
        let Some((overrides, theme, mono)) = keybindings_state(cx) else { return };
        let rows = keymap::grouped(&overrides);
        if let Some(handle) = handle {
            // Its window may have closed meanwhile.
            let (pushed, drawn_in) = (rows.clone(), theme.clone());
            if handle
                .update(cx, |w, window, cx| {
                    w.set_rows(pushed, cx);
                    w.set_theme(drawn_in, cx);
                    window.activate_window();
                })
                .is_ok()
            {
                return;
            }
        }
        let handle = keybindings::open(cx, rows, theme, mono);
        cx.global_mut::<Aux>().keybindings = handle;
    });
}

/// The Keybindings window's overrides, theme and monospace family: the Root's, or before the
/// project window exists, the launch config's and the launch theme.
fn keybindings_state(cx: &App) -> Option<(BTreeMap<String, String>, Theme, SharedString)> {
    if let Some(root) = root(cx) {
        let root = root.read(cx);
        return Some((root.config.keybindings.clone(), (*root.theme).clone(), root.family()));
    }
    let launch = cx.try_global::<Aux>()?.launch.as_ref()?;
    Some((launch.config.keybindings.clone(), launch.theme.clone(), launch.config.font.family.clone().into()))
}

/// Where the Documentation menu item and command go: the guide on GitHub (the README is the
/// short overview; the guide has every section).
pub const DOCS_URL: &str = "https://github.com/csarkosh/ide-arkos/blob/main/docs/guide.md";

/// The Documentation menu item and command: opens `DOCS_URL` in the default browser.
pub fn open_documentation(cx: &mut App) {
    crate::telemetry::help(crate::telemetry::Help::Documentation);
    cx.open_url(DOCS_URL);
}

/// The Help menu's "What Arkos Sends" outside the project window (the chooser, Settings, or
/// before any window): writes the disclosure page into the telemetry folder and opens it in the
/// default app for Markdown. The project window's own handler opens it in a viewer tab instead.
pub fn open_disclosure(cx: &mut App) {
    crate::telemetry::help(crate::telemetry::Help::Telemetry);
    match ::telemetry::disclosure::write_page(&paths::telemetry_dir()) {
        Ok(path) => cx.open_with_system(&path),
        Err(e) => eprintln!("arkos: cannot write the disclosure page: {e}"),
    }
}

/// Runs `f` on the project window's Root and brings that window forward; false when there is no
/// project window to run it in.
fn in_project_window(cx: &mut App, f: impl FnOnce(&mut Root, &mut gpui::Context<Root>)) -> bool {
    let project = cx.windows().into_iter().find_map(|w| w.downcast::<Root>());
    project.is_some_and(|handle| {
        handle
            .update(cx, |root, window, cx| {
                f(root, cx);
                window.activate_window();
            })
            .is_ok()
    })
}

/// Settings' "What Arkos sends": the disclosure page in a viewer tab of the project window, as its
/// Help item opens it there (`Root::open_disclosure`); before there is one, in the default app for
/// Markdown, as the chooser's Help item does. Deferred, as `open_settings` is.
pub fn open_disclosure_from_settings(cx: &mut App) {
    cx.defer(|cx| {
        if !in_project_window(cx, |root, cx| root.open_disclosure(cx)) {
            open_disclosure(cx);
        }
    });
}

/// Settings' "Last report": the session the daemon sealed last, `file`, in a viewer tab of the
/// project window, as "What Arkos sends" opens; before there is one, in the default app for JSON.
/// Deferred, as `open_settings` is.
pub fn open_last_report(cx: &mut App, file: PathBuf) {
    cx.defer(move |cx| {
        let opened = in_project_window(cx, |root, cx| {
            let _via = crate::telemetry::opened_via(crate::telemetry::Opened::Click);
            root.open_viewer(&file, true, cx);
        });
        if !opened {
            cx.open_with_system(&file);
        }
    });
}

/// Shows `shown` in the Settings window when it is open.
pub fn push_settings(cx: &mut App, shown: Shown) {
    let Some(handle) = cx.try_global::<Aux>().and_then(|aux| aux.settings) else { return };
    let _ = handle.update(cx, |w, _window, cx| w.set_shown(shown, cx));
}

/// Shows the bindings in force under `overrides` in the Keybindings window when it is open, so a
/// saved override shows at once.
pub fn push_keybindings(cx: &mut App, overrides: &BTreeMap<String, String>) {
    let Some(handle) = cx.try_global::<Aux>().and_then(|aux| aux.keybindings) else { return };
    let rows = keymap::grouped(overrides);
    let _ = handle.update(cx, |w, _window, cx| w.set_rows(rows, cx));
}

/// Redraws the open About, Settings and Keybindings windows and the crash dialog in `theme`, the
/// project window's new one.
pub fn push_theme(cx: &mut App, theme: &Theme) {
    let Some(aux) = cx.try_global::<Aux>() else { return };
    let (about, settings, keybindings, crash) = (aux.about, aux.settings, aux.keybindings, aux.crash);
    if let Some(h) = about {
        let theme = theme.clone();
        let _ = h.update(cx, |w, _window, cx| w.set_theme(theme, cx));
    }
    if let Some(h) = settings {
        let theme = theme.clone();
        let _ = h.update(cx, |w, _window, cx| w.set_theme(theme, cx));
    }
    if let Some(h) = keybindings {
        let theme = theme.clone();
        let _ = h.update(cx, |w, _window, cx| w.set_theme(theme, cx));
    }
    if let Some(h) = crash {
        let theme = theme.clone();
        let _ = h.update(cx, |w, _window, cx| w.set_theme(theme, cx));
    }
}

/// The Settings window's one write. With a project window, the Root reloads the config, which
/// applies the change and shows it in the window; a picked preset (the default for new
/// workspaces) is applied to this window as well. Before that, the write is the whole change: the
/// file is read back so the window shows it, and the project window reads it when it opens. A
/// refused write is said in the window above its controls, which stay live. Looked up at each
/// call, so a window opened from the chooser writes through the Root once there is one.
fn apply_settings_edits(edits: &[(&[&str], toml_edit::Value)], cx: &mut App) {
    if let Some(root) = root(cx) {
        let written = config::set_values(&paths::config_file(), edits);
        let picked = picked_preset(edits);
        root.update(cx, |root, cx| match written {
            Ok(()) => {
                root.reload_config(cx);
                // A changed default is applied by the reload; the default picked again is not.
                if let Some(name) = picked.filter(|name| *name != root.text_preset) {
                    if root.apply_text_preset(&name, cx) {
                        push_settings(cx, root.settings_shown());
                    }
                }
            }
            Err(e) => {
                eprintln!("arkos: settings: {e}");
                push_settings(cx, root.settings_shown().with_write_error(e));
            }
        });
        return;
    }
    let Some(launch) = cx.try_global::<Aux>().and_then(|aux| aux.launch.as_ref()) else { return };
    let (path, current) = (launch.config_path.clone(), launch.config.clone());
    let written = config::set_values(&path, edits);
    let (config, error) = config::reload_decision(&current, config::load(&path));
    if written.is_ok() && error.is_none() {
        // The Recorder takes the change now, as the project window's reload would: turning
        // usage on restarts the journal and says the new session id on the launch's connection.
        crate::telemetry::config_applied(&current, &config, launch.client.as_deref());
        if let Some(launch) = cx.global_mut::<Aux>().launch.as_mut() {
            launch.config = config.clone();
        }
    }
    let shown = Shown { config, error, write_error: None, update_ready: None, active_preset: None, sandbox: ::telemetry::is_sandbox() };
    let shown = match written {
        Ok(()) => shown,
        Err(e) => {
            eprintln!("arkos: settings: {e}");
            shown.with_write_error(e)
        }
    };
    push_settings(cx, shown);
}

/// A crash report was decided: an open Settings window reads its Reports list again.
pub fn reports_changed(cx: &mut App) {
    let Some(handle) = cx.try_global::<Aux>().and_then(|aux| aux.settings) else { return };
    let _ = handle.update(cx, |w, _window, cx| w.reports_changed(cx));
}

/// The preset name an edit of `[font] preset` writes, if the edits hold one.
fn picked_preset(edits: &[(&[&str], toml_edit::Value)]) -> Option<String> {
    edits.iter().find(|(keys, _)| *keys == ["font", "preset"]).and_then(|(_, v)| v.as_str()).map(str::to_string)
}

/// The Settings window's "Update now": the bell's action on the project window. It is only shown
/// with a version ready, which only the Root knows.
fn settings_update_now(cx: &mut App) {
    if let Some(root) = root(cx) {
        root.update(cx, |root, cx| root.update_clicked(cx));
    }
}

/// How many of the About, Settings and Keybindings windows and the crash dialog are open.
pub fn aux_open(cx: &App) -> usize {
    let Some(aux) = cx.try_global::<Aux>() else { return 0 };
    // A window being updated is still listed by `windows()`, while `read` would fail on it.
    let windows = cx.windows();
    let open = |id: Option<gpui::WindowId>| id.is_some_and(|id| windows.iter().any(|w| w.window_id() == id));
    usize::from(open(aux.about.map(|h| h.window_id())))
        + usize::from(open(aux.settings.map(|h| h.window_id())))
        + usize::from(open(aux.keybindings.map(|h| h.window_id())))
        + usize::from(open(aux.crash.map(|h| h.window_id())))
}

/// Closes the About, Settings and Keybindings windows and the crash dialog, and forgets their
/// handles. A crash dialog closed this way made no choice: its reports wait for the next launch.
pub fn close_all(cx: &mut App) {
    let Some(aux) = cx.try_global::<Aux>() else { return };
    let (about, settings, keybindings, crash) = (aux.about, aux.settings, aux.keybindings, aux.crash);
    if let Some(h) = about {
        let _ = h.update(cx, |_, window, _| window.remove_window());
    }
    if let Some(h) = settings {
        let _ = h.update(cx, |_, window, _| window.remove_window());
    }
    if let Some(h) = keybindings {
        let _ = h.update(cx, |_, window, _| window.remove_window());
    }
    if let Some(h) = crash {
        let _ = h.update(cx, |_, window, _| window.remove_window());
    }
    let aux = cx.global_mut::<Aux>();
    aux.about = None;
    aux.settings = None;
    aux.keybindings = None;
    aux.crash = None;
}

/// New Window pressed in About, Settings or Keybindings: the project window's New Window when
/// there is one; before that, the launch chooser comes forward, as its own New Window does. Deferred, as
/// `open_settings` is.
pub fn forward_new_window(cx: &mut App) {
    let root = root(cx);
    cx.defer(move |cx| {
        if let Some(root) = root {
            root.update(cx, |root, cx| root.new_window(cx));
            return;
        }
        let chooser = cx.windows().into_iter().find_map(|w| w.downcast::<launch::Chooser>());
        match chooser {
            Some(h) => {
                let _ = h.update(cx, |_, window, _| window.activate_window());
            }
            None => eprintln!("arkos: New Window asked for with no project window and no chooser open"),
        }
    });
}

/// The project window's Root while it lives.
fn root(cx: &App) -> Option<gpui::Entity<Root>> {
    cx.try_global::<Aux>()?.root.as_ref()?.upgrade()
}
