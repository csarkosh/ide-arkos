//! Which sessions run a Claude Code, under which config folder, and whether it reports itself
//! (spec 2026-11-12, section 2). A tab's mark comes only from Claude Code's hooks, and hooks live
//! in one settings file per config folder, so a `claude` started with a `CLAUDE_CONFIG_DIR` of
//! its own may tell the daemon nothing. The probe finds it from outside: the daemon's list gives
//! each session's shell and agent record, the process table gives the shell's descendants, and a
//! process's own arguments and environment say whether it is a `claude` and where its config is.
//! It runs off the UI thread, on no timer, and reads only the user's own processes.

use protocol::{Request, Response, SessionDetail, SessionId};
use std::path::{Path, PathBuf};

/// A Claude Code found running in a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub session: SessionId,
    /// Its config folder: its `CLAUDE_CONFIG_DIR`, else the default.
    pub folder: PathBuf,
    /// Whether its SessionStart hook left an agent record. One that did not reports nothing:
    /// its folder has no hooks, or it was started before they were installed.
    pub reports: bool,
}

/// `root` and every process descended from it in `table` (pid, parent pid), nearest first.
pub fn descendants(table: &[(i32, i32)], root: i32) -> Vec<i32> {
    let mut out = vec![root];
    let mut at = 0;
    while at < out.len() {
        let parent = out[at];
        for &(pid, ppid) in table {
            if ppid == parent && pid != parent && !out.contains(&pid) {
                out.push(pid);
            }
        }
        at += 1;
    }
    out
}

/// A process's arguments and environment from its `KERN_PROCARGS2` buffer: the argument count,
/// the executable's path, padding, then each argument and each `NAME=value`, all NUL-ended.
/// `None` when the buffer ends before the arguments do; an environment cut short is what was
/// read of it.
pub fn parse_procargs(buf: &[u8]) -> Option<(Vec<String>, Vec<(String, String)>)> {
    let argc = i32::from_ne_bytes(buf.get(..4)?.try_into().ok()?);
    let mut rest = &buf[4..];
    // The executable's path, then the NULs that pad it.
    let end = rest.iter().position(|b| *b == 0)?;
    rest = &rest[end..];
    let pad = rest.iter().position(|b| *b != 0)?;
    rest = &rest[pad..];
    let mut words = rest.split(|b| *b == 0);
    let mut argv = Vec::new();
    for _ in 0..argc.max(0) {
        argv.push(String::from_utf8_lossy(words.next()?).into_owned());
    }
    // The last argument must have had its NUL, or the buffer was cut inside it.
    let after_args: usize = argv.iter().map(|a| a.len() + 1).sum();
    if after_args > rest.len() {
        return None;
    }
    let mut env = Vec::new();
    for word in words {
        if word.is_empty() {
            break;
        }
        let text = String::from_utf8_lossy(word);
        if let Some((name, value)) = text.split_once('=') {
            env.push((name.to_string(), value.to_string()));
        }
    }
    Some((argv, env))
}

/// Whether these arguments are a Claude Code: a program named `claude`, or `node` running one.
pub fn is_claude(argv: &[String]) -> bool {
    let name = |a: &String| Path::new(a).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match argv.first().map(name).as_deref() {
        Some("claude") => true,
        Some("node") => argv.get(1).is_some_and(|a| name(a) == "claude" || a.ends_with("claude-code/cli.js")),
        _ => false,
    }
}

/// A Claude Code's config folder from its environment: `CLAUDE_CONFIG_DIR` when set and not
/// empty, else `default`.
pub fn folder_of(env: &[(String, String)], default: &Path) -> PathBuf {
    match env.iter().find(|(name, _)| name == "CLAUDE_CONFIG_DIR") {
        Some((_, dir)) if !dir.is_empty() => PathBuf::from(dir),
        _ => default.to_path_buf(),
    }
}

/// The Claude Code of each live session in `list`: the first process at or under its shell
/// whose arguments `is_claude`. `args` reads a process's `KERN_PROCARGS2` buffer; a process it
/// cannot read is skipped. A session with no shell pid, an ended one, and one running no
/// `claude` give nothing.
pub fn survey(list: &[SessionDetail], table: &[(i32, i32)], args: &dyn Fn(i32) -> Option<Vec<u8>>, default: &Path) -> Vec<Seen> {
    let mut out = Vec::new();
    for d in list.iter().filter(|d| d.info.alive) {
        let Some(shell) = d.info.pid else { continue };
        let found = descendants(table, shell as i32).into_iter().find_map(|pid| {
            let (argv, env) = parse_procargs(&args(pid)?)?;
            is_claude(&argv).then(|| folder_of(&env, default))
        });
        if let Some(folder) = found {
            out.push(Seen { session: d.info.id, folder, reports: d.agent.is_some() });
        }
    }
    out
}

/// Every process's pid and parent pid. A process that ends between the two reads is left out.
pub fn process_table() -> Vec<(i32, i32)> {
    // SAFETY: both calls write only inside the buffers given, whose sizes are passed in bytes.
    unsafe {
        let count = libc::proc_listallpids(std::ptr::null_mut(), 0);
        if count <= 0 {
            return Vec::new();
        }
        // Room for processes started since the count was taken.
        let mut pids = vec![0 as libc::pid_t; count as usize + 64];
        let bytes = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
        let got = libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes);
        if got <= 0 {
            return Vec::new();
        }
        pids.truncate((got as usize).min(pids.len()));
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        pids.into_iter()
            .filter_map(|pid| {
                let mut info: libc::proc_bsdinfo = std::mem::zeroed();
                let wrote = libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&mut info as *mut libc::proc_bsdinfo).cast(), size);
                (wrote == size).then_some((pid, info.pbi_ppid as i32))
            })
            .collect()
    }
}

/// A process's `KERN_PROCARGS2` buffer, which the kernel gives for the user's own processes.
pub fn procargs(pid: i32) -> Option<Vec<u8>> {
    // SAFETY: each sysctl writes at most `size` bytes into the buffer given and says how many.
    unsafe {
        let mut argmax: libc::c_int = 0;
        let mut size = std::mem::size_of::<libc::c_int>();
        let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
        if libc::sysctl(mib.as_mut_ptr(), 2, (&mut argmax as *mut libc::c_int).cast(), &mut size, std::ptr::null_mut(), 0) != 0 || argmax <= 0 {
            return None;
        }
        let mut buf = vec![0u8; argmax as usize];
        let mut size = buf.len();
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        if libc::sysctl(mib.as_mut_ptr(), 3, buf.as_mut_ptr().cast(), &mut size, std::ptr::null_mut(), 0) != 0 {
            return None;
        }
        buf.truncate(size);
        Some(buf)
    }
}

/// The probe: the daemon's list of sessions, asked on a connection of its own (as a hook asks,
/// with no Hello, so the daemon does not take it for a window), then `survey` over the real
/// process table. `default` is the folder a Claude Code with no `CLAUDE_CONFIG_DIR` uses.
pub fn run(socket: &Path, default: &Path) -> Result<Vec<Seen>, String> {
    match crate::hook_cli::send_one(socket, Request::ListSessionsDetailed) {
        Ok(Response::SessionsDetailed(list)) => Ok(survey(&list, &process_table(), &procargs, default)),
        Ok(Response::Error(e)) => Err(format!("arkosd answered: {e}")),
        Ok(other) => Err(format!("unexpected answer {other:?}")),
        Err(e) => Err(format!("cannot reach arkosd: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{AgentRecord, ClaudeState, SessionInfo};

    /// A `KERN_PROCARGS2` buffer as the kernel lays it out.
    fn buffer(exe: &str, argv: &[&str], env: &[&str]) -> Vec<u8> {
        let mut b = (argv.len() as i32).to_ne_bytes().to_vec();
        b.extend(exe.as_bytes());
        b.extend([0, 0, 0, 0]);
        for word in argv.iter().chain(env) {
            b.extend(word.as_bytes());
            b.push(0);
        }
        b.extend([0, 0, 0]);
        b
    }

    fn detail(id: u64, pid: Option<u32>, alive: bool, agent: bool) -> SessionDetail {
        SessionDetail {
            info: SessionInfo { id: SessionId(id), title: String::new(), cwd: "/p".into(), pid, alive, last_activity_unix_ms: 0, state: ClaudeState::Idle },
            recovered: false,
            agent: agent.then(|| AgentRecord { kind: "claude".into(), id: "abc".into(), resume: vec!["claude".into(), "--resume".into(), "abc".into()] }),
        }
    }

    #[test]
    fn the_descendants_of_a_shell_are_found_through_the_parent_ids() {
        let table = [(1, 0), (10, 1), (11, 10), (12, 11), (13, 10), (20, 1), (21, 20), (0, 0)];
        assert_eq!(descendants(&table, 10), vec![10, 11, 13, 12], "itself, then nearest first");
        assert_eq!(descendants(&table, 20), vec![20, 21]);
        assert_eq!(descendants(&table, 99), vec![99], "a pid not in the table has none");
        assert_eq!(descendants(&[(5, 6), (6, 5)], 5), vec![5, 6], "a loop ends");
    }

    #[test]
    fn procargs_give_the_words_and_the_environment() {
        let buf = buffer("/Users/me/.local/share/claude/versions/2.1.0", &["claude", "--resume", "abc"], &["HOME=/Users/me", "CLAUDE_CONFIG_DIR=/Users/me/.claude3", "EMPTY=", "A=b=c"]);
        let (argv, env) = parse_procargs(&buf).unwrap();
        assert_eq!(argv, ["claude", "--resume", "abc"]);
        assert_eq!(env, [("HOME".to_string(), "/Users/me".to_string()), ("CLAUDE_CONFIG_DIR".into(), "/Users/me/.claude3".into()), ("EMPTY".into(), String::new()), ("A".into(), "b=c".into())]);
        assert_eq!(parse_procargs(&buffer("/bin/zsh", &["-zsh"], &[])).unwrap(), (vec!["-zsh".to_string()], vec![]));
        assert_eq!(parse_procargs(&[]), None);
        assert_eq!(parse_procargs(&buf[..3]), None);
        let inside_args = 4 + "/Users/me/.local/share/claude/versions/2.1.0".len() + 4 + "claude\0--res".len();
        assert_eq!(parse_procargs(&buf[..inside_args]), None, "cut inside the arguments: nothing");
        let inside_env = buf.len() - 3 - "A=b=c\0EMPTY=\0".len() - 10;
        let (argv, env) = parse_procargs(&buf[..inside_env]).unwrap();
        assert_eq!((argv.len(), env.len()), (3, 2), "cut inside the environment: what was read of it");
        assert_eq!(env[0], ("HOME".to_string(), "/Users/me".to_string()));
    }

    #[test]
    fn a_claude_is_a_program_of_that_name_or_node_running_one() {
        let v = |words: &[&str]| words.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(is_claude(&v(&["claude"])));
        assert!(is_claude(&v(&["/Users/me/.local/bin/claude", "--resume", "abc"])));
        assert!(is_claude(&v(&["node", "/opt/homebrew/bin/claude"])));
        assert!(is_claude(&v(&["/usr/local/bin/node", "/usr/local/lib/node_modules/@anthropic-ai/claude-code/cli.js"])));
        assert!(!is_claude(&v(&["-zsh"])));
        assert!(!is_claude(&v(&["node", "server.js"])));
        assert!(!is_claude(&v(&["claude-squad"])));
        assert!(!is_claude(&v(&[])));
    }

    #[test]
    fn the_folder_is_the_variable_when_set_and_not_empty() {
        let default = Path::new("/Users/me/.claude");
        let env = |pairs: &[(&str, &str)]| pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
        assert_eq!(folder_of(&env(&[("HOME", "/Users/me"), ("CLAUDE_CONFIG_DIR", "/Users/me/.claude3")]), default), Path::new("/Users/me/.claude3"));
        assert_eq!(folder_of(&env(&[("CLAUDE_CONFIG_DIR", "")]), default), default);
        assert_eq!(folder_of(&env(&[("HOME", "/Users/me")]), default), default);
    }

    #[test]
    fn a_claude_with_an_agent_record_reports_and_a_plain_shell_is_not_one() {
        let default = Path::new("/Users/me/.claude");
        // Shell 10 runs a claude under .claude3 with no record; shell 20 runs one that reported;
        // shell 30 runs vim; shell 40 has ended; session 5 has no pid.
        let table = [(10, 1), (11, 10), (20, 1), (21, 20), (22, 21), (30, 1), (31, 30), (40, 1), (41, 40)];
        let args = |pid: i32| -> Option<Vec<u8>> {
            match pid {
                10 | 20 | 30 | 40 => Some(buffer("/bin/zsh", &["-zsh"], &["HOME=/Users/me"])),
                11 => Some(buffer("/x/2.1.0", &["claude"], &["CLAUDE_CONFIG_DIR=/Users/me/.claude3"])),
                21 => Some(buffer("/x/2.1.0", &["claude"], &["HOME=/Users/me"])),
                22 => None,
                31 => Some(buffer("/usr/bin/vim", &["vim", "claude"], &[])),
                41 => Some(buffer("/x/2.1.0", &["claude"], &[])),
                _ => None,
            }
        };
        let list = [detail(1, Some(10), true, false), detail(2, Some(20), true, true), detail(3, Some(30), true, false), detail(4, Some(40), false, false), detail(5, None, true, false)];
        let seen = survey(&list, &table, &args, default);
        assert_eq!(
            seen,
            vec![
                Seen { session: SessionId(1), folder: "/Users/me/.claude3".into(), reports: false },
                Seen { session: SessionId(2), folder: default.into(), reports: true },
            ]
        );
    }

    /// The child of the test below: this test binary run under the name `claude`. macOS hides the
    /// environment of its own programs (`/bin/sleep`) from `KERN_PROCARGS2`, so the stand-in has
    /// to be a program that is not Apple's. Run any other way it returns at once.
    #[test]
    #[ignore = "a helper process for a_child_named_claude_is_found_with_its_folder"]
    fn stands_in_for_a_claude() {
        if std::env::var_os("ARKOS_PROBE_TEST_CHILD").is_some() {
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
    }

    #[test]
    fn a_child_named_claude_is_found_with_its_folder() {
        use std::os::unix::process::CommandExt;
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg0("claude")
            .args(["--exact", "agent_probe::tests::stands_in_for_a_claude", "--ignored"])
            .env("ARKOS_PROBE_TEST_CHILD", "1")
            .env("CLAUDE_CONFIG_DIR", "/tmp/some claude folder")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let me = std::process::id();
        // The child is in the table once it has been forked; its arguments and environment are
        // its own once it has been replaced by the program, a moment later.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let seen = loop {
            let seen = survey(&[detail(7, Some(me), true, false)], &process_table(), &procargs, Path::new("/default"));
            if seen.iter().any(|s| s.folder != Path::new("/default")) || std::time::Instant::now() > deadline {
                break seen;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(seen, vec![Seen { session: SessionId(7), folder: "/tmp/some claude folder".into(), reports: false }]);
        assert!(process_table().iter().any(|(pid, _)| *pid == me as i32), "the table holds this process");
    }
}
