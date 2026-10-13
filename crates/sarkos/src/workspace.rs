//! Remembers which daemon session belongs to a project folder (spec 4, milestone 1 subset).
use crate::paths::workspace_file;
use protocol::SessionId;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Workspace {
    pub sessions: Vec<u64>,
}

pub fn load(project: &Path) -> Option<SessionId> {
    load_from(&workspace_file(project))
}

pub fn save(project: &Path, session: SessionId) {
    save_to(&workspace_file(project), session);
}

pub fn load_from(file: &Path) -> Option<SessionId> {
    let text = std::fs::read_to_string(file).ok()?;
    let ws: Workspace = serde_json::from_str(&text).ok()?;
    ws.sessions.first().map(|id| SessionId(*id))
}

pub fn save_to(file: &Path, session: SessionId) {
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let ws = Workspace { sessions: vec![session.0] };
    if let Ok(text) = serde_json::to_string_pretty(&ws) {
        let _ = std::fs::write(file, text);
    }
}
