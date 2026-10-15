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
    /// The connection ended: the counts mean nothing now.
    Lost,
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

    /// An answer that is the tracker's gives `Ok` (with the event it completes, if any); one to
    /// something it never asked (the Hello, the heartbeat) comes back as `Err`, for whoever did.
    pub fn on_response(&mut self, id: u32, response: Response) -> Result<Option<LinkEvent>, Response> {
        let Some(want) = self.pending.remove(&id) else { return Err(response) };
        Ok(match (want, response) {
            (Want::List, Response::Sessions(list)) => Some(LinkEvent::Sessions(list)),
            (Want::List, _) => None,
            (Want::Kill(folder), response) => {
                let Some((left, errors)) = self.stops.get_mut(&folder) else { return Ok(None) };
                // A session already gone is what was asked for.
                if let Response::Error(e) = response {
                    if !e.starts_with("no session") {
                        errors.push(e);
                    }
                }
                *left = left.saturating_sub(1);
                if *left > 0 {
                    return Ok(None);
                }
                self.stops.remove(&folder).map(|(_, errors)| LinkEvent::Stopped { folder, errors })
            }
        })
    }

    /// Whether every request has been answered.
    pub fn is_idle(&self) -> bool {
        self.pending.is_empty()
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
    fn the_tracker_matches_answers_by_id_and_returns_the_rest() {
        let mut t = Tracker::default();
        assert!(t.is_idle());
        t.listed(7);
        assert!(!t.is_idle());
        // The Hello's and the heartbeat's answers are somebody else's: handed back untouched.
        let hello = Response::Hello { daemon_version: "x".into(), protocol_min: 4, protocol_max: 4 };
        assert_eq!(t.on_response(1, hello.clone()), Err(hello));
        assert_eq!(t.on_response(3, Response::Ok), Err(Response::Ok));
        let list = vec![info(1, "/p", true)];
        assert_eq!(t.on_response(7, Response::Sessions(list.clone())), Ok(Some(LinkEvent::Sessions(list))));
        assert!(t.is_idle());
        assert_eq!(t.on_response(7, Response::Ok), Err(Response::Ok), "answered once");
        // Three kills: the event comes with the last answer; a session already gone is no
        // error, a refusal is kept.
        t.stopping(PathBuf::from("/p"), &[10, 11, 12]);
        assert_eq!(t.on_response(10, Response::Ok), Ok(None));
        assert_eq!(t.on_response(12, Response::Error("no session 12".into())), Ok(None));
        t.stopping(PathBuf::from("/q"), &[13]);
        assert_eq!(t.on_response(11, Response::Error("an upgrade is in progress".into())), Ok(Some(LinkEvent::Stopped { folder: PathBuf::from("/p"), errors: vec!["an upgrade is in progress".into()] })));
        assert!(!t.is_idle(), "/q's kill is still out");
        assert_eq!(t.on_response(13, Response::Ok), Ok(Some(LinkEvent::Stopped { folder: PathBuf::from("/q"), errors: vec![] })));
        assert!(t.is_idle());
    }
}
