//! The upload loop (telemetry spec, section 2.2, "Uploading"): every 15 minutes, on a thread of
//! its own, the daemon reads the mode afresh, seals the journals left behind by windows no longer
//! connected, and posts what the mode lets leave through the system curl. The crate's
//! `Queue::drain` decides what leaves and what a status means; this module gives it the
//! transport and the folder.
use ::telemetry::visits_app::{seal_stale_except, Mode, Outcome, Queue, Transport, Usage};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

/// How often the queue is posted and the journals left behind are sealed.
pub const EVERY: Duration = Duration::from_secs(15 * 60);

/// How long a journal of a window no longer connected must have been quiet before the loop seals
/// it as `unknown`: a day, so a window whose connection is only being re-made keeps its own.
pub const STALE_SECS: u64 = 86_400;

/// How long one post may take, curl's own `--max-time`.
pub const POST_SECS: u64 = 10;

/// The arguments of one post: https only, at most `POST_SECS`, the body from stdin, and only the
/// status written back. No `--fail`, so a refusal is a status the crate reads rather than an
/// error, and no `--location`, so a redirect is never followed off the collector.
pub fn curl_args(url: &str, user_agent: &str) -> Vec<String> {
    let mut args: Vec<String> = ["--disable", "--silent", "--show-error", "--proto", "=https", "--max-time"].map(String::from).into();
    args.push(POST_SECS.to_string());
    args.extend(["--user-agent", user_agent, "-H", "Content-Type: application/json", "--data-binary", "@-", "-o", "/dev/null", "-w", "%{http_code}", "--", url].map(String::from));
    args
}

/// The transport the crate's drain posts through: the configured curl (`--curl`).
pub struct Curl {
    pub curl: PathBuf,
}

impl Transport for Curl {
    fn post(&self, url: &str, user_agent: &str, body: &[u8]) -> Result<u16, String> {
        let mut cmd = Command::new(&self.curl);
        cmd.args(curl_args(url, user_agent));
        // curl's own --max-time ends it first; this is the backstop should it hang regardless.
        let out = crate::updates::run_bounded_input(&mut cmd, Some(body.to_vec()), Duration::from_secs(POST_SECS + 10)).map_err(|e| format!("curl: {e}"))?;
        if !out.status.success() {
            return Err(format!("curl: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        // `000` is curl saying no response came, which is an error and not a status.
        match String::from_utf8_lossy(&out.stdout).trim().parse::<u16>() {
            Ok(status) if (100..=599).contains(&status) => Ok(status),
            _ => Err("curl: no status".into()),
        }
    }
}

/// Opt-in's clean start for the journals this loop seals: when usage was not `On` at the last
/// pass (`previous`) and is now, deletes the journal of every window not among `connected`, so
/// nothing recorded before the consent is sealed and sent. A connected window restarts its own
/// journal under a new id when it sees the change (`config_watch`). With `since`, the moment
/// usage turned on, a journal written after it is kept: a window that launched under `On` and has
/// not sent its `Session` yet is not connected by sid, and its journal holds nothing from before
/// the consent. Returns how many it deleted.
pub fn forget_before_consent(dir: &Path, previous: Usage, usage: Usage, connected: &[&str], since: Option<SystemTime>) -> usize {
    if previous == Usage::On || usage != Usage::On {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(dir.join("journal")) else { return 0 };
    let mut deleted = 0;
    for path in entries.flatten().map(|e| e.path()) {
        let is_journal = path.extension().is_some_and(|e| e == "jsonl");
        let sid = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        let after_consent = since.is_some_and(|at| std::fs::metadata(&path).and_then(|m| m.modified()).is_ok_and(|m| m > at));
        if is_journal && !connected.contains(&sid) && !after_consent {
            match std::fs::remove_file(&path) {
                Ok(()) => deleted += 1,
                Err(e) => eprintln!("arkosd: cannot delete a usage journal recorded before consent: {e}"),
            }
        }
    }
    deleted
}

/// The file in the telemetry folder that holds the usage the loop last saw, one word (`off`,
/// `local` or `on`), so a turn-on made while the daemon was down is still seen as one.
pub const USAGE_SEEN: &str = "usage-seen";

/// The usage the loop last saw, from `USAGE_SEEN`. A missing or unreadable file is `Local`, the
/// default, so a first start that finds usage `On` starts clean too.
pub fn usage_seen(dir: &Path) -> Usage {
    match std::fs::read_to_string(dir.join(USAGE_SEEN)).as_deref().map(str::trim) {
        Ok("off") => Usage::Off,
        Ok("on") => Usage::On,
        _ => Usage::Local,
    }
}

/// Writes `usage` to `USAGE_SEEN` whole, through a temporary file renamed over it.
fn write_usage_seen(dir: &Path, usage: Usage) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!("{USAGE_SEEN}.tmp"));
    std::fs::write(&tmp, format!("{}\n", super::usage_name(usage)))?;
    std::fs::rename(&tmp, dir.join(USAGE_SEEN))
}

/// What `consent_check` deleted: journals left behind by gone windows, and queued sessions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Forgotten {
    pub journals: usize,
    pub sessions: usize,
}

/// The start of a pass: compares `usage` with the usage last seen (`usage_seen`, which outlives
/// the daemon). On a change to `On` it deletes the queued sessions (`Queue::clear_sessions`,
/// which the opt-in surfaces also call, but a turn-on with no window running has none of them:
/// a hand edit, `arkos settings set` with no window, a change made while Arkos was quit) and the
/// journals of gone windows from before the consent (`forget_before_consent`). The moment of the
/// consent is the config file's modification time, the file that says `On`; `usage-seen`'s own
/// time is no use here, since it was last written when usage last changed, long before.
/// Then it records `usage` as seen when it differs.
pub fn consent_check(dir: &Path, config: &Path, usage: Usage, connected: &[&str]) -> Forgotten {
    let previous = usage_seen(dir);
    let mut deleted = Forgotten::default();
    if previous != Usage::On && usage == Usage::On {
        let since = std::fs::metadata(config).and_then(|m| m.modified()).ok();
        deleted.journals = forget_before_consent(dir, previous, usage, connected, since);
        match Queue::new(dir).clear_sessions() {
            Ok(n) => deleted.sessions = n,
            Err(e) => eprintln!("arkosd: cannot clear the sessions queued before consent: {e}"),
        }
    }
    let recorded = std::fs::read_to_string(dir.join(USAGE_SEEN)).ok();
    if recorded.as_deref().map(str::trim) != Some(super::usage_name(usage)) {
        if let Err(e) = write_usage_seen(dir, usage) {
            eprintln!("arkosd: cannot record the usage setting seen: {e}");
        }
    }
    deleted
}

/// One pass of the loop at `now`: seals as `unknown` every journal quiet for `STALE_SECS` whose
/// window is not among `connected` (a sleeping machine makes a live journal look stale), drains
/// the queue through `t` to `url` under `mode`, and adds the drain's sent and refused counts to
/// the state file once the posts are done.
pub fn pass(dir: &Path, t: &dyn Transport, url: &str, user_agent: &str, mode: &Mode, connected: &[&str], now: u64) -> Outcome {
    // Without a journal folder there is nothing to seal, and the state file is left as it is.
    if dir.join("journal").is_dir() {
        match super::update_state(dir, |s| seal_stale_except(dir, now, STALE_SECS, connected, s)) {
            Ok(Ok(_)) => {}
            Ok(Err(e)) | Err(e) => eprintln!("arkosd: cannot seal the usage sessions left behind: {e}"),
        }
    }
    let out = Queue::new(dir).drain(t, url, user_agent, mode, now);
    if out.sent > 0 || out.rejected > 0 {
        let counted = super::update_state(dir, |s| {
            s.sent += out.sent as u64;
            s.rejected += out.rejected as u64;
        });
        if let Err(e) = counted {
            eprintln!("arkosd: cannot save the telemetry counts: {e}");
        }
    }
    out
}

/// Starts the loop on a thread of its own. Each pass, one `EVERY` after the last (the first one
/// `EVERY` after the start), reads the mode afresh from the config file, so `arkos telemetry on`
/// takes effect without a restart, and takes the sids of the windows still in the table, which
/// the stale seal leaves alone. A window leaving meanwhile seals its own journal on the way out.
/// Every pass, the first after a start included, compares the usage with the one last seen,
/// kept in `USAGE_SEEN` across restarts, so a change to `On` first deletes what was left behind
/// before the consent (`consent_check`).
pub fn start(state: crate::server::Shared, o: super::Options, curl: PathBuf) {
    let spawned = std::thread::Builder::new().name("telemetry-upload".into()).spawn(move || {
        let transport = Curl { curl };
        let (url, user_agent) = (o.reports_url(), format!("arkos/{}", env!("CARGO_PKG_VERSION")));
        loop {
            std::thread::sleep(EVERY);
            let mode = super::mode_now(&o.config);
            let sids: Vec<String> = state.lock().unwrap().clients.values().filter_map(|r| r.sid.clone()).collect();
            let connected: Vec<&str> = sids.iter().map(String::as_str).collect();
            consent_check(&o.dir, &o.config, mode.usage, &connected);
            pass(&o.dir, &transport, &url, &user_agent, &mode, &connected, crate::updates::apply::now_unix());
        }
    });
    if let Err(e) = spawned {
        eprintln!("arkosd: cannot start the telemetry upload: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::seal::window_gone;
    use crate::telemetry::tests::release_env;
    use ::telemetry::visits_app::{Channel, Crash, CrashKind, Crashes, End, Envelope, Journal, Memory, Queue, State, Uptime, Usage};
    use ::telemetry::TelemetryConfig;
    use std::cell::RefCell;

    const URL: &str = "https://apps.csarko.sh/a";
    const UA: &str = "arkos/0.5.0";
    const NOW: u64 = 20_000 * 86_400;
    const INC: &str = "abcdef0123456789abcdef0123456789";

    /// A transport that answers `status` to every post and remembers each one.
    struct Recorder {
        status: u16,
        posts: RefCell<Vec<(String, String, Vec<u8>)>>,
    }

    impl Recorder {
        fn answering(status: u16) -> Self {
            Self { status, posts: RefCell::new(Vec::new()) }
        }

        fn calls(&self) -> usize {
            self.posts.borrow().len()
        }
    }

    impl Transport for Recorder {
        fn post(&self, url: &str, user_agent: &str, body: &[u8]) -> Result<u16, String> {
            self.posts.borrow_mut().push((url.into(), user_agent.into(), body.to_vec()));
            Ok(self.status)
        }
    }

    fn crash(env: Envelope) -> Crash {
        Crash {
            env,
            inc: INC.into(),
            ck: CrashKind::Panic,
            commit: "9f2c1ab4e7d03b65a1c8f2e94d7b06a35c81e2fd".into(),
            pk: "other".into(),
            mh: None,
            file: "/src/crates/arkosd/src/server.rs".into(),
            line: 412,
            frames: vec![(0, 4096)],
            images: vec![("arkosd".into(), "0123456789abcdef0123456789abcdef".into(), 52428800)],
            sig: None,
            up: Uptime::Lt1h,
            mem: Some(Memory::Unknown),
        }
    }

    /// A session of `env` that ended at `NOW`, sealed into the queue as the daemon seals it.
    fn queue_session(dir: &Path, sid: &str, env: &Envelope) {
        Journal::open(dir, sid, env, ::telemetry::allowlist().clone(), NOW - 600).unwrap();
        window_gone(dir, sid, End::Quit, NOW).expect("sealed");
    }

    /// A crash the person agreed to send, queued at `NOW`.
    fn queue_crash(dir: &Path, env: Envelope) {
        Queue::new(dir).enqueue_crash(&crash(env), NOW).unwrap();
    }

    fn queued(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir.join("queue")).map(|d| d.map(|e| e.unwrap().file_name().into_string().unwrap()).filter(|n| n.ends_with(".json")).collect()).unwrap_or_default();
        names.sort();
        names
    }

    /// The mode of a release build for this table and environment, as `mode_for` makes it.
    fn mode(config: Result<&TelemetryConfig, ()>, env: &[(&str, &str)], interactive: bool) -> Mode {
        let env: Vec<(String, String)> = env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let read = move |k: &str| env.iter().find(|(name, _)| name == k).map(|(_, v)| v.clone());
        ::telemetry::mode_with(config, &read, &Channel::Release, interactive)
    }

    const CONSENTED: TelemetryConfig = TelemetryConfig { usage: Usage::On, crashes: Crashes::Always };

    #[test]
    fn curl_is_told_https_only_ten_seconds_the_body_on_stdin_and_only_the_status_back() {
        let args = curl_args(URL, UA);
        assert_eq!(args.first().map(String::as_str), Some("--disable"), "no ~/.curlrc can add options");
        let pairs = [("--proto", "=https"), ("--max-time", "10"), ("--user-agent", UA), ("-H", "Content-Type: application/json"), ("--data-binary", "@-"), ("-o", "/dev/null"), ("-w", "%{http_code}")];
        for (flag, value) in pairs {
            let i = args.iter().position(|a| a == flag).unwrap_or_else(|| panic!("no {flag} in {args:?}"));
            assert_eq!(args[i + 1], value, "{flag}");
        }
        for never in ["--fail", "-f", "--location", "-L", "--proto-redir"] {
            assert!(!args.iter().any(|a| a == never), "{never} in {args:?}");
        }
        assert_eq!(args.last().map(String::as_str), Some(URL), "the url is last so it cannot be read as an option");
        assert_eq!(args[args.len() - 2], "--");
    }

    /// A curl that records its arguments and the body it read from stdin, and prints the status
    /// written in `status` (with curl's exit 7 for `000`, no connection).
    fn fake_curl(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let script = dir.join("curl");
        let d = dir.display();
        let text = format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{d}/args'\ncat > '{d}/body'\ns=$(cat '{d}/status')\nprintf '%s' \"$s\"\n[ \"$s\" = 000 ] && exit 7\nexit 0\n");
        // Written under the spawn lock, so no fork in another test inherits the script open for
        // writing and makes its exec fail as busy.
        let _no_fork = crate::pty::spawn_lock();
        std::fs::write(&script, text).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script
    }

    #[test]
    fn the_curl_transport_gives_the_crate_each_status() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("telemetry");
        let curl = Curl { curl: fake_curl(tmp.path()) };
        let m = mode(Ok(&CONSENTED), &[], true);
        let set = |status: &str| std::fs::write(tmp.path().join("status"), status).unwrap();
        let name = format!("crash-{INC}.json");

        // Accepted: deleted and counted sent.
        queue_crash(&dir, release_env());
        let bytes = std::fs::read(dir.join("queue").join(&name)).unwrap();
        set("204");
        let out = pass(&dir, &curl, URL, UA, &m, &[], NOW);
        assert_eq!((out.sent, out.failed, out.rejected), (1, 0, 0), "{out:?}");
        assert!(queued(&dir).is_empty());
        assert_eq!(std::fs::read(tmp.path().join("body")).unwrap(), bytes, "the body is the queued bytes, through stdin");
        let args = std::fs::read_to_string(tmp.path().join("args")).unwrap();
        assert_eq!(args.lines().last(), Some(URL));
        assert!(args.lines().any(|a| a == UA));
        assert_eq!(State::load(&dir).sent, 1, "the count reaches the state file");

        // Too many requests: kept for the backoff.
        queue_crash(&dir, release_env());
        set("429");
        let out = pass(&dir, &curl, URL, UA, &m, &[], NOW);
        assert_eq!((out.sent, out.failed, out.rejected), (0, 1, 0), "{out:?}");
        assert_eq!(queued(&dir), [name.clone()]);

        // No connection: curl fails, kept for the backoff too.
        queue_crash(&dir, release_env());
        set("000");
        let out = pass(&dir, &curl, URL, UA, &m, &[], NOW);
        assert_eq!((out.sent, out.failed, out.rejected), (0, 1, 0), "{out:?}");
        assert_eq!(queued(&dir), [name.clone()]);

        // Refused: deleted and counted, never tried again.
        queue_crash(&dir, release_env());
        set("400");
        let out = pass(&dir, &curl, URL, UA, &m, &[], NOW);
        assert_eq!((out.sent, out.failed, out.rejected), (0, 0, 1), "{out:?}");
        assert!(queued(&dir).is_empty());
        assert_eq!((State::load(&dir).sent, State::load(&dir).rejected), (1, 1));
    }

    /// Review Focus 4: the default config sends no session. A crash queued from the dialog's
    /// Send is the person's consent for that report and does leave.
    #[test]
    fn the_default_config_uploads_no_session() {
        let dir = tempfile::tempdir().unwrap();
        queue_session(dir.path(), "0123456789abcdef0123456789abcdef", &release_env());
        let t = Recorder::answering(204);
        let out = pass(dir.path(), &t, URL, UA, &mode(Ok(&TelemetryConfig::default()), &[], true), &[], NOW);
        assert_eq!(t.calls(), 0, "{out:?}");
        assert_eq!(queued(dir.path()).len(), 1, "kept for seven days, should usage turn on");
        queue_crash(dir.path(), release_env());
        pass(dir.path(), &t, URL, UA, &mode(Ok(&TelemetryConfig::default()), &[], true), &[], NOW);
        assert_eq!(t.calls(), 1, "the crash the person sent");
        assert!(t.posts.borrow()[0].2.windows(7).any(|w| w == b"\"crash\""));
    }

    /// One rule of Review Focus 4: with everything consented in the config, this environment
    /// keeps both a session and a crash from leaving, and both stay queued.
    fn nothing_leaves_under(m: Mode) {
        let dir = tempfile::tempdir().unwrap();
        queue_session(dir.path(), "0123456789abcdef0123456789abcdef", &release_env());
        queue_crash(dir.path(), release_env());
        let t = Recorder::answering(204);
        let out = pass(dir.path(), &t, URL, UA, &m, &[], NOW);
        assert_eq!(t.calls(), 0, "{m:?}: {out:?}");
        assert_eq!(queued(dir.path()).len(), 2, "{m:?}");
        assert!(!m.uploads && m.reason.is_some(), "{m:?}");
    }

    #[test]
    fn do_not_track_uploads_nothing() {
        nothing_leaves_under(mode(Ok(&CONSENTED), &[("DO_NOT_TRACK", "1")], true));
    }

    #[test]
    fn arkos_telemetry_off_uploads_nothing() {
        nothing_leaves_under(mode(Ok(&CONSENTED), &[("ARKOS_TELEMETRY", "off")], true));
    }

    #[test]
    fn arkos_telemetry_log_uploads_nothing() {
        nothing_leaves_under(mode(Ok(&CONSENTED), &[("ARKOS_TELEMETRY", "log")], true));
    }

    #[test]
    fn ci_uploads_nothing() {
        nothing_leaves_under(mode(Ok(&CONSENTED), &[("CI", "true")], true));
    }

    #[test]
    fn a_non_interactive_run_uploads_nothing() {
        nothing_leaves_under(mode(Ok(&CONSENTED), &[], false));
    }

    #[test]
    fn a_config_error_uploads_nothing() {
        let m = mode(Err(()), &[], true);
        assert_eq!(m.reason, Some("config error"));
        nothing_leaves_under(m);
    }

    /// Review Focus 1: a source build's report never reaches the transport, whatever the
    /// settings; the drain deletes it.
    #[test]
    fn a_source_build_never_reaches_the_transport() {
        let dir = tempfile::tempdir().unwrap();
        let source = Envelope { ch: Channel::Source, ..release_env() };
        queue_session(dir.path(), "0123456789abcdef0123456789abcdef", &source);
        queue_crash(dir.path(), source);
        let no_env = |_: &str| None;
        let m = ::telemetry::mode_with(Ok::<_, ()>(&CONSENTED), &no_env, &Channel::Source, true);
        assert_eq!((m.uploads, m.reason), (false, Some("source build")));
        let t = Recorder::answering(204);
        let out = pass(dir.path(), &t, URL, UA, &m, &[], NOW);
        assert_eq!((t.calls(), out.dropped), (0, 2), "{out:?}");
        // A release daemon draining a source report drops it unsent too.
        queue_session(dir.path(), "fedcba9876543210fedcba9876543210", &Envelope { ch: Channel::Source, ..release_env() });
        let out = pass(dir.path(), &t, URL, UA, &mode(Ok(&CONSENTED), &[], true), &[], NOW);
        assert_eq!((t.calls(), out.dropped), (0, 1), "{out:?}");
        assert!(queued(dir.path()).is_empty());
    }

    /// Withdrawing consent stops the crashes already queued: one queued under `always` that
    /// failed once is not posted again once crashes are `never`.
    #[test]
    fn a_crash_queued_under_always_stops_once_crashes_is_never() {
        let dir = tempfile::tempdir().unwrap();
        queue_crash(dir.path(), release_env());
        let busy = Recorder::answering(503);
        pass(dir.path(), &busy, URL, UA, &mode(Ok(&CONSENTED), &[], true), &[], NOW);
        assert_eq!(busy.calls(), 1);
        let never = TelemetryConfig { crashes: Crashes::Never, ..CONSENTED };
        let t = Recorder::answering(204);
        let later = NOW + EVERY.as_secs() + 60;
        let out = pass(dir.path(), &t, URL, UA, &mode(Ok(&never), &[], true), &[], later);
        assert_eq!((t.calls(), out.skipped), (0, 1), "{out:?}");
        assert_eq!(queued(dir.path()), [format!("crash-{INC}.json")]);
    }

    /// The journals left by windows that are gone are sealed as `unknown` once a day quiet; a
    /// window still connected keeps its journal, however old it looks after a sleep.
    #[test]
    fn the_loop_seals_what_was_left_and_leaves_a_connected_windows_journal_alone() {
        let dir = tempfile::tempdir().unwrap();
        let (live, gone, fresh) = ("0123456789abcdef0123456789abcdef", "fedcba9876543210fedcba9876543210", "00112233445566778899aabbccddeeff");
        let allow = ::telemetry::allowlist();
        Journal::open(dir.path(), live, &release_env(), allow.clone(), NOW - 2 * STALE_SECS).unwrap();
        Journal::open(dir.path(), gone, &release_env(), allow.clone(), NOW - 2 * STALE_SECS).unwrap();
        Journal::open(dir.path(), fresh, &release_env(), allow.clone(), NOW - 60).unwrap();
        let t = Recorder::answering(204);
        pass(dir.path(), &t, URL, UA, &mode(Ok(&TelemetryConfig::default()), &[], true), &[live], NOW);
        let journal = |sid: &str| dir.path().join("journal").join(format!("{sid}.jsonl"));
        assert!(journal(live).exists(), "a connected window's journal stays");
        assert!(journal(fresh).exists(), "a journal quiet for less than a day stays");
        assert!(!journal(gone).exists());
        assert_eq!(queued(dir.path()), [format!("session-{gone}.json")]);
        assert_eq!(crate::telemetry::seal::tests::sealed_end(dir.path(), gone).as_deref(), Some("unknown"));
        // Once that window is gone too, the next pass seals it.
        pass(dir.path(), &t, URL, UA, &mode(Ok(&TelemetryConfig::default()), &[], true), &[], NOW);
        assert!(!journal(live).exists());
        assert_eq!(t.calls(), 0, "and under the default config nothing was sent");
    }

    /// Nothing recorded before the consent is sent: when usage turns `On`, the journals of the
    /// windows that are gone are deleted before the pass could seal them, and a connected
    /// window's journal is left for that window to restart.
    #[test]
    fn turning_usage_on_deletes_what_gone_windows_recorded_before_the_consent() {
        let dir = tempfile::tempdir().unwrap();
        let (live, gone) = ("0123456789abcdef0123456789abcdef", "fedcba9876543210fedcba9876543210");
        let allow = ::telemetry::allowlist();
        Journal::open(dir.path(), live, &release_env(), allow.clone(), NOW - 2 * STALE_SECS).unwrap();
        Journal::open(dir.path(), gone, &release_env(), allow.clone(), NOW - 2 * STALE_SECS).unwrap();
        std::fs::write(dir.path().join("journal").join("notes.txt"), "not a journal").unwrap();
        let journal = |sid: &str| dir.path().join("journal").join(format!("{sid}.jsonl"));
        // No change to `On`: nothing is deleted.
        for (previous, usage) in [(Usage::Local, Usage::Local), (Usage::On, Usage::On), (Usage::Off, Usage::Local), (Usage::On, Usage::Local)] {
            assert_eq!(forget_before_consent(dir.path(), previous, usage, &[], None), 0, "{previous:?} to {usage:?}");
        }
        assert!(journal(gone).exists() && journal(live).exists());
        assert_eq!(forget_before_consent(dir.path(), Usage::Local, Usage::On, &[live], None), 1);
        assert!(!journal(gone).exists(), "a gone window's journal from before the consent");
        assert!(journal(live).exists(), "a connected window restarts its own");
        assert!(dir.path().join("journal").join("notes.txt").exists(), "only journals are touched");
        let t = Recorder::answering(204);
        pass(dir.path(), &t, URL, UA, &mode(Ok(&CONSENTED), &[], true), &[live], NOW);
        assert!(queued(dir.path()).is_empty(), "nothing from before the consent was sealed");
        assert_eq!(t.calls(), 0, "or sent");
        assert_eq!(forget_before_consent(dir.path(), Usage::Off, Usage::On, &[], None), 1, "from off too");
    }

    /// A turn-on made while the daemon was down: the usage last seen is on disk, so the first
    /// pass after the start still deletes what a gone window recorded before the consent.
    #[test]
    fn a_turn_on_while_the_daemon_was_down_still_starts_clean() {
        let dir = tempfile::tempdir().unwrap();
        let gone = "fedcba9876543210fedcba9876543210";
        Journal::open(dir.path(), gone, &release_env(), ::telemetry::allowlist().clone(), NOW - 2 * STALE_SECS).unwrap();
        std::fs::write(dir.path().join(USAGE_SEEN), "local\n").unwrap();
        assert_eq!(consent_check(dir.path(), &dir.path().join("config.toml"), Usage::On, &[]).journals, 1);
        assert!(!dir.path().join("journal").join(format!("{gone}.jsonl")).exists(), "deleted before any seal");
        assert_eq!(std::fs::read_to_string(dir.path().join(USAGE_SEEN)).unwrap(), "on\n");
        assert!(!dir.path().join(format!("{USAGE_SEEN}.tmp")).exists(), "written through a renamed temporary");
        let t = Recorder::answering(204);
        pass(dir.path(), &t, URL, UA, &mode(Ok(&CONSENTED), &[], true), &[], NOW);
        assert!(queued(dir.path()).is_empty() && t.calls() == 0, "nothing sealed or sent");
        // Still on: a journal recorded after the consent is kept for the seal.
        Journal::open(dir.path(), gone, &release_env(), ::telemetry::allowlist().clone(), NOW - 2 * STALE_SECS).unwrap();
        assert_eq!(consent_check(dir.path(), &dir.path().join("config.toml"), Usage::On, &[]).journals, 0);
        // Back to local, then on again: a new consent starts clean again.
        assert_eq!(consent_check(dir.path(), &dir.path().join("config.toml"), Usage::Local, &[]).journals, 0);
        assert_eq!(usage_seen(dir.path()), Usage::Local);
        assert_eq!(consent_check(dir.path(), &dir.path().join("config.toml"), Usage::On, &[]).journals, 1);
    }

    /// A turn-on with no window running (a hand edit, `arkos settings set` with no window, a
    /// change made while Arkos was quit) still clears the sessions sealed under `local`.
    #[test]
    fn a_turn_on_with_no_window_running_clears_the_queued_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let sid = "00112233445566778899aabbccddeeff";
        queue_session(dir.path(), sid, &release_env());
        queue_crash(dir.path(), release_env());
        assert_eq!(queued(dir.path()).len(), 2);
        std::fs::write(dir.path().join(USAGE_SEEN), "local\n").unwrap();
        let forgotten = consent_check(dir.path(), &dir.path().join("config.toml"), Usage::On, &[]);
        assert_eq!(forgotten, Forgotten { journals: 0, sessions: 1 });
        assert_eq!(queued(dir.path()), [format!("crash-{INC}.json")], "the session is gone; a crash the person agreed to send stays");
        let t = Recorder::answering(204);
        let consent_for_usage_only = TelemetryConfig { usage: Usage::On, crashes: Crashes::Never };
        pass(dir.path(), &t, URL, UA, &mode(Ok(&consent_for_usage_only), &[], true), &[], NOW);
        assert_eq!(t.calls(), 0, "nothing was posted");
        // Still on at the next pass: what is sealed from now on stays queued for the drain.
        queue_session(dir.path(), "ffeeddccbbaa99887766554433221100", &release_env());
        assert_eq!(consent_check(dir.path(), &dir.path().join("config.toml"), Usage::On, &[]), Forgotten::default());
    }

    /// A window that launched under `On` and has not sent its `Session` yet is not connected by
    /// sid, but its journal was written after the consent, so it is kept.
    #[test]
    fn a_journal_written_after_the_turn_on_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let (before, after) = ("fedcba9876543210fedcba9876543210", "0123456789abcdef0123456789abcdef");
        let allow = ::telemetry::allowlist();
        Journal::open(dir.path(), before, &release_env(), allow.clone(), NOW - 2 * STALE_SECS).unwrap();
        Journal::open(dir.path(), after, &release_env(), allow.clone(), NOW - 60).unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(&config, "[telemetry]\nusage = \"on\"\n").unwrap();
        let turned_on = std::fs::metadata(&config).unwrap().modified().unwrap();
        let journal = |sid: &str| dir.path().join("journal").join(format!("{sid}.jsonl"));
        let set_mtime = |sid: &str, at: SystemTime| std::fs::File::options().write(true).open(journal(sid)).unwrap().set_modified(at).unwrap();
        set_mtime(before, turned_on - Duration::from_secs(3600));
        set_mtime(after, turned_on + Duration::from_secs(60));
        std::fs::write(dir.path().join(USAGE_SEEN), "local\n").unwrap();
        assert_eq!(consent_check(dir.path(), &config, Usage::On, &[]).journals, 1);
        assert!(!journal(before).exists(), "written before the consent");
        assert!(journal(after).exists(), "written after it");
    }

    #[test]
    fn a_missing_usage_seen_file_counts_as_local() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(usage_seen(dir.path()), Usage::Local);
        std::fs::write(dir.path().join(USAGE_SEEN), "garbage").unwrap();
        assert_eq!(usage_seen(dir.path()), Usage::Local);
        std::fs::write(dir.path().join(USAGE_SEEN), "off\n").unwrap();
        assert_eq!(usage_seen(dir.path()), Usage::Off);
        // A first start under the default writes what it saw.
        let fresh = tempfile::tempdir().unwrap();
        assert_eq!(consent_check(fresh.path(), &fresh.path().join("config.toml"), Usage::Local, &[]).journals, 0);
        assert_eq!(std::fs::read_to_string(fresh.path().join(USAGE_SEEN)).unwrap(), "local\n");
    }
}
