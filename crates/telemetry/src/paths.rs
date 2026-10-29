//! The folders the app and the daemon both read: the config folder (with the XDG rule) and
//! Application Support. They moved here from the app's `paths.rs`, which re-exports them, so the
//! daemon can find the config file's `[telemetry]` table without a copy of the rule.
use std::path::{Path, PathBuf};

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").expect("HOME is set"))
}

/// The config folder. `$ARKOS_CONFIG_DIR`, when set and non-empty, is the folder itself (nothing
/// is appended). It is Arkos's own variable, so a sandbox can move its config without moving any
/// other program's. Otherwise `$XDG_CONFIG_HOME/arkos` when that is set and non-empty, else
/// `~/.config/arkos` (spec 7).
pub fn config_dir_from(arkos: Option<&std::ffi::OsStr>, xdg: Option<&std::ffi::OsStr>, home: &Path) -> PathBuf {
    match (arkos, xdg) {
        (Some(a), _) if !a.is_empty() => PathBuf::from(a),
        (_, Some(x)) if !x.is_empty() => PathBuf::from(x).join("arkos"),
        _ => home.join(".config").join("arkos"),
    }
}

pub fn config_dir() -> PathBuf {
    config_dir_from(
        std::env::var_os("ARKOS_CONFIG_DIR").as_deref(),
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        &home(),
    )
}

/// True when this process is an `arkos sandbox` instance: `$ARKOS_SANDBOX` is set and non-empty.
pub fn is_sandbox() -> bool {
    is_sandbox_from(std::env::var_os("ARKOS_SANDBOX").as_deref())
}

pub fn is_sandbox_from(v: Option<&std::ffi::OsStr>) -> bool {
    v.is_some_and(|v| !v.is_empty())
}

pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn app_support_honours_the_override_only_when_it_is_set() {
        let home = Path::new("/Users/x");
        assert_eq!(app_support_from(None, home), PathBuf::from("/Users/x/Library/Application Support/arkos"));
        assert_eq!(app_support_from(Some(OsStr::new("")), home), PathBuf::from("/Users/x/Library/Application Support/arkos"));
        assert_eq!(app_support_from(Some(OsStr::new("/tmp/shoot")), home), PathBuf::from("/tmp/shoot"));
    }

    #[test]
    fn config_dir_honours_xdg_config_home() {
        let home = Path::new("/Users/me");
        assert_eq!(config_dir_from(None, None, home), PathBuf::from("/Users/me/.config/arkos"));
        assert_eq!(config_dir_from(None, Some(OsStr::new("")), home), PathBuf::from("/Users/me/.config/arkos"));
        assert_eq!(config_dir_from(None, Some(OsStr::new("/tmp/xdg")), home), PathBuf::from("/tmp/xdg/arkos"));
        assert!(config_file().ends_with("arkos/config.toml"));
    }

    #[test]
    fn arkos_config_dir_wins_over_xdg_which_wins_over_the_home_default_and_empty_means_unset() {
        let home = Path::new("/Users/me");
        assert_eq!(config_dir_from(Some(OsStr::new("/s/config")), Some(OsStr::new("/x")), home), PathBuf::from("/s/config"));
        assert_eq!(config_dir_from(None, Some(OsStr::new("/x")), home), PathBuf::from("/x/arkos"));
        assert_eq!(config_dir_from(Some(OsStr::new("")), None, home), PathBuf::from("/Users/me/.config/arkos"));
    }

    #[test]
    fn a_sandbox_is_marked_by_a_non_empty_arkos_sandbox() {
        assert!(is_sandbox_from(Some(OsStr::new("1"))));
        assert!(!is_sandbox_from(Some(OsStr::new(""))));
        assert!(!is_sandbox_from(None));
    }
}
