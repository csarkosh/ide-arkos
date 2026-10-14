//! Replacing the daemon by exec-in-place (update-in-place spec, section 2). The process keeps
//! its pid, so the shells keep their parent; the masters and the listening socket pass through
//! `execve` as inherited descriptors; everything else is rebuilt from the handover file.
use crate::handover::{self, Handover};
use crate::pty;
use crate::server::{Options, Shared};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long `--check-handover` may take before the candidate is rejected.
const CHECK_TIMEOUT: Duration = Duration::from_secs(5);
/// How long each reader gets to park before the snapshot is taken anyway.
const PARK_TIMEOUT: Duration = Duration::from_millis(500);

/// The binary must be ours, the user's, runnable, and able to read this format.
pub fn validate(exe: &Path) -> Result<(), String> {
    let meta = std::fs::metadata(exe).map_err(|e| format!("cannot use {} as the new sarkosd: {e}", exe.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a regular file", exe.display()));
    }
    if exe.file_name().and_then(|n| n.to_str()) != Some("sarkosd") {
        return Err(format!("{} is not named sarkosd", exe.display()));
    }
    // SAFETY: getuid has no preconditions.
    if meta.uid() != unsafe { libc::getuid() } {
        return Err(format!("{} is not owned by this user", exe.display()));
    }
    if meta.permissions().mode() & 0o100 == 0 {
        return Err(format!("{} is not executable", exe.display()));
    }
    let spawned = {
        // A fork must not inherit a PTY descriptor that is briefly without close-on-exec.
        let _no_open_pty_inheritable = pty::spawn_lock();
        Command::new(exe).arg("--check-handover").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
    };
    let mut child = spawned.map_err(|e| format!("cannot run {} --check-handover: {e}", exe.display()))?;
    let deadline = Instant::now() + CHECK_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{} --check-handover did not finish within {CHECK_TIMEOUT:?}", exe.display()));
            }
            Err(e) => return Err(format!("waiting for {} --check-handover: {e}", exe.display())),
        }
    };
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        use std::io::Read;
        let _ = stdout.read_to_string(&mut out);
    }
    if !status.success() {
        return Err(format!("{} --check-handover failed ({status})", exe.display()));
    }
    let expected = format!("handover {}", handover::FORMAT);
    if out.trim() != expected {
        return Err(format!("{} --check-handover answered {:?}, this daemon writes {expected:?}", exe.display(), out.trim()));
    }
    Ok(())
}

/// Claims the one upgrade slot.
pub fn begin(state: &Shared) -> Result<(), String> {
    let mut st = state.lock().unwrap();
    if st.upgrading {
        return Err("an upgrade is already in progress".into());
    }
    st.upgrading = true;
    Ok(())
}

pub fn abort(state: &Shared) {
    state.lock().unwrap().upgrading = false;
}

/// The handover file's path: next to the socket, named by this pid.
pub fn handover_path(socket: &Path) -> PathBuf {
    socket.with_file_name(format!("handover-{}.bin", std::process::id()))
}

/// Parks every reader, writes the handover, hands the descriptors over and execs. Returns only
/// on failure, with everything restored and the error to report.
pub fn execute(state: &Shared, exe: &Path, opts: &Options) -> String {
    let (sessions, listener_fd, next_session) = {
        let st = state.lock().unwrap();
        (st.sessions.values().cloned().collect::<Vec<_>>(), st.listener_fd, st.next_session())
    };
    for s in &sessions {
        s.pause();
    }
    for s in &sessions {
        if !s.wait_parked(PARK_TIMEOUT) {
            eprintln!("sarkosd: session {} reader did not park within {PARK_TIMEOUT:?}; continuing", s.id.0);
        }
    }
    let records: Vec<_> = sessions.iter().map(|s| s.snapshot()).collect();
    let h = Handover {
        format: handover::FORMAT,
        written_by: crate::version_string(),
        listener_fd,
        replay_capacity: opts.replay_capacity,
        watchdog_secs: opts.watchdog.as_secs(),
        reaper_millis: opts.reaper_every.as_millis() as u64,
        dead_session_secs: opts.dead_session_ttl.as_secs(),
        client_pending_cap: opts.client_pending_cap,
        high_water: opts.backpressure.high_water,
        low_water: opts.backpressure.low_water,
        stall_secs: opts.backpressure.stall.as_secs(),
        next_session,
        sessions: records,
    };
    let path = handover_path(&opts.socket);
    // Everything but the descriptor flags, which are put back under the spawn lock.
    let restore = |why: String| -> String {
        for s in &sessions {
            s.unpause();
        }
        let _ = std::fs::remove_file(&path);
        abort(state);
        eprintln!("sarkosd: upgrade to {} failed: {why}; the old daemon keeps running", exe.display());
        why
    };
    if let Err(e) = handover::write(&path, &h) {
        return restore(format!("cannot write {}: {e}", path.display()));
    }
    let args = [
        pty::cstring(Path::new("sarkosd")),
        pty::cstring(Path::new("--socket")),
        pty::cstring(&opts.socket),
        pty::cstring(Path::new("--resume")),
        pty::cstring(&path),
    ];
    let args: Vec<_> = match args.into_iter().collect::<Result<Vec<_>, _>>() {
        Ok(a) => a,
        Err(e) => return restore(format!("bad argument: {e}")),
    };
    let exe_c = match pty::cstring(exe) {
        Ok(c) => c,
        Err(e) => return restore(format!("bad path: {e}")),
    };
    let mut argv: Vec<*const libc::c_char> = args.iter().map(|a| a.as_ptr()).collect();
    argv.push(std::ptr::null());
    // Held from the first inheritable descriptor through the exec, so no fork in this process
    // (a new shell, the check, the watchdog's relaunch) can inherit a master or the listener.
    let no_fork = pty::spawn_lock();
    let why = (|| -> String {
        for s in &sessions {
            if let Err(e) = s.master.set_inheritable(true) {
                return format!("cannot make session {}'s pty inheritable: {e}", s.id.0);
            }
        }
        if let Err(e) = pty::set_inheritable_fd(listener_fd, true) {
            return format!("cannot make the listener inheritable: {e}");
        }
        eprintln!("sarkosd: upgrading to {} with {} sessions, handover {}", exe.display(), h.sessions.len(), path.display());
        // SAFETY: argv is NULL-terminated and every pointer in it, and exe_c, outlives the call;
        // execv returns only on failure, in which case this process continues unchanged.
        unsafe { libc::execv(exe_c.as_ptr(), argv.as_ptr()) };
        format!("execv failed: {}", std::io::Error::last_os_error())
    })();
    for s in &sessions {
        let _ = s.master.set_inheritable(false);
    }
    let _ = pty::set_inheritable_fd(listener_fd, false);
    drop(no_fork);
    restore(why)
}
