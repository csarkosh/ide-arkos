# Telemetry: what Arkos sends, when, and how a person says yes

**Date:** 2026-10-16
**Status:** Draft for review
**Depends on:** lib-visits app reports (`csarkosh/lib-visits`, `docs/superpowers/specs/2026-10-16-app-reports-design.md`, the collector route and the `visits-app` crate), the daemon's update check (`crates/arkosd/src/updates`), Settings (milestone 8a), the agent interface (8b), the guide (`docs/agent-guide.md`)
**Research:** csarko.sh `docs/published/2026-10-15-telemetry-without-identifying-anyone.md`
**Rulings (Cyrus, 2026-10-16):** a per-session journey with no persistent identifier; the update check moves to a first-party path and counts installs without consent; usage and crash reports are opt-in; the crash dialog's "Always send" starts unchecked; the collector lives in lib-visits and Arkos imports its client crate; the dashboard comes last.

## 1. What this is

Arkos has no way to tell how many people run it, what they use, or whether it breaks for them. This adds three things, each with its own consent:

- **The update check** says, once a day, that a machine with this version, OS and architecture exists. It goes through the maintainer's own path and needs no consent, because the person installed a self-updating tool and an update check is what one sends. It carries nothing else.
- **A session report** says what one run of the IDE did: which surfaces were opened and for how long, which agents were launched, which markdown features were used, how the run ended. It is recorded locally by default, uploaded only after the person turns usage reports on, and keyed by a random id that dies with the process.
- **A crash report** says where a panic happened, in which build, with a stack of offsets the maintainer symbolicates. It is written first and sent only after a dialog that shows its bytes.

The rules that every part keeps:

- **Nothing that identifies a person or a machine leaves it.** No hostname, username, home directory, path, file name, project name, environment variable, prompt, agent transcript, terminal content, IP kept, or persistent id. The one random value, the session id, exists for one run.
- **Every field is typed and allowlisted** in `telemetry/events.toml`, which is both the filter the client enforces and the schema the disclosure page shows. A value that is not in the allowlist is dropped and counted.
- **Telemetry never blocks the person.** Recording is an append to a file; uploading is the daemon's background job with a short timeout and silent failure.
- **The disclosure is true of the code**, ships inside the app, and names the files that are authoritative.

### What it will not do

- Collect anything from a source build, including the shoot's and a developer's: the channel `source` never uploads, and the collector refuses it anyway.
- Record what a terminal shows, what an agent said, what a file is called, or what was typed anywhere.
- Ask twice. One invitation, dismissable, and the switch lives in Settings after that.
- Send a crash report with the message in it. Panics Arkos raises on purpose carry a kind; the rest carry a hash.

## 2. The three reports

The wire shapes are lib-visits's (its section 2); this section says what Arkos puts in them.

### 2.1 The check (no consent)

The daemon fetches `manifest.json` every `update.check_every_secs` from `DEFAULT_UPDATE_URL` on GitHub. After this change, the first check of each UTC day goes to `https://apps.csarko.sh/a/check?v=2&app=arkos&ver=<version>&bin=daemon&os=macos&osv=<major>&arch=<arch>&ch=<channel>&k=check&rd=<days>`; the collector logs one row and answers a redirect to the GitHub manifest, which the existing fetcher follows (its final-host allowlist already admits only GitHub; the requested host gains `apps.csarko.sh` as a permitted start). The day's other checks go straight to GitHub as today. If the first-party request fails in any way, the daemon falls back to GitHub for that check and tries the first-party path again the next day, so a collector outage never delays an update. The manifest's `.sig` and the zip always come straight from GitHub.

`rd` comes from `visits_app::Check::next` and the daemon's telemetry state file. The daemon sends the check with `User-Agent: arkos/<version>` (curl's `--user-agent`), which the collector requires.

The channel is `release` for a bundle the release workflow built (a `channel` file in the bundle's `Contents/Resources`, written by the workflow) and `source` otherwise. A source daemon checks GitHub directly and never touches the first-party path. Telling `tap` from `direct` is deferred; Homebrew counts cask installs itself.

### 2.2 The session (consent: usage reports on)

**The journal.** The app writes, through `visits_app::Journal`, to `~/Library/Application Support/arkos/telemetry/journal/<sid>.jsonl` while it runs, when usage is `local` or `on`. `sid` is 16 random bytes as hex, made at launch. The events, with their `d` keys and the allowlisted values (the file `telemetry/events.toml` is the authority; this table is its content at the first version):

| `n` | `d` | When |
|---|---|---|
| `launch` | `how`: `normal`, `relaunch`, `updated`, `recovered` | the app started; `recovered` when the daemon's manifest said the last window died |
| `window_open`, `window_close` | | a project window |
| `pane_open` | `kind`: `terminal`, `viewer`, `browser`; `how`: `palette`, `key`, `click`, `explorer`, `link`, `restore` | a tab was opened |
| `pane_close` | `kind` | a tab was closed |
| `view` | `format`: `markdown`, `code`, `plain`, `image`, `video`, `binary`, `too_large`; `mode`: `rendered`, `raw` | the viewer showed a file |
| `md` | `feature`: `table_edit`, `mermaid`, `details`, `checklist`, `remote_image`, `video`, `inline_diff` | first use of a markdown feature in this session, once per feature |
| `agent_launch` | `agent`: `claude`, `other`; `how`: `new`, `resume` | an agent started in a terminal |
| `agent_state` | `agent`; `state`: `waiting` | an agent first asked for input in this session, once per terminal |
| `palette` | `command`: the `Command` enum's name in snake case | a palette command ran |
| `settings` | `key`: the config key changed, e.g. `font.family`, `viewer.wide_tables`; never its value | a Settings edit or `arkos settings set` |
| `theme` | `builtin`: `true`/`false` | a theme was chosen; built-in themes are not named, so a custom theme is just `false` |
| `update` | `step`: `staged`, `applied`, `failed`, `rolled_back` | the updater moved |
| `notice` | `kind`: `update_ready`, `updated`, `daemon`, `config_error`, `theme_error` | the bell got a notice |
| `error` | `kind`: `daemon_unreachable`, `daemon_restarted`, `session_lost`, `chrome_missing`, `mermaid_failed`, `save_failed` | something the person would have seen |
| `help` | `what`: `keybindings`, `documentation`, `about`, `telemetry` | a Help surface opened |

`focus` accumulates foreground seconds per tab kind (`terminal`, `viewer`, `browser`) and `settings`, flushed by `Journal::mark` once a minute and at quit. The journey Cyrus asked for is the ordered events with their `t`; iteration on granularity comes after dogfooding, as a change to `events.toml` and one release note line.

**Sealing.** The daemon seals the journal into `telemetry/queue/session-<sid>.json`: on `Goodbye` from the window with `end: quit`, on the window's socket closing without one with `end: lost`, and at its own start for any journal left behind with `end: unknown`. The daemon knows the `sid` because the window sends it in `Hello` (protocol 7 adds `sid: Option<String>`). The report's `iw` and `rd` come from the daemon's state file.

**Uploading.** The daemon runs the queue every 15 minutes, posting with the system curl (`--proto =https`, the host pinned to `apps.csarko.sh`, `--max-time 10`, `--user-agent arkos/<version>`, the body from the file), only when the effective mode uploads. Success deletes the file; failure reschedules on the crate's backoff; a refusal other than `429` deletes and counts.

### 2.3 The crash (consent: per incident, or "always")

**Capturing.** Both binaries install a panic hook at startup that writes `telemetry/incidents/<inc>.json` and then calls the previous hook. The report holds the panic's remapped `file` and `line` (the release build passes `--remap-path-prefix` for the source root and the Cargo home, so a path says `/src/crates/arkos/src/md_view.rs` and never a home directory), the `commit`, a `pk` when the panic came from one of Arkos's own named failure points (a small enum of `panic!` sites that matter, else `other`) and `mh` for the rest, the uptime bucket, and the faulting thread's frames as `(image index, offset)` pairs from `backtrace` plus `dladdr`, with the `images` list of the Arkos binaries only (system libraries by name, no address). The hook holds no lock the panicking thread may hold and writes with one `write` call to a pre-opened file descriptor where it can. The daemon also records, when a window's socket closes without `Goodbye` and the `sid` is known, the `lost` end on the session report; that is the crash-free rate without a crash report.

**Symbols.** The release workflow builds with `split-debuginfo = "packed"` and attaches `Arkos-IDE-<version>-dSYM.zip` to the GitHub release beside the app zip, so a report's offsets symbolicate with `atos` against the published dSYM. The symbols reveal nothing the GPL source does not.

**The dialog.** At the next interactive launch of the app, if `telemetry/incidents/` is not empty and crashes are `ask`, one window (the About window's family, transparent title bar) says what happened in one sentence ("Arkos IDE crashed on <date>" or "The background daemon crashed on <date>"), with a **Show report** disclosure that renders the exact bytes in a monospace box, **Don't send** (deletes the file), **Send** (moves it to the queue), and the unchecked box **Always send crash reports** (sets `telemetry.crashes = "always"`; from then on incidents go to the queue without a dialog, and Settings shows the switch). Several incidents are one dialog with a count and one decision for all. With crashes `never`, incidents are deleted at launch. With uploads off for any environment reason, the dialog is not shown and the incidents wait.

A **Reports** list in Settings shows past incidents by date with their `inc`, as a receipt the person can quote in an issue.

## 3. Consent and the surfaces

### 3.1 Config

```toml
[telemetry]
usage = "local"     # "off" | "local" | "on"
crashes = "ask"     # "never" | "ask" | "always"
```

Both have defaults, so an existing config is unchanged. `arkos settings get|set telemetry.usage` and `telemetry.crashes` work as for every key, and `arkos config reload` applies them.

### 3.2 Environment

`DO_NOT_TRACK`, `ARKOS_TELEMETRY=off|log`, `CI`, and a non-interactive session, exactly as the crate's mode table says. `ARKOS_TELEMETRY=log` prints every payload the daemon would send to its log instead, and the app's invitation says so.

### 3.3 The invitation

On the second launch of a `release` build that is interactive, with usage `local` and `telemetry/asked` absent, the window shows a banner strip under the tab bar of the first pane: "Help improve Arkos: share anonymous usage reports. Nothing identifies you or your files." with **Turn on**, **Not now** and **What is sent**. Either button writes `telemetry/asked` and the banner never returns; Settings holds the switch. The second launch, not the first, so a person who tries Arkos once and deletes it is never asked. "What is sent" opens the disclosure page in a viewer tab.

### 3.4 Settings, a Privacy section

Rows: **Usage reports** (Off / Local only / On, with one line under it saying what the current mode means and why, from the crate's `reason` when the environment forces it), **Crash reports** (Never / Ask each time / Always), **What Arkos sends** (opens the disclosure), **Last report** (opens the bytes of the last sealed session in a viewer tab), **Reports** (the incidents list). The Help menu gains "What Arkos Sends" under Documentation.

### 3.5 The command line

`arkos telemetry status` (the effective mode, its reason, the queue), `arkos telemetry on|off|local`, `arkos telemetry view [--last | <path>]` (the bytes), `arkos telemetry crashes never|ask|always`. The doctor gains a row: the mode and whether the queue drains. The guide gains a short section so an agent can answer "what does Arkos send" from the disclosure and never changes the setting on its own; `arkos settings set telemetry.usage on` works, but the guide says that is the person's choice.

## 4. The disclosure page

`docs/telemetry.md`, compiled in like the guide and opened by every "What is sent" and "What Arkos sends" surface; published on the website from the same file. The nine items from the research, in this order: the field table with an example value and the reason for each field, drawn from `events.toml` and lib-visits's section 2; the list of what is never collected; the default state and the exact commands and environment variables; how to print a payload before it is sent; the transport, where the rows live (Google Cloud, US, the maintainer's project), and retention (rows 365 days; the platform's request log with the address excluded for the collector's routes, or, where the platform keeps it, truncated and under 7 days, stated as built); who reads raw rows (one person) and what is published (aggregates by version, OS, architecture and channel with cells under ten merged); the authoritative files (`telemetry/events.toml`, `crates/arkos/src/telemetry.rs`, `crates/arkosd/src/telemetry/`, lib-visits's `crates/visits-core/src/app.rs`); the change process (every change to `events.toml` is a release-note line); and the legal sentences: the maintainer's name and contact as controller, consent as the basis for session and crash reports and necessity for the update check, that the data cannot identify anyone so access and erasure requests cannot be fulfilled for it, the right to withdraw by turning it off, and the right to complain to a supervisory authority. Homebrew's clause, in Arkos's words: these numbers inform, they do not decide; low use alone never removes a feature.

## 5. Where the code goes

- `crates/arkos/src/telemetry.rs`: the app's recorder (the journal, the allowlist from `events.toml` via `include_str!`, the focus clock, the event sites), the invitation, the crash dialog, the Settings rows, the `telemetry` subcommand, the panic hook. Event sites are one call each at transitions the code already branches on: `pane_view`, `viewer`, `launch`, `command_palette`, `settings`, `theme`, `update`, `notices`, `main`.
- `crates/arkosd/src/telemetry/`: the state file, sealing, the queue loop, the first-party check routing, the daemon's panic hook. The daemon is the only uploader.
- `crates/protocol`: protocol 7 adds `sid` to `Hello` and a `TelemetryStatus` request for the `status` command and the doctor row.
- `telemetry/events.toml`: the allowlist. `docs/telemetry.md`: the disclosure.
- `visits-app` as a git dependency pinned by tag; `visits-core` comes with it.
- The release workflow: the `channel` file, the dSYM archive, `--remap-path-prefix`.
- csarko.sh: the collector's `--app arkos=<manifest url>`, the Hosting rewrite of `apps.csarko.sh/a` and `/a/check`, the request-log exclusion widened to the routes and to `302`, and the `telemetry.md` copy on the product page. Its own small spec in that repository.

## 6. Testing

- `telemetry.rs` pure parts: the allowlist refusals per event, the focus clock across tab changes, the invitation's conditions (second launch, interactive, release, local, not asked), the dialog's decisions and their file moves.
- `arkosd` telemetry: sealing on `Goodbye`, on a lost socket, at start; the check routing (first of the day first-party, the rest GitHub, fallback on failure); the queue loop against a fake transport (sent, failed, rejected); the panic hook's output against the schema, in a subprocess test.
- The protocol round trip for `Hello` with and without `sid`.
- Hand checks owed: a real panic in a release build symbolicated with `atos` against the published dSYM; the first-party check row seen in BigQuery with `rd` right on the second day; the shoot's source build sending nothing (the collector's log clean after a shoot).

## 7. Order and release

1. lib-visits: the route, the check, the crate; release v0.3.0.
2. csarko.sh: deploy the collector with the app, the rewrite, the exclusion; see a hand-made check row land.
3. ide-arkos: the daemon side first (check routing, state, queue, sealing, panic hook), then the app side (journal, event sites, invitation, Settings, dialog, CLI, disclosure), then the release workflow changes; one release, 0.5.0, with a release note that says what is sent and that it is off until turned on.
4. The dashboard, last: a tab in the csarko.sh admin app over the dataset.

## 8. Out of scope

- Signal crashes, minidumps, Apple's `.ips` files: the schema has the kind; the hook handles panics.
- Telling `tap` from `direct`.
- A freshness token for cloned machines.
- Aggregate publication: the disclosure promises aggregates with cells under ten merged; the first version publishes nothing until the dashboard exists.
- Prose or UI font events, terminal sizes, keystroke counts: nothing that measures a person rather than the product.
