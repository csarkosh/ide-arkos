//! Accepts clients on the Unix socket, one reader and one writer thread each, and dispatches
//! requests to sessions. Shared state is one mutex; every operation under it is short.
use crate::handover::Handover;
use crate::session::{Backpressure, ClientId, ClientSink, Session, SpawnSpec};
use anyhow::{Context, Result};
use protocol::codec::{read_frame, write_frame};
use protocol::{ClientMessage, CustomizeOutcome, Event, Request, Response, ServerMessage, SessionId, StagedUpdate, CUSTOMIZE_FROM, CUSTOMIZE_WAIT, PROTOCOL_MIN, PROTOCOL_VERSION};
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
    /// The usage session id its `Session` named, whose journal is sealed when it goes; None for
    /// a client that sent none (a protocol 6 window, a tool, a window keeping no journal).
    pub sid: Option<String>,
}

/// A tool client's `Customize` waiting for the windows' answers (agent theme spec, section 5).
/// The daemon only relays: it knows nothing of themes.
pub struct CustomizeWait {
    /// The tool client that asked; its leaving drops the record.
    pub client: ClientId,
    /// The id of the tool's `Customize` message, which the `Customized` answer carries.
    pub msg_id: u32,
    /// Where the answer goes once every window has answered or the wait is over.
    pub tx: ClientSink,
    /// The pids of the windows asked that have not answered yet.
    pub waiting: Vec<u32>,
    /// The answers so far, in arrival order.
    pub outcomes: Vec<CustomizeOutcome>,
    /// The project windows on a protocol before `CUSTOMIZE_FROM`, which were not asked.
    pub too_old: Vec<u32>,
    /// When the windows were asked: only a wait this old is flushed by a deadline.
    pub started: Instant,
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
    /// The `Customize` requests waiting for windows, by the tool's own id.
    pub customize: HashMap<u64, CustomizeWait>,
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

/// A fresh state for the unit tests.
#[cfg(test)]
pub fn test_state() -> Shared {
    Arc::new(Mutex::new(State::default()))
}

use protocol::UPDATE_EVENTS_FROM;

/// The status both answers share: nothing not newer than what runs or is installed is offered,
/// so no window may offer a downgrade (no badge, no idle apply).
fn status_parts(st: &State, running: &crate::updates::version::Version) -> (Option<StagedUpdate>, Option<String>, Option<u64>) {
    let staged = st.update.staged.as_ref().filter(|s| !crate::updates::poll::is_stale(s, running)).map(|s| StagedUpdate { version: s.version.clone(), git: s.git.clone() });
    (staged, st.update.last_error.clone(), st.update.unreachable_for_secs(crate::updates::apply::now_unix()))
}

/// Queues `event` for every connected client that speaks protocol 4 or later: an older window
/// cannot decode the variant and would drop its connection, and a client that never said Hello
/// (protocol 0) is not a window. A client whose sink is closed is skipped; its reader thread is
/// already removing it.
pub fn broadcast(st: &State, event: Event) {
    for rec in st.clients.values().filter(|r| r.connected && r.protocol >= UPDATE_EVENTS_FROM) {
        rec.tx.send(ServerMessage::Event(event.clone()));
    }
}

/// The clients a `Customize` is relayed to, as (client, pid), in connection order, and the pids
/// of the project windows too old to be asked. A project window said Hello with a pid and a
/// folder: the launch chooser (whose empty folder `window_folder` keeps as None), `arkos
/// doctor`'s probe (pid 0, kept as None) and tool clients (no Hello, protocol 0) are neither.
fn customize_targets(st: &State) -> (Vec<(ClientId, u32)>, Vec<u32>) {
    let mut windows: Vec<(ClientId, u32, u32)> = st
        .clients
        .iter()
        .filter(|(_, r)| r.connected && r.ui_cwd.is_some())
        .filter_map(|(id, r)| r.ui_pid.map(|pid| (*id, pid, r.protocol)))
        .collect();
    windows.sort_by_key(|(id, _, _)| *id);
    let targets = windows.iter().filter(|(_, _, protocol)| *protocol >= CUSTOMIZE_FROM).map(|(id, pid, _)| (*id, *pid)).collect();
    let too_old = windows.iter().filter(|(_, _, protocol)| *protocol < CUSTOMIZE_FROM).map(|(_, pid, _)| *pid).collect();
    (targets, too_old)
}

/// Sends a finished wait's `Customized` answer: whoever is still waiting is `missing`. A tool
/// that hung up meanwhile is skipped by its closed sink.
fn answer_customize(w: CustomizeWait) {
    let response = Response::Customized { outcomes: w.outcomes, missing: w.waiting, too_old: w.too_old };
    w.tx.send(ServerMessage::Response { id: w.msg_id, response });
}

/// The deadline of the `Customize` `id`: answers it if it is still waiting and as old as
/// `CUSTOMIZE_WAIT`. A younger record under the same id is a later request, which its own
/// deadline answers.
fn flush_customize(state: &Shared, id: u64) {
    let mut st = state.lock().unwrap();
    if st.customize.get(&id).is_some_and(|w| w.started.elapsed() >= CUSTOMIZE_WAIT) {
        if let Some(w) = st.customize.remove(&id) {
            answer_customize(w);
        }
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
    crate::journal::note(format_args!("started {} (pid {}){}", crate::version_string(), std::process::id(), if recover { ", recovering" } else { "" }));
    if recover {
        let dir = crate::recovery::sessions_dir(&opts.socket);
        let done = crate::recovery::recover(&state, &dir, &opts);
        eprintln!("arkosd: recovered {} sessions and reopened {} windows", done.sessions, done.windows);
    }
    crate::updates::apply::on_start(&state, opts.update.bundle.as_deref(), &opts.update.dir, opts.update.connect_grace);
    crate::updates::drop_stale_staged(&state, &opts.update.dir, &crate::updates::floor(&opts.update.running(), opts.update.bundle.as_deref()));
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
    // An older daemon may have remembered `/`.
    st.windows = h.windows.into_iter().filter(|(cwd, _)| window_folder(cwd.clone()).is_some()).collect();
    st.manifest_dirty = true;
    eprintln!("arkosd: resumed {} sessions from a handover written by {}", st.sessions.len(), h.written_by);
    crate::journal::note(format_args!("resumed as {} (pid {}) from a handover written by {}, {} sessions", crate::version_string(), std::process::id(), h.written_by, st.sessions.len()));
    let state = Arc::new(Mutex::new(st));
    crate::updates::apply::on_start(&state, opts.update.bundle.as_deref(), &opts.update.dir, opts.update.connect_grace);
    crate::updates::drop_stale_staged(&state, &opts.update.dir, &crate::updates::floor(&opts.update.running(), opts.update.bundle.as_deref()));
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
        crate::updates::poll::start(state.clone(), fetcher, u.dir.clone(), keys, u.check_every, u.ditto.clone(), u.running(), u.bundle.clone());
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
            ClientRecord { tx: sink.clone(), attached: Vec::new(), last_heartbeat: Instant::now(), ui_pid: None, ui_exe: None, ui_cwd: None, connected: true, protocol: 0, relaunch: None, sid: None },
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
            disconnect(&state, client_id, &opts.update.dir);
            sink.close();
        })
        .expect("spawn reader");
}

fn disconnect(state: &Shared, client_id: ClientId, updates_dir: &Path) {
    let (relaunch, ui_exe, ui_cwd, clock) = {
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
        let (relaunch, ui_exe, ui_cwd, ui_pid) = match st.clients.remove(&client_id) {
            Some(rec) => (rec.relaunch, rec.ui_exe, rec.ui_cwd, rec.ui_pid),
            None => return,
        };
        // A tool that hung up wants no answer; a window that left will not answer, and a wait
        // left with nobody to hear from is answered now.
        st.customize.retain(|_, w| w.client != client_id);
        if let Some(pid) = ui_pid {
            let mut done = Vec::new();
            for (id, w) in st.customize.iter_mut() {
                w.waiting.retain(|p| *p != pid);
                if w.waiting.is_empty() {
                    done.push(*id);
                }
            }
            for id in done {
                if let Some(w) = st.customize.remove(&id) {
                    answer_customize(w);
                }
            }
        }
        // The new window starts now: an applied update's grace counts from here.
        let clock = if relaunch.is_some() { crate::updates::apply::restart_clock(&mut st.update, updates_dir) } else { None };
        (relaunch, ui_exe, ui_cwd, clock)
    };
    if let Some(line) = clock {
        crate::journal::note(line);
    }
    // The lock is released: launching never blocks other threads.
    if let Some((exe, cwd)) = relaunch {
        if let Err(e) = crate::watchdog::launch_ui(&exe, Some(&cwd), "relaunch the window asked for") {
            eprintln!("arkosd: relaunch of {} failed: {e}; launching the previous window instead", exe.display());
            if let Some(old) = ui_exe {
                if let Err(e) = crate::watchdog::launch_ui(&old, ui_cwd.as_deref(), "the previous window, after the relaunch failed") {
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

/// Starts a closed tab's grace unless an upgrade began meanwhile: the request's `upgrading`
/// check comes before `set_grace`, and a handover snapshot taken between the two would not carry
/// the grace. Re-checked after it, under the state lock alone (never under the session lock);
/// an upgrade seen then drops the grace and answers false, so the window keeps its own minute.
/// Otherwise the manifest is marked, so a restart in the minute keeps the grace.
fn grace_unless_upgrading(state: &Shared, s: &Session, secs: u32) -> bool {
    s.set_grace(secs);
    let upgrading = {
        let mut st = state.lock().unwrap();
        if !st.upgrading {
            st.manifest_dirty = true;
        }
        st.upgrading
    };
    if upgrading {
        s.clear_grace();
    }
    !upgrading
}

/// The folder a window's Hello names, as the daemon keeps it: None for an empty one (the
/// protocol's "keep the daemon's", which the launch chooser sends) and for `/`, where macOS starts
/// an app opened from the Dock. A window relaunched at `/` shows the chooser instead of a project,
/// so `/` is never remembered, reopened or relaunched into.
pub fn window_folder(cwd: PathBuf) -> Option<PathBuf> {
    (!cwd.as_os_str().is_empty() && cwd != Path::new("/")).then_some(cwd)
}

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
                rec.ui_cwd = window_folder(ui_cwd);
                rec.last_heartbeat = Instant::now();
                rec.protocol = protocol_version;
            }
            // A window that was not connected at the swap came up: the new one, which confirms
            // an applied update (see `updates::apply::watch`). The old window's reconnect after
            // the upgrade carries a pid the marker lists, and confirms nothing.
            let pid = st.clients.get(&client_id).and_then(|r| r.ui_pid);
            let (mut journal, mut confirmed) = (None, false);
            if let Some(applied) = &st.update.applied {
                let folder = st.clients.get(&client_id).and_then(|r| r.ui_cwd.as_ref()).map_or("no folder".to_string(), |c| c.display().to_string());
                let (confirms, why) = match pid {
                    Some(pid) if !applied.pids_at_apply.contains(&pid) => (true, "a window not connected at the apply"),
                    Some(_) => (false, "this pid was connected at the apply (the old window reconnecting)"),
                    None => (false, "no window pid"),
                };
                journal = Some(format!(
                    "hello from client {client_id}, pid {}, {folder}, protocol {protocol_version}, during the pending apply of {}: {} ({why})",
                    pid.map_or("none".to_string(), |p| p.to_string()),
                    applied.version,
                    if confirms { "confirms" } else { "does not confirm" }
                ));
                confirmed = confirms;
            }
            if confirmed {
                st.update.new_window_seen = true;
            }
            let window = st.clients.get(&client_id).and_then(|r| r.ui_cwd.clone().map(|cwd| (cwd, r.ui_exe.clone())));
            if let Some((cwd, exe)) = window {
                st.windows.insert(cwd, exe);
                st.manifest_dirty = true;
            }
            drop(st);
            if let Some(line) = journal {
                crate::journal::note(line);
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
                crate::journal::note(format_args!("upgrade to {} refused: {e}", exe.display()));
                crate::upgrade::abort(state);
                let line = crate::updates::apply::clock_after_failed_upgrade(&mut state.lock().unwrap().update, &opts.update.dir, &exe.display().to_string(), &e);
                if let Some(l) = line {
                    crate::journal::note(l);
                }
                return Some(Response::Error(e));
            }
            // Returns only on failure; on success this process is now the new binary.
            Response::Error(crate::upgrade::execute(state, &exe, opts))
        }
        Request::RelaunchClient { exe, cwd } => {
            if !is_arkos_binary(&exe) {
                let why = format!("{} is not a arkos binary owned by this user", exe.display());
                let line = crate::updates::apply::clock_after_failed_upgrade(&mut state.lock().unwrap().update, &opts.update.dir, &exe.display().to_string(), &why);
                if let Some(l) = line {
                    crate::journal::note(l);
                }
                Response::Error(why)
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
        // Kept on the record alone: the record leaves the table when the window goes, and the
        // journal is sealed from what it held. A later one replaces it.
        Request::Session { sid } => {
            if let Some(rec) = state.lock().unwrap().clients.get_mut(&client_id) {
                rec.sid = Some(sid);
            }
            Response::Ok
        }
        // Until the daemon keeps telemetry state: the config's defaults and an empty queue.
        Request::TelemetryStatus => Response::TelemetryStatus {
            usage: "local".into(),
            crashes: "ask".into(),
            uploads: false,
            reason: None,
            queued: 0,
            journals: 0,
            last_check_day: None,
            sent: 0,
            rejected: 0,
        },
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
                let had_grace = s.grace_secs().is_some();
                s.subscribe(client_id, tx.clone(), id);
                let mut st = state.lock().unwrap();
                if let Some(rec) = st.clients.get_mut(&client_id) {
                    if !rec.attached.contains(&sid) {
                        rec.attached.push(sid);
                    }
                }
                // The attach cleared a closed tab's grace: the manifest must not keep it.
                if had_grace {
                    st.manifest_dirty = true;
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
            // Read before the lock: it reads the installed bundle's Info.plist.
            let running = crate::updates::floor(&opts.update.running(), opts.update.bundle.as_deref());
            let st = state.lock().unwrap();
            let (staged, last_error, unreachable_for_secs) = status_parts(&st, &running);
            Response::UpdateStatus { staged, last_error, unreachable_for_secs }
        }
        Request::UpdateStatusDetailed => {
            let running = crate::updates::floor(&opts.update.running(), opts.update.bundle.as_deref());
            let st = state.lock().unwrap();
            let (staged, last_error, unreachable_for_secs) = status_parts(&st, &running);
            Response::UpdateStatusDetailed {
                staged,
                last_error,
                unreachable_for_secs,
                checking: st.update.checking,
                last_check_unix: st.update.last_check_unix,
                transient_error: st.update.transient_error.clone(),
            }
        }
        Request::DetachForGrace { secs: 0, .. } => Response::Error("a grace of zero: use Kill".into()),
        // The handover's snapshot may already be taken: a grace set now would not cross the exec,
        // and the session would outlive its closed tab. The window keeps its own timer instead.
        Request::DetachForGrace { .. } if state.lock().unwrap().upgrading => Response::Error(UPGRADE_IN_PROGRESS.into()),
        // A closed tab (milestone 6a spec, section 14): detached like `Detach`, then ended by the
        // reaper after `secs` unless a window attaches it first. Same lock order as `Detach`: the
        // session handle through `session(sid)` first, the state lock on its own afterwards. A
        // session that already ended, or is gone, answers `Ok` like `Detach` does: the tab is
        // closed either way, and an error would put a banner in the window for nothing.
        Request::DetachForGrace { session: sid, secs } => {
            let mut refused = false;
            if let Some(s) = session(sid) {
                s.unsubscribe(client_id);
                if s.info().alive {
                    refused = !grace_unless_upgrading(state, &s, secs);
                }
            }
            if let Some(rec) = state.lock().unwrap().clients.get_mut(&client_id) {
                rec.attached.retain(|x| *x != sid);
            }
            if refused {
                Response::Error(UPGRADE_IN_PROGRESS.into())
            } else {
                Response::Ok
            }
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
            use crate::updates::apply;
            // Checked and claimed under one lock, as `upgrade::begin` does: a second request
            // arriving while this one swaps is refused, so two cannot race the renames.
            let running = crate::updates::floor(&opts.update.running(), opts.update.bundle.as_deref());
            let asked_is_stale = crate::updates::version::parse_semver(&version).is_some_and(|(major, minor, patch)| !crate::updates::version::Version { major, minor, patch, git: String::new() }.is_newer_than(&running));
            let mut forgotten = None;
            let claimed = {
                let mut st = state.lock().unwrap();
                let pids: Vec<u32> = st.clients.values().filter(|c| c.connected).filter_map(|c| c.ui_pid).collect();
                let waiting = st.update.applying.clone().or_else(|| st.update.applied.as_ref().map(|a| a.version.clone()));
                match (st.update.staged.clone(), opts.update.bundle.clone()) {
                    // First, whatever is staged: a second swap would remove the `.old` the first
                    // one still needs for a rollback, and a window whose apply comes second hears
                    // that another one is applying (not "nothing staged", which reads as a failure).
                    _ if waiting.is_some() => Err(format!("update {} is still waiting for a window", waiting.unwrap_or_default())),
                    // The app was updated past it (by hand, while this daemon ran): applying it would
                    // be a downgrade. Forgotten here, deleted once the lock is released.
                    (Some(s), _) if crate::updates::poll::is_stale(&s, &running) => {
                        st.update.staged = None;
                        let e = format!("staged {} is not newer than running or installed {}", s.version, crate::updates::semver(&running));
                        forgotten = Some(s);
                        Err(e)
                    }
                    _ if asked_is_stale => Err(format!("{version} is not newer than running or installed {}", crate::updates::semver(&running))),
                    (None, _) => Err("nothing staged".to_string()),
                    (Some(s), _) if s.version != version => Err(format!("staged {} is not {version}", s.version)),
                    (Some(_), None) => Err("not running from an app bundle".into()),
                    (Some(s), Some(bundle)) => {
                        st.update.applying = Some(s.version.clone());
                        Ok((s, bundle, pids))
                    }
                }
            };
            let (s, bundle, pids) = match claimed {
                Ok(c) => c,
                Err(e) => {
                    crate::journal::note(format_args!("apply {version} refused: {e}"));
                    if let Some(folder) = forgotten.as_ref().and_then(|s| s.bundle.parent()) {
                        let deleted = forgotten.as_ref().map(|s| s.version.as_str()).unwrap_or(&version);
                        match std::fs::remove_dir_all(folder) {
                            Ok(()) => crate::journal::note(format_args!("staged {} deleted: {}", deleted, folder.display())),
                            Err(e) => crate::journal::note(format_args!("staged {} cannot be deleted from {}: {e}", deleted, folder.display())),
                        }
                    }
                    return Some(Response::Error(e));
                }
            };
            crate::journal::note(format_args!("apply {} ({}) to {} asked by client {client_id}; windows at the apply: pids {pids:?}", s.version, s.git, bundle.display()));
            let dir = opts.update.dir.clone();
            // The marker first: a daemon that dies mid-swap must find it at its next start, or
            // the start would clean the `.old` away unconfirmed.
            let marker = apply::Applied { version: s.version.clone(), old_kept: true, at_unix: apply::now_unix(), pids_at_apply: pids };
            if let Err(e) = apply::write_marker(&dir, &marker) {
                crate::journal::note(format_args!("cannot write the update marker for {}: {e}", s.version));
                state.lock().unwrap().update.applying = None;
                return Some(Response::Error(format!("cannot write the update marker: {e}")));
            }
            crate::journal::note(format_args!("marker written for {}", s.version));
            match crate::updates::swap::swap_in(&bundle, &s.bundle) {
                Ok(()) => {
                    crate::journal::note(format_args!("swap ok: {} is now {}; the previous app kept as .old until a window confirms", bundle.display(), s.version));
                    {
                        let mut st = state.lock().unwrap();
                        st.update.applying = None;
                        st.update.staged = None;
                        st.update.applied = Some(marker);
                        st.update.swapped = Some(s.version.clone());
                        // Only a window that says Hello from now on confirms the new bundle.
                        st.update.new_window_seen = false;
                        // The grace starts when the new window is launched, not now.
                        st.update.clock_running = false;
                    }
                    crate::updates::stage::clean_except(&dir, None);
                    // Confirms or rolls back in this image too, should no upgrade follow; an
                    // upgrade's new image starts its own from the marker.
                    apply::watch(state.clone(), bundle, dir, opts.update.connect_grace);
                    Response::Ok
                }
                Err(e) => {
                    crate::journal::note(format_args!("swap of {} failed: {e}", s.version));
                    // Only the marker this request wrote: never one another apply left.
                    if apply::read_marker(&dir).as_ref() == Some(&marker) {
                        apply::clear_marker(&dir);
                    }
                    let mut st = state.lock().unwrap();
                    st.update.applying = None;
                    st.update.last_error = Some(format!("update failed: {e}"));
                    Response::Error(e)
                }
            }
        }
        Request::SetState { session: sid, state: claude } => match session(sid) {
            Some(s) => {
                s.set_state(claude);
                Response::Ok
            }
            None => Response::Error(format!("no session {}", sid.0)),
        },
        // Relayed to every project window under one lock; the answer goes later through `tx`
        // with this message's id, when every window has answered or `CUSTOMIZE_WAIT` is over.
        Request::Customize { id: customize_id, action } => {
            let mut st = state.lock().unwrap();
            if st.customize.contains_key(&customize_id) {
                return Some(Response::Error("customize id in flight".into()));
            }
            let (targets, too_old) = customize_targets(&st);
            let started = Instant::now();
            let event = Event::Customize { id: customize_id, action };
            let waiting: Vec<u32> = targets
                .into_iter()
                .filter(|(client, _)| st.clients.get(client).is_some_and(|r| r.tx.send(ServerMessage::Event(event.clone()))))
                .map(|(_, pid)| pid)
                .collect();
            if waiting.is_empty() {
                return Some(Response::Customized { outcomes: vec![], missing: vec![], too_old });
            }
            st.customize.insert(customize_id, CustomizeWait { client: client_id, msg_id: id, tx: tx.clone(), waiting, outcomes: vec![], too_old, started });
            drop(st);
            let state = state.clone();
            std::thread::Builder::new()
                .name(format!("customize-{customize_id}"))
                .spawn(move || {
                    std::thread::sleep(CUSTOMIZE_WAIT);
                    flush_customize(&state, customize_id);
                })
                .expect("spawn customize deadline");
            return None;
        }
        // Always `Ok`: an answer after the wait, or a second one, is dropped, never an error the
        // window would show as a banner.
        Request::CustomizeDone { id: customize_id, outcome } => {
            let mut st = state.lock().unwrap();
            if let Some(w) = st.customize.get_mut(&customize_id) {
                if let Some(i) = w.waiting.iter().position(|p| *p == outcome.pid) {
                    w.waiting.remove(i);
                    w.outcomes.push(outcome);
                }
                if w.waiting.is_empty() {
                    if let Some(w) = st.customize.remove(&customize_id) {
                        answer_customize(w);
                    }
                }
            }
            Response::Ok
        }
    };
    Some(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The handover's snapshot can fall between the request's `upgrading` check and the grace:
    /// re-checked after, the grace is dropped and refused, so the window keeps its own minute.
    #[test]
    fn a_grace_set_while_an_upgrade_began_is_dropped_and_refused() {
        let spec = SpawnSpec {
            id: SessionId(41),
            cwd: std::env::temp_dir(),
            shell: Some("/bin/sh".into()),
            env: vec![],
            nice: 0,
            rows: 24,
            cols: 80,
            replay_capacity: 64,
            backpressure: crate::session::Backpressure::default(),
            seed: Vec::new(),
            recovered: false,
            agent: None,
        };
        let s = Session::spawn_for_test(spec, false).unwrap();
        let state: Shared = Arc::new(Mutex::new(State::default()));
        assert!(grace_unless_upgrading(&state, &s, 60));
        assert_eq!(s.grace_secs(), Some(60));
        assert!(state.lock().unwrap().manifest_dirty);
        state.lock().unwrap().upgrading = true;
        assert!(!grace_unless_upgrading(&state, &s, 60));
        assert_eq!(s.grace_secs(), None, "no grace the snapshot may have missed");
        let _ = s.kill();
    }

    fn test_options(dir: &Path) -> Options {
        let socket = dir.join("arkosd.sock");
        Options {
            replay_capacity: 64,
            watchdog: Duration::from_secs(60),
            client_pending_cap: 1 << 20,
            backpressure: Backpressure::default(),
            reaper_every: Duration::from_secs(1),
            dead_session_ttl: Duration::from_secs(60),
            flush_quiet: Duration::from_secs(1),
            update: crate::updates::UpdateOptions::defaults(&socket),
            socket,
        }
    }

    /// A client in the table with a sink nobody reads, as `serve` would add it.
    fn add_client(state: &Shared, client: ClientId) -> ClientSink {
        let (a, _b) = UnixStream::pair().unwrap();
        let sink = ClientSink::new(client, a, 1 << 20);
        let rec = ClientRecord { tx: sink.clone(), attached: Vec::new(), last_heartbeat: Instant::now(), ui_pid: None, ui_exe: None, ui_cwd: None, connected: true, protocol: 0, relaunch: None, sid: None };
        state.lock().unwrap().clients.insert(client, rec);
        sink
    }

    fn hello(protocol_version: u32) -> Request {
        Request::Hello { protocol_version, ui_pid: 0, ui_exe: None, ui_cwd: PathBuf::new() }
    }

    /// The sid a protocol 7 window sends after its Hello is what the daemon later seals that
    /// window's journal from; a second one (the journal restarted on an opt-in) replaces it.
    #[test]
    fn the_session_after_hello_is_kept_on_the_window_record() {
        let dir = tempfile::tempdir().unwrap();
        let (state, opts) = (test_state(), test_options(dir.path()));
        let tx = add_client(&state, 1);
        assert!(matches!(dispatch(&state, 1, &tx, 1, hello(7), &opts), Some(Response::Hello { protocol_max: 7, .. })));
        assert_eq!(state.lock().unwrap().clients[&1].sid, None, "no sid before the window sends one");
        let first = "0123456789abcdef0123456789abcdef";
        assert_eq!(dispatch(&state, 1, &tx, 2, Request::Session { sid: first.into() }, &opts), Some(Response::Ok));
        assert_eq!(state.lock().unwrap().clients[&1].sid.as_deref(), Some(first));
        let second = "fedcba9876543210fedcba9876543210";
        assert_eq!(dispatch(&state, 1, &tx, 3, Request::Session { sid: second.into() }, &opts), Some(Response::Ok));
        assert_eq!(state.lock().unwrap().clients[&1].sid.as_deref(), Some(second));
        // A second Hello on the same connection (the chooser naming its project) keeps it.
        dispatch(&state, 1, &tx, 4, hello(7), &opts);
        assert_eq!(state.lock().unwrap().clients[&1].sid.as_deref(), Some(second));
    }

    /// `TelemetryStatus` is not gated on the Hello's protocol: the command line and the doctor
    /// ask it, and a protocol 6 window that never sends `Session` simply has nothing sealed.
    #[test]
    fn telemetry_status_answers_whatever_the_protocol() {
        let dir = tempfile::tempdir().unwrap();
        let (state, opts) = (test_state(), test_options(dir.path()));
        let empty = Response::TelemetryStatus {
            usage: "local".into(),
            crashes: "ask".into(),
            uploads: false,
            reason: None,
            queued: 0,
            journals: 0,
            last_check_day: None,
            sent: 0,
            rejected: 0,
        };
        let old = add_client(&state, 1);
        dispatch(&state, 1, &old, 1, hello(6), &opts);
        assert_eq!(dispatch(&state, 1, &old, 2, Request::TelemetryStatus, &opts), Some(empty.clone()));
        assert_eq!(state.lock().unwrap().clients[&1].sid, None);
        let tool = add_client(&state, 2);
        assert_eq!(dispatch(&state, 2, &tool, 1, Request::TelemetryStatus, &opts), Some(empty));
    }
}
