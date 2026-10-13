//! Relaunches a UI whose main thread has stopped heartbeating (spec 2.4). Sessions are untouched.
use crate::server::Shared;
use std::os::unix::process::CommandExt;
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
            // A disconnected client's pid may have been reused by an unrelated process.
            if !rec.connected || now.duration_since(rec.last_heartbeat) < timeout {
                continue;
            }
            if !pid_alive(pid) {
                continue;
            }
            eprintln!("sarkosd: UI pid {pid} silent for {:?}, relaunching", timeout);
            to_restart.push((pid, rec.ui_exe.clone()));
            // Reset so one stuck UI is not killed again on the next scan while it restarts.
            rec.last_heartbeat = now;
        }
    }
    // The lock is released: killing and spawning never block other threads.
    for (pid, exe) in to_restart {
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
        }
        if let Some(exe) = exe {
            let mut cmd = Command::new(&exe);
            cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
            match cmd.spawn() {
                // Reap the child when it exits so it does not linger as a zombie.
                Ok(mut child) => {
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                }
                Err(e) => eprintln!("sarkosd: relaunch {} failed: {e}", exe.display()),
            }
        }
    }
}
