//! The check: manifest, signature, comparison, download, hash, stage. Pure apart from the
//! processes it runs, so the tests drive it over `file://`.
use super::fetch::{DownloadError, Fetcher};
use super::stage::{stage, Staged};
use super::version::Version;
use super::{manifest, verify, UpdateState, MAX_ZIP_BYTES};
use crate::server::Shared;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug)]
pub enum Outcome {
    /// The check worked and found nothing newer than what runs and what is staged: the
    /// manifest's version and git, and the " or than X staged or applied" text (None when
    /// nothing is staged).
    Nothing { version: String, git: String, staged: Option<String> },
    Staged(Staged),
    /// The manifest or the zip could not be fetched (offline, a captive portal, a timeout): logged
    /// and tried again at the next check, never reported to the windows.
    Transient(String),
    /// The manifest's signature does not verify (or the updates folder cannot be made): reported,
    /// and no manifest was reached.
    Invalid(String),
    /// The signed manifest was read, but the release is wrong (size, sha256) or staging failed:
    /// reported.
    Failed(String),
    /// The manifest names a release that already failed; the text is that failure's.
    Skipped(String),
}

/// Releases that failed their size, hash or staging, by `bad_key`, with the failure. Keyed on
/// the sha256 too, so a re-signed manifest with a corrected zip is tried again.
pub type Bad = HashMap<String, String>;

fn bad_key(m: &manifest::Manifest) -> String {
    format!("{} {} {}", m.version, m.git, m.arm64.sha256.to_ascii_lowercase())
}

/// The updates folder, private to this user: what is staged there is run later.
fn private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

/// Why a download, hash or stage failed: the release's fault (marked bad) or the network's.
enum Failure {
    Transport(String),
    Release(String),
}

/// One check. `staged` is the version already staged: a manifest must be newer than it and
/// than `running`, so the same release is not fetched twice and a replayed older manifest
/// cannot replace a newer staged one. A failure of the release itself (size, sha256, staging)
/// marks it bad; a transport failure is retried at the next check.
pub fn run_check(f: &Fetcher, dir: &Path, keys: &[&str], running: &Version, bad: &mut Bad, ditto: &Path, staged: Option<&Version>) -> Outcome {
    if let Err(e) = private_dir(dir) {
        return Outcome::Invalid(format!("updates dir: {e}"));
    }
    let (bytes, sig) = match f.fetch_manifest(dir) {
        Ok(x) => x,
        Err(e) => return Outcome::Transient(e),
    };
    if let Err(e) = verify::verify_manifest(&bytes, &sig, keys) {
        return Outcome::Invalid(e);
    }
    let m = match manifest::parse(&bytes) {
        Ok(m) => m,
        Err(e) => return Outcome::Failed(e),
    };
    let Some(v) = m.version() else { return Outcome::Failed("manifest: bad version".into()) };
    if !v.is_newer_than(running) || staged.is_some_and(|s| !v.is_newer_than(s)) {
        let staged = staged.map(|s| format!(" or than {}.{}.{} staged or applied", s.major, s.minor, s.patch));
        return Outcome::Nothing { version: m.version, git: m.git, staged };
    }
    let key = bad_key(&m);
    if let Some(e) = bad.get(&key) {
        return Outcome::Skipped(e.clone());
    }
    if m.arm64.size > MAX_ZIP_BYTES {
        let e = format!("release {} is {} bytes, over the {MAX_ZIP_BYTES} cap", m.version, m.arm64.size);
        bad.insert(key, e.clone());
        return Outcome::Failed(e);
    }
    let zip = dir.join(format!("{}.zip", m.version));
    let result = (|| {
        f.download(&m.arm64.url, m.arm64.size, &zip).map_err(|e| match e {
            DownloadError::Transport(e) => Failure::Transport(e),
            DownloadError::Release(e) => Failure::Release(e),
        })?;
        // Reading the file back is local: a failure here is not the release's fault.
        let got = verify::sha256_hex(&zip).map_err(|e| Failure::Transport(format!("sha256: {e}")))?;
        if !got.eq_ignore_ascii_case(&m.arm64.sha256) {
            return Err(Failure::Release(format!("sha256 of the download is {got}, the manifest says {}", m.arm64.sha256)));
        }
        stage(dir, &m, &zip, ditto).map_err(Failure::Release)
    })();
    let _ = std::fs::remove_file(&zip);
    match result {
        Ok(s) => {
            super::stage::clean_except(dir, Some(&s.bundle));
            Outcome::Staged(s)
        }
        Err(Failure::Transport(e)) => Outcome::Transient(e),
        Err(Failure::Release(e)) => {
            bad.insert(key, e.clone());
            Outcome::Failed(e)
        }
    }
}

/// An outcome the check can repeat, in the pieces its journal lines are built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repeat {
    /// `what` is "{version} ({git})", `floor` the running or installed semver, `first_line` the
    /// line as it is written today (with its "or than X staged or applied" suffix when any).
    NotNewer { what: String, floor: String, first_line: String },
    Transient { error: String },
    /// Any other outcome, by its line: a staged release, a failed or skipped one.
    Other { line: String },
}

impl Repeat {
    pub fn first_line(&self) -> String {
        match self {
            Repeat::NotNewer { first_line, .. } => first_line.clone(),
            Repeat::Transient { error } => format!("check: transient, tried again at the next check: {error}"),
            Repeat::Other { line } => line.clone(),
        }
    }
    fn count_line(&self, repeats: u32, since: &str) -> String {
        match self {
            Repeat::NotNewer { what, floor, .. } => format!("check: {repeats} more checks since {since} found {what}, still not newer than {floor}"),
            Repeat::Transient { error } => format!("check: transient, {repeats} more times since {since}: {error}"),
            Repeat::Other { line } => format!("check: {repeats} more times since {since}: {line}"),
        }
    }
}

/// Repeated journal lines from the check (the same outcome, whichever it is, check after check)
/// are counted, not written: the count goes out when the outcome changes, at a handover, and
/// once a day while it keeps repeating.
#[derive(Default)]
pub struct Collapse {
    current: Option<Repeat>,
    repeats: u32,
    since: Option<std::time::Instant>,
    since_wall: String,
    pending: Option<String>,
}

const DAILY: Duration = Duration::from_secs(24 * 60 * 60);

impl Collapse {
    /// `wall` is the local clock as "HH:MM" (`journal::clock_hhmm()`), passed in so tests pin it.
    pub fn note(&mut self, r: Repeat, now: std::time::Instant, wall: &str) -> Option<String> {
        if self.current.as_ref() == Some(&r) {
            self.repeats += 1;
            if self.since.is_some_and(|s| now.duration_since(s) >= DAILY) {
                let out = r.count_line(self.repeats, &self.since_wall);
                self.repeats = 0;
                self.since = Some(now);
                self.since_wall = wall.to_string();
                return Some(out);
            }
            return None;
        }
        let out = match (&self.current, self.repeats) {
            (Some(prev), n) if n > 0 => {
                self.pending = Some(r.first_line());
                prev.count_line(n, &self.since_wall)
            }
            _ => r.first_line(),
        };
        self.current = Some(r);
        self.repeats = 0;
        self.since = Some(now);
        self.since_wall = wall.to_string();
        Some(out)
    }

    /// The held first line of a new outcome, nothing else: what the poll thread writes after `note`.
    pub fn take_pending(&mut self) -> Option<String> {
        self.pending.take()
    }

    /// The count so far, else any held line (for a handover or exit). Restarts the count from now.
    pub fn flush(&mut self) -> Option<String> {
        if self.repeats > 0 {
            let out = self.current.as_ref()?.count_line(self.repeats, &self.since_wall);
            self.repeats = 0;
            self.since = Some(std::time::Instant::now());
            self.since_wall = crate::journal::clock_hhmm();
            return Some(out);
        }
        self.pending.take()
    }
}

fn semver_of(v: &Version) -> String {
    format!("{}.{}.{}", v.major, v.minor, v.patch)
}

/// What the journal says about one check, as a `Repeat` for the collapse; `floor` is what the
/// check compared against.
pub fn repeat_of(outcome: &Outcome, floor: &Version) -> Repeat {
    match outcome {
        Outcome::Nothing { version, git, staged } => Repeat::NotNewer {
            what: format!("{version} ({git})"),
            floor: semver_of(floor),
            first_line: format!("check: the manifest has {version} ({git}), not newer than running {}{}", semver_of(floor), staged.as_deref().unwrap_or("")),
        },
        Outcome::Transient(error) => Repeat::Transient { error: error.clone() },
        Outcome::Staged(s) => Repeat::Other { line: format!("staged {} ({}) at {}", s.version, s.git, s.bundle.display()) },
        Outcome::Invalid(e) | Outcome::Failed(e) => Repeat::Other { line: format!("check failed: {e}") },
        Outcome::Skipped(e) => Repeat::Other { line: format!("check: the release failed before, skipped: {e}") },
    }
}

/// A check's outcome into the shared state. Returns the event to tell the windows, if any.
/// `last_error` is cleared only by a check that positively found nothing newer or staged one; a
/// skipped bad release keeps its original failure reported, and a transport failure leaves it as
/// it was (a wifi drop is not news).
pub fn record(u: &mut UpdateState, outcome: Outcome) -> Option<protocol::StagedUpdate> {
    record_at(u, outcome, super::apply::now_unix())
}

/// `record` at `now_unix`. Every outcome but a transport failure and a bad signature reached a
/// verified manifest, which is what `last_ok_unix` counts.
pub fn record_at(u: &mut UpdateState, outcome: Outcome, now_unix: u64) -> Option<protocol::StagedUpdate> {
    u.last_check_transient = matches!(outcome, Outcome::Transient(_));
    u.last_check_unix = Some(now_unix);
    u.transient_error = match &outcome {
        Outcome::Transient(e) => Some(e.clone()),
        _ => None,
    };
    if !matches!(outcome, Outcome::Transient(_) | Outcome::Invalid(_)) {
        u.last_ok_unix = Some(now_unix);
    }
    match outcome {
        Outcome::Nothing { .. } => {
            u.last_error = None;
            None
        }
        Outcome::Staged(s) => {
            u.last_error = None;
            let update = protocol::StagedUpdate { version: s.version.clone(), git: s.git.clone() };
            u.staged = Some(s);
            Some(update)
        }
        Outcome::Transient(e) => {
            eprintln!("arkosd: update check: {e} (tried again at the next check)");
            None
        }
        Outcome::Failed(e) | Outcome::Invalid(e) => {
            eprintln!("arkosd: update check: {e}");
            u.last_error = Some(e);
            None
        }
        Outcome::Skipped(e) => {
            u.last_error = Some(e);
            None
        }
    }
}

/// Whether a staged release must never be offered or applied: its version is not newer than
/// what runs (or does not parse).
pub fn is_stale(staged: &Staged, running: &Version) -> bool {
    staged_version(staged).is_none_or(|v| !v.is_newer_than(running))
}

/// The staged release's version, for the comparison.
fn staged_version(s: &Staged) -> Option<Version> {
    let (major, minor, patch) = super::version::parse_semver(&s.version)?;
    Some(Version { major, minor, patch, git: s.git.clone() })
}

/// A version this process swapped in, as the comparison reads it.
fn swapped_version(v: &str) -> Option<Version> {
    let (major, minor, patch) = super::version::parse_semver(v)?;
    Some(Version { major, minor, patch, git: String::new() })
}

fn newest(a: Option<Version>, b: Option<Version>) -> Option<Version> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if b.is_newer_than(&a) { b } else { a }),
        (a, b) => a.or(b),
    }
}

/// The poll thread: every `every`, or sooner when a window asked (`check_now`). The state lock
/// is never held across a check.
/// `running` is this daemon's version; each check compares against the newer of it and the
/// installed `bundle`'s (`super::floor`), read afresh, so a hand-installed newer app is seen.
pub fn start(state: Shared, f: Fetcher, dir: PathBuf, keys: Vec<String>, every: Duration, ditto: PathBuf, running: Version, bundle: Option<PathBuf>) {
    {
        let mut st = state.lock().unwrap();
        st.update.polling = true;
        // A daemon exec that carried a newer value keeps it.
        if st.update.last_ok_unix.is_none() {
            st.update.last_ok_unix = super::last_check::read(&dir);
        }
    }
    std::thread::Builder::new()
        .name("updates".into())
        .spawn(move || {
            let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
            let mut bad = Bad::new();
            let floor = super::floor(&running, bundle.as_deref());
            if let Some(line) = super::adopt_staged(&state, &dir, &floor) {
                crate::journal::note(line);
            }
            let mut last = None::<std::time::Instant>;
            loop {
                let (due, staged) = {
                    let mut st = state.lock().unwrap();
                    let due = st.update.check_now || last.is_none_or(|t| t.elapsed() >= every);
                    st.update.check_now = false;
                    if due {
                        st.update.checking = true;
                    }
                    let staged = st.update.staged.as_ref().and_then(staged_version);
                    let swapped = st.update.swapped.as_deref().and_then(swapped_version);
                    (due, newest(staged, swapped))
                };
                if due {
                    last = Some(std::time::Instant::now());
                    let floor = super::floor(&running, bundle.as_deref());
                    super::drop_stale_staged(&state, &dir, &floor);
                    let staged = staged.filter(|s| s.is_newer_than(&floor));
                    let outcome = run_check(&f, &dir, &keys, &floor, &mut bad, &ditto, staged.as_ref());
                    // Every outcome through the collapse: a change always writes the new first
                    // line, and a repeat of any outcome is counted.
                    let r = repeat_of(&outcome, &floor);
                    let lines = {
                        let mut st = state.lock().unwrap();
                        [st.update.collapse.note(r, std::time::Instant::now(), &crate::journal::clock_hhmm()), st.update.collapse.take_pending()]
                    };
                    for l in lines.into_iter().flatten() {
                        crate::journal::note(l);
                    }
                    let reached_now = !matches!(outcome, Outcome::Transient(_) | Outcome::Invalid(_));
                    let reached = {
                        let mut st = state.lock().unwrap();
                        st.update.checking = false;
                        if let Some(update) = record(&mut st.update, outcome) {
                            crate::server::broadcast(&st, protocol::Event::UpdateStaged { update });
                        }
                        st.update.last_ok_unix
                    };
                    if let Some(ok) = reached.filter(|_| reached_now) {
                        if let Err(e) = super::last_check::write(&dir, ok) {
                            eprintln!("arkosd: cannot write last_check.json: {e}");
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        })
        .expect("spawn the updates thread");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::updates::stage::tests::{fake_bundle, manifest_for, zip_of};
    use crate::updates::verify::tests::{test_keygen, test_sign};

    #[test]
    fn repeated_check_lines_are_counted_and_flushed_on_change_and_daily() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let mut c = Collapse::default();
        let nothing = Repeat::NotNewer { what: "1.2.3 (abc1234)".into(), floor: "1.2.3".into(), first_line: "check: the manifest has 1.2.3 (abc1234), not newer than running 1.2.3".into() };
        assert_eq!(c.note(nothing.clone(), t0, "10:00"), Some(nothing.first_line().to_string()));
        assert_eq!(c.note(nothing.clone(), t0 + Duration::from_secs(300), "10:05"), None);
        assert_eq!(c.note(nothing.clone(), t0 + Duration::from_secs(600), "10:10"), None);
        let transient = Repeat::Transient { error: "dns".into() };
        let flushed = c.note(transient.clone(), t0 + Duration::from_secs(900), "10:15").unwrap();
        assert_eq!(flushed, "check: 2 more checks since 10:00 found 1.2.3 (abc1234), still not newer than 1.2.3");
        // The new outcome's first line is held until the flush that follows.
        assert_eq!(c.take_pending(), Some("check: transient, tried again at the next check: dns".into()));
        assert_eq!(c.note(transient.clone(), t0 + Duration::from_secs(1200), "10:20"), None);
        // A day of repeats writes a count without a change.
        let daily = c.note(transient.clone(), t0 + Duration::from_secs(1200 + 86_400), "10:20").unwrap();
        assert_eq!(daily, "check: transient, 2 more times since 10:15: dns");
        assert_eq!(c.take_pending(), None);
        // The daemon's handover flushes what is counted so far.
        c.note(transient.clone(), t0 + Duration::from_secs(1500 + 86_400), "10:25");
        assert_eq!(c.flush(), Some("check: transient, 1 more times since 10:20: dns".into()));
        // The poll thread's sequence (note, then take_pending) writes nothing on repeats; only the
        // handover's flush writes one count.
        let mut c = Collapse::default();
        assert!(c.note(nothing.clone(), t0, "10:00").is_some());
        assert_eq!(c.take_pending(), None);
        for k in 1..=3u64 {
            assert_eq!(c.note(nothing.clone(), t0 + Duration::from_secs(300 * k), "10:05"), None);
            assert_eq!(c.take_pending(), None);
        }
        assert_eq!(c.flush(), Some("check: 3 more checks since 10:00 found 1.2.3 (abc1234), still not newer than 1.2.3".into()));
        assert_eq!(c.flush(), None);
        // A change back to an earlier outcome writes its first line again, never a silent count.
        let failed = Repeat::Other { line: "check failed: bad signature".into() };
        let mut c = Collapse::default();
        assert_eq!(c.note(nothing.clone(), t0, "10:00"), Some(nothing.first_line()));
        assert_eq!(c.note(failed.clone(), t0 + Duration::from_secs(300), "10:05"), Some("check failed: bad signature".into()));
        assert_eq!(c.take_pending(), None);
        assert_eq!(c.note(nothing.clone(), t0 + Duration::from_secs(600), "10:10"), Some(nothing.first_line()));
        assert_eq!(c.note(failed.clone(), t0 + Duration::from_secs(900), "10:15"), Some("check failed: bad signature".into()));
        assert_eq!(c.note(failed.clone(), t0 + Duration::from_secs(1200), "10:20"), None, "a repeated failure is counted");
        assert_eq!(c.note(nothing.clone(), t0 + Duration::from_secs(1500), "10:25"), Some("check: 1 more times since 10:15: check failed: bad signature".into()));
        assert_eq!(c.take_pending(), Some(nothing.first_line()));
    }

    /// Every outcome goes through the collapse, each with the first line it always had.
    #[test]
    fn every_outcome_has_its_repeat_and_first_line() {
        let floor = ver(1, 2, 3);
        let line = |o: Outcome| repeat_of(&o, &floor).first_line();
        assert_eq!(line(Outcome::Nothing { version: "1.2.3".into(), git: "abc".into(), staged: None }), "check: the manifest has 1.2.3 (abc), not newer than running 1.2.3");
        assert_eq!(line(Outcome::Transient("dns".into())), "check: transient, tried again at the next check: dns");
        assert_eq!(line(Outcome::Invalid("bad signature".into())), "check failed: bad signature");
        assert_eq!(line(Outcome::Failed("bad hash".into())), "check failed: bad hash");
        assert_eq!(line(Outcome::Skipped("bad hash".into())), "check: the release failed before, skipped: bad hash");
        assert_eq!(repeat_of(&Outcome::Skipped("e".into()), &floor), Repeat::Other { line: "check: the release failed before, skipped: e".into() });
    }

    struct Site {
        _dir: tempfile::TempDir,
        base: String,
        root: PathBuf,
        sk: String,
        pk: String,
    }

    fn publish(site: &Site, version: &str, git: &str, tamper: bool) {
        publish_as(site, version, git, tamper, None)
    }

    /// `sha256`: what the manifest claims instead of the zip's real hash.
    fn publish_as(site: &Site, version: &str, git: &str, tamper: bool, sha256: Option<&str>) {
        let app = fake_bundle(&site.root.join("src"), version, git);
        let zip = site.root.join("bundle.zip");
        zip_of(&app, &zip);
        let m = manifest_for(&zip, version, git);
        if tamper {
            std::fs::write(&zip, b"not the zip at all, same length? no").unwrap();
        }
        let sha = sha256.unwrap_or(&m.arm64.sha256);
        let json = serde_json::json!({"format": 1, "version": m.version, "git": m.git, "published": "", "arm64": {"url": m.arm64.url, "size": m.arm64.size, "sha256": sha}, "min_protocol": 1}).to_string();
        std::fs::write(site.root.join("manifest.json"), &json).unwrap();
        std::fs::write(site.root.join("manifest.json.sig"), test_sign(&site.sk, json.as_bytes())).unwrap();
        std::fs::remove_dir_all(site.root.join("src")).unwrap();
    }

    fn site() -> Site {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("site");
        std::fs::create_dir_all(&root).unwrap();
        let (sk, pk) = test_keygen();
        Site { base: format!("file://{}/", root.display()), root, _dir: dir, sk, pk }
    }

    fn fetcher(s: &Site) -> Fetcher {
        Fetcher { base: s.base.clone(), curl: PathBuf::from("/usr/bin/curl") }
    }

    fn ver(major: u64, minor: u64, patch: u64) -> Version {
        Version { major, minor, patch, git: "x".into() }
    }

    fn check(s: &Site, dir: &Path, running: &Version, bad: &mut Bad, staged: Option<&Version>) -> Outcome {
        run_check(&fetcher(s), dir, &[&s.pk], running, bad, Path::new("/usr/bin/ditto"), staged)
    }

    #[test]
    fn a_newer_signed_release_is_staged_and_the_same_one_is_not_staged_twice() {
        let s = site();
        publish(&s, "0.2.1", "abc1234", false);
        let dir = tempfile::tempdir().unwrap();
        let running = Version { major: 0, minor: 2, patch: 0, git: "zzz".into() };
        let mut bad = Bad::new();
        match check(&s, dir.path(), &running, &mut bad, None) {
            Outcome::Staged(st) => assert_eq!(st.version, "0.2.1"),
            other => panic!("{other:?}"),
        }
        let staged = Version { major: 0, minor: 2, patch: 1, git: "abc1234".into() };
        assert!(matches!(check(&s, dir.path(), &running, &mut bad, Some(&staged)), Outcome::Nothing { .. }), "already staged: nothing to do");
        let newer = Version { major: 0, minor: 2, patch: 1, git: "abc1234".into() };
        assert!(matches!(check(&s, dir.path(), &newer, &mut bad, None), Outcome::Nothing { .. }), "running it already");
    }

    #[test]
    fn a_manifest_must_be_newer_than_what_is_staged_too() {
        let s = site();
        let dir = tempfile::tempdir().unwrap();
        let mut bad = Bad::new();
        publish(&s, "0.3.0", "c", false);
        assert!(matches!(check(&s, dir.path(), &ver(0, 1, 0), &mut bad, None), Outcome::Staged(_)));
        let staged = ver(0, 3, 0);
        publish(&s, "0.2.0", "b", false);
        assert!(matches!(check(&s, dir.path(), &ver(0, 1, 0), &mut bad, Some(&staged)), Outcome::Nothing { .. }), "a replayed older manifest");
        assert!(dir.path().join("0.3.0/Arkos IDE.app").is_dir(), "the newer staged release stays");
        assert!(!dir.path().join("0.2.0").exists());
        publish(&s, "0.4.0", "d", false);
        assert!(matches!(check(&s, dir.path(), &ver(0, 1, 0), &mut bad, Some(&staged)), Outcome::Staged(st) if st.version == "0.4.0"));
        assert!(dir.path().join("0.4.0/Arkos IDE.app").is_dir());
        assert!(!dir.path().join("0.3.0").exists(), "replaced");
    }

    #[test]
    fn a_bad_signature_stages_nothing_and_reads_nothing_else() {
        let s = site();
        publish(&s, "0.2.1", "abc1234", false);
        let (_, other_pk) = test_keygen();
        let dir = tempfile::tempdir().unwrap();
        let running = Version { major: 0, minor: 1, patch: 0, git: "x".into() };
        let mut bad = Bad::new();
        match run_check(&fetcher(&s), dir.path(), &[&other_pk], &running, &mut bad, Path::new("/usr/bin/ditto"), None) {
            Outcome::Invalid(e) => assert!(e.contains("signature"), "{e}"),
            other => panic!("{other:?}"),
        }
        assert!(bad.is_empty());
        assert!(!dir.path().join("0.2.1").exists());
        assert!(!dir.path().join("bundle.zip").exists(), "no download after a bad signature");
    }

    #[test]
    fn a_zip_with_the_wrong_hash_is_deleted_and_the_version_marked_bad() {
        let s = site();
        publish(&s, "0.2.1", "abc1234", true);
        let dir = tempfile::tempdir().unwrap();
        let running = ver(0, 1, 0);
        let mut bad = Bad::new();
        let first = match check(&s, dir.path(), &running, &mut bad, None) {
            Outcome::Failed(e) => e,
            other => panic!("{other:?}"),
        };
        assert!(first.contains("sha256") || first.contains("size"), "{first}");
        assert_eq!(bad.len(), 1);
        assert!(bad.keys().all(|k| k.starts_with("0.2.1 abc1234 ")), "{bad:?}");
        assert!(!dir.path().join("0.2.1").exists());
        assert!(!dir.path().join("0.2.1.zip").exists());
        match check(&s, dir.path(), &running, &mut bad, None) {
            Outcome::Skipped(e) => assert_eq!(e, first, "marked bad: not fetched again, the first failure still reported"),
            other => panic!("{other:?}"),
        }
        let mut u = UpdateState { last_error: Some(first.clone()), ..Default::default() };
        assert_eq!(record(&mut u, Outcome::Skipped(first.clone())), None);
        assert_eq!(u.last_error.as_deref(), Some(first.as_str()), "a skip keeps the error");
        record(&mut u, nothing());
        assert_eq!(u.last_error, None, "a check that found nothing newer clears it");
    }

    #[test]
    fn a_corrected_republish_of_the_same_version_is_fetched_again() {
        let s = site();
        let dir = tempfile::tempdir().unwrap();
        let mut bad = Bad::new();
        publish_as(&s, "0.2.1", "abc1234", false, Some(&"0".repeat(64)));
        assert!(matches!(check(&s, dir.path(), &ver(0, 1, 0), &mut bad, None), Outcome::Failed(e) if e.contains("sha256")));
        assert_eq!(bad.len(), 1);
        publish(&s, "0.2.1", "abc1234", false);
        assert!(matches!(check(&s, dir.path(), &ver(0, 1, 0), &mut bad, None), Outcome::Staged(st) if st.version == "0.2.1"), "a new sha256 is a new release");
    }

    #[test]
    fn a_download_that_fails_in_transport_is_retried_and_reported() {
        let s = site();
        publish(&s, "0.2.1", "abc1234", false);
        std::fs::remove_file(s.root.join("bundle.zip")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut bad = Bad::new();
        let outcome = check(&s, dir.path(), &ver(0, 1, 0), &mut bad, None);
        assert!(matches!(&outcome, Outcome::Transient(e) if e.contains("curl")), "{outcome:?}");
        assert!(bad.is_empty(), "a wifi drop is tried again at the next check");
        assert!(!dir.path().join("0.2.1.zip").exists() && !dir.path().join("0.2.1").exists());
        let mut u = UpdateState::default();
        record(&mut u, outcome);
        assert_eq!(u.last_error, None, "a transport failure is not reported to the windows");
        let mut u = UpdateState { last_error: Some("signature".into()), ..Default::default() };
        record(&mut u, Outcome::Transient("curl: offline".into()));
        assert_eq!(u.last_error.as_deref(), Some("signature"), "and leaves an earlier error as it was");
        assert!(matches!(check(&s, dir.path(), &ver(0, 1, 0), &mut bad, None), Outcome::Transient(_)), "tried again, not skipped");
    }

    fn nothing() -> Outcome {
        Outcome::Nothing { version: "1.0.0".into(), git: "a".into(), staged: None }
    }

    #[test]
    fn a_check_records_when_it_finished_and_what_failed_in_transport() {
        let mut u = UpdateState::default();
        record_at(&mut u, Outcome::Transient("offline".into()), 100);
        assert_eq!((u.last_check_unix, u.transient_error.as_deref(), u.last_ok_unix), (Some(100), Some("offline"), None));
        record_at(&mut u, Outcome::Nothing { version: "1.0.0".into(), git: "a".into(), staged: None }, 200);
        assert_eq!((u.last_check_unix, u.transient_error.as_deref(), u.last_ok_unix), (Some(200), None, Some(200)));
        record_at(&mut u, Outcome::Transient("dns".into()), 300);
        assert_eq!((u.last_check_unix, u.last_ok_unix), (Some(300), Some(200)), "a transient failure keeps the last reach");
    }

    #[test]
    fn a_check_that_reached_a_signed_manifest_marks_updates_reachable() {
        let mut u = UpdateState::default();
        record_at(&mut u, Outcome::Transient("curl: offline".into()), 100);
        assert_eq!((u.last_ok_unix, u.unreachable_for_secs(200)), (None, None), "never reached: nothing to say");
        record_at(&mut u, nothing(), 1_000);
        assert_eq!(u.last_ok_unix, Some(1_000));
        assert_eq!(u.unreachable_for_secs(5_000), Some(0), "the latest check worked");
        record_at(&mut u, Outcome::Transient("curl: offline".into()), 2_000);
        assert_eq!(u.last_ok_unix, Some(1_000), "a transport failure reached nothing");
        assert_eq!(u.unreachable_for_secs(91_000), Some(90_000));
        record_at(&mut u, Outcome::Invalid("signature".into()), 3_000);
        assert_eq!(u.last_ok_unix, Some(1_000), "an unverified manifest is not a reached one");
        record_at(&mut u, Outcome::Failed("sha256".into()), 4_000);
        assert_eq!(u.last_ok_unix, Some(4_000), "the manifest verified; the zip was wrong");
        record_at(&mut u, Outcome::Skipped("sha256".into()), 5_000);
        assert_eq!(u.last_ok_unix, Some(5_000));
        let s = crate::updates::stage::Staged { version: "0.2.1".into(), git: "g".into(), bundle: PathBuf::from("/x") };
        record_at(&mut u, Outcome::Staged(s), 6_000);
        assert_eq!((u.last_ok_unix, u.unreachable_for_secs(7_000)), (Some(6_000), Some(0)));
    }

    #[test]
    fn a_staged_release_not_newer_than_running_is_stale() {
        let s = |v: &str| Staged { version: v.into(), git: "g".into(), bundle: PathBuf::from("/x") };
        let running = ver(0, 2, 6);
        assert!(is_stale(&s("0.2.5"), &running), "older: a downgrade");
        assert!(is_stale(&s("0.2.6"), &running), "the same version");
        assert!(!is_stale(&s("0.2.7"), &running));
        assert!(!is_stale(&s("1.0.0"), &running));
        assert!(is_stale(&s("garbage"), &running), "unreadable: never applied");
    }

    #[test]
    fn a_missing_manifest_is_transient() {
        let s = site();
        let dir = tempfile::tempdir().unwrap();
        let mut bad = Bad::new();
        let outcome = check(&s, dir.path(), &ver(0, 1, 0), &mut bad, None);
        assert!(matches!(&outcome, Outcome::Transient(_)), "{outcome:?}");
        let mut u = UpdateState::default();
        record(&mut u, outcome);
        assert_eq!(u.last_error, None);
    }

    #[test]
    fn a_final_url_off_github_is_refused() {
        // `file://` cannot redirect, so this pins the pure check the download applies to curl's
        // reported final url before the file is read.
        let gh = "https://github.com/csarkosh/ide-arkos/releases/latest/download/manifest.json";
        assert!(!crate::updates::fetch::allowed_final_url(gh, "https://cdn.evil.example/Arkos.zip"));
        assert!(crate::updates::fetch::allowed_final_url(gh, "https://objects.githubusercontent.com/a"));
    }
}
