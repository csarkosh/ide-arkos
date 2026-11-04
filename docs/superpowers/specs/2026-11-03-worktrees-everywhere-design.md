# Worktrees everywhere: the side worktrees of every repository under the open folder

Status: design, 2026-11-03. Asked for by Cyrus. Rewritten the same day around FSEvents after
Cyrus asked for the most efficient design that is also real-time; the first draft polled.

## Why

Cyrus often opens a folder of repositories (`~/Projects`) rather than one repository. The
Worktrees section of the sidebar shows nothing there today: it lists `git worktree list` of the
project folder only, and the project folder is not a repository. The worktrees he cares about
are the ones Claude Code makes beside a repository (`<repo>/.worktrees/<name>`, Claude Code's own
`<repo>/.claude/worktrees/<name>`, or a sibling folder), and the question he brings to the list is
which of them exist, where they are, and which have gone stale, so they can be pruned. Claude
often works in a worktree for a long time without committing, so staleness means the last time
any file in the worktree changed, not the last commit.

He only wants this if it stays light and fast, and he wants it real-time. Arkos runs on a
MacBook Air.

## Goals

1. The Worktrees section lists the side worktrees of every repository under the open folder: the
   repository the open folder is in, if any (found upward, as git does, so opening a worktree or
   a subfolder of a repository shows that repository's side worktrees as today), and every
   repository found below it.
2. Each row says the worktree's name, how long since any of its files changed, and where it is.
3. Changes show within about a second: a worktree added, removed or edited by Claude.
4. Idle cost is nothing: while nothing changes under the open folder, no walk, no stat and no
   git process runs for this list.
5. Claude can ask Arkos for the same list, with ages, to find stale worktrees to prune, in one
   fast command.

## Non-goals

- Creating worktrees from the pane. Cyrus never does; Claude always does. The "+" on the header
  goes, with the create prompt behind it.
- Arkos deciding or performing a prune on its own. Claude asks the user and removes with git.
- Grouping rows by repository, or a tree of repositories. The list is flat ("only a top-level
  list").
- Moving the sidebar's other polls (the once-a-second fingerprints of expanded folders and open
  files) onto FSEvents. The watcher is built so they can move later; moving them is a later
  change of its own.

## Anti-goals

- A refresh that starts one git process per repository on a timer. Measured on Cyrus's
  `~/Projects` (24 repositories): `git worktree list` plus the `git_read` filter-config check for
  each costs about 470 ms. That is the version not to build.
- A walk with no bound. Opening the home folder, or a folder holding a large monorepo, must not
  stall the sidebar or spin a core.
- A watcher that costs more than the polling it replaces: an event flood from a build writing
  `target/` or from git rewriting its index must be dropped before it does any work.

## The list

Flat, one row per side worktree, newest change first.

- A side worktree is any worktree `git worktree list` reports for a repository that is not that
  repository's main checkout (the first block). Main checkouts never appear, also when the open
  folder is a single repository (Cyrus chose "side worktrees only" over "every checkout").
- A missing (prunable) worktree is listed, marked missing, since it is exactly what pruning is
  for. A bare repository has no side worktree rows of its own.
- A repository found under the open folder that is itself a linked worktree of another
  repository (its `.git` is a file; Cyrus's `~/Projects/asset-generation-leak-scan` is one) is
  not a second repository: it appears once, as a side worktree of its main repository. Git
  reports the main repository from inside the linked worktree, so its side worktrees are listed
  even when the main repository is outside the open folder (my call). The rule is one row per
  worktree path.
- A worktree outside the open folder but belonging to a repository inside it is listed (a
  sibling worktree beside the project is still that project's).

A row:

- The worktree's folder name (`csv-grid`). Cyrus: "`csv-grid` is enough info". The branch
  follows, dimmed, only when it differs from the folder name; a detached worktree says
  `detached` there.
- Right-aligned, the time since any of its files changed, short: `4 min`, `7 h`, `2 d`,
  `3 w`, `5 mo`. Until the first scan lands it is blank, never a guess. It is live: an edit in
  the worktree makes it read `now` within about a second, and the row moves to the top.
- Below, dimmed and smaller, where it is: the path relative to the open folder
  (`ide-arkos/.worktrees/csv-grid`), or the full path with `~` for home when it is outside.
- A row whose last change is a week or more ago reads dimmer (my call).

The mockup Cyrus chose, option B, is
`docs/superpowers/brainstorm/16891-1793750631/content/rows.html`.

What stays as it is: expanding a row to see its changed files (`git status` only while
expanded), clicking a changed file to read it, the right-click menu (Open terminal here, Remove
worktree, Remove worktree, force), Ctrl+Shift+G for the keyboard, and the section's open and
closed state. Each of these runs git in the worktree's own repository, not the open folder.
With the main checkouts gone, every row's menu offers removal.

An empty list (no repository, or no side worktrees) shows the section's header with nothing
under it, or a single dim line saying no worktrees; the builder picks, matching how the
Explorer and Outline sections say they are empty.

## How it stays light and real-time: FSEvents

macOS's FSEvents is the kernel's own change feed, the one Finder and Spotlight use. One stream
over the open folder reports, within a chosen latency, each directory in which something
changed. It costs nothing while nothing changes. The design does every walk once at the start
and then only what the stream reports.

### The stream

- One `FSEventStream` over a set of paths: the open folder, plus each listed worktree outside
  it (see below). It uses the per-directory flavour (the default; `kFSEventStreamCreateFlagFileEvents`
  is the opt-in, and a directory path per change is enough), a latency of 1 second, so events
  within a second arrive together and a change shows about a second after it happened (a
  sustained burst keeps arriving, one batch a second), and `kFSEventStreamCreateFlagWatchRoot`,
  so a rename or removal of the open folder is reported too. Exclusion paths are no help
  (`FSEventStreamSetExclusionPaths` takes 8 at most, and a stream is always recursive), so the
  callback filter is the only defence; the cost is the events delivered, which is what the
  filter bounds.
- Paths are matched canonically. FSEvents reports real paths (`/private/var/...`, symlinks
  resolved), while the open folder and git's worktree paths may go through a symlink (Cyrus
  keeps symlinked folders), so the open folder and every path from git are canonicalised once
  when stored and every event path is matched against those. The row's displayed path is the
  canonical path made relative to the canonical open folder, so it reads the same whichever way
  the folder was opened.
- When the path set changes (a worktree outside the open folder appears or goes), the stream is
  recreated over the new set with `sinceWhen` set to the old stream's latest event id, so no
  event is lost in between.
- Bound with `#[link(name = "CoreServices", kind = "framework")]` and a thin extern block, as
  `av_player.rs` binds QuartzCore: the handful of functions `FSEventStreamCreate`,
  `FSEventStreamSetDispatchQueue`, `FSEventStreamStart`, `FSEventStreamStop`,
  `FSEventStreamInvalidate`, `FSEventStreamRelease`, `FSEventStreamGetLatestEventId`, with a
  C callback. Teardown is Stop, Invalidate, Release, in that order, and the callback's context
  (the channel sender) outlives the stream, released only after it. No watcher crate: the app
  binds Apple frameworks itself, and a crate would bring its own thread model.
- The callback runs on a dispatch queue of its own. It filters (below) and sends the surviving
  directory paths over a `futures::channel::mpsc` unbounded channel, the way `client.rs` hands
  daemon events to the UI; a task spawned on the window's context drains it and updates the
  model. Nothing in the callback touches gpui.
- The stream is created when the window opens its folder and released when the window closes
  or opens another folder. `arkos sandbox` and the shoot's windows get their own, like any
  window.

### Filtering, in the callback

A build writing thousands of files, or git rewriting its index on every `git status` (our own
expanded-row status included), floods the stream. Each event path is matched against the open
folder's rules before anything else happens:

- Under a skipped folder (`node_modules`, `target`, `build`, `dist`, `.build`, `Pods`,
  `DerivedData`, `vendor`, `__pycache__`, `.venv`, `venv`, and in a home folder `Library` and
  `Applications`; the builder may refine the list): dropped.
- Under a `.git` directory: dropped, except the `.git/worktrees` directory itself (a worktree
  added or removed) and its immediate children `.git/worktrees/<name>` (a worktree's `HEAD`,
  `gitdir` or `locked` changed), which are forwarded. Git also writes `index` and `index.lock`
  in `.git/worktrees/<name>` on every `git status`, our own expanded-row status included, so an
  event there is not proof of anything: the rule below checks before running git.
- A hidden cache folder (`.pytest_cache`, `.mypy_cache`, `.ruff_cache`, `.next`, `.nuxt`,
  `.gradle`, `.turbo`, `.cache`, `.parcel-cache`, `.svelte-kit`; the builder may refine):
  dropped, for the same reason as `target`.
- Anything else is forwarded as "this directory changed".

The filter is a pure function of the path and the open folder, tested on its own.

### What each event does

Each surviving directory path is resolved against the model, on the UI thread, in this order:

1. **`<repo>/.git/worktrees` itself:** the set changed. `git worktree list --porcelain -z` runs
   for that repository alone, off the UI thread, through `git_read` as today (with its
   filter-config refusal). The result replaces that repository's rows.
   **`<repo>/.git/worktrees/<name>`:** the `HEAD`, `gitdir` and `locked` entries are stat'ed
   (three stats) and compared with what the model stored at the last list; git runs only when
   one differs. A `git status` writing `index` changes none of them, so our own status runs and
   Claude's cost nothing. The builder adds `--no-optional-locks` to Arkos's own git reads too,
   so they write as little as possible.
2. **Inside a listed worktree's folder:** that worktree changed now. Its age becomes zero. No
   walk: the event is the evidence. One stat of the worktree's folder confirms it still exists;
   a worktree whose folder is gone is marked missing at once. Finder writes `.DS_Store` when a
   folder is merely looked at, so an event for a directory whose only newer entry is
   `.DS_Store` does not count (one `read_dir` of that directory, only when the age would
   otherwise reset from more than a minute ago; the builder may simplify).
3. **A directory not inside any known repository or worktree, within the discovery depth:** a
   repository may have appeared or gone. That directory is re-read (one `read_dir`): a new
   `.git` registers a repository and runs git for it; a known repository whose `.git` is gone is
   dropped with its rows.
4. **Inside a repository's own files, outside its `.git`:** dropped. Edits in a main checkout
   are not this list's business (nested repositories aside, below).

A batch flagged `kFSEventStreamEventFlagMustScanSubDirs` (the kernel dropped events) triggers
the full first-load pass again, off the UI thread, with the current rows kept until it lands.
A `kFSEventStreamEventFlagRootChanged` event (the open folder renamed, moved or removed)
re-resolves the open folder: if it still exists, the full pass; if it is gone, the list empties
and the section says the folder is gone, until the window opens another.

Rows are re-sorted newest first when ages change, but not under the pointer or the keyboard:
while a row is hovered or selected in the section the order holds and only the age text
changes in place, and the sort applies when the pointer leaves and nothing is selected (my
call). Expansion and selection follow the worktree's path, not its index. A re-sort is not
animated (an instant list change, as the file tree's are).

### The first load

Once per opened folder, off the UI thread, in this order, with rows showing as each stage lands:

1. **Find repositories.** A walk of the open folder that stops descending at each repository's
   top folder (a folder holding `.git`, directory or file), skips hidden folders, symlinks and
   the skipped folders above, has a depth cap (4 below the open folder, my call) and a budget of
   folders read (the builder's number, in the low thousands). Measured: on `~/Projects` the walk
   reads 29 folders in about 1 ms; on the home folder at depth 4, 85 folders in about 1 ms; at
   depth 8, 1,158 folders, 13 ms warm and 143 ms cold. The repository the open folder is inside,
   if any, is found upward first (`git rev-parse --show-toplevel`, or `.git` in the ancestors).
2. **List worktrees.** `git worktree list` for every repository found, in parallel on the
   background executor (a few at a time), about half a second in total on `~/Projects`. Rows
   appear as answers land, without ages.
3. **Ages.** For each side worktree, the newest modification time of any file in it, by a walk of
   its folders that skips `.git`, symlinks, any folder holding a `.git` of its own (another
   checkout, so one worktree's age never counts another's), the skipped folders and the hidden
   cache folders, and ignores `.DS_Store`. Other hidden folders are walked (my call): Claude's
   edits to `.github/`, `.claude/` or a dotfile are work in that worktree, while generated
   folders and caches change on every build or test run and can hold hundreds of thousands of
   files. Gitignored files count otherwise: no `git check-ignore`, which would be a process per
   worktree. Each walk has a budget of entries (the
   builder's number, in the tens of thousands); a worktree over it shows the newest change found
   so far with a marker meaning "at least this recent". Measured on Cyrus's eight side worktrees
   (a Python walk, so an upper bound for Rust): about 19 ms for all eight warm, about 210 ms the
   first time after the disk cache is cold; the largest, 3,449 files, took 7 ms warm and 85 ms
   cold. The folder's own modification time is no substitute: `light-themes`' newest file is a
   day newer than its folder.

After the first load, nothing walks again unless the stream asks for it (above). A worktree
that first appears through an event (step 1 or 3 of the event rules) gets its age walk then, once.

### The safety net

- A full first-load pass runs again when the stream reports dropped events, when the open
  folder itself changes, and once every 10 minutes regardless (my call: cheap, and it bounds
  how long a missed event can mislead). It never shows a blank list while it runs.
- A worktree outside the open folder (a sibling of the project, say) is added to the stream's
  path set, up to 16 such paths (my call); beyond that, those worktrees fall back to the 10
  minute pass and are marked in the model as "age may lag".
- A network volume (SMB, NFS) accepts a stream and then delivers nothing, so an error is no
  signal. The open folder's volume is checked up front with `statfs` (`MNT_LOCAL`); a folder on
  a non-local volume, or a stream that fails to create or start, gets the list built once and
  refreshed by the 10 minute pass only, and the section says the folder is not watched. The
  sidebar never fails to show the list for want of a watcher.

### Ages on screen

An age is computed at paint time from the stored "last changed" instant, so a row's text ticks
from `4 min` to `5 min` without any work beyond redrawing: the sidebar already redraws for its
other timers; the builder reuses the existing slow tick to notify every 10 seconds, or finer
when an age is under an hour, rather than adding a timer.

## The command for Claude

A new agent command beside `arkos open` and `arkos panes`, working name `arkos worktrees` (the
builder may pick the name; it must read naturally in `arkos guide`):

```
arkos worktrees [<folder>] [--stale <age>] [--json]
```

- It lists the same side worktrees, by the same rules, for `<folder>`, else the current folder;
  a folder inside a repository resolves to that repository, as the pane does, so Claude can run
  it from any working directory.
- It needs no window and no daemon: it runs the first-load pass itself (the three steps above,
  the same code, which therefore lives in modules free of gpui, as `worktrees.rs` and
  `explorer::git_read` are today) and prints. No stream: a command has no steady state. A cold
  run of about a quarter of a second is fine; the budgets are the app's.
- A repository `git_read` refuses, or whose git fails, is not skipped silently: the text output
  says so on stderr, one line per repository, and the JSON carries a `repositories` array with
  each repository's path and an `error` when it had one. Exit 0 when at least the walk
  succeeded, since a partial list is still an answer.
- Text output: one line per worktree, newest first: age, name, branch when it differs, path.
- `--stale <age>` keeps only those unchanged for at least that long (`7d`, `36h`, `2w`).
- `--json`: one object, `{ "folder", "worktrees": [ { "path", "name", "branch" | null,
  "repository", "changed" | null, "age_seconds" | null, "missing", "locked", "capped" } ],
  "repositories": [ { "path", "error"? } ] }`: `folder` is the folder listed; each worktree has
  its `branch` (null when detached), `repository` (the main checkout's path), `changed` (seconds
  since the Unix epoch of the newest file, null with no files to date), `age_seconds` (null
  likewise), `missing`, `locked` and `capped` (the walk stopped at its budget); each repository
  has its path and an `error` when it could not be listed.
- Exit 0 with an empty list when there are none; a folder that does not exist is an error with
  the agent CLI's usual exit code and wording.
- It only reports. Removal is Claude's, with git, after the user agrees (Cyrus: "the user has
  confirmed they're fine with the removal"). The guide says so in one line, and names
  `git worktree remove <path>` run in that worktree's repository.
- `docs/agent-guide.md` (which `arkos guide` prints) gains a short section; its test that the
  guide names every command and flag gains this one.
- No telemetry: windowless agent commands record nothing, as `arkos theme` does.

## Nested repositories

A repository inside another repository's files (not a submodule) is a nice-to-have, "less
important" in Cyrus's words. The first-load walk stops at the outer repository and skips
hidden folders, so it does not find them (that skip is discovery's only; registration by event
applies the filter's skip list alone); finding them means walking each repository's working tree, which cost 12 ms warm and
82 ms cold on `~/Projects` and has no upper bound in a monorepo. With FSEvents there is a cheap
path: the stream reports every directory that changes inside a repository, and an event for a
directory that turns out to hold a `.git` (one `read_dir`, done anyway for event rule 3) can
register a nested repository as it is created or touched. Build that: it finds a nested
repository the first time anything in it changes, costs nothing more, and is honest about not
finding ones that never change. Do not add a full walk for them.

## What goes

- The "+" on the Worktrees header, the create prompt (branch, path, the filter-driver second
  Enter), `worktrees::create`, `default_path`, `branch_exists` and their tests, and the guide's
  paragraph about creating one.
- The main checkout's row, and with it the rule that the main checkout cannot be removed (it is
  never a row).
- The worktree list's place on the 10 second slow tick in `explorer_watch` (and `slow_tick_plan`'s
  `worktrees` flag). The expanded rows' `git status` reload stays on that tick as today.

## Docs and checks

- `docs/guide.md`'s Worktrees paragraph says what the list now holds, what a row shows, that a
  folder of repositories works, and that it is live.
- `docs/development.md` Status gains rows a person must check at the screen: the list on
  `~/Projects`; an age reading `now` within a second or two of Claude editing a file in a
  worktree; a worktree removed by hand turning missing; a `cargo build` in a worktree not
  moving its age (the `target/` flood is dropped); opening the home folder without the sidebar
  stalling; Activity Monitor showing Arkos idle while nothing changes.
- Tests, written first and named as sentences, cover at least: the event filter (skipped
  folders, `.git` except `.git/worktrees`); the event rules against a model (which repository
  reruns git, which worktree's age resets, a new `.git` registering, a missing folder); the
  discovery walk stopping at repositories and honouring the skips, depth cap and budget; a
  linked-worktree repository folded under its main repository; main checkouts never listed; the
  newest-file walk and its budget; canonical matching (a temporary folder is under `/var`, which
  is really `/private/var`, so the tests meet this at once); the age text; the command's text and JSON output and
  `--stale` filter. The FSEvents binding itself is checked by one test that creates a stream on
  a temporary folder, writes a file, and sees the directory reported within a few seconds (and
  is skipped when the stream cannot be created, so the suite stays headless and green on a
  runner).

## Left to the builder

The module layout (the binding, the filter and rules, the walks, the model, the pane, the
command); the entry and folder budgets; the exact age text past a few months; the
empty-section wording; the command's final name; how the pane learns the ages (a separate
state beside today's `Worktrees` model or folded into it); the dispatch queue's QoS.

## Calls I made (Cyrus to check)

- Newest change first; a week or older reads dimmer.
- A missing worktree is listed and marked, and a folder deleted by hand turns missing on its
  event.
- Depth cap 4 for finding repositories; the stream's latency 1 second.
- A full pass every 10 minutes as a safety net, and on dropped events.
- Worktrees outside the open folder join the stream's path set, up to 16; one stream, recreated
  with `sinceWhen` when the set changes.
- A `.git/worktrees/<name>` event runs git only when `HEAD`, `gitdir` or `locked` changed, so
  git status runs (ours and Claude's) cost nothing.
- Hidden cache folders and `.DS_Store` never count toward an age; other hidden folders do.
- Rows hold their order under the pointer or the keyboard and re-sort after; no animation.
- A folder on a network volume is listed once and refreshed every 10 minutes, and says so.
- A renamed or removed open folder is re-resolved (`WatchRoot`).
- `arkos worktrees` names a repository it could not read instead of skipping it.
- `arkos worktrees` works without a window, from the given or current folder, without a stream.
- Opening a worktree or a repository's subfolder resolves upward to its repository, as today.
- The age walk includes hidden folders but skips generated ones and other checkouts.
- Nested repositories are found through their events, never by a full walk.
- Expand, the right-click menu and Ctrl+Shift+G stay as they are; the other sidebar polls do not
  move to FSEvents in this change.
