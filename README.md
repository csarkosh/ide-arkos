<h1 align="center">Arkos IDE</h1>

<p align="center">Run many Claude Code sessions side by side, and never lose one.</p>

<picture>
  <img alt="Seven Arkos IDE windows fanned out like postcards, each in its own theme. In front, the multi tasker: three Claude Code sessions side by side, each running its own subagents on an unrelated project. Behind, the blogger with a launch plan in the markdown editor, the data analyst with a weekly report and its chart, the hacker hardening a service in Matrix green, the indie game dev with a dungeon crawl in pixel-framed purple, the student with lecture notes on a ruled notebook page, and the web developer with the app's page in a web tab" src="docs/images/hero-dark.png" width="900">
</picture>

<p align="center">
<b>Panes and tabs</b> for your sessions · <b>a daemon</b> that keeps them alive · <b>a web pane</b> · <b>a file tree</b> and <b>a live worktree list</b> across every repo · <b>a markdown editor</b> · <b>a CSV and spreadsheet grid</b> · <b>a video pane</b>
</p>

Arkos is a terminal layout manager built for Claude Code: a grid of terminal panes with tabs
that say which session is working and which is waiting on you, a web page in a pane beside the
code, a file tree, a live list of every repository's worktrees with how long each has sat
untouched, and a markdown editor for the documents the sessions write. Every session lives in a separate daemon, so a frozen window, an
update or a restart never loses work: the window comes back and every tab is where you left
it, with Claude Code resumed. Written in Rust with GPUI, it stays light on memory and scrolls
smoothly with dozens of sessions open.

Built for Apple silicon, macOS 14 or later.

## Install

```sh
brew install --cask csarkosh/tap/arkos-ide
```

Homebrew 6 and later install from a tap like this one only once you trust it, and the full name
above trusts the cask. If you ran `brew tap csarkosh/tap` first, or use a command with the short
name, and Homebrew answers `Refusing to load cask csarkosh/tap/arkos-ide from untrusted tap
csarkosh/tap`, run `brew trust --cask csarkosh/tap/arkos-ide` and try again.

Releases are signed with a Developer ID and notarized, so the download opens as any app does,
and from then on the app updates itself (see [Updating](docs/guide.md#updating)). Run "Install Claude Code hooks" from
the command palette once, and the tabs follow what each session is doing.

## Who it's for

Seven ways people use it, each shot in one of the ten built-in themes. Every picture is a real
session, taken by [`scripts/shoot.sh`](docs/development.md#screenshots).

**The multi tasker** runs Claude Code on unrelated work at once: a rate limiter, an inbox and a
trip, each with its own subagents. Every tab says whether its session is working or waiting on
you. Shot in Tokyo Rain, and in Navy, the theme Arkos starts in.

<picture>
  <img alt="One Arkos IDE window, no sidebar, three panes: a rate limiter being built by three background agents, an inbox triaged by four, and a trip being planned. The first two panes each have a second tab, a blog and a household budget, marked as waiting on you" src="docs/images/multitasker-dark.png" width="760">
</picture>

**The web developer** keeps a feature branch in a worktree, with Claude Code on it and the app's
page in a web tab beside the code. Shot in Lavender Material.

<picture>
  <img alt="An Arkos IDE window in Lavender Material, its panes as rounded near-white cards on a lavender frame: the file tree and the Worktrees list on the left, two Claude Code tabs in the middle pane, one working and one waiting, and a web tab showing the app's page the whole height of the window on the right" src="docs/images/web-developer-dark.png" width="760">
</picture>

**The blogger** lives in markdown: a launch plan rendered in the editor, and Claude Code beside it
bringing the plan up to date. Shot in Peach Watercolor and Lavender Watercolor, which wash the
window in pigment and ink.

<picture>
  <img alt="An Arkos IDE window in Peach Watercolor, two panes one above the other: a launch plan rendered in the markdown editor on top, its status table drawn as a grid with a wash over its header and an ink outline; below it a Claude Code session updating the plan from the notes, the window washed in the theme's pigments with ink lines round each pane" src="docs/images/blogger-dark.png" width="620">
</picture>

**The hacker** has Claude Code audit a file-drop service and harden its sign-in, while a second
session reads the ssh log for the intruder. Shot in Matrix, with rune rain behind the panes.

<picture>
  <img alt="An Arkos IDE window in the Matrix theme, black with green text and rune rain falling behind the panes: the file tree on the left, Claude Code hardening a service's sign-in with its newest line still scrambling into place and a second tab fixing a path traversal, a session reading an ssh log for a break-in, and a shell showing the end of that log" src="docs/images/hacker-dark.png" width="760">
</picture>

**The student** keeps a course in one folder: the lecture notes on the left, and Claude Code on
the right explaining why the problem set failed. Shot in Composition Notebook, its terminal text
written in by hand.

<picture>
  <img alt="An Arkos IDE window in Composition Notebook: a marbled black-and-white cover over the title bar, the file tree on a cream page bound by a wire spiral, the lecture notes on dynamic programming rendered on a ruled page with a table and a code block, and Claude Code on the right explaining a knapsack bug in handwritten type" src="docs/images/student-dark.png" width="760">
</picture>

**The data analyst** owns the weekly numbers: Claude Code runs the script, and the signups report
fills in, chart and all. Shot in Material.

<picture>
  <img alt="An Arkos IDE window in Material, its panes as white rounded cards on a pale frame: a weekly signups report rendered on the left with this week's table by channel filled in and a light blue bar chart, Claude Code on the right summing up the numbers it computed, and the CSV export of signups by week and channel below it" src="docs/images/data-analyst-dark.png" width="760">
</picture>

**The indie game dev** builds a dungeon crawl: Claude Code fixes the combat rule a test caught and
designs the next room's enemy. Shot in Timo Purple, a pixel-art night, and Arabia Solar, the same
world by day.

<picture>
  <img alt="An Arkos IDE window in Timo Purple, deep violet with cyan accents and pixel art: each pane in a notched pixel frame on a dark checkered ground, the focused one framed in cyan, stone tab bars with a small lantern on each active tab, stars and a cloud in the title bar and a castle under a crescent moon at the foot of the file tree; Claude Code on the left adding an enemy to a dungeon crawl, the design notes with their roster table rendered on the right, and a shell showing the failing test below them" src="docs/images/indie-game-dev-dark.png" width="760">
</picture>

## Privacy

Arkos can send anonymous usage reports and crash reports. Usage reports stay on your Mac until
you turn them on (Settings > Privacy, or `arkos telemetry on`). A crash report is sent only when
you press Send in the dialog that shows it, or when you have chosen Always. Once a day the update
check goes through the project's own server so installs can be counted, with nothing that
identifies you or your Mac. [What Arkos sends](docs/telemetry.md) lists every key.

## Learn more

- **[The guide](docs/guide.md)**: sessions and recovery, the sidebar, the markdown editor, the
  browser pane, the video pane, keys, settings and updates.
- **[Themes](docs/themes.md)**: the ten built in, and how to make your own.
- **Agents**: run `arkos guide` in an Arkos terminal for how to show the user a video or a
  document and how to change the theme and settings.
- **[Developing](docs/development.md)**: building from source, tests, releases and status.

[<img src="https://donate.csarko.sh/icon.svg" alt="" height="16"> Buy me tokens](https://donate.csarko.sh/arkos)

## License

GPL-3.0. Modified versions must be released under the same license. See [LICENSE](LICENSE).
