//! The auxiliary windows' registry: the About and Settings windows, one of each at most, and the
//! project window's Root once it exists. A gpui global, so the actions registered once in `main()`
//! reach them in every state, the launch chooser's included, and the close rules can count them.
use crate::about::AboutWindow;
use crate::config::Config;
use crate::settings::SettingsWindow;
use crate::theme::Theme;
use crate::{launch, Root};
use gpui::{App, WeakEntity, WindowHandle};
use std::path::PathBuf;
use std::time::SystemTime;

/// What `main()` read before any window existed: what About and Settings show until the project
/// window's Root takes over, and the config file's fingerprint, so the project window can tell a
/// change made from the chooser.
pub struct LaunchState {
    #[allow(dead_code)] // Used from Task 4 on.
    pub config: Config,
    #[allow(dead_code)] // Used from Task 3b on.
    pub theme: Theme,
    #[allow(dead_code)] // Used from Task 4 on.
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

/// The About menu item and command: brings the About window forward, or opens it. Until Task 3b
/// fills the window in, this only says it was asked.
pub fn open_about(_cx: &mut App) {
    eprintln!("arkos: About Arkos IDE asked for; the window arrives in Task 3b");
}

/// The Settings menu item and Cmd+,: brings the Settings window forward, or opens it. Until Task 4
/// fills the window in, this shows a notice on the project window, or says so in the log when
/// there is none yet. Deferred, because the Root's own command runs this while the Root is being
/// updated, and gpui refuses a second update of an entity meanwhile.
pub fn open_settings(cx: &mut App) {
    let root = root(cx);
    cx.defer(move |cx| match root {
        Some(root) => root.update(cx, |root, cx| root.show_notice("Settings arrive in the next task".into(), cx)),
        None => eprintln!("arkos: Settings asked for before the project window; they arrive in the next task"),
    });
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
#[allow(dead_code)] // Used from Task 3b on.
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
