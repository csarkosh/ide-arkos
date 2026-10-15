//! Remembers a project's layout: the split tree with every tab's session (spec 4). Saved on
//! every change, debounced, atomically, and off the UI thread.
use crate::paths::{legacy_workspace_file, workspace_file};
use crate::tree::{Tab, Tree};
use gpui::App;
use protocol::SessionId;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const FORMAT_VERSION: u32 = 3;
/// Layout changes within this window collapse into one write.
pub const SAVE_DEBOUNCE: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Workspace {
    pub version: u32,
    pub project: PathBuf,
    pub tree: Tree,
    /// The text preset this workspace last used (spec 7: a 2x2 grid and a single pane want
    /// different sizes). Absent in milestone 2a files.
    #[serde(default)]
    pub text_preset: Option<String>,
    #[serde(default)]
    pub sidebar: Option<SidebarSaved>,
}

/// The sidebar as the user left it for this folder (spec 4: "sidebar width and collapsed
/// states" are saved). Absent in files from before milestone 4.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SidebarSaved {
    pub width: f32,
    pub collapsed: bool,
    pub show_ignored: bool,
    /// Expanded directories, absolute, so the tree reopens as it was.
    pub expanded: Vec<PathBuf>,
    /// Expanded worktrees, absolute. Absent in files from before milestone 4b.
    #[serde(default)]
    pub worktrees_expanded: Vec<PathBuf>,
    /// Whether each section is open. Absent in files from before milestone 4b: both open.
    #[serde(default = "default_true")]
    pub explorer_open: bool,
    #[serde(default = "default_true")]
    pub worktrees_open: bool,
}

fn default_true() -> bool {
    true
}

/// Milestone 1 wrote `{ "sessions": [id] }`: one session, no layout.
#[derive(Deserialize)]
struct V1 {
    sessions: Vec<u64>,
}

/// One small read; callers may run it on the main thread before the window opens.
pub fn load(project: &Path) -> Option<Workspace> {
    load_or_migrate(&workspace_file(project), &legacy_workspace_file(project), project)
}

/// Loads `file`. When it does not exist yet but milestone 1's differently named `legacy` file
/// does, loads that instead, saves it under the new name and removes the old one, so an upgrade
/// keeps the session milestone 1 was showing.
fn load_or_migrate(file: &Path, legacy: &Path, project: &Path) -> Option<Workspace> {
    if file.exists() || legacy == file || !legacy.exists() {
        return load_from(file, project);
    }
    let ws = load_from(legacy, project)?;
    match save_to(file, &ws) {
        Ok(()) => {
            eprintln!("arkos: moved workspace {} to {}", legacy.display(), file.display());
            match std::fs::remove_file(legacy) {
                Ok(()) => eprintln!("arkos: removed old workspace {}", legacy.display()),
                Err(e) => eprintln!("arkos: cannot remove old workspace {}: {e}", legacy.display()),
            }
        }
        // The layout still loads; the next save writes the new name.
        Err(e) => eprintln!("arkos: cannot save migrated workspace {}: {e}", file.display()),
    }
    Some(ws)
}

/// Writes to disk; run it off the UI thread (see `request_save`).
/// The sessions `project`'s saved layout names, for the chooser's counts; none without a file.
pub fn session_ids(project: &Path) -> Vec<SessionId> {
    load(project)
        .map(|ws| {
            ws.tree
                .tabs()
                .into_iter()
                .filter_map(|(_, _, t)| match t {
                    Tab::Terminal { session, .. } => Some(*session),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

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
            eprintln!("arkos: cannot read workspace {}: {e}", file.display());
            return None;
        }
    };
    if let Ok(mut ws) = serde_json::from_str::<Workspace>(&text) {
        ws.tree.repair_ids();
        return Some(ws);
    }
    match serde_json::from_str::<V1>(&text) {
        Ok(v1) => {
            let session = v1.sessions.first().map(|id| SessionId(*id))?;
            eprintln!("arkos: migrating milestone 1 workspace {}", file.display());
            Some(Workspace { version: FORMAT_VERSION, project: project.to_path_buf(), tree: Tree::new(Tab::Terminal { session, cwd: None }), text_preset: None, sidebar: None })
        }
        Err(e) => {
            eprintln!("arkos: ignoring corrupt workspace {}: {e}", file.display());
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
    /// Counts offers, so writes can be ordered.
    generation: u64,
    /// The last workspace handed to a writer, with its generation. A flush at quit rewrites it in
    /// case that write is still running (or failed) when the process exits.
    last_taken: Option<(u64, Workspace)>,
    /// The generation last written. Writers hold it while writing, so a flush never interleaves
    /// with a background write and an older workspace never overwrites a newer one.
    written: Arc<Mutex<u64>>,
}

/// One workspace to write, with what is needed to keep writes in order.
pub struct SaveJob {
    pub ws: Workspace,
    generation: u64,
    written: Arc<Mutex<u64>>,
}

impl SaveJob {
    /// Writes to disk unless a newer (or this same) generation is already there.
    pub fn write(self) -> std::io::Result<()> {
        self.write_with(|ws| save(&ws.project, ws))
    }

    fn write_with(self, write: impl FnOnce(&Workspace) -> std::io::Result<()>) -> std::io::Result<()> {
        let mut last = self.written.lock().unwrap();
        if self.generation <= *last {
            return Ok(());
        }
        write(&self.ws)?;
        *last = self.generation;
        Ok(())
    }
}

impl SaveQueue {
    /// Returns true when the caller must schedule a flush; false when one is already pending.
    pub fn offer(&mut self, ws: Workspace) -> bool {
        self.generation += 1;
        self.latest = Some(ws);
        if self.scheduled {
            return false;
        }
        self.scheduled = true;
        true
    }

    fn job(&self, generation: u64, ws: Workspace) -> SaveJob {
        SaveJob { ws, generation, written: self.written.clone() }
    }

    /// The workspace to write now. Clears the schedule so the next offer schedules again.
    pub fn take(&mut self) -> Option<SaveJob> {
        self.scheduled = false;
        let ws = self.latest.take()?;
        self.last_taken = Some((self.generation, ws.clone()));
        Some(self.job(self.generation, ws))
    }

    /// For quitting: the pending workspace, else the last one taken (its write may not have
    /// finished). `SaveJob::write` skips it when it is already on disk.
    pub fn take_for_flush(&mut self) -> Option<SaveJob> {
        if self.latest.is_some() {
            return self.take();
        }
        let (generation, ws) = self.last_taken.clone()?;
        Some(self.job(generation, ws))
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
        let job = queue.lock().unwrap().take();
        if let Some(job) = job {
            cx.background_executor().spawn(async move {
                if let Err(e) = job.write() {
                    eprintln!("arkos: cannot save workspace: {e}");
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
    let job = queue.lock().unwrap().take_for_flush();
    if let Some(job) = job {
        if let Err(e) = job.write() {
            eprintln!("arkos: cannot save workspace: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_preset_is_saved_and_a_v2_file_has_none() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ws.json");
        let project = Path::new("/p");
        let mut ws = Workspace { version: FORMAT_VERSION, project: project.to_path_buf(), tree: tree_with_two(), text_preset: Some("relaxed".into()), sidebar: None };
        save_to(&file, &ws).unwrap();
        assert_eq!(load_from(&file, project), Some(ws.clone()));
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("\"text_preset\": \"relaxed\""), "{text}");
        // A milestone 2a file has no preset: the config default applies.
        let mut json: serde_json::Value = serde_json::from_str(&text).unwrap();
        json.as_object_mut().unwrap().remove("text_preset");
        json["version"] = 2.into();
        std::fs::write(&file, serde_json::to_string_pretty(&json).unwrap()).unwrap();
        ws.text_preset = None;
        ws.version = 2;
        assert_eq!(load_from(&file, project), Some(ws));
    }
    use crate::tree::Side;

    fn tree_with_two() -> Tree {
        let mut t = Tree::new(Tab::Terminal { session: SessionId(1), cwd: None });
        let a = t.focused;
        t.split(a, Side::Right, Tab::Terminal { session: SessionId(2), cwd: None });
        t
    }

    #[test]
    fn the_sidebar_state_round_trips_and_is_optional() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ws.json");
        let project = Path::new("/p");
        let mut ws = Workspace { version: FORMAT_VERSION, project: project.to_path_buf(), tree: tree_with_two(), text_preset: None, sidebar: None };
        save_to(&file, &ws).unwrap();
        assert_eq!(load_from(&file, project), Some(ws.clone()));
        ws.sidebar = Some(SidebarSaved { width: 300.0, collapsed: true, show_ignored: true, expanded: vec![PathBuf::from("/p/src")], worktrees_expanded: vec![PathBuf::from("/p-f")], explorer_open: true, worktrees_open: false });
        save_to(&file, &ws).unwrap();
        assert_eq!(load_from(&file, project), Some(ws));
        // A milestone 3 file has no sidebar object.
        std::fs::write(&file, r#"{ "version": 2, "project": "/p", "tree": { "root": { "Leaf": { "id": 1, "tabs": [], "active": 0 } }, "next_id": 2, "focused": 1, "maximized": null } }"#).unwrap();
        assert_eq!(load_from(&file, project).unwrap().sidebar, None);
        // A file from before 4b has a sidebar object without the expanded worktrees.
        std::fs::write(&file, r#"{ "version": 3, "project": "/p", "tree": { "root": { "Leaf": { "id": 1, "tabs": [], "active": 0 } }, "next_id": 2, "focused": 1, "maximized": null }, "sidebar": { "width": 250.0, "collapsed": false, "show_ignored": false, "expanded": [] } }"#).unwrap();
        let old = load_from(&file, project).unwrap().sidebar.unwrap();
        assert_eq!(old.worktrees_expanded, Vec::<PathBuf>::new());
        assert!(old.explorer_open && old.worktrees_open, "both sections start open");
    }

    #[test]
    fn round_trips_a_layout() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ws.json");
        let project = Path::new("/projects/x");
        assert_eq!(load_from(&file, project), None);
        let mut tree = tree_with_two();
        let pane = tree.focused;
        tree.add_tab(pane, Tab::Terminal { session: SessionId(3), cwd: Some(PathBuf::from("/projects/x-feat")) });
        tree.add_tab(pane, Tab::Browser { url: Some("https://example.com/docs".into()), app_mode: true });
        tree.add_tab(pane, Tab::Browser { url: None, app_mode: false });
        let ws = Workspace { version: FORMAT_VERSION, project: project.to_path_buf(), tree, text_preset: None, sidebar: None };
        save_to(&file, &ws).unwrap();
        let loaded = load_from(&file, project).unwrap();
        let tabs = &loaded.tree.pane(loaded.tree.focused).unwrap().tabs;
        assert!(tabs.contains(&Tab::Browser { url: Some("https://example.com/docs".into()), app_mode: true }), "a browser tab keeps its URL and mode");
        assert!(tabs.contains(&Tab::Browser { url: None, app_mode: false }), "a blank browser tab stays blank");
        assert_eq!(Some(loaded), Some(ws), "a worktree terminal keeps its folder");
        // A browser tab saved before app_mode existed reads as a normal window.
        std::fs::write(&file, r#"{ "version": 3, "project": "/projects/x", "tree": { "root": { "Leaf": { "id": 1, "tabs": [ { "Browser": { "url": "https://x.dev" } } ], "active": 0 } }, "next_id": 2, "focused": 1, "maximized": null } }"#).unwrap();
        let ws = load_from(&file, project).unwrap();
        assert_eq!(ws.tree.pane(1).unwrap().tabs, vec![Tab::Browser { url: Some("https://x.dev".into()), app_mode: false }]);
        // A file from before 4b has no cwd on its terminal tabs: they belong to the project.
        std::fs::write(&file, r#"{ "version": 3, "project": "/projects/x", "tree": { "root": { "Leaf": { "id": 1, "tabs": [ { "Terminal": { "session": 7 } } ], "active": 0 } }, "next_id": 2, "focused": 1, "maximized": null } }"#).unwrap();
        let ws = load_from(&file, project).unwrap();
        assert_eq!(ws.tree.pane(1).unwrap().tabs, vec![Tab::Terminal { session: SessionId(7), cwd: None }]);
    }

    #[test]
    fn migrates_a_milestone_1_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ws.json");
        std::fs::write(&file, r#"{ "sessions": [42] }"#).unwrap();
        let ws = load_from(&file, Path::new("/p")).expect("migrated");
        assert_eq!(ws.version, FORMAT_VERSION);
        assert_eq!(ws.project, PathBuf::from("/p"));
        assert_eq!(ws.tree, Tree::new(Tab::Terminal { session: SessionId(42), cwd: None }));
        std::fs::write(&file, r#"{ "sessions": [] }"#).unwrap();
        assert_eq!(load_from(&file, Path::new("/p")), None);
    }

    #[test]
    fn load_migrates_a_legacy_named_file() {
        use crate::paths::{legacy_workspace_file_in, workspace_file_in};
        let dir = tempfile::tempdir().unwrap();
        let project = Path::new("/projects/upgraded");
        let (file, legacy) = (workspace_file_in(dir.path(), project), legacy_workspace_file_in(dir.path(), project));
        assert_ne!(file, legacy);
        std::fs::write(&legacy, r#"{ "sessions": [42] }"#).unwrap();
        let ws = load_or_migrate(&file, &legacy, project).expect("the milestone 1 file is found");
        assert_eq!(ws.tree, Tree::new(Tab::Terminal { session: SessionId(42), cwd: None }));
        assert!(!legacy.exists(), "the old name is removed");
        assert_eq!(load_from(&file, project), Some(ws.clone()), "and saved under the new name");
        // From then on the new file wins, even if an old one reappears.
        std::fs::write(&legacy, r#"{ "sessions": [7] }"#).unwrap();
        assert_eq!(load_or_migrate(&file, &legacy, project), Some(ws));
        assert!(legacy.exists(), "an old file next to a new one is left alone");
    }

    #[test]
    fn a_file_without_tab_ids_loads_with_fresh_ones() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ws.json");
        std::fs::write(
            &file,
            r#"{ "version": 2, "project": "/p", "tree": { "root": { "Leaf": { "id": 1, "tabs": [
                { "Terminal": { "session": 7 } }, { "Terminal": { "session": 8 } } ], "active": 0, "mru": [0, 1] } },
                "next_id": 2, "focused": 1, "maximized": null } }"#,
        )
        .unwrap();
        let ws = load_from(&file, Path::new("/p")).expect("loads");
        let ids = &ws.tree.pane(1).unwrap().ids;
        assert_eq!(ids.len(), 2);
        assert!(ids[0] != ids[1] && ids.iter().all(|id| *id > 1), "unique and past the pane id");
        assert_eq!(ws.tree.locate(ids[1]), Some((1, 1)));
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
        let ws = Workspace { version: FORMAT_VERSION, project: PathBuf::from("/p"), tree: tree_with_two(), text_preset: None, sidebar: None };
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
        let ws = Workspace { version: FORMAT_VERSION, project: PathBuf::from("/p"), tree: tree_with_two(), text_preset: None, sidebar: None };
        assert!(save_to(&blocker.join("ws.json"), &ws).is_err());
    }

    #[test]
    fn save_queue_coalesces_until_taken() {
        let mut q = SaveQueue::default();
        let a = Workspace { version: FORMAT_VERSION, project: PathBuf::from("/a"), tree: tree_with_two(), text_preset: None, sidebar: None };
        let b = Workspace { version: FORMAT_VERSION, project: PathBuf::from("/b"), tree: tree_with_two(), text_preset: None, sidebar: None };
        assert!(q.offer(a.clone()), "first offer schedules a flush");
        assert!(!q.offer(b.clone()), "a flush is already pending");
        assert_eq!(q.take().map(|j| j.ws), Some(b.clone()), "the latest offer wins");
        assert!(q.take().is_none());
        // The background write may still be running at quit: the flush still sees the workspace.
        assert_eq!(q.take_for_flush().map(|j| j.ws), Some(b), "a flush rewrites the last taken workspace");
        assert!(q.offer(a.clone()), "after a flush the next offer schedules again");
        assert_eq!(q.take_for_flush().map(|j| j.ws), Some(a), "a pending offer outranks the last taken one");
    }

    #[test]
    fn writes_never_go_back_a_generation() {
        let mut q = SaveQueue::default();
        let a = Workspace { version: FORMAT_VERSION, project: PathBuf::from("/a"), tree: tree_with_two(), text_preset: None, sidebar: None };
        let b = Workspace { version: FORMAT_VERSION, project: PathBuf::from("/b"), tree: tree_with_two(), text_preset: None, sidebar: None };
        q.offer(a);
        let old = q.take().unwrap();
        q.offer(b);
        let new = q.take_for_flush().unwrap();
        let wrote = std::cell::RefCell::new(Vec::new());
        let record = |ws: &Workspace| {
            wrote.borrow_mut().push(ws.project.clone());
            Ok(())
        };
        new.write_with(record).unwrap();
        old.write_with(record).unwrap();
        // Flushing what is already on disk writes nothing.
        q.take_for_flush().unwrap().write_with(record).unwrap();
        assert_eq!(*wrote.borrow(), vec![PathBuf::from("/b")]);
    }
}
