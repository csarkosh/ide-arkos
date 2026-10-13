//! One pseudo-terminal, its shell, its replay buffer and its subscribers.
use crate::ring::RingBuffer;
use anyhow::{Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use protocol::{ClaudeState, Event, ServerMessage, SessionId, SessionInfo};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{channel, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub type ClientId = u64;

/// Frames a client may have queued before the daemon treats it as hung and drops it.
pub const CLIENT_QUEUE: usize = 256;

/// A bounded handoff to one client's writer thread. A full queue means the client is not
/// draining; the stream is shut down so its reader thread ends and the client is disconnected.
#[derive(Clone)]
pub struct ClientSink {
    tx: SyncSender<ServerMessage>,
    stream: Arc<UnixStream>,
}

impl ClientSink {
    pub fn new(tx: SyncSender<ServerMessage>, stream: UnixStream) -> Self {
        Self { tx, stream: Arc::new(stream) }
    }

    /// Returns false when the client is gone or was just dropped for being too slow.
    pub fn send(&self, msg: ServerMessage) -> bool {
        match self.tx.try_send(msg) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                let _ = self.stream.shutdown(Shutdown::Both);
                false
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }
}

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
    input: Mutex<Sender<Vec<u8>>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    pid: Option<u32>,
    state: Mutex<SessionState>,
}

struct SessionState {
    replay: RingBuffer,
    subscribers: HashMap<ClientId, ClientSink>,
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
        let (input_tx, input_rx) = channel::<Vec<u8>>();
        Self::start_writer(spec.id, writer, input_rx);
        let session = Arc::new(Session {
            id: spec.id,
            cwd: spec.cwd,
            master: Mutex::new(pair.master),
            input: Mutex::new(input_tx),
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
                st.subscribers.retain(|_, tx| tx.send(event.clone()));
            })
            .expect("spawn pty reader thread");
    }

    /// Owns the PTY's write side so a blocked write never stalls the dispatch path.
    fn start_writer(id: SessionId, mut writer: Box<dyn Write + Send>, rx: std::sync::mpsc::Receiver<Vec<u8>>) {
        std::thread::Builder::new()
            .name(format!("pty-writer-{}", id.0))
            .spawn(move || {
                while let Ok(bytes) = rx.recv() {
                    if writer.write_all(&bytes).and_then(|()| writer.flush()).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn pty writer thread");
    }

    fn on_output(&self, bytes: &[u8]) {
        let mut st = self.state.lock().unwrap();
        st.replay.push(bytes);
        st.last_activity_unix_ms = now_ms();
        let event = ServerMessage::Event(Event::Output { session: self.id, bytes: bytes.to_vec() });
        st.subscribers.retain(|_, tx| tx.send(event.clone()));
    }

    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        self.input.lock().unwrap().send(bytes.to_vec()).map_err(|_| anyhow::anyhow!("session input closed"))
    }

    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        self.master
            .lock()
            .unwrap()
            .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .context("resize pty")
    }

    pub fn kill(&self) -> Result<()> {
        // Never signal a pid that may already be reaped and reused.
        if !self.state.lock().unwrap().alive {
            return Ok(());
        }
        self.child.lock().unwrap().kill().context("kill child")
    }

    pub fn set_state(&self, state: ClaudeState) {
        let mut st = self.state.lock().unwrap();
        st.claude = state;
        let event = ServerMessage::Event(Event::State { session: self.id, state });
        st.subscribers.retain(|_, tx| tx.send(event.clone()));
    }

    /// Registers a subscriber and returns the replay buffer plus liveness, atomically with respect
    /// to new output, so nothing is missed or duplicated between the replay and the live stream.
    pub fn subscribe(&self, client: ClientId, tx: ClientSink) -> (Vec<u8>, bool) {
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
