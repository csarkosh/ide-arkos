# sarkos Milestone 1: Daemon plus one terminal. Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `sarkosd` daemon that owns pseudo-terminals and survives the UI, plus a `sarkos` GPUI window showing one terminal with smooth fractional scrolling, that reattaches to its session with scrollback intact after the UI is killed.

**Architecture:** Two binaries in one Cargo workspace share a `protocol` crate (postcard-encoded, length-prefixed frames over a Unix socket). The daemon uses plain std threads: one reader per PTY, one reader and one writer per client, a watchdog thread. The UI keeps an `alacritty_terminal` grid per terminal in the GPUI process, fed from a socket reader thread through a `futures` channel, and paints visible rows with a custom GPUI `Element` at a fractional line offset driven by a time-based scroll animator.

**Tech Stack:** Rust stable (edition 2021), gpui 0.2.2 (exact pin), alacritty_terminal 0.26, portable-pty 0.9, postcard 1.1 + serde, futures 0.3, libc 0.2, anyhow 1. Dev: tempfile 3.

**Spec:** `docs/superpowers/specs/2026-10-13-sarkos-design.md` (sections 2, 3, 7.1, 9, 10, 11 milestone 1, 12).

## Global Constraints

- macOS only, Apple Silicon. No Linux or Windows code paths.
- `gpui = "=0.2.2"` pinned exactly (spec 1.3: pre-1.0 API churn). Never bump inside this plan.
- Socket path: `~/Library/Application Support/sarkos/sarkosd.sock` (spec 2.1). Workspace files: `~/Library/Application Support/sarkos/workspaces/<hash-of-project-path>.json` (spec 4).
- Env var the daemon sets in every shell: `SARKOS_SESSION_ID` (spec 2.3).
- Replay buffer default 16 MB per session (spec 2.1). Scrollback default 20,000 lines (spec 3).
- Watchdog: heartbeat every 1 s from the UI main thread; daemon relaunches the UI after 30 s without one while the process is alive (spec 2.4).
- Text defaults: Menlo at 16 px, line height 1.5 (spec 3, 7.1). Background `#11161f`, foreground `#d6dae0` (spec 7.1).
- The UI main thread runs at user-interactive QoS (spec 2.5).
- Never block the UI thread: socket IO on threads, bounded or coalesced handoff (spec 9).
- Memory budgets measured later (milestone 6) but nothing in this milestone may allocate per output byte beyond the ring buffer in the daemon (spec 2.1).
- Commit messages: imperative, one line, body optional. No em-dashes in any copy or comment.
- **If a gpui or alacritty_terminal name in this plan does not compile**, the API moved. Do not guess: grep the vendored source at `~/.cargo/registry/src/*/gpui-0.2.2/src/` or `~/.cargo/registry/src/*/alacritty_terminal-0.26.*/src/` for the method, fix the call, and note the change in the commit message.

## Review Focus

1. **A session's shell exits while the UI is detached.** A reattach must still succeed, replay the buffer, and show the tab as dead rather than erroring. Pinned by Task 3's `reattach_after_shell_exit` test.
2. **Output arrives faster than the UI paints.** Thousands of frames per second from `yes` must coalesce into at most one repaint per frame and never grow a queue. Pinned by Task 9's `apply_output_coalesces_wakeups` test and Task 10's manual flood check.
3. **The replay buffer starts mid escape sequence.** Reattaching after more than 16 MB of output must not corrupt the grid beyond the first few bytes. Pinned by Task 3's `replay_truncated_to_capacity` test (ring semantics) and Task 9's `replay_from_mid_sequence_recovers` test.
4. **A stale socket file from a crashed daemon.** The UI must detect the dead socket, remove it, start a new daemon, and connect. Pinned by Task 5's `connect_replaces_stale_socket` test.
5. **Keystrokes with Option and Shift on a US layout.** Option-s must send "ß", Shift-Tab must send the back-tab sequence, and Ctrl-C must send 0x03, not "c". Pinned by Task 6's key mapping tests.

---

## File structure

```
ide-sarkos/
  Cargo.toml                       workspace: members = ["crates/*"], resolver = "2"
  rust-toolchain.toml              stable
  .gitignore                       target/
  crates/
    protocol/
      Cargo.toml
      src/lib.rs                   SessionId, ClaudeState, SessionInfo, Request, Response, Event, ClientMessage, ServerMessage
      src/codec.rs                 write_frame / read_frame (u32 BE length + postcard)
    sarkosd/
      Cargo.toml
      src/main.rs                  CLI flags, paths, start server
      src/ring.rs                  RingBuffer: bounded byte ring (replay buffer)
      src/session.rs               Session: PTY, child, replay ring, subscribers, reader thread
      src/server.rs                Unix socket accept loop, per-client reader/writer threads, dispatch
      src/watchdog.rs              heartbeat tracking, kill and relaunch
      tests/daemon.rs              integration tests against the real binary
    sarkos/
      Cargo.toml
      src/main.rs                  QoS, daemon autostart, GPUI Application, window, Root view
      src/paths.rs                 socket and workspace paths (shared logic with the daemon kept tiny by duplication)
      src/client.rs                socket client: writer thread, reader thread -> futures channel
      src/keys.rs                  Keystroke -> bytes
      src/scroll.rs                ScrollAnimator (pure)
      src/palette.rs               Navy ANSI palette, 256-colour index to Rgb
      src/layout.rs                CellStyle, Run, build_runs (pure grid -> glyph runs)
      src/terminal.rs              Terminal entity: Term + Processor + events + resize + input
      src/terminal_element.rs      TerminalElement: GPUI Element painting rows at a fractional offset
      src/workspace.rs             load/save the session id for the current folder
```

Responsibilities are one per file. `protocol` has no IO beyond the codec. `sarkosd` never parses escape sequences. `sarkos` never spawns a PTY.

---

### Task 1: Toolchain and workspace scaffold

**Files:**
- Create: `rust-toolchain.toml`, `Cargo.toml`, `.gitignore`, `crates/protocol/Cargo.toml`, `crates/protocol/src/lib.rs`, `crates/sarkosd/Cargo.toml`, `crates/sarkosd/src/main.rs`, `crates/sarkos/Cargo.toml`, `crates/sarkos/src/main.rs`

**Interfaces:**
- Produces: a workspace where `cargo build` succeeds and `cargo run -p sarkosd -- --version` prints `sarkosd 0.1.0`.

- [ ] **Step 1: Install the Rust toolchain if `cargo` is missing**

Run: `which cargo || (curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal && source "$HOME/.cargo/env")`
Then: `cargo --version && rustc --version`
Expected: both print a version 1.85 or newer (gpui 0.2.2 uses async closures).

- [ ] **Step 2: Write the workspace files**

`rust-toolchain.toml`:
```toml
[toolchain]
channel = "stable"
```

`Cargo.toml`:
```toml
[workspace]
members = ["crates/*"]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "GPL-3.0-only"

[workspace.dependencies]
protocol = { path = "crates/protocol" }
serde = { version = "1", features = ["derive"] }
postcard = { version = "1.1", features = ["alloc"] }
anyhow = "1"
libc = "0.2"
tempfile = "3"

[profile.release]
debug = 1
```

`.gitignore`:
```
target/
```

`crates/protocol/Cargo.toml`:
```toml
[package]
name = "protocol"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
serde.workspace = true
postcard.workspace = true
```

`crates/protocol/src/lib.rs`:
```rust
//! Messages and framing shared by sarkosd and sarkos.
pub mod codec;
```

`crates/protocol/src/codec.rs`:
```rust
//! Filled in by Task 2.
```

`crates/sarkosd/Cargo.toml`:
```toml
[package]
name = "sarkosd"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
protocol.workspace = true
serde.workspace = true
postcard.workspace = true
anyhow.workspace = true
libc.workspace = true
portable-pty = "0.9"

[dev-dependencies]
tempfile.workspace = true
```

`crates/sarkosd/src/main.rs`:
```rust
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") {
        println!("sarkosd {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    eprintln!("sarkosd: not implemented yet");
    std::process::exit(2);
}
```

`crates/sarkos/Cargo.toml`:
```toml
[package]
name = "sarkos"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
protocol.workspace = true
serde.workspace = true
postcard.workspace = true
anyhow.workspace = true
libc.workspace = true
gpui = "=0.2.2"
alacritty_terminal = "0.26"
futures = "0.3"
serde_json = "1"

[dev-dependencies]
tempfile.workspace = true
```

`crates/sarkos/src/main.rs`:
```rust
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") {
        println!("sarkos {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    eprintln!("sarkos: not implemented yet");
    std::process::exit(2);
}
```

- [ ] **Step 3: Build**

Run: `cargo build 2>&1 | tail -5`
Expected: `Finished` with no errors. The first build compiles gpui and takes several minutes.

- [ ] **Step 4: Check the version flags**

Run: `cargo run -q -p sarkosd -- --version && cargo run -q -p sarkos -- --version`
Expected:
```
sarkosd 0.1.0
sarkos 0.1.0
```

- [ ] **Step 5: Commit**

```bash
git add rust-toolchain.toml Cargo.toml Cargo.lock .gitignore crates
git commit -m "Scaffold the workspace: protocol, sarkosd, sarkos"
```

---

### Task 2: Protocol types and framing

**Files:**
- Modify: `crates/protocol/src/lib.rs`
- Create: `crates/protocol/src/codec.rs` (replace the stub)
- Test: inline `#[cfg(test)]` in `codec.rs`

**Interfaces:**
- Produces (used by every later task):
  - `SessionId(pub u64)`; `ClaudeState { Idle, Working, Waiting }`
  - `SessionInfo { id, title: String, cwd: PathBuf, pid: Option<u32>, alive: bool, last_activity_unix_ms: u64, state: ClaudeState }`
  - `Request` enum, `ClientMessage { id: u32, request: Request }`
  - `Response` enum, `Event` enum, `ServerMessage { Response { id, response } | Event(Event) }`
  - `codec::write_frame<W: Write, T: Serialize>(w: &mut W, msg: &T) -> io::Result<()>`
  - `codec::read_frame<R: Read, T: DeserializeOwned>(r: &mut R) -> io::Result<Option<T>>` (`Ok(None)` on clean EOF)
  - `codec::MAX_FRAME: u32 = 64 * 1024 * 1024`

- [ ] **Step 1: Write the failing codec tests**

Append to `crates/protocol/src/codec.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClientMessage, Request, Response, ServerMessage, SessionId};
    use std::io::Cursor;

    #[test]
    fn round_trips_two_frames() {
        let mut buf = Vec::new();
        let a = ClientMessage { id: 1, request: Request::Heartbeat { seq: 7 } };
        let b = ClientMessage { id: 2, request: Request::Input { session: SessionId(3), bytes: b"ls\r".to_vec() } };
        write_frame(&mut buf, &a).unwrap();
        write_frame(&mut buf, &b).unwrap();
        let mut r = Cursor::new(buf);
        let a2: ClientMessage = read_frame(&mut r).unwrap().unwrap();
        let b2: ClientMessage = read_frame(&mut r).unwrap().unwrap();
        assert_eq!(a2, a);
        assert_eq!(b2, b);
        let eof: Option<ClientMessage> = read_frame(&mut r).unwrap();
        assert!(eof.is_none());
    }

    #[test]
    fn server_message_round_trip() {
        let mut buf = Vec::new();
        let m = ServerMessage::Response { id: 9, response: Response::Attached { replay: vec![1, 2, 3] } };
        write_frame(&mut buf, &m).unwrap();
        let m2: ServerMessage = read_frame(&mut Cursor::new(buf)).unwrap().unwrap();
        assert_eq!(m2, m);
    }

    #[test]
    fn rejects_oversized_frame() {
        let mut buf = (MAX_FRAME + 1).to_be_bytes().to_vec();
        buf.extend_from_slice(&[0u8; 8]);
        let err = read_frame::<_, ClientMessage>(&mut Cursor::new(buf)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn truncated_frame_is_an_error_not_eof() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &ClientMessage { id: 1, request: Request::ListSessions }).unwrap();
        buf.truncate(buf.len() - 1);
        let err = read_frame::<_, ClientMessage>(&mut Cursor::new(buf)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p protocol 2>&1 | tail -5`
Expected: compile error, `write_frame` and the message types are not defined.

- [ ] **Step 3: Write the types**

Replace `crates/protocol/src/lib.rs`:
```rust
//! Messages and framing shared by sarkosd and sarkos.
//!
//! Every request from the UI carries an id; the daemon answers with a `ServerMessage::Response`
//! bearing the same id, and pushes `ServerMessage::Event`s on its own schedule.
pub mod codec;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    /// First message on a connection. `ui_pid` and `ui_exe` feed the watchdog (spec 2.4).
    Hello { ui_pid: u32, ui_exe: Option<PathBuf> },
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
```

- [ ] **Step 4: Write the codec**

Replace the top of `crates/protocol/src/codec.rs` (keep the tests module below it):
```rust
//! Length-prefixed frames: a big-endian u32 byte length, then a postcard-encoded message.
use serde::{de::DeserializeOwned, Serialize};
use std::io::{self, Read, Write};

/// Largest frame accepted. A 16 MB replay plus headroom fits; anything larger is a bug.
pub const MAX_FRAME: u32 = 64 * 1024 * 1024;

pub fn write_frame<W: Write, T: Serialize>(w: &mut W, msg: &T) -> io::Result<()> {
    let body = postcard::to_allocvec(msg).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let len = u32::try_from(body.len()).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "frame too large"))?;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    w.write_all(&len.to_be_bytes())?;
    w.write_all(&body)?;
    w.flush()
}

/// Reads one frame. Returns `Ok(None)` only when the stream ends exactly on a frame boundary.
pub fn read_frame<R: Read, T: DeserializeOwned>(r: &mut R) -> io::Result<Option<T>> {
    let mut len_buf = [0u8; 4];
    match r.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_be_bytes(len_buf);
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body)?;
    postcard::from_bytes(&body)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p protocol 2>&1 | tail -8`
Expected: `test result: ok. 4 passed`

- [ ] **Step 6: Commit**

```bash
git add crates/protocol
git commit -m "protocol: message types and length-prefixed postcard framing"
```

---

### Task 3: Daemon core: ring buffer, sessions, server, integration test

This task is larger than the others because the daemon only becomes testable once a socket, a session and the dispatch exist together. It is still one deliverable: a daemon binary a test can talk to.

**Files:**
- Create: `crates/sarkosd/src/ring.rs`, `crates/sarkosd/src/session.rs`, `crates/sarkosd/src/server.rs`
- Modify: `crates/sarkosd/src/main.rs`
- Test: `crates/sarkosd/src/ring.rs` (unit), `crates/sarkosd/tests/daemon.rs` (integration)

**Interfaces:**
- Consumes: everything in `protocol`.
- Produces:
  - `ring::RingBuffer::new(capacity: usize)`, `.push(&[u8])`, `.snapshot() -> Vec<u8>`, `.len()`
  - `session::Session::spawn(SpawnSpec) -> anyhow::Result<Arc<Session>>` with `SpawnSpec { id, cwd, shell: Option<String>, env, nice, rows, cols, replay_capacity }`
  - `session::Session::{write(&[u8]), resize(rows, cols), kill(), info() -> SessionInfo, subscribe(ClientId, Sender<ServerMessage>) -> Vec<u8> (replay), unsubscribe(ClientId)}`
  - `server::run(socket: &Path, opts: Options) -> anyhow::Result<()>` with `Options { replay_capacity: usize, watchdog: Duration }`
  - CLI: `sarkosd --socket <path> --replay-bytes <n> --watchdog-secs <n>`; defaults are the spec's. `--watchdog-secs 0` disables the watchdog (Task 4 implements it; this task only stores the value).

- [ ] **Step 1: Write the failing ring buffer tests**

Create `crates/sarkosd/src/ring.rs`:
```rust
//! A bounded byte ring: the replay buffer for one session (spec 2.1). Pushing past capacity drops
//! the oldest bytes. `snapshot` returns the bytes oldest first.

pub struct RingBuffer {
    buf: Vec<u8>,
    cap: usize,
    start: usize,
    len: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_everything_under_capacity() {
        let mut r = RingBuffer::new(8);
        r.push(b"abc");
        r.push(b"de");
        assert_eq!(r.snapshot(), b"abcde");
        assert_eq!(r.len(), 5);
    }

    #[test]
    fn drops_oldest_when_full() {
        let mut r = RingBuffer::new(5);
        r.push(b"abcdef");
        assert_eq!(r.snapshot(), b"bcdef");
        r.push(b"gh");
        assert_eq!(r.snapshot(), b"defgh");
    }

    #[test]
    fn push_larger_than_capacity_keeps_the_tail() {
        let mut r = RingBuffer::new(3);
        r.push(b"0123456789");
        assert_eq!(r.snapshot(), b"789");
        assert_eq!(r.len(), 3);
    }

    #[test]
    fn wraps_correctly_many_times() {
        let mut r = RingBuffer::new(4);
        for i in 0..100u8 {
            r.push(&[i]);
        }
        assert_eq!(r.snapshot(), vec![96, 97, 98, 99]);
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Add `mod ring;` to `crates/sarkosd/src/main.rs` (above `fn main`). Run: `cargo test -p sarkosd ring 2>&1 | tail -5`
Expected: compile error, `RingBuffer::new` not found.

- [ ] **Step 3: Implement the ring**

Insert between the struct and the tests in `ring.rs`:
```rust
impl RingBuffer {
    pub fn new(cap: usize) -> Self {
        assert!(cap > 0, "replay capacity must be positive");
        Self { buf: vec![0; cap], cap, start: 0, len: 0 }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn push(&mut self, bytes: &[u8]) {
        let bytes = if bytes.len() > self.cap { &bytes[bytes.len() - self.cap..] } else { bytes };
        for &b in bytes {
            let end = (self.start + self.len) % self.cap;
            self.buf[end] = b;
            if self.len == self.cap {
                self.start = (self.start + 1) % self.cap;
            } else {
                self.len += 1;
            }
        }
    }

    pub fn snapshot(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.len);
        for i in 0..self.len {
            out.push(self.buf[(self.start + i) % self.cap]);
        }
        out
    }
}
```

- [ ] **Step 4: Run the ring tests**

Run: `cargo test -p sarkosd ring 2>&1 | tail -5`
Expected: `4 passed`.

- [ ] **Step 5: Write the failing integration test**

Create `crates/sarkosd/tests/daemon.rs`:
```rust
//! Drives the real sarkosd binary over a temp socket.
use protocol::codec::{read_frame, write_frame};
use protocol::{ClientMessage, Event, Request, Response, ServerMessage, SessionId};
use std::io::{BufReader, BufWriter};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Daemon {
    child: Child,
    socket: PathBuf,
    _dir: tempfile::TempDir,
}

impl Daemon {
    fn start(extra: &[&str]) -> Daemon {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("d.sock");
        let child = Command::new(env!("CARGO_BIN_EXE_sarkosd"))
            .arg("--socket")
            .arg(&socket)
            .args(extra)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !socket.exists() {
            assert!(Instant::now() < deadline, "daemon did not create its socket");
            std::thread::sleep(Duration::from_millis(20));
        }
        Daemon { child, socket, _dir: dir }
    }

    fn connect(&self) -> Conn {
        let stream = UnixStream::connect(&self.socket).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut c = Conn { r: BufReader::new(stream.try_clone().unwrap()), w: BufWriter::new(stream), next: 1 };
        let r = c.request(Request::Hello { ui_pid: std::process::id(), ui_exe: None });
        assert_eq!(r, Response::Ok);
        c
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Conn {
    r: BufReader<UnixStream>,
    w: BufWriter<UnixStream>,
    next: u32,
}

impl Conn {
    fn send(&mut self, request: Request) -> u32 {
        let id = self.next;
        self.next += 1;
        write_frame(&mut self.w, &ClientMessage { id, request }).unwrap();
        id
    }

    fn next_message(&mut self) -> ServerMessage {
        read_frame(&mut self.r).unwrap().expect("daemon closed the connection")
    }

    /// Sends a request and returns its response, keeping any events that arrive first.
    fn request(&mut self, request: Request) -> Response {
        let id = self.send(request);
        loop {
            match self.next_message() {
                ServerMessage::Response { id: rid, response } if rid == id => return response,
                ServerMessage::Response { .. } => panic!("response for a different request"),
                ServerMessage::Event(_) => continue,
            }
        }
    }

    fn create(&mut self, shell: &str) -> SessionId {
        match self.request(Request::CreateSession {
            cwd: std::env::temp_dir(),
            shell: Some(shell.to_string()),
            env: vec![],
            nice: 0,
            rows: 24,
            cols: 80,
        }) {
            Response::SessionCreated { id } => id,
            other => panic!("unexpected {other:?}"),
        }
    }

    /// Collects Output events for `session` until `needle` appears or the deadline passes.
    fn wait_for_output(&mut self, session: SessionId, needle: &str) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut got = Vec::new();
        while Instant::now() < deadline {
            if let ServerMessage::Event(Event::Output { session: s, bytes }) = self.next_message() {
                if s == session {
                    got.extend_from_slice(&bytes);
                    if String::from_utf8_lossy(&got).contains(needle) {
                        return got;
                    }
                }
            }
        }
        panic!("never saw {needle:?} in {:?}", String::from_utf8_lossy(&got));
    }
}

#[test]
fn create_attach_echo_detach_reattach_replays() {
    let d = Daemon::start(&[]);
    let mut c = d.connect();
    let id = c.create("/bin/sh");
    let attached = c.request(Request::Attach { session: id });
    assert!(matches!(attached, Response::Attached { alive: true, .. }));
    c.send(Request::Input { session: id, bytes: b"echo hello-sarkos\n".to_vec() });
    c.wait_for_output(id, "hello-sarkos");
    assert_eq!(c.request(Request::Detach { session: id }), Response::Ok);
    drop(c);

    let mut c2 = d.connect();
    match c2.request(Request::Attach { session: id }) {
        Response::Attached { replay, alive } => {
            assert!(alive);
            assert!(String::from_utf8_lossy(&replay).contains("hello-sarkos"), "replay was {:?}", String::from_utf8_lossy(&replay));
        }
        other => panic!("unexpected {other:?}"),
    }
    // Still live after reattach.
    c2.send(Request::Input { session: id, bytes: b"echo second-line\n".to_vec() });
    c2.wait_for_output(id, "second-line");
}

#[test]
fn list_sessions_reports_the_session() {
    let d = Daemon::start(&[]);
    let mut c = d.connect();
    let id = c.create("/bin/sh");
    match c.request(Request::ListSessions) {
        Response::Sessions(list) => {
            let s = list.iter().find(|s| s.id == id).expect("session listed");
            assert!(s.alive);
            assert!(s.pid.is_some());
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn kill_emits_exited_and_marks_dead() {
    let d = Daemon::start(&[]);
    let mut c = d.connect();
    let id = c.create("/bin/sh");
    c.request(Request::Attach { session: id });
    assert_eq!(c.request(Request::Kill { session: id }), Response::Ok);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline);
        if let ServerMessage::Event(Event::Exited { session, .. }) = c.next_message() {
            assert_eq!(session, id);
            break;
        }
    }
    match c.request(Request::ListSessions) {
        Response::Sessions(list) => assert!(!list.iter().find(|s| s.id == id).unwrap().alive),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn reattach_after_shell_exit() {
    let d = Daemon::start(&[]);
    let mut c = d.connect();
    let id = c.create("/bin/sh");
    c.request(Request::Attach { session: id });
    c.send(Request::Input { session: id, bytes: b"echo before-exit; exit\n".to_vec() });
    c.wait_for_output(id, "before-exit");
    drop(c);
    std::thread::sleep(Duration::from_millis(300));
    let mut c2 = d.connect();
    match c2.request(Request::Attach { session: id }) {
        Response::Attached { replay, alive } => {
            assert!(!alive, "shell should have exited");
            assert!(String::from_utf8_lossy(&replay).contains("before-exit"));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn replay_truncated_to_capacity() {
    let d = Daemon::start(&["--replay-bytes", "64"]);
    let mut c = d.connect();
    let id = c.create("/bin/sh");
    c.request(Request::Attach { session: id });
    c.send(Request::Input { session: id, bytes: b"printf 'abcdefghij%.0s' 1 2 3 4 5 6 7 8 9 10; echo END\n".to_vec() });
    c.wait_for_output(id, "END");
    c.request(Request::Detach { session: id });
    drop(c);
    let mut c2 = d.connect();
    match c2.request(Request::Attach { session: id }) {
        Response::Attached { replay, .. } => {
            assert!(replay.len() <= 64);
            assert!(String::from_utf8_lossy(&replay).contains("END"));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn malformed_frame_disconnects_only_that_client() {
    let d = Daemon::start(&[]);
    let mut good = d.connect();
    let id = good.create("/bin/sh");
    let bad = UnixStream::connect(&d.socket).unwrap();
    use std::io::Write;
    (&bad).write_all(&[0, 0, 0, 3, 0xff, 0xff, 0xff]).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    // The good client still works.
    match good.request(Request::ListSessions) {
        Response::Sessions(list) => assert!(list.iter().any(|s| s.id == id)),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn session_env_carries_its_id() {
    let d = Daemon::start(&[]);
    let mut c = d.connect();
    let id = c.create("/bin/sh");
    c.request(Request::Attach { session: id });
    c.send(Request::Input { session: id, bytes: b"echo id=$SARKOS_SESSION_ID\n".to_vec() });
    c.wait_for_output(id, &format!("id={}", id.0));
}
```

- [ ] **Step 6: Run to verify it fails**

Run: `cargo test -p sarkosd --test daemon 2>&1 | tail -5`
Expected: the tests compile, then every test fails with "daemon did not create its socket" (the binary exits with "not implemented").

- [ ] **Step 7: Write the session module**

Create `crates/sarkosd/src/session.rs`:
```rust
//! One pseudo-terminal, its shell, its replay buffer and its subscribers.
use crate::ring::RingBuffer;
use anyhow::{Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use protocol::{ClaudeState, Event, ServerMessage, SessionId, SessionInfo};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub type ClientId = u64;

pub struct SpawnSpec {
    pub id: SessionId,
    pub cwd: PathBuf,
    pub shell: Option<String>,
    pub env: Vec<(String, String)>,
    pub nice: i32,
    pub rows: u16,
    pub cols: u16,
    pub replay_capacity: usize,
}

pub struct Session {
    pub id: SessionId,
    cwd: PathBuf,
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    pid: Option<u32>,
    state: Mutex<SessionState>,
}

struct SessionState {
    replay: RingBuffer,
    subscribers: HashMap<ClientId, Sender<ServerMessage>>,
    alive: bool,
    exit_code: Option<i32>,
    last_activity_unix_ms: u64,
    claude: ClaudeState,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl Session {
    pub fn spawn(spec: SpawnSpec) -> Result<Arc<Session>> {
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize { rows: spec.rows, cols: spec.cols, pixel_width: 0, pixel_height: 0 })
            .context("openpty")?;
        let shell = spec
            .shell
            .clone()
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| "/bin/zsh".to_string());
        let mut cmd = CommandBuilder::new(&shell);
        if shell.ends_with("zsh") || shell.ends_with("bash") {
            cmd.arg("-l");
        }
        cmd.cwd(&spec.cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("SARKOS_SESSION_ID", spec.id.0.to_string());
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        let child = pair.slave.spawn_command(cmd).context("spawn shell")?;
        drop(pair.slave);
        let pid = child.process_id();
        if spec.nice != 0 {
            if let Some(pid) = pid {
                // Children of the shell inherit the priority (spec 2.5).
                unsafe {
                    libc::setpriority(libc::PRIO_PROCESS, pid, spec.nice);
                }
            }
        }
        let reader = pair.master.try_clone_reader().context("clone reader")?;
        let writer = pair.master.take_writer().context("take writer")?;
        let session = Arc::new(Session {
            id: spec.id,
            cwd: spec.cwd,
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            pid,
            state: Mutex::new(SessionState {
                replay: RingBuffer::new(spec.replay_capacity),
                subscribers: HashMap::new(),
                alive: true,
                exit_code: None,
                last_activity_unix_ms: now_ms(),
                claude: ClaudeState::Idle,
            }),
        });
        Self::start_reader(session.clone(), reader);
        Ok(session)
    }

    /// Reads PTY output forever. On EOF the shell has exited: record the exit and tell subscribers.
    fn start_reader(session: Arc<Session>, mut reader: Box<dyn Read + Send>) {
        std::thread::Builder::new()
            .name(format!("pty-reader-{}", session.id.0))
            .spawn(move || {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => session.on_output(&buf[..n]),
                    }
                }
                let code = session.child.lock().unwrap().wait().ok().map(|s| s.exit_code() as i32);
                let mut st = session.state.lock().unwrap();
                st.alive = false;
                st.exit_code = code;
                let event = ServerMessage::Event(Event::Exited { session: session.id, code });
                st.subscribers.retain(|_, tx| tx.send(event.clone()).is_ok());
            })
            .expect("spawn pty reader thread");
    }

    fn on_output(&self, bytes: &[u8]) {
        let mut st = self.state.lock().unwrap();
        st.replay.push(bytes);
        st.last_activity_unix_ms = now_ms();
        let event = ServerMessage::Event(Event::Output { session: self.id, bytes: bytes.to_vec() });
        st.subscribers.retain(|_, tx| tx.send(event.clone()).is_ok());
    }

    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        let mut w = self.writer.lock().unwrap();
        w.write_all(bytes)?;
        w.flush()?;
        Ok(())
    }

    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        self.master
            .lock()
            .unwrap()
            .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .context("resize pty")
    }

    pub fn kill(&self) -> Result<()> {
        self.child.lock().unwrap().kill().context("kill child")
    }

    pub fn set_state(&self, state: ClaudeState) {
        let mut st = self.state.lock().unwrap();
        st.claude = state;
        let event = ServerMessage::Event(Event::State { session: self.id, state });
        st.subscribers.retain(|_, tx| tx.send(event.clone()).is_ok());
    }

    /// Registers a subscriber and returns the replay buffer plus liveness, atomically with respect
    /// to new output, so nothing is missed or duplicated between the replay and the live stream.
    pub fn subscribe(&self, client: ClientId, tx: Sender<ServerMessage>) -> (Vec<u8>, bool) {
        let mut st = self.state.lock().unwrap();
        let replay = st.replay.snapshot();
        let alive = st.alive;
        st.subscribers.insert(client, tx);
        (replay, alive)
    }

    pub fn unsubscribe(&self, client: ClientId) {
        self.state.lock().unwrap().subscribers.remove(&client);
    }

    pub fn info(&self) -> SessionInfo {
        let st = self.state.lock().unwrap();
        SessionInfo {
            id: self.id,
            title: String::new(),
            cwd: self.cwd.clone(),
            pid: self.pid,
            alive: st.alive,
            last_activity_unix_ms: st.last_activity_unix_ms,
            state: st.claude,
        }
    }
}
```

- [ ] **Step 8: Write the server module**

Create `crates/sarkosd/src/server.rs`:
```rust
//! Accepts clients on the Unix socket, one reader and one writer thread each, and dispatches
//! requests to sessions. Shared state is one mutex; every operation under it is short.
use crate::session::{ClientId, Session, SpawnSpec};
use anyhow::{Context, Result};
use protocol::codec::{read_frame, write_frame};
use protocol::{ClientMessage, Request, Response, ServerMessage, SessionId};
use std::collections::HashMap;
use std::io::{BufReader, BufWriter};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub struct Options {
    pub replay_capacity: usize,
    pub watchdog: Duration,
}

pub struct ClientRecord {
    pub tx: Sender<ServerMessage>,
    pub attached: Vec<SessionId>,
    pub last_heartbeat: Instant,
    pub ui_pid: Option<u32>,
    pub ui_exe: Option<PathBuf>,
    pub connected: bool,
}

#[derive(Default)]
pub struct State {
    pub sessions: HashMap<SessionId, Arc<Session>>,
    pub clients: HashMap<ClientId, ClientRecord>,
    next_session: u64,
    next_client: u64,
}

pub type Shared = Arc<Mutex<State>>;

pub fn run(socket: &Path, opts: Options) -> Result<()> {
    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    if socket.exists() {
        // A previous daemon died. Nobody can be listening if connect fails.
        if UnixStream::connect(socket).is_ok() {
            anyhow::bail!("another sarkosd is already listening on {}", socket.display());
        }
        std::fs::remove_file(socket).ok();
    }
    let listener = UnixListener::bind(socket).with_context(|| format!("bind {}", socket.display()))?;
    let state: Shared = Arc::new(Mutex::new(State::default()));
    crate::watchdog::start(state.clone(), opts.watchdog);
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => spawn_client(stream, state.clone(), opts),
            Err(e) => eprintln!("sarkosd: accept failed: {e}"),
        }
    }
    Ok(())
}

fn spawn_client(stream: UnixStream, state: Shared, opts: Options) {
    let (tx, rx) = channel::<ServerMessage>();
    let client_id = {
        let mut st = state.lock().unwrap();
        st.next_client += 1;
        let id = st.next_client;
        st.clients.insert(
            id,
            ClientRecord { tx: tx.clone(), attached: Vec::new(), last_heartbeat: Instant::now(), ui_pid: None, ui_exe: None, connected: true },
        );
        id
    };
    let writer_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("sarkosd: clone stream: {e}");
            return;
        }
    };
    std::thread::Builder::new()
        .name(format!("client-writer-{client_id}"))
        .spawn(move || {
            let mut w = BufWriter::new(writer_stream);
            while let Ok(msg) = rx.recv() {
                if write_frame(&mut w, &msg).is_err() {
                    break;
                }
            }
        })
        .expect("spawn writer");
    std::thread::Builder::new()
        .name(format!("client-reader-{client_id}"))
        .spawn(move || {
            let mut r = BufReader::new(stream);
            loop {
                match read_frame::<_, ClientMessage>(&mut r) {
                    Ok(Some(msg)) => {
                        let response = dispatch(&state, client_id, &tx, msg.request, opts);
                        if tx.send(ServerMessage::Response { id: msg.id, response }).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        eprintln!("sarkosd: client {client_id}: bad frame: {e}");
                        break;
                    }
                }
            }
            disconnect(&state, client_id);
        })
        .expect("spawn reader");
}

fn disconnect(state: &Shared, client_id: ClientId) {
    let mut st = state.lock().unwrap();
    if let Some(rec) = st.clients.get_mut(&client_id) {
        rec.connected = false;
        let attached = std::mem::take(&mut rec.attached);
        for sid in attached {
            if let Some(s) = st.sessions.get(&sid) {
                s.unsubscribe(client_id);
            }
        }
    }
    st.clients.remove(&client_id);
}

fn dispatch(state: &Shared, client_id: ClientId, tx: &Sender<ServerMessage>, request: Request, opts: Options) -> Response {
    let session = |sid: SessionId| -> Option<Arc<Session>> { state.lock().unwrap().sessions.get(&sid).cloned() };
    match request {
        Request::Hello { ui_pid, ui_exe } => {
            let mut st = state.lock().unwrap();
            if let Some(rec) = st.clients.get_mut(&client_id) {
                rec.ui_pid = Some(ui_pid);
                rec.ui_exe = ui_exe;
                rec.last_heartbeat = Instant::now();
            }
            Response::Ok
        }
        Request::Heartbeat { .. } => {
            if let Some(rec) = state.lock().unwrap().clients.get_mut(&client_id) {
                rec.last_heartbeat = Instant::now();
            }
            Response::Ok
        }
        Request::CreateSession { cwd, shell, env, nice, rows, cols } => {
            let id = {
                let mut st = state.lock().unwrap();
                st.next_session += 1;
                SessionId(st.next_session)
            };
            match Session::spawn(SpawnSpec { id, cwd, shell, env, nice, rows, cols, replay_capacity: opts.replay_capacity }) {
                Ok(s) => {
                    state.lock().unwrap().sessions.insert(id, s);
                    Response::SessionCreated { id }
                }
                Err(e) => Response::Error(format!("spawn failed: {e:#}")),
            }
        }
        Request::Attach { session: sid } => match session(sid) {
            Some(s) => {
                let (replay, alive) = s.subscribe(client_id, tx.clone());
                if let Some(rec) = state.lock().unwrap().clients.get_mut(&client_id) {
                    if !rec.attached.contains(&sid) {
                        rec.attached.push(sid);
                    }
                }
                Response::Attached { replay, alive }
            }
            None => Response::Error(format!("no session {}", sid.0)),
        },
        Request::Detach { session: sid } => {
            if let Some(s) = session(sid) {
                s.unsubscribe(client_id);
            }
            if let Some(rec) = state.lock().unwrap().clients.get_mut(&client_id) {
                rec.attached.retain(|x| *x != sid);
            }
            Response::Ok
        }
        Request::Input { session: sid, bytes } => match session(sid) {
            Some(s) => match s.write(&bytes) {
                Ok(()) => Response::Ok,
                Err(e) => Response::Error(format!("write failed: {e:#}")),
            },
            None => Response::Error(format!("no session {}", sid.0)),
        },
        Request::Resize { session: sid, rows, cols } => match session(sid) {
            Some(s) => match s.resize(rows, cols) {
                Ok(()) => Response::Ok,
                Err(e) => Response::Error(format!("resize failed: {e:#}")),
            },
            None => Response::Error(format!("no session {}", sid.0)),
        },
        Request::Kill { session: sid } => match session(sid) {
            Some(s) => match s.kill() {
                Ok(()) => Response::Ok,
                Err(e) => Response::Error(format!("kill failed: {e:#}")),
            },
            None => Response::Error(format!("no session {}", sid.0)),
        },
        Request::ListSessions => {
            let st = state.lock().unwrap();
            let mut list: Vec<_> = st.sessions.values().map(|s| s.info()).collect();
            list.sort_by_key(|s| s.id.0);
            Response::Sessions(list)
        }
        Request::SetState { session: sid, state: claude } => match session(sid) {
            Some(s) => {
                s.set_state(claude);
                Response::Ok
            }
            None => Response::Error(format!("no session {}", sid.0)),
        },
    }
}
```

- [ ] **Step 9: Write a watchdog stub so the server compiles (Task 4 fills it in)**

Create `crates/sarkosd/src/watchdog.rs`:
```rust
//! Filled in by Task 4. For now the watchdog thread does nothing.
use crate::server::Shared;
use std::time::Duration;

pub fn start(_state: Shared, _timeout: Duration) {}
```

- [ ] **Step 10: Write main**

Replace `crates/sarkosd/src/main.rs`:
```rust
mod ring;
mod server;
mod session;
mod watchdog;

use std::path::PathBuf;
use std::time::Duration;

const DEFAULT_REPLAY_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_WATCHDOG_SECS: u64 = 30;

fn default_socket() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME is set");
    PathBuf::from(home).join("Library/Application Support/sarkos/sarkosd.sock")
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut socket = default_socket();
    let mut replay = DEFAULT_REPLAY_BYTES;
    let mut watchdog = DEFAULT_WATCHDOG_SECS;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--version" => {
                println!("sarkosd {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--socket" => socket = PathBuf::from(args.next().expect("--socket needs a path")),
            "--replay-bytes" => replay = args.next().and_then(|v| v.parse().ok()).expect("--replay-bytes needs a number"),
            "--watchdog-secs" => watchdog = args.next().and_then(|v| v.parse().ok()).expect("--watchdog-secs needs a number"),
            other => {
                eprintln!("sarkosd: unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    let opts = server::Options { replay_capacity: replay, watchdog: Duration::from_secs(watchdog) };
    if let Err(e) = server::run(&socket, opts) {
        eprintln!("sarkosd: {e:#}");
        std::process::exit(1);
    }
}
```

- [ ] **Step 11: Run the integration tests**

Run: `cargo test -p sarkosd 2>&1 | tail -15`
Expected: ring tests `4 passed`; daemon tests `7 passed`. If `replay_truncated_to_capacity` fails because the shell echoes the long command and pushes "END" out of a 64-byte buffer, the test is wrong, not the daemon: raise `--replay-bytes` in that test to `256` and keep the `<= 256` assertion.

- [ ] **Step 12: Commit**

```bash
git add crates/sarkosd
git commit -m "sarkosd: sessions over a Unix socket with a replay ring buffer"
```

---

### Task 4: Watchdog

**Files:**
- Modify: `crates/sarkosd/src/watchdog.rs`
- Test: `crates/sarkosd/tests/daemon.rs` (append)

**Interfaces:**
- Consumes: `server::Shared`, `ClientRecord { last_heartbeat, ui_pid, ui_exe, connected }`.
- Produces: `watchdog::start(state, timeout)`: a thread that every `timeout / 6` (minimum 500 ms) scans clients; a client whose heartbeat is older than `timeout`, whose `ui_pid` is alive, gets `SIGKILL`, and if `ui_exe` is set the daemon spawns it detached. A zero timeout disables the watchdog.

- [ ] **Step 1: Write the failing test**

Append to `crates/sarkosd/tests/daemon.rs`:
```rust
#[test]
fn watchdog_kills_a_silent_ui_and_relaunches_it() {
    let d = Daemon::start(&["--watchdog-secs", "2"]);
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("relaunched");
    // The "UI" is a sleep; the "relaunch" is a script that touches a marker file.
    let script = dir.path().join("relaunch.sh");
    std::fs::write(&script, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut fake_ui = Command::new("/bin/sleep").arg("60").spawn().unwrap();

    let stream = UnixStream::connect(&d.socket).unwrap();
    let mut w = BufWriter::new(stream.try_clone().unwrap());
    let mut r = BufReader::new(stream);
    write_frame(&mut w, &ClientMessage { id: 1, request: Request::Hello { ui_pid: fake_ui.id(), ui_exe: Some(script.clone()) } }).unwrap();
    let _: ServerMessage = read_frame(&mut r).unwrap().unwrap();
    // No heartbeats. Within 2 s plus one scan the daemon must act.
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Ok(Some(_)) = fake_ui.try_wait() {
            break;
        }
        assert!(Instant::now() < deadline, "watchdog never killed the silent UI");
        std::thread::sleep(Duration::from_millis(100));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker.exists() {
        assert!(Instant::now() < deadline, "watchdog never relaunched the UI");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn watchdog_leaves_a_heartbeating_ui_alone() {
    let d = Daemon::start(&["--watchdog-secs", "2"]);
    let mut fake_ui = Command::new("/bin/sleep").arg("60").spawn().unwrap();
    let stream = UnixStream::connect(&d.socket).unwrap();
    let mut w = BufWriter::new(stream.try_clone().unwrap());
    let mut r = BufReader::new(stream);
    write_frame(&mut w, &ClientMessage { id: 1, request: Request::Hello { ui_pid: fake_ui.id(), ui_exe: None } }).unwrap();
    let _: ServerMessage = read_frame(&mut r).unwrap().unwrap();
    for seq in 0..5u64 {
        std::thread::sleep(Duration::from_millis(800));
        write_frame(&mut w, &ClientMessage { id: 2 + seq as u32, request: Request::Heartbeat { seq } }).unwrap();
        let _: ServerMessage = read_frame(&mut r).unwrap().unwrap();
    }
    assert!(fake_ui.try_wait().unwrap().is_none(), "UI was killed despite heartbeats");
    let _ = fake_ui.kill();
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p sarkosd --test daemon watchdog 2>&1 | tail -6`
Expected: `watchdog_kills_a_silent_ui_and_relaunches_it` fails with "watchdog never killed the silent UI"; the other passes trivially.

- [ ] **Step 3: Implement the watchdog**

Replace `crates/sarkosd/src/watchdog.rs`:
```rust
//! Relaunches a UI whose main thread has stopped heartbeating (spec 2.4). Sessions are untouched.
use crate::server::Shared;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn start(state: Shared, timeout: Duration) {
    if timeout.is_zero() {
        return;
    }
    let interval = std::cmp::max(timeout / 6, Duration::from_millis(500));
    std::thread::Builder::new()
        .name("watchdog".into())
        .spawn(move || loop {
            std::thread::sleep(interval);
            scan(&state, timeout);
        })
        .expect("spawn watchdog");
}

fn pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

fn scan(state: &Shared, timeout: Duration) {
    let now = Instant::now();
    let mut to_restart = Vec::new();
    {
        let mut st = state.lock().unwrap();
        for rec in st.clients.values_mut() {
            let Some(pid) = rec.ui_pid else { continue };
            if !rec.connected || now.duration_since(rec.last_heartbeat) < timeout {
                continue;
            }
            if !pid_alive(pid) {
                continue;
            }
            eprintln!("sarkosd: UI pid {pid} silent for {:?}, relaunching", timeout);
            to_restart.push((pid, rec.ui_exe.clone()));
            // Reset so one stuck UI is not killed again on the next scan while it restarts.
            rec.last_heartbeat = now;
        }
    }
    for (pid, exe) in to_restart {
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
        }
        if let Some(exe) = exe {
            let mut cmd = Command::new(&exe);
            cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
            if let Err(e) = cmd.spawn() {
                eprintln!("sarkosd: relaunch {} failed: {e}", exe.display());
            }
        }
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p sarkosd 2>&1 | tail -6`
Expected: all daemon tests pass, `9 passed` for the integration file.

- [ ] **Step 5: Commit**

```bash
git add crates/sarkosd
git commit -m "sarkosd: watchdog kills and relaunches a UI that stops heartbeating"
```

---

### Task 5: UI client, daemon autostart, and a window

**Files:**
- Create: `crates/sarkos/src/paths.rs`, `crates/sarkos/src/client.rs`
- Modify: `crates/sarkos/src/main.rs`
- Test: inline tests in `client.rs` against a fake server thread

**Interfaces:**
- Consumes: `protocol::*`.
- Produces:
  - `paths::socket_path() -> PathBuf`, `paths::workspace_file(project: &Path) -> PathBuf`, `paths::daemon_exe() -> PathBuf` (sibling of the current exe named `sarkosd`)
  - `client::Client::connect(socket: &Path) -> anyhow::Result<(Client, UnboundedReceiver<ServerMessage>)>`
  - `client::Client::connect_or_start(socket, daemon_exe) -> anyhow::Result<(Client, UnboundedReceiver<ServerMessage>)>`: connects, or removes a stale socket, starts the daemon detached, and retries for 3 s
  - `Client::send(&self, request: Request) -> u32` (request id), non-blocking
  - `Client::ensure_stale_socket_removed(socket)` (used by the test)
  - `main.rs`: a GPUI window titled `sarkos`, background `#11161f`, that connects on launch and prints the session list to stderr

- [ ] **Step 1: Write paths.rs**

```rust
//! Where sarkos keeps its socket and workspace files (spec 2.1, 4).
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

fn app_support() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME is set");
    PathBuf::from(home).join("Library/Application Support/sarkos")
}

pub fn socket_path() -> PathBuf {
    app_support().join("sarkosd.sock")
}

pub fn workspace_file(project: &Path) -> PathBuf {
    let mut h = DefaultHasher::new();
    project.hash(&mut h);
    app_support().join("workspaces").join(format!("{:016x}.json", h.finish()))
}

pub fn daemon_exe() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("sarkosd")))
        .unwrap_or_else(|| PathBuf::from("sarkosd"))
}
```

- [ ] **Step 2: Write the failing client tests**

Create `crates/sarkos/src/client.rs` with the tests module first:
```rust
//! The UI's connection to sarkosd: a writer thread fed by `send`, a reader thread that forwards
//! every `ServerMessage` into a futures channel the GPUI side awaits. Nothing here blocks the
//! caller.

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::codec::{read_frame, write_frame};
    use protocol::{ClientMessage, Request, Response};
    use std::io::{BufReader, BufWriter};
    use std::os::unix::net::UnixListener;
    use futures::executor::block_on;
    use futures::StreamExt;

    /// A one-connection fake daemon that answers every request with Ok.
    fn fake_daemon(socket: &std::path::Path) -> std::thread::JoinHandle<Vec<ClientMessage>> {
        let listener = UnixListener::bind(socket).unwrap();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let mut w = BufWriter::new(stream);
            let mut seen = Vec::new();
            while let Ok(Some(msg)) = read_frame::<_, ClientMessage>(&mut r) {
                let id = msg.id;
                seen.push(msg);
                write_frame(&mut w, &ServerMessage::Response { id, response: Response::Ok }).unwrap();
                if seen.len() == 2 {
                    break;
                }
            }
            seen
        })
    }

    #[test]
    fn sends_hello_first_and_forwards_responses() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let server = fake_daemon(&socket);
        let (client, mut rx) = Client::connect(&socket).unwrap();
        let id = client.send(Request::ListSessions);
        let first = block_on(rx.next()).unwrap();
        let second = block_on(rx.next()).unwrap();
        assert!(matches!(first, ServerMessage::Response { id: 1, response: Response::Ok }));
        assert!(matches!(second, ServerMessage::Response { id: i, .. } if i == id));
        let seen = server.join().unwrap();
        assert!(matches!(seen[0].request, Request::Hello { .. }));
        assert_eq!(seen[1].request, Request::ListSessions);
    }

    #[test]
    fn connect_replaces_stale_socket() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("stale.sock");
        // A socket file nobody listens on.
        drop(UnixListener::bind(&socket).unwrap());
        assert!(socket.exists());
        Client::ensure_stale_socket_removed(&socket);
        assert!(!socket.exists());
    }
}
```

- [ ] **Step 3: Run to verify they fail**

Add `mod client; mod paths;` to `crates/sarkos/src/main.rs`. Run: `cargo test -p sarkos client 2>&1 | tail -5`
Expected: compile error, `Client` not defined.

- [ ] **Step 4: Implement the client**

Insert above the tests module in `client.rs`:
```rust
use anyhow::{Context, Result};
use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use protocol::codec::{read_frame, write_frame};
use protocol::{ClientMessage, Request, ServerMessage};
use std::io::{BufReader, BufWriter};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::time::{Duration, Instant};

pub struct Client {
    tx: Sender<ClientMessage>,
    next_id: AtomicU32,
}

impl Client {
    pub fn connect(socket: &Path) -> Result<(Client, UnboundedReceiver<ServerMessage>)> {
        let stream = UnixStream::connect(socket).with_context(|| format!("connect {}", socket.display()))?;
        let reader_stream = stream.try_clone().context("clone socket")?;
        let (tx, rx) = channel::<ClientMessage>();
        let (ev_tx, ev_rx): (UnboundedSender<ServerMessage>, UnboundedReceiver<ServerMessage>) = unbounded();
        std::thread::Builder::new()
            .name("sarkosd-writer".into())
            .spawn(move || {
                let mut w = BufWriter::new(stream);
                while let Ok(msg) = rx.recv() {
                    if write_frame(&mut w, &msg).is_err() {
                        break;
                    }
                }
            })
            .context("spawn writer")?;
        std::thread::Builder::new()
            .name("sarkosd-reader".into())
            .spawn(move || {
                let mut r = BufReader::new(reader_stream);
                while let Ok(Some(msg)) = read_frame::<_, ServerMessage>(&mut r) {
                    if ev_tx.unbounded_send(msg).is_err() {
                        break;
                    }
                }
            })
            .context("spawn reader")?;
        let client = Client { tx, next_id: AtomicU32::new(1) };
        client.send(Request::Hello { ui_pid: std::process::id(), ui_exe: std::env::current_exe().ok() });
        Ok((client, ev_rx))
    }

    /// Queues a request. Returns its id so the response can be matched.
    pub fn send(&self, request: Request) -> u32 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let _ = self.tx.send(ClientMessage { id, request });
        id
    }

    /// Removes a socket file that exists but accepts no connections.
    pub fn ensure_stale_socket_removed(socket: &Path) {
        if socket.exists() && UnixStream::connect(socket).is_err() {
            let _ = std::fs::remove_file(socket);
        }
    }

    /// Connects, or starts the daemon detached from this process and retries for three seconds.
    pub fn connect_or_start(socket: &Path, daemon_exe: &Path) -> Result<(Client, UnboundedReceiver<ServerMessage>)> {
        if let Ok(c) = Client::connect(socket) {
            return Ok(c);
        }
        Client::ensure_stale_socket_removed(socket);
        let log = socket.with_file_name("sarkosd.log");
        if let Some(parent) = socket.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let log_file = std::fs::OpenOptions::new().create(true).append(true).open(&log).context("open daemon log")?;
        let mut cmd = Command::new(daemon_exe);
        cmd.arg("--socket").arg(socket).stdin(Stdio::null()).stdout(Stdio::null()).stderr(log_file);
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        cmd.spawn().with_context(|| format!("start {}", daemon_exe.display()))?;
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match Client::connect(socket) {
                Ok(c) => return Ok(c),
                Err(e) if Instant::now() >= deadline => return Err(e),
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    }
}
```

- [ ] **Step 5: Run the client tests**

Run: `cargo test -p sarkos client 2>&1 | tail -5`
Expected: `2 passed`.

- [ ] **Step 6: Write main with a GPUI window**

Replace `crates/sarkos/src/main.rs`:
```rust
mod client;
mod paths;

use futures::StreamExt;
use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, Window, WindowBounds, WindowOptions,
};
use protocol::{Request, Response, ServerMessage};
use std::sync::Arc;

const BG: u32 = 0x11161f;

struct Root {
    client: Arc<client::Client>,
    status: String,
}

impl Render for Root {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(BG)).text_color(rgb(0xd6dae0)).p_4().child(self.status.clone())
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") {
        println!("sarkos {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let (client, mut events) = match client::Client::connect_or_start(&paths::socket_path(), &paths::daemon_exe()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("sarkos: cannot reach sarkosd: {e:#}");
            std::process::exit(1);
        }
    };
    let client = Arc::new(client);
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let client_for_view = client.clone();
        let root = cx
            .open_window(
                WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
                move |_, cx| cx.new(|_| Root { client: client_for_view, status: "connecting".into() }),
            )
            .expect("open window");
        let list_id = client.send(Request::ListSessions);
        let root_entity = root.entity(cx).expect("root entity");
        cx.spawn(async move |cx| {
            while let Some(msg) = events.next().await {
                let ok = root_entity.update(cx, |root, cx| {
                    if let ServerMessage::Response { id, response: Response::Sessions(list) } = &msg {
                        if *id == list_id {
                            root.status = format!("{} session(s) in sarkosd", list.len());
                            eprintln!("sarkos: {}", root.status);
                            cx.notify();
                        }
                    }
                });
                if ok.is_err() {
                    break;
                }
            }
        })
        .detach();
    });
}
```

If `root.entity(cx)` does not exist on `WindowHandle<Root>` in gpui 0.2.2, grep `fn entity` in `window.rs` of the vendored crate; the alternative is `root.update(cx, |_, _, cx| cx.entity())`.

- [ ] **Step 7: Run it**

Run: `cargo run -p sarkos 2>&1 | head -5` (a window opens; close it with Cmd+Q)
Expected: stderr prints `sarkos: 0 session(s) in sarkosd` within a second, a navy window shows the same text, and `pgrep sarkosd` shows a daemon running afterwards. Note: the daemon keeps running; that is the design.

- [ ] **Step 8: Commit**

```bash
git add crates/sarkos
git commit -m "sarkos: socket client, daemon autostart, first GPUI window"
```

---

### Task 6: Keystroke to bytes

**Files:**
- Create: `crates/sarkos/src/keys.rs`
- Test: inline

**Interfaces:**
- Consumes: `gpui::Keystroke { key: String, key_char: Option<String>, modifiers: Modifiers { control, alt, shift, platform, function } }`.
- Produces: `keys::keystroke_to_bytes(k: &Keystroke, app_cursor: bool) -> Option<Vec<u8>>`. `None` means "not for the terminal" (for example Cmd combos, handled elsewhere).

- [ ] **Step 1: Write the failing tests**

```rust
//! Maps a GPUI keystroke to the bytes a terminal expects (xterm conventions).
use gpui::{Keystroke, Modifiers};

#[cfg(test)]
mod tests {
    use super::*;

    fn ks(key: &str, key_char: Option<&str>, m: Modifiers) -> Keystroke {
        Keystroke { modifiers: m, key: key.to_string(), key_char: key_char.map(|s| s.to_string()) }
    }
    fn none() -> Modifiers { Modifiers::default() }
    fn ctrl() -> Modifiers { Modifiers { control: true, ..Modifiers::default() } }
    fn alt() -> Modifiers { Modifiers { alt: true, ..Modifiers::default() } }
    fn shift() -> Modifiers { Modifiers { shift: true, ..Modifiers::default() } }
    fn cmd() -> Modifiers { Modifiers { platform: true, ..Modifiers::default() } }

    #[test]
    fn printable_uses_key_char() {
        assert_eq!(keystroke_to_bytes(&ks("a", Some("a"), none()), false), Some(b"a".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("a", Some("A"), shift()), false), Some(b"A".to_vec()));
    }

    #[test]
    fn option_s_sends_eszett() {
        assert_eq!(keystroke_to_bytes(&ks("s", Some("ß"), alt()), false), Some("ß".as_bytes().to_vec()));
    }

    #[test]
    fn control_letters_are_control_codes() {
        assert_eq!(keystroke_to_bytes(&ks("c", None, ctrl()), false), Some(vec![0x03]));
        assert_eq!(keystroke_to_bytes(&ks("d", None, ctrl()), false), Some(vec![0x04]));
        assert_eq!(keystroke_to_bytes(&ks("[", None, ctrl()), false), Some(vec![0x1b]));
    }

    #[test]
    fn editing_keys() {
        assert_eq!(keystroke_to_bytes(&ks("enter", None, none()), false), Some(b"\r".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("backspace", None, none()), false), Some(vec![0x7f]));
        assert_eq!(keystroke_to_bytes(&ks("tab", None, none()), false), Some(b"\t".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("tab", None, shift()), false), Some(b"\x1b[Z".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("escape", None, none()), false), Some(vec![0x1b]));
        assert_eq!(keystroke_to_bytes(&ks("delete", None, none()), false), Some(b"\x1b[3~".to_vec()));
    }

    #[test]
    fn arrows_follow_cursor_mode() {
        assert_eq!(keystroke_to_bytes(&ks("up", None, none()), false), Some(b"\x1b[A".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("up", None, none()), true), Some(b"\x1bOA".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("left", None, alt()), false), Some(b"\x1bb".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("right", None, alt()), false), Some(b"\x1bf".to_vec()));
    }

    #[test]
    fn navigation_keys() {
        assert_eq!(keystroke_to_bytes(&ks("home", None, none()), false), Some(b"\x1b[H".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("end", None, none()), false), Some(b"\x1b[F".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("pageup", None, none()), false), Some(b"\x1b[5~".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("pagedown", None, none()), false), Some(b"\x1b[6~".to_vec()));
    }

    #[test]
    fn command_combos_are_not_terminal_input() {
        assert_eq!(keystroke_to_bytes(&ks("v", None, cmd()), false), None);
        assert_eq!(keystroke_to_bytes(&ks("q", None, cmd()), false), None);
    }

    #[test]
    fn option_with_plain_letter_sends_meta() {
        // GPUI reports an alt keystroke with no key_char when the layout yields nothing special.
        assert_eq!(keystroke_to_bytes(&ks("x", None, alt()), false), Some(b"\x1bx".to_vec()));
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Add `mod keys;` to `main.rs`. Run: `cargo test -p sarkos keys 2>&1 | tail -5`
Expected: compile error, `keystroke_to_bytes` not defined.

- [ ] **Step 3: Implement**

Insert above the tests:
```rust
pub fn keystroke_to_bytes(k: &Keystroke, app_cursor: bool) -> Option<Vec<u8>> {
    let m = &k.modifiers;
    if m.platform {
        return None;
    }
    let key = k.key.as_str();
    let arrow = |csi: &str, ss3: &str| -> Vec<u8> {
        if app_cursor { format!("\x1bO{ss3}").into_bytes() } else { format!("\x1b[{csi}").into_bytes() }
    };
    let bytes = match key {
        "enter" => b"\r".to_vec(),
        "backspace" => vec![0x7f],
        "tab" if m.shift => b"\x1b[Z".to_vec(),
        "tab" => b"\t".to_vec(),
        "escape" => vec![0x1b],
        "delete" => b"\x1b[3~".to_vec(),
        "space" if m.control => vec![0x00],
        "space" => b" ".to_vec(),
        "up" => arrow("A", "A"),
        "down" => arrow("B", "B"),
        "right" if m.alt => b"\x1bf".to_vec(),
        "left" if m.alt => b"\x1bb".to_vec(),
        "right" => arrow("C", "C"),
        "left" => arrow("D", "D"),
        "home" => b"\x1b[H".to_vec(),
        "end" => b"\x1b[F".to_vec(),
        "pageup" => b"\x1b[5~".to_vec(),
        "pagedown" => b"\x1b[6~".to_vec(),
        _ => {
            if m.control {
                let c = key.chars().next()?;
                let code = match c {
                    'a'..='z' => (c as u8) - b'a' + 1,
                    '[' => 0x1b,
                    '\\' => 0x1c,
                    ']' => 0x1d,
                    '^' | '6' => 0x1e,
                    '_' | '-' => 0x1f,
                    '?' => 0x7f,
                    _ => return None,
                };
                return Some(vec![code]);
            }
            if let Some(ch) = &k.key_char {
                if !ch.is_empty() {
                    return Some(ch.as_bytes().to_vec());
                }
            }
            if m.alt && key.chars().count() == 1 {
                let mut v = vec![0x1b];
                v.extend_from_slice(key.as_bytes());
                return Some(v);
            }
            if key.chars().count() == 1 {
                return Some(key.as_bytes().to_vec());
            }
            return None;
        }
    };
    Some(bytes)
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p sarkos keys 2>&1 | tail -5`
Expected: `8 passed`.

- [ ] **Step 5: Commit**

```bash
git add crates/sarkos/src/keys.rs crates/sarkos/src/main.rs
git commit -m "sarkos: map keystrokes to terminal bytes"
```

---

### Task 7: Scroll animator

**Files:**
- Create: `crates/sarkos/src/scroll.rs`
- Test: inline

**Interfaces:**
- Produces: `ScrollAnimator { current: f32, target: f32 }` in lines of history (0 = bottom). `new()`, `scroll_by(&mut self, delta_lines: f32, max: f32)`, `jump_to(&mut self, lines: f32)`, `tick(&mut self, dt_seconds: f32) -> bool` (true while still moving), `clamp_max(&mut self, max: f32)`, `is_at_bottom() -> bool`.
- Spec 3: time-based ease-out, so frame drops change nothing about where it lands.

- [ ] **Step 1: Write the failing tests**

```rust
//! Smooth scrolling as a pure function of time (spec 3). Position is in lines of history:
//! 0.0 is the live bottom, larger is further up. Non-integer values are drawn as a fractional
//! pixel offset by the element.

pub struct ScrollAnimator {
    pub current: f32,
    pub target: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eases_toward_target_and_settles() {
        let mut s = ScrollAnimator::new();
        s.scroll_by(10.0, 100.0);
        assert!(s.tick(0.016));
        assert!(s.current > 0.0 && s.current < 10.0, "moved part way: {}", s.current);
        for _ in 0..60 {
            s.tick(0.016);
        }
        assert!(!s.tick(0.016), "should have settled");
        assert_eq!(s.current, 10.0);
    }

    #[test]
    fn landing_point_is_independent_of_frame_rate() {
        let mut a = ScrollAnimator::new();
        let mut b = ScrollAnimator::new();
        a.scroll_by(25.0, 100.0);
        b.scroll_by(25.0, 100.0);
        for _ in 0..100 { a.tick(0.016); }
        for _ in 0..10 { b.tick(0.16); }
        assert!((a.current - 25.0).abs() < 0.001);
        assert!((b.current - 25.0).abs() < 0.001);
    }

    #[test]
    fn clamps_to_history_and_bottom() {
        let mut s = ScrollAnimator::new();
        s.scroll_by(500.0, 42.0);
        assert_eq!(s.target, 42.0);
        s.scroll_by(-1000.0, 42.0);
        assert_eq!(s.target, 0.0);
    }

    #[test]
    fn shrinking_history_clamps_current_position() {
        let mut s = ScrollAnimator::new();
        s.jump_to(50.0);
        s.clamp_max(20.0);
        assert_eq!(s.current, 20.0);
        assert_eq!(s.target, 20.0);
    }

    #[test]
    fn at_bottom_only_when_settled_at_zero() {
        let mut s = ScrollAnimator::new();
        assert!(s.is_at_bottom());
        s.scroll_by(3.0, 10.0);
        assert!(!s.is_at_bottom());
        s.jump_to(0.0);
        assert!(s.is_at_bottom());
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Add `mod scroll;` to `main.rs`. Run: `cargo test -p sarkos scroll 2>&1 | tail -5`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
/// Time constant of the ease-out, in seconds. 0.06 feels like a trackpad glide without lag.
const TAU: f32 = 0.06;
const SETTLE: f32 = 0.005;

impl ScrollAnimator {
    pub fn new() -> Self {
        Self { current: 0.0, target: 0.0 }
    }

    pub fn scroll_by(&mut self, delta_lines: f32, max: f32) {
        self.target = (self.target + delta_lines).clamp(0.0, max.max(0.0));
    }

    pub fn jump_to(&mut self, lines: f32) {
        self.target = lines.max(0.0);
        self.current = self.target;
    }

    pub fn clamp_max(&mut self, max: f32) {
        let max = max.max(0.0);
        self.target = self.target.min(max);
        self.current = self.current.min(max);
    }

    /// Advances by `dt_seconds`. Returns true while another frame is needed.
    pub fn tick(&mut self, dt_seconds: f32) -> bool {
        let diff = self.target - self.current;
        if diff.abs() < SETTLE {
            self.current = self.target;
            return false;
        }
        let alpha = 1.0 - (-dt_seconds / TAU).exp();
        self.current += diff * alpha;
        true
    }

    pub fn is_at_bottom(&self) -> bool {
        self.current == 0.0 && self.target == 0.0
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p sarkos scroll 2>&1 | tail -5`
Expected: `5 passed`.

- [ ] **Step 5: Commit**

```bash
git add crates/sarkos/src/scroll.rs crates/sarkos/src/main.rs
git commit -m "sarkos: time-based smooth scroll animator"
```

---

### Task 8: Palette and row layout (pure)

**Files:**
- Create: `crates/sarkos/src/palette.rs`, `crates/sarkos/src/layout.rs`
- Test: inline in both

**Interfaces:**
- Produces:
  - `palette::Rgb { r: u8, g: u8, b: u8 }` (Copy, Eq); `palette::NAVY: Palette` with `bg`, `fg`, `cursor`, `ansi: [Rgb; 16]`; `Palette::indexed(&self, i: u8) -> Rgb` (ANSI 0 to 15 from the table, 16 to 231 the 6x6x6 cube, 232 to 255 the grey ramp); `Palette::resolve(&self, c: &alacritty_terminal::vte::ansi::Color) -> Rgb`
  - `layout::CellStyle { fg: Rgb, bg: Rgb, bold: bool, italic: bool, dim: bool, underline: bool, inverse: bool }`
  - `layout::Run { col: usize, text: String, style: CellStyle, wide: bool }`
  - `layout::build_runs(cells: impl Iterator<Item = (char, CellStyle, CellKind)>) -> Vec<Run>` where `CellKind { Normal, Wide, Spacer }`. Runs break on style change and before every wide char; spacers are skipped; `inverse` swaps fg and bg at build time so painters never do it.

- [ ] **Step 1: Write the failing palette tests**

Create `crates/sarkos/src/palette.rs`:
```rust
//! The Navy theme's colours, hardcoded for milestone 1 (theme files arrive in milestone 2).
//! Values follow spec 7.1: a lifted navy background, a bright grey foreground, and ANSI colours
//! that each clear 4.5:1 luminance contrast on the background.
use alacritty_terminal::vte::ansi::{Color, NamedColor};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

pub const fn rgb(hex: u32) -> Rgb {
    Rgb { r: ((hex >> 16) & 0xff) as u8, g: ((hex >> 8) & 0xff) as u8, b: (hex & 0xff) as u8 }
}

pub struct Palette {
    pub bg: Rgb,
    pub fg: Rgb,
    pub cursor: Rgb,
    pub ansi: [Rgb; 16],
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Rgb) -> f64 {
        fn ch(v: u8) -> f64 {
            let v = v as f64 / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        }
        0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
    }
    fn contrast(a: Rgb, b: Rgb) -> f64 {
        let (l1, l2) = (luminance(a), luminance(b));
        let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn foreground_is_bright_not_dim() {
        let c = contrast(NAVY.fg, NAVY.bg);
        assert!(c >= 11.0, "fg/bg contrast {c:.1} should be about 12:1");
    }

    #[test]
    fn every_ansi_colour_except_black_clears_4_5_to_1() {
        for (i, c) in NAVY.ansi.iter().enumerate() {
            if i == 0 || i == 8 {
                continue; // black and bright black are backgrounds by convention
            }
            let r = contrast(*c, NAVY.bg);
            assert!(r >= 4.5, "ansi {i} has contrast {r:.2}");
        }
    }

    #[test]
    fn indexed_cube_and_grey_ramp() {
        assert_eq!(NAVY.indexed(16), rgb(0x000000));
        assert_eq!(NAVY.indexed(231), rgb(0xffffff));
        assert_eq!(NAVY.indexed(196), rgb(0xff0000));
        assert_eq!(NAVY.indexed(232), rgb(0x080808));
        assert_eq!(NAVY.indexed(255), rgb(0xeeeeee));
        assert_eq!(NAVY.indexed(1), NAVY.ansi[1]);
    }

    #[test]
    fn resolves_named_spec_and_indexed() {
        assert_eq!(NAVY.resolve(&Color::Named(NamedColor::Foreground)), NAVY.fg);
        assert_eq!(NAVY.resolve(&Color::Named(NamedColor::Background)), NAVY.bg);
        assert_eq!(NAVY.resolve(&Color::Named(NamedColor::Red)), NAVY.ansi[1]);
        assert_eq!(NAVY.resolve(&Color::Named(NamedColor::BrightBlue)), NAVY.ansi[12]);
        assert_eq!(NAVY.resolve(&Color::Spec(alacritty_terminal::vte::ansi::Rgb { r: 1, g: 2, b: 3 })), Rgb { r: 1, g: 2, b: 3 });
        assert_eq!(NAVY.resolve(&Color::Indexed(196)), rgb(0xff0000));
    }
}
```

- [ ] **Step 2: Implement the palette**

Insert above the tests:
```rust
pub const NAVY: Palette = Palette {
    bg: rgb(0x11161f),
    fg: rgb(0xd6dae0),
    cursor: rgb(0xd6dae0),
    ansi: [
        rgb(0x1b2230), // black
        rgb(0xf08c8c), // red
        rgb(0x8fd9a8), // green
        rgb(0xe8c97a), // yellow
        rgb(0x8ab8f0), // blue
        rgb(0xc9a4e8), // magenta
        rgb(0x7fd4d4), // cyan
        rgb(0xc8ccd4), // white
        rgb(0x6b7585), // bright black
        rgb(0xf7a8a8), // bright red
        rgb(0xa8e6bd), // bright green
        rgb(0xf0d898), // bright yellow
        rgb(0xa8ccf5), // bright blue
        rgb(0xd9bcf0), // bright magenta
        rgb(0x9ee2e2), // bright cyan
        rgb(0xe8ebf0), // bright white
    ],
};

impl Palette {
    pub fn indexed(&self, i: u8) -> Rgb {
        match i {
            0..=15 => self.ansi[i as usize],
            16..=231 => {
                let i = i - 16;
                let steps = [0u8, 95, 135, 175, 215, 255];
                Rgb { r: steps[(i / 36) as usize], g: steps[((i % 36) / 6) as usize], b: steps[(i % 6) as usize] }
            }
            232..=255 => {
                let v = 8 + (i - 232) * 10;
                Rgb { r: v, g: v, b: v }
            }
        }
    }

    pub fn resolve(&self, c: &Color) -> Rgb {
        match c {
            Color::Spec(c) => Rgb { r: c.r, g: c.g, b: c.b },
            Color::Indexed(i) => self.indexed(*i),
            Color::Named(n) => match n {
                NamedColor::Foreground | NamedColor::BrightForeground => self.fg,
                NamedColor::Background => self.bg,
                NamedColor::Cursor => self.cursor,
                NamedColor::DimForeground => self.ansi[7],
                NamedColor::Black | NamedColor::DimBlack => self.ansi[0],
                NamedColor::Red | NamedColor::DimRed => self.ansi[1],
                NamedColor::Green | NamedColor::DimGreen => self.ansi[2],
                NamedColor::Yellow | NamedColor::DimYellow => self.ansi[3],
                NamedColor::Blue | NamedColor::DimBlue => self.ansi[4],
                NamedColor::Magenta | NamedColor::DimMagenta => self.ansi[5],
                NamedColor::Cyan | NamedColor::DimCyan => self.ansi[6],
                NamedColor::White | NamedColor::DimWhite => self.ansi[7],
                NamedColor::BrightBlack => self.ansi[8],
                NamedColor::BrightRed => self.ansi[9],
                NamedColor::BrightGreen => self.ansi[10],
                NamedColor::BrightYellow => self.ansi[11],
                NamedColor::BrightBlue => self.ansi[12],
                NamedColor::BrightMagenta => self.ansi[13],
                NamedColor::BrightCyan => self.ansi[14],
                NamedColor::BrightWhite => self.ansi[15],
            },
        }
    }
}
```

If `NamedColor` has variants not listed here in alacritty_terminal 0.26 (the compiler will say), map each extra one to the nearest listed colour.

- [ ] **Step 3: Run the palette tests**

Add `mod palette;` to `main.rs`. Run: `cargo test -p sarkos palette 2>&1 | tail -5`
Expected: `4 passed`. If `every_ansi_colour_except_black_clears_4_5_to_1` fails for a colour, brighten that entry until it passes; the test is the requirement (spec 7.1).

- [ ] **Step 4: Write the failing layout tests**

Create `crates/sarkos/src/layout.rs`:
```rust
//! Turns one row of terminal cells into text runs with a starting column, so the painter can
//! shape each run once and place it at an exact cell x. Pure: no GPUI, so it is unit tested.
use crate::palette::Rgb;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellStyle {
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub dim: bool,
    pub underline: bool,
    pub inverse: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    Normal,
    Wide,
    Spacer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub col: usize,
    pub text: String,
    pub style: CellStyle,
    pub wide: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::rgb;

    fn st(fg: u32, bg: u32) -> CellStyle {
        CellStyle { fg: rgb(fg), bg: rgb(bg), bold: false, italic: false, dim: false, underline: false, inverse: false }
    }

    #[test]
    fn merges_same_style_cells() {
        let a = st(0xffffff, 0x000000);
        let runs = build_runs(vec![('h', a, CellKind::Normal), ('i', a, CellKind::Normal)].into_iter());
        assert_eq!(runs, vec![Run { col: 0, text: "hi".into(), style: a, wide: false }]);
    }

    #[test]
    fn breaks_on_style_change_and_keeps_columns() {
        let a = st(0xffffff, 0x000000);
        let b = st(0xff0000, 0x000000);
        let runs = build_runs(vec![('a', a, CellKind::Normal), ('b', b, CellKind::Normal), ('c', b, CellKind::Normal)].into_iter());
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0], Run { col: 0, text: "a".into(), style: a, wide: false });
        assert_eq!(runs[1], Run { col: 1, text: "bc".into(), style: b, wide: false });
    }

    #[test]
    fn wide_chars_get_their_own_run_and_spacers_are_skipped() {
        let a = st(0xffffff, 0x000000);
        let runs = build_runs(vec![('x', a, CellKind::Normal), ('語', a, CellKind::Wide), (' ', a, CellKind::Spacer), ('y', a, CellKind::Normal)].into_iter());
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[1], Run { col: 1, text: "語".into(), style: a, wide: true });
        assert_eq!(runs[2].col, 3);
        assert_eq!(runs[2].text, "y");
    }

    #[test]
    fn inverse_swaps_colours_at_build_time() {
        let mut a = st(0x111111, 0x222222);
        a.inverse = true;
        let runs = build_runs(vec![('z', a, CellKind::Normal)].into_iter());
        assert_eq!(runs[0].style.fg, rgb(0x222222));
        assert_eq!(runs[0].style.bg, rgb(0x111111));
        assert!(!runs[0].style.inverse);
    }

    #[test]
    fn empty_row_gives_no_runs() {
        assert!(build_runs(std::iter::empty()).is_empty());
    }
}
```

- [ ] **Step 5: Implement build_runs**

Insert above the tests:
```rust
pub fn build_runs(cells: impl Iterator<Item = (char, CellStyle, CellKind)>) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for (col, (c, mut style, kind)) in cells.enumerate() {
        if kind == CellKind::Spacer {
            continue;
        }
        if style.inverse {
            std::mem::swap(&mut style.fg, &mut style.bg);
            style.inverse = false;
        }
        let wide = kind == CellKind::Wide;
        let extend = match runs.last() {
            Some(last) => !wide && !last.wide && last.style == style && last.col + last.text.chars().count() == col,
            None => false,
        };
        if extend {
            runs.last_mut().unwrap().text.push(c);
        } else {
            runs.push(Run { col, text: c.to_string(), style, wide });
        }
    }
    runs
}
```

- [ ] **Step 6: Run the layout tests**

Add `mod layout;` to `main.rs`. Run: `cargo test -p sarkos layout 2>&1 | tail -5`
Expected: `5 passed`.

- [ ] **Step 7: Commit**

```bash
git add crates/sarkos/src/palette.rs crates/sarkos/src/layout.rs crates/sarkos/src/main.rs
git commit -m "sarkos: Navy palette and pure row layout"
```

---

### Task 9: Terminal entity

**Files:**
- Create: `crates/sarkos/src/terminal.rs`
- Test: inline (the `Term` runs headless; GPUI is not needed for these tests, so the tests build a `TerminalCore` directly)

**Interfaces:**
- Consumes: `protocol`, `client::Client`, `scroll::ScrollAnimator`, `keys`, `layout`, `palette`.
- Produces:
  - `TerminalCore` (no GPUI types): `new(cols, rows, scrollback)`, `apply_output(&mut self, bytes: &[u8]) -> Wakeup` where `Wakeup { repaint: bool, title: Option<String>, bell: bool }`, `resize(cols, rows)`, `rows_for_paint(&self) -> RowsForPaint` giving the integer display offset and an iterator over visible rows plus the one row above (each row as `Vec<(char, CellStyle, CellKind)>`), `cursor(&self) -> Option<(usize, usize)>` (col, line in viewport), `app_cursor(&self) -> bool`, `bracketed_paste(&self) -> bool`, `history_size(&self) -> usize`, `scroll: ScrollAnimator`, `set_display_offset(&mut self, off: usize)`
  - `Terminal` (GPUI entity): `new(core: TerminalCore, session: SessionId, client: Arc<Client>, cx: &mut Context<Self>)`, fields `core`, `session`, `client`, `focus: FocusHandle`, `title: String`, `alive: bool`, `cell: Option<CellMetrics>`; methods `handle_event(&mut self, ev: &Event, cx)`, `handle_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>)`, `paste(&mut self, text: &str)`, `scroll_lines(&mut self, lines: f32, cx)`, `ensure_size(&mut self, cols: usize, rows: usize)`
  - `CellMetrics { width: f32, height: f32 }` in pixels

- [ ] **Step 1: Write the failing core tests**

Create `crates/sarkos/src/terminal.rs`:
```rust
//! The terminal state behind one tab: an alacritty grid fed by the daemon, plus scroll state.
//! `TerminalCore` has no GPUI dependency so its behaviour is unit tested; `Terminal` wraps it as
//! a GPUI entity and owns the socket side.
use crate::client::Client;
use crate::keys::keystroke_to_bytes;
use crate::layout::{CellKind, CellStyle};
use crate::palette::{Palette, NAVY};
use crate::scroll::ScrollAnimator;
use alacritty_terminal::event::{Event as TermEvent, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::Line;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::Processor;
use gpui::{Context, FocusHandle, Keystroke};
use protocol::{Event, Request, SessionId};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(row: &[(char, CellStyle, CellKind)]) -> String {
        row.iter().map(|(c, _, _)| *c).collect::<String>().trim_end().to_string()
    }

    #[test]
    fn apply_output_fills_the_grid() {
        let mut t = TerminalCore::new(20, 5, 100);
        let w = t.apply_output(b"hello\r\nworld");
        assert!(w.repaint);
        let rows = t.rows_for_paint();
        assert_eq!(text_of(&rows.visible[0]), "hello");
        assert_eq!(text_of(&rows.visible[1]), "world");
        assert_eq!(t.cursor(), Some((5, 1)));
    }

    #[test]
    fn title_and_bell_are_reported() {
        let mut t = TerminalCore::new(20, 5, 100);
        let w = t.apply_output(b"\x1b]0;my title\x07");
        assert_eq!(w.title.as_deref(), Some("my title"));
        let w = t.apply_output(b"\x07");
        assert!(w.bell);
    }

    #[test]
    fn apply_output_coalesces_wakeups() {
        let mut t = TerminalCore::new(20, 5, 100);
        // One call with many lines is one wakeup, not one per line.
        let big: Vec<u8> = (0..1000).flat_map(|i| format!("line {i}\r\n").into_bytes()).collect();
        let w = t.apply_output(&big);
        assert!(w.repaint);
        assert_eq!(t.history_size(), 100, "scrollback is capped");
    }

    #[test]
    fn scrolling_up_keeps_view_stable_while_output_arrives() {
        let mut t = TerminalCore::new(20, 5, 100);
        for i in 0..50 {
            t.apply_output(format!("l{i}\r\n").as_bytes());
        }
        t.set_display_offset(10);
        let before = text_of(&t.rows_for_paint().visible[0]);
        t.apply_output(b"more\r\nmore\r\n");
        let after = text_of(&t.rows_for_paint().visible[0]);
        assert_eq!(before, after, "new output must not yank the view down");
    }

    #[test]
    fn row_above_is_provided_when_scrolled() {
        let mut t = TerminalCore::new(20, 5, 100);
        for i in 0..20 {
            t.apply_output(format!("l{i}\r\n").as_bytes());
        }
        t.set_display_offset(3);
        let rows = t.rows_for_paint();
        let above = rows.above.expect("a row above the viewport");
        let top = text_of(&rows.visible[0]);
        // The row above is the line just before the visible top.
        let top_n: usize = top.trim_start_matches('l').parse().unwrap();
        assert_eq!(text_of(&above), format!("l{}", top_n - 1));
        t.set_display_offset(0);
        assert!(t.rows_for_paint().above.is_none() || t.history_size() > 0);
    }

    #[test]
    fn resize_reflows_and_reports_modes() {
        let mut t = TerminalCore::new(20, 5, 100);
        t.apply_output(b"\x1b[?1h\x1b[?2004h");
        assert!(t.app_cursor());
        assert!(t.bracketed_paste());
        t.resize(40, 10);
        assert_eq!(t.rows_for_paint().visible.len(), 10);
        assert_eq!(t.rows_for_paint().visible[0].len(), 40);
    }

    #[test]
    fn replay_from_mid_sequence_recovers() {
        let mut t = TerminalCore::new(20, 5, 100);
        // Cut into the middle of an SGR sequence, then normal text.
        t.apply_output(b"1;32mgreen\x1b[0m plain");
        let rows = t.rows_for_paint();
        let line = text_of(&rows.visible[0]);
        assert!(line.ends_with("plain"), "got {line:?}");
    }

    #[test]
    fn styles_come_through() {
        let mut t = TerminalCore::new(20, 5, 100);
        t.apply_output(b"\x1b[1;31mX\x1b[0m");
        let rows = t.rows_for_paint();
        let (c, style, _) = rows.visible[0][0];
        assert_eq!(c, 'X');
        assert!(style.bold);
        assert_eq!(style.fg, NAVY.ansi[1]);
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Add `mod terminal;` to `main.rs`. Run: `cargo test -p sarkos terminal 2>&1 | tail -5`
Expected: compile error, `TerminalCore` not defined.

- [ ] **Step 3: Implement TerminalCore**

Insert above the tests:
```rust
/// Collects alacritty events during one `advance` so they can be reported after it.
#[derive(Clone, Default)]
struct Listener {
    events: Rc<RefCell<VecDeque<TermEvent>>>,
}

impl EventListener for Listener {
    fn send_event(&self, event: TermEvent) {
        self.events.borrow_mut().push_back(event);
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Wakeup {
    pub repaint: bool,
    pub title: Option<String>,
    pub bell: bool,
    pub pty_write: Vec<u8>,
}

pub struct RowsForPaint {
    pub display_offset: usize,
    /// The line just above the viewport, present when scrolled into history.
    pub above: Option<Vec<(char, CellStyle, CellKind)>>,
    pub visible: Vec<Vec<(char, CellStyle, CellKind)>>,
}

pub struct TerminalCore {
    term: Term<Listener>,
    parser: Processor,
    listener: Listener,
    palette: &'static Palette,
    pub scroll: ScrollAnimator,
}

impl TerminalCore {
    pub fn new(cols: usize, rows: usize, scrollback: usize) -> Self {
        let listener = Listener::default();
        let config = Config { scrolling_history: scrollback, ..Config::default() };
        let term = Term::new(config, &TermSize::new(cols, rows), listener.clone());
        Self { term, parser: Processor::new(), listener, palette: &NAVY, scroll: ScrollAnimator::new() }
    }

    pub fn apply_output(&mut self, bytes: &[u8]) -> Wakeup {
        self.parser.advance(&mut self.term, bytes);
        let mut w = Wakeup { repaint: true, ..Default::default() };
        for ev in self.listener.events.borrow_mut().drain(..) {
            match ev {
                TermEvent::Title(t) => w.title = Some(t),
                TermEvent::ResetTitle => w.title = Some(String::new()),
                TermEvent::Bell => w.bell = true,
                TermEvent::PtyWrite(s) => w.pty_write.extend_from_slice(s.as_bytes()),
                _ => {}
            }
        }
        self.scroll.clamp_max(self.history_size() as f32);
        w
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.term.resize(TermSize::new(cols.max(2), rows.max(1)));
        self.scroll.clamp_max(self.history_size() as f32);
    }

    pub fn history_size(&self) -> usize {
        self.term.grid().history_size()
    }

    pub fn columns(&self) -> usize {
        self.term.columns()
    }

    pub fn screen_lines(&self) -> usize {
        self.term.screen_lines()
    }

    pub fn app_cursor(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }

    pub fn bracketed_paste(&self) -> bool {
        self.term.mode().contains(TermMode::BRACKETED_PASTE)
    }

    /// Moves alacritty's integer display offset to `off` (clamped to history).
    pub fn set_display_offset(&mut self, off: usize) {
        let cur = self.term.grid().display_offset() as i32;
        let want = off.min(self.history_size()) as i32;
        if want != cur {
            self.term.scroll_display(Scroll::Delta(want - cur));
        }
    }

    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    /// Cursor as (col, viewport line), or None when scrolled so it is off screen.
    pub fn cursor(&self) -> Option<(usize, usize)> {
        let content = self.term.renderable_content();
        let p = content.cursor.point;
        let line = p.line.0 + content.display_offset as i32;
        if line < 0 || line >= self.term.screen_lines() as i32 {
            return None;
        }
        Some((p.column.0, line as usize))
    }

    fn style_of(&self, cell: &alacritty_terminal::term::cell::Cell) -> CellStyle {
        let flags = cell.flags;
        CellStyle {
            fg: self.palette.resolve(&cell.fg),
            bg: self.palette.resolve(&cell.bg),
            bold: flags.intersects(Flags::BOLD),
            italic: flags.intersects(Flags::ITALIC),
            dim: flags.intersects(Flags::DIM),
            underline: flags.intersects(Flags::ALL_UNDERLINES),
            inverse: flags.intersects(Flags::INVERSE),
        }
    }

    fn row_cells(&self, line: Line) -> Vec<(char, CellStyle, CellKind)> {
        let row = &self.term.grid()[line];
        let cols = self.term.columns();
        (0..cols)
            .map(|c| {
                let cell = &row[alacritty_terminal::index::Column(c)];
                let kind = if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                    CellKind::Spacer
                } else if cell.flags.intersects(Flags::WIDE_CHAR) {
                    CellKind::Wide
                } else {
                    CellKind::Normal
                };
                let c = if cell.flags.intersects(Flags::HIDDEN) { ' ' } else { cell.c };
                (c, self.style_of(cell), kind)
            })
            .collect()
    }

    pub fn rows_for_paint(&self) -> RowsForPaint {
        let off = self.display_offset() as i32;
        let screen = self.term.screen_lines() as i32;
        let visible = (0..screen).map(|i| self.row_cells(Line(i - off))).collect();
        let above = if off < self.history_size() as i32 { Some(self.row_cells(Line(-off - 1))) } else { None };
        RowsForPaint { display_offset: off as usize, above, visible }
    }
}
```

- [ ] **Step 4: Run the core tests**

Run: `cargo test -p sarkos terminal 2>&1 | tail -12`
Expected: `8 passed`. Watch for two likely compile fixes: `Term::columns`/`screen_lines` come from the `Dimensions` trait (already imported); the `Row` index type is `alacritty_terminal::index::Column`.

- [ ] **Step 5: Add the GPUI entity**

Append below `TerminalCore`'s impl (above the tests):
```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellMetrics {
    pub width: f32,
    pub height: f32,
}

pub struct Terminal {
    pub core: TerminalCore,
    pub session: SessionId,
    pub client: Arc<Client>,
    pub focus: FocusHandle,
    pub title: String,
    pub alive: bool,
    pub cell: Option<CellMetrics>,
    pub bell_pending: bool,
    /// When the element last painted, for time-based scroll animation.
    pub last_frame: Option<Instant>,
}

impl Terminal {
    pub fn new(core: TerminalCore, session: SessionId, client: Arc<Client>, cx: &mut Context<Self>) -> Self {
        Self { core, session, client, focus: cx.focus_handle(), title: String::new(), alive: true, cell: None, bell_pending: false, last_frame: None }
    }

    /// Applies a daemon event. Returns true when a repaint is needed.
    pub fn handle_event(&mut self, ev: &Event, cx: &mut Context<Self>) -> bool {
        match ev {
            Event::Output { session, bytes } if *session == self.session => {
                let w = self.core.apply_output(bytes);
                if let Some(t) = w.title {
                    self.title = t;
                }
                if w.bell {
                    self.bell_pending = true;
                }
                if !w.pty_write.is_empty() {
                    self.client.send(Request::Input { session: self.session, bytes: w.pty_write });
                }
                cx.notify();
                true
            }
            Event::Exited { session, .. } if *session == self.session => {
                self.alive = false;
                cx.notify();
                true
            }
            _ => false,
        }
    }

    pub fn handle_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) {
        // Shift+PageUp/PageDown scroll locally; everything else goes to the shell.
        if ks.modifiers.shift && (ks.key == "pageup" || ks.key == "pagedown") {
            let page = self.core.screen_lines() as f32;
            self.scroll_lines(if ks.key == "pageup" { page } else { -page }, cx);
            return;
        }
        if let Some(bytes) = keystroke_to_bytes(ks, self.core.app_cursor()) {
            self.client.send(Request::Input { session: self.session, bytes });
            // Typing jumps back to the live bottom, as every terminal does.
            self.core.scroll.jump_to(0.0);
            self.core.set_display_offset(0);
            cx.notify();
        }
    }

    pub fn paste(&mut self, text: &str) {
        let mut bytes = Vec::with_capacity(text.len() + 12);
        if self.core.bracketed_paste() {
            bytes.extend_from_slice(b"\x1b[200~");
            bytes.extend_from_slice(text.as_bytes());
            bytes.extend_from_slice(b"\x1b[201~");
        } else {
            bytes.extend_from_slice(text.as_bytes());
        }
        self.client.send(Request::Input { session: self.session, bytes });
    }

    /// Positive lines scroll up into history.
    pub fn scroll_lines(&mut self, lines: f32, cx: &mut Context<Self>) {
        self.core.scroll.scroll_by(lines, self.core.history_size() as f32);
        cx.notify();
    }

    /// Resizes the grid and the PTY when the pane's cell grid changes.
    pub fn ensure_size(&mut self, cols: usize, rows: usize) {
        if cols == self.core.columns() && rows == self.core.screen_lines() {
            return;
        }
        self.core.resize(cols, rows);
        self.client.send(Request::Resize { session: self.session, rows: rows as u16, cols: cols as u16 });
    }
}
```

- [ ] **Step 6: Build**

Run: `cargo build -p sarkos 2>&1 | grep -E "^(error|warning: unused)" | head`
Expected: no errors. Unused warnings for `Terminal` are fine until Task 10 wires it in.

- [ ] **Step 7: Commit**

```bash
git add crates/sarkos/src/terminal.rs crates/sarkos/src/main.rs
git commit -m "sarkos: terminal core over alacritty_terminal with a GPUI entity wrapper"
```

---

### Task 10: TerminalElement and the live window

**Files:**
- Create: `crates/sarkos/src/terminal_element.rs`
- Modify: `crates/sarkos/src/main.rs`

**Interfaces:**
- Consumes: `Terminal`, `TerminalCore::rows_for_paint`, `layout::build_runs`, `ScrollAnimator`, gpui `Element`.
- Produces: `TerminalElement::new(terminal: Entity<Terminal>) -> TerminalElement`, which implements `Element` and `IntoElement`; `Root` renders it full-window, tracks focus, forwards keys and wheel events, runs the heartbeat, and creates or attaches one session on launch (attach logic completed in Task 11).

Painting cannot be unit tested headless in gpui 0.2.2 without its test-support harness, so this task ends with a manual checklist. The pure parts were tested in Tasks 7 to 9.

- [ ] **Step 1: Write the element**

Create `crates/sarkos/src/terminal_element.rs`:
```rust
//! Paints a `Terminal`'s visible rows at a fractional line offset (spec 3). Only the rows in the
//! viewport plus one above are shaped and painted, which is the virtualization.
use crate::layout::{build_runs, CellKind, CellStyle};
use crate::palette::{Rgb, NAVY};
use crate::terminal::{CellMetrics, Terminal};
use gpui::{
    fill, font, point, px, size, App, Bounds, ContentMask, Element, ElementId, Entity, FontWeight, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, LayoutId, Pixels, Point, Rgba, SharedString, Style, TextRun, Window,
};
use std::time::Instant;

pub const FONT_FAMILY: &str = "Menlo";
pub const FONT_SIZE: f32 = 16.0;
pub const LINE_HEIGHT: f32 = 1.5;

pub struct TerminalElement {
    terminal: Entity<Terminal>,
}

impl TerminalElement {
    pub fn new(terminal: Entity<Terminal>) -> Self {
        Self { terminal }
    }
}

fn hsla(c: Rgb) -> Hsla {
    Rgba { r: c.r as f32 / 255.0, g: c.g as f32 / 255.0, b: c.b as f32 / 255.0, a: 1.0 }.into()
}

fn text_run(text: &str, style: &CellStyle) -> TextRun {
    let mut f = font(FONT_FAMILY);
    if style.bold {
        f.weight = FontWeight::BOLD;
    }
    if style.italic {
        f.style = gpui::FontStyle::Italic;
    }
    let mut color = hsla(style.fg);
    if style.dim {
        color.a = 0.6;
    }
    TextRun { len: text.len(), font: f, color, background_color: None, underline: None, strikethrough: None }
}

/// Measures the cell grid once per window scale. `W` is the widest ASCII glyph in a monospace font.
fn measure_cell(window: &mut Window) -> CellMetrics {
    let run = TextRun { len: 1, font: font(FONT_FAMILY), color: gpui::white(), background_color: None, underline: None, strikethrough: None };
    let layout = window.text_system().layout_line("W", px(FONT_SIZE), &[run], None);
    CellMetrics { width: f32::from(layout.width), height: FONT_SIZE * LINE_HEIGHT }
}

impl IntoElement for TerminalElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

pub struct Prepaint {
    cell: CellMetrics,
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = gpui::relative(1.0).into();
        style.size.height = gpui::relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _state: &mut (), window: &mut Window, cx: &mut App) -> Prepaint {
        let cell = measure_cell(window);
        let cols = (f32::from(bounds.size.width) / cell.width).floor().max(2.0) as usize;
        let rows = (f32::from(bounds.size.height) / cell.height).floor().max(1.0) as usize;
        self.terminal.update(cx, |t, _| {
            t.cell = Some(cell);
            t.ensure_size(cols, rows);
        });
        Prepaint { cell }
    }

    fn paint(&mut self, _id: Option<&GlobalElementId>, _inspector: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _state: &mut (), prepaint: &mut Prepaint, window: &mut Window, cx: &mut App) {
        let cell = prepaint.cell;
        let lh = px(cell.height);
        let cw = cell.width;
        window.paint_quad(fill(bounds, hsla(NAVY.bg)));

        // Advance the scroll animation by wall-clock time, then split into integer offset + fraction.
        let (rows, frac, cursor, animating) = self.terminal.update(cx, |t, _| {
            let now = Instant::now();
            let dt = t.last_frame.map(|l| now.duration_since(l).as_secs_f32()).unwrap_or(0.016).min(0.1);
            t.last_frame = Some(now);
            let animating = t.core.scroll.tick(dt);
            let off = t.core.scroll.current.floor().max(0.0) as usize;
            t.core.set_display_offset(off);
            let frac = t.core.scroll.current - off as f32;
            (t.core.rows_for_paint(), frac, t.core.cursor(), animating)
        });

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            let origin = bounds.origin;
            let paint_row = |cells: &Vec<(char, CellStyle, CellKind)>, y: Pixels, window: &mut Window, cx: &mut App| {
                let runs = build_runs(cells.iter().copied());
                for run in &runs {
                    let x = origin.x + px(run.col as f32 * cw);
                    let w = px(if run.wide { 2.0 * cw } else { run.text.chars().count() as f32 * cw });
                    if run.style.bg != NAVY.bg {
                        window.paint_quad(fill(Bounds { origin: point(x, y), size: size(w, lh) }, hsla(run.style.bg)));
                    }
                    if run.text.trim().is_empty() {
                        continue;
                    }
                    let text: SharedString = run.text.clone().into();
                    let line = window.text_system().shape_line(text, px(FONT_SIZE), &[text_run(&run.text, &run.style)], None);
                    let _ = line.paint(point(x, y), lh, window, cx);
                    if run.style.underline {
                        let uy = y + lh - px(2.0);
                        window.paint_quad(fill(Bounds { origin: point(x, uy), size: size(w, px(1.0)) }, hsla(run.style.fg)));
                    }
                }
            };
            if let Some(above) = &rows.above {
                let y = origin.y + px((frac - 1.0) * cell.height);
                paint_row(above, y, window, cx);
            }
            for (i, row) in rows.visible.iter().enumerate() {
                let y = origin.y + px((i as f32 + frac) * cell.height);
                paint_row(row, y, window, cx);
            }
            if let Some((col, line)) = cursor {
                if frac == 0.0 {
                    let x = origin.x + px(col as f32 * cw);
                    let y = origin.y + px(line as f32 * cell.height);
                    let mut c = hsla(NAVY.cursor);
                    c.a = 0.85;
                    window.paint_quad(fill(Bounds { origin: point(x, y), size: size(px(cw), lh) }, c));
                }
            }
        });

        if animating {
            window.request_animation_frame();
        }
    }
}
```

- [ ] **Step 2: Wire the Root view**

Replace `crates/sarkos/src/main.rs`:
```rust
mod client;
mod keys;
mod layout;
mod palette;
mod paths;
mod scroll;
mod terminal;
mod terminal_element;
mod workspace;

use futures::StreamExt;
use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, Entity, KeyDownEvent, ScrollDelta,
    ScrollWheelEvent, Window, WindowBounds, WindowOptions,
};
use protocol::{Request, Response, ServerMessage, SessionId};
use std::sync::Arc;
use std::time::Duration;
use terminal::{Terminal, TerminalCore};
use terminal_element::TerminalElement;

const BG: u32 = 0x11161f;
const SCROLLBACK: usize = 20_000;

struct Root {
    client: Arc<client::Client>,
    terminal: Option<Entity<Terminal>>,
    status: String,
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(terminal) = self.terminal.clone() else {
            return div().size_full().bg(rgb(BG)).text_color(rgb(0xd6dae0)).p_4().child(self.status.clone());
        };
        let focus = terminal.read(cx).focus.clone();
        if !focus.is_focused(window) {
            window.focus(&focus);
        }
        div()
            .size_full()
            .bg(rgb(BG))
            .track_focus(&focus)
            .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, _window, cx| {
                let Some(t) = this.terminal.clone() else { return };
                let ks = &ev.keystroke;
                if ks.modifiers.platform && ks.key == "v" {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        t.update(cx, |t, _| t.paste(&text));
                    }
                    return;
                }
                t.update(cx, |t, cx| t.handle_key(ks, cx));
            }))
            .on_scroll_wheel(cx.listener(move |this, ev: &ScrollWheelEvent, _window, cx| {
                let Some(t) = this.terminal.clone() else { return };
                t.update(cx, |t, cx| {
                    let cell_h = t.cell.map(|c| c.height).unwrap_or(24.0);
                    // Positive wheel y means the content moves down, which is scrolling up into history.
                    let lines = match ev.delta {
                        ScrollDelta::Lines(p) => p.y,
                        ScrollDelta::Pixels(p) => f32::from(p.y) / cell_h,
                    };
                    // If the manual check in Task 10 step 6 shows inverted scrolling, negate `lines` here.
                    t.scroll_lines(lines, cx);
                });
            }))
            .child(TerminalElement::new(terminal))
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") {
        println!("sarkos {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let (client, mut events) = match client::Client::connect_or_start(&paths::socket_path(), &paths::daemon_exe()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("sarkos: cannot reach sarkosd: {e:#}");
            std::process::exit(1);
        }
    };
    let client = Arc::new(client);
    let project = std::env::current_dir().expect("cwd");

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let client_for_view = client.clone();
        let window = cx
            .open_window(
                WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
                move |_, cx| cx.new(|_| Root { client: client_for_view, terminal: None, status: "connecting to sarkosd".into() }),
            )
            .expect("open window");
        let root = window.entity(cx).expect("root entity");

        // Heartbeat from the main thread, once a second (spec 2.4).
        let hb_client = client.clone();
        cx.spawn(async move |cx| {
            let mut seq = 0u64;
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                hb_client.send(Request::Heartbeat { seq });
                seq += 1;
            }
        })
        .detach();

        // Decide which session to show: the workspace's saved one if the daemon still has it.
        let list_id = client.send(Request::ListSessions);
        let saved = workspace::load(&project);
        let mut pending_create: Option<u32> = None;
        let mut pending_attach: Option<(u32, SessionId)> = None;
        let project_for_task = project.clone();
        let client_for_task = client.clone();
        cx.spawn(async move |cx| {
            while let Some(msg) = events.next().await {
                let step = root.update(cx, |root, cx| {
                    match &msg {
                        ServerMessage::Response { id, response: Response::Sessions(list) } if *id == list_id => {
                            match saved.filter(|s| list.iter().any(|i| i.id == *s)) {
                                Some(sid) => {
                                    let aid = client_for_task.send(Request::Attach { session: sid });
                                    pending_attach = Some((aid, sid));
                                }
                                None => {
                                    let cid = client_for_task.send(Request::CreateSession {
                                        cwd: project_for_task.clone(),
                                        shell: None,
                                        env: vec![],
                                        nice: 0,
                                        rows: 24,
                                        cols: 80,
                                    });
                                    pending_create = Some(cid);
                                }
                            }
                        }
                        ServerMessage::Response { id, response: Response::SessionCreated { id: sid } } if Some(*id) == pending_create => {
                            pending_create = None;
                            workspace::save(&project_for_task, *sid);
                            let aid = client_for_task.send(Request::Attach { session: *sid });
                            pending_attach = Some((aid, *sid));
                        }
                        ServerMessage::Response { id, response: Response::Attached { replay, alive } } if pending_attach.map(|p| p.0) == Some(*id) => {
                            let (_, sid) = pending_attach.take().unwrap();
                            let mut core = TerminalCore::new(80, 24, SCROLLBACK);
                            core.apply_output(replay);
                            let client = root.client.clone();
                            let terminal = cx.new(|cx| {
                                let mut t = Terminal::new(core, sid, client, cx);
                                t.alive = *alive;
                                t
                            });
                            root.terminal = Some(terminal);
                            root.status = String::new();
                            cx.notify();
                        }
                        ServerMessage::Response { response: Response::Error(e), .. } => {
                            root.status = format!("sarkosd error: {e}");
                            cx.notify();
                        }
                        ServerMessage::Event(ev) => {
                            if let Some(t) = root.terminal.clone() {
                                t.update(cx, |t, cx| {
                                    t.handle_event(ev, cx);
                                });
                            }
                        }
                        _ => {}
                    }
                });
                if step.is_err() {
                    break;
                }
            }
        })
        .detach();
    });
}
```

- [ ] **Step 3: Write a minimal workspace module so this compiles (Task 11 tests it)**

Create `crates/sarkos/src/workspace.rs`:
```rust
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
```

- [ ] **Step 4: Build and fix API drift**

Run: `cargo build -p sarkos 2>&1 | grep -E "^error" -A 6 | head -60`
Expected: no errors. Likely drift points and their fixes, all verified against gpui 0.2.2 source:
- `item.text()` on `ClipboardItem`: if missing, use `item.entries().iter().find_map(|e| match e { gpui::ClipboardEntry::String(s) => Some(s.text().to_string()), _ => None })`.
- `window.entity(cx)`: if `WindowHandle` has no `entity`, replace with `window.update(cx, |_, _, cx| cx.entity()).expect("root entity")`.
- `gpui::relative(1.0).into()` for `style.size`: if the type does not convert, use `style.size.width = gpui::Length::Definite(gpui::DefiniteLength::Fraction(1.0))`.
- `f32::from(Pixels)`: if `From<Pixels> for f32` is missing, use `pixels.to_f64() as f32`.

- [ ] **Step 5: Manual check, part 1: it is a terminal**

Run: `cargo run -p sarkos` from any project folder.
Check, in order:
1. A navy window with a shell prompt in Menlo 16 px, line height visibly loose (1.5).
2. Type `ls -la` Enter: output appears, colours show, cursor block follows.
3. `printf '\e[1;31mbold red\e[0m \e[4munder\e[0m\n'`: bold red, then underlined.
4. `echo "日本語 and ✅ emoji"`: wide characters occupy two cells, emoji renders in colour.
5. Resize the window: the prompt reflows and `tput cols` matches the visible width.
6. Run `claude` (Claude Code), type a message, confirm the TUI renders its box drawing without gaps and responds to keys.

- [ ] **Step 6: Manual check, part 2: smooth scrolling and the flood**

1. `seq 1 5000`: output floods. The window stays responsive, the CPU for sarkos stays below one core in Activity Monitor, and the last line is `5000`.
2. Two-finger scroll up: the content glides with sub-line smoothness, no snapping. Scroll back down: it eases to the bottom.
3. Scroll up, then run `yes | head -n 1000` from a second terminal into this session's PTY (`echo text > /dev/ttysNNN`, where `tty` printed the device): the view does not jump down while scrolled up.
4. Shift+PageUp and Shift+PageDown page through history. Pressing any key jumps back to the bottom.

- [ ] **Step 7: Commit**

```bash
git add crates/sarkos
git commit -m "sarkos: paint the terminal with fractional smooth scrolling"
```

---

### Task 11: Workspace persistence and reattach after a kill

**Files:**
- Modify: `crates/sarkos/src/workspace.rs` (add tests)
- Test: inline; plus the manual kill test, which is the milestone's acceptance test

**Interfaces:**
- Consumes: `workspace::{load_from, save_to}`.
- Produces: the guarantee that killing the UI loses nothing.

- [ ] **Step 1: Write the failing tests**

Append to `workspace.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ws.json");
        assert_eq!(load_from(&file), None);
        save_to(&file, SessionId(42));
        assert_eq!(load_from(&file), Some(SessionId(42)));
    }

    #[test]
    fn corrupt_file_reads_as_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ws.json");
        std::fs::write(&file, "{not json").unwrap();
        assert_eq!(load_from(&file), None);
    }

    #[test]
    fn different_projects_get_different_files() {
        let a = crate::paths::workspace_file(std::path::Path::new("/tmp/a"));
        let b = crate::paths::workspace_file(std::path::Path::new("/tmp/b"));
        assert_ne!(a, b);
        assert!(a.to_string_lossy().contains("workspaces"));
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p sarkos workspace 2>&1 | tail -5`
Expected: `3 passed` (the implementation exists from Task 10; these pin it).

- [ ] **Step 3: Manual acceptance test: the kill**

1. `cargo run -p sarkos` in `~/Projects/ide-sarkos`. In the terminal run `claude` and start a short task, or simply `for i in $(seq 1 300); do echo line $i; sleep 0.05; done`.
2. While it runs, from another terminal: `pkill -9 -x sarkos`. The window vanishes.
3. `cargo run -p sarkos` again in the same folder. Expected: the same session reappears with its full scrollback, the loop is still running or has finished with all 300 lines, and typing works. `pgrep sarkosd` shows the same daemon pid as before.
4. Scroll up through the replayed history: smooth, complete.
5. Quit normally with Cmd+Q, relaunch: same session again.

- [ ] **Step 4: Manual acceptance test: the watchdog with the real UI**

1. With sarkos running, freeze it: `kill -STOP $(pgrep -x sarkos)`.
2. Wait 35 seconds. Expected: the daemon kills the frozen UI and relaunches it (watch `tail -f ~/Library/Application\ Support/sarkos/sarkosd.log` for the "silent for 30s, relaunching" line), and the relaunched window reattaches to the same session.

- [ ] **Step 5: Commit**

```bash
git add crates/sarkos/src/workspace.rs
git commit -m "sarkos: persist the session per project and reattach on launch"
```

---

### Task 12: QoS, release build, and README

**Files:**
- Modify: `crates/sarkos/src/main.rs`, `README.md`

**Interfaces:**
- Produces: the UI main thread at user-interactive QoS (spec 2.5); a documented way to build and run both binaries.

- [ ] **Step 1: Set the QoS at the top of main**

In `crates/sarkos/src/main.rs`, as the first statement of `main` after the `--version` check:
```rust
    // Spec 2.5: the UI thread is user-interactive so macOS schedules it ahead of the heavy work
    // Claude Code's children do.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0);
    }
```
If `libc` lacks `pthread_set_qos_class_self_np` (it has had it since 0.2.80), declare it:
```rust
extern "C" {
    fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
}
const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
```
and call `pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0)`.

- [ ] **Step 2: Verify the QoS took**

Run: `cargo run -p sarkos &` then `sleep 3 && ps -o pid,pri,comm -p $(pgrep -x sarkos)`; alternatively in Activity Monitor, the process should not show as "Background". Expected: it runs; no crash from the call.

- [ ] **Step 3: Release build and sizes**

Run: `cargo build --release 2>&1 | tail -2 && ls -la target/release/sarkos target/release/sarkosd && ps -o rss,comm -p $(pgrep -x sarkosd)`
Expected: both binaries exist. Note the daemon's RSS in the commit message; the spec's budget is under 15 MB and this is the first measurement.

- [ ] **Step 4: Update the README**

Replace the body of `README.md` under the first paragraph with:
```markdown
## Status

Milestone 1 of 6: the daemon and one terminal pane. See the spec for the rest.

## Build and run

```bash
cargo build --release
./target/release/sarkos        # starts sarkosd on demand, opens one terminal in the current folder
```

The daemon keeps running after the window closes; sessions survive the UI being killed. Logs:
`~/Library/Application Support/sarkos/sarkosd.log`. To stop everything: `pkill -x sarkosd`.

## Tests

```bash
cargo test
```

## If you get headaches

The defaults (16 px Menlo, line height 1.5, a bright grey on navy, no warm tint) follow
[Check your eyes before your theme](https://csarko.sh/research/check-your-eyes-before-your-theme).
The same research puts a comprehensive eye examination that tests convergence and small
astigmatism ahead of any setting. Not medical advice.

## License

GPL-3.0. Modified versions must be released under the same license. See [LICENSE](LICENSE).
```

- [ ] **Step 5: Commit**

```bash
git add crates/sarkos/src/main.rs README.md
git commit -m "sarkos: user-interactive QoS, README for milestone 1"
```

---

## Self-review notes

**Spec coverage for milestone 1.** Section 2.1 daemon, socket, replay buffer, robustness rules: Task 3. Section 2.2 UI reattach: Tasks 5, 10, 11. Section 2.3 protocol, including `SARKOS_SESSION_ID` and the `SetState` message (handled by the daemon now, sent by the hook CLI in milestone 3): Tasks 2, 3. Section 2.4 watchdog: Task 4 and the manual test in Task 11. Section 2.5 QoS and nice: Task 12 and Task 3's `setpriority`. Section 3 emulation, rendering, fractional scroll, frame pacing, text defaults, bracketed paste: Tasks 6 to 10. Box-drawing glyph stretching (section 3) is deferred to milestone 2 with the theme work, since the manual check in Task 10 step 5 decides whether Menlo's box glyphs already fill a 1.5 cell. Section 4 persistence: the milestone 1 subset in Task 11. Sections 5, 6, 8 are later milestones. Section 9 non-blocking: enforced by the thread-plus-channel design in Tasks 3 and 5.

**Deferred to milestone 2 deliberately:** mouse selection and copy, tab bar, title display, the bell badge, theme files, `Term` title in `SessionInfo`.
