//! The login item that brings sessions back after a restart (update-in-place spec, section 11
//! step 3): a launchd agent in `~/Library/LaunchAgents` that runs `arkosd --recover` once at
//! login. No KeepAlive: a daemon the window started is left alone, and `--recover` exits 0 when
//! one is listening. Installed only when the user asks (the palette) or the config says "on".
//!
//! The window never unloads the agent: once a recovery has run, the serving daemon is the
//! launchd job's process, and `launchctl bootout` would end it and every session with it. A
//! rewrite or a removal takes effect at the next login, the only time the agent runs anyway.
//! `install_with` takes the `launchctl` runner as a parameter, so the tests never run it.

use crate::config::StartAtLogin;
use std::path::{Path, PathBuf};

pub const LABEL: &str = "sh.csarko.arkosd";

pub fn agent_path() -> PathBuf {
    PathBuf::from(std::env::var("HOME").expect("HOME is set")).join("Library/LaunchAgents").join(format!("{LABEL}.plist"))
}

pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The agent: `arkosd --recover` with the window's `[daemon]` arguments, once at load, logging
/// where the window's own daemon logs. `ExitTimeOut` gives the SIGTERM flush twenty seconds
/// before launchd's SIGKILL at logout.
pub fn plist(daemon_exe: &Path, daemon_args: &[String], log: &Path) -> String {
    let mut args = vec![daemon_exe.to_string_lossy().into_owned(), "--recover".to_string()];
    args.extend(daemon_args.iter().cloned());
    let items: String = args.iter().map(|a| format!("\t\t<string>{}</string>\n", xml_escape(a))).collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n<dict>\n\
\t<key>Label</key>\n\t<string>{LABEL}</string>\n\
\t<key>ProgramArguments</key>\n\t<array>\n{items}\t</array>\n\
\t<key>RunAtLoad</key>\n\t<true/>\n\
\t<key>KeepAlive</key>\n\t<false/>\n\
\t<key>ProcessType</key>\n\t<string>Interactive</string>\n\
\t<key>ExitTimeOut</key>\n\t<integer>20</integer>\n\
\t<key>StandardOutPath</key>\n\t<string>{log}</string>\n\
\t<key>StandardErrorPath</key>\n\t<string>{log}</string>\n\
</dict>\n</plist>\n",
        log = xml_escape(&log.to_string_lossy())
    )
}

/// Whether a window running from `bundle` may write the login item: only the installed app,
/// `/Applications/Arkos IDE.app` or `~/Applications/Arkos IDE.app`, exactly. A copy run from a
/// scratch folder, or from the update's `Arkos IDE.app.new` staging name, must never be what
/// launchd runs at the next login.
pub fn may_install(bundle: &Path, home: &Path) -> bool {
    const APP: &str = "Arkos IDE.app";
    bundle == Path::new("/Applications").join(APP) || bundle == home.join("Applications").join(APP)
}

/// The bundle this window runs from (`<bundle>/Contents/MacOS/arkos`), as launched.
pub fn running_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.parent()?.parent()?.to_path_buf())
}

/// `may_install` for this process.
pub fn may_install_here() -> bool {
    let home = PathBuf::from(std::env::var("HOME").expect("HOME is set"));
    running_bundle().is_some_and(|b| may_install(&b, &home))
}

pub fn installed(path: &Path) -> bool {
    path.is_file()
}

/// What to do so the file matches the config: `Some(true)` install (or rewrite), `Some(false)`
/// remove, `None` leave it. "ask" never adds or removes it (the palette does, with the user's
/// say-so) but keeps an installed one current: the binary or the `[daemon]` values may have moved.
pub fn desired(mode: StartAtLogin, installed: bool) -> Option<bool> {
    match (mode, installed) {
        (StartAtLogin::On, _) | (StartAtLogin::Ask, true) => Some(true),
        (StartAtLogin::Off, true) => Some(false),
        (StartAtLogin::Off, false) | (StartAtLogin::Ask, false) => None,
    }
}

fn gui_domain() -> String {
    // SAFETY: getuid takes no arguments and cannot fail.
    format!("gui/{}", unsafe { libc::getuid() })
}

/// Runs `launchctl` with `args`: `Ok(true)` when it exits 0, `Ok(false)` when it fails.
fn launchctl(args: &[&str]) -> Result<bool, String> {
    let out = std::process::Command::new("launchctl").args(args).output().map_err(|e| format!("cannot run launchctl: {e}"))?;
    Ok(out.status.success())
}

/// `install_with` running the real `launchctl`. Blocks on it: call it off the UI thread.
pub fn install(path: &Path, contents: &str) -> Result<bool, String> {
    install_with(path, contents, &launchctl)
}

/// Writes the plist when it differs, and loads it into this login session only when the label
/// is not loaded yet, so a bad file fails now rather than at the next login. Loading runs
/// `arkosd --recover` once, which exits at once while a daemon is listening. A loaded label is
/// left alone (never booted out: it may be the serving daemon); the new file is read at the next
/// login. `Ok(true)` when it wrote, `Ok(false)` when the file already matched (nothing done).
/// `run` runs `launchctl` with its arguments and says whether it exited 0.
pub fn install_with(path: &Path, contents: &str, run: &dyn Fn(&[&str]) -> Result<bool, String>) -> Result<bool, String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    if std::fs::read_to_string(path).ok().as_deref() == Some(contents) {
        return Ok(false);
    }
    std::fs::write(path, contents).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    let domain = gui_domain();
    if run(&["print", &format!("{domain}/{LABEL}")])? {
        return Ok(true);
    }
    let path = path.to_string_lossy();
    if !run(&["bootstrap", &domain, &path])? {
        return Err(format!("launchctl bootstrap {domain} {path} failed"));
    }
    Ok(true)
}

/// Deletes the agent's file. Never unloads it: a running job (the serving daemon, after a
/// recovery) is left alone, and launchd finds nothing at the next login. A missing file is fine
/// (`Ok(false)`); `Ok(true)` when it removed one.
pub fn remove(path: &Path) -> Result<bool, String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("cannot remove {}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plist_runs_recover_once_at_login_with_the_daemon_arguments_escaped() {
        let p = plist(Path::new("/Users/me/My Apps/arkosd"), &["--watchdog-secs".into(), "30".into()], Path::new("/Users/me/Library/Application Support/arkos/arkosd.log"));
        assert!(p.contains("<string>sh.csarko.arkosd</string>"));
        assert!(p.contains("<string>/Users/me/My Apps/arkosd</string>\n\t\t<string>--recover</string>\n\t\t<string>--watchdog-secs</string>\n\t\t<string>30</string>"));
        assert!(p.contains("<key>RunAtLoad</key>\n\t<true/>"));
        assert!(p.contains("<key>KeepAlive</key>\n\t<false/>"));
        assert!(p.contains("<key>ExitTimeOut</key>\n\t<integer>20</integer>"));
        assert!(p.contains("<string>/Users/me/Library/Application Support/arkos/arkosd.log</string>"));
        assert_eq!(xml_escape("a&b<c>\"d\""), "a&amp;b&lt;c&gt;&quot;d&quot;");
        assert!(plist(Path::new("/x&y/arkosd"), &[], Path::new("/l")).contains("<string>/x&amp;y/arkosd</string>"));
        assert!(agent_path().ends_with("Library/LaunchAgents/sh.csarko.arkosd.plist"));
    }

    #[test]
    fn the_config_keeps_on_and_off_in_force_and_ask_leaves_the_file_alone() {
        assert_eq!(desired(StartAtLogin::On, false), Some(true));
        assert_eq!(desired(StartAtLogin::On, true), Some(true), "rewritten when the arguments changed; a same file is a no-op");
        assert_eq!(desired(StartAtLogin::Off, true), Some(false));
        assert_eq!(desired(StartAtLogin::Off, false), None);
        assert_eq!(desired(StartAtLogin::Ask, false), None);
        assert_eq!(desired(StartAtLogin::Ask, true), Some(true), "an installed item follows the binary and the daemon settings");
    }

    /// A fake `launchctl`: records every call and answers `print` with `loaded`, the rest with 0.
    fn fake(loaded: bool) -> (std::rc::Rc<std::cell::RefCell<Vec<Vec<String>>>>, impl Fn(&[&str]) -> Result<bool, String>) {
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let seen = calls.clone();
        let run = move |args: &[&str]| {
            seen.borrow_mut().push(args.iter().map(|a| a.to_string()).collect::<Vec<_>>());
            Ok(args[0] != "print" || loaded)
        };
        (calls, run)
    }

    #[test]
    fn an_unchanged_file_is_neither_rewritten_nor_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.plist");
        std::fs::write(&p, "same").unwrap();
        let (calls, run) = fake(false);
        assert_eq!(install_with(&p, "same", &run), Ok(false));
        assert!(calls.borrow().is_empty(), "no launchctl");
    }

    #[test]
    fn a_changed_file_is_written_but_a_loaded_label_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.plist");
        std::fs::write(&p, "old").unwrap();
        let (calls, run) = fake(true);
        assert_eq!(install_with(&p, "new", &run), Ok(true));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "new");
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1, "only the check: {calls:?}");
        assert_eq!(calls[0][0], "print");
        assert!(calls[0][1].ends_with("/sh.csarko.arkosd"));
        assert!(!calls.iter().any(|c| c[0] == "bootout" || c[0] == "bootstrap"));
    }

    #[test]
    fn a_new_file_is_written_and_loaded_when_the_label_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("agents").join("a.plist");
        let (calls, run) = fake(false);
        assert_eq!(install_with(&p, "new", &run), Ok(true));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "new");
        let calls = calls.borrow();
        assert_eq!(calls.iter().map(|c| c[0].as_str()).collect::<Vec<_>>(), ["print", "bootstrap"]);
        assert_eq!(calls[1][2], p.to_string_lossy());
    }

    #[test]
    fn a_failed_load_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.plist");
        assert!(install_with(&p, "new", &|_: &[&str]| Ok(false)).unwrap_err().contains("bootstrap"));
    }

    #[test]
    fn remove_deletes_the_file_only() {
        // `remove` takes no runner: it cannot reach launchctl.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.plist");
        std::fs::write(&p, "x").unwrap();
        assert_eq!(remove(&p), Ok(true));
        assert!(!p.exists());
        assert_eq!(remove(&p), Ok(false), "a missing file is fine");
    }

    #[test]
    fn only_the_installed_app_may_write_the_login_item() {
        let home = Path::new("/Users/me");
        assert!(may_install(Path::new("/Applications/Arkos IDE.app"), home));
        assert!(may_install(Path::new("/Users/me/Applications/Arkos IDE.app"), home));
        assert!(!may_install(Path::new("/Users/me/scratch/Arkos IDE.app"), home), "a copy run from a scratch folder");
        assert!(!may_install(Path::new("/Applications/Arkos IDE.app.new"), home), "the update's staging name");
        assert!(!may_install(Path::new("/Applications/Arkos IDE.app.old"), home), "the update's kept previous app");
        assert!(!may_install(Path::new("/Applications/Arkos IDE.app.new/Contents/MacOS/Arkos IDE.app"), home));
        assert!(!may_install(Path::new("/Users/other/Applications/Arkos IDE.app"), home), "another user's");
    }

    #[test]
    fn installed_is_whether_the_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.plist");
        assert!(!installed(&p));
        std::fs::write(&p, "x").unwrap();
        assert!(installed(&p));
    }
}
