# Layouts: Grid and Inbox

## Intent

Cyrus, 13 November: "I want to look at building a new "Layout" system and making it configurable
in the "Settings" menu. The current layout is how I see a "Grid" layout. The next layout I want
to look at is a hybrid between the Codex / Claude Desktop apps **and** how clients like Slack /
Gmail handle their unread / unresponded / sent notifications (we'll use that to help the user
manage which sessions need their attention, are complete, waiting on their response, etc)."

His choices from the sketch (`docs/superpowers/brainstorm/layouts/directions.html`): the third
step, "An inbox across projects"; the name **Inbox**; and "This window. Keep other windows
independent." He added: "For the sessions, I should be able to right click and end them", and:
"I also want the tab placement when a Claude Code session opens a new file to be customizable.
Right now, the Grid system prefers "Inplace" (Claude opens a new tab in the current pane). I
also want a "Neighbor" option which prefers the pane to the right or left of a current pane. If
there's only a single tab pane open, it creates a new one to the right of the current pane and
opens the item there."

My reading of the first and third answers together: everything in the third step (Later, ended
sessions kept and resumed, a macOS notification), with each window's Inbox listing only its own
sessions. "Across projects" is then the repositories under the window's folder.

## What exists

- A window's tabs live in one `Tree` of panes (`tree.rs`), saved per project (`workspace.rs`).
- A session's state is `ClaudeState`: `Idle`, `Working`, `Waiting`. Claude Code's hooks report
  it through `arkos notify`; the Stop hook and the permission notification both say `waiting`,
  and a click on the tab or a key typed into it sets it back to `Idle`.
- `arkos open` with neither `--pane` nor `--split` adds a tab to the calling session's pane
  (`direct::plan_open`, `Placement::Here`).
- Nothing reads a transcript, posts a macOS notification or badges the Dock icon.

## The config

```toml
[layout]
name = "grid"            # "grid" or "inbox"
placement = "neighbor"   # where a tab a session opens goes in the Grid: "neighbor" or "inplace"
notify = true            # a macOS notification when a session needs you and the window is not in front
```

Settings gains a **Layout** section: Layout (Grid, Inbox), "A session's files open" (In place,
Neighbor) and the notification switch. `arkos settings set` takes the three keys.

## Placement

It decides a plain `arkos open` (`Placement::Here`); `--pane` and `--split` say their own, and a
file already open is still shown where it is.

- **In place**: a new tab in the session's pane, as today.
- **Neighbor**: a new tab in the pane to the right of the session's pane; with none there, the
  pane to its left; with neither (a single pane, or panes only above and below), a new pane split
  off to its right. (The last case beyond a single pane is my call.)

In the Inbox layout a session's files always go to the side pane, whatever this says.

## The states

A session gains an **attention** beside its `ClaudeState`: whether its agent is **blocked**
(it cannot go on until the user acts) and a **line** that says what it asks or last said.
`ClaudeState` itself is unchanged, so a window or a daemon from before this still reads a
blocked session as `Waiting`. The hooks become one command for every event that says something
of the state, `arkos notify --hook`, which reads the event's JSON and decides:

| Claude Code event | State | Blocked | The line |
| --- | --- | --- | --- |
| `UserPromptSubmit`, `PostToolUse` | `Working` | no | kept |
| `PreToolUse` | `Working` | no | the tool and what it does: "Bash: cargo test" |
| `PreToolUse` of `AskUserQuestion` or `ExitPlanMode` | `Waiting` | yes | the question, or "Plan ready for review" |
| `PermissionRequest` | `Waiting` | yes | "Allow {tool}: {its command or file}" |
| `Notification` (`permission_prompt`) | `Waiting` | yes | kept (the event above said which tool) |
| `Notification` (`elicitation_dialog`, `agent_needs_input`) | `Waiting` | yes | its `message` |
| `Stop` | `Waiting` | no | the first line of `last_assistant_message` |
| `SessionStart`, `SessionEnd` | as today | | |

One command per event, because Claude Code runs an event's hooks side by side and two entries
for `PreToolUse` would race. The `idle_prompt` notification is no longer listened for: `Stop`
already says the turn ended. The old `--state` and `--event` forms still work, so hooks
installed before this keep reporting (blocked and finished both as `waiting`) until the
doctor's row, which already compares the entries, reinstalls them. No hook says a permission
was answered, so Enter or a digit typed into a blocked session puts it to work.

The line travels with the state: `Request::SetAttention { session, state, blocked, note }`,
kept by the daemon per session in memory, sent on as `Event::Attention` to the attached
windows on protocol 11, and listed for a connecting window by `Request::ListAttention`. All
appended; the protocol becomes version 11. A hook that finds a daemon from before it (the
connection drops) says the state alone in the old form. A plain `SetState` ends a block.

A window adds what only it knows, per session, saved with its workspace:

- **seen**: the user has looked since the session last began to wait or was blocked. Set when
  the session is shown and focused (a state that arrives while it is shown, focused and the
  window is in front is seen at once). A hooked session is no longer set to `Idle` by a click:
  it stays `Waiting`, seen. A session with no agent record (a shell that rang the bell) is
  cleared to `Idle` as today.
- **done** and **later until**: set by the user; both are dropped when the session next becomes
  `Waiting` or is blocked, so a session put away comes back when there is something new.

What a row shows, `attention::of(state, alive, marks, now)`:

| Attention | When | Mark |
| --- | --- | --- |
| Needs you | blocked | the warm dot inside a ring |
| Unread | `Waiting`, not seen | the warm dot (today's) |
| Unanswered | `Waiting`, seen | a warm ring |
| Working | `Working` | the breathing dot (today's) |
| Quiet | not finished, nothing happening: a plain shell, or a Claude not yet asked anything | none |
| Later | `Waiting` with a later time ahead | none, the time instead of the age |
| Done | marked done, or the shell ended | muted |

The marks use the themes' `working` and `waiting` colours, so no theme file changes. The tab
bar's marks in the Grid use the same shapes. Overworld's "continue" arrow shows for Needs you
and Unread.

## The session list

One list, `sessions_list.rs`, of the window's terminal tabs, grouped under Needs you, Unread,
Unanswered, Working, Quiet, Later and Done (collapsed), each with its count, empty groups left
out, newest change first within a group. A row: the mark, the tab's name, the age of its last
change, and under them its line (for an idle shell, its folder). A session whose folder is
another repository than the window's says which.

- Grid: a **Sessions** section in the sidebar above the Explorer, closed or open like the
  others. A click focuses the session's tab.
- Inbox: the sidebar's top has **Sessions** and **Files**; Sessions is this list with a filter
  field over it and "New session", Files is the Explorer and Worktrees as today.
- Right click on a live row: End, Mark unread or Mark read, Done or Not done, Later (In an
  hour, Tomorrow at 9), Rename. End closes the session's tab as Cmd+W does, with the same minute
  in which Shift+Cmd+T brings it back.
- An ended session that had a Claude Code record is kept under Done (the last 50, saved with
  the workspace) with its name, folder, line and resume command; a click resumes it in a new
  tab, and its right click has Resume and Remove. The filter field searches these too.
- "Next session that needs you" (a command with a key): the first row of Needs you, then
  Unread, then Unanswered.
- The Dock icon's badge is the window's count of Needs you and Unread.
- A session that becomes Needs you while the window is not in front posts a macOS
  notification, its title the session's name and its text the line; a click brings the window
  forward on that session. Only from the installed app (a build run from `target` has no bundle
  to post as), and only with `layout.notify`.

## The Inbox layout

The same `Tree`, kept in a fixed shape: one **sessions pane** holding every terminal tab, and,
when there are any, one **side pane** to its right holding every other tab. So saving,
recovery, `arkos open`, find, focus and the keys for the next and previous tab all work as they
do in the Grid.

- `inbox::arrange(tree)` puts any tree in that shape, keeping every tab, its id, its name and
  which tab was active; it runs when the layout is switched to and after every change to the
  tree while it is on, so nothing can leave a terminal in the side pane or a file among the
  sessions. Splitting, and dragging a tab to make a pane, do nothing in this layout.
- The sessions pane has no tab bar: the list is its tab bar. In its place a header: the
  session's name, its folder and branch, "Mark unread" and "Done".
- The side pane is an ordinary pane of tabs. Choosing a session brings forward the tab that
  session opened last (`arkos open` records which session opened a tab). Maximize (the existing
  command) hides and shows the side pane.
- Each layout remembers its arrangement: switching to Inbox keeps the Grid's tree aside, and
  switching back restores it, less the tabs closed meanwhile and with the tabs opened meanwhile
  added to its focused pane.

Not in this version: a Changes tab listing what a session modified (the sketch showed one), and
sessions from other windows.

## As built, 13 November

Built and tested headless: the filter field, the placement, the attention (hooks, daemon, marks), the list in
both layouts with its menu, Shift+Cmd+U, the Dock count, closed Claude Code sessions kept
under Done and resumed, the layout switch and the Inbox's window, Settings' Layout section.

Not built yet, with why:

- **The macOS notification.** It can be posted only from the bundled app, so it cannot be
  tried from a `target` build; it waits for a pass with a local bundle (`packaging/test.sh`).
  The `layout.notify` key is not in the config until then.

After Cyrus tried it, the same day: "when i click on a new session, all the tab panes should
be replaced. they only exist within the session." So the side is the session's own
(`inbox.rs`, `Sides`): one pane or several beside the sessions pane, lifted out of the tree
whole when another session is chosen (`Tree::inbox_take_side`) and put back when its own is
chosen again, saved with the workspace by the session's tab. A file opened from the Explorer
joins the session shown; an agent's `arkos open` joins its own session, also while another is
shown (its side is brought into the tree for the open and put away after); closing a session
closes its tabs first. Each tab's session is remembered through the Grid (`Sides::owner`).
This replaces "Choosing a session brings forward the tab that session opened last" and the
fixed two panes: a split among a session's panes now stays.

Cyrus, later the same day: "make "neighbor" the default new tab placement option." So
`layout.placement` is `neighbor` unless the config says `inplace`, which changes where a plain
`arkos open` lands for everyone who has not set it.

Differences from the text above: a session's row says its folder only when it has no line of
its own; a kept session is recorded when its tab closes (not when its shell exits, whose tab
stays as an ended row until closed); an agent's record reaches the window by a new
`Event::Agent`.

## Constraints

- Tests first, headless, named as sentences; the tree's regrouping, `attention::of`, the
  placement plan, the hook's mapping and the list's grouping are pure and tested.
- Motion on Material's tokens through `motion.rs`: the list's rows and the layout switch use the
  existing enter and exit; nothing new animates without adding itself to `Root::any_motion`.
- Colours read from `root.theme` at paint. The marks are checked by the contrast tests as the
  tab marks are.
- `docs/guide.md`, `docs/agent-guide.md` and the Status checklist are kept true.
