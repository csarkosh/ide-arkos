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

/// How long `--check-handover` (and `--handover-formats`) may take before the candidate is
/// rejected.
const CHECK_TIMEOUT: Duration = Duration::from_secs(5);
/// How long each reader gets to park; one that has not by then fails the upgrade.
const PARK_TIMEOUT: Duration = Duration::from_millis(500);
/// How long the upgrade waits for creates that passed their check before `begin` to finish.
const CREATE_TIMEOUT: Duration = Duration::from_secs(2);

/// Whether the session table is complete: `Some(true)` once no create is in flight,
/// `Some(false)` when one is still spawning after `CREATE_TIMEOUT`, `None` to keep waiting.
fn creates_settled(creating: usize, waited: Duration) -> Option<bool> {
    if creating == 0 {
        Some(true)
    } else if waited >= CREATE_TIMEOUT {
        Some(false)
    } else {
        None
    }
}

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
    let check = run_flag(exe, "--check-handover")?;
    if check_answer_is_ours(&check) {
        return Ok(());
    }
    // `--check-handover` names only the oldest format the candidate reads, so `handover 1` may
    // be the current main, which reads 1 alone, or a binary that reads 1 and 2: ask it.
    let cannot_read = |why: String| {
        format!(
            "{} cannot read handover format {}, which this daemon writes: it answered {:?} to --check-handover and {why}",
            exe.display(),
            handover::FORMAT,
            check.trim()
        )
    };
    let formats = run_flag(exe, "--handover-formats").map_err(cannot_read)?;
    if !formats_cover(&formats) {
        return Err(cannot_read(format!("{:?} to --handover-formats", formats.trim())));
    }
    Ok(())
}

/// Runs `exe flag` with no stdin and no stderr, and returns its stdout if it exits 0 within
/// `CHECK_TIMEOUT`.
fn run_flag(exe: &Path, flag: &str) -> Result<String, String> {
    let spawned = {
        // A fork must not inherit a PTY descriptor that is briefly without close-on-exec.
        let _no_open_pty_inheritable = pty::spawn_lock();
        Command::new(exe).arg(flag).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
    };
    let mut child = spawned.map_err(|e| format!("cannot run {} {flag}: {e}", exe.display()))?;
    let deadline = Instant::now() + CHECK_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{} {flag} did not finish within {CHECK_TIMEOUT:?}", exe.display()));
            }
            Err(e) => return Err(format!("waiting for {} {flag}: {e}", exe.display())),
        }
    };
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        use std::io::Read;
        let _ = stdout.read_to_string(&mut out);
    }
    if !status.success() {
        return Err(format!("{} {flag} failed ({status})", exe.display()));
    }
    Ok(out)
}

/// Whether `--check-handover` answered `handover <FORMAT>`: a binary whose oldest readable
/// format is the one this daemon writes reads it.
pub fn check_answer_is_ours(answer: &str) -> bool {
    answer.trim() == format!("handover {}", handover::FORMAT)
}

/// Whether `--handover-formats` answered `<oldest> <newest>` with this daemon's format between
/// them, inclusive.
pub fn formats_cover(answer: &str) -> bool {
    let mut nums = answer.split_whitespace().map(str::parse::<u32>);
    match (nums.next(), nums.next(), nums.next()) {
        (Some(Ok(oldest)), Some(Ok(newest)), None) => oldest <= handover::FORMAT && handover::FORMAT <= newest,
        _ => false,
    }
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

/// Held by the request that claimed the slot, from `begin` until `validate` and `execute` are
/// done: if either unwinds, the slot is released, or creates and kills would be refused for the
/// daemon's life. It does nothing on a normal return, where `abort` or `restore` has released
/// the slot once already (releasing it again could clear a slot another client has claimed
/// since), and a successful exec never returns at all.
pub struct SlotGuard<'a>(pub &'a Shared);

impl Drop for SlotGuard<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            // A second panic here would abort the process: take the lock even if poisoned.
            self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).upgrading = false;
            eprintln!("sarkosd: the upgrade panicked; the old daemon keeps running");
        }
    }
}

/// The handover file's path: next to the socket, named by this pid.
pub fn handover_path(socket: &Path) -> PathBuf {
    socket.with_file_name(format!("handover-{}.bin", std::process::id()))
}

/// Parks every reader, writes the handover, hands the descriptors over and execs. Returns only
/// on failure, with everything restored and the error to report.
pub fn execute(state: &Shared, exe: &Path, opts: &Options) -> String {
    // A create that passed its check before `begin` inserts its session later; gathered without
    // it, its master would close at the exec and its shell get SIGHUP. No new create starts now,
    // and the table is gathered under the same lock that sees none in flight.
    let started = Instant::now();
    let (sessions, listener_fd, next_session, windows, settled) = loop {
        let st = state.lock().unwrap();
        if let Some(settled) = creates_settled(st.creating, started.elapsed()) {
            let windows: Vec<(PathBuf, Option<PathBuf>)> = st.windows.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            break (st.sessions.values().cloned().collect::<Vec<_>>(), st.listener_fd, st.next_session(), windows, settled);
        }
        drop(st);
        std::thread::sleep(Duration::from_millis(5));
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
    if !settled {
        return restore(format!("a session being created did not finish within {CREATE_TIMEOUT:?}"));
    }
    for s in &sessions {
        s.pause();
    }
    for s in &sessions {
        // A reader still reading after the snapshot would lose what it reads: never lose output.
        if !s.wait_parked(PARK_TIMEOUT) {
            return restore(format!("session {} did not park", s.id.0));
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
        flush_quiet_millis: opts.flush_quiet.as_millis() as u64,
        next_session,
        windows,
        sessions: records,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_answer_naming_our_format_is_accepted_on_its_own() {
        assert_eq!(handover::FORMAT, 2);
        assert!(check_answer_is_ours("handover 2\n"));
        assert!(!check_answer_is_ours("handover 1"), "may read only 1 (the current main): ask for its formats");
        assert!(!check_answer_is_ours("handover 3"), "a binary that dropped our format cannot resume our file");
        assert!(!check_answer_is_ours("handover"));
        assert!(!check_answer_is_ours(""));
    }

    #[test]
    fn a_candidate_is_accepted_when_its_formats_cover_ours() {
        assert_eq!(handover::FORMAT, 2);
        assert!(formats_cover("1 2"));
        assert!(formats_cover("1 3\n"));
        assert!(formats_cover("2 2"));
        assert!(!formats_cover("1 1"), "reads only format 1: the current main");
        assert!(!formats_cover("3 4"), "dropped our format");
        assert!(!formats_cover("1 2 3"));
        assert!(!formats_cover("garbage"));
        assert!(!formats_cover(""));
    }

    #[test]
    fn the_upgrade_waits_for_creates_in_flight_then_gives_up() {
        assert_eq!(creates_settled(0, Duration::ZERO), Some(true));
        assert_eq!(creates_settled(0, CREATE_TIMEOUT * 2), Some(true), "settled wins over late");
        assert_eq!(creates_settled(1, Duration::from_millis(100)), None, "keep waiting");
        assert_eq!(creates_settled(2, CREATE_TIMEOUT), Some(false));
        assert_eq!(CREATE_TIMEOUT, Duration::from_secs(2));
    }

    #[test]
    fn a_panic_while_the_slot_is_held_releases_it_and_a_return_does_not() {
        use crate::server::State;
        use std::sync::{Arc, Mutex};
        let state: Shared = Arc::new(Mutex::new(State::default()));
        begin(&state).unwrap();
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _slot = SlotGuard(&state);
            panic!("validate or execute unwinds");
        }));
        assert!(unwound.is_err());
        assert!(!state.lock().unwrap().upgrading, "creates and kills are served again");
        // A normal return leaves the slot to abort or restore, which release it exactly once:
        // after that it may already belong to another client's upgrade.
        begin(&state).unwrap();
        drop(SlotGuard(&state));
        assert!(state.lock().unwrap().upgrading);
    }
}
