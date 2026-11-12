# A Claude Code under another config folder shows its state, and the doctor sees every folder in use

Status: built, 2026-11-12 (branch `claude-folders`). Asked for by Cyrus. Plan:
`docs/superpowers/plans/2026-11-12-claude-config-folders.md`.

## Why

On 2026-11-11 at 22:30 the Film tab had a Claude Code 21 seconds into a turn and no mark on
the tab: no breathing glyph while it worked, and no warm dot when the turn before it ended.
Cyrus's words: "When Claude is in-progress and actively working on a task, I'd expect to see the
slow blinking blue dot on the tab header. But in this screenshot there's no dot present. Why is
that and why isn't it displaying?"

A tab's mark comes only from Claude Code's hooks (`arkos notify --state working|waiting`), and
hooks live in one settings file per Claude config folder. Cyrus runs three folders, through two
aliases in his `.zshrc`:

```
alias claude2='CLAUDE_CONFIG_DIR="$HOME/.claude2" command claude'
alias claude3='CLAUDE_CONFIG_DIR="$HOME/.claude3" command claude'
```

The Film tab's `claude` ran under `~/.claude3`. Only `~/.claude/settings.json` had the six Arkos
entries; `~/.claude2` and `~/.claude3` had no hooks at all. So that Claude Code told the daemon
nothing, its session stayed `Idle`, and `TabMark::of` drew nothing.

Arkos could not see it. The window resolves one settings file, `hook_cli::claude_settings_path()`:
`$CLAUDE_CONFIG_DIR/settings.json` from its own environment, else `~/.claude/settings.json`. The
app is started with no `CLAUDE_CONFIG_DIR`, so the doctor's "Claude Code title hook" row read
`~/.claude`, said "installed", and its Install button could only ever write there. The same is
true of the two rows beside it: `~/.claude2` has `theme: dark` and no Arkos theme file (so it
keeps colours the theme row calls unreadable on some Arkos themes), and neither folder has the
wheel scrolling setting, while both rows read "on".

A second fault sits behind the first. The agent record a SessionStart hook leaves is
`claude --resume <id>`, with no folder. Once a folder other than the default has hooks, a
recovery types that command into the tab, where it runs under `~/.claude`, which does not hold
the conversation.

The hooks were installed into both folders by hand on 2026-11-12
(`CLAUDE_CONFIG_DIR=~/.claude2 arkos hooks install`, and the same for `.claude3`). This spec is
so that nobody has to know to do that.

## 1. The folders Arkos knows

Arkos keeps a list of Claude config folders: the default one (what `claude_settings_path()`
resolves today) and every other folder a Claude Code inside an Arkos session has been seen to
run under. The others are remembered in `claude-folders` in the application support folder, one
absolute path a line. The default folder is never written there; it is always first in the list.

A folder gets in one way, the probe of section 2. A line whose folder is no longer a directory
is skipped when the file is read and dropped the next time it is written. The file is written
whole, to a temporary name and renamed, and only when the list changes; two windows writing at
once each write the union of what they read and what they found, so a lost race costs nothing
the next probe does not put back. Nothing removes a folder that still exists: a folder used once
is checked from then on, which is what keeps its hooks right when the app moves.

A sandbox reads the file and never writes it.

## 2. The probe: which Claude Code runs where, and whether it reports

The probe answers, for every live session: is a Claude Code running in it, under which config
folder, and has it reported itself?

1. A fresh `ListSessionsDetailed`, on a connection of the probe's own with no Hello, as a hook
   asks. Each live session gives its shell's pid and its agent record, if a SessionStart hook
   left one. The window's own copy of the list will not do: no event carries a new agent
   record, so it may be stale.
2. One read of the process table (pid and parent pid of every process), and from it the
   processes descended from each shell.
3. Among a shell and its descendants, the first whose first argument's file name is `claude`
   (or `node` running one). Its environment is read (`sysctl` `KERN_PROCARGS2`, which the kernel
   allows for the user's own processes) and gives its folder: `CLAUDE_CONFIG_DIR` when set and
   not empty, else `~/.claude`. A process that cannot be read is skipped. macOS hides the
   environment of Apple's own programs; Claude Code is not one.

Each folder found joins the list of section 1. A session with a `claude` and no agent record is
a Claude Code that reports nothing: either its folder has no hooks, or it was started before
they were installed (Claude Code reads its hooks when it starts).

The probe runs off the UI thread: when the window connects to the daemon (each time the list of
sessions arrives), when the doctor's popover opens, when the hook row's action finishes, and at
one new moment: three seconds after a terminal's title changes while its session has no agent
record and has sent no `State` event, at most once a minute for that session. Claude Code sets
the title when it starts, so a `claude3` typed into a tab an hour after launch is found within
seconds, without a recurring timer; the three seconds let a Claude Code that has hooks report
first. A session the probe found silent that then sends a `State` event is taken off the row at
once, without a probe. A window does not probe in a sandbox.

## 3. The doctor's rows cover every folder

Three rows read a settings file: smooth wheel scrolling, follows the Arkos theme, and the title
hook. Each is now read in every folder of the list, and `doctor_panel::Probe` carries the list
in place of its one `settings` path.

- A row is good only when it is good in every folder. With the default folder alone, every text
  is what it is today.
- A row that needs a fix names the folders that need it, with the home folder written `~`:
  "missing in ~/.claude2, ~/.claude3: tabs running there show no Claude Code state".
- A row's button applies the fix to every folder that needs it, in the list's order. The notice
  says what was written where.
- A folder whose file cannot be parsed makes the row broken, with that file named and no button,
  as one folder does today. An action that fails in one folder still writes the others.

The title hook row has one more state. When every folder has the hooks and a probed session
still has a `claude` with no agent record, the row needs a fix and has no button:
"installed; restart claude in Film, Game for the tabs to show its state". The tabs are named as
the tab bar names them, and those of other windows are counted ("2 in other windows"). This is
the state the hand install of 2026-11-12 left the Film tab in.

Everything else the window does to the default folder it does to each folder of the list, by the
same rules: the startup `follow_theme_in_claude` (a stock or `auto` theme becomes `custom:arkos`,
any other choice is left for the row), and every write of Claude Code's Arkos theme file, which
goes to each folder's `themes`. The call sites are the uses of `claude_settings_path()` in
`main.rs` and `claude_theme::themes_dir()`.

The agent guide row is unchanged. Its promise, "other Claude config folders get it when a
session starts inside Arkos", is kept by the SessionStart hook, which every folder now has.

`arkos doctor` gains a section, "Claude Code config folders", that lists each folder with its
three answers, and each session the probe found reporting nothing, with its folder. It stays
read-only: it does not add to `claude-folders`. `arkos hooks install` is unchanged; it writes
the folder of the environment it is run in. The palette's "Install Claude Code hooks" writes
every folder that lacks them, or the default folder again when none does.

## 4. An agent is resumed under the folder it ran in

The SessionStart hook runs in its Claude Code's environment. When `CLAUDE_CONFIG_DIR` is set
and not empty there, the resume command in the agent record is

```
env CLAUDE_CONFIG_DIR=<folder> claude --resume <id>
```

and `claude --resume <id>` otherwise, as today. `env` because recovery types the command with
every word quoted, and a quoted `NAME=value` is not an assignment to a shell. The record's
`resume` is already a list of words, so the protocol, the manifest and the handover do not
change, and recovery, its budget and its "not resumed" line take the longer command as it is.
A record written by an older hook keeps the command it has.

## 5. What it costs

No recurring timer, thread or process (a title change waits three seconds, once). A probe is one request to the daemon, one read of the process table
and one `sysctl` for each `claude` found: well under a millisecond of work for a handful of
sessions, at startup, when the doctor is opened or used, and at most once a minute for a session
whose title changes while it reports nothing. The rows read one small JSON file more for each
extra folder. `claude-folders` is a few lines, written when a new folder is first seen.

## 6. What it does not do

- Show a working mark without hooks. Claude Code's title and the bell say something about what
  it is doing, but the hooks are the contract; this spec gets the hooks installed.
- Mark the tab itself, or raise a notice. The doctor's dot and row are where Arkos already says
  "tabs show no Claude Code state".
- Make hooks reach a Claude Code that is already running. It reads them at its start; the row
  says which tabs to restart it in.
- Find a folder no session inside Arkos has run under. The home folder is not searched for
  `.claude*`: a backup or an old copy would be written to.
- Put a `claude` of its own on the shells' `PATH` to pass the hooks with `--settings`. It would
  need no settings file at all, but both aliases above say `command claude`, a shell's own
  startup files reorder `PATH` after the daemon sets it, and a wrapper between the user and
  their `claude` is more than a tab mark is worth.
- Anything for an agent other than Claude Code.

## 7. Tests

Unit:

- the probe's pure parts: the descendants of a pid from a table of pid and parent pid; the first
  argument and the environment from a `KERN_PROCARGS2` buffer, a truncated one included; the
  folder from an environment (set, empty, unset)
- `claude-folders`: a missing folder is skipped and dropped on the next write, a folder is
  listed once, the default is never written, a sandbox does not write
- `doctor_panel::probe_all` over several folders: all good reads as today, one folder missing
  names it, the button's work is each folder that needs it, an unparseable file breaks the row
  and names the file, and the restart state names the tabs
- the resume command with and without a folder, a folder with a space in it, and that
  `typeable` accepts it

Against a real process: a child started as `claude` (the test binary itself under that name,
since macOS hides the environment of its own `sleep`) with `CLAUDE_CONFIG_DIR` set is found
under the test's own pid, with its folder.

By hand (`docs/development.md`, Status): with hooks in `~/.claude` only, run `claude3` in a tab.
A few seconds after its title appears the doctor's icon has a dot and the hook row names
`~/.claude3`. Press Install: the row asks for a restart in that tab. Restart `claude3` there and
give it a prompt: the tab's glyph breathes, the row is good. Then `pkill -9 -x arkosd`: the tab
comes back resumed, with its conversation, under `~/.claude3`.

## 8. Docs

`docs/guide.md` (the hooks section at the top, the doctor's rows, "After a restart"),
`docs/development.md` (Status), and the session awareness spec's hook table gets a pointer here.
