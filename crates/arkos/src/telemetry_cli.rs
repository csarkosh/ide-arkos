//! `arkos telemetry` (telemetry spec, section 3.5): `status`, the mode in force with its reason,
//! the queue and the counts; `on`, `off` and `local`, which write `telemetry.usage` through
//! `arkos settings set`'s writer, so `on` deletes the queued session reports first and every
//! window reloads; `crashes never|ask|always`, the same for `telemetry.crashes`; and `view`, the
//! queue, or the exact bytes of the session sealed last (`--last`) or of one report in the
//! telemetry folder. `status` asks the daemon, the process that uploads, when one runs that
//! answers `TelemetryStatus`, and reads the folder itself otherwise; the doctor's telemetry row
//! comes from the same place. No gpui here, and nothing is sent. Exit codes as `agent_cli`'s: 0
//! done, 1 refused or failed, 2 usage.
use crate::agent_cli::{self, fail, usage, Env};
use ::telemetry::visits_app::{payload_of, view, Crashes, Mode, Usage};
use ::telemetry::TelemetryConfig;
use protocol::codec::{read_frame, write_frame};
use protocol::{ClientMessage, Request, Response, ServerMessage, PROTOCOL_VERSION, TELEMETRY_FROM};
use std::io::{BufReader, BufWriter, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const USAGE: &str = "\
usage: arkos telemetry status
       arkos telemetry on|off|local
       arkos telemetry view [--last | <path>]
       arkos telemetry crashes never|ask|always
see: Help > What Arkos Sends in Arkos IDE
";

/// How long `status` waits for the daemon, as the doctor does.
const DAEMON_TIMEOUT: Duration = Duration::from_secs(2);

/// The copy `seal` keeps of the session it sealed last, in the telemetry folder.
const LAST_SESSION: &str = "last-session.json";

/// The mode for the config file's `[telemetry]` table as read, or the error that kept it from
/// being read, with this process's environment and channel; the tests give their own.
pub type ModeOf<'a> = &'a dyn Fn(Result<&TelemetryConfig, &String>) -> Mode;

/// Where the commands read and write (`agent_cli`'s `Env`, whose socket folder holds the
/// telemetry folder), how the mode is found, and the time `view` reads the queue at.
pub struct Ctx<'a> {
    pub env: &'a Env<'a>,
    pub mode: ModeOf<'a>,
    pub now: u64,
}

/// Where `status` read what it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A daemon on protocol 7 or later answered `TelemetryStatus`.
    Daemon,
    /// Nothing listens on the socket.
    NoDaemon,
    /// The daemon refused the Hello or speaks a protocol before `TELEMETRY_FROM`, so it was not
    /// asked: such a daemon decodes no `TelemetryStatus` and sends no report.
    OldDaemon,
    /// The daemon accepted the connection and then gave no answer in time.
    Silent,
}

/// What `status` and the doctor show: the daemon's `TelemetryStatus` answer, or the same read
/// from the telemetry folder here, and where it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub usage: String,
    pub crashes: String,
    pub uploads: bool,
    pub reason: Option<String>,
    pub queued: u32,
    pub journals: u32,
    pub last_check_day: Option<i64>,
    pub sent: u64,
    pub rejected: u64,
    pub source: Source,
}

/// `arkos telemetry ...` with the real paths, the process's environment and its streams.
pub fn telemetry_cli(args: &[String]) -> i32 {
    let (socket, config, themes) = (crate::paths::socket_path(), crate::paths::config_file(), crate::paths::themes_dir());
    let env = Env { socket: &socket, config: &config, themes: &themes, wait: protocol::CUSTOMIZE_WAIT };
    let ctx = Ctx { env: &env, mode: &real_mode, now: unix_now() };
    telemetry_cli_to(args, &ctx, &mut std::io::stdout(), &mut std::io::stderr())
}

/// The mode with this process's environment and channel, as the daemon the app starts would
/// have it: that daemon is the process that sends, and it is interactive, so the folder read
/// with no daemon running shows the mode a daemon would run in, not this command's own.
pub fn real_mode(config: Result<&TelemetryConfig, &String>) -> Mode {
    ::telemetry::mode_for(config, true)
}

pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// `arkos telemetry ...` against `ctx`.
pub fn telemetry_cli_to(args: &[String], ctx: &Ctx, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let dir = ctx.env.telemetry_dir();
    match words.as_slice() {
        ["status"] => {
            let s = status(ctx.env.socket, &dir, &|| mode_of_file(ctx.env.config, ctx.mode), ctx.now);
            for line in status_lines(&s) {
                let _ = writeln!(out, "{line}");
            }
            0
        }
        [usage @ ("on" | "off" | "local")] => set(ctx, "telemetry.usage", usage, out, err),
        ["crashes", crashes @ ("never" | "ask" | "always")] => set(ctx, "telemetry.crashes", crashes, out, err),
        ["view"] => view_queue(&dir, ctx, out),
        ["view", "--last"] => {
            let last = dir.join(LAST_SESSION);
            // A link in its place counts as there, so the folder check below refuses it.
            if last.symlink_metadata().is_err() {
                return fail(err, "no session sealed yet");
            }
            match report_in(&dir, &last.display().to_string()) {
                Ok(file) => print_bytes(&file, out, err),
                Err(e) => fail(err, &e),
            }
        }
        ["view", path] if !path.starts_with("--") => match report_in(&dir, path) {
            Ok(file) => print_bytes(&file, out, err),
            Err(e) => fail(err, &e),
        },
        _ => usage(err, USAGE),
    }
}

/// Writes `key` through `arkos settings set`, the one writer: it checks the value and the
/// edited file, deletes the queued session reports first on a change of usage to `on`, keeps
/// the file's comments and asks every window to reload.
fn set(ctx: &Ctx, key: &str, value: &str, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    agent_cli::settings_cli_to(&["set".into(), key.into(), value.into()], ctx.env, out, err)
}

/// The mode for the config file at `path` as it is now: a missing file is the defaults, and one
/// that cannot be read is the mode's error, never the defaults.
pub fn mode_of_file(path: &Path, mode: ModeOf) -> Mode {
    let config = match std::fs::read_to_string(path) {
        Ok(text) => ::telemetry::parse_table(&text).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(TelemetryConfig::default()),
        Err(e) => Err(format!("config.toml: {e}")),
    };
    mode(config.as_ref())
}

/// The file `raw` names when it is in the telemetry folder `dir`: a bare name is a queued report
/// when the queue has one by that name, and anything else is a path as given. Both sides are
/// resolved first, so neither `..` nor a link leads out of the folder.
fn report_in(dir: &Path, raw: &str) -> Result<PathBuf, String> {
    let given = PathBuf::from(raw);
    let queued = dir.join("queue").join(&given);
    let named = if given.components().count() == 1 && queued.symlink_metadata().is_ok() { queued } else { given };
    let file = named.canonicalize().map_err(|_| format!("no such report: {raw}"))?;
    let inside = dir.canonicalize().is_ok_and(|root| file.starts_with(root));
    if !inside {
        return Err(format!("{raw} is not in the telemetry folder {}", dir.display()));
    }
    if !file.is_file() {
        return Err(format!("{raw} is not a report"));
    }
    Ok(file)
}

/// The bytes of the report at `path`, exactly as they would be sent, with nothing added.
fn print_bytes(path: &Path, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match payload_of(path) {
        Ok(bytes) => {
            let _ = out.write_all(&bytes).and_then(|()| out.flush());
            0
        }
        Err(e) => fail(err, &format!("cannot read {}: {e}", path.display())),
    }
}

/// `view` alone: one line per queued report, journal and the last session, each with its path
/// for `view <path>`.
fn view_queue(dir: &Path, ctx: &Ctx, out: &mut dyn Write) -> i32 {
    let v = view(dir, mode_of_file(ctx.env.config, ctx.mode), ctx.now);
    if v.queued.is_empty() {
        let _ = writeln!(out, "queued: none");
    }
    for (path, _, _) in &v.queued {
        let _ = writeln!(out, "queued: {}", path.display());
    }
    for path in &v.journals {
        let _ = writeln!(out, "journal: {}", path.display());
    }
    let _ = writeln!(out, "last session: {}", v.last_session.map_or("none".to_string(), |p| p.display().to_string()));
    0
}

/// What the daemon on `socket` says when it answers `TelemetryStatus`, else the telemetry
/// folder `dir` read here with `mode()` (asked only then). Nothing is written either way.
pub fn status(socket: &Path, dir: &Path, mode: &dyn Fn() -> Mode, now: u64) -> Status {
    match ask_daemon(socket) {
        Ok(s) => s,
        Err(source) => local(dir, &mode(), now, source),
    }
}

/// The folder read through the crate's `view`, as the daemon's own answer is.
fn local(dir: &Path, mode: &Mode, now: u64, source: Source) -> Status {
    let v = view(dir, mode.clone(), now);
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    Status {
        usage: usage_name(mode.usage).into(),
        crashes: crashes_name(mode.crashes).into(),
        uploads: mode.uploads,
        reason: mode.reason.map(str::to_string),
        queued: count(v.queued.len()),
        journals: count(v.journals.len()),
        last_check_day: v.state.last_check_day,
        sent: v.state.sent,
        rejected: v.state.rejected,
        source,
    }
}

fn usage_name(u: Usage) -> &'static str {
    match u {
        Usage::Off => "off",
        Usage::Local => "local",
        Usage::On => "on",
    }
}

fn crashes_name(c: Crashes) -> &'static str {
    match c {
        Crashes::Never => "never",
        Crashes::Ask => "ask",
        Crashes::Always => "always",
    }
}

/// Says Hello as no window (pid 0, no folder, as the doctor does) and waits for the answer;
/// only a daemon whose `protocol_max` reaches `TELEMETRY_FROM` is then sent `TelemetryStatus`,
/// since an older one cannot decode it and would drop the connection.
fn ask_daemon(socket: &Path) -> Result<Status, Source> {
    let stream = UnixStream::connect(socket).map_err(|_| Source::NoDaemon)?;
    let silent = |_: std::io::Error| Source::Silent;
    stream.set_read_timeout(Some(DAEMON_TIMEOUT)).map_err(silent)?;
    stream.set_write_timeout(Some(DAEMON_TIMEOUT)).map_err(silent)?;
    let mut w = BufWriter::new(stream.try_clone().map_err(silent)?);
    let mut r = BufReader::new(stream);
    let hello = Request::Hello { protocol_version: PROTOCOL_VERSION, ui_pid: 0, ui_exe: None, ui_cwd: PathBuf::new() };
    write_frame(&mut w, &ClientMessage { id: 1, request: hello }).map_err(silent)?;
    match answer(&mut r, 1)? {
        Response::Hello { protocol_max, .. } if protocol_max >= TELEMETRY_FROM => {}
        _ => return Err(Source::OldDaemon),
    }
    write_frame(&mut w, &ClientMessage { id: 2, request: Request::TelemetryStatus }).map_err(silent)?;
    match answer(&mut r, 2)? {
        Response::TelemetryStatus { usage, crashes, uploads, reason, queued, journals, last_check_day, sent, rejected } => {
            Ok(Status { usage, crashes, uploads, reason, queued, journals, last_check_day, sent, rejected, source: Source::Daemon })
        }
        _ => Err(Source::Silent),
    }
}

/// The response to request `id`, skipping events. A daemon that hangs up first is one that
/// refused the connection's protocol.
fn answer(r: &mut BufReader<UnixStream>, id: u32) -> Result<Response, Source> {
    loop {
        match read_frame::<_, ServerMessage>(r) {
            Ok(Some(ServerMessage::Response { id: got, response })) if got == id => return Ok(response),
            Ok(Some(_)) => continue,
            Ok(None) => return Err(Source::OldDaemon),
            Err(_) => return Err(Source::Silent),
        }
    }
}

/// `status`'s lines, one fact each.
pub fn status_lines(s: &Status) -> Vec<String> {
    vec![
        format!("usage reports: {}", s.usage),
        format!("crash reports: {}", s.crashes),
        format!("reason: {}", reason_words(s)),
        format!("queued reports: {}", s.queued),
        format!("queue: {}", drain_words(s)),
        format!("journals: {}", s.journals),
        format!("update check last sent: {}", s.last_check_day.map_or("never".to_string(), date_of_day)),
        format!("reports sent: {}", s.sent),
        format!("reports refused: {}", s.rejected),
        format!("read from: {}", source_words(s.source)),
    ]
}

/// Why the mode is what it is, in the words of the disclosure page: the rule that keeps
/// uploads off when one does, else what the usage setting means.
pub fn reason_words(s: &Status) -> String {
    let words = match s.reason.as_deref() {
        Some("source build") => "this copy was built from source and sends nothing",
        Some("DO_NOT_TRACK") => "DO_NOT_TRACK is set: nothing is recorded and nothing is sent",
        Some("telemetry off") => "ARKOS_TELEMETRY=off: nothing is recorded and nothing is sent",
        Some("telemetry log") => "ARKOS_TELEMETRY=log: Arkos records as your settings say and sends nothing",
        Some("CI") => "CI is set: Arkos records as your settings say and sends nothing",
        Some("not interactive") => "arkos run from a script, with no terminal, never sends and never asks",
        Some("config error") => "the config file cannot be read, so nothing is recorded or sent for that run",
        Some(other) => other,
        None => match s.usage.as_str() {
            "on" => "usage reports are on: recorded and sent",
            "off" => "usage reports are off: nothing is recorded",
            _ => "usage reports are local: recorded on this Mac, never sent",
        },
    };
    words.to_string()
}

/// Whether the queue drains: only a daemon sends, a session report only when uploads are on,
/// and a crash report when no rule keeps uploads off and crash reports are not `never` (the
/// crate's `drain`).
fn drain_words(s: &Status) -> &'static str {
    match s.source {
        Source::NoDaemon => "not draining: no daemon running",
        Source::OldDaemon => "not draining: the daemon running is older than this command",
        Source::Silent => "unknown: the daemon did not answer",
        Source::Daemon => match (s.uploads, s.reason.is_none() && s.crashes != "never") {
            (true, true) => "draining",
            (true, false) => "draining session reports only",
            (false, true) => "draining crash reports only",
            (false, false) => "not draining",
        },
    }
}

fn source_words(source: Source) -> &'static str {
    match source {
        Source::Daemon => "the daemon",
        Source::NoDaemon => "the telemetry folder, no daemon running",
        Source::OldDaemon => "the telemetry folder, the daemon running is older than this command",
        Source::Silent => "the telemetry folder, the daemon did not answer",
    }
}

/// The doctor's row: the mode, its reason, and the queue with whether it drains.
pub fn row(s: &Status) -> String {
    format!("telemetry: usage reports {}, crash reports {}; {}; {} queued, {}", s.usage, s.crashes, reason_words(s), s.queued, drain_words(s))
}

/// A UTC day number (days since 1970-01-01) as `YYYY-MM-DD`, by the civil-from-days algorithm.
pub fn date_of_day(day: i64) -> String {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::telemetry::visits_app::{Channel, Usage};
    use protocol::codec::{read_frame, write_frame};
    use protocol::{ClientMessage, Request, Response, ServerMessage, PROTOCOL_VERSION};
    use std::io::{BufReader, BufWriter};
    use std::os::unix::net::UnixListener;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    const SID: &str = "0123456789abcdef0123456789abcdef";
    const INC: &str = "fedcba9876543210fedcba9876543210";

    /// A person's interactive run of a release build with nothing in the environment.
    fn release(config: Result<&TelemetryConfig, &String>) -> Mode {
        ::telemetry::mode_with(config, &|_| None, &Channel::Release, true)
    }

    /// Runs `arkos telemetry <args>` against `dir` (the socket, the config file and the
    /// telemetry folder all inside it) and returns the exit code, stdout's bytes and stderr.
    fn run(dir: &Path, args: &[&str]) -> (i32, Vec<u8>, String) {
        run_with(dir, &dir.join("none.sock"), args)
    }

    fn run_with(dir: &Path, socket: &Path, args: &[&str]) -> (i32, Vec<u8>, String) {
        let (cfg, themes) = (dir.join("config.toml"), dir.join("themes"));
        let env = Env { socket, config: &cfg, themes: &themes, wait: Duration::from_millis(10) };
        let ctx = Ctx { env: &env, mode: &release, now: 1_760_600_000 };
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = telemetry_cli_to(&args, &ctx, &mut out, &mut err);
        (code, out, String::from_utf8(err).unwrap())
    }

    fn text(out: Vec<u8>) -> String {
        String::from_utf8(out).unwrap()
    }

    /// `<root>/telemetry` as the daemon leaves it: one session and one crash report queued, a
    /// journal running and the counts in `state.toml`.
    fn folder(root: &Path) -> PathBuf {
        let dir = root.join("telemetry");
        std::fs::create_dir_all(dir.join("queue")).unwrap();
        std::fs::create_dir_all(dir.join("journal")).unwrap();
        std::fs::write(dir.join("queue").join(format!("session-{SID}.json")), "{\"k\":\"session\"}").unwrap();
        std::fs::write(dir.join("queue").join(format!("crash-{INC}.json")), "{\"k\":\"crash\"}").unwrap();
        std::fs::write(dir.join("journal").join(format!("{SID}.jsonl")), "{}\n").unwrap();
        std::fs::write(dir.join("state.toml"), "last_check_day = 20377\nrejected = 1\nsent = 4\n").unwrap();
        dir
    }

    #[test]
    fn the_command_line_refuses_what_it_does_not_know() {
        let dir = tempfile::tempdir().unwrap();
        for args in [&[][..], &["bogus"], &["on", "now"], &["status", "--json"], &["crashes"], &["crashes", "sometimes"], &["view", "--first"], &["view", "a", "b"], &["view", "--last", "a"]] {
            let (code, out, err) = run(dir.path(), args);
            assert_eq!(code, 2, "{args:?}");
            assert!(out.is_empty() && err.starts_with("usage: arkos telemetry status\n"), "{args:?}: {err}");
        }
        assert!(!dir.path().join("config.toml").exists(), "nothing written");
    }

    #[test]
    fn on_off_local_and_crashes_write_the_key_through_the_settings_writer() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        let table = || ::telemetry::parse_table(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
        assert_eq!(run(dir.path(), &["off"]), (0, b"saved; no window running\n".to_vec(), String::new()), "a missing config is created first");
        assert_eq!(table().usage, Usage::Off);
        assert_eq!(run(dir.path(), &["local"]).0, 0);
        assert_eq!(table().usage, Usage::Local);
        assert_eq!(run(dir.path(), &["crashes", "always"]).0, 0);
        assert_eq!(table().crashes, ::telemetry::visits_app::Crashes::Always);
        assert!(std::fs::read_to_string(&cfg).unwrap().contains("# Usage reports:"), "the comments stay");
    }

    #[test]
    fn on_deletes_the_queued_sessions_first_and_keeps_the_crash_reports() {
        let dir = tempfile::tempdir().unwrap();
        let t = folder(dir.path());
        std::fs::write(t.join("queue").join(format!("session-{SID}.retry")), "attempts = 1\n").unwrap();
        let (code, out, err) = run(dir.path(), &["on"]);
        assert_eq!((code, text(out).as_str(), err.as_str()), (0, "deleted 1 queued session report recorded before usage reports were on\nsaved; no window running\n", ""));
        assert!(!t.join("queue").join(format!("session-{SID}.json")).exists() && !t.join("queue").join(format!("session-{SID}.retry")).exists());
        assert!(t.join("queue").join(format!("crash-{INC}.json")).exists(), "crash reports stay");
        assert!(t.join("journal").join(format!("{SID}.jsonl")).exists(), "a running window's journal is the window's to restart");
        let cfg = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert_eq!(::telemetry::parse_table(&cfg).unwrap().usage, Usage::On);
    }

    #[test]
    fn view_last_prints_the_exact_bytes_of_the_last_session() {
        let dir = tempfile::tempdir().unwrap();
        let (code, out, err) = run(dir.path(), &["view", "--last"]);
        assert_eq!((code, out.is_empty(), err.as_str()), (1, true, "no session sealed yet\n"));
        let t = folder(dir.path());
        let bytes = b"{\"v\":2,\"k\":\"session\"}\n\xff no newline at the end".to_vec();
        std::fs::write(t.join("last-session.json"), &bytes).unwrap();
        assert_eq!(run(dir.path(), &["view", "--last"]), (0, bytes, String::new()));
        // A link planted in its place is refused like any path outside the folder.
        let outside = dir.path().join("secret.json");
        std::fs::write(&outside, "not a report").unwrap();
        std::fs::remove_file(t.join("last-session.json")).unwrap();
        std::os::unix::fs::symlink(&outside, t.join("last-session.json")).unwrap();
        let (code, out, err) = run(dir.path(), &["view", "--last"]);
        assert_eq!((code, out.is_empty()), (1, true));
        assert!(err.contains(" is not in the telemetry folder "), "{err}");
    }

    #[test]
    fn view_prints_a_report_in_the_folder_and_refuses_a_path_outside_it() {
        let dir = tempfile::tempdir().unwrap();
        let t = folder(dir.path());
        let report = t.join("queue").join(format!("session-{SID}.json"));
        assert_eq!(run(dir.path(), &["view", report.to_str().unwrap()]), (0, b"{\"k\":\"session\"}".to_vec(), String::new()));
        assert_eq!(run(dir.path(), &["view", &format!("crash-{INC}.json")]).1, b"{\"k\":\"crash\"}".to_vec(), "a bare name is a queued report");
        let journal = t.join("journal").join(format!("{SID}.jsonl"));
        assert_eq!(run(dir.path(), &["view", journal.to_str().unwrap()]).1, b"{}\n".to_vec(), "a journal gives its lines");
        let outside = dir.path().join("secret.json");
        std::fs::write(&outside, "not a report").unwrap();
        std::os::unix::fs::symlink(&outside, t.join("queue").join("session-link.json")).unwrap();
        let sneaky = t.join("queue").join("..").join("..").join("secret.json");
        let refused = format!("{} is not in the telemetry folder {}\n", outside.display(), t.display());
        for path in [outside.clone(), t.join("queue").join("session-link.json"), sneaky] {
            let (code, out, err) = run(dir.path(), &["view", path.to_str().unwrap()]);
            assert_eq!(code, 1, "{}", path.display());
            assert!(out.is_empty(), "nothing printed for {}", path.display());
            assert!(err.contains(" is not in the telemetry folder "), "{err}");
        }
        assert_eq!(run(dir.path(), &["view", outside.to_str().unwrap()]).2, refused);
        assert_eq!(run(dir.path(), &["view", t.join("queue").to_str().unwrap()]).0, 1, "a folder is not a report");
        // A bare name is a queued report first, even when the working folder has a file of that
        // name (the tests run in the crate's folder, which has a Cargo.toml).
        assert!(Path::new("Cargo.toml").is_file());
        std::fs::write(t.join("queue").join("Cargo.toml"), "queued").unwrap();
        assert_eq!(run(dir.path(), &["view", "Cargo.toml"]), (0, b"queued".to_vec(), String::new()));
        let (code, _, err) = run(dir.path(), &["view", "session-gone.json"]);
        assert_eq!((code, err.as_str()), (1, "no such report: session-gone.json\n"));
    }

    #[test]
    fn view_alone_lists_the_queue_the_journals_and_the_last_session() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(text(run(dir.path(), &["view"]).1), "queued: none\nlast session: none\n");
        let t = folder(dir.path());
        std::fs::write(t.join("last-session.json"), "{}").unwrap();
        let want = format!(
            "queued: {}\nqueued: {}\njournal: {}\nlast session: {}\n",
            t.join("queue").join(format!("crash-{INC}.json")).display(),
            t.join("queue").join(format!("session-{SID}.json")).display(),
            t.join("journal").join(format!("{SID}.jsonl")).display(),
            t.join("last-session.json").display()
        );
        assert_eq!(run(dir.path(), &["view"]), (0, want.into_bytes(), String::new()));
    }

    #[test]
    fn status_reads_the_folder_when_no_daemon_runs() {
        let dir = tempfile::tempdir().unwrap();
        folder(dir.path());
        std::fs::write(dir.path().join("config.toml"), "[telemetry]\nusage = \"on\"\n").unwrap();
        let want = "\
usage reports: on
crash reports: ask
reason: usage reports are on: recorded and sent
queued reports: 2
queue: not draining: no daemon running
journals: 1
update check last sent: 2025-10-16
reports sent: 4
reports refused: 1
read from: the telemetry folder, no daemon running
";
        assert_eq!(run(dir.path(), &["status"]), (0, want.as_bytes().to_vec(), String::new()));
        // A config the table cannot be read from is the mode's error, never the defaults.
        std::fs::write(dir.path().join("config.toml"), "[telemetry]\nusage = \"sometimes\"\n").unwrap();
        let out = text(run(dir.path(), &["status"]).1);
        assert!(out.starts_with("usage reports: off\ncrash reports: never\nreason: the config file cannot be read, so nothing is recorded or sent for that run\n"), "{out}");
    }

    /// A fake daemon on `socket` that answers the Hello with `protocol_max` and, when asked,
    /// `TelemetryStatus` with `answer`; it returns every request it received.
    fn fake_daemon(socket: &Path, protocol_max: u32, answer: Response) -> std::thread::JoinHandle<Vec<Request>> {
        let listener = UnixListener::bind(socket).unwrap();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let mut w = BufWriter::new(stream);
            let mut got = Vec::new();
            while let Ok(Some(msg)) = read_frame::<_, ClientMessage>(&mut r) {
                let response = match &msg.request {
                    Request::Hello { .. } => Response::Hello { daemon_version: "arkosd 0.5.0 (fffffff)".into(), protocol_min: 1, protocol_max },
                    Request::TelemetryStatus => answer.clone(),
                    _ => Response::Error("unexpected".into()),
                };
                got.push(msg.request);
                write_frame(&mut w, &ServerMessage::Response { id: msg.id, response }).unwrap();
            }
            got
        })
    }

    #[test]
    fn status_asks_a_protocol_7_daemon_and_never_sends_the_request_to_a_protocol_6_one() {
        let dir = tempfile::tempdir().unwrap();
        folder(dir.path());
        let hello = Request::Hello { protocol_version: PROTOCOL_VERSION, ui_pid: 0, ui_exe: None, ui_cwd: PathBuf::new() };
        let answer = Response::TelemetryStatus {
            usage: "local".into(),
            crashes: "always".into(),
            uploads: false,
            reason: None,
            queued: 3,
            journals: 2,
            last_check_day: None,
            sent: 0,
            rejected: 0,
        };
        let sock = dir.path().join("seven.sock");
        let server = fake_daemon(&sock, 7, answer.clone());
        let want = "\
usage reports: local
crash reports: always
reason: usage reports are local: recorded on this Mac, never sent
queued reports: 3
queue: draining crash reports only
journals: 2
update check last sent: never
reports sent: 0
reports refused: 0
read from: the daemon
";
        assert_eq!(run_with(dir.path(), &sock, &["status"]), (0, want.as_bytes().to_vec(), String::new()));
        assert_eq!(server.join().unwrap(), vec![hello.clone(), Request::TelemetryStatus], "a Hello that names no window, then the question");
        let sock = dir.path().join("six.sock");
        let server = fake_daemon(&sock, 6, answer);
        let out = text(run_with(dir.path(), &sock, &["status"]).1);
        assert_eq!(server.join().unwrap(), vec![hello], "a protocol 6 daemon is never sent TelemetryStatus");
        assert!(out.contains("\nqueued reports: 2\n"), "read from the folder: {out}");
        assert!(out.contains("\nqueue: not draining: the daemon running is older than this command\n"), "{out}");
        assert!(out.ends_with("\nread from: the telemetry folder, the daemon running is older than this command\n"), "{out}");
    }

    /// Every reason `status` can name is said in the words of the disclosure page.
    #[test]
    fn the_reasons_are_said_in_the_words_of_the_page() {
        // The page's lines wrap, so its words are compared with single spaces between them.
        let page = ::telemetry::disclosure::PAGE.split_whitespace().collect::<Vec<_>>().join(" ");
        let said = |config: Result<&TelemetryConfig, &String>, env: &[(&str, &str)], channel: Channel, interactive: bool| {
            let read = |k: &str| env.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
            let mode = ::telemetry::mode_with(config, &read, &channel, interactive);
            let dir = tempfile::tempdir().unwrap();
            reason_words(&status(&dir.path().join("none.sock"), dir.path(), &|| mode.clone(), 0))
        };
        let ok = TelemetryConfig::default();
        let cases: [(String, &str); 9] = [
            (said(Ok(&ok), &[], Channel::Source, true), "this copy was built from source and sends nothing"),
            (said(Ok(&ok), &[("DO_NOT_TRACK", "1")], Channel::Release, true), "DO_NOT_TRACK is set: nothing is recorded and nothing is sent"),
            (said(Ok(&ok), &[("ARKOS_TELEMETRY", "off")], Channel::Release, true), "ARKOS_TELEMETRY=off: nothing is recorded and nothing is sent"),
            (said(Ok(&ok), &[("ARKOS_TELEMETRY", "log")], Channel::Release, true), "ARKOS_TELEMETRY=log: Arkos records as your settings say and sends nothing"),
            (said(Ok(&ok), &[("CI", "true")], Channel::Release, true), "CI is set: Arkos records as your settings say and sends nothing"),
            (said(Ok(&ok), &[], Channel::Release, false), "arkos run from a script, with no terminal, never sends and never asks"),
            (said(Err(&"bad".to_string()), &[], Channel::Release, true), "the config file cannot be read, so nothing is recorded or sent for that run"),
            (said(Ok(&TelemetryConfig { usage: Usage::Off, ..ok }), &[], Channel::Release, true), "usage reports are off: nothing is recorded"),
            (said(Ok(&ok), &[], Channel::Release, true), "usage reports are local: recorded on this Mac, never sent"),
        ];
        for (got, want) in &cases {
            assert_eq!(got, want);
        }
        for phrase in ["built from source", "sends nothing", "nothing is recorded and nothing is sent", "records as your settings say and sends nothing", "run from a script, with no terminal, never sends and never asks", "for that run", "recorded on this Mac, never sent"] {
            assert!(page.contains(phrase), "the page says {phrase:?}");
        }
    }

    #[test]
    fn the_doctor_row_says_the_mode_its_reason_and_whether_the_queue_drains() {
        let base = Status {
            usage: "on".into(),
            crashes: "ask".into(),
            uploads: true,
            reason: None,
            queued: 2,
            journals: 1,
            last_check_day: Some(20377),
            sent: 4,
            rejected: 0,
            source: Source::Daemon,
        };
        assert_eq!(row(&base), "telemetry: usage reports on, crash reports ask; usage reports are on: recorded and sent; 2 queued, draining");
        let never = Status { crashes: "never".into(), ..base.clone() };
        assert_eq!(row(&never), "telemetry: usage reports on, crash reports never; usage reports are on: recorded and sent; 2 queued, draining session reports only");
        let dnt = Status { usage: "off".into(), crashes: "never".into(), uploads: false, reason: Some("DO_NOT_TRACK".into()), queued: 1, ..base.clone() };
        assert_eq!(row(&dnt), "telemetry: usage reports off, crash reports never; DO_NOT_TRACK is set: nothing is recorded and nothing is sent; 1 queued, not draining");
        let alone = Status { usage: "local".into(), uploads: false, queued: 0, source: Source::NoDaemon, ..base.clone() };
        assert_eq!(row(&alone), "telemetry: usage reports local, crash reports ask; usage reports are local: recorded on this Mac, never sent; 0 queued, not draining: no daemon running");
        let silent = Status { source: Source::Silent, ..base };
        assert!(row(&silent).ends_with("; 2 queued, unknown: the daemon did not answer"), "{}", row(&silent));
    }

    #[test]
    fn a_day_number_is_its_utc_date() {
        assert_eq!(date_of_day(0), "1970-01-01");
        assert_eq!(date_of_day(20377), "2025-10-16");
        assert_eq!(date_of_day(11016), "2000-02-29");
        assert_eq!(date_of_day(-1), "1969-12-31");
    }
}
