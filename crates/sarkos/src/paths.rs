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

pub fn workspace_file(project: &Path) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    app_support().join("workspaces").join(format!("{:016x}.json", fnv1a(project.as_os_str().as_bytes())))
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
}
