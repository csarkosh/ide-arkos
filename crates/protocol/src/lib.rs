//! Messages and framing shared by sarkosd and sarkos.
//!
//! Every request from the UI carries an id; the daemon answers with a `ServerMessage::Response`
//! bearing the same id, and pushes `ServerMessage::Event`s on its own schedule.
pub mod codec;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Bumped on any change to the wire format. A daemon answers a `Hello` with a different version
/// with an error and disconnects, since it cannot decode the rest reliably.
pub const PROTOCOL_VERSION: u32 = 1;

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
