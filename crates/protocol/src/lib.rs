//! Messages and framing shared by sarkosd and sarkos.
//!
//! Every request from the UI carries an id; the daemon answers with a `ServerMessage::Response`
//! bearing the same id, and pushes `ServerMessage::Event`s on its own schedule.
pub mod codec;
pub mod release_key;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Bumped only for a breaking change. New enum variants are appended, never inserted, and
/// existing structs never change shape, so a daemon can keep serving older windows: it answers
/// a `Hello` with the range it speaks (spec update-in-place section 5). Version 3 adds the agent
/// record, the detailed list and Goodbye for recovery after a restart.
pub const PROTOCOL_VERSION: u32 = 3;
/// The oldest window a daemon still serves. Version 1 is milestone 2b's.
pub const PROTOCOL_MIN: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClaudeState {
    Idle,
    Working,
    Waiting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: SessionId,
    pub title: String,
    pub cwd: PathBuf,
    pub pid: Option<u32>,
    pub alive: bool,
    pub last_activity_unix_ms: u64,
    pub state: ClaudeState,
}

/// The agent running inside a session, from its own SessionStart hook: enough to resume it
/// after a restart (update-in-place spec, section 11). The daemon never reads more into a shell
/// than this record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRecord {
    /// "claude" for Claude Code; "codex" in a later milestone.
    pub kind: String,
    /// The agent's own session id.
    pub id: String,
    /// The command that resumes it, as argv.
    pub resume: Vec<String>,
}

/// `SessionInfo` plus what milestone 3 adds. A new struct, because `SessionInfo` never changes
/// shape (section 5 of the update-in-place spec).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionDetail {
    pub info: SessionInfo,
    /// Recreated by `sarkosd --recover` after a restart.
    pub recovered: bool,
    pub agent: Option<AgentRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    /// First message on a connection. `protocol_version` comes first so it decodes even when the
    /// rest has changed; it must equal the daemon's `PROTOCOL_VERSION`. `ui_pid`, `ui_exe` and
    /// `ui_cwd` feed the watchdog (spec 2.4): a relaunched UI starts in `ui_cwd`, so it opens the
    /// same project's workspace. An empty `ui_cwd` means the UI could not read its own and the
    /// relaunch keeps the daemon's.
    Hello { protocol_version: u32, ui_pid: u32, ui_exe: Option<PathBuf>, ui_cwd: PathBuf },
    CreateSession {
        cwd: PathBuf,
        shell: Option<String>,
        env: Vec<(String, String)>,
        nice: i32,
        rows: u16,
        cols: u16,
    },
    Attach { session: SessionId },
    Detach { session: SessionId },
    Input { session: SessionId, bytes: Vec<u8> },
    Resize { session: SessionId, rows: u16, cols: u16 },
    Kill { session: SessionId },
    ListSessions,
    Heartbeat { seq: u64 },
    SetState { session: SessionId, state: ClaudeState },
    /// Replace the daemon with the binary at `exe` by exec-in-place (update-in-place spec,
    /// section 2). Answered `Error` on failure, with the old daemon intact; on success there is
    /// no answer: the connection ends when the exec closes it.
    Upgrade { exe: PathBuf },
    /// When this client disconnects, start `exe` in `cwd` (section 4). Answered `Ok` at once.
    RelaunchClient { exe: PathBuf, cwd: PathBuf },
    /// The window is quitting because the user asked: a restart must not reopen it. A window
    /// that vanishes without this (the shutdown killed it, or it crashed) is reopened.
    Goodbye,
    /// From the agent's SessionStart hook through `sarkos notify`. Replaces any earlier record.
    SetAgent { session: SessionId, agent: AgentRecord },
    /// From the agent's SessionEnd hook. Clears the record only when it is still `id`'s: a
    /// SessionEnd that lands after the next SessionStart must not erase the new record.
    ClearAgent { session: SessionId, id: String },
    /// `ListSessions` with the recovery fields. A version 3 window sends this one.
    ListSessionsDetailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientMessage {
    pub id: u32,
    pub request: Request,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Response {
    Ok,
    SessionCreated { id: SessionId },
    /// The replay buffer, oldest byte first. Live `Event::Output` frames follow.
    Attached { replay: Vec<u8>, alive: bool },
    Sessions(Vec<SessionInfo>),
    Error(String),
    /// The answer to a version 2 `Hello`: what runs and what it speaks.
    Hello { daemon_version: String, protocol_min: u32, protocol_max: u32 },
    SessionsDetailed(Vec<SessionDetail>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    Output { session: SessionId, bytes: Vec<u8> },
    Exited { session: SessionId, code: Option<i32> },
    State { session: SessionId, state: ClaudeState },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMessage {
    Response { id: u32, response: Response },
    Event(Event),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// postcard writes an enum's variant index first; appended variants keep every older index.
    #[test]
    fn new_variants_are_appended_after_the_old_ones() {
        assert_eq!(PROTOCOL_VERSION, 3);
        assert_eq!(PROTOCOL_MIN, 1);
        assert_eq!(postcard::to_allocvec(&Request::RelaunchClient { exe: "/x".into(), cwd: "/y".into() }).unwrap()[0], 11);
        assert_eq!(postcard::to_allocvec(&Request::Goodbye).unwrap()[0], 12);
        let agent = AgentRecord { kind: "claude".into(), id: "abc".into(), resume: vec!["claude".into(), "--resume".into(), "abc".into()] };
        assert_eq!(postcard::to_allocvec(&Request::SetAgent { session: SessionId(1), agent: agent.clone() }).unwrap()[0], 13);
        assert_eq!(postcard::to_allocvec(&Request::ClearAgent { session: SessionId(1), id: "abc".into() }).unwrap()[0], 14);
        assert_eq!(postcard::to_allocvec(&Request::ListSessionsDetailed).unwrap()[0], 15);
        assert_eq!(postcard::to_allocvec(&Response::Hello { daemon_version: String::new(), protocol_min: 1, protocol_max: 3 }).unwrap()[0], 5);
        let detail = SessionDetail {
            info: SessionInfo { id: SessionId(1), title: String::new(), cwd: "/p".into(), pid: None, alive: true, last_activity_unix_ms: 0, state: ClaudeState::Idle },
            recovered: true,
            agent: Some(agent),
        };
        let bytes = postcard::to_allocvec(&Response::SessionsDetailed(vec![detail.clone()])).unwrap();
        assert_eq!(bytes[0], 6);
        assert_eq!(postcard::from_bytes::<Response>(&bytes).unwrap(), Response::SessionsDetailed(vec![detail]));
    }
}
