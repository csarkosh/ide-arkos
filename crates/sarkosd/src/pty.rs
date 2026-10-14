//! One pseudo-terminal master over libc, instead of portable-pty: the handover (spec section 2)
//! needs a master that can be inherited across `execve` and adopted from a raw descriptor, and a
//! shell held by pid, neither of which portable-pty's private types allow. macOS only.
use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

#[cfg(not(any(target_os = "macos")))]
compile_error!("sarkosd's PTY layer is written for macOS (XNU ioctl numbers)");

/// `libc` 0.2 may lack these on Apple targets; the values are XNU's.
const TIOCSCTTY: libc::c_ulong = 0x2000_7461;
const TIOCSWINSZ: libc::c_ulong = 0x8008_7467;

pub struct Pty {
    master: OwnedFd,
}

static SPAWN_LOCK: Mutex<()> = Mutex::new(());

/// Serializes forks against PTY descriptors that are briefly inheritable. `openpty` returns its
/// pair without close-on-exec, so a shell forked by another thread before the flag is set would
/// inherit a terminal it does not own and keep that session's reader from ever seeing EOF.
/// `Pty::open` and `spawn` take it; anything else in the daemon that starts a process, or clears
/// close-on-exec on a master, must hold it too.
pub fn spawn_lock() -> MutexGuard<'static, ()> {
    // The lock guards no data, so a panic while it was held leaves nothing inconsistent.
    SPAWN_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn winsize(rows: u16, cols: u16) -> libc::winsize {
    libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 }
}

fn set_cloexec(fd: RawFd, on: bool) -> io::Result<()> {
    // SAFETY: fcntl on a descriptor we own.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    let flags = if on { flags | libc::FD_CLOEXEC } else { flags & !libc::FD_CLOEXEC };
    // SAFETY: as above.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

impl Pty {
    /// A master and slave pair at the given size, both close-on-exec.
    pub fn open(rows: u16, cols: u16) -> io::Result<(Pty, OwnedFd)> {
        let mut master: libc::c_int = -1;
        let mut slave: libc::c_int = -1;
        let mut ws = winsize(rows, cols);
        let _no_fork_until_cloexec = spawn_lock();
        // SAFETY: openpty writes two descriptors; the name and termios pointers are allowed null.
        let rc = unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), &mut ws) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: both descriptors are fresh and owned by this process. Owned before the fcntl
        // calls, so a failing one closes them instead of leaking them.
        let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
        set_cloexec(master.as_raw_fd(), true)?;
        set_cloexec(slave.as_raw_fd(), true)?;
        Ok((Pty { master }, slave))
    }

    /// Takes ownership of a master inherited across `execve` (spec 2.2) and makes it
    /// close-on-exec again, so the next handover decides afresh what to pass on.
    ///
    /// # Safety
    ///
    /// `fd` must be an open descriptor that the caller owns and that nothing else will use or
    /// close: the returned `Pty` closes it on drop, and the caller must not use the number after
    /// this call.
    #[cfg_attr(not(test), allow(dead_code))] // the handover's resume (Task 4) calls it
    pub unsafe fn adopt(fd: RawFd) -> io::Result<Pty> {
        // SAFETY: the caller hands over ownership of an open descriptor (see `# Safety`). Owned
        // first, so a failing fcntl closes it rather than leaving it to nobody.
        let master = unsafe { OwnedFd::from_raw_fd(fd) };
        set_cloexec(master.as_raw_fd(), true)?;
        Ok(Pty { master })
    }

    pub fn raw_fd(&self) -> RawFd {
        self.master.as_raw_fd()
    }

    /// Cleared right before an `execve` so the new image inherits the master; set again after.
    #[cfg_attr(not(test), allow(dead_code))] // the handover's exec (Task 4) calls it
    pub fn set_inheritable(&self, on: bool) -> io::Result<()> {
        set_cloexec(self.raw_fd(), !on)
    }

    pub fn resize(&self, rows: u16, cols: u16) -> io::Result<()> {
        let ws = winsize(rows, cols);
        // SAFETY: TIOCSWINSZ reads a winsize.
        if unsafe { libc::ioctl(self.raw_fd(), TIOCSWINSZ, &ws) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Waits up to `timeout` for output. True on data or hangup, so the caller's `read` then
    /// returns bytes or EOF without blocking; false on timeout. A reader that polls this way can
    /// be parked between reads, which the handover needs.
    pub fn read_ready(&self, timeout: Duration) -> io::Result<bool> {
        let mut fds = libc::pollfd { fd: self.raw_fd(), events: libc::POLLIN, revents: 0 };
        let ms = timeout.as_millis().min(i32::MAX as u128) as libc::c_int;
        // SAFETY: one pollfd, nfds 1.
        let rc = unsafe { libc::poll(&mut fds, 1, ms) };
        if rc < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                return Ok(false);
            }
            return Err(e);
        }
        Ok(rc > 0)
    }

    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        // SAFETY: buf is valid for buf.len() bytes.
        let n = unsafe { libc::read(self.raw_fd(), buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n < 0 {
            let e = io::Error::last_os_error();
            // EIO is what a pty master returns once the slave side is gone: EOF.
            if e.raw_os_error() == Some(libc::EIO) {
                return Ok(0);
            }
            return Err(e);
        }
        Ok(n as usize)
    }

    pub fn write_all(&self, mut bytes: &[u8]) -> io::Result<()> {
        while !bytes.is_empty() {
            // SAFETY: bytes is valid for bytes.len() bytes.
            let n = unsafe { libc::write(self.raw_fd(), bytes.as_ptr() as *const libc::c_void, bytes.len()) };
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
            bytes = &bytes[n as usize..];
        }
        Ok(())
    }
}

/// Starts `program` with the slave as its terminal: a new session, the slave as its controlling
/// terminal and standard streams. The slave is closed in the parent. Returns the pid; the child
/// is reaped through `wait_nohang`, never through a `std::process::Child`, because the handover
/// execs this process and a `Child` does not survive that.
pub fn spawn(slave: OwnedFd, program: &str, args: &[&str], cwd: &Path, env: &[(String, String)]) -> io::Result<libc::pid_t> {
    let stdin = Stdio::from(slave.try_clone()?);
    let stdout = Stdio::from(slave.try_clone()?);
    let stderr = Stdio::from(slave);
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(cwd).stdin(stdin).stdout(stdout).stderr(stderr);
    for (k, v) in env {
        cmd.env(k, v);
    }
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::ioctl(0, TIOCSCTTY, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = {
        let _no_open_pty_inheritable = spawn_lock();
        cmd.spawn()?
    };
    let pid = child.id() as libc::pid_t;
    // The Child must not reap on drop; std's Child does not wait in Drop, so forgetting it only
    // releases its stdio handles (clones of the slave, which we want closed in the parent).
    std::mem::forget(child);
    Ok(pid)
}

/// `waitpid(pid, WNOHANG)`: `None` while running, `Some(code)` once reaped (a signal death is
/// `128 + signal`), `Some(-1)` when the pid is not this process's child.
pub fn wait_nohang(pid: libc::pid_t) -> Option<i32> {
    let mut status: libc::c_int = 0;
    // SAFETY: status is a valid out-pointer.
    let rc = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
    if rc == 0 {
        return None;
    }
    if rc < 0 {
        return Some(-1);
    }
    if libc::WIFEXITED(status) {
        Some(libc::WEXITSTATUS(status))
    } else {
        Some(128 + libc::WTERMSIG(status))
    }
}

/// The C string for `execv`.
#[allow(dead_code)] // the handover's exec (Task 4) calls it
pub fn cstring(path: &Path) -> io::Result<CString> {
    use std::os::unix::ffi::OsStrExt;
    CString::new(path.as_os_str().as_bytes()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn read_until(pty: &Pty, needle: &str, timeout: Duration) -> String {
        let deadline = Instant::now() + timeout;
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        while Instant::now() < deadline {
            if pty.read_ready(Duration::from_millis(50)).unwrap() {
                match pty.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => out.extend_from_slice(&buf[..n]),
                }
            }
            if String::from_utf8_lossy(&out).contains(needle) {
                break;
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// Waits for `pid` to exit, reading and discarding the master meanwhile: on XNU a session
    /// leader's exit waits (`ttywait`) until its terminal's output is drained, so a shell whose
    /// last bytes nobody reads never finishes exiting.
    fn wait_draining(pty: &Pty, pid: libc::pid_t) -> i32 {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut buf = [0u8; 4096];
        loop {
            if let Some(code) = wait_nohang(pid) {
                return code;
            }
            assert!(Instant::now() < deadline, "shell never exited");
            if pty.read_ready(Duration::from_millis(10)).unwrap() && matches!(pty.read(&mut buf), Ok(0) | Err(_)) {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    #[test]
    fn spawns_a_shell_on_a_pty_and_reaps_it() {
        let (pty, slave) = Pty::open(24, 80).unwrap();
        let pid = spawn(slave, "/bin/sh", &["-c", "echo hello-pty; exit 7"], std::env::temp_dir().as_path(), &[]).unwrap();
        assert!(pid > 1);
        let out = read_until(&pty, "hello-pty", Duration::from_secs(5));
        assert!(out.contains("hello-pty"), "{out:?}");
        assert_eq!(wait_draining(&pty, pid), 7);
        assert_eq!(wait_nohang(pid), Some(-1), "a reaped pid is nobody's child");
    }

    #[test]
    fn resize_reaches_the_shell_and_env_is_passed() {
        let (pty, slave) = Pty::open(24, 80).unwrap();
        pty.resize(30, 100).unwrap();
        let env = vec![("SARKOS_PTY_TEST".to_string(), "yes".to_string())];
        let pid = spawn(slave, "/bin/sh", &["-c", "stty size; echo env=$SARKOS_PTY_TEST"], std::env::temp_dir().as_path(), &env).unwrap();
        let out = read_until(&pty, "env=", Duration::from_secs(5));
        assert!(out.contains("30 100"), "{out:?}");
        assert!(out.contains("env=yes"), "{out:?}");
        wait_draining(&pty, pid);
    }

    #[test]
    fn adopt_takes_over_a_raw_fd_and_cloexec_is_managed() {
        let (pty, slave) = Pty::open(24, 80).unwrap();
        let pid = spawn(slave, "/bin/sh", &["-c", "sleep 0.2; echo after-adopt"], std::env::temp_dir().as_path(), &[]).unwrap();
        let raw = pty.raw_fd();
        pty.set_inheritable(true).unwrap();
        // SAFETY: fcntl F_GETFD on an open descriptor.
        let flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
        assert_eq!(flags & libc::FD_CLOEXEC, 0, "inheritable means no close-on-exec");
        std::mem::forget(pty); // the "old daemon" is gone; the fd number survives
        // SAFETY: the forgotten `pty` owned `raw` and is gone, so the fd is open and ours alone.
        let adopted = unsafe { Pty::adopt(raw) }.unwrap();
        // SAFETY: as above; `adopted` now owns it.
        let flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
        assert_ne!(flags & libc::FD_CLOEXEC, 0, "adopt sets close-on-exec again");
        let out = read_until(&adopted, "after-adopt", Duration::from_secs(5));
        assert!(out.contains("after-adopt"), "{out:?}");
        wait_draining(&adopted, pid);
    }

    /// How many descriptors a shell spawned on a fresh PTY has open, counted by `ls /dev/fd`.
    fn shell_fd_count() -> usize {
        let (pty, slave) = Pty::open(24, 80).unwrap();
        let script = "echo fds=$(ls /dev/fd | wc -l | tr -d ' ') end";
        let pid = spawn(slave, "/bin/sh", &["-c", script], std::env::temp_dir().as_path(), &[]).unwrap();
        let out = read_until(&pty, " end", Duration::from_secs(5));
        wait_draining(&pty, pid);
        let count = out.split("fds=").nth(1).and_then(|rest| rest.split(" end").next());
        count.and_then(|n| n.trim().parse().ok()).unwrap_or_else(|| panic!("no count in {out:?}"))
    }

    #[test]
    fn concurrent_spawns_give_each_shell_only_its_own_terminal() {
        let alone = shell_fd_count();
        let threads = 8;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(threads));
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    (0..10)
                        .map(|_| {
                            barrier.wait();
                            shell_fd_count()
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for handle in handles {
            for count in handle.join().unwrap() {
                assert_eq!(count, alone, "a shell inherited another PTY's descriptors");
            }
        }
    }

    #[test]
    fn write_reaches_the_shell() {
        let (pty, slave) = Pty::open(24, 80).unwrap();
        let pid = spawn(slave, "/bin/sh", &[], std::env::temp_dir().as_path(), &[]).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        pty.write_all(b"echo typed-$((40+2))\n").unwrap();
        let out = read_until(&pty, "typed-42", Duration::from_secs(5));
        assert!(out.contains("typed-42"), "{out:?}");
        pty.write_all(b"exit\n").unwrap();
        wait_draining(&pty, pid);
    }
}
