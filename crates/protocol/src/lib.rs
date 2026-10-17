//! Messages and framing shared by arkosd and arkos.
//!
//! Every request from the UI carries an id; the daemon answers with a `ServerMessage::Response`
//! bearing the same id, and pushes `ServerMessage::Event`s on its own schedule.
pub mod codec;
pub mod release_key;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// Bumped only for a breaking change. New enum variants are appended, never inserted, and
/// existing structs never change shape, so a daemon can keep serving older windows: it answers
/// a `Hello` with the range it speaks (spec update-in-place section 5). Version 3 adds the agent
/// record, the detailed list and Goodbye for recovery after a restart. Version 4 adds the update
/// status, check and apply requests and the staged event. Version 5 adds the detailed status
/// (the check's state) and the grace detach. Version 6 adds the customize relay: a tool client's
/// `Customize`, the windows' `Event::Customize` and `CustomizeDone`, and the `Customized` answer.
/// Version 7 adds telemetry: the window's `Session` after its Hello, the `TelemetryStatus`
/// request and answer, and `SessionQuit`.
pub const PROTOCOL_VERSION: u32 = 7;
/// The oldest window a daemon still serves. Version 1 is milestone 2b's.
pub const PROTOCOL_MIN: u32 = 1;
/// The first protocol with `UpdateStatus` and `Event::UpdateStaged`.
pub const UPDATE_STATUS_FROM: u32 = 4;
pub const UPDATE_EVENTS_FROM: u32 = 4;
/// The first protocol with the check's state in the status answer.
pub const UPDATE_STATUS_DETAILED_FROM: u32 = 5;
/// The first protocol where the daemon ends a closed tab's session after its grace.
pub const DETACH_FOR_GRACE_FROM: u32 = 5;
/// The first protocol whose windows answer `Event::Customize`. An older project window is not
/// asked and is listed in `Response::Customized::too_old`.
pub const CUSTOMIZE_FROM: u32 = 6;
/// The first protocol whose windows send `Session` after their Hello. An older window sends
/// none, and the daemon seals nothing for it.
pub const TELEMETRY_FROM: u32 = 7;
/// How long the daemon waits for the windows' answers to a `Customize` before it replies with
/// the silent ones as `missing`.
pub const CUSTOMIZE_WAIT: Duration = Duration::from_secs(2);

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
    /// Recreated by `arkosd --recover` after a restart.
    pub recovered: bool,
    pub agent: Option<AgentRecord>,
}

/// A release the daemon downloaded, verified and unpacked, ready to swap in when a window asks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedUpdate {
    pub version: String,
    pub git: String,
}

/// What a tool client asks every project window to do (agent theme spec, section 5). A window
/// never writes a file for any of these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CustomizeAction {
    /// Show the theme in `toml` until the preview ends; the config is untouched.
    PreviewTheme { toml: String },
    /// End a preview and show the configured theme again.
    EndPreview,
    /// Read the config and the theme file again, as a file change would.
    ReloadConfig,
    /// Change nothing; report what is shown.
    ReportState,
}

/// What a window shows after an action: enough for the tool to check that a change landed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    /// The name of the theme on screen, a preview's included.
    pub theme: String,
    pub dark: bool,
    pub system_is_dark: bool,
    /// The text size preset.
    pub preset: String,
    /// The effective font family.
    pub family: String,
    /// The previewed theme's name while a preview is on.
    pub preview: Option<String>,
    /// The banner text when the config failed to load.
    pub config_error: Option<String>,
    /// The banner text when the theme file failed to load.
    pub theme_error: Option<String>,
}

/// One window's answer to a `Customize`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomizeOutcome {
    /// The window's process id.
    pub pid: u32,
    /// The project folder the window has open.
    pub project: PathBuf,
    /// The window's state after the action, or why the action failed.
    pub result: Result<Report, String>,
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
    /// From the agent's SessionStart hook through `arkos notify`. Replaces any earlier record.
    SetAgent { session: SessionId, agent: AgentRecord },
    /// From the agent's SessionEnd hook. Clears the record only when it is still `id`'s: a
    /// SessionEnd that lands after the next SessionStart must not erase the new record.
    ClearAgent { session: SessionId, id: String },
    /// `ListSessions` with the recovery fields. A version 3 window sends this one.
    ListSessionsDetailed,
    /// What is staged and the last check's error. Answered `UpdateStatus`.
    UpdateStatus,
    /// Check for a release now rather than at the next poll. Answered `Ok` at once; a release
    /// staged by it arrives as `Event::UpdateStaged`.
    CheckForUpdates,
    /// Swap in the staged release `version` and restart onto it.
    ApplyUpdate { version: String },
    /// `UpdateStatus` with the check's state. A protocol 5 window sends this one.
    UpdateStatusDetailed,
    /// The window closed a tab: end `session` unless a window attaches it within `secs`.
    /// Answered `Ok` at once; a second request restarts the grace.
    DetachForGrace { session: SessionId, secs: u32 },
    /// From a tool client (no Hello needed): relay `action` to every project window as
    /// `Event::Customize` and answer `Response::Customized` once all have answered or after
    /// `CUSTOMIZE_WAIT`. `id` is the tool's own and must not be in flight already.
    Customize { id: u64, action: CustomizeAction },
    /// A window's answer to `Event::Customize { id, .. }`. Answered `Ok` whether or not the id is
    /// still waiting: a late answer is never an error.
    CustomizeDone { id: u64, outcome: CustomizeOutcome },
    /// Sent by a protocol 7 window right after every Hello when it keeps a journal: the id of
    /// its usage session, whose journal the daemon seals when the window's connection ends. A
    /// later one replaces it (the journal restarts under a fresh id when usage is turned on).
    /// Answered `Ok`, or `Error` for an id that is not 16 to 32 lowercase hex characters.
    Session { sid: String },
    /// The telemetry mode and the queue as the daemon sees them, for `arkos telemetry status`
    /// and the doctor. Answered `TelemetryStatus`, whatever protocol the client said.
    TelemetryStatus,
    /// The window is quitting, so its usage session ended as `quit` when its connection closes.
    /// Unlike `Goodbye` it changes nothing else: the folder stays among the windows a restart
    /// reopens. Sent by a window, or the launch chooser, that quits without a `Goodbye` (a macOS
    /// logout or restart, after which the window comes back), and harmless beside one. Answered
    /// `Ok`.
    SessionQuit,
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
    /// `unreachable_for_secs`: None until a check first reached a signed manifest; then the
    /// seconds since the last one that did when the latest check failed in transport (offline, a
    /// blocked GitHub), else `Some(0)`. A network that blocks updates shows this way.
    UpdateStatus { staged: Option<StagedUpdate>, last_error: Option<String>, unreachable_for_secs: Option<u64> },
    UpdateStatusDetailed {
        staged: Option<StagedUpdate>,
        last_error: Option<String>,
        unreachable_for_secs: Option<u64>,
        /// A check is running now (asked for, or on the timer).
        checking: bool,
        /// When the last check finished, whatever it found; None before the first.
        last_check_unix: Option<u64>,
        /// The last check's transport failure, None once a check reaches the manifest again.
        transient_error: Option<String>,
    },
    /// The answer to `Customize`: the windows' outcomes in arrival order, the pids of windows
    /// that were asked and did not answer in time, and the pids of project windows on a protocol
    /// below `CUSTOMIZE_FROM`, which were not asked.
    Customized { outcomes: Vec<CustomizeOutcome>, missing: Vec<u32>, too_old: Vec<u32> },
    /// The answer to `TelemetryStatus`.
    TelemetryStatus {
        /// The usage setting in effect: "off", "local" or "on".
        usage: String,
        /// The crash setting in effect: "never", "ask" or "always".
        crashes: String,
        /// Whether the daemon would upload what is queued.
        uploads: bool,
        /// Why the mode is not what the config says (an environment rule, a config error, a
        /// source build), None when it is.
        reason: Option<String>,
        /// The reports waiting in the queue.
        queued: u32,
        /// The journals not sealed yet: sessions running, or left by a window that is gone.
        journals: u32,
        /// The UTC day number of the last first-party update check; None before the first.
        last_check_day: Option<i64>,
        /// The reports the collector accepted, ever.
        sent: u64,
        /// The reports the collector refused, ever.
        rejected: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    Output { session: SessionId, bytes: Vec<u8> },
    Exited { session: SessionId, code: Option<i32> },
    State { session: SessionId, state: ClaudeState },
    /// Sent to every connected client when a release has been staged.
    UpdateStaged { update: StagedUpdate },
    /// A tool client's `Customize`, sent only to project windows on `CUSTOMIZE_FROM` or later.
    /// The window answers with `Request::CustomizeDone` carrying the same `id`.
    Customize { id: u64, action: CustomizeAction },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMessage {
    Response { id: u32, response: Response },
    Event(Event),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_byte<T: serde::Serialize>(v: &T) -> u8 {
        postcard::to_allocvec(v).unwrap()[0]
    }

    /// postcard writes an enum's variant index first; appended variants keep every older index.
    #[test]
    fn new_variants_are_appended_after_the_old_ones() {
        assert_eq!(PROTOCOL_VERSION, 7);
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
        assert_eq!(postcard::to_allocvec(&Request::UpdateStatus).unwrap()[0], 16);
        assert_eq!(postcard::to_allocvec(&Request::CheckForUpdates).unwrap()[0], 17);
        assert_eq!(postcard::to_allocvec(&Request::ApplyUpdate { version: "0.2.1".into() }).unwrap()[0], 18);
        let staged = StagedUpdate { version: "0.2.1".into(), git: "a1b2c3d".into() };
        assert_eq!(postcard::to_allocvec(&Response::UpdateStatus { staged: Some(staged.clone()), last_error: None, unreachable_for_secs: Some(90_000) }).unwrap()[0], 7);
        let status = Response::UpdateStatus { staged: None, last_error: Some("e".into()), unreachable_for_secs: Some(90_000) };
        assert_eq!(postcard::from_bytes::<Response>(&postcard::to_allocvec(&status).unwrap()).unwrap(), status);
        assert_eq!(postcard::to_allocvec(&Event::UpdateStaged { update: staged }).unwrap()[0], 3);
        assert_eq!(first_byte(&Request::UpdateStatusDetailed), 19);
        assert_eq!(first_byte(&Request::DetachForGrace { session: SessionId(1), secs: 60 }), 20);
        let detailed = Response::UpdateStatusDetailed { staged: None, last_error: None, unreachable_for_secs: Some(0), checking: true, last_check_unix: Some(7), transient_error: Some("offline".into()) };
        assert_eq!(first_byte(&detailed), 8);
        let bytes = postcard::to_allocvec(&detailed).unwrap();
        assert_eq!(postcard::from_bytes::<Response>(&bytes).unwrap(), detailed);
        // An old-shape frame still decodes: the detailed variant is new, the old one untouched.
        let old = Response::UpdateStatus { staged: None, last_error: None, unreachable_for_secs: None };
        assert_eq!(postcard::from_bytes::<Response>(&postcard::to_allocvec(&old).unwrap()).unwrap(), old);
        assert_eq!((UPDATE_STATUS_FROM, UPDATE_EVENTS_FROM, UPDATE_STATUS_DETAILED_FROM, DETACH_FOR_GRACE_FROM), (4, 4, 5, 5));
    }

    #[test]
    fn customize_messages_round_trip_through_json_and_sit_at_the_end_of_their_enums() {
        let a = CustomizeAction::PreviewTheme { toml: "name = \"x\"".into() };
        let r = Report { theme: "navy".into(), dark: true, system_is_dark: true, preset: "comfortable".into(), family: "Menlo".into(), preview: None, config_error: None, theme_error: None };
        let o = CustomizeOutcome { pid: 7, project: "/p".into(), result: Ok(r.clone()) };
        for req in [Request::Customize { id: 9, action: a.clone() }, Request::CustomizeDone { id: 9, outcome: o.clone() }] {
            let text = serde_json::to_string(&req).unwrap();
            assert_eq!(serde_json::from_str::<Request>(&text).unwrap(), req);
        }
        let ev = Event::Customize { id: 9, action: CustomizeAction::ReportState };
        assert_eq!(serde_json::from_str::<Event>(&serde_json::to_string(&ev).unwrap()).unwrap(), ev);
        let resp = Response::Customized { outcomes: vec![o.clone()], missing: vec![8], too_old: vec![] };
        assert_eq!(serde_json::from_str::<Response>(&serde_json::to_string(&resp).unwrap()).unwrap(), resp);
        assert_eq!(PROTOCOL_VERSION, 7);
        assert_eq!(CUSTOMIZE_FROM, 6);
        assert_eq!(CUSTOMIZE_WAIT, Duration::from_secs(2));
        assert_eq!(first_byte(&Request::Customize { id: 9, action: a }), 21);
        assert_eq!(first_byte(&Request::CustomizeDone { id: 9, outcome: o }), 22);
        assert_eq!(first_byte(&resp), 9);
        assert_eq!(first_byte(&ev), 4);
        // The codec is postcard: the same messages survive it too.
        let bytes = postcard::to_allocvec(&resp).unwrap();
        assert_eq!(postcard::from_bytes::<Response>(&bytes).unwrap(), resp);
    }

    #[test]
    fn telemetry_messages_round_trip_and_sit_at_the_end_of_their_enums() {
        assert_eq!(PROTOCOL_VERSION, 7);
        assert_eq!(TELEMETRY_FROM, 7);
        let session = Request::Session { sid: "0123456789abcdef0123456789abcdef".into() };
        let status = Response::TelemetryStatus {
            usage: "on".into(),
            crashes: "never".into(),
            uploads: false,
            reason: Some("DO_NOT_TRACK is set".into()),
            queued: 2,
            journals: 1,
            last_check_day: Some(20_377),
            sent: 9,
            rejected: 1,
        };
        assert_eq!(first_byte(&session), 23);
        assert_eq!(first_byte(&Request::TelemetryStatus), 24);
        assert_eq!(first_byte(&Request::SessionQuit), 25);
        assert_eq!(first_byte(&status), 10);
        for req in [session, Request::TelemetryStatus, Request::SessionQuit] {
            let bytes = postcard::to_allocvec(&req).unwrap();
            assert_eq!(postcard::from_bytes::<Request>(&bytes).unwrap(), req);
            assert_eq!(serde_json::from_str::<Request>(&serde_json::to_string(&req).unwrap()).unwrap(), req);
        }
        let bytes = postcard::to_allocvec(&status).unwrap();
        assert_eq!(postcard::from_bytes::<Response>(&bytes).unwrap(), status);
    }

    /// The bytes a protocol 6 window writes for its Hello, spelled out rather than encoded by
    /// today's types: variant 0, the version, the pid as a varint (4242), `None`, then an empty
    /// folder. Protocol 7 leaves `Hello` alone, so they decode and encode exactly as before.
    #[test]
    fn a_protocol_6_hello_still_decodes() {
        let wire = [0u8, 6, 0x92, 0x21, 0, 0];
        let hello = Request::Hello { protocol_version: 6, ui_pid: 4242, ui_exe: None, ui_cwd: PathBuf::new() };
        assert_eq!(postcard::from_bytes::<Request>(&wire).unwrap(), hello);
        assert_eq!(postcard::to_allocvec(&hello).unwrap(), wire);
        let mut wire = vec![0u8, 6, 0x92, 0x21, 1, 8];
        wire.extend_from_slice(b"/a/arkos");
        wire.push(2);
        wire.extend_from_slice(b"/p");
        let hello = Request::Hello { protocol_version: 6, ui_pid: 4242, ui_exe: Some("/a/arkos".into()), ui_cwd: "/p".into() };
        assert_eq!(postcard::from_bytes::<Request>(&wire).unwrap(), hello);
        assert_eq!(postcard::to_allocvec(&hello).unwrap(), wire);
    }
}
