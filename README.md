<h1 align="center">Arkos IDE</h1>

<p align="center">Run many Claude Code sessions side by side, and never lose one.</p>

<picture>
  <img alt="Three Arkos IDE windows fanned out like postcards. In front, the multi tasker: three Claude Code sessions side by side, each running its own subagents on an unrelated project. Behind, the web developer with the app's page in a web tab, and the blogger with posts open in the markdown editor" src="docs/images/hero-dark.png" width="900">
</picture>

<p align="center">
<b>Panes and tabs</b> for your sessions · <b>a daemon</b> that keeps them alive · <b>a web pane</b> · <b>a file tree</b> with git worktrees · <b>a markdown editor</b> · <b>a video pane</b>
</p>

Arkos is a terminal layout manager built for Claude Code: a grid of terminal panes with tabs
that say which session is working and which is waiting on you, a web page in a pane beside the
code, a file tree and git worktree explorer, and a markdown editor for the
documents the sessions write. Every session lives in a separate daemon, so a frozen window, an
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

Four ways people use it, each in one of the built-in themes (navy, lavender-light,
peach-light, matrix); every picture is a real session, taken by [`scripts/shoot.sh`](docs/development.md#screenshots).

**The multi tasker** runs several Claude Code sessions on unrelated workstreams at once and
orchestrates the agents that orchestrate the subagents: here a rate limiter for a web service,
an inbox being triaged and a trip being planned, each with its own background agents, and
behind two of them a blog and a household budget whose sessions have finished. The tabs say
which session is working and which is waiting on you.

<picture>
  <img alt="One Arkos IDE window, no sidebar, three panes: a rate limiter being built by three background agents, an inbox triaged by four, and a trip being planned. The first two panes each have a second tab, a blog and a household budget, marked as waiting on you" src="docs/images/multitasker-dark.png" width="760">
</picture>

**The web developer** keeps a feature branch in a worktree, Claude Code on it and on the main
checkout, and the app's page in a web tab next to the code; the file tree and the worktree list
show what changed. It is shot in Lavender Light, with its watercolor effect.

<picture>
  <img alt="An Arkos IDE window in Lavender Light with its watercolor effect: the file tree and git worktrees on the left, two Claude Code tabs in the middle pane, one working and one waiting, and a web tab showing the app's page the whole height of the window on the right" src="docs/images/web-developer-dark.png" width="760">
</picture>

**The blogger** lives in markdown: a launch plan rendered in the editor, with its status table
as a grid, a Mermaid diagram, a checklist and a folded open-questions section, and a Claude Code
session beside it bringing the plan up to date from the review notes.

The same work stacked, one pane above the other: the file on top with the caret in it, since
the rendered view is also where you edit, and the session below. It is shot in Peach Light,
whose watercolor effect washes the window in the theme's pigments, inks the outlines of the
panes and the table, and draws the text as if in ink.

<picture>
  <img alt="An Arkos IDE window in Peach Light with its watercolor effect, two panes one above the other: a launch plan rendered in the markdown editor on top, its status table drawn as a grid with an apricot wash over its header and an ink outline; below it a Claude Code session updating the plan from the notes, the window washed in peach and rose with ink lines round each pane" src="docs/images/blogger-dark.png" width="620">
</picture>

**The hacker** works on security: Claude Code audits a small file-drop service and hardens its
sign-in, each fix with a regression test, while a second session fixes the path traversal it is
known to have, a third reads the server's ssh log for the address that guessed its way in, and a
shell shows the end of that log. It is shot in Matrix, black with soft phosphor green, whose
effect runs rune rain behind the panes, lights the text with a green glow, and scrambles each
new line for a moment before it settles, as on the audit's status line here.

<picture>
  <img alt="An Arkos IDE window in the Matrix theme, black with green text and rune rain falling behind the panes: the file tree on the left, Claude Code hardening a service's sign-in with its newest line still scrambling into place and a second tab fixing a path traversal, a session reading an ssh log for a break-in, and a shell showing the end of that log" src="docs/images/hacker-dark.png" width="760">
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
- **[Themes](docs/themes.md)**: the five built in, and how to make your own.
- **Agents**: run `arkos guide` in an Arkos terminal for how to show the user a video or a
  document and how to change the theme and settings.
- **[Developing](docs/development.md)**: building from source, tests, releases and status.

## License

GPL-3.0. Modified versions must be released under the same license. See [LICENSE](LICENSE).
