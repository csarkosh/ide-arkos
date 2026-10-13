//! The UI's connection to sarkosd: a writer thread fed by `send`, a reader thread that forwards
//! every `ServerMessage` into a futures channel the GPUI side awaits. Nothing here blocks the
//! caller.

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
