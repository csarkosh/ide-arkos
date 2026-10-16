# The agent theme interface (milestone 8b)

Status: plan for review. Spec: `docs/superpowers/specs/2026-10-16-agent-theme-interface-design.md`
(approved by Cyrus on 2026-10-16, "approve, defaults accepted": `arkos theme use`, `arkos guide`,
keybindings off the settings allow-list, a two-second wait for window answers, the protocol bump at
0.4.0, the skill at user level). Written against main at 0.3.4 (`f51470a`). Ships as 0.4.0.

Goal: a coding agent can discover, make, check, preview and apply a theme for Arkos from a shell,
with or without a window open, and read or write the settings on a short allow-list; a running
Arkos answers over the daemon socket through four relayed actions and never writes a file on that
channel.

## Global Constraints

- Machine rules: every cargo call is `nice -n 15 ~/.cargo/bin/cargo ...`; the worktree's
  `.cargo/config.toml` stays; no `--release`; never two cargo processes at once; focused filters
  while iterating, the workspace suite once per task; `cargo check --workspace --all-targets`
  prints no warning from our crates, in the test build too. Known timing flakes under load:
  `concurrent_spawns_give_each_shell_only_its_own_terminal`,
  `the_grace_counts_from_the_relaunch_so_a_slow_relaunch_keeps_the_old_bundle`,
  `recovery::tests::a_death_rewrites_the_manifest_without_the_dirty_mark`; rerun alone and report
  both results.
- Dead code between tasks: an item a later task first uses, reached by a test here, gets
  `#[cfg_attr(not(test), allow(dead_code))] // Used from Task N on.`; unreached items get
  `#[allow(dead_code)] // Used from Task N on.`; the task that first uses it removes the attribute;
  after Task 5 `grep -rn "Used from Task" crates/` prints nothing.
- No em-dashes in any file. Comments are full sentences. Headless tests only: no test opens a
  window; socket tests use the daemon test harness in `crates/arkosd/tests/daemon.rs` and the fake
  daemon pattern in `hook_cli.rs`.
- **Protocol** (`crates/protocol/src/lib.rs`): `PROTOCOL_VERSION` 5 becomes 6; `pub const
  CUSTOMIZE_FROM: u32 = 6`. New messages, all `Serialize, Deserialize, Debug, Clone, PartialEq, Eq`:
  `Request::Customize { id: u64, action: CustomizeAction }` (from a tool client),
  `Request::CustomizeDone { id: u64, outcome: CustomizeOutcome }` (from a window, answered `Ok`),
  `Event::Customize { id: u64, action: CustomizeAction }` (to windows), `Response::Customized {
  outcomes: Vec<CustomizeOutcome> }` (to the tool client). `pub enum CustomizeAction {
  PreviewTheme { toml: String }, EndPreview, ReloadConfig, ReportState }`. `pub struct
  CustomizeOutcome { pub pid: u32, pub project: PathBuf, pub result: Result<Report, String> }`.
  `pub struct Report { pub theme: String, pub dark: bool, pub system_is_dark: bool, pub preset:
  String, pub family: String, pub preview: Option<String>, pub config_error: Option<String> }`
  with `Default`. `serde_json` round-trips every new type (the window log and `--json` print
  them). The existing `broadcast` filter (`protocol >= UPDATE_EVENTS_FROM`) stays for the old
  events; `Event::Customize` goes only to clients with `protocol >= CUSTOMIZE_FROM`.
- **The daemon relay** (`crates/arkosd/src/server.rs`): `Request::Customize` from any client
  (a tool client has `protocol` 0 and never receives events) is given a pending record
  `Pending { tx: ClientSink, msg_id: u32, expected: usize, outcomes: Vec<CustomizeOutcome>,
  started: Instant }` in `State.customize: HashMap<u64, Pending>` keyed by the request's `id`;
  `expected` is the number of connected window clients (`protocol >= CUSTOMIZE_FROM`) at that
  moment; the event is broadcast to them; `dispatch` returns `None` (the answer comes later),
  unless `expected` is 0, when it answers `Customized { outcomes: vec![] }` at once. Each
  `CustomizeDone` with a known `id` appends its outcome and is answered `Ok`; when
  `outcomes.len() == expected` the pending record is removed and the tool client receives
  `Customized`. A thread spawned per pending record sleeps `CUSTOMIZE_WAIT` (2 s) and then flushes
  what arrived if the record is still there (the client prints the missing windows itself, by
  comparing against `expected`, which the response carries as `Customized { outcomes, expected:
  usize }`: add that field). A tool client that disconnects before the answer loses it (the
  record is dropped in `disconnect`). The `id` is chosen by the client (unix nanos); a duplicate
  pending `id` is answered `Error("customize id in flight")`.
- **The window** (`crates/arkos/src/main.rs`): `Root::route_events` gains the `Event::Customize`
  arm: `PreviewTheme { toml }` runs `theme::parse`, on `Ok` calls `set_theme` and sets
  `self.preview = Some(name)` (a new `Option<String>` field; the config is untouched), on `Err`
  reports the error; `EndPreview` clears `preview` and re-resolves through `resolve_theme` and
  `switch_theme_to`; `ReloadConfig` runs `reload_config` and reports (the resolved theme, or
  `config_error`); `ReportState` reports. Every arm ends with `self.sessions.client.send(Request::
  CustomizeDone { id, outcome })` through a shared `fn report(&self) -> Report` (`theme:
  self.theme.name`, `dark: !self.theme.is_light()`, `system_is_dark`, `preset: self.text_preset`,
  `family: self.config.font.family`, `preview`, `config_error`). A config reload or a theme file
  change ends a preview (`preview = None`) before applying, as the spec says. The outcome's
  `pid` is `std::process::id()` and `project` is `self.project`.
- **The watcher** (`crates/arkos/src/config_watch.rs`): `start(root, path, cx)` watches a second
  path that it asks the Root for after every poll: `root.read(cx).theme_file()`, which is
  `paths::themes_dir().join(format!("{}.toml", name))` for the theme the window shows (`theme.name`),
  whether or not the file exists (a built-in with no file has fingerprint `None`; the moment a
  file appears it differs). A change of either fingerprint calls `reload_config`, which already
  re-resolves the theme through `switch_theme_to` (`keep_or_switch` keeps the last good theme and
  returns the error); the banner text for a bad theme file is `"theme {name}: {error}"` (set
  where `config_error` is set; cleared by the next good reload). Pure helper `fn changed(last:
  &[Option<(u128, u64)>; 2], now: &[Option<(u128, u64)>; 2]) -> bool` with a test.
- **The tool client** (`crates/arkos/src/agent_cli.rs`, new): `pub fn customize(socket: &Path,
  action: CustomizeAction, wait: Duration) -> Result<Vec<Outcome>, String>` connects as
  `hook_cli::send_one` does (no Hello, a read timeout of `wait + 1 s`), sends `Customize { id:
  nanos, action }`, returns `Customized`'s outcomes plus one `Outcome::NoAnswer` per missing
  window (`expected - outcomes.len()`); `Err` names the socket when the daemon is not running.
  `pub enum Outcome { Window(CustomizeOutcome), NoAnswer }`. Every command below that needs a
  window maps "no daemon" or "no windows" to exit 3 with the line `no window running` (and, for
  `use` and `settings set`, to `saved; no window running` with exit 0).
- **Commands and exit codes**: 0 done; 1 refused or failed (a bad file, an unknown name, a window
  that reported an error, a low contrast pair); 2 usage (the usage text on stderr); 3 a window was
  needed and none is running. `--json` prints one JSON object (or array, for `list`) on stdout and
  nothing else; without it, human lines. The window lines are `window {pid} ({project}): {text}`;
  a missing one is `window ?: no answer`. All output goes through `out` and `err` writers so tests
  capture it; `main()` wires the real streams.
- **`arkos theme`** (`theme.rs` keeps `list` and `validate`; the new ones live in `agent_cli.rs`
  and call into `theme.rs`):
  - `template [<built-in>] [--json]`: the built-in's TOML (default navy) or, with `--json`, the
    schema `{ "tables": [{ "name", "keys": [{ "key", "meaning", "contrast_with"?, "needs"? }] }] }`
    from a new `theme::schema() -> Vec<TableDoc>` const data (the same facts as `docs/themes.md`,
    checked by a test that every key of `Theme` appears once: compare against the keys of the
    parsed navy TOML document).
  - `contrast <file-or-name> [--json]`: `theme::contrast_pairs(&Theme) -> Vec<Pair { what: String,
    fg: Rgb, bg: Rgb, needs: f64 }>` moved out of the test (the test `every_foreground_clears...`
    uses it, so the CLI and the test cannot drift): the general pairs at 4.5, `terminal.foreground
    on background` at 11.0, `chrome.text on window_bg` at 7.0, the ANSI pairs at 4.5 (ansi 0 only on
    a light theme), and the seven surface pairs Task 2 of milestone 8a added at 4.5. Lines
    `{what} {ratio:.1}:1 ok` or `{what} {ratio:.1}:1 LOW (needs {needs})`; exit 1 when any is low.
    JSON `{ "name", "ok", "pairs": [{ "what", "fg", "bg", "ratio", "needs", "ok" }] }`.
  - `add <file> [--replace] [--json]`: `theme::import_file`; "added {name}" or "replaced {name}";
    `Err("exists")` prints `exists: {name}; pass --replace` and exits 1; other errors exit 1.
  - `use <name-or-file> [--dark|--light] [--replace] [--json]`: a path ending in `.toml` is added
    first; a name must be built in or a file (else exit 1 `unknown theme: {name}`, nothing written);
    the key is `dark` or `light` from the flag, else from the file's `[appearance] mode` (`dark` or
    `light`), else (`system`) from the running windows' `system_is_dark` (a `ReportState` first;
    with no window, dark); `config::set_values` writes `["theme", key]`; then with windows the
    `ReloadConfig` action and one line per window `window {pid} ({project}): {theme} applied` (or
    its error, exit 1); with none, `saved; no window running`, exit 0. JSON `{ "name", "key",
    "saved": true, "windows": [{ "pid", "project", "theme"?, "error"? }] }`.
  - `preview <name-or-file> [--json]` and `preview --end`: the TOML (a file's text, or a built-in's,
    or a file in the themes folder) validated by `theme::parse` first (exit 1 on error, nothing
    sent); then `PreviewTheme { toml }` or `EndPreview`; lines `window {pid} ({project}):
    previewing {name}` / `preview ended, {theme}` or the window's error (exit 1); exit 3 with no
    window.
  - `current [--json]`: the file part from `config::load` (`dark`, `light`, `mode`), lines `config:
    dark {d}, light {l}, mode {m}` (or `config: {error}`), then `ReportState` per window: `window
    {pid} ({project}): {theme} ({dark|light}, system is {dark|light}), preset {preset}, family
    {family}` plus `, preview {name}` and `, config error: {text}` when set; exit 0 with no window.
- **`arkos settings`** (`agent_cli.rs`): `get [<key>] [--json]`: with no key, the config file's
  text verbatim, then the per-window report lines as `current` prints them; with a key, the value
  found by walking the parsed `toml::Value` of the file (`toml::from_str::<toml::Value>`),
  printed bare (strings unquoted, numbers and booleans as TOML writes them), exit 1 `no such key`
  when absent. `set <key> <value> [--json]`: `pub const SETTABLE: &[&str] = &["appearance.mode",
  "theme.dark", "theme.light", "font.family", "font.preset", "font.presets.*.size",
  "font.presets.*.line_height", "update.auto", "recovery.start_at_login"]` (a `*` segment matches
  one bare name); another key exits 2 with the list; the value is typed by the key (`size` and
  `line_height` f64, `update.auto` bool, the rest strings) and validated before writing:
  `appearance.mode` in system|dark|light, `recovery.start_at_login` in ask|on|off, a theme name
  built in or a file, `font.preset` a preset that exists in the file, `size` 6 to 72, `line_height`
  1.0 to 3.0 (the Settings window's steppers clamp narrower; the CLI allows what the config allows),
  `font.family` any non-empty string; then `config::set_values` (a preset's `size` or `line_height`
  alone is allowed here because the table exists already, checked first), then the `ReloadConfig`
  action per window (as `use`), or `saved; no window running`. JSON `{ "key", "value", "saved",
  "windows": [...] }`.
- **`arkos config reload [--json]`**: the `ReloadConfig` action alone; one line per window; exit 3
  with none.
- **The guide**: `docs/agent-guide.md` (the content outline is in the spec, section 7; about 150
  lines; every command's exit codes and `--json` shape stated; the worked Timo Purple example;
  the rules) compiled in with `include_str!("../../docs/agent-guide.md")` as
  `guide::TEXT`; `arkos guide` prints it (exit 0); `arkos guide --path` writes it to
  `<app support>/agent-guide.md` when the file is missing or differs and prints that path (a
  deviation from the spec's "bundle Resources": the binary carries it, so the path is always
  current and no packaging step is needed); `arkos guide --install` writes
  `~/.claude/skills/arkos-ide/SKILL.md` through `guide::install_skill(dir: &Path) -> Result<Installed
  { path, replaced: bool }, String>`: the front matter `---\nname: arkos-ide\ndescription:
  Customize Arkos IDE: themes, fonts and settings. Use when the user asks to change how Arkos
  looks or behaves.\narkos-guide-version: {version}\n---\n` then `TEXT`; refuses (exit 1, the
  path) a file there whose first lines are not that front matter shape with `name: arkos-ide`;
  then prints the paragraph for `AGENTS.md` or `CLAUDE.md`: `Arkos IDE: run \`arkos guide\`
  before changing its theme or settings.` `guide::skill_state(dir) -> SkillState::{Missing,
  Current, Outdated(String), Foreign}` compares the stamped version with `version_string()`.
  `arkos --help` (new; today only `--version` exists) prints the subcommands with `guide` first.
- **The hook** (`hook_cli.rs`): `run_with` for `--event session-start` prints to stdout, before
  anything else, when `session_env` is `Some`: `This session runs inside Arkos IDE.\nTo change its
  theme, fonts or settings, run \`arkos guide\` first and follow it.\n` (`pub const CONTEXT_LINES`),
  nothing on stdout otherwise; the daemon request behaviour is unchanged; `arkos notify` still
  exits 0 always.
- **The doctor**: `doctor_panel::probe_all` gains `CheckId::AgentGuide` ("Agent guide for Claude
  Code"): `State::Fix("not installed: agents cannot find the toolkit")` with button "Install",
  `State::Good("installed")`, `State::Fix("older than this build")` with "Update", `State::Broken
  ("a different skill sits at {path}")`, from `Probe.skill: SkillState`; the Root's Apply for it
  calls `guide::install_skill` and shows a notice. The read-only `doctor::report` gains a `themes`
  section: `{n} themes, all valid` or one line per invalid file `{path}: {error}` plus `theme.dark
  names no theme: {name}` / `theme.light ...` when the config's names resolve to nothing, and the
  skill's state line.
- **The palette** gains "Install agent guide" (`Command::InstallAgentGuide`, row `"install agent
  guide"` unbound, Section Maintenance, SPEC_ROWS and the count assertions adjusted) running the
  same install with a notice.
- **README**: a "Themes for agents" subsection under Config with the command table (command,
  what it does, exit codes), the live reload of the active theme file, `arkos settings`,
  `arkos guide` and the three discovery layers; Status rows from spec sections 9 and 11; the
  Keyboard table unchanged (nothing bound).

## Review Focus

1. A `Customize` request while no window is connected: the client gets `Customized { outcomes:
   [], expected: 0 }` at once and the command exits 3 (`use` and `settings set` exit 0 with
   `saved; no window running`) (Tasks 1 and 3).
2. A window that never answers: the tool client gets the others' outcomes after two seconds and
   prints `window ?: no answer` (Task 1's daemon test, Task 3's client test).
3. `arkos theme use ../x.toml`: the import refuses a name that is not a bare stem before anything
   is written (Task 3, through `import_file`).
4. A preview is active and the user saves `config.toml`: the preview ends and the config's theme
   shows (Task 2).
5. The active theme's file is edited to garbage: the window keeps the last good theme and the
   banner names the file; the next good save clears it (Task 2).

## File structure

- Create: `crates/arkos/src/agent_cli.rs` (the tool client and the `theme`, `settings`, `config`
  subcommands), `crates/arkos/src/guide.rs` (the guide text, `--path`, the skill), `docs/agent-guide.md`.
- Modify: `crates/protocol/src/lib.rs`, `crates/arkosd/src/server.rs` (+ `tests/daemon.rs`),
  `crates/arkos/src/main.rs` (the `Customize` arm, `preview`, `report`, `theme_file`, the dispatch
  of the new subcommands, `--help`, the doctor Apply), `config_watch.rs`, `theme.rs`
  (`contrast_pairs`, `schema`, `theme_cli_to` delegating the new subcommands), `config.rs`
  (nothing new expected; `set_values` is reused), `hook_cli.rs`, `doctor.rs`, `doctor_panel.rs`,
  `keymap.rs`, `command_palette.rs`, `README.md`, `docs/themes.md` (a pointer to the toolkit).

---

### Task 1: The protocol and the daemon relay

**Files:**
- Modify: `crates/protocol/src/lib.rs` (the constants and the four messages with their types; the
  round-trip tests), `crates/arkosd/src/server.rs` (`State.customize`, the `Customize` and
  `CustomizeDone` arms in `dispatch` ~419, the deadline thread, `disconnect` ~331 drops a tool
  client's pending record), `crates/arkosd/tests/daemon.rs` (two tests)

**Interfaces:** as the Protocol and Daemon relay constraints state. Nothing in `crates/arkos` yet
(the window ignores unknown events? No: the window decodes `Event` by type, so the new variant
must exist before any window speaks protocol 6; Task 2 adds the handling, and until then the
client's `Hello` already carries `PROTOCOL_VERSION`, so a Task 1 build of the window connects and
simply ignores `Event::Customize` in `route_events` (add the empty arm here, with the Task 2
attribute comment) and `sessions.rs`'s event grouping treats it like `UpdateStaged` (not a session
event: `group_by_session` 161 and `stash_if_restoring` 538 get the arm).

- [ ] **Step 1: Failing tests**

```rust
// protocol
#[test]
fn customize_messages_round_trip_through_json_and_the_codec() {
    let a = CustomizeAction::PreviewTheme { toml: "name = \"x\"".into() };
    let r = Report { theme: "navy".into(), dark: true, system_is_dark: true, preset: "comfortable".into(), family: "Menlo".into(), preview: None, config_error: None };
    let o = CustomizeOutcome { pid: 7, project: "/p".into(), result: Ok(r.clone()) };
    for req in [Request::Customize { id: 9, action: a.clone() }, Request::CustomizeDone { id: 9, outcome: o.clone() }] {
        let text = serde_json::to_string(&req).unwrap();
        assert_eq!(serde_json::from_str::<Request>(&text).unwrap(), req);
    }
    let ev = Event::Customize { id: 9, action: CustomizeAction::ReportState };
    assert_eq!(serde_json::from_str::<Event>(&serde_json::to_string(&ev).unwrap()).unwrap(), ev);
    let resp = Response::Customized { outcomes: vec![o], expected: 2 };
    assert_eq!(serde_json::from_str::<Response>(&serde_json::to_string(&resp).unwrap()).unwrap(), resp);
    assert_eq!(PROTOCOL_VERSION, 6);
    assert_eq!(CUSTOMIZE_FROM, 6);
}

// arkosd tests/daemon.rs (the harness: `connect_as(pid)` sends Hello; `connect_raw` does not)
#[test]
fn a_customize_request_is_relayed_to_every_window_and_answered_when_all_report() {
    let d = Daemon::start();
    let mut w1 = d.connect_as(11);
    let mut w2 = d.connect_as(12);
    let mut tool = d.connect_raw();
    tool.send(Request::Customize { id: 1, action: CustomizeAction::ReportState });
    for (w, pid) in [(&mut w1, 11), (&mut w2, 12)] {
        let ev = w.next_event();
        assert_eq!(ev, Event::Customize { id: 1, action: CustomizeAction::ReportState });
        let r = w.request(Request::CustomizeDone { id: 1, outcome: CustomizeOutcome { pid, project: "/p".into(), result: Ok(Report::default()) } });
        assert_eq!(r, Response::Ok);
    }
    match tool.response() {
        Response::Customized { outcomes, expected } => {
            assert_eq!(expected, 2);
            assert_eq!(outcomes.iter().map(|o| o.pid).collect::<Vec<_>>(), vec![11, 12]);
        }
        other => panic!("{other:?}"),
    }
    let mut lonely = d.connect_raw();
    lonely.send(Request::Customize { id: 1, action: CustomizeAction::ReportState });
    // The same id again while the first is done is fine; an id in flight is refused.
    assert!(matches!(lonely.response(), Response::Customized { expected: 2, .. }));
}

#[test]
fn a_silent_window_is_cut_after_the_wait_and_no_window_answers_at_once() {
    let d = Daemon::start();
    let _w1 = d.connect_as(11);
    let mut w2 = d.connect_as(12);
    let mut tool = d.connect_raw();
    let t0 = Instant::now();
    tool.send(Request::Customize { id: 5, action: CustomizeAction::EndPreview });
    let _ = w2.next_event();
    w2.request(Request::CustomizeDone { id: 5, outcome: CustomizeOutcome { pid: 12, project: "/p".into(), result: Err("no preview".into()) } });
    let r = tool.response();
    assert!(t0.elapsed() >= Duration::from_millis(1900) && t0.elapsed() < Duration::from_secs(5), "{:?}", t0.elapsed());
    assert!(matches!(&r, Response::Customized { outcomes, expected: 2 } if outcomes.len() == 1 && outcomes[0].pid == 12), "{r:?}");
    let d2 = Daemon::start();
    let mut tool = d2.connect_raw();
    tool.send(Request::Customize { id: 1, action: CustomizeAction::ReportState });
    assert_eq!(tool.response(), Response::Customized { outcomes: vec![], expected: 0 });
}
```

The harness may need `next_event()` (reads frames until an `Event`) and `send`/`response` split
from `request`; add them to `Conn` in the test file.

- [ ] **Step 2: Run to see them fail**: `nice -n 15 ~/.cargo/bin/cargo test -p protocol` and
  `nice -n 15 ~/.cargo/bin/cargo test -p arkosd --test daemon -- customize silent`.
- [ ] **Step 3: Implement** per the constraints. The deadline thread: `std::thread::spawn` with the
  shared state, sleeping `CUSTOMIZE_WAIT`, then `flush_customize(&state, id)` which removes the
  record if present and sends `Customized`. `disconnect` removes records whose `tx` is the
  leaving client's. Keep `dispatch`'s shape (an `Option<Response>`).
- [ ] **Step 4: Run, check, commit**: the two crates' tests, the arkos suite (the empty arms),
  the warning check. `git commit -m "Protocol 6: a Customize request the daemon relays to every window and answers in two seconds"`.

---

### Task 2: The window answers, previews, and watches its theme file

**Files:**
- Modify: `crates/arkos/src/main.rs` (`route_events` ~2917: the `Customize` arm; `preview:
  Option<String>` on Root; `fn report(&self) -> Report`; `fn theme_file(&self) -> PathBuf`;
  `reload_config` ~1079 and `switch_theme_to` ~1065 end a preview; the bad-theme banner text),
  `crates/arkos/src/config_watch.rs` (the second path; `changed`), `crates/arkos/src/theme.rs`
  (`pub fn preview_from(toml: &str) -> Result<Theme, String>` is just `parse`; no change unless a
  name check is wanted: a preview's `name` may be anything bare), `crates/arkos/src/sessions.rs`
  (nothing beyond Task 1's arms)

**Interfaces:**
```rust
impl Root {
    pub fn report(&self) -> protocol::Report;
    pub fn theme_file(&self) -> PathBuf; // themes_dir/<theme.name>.toml
    fn customize(&mut self, id: u64, action: CustomizeAction, cx: &mut Context<Self>); // applies and sends CustomizeDone
}
pub fn customize_outcome(action: &CustomizeAction, parsed: Result<Theme, String>, after: Report) -> Result<Report, String>; // pure: the Err text for a bad preview, the report otherwise
pub fn changed(last: &[Option<(u128, u64)>; 2], now: &[Option<(u128, u64)>; 2]) -> bool;
```

- [ ] **Step 1: Failing tests**

```rust
// main.rs tests
#[test]
fn a_bad_preview_reports_its_error_and_a_good_one_names_the_theme() {
    let after = Report { theme: "x".into(), preview: Some("x".into()), ..Report::default() };
    let bad = customize_outcome(&CustomizeAction::PreviewTheme { toml: "nope".into() }, Err("expected an equals".into()), after.clone());
    assert_eq!(bad, Err("expected an equals".into()));
    let good = customize_outcome(&CustomizeAction::PreviewTheme { toml: "...".into() }, Ok(theme::builtin("navy").unwrap()), after.clone());
    assert_eq!(good.unwrap().preview.as_deref(), Some("x"));
    assert!(customize_outcome(&CustomizeAction::ReportState, Err("unused".into()), after).is_ok(), "only a preview can fail");
}

// config_watch.rs
#[test]
fn either_path_changing_counts() {
    let a = Some((1, 1)); let b = Some((2, 2));
    assert!(!changed(&[a, None], &[a, None]));
    assert!(changed(&[a, None], &[b, None]));
    assert!(changed(&[a, None], &[a, b]), "a theme file appearing");
    assert!(changed(&[a, b], &[a, None]), "a theme file deleted");
}
```

- [ ] **Step 2: Run to see them fail**: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- tests::a_bad_preview config_watch::`.
- [ ] **Step 3: Implement** per the Window and Watcher constraints. The `Customize` arm runs on the
  UI thread inside `route_events`; `set_theme` is the same call the config path uses, so About,
  Settings and Keybindings follow a preview too (`push_theme`). `reload_config` sets `preview =
  None` first. The watcher's loop keeps `last: [Option<..>; 2]`, polls both paths on the
  background executor, and asks the Root for `theme_file()` after each reload (the name may have
  changed). The bad-theme banner: in `switch_theme_to`, when `wanted` is `Err(e)` and the config
  itself parsed, the banner reads `theme {name}: {e}` (today it shows the error alone; check what
  `keep_or_switch` returns and format at the one place the banner is set, ~1081).
- [ ] **Step 4: Run, check, commit**: `git commit -m "A window answers Customize: preview, end, reload, report; the active theme file is watched"`.

---

### Task 3: The tool client and the `theme`, `settings` and `config` commands

**Files:**
- Create: `crates/arkos/src/agent_cli.rs`
- Modify: `crates/arkos/src/theme.rs` (`contrast_pairs` out of the test, `schema`, `Pair`; the
  general contrast test iterates `contrast_pairs`), `crates/arkos/src/main.rs` (`main()` ~3604
  dispatches `theme` to `agent_cli::theme_cli(&args[2..])` which handles the new subcommands and
  delegates `list`/`validate` to `theme::theme_cli_to`; `settings` and `config` likewise;
  `--help`), `crates/arkos/src/hook_cli.rs` (`send_one` gains a timeout parameter or a sibling
  `send_one_with(socket, request, timeout)`; the old signature stays for `notify`)

**Interfaces:**
```rust
// theme.rs
#[derive(Debug, Clone, PartialEq)] pub struct Pair { pub what: String, pub fg: Rgb, pub bg: Rgb, pub needs: f64 }
pub fn contrast_pairs(t: &Theme) -> Vec<Pair>;
pub fn contrast_report(t: &Theme) -> (bool, Vec<(Pair, f64)>); // ok when every ratio >= needs
pub struct KeyDoc { pub key: &'static str, pub meaning: &'static str, pub contrast_with: Option<&'static str>, pub needs: Option<f64> }
pub struct TableDoc { pub name: &'static str, pub keys: &'static [KeyDoc] }
pub fn schema() -> &'static [TableDoc];
// agent_cli.rs
pub enum Outcome { Window(CustomizeOutcome), NoAnswer }
pub fn customize(socket: &Path, action: CustomizeAction, wait: Duration) -> Result<Vec<Outcome>, String>;
pub struct Env<'a> { pub socket: &'a Path, pub config: &'a Path, pub themes: &'a Path, pub wait: Duration, pub system_is_dark_fallback: bool }
pub fn theme_cli_to(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32;
pub fn settings_cli_to(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32;
pub fn config_cli_to(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32;
pub fn theme_key_for_use(flag: Option<&str>, mode: AppearanceMode, windows_dark: Option<bool>) -> &'static str;
pub fn settable(key: &str) -> bool; // the allow-list with the * segment
pub fn typed_value(key: &str, raw: &str) -> Result<toml_edit::Value, String>; // typing and range checks
pub const SETTABLE: &[&str];
pub const USAGE: &str; // arkos --help text
```

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn the_contrast_report_flags_a_low_pair_and_passes_every_builtin() {
    for (name, _) in theme::BUILTIN {
        let (ok, rows) = theme::contrast_report(&theme::builtin(name).unwrap());
        assert!(ok, "{name}: {:?}", rows.iter().filter(|(p, r)| r < &p.needs).map(|(p, _)| &p.what).collect::<Vec<_>>());
    }
    let text = theme::BUILTIN[0].1.replace("ignored = \"", "ignored = \"#3a3a5a\" # was \"");
    let t = theme::parse(&text).unwrap();
    let (ok, rows) = theme::contrast_report(&t);
    assert!(!ok);
    let low = rows.iter().find(|(p, r)| r < &p.needs).unwrap();
    assert_eq!(low.0.what, "tree.ignored on window_bg");
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("low.toml");
    std::fs::write(&f, &text).unwrap();
    let env = Env { socket: &dir.path().join("none.sock"), config: &dir.path().join("config.toml"), themes: dir.path(), wait: Duration::from_millis(10), system_is_dark_fallback: true };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(theme_cli_to(&["contrast".into(), f.display().to_string()], &env, &mut out, &mut err), 1);
    let s = String::from_utf8(out).unwrap();
    assert!(s.lines().any(|l| l.starts_with("tree.ignored on window_bg ") && l.ends_with("LOW (needs 4.5)")), "{s}");
    assert!(s.lines().any(|l| l == format!("chrome.text on window_bg {:.1}:1 ok", palette::contrast(t.chrome.text, t.chrome.window_bg))));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(theme_cli_to(&["contrast".into(), "navy".into(), "--json".into()], &env, &mut out, &mut err), 0);
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["name"], "navy");
    assert_eq!(v["ok"], true);
    assert!(v["pairs"].as_array().unwrap().len() > 20);
}

#[test]
fn use_writes_the_right_key_refuses_an_unknown_name_and_says_so_without_a_window() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("config.toml");
    std::fs::write(&cfg, "# mine\n[appearance]\nmode = \"light\"\n[theme]\ndark = \"navy\" # d\nlight = \"navy-light\"\n").unwrap();
    let themes = dir.path().join("themes");
    let env = Env { socket: &dir.path().join("none.sock"), config: &cfg, themes: &themes, wait: Duration::from_millis(10), system_is_dark_fallback: true };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(theme_cli_to(&["use".into(), "timo-purple".into()], &env, &mut out, &mut err), 0);
    assert_eq!(String::from_utf8(out).unwrap(), "saved; no window running\n");
    let text = std::fs::read_to_string(&cfg).unwrap();
    assert!(text.contains("light = \"timo-purple\""), "mode light picks the light key: {text}");
    assert!(text.contains("# mine") && text.contains("dark = \"navy\" # d"), "comments kept: {text}");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(theme_cli_to(&["use".into(), "nope".into(), "--dark".into()], &env, &mut out, &mut err), 1);
    assert_eq!(String::from_utf8(err).unwrap(), "unknown theme: nope\n");
    assert!(std::fs::read_to_string(&cfg).unwrap().contains("dark = \"navy\""), "nothing written");
    let src = dir.path().join("ocean.toml");
    std::fs::write(&src, theme::BUILTIN[0].1.replace("name = \"navy\"", "name = \"ocean\"")).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(theme_cli_to(&["use".into(), src.display().to_string(), "--dark".into(), "--json".into()], &env, &mut out, &mut err), 0);
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!((v["name"].as_str(), v["key"].as_str(), v["saved"].as_bool()), (Some("ocean"), Some("dark"), Some(true)));
    assert!(themes.join("ocean.toml").is_file());
    assert!(std::fs::read_to_string(&cfg).unwrap().contains("dark = \"ocean\" # d"));
    assert_eq!(theme_key_for_use(None, AppearanceMode::System, Some(false)), "light");
    assert_eq!(theme_key_for_use(None, AppearanceMode::System, None), "dark");
    assert_eq!(theme_key_for_use(Some("--light"), AppearanceMode::Dark, Some(true)), "light");
}

#[test]
fn settings_set_keeps_to_the_allow_list_types_the_value_and_refuses_bad_ones_before_writing() {
    assert!(settable("theme.dark") && settable("font.presets.big.size") && settable("update.auto"));
    assert!(!settable("keybindings.find") && !settable("font.presets.big.other") && !settable("font.presets.a.b.size"));
    assert_eq!(typed_value("update.auto", "true").unwrap(), toml_edit::Value::from(true));
    assert_eq!(typed_value("font.presets.big.size", "18").unwrap(), toml_edit::Value::from(18.0));
    assert!(typed_value("font.presets.big.size", "99").is_err());
    assert!(typed_value("appearance.mode", "blue").is_err());
    assert!(typed_value("recovery.start_at_login", "on").is_ok());
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("config.toml");
    std::fs::write(&cfg, config::DEFAULT_CONFIG).unwrap();
    let env = Env { socket: &dir.path().join("none.sock"), config: &cfg, themes: &dir.path().join("themes"), wait: Duration::from_millis(10), system_is_dark_fallback: true };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(settings_cli_to(&["set".into(), "keybindings.find".into(), "cmd-g".into()], &env, &mut out, &mut err), 2);
    assert!(String::from_utf8(err).unwrap().contains("theme.dark"), "the list is printed");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(settings_cli_to(&["set".into(), "font.preset".into(), "huge".into()], &env, &mut out, &mut err), 1, "no such preset");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(settings_cli_to(&["set".into(), "font.preset".into(), "compact".into()], &env, &mut out, &mut err), 0);
    assert_eq!(String::from_utf8(out).unwrap(), "saved; no window running\n");
    let text = std::fs::read_to_string(&cfg).unwrap();
    assert!(text.contains("preset = \"compact\""));
    assert_eq!(text.matches('#').count(), config::DEFAULT_CONFIG.matches('#').count(), "comments kept");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(settings_cli_to(&["get".into(), "font.preset".into()], &env, &mut out, &mut err), 0);
    assert_eq!(String::from_utf8(out).unwrap(), "compact\n");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(settings_cli_to(&["get".into(), "nope.key".into()], &env, &mut out, &mut err), 1);
}

#[test]
fn the_client_reports_every_window_and_the_silent_ones() {
    // A fake daemon on a temp socket, as hook_cli's tests do: it reads Customize and answers
    // Customized { outcomes: [one], expected: 2 }.
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("d.sock");
    let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
    let server = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut r = std::io::BufReader::new(s.try_clone().unwrap());
        let mut w = std::io::BufWriter::new(s);
        let msg: ClientMessage = protocol::read_frame(&mut r).unwrap().unwrap();
        let Request::Customize { action: CustomizeAction::ReportState, .. } = msg.request else { panic!() };
        let one = CustomizeOutcome { pid: 3, project: "/p".into(), result: Ok(Report { theme: "navy".into(), ..Report::default() }) };
        protocol::write_frame(&mut w, &ServerMessage::Response { id: msg.id, response: Response::Customized { outcomes: vec![one], expected: 2 } }).unwrap();
        w.flush().unwrap();
    });
    let got = customize(&sock, CustomizeAction::ReportState, Duration::from_secs(1)).unwrap();
    assert_eq!(got.len(), 2);
    assert!(matches!(&got[0], Outcome::Window(o) if o.pid == 3));
    assert!(matches!(&got[1], Outcome::NoAnswer));
    server.join().unwrap();
    assert!(customize(&dir.path().join("absent.sock"), CustomizeAction::ReportState, Duration::from_millis(10)).is_err());
    let env = Env { socket: &dir.path().join("absent.sock"), config: &dir.path().join("c.toml"), themes: dir.path(), wait: Duration::from_millis(10), system_is_dark_fallback: true };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(theme_cli_to(&["preview".into(), "navy".into()], &env, &mut out, &mut err), 3);
    assert_eq!(String::from_utf8(err).unwrap(), "no window running\n");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(config_cli_to(&["reload".into()], &env, &mut out, &mut err), 3);
}

#[test]
fn the_schema_names_every_key_of_a_theme_once_and_the_template_prints_a_builtin() {
    let doc: toml::Value = toml::from_str(theme::BUILTIN[0].1).unwrap();
    let mut from_toml: Vec<String> = Vec::new();
    for (table, v) in doc.as_table().unwrap() {
        if let Some(t) = v.as_table() { for k in t.keys() { from_toml.push(format!("{table}.{k}")); } }
    }
    let mut from_schema: Vec<String> = theme::schema().iter().flat_map(|t| t.keys.iter().map(move |k| format!("{}.{}", t.name, k.key))).collect();
    from_toml.sort(); from_schema.sort();
    assert_eq!(from_schema, from_toml);
    let dir = tempfile::tempdir().unwrap();
    let env = Env { socket: &dir.path().join("none.sock"), config: &dir.path().join("c.toml"), themes: dir.path(), wait: Duration::from_millis(10), system_is_dark_fallback: true };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(theme_cli_to(&["template".into()], &env, &mut out, &mut err), 0);
    assert_eq!(String::from_utf8(out).unwrap(), theme::BUILTIN[0].1);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(theme_cli_to(&["template".into(), "nope".into()], &env, &mut out, &mut err), 1);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(theme_cli_to(&["frob".into()], &env, &mut out, &mut err), 2);
}
```

- [ ] **Step 2: Run to see them fail**: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- agent_cli:: theme::`.
- [ ] **Step 3: Implement** per the Commands constraints. `current` and `use` with windows are
  covered by the fake-daemon pattern (one more test over `current` with a window outcome is
  welcome); `preview` sends the TOML text of a built-in, a themes-folder file or a path. `main()`:
  `theme` → `agent_cli::theme_cli` (real env: `paths::socket_path()`, `paths::config_file()`,
  `paths::themes_dir()`, `CUSTOMIZE_WAIT`), `settings` → `settings_cli`, `config` → `config_cli`,
  `--help`/`-h`/`help` → `USAGE` on stdout, exit 0; the `theme` subcommand's `list` and `validate`
  still reach `theme::theme_cli_to`. `--json` for `list` and `validate` is added here (the
  `Source` shape in the constraint).
- [ ] **Step 4: Run, check, commit**: `git commit -m "arkos theme template, contrast, add, use, preview and current; arkos settings and config reload"`.

---

### Task 4: The guide, the skill, the hook line, the doctor and the palette

**Files:**
- Create: `docs/agent-guide.md`, `crates/arkos/src/guide.rs`
- Modify: `crates/arkos/src/main.rs` (`guide` dispatch; the doctor Apply for `AgentGuide`; the
  palette command), `crates/arkos/src/hook_cli.rs` (`CONTEXT_LINES` on session-start),
  `crates/arkos/src/doctor.rs` (the `themes` section), `crates/arkos/src/doctor_panel.rs`
  (`CheckId::AgentGuide`, `Probe.skill`), `crates/arkos/src/keymap.rs` and `command_palette.rs`
  ("install agent guide"), `crates/arkos/src/config.rs` (DEFAULT_CONFIG keybinding comment
  `# "install agent guide" = ""`), `docs/themes.md` (a pointer to the toolkit)

**Interfaces:**
```rust
// guide.rs
pub const TEXT: &str = include_str!("../../docs/agent-guide.md");
pub const SKILL_NAME: &str = "arkos-ide";
pub const SKILL_DESCRIPTION: &str = "Customize Arkos IDE: themes, fonts and settings. Use when the user asks to change how Arkos looks or behaves.";
pub const AGENTS_MD_LINE: &str = "Arkos IDE: run `arkos guide` before changing its theme or settings.";
pub fn skill_text(version: &str) -> String;
#[derive(Debug, Clone, PartialEq, Eq)] pub enum SkillState { Missing, Current, Outdated(String), Foreign }
pub fn skill_state(dir: &Path, version: &str) -> SkillState; // dir is ~/.claude/skills
pub struct Installed { pub path: PathBuf, pub replaced: bool }
pub fn install_skill(dir: &Path, version: &str) -> Result<Installed, String>;
pub fn write_copy(app_support: &Path) -> Result<PathBuf, String>; // --path
pub fn guide_cli_to(args: &[String], skills_dir: &Path, app_support: &Path, version: &str, out: &mut dyn Write, err: &mut dyn Write) -> i32;
pub fn skills_dir() -> PathBuf; // $HOME/.claude/skills
// hook_cli.rs
pub const CONTEXT_LINES: &str = "This session runs inside Arkos IDE.\nTo change its theme, fonts or settings, run `arkos guide` first and follow it.\n";
// doctor.rs
pub fn themes_section(themes: &Path, config: &Path, skill: &SkillState) -> Vec<String>;
```

- [ ] **Step 1: Failing tests**

```rust
// guide.rs
#[test]
fn the_skill_is_installed_once_refreshed_when_older_and_never_over_a_stranger() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(skill_state(dir.path(), "0.4.0"), SkillState::Missing);
    let i = install_skill(dir.path(), "0.4.0").unwrap();
    assert_eq!(i.path, dir.path().join("arkos-ide").join("SKILL.md"));
    assert!(!i.replaced);
    let text = std::fs::read_to_string(&i.path).unwrap();
    assert!(text.starts_with("---\nname: arkos-ide\ndescription: Customize Arkos IDE"), "{text}");
    assert!(text.contains("arkos-guide-version: 0.4.0\n---\n"));
    assert!(text.ends_with(TEXT));
    assert_eq!(skill_state(dir.path(), "0.4.0"), SkillState::Current);
    assert_eq!(skill_state(dir.path(), "0.4.1"), SkillState::Outdated("0.4.0".into()));
    assert!(install_skill(dir.path(), "0.4.1").unwrap().replaced);
    std::fs::write(&i.path, "---\nname: other\n---\nhi").unwrap();
    assert_eq!(skill_state(dir.path(), "0.4.1"), SkillState::Foreign);
    let e = install_skill(dir.path(), "0.4.1").unwrap_err();
    assert!(e.contains("SKILL.md"), "{e}");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(guide_cli_to(&[], dir.path(), dir.path(), "0.4.1", &mut out, &mut err), 0);
    assert_eq!(String::from_utf8(out).unwrap(), TEXT);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(guide_cli_to(&["--path".into()], dir.path(), dir.path(), "0.4.1", &mut out, &mut err), 0);
    let p = PathBuf::from(String::from_utf8(out).unwrap().trim());
    assert_eq!(std::fs::read_to_string(&p).unwrap(), TEXT);
    assert_eq!(guide_cli_to(&["--install".into()], dir.path(), dir.path(), "0.4.1", &mut Vec::new(), &mut Vec::new()), 1, "the stranger stays");
    assert!(!TEXT.contains('\u{2014}'));
    assert!(TEXT.lines().count() > 100);
    for cmd in ["arkos theme template", "arkos theme contrast", "arkos theme preview", "arkos theme use", "arkos theme current", "arkos settings set", "arkos guide"] {
        assert!(TEXT.contains(cmd), "the guide names {cmd}");
    }
}

// hook_cli.rs: extend the session-start test to assert stdout equals CONTEXT_LINES when the
// session env is set and is empty when it is not (run_with gains an `out: &mut dyn Write`).

// doctor.rs
#[test]
fn the_themes_section_lists_invalid_files_and_missing_names() {
    let dir = tempfile::tempdir().unwrap();
    let themes = dir.path().join("themes");
    std::fs::create_dir_all(&themes).unwrap();
    std::fs::write(themes.join("broken.toml"), "name = \"broken\"\n").unwrap();
    let cfg = dir.path().join("config.toml");
    std::fs::write(&cfg, "[theme]\ndark = \"gone\"\nlight = \"navy-light\"\n").unwrap();
    let lines = themes_section(&themes, &cfg, &SkillState::Missing);
    assert!(lines.iter().any(|l| l.starts_with(&format!("{}: ", themes.join("broken.toml").display()))), "{lines:?}");
    assert!(lines.iter().any(|l| l == "theme.dark names no theme: gone"), "{lines:?}");
    assert!(lines.iter().any(|l| l.contains("agent guide skill: not installed")), "{lines:?}");
    std::fs::remove_file(themes.join("broken.toml")).unwrap();
    std::fs::write(&cfg, "[theme]\ndark = \"navy\"\n").unwrap();
    let lines = themes_section(&themes, &cfg, &SkillState::Current);
    assert_eq!(lines[0], format!("{} themes, all valid", theme::BUILTIN.len()));
}

// doctor_panel.rs: probe_all with Probe.skill = Missing gives the AgentGuide check in Fix with
// "Install"; Current gives Good with no button; Outdated gives Fix with "Update"; Foreign Broken.
```

- [ ] **Step 2: Run to see them fail**: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- guide:: hook_cli:: doctor:: doctor_panel:: keymap:: command_palette::`.
- [ ] **Step 3: Write the guide and implement** per the Guide, Hook, Doctor and Palette
  constraints. The guide follows spec section 7's outline with the exact command lines from Task
  3's usage; the worked example is Timo Purple (accent `#00ffff`, background `#330076`) with the
  contrast reasoning from `crates/arkos/themes/timo-purple.toml`'s values. The Root's doctor Apply
  for `AgentGuide` calls `guide::install_skill(&guide::skills_dir(), &version_string())` on the
  background executor and shows a notice with the path or the error; the palette command does the
  same. `doctor::report` calls `themes_section` with the real paths and `skill_state`.
- [ ] **Step 4: Run, check, commit**: `git commit -m "The agent guide: arkos guide, a Claude Code skill, the hook's pointer, a doctor check"`.

---

### Task 5: README, docs, and the gates

**Files:**
- Modify: `README.md` (the "Themes for agents" subsection under Config; the Status rows; the
  Updating section's note that 0.4.0 moves the daemon and every window together), `docs/themes.md`
  (the toolkit pointer, if Task 4 left it), `crates/arkos/src/*.rs` (remove any "Used from Task"
  attribute left)

- [ ] **Step 1: README** per the README constraint, written against the code's actual output
  lines (run nothing; read `agent_cli.rs`'s strings).
- [ ] **Step 2: Gates**: `grep -rn "Used from Task" crates/` empty; no em-dash in README.md,
  docs/agent-guide.md, docs/themes.md or `crates/*/src/**/*.rs`; the warning check; the workspace
  suite. `git commit -m "README: themes for agents, the guide, the discovery layers"`.

---

## Deferred, for the user to rule on

- `arkos settings set keybindings.<action>` (off the list by the approved default).
- A configurable wait (`--wait <secs>`) for window answers.
- The guide as a Codex-native file; a project-level skill install (`--install --project`).
- `arkos theme preview` for one window only (`--pid`); today every window previews.
- A `--watch` mode for `arkos theme contrast` that re-reports on every save.
- The daemon answering `Customize` for windows on protocol 5 (they are told to update instead).

## Self-review notes

- Spec section 3: Task 2. Section 4: Task 3. Section 5: Tasks 1, 2, 3. Section 6: Task 3. Section
  7: Task 4. Section 8: Task 4. Section 9: Task 5. Section 11: Task 4 (the hook, the skill,
  `--help`).
- Deviations from the spec's letter: the guide is compiled into the binary and `--path` writes a
  copy under Application Support instead of a bundle Resources file (no packaging step, always
  current); `Customized` carries `expected` so the client can name silent windows; `settings set`
  allows a preset's `size` or `line_height` alone because the preset table must already exist.
