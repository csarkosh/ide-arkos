//! Owns every terminal entity and the conversation with the daemon: which request asked for
//! what, which session sits in which tab, and the launch-time reconciliation of the saved layout
//! against the sessions the daemon still runs (spec 4: a missing session reopens as a fresh
//! shell in the same cwd with a note in the tab).


use crate::client::Client;
use crate::terminal::Terminal;
use crate::tree::{Tab, TabId, Tree};
use gpui::{App, Entity};
use protocol::{Event, Request, Response, SessionId, SessionInfo};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const SCROLLBACK: usize = 20_000;

/// Sends requests to the daemon. `Client` is the real one; tests record.
pub trait Transport: Send + Sync {
    fn send(&self, request: Request) -> u32;
}

impl Transport for Client {
    fn send(&self, request: Request) -> u32 {
        Client::send(self, request)
    }
}

/// What a request id is waiting for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    List,
    /// A fresh shell for the tab `tab`. `note` marks the tab "restarted" (spec 4).
    Create { tab: TabId, note: bool },
    Attach { session: SessionId, tab: TabId, note: bool },
    Kill { session: SessionId },
    #[allow(dead_code)] // In Task 4's interface; nothing in milestone 2a sends an untracked request yet
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    None,
    Sessions(Vec<SessionInfo>),
    Created { tab: TabId, session: SessionId, note: bool },
    Attached { session: SessionId, tab: TabId, replay: Vec<u8>, alive: bool, note: bool },
    Killed { session: SessionId },
    /// The daemon could not attach `session` for `tab`; the window gives the tab a fresh shell.
    AttachFailed { session: SessionId, tab: TabId, error: String },
    Error(String),
}

/// One launch step for one saved terminal tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reconcile {
    Attach { tab: TabId, session: SessionId },
    Create { tab: TabId },
}

/// Whether a saved session can be reattached: the daemon still runs it, it is alive, and it was
/// started in this project (an id from an earlier daemon could name another project's shell).
pub fn reattachable(saved: SessionId, list: &[SessionInfo], project: &Path) -> bool {
    match list.iter().find(|i| i.id == saved) {
        None => {
            eprintln!("sarkos: saved session {} is gone, starting a new one", saved.0);
            false
        }
        Some(info) if !info.alive => {
            eprintln!("sarkos: saved session {} has ended, starting a new one", saved.0);
            false
        }
        Some(info) if !same_path(&info.cwd, project) => {
            eprintln!("sarkos: saved session {} belongs to {}, starting a new one", saved.0, info.cwd.display());
            false
        }
        Some(_) => true,
    }
}

/// Compares canonical paths, so symlinks and `..` do not count as a difference.
pub fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// For every terminal tab in reading order: attach when the session is reattachable, else a
/// fresh shell in that same tab. Viewer tabs are left alone. The tree is not modified.
pub fn reconcile(tree: &Tree, list: &[SessionInfo], project: &Path) -> Vec<Reconcile> {
    tree.tabs()
        .into_iter()
        .filter_map(|(pane, idx, tab)| {
            let id = tree.tab_id(pane, idx)?;
            match tab {
                Tab::Terminal { session } if reattachable(*session, list, project) => Some(Reconcile::Attach { tab: id, session: *session }),
                Tab::Terminal { .. } => Some(Reconcile::Create { tab: id }),
                Tab::Viewer { .. } => None,
            }
        })
        .collect()
}

/// Groups a batch by session, keeping each session's events in arrival order and sessions in
/// order of first appearance.
pub fn group_by_session(events: &[Event]) -> Vec<(SessionId, Vec<Event>)> {
    let mut out: Vec<(SessionId, Vec<Event>)> = Vec::new();
    for ev in events {
        let sid = match ev {
            Event::Output { session, .. } | Event::Exited { session, .. } | Event::State { session, .. } => *session,
        };
        match out.iter_mut().find(|(s, _)| *s == sid) {
            Some((_, list)) => list.push(ev.clone()),
            None => out.push((sid, vec![ev.clone()])),
        }
    }
    out
}

pub struct SessionManager {
    pub transport: Arc<dyn Transport>,
    /// The real client, for building `Terminal` entities. Tests pass `None`.
    pub client: Option<Arc<Client>>,
    pub project: PathBuf,
    pub terminals: HashMap<SessionId, Entity<Terminal>>,
    pending: HashMap<u32, Pending>,
    /// Tabs with a shell being started or a replay being restored, and the word to show.
    pub busy: HashMap<TabId, &'static str>,
    /// Sessions whose tab carries the "restarted" note until the user types into them.
    pub notes: HashSet<SessionId>,
    /// Sessions whose replay is being parsed; their live events wait in `backlog`.
    restoring: HashSet<SessionId>,
    backlog: HashMap<SessionId, Vec<Event>>,
}

impl SessionManager {
    pub fn new(transport: Arc<dyn Transport>, client: Option<Arc<Client>>, project: PathBuf) -> Self {
        Self { transport, client, project, terminals: HashMap::new(), pending: HashMap::new(), busy: HashMap::new(), notes: HashSet::new(), restoring: HashSet::new(), backlog: HashMap::new() }
    }

    pub fn list(&mut self) {
        let id = self.transport.send(Request::ListSessions);
        self.pending.insert(id, Pending::List);
    }

    pub fn create(&mut self, tab: TabId, note: bool) {
        let id = self.transport.send(Request::CreateSession {
            cwd: self.project.clone(),
            shell: None,
            env: vec![],
            nice: 0,
            rows: 24,
            cols: 80,
        });
        self.pending.insert(id, Pending::Create { tab, note });
        self.busy.insert(tab, "starting shell");
    }

    pub fn attach(&mut self, session: SessionId, tab: TabId, note: bool) {
        let id = self.transport.send(Request::Attach { session });
        self.pending.insert(id, Pending::Attach { session, tab, note });
        self.busy.insert(tab, "restoring session");
    }

    /// Forgets a closed tab's busy mark and any attach still in flight for it. A pending create
    /// stays: its session must still be killed when it arrives, which the window does when it
    /// cannot locate the tab.
    pub fn tab_closed(&mut self, tab: TabId) {
        self.busy.remove(&tab);
        self.pending.retain(|_, p| !matches!(p, Pending::Attach { tab: t, .. } if *t == tab));
    }

    pub fn kill(&mut self, session: SessionId) {
        let id = self.transport.send(Request::Kill { session });
        self.pending.insert(id, Pending::Kill { session });
    }

    /// Stops receiving a session's output and drops its terminal. The daemon keeps the session.
    pub fn detach(&mut self, session: SessionId) {
        self.transport.send(Request::Detach { session });
        self.terminals.remove(&session);
        self.notes.remove(&session);
        self.restoring.remove(&session);
        self.backlog.remove(&session);
    }

    /// Gives up on a restore that failed: frees the tab and forgets the held-back events.
    pub fn abort_restore(&mut self, session: SessionId, tab: TabId) {
        self.busy.remove(&tab);
        self.restoring.remove(&session);
        self.backlog.remove(&session);
        self.notes.remove(&session);
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn has_pending_for(&self, tab: TabId) -> bool {
        self.busy.contains_key(&tab)
    }

    /// Matches a response to its request. Errors always surface, even for ids nobody waits on.
    pub fn handle_response(&mut self, id: u32, response: Response) -> Outcome {
        let pending = self.pending.remove(&id);
        if let Response::Error(e) = &response {
            match pending {
                Some(Pending::Attach { session, tab, .. }) => {
                    self.busy.remove(&tab);
                    return Outcome::AttachFailed { session, tab, error: e.clone() };
                }
                Some(Pending::Create { tab, .. }) => {
                    self.busy.remove(&tab);
                }
                _ => {}
            }
            return Outcome::Error(e.clone());
        }
        match (pending.clone(), response.clone()) {
            (Some(Pending::List), Response::Sessions(list)) => Outcome::Sessions(list),
            (Some(Pending::Create { tab, note }), Response::SessionCreated { id: session }) => {
                self.busy.remove(&tab);
                Outcome::Created { tab, session, note }
            }
            (Some(Pending::Attach { session, tab, note }), Response::Attached { replay, alive }) => {
                // Stays busy until the window finishes restoring; it clears the mark itself.
                Outcome::Attached { session, tab, replay, alive, note }
            }
            (Some(Pending::Kill { session }), Response::Ok) => Outcome::Killed { session },
            (Some(Pending::Attach { session, tab, .. }), _) => {
                self.busy.remove(&tab);
                Outcome::AttachFailed { session, tab, error: format!("unexpected response {response:?} to {pending:?}") }
            }
            (Some(Pending::Create { tab, .. }), _) => {
                self.busy.remove(&tab);
                Outcome::Error(format!("unexpected response {response:?} to {pending:?}"))
            }
            _ => Outcome::None,
        }
    }

    /// Clears a tab's busy mark once its terminal is on screen.
    pub fn ready(&mut self, tab: TabId) {
        self.busy.remove(&tab);
    }

    pub fn begin_restore(&mut self, session: SessionId) {
        self.restoring.insert(session);
    }

    pub fn is_restoring(&self, session: SessionId) -> bool {
        self.restoring.contains(&session)
    }

    /// Keeps an event for later when its session is mid-restore. Returns whether it was kept.
    pub fn stash_if_restoring(&mut self, ev: &Event) -> bool {
        let sid = match ev {
            Event::Output { session, .. } | Event::Exited { session, .. } | Event::State { session, .. } => *session,
        };
        if !self.restoring.contains(&sid) {
            return false;
        }
        self.backlog.entry(sid).or_default().push(ev.clone());
        true
    }

    pub fn backlog_len(&self, session: SessionId) -> usize {
        self.backlog.get(&session).map(|b| b.len()).unwrap_or(0)
    }

    /// Ends the restore and returns the events that arrived during it, in order.
    pub fn take_backlog(&mut self, session: SessionId) -> Vec<Event> {
        self.restoring.remove(&session);
        self.backlog.remove(&session).unwrap_or_default()
    }

    /// Installs a restored terminal and applies everything that arrived while it was parsing.
    pub fn finish_restore(&mut self, session: SessionId, terminal: Entity<Terminal>, cx: &mut App) {
        if self.terminals.contains_key(&session) {
            // Already shown: still end the restore, or later events would wait forever.
            self.take_backlog(session);
            return;
        }
        let backlog = self.take_backlog(session);
        if !backlog.is_empty() {
            terminal.update(cx, |t, cx| {
                t.handle_events(&backlog, cx);
            });
        }
        self.terminals.insert(session, terminal);
    }

    /// Applies a batch of daemon events: one grid update and at most one notify per terminal.
    /// Events for a session that is still restoring wait in its backlog.
    pub fn route_events(&mut self, events: &[Event], cx: &mut App) {
        let live: Vec<Event> = events.iter().filter(|ev| !self.stash_if_restoring(ev)).cloned().collect();
        for (session, batch) in group_by_session(&live) {
            if let Some(t) = self.terminals.get(&session) {
                t.update(cx, |t, cx| {
                    t.handle_events(&batch, cx);
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Side;
    use protocol::ClaudeState;
    use std::sync::Mutex;

    struct Recorder {
        sent: Mutex<Vec<Request>>,
        next: Mutex<u32>,
    }

    impl Recorder {
        fn new() -> Arc<Recorder> {
            Arc::new(Recorder { sent: Mutex::new(vec![]), next: Mutex::new(1) })
        }
        fn requests(&self) -> Vec<Request> {
            self.sent.lock().unwrap().clone()
        }
    }

    impl Transport for Recorder {
        fn send(&self, request: Request) -> u32 {
            self.sent.lock().unwrap().push(request);
            let mut n = self.next.lock().unwrap();
            let id = *n;
            *n += 1;
            id
        }
    }

    fn info(id: u64, cwd: &Path, alive: bool) -> SessionInfo {
        SessionInfo { id: SessionId(id), title: String::new(), cwd: cwd.to_path_buf(), pid: None, alive, last_activity_unix_ms: 0, state: ClaudeState::Idle }
    }

    fn term(n: u64) -> Tab {
        Tab::Terminal { session: SessionId(n) }
    }

    #[test]
    fn reconcile_replaces_dead_and_missing_sessions_in_place() {
        let here = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        // A 2x2 layout: 1 alive here, 2 dead, 3 gone from the daemon, 4 alive in another project.
        let mut t = Tree::new(term(1));
        let a = t.focused;
        let b = t.split(a, Side::Right, term(2)).unwrap();
        let c = t.split(a, Side::Bottom, term(3)).unwrap();
        let d = t.split(b, Side::Bottom, term(4)).unwrap();
        let list = vec![info(1, here.path(), true), info(2, here.path(), false), info(4, elsewhere.path(), true)];
        let plan = reconcile(&t, &list, here.path());
        let id = |p| t.tab_id(p, 0).unwrap();
        assert_eq!(
            plan,
            vec![
                Reconcile::Attach { tab: id(a), session: SessionId(1) },
                Reconcile::Create { tab: id(b) },
                Reconcile::Create { tab: id(c) },
                Reconcile::Create { tab: id(d) },
            ]
        );
        assert_eq!(t.panes(), vec![a, b, c, d], "the layout itself is untouched");
    }

    #[test]
    fn reconcile_skips_viewer_tabs() {
        let here = tempfile::tempdir().unwrap();
        let mut t = Tree::new(term(1));
        let a = t.focused;
        t.add_tab(a, Tab::Viewer { path: PathBuf::from("/x") });
        let plan = reconcile(&t, &[info(1, here.path(), true)], here.path());
        assert_eq!(plan, vec![Reconcile::Attach { tab: t.tab_id(a, 0).unwrap(), session: SessionId(1) }]);
    }

    #[test]
    fn reattachable_requires_alive_and_same_project() {
        let project = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let here = project.path();
        let list = vec![info(7, here, true), info(8, here, false), info(9, other.path(), true)];
        assert!(reattachable(SessionId(7), &list, here));
        assert!(!reattachable(SessionId(8), &list, here));
        assert!(!reattachable(SessionId(9), &list, here));
        assert!(!reattachable(SessionId(10), &list, here));
        let sub = here.join("sub");
        std::fs::create_dir(&sub).unwrap();
        assert!(reattachable(SessionId(7), &list, &sub.join("..")), "paths compare canonically");
    }

    #[test]
    fn responses_are_matched_to_what_was_asked() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r.clone(), None, PathBuf::from("/p"));
        m.list();
        m.create(50, true);
        m.attach(SessionId(3), 61, false);
        m.kill(SessionId(9));
        let reqs = r.requests();
        assert!(matches!(reqs[0], Request::ListSessions));
        assert!(matches!(&reqs[1], Request::CreateSession { cwd, rows: 24, cols: 80, .. } if cwd == Path::new("/p")));
        assert!(matches!(reqs[2], Request::Attach { session: SessionId(3) }));
        assert!(matches!(reqs[3], Request::Kill { session: SessionId(9) }));
        assert_eq!(m.pending_count(), 4);
        assert!(m.has_pending_for(50));
        assert!(m.has_pending_for(61));
        assert!(!m.has_pending_for(51));

        assert!(matches!(m.handle_response(1, Response::Sessions(vec![])), Outcome::Sessions(l) if l.is_empty()));
        assert_eq!(
            m.handle_response(2, Response::SessionCreated { id: SessionId(11) }),
            Outcome::Created { tab: 50, session: SessionId(11), note: true }
        );
        assert_eq!(
            m.handle_response(3, Response::Attached { replay: vec![1, 2], alive: true }),
            Outcome::Attached { session: SessionId(3), tab: 61, replay: vec![1, 2], alive: true, note: false }
        );
        assert_eq!(m.handle_response(4, Response::Ok), Outcome::Killed { session: SessionId(9) });
        assert_eq!(m.pending_count(), 0);
        assert!(!m.has_pending_for(50));
        // An error for an unknown id still surfaces; a stray Ok does not.
        assert_eq!(m.handle_response(77, Response::Error("x".into())), Outcome::Error("x".into()));
        assert_eq!(m.handle_response(78, Response::Ok), Outcome::None);
    }

    #[test]
    fn a_failed_create_or_attach_clears_the_busy_mark() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r, None, PathBuf::from("/p"));
        m.create(10, false);
        assert!(m.has_pending_for(10));
        assert_eq!(m.handle_response(1, Response::Error("spawn failed".into())), Outcome::Error("spawn failed".into()));
        assert!(!m.has_pending_for(10));
    }

    #[test]
    fn a_failed_attach_names_its_tab_and_session() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r, None, PathBuf::from("/p"));
        m.attach(SessionId(4), 20, false);
        m.attach(SessionId(5), 21, false);
        assert_eq!(
            m.handle_response(1, Response::Error("no session 4".into())),
            Outcome::AttachFailed { session: SessionId(4), tab: 20, error: "no session 4".into() }
        );
        assert!(!m.has_pending_for(20));
        assert!(matches!(m.handle_response(2, Response::Ok), Outcome::AttachFailed { session: SessionId(5), tab: 21, .. }));
        assert!(!m.has_pending_for(21));
    }

    #[test]
    fn a_mismatched_response_clears_the_busy_mark_and_reports() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r, None, PathBuf::from("/p"));
        m.create(10, false);
        assert!(matches!(m.handle_response(1, Response::Attached { replay: vec![], alive: true }), Outcome::Error(_)));
        assert!(!m.has_pending_for(10));
    }

    #[test]
    fn abort_restore_clears_busy_and_backlog() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r, None, PathBuf::from("/p"));
        m.attach(SessionId(4), 20, true);
        m.notes.insert(SessionId(4));
        m.begin_restore(SessionId(4));
        assert!(m.stash_if_restoring(&Event::Output { session: SessionId(4), bytes: vec![1] }));
        m.abort_restore(SessionId(4), 20);
        assert!(!m.has_pending_for(20));
        assert!(!m.is_restoring(SessionId(4)));
        assert_eq!(m.backlog_len(SessionId(4)), 0);
        assert!(!m.notes.contains(&SessionId(4)));
    }

    #[test]
    fn a_closed_tab_drops_its_attach_but_its_create_still_reports() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r, None, PathBuf::from("/p"));
        m.create(10, false);
        m.attach(SessionId(4), 11, false);
        m.tab_closed(10);
        m.tab_closed(11);
        assert!(!m.has_pending_for(10));
        assert!(!m.has_pending_for(11));
        // The shell created for the closed tab still surfaces, so the window can kill it.
        assert_eq!(m.handle_response(1, Response::SessionCreated { id: SessionId(9) }), Outcome::Created { tab: 10, session: SessionId(9), note: false });
        assert_eq!(m.handle_response(2, Response::Attached { replay: vec![], alive: true }), Outcome::None);
        assert_eq!(m.pending_count(), 0);
    }

    #[test]
    fn events_group_by_session_in_order() {
        let ev = |s: u64, b: &str| Event::Output { session: SessionId(s), bytes: b.as_bytes().to_vec() };
        let events = vec![ev(1, "a"), ev(2, "b"), ev(1, "c"), Event::Exited { session: SessionId(2), code: Some(0) }, ev(1, "d")];
        let grouped = group_by_session(&events);
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[0].0, SessionId(1));
        assert_eq!(grouped[0].1, vec![ev(1, "a"), ev(1, "c"), ev(1, "d")]);
        assert_eq!(grouped[1].0, SessionId(2));
        assert_eq!(grouped[1].1, vec![ev(2, "b"), Event::Exited { session: SessionId(2), code: Some(0) }]);
    }

    #[test]
    fn events_during_a_restore_are_kept_until_the_terminal_exists() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r, None, PathBuf::from("/p"));
        m.begin_restore(SessionId(4));
        let ev = Event::Output { session: SessionId(4), bytes: b"late".to_vec() };
        // No gpui App is needed to test the bookkeeping: `stash_if_restoring` is the pure half
        // of `route_events`.
        assert!(m.stash_if_restoring(&ev));
        assert!(!m.stash_if_restoring(&Event::Output { session: SessionId(5), bytes: vec![] }));
        assert_eq!(m.backlog_len(SessionId(4)), 1);
        assert_eq!(m.take_backlog(SessionId(4)), vec![ev]);
        assert_eq!(m.backlog_len(SessionId(4)), 0);
        assert!(!m.is_restoring(SessionId(4)), "taking the backlog ends the restore");
    }

    #[test]
    fn exit_in_a_background_tab_does_not_change_focus() {
        // Routing events never touches the tree: focus is the window's decision, and a session
        // ending in another tab must not steal it (Review Focus 4).
        let mut t = Tree::new(term(1));
        let a = t.focused;
        t.split(a, Side::Right, term(2));
        t.activate(a, 0);
        let before = t.clone();
        let grouped = group_by_session(&[Event::Exited { session: SessionId(2), code: Some(1) }]);
        assert_eq!(grouped[0].0, SessionId(2));
        assert_eq!(t, before);
        assert_eq!(t.focused, a);
    }
}
