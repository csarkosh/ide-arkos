# Telemetry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Arkos sends three things, each with its own consent: a daily update check through a first-party path (no consent), a per-session journey keyed by an id that dies with the process (opt-in, recorded locally by default), and panic reports sent after a dialog that shows the bytes. Nothing identifies a person or a machine; the disclosure page is true of the code and ships inside the app.

**Architecture:** The client half is lib-visits's `visits-app` crate (git tag `v0.3.0`): the report types, the consent `mode()`, the journal with its allowlist, sealing, the queue with its backoff, the daily `Check`, `view`. Arkos adds what the crate leaves to the program: the allowlist file `telemetry/events.toml`, the event sites, the focus clock, the daemon as the only uploader and sealer (it has curl), the first-party routing of the first update check of the day, the consent surfaces (config, CLI, banner, Settings, the crash dialog, the disclosure page), the panic hooks, and the release changes (a channel file in the bundle, dSYMs published). Protocol 7 carries the session id in `Hello` and a `TelemetryStatus` request.

**Tech Stack:** Rust, gpui, `visits-app` (serde, toml), the system curl through the daemon's existing `Fetcher` pattern, `backtrace` for frames.

**Spec:** `docs/superpowers/specs/2026-10-16-telemetry-design.md` (approved); the wire format is lib-visits `spec/SPEC.md` section 6; the crate's loop is its `lib.rs` docs.

## Global Constraints

- Build hygiene (binding on this laptop): every cargo call `nice -n 15 ~/.cargo/bin/cargo ...` with the worktree's `.cargo/config.toml` (jobs 3, shared target dir) copied from the main checkout and never committed; no `--release`; one cargo process at a time (check `pgrep -x cargo`, wait if one runs). Focused tests while iterating; once before committing `nice -n 15 ~/.cargo/bin/cargo test --workspace --no-fail-fast` and `nice -n 15 ~/.cargo/bin/cargo check --workspace --all-targets` (no warning from our crates). Known timing flakes under load, rerun alone and report both results: `concurrent_spawns_give_each_shell_only_its_own_terminal`, `the_grace_counts_from_the_relaunch_so_a_slow_relaunch_keeps_the_old_bundle`, `recovery::tests::a_death_rewrites_the_manifest_without_the_dirty_mark`, `continuous_reader_survives_a_flood`.
- Never launch the Arkos app binary; headless tests only. Nothing in a test touches the network: the daemon's transport is tested against a `std::net::TcpListener` the test owns, and the first-party check against a `file://` redirect site like the existing update tests.
- Privacy rules that bind every task: no hostname, username, home directory, path, file name, project name, environment variable, prompt, terminal content, agent transcript, IP or persistent id in any event, report, log line or journal. Every event value is one of the allowlisted names in `telemetry/events.toml`; a value that is not in it is dropped and counted. The daemon is the only process that uploads. A `source` build never uploads. `DO_NOT_TRACK`, `ARKOS_TELEMETRY=off`, `CI` and a non-interactive session keep everything local.
- The release workflow and the shoot stay green; dev builds and the shoot are `source` and send nothing.
- No em-dashes anywhere. Comments are full sentences in the files' existing voice. `docs/development.md` Status gains the hand checks owed.
- Commit trailers (after a blank line): `Co-Authored-By: Claude <model> <noreply@anthropic.com>` with the implementer's model name, and `Claude-Session: https://claude.ai/code/session_015xrVD54zbvkgksJUndxfpr`.

## Shared definitions

- **Paths.** `telemetry_dir()` = `paths::app_support().join("telemetry")` (`~/Library/Application Support/arkos/telemetry`): the crate's `journal/`, `queue/`, `state.toml`, `last-session.json`; Arkos adds `incidents/` (crash reports awaiting the dialog), `asked` (the invitation was answered) and `launches` (a counter file, the launch count, for "the second launch").
- **Envelope.** `app = "arkos"`, `ver = env!("CARGO_PKG_VERSION")`, `bin = "app" | "daemon"`, `os = macos`, `osv` = the major of `sw_vers -productVersion` read once at startup (0 if unknown), `arch` from `std::env::consts::ARCH` (`aarch64` is `arm64`), `ch` = `release` when the running bundle has `Contents/Resources/channel` containing `release`, else `source`.
- **Endpoint.** `https://apps.csarko.sh/a` for reports, `https://apps.csarko.sh/a/check` for the check, user agent `arkos/<ver>`; both overridable for tests by the daemon flags `--telemetry-url <base>` (a `file://` base is accepted for the check only) and the existing `--curl`.
- **Config.** `[telemetry] usage = "off" | "local" | "on"` (default `local`), `crashes = "never" | "ask" | "always"` (default `ask`); `TelemetryConfig` with defaults and `deny_unknown_fields`, read by both binaries from `paths::config_file()`.
- **Environment.** `ARKOS_TELEMETRY` is the crate's `<APP>_TELEMETRY` (`off`, `log`); `DO_NOT_TRACK`; `CI`. `interactive` is true for the app and for the daemon it starts (installed for a person); false when `arkos` runs as a CLI under a non-TTY.
- **The allowlist** `telemetry/events.toml` (compiled in with `include_str!`): the spec's section 2.2 table, verbatim, plus `[focus] surfaces = ["terminal", "viewer", "browser", "settings"]`.
- **Protocol 7.** `Request::Hello` gains `sid: Option<String>`; `Request::TelemetryStatus` and `Response::TelemetryStatus { usage, crashes, uploads, reason, queued, journals, last_check_day, sent, rejected }`; `TELEMETRY_FROM: u32 = 7`. A protocol 6 window sends no `sid`, and the daemon seals nothing for it.
- **Incidents.** `incidents/<inc>.json` is the bytes of `Crash::to_bytes`, written by the panic hook; the dialog moves a sent one into the queue with `Queue::enqueue_crash` (re-parsed into a `Crash`), or deletes it.
- **Events** are recorded only in the app; the daemon records none (its crashes are incidents).

## Review Focus

1. A `source` build (every dev build, the shoot) never reaches the transport and never routes a check first-party: `mode()`'s source rule, `Check::next`'s `None`, and the collector's 403 are three independent guards. Pinned in Tasks 3 and 4.
2. An event site can never carry a value outside the allowlist: the journal rejects and counts it, and a test enumerates every call site's literal values against `events.toml`. Pinned in Task 5.
3. The first-party check is at most once a day, falls back to GitHub on any failure in the same check, and never delays an update. Pinned in Task 3.
4. Nothing uploads under the default config, under `DO_NOT_TRACK`, under `ARKOS_TELEMETRY=off`, under `CI`, or from a non-interactive CLI; a crash queued under `always` stops uploading once `crashes` is `never`. Pinned in Task 4.
5. The invitation shows once, on the second launch of a release build, interactive, usage `local`, not yet asked; never on the first launch; never again after either button. Pinned in Task 6.
6. The crash dialog's "Always send" starts unchecked; Send is consent for one report; Don't send deletes; nothing is sent while the dialog is open. Pinned in Task 7.
7. The disclosure page lists every field and every event of `events.toml`, generated from them so it cannot drift, and ships inside the app. Pinned in Task 6.

---

### Task 1: The crate, the allowlist and the envelope

**Files:**
- Modify: `Cargo.toml` (workspace dependency `visits-app = { git = "https://github.com/csarkosh/lib-visits", tag = "v0.3.0" }`), `crates/arkos/Cargo.toml`, `crates/arkosd/Cargo.toml` (both use it), `Cargo.lock`
- Create: `telemetry/events.toml`, `crates/telemetry/` (a small shared crate `arkos-telemetry`: `envelope(bin: Bin) -> Envelope` with `osv` and the channel; `telemetry_dir()`; `TelemetryConfig` and its TOML section parsed from the config file; `mode_for(config, interactive) -> Mode` wrapping `visits_app::mode` with `ARKOS_TELEMETRY`; `allowlist() -> &'static Allowlist` from the compiled file; `channel_of(bundle: &Path) -> Channel` pure)
- Modify: `crates/arkos/src/config.rs` (`[telemetry]` section through the shared crate's type, with the default config comment lines)

- [ ] **Step 1: Tests** (RED): `events.toml` parses through `Allowlist::from_toml` and lists every event of the spec's table with every value; `channel_of` on a folder with and without the `channel` file; `mode_for` with the default config gives usage `Local`, `uploads: false`, `invite` true when interactive and release; an unknown key under `[telemetry]` is a config error like any other section.
- [ ] **Step 2: Implement.** `osv` is read once with `sw_vers` and cached; failure gives 0.
- [ ] **Step 3: Gates; commit** `Telemetry: the visits-app crate, the event allowlist and the envelope`.

### Task 2: Protocol 7

**Files:**
- Modify: `crates/protocol/src/lib.rs` (`PROTOCOL_VERSION = 7`, `TELEMETRY_FROM`, `Hello.sid: Option<String>` with `#[serde(default)]`, `Request::TelemetryStatus`, `Response::TelemetryStatus {..}`), `crates/arkosd/src/server.rs` (`Hello` stores `rec.sid`; `TelemetryStatus` answers from the daemon's telemetry state), `crates/arkos/src/client.rs` (`Hello` sends the app's `sid`)

- [ ] **Step 1: Tests** (RED): the round trip of `Hello` with and without `sid` (a protocol 6 encoding decodes with `None`); `TelemetryStatus` round trip; the daemon's handler answers for a window on protocol 6 too.
- [ ] **Step 2: Implement.** **Step 3: Gates; commit** `Protocol 7: the session id in Hello and a telemetry status`.

### Task 3: The daemon's state and the first-party check

**Files:**
- Create: `crates/arkosd/src/telemetry/mod.rs` (the daemon's telemetry: `Shared` state with the `Mode`, the dir, the envelope; `State::update` around every crate call), `crates/arkosd/src/telemetry/check.rs`
- Modify: `crates/arkosd/src/updates/fetch.rs` (`Fetcher` gains `first_party: Option<String>` and a `user_agent`; `allowed_final_url` accepts a requested first-party host whose final url is GitHub's; curl gets `--user-agent`), `crates/arkosd/src/updates/poll.rs` (before each check: `Check::next(env, state, now)`; when `Some`, the manifest is fetched from `first_party + "/check?" + check.to_query()` and the state is `checked` whatever the answer; any failure of that fetch falls back to the GitHub url in the same check; `None` means GitHub as today), `crates/arkosd/src/main.rs` (`--telemetry-url`)

- [ ] **Step 1: Tests** (RED): with a `file://` redirect site like the existing update tests, the first check of a day goes first-party and the second goes to GitHub; a first-party failure falls back in the same check and the state still records the day; a `source` envelope never goes first-party (Review Focus 1 and 3); the query carries `rd` -1 then the day difference; `allowed_final_url` with the first-party start.
- [ ] **Step 2: Implement.** **Step 3: Gates; commit** `The daemon routes the first update check of a day through the first-party path`.

### Task 4: The daemon seals and uploads

**Files:**
- Create: `crates/arkosd/src/telemetry/upload.rs` (the `Transport` through curl: `--proto =https`, `--max-time 10`, `--user-agent`, `--data-binary @file`, `-o /dev/null -w %{http_code}`, the host pinned to the configured base; the queue loop every 15 minutes on the daemon's existing timer thread or a thread of its own; `State::update` applies the outcome), `crates/arkosd/src/telemetry/seal.rs` (sealing: on `Goodbye` with `End::Quit`; on a window's socket closing without one (`session.rs` around line 105, `set_ui(false)`) with `End::Lost`; at daemon start `seal_stale` with a day; only for windows that sent a `sid`), `crates/arkosd/src/telemetry/panic.rs` (the daemon's panic hook writing `incidents/<inc>.json` from `Crash` with the remapped file and line, the kind `other` and the message hash, the uptime bucket, frames from `backtrace::Backtrace` as (image index, offset) with the images list of the two Arkos binaries only; then the previous hook)
- Modify: `crates/arkosd/src/main.rs` (install the hook first thing; start the loop; `--telemetry-url`), `crates/arkosd/src/server.rs` (Goodbye and the lost path call seal), `Cargo.toml` (`backtrace`)

- [ ] **Step 1: Tests** (RED): the transport against a `TcpListener` answering 204, 429 and 400 (the queue outcomes follow the crate); the loop under the default config calls nothing (Review Focus 4, one test per environment rule through `mode_for`); a queued crash stops once `crashes` is `never`; sealing on Goodbye gives `quit`, on a lost socket `lost`, at start `unknown`; the panic hook's file validates through `Crash::to_bytes` and carries no path outside `/src/` (a subprocess test that panics on purpose with `ARKOS_TELEMETRY_DIR` pointing at a temp dir).
- [ ] **Step 2: Implement.** **Step 3: Gates; commit** `The daemon seals sessions, uploads the queue, and writes its own crash reports`.

### Task 5: The app records

**Files:**
- Create: `crates/arkos/src/telemetry.rs` (the recorder: `Recorder` holding the `Journal` when `mode.usage != Off`, `sid` from `new_id()`, the focus clock (`focus(kind)` on every focus change with the elapsed seconds since the last), `mark` once a minute from the window's existing timer and at quit, `event(name, d)` with typed helpers per event so a call site cannot spell a value wrong (`pane_open(kind, how)`, `view(format, mode)`, `md(feature)`, `agent_launch(agent, how)`, ...), the app's panic hook like the daemon's with `bin = app`)
- Modify: the event sites, one call each: `main.rs` (launch with `how`, window open and close, help, settings changes, theme, notices, errors), `pane_view.rs` (pane open and close with the kind and how), `viewer.rs` (view with format and mode), `md_view.rs` (md features, first use per session), `launch.rs` (agent launch), `sessions.rs` (agent waiting, once per terminal), `command_palette.rs` (palette command), `update.rs` (update steps), `client.rs` (the `sid` in Hello)

- [ ] **Step 1: Tests** (RED): a test that walks every typed helper's possible values against `events.toml` (Review Focus 2); the focus clock across three focus changes; `md` features recorded once per session; the recorder under usage `Off` writes no journal; the first-launch and second-launch counter.
- [ ] **Step 2: Implement.** **Step 3: Gates; commit** `The app records its session: events, focus time, and its own crash reports`.

### Task 6: Consent: config, CLI, the invitation, Settings, the disclosure

**Files:**
- Create: `docs/telemetry.md` (the nine items from the spec's section 4, generated: `crates/telemetry/src/disclosure.rs` renders the field table from `events.toml` and lib-visits's section 6 into the page at build time through a test that fails when `docs/telemetry.md` is stale, like a snapshot; the prose parts are hand-written in the file around the generated tables), `crates/arkos/src/telemetry_cli.rs` (`arkos telemetry status|on|off|local|view [--last|<path>]|crashes never|ask|always`, through the shared crate and `view`; `status` asks the daemon `TelemetryStatus` when it runs)
- Modify: `crates/arkos/src/main.rs` (the subcommand; the Help menu item "What Arkos Sends" opening `docs/telemetry.md` in a viewer tab from the compiled text, like the Documentation item), `crates/arkos/src/settings.rs` (a `Privacy` section: Usage reports Off / Local only / On with the mode's reason line; Crash reports Never / Ask each time / Always; What Arkos sends; Last report; Reports: the incidents list by date with their `inc`), `crates/arkos/src/notices.rs` or a new banner in `pane_view.rs` (the invitation strip: text, Turn on, Not now, What is sent; shown on the second launch per Review Focus 5; writes `asked`), `crates/arkos/src/agent_cli.rs` (`settings get|set telemetry.*`), `docs/agent-guide.md` (a short section: the agent answers from the disclosure and never changes the setting on its own), `crates/arkos/src/doctor.rs` (a row: the mode, its reason, the queue)

- [ ] **Step 1: Tests** (RED): the invitation's conditions table (Review Focus 5); the CLI's parsing and its outputs on a temp dir; the disclosure snapshot test (Review Focus 7); the Settings rows' labels and their config edits (like the existing Wide tables row test); the doctor row text.
- [ ] **Step 2: Implement.** **Step 3: Gates; commit** `Consent: the telemetry config, command, invitation, Settings section and disclosure page`.

### Task 7: The crash dialog

**Files:**
- Create: `crates/arkos/src/crash_dialog.rs` (a window of the About window's family: one sentence with the date and which binary; a Show report disclosure rendering the exact bytes monospace; Don't send (deletes), Send (moves to the queue through `enqueue_crash`), the unchecked Always send box (sets `telemetry.crashes = "always"` through the config writer `settings_cli` uses); several incidents are one dialog with a count and one decision for all; shown at the next interactive launch when `incidents/` is not empty and crashes are `ask`; with `never` incidents are deleted at launch; with `always` they are queued at launch without a dialog; with uploads off for an environment reason the dialog is not shown and incidents wait)
- Modify: `crates/arkos/src/main.rs` (open it at startup after the window), `crates/arkos/src/settings.rs` (the Reports list reads `queue/crash-*` and the state's counts)

- [ ] **Step 1: Tests** (RED): the pure decision `crash_dialog::decide(crashes, mode, incidents) -> Decision` (`Show(n)`, `QueueAll`, `DeleteAll`, `Wait`) for every combination (Review Focus 6); the moves on a temp dir; the "always" write to the config.
- [ ] **Step 2: Implement.** **Step 3: Gates; commit** `The crash dialog: show the bytes, send or not, and Always send unchecked`.

### Task 8: The release: the channel file, the symbols, the remapped paths

**Files:**
- Modify: `packaging/bundle.sh` (writes `Contents/Resources/channel` with `release`), `.github/workflows/release.yml` (`RUSTFLAGS="--remap-path-prefix=$PWD=/src --remap-path-prefix=$HOME/.cargo=/cargo"` on the build; `split-debuginfo = "packed"` under `[profile.release]` in `Cargo.toml`; a step zipping `target/release/arkos.dSYM` and `arkosd.dSYM` as `Arkos-IDE-<version>-dSYM.zip` into `dist/` and publishing it with the release), `docs/development.md` (how to symbolicate a report: `atos -o <dSYM> -l <base> <offset>` from the images list)

- [ ] **Step 1:** a test in the packaging script's style if one exists, else a dry run of `bundle.sh` against a fake `target/release` in a temp dir checking the channel file. **Step 2: Implement. Step 3: commit** `Release: the channel file, published dSYMs, and paths remapped out of the binaries`.

### Task 9: Docs and status

**Files:**
- Modify: `README.md` (one paragraph: what is sent, off until turned on, the link to `docs/telemetry.md`), `docs/development.md` Status (the hand checks owed: a real panic in a release build symbolicated against the published dSYM; the first-party check row seen in BigQuery with `rd` right on the second day; the shoot's source build sending nothing; the invitation on the second launch; the dialog), `docs/agent-guide.md` (already in Task 6; check it reads well), the release note text for 0.5.0 in `docs/` if the repository keeps one

- [ ] **Step 1: Write. Step 2: commit** `Docs: telemetry in the README, the status, and the hand checks owed`.
