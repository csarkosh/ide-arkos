//! The bundle swap (auto-update spec, section 4): two renames on one volume, the old bundle kept
//! as `<bundle>.old` until a window confirms the new one runs.
use std::path::{Path, PathBuf};

pub fn old_of(bundle: &Path) -> PathBuf {
    with_suffix(bundle, ".old")
}

fn new_of(bundle: &Path) -> PathBuf {
    with_suffix(bundle, ".new")
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

/// Removes a stale `.old` or `.new` left by an interrupted swap. The installed bundle is never
/// touched.
pub fn cleanup_leftovers(bundle: &Path) {
    let _ = std::fs::remove_dir_all(old_of(bundle));
    let _ = std::fs::remove_dir_all(new_of(bundle));
}

/// `bundle` becomes `<bundle>.old` and `new` becomes `bundle`. If the second rename fails the
/// first is undone, so the app is never missing.
pub fn swap_in(bundle: &Path, new: &Path) -> Result<(), String> {
    cleanup_leftovers(bundle);
    let old = old_of(bundle);
    std::fs::rename(bundle, &old).map_err(|e| format!("rename {} to .old: {e}", bundle.display()))?;
    if let Err(e) = std::fs::rename(new, bundle) {
        let undo = std::fs::rename(&old, bundle);
        return Err(match undo {
            Ok(()) => format!("rename {} into place: {e} (the old bundle is back)", new.display()),
            Err(u) => format!("rename {} into place: {e}; and putting the old bundle back failed too: {u}", new.display()),
        });
    }
    Ok(())
}

pub fn finish(bundle: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(old_of(bundle)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        r => r,
    }
}

/// Puts `<bundle>.old` back in place of `bundle` (which is removed).
pub fn rollback(bundle: &Path) -> Result<(), String> {
    let old = old_of(bundle);
    if !old.is_dir() {
        return Err("no previous bundle to roll back to".into());
    }
    let failed = new_of(bundle);
    let _ = std::fs::remove_dir_all(&failed);
    std::fs::rename(bundle, &failed).map_err(|e| format!("move the failed bundle aside: {e}"))?;
    std::fs::rename(&old, bundle).map_err(|e| format!("rename .old back: {e}"))?;
    let _ = std::fs::remove_dir_all(&failed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle(dir: &Path, name: &str, tag: &str) -> PathBuf {
        let b = dir.join(name);
        std::fs::create_dir_all(b.join("Contents/MacOS")).unwrap();
        std::fs::write(b.join("Contents/MacOS/arkosd"), tag).unwrap();
        b
    }

    #[test]
    fn the_swap_is_two_renames_and_keeps_the_old_bundle_until_finish() {
        let dir = tempfile::tempdir().unwrap();
        let installed = bundle(dir.path(), "Arkos IDE.app", "old");
        let new = bundle(&dir.path().join("staging"), "Arkos IDE.app", "new");
        swap_in(&installed, &new).unwrap();
        assert_eq!(std::fs::read_to_string(installed.join("Contents/MacOS/arkosd")).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(old_of(&installed).join("Contents/MacOS/arkosd")).unwrap(), "old");
        assert!(!new.exists(), "moved, not copied");
        finish(&installed).unwrap();
        assert!(!old_of(&installed).exists());
    }

    #[test]
    fn rollback_puts_the_old_bundle_back() {
        let dir = tempfile::tempdir().unwrap();
        let installed = bundle(dir.path(), "Arkos IDE.app", "old");
        let new = bundle(&dir.path().join("staging"), "Arkos IDE.app", "new");
        swap_in(&installed, &new).unwrap();
        rollback(&installed).unwrap();
        assert_eq!(std::fs::read_to_string(installed.join("Contents/MacOS/arkosd")).unwrap(), "old");
        assert!(!old_of(&installed).exists());
        assert!(rollback(&installed).is_err(), "nothing to roll back to");
    }

    #[test]
    fn a_failed_second_rename_restores_the_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let installed = bundle(dir.path(), "Arkos IDE.app", "old");
        // The new bundle is on another "volume" for rename: a path that does not exist makes the
        // second rename fail after the first succeeded.
        let missing = dir.path().join("staging/Arkos IDE.app");
        let err = swap_in(&installed, &missing).unwrap_err();
        assert!(err.contains("rename"), "{err}");
        assert_eq!(std::fs::read_to_string(installed.join("Contents/MacOS/arkosd")).unwrap(), "old", "restored from .old");
        assert!(!old_of(&installed).exists());
    }

    #[test]
    fn a_stale_old_or_new_is_removed_first() {
        let dir = tempfile::tempdir().unwrap();
        let installed = bundle(dir.path(), "Arkos IDE.app", "old");
        bundle(dir.path(), "Arkos IDE.app.old", "stale");
        bundle(dir.path(), "Arkos IDE.app.new", "stale");
        cleanup_leftovers(&installed);
        assert!(!old_of(&installed).exists());
        assert!(!dir.path().join("Arkos IDE.app.new").exists());
        assert!(installed.exists());
    }
}
