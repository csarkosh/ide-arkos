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
        self.connect_as(std::process::id())
    }

    /// Connects and says Hello with `ui_pid` as the UI's pid.
    fn connect_as(&self, ui_pid: u32) -> Conn {
        let stream = UnixStream::connect(&self.socket).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut c = Conn { r: BufReader::new(stream.try_clone().unwrap()), w: BufWriter::new(stream), next: 1 };
        let r = c.request(Request::Hello { ui_pid, ui_exe: None, ui_cwd: std::env::temp_dir() });
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

/// Writes the "UI executable" the watchdog relaunches: a script that records its working
/// directory in the returned marker file, written whole by a rename so it is never read half
/// written. Returns (script, marker).
fn relaunch_script(dir: &std::path::Path) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let marker = dir.join("relaunched");
    let script = dir.join("relaunch.sh");
    std::fs::write(&script, format!("#!/bin/sh\npwd -P > '{m}.tmp' && mv '{m}.tmp' '{m}'\n", m = marker.display())).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    (script, marker)
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
    write_frame(&mut w, &ClientMessage { id: 1, request: Request::Hello { ui_pid: fake_ui.id(), ui_exe: Some(script.clone()), ui_cwd: ui_cwd.path().to_path_buf() } }).unwrap();
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
    while !marker.exists() {
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
    write_frame(&mut w, &ClientMessage { id: 1, request: Request::Hello { ui_pid: fake_ui.id(), ui_exe: None, ui_cwd: std::env::temp_dir() } }).unwrap();
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
    let hello = Request::Hello { ui_pid: fake_ui.id(), ui_exe: Some(script), ui_cwd: dir.path().to_path_buf() };
    assert_eq!(ui.request(hello), Response::Ok);
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
    while !marker.exists() {
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
