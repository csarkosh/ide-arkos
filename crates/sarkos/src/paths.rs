//! Where sarkos keeps its socket and workspace files (spec 2.1, 4).
use std::path::{Path, PathBuf};

fn app_support() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME is set");
    PathBuf::from(home).join("Library/Application Support/sarkos")
}

pub fn socket_path() -> PathBuf {
    app_support().join("sarkosd.sock")
}

/// FNV-1a over the path bytes. Rust's `DefaultHasher` is not stable across releases, which would
/// orphan every workspace file on a toolchain upgrade.
fn fnv1a(bytes: &[u8]) -> u64 {
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
        .and_then(|p| p.parent().map(|d| d.join("sarkosd")))
        .unwrap_or_else(|| PathBuf::from("sarkosd"))
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
}
