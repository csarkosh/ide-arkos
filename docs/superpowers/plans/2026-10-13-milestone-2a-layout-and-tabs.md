# Arkos IDE Milestone 2a: Layout and tabs. Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the one-terminal window into the spec's grid: a binary split tree of panes with tabs, drag and drop, the VS Code keyboard map, persistence of the whole layout, and the daemon lifecycle fixes milestone 1 deferred, so that Cyrus can run four Claude Code sessions side by side and relaunch into the same layout.

**Architecture:** The layout is a pure data structure (`tree.rs`) with property tests; GPUI views render it (`split_view.rs`, `pane_view.rs`, `empty_pane.rs`) and translate mouse and keyboard into tree operations. One `SessionManager` owns every `Terminal` entity, routes daemon events by session id, and reconciles the saved layout against the daemon's session list on launch. The daemon gains exit detection independent of PTY EOF, kill that never signals a reaped pid, and reaping of dead sessions, so Cmd+W can send `Kill` safely.

**Tech Stack:** Rust stable (edition 2021), gpui 0.2.2 (exact pin, `runtime_shaders`), alacritty_terminal 0.26, portable-pty 0.9, postcard 1.1 + serde + serde_json, futures 0.3, libc 0.2, anyhow 1, objc2-foundation 0.3.2 (already in `Cargo.lock` through gpui). Dev: tempfile 3, proptest 1.

**Spec:** `docs/superpowers/specs/2026-10-13-arkos-design.md` (sections 2.1, 2.4, 3 "jump to bottom", 4, 9, 10, 11 milestone 2, 12). The config file, themes, text presets, the command palette and find (sections 4 "Open config / command palette", "Cmd+F", and 7) are **milestone 2b**, a separate plan, because they are an independent subsystem; this plan leaves their bindings unbound and says so where they would go.

## Global Constraints

- macOS only, Apple Silicon. No Linux or Windows code paths.
- `gpui = { version = "=0.2.2", features = ["runtime_shaders"] }` pinned exactly. Never bump inside this plan. If a gpui or alacritty_terminal name in this plan does not compile, the API moved: grep `~/.cargo/registry/src/*/gpui-0.2.2/src/` (its `examples/drag_drop.rs`, `examples/set_menus.rs` and `examples/on_window_close_quit.rs` are the reference for drag and drop, menus and quit) or `~/.cargo/registry/src/*/alacritty_terminal-0.26.*/src/` for the method, fix the call, and note the change in the commit message. Do not guess.
- Display name "Arkos IDE" in the window title and user-facing text; binaries, crates, the socket directory `~/Library/Application Support/arkos/` and log prefixes stay lowercase `arkos`.
- Text defaults: Menlo at 16 px, line height 1.5; background `#11161f`, foreground `#d6dae0` (spec 7.1). Cmd+= and Cmd+- nudge the focused terminal's size by 1 px within 10 to 32 px (spec 7, "nudge the size of the focused terminal without changing the preset").
- Layout (spec 4): binary split tree; every split edge is a 4 px handle with an 8 px hit area; dragging updates the ratio live; double-click resets to equal; minimum pane size 160 px wide, 80 px tall; panes are numbered Cmd+1 to Cmd+9 left to right, top to bottom; a pane whose last tab closes with Cmd+W stays open and shows the empty state; a pane auto-closes only when its last tab is dragged into another pane; the last pane in the workspace never closes.
- Keyboard (spec 4 table): the bindings in Task 7 are the spec's, verbatim. Unbound keys reach the focused terminal as bytes. No bell-jump or browser-focus shortcut exists.
- Persistence (spec 4): layout tree, tab order, active tabs, split ratios, each tab's session id or viewer path are written to `~/Library/Application Support/arkos/workspaces/<hash>.json` on every change, debounced (300 ms); on launch the tree is rebuilt and every tab reattached; a missing or dead session reopens as a fresh shell in the same cwd with a note in the tab.
- Daemon (spec 2.1, 2.4, 9): the daemon never parses escape sequences; the UI thread never blocks on IO; a UI client is never dropped by the daemon for slowness, the watchdog is the only authority that ends a UI; nothing allocates per output byte beyond the ring.
- Motion (spec 7.1): the empty-state animation is slow, low contrast, plays on appearance and hover then settles, costs no frames when idle, and honours Reduce Motion.
- Commit messages: imperative, one line, body optional. No em-dashes in any copy or comment. Every commit ends with a blank line and `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Tests: `cargo test -p arkos` must stay headless (no window); daemon tests run the real binary from `crates/arkosd/tests/daemon.rs` and must pass ten times in a row; the build stays warning-free for the project's crates.

## Review Focus

1. **Closing the last tab in the last pane.** Cmd+W on the only tab of the only pane must show the empty state, never a blank window or a closed window. Pinned by Task 2's `last_pane_never_closes` and Task 9's `empty_pane_renders_bindings` checks.
2. **Dropping a tab onto its own position.** Dragging a tab and dropping it on its own tab bar at the same index must change nothing and never lose the tab. Pinned by Task 2's `move_tab_onto_itself_is_a_no_op` property.
3. **Pane numbering after splits and closes.** Cmd+1 to Cmd+9 must always count left to right, top to bottom, and every pane must be reachable. Pinned by Task 2's `panes_are_numbered_in_reading_order` property and `every_pane_is_reachable`.
4. **A session dying in a background tab.** The tab must show "ended", the pane must not steal focus, and keys to the focused terminal must keep flowing. Pinned by Task 5's `exit_in_a_background_tab_does_not_change_focus`.
5. **Relaunch into a 2x2 layout whose sessions are partly gone.** Each missing or dead session becomes a fresh shell in the same pane, the layout is preserved, and the tab carries the "restarted" note. Pinned by Task 5's `reconcile_replaces_dead_and_missing_sessions_in_place`.

---

## File structure

```
crates/
  protocol/src/lib.rs             unchanged wire format (PROTOCOL_VERSION stays 1: no message changes in this plan)
  arkosd/
    src/session.rs                ChildSlot (try_reap), mark_exited, poll_exit, kill that never signals a reaped pid
    src/reaper.rs                 NEW: periodic poll_exit and removal of dead sessions nobody is attached to
    src/server.rs                 Options gains reaper_millis and dead_session_ttl; Hello validates ui_exe; socket path checks
    src/main.rs                   flags --reaper-millis, --dead-session-secs
    tests/daemon.rs               lifecycle, reaping and hardening tests (append)
  arkos/
    src/tree.rs                   NEW: Tree, Node, Pane, Tab, Side, layout(), neighbor(), property tests
    src/workspace.rs              v2 format: the tree plus project path; v1 migration; WorkspaceSaver (debounced, off-thread)
    src/paths.rs                  stable FNV-1a workspace file name (replaces DefaultHasher)
    src/sessions.rs               NEW: SessionManager: terminals by session id, event routing, attach/create/restore per tab, reconcile
    src/keymap.rs                 NEW: actions! and the spec's bindings table
    src/pane_view.rs              NEW: PaneView: tab bar, terminal element, drop zones, jump-to-bottom pill
    src/split_view.rs             NEW: renders the tree with resize handles
    src/empty_pane.rs             NEW: the friendly empty state with its ambient animation
    src/chrome.rs                 NEW: top bar and status bar
    src/motion.rs                 NEW: Reduce Motion detection
    src/app_nap.rs                NEW: keeps the heartbeat timer unthrottled while minimized
    src/terminal.rs               clear_scrollback, title fallback, scrolled_up(), modifier keys on arrows (keys.rs)
    src/keys.rs                   Ctrl and Shift on arrows, Home and End (xterm modifier encoding)
    src/terminal_element.rs       font size as a parameter instead of a constant
    src/main.rs                   Root: owns Tree, SessionManager, focus, drag state; actions; menus
  README.md                       milestone 2a status and the human checklist
```

Responsibilities stay one per file. `tree.rs`, `workspace.rs`, `sessions.rs` (its pure parts) and `keymap.rs` have no gpui types in their tested surface. Views never mutate the tree except through `Root` methods, so every layout change is saved once.

---

### Task 1: Daemon session lifecycle: exit detection, safe kill, reaping, hardening

**Files:**
- Modify: `crates/arkosd/src/session.rs`, `crates/arkosd/src/server.rs`, `crates/arkosd/src/main.rs`
- Create: `crates/arkosd/src/reaper.rs`
- Test: `crates/arkosd/tests/daemon.rs` (append)

**Interfaces:**
- Consumes: the existing `Session`, `ClientSink`, `server::{Shared, State, Options, dispatch}`, `watchdog::start`.
- Produces:
  - `session::Session::poll_exit(&self)`: non-blocking; if the shell has exited, records it and emits `Event::Exited` once, even while a background job keeps the PTY open.
  - `session::Session::kill(&self) -> anyhow::Result<()>`: never signals a pid once the child has been reaped.
  - `session::Session::exited_at(&self) -> Option<Instant>` and `has_subscribers(&self) -> bool`.
  - `reaper::start(state: Shared, every: Duration, ttl: Duration)`: a thread that calls `poll_exit` on every session each `every` and removes a dead session with no subscribers once `ttl` has passed since it exited.
  - `server::Options { replay_capacity, watchdog, client_pending_cap, subscriber_high_water, subscriber_stall, reaper_every: Duration, dead_session_ttl: Duration }`; CLI `--reaper-millis <n>` (default 1000) and `--dead-session-secs <n>` (default 600).
  - `Hello` with a `ui_exe` whose file name is not `arkos` or that is not a regular file is stored as `None` and logged.
  - `server::run` refuses to start when the socket path exists and is not a socket, and sets the parent directory to 0700 only when it created it.

Why first: Task 7 binds Cmd+W to `Kill`, and milestone 1's final review found that `Kill` could signal a reaped (possibly reused) pid, that exit was detected only by PTY EOF, that the reader held the child mutex across a blocking `wait()`, and that dead sessions were never removed. These are fixed as one unit.

- [ ] **Step 1: Write the failing lifecycle tests**

Append to `crates/arkosd/tests/daemon.rs`:

```rust
/// Waits up to `timeout` for an `Exited` event for `session`, returning its code.
fn wait_for_exit(conn: &mut Conn, session: SessionId, timeout: Duration) -> Option<i32> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let ServerMessage::Event(Event::Exited { session: s, code }) = conn.next_message() {
            if s == session {
                return code;
            }
        }
    }
    panic!("no Exited event for session {} within {timeout:?}", session.0);
}

#[test]
fn exit_is_detected_while_a_background_job_holds_the_pty() {
    // The reaper polls every 100 ms here; the background `sleep 2` keeps the PTY slave open, so
    // EOF cannot be what detects the exit before the sleep ends.
    let d = Daemon::start(&["--reaper-millis", "100"]);
    let mut c = d.connect();
    let sid = c.create("/bin/sh");
    assert!(matches!(c.request(Request::Attach { session: sid }), Response::Attached { .. }));
    let started = Instant::now();
    c.send(Request::Input { session: sid, bytes: b"sleep 2 & exit 3\n".to_vec() });
    let code = wait_for_exit(&mut c, sid, Duration::from_millis(1800));
    assert_eq!(code, Some(3));
    assert!(started.elapsed() < Duration::from_millis(1800), "exit was only noticed at EOF");
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
```

`wait_for_exit` must not hang when the event never comes: give `Conn` a `set_read_timeout(&mut self, Option<Duration>)` helper that calls `set_read_timeout` on its stream, and call it with 5 s at the start of `wait_for_exit` (and `None` at the end); a timed-out `next_message` then panics with the daemon's silence as the message. `Daemon::connect_raw()` is a connection that sends no `Hello`; if the harness only has `connect()` (which sends a tool `Hello`) and `connect_as(ui_pid)`, add `connect_raw` next to them returning a `Conn` without the handshake. `env!("CARGO_BIN_EXE_arkosd")` is how the existing `Daemon::start` finds the binary; reuse the same expression.

- [ ] **Step 2: Run the tests to see them fail**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test -p arkosd --test daemon exit_is_detected 2>&1 | tail -15`
Expected: `exit_is_detected_while_a_background_job_holds_the_pty` fails ("no Exited event ... within 1.8s", because only EOF detects exit today); `daemon_refuses_a_regular_file_at_the_socket_path` fails (the daemon removes the file and starts); `dead_session_is_reaped...` fails ("dead session never reaped"); the others may compile-fail on `--reaper-millis` being unknown, which the daemon reports as an error exit.

- [ ] **Step 3: Replace the child mutex with a reaping slot**

In `crates/arkosd/src/session.rs`, change the `child` field and add the slot type. Replace `child: Mutex<Box<dyn Child + Send + Sync>>,` with `child: Mutex<ChildSlot>,` and add above `pub struct Session`:

```rust
/// The shell process and, once reaped, its exit code. A reaped pid may be reused by any other
/// process, so every signal and every wait goes through here and nothing touches the pid after
/// `exited` is set.
struct ChildSlot {
    child: Box<dyn Child + Send + Sync>,
    exited: Option<i32>,
}

impl ChildSlot {
    /// Non-blocking. Reaps the child if it has exited and remembers the code.
    fn try_reap(&mut self) -> Option<i32> {
        if self.exited.is_none() {
            if let Ok(Some(status)) = self.child.try_wait() {
                self.exited = Some(status.exit_code() as i32);
            }
        }
        self.exited
    }
}
```

In `Session::spawn`, where the struct is built, use `child: Mutex::new(ChildSlot { child, exited: None }),` (the local variable holding the spawned child is named `child` today; keep the name).

Add to `SessionState`: `exited_at: Option<Instant>,` (initialised `None` in `spawn`), and `use std::time::Instant;` if absent.

- [ ] **Step 4: Record exits once, from either the reader or the reaper**

Add to `impl Session`:

```rust
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
```

In `start_reader`, replace the block after the read loop:

```rust
                let code = session.child.lock().unwrap().wait().ok().map(|s| s.exit_code() as i32);
                let mut st = session.state.lock().unwrap();
                st.alive = false;
                st.exit_code = code;
                let event = ServerMessage::Event(Event::Exited { session: session.id, code });
                st.subscribers.retain(|_, tx| tx.send(event.clone()));
```

with a poll that never holds the child lock across a blocking wait (EOF and exit are normally microseconds apart; a background job holding the slave is the case the reaper covers, and this loop ends when it reaps):

```rust
                let code = loop {
                    if let Some(code) = session.child.lock().unwrap().try_reap() {
                        break code;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                };
                session.mark_exited(Some(code));
```

- [ ] **Step 5: Make kill and is_ending go through the slot**

Replace `is_ending`:

```rust
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
```

Replace `kill`:

```rust
    /// Ends the shell. The exit check and the signal happen under the same lock, so a child that
    /// exits in between is reaped here and never signalled by its possibly reused pid.
    pub fn kill(&self) -> Result<()> {
        let mut slot = self.child.lock().unwrap();
        if slot.try_reap().is_some() {
            return Ok(());
        }
        let killed = slot.child.kill().context("kill child");
        drop(slot);
        self.ending.store(true, Ordering::Relaxed);
        // Wake a reader paused on a slow subscriber so it drains to EOF instead of waiting.
        let st = self.state.lock().unwrap();
        for sink in st.subscribers.values() {
            sink.wake();
        }
        killed
    }
```

Lock order after this change: the child lock is a leaf everywhere except `kill`, which takes it alone and releases it before the state lock; nothing takes the state lock and then the child lock. Keep it that way.

- [ ] **Step 6: Add the reaper thread**

Create `crates/arkosd/src/reaper.rs`:

```rust
//! Notices shells that exited while something still holds their PTY open, and forgets dead
//! sessions once nobody is attached and the grace period has passed (spec 2.1: the daemon keeps
//! sessions for reattach, not forever).
use crate::server::Shared;
use std::time::Duration;

pub fn start(state: Shared, every: Duration, ttl: Duration) {
    std::thread::Builder::new()
        .name("reaper".into())
        .spawn(move || loop {
            std::thread::sleep(every);
            scan(&state, ttl);
        })
        .expect("spawn reaper");
}

fn scan(state: &Shared, ttl: Duration) {
    // Clone the handles out so no session lock is taken under the global lock.
    let sessions: Vec<_> = state.lock().unwrap().sessions.values().cloned().collect();
    for s in &sessions {
        s.poll_exit();
    }
    let mut dead = Vec::new();
    for s in &sessions {
        if let Some(at) = s.exited_at() {
            if at.elapsed() >= ttl && !s.has_subscribers() {
                dead.push(s.id);
            }
        }
    }
    if dead.is_empty() {
        return;
    }
    let mut st = state.lock().unwrap();
    for id in dead {
        // Re-check under the lock: a client may have attached meanwhile.
        let still_dead = st.sessions.get(&id).map(|s| !s.has_subscribers()).unwrap_or(false);
        if still_dead {
            st.sessions.remove(&id);
            eprintln!("arkosd: session {} reaped {}s after it ended", id.0, ttl.as_secs());
        }
    }
}
```

`Session.id` must be `pub` (it is `pub id: SessionId` today; if not, make it so). Dropping the last `Arc<Session>` closes the PTY master, which ends the reader thread's `read` with an error and the writer thread's channel; nothing else holds the session.

Add `mod reaper;` to `crates/arkosd/src/main.rs`.

- [ ] **Step 7: Options, flags, and starting the reaper**

In `crates/arkosd/src/server.rs`, add to `Options`:

```rust
    /// How often the reaper polls for exits and dead sessions.
    pub reaper_every: Duration,
    /// How long a dead session stays listed after it ended, while nobody is attached.
    pub dead_session_ttl: Duration,
```

In `server::run`, right after `crate::watchdog::start(state.clone(), opts.watchdog);` add `crate::reaper::start(state.clone(), opts.reaper_every, opts.dead_session_ttl);`.

In `crates/arkosd/src/main.rs`, add two flags beside the existing ones, following the exact parsing style used for `--watchdog-secs`:

```rust
            "--reaper-millis" => reaper_millis = args.next().and_then(|v| v.parse().ok()).expect("--reaper-millis needs a number"),
            "--dead-session-secs" => dead_session_secs = args.next().and_then(|v| v.parse().ok()).expect("--dead-session-secs needs a number"),
```

with `let mut reaper_millis: u64 = 1000;` and `let mut dead_session_secs: u64 = 600;` declared beside the other defaults, and in the `Options` literal `reaper_every: Duration::from_millis(reaper_millis.max(10)), dead_session_ttl: Duration::from_secs(dead_session_secs),`. Also validate `--replay-bytes` here: `if replay == 0 { eprintln!("arkosd: --replay-bytes must be at least 1"); std::process::exit(2); }` (milestone 1 left a zero value panicking a client thread).

- [ ] **Step 8: Validate ui_exe and the socket path**

In `server::dispatch`'s `Request::Hello` arm, where `ui_exe` is stored on the `ClientRecord`, replace the assignment with:

```rust
            let ui_exe = ui_exe.filter(|p| {
                let named_arkos = p.file_name().map(|n| n == "arkos").unwrap_or(false);
                let ok = named_arkos && p.is_file();
                if !ok {
                    eprintln!("arkosd: client {client_id} offered ui_exe {} which is not a arkos binary; it will not be relaunched", p.display());
                }
                ok
            });
```

(`client_id` is the variable name in `dispatch`; use whatever the function's parameter is called.)

In `server::run`, replace the stale-socket removal (`std::fs::remove_file(socket).ok();` or equivalent) and the parent handling with:

```rust
    let parent = socket.parent().ok_or_else(|| anyhow::anyhow!("socket path has no parent"))?;
    let created_parent = !parent.exists();
    std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    if created_parent {
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).ok();
    }
    match std::fs::symlink_metadata(socket) {
        Ok(meta) if meta.file_type().is_socket() => {
            std::fs::remove_file(socket).with_context(|| format!("remove stale socket {}", socket.display()))?;
        }
        Ok(_) => anyhow::bail!("{} exists and is not a socket; refusing to replace it", socket.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("inspect {}", socket.display())),
    }
```

with `use std::os::unix::fs::{FileTypeExt, PermissionsExt};` and `use anyhow::Context;` at the top if not already imported. Keep the existing `set_permissions(socket, 0o600)` after `bind`.

- [ ] **Step 9: Run the lifecycle tests, then the whole daemon suite ten times**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test -p arkosd --test daemon 2>&1 | tail -8`
Expected: all previous tests plus the five new ones pass.

Run: `for i in $(seq 1 10); do cargo test -p arkosd 2>&1 | grep -E 'test result|FAILED' ; done; pkill -x arkosd; pgrep -lx arkosd || echo clean`
Expected: ten green runs, no leftover daemon.

- [ ] **Step 10: Commit**

```bash
git add crates/arkosd
git commit -m "arkosd: detect exit under a held PTY, never signal a reaped pid, reap dead sessions"
```

---

### Task 2: The layout tree (pure)

**Files:**
- Create: `crates/arkos/src/tree.rs`
- Modify: `crates/arkos/Cargo.toml` (dev-dependency `proptest = "1"`), `crates/arkos/src/main.rs` (`mod tree;`)
- Test: inline unit tests and proptest properties in `tree.rs`

**Interfaces:**
- Produces (used by Tasks 3 to 9):
  - `tree::{PaneId, SplitId}` (`u64`), `Orientation { Horizontal, Vertical }` (Horizontal: children side by side, the result of "split right"; Vertical: stacked, "split down"), `Side { Left, Right, Top, Bottom }`, `Tab { Terminal { session: SessionId }, Viewer { path: PathBuf } }`, `Pane { id, tabs: Vec<Tab>, active: usize, mru: Vec<usize> }`, `Node { Leaf(Pane) | Split { id, orientation, ratio, first, second } }`, `Tree { focused: PaneId, maximized: Option<PaneId>, .. }`.
  - `Rect { x, y, w, h }` (f32 pixels), `Layout { panes: Vec<(PaneId, Rect)>, handles: Vec<(SplitId, Orientation, Rect, Rect)> }` where the two rects are the handle itself and the whole split's bounds (a drag maps the pointer into the second to get a ratio), constants `MIN_PANE_W = 160.0`, `MIN_PANE_H = 80.0`, `HANDLE = 4.0`.
  - `Tree::new(first: Tab) -> Tree`; `panes(&self) -> Vec<PaneId>` in reading order; `pane(&self, PaneId) -> Option<&Pane>`; `pane_mut`; `pane_index(&self, PaneId) -> Option<usize>`; `pane_at(&self, usize) -> Option<PaneId>`; `find_session(&self, SessionId) -> Option<(PaneId, usize)>`; `tabs(&self) -> Vec<(PaneId, usize, &Tab)>`.
  - `split(&mut self, pane: PaneId, side: Side, tab: Tab) -> Option<PaneId>`; `close_tab(&mut self, pane, idx) -> Option<Tab>`; `close_pane(&mut self, pane) -> bool` (false for the last pane); `move_tab(&mut self, from: PaneId, idx: usize, to: PaneId, at: Option<usize>) -> bool`; `activate(&mut self, pane, idx)`; `mru_next(&self, pane) -> Option<usize>`; `add_tab(&mut self, pane, tab) -> usize`; `replace_tab(&mut self, pane, idx, tab)`; `move_tab_within(&mut self, pane, idx, delta: isize) -> bool`.
  - `set_ratio(&mut self, split: SplitId, ratio: f32)`; `reset_ratio(&mut self, split)`; `split_of(&self, split) -> Option<(Orientation, f32)>`; `toggle_maximize(&mut self, pane)`.
  - `layout(&self, bounds: Rect) -> Layout`; `neighbor(&self, pane, dir: Side, bounds: Rect) -> Option<PaneId>`.
  - `Tree` derives `Serialize, Deserialize, Clone, Debug, PartialEq` (Task 3 persists it).

- [ ] **Step 1: Add proptest and write the failing unit tests**

In `crates/arkos/Cargo.toml`, under `[dev-dependencies]` (create the section if missing; `tempfile` is already there), add `proptest = "1"`.

Create `crates/arkos/src/tree.rs` with only the test module first (the implementation comes in step 3):

```rust
//! The layout model (spec 4): a binary split tree of panes, each pane an ordered list of tabs.
//! Pure data with no gpui types, so every operation is unit and property tested.

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn term(n: u64) -> Tab {
        Tab::Terminal { session: SessionId(n) }
    }

    fn bounds() -> Rect {
        Rect { x: 0.0, y: 0.0, w: 1200.0, h: 800.0 }
    }

    #[test]
    fn a_new_tree_has_one_pane_with_one_tab() {
        let t = Tree::new(term(1));
        assert_eq!(t.panes().len(), 1);
        let p = t.pane(t.focused).unwrap();
        assert_eq!(p.tabs, vec![term(1)]);
        assert_eq!(p.active, 0);
        assert_eq!(t.pane_index(t.focused), Some(0));
    }

    #[test]
    fn split_right_puts_the_new_pane_second_and_focuses_it() {
        let mut t = Tree::new(term(1));
        let left = t.focused;
        let right = t.split(left, Side::Right, term(2)).unwrap();
        assert_eq!(t.panes(), vec![left, right]);
        assert_eq!(t.focused, right);
        let l = t.layout(bounds());
        let (_, lr) = l.panes.iter().find(|(id, _)| *id == left).unwrap();
        let (_, rr) = l.panes.iter().find(|(id, _)| *id == right).unwrap();
        assert!(lr.x < rr.x && (lr.y - rr.y).abs() < 0.01);
        assert_eq!(l.handles.len(), 1);
        assert_eq!(l.handles[0].1, Orientation::Horizontal);
    }

    #[test]
    fn split_left_and_top_put_the_new_pane_first() {
        let mut t = Tree::new(term(1));
        let orig = t.focused;
        let left = t.split(orig, Side::Left, term(2)).unwrap();
        assert_eq!(t.panes(), vec![left, orig]);
        let top = t.split(orig, Side::Top, term(3)).unwrap();
        assert_eq!(t.panes(), vec![left, top, orig]);
        let l = t.layout(bounds());
        let r = |id| l.panes.iter().find(|(p, _)| *p == id).unwrap().1;
        assert!(r(top).y < r(orig).y);
        assert!(r(left).x < r(top).x);
    }

    #[test]
    fn screenshot_layout_numbers_left_then_top_right_then_bottom_right() {
        // Spec 4: a vertical split at 0.5 whose right child is a horizontal split at 0.5.
        let mut t = Tree::new(term(1));
        let left = t.focused;
        let right_top = t.split(left, Side::Right, term(2)).unwrap();
        let right_bottom = t.split(right_top, Side::Bottom, term(3)).unwrap();
        assert_eq!(t.panes(), vec![left, right_top, right_bottom]);
        assert_eq!(t.pane_at(1), Some(right_top));
        let l = t.layout(bounds());
        let r = |id| l.panes.iter().find(|(p, _)| *p == id).unwrap().1;
        assert!((r(left).w - (600.0 - HANDLE / 2.0)).abs() < 1.0);
        assert!((r(right_top).h - (400.0 - HANDLE / 2.0)).abs() < 1.0);
        assert_eq!(l.handles.len(), 2);
    }

    #[test]
    fn close_tab_keeps_an_empty_pane_open() {
        let mut t = Tree::new(term(1));
        let p = t.focused;
        assert_eq!(t.close_tab(p, 0), Some(term(1)));
        assert!(t.pane(p).unwrap().tabs.is_empty());
        assert_eq!(t.panes(), vec![p]);
    }

    #[test]
    fn close_tab_moves_active_to_the_neighbour() {
        let mut t = Tree::new(term(1));
        let p = t.focused;
        t.add_tab(p, term(2));
        t.add_tab(p, term(3));
        t.activate(p, 2);
        t.close_tab(p, 2);
        assert_eq!(t.pane(p).unwrap().active, 1);
        t.activate(p, 0);
        t.close_tab(p, 0);
        assert_eq!(t.pane(p).unwrap().active, 0);
        assert_eq!(t.pane(p).unwrap().tabs, vec![term(2)]);
    }

    #[test]
    fn last_pane_never_closes() {
        let mut t = Tree::new(term(1));
        let p = t.focused;
        t.close_tab(p, 0);
        assert!(!t.close_pane(p));
        assert_eq!(t.panes(), vec![p]);
    }

    #[test]
    fn close_pane_collapses_the_split_and_focuses_the_sibling() {
        let mut t = Tree::new(term(1));
        let a = t.focused;
        let b = t.split(a, Side::Right, term(2)).unwrap();
        let c = t.split(b, Side::Bottom, term(3)).unwrap();
        assert!(t.close_pane(b));
        assert_eq!(t.panes(), vec![a, c]);
        assert_eq!(t.focused, c);
        assert_eq!(t.layout(bounds()).handles.len(), 1);
        assert!(t.close_pane(c));
        assert_eq!(t.panes(), vec![a]);
        assert!(t.layout(bounds()).handles.is_empty());
    }

    #[test]
    fn move_tab_between_panes() {
        let mut t = Tree::new(term(1));
        let a = t.focused;
        let b = t.split(a, Side::Right, term(2)).unwrap();
        t.add_tab(a, term(3));
        assert!(t.move_tab(a, 1, b, Some(0)));
        assert_eq!(t.pane(a).unwrap().tabs, vec![term(1)]);
        assert_eq!(t.pane(b).unwrap().tabs, vec![term(3), term(2)]);
        assert_eq!(t.pane(b).unwrap().active, 0);
        // Without a position the tab goes last.
        assert!(t.move_tab(b, 0, a, None));
        assert_eq!(t.pane(a).unwrap().tabs, vec![term(1), term(3)]);
    }

    #[test]
    fn move_tab_within_a_pane_reorders() {
        let mut t = Tree::new(term(1));
        let p = t.focused;
        t.add_tab(p, term(2));
        t.add_tab(p, term(3));
        t.activate(p, 0);
        assert!(t.move_tab_within(p, 0, 1));
        assert_eq!(t.pane(p).unwrap().tabs, vec![term(2), term(1), term(3)]);
        assert_eq!(t.pane(p).unwrap().active, 1);
        assert!(!t.move_tab_within(p, 2, 1));
    }

    #[test]
    fn mru_cycles_most_recent_first() {
        let mut t = Tree::new(term(1));
        let p = t.focused;
        t.add_tab(p, term(2));
        t.add_tab(p, term(3));
        t.activate(p, 2);
        t.activate(p, 0);
        assert_eq!(t.mru_next(p), Some(2));
        t.close_tab(p, 2);
        assert_eq!(t.mru_next(p), Some(1));
    }

    #[test]
    fn ratio_respects_minimum_pane_size() {
        let mut t = Tree::new(term(1));
        let a = t.focused;
        let b = t.split(a, Side::Right, term(2)).unwrap();
        let split = t.layout(bounds()).handles[0].0;
        t.set_ratio(split, 0.01);
        let l = t.layout(bounds());
        let r = |id| l.panes.iter().find(|(p, _)| *p == id).unwrap().1;
        assert!(r(a).w >= MIN_PANE_W - 0.01);
        t.reset_ratio(split);
        assert_eq!(t.split_of(split).unwrap().1, 0.5);
        assert!(r(b).w > 0.0);
    }

    #[test]
    fn maximize_gives_one_pane_the_whole_window() {
        let mut t = Tree::new(term(1));
        let a = t.focused;
        let b = t.split(a, Side::Right, term(2)).unwrap();
        t.toggle_maximize(b);
        let l = t.layout(bounds());
        assert_eq!(l.panes.len(), 1);
        assert_eq!(l.panes[0].0, b);
        assert!(l.handles.is_empty());
        assert_eq!(t.panes().len(), 2, "numbering still counts every pane");
        t.toggle_maximize(b);
        assert_eq!(t.layout(bounds()).panes.len(), 2);
    }

    #[test]
    fn neighbor_follows_geometry() {
        let mut t = Tree::new(term(1));
        let left = t.focused;
        let right_top = t.split(left, Side::Right, term(2)).unwrap();
        let right_bottom = t.split(right_top, Side::Bottom, term(3)).unwrap();
        assert_eq!(t.neighbor(left, Side::Right, bounds()), Some(right_top));
        assert_eq!(t.neighbor(right_bottom, Side::Left, bounds()), Some(left));
        assert_eq!(t.neighbor(right_top, Side::Bottom, bounds()), Some(right_bottom));
        assert_eq!(t.neighbor(right_bottom, Side::Top, bounds()), Some(right_top));
        assert_eq!(t.neighbor(left, Side::Left, bounds()), None);
    }

    #[test]
    fn find_session_locates_a_tab() {
        let mut t = Tree::new(term(1));
        let a = t.focused;
        let b = t.split(a, Side::Right, term(2)).unwrap();
        assert_eq!(t.find_session(SessionId(2)), Some((b, 0)));
        assert_eq!(t.find_session(SessionId(9)), None);
        t.replace_tab(b, 0, term(9));
        assert_eq!(t.find_session(SessionId(9)), Some((b, 0)));
    }

    #[test]
    fn serializes_round_trip() {
        let mut t = Tree::new(term(1));
        let a = t.focused;
        t.split(a, Side::Bottom, term(2));
        let json = serde_json::to_string(&t).unwrap();
        let back: Tree = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }

    #[derive(Debug, Clone)]
    enum Op {
        Split(usize, Side, u64),
        CloseTab(usize, usize),
        ClosePane(usize),
        MoveTab(usize, usize, usize, Option<usize>),
        Ratio(usize, f32),
        Maximize(usize),
        Activate(usize, usize),
    }

    fn op() -> impl Strategy<Value = Op> {
        let side = prop_oneof![Just(Side::Left), Just(Side::Right), Just(Side::Top), Just(Side::Bottom)];
        prop_oneof![
            (0..8usize, side, 1..1000u64).prop_map(|(p, s, n)| Op::Split(p, s, n)),
            (0..8usize, 0..6usize).prop_map(|(p, i)| Op::CloseTab(p, i)),
            (0..8usize).prop_map(Op::ClosePane),
            (0..8usize, 0..6usize, 0..8usize, proptest::option::of(0..6usize)).prop_map(|(f, i, t, a)| Op::MoveTab(f, i, t, a)),
            (0..8usize, 0.0f32..1.0).prop_map(|(s, r)| Op::Ratio(s, r)),
            (0..8usize).prop_map(Op::Maximize),
            (0..8usize, 0..6usize).prop_map(|(p, i)| Op::Activate(p, i)),
        ]
    }

    /// Applies an op to whatever pane or split the indices name modulo what exists.
    fn apply(t: &mut Tree, op: &Op, next: &mut u64) {
        let panes = t.panes();
        let pick = |i: usize| panes[i % panes.len()];
        match *op {
            Op::Split(p, side, _) => {
                *next += 1;
                t.split(pick(p), side, term(*next));
            }
            Op::CloseTab(p, i) => {
                let p = pick(p);
                let n = t.pane(p).unwrap().tabs.len();
                if n > 0 {
                    t.close_tab(p, i % n);
                }
            }
            Op::ClosePane(p) => {
                t.close_pane(pick(p));
            }
            Op::MoveTab(f, i, to, at) => {
                let (f, to) = (pick(f), pick(to));
                let n = t.pane(f).unwrap().tabs.len();
                if n > 0 {
                    t.move_tab(f, i % n, to, at);
                }
            }
            Op::Ratio(s, r) => {
                let handles = t.layout(bounds()).handles;
                if !handles.is_empty() {
                    t.set_ratio(handles[s % handles.len()].0, r);
                }
            }
            Op::Maximize(p) => t.toggle_maximize(pick(p)),
            Op::Activate(p, i) => {
                let p = pick(p);
                let n = t.pane(p).unwrap().tabs.len();
                if n > 0 {
                    t.activate(p, i % n);
                }
            }
        }
    }

    fn check_invariants(t: &Tree) {
        let panes = t.panes();
        assert!(!panes.is_empty());
        let mut seen = std::collections::HashSet::new();
        for p in &panes {
            assert!(seen.insert(*p), "duplicate pane id");
            let pane = t.pane(*p).expect("every listed pane resolves");
            assert!(pane.tabs.is_empty() || pane.active < pane.tabs.len(), "active in range");
            for i in &pane.mru {
                assert!(*i < pane.tabs.len(), "mru in range");
            }
        }
        assert!(panes.contains(&t.focused), "focused pane exists");
        if let Some(m) = t.maximized {
            assert!(panes.contains(&m));
        }
        let l = t.layout(bounds());
        let shown: Vec<PaneId> = l.panes.iter().map(|(id, _)| *id).collect();
        match t.maximized {
            Some(m) => assert_eq!(shown, vec![m]),
            None => assert_eq!(shown, panes, "layout lists panes in reading order"),
        }
        for (i, (_, a)) in l.panes.iter().enumerate() {
            assert!(a.w >= 0.0 && a.h >= 0.0);
            for (_, b) in l.panes.iter().skip(i + 1) {
                let overlap = a.x < b.x + b.w - 0.01 && b.x < a.x + a.w - 0.01 && a.y < b.y + b.h - 0.01 && b.y < a.y + a.h - 0.01;
                assert!(!overlap, "panes overlap: {a:?} {b:?}");
            }
        }
        for (_, _, r, area) in &l.handles {
            assert!(r.w > 0.0 && r.h > 0.0);
            assert!(area.w >= r.w && area.h >= r.h);
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(300))]

        #[test]
        fn random_operations_keep_the_tree_valid(ops in proptest::collection::vec(op(), 0..40)) {
            let mut t = Tree::new(term(1));
            let mut next = 1;
            for op in &ops {
                apply(&mut t, op, &mut next);
                check_invariants(&t);
            }
        }

        #[test]
        fn panes_are_numbered_in_reading_order(ops in proptest::collection::vec(op(), 0..30)) {
            let mut t = Tree::new(term(1));
            let mut next = 1;
            for op in &ops {
                apply(&mut t, op, &mut next);
            }
            t.maximized = None;
            let l = t.layout(bounds());
            // Reading order: for consecutive panes, either the next one starts further right on
            // the same band, or it starts lower.
            for w in l.panes.windows(2) {
                let (a, b) = (w[0].1, w[1].1);
                let same_band = (a.y - b.y).abs() < 0.01;
                prop_assert!(if same_band { b.x > a.x } else { b.y > a.y || b.x < a.x + a.w }, "{a:?} then {b:?}");
            }
        }

        #[test]
        fn every_pane_is_reachable(ops in proptest::collection::vec(op(), 0..30)) {
            let mut t = Tree::new(term(1));
            let mut next = 1;
            for op in &ops {
                apply(&mut t, op, &mut next);
            }
            let panes = t.panes();
            for (i, p) in panes.iter().enumerate() {
                prop_assert_eq!(t.pane_at(i), Some(*p));
                prop_assert_eq!(t.pane_index(*p), Some(i));
            }
            prop_assert_eq!(t.pane_at(panes.len()), None);
        }

        #[test]
        fn move_tab_onto_itself_is_a_no_op(ops in proptest::collection::vec(op(), 0..20), pick in 0..8usize, at in proptest::option::of(0..6usize)) {
            let mut t = Tree::new(term(1));
            let mut next = 1;
            for op in &ops {
                apply(&mut t, op, &mut next);
            }
            let panes = t.panes();
            let p = panes[pick % panes.len()];
            let n = t.pane(p).unwrap().tabs.len();
            if n > 0 {
                let idx = pick % n;
                let before = t.clone();
                let at = at.map(|a| a % (n + 1));
                // Same pane, same position (or the position a removal-then-insert lands on).
                let same_spot = match at { None => idx == n - 1, Some(a) => a == idx || a == idx + 1 };
                if same_spot {
                    t.move_tab(p, idx, p, at);
                    prop_assert_eq!(t, before);
                }
            }
        }
    }
}
```

- [ ] **Step 2: Run the tests to see them fail to compile**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test -p arkos tree 2>&1 | grep -E '^error' | head -5`
Expected: errors such as `cannot find type Tree`, `cannot find value HANDLE`.

- [ ] **Step 3: Implement the tree**

Insert the implementation above the `#[cfg(test)]` module in `crates/arkos/src/tree.rs`:

```rust
use protocol::SessionId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub type PaneId = u64;
pub type SplitId = u64;

pub const MIN_PANE_W: f32 = 160.0;
pub const MIN_PANE_H: f32 = 80.0;
/// The visible width of a split handle. The hit area is wider (the view decides).
pub const HANDLE: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
    /// Children side by side ("split right").
    Horizontal,
    /// Children stacked ("split down").
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    fn orientation(self) -> Orientation {
        match self {
            Side::Left | Side::Right => Orientation::Horizontal,
            Side::Top | Side::Bottom => Orientation::Vertical,
        }
    }

    /// Whether the new pane goes before the existing one in the split.
    fn new_pane_first(self) -> bool {
        matches!(self, Side::Left | Side::Top)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tab {
    Terminal { session: SessionId },
    /// Milestone 4's read-only viewer. Persisted now so the format does not change later.
    Viewer { path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pane {
    pub id: PaneId,
    pub tabs: Vec<Tab>,
    pub active: usize,
    /// Tab indices, most recently activated first. Drives Ctrl+Tab.
    #[serde(default)]
    pub mru: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Node {
    Leaf(Pane),
    Split { id: SplitId, orientation: Orientation, ratio: f32, first: Box<Node>, second: Box<Node> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tree {
    root: Node,
    next_id: u64,
    pub focused: PaneId,
    pub maximized: Option<PaneId>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layout {
    pub panes: Vec<(PaneId, Rect)>,
    /// (split, orientation, handle rect, the split's whole bounds).
    pub handles: Vec<(SplitId, Orientation, Rect, Rect)>,
}

const MIN_RATIO: f32 = 0.05;
const MAX_RATIO: f32 = 0.95;

impl Node {
    fn leaves(&self, out: &mut Vec<PaneId>) {
        match self {
            Node::Leaf(p) => out.push(p.id),
            Node::Split { first, second, .. } => {
                first.leaves(out);
                second.leaves(out);
            }
        }
    }

    fn find(&self, id: PaneId) -> Option<&Pane> {
        match self {
            Node::Leaf(p) => (p.id == id).then_some(p),
            Node::Split { first, second, .. } => first.find(id).or_else(|| second.find(id)),
        }
    }

    fn find_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        match self {
            Node::Leaf(p) => (p.id == id).then_some(p),
            Node::Split { first, second, .. } => match first.find_mut(id) {
                Some(p) => Some(p),
                None => second.find_mut(id),
            },
        }
    }

    fn find_split_mut(&mut self, id: SplitId) -> Option<&mut Node> {
        match self {
            Node::Leaf(_) => None,
            Node::Split { id: sid, first, second, .. } => {
                if *sid == id {
                    return Some(self);
                }
                match first.find_split_mut(id) {
                    Some(n) => Some(n),
                    None => second.find_split_mut(id),
                }
            }
        }
    }

    fn find_split(&self, id: SplitId) -> Option<(Orientation, f32)> {
        match self {
            Node::Leaf(_) => None,
            Node::Split { id: sid, orientation, ratio, first, second } => {
                if *sid == id {
                    return Some((*orientation, *ratio));
                }
                first.find_split(id).or_else(|| second.find_split(id))
            }
        }
    }

    /// Replaces the leaf `id` with a split holding it and a new pane; returns whether it happened.
    fn split_leaf(&mut self, id: PaneId, side: Side, new_pane: Pane, split_id: SplitId) -> bool {
        match self {
            Node::Leaf(p) if p.id == id => {
                let existing = std::mem::replace(self, Node::Leaf(Pane { id: 0, tabs: vec![], active: 0, mru: vec![] }));
                let new_leaf = Node::Leaf(new_pane);
                let (first, second) = if side.new_pane_first() { (new_leaf, existing) } else { (existing, new_leaf) };
                *self = Node::Split { id: split_id, orientation: side.orientation(), ratio: 0.5, first: Box::new(first), second: Box::new(second) };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { first, second, .. } => first.split_leaf(id, side, new_pane.clone(), split_id) || second.split_leaf(id, side, new_pane, split_id),
        }
    }

    /// Removes leaf `id`, promoting its sibling. Returns the sibling's first pane for focus.
    fn remove_leaf(&mut self, id: PaneId) -> Option<PaneId> {
        if let Node::Split { first, second, .. } = self {
            let (victim_first, victim_second) = (matches!(**first, Node::Leaf(ref p) if p.id == id), matches!(**second, Node::Leaf(ref p) if p.id == id));
            if victim_first || victim_second {
                let keep = if victim_first { std::mem::replace(&mut **second, placeholder()) } else { std::mem::replace(&mut **first, placeholder()) };
                *self = keep;
                let mut leaves = Vec::new();
                self.leaves(&mut leaves);
                return leaves.first().copied();
            }
            return first.remove_leaf(id).or_else(|| second.remove_leaf(id));
        }
        None
    }

    fn layout_into(&self, bounds: Rect, out: &mut Layout) {
        match self {
            Node::Leaf(p) => out.panes.push((p.id, bounds)),
            Node::Split { id, orientation, ratio, first, second } => {
                let (first_b, handle, second_b) = split_rects(bounds, *orientation, *ratio);
                out.handles.push((*id, *orientation, handle, bounds));
                first.layout_into(first_b, out);
                second.layout_into(second_b, out);
            }
        }
    }
}

fn placeholder() -> Node {
    Node::Leaf(Pane { id: 0, tabs: vec![], active: 0, mru: vec![] })
}

/// Splits `bounds` along `orientation` at `ratio`, leaving a handle between the halves. Each half
/// keeps at least the minimum pane size when the whole is big enough for two minimums.
fn split_rects(bounds: Rect, orientation: Orientation, ratio: f32) -> (Rect, Rect, Rect) {
    match orientation {
        Orientation::Horizontal => {
            let total = (bounds.w - HANDLE).max(0.0);
            let first_w = clamp_first(total, ratio, MIN_PANE_W);
            let first = Rect { x: bounds.x, y: bounds.y, w: first_w, h: bounds.h };
            let handle = Rect { x: bounds.x + first_w, y: bounds.y, w: HANDLE, h: bounds.h };
            let second = Rect { x: bounds.x + first_w + HANDLE, y: bounds.y, w: total - first_w, h: bounds.h };
            (first, handle, second)
        }
        Orientation::Vertical => {
            let total = (bounds.h - HANDLE).max(0.0);
            let first_h = clamp_first(total, ratio, MIN_PANE_H);
            let first = Rect { x: bounds.x, y: bounds.y, w: bounds.w, h: first_h };
            let handle = Rect { x: bounds.x, y: bounds.y + first_h, w: bounds.w, h: HANDLE };
            let second = Rect { x: bounds.x, y: bounds.y + first_h + HANDLE, w: bounds.w, h: total - first_h };
            (first, handle, second)
        }
    }
}

fn clamp_first(total: f32, ratio: f32, min: f32) -> f32 {
    let wanted = total * ratio;
    if total >= 2.0 * min {
        wanted.clamp(min, total - min)
    } else {
        wanted.clamp(0.0, total)
    }
}

impl Tree {
    pub fn new(first: Tab) -> Tree {
        let pane = Pane { id: 1, tabs: vec![first], active: 0, mru: vec![0] };
        Tree { root: Node::Leaf(pane), next_id: 2, focused: 1, maximized: None }
    }

    fn fresh_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Pane ids in reading order: left to right, top to bottom. This is the Cmd+1 to Cmd+9 order.
    pub fn panes(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        self.root.leaves(&mut out);
        out
    }

    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.root.find(id)
    }

    pub fn pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        self.root.find_mut(id)
    }

    pub fn pane_index(&self, id: PaneId) -> Option<usize> {
        self.panes().iter().position(|p| *p == id)
    }

    pub fn pane_at(&self, index: usize) -> Option<PaneId> {
        self.panes().get(index).copied()
    }

    pub fn find_session(&self, session: SessionId) -> Option<(PaneId, usize)> {
        for id in self.panes() {
            let pane = self.pane(id)?;
            if let Some(i) = pane.tabs.iter().position(|t| matches!(t, Tab::Terminal { session: s } if *s == session)) {
                return Some((id, i));
            }
        }
        None
    }

    pub fn tabs(&self) -> Vec<(PaneId, usize, &Tab)> {
        let mut out = Vec::new();
        for id in self.panes() {
            if let Some(pane) = self.pane(id) {
                for (i, t) in pane.tabs.iter().enumerate() {
                    out.push((id, i, t));
                }
            }
        }
        out
    }

    pub fn split(&mut self, pane: PaneId, side: Side, tab: Tab) -> Option<PaneId> {
        self.pane(pane)?;
        let new_id = self.fresh_id();
        let split_id = self.fresh_id();
        let new_pane = Pane { id: new_id, tabs: vec![tab], active: 0, mru: vec![0] };
        if !self.root.split_leaf(pane, side, new_pane, split_id) {
            return None;
        }
        self.focused = new_id;
        self.maximized = None;
        Some(new_id)
    }

    pub fn add_tab(&mut self, pane: PaneId, tab: Tab) -> usize {
        let p = self.pane_mut(pane).expect("pane exists");
        p.tabs.push(tab);
        let idx = p.tabs.len() - 1;
        p.active = idx;
        p.mru.retain(|i| *i != idx);
        p.mru.insert(0, idx);
        idx
    }

    pub fn replace_tab(&mut self, pane: PaneId, idx: usize, tab: Tab) {
        if let Some(p) = self.pane_mut(pane) {
            if idx < p.tabs.len() {
                p.tabs[idx] = tab;
            }
        }
    }

    pub fn activate(&mut self, pane: PaneId, idx: usize) {
        if let Some(p) = self.pane_mut(pane) {
            if idx < p.tabs.len() {
                p.active = idx;
                p.mru.retain(|i| *i != idx);
                p.mru.insert(0, idx);
            }
        }
        self.focused = pane;
    }

    /// The tab Ctrl+Tab would switch to: the most recently used one that is not active.
    pub fn mru_next(&self, pane: PaneId) -> Option<usize> {
        let p = self.pane(pane)?;
        p.mru.iter().copied().find(|i| *i != p.active && *i < p.tabs.len())
    }

    /// Removes a tab; the pane stays even when it empties (the empty state).
    pub fn close_tab(&mut self, pane: PaneId, idx: usize) -> Option<Tab> {
        let p = self.pane_mut(pane)?;
        if idx >= p.tabs.len() {
            return None;
        }
        let tab = p.tabs.remove(idx);
        p.mru.retain(|i| *i != idx);
        for i in p.mru.iter_mut() {
            if *i > idx {
                *i -= 1;
            }
        }
        if p.tabs.is_empty() {
            p.active = 0;
        } else if p.active > idx || p.active == p.tabs.len() {
            p.active -= 1;
        }
        Some(tab)
    }

    /// Removes a pane and its tabs. The last pane never closes.
    pub fn close_pane(&mut self, pane: PaneId) -> bool {
        if matches!(self.root, Node::Leaf(_)) {
            return false;
        }
        let Some(next) = self.root.remove_leaf(pane) else { return false };
        if self.focused == pane {
            self.focused = next;
        }
        if self.maximized == Some(pane) {
            self.maximized = None;
        }
        true
    }

    /// Moves a tab to another pane (or another position in the same pane). `at` is the index to
    /// insert at in the target's list before removal is accounted for; `None` appends.
    pub fn move_tab(&mut self, from: PaneId, idx: usize, to: PaneId, at: Option<usize>) -> bool {
        let n_from = match self.pane(from) {
            Some(p) if idx < p.tabs.len() => p.tabs.len(),
            _ => return false,
        };
        if self.pane(to).is_none() {
            return false;
        }
        if from == to {
            let target = at.unwrap_or(n_from - 1);
            let landing = if target > idx { target - 1 } else { target };
            if landing == idx {
                return true;
            }
            let tab = self.close_tab(from, idx).expect("tab exists");
            let p = self.pane_mut(to).expect("pane exists");
            let landing = landing.min(p.tabs.len());
            p.tabs.insert(landing, tab);
            for i in p.mru.iter_mut() {
                if *i >= landing {
                    *i += 1;
                }
            }
            p.active = landing;
            p.mru.retain(|i| *i != landing);
            p.mru.insert(0, landing);
            return true;
        }
        let tab = self.close_tab(from, idx).expect("tab exists");
        let p = self.pane_mut(to).expect("pane exists");
        let landing = at.unwrap_or(p.tabs.len()).min(p.tabs.len());
        p.tabs.insert(landing, tab);
        for i in p.mru.iter_mut() {
            if *i >= landing {
                *i += 1;
            }
        }
        p.active = landing;
        p.mru.retain(|i| *i != landing);
        p.mru.insert(0, landing);
        self.focused = to;
        true
    }

    /// Moves a tab left (negative) or right within its pane.
    pub fn move_tab_within(&mut self, pane: PaneId, idx: usize, delta: isize) -> bool {
        let n = match self.pane(pane) {
            Some(p) => p.tabs.len(),
            None => return false,
        };
        let target = idx as isize + delta;
        if idx >= n || target < 0 || target as usize >= n {
            return false;
        }
        let p = self.pane_mut(pane).expect("pane exists");
        let tab = p.tabs.remove(idx);
        p.tabs.insert(target as usize, tab);
        let remap = |i: usize| -> usize {
            if i == idx {
                target as usize
            } else if delta > 0 && i > idx && i <= target as usize {
                i - 1
            } else if delta < 0 && i >= target as usize && i < idx {
                i + 1
            } else {
                i
            }
        };
        p.active = remap(p.active);
        for i in p.mru.iter_mut() {
            *i = remap(*i);
        }
        true
    }

    pub fn set_ratio(&mut self, split: SplitId, ratio: f32) {
        if let Some(Node::Split { ratio: r, .. }) = self.root.find_split_mut(split) {
            *r = ratio.clamp(MIN_RATIO, MAX_RATIO);
        }
    }

    pub fn reset_ratio(&mut self, split: SplitId) {
        self.set_ratio(split, 0.5);
    }

    pub fn split_of(&self, split: SplitId) -> Option<(Orientation, f32)> {
        self.root.find_split(split)
    }

    pub fn toggle_maximize(&mut self, pane: PaneId) {
        if self.pane(pane).is_none() {
            return;
        }
        self.maximized = if self.maximized == Some(pane) { None } else { Some(pane) };
        self.focused = pane;
    }

    /// Pane and handle rectangles for `bounds`. A maximized pane takes everything.
    pub fn layout(&self, bounds: Rect) -> Layout {
        let mut out = Layout::default();
        if let Some(m) = self.maximized {
            if self.pane(m).is_some() {
                out.panes.push((m, bounds));
                return out;
            }
        }
        self.root.layout_into(bounds, &mut out);
        out
    }

    /// The pane in direction `dir` from `pane`: the one across the gap with the most overlap along
    /// the other axis, nearest first.
    pub fn neighbor(&self, pane: PaneId, dir: Side, bounds: Rect) -> Option<PaneId> {
        let mut t = self.clone();
        t.maximized = None;
        let l = t.layout(bounds);
        let cur = l.panes.iter().find(|(id, _)| *id == pane)?.1;
        let mut best: Option<(PaneId, f32, f32)> = None;
        for (id, r) in &l.panes {
            if *id == pane {
                continue;
            }
            let (across, distance, overlap) = match dir {
                Side::Left => (r.x + r.w <= cur.x + HANDLE + 0.5, cur.x - (r.x + r.w), overlap(r.y, r.h, cur.y, cur.h)),
                Side::Right => (r.x >= cur.x + cur.w - HANDLE - 0.5, r.x - (cur.x + cur.w), overlap(r.y, r.h, cur.y, cur.h)),
                Side::Top => (r.y + r.h <= cur.y + HANDLE + 0.5, cur.y - (r.y + r.h), overlap(r.x, r.w, cur.x, cur.w)),
                Side::Bottom => (r.y >= cur.y + cur.h - HANDLE - 0.5, r.y - (cur.y + cur.h), overlap(r.x, r.w, cur.x, cur.w)),
            };
            if !across || overlap <= 0.0 {
                continue;
            }
            let better = match best {
                None => true,
                Some((_, d, o)) => distance < d - 0.5 || ((distance - d).abs() <= 0.5 && overlap > o),
            };
            if better {
                best = Some((*id, distance, overlap));
            }
        }
        best.map(|(id, _, _)| id)
    }
}

fn overlap(a: f32, aw: f32, b: f32, bw: f32) -> f32 {
    (a + aw).min(b + bw) - a.max(b)
}
```

Add `mod tree;` to `crates/arkos/src/main.rs` (alphabetically among the other `mod` lines). Until Task 5 uses it, put `#![allow(dead_code)] // Used from Task 5 onward` at the top of `tree.rs`.

- [ ] **Step 4: Run the tests**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test -p arkos tree 2>&1 | tail -6`
Expected: all unit tests and the four properties pass (proptest runs 300 cases each; allow a minute). If a property finds a counterexample, it prints the minimal op sequence; fix the operation, not the property.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/Cargo.toml crates/arkos/src/tree.rs crates/arkos/src/main.rs Cargo.lock
git commit -m "arkos: layout tree with splits, tabs, reading-order numbering and property tests"
```

---

### Task 3: Workspace v2: the whole layout, stable file names, debounced saves

**Files:**
- Modify: `crates/arkos/src/workspace.rs` (rewrite), `crates/arkos/src/paths.rs`
- Test: inline in both

**Interfaces:**
- Consumes: `tree::{Tree, Tab}`, `paths::workspace_file`.
- Produces:
  - `workspace::Workspace { version: u32, project: PathBuf, tree: Tree }`, `FORMAT_VERSION = 2`, `SAVE_DEBOUNCE = 300 ms`.
  - `workspace::load(project: &Path) -> Option<Workspace>`; `load_from(file: &Path, project: &Path) -> Option<Workspace>` (reads v2, migrates v1 `{ "sessions": [id] }` into a one-pane tree, logs corrupt files); `save(project, &Workspace) -> io::Result<()>`; `save_to(file, &Workspace) -> io::Result<()>` (temp file plus rename, as today).
  - `workspace::SaveQueue { offer(&mut self, Workspace) -> bool, take(&mut self) -> Option<Workspace> }`, `SharedSaveQueue = Arc<Mutex<SaveQueue>>`, and `request_save(queue: &SharedSaveQueue, ws: Workspace, cx: &mut App)` which coalesces calls within the debounce and writes once on the background executor.
  - `paths::workspace_file(project) -> PathBuf` now names the file by FNV-1a of the path, stable across Rust releases: `<16 hex>.json`.

Milestone 1 used `DefaultHasher`, whose output is not stable across toolchains, so a v1 file written by that build is not found by name. The only v1 user is this machine; the launch then creates a fresh shell, which is the spec's behaviour for a missing session. No scan of old files is attempted.

- [ ] **Step 1: Write the failing tests**

Replace the test module of `crates/arkos/src/workspace.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Side;

    fn tree_with_two() -> Tree {
        let mut t = Tree::new(Tab::Terminal { session: SessionId(1) });
        let a = t.focused;
        t.split(a, Side::Right, Tab::Terminal { session: SessionId(2) });
        t
    }

    #[test]
    fn round_trips_a_layout() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ws.json");
        let project = Path::new("/projects/x");
        assert_eq!(load_from(&file, project), None);
        let ws = Workspace { version: FORMAT_VERSION, project: project.to_path_buf(), tree: tree_with_two() };
        save_to(&file, &ws).unwrap();
        assert_eq!(load_from(&file, project), Some(ws));
    }

    #[test]
    fn migrates_a_milestone_1_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ws.json");
        std::fs::write(&file, r#"{ "sessions": [42] }"#).unwrap();
        let ws = load_from(&file, Path::new("/p")).expect("migrated");
        assert_eq!(ws.version, FORMAT_VERSION);
        assert_eq!(ws.project, PathBuf::from("/p"));
        assert_eq!(ws.tree, Tree::new(Tab::Terminal { session: SessionId(42) }));
        std::fs::write(&file, r#"{ "sessions": [] }"#).unwrap();
        assert_eq!(load_from(&file, Path::new("/p")), None);
    }

    #[test]
    fn corrupt_file_reads_as_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ws.json");
        std::fs::write(&file, "{not json").unwrap();
        assert_eq!(load_from(&file, Path::new("/p")), None);
    }

    #[test]
    fn save_leaves_no_temp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("workspaces").join("ws.json");
        let ws = Workspace { version: FORMAT_VERSION, project: PathBuf::from("/p"), tree: tree_with_two() };
        save_to(&file, &ws).unwrap();
        save_to(&file, &ws).unwrap();
        let names: Vec<String> = std::fs::read_dir(file.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["ws.json".to_string()]);
    }

    #[test]
    fn save_reports_an_unwritable_target() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, "").unwrap();
        let ws = Workspace { version: FORMAT_VERSION, project: PathBuf::from("/p"), tree: tree_with_two() };
        assert!(save_to(&blocker.join("ws.json"), &ws).is_err());
    }

    #[test]
    fn save_queue_coalesces_until_taken() {
        let mut q = SaveQueue::default();
        let a = Workspace { version: FORMAT_VERSION, project: PathBuf::from("/a"), tree: tree_with_two() };
        let b = Workspace { version: FORMAT_VERSION, project: PathBuf::from("/b"), tree: tree_with_two() };
        assert!(q.offer(a.clone()), "first offer schedules a flush");
        assert!(!q.offer(b.clone()), "a flush is already pending");
        assert_eq!(q.take(), Some(b), "the latest offer wins");
        assert_eq!(q.take(), None);
        assert!(q.offer(a), "after a flush the next offer schedules again");
    }
}
```

Append to the tests in `crates/arkos/src/paths.rs` (create the module if the file has none):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_file_name_is_stable_across_builds() {
        // FNV-1a of the path bytes; the value is pinned so a toolchain change cannot orphan files.
        let f = workspace_file(Path::new("/tmp/a"));
        assert_eq!(f.file_name().unwrap().to_str().unwrap(), "6cc13bddf2746ce7.json");
        assert_ne!(workspace_file(Path::new("/tmp/b")), f);
        assert!(f.to_string_lossy().contains("workspaces"));
    }
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test -p arkos workspace paths 2>&1 | grep -E '^error|FAILED|panicked' | head`
Expected: compile errors (`Workspace` has no `tree` field, `load_from` takes one argument, `SaveQueue` missing) and the paths test failing on the file name.

- [ ] **Step 3: Rewrite workspace.rs**

Replace everything above the test module in `crates/arkos/src/workspace.rs` with:

```rust
//! Remembers a project's layout: the split tree with every tab's session (spec 4). Saved on
//! every change, debounced, atomically, and off the UI thread.
use crate::paths::workspace_file;
use crate::tree::{Tab, Tree};
use gpui::App;
use protocol::SessionId;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const FORMAT_VERSION: u32 = 2;
/// Layout changes within this window collapse into one write.
pub const SAVE_DEBOUNCE: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Workspace {
    pub version: u32,
    pub project: PathBuf,
    pub tree: Tree,
}

/// Milestone 1 wrote `{ "sessions": [id] }`: one session, no layout.
#[derive(Deserialize)]
struct V1 {
    sessions: Vec<u64>,
}

/// One small read; callers may run it on the main thread before the window opens.
pub fn load(project: &Path) -> Option<Workspace> {
    load_from(&workspace_file(project), project)
}

/// Writes to disk; run it off the UI thread (see `request_save`).
pub fn save(project: &Path, ws: &Workspace) -> std::io::Result<()> {
    save_to(&workspace_file(project), ws)
}

/// A missing file means no saved layout. An unreadable or corrupt file also loads as nothing,
/// but is reported on stderr, since it orphans sessions the daemon is still running.
pub fn load_from(file: &Path, project: &Path) -> Option<Workspace> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            eprintln!("arkos: cannot read workspace {}: {e}", file.display());
            return None;
        }
    };
    if let Ok(ws) = serde_json::from_str::<Workspace>(&text) {
        return Some(ws);
    }
    match serde_json::from_str::<V1>(&text) {
        Ok(v1) => {
            let session = v1.sessions.first().map(|id| SessionId(*id))?;
            eprintln!("arkos: migrating milestone 1 workspace {}", file.display());
            Some(Workspace { version: FORMAT_VERSION, project: project.to_path_buf(), tree: Tree::new(Tab::Terminal { session }) })
        }
        Err(e) => {
            eprintln!("arkos: ignoring corrupt workspace {}: {e}", file.display());
            None
        }
    }
}

/// Writes a sibling temp file and renames it over `file`. The rename is atomic on one volume, so
/// a crash mid-save leaves the previous workspace intact instead of a torn file.
pub fn save_to(file: &Path, ws: &Workspace) -> std::io::Result<()> {
    let parent = file.parent().ok_or_else(|| std::io::Error::other("workspace path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let name = file.file_name().ok_or_else(|| std::io::Error::other("workspace path has no file name"))?;
    let tmp = parent.join(format!(".{}.{}.tmp", name.to_string_lossy(), std::process::id()));
    let text = serde_json::to_string_pretty(ws).map_err(std::io::Error::other)?;
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, file)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// Coalesces rapid saves: the latest workspace wins and one write happens per debounce window.
#[derive(Default)]
pub struct SaveQueue {
    latest: Option<Workspace>,
    scheduled: bool,
}

impl SaveQueue {
    /// Returns true when the caller must schedule a flush; false when one is already pending.
    pub fn offer(&mut self, ws: Workspace) -> bool {
        self.latest = Some(ws);
        if self.scheduled {
            return false;
        }
        self.scheduled = true;
        true
    }

    /// The workspace to write now. Clears the schedule so the next offer schedules again.
    pub fn take(&mut self) -> Option<Workspace> {
        self.scheduled = false;
        self.latest.take()
    }
}

pub type SharedSaveQueue = Arc<Mutex<SaveQueue>>;

/// Call on the UI thread after every layout change. The debounce timer and the write both run
/// off the UI thread; failures are logged.
pub fn request_save(queue: &SharedSaveQueue, ws: Workspace, cx: &mut App) {
    if !queue.lock().unwrap().offer(ws) {
        return;
    }
    let queue = queue.clone();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(SAVE_DEBOUNCE).await;
        let ws = queue.lock().unwrap().take();
        if let Some(ws) = ws {
            cx.background_spawn(async move {
                if let Err(e) = save(&ws.project, &ws) {
                    eprintln!("arkos: cannot save workspace: {e}");
                }
            })
            .await;
        }
    })
    .detach();
}
```

- [ ] **Step 4: Stable file names in paths.rs**

Replace `workspace_file` in `crates/arkos/src/paths.rs` with:

```rust
/// FNV-1a over the path bytes. Rust's `DefaultHasher` is not stable across releases, which would
/// orphan every workspace file on a toolchain upgrade.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

pub fn workspace_file(project: &Path) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    app_support().join("workspaces").join(format!("{:016x}.json", fnv1a(project.as_os_str().as_bytes())))
}
```

(`app_support()` is the existing helper that returns `~/Library/Application Support/arkos`; keep it.) Remove the `DefaultHasher` import.

- [ ] **Step 5: Run the tests**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test -p arkos workspace paths 2>&1 | tail -6`
Expected: all pass. `main.rs` still calls the old `workspace::load`/`save` signatures and will not compile until Task 5 replaces it; to keep the crate building in between, update the two call sites in `main.rs` minimally: `let saved = workspace::load(&project).map(|ws| ws.tree.pane(ws.tree.focused).and_then(|p| p.tabs.first().cloned())).flatten().and_then(|t| match t { Tab::Terminal { session } => Some(session), _ => None });` and `workspace::save(&project, &Workspace { version: FORMAT_VERSION, project: project.clone(), tree: Tree::new(Tab::Terminal { session: sid }) })`. Task 5 deletes both.

- [ ] **Step 6: Commit**

```bash
git add crates/arkos/src/workspace.rs crates/arkos/src/paths.rs crates/arkos/src/main.rs
git commit -m "arkos: persist the layout tree, name workspace files stably, debounce saves"
```

---

### Task 4: The session manager (pure parts tested)

**Files:**
- Create: `crates/arkos/src/sessions.rs`
- Modify: `crates/arkos/src/main.rs` (`mod sessions;`; move `reattachable` and `same_path` out, with their tests)
- Test: inline

**Interfaces:**
- Consumes: `client::Client`, `terminal::{Terminal, TerminalCore}`, `tree::{PaneId, Tab, Tree}`, `protocol::*`.
- Produces (used by Tasks 5 to 9):
  - `sessions::Transport` trait: `fn send(&self, request: Request) -> u32`, implemented for `Client`, so the manager is testable with a recorder.
  - `sessions::SCROLLBACK = 20_000`.
  - `sessions::Pending { List, Create { pane, idx, note }, Attach { session, pane, idx, note }, Kill { session }, Other }`.
  - `sessions::Outcome { None, Sessions(Vec<SessionInfo>), Created { pane, idx, session, note }, Attached { session, pane, idx, replay: Vec<u8>, alive: bool, note: bool }, Killed { session }, Error(String) }`.
  - `sessions::Reconcile { Attach { pane, idx, session }, Create { pane, idx } }` and `reconcile(tree: &Tree, list: &[SessionInfo], project: &Path) -> Vec<Reconcile>` (pure).
  - `sessions::group_by_session(events: &[Event]) -> Vec<(SessionId, Vec<Event>)>` (pure, order preserved).
  - `sessions::SessionManager { transport: Arc<dyn Transport>, client: Option<Arc<Client>>, project: PathBuf, terminals: HashMap<SessionId, Entity<Terminal>>, busy: HashMap<(PaneId, usize), &'static str>, notes: HashSet<SessionId>, .. }` with `new(transport, client, project)`, `list()`, `create(pane, idx, note)`, `attach(session, pane, idx, note)`, `kill(session)`, `detach(session)`, `handle_response(id, Response) -> Outcome`, `route_events(&mut self, events: &[Event], cx: &mut App)`, `begin_restore(session)`, `finish_restore(session, Entity<Terminal>, cx)`, `backlog_len(session) -> usize`, `ready(pane, idx)`, `pending_count() -> usize`, `has_pending_for(pane, idx) -> bool`. Events for a session whose replay is still being parsed are kept in a backlog and applied to its terminal the moment it exists, so concurrent restores lose nothing.
  - `sessions::reattachable(saved: SessionId, list, project) -> bool` and `same_path(a, b) -> bool` (moved from `main.rs`).

- [ ] **Step 1: Write the failing tests**

Create `crates/arkos/src/sessions.rs` with only the test module (the implementation is step 3):

```rust
//! Owns every terminal entity and the conversation with the daemon: which request asked for
//! what, which session sits in which tab, and the launch-time reconciliation of the saved layout
//! against the sessions the daemon still runs (spec 4: a missing session reopens as a fresh
//! shell in the same cwd with a note in the tab).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Side;
    use protocol::ClaudeState;
    use std::sync::Mutex;

    struct Recorder {
        sent: Mutex<Vec<Request>>,
        next: Mutex<u32>,
    }

    impl Recorder {
        fn new() -> Arc<Recorder> {
            Arc::new(Recorder { sent: Mutex::new(vec![]), next: Mutex::new(1) })
        }
        fn requests(&self) -> Vec<Request> {
            self.sent.lock().unwrap().clone()
        }
    }

    impl Transport for Recorder {
        fn send(&self, request: Request) -> u32 {
            self.sent.lock().unwrap().push(request);
            let mut n = self.next.lock().unwrap();
            let id = *n;
            *n += 1;
            id
        }
    }

    fn info(id: u64, cwd: &Path, alive: bool) -> SessionInfo {
        SessionInfo { id: SessionId(id), title: String::new(), cwd: cwd.to_path_buf(), pid: None, alive, last_activity_unix_ms: 0, state: ClaudeState::Idle }
    }

    fn term(n: u64) -> Tab {
        Tab::Terminal { session: SessionId(n) }
    }

    #[test]
    fn reconcile_replaces_dead_and_missing_sessions_in_place() {
        let here = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        // A 2x2 layout: 1 alive here, 2 dead, 3 gone from the daemon, 4 alive in another project.
        let mut t = Tree::new(term(1));
        let a = t.focused;
        let b = t.split(a, Side::Right, term(2)).unwrap();
        let c = t.split(a, Side::Bottom, term(3)).unwrap();
        let d = t.split(b, Side::Bottom, term(4)).unwrap();
        let list = vec![info(1, here.path(), true), info(2, here.path(), false), info(4, elsewhere.path(), true)];
        let plan = reconcile(&t, &list, here.path());
        assert_eq!(
            plan,
            vec![
                Reconcile::Attach { pane: a, idx: 0, session: SessionId(1) },
                Reconcile::Create { pane: c, idx: 0 },
                Reconcile::Create { pane: b, idx: 0 },
                Reconcile::Create { pane: d, idx: 0 },
            ]
        );
        assert_eq!(t.panes(), vec![a, c, b, d], "the layout itself is untouched");
    }

    #[test]
    fn reconcile_skips_viewer_tabs() {
        let here = tempfile::tempdir().unwrap();
        let mut t = Tree::new(term(1));
        let a = t.focused;
        t.add_tab(a, Tab::Viewer { path: PathBuf::from("/x") });
        let plan = reconcile(&t, &[info(1, here.path(), true)], here.path());
        assert_eq!(plan, vec![Reconcile::Attach { pane: a, idx: 0, session: SessionId(1) }]);
    }

    #[test]
    fn reattachable_requires_alive_and_same_project() {
        let project = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let here = project.path();
        let list = vec![info(7, here, true), info(8, here, false), info(9, other.path(), true)];
        assert!(reattachable(SessionId(7), &list, here));
        assert!(!reattachable(SessionId(8), &list, here));
        assert!(!reattachable(SessionId(9), &list, here));
        assert!(!reattachable(SessionId(10), &list, here));
        let sub = here.join("sub");
        std::fs::create_dir(&sub).unwrap();
        assert!(reattachable(SessionId(7), &list, &sub.join("..")), "paths compare canonically");
    }

    #[test]
    fn responses_are_matched_to_what_was_asked() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r.clone(), None, PathBuf::from("/p"));
        m.list();
        m.create(5, 0, true);
        m.attach(SessionId(3), 6, 1, false);
        m.kill(SessionId(9));
        let reqs = r.requests();
        assert!(matches!(reqs[0], Request::ListSessions));
        assert!(matches!(&reqs[1], Request::CreateSession { cwd, rows: 24, cols: 80, .. } if cwd == Path::new("/p")));
        assert!(matches!(reqs[2], Request::Attach { session: SessionId(3) }));
        assert!(matches!(reqs[3], Request::Kill { session: SessionId(9) }));
        assert_eq!(m.pending_count(), 4);
        assert!(m.has_pending_for(5, 0));
        assert!(m.has_pending_for(6, 1));
        assert!(!m.has_pending_for(5, 1));

        assert!(matches!(m.handle_response(1, Response::Sessions(vec![])), Outcome::Sessions(l) if l.is_empty()));
        assert_eq!(
            m.handle_response(2, Response::SessionCreated { id: SessionId(11) }),
            Outcome::Created { pane: 5, idx: 0, session: SessionId(11), note: true }
        );
        assert_eq!(
            m.handle_response(3, Response::Attached { replay: vec![1, 2], alive: true }),
            Outcome::Attached { session: SessionId(3), pane: 6, idx: 1, replay: vec![1, 2], alive: true, note: false }
        );
        assert_eq!(m.handle_response(4, Response::Ok), Outcome::Killed { session: SessionId(9) });
        assert_eq!(m.pending_count(), 0);
        assert!(!m.has_pending_for(5, 0));
        // An error for an unknown id still surfaces; a stray Ok does not.
        assert_eq!(m.handle_response(77, Response::Error("x".into())), Outcome::Error("x".into()));
        assert_eq!(m.handle_response(78, Response::Ok), Outcome::None);
    }

    #[test]
    fn a_failed_create_or_attach_clears_the_busy_mark() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r, None, PathBuf::from("/p"));
        m.create(1, 0, false);
        assert!(m.has_pending_for(1, 0));
        assert_eq!(m.handle_response(1, Response::Error("spawn failed".into())), Outcome::Error("spawn failed".into()));
        assert!(!m.has_pending_for(1, 0));
    }

    #[test]
    fn events_group_by_session_in_order() {
        let ev = |s: u64, b: &str| Event::Output { session: SessionId(s), bytes: b.as_bytes().to_vec() };
        let events = vec![ev(1, "a"), ev(2, "b"), ev(1, "c"), Event::Exited { session: SessionId(2), code: Some(0) }, ev(1, "d")];
        let grouped = group_by_session(&events);
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[0].0, SessionId(1));
        assert_eq!(grouped[0].1, vec![ev(1, "a"), ev(1, "c"), ev(1, "d")]);
        assert_eq!(grouped[1].0, SessionId(2));
        assert_eq!(grouped[1].1, vec![ev(2, "b"), Event::Exited { session: SessionId(2), code: Some(0) }]);
    }

    #[test]
    fn events_during_a_restore_are_kept_until_the_terminal_exists() {
        let r = Recorder::new();
        let mut m = SessionManager::new(r, None, PathBuf::from("/p"));
        m.begin_restore(SessionId(4));
        let ev = Event::Output { session: SessionId(4), bytes: b"late".to_vec() };
        // No gpui App is needed to test the bookkeeping: `stash_if_restoring` is the pure half
        // of `route_events`.
        assert!(m.stash_if_restoring(&ev));
        assert!(!m.stash_if_restoring(&Event::Output { session: SessionId(5), bytes: vec![] }));
        assert_eq!(m.backlog_len(SessionId(4)), 1);
        assert_eq!(m.take_backlog(SessionId(4)), vec![ev]);
        assert_eq!(m.backlog_len(SessionId(4)), 0);
        assert!(!m.is_restoring(SessionId(4)), "taking the backlog ends the restore");
    }

    #[test]
    fn exit_in_a_background_tab_does_not_change_focus() {
        // Routing events never touches the tree: focus is the window's decision, and a session
        // ending in another tab must not steal it (Review Focus 4).
        let mut t = Tree::new(term(1));
        let a = t.focused;
        t.split(a, Side::Right, term(2));
        t.activate(a, 0);
        let before = t.clone();
        let grouped = group_by_session(&[Event::Exited { session: SessionId(2), code: Some(1) }]);
        assert_eq!(grouped[0].0, SessionId(2));
        assert_eq!(t, before);
        assert_eq!(t.focused, a);
    }
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test -p arkos sessions 2>&1 | grep -E '^error' | head -5`
Expected: `cannot find type SessionManager`, `cannot find function reconcile`.

- [ ] **Step 3: Implement the manager**

Insert above the test module in `crates/arkos/src/sessions.rs`:

```rust
use crate::client::Client;
use crate::terminal::Terminal;
use crate::tree::{PaneId, Tab, Tree};
use gpui::{App, Entity};
use protocol::{Event, Request, Response, SessionId, SessionInfo};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const SCROLLBACK: usize = 20_000;

/// Sends requests to the daemon. `Client` is the real one; tests record.
pub trait Transport: Send + Sync {
    fn send(&self, request: Request) -> u32;
}

impl Transport for Client {
    fn send(&self, request: Request) -> u32 {
        Client::send(self, request)
    }
}

/// What a request id is waiting for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    List,
    /// A fresh shell for the tab at (pane, idx). `note` marks the tab "restarted" (spec 4).
    Create { pane: PaneId, idx: usize, note: bool },
    Attach { session: SessionId, pane: PaneId, idx: usize, note: bool },
    Kill { session: SessionId },
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    None,
    Sessions(Vec<SessionInfo>),
    Created { pane: PaneId, idx: usize, session: SessionId, note: bool },
    Attached { session: SessionId, pane: PaneId, idx: usize, replay: Vec<u8>, alive: bool, note: bool },
    Killed { session: SessionId },
    Error(String),
}

/// One launch step for one saved terminal tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reconcile {
    Attach { pane: PaneId, idx: usize, session: SessionId },
    Create { pane: PaneId, idx: usize },
}

/// Whether a saved session can be reattached: the daemon still runs it, it is alive, and it was
/// started in this project (an id from an earlier daemon could name another project's shell).
pub fn reattachable(saved: SessionId, list: &[SessionInfo], project: &Path) -> bool {
    match list.iter().find(|i| i.id == saved) {
        None => {
            eprintln!("arkos: saved session {} is gone, starting a new one", saved.0);
            false
        }
        Some(info) if !info.alive => {
            eprintln!("arkos: saved session {} has ended, starting a new one", saved.0);
            false
        }
        Some(info) if !same_path(&info.cwd, project) => {
            eprintln!("arkos: saved session {} belongs to {}, starting a new one", saved.0, info.cwd.display());
            false
        }
        Some(_) => true,
    }
}

/// Compares canonical paths, so symlinks and `..` do not count as a difference.
pub fn same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// For every terminal tab in reading order: attach when the session is reattachable, else a
/// fresh shell in that same tab. Viewer tabs are left alone. The tree is not modified.
pub fn reconcile(tree: &Tree, list: &[SessionInfo], project: &Path) -> Vec<Reconcile> {
    tree.tabs()
        .into_iter()
        .filter_map(|(pane, idx, tab)| match tab {
            Tab::Terminal { session } if reattachable(*session, list, project) => Some(Reconcile::Attach { pane, idx, session: *session }),
            Tab::Terminal { .. } => Some(Reconcile::Create { pane, idx }),
            Tab::Viewer { .. } => None,
        })
        .collect()
}

/// Groups a batch by session, keeping each session's events in arrival order and sessions in
/// order of first appearance.
pub fn group_by_session(events: &[Event]) -> Vec<(SessionId, Vec<Event>)> {
    let mut out: Vec<(SessionId, Vec<Event>)> = Vec::new();
    for ev in events {
        let sid = match ev {
            Event::Output { session, .. } | Event::Exited { session, .. } | Event::State { session, .. } => *session,
        };
        match out.iter_mut().find(|(s, _)| *s == sid) {
            Some((_, list)) => list.push(ev.clone()),
            None => out.push((sid, vec![ev.clone()])),
        }
    }
    out
}

pub struct SessionManager {
    pub transport: Arc<dyn Transport>,
    /// The real client, for building `Terminal` entities. Tests pass `None`.
    pub client: Option<Arc<Client>>,
    pub project: PathBuf,
    pub terminals: HashMap<SessionId, Entity<Terminal>>,
    pending: HashMap<u32, Pending>,
    /// Tabs with a shell being started or a replay being restored, and the word to show.
    pub busy: HashMap<(PaneId, usize), &'static str>,
    /// Sessions whose tab carries the "restarted" note until the user types into them.
    pub notes: HashSet<SessionId>,
    /// Sessions whose replay is being parsed; their live events wait in `backlog`.
    restoring: HashSet<SessionId>,
    backlog: HashMap<SessionId, Vec<Event>>,
}

impl SessionManager {
    pub fn new(transport: Arc<dyn Transport>, client: Option<Arc<Client>>, project: PathBuf) -> Self {
        Self { transport, client, project, terminals: HashMap::new(), pending: HashMap::new(), busy: HashMap::new(), notes: HashSet::new(), restoring: HashSet::new(), backlog: HashMap::new() }
    }

    pub fn list(&mut self) {
        let id = self.transport.send(Request::ListSessions);
        self.pending.insert(id, Pending::List);
    }

    pub fn create(&mut self, pane: PaneId, idx: usize, note: bool) {
        let id = self.transport.send(Request::CreateSession {
            cwd: self.project.clone(),
            shell: None,
            env: vec![],
            nice: 0,
            rows: 24,
            cols: 80,
        });
        self.pending.insert(id, Pending::Create { pane, idx, note });
        self.busy.insert((pane, idx), "starting shell");
    }

    pub fn attach(&mut self, session: SessionId, pane: PaneId, idx: usize, note: bool) {
        let id = self.transport.send(Request::Attach { session });
        self.pending.insert(id, Pending::Attach { session, pane, idx, note });
        self.busy.insert((pane, idx), "restoring session");
    }

    pub fn kill(&mut self, session: SessionId) {
        let id = self.transport.send(Request::Kill { session });
        self.pending.insert(id, Pending::Kill { session });
    }

    /// Stops receiving a session's output and drops its terminal. The daemon keeps the session.
    pub fn detach(&mut self, session: SessionId) {
        self.transport.send(Request::Detach { session });
        self.terminals.remove(&session);
        self.notes.remove(&session);
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn has_pending_for(&self, pane: PaneId, idx: usize) -> bool {
        self.busy.contains_key(&(pane, idx))
    }

    /// Matches a response to its request. Errors always surface, even for ids nobody waits on.
    pub fn handle_response(&mut self, id: u32, response: Response) -> Outcome {
        let pending = self.pending.remove(&id);
        if let Response::Error(e) = &response {
            match pending {
                Some(Pending::Create { pane, idx, .. }) | Some(Pending::Attach { pane, idx, .. }) => {
                    self.busy.remove(&(pane, idx));
                }
                _ => {}
            }
            return Outcome::Error(e.clone());
        }
        match (pending, response) {
            (Some(Pending::List), Response::Sessions(list)) => Outcome::Sessions(list),
            (Some(Pending::Create { pane, idx, note }), Response::SessionCreated { id: session }) => {
                self.busy.remove(&(pane, idx));
                Outcome::Created { pane, idx, session, note }
            }
            (Some(Pending::Attach { session, pane, idx, note }), Response::Attached { replay, alive }) => {
                // Stays busy until the window finishes restoring; it clears the mark itself.
                Outcome::Attached { session, pane, idx, replay, alive, note }
            }
            (Some(Pending::Kill { session }), Response::Ok) => Outcome::Killed { session },
            _ => Outcome::None,
        }
    }

    /// Clears a tab's busy mark once its terminal is on screen.
    pub fn ready(&mut self, pane: PaneId, idx: usize) {
        self.busy.remove(&(pane, idx));
    }

    pub fn begin_restore(&mut self, session: SessionId) {
        self.restoring.insert(session);
    }

    pub fn is_restoring(&self, session: SessionId) -> bool {
        self.restoring.contains(&session)
    }

    /// Keeps an event for later when its session is mid-restore. Returns whether it was kept.
    pub fn stash_if_restoring(&mut self, ev: &Event) -> bool {
        let sid = match ev {
            Event::Output { session, .. } | Event::Exited { session, .. } | Event::State { session, .. } => *session,
        };
        if !self.restoring.contains(&sid) {
            return false;
        }
        self.backlog.entry(sid).or_default().push(ev.clone());
        true
    }

    pub fn backlog_len(&self, session: SessionId) -> usize {
        self.backlog.get(&session).map(|b| b.len()).unwrap_or(0)
    }

    /// Ends the restore and returns the events that arrived during it, in order.
    pub fn take_backlog(&mut self, session: SessionId) -> Vec<Event> {
        self.restoring.remove(&session);
        self.backlog.remove(&session).unwrap_or_default()
    }

    /// Installs a restored terminal and applies everything that arrived while it was parsing.
    pub fn finish_restore(&mut self, session: SessionId, terminal: Entity<Terminal>, cx: &mut App) {
        let backlog = self.take_backlog(session);
        if !backlog.is_empty() {
            terminal.update(cx, |t, cx| {
                t.handle_events(&backlog, cx);
            });
        }
        self.terminals.insert(session, terminal);
    }

    /// Applies a batch of daemon events: one grid update and at most one notify per terminal.
    /// Events for a session that is still restoring wait in its backlog.
    pub fn route_events(&mut self, events: &[Event], cx: &mut App) {
        let live: Vec<Event> = events.iter().filter(|ev| !self.stash_if_restoring(ev)).cloned().collect();
        for (session, batch) in group_by_session(&live) {
            if let Some(t) = self.terminals.get(&session) {
                t.update(cx, |t, cx| {
                    t.handle_events(&batch, cx);
                });
            }
        }
    }
}
```

Add `mod sessions;` to `main.rs`. Remove `reattachable`, `same_path` and their two tests (`reattaches_only_a_live_session_in_this_project`, `project_paths_compare_canonically`) from `main.rs`, and point `Launch` at `sessions::reattachable` for now: in `Launch::process` the call `reattachable(self.saved, &list, &self.project)` becomes `self.saved.filter(|s| sessions::reattachable(*s, &list, &self.project))`. Put `#![allow(dead_code)] // Used from Task 5 onward` at the top of `sessions.rs` until the window uses the manager.

- [ ] **Step 4: Run the tests**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test -p arkos 2>&1 | tail -4`
Expected: everything passes, including the moved tests, and `cargo build -p arkos` is warning-free.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/sessions.rs crates/arkos/src/main.rs
git commit -m "arkos: session manager with launch reconciliation and per-session event routing"
```

---

### Task 5: The multi-pane window: split view, pane view, per-tab restore, focus

**Files:**
- Create: `crates/arkos/src/split_view.rs`, `crates/arkos/src/pane_view.rs`
- Modify: `crates/arkos/src/main.rs` (Root rewrite), `crates/arkos/src/terminal_element.rs` (font size parameter), `crates/arkos/src/tree.rs` and `sessions.rs` (remove the `dead_code` allows)
- Test: the pure helpers in `split_view.rs` (`ratio_from_pointer`) and `pane_view.rs` (`tab_label`) inline; the rest is the manual checklist at the end

**Interfaces:**
- Consumes: `tree::*`, `sessions::*`, `workspace::{Workspace, FORMAT_VERSION, SharedSaveQueue, request_save, load}`, `terminal::{Terminal, TerminalCore}`, `terminal_element::{TerminalElement, grid_for}`, `client::Client`.
- Produces (used by Tasks 6 to 9):
  - `terminal_element::TerminalElement::new(terminal: Entity<Terminal>, font_size: f32)`, `grid_for(bounds, window, font_size)`, `DEFAULT_FONT_SIZE = 16.0`, `cell_height(font_size) -> f32`.
  - `main.rs`: `Root { tree: Tree, sessions: SessionManager, save_queue, project, status, grids, grid_waiters, pane_focus, resizing, font_size, content: Rect }` with methods `focus_pane(&mut self, PaneId, cx)`, `activate_tab(&mut self, PaneId, usize, cx)`, `close_tab(&mut self, PaneId, usize, cx)`, `new_terminal_tab(&mut self, PaneId, cx)`, `split_focused(&mut self, Side, cx)`, `save(&mut self, cx)`, `report_grid(&mut self, PaneId, (usize, usize))`, `handle_outcome(&mut self, Outcome, cx)`, `active_terminal(&self, cx) -> Option<Entity<Terminal>>`, `layout(&self) -> Layout`.
  - `split_view::render_tree(root: &mut Root, window: &mut Window, cx: &mut Context<Root>) -> Div`, `split_view::ratio_from_pointer(area: Rect, orientation, x, y) -> f32`, `HANDLE_HIT = 8.0`.
  - `pane_view::render_pane(root: &mut Root, pane: PaneId, rect: Rect, window, cx) -> Div`, `pane_view::tab_label(title: &str, alive: bool, exit_code: Option<i32>, note: bool, busy: Option<&str>) -> String`, `TAB_BAR_H = 28.0`.

- [ ] **Step 1: Write the failing pure tests**

Create `crates/arkos/src/split_view.rs` with its test module first:

```rust
//! Renders the layout tree: absolutely positioned panes and the resize handles between them
//! (spec 4: a 4 px handle with a wider hit area, live ratio while dragging, double-click resets).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Orientation;

    #[test]
    fn ratio_follows_the_pointer_inside_the_split() {
        let area = Rect { x: 100.0, y: 0.0, w: 404.0, h: 300.0 };
        assert!((ratio_from_pointer(area, Orientation::Horizontal, 300.0, 0.0) - 0.5).abs() < 1e-3);
        assert!((ratio_from_pointer(area, Orientation::Horizontal, 100.0, 0.0) - 0.0).abs() < 1e-3);
        // Past either edge clamps instead of inverting the split.
        assert_eq!(ratio_from_pointer(area, Orientation::Horizontal, 9999.0, 0.0), 1.0);
        let tall = Rect { x: 0.0, y: 50.0, w: 100.0, h: 204.0 };
        assert!((ratio_from_pointer(tall, Orientation::Vertical, 0.0, 100.0) - 0.25).abs() < 1e-3);
    }
}
```

Create `crates/arkos/src/pane_view.rs` with its test module first:

```rust
//! One pane: its tab bar and the active tab's content (a terminal, a busy notice while a shell
//! starts or a replay is parsed, or the empty state).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_labels_show_state_after_the_title() {
        assert_eq!(tab_label("zsh", true, None, false, None), "zsh");
        assert_eq!(tab_label("", true, None, false, None), "shell");
        assert_eq!(tab_label("zsh", false, Some(1), false, None), "zsh (ended, exit 1)");
        assert_eq!(tab_label("zsh", false, None, false, None), "zsh (ended)");
        assert_eq!(tab_label("zsh", true, None, true, None), "zsh (restarted)");
        assert_eq!(tab_label("", true, None, false, Some("starting shell")), "starting shell");
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test -p arkos split_view pane_view 2>&1 | grep -E '^error' | head -3`
Expected: the modules are not declared yet (`mod` lines missing) or the functions are not found.

- [ ] **Step 3: Font size becomes a parameter of the element**

In `crates/arkos/src/terminal_element.rs`:
- Rename `pub const FONT_SIZE: f32 = 16.0;` to `pub const DEFAULT_FONT_SIZE: f32 = 16.0;`.
- Add `pub fn cell_height(font_size: f32) -> f32 { font_size * LINE_HEIGHT }`.
- `TerminalElement { terminal: Entity<Terminal>, font_size: f32 }` with `pub fn new(terminal: Entity<Terminal>, font_size: f32) -> Self`.
- `fn measure_cell(window: &mut Window, font_size: f32) -> CellMetrics` uses `px(font_size)` for the layout and `cell_height(font_size)`.
- `pub fn grid_for(bounds: Bounds<Pixels>, window: &mut Window, font_size: f32) -> (usize, usize)`.
- In `prepaint` call `measure_cell(window, self.font_size)`; in `paint`, `shape_line(text, px(self.font_size), ...)`.

Any other reference to `FONT_SIZE` in the crate (the banner in `main.rs`) uses `DEFAULT_FONT_SIZE`.

- [ ] **Step 4: Implement split_view.rs**

Insert above the test module:

```rust
use crate::main_root::Root;
use crate::pane_view::render_pane;
use crate::tree::{Orientation, Rect, SplitId, HANDLE};
use gpui::{div, prelude::*, px, rgb, Context, Div, MouseButton, MouseDownEvent, Window};

/// The hit area around a 4 px handle.
pub const HANDLE_HIT: f32 = 8.0;
const HANDLE_COLOR: u32 = 0x1f2630;
const HANDLE_HOVER: u32 = 0x3a4556;

/// Where the pointer sits along the split, as a ratio in [0, 1].
pub fn ratio_from_pointer(area: Rect, orientation: Orientation, x: f32, y: f32) -> f32 {
    let (pos, len) = match orientation {
        Orientation::Horizontal => (x - area.x, area.w - HANDLE),
        Orientation::Vertical => (y - area.y, area.h - HANDLE),
    };
    if len <= 0.0 {
        return 0.5;
    }
    (pos / len).clamp(0.0, 1.0)
}

/// The panes and handles for the current tree, positioned absolutely inside `root.content`.
pub fn render_tree(root: &mut Root, window: &mut Window, cx: &mut Context<Root>) -> Div {
    let layout = root.layout();
    let mut container = div().relative().size_full();
    for (pane, rect) in &layout.panes {
        container = container.child(
            div()
                .absolute()
                .left(px(rect.x))
                .top(px(rect.y))
                .w(px(rect.w))
                .h(px(rect.h))
                .overflow_hidden()
                .child(render_pane(root, *pane, *rect, window, cx)),
        );
    }
    for (split, orientation, rect, area) in &layout.handles {
        container = container.child(render_handle(*split, *orientation, *rect, *area, cx));
    }
    container
}

fn render_handle(split: SplitId, orientation: Orientation, rect: Rect, area: Rect, cx: &mut Context<Root>) -> Div {
    let pad = (HANDLE_HIT - HANDLE) / 2.0;
    let (x, y, w, h) = match orientation {
        Orientation::Horizontal => (rect.x - pad, rect.y, HANDLE_HIT, rect.h),
        Orientation::Vertical => (rect.x, rect.y - pad, rect.w, HANDLE_HIT),
    };
    let stripe = match orientation {
        Orientation::Horizontal => div().absolute().left(px(pad)).top_0().w(px(HANDLE)).h_full(),
        Orientation::Vertical => div().absolute().top(px(pad)).left_0().h(px(HANDLE)).w_full(),
    };
    let handle = div()
        .id(("handle", split))
        .absolute()
        .left(px(x))
        .top(px(y))
        .w(px(w))
        .h(px(h))
        .group("handle")
        .child(stripe.bg(rgb(HANDLE_COLOR)).group_hover("handle", |s| s.bg(rgb(HANDLE_HOVER))))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |root, ev: &MouseDownEvent, _window, cx| {
                if ev.click_count == 2 {
                    root.tree.reset_ratio(split);
                    root.resizing = None;
                    root.save(cx);
                } else {
                    root.resizing = Some((split, orientation, area));
                }
                cx.notify();
            }),
        );
    match orientation {
        Orientation::Horizontal => handle.cursor_col_resize(),
        Orientation::Vertical => handle.cursor_row_resize(),
    }
}
```

`cursor_col_resize` and `cursor_row_resize` are generated on `Styled` by gpui's `cursor_style_methods!` macro; if the names differ in 0.2.2, grep `col_resize` in `~/.cargo/registry/src/*/gpui-0.2.2/src/` and use what it defines. `group`/`group_hover` are on `InteractiveElement` and `Styled`. `crate::main_root::Root` is the path the plan uses for the `Root` struct: keep `Root` in `main.rs` and expose it to the view modules with `pub(crate)` fields; since `main.rs` is the crate root, the views refer to it as `crate::Root`. Replace `crate::main_root::Root` with `crate::Root` everywhere in this task.

- [ ] **Step 5: Implement pane_view.rs**

Insert above the test module:

```rust
use crate::terminal_element::{grid_for, TerminalElement};
use crate::tree::{PaneId, Rect, Tab};
use crate::Root;
use gpui::{canvas, div, prelude::*, px, rgb, ClickEvent, Context, Div, FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent, ScrollDelta, ScrollWheelEvent, Window};

pub const TAB_BAR_H: f32 = 28.0;
const BG: u32 = 0x11161f;
const FG: u32 = 0xd6dae0;
const BAR_BG: u32 = 0x0d1117;
const TAB_ACTIVE: u32 = 0x11161f;
const TAB_INACTIVE: u32 = 0x0d1117;
const DIM: u32 = 0x8a93a3;
const FOCUS_RING: u32 = 0x3a7bd5;

/// The text on a tab: the terminal's title (or "shell"), then its state.
pub fn tab_label(title: &str, alive: bool, exit_code: Option<i32>, note: bool, busy: Option<&str>) -> String {
    if let Some(b) = busy {
        return b.to_string();
    }
    let name = if title.is_empty() { "shell" } else { title };
    if !alive {
        return match exit_code {
            Some(c) => format!("{name} (ended, exit {c})"),
            None => format!("{name} (ended)"),
        };
    }
    if note {
        return format!("{name} (restarted)");
    }
    name.to_string()
}

pub fn render_pane(root: &mut Root, pane: PaneId, rect: Rect, window: &mut Window, cx: &mut Context<Root>) -> Div {
    let focused = root.tree.focused == pane;
    let Some(p) = root.tree.pane(pane).cloned() else { return div() };
    let number = root.tree.pane_index(pane).map(|i| i + 1).unwrap_or(0);
    let bar = render_tab_bar(root, pane, &p.tabs, p.active, number, cx);
    let body = match p.tabs.get(p.active) {
        None => render_no_tabs(root, pane, cx),
        Some(Tab::Terminal { session }) => match root.sessions.terminals.get(session).cloned() {
            Some(terminal) => render_terminal(root, pane, terminal, cx),
            None => render_busy(root, pane, p.active, cx),
        },
        Some(Tab::Viewer { .. }) => div().size_full().child("viewer tabs arrive in milestone 4"),
    };
    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(rgb(BG))
        .border_1()
        .border_color(rgb(if focused { FOCUS_RING } else { BAR_BG }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |root, _ev: &MouseDownEvent, _window, cx| {
                root.focus_pane(pane, cx);
            }),
        )
        .child(bar)
        .child(div().flex_1().min_h_0().child(body))
}

fn render_tab_bar(root: &mut Root, pane: PaneId, tabs: &[Tab], active: usize, number: usize, cx: &mut Context<Root>) -> Div {
    let mut bar = div()
        .h(px(TAB_BAR_H))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .bg(rgb(BAR_BG))
        .text_color(rgb(FG))
        .font_family("Menlo")
        .text_size(px(12.0))
        .child(div().px(px(8.0)).text_color(rgb(DIM)).child(format!("{number}")));
    for (idx, tab) in tabs.iter().enumerate() {
        let label = match tab {
            Tab::Terminal { session } => {
                let busy = root.sessions.busy.get(&(pane, idx)).copied();
                let note = root.sessions.notes.contains(session);
                match root.sessions.terminals.get(session) {
                    Some(t) => {
                        let t = t.read(cx);
                        tab_label(&t.title, t.alive, t.exit_code, note, busy)
                    }
                    None => tab_label("", true, None, note, busy.or(Some("starting shell"))),
                }
            }
            Tab::Viewer { path } => path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        };
        let is_active = idx == active;
        bar = bar.child(
            div()
                .id(("tab", pane, idx))
                .h_full()
                .px(px(10.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .bg(rgb(if is_active { TAB_ACTIVE } else { TAB_INACTIVE }))
                .text_color(rgb(if is_active { FG } else { DIM }))
                .cursor_pointer()
                .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| root.activate_tab(pane, idx, cx)))
                .child(label)
                .child(
                    div()
                        .id(("close", pane, idx))
                        .text_color(rgb(DIM))
                        .hover(|s| s.text_color(rgb(FG)))
                        .child("×")
                        .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| root.close_tab(pane, idx, cx))),
                ),
        );
    }
    bar.child(
        div()
            .id(("plus", pane))
            .px(px(10.0))
            .text_color(rgb(DIM))
            .hover(|s| s.text_color(rgb(FG)))
            .cursor_pointer()
            .child("+")
            .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| root.new_terminal_tab(pane, cx))),
    )
}

fn render_terminal(root: &mut Root, pane: PaneId, terminal: gpui::Entity<crate::terminal::Terminal>, cx: &mut Context<Root>) -> Div {
    let focus = terminal.read(cx).focus.clone();
    let font_size = root.font_size;
    let t_keys = terminal.clone();
    let t_wheel = terminal.clone();
    div()
        .size_full()
        .track_focus(&focus)
        .on_key_down(cx.listener(move |root, ev: &KeyDownEvent, _window, cx| {
            let ks = &ev.keystroke;
            if ks.modifiers.platform && ks.key == "v" {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    t_keys.update(cx, |t, _| t.paste(&text));
                }
                return;
            }
            if ks.modifiers.platform {
                // Cmd combinations are actions (Task 6) or nothing; never terminal bytes.
                return;
            }
            let session = t_keys.read(cx).session;
            root.sessions.notes.remove(&session);
            t_keys.update(cx, |t, cx| t.handle_key(ks, cx));
        }))
        .on_scroll_wheel(cx.listener(move |_root, ev: &ScrollWheelEvent, _window, cx| {
            t_wheel.update(cx, |t, cx| {
                let cell_h = t.cell.map(|c| c.height).unwrap_or(24.0);
                let lines = match ev.delta {
                    ScrollDelta::Lines(p) => p.y,
                    ScrollDelta::Pixels(p) => f32::from(p.y) / cell_h,
                };
                t.scroll_lines(lines, cx);
            });
        }))
        .child(TerminalElement::new(terminal, font_size))
}

/// A tab whose shell is starting or whose replay is being parsed: measure the grid the terminal
/// will get, so the replay can be parsed at that size (milestone 1's rule, now per pane).
fn render_busy(root: &mut Root, pane: PaneId, idx: usize, cx: &mut Context<Root>) -> Div {
    let text = root.sessions.busy.get(&(pane, idx)).copied().unwrap_or("starting shell");
    let weak = cx.weak_entity();
    let font_size = root.font_size;
    let measure = canvas(
        move |bounds, window, cx| {
            let grid = grid_for(bounds, window, font_size);
            let _ = weak.update(cx, |root, _| root.report_grid(pane, grid));
        },
        |_, _, _, _| {},
    )
    .size_full();
    let focus = pane_focus(root, pane, cx);
    div()
        .size_full()
        .relative()
        .track_focus(&focus)
        .child(measure)
        .child(div().absolute().top(px(8.0)).left(px(12.0)).text_color(rgb(DIM)).font_family("Menlo").text_size(px(font_size)).child(text))
}

/// Placeholder until Task 8 brings the friendly empty state.
fn render_no_tabs(root: &mut Root, pane: PaneId, cx: &mut Context<Root>) -> Div {
    let focus = pane_focus(root, pane, cx);
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .track_focus(&focus)
        .text_color(rgb(DIM))
        .font_family("Menlo")
        .text_size(px(14.0))
        .child("No tabs. Press Ctrl+Shift+` for a new terminal.")
}

/// A pane without a terminal still needs something to focus so Cmd+N and typing land somewhere.
fn pane_focus(root: &mut Root, pane: PaneId, cx: &mut Context<Root>) -> FocusHandle {
    root.pane_focus.entry(pane).or_insert_with(|| cx.focus_handle()).clone()
}
```

- [ ] **Step 6: Rewrite Root in main.rs**

Replace the `Root` struct, its `impl`, `impl Render for Root`, `banner_bar`, `success_clears`, `Launch`, `Restore`, `restore`, and the event loop inside `main` with the following. Keep `output_len`, `yield_now`, the `actions!(arkos, [Quit])`, the QoS block, the connect code, the heartbeat task, `on_window_closed`, the menus and the `--version` check as they are.

```rust
mod pane_view;
mod sessions;
mod split_view;
mod tree;

use futures::channel::oneshot;
use sessions::{reconcile, Outcome, Reconcile, SessionManager, SCROLLBACK};
use std::collections::HashMap;
use tree::{Orientation, PaneId, Rect, Side, SplitId, Tab, Tree};
use workspace::{request_save, SharedSaveQueue, Workspace, FORMAT_VERSION};

const BG: u32 = 0x11161f;
const FG: u32 = 0xd6dae0;
const BANNER_BG: u32 = 0x1a2130;
const BATCH_BUDGET: usize = 1 << 20;
const ERROR_PREFIX: &str = "arkosd error: ";

pub struct Root {
    pub tree: Tree,
    pub sessions: SessionManager,
    pub save_queue: SharedSaveQueue,
    pub project: PathBuf,
    /// Connection-level banner (errors, disconnects). Empty when all is well.
    pub status: String,
    /// The grid each pane last measured while it had no terminal to measure itself.
    pub grids: HashMap<PaneId, (usize, usize)>,
    pub grid_waiters: HashMap<PaneId, Vec<oneshot::Sender<(usize, usize)>>>,
    /// Focus handles for panes that show no terminal (busy or empty).
    pub pane_focus: HashMap<PaneId, FocusHandle>,
    /// A split handle being dragged: the split, its orientation, and the split's bounds.
    pub resizing: Option<(SplitId, Orientation, Rect)>,
    pub font_size: f32,
    /// The area the tree is laid out in, in window pixels, from the last render.
    pub content: Rect,
}

impl Root {
    fn new(client: Arc<client::Client>, project: PathBuf, saved: Option<Workspace>) -> Self {
        // A fresh workspace starts with one pane holding a placeholder session that the launch
        // replaces with a real shell (reconcile sees it is not in the daemon's list).
        let tree = saved.map(|ws| ws.tree).unwrap_or_else(|| Tree::new(Tab::Terminal { session: SessionId(0) }));
        Self {
            tree,
            sessions: SessionManager::new(client.clone(), Some(client), project.clone()),
            save_queue: SharedSaveQueue::default(),
            project,
            status: "connecting to arkosd".into(),
            grids: HashMap::new(),
            grid_waiters: HashMap::new(),
            pane_focus: HashMap::new(),
            resizing: None,
            font_size: terminal_element::DEFAULT_FONT_SIZE,
            content: Rect { x: 0.0, y: 0.0, w: 1200.0, h: 800.0 },
        }
    }

    pub fn layout(&self) -> tree::Layout {
        self.tree.layout(self.content)
    }

    pub fn save(&mut self, cx: &mut App) {
        let ws = Workspace { version: FORMAT_VERSION, project: self.project.clone(), tree: self.tree.clone() };
        request_save(&self.save_queue, ws, cx);
    }

    pub fn report_grid(&mut self, pane: PaneId, grid: (usize, usize)) {
        self.grids.insert(pane, grid);
        for tx in self.grid_waiters.remove(&pane).unwrap_or_default() {
            let _ = tx.send(grid);
        }
    }

    /// The grid a terminal in `pane` would get: the pane's current terminal's, else the last
    /// measurement of the pane's placeholder.
    fn pane_grid(&self, pane: PaneId, cx: &App) -> Option<(usize, usize)> {
        let p = self.tree.pane(pane)?;
        if let Some(Tab::Terminal { session }) = p.tabs.get(p.active) {
            if let Some(t) = self.sessions.terminals.get(session) {
                let core = &t.read(cx).core;
                return Some((core.columns(), core.screen_lines()));
            }
        }
        self.grids.get(&pane).copied()
    }

    pub fn active_terminal(&self, cx: &App) -> Option<Entity<Terminal>> {
        let p = self.tree.pane(self.tree.focused)?;
        match p.tabs.get(p.active)? {
            Tab::Terminal { session } => self.sessions.terminals.get(session).cloned(),
            Tab::Viewer { .. } => None,
        }
    }

    pub fn focus_pane(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        if self.tree.pane(pane).is_none() {
            return;
        }
        self.tree.focused = pane;
        cx.notify();
    }

    pub fn activate_tab(&mut self, pane: PaneId, idx: usize, cx: &mut Context<Self>) {
        self.tree.activate(pane, idx);
        self.save(cx);
        cx.notify();
    }

    /// Closes a tab. A terminal's session is killed and detached; the pane stays (spec 4).
    pub fn close_tab(&mut self, pane: PaneId, idx: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.tree.close_tab(pane, idx) else { return };
        if let Tab::Terminal { session } = tab {
            if session != SessionId(0) {
                self.sessions.kill(session);
                self.sessions.detach(session);
            }
            self.sessions.ready(pane, idx);
        }
        self.save(cx);
        cx.notify();
    }

    pub fn new_terminal_tab(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        let idx = self.tree.add_tab(pane, Tab::Terminal { session: SessionId(0) });
        self.tree.focused = pane;
        self.sessions.create(pane, idx, false);
        self.save(cx);
        cx.notify();
    }

    /// Splits the focused pane and starts a fresh shell in the new one.
    pub fn split_focused(&mut self, side: Side, cx: &mut Context<Self>) {
        let pane = self.tree.focused;
        if let Some(new_pane) = self.tree.split(pane, side, Tab::Terminal { session: SessionId(0) }) {
            self.sessions.create(new_pane, 0, false);
            self.save(cx);
            cx.notify();
        }
    }

    /// Applies what the daemon answered.
    pub fn handle_outcome(&mut self, outcome: Outcome, saved_layout: bool, cx: &mut Context<Self>) {
        match outcome {
            Outcome::None => {}
            Outcome::Sessions(list) => {
                for step in reconcile(&self.tree, &list, &self.project) {
                    match step {
                        Reconcile::Attach { pane, idx, session } => self.sessions.attach(session, pane, idx, false),
                        Reconcile::Create { pane, idx } => self.sessions.create(pane, idx, saved_layout),
                    }
                }
                cx.notify();
            }
            Outcome::Created { pane, idx, session, note } => {
                self.tree.replace_tab(pane, idx, Tab::Terminal { session });
                if note {
                    self.sessions.notes.insert(session);
                }
                self.save(cx);
                self.sessions.attach(session, pane, idx, note);
                cx.notify();
            }
            Outcome::Attached { session, pane, idx, replay, alive, note: _ } => {
                self.sessions.begin_restore(session);
                let root = cx.entity();
                cx.spawn(async move |cx| {
                    if let Err(e) = restore(&root, session, pane, idx, replay, alive, cx).await {
                        eprintln!("arkos: restoring session {} failed: {e:#}", session.0);
                    }
                })
                .detach();
            }
            Outcome::Killed { .. } => {}
            Outcome::Error(e) => {
                self.status = format!("{ERROR_PREFIX}{e}");
                eprintln!("arkos: {}", self.status);
                cx.notify();
            }
        }
    }
}

/// Parses a replay off the main thread at the pane's grid, then installs the terminal.
async fn restore(root: &Entity<Root>, session: SessionId, pane: PaneId, idx: usize, replay: Vec<u8>, alive: bool, cx: &mut AsyncApp) -> anyhow::Result<()> {
    let known = root.update(cx, |root, cx| match root.pane_grid(pane, cx) {
        Some(grid) => Ok(grid),
        None => {
            let (tx, rx) = oneshot::channel();
            root.grid_waiters.entry(pane).or_default().push(tx);
            cx.notify();
            Err(rx)
        }
    })?;
    let (cols, rows) = match known {
        Ok(grid) => grid,
        Err(rx) => rx.await?,
    };
    let (core, title) = cx
        .background_spawn(async move {
            let mut core = TerminalCore::new(cols, rows, SCROLLBACK);
            // The previous UI already answered any queries in the replay; keep only the title.
            let title = core.apply_output(&replay).title;
            (core, title)
        })
        .await;
    root.update(cx, |root, cx| {
        let client = root.sessions.transport.clone();
        client.send(Request::Resize { session, rows: rows as u16, cols: cols as u16 });
        let terminal = cx.new(|cx| {
            let mut t = Terminal::new(core, session, root.sessions.client.clone().expect("client"), cx);
            t.alive = alive;
            t.title = title.unwrap_or_default();
            t
        });
        root.sessions.finish_restore(session, terminal, cx);
        root.sessions.ready(pane, idx);
        cx.notify();
    })
}
```

`Terminal::new` takes an `Arc<Client>`; the manager carries it as `client: Option<Arc<Client>>` (Task 4), so `restore` uses `root.sessions.client.clone().expect("client")`, and `Root::new` builds the manager with `SessionManager::new(client.clone(), Some(client), project.clone())`. Extend `main.rs`'s gpui `use` list with `FocusHandle, MouseButton, MouseMoveEvent, MouseUpEvent` and the `Rect` import from `tree`.

The `Render` impl:

```rust
impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = window.viewport_size();
        let banner_h = if self.status.is_empty() { 0.0 } else { 28.0 };
        self.content = Rect { x: 0.0, y: banner_h, w: f32::from(size.width), h: f32::from(size.height) - banner_h };
        // Focus follows the tree: the focused pane's terminal, or the pane's own handle.
        let wanted = match self.active_terminal(cx) {
            Some(t) => Some(t.read(cx).focus.clone()),
            None => self.pane_focus.get(&self.tree.focused).cloned(),
        };
        if let Some(f) = wanted {
            if !f.is_focused(window) {
                window.focus(&f);
            }
        }
        let body = split_view::render_tree(self, window, cx);
        div()
            .size_full()
            .bg(rgb(BG))
            .flex()
            .flex_col()
            .on_mouse_move(cx.listener(|root, ev: &MouseMoveEvent, _window, cx| {
                if let Some((split, orientation, area)) = root.resizing {
                    let ratio = split_view::ratio_from_pointer(area, orientation, f32::from(ev.position.x), f32::from(ev.position.y) - root.content.y);
                    root.tree.set_ratio(split, ratio);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|root, _ev: &MouseUpEvent, _window, cx| {
                    if root.resizing.take().is_some() {
                        root.save(cx);
                    }
                }),
            )
            .children((!self.status.is_empty()).then(|| banner_bar(self.status.clone())))
            .child(div().flex_1().min_h_0().child(body))
    }
}

fn banner_bar(text: String) -> Div {
    div()
        .w_full()
        .h(px(28.0))
        .flex_none()
        .px(px(8.0))
        .flex()
        .items_center()
        .bg(rgb(BANNER_BG))
        .text_color(rgb(FG))
        .font_family(terminal_element::FONT_FAMILY)
        .text_size(px(13.0))
        .child(text)
}
```

The handle rects come from `self.content`, which is offset by the banner, while mouse positions are window coordinates: `render_tree` positions panes inside a container that starts at `content.y`, so the subtraction above keeps them consistent. `window.viewport_size()` exists on `Window` in gpui 0.2.2; if the name differs, grep `fn viewport_size`.

The event loop in `main`, replacing the `Launch`-based one:

```rust
        let saved_layout = saved.is_some();
        root.update(cx, |root, _| root.sessions.list());
        let client_for_task = client.clone();
        cx.spawn(async move |cx| {
            let mut closed = false;
            while !closed {
                let Some(first) = events.next().await else { break };
                let mut bytes = output_len(&first);
                let mut queue = VecDeque::from([first]);
                while bytes <= BATCH_BUDGET {
                    match events.try_recv() {
                        Ok(msg) => {
                            bytes += output_len(&msg);
                            queue.push_back(msg);
                        }
                        Err(TryRecvError::Closed) => {
                            closed = true;
                            break;
                        }
                        Err(TryRecvError::Empty) => break,
                    }
                }
                let Ok(()) = root.update(cx, |root, cx| {
                    let mut pending: Vec<Event> = Vec::new();
                    for msg in queue.drain(..) {
                        match msg {
                            ServerMessage::Event(ev) => pending.push(ev),
                            ServerMessage::Response { id, response } => {
                                if !pending.is_empty() {
                                    root.sessions.route_events(&pending, cx);
                                    pending.clear();
                                }
                                let is_error = matches!(response, Response::Error(_));
                                let outcome = root.sessions.handle_response(id, response);
                                root.handle_outcome(outcome, saved_layout, cx);
                                if !is_error && root.status.starts_with(ERROR_PREFIX) {
                                    root.status.clear();
                                    cx.notify();
                                }
                            }
                        }
                    }
                    if !pending.is_empty() {
                        root.sessions.route_events(&pending, cx);
                    }
                }) else { return };
                client_for_task.consumed_output(bytes);
                yield_now().await;
            }
            let reason = client_for_task.disconnect_reason().unwrap_or_else(|| "daemon connection ended".into());
            eprintln!("arkos: {reason}");
            let _ = root.update(cx, |root, cx| {
                root.status = if root.status.starts_with(ERROR_PREFIX) { format!("{}; {reason}", root.status) } else { reason };
                cx.notify();
            });
        })
        .detach();
```

and before `Application::new().run`: `let saved = workspace::load(&project);` with `Root::new(client_for_view, project.clone(), saved.clone())` in the window constructor. Remove the `mod` lines' `dead_code` allows in `tree.rs` and `sessions.rs`.

- [ ] **Step 7: Build, run the pure tests, then the manual check**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo build -p arkos 2>&1 | grep -E '^(warning|error)' | head; cargo test -p arkos 2>&1 | tail -3`
Expected: no warnings from the crate, all tests pass.

Manual (record each in the report; what cannot be driven without Accessibility permission is marked "needs human eyes"):
1. `pkill -x arkosd; cargo run -p arkos 2>.superpowers/run.log &` from the repo root: one pane, tab bar with "1", the tab titled by the shell, a "+" button.
2. Click "+": a second tab appears with "starting shell" then the shell's title; typing goes to it.
3. Close a tab with its "×": the pane stays; close the last one: the "No tabs" placeholder shows.
4. Through a second connection or by editing the saved workspace JSON into the screenshot layout (a vertical split whose right child is a horizontal split), relaunch: three panes numbered 1, 2, 3 left to right then top to bottom, each restoring its session or starting a fresh one with "(restarted)".
5. Drag a handle: the ratio follows the pointer; neither side goes below 160 px wide or 80 px tall; double-click resets to half.
6. `pkill -9 arkos`, relaunch: the same layout and sessions return.

- [ ] **Step 8: Commit**

```bash
git add crates/arkos/src
git commit -m "arkos: split view, pane view with tab bars, per-tab restore, resize handles"
```

---

### Task 6: The keyboard map

**Files:**
- Create: `crates/arkos/src/keymap.rs`
- Modify: `crates/arkos/src/main.rs` (actions on the root, `Root::run`), `crates/arkos/src/pane_view.rs` (key contexts), `crates/arkos/src/terminal.rs` (`clear_scrollback`)
- Test: inline in `keymap.rs` and `terminal.rs`

**Interfaces:**
- Consumes: `Root` methods from Task 5, `tree::*`.
- Produces:
  - `keymap::Command` enum: `FocusPane(usize)`, `FocusDir(Side)`, `Split(Side)`, `ToggleMaximize`, `MoveTabToPane(isize)`, `CycleTab(isize)`, `MruTab`, `CloseTab`, `ReopenClosedTab`, `CloseAllInPane`, `MoveTabWithin(isize)`, `NewTerminal`, `ClearScrollback`, `FontDelta(f32)`.
  - `keymap::bindings() -> Vec<KeyBinding>` (the spec's table) and `keymap::SPEC_ROWS: &[(&str, &str)]` (binding string, command name) for the table test.
  - `keymap::attach(div: Div, cx: &mut Context<Root>) -> Div` registers every `on_action` on the root div, routing to `Root::run(Command, window, cx)`.
  - `keymap::tab_cycle(active: usize, len: usize, delta: isize) -> usize` (pure).
  - `terminal::TerminalCore::clear_scrollback(&mut self)`.
  - Not bound here, by design: Cmd+F (find, milestone 2b), Cmd+, and Cmd+Shift+P (config and palette, 2b), Cmd+B, Cmd+Shift+E, Ctrl+Shift+G (sidebar, milestone 4). The config file's `keybindings` section (2b) will feed `bindings()`; here the table is a constant.

- [ ] **Step 1: Write the failing tests**

Create `crates/arkos/src/keymap.rs` with its test module:

```rust
//! The keyboard map (spec 4): VS Code's macOS defaults for the editor area, applied to panes
//! and terminal tabs. Every binding is global except ClearScrollback, which needs a terminal.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_spec_row_is_bound_once() {
        let b = bindings();
        for (keys, name) in SPEC_ROWS {
            let hits: Vec<_> = b.iter().filter(|kb| kb.keystrokes().iter().map(|k| k.unparse()).collect::<Vec<_>>().join(" ") == *keys).collect();
            assert_eq!(hits.len(), 1, "{keys} ({name}) bound {} times", hits.len());
        }
        assert_eq!(b.len(), SPEC_ROWS.len(), "a binding exists that the spec table does not list");
    }

    #[test]
    fn tab_cycling_wraps() {
        assert_eq!(tab_cycle(0, 3, 1), 1);
        assert_eq!(tab_cycle(2, 3, 1), 0);
        assert_eq!(tab_cycle(0, 3, -1), 2);
        assert_eq!(tab_cycle(0, 1, 1), 0);
        assert_eq!(tab_cycle(5, 0, 1), 0);
    }
}
```

`Keystroke::unparse()` renders a keystroke back to gpui's `cmd-k` form and `KeyBinding::keystrokes()` returns the parsed sequence; both exist in gpui 0.2.2 (`keymap/binding.rs`, `platform/keystroke.rs`). If `unparse` is missing, compare against `format!("{k}")` (Keystroke implements Display).

Append to the tests in `crates/arkos/src/terminal.rs`:

```rust
    #[test]
    fn clear_scrollback_drops_history_and_keeps_the_screen() {
        let mut t = TerminalCore::new(20, 3, 100);
        for i in 0..10 {
            t.apply_output(format!("line{i}\r\n").as_bytes());
        }
        assert!(t.history_size() > 0);
        t.scroll.jump_to(3.0);
        t.set_display_offset(3);
        t.clear_scrollback();
        assert_eq!(t.history_size(), 0);
        assert_eq!(t.display_offset(), 0);
        assert!(t.scroll.is_at_bottom());
        let rows = t.rows_for_paint();
        assert_eq!(text_of(&rows.visible[0]), "line8");
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test -p arkos keymap clear_scrollback 2>&1 | grep -E '^error' | head -3`
Expected: `bindings`, `SPEC_ROWS`, `tab_cycle`, `clear_scrollback` not found.

- [ ] **Step 3: Implement keymap.rs**

Insert above the test module:

```rust
use crate::tree::Side;
use crate::Root;
use gpui::{actions, Context, Div, KeyBinding, Window};
use gpui::prelude::*;

actions!(
    arkos,
    [
        FocusPane1, FocusPane2, FocusPane3, FocusPane4, FocusPane5, FocusPane6, FocusPane7, FocusPane8, FocusPane9,
        FocusLeft, FocusRight, FocusUp, FocusDown,
        SplitRight, SplitDown, ToggleMaximize,
        MoveTabToPrevPane, MoveTabToNextPane,
        NextTab, PrevTab, NextTabAlt, PrevTabAlt, MruTab,
        CloseTab, ReopenClosedTab, CloseAllInPane,
        MoveTabLeft, MoveTabRight,
        NewTerminal, ClearScrollback, FontBigger, FontSmaller
    ]
);

/// What a key does, in the window's terms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    FocusPane(usize),
    FocusDir(Side),
    Split(Side),
    ToggleMaximize,
    /// Move the active tab to the previous (-1) or next (+1) pane in reading order.
    MoveTabToPane(isize),
    CycleTab(isize),
    MruTab,
    CloseTab,
    ReopenClosedTab,
    CloseAllInPane,
    MoveTabWithin(isize),
    NewTerminal,
    ClearScrollback,
    FontDelta(f32),
}

/// The spec's table (section 4), gpui keystroke syntax. Chords are space-separated.
pub const SPEC_ROWS: &[(&str, &str)] = &[
    ("cmd-1", "focus pane 1"), ("cmd-2", "focus pane 2"), ("cmd-3", "focus pane 3"), ("cmd-4", "focus pane 4"),
    ("cmd-5", "focus pane 5"), ("cmd-6", "focus pane 6"), ("cmd-7", "focus pane 7"), ("cmd-8", "focus pane 8"),
    ("cmd-9", "focus pane 9"),
    ("cmd-k cmd-left", "focus left"), ("cmd-k cmd-right", "focus right"), ("cmd-k cmd-up", "focus up"), ("cmd-k cmd-down", "focus down"),
    ("cmd-\\", "split right"), ("cmd-k cmd-\\", "split down"),
    ("cmd-k cmd-m", "maximize pane"),
    ("ctrl-cmd-left", "move tab to previous pane"), ("ctrl-cmd-right", "move tab to next pane"),
    ("cmd-alt-right", "next tab"), ("cmd-alt-left", "previous tab"), ("cmd-shift-]", "next tab"), ("cmd-shift-[", "previous tab"),
    ("ctrl-tab", "most recently used tab"),
    ("cmd-w", "close tab"), ("cmd-shift-t", "reopen closed tab"), ("cmd-k w", "close all in pane"),
    ("cmd-k cmd-shift-left", "move tab left"), ("cmd-k cmd-shift-right", "move tab right"),
    ("ctrl-shift-`", "new terminal tab"),
    ("cmd-k", "clear scrollback"),
    ("cmd-=", "font bigger"), ("cmd--", "font smaller"),
];

pub fn bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("cmd-1", FocusPane1, None),
        KeyBinding::new("cmd-2", FocusPane2, None),
        KeyBinding::new("cmd-3", FocusPane3, None),
        KeyBinding::new("cmd-4", FocusPane4, None),
        KeyBinding::new("cmd-5", FocusPane5, None),
        KeyBinding::new("cmd-6", FocusPane6, None),
        KeyBinding::new("cmd-7", FocusPane7, None),
        KeyBinding::new("cmd-8", FocusPane8, None),
        KeyBinding::new("cmd-9", FocusPane9, None),
        KeyBinding::new("cmd-k cmd-left", FocusLeft, None),
        KeyBinding::new("cmd-k cmd-right", FocusRight, None),
        KeyBinding::new("cmd-k cmd-up", FocusUp, None),
        KeyBinding::new("cmd-k cmd-down", FocusDown, None),
        KeyBinding::new("cmd-\\", SplitRight, None),
        KeyBinding::new("cmd-k cmd-\\", SplitDown, None),
        KeyBinding::new("cmd-k cmd-m", ToggleMaximize, None),
        KeyBinding::new("ctrl-cmd-left", MoveTabToPrevPane, None),
        KeyBinding::new("ctrl-cmd-right", MoveTabToNextPane, None),
        KeyBinding::new("cmd-alt-right", NextTab, None),
        KeyBinding::new("cmd-alt-left", PrevTab, None),
        KeyBinding::new("cmd-shift-]", NextTabAlt, None),
        KeyBinding::new("cmd-shift-[", PrevTabAlt, None),
        KeyBinding::new("ctrl-tab", MruTab, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("cmd-shift-t", ReopenClosedTab, None),
        KeyBinding::new("cmd-k w", CloseAllInPane, None),
        KeyBinding::new("cmd-k cmd-shift-left", MoveTabLeft, None),
        KeyBinding::new("cmd-k cmd-shift-right", MoveTabRight, None),
        KeyBinding::new("ctrl-shift-`", NewTerminal, None),
        // Only when a terminal has focus, so the Cmd+K chords above keep working elsewhere.
        KeyBinding::new("cmd-k", ClearScrollback, Some("Terminal")),
        KeyBinding::new("cmd-=", FontBigger, None),
        KeyBinding::new("cmd--", FontSmaller, None),
    ]
}

pub fn tab_cycle(active: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let n = len as isize;
    (((active as isize + delta) % n + n) % n) as usize
}

macro_rules! route {
    ($div:expr, $cx:expr, $($action:ty => $command:expr),* $(,)?) => {{
        let mut d = $div;
        $( d = d.on_action($cx.listener(|root: &mut Root, _: &$action, window: &mut Window, cx: &mut Context<Root>| root.run($command, window, cx))); )*
        d
    }};
}

/// Registers every action on the window's root element.
pub fn attach(div: Div, cx: &mut Context<Root>) -> Div {
    route!(div, cx,
        FocusPane1 => Command::FocusPane(0), FocusPane2 => Command::FocusPane(1), FocusPane3 => Command::FocusPane(2),
        FocusPane4 => Command::FocusPane(3), FocusPane5 => Command::FocusPane(4), FocusPane6 => Command::FocusPane(5),
        FocusPane7 => Command::FocusPane(6), FocusPane8 => Command::FocusPane(7), FocusPane9 => Command::FocusPane(8),
        FocusLeft => Command::FocusDir(Side::Left), FocusRight => Command::FocusDir(Side::Right),
        FocusUp => Command::FocusDir(Side::Top), FocusDown => Command::FocusDir(Side::Bottom),
        SplitRight => Command::Split(Side::Right), SplitDown => Command::Split(Side::Bottom),
        ToggleMaximize => Command::ToggleMaximize,
        MoveTabToPrevPane => Command::MoveTabToPane(-1), MoveTabToNextPane => Command::MoveTabToPane(1),
        NextTab => Command::CycleTab(1), PrevTab => Command::CycleTab(-1),
        NextTabAlt => Command::CycleTab(1), PrevTabAlt => Command::CycleTab(-1),
        MruTab => Command::MruTab,
        CloseTab => Command::CloseTab, ReopenClosedTab => Command::ReopenClosedTab, CloseAllInPane => Command::CloseAllInPane,
        MoveTabLeft => Command::MoveTabWithin(-1), MoveTabRight => Command::MoveTabWithin(1),
        NewTerminal => Command::NewTerminal, ClearScrollback => Command::ClearScrollback,
        FontBigger => Command::FontDelta(1.0), FontSmaller => Command::FontDelta(-1.0),
    )
}
```

Chord behaviour to verify once wired (step 6): gpui's keymap treats `cmd-k` as pending when any multi-key binding starts with it, so a lone Cmd+K in a terminal must still clear the scrollback. If it does not (gpui 0.2.2 waits for the chord's second key indefinitely), rebind ClearScrollback to `"cmd-k cmd-k"`, change the row in `SPEC_ROWS` to match, and record the deviation in the README's keyboard section.

- [ ] **Step 4: clear_scrollback on the core**

Add to `impl TerminalCore` in `terminal.rs`:

```rust
    /// Drops the history and lands at the live bottom. The screen itself is kept.
    pub fn clear_scrollback(&mut self) {
        self.term.grid_mut().clear_history();
        self.set_display_offset(0);
        self.scroll.jump_to(0.0);
        self.scroll.clamp_max(0.0);
    }
```

`Term::grid_mut()` and `Grid::clear_history()` exist in alacritty_terminal 0.26 (`term/mod.rs`, `grid/mod.rs`).

- [ ] **Step 5: Root::run and wiring**

In `main.rs`, add to `impl Root` (it needs `closed: Vec<PaneId>` on `Root`, the panes tabs were closed from, most recent last; add the field, initialised empty):

```rust
    /// Executes one keyboard command.
    pub fn run(&mut self, cmd: keymap::Command, window: &mut Window, cx: &mut Context<Self>) {
        use keymap::Command::*;
        let focused = self.tree.focused;
        match cmd {
            FocusPane(i) => {
                if let Some(p) = self.tree.pane_at(i) {
                    self.focus_pane(p, cx);
                }
            }
            FocusDir(side) => {
                if let Some(p) = self.tree.neighbor(focused, side, self.content) {
                    self.focus_pane(p, cx);
                }
            }
            Split(side) => self.split_focused(side, cx),
            ToggleMaximize => {
                self.tree.toggle_maximize(focused);
                self.save(cx);
                cx.notify();
            }
            MoveTabToPane(delta) => {
                let panes = self.tree.panes();
                let Some(i) = self.tree.pane_index(focused) else { return };
                let target = i as isize + delta;
                if target < 0 || target as usize >= panes.len() {
                    return;
                }
                let to = panes[target as usize];
                let Some(p) = self.tree.pane(focused) else { return };
                if p.tabs.is_empty() {
                    return;
                }
                let idx = p.active;
                if self.tree.move_tab(focused, idx, to, None) {
                    self.save(cx);
                    cx.notify();
                }
            }
            CycleTab(delta) => {
                if let Some(p) = self.tree.pane(focused) {
                    let next = keymap::tab_cycle(p.active, p.tabs.len(), delta);
                    self.activate_tab(focused, next, cx);
                }
            }
            MruTab => {
                if let Some(next) = self.tree.mru_next(focused) {
                    self.activate_tab(focused, next, cx);
                }
            }
            CloseTab => {
                let Some(p) = self.tree.pane(focused) else { return };
                if p.tabs.is_empty() {
                    // Cmd+W on an empty pane closes the pane; the last pane never closes.
                    if self.tree.close_pane(focused) {
                        self.save(cx);
                        cx.notify();
                    }
                    return;
                }
                let idx = p.active;
                self.closed.push(focused);
                self.close_tab(focused, idx, cx);
            }
            ReopenClosedTab => {
                // The closed tab's shell was killed, so "reopen" is a fresh shell where it was.
                let pane = self.closed.pop().filter(|p| self.tree.pane(*p).is_some()).unwrap_or(focused);
                self.new_terminal_tab(pane, cx);
            }
            CloseAllInPane => {
                let n = self.tree.pane(focused).map(|p| p.tabs.len()).unwrap_or(0);
                for _ in 0..n {
                    self.close_tab(focused, 0, cx);
                }
            }
            MoveTabWithin(delta) => {
                let Some(p) = self.tree.pane(focused) else { return };
                if self.tree.move_tab_within(focused, p.active, delta) {
                    self.save(cx);
                    cx.notify();
                }
            }
            NewTerminal => self.new_terminal_tab(focused, cx),
            ClearScrollback => {
                if let Some(t) = self.active_terminal(cx) {
                    t.update(cx, |t, cx| {
                        t.core.clear_scrollback();
                        cx.notify();
                    });
                }
            }
            FontDelta(d) => {
                self.font_size = (self.font_size + d).clamp(10.0, 32.0);
                cx.notify();
            }
        }
        let _ = window;
    }
```

In `Render for Root`, wrap the root div: `keymap::attach(div().size_full()..., cx)` so the `.on_action` handlers sit on the root; give it `.key_context("Root")`. In `pane_view::render_terminal`, add `.key_context("Terminal")` to the terminal's div so the `ClearScrollback` binding's context matches. In `main`, after `cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)])`, add `cx.bind_keys(keymap::bindings());`. Add `mod keymap;`.

- [ ] **Step 6: Tests, build, manual check**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test -p arkos 2>&1 | tail -3; cargo build -p arkos 2>&1 | grep -E '^warning' | head`
Expected: green, no warnings.

Manual (in the running app; what needs Accessibility to drive is "needs human eyes"): Cmd+\ splits right and the new pane starts a shell; Cmd+K Cmd+\ splits down; Cmd+1, Cmd+2, Cmd+3 focus in reading order (the focus ring moves); Cmd+K Cmd+Left/Right move focus by geometry; Cmd+W closes the active tab and the pane stays; Cmd+W on an empty pane closes it; Cmd+W on the last pane does nothing; Cmd+Shift+T opens a fresh shell in the pane the tab was closed from; Ctrl+Shift+` opens a tab; Cmd+Alt+Right and Cmd+Shift+] cycle; Ctrl+Tab goes to the previous tab; Cmd+K alone in a terminal clears the scrollback (see the chord note in step 3); Cmd+= and Cmd+- change the size and the grid re-measures (`tput cols` changes); Cmd+K Cmd+M maximizes and restores.

- [ ] **Step 7: Commit**

```bash
git add crates/arkos/src
git commit -m "arkos: VS Code keyboard map for panes and tabs"
```

---

### Task 7: Drag and drop tabs

**Files:**
- Create: `crates/arkos/src/drag.rs`
- Modify: `crates/arkos/src/pane_view.rs`, `crates/arkos/src/main.rs` (drop state on `Root`), `crates/arkos/src/tree.rs` (`take_tab`, `split_with_tab`)
- Test: pure helpers inline in `drag.rs`; a tree test for `split_with_tab`

**Interfaces:**
- Consumes: `Root`, `tree::*`, gpui `on_drag`, `on_drag_move`, `on_drop` (see `examples/drag_drop.rs`).
- Produces:
  - `drag::TabDrag { pane: PaneId, idx: usize, label: String, position: Point<Pixels> }` (Clone; implements `Render` for the ghost chip).
  - `drag::Zone { Center, Left, Right, Top, Bottom }`, `drag::zone_for(bounds: Bounds<Pixels>, pos: Point<Pixels>) -> Zone` (pure).
  - `drag::DropTarget { TabBar { pane: PaneId, at: usize }, Pane { pane: PaneId, zone: Zone } }`, `drag::insert_index(tab_bounds: Bounds<Pixels>, pos: Point<Pixels>, idx: usize) -> usize` (pure).
  - `Root.drop_target: Option<DropTarget>` and `Root::drop_tab(&mut self, drag: &TabDrag, target: DropTarget, cx)`.
  - `tree::Tree::take_tab(&mut self, pane, idx) -> Option<Tab>` (removes without closing semantics; same as `close_tab`) and `tree::Tree::split_with_tab(&mut self, pane, side, tab) -> Option<PaneId>` (alias of `split` kept for clarity).
  - `Root::drop_tab`: Center moves the tab into the pane (append); a side splits the pane with the tab; a tab bar target inserts at `at`; afterwards, if the source pane is empty and is not the last pane, it closes (spec 4: auto-close only when the last tab is dragged out).

- [ ] **Step 1: Write the failing pure tests**

Create `crates/arkos/src/drag.rs` with its test module:

```rust
//! Dragging a tab (spec 4): a ghost chip follows the pointer; the hovered tab bar shows where
//! it would land; the hovered pane shows five drop zones (center, left, right, top, bottom).

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size, Bounds};

    fn b(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds { origin: point(px(x), px(y)), size: size(px(w), px(h)) }
    }

    #[test]
    fn zones_split_the_pane_into_center_and_four_edges() {
        let pane = b(100.0, 100.0, 400.0, 200.0);
        assert_eq!(zone_for(pane, point(px(300.0), px(200.0))), Zone::Center);
        assert_eq!(zone_for(pane, point(px(110.0), px(200.0))), Zone::Left);
        assert_eq!(zone_for(pane, point(px(490.0), px(200.0))), Zone::Right);
        assert_eq!(zone_for(pane, point(px(300.0), px(105.0))), Zone::Top);
        assert_eq!(zone_for(pane, point(px(300.0), px(295.0))), Zone::Bottom);
        // Corners go to the nearer edge by distance.
        assert_eq!(zone_for(pane, point(px(104.0), px(110.0))), Zone::Left);
    }

    #[test]
    fn insert_index_depends_on_which_half_of_the_tab_is_hovered() {
        let tab = b(200.0, 0.0, 100.0, 28.0);
        assert_eq!(insert_index(tab, point(px(210.0), px(10.0)), 3), 3);
        assert_eq!(insert_index(tab, point(px(290.0), px(10.0)), 3), 4);
    }
}
```

Append to `tree.rs` tests:

```rust
    #[test]
    fn split_with_tab_moves_an_existing_tab_into_a_new_pane() {
        let mut t = Tree::new(term(1));
        let a = t.focused;
        t.add_tab(a, term(2));
        let tab = t.take_tab(a, 1).unwrap();
        let b = t.split_with_tab(a, Side::Bottom, tab).unwrap();
        assert_eq!(t.pane(a).unwrap().tabs, vec![term(1)]);
        assert_eq!(t.pane(b).unwrap().tabs, vec![term(2)]);
        assert_eq!(t.panes(), vec![a, b]);
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test -p arkos drag split_with_tab 2>&1 | grep -E '^error' | head -3`
Expected: missing items.

- [ ] **Step 3: Tree helpers**

Add to `impl Tree` in `tree.rs`:

```rust
    /// Removes a tab for moving elsewhere. Identical to `close_tab`; the name says the intent.
    pub fn take_tab(&mut self, pane: PaneId, idx: usize) -> Option<Tab> {
        self.close_tab(pane, idx)
    }

    /// Splits `pane` and puts an existing tab in the new pane.
    pub fn split_with_tab(&mut self, pane: PaneId, side: Side, tab: Tab) -> Option<PaneId> {
        self.split(pane, side, tab)
    }
```

- [ ] **Step 4: Implement drag.rs**

Insert above the test module:

```rust
use crate::tree::PaneId;
use gpui::{div, prelude::*, px, rgb, Bounds, Context, Pixels, Point, Render, Window};

const CHIP_BG: u32 = 0x1a2130;
const CHIP_FG: u32 = 0xd6dae0;

#[derive(Debug, Clone, PartialEq)]
pub struct TabDrag {
    pub pane: PaneId,
    pub idx: usize,
    pub label: String,
    pub position: Point<Pixels>,
}

impl TabDrag {
    pub fn at(mut self, position: Point<Pixels>) -> Self {
        self.position = position;
        self
    }
}

/// The ghost chip gpui draws under the pointer while dragging.
impl Render for TabDrag {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().pl(self.position.x + px(8.0)).pt(self.position.y + px(8.0)).child(
            div()
                .px(px(10.0))
                .py(px(4.0))
                .bg(rgb(CHIP_BG))
                .text_color(rgb(CHIP_FG))
                .font_family("Menlo")
                .text_size(px(12.0))
                .opacity(0.9)
                .child(self.label.clone()),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Center,
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropTarget {
    TabBar { pane: PaneId, at: usize },
    Pane { pane: PaneId, zone: Zone },
}

/// The inner half of the pane (by each axis) is the center zone; outside it, the nearest edge.
pub fn zone_for(bounds: Bounds<Pixels>, pos: Point<Pixels>) -> Zone {
    let x = f32::from(pos.x - bounds.origin.x) / f32::from(bounds.size.width).max(1.0);
    let y = f32::from(pos.y - bounds.origin.y) / f32::from(bounds.size.height).max(1.0);
    if (0.25..=0.75).contains(&x) && (0.25..=0.75).contains(&y) {
        return Zone::Center;
    }
    let candidates = [(x, Zone::Left), (1.0 - x, Zone::Right), (y, Zone::Top), (1.0 - y, Zone::Bottom)];
    candidates.iter().fold((f32::MAX, Zone::Center), |best, (d, z)| if *d < best.0 { (*d, *z) } else { best }).1
}

/// Hovering the left half of tab `idx` inserts before it, the right half after it.
pub fn insert_index(tab_bounds: Bounds<Pixels>, pos: Point<Pixels>, idx: usize) -> usize {
    let mid = tab_bounds.origin.x + tab_bounds.size.width / 2.0;
    if pos.x < mid {
        idx
    } else {
        idx + 1
    }
}

/// The rectangle a zone highlights inside a pane's bounds, as (x, y, w, h) relative to the pane.
pub fn zone_rect(zone: Zone, w: f32, h: f32) -> (f32, f32, f32, f32) {
    match zone {
        Zone::Center => (0.0, 0.0, w, h),
        Zone::Left => (0.0, 0.0, w / 2.0, h),
        Zone::Right => (w / 2.0, 0.0, w / 2.0, h),
        Zone::Top => (0.0, 0.0, w, h / 2.0),
        Zone::Bottom => (0.0, h / 2.0, w, h / 2.0),
    }
}
```

- [ ] **Step 5: Wire the views**

Add `pub drop_target: Option<drag::DropTarget>` to `Root` (initialised `None`) and this method:

```rust
    /// Lands a dragged tab. The source pane closes only when the drag emptied it (spec 4).
    pub fn drop_tab(&mut self, d: &drag::TabDrag, target: drag::DropTarget, cx: &mut Context<Self>) {
        self.drop_target = None;
        let source = d.pane;
        match target {
            drag::DropTarget::TabBar { pane, at } => {
                self.tree.move_tab(source, d.idx, pane, Some(at));
            }
            drag::DropTarget::Pane { pane, zone: drag::Zone::Center } => {
                if pane == source {
                    cx.notify();
                    return;
                }
                self.tree.move_tab(source, d.idx, pane, None);
            }
            drag::DropTarget::Pane { pane, zone } => {
                let side = match zone {
                    drag::Zone::Left => Side::Left,
                    drag::Zone::Right => Side::Right,
                    drag::Zone::Top => Side::Top,
                    drag::Zone::Bottom => Side::Bottom,
                    drag::Zone::Center => unreachable!(),
                };
                if pane == source && self.tree.pane(source).map(|p| p.tabs.len()).unwrap_or(0) == 1 {
                    cx.notify();
                    return;
                }
                if let Some(tab) = self.tree.take_tab(source, d.idx) {
                    self.tree.split_with_tab(pane, side, tab);
                }
            }
        }
        if self.tree.pane(source).map(|p| p.tabs.is_empty()).unwrap_or(false) {
            self.tree.close_pane(source);
        }
        self.save(cx);
        cx.notify();
    }
```

In `pane_view::render_tab_bar`, each tab div gains (the tab div already has an `id`, which `on_drag` requires):

```rust
                .on_drag(
                    drag::TabDrag { pane, idx, label: label.clone(), position: Default::default() },
                    |d: &drag::TabDrag, position, _window, cx| cx.new(|_| d.clone().at(position)),
                )
                .on_drag_move(cx.listener(move |root, ev: &gpui::DragMoveEvent<drag::TabDrag>, _window, cx| {
                    let at = drag::insert_index(ev.bounds, ev.event.position, idx);
                    root.drop_target = Some(drag::DropTarget::TabBar { pane, at });
                    cx.notify();
                }))
```

and the bar itself gets `.on_drop(cx.listener(move |root, d: &drag::TabDrag, _window, cx| { let target = root.drop_target.unwrap_or(drag::DropTarget::TabBar { pane, at: usize::MAX }); root.drop_tab(d, target, cx); }))` with the bar given an id `("bar", pane)`. A drop with `at: usize::MAX` appends (`move_tab` clamps).

In `render_pane`, the body wrapper gains an id `("body", pane)`, `.on_drag_move(cx.listener(move |root, ev: &gpui::DragMoveEvent<drag::TabDrag>, _window, cx| { let zone = drag::zone_for(ev.bounds, ev.event.position); root.drop_target = Some(drag::DropTarget::Pane { pane, zone }); cx.notify(); }))`, `.on_drop(cx.listener(move |root, d: &drag::TabDrag, _window, cx| { if let Some(target) = root.drop_target { root.drop_tab(d, target, cx); } }))`, and when `root.drop_target == Some(DropTarget::Pane { pane, zone })` an overlay child:

```rust
    let overlay = match root.drop_target {
        Some(drag::DropTarget::Pane { pane: p, zone }) if p == pane => {
            let (x, y, w, h) = drag::zone_rect(zone, rect.w, rect.h - TAB_BAR_H);
            Some(div().absolute().left(px(x)).top(px(y)).w(px(w)).h(px(h)).bg(gpui::rgba(0x3a7bd566)))
        }
        _ => None,
    };
```

added with `.children(overlay)` to a `.relative()` body. Clear `drop_target` on the root's `on_mouse_up` (Task 5's handler) so a cancelled drag leaves no overlay. Add `mod drag;`.

- [ ] **Step 6: Tests, build, manual check**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test -p arkos 2>&1 | tail -3; cargo build -p arkos 2>&1 | grep -E '^warning' | head`

Manual (needs human eyes unless a pointer can be driven): drag a tab within its bar to reorder; drag a tab onto another pane's bar to move it; drag a tab to a pane's center, left, right, top, bottom and see the overlay and the resulting split; drag the last tab out of a pane and see the pane close; drop a tab back on its own place and see nothing change.

- [ ] **Step 7: Commit**

```bash
git add crates/arkos/src
git commit -m "arkos: drag tabs between panes with five drop zones"
```

---

### Task 8: The empty state and the window chrome

**Files:**
- Create: `crates/arkos/src/empty_pane.rs`, `crates/arkos/src/motion.rs`, `crates/arkos/src/chrome.rs`
- Modify: `crates/arkos/src/pane_view.rs` (`render_no_tabs` uses `empty_pane`), `crates/arkos/src/main.rs` (bars, `content` accounts for them, `empty_anim` state)
- Test: pure helpers inline

**Interfaces:**
- Consumes: `Root`, `tree::PaneId`.
- Produces:
  - `motion::reduce_motion() -> bool` (cached; reads macOS's Reduce Motion setting once) and `motion::parse_reduce_motion(stdout: &str) -> bool` (pure).
  - `empty_pane::ANIM_SECS = 6.0`, `empty_pane::dot_positions(elapsed: f32, w: f32, h: f32) -> [(f32, f32); 3]` (pure, settles at `ANIM_SECS`), `empty_pane::render(root: &mut Root, pane: PaneId, w: f32, h: f32, focus: FocusHandle, window, cx) -> Div`, `empty_pane::BINDINGS: &[(&str, &str)]` (the four hints shown).
  - `Root.empty_anim: HashMap<PaneId, Instant>` (when the pane's animation started; absent means settled).
  - `chrome::TOP_BAR_H = 30.0`, `chrome::STATUS_BAR_H = 24.0`, `chrome::top_bar(project_name: &str) -> Div`, `chrome::status_bar(project: &Path, daemon: &str, sessions: usize, waiting: usize) -> Div`, `chrome::status_text(sessions: usize, waiting: usize) -> String` (pure).

- [ ] **Step 1: Write the failing pure tests**

Create `crates/arkos/src/motion.rs`:

```rust
//! Honours macOS's Reduce Motion setting (spec 7.1: the UI adds no motion the user turned off).
//! Read once at launch through `defaults`, which needs no extra dependency; the config reload in
//! milestone 2b re-reads it.
use std::sync::OnceLock;

static REDUCE: OnceLock<bool> = OnceLock::new();

pub fn reduce_motion() -> bool {
    *REDUCE.get_or_init(|| {
        let out = std::process::Command::new("defaults").args(["read", "com.apple.universalaccess", "reduceMotion"]).output();
        match out {
            Ok(o) => parse_reduce_motion(&String::from_utf8_lossy(&o.stdout)),
            Err(_) => false,
        }
    })
}

/// `defaults` prints `1` when the setting is on, `0` or nothing (an error on stderr) when off.
pub fn parse_reduce_motion(stdout: &str) -> bool {
    stdout.trim() == "1"
}

#[cfg(test)]
mod tests {
    use super::parse_reduce_motion;

    #[test]
    fn parses_defaults_output() {
        assert!(parse_reduce_motion("1\n"));
        assert!(!parse_reduce_motion("0\n"));
        assert!(!parse_reduce_motion(""));
    }
}
```

Create `crates/arkos/src/empty_pane.rs` with its test module:

```rust
//! The friendly empty pane (spec 4): a greeting, the bindings you want next, and a slow,
//! low-contrast ambient animation that plays on appearance and on hover, then settles so an idle
//! empty pane costs no frames.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_move_then_settle() {
        let a = dot_positions(0.0, 400.0, 300.0);
        let b = dot_positions(1.5, 400.0, 300.0);
        assert_ne!(a, b, "the animation moves");
        let end = dot_positions(ANIM_SECS, 400.0, 300.0);
        let later = dot_positions(ANIM_SECS + 10.0, 400.0, 300.0);
        assert_eq!(end, later, "after ANIM_SECS the frame is static");
        for (x, y) in end {
            assert!((0.0..=400.0).contains(&x) && (0.0..=300.0).contains(&y));
        }
    }
}
```

Create `crates/arkos/src/chrome.rs` with its test module:

```rust
//! The window chrome (spec 4): a top bar with the project name and a bottom status bar with the
//! project path, daemon status, session count and the count of sessions waiting on the user.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_text_counts_sessions_and_waiting() {
        assert_eq!(status_text(1, 0), "1 session");
        assert_eq!(status_text(4, 2), "4 sessions, 2 waiting");
        assert_eq!(status_text(0, 0), "no sessions");
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test -p arkos motion empty_pane chrome 2>&1 | grep -E '^error|test result' | head -4`
Expected: `motion` passes once its `mod` line exists; the other two fail on missing items.

- [ ] **Step 3: Implement empty_pane.rs**

Insert above the test module:

```rust
use crate::motion::reduce_motion;
use crate::tree::PaneId;
use crate::Root;
use gpui::{div, prelude::*, px, rgb, rgba, Context, Div, FocusHandle, Window};
use std::time::Instant;

pub const ANIM_SECS: f32 = 6.0;
const FG: u32 = 0xd6dae0;
const DIM: u32 = 0x8a93a3;
const DOT: u32 = 0x3a7bd5;

/// The four things a person wants next from an empty pane.
pub const BINDINGS: &[(&str, &str)] = &[
    ("Ctrl+Shift+`", "new terminal here"),
    ("Cmd+\\", "split right"),
    ("Cmd+1 to Cmd+9", "focus a pane"),
    ("Cmd+W", "close this pane"),
];

/// Three dots that drift along slow arcs and ease into their rest positions by `ANIM_SECS`.
/// The curves are time-based, so a dropped frame changes nothing about where they end.
pub fn dot_positions(elapsed: f32, w: f32, h: f32) -> [(f32, f32); 3] {
    let t = (elapsed / ANIM_SECS).clamp(0.0, 1.0);
    // Ease-out: fast at first, settling smoothly.
    let e = 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t);
    let rest = [(0.3, 0.35), (0.5, 0.3), (0.7, 0.35)];
    let mut out = [(0.0, 0.0); 3];
    for (i, (rx, ry)) in rest.iter().enumerate() {
        let phase = i as f32 * 2.1;
        let drift = (1.0 - e) * 0.08;
        let x = rx + drift * (t * 3.0 + phase).sin();
        let y = ry + drift * (t * 2.0 + phase).cos();
        out[i] = ((x * w * 100.0).round() / 100.0, (y * h * 100.0).round() / 100.0);
    }
    out
}

pub fn render(root: &mut Root, pane: PaneId, w: f32, h: f32, focus: FocusHandle, window: &mut Window, cx: &mut Context<Root>) -> Div {
    let started = *root.empty_anim.entry(pane).or_insert_with(Instant::now);
    let elapsed = if reduce_motion() { ANIM_SECS } else { started.elapsed().as_secs_f32() };
    let animating = elapsed < ANIM_SECS;
    if animating {
        window.request_animation_frame();
    } else {
        // Settled: forget the start so the next appearance plays again, and stop costing frames.
        root.empty_anim.insert(pane, started);
    }
    let dots = dot_positions(elapsed, w, h);
    let mut view = div()
        .id(("empty", pane))
        .size_full()
        .relative()
        .track_focus(&focus)
        .on_hover(cx.listener(move |root, hovered: &bool, _window, cx| {
            if *hovered && !reduce_motion() {
                root.empty_anim.insert(pane, Instant::now());
                cx.notify();
            }
        }));
    for (i, (x, y)) in dots.iter().enumerate() {
        let size = 18.0 + 4.0 * i as f32;
        view = view.child(div().absolute().left(px(x - size / 2.0)).top(px(y - size / 2.0)).size(px(size)).rounded_full().bg(rgba((DOT << 8) | 0x33)));
    }
    let mut hints = div().flex().flex_col().gap(px(6.0)).text_color(rgb(DIM)).font_family("Menlo").text_size(px(13.0));
    for (keys, what) in BINDINGS {
        hints = hints.child(div().flex().flex_row().gap(px(12.0)).child(div().w(px(150.0)).text_color(rgb(FG)).child(*keys)).child(*what));
    }
    view.child(
        div()
            .absolute()
            .left_0()
            .top(px(h * 0.5))
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(14.0))
            .child(div().text_color(rgb(FG)).font_family("Menlo").text_size(px(15.0)).child("This pane is empty. Nothing is lost."))
            .child(hints),
    )
}
```

`rgba(u32)` takes `0xRRGGBBAA`; the dots sit at 20 percent alpha on navy, well below the contrast that spec 7.1 counts as modulation. `on_hover` and `rounded_full` are on gpui's interactive and styled traits; `request_animation_frame` on `Window`.

In `pane_view.rs`, replace `render_no_tabs` with:

```rust
fn render_no_tabs(root: &mut Root, pane: PaneId, rect: Rect, window: &mut Window, cx: &mut Context<Root>) -> Div {
    let focus = pane_focus(root, pane, cx);
    crate::empty_pane::render(root, pane, rect.w, rect.h - TAB_BAR_H, focus, window, cx)
}
```

and pass `rect` and `window` from `render_pane` (its signature already has both). Add `pub empty_anim: HashMap<PaneId, Instant>` to `Root`.

- [ ] **Step 4: Implement chrome.rs**

Insert above the test module:

```rust
use gpui::{div, prelude::*, px, rgb, Div};
use std::path::Path;

pub const TOP_BAR_H: f32 = 30.0;
pub const STATUS_BAR_H: f32 = 24.0;
const BAR_BG: u32 = 0x0a0e14;
const FG: u32 = 0xd6dae0;
const DIM: u32 = 0x8a93a3;

pub fn status_text(sessions: usize, waiting: usize) -> String {
    let base = match sessions {
        0 => "no sessions".to_string(),
        1 => "1 session".to_string(),
        n => format!("{n} sessions"),
    };
    if waiting > 0 {
        format!("{base}, {waiting} waiting")
    } else {
        base
    }
}

pub fn top_bar(project_name: &str) -> Div {
    div()
        .h(px(TOP_BAR_H))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(BAR_BG))
        .text_color(rgb(FG))
        .font_family("Menlo")
        .text_size(px(13.0))
        .child(project_name.to_string())
}

pub fn status_bar(project: &Path, daemon: &str, sessions: usize, waiting: usize) -> Div {
    div()
        .h(px(STATUS_BAR_H))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .px(px(10.0))
        .bg(rgb(BAR_BG))
        .text_color(rgb(DIM))
        .font_family("Menlo")
        .text_size(px(12.0))
        .child(project.display().to_string())
        .child(div().flex().gap(px(16.0)).child(daemon.to_string()).child(status_text(sessions, waiting)))
}
```

In `Render for Root`: compute `top = chrome::TOP_BAR_H`, `bottom = chrome::STATUS_BAR_H`, set `self.content = Rect { x: 0.0, y: top + banner_h, w, h: height - top - bottom - banner_h }`, and build the column as top bar, optional banner, the tree body (`flex_1().min_h_0()`), status bar with `daemon = if self.status.is_empty() { "arkosd connected" } else { "arkosd: see banner" }`, `sessions = self.sessions.terminals.values().filter(|t| t.read(cx).alive).count()`, `waiting = 0` (milestone 3 fills it from `ClaudeState`). The project name is `self.project.file_name()`'s string. Add `mod chrome; mod empty_pane; mod motion;`.

- [ ] **Step 5: Tests, build, manual check**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test -p arkos 2>&1 | tail -3; cargo build -p arkos 2>&1 | grep -E '^warning' | head`

Manual: close every tab in a pane: the greeting, the four hints and three soft dots that drift for about six seconds and stop; hovering replays it; with System Settings > Accessibility > Display > Reduce Motion on, nothing moves; with the pane idle, Activity Monitor shows `arkos` at 0 percent CPU (no frames); the top bar shows the folder name; the status bar shows the path, "arkosd connected" and the session count.

- [ ] **Step 6: Commit**

```bash
git add crates/arkos/src
git commit -m "arkos: friendly empty pane with a settling animation, top bar and status bar"
```

---

### Task 9: Milestone 1 carry-overs in the terminal

**Files:**
- Modify: `crates/arkos/src/keys.rs`, `crates/arkos/src/terminal.rs`, `crates/arkos/src/pane_view.rs`
- Test: inline

**Interfaces:**
- Produces:
  - `keys::keystroke_to_bytes`: Shift, Alt (Option) and Ctrl on arrows, Home and End send xterm's modified sequences `ESC [ 1 ; m X` with `m = 1 + shift + 2 alt + 4 ctrl`; Option-s and friends keep sending the composed character.
  - `terminal::TerminalCore::scrolled_up(&self) -> bool`; `terminal::Terminal::jump_to_bottom(&mut self, cx)`; `handle_key` sends nothing once the session has ended.
  - The jump-to-bottom pill in `pane_view::render_terminal` (spec 3: "a small jump to bottom affordance appears" while scrolled up).

- [ ] **Step 1: Write the failing tests**

Append to `keys.rs` tests:

```rust
    #[test]
    fn modified_arrows_use_xterm_parameters() {
        assert_eq!(keystroke_to_bytes(&ks("up", None, shift()), false), Some(b"\x1b[1;2A".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("left", None, ctrl()), false), Some(b"\x1b[1;5D".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("right", None, alt()), false), Some(b"\x1b[1;3C".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("end", None, shift()), false), Some(b"\x1b[1;2F".to_vec()));
        // Modifiers win over application cursor mode, as in xterm.
        assert_eq!(keystroke_to_bytes(&ks("up", None, shift()), true), Some(b"\x1b[1;2A".to_vec()));
        assert_eq!(keystroke_to_bytes(&ks("up", None, none()), true), Some(b"\x1bOA".to_vec()));
    }
```

(`ks`, `shift`, `ctrl`, `alt`, `none` are the helpers already in that test module; if `alt()` is missing add it beside `shift()`.)

Append to `terminal.rs` tests:

```rust
    #[test]
    fn scrolled_up_tracks_the_animator_and_the_grid() {
        let mut t = TerminalCore::new(20, 3, 100);
        for i in 0..10 {
            t.apply_output(format!("l{i}\r\n").as_bytes());
        }
        assert!(!t.scrolled_up());
        t.scroll.scroll_by(2.0, t.history_size() as f32);
        assert!(t.scrolled_up(), "a scroll target above the bottom counts");
        t.scroll.jump_to(0.0);
        t.set_display_offset(0);
        assert!(!t.scrolled_up());
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test -p arkos keys terminal 2>&1 | grep -E '^error|FAILED|panicked' | head -4`

- [ ] **Step 3: Modified arrows in keys.rs**

In `keystroke_to_bytes`, before the existing arrow handling, add:

```rust
    let m = &k.modifiers;
    let mods = 1 + m.shift as u8 + 2 * m.alt as u8 + 4 * m.control as u8;
    if mods > 1 {
        let suffix = match k.key.as_str() {
            "up" => Some('A'),
            "down" => Some('B'),
            "right" => Some('C'),
            "left" => Some('D'),
            "home" => Some('H'),
            "end" => Some('F'),
            _ => None,
        };
        if let Some(c) = suffix {
            return Some(format!("\x1b[1;{mods}{c}").into_bytes());
        }
    }
```

(Cmd combinations already returned `None` above this point; keep that order.)

- [ ] **Step 4: scrolled_up, jump_to_bottom, dead sessions**

In `terminal.rs`, add to `impl TerminalCore`:

```rust
    /// True while the view shows history: either the grid is offset or the animator is heading
    /// there. Drives the jump-to-bottom affordance.
    pub fn scrolled_up(&self) -> bool {
        self.display_offset() > 0 || self.scroll.target > 0.5 || self.scroll.current > 0.5
    }
```

and to `impl Terminal`:

```rust
    pub fn jump_to_bottom(&mut self, cx: &mut Context<Self>) {
        self.core.scroll.jump_to(0.0);
        self.core.set_display_offset(0);
        cx.notify();
    }
```

At the top of `handle_key`, add `if !self.alive { return; }` (the daemon would only answer "no session"; the banner already says it ended). Do the same in `paste`.

In `pane_view::render_terminal`, make the wrapper `.relative()` and append, when `terminal.read(cx).core.scrolled_up()`:

```rust
        .children(scrolled_up.then(|| {
            let t = terminal.clone();
            div()
                .id(("jump", pane))
                .absolute()
                .bottom(px(10.0))
                .right(px(14.0))
                .px(px(10.0))
                .py(px(4.0))
                .rounded(px(10.0))
                .bg(rgb(0x1a2130))
                .text_color(rgb(FG))
                .font_family("Menlo")
                .text_size(px(12.0))
                .cursor_pointer()
                .child("jump to bottom")
                .on_click(cx.listener(move |_root, _ev: &ClickEvent, _window, cx| t.update(cx, |t, cx| t.jump_to_bottom(cx))))
        }))
```

(`scrolled_up` read before building the div, since `cx` is borrowed by the listener.) The pill paints only while scrolled up, so an idle terminal draws nothing extra.

- [ ] **Step 5: Tests, build, manual check**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo test -p arkos 2>&1 | tail -3`
Manual: scroll up in a terminal: the pill appears; click it: the view glides to the bottom and the pill goes; Shift+Up in `less` or in a shell with `bindkey` shows the modified sequence (`cat -v` then press Shift+Up prints `^[[1;2A`).

- [ ] **Step 6: Commit**

```bash
git add crates/arkos/src
git commit -m "arkos: jump-to-bottom affordance, modified arrow keys, no input to ended sessions"
```

---

### Task 10: Keep the heartbeat alive while minimized (App Nap)

**Files:**
- Create: `crates/arkos/src/app_nap.rs`
- Modify: `crates/arkos/Cargo.toml`, `crates/arkos/src/main.rs`
- Test: manual (the behaviour is the OS's)

**Interfaces:**
- Produces: `app_nap::hold()` called once at startup: begins an `NSProcessInfo` activity with `NSActivityUserInitiatedAllowingIdleSystemSleep` and reason "arkosd heartbeat" and keeps the token for the process lifetime, so macOS App Nap cannot throttle the 1 s heartbeat timer of a minimized or occluded window into a watchdog kill (spec 2.4).

- [ ] **Step 1: Dependency**

In `crates/arkos/Cargo.toml` add, with the versions `Cargo.lock` already resolves for gpui (`objc2-foundation 0.3.2`; check `objc2`'s with `grep -A1 'name = "objc2"$' Cargo.lock`):

```toml
objc2 = "0.6"
objc2-foundation = { version = "0.3.2", features = ["NSProcessInfo", "NSString", "NSObjCRuntime"] }
```

If `cargo build` reports a feature name that does not exist, list the crate's features with `grep -n '^\[features\]' -A 200 ~/.cargo/registry/src/*/objc2-foundation-0.3.2/Cargo.toml | grep -i 'processinfo\|nsstring'` and use those names.

- [ ] **Step 2: app_nap.rs**

```rust
//! Opts out of App Nap for the whole process. A minimized or fully occluded window would
//! otherwise have its timers throttled, and the daemon's watchdog reads a late heartbeat as a
//! hung UI (spec 2.4).
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSActivityOptions, NSObjectProtocol, NSProcessInfo, NSString};
use std::sync::OnceLock;

static TOKEN: OnceLock<Retained<ProtocolObject<dyn NSObjectProtocol>>> = OnceLock::new();

pub fn hold() {
    TOKEN.get_or_init(|| {
        let info = NSProcessInfo::processInfo();
        let reason = NSString::from_str("arkosd heartbeat");
        // SAFETY: plain Foundation call on the main thread with valid arguments; the returned
        // token is retained for the process lifetime.
        unsafe { info.beginActivityWithOptions_reason(NSActivityOptions::UserInitiatedAllowingIdleSystemSleep, &reason) }
    });
}
```

The exact names come from the generated bindings: `grep -n 'beginActivityWithOptions_reason\|UserInitiatedAllowingIdleSystemSleep' ~/.cargo/registry/src/*/objc2-foundation-0.3.2/src/generated/NSProcessInfo.rs` shows the method and the option constant; the return type is what the method signature says (a `Retained<ProtocolObject<dyn NSObjectProtocol>>` in 0.3.x). Adjust the `TOKEN` type to match exactly what the method returns; `unsafe` is required only if the binding is marked unsafe. The `Retained` token must stay alive: the `OnceLock` does that.

In `main`, call `app_nap::hold();` right after the QoS block. Add `mod app_nap;`.

- [ ] **Step 3: Build and the manual check**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; cargo build -p arkos 2>&1 | grep -E '^(warning|error)' | head`
Manual (needs human eyes, at least 2 minutes): launch, minimize the window, wait two minutes with another app in front, restore: the terminal is still attached (no relaunch happened; `~/Library/Application Support/arkos/arkosd.log` shows no "killing" line and `pgrep -x arkos` is the same pid as before). Record the pid before and after in the report.

- [ ] **Step 4: Commit**

```bash
git add crates/arkos/Cargo.toml crates/arkos/src/app_nap.rs crates/arkos/src/main.rs Cargo.lock
git commit -m "arkos: hold an NSProcessInfo activity so App Nap cannot throttle the heartbeat"
```

---

### Task 11: README, measurements, and the human checklist

**Files:**
- Modify: `README.md`
- Test: `cargo test --workspace`, the daemon suite ten times, a release build, and the measurements below

- [ ] **Step 1: Full suite and release build**

Run: `export PATH="$HOME/.cargo/bin:$PATH"; pkill -x arkosd; cargo test --workspace 2>&1 | grep -E 'test result|warning: unused|FAILED'; for i in $(seq 1 10); do cargo test -p arkosd 2>&1 | grep -E 'FAILED' ; done; echo done; cargo build --release 2>&1 | tail -1`
Expected: every suite green, ten clean daemon runs, release build finished.

- [ ] **Step 2: Measure four terminals**

Run the release UI (`./target/release/arkos` from a project folder, stderr to a file under the scratch directory), make a 2x2 layout with Cmd+\ and Cmd+K Cmd+\ (or by editing the workspace JSON and relaunching), let it idle for 30 s, then `ps -o rss,comm -p $(pgrep -x arkos) -p $(pgrep -x arkosd)`. Record both numbers in the commit body against the spec's budgets (UI with four terminals under 120 MB, daemon under 15 MB). Stop both processes afterwards.

- [ ] **Step 3: README**

Replace the `## Status` section with:

```markdown
## Status

Milestone 2a of 6: panes, tabs, drag and drop, the VS Code keyboard map, and a layout that
survives relaunch. Milestone 2b (config file, themes, text presets, command palette, find)
is next. The daemon now notices a shell exit even when a background job keeps the terminal
open, never signals a reaped process, and forgets dead sessions ten minutes after they end
once no window shows them.

Things only a person at the screen can check, in order of value:

- typing in each pane, and that Cmd combinations never reach the shell
- Cmd+1 to Cmd+9 numbering after splits and closes, left to right then top to bottom
- dragging a tab to each of the five drop zones, within a bar, and out of a pane
- dragging a split handle, the 160 by 80 px minimum, and double-click resetting it
- Cmd+K alone clearing the scrollback while the Cmd+K chords still work
- the empty pane's animation settling, replaying on hover, and holding still with Reduce Motion
- scroll direction and glide, and the jump-to-bottom pill
- Cmd+V bracketed paste into `claude`
- after `kill -9`: the layout, every session, replayed history, and typing
- a session ended in a background tab marks its tab without stealing focus
- the window minimized for two minutes keeps its sessions
- `ps` after thirty seconds idle with four terminals
```

Add a `## Keyboard` section listing the spec's table (the rows of `SPEC_ROWS` with their meanings) and the note that Cmd+F, Cmd+,, Cmd+Shift+P, Cmd+B, Cmd+Shift+E and Ctrl+Shift+G are not bound until milestones 2b and 4. Keep the rest of the README as it is; keep every link; no em-dashes.

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "README: milestone 2a status, keyboard map, human checklist"
```

---

## Self-review notes

**Spec coverage for milestone 2a.** Section 4 model, resizing, tabs, drag and drop between panes, empty state, keyboard, persistence, window chrome: Tasks 2, 5, 7, 8, 6, 3, 8. Section 3 "jump to bottom": Task 9. Section 2.1 reaping, 2.4 relaunch safety: Task 1 (and Task 10 for App Nap). Section 9 non-blocking: the save queue (Task 3) and the background replay (Task 5) keep IO off the UI thread. Section 10 "property tests for split, close, move and ratio; every pane reachable by Cmd+1 to 9": Task 2. Section 11 milestone 2 also names the Navy theme and config with live reload: those are milestone 2b, a separate plan, together with text presets (section 7), the command palette, find, and the tab bar's overflow menu (which needs the palette's commands to be useful).

**Deliberate deviations recorded for the executor.** The tab bar has a plus button but no overflow menu (2b). Cmd+K's chord behaviour is verified at Task 6 step 6 with a stated fallback. Reduce Motion is read once at launch through `defaults` rather than observed live (2b's config reload re-reads it). Session ids of `0` mark placeholder tabs whose shell is being created; reconcile treats them as missing, which is the fresh-shell path.

**Type consistency checked:** `Layout.handles` is a 4-tuple everywhere (Tasks 2 and 5); `SessionManager::new(transport, client, project)` (Tasks 4 and 5); `TerminalElement::new(terminal, font_size)` and `grid_for(bounds, window, font_size)` (Tasks 5, 8); `render_pane(root, pane, rect, window, cx)` (Tasks 5, 7, 8); `Root::run(Command, window, cx)` (Task 6); `drop_tab(&TabDrag, DropTarget, cx)` (Task 7).

**Review Focus pins:** 1 in Task 2 (`last_pane_never_closes`) and Task 8 (manual: closing the last tab shows the empty state); 2 in Task 2 (`move_tab_onto_itself_is_a_no_op`); 3 in Task 2 (`panes_are_numbered_in_reading_order`, `every_pane_is_reachable`); 4 in Task 4 (`exit_in_a_background_tab_does_not_change_focus`); 5 in Task 4 (`reconcile_replaces_dead_and_missing_sessions_in_place`).
