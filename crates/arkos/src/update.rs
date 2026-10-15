//! Noticing an installed update and running it (update-in-place spec, section 4). The window
//! compares its own version string with the one the binary at its launch path prints now:
//! after `brew upgrade` or a fresh build into place, that binary is the new one while this
//! process keeps running the old file.
use crate::Root;
use gpui::{App, Entity};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const CHECK_EVERY: Duration = Duration::from_secs(300);
const VERSION_TIMEOUT: Duration = Duration::from_secs(2);

/// Unresolved on macOS: the Homebrew symlink when launched through it, which is the path that
/// points at the new version after an upgrade (the resolved Cellar path is the old one).
pub fn launch_path() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

pub fn sibling_daemon(exe: &Path) -> PathBuf {
    exe.with_file_name("arkosd")
}

/// What `exe --version` prints, within two seconds; `None` when it cannot run or says nothing.
pub fn installed_version(exe: &Path) -> Option<String> {
    let mut child = Command::new(exe).arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let deadline = Instant::now() + VERSION_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return None,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    use std::io::Read;
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    let out = out.trim().to_string();
    (!out.is_empty()).then_some(out)
}

pub fn differs(running: &str, installed: &str) -> bool {
    running.trim() != installed.trim()
}

/// "update ready: 0.2.0 (def5678)": the installed string without the binary's name.
pub fn status_text(installed: &str) -> String {
    let version = installed.trim().split_once(' ').map(|(_, v)| v).unwrap_or(installed.trim());
    format!("update ready: {version}")
}

/// Checks now and every `CHECK_EVERY`, off the UI thread, and records the result on the root.
pub fn start(root: Entity<Root>, cx: &mut App) {
    let running = crate::version_string();
    cx.spawn(async move |cx| loop {
        let running = running.clone();
        let ready = cx
            .background_executor()
            .spawn(async move {
                let exe = launch_path()?;
                let installed = installed_version(&exe)?;
                differs(&running, &installed).then(|| status_text(&installed))
            })
            .await;
        if root.update(cx, |root, cx| {
            if root.update_ready != ready {
                root.update_ready = ready;
                cx.notify();
            }
        }).is_err() {
            return;
        }
        cx.background_executor().timer(CHECK_EVERY).await;
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_differ_by_text_and_the_status_names_the_installed_one() {
        assert!(!differs("arkos 0.1.0 (abc1234)", "arkos 0.1.0 (abc1234)\n"));
        assert!(differs("arkos 0.1.0 (abc1234)", "arkos 0.1.0 (def5678)"), "a rebuild is an update");
        assert!(differs("arkos 0.1.0 (abc1234)", "arkos 0.2.0 (abc1234)"));
        assert_eq!(status_text("arkos 0.2.0 (def5678)"), "update ready: 0.2.0 (def5678)");
        assert_eq!(status_text("garbage"), "update ready: garbage");
        assert_eq!(sibling_daemon(std::path::Path::new("/opt/homebrew/bin/arkos")), std::path::PathBuf::from("/opt/homebrew/bin/arkosd"));
    }

    #[test]
    fn installed_version_runs_the_binary_and_tolerates_a_missing_one() {
        let _fork = crate::client::fork_lock();
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(installed_version(&dir.path().join("nope")), None);
        use std::os::unix::fs::PermissionsExt;
        let exe = dir.path().join("arkos");
        std::fs::write(&exe, "#!/bin/sh\necho 'arkos 9.9.9 (test)'\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(installed_version(&exe).as_deref(), Some("arkos 9.9.9 (test)"));
        let slow = dir.path().join("arkos-slow");
        std::fs::write(&slow, "#!/bin/sh\nsleep 5\n").unwrap();
        std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
        let started = std::time::Instant::now();
        assert_eq!(installed_version(&slow), None);
        assert!(started.elapsed() < Duration::from_secs(4), "the timeout holds");
    }
}
