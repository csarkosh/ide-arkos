# Arkos IDE: update and launcher polish (milestone 6a)

Date: 2026-10-15. Status: written from the rulings Cyrus gave in chat; awaiting their review.
Builds on the auto-update design (`2026-10-14-auto-update-design.md`), the update-in-place design
(`2026-10-14-update-in-place-design.md`) and the launcher and incident work of 2026-10-15. Every
item here was deferred from one of those and confirmed still present at `39d4b89` (0.2.10).

## 1. Why this exists

The update path and the launcher are what Cyrus meets every day: the daemon checks every five
minutes, a release lands while they type, and the chooser is the first thing a Dock launch shows.
Each item below is a place where that path lies, stalls, or shows nothing. None changes what the
IDE does; together they make what it already does trustworthy.

Rulings taken in chat on 2026-10-15:

- The `checking` state reaches the window: protocol 5, with the old shape still served.
- A Dock launch that cannot start the daemon shows a real window naming the daemon log, with a
  way to open it, instead of exiting to nowhere.
- The journal collapses repeated "not newer" checks into one line with a count.
- Sessions a crashed window would orphan are ended by the daemon. The ten-minute rule asked
  for in chat is replaced by a daemon-side close grace (section 14), for a reason that only
  showed in the code: a window that quits on purpose leaves its tab sessions running so the
  folder reopens with them, and the daemon cannot tell those from the orphans.

## 2. Scope

| # | Item | Where |
|---|---|---|
| 1 | "no update" notice waits for the check and says what it found | window, daemon, protocol |
| 2 | A failed upgrade starts the grace clock | daemon |
| 3 | The last successful check survives a daemon restart | daemon |
| 4 | "update ready" outranks "unreachable" and "failed" in the title bar | window |
| 5 | Startup never blocks the main thread; a daemon that cannot start gets a window | window |
| 6 | The chooser reconnects and does not freeze on a cold daemon | window |
| 7 | The journal collapses repeated checks | daemon |
| 8 | The stale-staged drop waits for an apply in flight; the "deleted" line names the right version | daemon |
| 9 | A staged bundle from before a restart is adopted, not downloaded again | daemon |
| 10 | Small cleanups: one protocol constant for update events, `old_kept` tolerated when absent | daemon, protocol |
| 11 | The chooser's text follows the preset and the appearance | window |
| 12 | Two windows opening at once both reach the recents file | window |
| 13 | The closed-tab grace lives in the daemon | daemon, window, protocol |

Out of scope: the browser follow-ups (6b), the sidebar and memory items (6c), any change to what
is downloaded or how it is verified.

## 3. Protocol 5

The wire format is postcard: positional, no field names. A struct variant cannot grow a field
without breaking an older peer, so the repo's rule holds: new variants are appended, never
inserted, and existing shapes never change. `PROTOCOL_VERSION` becomes 5; `PROTOCOL_MIN` stays 1.

Appended:

```rust
// Request, after ApplyUpdate (index 19, 20):
/// `UpdateStatus` with the check's state. A protocol 5 window sends this one.
UpdateStatusDetailed,
/// The window closed a tab: end `session` unless a window attaches it within `secs` seconds
/// (section 14). Answered `Ok` at once; a second request restarts the grace.
DetachForGrace { session: SessionId, secs: u32 },

// Response, after UpdateStatus (index 8):
UpdateStatusDetailed {
    staged: Option<StagedUpdate>,
    last_error: Option<String>,
    unreachable_for_secs: Option<u64>,
    /// A check is running now (asked for, or on the timer).
    checking: bool,
    /// When the last check finished, whatever it found; None before the first.
    last_check_unix: Option<u64>,
    /// The last check's transport failure, None once a check reaches the manifest again.
    transient_error: Option<String>,
},
```

The daemon answers `UpdateStatus` (old shape) and `UpdateStatusDetailed` alike; the window sends
the detailed one when the daemon's `protocol_max >= 5` and the old one otherwise (today's literal
`protocol_max >= 4` gate moves to the protocol crate as `UPDATE_STATUS_FROM = 4`, beside a new
`UPDATE_STATUS_DETAILED_FROM = 5`, `DETACH_FOR_GRACE_FROM = 5`, and the daemon's existing
`UPDATE_EVENTS_FROM = 4`, which moves there too). The tests that pin the version and the variant
indexes (`new_variants_are_appended_after_the_old_ones`, the codec check) are updated with the new
indexes.

## 4. The update check and its notice (item 1)

Today "Check for updates" sleeps three seconds and asks once; it says "no update" when the daemon
has never reached the manifest, says nothing after a transient failure, and can say "no update"
while a slow check is about to stage a release. The daemon already keeps a `checking` flag; nothing
reads it.

New behaviour:

- `CheckForUpdates` is answered `Ok` as today. The window records `asked_at` (unix seconds) and
  polls `UpdateStatusDetailed` every 500 ms until `!checking && last_check_unix >= asked_at`, or
  30 seconds pass. (`last_check_unix` is set when a check finishes, so a check still being picked
  up by the poll thread, up to 250 ms, is waited for too.)
- The outcome is one notice:
  - staged: "update ready: X" (the badge appears as today; the notice confirms the check found it);
  - `last_error`: "update check failed: …" (today's text, unchanged);
  - `transient_error`: "updates unreachable: …" with the daemon's words;
  - the timeout: "the update check is still running";
  - otherwise: "no update".
- "Retry update" uses the same path, so its outcome is spoken too.
- The daemon keeps `transient_error` beside `last_check_transient` (same lifecycle: set on
  `Outcome::Transient`, cleared on any other outcome) and sets `last_check_unix` where
  `record_at` runs.

A protocol 4 daemon (an older `arkosd` still running) gets today's single query after three
seconds, with one correction that needs no protocol: `unreachable_for_secs == None` no longer
counts as "nothing"; it says "the daemon has not reached the update server yet".

## 5. A failed upgrade starts the grace clock (item 2)

After `ApplyUpdate` swaps the bundle, the grace clock starts only when the window asks to be
relaunched. If the `Upgrade` that follows fails (the exec refused, a session that would not
park, a handover that would not write), the window shows "update failed", no relaunch is asked
for, and the daemon waits forever with `.old` beside the new bundle, refusing every later apply
with "still waiting for a window".

New behaviour: every path that ends an upgrade attempt without a relaunch starts the clock when
an apply is pending: `restore()` in `upgrade.rs`, the `validate` refusal in the server, and a
refused `RelaunchClient`. Each journals "the upgrade onto X failed: …; the grace for X starts
now". The watch then rolls the bundle back after the usual 20 s unless a new window says Hello,
and the window's "update failed" notice stands. A later apply works again after the rollback.

## 6. The last successful check survives a restart (item 3)

`last_ok_unix` lives in memory, so every daemon restart or exec forgets it; a Mac that reboots on
a network that blocks GitHub shows nothing for a day, then "unreachable for 1 day" from the wrong
day. The daemon writes `updates/last_check.json`, `{ "ok_unix": N }`, through a temporary file
and rename whenever a check reaches the manifest, and reads it at poll start into `last_ok_unix`.
The file is written after `private_dir` has made the folder, like the journal; a missing or
unreadable file is None, as today.

## 7. Title bar precedence (item 4)

The title-bar item is chosen as: a staged release, or a newer bundle on disk, wins ("update
ready: X", clickable); else a failed update ("update failed"); else "updates unreachable for N
days". Today a failure or the unreachable text hides a ready badge, which is the one item that can
be acted on. The `update_ready` text is also kept when `Event::UpdateStaged` arrives, and the
unreachable text is cleared then, since a staged release means the manifest was reached.

## 8. Startup without a blocking connect, and a window for a daemon that cannot start (items 5 and 6, first half)

Today `main` connects to the daemon on the main thread before `Application::run`, waiting up to
three seconds for a cold daemon, and exits with a line on stderr when it cannot: from the Dock
that is a bounce and nothing. The chooser cannot open before the connection either.

New behaviour:

- `Application::run` starts first. The connect runs on the background executor; the main thread
  never waits on it.
- Chooser plan: the chooser opens at once, with its session counts arriving once connected (the
  link starts when the client exists; until then the rows show no trailing group and the list's
  footer says "connecting to the daemon…"). The Hello that confirms a relaunched update is sent
  as soon as the connection exists, well inside the 20 s grace.
- Project plan: the project window opens once connected, as today, with no window for at most
  the three seconds a cold daemon takes; the Dock icon bounces meanwhile, which is what every
  app does while starting.
- When the connect fails, a small window ("Arkos cannot reach its daemon", 480 by 220) shows the
  error text and three buttons: "Open the log" (`open` on the daemon log, which is
  `~/Library/Application Support/arkos/arkosd.log`), "Try again" (the connect runs again) and
  "Quit". Escape quits. This window is also what a relaunched window shows, so a failed relaunch
  after an update is seen rather than silent.

## 9. The chooser reconnects (item 6, second half)

The chooser's link gives up when the connection ends. It now runs the same reconnect loop the
project window runs (`connection::backoff`, a daemon start after ten seconds, the five-start
cap), through a shared helper that both use. Each new client sends its Hello at connect; the
link's receiver and client are replaced; the heartbeat follows the current client; the chooser's
notice says "reconnecting to the daemon…" and clears with the next session list. A folder chosen
while disconnected opens the project window on the link's current client, whose loop carries on.

## 10. The journal collapses repeated checks (item 7)

The check writes "check: the manifest has X, not newer than running Y" every five minutes, about
288 lines a day, which fills the 200 KB journal in about eight days. New rule, in the poll
thread: a "not newer" outcome identical to the previous one (same manifest version, git and
floor) is counted, not written. The count is written when the outcome changes, when the daemon
hands over or exits, and once a day while it keeps repeating: "check: N more checks since HH:MM
found X, still not newer than Y". The first occurrence is written as today. Transient failures
collapse the same way ("check: transient, N more times since HH:MM: …"). The journal then holds
weeks, and the existing test substrings still appear once.

## 11. Stale-staged drop, the "deleted" line, adoption, cleanups (items 8, 9, 10)

- `drop_stale_staged` returns at once while `applying` or `applied` is set, so a poll tick during a
  swap neither forgets the release being applied nor journals a false "deleted".
- The `ApplyUpdate` refusal that deletes a stale staged folder journals the staged version, not
  the requested one.
- At poll start, before the first check, the daemon looks for the newest `updates/<version>/`
  folder that is newer than the floor, whose bundle passes `check_bundle`, and whose version the
  saved `manifest.json` names; it becomes `staged` (git from the manifest) and the journal says
  "adopted staged X from before the restart". The first check then finds nothing newer and the
  badge never disappears across a daemon exec.
- `UPDATE_EVENTS_FROM` and the window's gate live in the protocol crate (section 3). `old_kept`
  gains `#[serde(default)]` and a comment saying it is written for older daemons and read by
  none; it is still written as `true`.

## 12. The chooser's text and appearance (item 11)

The chooser's 13 px text becomes the config's default preset size (`resolve_preset(None,
&config.font.preset, &config.font.presets)`), so it matches the window it opens. The chooser
observes the window appearance like the project window and, when `appearance.mode` is `system`,
re-themes through `theme::pick_name` and `theme::load` (built-in fallback as in `main`); a chooser
opened from a project window starts from that window's theme and follows the appearance the same
way.

## 13. The recents file under two writers (item 12)

`recent::note` reads, modifies and writes atomically, but without a lock two windows starting in
the same instant each write the other's entry away. The write takes `flock(LOCK_EX)` on
`recent.json.lock` beside the file (through `libc`, already a dependency) around the read and the
rename; the lock file stays (empty) and the test that counts the folder's files allows it.

## 14. The closed-tab grace lives in the daemon (item 13)

Closing a tab detaches its session and keeps it for 60 s so Shift+Cmd+T can bring it back; the
window kills it after the minute. The stack is in the window's memory, so a window that crashes,
is killed by the watchdog, or loses the daemon leaves those sessions alive and unattached in the
daemon for good; a restart even recovers them.

New behaviour: on closing a tab the window sends `DetachForGrace { session, secs: 60 }` (protocol
5; to an older daemon it keeps today's Detach and its own kill). The daemon records the deadline
on the session; `subscribe` clears it (a reattach, by any window); the reaper, which already walks
every session each second, ends a session past its deadline with the same signal escalation as
`Kill` and journals "session N in F: not reopened within 60 s, ended". Deadlines are kept in the
handover and the recovery manifest, so an upgrade or a restart during the minute still ends the
session on time rather than reviving it. Quitting the window still ends every closed tab's session
at once, as today. Sessions in tabs are never touched: a window that quits on purpose leaves them
running, and that stays.

## 15. Tests

- Protocol: the version and index test extended for the three new variants; a round trip of
  `UpdateStatusDetailed`; a decode of an old `UpdateStatus` frame by the new code.
- Daemon: a check that reached the manifest sets `last_check_unix` and clears
  `transient_error`; a transient one keeps the previous `last_ok_unix`, which is read back from
  `last_check.json` by a fresh state; a failed upgrade with an apply pending starts the clock and
  the watch rolls back; `drop_stale_staged` is a no-op while applying; the refusal line names the
  staged version; a bundle folder newer than the floor is adopted at poll start and a stale one is
  not; the journal writes a count instead of repeats and flushes it at handover; `DetachForGrace`
  ends a session after its grace, a reattach cancels it, a second request restarts it, and the
  deadline survives a handover.
- Window: the check outcome machine (asked, polling, each of the five outcomes, the timeout) as a
  pure function over status answers; the title-bar choice as a pure function; `note_update_status`
  keeping the ready text; `reconcile` unchanged. `recent::note` under two writers in two threads
  keeps both entries.
- Launcher: the chooser's text size from a config; the appearance switch picks the other theme
  name; the connect-failure window's text.

## 16. README

The Updating section gains: what "Check for updates" says in each case; that a failed upgrade
rolls back after 20 s; that the journal counts quiet checks; that a release staged before a
daemon restart is kept. The launcher paragraph gains the reconnect and the connect-failure
window. The Status checklist gains rows for: the chooser opening with the daemon stopped
(`pkill -x arkosd` first) and reconnecting; the connect-failure window from a Dock launch with a
broken daemon path; "Check for updates" offline saying unreachable; a closed tab ending after a
minute with the window killed in between (`kill -9` the window, wait, `ps`).

## 17. Open questions for review

1. Section 14 replaces the ten-minute rule. If you would rather have both (the grace in the
   daemon and a longer cap for sessions no window ever names), say so; the cap would need the
   daemon to know each window's layout, which it does not today.
2. Section 8 opens the chooser before the connection exists. The alternative is to keep the
   chooser waiting for the connect (never more than three seconds) and only add the failure
   window; simpler, and the chooser's counts would always be there when it appears.
