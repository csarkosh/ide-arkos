# What Arkos sends

Arkos IDE can send three things, and each has its own consent:

- **The update check.** Once a day, the background daemon says that a copy of Arkos with this
  version, macOS version and processor exists. No consent is asked: an update check is what a
  self-updating program sends.
- **Session reports.** What a window did while it ran: which kinds of tab were opened and for
  how long, which agents were started, which markdown features were used, how it ended. They
  are recorded on your Mac by default and sent only after you turn usage reports on.
- **Crash reports.** Where a panic happened, in which build, with a stack of offsets. Each one is
  sent only after a dialog that shows its exact bytes, unless you chose to always send them.

Nothing in them identifies you or your Mac. This page is true of the code: its tables below are
made from the files Arkos itself uses, and a test fails when the two differ. The same
page is in the app, under Help > What Arkos Sends, and on the website.

## 1. What is sent and why

Every key of every report is listed here, with an example value and the reason it is there.
There are no other keys: the collector refuses a report with a key it does not know, and Arkos
builds each report with the same code the collector checks it with.

<!-- tables:start -->

### Every report: the envelope

The check, the session report and the crash report all start with these keys. They describe the copy of Arkos, never the person or the Mac.

| Key | Example | Why it is there |
|---|---|---|
| `v` | `2` | The version of the report format, so a later format can be read beside this one. |
| `app` | `arkos` | The program the report is about; the collector refuses one it does not know. |
| `ver` | `0.5.0` | The Arkos version, to see which releases are in use and which of them crash. |
| `bin` | `app` | The part of Arkos that wrote it: `app`, the window, or `daemon`, the background process. |
| `os` | `macos` | The operating system. Arkos runs on macOS only, so it is always `macos`. |
| `osv` | `15` | The major macOS version, to know which versions to keep supporting. The minor version is not sent. |
| `arch` | `arm64` | The processor type, `arm64` for Apple silicon or `x86_64` for Intel, to know which builds matter. |
| `ch` | `release` | How this copy was built: `release` for the published app. A copy built from source says `source` and sends nothing. The maintainer's own Macs say `dogfood`, so his use is counted apart from yours. |
| `k` | `session` | The kind of report: `check`, `session` or `crash`. |

### The update check

Sent by the daemon once a day at most, as the query of the first update check of the day. It needs no consent: an update check is what a self-updating program sends.

| Key | Example | Why it is there |
|---|---|---|
| `rd` | `1` | Days since this copy's last check through this path, rounded down, or `-1` the first time. It counts installs in use and how often they return, without any id. |

### The session report

One report per journal, which is one run of a window, recorded only while usage reports are `local` or `on` and sent only when they are `on`. A run that reconnects to the daemon, or in which you turn usage reports on, starts a new report under a new id.

| Key | Example | Why it is there |
|---|---|---|
| `sid` | `9f2c4e1a7b3d5f60a1c2e3f4b5d6e7f8` | A random number made for one report and never used again. It ties that report's events together. |
| `len` | `5400` | How long the report covers, in seconds. |
| `end` | `quit` | How the report ended: `quit` when the window or the launch chooser quit (a macOS logout or restart included) or relaunched for an update, `lost` when its connection to the daemon dropped without a quit (a crash or a force quit), or `unknown` when it was sealed later from a journal left behind, as after the daemon restarted or was upgraded. The share of `lost` reports estimates the crash rate. |
| `iw` | `20370` | The week the first session was recorded on this Mac, as days since 1 January 1970 rounded down to a multiple of 7. It shows how long people keep using Arkos; everyone who started that week sends the same number. |
| `rd` | `2` | Days since the last session report from this copy, or `-1` for the first. It shows how often Arkos is used, without an id. |
| `focus` | `{"terminal": 3100, "viewer": 900}` | Seconds the window was in front during the report, per kind of tab and for Settings, to see which parts of Arkos are used. |
| `ev` | `[{"n": "launch", "t": 0, "d": {"how": "normal"}}]` | The report's events in order, at most 200. Each one is in the events table below. |
| `ev_dropped` | `0` | How many events came after the 200th, counted and not kept. |
| `ev_rejected` | `0` | How many events the allowlist refused, counted so a mistake in Arkos shows up. The events themselves are never written. |

### An event

Each entry of `ev` has these keys.

| Key | Example | Why it is there |
|---|---|---|
| `n` | `pane_open` | The event's name, one of the events table below. |
| `t` | `42` | Seconds since the report started. |
| `d` | `{"kind": "terminal", "how": "key"}` | The event's details: only the keys and values the events table allows, never a name, a path or anything typed. |

### The events

An event is one of these names, with only these keys, each taking only the values listed. Anything else is dropped before it is written and counted in `ev_rejected`.

| Event | Keys and allowed values | When |
|---|---|---|
| `launch` | `how`: `normal`, `relaunch`, `updated`, `recovered` | The window started, and how. `recovered` is reserved, not sent yet: it would mark a start after the last window died. |
| `window_open` | none | A project window opened. |
| `window_close` | none | A project window closed. |
| `pane_open` | `kind`: `terminal`, `viewer`, `browser`, `video`; `how`: `palette`, `key`, `click`, `explorer`, `link`, `restore`, `agent`, `poster` | A tab was opened, and how. |
| `pane_close` | `kind`: `terminal`, `viewer`, `browser`, `video` | A tab was closed. |
| `view` | `format`: `markdown`, `code`, `plain`, `image`, `video`, `binary`, `too_large`; `mode`: `rendered`, `raw` | The viewer showed a file, by its kind, never its name, or a video tab showed its first frame (`video`). `image` is reserved, not sent yet. |
| `md` | `feature`: `table_edit`, `mermaid`, `details`, `checklist`, `remote_image`, `video`, `inline_diff` | A markdown feature was used for the first time in this report, once per feature. |
| `agent_launch` | `agent`: `claude`, `other`; `how`: `new`, `resume` | A coding agent started in a terminal. |
| `agent_state` | `agent`: `claude`, `other`; `state`: `waiting` | An agent first asked for input, once per terminal. |
| `palette` | `command`: `focus_pane`, `focus_dir`, `split`, `toggle_maximize`, `move_tab_to_pane`, `cycle_tab`, `mru_tab`, `close_tab`, `reopen_closed_tab`, `close_all_in_pane`, `move_tab_within`, `new_terminal`, `clear_to_start`, `font_delta`, `about`, `open_settings`, `keybindings`, `documentation`, `open_config`, `text_preset`, `reload_config`, `extract_themes`, `command_palette`, `set_theme`, `find`, `copy_as_shown`, `update_now`, `check_for_updates`, `notifications`, `doctor`, `retry_update`, `start_at_login`, `install_hooks`, `install_agent_guide`, `toggle_sidebar`, `focus_file_tree`, `show_ignored_files`, `focus_worktrees`, `toggle_inline_diff`, `toggle_markdown_view`, `save_file`, `bold`, `italic`, `fold_all`, `unfold_all`, `toggle_presentation`, `open_folder`, `new_window`, `new_browser_tab`, `open_file_in_pane`, `video_play_pause`, `video_back`, `video_forward`, `video_back_fine`, `video_forward_fine`, `video_step_back`, `video_step_forward`, `video_start`, `video_loop`, `video_mute`, `video_reload`, `page_back`, `page_forward`, `reload_page`, `focus_address`, `open_in_browser`, `zoom_reset` | A command ran from the command palette. |
| `settings` | `key`: `font.family`, `font.preset`, `font.presets`, `terminal.scrollback`, `terminal.shell`, `terminal.nice`, `terminal.option_as_meta`, `keybindings`, `sidebar.width`, `sidebar.collapsed`, `viewer.highlight_limit_bytes`, `viewer.wide_tables`, `viewer.image_destination`, `viewer.page_width`, `browser.url`, `browser.app_mode`, `daemon.watchdog_secs`, `daemon.replay_bytes`, `theme.dark`, `theme.light`, `effect.name`, `effect.intensity`, `effect.run_unfocused`, `appearance.mode`, `recovery.start_at_login`, `update.auto`, `update.check_every_secs`, `update.idle_after_secs`, `telemetry.usage`, `telemetry.crashes` | A setting was changed: the key, never its value. |
| `theme` | `builtin`: `true`, `false` | A theme was chosen. A built-in theme is not named, and a theme of your own is just `false`. |
| `update` | `step`: `staged`, `applied`, `failed`, `rolled_back` | The updater moved a step. |
| `notice` | `kind`: `update_ready`, `updated`, `daemon`, `config_error`, `theme_error`, `theme_retired`, `claude_colours` | The bell got a notice. |
| `error` | `kind`: `daemon_unreachable`, `daemon_restarted`, `session_lost`, `mermaid_failed`, `save_failed`, `video_unplayable` | Something went wrong where you would have seen it. |
| `agent_pane` | `command`: `open`, `open_web`, `status`, `play`, `pause`, `seek`, `step`, `loop`, `reload`, `close`, `mute`, `panes`; `ok`: `true`, `false` | An agent's `arkos open`, `arkos video` or `arkos panes` command ran, which one and whether it succeeded. |
| `web_load` | `outcome`: `ready`, `failed`; `scheme`: `http`, `https`, `file` | A web tab's page loaded, or failed to: the scheme of its address (`http`, `https` or `file`), never the address, the page or its title. |
| `help` | `what`: `keybindings`, `documentation`, `about`, `telemetry` | A Help surface opened. |

The keys of `focus` are these surfaces only: `terminal`, `viewer`, `browser`, `video`, `settings`.

### The crash report

Written when Arkos panics, and sent only after you agree: in the dialog for that crash, or for every crash once you choose to always send them.

| Key | Example | Why it is there |
|---|---|---|
| `inc` | `b7e14c09d2a35f8e6c1b0a9d8e7f6a5b` | A random number for this one crash, shown under Reports in Settings so you can quote it in an issue. |
| `ck` | `panic` | The kind of crash. Arkos reports panics, Rust's own failures. |
| `commit` | `4f2a9c1` | The source commit of the build, to find the exact code that failed. |
| `pk` | `other` | The name of one of Arkos's own failure points when the panic came from one, else `other`. |
| `mh` | `5d41402abc4b2a76b9719d911017c592ae2d1b7a8e3f6c0d9b8a7f6e5d4c3b2a` | For an `other` panic, the SHA-256 hash of the message's first line with every path and number taken out. Crashes with one cause share it; the message is never sent. |
| `file` | `/src/crates/arkos/src/md_view.rs` | The source file the panic came from, as a path inside the Arkos source (`/src/`), a dependency (`/cargo/`) or Rust itself (`/rustc/`); any other path is cut to its file name. |
| `line` | `812` | The line in that file. |
| `frames` | `[[0, 1843200], [0, 1790466]]` | The call stack, as offsets into the Arkos binary. The maintainer turns them into function names with the published debug symbols. No memory is sent. |
| `images` | `[["arkos", "6c3f0e2a9b8d4c7e1f5a0b9c8d7e6f5a", 48234496]]` | The Arkos binary the offsets belong to: its name, its build id and its size. No other program or library is listed. |
| `sig` | `11` | The signal number of a crash by signal. Arkos does not send it: it reports panics only. |
| `up` | `<10m` | How long the process had been running, in one of five buckets: `<1m`, `<10m`, `<1h`, `<1d`, `>=1d`. |
| `mem` | `normal` | How short of memory the Mac was. Arkos does not send it. |

<!-- tables:end -->

## 2. What is never collected

- Your name, username, email, or anything about your account.
- The Mac's name, serial number, hardware model or any other id of the machine.
- Any path, file name, folder name or project name. The one path a crash report carries is a
  path inside the Arkos source, a dependency's source or Rust's own, never a folder of yours.
- What a terminal shows, what you type anywhere, what an agent says, or a prompt.
- The contents of a file, a document, a setting's value or a theme's name.
- Environment variables.
- Your IP address. The collector does not write it (see section 5).
- A persistent id. A session id is made for one report and never used again; a crash id names
  one crash.

A source build, which is every copy built from the code rather than downloaded as a release,
sends nothing to the maintainer: its update checks go straight to GitHub.

## 3. The defaults, and how to change them

| What | Default | Choices |
|---|---|---|
| The update check | sent once a day | no switch: it is part of updating |
| Usage reports | `local`: recorded on this Mac, never sent | `off`, `local`, `on` |
| Crash reports | `ask`: the dialog asks before each one is sent | `never`, `ask`, `always` |

Arkos asks once. On a day after the one you installed it, a release build shows a strip under
the tab bar: "Help improve Arkos: share anonymous usage reports. Nothing identifies you or your
files." Its buttons are **Turn on**, **Not now** and **What is sent**, which opens this page.
Either of the first two answers it for good. After that the switch is in Settings.

Turning usage reports on deletes the session reports recorded before, so nothing recorded before
your consent is sent.

The ways to change it, all equal:

- **Settings**, in the Privacy section: Usage reports and Crash reports.
- **The command line**: `arkos telemetry on`, `arkos telemetry off`, `arkos telemetry local`,
  `arkos telemetry crashes never|ask|always`, and `arkos telemetry status` to see the mode in
  force and the reason for it.
- **The config file**, `~/.config/arkos/config.toml` (or under `$XDG_CONFIG_HOME`):

  ```toml
  [telemetry]
  usage = "local"     # "off" | "local" | "on"
  crashes = "ask"     # "never" | "ask" | "always"
  ```

  `arkos settings set telemetry.usage off` writes the same key. A config file the window cannot
  read stops it recording for that run, never falling back to the defaults; the daemon sends only
  what the `[telemetry]` table, as written, allows.

Environment variables take precedence over the settings:

- `DO_NOT_TRACK` set to anything but empty, `0` or `false`: nothing is recorded and nothing is
  sent, crash reports included.
- `ARKOS_TELEMETRY=off`: the same.
- `ARKOS_TELEMETRY=log`: Arkos records as your settings say and sends nothing. Every 15 minutes
  the daemon prints each report waiting in the queue to its log instead (section 4). The
  invitation is not shown.
- `CI` set to anything but empty: Arkos records as your settings say and sends nothing.
- `arkos` run from a script, with no terminal, never sends and never asks.

An app launched from the Dock or the Finder, and the daemon it starts, do not see variables set
in a shell's rc file such as `~/.zshrc`. The daemon, which does the sending, reads the variables
it was started with and no later ones. The config keys are the reliable switch: the daemon reads
them again before every upload, so a change applies without a restart.

None of these switches stops the update check. It carries only the envelope and `rd`.

## 4. How to see a report before it is sent

Everything Arkos would send is a file on your Mac first, in
`~/Library/Application Support/arkos/telemetry/`:

- `journal/` holds each running window's session, one line per event, as it is recorded. A run
  that reconnects to the daemon, or in which you turn usage reports on, starts a new report under
  a new id.
- `queue/` holds the reports waiting to leave. Under `local`, every session waits here and is
  never sent; reports older than seven days are deleted.
- `last-session.json` is the session sealed last, kept after it was sent.
- `incidents/` holds crash reports waiting for the dialog.
- `receipts` has one line per crash report you have decided on: its id, the date, and whether it
  was queued or declined. Settings > Privacy > Reports lists each report by date with its id as
  `waiting` (not decided yet), `queued`, `declined`, or `left the queue`: sent, or refused by the
  collector or older than seven days and deleted, which Arkos cannot tell apart.
- `usage-seen` holds the usage setting the daemon last saw, one word: `off`, `local` or `on`. It
  lets the daemon notice a turn-on made while it was not running.

To read them:

- `arkos telemetry view` shows the queue; `arkos telemetry view --last` prints the session sealed
  last; `arkos telemetry view <path>` prints any one report. The bytes printed are the bytes sent.
- Settings > Privacy > **Last report** opens the last session in a viewer tab.
- The crash dialog's **Show report** shows the exact bytes before you choose.
- With `ARKOS_TELEMETRY=log` set for the daemon, each report it would send is printed to
  `~/Library/Application Support/arkos/arkosd.log` as a line starting with
  `visits-app: would send`, and stays in the queue.

## 5. How it travels, where it is kept, and for how long

Only the daemon sends. It uses the system's `curl`, over HTTPS only, to `apps.csarko.sh`, the
maintainer's own address, with the user agent `arkos/<version>`. Firebase Hosting serves that
address and passes each request on to the collector. Reports go up every 15 minutes
with a 10 second limit; the update check has its own 10 second limit and falls back to GitHub
straight away if it fails, so a collector outage never delays an update. A report that fails is
tried again after 15 minutes, an hour, four hours, then daily, and is deleted after seven days.
The day's other update checks, the update manifest's signature and every update download go
straight to GitHub and carry only the user agent.

The collector is lib-visits's open source `visits` server. It runs on Google Cloud Run in the
United States (`us-west1`), in the maintainer's own Google Cloud project. It writes each report
and each check it accepts as one row, with only their keys and the time they arrived. The rows
are kept in a Cloud Logging bucket in that project, whose location is Google's `global`, and read
through BigQuery. Rows are kept for two years (730 days), so a year can be compared with the one
before.

The collector sees your IP address to answer the request and to limit how often one address may
post, and to tell whether it is one of the maintainer's own, whose rows it marks `own` so his
use is counted apart. It never writes the address, and Google Cloud keeps no request-log entry
for a report or a check, accepted or refused, so no address is kept.

## 6. Who reads it, and what is published

Two can read the raw rows: the maintainer, and the service account of his own admin service,
which runs in the same Google Cloud project. Crash offsets are turned into function names
with the debug symbols published beside each release (`Arkos-IDE-<version>-dSYM.zip`), which say
nothing the source code does not.

Nothing is published yet. When it is, it will be totals by Arkos version, macOS version,
processor and channel, with any group of fewer than ten merged into "other". Raw rows are never
published.

These numbers inform decisions; they do not make them. Low use alone never removes a feature.

## 7. The files that decide

This page describes them; if the two ever disagree, the files are right and the page is a bug.

- `telemetry/events.toml`: every event, key and value a session report may carry. Anything not
  in it is dropped.
- `crates/telemetry/`: the envelope, the config keys and the mode they give, the panic hook that
  writes crash reports, and this page's tables.
- `crates/arkos/src/telemetry.rs`: where the app records each event.
- `crates/arkosd/src/telemetry/`: sealing, the upload, and the routing of the update check.
- lib-visits's `crates/visits-core/src/app.rs`: the report format, which the client and the
  collector both check against, and its `crates/visits-app/` and `crates/visits/`: the client
  code and the collector.

## 8. How it changes

Every change to `telemetry/events.toml` is a line in the release notes. The tables on this page
are made from that file and from the report format, so they change with them, and a test fails
until they match. A new kind of report or a new key in one is a new version of the format in
lib-visits, and gets its own release note.

## 9. Your rights

The controller of this data is Arkos's maintainer, Cyrus Sarkosh. To reach him, open an issue at
<https://github.com/csarkosh/ide-arkos/issues>.

Session and crash reports are sent on the basis of your consent. The update check is sent
because it is necessary for the self-updating program you installed.

The data cannot identify anyone, and Arkos keeps nothing that could link a report to a person, so
a request to access or erase one person's data cannot be fulfilled for it.

You can withdraw your consent at any time. Turning usage reports to `off` or `local` stops session
reports, and turning crash reports to `never` stops crash reports, including those already
waiting in the queue.

You have the right to complain to a supervisory authority, such as the data protection authority
where you live.
