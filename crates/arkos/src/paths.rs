//! Where arkos keeps its socket and workspace files (spec 2.1, 4).
use std::path::{Path, PathBuf};

// The config folder and Application Support moved to the shared crate, so the daemon reads the
// same `[telemetry]` table; the app's callers keep their `paths::` names.
pub use ::telemetry::{app_support, config_dir, config_file};

pub fn themes_dir() -> PathBuf {
    config_dir().join("themes")
}

/// Holds the unix seconds of this install's first run (`startup::installed_since`).
pub fn installed_file() -> PathBuf {
    app_support().join("installed")
}

/// The folders opened recently, for the launch chooser (`recent.rs`).
pub fn recent_file() -> PathBuf {
    app_support().join("recent.json")
}

/// The version the last launch ran, so a relaunch can say "Updated to X" (notices.rs).
pub fn last_version_file() -> PathBuf {
    app_support().join("last_version")
}

/// Where unsaved markdown edits go when arkos has to drop them (a forced quit, macOS's terminate,
/// an update's relaunch): `viewer::write_recovery` creates it on demand.
pub fn recovered_dir() -> PathBuf {
    app_support().join("recovered")
}

/// The app's telemetry folder: the journals, the queue and `incidents/`, the crash reports the
/// panic hook writes for the dialog.
pub fn telemetry_dir() -> PathBuf {
    app_support().join("telemetry")
}

/// The telemetry folder of the daemon on `socket`: the socket folder's `telemetry/`, the
/// daemon's own rule, which is `telemetry_dir()` for the real socket. The command line and the
/// doctor read it there, so a test's socket in a temporary folder brings its own.
pub fn telemetry_dir_beside(socket: &Path) -> PathBuf {
    socket.parent().unwrap_or(Path::new(".")).join("telemetry")
}

pub fn socket_path() -> PathBuf {
    app_support().join("arkosd.sock")
}

/// FNV-1a over the path bytes. Rust's `DefaultHasher` is not stable across releases, which would
/// orphan every workspace file on a toolchain upgrade.
pub(crate) fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// The folder holding every project's workspace file.
pub fn workspaces_dir() -> PathBuf {
    app_support().join("workspaces")
}

pub fn workspace_file(project: &Path) -> PathBuf {
    workspace_file_in(&workspaces_dir(), project)
}

/// `workspace_file` under `dir`, so tests can use a temporary folder.
pub fn workspace_file_in(dir: &Path, project: &Path) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    dir.join(format!("{:016x}.json", fnv1a(project.as_os_str().as_bytes())))
}

/// The name milestone 1 gave a project's workspace file: `DefaultHasher` over `Path::hash`, the
/// exact code it shipped with. Only read, to migrate it to `workspace_file`.
pub fn legacy_workspace_file(project: &Path) -> PathBuf {
    legacy_workspace_file_in(&workspaces_dir(), project)
}

/// `legacy_workspace_file` under `dir`, so tests can use a temporary folder.
pub fn legacy_workspace_file_in(dir: &Path, project: &Path) -> PathBuf {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    project.hash(&mut h);
    dir.join(format!("{:016x}.json", h.finish()))
}

pub fn daemon_exe() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("arkosd")))
        .unwrap_or_else(|| PathBuf::from("arkosd"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_file_name_is_stable_across_builds() {
        // FNV-1a of the path bytes; the value is pinned so a toolchain change cannot orphan files.
        let f = workspace_file(Path::new("/tmp/a"));
        assert_eq!(f.file_name().unwrap().to_str().unwrap(), "6cc13bddf2746ce7.json");
        assert_ne!(workspace_file(Path::new("/tmp/b")), f);
        assert!(f.to_string_lossy().contains("workspaces"));
    }

    #[test]
    fn legacy_name_is_milestone_1s() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let project = Path::new("/tmp/a");
        let mut h = DefaultHasher::new();
        project.hash(&mut h);
        let want = format!("{:016x}.json", h.finish());
        let f = legacy_workspace_file(project);
        assert_eq!(f.file_name().unwrap().to_str().unwrap(), want);
        assert_eq!(f.parent(), workspace_file(project).parent(), "both live in the same folder");
    }

    #[test]
    fn themes_live_in_the_config_folder() {
        assert_eq!(themes_dir(), config_dir().join("themes"));
        assert!(config_file().ends_with("arkos/config.toml"));
    }

    #[test]
    fn last_version_lives_in_app_support() {
        assert!(last_version_file().ends_with("arkos/last_version"));
    }

    #[test]
    fn recovered_edits_live_in_app_support() {
        assert!(recovered_dir().ends_with("arkos/recovered"));
    }

    #[test]
    fn the_install_marker_lives_in_app_support() {
        assert_eq!(installed_file(), app_support().join("installed"));
    }
}
