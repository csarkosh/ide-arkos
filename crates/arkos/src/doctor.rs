//! `arkos doctor`: what to look at when an update goes wrong, in one read-only printout. The
//! installed app and its leftovers, the daemon (asked over its socket with a Hello that names no
//! window, so it confirms nothing and is never relaunched), the update marker and the rejected
//! versions, the end of the daemon's update journal and of `window.log`, and the crash reports
//! from the last day. It starts no daemon, opens no window and changes no file.
use protocol::codec::{read_frame, write_frame};
use protocol::{ClientMessage, Request, Response, ServerMessage, PROTOCOL_VERSION};
use std::fmt::Write as _;
use std::io::{BufReader, BufWriter};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const JOURNAL_LINES: usize = 40;
pub const WINDOW_LOG_LINES: usize = 20;
const REPORTS_WITHIN: Duration = Duration::from_secs(24 * 60 * 60);
const DAEMON_TIMEOUT: Duration = Duration::from_secs(2);

/// Where everything is; the tests point it at a temporary folder.
pub struct Where {
    /// The running binary, to find the bundle it runs from.
    pub exe: Option<PathBuf>,
    /// `/Applications/Arkos IDE.app`, when the binary does not run from a bundle.
    pub default_bundle: PathBuf,
    pub app_support: PathBuf,
    pub socket: PathBuf,
    pub reports: PathBuf,
}

impl Where {
    fn real() -> Where {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        Where {
            exe: std::env::current_exe().ok(),
            default_bundle: PathBuf::from("/Applications/Arkos IDE.app"),
            app_support: crate::paths::app_support(),
            socket: crate::paths::socket_path(),
            reports: home.join("Library/Logs/DiagnosticReports"),
        }
    }

    /// The bundle the running binary is in, else the default install.
    fn bundle(&self) -> PathBuf {
        let inside = self.exe.as_deref().and_then(|e| e.parent()?.parent()?.parent()).filter(|b| b.extension().is_some_and(|x| x == "app"));
        inside.map(Path::to_path_buf).unwrap_or_else(|| self.default_bundle.clone())
    }
}

/// Prints the report; always exits 0, whatever it finds.
pub fn run() -> i32 {
    print!("{}", report(&Where::real(), SystemTime::now()));
    0
}

pub fn report(w: &Where, now: SystemTime) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{} doctor (read-only)", crate::version_string());
    if let Some(exe) = &w.exe {
        let _ = writeln!(out, "this binary: {}", exe.display());
    }
    let bundle = w.bundle();
    let installed = if bundle.is_dir() {
        crate::update::installed_version(&bundle.join("Contents/MacOS/arkos")).unwrap_or_else(|| "its arkos does not say its version".into())
    } else {
        "not found".into()
    };
    let _ = writeln!(out, "installed app: {}: {installed}", bundle.display());
    for (what, suffix) in [("previous app kept for a rollback", "app.old"), ("new app being swapped in", "app.new")] {
        let p = bundle.with_extension(suffix);
        let _ = writeln!(out, "  {}: {}", p.display(), if p.exists() { format!("present ({what})") } else { "absent".into() });
    }
    let _ = writeln!(out, "daemon on {}: {}", w.socket.display(), daemon(&w.socket));
    let updates = w.app_support.join("updates");
    let marker = updates.join("applied.json");
    let _ = writeln!(
        out,
        "update marker {}: {}",
        marker.display(),
        match std::fs::read_to_string(&marker) {
            Ok(text) => format!("present: {} (an applied update waits for a window)", text.trim()),
            Err(_) => "absent".into(),
        }
    );
    let rejected = std::fs::read_to_string(updates.join("rejected")).unwrap_or_default();
    let rejected: Vec<&str> = rejected.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let _ = writeln!(out, "rejected versions: {}", if rejected.is_empty() { "none".to_string() } else { rejected.join(", ") });
    let journal = updates.join("journal.log");
    let mut text = read_lossy(&journal.with_file_name("journal.log.1"));
    text.push_str(&read_lossy(&journal));
    section(&mut out, &format!("last {JOURNAL_LINES} lines of {}", journal.display()), &tail(&text, JOURNAL_LINES));
    let window_log = w.app_support.join("window.log");
    section(&mut out, &format!("last {WINDOW_LOG_LINES} lines of {}", window_log.display()), &tail(&read_lossy(&window_log), WINDOW_LOG_LINES));
    let reports: Vec<String> = recent_reports(&w.reports, now).into_iter().map(|p| p.display().to_string()).collect();
    section(&mut out, &format!("crash reports from the last day in {}", w.reports.display()), &reports);
    let _ = writeln!(out, "the daemon's own log: {}", w.socket.with_file_name("arkosd.log").display());
    out
}

fn section(out: &mut String, title: &str, lines: &[String]) {
    let _ = writeln!(out, "\n{title}:");
    if lines.is_empty() {
        let _ = writeln!(out, "  (none)");
    }
    for l in lines {
        let _ = writeln!(out, "  {l}");
    }
}

fn read_lossy(path: &Path) -> String {
    std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
}

/// The last `n` lines of `text`.
pub fn tail(text: &str, n: usize) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].iter().map(|l| l.to_string()).collect()
}

/// `arkos*` files in `dir` (the window's and the daemon's crash and hang reports) changed within
/// the last day, oldest first.
pub fn recent_reports(dir: &Path, now: SystemTime) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("arkos"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .filter(|(t, _)| now.duration_since(*t).map_or(true, |age| age <= REPORTS_WITHIN))
        .collect();
    found.sort();
    found.into_iter().map(|(_, p)| p).collect()
}

/// What the daemon on `socket` says it runs, its pid and its update status; "not running" when
/// nothing answers. Never starts one.
fn daemon(socket: &Path) -> String {
    match ask_daemon(socket) {
        Ok(s) => s,
        Err(e) => format!("not running ({e})"),
    }
}

fn ask_daemon(socket: &Path) -> std::io::Result<String> {
    let stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(DAEMON_TIMEOUT))?;
    stream.set_write_timeout(Some(DAEMON_TIMEOUT))?;
    let pid = peer_pid(&stream).map_or("unknown pid".to_string(), |p| format!("pid {p}"));
    let mut w = BufWriter::new(stream.try_clone()?);
    let mut r = BufReader::new(stream);
    // Pid 0 and no folder: not a window. The daemon neither watches it nor counts it as the new
    // window an applied update waits for, nor remembers a folder for it.
    let hello = Request::Hello { protocol_version: PROTOCOL_VERSION, ui_pid: 0, ui_exe: None, ui_cwd: PathBuf::new() };
    write_frame(&mut w, &ClientMessage { id: 1, request: hello })?;
    write_frame(&mut w, &ClientMessage { id: 2, request: Request::UpdateStatus })?;
    let (mut version, mut status) = (None, None);
    while version.is_none() || status.is_none() {
        match read_frame::<_, ServerMessage>(&mut r)? {
            Some(ServerMessage::Response { id: 1, response: Response::Hello { daemon_version, .. } }) => version = Some(daemon_version),
            Some(ServerMessage::Response { id: 1, response: Response::Error(e) }) => return Ok(format!("{pid}, refused this doctor's Hello: {e}")),
            Some(ServerMessage::Response { id: 2, response: Response::UpdateStatus { staged, last_error, .. } }) => {
                let staged = staged.map_or("nothing staged".to_string(), |s| format!("{} ({}) staged", s.version, s.git));
                status = Some(match last_error {
                    Some(e) => format!("{staged}; last update error: {e}"),
                    None => staged,
                });
            }
            Some(ServerMessage::Response { id: 2, response }) => status = Some(format!("update status not available ({response:?})")),
            Some(_) => continue,
            None => break,
        }
    }
    Ok(format!("{}, {pid}; {}", version.unwrap_or_else(|| "an arkosd that did not say its version".into()), status.unwrap_or_else(|| "no update status".into())))
}

/// The pid of the process at the other end of `stream`.
fn peer_pid(stream: &UnixStream) -> Option<i32> {
    let mut pid: libc::pid_t = 0;
    let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: the fd is open for the call; pid and len are valid for writes of their size.
    let rc = unsafe { libc::getsockopt(stream.as_raw_fd(), libc::SOL_LOCAL, libc::LOCAL_PEERPID, (&mut pid as *mut libc::pid_t).cast(), &mut len) };
    (rc == 0 && pid > 0).then_some(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(dir: &Path) -> Vec<(PathBuf, u64, Option<SystemTime>)> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let m = std::fs::symlink_metadata(e.path()).unwrap();
            if m.is_dir() {
                out.extend(listing(&e.path()));
            }
            out.push((e.path(), m.len(), m.modified().ok()));
        }
        out.sort();
        out
    }

    #[test]
    fn doctor_reports_everything_and_changes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let support = root.path().join("support");
        let updates = support.join("updates");
        std::fs::create_dir_all(&updates).unwrap();
        let journal: String = (0..50).map(|i| format!("2026-10-15 08:04:{:02} -0400 step {i}\n", i % 60)).collect();
        std::fs::write(updates.join("journal.log.1"), "2026-10-14 older line\n").unwrap();
        std::fs::write(updates.join("journal.log"), &journal).unwrap();
        std::fs::write(updates.join("rejected"), "0.2.4\n").unwrap();
        std::fs::write(updates.join("applied.json"), r#"{"version":"0.2.4"}"#).unwrap();
        let window: String = (0..30).map(|i| format!("window line {i}\n")).collect();
        std::fs::write(support.join("window.log"), &window).unwrap();
        let bundle = root.path().join("Arkos IDE.app");
        std::fs::create_dir_all(bundle.with_extension("app.old")).unwrap();
        let reports = root.path().join("reports");
        std::fs::create_dir_all(&reports).unwrap();
        std::fs::write(reports.join("arkos-2026-10-15-080412.ips"), "crash").unwrap();
        std::fs::write(reports.join("Safari-2026-10-15.ips"), "not ours").unwrap();
        let old = reports.join("arkosd-2026-10-01.ips");
        std::fs::write(&old, "old crash").unwrap();
        let now = SystemTime::now();
        std::fs::File::options().write(true).open(&old).unwrap().set_modified(now - Duration::from_secs(3 * 24 * 3600)).unwrap();
        let w = Where { exe: None, default_bundle: bundle.clone(), app_support: support.clone(), socket: support.join("arkosd.sock"), reports: reports.clone() };
        let before = listing(root.path());
        let text = report(&w, now);
        assert_eq!(listing(root.path()), before, "read-only");
        assert!(text.contains(&format!("installed app: {}: not found", bundle.display())), "{text}");
        assert!(text.contains("Arkos IDE.app.old: present"), "{text}");
        assert!(text.contains("Arkos IDE.app.new: absent"), "{text}");
        assert!(text.contains("arkosd.sock: not running"), "no daemon is started: {text}");
        assert!(!support.join("arkosd.sock").exists());
        assert!(text.contains(r#"present: {"version":"0.2.4"}"#), "{text}");
        assert!(text.contains("rejected versions: 0.2.4"), "{text}");
        assert!(text.contains("  2026-10-15 08:04:49 -0400 step 49\n"), "{text}");
        assert!(text.contains("step 10\n") && !text.contains("step 9\n"), "the last 40 journal lines: {text}");
        assert!(text.contains("window line 29\n") && text.contains("window line 10\n") && !text.contains("window line 9\n"), "the last 20: {text}");
        assert!(text.contains("arkos-2026-10-15-080412.ips"), "{text}");
        assert!(!text.contains("Safari") && !text.contains("arkosd-2026-10-01"), "only ours, and only from the last day: {text}");
    }

    #[test]
    fn doctor_asks_the_daemon_as_no_window() {
        use std::os::unix::net::UnixListener;
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("d.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let mut w = BufWriter::new(stream);
            let hello: ClientMessage = read_frame(&mut r).unwrap().unwrap();
            let status: ClientMessage = read_frame(&mut r).unwrap().unwrap();
            let staged = protocol::StagedUpdate { version: "0.2.6".into(), git: "abc1234".into() };
            write_frame(&mut w, &ServerMessage::Response { id: hello.id, response: Response::Hello { daemon_version: "arkosd 0.2.5 (fffffff)".into(), protocol_min: 1, protocol_max: 4 } }).unwrap();
            let answer = Response::UpdateStatus { staged: Some(staged), last_error: None, unreachable_for_secs: Some(0) };
            write_frame(&mut w, &ServerMessage::Response { id: status.id, response: answer }).unwrap();
            hello.request
        });
        let said = daemon(&socket);
        let hello = server.join().unwrap();
        assert_eq!(hello, Request::Hello { protocol_version: PROTOCOL_VERSION, ui_pid: 0, ui_exe: None, ui_cwd: PathBuf::new() }, "not a window: confirms nothing, remembers no folder");
        assert_eq!(said, format!("arkosd 0.2.5 (fffffff), pid {}; 0.2.6 (abc1234) staged", std::process::id()));
        assert!(daemon(&dir.path().join("none.sock")).starts_with("not running"));
    }

    #[test]
    fn doctor_tail_takes_the_last_lines() {
        assert_eq!(tail("a\nb\nc\n", 2), vec!["b", "c"]);
        assert_eq!(tail("a\n", 5), vec!["a"]);
        assert!(tail("", 3).is_empty());
    }

    #[test]
    fn doctor_finds_the_bundle_the_binary_runs_from() {
        let w = |exe: &str| Where { exe: Some(exe.into()), default_bundle: "/Applications/Arkos IDE.app".into(), app_support: "/s".into(), socket: "/s/a.sock".into(), reports: "/r".into() };
        assert_eq!(w("/Users/me/Apps/Arkos IDE.app/Contents/MacOS/arkos").bundle(), PathBuf::from("/Users/me/Apps/Arkos IDE.app"));
        assert_eq!(w("/x/target/debug/arkos").bundle(), PathBuf::from("/Applications/Arkos IDE.app"));
    }
}
