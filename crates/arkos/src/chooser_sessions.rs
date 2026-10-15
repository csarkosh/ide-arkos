//! The sessions behind each recent folder in the launch chooser: how many run, and stopping
//! them. Pure, so it is tested; `launch_link` moves the bytes.
use protocol::{Response, SessionId, SessionInfo};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The live sessions of one folder: those its saved workspace names (a worktree tab's shell runs
/// elsewhere) and those whose shell runs in the folder (a tab closed within its grace is in no
/// workspace). Each once, in the daemon's order.
pub fn sessions_of(folder: &Path, saved: &[SessionId], live: &[SessionInfo]) -> Vec<SessionId> {
    live.iter().filter(|s| s.alive && (saved.contains(&s.id) || s.cwd == folder)).map(|s| s.id).collect()
}

/// The row's right-hand count; nothing when none run.
pub fn count_label(n: usize) -> Option<String> {
    match n {
        0 => None,
        1 => Some("1 session".to_string()),
        n => Some(format!("{n} sessions")),
    }
}

/// What the link tells the chooser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkEvent {
    /// The daemon's session list.
    Sessions(Vec<SessionInfo>),
    /// Every Kill for the folder was answered; `errors` holds the daemon's words for those it
    /// refused.
    Stopped { folder: PathBuf, errors: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Want {
    List,
    Kill(PathBuf),
}

/// Matches the daemon's answers to the chooser's requests by id. Answers to nothing it asked
/// (the heartbeat's) are not its business.
#[derive(Debug, Default)]
pub struct Tracker {
    pending: HashMap<u32, Want>,
    /// Kills still unanswered per folder, with the errors so far.
    stops: HashMap<PathBuf, (usize, Vec<String>)>,
}

impl Tracker {
    pub fn listed(&mut self, id: u32) {
        self.pending.insert(id, Want::List);
    }

    /// `ids` are the request ids of the folder's Kill requests, at least one.
    pub fn stopping(&mut self, folder: PathBuf, ids: &[u32]) {
        for id in ids {
            self.pending.insert(*id, Want::Kill(folder.clone()));
        }
        self.stops.insert(folder, (ids.len(), Vec::new()));
    }

    pub fn on_response(&mut self, id: u32, response: Response) -> Option<LinkEvent> {
        match (self.pending.remove(&id)?, response) {
            (Want::List, Response::Sessions(list)) => Some(LinkEvent::Sessions(list)),
            (Want::List, _) => None,
            (Want::Kill(folder), response) => {
                let (left, errors) = self.stops.get_mut(&folder)?;
                if let Response::Error(e) = response {
                    errors.push(e);
                }
                *left = left.saturating_sub(1);
                if *left > 0 {
                    return None;
                }
                let (_, errors) = self.stops.remove(&folder)?;
                Some(LinkEvent::Stopped { folder, errors })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::ClaudeState;

    fn info(id: u64, cwd: &str, alive: bool) -> SessionInfo {
        SessionInfo { id: SessionId(id), title: String::new(), cwd: PathBuf::from(cwd), pid: None, alive, last_activity_unix_ms: 0, state: ClaudeState::Idle }
    }

    #[test]
    fn a_folders_sessions_are_its_saved_ones_and_the_shells_in_it() {
        let live = vec![
            info(1, "/p", true),      // a tab in the workspace
            info(2, "/p-feat", true), // a worktree tab: saved, shell elsewhere
            info(3, "/p", true),      // closed within its grace: not saved, shell here
            info(4, "/p", false),     // ended
            info(5, "/q", true),      // another project
        ];
        let saved = [SessionId(1), SessionId(2), SessionId(4)];
        assert_eq!(sessions_of(Path::new("/p"), &saved, &live), vec![SessionId(1), SessionId(2), SessionId(3)]);
        assert_eq!(sessions_of(Path::new("/q"), &[], &live), vec![SessionId(5)]);
        assert!(sessions_of(Path::new("/r"), &[], &live).is_empty());
    }

    #[test]
    fn the_count_label_reads_naturally() {
        assert_eq!(count_label(0), None);
        assert_eq!(count_label(1).as_deref(), Some("1 session"));
        assert_eq!(count_label(3).as_deref(), Some("3 sessions"));
    }

    #[test]
    fn the_tracker_matches_answers_by_id() {
        let mut t = Tracker::default();
        t.listed(7);
        assert_eq!(t.on_response(3, Response::Ok), None, "the heartbeat's answer");
        let list = vec![info(1, "/p", true)];
        assert_eq!(t.on_response(7, Response::Sessions(list.clone())), Some(LinkEvent::Sessions(list)));
        assert_eq!(t.on_response(7, Response::Ok), None, "answered once");
        // Three kills: the event comes with the last answer, errors kept.
        t.stopping(PathBuf::from("/p"), &[10, 11, 12]);
        assert_eq!(t.on_response(10, Response::Ok), None);
        assert_eq!(t.on_response(12, Response::Error("no session 12".into())), None);
        assert_eq!(t.on_response(11, Response::Ok), Some(LinkEvent::Stopped { folder: PathBuf::from("/p"), errors: vec!["no session 12".into()] }));
        // Two folders at once stay apart.
        t.stopping(PathBuf::from("/a"), &[20]);
        t.stopping(PathBuf::from("/b"), &[21]);
        assert_eq!(t.on_response(21, Response::Ok), Some(LinkEvent::Stopped { folder: PathBuf::from("/b"), errors: vec![] }));
        assert_eq!(t.on_response(20, Response::Ok), Some(LinkEvent::Stopped { folder: PathBuf::from("/a"), errors: vec![] }));
    }
}
