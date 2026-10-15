//! The check: manifest, signature, comparison, download, hash, stage. Pure apart from the
//! processes it runs, so the tests drive it over `file://`.
use super::fetch::Fetcher;
use super::stage::{stage, Staged};
use super::version::Version;
use super::{manifest, verify, MAX_ZIP_BYTES};
use crate::server::Shared;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug)]
pub enum Outcome {
    Nothing,
    Staged(Staged),
    Failed(String),
}

fn bad_key(version: &str, git: &str) -> String {
    format!("{version} {git}")
}

/// The updates folder, private to this user: what is staged there is run later.
fn private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

/// One check. `staged` is what is already staged (its bundle path), so the same release is not
/// fetched twice. A failure after the signature marks the version bad.
pub fn run_check(f: &Fetcher, dir: &Path, keys: &[&str], running: &Version, bad: &mut HashSet<String>, ditto: &Path, staged: Option<&Path>) -> Outcome {
    if let Err(e) = private_dir(dir) {
        return Outcome::Failed(format!("updates dir: {e}"));
    }
    let (bytes, sig) = match f.fetch_manifest(dir) {
        Ok(x) => x,
        Err(e) => return Outcome::Failed(e),
    };
    if let Err(e) = verify::verify_manifest(&bytes, &sig, keys) {
        return Outcome::Failed(e);
    }
    let m = match manifest::parse(&bytes) {
        Ok(m) => m,
        Err(e) => return Outcome::Failed(e),
    };
    let Some(v) = m.version() else { return Outcome::Failed("manifest: bad version".into()) };
    if !v.is_newer_than(running) || bad.contains(&bad_key(&m.version, &m.git)) {
        return Outcome::Nothing;
    }
    if staged.is_some_and(|s| s.parent().and_then(|p| p.file_name()) == Some(std::ffi::OsStr::new(&m.version))) {
        return Outcome::Nothing;
    }
    if m.arm64.size > MAX_ZIP_BYTES {
        bad.insert(bad_key(&m.version, &m.git));
        return Outcome::Failed(format!("release {} is {} bytes, over the {MAX_ZIP_BYTES} cap", m.version, m.arm64.size));
    }
    let zip = dir.join(format!("{}.zip", m.version));
    let result = (|| {
        f.download(&m.arm64.url, m.arm64.size, &zip)?;
        let got = verify::sha256_hex(&zip).map_err(|e| format!("sha256: {e}"))?;
        if !got.eq_ignore_ascii_case(&m.arm64.sha256) {
            return Err(format!("sha256 of the download is {got}, the manifest says {}", m.arm64.sha256));
        }
        stage(dir, &m, &zip, ditto)
    })();
    let _ = std::fs::remove_file(&zip);
    match result {
        Ok(s) => {
            super::stage::clean_except(dir, Some(&s.bundle));
            Outcome::Staged(s)
        }
        Err(e) => {
            bad.insert(bad_key(&m.version, &m.git));
            Outcome::Failed(e)
        }
    }
}

/// The poll thread: every `every`, or sooner when a window asked (`check_now`). The state lock
/// is never held across a check.
pub fn start(state: Shared, f: Fetcher, dir: PathBuf, keys: Vec<String>, every: Duration, ditto: PathBuf) {
    std::thread::Builder::new()
        .name("updates".into())
        .spawn(move || {
            let running = super::version::parse_version_string(&crate::version_string()).expect("our own version string parses");
            let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
            let mut bad = HashSet::new();
            let mut last = None::<std::time::Instant>;
            loop {
                let (due, staged) = {
                    let mut st = state.lock().unwrap();
                    let due = st.update.check_now || last.is_none_or(|t| t.elapsed() >= every);
                    st.update.check_now = false;
                    if due {
                        st.update.checking = true;
                    }
                    (due, st.update.staged.as_ref().map(|s| s.bundle.clone()))
                };
                if due {
                    last = Some(std::time::Instant::now());
                    let outcome = run_check(&f, &dir, &keys, &running, &mut bad, &ditto, staged.as_deref());
                    let mut st = state.lock().unwrap();
                    st.update.checking = false;
                    match outcome {
                        Outcome::Nothing => st.update.last_error = None,
                        Outcome::Staged(s) => {
                            st.update.last_error = None;
                            let update = protocol::StagedUpdate { version: s.version.clone(), git: s.git.clone() };
                            st.update.staged = Some(s);
                            crate::server::broadcast(&st, protocol::Event::UpdateStaged { update });
                        }
                        Outcome::Failed(e) => {
                            eprintln!("arkosd: update check: {e}");
                            st.update.last_error = Some(e);
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

    struct Site {
        _dir: tempfile::TempDir,
        base: String,
        root: PathBuf,
        sk: String,
        pk: String,
    }

    fn publish(site: &Site, version: &str, git: &str, tamper: bool) {
        let app = fake_bundle(&site.root.join("src"), version, git);
        let zip = site.root.join("bundle.zip");
        zip_of(&app, &zip);
        let m = manifest_for(&zip, version, git);
        if tamper {
            std::fs::write(&zip, b"not the zip at all, same length? no").unwrap();
            let _ = m.arm64.size;
        }
        let json = serde_json::json!({"format": 1, "version": m.version, "git": m.git, "published": "", "arm64": {"url": m.arm64.url, "size": m.arm64.size, "sha256": m.arm64.sha256}, "min_protocol": 1}).to_string();
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

    #[test]
    fn a_newer_signed_release_is_staged_and_the_same_one_is_not_staged_twice() {
        let s = site();
        publish(&s, "0.2.1", "abc1234", false);
        let dir = tempfile::tempdir().unwrap();
        let running = Version { major: 0, minor: 2, patch: 0, git: "zzz".into() };
        let mut bad = HashSet::new();
        match run_check(&fetcher(&s), dir.path(), &[&s.pk], &running, &mut bad, Path::new("/usr/bin/ditto"), None) {
            Outcome::Staged(st) => assert_eq!(st.version, "0.2.1"),
            other => panic!("{other:?}"),
        }
        let staged = dir.path().join("0.2.1").join("Arkos IDE.app");
        assert!(matches!(run_check(&fetcher(&s), dir.path(), &[&s.pk], &running, &mut bad, Path::new("/usr/bin/ditto"), Some(&staged)), Outcome::Nothing), "already staged: nothing to do");
        let newer = Version { major: 0, minor: 2, patch: 1, git: "abc1234".into() };
        assert!(matches!(run_check(&fetcher(&s), dir.path(), &[&s.pk], &newer, &mut bad, Path::new("/usr/bin/ditto"), None), Outcome::Nothing), "running it already");
    }

    #[test]
    fn a_bad_signature_stages_nothing_and_reads_nothing_else() {
        let s = site();
        publish(&s, "0.2.1", "abc1234", false);
        let (_, other_pk) = test_keygen();
        let dir = tempfile::tempdir().unwrap();
        let running = Version { major: 0, minor: 1, patch: 0, git: "x".into() };
        let mut bad = HashSet::new();
        match run_check(&fetcher(&s), dir.path(), &[&other_pk], &running, &mut bad, Path::new("/usr/bin/ditto"), None) {
            Outcome::Failed(e) => assert!(e.contains("signature"), "{e}"),
            other => panic!("{other:?}"),
        }
        assert!(!dir.path().join("0.2.1").exists());
        assert!(!dir.path().join("bundle.zip").exists(), "no download after a bad signature");
    }

    #[test]
    fn a_zip_with_the_wrong_hash_is_deleted_and_the_version_marked_bad() {
        let s = site();
        publish(&s, "0.2.1", "abc1234", true);
        let dir = tempfile::tempdir().unwrap();
        let running = Version { major: 0, minor: 1, patch: 0, git: "x".into() };
        let mut bad = HashSet::new();
        match run_check(&fetcher(&s), dir.path(), &[&s.pk], &running, &mut bad, Path::new("/usr/bin/ditto"), None) {
            Outcome::Failed(e) => assert!(e.contains("sha256") || e.contains("size"), "{e}"),
            other => panic!("{other:?}"),
        }
        assert!(bad.contains("0.2.1 abc1234"));
        assert!(!dir.path().join("0.2.1").exists());
        assert!(matches!(run_check(&fetcher(&s), dir.path(), &[&s.pk], &running, &mut bad, Path::new("/usr/bin/ditto"), None), Outcome::Nothing), "marked bad: not fetched again");
    }

    #[test]
    fn a_final_url_off_github_is_refused() {
        // `file://` cannot redirect, so this pins the pure check the download applies to curl's
        // reported final url before the file is read.
        assert!(!crate::updates::fetch::allowed_final_url("https://cdn.evil.example/Arkos.zip"));
        assert!(crate::updates::fetch::allowed_final_url("https://objects.githubusercontent.com/a"));
    }
}
