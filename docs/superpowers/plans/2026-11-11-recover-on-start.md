# Recover on Start Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** A daemon that was killed (or ended any other way) brings its sessions back when it next starts, whoever starts it: the same ids, the histories under the divider, each Claude Code conversation resumed. Nothing new runs while the daemon is up.

**Architecture:** The files are the ones the flusher already writes (`sessions/manifest.bin`, `sessions/<id>.replay`). `recovery::recover` gains a mode: a plain start recovers the sessions and remembers the windows, `--recover` (the login item) also reopens them, `--no-recover` (sandbox, shoot, tests) does neither. A budget in `sessions/resumes` keeps a daemon that is killed again and again from starting every agent each time. The window changes by one argument: `--no-recover` in a sandbox.

**Tech Stack:** Rust, `arkosd` (`main.rs`, `server.rs`, `recovery.rs`), the daemon's integration tests (`crates/arkosd/tests/daemon.rs`), `arkos` (`main.rs`'s `daemon_args`), `arkos-shoot`.

**Spec:** `docs/superpowers/specs/2026-11-11-recover-on-start-design.md`

## Global Constraints

- Tests headless (`cargo test --workspace`), named as sentences, written before the code they check. Doc comments plain prose; no em-dashes anywhere (code, docs, commits).
- `nice -n 15 cargo ...`, never `--release`, one cargo at a time; finish with `nice -n 15 cargo check --workspace --all-targets` showing no warnings.
- Numbers from the spec: 3 resuming recoveries in any 5 minutes; the file is `sessions/resumes`, one line of seconds since the epoch per recovery.
- Copy from the spec: the held line is `── not resumed, the daemon restarted 3 times in 5 minutes: run <the quoted command> ──`, dim, under the "resumed after restart" divider; "ends every session" becomes "restarts every session" wherever `pkill -x arkosd` is suggested.
- The manifest's formats do not change: 0.2.12's `--recover` must still read what this daemon writes.

## Review Focus

1. A daemon killed in a loop (a window open, the daemon killed every time it starts): the shells come back each time, the agents are started at most three times in five minutes. Pinned in Task 3 (`a_fourth_recovery_in_five_minutes_brings_the_sessions_back_without_resuming_their_agents`).
2. A window starting the daemon must never get a second window on its folder. Pinned in Task 2 (`a_plain_start_recovers_the_sessions_and_opens_no_window`).
3. A sandbox stopped and opened again starts empty. Pinned in Task 2 (`no_recover_starts_empty_and_removes_nothing`) and Task 4 (`a_sandbox_daemon_is_told_not_to_recover`).
4. The login item's behaviour is unchanged: every existing `recover_*` test passes untouched.

## Task 1: The tests' daemon can be killed outright

**Files:** `crates/arkosd/tests/daemon.rs`

- [x] Add `Daemon::sigkill(self) -> tempfile::TempDir`: SIGKILL, wait for the exit, hand the dir back (as `sigterm` does).

## Task 2: A cold start recovers

**Files:** `crates/arkosd/tests/daemon.rs`, `crates/arkosd/src/recovery.rs`, `crates/arkosd/src/server.rs`, `crates/arkosd/src/main.rs`

- [x] Tests first, failing:
  - `a_daemon_killed_outright_and_started_again_brings_its_sessions_back_and_resumes_their_agents`
  - `a_plain_start_recovers_the_sessions_and_opens_no_window` (the folder is still in the manifest the new daemon writes)
  - `no_recover_starts_empty_and_removes_nothing`
  - `a_start_without_a_manifest_removes_the_histories_left_behind`
- [x] `recovery::Recover { Off, Sessions, All }`; `recover(state, dir, opts, reopen_windows)`: with `reopen_windows` false the manifest's folders that `window_folder` accepts and that exist go into `st.windows` with their binary and nothing is launched.
- [x] `recovery::sweep(dir, keep)`: removes each `<n>.replay` whose id is not in `keep`; called with the manifest's ids, and with none when there is no manifest. Not called for an unreadable manifest.
- [x] `server::run_with(opts, recover: Recover)`; `main.rs`: the default is `Sessions`, `--recover` is `All`, `--no-recover` is `Off`, the last given wins; only `All` treats "already listening" as success.
- [x] The four tests pass; every existing `recover_*` test passes unchanged.

## Task 3: The resume budget

**Files:** `crates/arkosd/src/recovery.rs`, `crates/arkosd/tests/daemon.rs`

- [x] Tests first, failing. Unit: `three_resuming_recoveries_in_five_minutes_use_the_budget_up`, `times_older_than_five_minutes_are_dropped_and_garbage_is_no_time`, `the_held_line_shows_the_command_as_it_would_have_been_typed`. Integration: `a_fourth_recovery_in_five_minutes_brings_the_sessions_back_without_resuming_their_agents` (write three recent times to `sessions/resumes` before the start).
- [x] `RESUME_LIMIT = 3`, `RESUME_WINDOW = 5 min`, `resumes_path(dir)`, `recent_resumes(text, now) -> Vec<u64>`, `held_line(argv) -> Vec<u8>`.
- [x] In `recover`: when the manifest has a session with a typeable resume command, read the file once; under the limit, write the kept times and now, and type as today; at the limit, type nothing and end each such session's seed with `held_line`.
- [x] The tests pass.

## Task 4: The window and the shoot

**Files:** `crates/arkos/src/main.rs`, `crates/shoot/src/main.rs`

- [x] Test first: `a_sandbox_daemon_is_told_not_to_recover` (a sandbox's `daemon_args` carry `--no-recover`, a real window's do not, and the login item's arguments are the real window's).
- [x] `daemon_args` adds `--no-recover` in a sandbox; the shoot's daemon is started with it.

## Task 5: The words

**Files:** `crates/arkosd/src/server.rs`, `crates/arkos/src/{main,connection,agent_cli,config}.rs`, `crates/arkosd/tests/daemon.rs`, `docs/guide.md`, `docs/development.md`, `docs/superpowers/specs/2026-10-14-update-in-place-design.md`

- [x] "ends every session" becomes "restarts every session" in the `pkill -x arkosd` hints and the tests that quote them.
- [x] `docs/guide.md`: "After a restart" says a daemon that was killed recovers when it next starts, what the login item adds, and the resume budget; "Updating" says a lost daemon's tabs come back resumed.
- [x] `docs/development.md`: Build and run's `pkill` lines; Status: the `pkill -x arkosd` row's result, and two new rows (`pkill -9 -x arkosd` with the window open; the window killed too and Arkos opened again).
- [x] The default config's `[recovery]` and `[daemon]` comments; the update-in-place spec's non-goal points here.

## Task 6: Finish

- [x] `nice -n 15 cargo test --workspace`
- [x] `nice -n 15 cargo check --workspace --all-targets`, no warnings
