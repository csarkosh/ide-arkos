//! Accepts clients on the Unix socket, one reader and one writer thread each, and dispatches
//! requests to sessions. Shared state is one mutex; every operation under it is short.
use crate::handover::Handover;
use crate::session::{Backpressure, ClientId, ClientSink, Session, SpawnSpec};
use anyhow::{Context, Result};
use protocol::codec::{read_frame, write_frame};
use protocol::{ClientMessage, Request, Response, ServerMessage, SessionId, PROTOCOL_MIN, PROTOCOL_VERSION};
use std::collections::HashMap;
use std::io::{BufReader, BufWriter};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::net::Shutdown;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct Options {
    pub replay_capacity: usize,
    pub watchdog: Duration,
    /// Output bytes a client may have pending before it is dropped as hung.
    pub client_pending_cap: usize,
    pub backpressure: Backpressure,
    /// How often the reaper polls for exits and dead sessions.
    pub reaper_every: Duration,
    /// How long a dead session stays listed after it ended, while nobody is attached.
    pub dead_session_ttl: Duration,
    /// The listening socket's path; the handover file is written next to it.
    pub socket: PathBuf,
}

pub struct ClientRecord {
    #[allow(dead_code)] // kept so the watchdog can reach a client
    pub tx: ClientSink,
    pub attached: Vec<SessionId>,
    pub last_heartbeat: Instant,
    pub ui_pid: Option<u32>,
    pub ui_exe: Option<PathBuf>,
    /// Where the watchdog relaunches the UI, so it reopens the same project.
    pub ui_cwd: Option<PathBuf>,
    pub connected: bool,
    /// The protocol version its Hello announced; 0 until then.
    pub protocol: u32,
}

#[derive(Default)]
pub struct State {
    pub sessions: HashMap<SessionId, Arc<Session>>,
    pub clients: HashMap<ClientId, ClientRecord>,
    next_session: u64,
    next_client: u64,
    /// The listening socket, which an upgrade passes through the exec.
    pub listener_fd: RawFd,
    /// Set while an upgrade is being checked or carried out; a second one is refused.
    pub upgrading: bool,
}

impl State {
    /// Session ids start from the clock (milliseconds since the epoch, shifted left 16 bits), so
    /// ids stay unique across daemon lifetimes: a workspace saved before a restart cannot name
    /// another project's new session.
    fn new() -> Self {
        let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        Self { next_session: ms << 16, ..Default::default() }
    }

    /// The last session id handed out, which the handover carries over.
    pub fn next_session(&self) -> u64 {
        self.next_session
    }
}

pub type Shared = Arc<Mutex<State>>;

pub fn run(opts: Options) -> Result<()> {
    let socket = opts.socket.as_path();
    let parent = socket.parent().ok_or_else(|| anyhow::anyhow!("socket path has no parent"))?;
    let created_parent = !parent.exists();
    std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    if created_parent {
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).ok();
    }
    match std::fs::symlink_metadata(socket) {
        Ok(meta) if meta.file_type().is_socket() => {
            // A previous daemon died. Nobody can be listening if connect fails.
            if UnixStream::connect(socket).is_ok() {
                anyhow::bail!("another sarkosd is already listening on {}", socket.display());
            }
            std::fs::remove_file(socket).with_context(|| format!("remove stale socket {}", socket.display()))?;
        }
        Ok(_) => anyhow::bail!("{} exists and is not a socket; refusing to replace it", socket.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("inspect {}", socket.display())),
    }
    let listener = UnixListener::bind(socket).with_context(|| format!("bind {}", socket.display()))?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600)).context("chmod socket")?;
    let mut st = State::new();
    st.listener_fd = listener.as_raw_fd();
    serve(listener, Arc::new(Mutex::new(st)), opts)
}

/// The new image after an upgrade: adopt the listener and every session, then serve.
pub fn run_resumed(h: Handover, socket: PathBuf) -> Result<()> {
    use std::os::fd::FromRawFd;
    crate::pty::set_inheritable_fd(h.listener_fd, false).context("listener close-on-exec")?;
    // SAFETY: the descriptor was passed through execve by the old image for this purpose, and
    // nothing else in this process owns the number.
    let listener = unsafe { UnixListener::from_raw_fd(h.listener_fd) };
    let opts = Options {
        replay_capacity: h.replay_capacity,
        watchdog: Duration::from_secs(h.watchdog_secs),
        client_pending_cap: h.client_pending_cap,
        backpressure: Backpressure { high_water: h.high_water, low_water: h.low_water, stall: Duration::from_secs(h.stall_secs) },
        reaper_every: Duration::from_millis(h.reaper_millis.max(10)),
        dead_session_ttl: Duration::from_secs(h.dead_session_secs),
        socket,
    };
    let mut st = State::new();
    st.next_session = h.next_session;
    st.listener_fd = h.listener_fd;
    for rec in &h.sessions {
        // SAFETY: the old image cleared close-on-exec on this master for the exec and recorded
        // its number; it passed through execve for this purpose and nothing else owns it.
        let master = match unsafe { crate::pty::Pty::adopt(rec.master_fd) } {
            Ok(m) => m,
            Err(e) => {
                eprintln!("sarkosd: session {} lost in the handover: cannot adopt fd {}: {e}", rec.id, rec.master_fd);
                continue;
            }
        };
        let s = Session::adopt(rec, master, opts.replay_capacity, opts.backpressure);
        st.sessions.insert(SessionId(rec.id), s);
    }
    eprintln!("sarkosd: resumed {} sessions from a handover written by {}", st.sessions.len(), h.written_by);
    serve(listener, Arc::new(Mutex::new(st)), opts)
}

/// The watchdog, the reaper and the accept loop, shared by a fresh start and a resume.
fn serve(listener: UnixListener, state: Shared, opts: Options) -> Result<()> {
    crate::watchdog::start(state.clone(), opts.watchdog);
    crate::reaper::start(state.clone(), opts.reaper_every, opts.dead_session_ttl);
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => spawn_client(stream, state.clone(), opts.clone()),
            Err(e) => eprintln!("sarkosd: accept failed: {e}"),
        }
    }
    Ok(())
}

fn spawn_client(stream: UnixStream, state: Shared, opts: Options) {
    let (writer_stream, shutdown_stream) = match (stream.try_clone(), stream.try_clone()) {
        (Ok(a), Ok(b)) => (a, b),
        _ => {
            eprintln!("sarkosd: clone stream failed");
            return;
        }
    };
    let (client_id, sink) = {
        let mut st = state.lock().unwrap();
        st.next_client += 1;
        let id = st.next_client;
        // Not a UI until its Hello says so: writes time out (see `ClientSink::set_ui`).
        let sink = ClientSink::new(id, shutdown_stream, opts.client_pending_cap);
        st.clients.insert(
            id,
            ClientRecord { tx: sink.clone(), attached: Vec::new(), last_heartbeat: Instant::now(), ui_pid: None, ui_exe: None, ui_cwd: None, connected: true, protocol: 0 },
        );
        (id, sink)
    };
    let writer_sink = sink.clone();
    std::thread::Builder::new()
        .name(format!("client-writer-{client_id}"))
        .spawn(move || {
            let mut w = BufWriter::new(writer_stream);
            // Everything queued since the last pass is written in order; merged output means a
            // flood costs a few large frames rather than one per PTY read.
            'outer: while let Some(items) = writer_sink.take() {
                for item in items {
                    if let Err(e) = write_frame(&mut w, &item.into_message()) {
                        // Timed out (never for a UI) or broken: end the connection so the reader
                        // thread cleans up.
                        eprintln!(
                            "sarkosd: client {client_id} dropped: write failed ({e}), {} output bytes pending",
                            writer_sink.backlog()
                        );
                        let _ = w.get_ref().shutdown(Shutdown::Both);
                        writer_sink.close();
                        break 'outer;
                    }
                }
                // Lets a PTY paused on this client's backlog resume.
                writer_sink.written();
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
                        if let Request::Hello { protocol_version, .. } = msg.request {
                            if !(PROTOCOL_MIN..=PROTOCOL_VERSION).contains(&protocol_version) {
                                let error = format!(
                                    "protocol version mismatch: daemon speaks {PROTOCOL_MIN} to {PROTOCOL_VERSION}, client {protocol_version}; \
                                     update the daemon: run Update Sarkos IDE now"
                                );
                                eprintln!("sarkosd: client {client_id} dropped: {error}");
                                // Closing below lets the writer send this before the socket closes.
                                sink.send(ServerMessage::Response { id: msg.id, response: Response::Error(error) });
                                break;
                            }
                        }
                        // Attach queues its own response so it lands before any live output.
                        if let Some(response) = dispatch(&state, client_id, &sink, msg.id, msg.request, &opts) {
                            if !sink.send(ServerMessage::Response { id: msg.id, response }) {
                                break;
                            }
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
            sink.close();
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

/// What `CreateSession` and `Kill` answer while an upgrade holds the slot: a new session would
/// miss the handover, and a kill could reap a shell after its snapshot called it alive.
const UPGRADE_IN_PROGRESS: &str = "an upgrade is in progress; retry in a moment";

/// Handles one request. Returns the response to queue, or None when it was already queued.
fn dispatch(state: &Shared, client_id: ClientId, tx: &ClientSink, id: u32, request: Request, opts: &Options) -> Option<Response> {
    let session = |sid: SessionId| -> Option<Arc<Session>> { state.lock().unwrap().sessions.get(&sid).cloned() };
    let response = match request {
        Request::Hello { protocol_version, ui_pid, ui_exe, ui_cwd } => {
            let mut st = state.lock().unwrap();
            if let Some(rec) = st.clients.get_mut(&client_id) {
                // As a pid_t, 0 is the daemon's process group, 1 is launchd, and anything above
                // i32::MAX is negative, which kill() reads as a group or, for -1, every process.
                let ui_pid = (2..=i32::MAX as u32).contains(&ui_pid).then_some(ui_pid);
                rec.ui_pid = ui_pid;
                // A UI is never dropped for being slow; the watchdog alone ends it.
                tx.set_ui(ui_pid.is_some());
                let ui_exe = ui_exe.filter(|p| {
                    let named_sarkos = p.file_name().map(|n| n == "sarkos").unwrap_or(false);
                    let ok = named_sarkos && p.is_file();
                    if !ok {
                        eprintln!("sarkosd: client {client_id} offered ui_exe {} which is not a sarkos binary; it will not be relaunched", p.display());
                    }
                    ok
                });
                rec.ui_exe = ui_exe;
                rec.ui_cwd = (!ui_cwd.as_os_str().is_empty()).then_some(ui_cwd);
                rec.last_heartbeat = Instant::now();
                rec.protocol = protocol_version;
            }
            if protocol_version >= 2 {
                Response::Hello { daemon_version: crate::version_string(), protocol_min: PROTOCOL_MIN, protocol_max: PROTOCOL_VERSION }
            } else {
                Response::Ok
            }
        }
        Request::Upgrade { exe } => {
            // Claim the slot first: the check takes seconds, and a second client must be
            // refused for its whole length, not only after it passes.
            if let Err(e) = crate::upgrade::begin(state) {
                return Some(Response::Error(e));
            }
            if let Err(e) = crate::upgrade::validate(&exe) {
                crate::upgrade::abort(state);
                return Some(Response::Error(e));
            }
            // Returns only on failure; on success this process is now the new binary.
            Response::Error(crate::upgrade::execute(state, &exe, opts))
        }
        // Task 6 replaces this placeholder.
        Request::RelaunchClient { .. } => Response::Error("not yet".into()),
        Request::Heartbeat { .. } => {
            if let Some(rec) = state.lock().unwrap().clients.get_mut(&client_id) {
                rec.last_heartbeat = Instant::now();
            }
            Response::Ok
        }
        Request::CreateSession { cwd, shell, env, nice, rows, cols } => {
            let id = {
                let mut st = state.lock().unwrap();
                if st.upgrading {
                    return Some(Response::Error(UPGRADE_IN_PROGRESS.into()));
                }
                st.next_session += 1;
                SessionId(st.next_session)
            };
            match Session::spawn(SpawnSpec { id, cwd, shell, env, nice, rows, cols, replay_capacity: opts.replay_capacity, backpressure: opts.backpressure }) {
                Ok(s) => {
                    state.lock().unwrap().sessions.insert(id, s);
                    Response::SessionCreated { id }
                }
                Err(e) => Response::Error(format!("spawn failed: {e:#}")),
            }
        }
        Request::Attach { session: sid } => match session(sid) {
            Some(s) => {
                s.subscribe(client_id, tx.clone(), id);
                if let Some(rec) = state.lock().unwrap().clients.get_mut(&client_id) {
                    if !rec.attached.contains(&sid) {
                        rec.attached.push(sid);
                    }
                }
                return None;
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
        Request::Kill { .. } if state.lock().unwrap().upgrading => Response::Error(UPGRADE_IN_PROGRESS.into()),
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
    };
    Some(response)
}
