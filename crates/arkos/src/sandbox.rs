//! `arkos sandbox`: a second Arkos beside the one in use, with its own config folder, its own
//! daemon and its own state, so a person or their agent can try a theme or an effect without
//! touching the real setup (the sandbox design). Every process, signal and process lookup goes
//! through `Host`, so `sandbox_cli_to` is tested with a fake one and never starts anything.

use crate::launch;
use ::telemetry::visits_app::Channel;
use serde_json::{json, Value as Json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The first words `arkos sandbox` hands to the ordinary command with the sandbox's variables.
pub const COMMANDS: &[&str] = &["open", "video", "panes", "theme", "effect", "settings", "config", "guide", "doctor", "telemetry", "hooks", "notify"];

pub const SANDBOX_USAGE: &str = "usage: arkos sandbox [<folder>]\n       arkos sandbox <command> ...   any arkos command, aimed at the sandbox\n       arkos sandbox status | stop | reset\nsee: arkos guide\n";

/// What a sandbox (`ARKOS_SANDBOX` set) says where an action would write the user's own setup:
/// their Claude Code colours, hooks or skill, the login item, or an update. The action does nothing.
pub const REFUSAL: &str = "not in a sandbox";

/// The line that ends `status`: what a sandbox never does.
const PROMISE: &str = "sends nothing; Claude Code colours untouched";

/// How long `terminate` waits after SIGTERM before SIGKILL, and after SIGKILL before giving up.
const TERM_WAIT: Duration = Duration::from_secs(3);
const KILL_WAIT: Duration = Duration::from_secs(1);

/// The sandbox's folders: the root under the caches folder, the config folder Arkos reads as
/// `ARKOS_CONFIG_DIR`, and the state folder it reads as `ARKOS_APP_SUPPORT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    pub root: PathBuf,
    pub config: PathBuf,
    pub state: PathBuf,
}

/// One folder per build, so the installed app's sandbox and a dev build's never share a daemon.
/// Only a source build is the dev one: a dogfooded copy is the installed app on the maintainer's
/// Mac and shares the installed app's sandbox.
pub fn dirs(home: &Path, channel: &Channel) -> Dirs {
    let name = match channel {
        Channel::Source => "arkos-sandbox-dev",
        _ => "arkos-sandbox",
    };
    let root = home.join("Library/Caches").join(name);
    Dirs { config: root.join("config"), state: root.join("state"), root }
}

/// The variables a sandbox process gets, and the names taken out of the environment it inherits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxEnv {
    pub set: Vec<(String, String)>,
    pub remove: Vec<String>,
}

/// Whether a variable of the calling environment stays out of the sandbox: the session's own
/// markers, and every `CLAUDE` variable but the sign-in token, so a `claude` started inside the
/// sandbox does not take itself for a child of the agent that opened it.
fn stripped(name: &str) -> bool {
    name == "ARKOS_SESSION_ID" || name == "ARKOS_RELAUNCH" || (name.starts_with("CLAUDE") && name != "CLAUDE_CODE_OAUTH_TOKEN")
}

/// The sandbox's environment for `dirs`, with `remove` listing the names of `current` that
/// `stripped` matches, in `current`'s order.
pub fn env(dirs: &Dirs, current: &[(String, String)]) -> SandboxEnv {
    let set = vec![
        ("ARKOS_CONFIG_DIR".to_string(), dirs.config.to_string_lossy().into_owned()),
        ("ARKOS_APP_SUPPORT".to_string(), dirs.state.to_string_lossy().into_owned()),
        ("ARKOS_SANDBOX".to_string(), "1".to_string()),
        ("ARKOS_TELEMETRY".to_string(), "off".to_string()),
    ];
    let remove = current.iter().filter(|(k, _)| stripped(k)).map(|(k, _)| k.clone()).collect();
    SandboxEnv { set, remove }
}

/// Takes `env`'s removals out of `cmd`'s environment and sets its variables.
pub fn apply(cmd: &mut Command, env: &SandboxEnv) {
    for name in &env.remove {
        cmd.env_remove(name);
    }
    for (k, v) in &env.set {
        cmd.env(k, v);
    }
}

/// Copies the folder `from` into `to`, recursively.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Makes the sandbox's folders and, when its config folder holds no `config.toml` yet, copies the
/// user's `config.toml`, `themes/` and `effects/` from `real_config` into it. The real files are
/// only read. `config.toml` goes last, so a copy cut short is tried again on the next start.
/// True when there was no `config.toml` to keep.
pub fn first_start(dirs: &Dirs, real_config: &Path) -> std::io::Result<bool> {
    let file = dirs.config.join("config.toml");
    let fresh = !file.exists();
    if fresh {
        for sub in ["themes", "effects"] {
            let from = real_config.join(sub);
            if from.is_dir() {
                copy_tree(&from, &dirs.config.join(sub))?;
            }
        }
    }
    for folder in [dirs.config.join("themes"), dirs.config.join("effects"), dirs.state.clone()] {
        std::fs::create_dir_all(folder)?;
    }
    let real_file = real_config.join("config.toml");
    if fresh && real_file.is_file() {
        std::fs::copy(real_file, file)?;
    }
    Ok(fresh)
}

/// Everything `arkos sandbox` does outside its own folders: processes, signals and lookups.
pub trait Host {
    /// Runs `exe args` with `env` applied and the streams inherited; its exit code.
    fn run(&mut self, exe: &Path, args: &[String], env: &SandboxEnv) -> i32;
    /// Starts a window on `folder` in its own session, its output to `log`; its pid.
    fn spawn_window(&mut self, exe: &Path, folder: &Path, env: &SandboxEnv, log: &Path) -> std::io::Result<u32>;
    /// The sandbox's windows and their projects, from `panes --all --json`; an error when the
    /// daemon does not answer.
    fn windows(&mut self, exe: &Path, env: &SandboxEnv) -> Result<Vec<(u32, PathBuf)>, String>;
    /// The pids of the daemon whose command line is `arkosd --socket <socket>`.
    fn daemon_pids(&mut self, socket: &Path) -> Vec<u32>;
    /// SIGTERM, up to three seconds, then SIGKILL; true when the process is gone.
    fn terminate(&mut self, pid: u32) -> bool;
    /// This process's environment.
    fn current_vars(&self) -> Vec<(String, String)>;
}

/// What `sandbox_cli_to` reads of the world: the home folder, the running build's channel, the
/// executable, the user's real config folder and the working directory.
pub struct Ctx<'a> {
    pub home: &'a Path,
    pub channel: Channel,
    pub exe: &'a Path,
    pub real_config: &'a Path,
    pub cwd: &'a Path,
}

/// `arkos sandbox ...` against `ctx` and `host`. Exit codes: 0 done, 1 failed or refused, 2 usage,
/// and a forwarded command's own.
pub fn sandbox_cli_to(args: &[String], ctx: &Ctx, host: &mut dyn Host, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let dirs = dirs(ctx.home, &ctx.channel);
    let env = env(&dirs, &host.current_vars());
    let rest = args.get(1..).unwrap_or(&[]);
    match args.first().map(String::as_str) {
        Some("status") => match only_json(rest) {
            Some(json) => status(&dirs, &env, ctx, host, json, out),
            None => usage(err),
        },
        Some("stop") if rest.is_empty() => stop(&dirs, &env, ctx, host, out, err),
        Some("reset") if rest.is_empty() => reset(&dirs, &env, ctx, host, out, err),
        Some("stop") | Some("reset") => usage(err),
        Some(word) if COMMANDS.contains(&word) => host.run(ctx.exe, args, &env),
        _ => open(args, &dirs, &env, ctx, host, out, err),
    }
}

/// `Some(true)` for `--json` alone, `Some(false)` for nothing, `None` for anything else.
fn only_json(args: &[String]) -> Option<bool> {
    match args {
        [] => Some(false),
        [flag] if flag == "--json" => Some(true),
        _ => None,
    }
}

fn usage(err: &mut dyn Write) -> i32 {
    let _ = err.write_all(SANDBOX_USAGE.as_bytes());
    2
}

fn fail(err: &mut dyn Write, text: &str) -> i32 {
    let _ = writeln!(err, "sandbox: {text}");
    1
}

/// `arkos sandbox [<folder>] [--json]`: the first start, then a window on the folder.
fn open(args: &[String], dirs: &Dirs, env: &SandboxEnv, ctx: &Ctx, host: &mut dyn Host, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let json = args.iter().any(|a| a == "--json");
    let words: Vec<&String> = args.iter().filter(|a| *a != "--json").collect();
    let folder = match words.as_slice() {
        [] => ctx.cwd.to_path_buf(),
        [word] if !word.starts_with("--") => ctx.cwd.join(word),
        _ => return usage(err),
    };
    if !folder.is_dir() {
        return usage(err);
    }
    let folder = std::fs::canonicalize(&folder).unwrap_or(folder);
    if let Err(e) = first_start(dirs, ctx.real_config) {
        return fail(err, &format!("could not make {}: {e}", dirs.root.display()));
    }
    let pid = match host.spawn_window(ctx.exe, &folder, env, &dirs.state.join("window.log")) {
        Ok(pid) => pid,
        Err(e) => return fail(err, &format!("could not start the window: {e}")),
    };
    if json {
        let _ = writeln!(out, "{}", json!({ "folder": folder, "config": dirs.config, "state": dirs.state, "pid": pid }));
    } else {
        let _ = writeln!(out, "sandbox: opened {} (config {}, state {})", folder.display(), dirs.config.display(), dirs.state.display());
    }
    0
}

/// The socket the sandbox's daemon listens on, as `paths::socket_path` names it for the state
/// folder.
fn socket(dirs: &Dirs) -> PathBuf {
    dirs.state.join("arkosd.sock")
}

/// `arkos sandbox status`. A daemon with no window answers no `panes --all`, so its process
/// running counts as the daemon running too.
fn status(dirs: &Dirs, env: &SandboxEnv, ctx: &Ctx, host: &mut dyn Host, json: bool, out: &mut dyn Write) -> i32 {
    let windows = host.windows(ctx.exe, env);
    let daemon = windows.is_ok() || !host.daemon_pids(&socket(dirs)).is_empty();
    let windows = windows.unwrap_or_default();
    if json {
        let list: Vec<Json> = windows.iter().map(|(pid, project)| json!({ "pid": pid, "project": project })).collect();
        let _ = writeln!(out, "{}", json!({ "config": dirs.config, "state": dirs.state, "daemon": daemon, "windows": list }));
        return 0;
    }
    let _ = writeln!(out, "config: {}", dirs.config.display());
    let _ = writeln!(out, "state: {}", dirs.state.display());
    let _ = writeln!(out, "daemon: {}", if daemon { "running" } else { "not running" });
    if windows.is_empty() {
        let _ = writeln!(out, "windows: none");
    }
    for (pid, project) in &windows {
        let _ = writeln!(out, "window {pid} ({})", project.display());
    }
    let _ = writeln!(out, "{PROMISE}");
    0
}

/// `arkos sandbox stop`: the windows first, since a window restarts a daemon that goes away
/// under it, then the daemon. The folders stay.
fn stop(dirs: &Dirs, env: &SandboxEnv, ctx: &Ctx, host: &mut dyn Host, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let windows = host.windows(ctx.exe, env).unwrap_or_default();
    let mut stuck = Vec::new();
    for (pid, _) in &windows {
        if !host.terminate(*pid) {
            stuck.push(*pid);
        }
    }
    let daemons = host.daemon_pids(&socket(dirs));
    for pid in &daemons {
        if !host.terminate(*pid) {
            stuck.push(*pid);
        }
    }
    if !stuck.is_empty() {
        for pid in stuck {
            fail(err, &format!("process {pid} would not end"));
        }
        return 1;
    }
    let n = windows.len();
    let line = match (n, daemons.is_empty()) {
        (0, true) => "sandbox: nothing running".to_string(),
        (_, true) => format!("sandbox: stopped {n} {}", plural(n)),
        (_, false) => format!("sandbox: stopped {n} {} and the daemon", plural(n)),
    };
    let _ = writeln!(out, "{line}");
    0
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        "window"
    } else {
        "windows"
    }
}

/// `arkos sandbox reset`: removes the sandbox's folder whole, unless a window or the daemon runs.
fn reset(dirs: &Dirs, env: &SandboxEnv, ctx: &Ctx, host: &mut dyn Host, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let windows = host.windows(ctx.exe, env).unwrap_or_default();
    if !windows.is_empty() || !host.daemon_pids(&socket(dirs)).is_empty() {
        return fail(err, "still running; run arkos sandbox stop first");
    }
    match std::fs::remove_dir_all(&dirs.root) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return fail(err, &format!("could not remove {}: {e}", dirs.root.display())),
    }
    let _ = writeln!(out, "sandbox: reset; the next start copies your config again");
    0
}

/// The windows in `arkos panes --all --json`'s output: each element's `window` pid and `project`
/// (empty for a window that did not answer). The `error` of a single object otherwise.
fn parse_windows(stdout: &[u8]) -> Result<Vec<(u32, PathBuf)>, String> {
    let value: Json = serde_json::from_slice(stdout).map_err(|e| format!("panes --all answered no JSON: {e}"))?;
    match value {
        Json::Array(items) => Ok(items
            .iter()
            .filter_map(|w| {
                let pid = u32::try_from(w["window"].as_u64()?).ok()?;
                Some((pid, PathBuf::from(w["project"].as_str().unwrap_or(""))))
            })
            .collect()),
        other => Err(other["error"].as_str().unwrap_or("panes --all answered something else").to_string()),
    }
}

/// The real host: child processes and signals.
struct Processes;

impl Host for Processes {
    fn run(&mut self, exe: &Path, args: &[String], env: &SandboxEnv) -> i32 {
        let mut cmd = Command::new(exe);
        cmd.args(args);
        apply(&mut cmd, env);
        match cmd.status() {
            Ok(status) => status.code().unwrap_or(1),
            Err(e) => {
                eprintln!("sandbox: could not run {}: {e}", exe.display());
                1
            }
        }
    }

    fn spawn_window(&mut self, exe: &Path, folder: &Path, env: &SandboxEnv, log: &Path) -> std::io::Result<u32> {
        let log = std::fs::OpenOptions::new().create(true).append(true).open(log)?;
        let mut cmd = launch::window_command(exe, folder);
        apply(&mut cmd, env);
        cmd.current_dir(folder).stdout(log.try_clone()?).stderr(log);
        // The window is in its own session and outlives this command, which exits at once.
        Ok(cmd.spawn()?.id())
    }

    fn windows(&mut self, exe: &Path, env: &SandboxEnv) -> Result<Vec<(u32, PathBuf)>, String> {
        let mut cmd = Command::new(exe);
        cmd.args(["panes", "--all", "--json"]).stdin(Stdio::null()).stderr(Stdio::null());
        apply(&mut cmd, env);
        let output = cmd.output().map_err(|e| e.to_string())?;
        parse_windows(&output.stdout)
    }

    fn daemon_pids(&mut self, socket: &Path) -> Vec<u32> {
        let pattern = format!("arkosd --socket {}", socket.display());
        let Ok(output) = Command::new("pgrep").arg("-f").arg(pattern).stdin(Stdio::null()).stderr(Stdio::null()).output() else {
            return Vec::new();
        };
        String::from_utf8_lossy(&output.stdout).lines().filter_map(|l| l.trim().parse().ok()).collect()
    }

    fn terminate(&mut self, pid: u32) -> bool {
        let pid = pid as libc::pid_t;
        let alive = || unsafe { libc::kill(pid, 0) } == 0;
        let wait = |limit: Duration| {
            let until = Instant::now() + limit;
            while alive() && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(100));
            }
            !alive()
        };
        // SAFETY: kill only sends a signal; a pid that is gone answers ESRCH.
        unsafe { libc::kill(pid, libc::SIGTERM) };
        if wait(TERM_WAIT) {
            return true;
        }
        unsafe { libc::kill(pid, libc::SIGKILL) };
        wait(KILL_WAIT)
    }

    fn current_vars(&self) -> Vec<(String, String)> {
        std::env::vars_os().map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned())).collect()
    }
}

/// `arkos sandbox ...` with the real host. The real config folder is resolved with
/// `ARKOS_CONFIG_DIR` left out, so `arkos sandbox` run from a sandbox shell still copies from the
/// user's own config and opens the same sandbox.
pub fn sandbox_cli(args: &[String]) -> i32 {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => return fail(&mut std::io::stderr(), &format!("could not find this executable: {e}")),
    };
    let real_config = ::telemetry::config_dir_from(None, std::env::var_os("XDG_CONFIG_HOME").as_deref(), &home);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let ctx = Ctx { home: &home, channel: ::telemetry::channel(), exe: &exe, real_config: &real_config, cwd: &cwd };
    sandbox_cli_to(args, &ctx, &mut Processes, &mut std::io::stdout(), &mut std::io::stderr())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records every call and answers from its fields.
    struct FakeHost {
        windows: Result<Vec<(u32, PathBuf)>, String>,
        daemons: Vec<u32>,
        vars: Vec<(String, String)>,
        run_code: i32,
        /// The pids `terminate` cannot end.
        stuck: Vec<u32>,
        ran: Vec<(PathBuf, Vec<String>, SandboxEnv)>,
        spawned: Vec<(PathBuf, PathBuf, SandboxEnv, PathBuf)>,
        sockets: Vec<PathBuf>,
        terminated: Vec<u32>,
    }

    impl FakeHost {
        fn new() -> Self {
            FakeHost {
                windows: Err("no window running".into()),
                daemons: Vec::new(),
                vars: vec![("ARKOS_SESSION_ID".into(), "7".into()), ("CLAUDECODE".into(), "1".into()), ("HOME".into(), "/h".into())],
                run_code: 0,
                stuck: Vec::new(),
                ran: Vec::new(),
                spawned: Vec::new(),
                sockets: Vec::new(),
                terminated: Vec::new(),
            }
        }
    }

    impl Host for FakeHost {
        fn run(&mut self, exe: &Path, args: &[String], env: &SandboxEnv) -> i32 {
            self.ran.push((exe.to_path_buf(), args.to_vec(), env.clone()));
            self.run_code
        }

        fn spawn_window(&mut self, exe: &Path, folder: &Path, env: &SandboxEnv, log: &Path) -> std::io::Result<u32> {
            self.spawned.push((exe.to_path_buf(), folder.to_path_buf(), env.clone(), log.to_path_buf()));
            Ok(4242)
        }

        fn windows(&mut self, _exe: &Path, _env: &SandboxEnv) -> Result<Vec<(u32, PathBuf)>, String> {
            self.windows.clone()
        }

        fn daemon_pids(&mut self, socket: &Path) -> Vec<u32> {
            self.sockets.push(socket.to_path_buf());
            self.daemons.clone()
        }

        fn terminate(&mut self, pid: u32) -> bool {
            self.terminated.push(pid);
            !self.stuck.contains(&pid)
        }

        fn current_vars(&self) -> Vec<(String, String)> {
            self.vars.clone()
        }
    }

    const EXE: &str = "/bin/arkos";

    /// Runs `arkos sandbox <args>` for the release build with `home` and `cwd`.
    fn call(args: &[&str], home: &Path, cwd: &Path, host: &mut FakeHost) -> (i32, String, String) {
        let exe = PathBuf::from(EXE);
        let real = home.join(".config/arkos");
        let ctx = Ctx { home, channel: Channel::Release, exe: &exe, real_config: &real, cwd };
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = sandbox_cli_to(&args, &ctx, host, &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    #[test]
    fn the_sandbox_folder_is_per_build_and_holds_config_and_state() {
        let d = dirs(Path::new("/Users/me"), &Channel::Release);
        assert_eq!(d.root, PathBuf::from("/Users/me/Library/Caches/arkos-sandbox"));
        assert_eq!((d.config, d.state), (d.root.join("config"), d.root.join("state")));
        assert_eq!(dirs(Path::new("/Users/me"), &Channel::Source).root, PathBuf::from("/Users/me/Library/Caches/arkos-sandbox-dev"));
        assert_eq!(
            dirs(Path::new("/Users/me"), &Channel::Named(::telemetry::DOGFOOD.into())).root,
            PathBuf::from("/Users/me/Library/Caches/arkos-sandbox"),
            "a dogfooded copy is the installed app"
        );
    }

    #[test]
    fn the_environment_sets_arkos_s_own_variables_and_strips_the_session_and_claude_s_markers() {
        let d = dirs(Path::new("/h"), &Channel::Release);
        let current = vec![
            ("ARKOS_SESSION_ID".into(), "7".into()),
            ("CLAUDECODE".into(), "1".into()),
            ("CLAUDE_CODE_OAUTH_TOKEN".into(), "t".into()),
            ("XDG_CONFIG_HOME".into(), "/x".into()),
            ("ARKOS_RELAUNCH".into(), "1".into()),
        ];
        let e = env(&d, &current);
        assert_eq!(
            e.set,
            vec![
                ("ARKOS_CONFIG_DIR".to_string(), "/h/Library/Caches/arkos-sandbox/config".to_string()),
                ("ARKOS_APP_SUPPORT".to_string(), "/h/Library/Caches/arkos-sandbox/state".to_string()),
                ("ARKOS_SANDBOX".to_string(), "1".to_string()),
                ("ARKOS_TELEMETRY".to_string(), "off".to_string()),
            ]
        );
        assert_eq!(e.remove, vec!["ARKOS_SESSION_ID".to_string(), "CLAUDECODE".to_string(), "ARKOS_RELAUNCH".to_string()]);
        assert!(!e.set.iter().any(|(k, _)| k == "XDG_CONFIG_HOME"), "other programs keep their config");
    }

    #[test]
    fn the_first_start_copies_the_real_config_once_and_makes_empty_folders_when_there_is_none() {
        let t = tempfile::tempdir().unwrap();
        let real = t.path().join("real");
        std::fs::create_dir_all(real.join("themes")).unwrap();
        std::fs::write(real.join("config.toml"), "[theme]\ndark = \"navy\"\n").unwrap();
        std::fs::write(real.join("themes/mine.toml"), "name = \"mine\"\n").unwrap();
        let d = dirs(t.path(), &Channel::Release);
        assert!(first_start(&d, &real).unwrap());
        assert_eq!(std::fs::read_to_string(d.config.join("config.toml")).unwrap(), "[theme]\ndark = \"navy\"\n");
        assert!(d.config.join("themes/mine.toml").is_file() && d.config.join("effects").is_dir() && d.state.is_dir());
        std::fs::write(d.config.join("config.toml"), "changed").unwrap();
        assert!(!first_start(&d, &real).unwrap(), "a config already there is kept");
        assert_eq!(std::fs::read_to_string(d.config.join("config.toml")).unwrap(), "changed");
        let none = dirs(&t.path().join("other"), &Channel::Release);
        assert!(first_start(&none, &t.path().join("missing")).unwrap());
        assert!(none.config.join("themes").is_dir() && none.config.join("effects").is_dir() && !none.config.join("config.toml").exists());
    }

    #[test]
    fn a_folder_opens_a_window_a_command_word_is_forwarded_and_anything_else_is_usage() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        let cwd = t.path().join("work");
        std::fs::create_dir_all(cwd.join("proj")).unwrap();
        let d = dirs(&home, &Channel::Release);

        // A word in COMMANDS is forwarded, nothing is copied first, and its code comes back.
        let mut host = FakeHost::new();
        host.run_code = 5;
        assert_eq!(call(&["theme", "use", "x"], &home, &cwd, &mut host), (5, String::new(), String::new()));
        let words: Vec<String> = ["theme", "use", "x"].iter().map(|w| w.to_string()).collect();
        assert_eq!(host.ran, vec![(PathBuf::from(EXE), words, env(&d, &host.vars))]);
        assert!(host.spawned.is_empty() && !d.root.exists(), "a forwarded command copies nothing");

        // A folder: the first start, then the window with the env and the log.
        let (code, out, err) = call(&["proj"], &home, &cwd, &mut host);
        assert_eq!(code, 0, "{err}");
        let folder = std::fs::canonicalize(cwd.join("proj")).unwrap();
        let log = d.state.join("window.log");
        assert_eq!(host.spawned, vec![(PathBuf::from(EXE), folder.clone(), env(&d, &host.vars), log.clone())]);
        assert_eq!(out, format!("sandbox: opened {} (config {}, state {})\n", folder.display(), d.config.display(), d.state.display()));
        assert!(d.config.join("themes").is_dir() && d.state.is_dir(), "the first start ran before the window");

        // `--json` prints the folders and the window's pid.
        let (code, out, _) = call(&["proj", "--json"], &home, &cwd, &mut host);
        assert_eq!(code, 0);
        let v: Json = serde_json::from_str(&out).unwrap();
        assert_eq!(v, json!({ "folder": folder, "config": d.config, "state": d.state, "pid": 4242 }));

        // Not a command and not a folder: usage.
        let mut host = FakeHost::new();
        assert_eq!(call(&["frobnicate"], &home, &cwd, &mut host), (2, String::new(), SANDBOX_USAGE.to_string()));
        assert_eq!(call(&["proj", "--frob"], &home, &cwd, &mut host), (2, String::new(), SANDBOX_USAGE.to_string()));
        assert!(host.spawned.is_empty() && host.ran.is_empty());

        // No word: the current folder.
        let (code, _, _) = call(&[], &home, &cwd, &mut host);
        assert_eq!(code, 0);
        assert_eq!(host.spawned[0].1, std::fs::canonicalize(&cwd).unwrap());
    }

    #[test]
    fn status_names_the_folders_the_daemon_and_the_windows() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().to_path_buf();
        let d = dirs(&home, &Channel::Release);
        let c = d.config.display();
        let s = d.state.display();

        let mut host = FakeHost::new();
        host.windows = Ok(vec![(41, PathBuf::from("/p"))]);
        let (code, out, err) = call(&["status"], &home, &home, &mut host);
        assert_eq!((code, err.as_str()), (0, ""));
        assert_eq!(out, format!("config: {c}\nstate: {s}\ndaemon: running\nwindow 41 (/p)\nsends nothing; Claude Code colours untouched\n"));
        let (code, out, _) = call(&["status", "--json"], &home, &home, &mut host);
        assert_eq!(code, 0);
        let v: Json = serde_json::from_str(&out).unwrap();
        assert_eq!(v, json!({ "config": d.config, "state": d.state, "daemon": true, "windows": [{ "pid": 41, "project": "/p" }] }));

        let mut host = FakeHost::new();
        let (code, out, _) = call(&["status"], &home, &home, &mut host);
        assert_eq!(code, 0);
        assert_eq!(out, format!("config: {c}\nstate: {s}\ndaemon: not running\nwindows: none\nsends nothing; Claude Code colours untouched\n"));
        let (_, out, _) = call(&["status", "--json"], &home, &home, &mut host);
        let v: Json = serde_json::from_str(&out).unwrap();
        assert_eq!(v, json!({ "config": d.config, "state": d.state, "daemon": false, "windows": [] }));

        // A daemon with no window answers no `panes --all`; its process still says it runs.
        host.daemons = vec![9];
        let (_, out, _) = call(&["status"], &home, &home, &mut host);
        assert!(out.contains("\ndaemon: running\nwindows: none\n"), "{out}");
        assert_eq!(host.sockets.last(), Some(&d.state.join("arkosd.sock")));
    }

    #[test]
    fn stop_ends_the_windows_before_the_daemon_and_reports_the_counts() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().to_path_buf();
        let d = dirs(&home, &Channel::Release);

        let mut host = FakeHost::new();
        host.windows = Ok(vec![(41, PathBuf::from("/a")), (42, PathBuf::from("/b"))]);
        host.daemons = vec![9];
        assert_eq!(call(&["stop"], &home, &home, &mut host), (0, "sandbox: stopped 2 windows and the daemon\n".to_string(), String::new()));
        assert_eq!(host.terminated, vec![41, 42, 9]);
        assert_eq!(host.sockets, vec![d.state.join("arkosd.sock")]);

        let mut host = FakeHost::new();
        assert_eq!(call(&["stop"], &home, &home, &mut host), (0, "sandbox: nothing running\n".to_string(), String::new()));
        assert!(host.terminated.is_empty());

        let mut host = FakeHost::new();
        host.windows = Ok(vec![(41, PathBuf::from("/a")), (42, PathBuf::from("/b"))]);
        host.daemons = vec![9];
        host.stuck = vec![42];
        assert_eq!(call(&["stop"], &home, &home, &mut host), (1, String::new(), "sandbox: process 42 would not end\n".to_string()));
        assert_eq!(host.terminated, vec![41, 42, 9], "the rest are still ended");
    }

    #[test]
    fn reset_refuses_while_the_sandbox_runs_and_otherwise_removes_the_folder() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().to_path_buf();
        let d = dirs(&home, &Channel::Release);
        std::fs::create_dir_all(&d.config).unwrap();
        std::fs::write(d.config.join("config.toml"), "x").unwrap();

        let mut host = FakeHost::new();
        host.daemons = vec![9];
        assert_eq!(call(&["reset"], &home, &home, &mut host), (1, String::new(), "sandbox: still running; run arkos sandbox stop first\n".to_string()));
        assert!(d.config.join("config.toml").is_file(), "the folder is kept");

        let mut host = FakeHost::new();
        host.windows = Ok(vec![(41, PathBuf::from("/a"))]);
        assert_eq!(call(&["reset"], &home, &home, &mut host).0, 1, "a window counts as running");

        let mut host = FakeHost::new();
        assert_eq!(call(&["reset"], &home, &home, &mut host), (0, "sandbox: reset; the next start copies your config again\n".to_string(), String::new()));
        assert!(!d.root.exists());
        assert_eq!(call(&["reset"], &home, &home, &mut host).0, 0, "a sandbox never started resets too");
    }

    #[test]
    fn the_windows_are_read_from_the_panes_all_json() {
        let text = br#"[{"window":41,"project":"/p","panes":[]},{"window":42,"error":"timed out"}]"#;
        assert_eq!(parse_windows(text), Ok(vec![(41, PathBuf::from("/p")), (42, PathBuf::new())]));
        assert_eq!(parse_windows(br#"{"error":"no window running","exit":3}"#), Err("no window running".to_string()));
        assert!(parse_windows(b"").is_err());
    }
}
