//! Owns every terminal entity and the conversation with the daemon: which request asked for
//! what, which session sits in which tab, and the launch-time reconciliation of the saved layout
//! against the sessions the daemon still runs (spec 4: a missing session reopens as a fresh
//! shell in the same cwd with a note in the tab).


use crate::client::Client;
use crate::terminal::Terminal;
use crate::tree::{Tab, TabId, Tree};
use gpui::{App, Entity};
use protocol::ClaudeState;
use protocol::{Event, Request, Response, SessionDetail, SessionId, SessionInfo};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const SCROLLBACK: usize = 20_000;

/// How the daemon's refusal of a create or kill starts while an upgrade holds its slot.
const UPGRADE_BUSY: &str = "an upgrade is in progress";

/// The word after a tab's title: the session is new in place of one the daemon lost, or was
/// recreated by `sarkosd --recover` after a restart. Both clear when the user types into the tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Note {
    Restarted,
    Resumed,
}

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
    /// The daemon replacing itself; answered only on failure (success ends the connection).
    Upgrade,
    /// The daemon starting the installed window once this one disconnects.
    Relaunch,
    #[allow(dead_code)] // In Task 4's interface; nothing in milestone 2a sends an untracked request yet
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    None,
    Sessions(Vec<SessionDetail>),
    Created { tab: TabId, session: SessionId, note: bool },
    Attached { session: SessionId, tab: TabId, replay: Vec<u8>, alive: bool, note: bool },
    Killed { session: SessionId },
    /// The daemon could not attach `session` for `tab`; the window gives the tab a fresh shell.
    AttachFailed { session: SessionId, tab: TabId, error: String },
    Error(String),
    /// The daemon refused or failed the upgrade and keeps running as it was.
    UpgradeFailed(String),
    /// A version 2 daemon's answer to Hello: what it runs and which protocol versions it speaks.
    DaemonHello { version: String, protocol_min: u32, protocol_max: u32 },
    /// The daemon will start the installed window once this one is gone: the window may quit.
    RelaunchAccepted,
    /// The daemon will not relaunch the window, which therefore stays.
    RelaunchRefused(String),
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
    /// Sessions whose tab carries a note until the user types into them.
    pub notes: HashMap<SessionId, Note>,
    /// What each session's agent is doing (spec 5), from the list at connect and `Event::State`
    /// after; a BEL marks waiting too. A session not here is idle.
    pub states: HashMap<SessionId, ClaudeState>,
    /// Sessions whose replay is being parsed; their live events wait in `backlog`.
    restoring: HashSet<SessionId>,
    backlog: HashMap<SessionId, Vec<Event>>,
    /// Between `disconnect` and the next `set_client`: kills wait in `kills_to_resend`.
    offline: bool,
    /// Kills asked for while disconnected (a closed tab's grace ran out) or refused during an
    /// upgrade, sent on the next connection or by `resend_kills`.
    kills_to_resend: Vec<SessionId>,
    /// The `[terminal]` section: scrollback for terminals built from now on, and the shell and
    /// nice level new sessions start with.
    pub scrollback: usize,
    pub shell: Option<String>,
    pub nice: i32,
}

impl SessionManager {
    pub fn new(transport: Arc<dyn Transport>, client: Option<Arc<Client>>, project: PathBuf) -> Self {
        Self { transport, client, project, terminals: HashMap::new(), pending: HashMap::new(), busy: HashMap::new(), notes: HashMap::new(), states: HashMap::new(), restoring: HashSet::new(), backlog: HashMap::new(), offline: false, kills_to_resend: Vec::new(), scrollback: SCROLLBACK, shell: None, nice: 0 }
    }

    /// A new connection: requests go to it from now on.
    pub fn set_client(&mut self, client: Arc<Client>) {
        self.transport = client.clone();
        self.client = Some(client);
        self.connected();
    }

    #[cfg(test)]
    pub fn set_client_for_test(&mut self, transport: Arc<dyn Transport>) {
        self.transport = transport;
        self.connected();
    }

    /// Forgets what was sent on the dead connection, whose ids the new one reuses (its heartbeats
    /// would match them); the reconcile that follows asks again for every terminal tab. Kills
    /// asked for meanwhile go out now.
    fn connected(&mut self) {
        self.pending.clear();
        self.offline = false;
        self.resend_kills();
    }

    /// Sends the kills that are waiting, unless the connection is down: then the next one does.
    /// The window calls this every second, for kills refused by an upgrade that failed its check
    /// and so brought no reconnect.
    pub fn resend_kills(&mut self) {
        if self.offline {
            return;
        }
        for session in std::mem::take(&mut self.kills_to_resend) {
            self.kill(session);
        }
    }

    /// The connection ended: every terminal is gone with it (the next attach replays), nothing
    /// in flight can be answered, and no tab is busy with anything from the old connection.
    pub fn disconnect(&mut self) {
        self.offline = true;
        // A kill nobody answered may never have been read (one queued behind an upgrade's exec).
        // Sending it again is harmless: a session already gone answers as killed.
        let unanswered = self.pending.values().filter_map(|p| match p {
            Pending::Kill { session } => Some(*session),
            _ => None,
        });
        self.kills_to_resend.extend(unanswered);
        self.terminals.clear();
        self.pending.clear();
        self.busy.clear();
        self.notes.clear();
        self.states.clear();
        self.restoring.clear();
        self.backlog.clear();
    }

    /// The `[terminal]` section, applied to shells started from now on (spec 7).
    pub fn set_defaults(&mut self, scrollback: usize, shell: Option<String>, nice: i32) {
        self.scrollback = scrollback;
        self.shell = shell;
        self.nice = nice;
    }

    /// Whether the connection is down and the window is waiting for the next one.
    pub fn is_offline(&self) -> bool {
        self.offline
    }

    /// Asks the daemon to replace itself with `exe`.
    pub fn upgrade(&mut self, exe: PathBuf) {
        let id = self.transport.send(Request::Upgrade { exe });
        self.pending.insert(id, Pending::Upgrade);
    }

    /// Asks the daemon to start `exe` in this project once this window disconnects.
    pub fn relaunch(&mut self, exe: PathBuf) {
        let id = self.transport.send(Request::RelaunchClient { exe, cwd: self.project.clone() });
        self.pending.insert(id, Pending::Relaunch);
    }

    pub fn list(&mut self) {
        let id = self.transport.send(Request::ListSessionsDetailed);
        self.pending.insert(id, Pending::List);
    }

    /// Records what the daemon's list says about each session. A "resumed" note comes back only
    /// until the user types into the session: the daemon clears its `recovered` on that Input.
    pub fn note_list(&mut self, list: &[SessionDetail]) {
        for d in list {
            self.states.insert(d.info.id, d.info.state);
            if d.recovered {
                self.notes.insert(d.info.id, Note::Resumed);
            }
        }
    }

    pub fn note_state(&mut self, session: SessionId, state: ClaudeState) {
        self.states.insert(session, state);
    }

    /// The user looked at or typed into a waiting tab: idle now, and the daemon is told so a
    /// reconnect or another window agrees. True when there was something to acknowledge.
    pub fn acknowledge(&mut self, session: SessionId) -> bool {
        if self.states.get(&session) != Some(&ClaudeState::Waiting) {
            return false;
        }
        self.states.insert(session, ClaudeState::Idle);
        self.transport.send(Request::SetState { session, state: ClaudeState::Idle });
        true
    }

    /// Terminal tabs in the tree whose session waits on the user (the status bar's count).
    pub fn waiting_count(&self, tree: &Tree) -> usize {
        tree.tabs().into_iter().filter(|(_, _, tab)| matches!(tab, Tab::Terminal { session } if self.states.get(session) == Some(&ClaudeState::Waiting))).count()
    }

    /// Whether a terminal tab in the tree shows a working agent. The list carries every
    /// project's sessions, and one no tab shows gets no `State` events to end its `Working`.
    pub fn any_working(&self, tree: &Tree) -> bool {
        tree.tabs().into_iter().any(|(_, _, tab)| matches!(tab, Tab::Terminal { session } if self.states.get(session) == Some(&ClaudeState::Working)))
    }

    /// The user is quitting this window: a restart must not reopen it (section 11 step 4).
    pub fn goodbye(&mut self) {
        self.transport.send(Request::Goodbye);
    }

    pub fn create(&mut self, tab: TabId, note: bool) {
        self.create_in(tab, note, self.project.clone());
    }

    /// `create`, with the shell starting in `cwd` rather than the project (a worktree).
    pub fn create_in(&mut self, tab: TabId, note: bool, cwd: PathBuf) {
        let id = self.transport.send(Request::CreateSession {
            cwd,
            shell: self.shell.clone(),
            env: vec![],
            nice: self.nice,
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
        if self.offline {
            self.kills_to_resend.push(session);
            return;
        }
        let id = self.transport.send(Request::Kill { session });
        self.pending.insert(id, Pending::Kill { session });
    }

    /// Stops receiving a session's output and drops its terminal. The daemon keeps the session.
    pub fn detach(&mut self, session: SessionId) {
        self.transport.send(Request::Detach { session });
        self.terminals.remove(&session);
        self.notes.remove(&session);
        // No tab shows it now and no `State` event reaches it: a reopen starts from idle rather
        // than from a stale state.
        self.states.remove(&session);
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
        if let Response::Hello { daemon_version, protocol_min, protocol_max } = &response {
            return Outcome::DaemonHello { version: daemon_version.clone(), protocol_min: *protocol_min, protocol_max: *protocol_max };
        }
        if let Response::Error(e) = &response {
            match pending {
                Some(Pending::Attach { session, tab, .. }) => {
                    self.busy.remove(&tab);
                    return Outcome::AttachFailed { session, tab, error: e.clone() };
                }
                Some(Pending::Create { tab, .. }) => {
                    self.busy.remove(&tab);
                }
                Some(Pending::Upgrade) => return Outcome::UpgradeFailed(e.clone()),
                Some(Pending::Relaunch) => return Outcome::RelaunchRefused(e.clone()),
                Some(Pending::Kill { session }) if e.starts_with("no session") => {
                    // Reaped before the kill arrived: the session is gone, which is what we asked.
                    eprintln!("sarkos: session {} was already gone when killed", session.0);
                    return Outcome::Killed { session };
                }
                Some(Pending::Kill { session }) if e.starts_with(UPGRADE_BUSY) => {
                    // Refused, not failed: dropped here, the shell would live on unseen.
                    eprintln!("sarkos: kill of session {} refused during an upgrade; sending it again", session.0);
                    self.kills_to_resend.push(session);
                    return Outcome::None;
                }
                _ => {}
            }
            return Outcome::Error(e.clone());
        }
        match (pending.clone(), response.clone()) {
            (Some(Pending::List), Response::SessionsDetailed(list)) => Outcome::Sessions(list),
            (Some(Pending::Create { tab, note }), Response::SessionCreated { id: session }) => {
                self.busy.remove(&tab);
                Outcome::Created { tab, session, note }
            }
            (Some(Pending::Attach { session, tab, note }), Response::Attached { replay, alive }) => {
                // Stays busy until the window finishes restoring; it clears the mark itself.
                Outcome::Attached { session, tab, replay, alive, note }
            }
            (Some(Pending::Kill { session }), Response::Ok) => Outcome::Killed { session },
            (Some(Pending::Relaunch), Response::Ok) => Outcome::RelaunchAccepted,
            (Some(Pending::Relaunch), _) => Outcome::RelaunchRefused(format!("unexpected response {response:?} to {pending:?}")),
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
            let rang = terminal.update(cx, |t, cx| {
                t.handle_events(&backlog, cx);
                std::mem::take(&mut t.bell_pending)
            });
            if rang {
                self.states.insert(session, ClaudeState::Waiting);
            }
        }
        self.terminals.insert(session, terminal);
    }

    /// The window's half of a batch: what each session's agent is doing. Recorded at once, even
    /// for a session mid-restore or with no terminal yet; a shell that ended runs no agent.
    pub fn note_events(&mut self, events: &[Event]) {
        for ev in events {
            match ev {
                Event::State { session, state } => {
                    self.states.insert(*session, *state);
                }
                Event::Exited { session, .. } => {
                    self.states.remove(session);
                }
                Event::Output { .. } => {}
            }
        }
    }

    /// Applies a batch of daemon events: one grid update and at most one notify per terminal.
    /// Events for a session that is still restoring wait in its backlog.
    pub fn route_events(&mut self, events: &[Event], cx: &mut App) {
        self.note_events(events);
        let live: Vec<Event> = events.iter().filter(|ev| !self.stash_if_restoring(ev)).cloned().collect();
        for (session, batch) in group_by_session(&live) {
            if let Some(t) = self.terminals.get(&session) {
                let rang = t.update(cx, |t, cx| {
                    t.handle_events(&batch, cx);
                    std::mem::take(&mut t.bell_pending)
                });
                if rang {
                    // Spec 5: a BEL means "needs you" even without hooks.
                    self.states.insert(session, ClaudeState::Waiting);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Side;
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

    fn detail(id: u64, cwd: &Path, alive: bool, state: ClaudeState, recovered: bool) -> protocol::SessionDetail {
        let mut i = info(id, cwd, alive);
        i.state = state;
        protocol::SessionDetail { info: i, recovered, agent: None }
    }

    #[test]
    fn the_list_is_detailed_and_records_each_sessions_state_and_resumed_note() {
        let rec = Recorder::new();
        let mut m = SessionManager::new(rec.clone(), None, PathBuf::from("/p"));
        m.list();
        assert_eq!(rec.requests(), vec![Request::ListSessionsDetailed]);
        let list = vec![detail(1, Path::new("/p"), true, ClaudeState::Working, false), detail(2, Path::new("/p"), true, ClaudeState::Waiting, true)];
        let outcome = m.handle_response(1, Response::SessionsDetailed(list.clone()));
        assert_eq!(outcome, Outcome::Sessions(list));
        // The window records them (handle_outcome does, through `note_list`):
        let Outcome::Sessions(back) = outcome else { unreachable!() };
        m.note_list(&back);
        assert_eq!(m.states.get(&SessionId(1)), Some(&ClaudeState::Working));
        assert_eq!(m.states.get(&SessionId(2)), Some(&ClaudeState::Waiting));
        assert_eq!(m.notes.get(&SessionId(2)), Some(&Note::Resumed));
        assert_eq!(m.notes.get(&SessionId(1)), None);
    }

    #[test]
    fn a_state_event_updates_the_state_and_acknowledge_tells_the_daemon_idle() {
        let rec = Recorder::new();
        let mut m = SessionManager::new(rec.clone(), None, PathBuf::from("/p"));
        m.note_state(SessionId(3), ClaudeState::Waiting);
        assert_eq!(m.states.get(&SessionId(3)), Some(&ClaudeState::Waiting));
        assert!(m.acknowledge(SessionId(3)), "a waiting session is acknowledged");
        assert_eq!(m.states.get(&SessionId(3)), Some(&ClaudeState::Idle));
        assert_eq!(rec.requests(), vec![Request::SetState { session: SessionId(3), state: ClaudeState::Idle }]);
        assert!(!m.acknowledge(SessionId(3)), "nothing to acknowledge twice");
        m.note_state(SessionId(3), ClaudeState::Working);
        assert!(!m.acknowledge(SessionId(3)), "working is not waiting");
        assert_eq!(rec.requests().len(), 1);
    }

    #[test]
    fn waiting_count_covers_terminal_tabs_in_the_tree_and_any_working_is_true_with_one() {
        let rec = Recorder::new();
        let mut m = SessionManager::new(rec, None, PathBuf::from("/p"));
        let mut tree = Tree::new(Tab::Terminal { session: SessionId(1) });
        let root = tree.panes()[0];
        tree.add_tab(root, Tab::Terminal { session: SessionId(2) });
        tree.add_tab(root, Tab::Viewer { path: "/f".into() });
        m.note_state(SessionId(1), ClaudeState::Waiting);
        m.note_state(SessionId(2), ClaudeState::Waiting);
        m.note_state(SessionId(9), ClaudeState::Waiting);
        assert_eq!(m.waiting_count(&tree), 2, "a session no tab shows is not counted");
        assert!(!m.any_working(&tree));
        m.note_state(SessionId(8), ClaudeState::Working);
        assert!(!m.any_working(&tree), "another project's session, or one reconcile replaced, never pulses here");
        m.note_state(SessionId(2), ClaudeState::Working);
        assert!(m.any_working(&tree));
        assert_eq!(m.waiting_count(&tree), 1);
    }

    #[test]
    fn goodbye_is_sent_once_and_disconnect_forgets_states() {
        let rec = Recorder::new();
        let mut m = SessionManager::new(rec.clone(), None, PathBuf::from("/p"));
        m.note_state(SessionId(1), ClaudeState::Working);
        m.goodbye();
        assert_eq!(rec.requests(), vec![Request::Goodbye]);
        m.disconnect();
        assert!(m.states.is_empty(), "the next list says what each session does");
    }

    #[test]
    fn states_follow_events_and_end_with_the_shell_or_the_tab() {
        let rec = Recorder::new();
        let mut m = SessionManager::new(rec, None, PathBuf::from("/p"));
        m.begin_restore(SessionId(1));
        m.note_events(&[Event::State { session: SessionId(1), state: ClaudeState::Working }, Event::State { session: SessionId(2), state: ClaudeState::Working }]);
        assert_eq!(m.states.get(&SessionId(1)), Some(&ClaudeState::Working), "recorded mid-restore");
        assert_eq!(m.states.get(&SessionId(2)), Some(&ClaudeState::Working), "recorded with no terminal");
        m.note_events(&[Event::Exited { session: SessionId(1), code: Some(0) }]);
        assert_eq!(m.states.get(&SessionId(1)), None);
        m.detach(SessionId(2));
        assert_eq!(m.states.get(&SessionId(2)), None, "a closed tab's session forgets its state");
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
    fn create_in_starts_the_shell_in_the_given_folder() {
        let rec = Recorder::new();
        let mut m = SessionManager::new(rec.clone(), None, PathBuf::from("/p"));
        m.create_in(7, false, PathBuf::from("/elsewhere"));
        assert!(matches!(&rec.requests()[0], Request::CreateSession { cwd, .. } if cwd == Path::new("/elsewhere")));
    }

    #[test]
    fn create_uses_the_configured_shell_and_nice() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r.clone(), None, PathBuf::from("/p"));
        m.set_defaults(5_000, Some("/bin/bash".into()), 5);
        assert_eq!(m.scrollback, 5_000);
        m.create(1, false);
        match &r.requests()[0] {
            Request::CreateSession { shell, nice, .. } => {
                assert_eq!(shell.as_deref(), Some("/bin/bash"));
                assert_eq!(*nice, 5);
            }
            other => panic!("{other:?}"),
        }
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
        assert!(matches!(reqs[0], Request::ListSessionsDetailed));
        assert!(matches!(&reqs[1], Request::CreateSession { cwd, rows: 24, cols: 80, .. } if cwd == Path::new("/p")));
        assert!(matches!(reqs[2], Request::Attach { session: SessionId(3) }));
        assert!(matches!(reqs[3], Request::Kill { session: SessionId(9) }));
        assert_eq!(m.pending_count(), 4);
        assert!(m.has_pending_for(50));
        assert!(m.has_pending_for(61));
        assert!(!m.has_pending_for(51));

        assert!(matches!(m.handle_response(1, Response::SessionsDetailed(vec![])), Outcome::Sessions(l) if l.is_empty()));
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
        m.notes.insert(SessionId(4), Note::Restarted);
        m.begin_restore(SessionId(4));
        assert!(m.stash_if_restoring(&Event::Output { session: SessionId(4), bytes: vec![1] }));
        m.abort_restore(SessionId(4), 20);
        assert!(!m.has_pending_for(20));
        assert!(!m.is_restoring(SessionId(4)));
        assert_eq!(m.backlog_len(SessionId(4)), 0);
        assert!(!m.notes.contains_key(&SessionId(4)));
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

    #[test]
    fn a_kill_of_a_reaped_session_is_a_quiet_success() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r.clone(), None, PathBuf::from("/p"));
        m.kill(SessionId(9));
        assert_eq!(m.handle_response(1, Response::Error("no session 9".into())), Outcome::Killed { session: SessionId(9) });
        m.kill(SessionId(10));
        assert_eq!(m.handle_response(2, Response::Error("permission denied".into())), Outcome::Error("permission denied".into()), "other errors still surface");
    }

    #[test]
    fn only_the_upgrade_request_s_own_error_fails_the_upgrade() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r.clone(), None, PathBuf::from("/p"));
        m.upgrade(PathBuf::from("/bin/sarkosd"));
        assert!(matches!(&r.requests()[..], [Request::Upgrade { exe }] if exe == &PathBuf::from("/bin/sarkosd")));
        m.kill(SessionId(9));
        // A kill refused while the daemon holds the upgrade slot is not the upgrade failing.
        let busy = "an upgrade is in progress; retry in a moment";
        assert_eq!(m.handle_response(2, Response::Error(busy.into())), Outcome::None);
        assert_eq!(m.handle_response(1, Response::Error("execv failed".into())), Outcome::UpgradeFailed("execv failed".into()));
        assert!(!m.is_offline());
        m.disconnect();
        assert!(m.is_offline());
    }

    #[test]
    fn a_kill_refused_during_an_upgrade_is_sent_again_without_a_banner() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r.clone(), None, PathBuf::from("/p"));
        m.kill(SessionId(9));
        let busy = "an upgrade is in progress; retry in a moment";
        assert_eq!(m.handle_response(1, Response::Error(busy.into())), Outcome::None);
        assert_eq!(m.kills_to_resend, vec![SessionId(9)]);
        // The upgrade failed its check, so no reconnect follows: the retry tick sends it again.
        m.resend_kills();
        assert!(m.kills_to_resend.is_empty());
        assert!(matches!(r.requests()[..], [Request::Kill { session: SessionId(9) }, Request::Kill { session: SessionId(9) }]));
        assert_eq!(m.handle_response(2, Response::Ok), Outcome::Killed { session: SessionId(9) });
        // Nothing goes out while the connection is down; the next one sends it.
        m.kill(SessionId(10));
        assert_eq!(m.handle_response(3, Response::Error(busy.into())), Outcome::None);
        m.disconnect();
        m.resend_kills();
        assert_eq!(r.requests().len(), 3);
        let r2 = Recorder::new();
        m.set_client_for_test(r2.clone());
        assert!(matches!(r2.requests()[..], [Request::Kill { session: SessionId(10) }]));
    }

    #[test]
    fn a_kill_unanswered_when_the_connection_drops_is_sent_again() {
        // The upgrading window's own kill waits unread behind its Upgrade, and the exec closes
        // the connection: nothing will answer it.
        let r = Recorder::new();
        let mut m = SessionManager::new(r.clone(), None, PathBuf::from("/p"));
        m.upgrade(PathBuf::from("/bin/sarkosd"));
        m.kill(SessionId(9));
        m.disconnect();
        let r2 = Recorder::new();
        m.set_client_for_test(r2.clone());
        assert!(matches!(r2.requests()[..], [Request::Kill { session: SessionId(9) }]));
    }

    #[test]
    fn an_accepted_relaunch_lets_the_window_quit() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r.clone(), None, PathBuf::from("/p"));
        m.relaunch(PathBuf::from("/Applications/bin/sarkos"));
        assert!(matches!(&r.requests()[..], [Request::RelaunchClient { exe, cwd }] if exe == Path::new("/Applications/bin/sarkos") && cwd == Path::new("/p")));
        assert_eq!(m.handle_response(1, Response::Ok), Outcome::RelaunchAccepted);
        assert_eq!(m.pending_count(), 0);
    }

    #[test]
    fn a_refused_relaunch_keeps_the_window() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r.clone(), None, PathBuf::from("/p"));
        m.relaunch(PathBuf::from("/tmp/evil"));
        let refused = "/tmp/evil is not a sarkos binary owned by this user";
        assert_eq!(m.handle_response(1, Response::Error(refused.into())), Outcome::RelaunchRefused(refused.into()));
        // Any other answer is not an acceptance either.
        m.relaunch(PathBuf::from("/tmp/sarkos"));
        assert!(matches!(m.handle_response(2, Response::Sessions(vec![])), Outcome::RelaunchRefused(_)));
    }

    #[test]
    fn a_hello_answer_reports_the_daemon_version() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r.clone(), None, PathBuf::from("/p"));
        let hello = Response::Hello { daemon_version: "sarkosd 0.2.0 (abc1234)".into(), protocol_min: 1, protocol_max: 2 };
        assert_eq!(
            m.handle_response(1, hello),
            Outcome::DaemonHello { version: "sarkosd 0.2.0 (abc1234)".into(), protocol_min: 1, protocol_max: 2 }
        );
        assert_eq!(m.handle_response(1, Response::Ok), Outcome::None, "an old daemon answers Ok");
    }

    #[test]
    fn disconnect_forgets_terminals_pendings_and_marks() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r.clone(), None, PathBuf::from("/p"));
        m.create(50, true);
        m.attach(SessionId(3), 61, false);
        m.begin_restore(SessionId(3));
        m.notes.insert(SessionId(3), Note::Restarted);
        assert_eq!(m.pending_count(), 2);
        assert!(m.has_pending_for(50));
        m.disconnect();
        assert_eq!(m.pending_count(), 0);
        assert!(!m.has_pending_for(50) && !m.has_pending_for(61));
        assert!(!m.is_restoring(SessionId(3)));
        assert!(m.notes.is_empty());
        assert!(m.terminals.is_empty());
        // A response from the old connection, arriving late, matches nothing.
        assert_eq!(m.handle_response(1, Response::SessionCreated { id: SessionId(9) }), Outcome::None);
        let r2 = Recorder::new();
        m.set_client_for_test(r2.clone());
        m.list();
        assert_eq!(r2.requests().len(), 1, "requests go to the new transport");
    }

    #[test]
    fn a_request_sent_while_disconnected_matches_nothing_on_the_new_connection() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r.clone(), None, PathBuf::from("/p"));
        m.list();
        m.disconnect();
        // Cmd+T during the outage: sent on the dead connection as id 2.
        m.create(50, false);
        let r2 = Recorder::new();
        m.set_client_for_test(r2.clone());
        // The new connection's own id 2 is a heartbeat, which the daemon answers with Ok.
        assert_eq!(m.handle_response(2, Response::Ok), Outcome::None);
        assert_eq!(m.pending_count(), 0);
    }

    #[test]
    fn a_kill_made_while_disconnected_is_sent_on_the_new_connection() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r.clone(), None, PathBuf::from("/p"));
        m.disconnect();
        m.kill(SessionId(9));
        assert!(r.requests().is_empty(), "nothing goes to the dead connection");
        let r2 = Recorder::new();
        m.set_client_for_test(r2.clone());
        assert!(matches!(r2.requests()[..], [Request::Kill { session: SessionId(9) }]));
        assert_eq!(m.handle_response(1, Response::Ok), Outcome::Killed { session: SessionId(9) });
        let r3 = Recorder::new();
        m.set_client_for_test(r3.clone());
        assert!(r3.requests().is_empty(), "sent once");
    }
}
