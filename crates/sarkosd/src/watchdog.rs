//! Relaunches a UI whose main thread has stopped heartbeating (spec 2.4). Sessions are untouched.
use crate::server::Shared;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn start(state: Shared, timeout: Duration) {
    if timeout.is_zero() {
        return;
    }
    let interval = std::cmp::max(timeout / 6, Duration::from_millis(500));
    std::thread::Builder::new()
        .name("watchdog".into())
        .spawn(move || loop {
            std::thread::sleep(interval);
            scan(&state, timeout);
        })
        .expect("spawn watchdog");
}

fn pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

fn scan(state: &Shared, timeout: Duration) {
    let now = Instant::now();
    let mut to_restart = Vec::new();
    {
        let mut st = state.lock().unwrap();
        for rec in st.clients.values_mut() {
            let Some(pid) = rec.ui_pid else { continue };
            // Signalling pid 0 would hit the daemon's whole process group, and its own pid itself;
            // a pid above i32::MAX is negative as a pid_t and would signal a group or everything.
            // Hello already refuses these; this keeps the watchdog safe on its own.
            if pid < 2 || pid > i32::MAX as u32 || pid == std::process::id() {
                continue;
            }
            // A disconnected client's pid may have been reused by an unrelated process.
            if !rec.connected || now.duration_since(rec.last_heartbeat) < timeout {
                continue;
            }
            if !pid_alive(pid) {
                continue;
            }
            eprintln!("sarkosd: UI pid {pid} silent for {:?}, relaunching", timeout);
            to_restart.push((pid, rec.ui_exe.clone(), rec.ui_cwd.clone()));
            // Reset so one stuck UI is not killed again on the next scan while it restarts.
            rec.last_heartbeat = now;
        }
    }
    // The lock is released: killing and spawning never block other threads.
    for (pid, exe, cwd) in to_restart {
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
        }
        if let Some(exe) = exe {
            if let Err(e) = launch_ui(&exe, cwd.as_deref()) {
                eprintln!("sarkosd: relaunch {} failed: {e}", exe.display());
            }
        }
    }
}

/// Starts a window in its own session with no stdio, in `cwd` (the window picks its project
/// from its working directory; without one it would inherit the daemon's), and reaps it when it
/// exits. Used for the watchdog's relaunch and for `RelaunchClient`.
pub fn launch_ui(exe: &Path, cwd: Option<&Path>) -> std::io::Result<()> {
    let mut cmd = Command::new(exe);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    // No fork while a session's PTY descriptors are still inheritable, or the relaunched UI
    // would hold that terminal open for its whole life.
    let mut child = {
        let _no_open_pty_inheritable = crate::pty::spawn_lock();
        cmd.spawn()?
    };
    // Reap the child when it exits so it does not linger as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
