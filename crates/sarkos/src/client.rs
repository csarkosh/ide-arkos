//! The UI's connection to sarkosd: a writer thread fed by `send`, a reader thread that forwards
//! every `ServerMessage` into a futures channel the GPUI side awaits. Nothing here blocks the
//! caller. The reader stops pulling frames while too much output sits unread in the channel, so
//! a flood backs up into the daemon, whose backpressure then pauses the PTY.

use anyhow::{Context, Result};
use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use protocol::codec::{read_frame, write_frame};
use protocol::{ClientMessage, Event, Request, ServerMessage, PROTOCOL_VERSION};
use std::io::{BufReader, BufWriter};
use std::io::ErrorKind;
use std::os::unix::fs::FileTypeExt;
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
    pub fn connect(socket: &Path) -> Result<(Client, UnboundedReceiver<ServerMessage>)> {
        Client::connect_with_cap(socket, CLIENT_UNREAD_CAP)
    }

    /// Connects with `cap` unread output bytes allowed in the channel before the reader waits.
    pub fn connect_with_cap(socket: &Path, cap: usize) -> Result<(Client, UnboundedReceiver<ServerMessage>)> {
        let stream = UnixStream::connect(socket).with_context(|| format!("connect {}", socket.display()))?;
        let reader_stream = stream.try_clone().context("clone socket")?;
        let shutdown_stream = stream.try_clone().context("clone socket")?;
        let unread: UnreadGate = Arc::new((Mutex::new(Unread::default()), Condvar::new()));
        let unread_for_reader = unread.clone();
        let (tx, rx) = channel::<Outgoing>();
        let (ev_tx, ev_rx): (UnboundedSender<ServerMessage>, UnboundedReceiver<ServerMessage>) = unbounded();
        std::thread::Builder::new()
            .name("sarkosd-writer".into())
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
            .name("sarkosd-reader".into())
            .spawn(move || {
                let mut r = BufReader::new(reader_stream);
                let (lock, cvar) = &*unread_for_reader;
                let reason = loop {
                    match read_frame::<_, ServerMessage>(&mut r) {
                        Ok(Some(msg)) => {
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
        let client = Client { tx, next_id: AtomicU32::new(1), disconnect_reason, unread, stream: shutdown_stream };
        client.send(Request::Hello {
            protocol_version: PROTOCOL_VERSION,
            ui_pid: std::process::id(),
            ui_exe: std::env::current_exe().ok(),
            ui_cwd: std::env::current_dir().unwrap_or_default(),
        });
        Ok((client, ev_rx))
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
    /// socket) and retries for three seconds.
    pub fn connect_or_start(socket: &Path, daemon_exe: &Path, daemon_args: &[String]) -> Result<(Client, UnboundedReceiver<ServerMessage>)> {
        if let Ok(c) = Client::connect(socket) {
            return Ok(c);
        }
        Client::ensure_stale_socket_removed(socket)?;
        let log = socket.with_file_name("sarkosd.log");
        if let Some(parent) = socket.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let log_file = std::fs::OpenOptions::new().create(true).append(true).open(&log).context("open daemon log")?;
        let mut cmd = Command::new(daemon_exe);
        cmd.arg("--socket").arg(socket).args(daemon_args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(log_file);
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

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::codec::{read_frame, write_frame};
    use protocol::{ClientMessage, Event, Request, Response};
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
        assert!(matches!(seen[0].request, Request::Hello { protocol_version: PROTOCOL_VERSION, .. }));
        assert_eq!(seen[1].request, Request::ListSessions);
    }

    #[test]
    fn wait_sent_returns_once_earlier_requests_are_written() {
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
        let (client, _rx) = Client::connect(&socket).unwrap();
        client.send(Request::Kill { session: protocol::SessionId(7) });
        assert!(client.wait_sent(Duration::from_secs(5)));
        // Dropping the client shuts the socket at once; the Kill was already on the wire.
        drop(client);
        let seen = server.join().unwrap();
        assert_eq!(seen.last(), Some(&Request::Kill { session: protocol::SessionId(7) }));
    }

    #[test]
    fn connect_replaces_stale_socket() {
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
        let (client, mut rx) = Client::connect_with_cap(&socket, CAP).unwrap();
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
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("hangup.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || drop(listener.accept().unwrap()));
        let (client, mut rx) = Client::connect(&socket).unwrap();
        server.join().unwrap();
        assert!(block_on(rx.next()).is_none());
        assert_eq!(client.disconnect_reason().as_deref(), Some("daemon closed the connection"));
    }
}
