//! One pseudo-terminal, its shell, its replay buffer and its subscribers.
use crate::pty::{self, Pty};
use crate::ring::RingBuffer;
use anyhow::{Context, Result};
use protocol::{AgentRecord, ClaudeState, Event, Response, ServerMessage, SessionDetail, SessionId, SessionInfo};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{channel, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub type ClientId = u64;

/// Output bytes a client may have queued before the daemon treats it as hung and drops it. With
/// backpressure a live client stays far below this; it is the backstop. A UI is never dropped.
pub const CLIENT_PENDING_CAP: usize = 16 * 1024 * 1024;
/// A subscriber with more output than this queued or being written pauses its session's PTY.
pub const SUBSCRIBER_HIGH_WATER: usize = 4 * 1024 * 1024;
/// A paused PTY resumes once that subscriber's backlog falls to this.
pub const SUBSCRIBER_LOW_WATER: usize = 1024 * 1024;
/// A subscriber that keeps the PTY paused this long is dropped as stalled, unless it is a UI.
pub const SUBSCRIBER_STALL: Duration = Duration::from_secs(10);
/// A socket write to a client that is not a UI fails after this long, which drops the client.
pub const CLIENT_WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// How a session's PTY reader yields to slow subscribers.
#[derive(Clone, Copy, Debug)]
pub struct Backpressure {
    pub high_water: usize,
    pub low_water: usize,
    pub stall: Duration,
}

impl Default for Backpressure {
    fn default() -> Self {
        Self { high_water: SUBSCRIBER_HIGH_WATER, low_water: SUBSCRIBER_LOW_WATER, stall: SUBSCRIBER_STALL }
    }
}

/// One queued frame. Consecutive output for the same session is merged into one `Output` item,
/// so a flood becomes a few large frames instead of one frame per PTY read.
pub enum Item {
    Output { session: SessionId, bytes: Vec<u8> },
    Message(ServerMessage),
}

impl Item {
    pub fn into_message(self) -> ServerMessage {
        match self {
            Item::Output { session, bytes } => ServerMessage::Event(Event::Output { session, bytes }),
            Item::Message(m) => m,
        }
    }
}

struct Pending {
    items: VecDeque<Item>,
    /// Output bytes queued and not yet taken by the writer.
    bytes: usize,
    /// Output bytes the writer has taken and is still writing.
    in_flight: usize,
    closed: bool,
}

struct SinkInner {
    client: ClientId,
    cap: usize,
    /// Set once the client names its UI process in Hello. A UI is never dropped for being slow:
    /// a frozen one is the watchdog's to kill and relaunch (spec 2.4), and backpressure keeps its
    /// backlog near high water, so the memory it holds stays bounded.
    ui: AtomicBool,
    pending: Mutex<Pending>,
    /// Wakes the writer when something is queued.
    ready: Condvar,
    /// Wakes a paused PTY reader when the writer finishes a batch or the sink closes.
    drained: Condvar,
    stream: UnixStream,
}

/// The handoff to one client's writer thread. Sending never blocks: items are queued in order
/// and the writer drains them all at once. A client that is not a UI and whose pending output
/// passes the byte cap is not draining; its stream is shut down so its reader thread ends and it
/// is disconnected.
#[derive(Clone)]
pub struct ClientSink {
    inner: Arc<SinkInner>,
}

impl ClientSink {
    pub fn new(client: ClientId, stream: UnixStream, cap: usize) -> Self {
        let pending = Pending { items: VecDeque::new(), bytes: 0, in_flight: 0, closed: false };
        let inner = SinkInner {
            client,
            cap,
            ui: AtomicBool::new(false),
            pending: Mutex::new(pending),
            ready: Condvar::new(),
            drained: Condvar::new(),
            stream,
        };
        let sink = Self { inner: Arc::new(inner) };
        sink.set_ui(false);
        sink
    }

    /// Marks the client as a UI or not. A UI's socket writes wait as long as they must; anyone
    /// else's time out and drop the client.
    pub fn set_ui(&self, ui: bool) {
        self.inner.ui.store(ui, Ordering::Relaxed);
        let timeout = if ui { None } else { Some(CLIENT_WRITE_TIMEOUT) };
        // macOS refuses socket options once the peer has hung up, and then no write matters.
        let _ = self.inner.stream.set_write_timeout(timeout);
    }

    pub fn is_ui(&self) -> bool {
        self.inner.ui.load(Ordering::Relaxed)
    }

    /// Queues a response or a control event. Returns false when the client is gone.
    pub fn send(&self, msg: ServerMessage) -> bool {
        let mut p = self.inner.pending.lock().unwrap();
        if p.closed {
            return false;
        }
        p.items.push_back(Item::Message(msg));
        self.inner.ready.notify_one();
        true
    }

    /// Queues PTY output, merging it into the last item when that is output for the same
    /// session. Returns false when the client is gone or was just dropped for being too slow.
    pub fn send_output(&self, session: SessionId, bytes: &[u8]) -> bool {
        let mut p = self.inner.pending.lock().unwrap();
        if p.closed {
            return false;
        }
        if p.bytes + bytes.len() > self.inner.cap && !self.is_ui() {
            let pending = p.bytes + p.in_flight;
            p.closed = true;
            p.items.clear();
            p.bytes = 0;
            drop(p);
            eprintln!(
                "sarkosd: client {} dropped: pending cap exceeded, {} output bytes pending for session {} (cap {})",
                self.inner.client, pending + bytes.len(), session.0, self.inner.cap
            );
            let _ = self.inner.stream.shutdown(Shutdown::Both);
            self.inner.ready.notify_one();
            self.inner.drained.notify_all();
            return false;
        }
        p.bytes += bytes.len();
        match p.items.back_mut() {
            Some(Item::Output { session: s, bytes: b }) if *s == session => b.extend_from_slice(bytes),
            _ => p.items.push_back(Item::Output { session, bytes: bytes.to_vec() }),
        }
        self.inner.ready.notify_one();
        true
    }

    /// Waits until something is queued and takes all of it. None once the sink is closed and
    /// everything queued before the close has been taken.
    pub fn take(&self) -> Option<VecDeque<Item>> {
        let mut p = self.inner.pending.lock().unwrap();
        while p.items.is_empty() && !p.closed {
            p = self.inner.ready.wait(p).unwrap();
        }
        if p.items.is_empty() {
            return None;
        }
        p.in_flight += p.bytes;
        p.bytes = 0;
        Some(std::mem::take(&mut p.items))
    }

    /// Called by the writer after it has written everything `take` returned.
    pub fn written(&self) {
        self.inner.pending.lock().unwrap().in_flight = 0;
        self.inner.drained.notify_all();
    }

    /// Output bytes queued or being written: what a paused PTY waits on.
    pub fn backlog(&self) -> usize {
        let p = self.inner.pending.lock().unwrap();
        if p.closed { 0 } else { p.bytes + p.in_flight }
    }

    /// Waits up to `timeout` for the backlog to fall to `low` or the sink to close. Returns
    /// true when it did.
    pub fn wait_drained(&self, low: usize, timeout: Duration) -> bool {
        let p = self.inner.pending.lock().unwrap();
        let (p, _) = self
            .inner
            .drained
            .wait_timeout_while(p, timeout, |p| !p.closed && p.bytes + p.in_flight > low)
            .unwrap();
        p.closed || p.bytes + p.in_flight <= low
    }

    /// Wakes a PTY reader paused on this sink so it can re-check why it is waiting.
    pub fn wake(&self) {
        self.inner.drained.notify_all();
    }

    /// Drops a client that kept its session's PTY paused too long.
    pub fn drop_stalled(&self, stall: Duration) {
        let pending = {
            let mut p = self.inner.pending.lock().unwrap();
            if p.closed {
                return;
            }
            let pending = p.bytes + p.in_flight;
            p.closed = true;
            p.items.clear();
            p.bytes = 0;
            pending
        };
        eprintln!(
            "sarkosd: client {} dropped: stalled above high water for {} s, {pending} output bytes pending",
            self.inner.client,
            stall.as_secs_f32()
        );
        let _ = self.inner.stream.shutdown(Shutdown::Both);
        self.inner.ready.notify_one();
        self.inner.drained.notify_all();
    }

    /// Refuses further sends; the writer thread drains what is already queued, then exits.
    pub fn close(&self) {
        self.inner.pending.lock().unwrap().closed = true;
        self.inner.ready.notify_one();
        self.inner.drained.notify_all();
    }
}

/// What the flusher writes: the bytes since its last offset, or the whole ring.
#[derive(Debug, PartialEq, Eq)]
pub enum ReplayDelta {
    Append { total: u64, bytes: Vec<u8> },
    Rewrite { total: u64, all: Vec<u8> },
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
    pub backpressure: Backpressure,
    /// Bytes the ring starts with: the history a recovered session shows under its divider.
    pub seed: Vec<u8>,
    /// Recreated by `--recover` after a restart.
    pub recovered: bool,
    /// The agent the session starts with, if any.
    pub agent: Option<AgentRecord>,
}

/// The shell's pid and, once reaped, its exit code. A reaped pid may be reused by any other
/// process, so every signal and every wait goes through here and nothing touches the pid after
/// `exited` is set. Held by pid, not `std::process::Child`, so it survives the handover's exec.
struct ChildSlot {
    pid: libc::pid_t,
    exited: Option<i32>,
    /// When `kill` sent SIGHUP; the reaper escalates to SIGKILL if the shell ignores it.
    kill_requested_at: Option<Instant>,
}

impl ChildSlot {
    /// Non-blocking. Reaps the child if it has exited and remembers the code.
    fn try_reap(&mut self) -> Option<i32> {
        if self.exited.is_none() {
            match pty::wait_nohang(self.pid) {
                Some(-1) => self.exited = Some(-1), // not our child any more: treat as gone
                Some(code) => self.exited = Some(code),
                None => {}
            }
        }
        self.exited
    }
}

pub struct Session {
    pub id: SessionId,
    cwd: PathBuf,
    /// Shared with the reader and writer threads. No lock: each call goes straight to the
    /// kernel, only the reader reads and only the writer writes. The upgrade makes it
    /// inheritable for the exec.
    pub master: Arc<Pty>,
    /// The last size set, which the handover records.
    size: Mutex<(u16, u16)>,
    input: Mutex<Sender<Vec<u8>>>,
    child: Mutex<ChildSlot>,
    pid: Option<u32>,
    state: Mutex<SessionState>,
    backpressure: Backpressure,
    /// Set by `kill`: the PTY reader stops pausing so it can drain to EOF.
    ending: AtomicBool,
    /// Set by `pause`: the reader stops between reads until `unpause`.
    paused: AtomicBool,
    /// Set by the reader once it has stopped for `pause`, and when there is no reader left.
    parked: AtomicBool,
    /// What a restart needs to recreate the shell (update-in-place spec, section 11). Read by
    /// the handover and the manifest.
    shell: String,
    nice: i32,
    /// The caller's extra variables only, not `TERM` and the session id.
    env: Vec<(String, String)>,
    /// The agent inside, from its SessionStart hook; its SessionEnd hook clears it. A leaf lock:
    /// never taken while `state` or `child` is held, and neither of those is taken under it.
    agent: Mutex<Option<AgentRecord>>,
    recovered: bool,
}

struct SessionState {
    replay: RingBuffer,
    subscribers: HashMap<ClientId, ClientSink>,
    alive: bool,
    exit_code: Option<i32>,
    exited_at: Option<Instant>,
    last_activity_unix_ms: u64,
    claude: ClaudeState,
    /// When output first arrived after the last flush to disk; `None` while the file is up to date.
    dirty_since: Option<Instant>,
    last_output: Instant,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl Session {
    pub fn spawn(spec: SpawnSpec) -> Result<Arc<Session>> {
        Self::spawn_inner(spec, true)
    }

    #[cfg(test)]
    pub(crate) fn spawn_for_test(spec: SpawnSpec, start_reader: bool) -> Result<Arc<Session>> {
        Self::spawn_inner(spec, start_reader)
    }

    fn spawn_inner(spec: SpawnSpec, start_reader: bool) -> Result<Arc<Session>> {
        let (master, slave) = Pty::open(spec.rows, spec.cols).context("openpty")?;
        let shell = spec.shell.clone().or_else(|| std::env::var("SHELL").ok()).unwrap_or_else(|| "/bin/zsh".to_string());
        let args: Vec<&str> = if shell.ends_with("zsh") || shell.ends_with("bash") { vec!["-l"] } else { vec![] };
        let mut env = vec![
            ("TERM".to_string(), "xterm-256color".to_string()),
            ("COLORTERM".to_string(), "truecolor".to_string()),
            ("SARKOS_SESSION_ID".to_string(), spec.id.0.to_string()),
        ];
        env.extend(spec.env.iter().cloned());
        let pid = pty::spawn(slave, &shell, &args, &spec.cwd, &env).context("spawn shell")?;
        if spec.nice != 0 {
            // Children of the shell inherit the priority (spec 2.5).
            // SAFETY: setpriority takes plain integers; the pid is our unreaped child.
            unsafe { libc::setpriority(libc::PRIO_PROCESS, pid as libc::id_t, spec.nice) };
        }
        let master = Arc::new(master);
        let (input_tx, input_rx) = channel::<Vec<u8>>();
        Self::start_writer(spec.id, master.clone(), input_rx);
        let session = Arc::new(Session {
            id: spec.id,
            cwd: spec.cwd,
            master,
            size: Mutex::new((spec.rows, spec.cols)),
            input: Mutex::new(input_tx),
            child: Mutex::new(ChildSlot { pid, exited: None, kill_requested_at: None }),
            pid: Some(pid as u32),
            state: Mutex::new(SessionState {
                replay: RingBuffer::from_bytes(spec.replay_capacity, &spec.seed),
                subscribers: HashMap::new(),
                alive: true,
                exit_code: None,
                exited_at: None,
                last_activity_unix_ms: now_ms(),
                claude: ClaudeState::Idle,
                dirty_since: (!spec.seed.is_empty()).then(Instant::now),
                last_output: Instant::now(),
            }),
            backpressure: spec.backpressure,
            ending: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            parked: AtomicBool::new(false),
            shell,
            nice: spec.nice,
            env: spec.env,
            agent: Mutex::new(spec.agent),
            recovered: spec.recovered,
        });
        if start_reader {
            Self::start_reader(session.clone());
        } else {
            // Tests only: drain and discard output like the real reader, but never record the
            // exit, so the test can drive `poll_exit` itself.
            let master = session.master.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match master.read_ready(Duration::from_millis(50)) {
                        Ok(false) => continue,
                        Ok(true) => {}
                        Err(_) => break,
                    }
                    if !matches!(master.read(&mut buf), Ok(n) if n > 0) {
                        break;
                    }
                }
            });
        }
        Ok(session)
    }

    /// Reads PTY output until EOF, when the shell has exited: record the exit and tell
    /// subscribers. It waits for output in 50 ms polls rather than one blocking read, so `pause`
    /// can park it between reads.
    fn start_reader(session: Arc<Session>) {
        std::thread::Builder::new()
            .name(format!("pty-reader-{}", session.id.0))
            .spawn(move || {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    if session.paused.load(Ordering::SeqCst) {
                        session.parked.store(true, Ordering::SeqCst);
                        while session.paused.load(Ordering::SeqCst) {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        session.parked.store(false, Ordering::SeqCst);
                        continue;
                    }
                    match session.master.read_ready(Duration::from_millis(50)) {
                        Ok(false) => continue,
                        Ok(true) => {}
                        Err(_) => break,
                    }
                    match session.master.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            session.on_output(&buf[..n]);
                            session.yield_to_slow_subscribers();
                        }
                    }
                }
                let code = loop {
                    if let Some(code) = session.child.lock().unwrap().try_reap() {
                        break code;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                };
                session.mark_exited(Some(code));
                // No reader is left, so an upgrade has nothing to wait for. Set here rather than in
                // `mark_exited`: the reaper also calls that while a background job keeps the PTY
                // open and this thread reading.
                session.parked.store(true, Ordering::SeqCst);
            })
            .expect("spawn pty reader thread");
    }

    /// Owns the PTY's write side so a blocked write never stalls the dispatch path.
    fn start_writer(id: SessionId, master: Arc<Pty>, rx: std::sync::mpsc::Receiver<Vec<u8>>) {
        std::thread::Builder::new()
            .name(format!("pty-writer-{}", id.0))
            .spawn(move || {
                while let Ok(bytes) = rx.recv() {
                    if master.write_all(&bytes).is_err() {
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
        st.last_output = Instant::now();
        if st.dirty_since.is_none() {
            st.dirty_since = Some(st.last_output);
        }
        let id = self.id;
        st.subscribers.retain(|_, tx| tx.send_output(id, bytes));
    }

    /// Backpressure: while a subscriber's backlog is above high water, stop reading the PTY (so
    /// the kernel buffer fills and the program blocks, as in any terminal) until it falls to low
    /// water. A subscriber that keeps it paused past the stall timeout is dropped, unless it is a
    /// UI: that one is waited on until it drains, disconnects, or the watchdog kills it. Waits
    /// happen outside the state lock, so attach, resize and kill proceed while the reader is paused.
    fn yield_to_slow_subscribers(&self) {
        let bp = self.backpressure;
        let slow: Vec<ClientSink> = {
            let st = self.state.lock().unwrap();
            st.subscribers.values().filter(|s| s.backlog() > bp.high_water).cloned().collect()
        };
        for sink in slow {
            let started = Instant::now();
            loop {
                if sink.wait_drained(bp.low_water, Duration::from_millis(50)) || self.is_ending() || self.paused.load(Ordering::SeqCst) {
                    break;
                }
                if started.elapsed() >= bp.stall && !sink.is_ui() {
                    sink.drop_stalled(bp.stall);
                    break;
                }
            }
        }
    }

    /// True once the session is being killed or its shell has exited: no more pausing.
    fn is_ending(&self) -> bool {
        if self.ending.load(Ordering::Relaxed) {
            return true;
        }
        let exited = self.child.lock().unwrap().try_reap().is_some();
        if exited {
            self.ending.store(true, Ordering::Relaxed);
        }
        exited
    }

    /// Records the shell's exit once and tells every subscriber. The PTY reader calls this at
    /// EOF; the reaper calls it when the shell is gone but a background job still holds the PTY
    /// open, which would otherwise hide the exit until that job ends.
    fn mark_exited(&self, code: Option<i32>) {
        let mut st = self.state.lock().unwrap();
        if !st.alive {
            return;
        }
        st.alive = false;
        st.exit_code = code;
        st.exited_at = Some(Instant::now());
        let event = ServerMessage::Event(Event::Exited { session: self.id, code });
        st.subscribers.retain(|_, tx| tx.send(event.clone()));
    }

    /// Stops the reader between reads (the handover's snapshot must be the last word). Bytes the
    /// shell prints meanwhile wait in the kernel.
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    /// True once the reader has stopped and will not read until `unpause`.
    pub fn wait_parked(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while !self.parked.load(Ordering::SeqCst) {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        true
    }

    pub fn unpause(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    /// Non-blocking exit check for the reaper thread.
    pub fn poll_exit(&self) {
        let code = self.child.lock().unwrap().try_reap();
        if let Some(code) = code {
            self.mark_exited(Some(code));
        }
    }

    pub fn exited_at(&self) -> Option<Instant> {
        self.state.lock().unwrap().exited_at
    }

    pub fn has_subscribers(&self) -> bool {
        !self.state.lock().unwrap().subscribers.is_empty()
    }

    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        self.input.lock().unwrap().send(bytes.to_vec()).map_err(|_| anyhow::anyhow!("session input closed"))
    }

    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        let mut size = self.size.lock().unwrap();
        self.master.resize(rows, cols).context("resize pty")?;
        *size = (rows, cols);
        Ok(())
    }

    /// The last `rows, cols` set.
    pub fn size(&self) -> (u16, u16) {
        *self.size.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Ends the shell. The exit check and the signal happen under the same lock, so a child that
    /// exits in between is reaped here and never signalled by its possibly reused pid.
    pub fn kill(&self) -> Result<()> {
        let mut slot = self.child.lock().unwrap();
        if slot.try_reap().is_some() {
            return Ok(());
        }
        // SIGHUP, as a closing terminal would send; SIGKILL follows from `escalate_kill` if the
        // shell ignores it.
        // SAFETY: the pid is unreaped (the slot has not reaped it and nothing else waits on it),
        // so it still names our child.
        let killed = if unsafe { libc::kill(slot.pid, libc::SIGHUP) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error()).context("kill child")
        };
        slot.kill_requested_at = Some(Instant::now());
        drop(slot);
        self.ending.store(true, Ordering::Relaxed);
        // Wake a reader paused on a slow subscriber so it drains to EOF instead of waiting.
        let st = self.state.lock().unwrap();
        for sink in st.subscribers.values() {
            sink.wake();
        }
        killed
    }

    /// Sends SIGKILL if `kill` asked for SIGHUP over a second ago and the shell is still running.
    pub fn escalate_kill(&self) {
        let mut slot = self.child.lock().unwrap();
        let Some(at) = slot.kill_requested_at else { return };
        if at.elapsed() < Duration::from_secs(1) || slot.try_reap().is_some() {
            return;
        }
        // SAFETY: as in `kill`, the slot has not reaped the pid, so it is still our child.
        unsafe { libc::kill(slot.pid, libc::SIGKILL) };
        // Once per request: the reaper sees the exit next pass.
        slot.kill_requested_at = None;
        eprintln!("sarkosd: session {} did not exit after SIGHUP, sent SIGKILL", self.id.0);
    }

    pub fn set_state(&self, state: ClaudeState) {
        let mut st = self.state.lock().unwrap();
        st.claude = state;
        let event = ServerMessage::Event(Event::State { session: self.id, state });
        st.subscribers.retain(|_, tx| tx.send(event.clone()));
    }

    /// Queues the `Attached` response (replay plus liveness) for request `id` and registers the
    /// subscriber, atomically with respect to new output: nothing is missed or duplicated between
    /// the replay and the live stream, and no live output reaches the client before the response.
    pub fn subscribe(&self, client: ClientId, tx: ClientSink, id: u32) {
        let mut st = self.state.lock().unwrap();
        let response = Response::Attached { replay: st.replay.snapshot(), alive: st.alive };
        if tx.send(ServerMessage::Response { id, response }) {
            st.subscribers.insert(client, tx);
        }
    }

    pub fn unsubscribe(&self, client: ClientId) {
        self.state.lock().unwrap().subscribers.remove(&client);
    }

    /// The record the handover file carries for this session. The master fd number is the one
    /// in this process; the caller makes the descriptor inheritable before the exec.
    pub fn snapshot(&self) -> crate::handover::SessionRecord {
        // A leaf lock, taken and released before `state`.
        let agent = self.agent();
        let st = self.state.lock().unwrap();
        let slot = self.child.lock().unwrap();
        let (rows, cols) = self.size();
        crate::handover::SessionRecord {
            id: self.id.0,
            cwd: self.cwd.clone(),
            pid: slot.pid,
            // The slot may have reaped the shell (in `kill`, `is_ending` or `poll_exit`) before
            // the state records it; the record must not call a reaped pid alive.
            alive: st.alive && slot.exited.is_none(),
            exit_code: st.exit_code.or(slot.exited),
            exited_ago_ms: st.exited_at.map(|at| at.elapsed().as_millis() as u64).or(slot.exited.map(|_| 0)),
            claude: st.claude,
            last_activity_unix_ms: st.last_activity_unix_ms,
            master_fd: self.master.raw_fd(),
            rows,
            cols,
            kill_requested: slot.kill_requested_at.is_some(),
            replay: st.replay.snapshot(),
            shell: self.shell.clone(),
            nice: self.nice,
            env: self.env.clone(),
            agent,
            recovered: self.recovered,
        }
    }

    /// Rebuilds a session around an inherited master after `--resume`. The shell is still this
    /// process's child (the pid never changed), so `try_reap` works; a shell that exited during
    /// the swap is caught here.
    pub fn adopt(record: &crate::handover::SessionRecord, master: Pty, replay_capacity: usize, backpressure: Backpressure) -> Arc<Session> {
        let master = Arc::new(master);
        let (input_tx, input_rx) = channel::<Vec<u8>>();
        Self::start_writer(SessionId(record.id), master.clone(), input_rx);
        // An exit older than this process's clock can express is clamped to now: the reaper's
        // grace restarts rather than the subtraction panicking.
        let exited_at = record
            .exited_ago_ms
            .map(|ms| Instant::now().checked_sub(Duration::from_millis(ms)).unwrap_or_else(Instant::now));
        let session = Arc::new(Session {
            id: SessionId(record.id),
            cwd: record.cwd.clone(),
            master,
            size: Mutex::new((record.rows, record.cols)),
            input: Mutex::new(input_tx),
            child: Mutex::new(ChildSlot {
                pid: record.pid,
                exited: if record.alive { None } else { Some(record.exit_code.unwrap_or(-1)) },
                kill_requested_at: record.kill_requested.then(Instant::now),
            }),
            pid: Some(record.pid as u32),
            state: Mutex::new(SessionState {
                replay: RingBuffer::from_bytes(replay_capacity, &record.replay),
                subscribers: HashMap::new(),
                alive: record.alive,
                exit_code: record.exit_code,
                exited_at,
                last_activity_unix_ms: record.last_activity_unix_ms,
                claude: record.claude,
                // The new image rewrites the file once.
                dirty_since: Some(Instant::now()),
                last_output: Instant::now(),
            }),
            backpressure,
            // A pending kill drains to EOF without pausing, as it did in the old image; so does a
            // dead record's reader, which never pauses on a slow subscriber.
            ending: AtomicBool::new(!record.alive || record.kill_requested),
            paused: AtomicBool::new(false),
            // Set by the reader when it parks for an upgrade or stops at EOF.
            parked: AtomicBool::new(false),
            shell: record.shell.clone(),
            nice: record.nice,
            env: record.env.clone(),
            agent: Mutex::new(record.agent.clone()),
            recovered: record.recovered,
        });
        if record.alive {
            // The reader reports EOF later; a shell gone during the swap is recorded now.
            session.poll_exit();
        }
        // A dead record gets a reader too: a background job still holding the slave would block
        // on a full buffer until the TTL otherwise. At EOF `try_reap` returns the recorded code at
        // once and `mark_exited` does nothing.
        Self::start_reader(session.clone());
        session
    }

    /// The state lock, taken even when a panic elsewhere poisoned it: the ring is still bytes and
    /// the flags still say what they said, so the flusher and the manifest read on.
    fn state_even_poisoned(&self) -> std::sync::MutexGuard<'_, SessionState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn info(&self) -> SessionInfo {
        let st = self.state_even_poisoned();
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

    pub fn shell(&self) -> &str {
        &self.shell
    }

    pub fn nice(&self) -> i32 {
        self.nice
    }

    pub fn env(&self) -> &[(String, String)] {
        &self.env
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn set_agent(&self, agent: AgentRecord) {
        *self.agent.lock().unwrap() = Some(agent);
    }

    /// Clears the record when it is `id`'s. A SessionEnd hook that arrives after the next
    /// SessionStart (a `/clear` ends one session and starts another) must not erase the new one.
    pub fn clear_agent(&self, id: &str) -> bool {
        let mut a = self.agent.lock().unwrap();
        match &*a {
            Some(rec) if rec.id == id => {
                *a = None;
                true
            }
            _ => false,
        }
    }

    pub fn agent(&self) -> Option<AgentRecord> {
        self.agent.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// `info` releases the state lock before `agent` takes its own, so the two never nest.
    pub fn detail(&self) -> SessionDetail {
        SessionDetail { info: self.info(), recovered: self.recovered, agent: self.agent() }
    }

    pub fn replay_capacity(&self) -> usize {
        self.state_even_poisoned().replay.capacity()
    }

    /// Unflushed output that stopped `quiet` ago, or has been flowing for `max_wait`: a chatty
    /// session is still written every `max_wait`.
    pub fn flush_due(&self, quiet: Duration, max_wait: Duration) -> bool {
        let st = self.state_even_poisoned();
        match st.dirty_since {
            None => false,
            Some(since) => st.last_output.elapsed() >= quiet || since.elapsed() >= max_wait,
        }
    }

    /// The bytes since stream position `from`, or the whole ring when `from` is `None` or the
    /// ring dropped some of them. Clears the dirty mark under the same lock, so output arriving
    /// after this call makes the session dirty again.
    pub fn take_delta(&self, from: Option<u64>) -> ReplayDelta {
        let mut st = self.state_even_poisoned();
        st.dirty_since = None;
        let total = st.replay.total();
        match from.and_then(|f| st.replay.since(f)) {
            Some(bytes) => ReplayDelta::Append { total, bytes },
            None => ReplayDelta::Rewrite { total, all: st.replay.snapshot() },
        }
    }
}

#[cfg(test)]
impl Session {
    /// Poisons the state lock the way a panic in the reader would.
    pub(crate) fn poison_state_for_test(self: &Arc<Self>) {
        let s = self.clone();
        let _ = std::thread::spawn(move || {
            let _held = s.state.lock().unwrap();
            panic!("deliberate panic to poison session {}'s state", s.id.0);
        })
        .join();
        assert!(self.state.is_poisoned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::codec::read_frame;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    #[test]
    fn poll_exit_records_the_exit_once_without_a_reader() {
        let spec = SpawnSpec {
            id: SessionId(7),
            cwd: std::env::temp_dir(),
            shell: Some("/bin/sh".into()),
            env: vec![],
            nice: 0,
            rows: 24,
            cols: 80,
            replay_capacity: 1024,
            backpressure: Backpressure::default(),
            seed: Vec::new(),
            recovered: false,
            agent: None,
        };
        let s = Session::spawn_for_test(spec, false).unwrap();
        // Input typed before the shell sets up its terminal can be flushed away; give it a moment.
        std::thread::sleep(Duration::from_millis(500));
        s.write(b"exit 3\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while s.child.lock().unwrap().try_reap() != Some(3) {
            assert!(Instant::now() < deadline, "shell never exited: {:?}", s.child.lock().unwrap().try_reap());
            std::thread::sleep(Duration::from_millis(10));
        }
        let (a, _b) = UnixStream::pair().unwrap();
        let sink = ClientSink::new(1, a, 1 << 20);
        s.subscribe(1, sink.clone(), 1);
        s.poll_exit();
        s.poll_exit();
        assert!(!s.info().alive);
        assert_eq!(s.state.lock().unwrap().exit_code, Some(3));
        assert!(s.exited_at().is_some());
        sink.close();
        let mut exits = 0;
        // The writer thread is absent here, so drain the sink the way it would and write frames.
        let mut out = Vec::new();
        while let Some(items) = sink.take() {
            for item in items {
                protocol::codec::write_frame(&mut out, &item.into_message()).unwrap();
            }
        }
        let mut cur = std::io::Cursor::new(out);
        while let Some(m) = read_frame::<_, ServerMessage>(&mut cur).unwrap() {
            if let ServerMessage::Event(Event::Exited { code: Some(3), .. }) = m {
                exits += 1;
            }
        }
        assert_eq!(exits, 1);
    }

    #[test]
    fn snapshot_records_the_session_and_adopt_rebuilds_it() {
        let spec = SpawnSpec {
            id: SessionId(11),
            cwd: std::env::temp_dir(),
            shell: Some("/bin/sh".into()),
            env: vec![("SARKOS_TEST".into(), "1".into())],
            nice: 0,
            rows: 24,
            cols: 80,
            replay_capacity: 4096,
            backpressure: Backpressure::default(),
            seed: Vec::new(),
            recovered: true,
            agent: None,
        };
        let s = Session::spawn(spec).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        s.write(b"echo snap-$((1+1))\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !String::from_utf8_lossy(&s.state.lock().unwrap().replay.snapshot()).contains("snap-2") {
            assert!(Instant::now() < deadline, "no output");
            std::thread::sleep(Duration::from_millis(20));
        }
        s.resize(30, 100).unwrap();
        let agent = AgentRecord { kind: "claude".into(), id: "a1".into(), resume: vec!["claude".into(), "--resume".into(), "a1".into()] };
        s.set_agent(agent.clone());
        let rec = s.snapshot();
        assert_eq!(rec.id, 11);
        assert_eq!(rec.agent, Some(agent.clone()));
        assert!(rec.recovered);
        assert_eq!((rec.shell.as_str(), rec.nice), ("/bin/sh", 0));
        assert_eq!(rec.env, vec![("SARKOS_TEST".to_string(), "1".to_string())]);
        assert!(rec.alive);
        assert_eq!((rec.rows, rec.cols), (30, 100));
        assert_eq!(rec.pid, s.info().pid.unwrap() as i32);
        assert!(String::from_utf8_lossy(&rec.replay).contains("snap-2"));
        assert_eq!(rec.master_fd, s.master.raw_fd());
        // Hand the master over as the exec would: make it inheritable and adopt a copy of the
        // number. The old image is gone after a real exec; here its reader would keep taking
        // output from the shared terminal, so its descriptor is replaced with /dev/null, where
        // that reader sees EOF and stops. The spawn lock is held while any of these descriptors
        // lacks close-on-exec, so a shell spawned by a concurrent test cannot inherit one.
        let devnull = std::fs::File::open("/dev/null").unwrap();
        let no_fork = pty::spawn_lock();
        s.master.set_inheritable(true).unwrap();
        let raw = rec.master_fd;
        // SAFETY: dup on an open descriptor this test's session owns.
        let dup = unsafe { libc::dup(raw) };
        assert!(dup >= 0);
        // SAFETY: `dup` is a fresh descriptor nothing else holds; the adopted `Pty` owns it.
        let master = unsafe { Pty::adopt(dup) }.unwrap();
        // SAFETY: dup2 onto `raw`, which the old session's `Pty` still owns and closes on drop;
        // only the file behind the number changes.
        assert!(unsafe { libc::dup2(devnull.as_raw_fd(), raw) } >= 0);
        s.master.set_inheritable(false).unwrap();
        drop(no_fork);
        // After a real exec the old image cannot reap the shell; holding its slot keeps the old
        // reader, now at EOF, from reaping it here, so the adopted session must.
        let _old_image_gone = s.child.lock().unwrap();
        let adopted = Session::adopt(&rec, master, 4096, Backpressure::default());
        assert!(adopted.info().alive);
        assert_eq!(adopted.info().pid, s.info().pid);
        assert_eq!(adopted.size(), (30, 100));
        assert_eq!(adopted.agent(), Some(agent));
        assert!(adopted.detail().recovered);
        assert_eq!((adopted.shell(), adopted.nice(), adopted.env()), (rec.shell.as_str(), rec.nice, rec.env.as_slice()));
        assert!(String::from_utf8_lossy(&adopted.state.lock().unwrap().replay.snapshot()).contains("snap-2"));
        std::thread::sleep(Duration::from_millis(300));
        adopted.write(b"echo after-adopt-$((2+2))\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !String::from_utf8_lossy(&adopted.state.lock().unwrap().replay.snapshot()).contains("after-adopt-4") {
            assert!(Instant::now() < deadline, "the adopted session does not read");
            std::thread::sleep(Duration::from_millis(20));
        }
        adopted.kill().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while adopted.info().alive {
            assert!(Instant::now() < deadline, "the adopted session never saw the exit");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(adopted.state.lock().unwrap().exit_code, Some(129), "128 + SIGHUP, reaped by the adopted session");
    }

    #[test]
    fn adopt_keeps_a_dead_record_dead_and_carries_its_exit_age() {
        let (master, _slave) = Pty::open(24, 80).unwrap();
        let mut rec = crate::handover::SessionRecord {
            id: 12,
            cwd: std::env::temp_dir(),
            pid: i32::MAX, // never signalled or waited on: the record says it was reaped
            alive: false,
            exit_code: Some(3),
            exited_ago_ms: Some(60_000),
            claude: ClaudeState::Idle,
            last_activity_unix_ms: 5,
            master_fd: master.raw_fd(),
            rows: 24,
            cols: 80,
            kill_requested: false,
            replay: b"0123456789".to_vec(),
            shell: String::new(),
            nice: 0,
            env: vec![],
            agent: None,
            recovered: false,
        };
        let s = Session::adopt(&rec, master, 4, Backpressure::default());
        assert!(!s.info().alive);
        assert_eq!(s.state.lock().unwrap().exit_code, Some(3));
        assert!(s.exited_at().unwrap().elapsed() >= Duration::from_secs(60), "the reaper's grace carries over");
        assert_eq!(s.state.lock().unwrap().replay.snapshot(), b"6789");
        s.kill().unwrap();
        assert_eq!(s.child.lock().unwrap().try_reap(), Some(3));
        // An age no clock can subtract still adopts.
        rec.exited_ago_ms = Some(u64::MAX);
        let (master, _slave) = Pty::open(24, 80).unwrap();
        assert!(Session::adopt(&rec, master, 4, Backpressure::default()).exited_at().is_some());
    }

    #[test]
    fn a_dead_record_drains_a_background_job_and_parks_at_eof() {
        use std::io::Write;
        let (master, slave) = Pty::open(24, 80).unwrap();
        let rec = crate::handover::SessionRecord {
            id: 14,
            cwd: std::env::temp_dir(),
            pid: i32::MAX, // never signalled or waited on: the record says it was reaped
            alive: false,
            exit_code: Some(0),
            exited_ago_ms: Some(0),
            claude: ClaudeState::Idle,
            last_activity_unix_ms: 0,
            master_fd: master.raw_fd(),
            rows: 24,
            cols: 80,
            kill_requested: false,
            replay: vec![],
            shell: String::new(),
            nice: 0,
            env: vec![],
            agent: None,
            recovered: false,
        };
        let s = Session::adopt(&rec, master, 1 << 20, Backpressure::default());
        // A background job that outlived the shell still holds the slave and writes far more
        // than the kernel buffers: with nothing reading, it would block until the TTL.
        let (done_tx, done_rx) = channel();
        std::thread::spawn(move || {
            let mut job = std::fs::File::from(slave);
            job.write_all(&vec![b'x'; 256 * 1024]).unwrap();
            drop(job);
            done_tx.send(()).unwrap();
        });
        done_rx.recv_timeout(Duration::from_secs(5)).expect("the job blocked: nothing reads the dead record");
        assert!(s.wait_parked(Duration::from_secs(5)), "the reader parks at EOF");
        assert!(!s.info().alive);
        assert_eq!(s.state.lock().unwrap().exit_code, Some(0));
        assert_eq!(s.state.lock().unwrap().replay.snapshot().len(), 256 * 1024);
    }

    #[test]
    fn snapshot_records_an_exit_the_slot_reaped_before_the_state_knew() {
        let spec = SpawnSpec {
            id: SessionId(13),
            cwd: std::env::temp_dir(),
            shell: Some("/bin/sh".into()),
            env: vec![],
            nice: 0,
            rows: 24,
            cols: 80,
            replay_capacity: 1024,
            backpressure: Backpressure::default(),
            seed: Vec::new(),
            recovered: false,
            agent: None,
        };
        // No reader, so nothing but this test reaps or records the exit.
        let s = Session::spawn_for_test(spec, false).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        s.write(b"exit 3\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while s.child.lock().unwrap().try_reap() != Some(3) {
            assert!(Instant::now() < deadline, "shell never exited");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(s.info().alive, "the state has not recorded the exit yet");
        let rec = s.snapshot();
        assert!(!rec.alive);
        assert_eq!(rec.exit_code, Some(3));
        assert_eq!(rec.exited_ago_ms, Some(0));
    }

    #[test]
    fn the_agent_record_is_set_cleared_by_its_own_id_and_reported_in_detail() {
        let dir = tempfile::tempdir().unwrap();
        let s = Session::spawn_for_test(
            SpawnSpec {
                id: SessionId(7),
                cwd: dir.path().to_path_buf(),
                shell: Some("/bin/sh".into()),
                env: vec![("SARKOS_TEST".into(), "1".into())],
                nice: 0,
                rows: 24,
                cols: 80,
                replay_capacity: 1024,
                backpressure: Backpressure::default(),
                seed: b"old output".to_vec(),
                recovered: true,
                agent: None,
            },
            false,
        )
        .unwrap();
        assert_eq!(s.shell(), "/bin/sh");
        assert_eq!(s.nice(), 0);
        assert_eq!(s.env(), &[("SARKOS_TEST".to_string(), "1".to_string())]);
        assert_eq!(s.cwd(), dir.path());
        let d = s.detail();
        assert!(d.recovered);
        assert_eq!(d.agent, None);
        assert_eq!(d.info.id, SessionId(7));
        let agent = protocol::AgentRecord { kind: "claude".into(), id: "a1".into(), resume: vec!["claude".into(), "--resume".into(), "a1".into()] };
        s.set_agent(agent.clone());
        assert_eq!(s.agent(), Some(agent.clone()));
        assert!(!s.clear_agent("someone-else"), "another id's SessionEnd leaves the record");
        assert_eq!(s.agent(), Some(agent));
        assert!(s.clear_agent("a1"));
        assert_eq!(s.detail().agent, None);
        // The seed is the ring's first content.
        assert_eq!(s.snapshot().replay, b"old output");
        let _ = s.kill();
    }

    #[test]
    fn output_makes_a_session_due_for_a_flush_and_take_delta_clears_it() {
        let dir = tempfile::tempdir().unwrap();
        let spec = SpawnSpec {
            id: SessionId(3),
            cwd: dir.path().to_path_buf(),
            shell: Some("/bin/sh".into()),
            env: vec![],
            nice: 0,
            rows: 24,
            cols: 80,
            replay_capacity: 8,
            backpressure: Backpressure::default(),
            seed: Vec::new(),
            recovered: false,
            agent: None,
        };
        let s = Session::spawn_for_test(spec, false).unwrap();
        assert_eq!(s.replay_capacity(), 8);
        assert!(!s.flush_due(Duration::ZERO, Duration::ZERO), "nothing was ever printed");
        s.on_output(b"abc");
        assert!(!s.flush_due(Duration::from_secs(60), Duration::from_secs(60)), "output is recent and the wait is young");
        assert!(s.flush_due(Duration::ZERO, Duration::from_secs(60)), "quiet for long enough");
        assert!(matches!(s.take_delta(None), ReplayDelta::Rewrite { total: 3, all } if all == b"abc"));
        assert!(!s.flush_due(Duration::ZERO, Duration::ZERO), "flushed");
        s.on_output(b"de");
        assert!(matches!(s.take_delta(Some(3)), ReplayDelta::Append { total: 5, bytes } if bytes == b"de"));
        s.on_output(b"0123456789");
        assert!(matches!(s.take_delta(Some(5)), ReplayDelta::Rewrite { total: 15, all } if all == b"23456789"), "the ring dropped unflushed bytes");
        assert!(matches!(s.take_delta(Some(15)), ReplayDelta::Append { total: 15, bytes } if bytes.is_empty()));
        let _ = s.kill();
    }

    #[test]
    fn a_seeded_ring_is_due_at_once_so_a_recovered_history_is_written_again() {
        let dir = tempfile::tempdir().unwrap();
        let spec = SpawnSpec {
            id: SessionId(4),
            cwd: dir.path().to_path_buf(),
            shell: Some("/bin/sh".into()),
            env: vec![],
            nice: 0,
            rows: 24,
            cols: 80,
            replay_capacity: 64,
            backpressure: Backpressure::default(),
            seed: b"old".to_vec(),
            recovered: true,
            agent: None,
        };
        let s = Session::spawn_for_test(spec, false).unwrap();
        assert!(s.flush_due(Duration::ZERO, Duration::from_secs(60)));
        let _ = s.kill();
    }
}
