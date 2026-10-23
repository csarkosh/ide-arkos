//! Starting a window from the daemon as its own responsible process. macOS checks a process's
//! permissions (Accessibility, the Documents folder and the rest) against its *responsible*
//! process, and a child the daemon forks is the daemon's: a window it relaunched after an update
//! that way read Accessibility as not granted although Arkos IDE had it. `posix_spawn` with
//! `responsibility_spawnattrs_setdisclaim` (what iTerm2 and VS Code use) makes the window
//! responsible for itself, so it is checked as Arkos IDE. A tab's shell is started by `pty.rs`
//! instead: it answers to the daemon, and it needs a controlling terminal, which `posix_spawn`
//! cannot give it.
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

/// Starts `program` (looked up on `PATH` when it has no slash) with `args` in `cwd`, this
/// process's environment with `env` set over it, `/dev/null` as its input and `stdout` and
/// `stderr` as its outputs, responsible for itself; returns its pid. The caller reaps it.
pub fn spawn(program: &str, args: &[&str], cwd: &Path, env: &[(String, String)], stdout: RawFd, stderr: RawFd) -> io::Result<libc::pid_t> {
    let c = |s: &[u8]| CString::new(s).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "an argument contains a NUL"));
    let program_c = c(program.as_bytes())?;
    let argv_c: Vec<CString> = std::iter::once(Ok(program_c.clone())).chain(args.iter().map(|a| c(a.as_bytes()))).collect::<io::Result<_>>()?;
    let envp_c: Vec<CString> = environment(env).into_iter().map(|kv| c(&kv.into_vec())).collect::<io::Result<_>>()?;
    let cwd_c = c(cwd.as_os_str().as_bytes())?;
    let null_c = c(b"/dev/null")?;
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
            check(responsibility_spawnattrs_setdisclaim(&mut attrs, 1))?;
            check(libc::posix_spawn_file_actions_addopen(&mut actions, 0, null_c.as_ptr(), libc::O_RDONLY, 0))?;
            check(libc::posix_spawn_file_actions_adddup2(&mut actions, stdout, 1))?;
            check(libc::posix_spawn_file_actions_adddup2(&mut actions, stderr, 2))?;
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
        let pid = spawn("sh", &["-c", &script], dir.path(), &[("ARKOS_X".into(), "set".into())], fd, fd).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        // SAFETY: a plain query by pid.
        assert_eq!(unsafe { responsibility_get_pid_responsible_for_pid(pid) }, pid, "disclaimed");
        assert_eq!(wait(pid), 0);
        let said = std::fs::read_to_string(dir.path().join("out")).unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        assert_eq!(said, format!("{}\nset\nclosed\nok\n", cwd.display()));
    }
}
