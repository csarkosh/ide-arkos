//! Drives the real arkosd binary over a temp socket.
use protocol::codec::{read_frame, write_frame};
use protocol::{AgentRecord, ClientMessage, Event, Request, Response, ServerMessage, SessionId, PROTOCOL_VERSION};
use std::io::{BufReader, BufWriter};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Daemon {
    child: Child,
    socket: PathBuf,
    dir: Option<tempfile::TempDir>,
}

impl Daemon {
    fn start(extra: &[&str]) -> Daemon {
        Daemon::start_in(tempfile::tempdir().unwrap(), extra)
    }

    /// A daemon whose socket, and so its sessions dir, is in `dir`: a later daemon can share it.
    fn start_in(dir: tempfile::TempDir, extra: &[&str]) -> Daemon {
        let socket = dir.path().join("d.sock");
        // No test reaches GitHub: polling is off unless a test turns it on (later arguments win).
        let child = Command::new(env!("CARGO_BIN_EXE_arkosd"))
            .arg("--socket")
            .arg(&socket)
            .args(["--check-every-secs", "0"])
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
        Daemon { child, socket, dir: Some(dir) }
    }

    fn path(&self) -> &std::path::Path {
        self.dir.as_ref().unwrap().path()
    }

    fn sessions_dir(&self) -> PathBuf {
        self.socket.with_file_name("sessions")
    }

    /// SIGTERM, as a shutdown sends it. The daemon must exit 0 within five seconds. The dir is
    /// handed back so a successor can recover from it.
    fn sigterm(mut self) -> tempfile::TempDir {
        // SAFETY: the pid is our unreaped child.
        unsafe { libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "arkosd exited with {status} on SIGTERM");
                break;
            }
            assert!(Instant::now() < deadline, "arkosd did not exit within 5 s of SIGTERM");
            std::thread::sleep(Duration::from_millis(20));
        }
        self.dir.take().unwrap()
    }

    fn connect(&self) -> Conn {
        self.connect_as(std::process::id())
    }

    /// Connects without saying Hello.
    fn connect_raw(&self) -> Conn {
        let stream = UnixStream::connect(&self.socket).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        Conn { r: BufReader::new(stream.try_clone().unwrap()), w: BufWriter::new(stream), next: 1 }
    }

    /// Connects and says Hello with `ui_pid` as the UI's pid.
    fn connect_as(&self, ui_pid: u32) -> Conn {
        let stream = UnixStream::connect(&self.socket).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut c = Conn { r: BufReader::new(stream.try_clone().unwrap()), w: BufWriter::new(stream), next: 1 };
        let r = c.request(Request::Hello { protocol_version: PROTOCOL_VERSION, ui_pid, ui_exe: None, ui_cwd: std::env::temp_dir() });
        assert!(matches!(r, Response::Ok | Response::Hello { .. }), "{r:?}");
        c
    }

    /// The daemon's pid, which an exec-in-place must keep.
    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Connects again after an upgrade: the socket file stays, the listener passes through the
    /// exec, so this succeeds once the new image accepts.
    fn reconnect(&self) -> Conn {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if UnixStream::connect(&self.socket).is_ok() {
                return self.connect();
            }
            assert!(Instant::now() < deadline, "the resumed daemon never accepted");
            std::thread::sleep(Duration::from_millis(20));
        }
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
        read_frame(&mut self.r).expect("daemon went silent or the frame was bad").expect("daemon closed the connection")
    }

    fn set_read_timeout(&mut self, timeout: Option<Duration>) {
        self.r.get_ref().set_read_timeout(timeout).unwrap();
    }

    /// Reads until the daemon closes the connection (an upgrade's exec), within `timeout`.
    fn expect_disconnect(&mut self, timeout: Duration) {
        self.set_read_timeout(Some(timeout));
        loop {
            match read_frame::<_, ServerMessage>(&mut self.r) {
                Ok(Some(ServerMessage::Response { response: Response::Error(e), .. })) => panic!("upgrade refused: {e}"),
                Ok(Some(_)) => continue,
                Ok(None) | Err(_) => return,
            }
        }
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

    fn attach(&mut self, session: SessionId) {
        assert!(matches!(self.request(Request::Attach { session }), Response::Attached { .. }));
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
    // Ids start from the clock, not 1, so they stay unique across daemon restarts.
    assert!(id.0 > 1 << 56, "session id {} does not come from the clock", id.0);
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
    c.send(Request::Input { session: id, bytes: b"echo id=$ARKOS_SESSION_ID\n".to_vec() });
    c.wait_for_output(id, &format!("id={}", id.0));
}

#[test]
fn socket_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let d = Daemon::start(&[]);
    let mode = std::fs::metadata(&d.socket).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

/// Writes the "UI executable" the watchdog relaunches: a script that appends its working
/// directory to the returned marker file, one line per launch in a single write, so a test can
/// count launches. Wait for it with `marker_written`. Returns (script, marker).
fn relaunch_script(dir: &std::path::Path) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let marker = dir.join("relaunched");
    let script = dir.join("arkos");
    std::fs::write(&script, format!("#!/bin/sh\npwd -P >> '{m}'\n", m = marker.display())).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    (script, marker)
}

/// Whether a launch has finished writing its line: the appending open creates the file a moment
/// before the write, so existing is not enough.
fn marker_written(marker: &std::path::Path) -> bool {
    std::fs::read_to_string(marker).is_ok_and(|s| s.ends_with('\n'))
}

#[test]
fn watchdog_kills_a_silent_ui_and_relaunches_it() {
    let d = Daemon::start(&["--watchdog-secs", "2"]);
    let dir = tempfile::tempdir().unwrap();
    // The UI's working directory, distinct from the daemon's, which is the test's own.
    let ui_cwd = tempfile::tempdir().unwrap();
    // The "UI" is a sleep; the "relaunch" is the marker script.
    let (script, marker) = relaunch_script(dir.path());
    let mut fake_ui = Command::new("/bin/sleep").arg("60").spawn().unwrap();

    let stream = UnixStream::connect(&d.socket).unwrap();
    let mut w = BufWriter::new(stream.try_clone().unwrap());
    let mut r = BufReader::new(stream);
    write_frame(&mut w, &ClientMessage { id: 1, request: Request::Hello { protocol_version: PROTOCOL_VERSION, ui_pid: fake_ui.id(), ui_exe: Some(script.clone()), ui_cwd: ui_cwd.path().to_path_buf() } }).unwrap();
    let _: ServerMessage = read_frame(&mut r).unwrap().unwrap();
    // No heartbeats. Within 2 s plus one scan the daemon must act.
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Ok(Some(_)) = fake_ui.try_wait() {
            break;
        }
        assert!(Instant::now() < deadline, "watchdog never killed the silent UI");
        std::thread::sleep(Duration::from_millis(100));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker_written(&marker) {
        assert!(Instant::now() < deadline, "watchdog never relaunched the UI");
        std::thread::sleep(Duration::from_millis(50));
    }
    let relaunched_in = std::fs::read_to_string(&marker).unwrap();
    assert_eq!(relaunched_in.trim_end(), ui_cwd.path().canonicalize().unwrap().to_str().unwrap(), "relaunched outside the UI's cwd");
}

#[test]
fn watchdog_leaves_a_heartbeating_ui_alone() {
    let d = Daemon::start(&["--watchdog-secs", "2"]);
    let mut fake_ui = Command::new("/bin/sleep").arg("60").spawn().unwrap();
    let stream = UnixStream::connect(&d.socket).unwrap();
    let mut w = BufWriter::new(stream.try_clone().unwrap());
    let mut r = BufReader::new(stream);
    write_frame(&mut w, &ClientMessage { id: 1, request: Request::Hello { protocol_version: PROTOCOL_VERSION, ui_pid: fake_ui.id(), ui_exe: None, ui_cwd: std::env::temp_dir() } }).unwrap();
    let _: ServerMessage = read_frame(&mut r).unwrap().unwrap();
    for seq in 0..5u64 {
        std::thread::sleep(Duration::from_millis(800));
        write_frame(&mut w, &ClientMessage { id: 2 + seq as u32, request: Request::Heartbeat { seq } }).unwrap();
        let _: ServerMessage = read_frame(&mut r).unwrap().unwrap();
    }
    assert!(fake_ui.try_wait().unwrap().is_none(), "UI was killed despite heartbeats");
    let _ = fake_ui.kill();
}

/// Bytes of `yes` output each flood test asks for: one million `y\n` lines.
const FLOOD_BYTES: usize = 2_000_000;

#[test]
fn continuous_reader_survives_a_flood() {
    let d = Daemon::start(&[]);
    let mut c = d.connect();
    let id = c.create("/bin/sh");
    assert!(matches!(c.request(Request::Attach { session: id }), Response::Attached { .. }));
    // The arithmetic keeps the marker out of the echoed command line. `-onlcr` stops the line
    // discipline rewriting `\n`, which on macOS can also emit a stray `\r` when its output blocks.
    let cmd = format!("stty -onlcr; yes | head -c {FLOOD_BYTES}; echo FLOOD-DONE-$((40+2))\n");
    c.send(Request::Input { session: id, bytes: cmd.into_bytes() });
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut stream = Vec::new();
    let mut largest = 0;
    loop {
        assert!(Instant::now() < deadline, "flood did not finish");
        // `next_message` panics if the daemon closes the connection.
        if let ServerMessage::Event(Event::Output { session, bytes }) = c.next_message() {
            assert_eq!(session, id);
            largest = largest.max(bytes.len());
            stream.extend_from_slice(&bytes);
            if stream.windows(13).any(|w| w == b"FLOOD-DONE-42") {
                break;
            }
        }
    }
    // Everything between the echoed command line (the line discipline and the shell can both
    // echo it, so take the last) and the marker must be the flood, complete and in order.
    let text = String::from_utf8_lossy(&stream);
    let start = text.rfind("$((40+2))\r\n").expect("echoed command line") + "$((40+2))\r\n".len();
    let end = text.find("FLOOD-DONE-42").unwrap();
    let flood = &text[start..end];
    assert_eq!(flood.len(), FLOOD_BYTES, "flood bytes lost or duplicated");
    assert!(flood == "y\n".repeat(FLOOD_BYTES / 2), "flood bytes out of order");
    assert!(largest > 64 * 1024, "output was never coalesced: largest frame {largest} bytes");
    // Still connected.
    assert!(matches!(c.request(Request::ListSessions), Response::Sessions(_)));
}

#[test]
fn non_reading_client_is_dropped_past_the_byte_cap() {
    use std::io::Read;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    const CAP: usize = 1 << 20;
    let d = Daemon::start(&["--client-pending-bytes", &CAP.to_string()]);
    let mut reader = d.connect();
    let id = reader.create("/bin/sh");
    reader.request(Request::Attach { session: id });

    // A client that attaches and then never reads. It sends no Hello, so it is a tool, not a UI:
    // only those are dropped for being slow.
    let mut stalled = UnixStream::connect(&d.socket).unwrap();
    // Set now: macOS refuses socket options once the daemon has shut the connection down.
    stalled.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    {
        let mut w = BufWriter::new(stalled.try_clone().unwrap());
        write_frame(&mut w, &ClientMessage { id: 1, request: Request::Attach { session: id } }).unwrap();
    }
    std::thread::sleep(Duration::from_millis(200));

    let started = Instant::now();
    let cmd = format!("yes | head -c {FLOOD_BYTES}; echo CAP-DONE-$((40+2))\n");
    reader.send(Request::Input { session: id, bytes: cmd.into_bytes() });
    // The reading client drains on its own thread, counting what it has received. It looks for
    // the marker in a short tail only, so it keeps pace with the PTY.
    let received = Arc::new(AtomicUsize::new(0));
    let counter = received.clone();
    let reading = std::thread::spawn(move || {
        let mut tail: Vec<u8> = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if let ServerMessage::Event(Event::Output { bytes, .. }) = reader.next_message() {
                counter.fetch_add(bytes.len(), Ordering::Relaxed);
                tail.extend_from_slice(&bytes);
                if tail.windows(11).any(|w| w == b"CAP-DONE-42") {
                    return true;
                }
                let keep = tail.len().saturating_sub(16);
                tail.drain(..keep);
            }
        }
        false
    });
    // Once the PTY has produced the cap plus a margin for socket buffers, the stalled client's
    // pending output has passed the cap.
    while received.load(Ordering::Relaxed) < CAP + CAP / 2 {
        assert!(started.elapsed() < Duration::from_secs(5), "flood too slow to test the cap");
        std::thread::sleep(Duration::from_millis(10));
    }
    // The stalled client was shut down: draining what reached its socket ends in EOF or an error.
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match stalled.read(&mut buf) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => break,
        }
        assert!(started.elapsed() < Duration::from_secs(5), "stalled client was not dropped within 5 s");
    }
    assert!(started.elapsed() < Duration::from_secs(5), "stalled client was not dropped within 5 s");
    // The reading client on the same session got the whole flood.
    assert!(reading.join().unwrap(), "reading client never saw the end of the flood");
}

/// Reads at most 16 KiB per call and sleeps 20 ms after each: about 0.8 MB/s, well under what
/// a `yes` flood produces, so the daemon must hold the PTY back to keep this client.
struct SlowStream(UnixStream);

impl std::io::Read for SlowStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = buf.len().min(16 * 1024);
        let got = self.0.read(&mut buf[..n])?;
        std::thread::sleep(Duration::from_millis(20));
        Ok(got)
    }
}

#[test]
fn backpressure_slows_the_pty_instead_of_dropping() {
    // Without backpressure this reader would pass the 1 MiB cap within about a second.
    let d = Daemon::start(&["--client-pending-bytes", "1048576", "--subscriber-high-water", "262144"]);
    let mut c = d.connect();
    let id = c.create("/bin/sh");
    assert!(matches!(c.request(Request::Attach { session: id }), Response::Attached { .. }));
    let stream = c.r.get_ref().try_clone().unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut slow = BufReader::new(SlowStream(stream));
    let bytes = 2 * FLOOD_BYTES;
    c.send(Request::Input { session: id, bytes: format!("stty -onlcr; yes | head -c {bytes}; echo BP-DONE-$((40+2))\n").into_bytes() });
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut stream_bytes = Vec::new();
    loop {
        assert!(Instant::now() < deadline, "flood did not finish");
        match read_frame::<_, ServerMessage>(&mut slow) {
            Ok(Some(ServerMessage::Event(Event::Output { bytes, .. }))) => {
                stream_bytes.extend_from_slice(&bytes);
                let tail = &stream_bytes[stream_bytes.len().saturating_sub(bytes.len() + 16)..];
                if tail.windows(10).any(|w| w == b"BP-DONE-42") {
                    break;
                }
            }
            Ok(Some(_)) => {}
            Ok(None) => panic!("slow client was disconnected"),
            Err(e) => panic!("slow client read failed: {e}"),
        }
    }
    let text = String::from_utf8_lossy(&stream_bytes);
    let start = text.rfind("$((40+2))\r\n").expect("echoed command line") + "$((40+2))\r\n".len();
    let end = text.find("BP-DONE-42").unwrap();
    assert_eq!(end - start, bytes, "flood bytes lost or duplicated");
    assert!(text[start..end] == "y\n".repeat(bytes / 2), "flood bytes out of order");
}

#[test]
fn stalled_client_is_dropped_after_the_stall_timeout() {
    use std::io::Read;
    let d = Daemon::start(&["--subscriber-high-water", "262144", "--subscriber-stall-secs", "1"]);
    let mut reader = d.connect();
    let id = reader.create("/bin/sh");
    reader.request(Request::Attach { session: id });
    let mut stalled = UnixStream::connect(&d.socket).unwrap();
    // Set now: macOS refuses socket options once the daemon has shut the connection down.
    stalled.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    {
        let mut w = BufWriter::new(stalled.try_clone().unwrap());
        write_frame(&mut w, &ClientMessage { id: 1, request: Request::Attach { session: id } }).unwrap();
    }
    std::thread::sleep(Duration::from_millis(200));

    let started = Instant::now();
    reader.send(Request::Input { session: id, bytes: format!("yes | head -c {FLOOD_BYTES}; echo STALL-DONE-$((40+2))\n").into_bytes() });
    let reading = std::thread::spawn(move || {
        let mut tail: Vec<u8> = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if let ServerMessage::Event(Event::Output { bytes, .. }) = reader.next_message() {
                tail.extend_from_slice(&bytes);
                if tail.windows(13).any(|w| w == b"STALL-DONE-42") {
                    return true;
                }
                let keep = tail.len().saturating_sub(16);
                tail.drain(..keep);
            }
        }
        false
    });
    // The stalled client pauses the PTY almost at once and is dropped one stall timeout later.
    // Reading it any earlier would make it a reader, so wait first.
    std::thread::sleep(Duration::from_secs(2));
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match stalled.read(&mut buf) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => break,
        }
        assert!(started.elapsed() < Duration::from_secs(3), "stalled client was not dropped within 3 s");
    }
    assert!(started.elapsed() < Duration::from_secs(3), "stalled client was not dropped within 3 s");
    // The PTY resumed: the reading client gets the rest of the flood.
    assert!(reading.join().unwrap(), "reading client never saw the end of the flood");
}

#[test]
fn hello_with_pid_zero_or_out_of_range_is_never_signalled() {
    let d = Daemon::start(&["--watchdog-secs", "1"]);
    let mut zero = d.connect_as(0);
    // u32::MAX is -1 as a pid_t: kill(-1, SIGKILL) would end every process this user owns.
    let mut negative = d.connect_as(u32::MAX);
    // Two watchdog periods with no heartbeat: neither pid may be treated as a UI.
    std::thread::sleep(Duration::from_secs(3));
    assert!(matches!(zero.request(Request::ListSessions), Response::Sessions(_)), "daemon stopped answering");
    assert!(matches!(negative.request(Request::ListSessions), Response::Sessions(_)), "daemon stopped answering");
}

#[test]
fn frozen_ui_with_busy_session_is_relaunched() {
    // A stall timeout and pending cap this small would drop any other client within about a
    // second, well before the 3 s watchdog acts; a UI must instead be left to the watchdog.
    let d = Daemon::start(&[
        "--watchdog-secs",
        "3",
        "--subscriber-high-water",
        "262144",
        "--subscriber-stall-secs",
        "1",
        "--client-pending-bytes",
        "1048576",
    ]);
    let dir = tempfile::tempdir().unwrap();
    let (script, marker) = relaunch_script(dir.path());
    let mut fake_ui = Command::new("/bin/sleep").arg("60").spawn().unwrap();

    let stream = UnixStream::connect(&d.socket).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut ui = Conn { r: BufReader::new(stream.try_clone().unwrap()), w: BufWriter::new(stream), next: 1 };
    let hello = Request::Hello { protocol_version: PROTOCOL_VERSION, ui_pid: fake_ui.id(), ui_exe: Some(script), ui_cwd: dir.path().to_path_buf() };
    let r = ui.request(hello);
    assert!(matches!(r, Response::Ok | Response::Hello { .. }), "{r:?}");
    let id = ui.create("/bin/sh");
    assert!(matches!(ui.request(Request::Attach { session: id }), Response::Attached { alive: true, .. }));
    // Busy for good: the PTY fills the UI's backlog and stays paused on it.
    ui.send(Request::Input { session: id, bytes: b"yes\n".to_vec() });
    // From here the UI neither reads nor heartbeats.
    let frozen = Instant::now();

    let deadline = frozen + Duration::from_secs(10);
    loop {
        if let Ok(Some(_)) = fake_ui.try_wait() {
            break;
        }
        assert!(Instant::now() < deadline, "watchdog never killed the frozen UI");
        std::thread::sleep(Duration::from_millis(100));
    }
    while !marker_written(&marker) {
        assert!(Instant::now() < deadline, "watchdog never relaunched the frozen UI");
        std::thread::sleep(Duration::from_millis(50));
    }
    // The session survives its UI (spec 10). Pid 0 makes this client a tool, not a UI, so the
    // watchdog leaves the test process alone.
    let mut tool = d.connect_as(0);
    match tool.request(Request::ListSessions) {
        Response::Sessions(list) => assert!(list.iter().find(|s| s.id == id).expect("session listed").alive, "session died with its UI"),
        other => panic!("unexpected {other:?}"),
    }
    drop(ui);
}

#[test]
fn kill_wakes_a_pty_paused_by_a_stalled_client() {
    // The default 10 s stall timeout keeps the PTY paused for the whole test unless kill wakes it.
    let d = Daemon::start(&["--subscriber-high-water", "262144"]);
    let mut c = d.connect();
    let id = c.create("/bin/sh");
    c.request(Request::Attach { session: id });
    let stalled = UnixStream::connect(&d.socket).unwrap();
    {
        let mut w = BufWriter::new(stalled.try_clone().unwrap());
        write_frame(&mut w, &ClientMessage { id: 1, request: Request::Attach { session: id } }).unwrap();
    }
    std::thread::sleep(Duration::from_millis(200));
    c.send(Request::Input { session: id, bytes: b"yes\n".to_vec() });
    // Let the stalled client pause the PTY.
    std::thread::sleep(Duration::from_millis(1000));
    let killed = Instant::now();
    c.send(Request::Kill { session: id });
    loop {
        assert!(killed.elapsed() < Duration::from_secs(5), "kill did not end a paused session");
        if let ServerMessage::Event(Event::Exited { session, .. }) = c.next_message() {
            assert_eq!(session, id);
            break;
        }
    }
    drop(stalled);
}

#[test]
fn hello_below_the_minimum_protocol_is_refused() {
    let d = Daemon::start(&[]);
    let stream = UnixStream::connect(&d.socket).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut c = Conn { r: BufReader::new(stream.try_clone().unwrap()), w: BufWriter::new(stream), next: 1 };
    let hello = Request::Hello { protocol_version: 0, ui_pid: 0, ui_exe: None, ui_cwd: std::env::temp_dir() };
    match c.request(hello) {
        Response::Error(e) => {
            assert!(e.contains("client 0") && e.contains("this window is older than arkosd"), "{e}");
            assert!(!e.contains("pkill"), "restarting the daemon cannot help an older window: {e}");
        }
        other => panic!("unexpected {other:?}"),
    }
    // Then the daemon hangs up.
    assert!(matches!(read_frame::<_, ServerMessage>(&mut c.r), Ok(None) | Err(_)), "daemon kept the connection open");
    // And keeps serving everyone else.
    let mut ok = d.connect_as(0);
    assert!(matches!(ok.request(Request::ListSessions), Response::Sessions(_)));
}

/// Waits up to `timeout` for an `Exited` event for `session`, returning its code.
fn wait_for_exit(conn: &mut Conn, session: SessionId, timeout: Duration) -> Option<i32> {
    conn.set_read_timeout(Some(Duration::from_secs(5)));
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let ServerMessage::Event(Event::Exited { session: s, code }) = conn.next_message() {
            if s == session {
                conn.set_read_timeout(None);
                return code;
            }
        }
    }
    panic!("no Exited event for session {} within {timeout:?}", session.0);
}

#[test]
fn exit_is_reported_with_its_code_while_a_background_job_lingers() {
    // The background `sleep 2` may keep the PTY slave open; the exit and its code must arrive
    // regardless. (On macOS EOF usually follows quickly; the reaper path has a unit test.)
    let d = Daemon::start(&["--reaper-millis", "100"]);
    let mut c = d.connect();
    let sid = c.create("/bin/sh");
    assert!(matches!(c.request(Request::Attach { session: sid }), Response::Attached { .. }));
    c.send(Request::Input { session: sid, bytes: b"sleep 2 & exit 3\n".to_vec() });
    let code = wait_for_exit(&mut c, sid, Duration::from_millis(1800));
    assert_eq!(code, Some(3));
    match c.request(Request::ListSessions) {
        Response::Sessions(list) => {
            let info = list.iter().find(|i| i.id == sid).expect("session listed");
            assert!(!info.alive);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn kill_on_a_dead_session_is_ok_and_sends_no_signal() {
    let d = Daemon::start(&["--reaper-millis", "100"]);
    let mut c = d.connect();
    let sid = c.create("/bin/sh");
    assert!(matches!(c.request(Request::Attach { session: sid }), Response::Attached { .. }));
    c.send(Request::Input { session: sid, bytes: b"exit 0\n".to_vec() });
    wait_for_exit(&mut c, sid, Duration::from_secs(3));
    // The pid is reaped by now; a Kill must not touch it (it may belong to another process).
    assert_eq!(c.request(Request::Kill { session: sid }), Response::Ok);
    assert_eq!(c.request(Request::Kill { session: sid }), Response::Ok);
}

#[test]
fn dead_session_is_reaped_after_the_ttl_once_nobody_is_attached() {
    let d = Daemon::start(&["--reaper-millis", "100", "--dead-session-secs", "1"]);
    let mut c = d.connect();
    let sid = c.create("/bin/sh");
    assert!(matches!(c.request(Request::Attach { session: sid }), Response::Attached { .. }));
    c.send(Request::Input { session: sid, bytes: b"exit\n".to_vec() });
    wait_for_exit(&mut c, sid, Duration::from_secs(3));
    // Still attached: the dead session stays listed so a UI can show "session ended".
    std::thread::sleep(Duration::from_millis(1500));
    match c.request(Request::ListSessions) {
        Response::Sessions(list) => assert!(list.iter().any(|i| i.id == sid), "reaped while attached"),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(c.request(Request::Detach { session: sid }), Response::Ok);
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        match c.request(Request::ListSessions) {
            Response::Sessions(list) if !list.iter().any(|i| i.id == sid) => break,
            Response::Sessions(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            Response::Sessions(_) => panic!("dead session never reaped"),
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[test]
fn hello_with_a_foreign_ui_exe_is_not_relaunched() {
    // A client may only ask the watchdog to relaunch a binary named `arkos`.
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("marker");
    let script = dir.path().join("other");
    std::fs::write(&script, format!("#!/bin/sh\ntouch {}\n", marker.display())).unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let d = Daemon::start(&["--watchdog-secs", "1"]);
    let mut fake_ui = std::process::Command::new("/bin/sleep").arg("60").spawn().unwrap();
    let mut c = d.connect_raw();
    c.send(Request::Hello {
        protocol_version: protocol::PROTOCOL_VERSION,
        ui_pid: fake_ui.id(),
        ui_exe: Some(script.clone()),
        ui_cwd: dir.path().to_path_buf(),
    });
    std::thread::sleep(Duration::from_millis(3500));
    assert!(fake_ui.try_wait().unwrap().is_some(), "silent UI was not killed");
    assert!(!marker.exists(), "a binary not named arkos was relaunched");
    let _ = fake_ui.wait();
}

#[test]
fn daemon_refuses_a_regular_file_at_the_socket_path() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("arkosd.sock");
    std::fs::write(&socket, "not a socket").unwrap();
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_arkosd"))
        .args(["--socket", socket.to_str().unwrap(), "--watchdog-secs", "0"])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success());
    assert_eq!(std::fs::read_to_string(&socket).unwrap(), "not a socket", "the file was removed");
}

#[test]
fn kill_escalates_to_sigkill_when_the_shell_ignores_sighup() {
    let d = Daemon::start(&["--reaper-millis", "100"]);
    let mut c = d.connect();
    let sid = c.create("/bin/sh");
    assert!(matches!(c.request(Request::Attach { session: sid }), Response::Attached { .. }));
    c.send(Request::Input { session: sid, bytes: b"trap '' HUP; sleep 30\n".to_vec() });
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(c.request(Request::Kill { session: sid }), Response::Ok);
    wait_for_exit(&mut c, sid, Duration::from_secs(3));
}

#[test]
fn hello_version_1_gets_ok_and_version_2_gets_the_daemon_version() {
    let d = Daemon::start(&[]);
    let mut c = d.connect_raw();
    let r = c.request(Request::Hello { protocol_version: 1, ui_pid: std::process::id(), ui_exe: None, ui_cwd: std::env::temp_dir() });
    assert_eq!(r, Response::Ok, "a milestone 2b window keeps working");
    let mut c2 = d.connect_raw();
    match c2.request(Request::Hello { protocol_version: 2, ui_pid: std::process::id(), ui_exe: None, ui_cwd: std::env::temp_dir() }) {
        Response::Hello { daemon_version, protocol_min, protocol_max } => {
            assert!(daemon_version.starts_with("arkosd "), "{daemon_version}");
            assert_eq!((protocol_min, protocol_max), (1, PROTOCOL_VERSION));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn hello_with_a_newer_protocol_is_refused_with_the_pkill_hint() {
    let d = Daemon::start(&[]);
    let mut c = d.connect_raw();
    match c.request(Request::Hello { protocol_version: PROTOCOL_VERSION + 1, ui_pid: std::process::id(), ui_exe: None, ui_cwd: std::env::temp_dir() }) {
        Response::Error(e) => {
            assert!(e.starts_with("protocol version mismatch"), "the window's is_final matches this: {e}");
            assert!(e.contains("this arkosd is older than the window") && e.contains("pkill -x arkosd (ends every session)"), "{e}");
            assert!(!e.contains("Update"), "Update now cannot reach a daemon that refused the window: {e}");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn version_and_check_handover_flags() {
    let out = Command::new(env!("CARGO_BIN_EXE_arkosd")).arg("--version").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("arkosd ") && text.contains('(') && text.trim_end().ends_with(')'), "{text:?}");
    let out = Command::new(env!("CARGO_BIN_EXE_arkosd")).arg("--check-handover").output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "handover 1", "exactly what the current main's daemon expects");
    let out = Command::new(env!("CARGO_BIN_EXE_arkosd")).arg("--handover-formats").output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "1 2");
}

/// A fake `arkosd` at `dir/arkosd` whose body is `script`, for the validation paths.
fn fake_arkosd(dir: &std::path::Path, script: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let exe = dir.join("arkosd");
    std::fs::write(&exe, format!("#!/bin/sh\n{script}\n")).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    exe
}

fn parent_pid(pid: u32) -> u32 {
    let out = Command::new("ps").args(["-o", "ppid=", "-p", &pid.to_string()]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

#[test]
fn upgrade_onto_the_same_binary_keeps_sessions_the_pid_and_parenthood() {
    let d = Daemon::start(&["--reaper-millis", "100"]);
    let mut c = d.connect();
    let sid = c.create("/bin/sh");
    assert!(matches!(c.request(Request::Attach { session: sid }), Response::Attached { .. }));
    c.send(Request::Input { session: sid, bytes: b"echo marker-$((20+1))\n".to_vec() });
    c.wait_for_output(sid, "marker-21");
    let shell_pid = match c.request(Request::ListSessions) {
        Response::Sessions(list) => list.iter().find(|s| s.id == sid).unwrap().pid.unwrap(),
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(parent_pid(shell_pid), d.pid(), "the shell is the daemon's child before");
    // The journal writes only into an updates folder the poll made; polling is off here.
    std::fs::create_dir_all(d.path().join("updates")).unwrap();
    c.send(Request::Upgrade { exe: PathBuf::from(env!("CARGO_BIN_EXE_arkosd")) });
    c.expect_disconnect(Duration::from_secs(10));
    let mut c = d.reconnect();
    let journal = std::fs::read_to_string(d.path().join("updates/journal.log")).unwrap();
    let exec = journal.find(&format!("exec {} (upgrade from arkosd ", env!("CARGO_BIN_EXE_arkosd"))).unwrap_or_else(|| panic!("{journal}"));
    let resumed = journal.find(&format!("resumed as arkosd {} (", env!("CARGO_PKG_VERSION"))).unwrap_or_else(|| panic!("{journal}"));
    assert!(exec < resumed, "{journal}");
    assert!(journal[resumed..].lines().next().unwrap().contains(&format!("(pid {})", d.pid())), "{journal}");
    let listed = match c.request(Request::ListSessions) {
        Response::Sessions(list) => list.into_iter().find(|s| s.id == sid).expect("the session survived"),
        other => panic!("unexpected {other:?}"),
    };
    assert!(listed.alive);
    assert_eq!(listed.pid, Some(shell_pid));
    assert_eq!(parent_pid(shell_pid), d.pid(), "same daemon pid, still the parent after the exec");
    match c.request(Request::Attach { session: sid }) {
        Response::Attached { replay, alive } => {
            assert!(alive);
            assert!(String::from_utf8_lossy(&replay).contains("marker-21"), "the replay crossed the swap");
        }
        other => panic!("unexpected {other:?}"),
    }
    c.send(Request::Input { session: sid, bytes: b"echo marker-$((40+2))\n".to_vec() });
    c.wait_for_output(sid, "marker-42");
    c.send(Request::Input { session: sid, bytes: b"exit 5\n".to_vec() });
    assert_eq!(wait_for_exit(&mut c, sid, Duration::from_secs(5)), Some(5), "the new image reaps the shell");
}

#[test]
fn output_printed_during_the_swap_is_not_lost() {
    let d = Daemon::start(&[]);
    let mut c = d.connect();
    let sid = c.create("/bin/sh");
    assert!(matches!(c.request(Request::Attach { session: sid }), Response::Attached { .. }));
    c.send(Request::Input { session: sid, bytes: b"sleep 0.3; echo late-$((1+1))\n".to_vec() });
    c.send(Request::Upgrade { exe: PathBuf::from(env!("CARGO_BIN_EXE_arkosd")) });
    c.expect_disconnect(Duration::from_secs(10));
    let mut c = d.reconnect();
    let replay = match c.request(Request::Attach { session: sid }) {
        Response::Attached { replay, .. } => replay,
        other => panic!("unexpected {other:?}"),
    };
    if !String::from_utf8_lossy(&replay).contains("late-2") {
        c.wait_for_output(sid, "late-2");
    }
}

#[test]
fn exit_during_the_swap_is_reported_after_resume() {
    let d = Daemon::start(&["--reaper-millis", "100"]);
    let mut c = d.connect();
    let sid = c.create("/bin/sh");
    assert!(matches!(c.request(Request::Attach { session: sid }), Response::Attached { .. }));
    c.send(Request::Input { session: sid, bytes: b"sleep 0.2; exit 9\n".to_vec() });
    c.send(Request::Upgrade { exe: PathBuf::from(env!("CARGO_BIN_EXE_arkosd")) });
    c.expect_disconnect(Duration::from_secs(10));
    let mut c = d.reconnect();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match c.request(Request::ListSessions) {
            Response::Sessions(list) => {
                let s = list.iter().find(|s| s.id == sid).expect("listed");
                if !s.alive {
                    break;
                }
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(Instant::now() < deadline, "the exit during the swap was never noticed");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn upgrade_with_a_missing_binary_is_refused() {
    let d = Daemon::start(&[]);
    let mut c = d.connect();
    let sid = c.create("/bin/sh");
    assert!(matches!(c.request(Request::Attach { session: sid }), Response::Attached { .. }));
    match c.request(Request::Upgrade { exe: std::env::temp_dir().join("no-such-dir").join("arkosd") }) {
        Response::Error(e) => assert!(e.contains("arkosd"), "{e}"),
        other => panic!("unexpected {other:?}"),
    }
    match c.request(Request::Upgrade { exe: PathBuf::from("/bin/sh") }) {
        Response::Error(e) => assert!(e.contains("named arkosd"), "{e}"),
        other => panic!("unexpected {other:?}"),
    }
    c.send(Request::Input { session: sid, bytes: b"echo still-$((1+1))\n".to_vec() });
    c.wait_for_output(sid, "still-2");
}

#[test]
fn upgrade_with_a_binary_that_fails_the_check_keeps_the_old_daemon() {
    let d = Daemon::start(&[]);
    let dir = tempfile::tempdir().unwrap();
    let exe = fake_arkosd(dir.path(), "exit 1");
    let mut c = d.connect();
    let sid = c.create("/bin/sh");
    assert!(matches!(c.request(Request::Attach { session: sid }), Response::Attached { .. }));
    match c.request(Request::Upgrade { exe }) {
        Response::Error(e) => assert!(e.contains("check-handover"), "{e}"),
        other => panic!("unexpected {other:?}"),
    }
    let exe = fake_arkosd(dir.path(), "echo handover 999");
    match c.request(Request::Upgrade { exe }) {
        Response::Error(e) => assert!(e.contains("handover 999") || e.contains("format"), "{e}"),
        other => panic!("unexpected {other:?}"),
    }
    c.send(Request::Input { session: sid, bytes: b"echo still-$((2+2))\n".to_vec() });
    c.wait_for_output(sid, "still-4");
}

/// The current main's binary answers `handover 1` and exits 2 on any flag it does not know: it
/// cannot read the format 2 file this daemon writes, so the upgrade is refused.
#[test]
fn upgrade_onto_a_binary_that_reads_only_format_1_is_refused() {
    let d = Daemon::start(&[]);
    let dir = tempfile::tempdir().unwrap();
    let exe = fake_arkosd(dir.path(), "case \"$1\" in\n--check-handover) echo 'handover 1' ;;\n*) exit 2 ;;\nesac");
    let mut c = d.connect();
    match c.request(Request::Upgrade { exe }) {
        Response::Error(e) => assert!(e.contains("handover format 2"), "{e}"),
        other => panic!("unexpected {other:?}"),
    }
    let sid = c.create("/bin/sh");
    assert!(matches!(c.request(Request::Attach { session: sid }), Response::Attached { .. }));
    c.send(Request::Input { session: sid, bytes: b"echo served-$((3+3))\n".to_vec() });
    c.wait_for_output(sid, "served-6");
}

#[test]
fn upgrade_whose_exec_fails_keeps_the_old_daemon_serving() {
    let d = Daemon::start(&[]);
    let answer = |flag: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_arkosd")).arg(flag).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let (check, formats) = (answer("--check-handover"), answer("--handover-formats"));
    // Validate asks for the formats only when the check does not name the newest one.
    let newest = formats.split_whitespace().last().unwrap().to_string();
    let (rm_on_check, rm_on_formats) = if check == format!("handover {newest}") { ("; rm \"$0\"", "") } else { ("", "; rm \"$0\"") };
    let dir = tempfile::tempdir().unwrap();
    // Passes the checks as this binary would, removing itself on the last one, so the exec that
    // follows fails.
    let exe = fake_arkosd(
        dir.path(),
        &format!("case \"$1\" in\n--check-handover) echo '{check}'{rm_on_check} ;;\n--handover-formats) echo '{formats}'{rm_on_formats} ;;\nesac"),
    );
    let mut c = d.connect();
    let sid = c.create("/bin/sh");
    assert!(matches!(c.request(Request::Attach { session: sid }), Response::Attached { .. }));
    match c.request(Request::Upgrade { exe: exe.clone() }) {
        Response::Error(e) => assert!(e.contains("execv failed"), "{e}"),
        other => panic!("unexpected {other:?}"),
    }
    // The reader was unparked, the handover file deleted and the slot freed.
    c.send(Request::Input { session: sid, bytes: b"echo still-$((3+3))\n".to_vec() });
    c.wait_for_output(sid, "still-6");
    let leftovers: Vec<_> = std::fs::read_dir(d.socket.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains("handover"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    match c.request(Request::Upgrade { exe }) {
        Response::Error(e) => assert!(!e.contains("already in progress"), "{e}"),
        other => panic!("unexpected {other:?}"),
    }
    d.connect();
}

#[test]
fn a_second_upgrade_during_the_check_is_refused() {
    let d = Daemon::start(&[]);
    let dir = tempfile::tempdir().unwrap();
    let slow = fake_arkosd(dir.path(), "sleep 1; exit 1");
    let mut a = d.connect();
    let mut b = d.connect();
    let first = a.send(Request::Upgrade { exe: slow.clone() });
    std::thread::sleep(Duration::from_millis(200));
    match b.request(Request::Upgrade { exe: slow.clone() }) {
        Response::Error(e) => assert!(e.contains("already in progress"), "{e}"),
        other => panic!("unexpected {other:?}"),
    }
    // The first attempt fails its check and clears the flag; a new attempt is judged afresh.
    loop {
        if let ServerMessage::Response { id, response } = a.next_message() {
            if id == first {
                assert!(matches!(response, Response::Error(ref e) if e.contains("check-handover")), "{response:?}");
                break;
            }
        }
    }
    match b.request(Request::Upgrade { exe: slow }) {
        Response::Error(e) => assert!(e.contains("check-handover"), "{e}"),
        other => panic!("unexpected {other:?}"),
    }
}

/// Starts an upgrade on `a` whose check holds the slot for a second, runs `during` on `b` while
/// it is held, waits for the upgrade to fail its check, then runs `after` on `b`.
fn while_an_upgrade_is_checked(d: &Daemon, during: impl FnOnce(&mut Conn), after: impl FnOnce(&mut Conn)) {
    let dir = tempfile::tempdir().unwrap();
    let slow = fake_arkosd(dir.path(), "sleep 1; exit 1");
    let mut a = d.connect();
    let mut b = d.connect();
    let first = a.send(Request::Upgrade { exe: slow });
    std::thread::sleep(Duration::from_millis(200));
    during(&mut b);
    loop {
        if let ServerMessage::Response { id, response } = a.next_message() {
            if id == first {
                assert!(matches!(response, Response::Error(ref e) if e.contains("check-handover")), "{response:?}");
                break;
            }
        }
    }
    after(&mut b);
}

#[test]
fn create_during_an_upgrade_is_refused() {
    let d = Daemon::start(&[]);
    let create = |c: &mut Conn| {
        c.request(Request::CreateSession { cwd: std::env::temp_dir(), shell: Some("/bin/sh".into()), env: vec![], nice: 0, rows: 24, cols: 80 })
    };
    while_an_upgrade_is_checked(
        &d,
        |b| match create(b) {
            Response::Error(e) => assert!(e.contains("upgrade is in progress"), "{e}"),
            other => panic!("unexpected {other:?}"),
        },
        |b| assert!(matches!(create(b), Response::SessionCreated { .. })),
    );
}

#[test]
fn kill_during_an_upgrade_is_refused() {
    let d = Daemon::start(&[]);
    let sid = d.connect().create("/bin/sh");
    while_an_upgrade_is_checked(
        &d,
        |b| match b.request(Request::Kill { session: sid }) {
            Response::Error(e) => assert!(e.contains("upgrade is in progress"), "{e}"),
            other => panic!("unexpected {other:?}"),
        },
        |b| {
            assert!(matches!(b.request(Request::Attach { session: sid }), Response::Attached { alive: true, .. }), "the refused kill left the shell running");
            assert_eq!(b.request(Request::Kill { session: sid }), Response::Ok);
            assert!(wait_for_exit(b, sid, Duration::from_secs(5)).is_some(), "the kill after the failed upgrade ends the shell");
        },
    );
}

#[test]
fn resume_with_a_corrupt_handover_file_exits_without_touching_anything() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("handover-1.bin");
    std::fs::write(&file, b"SKHOgarbage").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_arkosd"))
        .arg("--socket")
        .arg(dir.path().join("d.sock"))
        .arg("--resume")
        .arg(&file)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("handover"));
    assert!(!file.exists(), "an unreadable handover still holds terminal contents; it is deleted");
}

#[test]
fn relaunch_client_starts_the_named_binary_when_the_client_disconnects() {
    let d = Daemon::start(&[]);
    let dir = tempfile::tempdir().unwrap();
    let (exe, marker) = relaunch_script(dir.path());
    let mut c = d.connect();
    assert_eq!(c.request(Request::RelaunchClient { exe: exe.clone(), cwd: dir.path().to_path_buf() }), Response::Ok);
    drop(c);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker_written(&marker) {
        assert!(Instant::now() < deadline, "the daemon did not launch {}", exe.display());
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn relaunch_client_refuses_a_foreign_binary() {
    let d = Daemon::start(&[]);
    let mut c = d.connect();
    match c.request(Request::RelaunchClient { exe: PathBuf::from("/bin/sh"), cwd: std::env::temp_dir() }) {
        Response::Error(e) => assert!(e.contains("arkos"), "{e}"),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn relaunch_client_falls_back_to_the_previous_window_when_the_new_one_cannot_start() {
    let d = Daemon::start(&[]);
    let old = tempfile::tempdir().unwrap();
    let (old_exe, marker) = relaunch_script(old.path());
    // Named arkos and owned by this user, so it is accepted, but not executable.
    let new = tempfile::tempdir().unwrap();
    let new_exe = new.path().join("arkos");
    std::fs::write(&new_exe, "#!/bin/sh\n").unwrap();
    let mut c = d.connect_raw();
    let hello = Request::Hello { protocol_version: PROTOCOL_VERSION, ui_pid: std::process::id(), ui_exe: Some(old_exe), ui_cwd: old.path().to_path_buf() };
    assert!(matches!(c.request(hello), Response::Hello { .. }));
    assert_eq!(c.request(Request::RelaunchClient { exe: new_exe, cwd: new.path().to_path_buf() }), Response::Ok);
    drop(c);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker_written(&marker) {
        assert!(Instant::now() < deadline, "the daemon did not fall back to the previous window");
        std::thread::sleep(Duration::from_millis(50));
    }
    let cwd = std::fs::read_to_string(&marker).unwrap();
    assert_eq!(cwd.trim(), old.path().canonicalize().unwrap().to_string_lossy(), "in the previous window's directory");
}

fn agent(id: &str) -> AgentRecord {
    AgentRecord { kind: "claude".into(), id: id.into(), resume: vec!["claude".into(), "--resume".into(), id.into()] }
}

#[test]
fn set_agent_is_reported_in_the_detailed_list_and_cleared_only_by_its_own_id() {
    let d = Daemon::start(&[]);
    let mut c = d.connect();
    let sid = c.create("/bin/sh");
    assert_eq!(c.request(Request::SetAgent { session: sid, agent: agent("a1") }), Response::Ok);
    let Response::SessionsDetailed(list) = c.request(Request::ListSessionsDetailed) else { panic!("not a detailed list") };
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].info.id, sid);
    assert!(!list[0].recovered);
    assert_eq!(list[0].agent, Some(agent("a1")));
    assert_eq!(c.request(Request::ClearAgent { session: sid, id: "other".into() }), Response::Ok);
    let Response::SessionsDetailed(list) = c.request(Request::ListSessionsDetailed) else { panic!() };
    assert_eq!(list[0].agent, Some(agent("a1")), "another id's SessionEnd leaves the record");
    assert_eq!(c.request(Request::ClearAgent { session: sid, id: "a1".into() }), Response::Ok);
    let Response::SessionsDetailed(list) = c.request(Request::ListSessionsDetailed) else { panic!() };
    assert_eq!(list[0].agent, None);
    // The old list still answers, for older windows.
    assert!(matches!(c.request(Request::ListSessions), Response::Sessions(l) if l.len() == 1));
    assert!(matches!(c.request(Request::SetAgent { session: SessionId(999), agent: agent("x") }), Response::Error(e) if e.contains("no session 999")));
}

#[test]
fn hello_version_3_gets_the_range_and_goodbye_is_ok() {
    let d = Daemon::start(&[]);
    let mut c = d.connect_raw();
    let r = c.request(Request::Hello { protocol_version: 3, ui_pid: std::process::id(), ui_exe: None, ui_cwd: std::env::temp_dir() });
    assert!(matches!(r, Response::Hello { protocol_min: 1, protocol_max, .. } if protocol_max == PROTOCOL_VERSION), "{r:?}");
    assert_eq!(c.request(Request::Goodbye), Response::Ok);
    assert_eq!(c.request(Request::Goodbye), Response::Ok, "a second Goodbye is harmless");
}

fn replay_file(d: &Daemon, sid: SessionId) -> PathBuf {
    d.sessions_dir().join(format!("{}.replay", sid.0))
}

fn wait_for_file_containing(path: &std::path::Path, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(bytes) = std::fs::read(path) {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            if text.contains(needle) {
                return text;
            }
        }
        assert!(Instant::now() < deadline, "{} never contained {needle:?}", path.display());
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn history_is_written_to_disk_shortly_after_output_stops_and_appended_after() {
    let d = Daemon::start(&["--flush-quiet-millis", "200"]);
    let mut c = d.connect();
    let sid = c.create("/bin/sh");
    c.attach(sid);
    c.request(Request::Input { session: sid, bytes: b"echo hello flush\n".to_vec() });
    c.wait_for_output(sid, "hello flush");
    let text = wait_for_file_containing(&replay_file(&d, sid), "hello flush");
    let manifest = std::fs::read(d.sessions_dir().join("manifest.bin")).expect("the manifest is written with the first flush");
    assert_eq!(&manifest[..4], b"SKMF");
    c.request(Request::Input { session: sid, bytes: b"echo second line\n".to_vec() });
    c.wait_for_output(sid, "second line");
    let later = wait_for_file_containing(&replay_file(&d, sid), "second line");
    assert!(later.starts_with(&text), "the file grew by appending");
    assert!(later.find("hello flush").unwrap() < later.find("second line").unwrap());
    // A killed session's file goes with it once the reaper forgets the session.
}

#[test]
fn sigterm_writes_the_last_output_and_exits_zero() {
    let d = Daemon::start(&["--flush-quiet-millis", "60000"]);
    let mut c = d.connect();
    let sid = c.create("/bin/sh");
    c.attach(sid);
    c.request(Request::Input { session: sid, bytes: b"i=0; while [ $i -lt 3000 ]; do echo line $i; i=$((i+1)); done; echo END-$((6*7))\n".to_vec() });
    // The terminal echoes the command line too, so the marker is one only the shell prints.
    c.wait_for_output(sid, "END-42");
    // Whatever a periodic pass wrote goes: only the SIGTERM pass can write the file now, and it
    // rewrites the whole ring into a file that vanished.
    let _ = std::fs::remove_file(replay_file(&d, sid));
    let dir = d.sigterm();
    let bytes = std::fs::read(dir.path().join("sessions").join(format!("{}.replay", sid.0))).expect("written on SIGTERM");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("line 2999"), "the last lines are there");
    assert!(text.rfind("END-42").unwrap() > text.rfind("line 2999").unwrap());
    assert!(dir.path().join("sessions").join("manifest.bin").exists());
}

#[test]
fn windows_survive_an_upgrade_and_reach_the_manifest() {
    let d = Daemon::start(&["--flush-quiet-millis", "200"]);
    let project = d.path().join("a-project-window");
    let manifest = d.sessions_dir().join("manifest.bin");
    // Not a UI (pid 0), so the watchdog leaves it alone; the window list keys on the folder.
    let mut w = d.connect_raw();
    let hello = Request::Hello { protocol_version: PROTOCOL_VERSION, ui_pid: 0, ui_exe: None, ui_cwd: project.clone() };
    assert!(matches!(w.request(hello), Response::Hello { .. }));
    wait_for_file_containing(&manifest, &project.to_string_lossy());
    // Nothing changes the table from here, so only the resumed image writes a manifest again.
    std::fs::remove_file(&manifest).unwrap();
    w.send(Request::Upgrade { exe: PathBuf::from(env!("CARGO_BIN_EXE_arkosd")) });
    w.expect_disconnect(Duration::from_secs(10));
    // Postcard keeps a path as its UTF-8 bytes behind a length prefix.
    wait_for_file_containing(&manifest, &project.to_string_lossy());
    let mut c = d.reconnect();
    assert!(matches!(c.request(Request::ListSessions), Response::Sessions(_)), "the resumed daemon serves");
}

fn wait_for_file_without(path: &std::path::Path, needle: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let bytes = std::fs::read(path).unwrap_or_default();
        if !String::from_utf8_lossy(&bytes).contains(needle) {
            return;
        }
        assert!(Instant::now() < deadline, "{} still contains {needle:?}", path.display());
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_session_that_exits_leaves_the_manifest_long_before_the_reaper_forgets_it() {
    let d = Daemon::start(&["--flush-quiet-millis", "100"]);
    let cwd = d.path().join("a-session-that-exits");
    std::fs::create_dir(&cwd).unwrap();
    let manifest = d.sessions_dir().join("manifest.bin");
    let mut c = d.connect();
    let sid = match c.request(Request::CreateSession { cwd: cwd.clone(), shell: Some("/bin/sh".into()), env: vec![], nice: 0, rows: 24, cols: 80 }) {
        Response::SessionCreated { id } => id,
        other => panic!("unexpected {other:?}"),
    };
    c.attach(sid);
    wait_for_file_containing(&manifest, &cwd.to_string_lossy());
    c.send(Request::Input { session: sid, bytes: b"exit\n".to_vec() });
    assert_eq!(wait_for_exit(&mut c, sid, Duration::from_secs(5)), Some(0));
    // The reaper keeps the dead session listed for ten minutes; the manifest drops it at once.
    wait_for_file_without(&manifest, &cwd.to_string_lossy(), Duration::from_secs(3));
}

fn detailed(c: &mut Conn) -> Vec<protocol::SessionDetail> {
    match c.request(Request::ListSessionsDetailed) {
        Response::SessionsDetailed(l) => l,
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn recover_brings_back_sessions_with_their_history_and_runs_the_resume_command() {
    let a = Daemon::start(&["--flush-quiet-millis", "100"]);
    let mut c = a.connect();
    let sid = c.create("/bin/sh");
    c.attach(sid);
    c.request(Request::Input { session: sid, bytes: b"echo before restart\n".to_vec() });
    c.wait_for_output(sid, "before restart");
    let agent = AgentRecord { kind: "test".into(), id: "t1".into(), resume: vec!["echo".into(), "RESUMED".into(), "t1".into()] };
    assert_eq!(c.request(Request::SetAgent { session: sid, agent: agent.clone() }), Response::Ok);
    wait_for_file_containing(&replay_file(&a, sid), "before restart");
    let dir = a.sigterm();
    let b = Daemon::start_in(dir, &["--recover"]);
    let mut c = b.connect();
    let list = detailed(&mut c);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].info.id, sid, "the id is kept, so the saved layout finds it");
    assert!(list[0].recovered && list[0].info.alive);
    assert_eq!(list[0].agent, Some(agent));
    let Response::Attached { replay, alive: true } = c.request(Request::Attach { session: sid }) else { panic!("not attached alive") };
    let mut text = String::from_utf8_lossy(&replay).into_owned();
    let before = text.find("before restart").expect("the old history comes first");
    let divider = text.find("resumed after restart").expect("then the divider");
    assert!(before < divider);
    // The typed line is `'echo' 'RESUMED' 't1'`, which the terminal echoes; only the command's
    // output has "RESUMED t1" with a single space.
    if !text.contains("RESUMED t1") {
        text.push_str(&String::from_utf8_lossy(&c.wait_for_output(sid, "RESUMED t1")));
    }
    assert!(text.find("RESUMED t1").unwrap() > divider, "the typed resume command ran in the new shell");
    assert!(detailed(&mut c)[0].recovered, "the resume line the daemon typed is not the user typing");
    assert_eq!(c.request(Request::Input { session: sid, bytes: b"\n".to_vec() }), Response::Ok);
    assert!(!detailed(&mut c)[0].recovered, "the user typed: the note must not come back with the next list");
    let newer = c.create("/bin/sh");
    assert!(newer.0 > sid.0, "ids stay unique after a recovery");
}

#[test]
fn recover_reopens_windows_that_were_open_and_not_those_that_said_goodbye() {
    let a = Daemon::start(&["--flush-quiet-millis", "100"]);
    let (script, marker) = relaunch_script(a.path());
    let keep = tempfile::tempdir().unwrap();
    let gone = tempfile::tempdir().unwrap();
    let hello = |cwd: &std::path::Path| Request::Hello { protocol_version: PROTOCOL_VERSION, ui_pid: std::process::id(), ui_exe: Some(script.clone()), ui_cwd: cwd.to_path_buf() };
    let mut w1 = a.connect_raw();
    assert!(matches!(w1.request(hello(keep.path())), Response::Hello { .. }));
    let mut w2 = a.connect_raw();
    assert!(matches!(w2.request(hello(gone.path())), Response::Hello { .. }));
    assert_eq!(w2.request(Request::Goodbye), Response::Ok);
    std::thread::sleep(Duration::from_millis(400));
    let dir = a.sigterm();
    let _b = Daemon::start_in(dir, &["--recover"]);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker_written(&marker) {
        assert!(Instant::now() < deadline, "the window for the kept project was not reopened");
        std::thread::sleep(Duration::from_millis(20));
    }
    let want = keep.path().canonicalize().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let launches = std::fs::read_to_string(&marker).unwrap();
    assert_eq!(launches.lines().collect::<Vec<_>>(), vec![want.to_string_lossy()], "one window, for the kept project; the one that said goodbye stays closed");
}

#[test]
fn a_hello_from_the_root_folder_is_never_remembered_or_reopened() {
    // A window opened from the Dock starts at `/`: the daemon must not keep `/` as a project,
    // or a restart or a rollback would reopen a window there, which shows the chooser.
    let a = Daemon::start(&["--flush-quiet-millis", "100"]);
    let (script, marker) = relaunch_script(a.path());
    let keep = tempfile::tempdir().unwrap();
    let hello = |cwd: &std::path::Path| Request::Hello { protocol_version: PROTOCOL_VERSION, ui_pid: std::process::id(), ui_exe: Some(script.clone()), ui_cwd: cwd.to_path_buf() };
    let mut root = a.connect_raw();
    assert!(matches!(root.request(hello(std::path::Path::new("/"))), Response::Hello { .. }));
    let mut project = a.connect_raw();
    assert!(matches!(project.request(hello(keep.path())), Response::Hello { .. }));
    wait_for_file_containing(&a.sessions_dir().join("manifest.bin"), &keep.path().to_string_lossy());
    let dir = a.sigterm();
    let _b = Daemon::start_in(dir, &["--recover"]);
    wait_until(|| marker_written(&marker), Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(500));
    let launches = std::fs::read_to_string(&marker).unwrap();
    let want = keep.path().canonicalize().unwrap();
    assert_eq!(launches.lines().collect::<Vec<_>>(), vec![want.to_string_lossy()], "only the project is reopened, never `/`");
}

#[test]
fn recover_puts_a_session_whose_folder_is_gone_in_home() {
    let a = Daemon::start(&["--flush-quiet-millis", "100"]);
    let mut c = a.connect();
    let folder = tempfile::tempdir().unwrap();
    let sid = match c.request(Request::CreateSession { cwd: folder.path().to_path_buf(), shell: Some("/bin/sh".into()), env: vec![], nice: 0, rows: 24, cols: 80 }) {
        Response::SessionCreated { id } => id,
        other => panic!("{other:?}"),
    };
    // The first manifest's write ends in an F_FULLFSYNC, which under disk load can take longer
    // than any fixed sleep: wait, with a deadline, until it lists the session's folder.
    wait_for_file_containing(&a.sessions_dir().join("manifest.bin"), &folder.path().to_string_lossy());
    drop(folder);
    let dir = a.sigterm();
    let b = Daemon::start_in(dir, &["--recover"]);
    let mut c = b.connect();
    let list = detailed(&mut c);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].info.id, sid, "the same session, recovered");
    assert_eq!(list[0].info.cwd, PathBuf::from(std::env::var("HOME").unwrap()));
    assert!(list[0].info.alive);
}

#[test]
fn recover_without_a_manifest_is_a_plain_start() {
    let d = Daemon::start(&["--recover"]);
    let mut c = d.connect();
    assert!(detailed(&mut c).is_empty());
    let sid = c.create("/bin/sh");
    c.attach(sid);
}

#[test]
fn recover_keeps_an_unreadable_manifest_aside_as_bad_and_starts_plain() {
    let dir = tempfile::tempdir().unwrap();
    let sessions = dir.path().join("sessions");
    std::fs::create_dir(&sessions).unwrap();
    std::fs::write(sessions.join("manifest.bin"), b"not a manifest").unwrap();
    std::fs::write(sessions.join("manifest.bin.bad"), b"an older bad one").unwrap();
    let d = Daemon::start_in(dir, &["--recover"]);
    let mut c = d.connect();
    assert!(detailed(&mut c).is_empty());
    assert_eq!(std::fs::read(d.sessions_dir().join("manifest.bin.bad")).unwrap(), b"not a manifest", "kept for a look, over the older one");
}

#[test]
fn recover_while_a_daemon_is_listening_exits_zero_and_leaves_it() {
    let a = Daemon::start(&[]);
    let mut second = Command::new(env!("CARGO_BIN_EXE_arkosd")).arg("--socket").arg(&a.socket).arg("--recover").stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(s) = second.try_wait().unwrap() {
            break s;
        }
        assert!(Instant::now() < deadline, "the second arkosd did not exit");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "{status}");
    let mut c = a.connect();
    let sid = c.create("/bin/sh");
    c.attach(sid);
}

/// A throwaway ed25519 pair, base64 (secret, public), as `verify.rs`'s test helpers make it.
fn test_release_key() -> (String, String) {
    use base64::Engine;
    let kp = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::generate());
    (base64::engine::general_purpose::STANDARD.encode(kp.sk.as_ref()), base64::engine::general_purpose::STANDARD.encode(kp.pk.as_ref()))
}

/// A release site in `dir`: a fake `Arkos IDE.app` whose arkosd prints `version`, zipped with
/// ditto, and a manifest signed with `sk`. Returns the zip.
fn publish_fake_release(dir: &std::path::Path, version: &str, git: &str, sk: &str) -> PathBuf {
    use base64::Engine;
    use sha2::Digest;
    use std::os::unix::fs::PermissionsExt;
    let src = dir.join("src");
    let app = src.join("Arkos IDE.app");
    std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    std::fs::write(app.join("Contents/Info.plist"), format!("<plist><dict><key>CFBundleShortVersionString</key><string>{version}</string></dict></plist>")).unwrap();
    for name in ["arkos", "arkosd"] {
        let p = app.join("Contents/MacOS").join(name);
        std::fs::write(&p, format!("#!/bin/sh\n[ \"$1\" = --version ] && echo '{name} {version} ({git})'\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let zip = dir.join("bundle.zip");
    let st = Command::new("/usr/bin/ditto").args(["-c", "-k", "--keepParent"]).arg(&app).arg(&zip).stdin(Stdio::null()).status().unwrap();
    assert!(st.success());
    std::fs::remove_dir_all(&src).unwrap();
    let bytes = std::fs::read(&zip).unwrap();
    let sha: String = sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    let json = format!(
        r#"{{"format":1,"version":"{version}","git":"{git}","published":"","arm64":{{"url":"file://{}","size":{},"sha256":"{sha}"}},"min_protocol":1}}"#,
        zip.display(),
        bytes.len()
    );
    std::fs::write(dir.join("manifest.json"), &json).unwrap();
    let b64 = base64::engine::general_purpose::STANDARD;
    let sk = ed25519_compact::SecretKey::from_slice(&b64.decode(sk).unwrap()).unwrap();
    std::fs::write(dir.join("manifest.json.sig"), b64.encode(sk.sign(json.as_bytes(), None).as_ref())).unwrap();
    zip
}

#[test]
fn a_staged_update_is_announced_and_listed() {
    // A site on disk, signed with a throwaway key the daemon is told to trust.
    let site = tempfile::tempdir().unwrap();
    let (sk, pk) = test_release_key();
    let zip = publish_fake_release(site.path(), "9.9.9", "fffffff", &sk);
    let d = Daemon::start(&["--update-url", &format!("file://{}/", site.path().display()), "--check-every-secs", "1", "--release-key", &pk]);
    let mut c = d.connect();
    // Before the first check lands: nothing staged.
    let deadline = Instant::now() + Duration::from_secs(20);
    let staged = loop {
        match c.request(Request::UpdateStatus) {
            Response::UpdateStatus { staged: Some(s), .. } => break s,
            Response::UpdateStatus { staged: None, last_error, .. } => {
                assert!(last_error.is_none(), "{last_error:?}");
                assert!(Instant::now() < deadline, "never staged");
                std::thread::sleep(Duration::from_millis(100));
            }
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(staged.version, "9.9.9");
    assert_eq!(staged.git, "fffffff");
    assert!(d.path().join("updates/9.9.9/Arkos IDE.app/Contents/MacOS/arkosd").is_file(), "staged next to the socket");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(d.path().join("updates")).unwrap().permissions().mode() & 0o777, 0o700, "the staging folder is private");
    let _ = zip;
    // A second check finds the same release already staged: still staged, nothing new.
    assert_eq!(c.request(Request::CheckForUpdates), Response::Ok);
    assert!(matches!(c.request(Request::UpdateStatus), Response::UpdateStatus { staged: Some(_), .. }));
}

#[test]
fn a_connected_window_hears_update_staged() {
    // The site is empty until both windows are connected, so the check that stages the release
    // surely runs after they are: the first checks fail on the missing manifest.
    let site = tempfile::tempdir().unwrap();
    let (sk, pk) = test_release_key();
    let d = Daemon::start(&["--update-url", &format!("file://{}/", site.path().display()), "--check-every-secs", "1", "--release-key", &pk]);
    let mut c = d.connect();
    let mut other = d.connect();
    publish_fake_release(site.path(), "9.9.9", "fffffff", &sk);
    for conn in [&mut c, &mut other] {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            assert!(Instant::now() < deadline, "never heard UpdateStaged");
            if let ServerMessage::Event(Event::UpdateStaged { update }) = conn.next_message() {
                assert_eq!(update.version, "9.9.9");
                break;
            }
        }
    }
}

#[test]
fn a_check_that_finds_no_manifest_is_journaled_as_transient() {
    let site = tempfile::tempdir().unwrap();
    let (_, pk) = test_release_key();
    let d = Daemon::start(&["--update-url", &format!("file://{}/", site.path().display()), "--check-every-secs", "1", "--release-key", &pk]);
    let journal = d.path().join("updates/journal.log");
    wait_until(|| std::fs::read_to_string(&journal).is_ok_and(|t| t.contains("check: transient, tried again at the next check")), Duration::from_secs(10));
}

#[test]
fn with_polling_off_a_check_request_fetches_nothing() {
    let site = tempfile::tempdir().unwrap();
    let (sk, pk) = test_release_key();
    publish_fake_release(site.path(), "9.9.9", "fffffff", &sk);
    let d = Daemon::start(&["--update-url", &format!("file://{}/", site.path().display()), "--check-every-secs", "0", "--release-key", &pk]);
    let mut c = d.connect();
    assert_eq!(c.request(Request::CheckForUpdates), Response::Error("update checks are off".into()));
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(c.request(Request::UpdateStatus), Response::UpdateStatus { staged: None, last_error: None, unreachable_for_secs: None });
    assert!(!d.path().join("updates").exists(), "no poll thread, so no staging folder");
}

#[test]
fn a_protocol_3_window_never_hears_update_staged_and_keeps_its_connection() {
    let site = tempfile::tempdir().unwrap();
    let (sk, pk) = test_release_key();
    let d = Daemon::start(&["--update-url", &format!("file://{}/", site.path().display()), "--check-every-secs", "1", "--release-key", &pk]);
    let mut old = d.connect_raw();
    let r = old.request(Request::Hello { protocol_version: 3, ui_pid: std::process::id(), ui_exe: None, ui_cwd: std::env::temp_dir() });
    assert!(matches!(r, Response::Hello { .. }), "{r:?}");
    let mut new = d.connect();
    publish_fake_release(site.path(), "9.9.9", "fffffff", &sk);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        assert!(Instant::now() < deadline, "the protocol 4 window never heard UpdateStaged");
        if let ServerMessage::Event(Event::UpdateStaged { update }) = new.next_message() {
            assert_eq!(update.version, "9.9.9");
            break;
        }
    }
    // Broadcast queues to every window under one lock, so anything meant for the old window is
    // ahead of this answer.
    let id = old.send(Request::ListSessions);
    loop {
        match old.next_message() {
            ServerMessage::Event(Event::UpdateStaged { .. }) => panic!("a protocol 3 window cannot decode UpdateStaged"),
            ServerMessage::Response { id: rid, response } if rid == id => {
                assert!(matches!(response, Response::Sessions(_)), "{response:?}");
                break;
            }
            _ => continue,
        }
    }
}

/// Polls `f` every 50 ms until it holds; panics at the deadline.
fn wait_until(mut f: impl FnMut() -> bool, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !f() {
        assert!(Instant::now() < deadline, "the condition never held within {timeout:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

impl Daemon {
    /// The marker an applied update leaves until a window confirms it or it is rolled back.
    fn updates_marker(&self) -> PathBuf {
        self.socket.parent().unwrap().join("updates/applied.json")
    }
}

/// A fake `Arkos IDE.app` at exactly `app`, the shape of `publish_fake_release`'s: its arkosd
/// prints `version`, and its arkos appends "launched" to `launches` next to the bundle.
fn fake_bundle_at(app: &std::path::Path, version: &str, git: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let launches = app.parent().unwrap().join("launches");
    std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    std::fs::write(app.join("Contents/Info.plist"), format!("<plist><dict><key>CFBundleShortVersionString</key><string>{version}</string></dict></plist>")).unwrap();
    let arkosd = format!("#!/bin/sh\n[ \"$1\" = --version ] && echo 'arkosd {version} ({git})'\n");
    let arkos = format!("#!/bin/sh\n[ \"$1\" = --version ] && echo 'arkos {version} ({git})' && exit 0\necho launched >> '{}'\n", launches.display());
    for (name, script) in [("arkos", arkos), ("arkosd", arkosd)] {
        let p = app.join("Contents/MacOS").join(name);
        std::fs::write(&p, script).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    app.to_path_buf()
}

/// `<home>/Arkos IDE.app`, whose arkos appends to `<home>/launches`.
fn fake_installed_bundle(home: &std::path::Path, version: &str, git: &str) -> PathBuf {
    fake_bundle_at(&home.join("Arkos IDE.app"), version, git)
}

#[test]
fn apply_update_swaps_the_bundle_and_the_new_window_confirms_it() {
    let site = tempfile::tempdir().unwrap();
    let (sk, pk) = test_release_key();
    publish_fake_release(site.path(), "9.9.9", "fffffff", &sk);
    let home = tempfile::tempdir().unwrap();
    let installed = fake_installed_bundle(home.path(), "0.0.1", "0000000"); // its arkos script appends "launched" to <home>/launches
    // No watchdog: the stand-in new window's pid below is not ours to signal.
    let d = Daemon::start(&["--update-url", &format!("file://{}/", site.path().display()), "--check-every-secs", "1", "--release-key", &pk, "--bundle", installed.to_str().unwrap(), "--watchdog-secs", "0"]);
    let mut c = d.connect();
    wait_until(|| matches!(c.request(Request::UpdateStatus), Response::UpdateStatus { staged: Some(_), .. }), Duration::from_secs(10));
    assert_eq!(c.request(Request::ApplyUpdate { version: "9.9.8".into() }), Response::Error("staged 9.9.9 is not 9.9.8".into()));
    assert_eq!(c.request(Request::ApplyUpdate { version: "9.9.9".into() }), Response::Ok);
    assert!(std::fs::read_to_string(installed.join("Contents/Info.plist")).unwrap().contains("9.9.9"), "the new bundle is in place");
    assert!(installed.with_extension("app.old").exists(), "the old one is kept until a window confirms");
    assert!(d.updates_marker().exists());
    // After the upgrade's exec the old window reconnects with its own pid: that is not the new
    // window, so nothing is confirmed.
    let _reconnect = d.connect();
    std::thread::sleep(Duration::from_millis(1500));
    assert!(installed.with_extension("app.old").exists(), "the old window's reconnect confirms nothing");
    assert!(d.updates_marker().exists());
    // The window's Upgrade and RelaunchClient follow in production; here a Hello from another
    // pid stands in for the relaunched window.
    let _c2 = d.connect_as(std::process::id() + 1);
    wait_until(|| !installed.with_extension("app.old").exists(), Duration::from_secs(5));
    assert!(!d.updates_marker().exists());
    assert_eq!(c.request(Request::ApplyUpdate { version: "9.9.9".into() }), Response::Error("nothing staged".into()), "applied once");
}

#[test]
fn a_hello_with_an_empty_cwd_confirms_an_applied_update() {
    // The relaunched window may open the launch chooser (it was relaunched at `/`): it connects
    // at once with an empty `ui_cwd`, the protocol's "keep the daemon's", and that confirms.
    let site = tempfile::tempdir().unwrap();
    let (sk, pk) = test_release_key();
    publish_fake_release(site.path(), "9.9.9", "fffffff", &sk);
    let home = tempfile::tempdir().unwrap();
    let installed = fake_installed_bundle(home.path(), "0.0.1", "0000000");
    let d = Daemon::start(&["--update-url", &format!("file://{}/", site.path().display()), "--check-every-secs", "1", "--release-key", &pk, "--bundle", installed.to_str().unwrap(), "--watchdog-secs", "0", "--flush-quiet-millis", "100"]);
    let project = tempfile::tempdir().unwrap();
    let mut c = d.connect_raw();
    let hello = Request::Hello { protocol_version: PROTOCOL_VERSION, ui_pid: std::process::id(), ui_exe: None, ui_cwd: project.path().to_path_buf() };
    assert!(matches!(c.request(hello), Response::Hello { .. }));
    wait_until(|| matches!(c.request(Request::UpdateStatus), Response::UpdateStatus { staged: Some(_), .. }), Duration::from_secs(10));
    assert_eq!(c.request(Request::ApplyUpdate { version: "9.9.9".into() }), Response::Ok);
    assert!(d.updates_marker().exists());
    let mut chooser = d.connect_raw();
    let empty = Request::Hello { protocol_version: PROTOCOL_VERSION, ui_pid: std::process::id() + 1, ui_exe: None, ui_cwd: PathBuf::new() };
    assert!(matches!(chooser.request(empty), Response::Hello { .. }));
    wait_until(|| !installed.with_extension("app.old").exists(), Duration::from_secs(5));
    assert!(!d.updates_marker().exists(), "confirmed: the marker is cleared");
    // The folder is chosen: the same connection says Hello again with the project, and the
    // daemon takes it, so a later relaunch of this window reopens that folder.
    let folder = tempfile::tempdir().unwrap();
    let chosen = Request::Hello { protocol_version: PROTOCOL_VERSION, ui_pid: std::process::id() + 1, ui_exe: None, ui_cwd: folder.path().to_path_buf() };
    assert!(matches!(chooser.request(chosen), Response::Hello { .. }), "a second Hello on one connection is answered");
    wait_for_file_containing(&d.sessions_dir().join("manifest.bin"), &folder.path().to_string_lossy());
    assert!(matches!(chooser.request(Request::ListSessions), Response::Sessions(_)), "and the connection keeps serving");
}

/// The response to request `id` on `c`, skipping events and other responses.
fn response_to(c: &mut Conn, id: u32) -> Response {
    loop {
        match c.next_message() {
            ServerMessage::Response { id: rid, response } if rid == id => return response,
            _ => continue,
        }
    }
}

#[test]
fn two_applies_at_once_swap_once_and_the_first_keeps_its_old_bundle_and_marker() {
    let site = tempfile::tempdir().unwrap();
    let (sk, pk) = test_release_key();
    publish_fake_release(site.path(), "9.9.9", "fffffff", &sk);
    let home = tempfile::tempdir().unwrap();
    let installed = fake_installed_bundle(home.path(), "0.0.1", "0000000");
    let d = Daemon::start(&["--update-url", &format!("file://{}/", site.path().display()), "--check-every-secs", "1", "--release-key", &pk, "--bundle", installed.to_str().unwrap()]);
    let mut a = d.connect();
    let mut b = d.connect();
    wait_until(|| matches!(a.request(Request::UpdateStatus), Response::UpdateStatus { staged: Some(_), .. }), Duration::from_secs(10));
    // Two idle windows ask in the same instant.
    let ia = a.send(Request::ApplyUpdate { version: "9.9.9".into() });
    let ib = b.send(Request::ApplyUpdate { version: "9.9.9".into() });
    let (ra, rb) = (response_to(&mut a, ia), response_to(&mut b, ib));
    let refusal = Response::Error("update 9.9.9 is still waiting for a window".into());
    assert!((ra == Response::Ok && rb == refusal) || (rb == Response::Ok && ra == refusal), "{ra:?} {rb:?}");
    std::thread::sleep(Duration::from_millis(500));
    let old = installed.with_extension("app.old");
    assert!(std::fs::read_to_string(old.join("Contents/Info.plist")).unwrap().contains("0.0.1"), "the genuine old bundle is kept");
    assert!(std::fs::read_to_string(installed.join("Contents/Info.plist")).unwrap().contains("9.9.9"));
    let marker = std::fs::read_to_string(d.updates_marker()).unwrap();
    assert!(marker.contains("\"version\":\"9.9.9\""), "{marker}");
    assert!(matches!(a.request(Request::UpdateStatus), Response::UpdateStatus { last_error: None, .. }), "the refused one failed nothing");
}

#[test]
fn the_grace_counts_from_the_relaunch_so_a_slow_relaunch_keeps_the_old_bundle() {
    let site = tempfile::tempdir().unwrap();
    let (sk, pk) = test_release_key();
    publish_fake_release(site.path(), "9.9.9", "fffffff", &sk);
    let home = tempfile::tempdir().unwrap();
    let installed = fake_installed_bundle(home.path(), "0.0.1", "0000000");
    let d = Daemon::start(&["--update-url", &format!("file://{}/", site.path().display()), "--check-every-secs", "1", "--release-key", &pk, "--bundle", installed.to_str().unwrap(), "--connect-grace-secs", "2"]);
    let mut c = d.connect();
    wait_until(|| matches!(c.request(Request::UpdateStatus), Response::UpdateStatus { staged: Some(_), .. }), Duration::from_secs(10));
    assert_eq!(c.request(Request::ApplyUpdate { version: "9.9.9".into() }), Response::Ok);
    // Well past the grace, and no relaunch yet: nothing rolls back.
    std::thread::sleep(Duration::from_millis(3500));
    let old = installed.with_extension("app.old");
    assert!(old.exists(), "the grace has not started: no window was launched yet");
    assert!(d.updates_marker().exists());
    // The window asks for its relaunch and goes: the clock starts now. The new window (the fake
    // bundle's arkos, which never connects) does not say Hello, so the grace runs out.
    let exe = installed.join("Contents/MacOS/arkos");
    assert_eq!(c.request(Request::RelaunchClient { exe, cwd: home.path().to_path_buf() }), Response::Ok);
    let gone = Instant::now();
    drop(c);
    wait_until(|| std::fs::read_to_string(installed.join("Contents/Info.plist")).unwrap_or_default().contains("0.0.1"), Duration::from_secs(10));
    // The clock counts whole seconds, so a 2 s grace is at least one.
    assert!(gone.elapsed() >= Duration::from_secs(1), "rolled back {:?} after the relaunch, inside the grace", gone.elapsed());
    assert!(!old.exists());
    assert!(!d.updates_marker().exists());
}

#[test]
fn a_window_that_never_connects_rolls_the_bundle_back() {
    // The marker says an update was applied 25 s ago and nobody has connected: a daemon starting
    // now rolls back and launches the old window in each remembered project, or, with none
    // remembered (the 2026-10-15 case: the window was at `/`), once at `/`, the chooser.
    let home = tempfile::tempdir().unwrap();
    let installed = fake_installed_bundle(home.path(), "9.9.9", "fffffff");
    let old = fake_bundle_at(&home.path().join("Arkos IDE.app.old"), "0.0.1", "0000000");
    let launches = home.path().join("launches");
    std::fs::write(old.join("Contents/MacOS/arkos"), format!("#!/bin/sh\npwd -P >> '{}'\n", launches.display())).unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("updates")).unwrap();
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() - 25;
    std::fs::write(dir.path().join("updates/applied.json"), format!("{{\"version\":\"9.9.9\",\"old_kept\":true,\"at_unix\":{at}}}")).unwrap();
    // The grace restarts at this start, so a short one keeps the test quick.
    let d = Daemon::start_in(dir, &["--check-every-secs", "0", "--bundle", installed.to_str().unwrap(), "--connect-grace-secs", "1"]);
    // Read leniently: the bundle is briefly absent between the two renames of the rollback.
    wait_until(|| std::fs::read_to_string(installed.join("Contents/Info.plist")).unwrap_or_default().contains("0.0.1"), Duration::from_secs(5));
    assert!(!old.exists(), "renamed back");
    wait_until(|| marker_written(&launches), Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(std::fs::read_to_string(&launches).unwrap(), "/\n", "no folder remembered: one window, at `/`, which shows the chooser");
    assert!(!d.updates_marker().exists());
    let mut c = d.connect();
    assert!(matches!(c.request(Request::UpdateStatus), Response::UpdateStatus { staged: None, last_error: Some(e), .. } if e.contains("rolled back")));
    let journal = std::fs::read_to_string(d.path().join("updates/journal.log")).unwrap();
    for step in ["marker found at start: 9.9.9", "reject 9.9.9", "rollback of 9.9.9: update 9.9.9 rolled back", "relaunch spawned (the previous app after a rollback, no folder remembered)"] {
        assert!(journal.contains(step), "{step:?} missing from the journal:\n{journal}");
    }
}

#[test]
fn a_hello_naming_no_window_during_a_pending_apply_confirms_nothing() {
    // `arkos doctor`'s Hello: pid 0 and no folder. It must not count as the new window.
    let site = tempfile::tempdir().unwrap();
    let (sk, pk) = test_release_key();
    publish_fake_release(site.path(), "9.9.9", "fffffff", &sk);
    let home = tempfile::tempdir().unwrap();
    let installed = fake_installed_bundle(home.path(), "0.0.1", "0000000");
    let d = Daemon::start(&["--update-url", &format!("file://{}/", site.path().display()), "--check-every-secs", "1", "--release-key", &pk, "--bundle", installed.to_str().unwrap(), "--watchdog-secs", "0"]);
    let mut c = d.connect();
    wait_until(|| matches!(c.request(Request::UpdateStatus), Response::UpdateStatus { staged: Some(_), .. }), Duration::from_secs(10));
    assert_eq!(c.request(Request::ApplyUpdate { version: "9.9.9".into() }), Response::Ok);
    let mut doctor = d.connect_raw();
    let hello = Request::Hello { protocol_version: PROTOCOL_VERSION, ui_pid: 0, ui_exe: None, ui_cwd: PathBuf::new() };
    assert!(matches!(doctor.request(hello), Response::Hello { .. }));
    assert!(matches!(doctor.request(Request::UpdateStatus), Response::UpdateStatus { .. }));
    drop(doctor);
    std::thread::sleep(Duration::from_millis(1500));
    assert!(installed.with_extension("app.old").exists(), "not confirmed");
    assert!(d.updates_marker().exists(), "still waiting for the new window");
    let journal = std::fs::read_to_string(d.path().join("updates/journal.log")).unwrap();
    assert!(journal.contains("pid none, no folder, protocol 4, during the pending apply of 9.9.9: does not confirm (no window pid)"), "{journal}");
    assert!(!journal.contains("relaunch spawned"), "its disconnect relaunches nothing: {journal}");
}

#[test]
fn the_journal_records_an_apply_from_check_to_confirm_and_the_window_log_keeps_its_output() {
    use std::os::unix::fs::PermissionsExt;
    let site = tempfile::tempdir().unwrap();
    let (sk, pk) = test_release_key();
    publish_fake_release(site.path(), "9.9.9", "fffffff", &sk);
    let home = tempfile::tempdir().unwrap();
    let installed = fake_installed_bundle(home.path(), "0.0.1", "0000000");
    let d = Daemon::start(&["--update-url", &format!("file://{}/", site.path().display()), "--check-every-secs", "1", "--release-key", &pk, "--bundle", installed.to_str().unwrap(), "--watchdog-secs", "0"]);
    // The relaunched "window": prints to both outputs, as a window's startup line does.
    let bin = tempfile::tempdir().unwrap();
    let window = bin.path().join("arkos");
    std::fs::write(&window, "#!/bin/sh\necho \"window says hi on stdout\"\necho \"arkos starting in $(pwd -P)\" >&2\n").unwrap();
    std::fs::set_permissions(&window, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut c = d.connect();
    wait_until(|| matches!(c.request(Request::UpdateStatus), Response::UpdateStatus { staged: Some(_), .. }), Duration::from_secs(10));
    assert_eq!(c.request(Request::ApplyUpdate { version: "9.9.9".into() }), Response::Ok);
    assert_eq!(c.request(Request::RelaunchClient { exe: window.clone(), cwd: home.path().to_path_buf() }), Response::Ok);
    drop(c);
    let log = d.path().join("window.log");
    wait_until(|| std::fs::read_to_string(&log).is_ok_and(|t| t.contains("arkos starting in")), Duration::from_secs(5));
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.contains("window says hi on stdout"), "{text}");
    let _new = d.connect_as(std::process::id() + 1);
    wait_until(|| !installed.with_extension("app.old").exists(), Duration::from_secs(5));
    let journal = d.path().join("updates/journal.log");
    wait_until(|| std::fs::read_to_string(&journal).is_ok_and(|t| t.contains("confirm 9.9.9")), Duration::from_secs(5));
    let journal = std::fs::read_to_string(&journal).unwrap();
    let pid = std::process::id();
    let steps = [
        "staged 9.9.9 (fffffff)".to_string(),
        format!("apply 9.9.9 (fffffff) to {}", installed.display()),
        format!("windows at the apply: pids [{pid}]"),
        "marker written for 9.9.9".to_string(),
        "swap ok".to_string(),
        "the grace for 9.9.9 starts now".to_string(),
        format!("relaunch spawned (relaunch the window asked for): {} in {}, pid ", window.display(), home.path().display()),
        format!("pid {}, {}, protocol {PROTOCOL_VERSION}, during the pending apply of 9.9.9: confirms", pid + 1, std::env::temp_dir().display()),
        "confirm 9.9.9".to_string(),
    ];
    let mut at = 0;
    for step in &steps {
        let found = journal[at..].find(step.as_str()).unwrap_or_else(|| panic!("{step:?} missing or out of order in the journal:\n{journal}"));
        at += found;
    }
    // Every line is timestamped: `2026-10-15 08:04:12 -0400 ...`.
    assert!(journal.lines().all(|l| l.as_bytes().get(4) == Some(&b'-') && l.as_bytes().get(19) == Some(&b' ')), "{journal}");
    let later = d.path().join("updates/journal.log");
    wait_until(|| std::fs::read_to_string(&later).is_ok_and(|t| t.contains("check: the manifest has 9.9.9 (fffffff), not newer than running")), Duration::from_secs(5));
}

#[test]
fn apply_update_outside_an_app_bundle_is_refused_and_touches_nothing() {
    // The test binary runs from target/, not from an app: no bundle to swap.
    let site = tempfile::tempdir().unwrap();
    let (sk, pk) = test_release_key();
    publish_fake_release(site.path(), "9.9.9", "fffffff", &sk);
    let d = Daemon::start(&["--update-url", &format!("file://{}/", site.path().display()), "--check-every-secs", "1", "--release-key", &pk]);
    let mut c = d.connect();
    wait_until(|| matches!(c.request(Request::UpdateStatus), Response::UpdateStatus { staged: Some(_), .. }), Duration::from_secs(10));
    assert_eq!(c.request(Request::ApplyUpdate { version: "9.9.9".into() }), Response::Error("not running from an app bundle".into()));
    assert!(matches!(c.request(Request::UpdateStatus), Response::UpdateStatus { staged: Some(_), .. }), "still staged");
    assert!(!d.updates_marker().exists());
}

#[test]
fn a_second_apply_while_the_first_waits_for_its_window_is_refused() {
    let site = tempfile::tempdir().unwrap();
    let (sk, pk) = test_release_key();
    publish_fake_release(site.path(), "9.9.9", "fffffff", &sk);
    let home = tempfile::tempdir().unwrap();
    let installed = fake_installed_bundle(home.path(), "0.0.1", "0000000");
    let d = Daemon::start(&["--update-url", &format!("file://{}/", site.path().display()), "--check-every-secs", "1", "--release-key", &pk, "--bundle", installed.to_str().unwrap()]);
    let mut c = d.connect();
    wait_until(|| matches!(c.request(Request::UpdateStatus), Response::UpdateStatus { staged: Some(_), .. }), Duration::from_secs(10));
    assert_eq!(c.request(Request::ApplyUpdate { version: "9.9.9".into() }), Response::Ok);
    publish_fake_release(site.path(), "9.9.10", "eeeeeee", &sk);
    wait_until(|| matches!(c.request(Request::UpdateStatus), Response::UpdateStatus { staged: Some(_), .. }), Duration::from_secs(10));
    assert_eq!(c.request(Request::ApplyUpdate { version: "9.9.10".into() }), Response::Error("update 9.9.9 is still waiting for a window".into()));
    assert!(installed.with_extension("app.old").exists(), "the first swap's old bundle is kept");
    assert!(std::fs::read_to_string(installed.join("Contents/Info.plist")).unwrap().contains("9.9.9"));
}

#[test]
fn a_rejected_version_is_not_staged_again_by_a_later_daemon() {
    let site = tempfile::tempdir().unwrap();
    let (sk, pk) = test_release_key();
    publish_fake_release(site.path(), "9.9.9", "fffffff", &sk);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("updates")).unwrap();
    std::fs::write(dir.path().join("updates/rejected"), "9.9.9\n").unwrap();
    let d = Daemon::start_in(dir, &["--update-url", &format!("file://{}/", site.path().display()), "--check-every-secs", "1", "--release-key", &pk]);
    let mut c = d.connect();
    // A few checks run in this time; each finds nothing newer than the rejected 9.9.9.
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(c.request(Request::UpdateStatus), Response::UpdateStatus { staged: None, last_error: None, unreachable_for_secs: Some(0) }, "the manifest was reached");
    assert!(!d.path().join("updates/9.9.9").exists());
}

#[test]
fn a_staged_release_not_newer_than_a_restarted_daemon_is_deleted_and_never_offered() {
    // A daemon stages 9.9.9; the app is then updated past it by hand, and the daemon comes back
    // as 9.9.10 (`--running-version`, the test's stand-in for a newer binary).
    let site = tempfile::tempdir().unwrap();
    let (sk, pk) = test_release_key();
    publish_fake_release(site.path(), "9.9.9", "fffffff", &sk);
    let url = format!("file://{}/", site.path().display());
    let home = tempfile::tempdir().unwrap();
    let installed = fake_installed_bundle(home.path(), "0.0.1", "0000000");
    let d = Daemon::start(&["--update-url", &url, "--check-every-secs", "1", "--release-key", &pk, "--bundle", installed.to_str().unwrap()]);
    let mut c = d.connect();
    wait_until(|| matches!(c.request(Request::UpdateStatus), Response::UpdateStatus { staged: Some(_), .. }), Duration::from_secs(10));
    drop(c);
    let dir = d.sigterm();
    assert!(dir.path().join("updates/9.9.9/Arkos IDE.app").is_dir(), "staged on disk before the restart");
    let d = Daemon::start_in(dir, &["--update-url", &url, "--check-every-secs", "1", "--release-key", &pk, "--bundle", installed.to_str().unwrap(), "--running-version", "9.9.10"]);
    let mut c = d.connect();
    // Requests are served only after the start's cleanup.
    assert!(!d.path().join("updates/9.9.9").exists(), "removed at the start");
    let journal = std::fs::read_to_string(d.path().join("updates/journal.log")).unwrap();
    assert!(journal.contains("start: staged 9.9.9 on disk is not newer than running 9.9.10: removed"), "{journal}");
    // A few checks run; each finds the manifest's 9.9.9 not newer than 9.9.10.
    std::thread::sleep(Duration::from_millis(2500));
    assert!(matches!(c.request(Request::UpdateStatus), Response::UpdateStatus { staged: None, last_error: None, .. }));
    assert!(matches!(c.request(Request::ApplyUpdate { version: "9.9.9".into() }), Response::Error(_)));
    assert!(std::fs::read_to_string(installed.join("Contents/Info.plist")).unwrap().contains("0.0.1"), "nothing swapped");
    assert!(!d.updates_marker().exists());
}
