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
