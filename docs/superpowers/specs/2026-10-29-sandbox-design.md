# `arkos sandbox`: a second Arkos to try things in

Status: shape agreed with Cyrus in chat, 2026-10-29 ("this looks good to me too"; the name is
`sandbox`). Built before the matrix effect (`2026-10-29-matrix-effect-design.md`), whose first
look it is for.

## Intent

A supported way to run a second, separate Arkos beside the one in use, so a person or their
coding agent can try a theme, an effect or a settings change and look at it without touching
their real setup. Today this takes a hand-made script that sets two environment variables, lives
in a scratch folder, and still rewrites the user's Claude Code colours. Cyrus's words: "an
officially-supported way for others to test their own graphics and theme changes. It should be
discoverable in the skill too so people know there's an ability to preview."

Run from a dev build, the same command is the dev window Cyrus uses to iterate on effects.

## Goals

1. `arkos sandbox [<folder>]` opens a window of a separate Arkos instance on the folder (default:
   the current one): its own config, its own daemon and sessions, its own logs.
2. `arkos sandbox <command> ...` runs any `arkos` command aimed at that instance: `theme use`,
   `effect use`, `settings set`, `open`, `panes`, `effect current`, `theme preview`, and so on.
3. A shell inside a sandbox window is already aimed at the sandbox: `arkos theme use ...` there
   needs no prefix, and so an agent working inside it follows the ordinary guide.
4. The sandbox never changes anything of the user's: not their config, not their Claude Code
   colours (`~/.claude/themes/arkos.json`), not their login items, not their updates, and it sends
   no telemetry.
5. `arkos sandbox status` and `arkos sandbox stop`, and `arkos sandbox reset` to start clean.
6. The guide (and so the `arkos-ide` skill) tells agents it exists and when to use it.

## Non-goals

- No second app bundle, icon or name in the Dock: the sandbox is the same binary run again. (The
  screenshot shoot keeps its own signed "Arkos Shoot" bundle for Screen Recording.)
- No sharing of sessions, workspaces or recovery between the sandbox and the real instance.
- No sandboxing in the macOS security sense: the sandbox's shells are ordinary shells with the
  user's rights. The word means "a place to try things".
- No change to the shoot (`crates/shoot`), which keeps setting the environment itself.

## Anti-goals (a failure even if it works)

- Anything in the sandbox reaching the real instance: a `theme use` that lands in
  `~/.config/arkos`, a daemon that answers on the real socket, a session recovered at login.
- Other programs in a sandbox shell seeing a changed `XDG_CONFIG_HOME` (today's script sets it,
  which would move `gh`, `git` and others off their config). The sandbox sets only Arkos's own
  variables.
- A user unable to tell the two windows apart.

## Constraints

- The rules of `CLAUDE.md` and `docs/development.md`: `nice -n 15`, no `--release`, one cargo at a
  time, tests before code, headless tests named as sentences, no em-dashes in doc comments.
- The environment variables already honoured stay as they are: `ARKOS_APP_SUPPORT` (the state
  folder; the shoot uses it) and `XDG_CONFIG_HOME` (the config folder, as the XDG spec says).
- Branch `matrix`, worktree `~/Projects/ide-arkos/.claude/worktrees/matrix`, as the matrix work;
  the sandbox lands first, in its own commits.

## How it works

### The instance

A sandbox instance is Arkos with three environment variables set:

| Variable | Value | Effect |
| --- | --- | --- |
| `ARKOS_CONFIG_DIR` | `<sandbox folder>/config` | New. Arkos's config folder (`config.toml`, `themes/`, `effects/`), taking precedence over `XDG_CONFIG_HOME` and `~/.config/arkos`. Only Arkos reads it, so the sandbox's shells keep every other program's config where it is. |
| `ARKOS_APP_SUPPORT` | `<sandbox folder>/state` | Existing. The daemon's socket and logs, the sessions, the workspaces, the window log. |
| `ARKOS_SANDBOX` | `1` | New. Marks the instance as a sandbox: the rules under "What a sandbox never does" apply, and the title bar says so. |

`ARKOS_TELEMETRY=off` is set too (an existing variable: telemetry's `mode_with` reads it), so the
sandbox sends nothing and records nothing.

The sandbox folder is one per build, so a dev build's sandbox and the installed app's never share
a daemon (a window refuses a daemon of another build): `~/Library/Caches/arkos-sandbox` for the
installed app (the release channel, as the telemetry crate already reads it) and
`~/Library/Caches/arkos-sandbox-dev` for a source build. `status`, `stop` and `reset` act on the
running build's own sandbox.

The window removes from the environment it passes on, as the screenshot shoot does: `ARKOS_SESSION_ID`
and `ARKOS_RELAUNCH`, and every variable starting with `CLAUDE` except `CLAUDE_CODE_OAUTH_TOKEN`
(`CLAUDE_CONFIG_DIR` among them), so a `claude` started inside the sandbox does not take itself for
a child of the agent that opened the sandbox.

The variables are inherited down the chain as they are today: the window starts the daemon
(`client.rs`) with its own environment, the daemon starts each shell with its environment plus
`ARKOS_SESSION_ID`, `ARKOS_BIN` and `PATH`, so `arkos` inside a sandbox shell is aimed at the
sandbox with no prefix (goal 3). `ARKOS_CONFIG_DIR` is read in `crates/telemetry/src/paths.rs`
(`config_dir`), which every crate already goes through.

### `arkos sandbox [<folder>]`

1. Resolves the folder (default the current directory; a missing folder exits 2 as `arkos
   <folder>` does).
2. On the first run (no `config/config.toml` in the sandbox folder), copies the user's real
   config into it: `config.toml`, `themes/` and `effects/` from the real config folder (resolved
   with the sandbox variables unset), creating empty folders when the user has none. The copy is
   theirs to change; the real files are never written.
3. Starts the window: the current executable (`std::env::current_exe()`, so a dev build runs a
   dev sandbox and the installed app an installed one) with the folder as its argument, the four
   variables set and the removals above applied, in its own session (`setsid`, as `launch.rs`
   relaunches), stdout and stderr to `state/window.log`. The window starts the sandbox daemon on
   its own when none answers, as it does today, passing `--check-every-secs 0` so the daemon never
   polls for updates (the daemon reads no `ARKOS_SANDBOX`; the shoot does the same).
4. Prints `sandbox: opened <folder> (config <config dir>, state <state dir>)` and exits 0. With
   `--json`: `{ "folder", "config", "state", "pid" }`.

A second `arkos sandbox <folder>` while the sandbox runs opens another window of the same
instance, as a second `arkos <folder>` does.

### `arkos sandbox <command> ...`

Any first word that is one of the ordinary commands (`open`, `video`, `panes`, `theme`, `effect`,
`settings`, `config`, `guide`, `doctor`, `telemetry`, `hooks`, `notify`) runs the current
executable with those arguments and the sandbox variables set, forwarding its streams and exit
code. Nothing is copied first: a command before any sandbox window has run works on the sandbox's
config folder as the ordinary command works with no window (an empty config folder gives the
defaults, as a fresh Arkos does), and the next window start finds a `config.toml` and copies
nothing. `arkos sandbox theme use matrix` therefore writes the sandbox's `config.toml`, never the
user's.

### `arkos sandbox status`

Prints, one line each: the config and state folders; whether the sandbox daemon answers (`daemon:
running` or `daemon: not running`); each window as `arkos panes --all` prints it (`window {pid}
({project})`), none as `windows: none`; and `sends nothing; Claude Code colours untouched`. Exit 0
either way. `--json`: `{ "config", "state", "daemon": bool, "windows": [{ "pid", "project" }] }`.

### `arkos sandbox stop`

Ends the sandbox: every window (found by `panes --all --json` on the sandbox socket, `SIGTERM` to
each pid), then the daemon (the process whose command line is `arkosd --socket <the sandbox
socket>`, found with `pgrep -f`; `SIGTERM`, then `SIGKILL` after three seconds), in that order,
since a window restarts a daemon that goes away under it. Prints `sandbox: stopped {n} windows and
the daemon` or `sandbox: nothing running`. The folders stay. Exit 0; 1 when a process would not
end.

### `arkos sandbox reset`

Refuses while the sandbox runs (`sandbox: still running; run arkos sandbox stop first`, exit 1).
Otherwise removes `~/Library/Caches/arkos-sandbox` whole and prints `sandbox: reset; the next start
copies your config again`. Nothing else is asked: the folder is the sandbox's own.

### What a sandbox never does (`ARKOS_SANDBOX=1`)

- Claude Code colours: the window skips both writes of `~/.claude/themes/arkos.json` and the
  settings switch to `custom:arkos` (`main.rs` around lines 826 and 929). The doctor's "Claude
  Code: follows the Arkos theme" row reads `not in a sandbox`.
- Start at login: `recovery.start_at_login` is treated as `off`; the launch agent is never
  written, and the Settings toggle and palette item say `not in a sandbox` and do nothing.
- Updates: the daemon is started with `--check-every-secs 0`, so there is no daily check;
  Settings' auto-update switch and its "Update now" pill, and the palette's update item, read
  `not in a sandbox` and do nothing; `update.auto` is treated as `false`.
- Claude Code's hooks and skill: `arkos hooks install`, `arkos guide --install` and the
  SessionStart hook's skill write (`hook_cli`), which write `~/.claude/settings.json` and
  `~/.claude/skills`, are skipped in a sandbox with `not in a sandbox` (a dev build's sandbox
  would otherwise point the user's hooks at the dev binary); the doctor's "Claude Code title hook"
  and "Agent guide for Claude Code" rows and the palette's install items say the same and do
  nothing. `arkos notify` (the hooks' events) still runs, so the sandbox's tabs show their
  sessions' states.
- Telemetry: off through `ARKOS_TELEMETRY=off`; `arkos telemetry status` in the sandbox says so
  with the reason `sandbox`.
- Recovery: sessions are the sandbox's own and are never recovered by the real instance; the
  sandbox daemon recovers its own on its next start as any daemon does.

### Telling the windows apart

The project window's title bar shows `sandbox` in the dim text colour after the project's name
while `ARKOS_SANDBOX` is set; the window's title (the constant "Arkos IDE" today, what the Dock
and Mission Control show) becomes "Arkos IDE (sandbox)". The chooser (the launch window) says
`Sandbox` in its title bar the same way.

### Discoverability

- `arkos --help` gains the line `arkos sandbox [<folder> | <command> ...]   a second Arkos with
  its own config, to try a theme or effect in`.
- The guide (`docs/agent-guide.md`, and so the skill) gains a section after "The theme
  workflow": **Trying a theme or effect without touching the user's setup**, saying what the
  sandbox is, the four commands, that a shell inside it is already aimed at it, and when to use
  it: whenever the user wants to see a change before it lands, or the agent is working on a
  theme or an effect and wants to look while the user keeps working in their own window. The
  theme workflow (section 6) and the effect steps (section 8) each gain one line pointing at it
  (`arkos sandbox theme use draft.toml` beside step 4; `arkos sandbox effect use <name>` beside
  step 5). "Where things are" (section 2) notes `ARKOS_CONFIG_DIR`.
- `docs/guide.md` (the user's guide) gets a paragraph under the themes section, and
  `docs/development.md` gains, in its Build and run section, that a dev window is
  `target/debug/arkos sandbox <folder>` (its own folder, `arkos-sandbox-dev`, so it never meets
  the installed app's sandbox or Cyrus's real config), and adds the hand checks below to Status.
  The matrix design's Constraints point at the command in place of the env-var script.

## Tests (written first, headless)

- `paths`: `ARKOS_CONFIG_DIR` wins over `XDG_CONFIG_HOME`, which wins over `~/.config/arkos`; an
  empty value is unset.
- `sandbox_dirs(home, channel)` gives `arkos-sandbox` for the release channel and
  `arkos-sandbox-dev` otherwise, with the two folders; `sandbox_env(dirs)` the four variables to
  set and the names to remove (`ARKOS_SESSION_ID`, `ARKOS_RELAUNCH`, every `CLAUDE*` but
  `CLAUDE_CODE_OAUTH_TOKEN`), applied to a `Command`.
- First start copies `config.toml`, `themes/` and `effects/` from a given real config folder into
  an empty sandbox folder, makes the folders when the real ones are missing, and copies nothing
  when `config.toml` is already there (a changed sandbox config is kept).
- The command dispatch: `arkos sandbox theme use x` runs the executable with `["theme", "use",
  "x"]` and the sandbox environment; `arkos sandbox <folder>` runs it with the folder; a first
  word that is neither a command nor a folder exits 2 with the usage; `status`, `stop` and
  `reset` are the sandbox's own. Tested through an injected runner, as the shoot and the agent
  CLI test theirs.
- `stop` orders the windows before the daemon and reports the counts; `reset` refuses while the
  daemon answers.
- With `ARKOS_SANDBOX` set: `claude_theme` is not written (the follow and the ensure paths are
  skipped), `launch_agent::desired` is `None`, the daemon is started with `--check-every-secs 0`,
  `hooks install`, `guide --install` and the SessionStart skill write exit with `not in a
  sandbox` and write nothing, the title bar carries `sandbox`.
- The guide and `USAGE` name `arkos sandbox`.

## Hand checks (Status list)

- `arkos sandbox ~/Projects/x` opens a second window titled "Arkos IDE (sandbox)" with `sandbox`
  in its title bar; the first window is unchanged; `arkos sandbox status` lists it
- `arkos sandbox theme use timo-purple` changes only the sandbox window;
  `~/.config/arkos/config.toml` and `~/.claude/themes/arkos.json` are byte for byte as before
- inside a sandbox shell, `arkos theme current` names the sandbox's theme and `echo
  $XDG_CONFIG_HOME` is as it was outside; `gh auth status` still works there
- `arkos sandbox stop` closes the sandbox window and its daemon and leaves the real instance
  running; `arkos sandbox reset` then `arkos sandbox <folder>` opens on a fresh copy of the config
- Settings in the sandbox: Start at login and the update switch read `not in a sandbox`; the
  doctor's hook and guide rows too; `~/.claude/settings.json` is byte for byte as before

## Left to the builder

File and module names (a `sandbox.rs` beside `agent_cli.rs` is the natural place), how the
title-bar label is laid out, the exact JSON field order, and the test names. Calls I made on
Cyrus's behalf, open to his correction: `ARKOS_CONFIG_DIR` as a new variable instead of
`XDG_CONFIG_HOME` (so the sandbox's shells keep every other tool's config), the Caches folder for
the sandbox (disposable by nature; `reset` deletes it), `stop` by signal (there is no quit request
in the protocol), the first-start copy of the user's config (so the sandbox looks like their
Arkos until they change it), and telemetry off rather than the user's setting.

## Rulings from the builder check (2026-10-29, my calls)

1. One sandbox per build: `arkos-sandbox` for the installed app, `arkos-sandbox-dev` for a source
   build, keyed on the telemetry crate's channel. A dev sandbox and an installed one never share a
   daemon, and `stop` or `reset` from one never touches the other.
2. The sandbox window strips every `CLAUDE*` variable but `CLAUDE_CODE_OAUTH_TOKEN`, as the shoot
   does, so a Claude Code started inside it is not a child of the agent that opened it.
3. `hooks install`, `guide --install` and the SessionStart skill write are refused in a sandbox
   (`not in a sandbox`), as the Claude theme write is: they write the user's `~/.claude`. Events
   (`arkos notify`) still flow. The daemon's update poll is turned off with `--check-every-secs 0`;
   the window title becomes "Arkos IDE (sandbox)"; Settings' update switch and "Update now" carry
   the `not in a sandbox` text.
