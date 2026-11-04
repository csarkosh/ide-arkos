# Worktrees everywhere: the side worktrees of every repository under the open folder

Status: design, 2026-11-03. Asked for by Cyrus.

## Why

Cyrus often opens a folder of repositories (`~/Projects`) rather than one repository. The
Worktrees section of the sidebar shows nothing there today: it lists `git worktree list` of the
project folder only, and the project folder is not a repository. The worktrees he cares about
are the ones Claude Code makes beside a repository (`<repo>/.worktrees/<name>`, Claude Code's own
`<repo>/.claude/worktrees/<name>`, or a sibling folder), and the question he brings to the list is
which of them exist, where they are, and which have gone stale, so they can be pruned. Claude
often works in a worktree for a long time without committing, so staleness means the last time
any file in the worktree changed, not the last commit.

He only wants this if it stays light and fast. The sidebar refreshes the list every 10 seconds
(`explorer_watch::STATUS_EVERY`), and Arkos runs on a MacBook Air.

## Goals

1. The Worktrees section lists the side worktrees of every repository under the open folder: the
   repository the open folder is in, if any (found upward, as git does, so opening a worktree or
   a subfolder of a repository shows that repository's side worktrees as today), and every
   repository found below it.
2. Each row says the worktree's name, how long since any of its files changed, and where it is.
3. Claude can ask Arkos for the same list, with ages, to find stale worktrees to prune, in one
   fast command.
4. Light: the steady-state refresh starts no git process when nothing changed.

## Non-goals

- Creating worktrees from the pane. Cyrus never does; Claude always does. The "+" on the header
  goes, with the create prompt behind it.
- Arkos deciding or performing a prune on its own. Claude asks the user and removes with git.
- Grouping rows by repository, or a tree of repositories. The list is flat ("only a top-level
  list").

## Anti-goals

- A refresh that starts one git process per repository every 10 seconds. Measured on Cyrus's
  `~/Projects` (24 repositories): `git worktree list` plus the `git_read` filter-config check for
  each costs about 470 ms per refresh. That is the version not to build.
- A walk with no bound. Opening the home folder, or a folder holding a large monorepo, must not
  stall the sidebar or spin a core.

## The list

Flat, one row per side worktree, newest change first.

- A side worktree is any worktree `git worktree list` reports for a repository that is not that
  repository's main checkout (the first block). Main checkouts never appear, also when the open
  folder is a single repository (Cyrus chose "side worktrees only" over "every checkout").
- Bare and missing (prunable) worktrees: a missing one is listed, marked missing, since it is
  exactly what pruning is for; a bare repository has no side worktree rows of its own.
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
  `3 w`, `5 mo`. Until the first scan lands it is blank, never a guess.
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

## "Last changed"

The newest modification time of any file in the worktree, found by a walk of its folders that
skips `.git`, symlinks, any folder holding a `.git` of its own (another checkout, so one
worktree's age never counts another's), and the generated folders the repository walk skips
(`node_modules`, `target` and the rest of that list). Hidden folders are walked (my call):
Claude's edits to `.github/`, `.claude/` or a dotfile are work in that worktree, while generated
folders change on every build and can hold hundreds of thousands of files. Gitignored files
count otherwise. A worktree's age is the time since that newest file.

Measured on Cyrus's eight side worktrees (a Python walk, so an upper bound for Rust): about
19 ms for all eight warm, about 210 ms the first time after the disk cache is cold; the largest,
3,449 files, took 7 ms warm and 85 ms cold. The folder's own modification time is no substitute:
`light-themes`' newest file is a day newer than its folder.

So:

- It runs off the UI thread, never on the 10 second timer. My call: once when the list first
  lands, then every 60 seconds, and for one worktree when the explorer's watcher or the
  expanded-worktree status sees a change in it. Ages are shown in minutes at best, so a minute
  of lag is invisible.
- A walk has a budget per worktree (a number of entries, the builder's choice, in the tens of
  thousands). A worktree over it shows the newest change found so far with a `+` or similar
  marker meaning "at least this recent", rather than walking on.

## Finding the repositories

A walk of the open folder that:

- Stops descending at each repository's top folder (a folder holding `.git`, as a directory or
  a file). The open folder itself is walked into even when it is a repository.
- Skips hidden folders (names starting with `.`), symlinks, and generated folders:
  `node_modules`, `target`, `build`, `dist`, `.build`, `Pods`, `DerivedData`, `vendor`,
  `__pycache__`, `.venv`, `venv`, and in a home folder `Library` and `Applications`. The builder
  may refine the list.
- Has a depth cap (4 below the open folder, my call) and a budget of folders read. Measured: on
  `~/Projects` the walk reads 29 folders in about 1 ms; on the home folder at depth 4, 85 folders
  in about 1 ms; at depth 8, 1,158 folders, 13 ms warm and 143 ms cold.

Nested repositories (a repository inside another repository's files, not a submodule) are a
nice-to-have, "less important" in Cyrus's words. The walk above does not find them, since it
stops at the outer repository. Finding them means walking each repository's working tree, which
cost 12 ms warm and 82 ms cold on `~/Projects` and has no upper bound in a monorepo. Build it
only if it stays inside the same budgets and runs on the slow (60 second) timer with the
staleness walk, sharing that walk, since the staleness walk already visits every folder of each
worktree; otherwise leave it out and say so.

## Listing worktrees without a git process every 10 seconds

Git stays the source of truth for a repository's worktrees (`git worktree list --porcelain -z`
through `git_read`, as today, with its filter-config refusal), but it runs only when that
repository's worktrees can have changed:

- Per repository, a cheap fingerprint from file metadata only: the modification times of
  `.git`, `.git/worktrees`, and each `.git/worktrees/<name>` (a branch switch in a worktree
  rewrites its `HEAD` there; adding or removing a worktree changes `.git/worktrees`). A
  repository whose fingerprint is unchanged keeps its last list; only a changed one runs git.
- The 10 second tick walks for repositories (about 1 ms) and compares fingerprints (a few stat
  calls each). On Cyrus's folder that is well under a few milliseconds and starts no process
  when nothing changed.
- Each tick also checks that every listed worktree's folder still exists (one stat each), so a
  worktree deleted by hand, which touches nothing under `.git/worktrees`, shows as missing within
  10 seconds without waiting for git (my call).
- The first load runs git for every repository, off the UI thread, and the list fills as the
  answers land (or all at once; the builder's choice). About half a second in total on
  `~/Projects`.

Reading `.git/worktrees/*/gitdir` directly instead of running git is about 0.4 ms for all 24
repositories and was considered; it means re-implementing git's format (relative worktree paths
in newer git, `locked` and `prunable`), so git on change is the design. The builder may use the
files as a fallback signal but not as the list.

The open folder no longer needs to be a repository for the section to work: the `not_git`
gating of the slow tick (`explorer_watch::slow_tick_plan`) applies to the file tree's status
only, not to the worktree list.

## The command for Claude

A new agent command beside `arkos open` and `arkos panes`, working name `arkos worktrees` (the
builder may pick the name; it must read naturally in `arkos guide`):

```
arkos worktrees [<folder>] [--stale <age>] [--json]
```

- It lists the same side worktrees, by the same rules, for `<folder>`, else the current folder;
  a folder inside a repository resolves to that repository, as the pane does, so Claude can run
  it from any working directory.
  It needs no window and no daemon: it walks and asks git itself, so it works anywhere.
- Text output: one line per worktree, newest first: age, name, branch when it differs, path.
- `--stale <age>` keeps only those unchanged for at least that long (`7d`, `36h`, `2w`).
- `--json`: an array of objects with `path`, `name`, `branch` (or null when detached),
  `repository` (the main checkout's path), `changed` (seconds since the Unix epoch of the newest
  file), `age_seconds`, `missing`, `locked`, and a flag for a budget-capped walk.
- Exit 0 with an empty list when there are none; a folder that does not exist is an error with
  the agent CLI's usual exit code and wording.
- It only reports. Removal is Claude's, with git, after the user agrees (Cyrus: "the user has
  confirmed they're fine with the removal"). The guide says so in one line, and names
  `git worktree remove <path>` run in that worktree's repository.
- `docs/agent-guide.md` (which `arkos guide` prints) gains a short section; its test that the
  guide names every command and flag gains this one.
- Telemetry: the command is counted like the other agent commands (`AgentCommand`,
  `telemetry/events.toml`, `docs/telemetry.md` regenerated). No paths or names are sent.

## What goes

- The "+" on the Worktrees header, the create prompt (branch, path, the filter-driver second
  Enter), `worktrees::create`, `default_path`, `branch_exists` and their tests, and the guide's
  paragraph about creating one.
- The main checkout's row, and with it the rule that the main checkout cannot be removed (it is
  never a row).

## Docs and checks

- `docs/guide.md`'s Worktrees paragraph says what the list now holds, what a row shows, and
  that a folder of repositories works.
- `docs/development.md` Status gains rows a person must check at the screen: the list on
  `~/Projects`, ages that move when Claude edits a file in a worktree, and opening the home
  folder without the sidebar stalling.
- Tests, written first and named as sentences, cover at least: the walk stopping at
  repositories and honouring the skips, depth cap and budget; a linked-worktree repository
  folded under its main repository; main checkouts never listed; the fingerprint deciding when
  git runs; the newest-file walk and its budget; the age text; the command's text and JSON
  output and `--stale` filter.

## Left to the builder

The module layout; the entry budgets; whether the list fills progressively on first load; the
exact age text past a few months; the empty-section wording; the command's final name; how the
pane learns the staleness results (a separate state beside today's `Worktrees` model or folded
into it).

## Calls I made (Cyrus to check)

- Newest change first; a week or older reads dimmer.
- A missing worktree is listed and marked, since pruning is what it is for.
- Staleness refreshes every 60 seconds, not every 10.
- Depth cap 4 for finding repositories.
- `arkos worktrees` works without a window, from the given or current folder.
- Expand, the right-click menu and Ctrl+Shift+G stay as they are.
- Opening a worktree or a repository's subfolder resolves upward to its repository, as today.
- The age walk includes hidden folders but skips generated ones and other checkouts.
- A worktree folder deleted by hand shows as missing within one 10 second tick.
