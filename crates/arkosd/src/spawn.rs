//! Starting a window or a shell from the daemon. macOS checks a process's permissions
//! (Accessibility, the Documents folder and the rest) against its *responsible* process, and a
//! child the daemon forks is the daemon's. A window relaunched after an update that way read
//! Accessibility as not granted although Arkos IDE had it, so a window is spawned with
//! `responsibility_spawnattrs_setdisclaim` (what iTerm2 and VS Code use) and is responsible for
//! itself: checked as Arkos IDE. A shell is not: it stays the daemon's, so a command in a tab that
//! reaches a protected folder asks once, for Arkos IDE, and the grant holds for every tab. A shell
//! disclaimed to itself was worse than it looked: Claude Code disclaims itself in turn whenever
//! its parent shell is its own responsible process, so every prompt named Claude Code and each
//! session asked again.
//!
//! Every child also gets a new session, every signal at its default and an empty mask (the daemon
//! ignores SIGPIPE, which `Command` would have reset), and only the descriptors named here
//! (`POSIX_SPAWN_CLOEXEC_DEFAULT`), so no other session's terminal leaks into it.
use std::ffi::{CString, OsString};
use std::io;
use std::os::fd::RawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::Path;

// Apple's spawn flags, which libc 0.2 may not export on every target.
const POSIX_SPAWN_SETSIGDEF: libc::c_short = 0x0004;
const POSIX_SPAWN_SETSIGMASK: libc::c_short = 0x0008;
const POSIX_SPAWN_SETSID: libc::c_short = 0x0400;
const POSIX_SPAWN_CLOEXEC_DEFAULT: libc::c_short = 0x4000;

extern "C" {
    /// libSystem's (since macOS 10.14), undocumented: the child disclaims its parent's
    /// responsibility and becomes its own responsible process.
    fn responsibility_spawnattrs_setdisclaim(attrs: *mut libc::posix_spawnattr_t, disclaim: libc::c_int) -> libc::c_int;
    fn posix_spawn_file_actions_addchdir_np(actions: *mut libc::posix_spawn_file_actions_t, path: *const libc::c_char) -> libc::c_int;
}

/// Whose permissions macOS checks the child against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Responsibility {
    /// The child's own: a window, so Accessibility is Arkos IDE's.
    Own,
    /// This process's: a shell, so a file prompt in a tab is Arkos IDE's and holds for every tab.
    Inherited,
}

/// What the child's standard streams are.
pub enum Stdio<'a> {
    /// The terminal at this path (`/dev/ttys003`), opened in the new session, so it becomes the
    /// controlling terminal, as 0, 1 and 2.
    Terminal(&'a Path),
    /// `/dev/null` as 0, and these descriptors as 1 and 2.
    Output { stdout: RawFd, stderr: RawFd },
}

/// Starts `program` (looked up on `PATH` when it has no slash) with `args` in `cwd`, this
/// process's environment with `env` set over it, `stdio` and `responsibility`; returns its pid.
/// The caller reaps it.
pub fn spawn(program: &str, args: &[&str], cwd: &Path, env: &[(String, String)], stdio: Stdio, responsibility: Responsibility) -> io::Result<libc::pid_t> {
    let c = |s: &[u8]| CString::new(s).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "an argument contains a NUL"));
    let program_c = c(program.as_bytes())?;
    let argv_c: Vec<CString> = std::iter::once(Ok(program_c.clone())).chain(args.iter().map(|a| c(a.as_bytes()))).collect::<io::Result<_>>()?;
    let envp_c: Vec<CString> = environment(env).into_iter().map(|kv| c(&kv.into_vec())).collect::<io::Result<_>>()?;
    let cwd_c = c(cwd.as_os_str().as_bytes())?;
    let null_c = c(b"/dev/null")?;
    let terminal_c = match &stdio {
        Stdio::Terminal(path) => Some(c(path.as_os_str().as_bytes())?),
        Stdio::Output { .. } => None,
    };
    let mut argv: Vec<*mut libc::c_char> = argv_c.iter().map(|s| s.as_ptr() as *mut _).collect();
    argv.push(std::ptr::null_mut());
    let mut envp: Vec<*mut libc::c_char> = envp_c.iter().map(|s| s.as_ptr() as *mut _).collect();
    envp.push(std::ptr::null_mut());

    let check = |rc: libc::c_int| if rc == 0 { Ok(()) } else { Err(io::Error::from_raw_os_error(rc)) };
    // SAFETY: the attributes and file actions are initialized before use and destroyed once on
    // every path below; every pointer passed refers to a CString or array that outlives the call.
    unsafe {
        let mut attrs: libc::posix_spawnattr_t = std::mem::zeroed();
        check(libc::posix_spawnattr_init(&mut attrs))?;
        let mut actions: libc::posix_spawn_file_actions_t = std::mem::zeroed();
        if let Err(e) = check(libc::posix_spawn_file_actions_init(&mut actions)) {
            libc::posix_spawnattr_destroy(&mut attrs);
            return Err(e);
        }
        let result = (|| {
            check(libc::posix_spawnattr_setflags(&mut attrs, POSIX_SPAWN_SETSID | POSIX_SPAWN_CLOEXEC_DEFAULT | POSIX_SPAWN_SETSIGDEF | POSIX_SPAWN_SETSIGMASK))?;
            let mut all: libc::sigset_t = std::mem::zeroed();
            libc::sigfillset(&mut all);
            check(libc::posix_spawnattr_setsigdefault(&mut attrs, &all))?;
            let mut none: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut none);
            check(libc::posix_spawnattr_setsigmask(&mut attrs, &none))?;
            if responsibility == Responsibility::Own {
                check(responsibility_spawnattrs_setdisclaim(&mut attrs, 1))?;
            }
            match (&stdio, &terminal_c) {
                (Stdio::Terminal(_), Some(tty)) => {
                    check(libc::posix_spawn_file_actions_addopen(&mut actions, 0, tty.as_ptr(), libc::O_RDWR, 0))?;
                    check(libc::posix_spawn_file_actions_adddup2(&mut actions, 0, 1))?;
                    check(libc::posix_spawn_file_actions_adddup2(&mut actions, 0, 2))?;
                }
                (Stdio::Output { stdout, stderr }, _) => {
                    check(libc::posix_spawn_file_actions_addopen(&mut actions, 0, null_c.as_ptr(), libc::O_RDONLY, 0))?;
                    check(libc::posix_spawn_file_actions_adddup2(&mut actions, *stdout, 1))?;
                    check(libc::posix_spawn_file_actions_adddup2(&mut actions, *stderr, 2))?;
                }
                (Stdio::Terminal(_), None) => unreachable!("a terminal always has its path"),
            }
            check(posix_spawn_file_actions_addchdir_np(&mut actions, cwd_c.as_ptr()))?;
            let mut pid: libc::pid_t = 0;
            check(libc::posix_spawnp(&mut pid, program_c.as_ptr(), &actions, &attrs, argv.as_ptr(), envp.as_ptr()))?;
            Ok(pid)
        })();
        libc::posix_spawn_file_actions_destroy(&mut actions);
        libc::posix_spawnattr_destroy(&mut attrs);
        result
    }
}

/// This process's environment with `env` set over it, as `KEY=value` entries.
fn environment(env: &[(String, String)]) -> Vec<OsString> {
    let mut vars: Vec<(OsString, OsString)> = std::env::vars_os().filter(|(k, _)| !env.iter().any(|(e, _)| k.as_os_str() == e.as_str())).collect();
    vars.extend(env.iter().map(|(k, v)| (k.into(), v.into())));
    vars.into_iter()
        .map(|(k, v)| {
            let mut kv = k;
            kv.push("=");
            kv.push(v);
            kv
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" {
        fn responsibility_get_pid_responsible_for_pid(pid: libc::pid_t) -> libc::pid_t;
    }

    fn wait(pid: libc::pid_t) -> i32 {
        let mut status = 0;
        // SAFETY: status is a valid out-pointer.
        unsafe { libc::waitpid(pid, &mut status, 0) };
        libc::WEXITSTATUS(status)
    }

    #[test]
    fn a_child_is_its_own_responsible_process_with_only_the_streams_given() {
        let dir = tempfile::tempdir().unwrap();
        let out = std::fs::File::create(dir.path().join("out")).unwrap();
        use std::os::fd::AsRawFd;
        let fd = out.as_raw_fd();
        // Held open and inheritable in this process: the child must not get it.
        let leak = std::fs::File::open("/dev/null").unwrap();
        // SAFETY: clears close-on-exec on a descriptor this test owns.
        unsafe { libc::fcntl(leak.as_raw_fd(), libc::F_SETFD, 0) };
        let script = format!("sleep 0.3; pwd; echo \"$ARKOS_X\"; [ -e /dev/fd/{} ] && echo leaked || echo closed; yes | head -1 >/dev/null; echo ok", leak.as_raw_fd());
        // `yes` dies quietly of SIGPIPE only when the signal is back at its default: ignored (as
        // this test binary has it), it would print "Broken pipe" to the same file.
        let pid = spawn("sh", &["-c", &script], dir.path(), &[("ARKOS_X".into(), "set".into())], Stdio::Output { stdout: fd, stderr: fd }, Responsibility::Own).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        // SAFETY: a plain query by pid.
        assert_eq!(unsafe { responsibility_get_pid_responsible_for_pid(pid) }, pid, "disclaimed");
        assert_eq!(wait(pid), 0);
        let said = std::fs::read_to_string(dir.path().join("out")).unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        assert_eq!(said, format!("{}\nset\nclosed\nok\n", cwd.display()));
    }

    #[test]
    fn a_child_can_stay_this_process_s_responsibility() {
        let pid = spawn("sh", &["-c", "sleep 0.3"], Path::new("/"), &[], Stdio::Output { stdout: 1, stderr: 2 }, Responsibility::Inherited).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        // SAFETY: plain queries by pid.
        let (child, mine) = unsafe { (responsibility_get_pid_responsible_for_pid(pid), responsibility_get_pid_responsible_for_pid(libc::getpid())) };
        assert_eq!(child, mine, "the shell answers to whoever this process answers to");
        assert_ne!(child, pid, "not to itself");
        assert_eq!(wait(pid), 0);
    }

    #[test]
    fn a_terminal_becomes_the_childs_controlling_terminal() {
        let (mut master, mut slave) = (0, 0);
        // SAFETY: valid out-pointers; the names and sizes may be null.
        assert_eq!(unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) }, 0);
        // SAFETY: slave is an open terminal; ttyname returns a static string or null.
        let path = unsafe { std::ffi::CStr::from_ptr(libc::ttyname(slave)) }.to_str().unwrap().to_string();
        let pid = spawn("/bin/sh", &["-c", "exec 3</dev/tty && echo ctty"], Path::new("/"), &[], Stdio::Terminal(Path::new(&path)), Responsibility::Inherited).unwrap();
        // SAFETY: closes this test's descriptor once.
        unsafe { libc::close(slave) };
        let mut got = Vec::new();
        let mut buf = [0u8; 256];
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !String::from_utf8_lossy(&got).contains("ctty") && std::time::Instant::now() < deadline {
            // SAFETY: buf is valid for its length.
            let n = unsafe { libc::read(master, buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                break;
            }
            got.extend_from_slice(&buf[..n as usize]);
        }
        assert!(String::from_utf8_lossy(&got).contains("ctty"), "{:?}", String::from_utf8_lossy(&got));
        assert_eq!(wait(pid), 0);
        // SAFETY: closes this test's descriptor once.
        unsafe { libc::close(master) };
    }
}
