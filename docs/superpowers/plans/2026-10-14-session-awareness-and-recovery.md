# Sarkos IDE: Session awareness and recovery after a restart. Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every tab say what its Claude Code session is doing (working, waiting on you, idle, ended) and bring every session back on its own after a restart: the same folders, the history on screen, and each Claude Code conversation resumed, with nothing to redo by hand.

**Architecture:** Claude Code's lifecycle hooks run `sarkos notify`, which reads `SARKOS_SESSION_ID` from its environment and sends the daemon a state (`SetState`, already in the protocol) or an agent record (`SetAgent`: the agent's own session id and the command that resumes it). The daemon keeps each session's shell, folder, agent record and state, flushes each replay ring to a file a couple of seconds after output stops and in full on SIGTERM, and writes a manifest of sessions and open windows. A launchd agent starts `sarkosd --recover` at login, which recreates each shell in its folder with the old history under a divider, types the recorded resume command into it, and launches a window for each project that had one. The window shows a slow pulse on working tabs, a warm dot on waiting ones, counts waiting sessions in the status bar, and clears a dot when the tab is activated or typed into. Protocol 3 appends four requests and one response; the daemon still serves protocol 1 and 2 windows; the handover file becomes format 2 and the daemon still reads format 1, so the update into this version is an ordinary "Update now".

**Tech Stack:** Rust stable (edition 2021), gpui 0.2.2 (exact pin, `runtime_shaders`), libc 0.2 (`sigwait`, `pthread_sigmask`), postcard 1.1 for the manifest and handover, serde, serde_json (already a dependency of `sarkos`) for the hook settings. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-10-13-sarkos-design.md` section 5 (session awareness), section 2.3 (the `notify` CLI), section 7.1 (motion) and section 11 milestone 3; `docs/superpowers/specs/2026-10-14-update-in-place-design.md` section 11 (recovery after a reboot). The update-in-place spec's sections 2 and 5 (handover file, protocol rules) still bind.

## Global Constraints

- macOS only, Apple Silicon. Sessions always run on this Mac: nothing in this plan talks to any other machine, and every file this milestone writes stays under `~/Library/Application Support/sarkos/`, `~/Library/LaunchAgents/` or Claude Code's own settings file.
- `gpui = { version = "=0.2.2", features = ["runtime_shaders"] }` pinned exactly; never bump. If a gpui or libc name in this plan does not compile, the API differs: grep `~/.cargo/registry/src/*/gpui-0.2.2/src/` or `~/.cargo/registry/src/*/libc-0.2.*/src/unix/bsd/apple/mod.rs` for it, fix the call, and note the change in the commit message. Do not guess.
- Protocol (update-in-place spec section 5): `PROTOCOL_VERSION` becomes 3, `PROTOCOL_MIN` stays 1. New enum variants are appended, never inserted; existing structs never change shape (new data goes in new structs). The daemon serves versions 1, 2 and 3.
- Handover (update-in-place spec section 2): `handover::FORMAT` becomes 2 and the daemon still reads format 1, so "Update now" from the current main works without a `pkill`. `--check-handover` prints the oldest format the binary reads; a daemon accepts a candidate whose oldest readable format is at most the one it writes.
- The daemon never parses escape sequences (the divider line it seeds into a recovered ring is written, not parsed); the UI thread never blocks on IO (the hook CLI, `launchctl` and the settings merge run off it or in a separate process); the daemon never depends on which agent runs inside a shell beyond the agent record.
- `sarkos notify` always exits 0 and never blocks Claude Code: no `SARKOS_SESSION_ID`, no daemon, a daemon that does not understand the request, or bad stdin all end quietly within two seconds.
- Recovery only resumes; it never sends a message to an agent. The resume command is typed into the shell as a command line and nothing else is.
- Display name "Sarkos IDE" in user-facing text; binaries, crates, the socket directory and log prefixes stay lowercase `sarkos`. The launchd label is `sh.csarko.sarkosd`.
- Commit messages: imperative, one line, body optional. No em-dashes in any copy or comment. Every commit ends with a blank line, `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_015xrVD54zbvkgksJUndxfpr`.
- Tests: `cargo test -p sarkos` stays headless; daemon tests in `crates/sarkosd/tests/daemon.rs` run the real binary and must pass ten times in a row; the build stays warning-free for the project's crates; `cargo` is at `~/.cargo/bin/cargo`. Daemon tests use `--socket` under a temp dir, so their replay files and manifest land in that dir too; never the user's.

## Review Focus

1. **A hook fires outside Sarkos IDE.** `claude` run in Terminal.app has no `SARKOS_SESSION_ID`; its hooks still run `sarkos notify`, which must exit 0 at once and print nothing Claude Code would show. Pinned by Task 3's `notify_without_a_session_id_exits_zero_and_sends_nothing`.
2. **SIGTERM while output floods.** A shutdown's SIGTERM during heavy output must write the ring's last bytes and exit 0 in well under launchd's timeout, never hang on a reader. Pinned by Task 4's `sigterm_writes_the_last_output_and_exits_zero`.
3. **A session's folder is gone after the restart.** A manifest entry whose cwd no longer exists must come back in the home folder with a log line, not fail the whole recovery. Pinned by Task 5's `recover_puts_a_session_whose_folder_is_gone_in_home`.
4. **The replay file is longer than the ring.** Appends run ahead of compaction, so a file can hold more than `replay_capacity` bytes; recovery must seed the ring with the tail, never the head. Pinned by Task 5's `tail_keeps_the_newest_bytes`.
5. **Reduce Motion is on.** The working glyph must hold still and the window must not tick at 10 Hz for it. Pinned by Task 6's `the_pulse_holds_still_under_reduce_motion`.

---

## File structure

```
crates/
  protocol/src/lib.rs             PROTOCOL_VERSION 3; AgentRecord, SessionDetail; Request::{Goodbye, SetAgent, ClearAgent, ListSessionsDetailed}; Response::SessionsDetailed
  sarkosd/
    src/session.rs                shell, nice, env, agent, recovered, seed; dirty tracking and take_delta for the flusher; detail()
    src/server.rs                 the four new arms; State::{windows, manifest_dirty}; is_sarkos_binary(); run_with(opts, recover); ALREADY_LISTENING
    src/handover.rs               FORMAT 2 with the new fields, OLDEST_READ 1, frozen V1 structs and conversion; atomic_write factored out
    src/upgrade.rs                validate accepts "handover N" with N <= FORMAT; the handover carries windows and flush_quiet_millis
    src/ring.rs                   total() and since(from) for incremental flushing
    src/recovery.rs               NEW: sessions dir, replay files, manifest, the flusher thread, SIGTERM watcher, recover()
    src/main.rs                   --recover, --flush-quiet-millis; --check-handover prints OLDEST_READ; signals blocked before any thread
    tests/daemon.rs               agent record, detailed list, goodbye, flush, sigterm, recover tests (append); start_in / sigterm helpers
  sarkos/
    src/hook_cli.rs               NEW: `sarkos notify` and `sarkos hooks`: arg parsing, stdin JSON, one-shot request, hooks JSON, settings merge
    src/pulse.rs                  NEW: the working pulse curve and its low-rate ticker
    src/launch_agent.rs           NEW: the launchd plist, install/remove through launchctl, the desired-state decision
    src/sessions.rs               states, Note, SessionDetail list, set_state/acknowledge, goodbye, waiting_count
    src/pane_view.rs              tab marks: the pulse glyph, the waiting dot, muted ended tabs
    src/chrome.rs                 unchanged API; the waiting count is now real
    src/config.rs                 [recovery] start_at_login = "ask" | "on" | "off"
    src/keymap.rs                 "start at login" and "install claude code hooks" (no keys)
    src/command_palette.rs        the two items with their details
    src/main.rs                   subcommands before the GUI; Root: window_active, recovery notice; leaving() sends Goodbye; acknowledge on focus and typing
  README.md                       Status, Session awareness, After a restart, checklist
```

`recovery.rs` is the only daemon file that knows the sessions directory. `hook_cli.rs` has no gpui in it. `pulse.rs` and `launch_agent.rs` keep their pure parts (curve, plist text, decision) free of IO so tests pin them.

Rulings the plan makes where the spec is silent or where the spec's letter costs more than its intent (the executor records any further one in the ledger):

1. The replay file holds the ring's bytes as they are, appended as output arrives, not the handover record: the handover's pid and descriptor numbers mean nothing after a reboot, and rewriting 16 MB every few seconds for a chatty session would write gigabytes a day to the SSD. The file is rewritten from the ring only when the ring dropped bytes that were never flushed, or when the file has grown past twice the ring (compaction).
2. A window the user quits (Cmd+Q, or closing the last window) says `Goodbye`, so a restart does not reopen it; a window that vanishes without one (killed by the shutdown, or crashed) is reopened. macOS's own terminate at logout does not run the window's Quit action, only gpui's quit observers, so nothing there sends `Goodbye`.
3. The resume command is typed into the recovered shell as one line (typeahead: the bytes wait in the PTY until the shell reads its first prompt). Nothing else is ever typed; the user sends the next message.
4. The waiting dot clears when the tab is activated or typed into, and the window tells the daemon (`SetState Idle`) so a reconnect or another window agrees.
5. A terminal BEL marks the tab waiting too (spec 5: the feature degrades without hooks).
6. `sarkosd --recover` with a daemon already listening exits 0 and leaves it alone; without a manifest it is a plain start. So the launchd agent is harmless whenever it runs.
7. Start at login needs the user's say-so: `[recovery] start_at_login = "ask"` by default shows a one-line notice at launch until "Start at login" runs from the palette; `"on"` and `"off"` are kept in force by every config reload.
8. Claude Code hooks are installed by `sarkos hooks install` (and the palette's "Install Claude Code hooks"), which merges into `~/.claude/settings.json` idempotently after writing a `.bak`; `sarkos hooks print` prints the block for a hand paste.

---

### Task 1: Protocol 3 and the daemon's agent record, window list and detailed list

**Files:**
- Modify: `crates/protocol/src/lib.rs`, `crates/sarkosd/src/session.rs`, `crates/sarkosd/src/server.rs`, `crates/sarkosd/src/upgrade.rs` (only where `SpawnSpec` is built, if at all), `crates/sarkosd/tests/daemon.rs`
- Test: `crates/protocol/src/lib.rs` (new unit test), `crates/sarkosd/src/session.rs` (unit), `crates/sarkosd/tests/daemon.rs` (append)

**Interfaces:**
- Consumes: `Request::SetState`, `Session::info()`, `State.clients` with `ui_cwd` and `ui_exe` (from the update-in-place milestone).
- Produces, used by every later task:
  - `protocol::AgentRecord { kind: String, id: String, resume: Vec<String> }`, `protocol::SessionDetail { info: SessionInfo, recovered: bool, agent: Option<AgentRecord> }`
  - `Request::Goodbye`, `Request::SetAgent { session, agent }`, `Request::ClearAgent { session, id }`, `Request::ListSessionsDetailed`; `Response::SessionsDetailed(Vec<SessionDetail>)`; `PROTOCOL_VERSION = 3`
  - `SpawnSpec { .., seed: Vec<u8>, recovered: bool, agent: Option<AgentRecord> }`
  - `Session::{set_agent, clear_agent(&str) -> bool, agent(), detail(), shell(), nice(), env(), cwd()}`
  - `State { windows: HashMap<PathBuf, Option<PathBuf>>, manifest_dirty: bool, .. }`, `server::is_sarkos_binary(&Path) -> bool`

- [ ] **Step 1: Write the failing protocol test**

Append to `crates/protocol/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// postcard writes an enum's variant index first; appended variants keep every older index.
    #[test]
    fn new_variants_are_appended_after_the_old_ones() {
        assert_eq!(PROTOCOL_VERSION, 3);
        assert_eq!(PROTOCOL_MIN, 1);
        assert_eq!(postcard::to_allocvec(&Request::RelaunchClient { exe: "/x".into(), cwd: "/y".into() }).unwrap()[0], 11);
        assert_eq!(postcard::to_allocvec(&Request::Goodbye).unwrap()[0], 12);
        let agent = AgentRecord { kind: "claude".into(), id: "abc".into(), resume: vec!["claude".into(), "--resume".into(), "abc".into()] };
        assert_eq!(postcard::to_allocvec(&Request::SetAgent { session: SessionId(1), agent: agent.clone() }).unwrap()[0], 13);
        assert_eq!(postcard::to_allocvec(&Request::ClearAgent { session: SessionId(1), id: "abc".into() }).unwrap()[0], 14);
        assert_eq!(postcard::to_allocvec(&Request::ListSessionsDetailed).unwrap()[0], 15);
        assert_eq!(postcard::to_allocvec(&Response::Hello { daemon_version: String::new(), protocol_min: 1, protocol_max: 3 }).unwrap()[0], 5);
        let detail = SessionDetail {
            info: SessionInfo { id: SessionId(1), title: String::new(), cwd: "/p".into(), pid: None, alive: true, last_activity_unix_ms: 0, state: ClaudeState::Idle },
            recovered: true,
            agent: Some(agent),
        };
        let bytes = postcard::to_allocvec(&Response::SessionsDetailed(vec![detail.clone()])).unwrap();
        assert_eq!(bytes[0], 6);
        assert_eq!(postcard::from_bytes::<Response>(&bytes).unwrap(), Response::SessionsDetailed(vec![detail]));
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `~/.cargo/bin/cargo test -p protocol -- new_variants_are_appended`
Expected: FAIL to compile: `AgentRecord`, `SessionDetail`, `Goodbye` not found.

- [ ] **Step 3: Add the types and variants**

In `crates/protocol/src/lib.rs`: `PROTOCOL_VERSION` becomes `3` (update its doc comment: "Version 3 adds the agent record, the detailed list and Goodbye for recovery after a restart"). After `SessionInfo` add:

```rust
/// The agent running inside a session, from its own SessionStart hook: enough to resume it
/// after a restart (update-in-place spec, section 11). The daemon never reads more into a shell
/// than this record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRecord {
    /// "claude" for Claude Code; "codex" in a later milestone.
    pub kind: String,
    /// The agent's own session id.
    pub id: String,
    /// The command that resumes it, as argv.
    pub resume: Vec<String>,
}

/// `SessionInfo` plus what milestone 3 adds. A new struct, because `SessionInfo` never changes
/// shape (section 5 of the update-in-place spec).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionDetail {
    pub info: SessionInfo,
    /// Recreated by `sarkosd --recover` after a restart.
    pub recovered: bool,
    pub agent: Option<AgentRecord>,
}
```

Append to `Request`, after `RelaunchClient`:

```rust
    /// The window is quitting because the user asked: a restart must not reopen it. A window
    /// that vanishes without this (the shutdown killed it, or it crashed) is reopened.
    Goodbye,
    /// From the agent's SessionStart hook through `sarkos notify`. Replaces any earlier record.
    SetAgent { session: SessionId, agent: AgentRecord },
    /// From the agent's SessionEnd hook. Clears the record only when it is still `id`'s: a
    /// SessionEnd that lands after the next SessionStart must not erase the new record.
    ClearAgent { session: SessionId, id: String },
    /// `ListSessions` with the recovery fields. A version 3 window sends this one.
    ListSessionsDetailed,
```

Append to `Response`, after `Hello`:

```rust
    SessionsDetailed(Vec<SessionDetail>),
```

- [ ] **Step 4: Run the protocol test**

Run: `~/.cargo/bin/cargo test -p protocol`
Expected: PASS (the new test and the existing codec tests).

- [ ] **Step 5: Write the failing session unit test**

In `crates/sarkosd/src/session.rs`'s `mod tests`, add (model it on `snapshot_records_the_session_and_adopt_rebuilds_it`, which shows how a `SpawnSpec` is built in tests; add the three new fields there and in every other `SpawnSpec` literal in the file):

```rust
    #[test]
    fn the_agent_record_is_set_cleared_by_its_own_id_and_reported_in_detail() {
        let dir = tempfile::tempdir().unwrap();
        let s = Session::spawn_for_test(
            SpawnSpec {
                id: SessionId(7),
                cwd: dir.path().to_path_buf(),
                shell: Some("/bin/sh".into()),
                env: vec![("SARKOS_TEST".into(), "1".into())],
                nice: 0,
                rows: 24,
                cols: 80,
                replay_capacity: 1024,
                backpressure: Backpressure::default(),
                seed: b"old output".to_vec(),
                recovered: true,
                agent: None,
            },
            false,
        )
        .unwrap();
        assert_eq!(s.shell(), "/bin/sh");
        assert_eq!(s.nice(), 0);
        assert_eq!(s.env(), &[("SARKOS_TEST".to_string(), "1".to_string())]);
        assert_eq!(s.cwd(), dir.path());
        let d = s.detail();
        assert!(d.recovered);
        assert_eq!(d.agent, None);
        assert_eq!(d.info.id, SessionId(7));
        let agent = protocol::AgentRecord { kind: "claude".into(), id: "a1".into(), resume: vec!["claude".into(), "--resume".into(), "a1".into()] };
        s.set_agent(agent.clone());
        assert_eq!(s.agent(), Some(agent.clone()));
        assert!(!s.clear_agent("someone-else"), "another id's SessionEnd leaves the record");
        assert_eq!(s.agent(), Some(agent));
        assert!(s.clear_agent("a1"));
        assert_eq!(s.detail().agent, None);
        // The seed is the ring's first content.
        assert_eq!(s.snapshot().replay, b"old output");
        let _ = s.kill();
    }
```

- [ ] **Step 6: Run it to see it fail**

Run: `~/.cargo/bin/cargo test -p sarkosd --bin sarkosd -- the_agent_record_is_set`
Expected: FAIL to compile: no field `seed`, no method `shell`.

- [ ] **Step 7: Add the fields and methods to Session**

In `crates/sarkosd/src/session.rs`:

`SpawnSpec` gains three fields (add doc comments):

```rust
    /// Bytes the ring starts with: the history a recovered session shows under its divider.
    pub seed: Vec<u8>,
    /// Recreated by `--recover` after a restart.
    pub recovered: bool,
    pub agent: Option<AgentRecord>,
```

`Session` gains:

```rust
    /// What a restart needs to recreate the shell (update-in-place spec, section 11).
    shell: String,
    nice: i32,
    env: Vec<(String, String)>,
    /// The agent inside, from its SessionStart hook; its SessionEnd hook clears it.
    agent: Mutex<Option<AgentRecord>>,
    recovered: bool,
```

In `spawn_inner`: the resolved `shell` string is stored (`shell: shell.clone()` after the `let shell = ...` line); `nice: spec.nice`, `env: spec.env.clone()` (the caller's extra variables only, not `TERM` and the id), `agent: Mutex::new(spec.agent)`, `recovered: spec.recovered`, and the ring starts as `RingBuffer::from_bytes(spec.replay_capacity, &spec.seed)`. In `adopt`, until Task 2 adds the fields to the record, set `shell: String::new()`, `nice: 0`, `env: Vec::new()`, `agent: Mutex::new(None)`, `recovered: false` (Task 2 replaces these with the record's).

Methods (next to `info`):

```rust
    pub fn shell(&self) -> &str {
        &self.shell
    }

    pub fn nice(&self) -> i32 {
        self.nice
    }

    pub fn env(&self) -> &[(String, String)] {
        &self.env
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn set_agent(&self, agent: AgentRecord) {
        *self.agent.lock().unwrap() = Some(agent);
    }

    /// Clears the record when it is `id`'s. A SessionEnd hook that arrives after the next
    /// SessionStart (a `/clear` ends one session and starts another) must not erase the new one.
    pub fn clear_agent(&self, id: &str) -> bool {
        let mut a = self.agent.lock().unwrap();
        match &*a {
            Some(rec) if rec.id == id => {
                *a = None;
                true
            }
            _ => false,
        }
    }

    pub fn agent(&self) -> Option<AgentRecord> {
        self.agent.lock().unwrap().clone()
    }

    pub fn detail(&self) -> SessionDetail {
        SessionDetail { info: self.info(), recovered: self.recovered, agent: self.agent() }
    }
```

The `agent` lock is never taken while `state` or `child` is held, and vice versa: say so in a comment on the field.

- [ ] **Step 8: Run the session tests**

Run: `~/.cargo/bin/cargo test -p sarkosd --bin sarkosd`
Expected: PASS, every `SpawnSpec` literal in the crate updated (grep `SpawnSpec {` in `server.rs` and `session.rs`: the `CreateSession` arm sets `seed: Vec::new(), recovered: false, agent: None`).

- [ ] **Step 9: Write the failing daemon integration tests**

Append to `crates/sarkosd/tests/daemon.rs` (add `AgentRecord` to the `use protocol::{...}` line):

```rust
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
    assert!(matches!(r, Response::Hello { protocol_min: 1, protocol_max: 3, .. }), "{r:?}");
    assert_eq!(c.request(Request::Goodbye), Response::Ok);
    assert_eq!(c.request(Request::Goodbye), Response::Ok, "a second Goodbye is harmless");
}
```

(`Daemon::connect` already sends `PROTOCOL_VERSION`, now 3; the existing `hello_version_1_gets_ok_and_version_2_gets_the_daemon_version` and `hello_with_a_newer_protocol_is_refused_with_the_pkill_hint` tests use literal versions: update the "newer" test to send `PROTOCOL_VERSION + 1` if it does not already, and the expected range text in any assertion that spells out "1 to 2".)

- [ ] **Step 10: Run them to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkosd --test daemon -- set_agent hello_version_3`
Expected: FAIL: the daemon answers `Error` or drops the client (unknown request, or `dispatch` is not exhaustive and the crate does not compile).

- [ ] **Step 11: Add the state and the arms**

In `crates/sarkosd/src/server.rs`:

`State` gains:

```rust
    /// Project folders with a window open, and the window binary each said it runs: what a
    /// restart reopens (update-in-place spec, section 11 step 4). Fed by Hello, emptied by
    /// Goodbye; a window that vanishes without one (killed by a shutdown, or crashed) stays.
    pub windows: HashMap<PathBuf, Option<PathBuf>>,
    /// Set by every change the manifest records; the flusher writes it and clears this.
    pub manifest_dirty: bool,
```

A helper above `dispatch`, used by `RelaunchClient` (replace its inline check) and later by recovery:

```rust
/// A regular file named `sarkos` owned by this user: the only binary the daemon launches as a
/// window, whoever named it.
pub fn is_sarkos_binary(p: &Path) -> bool {
    let named = p.file_name().map(|n| n == "sarkos").unwrap_or(false);
    // SAFETY: getuid takes no arguments and cannot fail.
    named && std::fs::metadata(p).map(|m| m.is_file() && m.uid() == unsafe { libc::getuid() }).unwrap_or(false)
}
```

In the `Hello` arm, after the `if let Some(rec) = st.clients.get_mut(&client_id) { ... }` block (so the borrow of `clients` has ended), record the window:

```rust
            let window = st.clients.get(&client_id).and_then(|r| r.ui_cwd.clone().map(|cwd| (cwd, r.ui_exe.clone())));
            if let Some((cwd, exe)) = window {
                st.windows.insert(cwd, exe);
                st.manifest_dirty = true;
            }
```

New arms, after `RelaunchClient`:

```rust
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
```

`CreateSession`'s success path sets `st.manifest_dirty = true` under the lock where it inserts the session. The reaper's removal (`reaper.rs`, `st.sessions.remove(&id)`) sets `st.manifest_dirty = true` too. `run_resumed` leaves `windows` empty for now (Task 2 fills it from the handover).

- [ ] **Step 12: Run the daemon suite**

Run: `~/.cargo/bin/cargo test -p sarkosd --test daemon`
Expected: PASS (including the two new tests).

- [ ] **Step 13: Commit**

```bash
git add crates/protocol/src/lib.rs crates/sarkosd/src/session.rs crates/sarkosd/src/server.rs crates/sarkosd/src/reaper.rs crates/sarkosd/tests/daemon.rs
git commit -m "Speak protocol 3: the agent record, the detailed list, Goodbye and the window list"
```

---

### Task 2: Handover format 2 that still reads format 1

**Files:**
- Modify: `crates/sarkosd/src/handover.rs`, `crates/sarkosd/src/upgrade.rs`, `crates/sarkosd/src/session.rs` (`snapshot`, `adopt`), `crates/sarkosd/src/server.rs` (`run_resumed`, `Options`), `crates/sarkosd/src/main.rs` (`--check-handover`, `--flush-quiet-millis`)
- Test: unit tests in `handover.rs`, `upgrade.rs`, `session.rs`; `crates/sarkosd/tests/daemon.rs` (the upgrade tests must stay green)

**Interfaces:**
- Consumes: Task 1's `Session` fields and `State.windows`.
- Produces: `handover::FORMAT = 2`, `handover::OLDEST_READ = 1`, `handover::atomic_write(path, body) -> io::Result<()>`, `SessionRecord { .., shell, nice, env, agent, recovered }`, `Handover { .., flush_quiet_millis, windows: Vec<(PathBuf, Option<PathBuf>)> }`, `upgrade::accepts_check(answer: &str) -> bool`, `Options.flush_quiet: Duration`.

- [ ] **Step 1: Write the failing handover tests**

In `crates/sarkosd/src/handover.rs`'s `mod tests` add:

```rust
    /// A file written by the current main (format 1) resumes under this daemon with defaults for
    /// what it lacks: "Update now" from that build must work without a pkill.
    #[test]
    fn a_format_1_file_is_read_with_defaults_for_the_new_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h.bin");
        let v1 = HandoverV1 {
            format: 1,
            written_by: "sarkosd 0.1.0 (old)".into(),
            listener_fd: 5,
            replay_capacity: 1024,
            watchdog_secs: 30,
            reaper_millis: 1000,
            dead_session_secs: 600,
            client_pending_cap: 1 << 20,
            high_water: 1 << 16,
            low_water: 1 << 15,
            stall_secs: 10,
            next_session: 42,
            sessions: vec![SessionRecordV1 {
                id: 9,
                cwd: "/p".into(),
                pid: 123,
                alive: true,
                exit_code: None,
                exited_ago_ms: None,
                claude: ClaudeState::Working,
                last_activity_unix_ms: 7,
                master_fd: 6,
                rows: 24,
                cols: 80,
                kill_requested: false,
                replay: b"abc".to_vec(),
            }],
        };
        let body = postcard::to_allocvec(&v1).unwrap();
        atomic_write(&path, &[MAGIC.as_slice(), &body].concat()).unwrap();
        let h = read(&path).unwrap();
        assert_eq!(h.format, FORMAT);
        assert_eq!(h.written_by, "sarkosd 0.1.0 (old)");
        assert_eq!(h.next_session, 42);
        assert_eq!(h.flush_quiet_millis, 2000);
        assert!(h.windows.is_empty());
        let s = &h.sessions[0];
        assert_eq!((s.id, s.pid, s.master_fd, s.rows, s.cols), (9, 123, 6, 24, 80));
        assert_eq!(s.claude, ClaudeState::Working);
        assert_eq!(s.replay, b"abc");
        assert_eq!(s.shell, "");
        assert_eq!(s.nice, 0);
        assert!(s.env.is_empty());
        assert_eq!(s.agent, None);
        assert!(!s.recovered);
    }

    #[test]
    fn a_format_2_file_round_trips_with_the_new_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h.bin");
        let agent = protocol::AgentRecord { kind: "claude".into(), id: "a1".into(), resume: vec!["claude".into(), "--resume".into(), "a1".into()] };
        let h = Handover {
            format: FORMAT,
            written_by: "me".into(),
            listener_fd: 5,
            replay_capacity: 1024,
            watchdog_secs: 30,
            reaper_millis: 1000,
            dead_session_secs: 600,
            client_pending_cap: 1 << 20,
            high_water: 1 << 16,
            low_water: 1 << 15,
            stall_secs: 10,
            flush_quiet_millis: 250,
            next_session: 42,
            windows: vec![("/p".into(), Some("/opt/homebrew/bin/sarkos".into())), ("/q".into(), None)],
            sessions: vec![SessionRecord {
                id: 9,
                cwd: "/p".into(),
                pid: 123,
                alive: true,
                exit_code: None,
                exited_ago_ms: None,
                claude: ClaudeState::Idle,
                last_activity_unix_ms: 7,
                master_fd: 6,
                rows: 24,
                cols: 80,
                kill_requested: false,
                replay: b"abc".to_vec(),
                shell: "/bin/zsh".into(),
                nice: 5,
                env: vec![("A".into(), "B".into())],
                agent: Some(agent),
                recovered: true,
            }],
        };
        write(&path, &h).unwrap();
        assert_eq!(read(&path).unwrap(), h);
    }

    #[test]
    fn an_unknown_format_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h.bin");
        let body = postcard::to_allocvec(&(FORMAT + 1)).unwrap();
        atomic_write(&path, &[MAGIC.as_slice(), &body].concat()).unwrap();
        let e = read(&path).unwrap_err();
        assert!(e.contains(&format!("format {}", FORMAT + 1)), "{e}");
    }
```

(Keep the existing tests; one of them may build a `Handover` or `SessionRecord` literal: add the new fields there.)

- [ ] **Step 2: Run them to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkosd --bin sarkosd -- handover`
Expected: FAIL to compile: `HandoverV1`, `atomic_write`, `flush_quiet_millis` not found.

- [ ] **Step 3: Implement format 2 with a format 1 reader**

In `crates/sarkosd/src/handover.rs`:

```rust
pub const FORMAT: u32 = 2;
/// The oldest format `read` still decodes. `--check-handover` prints this, and a daemon accepts
/// a candidate whose oldest readable format is at most the format it writes.
pub const OLDEST_READ: u32 = 1;
```

`SessionRecord` gains, after `replay` (append, with doc comments): `pub shell: String`, `pub nice: i32`, `pub env: Vec<(String, String)>`, `pub agent: Option<AgentRecord>`, `pub recovered: bool`. `Handover` gains, after `stall_secs`: `pub flush_quiet_millis: u64`, and after `next_session`: `pub windows: Vec<(PathBuf, Option<PathBuf>)>` (doc: "Project folders with a window open and the binary each runs, carried so a restart after the swap still knows them").

Frozen copies of the format 1 structs, named `SessionRecordV1` and `HandoverV1`, with exactly the fields the current file has (copy them from git: `git show HEAD:crates/sarkosd/src/handover.rs`), both `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]` and `pub(crate)`, with a comment: "Format 1, as the current main writes it. Never edited: a format 1 file must decode with this exact shape."

```rust
impl From<HandoverV1> for Handover {
    fn from(v: HandoverV1) -> Self {
        Handover {
            format: FORMAT,
            written_by: v.written_by,
            listener_fd: v.listener_fd,
            replay_capacity: v.replay_capacity,
            watchdog_secs: v.watchdog_secs,
            reaper_millis: v.reaper_millis,
            dead_session_secs: v.dead_session_secs,
            client_pending_cap: v.client_pending_cap,
            high_water: v.high_water,
            low_water: v.low_water,
            stall_secs: v.stall_secs,
            flush_quiet_millis: 2000,
            next_session: v.next_session,
            windows: Vec::new(),
            sessions: v
                .sessions
                .into_iter()
                .map(|s| SessionRecord {
                    id: s.id,
                    cwd: s.cwd,
                    pid: s.pid,
                    alive: s.alive,
                    exit_code: s.exit_code,
                    exited_ago_ms: s.exited_ago_ms,
                    claude: s.claude,
                    last_activity_unix_ms: s.last_activity_unix_ms,
                    master_fd: s.master_fd,
                    rows: s.rows,
                    cols: s.cols,
                    kill_requested: s.kill_requested,
                    replay: s.replay,
                    shell: String::new(),
                    nice: 0,
                    env: Vec::new(),
                    agent: None,
                    recovered: false,
                })
                .collect(),
        }
    }
}
```

Factor the temp-file write out of `write` into:

```rust
/// Writes `body` to a sibling temp file with mode 0600, syncs it and renames it over `path`, so
/// a crash mid-write leaves nothing a reader could misread. The manifest uses it too.
pub fn atomic_write(path: &Path, body: &[u8]) -> io::Result<()> {
```

(the existing code, taking `body` instead of encoding `h`), and `write` becomes `atomic_write(path, &[MAGIC.as_slice(), &postcard::to_allocvec(h).map_err(io::Error::other)?].concat())`. `read` dispatches on the format:

```rust
    match format {
        FORMAT => postcard::from_bytes(body).map_err(decode_err),
        1 => postcard::from_bytes::<HandoverV1>(body).map(Handover::from).map_err(decode_err),
        other => Err(format!("handover format {other} is not one this sarkosd reads ({OLDEST_READ} to {FORMAT})")),
    }
```

`crates/sarkosd/src/main.rs`: `--check-handover` prints `handover {}` with `handover::OLDEST_READ`; add `--flush-quiet-millis <n>` (default 2000, into `Options.flush_quiet`). `Options` gains `pub flush_quiet: Duration` (doc: "How long output must have stopped before a session's ring is written to disk"); `run_resumed` reads it from `h.flush_quiet_millis` and sets `st.windows = h.windows.into_iter().collect(); st.manifest_dirty = true;`. `upgrade::execute` fills `flush_quiet_millis: opts.flush_quiet.as_millis() as u64` and `windows` from `State.windows` (gather it under the same lock as the session list: extend the tuple the loop breaks with).

`session.rs`: `snapshot` fills `shell: self.shell.clone(), nice: self.nice, env: self.env.clone(), agent: self.agent(), recovered: self.recovered` (take the `agent` lock before the `state` lock is taken, or after it is dropped: never nested; simplest is `let agent = self.agent();` as the first line). `adopt` takes them from the record (`shell: record.shell.clone()` and so on, `agent: Mutex::new(record.agent.clone())`). Extend `snapshot_records_the_session_and_adopt_rebuilds_it` to set an agent and `recovered: true` and assert both survive `snapshot` then `adopt`.

- [ ] **Step 4: Write the failing validate test**

In `crates/sarkosd/src/upgrade.rs`'s `mod tests`:

```rust
    #[test]
    fn a_candidate_is_accepted_when_its_oldest_readable_format_is_not_newer_than_ours() {
        assert!(accepts_check("handover 1\n"), "the current main's binary, which reads only 1, is accepted by nobody newer; this binary prints 1 too");
        assert!(accepts_check(&format!("handover {}", handover::FORMAT)));
        assert!(!accepts_check(&format!("handover {}", handover::FORMAT + 1)), "a future binary that dropped our format cannot resume our file");
        assert!(!accepts_check("handover"));
        assert!(!accepts_check("something else"));
        assert!(!accepts_check(""));
    }
```

- [ ] **Step 5: Run it to see it fail**

Run: `~/.cargo/bin/cargo test -p sarkosd --bin sarkosd -- accepts_check`
Expected: FAIL to compile: `accepts_check` not found.

- [ ] **Step 6: Implement the check**

In `upgrade.rs`:

```rust
/// What `--check-handover` must answer: `handover N`, the oldest format the candidate reads,
/// with N at most the format this daemon writes.
pub fn accepts_check(answer: &str) -> bool {
    answer.trim().strip_prefix("handover ").and_then(|n| n.trim().parse::<u32>().ok()).is_some_and(|oldest| oldest <= handover::FORMAT)
}
```

and `validate` uses it:

```rust
    if !accepts_check(&out) {
        return Err(format!(
            "{} --check-handover answered {:?}; this daemon writes handover format {} and needs a binary that reads it",
            exe.display(),
            out.trim(),
            handover::FORMAT
        ));
    }
```

- [ ] **Step 7: Run the whole daemon crate**

Run: `~/.cargo/bin/cargo test -p sarkosd` then `for i in $(seq 10); do ~/.cargo/bin/cargo test -p sarkosd --test daemon 2>&1 | grep "test result" || break; done`
Expected: PASS; `version_and_check_handover_flags` still sees `handover 1` (now `OLDEST_READ`); the upgrade tests still pass (the fake binaries that print `handover 1` are accepted; the one that prints something else is refused).

- [ ] **Step 8: Commit**

```bash
git add crates/sarkosd/src/handover.rs crates/sarkosd/src/upgrade.rs crates/sarkosd/src/session.rs crates/sarkosd/src/server.rs crates/sarkosd/src/main.rs
git commit -m "Write handover format 2 and still read format 1, so the update into this build needs no pkill"
```

---

### Task 3: The hook CLI: `sarkos notify` and `sarkos hooks`

**Files:**
- Create: `crates/sarkos/src/hook_cli.rs`
- Modify: `crates/sarkos/src/main.rs` (`mod hook_cli;` and the subcommand route at the top of `fn main`), `crates/sarkos/src/update.rs` (nothing; `launch_path()` is reused)
- Test: unit tests in `hook_cli.rs` (a fake daemon on a temp socket)

**Interfaces:**
- Consumes: Task 1's `Request::{SetAgent, ClearAgent}` and `AgentRecord`; `protocol::codec::{read_frame, write_frame}`; `paths::socket_path()`; `update::launch_path()`.
- Produces: `hook_cli::run(args: &[String]) -> i32` (the whole subcommand), `hook_cli::install_hooks(exe: &Path) -> Result<(PathBuf, usize, usize), String>` (path, added, replaced; the palette calls it in Task 7), `hook_cli::hooks_json(exe: &Path) -> serde_json::Value`, `hook_cli::claude_settings_path() -> PathBuf`.

- [ ] **Step 1: Write the failing tests**

Create `crates/sarkos/src/hook_cli.rs` with the module doc and the tests first:

```rust
//! The hook side of session awareness (spec 5 and 2.3) and of recovery (update-in-place spec,
//! section 11): `sarkos notify`, run by Claude Code's hooks with `SARKOS_SESSION_ID` in its
//! environment, sends the daemon one request and exits 0 whatever happens; `sarkos hooks` prints
//! or installs the hook entries. No gpui here: this runs before the window exists.

use protocol::codec::{read_frame, write_frame};
use protocol::{AgentRecord, ClaudeState, ClientMessage, Request, Response, ServerMessage, SessionId};
use std::io::{BufReader, BufWriter, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn notify_arguments_name_a_state_or_an_event() {
        assert_eq!(parse_notify(&args("notify --state working")), Ok(Notify::State(ClaudeState::Working)));
        assert_eq!(parse_notify(&args("notify --state waiting")), Ok(Notify::State(ClaudeState::Waiting)));
        assert_eq!(parse_notify(&args("notify --state idle")), Ok(Notify::State(ClaudeState::Idle)));
        assert_eq!(parse_notify(&args("notify --event session-start")), Ok(Notify::Event(HookEvent::SessionStart)));
        assert_eq!(parse_notify(&args("notify --event session-end")), Ok(Notify::Event(HookEvent::SessionEnd)));
        assert!(parse_notify(&args("notify --state busy")).is_err());
        assert!(parse_notify(&args("notify")).is_err());
        assert!(parse_notify(&args("notify --event")).is_err());
    }

    #[test]
    fn hook_stdin_gives_the_agents_session_id() {
        let json = r#"{"session_id":"abc123","transcript_path":"/t.jsonl","cwd":"/p","hook_event_name":"SessionStart","source":"resume"}"#;
        assert_eq!(parse_hook_input(json), Some(HookInput { session_id: "abc123".into() }));
        assert_eq!(parse_hook_input("not json"), None);
        assert_eq!(parse_hook_input(r#"{"cwd":"/p"}"#), None);
        assert_eq!(parse_hook_input(r#"{"session_id":""}"#), None);
    }

    #[test]
    fn the_resume_command_is_claude_resume_with_the_id() {
        assert_eq!(resume_command("claude", "abc"), vec!["claude", "--resume", "abc"]);
        let r = request_for(&Notify::Event(HookEvent::SessionStart), SessionId(7), Some(&HookInput { session_id: "abc".into() }));
        assert_eq!(r, Some(Request::SetAgent { session: SessionId(7), agent: AgentRecord { kind: "claude".into(), id: "abc".into(), resume: resume_command("claude", "abc") } }));
        assert_eq!(request_for(&Notify::Event(HookEvent::SessionEnd), SessionId(7), Some(&HookInput { session_id: "abc".into() })), Some(Request::ClearAgent { session: SessionId(7), id: "abc".into() }));
        assert_eq!(request_for(&Notify::Event(HookEvent::SessionStart), SessionId(7), None), None, "no stdin, nothing to record");
        assert_eq!(request_for(&Notify::State(ClaudeState::Waiting), SessionId(7), None), Some(Request::SetState { session: SessionId(7), state: ClaudeState::Waiting }));
    }

    /// A daemon stand-in: accepts one client, records its first request, answers Ok.
    fn fake_daemon(dir: &Path) -> (PathBuf, mpsc::Receiver<Request>) {
        let socket = dir.join("d.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let mut w = BufWriter::new(stream);
            if let Ok(Some(msg)) = read_frame::<_, ClientMessage>(&mut r) {
                tx.send(msg.request).unwrap();
                write_frame(&mut w, &ServerMessage::Response { id: msg.id, response: Response::Ok }).unwrap();
                w.flush().unwrap();
            }
        });
        (socket, rx)
    }

    #[test]
    fn notify_sends_one_request_for_the_session_in_the_environment() {
        let dir = tempfile::tempdir().unwrap();
        let (socket, rx) = fake_daemon(dir.path());
        let code = run_with(&args("notify --state waiting"), &socket, Some("7"), &mut std::io::empty());
        assert_eq!(code, 0);
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Request::SetState { session: SessionId(7), state: ClaudeState::Waiting });
    }

    #[test]
    fn a_session_start_hook_records_the_agent_from_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let (socket, rx) = fake_daemon(dir.path());
        let mut stdin = r#"{"session_id":"abc","cwd":"/p","hook_event_name":"SessionStart","source":"startup"}"#.as_bytes();
        assert_eq!(run_with(&args("notify --event session-start"), &socket, Some("7"), &mut stdin), 0);
        let agent = AgentRecord { kind: "claude".into(), id: "abc".into(), resume: vec!["claude".into(), "--resume".into(), "abc".into()] };
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Request::SetAgent { session: SessionId(7), agent });
    }

    #[test]
    fn notify_without_a_session_id_exits_zero_and_sends_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (socket, rx) = fake_daemon(dir.path());
        assert_eq!(run_with(&args("notify --state working"), &socket, None, &mut std::io::empty()), 0);
        assert_eq!(run_with(&args("notify --state working"), &socket, Some("not a number"), &mut std::io::empty()), 0);
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err(), "nothing reached the daemon");
    }

    #[test]
    fn notify_without_a_daemon_or_with_bad_arguments_exits_zero_quickly() {
        let dir = tempfile::tempdir().unwrap();
        let started = std::time::Instant::now();
        assert_eq!(run_with(&args("notify --state working"), &dir.path().join("missing.sock"), Some("7"), &mut std::io::empty()), 0);
        assert_eq!(run_with(&args("notify --state nonsense"), &dir.path().join("missing.sock"), Some("7"), &mut std::io::empty()), 0);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn the_hook_entries_cover_the_six_events_with_the_notification_matcher() {
        let v = hooks_json(Path::new("/opt/homebrew/bin/sarkos"));
        let hooks = v["hooks"].as_object().unwrap();
        let mut events: Vec<&String> = hooks.keys().collect();
        events.sort();
        assert_eq!(events, ["Notification", "PreToolUse", "SessionEnd", "SessionStart", "Stop", "UserPromptSubmit"]);
        assert_eq!(hooks["Notification"][0]["matcher"], "permission_prompt|idle_prompt");
        assert_eq!(hooks["Notification"][0]["hooks"][0]["command"], "/opt/homebrew/bin/sarkos notify --state waiting");
        assert_eq!(hooks["Stop"][0]["hooks"][0]["command"], "/opt/homebrew/bin/sarkos notify --state waiting");
        assert_eq!(hooks["UserPromptSubmit"][0]["hooks"][0]["command"], "/opt/homebrew/bin/sarkos notify --state working");
        assert_eq!(hooks["PreToolUse"][0]["hooks"][0]["command"], "/opt/homebrew/bin/sarkos notify --state working");
        assert!(hooks["PreToolUse"][0].get("matcher").is_none(), "every tool");
        assert_eq!(hooks["SessionStart"][0]["hooks"][0]["command"], "/opt/homebrew/bin/sarkos notify --event session-start");
        assert_eq!(hooks["SessionEnd"][0]["hooks"][0]["command"], "/opt/homebrew/bin/sarkos notify --event session-end");
        assert_eq!(hooks["Stop"][0]["hooks"][0]["timeout"], 5);
        let spaced = hooks_json(Path::new("/Users/me/My Apps/sarkos"));
        assert_eq!(spaced["hooks"]["Stop"][0]["hooks"][0]["command"], "'/Users/me/My Apps/sarkos' notify --state waiting");
    }

    #[test]
    fn merging_is_idempotent_replaces_an_old_path_and_keeps_other_hooks() {
        let ours = hooks_json(Path::new("/new/sarkos"));
        let existing: serde_json::Value = serde_json::from_str(
            r#"{"model":"opus","hooks":{"Stop":[
                {"hooks":[{"type":"command","command":"/old/sarkos notify --state waiting"}]},
                {"hooks":[{"type":"command","command":"say done"}]}
            ]}}"#,
        )
        .unwrap();
        let (merged, added, replaced) = merge_hooks(existing, &ours);
        assert_eq!((added, replaced), (6, 1));
        assert_eq!(merged["model"], "opus");
        let stop = merged["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0]["hooks"][0]["command"], "say done", "the other hook stays, first");
        assert_eq!(stop[1]["hooks"][0]["command"], "/new/sarkos notify --state waiting");
        let (again, added, replaced) = merge_hooks(merged.clone(), &ours);
        assert_eq!((added, replaced), (6, 6), "a second install replaces its own entries");
        assert_eq!(again, merged);
    }

    #[test]
    fn install_writes_a_backup_and_the_merged_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"model":"opus"}"#).unwrap();
        let (p, added, replaced) = install_hooks_at(&path, Path::new("/x/sarkos")).unwrap();
        assert_eq!((p, added, replaced), (path.clone(), 6, 0));
        assert_eq!(std::fs::read_to_string(dir.path().join("settings.json.bak")).unwrap(), r#"{"model":"opus"}"#);
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["model"], "opus");
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["command"], "/x/sarkos notify --state waiting");
        // A missing file is created; a broken one is left alone.
        let fresh = dir.path().join("sub").join("settings.json");
        assert_eq!(install_hooks_at(&fresh, Path::new("/x/sarkos")).unwrap().1, 6);
        std::fs::write(&path, "{ not json").unwrap();
        assert!(install_hooks_at(&path, Path::new("/x/sarkos")).unwrap_err().contains("cannot parse"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkos -- hook_cli`
Expected: FAIL to compile (add `mod hook_cli;` to `main.rs` first so the file is part of the crate): `parse_notify`, `run_with` not found.

- [ ] **Step 3: Implement the module**

Above the tests in `hook_cli.rs`:

```rust
/// How long one request to the daemon may take; a hook must never hold Claude Code up.
const TIMEOUT: Duration = Duration::from_secs(2);
/// A hook's stdin is a small JSON object; more than this is not one.
const STDIN_CAP: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    SessionStart,
    SessionEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notify {
    State(ClaudeState),
    Event(HookEvent),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookInput {
    pub session_id: String,
}

/// `notify --state <working|waiting|idle>` or `notify --event <session-start|session-end>`.
pub fn parse_notify(args: &[String]) -> Result<Notify, String> {
    let usage = "usage: sarkos notify --state <working|waiting|idle> | --event <session-start|session-end>";
    match (args.first().map(String::as_str), args.get(1).map(String::as_str), args.get(2).map(String::as_str)) {
        (Some("notify"), Some("--state"), Some("working")) => Ok(Notify::State(ClaudeState::Working)),
        (Some("notify"), Some("--state"), Some("waiting")) => Ok(Notify::State(ClaudeState::Waiting)),
        (Some("notify"), Some("--state"), Some("idle")) => Ok(Notify::State(ClaudeState::Idle)),
        (Some("notify"), Some("--event"), Some("session-start")) => Ok(Notify::Event(HookEvent::SessionStart)),
        (Some("notify"), Some("--event"), Some("session-end")) => Ok(Notify::Event(HookEvent::SessionEnd)),
        _ => Err(usage.to_string()),
    }
}

/// The fields of a hook's stdin this needs: Claude Code sends `session_id`, `transcript_path`,
/// `cwd` and `hook_event_name` on every event (its hooks reference); only the id is kept.
pub fn parse_hook_input(stdin: &str) -> Option<HookInput> {
    let v: serde_json::Value = serde_json::from_str(stdin).ok()?;
    let id = v.get("session_id")?.as_str()?.trim();
    (!id.is_empty()).then(|| HookInput { session_id: id.to_string() })
}

/// The command that resumes an agent's session, as argv. Claude Code: `claude --resume <id>`
/// works from any directory and never sends a message.
pub fn resume_command(kind: &str, id: &str) -> Vec<String> {
    match kind {
        "claude" => vec!["claude".into(), "--resume".into(), id.into()],
        other => vec![other.into(), "--resume".into(), id.into()],
    }
}

pub fn request_for(notify: &Notify, session: SessionId, input: Option<&HookInput>) -> Option<Request> {
    match notify {
        Notify::State(state) => Some(Request::SetState { session, state: *state }),
        Notify::Event(HookEvent::SessionStart) => {
            let id = input?.session_id.clone();
            Some(Request::SetAgent { session, agent: AgentRecord { kind: "claude".into(), resume: resume_command("claude", &id), id } })
        }
        Notify::Event(HookEvent::SessionEnd) => Some(Request::ClearAgent { session, id: input?.session_id.clone() }),
    }
}

/// One request on a fresh connection, with no Hello: the daemon must not take a hook for a
/// window. Events the daemon pushes meanwhile are skipped until the answer arrives.
pub fn send_one(socket: &Path, request: Request) -> std::io::Result<Response> {
    let stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    let mut w = BufWriter::new(stream.try_clone()?);
    let mut r = BufReader::new(stream);
    write_frame(&mut w, &ClientMessage { id: 1, request })?;
    w.flush()?;
    loop {
        match read_frame::<_, ServerMessage>(&mut r)? {
            Some(ServerMessage::Response { id: 1, response }) => return Ok(response),
            Some(_) => continue,
            None => return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "the daemon hung up without answering")),
        }
    }
}

/// `sarkos notify ...` with its inputs explicit, for tests. Always 0 for `notify`: a hook
/// that fails would show up in Claude Code as a hook error, for something that is only a tab
/// icon. Every reason is on stderr, which Claude Code only shows for a non-zero exit.
pub fn run_with(args: &[String], socket: &Path, session_env: Option<&str>, stdin: &mut dyn Read) -> i32 {
    let notify = match parse_notify(args) {
        Ok(n) => n,
        Err(usage) => {
            eprintln!("sarkos: {usage}");
            return 0;
        }
    };
    let Some(session) = session_env.and_then(|s| s.trim().parse::<u64>().ok()).map(SessionId) else {
        // Started outside Sarkos IDE: nothing to tell anyone.
        return 0;
    };
    let input = match notify {
        Notify::Event(_) => {
            let mut text = String::new();
            let _ = stdin.take(STDIN_CAP).read_to_string(&mut text);
            parse_hook_input(&text)
        }
        Notify::State(_) => None,
    };
    let Some(request) = request_for(&notify, session, input.as_ref()) else {
        eprintln!("sarkos notify: the hook's stdin had no session_id; nothing recorded");
        return 0;
    };
    match send_one(socket, request) {
        Ok(Response::Ok) => {}
        Ok(Response::Error(e)) => eprintln!("sarkos notify: sarkosd answered: {e}"),
        Ok(other) => eprintln!("sarkos notify: unexpected answer {other:?}"),
        // Includes a daemon too old to decode SetAgent: it drops the client without an answer.
        Err(e) => eprintln!("sarkos notify: cannot reach sarkosd: {e}"),
    }
    0
}

/// Quoted for `sh` when the path needs it.
fn shell_quote(path: &Path) -> String {
    let s = path.to_string_lossy();
    if s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c)) {
        s.into_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// The settings block Claude Code needs (spec 5's table, plus the two lifecycle events the agent
/// record comes from). `exe` is this binary's launch path: through Homebrew's link it survives
/// upgrades; a build in a worktree needs `sarkos hooks install` again after moving.
pub fn hooks_json(exe: &Path) -> serde_json::Value {
    let cmd = |tail: &str| serde_json::json!({ "type": "command", "command": format!("{} notify {tail}", shell_quote(exe)), "timeout": 5 });
    serde_json::json!({
        "hooks": {
            "SessionStart": [{ "hooks": [cmd("--event session-start")] }],
            "SessionEnd": [{ "hooks": [cmd("--event session-end")] }],
            "UserPromptSubmit": [{ "hooks": [cmd("--state working")] }],
            "PreToolUse": [{ "hooks": [cmd("--state working")] }],
            "Notification": [{ "matcher": "permission_prompt|idle_prompt", "hooks": [cmd("--state waiting")] }],
            "Stop": [{ "hooks": [cmd("--state waiting")] }],
        }
    })
}

/// A matcher group that runs `sarkos notify`, whatever path it was installed from.
fn is_ours(group: &serde_json::Value) -> bool {
    group["hooks"].as_array().is_some_and(|hooks| {
        hooks.iter().any(|h| h["command"].as_str().is_some_and(|c| c.contains("sarkos notify") || c.contains("sarkos' notify")))
    })
}

/// Adds `ours` to `settings`, replacing every group of ours already there (an older path) and
/// leaving everyone else's groups first. Returns (settings, added, replaced).
pub fn merge_hooks(mut settings: serde_json::Value, ours: &serde_json::Value) -> (serde_json::Value, usize, usize) {
    if !settings.is_object() {
        settings = serde_json::json!({});
    }
    let (mut added, mut replaced) = (0, 0);
    let root = settings.as_object_mut().unwrap();
    let hooks = root.entry("hooks").or_insert_with(|| serde_json::json!({}));
    if !hooks.is_object() {
        *hooks = serde_json::json!({});
    }
    for (event, groups) in ours["hooks"].as_object().unwrap() {
        let slot = hooks.as_object_mut().unwrap().entry(event.clone()).or_insert_with(|| serde_json::json!([]));
        if !slot.is_array() {
            *slot = serde_json::json!([]);
        }
        let arr = slot.as_array_mut().unwrap();
        let before = arr.len();
        arr.retain(|g| !is_ours(g));
        replaced += before - arr.len();
        for g in groups.as_array().unwrap() {
            arr.push(g.clone());
            added += 1;
        }
    }
    (settings, added, replaced)
}

/// `$CLAUDE_CONFIG_DIR/settings.json`, else `~/.claude/settings.json` (Claude Code's user settings).
pub fn claude_settings_path() -> PathBuf {
    match std::env::var_os("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("settings.json"),
        None => PathBuf::from(std::env::var("HOME").expect("HOME is set")).join(".claude").join("settings.json"),
    }
}

/// Merges the hook entries into Claude Code's settings file, writing `<file>.bak` first when
/// the file exists. A file that does not parse is left untouched, with the error returned.
pub fn install_hooks_at(path: &Path, exe: &Path) -> Result<(PathBuf, usize, usize), String> {
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => {
            let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("cannot parse {}: {e}; fix it by hand first", path.display()))?;
            std::fs::write(path.with_extension("json.bak"), &text).map_err(|e| format!("cannot write the backup: {e}"))?;
            v
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let (merged, added, replaced) = merge_hooks(existing, &hooks_json(exe));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(&merged).map_err(|e| e.to_string())? + "\n";
    std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok((path.to_path_buf(), added, replaced))
}

pub fn install_hooks(exe: &Path) -> Result<(PathBuf, usize, usize), String> {
    install_hooks_at(&claude_settings_path(), exe)
}

fn this_exe() -> PathBuf {
    crate::update::launch_path().unwrap_or_else(|| PathBuf::from("sarkos"))
}

/// `sarkos notify ...` and `sarkos hooks print|install`. The exit code for the process.
pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("notify") => run_with(args, &crate::paths::socket_path(), std::env::var("SARKOS_SESSION_ID").ok().as_deref(), &mut std::io::stdin()),
        Some("hooks") => match args.get(1).map(String::as_str) {
            Some("print") => {
                println!("{}", serde_json::to_string_pretty(&hooks_json(&this_exe())).unwrap());
                0
            }
            Some("install") => match install_hooks(&this_exe()) {
                Ok((path, added, replaced)) => {
                    println!("installed {added} hook entries in {} (replaced {replaced}); restart claude for them to take effect", path.display());
                    0
                }
                Err(e) => {
                    eprintln!("sarkos hooks: {e}");
                    1
                }
            },
            _ => {
                eprintln!("usage: sarkos hooks print | install");
                2
            }
        },
        _ => 2,
    }
}
```

In `crates/sarkos/src/main.rs`, `fn main` starts with the subcommand route, before `--version`, QoS and everything gpui:

```rust
    let args: Vec<String> = std::env::args().collect();
    // The hook CLI: no window, no daemon start, no QoS. `claude` runs these on every event.
    if matches!(args.get(1).map(String::as_str), Some("notify") | Some("hooks")) {
        std::process::exit(hook_cli::run(&args[1..]));
    }
```

- [ ] **Step 4: Run the tests**

Run: `~/.cargo/bin/cargo test -p sarkos -- hook_cli`
Expected: PASS (nine tests). Then `~/.cargo/bin/cargo build --release -p sarkos 2>&1 | grep -i "^warning"` prints nothing for the crate.

- [ ] **Step 5: Try it against the real daemon, once, by hand**

With a daemon running (the window's, or `~/.cargo/bin/cargo run -p sarkosd -- --socket /tmp/t.sock` in another terminal), from a shell inside a Sarkos IDE tab (so `SARKOS_SESSION_ID` is set) run `target/release/sarkos notify --state waiting; echo $?` and expect `0` with nothing printed; then `echo '{"session_id":"abc"}' | target/release/sarkos notify --event session-start; echo $?`. Put the two lines and their output in the report. Outside a tab (no id) expect `0` at once.

- [ ] **Step 6: Commit**

```bash
git add crates/sarkos/src/hook_cli.rs crates/sarkos/src/main.rs
git commit -m "Add the hook CLI: sarkos notify sends a state or the agent record, sarkos hooks installs the entries"
```

---

### Task 4: Replay files, the manifest and the SIGTERM flush

**Files:**
- Create: `crates/sarkosd/src/recovery.rs`
- Modify: `crates/sarkosd/src/ring.rs` (`total`, `since`, `capacity`), `crates/sarkosd/src/session.rs` (dirty tracking, `flush_due`, `take_delta`, `replay_capacity`, `size` already exists), `crates/sarkosd/src/server.rs` (`serve` starts the flusher and the signal watcher; `next_session` setter), `crates/sarkosd/src/main.rs` (`mod recovery;`, block the signals first thing), `crates/sarkosd/tests/daemon.rs` (harness: `start_in`, `sigterm`, `sessions_dir`; two tests)
- Test: unit tests in `ring.rs`, `session.rs`, `recovery.rs`; integration tests

**Interfaces:**
- Consumes: Task 1's `Session::{shell, nice, env, agent, cwd}`, `State::{windows, manifest_dirty}`; Task 2's `atomic_write`, `Options.flush_quiet`.
- Produces: `recovery::{sessions_dir(socket), replay_path(dir, id), manifest_path(dir), Manifest, ManifestSession, ManifestWindow, MANIFEST_FORMAT, manifest_of(&State), write_manifest, read_manifest, Flusher, plan, start, block_termination_signals, watch_termination}`; `ring::RingBuffer::{total, since, capacity}`; `session::ReplayDelta`, `Session::{flush_due, take_delta, replay_capacity}`; `State::set_next_session`.

- [ ] **Step 1: Write the failing ring tests**

In `crates/sarkosd/src/ring.rs`'s `mod tests`:

```rust
    #[test]
    fn total_counts_every_byte_pushed_and_since_gives_the_unflushed_tail() {
        let mut r = RingBuffer::new(4);
        assert_eq!((r.total(), r.capacity()), (0, 4));
        r.push(b"ab");
        assert_eq!(r.total(), 2);
        assert_eq!(r.since(0), Some(b"ab".to_vec()));
        assert_eq!(r.since(1), Some(b"b".to_vec()));
        assert_eq!(r.since(2), Some(vec![]), "nothing new");
        r.push(b"cde");
        assert_eq!(r.total(), 5, "the dropped byte still counts");
        assert_eq!(r.since(2), Some(b"cde".to_vec()));
        assert_eq!(r.since(1), Some(b"bcde".to_vec()), "offset 1 is the oldest byte kept");
        assert_eq!(r.since(0), None, "offset 0 was dropped: rewrite from the snapshot");
        assert_eq!(r.since(9), None, "past the end is not a position in the stream");
        r.push(b"0123456789");
        assert_eq!(r.total(), 15);
        assert_eq!(r.since(11), Some(b"6789".to_vec()));
        let f = RingBuffer::from_bytes(8, b"abc");
        assert_eq!(f.total(), 3);
        assert_eq!(f.since(0), Some(b"abc".to_vec()));
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `~/.cargo/bin/cargo test -p sarkosd --bin sarkosd -- total_counts`
Expected: FAIL to compile: `total`, `since`, `capacity` not found.

- [ ] **Step 3: Implement them**

In `ring.rs`: a `total: u64` field (doc: "Bytes ever pushed, so a reader can name a position in the stream the ring may no longer hold"), incremented by the full length of every `push` before the tail is taken (`self.total += bytes.len() as u64;` as the first line), set to `bytes.len()` by `from_bytes` (it goes through `push`, so nothing to add). Methods:

```rust
    pub fn capacity(&self) -> usize {
        self.cap
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    /// The bytes at stream positions `from..total`, when the ring still holds them all; `None`
    /// when some were dropped (the caller writes the snapshot instead) or `from` is past the end.
    pub fn since(&self, from: u64) -> Option<Vec<u8>> {
        let oldest_kept = self.total - self.len as u64;
        if from < oldest_kept || from > self.total {
            return None;
        }
        let skip = (from - oldest_kept) as usize;
        Some(self.snapshot()[skip..].to_vec())
    }
```

Remove the `#[allow(dead_code)]` on `len` if the new code uses it; otherwise leave it.

- [ ] **Step 4: Write the failing session tests**

In `session.rs`'s `mod tests` (use `spawn_for_test(.., false)` so no reader records output; drive `on_output` yourself: it is private, so the test is in this module):

```rust
    #[test]
    fn output_makes_a_session_due_for_a_flush_and_take_delta_clears_it() {
        let dir = tempfile::tempdir().unwrap();
        let spec = SpawnSpec {
            id: SessionId(3),
            cwd: dir.path().to_path_buf(),
            shell: Some("/bin/sh".into()),
            env: vec![],
            nice: 0,
            rows: 24,
            cols: 80,
            replay_capacity: 8,
            backpressure: Backpressure::default(),
            seed: Vec::new(),
            recovered: false,
            agent: None,
        };
        let s = Session::spawn_for_test(spec, false).unwrap();
        assert_eq!(s.replay_capacity(), 8);
        assert!(!s.flush_due(Duration::ZERO, Duration::ZERO), "nothing was ever printed");
        s.on_output(b"abc");
        assert!(!s.flush_due(Duration::from_secs(60), Duration::from_secs(60)), "output is recent and the wait is young");
        assert!(s.flush_due(Duration::ZERO, Duration::from_secs(60)), "quiet for long enough");
        assert!(matches!(s.take_delta(None), ReplayDelta::Rewrite { total: 3, all } if all == b"abc"));
        assert!(!s.flush_due(Duration::ZERO, Duration::ZERO), "flushed");
        s.on_output(b"de");
        assert!(matches!(s.take_delta(Some(3)), ReplayDelta::Append { total: 5, bytes } if bytes == b"de"));
        s.on_output(b"0123456789");
        assert!(matches!(s.take_delta(Some(5)), ReplayDelta::Rewrite { total: 15, all } if all == b"23456789"), "the ring dropped unflushed bytes");
        assert!(matches!(s.take_delta(Some(15)), ReplayDelta::Append { total: 15, bytes } if bytes.is_empty()));
        let _ = s.kill();
    }

    #[test]
    fn a_seeded_ring_is_due_at_once_so_a_recovered_history_is_written_again() {
        let dir = tempfile::tempdir().unwrap();
        let spec = SpawnSpec {
            id: SessionId(4),
            cwd: dir.path().to_path_buf(),
            shell: Some("/bin/sh".into()),
            env: vec![],
            nice: 0,
            rows: 24,
            cols: 80,
            replay_capacity: 64,
            backpressure: Backpressure::default(),
            seed: b"old".to_vec(),
            recovered: true,
            agent: None,
        };
        let s = Session::spawn_for_test(spec, false).unwrap();
        assert!(s.flush_due(Duration::ZERO, Duration::from_secs(60)));
        let _ = s.kill();
    }
```

- [ ] **Step 5: Run them to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkosd --bin sarkosd -- flush_due a_seeded_ring`
Expected: FAIL to compile.

- [ ] **Step 6: Implement dirty tracking**

In `session.rs`: `SessionState` gains `dirty_since: Option<Instant>` (doc: "When output first arrived after the last flush to disk; `None` while the file is up to date") and `last_output: Instant`. `spawn_inner` sets `dirty_since: (!spec.seed.is_empty()).then(Instant::now)`, `last_output: Instant::now()`; `adopt` sets `dirty_since: Some(Instant::now())` (the new image rewrites the file once) and `last_output: Instant::now()`. `on_output` adds `st.last_output = Instant::now(); if st.dirty_since.is_none() { st.dirty_since = Some(st.last_output); }`. New items:

```rust
/// What the flusher writes: the bytes since its last offset, or the whole ring.
#[derive(Debug, PartialEq, Eq)]
pub enum ReplayDelta {
    Append { total: u64, bytes: Vec<u8> },
    Rewrite { total: u64, all: Vec<u8> },
}

impl Session {
    pub fn replay_capacity(&self) -> usize {
        self.state.lock().unwrap().replay.capacity()
    }

    /// Unflushed output that stopped `quiet` ago, or has been flowing for `max_wait`: a chatty
    /// session is still written every `max_wait`.
    pub fn flush_due(&self, quiet: Duration, max_wait: Duration) -> bool {
        let st = self.state.lock().unwrap();
        match st.dirty_since {
            None => false,
            Some(since) => st.last_output.elapsed() >= quiet || since.elapsed() >= max_wait,
        }
    }

    /// The bytes since stream position `from`, or the whole ring when `from` is `None` or the
    /// ring dropped some of them. Clears the dirty mark under the same lock, so output arriving
    /// after this call makes the session dirty again.
    pub fn take_delta(&self, from: Option<u64>) -> ReplayDelta {
        let mut st = self.state.lock().unwrap();
        st.dirty_since = None;
        let total = st.replay.total();
        match from.and_then(|f| st.replay.since(f)) {
            Some(bytes) => ReplayDelta::Append { total, bytes },
            None => ReplayDelta::Rewrite { total, all: st.replay.snapshot() },
        }
    }
}
```

- [ ] **Step 7: Run the session tests**

Run: `~/.cargo/bin/cargo test -p sarkosd --bin sarkosd -- session`
Expected: PASS.

- [ ] **Step 8: Write the failing recovery unit tests**

Create `crates/sarkosd/src/recovery.rs` with its doc comment and these tests (the implementation follows in Step 10):

```rust
//! Recovery after a restart (update-in-place spec, section 11): each session's replay ring goes
//! to `<sessions dir>/<id>.replay` a couple of seconds after output stops and in full on
//! SIGTERM, with a manifest of the sessions (folder, shell, agent record) and the windows open;
//! `--recover` (Task 5) rebuilds everything from them. The sessions dir sits next to the socket,
//! so tests get their own under their temp socket.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sessions_dir_and_file_names_sit_next_to_the_socket() {
        let dir = sessions_dir(Path::new("/x/sarkos/sarkosd.sock"));
        assert_eq!(dir, PathBuf::from("/x/sarkos/sessions"));
        assert_eq!(replay_path(&dir, SessionId(42)), PathBuf::from("/x/sarkos/sessions/42.replay"));
        assert_eq!(manifest_path(&dir), PathBuf::from("/x/sarkos/sessions/manifest.bin"));
    }

    #[test]
    fn a_flush_appends_while_the_file_is_sane_and_rewrites_otherwise() {
        assert_eq!(plan(None, 0, 1024), None, "first flush of this daemon life: rewrite");
        assert_eq!(plan(Some(0), 0, 1024), Some(0), "an empty file and nothing flushed yet: append from the start");
        assert_eq!(plan(Some(5), 100, 1024), Some(5));
        assert_eq!(plan(Some(5), 2048, 1024), Some(5), "twice the ring is still fine");
        assert_eq!(plan(Some(5), 2049, 1024), None, "past twice the ring: compact");
        assert_eq!(plan(Some(5), 0, 1024), None, "the file vanished under us: rewrite");
    }

    #[test]
    fn the_manifest_round_trips_and_a_missing_one_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_manifest(dir.path()), Ok(None));
        let m = Manifest {
            format: MANIFEST_FORMAT,
            written_by: "me".into(),
            sessions: vec![ManifestSession {
                id: 9,
                cwd: "/p".into(),
                shell: "/bin/zsh".into(),
                nice: 0,
                env: vec![],
                agent: Some(protocol::AgentRecord { kind: "claude".into(), id: "a".into(), resume: vec!["claude".into(), "--resume".into(), "a".into()] }),
                rows: 24,
                cols: 80,
            }],
            windows: vec![ManifestWindow { cwd: "/p".into(), exe: Some("/opt/homebrew/bin/sarkos".into()) }],
        };
        write_manifest(dir.path(), &m).unwrap();
        assert_eq!(read_manifest(dir.path()), Ok(Some(m)));
        std::fs::write(manifest_path(dir.path()), b"SKMFgarbage").unwrap();
        assert!(read_manifest(dir.path()).unwrap_err().contains("cannot decode"));
        std::fs::write(manifest_path(dir.path()), b"nope").unwrap();
        assert!(read_manifest(dir.path()).unwrap_err().contains("not a manifest"));
    }

    #[test]
    fn replay_files_are_private_and_appended() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join("sessions");
        let path = replay_path(&sessions, SessionId(1));
        write_replay(&path, b"abc", false).unwrap();
        write_replay(&path, b"de", true).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"abcde");
        write_replay(&path, b"xy", false).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"xy");
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(&sessions).unwrap().permissions().mode() & 0o777, 0o700);
    }
}
```

- [ ] **Step 9: Run them to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkosd --bin sarkosd -- recovery`
Expected: FAIL to compile (add `mod recovery;` to `main.rs` first).

- [ ] **Step 10: Implement the files, the manifest and the flusher**

In `recovery.rs`, above the tests:

```rust
use crate::server::{Shared, State};
use crate::session::{ReplayDelta, Session};
use protocol::{AgentRecord, SessionId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const MANIFEST_FORMAT: u32 = 1;
const MANIFEST_MAGIC: [u8; 4] = *b"SKMF";
/// A session printing without pause is still written this often.
pub const MAX_WAIT: Duration = Duration::from_secs(15);

pub fn sessions_dir(socket: &Path) -> PathBuf {
    socket.with_file_name("sessions")
}

pub fn replay_path(dir: &Path, id: SessionId) -> PathBuf {
    dir.join(format!("{}.replay", id.0))
}

pub fn manifest_path(dir: &Path) -> PathBuf {
    dir.join("manifest.bin")
}

/// One session as a restart recreates it: the folder, the shell and its priority, the extra
/// environment the window asked for, and the agent inside. Dead sessions are not listed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestSession {
    pub id: u64,
    pub cwd: PathBuf,
    /// Empty means the daemon's `$SHELL` (a session adopted from a format 1 handover).
    pub shell: String,
    pub nice: i32,
    pub env: Vec<(String, String)>,
    pub agent: Option<AgentRecord>,
    pub rows: u16,
    pub cols: u16,
}

/// A project folder with a window open, and the binary that window runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestWindow {
    pub cwd: PathBuf,
    pub exe: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub written_by: String,
    pub sessions: Vec<ManifestSession>,
    pub windows: Vec<ManifestWindow>,
}

/// The manifest for the table as it is: live sessions by id, windows by folder.
pub fn manifest_of(st: &State) -> Manifest {
    let mut sessions: Vec<ManifestSession> = st
        .sessions
        .values()
        .filter(|s| s.info().alive)
        .map(|s| {
            let (rows, cols) = s.size();
            ManifestSession { id: s.id.0, cwd: s.cwd().to_path_buf(), shell: s.shell().to_string(), nice: s.nice(), env: s.env().to_vec(), agent: s.agent(), rows, cols }
        })
        .collect();
    sessions.sort_by_key(|s| s.id);
    let mut windows: Vec<ManifestWindow> = st.windows.iter().map(|(cwd, exe)| ManifestWindow { cwd: cwd.clone(), exe: exe.clone() }).collect();
    windows.sort_by(|a, b| a.cwd.cmp(&b.cwd));
    Manifest { format: MANIFEST_FORMAT, written_by: crate::version_string(), sessions, windows }
}

fn ensure_dir(dir: &Path) -> io::Result<()> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub fn write_manifest(dir: &Path, m: &Manifest) -> io::Result<()> {
    ensure_dir(dir)?;
    let body = postcard::to_allocvec(m).map_err(io::Error::other)?;
    crate::handover::atomic_write(&manifest_path(dir), &[MANIFEST_MAGIC.as_slice(), &body].concat())
}

/// `Ok(None)` when there is no manifest; an error names what is wrong with one that exists.
pub fn read_manifest(dir: &Path) -> Result<Option<Manifest>, String> {
    let path = manifest_path(dir);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    if bytes.len() < MANIFEST_MAGIC.len() || bytes[..MANIFEST_MAGIC.len()] != MANIFEST_MAGIC {
        return Err(format!("{} is not a manifest (bad magic)", path.display()));
    }
    let body = &bytes[MANIFEST_MAGIC.len()..];
    let decode_err = |e: postcard::Error| format!("cannot decode {}: {e}", path.display());
    let (format, _) = postcard::take_from_bytes::<u32>(body).map_err(decode_err)?;
    if format != MANIFEST_FORMAT {
        return Err(format!("manifest format {format} is not the {MANIFEST_FORMAT} this sarkosd reads"));
    }
    postcard::from_bytes(body).map(Some).map_err(decode_err)
}

/// The ring's bytes as they are, mode 0600: appended while the file is sane, else rewritten.
pub fn write_replay(path: &Path, bytes: &[u8], append: bool) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        ensure_dir(dir)?;
    }
    let mut f = std::fs::OpenOptions::new().write(true).create(true).append(append).truncate(!append).mode(0o600).open(path)?;
    f.write_all(bytes)
}

/// Where the next flush starts: `Some(offset)` appends from there, `None` rewrites the whole
/// ring. A file past twice the ring is compacted; one that vanished is rewritten.
pub fn plan(offset: Option<u64>, file_len: u64, cap: usize) -> Option<u64> {
    match offset {
        Some(0) if file_len == 0 => Some(0),
        Some(o) if file_len > 0 && file_len <= 2 * cap as u64 => Some(o),
        _ => None,
    }
}

/// Writes rings and the manifest. One per daemon, shared by the periodic thread and the
/// SIGTERM watcher, which locks it so no two writers touch a file at once.
pub struct Flusher {
    dir: PathBuf,
    /// The stream position each session's file ends at, for this daemon life.
    offsets: HashMap<SessionId, u64>,
}

impl Flusher {
    pub fn new(dir: PathBuf) -> Self {
        Flusher { dir, offsets: HashMap::new() }
    }

    /// One pass: every session due (all of them when `force`), the files of sessions gone from
    /// the table, then the manifest when it changed (or `force`).
    pub fn tick(&mut self, state: &Shared, quiet: Duration, force: bool) {
        let sessions: Vec<Arc<Session>> = state.lock().unwrap().sessions.values().cloned().collect();
        for s in &sessions {
            if force || s.flush_due(quiet, MAX_WAIT) {
                self.flush_session(s);
            }
        }
        let gone: Vec<SessionId> = self.offsets.keys().filter(|id| !sessions.iter().any(|s| s.id == **id)).copied().collect();
        for id in gone {
            self.offsets.remove(&id);
            let _ = std::fs::remove_file(replay_path(&self.dir, id));
        }
        let manifest = {
            let mut st = state.lock().unwrap();
            if force || st.manifest_dirty {
                st.manifest_dirty = false;
                Some(manifest_of(&st))
            } else {
                None
            }
        };
        if let Some(m) = manifest {
            if let Err(e) = write_manifest(&self.dir, &m) {
                eprintln!("sarkosd: cannot write the manifest in {}: {e}", self.dir.display());
            }
        }
    }

    fn flush_session(&mut self, s: &Arc<Session>) {
        let path = replay_path(&self.dir, s.id);
        let file_len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let from = plan(self.offsets.get(&s.id).copied(), file_len, s.replay_capacity());
        let (total, bytes, append) = match s.take_delta(from) {
            ReplayDelta::Append { total, bytes } => (total, bytes, true),
            ReplayDelta::Rewrite { total, all } => (total, all, false),
        };
        if append && bytes.is_empty() {
            self.offsets.insert(s.id, total);
            return;
        }
        match write_replay(&path, &bytes, append) {
            Ok(()) => {
                self.offsets.insert(s.id, total);
            }
            Err(e) => {
                eprintln!("sarkosd: cannot write {}: {e}", path.display());
                // Next time, rewrite from the ring rather than append to a file of unknown state.
                self.offsets.remove(&s.id);
            }
        }
    }
}

/// The periodic flusher: a pass every half of `quiet` (at least 50 ms, at most a second).
pub fn start(state: Shared, dir: PathBuf, quiet: Duration) -> Arc<Mutex<Flusher>> {
    let flusher = Arc::new(Mutex::new(Flusher::new(dir)));
    let tick = (quiet / 2).clamp(Duration::from_millis(50), Duration::from_secs(1));
    let worker = flusher.clone();
    std::thread::Builder::new()
        .name("recovery-flusher".into())
        .spawn(move || loop {
            std::thread::sleep(tick);
            worker.lock().unwrap_or_else(|p| p.into_inner()).tick(&state, quiet, false);
        })
        .expect("spawn flusher");
    flusher
}

fn termination_set() -> libc::sigset_t {
    // SAFETY: sigemptyset and sigaddset only write the set they are given.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGTERM);
        libc::sigaddset(&mut set, libc::SIGHUP);
        set
    }
}

/// Blocks SIGTERM and SIGHUP in the calling thread. Called first thing in `main`, before any
/// other thread exists, so every thread inherits the mask and `watch_termination` alone takes
/// the signals with `sigwait`. Children started through `Command` get an empty mask from the
/// standard library, so shells see signals as before; the exec'd image blocks them again itself.
pub fn block_termination_signals() {
    let set = termination_set();
    // SAFETY: the set is initialised above; a null old-set pointer is allowed.
    unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()) };
}

/// On SIGTERM (what launchd sends at logout and shutdown) or SIGHUP: write every ring and the
/// manifest, then exit 0. Closing the masters sends the shells SIGHUP, as the shutdown would.
pub fn watch_termination(state: Shared, flusher: Arc<Mutex<Flusher>>, quiet: Duration) {
    std::thread::Builder::new()
        .name("termination".into())
        .spawn(move || {
            let set = termination_set();
            let mut sig: libc::c_int = 0;
            // SAFETY: the set is initialised and `sig` is a valid out-pointer.
            let rc = unsafe { libc::sigwait(&set, &mut sig) };
            if rc != 0 {
                eprintln!("sarkosd: sigwait failed ({rc}); a shutdown will not flush histories");
                return;
            }
            let name = if sig == libc::SIGTERM { "SIGTERM" } else { "SIGHUP" };
            eprintln!("sarkosd: {name}: writing every session's history and the manifest before exiting");
            flusher.lock().unwrap_or_else(|p| p.into_inner()).tick(&state, quiet, true);
            eprintln!("sarkosd: exiting");
            std::process::exit(0);
        })
        .expect("spawn termination watcher");
}
```

`server.rs`: `serve` starts them after the watchdog and reaper:

```rust
    let dir = crate::recovery::sessions_dir(&opts.socket);
    let flusher = crate::recovery::start(state.clone(), dir, opts.flush_quiet);
    crate::recovery::watch_termination(state.clone(), flusher, opts.flush_quiet);
```

and `State` gets `pub fn set_next_session(&mut self, v: u64) { self.next_session = v; }` (Task 5 uses it). `main.rs`: `crate::recovery::block_termination_signals();` is the first statement of `main`, before argument parsing (the `--check-handover` child never reaches a signal, and `--version` does not care).

- [ ] **Step 11: Run the unit tests**

Run: `~/.cargo/bin/cargo test -p sarkosd --bin sarkosd`
Expected: PASS.

- [ ] **Step 12: Write the failing integration tests**

In `crates/sarkosd/tests/daemon.rs`, the harness grows (`_dir` becomes `Option<tempfile::TempDir>`; `start` calls `start_in`):

```rust
impl Daemon {
    fn start(extra: &[&str]) -> Daemon {
        Daemon::start_in(tempfile::tempdir().unwrap(), extra)
    }

    /// A daemon whose socket, and so its sessions dir, is in `dir`: a later daemon can share it.
    fn start_in(dir: tempfile::TempDir, extra: &[&str]) -> Daemon {
        let socket = dir.path().join("d.sock");
        // (the existing body of `start`, with `_dir: Some(dir)`)
    }

    fn dir(&self) -> &std::path::Path {
        self._dir.as_ref().unwrap().path()
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
                assert!(status.success(), "sarkosd exited with {status} on SIGTERM");
                break;
            }
            assert!(Instant::now() < deadline, "sarkosd did not exit within 5 s of SIGTERM");
            std::thread::sleep(Duration::from_millis(20));
        }
        self._dir.take().unwrap()
    }
}
```

(`Drop` keeps working: `kill` and `wait` on an exited child are errors it already ignores.) Add `fn attach(&mut self, session: SessionId)` to `Conn` if no helper exists: `assert!(matches!(self.request(Request::Attach { session }), Response::Attached { .. }))`. Then:

```rust
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
    c.request(Request::Input { session: sid, bytes: b"i=0; while [ $i -lt 3000 ]; do echo line $i; i=$((i+1)); done; echo END-MARKER\n".to_vec() });
    c.wait_for_output(sid, "END-MARKER");
    assert!(!replay_file(&d, sid).exists(), "the periodic flush never ran");
    let dir = d.sigterm();
    let bytes = std::fs::read(dir.path().join("sessions").join(format!("{}.replay", sid.0))).expect("written on SIGTERM");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("line 2999"), "the last lines are there");
    assert!(text.rfind("END-MARKER").unwrap() > text.rfind("line 2999").unwrap());
    assert!(dir.path().join("sessions").join("manifest.bin").exists());
}
```

- [ ] **Step 13: Run them to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkosd --test daemon -- history_is_written sigterm_writes`
Expected: with Step 10 in place they pass; if either fails, the daemon side is wrong, not the test: fix the daemon. (TDD here is at the unit level; the integration tests pin the wiring in `serve` and `main`.)

- [ ] **Step 14: Run the suite ten times**

Run: `for i in $(seq 10); do ~/.cargo/bin/cargo test -p sarkosd --test daemon 2>&1 | grep "test result" || break; done`
Expected: ten `ok` lines.

- [ ] **Step 15: Commit**

```bash
git add crates/sarkosd/src/recovery.rs crates/sarkosd/src/ring.rs crates/sarkosd/src/session.rs crates/sarkosd/src/server.rs crates/sarkosd/src/main.rs crates/sarkosd/tests/daemon.rs
git commit -m "Write each session's history and a manifest to disk, in full on SIGTERM"
```

---

### Task 5: `sarkosd --recover`

**Files:**
- Modify: `crates/sarkosd/src/recovery.rs` (`recover`, `seed_for`, `tail`, `shell_line`, `DIVIDER`), `crates/sarkosd/src/server.rs` (`run_with`, `ALREADY_LISTENING`), `crates/sarkosd/src/main.rs` (`--recover`), `crates/sarkosd/tests/daemon.rs`
- Test: unit tests in `recovery.rs`; integration tests

**Interfaces:**
- Consumes: Task 4's manifest and files; Task 1's `SpawnSpec` fields, `is_sarkos_binary`; `watchdog::launch_ui`; `State::set_next_session`.
- Produces: `recovery::recover(state, dir, opts) -> Recovered { sessions, windows }`, `server::run_with(opts, recover: bool)`, `server::ALREADY_LISTENING`, the `--recover` flag.

- [ ] **Step 1: Write the failing unit tests**

In `recovery.rs`'s `mod tests`:

```rust
    #[test]
    fn tail_keeps_the_newest_bytes() {
        assert_eq!(tail(b"0123456789".to_vec(), 4), b"6789");
        assert_eq!(tail(b"abc".to_vec(), 4), b"abc");
        assert_eq!(tail(Vec::new(), 4), b"");
    }

    #[test]
    fn the_seed_is_the_old_history_then_the_divider_or_nothing() {
        assert_eq!(seed_for(Vec::new(), 1024), Vec::<u8>::new(), "a session with no history shows no divider");
        let seed = seed_for(b"old output".to_vec(), 1024);
        assert!(seed.starts_with(b"old output"));
        assert!(seed.ends_with(DIVIDER));
        assert!(String::from_utf8_lossy(DIVIDER).contains("resumed after restart"));
        let big = seed_for(vec![b'x'; 2000], 1024);
        assert_eq!(big.len(), 1024, "old history plus divider fit the ring");
        assert!(big.ends_with(DIVIDER));
    }

    #[test]
    fn shell_line_quotes_each_argument_and_ends_the_line() {
        let argv: Vec<String> = ["claude", "--resume", "ab-12"].iter().map(|s| s.to_string()).collect();
        assert_eq!(shell_line(&argv), b"'claude' '--resume' 'ab-12'\n");
        let odd: Vec<String> = vec!["echo".into(), "it's".into()];
        assert_eq!(shell_line(&odd), b"'echo' 'it'\\''s'\n");
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkosd --bin sarkosd -- recovery`
Expected: FAIL to compile.

- [ ] **Step 3: Implement recover**

In `recovery.rs`:

```rust
/// Dim "── resumed after restart ──" between the old history and the new shell. Written into
/// the ring, never parsed: the window's emulator draws it like any other output.
pub const DIVIDER: &[u8] = b"\r\n\x1b[2m\xe2\x94\x80\xe2\x94\x80 resumed after restart \xe2\x94\x80\xe2\x94\x80\x1b[0m\r\n";

pub fn tail(mut bytes: Vec<u8>, cap: usize) -> Vec<u8> {
    if bytes.len() > cap {
        bytes.drain(..bytes.len() - cap);
    }
    bytes
}

/// What a recovered ring starts with: the newest history that fits, then the divider.
pub fn seed_for(old: Vec<u8>, cap: usize) -> Vec<u8> {
    if old.is_empty() {
        return Vec::new();
    }
    let mut seed = tail(old, cap.saturating_sub(DIVIDER.len()));
    seed.extend_from_slice(DIVIDER);
    seed
}

/// One command line for `sh` or `zsh`: every argument single-quoted, then a newline. The only
/// thing recovery ever types into a shell.
pub fn shell_line(argv: &[String]) -> Vec<u8> {
    let mut line = argv.iter().map(|a| format!("'{}'", a.replace('\'', "'\\''"))).collect::<Vec<_>>().join(" ");
    line.push('\n');
    line.into_bytes()
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Recovered {
    pub sessions: usize,
    pub windows: usize,
}

/// Rebuilds the table from the manifest: each session as a new shell in its folder (the home
/// folder when that is gone) with its history under the divider and its agent's resume command
/// typed in; then a window for each project that had one. Files the manifest does not name are
/// removed. Runs once, after the socket is bound and before it is served.
pub fn recover(state: &Shared, dir: &Path, opts: &crate::server::Options) -> Recovered {
    let manifest = match read_manifest(dir) {
        Ok(Some(m)) => m,
        Ok(None) => {
            eprintln!("sarkosd: no manifest in {}: nothing to recover", dir.display());
            return Recovered::default();
        }
        Err(e) => {
            eprintln!("sarkosd: cannot read the manifest: {e}; starting without it");
            let _ = std::fs::remove_file(manifest_path(dir));
            return Recovered::default();
        }
    };
    eprintln!("sarkosd: recovering {} sessions and {} windows from a manifest written by {}", manifest.sessions.len(), manifest.windows.len(), manifest.written_by);
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()));
    let mut done = Recovered::default();
    for m in &manifest.sessions {
        let id = SessionId(m.id);
        let old = std::fs::read(replay_path(dir, id)).unwrap_or_default();
        let cwd = if m.cwd.is_dir() {
            m.cwd.clone()
        } else {
            eprintln!("sarkosd: session {}: {} is gone, recovering it in {}", m.id, m.cwd.display(), home.display());
            home.clone()
        };
        let spec = crate::session::SpawnSpec {
            id,
            cwd,
            shell: (!m.shell.is_empty()).then(|| m.shell.clone()),
            env: m.env.clone(),
            nice: m.nice,
            rows: m.rows,
            cols: m.cols,
            replay_capacity: opts.replay_capacity,
            backpressure: opts.backpressure,
            seed: seed_for(old, opts.replay_capacity),
            recovered: true,
            agent: m.agent.clone(),
        };
        match Session::spawn(spec) {
            Ok(s) => {
                if let Some(agent) = &m.agent {
                    if let Err(e) = s.write(&shell_line(&agent.resume)) {
                        eprintln!("sarkosd: session {}: cannot type the resume command: {e:#}", m.id);
                    }
                }
                let mut st = state.lock().unwrap();
                if st.next_session() <= m.id {
                    st.set_next_session(m.id + 1);
                }
                st.sessions.insert(id, s);
                done.sessions += 1;
            }
            Err(e) => eprintln!("sarkosd: session {}: cannot recreate its shell: {e:#}", m.id),
        }
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let stale = name
                .to_string_lossy()
                .strip_suffix(".replay")
                .and_then(|n| n.parse::<u64>().ok())
                .is_some_and(|id| !manifest.sessions.iter().any(|m| m.id == id));
            if stale {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    for w in &manifest.windows {
        let Some(exe) = &w.exe else {
            eprintln!("sarkosd: window for {}: no binary recorded, not reopened", w.cwd.display());
            continue;
        };
        if !w.cwd.is_dir() {
            eprintln!("sarkosd: window for {}: the folder is gone, not reopened", w.cwd.display());
            continue;
        }
        if !crate::server::is_sarkos_binary(exe) {
            eprintln!("sarkosd: window for {}: {} is not a sarkos binary owned by this user, not reopened", w.cwd.display(), exe.display());
            continue;
        }
        match crate::watchdog::launch_ui(exe, Some(&w.cwd)) {
            Ok(()) => {
                state.lock().unwrap().windows.insert(w.cwd.clone(), Some(exe.clone()));
                done.windows += 1;
            }
            Err(e) => eprintln!("sarkosd: window for {}: cannot start {}: {e}", w.cwd.display(), exe.display()),
        }
    }
    state.lock().unwrap().manifest_dirty = true;
    done
}
```

`server.rs`:

```rust
/// How `run` fails when a daemon already serves the socket; `--recover` treats it as success.
pub const ALREADY_LISTENING: &str = "another sarkosd is already listening on";
```

(the existing `bail!` uses this constant), and `run` becomes `run_with(opts, false)`:

```rust
pub fn run(opts: Options) -> Result<()> {
    run_with(opts, false)
}

/// `recover`: rebuild the table from the manifest before serving (the launchd agent's start).
pub fn run_with(opts: Options, recover: bool) -> Result<()> {
    // (everything `run` did up to `serve`, then:)
    let state: Shared = Arc::new(Mutex::new(st));
    if recover {
        let dir = crate::recovery::sessions_dir(&opts.socket);
        let done = crate::recovery::recover(&state, &dir, &opts);
        eprintln!("sarkosd: recovered {} sessions and reopened {} windows", done.sessions, done.windows);
    }
    serve(listener, state, opts)
}
```

`main.rs`: a `--recover` flag (`let mut recover = false;`), and at the end:

```rust
    if let Err(e) = server::run_with(opts, recover) {
        if recover && e.to_string().contains(server::ALREADY_LISTENING) {
            eprintln!("sarkosd: already running; nothing to recover");
            return;
        }
        eprintln!("sarkosd: {e:#}");
        std::process::exit(1);
    }
```

- [ ] **Step 4: Run the unit tests**

Run: `~/.cargo/bin/cargo test -p sarkosd --bin sarkosd -- recovery`
Expected: PASS.

- [ ] **Step 5: Write the integration tests**

Append to `crates/sarkosd/tests/daemon.rs`:

```rust
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
    let agent = AgentRecord { kind: "test".into(), id: "t1".into(), resume: vec!["echo".into(), "RESUMED t1".into()] };
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
    if !text.contains("RESUMED t1") {
        text.push_str(&String::from_utf8_lossy(&c.wait_for_output(sid, "RESUMED t1")));
    }
    assert!(text.find("RESUMED t1").unwrap() > divider, "the typed resume command ran in the new shell");
    let newer = c.create("/bin/sh");
    assert!(newer.0 > sid.0, "ids stay unique after a recovery");
}

#[test]
fn recover_reopens_windows_that_were_open_and_not_those_that_said_goodbye() {
    let a = Daemon::start(&["--flush-quiet-millis", "100"]);
    let (script, marker) = relaunch_script(a.dir());
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
    while !marker.exists() {
        assert!(Instant::now() < deadline, "the window for the kept project was not reopened");
        std::thread::sleep(Duration::from_millis(20));
    }
    let want = keep.path().canonicalize().unwrap();
    assert_eq!(std::fs::read_to_string(&marker).unwrap().trim(), want.to_string_lossy());
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(std::fs::read_to_string(&marker).unwrap().trim(), want.to_string_lossy(), "the project that said goodbye stays closed");
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
    std::thread::sleep(Duration::from_millis(400));
    assert!(a.sessions_dir().join("manifest.bin").exists());
    drop(folder);
    let dir = a.sigterm();
    let b = Daemon::start_in(dir, &["--recover"]);
    let mut c = b.connect();
    let list = detailed(&mut c);
    assert_eq!(list.len(), 1);
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
fn recover_while_a_daemon_is_listening_exits_zero_and_leaves_it() {
    let a = Daemon::start(&[]);
    let mut second = Command::new(env!("CARGO_BIN_EXE_sarkosd")).arg("--socket").arg(&a.socket).arg("--recover").stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(s) = second.try_wait().unwrap() {
            break s;
        }
        assert!(Instant::now() < deadline, "the second sarkosd did not exit");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "{status}");
    let mut c = a.connect();
    let sid = c.create("/bin/sh");
    c.attach(sid);
}
```

- [ ] **Step 6: Run them**

Run: `~/.cargo/bin/cargo test -p sarkosd --test daemon -- recover`
Expected: PASS (five tests). If `recover_puts_a_session_whose_folder_is_gone_in_home` fails because the shell refuses to start in a deleted folder, the daemon's fallback is wrong, not the test.

- [ ] **Step 7: Run the suite ten times and the whole workspace**

Run: `for i in $(seq 10); do ~/.cargo/bin/cargo test -p sarkosd --test daemon 2>&1 | grep "test result" || break; done` then `~/.cargo/bin/cargo test --workspace`
Expected: ten `ok` lines; everything green.

- [ ] **Step 8: Commit**

```bash
git add crates/sarkosd/src/recovery.rs crates/sarkosd/src/server.rs crates/sarkosd/src/main.rs crates/sarkosd/tests/daemon.rs
git commit -m "Add sarkosd --recover: rebuild every session from the manifest and reopen its windows"
```

---

### Task 6: Tab states in the window: the pulse, the dot, the count, the notes, Goodbye

**Files:**
- Create: `crates/sarkos/src/pulse.rs`
- Modify: `crates/sarkos/src/sessions.rs`, `crates/sarkos/src/pane_view.rs`, `crates/sarkos/src/main.rs` (`mod pulse;`, `Root` fields, `handle_outcome`, `activate_tab`, `focus_pane`, `leaving`, the status bar call, the quit handlers)
- Test: unit tests in `pulse.rs`, `sessions.rs`, `pane_view.rs`, `main.rs`

**Interfaces:**
- Consumes: Task 1's `Request::{ListSessionsDetailed, Goodbye}`, `Response::SessionsDetailed`, `SessionDetail`; `Event::State`; `Terminal.bell_pending`; `theme.semantic.{working, waiting}`; `motion::reduce_motion()`.
- Produces: `sessions::Note { Restarted, Resumed }`, `SessionManager::{states, notes: HashMap<SessionId, Note>, note_state, acknowledge, goodbye, waiting_count(&Tree)}`, `Outcome::Sessions(Vec<SessionDetail>)`, `pulse::{alpha, TICK, start}`, `pane_view::{TabMark, tab_label(.., note: Option<Note>, ..)}`, `Root::{window_active, acknowledge, leaving, focused_session}`.

- [ ] **Step 1: Write the failing pulse test**

Create `crates/sarkos/src/pulse.rs`:

```rust
//! The working glyph's breathing (spec 5): a slow, soft, time-based pulse ticked at a low rate,
//! paused while the window is not active and held still under Reduce Motion (spec 7.1).

use crate::Root;
use gpui::{App, Entity};
use std::time::Duration;

/// One breath.
pub const CYCLE: Duration = Duration::from_secs(2);
/// How often a working tab asks for a repaint: ten frames a second is a few percent of one core
/// for a tab bar, and the eye cannot tell at this speed.
pub const TICK: Duration = Duration::from_millis(100);
/// While nothing pulses: a slow check for a tab that started working.
const IDLE_TICK: Duration = Duration::from_millis(500);

/// The glyph's opacity at `elapsed` into the pulse: a cosine between 0.35 and 1.0, so it never
/// vanishes. Reduce Motion holds it at full.
pub fn alpha(elapsed: Duration, reduce_motion: bool) -> f32 {
    if reduce_motion {
        return 1.0;
    }
    let t = (elapsed.as_secs_f32() % CYCLE.as_secs_f32()) / CYCLE.as_secs_f32();
    let wave = 0.5 - 0.5 * (t * std::f32::consts::TAU).cos();
    0.35 + 0.65 * wave
}

/// Repaints the window every `TICK` while a tab is working and the window is active, and
/// looks every `IDLE_TICK` otherwise. One task per window, for its whole life.
pub fn start(root: Entity<Root>, cx: &mut App) {
    cx.spawn(async move |cx| loop {
        // The window is gone when the update fails: the task ends with it.
        let Ok(pulsing) = root.update(cx, |root, cx| {
            let pulsing = root.window_active && !crate::motion::reduce_motion() && root.sessions.any_working();
            if pulsing {
                cx.notify();
            }
            pulsing
        }) else {
            return;
        };
        cx.background_executor().timer(if pulsing { TICK } else { IDLE_TICK }).await;
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pulse_breathes_between_a_third_and_full_over_two_seconds() {
        assert!((alpha(Duration::ZERO, false) - 0.35).abs() < 1e-4);
        assert!((alpha(Duration::from_secs(1), false) - 1.0).abs() < 1e-4);
        assert!((alpha(Duration::from_secs(2), false) - 0.35).abs() < 1e-4, "periodic");
        assert!((alpha(Duration::from_millis(500), false) - 0.675).abs() < 1e-3);
        let a = alpha(Duration::from_millis(100), false);
        let b = alpha(Duration::from_millis(200), false);
        assert!(b > a && b - a < 0.1, "a tick moves it by little");
    }

    #[test]
    fn the_pulse_holds_still_under_reduce_motion() {
        for ms in [0, 300, 1000, 1700] {
            assert_eq!(alpha(Duration::from_millis(ms), true), 1.0);
        }
    }
}
```

- [ ] **Step 2: Run it to see it fail, then pass**

Run: `~/.cargo/bin/cargo test -p sarkos -- pulse`
Expected: FAIL to compile until `mod pulse;` is in `main.rs` and `Root::window_active` and `SessionManager::any_working` exist (Steps 4 and 6); then PASS.

- [ ] **Step 3: Write the failing session manager tests**

In `crates/sarkos/src/sessions.rs`'s `mod tests` (the `info` helper stays; add `detail`):

```rust
    fn detail(id: u64, cwd: &Path, alive: bool, state: ClaudeState, recovered: bool) -> protocol::SessionDetail {
        let mut i = info(id, cwd, alive);
        i.state = state;
        protocol::SessionDetail { info: i, recovered, agent: None }
    }

    #[test]
    fn the_list_is_detailed_and_records_each_sessions_state_and_resumed_note() {
        let rec = Recorder::new();
        let mut m = SessionManager::new(rec.clone(), None, PathBuf::from("/p"));
        m.list();
        assert_eq!(rec.requests(), vec![Request::ListSessionsDetailed]);
        let list = vec![detail(1, Path::new("/p"), true, ClaudeState::Working, false), detail(2, Path::new("/p"), true, ClaudeState::Waiting, true)];
        let outcome = m.handle_response(1, Response::SessionsDetailed(list.clone()));
        assert_eq!(outcome, Outcome::Sessions(list));
        // The window records them (handle_outcome does, through `note_list`):
        let Outcome::Sessions(back) = outcome else { unreachable!() };
        m.note_list(&back);
        assert_eq!(m.states.get(&SessionId(1)), Some(&ClaudeState::Working));
        assert_eq!(m.states.get(&SessionId(2)), Some(&ClaudeState::Waiting));
        assert_eq!(m.notes.get(&SessionId(2)), Some(&Note::Resumed));
        assert_eq!(m.notes.get(&SessionId(1)), None);
    }

    #[test]
    fn a_state_event_updates_the_state_and_acknowledge_tells_the_daemon_idle() {
        let rec = Recorder::new();
        let mut m = SessionManager::new(rec.clone(), None, PathBuf::from("/p"));
        m.note_state(SessionId(3), ClaudeState::Waiting);
        assert_eq!(m.states.get(&SessionId(3)), Some(&ClaudeState::Waiting));
        assert!(m.acknowledge(SessionId(3)), "a waiting session is acknowledged");
        assert_eq!(m.states.get(&SessionId(3)), Some(&ClaudeState::Idle));
        assert_eq!(rec.requests(), vec![Request::SetState { session: SessionId(3), state: ClaudeState::Idle }]);
        assert!(!m.acknowledge(SessionId(3)), "nothing to acknowledge twice");
        m.note_state(SessionId(3), ClaudeState::Working);
        assert!(!m.acknowledge(SessionId(3)), "working is not waiting");
        assert_eq!(rec.requests().len(), 1);
    }

    #[test]
    fn waiting_count_covers_terminal_tabs_in_the_tree_and_any_working_is_true_with_one() {
        let rec = Recorder::new();
        let mut m = SessionManager::new(rec, None, PathBuf::from("/p"));
        let mut tree = Tree::new(Tab::Terminal { session: SessionId(1) });
        let root = tree.panes()[0];
        tree.add_tab(root, Tab::Terminal { session: SessionId(2) });
        tree.add_tab(root, Tab::Viewer { path: "/f".into() });
        m.note_state(SessionId(1), ClaudeState::Waiting);
        m.note_state(SessionId(2), ClaudeState::Waiting);
        m.note_state(SessionId(9), ClaudeState::Waiting);
        assert_eq!(m.waiting_count(&tree), 2, "a session no tab shows is not counted");
        assert!(!m.any_working());
        m.note_state(SessionId(2), ClaudeState::Working);
        assert!(m.any_working());
        assert_eq!(m.waiting_count(&tree), 1);
    }

    #[test]
    fn goodbye_is_sent_once_and_disconnect_forgets_states() {
        let rec = Recorder::new();
        let mut m = SessionManager::new(rec.clone(), None, PathBuf::from("/p"));
        m.note_state(SessionId(1), ClaudeState::Working);
        m.goodbye();
        assert_eq!(rec.requests(), vec![Request::Goodbye]);
        m.disconnect();
        assert!(m.states.is_empty(), "the next list says what each session does");
    }
```

Update the existing tests that build `Response::Sessions(..)` for `Pending::List` (`responses_are_matched_to_what_was_asked` at least) to `Response::SessionsDetailed(..)` and `Outcome::Sessions(Vec<SessionDetail>)`, and those that insert into `notes` to `notes.insert(id, Note::Restarted)` / `notes.contains_key`.

- [ ] **Step 4: Run them to see them fail**

Run: `~/.cargo/bin/cargo test -p sarkos -- sessions`
Expected: FAIL to compile: `Note`, `note_list`, `note_state`, `acknowledge`, `waiting_count`, `any_working`, `goodbye` not found.

- [ ] **Step 5: Implement the manager side**

In `sessions.rs`:

```rust
/// The word after a tab's title: the session is new in place of one the daemon lost, or was
/// recreated by `sarkosd --recover` after a restart. Both clear when the user types into the tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Note {
    Restarted,
    Resumed,
}
```

`SessionManager`: `pub notes: HashMap<SessionId, Note>` (every `notes.insert(session)` becomes `notes.insert(session, Note::Restarted)`, `contains` becomes `contains_key`, `remove(..)` returns an `Option` now: `is_some()` where a bool was used) and

```rust
    /// What each session's agent is doing (spec 5), from the list at connect and `Event::State`
    /// after; a BEL marks waiting too. A session not here is idle.
    pub states: HashMap<SessionId, ClaudeState>,
```

`list()` sends `Request::ListSessionsDetailed`; `handle_response` maps `(Some(Pending::List), Response::SessionsDetailed(list)) => Outcome::Sessions(list)` and `Outcome::Sessions(Vec<SessionDetail>)`. `reconcile` keeps taking `&[SessionInfo]`; the window passes `list.iter().map(|d| d.info.clone()).collect::<Vec<_>>()`. `disconnect()` clears `states`. New methods:

```rust
    /// Records what the daemon's list says about each session.
    pub fn note_list(&mut self, list: &[protocol::SessionDetail]) {
        for d in list {
            self.states.insert(d.info.id, d.info.state);
            if d.recovered {
                self.notes.insert(d.info.id, Note::Resumed);
            }
        }
    }

    pub fn note_state(&mut self, session: SessionId, state: ClaudeState) {
        self.states.insert(session, state);
    }

    /// The user looked at or typed into a waiting tab: idle now, and the daemon is told so a
    /// reconnect or another window agrees. True when there was something to acknowledge.
    pub fn acknowledge(&mut self, session: SessionId) -> bool {
        if self.states.get(&session) != Some(&ClaudeState::Waiting) {
            return false;
        }
        self.states.insert(session, ClaudeState::Idle);
        self.transport.send(Request::SetState { session, state: ClaudeState::Idle });
        true
    }

    /// Terminal tabs in the tree whose session waits on the user (the status bar's count).
    pub fn waiting_count(&self, tree: &Tree) -> usize {
        tree.tabs().into_iter().filter(|(_, _, tab)| matches!(tab, Tab::Terminal { session } if self.states.get(session) == Some(&ClaudeState::Waiting))).count()
    }

    pub fn any_working(&self) -> bool {
        self.states.values().any(|s| *s == ClaudeState::Working)
    }

    /// The user is quitting this window: a restart must not reopen it (section 11 step 4).
    pub fn goodbye(&mut self) {
        self.transport.send(Request::Goodbye);
    }
```

`route_events` records states and bells (the signature gains nothing; the terminal's bell flag is read after its events are applied):

```rust
    pub fn route_events(&mut self, events: &[Event], cx: &mut App) {
        let live: Vec<Event> = events.iter().filter(|ev| !self.stash_if_restoring(ev)).cloned().collect();
        for ev in &live {
            if let Event::State { session, state } = ev {
                self.states.insert(*session, *state);
            }
        }
        for (session, batch) in group_by_session(&live) {
            if let Some(t) = self.terminals.get(&session) {
                let rang = t.update(cx, |t, cx| {
                    t.handle_events(&batch, cx);
                    std::mem::take(&mut t.bell_pending)
                });
                if rang {
                    // Spec 5: a BEL means "needs you" even without hooks.
                    self.states.insert(session, ClaudeState::Waiting);
                }
            }
        }
    }
```

(`Terminal::handle_events` already sets `bell_pending` when the emulator reports a bell; `bell_pending` is `pub`.) Add `use protocol::ClaudeState;` at the top (the tests' own import then goes).

- [ ] **Step 6: Wire the window**

In `main.rs`:

`Root` gains `pub window_active: bool` (doc: "Whether this is the key window, read each frame; the pulse ticks only while it is") set in `render` right after `let size = ...`: `self.window_active = window.is_window_active();`. `Root::new` starts it `true`. `main` calls `pulse::start(root.clone(), cx);` next to `update::start`.

```rust
    /// The session of the focused pane's active terminal tab.
    pub fn focused_session(&self) -> Option<SessionId> {
        let p = self.tree.pane(self.tree.focused)?;
        match p.tabs.get(p.active)? {
            Tab::Terminal { session } => Some(*session),
            Tab::Viewer { .. } => None,
        }
    }

    /// Clears the waiting dot of the focused tab (the user is looking at it).
    pub fn acknowledge_focused(&mut self, cx: &mut Context<Self>) {
        if let Some(s) = self.focused_session() {
            if self.sessions.acknowledge(s) {
                cx.notify();
            }
        }
    }

    /// The user is quitting: tell the daemon not to reopen this window after a restart, then
    /// the flush every quit does. Not for the relaunch an update makes (`RelaunchAccepted`).
    pub fn leaving(&mut self) {
        self.sessions.goodbye();
        self.flush_for_quit();
    }
```

`activate_tab` and `focus_pane` call `self.acknowledge_focused(cx)` after they change the tree. The `on_key_down` listener in `pane_view::render_terminal` acknowledges the session it types into, next to the note removal: `if root.sessions.notes.remove(&session).is_some() | root.sessions.acknowledge(session) { cx.notify(); }` (single `|`, both sides run). The `Quit` action handler and the `on_window_closed` handler call `root.leaving()` instead of `root.flush_for_quit()`; `Outcome::RelaunchAccepted` keeps `flush_for_quit`. `handle_outcome`'s `Outcome::Sessions(list)` arm calls `self.sessions.note_list(&list)` first and reconciles with the infos; `Outcome::Created { .., note }` inserts `Note::Restarted`. The status bar call passes `self.sessions.waiting_count(&self.tree)` instead of `0`.

- [ ] **Step 7: Write the failing tab label tests**

In `pane_view.rs`'s tests, replace `tab_labels_show_state_after_the_title`:

```rust
    #[test]
    fn tab_labels_show_state_after_the_title() {
        assert_eq!(tab_label("vim", "zsh", true, None, None, None), "vim");
        assert_eq!(tab_label("", "shell", true, None, None, None), "shell");
        assert_eq!(tab_label("", "zsh", false, Some(1), None, None), "zsh (ended, exit 1)");
        assert_eq!(tab_label("vim", "zsh", false, None, None, None), "vim (ended)");
        assert_eq!(tab_label("", "zsh", true, None, Some(Note::Restarted), None), "zsh (restarted)");
        assert_eq!(tab_label("", "zsh", true, None, Some(Note::Resumed), None), "zsh (resumed after restart)");
        assert_eq!(tab_label("", "zsh", true, None, None, Some("starting shell")), "starting shell");
    }

    #[test]
    fn a_tab_mark_is_the_state_unless_the_shell_ended() {
        use protocol::ClaudeState;
        assert_eq!(TabMark::of(Some(ClaudeState::Working), true), TabMark::Working);
        assert_eq!(TabMark::of(Some(ClaudeState::Waiting), true), TabMark::Waiting);
        assert_eq!(TabMark::of(Some(ClaudeState::Idle), true), TabMark::None);
        assert_eq!(TabMark::of(None, true), TabMark::None);
        assert_eq!(TabMark::of(Some(ClaudeState::Working), false), TabMark::Dead, "ended outranks a stale state");
    }
```

- [ ] **Step 8: Run them to see them fail, then implement the bar**

Run: `~/.cargo/bin/cargo test -p sarkos -- pane_view`
Expected: FAIL to compile. Then in `pane_view.rs`:

```rust
/// What a tab shows before its title (spec 5): a breathing glyph, a warm dot, nothing, or a
/// muted title for a shell that ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabMark {
    None,
    Working,
    Waiting,
    Dead,
}

impl TabMark {
    pub fn of(state: Option<protocol::ClaudeState>, alive: bool) -> TabMark {
        use protocol::ClaudeState::*;
        match (alive, state) {
            (false, _) => TabMark::Dead,
            (true, Some(Working)) => TabMark::Working,
            (true, Some(Waiting)) => TabMark::Waiting,
            _ => TabMark::None,
        }
    }
}
```

`tab_label`'s `note: bool` becomes `note: Option<Note>` (import `crate::sessions::Note`), printing `(restarted)` or `(resumed after restart)`. `tab_labels` returns `Vec<(String, TabMark)>`: for a terminal tab with a terminal entity, `TabMark::of(root.sessions.states.get(session).copied(), t.alive)`; without one (starting), `TabMark::None`; viewers `TabMark::None`. `menu_rows` and the drag label take the string half. In `render_tab_bar`, per tab, before `.child(label)`:

```rust
        let alpha = pulse::alpha(root.pulse_epoch.elapsed(), crate::motion::reduce_motion());
        let glyph = match mark {
            TabMark::Working => {
                let mut c = theme.semantic.working.hsla();
                c.a = alpha;
                Some(div().size(px(7.0)).rounded_full().bg(c))
            }
            TabMark::Waiting => Some(div().size(px(7.0)).rounded_full().bg(theme.semantic.waiting.hsla())),
            TabMark::None | TabMark::Dead => None,
        };
```

added with `.when_some(glyph, |el, g| el.child(g))`, and a dead tab's text colour is `dim` with its alpha multiplied by `0.6` (`let mut muted = dim.hsla(); muted.a *= 0.6;`). `Root` gains `pub pulse_epoch: Instant` (set in `new`; the pulse's phase is shared by every working tab, so they breathe together). Compute `alpha` once per bar, not per tab.

- [ ] **Step 9: Add two Root tests and run everything**

In `main.rs`'s tests (headless; `Root` is not built there today, so test the pure helpers you added: if `focused_session` needs a `Root`, test it through `Tree` directly by extracting `fn focused_session_of(tree: &Tree) -> Option<SessionId>` and having the method call it):

```rust
    #[test]
    fn the_focused_session_is_the_focused_panes_active_terminal_tab() {
        let mut tree = Tree::new(Tab::Terminal { session: SessionId(5) });
        let root = tree.panes()[0];
        assert_eq!(focused_session_of(&tree), Some(SessionId(5)));
        tree.add_tab(root, Tab::Viewer { path: "/f".into() });
        tree.activate(root, 1);
        assert_eq!(focused_session_of(&tree), None);
    }
```

Run: `~/.cargo/bin/cargo test -p sarkos` then `~/.cargo/bin/cargo build --release 2>&1 | grep -i "^warning"`
Expected: PASS; no warnings for the project's crates. Then run the window for a minute against the real daemon with a `claude` session and the hooks from Task 3 installed (`target/release/sarkos hooks install`, then start `claude` in a tab): the tab pulses while Claude works, shows the dot when it stops, the status bar says "1 waiting", clicking the tab clears it. Put what you saw in the report; if the screen is locked, say so and leave it to the README checklist.

- [ ] **Step 10: Commit**

```bash
git add crates/sarkos/src/pulse.rs crates/sarkos/src/sessions.rs crates/sarkos/src/pane_view.rs crates/sarkos/src/main.rs
git commit -m "Show what each tab's agent is doing: a breathing glyph, a waiting dot, the count, and notes"
```

---

### Task 7: Start at login, the palette items, the config section and the README

**Files:**
- Create: `crates/sarkos/src/launch_agent.rs`
- Modify: `crates/sarkos/src/config.rs`, `crates/sarkos/src/keymap.rs`, `crates/sarkos/src/command_palette.rs`, `crates/sarkos/src/main.rs` (`mod launch_agent;`, `Command::{StartAtLogin, InstallHooks}` in `run`, `apply_recovery`, the launch notice), `README.md`
- Test: unit tests in `launch_agent.rs`, `config.rs`, `command_palette.rs`, `keymap.rs` (the table tests must still pass)

**Interfaces:**
- Consumes: Task 3's `hook_cli::{install_hooks, claude_settings_path}`; `paths::{daemon_exe, socket_path}`; `client::daemon_log`; `config.daemon`; `update::launch_path`.
- Produces: `config::{RecoveryConfig, StartAtLogin}`, `launch_agent::{LABEL, agent_path, plist, xml_escape, installed, install, remove, desired}`, `Command::{StartAtLogin, InstallHooks}`, palette items "Start at login" and "Install Claude Code hooks".

- [ ] **Step 1: Write the failing config test**

In `config.rs`'s tests:

```rust
    #[test]
    fn the_recovery_section_defaults_to_ask_and_parses_on_and_off() {
        let c = Config::default();
        assert_eq!(c.recovery.start_at_login, StartAtLogin::Ask);
        let on: Config = toml::from_str("[recovery]\nstart_at_login = \"on\"\n").unwrap();
        assert_eq!(on.recovery.start_at_login, StartAtLogin::On);
        let off: Config = toml::from_str("[recovery]\nstart_at_login = \"off\"\n").unwrap();
        assert_eq!(off.recovery.start_at_login, StartAtLogin::Off);
        assert!(toml::from_str::<Config>("[recovery]\nstart_at_login = true\n").is_err(), "a string, like appearance.mode");
        let parsed: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        assert_eq!(parsed.recovery.start_at_login, StartAtLogin::Ask, "the written defaults say ask");
    }
```

- [ ] **Step 2: Run it to see it fail, then add the section**

Run: `~/.cargo/bin/cargo test -p sarkos -- the_recovery_section`
Expected: FAIL to compile. Then in `config.rs`, after `AppearanceConfig`:

```rust
/// Whether sarkosd starts at login to bring sessions back after a restart (update-in-place
/// spec, section 11): "ask" until the user runs "Start at login" from the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StartAtLogin {
    #[default]
    Ask,
    On,
    Off,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RecoveryConfig {
    pub start_at_login: StartAtLogin,
}
```

`Config` gains `pub recovery: RecoveryConfig` (and its `Default`). `DEFAULT_CONFIG` gains, after `[appearance]`:

```toml

[recovery]
# After a restart, sarkosd started at login brings every session back: the same folders, the
# history on screen under a divider, and each Claude Code conversation resumed (never continued:
# you send the next message). Everything stays on this Mac. "ask" shows a notice at launch until
# "Start at login" runs from the command palette; "on" installs the login item
# (~/Library/LaunchAgents/sh.csarko.sarkosd.plist) and keeps it; "off" removes it.
start_at_login = "ask"
```

and the `[keybindings]` comment block gains `# "start at login" = ""` and `# "install claude code hooks" = ""` in the places the `ACTIONS` order puts them (the test that every action is documented tells you).

- [ ] **Step 3: Write the failing launch agent tests**

Create `crates/sarkos/src/launch_agent.rs`:

```rust
//! The login item that brings sessions back after a restart (update-in-place spec, section 11
//! step 3): a launchd agent in `~/Library/LaunchAgents` that runs `sarkosd --recover` once at
//! login. No KeepAlive: a daemon the window started is left alone, and `--recover` exits 0 when
//! one is listening. Installed only when the user asks (the palette) or the config says "on".

use crate::config::StartAtLogin;
use std::path::{Path, PathBuf};

pub const LABEL: &str = "sh.csarko.sarkosd";

pub fn agent_path() -> PathBuf {
    PathBuf::from(std::env::var("HOME").expect("HOME is set")).join("Library/LaunchAgents").join(format!("{LABEL}.plist"))
}

pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The agent: `sarkosd --recover` with the window's `[daemon]` arguments, once at load, logging
/// where the window's own daemon logs. `ExitTimeOut` gives the SIGTERM flush twenty seconds
/// before launchd's SIGKILL at logout.
pub fn plist(daemon_exe: &Path, daemon_args: &[String], log: &Path) -> String {
    let mut args = vec![daemon_exe.to_string_lossy().into_owned(), "--recover".to_string()];
    args.extend(daemon_args.iter().cloned());
    let items: String = args.iter().map(|a| format!("\t\t<string>{}</string>\n", xml_escape(a))).collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n<dict>\n\
\t<key>Label</key>\n\t<string>{LABEL}</string>\n\
\t<key>ProgramArguments</key>\n\t<array>\n{items}\t</array>\n\
\t<key>RunAtLoad</key>\n\t<true/>\n\
\t<key>KeepAlive</key>\n\t<false/>\n\
\t<key>ProcessType</key>\n\t<string>Interactive</string>\n\
\t<key>ExitTimeOut</key>\n\t<integer>20</integer>\n\
\t<key>StandardOutPath</key>\n\t<string>{log}</string>\n\
\t<key>StandardErrorPath</key>\n\t<string>{log}</string>\n\
</dict>\n</plist>\n",
        log = xml_escape(&log.to_string_lossy())
    )
}

pub fn installed(path: &Path) -> bool {
    path.is_file()
}

/// What to do so the file matches the config: `Some(true)` install (or rewrite), `Some(false)`
/// remove, `None` leave it. "ask" never touches it: the palette does, with the user's say-so.
pub fn desired(mode: StartAtLogin, installed: bool) -> Option<bool> {
    match (mode, installed) {
        (StartAtLogin::On, _) => Some(true),
        (StartAtLogin::Off, true) => Some(false),
        (StartAtLogin::Off, false) | (StartAtLogin::Ask, _) => None,
    }
}

fn gui_domain() -> String {
    // SAFETY: getuid takes no arguments and cannot fail.
    format!("gui/{}", unsafe { libc::getuid() })
}

fn launchctl(args: &[&str]) -> Result<(), String> {
    let out = std::process::Command::new("launchctl").args(args).output().map_err(|e| format!("cannot run launchctl: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("launchctl {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Writes the plist and loads it into this login session, so a bad file fails now rather than
/// at the next login. Loading runs `sarkosd --recover` once, which exits at once while a daemon
/// is listening. Blocks on launchctl: call it off the UI thread.
pub fn install(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    if std::fs::read_to_string(path).ok().as_deref() == Some(contents) {
        return Ok(());
    }
    std::fs::write(path, contents).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    // A definition already loaded (an earlier install) must go before the new one loads.
    let _ = launchctl(&["bootout", &format!("{}/{LABEL}", gui_domain())]);
    launchctl(&["bootstrap", &gui_domain(), &path.to_string_lossy()])
}

/// Unloads and deletes the agent. A missing file is fine.
pub fn remove(path: &Path) -> Result<(), String> {
    let _ = launchctl(&["bootout", &format!("{}/{LABEL}", gui_domain())]);
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("cannot remove {}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plist_runs_recover_once_at_login_with_the_daemon_arguments_escaped() {
        let p = plist(Path::new("/Users/me/My Apps/sarkosd"), &["--watchdog-secs".into(), "30".into()], Path::new("/Users/me/Library/Application Support/sarkos/sarkosd.log"));
        assert!(p.contains("<string>sh.csarko.sarkosd</string>"));
        assert!(p.contains("<string>/Users/me/My Apps/sarkosd</string>\n\t\t<string>--recover</string>\n\t\t<string>--watchdog-secs</string>\n\t\t<string>30</string>"));
        assert!(p.contains("<key>RunAtLoad</key>\n\t<true/>"));
        assert!(p.contains("<key>KeepAlive</key>\n\t<false/>"));
        assert!(p.contains("<key>ExitTimeOut</key>\n\t<integer>20</integer>"));
        assert!(p.contains("<string>/Users/me/Library/Application Support/sarkos/sarkosd.log</string>"));
        assert_eq!(xml_escape("a&b<c>\"d\""), "a&amp;b&lt;c&gt;&quot;d&quot;");
        assert!(plist(Path::new("/x&y/sarkosd"), &[], Path::new("/l")).contains("<string>/x&amp;y/sarkosd</string>"));
        assert!(agent_path().ends_with("Library/LaunchAgents/sh.csarko.sarkosd.plist"));
    }

    #[test]
    fn the_config_keeps_on_and_off_in_force_and_ask_leaves_the_file_alone() {
        assert_eq!(desired(StartAtLogin::On, false), Some(true));
        assert_eq!(desired(StartAtLogin::On, true), Some(true), "rewritten when the arguments changed; a same file is a no-op");
        assert_eq!(desired(StartAtLogin::Off, true), Some(false));
        assert_eq!(desired(StartAtLogin::Off, false), None);
        assert_eq!(desired(StartAtLogin::Ask, false), None);
        assert_eq!(desired(StartAtLogin::Ask, true), None);
    }

    #[test]
    fn installed_is_whether_the_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.plist");
        assert!(!installed(&p));
        std::fs::write(&p, "x").unwrap();
        assert!(installed(&p));
    }
}
```

(`install` and `remove` call `launchctl` and are not unit-tested: the README checklist covers them. Say so in the module doc.)

- [ ] **Step 4: Run the tests**

Run: `~/.cargo/bin/cargo test -p sarkos -- launch_agent config`
Expected: PASS (after `mod launch_agent;` in `main.rs`).

- [ ] **Step 5: Write the failing palette and keymap tests**

In `command_palette.rs`'s tests, extend `items_cover_actions_presets_themes_and_config` (its `items(..)` call gains `StartAtLogin::Ask, false, Path::new("/Users/me/.claude/settings.json")` at the end) and add:

```rust
    #[test]
    fn the_recovery_items_say_what_running_them_does() {
        use crate::config::StartAtLogin;
        let cfg = Config::default();
        let settings = Path::new("/Users/me/.claude/settings.json");
        let find = |items: &[Item], label: &str| items.iter().find(|i| i.label == label).cloned().unwrap_or_else(|| panic!("no {label}"));
        let ask_off = items(&cfg, &[], "comfortable", "navy", Path::new("/t"), None, StartAtLogin::Ask, false, settings);
        assert_eq!(find(&ask_off, "Start at login").detail, "off: turn on so sessions come back after a restart");
        assert_eq!(find(&ask_off, "Start at login").command, Command::StartAtLogin);
        let ask_on = items(&cfg, &[], "comfortable", "navy", Path::new("/t"), None, StartAtLogin::Ask, true, settings);
        assert_eq!(find(&ask_on, "Start at login").detail, "on: run to turn off");
        let pinned = items(&cfg, &[], "comfortable", "navy", Path::new("/t"), None, StartAtLogin::On, true, settings);
        assert_eq!(find(&pinned, "Start at login").detail, "on, set in config.toml ([recovery] start_at_login)");
        let hooks = find(&ask_off, "Install Claude Code hooks");
        assert_eq!(hooks.detail, "into /Users/me/.claude/settings.json");
        assert_eq!(hooks.command, Command::InstallHooks);
    }
```

In `keymap.rs`: the `actions!` list gains `StartAtLogin, InstallHooks`; `Command` gains `StartAtLogin` (doc: "Install or remove the login item that brings sessions back after a restart; with the config pinning it, open the config instead") and `InstallHooks` ("Merge the Claude Code hook entries into its settings file"); `ACTIONS` gains `row("start at login", "")` and `row("install claude code hooks", "")` after `row("update now", "")`; `bindings_for`, `command_for` and `attach` get the two names. The existing table tests (`the_table_is_the_spec_table`, `every_row_runs_the_action_it_describes`) pin the rows: update their expected lists.

- [ ] **Step 6: Run them to see them fail, then implement**

Run: `~/.cargo/bin/cargo test -p sarkos -- command_palette keymap`
Expected: FAIL to compile. Then `command_palette::items` gains the three parameters (`start_at_login: StartAtLogin, login_item_installed: bool, claude_settings: &Path`) and, in the `ACTIONS` loop, details for the two commands:

```rust
        let detail = match command {
            Command::UpdateNow => update_ready.unwrap_or("relaunch on the installed binaries").to_string(),
            Command::StartAtLogin => match (start_at_login, login_item_installed) {
                (StartAtLogin::Ask, false) => "off: turn on so sessions come back after a restart".to_string(),
                (StartAtLogin::Ask, true) => "on: run to turn off".to_string(),
                (StartAtLogin::On, _) => "on, set in config.toml ([recovery] start_at_login)".to_string(),
                (StartAtLogin::Off, _) => "off, set in config.toml ([recovery] start_at_login)".to_string(),
            },
            Command::InstallHooks => format!("into {}", claude_settings.display()),
            _ => keymap::keys_for(a.name, &cfg.keybindings).map(|k| keymap::pretty_keys(&k)).unwrap_or_default(),
        };
```

In `main.rs`, `toggle_palette` passes `self.config.recovery.start_at_login, launch_agent::installed(&launch_agent::agent_path()), &hook_cli::claude_settings_path()`. `Root::run` gains:

```rust
            StartAtLogin => self.toggle_start_at_login(cx),
            InstallHooks => {
                let exe = update::launch_path().unwrap_or_else(|| PathBuf::from("sarkos"));
                let root = cx.entity();
                cx.spawn(async move |_, cx| {
                    let result = cx.background_executor().spawn(async move { hook_cli::install_hooks(&exe) }).await;
                    let text = match result {
                        Ok((path, added, replaced)) => format!("installed {added} hook entries in {} (replaced {replaced}); restart claude for them to take effect", path.display()),
                        Err(e) => format!("cannot install hooks: {e}"),
                    };
                    let _ = root.update(cx, |root, cx| root.show_notice(text, cx));
                })
                .detach();
            }
```

and the methods:

```rust
    /// The plist for this install: the sibling daemon, the config's daemon arguments, the log.
    fn agent_plist(&self) -> String {
        let args = vec![
            "--watchdog-secs".to_string(),
            self.config.daemon.watchdog_secs.to_string(),
            "--replay-bytes".to_string(),
            self.config.daemon.replay_bytes.to_string(),
        ];
        launch_agent::plist(&paths::daemon_exe(), &args, &client::daemon_log(&paths::socket_path()))
    }

    /// Installs or removes the login item off the UI thread and says what happened.
    fn set_start_at_login(&mut self, on: bool, cx: &mut Context<Self>) {
        let path = launch_agent::agent_path();
        let contents = self.agent_plist();
        let root = cx.entity();
        cx.spawn(async move |_, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { if on { launch_agent::install(&path, &contents) } else { launch_agent::remove(&path) } })
                .await;
            let text = match (on, result) {
                (true, Ok(())) => "start at login is on: after a restart, sessions come back on their own".to_string(),
                (false, Ok(())) => "start at login is off".to_string(),
                (_, Err(e)) => format!("start at login: {e}"),
            };
            let _ = root.update(cx, |root, cx| root.show_notice(text, cx));
        })
        .detach();
    }

    /// The palette's "Start at login": flips the login item while the config says "ask"; when
    /// the config pins it, opens the config instead.
    pub fn toggle_start_at_login(&mut self, cx: &mut Context<Self>) {
        match self.config.recovery.start_at_login {
            config::StartAtLogin::Ask => {
                let on = !launch_agent::installed(&launch_agent::agent_path());
                self.set_start_at_login(on, cx);
            }
            _ => self.open_config(cx),
        }
    }

    /// Keeps the login item as the config says; with "ask" and nothing installed, says once per
    /// launch how to turn it on.
    pub fn apply_recovery(&mut self, at_launch: bool, cx: &mut Context<Self>) {
        let installed = launch_agent::installed(&launch_agent::agent_path());
        match launch_agent::desired(self.config.recovery.start_at_login, installed) {
            Some(on) => self.set_start_at_login(on, cx),
            None if at_launch && !installed => {
                self.show_notice("sessions can come back after a restart: run \"Start at login\" from the command palette (Cmd+Shift+P)".into(), cx)
            }
            None => {}
        }
    }
```

`apply_config` calls `self.apply_recovery(false, cx)`; `main` calls `root.update(cx, |root, cx| root.apply_recovery(true, cx))` right after the config and theme errors are set. `set_start_at_login(true, ..)` with an unchanged file is a no-op inside `install` (so a reload with "on" costs one file read and no launchctl).

- [ ] **Step 7: Run the whole window crate and the build**

Run: `~/.cargo/bin/cargo test -p sarkos` then `~/.cargo/bin/cargo build --release 2>&1 | grep -i "^warning"`
Expected: PASS; no warnings for the project's crates.

- [ ] **Step 8: Write the README**

In `README.md`:

The **Status** paragraph becomes:

> Session awareness and recovery after a restart: each tab shows what its Claude Code session is doing (a breathing glyph while it works, a warm dot when it waits on you, a muted title when the shell ended), the status bar counts the sessions waiting, and with "Start at login" on, a restart brings every session back on its own: the same folders, the history on screen under a divider, and each Claude Code conversation resumed. Milestone 4 (the sidebar) is next.

Two new sections before **Keyboard**:

```markdown
## Session awareness

Claude Code tells the IDE what it is doing through its hooks. Run "Install Claude Code hooks"
from the command palette (or `sarkos hooks install`), which merges six entries into
`~/.claude/settings.json` after writing a `.bak`, then start or restart `claude` in a tab:

| Claude Code event | The tab |
|---|---|
| a prompt is sent, a tool runs | a slow breathing glyph: working |
| Claude stops, or asks for permission or input | a warm dot: waiting on you |
| you click the tab or type into it | the dot clears |
| the shell exits | a muted title, with the exit code |

A terminal bell marks a tab waiting too, so a `claude` set to ring the bell works without hooks.
`sarkos notify` is the hook side: it reads `SARKOS_SESSION_ID`, which every shell the daemon
starts carries, and exits 0 whatever happens, so a `claude` run outside Sarkos IDE is unaffected.
`sarkos hooks print` prints the entries for a hand paste. The entries name this binary by its
launch path; after moving it, install again.

## After a restart

A shutdown or restart ends every process; what becomes automatic is the recovery. The daemon
writes each session's history to `~/Library/Application Support/sarkos/sessions/<id>.replay`
a couple of seconds after output stops (in full on SIGTERM, which a clean shutdown sends) and a
manifest of the sessions (folder, shell, the Claude Code session id from its SessionStart hook)
and of the project folders with a window open. "Start at login" from the palette installs
`~/Library/LaunchAgents/sh.csarko.sarkosd.plist`, which runs `sarkosd --recover` once at login:
it recreates each shell in its folder (the home folder when that is gone) with the old output
under a dim "resumed after restart" line, types `claude --resume <id>` into the shells that had
a Claude Code session, and opens a window for each project that had one; each tab says
"(resumed after restart)" until you type into it. A window you quit yourself stays closed.

What does not come back: whatever was executing at that moment (a half-finished tool call, a
running build, shell variables, background jobs). Claude Code's transcript records the
unfinished tool, and the agent continues from there once asked; recovery only resumes, it never
sends a message. Everything stays on this Mac. `[recovery] start_at_login` in the config pins
it on or off; "ask", the default, shows a one-line notice at launch until you decide.
```

In **Updating**, the protocol sentence becomes "The daemon and window speak protocol 3; the daemon still serves protocol 1 and 2 windows, and reads the previous handover format, so updating from the update-in-place build is an ordinary "Update now". Upgrading from milestone 2b is the one update that needs `pkill -x sarkosd`". The checklist gains, at the top:

```markdown
- "Install Claude Code hooks", then `claude` in a tab: the glyph breathes while it works, a dot
  appears when it stops, the status bar says "1 waiting", clicking the tab clears both; with
  Reduce Motion on, the glyph holds still
- "Start at login", then `pkill -TERM sarkosd` with four tabs open: the log shows "SIGTERM:
  writing every session's history"; `launchctl kickstart gui/$(id -u)/sh.csarko.sarkosd` (what
  login does) brings a window back with every tab's history under the divider and
  `claude --resume` typed into the ones that ran Claude; the tabs say "(resumed after restart)"
- a real restart with the same four tabs: the window is back after login with the same result
- Cmd+Q a window, then the restart: that project does not reopen
- `start_at_login = "off"` in the config: the plist is gone within a second; `"on"`: it is back
```

- [ ] **Step 9: Check the copy and commit**

Run: `grep -n "—" README.md crates/sarkos/src/config.rs crates/sarkos/src/launch_agent.rs crates/sarkos/src/hook_cli.rs crates/sarkosd/src/recovery.rs` (no em-dashes), then `~/.cargo/bin/cargo test --workspace`.
Expected: nothing from grep; everything green.

```bash
git add crates/sarkos/src/launch_agent.rs crates/sarkos/src/config.rs crates/sarkos/src/keymap.rs crates/sarkos/src/command_palette.rs crates/sarkos/src/main.rs README.md
git commit -m "Offer start at login and hook installation from the palette, and document recovery"
```

---

## Self-review notes

- Spec 5's table maps `Stop` to waiting and `Notification` (permission or idle prompt) to waiting; `UserPromptSubmit` and `PreToolUse` to working: Task 3's `hooks_json`. Icons, the pulse's rate and pause, the dot until focus, dead tabs muted, the BEL: Task 6. The status bar count: Task 6. "Empty states" in milestone 3's line already shipped in milestone 2 (the empty pane's animation honours Reduce Motion).
- Section 11's four steps: replay files and manifest (Task 4), the agent record from SessionStart (Tasks 1 and 3), the launchd agent with consent (Task 7), windows reopened after the daemon listens (Task 5). The manifest leaves out the Claude state on purpose: a recovered agent starts idle.
- Spec 2.1's "Nothing survives daemon death or a reboot" is superseded by section 11 for a reboot; daemon death without a shutdown signal still loses the ring's last seconds (whatever was not flushed), which the README says.
- Type consistency checked: `SpawnSpec` fields (`seed`, `recovered`, `agent`) are named the same in Tasks 1, 4 and 5; `Outcome::Sessions(Vec<SessionDetail>)` in Task 6 matches Task 1's response; `command_palette::items` has the same nine parameters in Task 7's test and call; `Note` is `sessions::Note` everywhere; `is_sarkos_binary` lives in `server.rs` (Task 1) and is used by Task 5.
- Spec 5 pauses the pulse "when the window is not visible"; gpui 0.2.2 exposes no occlusion state, so Task 6 pauses it when the window is not the key window (`is_window_active`), which covers minimized, hidden and backgrounded windows; a visible window behind another keeps breathing at ten frames a second. Recorded as a ruling.
- Review Focus pins: 1 in Task 3, 2 in Task 4, 3 and 4 in Task 5, 5 in Task 6.
