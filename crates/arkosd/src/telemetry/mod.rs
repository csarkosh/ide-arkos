//! The daemon's telemetry (telemetry spec, section 5): its folder, the envelope it sends, the
//! crate's state file around every call that changes it, and the mode read afresh from the
//! config file. The daemon is the only process that uploads and the only one that seals. It
//! routes the first update check of each UTC day through the first-party path (`check`); the
//! check needs no consent, so the mode does not decide it, and a `source` build never makes it
//! (`Check::next`). It seals a window's journal when the window's connection ends (`seal`), and
//! every 15 minutes seals what was left behind and posts the queue (`upload`).
pub mod check;
pub mod seal;
pub mod upload;

use ::telemetry::visits_app::{view, Check, Envelope, Mode, State};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The collector's base: the check goes to `<base>/a/check`.
pub const DEFAULT_URL: &str = "https://apps.csarko.sh";

/// Seconds in a UTC day, the crate's day.
const DAY: u64 = 86_400;

/// The daemon's telemetry arguments (`--telemetry-dir` and `--telemetry-url`, see `main.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// The crate's folder: `<socket dir>/telemetry` unless told otherwise.
    pub dir: PathBuf,
    /// Test only: the collector's base, `DEFAULT_URL` or a `file://` folder (`allowed_url`).
    pub url: String,
    /// The config file whose `[telemetry]` table gives the mode: always `config_file()`, which a
    /// resumed daemon works out again, so no flag sets it and the exec does not carry it. A unit
    /// test points it into its temp folder.
    pub config: PathBuf,
}

impl Options {
    pub fn defaults(socket: &Path) -> Self {
        Self { dir: socket.parent().unwrap_or(Path::new("/")).join("telemetry"), url: DEFAULT_URL.into(), config: ::telemetry::config_file() }
    }

    /// Where the reports are posted: the collector's `<base>/a`, so the host is the configured
    /// base and nothing a report or a response says can move it.
    pub fn reports_url(&self) -> String {
        format!("{}/a", self.url.strip_suffix('/').unwrap_or(&self.url))
    }

    /// The arguments that give a resumed daemon these options: only those that differ from the
    /// defaults, as `UpdateOptions::args` does.
    pub fn args(&self, socket: &Path) -> Vec<String> {
        let d = Self::defaults(socket);
        let mut out = Vec::new();
        if self.dir != d.dir {
            out.extend(["--telemetry-dir".into(), self.dir.to_string_lossy().into_owned()]);
        }
        if self.url != d.url {
            out.extend(["--telemetry-url".into(), self.url.clone()]);
        }
        out
    }
}

/// Whether `--telemetry-url` may name `url`: the collector itself or a `file://` folder (the
/// tests' sites), so no flag can send the check to another host. A trailing `/` is ignored.
pub fn allowed_url(url: &str) -> bool {
    let url = url.strip_suffix('/').unwrap_or(url);
    url == DEFAULT_URL || url.starts_with("file:///")
}

/// What the poll thread needs for the day's check. It lives on that thread.
#[derive(Debug)]
pub struct Telemetry {
    pub dir: PathBuf,
    pub env: Envelope,
    /// The check's endpoint, `<base>/a/check`, which `check::route` gives the query.
    pub check_url: String,
    /// The day of the last first-party attempt this process made, so a state file that cannot
    /// be saved still costs no more than one check a day.
    attempted: Cell<Option<u64>>,
}

impl Telemetry {
    /// The daemon's own envelope (`bin` is `daemon`; the channel is its bundle's).
    pub fn new(o: &Options) -> Self {
        Self::with_env(o, ::telemetry::envelope(::telemetry::Bin::Daemon))
    }

    /// With `env` given, so a test can be a `release` build or a `source` one.
    pub fn with_env(o: &Options, env: Envelope) -> Self {
        let base = o.url.strip_suffix('/').unwrap_or(&o.url);
        Self { dir: o.dir.clone(), env, check_url: format!("{base}/a/check"), attempted: Cell::new(None) }
    }

    /// The day's check when the update fetch about to happen is it, from the state file loaded
    /// just before: `None` on a day that had one, and always for a `source` build.
    pub fn next_check(&self, now: u64) -> Option<Check> {
        if self.attempted.get() == Some(now / DAY) {
            return None;
        }
        Check::next(&self.env, &State::load(&self.dir), now)
    }

    /// Records that the day's check was made at `now`, whatever it answered: the state file is
    /// loaded just before and saved just after. A state that cannot be saved is logged, and the
    /// day is still remembered in memory.
    pub fn checked(&self, now: u64) {
        self.attempted.set(Some(now / DAY));
        if let Err(e) = update_state(&self.dir, |s| s.checked(now)) {
            eprintln!("arkosd: cannot save the telemetry state: {e}");
        }
    }
}

/// Keeps this process's changes to the state file one at a time. The crate loads the file, hands
/// it over and saves it through a temporary file named after the process, so two threads of the
/// daemon (a window's seal and the upload loop, or the day's check) would share that file and
/// could undo each other's counts; every change in the daemon goes through `update_state`.
static STATE_FILE: Mutex<()> = Mutex::new(());

/// `State::update` in `dir`, one thread of the daemon at a time.
pub fn update_state<R>(dir: &Path, f: impl FnOnce(&mut State) -> R) -> std::io::Result<R> {
    // The lock guards no data, so a panic while it was held leaves nothing inconsistent.
    let _one = STATE_FILE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    State::update(dir, f)
}

/// The mode for this run, from the `[telemetry]` table of `config` read afresh, so a change
/// applies without a restart: the upload loop asks before every drain, and `TelemetryStatus`
/// before every answer. The daemon is started by a window, so it is interactive. The check
/// needs no consent and never reads it.
///
/// The daemon parses only the `[telemetry]` table, with toml 1 (`parse_table`), while the app
/// parses the whole file with its own reader, toml 0.8, so the two can disagree when the app
/// refuses the file (a mistake in another section, or something only one of the two parsers
/// accepts). The app runs `Off` only for a file it refused at launch, and then for the whole
/// run: it records nothing, so nothing new reaches the queue. A reload it refuses keeps the
/// settings and the journal it had, while the daemon follows the table as written. Should that
/// table turn usage `On`, the window does not restart its journal, which is why the consent
/// check (`upload::consent_check`) judges every journal by its start, a connected window's too.
pub fn mode_now(config: &Path) -> Mode {
    ::telemetry::mode_for(table_now(config).as_ref(), true)
}

/// The `[telemetry]` table of `config` read afresh, before any environment rule: what the
/// person wrote. The mode is made from it (`mode_now`), and so is the consent check.
pub fn table_now(config: &Path) -> Result<::telemetry::TelemetryConfig, String> {
    config_of(std::fs::read_to_string(config))
}

/// The answer to `TelemetryStatus`: the mode as given and the folder as it stands at `now`,
/// through the crate's `view`, so the command line shows what is on disk.
pub fn status(dir: &Path, mode: Mode, now: u64) -> protocol::Response {
    let v = view(dir, mode, now);
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    protocol::Response::TelemetryStatus {
        usage: ::telemetry::usage_name(v.mode.usage).into(),
        crashes: ::telemetry::crashes_name(v.mode.crashes).into(),
        uploads: v.mode.uploads,
        reason: v.mode.reason.map(str::to_string),
        queued: count(v.queued.len()),
        journals: count(v.journals.len()),
        last_check_day: v.state.last_check_day,
        sent: v.state.sent,
        rejected: v.state.rejected,
    }
}

/// The `[telemetry]` table of the config file as read: a missing file is the defaults, and an
/// unreadable one is an error, which the mode turns into `Off` for the run.
fn config_of(read: std::io::Result<String>) -> Result<::telemetry::TelemetryConfig, String> {
    match read {
        Ok(text) => ::telemetry::parse_table(&text).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(::telemetry::TelemetryConfig::default()),
        Err(e) => Err(format!("config.toml: {e}")),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use ::telemetry::visits_app::{Arch, Channel, Crashes, Os, Usage};

    pub fn release_env() -> Envelope {
        Envelope { app: "arkos".into(), ver: "0.5.0".into(), bin: "daemon".into(), os: Os::Macos, osv: 15, arch: Arch::Arm64, ch: Channel::Release }
    }

    fn telemetry(dir: &Path, ch: Channel) -> Telemetry {
        let o = Options { dir: dir.to_path_buf(), url: DEFAULT_URL.into(), config: dir.join("config.toml") };
        Telemetry::with_env(&o, Envelope { ch, ..release_env() })
    }

    #[test]
    fn the_folder_follows_the_socket_and_only_a_changed_option_rides_the_exec() {
        let socket = Path::new("/s/arkosd.sock");
        let d = Options::defaults(socket);
        assert_eq!(d, Options { dir: PathBuf::from("/s/telemetry"), url: DEFAULT_URL.into(), config: ::telemetry::config_file() });
        assert!(d.args(socket).is_empty(), "a release passes none, so an older daemon can resume");
        let o = Options { dir: PathBuf::from("/t/tel"), url: "file:///t/site".into(), config: PathBuf::from("/t/config.toml") };
        assert_eq!(o.args(socket), ["--telemetry-dir", "/t/tel", "--telemetry-url", "file:///t/site"]);
    }

    #[test]
    fn the_url_flag_takes_only_the_collector_or_a_file_folder() {
        assert!(allowed_url("https://apps.csarko.sh"));
        assert!(allowed_url("https://apps.csarko.sh/"));
        assert!(allowed_url("file:///tmp/site"));
        for bad in ["https://evil.example", "https://apps.csarko.sh.evil.example", "https://apps.csarko.sh@evil.example", "http://apps.csarko.sh", "https://apps.csarko.sh:8443", "https://apps.csarko.sh/elsewhere", "file://host/tmp", ""] {
            assert!(!allowed_url(bad), "{bad}");
        }
    }

    #[test]
    fn the_check_goes_to_the_collectors_check_path() {
        let o = Options { dir: PathBuf::from("/t"), url: "https://apps.csarko.sh/".into(), config: PathBuf::from("/t/config.toml") };
        let t = Telemetry::with_env(&o, release_env());
        assert_eq!(t.check_url, "https://apps.csarko.sh/a/check");
        // `Telemetry::new` reads the envelope, whose `osv` runs `sw_vers` the first time: under the
        // spawn lock, so no other test's fork inherits a descriptor it holds open.
        let t = {
            let _no_fork = crate::pty::spawn_lock();
            Telemetry::new(&Options { dir: PathBuf::from("/t"), url: "file:///t/site".into(), config: PathBuf::from("/t/config.toml") })
        };
        assert_eq!(t.check_url, "file:///t/site/a/check");
        assert_eq!(t.env.bin, "daemon");
        assert_eq!(::telemetry::user_agent(), format!("arkos/{}", t.env.ver), "the check's user agent names the envelope's version");
    }

    #[test]
    fn a_check_is_due_once_a_day_with_the_days_since_the_last() {
        let dir = tempfile::tempdir().unwrap();
        let t = telemetry(dir.path(), Channel::Release);
        let day = 20_000 * DAY;
        assert_eq!(t.next_check(day + 10).map(|c| c.rd), Some(-1), "the first ever");
        t.checked(day + 10);
        assert_eq!(State::load(dir.path()).last_check_day, Some(20_000));
        assert_eq!(t.next_check(day + DAY - 1), None, "the same UTC day");
        // A new process, which remembers only the file.
        let t = telemetry(dir.path(), Channel::Release);
        assert_eq!(t.next_check(day + 50), None);
        assert_eq!(t.next_check(day + 3 * DAY).map(|c| c.rd), Some(3));
    }

    #[test]
    fn a_source_build_never_has_a_check() {
        let dir = tempfile::tempdir().unwrap();
        let t = telemetry(dir.path(), Channel::Source);
        assert_eq!(t.next_check(20_000 * DAY), None);
        assert!(!dir.path().join("state.toml").exists());
    }

    #[test]
    fn a_state_that_cannot_be_saved_still_costs_one_check_a_day() {
        let dir = tempfile::tempdir().unwrap();
        // A file where the folder should be: the state can be neither read nor saved.
        let blocked = dir.path().join("telemetry");
        std::fs::write(&blocked, b"").unwrap();
        let t = telemetry(&blocked, Channel::Release);
        let day = 20_000 * DAY;
        assert!(t.next_check(day).is_some());
        t.checked(day);
        assert_eq!(t.next_check(day + 60), None, "remembered in memory");
        assert!(t.next_check(day + DAY).is_some(), "and due again the next day");
    }

    #[test]
    fn the_config_table_as_read_gives_the_mode() {
        let env = |_: &str| None;
        let mode = |read| ::telemetry::mode_with(config_of(read).as_ref(), &env, &Channel::Release, true);
        let missing = || Err(std::io::Error::from(std::io::ErrorKind::NotFound));
        assert_eq!(config_of(missing()), Ok(::telemetry::TelemetryConfig::default()), "no file is the defaults");
        assert_eq!(mode(missing()).usage, Usage::Local);
        let on = mode(Ok("[telemetry]\nusage = \"on\"\ncrashes = \"always\"\n".into()));
        assert_eq!((on.usage, on.crashes), (Usage::On, Crashes::Always));
        let unreadable = mode(Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied)));
        assert_eq!((unreadable.usage, unreadable.crashes), (Usage::Off, Crashes::Never), "an unreadable file is a config error");
        let broken = mode(Ok("[telemetry]\nusage = \"sometimes\"\n".into()));
        assert_eq!(broken.usage, Usage::Off);
    }

    #[test]
    fn reports_go_to_the_collectors_report_path() {
        let mut o = Options::defaults(Path::new("/s/arkosd.sock"));
        assert_eq!(o.reports_url(), "https://apps.csarko.sh/a");
        o.url = "https://apps.csarko.sh/".into();
        assert_eq!(o.reports_url(), "https://apps.csarko.sh/a");
        o.url = "file:///t/site".into();
        assert_eq!(o.reports_url(), "file:///t/site/a");
    }

    #[test]
    fn the_mode_is_read_from_the_config_file_each_time() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        // A test binary is never in a release bundle, so the reason is always the source rule.
        assert_eq!(mode_now(&config).reason, Some("source build"));
        assert!(!mode_now(&config).uploads);
        std::fs::write(&config, "[telemetry]\nusage = \"sometimes\"\n").unwrap();
        assert_eq!((mode_now(&config).usage, mode_now(&config).crashes), (Usage::Off, Crashes::Never), "a config error is Off, never the defaults");
    }

    /// The status is the mode as given and the folder as it is: queued reports, journals not yet
    /// sealed, the day of the last check and the counts.
    #[test]
    fn the_status_answers_from_the_folder_and_the_mode() {
        let dir = tempfile::tempdir().unwrap();
        let no_env = |_: &str| None;
        let on = ::telemetry::TelemetryConfig { usage: Usage::On, crashes: Crashes::Always };
        let mode = ::telemetry::mode_with(Ok::<_, ()>(&on), &no_env, &Channel::Release, true);
        let empty = protocol::Response::TelemetryStatus {
            usage: "on".into(),
            crashes: "always".into(),
            uploads: true,
            reason: None,
            queued: 0,
            journals: 0,
            last_check_day: None,
            sent: 0,
            rejected: 0,
        };
        assert_eq!(status(dir.path(), mode.clone(), 0), empty, "an empty or missing folder is all zeros");
        let now = 20_000 * DAY;
        let allow = ::telemetry::allowlist().clone();
        ::telemetry::visits_app::Journal::open(dir.path(), "0123456789abcdef0123456789abcdef", &release_env(), allow.clone(), now).unwrap();
        ::telemetry::visits_app::Journal::open(dir.path(), "fedcba9876543210fedcba9876543210", &release_env(), allow, now).unwrap();
        update_state(dir.path(), |s| ::telemetry::visits_app::seal(dir.path(), "fedcba9876543210fedcba9876543210", ::telemetry::visits_app::End::Quit, now + 60, s)).unwrap().unwrap();
        update_state(dir.path(), |s| {
            s.checked(now);
            s.sent = 9;
            s.rejected = 1;
        })
        .unwrap();
        let env = |k: &str| (k == "DO_NOT_TRACK").then(|| "1".to_string());
        let dnt = ::telemetry::mode_with(Ok::<_, ()>(&on), &env, &Channel::Release, true);
        let want = protocol::Response::TelemetryStatus {
            usage: "off".into(),
            crashes: "never".into(),
            uploads: false,
            reason: Some("DO_NOT_TRACK".into()),
            queued: 1,
            journals: 1,
            last_check_day: Some(20_000),
            sent: 9,
            rejected: 1,
        };
        assert_eq!(status(dir.path(), dnt, now), want);
    }
}
