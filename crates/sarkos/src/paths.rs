//! Where sarkos keeps its socket and workspace files (spec 2.1, 4).
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

fn app_support() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME is set");
    PathBuf::from(home).join("Library/Application Support/sarkos")
}

pub fn socket_path() -> PathBuf {
    app_support().join("sarkosd.sock")
}

pub fn workspace_file(project: &Path) -> PathBuf {
    let mut h = DefaultHasher::new();
    project.hash(&mut h);
    app_support().join("workspaces").join(format!("{:016x}.json", h.finish()))
}

pub fn daemon_exe() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("sarkosd")))
        .unwrap_or_else(|| PathBuf::from("sarkosd"))
}
