//! The UI's connection to arkosd: a writer thread fed by `send`, a reader thread that forwards
//! every `ServerMessage` into a futures channel the GPUI side awaits. Nothing here blocks the
//! caller. The reader stops pulling frames while too much output sits unread in the channel, so
//! a flood backs up into the daemon, whose backpressure then pauses the PTY.

use anyhow::{Context, Result};
use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use protocol::codec::{read_frame, write_frame};
use protocol::{ClientMessage, Event, Request, Response, ServerMessage, PROTOCOL_VERSION};
use std::io::{BufReader, BufWriter};
use std::io::ErrorKind;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::sync::mpsc::{channel, Sender};
use std::time::{Duration, Instant};

/// Output bytes the reader may leave unread in the channel before it stops reading the socket.
pub const CLIENT_UNREAD_CAP: usize = 8 * 1024 * 1024;

#[derive(Default)]
struct Unread {
    bytes: usize,
    closed: bool,
}

/// Unread output bytes in the channel, shared by the reader thread and the UI.
type UnreadGate = Arc<(Mutex<Unread>, Condvar)>;

/// What the writer thread takes: a frame to write, or a barrier to answer once every frame
/// queued before it is written.
enum Outgoing {
    Frame(ClientMessage),
    Barrier(Sender<()>),
}

pub struct Client {
    tx: Sender<Outgoing>,
    next_id: AtomicU32,
    disconnect_reason: Arc<Mutex<Option<String>>>,
    unread: UnreadGate,
    /// Kept so dropping the client can close the socket and release a waiting reader.
    stream: UnixStream,
    /// The pid of the process at the other end, read once at connect; `None` when the system
    /// would not say.
    peer_pid: Option<u32>,
    /// The usage session id sent after every Hello, so the daemon can seal this window's
    /// journal; None when the window keeps none. It changes on a live client (`set_sid`).
    sid: Mutex<Option<String>>,
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        let (lock, cvar) = &*self.unread;
        lock.lock().unwrap().closed = true;
        cvar.notify_all();
    }
}

impl Client {
    /// Connects and says Hello, followed by `Session` when `sid` is given.
    pub fn connect(socket: &Path, sid: Option<String>) -> Result<(Client, UnboundedReceiver<ServerMessage>)> {
        Client::connect_with_cap(socket, CLIENT_UNREAD_CAP, sid)
    }

    /// Connects with `cap` unread output bytes allowed in the channel before the reader waits.
    pub fn connect_with_cap(socket: &Path, cap: usize, sid: Option<String>) -> Result<(Client, UnboundedReceiver<ServerMessage>)> {
        let stream = UnixStream::connect(socket).with_context(|| format!("connect {}", socket.display()))?;
        let peer_pid = peer_pid_of(&stream);
        let reader_stream = stream.try_clone().context("clone socket")?;
        let shutdown_stream = stream.try_clone().context("clone socket")?;
        let unread: UnreadGate = Arc::new((Mutex::new(Unread::default()), Condvar::new()));
        let unread_for_reader = unread.clone();
        let (tx, rx) = channel::<Outgoing>();
        let (ev_tx, ev_rx): (UnboundedSender<ServerMessage>, UnboundedReceiver<ServerMessage>) = unbounded();
        std::thread::Builder::new()
            .name("arkosd-writer".into())
            .spawn(move || {
                let mut w = BufWriter::new(stream);
                while let Ok(out) = rx.recv() {
                    match out {
                        Outgoing::Frame(msg) => {
                            if write_frame(&mut w, &msg).is_err() {
                                break;
                            }
                        }
                        Outgoing::Barrier(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            })
            .context("spawn writer")?;
        let disconnect_reason = Arc::new(Mutex::new(None));
        let reason_for_reader = disconnect_reason.clone();
        std::thread::Builder::new()
            .name("arkosd-reader".into())
            .spawn(move || {
                let mut r = BufReader::new(reader_stream);
                let (lock, cvar) = &*unread_for_reader;
                let mut greeted = false;
                let reason = loop {
                    match read_frame::<_, ServerMessage>(&mut r) {
                        Ok(Some(msg)) => {
                            // One line per connection, for window.log.
                            if let ServerMessage::Response { response: Response::Hello { daemon_version, .. }, .. } = &msg {
                                if !greeted {
                                    greeted = true;
                                    eprintln!("arkos: connected to {daemon_version}");
                                }
                            }
                            let output = match &msg {
                                ServerMessage::Event(Event::Output { bytes, .. }) => bytes.len(),
                                _ => 0,
                            };
                            // Counted before the send so the UI never consumes bytes not yet added.
                            if output > 0 {
                                lock.lock().unwrap().bytes += output;
                            }
                            if ev_tx.unbounded_send(msg).is_err() {
                                return;
                            }
                            // Control messages never wait; only reading further frames does.
                            if output > 0 {
                                let mut u = lock.lock().unwrap();
                                while u.bytes > cap && !u.closed {
                                    u = cvar.wait(u).unwrap();
                                }
                                if u.closed {
                                    return;
                                }
                            }
                        }
                        Ok(None) => break "daemon closed the connection".to_string(),
                        Err(e) => break format!("connection to daemon failed: {e}"),
                    }
                };
                // Set before ev_tx drops so a receiver that sees the end can read the reason.
                *reason_for_reader.lock().unwrap() = Some(reason);
            })
            .context("spawn reader")?;
        let client = Client { tx, next_id: AtomicU32::new(1), disconnect_reason, unread, stream: shutdown_stream, peer_pid, sid: Mutex::new(sid) };
        client.hello();
        Ok((client, ev_rx))
    }

    /// The daemon's pid as the socket reported it at connect, for the About window.
    pub fn peer_pid(&self) -> Option<u32> {
        self.peer_pid
    }

    /// Says Hello with this process's pid, binary and folder: at connect, and again on the same
    /// connection once the launch chooser's folder is the working directory, so the daemon
    /// replaces the empty folder the first one named. The session id follows it at once when
    /// there is one. Returns the Hello's id.
    pub fn hello(&self) -> u32 {
        let id = self.send(Request::Hello {
            protocol_version: PROTOCOL_VERSION,
            ui_pid: std::process::id(),
            ui_exe: std::env::current_exe().ok(),
            ui_cwd: crate::launch::hello_cwd(&std::env::current_dir().unwrap_or_default()),
        });
        let sid = self.sid.lock().unwrap().clone();
        if let Some(sid) = sid {
            self.send(Request::Session { sid });
        }
        id
    }

    /// Replaces the session id: the opt-in starts a new journal, and turning usage off keeps
    /// none. A new one is said at once, so the daemon seals the new journal when this window
    /// goes; every later Hello says it again.
    pub fn set_sid(&self, sid: Option<String>) {
        *self.sid.lock().unwrap() = sid.clone();
        if let Some(sid) = sid {
            self.send(Request::Session { sid });
        }
    }

    /// Tells the daemon this window's usage session ended as a quit, on a quit that sends no
    /// `Goodbye` (a macOS logout or restart, after which the window comes back): without it the
    /// closing socket reads as a crash. Nothing else changes. Returns the request's id.
    pub fn session_quit(&self) -> u32 {
        self.send(Request::SessionQuit)
    }

    /// Queues a request. Returns its id so the response can be matched.
    pub fn send(&self, request: Request) -> u32 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let _ = self.tx.send(Outgoing::Frame(ClientMessage { id, request }));
        id
    }

    /// Waits up to `timeout` until every request queued so far is written to the socket. For
    /// quitting, when the process may exit before the writer thread gets to them. Returns
    /// whether they were written.
    pub fn wait_sent(&self, timeout: Duration) -> bool {
        let (done, rx) = channel();
        if self.tx.send(Outgoing::Barrier(done)).is_err() {
            return false;
        }
        rx.recv_timeout(timeout).is_ok()
    }

    /// Reports that the UI has applied `bytes` of output taken from the channel, letting a reader
    /// waiting on the unread cap continue.
    pub fn consumed_output(&self, bytes: usize) {
        if bytes == 0 {
            return;
        }
        let (lock, cvar) = &*self.unread;
        let mut u = lock.lock().unwrap();
        u.bytes = u.bytes.saturating_sub(bytes);
        cvar.notify_all();
    }

    /// Why the connection ended, once the receiver has finished. None while still connected.
    pub fn disconnect_reason(&self) -> Option<String> {
        self.disconnect_reason.lock().unwrap().clone()
    }

    /// Removes a socket file that exists but refuses connections. Any other file type or connect
    /// error is returned to the caller and the path is left alone.
    pub fn ensure_stale_socket_removed(socket: &Path) -> Result<()> {
        let meta = match std::fs::symlink_metadata(socket) {
            Ok(m) => m,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e).with_context(|| format!("stat {}", socket.display())),
        };
        match UnixStream::connect(socket) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == ErrorKind::ConnectionRefused && meta.file_type().is_socket() => {
                std::fs::remove_file(socket).with_context(|| format!("remove stale socket {}", socket.display()))
            }
            Err(e) => Err(e).with_context(|| format!("cannot use {}", socket.display())),
        }
    }

    /// Connects, or starts the daemon detached from this process (with `daemon_args` after its
    /// socket) and retries for three seconds. `sid` is `connect`'s.
    pub fn connect_or_start(socket: &Path, daemon_exe: &Path, daemon_args: &[String], sid: Option<String>) -> Result<(Client, UnboundedReceiver<ServerMessage>)> {
        if let Ok(c) = Client::connect(socket, sid.clone()) {
            return Ok(c);
        }
        Client::ensure_stale_socket_removed(socket)?;
        let log = daemon_log(socket);
        if let Some(parent) = socket.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let log_file = std::fs::OpenOptions::new().create(true).append(true).open(&log).context("open daemon log")?;
        let mut cmd = Command::new(daemon_exe);
        cmd.arg("--socket").arg(socket).args(daemon_args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(log_file);
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                unblock_signals();
                Ok(())
            });
        }
        let mut child = cmd.spawn().with_context(|| format!("start {}", daemon_exe.display()))?;
        // Reaped when it exits, so a daemon that keeps dying leaves no zombies behind.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match Client::connect(socket, sid.clone()) {
                Ok(c) => return Ok(c),
                Err(e) if Instant::now() >= deadline => {
                    return Err(e.context(format!(
                        "started {} but it never accepted a connection; see {}",
                        daemon_exe.display(),
                        log.display()
                    )))
                }
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    }
}

/// The pid of the process at the other end of a connected Unix socket (`LOCAL_PEERPID`); `None`
/// when the call fails.
fn peer_pid_of(stream: &UnixStream) -> Option<u32> {
    let mut pid: libc::pid_t = 0;
    let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: getsockopt writes at most `len` bytes into `pid`, which is that size, and the fd is
    // the stream's own, open for the duration of the call.
    let rc = unsafe { libc::getsockopt(stream.as_raw_fd(), libc::SOL_LOCAL, libc::LOCAL_PEERPID, (&mut pid as *mut libc::pid_t).cast(), &mut len) };
    (rc == 0 && pid > 0).then_some(pid as u32)
}

/// Where a daemon the window starts writes its log: next to its socket.
pub fn daemon_log(socket: &Path) -> std::path::PathBuf {
    socket.with_file_name("arkosd.log")
}

/// Between fork and exec: no signal blocked. The reconnect loop starts the daemon from a
/// background executor thread, a GCD worker that blocks nearly every signal; a blocked signal
/// stays blocked across fork and exec, so that daemon would outlive `pkill -x arkosd`, and so
/// would its shells. Only async-signal-safe calls.
fn unblock_signals() {
    // SAFETY: sigemptyset and pthread_sigmask are async-signal-safe and get valid pointers.
    unsafe {
        let mut none: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut none);
        libc::pthread_sigmask(libc::SIG_SETMASK, &none, std::ptr::null_mut());
    }
}

/// Held by every test that starts a process and by every test that asserts on a closed socket:
/// between fork and exec a child holds a copy of every descriptor in the test binary, a listener
/// another test thread just dropped included, so that socket would still accept.
#[cfg(test)]
pub(crate) static FORK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// `FORK_LOCK`, still usable after a test that held it failed.
#[cfg(test)]
pub(crate) fn fork_lock() -> std::sync::MutexGuard<'static, ()> {
    FORK_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::codec::{read_frame, write_frame};
    use protocol::{ClientMessage, Event, Request, Response};
    use std::io::{BufReader, BufWriter};
    use std::os::unix::net::UnixListener;
    use futures::executor::block_on;
    use futures::StreamExt;

    /// A one-connection fake daemon that answers every request with Ok, for two requests.
    fn fake_daemon(socket: &std::path::Path) -> std::thread::JoinHandle<Vec<ClientMessage>> {
        fake_daemon_for(socket, 2)
    }

    /// A one-connection fake daemon that answers `frames` requests with Ok and returns them.
    fn fake_daemon_for(socket: &std::path::Path, frames: usize) -> std::thread::JoinHandle<Vec<ClientMessage>> {
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
                if seen.len() == frames {
                    break;
                }
            }
            seen
        })
    }

    #[test]
    fn a_child_started_from_a_thread_that_blocks_sigterm_still_dies_of_it() {
        let _fork = fork_lock();
        // Like a GCD worker, the spawning thread blocks SIGTERM; the mask is per thread, so the
        // rest of the test process is untouched.
        let mut child = std::thread::spawn(|| {
            unsafe {
                let mut term: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&mut term);
                libc::sigaddset(&mut term, libc::SIGTERM);
                libc::pthread_sigmask(libc::SIG_BLOCK, &term, std::ptr::null_mut());
            }
            let mut cmd = Command::new("/bin/sleep");
            cmd.arg("30");
            unsafe {
                cmd.pre_exec(|| {
                    unblock_signals();
                    Ok(())
                });
            }
            cmd.spawn().unwrap()
        })
        .join()
        .unwrap();
        // Give the child time to exec before the signal.
        std::thread::sleep(Duration::from_millis(200));
        unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(3);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("the child kept SIGTERM blocked");
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(libc::SIGTERM));
    }

    #[test]
    fn sends_hello_first_and_forwards_responses() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let server = fake_daemon(&socket);
        let (client, mut rx) = Client::connect(&socket, None).unwrap();
        let id = client.send(Request::ListSessions);
        let first = block_on(rx.next()).unwrap();
        let second = block_on(rx.next()).unwrap();
        assert!(matches!(first, ServerMessage::Response { id: 1, response: Response::Ok }));
        assert!(matches!(second, ServerMessage::Response { id: i, .. } if i == id));
        let seen = server.join().unwrap();
        assert!(matches!(seen[0].request, Request::Hello { protocol_version: PROTOCOL_VERSION, .. }));
        assert_eq!(seen[1].request, Request::ListSessions);
    }

    /// A window with a session id says it right after every Hello, so the daemon can seal its
    /// journal; one without sends Hello alone, as a protocol 6 window does.
    #[test]
    fn sends_the_session_right_after_hello_only_when_it_has_a_sid() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("sid.sock");
        let server = fake_daemon_for(&socket, 5);
        let sid = "0123456789abcdef0123456789abcdef".to_string();
        let (client, _rx) = Client::connect(&socket, Some(sid.clone())).unwrap();
        client.send(Request::ListSessions);
        // The chooser's second Hello names the project; the sid follows it again.
        client.hello();
        let seen: Vec<Request> = server.join().unwrap().into_iter().map(|m| m.request).collect();
        assert!(matches!(seen[0], Request::Hello { protocol_version: PROTOCOL_VERSION, .. }), "{seen:?}");
        assert_eq!(seen[1], Request::Session { sid: sid.clone() });
        assert_eq!(seen[2], Request::ListSessions);
        assert!(matches!(seen[3], Request::Hello { .. }), "{seen:?}");
        assert_eq!(seen[4], Request::Session { sid });

        let socket = dir.path().join("nosid.sock");
        let server = fake_daemon_for(&socket, 3);
        let (client, _rx) = Client::connect(&socket, None).unwrap();
        client.send(Request::ListSessions);
        client.hello();
        let seen: Vec<Request> = server.join().unwrap().into_iter().map(|m| m.request).collect();
        assert!(matches!(seen[0], Request::Hello { .. }), "{seen:?}");
        assert_eq!(seen[1], Request::ListSessions);
        assert!(matches!(seen[2], Request::Hello { .. }), "{seen:?}");
    }

    /// The opt-in and a sealed journal give a running window a new session id: it is said at
    /// once on the live connection, and after every later Hello.
    #[test]
    fn a_live_client_says_a_new_sid_at_once_and_after_every_hello() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("newsid.sock");
        let server = fake_daemon_for(&socket, 6);
        let (first, second) = ("0123456789abcdef0123456789abcdef".to_string(), "fedcba9876543210fedcba9876543210".to_string());
        let (client, _rx) = Client::connect(&socket, Some(first.clone())).unwrap();
        client.set_sid(Some(second.clone()));
        client.hello();
        // A window that turned usage off keeps its connection and names no session again.
        client.set_sid(None);
        client.hello();
        let seen: Vec<Request> = server.join().unwrap().into_iter().map(|m| m.request).collect();
        assert!(matches!(seen[0], Request::Hello { .. }), "{seen:?}");
        assert_eq!(seen[1], Request::Session { sid: first });
        assert_eq!(seen[2], Request::Session { sid: second.clone() }, "said at once");
        assert!(matches!(seen[3], Request::Hello { .. }), "{seen:?}");
        assert_eq!(seen[4], Request::Session { sid: second });
        assert!(matches!(seen[5], Request::Hello { .. }), "{seen:?}");
    }

    /// A quit without a Goodbye tells the daemon the session ended as a quit, in one request.
    #[test]
    fn session_quit_sends_the_request() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("quit.sock");
        let server = fake_daemon(&socket);
        let (client, _rx) = Client::connect(&socket, None).unwrap();
        client.session_quit();
        let seen: Vec<Request> = server.join().unwrap().into_iter().map(|m| m.request).collect();
        assert!(matches!(seen[0], Request::Hello { .. }), "{seen:?}");
        assert_eq!(seen[1], Request::SessionQuit);
    }

    #[test]
    fn the_peer_pid_is_the_process_at_the_other_end() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fake.sock");
        let server = fake_daemon(&socket);
        let (client, _rx) = Client::connect(&socket, None).unwrap();
        // The fake daemon runs in this test process.
        assert_eq!(client.peer_pid(), Some(std::process::id()));
        client.send(Request::ListSessions);
        server.join().unwrap();
    }

    #[test]
    fn wait_sent_returns_once_earlier_requests_are_written() {
        let _fork = fork_lock();
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("barrier.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        // Reads every frame until the client hangs up.
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut r = BufReader::new(stream);
            let mut seen = Vec::new();
            while let Ok(Some(msg)) = read_frame::<_, ClientMessage>(&mut r) {
                seen.push(msg.request);
            }
            seen
        });
        let (client, _rx) = Client::connect(&socket, None).unwrap();
        client.send(Request::Kill { session: protocol::SessionId(7) });
        assert!(client.wait_sent(Duration::from_secs(5)));
        // Dropping the client shuts the socket at once; the Kill was already on the wire.
        drop(client);
        let seen = server.join().unwrap();
        assert_eq!(seen.last(), Some(&Request::Kill { session: protocol::SessionId(7) }));
    }

    #[test]
    fn connect_replaces_stale_socket() {
        let _fork = fork_lock();
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("stale.sock");
        // A socket file nobody listens on.
        drop(UnixListener::bind(&socket).unwrap());
        assert!(socket.exists());
        Client::ensure_stale_socket_removed(&socket).unwrap();
        assert!(!socket.exists());
    }

    #[test]
    fn regular_file_at_socket_path_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("not-a-socket");
        std::fs::write(&socket, b"precious").unwrap();
        assert!(Client::ensure_stale_socket_removed(&socket).is_err());
        assert!(socket.exists());
    }

    #[test]
    fn reader_stops_pulling_output_past_the_unread_cap() {
        const FRAME: usize = 16 * 1024;
        const FRAMES: usize = 64;
        const CAP: usize = 64 * 1024;
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("flood.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        // Writes 1 MiB of output in 16 KiB frames, each filled with its own index.
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut w = BufWriter::new(stream);
            for i in 0..FRAMES {
                let msg = ServerMessage::Event(Event::Output { session: protocol::SessionId(1), bytes: vec![i as u8; FRAME] });
                write_frame(&mut w, &msg).unwrap();
            }
        });
        let (client, mut rx) = Client::connect_with_cap(&socket, CAP, None).unwrap();
        let mut got: Vec<u8> = Vec::new();
        let mut rounds = 0;
        while got.len() < FRAME * FRAMES {
            // Give the reader time to run as far ahead as the cap lets it.
            std::thread::sleep(std::time::Duration::from_millis(200));
            let mut batch = 0;
            while let Ok(msg) = rx.try_recv() {
                if let ServerMessage::Event(Event::Output { bytes, .. }) = msg {
                    batch += bytes.len();
                    got.extend_from_slice(&bytes);
                }
            }
            assert!(batch > 0, "nothing arrived after {} bytes", got.len());
            // The reader waits once the unread total passes the cap: at most one frame over it.
            assert!(batch <= CAP + FRAME, "{batch} unread bytes in the channel with a {CAP} byte cap");
            client.consumed_output(batch);
            rounds += 1;
        }
        assert!(rounds >= FRAMES * FRAME / (CAP + FRAME), "the cap never held the reader back");
        for (i, chunk) in got.chunks(FRAME).enumerate() {
            assert!(chunk.iter().all(|b| *b == i as u8), "frame {i} out of order");
        }
        server.join().unwrap();
    }

    #[test]
    fn server_hangup_ends_receiver_and_sets_reason() {
        let _fork = fork_lock();
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("hangup.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || drop(listener.accept().unwrap()));
        let (client, mut rx) = Client::connect(&socket, None).unwrap();
        server.join().unwrap();
        assert!(block_on(rx.next()).is_none());
        assert_eq!(client.disconnect_reason().as_deref(), Some("daemon closed the connection"));
    }
}
