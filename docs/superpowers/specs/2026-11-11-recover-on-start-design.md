# A daemon that was killed brings its sessions back when it next starts

Status: design, 2026-11-11. Asked for by Cyrus.

## Why

On 2026-11-10, about 15:51 Eastern, the antivirus on Cyrus's work Mac killed `arkosd` outright.
Every shell and every Claude Code process went with it. When he opened Arkos again the tabs were
there with their names, each one a fresh shell, and he started a new `claude` in each and told it
to find the conversation it replaced. The usage reports show it: one window's report opens with
eight `session_lost` errors two seconds after launch, then six `agent_launch` (`new`) between two
and five minutes in. His words: "How can Arkos IDE be updated so that these sudden daemon kills
allows Arkos to recover the Claude sessions automatically when I start it back up again while
still being extremely light on my system's resources?"

Everything a recovery needs was on disk when the daemon died. The flusher keeps
`sessions/manifest.bin` (each session's folder, shell and agent record, `claude --resume <id>`)
within a second of the truth and each `sessions/<id>.replay` within two seconds of quiet output
(fifteen under output that never pauses), and neither write waits for a signal. But the manifest
has one reader, `arkosd --recover`, and one caller, the login item, which runs once at login. A
daemon the window starts (at launch, or ten seconds after it lost the old one) starts empty, each
tab whose session is gone becomes a new shell, and the first of those shells changes the live set,
so the flusher writes a new manifest over the old one. The histories nothing names any more stay
in `sessions/` for good: the development Mac had ten of them, about 130 MB, from two daemon
deaths in October.

The update-in-place spec ruled this out on purpose ("Surviving a daemon crash, a logout or a
reboot ... nothing survives daemon death") and its section 11 added the reboot alone. This spec
adds the crash and the kill, with the files and the code section 11 already has.

## 1. A cold start recovers

A daemon started fresh (not an upgrade's `--resume`) reads the manifest before it serves, whoever
started it: the window at launch, the window's reconnect loop, the launch chooser's link, or the
login item. Three modes, chosen by the arguments, the last one given winning:

| Arguments | Sessions | Windows | Who passes it |
|---|---|---|---|
| none | recovered | remembered, not opened | a window, the chooser |
| `--recover` | recovered | reopened | the login item, as today |
| `--no-recover` | left alone | left alone | a sandbox's window, the shoot, tests |

Recovering a session is what `--recover` does today and is not changed: a new shell under the old
id (so the window's saved layout finds it), in its folder or the home folder when that is gone,
with its recorded shell or the default one, its history under the dim "resumed after restart"
line, what was left of a closed tab's minute, and its agent's resume command typed in. The tab
says "(resumed after restart)" until the user types into it. Recovery only resumes: it never
sends a message, so whatever the agent was doing when the daemon died is not run again until
the user asks.

Without `--recover` no window is opened: the window that started the daemon is the one the user
has, and a second on the same folder would be wrong. The manifest's folders are still put in
the daemon's window list (those `window_folder` accepts and that still exist), as a window that
vanished without a Goodbye stays in it today, so a later restart or a rollback still reopens
them.

`--recover` while a daemon is already listening still exits 0 at once. A plain start in that
case fails as it does today.

A sandbox's daemon and the shoot's are throwaway: `arkos sandbox stop` ends a sandbox's sessions
and they must not come back at the next `arkos sandbox`, so the window passes `--no-recover` in
a sandbox and the shoot passes it always.

`pkill -x arkosd` no longer ends every session for good: SIGTERM writes every history and the
manifest, and the daemon the window starts next recovers them. What was running in the shells
is still ended. Every place that says "ends every session" says "restarts every session".
"Stop all" in the launch chooser is the way to end a folder's sessions.

## 2. Histories nothing names are removed

A recovering start removes every `<id>.replay` the manifest does not name, as `--recover` does
today, and now also when there is no manifest at all (every history is then one nothing names).
An unreadable manifest is still moved to `manifest.bin.bad` and the histories beside it are left
for a look; the next recovering start with a readable manifest removes them. `--no-recover`
removes nothing.

## 3. A daemon killed again and again does not resume the agents each time

A window that outlives its daemon starts a new one ten seconds later, for as long as it is open.
If whatever killed the daemon kills each new one too, every start would type `claude --resume`
into every tab: several Claude Code processes started every few seconds, unattended. So the
resumes are budgeted: at most 3 recoveries in any 5 minutes type the resume commands. A recovery
past that still brings back every shell and its history, keeps each agent's record (so a later
recovery resumes it), and instead of typing writes one more dim line under the divider:

```
── resumed after restart ──
── not resumed, the daemon restarted 3 times in 5 minutes: run 'claude' '--resume' 'ab-12' ──
```

The line is the resume command as recovery would have typed it, so it can be copied. A command
with a control character is neither typed nor shown, as today.

The times of the recoveries that typed a resume command are kept in `sessions/resumes`, one
line of seconds since the epoch each, rewritten on such a recovery with the ones older than five
minutes dropped. A recovery with no agent to resume does not read or write it. Nothing clears it
on a timer: the times age out when they are next read.

A person in the loop (reopen, ask the agent to do the thing again, killed again) takes longer than
a minute and a half a round, so the budget is for the unattended loop; if it does hold a resume
back, the command is on screen.

## 4. What it costs

Nothing while the daemon runs: no new process, thread, timer or write. The manifest and the
histories are the ones the flusher writes already. A cold start reads the manifest and each
history once, and a recovery that resumes an agent rewrites one file of at most three lines.

## 5. What it does not do

- Keep anything running through the kill. The shells and agents die with the daemon; the
  conversations come back because Claude Code's transcripts are on disk. Whatever was executing
  (a tool call, a build, shell variables, background jobs) is gone, as after a reboot.
- Keep the last seconds of history: up to two seconds of quiet output, fifteen of unbroken
  output. An agent that started in the daemon's last second may not be in the manifest.
- Supervise the daemon. A process that holds the terminals so the shells outlive the daemon was
  considered and rejected: it is a second process that never exits, and what kills the daemon
  would kill it too.
- End a process that outlived its daemon. A shell that ignored the hangup is not found or
  stopped; the manifest keeps no pids.
- Change the window. Its reconnect and attach path is the one the login recovery already uses.

## 6. Tests

Daemon, against the real binary (`crates/arkosd/tests/daemon.rs`):

- a daemon ended with SIGKILL and started again with no arguments has the session under its old
  id, marked recovered, its history above the divider and its resume command's output below it
- a plain start opens no window and still lists the folder in the manifest it writes;
  `--recover` opens it (the existing test)
- `--no-recover` starts empty and removes nothing
- a start with no manifest removes a history left behind
- with three recoveries in the last five minutes on record, the session comes back, the resume
  command is not run, and the history ends with the "not resumed" line

Unit (`recovery.rs`): the budget (three in five minutes, old times dropped, an unreadable file
is no times), the "not resumed" line's text and quoting, the sweep. Window (`main.rs`): a
sandbox's daemon arguments carry `--no-recover` and a real window's do not.

By hand (`docs/development.md`, Status): four tabs with `claude` running, `pkill -9 -x arkosd`,
wait: each tab comes back "(resumed after restart)" with its conversation; the same with the
window killed too and Arkos opened again.

## 7. Docs

`docs/guide.md` ("After a restart", "Updating"), `docs/development.md` (Build and run, Status),
the `[recovery]` and `[daemon]` comments in the default config, and the update-in-place spec's
non-goal gets a pointer here.
