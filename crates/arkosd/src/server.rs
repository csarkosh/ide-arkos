//! Accepts clients on the Unix socket, one reader and one writer thread each, and dispatches
//! requests to sessions. Shared state is one mutex; every operation under it is short.
use crate::handover::Handover;
use crate::session::{Backpressure, ClientId, ClientSink, Session, SpawnSpec};
use anyhow::{Context, Result};
use protocol::codec::{read_frame, write_frame};
use protocol::{ClientMessage, Event, Request, Response, ServerMessage, SessionId, StagedUpdate, PROTOCOL_MIN, PROTOCOL_VERSION};
use std::collections::HashMap;
use std::io::{BufReader, BufWriter};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::net::Shutdown;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
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
    /// How long output must have stopped before a session's ring is written to disk.
    pub flush_quiet: Duration,
    /// Where and how often to look for a release; polling is off when `check_every` is zero.
    pub update: crate::updates::UpdateOptions,
}

pub struct ClientRecord {
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
    /// The window and directory `RelaunchClient` asked for, launched when this client goes.
    pub relaunch: Option<(PathBuf, PathBuf)>,
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
    /// Creates past their `upgrading` check whose session is not in `sessions` yet; an upgrade
    /// waits for none before it gathers the table.
    pub creating: usize,
    /// Project folders with a window open, and the window binary each said it runs: what a
    /// restart reopens (update-in-place spec, section 11 step 4). Fed by Hello, emptied by
    /// Goodbye; a window that vanishes without one (killed by a shutdown, or crashed) stays.
    pub windows: HashMap<PathBuf, Option<PathBuf>>,
    /// Set by every change the manifest records; the flusher writes it and clears this.
    pub manifest_dirty: bool,
    /// The poll thread's findings: what is staged and how the last check went.
    pub update: crate::updates::UpdateState,
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

    pub fn set_next_session(&mut self, v: u64) {
        self.next_session = v;
    }
}

pub type Shared = Arc<Mutex<State>>;

/// The first protocol whose windows decode `Event::UpdateStaged`.
const UPDATE_EVENTS_FROM: u32 = 4;

/// Queues `event` for every connected client that speaks protocol 4 or later: an older window
/// cannot decode the variant and would drop its connection, and a client that never said Hello
/// (protocol 0) is not a window. A client whose sink is closed is skipped; its reader thread is
/// already removing it.
pub fn broadcast(st: &State, event: Event) {
    for rec in st.clients.values().filter(|r| r.connected && r.protocol >= UPDATE_EVENTS_FROM) {
        rec.tx.send(ServerMessage::Event(event.clone()));
    }
}

/// How `run_with` fails when a daemon already serves the socket; `--recover` treats it as success.
pub const ALREADY_LISTENING: &str = "another arkosd is already listening on";

/// A fresh start. `recover`: rebuild the table from the manifest before serving (the launchd
/// agent's start).
pub fn run_with(opts: Options, recover: bool) -> Result<()> {
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
                anyhow::bail!("{ALREADY_LISTENING} {}", socket.display());
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
    let state: Shared = Arc::new(Mutex::new(st));
    if recover {
        let dir = crate::recovery::sessions_dir(&opts.socket);
        let done = crate::recovery::recover(&state, &dir, &opts);
        eprintln!("arkosd: recovered {} sessions and reopened {} windows", done.sessions, done.windows);
    }
    crate::updates::apply::on_start(&state, opts.update.bundle.as_deref(), &opts.update.dir);
    serve(listener, state, opts)
}

/// The new image after an upgrade: adopt the listener and every session, then serve.
pub fn run_resumed(h: Handover, socket: PathBuf, update: crate::updates::UpdateOptions) -> Result<()> {
    use std::os::fd::FromRawFd;
    // Before anything is adopted: the numbers come from a file.
    let records = crate::handover::adoptable(&h).map_err(anyhow::Error::msg)?;
    crate::pty::set_inheritable_fd(h.listener_fd, false).context("listener close-on-exec")?;
    // A master `Pty::adopt` fails on is closed by it; one never adopted is closed here.
    for fd in crate::handover::strays(&h, &records) {
        eprintln!("arkosd: closing fd {fd}, the master of a session lost in the handover");
        // SAFETY: the number came through the exec for a session nothing here adopts, so
        // nothing in this process owns it.
        unsafe { libc::close(fd) };
    }
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
        flush_quiet: Duration::from_millis(h.flush_quiet_millis),
        update,
    };
    let mut st = State::new();
    st.next_session = h.next_session;
    st.listener_fd = h.listener_fd;
    for rec in records {
        // SAFETY: the old image cleared close-on-exec on this master for the exec and recorded
        // its number; it passed through execve for this purpose and nothing else owns it.
        let master = match unsafe { crate::pty::Pty::adopt(rec.master_fd) } {
            Ok(m) => m,
            Err(e) => {
                eprintln!("arkosd: session {} lost in the handover: cannot adopt fd {}: {e}", rec.id, rec.master_fd);
                continue;
            }
        };
        let s = Session::adopt(rec, master, opts.replay_capacity, opts.backpressure);
        st.sessions.insert(SessionId(rec.id), s);
    }
    st.windows = h.windows.into_iter().collect();
    st.manifest_dirty = true;
    eprintln!("arkosd: resumed {} sessions from a handover written by {}", st.sessions.len(), h.written_by);
    let state = Arc::new(Mutex::new(st));
    crate::updates::apply::on_start(&state, opts.update.bundle.as_deref(), &opts.update.dir);
    serve(listener, state, opts)
}

/// The watchdog, the reaper, the flusher, the signal watcher, the update poll and the accept
/// loop, shared by a fresh start and a resume.
fn serve(listener: UnixListener, state: Shared, opts: Options) -> Result<()> {
    crate::watchdog::start(state.clone(), opts.watchdog);
    let u = &opts.update;
    if !u.check_every.is_zero() {
        let keys = protocol::release_key::RELEASE_PUBLIC_KEYS.iter().map(|k| k.to_string()).chain(u.extra_keys.iter().cloned()).collect();
        let fetcher = crate::updates::fetch::Fetcher { base: u.url.clone(), curl: u.curl.clone() };
        crate::updates::poll::start(state.clone(), fetcher, u.dir.clone(), keys, u.check_every, u.ditto.clone());
    }
    crate::reaper::start(state.clone(), opts.reaper_every, opts.dead_session_ttl);
    let dir = crate::recovery::sessions_dir(&opts.socket);
    let flusher = crate::recovery::start(state.clone(), dir, opts.flush_quiet);
    crate::recovery::watch_termination(state.clone(), flusher);
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => spawn_client(stream, state.clone(), opts.clone()),
            Err(e) => eprintln!("arkosd: accept failed: {e}"),
        }
    }
    Ok(())
}

fn spawn_client(stream: UnixStream, state: Shared, opts: Options) {
    let (writer_stream, shutdown_stream) = match (stream.try_clone(), stream.try_clone()) {
        (Ok(a), Ok(b)) => (a, b),
        _ => {
            eprintln!("arkosd: clone stream failed");
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
            ClientRecord { tx: sink.clone(), attached: Vec::new(), last_heartbeat: Instant::now(), ui_pid: None, ui_exe: None, ui_cwd: None, connected: true, protocol: 0, relaunch: None },
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
                            "arkosd: client {client_id} dropped: write failed ({e}), {} output bytes pending",
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
                                // The window's `connection::is_final` matches the prefix: a refused
                                // window stops reconnecting, so the hint names a fix that needs no
                                // connection.
                                let hint = if protocol_version > PROTOCOL_VERSION {
                                    "this arkosd is older than the window: run pkill -x arkosd (ends every session) and open Arkos IDE again"
                                } else {
                                    "this window is older than arkosd: quit it and open the installed Arkos IDE"
                                };
                                let error = format!(
                                    "protocol version mismatch: daemon speaks {PROTOCOL_MIN} to {PROTOCOL_VERSION}, client {protocol_version}; {hint}"
                                );
                                eprintln!("arkosd: client {client_id} dropped: {error}");
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
                        eprintln!("arkosd: client {client_id}: bad frame: {e}");
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
    let (relaunch, ui_exe, ui_cwd) = {
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
        match st.clients.remove(&client_id) {
            Some(rec) => (rec.relaunch, rec.ui_exe, rec.ui_cwd),
            None => return,
        }
    };
    // The lock is released: launching never blocks other threads.
    if let Some((exe, cwd)) = relaunch {
        if let Err(e) = crate::watchdog::launch_ui(&exe, Some(&cwd)) {
            eprintln!("arkosd: relaunch of {} failed: {e}; launching the previous window instead", exe.display());
            if let Some(old) = ui_exe {
                if let Err(e) = crate::watchdog::launch_ui(&old, ui_cwd.as_deref()) {
                    eprintln!("arkosd: relaunch of {} failed too: {e}", old.display());
                }
            }
        }
    }
}

/// Counts one create in `State::creating` from its `upgrading` check until it is dropped, after
/// the session is inserted or the spawn failed, so no return path can leak the count.
struct Creating<'a>(&'a Shared);

impl Drop for Creating<'_> {
    fn drop(&mut self) {
        // Also runs while unwinding, so a poisoned lock must not panic again.
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).creating -= 1;
    }
}

/// What `CreateSession` and `Kill` answer while an upgrade holds the slot: a new session would
/// miss the handover, and a kill could reap a shell after its snapshot called it alive.
const UPGRADE_IN_PROGRESS: &str = "an upgrade is in progress; retry in a moment";

/// A regular file named `arkos` owned by this user: the only binary the daemon launches as a
/// window, whoever named it.
pub fn is_arkos_binary(p: &Path) -> bool {
    let named = p.file_name().map(|n| n == "arkos").unwrap_or(false);
    // SAFETY: getuid takes no arguments and cannot fail.
    named && std::fs::metadata(p).map(|m| m.is_file() && m.uid() == unsafe { libc::getuid() }).unwrap_or(false)
}

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
                    let named_arkos = p.file_name().map(|n| n == "arkos").unwrap_or(false);
                    let ok = named_arkos && p.is_file();
                    if !ok {
                        eprintln!("arkosd: client {client_id} offered ui_exe {} which is not a arkos binary; it will not be relaunched", p.display());
                    }
                    ok
                });
                rec.ui_exe = ui_exe;
                rec.ui_cwd = (!ui_cwd.as_os_str().is_empty()).then_some(ui_cwd);
                rec.last_heartbeat = Instant::now();
                rec.protocol = protocol_version;
            }
            // A window came up: an applied update is confirmed (see `updates::apply::watch`).
            st.update.connected_since_start = true;
            let window = st.clients.get(&client_id).and_then(|r| r.ui_cwd.clone().map(|cwd| (cwd, r.ui_exe.clone())));
            if let Some((cwd, exe)) = window {
                st.windows.insert(cwd, exe);
                st.manifest_dirty = true;
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
            let _released_if_this_unwinds = crate::upgrade::SlotGuard(state);
            if let Err(e) = crate::upgrade::validate(&exe) {
                crate::upgrade::abort(state);
                return Some(Response::Error(e));
            }
            // Returns only on failure; on success this process is now the new binary.
            Response::Error(crate::upgrade::execute(state, &exe, opts))
        }
        Request::RelaunchClient { exe, cwd } => {
            if !is_arkos_binary(&exe) {
                Response::Error(format!("{} is not a arkos binary owned by this user", exe.display()))
            } else {
                if let Some(rec) = state.lock().unwrap().clients.get_mut(&client_id) {
                    rec.relaunch = Some((exe, cwd));
                }
                Response::Ok
            }
        }
        Request::Goodbye => {
            let mut st = state.lock().unwrap();
            if let Some(cwd) = st.clients.get(&client_id).and_then(|r| r.ui_cwd.clone()) {
                st.windows.remove(&cwd);
                st.manifest_dirty = true;
            }
            Response::Ok
        }
        Request::SetAgent { session: sid, agent } => match session(sid) {
            Some(s) => {
                s.set_agent(agent);
                state.lock().unwrap().manifest_dirty = true;
                Response::Ok
            }
            None => Response::Error(format!("no session {}", sid.0)),
        },
        Request::ClearAgent { session: sid, id: agent_id } => match session(sid) {
            Some(s) => {
                if s.clear_agent(&agent_id) {
                    state.lock().unwrap().manifest_dirty = true;
                }
                Response::Ok
            }
            None => Response::Error(format!("no session {}", sid.0)),
        },
        Request::ListSessionsDetailed => {
            let st = state.lock().unwrap();
            let mut list: Vec<_> = st.sessions.values().map(|s| s.detail()).collect();
            list.sort_by_key(|d| d.info.id.0);
            Response::SessionsDetailed(list)
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
                if st.upgrading {
                    return Some(Response::Error(UPGRADE_IN_PROGRESS.into()));
                }
                // Under the same lock as the check: an upgrade that begins after it waits for
                // this session to be in the table.
                st.creating += 1;
                st.next_session += 1;
                SessionId(st.next_session)
            };
            let _creating = Creating(state);
            let spec = SpawnSpec {
                id,
                cwd,
                shell,
                env,
                nice,
                rows,
                cols,
                replay_capacity: opts.replay_capacity,
                backpressure: opts.backpressure,
                seed: Vec::new(),
                recovered: false,
                agent: None,
            };
            match Session::spawn(spec) {
                Ok(s) => {
                    let mut st = state.lock().unwrap();
                    st.sessions.insert(id, s);
                    st.manifest_dirty = true;
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
            Some(s) => match {
                // Only the window's Input: the resume line `recover` types uses `write` directly.
                s.clear_recovered();
                s.write(&bytes)
            } {
                Ok(()) => Response::Ok,
                Err(e) => Response::Error(format!("write failed: {e:#}")),
            },
            None => Response::Error(format!("no session {}", sid.0)),
        },
        Request::Resize { session: sid, rows, cols } => match session(sid) {
            Some(s) => match s.resize(rows, cols) {
                Ok(()) => {
                    // The manifest records the size a restart recreates the session at.
                    state.lock().unwrap().manifest_dirty = true;
                    Response::Ok
                }
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
        Request::UpdateStatus => {
            let st = state.lock().unwrap();
            Response::UpdateStatus { staged: st.update.staged.as_ref().map(|s| StagedUpdate { version: s.version.clone(), git: s.git.clone() }), last_error: st.update.last_error.clone() }
        }
        Request::CheckForUpdates => {
            let mut st = state.lock().unwrap();
            if st.update.polling {
                st.update.check_now = true;
                Response::Ok
            } else {
                Response::Error("update checks are off".into())
            }
        }
        Request::ApplyUpdate { version } => {
            let (staged, pending, bundle, dir) = {
                let st = state.lock().unwrap();
                (st.update.staged.clone(), st.update.applied.as_ref().map(|a| a.version.clone()), opts.update.bundle.clone(), opts.update.dir.clone())
            };
            match (staged, bundle) {
                (None, _) => Response::Error("nothing staged".into()),
                (Some(s), _) if s.version != version => Response::Error(format!("staged {} is not {version}", s.version)),
                (Some(_), None) => Response::Error("not running from an app bundle".into()),
                // A second swap would remove the `.old` the first one still needs for a rollback.
                (Some(_), Some(_)) if pending.is_some() => Response::Error(format!("update {} is still waiting for a window", pending.unwrap_or_default())),
                (Some(s), Some(bundle)) => match crate::updates::swap::swap_in(&bundle, &s.bundle) {
                    Ok(()) => {
                        let marker = crate::updates::apply::Applied { version: s.version.clone(), old_kept: true, at_unix: crate::updates::apply::now_unix() };
                        if let Err(e) = crate::updates::apply::write_marker(&dir, &marker) {
                            eprintln!("arkosd: cannot write the update marker: {e}");
                        }
                        {
                            let mut st = state.lock().unwrap();
                            st.update.staged = None;
                            st.update.applied = Some(marker);
                            st.update.swapped = Some(s.version.clone());
                            // Only a window that says Hello from now on confirms the new bundle.
                            st.update.connected_since_start = false;
                        }
                        crate::updates::stage::clean_except(&dir, None);
                        // Confirms or rolls back in this image too, should no upgrade follow; an
                        // upgrade's new image starts its own from the marker.
                        crate::updates::apply::watch(state.clone(), bundle, dir);
                        Response::Ok
                    }
                    Err(e) => {
                        let mut st = state.lock().unwrap();
                        st.update.last_error = Some(format!("update failed: {e}"));
                        Response::Error(e)
                    }
                },
            }
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
