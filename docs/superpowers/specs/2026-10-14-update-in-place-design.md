# Sarkos IDE: updating in place without losing sessions

**Status:** design approved in conversation on 2026-10-14; this document is the written spec for review before the implementation plan. Builds on `2026-10-13-sarkos-design.md` (the main spec), whose section 2 process architecture it extends. Scheduled as its own milestone immediately after milestone 2b and before milestone 3, and required before the Homebrew step.

## 1. Why this exists

Every IDE Cyrus has used (Cursor, VS Code and others) ends the Claude Code sessions running in its terminals when it updates: the app must quit, so every shell and every agent inside it dies, and the sessions have to be recovered by hand afterwards. Their employer requires updates within seven days of release, so this happens often and is disruptive enough that updates get delayed. The requirement is simple: **updating Sarkos IDE, including its daemon, never ends a session.**

The main spec already puts every session in `sarkosd`, a separate process the window attaches to, so a window update (quit, replace the binary, relaunch) keeps every session today. What is missing is replacing the daemon itself without killing it, and making the whole update one command.

### 1.1 Non-goals

- Surviving a daemon crash, a logout or a reboot. Section 2.1 of the main spec still holds: nothing survives daemon death. This milestone is replacement without death.
- Downloading updates. Homebrew (`brew upgrade`) puts the new binaries on disk; Sarkos IDE notices and switches to them.
- Updating while a session is mid-keystroke with zero latency. A handover pauses output for well under a second; the kernel buffers it.

## 2. Mechanism: exec-in-place handover

The daemon replaces itself with the new binary by `execve`, in the same process. The process id does not change, so the shells stay its children, `waitpid` keeps working for exit detection, and the PTY master descriptors and the listening socket pass straight through to the new program. Everything else (threads, client connections, memory) is rebuilt from a snapshot the old daemon writes just before the exec.

The alternative, handing descriptors to a fresh process over a Unix socket (`SCM_RIGHTS`), reparents the shells to launchd. The new daemon could then never reap them, and exit detection would fall back to PTY EOF, which milestone 2a showed is unreliable under a background job. Exec-in-place is the design because it keeps the parent relationship.

### 2.1 Steps, in the old daemon

1. Receive `Request::Upgrade { exe: PathBuf }` from a client (section 4 says who sends it and when). Validate `exe`: a regular file named `sarkosd`, owned by the current user, executable. Run it as `sarkosd --check-handover` (it prints its protocol range and exits 0) to prove it starts at all before anything is touched. Refuse with `Response::Error` otherwise; the old daemon keeps running.
2. Stop accepting new clients and stop the reader threads (each session's PTY reader parks; bytes arriving meanwhile wait in the kernel's PTY buffer, and a shell that fills it blocks on `write` for the duration).
3. Write the handover file to `~/Library/Application Support/sarkos/handover-<pid>.bin`: a header (format version, daemon version, the listening socket's fd number, the watchdog and replay settings in force), then one record per session: id, cwd, pid, alive flag and exit code if ended, exit time, title, Claude state, last activity, the master fd number, the window size last set, and the replay ring's bytes. Written with `sync_all` and renamed into place.
4. Clear `FD_CLOEXEC` on every master fd and the listening socket. Every other descriptor keeps it.
5. `execve(exe, ["sarkosd", "--resume", <handover path>], env)`. If `execve` returns, it failed: restore `FD_CLOEXEC`, restart the readers, delete the file, answer the client with the error. Nothing was lost.

### 2.2 Steps, in the new daemon (`--resume`)

1. Read and validate the handover file before touching any descriptor; an unreadable or wrong-version file is fatal, and the exit is logged, because at this point the old program is gone (this is why step 1 above runs the new binary first).
2. Rebuild every session around its inherited fd: set `FD_CLOEXEC` again, re-create the reader thread, the input writer and the replay ring from the record. A session whose record says it ended stays ended. For each live session, `waitpid(pid, WNOHANG)` once, since a shell may have exited during the swap.
3. Adopt the listening socket fd and start accepting. Delete the handover file.
4. Clients reconnect on their own (section 3). Their first `Attach` replays the ring as always.

Target: under one second for four sessions with full 16 MB rings, dominated by writing and reading the file.

### 2.3 Changes to the daemon this needs

- **Children by pid.** `std::process::Child` cannot survive an exec. The session keeps the pid and uses `libc::waitpid(pid, WNOHANG)` for `poll_exit` and reaping, and `libc::kill` as today. The reaper from milestone 2a is unchanged in behaviour.
- **A thin PTY layer.** portable-pty builds its master type privately and cannot adopt an existing descriptor. The daemon replaces it with about a hundred lines over libc: `openpty`, spawn through `std::process::Command` with `pre_exec` (`setsid`, `TIOCSCTTY`, the slave as stdin, stdout and stderr), `TIOCSWINSZ` for resize, and `adopt(fd)` for `--resume`. portable-pty leaves the dependency list.
- **Protocol additions.** `Request::Upgrade { exe }`, `Request::RelaunchClient { exe, cwd }` (section 4), and a `Response::Hello { daemon_version, protocol_min, protocol_max }` to the `Hello` request, all appended as new variants (section 5 says why order matters).
- **Socket adoption.** `server::run` accepts an already-open listener.

## 3. The window reconnects by itself

Today a dropped daemon connection shows a banner and stays dead. From this milestone the window reconnects: 250 ms, then doubling to 2 s, for as long as the window is open. On reconnect it sends `Hello`, lists sessions and runs the existing reconcile path, so every tab reattaches with a replay (the same redraw as after a UI crash today). Tabs show "reconnecting" in their busy mark meanwhile. The banner appears only after 10 s without a connection, and says what the window is doing. A session that is gone after the reconnect becomes a fresh shell with the "(restarted)" note, as today.

## 4. The update, end to end

1. `brew upgrade sarkos` (or any copy of new binaries into place) replaces `sarkos` and `sarkosd` on disk. Running processes keep running the old files (the old inodes stay open), so nothing changes yet.
2. The window checks for an update on launch, on focus, and every five minutes: it runs the binary at its own launch path with `--version` and compares to its own version. On macOS Rust's `std::env::current_exe` returns the path the process was launched through, unresolved (`_NSGetExecutablePath` with no canonicalisation), so for a Homebrew install that is the `/opt/homebrew/bin/sarkos` symlink Homebrew repoints on upgrade; the window must not resolve it, since the resolved Cellar path is the old version's and `brew cleanup` deletes it. A newer version shows **"update ready: 0.4.0"** in the status bar and adds the palette command **"Update Sarkos IDE now"**.
3. The command does, in order:
   - If the daemon's version (from `Response::Hello`) differs from the on-disk `sarkosd` next to the new `sarkos`: send `Upgrade { exe }` and wait for the reconnect (section 3). The status bar says "updating daemon".
   - Send `RelaunchClient { exe: <new sarkos>, cwd: <project> }`: the daemon notes it, the window flushes its workspace file and quits, the daemon sees the client disconnect and spawns the new window in that cwd (the watchdog already knows how to launch a window; this reuses it). A clean quit never triggers the watchdog's own relaunch, as today.
4. The new window connects, reattaches every tab, and the shells never noticed. Every open window does the same when it runs the command; a window that does not run it keeps working on the old version until it is relaunched.

The order matters: the daemon updates first, so a window is never newer than its daemon (section 5).

## 5. Protocol compatibility

The wire format is postcard, which is not self-describing: a struct field or an enum variant inserted in the middle breaks every older peer. Rules from this milestone on:

- New enum variants are appended, never inserted; existing structs never change shape. A message that needs new fields is a new variant.
- `Hello` carries the window's protocol version; the daemon answers `Response::Hello` with the range it speaks. A daemon refuses a window newer than its maximum with an error naming the fix ("update the daemon: run Update Sarkos IDE now"), which the update command performs. A daemon serves any window at or above its minimum.
- `PROTOCOL_VERSION` is bumped only for a breaking change, which the rules above make rare. The current version 1 daemon does not speak `Upgrade`; the first upgrade to this milestone's daemon is the one time `pkill -x sarkosd` is still needed, and the README says so.

## 6. Versions

`sarkos --version` and `sarkosd --version` print the crate version and the short git hash the binary was built from. The status bar's update notice shows the crate version. The handover file records both so a resume from a mismatched build is refused.

## 7. Failure modes

| Failure | Behaviour |
|---|---|
| New `sarkosd` does not start (`--check-handover` fails) | Old daemon keeps running; error banner in the window |
| `execve` fails | Old daemon restores its readers and keeps running; error banner |
| New daemon rejects the handover file | Fatal: logged, sessions lost. Guarded by the check above and by the file's format version; the integration test covers a corrupt file by running the resume path on it in a daemon with no real sessions |
| A shell exits during the swap | `waitpid` at resume catches it; `Exited` is sent on the next attach |
| Window quits for relaunch but the new `sarkos` fails to launch | The daemon logs it and relaunches the old binary path from the last `Hello`, so a window comes back |
| Update command while another window is mid-update | `Upgrade` is refused with "already upgrading" until the resume completes |

## 8. Testing

- Unit: handover file round trip (header, records, rings); the libc PTY layer spawns a shell, resizes it, reads output, adopts its own fd; children by pid detect exit under a held PTY (the milestone 2a test, re-pointed).
- Integration (`crates/sarkosd/tests/daemon.rs`): start a daemon, create a session, type a command, send `Upgrade` pointing at the same binary; assert the pid is unchanged, `Attach` replays the earlier output, typing after the upgrade works, and `exit` is detected and reaped afterwards. A second test sends `Upgrade` with a bogus path and asserts the daemon still answers and the session still runs. A third sends `Upgrade` while the previous one is in flight.
- Window: the reconnect loop against a daemon that is killed and restarted by the test harness; the version comparison and the status text are pure functions with unit tests.
- Manual (README checklist): `cargo build --release` a new version into place while four sessions run `claude`, run "Update Sarkos IDE now", and confirm every tab comes back with its history and the agents keep working.

## 9. Security

The daemon execs only a path given by a same-user client over the 0600 socket, and only after checking it is a regular file named `sarkosd` owned by that user. The handover file is created 0600 in the sarkos directory and deleted as soon as it is read. No new privileges, no network.

## 10. Where it sits

A milestone of about six tasks between 2b and 3: children by pid; the PTY layer; the handover (daemon side, with `--check-handover` and `--resume`); the window's reconnect; update detection, the status notice and the palette command with `RelaunchClient`; protocol negotiation and the version strings. The Homebrew tap comes after every milestone, as decided, and is what makes step 1 of section 4 a single command.
