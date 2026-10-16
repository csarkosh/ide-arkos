//! The auxiliary windows' registry: the About and Settings windows, one of each at most, and the
//! project window's Root once it exists. A gpui global, so the actions registered once in `main()`
//! reach them in every state, the launch chooser's included, and the close rules can count them.
use crate::about::{self, AboutWindow};
use crate::config::{self, Config};
use crate::settings::{self, SettingsWindow, Shown};
use crate::theme::Theme;
use crate::{launch, paths, Root};
use gpui::{App, WeakEntity, WindowHandle};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::SystemTime;

/// What `main()` read before any window existed: what About and Settings show until the project
/// window's Root takes over, and the config file's fingerprint, so the project window can tell a
/// change made from the chooser.
pub struct LaunchState {
    pub config: Config,
    pub theme: Theme,
    pub config_path: PathBuf,
    /// When this install first ran (`startup::installed_since`).
    pub installed: SystemTime,
    /// `config_watch::fingerprint` of the config file as `main()` loaded it.
    pub fingerprint: Option<(u128, u64)>,
}

/// The registry. A handle stays here after its window closes; `aux_open` asks which are open.
#[derive(Default)]
pub struct Aux {
    pub about: Option<WindowHandle<AboutWindow>>,
    pub settings: Option<WindowHandle<SettingsWindow>>,
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

/// Records the project window's Root, from which About and Settings take their details from now
/// on.
pub fn set_root(cx: &mut App, root: WeakEntity<Root>) {
    cx.global_mut::<Aux>().root = Some(root);
}

/// The About menu item and command: brings the About window forward, or opens it. Its details
/// come from the project window's Root when there is one, else from what `main()` loaded, with no
/// daemon. Deferred, because the Root's own command runs this while the Root is being updated, and
/// gpui refuses a second update or read of an entity meanwhile.
pub fn open_about(cx: &mut App) {
    cx.defer(|cx| {
        let Some(aux) = cx.try_global::<Aux>() else { return };
        if let Some(handle) = aux.about {
            if handle.update(cx, |_, window, _| window.activate_window()).is_ok() {
                return;
            }
        }
        let shown = match root(cx) {
            Some(root) => {
                let root = root.read(cx);
                Some((root.details(), (*root.theme).clone()))
            }
            // Before the project window there is no daemon connection to report.
            None => cx.global::<Aux>().launch.as_ref().map(|l| (about::Details::gather(l.installed, None), l.theme.clone())),
        };
        let Some((details, theme)) = shown else { return };
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
            // A window opened from the chooser shows the Root's state once there is a Root.
            let pushed = shown.clone();
            if handle
                .update(cx, |w, window, cx| {
                    w.set_shown(pushed, cx);
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
    Some((Shown { config, error, update_ready: None }, launch.theme.clone()))
}

/// Shows `shown` in the Settings window when it is open.
pub fn push_settings(cx: &mut App, shown: Shown) {
    let Some(handle) = cx.try_global::<Aux>().and_then(|aux| aux.settings) else { return };
    let _ = handle.update(cx, |w, _window, cx| w.set_shown(shown, cx));
}

/// The Settings window's one write. With a project window, the Root reloads the config, which
/// applies the change and shows it in the window. Before that, the write is the whole change: the
/// file is read back so the window shows it, and the project window reads it when it opens. A
/// refused write shows as the window's banner. Looked up at each call, so a window opened from
/// the chooser writes through the Root once there is one.
fn apply_settings_edits(edits: &[(&[&str], toml_edit::Value)], cx: &mut App) {
    if let Some(root) = root(cx) {
        let written = config::set_values(&paths::config_file(), edits);
        root.update(cx, |root, cx| match written {
            Ok(()) => root.reload_config(cx),
            Err(e) => {
                eprintln!("arkos: settings: {e}");
                push_settings(cx, Shown { error: Some(e), ..root.settings_shown() });
            }
        });
        return;
    }
    let Some(launch) = cx.try_global::<Aux>().and_then(|aux| aux.launch.as_ref()) else { return };
    let (path, current) = (launch.config_path.clone(), launch.config.clone());
    let shown = match config::set_values(&path, edits) {
        Ok(()) => {
            let (config, error) = config::reload_decision(&current, config::load(&path));
            if error.is_none() {
                if let Some(launch) = cx.global_mut::<Aux>().launch.as_mut() {
                    launch.config = config.clone();
                }
            }
            Shown { config, error, update_ready: None }
        }
        Err(e) => {
            eprintln!("arkos: settings: {e}");
            Shown { config: current, error: Some(e), update_ready: None }
        }
    };
    push_settings(cx, shown);
}

/// The Settings window's "Update now": the bell's action on the project window. It is only shown
/// with a version ready, which only the Root knows.
fn settings_update_now(cx: &mut App) {
    if let Some(root) = root(cx) {
        root.update(cx, |root, cx| root.update_clicked(cx));
    }
}

/// How many of the About and Settings windows are open.
pub fn aux_open(cx: &App) -> usize {
    let Some(aux) = cx.try_global::<Aux>() else { return 0 };
    // A window being updated is still listed by `windows()`, while `read` would fail on it.
    let windows = cx.windows();
    let open = |id: Option<gpui::WindowId>| id.is_some_and(|id| windows.iter().any(|w| w.window_id() == id));
    usize::from(open(aux.about.map(|h| h.window_id()))) + usize::from(open(aux.settings.map(|h| h.window_id())))
}

/// Closes the About and Settings windows and forgets their handles.
pub fn close_all(cx: &mut App) {
    let Some(aux) = cx.try_global::<Aux>() else { return };
    let (about, settings) = (aux.about, aux.settings);
    if let Some(h) = about {
        let _ = h.update(cx, |_, window, _| window.remove_window());
    }
    if let Some(h) = settings {
        let _ = h.update(cx, |_, window, _| window.remove_window());
    }
    let aux = cx.global_mut::<Aux>();
    aux.about = None;
    aux.settings = None;
}

/// New Window pressed in About or Settings: the project window's New Window when there is one;
/// before that, the launch chooser comes forward, as its own New Window does. Deferred, as
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
