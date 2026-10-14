//! Remembers a project's layout: the split tree with every tab's session (spec 4). Saved on
//! every change, debounced, atomically, and off the UI thread.
use crate::paths::workspace_file;
use crate::tree::{Tab, Tree};
use gpui::App;
use protocol::SessionId;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const FORMAT_VERSION: u32 = 2;
/// Layout changes within this window collapse into one write.
pub const SAVE_DEBOUNCE: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Workspace {
    pub version: u32,
    pub project: PathBuf,
    pub tree: Tree,
}

/// Milestone 1 wrote `{ "sessions": [id] }`: one session, no layout.
#[derive(Deserialize)]
struct V1 {
    sessions: Vec<u64>,
}

/// One small read; callers may run it on the main thread before the window opens.
pub fn load(project: &Path) -> Option<Workspace> {
    load_from(&workspace_file(project), project)
}

/// Writes to disk; run it off the UI thread (see `request_save`).
pub fn save(project: &Path, ws: &Workspace) -> std::io::Result<()> {
    save_to(&workspace_file(project), ws)
}

/// A missing file means no saved layout. An unreadable or corrupt file also loads as nothing,
/// but is reported on stderr, since it orphans sessions the daemon is still running.
pub fn load_from(file: &Path, project: &Path) -> Option<Workspace> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            eprintln!("sarkos: cannot read workspace {}: {e}", file.display());
            return None;
        }
    };
    if let Ok(ws) = serde_json::from_str::<Workspace>(&text) {
        return Some(ws);
    }
    match serde_json::from_str::<V1>(&text) {
        Ok(v1) => {
            let session = v1.sessions.first().map(|id| SessionId(*id))?;
            eprintln!("sarkos: migrating milestone 1 workspace {}", file.display());
            Some(Workspace { version: FORMAT_VERSION, project: project.to_path_buf(), tree: Tree::new(Tab::Terminal { session }) })
        }
        Err(e) => {
            eprintln!("sarkos: ignoring corrupt workspace {}: {e}", file.display());
            None
        }
    }
}

/// Writes a sibling temp file and renames it over `file`. The rename is atomic on one volume, so
/// a crash mid-save leaves the previous workspace intact instead of a torn file.
pub fn save_to(file: &Path, ws: &Workspace) -> std::io::Result<()> {
    let parent = file.parent().ok_or_else(|| std::io::Error::other("workspace path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let name = file.file_name().ok_or_else(|| std::io::Error::other("workspace path has no file name"))?;
    let tmp = parent.join(format!(".{}.{}.tmp", name.to_string_lossy(), std::process::id()));
    let text = serde_json::to_string_pretty(ws).map_err(std::io::Error::other)?;
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

/// Coalesces rapid saves: the latest workspace wins and one write happens per debounce window.
#[derive(Default)]
pub struct SaveQueue {
    latest: Option<Workspace>,
    scheduled: bool,
}

impl SaveQueue {
    /// Returns true when the caller must schedule a flush; false when one is already pending.
    pub fn offer(&mut self, ws: Workspace) -> bool {
        self.latest = Some(ws);
        if self.scheduled {
            return false;
        }
        self.scheduled = true;
        true
    }

    /// The workspace to write now. Clears the schedule so the next offer schedules again.
    pub fn take(&mut self) -> Option<Workspace> {
        self.scheduled = false;
        self.latest.take()
    }
}

pub type SharedSaveQueue = Arc<Mutex<SaveQueue>>;

/// Call on the UI thread after every layout change. The debounce timer and the write both run
/// off the UI thread; failures are logged.
pub fn request_save(queue: &SharedSaveQueue, ws: Workspace, cx: &mut App) {
    if !queue.lock().unwrap().offer(ws) {
        return;
    }
    let queue = queue.clone();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(SAVE_DEBOUNCE).await;
        let ws = queue.lock().unwrap().take();
        if let Some(ws) = ws {
            cx.background_executor().spawn(async move {
                if let Err(e) = save(&ws.project, &ws) {
                    eprintln!("sarkos: cannot save workspace: {e}");
                }
            })
            .await;
        }
    })
    .detach();
}

/// Writes the pending save now, on the calling thread: one small write, for quitting, when the
/// debounce timer would never fire.
pub fn flush_now(queue: &SharedSaveQueue) {
    let ws = queue.lock().unwrap().take();
    if let Some(ws) = ws {
        if let Err(e) = save(&ws.project, &ws) {
            eprintln!("sarkos: cannot save workspace: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Side;

    fn tree_with_two() -> Tree {
        let mut t = Tree::new(Tab::Terminal { session: SessionId(1) });
        let a = t.focused;
        t.split(a, Side::Right, Tab::Terminal { session: SessionId(2) });
        t
    }

    #[test]
    fn round_trips_a_layout() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ws.json");
        let project = Path::new("/projects/x");
        assert_eq!(load_from(&file, project), None);
        let ws = Workspace { version: FORMAT_VERSION, project: project.to_path_buf(), tree: tree_with_two() };
        save_to(&file, &ws).unwrap();
        assert_eq!(load_from(&file, project), Some(ws));
    }

    #[test]
    fn migrates_a_milestone_1_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ws.json");
        std::fs::write(&file, r#"{ "sessions": [42] }"#).unwrap();
        let ws = load_from(&file, Path::new("/p")).expect("migrated");
        assert_eq!(ws.version, FORMAT_VERSION);
        assert_eq!(ws.project, PathBuf::from("/p"));
        assert_eq!(ws.tree, Tree::new(Tab::Terminal { session: SessionId(42) }));
        std::fs::write(&file, r#"{ "sessions": [] }"#).unwrap();
        assert_eq!(load_from(&file, Path::new("/p")), None);
    }

    #[test]
    fn corrupt_file_reads_as_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ws.json");
        std::fs::write(&file, "{not json").unwrap();
        assert_eq!(load_from(&file, Path::new("/p")), None);
    }

    #[test]
    fn save_leaves_no_temp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("workspaces").join("ws.json");
        let ws = Workspace { version: FORMAT_VERSION, project: PathBuf::from("/p"), tree: tree_with_two() };
        save_to(&file, &ws).unwrap();
        save_to(&file, &ws).unwrap();
        let names: Vec<String> = std::fs::read_dir(file.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["ws.json".to_string()]);
    }

    #[test]
    fn save_reports_an_unwritable_target() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, "").unwrap();
        let ws = Workspace { version: FORMAT_VERSION, project: PathBuf::from("/p"), tree: tree_with_two() };
        assert!(save_to(&blocker.join("ws.json"), &ws).is_err());
    }

    #[test]
    fn save_queue_coalesces_until_taken() {
        let mut q = SaveQueue::default();
        let a = Workspace { version: FORMAT_VERSION, project: PathBuf::from("/a"), tree: tree_with_two() };
        let b = Workspace { version: FORMAT_VERSION, project: PathBuf::from("/b"), tree: tree_with_two() };
        assert!(q.offer(a.clone()), "first offer schedules a flush");
        assert!(!q.offer(b.clone()), "a flush is already pending");
        assert_eq!(q.take(), Some(b), "the latest offer wins");
        assert_eq!(q.take(), None);
        assert!(q.offer(a), "after a flush the next offer schedules again");
    }
}
