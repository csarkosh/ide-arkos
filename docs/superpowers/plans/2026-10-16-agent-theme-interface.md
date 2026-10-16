# The agent theme interface (milestone 8b)

Status: plan reviewed (opus, 2026-10-16, "With fixes") and amended; the rulings on the reviewer's
questions are in the ledger and in the constraints below. Spec:
`docs/superpowers/specs/2026-10-16-agent-theme-interface-design.md` (approved by Cyrus on
2026-10-16, "approve, defaults accepted": `arkos theme use`, `arkos guide`, keybindings off the
settings allow-list, a two-second wait for window answers, the protocol bump at 0.4.0, the skill
at user level). Written against main at 0.3.4 (`f51470a`). Ships as 0.4.0.

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
  after Task 5 `grep -rn "Used from Task" crates/` prints nothing. A field or variant payload that
  only tests read counts as unreached (derived `PartialEq` and `Debug` do not count as reads).
- No em-dashes in any file. Comments are full sentences. Headless tests only: no test opens a
  window; socket tests use the daemon test harness in `crates/arkosd/tests/daemon.rs` and the fake
  daemon pattern in `hook_cli.rs`'s tests.
- **Protocol** (`crates/protocol/src/lib.rs`): `PROTOCOL_VERSION` 5 becomes 6; `pub const
  CUSTOMIZE_FROM: u32 = 6`; `pub const CUSTOMIZE_WAIT: Duration = Duration::from_secs(2)` lives
  here (both crates name it). New messages, all `Serialize, Deserialize, Debug, Clone, PartialEq,
  Eq`, appended at the end of their enums (the codec's index asserts at lib.rs ~190 gain the new
  indexes: `Request::Customize` 21, `Request::CustomizeDone` 22, `Response::Customized` 9,
  `Event::Customize` 4): `Request::Customize { id: u64, action: CustomizeAction }` (from a tool
  client), `Request::CustomizeDone { id: u64, outcome: CustomizeOutcome }` (from a window,
  answered `Ok` whether or not the id is still waiting), `Event::Customize { id: u64, action:
  CustomizeAction }` (to project windows only), `Response::Customized { outcomes:
  Vec<CustomizeOutcome>, missing: Vec<u32>, too_old: Vec<u32> }` (to the tool client: `missing`
  the pids of windows that were asked and did not answer in time, `too_old` the pids of connected
  project windows on a protocol below `CUSTOMIZE_FROM`, which were not asked). `pub enum
  CustomizeAction { PreviewTheme { toml: String }, EndPreview, ReloadConfig, ReportState }`.
  `pub struct CustomizeOutcome { pub pid: u32, pub project: PathBuf, pub result: Result<Report,
  String> }`. `pub struct Report { pub theme: String, pub dark: bool, pub system_is_dark: bool,
  pub preset: String, pub family: String, pub preview: Option<String>, pub config_error:
  Option<String>, pub theme_error: Option<String> }` with `Default`. The existing tests that
  assert `PROTOCOL_VERSION == 5` (lib.rs ~193, codec.rs ~124) move to 6. `crates/protocol/
  Cargo.toml` gains `[dev-dependencies] serde_json.workspace = true` for the round-trip test (the
  codec itself is postcard and tolerates appended variants only through the version check: a
  0.3.x window and a 0.4 daemon are a version mismatch handled by the existing Hello path, and a
  0.4 window with a 0.3 daemon likewise).
- **Which clients are windows**: a project window is a connected client with `protocol >=
  CUSTOMIZE_FROM`, `ui_pid.is_some()` and `ui_cwd.is_some()` (the launch chooser says Hello with
  no folder and must not be asked, nor `arkos doctor`'s probe, nor tool clients on protocol 0).
  `Event::Customize` goes only to those; the existing `broadcast` and its filter stay as they are
  for the old events; add `fn customize_targets(st: &State) -> (Vec<(ClientId, u32)>, Vec<u32>)`
  returning the targets with their pids and the too-old project windows' pids.
- **The daemon relay** (`crates/arkosd/src/server.rs`): `Request::Customize` from any client is
  given a `CustomizeWait { client: ClientId, msg_id: u32, tx: ClientSink, waiting: Vec<u32>,
  outcomes: Vec<CustomizeOutcome>, too_old: Vec<u32>, started: Instant }` in `State.customize:
  HashMap<u64, CustomizeWait>` keyed by the request's `id` (a duplicate `id` still waiting is
  answered `Error("customize id in flight")`); the event is sent to every target under the same
  lock and `waiting` holds the pids of the sends that succeeded; `dispatch` returns `None` (the
  answer comes later through `tx` with `msg_id`), except when `waiting` is empty, when it answers
  `Customized { outcomes: vec![], missing: vec![], too_old }` at once. A `CustomizeDone` whose
  `id` is waiting appends its outcome and removes its pid from `waiting`; when `waiting` is empty
  the record is removed and `Customized` is sent; a `CustomizeDone` for an unknown or finished
  `id` is answered `Ok` and dropped (a late answer must never produce an `Error`, which the window
  would show as a banner). A thread spawned per record sleeps `CUSTOMIZE_WAIT` and then calls
  `flush_customize(&state, id)`, which, if the record is still there, sends `Customized` with
  `missing = waiting`. `disconnect` removes records whose `client` is the leaving client (the tool
  hung up) and, for a leaving window, removes its pid from every record's `waiting` (flushing a
  record that becomes complete). `Shared` is the existing `Arc<Mutex<State>>`.
- **The window** (`crates/arkos/src/main.rs`): `Root::route_events` (~2917, an `if let` today, so
  no arm is needed for Task 1) gains a `Customize` arm in Task 2: `PreviewTheme { toml }` runs
  `theme::parse`, on `Ok` calls `set_theme` and sets `self.preview = Some(name)` (a new
  `Option<String>` field; the config is untouched), on `Err` reports the error; `EndPreview` clears
  `preview` and re-resolves through `resolve_theme` and `switch_theme_to`; `ReloadConfig` runs
  `reload_config` and reports; `ReportState` reports. Every arm ends with
  `client.send(Request::CustomizeDone { id, outcome })` through `self.sessions.client`
  (`Option<Arc<Client>>`; with no client the event cannot have arrived) and a shared `fn report(&self)
  -> Report` (`theme: self.theme.name`, `dark: !self.theme.is_light()`, `system_is_dark`, `preset:
  self.text_preset` (a `String`), `family: self.family()` (the effective family, ~1136), `preview`,
  `config_error`, `theme_error`). A preview ends (`preview = None`) at the start of `reload_config`
  (both branches: when the config fails to load, call `switch_theme_to(self.resolve_theme(), cx)`
  so the screen matches the report), at a theme file change, and in `set_system_appearance`
  (~1049). The outcome's `pid` is `std::process::id()` and `project` is `self.project`. The
  exhaustive `Event` matches that must gain the new variant in Task 1: `sessions.rs`
  `group_by_session` (~164), `stash_if_restoring` (~537) and `note_events` (~582 to 590), all
  treating it as not a session event.
- **The watcher** (`crates/arkos/src/config_watch.rs`, `POLL` is 500 ms): `start(root, path, cx)`
  watches a second path it asks the Root for after every poll with `root.read_with(cx, |root, _|
  root.theme_file())` (the closure's `cx` is `&mut AsyncApp`; `Err` means the window is gone:
  break). `Root::theme_file()` is `paths::themes_dir().join(format!("{name}.toml"))` for `name =
  theme::pick_name(self.config.appearance.mode, self.system_is_dark, &self.config.theme)` (the
  configured name, as `open_theme` ~737 resolves it, never `theme.name`: a preview or a failed
  load must not move the watched path). The loop keeps `last: [Option<(u128, u64)>; 2]`; after
  any reload it re-reads the theme path and takes that path's fingerprint fresh, so a path change
  is not counted as an edit. A change of either fingerprint calls `reload_config`, which already
  re-resolves the theme through `switch_theme_to` and `keep_or_switch` (theme.rs ~350): the
  banner for a bad theme file is `theme_error`, whose text `theme: {path}: {err}; keeping {name}`
  already names the file (leave it; `config_error` is never used for theme failures). A theme
  file edit while `config.toml` itself is broken is not applied until the config loads again
  (stated in the guide). Pure helper `fn changed(last: &[Option<(u128, u64)>; 2], now:
  &[Option<(u128, u64)>; 2]) -> bool` with a test.
- **The tool client** (`crates/arkos/src/agent_cli.rs`, new): `pub fn customize(socket: &Path,
  action: CustomizeAction, wait: Duration) -> Result<Answer, String>` connects as
  `hook_cli::send_one` does but through a new `hook_cli::send_one_with(socket, request, timeout)`
  with `timeout = wait + 1 s` (`send_one`'s own `TIMEOUT` is 2 s, equal to the cut); no Hello (a
  tool client stays on protocol 0, outside every broadcast and outside `State.windows`); sends
  `Customize { id: unix nanos, action }`; `pub struct Answer { pub outcomes: Vec<CustomizeOutcome>,
  pub missing: Vec<u32>, pub too_old: Vec<u32> }`. `Err` cases, each its own line: the socket is
  not there or refuses (`no window running`, exit 3 for the commands that need one); the daemon
  hangs up without answering (`arkosd is older than this command; quit and reopen Arkos IDE`,
  exit 1); any other io error with its text (exit 1).
- **Commands and exit codes**: 0 done; 1 refused or failed (a bad file, an unknown name, a window
  that reported an error or whose report does not show the requested change, a low contrast pair,
  an old daemon); 2 usage (the usage text on stderr); 3 a window was needed and none is running.
  `use` and `settings set` exit 0 with `saved; no window running` when there is no daemon or no
  window, and exit 0 when every answering window applied the change even if some were silent or
  too old (those print their lines). `--json` anywhere in the arguments prints one JSON value on
  stdout and nothing else; without it, human lines. The window lines: `window {pid} ({project}):
  {text}`; `window {pid}: no answer`; `window {pid}: too old for this command`. All output goes
  through `out` and `err` writers so tests capture it; `main()` wires the real streams. `main()`
  dispatches `theme`, `settings`, `config`, `guide`, `--help`/`-h`/`help` before the line that
  treats a first argument as a folder (~625), so a folder literally named `config`, `settings` or
  `guide` is opened with `arkos ./config` (the README says so).
- **`arkos theme`** (`theme.rs` keeps `list` and `validate` in `theme_cli_to`, which gains `--json`;
  `theme::theme_cli` is removed; the new subcommands live in `agent_cli.rs`): `arkos theme` with no
  arguments prints the usage and `see: arkos guide` on stderr, exit 2.
  - `list [--json]`: JSON `[{ "name", "source": "built-in" | "file" | "invalid", "error"?, "path"? }]`.
  - `validate <file> [--json]`: JSON `{ "ok": bool, "name"?, "error"? }`.
  - `template [<built-in>] [--json]`: the built-in's TOML (default navy); an unknown built-in exits
    1; with `--json` the schema `{ "tables": [{ "name", "keys": [{ "key", "meaning",
    "contrast_with"?, "needs"? }] }] }` from `theme::schema() -> &'static [TableDoc]` (const data:
    the top-level `name` as a table named "" with one key, then chrome, terminal, semantic, tree,
    viewer, font; a test checks the schema's keys equal the union of the keys of every `BUILTIN`
    document, since navy has no `[font]` table and peach-light has all three font keys).
  - `contrast <file-or-name> [--json]`: `theme::contrast_pairs(&Theme) -> Vec<Pair { what: String,
    fg: Rgb, bg: Rgb, needs: f64 }>` moved out of the test module (the general contrast test
    iterates it, so the CLI and the test cannot drift). Labels are `{table}.{key} on {surface}`:
    the pairs of today's `foregrounds` relabelled (`tree.ignored on window_bg`, `semantic.working
    on terminal.background`, `chrome.text_dim on status_bar`, and so on) at 4.5, except `chrome.text
    on window_bg` listed once at 7.0; `terminal.foreground on terminal.background` at 11.0; the
    ANSI pairs `ansi {i} on terminal.background` at 4.5 skipping index 0 unless `is_light()`; the
    seven surface pairs from milestone 8a's Timo test at 4.5 (they pass on all five built-ins).
    `theme::contrast_report(&Theme) -> (bool, Vec<(Pair, f64)>)`. Lines `{what} {ratio:.1}:1 ok` or
    `{what} {ratio:.1}:1 LOW (needs {needs})`; exit 1 when any is low. JSON `{ "name", "ok",
    "pairs": [{ "what", "fg", "bg", "ratio", "needs", "ok" }] }` with the colours as `#rrggbb`.
    The comment on `palette::contrast`'s dead-code attribute changes (it is used outside tests now).
  - `add <file> [--replace] [--json]`: `theme::import_file`; `added {name}` or `replaced {name}`;
    `Err("exists")` prints `exists: {name}; pass --replace` and exits 1. JSON `{ "name", "replaced" }`.
  - `use <name-or-file> [--dark|--light] [--replace] [--json]`: a path ending in `.toml` is added
    first; the name must then load (`theme::load(name, themes)`; an unknown name exits 1 `unknown
    theme: {name}`, an invalid file exits 1 with its error, nothing written); the key is `dark` or
    `light` from the flag, else from the config's `[appearance] mode` (`dark` or `light`), else
    (`system`) from a `ReportState` round trip when windows answer (`system_is_dark` of the first),
    else `dark`; `config::load` first (it writes the defaults when the file is missing), then
    `config::set_values` writes `["theme", key]`; then the `ReloadConfig` action: a window whose
    report has `theme == name` and no `theme_error` and no `config_error` prints `{name} applied`,
    any other report prints its error (`theme_error`, `config_error`, or `shows {other}`) and the
    command exits 1. JSON `{ "name", "key", "saved": true, "windows": [{ "pid", "project",
    "theme"?, "error"? }], "missing": [pids], "too_old": [pids] }`.
  - `preview <name-or-file> [--json]` and `preview --end`: the TOML (a file's text, a built-in's, or
    a themes-folder file's) validated by `theme::parse` first (exit 1, nothing sent); then
    `PreviewTheme { toml }` or `EndPreview`; lines `previewing {name}` / `preview ended, {theme}` or
    the window's error (exit 1); exit 3 with no window.
  - `current [--json]`: the file part from `std::fs::read_to_string` plus `config::parse` (`dark`,
    `light`, `mode`), lines `config: dark {d}, light {l}, mode {m}` (or `config: {error}`), then
    `ReportState` per window: `{theme} ({dark|light}, system is {dark|light}), preset {preset},
    family {family}` plus `, preview {name}`, `, config error: {text}`, `, theme error: {text}` when
    set; exit 0 with no window.
- **`arkos settings`** (`agent_cli.rs`): `get [<key>] [--json]`: with no key, the config file's
  text verbatim then the window lines as `current` prints them (JSON `{ "file": text, "windows":
  [reports] }`); with a key, the value found by walking `toml::from_str::<toml::Value>` of the
  file, printed bare (strings unquoted; numbers and booleans as TOML writes them; JSON `{ "key",
  "value" }`), exit 1 `no such key` when absent. `set <key> <value> [--json]`: `pub const
  SETTABLE: &[&str] = &["appearance.mode", "theme.dark", "theme.light", "font.family",
  "font.preset", "font.presets.*.size", "font.presets.*.line_height", "update.auto",
  "recovery.start_at_login"]` (a `*` segment matches exactly one bare name); another key exits 2
  with the list; the value is typed by the key (`size` and `line_height` f64, `update.auto` bool,
  the rest strings) and checked before writing with the config's own rules: `appearance.mode` in
  system|dark|light, `recovery.start_at_login` in ask|on|off, a theme name that `theme::load`s,
  `font.preset` a preset of the merged config (`config::load(..).font.presets`, built-ins
  included), `size` 8 to 48 and `line_height` 1.0 to 2.5 (`Config::validate`, config.rs ~230),
  `font.family` any non-empty string; a preset's `size` or `line_height` is written together with
  the other field (taken from the merged preset) when the file has no table for that preset, since
  `TextPreset` requires both; then the edits are applied to the file's text in memory and run
  through `config::parse` before anything is written (a new pure `config::apply_edits(text,
  edits) -> Result<String, String>` split out of `set_values`, which calls it); then
  `config::set_values` (after `config::load` created a missing file), then the `ReloadConfig` action
  per window as `use` does (the report's `config_error` is the failure), or `saved; no window
  running`. JSON `{ "key", "value", "saved", "windows": [...], "missing", "too_old" }`.
- **`arkos config reload [--json]`**: the `ReloadConfig` action alone; one line per window; exit 3
  with none.
- **The guide**: `docs/agent-guide.md` (spec section 7's outline; about 150 lines; every
  command's exit codes and `--json` shape; the worked Timo Purple example; the rules; the
  statement that a theme edit waits while `config.toml` is broken) compiled in with
  `include_str!("../../../docs/agent-guide.md")` (from `crates/arkos/src/guide.rs`) as
  `guide::TEXT`; `arkos guide` prints it (exit 0); `arkos guide --path` writes it to `<app
  support>/agent-guide.md` when the file is missing or differs and prints that path; `arkos guide
  --install` writes `<skills dir>/arkos-ide/SKILL.md` through `guide::install_skill(dir)`: the
  front matter `---\nname: arkos-ide\ndescription: Customize Arkos IDE: themes, fonts and
  settings. Use when the user asks to change how Arkos looks or behaves.\n---\n`, then one line
  `<!-- arkos-guide: {stamp} -->` where `stamp` is `guide::STAMP`, the first 16 hex digits of the
  SHA-256 of `TEXT` (the `sha2` crate is already a dependency of the workspace: check; else a
  simple FNV-1a 64 in `guide.rs` with a test), then `TEXT`; refuses (exit 1, the path on stderr) a
  file there whose first line is not `---` followed by `name: arkos-ide`; then prints the paragraph
  for `AGENTS.md` or `CLAUDE.md`: `Arkos IDE: run \`arkos guide\` before changing its theme or
  settings.` `guide::skill_state(dir) -> SkillState::{Missing, Current, Outdated(String), Foreign}`
  compares the stamp (`Outdated` carries the old stamp, which the doctor line shows). `guide::
  skills_dir()` is `$CLAUDE_CONFIG_DIR/skills` when that variable is set (as `hook_cli::
  claude_settings_path` honours it, ~239) else `~/.claude/skills`. `arkos --help` (new; today only
  `--version` exists) prints `USAGE` with `guide` first.
- **The hook** (`hook_cli.rs`): `run_with` gains `out: &mut dyn Write` (the call at ~306 and the
  eight test calls change); for `--event session-start`, after the session id parses (~113), it
  writes `CONTEXT_LINES` to `out` before the daemon request; nothing on stdout otherwise; `arkos
  notify` still exits 0 always. `pub const CONTEXT_LINES: &str = "This session runs inside Arkos
  IDE.\nTo change its theme, fonts or settings, run \`arkos guide\` first and follow it.\n"`.
- **The doctor**: `doctor_panel::probe_all` gains `CheckId::AgentGuide` ("Agent guide for Claude
  Code") from `Probe.skill: SkillState` and `Probe.claude_present: bool` (the Claude config dir
  exists): `State::Fix("not installed: agents cannot find the toolkit")` with "Install";
  `State::Good("installed")`; `State::Fix("older than this build ({old})")` with "Update";
  `State::Broken("a different skill sits at {path}")`; when the Claude config dir does not exist,
  `State::Pinned("no Claude Code config found")` with no button, not counted as a fix. The two
  `Probe` construction sites change (main.rs ~763 and the test helper doctor_panel.rs ~89, which
  sets `skill: SkillState::Current, claude_present: true` so the existing counts hold); the
  existing tests' id vectors gain `AgentGuide`; `doctor_action`'s exhaustive match (main.rs ~783)
  gains the arm calling `guide::install_skill` on the background executor with a notice. The
  read-only `doctor::report` gains a `themes` section from `pub fn themes_section(themes: &Path,
  config: &Path, skill: &SkillState) -> Vec<String>`: `{n} themes, all valid` or one line per
  invalid file `{path}: {error}`; `theme.dark names no theme: {name}` (and `light`) only when
  neither a built-in nor a file of that name exists; `agent guide skill: not installed | installed
  | older than this build ({old}) | a different skill at {path}`; it reads the config with
  `read_to_string` and `config::parse` (never `config::load`, which writes). `doctor::Where`
  gains `themes`, `config` and `skills` paths (its two test constructions ~227 and ~279 change). A
  "preview still active" check is deferred (a two-second round trip in a read-only command).
- **The palette** gains "Install agent guide" (`Command::InstallAgentGuide`, row `"install agent
  guide"` unbound, Section Maintenance, SPEC_ROWS and the count assertions move by one unbound
  row; the label comes from `keymap::label_for`'s default), handled in `Root::run`'s exhaustive
  `match cmd` next to `InstallHooks` (~2504) with the same install and notice. DEFAULT_CONFIG's
  keybinding comments gain `# "install agent guide" = ""`.
- **README**: a "Themes for agents" subsection under Config with the command table (command, what
  it does, exit codes), the live reload of the active theme file, `arkos settings`, `arkos guide`,
  the three discovery layers, the folder-name note; Status rows from spec sections 9 and 11; the
  Updating section notes that 0.4.0 moves the daemon and every window together.

## Review Focus

1. A `Customize` request while no project window is connected, with the chooser open: the client
   gets `Customized { outcomes: [], missing: [], too_old: [] }` at once; the command exits 3
   (`use` and `settings set` exit 0 with `saved; no window running`) (Tasks 1 and 3).
2. A window that never answers: the tool client gets the others' outcomes after two seconds and
   prints `window {pid}: no answer` (Task 1's daemon test, Task 3's client test).
3. `arkos theme use ../x.toml`: the import refuses a name that is not a bare stem before anything
   is written (Task 3, through `import_file`).
4. A preview is active and the user saves `config.toml`: the preview ends and the config's theme
   shows, also when the saved config fails to load (Task 2).
5. The configured theme's file is edited to garbage: the window keeps the last good theme and
   `theme_error` names the file; the next good save clears it; the file appearing for a built-in
   name is picked up (Task 2).
6. `arkos settings set font.presets.compact.size 15` on a config with no `[font.presets.compact]`
   table writes both fields and the file still loads (Task 3).

## File structure

- Create: `crates/arkos/src/agent_cli.rs`, `crates/arkos/src/guide.rs`, `docs/agent-guide.md`.
- Modify: `crates/protocol/src/lib.rs`, `crates/protocol/src/codec.rs` (the version assert),
  `crates/protocol/Cargo.toml`, `crates/arkosd/src/server.rs` (+ `tests/daemon.rs`),
  `crates/arkos/src/main.rs`, `sessions.rs`, `config_watch.rs`, `theme.rs`, `palette.rs` (a
  comment), `config.rs` (`apply_edits`), `hook_cli.rs`, `doctor.rs`, `doctor_panel.rs`,
  `keymap.rs`, `command_palette.rs`, `README.md`, `docs/themes.md` (a pointer to the toolkit).

---

### Task 1: The protocol and the daemon relay

**Files:**
- Modify: `crates/protocol/src/lib.rs`, `crates/protocol/src/codec.rs`, `crates/protocol/Cargo.toml`,
  `crates/arkosd/src/server.rs` (`State.customize`, `customize_targets`, the `Customize` and
  `CustomizeDone` arms in `dispatch` ~419, `flush_customize`, the deadline thread, `disconnect`
  ~331), `crates/arkosd/tests/daemon.rs` (two tests; `Conn` gains `next_event()` and `response()`;
  `send` exists already and returns the message id), `crates/arkos/src/sessions.rs` (the three
  exhaustive `Event` matches gain `Event::Customize { .. }` treated as not a session event)

**Interfaces:** as the Protocol, Which clients are windows, and Daemon relay constraints state.

- [ ] **Step 1: Failing tests**

```rust
// protocol lib.rs tests
#[test]
fn customize_messages_round_trip_through_json_and_sit_at_the_end_of_their_enums() {
    let a = CustomizeAction::PreviewTheme { toml: "name = \"x\"".into() };
    let r = Report { theme: "navy".into(), dark: true, system_is_dark: true, preset: "comfortable".into(), family: "Menlo".into(), preview: None, config_error: None, theme_error: None };
    let o = CustomizeOutcome { pid: 7, project: "/p".into(), result: Ok(r.clone()) };
    for req in [Request::Customize { id: 9, action: a.clone() }, Request::CustomizeDone { id: 9, outcome: o.clone() }] {
        let text = serde_json::to_string(&req).unwrap();
        assert_eq!(serde_json::from_str::<Request>(&text).unwrap(), req);
    }
    let ev = Event::Customize { id: 9, action: CustomizeAction::ReportState };
    assert_eq!(serde_json::from_str::<Event>(&serde_json::to_string(&ev).unwrap()).unwrap(), ev);
    let resp = Response::Customized { outcomes: vec![o], missing: vec![8], too_old: vec![] };
    assert_eq!(serde_json::from_str::<Response>(&serde_json::to_string(&resp).unwrap()).unwrap(), resp);
    assert_eq!(PROTOCOL_VERSION, 6);
    assert_eq!(CUSTOMIZE_FROM, 6);
    assert_eq!(CUSTOMIZE_WAIT, Duration::from_secs(2));
    // The index asserts follow the file's convention at ~190: Customize 21, CustomizeDone 22,
    // Customized 9, Event::Customize 4.
}

// arkosd tests/daemon.rs
#[test]
fn a_customize_request_is_relayed_to_every_project_window_and_answered_when_all_report() {
    let d = Daemon::start(&[]);
    let base = std::process::id();
    let mut w1 = d.connect_as(base + 1);
    let mut w2 = d.connect_as(base + 2);
    let mut chooser = d.connect_raw();
    // A chooser says Hello with no folder and must not be asked.
    chooser.request(Request::Hello { protocol_version: PROTOCOL_VERSION, ui_pid: base + 3, ui_exe: None, ui_cwd: PathBuf::new() });
    let mut tool = d.connect_raw();
    tool.send(Request::Customize { id: 1, action: CustomizeAction::ReportState });
    for (w, pid) in [(&mut w1, base + 1), (&mut w2, base + 2)] {
        assert_eq!(w.next_event(), Event::Customize { id: 1, action: CustomizeAction::ReportState });
        assert_eq!(w.request(Request::CustomizeDone { id: 1, outcome: CustomizeOutcome { pid, project: "/p".into(), result: Ok(Report::default()) } }), Response::Ok);
    }
    match tool.response() {
        Response::Customized { outcomes, missing, too_old } => {
            assert_eq!(outcomes.iter().map(|o| o.pid).collect::<Vec<_>>(), vec![base + 1, base + 2]);
            assert!(missing.is_empty() && too_old.is_empty());
        }
        other => panic!("{other:?}"),
    }
    // A late answer is still Ok, never an error the window would show.
    assert_eq!(w1.request(Request::CustomizeDone { id: 1, outcome: CustomizeOutcome { pid: base + 1, project: "/p".into(), result: Ok(Report::default()) } }), Response::Ok);
}

#[test]
fn a_silent_window_is_named_after_the_wait_and_no_window_answers_at_once() {
    let d = Daemon::start(&[]);
    let base = std::process::id();
    let _w1 = d.connect_as(base + 1);
    let mut w2 = d.connect_as(base + 2);
    let mut tool = d.connect_raw();
    let t0 = Instant::now();
    tool.send(Request::Customize { id: 5, action: CustomizeAction::EndPreview });
    let _ = w2.next_event();
    w2.request(Request::CustomizeDone { id: 5, outcome: CustomizeOutcome { pid: base + 2, project: "/p".into(), result: Err("no preview".into()) } });
    let r = tool.response();
    assert!(t0.elapsed() >= Duration::from_millis(1900) && t0.elapsed() < Duration::from_secs(5), "{:?}", t0.elapsed());
    match r {
        Response::Customized { outcomes, missing, .. } => {
            assert_eq!(outcomes.iter().map(|o| o.pid).collect::<Vec<_>>(), vec![base + 2]);
            assert_eq!(missing, vec![base + 1]);
        }
        other => panic!("{other:?}"),
    }
    let d2 = Daemon::start(&[]);
    let mut tool = d2.connect_raw();
    tool.send(Request::Customize { id: 1, action: CustomizeAction::ReportState });
    assert_eq!(tool.response(), Response::Customized { outcomes: vec![], missing: vec![], too_old: vec![] });
}
```

Imports in the test file: `CustomizeAction`, `CustomizeOutcome`, `Report`, `Event`, `PathBuf`,
`Instant`. `next_event()` reads frames until an `Event`; `response()` reads frames until a
`Response` (the harness's `response_to` ~1791 is the model).

- [ ] **Step 2: Run to see them fail**: `nice -n 15 ~/.cargo/bin/cargo test -p protocol` and
  `nice -n 15 ~/.cargo/bin/cargo test -p arkosd --test daemon -- customize silent`.
- [ ] **Step 3: Implement** per the constraints. Keep `dispatch`'s shape (an `Option<Response>`).
  The arkos crate must compile with the new variants (the three `sessions.rs` matches); nothing
  else in arkos changes in this task.
- [ ] **Step 4: Run, check, commit**: the protocol and arkosd tests, the workspace suite, the
  warning check. `git commit -m "Protocol 6: a Customize request the daemon relays to every project window and answers in two seconds"`.

---

### Task 2: The window answers, previews, and watches its theme file

**Files:**
- Modify: `crates/arkos/src/main.rs` (`route_events` ~2917: the `Customize` arm; `preview:
  Option<String>` on Root; `fn report(&self) -> Report`; `fn theme_file(&self) -> PathBuf`;
  `reload_config` ~1079 and `set_system_appearance` ~1049 end a preview; the config-error branch
  of `reload_config` re-resolves the theme when a preview was active), `crates/arkos/src/config_watch.rs`

**Interfaces:**
```rust
impl Root {
    pub fn report(&self) -> protocol::Report;
    pub fn theme_file(&self) -> PathBuf; // themes_dir/<configured name>.toml
    fn customize(&mut self, id: u64, action: CustomizeAction, cx: &mut Context<Self>); // applies and sends CustomizeDone
}
pub fn customize_outcome(action: &CustomizeAction, parsed: Result<Theme, String>, after: Report) -> Result<Report, String>; // pure: the Err text for a bad preview, the report otherwise
pub fn theme_file_for(mode: AppearanceMode, system_is_dark: bool, cfg: &ThemeConfig, themes: &Path) -> PathBuf; // pure
pub fn changed(last: &[Option<(u128, u64)>; 2], now: &[Option<(u128, u64)>; 2]) -> bool;
```

- [ ] **Step 1: Failing tests**

```rust
// main.rs tests
#[test]
fn a_bad_preview_reports_its_error_and_the_watched_file_is_the_configured_theme() {
    let after = Report { theme: "x".into(), preview: Some("x".into()), ..Report::default() };
    let bad = customize_outcome(&CustomizeAction::PreviewTheme { toml: "nope".into() }, Err("expected an equals".into()), after.clone());
    assert_eq!(bad, Err("expected an equals".into()));
    let good = customize_outcome(&CustomizeAction::PreviewTheme { toml: "...".into() }, Ok(theme::builtin("navy").unwrap()), after.clone());
    assert_eq!(good.unwrap().preview.as_deref(), Some("x"));
    assert!(customize_outcome(&CustomizeAction::ReportState, Err("unused".into()), after).is_ok(), "only a preview can fail");
    let cfg = config::ThemeConfig { dark: "timo-purple".into(), light: "navy-light".into() };
    assert_eq!(theme_file_for(config::AppearanceMode::System, true, &cfg, Path::new("/t")), PathBuf::from("/t/timo-purple.toml"));
    assert_eq!(theme_file_for(config::AppearanceMode::Light, true, &cfg, Path::new("/t")), PathBuf::from("/t/navy-light.toml"));
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
- [ ] **Step 3: Implement** per the Window and Watcher constraints. `set_theme` is the same call
  the config path uses, so About, Settings and Keybindings follow a preview too (`push_theme`).
- [ ] **Step 4: Run, check, commit**: `git commit -m "A window answers Customize: preview, end, reload, report; the configured theme file is watched"`.

---

### Task 3: The tool client and the `theme`, `settings` and `config` commands

**Files:**
- Create: `crates/arkos/src/agent_cli.rs`
- Modify: `crates/arkos/src/theme.rs` (`Pair`, `contrast_pairs`, `contrast_report`, `schema`, the
  general contrast test over `contrast_pairs`, `--json` for `list` and `validate`, `theme_cli`
  removed), `crates/arkos/src/palette.rs` (the attribute comment), `crates/arkos/src/config.rs`
  (`apply_edits` split out of `set_values`), `crates/arkos/src/hook_cli.rs` (`send_one_with`),
  `crates/arkos/src/main.rs` (the dispatch before the folder line ~625; `--help`)

**Interfaces:**
```rust
// theme.rs
#[derive(Debug, Clone, PartialEq)] pub struct Pair { pub what: String, pub fg: Rgb, pub bg: Rgb, pub needs: f64 }
pub fn contrast_pairs(t: &Theme) -> Vec<Pair>;
pub fn contrast_report(t: &Theme) -> (bool, Vec<(Pair, f64)>);
pub struct KeyDoc { pub key: &'static str, pub meaning: &'static str, pub contrast_with: Option<&'static str>, pub needs: Option<f64> }
pub struct TableDoc { pub name: &'static str, pub keys: &'static [KeyDoc] }
pub fn schema() -> &'static [TableDoc];
// config.rs
pub fn apply_edits(text: &str, edits: &[(&[&str], toml_edit::Value)]) -> Result<String, String>;
// hook_cli.rs
pub fn send_one_with(socket: &Path, request: Request, timeout: Duration) -> std::io::Result<Response>;
// agent_cli.rs
pub struct Answer { pub outcomes: Vec<CustomizeOutcome>, pub missing: Vec<u32>, pub too_old: Vec<u32> }
pub enum Reach { NoWindow, OldDaemon, Io(String) }
pub fn customize(socket: &Path, action: CustomizeAction, wait: Duration) -> Result<Answer, Reach>;
pub struct Env<'a> { pub socket: &'a Path, pub config: &'a Path, pub themes: &'a Path, pub wait: Duration }
pub fn theme_cli_to(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32;
pub fn settings_cli_to(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32;
pub fn config_cli_to(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32;
pub fn theme_key_for_use(flag: Option<&str>, mode: AppearanceMode, windows_dark: Option<bool>) -> &'static str;
pub fn settable(key: &str) -> bool;
pub fn typed_value(key: &str, raw: &str) -> Result<toml_edit::Value, String>;
pub fn preset_edits<'a>(key_path: &'a [&'a str], value: toml_edit::Value, file_has_table: bool, merged: &TextPreset) -> Vec<(Vec<String>, toml_edit::Value)>;
pub const SETTABLE: &[&str];
pub const USAGE: &str; // arkos --help
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
    assert_eq!(rows.iter().find(|(p, r)| r < &p.needs).unwrap().0.what, "tree.ignored on window_bg");
    assert_eq!(rows.iter().filter(|(p, _)| p.what == "chrome.text on window_bg").count(), 1);
    assert_eq!(rows.iter().find(|(p, _)| p.what == "chrome.text on window_bg").unwrap().0.needs, 7.0);
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("low.toml");
    std::fs::write(&f, &text).unwrap();
    let env = Env { socket: &dir.path().join("none.sock"), config: &dir.path().join("config.toml"), themes: dir.path(), wait: Duration::from_millis(10) };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(theme_cli_to(&["contrast".into(), f.display().to_string()], &env, &mut out, &mut err), 1);
    let s = String::from_utf8(out).unwrap();
    assert!(s.lines().any(|l| l.starts_with("tree.ignored on window_bg ") && l.ends_with("LOW (needs 4.5)")), "{s}");
    assert!(s.lines().any(|l| l == format!("chrome.text on window_bg {:.1}:1 ok", crate::palette::contrast(t.chrome.text, t.chrome.window_bg))));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(theme_cli_to(&["contrast".into(), "navy".into(), "--json".into()], &env, &mut out, &mut err), 0);
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!((v["name"].as_str(), v["ok"].as_bool()), (Some("navy"), Some(true)));
    assert!(v["pairs"].as_array().unwrap().len() > 20);
}

#[test]
fn use_writes_the_right_key_refuses_an_unknown_name_and_says_so_without_a_window() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("config.toml");
    std::fs::write(&cfg, "# mine\n[appearance]\nmode = \"light\"\n[theme]\ndark = \"navy\" # d\nlight = \"navy-light\"\n").unwrap();
    let themes = dir.path().join("themes");
    let env = Env { socket: &dir.path().join("none.sock"), config: &cfg, themes: &themes, wait: Duration::from_millis(10) };
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
    let missing = dir.path().join("fresh").join("config.toml");
    let env2 = Env { socket: &dir.path().join("none.sock"), config: &missing, themes: &themes, wait: Duration::from_millis(10) };
    assert_eq!(theme_cli_to(&["use".into(), "navy".into(), "--dark".into()], &env2, &mut Vec::new(), &mut Vec::new()), 0, "a missing config is created first");
    assert!(missing.is_file());
    assert_eq!(theme_key_for_use(None, AppearanceMode::System, Some(false)), "light");
    assert_eq!(theme_key_for_use(None, AppearanceMode::System, None), "dark");
    assert_eq!(theme_key_for_use(Some("--light"), AppearanceMode::Dark, Some(true)), "light");
}

#[test]
fn settings_set_keeps_to_the_allow_list_types_the_value_and_refuses_bad_ones_before_writing() {
    assert!(settable("theme.dark") && settable("font.presets.big.size") && settable("update.auto"));
    assert!(!settable("keybindings.find") && !settable("font.presets.big.other") && !settable("font.presets.a.b.size"));
    assert_eq!(typed_value("update.auto", "true").unwrap().as_bool(), Some(true));
    assert_eq!(typed_value("font.presets.big.size", "18").unwrap().as_float(), Some(18.0));
    assert!(typed_value("font.presets.big.size", "60").is_err(), "the config allows 8 to 48");
    assert!(typed_value("font.presets.big.line_height", "2.6").is_err(), "the config allows 1.0 to 2.5");
    assert!(typed_value("appearance.mode", "blue").is_err());
    assert!(typed_value("recovery.start_at_login", "on").is_ok());
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("config.toml");
    std::fs::write(&cfg, config::DEFAULT_CONFIG).unwrap();
    let env = Env { socket: &dir.path().join("none.sock"), config: &cfg, themes: &dir.path().join("themes"), wait: Duration::from_millis(10) };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(settings_cli_to(&["set".into(), "keybindings.find".into(), "cmd-g".into()], &env, &mut out, &mut err), 2);
    assert!(String::from_utf8(err).unwrap().contains("theme.dark"), "the list is printed");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(settings_cli_to(&["set".into(), "font.preset".into(), "huge".into()], &env, &mut out, &mut err), 1, "no such preset");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(settings_cli_to(&["set".into(), "font.preset".into(), "compact".into()], &env, &mut out, &mut err), 0, "a built-in preset counts");
    assert_eq!(String::from_utf8(out).unwrap(), "saved; no window running\n");
    let text = std::fs::read_to_string(&cfg).unwrap();
    assert!(text.contains("preset = \"compact\""));
    assert_eq!(text.matches('#').count(), config::DEFAULT_CONFIG.matches('#').count(), "comments kept");
    // A preset field alone on a file with no table for it: both fields are written and it loads.
    std::fs::write(&cfg, "[font]\npreset = \"compact\"\n").unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(settings_cli_to(&["set".into(), "font.presets.compact.size".into(), "15".into()], &env, &mut out, &mut err), 0);
    let loaded = config::load(&cfg).unwrap();
    assert_eq!(loaded.font.presets["compact"].size, 15.0);
    assert!(std::fs::read_to_string(&cfg).unwrap().contains("line_height"));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(settings_cli_to(&["get".into(), "font.preset".into()], &env, &mut out, &mut err), 0);
    assert_eq!(String::from_utf8(out).unwrap(), "compact\n");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(settings_cli_to(&["get".into(), "nope.key".into()], &env, &mut out, &mut err), 1);
}

#[test]
fn the_client_reports_every_window_the_silent_ones_and_an_old_daemon() {
    use std::io::Write as _;
    use protocol::codec::{read_frame, write_frame};
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("d.sock");
    let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
    let server = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut r = std::io::BufReader::new(s.try_clone().unwrap());
        let mut w = std::io::BufWriter::new(s);
        let msg: ClientMessage = read_frame(&mut r).unwrap().unwrap();
        let Request::Customize { action: CustomizeAction::ReportState, .. } = msg.request else { panic!() };
        let one = CustomizeOutcome { pid: 3, project: "/p".into(), result: Ok(Report { theme: "navy".into(), ..Report::default() }) };
        write_frame(&mut w, &ServerMessage::Response { id: msg.id, response: Response::Customized { outcomes: vec![one], missing: vec![4], too_old: vec![5] } }).unwrap();
        w.flush().unwrap();
        // The second connection: an old daemon that cannot decode the frame hangs up.
        let (s, _) = listener.accept().unwrap();
        drop(s);
    });
    let got = customize(&sock, CustomizeAction::ReportState, Duration::from_secs(1)).unwrap();
    assert_eq!((got.outcomes[0].pid, got.missing.clone(), got.too_old.clone()), (3, vec![4], vec![5]));
    assert!(matches!(customize(&sock, CustomizeAction::ReportState, Duration::from_secs(1)), Err(Reach::OldDaemon)));
    server.join().unwrap();
    assert!(matches!(customize(&dir.path().join("absent.sock"), CustomizeAction::ReportState, Duration::from_millis(10)), Err(Reach::NoWindow)));
    let env = Env { socket: &dir.path().join("absent.sock"), config: &dir.path().join("c.toml"), themes: dir.path(), wait: Duration::from_millis(10) };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(theme_cli_to(&["preview".into(), "navy".into()], &env, &mut out, &mut err), 3);
    assert_eq!(String::from_utf8(err).unwrap(), "no window running\n");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(config_cli_to(&["reload".into()], &env, &mut out, &mut err), 3);
}

#[test]
fn the_schema_names_every_key_of_the_builtins_once_and_the_template_prints_a_builtin() {
    let mut from_toml: std::collections::BTreeSet<String> = Default::default();
    for (_, text) in theme::BUILTIN {
        let doc: toml::Value = toml::from_str(text).unwrap();
        for (table, v) in doc.as_table().unwrap() {
            match v.as_table() {
                Some(t) => for k in t.keys() { from_toml.insert(format!("{table}.{k}")); },
                None => { from_toml.insert(table.clone()); }
            }
        }
    }
    let from_schema: std::collections::BTreeSet<String> = theme::schema().iter().flat_map(|t| t.keys.iter().map(move |k| if t.name.is_empty() { k.key.to_string() } else { format!("{}.{}", t.name, k.key) })).collect();
    assert_eq!(from_schema, from_toml);
    let dir = tempfile::tempdir().unwrap();
    let env = Env { socket: &dir.path().join("none.sock"), config: &dir.path().join("c.toml"), themes: dir.path(), wait: Duration::from_millis(10) };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(theme_cli_to(&["template".into()], &env, &mut out, &mut err), 0);
    assert_eq!(String::from_utf8(out).unwrap(), theme::BUILTIN[0].1);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(theme_cli_to(&["template".into(), "nope".into()], &env, &mut out, &mut err), 1);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(theme_cli_to(&[], &env, &mut out, &mut err), 2);
    assert!(String::from_utf8(err).unwrap().contains("see: arkos guide"));
}
```

- [ ] **Step 2: Run to see them fail**: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- agent_cli:: theme:: config::`.
- [ ] **Step 3: Implement** per the Commands constraints. One more fake-daemon test over `use`
  with a window whose report shows another theme (exit 1, the `shows {other}` line) is welcome.
  `main()`: `theme` → `agent_cli::theme_cli` (real env: `paths::socket_path()`,
  `paths::config_file()`, `paths::themes_dir()`, `protocol::CUSTOMIZE_WAIT`), `settings`,
  `config`, `--help`; all before the folder line.
- [ ] **Step 4: Run, check, commit**: `git commit -m "arkos theme template, contrast, add, use, preview and current; arkos settings and config reload"`.

---

### Task 4: The guide, the skill, the hook line, the doctor and the palette

**Files:**
- Create: `docs/agent-guide.md`, `crates/arkos/src/guide.rs`
- Modify: `crates/arkos/src/main.rs` (`guide` dispatch; `Probe` at ~763; `doctor_action` ~783;
  `Root::run` ~2504), `crates/arkos/src/hook_cli.rs` (`run_with` gains `out`; `CONTEXT_LINES`),
  `crates/arkos/src/doctor.rs` (`Where` fields; `themes_section`), `crates/arkos/src/doctor_panel.rs`
  (`CheckId::AgentGuide`, `Probe.skill`, `Probe.claude_present`, the test helper and the id
  vectors), `crates/arkos/src/keymap.rs`, `command_palette.rs`, `config.rs` (the DEFAULT_CONFIG
  comment), `docs/themes.md`

**Interfaces:**
```rust
// guide.rs
pub const TEXT: &str = include_str!("../../../docs/agent-guide.md");
pub const SKILL_NAME: &str = "arkos-ide";
pub const SKILL_DESCRIPTION: &str = "Customize Arkos IDE: themes, fonts and settings. Use when the user asks to change how Arkos looks or behaves.";
pub const AGENTS_MD_LINE: &str = "Arkos IDE: run `arkos guide` before changing its theme or settings.";
pub fn stamp() -> String; // 16 hex digits over TEXT
pub fn skill_text() -> String;
#[derive(Debug, Clone, PartialEq, Eq)] pub enum SkillState { Missing, Current, Outdated(String), Foreign }
pub fn skill_state(dir: &Path) -> SkillState; // dir is the skills folder
pub struct Installed { pub path: PathBuf, pub replaced: bool }
pub fn install_skill(dir: &Path) -> Result<Installed, String>;
pub fn write_copy(app_support: &Path) -> Result<PathBuf, String>; // --path
pub fn guide_cli_to(args: &[String], skills_dir: &Path, app_support: &Path, out: &mut dyn Write, err: &mut dyn Write) -> i32;
pub fn skills_dir() -> PathBuf; // $CLAUDE_CONFIG_DIR/skills or ~/.claude/skills
pub fn skills_dir_from(config_dir: Option<&OsStr>, home: &Path) -> PathBuf; // pure
// hook_cli.rs
pub const CONTEXT_LINES: &str;
pub fn run_with(args: &[String], socket: &Path, session_env: Option<&str>, stdin: &mut dyn Read, out: &mut dyn Write) -> i32;
// doctor.rs
pub fn themes_section(themes: &Path, config: &Path, skill: &SkillState) -> Vec<String>;
```

- [ ] **Step 1: Failing tests**

```rust
// guide.rs
#[test]
fn the_skill_is_installed_once_refreshed_when_stale_and_never_over_a_stranger() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(skill_state(dir.path()), SkillState::Missing);
    let i = install_skill(dir.path()).unwrap();
    assert_eq!(i.path, dir.path().join("arkos-ide").join("SKILL.md"));
    assert!(!i.replaced);
    let text = std::fs::read_to_string(&i.path).unwrap();
    assert!(text.starts_with("---\nname: arkos-ide\ndescription: Customize Arkos IDE"), "{text}");
    assert!(text.contains(&format!("<!-- arkos-guide: {} -->\n", stamp())));
    assert!(text.ends_with(TEXT));
    assert_eq!(skill_state(dir.path()), SkillState::Current);
    let stale = text.replace(&stamp(), "0000000000000000");
    std::fs::write(&i.path, &stale).unwrap();
    assert_eq!(skill_state(dir.path()), SkillState::Outdated("0000000000000000".into()));
    assert!(install_skill(dir.path()).unwrap().replaced);
    std::fs::write(&i.path, "---\nname: other\n---\nhi").unwrap();
    assert_eq!(skill_state(dir.path()), SkillState::Foreign);
    let e = install_skill(dir.path()).unwrap_err();
    assert!(e.contains("SKILL.md"), "{e}");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(guide_cli_to(&[], dir.path(), dir.path(), &mut out, &mut err), 0);
    assert_eq!(String::from_utf8(out).unwrap(), TEXT);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(guide_cli_to(&["--path".into()], dir.path(), dir.path(), &mut out, &mut err), 0);
    let p = PathBuf::from(String::from_utf8(out).unwrap().trim());
    assert_eq!(std::fs::read_to_string(&p).unwrap(), TEXT);
    assert_eq!(guide_cli_to(&["--install".into()], dir.path(), dir.path(), &mut Vec::new(), &mut Vec::new()), 1, "the stranger stays");
    assert_eq!(skills_dir_from(Some(std::ffi::OsStr::new("/c")), Path::new("/h")), PathBuf::from("/c/skills"));
    assert_eq!(skills_dir_from(None, Path::new("/h")), PathBuf::from("/h/.claude/skills"));
    assert!(!TEXT.contains('\u{2014}'));
    assert!(TEXT.lines().count() > 100);
    for cmd in ["arkos theme template", "arkos theme contrast", "arkos theme preview", "arkos theme use", "arkos theme current", "arkos settings set", "arkos guide"] {
        assert!(TEXT.contains(cmd), "the guide names {cmd}");
    }
}

// hook_cli.rs: extend the session-start test so `out` holds CONTEXT_LINES exactly when the
// session id parses, and is empty for session-end, for a state notify, and when the env is unset.

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
    assert!(!lines.iter().any(|l| l.contains("names no theme: broken")), "an invalid file has its own line");
    assert!(lines.iter().any(|l| l == "agent guide skill: not installed"), "{lines:?}");
    std::fs::remove_file(themes.join("broken.toml")).unwrap();
    std::fs::write(&cfg, "[theme]\ndark = \"navy\"\n").unwrap();
    let lines = themes_section(&themes, &cfg, &SkillState::Outdated("abc".into()));
    assert_eq!(lines[0], format!("{} themes, all valid", theme::BUILTIN.len()));
    assert!(lines.iter().any(|l| l == "agent guide skill: older than this build (abc)"));
}

// doctor_panel.rs: probe_all with Probe.skill = Missing (claude_present true) gives the AgentGuide
// check in Fix with "Install"; Current gives Good with no button; Outdated("x") gives Fix "Update"
// with the text naming x; Foreign gives Broken; claude_present false gives Pinned with no button
// and does not count in needs_fix. The existing tests' id vectors gain AgentGuide at the end.
```

- [ ] **Step 2: Run to see them fail**: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- guide:: hook_cli:: doctor:: doctor_panel:: keymap:: command_palette::`.
- [ ] **Step 3: Write the guide and implement** per the Guide, Hook, Doctor and Palette
  constraints. The guide follows spec section 7's outline with the exact command lines and JSON
  shapes from Task 3; the worked example is Timo Purple (accent `#00ffff`, background `#330076`)
  with the contrast reasoning behind `crates/arkos/themes/timo-purple.toml`'s values. The Root's
  doctor Apply and the palette command both call `guide::install_skill(&guide::skills_dir())` on
  the background executor and show a notice with the path or the error.
- [ ] **Step 4: Run, check, commit**: `git commit -m "The agent guide: arkos guide, a Claude Code skill, the hook's pointer, a doctor check"`.

---

### Task 5: README, docs, and the gates

**Files:**
- Modify: `README.md`, `docs/themes.md` (the toolkit pointer, if Task 4 left it), `crates/*/src`
  (remove any "Used from Task" attribute left)

- [ ] **Step 1: README** per the README constraint, written against the code's actual output
  lines (read `agent_cli.rs`'s strings; run nothing).
- [ ] **Step 2: Gates**: `grep -rn "Used from Task" crates/` empty; no em-dash in README.md,
  docs/agent-guide.md, docs/themes.md or `crates/*/src/**/*.rs`; the warning check; the workspace
  suite. `git commit -m "README: themes for agents, the guide, the discovery layers"`.

---

## Deferred, for the user to rule on

- `arkos settings set keybindings.<action>` (off the list by the approved default).
- A configurable wait (`--wait <secs>`) for window answers; dropping a disconnecting window from
  a wait at once rather than at the cut.
- The doctor's "preview still active" check (a two-second round trip in a read-only command).
- The guide as a Codex-native file; a project-level skill install (`--install --project`);
  installing the skill into several Claude config directories at once (this machine uses
  `~/.claude2` and `~/.claude3` through aliases).
- `arkos theme preview` for one window only (`--pid`); today every window previews.
- A `--watch` mode for `arkos theme contrast`.
- `settings get` falling back to the effective (merged) config for keys absent from the file.

## Self-review notes

- Spec section 3: Task 2. Section 4: Task 3. Section 5: Tasks 1, 2, 3. Section 6: Task 3. Section
  7: Task 4. Section 8: Task 4. Section 9: Task 5. Section 11: Task 4 (the hook, the skill,
  `--help`).
- Rulings on the plan reviewer's questions (Cyrus to confirm at approval): the skill installs under
  `$CLAUDE_CONFIG_DIR/skills` when that is set, else `~/.claude/skills`; the stamp tracks the
  guide's content, not the release; a machine without a Claude config shows the row pinned, not as
  a fix; `arkos config`, `arkos settings` and `arkos guide` shadow folders of those names (use
  `arkos ./config`); a silent or too-old window prints its line and `use`/`settings set` still
  exit 0 when every answering window applied; the "preview still active" doctor check is deferred.
- Deviations from the spec's letter: the guide is compiled into the binary and `--path` writes a
  copy under Application Support instead of a bundle Resources file (no packaging step, always
  current); the tool client sends no Hello (protocol 0 keeps it out of every broadcast and out of
  `State.windows`); `Customized` carries the silent and too-old windows' pids so the client prints
  the spec's lines; the contrast JSON adds `what` and prints colours as hex (documented in the
  guide); the doctor panel gets only the skill row while the read-only report lists invalid files.
