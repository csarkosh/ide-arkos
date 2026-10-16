//! The daemon's telemetry (telemetry spec, section 5): its folder, the envelope it sends, the
//! crate's state file around every call that changes it, and the mode read afresh from the
//! config file. The daemon is the only process that uploads. For now it routes the first update
//! check of each UTC day through the first-party path (`check`); the check needs no consent, so
//! the mode does not decide it, and a `source` build never makes it (`Check::next`).
pub mod check;

use ::telemetry::visits_app::{Check, Envelope, Mode, State};
use std::cell::Cell;
use std::path::{Path, PathBuf};

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
}

impl Options {
    pub fn defaults(socket: &Path) -> Self {
        Self { dir: socket.parent().unwrap_or(Path::new("/")).join("telemetry"), url: DEFAULT_URL.into() }
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

    /// `arkos/<version>`, the user agent the collector requires of the check.
    pub fn user_agent(&self) -> String {
        format!("arkos/{}", self.env.ver)
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
        if let Err(e) = State::update(&self.dir, |s| s.checked(now)) {
            eprintln!("arkosd: cannot save the telemetry state in {}: {e}", self.dir.display());
        }
    }
}

/// The mode for this run, from the config file's `[telemetry]` table read afresh, so a change
/// applies without a restart. The daemon is started by a window, so it is interactive. Nothing
/// calls it yet: the upload loop will, before every drain. The check needs no consent and never
/// reads it.
#[allow(dead_code)]
pub fn mode_now() -> Mode {
    let config = config_of(std::fs::read_to_string(::telemetry::config_file()));
    ::telemetry::mode_for(config.as_ref(), true)
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
        let o = Options { dir: dir.to_path_buf(), url: DEFAULT_URL.into() };
        Telemetry::with_env(&o, Envelope { ch, ..release_env() })
    }

    #[test]
    fn the_folder_follows_the_socket_and_only_a_changed_option_rides_the_exec() {
        let socket = Path::new("/s/arkosd.sock");
        let d = Options::defaults(socket);
        assert_eq!(d, Options { dir: PathBuf::from("/s/telemetry"), url: DEFAULT_URL.into() });
        assert!(d.args(socket).is_empty(), "a release passes none, so an older daemon can resume");
        let o = Options { dir: PathBuf::from("/t/tel"), url: "file:///t/site".into() };
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
    fn the_check_goes_to_the_collectors_check_path_with_the_arkos_user_agent() {
        let o = Options { dir: PathBuf::from("/t"), url: "https://apps.csarko.sh/".into() };
        let t = Telemetry::with_env(&o, release_env());
        assert_eq!(t.check_url, "https://apps.csarko.sh/a/check");
        assert_eq!(t.user_agent(), "arkos/0.5.0");
        let t = Telemetry::new(&Options { dir: PathBuf::from("/t"), url: "file:///t/site".into() });
        assert_eq!(t.check_url, "file:///t/site/a/check");
        assert_eq!(t.env.bin, "daemon");
        assert_eq!(t.user_agent(), format!("arkos/{}", env!("CARGO_PKG_VERSION")));
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
}
