# Update and Launcher Polish (milestone 6a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the update path and the launcher trustworthy: the check says what it found, a failed upgrade rolls back, the daemon remembers what it reached, the title bar ranks what can be acted on first, a Dock launch never dies silently, the chooser opens at once and reconnects, the journal stays readable, a staged release survives a daemon restart, two windows both reach the recents file, and a closed tab's session cannot be orphaned by a crash.

**Architecture:** Protocol 5 appends two requests and one response (postcard is positional, so nothing existing changes shape). The daemon gains `last_check.json`, an adopted-stage pass at poll start, a journal collapse in the poll thread, a grace deadline per session enforced by the reaper, and a clock restart on every failed-upgrade path. The window gains a pure check-outcome machine, a pure title-bar choice, a startup that runs `Application::run` first and connects on the background executor, a connect-failure window, a reconnecting chooser link, and a locked recents write.

**Tech Stack:** Rust 2021, gpui 0.2.2 (unchanged), postcard (unchanged), serde_json for the two small daemon files, `libc::flock`. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-10-15-update-launcher-polish-design.md` (sections 3 to 16 map to the tasks below; its rulings in section 17 are binding: grace only, chooser opens first).

## Global Constraints

- Machine rules (the user's MacBook Air locked up from build load): every cargo call is `nice -n 15 ~/.cargo/bin/cargo ...`; the worktree's local `.cargo/config.toml` (`jobs = 3`, shared `target-dir`) stays; no `--release` builds on the laptop; never two cargo processes at once; focused filters while iterating, the workspace suite once per task; the warning check is `cargo check --workspace --all-targets`; the daemon suite's ten-run rule runs serially.
- Protocol: `PROTOCOL_VERSION = 5`, `PROTOCOL_MIN = 1`. New variants are appended at the end of their enums, never inserted: `Request::UpdateStatusDetailed` (index 19), `Request::DetachForGrace { session, secs: u32 }` (20), `Response::UpdateStatusDetailed { staged, last_error, unreachable_for_secs, checking: bool, last_check_unix: Option<u64>, transient_error: Option<String> }` (8). Existing variants and structs never change shape. The daemon answers the old `UpdateStatus` and the new one alike; the window sends the new one only when the daemon's `protocol_max >= UPDATE_STATUS_DETAILED_FROM`.
- Protocol constants, all in `crates/protocol/src/lib.rs`: `UPDATE_STATUS_FROM = 4`, `UPDATE_EVENTS_FROM = 4`, `UPDATE_STATUS_DETAILED_FROM = 5`, `DETACH_FOR_GRACE_FROM = 5`. No literal `4` or `5` protocol gate anywhere else.
- Daemon files under the updates dir: `last_check.json` is `{"ok_unix":N}` written through `last_check.json.tmp` and `rename`; `applied.json` keeps `old_kept`, now `#[serde(default)]`.
- Journal lines (exact): first "not newer" check as today; the collapse line is `check: {n} more checks since {HH:MM} found {version} ({git}), still not newer than {floor}`; the transient collapse is `check: transient, {n} more times since {HH:MM}: {error}`; the failed-upgrade line is `the upgrade onto {version} failed: {why}; the grace for {version} starts now`; the adoption line is `adopted staged {version} ({git}) from before the restart`; the grace end line is `session {id} in {cwd}: not reopened within {secs} s, ended`.
- Window texts (exact): "update ready: {version}" (clickable, `chrome::is_update_item` unchanged), "update failed", the unreachable text from `update::unreachable_text`; check notices "no update", "update check failed: {e}", "updates unreachable: {e}", "the update check is still running", and for a protocol 4 daemon "the daemon has not reached the update server yet"; chooser footer "connecting to the daemon…" and "reconnecting to the daemon…"; the failure window title "Arkos cannot reach its daemon" with buttons "Open the log", "Try again", "Quit".
- Timings: the window polls the check every 500 ms for at most 30 s; the chooser's reconnect uses `connection::backoff`, `START_DAEMON_AFTER` (10 s) and `MAX_START_FAILURES` (5); the close grace is 60 s (`closed_stack::CLOSE_GRACE`); the reaper's pass stays 1 s; the journal's daily collapse line is every 24 h.
- No em-dashes in any file. Warning-free. Headless tests only; nothing in `cargo test` opens a window or touches the real daemon socket.

## Review Focus

1. A daemon exec (update in place) in the middle of a window's check poll: the poll's `UpdateStatusDetailed` answers stop arriving, the 30 s timeout fires, the notice says "the update check is still running", and the new connection's Hello restarts nothing twice (Task 4 `a_check_that_outlives_the_connection_times_out_quietly`).
2. `DetachForGrace` for a session another window has attached: the deadline is set, the other window's attach is a `subscribe` and clears it, so nothing ends (Task 8 `a_grace_is_cancelled_by_any_attach`).
3. A `last_check.json` from a daemon that ran under a different user, unreadable or malformed: None, no panic, no journal spam (Task 2 `a_bad_last_check_file_is_ignored`).
4. The failure window's "Try again" when the daemon binary path is wrong: the second attempt fails the same way, the window stays with the new error text, nothing spawns twice (Task 5 `try_again_reruns_the_connect_once`).
5. Adoption of a staged folder whose bundle is newer than the floor but whose `manifest.json` names a different version: not adopted, and the next check proceeds as today (Task 2 `a_bundle_the_manifest_does_not_name_is_not_adopted`).

## File structure

- `crates/protocol/src/lib.rs`: the three variants, the four constants, the index test.
- `crates/arkosd/src/server.rs`: answers `UpdateStatusDetailed` and `DetachForGrace`; the refusal line fix; the failed-upgrade clock in the `RelaunchClient` refusal and `validate` paths; `broadcast` uses the protocol constant.
- `crates/arkosd/src/updates/mod.rs`: `last_check_unix`, `transient_error`, `last_check.json` read and write, `adopt_staged`, the guard in `drop_stale_staged`.
- `crates/arkosd/src/updates/poll.rs`: `record_at` sets the new fields and writes the file; `start` adopts first and collapses repeats through a new `Collapse` struct (pure, tested).
- `crates/arkosd/src/updates/apply.rs`: `old_kept` default.
- `crates/arkosd/src/upgrade.rs`: `restore` starts the clock.
- `crates/arkosd/src/session.rs`, `reaper.rs`, `handover.rs`, `recovery.rs`: the grace deadline.
- `crates/arkos/src/update_check.rs` (new): the pure outcome machine and the pure title-bar choice.
- `crates/arkos/src/sessions.rs`, `main.rs`: the poll, the notices, the title bar, the startup, the failure window, `close_tab`.
- `crates/arkos/src/startup.rs` (new): `connect_in_background`, the failure window.
- `crates/arkos/src/connection.rs`: `reconnect(socket, daemon_exe, daemon_args, lost_at, attempt, start_failures)` extracted for the chooser link.
- `crates/arkos/src/launch_link.rs`, `launch.rs`: reconnect, the footer, the font, the appearance.
- `crates/arkos/src/recent.rs`: the lock.
- `README.md`: Updating section, launcher paragraph, Status rows.

---

### Task 1: Protocol 5

**Files:**
- Modify: `crates/protocol/src/lib.rs` (constants near line 16; `Request` after `ApplyUpdate`; `Response` after `UpdateStatus`; the test `new_variants_are_appended_after_the_old_ones`)
- Modify: `crates/protocol/src/codec.rs` (the version assertions near line 124)
- Modify: `crates/arkosd/src/server.rs` (`UPDATE_EVENTS_FROM`, the `UpdateStatus` arm, two new arms)
- Modify: `crates/arkos/src/main.rs:2387` (the `protocol_max >= 4` gate)

**Interfaces:**
- Produces: `protocol::{UPDATE_STATUS_FROM, UPDATE_EVENTS_FROM, UPDATE_STATUS_DETAILED_FROM, DETACH_FOR_GRACE_FROM}`, `Request::UpdateStatusDetailed`, `Request::DetachForGrace { session: SessionId, secs: u32 }`, `Response::UpdateStatusDetailed { .. }`. Task 2 fills the daemon's answer; Task 8 implements `DetachForGrace`; Task 4 sends the detailed request.

- [ ] **Step 1: Write the failing protocol test**

In `crates/protocol/src/lib.rs`, extend `new_variants_are_appended_after_the_old_ones`:

```rust
assert_eq!(PROTOCOL_VERSION, 5);
assert_eq!(PROTOCOL_MIN, 1);
assert_eq!(first_byte(&Request::UpdateStatusDetailed), 19);
assert_eq!(first_byte(&Request::DetachForGrace { session: SessionId(1), secs: 60 }), 20);
let detailed = Response::UpdateStatusDetailed { staged: None, last_error: None, unreachable_for_secs: Some(0), checking: true, last_check_unix: Some(7), transient_error: Some("offline".into()) };
assert_eq!(first_byte(&detailed), 8);
let bytes = postcard::to_allocvec(&detailed).unwrap();
assert_eq!(postcard::from_bytes::<Response>(&bytes).unwrap(), detailed);
// An old-shape frame still decodes: the detailed variant is new, the old one untouched.
let old = Response::UpdateStatus { staged: None, last_error: None, unreachable_for_secs: None };
assert_eq!(postcard::from_bytes::<Response>(&postcard::to_allocvec(&old).unwrap()).unwrap(), old);
assert_eq!((UPDATE_STATUS_FROM, UPDATE_EVENTS_FROM, UPDATE_STATUS_DETAILED_FROM, DETACH_FOR_GRACE_FROM), (4, 4, 5, 5));
```

(The existing test indexes `postcard::to_allocvec(&v).unwrap()[0]` inline; add `fn first_byte<T: serde::Serialize>(v: &T) -> u8 { postcard::to_allocvec(v).unwrap()[0] }` at the top of the test module and keep the existing assertions.)

- [ ] **Step 2: Run it to see it fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p protocol`
Expected: compile errors for the missing variants and constants.

- [ ] **Step 3: Add the variants and constants**

```rust
pub const PROTOCOL_VERSION: u32 = 5;
pub const PROTOCOL_MIN: u32 = 1;
/// The first protocol with `UpdateStatus` and `Event::UpdateStaged`.
pub const UPDATE_STATUS_FROM: u32 = 4;
pub const UPDATE_EVENTS_FROM: u32 = 4;
/// The first protocol with the check's state in the status answer.
pub const UPDATE_STATUS_DETAILED_FROM: u32 = 5;
/// The first protocol where the daemon ends a closed tab's session after its grace.
pub const DETACH_FOR_GRACE_FROM: u32 = 5;
```

Append to `Request` (after `ApplyUpdate`):

```rust
    /// `UpdateStatus` with the check's state. A protocol 5 window sends this one.
    UpdateStatusDetailed,
    /// The window closed a tab: end `session` unless a window attaches it within `secs`.
    /// Answered `Ok` at once; a second request restarts the grace.
    DetachForGrace { session: SessionId, secs: u32 },
```

Append to `Response` (after `UpdateStatus`):

```rust
    UpdateStatusDetailed {
        staged: Option<StagedUpdate>,
        last_error: Option<String>,
        unreachable_for_secs: Option<u64>,
        /// A check is running now (asked for, or on the timer).
        checking: bool,
        /// When the last check finished, whatever it found; None before the first.
        last_check_unix: Option<u64>,
        /// The last check's transport failure, None once a check reaches the manifest again.
        transient_error: Option<String>,
    },
```

Update the codec assertions (`PROTOCOL_VERSION == 5`) and the doc comment on the version constants to mention protocol 5.

- [ ] **Step 4: The daemon answers both shapes and speaks the constants**

In `crates/arkosd/src/server.rs`, replace `const UPDATE_EVENTS_FROM: u32 = 4;` with `use protocol::UPDATE_EVENTS_FROM;`. Factor the existing `Request::UpdateStatus` arm's body into a helper and add the detailed arm:

```rust
/// The status both answers share: nothing not newer than what runs or is installed is offered.
fn status_parts(st: &State, running: &crate::updates::version::Version) -> (Option<StagedUpdate>, Option<String>, Option<u64>) {
    let staged = st.update.staged.as_ref().filter(|s| !crate::updates::poll::is_stale(s, running)).map(|s| StagedUpdate { version: s.version.clone(), git: s.git.clone() });
    (staged, st.update.last_error.clone(), st.update.unreachable_for_secs(crate::updates::apply::now_unix()))
}
```

```rust
        Request::UpdateStatus => {
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
        // Task 8 replaces this with the real grace; until then it detaches like `Detach`. Same
        // lock order as the `Detach` arm: the session handle through `session(sid)` first, the
        // state lock on its own afterwards, never the session's lock under the state's.
        Request::DetachForGrace { session: sid, .. } => match session(sid) {
            Some(s) => {
                s.unsubscribe(client_id);
                if let Some(rec) = state.lock().unwrap().clients.get_mut(&client_id) {
                    rec.attached.retain(|x| *x != sid);
                }
                Response::Ok
            }
            None => Response::Error(format!("no session {}", sid.0)),
        },
```

`last_check_unix` and `transient_error` do not exist yet: add them to `UpdateState` in `crates/arkosd/src/updates/mod.rs` now as `pub last_check_unix: Option<u64>` and `pub transient_error: Option<String>` (both default None); Task 2 sets them.

In `crates/arkos/src/main.rs`, the Hello handler: `if protocol_max >= protocol::UPDATE_STATUS_FROM { self.sessions.update_status(); }` and store `self.daemon_protocol_max = protocol_max;` in a new `pub daemon_protocol_max: u32` field on `Root` (initialised 0). Task 4 reads it.

- [ ] **Step 5: Run the protocol and daemon tests**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p protocol` then `nice -n 15 ~/.cargo/bin/cargo test -p arkosd -- hello_ protocol`
Expected: PASS. Update the daemon tests that assert `(protocol_min, protocol_max) == (1, PROTOCOL_VERSION)` only if they hard-code 4 (they use the constant; confirm). The window tests in `doctor.rs:258` and `chooser_sessions.rs:138` fake `protocol_max: 4`; leave them (a 4 daemon is still valid).

- [ ] **Step 6: Commit**

```bash
git add crates/protocol crates/arkosd/src/server.rs crates/arkosd/src/updates/mod.rs crates/arkos/src/main.rs
git commit -m "Protocol 5: the check's state in the status answer, a grace detach"
```

---

### Task 2: The daemon's check state, its persistence, adoption, and the stale-drop guard

**Files:**
- Modify: `crates/arkosd/src/updates/mod.rs` (`UpdateState`, `drop_stale_staged`, new `last_check` and `adopt_staged`)
- Modify: `crates/arkosd/src/updates/poll.rs` (`record_at`, `start`)
- Modify: `crates/arkosd/src/updates/apply.rs:13-22` (`old_kept`)
- Modify: `crates/arkosd/src/server.rs:663-670` (the "deleted" line)
- Test: the module tests in `mod.rs` and `poll.rs`, the daemon suite

**Interfaces:**
- Consumes: `UpdateState.last_check_unix`, `transient_error` (Task 1).
- Produces: `updates::last_check::{read, write}`, `updates::adopt_staged(state, dir, floor) -> Option<String>` (the journal line).

- [ ] **Step 1: Failing tests for `record_at` and the file**

In `poll.rs` tests, add:

```rust
#[test]
fn a_check_records_when_it_finished_and_what_failed_in_transport() {
    let mut u = UpdateState::default();
    record_at(&mut u, Outcome::Transient("offline".into()), 100);
    assert_eq!((u.last_check_unix, u.transient_error.as_deref(), u.last_ok_unix), (Some(100), Some("offline"), None));
    record_at(&mut u, Outcome::Nothing { version: "1.0.0".into(), git: "a".into(), staged: None }, 200);
    assert_eq!((u.last_check_unix, u.transient_error.as_deref(), u.last_ok_unix), (Some(200), None, Some(200)));
    record_at(&mut u, Outcome::Transient("dns".into()), 300);
    assert_eq!((u.last_check_unix, u.last_ok_unix), (Some(300), Some(200)), "a transient failure keeps the last reach");
}
```

In `mod.rs` tests (create the module if there is none), add:

```rust
#[test]
fn the_last_reach_round_trips_through_the_file_and_a_bad_file_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(last_check::read(dir.path()), None);
    last_check::write(dir.path(), 1234).unwrap();
    assert_eq!(last_check::read(dir.path()), Some(1234));
    assert!(!dir.path().join("last_check.json.tmp").exists());
    std::fs::write(dir.path().join("last_check.json"), b"{not json").unwrap();
    assert_eq!(last_check::read(dir.path()), None);
}

#[test]
fn a_bad_last_check_file_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("last_check.json"), b"{\"ok_unix\":\"soon\"}").unwrap();
    assert_eq!(last_check::read(dir.path()), None);
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkosd -- updates::`
Expected: compile errors (`last_check` missing, fields unset).

- [ ] **Step 3: Implement the state and the file**

In `mod.rs`:

```rust
pub mod last_check {
    use std::path::Path;
    #[derive(serde::Serialize, serde::Deserialize)]
    struct File { ok_unix: u64 }
    fn path(dir: &Path) -> std::path::PathBuf { dir.join("last_check.json") }
    /// When a check last reached the manifest, from before this daemon started; None when
    /// there is no readable file.
    pub fn read(dir: &Path) -> Option<u64> {
        let text = std::fs::read_to_string(path(dir)).ok()?;
        serde_json::from_str::<File>(&text).ok().map(|f| f.ok_unix)
    }
    /// Written through a temporary file and a rename, like the marker.
    pub fn write(dir: &Path, ok_unix: u64) -> std::io::Result<()> {
        let tmp = dir.join("last_check.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(&File { ok_unix })?)?;
        std::fs::rename(&tmp, path(dir))
    }
}
```

In `poll.rs` `record_at`, after the existing `last_check_transient` line:

```rust
    u.last_check_unix = Some(now_unix);
    u.transient_error = match &outcome {
        Outcome::Transient(e) => Some(e.clone()),
        _ => None,
    };
```

And in the poll thread, after `record(&mut st.update, outcome)`, while the lock is held: `let reached = st.update.last_ok_unix;` then after releasing it, `if let Some(ok) = reached.filter(|_| reached_now) { if let Err(e) = super::last_check::write(&dir, ok) { eprintln!("arkosd: cannot write last_check.json: {e}"); } }` where `reached_now` is `matches!(outcome_kind, Nothing | Staged | Failed | Skipped)` captured before `record` consumes the outcome (`let reached_now = !matches!(outcome, Outcome::Transient(_) | Outcome::Invalid(_));`). At the top of `start`, before the thread: `state.lock().unwrap().update.last_ok_unix = super::last_check::read(&dir);` (only when None, so a daemon exec that carried a newer value keeps it: `if st.update.last_ok_unix.is_none()`).

- [ ] **Step 4: Failing tests for adoption and the guard**

In `mod.rs` tests:

```rust
#[test]
fn a_newer_staged_bundle_from_before_the_restart_is_adopted_and_a_stale_one_is_not() {
    use super::stage::tests::{fake_bundle, manifest_for, zip_of};
    let dir = tempfile::tempdir().unwrap();
    let app = fake_bundle(dir.path(), "9.9.9", "abcdef1");
    let staged_dir = dir.path().join("9.9.9");
    std::fs::create_dir_all(&staged_dir).unwrap();
    std::fs::rename(&app, staged_dir.join("Arkos IDE.app")).unwrap();
    let zip = dir.path().join("x.zip");
    zip_of(&staged_dir.join("Arkos IDE.app"), &zip);
    let m = manifest_for(&zip, "9.9.9", "abcdef1");
    // `Manifest` derives only Deserialize: write the JSON the fetch writes, by hand.
    std::fs::write(dir.path().join("manifest.json"), manifest_json(&m)).unwrap();
    let state = crate::server::test_state();
    let floor = version::Version { major: 0, minor: 1, patch: 0, git: String::new() };
    let line = adopt_staged(&state, dir.path(), &floor);
    assert_eq!(line.as_deref(), Some("adopted staged 9.9.9 (abcdef1) from before the restart"));
    let st = state.lock().unwrap();
    assert_eq!(st.update.staged.as_ref().map(|s| (s.version.as_str(), s.git.as_str())), Some(("9.9.9", "abcdef1")));
    drop(st);
    // Not newer than the floor: left for drop_stale_staged, not adopted.
    let newer_floor = version::Version { major: 9, minor: 9, patch: 9, git: String::new() };
    state.lock().unwrap().update.staged = None;
    assert_eq!(adopt_staged(&state, dir.path(), &newer_floor), None);
}

#[test]
fn a_bundle_the_manifest_does_not_name_is_not_adopted() {
    use super::stage::tests::{fake_bundle, manifest_for, zip_of};
    let dir = tempfile::tempdir().unwrap();
    let app = fake_bundle(dir.path(), "9.9.9", "abcdef1");
    let staged_dir = dir.path().join("9.9.9");
    std::fs::create_dir_all(&staged_dir).unwrap();
    std::fs::rename(&app, staged_dir.join("Arkos IDE.app")).unwrap();
    let zip = dir.path().join("x.zip");
    zip_of(&staged_dir.join("Arkos IDE.app"), &zip);
    // The saved manifest names another version: the folder is not what it says it is.
    let m = manifest_for(&zip, "9.9.8", "abcdef1");
    std::fs::write(dir.path().join("manifest.json"), manifest_json(&m)).unwrap();
    let state = crate::server::test_state();
    let floor = version::Version { major: 0, minor: 1, patch: 0, git: String::new() };
    assert_eq!(adopt_staged(&state, dir.path(), &floor), None);
    assert!(state.lock().unwrap().update.staged.is_none());
    // A version rolled back before is never adopted either.
    let m = manifest_for(&zip, "9.9.9", "abcdef1");
    std::fs::write(dir.path().join("manifest.json"), manifest_json(&m)).unwrap();
    apply::reject(dir.path(), "9.9.9").unwrap();
    assert_eq!(adopt_staged(&state, dir.path(), &floor), None);
}
```

`manifest_json(&Manifest) -> Vec<u8>` is a test helper in `stage::tests` beside `manifest_for`: it formats the manifest's fields as the JSON `manifest::parse` reads (`format`, `version`, `git`, `published`, `min_protocol`, `arm64: { url, size, sha256 }`), so no `Serialize` derive is added to `Manifest`.

```rust

#[test]
fn the_stale_drop_waits_while_an_apply_is_in_flight() {
    let state = crate::server::test_state();
    let dir = tempfile::tempdir().unwrap();
    {
        let mut st = state.lock().unwrap();
        st.update.staged = Some(stage::Staged { version: "0.1.0".into(), git: "a".into(), bundle: dir.path().join("0.1.0/Arkos IDE.app") });
        st.update.applying = Some("0.1.0".into());
    }
    let floor = version::Version { major: 0, minor: 2, patch: 0, git: String::new() };
    drop_stale_staged(&state, dir.path(), &floor);
    assert!(state.lock().unwrap().update.staged.is_some(), "nothing forgotten mid-swap");
    state.lock().unwrap().update.applying = None;
    drop_stale_staged(&state, dir.path(), &floor);
    assert!(state.lock().unwrap().update.staged.is_none());
}
```

`crate::server::test_state()` is whatever helper the daemon tests already use to build a `Shared` (grep `fn test_state` or the `State::new` call in the daemon tests and reuse it; add a `#[cfg(test)] pub fn test_state() -> Shared` in `server.rs` if none exists). `stage::tests::{fake_bundle, zip_of, manifest_for}` exist (stage.rs:116-134); make them `pub(crate)` under `#[cfg(test)]`.

- [ ] **Step 5: Implement adoption and the guard**

In `mod.rs`:

```rust
/// At poll start: a release staged before this daemon started, newer than the floor, with a
/// sound bundle and named by the saved manifest, is staged again without a download. Returns
/// the journal line.
pub fn adopt_staged(state: &Shared, dir: &Path, floor: &version::Version) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("manifest.json")).ok()?;
    let m = manifest::parse(text.as_bytes()).ok()?;
    let v = m.version()?;
    if !v.is_newer_than(floor) {
        return None;
    }
    // Rolled back before: the poll's floor would refuse it too.
    if apply::newest_rejected(dir).and_then(|r| version::parse_semver(&r)).is_some_and(|(a, b, c)| !v.is_newer_than(&version::Version { major: a, minor: b, patch: c, git: String::new() })) {
        return None;
    }
    let bundle = dir.join(&m.version).join("Arkos IDE.app");
    stage::check_bundle(&bundle, &m.version).ok()?;
    let mut st = state.lock().unwrap();
    if st.update.staged.is_some() {
        return None;
    }
    st.update.staged = Some(stage::Staged { version: m.version.clone(), git: m.git.clone(), bundle });
    Some(format!("adopted staged {} ({}) from before the restart", m.version, m.git))
}
```

(`manifest::parse` takes bytes and verifies nothing: signature verification is the fetch's job; the saved manifest was verified when written. If `parse` requires the signature, use the saved `manifest.json.sig` with `verify` the way `run_check` does; the test above then writes a signed pair with the test key the suite already has.)

In `drop_stale_staged`, first thing under the lock:

```rust
    if st.update.applying.is_some() || st.update.applied.is_some() {
        return;
    }
```

In `poll::start`, before the loop (after reading `last_check`): `let floor = super::floor(&running, bundle.as_deref()); if let Some(line) = super::adopt_staged(&state, &dir, &floor) { crate::journal::note(line); }`.

In `server.rs` the refusal arm: both journal lines use `forgotten.as_ref().map(|s| s.version.as_str()).unwrap_or(&version)` in place of `version`. Add a daemon test `a_refused_apply_names_the_staged_version_it_deleted` beside `a_staged_release_not_newer_than_a_hand_installed_app_is_never_offered_or_applied`: stage 9.9.9 with the installed app at 9.9.10, send `ApplyUpdate { version: "9.9.9" }` and then `ApplyUpdate { version: "1.2.3" }` with a stale 9.9.9 staged again, and assert the journal line is `staged 9.9.9 deleted:` for both, never `staged 1.2.3 deleted:`.

In `apply.rs`: `#[serde(default)] pub old_kept: bool,` with the comment "Written as true for daemons that read it; read by none since 2026-10-15."

- [ ] **Step 6: Run the daemon tests**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkosd -- updates:: daemon`
Expected: PASS, including `a_staged_release_not_newer_than_a_restarted_daemon_is_deleted_and_never_offered` (adoption skips a stale folder) and the "deleted" substring test at daemon.rs:2031.

- [ ] **Step 7: Commit**

```bash
git add crates/arkosd
git commit -m "Updates: the check's state, last_check.json, adoption at start, a guarded stale drop"
```

---

### Task 3: A failed upgrade starts the grace clock; the journal collapses repeats

**Files:**
- Modify: `crates/arkosd/src/upgrade.rs:172-181` (`restore`)
- Modify: `crates/arkosd/src/server.rs` (the `validate` refusal near line 456; the `RelaunchClient` refusal near line 466)
- Modify: `crates/arkosd/src/updates/poll.rs` (the thread loop; a new `Collapse`)
- Modify: `crates/arkosd/src/upgrade.rs` (flush the collapse before the exec) and `crates/arkosd/src/main.rs` (flush at SIGTERM, where the manifest is written)
- Test: `poll.rs` tests, the daemon suite

**Interfaces:**
- Produces: `apply::clock_after_failed_upgrade(u: &mut UpdateState, dir, version_or_exe: &str, why: &str) -> Option<String>`; `poll::Collapse` with `fn note(&mut self, key: String, line: String, now: Instant) -> Option<String>` and `fn flush(&mut self, now: Instant) -> Option<String>`.

- [ ] **Step 1: Failing tests**

In `apply.rs` tests:

```rust
#[test]
fn a_failed_upgrade_with_an_apply_pending_starts_the_grace() {
    let dir = tempfile::tempdir().unwrap();
    let mut u = super::super::UpdateState::default();
    assert_eq!(clock_after_failed_upgrade(&mut u, dir.path(), "/x/arkosd", "exec failed"), None, "nothing applied: nothing to time");
    u.applied = Some(Applied { version: "0.3.0".into(), old_kept: true, at_unix: 1, pids_at_apply: vec![] });
    let line = clock_after_failed_upgrade(&mut u, dir.path(), "/x/arkosd", "exec failed").unwrap();
    assert_eq!(line, "the upgrade onto 0.3.0 failed: exec failed; the grace for 0.3.0 starts now");
    assert!(u.clock_running);
    assert!(u.applied.as_ref().unwrap().at_unix > 1);
}
```

In `poll.rs` tests:

```rust
#[test]
fn repeated_check_lines_are_counted_and_flushed_on_change_and_daily() {
    use std::time::{Duration, Instant};
    let t0 = Instant::now();
    let mut c = Collapse::default();
    let nothing = Repeat::NotNewer { what: "1.2.3 (abc1234)".into(), floor: "1.2.3".into(), first_line: "check: the manifest has 1.2.3 (abc1234), not newer than running 1.2.3".into() };
    assert_eq!(c.note(nothing.clone(), t0, "10:00"), Some(nothing.first_line().to_string()));
    assert_eq!(c.note(nothing.clone(), t0 + Duration::from_secs(300), "10:05"), None);
    assert_eq!(c.note(nothing.clone(), t0 + Duration::from_secs(600), "10:10"), None);
    let transient = Repeat::Transient { error: "dns".into() };
    let flushed = c.note(transient.clone(), t0 + Duration::from_secs(900), "10:15").unwrap();
    assert_eq!(flushed, "check: 2 more checks since 10:00 found 1.2.3 (abc1234), still not newer than 1.2.3");
    // The new outcome's first line is held until the flush that follows.
    assert_eq!(c.flush(), Some("check: transient, tried again at the next check: dns".into()));
    assert_eq!(c.note(transient.clone(), t0 + Duration::from_secs(1200), "10:20"), None);
    // A day of repeats writes a count without a change.
    let daily = c.note(transient.clone(), t0 + Duration::from_secs(1200 + 86_400), "10:20").unwrap();
    assert_eq!(daily, "check: transient, 2 more times since 10:15: dns");
    assert_eq!(c.flush(), None);
    // The daemon's handover flushes what is counted so far.
    c.note(transient.clone(), t0 + Duration::from_secs(1500 + 86_400), "10:25");
    assert_eq!(c.flush(), Some("check: transient, 1 more times since 10:20: dns".into()));
}
```

(`note` returns the line to write now: the first occurrence, or the count line when the outcome changes, in which case the new outcome's first line is held and returned by the next `flush`. The poll thread calls `note`, writes what it gets, then calls `flush` and writes that too if any. `Repeat` carries the pieces of the line, so no line is ever parsed back.)

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkosd -- apply:: poll::`
Expected: compile errors.

- [ ] **Step 3: Implement the clock restart**

In `apply.rs`:

```rust
/// An upgrade attempt that ended without a relaunch while an apply waits: the grace starts
/// now, so the watch rolls back in `CONNECT_GRACE` unless a new window says Hello. Returns the
/// journal line; None when nothing is applied.
#[must_use]
pub fn clock_after_failed_upgrade(u: &mut super::UpdateState, dir: &Path, _exe: &str, why: &str) -> Option<String> {
    let version = u.applied.as_ref()?.version.clone();
    if u.clock_running {
        return None;
    }
    let _ = restart_clock(u, dir)?;
    Some(format!("the upgrade onto {version} failed: {why}; the grace for {version} starts now"))
}
```

Call it from `upgrade.rs`'s `restore` (it has `state` and `opts`): after `abort(state)`, `let line = clock_after_failed_upgrade(&mut state.lock().unwrap().update, &opts.update.dir, &exe.display().to_string(), &why); if let Some(l) = line { crate::journal::note(l); }`. Same two lines in `server.rs` at the `validate` refusal and at the `RelaunchClient` refusal ("is not a arkos binary owned by this user"), with `why` the refusal text.

- [ ] **Step 4: Implement the collapse**

In `poll.rs`:

```rust
/// An outcome the check can repeat, in the pieces its journal lines are built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Repeat {
    /// `what` is "{version} ({git})", `floor` the running or installed semver, `first_line` the
    /// line as it is written today (with its "or than X staged or applied" suffix when any).
    NotNewer { what: String, floor: String, first_line: String },
    Transient { error: String },
}

impl Repeat {
    pub fn first_line(&self) -> String {
        match self {
            Repeat::NotNewer { first_line, .. } => first_line.clone(),
            Repeat::Transient { error } => format!("check: transient, tried again at the next check: {error}"),
        }
    }
    fn count_line(&self, repeats: u32, since: &str) -> String {
        match self {
            Repeat::NotNewer { what, floor, .. } => format!("check: {repeats} more checks since {since} found {what}, still not newer than {floor}"),
            Repeat::Transient { error } => format!("check: transient, {repeats} more times since {since}: {error}"),
        }
    }
}

/// Repeated journal lines from the check (the same "not newer" or the same transient failure)
/// are counted, not written: the count goes out when the outcome changes, at a handover, and
/// once a day while it keeps repeating.
#[derive(Default)]
pub struct Collapse {
    current: Option<Repeat>,
    repeats: u32,
    since: Option<std::time::Instant>,
    since_wall: String,
    pending: Option<String>,
}

const DAILY: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

impl Collapse {
    /// `wall` is the local clock as "HH:MM" (`journal::clock_hhmm()`), passed in so tests pin it.
    pub fn note(&mut self, r: Repeat, now: std::time::Instant, wall: &str) -> Option<String> {
        if self.current.as_ref() == Some(&r) {
            self.repeats += 1;
            if self.since.is_some_and(|s| now.duration_since(s) >= DAILY) {
                let out = r.count_line(self.repeats, &self.since_wall);
                self.repeats = 0;
                self.since = Some(now);
                self.since_wall = wall.to_string();
                return Some(out);
            }
            return None;
        }
        let out = match (&self.current, self.repeats) {
            (Some(prev), n) if n > 0 => {
                self.pending = Some(r.first_line());
                prev.count_line(n, &self.since_wall)
            }
            _ => r.first_line(),
        };
        self.current = Some(r);
        self.repeats = 0;
        self.since = Some(now);
        self.since_wall = wall.to_string();
        Some(out)
    }

    /// The held first line of a new outcome, else the count so far (for a handover or exit).
    pub fn flush(&mut self) -> Option<String> {
        if let Some(p) = self.pending.take() {
            return Some(p);
        }
        if self.repeats > 0 {
            let out = self.current.as_ref()?.count_line(self.repeats, &self.since_wall);
            self.repeats = 0;
            return Some(out);
        }
        None
    }
}
```

Add `pub fn clock_hhmm() -> String` to `journal.rs` (local time `HH:MM` through the same `localtime_r` the timestamp uses).

`run_check` changes: `Outcome::Nothing` becomes `Nothing { version: String, git: String, staged: Option<String> }` where `staged` is the " or than X staged or applied" suffix text (None when nothing is staged), and `run_check` no longer journals the line itself (remove the `journal::note` at poll.rs:71-75; keep the early return). This touches every `Outcome::Nothing` match: `record_at` (`Outcome::Nothing { .. }`), the poll thread, and the existing `poll.rs` tests that construct `Outcome::Nothing` (Task 2's new test already uses the struct form). In the poll thread, the two journal calls for `Nothing` and `Transient` become: build the `Repeat` (`NotNewer { what: format!("{version} ({git})"), floor: semver(&floor), first_line: format!("check: the manifest has {version} ({git}), not newer than running {}{}", semver(&floor), staged.unwrap_or_default()) }` or `Transient { error }`), then `if let Some(l) = st.update.collapse.note(r, Instant::now(), &journal::clock_hhmm()) { journal::note(l) }` followed by `if let Some(l) = st.update.collapse.flush() { journal::note(l) }`; every other outcome first flushes (`if let Some(l) = collapse.flush() { note(l) }`) and then writes its own line as today. `Collapse` lives in `UpdateState` as `pub collapse: poll::Collapse`, so `upgrade.rs` (before the exec, where the handover is written) and the SIGTERM path in `main.rs` (where the manifest is written) call `flush` and journal the line; the daemon test that pins "check: the manifest has 9.9.9 (fffffff), not newer than running" still sees the first occurrence. Add a daemon test `the_collapsed_count_is_journaled_at_the_handover`: with `--check-every-secs 1`, wait for three checks, run an Upgrade onto the same binary, and assert the journal has a `check: 2 more checks since` line before the "upgrade" line.

- [ ] **Step 5: Run the daemon suite**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkosd`
Expected: PASS, including a new daemon test `upgrade_whose_exec_fails_after_an_apply_rolls_back_after_the_grace` (model it on `upgrade_whose_exec_fails_keeps_the_old_daemon_serving` and `a_window_that_never_connects_rolls_the_bundle_back`: apply, then an Upgrade onto a non-executable path, then assert the journal has "the grace for 9.9.9 starts now" and, with the grace shortened as the rollback test does, "rolled back").

- [ ] **Step 6: Commit**

```bash
git add crates/arkosd
git commit -m "Updates: a failed upgrade starts the grace; the journal counts repeated checks"
```

---

### Task 4: The window's check outcome and the title bar

**Files:**
- Create: `crates/arkos/src/update_check.rs`
- Modify: `crates/arkos/src/sessions.rs` (`Pending::StatusAfterCheck` answers both shapes; `status_after_check` sends the detailed request when allowed)
- Modify: `crates/arkos/src/main.rs` (`CheckStarted`, `Checked`, the title-bar match, `note_update_status`, `route_events` for `UpdateStaged`)
- Modify: `crates/arkos/src/main.rs` `mod` list

**Interfaces:**
- Consumes: `Root.daemon_protocol_max` (Task 1), `Response::UpdateStatusDetailed`.
- Produces: `update_check::{Poll, Answer, Verdict, title_text}`.

- [ ] **Step 1: Failing tests in `update_check.rs`**

```rust
//! What "Check for updates" says, decided from the daemon's status answers, and which update
//! text the title bar shows. Pure, so it is tested.
use protocol::StagedUpdate;

/// One status answer, in either protocol shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub staged: Option<StagedUpdate>,
    pub last_error: Option<String>,
    pub unreachable_for_secs: Option<u64>,
    /// None for a protocol 4 daemon, which has no check state to report.
    pub checking: Option<bool>,
    pub last_check_unix: Option<u64>,
    pub transient_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Ready(String),
    Failed(String),
    Unreachable(String),
    StillRunning,
    NotReachedYet,
    NoUpdate,
}

/// The poll after a check the user asked for: `asked_at` is when the daemon accepted it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Poll {
    pub asked_at: u64,
    pub polls: u32,
}

pub const POLL_EVERY_MS: u64 = 500;
pub const MAX_POLLS: u32 = 60;

impl Poll {
    pub fn new(asked_at: u64) -> Self { Poll { asked_at, polls: 0 } }

    /// None: ask again after `POLL_EVERY_MS`. Some: the notice to show.
    pub fn answer(&mut self, a: &Answer) -> Option<Verdict> {
        self.polls += 1;
        match a.checking {
            // Protocol 4: one answer, read as today, with the never-reached case named.
            None => Some(Self::verdict(a, true)),
            Some(checking) => {
                let done = !checking && a.last_check_unix.is_some_and(|t| t >= self.asked_at);
                if done { Some(Self::verdict(a, false)) } else if self.polls >= MAX_POLLS { Some(Verdict::StillRunning) } else { None }
            }
        }
    }

    fn verdict(a: &Answer, old_daemon: bool) -> Verdict {
        if let Some(s) = &a.staged { return Verdict::Ready(s.version.clone()); }
        if let Some(e) = &a.last_error { return Verdict::Failed(e.clone()); }
        if let Some(e) = &a.transient_error { return Verdict::Unreachable(e.clone()); }
        if old_daemon && a.unreachable_for_secs.is_none() { return Verdict::NotReachedYet; }
        if a.unreachable_for_secs.is_some_and(|s| s > 0) { return Verdict::Unreachable("the last check did not reach the update server".into()); }
        Verdict::NoUpdate
    }
}

/// The title bar's update item: what can be acted on first.
pub fn title_text<'a>(ready: Option<&'a str>, error: Option<&'a str>, unreachable: Option<&'a str>) -> Option<&'a str> {
    ready.or(error.map(|_| "update failed")).or(unreachable)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn answer(checking: Option<bool>, last: Option<u64>) -> Answer {
        Answer { staged: None, last_error: None, unreachable_for_secs: Some(0), checking, last_check_unix: last, transient_error: None }
    }
    #[test]
    fn the_poll_waits_for_a_check_that_finished_after_the_ask() {
        let mut p = Poll::new(100);
        assert_eq!(p.answer(&answer(Some(false), Some(90))), None, "an older check");
        assert_eq!(p.answer(&answer(Some(true), Some(90))), None, "running");
        assert_eq!(p.answer(&answer(Some(false), Some(100))), Some(Verdict::NoUpdate));
    }
    #[test]
    fn each_outcome_has_its_notice() {
        let mut p = Poll::new(1);
        let mut a = answer(Some(false), Some(5));
        a.staged = Some(StagedUpdate { version: "1.2.3".into(), git: "g".into() });
        assert_eq!(p.answer(&a), Some(Verdict::Ready("1.2.3".into())));
        let mut a = answer(Some(false), Some(5));
        a.last_error = Some("bad hash".into());
        assert_eq!(Poll::new(1).answer(&a), Some(Verdict::Failed("bad hash".into())));
        let mut a = answer(Some(false), Some(5));
        a.transient_error = Some("offline".into());
        assert_eq!(Poll::new(1).answer(&a), Some(Verdict::Unreachable("offline".into())));
    }
    #[test]
    fn a_check_that_outlives_the_connection_times_out_quietly() {
        let mut p = Poll::new(1);
        for _ in 0..MAX_POLLS - 1 { assert_eq!(p.answer(&answer(Some(true), None)), None); }
        assert_eq!(p.answer(&answer(Some(true), None)), Some(Verdict::StillRunning));
    }
    #[test]
    fn a_protocol_4_daemon_answers_once_and_names_the_never_reached_case() {
        let mut a = answer(None, None);
        a.unreachable_for_secs = None;
        assert_eq!(Poll::new(1).answer(&a), Some(Verdict::NotReachedYet));
        assert_eq!(Poll::new(1).answer(&answer(None, None)), Some(Verdict::NoUpdate));
    }
    #[test]
    fn the_title_bar_ranks_ready_then_failed_then_unreachable() {
        assert_eq!(title_text(Some("update ready: 1"), Some("e"), Some("updates unreachable for 2 days")), Some("update ready: 1"));
        assert_eq!(title_text(None, Some("e"), Some("u")), Some("update failed"));
        assert_eq!(title_text(None, None, Some("u")), Some("u"));
        assert_eq!(title_text(None, None, None), None);
    }
}
```

- [ ] **Step 2: Run to see the module compile and tests pass (they are pure)**

Add `mod update_check;` to `main.rs`. Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- update_check::`
Expected: PASS (the module is self-contained; if `Answer` needs `StagedUpdate: Eq`, it has it).

- [ ] **Step 3: Wire the poll**

`sessions.rs`: `status_after_check(&mut self, detailed: bool)` sends `Request::UpdateStatusDetailed` when `detailed` else `Request::UpdateStatus`, both `Pending::StatusAfterCheck`. In `handle_response`, `Response::UpdateStatusDetailed { .. }` with `Pending::StatusAfterCheck` becomes `Outcome::Checked(update_check::Answer { .., checking: Some(checking), .. })`, the old shape `Outcome::Checked(Answer { checking: None, last_check_unix: None, transient_error: None, .. })`; with `Pending::Status` the detailed shape maps to `Outcome::UpdateStatus` with the three old fields (the window's badge logic is unchanged). Change `Outcome::Checked` to carry an `Answer`.

`main.rs`: `Root.check_poll: Option<update_check::Poll>`. `Outcome::CheckStarted`: `self.check_poll = Some(Poll::new(now_unix()))` and schedule one `status_after_check(detailed)` where `detailed = self.daemon_protocol_max >= protocol::UPDATE_STATUS_DETAILED_FROM`; the delay is `POLL_EVERY_MS` when detailed and today's `CHECK_RESULT_AFTER` (3 s, kept) for a protocol 4 daemon, whose single answer is final (spec section 4). `Outcome::Checked(answer)`: `note_update_status(answer.staged, answer.last_error, answer.unreachable_for_secs, true, cx)` as today, then `match self.check_poll.as_mut().map(|p| p.answer(&answer))`: `Some(None)` schedules another poll; `Some(Some(v))` clears `check_poll` and shows the notice per the Global Constraints texts (Ready: "update ready: X"; Failed: "update check failed: e", but not twice: `note_update_status` already notices `last_error` when `requested`, so for `Failed` show nothing extra); `None` (no poll running, e.g. a stray answer) shows nothing. A disconnect (`Root::disconnected`) clears `check_poll`. "Retry update" (`RetryUpdate`) goes through the same `CheckStarted` path (it already calls `check_for_updates`; make sure it reaches `CheckStarted`).

- [ ] **Step 4: Wire the title bar**

Replace the match at `main.rs:2681` with `let update_text = update_check::title_text(self.update_ready.as_deref(), self.update_error.as_deref(), self.update_unreachable.as_deref());` where `update_ready` is kept as today (`refresh_update_badge`). In the `Event::UpdateStaged` handler set `self.update_unreachable = None` (the manifest was reached). Add a window test `note_update_status_keeps_the_ready_text_through_a_failure` (in `main.rs`'s test module, where `Root` tests live): with `staged = Some(..)` then `note_update_status(None, Some("bad hash"), None, false)`, `update_ready` is still "update ready: X" when `update_ready_on_disk` is set, and `title_text` picks it.

- [ ] **Step 5: Tests and the suite**

Add to `sessions.rs` tests: `a_detailed_status_after_a_check_is_a_checked_outcome_with_its_state` (fake a `Response::UpdateStatusDetailed` for a `StatusAfterCheck` id and assert `Outcome::Checked(Answer { checking: Some(true), .. })`). Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos` (the window suite). Expected: PASS; the existing `an_accepted_check_is_followed_by_a_status_request_whose_answer_says_what_it_found` is updated for the `Answer` shape.

- [ ] **Step 6: Commit**

```bash
git add crates/arkos
git commit -m "Window: the check's notice waits for the check; the ready badge outranks failures"
```

---

### Task 5: Startup without a blocking connect; the connect-failure window; the chooser opens first

**Files:**
- Create: `crates/arkos/src/startup.rs`
- Modify: `crates/arkos/src/main.rs` (`main` from `let (client, events) = connect(&daemon_args);` through the chooser branch; `connect()` moves to `startup.rs`)
- Modify: `crates/arkos/src/launch.rs` (`open_chooser` takes `link: Option<Link>` still, plus a footer state; a `Chooser::attach_link(link, cx)` method)
- Modify: `crates/arkos/src/launch_link.rs` (`Link::new` unchanged; `start` may be called later)

**Interfaces:**
- Produces: `startup::connect_in_background(daemon_args: Vec<String>, cx: &mut App) -> Task<Result<(Client, UnboundedReceiver<ServerMessage>), String>>`; `startup::open_failure_window(error: String, retry: Rc<dyn Fn(&mut App)>, cx: &mut App)`; `Chooser::attach_link(&mut self, link: Link, cx)`; `Chooser::set_footer(&mut self, text: Option<String>, cx)`; `Link::client(&self) -> Arc<Client>`.
- Consumes: `launch_link::Link` (0.2.10), `connection::heartbeat`.

- [ ] **Step 1: The failing test for the failure window's text and retry count**

In `startup.rs`:

```rust
/// What the failure window says: the error, where the log is, and that Try again reruns it.
pub fn failure_lines(error: &str, log: &Path) -> (String, String) {
    ("Arkos cannot reach its daemon".to_string(), format!("{error}\n\nThe daemon's log is {}.", log.display()))
}

/// Try again runs one connect at a time: a second press while one runs is ignored.
#[derive(Default)]
pub struct Retry { running: bool, attempts: u32 }
impl Retry {
    pub fn begin(&mut self) -> bool { if self.running { return false; } self.running = true; self.attempts += 1; true }
    pub fn end(&mut self) { self.running = false; }
    pub fn attempts(&self) -> u32 { self.attempts }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn try_again_reruns_the_connect_once() {
        let mut r = Retry::default();
        assert!(r.begin());
        assert!(!r.begin(), "one at a time");
        r.end();
        assert!(r.begin());
        assert_eq!(r.attempts(), 2);
    }
    #[test]
    fn the_failure_window_names_the_log() {
        let (title, body) = failure_lines("started /x/arkosd but it never accepted a connection", Path::new("/l/arkosd.log"));
        assert_eq!(title, "Arkos cannot reach its daemon");
        assert!(body.contains("/l/arkosd.log") && body.starts_with("started /x/arkosd"));
    }
}
```

- [ ] **Step 2: Run to see it fail, then implement the background connect**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- startup::` (compile error until the module exists; add `mod startup;`).

```rust
/// Connects to the daemon, starting it when none answers, off the main thread. The window
/// never waits on it: the chooser opens meanwhile, or the project window once it is done.
pub fn connect_in_background(daemon_args: Vec<String>, cx: &mut App) -> Task<Result<(Client, UnboundedReceiver<ServerMessage>), String>> {
    cx.background_executor().spawn(async move {
        Client::connect_or_start(&paths::socket_path(), &paths::daemon_exe(), &daemon_args).map_err(|e| format!("{e:#}"))
    })
}
```

(`Client` must be `Send` to cross executors; it already is, since the project window hands it to background tasks in `connection.rs`. If the receiver is not `Send`, connect on the background executor and move only the `Client` and receiver back; `UnboundedReceiver` is `Send` when its item is.)

The failure window:

```rust
pub fn open_failure_window(error: String, retry: Rc<dyn Fn(&mut App)>, cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(480.0), px(220.0)), cx);
    let options = WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), titlebar: Some(TitlebarOptions { title: Some("Arkos".into()), ..Default::default() }), ..Default::default() };
    let log = client::daemon_log(&paths::socket_path());
    let _ = cx.open_window(options, move |window, cx| {
        let view = cx.new(|cx| Failure { error, log, retry, focus: cx.focus_handle() });
        window.focus(&view.read(cx).focus);
        view
    });
}
```

`Failure` renders the title, the body from `failure_lines`, and three buttons: "Open the log" runs `std::process::Command::new("open").arg(&self.log).spawn()`; "Try again" calls `(self.retry)(cx)` and closes this window; "Quit" and Escape call `cx.quit()`. Closing the window by its close button quits too: the project plan registers `cx.on_window_closed` to quit when no window is left and no project window has opened (the same handler the chooser plan already has), so the process never lingers without a window. Style it like the chooser (same font family from the config, `chrome.window_bg`, `chrome.text`, buttons as in `launch.rs`'s trailing group).

- [ ] **Step 3: Restructure `main`**

In `main`, remove the blocking `connect(&daemon_args)` and the `connects_first` assertion's premise (keep `Plan` as is; `connects_first` now documents that the Hello is sent as soon as the connection exists). Inside `Application::run`:

```rust
let startup = Rc::new(RefCell::new(Some(start)));   // Startup moves once, into whichever path opens the project window
let connecting = startup::connect_in_background(daemon_args.clone(), cx);
if plan.is_chooser() {
    // The chooser first, with no link yet; its counts arrive with the connection.
    let opened = Rc::new(Cell::new(false));
    // Quit and window-closed handlers as today (no client to wait_sent on yet: the link's
    // client, when there is one, is flushed by the chooser's own quit path).
    let chooser = launch::open_chooser(cx, true, plan.focus(), config.clone(), theme.clone(), None, on_choice);
    cx.spawn(async move |cx| {
        let handle = chooser.await;
        match connecting.await {
            Ok((client, events)) => {
                let client = Arc::new(client);
                let link = launch_link::Link::new(client.clone(), events);
                // The chooser has a ui_pid from its Hello: without heartbeats the watchdog would
                // kill it. This task beats until the project window's loop runs its own.
                let (beat_client, waiting) = (client.clone(), opened.clone());
                let _ = cx.update(|cx| cx.spawn(async move |cx| connection::heartbeat(beat_client, move |_| !waiting.get(), cx).await).detach());
                let _ = cx.update(|cx| { if let Some(h) = handle { let _ = h.update(cx, |chooser, _, cx| chooser.attach_link(link.clone(), cx)); } });
            }
            Err(e) => { let _ = cx.update(|cx| startup::open_failure_window(e, retry.clone(), cx)); }
        }
    }).detach();
} else {
    cx.spawn(async move |cx| match connecting.await {
        Ok((client, events)) => { let _ = cx.update(|cx| open_project_window(cwd, (Arc::new(client), events, Vec::new()), startup.borrow_mut().take().unwrap(), cx)); }
        Err(e) => { let _ = cx.update(|cx| startup::open_failure_window(e, retry.clone(), cx)); }
    }).detach();
}
```

Where `retry` is an `Rc<dyn Fn(&mut App)>` that runs the same spawn again (factor the "connect then open or fail" into `fn begin_connect(plan, cwd, config, theme, startup, chooser_handle, cx)` so Try again can call it; `startup::Retry` guards double presses). The chooser's `on_choice` reads the link from the chooser (`Chooser.link`) at choice time: `link.finish()` as in 0.2.10, then `open_project_window(project, (link.client(), events, backlog), ...)`; add `Link::client(&self) -> Arc<Client>`. Until a link exists, choosing a folder still works: the choice is recorded and `open_project_window` runs when the connection arrives (`Chooser` keeps `chosen: Option<PathBuf>`; `attach_link` checks it and finishes at once). The window-closed and Quit handlers call `wait_sent` on the link's client when there is one.

`Chooser::attach_link`: stores the link, starts it with the `on_link` callback (as `open_chooser` does today), sets the footer to None; before it, `open_chooser` with `link: None` at launch sets the footer "connecting to the daemon…" (not from a running window, where there is no link by design: `open_chooser` already has `at_launch`, use it). `open_chooser` computes `saved` (each recent folder's `workspace::session_ids`) when `at_launch`, not when a link is passed (today's `with_saved = link.is_some()` would leave the counts empty forever for a chooser that gets its link later). Render the footer as the notice line is rendered.

- [ ] **Step 4: Run the window suite and the warning check**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos` then `nice -n 15 ~/.cargo/bin/cargo check --workspace --all-targets`
Expected: PASS, no warnings. `launch_startup_plan_connects_first_whatever_opens` keeps passing (the plan API is unchanged); update its comment.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos
git commit -m "Startup: connect off the main thread, a window when the daemon cannot start, the chooser first"
```

---

### Task 6: The chooser reconnects, follows the preset and the appearance

**Files:**
- Modify: `crates/arkos/src/connection.rs` (extract the reconnect attempt)
- Modify: `crates/arkos/src/launch_link.rs` (reconnect loop; `client()`; the heartbeat)
- Modify: `crates/arkos/src/launch.rs` (`TEXT` from the preset; `observe_window_appearance`; the "reconnecting" footer)
- Test: `connection.rs`, `launch.rs`

**Interfaces:**
- Produces: `connection::Reconnect { lost_at: Instant, attempt: u32, start_failures: u32 }` with `fn next_delay(&self) -> Duration` and `fn should_start(&self) -> bool`, and `async fn connection::attempt(socket, daemon_exe, daemon_args, start: bool, cx) -> Result<(Client, Receiver), String>`; `Link::client()`.

- [ ] **Step 1: Failing test for the extracted policy**

In `connection.rs` tests:

```rust
#[test]
fn the_reconnect_policy_matches_the_window_loop() {
    let mut r = Reconnect::new(Instant::now() - Duration::from_secs(11));
    assert_eq!(r.next_delay(), backoff(0));
    assert!(r.should_start(), "ten seconds lost: start the daemon");
    r.failed_start();
    r.failed_start();
    r.failed_start();
    r.failed_start();
    r.failed_start();
    assert!(!r.should_start(), "five failed starts: only connect");
    r.next_attempt();
    assert_eq!(r.next_delay(), backoff(1));
}
```

- [ ] **Step 2: Implement `Reconnect` and use it in both loops**

```rust
pub struct Reconnect { lost_at: Instant, attempt: u32, start_failures: u32 }
impl Reconnect {
    pub fn new(lost_at: Instant) -> Self { Reconnect { lost_at, attempt: 0, start_failures: 0 } }
    pub fn next_delay(&self) -> Duration { backoff(self.attempt) }
    pub fn should_start(&self) -> bool { should_start_daemon(self.lost_at.elapsed(), self.start_failures) }
    pub fn next_attempt(&mut self) { self.attempt += 1; }
    pub fn failed_start(&mut self) { self.start_failures += 1; }
}
```

Rewrite the window loop's inner `loop` in `start` to use it (same behaviour, same status texts). `Link::new` gains what a reconnect needs: `Link::new(client, events, socket: PathBuf, daemon_exe: PathBuf, daemon_args: Vec<String>)` (main passes `paths::socket_path()`, `paths::daemon_exe()` and `daemon_args.clone()`). In `launch_link.rs`, when `events.next()` yields `None`: do not break; instead `on_event(LinkEvent::Lost)` (the chooser shows "reconnecting to the daemon…"), then loop: `timer(r.next_delay())`, `attempt(..)` on the background executor, on success replace `self.client` (make `client` an `Rc<RefCell<Arc<Client>>>` in `Link`) and `events`, send nothing extra (the new `Client` sends Hello at connect), then `list()` and continue reading; on failure `r.next_attempt()` (and `failed_start()` when it tried to start). `finish()` returns the current receiver, and `on_choice` opens the project window on `link.client()`. The heartbeat: the task Task 5 started beats a fixed `Arc<Client>`, which goes stale on reconnect; move it into the link (`start` spawns it, beating `self.client()` each tick; `finish` stops it) and remove the one in `main`.

- [ ] **Step 3: Font and appearance**

In `launch.rs`: replace `TEXT` with `let size = config::resolve_preset(None, &self.config.font.preset, &self.config.font.presets).1.size;` in `render` (`text_size(px(size))`). In `open_chooser`'s `open_window` closure add:

```rust
let observed = view.clone();
window.observe_window_appearance(move |window, cx| {
    let dark = theme::is_dark(window.appearance());
    observed.update(cx, |chooser, cx| chooser.set_system_appearance(dark, cx));
}).detach();
```

with

```rust
pub fn set_system_appearance(&mut self, dark: bool, cx: &mut Context<Self>) {
    if self.config.appearance.mode != config::AppearanceMode::System { return; }
    let name = theme::pick_name(self.config.appearance.mode, dark, &self.config.theme).to_string();
    if name == self.theme.name { return; }
    match theme::load(&name, &paths::themes_dir()) {
        Ok(t) => { self.theme = t; cx.notify(); }
        Err(e) => eprintln!("arkos: chooser theme: {e}"),
    }
}
```

Test (pure): `the_chooser_picks_the_other_theme_name_when_the_appearance_flips` asserting `theme::pick_name(System, true, &cfg.theme) != theme::pick_name(System, false, &cfg.theme)` for the default config, and a config test that `resolve_preset(None, "comfortable", &presets).1.size == 16.0` (if not already pinned).

- [ ] **Step 4: Run the window suite and the warning check; commit**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos` and the check.

```bash
git add crates/arkos
git commit -m "Chooser: reconnects like the window, text from the preset, follows the appearance"
```

---

### Task 7: The recents file under two writers

**Files:**
- Modify: `crates/arkos/src/recent.rs` (`note`; the test `recent_newest_first_and_deduplicated`)

- [ ] **Step 1: Failing test**

```rust
#[test]
fn two_writers_at_once_keep_both_entries() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("recent.json");
    let (a, b) = (file.clone(), file.clone());
    let ta = std::thread::spawn(move || for i in 0..20 { note(&a, Path::new(&format!("/a/{i}")), 1000 + i).unwrap(); });
    let tb = std::thread::spawn(move || for i in 0..20 { note(&b, Path::new(&format!("/b/{i}")), 2000 + i).unwrap(); });
    ta.join().unwrap();
    tb.join().unwrap();
    let list = load(&file);
    assert_eq!(list.len(), 40, "forty notes under the cap of fifty: a lost update would show as fewer");
    for i in 0..20 {
        assert!(list.iter().any(|r| r.path == Path::new(&format!("/a/{i}"))), "/a/{i} lost");
        assert!(list.iter().any(|r| r.path == Path::new(&format!("/b/{i}"))), "/b/{i} lost");
    }
}
```

(`with_noted` keeps the list's order and puts the new folder first; it does not sort by time, so the test counts presence, under the cap, rather than order.)

- [ ] **Step 2: Implement the lock**

In `note`, before `load`:

```rust
let lock_path = parent.join("recent.json.lock");
let lock = std::fs::OpenOptions::new().create(true).write(true).open(&lock_path)?;
// SAFETY: flock on a descriptor this process owns; released when `lock` drops.
let rc = unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&lock), libc::LOCK_EX) };
if rc != 0 { return Err(std::io::Error::last_os_error()); }
```

(keep `lock` alive until after the `rename`). Update `recent_newest_first_and_deduplicated` to allow the lock file: count only files not named `recent.json.lock`, or assert the set `{recent.json, recent.json.lock}`.

- [ ] **Step 3: Run and commit**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- recent::`

```bash
git add crates/arkos/src/recent.rs
git commit -m "Recents: a lock around the read, modify and write"
```

---

### Task 8: The closed-tab grace in the daemon

**Files:**
- Modify: `crates/arkosd/src/session.rs` (`SessionState.grace_deadline: Option<Instant>`, `grace_secs: Option<u32>`, `set_grace(secs)`, `grace_due() -> bool`, `grace_left() -> Option<Duration>`, `clear_grace()`, `subscribe` clears it; the handover record and `adopt`)
- Modify: `crates/arkosd/src/handover.rs`: the handover is postcard (positional) with a `format` field. Bump `FORMAT` to 3; freeze the current `SessionRecord` and `Handover` as `SessionRecordV2` and `HandoverV2` (exact copies of today's structs); the live `SessionRecord` gains `grace_left_ms: Option<u64>` and `grace_secs: Option<u32>` at the end; `handover::read` matches the format: `2 =>` decodes `HandoverV2` and converts (grace fields None), `3 =>` decodes the live shape, anything else is the error it is today. Update the `upgrade.rs` tests that hard-code the format (`assert_eq!(handover::FORMAT, 2)` becomes 3, `check_answer_is_ours("handover 2\n")` becomes `"handover 3\n"` or whatever the exact literals are; grep `handover 2` and `FORMAT, 2`).
- Modify: `crates/arkosd/src/recovery.rs`: the manifest is postcard too (`manifest.bin`, magic SKMF, `MANIFEST_FORMAT`). Bump `MANIFEST_FORMAT` to 2; freeze today's `ManifestSession` and `Manifest` as `ManifestSessionV1`/`ManifestV1`; the live `ManifestSession` gains `grace_left_secs: Option<u32>` at the end; the reader accepts `1` (decode V1, convert with None) and `2`; `manifest_of` fills the field from `grace_left()`; `recover` sets the deadline after spawning. A `manifest.bin` from 0.2.12 still recovers.
- Modify: `crates/arkosd/src/reaper.rs` (`scan` ends sessions past their deadline)
- Modify: `crates/arkosd/src/server.rs` (`DetachForGrace` sets the deadline)
- Modify: `crates/arkos/src/sessions.rs` (`detach_for_grace(session, secs)`), `crates/arkos/src/main.rs` (`close_tab`, `expire_closed`)
- Test: the daemon suite, `sessions.rs`

**Interfaces:**
- Consumes: `Request::DetachForGrace` (Task 1), `Root.daemon_protocol_max` (Task 1).

- [ ] **Step 1: Failing daemon tests**

In the daemon suite, with its helpers `Daemon::start` (options include the reaper interval and an updates dir; the journal writes only when the updates dir exists, so pass one), `d.connect()`, `c.create(..)`, `c.attach(..)`, `c.request(..)`, and the upgrade helper the `upgrade_*` tests use; model the shape on `dead_session_is_reaped_after_the_ttl_once_nobody_is_attached`:

```rust
#[test]
fn a_detached_for_grace_session_ends_after_its_grace() {
    let d = Daemon::start(DaemonOptions { reaper_every_ms: 100, updates_dir: Some(tempdir_path()), ..Default::default() });
    let c = d.connect();
    let sid = c.create("/tmp");
    c.attach(sid);
    assert_eq!(c.request(Request::DetachForGrace { session: sid, secs: 1 }), Response::Ok);
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!c.info(sid).alive, "ended after its grace");
    assert!(d.journal().contains(&format!("session {} in /tmp: not reopened within 1 s, ended", sid.0)));
}

#[test]
fn a_grace_is_cancelled_by_any_attach() {
    let d = Daemon::start(DaemonOptions { reaper_every_ms: 100, ..Default::default() });
    let a = d.connect();
    let b = d.connect();
    let sid = a.create("/tmp");
    a.attach(sid);
    assert_eq!(a.request(Request::DetachForGrace { session: sid, secs: 1 }), Response::Ok);
    std::thread::sleep(Duration::from_millis(400));
    b.attach(sid);
    std::thread::sleep(Duration::from_millis(1500));
    assert!(b.info(sid).alive, "another window's attach cancels the grace");
}

#[test]
fn a_second_grace_request_restarts_the_clock() {
    let d = Daemon::start(DaemonOptions { reaper_every_ms: 100, ..Default::default() });
    let c = d.connect();
    let sid = c.create("/tmp");
    c.attach(sid);
    c.request(Request::DetachForGrace { session: sid, secs: 1 });
    std::thread::sleep(Duration::from_millis(700));
    c.request(Request::DetachForGrace { session: sid, secs: 1 });
    std::thread::sleep(Duration::from_millis(700));
    assert!(c.info(sid).alive, "1.4 s after the first: the second restarted the clock");
    std::thread::sleep(Duration::from_millis(900));
    assert!(!c.info(sid).alive, "2.3 s after the first: ended");
}

#[test]
fn a_grace_survives_the_handover_and_the_manifest() {
    // Handover: DetachForGrace { secs: 2 }, an Upgrade onto the same binary as the
    // upgrade_* tests do, then the resumed daemon ends the session by 3 s after the request.
    // Manifest: DetachForGrace { secs: 60 }, SIGTERM as the recover_* tests do, then read
    // manifest.bin with recovery::read_manifest and assert the session's grace_left_secs is
    // Some(n) with n <= 60; then --recover as those tests do and assert the recovered session
    // carries a grace (c.info(sid).alive now, and the journal's "not reopened" line once the
    // remaining seconds pass, using secs: 2 for that half of the test).
}
```

(`DaemonOptions` names are illustrative: use the suite's actual option struct and helper names, which the implementer reads from `dead_session_is_reaped_after_the_ttl_once_nobody_is_attached` and the `upgrade_*`/`recover_*` tests. The fourth test's body follows those tests' shape literally; it is the one place the implementer adapts rather than transcribes.) Use the ten-run rule serially.

- [ ] **Step 2: Implement in the session and the reaper**

`session.rs`: in `SessionState` add `grace_deadline: Option<Instant>` and `grace_secs: Option<u32>`; `pub fn set_grace(&self, secs: u32) { let mut st = self.state.lock().unwrap(); st.grace_deadline = Some(Instant::now() + Duration::from_secs(secs as u64)); st.grace_secs = Some(secs); }`; `pub fn grace_left(&self) -> Option<Duration>` (deadline minus now, zero when past); `pub fn grace_secs(&self) -> Option<u32>`; `pub fn grace_due(&self) -> bool { self.state.lock().unwrap().grace_deadline.is_some_and(|d| d <= Instant::now()) }`; `pub fn clear_grace(&self)`; in `subscribe`, after inserting the subscriber: `st.grace_deadline = None; st.grace_secs = None;`. The handover and manifest changes are as the Files list above says (frozen V2/V1 shapes, format bumps, converters); on adopt and on recover, `Some(ms)`/`Some(secs)` sets the deadline from now.

`reaper.rs` `scan`, after `escalate_kill`:

```rust
    for s in &sessions {
        if s.grace_due() && !s.has_subscribers() && s.info().alive {
            let secs = s.grace_secs().unwrap_or(0);
            crate::journal::note(format_args!("session {} in {}: not reopened within {secs} s, ended", s.id.0, s.cwd().display()));
            if let Err(e) = s.kill() { eprintln!("arkosd: session {}: cannot end after its grace: {e:#}", s.id.0); }
            s.clear_grace();
        }
    }
```

(`kill` is the same path `Request::Kill` uses, so the escalation to SIGKILL after a second follows; `grace_secs` is the requested length, kept beside the deadline for the line.) `server.rs` `DetachForGrace`: the detach as in Task 1 plus `s.set_grace(secs)`; refuse `secs == 0` with `Response::Error("a grace of zero: use Kill")`.

- [ ] **Step 3: The window**

`sessions.rs`: `pub fn detach_for_grace(&mut self, session: SessionId, secs: u32)` sends `Request::DetachForGrace { session, secs }` with `Pending::Detach`-like handling (an `Error` surfaces as today's errors do). `main.rs` `close_tab`, the terminal branch that today calls `self.sessions.detach(session)` and pushes on `self.closed`: when `self.daemon_protocol_max >= protocol::DETACH_FOR_GRACE_FROM`, call `detach_for_grace(session, closed_stack::CLOSE_GRACE.as_secs() as u32)` instead of `detach`, and mark the `ClosedTab` as `daemon_timed: true` so `expire_closed` skips the kill for it (the daemon ends it; a window that outlives the minute must not kill a session another window reattached: it cannot, since the daemon's deadline was cleared by that attach, but the window's own kill would still land, so it is skipped). Reopening within the minute (Shift+Cmd+T) attaches, which clears the deadline. Quit still kills every closed tab's session at once.

- [ ] **Step 4: Run both suites and the check; README**

Run: `nice -n 15 ~/.cargo/bin/cargo test --workspace` (once), then the check.

README: the Updating section (what "Check for updates" says in each case; the failed upgrade's rollback; the journal's counts; a release staged before a restart is kept), the launcher paragraph (opens before the daemon answers; reconnects; the failure window), the Keyboard notes (closing a tab's minute is kept by the daemon now), and the Status rows from spec section 16.

- [ ] **Step 5: Commit**

```bash
git add crates README.md
git commit -m "The closed tab's minute lives in the daemon; README for milestone 6a"
```

---

## Self-review notes

- Spec coverage: section 3 (Task 1), 4 (Tasks 2 and 4), 5 (Task 3), 6 (Task 2), 7 (Task 4), 8 (Task 5), 9 (Task 6), 10 (Task 3), 11 (Task 2), 12 (Task 6), 13 (Task 7), 14 (Task 8), 15 (tests in each), 16 (Task 8 step 4).
- Type consistency: `Answer`, `Poll`, `Verdict` are defined in Task 4 and used only there; `Reconnect` in Task 6; `Link::client()` in Task 5 and 6 (Task 5 adds it, Task 6 uses it); `Root.daemon_protocol_max` in Task 1, read in Tasks 4 and 8; `UpdateState.{last_check_unix, transient_error}` in Task 1, set in Task 2, read by Task 1's handler; `Outcome::Nothing { version, git }` changes in Task 3 only.
- Review Focus tests: 1 and 4 in Task 4 and 5 (pure), 2 in Task 8, 3 and 5 in Task 2.
