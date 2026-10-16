# Arkos IDE: the agent theme interface (milestone 8b)

Date: 2026-10-16. Status: draft for Cyrus's review. Builds on the menubar, About and Settings
design (`2026-10-15-menubar-about-settings-design.md`): the theme schema in `docs/themes.md`,
`arkos theme list` and `validate`, `theme::import_file`, and the Settings window's theme pickers.
Written against main at the 0.3.3 release (the Help menu and the Keybindings window).

## 1. Why this exists

Cyrus wants other people's coding agents (Claude Code, Codex, whatever they run) to be able to
take Arkos and make it their own without reading the Rust. The brainstorm on 2026-10-15 settled the
first cut: themes only; everything local; the agent's interface is files plus the `arkos` command
line; agents may talk to a running Arkos over a short allow-list (apply or preview a theme, reload
the config, read the effective settings) and nothing else over that channel (no opening files, no
running commands, no session access); a guide ships with the app and is written for an agent; the
doctor reports themes that no longer validate. "Agent-first" means every step an agent takes can
be discovered, run, checked and repeated from a shell, with output it can parse and exit codes it
can branch on, and with the same result whether or not a window is open.

The test of the design: an agent given a screenshot and the one sentence "make Arkos look like
this" produces a theme, checks its contrast, shows it in the running window, and makes it the
default, without a human touching the IDE, and the human can undo it by hand in the Settings
window afterwards.

## 2. What ships

1. **The themes folder is live.** The active theme's file is watched like `config.toml`: a saved
   edit to `~/.config/arkos/themes/<active>.toml` applies within a second, in every window. A
   file that stops validating keeps the last good theme and shows the parse error in the banner,
   exactly as a bad `config.toml` does today.
2. **`arkos theme` grows into the agent's toolkit** (section 4): `template`, `contrast`, `add`,
   `use`, `preview`, `current`, next to the existing `list` and `validate`, all with `--json`.
3. **A live channel through the daemon** (section 5): the command line asks the daemon, the
   daemon relays to every connected window, each window answers, and the command prints one
   line per window. Four actions only: preview a theme, end a preview, reload the config, report
   the state.
4. **`arkos settings get|set`** (section 6): read the config as the file says it and as each
   window has it; write one of an allow-list of keys with `config::set_values`, keeping the
   user's comments, and confirm the reload.
5. **The agent guide** (section 7): `docs/agent-guide.md` in the repo and in the bundle, printed
   by `arkos guide`, written to be pasted into an agent's context.
6. **The doctor** gains a Themes check (section 8).
7. **README** documents the toolkit and the guide (section 9).

Out of scope for this cut, listed for the next: plugins (commands, keybindings, panels, hooks),
installing from a URL, agent-driven file opening or session control, a theme editor in the window,
a theme generator from an accent and a background (an agent can do that with this toolkit).

## 3. Files and the watcher

- `~/.config/arkos/themes/<name>.toml` is a theme; `name` inside must equal the file stem, bare
  (letters, digits, `-`, `_`), as today. A file shadows a built-in of the same name.
- `config.toml` `[theme] dark = "<name>"` and `light = "<name>"` choose; `[appearance] mode`
  picks which applies. Nothing changes here.
- **The watcher** (`config_watch`) gains the active theme's file. Each window watches
  `config.toml` and, resolved from it, the file of the theme it is showing (a built-in with no
  file is not watched; the moment a file of that name appears, the next poll picks it up because
  the config reload resolves the name again). A change applies through the same path a config
  reload takes (`switch_theme_to`): a valid file replaces the theme; an invalid one keeps the
  last good theme and shows "theme <name>: <error>" in the banner until the next valid save.
  The poll is the existing one-second fingerprint poll, extended to two paths.
- **Preview state** (section 5) never touches files. It lives in the window and ends at the
  next config or theme reload, at `arkos theme preview --end`, or when the window closes.

## 4. The `arkos theme` toolkit

Every subcommand prints human lines by default and JSON with `--json`; errors go to stderr; the
exit codes are 0 (done), 1 (the request was refused or failed: a bad file, an unknown name, a
window that reported an error), 2 (usage), 3 (a window was needed and none is running). The
`--json` shape is one object per command, documented in the guide; field names are stable from
0.4.0 on.

- `arkos theme list [--json]` (exists): every theme with its source; JSON
  `[{ "name", "source": "built-in" | "file" | "invalid", "error"?, "path"? }]`.
- `arkos theme validate <file> [--json]` (exists): "ok: <name>" or the error.
- `arkos theme template [<built-in>] [--json]`: prints a built-in's TOML to stdout (default
  `navy`), the starting point for a new theme; `--json` prints the schema instead: every table
  and key with a one-line meaning and the surface it must contrast with (the same facts as
  `docs/themes.md`, as data).
- `arkos theme contrast <file-or-name> [--json]`: the contrast report the built-in test enforces,
  one line per pair: `chrome.text on window_bg 13.1:1 ok`, `tree.ignored on window_bg 4.2:1 LOW
  (needs 4.5)`; the terminal text pair wants 11:1 and the chrome text 7:1, as the test does; exit
  1 when any pair is low, so an agent can loop until it passes. `--json` gives
  `{ "name", "pairs": [{ "fg", "bg", "ratio", "needs", "ok" }], "ok" }`.
- `arkos theme add <file> [--replace] [--json]`: `theme::import_file`; "added <name>" or
  "replaced <name>"; refuses a same-named file or built-in without `--replace` (exit 1, "exists:
  <name>; pass --replace").
- `arkos theme use <name-or-file> [--dark | --light] [--json]`: a `.toml` path is added first
  (as `add`, `--replace` applies); then `[theme] dark` or `light` is written with
  `config::set_values` (the key from the flag, else from `[appearance] mode` and, when that is
  `system`, from the running window's appearance, else dark); then, when windows are connected,
  the command waits for them to reload (section 5, the reload action) and prints one line per
  window: `window 4242 (/Users/x/proj): timo-purple applied`. With no window it prints
  `saved; no window running` and exits 0. A name that is neither built in nor a file is refused
  (exit 1) before anything is written.
- `arkos theme preview <name-or-file> [--json]` and `arkos theme preview --end`: live only
  (exit 3 without a window). The theme's TOML travels inline in the action, validated by the
  command first and by each window again, so an agent can preview a draft file without adding
  it. Every window shows it until the preview ends; nothing is written. One line per window:
  `window 4242 (/Users/x/proj): previewing draft` or the window's error.
- `arkos theme current [--json]`: the config's choice (`dark`, `light`, `mode`) from the file,
  then, per connected window, what it shows: `window 4242 (/Users/x/proj): timo-purple (dark,
  system is dark), preset comfortable, family Menlo` plus `preview: draft` when one is active
  and `config error: <text>` when the window is holding the last good config. Exit 0 with no
  window (the file part still prints).

Everything above that reads or writes files works without a daemon or a window, and the guide
says so, so an agent can prepare a theme before Arkos is even installed.

## 5. The live channel

The daemon knows nothing about themes. It relays.

- **Protocol** (`crates/protocol`, `PROTOCOL_VERSION` bumped): `Request::Customize { id: u64,
  action: CustomizeAction }` from a command-line client; `Event::Customize { id, action }`
  broadcast to every connected window; `Request::CustomizeDone { id, outcome: CustomizeOutcome }`
  from each window; `Response::Customized { outcomes: Vec<CustomizeOutcome> }` to the client.
  `CustomizeAction` is `PreviewTheme { toml: String } | EndPreview | ReloadConfig | ReportState`.
  `CustomizeOutcome` is `{ pid: u32, project: PathBuf, result: Result<Report, String> }` where
  `Report` is `{ theme: String, dark: bool, system_is_dark: bool, preset: String, family:
  String, preview: Option<String>, config_error: Option<String> }` (the state action fills it;
  the others fill `theme` and `preview` and leave the rest).
- **The daemon** answers `Customized` when every window connected at the moment of the request
  has answered, or after two seconds with the missing ones absent; the client prints `window
  <pid>: no answer` for those. A request with no connected window is answered at once with an
  empty list, which the command turns into exit 3 (or `saved; no window running` for `use`).
- **The window** handles `Event::Customize` on the UI thread through the Root: `PreviewTheme`
  parses the TOML (`theme::parse`), applies it with `set_theme` without touching `config`, and
  remembers the preview name; `EndPreview` re-resolves the theme from the config; `ReloadConfig`
  runs `reload_config` and reports the resolved theme (or the config error); `ReportState` reads.
  Each answers with `CustomizeDone`. A window never writes a file on this channel.
- **The command line** connects like `hook_cli::send_one` does, sends `Hello` as a tool client
  (the daemon already accepts short-lived clients for `arkos notify`), then `Customize`, waits
  for `Customized`, and prints. The socket is the user's own (`0600` in a `0700` folder), so the
  allow-list is a promise about what Arkos ships, not a sandbox: the guide tells agents which
  commands exist and that nothing else is supported.
- **Version skew**: a window on an older protocol is told to update by the existing Hello path;
  the command prints `window <pid>: too old for this command` for a window that answers nothing.

## 6. `arkos settings get` and `set`

- `arkos settings get [<key>] [--json]`: with no key, the config as the file has it (its text,
  comments included) and, per window, the effective values the report carries; with a key, that
  value alone (`arkos settings get theme.dark` prints `timo-purple`). Keys are dotted paths into
  `config.toml`.
- `arkos settings set <key> <value> [--json]`: the allow-list is `appearance.mode`, `theme.dark`,
  `theme.light`, `font.family`, `font.preset`, `font.presets.<name>.size`,
  `font.presets.<name>.line_height`, `update.auto`, `recovery.start_at_login`. The value is
  parsed to the key's type (a string, a number, `true`/`false`), validated against the same
  rules `config::load` applies (an unknown theme name, a preset with no table, a mode outside
  system/dark/light are refused before writing), written with `config::set_values` (comments
  kept), then confirmed through the reload action when windows are connected, one line per
  window. Any other key is refused with exit 2 and the list of allowed keys, so an agent cannot
  rewrite keybindings or recovery settings through this path; those stay a hand edit.
- `arkos config reload [--json]`: the reload action alone, for an agent that edited a file by
  hand and wants the windows' verdict now rather than within a second.

## 7. The agent guide

`docs/agent-guide.md`, copied into the bundle's Resources at build time and printed by `arkos
guide` (exit 0; `--path` prints the file's location instead). Written in the second person for an
agent, about 150 lines, no em-dashes:

1. What Arkos is and what this guide lets you change (themes and the settings keys above) and
   what it does not (nothing else; do not edit the app bundle; do not edit built-in theme files,
   which do not exist on disk anyway; keep the user's comments in `config.toml`, which the
   commands do for you).
2. Where things are: `~/.config/arkos/config.toml`, `~/.config/arkos/themes/`, the socket, the
   logs, and that `arkos` is the binary inside the app (`/Applications/Arkos IDE.app/Contents/
   MacOS/arkos`, also on PATH when Homebrew installed it).
3. The theme schema, table by table, the contrast rules, and the one-second live reload.
4. The commands with their exit codes and `--json` shapes.
5. The workflow: `arkos theme template > draft.toml`, edit, `arkos theme contrast draft.toml`
   until it passes, `arkos theme preview draft.toml`, `arkos theme use draft.toml`, then
   `arkos theme current` to confirm; and how to revert (`arkos theme use navy`).
6. A worked example: Timo Purple from an accent and a background, with the colour choices and
   why each passes contrast.
7. Rules: validate before you preview, preview before you use, never leave a preview running,
   never write `config.toml` by hand when a command exists, report the `arkos theme current`
   line to the user when you finish.

README's Config section tells a human how to hand the guide to their agent: `arkos guide` into a
file in the project (a Claude Code skill directory works), or the path from `arkos guide --path`.

## 8. The doctor

A Themes section: every file in the themes folder that fails to validate, with its error and the
fix ("run `arkos theme validate <file>`"); a configured `theme.dark` or `theme.light` that names
no theme; a preview still active in a window (the state action, when the daemon is up). Green
reads "N themes, all valid".

## 9. README

A "Themes for agents" subsection under Config: the toolkit in one table (command, what it does,
exit codes), the live reload of the active theme file, `arkos settings`, `arkos guide`, and the
Status rows: a theme file edit shows within a second; `arkos theme contrast` flags a low pair and
passes after the fix; `arkos theme preview` shows a draft in every open window and `--end`
restores; `arkos theme use` writes the config keeping comments and reports each window; `arkos
settings set font.preset compact` changes the terminals; the doctor lists an invalid theme file;
`arkos guide` prints the guide; an agent given only the guide produces a passing theme (the demo).

## 10. Tests

Headless, as always: the contrast report over every built-in and over a fixture with one low
pair; the `--json` shapes round-trip through `serde_json`; `use` with and without `--dark`,
with a file and with a name, writes the right key and refuses an unknown name before writing;
`settings set` refuses keys off the allow-list and bad values before writing and keeps comments;
the protocol round-trips the new messages; a fake daemon test (as `hook_cli` has) for the
command's send-and-wait with two windows and with a window that never answers (the two-second
cut); the daemon's relay with a fake window client; the window's `PreviewTheme` and `EndPreview`
through the Root's pure helpers (`preview_theme(text) -> Result<Theme, String>`, the resolution
after `EndPreview`); the watcher's second path; the doctor's Themes lines from a fixture folder.
Windows and sockets on the real machine are hand checks in the README.

## 11. Open questions for Cyrus

1. The command names: `arkos theme use` (chosen) against `apply` or `set`; `arkos guide` against
   `arkos agent-guide`.
2. Whether `arkos settings set` should also accept `keybindings.<action>` (the Keybindings window
   makes that visible now; it is off the list in this draft because a bad binding breaks a key
   until the next edit).
3. The two-second wait for window answers: fine, or configurable.
4. The protocol bump forces the daemon and every window onto the new build at once (the existing
   update path handles it); confirm that is acceptable for a 0.4.0.
