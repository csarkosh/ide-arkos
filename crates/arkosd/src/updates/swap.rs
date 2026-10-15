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

/// When the bundle is missing (a swap or rollback died between its renames), puts a leftover
/// back in its place: `.old`, or failing that `.new`. Removes nothing.
pub fn restore_missing(bundle: &Path) {
    if !bundle.exists() {
        for leftover in [old_of(bundle), new_of(bundle)] {
            if leftover.is_dir() && std::fs::rename(&leftover, bundle).is_ok() {
                eprintln!("arkosd: the app bundle was missing; restored it from {}", leftover.display());
                break;
            }
        }
    }
}

/// Removes a stale `.old` or `.new` left by an interrupted swap. The installed bundle is never
/// touched; when it is missing the leftover is put back in its place instead (`restore_missing`).
pub fn cleanup_leftovers(bundle: &Path) {
    restore_missing(bundle);
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

/// Puts `<bundle>.old` back in place of `bundle` (which is removed). If the second rename fails
/// the failed bundle is put back, so the app is never missing; if `bundle` is already missing (an
/// earlier rollback died between its renames) `.old` goes straight into place.
pub fn rollback(bundle: &Path) -> Result<(), String> {
    rollback_with(bundle, |from, to| std::fs::rename(from, to))
}

/// `rollback` with the rename given, so a test can make one of them fail.
fn rollback_with(bundle: &Path, rename: impl Fn(&Path, &Path) -> std::io::Result<()>) -> Result<(), String> {
    let old = old_of(bundle);
    if !old.is_dir() {
        return Err("no previous bundle to roll back to".into());
    }
    if !bundle.exists() {
        return rename(&old, bundle).map_err(|e| format!("rename .old back: {e}"));
    }
    let failed = new_of(bundle);
    let _ = std::fs::remove_dir_all(&failed);
    rename(bundle, &failed).map_err(|e| format!("move the failed bundle aside: {e}"))?;
    if let Err(e) = rename(&old, bundle) {
        return Err(match rename(&failed, bundle) {
            Ok(()) => format!("rename .old back: {e} (the new bundle stays in place)"),
            Err(u) => format!("rename .old back: {e}; and putting the new bundle back failed too: {u}"),
        });
    }
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

    #[test]
    fn a_missing_bundle_is_restored_from_old_rather_than_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let installed = dir.path().join("Arkos IDE.app");
        bundle(dir.path(), "Arkos IDE.app.old", "old");
        cleanup_leftovers(&installed);
        assert_eq!(std::fs::read_to_string(installed.join("Contents/MacOS/arkosd")).unwrap(), "old");
        assert!(!old_of(&installed).exists());
    }

    #[test]
    fn rollback_with_the_bundle_already_missing_puts_old_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let installed = dir.path().join("Arkos IDE.app");
        bundle(dir.path(), "Arkos IDE.app.old", "old");
        bundle(dir.path(), "Arkos IDE.app.new", "new");
        rollback(&installed).unwrap();
        assert_eq!(std::fs::read_to_string(installed.join("Contents/MacOS/arkosd")).unwrap(), "old");
        assert!(!old_of(&installed).exists());
    }

    #[test]
    fn a_failed_rename_of_old_back_puts_the_new_bundle_back() {
        let dir = tempfile::tempdir().unwrap();
        let installed = bundle(dir.path(), "Arkos IDE.app", "new");
        bundle(dir.path(), "Arkos IDE.app.old", "old");
        let old = old_of(&installed);
        let err = rollback_with(&installed, |from, to| {
            if from == old {
                Err(std::io::Error::other("injected"))
            } else {
                std::fs::rename(from, to)
            }
        })
        .unwrap_err();
        assert!(err.contains("the new bundle stays in place"), "{err}");
        assert_eq!(std::fs::read_to_string(installed.join("Contents/MacOS/arkosd")).unwrap(), "new", "the app is never missing");
        assert!(old.is_dir(), "kept for another try");
    }
}
