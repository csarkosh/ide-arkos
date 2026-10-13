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
        // The socket file appears at bind(), slightly before listen(); wait until connecting works.
        while UnixStream::connect(&socket).is_err() {
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
                // Fire-and-forget `send`s leave their responses queued; skip those from earlier ids.
                ServerMessage::Response { id: rid, .. } if rid < id => continue,
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
    c.send(Request::Input { session: id, bytes: b"echo hello-$((40+2))\n".to_vec() });
    c.wait_for_output(id, "hello-42");
    assert_eq!(c.request(Request::Detach { session: id }), Response::Ok);
    drop(c);

    let mut c2 = d.connect();
    match c2.request(Request::Attach { session: id }) {
        Response::Attached { replay, alive } => {
            assert!(alive);
            assert!(String::from_utf8_lossy(&replay).contains("hello-42"), "replay was {:?}", String::from_utf8_lossy(&replay));
        }
        other => panic!("unexpected {other:?}"),
    }
    // Still live after reattach.
    c2.send(Request::Input { session: id, bytes: b"echo second-$((50+8))\n".to_vec() });
    c2.wait_for_output(id, "second-58");
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
    c.send(Request::Input { session: id, bytes: b"echo before-$((6*7)); exit\n".to_vec() });
    c.wait_for_output(id, "before-42");
    drop(c);
    std::thread::sleep(Duration::from_millis(300));
    let mut c2 = d.connect();
    match c2.request(Request::Attach { session: id }) {
        Response::Attached { replay, alive } => {
            assert!(!alive, "shell should have exited");
            assert!(String::from_utf8_lossy(&replay).contains("before-42"));
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
    c.send(Request::Input { session: id, bytes: b"echo OLDMARK-$((1+1)); printf 'abcdefghij%.0s' 1 2 3 4 5 6 7 8 9 10; echo NEWMARK-$((3+4))\n".to_vec() });
    c.wait_for_output(id, "NEWMARK-7");
    c.request(Request::Detach { session: id });
    drop(c);
    let mut c2 = d.connect();
    match c2.request(Request::Attach { session: id }) {
        Response::Attached { replay, .. } => {
            assert!(replay.len() <= 64);
            let text = String::from_utf8_lossy(&replay);
            assert!(text.contains("NEWMARK-7"), "newest output missing: {text:?}");
            assert!(!text.contains("OLDMARK-2"), "oldest output should have been dropped: {text:?}");
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

#[test]
fn socket_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let d = Daemon::start(&[]);
    let mode = std::fs::metadata(&d.socket).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}
