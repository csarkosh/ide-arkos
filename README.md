<h1 align="center">Arkos IDE</h1>

<p align="center">Run many Claude Code sessions side by side, and never lose one.</p>

<picture>
  <img alt="Seven Arkos IDE windows fanned out like postcards, each in its own theme. In front, the multi tasker: three Claude Code sessions side by side, each running its own subagents on an unrelated project. Behind, the blogger with a launch plan in the markdown editor, the data analyst with a weekly report and its chart, the hacker hardening a service in Matrix green, the indie game dev with a dungeon crawl in pixel-framed purple, the student with lecture notes on a ruled notebook page, and the web developer with the app's page in a web tab" src="docs/images/hero-dark.png" width="900">
</picture>

Arkos keeps track of where each of your Claude Code sessions stands, so with many running at
once you always know which one to look at next. They sit in one list, grouped by what they need
from you: Needing your input → Still at work → Quiet or done.

- **Durable:** sessions keep running through a frozen window or an update, and come back with
  their conversations after a restart, a shutdown or a macOS update.
- **Customizable:** ten distinct themes, some of them animated, and layouts you arrange by drag
  and drop: split a session, move any pane or tab, and start more Claude Code sessions in it.
- **All in one place:** a file browser, web pages and an editor for the documents Claude writes
  sit beside your sessions.
- **Reminders:** for a session you need to come back to tomorrow, or check on once a day,
  schedule it for any date and time down to the minute, once or every day, weekday or week; it
  waits under Scheduled and comes back as unread when it is due.

Built for Apple silicon, macOS 14 or later.

## Install

```sh
# Or install through brew
brew trust --cask csarkosh/tap/arkos-ide
brew install --cask csarkosh/tap/arkos-ide
```

## Who it's for

Seven ways people use it, each shot in one of the ten built-in themes. Every picture is a real
session, taken by [`scripts/shoot.sh`](docs/development.md#screenshots).

**The multi tasker** runs Claude Code on unrelated work at once: a rate limiter, an inbox and a
trip, each with its own subagents. The Inbox lists every session by what it needs: a question
first, then the answers not yet read, then the ones at work. Shot in Tokyo Rain, and in Navy, the theme Arkos starts in.

<picture>
  <img alt="An Arkos IDE window in Tokyo Rain, rain falling behind the text: the session list on the left, sorted into Needs you (a trip to book), Unread (a blog post and a household budget), Working (a rate limiter, a trip and an inbox), then one for later and one done; the rate limiter's Claude Code session shown at full size, its three background agents writing the limiter, wiring it into the server and documenting it" src="docs/images/multitasker-dark-inbox.png" width="760">
</picture>

**The web developer** keeps a feature branch in a worktree, with Claude Code on it and the app's
page in a web tab beside the code. Shot in Lavender Material.

<picture>
  <img alt="An Arkos IDE window in Lavender Material, its panes as rounded near-white cards on a lavender frame: the session list on the left, Claude Code in the middle adding rate limiting to a link shortener and updating its README, and a web tab on the right showing the app's page, served from localhost" src="docs/images/web-developer-dark-inbox.png" width="760">
</picture>

**The blogger** lives in markdown: a launch plan rendered in the editor, and Claude Code beside it
bringing the plan up to date. Shot in Peach Watercolor and Lavender Watercolor, which wash the
window in pigment and ink.

<picture>
  <img alt="An Arkos IDE window in Peach Watercolor, washed in the theme's pigments with ink lines round each pane: the session list on the left, and the session shown with a launch plan rendered in the markdown editor on top, its status table drawn with a wash over its header, and below it Claude Code updating the plan from the notes" src="docs/images/blogger-dark-inbox.png" width="620">
</picture>

**The indie game dev** builds a dungeon crawl: Claude Code fixes the combat rule a test caught and
designs the next room's enemy. Shot in Timo Purple, a pixel-art night, and Arabia Solar, the same
world by day.

<picture>
  <img alt="An Arkos IDE window in Timo Purple, deep violet with cyan accents and pixel art: each pane in a notched pixel frame, the focused one framed in cyan, and a castle under a crescent moon at the foot of the session list; Claude Code in the middle explaining a fixed combat rule and a new enemy, and the design notes on the right with their roster of enemies rendered as a table" src="docs/images/indie-game-dev-dark-inbox.png" width="760">
</picture>

**The student** keeps a course in one folder: the lecture notes on the left, and Claude Code on
the right explaining why the problem set failed. Shot in Composition Notebook, its terminal text
written in by hand.

<picture>
  <img alt="An Arkos IDE window in Composition Notebook: a marbled black-and-white cover over the title bar, the session list on a cream page bound by a wire spiral, Claude Code in the middle explaining a knapsack bug in handwritten type over its diff, and the lecture notes on dynamic programming rendered on a ruled page on the right, with a table and a code block" src="docs/images/student-dark-inbox.png" width="760">
</picture>

**The data analyst** owns the weekly numbers: Claude Code runs the script, and the signups report
fills in, chart and all. Shot in Material.

<picture>
  <img alt="An Arkos IDE window in Material, its panes as white rounded cards on a pale frame: the session list on the left, Claude Code in the middle summing up the week's signups by channel, and the weekly signups report rendered on the right with this week's table filled in and a light blue bar chart" src="docs/images/data-analyst-dark-inbox.png" width="760">
</picture>

**The hacker** has Claude Code audit a file-drop service and harden its sign-in, while a second
session reads the ssh log for the intruder. Shot in Matrix, with rune rain behind the panes.

<picture>
  <img alt="An Arkos IDE window in the Matrix theme, black with green text and rune rain falling behind the panes: the session list on the left, a session asking whether to rotate a signing key, and Claude Code at full size hardening a file-drop service's sign-in, its diff in red and green" src="docs/images/hacker-dark-inbox.png" width="760">
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
