//! Automatic updates (auto-update spec, sections 5 to 7): the daemon polls a signed manifest,
//! downloads and verifies the release, stages it, and swaps the app bundle when a window asks.
pub mod apply;
pub mod fetch;
pub mod manifest;
pub mod poll;
pub mod stage;
pub mod swap;
pub mod verify;
pub mod version;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// The most a release zip may be, whatever the manifest says.
pub const MAX_ZIP_BYTES: u64 = 200 * 1024 * 1024;

pub const DEFAULT_UPDATE_URL: &str = "https://github.com/csarkosh/ide-arkos/releases/latest/download/";
pub const DEFAULT_CHECK_EVERY_SECS: u64 = 300;

/// The daemon's update arguments (`--update-url` and the rest, see `main.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateOptions {
    /// Ends with `/`; the manifest and its signature are fetched from here.
    pub url: String,
    /// Zero turns polling off.
    pub check_every: Duration,
    pub curl: PathBuf,
    pub ditto: PathBuf,
    /// The staging folder, `<socket dir>/updates` unless told otherwise.
    pub dir: PathBuf,
    /// Test only: keys trusted besides `RELEASE_PUBLIC_KEYS`.
    pub extra_keys: Vec<String>,
    /// The installed `Arkos IDE.app` that `ApplyUpdate` swaps; None when the daemon does not run
    /// from a bundle, which refuses `ApplyUpdate`.
    pub bundle: Option<PathBuf>,
    /// How long the new window has to connect after its relaunch (`apply::CONNECT_GRACE`; test
    /// only `--connect-grace-secs`).
    pub connect_grace: Duration,
    /// Test only (`--running-version`): the semver the updater compares releases against instead
    /// of this binary's own, so a test can restart the daemon as a newer version.
    pub running_version: Option<String>,
}

impl UpdateOptions {
    pub fn defaults(socket: &Path) -> Self {
        Self {
            url: DEFAULT_UPDATE_URL.into(),
            check_every: Duration::from_secs(DEFAULT_CHECK_EVERY_SECS),
            curl: PathBuf::from("/usr/bin/curl"),
            ditto: PathBuf::from("/usr/bin/ditto"),
            dir: socket.parent().unwrap_or(Path::new("/")).join("updates"),
            extra_keys: Vec::new(),
            bundle: std::env::current_exe().ok().and_then(|exe| bundle_of(&exe)),
            connect_grace: apply::CONNECT_GRACE,
            running_version: None,
        }
    }

    /// The version every comparison treats as running: this binary's, unless a test said otherwise.
    pub fn running(&self) -> version::Version {
        let ours = version::parse_version_string(&crate::version_string()).expect("our own version string parses");
        match self.running_version.as_deref().and_then(version::parse_semver) {
            Some((major, minor, patch)) => version::Version { major, minor, patch, git: ours.git },
            None => ours,
        }
    }

    /// The arguments that give a resumed daemon these options: only those that differ from the
    /// defaults, so a release's exec passes none and an older binary that knows none of them
    /// can still be resumed.
    pub fn args(&self, socket: &Path) -> Vec<String> {
        let d = Self::defaults(socket);
        let mut out = Vec::new();
        let path = |p: &Path| p.to_string_lossy().into_owned();
        if self.url != d.url {
            out.extend(["--update-url".into(), self.url.clone()]);
        }
        if self.check_every != d.check_every {
            out.extend(["--check-every-secs".into(), self.check_every.as_secs().to_string()]);
        }
        if self.curl != d.curl {
            out.extend(["--curl".into(), path(&self.curl)]);
        }
        if self.ditto != d.ditto {
            out.extend(["--ditto".into(), path(&self.ditto)]);
        }
        if self.dir != d.dir {
            out.extend(["--updates-dir".into(), path(&self.dir)]);
        }
        for k in &self.extra_keys {
            out.extend(["--release-key".into(), k.clone()]);
        }
        if let Some(b) = self.bundle.as_ref().filter(|b| d.bundle.as_ref() != Some(b)) {
            out.extend(["--bundle".into(), path(b)]);
        }
        if self.connect_grace != d.connect_grace {
            out.extend(["--connect-grace-secs".into(), self.connect_grace.as_secs().to_string()]);
        }
        if let Some(v) = &self.running_version {
            out.extend(["--running-version".into(), v.clone()]);
        }
        out
    }
}

/// The app bundle holding `exe` when it runs from one (`<bundle>.app/Contents/MacOS/<exe>`).
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let is_app = bundle.extension().is_some_and(|e| e == "app");
    (is_app && macos.file_name()? == "MacOS" && contents.file_name()? == "Contents").then(|| bundle.to_path_buf())
}

/// What the poll thread knows, kept in the daemon's shared state.
#[derive(Default)]
pub struct UpdateState {
    pub staged: Option<stage::Staged>,
    pub last_error: Option<String>,
    pub checking: bool,
    pub check_now: bool,
    /// Whether a poll thread runs (`--check-every-secs` is not zero); `CheckForUpdates` is
    /// refused when not.
    pub polling: bool,
    /// The update swapped in and not yet confirmed by a window or rolled back (the marker's
    /// contents).
    pub applied: Option<apply::Applied>,
    /// Whether a window that was not connected at the swap (its `ui_pid` is not in the marker's
    /// `pids_at_apply`) said Hello since: the new window, which confirms the update.
    pub new_window_seen: bool,
    /// Whether the grace runs for `applied`: from the new window's relaunch or this daemon's
    /// start (`apply::restart_clock`). Between the swap and either, a rollback waits.
    pub clock_running: bool,
    /// The version an `ApplyUpdate` is swapping in right now: a second request is refused until
    /// the swap finished or failed, so two cannot race the renames.
    pub applying: Option<String>,
    /// When a check last reached a manifest whose signature verified (whatever it found).
    pub last_ok_unix: Option<u64>,
    /// Whether the latest check failed in transport (`poll::Outcome::Transient`).
    pub last_check_transient: bool,
    /// The last version this process swapped in or found applied: the poll treats it like a
    /// staged one, so an applied or rolled-back release is not fetched and staged again.
    pub swapped: Option<String>,
    /// When the last check finished, whatever it found; None before the first.
    pub last_check_unix: Option<u64>,
    /// The last check's transport failure, None once a check reaches the manifest again.
    pub transient_error: Option<String>,
}

impl UpdateState {
    /// `Response::UpdateStatus`'s `unreachable_for_secs` at `now_unix`: None before the first
    /// verified manifest, the time since it when the latest check failed in transport, else 0.
    pub fn unreachable_for_secs(&self, now_unix: u64) -> Option<u64> {
        let ok = self.last_ok_unix?;
        Some(if self.last_check_transient { now_unix.saturating_sub(ok) } else { 0 })
    }
}

/// The version a release must be newer than: the newer of what runs and what the installed
/// bundle says it is (the app may have been updated by hand while an older daemon runs). A
/// missing or unreadable bundle version falls back to what runs.
pub fn floor(running: &version::Version, bundle: Option<&Path>) -> version::Version {
    match bundle.and_then(stage::installed_version) {
        Some(installed) if installed.is_newer_than(running) => installed,
        _ => running.clone(),
    }
}

/// `0.2.6`, for messages.
pub fn semver(v: &version::Version) -> String {
    format!("{}.{}.{}", v.major, v.minor, v.patch)
}

/// At every daemon start (the fresh one and the one an in-place exec resumes) and before every
/// check: a staged release that is not newer than `running` (the `floor`: the app may have been
/// updated by hand past it) is deleted from disk and forgotten, so no window is offered it and
/// none can apply a downgrade.
pub fn drop_stale_staged(state: &crate::server::Shared, dir: &Path, running: &version::Version) {
    let dropped = {
        let mut st = state.lock().unwrap();
        st.update.staged.take_if(|s| poll::is_stale(s, running))
    };
    if let Some(s) = dropped {
        crate::journal::note(format_args!("staged {} ({}) is not newer than running or installed {}: forgotten", s.version, s.git, semver(running)));
    }
    for v in stage::remove_not_newer(dir, running) {
        crate::journal::note(format_args!("staged {v} on disk is not newer than running or installed {}: deleted", semver(running)));
    }
}

/// Runs `cmd` with stdin null, a minimal environment and both outputs captured, killing it once
/// `timeout` passes. Every process the updater starts goes through here. The child leads its own
/// process group, so a grandchild that keeps a pipe open past the child's exit (a staged binary
/// that forks) is killed with it rather than holding this thread.
pub fn run_bounded(cmd: &mut Command, timeout: Duration) -> Result<Output, String> {
    use std::os::unix::process::CommandExt;
    use std::sync::mpsc;
    cmd.env_clear().env("PATH", "/usr/bin:/bin");
    if let Some(home) = std::env::var_os("HOME") {
        cmd.env("HOME", home);
    }
    let deadline = Instant::now() + timeout;
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0).spawn().map_err(|e| e.to_string())?;
    let group = child.id() as libc::pid_t;
    let timed_out = || {
        // SAFETY: killpg only signals; the group is the child's own, made at spawn.
        unsafe { libc::killpg(group, libc::SIGKILL) };
        format!("timed out after {} s", timeout.as_secs())
    };
    // Drained on their own threads, so a chatty child cannot fill a pipe and stall until killed.
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            let _ = tx.send(buf);
        });
        rx
    };
    let out = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let e = timed_out();
                let _ = child.wait();
                return Err(e);
            }
            Err(e) => {
                let _ = timed_out();
                let _ = child.wait();
                return Err(e.to_string());
            }
        }
    };
    // The child is reaped; its pipes close when every holder is gone, or the group is killed.
    let collect = |rx: mpsc::Receiver<Vec<u8>>| rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).map_err(|_| timed_out());
    let stdout = collect(out)?;
    let stderr = collect(err)?;
    Ok(Output { status, stdout, stderr })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resumed_daemon_is_given_only_the_update_arguments_that_differ_from_the_defaults() {
        let socket = Path::new("/s/arkosd.sock");
        let d = UpdateOptions::defaults(socket);
        assert_eq!(d.dir, Path::new("/s/updates"));
        assert!(d.args(socket).is_empty(), "a release's daemon passes none, so an older one can resume");
        let o = UpdateOptions { check_every: Duration::ZERO, extra_keys: vec!["k".into()], connect_grace: Duration::from_secs(2), ..d };
        assert_eq!(o.args(socket), ["--check-every-secs", "0", "--release-key", "k", "--connect-grace-secs", "2"]);
        let o = UpdateOptions { running_version: Some("9.9.9".into()), ..UpdateOptions::defaults(socket) };
        assert_eq!(o.args(socket), ["--running-version", "9.9.9"], "a test's running version survives the exec");
        assert_eq!((o.running().major, o.running().minor, o.running().patch), (9, 9, 9));
    }

    #[test]
    fn the_bundle_is_found_only_for_an_executable_inside_an_app() {
        assert_eq!(bundle_of(Path::new("/Applications/Arkos IDE.app/Contents/MacOS/arkosd")), Some(PathBuf::from("/Applications/Arkos IDE.app")));
        assert_eq!(bundle_of(Path::new("/x/target/debug/arkosd")), None);
        assert_eq!(bundle_of(Path::new("/x/Arkos IDE.app/Contents/Resources/arkosd")), None);
        let socket = Path::new("/s/arkosd.sock");
        let o = UpdateOptions { bundle: Some(PathBuf::from("/t/Arkos IDE.app")), ..UpdateOptions::defaults(socket) };
        assert_eq!(o.args(socket), ["--bundle", "/t/Arkos IDE.app"], "a non-default bundle passes through the exec");
    }

    #[test]
    fn a_bounded_process_gets_no_stdin_a_minimal_environment_and_a_timeout() {
        let out = run_bounded(Command::new("/bin/sh").args(["-c", "cat; echo \"$PATH\"; env | wc -l"]), Duration::from_secs(5)).unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.starts_with("/usr/bin:/bin\n"), "{text}");
        assert!(text.lines().nth(1).unwrap().trim().parse::<u32>().unwrap() <= 6, "{text}");
        let started = Instant::now();
        assert!(run_bounded(Command::new("/bin/sleep").arg("5"), Duration::from_millis(200)).unwrap_err().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn a_grandchild_holding_stdout_cannot_hold_the_caller_past_the_timeout() {
        let started = Instant::now();
        let r = run_bounded(Command::new("/bin/sh").args(["-c", "/bin/sleep 5 & echo hi"]), Duration::from_millis(500));
        assert!(r.unwrap_err().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    }
}
