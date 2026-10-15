//! Where arkos keeps its socket and workspace files (spec 2.1, 4).
use std::path::{Path, PathBuf};

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").expect("HOME is set"))
}

/// `$XDG_CONFIG_HOME/arkos` when that is set and non-empty, else `~/.config/arkos` (spec 7).
pub fn config_dir_from(xdg: Option<&std::ffi::OsStr>, home: &Path) -> PathBuf {
    match xdg {
        Some(x) if !x.is_empty() => PathBuf::from(x).join("arkos"),
        _ => home.join(".config").join("arkos"),
    }
}

pub fn config_dir() -> PathBuf {
    config_dir_from(std::env::var_os("XDG_CONFIG_HOME").as_deref(), &home())
}

pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn themes_dir() -> PathBuf {
    config_dir().join("themes")
}

/// `~/Library/Application Support/arkos`: the socket, the daemon's logs, `updates/`, `window.log`.
/// `$ARKOS_APP_SUPPORT`, when set and non-empty, replaces it: a second, separate Arkos (its own
/// daemon, sessions and workspaces) beside the one in use, which the screenshot shoot
/// (`scripts/shoot.sh`) runs from a dev build.
pub fn app_support() -> PathBuf {
    app_support_from(std::env::var_os("ARKOS_APP_SUPPORT").as_deref(), &home())
}

pub fn app_support_from(override_dir: Option<&std::ffi::OsStr>, home: &Path) -> PathBuf {
    match override_dir {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => home.join("Library/Application Support/arkos"),
    }
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
    fn app_support_honours_the_override_only_when_it_is_set() {
        let home = Path::new("/Users/x");
        assert_eq!(app_support_from(None, home), PathBuf::from("/Users/x/Library/Application Support/arkos"));
        assert_eq!(app_support_from(Some(std::ffi::OsStr::new("")), home), PathBuf::from("/Users/x/Library/Application Support/arkos"));
        assert_eq!(app_support_from(Some(std::ffi::OsStr::new("/tmp/shoot")), home), PathBuf::from("/tmp/shoot"));
    }

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
    fn config_dir_honours_xdg_config_home() {
        use std::ffi::OsStr;
        let home = Path::new("/Users/me");
        assert_eq!(config_dir_from(None, home), PathBuf::from("/Users/me/.config/arkos"));
        assert_eq!(config_dir_from(Some(OsStr::new("")), home), PathBuf::from("/Users/me/.config/arkos"));
        assert_eq!(config_dir_from(Some(OsStr::new("/tmp/xdg")), home), PathBuf::from("/tmp/xdg/arkos"));
        assert!(config_file().ends_with("arkos/config.toml"));
        assert!(themes_dir().ends_with("arkos/themes"));
    }

    #[test]
    fn last_version_lives_in_app_support() {
        assert!(last_version_file().ends_with("arkos/last_version"));
    }

    #[test]
    fn recovered_edits_live_in_app_support() {
        assert!(recovered_dir().ends_with("arkos/recovered"));
    }
}
