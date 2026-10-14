//! The login item that brings sessions back after a restart (update-in-place spec, section 11
//! step 3): a launchd agent in `~/Library/LaunchAgents` that runs `sarkosd --recover` once at
//! login. No KeepAlive: a daemon the window started is left alone, and `--recover` exits 0 when
//! one is listening. Installed only when the user asks (the palette) or the config says "on".
//!
//! `install` and `remove` call `launchctl` and are not unit-tested: the README checklist covers
//! them. The pure parts (the plist, the escaping, the decision) are tested below.

use crate::config::StartAtLogin;
use std::path::{Path, PathBuf};

pub const LABEL: &str = "sh.csarko.sarkosd";

pub fn agent_path() -> PathBuf {
    PathBuf::from(std::env::var("HOME").expect("HOME is set")).join("Library/LaunchAgents").join(format!("{LABEL}.plist"))
}

pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The agent: `sarkosd --recover` with the window's `[daemon]` arguments, once at load, logging
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

pub fn installed(path: &Path) -> bool {
    path.is_file()
}

/// What to do so the file matches the config: `Some(true)` install (or rewrite), `Some(false)`
/// remove, `None` leave it. "ask" never touches it: the palette does, with the user's say-so.
pub fn desired(mode: StartAtLogin, installed: bool) -> Option<bool> {
    match (mode, installed) {
        (StartAtLogin::On, _) => Some(true),
        (StartAtLogin::Off, true) => Some(false),
        (StartAtLogin::Off, false) | (StartAtLogin::Ask, _) => None,
    }
}

fn gui_domain() -> String {
    // SAFETY: getuid takes no arguments and cannot fail.
    format!("gui/{}", unsafe { libc::getuid() })
}

fn launchctl(args: &[&str]) -> Result<(), String> {
    let out = std::process::Command::new("launchctl").args(args).output().map_err(|e| format!("cannot run launchctl: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("launchctl {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Writes the plist and loads it into this login session, so a bad file fails now rather than
/// at the next login. Loading runs `sarkosd --recover` once, which exits at once while a daemon
/// is listening. Blocks on launchctl: call it off the UI thread.
pub fn install(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    if std::fs::read_to_string(path).ok().as_deref() == Some(contents) {
        return Ok(());
    }
    std::fs::write(path, contents).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    // A definition already loaded (an earlier install) must go before the new one loads.
    let _ = launchctl(&["bootout", &format!("{}/{LABEL}", gui_domain())]);
    launchctl(&["bootstrap", &gui_domain(), &path.to_string_lossy()])
}

/// Unloads and deletes the agent. A missing file is fine.
pub fn remove(path: &Path) -> Result<(), String> {
    let _ = launchctl(&["bootout", &format!("{}/{LABEL}", gui_domain())]);
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("cannot remove {}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plist_runs_recover_once_at_login_with_the_daemon_arguments_escaped() {
        let p = plist(Path::new("/Users/me/My Apps/sarkosd"), &["--watchdog-secs".into(), "30".into()], Path::new("/Users/me/Library/Application Support/sarkos/sarkosd.log"));
        assert!(p.contains("<string>sh.csarko.sarkosd</string>"));
        assert!(p.contains("<string>/Users/me/My Apps/sarkosd</string>\n\t\t<string>--recover</string>\n\t\t<string>--watchdog-secs</string>\n\t\t<string>30</string>"));
        assert!(p.contains("<key>RunAtLoad</key>\n\t<true/>"));
        assert!(p.contains("<key>KeepAlive</key>\n\t<false/>"));
        assert!(p.contains("<key>ExitTimeOut</key>\n\t<integer>20</integer>"));
        assert!(p.contains("<string>/Users/me/Library/Application Support/sarkos/sarkosd.log</string>"));
        assert_eq!(xml_escape("a&b<c>\"d\""), "a&amp;b&lt;c&gt;&quot;d&quot;");
        assert!(plist(Path::new("/x&y/sarkosd"), &[], Path::new("/l")).contains("<string>/x&amp;y/sarkosd</string>"));
        assert!(agent_path().ends_with("Library/LaunchAgents/sh.csarko.sarkosd.plist"));
    }

    #[test]
    fn the_config_keeps_on_and_off_in_force_and_ask_leaves_the_file_alone() {
        assert_eq!(desired(StartAtLogin::On, false), Some(true));
        assert_eq!(desired(StartAtLogin::On, true), Some(true), "rewritten when the arguments changed; a same file is a no-op");
        assert_eq!(desired(StartAtLogin::Off, true), Some(false));
        assert_eq!(desired(StartAtLogin::Off, false), None);
        assert_eq!(desired(StartAtLogin::Ask, false), None);
        assert_eq!(desired(StartAtLogin::Ask, true), None);
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
