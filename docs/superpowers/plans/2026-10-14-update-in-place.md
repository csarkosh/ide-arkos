# Sarkos IDE: Updating in place without losing sessions. Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make updating Sarkos IDE, daemon included, never end a session: the daemon replaces itself by `execve` in the same process with its shells still its children, the window reconnects on its own, and one palette command runs the whole update after `brew upgrade` or a fresh build.

**Architecture:** The daemon drops portable-pty for a hundred-line libc PTY layer that can adopt an inherited descriptor, holds each shell by pid, and on `Upgrade` writes a handover file (session table and replay rings), clears close-on-exec on its PTY masters and listening socket, and execs the new binary, which rebuilds everything from the file (`--resume`). The window gets a connection loop that reconnects with backoff and reattaches every tab through the existing reconcile path, an update check against the binary at its launch path, and a palette command that upgrades the daemon first, then asks the daemon to relaunch the window. The protocol becomes version 2 with append-only rules and a daemon that still serves version 1 windows.

**Tech Stack:** Rust stable (edition 2021), gpui 0.2.2 (exact pin, `runtime_shaders`), libc 0.2 (already a dependency; `openpty`, `waitpid`, `poll`, `execv`, `fcntl`), postcard 1.1 for the handover file, serde. portable-pty 0.9 is removed. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-10-14-update-in-place-design.md`, sections 1 to 10 (section 11, recovery after a reboot, is milestone 3's). The main spec `docs/superpowers/specs/2026-10-13-sarkos-design.md` sections 2.1, 2.3, 2.4 and 9 still bind.

## Global Constraints

- macOS only, Apple Silicon. No Linux or Windows code paths. XNU semantics are the ones that matter: `execve` keeps the pid, parent, children, session, controlling terminal and every descriptor without `FD_CLOEXEC`; Rust's standard library sets `FD_CLOEXEC` on every descriptor it opens, so anything that must survive the exec has the flag cleared on purpose, right before the exec, and set again right after.
- `gpui = { version = "=0.2.2", features = ["runtime_shaders"] }` pinned exactly; never bump. If a gpui or libc name in this plan does not compile, the API differs: grep `~/.cargo/registry/src/*/gpui-0.2.2/src/` or `~/.cargo/registry/src/*/libc-0.2.190/src/unix/bsd/apple/mod.rs` for it, fix the call, and note the change in the commit message. Do not guess. macOS ioctl numbers if libc lacks the constant: `TIOCSCTTY = 0x2000_7461`, `TIOCSWINSZ = 0x8008_7467` (both `c_ulong`).
- Protocol (spec section 5): `PROTOCOL_VERSION` becomes 2. New enum variants are appended, never inserted; existing structs never change shape. The daemon serves protocol versions 1 and 2: a version 1 `Hello` is answered with `Response::Ok` exactly as today, a version 2 `Hello` with the new `Response::Hello`. A window newer than the daemon's maximum is refused with an error naming the fix.
- The daemon execs only a path given by a same-user client over its 0600 socket, and only after checking it is a regular file named `sarkosd`, owned by the current user, executable, and that `sarkosd --check-handover` run from that path exits 0. The handover file is created 0600 in the sarkos directory and deleted as soon as it is read.
- Never lose output across a swap: readers park before the snapshot and do not read again until the new image starts them; bytes stay in the kernel's PTY buffer (about 1 KiB on XNU before the writer blocks), so the swap must stay fast: target under one second for four sessions with full 16 MB rings.
- The daemon never parses escape sequences; the UI thread never blocks on IO; a UI client is never dropped by the daemon for slowness.
- Display name "Sarkos IDE" in user-facing text; binaries, crates, the socket directory `~/Library/Application Support/sarkos/` and log prefixes stay lowercase `sarkos`.
- Commit messages: imperative, one line, body optional. No em-dashes in any copy or comment. Every commit ends with a blank line, `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_015xrVD54zbvkgksJUndxfpr`.
- Tests: `cargo test -p sarkos` stays headless; daemon tests in `crates/sarkosd/tests/daemon.rs` run the real binary and must pass ten times in a row; the build stays warning-free for the project's crates; `cargo` is at `~/.cargo/bin/cargo`.

## Review Focus

1. **The new binary cannot start.** `Upgrade` pointing at a binary that fails `--check-handover`, or an `execve` that fails, must leave the old daemon running with every session intact and report the error to the window. Pinned by Task 4's `upgrade_with_a_binary_that_fails_the_check_keeps_the_old_daemon` and `upgrade_with_a_missing_binary_is_refused`.
2. **Output arrives during the swap.** A shell printing while the daemon execs must lose nothing: the bytes wait in the kernel and appear after resume. Pinned by Task 4's `output_printed_during_the_swap_is_not_lost`.
3. **A shell exits during the swap.** The exit must be reported after resume with its code, not hidden by a reader that was not running. Pinned by Task 4's `exit_during_the_swap_is_reported_after_resume`.
4. **The daemon is really gone.** When the window reconnects and nothing ever answers, it must not spin forever behind a silent screen: after ten seconds it says so and starts a fresh daemon, and tabs come back as new shells with the "(restarted)" note. Pinned by Task 5's `after_ten_seconds_the_window_starts_a_daemon` on the pure decision and by the README checklist.
5. **A second update mid-update.** `Upgrade` while a handover is in progress is refused, never run twice. Pinned by Task 4's `a_second_upgrade_during_the_check_is_refused`.

---

## File structure

```
crates/
  protocol/src/lib.rs           PROTOCOL_VERSION 2; Request::{Upgrade, RelaunchClient} and Response::Hello appended; PROTOCOL_MIN
  sarkosd/
    Cargo.toml                  portable-pty removed
    build.rs                    NEW: SARKOS_GIT_HASH from `git rev-parse --short HEAD`
    src/pty.rs                  NEW: Pty over libc: open, spawn (setsid, TIOCSCTTY), adopt(fd), resize, poll-read, inheritable
    src/session.rs              ChildSlot by pid (waitpid); Session::adopt; snapshot; pause/park for the handover
    src/handover.rs             NEW: Handover and SessionRecord, postcard file with magic and format version, write/read, mode 0600
    src/upgrade.rs              NEW: validate the binary, run --check-handover, park, write, clear CLOEXEC, execv; restore on failure
    src/server.rs               Hello v1/v2; Upgrade, RelaunchClient; run_resumed(listener, handover); relaunch on disconnect
    src/watchdog.rs             launch_ui(exe, cwd) factored out for the watchdog and RelaunchClient
    src/main.rs                 --version with hash, --check-handover, --resume <path>
    tests/daemon.rs             upgrade, resume, relaunch and version tests (append)
  sarkos/
    build.rs                    NEW: same hash
    src/connection.rs           NEW: the connection loop: connect, heartbeat, event loop, reconnect with backoff, start a daemon after 10 s
    src/update.rs               NEW: launch path, installed version, differs, the update check task, the update flow state
    src/sessions.rs             disconnect() and set_client(); Outcome::DaemonHello
    src/main.rs                 Root: daemon_version, update_ready, updating; Command::UpdateNow; status text; quit handlers through the current client
    src/keymap.rs               "update now" action for the palette (no key)
    src/command_palette.rs      the update item, shown when an update is ready
  README.md                     Updating section, the one-time pkill note, checklist
```

Responsibilities stay one per file. `pty.rs`, `handover.rs` and `update.rs` (its pure parts) have no socket, window or process state in their tested surface. `upgrade.rs` is the only file that calls `execv`.

---

### Task 1: A libc PTY layer and children held by pid

**Files:**
- Create: `crates/sarkosd/src/pty.rs`
- Modify: `crates/sarkosd/src/session.rs` (spawn, reader, `ChildSlot`, `kill`, `escalate_kill`, `resize`, `info`), `crates/sarkosd/src/main.rs` (`mod pty;`), `crates/sarkosd/Cargo.toml` (remove `portable-pty`), `Cargo.lock`
- Test: unit tests in `pty.rs` and `session.rs`; the existing `crates/sarkosd/tests/daemon.rs` suite must stay green

**Interfaces:**
- Consumes: nothing new; `libc`.
- Produces:
  - `pty::Pty { master: OwnedFd }` with `Pty::open(rows: u16, cols: u16) -> io::Result<(Pty, OwnedFd)>` (master, slave), `Pty::adopt(fd: RawFd) -> io::Result<Pty>` (takes ownership, sets `FD_CLOEXEC`), `Pty::raw_fd(&self) -> RawFd`, `Pty::resize(&self, rows, cols) -> io::Result<()>` (`TIOCSWINSZ`), `Pty::set_inheritable(&self, on: bool) -> io::Result<()>` (clears or sets `FD_CLOEXEC`), `Pty::read_ready(&self, timeout: Duration) -> io::Result<bool>` (`poll` for `POLLIN`; true also on `POLLHUP`), `Pty::read(&self, buf: &mut [u8]) -> io::Result<usize>`, `Pty::write_all(&self, bytes: &[u8]) -> io::Result<()>`.
  - `pty::spawn(slave: OwnedFd, program: &str, args: &[&str], cwd: &Path, env: &[(String, String)]) -> io::Result<libc::pid_t>`: forks through `std::process::Command` with the slave as stdin, stdout and stderr and a `pre_exec` that calls `setsid` then `ioctl(0, TIOCSCTTY, 0)`; returns the pid and drops the slave in the parent.
  - `pty::wait_nohang(pid) -> Option<i32>`: `waitpid(pid, WNOHANG)`; `Some(code)` once exited (signal death reported as `128 + signal`), `None` while running; `Some(-1)` if the pid is not our child (`ECHILD`).
  - `session::ChildSlot { pid: libc::pid_t, exited: Option<i32>, kill_requested_at }` replacing the boxed child; `Session::pid` stays `Option<u32>`.
  - `Session::spawn` unchanged in signature. The reader loop uses `read_ready` with a 50 ms timeout, so a later task can park it.

Why first: nothing in the handover can be built on a `Box<dyn Child>` or a master type that cannot adopt a descriptor.

- [ ] **Step 1: Write the failing tests**

Create `crates/sarkosd/src/pty.rs` with only:

```rust
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

    #[test]
    fn spawns_a_shell_on_a_pty_and_reaps_it() {
        let (pty, slave) = Pty::open(24, 80).unwrap();
        let pid = spawn(slave, "/bin/sh", &["-c", "echo hello-pty; exit 7"], std::env::temp_dir().as_path(), &[]).unwrap();
        assert!(pid > 1);
        let out = read_until(&pty, "hello-pty", Duration::from_secs(5));
        assert!(out.contains("hello-pty"), "{out:?}");
        let deadline = Instant::now() + Duration::from_secs(5);
        let code = loop {
            if let Some(code) = wait_nohang(pid) {
                break code;
            }
            assert!(Instant::now() < deadline, "shell never exited");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(code, 7);
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
        while wait_nohang(pid).is_none() {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn adopt_takes_over_a_raw_fd_and_cloexec_is_managed() {
        let (pty, slave) = Pty::open(24, 80).unwrap();
        let pid = spawn(slave, "/bin/sh", &["-c", "sleep 0.2; echo after-adopt"], std::env::temp_dir().as_path(), &[]).unwrap();
        let raw = pty.raw_fd();
        pty.set_inheritable(true).unwrap();
        let flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
        assert_eq!(flags & libc::FD_CLOEXEC, 0, "inheritable means no close-on-exec");
        std::mem::forget(pty); // the "old daemon" is gone; the fd number survives
        let adopted = Pty::adopt(raw).unwrap();
        let flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
        assert_ne!(flags & libc::FD_CLOEXEC, 0, "adopt sets close-on-exec again");
        let out = read_until(&adopted, "after-adopt", Duration::from_secs(5));
        assert!(out.contains("after-adopt"), "{out:?}");
        while wait_nohang(pid).is_none() {
            std::thread::sleep(Duration::from_millis(10));
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
        while wait_nohang(pid).is_none() {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
```

Add `mod pty;` to `crates/sarkosd/src/main.rs`.

- [ ] **Step 2: Run the tests to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkosd --lib -- pty 2>&1 | tail -20`
Expected: compile errors (`Pty`, `spawn`, `wait_nohang` not found).

- [ ] **Step 3: Write the PTY layer**

Above the tests in `crates/sarkosd/src/pty.rs`:

```rust
//! One pseudo-terminal master over libc, instead of portable-pty: the handover (spec section 2)
//! needs a master that can be inherited across `execve` and adopted from a raw descriptor, and a
//! shell held by pid, neither of which portable-pty's private types allow. macOS only.
use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

#[cfg(not(any(target_os = "macos")))]
compile_error!("sarkosd's PTY layer is written for macOS (XNU ioctl numbers)");

/// `libc` 0.2 may lack these on Apple targets; the values are XNU's.
const TIOCSCTTY: libc::c_ulong = 0x2000_7461;
const TIOCSWINSZ: libc::c_ulong = 0x8008_7467;

pub struct Pty {
    master: OwnedFd,
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
        // SAFETY: openpty writes two descriptors; the name and termios pointers are allowed null.
        let rc = unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), &mut ws) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        set_cloexec(master, true)?;
        set_cloexec(slave, true)?;
        // SAFETY: both descriptors are fresh and owned by this process.
        Ok((Pty { master: unsafe { OwnedFd::from_raw_fd(master) } }, unsafe { OwnedFd::from_raw_fd(slave) }))
    }

    /// Takes ownership of a master inherited across `execve` (spec 2.2) and makes it
    /// close-on-exec again, so the next handover decides afresh what to pass on.
    pub fn adopt(fd: RawFd) -> io::Result<Pty> {
        set_cloexec(fd, true)?;
        // SAFETY: the caller hands over ownership of an open descriptor.
        Ok(Pty { master: unsafe { OwnedFd::from_raw_fd(fd) } })
    }

    pub fn raw_fd(&self) -> RawFd {
        self.master.as_raw_fd()
    }

    /// Cleared right before an `execve` so the new image inherits the master; set again after.
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
    let child = cmd.spawn()?;
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
pub fn cstring(path: &Path) -> io::Result<CString> {
    use std::os::unix::ffi::OsStrExt;
    CString::new(path.as_os_str().as_bytes()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL"))
}
```

The `std::mem::forget(child)` deserves a check: `std::process::Child` does not `wait` in `Drop`, so forgetting it leaks only the small struct and its stdio handles, which are clones of the slave that the parent wants closed anyway; and the real child is reaped by `wait_nohang`. If a clippy lint complains, keep the forget and silence it with a comment, since dropping the `Child` would be equally correct here.

- [ ] **Step 4: Run the pty tests**

Run: `~/.cargo/bin/cargo test -p sarkosd --lib -- pty 2>&1 | tail -15`
Expected: four pass. If `stty size` prints `0 0`, the resize happened before the shell set up the terminal, which is fine: `openpty` already set the size, so move the `resize` call after the spawn and before `read_until`, and keep `30 100` as the expectation.

- [ ] **Step 5: Move `Session` onto the new layer**

In `crates/sarkosd/src/session.rs`:

1. Replace the portable-pty imports with `use crate::pty::{self, Pty};`.
2. `ChildSlot`:

```rust
/// The shell's pid and, once reaped, its exit code. A reaped pid may be reused by any other
/// process, so every signal and every wait goes through here and nothing touches the pid after
/// `exited` is set. Held by pid, not `std::process::Child`, so it survives the handover's exec.
struct ChildSlot {
    pid: libc::pid_t,
    exited: Option<i32>,
    /// When `kill` sent SIGHUP; the reaper escalates to SIGKILL if the shell ignores it.
    kill_requested_at: Option<Instant>,
}

impl ChildSlot {
    /// Non-blocking. Reaps the child if it has exited and remembers the code.
    fn try_reap(&mut self) -> Option<i32> {
        if self.exited.is_none() {
            match pty::wait_nohang(self.pid) {
                Some(-1) => self.exited = Some(-1), // not our child any more: treat as gone
                Some(code) => self.exited = Some(code),
                None => {}
            }
        }
        self.exited
    }
}
```

3. `Session`: `master: Pty` (no mutex needed: every method takes `&self` and the fd calls are atomic at the kernel), `rows: Mutex<(u16, u16)>` (the last size, for the handover), and `pid: libc::pid_t` kept as today's `Option<u32>` for `info`. The writer thread writes through `master.write_all` (the `Pty` is shared: wrap it in `Arc<Pty>`, held by the session and cloned into the reader and writer threads).
4. `spawn_inner`:

```rust
        let (master, slave) = Pty::open(spec.rows, spec.cols).context("openpty")?;
        let shell = spec.shell.clone().or_else(|| std::env::var("SHELL").ok()).unwrap_or_else(|| "/bin/zsh".to_string());
        let args: Vec<&str> = if shell.ends_with("zsh") || shell.ends_with("bash") { vec!["-l"] } else { vec![] };
        let mut env = vec![
            ("TERM".to_string(), "xterm-256color".to_string()),
            ("COLORTERM".to_string(), "truecolor".to_string()),
            ("SARKOS_SESSION_ID".to_string(), spec.id.0.to_string()),
        ];
        env.extend(spec.env.iter().cloned());
        let pid = pty::spawn(slave, &shell, &args, &spec.cwd, &env).context("spawn shell")?;
        if spec.nice != 0 {
            // Children of the shell inherit the priority (spec 2.5).
            unsafe { libc::setpriority(libc::PRIO_PROCESS, pid as libc::id_t, spec.nice) };
        }
        let master = Arc::new(master);
```

The writer thread takes `master.clone()` and an mpsc receiver as today; the reader thread takes `session.clone()` and reads through `session.master`.

5. The reader loop (`start_reader`), now pollable so Task 4 can park it:

```rust
    fn start_reader(session: Arc<Session>) {
        std::thread::Builder::new()
            .name(format!("pty-reader-{}", session.id.0))
            .spawn(move || {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match session.master.read_ready(Duration::from_millis(50)) {
                        Ok(false) => continue,
                        Ok(true) => {}
                        Err(_) => break,
                    }
                    match session.master.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            session.on_output(&buf[..n]);
                            session.yield_to_slow_subscribers();
                        }
                    }
                }
                let code = loop {
                    if let Some(code) = session.child.lock().unwrap().try_reap() {
                        break code;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                };
                session.mark_exited(Some(code));
            })
            .expect("spawn pty reader thread");
    }
```

6. `kill` and `escalate_kill` use `slot.pid` directly instead of `child.process_id()`; `resize` calls `self.master.resize(rows, cols)` and records the size; `spawn_for_test` keeps its drain thread, reading through the master the same way.
7. Remove `portable-pty = "0.9"` from `crates/sarkosd/Cargo.toml`.

- [ ] **Step 6: Run the whole daemon suite ten times**

Run: `~/.cargo/bin/cargo test -p sarkosd 2>&1 | grep "test result"` and then `for i in $(seq 10); do ~/.cargo/bin/cargo test -p sarkosd --test daemon 2>&1 | grep -q "test result: ok" || echo "run $i failed"; done`
Expected: unit and integration suites green every time (the integration suite exercises exits, kills, reaping under a held PTY, backpressure and the watchdog, all of which now run on the new layer). `~/.cargo/bin/cargo build -p sarkosd 2>&1 | grep -c "^warning"` prints 0 (cargo's note about other crates' future incompatibilities is not a project warning).

- [ ] **Step 7: Commit**

```bash
git add crates/sarkosd/src/pty.rs crates/sarkosd/src/session.rs crates/sarkosd/src/main.rs crates/sarkosd/Cargo.toml Cargo.lock
git commit -m "Hold shells by pid on a libc PTY layer that can adopt a descriptor"
```

---

### Task 2: The handover file and session snapshot and adopt

**Files:**
- Create: `crates/sarkosd/src/handover.rs`
- Modify: `crates/sarkosd/src/session.rs` (`snapshot`, `adopt`, size tracking), `crates/sarkosd/src/ring.rs` (`from_bytes`), `crates/sarkosd/src/main.rs` (`mod handover;`)
- Test: unit tests in `handover.rs`, `ring.rs`, `session.rs`

**Interfaces:**
- Consumes: `pty::Pty::{adopt, raw_fd}`, `Session` internals, `RingBuffer`.
- Produces:
  - `handover::FORMAT: u32 = 1`, `handover::MAGIC: [u8; 4] = *b"SKHO"`.
  - `handover::Handover { format: u32, written_by: String, listener_fd: i32, replay_capacity: usize, watchdog_secs: u64, reaper_millis: u64, dead_session_secs: u64, client_pending_cap: usize, high_water: usize, low_water: usize, stall_secs: u64, next_session: u64, sessions: Vec<SessionRecord> }` (serde, postcard).
  - `handover::SessionRecord { id: u64, cwd: PathBuf, pid: i32, alive: bool, exit_code: Option<i32>, exited_ago_ms: Option<u64>, claude: ClaudeState, last_activity_unix_ms: u64, master_fd: i32, rows: u16, cols: u16, kill_requested: bool, replay: Vec<u8> }`.
  - `handover::write(path: &Path, h: &Handover) -> io::Result<()>`: magic, then the postcard bytes; written to a sibling temp file with mode 0600, `sync_all`, renamed into place.
  - `handover::read(path: &Path) -> Result<Handover, String>`: refuses a bad magic, a `format` other than `FORMAT`, or undecodable bytes, each with a message naming the problem.
  - `RingBuffer::from_bytes(cap: usize, bytes: &[u8]) -> RingBuffer` (keeps the tail if `bytes` exceeds `cap`).
  - `Session::snapshot(&self) -> SessionRecord` (the master fd number as it is now; the caller makes it inheritable).
  - `Session::adopt(record: &SessionRecord, master: Pty, replay_capacity: usize, backpressure: Backpressure) -> Arc<Session>`: rebuilds state from the record (ring from `replay` at `replay_capacity`; `exited_at = now - exited_ago_ms`), reaps once with `try_reap` if alive, and starts the reader and writer threads.
  - `Session::size(&self) -> (u16, u16)` (last `rows, cols` set).

A ruling the spec needs: section 6 says the file "records both" versions "so a resume from a mismatched build is refused". The file is written by the old binary and read by the new one, so their versions differ by design; what gates a resume is `FORMAT`. `written_by` is kept for the log line only.

- [ ] **Step 1: Write the failing tests**

Create `crates/sarkosd/src/handover.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use protocol::ClaudeState;

    fn sample() -> Handover {
        Handover {
            format: FORMAT,
            written_by: "sarkosd 0.1.0 (abc1234)".into(),
            listener_fd: 5,
            replay_capacity: 1024,
            watchdog_secs: 30,
            reaper_millis: 1000,
            dead_session_secs: 600,
            client_pending_cap: 16 << 20,
            high_water: 4 << 20,
            low_water: 1 << 20,
            stall_secs: 10,
            next_session: 99,
            sessions: vec![SessionRecord {
                id: 7,
                cwd: "/tmp".into(),
                pid: 4242,
                alive: true,
                exit_code: None,
                exited_ago_ms: None,
                claude: ClaudeState::Working,
                last_activity_unix_ms: 123,
                master_fd: 9,
                rows: 40,
                cols: 120,
                kill_requested: false,
                replay: b"hello\r\n".to_vec(),
            }],
        }
    }

    #[test]
    fn round_trips_through_a_file_with_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("handover.bin");
        write(&file, &sample()).unwrap();
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(read(&file).unwrap(), sample());
        let names: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 1, "no temp file left behind: {names:?}");
    }

    #[test]
    fn refuses_a_wrong_magic_format_or_truncated_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("h.bin");
        std::fs::write(&file, b"NOPE....").unwrap();
        assert!(read(&file).unwrap_err().contains("magic"));
        let mut other = sample();
        other.format = FORMAT + 1;
        write(&file, &other).unwrap();
        assert!(read(&file).unwrap_err().contains("format"));
        write(&file, &sample()).unwrap();
        let bytes = std::fs::read(&file).unwrap();
        std::fs::write(&file, &bytes[..bytes.len() / 2]).unwrap();
        assert!(read(&file).is_err());
        assert!(read(&dir.path().join("missing.bin")).unwrap_err().contains("missing.bin"));
    }
}
```

Append to the `tests` module in `crates/sarkosd/src/ring.rs`:

```rust
    #[test]
    fn from_bytes_restores_a_snapshot_and_keeps_the_tail_when_too_long() {
        let r = RingBuffer::from_bytes(8, b"abcde");
        assert_eq!(r.snapshot(), b"abcde");
        assert_eq!(r.len(), 5);
        let r = RingBuffer::from_bytes(3, b"0123456789");
        assert_eq!(r.snapshot(), b"789");
        let mut r = RingBuffer::from_bytes(4, b"ab");
        r.push(b"cde");
        assert_eq!(r.snapshot(), b"bcde");
    }
```

Append to the `tests` module in `crates/sarkosd/src/session.rs`:

```rust
    #[test]
    fn snapshot_records_the_session_and_adopt_rebuilds_it() {
        let spec = SpawnSpec {
            id: SessionId(11),
            cwd: std::env::temp_dir(),
            shell: Some("/bin/sh".into()),
            env: vec![],
            nice: 0,
            rows: 24,
            cols: 80,
            replay_capacity: 4096,
            backpressure: Backpressure::default(),
        };
        let s = Session::spawn(spec).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        s.write(b"echo snap-$((1+1))\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !String::from_utf8_lossy(&s.state.lock().unwrap().replay.snapshot()).contains("snap-2") {
            assert!(Instant::now() < deadline, "no output");
            std::thread::sleep(Duration::from_millis(20));
        }
        s.resize(30, 100).unwrap();
        let rec = s.snapshot();
        assert_eq!(rec.id, 11);
        assert!(rec.alive);
        assert_eq!((rec.rows, rec.cols), (30, 100));
        assert_eq!(rec.pid, s.info().pid.unwrap() as i32);
        assert!(String::from_utf8_lossy(&rec.replay).contains("snap-2"));
        assert_eq!(rec.master_fd, s.master.raw_fd());
        // Hand the master over as the exec would: make it inheritable, forget the old session's
        // handle (its threads end when the fd they read returns an error), adopt the number.
        s.master.set_inheritable(true).unwrap();
        let raw = rec.master_fd;
        let dup = unsafe { libc::dup(raw) };
        assert!(dup >= 0);
        let adopted = Session::adopt(&rec, Pty::adopt(dup).unwrap(), 4096, Backpressure::default());
        assert!(adopted.info().alive);
        assert_eq!(adopted.info().pid, s.info().pid);
        assert!(String::from_utf8_lossy(&adopted.state.lock().unwrap().replay.snapshot()).contains("snap-2"));
        std::thread::sleep(Duration::from_millis(300));
        adopted.write(b"echo after-adopt-$((2+2))\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !String::from_utf8_lossy(&adopted.state.lock().unwrap().replay.snapshot()).contains("after-adopt-4") {
            assert!(Instant::now() < deadline, "the adopted session does not read");
            std::thread::sleep(Duration::from_millis(20));
        }
        adopted.kill().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while adopted.info().alive {
            adopted.poll_exit();
            assert!(Instant::now() < deadline, "the adopted session never saw the exit");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
```

(In this in-process test the old session's reader thread still reads the same terminal through the original fd, so output may be split between the two; the asserts only need each side to see its own command's echo, which the shell prints after the write arrives. Both sessions share one pid, and `waitpid` succeeds for whichever reaps first; the test only requires the adopted one to notice the exit, so it calls `adopted.poll_exit()` in its loop. Add `mod handover;` to `crates/sarkosd/src/main.rs`.)

- [ ] **Step 2: Run the tests to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkosd --lib -- handover ring snapshot 2>&1 | tail -20`
Expected: compile errors.

- [ ] **Step 3: Write the handover module and the ring constructor**

Above the tests in `handover.rs`:

```rust
//! The handover file (spec section 2): everything the new daemon image needs to rebuild the
//! session table after `execve`, written by the old image just before. Descriptor numbers are
//! carried as numbers because the descriptors themselves pass through the exec. The format
//! version gates a resume; `written_by` is for the log.
use protocol::ClaudeState;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

pub const FORMAT: u32 = 1;
pub const MAGIC: [u8; 4] = *b"SKHO";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: u64,
    pub cwd: PathBuf,
    pub pid: i32,
    pub alive: bool,
    pub exit_code: Option<i32>,
    /// How long ago the shell exited, so the reaper's grace carries over.
    pub exited_ago_ms: Option<u64>,
    pub claude: ClaudeState,
    pub last_activity_unix_ms: u64,
    pub master_fd: i32,
    pub rows: u16,
    pub cols: u16,
    /// A SIGHUP was sent and not yet answered: the new image escalates as the old one would have.
    pub kill_requested: bool,
    pub replay: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Handover {
    pub format: u32,
    pub written_by: String,
    pub listener_fd: i32,
    pub replay_capacity: usize,
    pub watchdog_secs: u64,
    pub reaper_millis: u64,
    pub dead_session_secs: u64,
    pub client_pending_cap: usize,
    pub high_water: usize,
    pub low_water: usize,
    pub stall_secs: u64,
    /// The id counter, so ids stay unique across the swap (spec 2.1 of the main spec).
    pub next_session: u64,
    pub sessions: Vec<SessionRecord>,
}

/// Magic, then postcard. A sibling temp file is written with mode 0600, synced and renamed over
/// `path`, so a crash mid-write leaves nothing a resume could misread.
pub fn write(path: &Path, h: &Handover) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| io::Error::other("handover path has no parent"))?;
    let name = path.file_name().ok_or_else(|| io::Error::other("handover path has no file name"))?;
    let tmp = parent.join(format!(".{}.{}.tmp", name.to_string_lossy(), std::process::id()));
    let body = postcard::to_allocvec(h).map_err(io::Error::other)?;
    let written = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(&MAGIC)?;
        f.write_all(&body)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

pub fn read(path: &Path) -> Result<Handover, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read handover {}: {e}", path.display()))?;
    if bytes.len() < MAGIC.len() || bytes[..MAGIC.len()] != MAGIC {
        return Err(format!("{} is not a handover file (bad magic)", path.display()));
    }
    let h: Handover = postcard::from_bytes(&bytes[MAGIC.len()..]).map_err(|e| format!("cannot decode handover {}: {e}", path.display()))?;
    if h.format != FORMAT {
        return Err(format!("handover format {} is not the {FORMAT} this sarkosd reads", h.format));
    }
    Ok(h)
}
```

In `ring.rs`:

```rust
    /// A ring holding `bytes` (the tail of them when longer than `cap`): the replay carried
    /// across a handover.
    pub fn from_bytes(cap: usize, bytes: &[u8]) -> Self {
        let mut r = Self::new(cap);
        r.push(bytes);
        r
    }
```

- [ ] **Step 4: Snapshot and adopt on `Session`**

In `session.rs`:

1. Add `size: Mutex<(u16, u16)>` to `Session`, set from the spec in `spawn_inner` and updated in `resize`; `pub fn size(&self) -> (u16, u16)`.
2. Add:

```rust
    /// The record the handover file carries for this session. The master fd number is the one
    /// in this process; the caller makes the descriptor inheritable before the exec.
    pub fn snapshot(&self) -> crate::handover::SessionRecord {
        let st = self.state.lock().unwrap();
        let slot = self.child.lock().unwrap();
        let (rows, cols) = *self.size.lock().unwrap();
        crate::handover::SessionRecord {
            id: self.id.0,
            cwd: self.cwd.clone(),
            pid: slot.pid,
            alive: st.alive,
            exit_code: st.exit_code,
            exited_ago_ms: st.exited_at.map(|at| at.elapsed().as_millis() as u64),
            claude: st.claude,
            last_activity_unix_ms: st.last_activity_unix_ms,
            master_fd: self.master.raw_fd(),
            rows,
            cols,
            kill_requested: slot.kill_requested_at.is_some(),
            replay: st.replay.snapshot(),
        }
    }

    /// Rebuilds a session around an inherited master after `--resume`. The shell is still this
    /// process's child (the pid never changed), so `try_reap` works; a shell that exited during
    /// the swap is caught here.
    pub fn adopt(record: &crate::handover::SessionRecord, master: Pty, replay_capacity: usize, backpressure: Backpressure) -> Arc<Session> {
        let master = Arc::new(master);
        let (input_tx, input_rx) = channel::<Vec<u8>>();
        Self::start_writer(SessionId(record.id), master.clone(), input_rx);
        let session = Arc::new(Session {
            id: SessionId(record.id),
            cwd: record.cwd.clone(),
            master,
            input: Mutex::new(input_tx),
            child: Mutex::new(ChildSlot {
                pid: record.pid,
                exited: if record.alive { None } else { Some(record.exit_code.unwrap_or(-1)) },
                kill_requested_at: record.kill_requested.then(Instant::now),
            }),
            pid: Some(record.pid as u32),
            size: Mutex::new((record.rows, record.cols)),
            state: Mutex::new(SessionState {
                replay: RingBuffer::from_bytes(replay_capacity, &record.replay),
                subscribers: HashMap::new(),
                alive: record.alive,
                exit_code: record.exit_code,
                exited_at: record.exited_ago_ms.map(|ms| Instant::now() - Duration::from_millis(ms)),
                last_activity_unix_ms: record.last_activity_unix_ms,
                claude: record.claude,
            }),
            backpressure,
            ending: AtomicBool::new(!record.alive),
        });
        if record.alive {
            // The reader reports EOF later; a shell gone during the swap is recorded now.
            session.poll_exit();
            Self::start_reader(session.clone());
        }
        session
    }
```

(`Instant::now() - Duration` panics if the duration is larger than the instant's age since boot; clamp with `checked_sub(..).unwrap_or_else(Instant::now)`.)

- [ ] **Step 5: Run the tests**

Run: `~/.cargo/bin/cargo test -p sarkosd --lib 2>&1 | tail -15`
Expected: all unit tests pass, including the three new ones. `~/.cargo/bin/cargo build -p sarkosd 2>&1 | grep -c "^warning"` prints 0; items unused until Task 4 (`handover::write`, `Session::adopt`, `snapshot`) take `#[cfg_attr(not(test), allow(dead_code))]` with a comment naming Task 4, removed there.

- [ ] **Step 6: Commit**

```bash
git add crates/sarkosd/src/handover.rs crates/sarkosd/src/ring.rs crates/sarkosd/src/session.rs crates/sarkosd/src/main.rs
git commit -m "Add the handover file format and session snapshot and adopt"
```

---

### Task 3: Protocol version 2, the daemon's Hello answer, and version strings

**Files:**
- Modify: `crates/protocol/src/lib.rs`, `crates/protocol/src/codec.rs` (tests), `crates/sarkosd/src/server.rs` (Hello), `crates/sarkosd/src/main.rs` (`--version`, `--check-handover`), `crates/sarkos/src/main.rs` (`--version`), `crates/sarkos/src/sessions.rs` (`Outcome::DaemonHello`), `crates/sarkos/src/client.rs` (Hello sends version 2), `crates/sarkosd/tests/daemon.rs` (harness and tests)
- Create: `crates/sarkosd/build.rs`, `crates/sarkos/build.rs`
- Test: codec round trips, daemon tests, a sessions test

**Interfaces:**
- Produces:
  - `protocol::PROTOCOL_VERSION: u32 = 2`, `protocol::PROTOCOL_MIN: u32 = 1`.
  - `Request::Upgrade { exe: PathBuf }` and `Request::RelaunchClient { exe: PathBuf, cwd: PathBuf }` appended after `SetState`; `Response::Hello { daemon_version: String, protocol_min: u32, protocol_max: u32 }` appended after `Error`. Nothing else in the enums moves.
  - Daemon `Hello`: `protocol_version` outside `PROTOCOL_MIN..=PROTOCOL_VERSION` is refused with `Error("protocol version mismatch: daemon speaks {min} to {max}, client {v}; update the daemon: run Update Sarkos IDE now")`; version 1 is answered `Ok` (as today); version 2 or more is answered `Response::Hello`. `ClientRecord.protocol: u32`.
  - `sarkosd --version` prints `sarkosd <crate version> (<short git hash>)`; `sarkos --version` prints `sarkos <crate version> (<short git hash>)`; the hash comes from `build.rs` as `SARKOS_GIT_HASH` (`unknown` when git is unavailable). A `version_string()` helper in each binary's `main.rs`.
  - `sarkosd --check-handover` prints `handover <FORMAT>` and exits 0.
  - `sessions::Outcome::DaemonHello { version: String, protocol_min: u32, protocol_max: u32 }` from `handle_response` for a `Response::Hello` on an id nobody waits on; `Root.daemon_version: Option<String>` set from it. `Response::Ok` on that id stays `Outcome::None` (an old daemon).
  - The daemon test harness's `connect_as` accepts `Response::Hello { .. }` as well as `Ok`.

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `crates/protocol/src/codec.rs` (read the file for the existing round-trip helper's name and reuse it; if there is none, write frames with `write_frame` into a `Vec<u8>` and read them back with `read_frame` over a `Cursor`):

```rust
    #[test]
    fn version_2_messages_round_trip_and_the_old_variants_keep_their_indices() {
        use crate::{Request, Response};
        use std::path::PathBuf;
        for req in [
            Request::Upgrade { exe: PathBuf::from("/opt/homebrew/bin/sarkosd") },
            Request::RelaunchClient { exe: PathBuf::from("/opt/homebrew/bin/sarkos"), cwd: PathBuf::from("/p") },
        ] {
            let bytes = postcard::to_allocvec(&req).unwrap();
            assert_eq!(postcard::from_bytes::<Request>(&bytes).unwrap(), req);
        }
        let hello = Response::Hello { daemon_version: "sarkosd 0.2.0 (abc1234)".into(), protocol_min: 1, protocol_max: 2 };
        let bytes = postcard::to_allocvec(&hello).unwrap();
        assert_eq!(postcard::from_bytes::<Response>(&bytes).unwrap(), hello);
        // postcard writes an enum's variant index first; Ok is still variant 0 and Error still 4,
        // so a version 1 window decodes every reply it can receive from this daemon.
        assert_eq!(postcard::to_allocvec(&Response::Ok).unwrap(), vec![0]);
        assert_eq!(postcard::to_allocvec(&Response::Error(String::new())).unwrap(), vec![4, 0]);
        assert_eq!(crate::PROTOCOL_VERSION, 2);
        assert_eq!(crate::PROTOCOL_MIN, 1);
    }
```

In `crates/sarkosd/tests/daemon.rs`, change `connect_as` to accept both answers:

```rust
        let r = c.request(Request::Hello { protocol_version: PROTOCOL_VERSION, ui_pid, ui_exe: None, ui_cwd: std::env::temp_dir() });
        assert!(matches!(r, Response::Ok | Response::Hello { .. }), "{r:?}");
```

and append:

```rust
#[test]
fn hello_version_1_gets_ok_and_version_2_gets_the_daemon_version() {
    let d = Daemon::start(&[]);
    let mut c = d.connect_raw();
    let r = c.request(Request::Hello { protocol_version: 1, ui_pid: std::process::id(), ui_exe: None, ui_cwd: std::env::temp_dir() });
    assert_eq!(r, Response::Ok, "a milestone 2b window keeps working");
    let mut c2 = d.connect_raw();
    match c2.request(Request::Hello { protocol_version: 2, ui_pid: std::process::id(), ui_exe: None, ui_cwd: std::env::temp_dir() }) {
        Response::Hello { daemon_version, protocol_min, protocol_max } => {
            assert!(daemon_version.starts_with("sarkosd "), "{daemon_version}");
            assert_eq!((protocol_min, protocol_max), (1, 2));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn hello_with_a_newer_protocol_is_refused_with_the_update_hint() {
    let d = Daemon::start(&[]);
    let mut c = d.connect_raw();
    match c.request(Request::Hello { protocol_version: 3, ui_pid: std::process::id(), ui_exe: None, ui_cwd: std::env::temp_dir() }) {
        Response::Error(e) => assert!(e.contains("Update Sarkos IDE now"), "{e}"),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn version_and_check_handover_flags() {
    let out = Command::new(env!("CARGO_BIN_EXE_sarkosd")).arg("--version").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("sarkosd ") && text.contains('(') && text.trim_end().ends_with(')'), "{text:?}");
    let out = Command::new(env!("CARGO_BIN_EXE_sarkosd")).arg("--check-handover").output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "handover 1");
}
```

Update the existing `hello_with_another_protocol_version_is_refused` test: it sends some other version; make it send `0` (below the minimum) and assert the error mentions `0`.

Append to the `tests` module in `crates/sarkos/src/sessions.rs`:

```rust
    #[test]
    fn a_hello_answer_reports_the_daemon_version() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r.clone(), None, PathBuf::from("/p"));
        let hello = Response::Hello { daemon_version: "sarkosd 0.2.0 (abc1234)".into(), protocol_min: 1, protocol_max: 2 };
        assert_eq!(
            m.handle_response(1, hello),
            Outcome::DaemonHello { version: "sarkosd 0.2.0 (abc1234)".into(), protocol_min: 1, protocol_max: 2 }
        );
        assert_eq!(m.handle_response(1, Response::Ok), Outcome::None, "an old daemon answers Ok");
    }
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `~/.cargo/bin/cargo test -p protocol 2>&1 | tail -5; ~/.cargo/bin/cargo test -p sarkos -- hello_answer 2>&1 | tail -5`
Expected: compile errors (`Upgrade`, `Hello`, `PROTOCOL_MIN`, `DaemonHello`).

- [ ] **Step 3: The protocol**

In `crates/protocol/src/lib.rs`:

```rust
/// Bumped only for a breaking change. New enum variants are appended, never inserted, and
/// existing structs never change shape, so a daemon can keep serving older windows: it answers
/// a `Hello` with the range it speaks (spec update-in-place section 5).
pub const PROTOCOL_VERSION: u32 = 2;
/// The oldest window a daemon still serves. Version 1 is milestone 2b's.
pub const PROTOCOL_MIN: u32 = 1;
```

Append to `Request` after `SetState`:

```rust
    /// Replace the daemon with the binary at `exe` by exec-in-place (update-in-place spec,
    /// section 2). Answered `Ok` just before the exec, or `Error` with the old daemon intact.
    Upgrade { exe: PathBuf },
    /// When this client disconnects, start `exe` in `cwd` (section 4). Answered `Ok` at once.
    RelaunchClient { exe: PathBuf, cwd: PathBuf },
```

Append to `Response` after `Error`:

```rust
    /// The answer to a version 2 `Hello`: what runs and what it speaks.
    Hello { daemon_version: String, protocol_min: u32, protocol_max: u32 },
```

- [ ] **Step 4: Version strings**

Create `crates/sarkosd/build.rs` and `crates/sarkos/build.rs`, identical:

```rust
//! Records the short git hash so `--version` names the exact build (update-in-place spec,
//! section 6). `unknown` outside a git checkout.
fn main() {
    let hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=SARKOS_GIT_HASH={hash}");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs");
}
```

In each binary's `main.rs`:

```rust
/// `<name> <crate version> (<short git hash>)`, the same shape for both binaries.
pub fn version_string() -> String {
    format!("{} {} ({})", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"), env!("SARKOS_GIT_HASH"))
}
```

and `--version` prints `version_string()`. In `sarkosd/src/main.rs` add the flag, before the argument loop's `--socket` arm:

```rust
            "--check-handover" => {
                println!("handover {}", handover::FORMAT);
                return;
            }
```

- [ ] **Step 5: The daemon's Hello**

In `server.rs`, `ClientRecord` gains `pub protocol: u32` (0 until Hello). In `spawn_client`'s reader loop replace the version check with:

```rust
                        if let Request::Hello { protocol_version, .. } = msg.request {
                            if !(PROTOCOL_MIN..=PROTOCOL_VERSION).contains(&protocol_version) {
                                let error = format!(
                                    "protocol version mismatch: daemon speaks {PROTOCOL_MIN} to {PROTOCOL_VERSION}, client {protocol_version}; \
                                     update the daemon: run Update Sarkos IDE now"
                                );
                                eprintln!("sarkosd: client {client_id} dropped: {error}");
                                sink.send(ServerMessage::Response { id: msg.id, response: Response::Error(error) });
                                break;
                            }
                        }
```

and in `dispatch`'s `Hello` arm, record `rec.protocol = protocol_version` and answer:

```rust
            if protocol_version >= 2 {
                Response::Hello { daemon_version: crate::version_string(), protocol_min: PROTOCOL_MIN, protocol_max: PROTOCOL_VERSION }
            } else {
                Response::Ok
            }
```

(`Request::Hello { protocol_version, .. }` is now bound by name in the pattern.) `Upgrade` and `RelaunchClient` get `Response::Error("not yet".into())` arms in this task so the match is exhaustive; Task 4 and Task 6 replace them. Import `PROTOCOL_MIN`.

- [ ] **Step 6: The window side**

`sessions.rs`: `Outcome::DaemonHello { version: String, protocol_min: u32, protocol_max: u32 }`; in `handle_response`, before the `match (pending.clone(), response.clone())`, add:

```rust
        if let Response::Hello { daemon_version, protocol_min, protocol_max } = &response {
            return Outcome::DaemonHello { version: daemon_version.clone(), protocol_min: *protocol_min, protocol_max: *protocol_max };
        }
```

`main.rs`: `Root.daemon_version: Option<String>` (`None` at start), and in `handle_outcome`: `Outcome::DaemonHello { version, .. } => { self.daemon_version = Some(version); }`. `client.rs` already sends `PROTOCOL_VERSION` in `Hello`, now 2. The README's "a protocol change requires restarting the daemon" line is rewritten in Task 6.

- [ ] **Step 7: Run everything**

Run: `~/.cargo/bin/cargo test --workspace 2>&1 | grep "test result" && ~/.cargo/bin/cargo build --workspace 2>&1 | grep -c "^warning"`
Expected: every suite passes (the daemon suite now includes the three new tests); `0`.

- [ ] **Step 8: Commit**

```bash
git add crates/protocol crates/sarkosd crates/sarkos/build.rs crates/sarkos/src/main.rs crates/sarkos/src/sessions.rs crates/sarkos/src/client.rs
git commit -m "Protocol 2: append Upgrade, RelaunchClient and Hello, serve version 1 windows too"
```

---

### Task 4: Upgrade and resume in the daemon

**Files:**
- Create: `crates/sarkosd/src/upgrade.rs`
- Modify: `crates/sarkosd/src/session.rs` (pause and park), `crates/sarkosd/src/server.rs` (`Options.socket`, `State.listener_fd`, the `Upgrade` arm, `run_resumed`, the shared accept loop), `crates/sarkosd/src/main.rs` (`--resume <path>`), `crates/sarkosd/src/pty.rs` (`set_inheritable_fd` made public)
- Test: `crates/sarkosd/tests/daemon.rs` (append)

**Interfaces:**
- Consumes: `handover::{Handover, SessionRecord, write, read, FORMAT}`, `Session::{snapshot, adopt, size}`, `pty::Pty::{adopt, set_inheritable, raw_fd}`.
- Produces:
  - `pty::set_inheritable_fd(fd: RawFd, on: bool) -> io::Result<()>` (the former private `set_cloexec`, inverted), for the listener.
  - `Session::pause(&self)`, `Session::wait_parked(&self, timeout: Duration) -> bool`, `Session::unpause(&self)`: the reader stops between reads and reports it; `yield_to_slow_subscribers` also returns while paused.
  - `upgrade::validate(exe: &Path) -> Result<(), String>`: a regular file named `sarkosd`, owned by the current uid, with an execute bit, and `exe --check-handover` exiting 0 within 5 s with stdout `handover <FORMAT>`.
  - `upgrade::begin(state: &Shared) -> Result<(), String>` (`Err("an upgrade is already in progress")` when the flag is set) and `upgrade::abort(state)`.
  - `upgrade::execute(state: &Shared, exe: &Path, opts: &Options) -> String`: parks every reader, writes the handover file next to the socket as `handover-<pid>.bin`, makes the masters and the listener inheritable, and calls `execv(exe, ["sarkosd", "--socket", <socket>, "--resume", <file>])`. Returns only when the exec failed, after restoring close-on-exec, unparking the readers, deleting the file and clearing the flag; the string is the error.
  - `server::Options` becomes `Clone` (not `Copy`) and gains `socket: PathBuf`; `State` gains `listener_fd: RawFd` and `upgrading: bool`.
  - `server::run_resumed(h: Handover, socket: PathBuf) -> Result<()>`: adopts the listener and every session from `h`, then runs the same accept loop as `run`.
  - `sarkosd --resume <path>`: reads the file (a failure is fatal, exit 1, logged), deletes it, calls `run_resumed`.
  - The `Upgrade` request: `begin` (claim the slot), `validate` (`abort` on failure), then `execute` on the requesting client's reader thread; the client sees `Response::Error(..)` on failure, and on success no response at all, only the connection ending when the exec closes every socket. (The error cannot be delivered after an exec, and an `Ok` before it would race the exec, so the window treats the disconnect as success and an `Error` as failure.)

- [ ] **Step 1: Write the failing tests**

Append to `crates/sarkosd/tests/daemon.rs`. First two harness helpers inside `impl Daemon` and `impl Conn`:

```rust
    /// The daemon's pid, which an exec-in-place must keep.
    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Connects again after an upgrade: the socket file stays, the listener passes through the
    /// exec, so this succeeds once the new image accepts.
    fn reconnect(&self) -> Conn {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if UnixStream::connect(&self.socket).is_ok() {
                return self.connect();
            }
            assert!(Instant::now() < deadline, "the resumed daemon never accepted");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
```

```rust
    /// Reads until the daemon closes the connection (an upgrade's exec), within `timeout`.
    fn expect_disconnect(&mut self, timeout: Duration) {
        self.set_read_timeout(Some(timeout));
        loop {
            match read_frame::<_, ServerMessage>(&mut self.r) {
                Ok(Some(ServerMessage::Response { response: Response::Error(e), .. })) => panic!("upgrade refused: {e}"),
                Ok(Some(_)) => continue,
                Ok(None) | Err(_) => return,
            }
        }
    }
```

Then the tests:

```rust
/// A fake `sarkosd` at `dir/sarkosd` whose body is `script`, for the validation paths.
fn fake_sarkosd(dir: &std::path::Path, script: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let exe = dir.join("sarkosd");
    std::fs::write(&exe, format!("#!/bin/sh\n{script}\n")).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    exe
}

fn parent_pid(pid: u32) -> u32 {
    let out = Command::new("ps").args(["-o", "ppid=", "-p", &pid.to_string()]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

#[test]
fn upgrade_onto_the_same_binary_keeps_sessions_the_pid_and_parenthood() {
    let d = Daemon::start(&["--reaper-millis", "100"]);
    let mut c = d.connect();
    let sid = c.create("/bin/sh");
    assert!(matches!(c.request(Request::Attach { session: sid }), Response::Attached { .. }));
    c.send(Request::Input { session: sid, bytes: b"echo marker-$((20+1))\n".to_vec() });
    c.wait_for_output(sid, "marker-21");
    let shell_pid = match c.request(Request::ListSessions) {
        Response::Sessions(list) => list.iter().find(|s| s.id == sid).unwrap().pid.unwrap(),
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(parent_pid(shell_pid), d.pid(), "the shell is the daemon's child before");
    c.send(Request::Upgrade { exe: PathBuf::from(env!("CARGO_BIN_EXE_sarkosd")) });
    c.expect_disconnect(Duration::from_secs(10));
    let mut c = d.reconnect();
    let listed = match c.request(Request::ListSessions) {
        Response::Sessions(list) => list.into_iter().find(|s| s.id == sid).expect("the session survived"),
        other => panic!("unexpected {other:?}"),
    };
    assert!(listed.alive);
    assert_eq!(listed.pid, Some(shell_pid));
    assert_eq!(parent_pid(shell_pid), d.pid(), "same daemon pid, still the parent after the exec");
    match c.request(Request::Attach { session: sid }) {
        Response::Attached { replay, alive } => {
            assert!(alive);
            assert!(String::from_utf8_lossy(&replay).contains("marker-21"), "the replay crossed the swap");
        }
        other => panic!("unexpected {other:?}"),
    }
    c.send(Request::Input { session: sid, bytes: b"echo marker-$((40+2))\n".to_vec() });
    c.wait_for_output(sid, "marker-42");
    c.send(Request::Input { session: sid, bytes: b"exit 5\n".to_vec() });
    assert_eq!(wait_for_exit(&mut c, sid, Duration::from_secs(5)), Some(5), "the new image reaps the shell");
}

#[test]
fn output_printed_during_the_swap_is_not_lost() {
    let d = Daemon::start(&[]);
    let mut c = d.connect();
    let sid = c.create("/bin/sh");
    assert!(matches!(c.request(Request::Attach { session: sid }), Response::Attached { .. }));
    c.send(Request::Input { session: sid, bytes: b"sleep 0.3; echo late-$((1+1))\n".to_vec() });
    c.send(Request::Upgrade { exe: PathBuf::from(env!("CARGO_BIN_EXE_sarkosd")) });
    c.expect_disconnect(Duration::from_secs(10));
    let mut c = d.reconnect();
    let replay = match c.request(Request::Attach { session: sid }) {
        Response::Attached { replay, .. } => replay,
        other => panic!("unexpected {other:?}"),
    };
    if !String::from_utf8_lossy(&replay).contains("late-2") {
        c.wait_for_output(sid, "late-2");
    }
}

#[test]
fn exit_during_the_swap_is_reported_after_resume() {
    let d = Daemon::start(&["--reaper-millis", "100"]);
    let mut c = d.connect();
    let sid = c.create("/bin/sh");
    assert!(matches!(c.request(Request::Attach { session: sid }), Response::Attached { .. }));
    c.send(Request::Input { session: sid, bytes: b"sleep 0.2; exit 9\n".to_vec() });
    c.send(Request::Upgrade { exe: PathBuf::from(env!("CARGO_BIN_EXE_sarkosd")) });
    c.expect_disconnect(Duration::from_secs(10));
    let mut c = d.reconnect();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match c.request(Request::ListSessions) {
            Response::Sessions(list) => {
                let s = list.iter().find(|s| s.id == sid).expect("listed");
                if !s.alive {
                    break;
                }
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(Instant::now() < deadline, "the exit during the swap was never noticed");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn upgrade_with_a_missing_binary_is_refused() {
    let d = Daemon::start(&[]);
    let mut c = d.connect();
    let sid = c.create("/bin/sh");
    assert!(matches!(c.request(Request::Attach { session: sid }), Response::Attached { .. }));
    match c.request(Request::Upgrade { exe: std::env::temp_dir().join("no-such-dir").join("sarkosd") }) {
        Response::Error(e) => assert!(e.contains("sarkosd"), "{e}"),
        other => panic!("unexpected {other:?}"),
    }
    match c.request(Request::Upgrade { exe: PathBuf::from("/bin/sh") }) {
        Response::Error(e) => assert!(e.contains("named sarkosd"), "{e}"),
        other => panic!("unexpected {other:?}"),
    }
    c.send(Request::Input { session: sid, bytes: b"echo still-$((1+1))\n".to_vec() });
    c.wait_for_output(sid, "still-2");
}

#[test]
fn upgrade_with_a_binary_that_fails_the_check_keeps_the_old_daemon() {
    let d = Daemon::start(&[]);
    let dir = tempfile::tempdir().unwrap();
    let exe = fake_sarkosd(dir.path(), "exit 1");
    let mut c = d.connect();
    let sid = c.create("/bin/sh");
    assert!(matches!(c.request(Request::Attach { session: sid }), Response::Attached { .. }));
    match c.request(Request::Upgrade { exe }) {
        Response::Error(e) => assert!(e.contains("check-handover"), "{e}"),
        other => panic!("unexpected {other:?}"),
    }
    let exe = fake_sarkosd(dir.path(), "echo handover 999");
    match c.request(Request::Upgrade { exe }) {
        Response::Error(e) => assert!(e.contains("handover 999") || e.contains("format"), "{e}"),
        other => panic!("unexpected {other:?}"),
    }
    c.send(Request::Input { session: sid, bytes: b"echo still-$((2+2))\n".to_vec() });
    c.wait_for_output(sid, "still-4");
}

#[test]
fn a_second_upgrade_during_the_check_is_refused() {
    let d = Daemon::start(&[]);
    let dir = tempfile::tempdir().unwrap();
    let slow = fake_sarkosd(dir.path(), "sleep 1; exit 1");
    let mut a = d.connect();
    let mut b = d.connect();
    let first = a.send(Request::Upgrade { exe: slow.clone() });
    std::thread::sleep(Duration::from_millis(200));
    match b.request(Request::Upgrade { exe: slow.clone() }) {
        Response::Error(e) => assert!(e.contains("already in progress"), "{e}"),
        other => panic!("unexpected {other:?}"),
    }
    // The first attempt fails its check and clears the flag; a new attempt is judged afresh.
    loop {
        if let ServerMessage::Response { id, response } = a.next_message() {
            if id == first {
                assert!(matches!(response, Response::Error(ref e) if e.contains("check-handover")), "{response:?}");
                break;
            }
        }
    }
    match b.request(Request::Upgrade { exe: slow }) {
        Response::Error(e) => assert!(e.contains("check-handover"), "{e}"),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn resume_with_a_corrupt_handover_file_exits_without_touching_anything() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("handover-1.bin");
    std::fs::write(&file, b"SKHOgarbage").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_sarkosd"))
        .arg("--socket")
        .arg(dir.path().join("d.sock"))
        .arg("--resume")
        .arg(&file)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("handover"));
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkosd --test daemon -- upgrade resume swap 2>&1 | tail -20`
Expected: the validation tests fail with `Error("not yet")`; the rest fail on `expect_disconnect` (the daemon never closes).

- [ ] **Step 3: Pause and park on `Session`**

In `session.rs`, add to `Session`: `paused: AtomicBool`, `parked: AtomicBool`. Add:

```rust
    /// Stops the reader between reads (the handover's snapshot must be the last word). Bytes the
    /// shell prints meanwhile wait in the kernel.
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    /// True once the reader has stopped and will not read until `unpause`.
    pub fn wait_parked(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while !self.parked.load(Ordering::SeqCst) {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        true
    }

    pub fn unpause(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }
```

A session whose shell has exited has no reader; `wait_parked` on it must return true at once: set `parked` to true in `mark_exited` and in `adopt` for a dead record. In `start_reader`'s loop, before `read_ready`:

```rust
                    if session.paused.load(Ordering::SeqCst) {
                        session.parked.store(true, Ordering::SeqCst);
                        while session.paused.load(Ordering::SeqCst) {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        session.parked.store(false, Ordering::SeqCst);
                        continue;
                    }
```

and in `yield_to_slow_subscribers`'s inner loop, break when `self.paused.load(Ordering::SeqCst)` as well as on `is_ending()`.

- [ ] **Step 4: The upgrade module**

Create `crates/sarkosd/src/upgrade.rs`:

```rust
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
    let mut child = Command::new(exe)
        .arg("--check-handover")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot run {} --check-handover: {e}", exe.display()))?;
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
    let restore = |why: String| -> String {
        for s in &sessions {
            let _ = s.master.set_inheritable(false);
            s.unpause();
        }
        let _ = pty::set_inheritable_fd(listener_fd, false);
        let _ = std::fs::remove_file(&path);
        abort(state);
        eprintln!("sarkosd: upgrade to {} failed: {why}; the old daemon keeps running", exe.display());
        why
    };
    if let Err(e) = handover::write(&path, &h) {
        return restore(format!("cannot write {}: {e}", path.display()));
    }
    for s in &sessions {
        if let Err(e) = s.master.set_inheritable(true) {
            return restore(format!("cannot make session {}'s pty inheritable: {e}", s.id.0));
        }
    }
    if let Err(e) = pty::set_inheritable_fd(listener_fd, true) {
        return restore(format!("cannot make the listener inheritable: {e}"));
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
    eprintln!("sarkosd: upgrading to {} with {} sessions, handover {}", exe.display(), h.sessions.len(), path.display());
    // SAFETY: argv is NUL-terminated and every pointer outlives the call; execv returns only on
    // failure, in which case this process continues unchanged.
    unsafe { libc::execv(exe_c.as_ptr(), argv.as_ptr()) };
    restore(format!("execv failed: {}", std::io::Error::last_os_error()))
}
```

(`State::next_session()` is a small accessor to add, since the field is private.) `pty::set_inheritable_fd(fd, on)` is the renamed public form of `set_cloexec` with the flag inverted. Add `mod upgrade;` to `main.rs`.

- [ ] **Step 5: The server: options, the Upgrade arm, resume**

In `server.rs`:

1. `Options` derives `Clone` instead of `Copy` and gains `pub socket: PathBuf`; `run` sets it (it already has `socket`), and `spawn_client` and `dispatch` take `&Options` or clone it.
2. `State` gains `pub listener_fd: RawFd` (set in `run` from `listener.as_raw_fd()` right after the bind) and `pub upgrading: bool`; `pub fn next_session(&self) -> u64`.
3. Split `run` into `run` (bind, chmod, state, then `serve`) and `fn serve(listener: UnixListener, state: Shared, opts: Options) -> Result<()>` (watchdog, reaper, accept loop).
4. The `Upgrade` arm in `dispatch`:

```rust
        Request::Upgrade { exe } => {
            // Claim the slot first: the check takes seconds, and a second client must be
            // refused for its whole length, not only after it passes.
            if let Err(e) = crate::upgrade::begin(state) {
                return Some(Response::Error(e));
            }
            if let Err(e) = crate::upgrade::validate(&exe) {
                crate::upgrade::abort(state);
                return Some(Response::Error(e));
            }
            // Returns only on failure; on success this process is now the new binary.
            Response::Error(crate::upgrade::execute(state, &exe, opts))
        }
```

5. Resume:

```rust
/// The new image after an upgrade: adopt the listener and every session, then serve.
pub fn run_resumed(h: Handover, socket: PathBuf) -> Result<()> {
    use std::os::fd::FromRawFd;
    crate::pty::set_inheritable_fd(h.listener_fd, false).context("listener close-on-exec")?;
    // SAFETY: the descriptor was passed through execve by the old image for this purpose.
    let listener = unsafe { UnixListener::from_raw_fd(h.listener_fd) };
    let opts = Options {
        replay_capacity: h.replay_capacity,
        watchdog: Duration::from_secs(h.watchdog_secs),
        client_pending_cap: h.client_pending_cap,
        backpressure: Backpressure { high_water: h.high_water, low_water: h.low_water, stall: Duration::from_secs(h.stall_secs) },
        reaper_every: Duration::from_millis(h.reaper_millis.max(10)),
        dead_session_ttl: Duration::from_secs(h.dead_session_secs),
        socket,
    };
    let mut st = State::new();
    st.next_session = h.next_session;
    st.listener_fd = h.listener_fd;
    for rec in &h.sessions {
        let master = match crate::pty::Pty::adopt(rec.master_fd) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("sarkosd: session {} lost in the handover: cannot adopt fd {}: {e}", rec.id, rec.master_fd);
                continue;
            }
        };
        let s = Session::adopt(rec, master, opts.replay_capacity, opts.backpressure);
        st.sessions.insert(SessionId(rec.id), s);
    }
    eprintln!("sarkosd: resumed {} sessions from a handover written by {}", st.sessions.len(), h.written_by);
    serve(listener, Arc::new(Mutex::new(st)), opts)
}
```

6. `disconnect` must not drop a client that the exec already closed: nothing to do, the exec closed every socket and the new image has no clients.

In `main.rs`, parse `"--resume" => resume = Some(PathBuf::from(args.next().expect("--resume needs a path")))`, and after the loop:

```rust
    if let Some(path) = resume {
        let h = match handover::read(&path) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("sarkosd: cannot resume: {e}");
                std::process::exit(1);
            }
        };
        let _ = std::fs::remove_file(&path);
        if let Err(e) = server::run_resumed(h, socket) {
            eprintln!("sarkosd: {e:#}");
            std::process::exit(1);
        }
        return;
    }
```

- [ ] **Step 6: Run the daemon suite ten times**

Run: `~/.cargo/bin/cargo test -p sarkosd 2>&1 | grep "test result"` then `for i in $(seq 10); do ~/.cargo/bin/cargo test -p sarkosd --test daemon 2>&1 | grep -q "test result: ok" || echo "run $i failed"; done`
Expected: all green, every time; no "run N failed" line. The upgrade tests take a few seconds each (the shell's `-l` login and the check). `~/.cargo/bin/cargo build -p sarkosd 2>&1 | grep -c "^warning"` prints 0, and the Task 2 dead-code allows are removed.

Then a manual timing line for the report: with the daemon started by hand on a temp socket and four `/bin/zsh` sessions created and attached by a small script (or by running the test binary under `time`), the `upgrading to` and `resumed` log lines in the daemon's stderr are under one second apart.

- [ ] **Step 7: Commit**

```bash
git add crates/sarkosd
git commit -m "Upgrade the daemon by exec-in-place and resume from the handover file"
```

---

### Task 5: The window reconnects by itself

**Files:**
- Create: `crates/sarkos/src/connection.rs`
- Modify: `crates/sarkos/src/main.rs` (the event loop, heartbeat and quit handlers move or change; `Root::{connected, disconnected}`, `daemon_label`, banner timing), `crates/sarkos/src/sessions.rs` (`set_client`, `disconnect`), `crates/sarkos/src/pane_view.rs` (nothing new: the busy mark text "reconnecting" renders through the existing busy path)
- Test: unit tests in `connection.rs`, `sessions.rs`, `main.rs`

**Interfaces:**
- Consumes: `client::Client::{connect, connect_or_start, disconnect_reason, consumed_output}`, `Root::handle_outcome`, `sessions::reconcile`.
- Produces:
  - `connection::FIRST_RETRY = 250 ms`, `connection::MAX_RETRY = 2 s`, `connection::START_DAEMON_AFTER = 10 s`, `connection::BANNER_AFTER = 10 s`.
  - `connection::backoff(attempt: u32) -> Duration`: 250 ms doubling, capped at 2 s.
  - `connection::should_start_daemon(lost_for: Duration) -> bool`.
  - `connection::start(root: Entity<Root>, socket: PathBuf, daemon_exe: PathBuf, daemon_args: Vec<String>, first: (Arc<Client>, UnboundedReceiver<ServerMessage>), cx: &mut App)`: owns the connection for the life of the window: binds a client to `Root`, runs its heartbeat and event loop, and when the connection ends reconnects with backoff, starting a daemon once the outage passes ten seconds.
  - `SessionManager::set_client(&mut self, client: Arc<Client>)` (transport and client) and `SessionManager::disconnect(&mut self)` (drops every terminal, pending request, busy mark, restore state and note).
  - `Root::connected(&mut self, client: Arc<Client>, cx)`: `set_client`, `status = CONNECTING`, `reconnecting_since = None`, `connection_generation += 1`, `sessions.list()`.
  - `Root::disconnected(&mut self, reason: String, cx)`: `sessions.disconnect()`, every terminal tab's busy mark set to `"reconnecting"`, `status = reason`, `reconnecting_since = Some(now)`.
  - `Root { connection_generation: u64, reconnecting_since: Option<Instant> }`; `daemon_label` gains `"sarkosd reconnecting"` while `reconnecting_since` is set; the status banner for a disconnect shows only after `BANNER_AFTER` (a pure `banner_visible(reconnecting_since: Option<Instant>, now: Instant) -> bool`).
  - `flush_on_quit(root, queue, cx)` reads the current client from `root.sessions.client`.
  - After a reconnect, reconcile runs with `saved_layout = true`, so a session the daemon no longer has becomes a fresh shell with the "(restarted)" note, as a relaunch does today.

- [ ] **Step 1: Write the failing tests**

Create `crates/sarkos/src/connection.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_from_a_quarter_second_to_two_seconds() {
        assert_eq!(backoff(0), Duration::from_millis(250));
        assert_eq!(backoff(1), Duration::from_millis(500));
        assert_eq!(backoff(2), Duration::from_millis(1000));
        assert_eq!(backoff(3), Duration::from_millis(2000));
        assert_eq!(backoff(4), Duration::from_millis(2000));
        assert_eq!(backoff(40), Duration::from_millis(2000), "no overflow");
    }

    #[test]
    fn after_ten_seconds_the_window_starts_a_daemon() {
        assert!(!should_start_daemon(Duration::from_secs(0)));
        assert!(!should_start_daemon(Duration::from_millis(9_999)));
        assert!(should_start_daemon(Duration::from_secs(10)));
    }
}
```

Append to the `tests` module in `sessions.rs`:

```rust
    #[test]
    fn disconnect_forgets_terminals_pendings_and_marks() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r.clone(), None, PathBuf::from("/p"));
        m.create(50, true);
        m.attach(SessionId(3), 61, false);
        m.begin_restore(SessionId(3));
        m.notes.insert(SessionId(3));
        assert_eq!(m.pending_count(), 2);
        assert!(m.has_pending_for(50));
        m.disconnect();
        assert_eq!(m.pending_count(), 0);
        assert!(!m.has_pending_for(50) && !m.has_pending_for(61));
        assert!(!m.is_restoring(SessionId(3)));
        assert!(m.notes.is_empty());
        assert!(m.terminals.is_empty());
        // A response from the old connection, arriving late, matches nothing.
        assert_eq!(m.handle_response(1, Response::SessionCreated { id: SessionId(9) }), Outcome::None);
        let r2 = Recorder::new();
        m.set_client_for_test(r2.clone());
        m.list();
        assert_eq!(r2.requests().len(), 1, "requests go to the new transport");
    }
```

(`set_client_for_test(transport: Arc<dyn Transport>)` is a `#[cfg(test)]` twin of `set_client`, since tests have no real `Client`; both set `transport`, and `set_client` also sets `client`.)

Append to the `tests` module in `main.rs`:

```rust
    #[test]
    fn a_disconnect_banner_waits_ten_seconds_but_the_label_says_so_at_once() {
        use super::{banner_visible, daemon_label_with};
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        assert!(!banner_visible(Some(t0), t0 + Duration::from_secs(3)));
        assert!(banner_visible(Some(t0), t0 + Duration::from_secs(10)));
        assert!(banner_visible(None, t0), "a status that is not a reconnect shows at once");
        assert_eq!(daemon_label_with("daemon closed the connection", true), "sarkosd reconnecting");
        assert_eq!(daemon_label_with("", false), "sarkosd connected");
    }
```

(`daemon_label_with(status, reconnecting)` is the existing `daemon_label` with the reconnect flag; keep `daemon_label(status)` calling it with `false` so the existing test stands.)

- [ ] **Step 2: Run the tests to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkos -- connection disconnect_forgets banner_waits 2>&1 | tail -20`
Expected: compile errors.

- [ ] **Step 3: The pure parts and the session manager**

`connection.rs`, above the tests:

```rust
//! The window's link to sarkosd for the life of the window (update-in-place spec, section 3):
//! one client at a time, its heartbeat and event loop, and a reconnect with backoff when the
//! daemon's connection ends, which an upgrade's exec does on purpose. After ten seconds without
//! a daemon the window starts one, since the old one is gone for good.
use crate::client::Client;
use crate::{output_len, yield_now, Root, ERROR_PREFIX};
use futures::channel::mpsc::{TryRecvError, UnboundedReceiver};
use futures::StreamExt;
use gpui::{App, Entity};
use protocol::{Event, Request, Response, ServerMessage};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const FIRST_RETRY: Duration = Duration::from_millis(250);
pub const MAX_RETRY: Duration = Duration::from_secs(2);
pub const START_DAEMON_AFTER: Duration = Duration::from_secs(10);
pub const BANNER_AFTER: Duration = Duration::from_secs(10);
/// Output bytes applied in one batch before the event loop yields to the frame loop.
const BATCH_BUDGET: usize = 1 << 20;

pub fn backoff(attempt: u32) -> Duration {
    let factor = 1u32 << attempt.min(3);
    (FIRST_RETRY * factor).min(MAX_RETRY)
}

pub fn should_start_daemon(lost_for: Duration) -> bool {
    lost_for >= START_DAEMON_AFTER
}
```

Then `start`:

```rust
/// Binds `first`, then keeps the window connected until it closes.
pub fn start(root: Entity<Root>, socket: PathBuf, daemon_exe: PathBuf, daemon_args: Vec<String>, first: (Arc<Client>, UnboundedReceiver<ServerMessage>), cx: &mut App) {
    cx.spawn(async move |cx| {
        let (mut client, mut events) = first;
        loop {
            let generation = match root.update(cx, |root, cx| {
                root.connected(client.clone(), cx);
                root.connection_generation
            }) {
                Ok(g) => g,
                Err(_) => return,
            };
            heartbeat(root.clone(), client.clone(), generation, cx);
            let reason = pump(&root, &client, &mut events, cx).await;
            if root.update(cx, |root, cx| root.disconnected(reason, cx)).is_err() {
                return;
            }
            let lost_at = Instant::now();
            let mut attempt = 0u32;
            let next = loop {
                cx.background_executor().timer(backoff(attempt)).await;
                attempt += 1;
                let (s, d, a) = (socket.clone(), daemon_exe.clone(), daemon_args.clone());
                let start = should_start_daemon(lost_at.elapsed());
                let attempt_result = cx
                    .background_executor()
                    .spawn(async move { if start { Client::connect_or_start(&s, &d, &a) } else { Client::connect(&s) } })
                    .await;
                match attempt_result {
                    Ok(c) => break c,
                    Err(e) => {
                        if root.update(cx, |root, cx| {
                            if start {
                                root.status = format!("sarkosd is not running and could not be started: {e:#}");
                            }
                            cx.notify();
                        }).is_err() {
                            return;
                        }
                    }
                }
            };
            client = Arc::new(next.0);
            events = next.1;
        }
    })
    .detach();
}

/// Sends a heartbeat each second while `generation` is the window's current connection.
fn heartbeat(root: Entity<Root>, client: Arc<Client>, generation: u64, cx: &mut gpui::AsyncApp) {
    cx.spawn(async move |cx| {
        let mut seq = 0u64;
        loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            match root.read_with(cx, |root, _| root.connection_generation) {
                Ok(g) if g == generation => {}
                _ => return,
            }
            client.send(Request::Heartbeat { seq });
            seq += 1;
        }
    })
    .detach();
}
```

`pump` is the body of today's event task in `main.rs`, moved verbatim with these changes: it takes `root`, `client`, `events` and returns the disconnect reason instead of setting `root.status` itself; `saved_layout` is always `true` after the first connection (pass a `bool` for the first pump: the caller decides, `main` passes `saved.is_some()` for the first connection and `start` uses `true` afterwards; thread it as a field on `Root`, `reconcile_with_note: bool`, set by `connected`). The "older build" hint stays in the reason when nothing was answered. `output_len`, `yield_now`, `BATCH_BUDGET` and `ERROR_PREFIX` move to `connection.rs` or become `pub(crate)` in `main.rs`; pick one and delete the other copy.

`sessions.rs`:

```rust
    /// A new connection: requests go to it from now on.
    pub fn set_client(&mut self, client: Arc<Client>) {
        self.transport = client.clone();
        self.client = Some(client);
    }

    #[cfg(test)]
    pub fn set_client_for_test(&mut self, transport: Arc<dyn Transport>) {
        self.transport = transport;
    }

    /// The connection ended: every terminal is gone with it (the next attach replays), nothing
    /// in flight can be answered, and no tab is busy with anything from the old connection.
    pub fn disconnect(&mut self) {
        self.terminals.clear();
        self.pending.clear();
        self.busy.clear();
        self.notes.clear();
        self.restoring.clear();
        self.backlog.clear();
    }
```

- [ ] **Step 4: Root and main**

`main.rs`:

1. Fields: `pub connection_generation: u64` (0), `pub reconnecting_since: Option<Instant>` (None), `pub reconcile_with_note: bool` (set from `saved.is_some()` in `Root::new`).
2. Methods:

```rust
    /// A connection is up (the first one, or a reconnect): its session list reconciles the tree.
    pub fn connected(&mut self, client: Arc<client::Client>, cx: &mut Context<Self>) {
        self.sessions.set_client(client);
        self.connection_generation += 1;
        self.reconnecting_since = None;
        self.status = CONNECTING.into();
        self.grids.clear();
        self.sessions.list();
        cx.notify();
    }

    /// The connection ended. Every terminal tab waits for the reconnect; the daemon kept the
    /// sessions, so the next attach replays them.
    pub fn disconnected(&mut self, reason: String, cx: &mut Context<Self>) {
        eprintln!("sarkos: {reason}; reconnecting");
        self.sessions.disconnect();
        for (pane, idx, tab) in self.tree.tabs() {
            if let Tab::Terminal { .. } = tab {
                if let Some(id) = self.tree.tab_id(pane, idx) {
                    self.sessions.busy.insert(id, "reconnecting");
                }
            }
        }
        // An error the daemon sent before hanging up (a protocol mismatch) says why; keep it.
        self.status = if self.status.starts_with(ERROR_PREFIX) { format!("{}; {reason}", self.status) } else { reason };
        self.reconnecting_since = Some(Instant::now());
        cx.notify();
    }
```

After a reconnect, `handle_outcome`'s `Outcome::Sessions` arm passes `self.reconcile_with_note` (true after the first connection) to `reconcile`'s create path; set `self.reconcile_with_note = true` at the end of `connected`.

3. Banner timing and label:

```rust
/// A reconnect's status waits `BANNER_AFTER` before it becomes a banner; the status bar says
/// "reconnecting" at once. Any other status shows at once.
fn banner_visible(reconnecting_since: Option<Instant>, now: Instant) -> bool {
    match reconnecting_since {
        Some(since) => now.duration_since(since) >= connection::BANNER_AFTER,
        None => true,
    }
}

fn daemon_label_with(status: &str, reconnecting: bool) -> &'static str {
    if reconnecting {
        "sarkosd reconnecting"
    } else {
        daemon_label(status)
    }
}
```

`render` uses `banner_visible(self.reconnecting_since, Instant::now())` to decide whether `status` joins the banner list, and `daemon_label_with(&self.status, self.reconnecting_since.is_some())` for the status bar. The reconnect loop notifies on every attempt (at most every two seconds), so the banner appears within two seconds of the ten-second mark; good enough.

4. `main()`: delete the heartbeat task, the `root.sessions.list()` call and the event task. `Root::new` keeps taking the first `Arc<Client>` as today. After the quit handlers and the config watcher, call `connection::start(root.clone(), paths::socket_path(), paths::daemon_exe(), daemon_args.clone(), (client.clone(), events), cx)`: the same `Arc` the root was built with, plus the receiver. `flush_on_quit(root, queue, cx)` no longer takes a client parameter; it reads the current one from `root.read(cx).sessions.client.clone()`.

- [ ] **Step 5: Run everything, then a live reconnect check**

Run: `~/.cargo/bin/cargo test -p sarkos 2>&1 | tail -15 && ~/.cargo/bin/cargo build -p sarkos 2>&1 | grep -c "^warning"`
Expected: all pass; `0`.

Live (release build, from `/tmp`, log to a file, the screen may be locked): with the window running, send the daemon an upgrade onto itself with a tiny client, or simpler, `kill -STOP` is not it; use the daemon test binary's path: run `pkill -x sarkosd` and watch the window's log: within a second it logs the disconnect and `reconnecting`; after ten seconds it logs that it started a daemon, every tab becomes a new shell with "(restarted)". Then quit. Record the log lines in the report.

- [ ] **Step 6: Commit**

```bash
git add crates/sarkos/src
git commit -m "Reconnect to the daemon with backoff and reattach every tab"
```

---

### Task 6: Update detection, the update command, RelaunchClient, and the README

**Files:**
- Create: `crates/sarkos/src/update.rs`
- Modify: `crates/sarkos/src/main.rs` (`Root::{update_ready, updating, update_now, relaunch_now}`, `handle_outcome` error path while updating, `connected` continuing an update), `crates/sarkos/src/keymap.rs` (`Command::UpdateNow`, the `update now` row with no default key), `crates/sarkos/src/config.rs` (`DEFAULT_CONFIG` line), `crates/sarkos/src/command_palette.rs` (the item), `crates/sarkos/src/chrome.rs` (status bar text), `crates/sarkosd/src/server.rs` (`RelaunchClient`, relaunch on disconnect), `crates/sarkosd/src/watchdog.rs` (`launch_ui`), `crates/sarkosd/tests/daemon.rs`, `README.md`
- Test: unit tests in `update.rs`, `keymap.rs`, `command_palette.rs`, `chrome.rs`; daemon tests

**Interfaces:**
- Consumes: `Root::{daemon_version, connected, show_notice}`, `connection::start`'s reconnect, `Request::{Upgrade, RelaunchClient}`, `paths::daemon_exe`.
- Produces:
  - `update::launch_path() -> Option<PathBuf>` (`std::env::current_exe`, unresolved: on macOS that is the path the process was launched through, the Homebrew symlink when installed that way), `update::sibling_daemon(exe: &Path) -> PathBuf`.
  - `update::installed_version(exe: &Path) -> Option<String>`: runs `exe --version`, two-second timeout, trimmed stdout.
  - `update::differs(running: &str, installed: &str) -> bool`: trimmed inequality (semver or git hash; a rebuild counts as an update, which is what a developer building from source wants).
  - `update::status_text(installed: &str) -> String`: `update ready: <version part>` where the version part is the text after the binary name.
  - `update::CHECK_EVERY = 5 min`; `update::start(root: Entity<Root>, cx)`: checks on launch and every five minutes on the background executor and sets `Root.update_ready`.
  - `Root { update_ready: Option<String>, updating: Option<UpdateStage> }`, `enum UpdateStage { DaemonUpgrading, Relaunching }`.
  - `Root::update_now(cx)`: on the background executor, reads the installed daemon's version; if it differs from `daemon_version`, sends `Upgrade { exe: sibling }`, sets `updating = DaemonUpgrading` and a notice "updating the daemon"; else `relaunch_now`. `Root::connected` continues a `DaemonUpgrading` update by calling `relaunch_now`. `Outcome::Error` while `DaemonUpgrading` clears `updating` and shows the error as a notice.
  - `Root::relaunch_now(cx)`: sends `RelaunchClient { exe: launch path, cwd: project }`, ends closed tabs, flushes the workspace, waits for the requests to be sent, and quits.
  - `Command::UpdateNow`; keymap row `("update now", "")` (unbound by default; `keys_for` returns `None` for an empty default, so `bindings_for` skips it); `DEFAULT_CONFIG` line `# "update now" = ""`; palette item "Update Sarkos IDE now" with detail `update_ready` or "relaunch on the installed binaries".
  - `chrome::status_bar` gains `update: Option<String>` shown before the daemon label.
  - Daemon: `RelaunchClient { exe, cwd }` validates `exe` like `ui_exe` (a regular file named `sarkos`, owned by this user) and stores `rec.relaunch = Some((exe, cwd))`; `disconnect` launches it through `watchdog::launch_ui(exe, cwd)` (factored from the watchdog's spawn), falling back to `rec.ui_exe` and `rec.ui_cwd` if the launch fails. A rejected `exe` answers `Error`.
  - README: an "Updating" section and checklist lines.

- [ ] **Step 1: Write the failing tests**

Create `crates/sarkos/src/update.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_differ_by_text_and_the_status_names_the_installed_one() {
        assert!(!differs("sarkos 0.1.0 (abc1234)", "sarkos 0.1.0 (abc1234)\n"));
        assert!(differs("sarkos 0.1.0 (abc1234)", "sarkos 0.1.0 (def5678)"), "a rebuild is an update");
        assert!(differs("sarkos 0.1.0 (abc1234)", "sarkos 0.2.0 (abc1234)"));
        assert_eq!(status_text("sarkos 0.2.0 (def5678)"), "update ready: 0.2.0 (def5678)");
        assert_eq!(status_text("garbage"), "update ready: garbage");
        assert_eq!(sibling_daemon(std::path::Path::new("/opt/homebrew/bin/sarkos")), std::path::PathBuf::from("/opt/homebrew/bin/sarkosd"));
    }

    #[test]
    fn installed_version_runs_the_binary_and_tolerates_a_missing_one() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(installed_version(&dir.path().join("nope")), None);
        use std::os::unix::fs::PermissionsExt;
        let exe = dir.path().join("sarkos");
        std::fs::write(&exe, "#!/bin/sh\necho sarkos 9.9.9 (test)\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(installed_version(&exe).as_deref(), Some("sarkos 9.9.9 (test)"));
        let slow = dir.path().join("sarkos-slow");
        std::fs::write(&slow, "#!/bin/sh\nsleep 5\n").unwrap();
        std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
        let started = std::time::Instant::now();
        assert_eq!(installed_version(&slow), None);
        assert!(started.elapsed() < Duration::from_secs(4), "the timeout holds");
    }
}
```

Append to `keymap.rs`'s test table `SPEC_ROWS` the row `("", "update now", "UpdateNow")` and extend `the_table_is_the_spec_table` so a row with empty keys is allowed, plus in `overrides_name_actions_and_keys_exactly` the assertion `assert_eq!(keys_for("update now", &BTreeMap::new()), None, "unbound by default");` and that `bindings().len() == ACTIONS.len() - 1`. `every_row_runs_the_action_it_describes` skips rows with empty keys. In `command_palette.rs`'s `items_cover_actions_presets_themes_and_config`, the count becomes `n_actions + 3 + 2 + 2` with `n_actions = ACTIONS.len() - 2` still (the update row is an action like any other) and add `assert_eq!(items.iter().find(|i| i.label == "Update now").unwrap().command, Command::UpdateNow);` (the label is `keymap::label("update now")`; rename the row to `"update sarkos ide now"` if you want the palette to read "Update sarkos ide now", or keep `update now`; pick `"update now"` and accept the shorter label). In `chrome.rs` tests add:

```rust
    #[test]
    fn status_text_mentions_a_ready_update_first() {
        assert_eq!(status_items(Some("update ready: 0.2.0"), "sarkosd connected", 2, 0), vec!["update ready: 0.2.0", "sarkosd connected", "2 sessions"]);
        assert_eq!(status_items(None, "sarkosd connected", 1, 0), vec!["sarkosd connected", "1 session"]);
    }
```

(`status_items(update, daemon, sessions, waiting) -> Vec<String>` is a pure helper the status bar renders from.)

Append to `crates/sarkosd/tests/daemon.rs`:

```rust
#[test]
fn relaunch_client_starts_the_named_binary_when_the_client_disconnects() {
    let d = Daemon::start(&[]);
    let dir = tempfile::tempdir().unwrap();
    let (exe, marker) = relaunch_script(dir.path());
    let mut c = d.connect();
    assert_eq!(c.request(Request::RelaunchClient { exe: exe.clone(), cwd: dir.path().to_path_buf() }), Response::Ok);
    drop(c);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker.exists() {
        assert!(Instant::now() < deadline, "the daemon did not launch {}", exe.display());
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn relaunch_client_refuses_a_foreign_binary() {
    let d = Daemon::start(&[]);
    let mut c = d.connect();
    match c.request(Request::RelaunchClient { exe: PathBuf::from("/bin/sh"), cwd: std::env::temp_dir() }) {
        Response::Error(e) => assert!(e.contains("sarkos"), "{e}"),
        other => panic!("unexpected {other:?}"),
    }
}
```

(`relaunch_script(dir)` already exists in the file for the watchdog tests: it writes a `sarkos` script that touches a marker file and returns `(exe, marker)`; read it and reuse it as is. If its marker is written into `cwd` rather than a fixed path, pass `dir.path()` as the `cwd` here, as above.)

- [ ] **Step 2: Run the tests to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkos -- update status_text 2>&1 | tail -10; ~/.cargo/bin/cargo test -p sarkosd --test daemon -- relaunch_client 2>&1 | tail -10`
Expected: compile errors in sarkos; the daemon tests fail with `Error("not yet")`.

- [ ] **Step 3: The update module**

Above the tests in `update.rs`:

```rust
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
    exe.with_file_name("sarkosd")
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
```

- [ ] **Step 4: Root, keymap, palette, chrome**

`keymap.rs`: `UpdateNow` in `actions!`, `Command::UpdateNow`, `row("update now", "")` after `find`, the `binding` arm `"update now" => KeyBinding::new(keys, UpdateNow, context)` (never reached with empty keys, since `keys_for` returns `None`), the `attach` route, and `command_for`'s arm. `config.rs`: the line `# "update now" = ""` after `find` in `DEFAULT_CONFIG`. `command_palette.rs`: the item's detail is `update_ready` text when set, else "relaunch on the installed binaries" (pass `update_ready: Option<&str>` into `items`).

`chrome.rs`:

```rust
/// The right side of the status bar, left to right.
pub fn status_items(update: Option<&str>, daemon: &str, sessions: usize, waiting: usize) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(u) = update {
        v.push(u.to_string());
    }
    v.push(daemon.to_string());
    v.push(status_text(sessions, waiting));
    v
}
```

and `status_bar` takes `update: Option<&str>` and renders `status_items`.

`main.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStage {
    DaemonUpgrading,
    Relaunching,
}
```

fields `pub update_ready: Option<String>`, `pub updating: Option<UpdateStage>`; `run` gains `UpdateNow => self.update_now(cx)`;

```rust
    /// The palette's "Update now": the daemon first when its binary changed, then this window.
    pub fn update_now(&mut self, cx: &mut Context<Self>) {
        if self.updating.is_some() {
            self.show_notice("an update is already running".into(), cx);
            return;
        }
        let Some(exe) = update::launch_path() else {
            self.show_notice("cannot find this window's own binary".into(), cx);
            return;
        };
        let daemon_exe = update::sibling_daemon(&exe);
        let running_daemon = self.daemon_version.clone();
        let root = cx.entity();
        cx.spawn(async move |_, cx| {
            let d = daemon_exe.clone();
            let installed = cx.background_executor().spawn(async move { update::installed_version(&d) }).await;
            let _ = root.update(cx, |root, cx| match installed {
                None => root.show_notice(format!("cannot run {} --version", daemon_exe.display()), cx),
                Some(installed) if running_daemon.as_deref().map_or(true, |r| update::differs(r, &installed)) => {
                    root.updating = Some(UpdateStage::DaemonUpgrading);
                    root.show_notice("updating the daemon; sessions stay".into(), cx);
                    root.sessions.transport.send(Request::Upgrade { exe: daemon_exe.clone() });
                }
                Some(_) => root.relaunch_now(cx),
            });
        })
        .detach();
    }

    /// Asks the daemon to start the installed window in this project once we are gone, then quits.
    pub fn relaunch_now(&mut self, cx: &mut Context<Self>) {
        let Some(exe) = update::launch_path() else { return };
        self.updating = Some(UpdateStage::Relaunching);
        self.sessions.transport.send(Request::RelaunchClient { exe, cwd: self.project.clone() });
        let killed = self.end_closed_tabs();
        if let Some(client) = self.sessions.client.clone() {
            if !client.wait_sent(Duration::from_millis(500)) {
                eprintln!("sarkos: could not send the relaunch request ({killed} kills pending)");
            }
        }
        workspace::flush_now(&self.save_queue);
        cx.quit();
    }
```

`connected` gains, after `sessions.list()`: `if self.updating == Some(UpdateStage::DaemonUpgrading) { self.relaunch_now(cx); }`. `handle_outcome`'s `Outcome::Error(e)` arm: when `self.updating == Some(UpdateStage::DaemonUpgrading)`, set `updating = None` and show the notice `format!("update failed: {e}")` instead of the status banner. `main()` calls `update::start(root.clone(), cx)` after `connection::start`. `render` passes `self.update_ready.as_deref()` to `status_bar`. `version_string()` exists from Task 3.

Daemon side, `server.rs`:

```rust
        Request::RelaunchClient { exe, cwd } => {
            let named_sarkos = exe.file_name().map(|n| n == "sarkos").unwrap_or(false);
            let owned = std::fs::metadata(&exe).map(|m| m.is_file() && m.uid() == unsafe { libc::getuid() }).unwrap_or(false);
            if !named_sarkos || !owned {
                Response::Error(format!("{} is not a sarkos binary owned by this user", exe.display()))
            } else {
                if let Some(rec) = state.lock().unwrap().clients.get_mut(&client_id) {
                    rec.relaunch = Some((exe, cwd));
                }
                Response::Ok
            }
        }
```

(`use std::os::unix::fs::MetadataExt;`). `ClientRecord` gains `pub relaunch: Option<(PathBuf, PathBuf)>`. In `disconnect`, after the record is removed:

```rust
    if let Some((exe, cwd)) = relaunch {
        if let Err(e) = crate::watchdog::launch_ui(&exe, Some(&cwd)) {
            eprintln!("sarkosd: relaunch of {} failed: {e}; launching the previous window instead", exe.display());
            if let Some(old) = ui_exe {
                if let Err(e) = crate::watchdog::launch_ui(&old, ui_cwd.as_deref()) {
                    eprintln!("sarkosd: relaunch of {} failed too: {e}", old.display());
                }
            }
        }
    }
```

where `relaunch`, `ui_exe` and `ui_cwd` were taken from the record under the lock, and the launches happen after the lock is released. `watchdog::launch_ui(exe: &Path, cwd: Option<&Path>) -> std::io::Result<()>` is the existing spawn block (null stdio, `setsid` in `pre_exec`, a thread that waits on the child), used by `scan` too.

- [ ] **Step 5: README**

In `README.md`:

1. Status paragraph: "Updating in place: the daemon replaces itself by exec with every shell still its child, the window reconnects by itself, and the palette's "Update now" runs the whole update after `brew upgrade` or a build into place. Milestone 3 (Claude Code session awareness, with recovery after a reboot) is next." Keep the Further reading line and add a second one for https://csarko.sh/research/bring-the-agents-back-after-a-reboot.
2. A section after Config:

```markdown
## Updating

Sessions live in `sarkosd`, so updating never ends one. After `brew upgrade sarkos`, or
`cargo build --release` into the binaries you run, the status bar shows "update ready" within
five minutes (or at the next launch), and the palette's "Update now" does the rest: if the
daemon's binary changed, the daemon writes a handover file, execs the new binary in the same
process (same pid, the shells stay its children) and the window reconnects; then the window
asks the daemon to relaunch it from the new binary and quits. Every tab comes back with its
history. The daemon and window speak protocol 2; the daemon still serves a protocol 1 window
from milestone 2b. Upgrading *to* this version from milestone 2b is the one update that needs
`pkill -x sarkosd` (the old daemon knows nothing of handovers), which ends every session.

If the new daemon fails its check or the exec fails, the old daemon keeps running and the window
shows the error. If the window loses the daemon for ten seconds, it starts a new one and every
tab becomes a fresh shell with the "(restarted)" note.
```

3. Checklist lines (replace the 2b block's first items with these, keep the rest):

```markdown
- with four `claude` sessions running, `cargo build --release` a change into place, wait for
  "update ready" in the status bar (or relaunch), run "Update now" from the palette: the window
  reappears with every tab, every history and every agent still running; `sarkosd`'s pid in
  `ps` is unchanged and its log shows "upgrading to" and "resumed N sessions" under a second apart
- `pkill -x sarkosd` with the window open: tabs say "reconnecting", the status bar too; after ten
  seconds a banner appears, a new daemon starts, and each tab comes back as a new shell marked
  "(restarted)"
- "Update now" when nothing changed relaunches the window and every tab comes back attached
- edit `~/.config/sarkos/config.toml` while the app runs ...
```

4. Replace the line "A protocol change between the window and the daemon requires restarting the daemon; the window reports the mismatch in its status banner." with "A window newer than the daemon is refused with a banner that says to run Update now; a window older than the daemon keeps working."

- [ ] **Step 6: Run everything, ten daemon runs, and the end-to-end check**

Run: `~/.cargo/bin/cargo test --workspace 2>&1 | grep "test result" && ~/.cargo/bin/cargo build --release 2>&1 | grep -c "^warning"` and the ten-run loop for the daemon suite.

The full palette-driven update needs a screen, so it stays on the README checklist. What can be recorded without one, with the release binaries from `/tmp` and logs to files: the daemon's `upgrading to` and `resumed N sessions` lines from the Task 4 suite's own daemon logs (run the suite with `--nocapture` once and copy the two lines with their timestamps), and the window's `reconnecting` and `connected` lines from Task 5's live check. Put both in the report.

- [ ] **Step 7: Commit**

```bash
git add crates/sarkos crates/sarkosd README.md
git commit -m "Notice an installed update and run it: daemon first, then relaunch the window"
```

---

## Self-review notes

Spec coverage, section by section:

- 2 and 2.1 (handover steps): Task 4 (validate, `--check-handover`, park, write, `FD_CLOEXEC`, `execv`, restore on failure). The spec's "stop accepting new clients" is implicit: the accept loop ends with the exec, and nothing between the park and the exec takes more than the write.
- 2.2 (resume): Task 4 `run_resumed`, `Session::adopt` with its `poll_exit`, the file deleted after reading. 2.3 (children by pid, PTY layer, protocol additions, socket adoption): Tasks 1, 3, 4.
- 3 (the window reconnects): Task 5, with "reconnecting" marks, the ten-second banner, the fresh daemon after ten seconds.
- 4 (end to end): Task 6: the check on launch and every five minutes (the spec's "on focus" is dropped: gpui 0.2.2 has no window-activation observer in its public API; a five-minute poll plus launch covers it, and the palette command works regardless), the status text, the palette command, daemon first then `RelaunchClient`, the daemon launching the window on disconnect, the fallback to the old window binary.
- 5 (protocol rules): Task 3 (`PROTOCOL_MIN`, append-only, `Response::Hello`, the refusal text). 6 (versions): Task 3 (`build.rs`, `version_string`). 7 (failure table): Tasks 4 and 6 (each row has a test or a log path). 8 (testing): Tasks 1, 2, 4, 5, 6. 9 (security): Task 4's `validate` and `handover::write` mode, Task 6's `RelaunchClient` check.

Rulings written into the plan: the handover's `FORMAT` gates a resume, not the writer's version; `Upgrade` answers only on failure, success is the disconnect; a version string difference (hash included) counts as an update; the "on focus" check is replaced by the five-minute poll; the first upgrade from milestone 2b still needs `pkill -x sarkosd`.

Type consistency: `pty::Pty` methods (Task 1) are what `Session` (Tasks 1, 2, 4) and `upgrade.rs` (Task 4) call; `handover::{Handover, SessionRecord, FORMAT}` (Task 2) are used by Tasks 3 (`--check-handover`), 4; `Options.socket` and `State.listener_fd` (Task 4) are used by `upgrade::execute`; `connection::start`'s `first` tuple and `Root::{connected, disconnected}` (Task 5) are what Task 6's `relaunch_now` and `update_now` build on; `version_string()` (Task 3) is used by Tasks 4 and 6.
