//! Unpacking a verified zip into the staging folder and checking what came out before any window
//! hears about it.
use super::manifest::Manifest;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    pub version: String,
    pub git: String,
    pub bundle: PathBuf,
}

pub const BUNDLE_NAME: &str = "Arkos IDE.app";

/// How long ditto may take to unpack a release (at most `MAX_ZIP_BYTES` of zip).
const DITTO_TIMEOUT: Duration = Duration::from_secs(120);

/// `<dir>/<version>/Arkos IDE.app` from the zip, or an error with the folder removed.
pub fn stage(dir: &Path, m: &Manifest, zip: &Path, ditto: &Path) -> Result<Staged, String> {
    let into = dir.join(&m.version);
    let _ = std::fs::remove_dir_all(&into);
    std::fs::create_dir_all(&into).map_err(|e| format!("staging: {e}"))?;
    let result = (|| {
        let st = super::run_bounded(Command::new(ditto).args(["-x", "-k"]).arg(zip).arg(&into), DITTO_TIMEOUT).map_err(|e| format!("ditto: {e}"))?;
        if !st.status.success() {
            return Err(format!("ditto: {}", String::from_utf8_lossy(&st.stderr).trim()));
        }
        let bundle = into.join(BUNDLE_NAME);
        check_bundle(&bundle, &m.version)?;
        Ok(Staged { version: m.version.clone(), git: m.git.clone(), bundle })
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&into);
    }
    result
}

/// Both executables present and runnable, and the daemon inside says the manifest's version.
pub fn check_bundle(bundle: &Path, want_version: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    for name in ["arkos", "arkosd"] {
        let p = bundle.join("Contents/MacOS").join(name);
        let meta = std::fs::metadata(&p).map_err(|e| format!("{name}: {e}"))?;
        if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
            return Err(format!("{name}: not an executable file"));
        }
    }
    let said = probe_version(&bundle.join("Contents/MacOS/arkosd"), Duration::from_secs(2)).ok_or("the staged arkosd did not print its version")?;
    match super::version::parse_version_string(&said) {
        Some(v) if format!("{}.{}.{}", v.major, v.minor, v.patch) == want_version => Ok(()),
        _ => Err(format!("the staged arkosd says {said:?}, the manifest {want_version}")),
    }
}

/// `exe --version`, within `timeout`, else None (the child is killed).
pub fn probe_version(exe: &Path, timeout: Duration) -> Option<String> {
    let out = super::run_bounded(Command::new(exe).arg("--version"), timeout).ok()?;
    if !out.status.success() {
        return None;
    }
    let out = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!out.is_empty()).then_some(out)
}

/// Removes every version folder in `dir` except the one holding `keep` (the staged bundle, or
/// its version folder itself); files at the top level stay.
pub fn clean_except(dir: &Path, keep: Option<&Path>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() && !keep.is_some_and(|k| k.starts_with(&p)) {
            let _ = std::fs::remove_dir_all(&p);
        }
    }
}

/// Removes every version folder in `dir` whose name is a version not newer than `running`;
/// returns the versions removed. Folders whose name is no version, and files, stay.
pub fn remove_not_newer(dir: &Path, running: &super::version::Version) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut removed = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        let Some(name) = p.file_name().and_then(|n| n.to_str()).map(str::to_string) else { continue };
        let Some((major, minor, patch)) = super::version::parse_semver(&name) else { continue };
        let v = super::version::Version { major, minor, patch, git: String::new() };
        if p.is_dir() && !v.is_newer_than(running) {
            match std::fs::remove_dir_all(&p) {
                Ok(()) => removed.push(name),
                Err(e) => crate::journal::note(format_args!("staged {name} is not newer than {}.{}.{} but cannot be deleted: {e}", running.major, running.minor, running.patch)),
            }
        }
    }
    removed.sort();
    removed
}

/// The version an app bundle says it is: `CFBundleShortVersionString` in its `Info.plist` (XML,
/// as `packaging/Info.plist.in` writes it). None when missing or not a version.
pub fn installed_version(bundle: &Path) -> Option<super::version::Version> {
    let text = std::fs::read_to_string(bundle.join("Contents/Info.plist")).ok()?;
    let after = &text[text.find("<key>CFBundleShortVersionString</key>")? + "<key>CFBundleShortVersionString</key>".len()..];
    let value = after.trim_start().strip_prefix("<string>")?;
    let value = &value[..value.find("</string>")?];
    let (major, minor, patch) = super::version::parse_semver(value.trim())?;
    Some(super::version::Version { major, minor, patch, git: String::new() })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::updates::manifest::{Asset, Manifest};

    pub fn fake_bundle(dir: &Path, version: &str, git: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let app = dir.join("Arkos IDE.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::write(app.join("Contents/Info.plist"), format!("<plist><dict><key>CFBundleShortVersionString</key><string>{version}</string></dict></plist>")).unwrap();
        for name in ["arkos", "arkosd"] {
            let p = app.join("Contents/MacOS").join(name);
            std::fs::write(&p, format!("#!/bin/sh\n[ \"$1\" = --version ] && echo '{name} {version} ({git})'\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        app
    }

    pub fn zip_of(app: &Path, dest: &Path) {
        let st = std::process::Command::new("/usr/bin/ditto").args(["-c", "-k", "--keepParent"]).arg(app).arg(dest).status().unwrap();
        assert!(st.success());
    }

    pub fn manifest_for(zip: &Path, version: &str, git: &str) -> Manifest {
        Manifest { format: 1, version: version.into(), git: git.into(), published: String::new(), arm64: Asset { url: format!("file://{}", zip.display()), size: std::fs::metadata(zip).unwrap().len(), sha256: crate::updates::verify::sha256_hex(zip).unwrap() }, min_protocol: 1 }
    }

    /// The JSON `manifest::parse` reads, for a manifest written to disk by a test.
    pub fn manifest_json(m: &Manifest) -> Vec<u8> {
        serde_json::json!({"format": m.format, "version": m.version, "git": m.git, "published": m.published, "arm64": {"url": m.arm64.url, "size": m.arm64.size, "sha256": m.arm64.sha256}, "min_protocol": m.min_protocol}).to_string().into_bytes()
    }

    #[test]
    fn a_zip_is_unpacked_checked_and_probed() {
        let src = tempfile::tempdir().unwrap();
        let app = fake_bundle(src.path(), "0.2.1", "abc1234");
        let zip = src.path().join("b.zip");
        zip_of(&app, &zip);
        let m = manifest_for(&zip, "0.2.1", "abc1234");
        let dir = tempfile::tempdir().unwrap();
        let s = stage(dir.path(), &m, &zip, Path::new("/usr/bin/ditto")).unwrap();
        assert_eq!(s.version, "0.2.1");
        assert_eq!(s.bundle, dir.path().join("0.2.1").join("Arkos IDE.app"));
        assert!(s.bundle.join("Contents/MacOS/arkosd").is_file());
        // The wrong version inside: refused and cleaned.
        let wrong = manifest_for(&zip, "0.2.2", "abc1234");
        assert!(stage(dir.path(), &wrong, &zip, Path::new("/usr/bin/ditto")).unwrap_err().contains("0.2.1"));
        assert!(!dir.path().join("0.2.2").exists());
    }

    #[test]
    fn check_bundle_wants_both_executables_and_the_version() {
        let dir = tempfile::tempdir().unwrap();
        let app = fake_bundle(dir.path(), "0.2.1", "x");
        assert_eq!(check_bundle(&app, "0.2.1"), Ok(()));
        assert!(check_bundle(&app, "0.3.0").unwrap_err().contains("0.2.1"));
        std::fs::remove_file(app.join("Contents/MacOS/arkos")).unwrap();
        assert!(check_bundle(&app, "0.2.1").unwrap_err().contains("arkos"));
    }

    #[test]
    fn clean_except_keeps_only_the_staged_version() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("0.1.9")).unwrap();
        std::fs::create_dir_all(dir.path().join("0.2.1")).unwrap();
        std::fs::write(dir.path().join("manifest.json"), b"x").unwrap();
        clean_except(dir.path(), Some(&dir.path().join("0.2.1")));
        assert!(!dir.path().join("0.1.9").exists());
        assert!(dir.path().join("0.2.1").exists());
        assert!(dir.path().join("manifest.json").exists(), "files at the top stay");
        clean_except(dir.path(), None);
        assert!(!dir.path().join("0.2.1").exists());
    }

    #[test]
    fn the_installed_version_is_read_from_the_info_plist() {
        let dir = tempfile::tempdir().unwrap();
        let app = fake_bundle(dir.path(), "0.2.6", "x");
        assert_eq!(installed_version(&app).map(|v| (v.major, v.minor, v.patch)), Some((0, 2, 6)));
        std::fs::write(app.join("Contents/Info.plist"), "<plist><dict>\n\t<key>CFBundleShortVersionString</key>\n\t<string>1.2.3</string>\n</dict></plist>").unwrap();
        assert_eq!(installed_version(&app).map(|v| v.patch), Some(3), "the template's layout");
        std::fs::write(app.join("Contents/Info.plist"), "<plist><dict><key>CFBundleShortVersionString</key><string>@VERSION@</string></dict></plist>").unwrap();
        assert_eq!(installed_version(&app), None);
        assert_eq!(installed_version(&dir.path().join("missing.app")), None);
    }

    #[test]
    fn remove_not_newer_keeps_newer_versions_and_everything_else() {
        let dir = tempfile::tempdir().unwrap();
        for d in ["0.2.5", "0.2.6", "0.2.7", "scratch"] {
            std::fs::create_dir_all(dir.path().join(d)).unwrap();
        }
        std::fs::write(dir.path().join("0.1.0"), b"a file").unwrap();
        let running = super::super::version::Version { major: 0, minor: 2, patch: 6, git: "x".into() };
        assert_eq!(remove_not_newer(dir.path(), &running), ["0.2.5", "0.2.6"]);
        assert!(dir.path().join("0.2.7").is_dir() && dir.path().join("scratch").is_dir() && dir.path().join("0.1.0").is_file());
    }
}
