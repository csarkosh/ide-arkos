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
    /// The check worked and found nothing newer than what runs and what is staged.
    Nothing,
    Staged(Staged),
    /// The manifest or the zip could not be fetched (offline, a captive portal, a timeout): logged
    /// and tried again at the next check, never reported to the windows.
    Transient(String),
    /// The release itself is wrong (signature, size, sha256) or staging failed: reported.
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
        return Outcome::Failed(format!("updates dir: {e}"));
    }
    let (bytes, sig) = match f.fetch_manifest(dir) {
        Ok(x) => x,
        Err(e) => return Outcome::Transient(e),
    };
    if let Err(e) = verify::verify_manifest(&bytes, &sig, keys) {
        return Outcome::Failed(e);
    }
    let m = match manifest::parse(&bytes) {
        Ok(m) => m,
        Err(e) => return Outcome::Failed(e),
    };
    let Some(v) = m.version() else { return Outcome::Failed("manifest: bad version".into()) };
    if !v.is_newer_than(running) || staged.is_some_and(|s| !v.is_newer_than(s)) {
        return Outcome::Nothing;
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

/// A check's outcome into the shared state. Returns the event to tell the windows, if any.
/// `last_error` is cleared only by a check that positively found nothing newer or staged one; a
/// skipped bad release keeps its original failure reported, and a transport failure leaves it as
/// it was (a wifi drop is not news).
pub fn record(u: &mut UpdateState, outcome: Outcome) -> Option<protocol::StagedUpdate> {
    match outcome {
        Outcome::Nothing => {
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
        Outcome::Failed(e) => {
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
pub fn start(state: Shared, f: Fetcher, dir: PathBuf, keys: Vec<String>, every: Duration, ditto: PathBuf) {
    state.lock().unwrap().update.polling = true;
    std::thread::Builder::new()
        .name("updates".into())
        .spawn(move || {
            let running = super::version::parse_version_string(&crate::version_string()).expect("our own version string parses");
            let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
            let mut bad = Bad::new();
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
                    let outcome = run_check(&f, &dir, &keys, &running, &mut bad, &ditto, staged.as_ref());
                    let mut st = state.lock().unwrap();
                    st.update.checking = false;
                    if let Some(update) = record(&mut st.update, outcome) {
                        crate::server::broadcast(&st, protocol::Event::UpdateStaged { update });
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
        assert!(matches!(check(&s, dir.path(), &running, &mut bad, Some(&staged)), Outcome::Nothing), "already staged: nothing to do");
        let newer = Version { major: 0, minor: 2, patch: 1, git: "abc1234".into() };
        assert!(matches!(check(&s, dir.path(), &newer, &mut bad, None), Outcome::Nothing), "running it already");
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
        assert!(matches!(check(&s, dir.path(), &ver(0, 1, 0), &mut bad, Some(&staged)), Outcome::Nothing), "a replayed older manifest");
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
            Outcome::Failed(e) => assert!(e.contains("signature"), "{e}"),
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
        record(&mut u, Outcome::Nothing);
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
