//! Remembers which daemon session belongs to a project folder (spec 4, milestone 1 subset).
use crate::paths::workspace_file;
use protocol::SessionId;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Workspace {
    pub sessions: Vec<u64>,
}

/// One small read; callers may run it on the main thread before the window opens.
pub fn load(project: &Path) -> Option<SessionId> {
    load_from(&workspace_file(project))
}

/// Writes to disk; run it off the UI thread.
pub fn save(project: &Path, session: SessionId) -> std::io::Result<()> {
    save_to(&workspace_file(project), session)
}

/// A missing file means no saved session. An unreadable or corrupt file also loads as nothing,
/// but is reported on stderr, since it orphans the session the daemon is still running.
pub fn load_from(file: &Path) -> Option<SessionId> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            eprintln!("sarkos: cannot read workspace {}: {e}", file.display());
            return None;
        }
    };
    match serde_json::from_str::<Workspace>(&text) {
        Ok(ws) => ws.sessions.first().map(|id| SessionId(*id)),
        Err(e) => {
            eprintln!("sarkos: ignoring corrupt workspace {}: {e}", file.display());
            None
        }
    }
}

/// Writes a sibling temp file and renames it over `file`. The rename is atomic on one volume, so
/// a crash mid-save leaves the previous workspace intact instead of a torn file.
pub fn save_to(file: &Path, session: SessionId) -> std::io::Result<()> {
    let parent = file.parent().ok_or_else(|| std::io::Error::other("workspace path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let name = file.file_name().ok_or_else(|| std::io::Error::other("workspace path has no file name"))?;
    let tmp = parent.join(format!(".{}.{}.tmp", name.to_string_lossy(), std::process::id()));
    let ws = Workspace { sessions: vec![session.0] };
    let text = serde_json::to_string_pretty(&ws).map_err(std::io::Error::other)?;
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, file)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ws.json");
        assert_eq!(load_from(&file), None);
        save_to(&file, SessionId(42)).unwrap();
        assert_eq!(load_from(&file), Some(SessionId(42)));
    }

    #[test]
    fn corrupt_file_reads_as_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ws.json");
        std::fs::write(&file, "{not json").unwrap();
        assert_eq!(load_from(&file), None);
    }

    #[test]
    fn different_projects_get_different_files() {
        let a = crate::paths::workspace_file(std::path::Path::new("/tmp/a"));
        let b = crate::paths::workspace_file(std::path::Path::new("/tmp/b"));
        assert_ne!(a, b);
        assert!(a.to_string_lossy().contains("workspaces"));
    }

    #[test]
    fn save_leaves_no_temp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("workspaces").join("ws.json");
        save_to(&file, SessionId(1)).unwrap();
        save_to(&file, SessionId(2)).unwrap();
        let names: Vec<String> = std::fs::read_dir(file.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["ws.json".to_string()]);
        assert_eq!(load_from(&file), Some(SessionId(2)));
    }

    #[test]
    fn save_reports_an_unwritable_target() {
        let dir = tempfile::tempdir().unwrap();
        // The parent is a regular file, so neither the directory nor the temp file can be made.
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, "").unwrap();
        assert!(save_to(&blocker.join("ws.json"), SessionId(7)).is_err());
    }
}
