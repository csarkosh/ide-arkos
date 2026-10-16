# Telemetry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Arkos sends three things, each with its own consent: a daily update check through a first-party path (no consent), a per-session journey keyed by an id that dies with the process (opt-in, recorded locally by default), and panic reports sent after a dialog that shows the bytes. Nothing identifies a person or a machine; the disclosure page is true of the code and ships inside the app.

**Architecture:** The client half is lib-visits's `visits-app` crate (git tag `v0.3.1`): the report types, the consent `mode()`, the journal with its allowlist, sealing, the queue with its backoff, the daily `Check`, `view`. Arkos adds what the crate leaves to the program, in a new workspace crate `crates/telemetry` (package `telemetry`) shared by the app and the daemon: the allowlist file, the envelope, the config table, the panic hook, the disclosure's field table. The daemon is the only uploader and sealer (it has curl) and routes the first update check of the day first-party. The app records events and focus time and owns every consent surface. Protocol 7 adds a `Session { sid }` request after `Hello` and a `TelemetryStatus` request; no existing message changes shape.

**Tech Stack:** Rust, gpui, `visits-app` (serde, toml 1), the system curl through the daemon's existing `--curl` seam, `backtrace` (unresolved frames only), `sha2`.

**Spec:** `docs/superpowers/specs/2026-10-16-telemetry-design.md` (approved; section 3.2 amended for `log` and Dock launches); the wire format is lib-visits `spec/SPEC.md` section 6; the crate's loop is its `lib.rs` docs.

**Prerequisite (done by the controller before Task 1):** lib-visits v0.3.1 adds `Crash::from_bytes` and `Session::from_bytes` (parse, then validate) so an incident file can be re-queued, and `seal_stale_except(dir, now, older, &[sid], state)` so the daemon can leave the connected windows' journals alone.

## Global Constraints

- Build hygiene (binding on this laptop): every cargo call `nice -n 15 ~/.cargo/bin/cargo ...` with the worktree's `.cargo/config.toml` (jobs 3, shared target dir) copied from the main checkout and never committed; no `--release`; one cargo process at a time (check `pgrep -x cargo`, wait if one runs). Focused tests while iterating; once before committing `nice -n 15 ~/.cargo/bin/cargo test --workspace --no-fail-fast` and `nice -n 15 ~/.cargo/bin/cargo check --workspace --all-targets` (no warning from our crates). Known timing flakes under load, rerun alone and report both results: `concurrent_spawns_give_each_shell_only_its_own_terminal`, `the_grace_counts_from_the_relaunch_so_a_slow_relaunch_keeps_the_old_bundle`, `recovery::tests::a_death_rewrites_the_manifest_without_the_dirty_mark`, `continuous_reader_survives_a_flood`.
- Never launch the Arkos app binary; headless tests only. Nothing in a test touches the network or the developer's real folders: every test uses a temp dir (`ARKOS_APP_SUPPORT` for the app, the daemon's `--telemetry-dir`), and curl is replaced by a fake script through the daemon's `--curl` flag that prints a chosen status or final URL.
- Privacy rules that bind every task: no hostname, username, home directory, path, file name, project name, environment variable, prompt, terminal content, agent transcript, IP or persistent id in any event, report, log line or journal. Every event value is one of the allowlisted names in `telemetry/events.toml`; a value that is not in it is dropped and counted. The daemon is the only process that uploads. A `source` build never uploads and never writes an incident. `DO_NOT_TRACK`, `ARKOS_TELEMETRY=off`, `CI` and a non-interactive session keep everything local. A config error means telemetry `Off` for that run, never the defaults.
- The release workflow, `packaging/test.sh` and the shoot stay green; dev builds and the shoot are `source` and send nothing.
- No em-dashes anywhere. Comments are full sentences in the files' existing voice. `docs/development.md` Status gains the hand checks owed.
- Commit trailers (after a blank line): `Co-Authored-By: Claude <model> <noreply@anthropic.com>` with the implementer's model name, and `Claude-Session: https://claude.ai/code/session_015xrVD54zbvkgksJUndxfpr`.

## Shared definitions

- **Paths.** `crates/telemetry` takes over `config_dir`, `config_file` (with the XDG rule) and `app_support` from the app's `paths.rs`, which re-exports them, so the daemon can read the config table too. The app: `telemetry_dir()` = `app_support().join("telemetry")` (`~/Library/Application Support/arkos/telemetry`, honouring `ARKOS_APP_SUPPORT`). The daemon: always the `--socket` folder's `telemetry/` (as `updates/` already does; it is the same folder in practice), overridable with `--telemetry-dir`. Inside: the crate's `journal/`, `queue/`, `state.toml`, `last-session.json`, plus Arkos's `incidents/` (crash reports awaiting the dialog), `asked` (the invitation was answered), `receipts` (one line per crash report: inc, date, outcome) and `what-arkos-sends.md` (the disclosure written out to be opened).
- **Envelope.** `app = "arkos"`, `ver = env!("CARGO_PKG_VERSION")`, `bin = "app" | "daemon"`, `os = macos`, `osv` = the major of `sw_vers -productVersion` read once (0 if unknown), `arch` from `std::env::consts::ARCH` (`aarch64` is `arm64`), `ch` = `release` when the running bundle has `Contents/Resources/channel` containing `release`, else `source`.
- **Endpoint.** `https://apps.csarko.sh/a` for reports, `https://apps.csarko.sh/a/check` for the check, user agent `arkos/<ver>`; the daemon's test-only flag `--telemetry-url <base>` (like `--release-key`) accepts only `https://apps.csarko.sh` or a `file://` base.
- **Config.** `[telemetry] usage = "off" | "local" | "on"` (default `local`), `crashes = "never" | "ask" | "always"` (default `ask`); `TelemetryConfig` in `crates/telemetry` with `deny_unknown_fields`; the app reads it with the rest of the config, the daemon parses only the `[telemetry]` table of `paths::config_file()` and re-reads it before every drain and check, so `arkos telemetry on` takes effect without a restart. `DEFAULT_CONFIG` gains the two keys commented.
- **Environment.** `ARKOS_TELEMETRY` is the crate's `<APP>_TELEMETRY`; `DO_NOT_TRACK`; `CI`. `interactive` is true for the app and the daemon it starts; false for `arkos` as a CLI under a non-TTY.
- **The allowlist** `telemetry/events.toml` (compiled in): the spec's section 2.2 table verbatim (the `palette` values are `keymap::Command`'s names in snake case; `settings` keys are the config keys with `font.presets.*` collapsed to `font.presets`), plus `[focus] surfaces = ["terminal", "viewer", "browser", "settings"]`.
- **Protocol 7.** `PROTOCOL_VERSION = 7`, `TELEMETRY_FROM = 7`; `Request::Session { sid: String }` sent right after `Hello` by a protocol 7 window; `Request::TelemetryStatus` and `Response::TelemetryStatus { usage, crashes, uploads, reason, queued, journals, last_check_day, sent, rejected }`. `Hello` is unchanged. A window that sent no `Session` gets nothing sealed.
- **Incidents.** `incidents/<inc>.json` is the bytes of `Crash::to_bytes`, written by the panic hook; the dialog claims it with an atomic rename to `incidents/claimed/<inc>.<pid>.json` first (so two restored windows never both show it; a claim whose pid is dead is put back at the next launch), then moves it to the queue through `Crash::from_bytes` and `Queue::enqueue_crash(.., now)`, or deletes it. The app writes the receipt at decision time (`receipts`: inc, date, `queued` or `declined`); the Reports list shows a queued one as `sent` once its file has left the queue. A `source` build's hook writes nothing.
- **Second launch.** A launch on a later UTC day than `paths::installed_file()`'s date (each window is its own process, so a counter cannot say it).
- **Opt-in.** Every path that turns usage `On` (the banner, Settings, `arkos telemetry on`, `settings set` as seen by `config_watch`) calls `Queue::clear_sessions` first, and every window, on seeing the change through `config_watch`, deletes its running journal, opens a new one under a fresh `new_id()` and sends `Session` again (the daemon's record takes the new sid): a clean start, nothing recorded before the consent is sent.

## Review Focus

1. A `source` build (every dev build, the shoot) never reaches the transport, never routes a check first-party, and never writes an incident: `mode()`'s source rule, `Check::next`'s `None`, the hook's early return and the collector's 403 are independent guards. Pinned in Tasks 3, 4 and 5.
2. An event site can never carry a value outside the allowlist: the journal rejects and counts it, and a test enumerates every typed helper's values against `events.toml`. Pinned in Task 6.
3. The first-party check is at most once a day, has its own 5 s timeout, falls back to GitHub on any failure in the same check (a `to_query` error included), and never delays an update. Pinned in Task 3.
4. Nothing uploads under the default config, under `DO_NOT_TRACK`, under `ARKOS_TELEMETRY=off`, under `CI`, from a non-interactive CLI, or on a config error; a crash queued under `always` stops uploading once `crashes` is `never`. Pinned in Task 4.
5. The invitation shows once, on a later day than the install, in a release build, interactive, usage `local`, not yet asked; never on the install day; never again after either button; a second window re-checks `asked` before it renders. Pinned in Task 9.
6. The crash dialog's "Always send" starts unchecked; Send is consent for one report; Don't send deletes; nothing is sent while the dialog is open; one dialog across restored windows. Pinned in Task 10.
7. The disclosure's field table and event list are rendered from the shared crate's tables, which a test proves equal to the keys of real sample reports and to `events.toml`, and the marked region of `docs/telemetry.md` equals the render. Pinned in Task 7.
8. The panic hook allocates only on the panicking thread, takes no lock but the allocator's (`backtrace::trace_unsynchronized`, no symbol resolution, no Recorder, config or gpui state), and strips every path and number from what it hashes. Pinned in Task 5.

---

### Task 1: The crate, the allowlist and the envelope

**Files:**
- Modify: `Cargo.toml` (workspace dependency `visits-app = { git = "https://github.com/csarkosh/lib-visits", tag = "v0.3.1" }`; `backtrace` and `sha2` as workspace dependencies), `crates/arkos/Cargo.toml`, `crates/arkosd/Cargo.toml` (both depend on `telemetry`), `Cargo.lock`
- Create: `telemetry/events.toml`, `crates/telemetry/` (package `telemetry`: `envelope(bin: Bin) -> Envelope` with `osv` and the channel; `channel_of(bundle: &Path) -> Channel` pure; `TelemetryConfig` and `parse_table(config_text) -> Result<TelemetryConfig, Error>` that reads only the `[telemetry]` table; `mode_for(config: Result<&TelemetryConfig, _>, interactive) -> Mode` wrapping `visits_app::mode` with `ARKOS_TELEMETRY`, a config error giving `Off`; `allowlist() -> &'static Allowlist`; `second_launch(installed: &Path, now) -> bool`)
- Modify: `crates/arkos/src/config.rs` (`[telemetry]` through the shared type; `DEFAULT_CONFIG`'s commented keys, which the existing test at line 732 checks)

- [ ] **Step 1: Tests** (RED): `events.toml` parses through `Allowlist::from_toml`, lists every event of the spec's table with every value, and every `palette` value is a `keymap::Command` name; `channel_of` with and without the file; `parse_table` on a full config file, one with no table, one with an unknown key (error), one with a broken other section (the table still parses); `mode_for` with the defaults gives `Local`, `uploads: false`, `invite` true when interactive and release; a config error gives `Off`; `second_launch` on the install day and the next day.
- [ ] **Step 2: Implement.** `osv` is read once with `sw_vers` and cached. **Step 3: Gates; commit** `Telemetry: the shared crate, the event allowlist and the envelope`.

### Task 2: Protocol 7

**Files:**
- Modify: `crates/protocol/src/lib.rs` (`PROTOCOL_VERSION = 7`, `TELEMETRY_FROM`, `Request::Session { sid }` appended, `Request::TelemetryStatus` appended, `Response::TelemetryStatus {..}` appended; `Hello` untouched), `crates/arkosd/src/server.rs` (`Session` stores `rec.sid` on the `ClientRecord`; `TelemetryStatus` answers an empty status for now, filled in Task 4), `crates/arkos/src/client.rs` (sends `Session` right after `Hello` when a `sid` is given), `crates/arkos/src/doctor.rs` (its Hello use unchanged; the test at line 211 still passes)

- [ ] **Step 1: Tests** (RED): the postcard round trips of the three new messages; a protocol 6 `Hello` encoding still decodes; the client sends `Hello` then `Session`; the daemon stores the sid.
- [ ] **Step 2: Implement.** **Step 3: Gates; commit** `Protocol 7: the session id after Hello and a telemetry status`.

### Task 3: The daemon's state and the first-party check

**Files:**
- Create: `crates/arkosd/src/telemetry/mod.rs` (the daemon's telemetry: the dir, the envelope, `State::update` around every crate call, `mode_now()` re-reading the config table), `crates/arkosd/src/telemetry/check.rs` (`route(check: Option<Check>, first_party: Option<&str>, github: &str) -> Vec<Url>` pure: the first-party url first when `Some`, GitHub always last)
- Modify: `crates/arkosd/src/updates/fetch.rs` (`Fetcher` gains `user_agent`; curl gets `--user-agent`; `allowed_final_url` accepts a requested first-party host whose final url is GitHub's; a first-party fetch gets `--max-time 5`), `crates/arkosd/src/updates/poll.rs` (before each check: `Check::next(env, &state, now)` from `State::load`; the urls from `route`; the manifest comes from the first that answers; after a first-party attempt, whatever it answered, `State::update(|s| s.checked(now))`; the `.sig` always from GitHub), `crates/arkosd/src/main.rs` (`--telemetry-dir`, the test-only `--telemetry-url`)

- [ ] **Step 1: Tests** (RED): `route` for `None`, `Some`, and a `to_query` error (GitHub only); with the fake curl script, the first check of a day goes first-party and the second goes to GitHub; a first-party failure falls back in the same check and the state still records the day; a `source` envelope never goes first-party (Review Focus 1 and 3); the query carries `rd` -1 then the day difference; `allowed_final_url` with the first-party start.
- [ ] **Step 2: Implement.** **Step 3: Gates; commit** `The daemon routes the first update check of a day through the first-party path`.

### Task 4: The daemon seals and uploads

**Files:**
- Create: `crates/arkosd/src/telemetry/upload.rs` (the `Transport` through curl: `--proto =https`, `--max-time 10`, `--user-agent`, `--data-binary @-` over stdin, `-o /dev/null -w %{http_code}`, no `--fail`, no `--location`, the host pinned to the configured base; the 15-minute loop: `mode_now()`, `seal_stale_except` with the sids of the connected windows (sleep makes a live journal look stale), `drain`, then `State::update` with the outcome), `crates/arkosd/src/telemetry/seal.rs` (sealing in `server.rs::disconnect` (around line 388) after the state lock is dropped, from the removed `ClientRecord`'s `sid` and `end`: `Quit` when `Goodbye` set it or the record has `relaunch` set, else `Lost`; `Goodbye` only sets `rec.end`)
- Modify: `crates/arkosd/src/server.rs` (the record's `sid`, `end`; `TelemetryStatus` filled from `view`), `crates/arkosd/src/main.rs` (start the loop)

- [ ] **Step 1: Tests** (RED): the transport against the fake curl (204, 429, 400 outcomes through the crate); the loop under the default config calls nothing (Review Focus 4, one test per environment rule and the config-error rule through `mode_for`); a queued crash stops once `crashes` is `never`; sealing on Goodbye gives `quit`, on a relaunch `quit`, on a dropped socket `lost`; `seal_stale_except` leaves a connected window's journal alone.
- [ ] **Step 2: Implement.** **Step 3: Gates; commit** `The daemon seals sessions on disconnect and uploads the queue`.

### Task 5: The panic hook

**Files:**
- Create: `crates/telemetry/src/panic.rs` (`install(env: Envelope, dir: &Path)` (the envelope passed in, so a test can install with `Channel::Release` while `main` passes the real one): computes at install time into a `OnceLock` the envelope, the commit, the incidents dir, and the images list `[name, uuid, size]` for the Arkos binaries only (LC_UUID read through `_dyld_get_image_header` and the load commands), and pre-opens nothing but keeps the dir's fd; the hook: returns at once for a `source` channel; downcasts the payload for a named `Kind` (`pk`), else `other` with `mh` = SHA-256 of the first line after every path-like token and every number are stripped; `file` kept only when it starts with `/src/`, `/cargo/` or `/rustc/`, else its basename; `line`; frames from `backtrace::trace_unsynchronized` unresolved as (image index, offset) against the Arkos images' bases, frames in any other image skipped; the uptime bucket; writes `incidents/<new_id()>.json` with one `write` to a file created with `openat` on the kept dir fd; then the previous hook), `crates/telemetry/src/kinds.rs` (the `Kind` enum of Arkos's named failure points, for `panic_any`)
- Modify: `crates/arkosd/src/main.rs` (install first thing) and `crates/arkos/src/main.rs` (install after the CLI branches `notify`, `hooks`, `guide`, `theme`, `settings`, `config`, `telemetry` and `doctor`, so a crash in a CLI run is never an app incident)

- [ ] **Step 1: Tests** (RED): a subprocess test (a small test binary or `cargo test` re-invoking itself with an env flag, installed with a `release` envelope) that panics with a message containing a fake home path and a number: the incident validates through `Crash::from_bytes`, its `file` is remapped or a basename, its `mh` equals the hash of the stripped line, the images carry 32-hex uuids; a `source` run writes nothing (Review Focus 1 and 8); the stripping function's unit tests.
- [ ] **Step 2: Implement.** **Step 3: Gates; commit** `A panic hook that writes a crash report with nothing of the machine in it`.

### Task 6: The app records

**Files:**
- Create: `crates/arkos/src/telemetry.rs` (the `Recorder`: `Journal` when `mode.usage != Off`, `sid` from `new_id()`, the focus clock, `mark` once a minute from a `background_executor().timer` loop like main.rs around line 4262 and in `flush_for_quit` (main.rs around 3075, every quit path including macOS's terminate), typed helpers per event so a call site cannot spell a value wrong)
- Modify: the event sites, one call each: `main.rs` (launch with `how`, window open; `window_close` and the final mark before `sessions.goodbye()`; help; theme; notices; errors; the `settings` event derived from `config_watch`'s diff, keys collapsed), `pane_view.rs` (pane open and close), `viewer.rs` (view), `md_view.rs` (md features, once per session), `launch.rs` (agent launch), `sessions.rs` (agent waiting, once per terminal), `command_palette.rs` (palette command by `keymap::Command` name), `update.rs` (update steps), `client.rs` (the `Session` request)

- [ ] **Step 1: Tests** (RED): every typed helper's possible values against `events.toml` (Review Focus 2); the focus clock across three focus changes; `md` features once per session; usage `Off` writes no journal; the settings diff to event keys; a usage change to `On` seen through `config_watch` restarts the journal under a fresh sid and resends `Session`.
- [ ] **Step 2: Implement.** **Step 3: Gates; commit** `The app records its session: events and focus time`.

### Task 7: The disclosure and the Help menu

**Files:**
- Create: `crates/telemetry/src/disclosure.rs` (the field table: key, example, reason, per kind, as data; `render_tables() -> String`; `write_page(dir) -> PathBuf` that writes the compiled `docs/telemetry.md` to `telemetry/what-arkos-sends.md`), `docs/telemetry.md` (the nine items from the spec's section 4 as prose, with a marked region holding the rendered tables, the Dock note, the `log` note, the authoritative files including `crates/telemetry/`)
- Modify: `crates/arkos/src/main.rs` (the Help menu item "What Arkos Sends": in a project window, `write_page` then open the file in a viewer tab; from the chooser or Settings, `write_page` then `cx.open_with_system`), `docs/agent-guide.md` (a short section: the agent answers from the page and never changes the setting on its own)

- [ ] **Step 1: Tests** (RED): the field table's keys equal the keys of sample `Session::to_bytes`, `Crash::to_bytes` and `Check::to_query` outputs, and every event and value of `events.toml` appears in the render; the marked region of `docs/telemetry.md` equals `render_tables()` (Review Focus 7); `write_page` writes the page.
- [ ] **Step 2: Implement.** **Step 3: Gates; commit** `The disclosure page: generated tables, shipped in the app, opened from Help`.

### Task 8: The command line, the settings keys and the doctor row

**Files:**
- Create: `crates/arkos/src/telemetry_cli.rs` (`arkos telemetry status|on|off|local|view [--last|<path>]|crashes never|ask|always`; `on` clears queued sessions first; `status` asks the daemon `TelemetryStatus` when it runs and falls back to `view`)
- Modify: `crates/arkos/src/main.rs` (the subcommand beside `theme` and `settings`), `crates/arkos/src/agent_cli.rs` (`settings get|set telemetry.usage|crashes`, with the clear on `on`), `crates/arkos/src/config_watch.rs` (a usage change to `On` seen from the file clears queued sessions too), `crates/arkos/src/doctor.rs` (a row: the mode, its reason, the queue)

- [ ] **Step 1: Tests** (RED): the CLI's parsing and outputs on a temp dir; `on` clears the sessions; the doctor row text; `settings set telemetry.usage on` through the writer.
- [ ] **Step 2: Implement.** **Step 3: Gates; commit** `arkos telemetry, the settings keys and a doctor row`.

### Task 9: The Settings section and the invitation

**Files:**
- Modify: `crates/arkos/src/settings.rs` (a `Privacy` section: Usage reports Off / Local only / On with the mode's reason line; Crash reports Never / Ask each time / Always; What Arkos sends; Last report (opens `last-session.json` in a viewer tab); turning usage On clears the sessions), `crates/arkos/src/pane_view.rs` or `notices.rs` (the invitation strip under the first pane's tab bar: the text, Turn on, Not now, What is sent; shown per Review Focus 5; re-checks `asked` before rendering; writes `asked` on either button)

- [ ] **Step 1: Tests** (RED): the invitation's conditions table, pure (Review Focus 5); the Settings rows' labels and their config edits like the Wide tables row test.
- [ ] **Step 2: Implement.** **Step 3: Gates; commit** `A Privacy section in Settings, and the invitation on the second day`.

### Task 10: The crash dialog and the Reports list

**Files:**
- Create: `crates/arkos/src/crash_dialog.rs` (a window of the About window's family: one sentence with the date and which binary; Show report rendering the exact bytes monospace; Don't send (deletes); Send (claims, `Crash::from_bytes`, `enqueue_crash`); the unchecked Always send box (sets `telemetry.crashes = "always"` through the config writer `settings_cli` uses); several incidents are one dialog with a count and one decision; the pure `decide(crashes, mode, incidents) -> Decision` (`Show(n)`, `QueueAll`, `DeleteAll`, `Wait`))
- Modify: `crates/arkos/src/main.rs` (claim and decide at startup after the window), `crates/arkos/src/settings.rs` (Reports: the `receipts` file by date with inc and outcome, plus incidents still waiting)

- [ ] **Step 1: Tests** (RED): `decide` for every combination (Review Focus 6); the claim rename is exclusive between two processes (a test with two claims on one file); a claim with a dead pid is put back; the moves on a temp dir and the receipt line; the "always" write.
- [ ] **Step 2: Implement.** **Step 3: Gates; commit** `The crash dialog: show the bytes, send or not, Always send unchecked, and the Reports list`.

### Task 11: The release: the channel file, the symbols, the remapped paths

**Files:**
- Modify: `packaging/bundle.sh` (writes `Contents/Resources/channel` with `release` only when `ARKOS_CHANNEL=release` is set, before codesign; `packaging/test.sh` keeps dev bundles as `source`), `.github/workflows/release.yml`, `.github/workflows/release-notarized.yml`, `packaging/release.sh` (all three: `ARKOS_CHANNEL=release`, `RUSTFLAGS="--remap-path-prefix=$PWD=/src --remap-path-prefix=$HOME/.cargo=/cargo"` on the build, and a dSYM zip `Arkos-IDE-<version>-dSYM.zip` from `arkos.dSYM` and `arkosd.dSYM` published with the release), `Cargo.toml` (`split-debuginfo = "packed"` under `[profile.release]`), `docs/development.md` (how to symbolicate: `atos -o <dSYM> -l <base> <offset>` from the images list)

- [ ] **Step 1:** a dry run of `bundle.sh` against a fake `target/release` in a temp dir with and without `ARKOS_CHANNEL` checking the channel file. **Step 2: Implement. Step 3: commit** `Release: the channel file, published dSYMs, and paths remapped out of the binaries`.

### Task 12: Docs and status

**Files:**
- Modify: `README.md` (one paragraph: what is sent, off until turned on, the link to `docs/telemetry.md`), `docs/development.md` Status (the hand checks owed: a real panic in a release build symbolicated against the published dSYM; the first-party check row seen in BigQuery with `rd` right on the second day; the shoot's source build sending nothing; the invitation on the second day; the dialog), the 0.5.0 release note line

- [ ] **Step 1: Write. Step 2: commit** `Docs: telemetry in the README, the status, and the hand checks owed`.
