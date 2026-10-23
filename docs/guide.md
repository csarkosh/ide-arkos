# Arkos IDE guide

How Arkos IDE works, section by section. The [README](../README.md) has the overview and the
install; [development.md](development.md) is for building, testing and releasing it.

## Session awareness

Claude Code tells the IDE what it is doing through its hooks. Run "Install Claude Code hooks"
from the command palette (or `arkos hooks install`), or the doctor's "Claude Code title hook"
row, which merges six entries into `~/.claude/settings.json` after writing a `.bak`, then start
or restart `claude` in a tab:

| Claude Code event | The tab |
|---|---|
| a prompt is sent, a tool runs | a slow breathing glyph: working |
| Claude stops, or asks for permission or input | a warm dot: waiting on you |
| you click the tab or type into it | the dot clears |
| the shell exits | a muted title, with the exit code |

A terminal bell marks a tab waiting too, so a `claude` set to ring the bell works without hooks.
`arkos notify` is the hook side: it reads `ARKOS_SESSION_ID`, which every shell the daemon
starts carries, and exits 0 whatever happens, so a `claude` run outside Arkos IDE is unaffected.
`arkos hooks print` prints the entries for a hand paste. The entries name this binary by its
launch path; after moving it, install again. A Claude Code killed by a shutdown (its SessionEnd
hook says `other`) keeps its record, so the recovery after the restart resumes it.

## After a restart

A shutdown or restart ends every process; what becomes automatic is the recovery. The daemon
writes each session's history to `~/Library/Application Support/arkos/sessions/<id>.replay`
a couple of seconds after output stops (in full on SIGTERM, which a clean shutdown sends) and a
manifest of the sessions (folder, shell, the Claude Code session id from its SessionStart hook)
and of the project folders with a window open. "Start at login" from the palette installs
`~/Library/LaunchAgents/sh.csarko.arkosd.plist`, which runs `arkosd --recover` once at login:
it recreates each shell in its folder (the home folder when that is gone) with the old output
under a dim "resumed after restart" line, types `claude --resume <id>` into the shells that had
a Claude Code session, and opens a window for each project that had one; each tab says
"(resumed after restart)" until you type into it. A window you quit yourself stays closed.

What does not come back: whatever was executing at that moment (a half-finished tool call, a
running build, shell variables, background jobs). Claude Code's transcript records the
unfinished tool, and the agent continues from there once asked; recovery only resumes, it never
sends a message. Everything stays on this Mac. `[recovery] start_at_login` in the config pins
it on or off; "ask", the default, shows a one-line notice at launch until you decide. While the
login item is installed it follows the binary and the `[daemon]` settings: a moved binary or an
edited value rewrites it. The window never unloads the login item, since after a recovery the
running daemon is the login item's own process: a rewritten file is loaded at the next login, and
turning it off deletes the file but does not stop a daemon already running.

## Sidebar and viewer

Cmd+B shows the sidebar: a file tree rooted at the folder, read one directory at a time as you
expand it, with `.git` hidden and anything `.gitignore` ignores left out (the palette's "Show
ignored files" lists them dimmed). It follows the file system within a second. A single click
opens a file in a preview tab in the focused pane, which the next single click replaces; a
double click, Enter in the tree, or dragging the tab pins it. Right click: Reveal in Finder,
Copy path, Copy relative path, Open in default app. Drag a file onto a terminal to paste its
path. Shift+Cmd+E puts the keyboard in the tree: arrows move and expand, Enter opens, Space
previews, Escape goes back to the pane.

Under the Explorer, Worktrees lists `git worktree list`: each worktree's folder, branch and, once
expanded, its changed files in the status colours with a count in the row (a collapsed worktree
never runs `git status`). Click a changed file to read it in the viewer. Right click a worktree:
Open terminal here, Remove worktree (refused while it has changes), Remove worktree, force. The
"+" on the header creates one: a branch name, then the path (prefilled beside the project);
an existing branch is checked out, a new one created. If the repository's own config sets
filter drivers (`git lfs install --local`, for one), creating a worktree names them and asks for
a second Enter, since the checkout runs them; a filter config with a driver name it cannot read
safely creates nothing. The main checkout is first and cannot be removed. Ctrl+Shift+G puts the
keyboard in the list. The tree's rows carry git status in the theme's pastels (added, modified,
untracked) and a folder with changes inside carries a dot.

The viewer is read-only: syntax colours from your theme's `[viewer]` section through syntect's
TextMate grammars, line numbers, smooth scrolling, Cmd+F to find, and a reload within a second
of the file changing on disk. Files over `[viewer] highlight_limit_bytes` (5 MB by default) open
plain, and above 64 MB a notice; binaries show a notice. The text preset applies to viewers as to
terminals, and Cmd+= and Cmd+- resize the focused one. A file in a git repository shows its
changes against the worktree's HEAD in the gutter: an added line a green bar, a changed line an
amber one, a deletion a red mark at the top of the line that follows; Shift+Cmd+D (or the "inline
diff" chip) shows the deleted lines in red between the current ones.

Menus, the palette, notices, find bars and the drop preview ease in over 120 ms and out over
90 ms; Reduce Motion turns that off. The drop preview glides between zones, the focus glow
cross-fades as focus moves and breathes slowly while the window is active (six seconds a
breath, a little brighter and wider at the peak; Reduce Motion rests it), a section's rows fade
in as it opens, and Cmd+B slides the sidebar
over 150 ms with the panes following. Something on its way out is only drawn: it takes no
keys, focus or clicks.

## Editing markdown

A `.md` file opens as a document you can edit. Two modes: Rendered shows headings, lists, task
lists, quotes, code blocks, links and images as they read; Raw shows the file's own text with
Arkos's light markdown colouring (not a TextMate grammar, so a 5 MB file stays quick). The pill at
the top right of the pane, or Shift+Cmd+M, switches between them, and each tab remembers its mode
across a relaunch.

In Rendered mode one block is active at a time. Click a paragraph, heading or list and that block
turns into its raw text with the caret where you clicked; type and the file changes. Click
another block, or press the arrows past the end of this one, and the active block moves. When the
pane loses focus the active block renders again.

Arkos saves a second after you stop typing, and Cmd+S saves at once. The tab shows " •" until the
file is written. Line endings (`\n` or `\r\n`) and a byte order mark are kept as the file had
them. A file with both kinds of line ending, or one that is not valid UTF-8, opens read-only with
a banner saying so, since a save could not keep it as it was.

If the file changes on disk while you have unsaved edits, a banner says "The file changed on disk"
with Reload (take the disk's version) and Keep mine (your text wins and the next save overwrites
the disk). Cmd+S means Keep mine. With nothing unsaved the file reloads on its own. A failed save
shows its reason in a banner with Retry.

Every other file type stays a read-only viewer.

### Tables

A table renders as a grid: a bold header row, rules between the cells, the column alignment from
the delimiter row. A click lands in the cell, with the caret where you clicked, and the block
turns into its raw text. In that block, Tab moves to the next cell and Shift+Tab to the previous
one, Up and Down move to the cell above and below, and Tab in the last cell adds a row. Where a
move has nowhere to go, the key does what it does in any block. These keys work in Rendered mode
only; Raw mode is a plain text editor.

When you leave an edited table, or press Escape in it, Arkos re-aligns its pipes so the source
reads as a grid too. One undo takes the alignment back, and an undone table is not re-aligned
again. Re-alignment never drops a cell: a row longer than the header keeps its extra cells after
the aligned ones. A table ends at its first blank line, so the link reference definitions below it
are never touched. In the rare case that a table you deleted is replaced by another on the same
start line, the new one can be re-aligned as well.

A table wider than the view wraps by default: the columns shrink and the cells wrap. Pick
"Scroll" under "Wide tables" in Settings, or run `arkos settings set viewer.wide_tables scroll`,
and the columns keep their widths and the table scrolls sideways (Shift+wheel, or a horizontal
swipe). A table over 500 rows shows as source.

### Images and media

A remote image (`https://`) loads on its own: the block shows the alt text and "loading" until
the file arrives. Arkos fetches it through `curl` into its cache, at most 10 MB per image, and
keeps it 30 days. A reply that is not an image (a Wi-Fi sign-in page, say) is not kept, and the
image is fetched again the next time the file opens. There is no switch for this. Badge rows wrap
in a narrow pane, and an image wider than the view is scaled to the view.

An HTML block that is one `<img>` tag, alone or inside a `<p>`, `<div>` or `<a>`, renders as the
image too, local or remote. Its `width` is honoured up to the view's width, and it is centred
when the wrapper has `align="center"`. Inside an `<a href>` it follows the link, and its `alt`
shows when the image cannot be drawn.

A plain click on an image, a video's poster or a badge places the caret in the block. Cmd+click
opens it: an image in Preview, or the link for a linked image. A video (`.mp4`, `.mov` and the
like) shows a poster frame made by Quick Look, with "loading" first, and Cmd+click opens it in a
video tab beside the document; a file Arkos cannot play shows why, with a button for QuickTime
Player. Audio shows as a one-line box and plays in QuickTime Player. A local `.webm`
is shown in Finder instead, and remote media are links. Every local open resolves symlinks and
checks the file type first.

### Diagrams

A code block fenced as `mermaid` renders as a diagram. Arkos draws it with its bundled mermaid
in headless Google Chrome, which must be in `/Applications` or `~/Applications`. The page is
local, nothing is fetched, and Chrome runs with a throwaway profile, so your own Chrome is left
alone. The block shows its source until the diagram is ready (about two seconds) and again if you
edit it, then the new diagram replaces it. Results are cached. Without Chrome the source shows
with a message saying so, and so does a diagram over 8192 CSS px on a side or 64 million pixels.

### Collapsible sections

A `<details>` block, with blank lines inside if you like, renders as its `<summary>` (or
"Details") behind a triangle. A click on the summary opens and closes it. `<details open>` starts
open; once you open or close a section, your choice wins, in either direction. The choice is kept
per file in the workspace file, keyed by the summary and its place among the file's sections. It
survives a relaunch, is shared by every tab on the file, and is never written to the markdown.

The content is read-only: a click in it turns the whole block into raw text. A Mermaid block in a
section shows as code, a table as a plain grid without the editing keys, and a task box does not
toggle. A section inside another shows as source. A `</details>` inside inline code or a code
block does not close a section. HTML after the closing tag shows as dimmed source, and so does
every other HTML.

## Browser pane

A browser tab is a placeholder in a pane with a real Chrome window sitting on top of it. Arkos
does not render the web: it moves and sizes the Chrome window through the Accessibility API so
that it follows the placeholder as you split, drag a handle, toggle the sidebar or move the
window, hides it when the tab is not the active one (or arkos is minimized or hidden), and
raises it when you click the pane.

A window gets docked in one of four ways. While a Claude Code session is working, the next Chrome
window it opens lands in a browser tab on its own. The palette's "Dock a Chrome window…" lists
Chrome's windows and docks the one you pick into the focused pane. "New browser tab…" (Cmd+Shift+B) asks for a URL
and opens it in a new Chrome window that docks into the tab; "New single-page browser tab…"
does the same in Chrome's app mode (no tab strip, no address bar), as does `app_mode` in the
config. Or drag any Chrome window by its title bar into a pane and drop it.

It is released four ways: close the tab (the window stays where it is; arkos never closes it),
the palette's "Undock Chrome window", drag the tab out of the arkos window and drop it there, or
drag the window's title bar more than 40 px away from the placeholder. Dropping the tab in
another pane moves it, still docked. Chrome's own tab dragging is left alone.

Moving another app's window needs the Accessibility permission. Arkos asks once, when the first
browser pane needs it; until it is granted the pane explains what is missing and links to System
Settings. Revoking it later brings the explanation back within a second. The grant is Arkos IDE's,
and holds for every window, including one the daemon reopens after an update. A shell in a tab is
its own app to macOS: a command in it that reaches a protected folder (Documents, Desktop,
Downloads) makes macOS ask for that shell (`zsh`), once.

A pane holding a browser tab shows a slim 14 px tab bar: the pane number and one dot per tab, no
titles, since Chrome shows the page's own title. Dragging a tab still drops onto it. To get the
vertical space back, a tip (once per run) suggests right-clicking Chrome's tab strip and choosing
Show Tabs Vertically.

Only the URL and the app-mode flag are saved. After a relaunch a browser tab waits for a window;
arkos never tries to find the old one. A hidden tab's window waits off-screen, and quitting puts
it back; after a crash or a kill it stays there, and Chrome's Window > Zoom (or quitting and
reopening Chrome) brings it back on screen.

## Video pane

A video (`.mp4`, `.mov`, `.m4v`) opens as a tab of its own, drawn by Arkos in the pane like a
terminal or a document: it moves between panes, splits, reorders, renames, closes and comes back
with Shift+Cmd+T, and a relaunch brings it back paused on the frame it showed, with its loop.
There are four ways in: a click on the file in the sidebar previews it (paused) and a double click
pins it, as for any file; Cmd+click on a video's poster in a markdown file opens it in the pane
to the right of the document, or splits one off; the palette's "Open file in pane…" asks for a
path and opens it in the focused pane; and a coding agent can put it beside itself with
`arkos open` (see Agents, under Config).

Under the picture a strip is always shown: play and pause, the time as `m:ss.f / m:ss.f`, a
scrubber, the loop toggle and the mute toggle, and a status word at the right. A click on the
scrubber seeks; a drag scrubs through the frames and resumes playback afterwards if it was
playing; a Shift+drag selects a range, which L (or the loop toggle) then loops. The loop toggle on
without a range loops the whole file. With the pane focused (click it first) the keys are Space to
play or pause, Left and Right for 5 seconds, Shift+Left and Shift+Right for 1 second, comma and
period for one frame back or forward, Home or 0 for the start, L to loop, M to mute and R to
reload. Every one can be rebound; the Keybindings window lists them under "Video". Terminals never
see these keys.

A video plays only while it is the active tab of its pane and the window is not hidden or
minimized: switching to another tab pauses it and switching back resumes it. Moving to another app
does not pause it. While a video plays the display does not sleep, as with QuickTime Player.

The tab follows its file. When the file changes on disk and then stays the same for a second, the
tab reloads it, within about two seconds, holding the position and whether it was playing; the
strip says "reloading" and then "new cut" for three seconds. A file still being written (an
`ffmpeg` render, say) is never shown half way. A new file that cannot be played yet leaves the old
one showing with "new file cannot be played yet", and the next change tries again; a deleted
file leaves the last frame dimmed with "waiting for file" until it comes back. R reloads at once,
for a tool that keeps the modification time.

A file Arkos cannot play shows the reason over the picture with a button, "Open in QuickTime
Player". Audio still opens in QuickTime Player, and a `.webm` is shown in Finder.

## Keyboard

The VS Code map, in a terminal-friendly form.

| Keys | Action |
| --- | --- |
| Cmd+1 | focus pane 1 |
| Cmd+2 | focus pane 2 |
| Cmd+3 | focus pane 3 |
| Cmd+4 | focus pane 4 |
| Cmd+5 | focus pane 5 |
| Cmd+6 | focus pane 6 |
| Cmd+7 | focus pane 7 |
| Cmd+8 | focus pane 8 |
| Cmd+9 | focus pane 9 |
| Cmd+K then Cmd+Left | focus left |
| Cmd+K then Cmd+Right | focus right |
| Cmd+K then Cmd+Up | focus up |
| Cmd+K then Cmd+Down | focus down |
| Cmd+\ | split right |
| Cmd+K then Cmd+\ | split down |
| Cmd+K then Cmd+M | maximize pane |
| Ctrl+Cmd+Left | move tab to previous pane |
| Ctrl+Cmd+Right | move tab to next pane |
| Option+Cmd+Right | next tab |
| Option+Cmd+Left | previous tab |
| Shift+Cmd+] | next tab |
| Shift+Cmd+[ | previous tab |
| Ctrl+Tab | most recently used tab |
| Cmd+W | close tab |
| Shift+Cmd+T | reopen closed tab |
| Cmd+K then W | close all in pane |
| Cmd+K then Shift+Cmd+Left | move tab left |
| Cmd+K then Shift+Cmd+Right | move tab right |
| Ctrl+Shift+` | new terminal tab |
| Cmd+Shift+B | new browser tab: asks for a URL, opens it in a Chrome window docked into the tab |
| Cmd+K | clear to start: history and screen go, the prompt line moves to the top, as in Terminal.app |
| Cmd+= | font bigger |
| Cmd+- | font smaller |
| Cmd+F | find in the terminal or viewer |
| Cmd+C | copy the selected terminal text, with the lines a program wrapped to the pane's width joined |
| Shift+Cmd+C | copy as shown: the selected terminal text with its rows as they are on screen, for code or a table |
| Cmd+V | paste |
| Cmd+B | toggle the sidebar |
| Shift+Cmd+E | focus the file tree |
| Ctrl+Shift+G | focus the worktree list |
| Shift+Cmd+D | inline diff in the viewer |
| Shift+Cmd+M | toggle markdown view: rendered or raw |
| Cmd+S | save file (a markdown file also saves itself a second after you stop typing) |
| Arrows | in a markdown file: move the caret; with Option by word, with Cmd to the line or document edge; with Shift they select |
| Delete, Option+Delete, Cmd+Delete, Fn+Delete | in a markdown file: delete a character, a word, to the line start, or forward |
| Option+Fn+Delete | in a markdown file: delete the word after the caret |
| Cmd+Fn+Delete | in a markdown file: delete to the line end |
| Cmd+Z, Shift+Cmd+Z | in a markdown file: undo, redo |
| Cmd+A, Cmd+C, Cmd+X, Cmd+V | in a markdown file: select all, copy, cut, paste |
| Tab, Shift+Tab | in a markdown list: indent or outdent the item; in a table: the next or previous cell (Tab in the last cell adds a row) |
| Up, Down | in a table: the cell above or below |
| Cmd+click | in a markdown file: open a link, or an image, poster, video or audio |
| Enter | in a markdown list: start the next item; on an empty item, end the list |
| Escape | in a markdown file: collapse the selection, leave the active block, or close find |
| Space | in a video tab: play or pause |
| Left, Right | in a video tab: back or forward 5 seconds |
| Shift+Left, Shift+Right | in a video tab: back or forward 1 second |
| , and . | in a video tab: one frame back or forward, pausing first |
| Home, 0 | in a video tab: to the start |
| L | in a video tab: loop on or off; after a Shift+drag on the scrubber, loop that range |
| M | in a video tab: mute on or off |
| R | in a video tab: reload the file now |
| Cmd+O | open folder…: the native picker; the folder opens in a new window |
| Cmd+N | new window: the project chooser in its own window (also the menu's "New Window"); the folder chosen opens in a new window; a config file that still says `"open recent"` keeps working |
| Cmd+, | settings |
| Cmd+/ | keybindings: the Keybindings window (also Help, Keybindings) |
| (unbound) | about, documentation, open config |
| Shift+Cmd+P | command palette |

Notes:

- A lone Cmd+K clears the scrollback after gpui's one-second pending-chord delay; the Cmd+K
  chords fire at once. Cmd+K alone does nothing in a busy or empty pane.
- Pane numbers are structural reading order: rows top to bottom, left to right within a row,
  computed as if every split were at half, so dragging a handle never renumbers a pane.
- Cmd+W (and the tab's ×, and Cmd+K then W) keeps a closed terminal's shell alive for a
  minute: Shift+Cmd+T within that minute brings the same shell back, history and all, in the
  pane it was closed from. After the minute the shell ends, and Shift+Cmd+T opens a new one.
  The daemon keeps that minute, so a window that crashes or is killed within it still has the
  shell ended on time (the update journal says "not reopened within 60 s, ended"), and an update
  or a restart in the minute keeps the deadline; any window that attaches the session first
  cancels it. Quitting ends every closed tab's shell at once. A tab whose shell has already
  ended is not kept for Shift+Cmd+T, since nothing is left to end or bring back. A closed
  viewer tab comes back too, pinned, in the pane it was closed from.
- Option is Meta, as with Terminal.app's "Use Option as Meta Key": Option+a key sends ESC plus
  the key, so Claude Code's Option+Enter (newline), Option+P, Option+T, Option+O and Alt+B/F/D/Y
  all work. Option+Left and Option+Right send the word-jump sequences ESC b and ESC f; every
  other modified arrow, Home and End sends xterm's `ESC [ 1 ; m X`; Option+Delete sends ESC DEL,
  the word delete, and a modified forward delete sends `ESC [ 3 ; m ~`. Set
  `option_as_meta = false` under `[terminal]` to have Option compose characters (ß, π) instead.
- A program that turns on the kitty keyboard protocol (Claude Code does) gets its
  "disambiguate" encoding: Esc as `ESC [ 27 u`, and a modified Enter, Tab, Backspace or Space, a
  key with Option, or Ctrl+Shift with a key as `ESC [ code ; mods u`, so Shift+Enter is a newline
  there. Ctrl+letter keeps its control byte even then, so Ctrl+I, Ctrl+M and Ctrl+[ stay Tab,
  Enter and Esc. Everywhere else those keys keep their usual bytes: Shift+Enter in a shell is
  Enter. A program killed on the alternate screen can leave the protocol on; `reset` clears it.
- Drag across terminal text to select it (double click a word, triple click a line); Cmd+C copies
  it with a program's own line wraps joined (see the next item), Shift+Cmd+C copies it as shown,
  and a click clears it. A selection keeps its place while you scroll, and dragging past the
  pane's top or bottom edge scrolls the history, except in programs that take the mouse or the
  whole screen, such as vim or `less`. A program that asks for mouse reports, such as Claude
  Code's fullscreen renderer, gets the wheel and single clicks, so its conversation scrolls and
  its options can be clicked; a drag still selects locally and the program never sees it, though
  the first click of a double click has already reached it. On the alternate screen without mouse
  reports (`less`, for one) the wheel sends arrow keys (xterm's alternate scroll, `?1007`, on by
  default), and Shift+wheel always scrolls Arkos's own history.
- Claude Code wraps its own prose to the pane's width and ends each row with a real newline, so
  Cmd+C joins those rows back into one line per paragraph or list item: a row joins the next when
  the next one's first word would not have fit on it. A row that ends early, a blank line, a list
  marker, a box-drawn frame or table, a rule (`=====`), columns padded with spaces (`ps aux`), a
  row cut short with `...`, and text that never came within 12 cells of the width all keep their
  line breaks, and a continuation's indent is dropped on the join. Shift+Cmd+C
  ("copy as shown") copies the rows exactly as they are on screen, and a block selection is
  always copied as shown.
- A link in terminal output (anything starting `http://` or `https://`, up to the next space
  or quote) is underlined, and Cmd+click opens it in your browser, as in
  Terminal.app. A plain click does nothing to it. A right-click (or Control+click) on a link
  offers Open Link and Copy Link. A URL wider than the pane is one link across the rows it
  takes, whether the terminal wrapped it or Claude Code cut it at its own width, and Cmd+click
  opens the whole of it.
- A pane closes with its last tab and its neighbour takes the space; the last pane stays and
  shows the empty screen, which is also what a folder with no saved layout opens on.
- Double-click a tab's title to rename it: Enter keeps the name, Escape drops the edit, a click
  elsewhere keeps it, a blank name brings the automatic title back. The name is saved with the
  layout (it survives a relaunch and an update, and follows the tab between panes) and goes
  when the tab closes.
- Every binding can be changed in the config file; see Config. A shifted symbol can be written
  as Shift plus the plain key (`ctrl-shift-\``) or as the character itself (`ctrl-~`): macOS
  reports the character, and both spellings are the same binding.

## Menu, About, Settings and Keybindings

The menu bar has two menus. "Arkos IDE" holds About Arkos IDE, a line, New Window, Settings, Quit;
"Help" holds Keybindings, Documentation and What Arkos Sends. They work from the chooser, the
project window, About, Settings and Keybindings alike. Each Arkos process has its own About,
Settings and Keybindings windows, so two open projects mean two of each. Closing the project
window closes them and quits the app. New Window from any of the three goes to the project window,
or brings the chooser forward. The three draw the project window's title bar instead of the native
one: the window's name in bold where the project path would be, dragged to move the window and
double clicked to zoom it (About keeps its fixed size).

Help, Keybindings (or Cmd+/, or "Keybindings" in the palette) opens a read-only list of every
action with the keys in force, grouped by what it acts on: Window and app, Panes, Tabs, Terminal,
Sidebar and files, Browser, Viewer and markdown editing, and Updates and maintenance. The names
read as the palette writes them, and the keys as the Mac does (Shift+Cmd+P). An override in
`config.toml` shows as the key it binds, within a second of saving; an unbound action reads
"none". The nine "focus pane" rows read as one, "Focus pane 1 to 9", while all nine keep their
defaults. The viewer section ends with the editor's fixed keys (undo, copy, a list's Tab), which
no config entry changes. The footer says "Change them under [keybindings] in config.toml" next to
an "Open config" button. Help, Documentation opens this README in the default browser; it has no
key unless the config binds `"documentation"`.

About (menu, or "About Arkos IDE" in the palette; no key), under the shared title bar, shows
these rows: Version, Build (hash and date), Last updated, First installed, Daemon, Protocol,
macOS, Config, Themes, App Support, Window log and Daemon log. Daemon reads "{version} (hash), pid
N", or "not connected". Dates read "today HH:MM" for the current day, else YYYY-MM-DD. Asking for
About again refreshes the rows. "Copy details" puts "Arkos IDE {version}" and one `label: value`
line per row on the clipboard. Below are "Made by Cyrus Sarkosh" (a link to his GitHub); a "Buy
me more tokens" line shows only once a donation URL is set in the build.

Settings (menu or Cmd+,) has the shared title bar and six sections.

- Appearance: Mode (System, Dark, Light), the dark and light theme, "Add theme…" and "Show themes
  folder". "Add theme…" takes a `.toml` file (anything else is refused), names the copy after
  the file's own `name`, shows the error under the button for a bad file, and asks "Replace the
  existing {name}?" before overwriting a theme file or shadowing a built-in of that name. A new
  theme is set as the dark or light theme by the mode in force. "Show themes folder" creates the
  folder, then reveals it. A configured theme that is neither built in nor a file is listed
  first as "(not installed)".
- Text: the font family (installed monospace families; a configured family that is missing is
  listed first as "(not installed)"), the text preset, and Size and Line height steppers that
  write both values of the preset together and show the file's values as they are. The preset
  shown and stepped is the one this project window uses (in the chooser, the config's default);
  picking one makes it the default for new workspaces and applies it to this window.
- Startup: "Start at login" on or off; it reads "set by config to ask" while the file says `ask`.
- Updates: the automatic-updates switch; "{version} is ready" with "Update now" when a build
  waits.
- Privacy: Usage reports (Off, Local only, On) with a line under it saying what the mode means,
  or which rule of the environment forces it; Crash reports (Never, Ask each time, Always); "What
  Arkos sends", which opens the disclosure page; "Last report", which opens the session sealed
  last in a viewer tab; and Reports, each crash report by date with its id (a click copies it)
  and what became of it: `waiting`, `queued`, `left the queue` or `declined`.
- Keyboard: "Keybindings live in config.toml" and an "Open config" button, then "Every binding is
  listed under Help, Keybindings" and a "Keybindings" button that opens that window.

Every change is written to `config.toml` in place, keeping its comments, order and formatting, and
applies at once; a theme change cross-fades the window, its terminals included, over 200 ms (at
once with Reduce Motion on), and Settings, About and Keybindings follow it. Other Arkos windows
are other processes and follow within a second through the config watcher. A change made in the
chooser is written and takes effect when the project window opens. While the config has an error,
Settings shows the error in a banner and its controls are read-only, except "Open config". A write
the file refuses (a read-only file, say) is said above the controls, which stay live. Keybindings
stay in the file. A user's `"open config" = "cmd-,"` makes the key open the config file instead of
Settings; the menu still shows the shortcut next to Settings, a known limit.

Help, What Arkos Sends opens the disclosure page, [telemetry.md](telemetry.md), which lists every
key a report can carry. Usage reports are recorded on your Mac and sent only once you turn them on.
On a day after the install, a release build asks once, in a strip under the first pane's tab bar:
**Turn on**, **Not now** or **What is sent**. After a crash, the next launch shows a dialog with the
report's exact bytes under **Show report**: **Send** queues that report to be sent, **Don't send**
deletes it, and the "Always send crash reports" box, unchecked until you check it, sends later ones
without asking. Settings > Privacy > Reports keeps a line for each, with the id to quote in an
issue. `arkos telemetry status` shows the mode in force and why; `arkos telemetry on`, `off` and
`local` and `arkos telemetry crashes never|ask|always` change it; `arkos telemetry view` prints what
waits to be sent.

## Config

`~/.config/arkos/config.toml` (or `$XDG_CONFIG_HOME/arkos/config.toml`) is written with commented
defaults on first run and opened in your text editor from Settings' Keyboard section ("Open
config"), the Keybindings window's footer, or the palette's "Open config". It is watched: a saved
edit applies within a second. A bad edit shows a banner with the line number and keeps the last
good settings.
Sections: `font` (family, the default text preset, and the presets), `terminal`
(scrollback, shell, nice, option_as_meta), `keybindings` (action name to keys, `""` unbinds), `theme` (`dark`
and `light`), `appearance` (`system`, `dark` or `light`), `daemon` (applies when the window starts
the daemon), `recovery` (`start_at_login`: `ask`, `on` or `off`), `update` (below), `sidebar` (its width and
whether it starts collapsed), `viewer` (`highlight_limit_bytes`, and `wide_tables`: `wrap` or `scroll`), and `browser` (`url`, the page a new
browser tab opens when its prompt is left empty, and `app_mode`, which opens user URLs as a single
page).

```toml
[update]
# The daemon checks the release manifest every check_every_secs and downloads a newer, signed
# build in the background. With auto = true the window applies it once you have been idle for
# idle_after_secs (no typing, nothing open): the daemon swaps in place and the window relaunches
# with every shell still there. auto = false keeps the bell's dot, "Update ready" and the
# palette's "Update now" instead.
auto = true
check_every_secs = 300
idle_after_secs = 30
```

`check_every_secs` is the daemon's: it reaches a running daemon only when the window starts it or
the login item is reinstalled ("Start at login" in the palette); a daemon already running keeps
the interval it started with. `auto` and `idle_after_secs` apply at once.

Claude Code tips: fast trackpad scrolling in a fullscreen `claude` jumps unless
`wheelScrollAccelerationEnabled` is `false` in `~/.claude/settings.json`; the doctor (the
wrench at the right of the title bar) sets it for you. Claude Code's colours follow the Arkos
theme when its theme is Auto (`"theme": "auto"`): it asks the terminal for its background, and
Arkos answers with the theme's and tells it at the end of every theme switch's fade, so a light
theme never shows Claude Code's dark-theme text. Arkos sets Auto itself at start when the theme is unset, `dark` or
`light`; for any other choice the doctor offers it.

Themes are one TOML file each in `~/.config/arkos/themes/`; `navy`, `navy-light`,
`timo-purple`, `peach-light` (warm cream and peach) and `lavender-light` (a VS Code lavender look) are built in. [themes.md](themes.md) documents every key;
`arkos theme list` names the available themes and `arkos theme validate <file>` checks one
(`ok: {name}`, or the error and exit status 1).
"Extract built-in themes" in the palette writes the built-ins there to copy from. A theme chosen
from the palette lasts until the config reloads or the app relaunches; the config file decides
otherwise. However a theme arrives (Settings, the palette, `arkos theme use` or `preview`, an
edit to the theme's file, a macOS light or dark switch), the window fades from the old colours to
the new over 200 ms; with Reduce Motion on, it switches at once. Text presets are `compact` (14 px, 1.3), `comfortable` (16 px, 1.5) and `relaxed`
(18 px, 1.6); add your own under `[font.presets.<name>]`. Each folder remembers its preset.

### Agents

A coding agent running in an Arkos terminal can show you a video or a document in a pane beside
it and drive the video, and it can make a theme, try it, check its contrast and switch to it with
commands, and change the common settings the same way. `arkos guide` prints what the agent needs.

`arkos open <file>` puts a video, a markdown file or any other text file in a pane of the window
the agent runs in. The first open splits the agent's own pane to the right and later ones reuse
that pane, so it never covers what you are looking at; `--pane N` (numbered as Cmd+1 to Cmd+9)
and `--split` choose another place. A file already open is shown where it is rather than opened
twice. The keyboard focus stays where you are typing unless the agent passes `--focus`. A video
opens paused unless the agent asks otherwise, and `--at 0:12`, `--loop 0:12-0:18` and `--playing`
jump to a scene and play it on repeat. `arkos video` plays, pauses, seeks, steps, loops, reloads,
mutes and closes a video tab, and `arkos panes` lists the panes and their tabs (their titles,
never what a terminal shows). Nothing on this channel runs a command in a terminal or reaches a
session.

Inside Arkos shells, `arkos` is on PATH and `$ARKOS_BIN` names the binary; elsewhere use the full
path, usually `/Applications/Arkos IDE.app/Contents/MacOS/arkos` (the Homebrew cask installs the
app alone and links nothing onto the PATH).

Exit codes, for every command: 0 done; 1 refused or failed (a bad file, an unknown name, a low
contrast pair, a window that reported an error or none that answered, an old daemon); 2 usage;
3 a window was needed and none is running. `--json` prints one JSON value and nothing else.

| Command | What it does | Exit codes |
| --- | --- | --- |
| `arkos theme list` | every theme, marking a broken file | 0 |
| `arkos theme validate <file>` | checks a file: `ok: {name}` or the error | 0, 1 |
| `arkos theme template [<built-in>]` | a built-in's TOML to start from; `--json` gives the key schema | 0, 1 |
| `arkos theme contrast <file-or-name>` | one line per colour pair, `ok` or `LOW (needs 4.5)` | 0, 1 when any pair is low |
| `arkos theme add <file>` | copies a file into the themes folder; `--replace` overwrites | 0, 1 |
| `arkos theme use <name-or-file>` | writes `[theme] dark` or `light` (`--dark`, `--light`) and reloads every window | 0, 1 |
| `arkos theme preview <name-or-file>` | shows a draft in every window without writing; `--end` restores | 0, 1, 3 |
| `arkos theme current` | the config's themes and what each window shows | 0, 1 |
| `arkos settings get [<key>]` | the config file, or one key's value | 0, 1 |
| `arkos settings set <key> <value>` | sets a key on the allow-list, then reloads every window | 0, 1, 2 |
| `arkos config reload` | asks every window to read `config.toml` now | 0, 1, 3 |
| `arkos open <file>` | shows a video, a markdown file or a text file in a pane beside the agent | 0, 1, 2, 3 |
| `arkos video <command> [<file>]` | `status`, `play`, `pause`, `seek`, `step`, `loop`, `reload`, `close`, `mute`, `unmute` on a video tab | 0, 1, 2, 3 |
| `arkos panes` | the window's panes in Cmd+1 order with their tabs; `--all` for every window | 0, 1, 3 |
| `arkos guide` | prints the agent guide; `--path` writes it to a file and prints the path; `--install` installs it as a Claude Code skill | 0, 1 |

Every command that talks to windows prints one line per window, `window {pid} ({project}): ...`,
and `window {pid}: no answer` for one that stays silent for two seconds. `use` and `settings set`
with no window open save the file and print `saved; no window running` (exit 0). `use` and
`settings set` write `config.toml` in place and keep its comments; a `config.toml` that does not
load is refused by `use`, and repaired by `settings set` when the edit makes it load.

The configured theme's file is watched like `config.toml`: an edit applies within a second. A file
that does not parse keeps the last good theme on screen and puts the error in the banner. While
`config.toml` itself is broken, a theme edit waits for the config to load again. A preview ends at
`--end`, at the next config or theme reload, when the system appearance changes (dark to light or
back), or when the window closes; ending one while the configured theme's file is broken keeps
the preview's colours with the error shown.

`arkos settings set` takes `appearance.mode` (system, dark, light), `theme.dark`, `theme.light`,
`font.family`, `font.preset`, `font.presets.<name>.size` (8 to 48),
`font.presets.<name>.line_height` (1.0 to 2.5), `viewer.wide_tables` (wrap, scroll),
`update.auto` (true, false), `recovery.start_at_login` (ask, on, off), `telemetry.usage` (off, local, on) and
`telemetry.crashes` (never, ask, always). Any other key exits 2 and lists these. A bad value is
refused before anything is written. `settings get <key>` reads the file only, so a key left at its
default answers `no such key`.

An agent finds `arkos guide` in three ways, from no setup to one command:

1. The SessionStart hook ("Install Claude Code hooks" in the palette, or the doctor). In an Arkos
   shell it prints two lines telling the session it runs inside Arkos IDE and, to show you a video
   or a document or to change the theme or settings, to run `arkos guide` first (by the binary's
   full path when `arkos` is not on the hook's PATH). It also installs the
   guide as a skill into the starting Claude Code's own config folder (`CLAUDE_CONFIG_DIR`, else
   `~/.claude`); that Claude Code loads the skill at its next start.
2. `arkos guide --install` writes the skill (`skills/arkos-ide/SKILL.md`) into the config folder of
   the current environment and prints a line to paste into `AGENTS.md` or `CLAUDE.md`, for agents
   that read those; a skill already up to date is left alone (`already current: {path}`). The
   palette's "Install agent guide" installs it too. It never overwrites a skill it did not write.
   `arkos guide --path` writes the guide to `~/Library/Application Support/arkos/agent-guide.md`
   for agents that take a file.
3. `arkos --help` lists `guide` first and `open`, `video` and `panes` after it, and `arkos theme`,
   `arkos open` and `arkos video` with no arguments say `see: arkos guide`.

The doctor ("Agent guide for Claude Code" in the wrench's list) offers to install or refresh the
skill, and `arkos doctor` has a `themes:` section: `{n} themes, all valid` or one line per theme
file that does not load (ending with the `arkos theme validate` command to run on it), a
`theme.dark` or `theme.light` that names no theme, and the skill's state (installed, not
installed, older than this build, or a different skill; an older one means agents will not know
about video panes until it is reinstalled). The doctor row checks only the folder of
the environment Arkos runs in.

`arkos config`, `arkos settings` and `arkos guide` are commands, so they win over a folder of the
same name: `arkos ./config` opens the folder.

## Updating

The IDE is installed as `/Applications/Arkos IDE.app` through the Homebrew cask (its text is in
`homebrew/arkos-ide.rb`, published in `csarkosh/homebrew-tap`), which holds both `arkos` and `arkosd`:

```
brew install --cask csarkosh/tap/arkos-ide
```

Releases from 0.3.2 on are signed with a Developer ID and notarized, so the download opens as any
app does. The cask says `auto_updates`, so
`brew upgrade` leaves the app alone and the IDE updates itself.

Updates arrive by themselves: the daemon checks the release manifest every five minutes, downloads
and verifies a newer build, and the window applies it once you have been idle for thirty seconds
(no typing, nothing open): `arkosd` swaps in place and the window relaunches with every shell
still there. `[update] auto = false` keeps the bell's dot and "Update now" instead.

"Check for updates" in the palette asks now and waits for that check (up to 30 seconds) before
saying one thing: "update ready: X" when it found a release (the bell gets its dot too), "update
check failed: …" when the release itself was bad, "updates unreachable: …" with the daemon's
words when GitHub could not be reached, "the update check is still running" when 30 seconds were
not enough, and "no update" only when the check reached the manifest and found nothing newer.
"Retry update" says its outcome the same way. A daemon from before protocol 5 is asked once after
three seconds, and says "the daemon has not reached the update server yet" when it never has.

A release downloaded and staged before a daemon restart is kept: the restarted daemon adopts the
staged bundle (the journal says "adopted staged X from before the restart") instead of
downloading it again, and the bell's dot stays.

A check that cannot reach GitHub (offline, a captive portal) is retried quietly at the next one;
when none has reached the signed manifest for over a day, the bell's popover says
"updates unreachable for N days" until one does.
When the daemon's last check or update failed for a reason that is the release's (a bad
signature, a download that does not match its manifest) or the swap's, the bell's popover reads
"update failed: <reason>", a toast shows the reason once, and the palette offers "Retry update".
If the new window does not come up within 20 seconds of its relaunch, or the daemon's own
upgrade onto the new build fails (the journal says "the upgrade onto X failed: …; the grace for
X starts now"), the daemon puts the previous app back 20 seconds later, reopens your windows,
and records the version in
`~/Library/Application Support/arkos/updates/rejected` so it is never downloaded again (a newer
release still is). To try a rejected version again, delete that file.

This version (0.4.0 when released) speaks protocol 6, which the agent commands need: the update
moves the daemon and every window together, so a window never talks to an older daemon.

Sessions live in `arkosd`, so updating never ends one. After `cargo build --release` into the
binaries you run (a build from source), the bell gets its dot and "Update ready" within
five minutes (or at the next launch), and the palette's "Update now" does the rest: if the
daemon's binary changed, the daemon writes a handover file, execs the new binary in the same
process (same pid, the shells stay its children) and the window reconnects; then the window
asks the daemon to relaunch it from the new binary and quits. Every tab comes back with its
history. The daemon and window speak protocol 6; the daemon still serves protocol 1 to 5 windows,
and reads the previous handover formats and recovery manifest, so updating from 0.2.12 or the
update-in-place build is an ordinary "Update now". Upgrading from milestone 2b is the one
update that needs `pkill -x arkosd` (the old daemon knows nothing of handovers), which ends
every session.

Click the bell, then "Update now", to update at once.

If the new daemon fails its check or the exec fails, the old daemon keeps running and the window
shows the error. If the window loses the daemon for ten seconds, it starts a new one and every
tab becomes a fresh shell with the "(restarted)" note.

A window newer than its daemon (one started from a new build while an older daemon still runs)
is refused and stops reconnecting, so "Update now" cannot reach that daemon. The fix is the
one-time path above: `pkill -x arkosd`, which ends every session, and open the window again.

### When an update goes wrong

Run `arkos doctor` (from the app: `"/Applications/Arkos IDE.app/Contents/MacOS/arkos" doctor`).
It only reads: it prints the installed app and its version, whether a previous (`.old`) or
half-swapped (`.new`) copy sits beside it, the running daemon's version and pid, the update
marker and the rejected versions, the last lines of the daemon's update journal and of
`window.log`, and any crash reports from the last day. The journal,
`~/Library/Application Support/arkos/updates/journal.log`, has one timestamped line per step of
each check, apply, relaunch, confirmation and rollback, including every Hello the daemon heard
while it waited for the new window. A check that finds the same thing as the one before is
counted rather than written again: "check: N more checks since HH:MM found X, still not newer
than Y" when the outcome changes, at an update or exit, and once a day (failed checks the same
way, "check: transient, N more times since HH:MM: …"), so the journal holds weeks.
`~/Library/Application Support/arkos/window.log` keeps the
output of every window the daemon launched. Both are what to attach to a bug report; both name
the folders your windows had open, so look them over first.

### Renamed from Sarkos

The IDE used to be called Sarkos. A machine that ran it keeps the old login item, daemon,
config and saved workspaces under the old name; move them over once (this ends the old
daemon's sessions):

1. `launchctl bootout gui/$(id -u)/sh.csarko.sarkosd && rm ~/Library/LaunchAgents/sh.csarko.sarkosd.plist`
2. `pkill -x sarkosd`
3. `mv ~/.config/sarkos ~/.config/arkos`
4. `mv "$HOME/Library/Application Support/sarkos/workspaces" "$HOME/Library/Application Support/arkos/"`
5. Open Arkos IDE and run "Install Claude Code hooks" and "Start at login" from the palette
   (Cmd+Shift+P).

## If you get headaches

The defaults (16 px Menlo, line height 1.5, a bright grey on navy, no warm tint) follow
[Check your eyes before your theme](https://csarko.sh/research/check-your-eyes-before-your-theme).
The same research puts a comprehensive eye examination that tests convergence and small
astigmatism ahead of any setting. Not medical advice.

## Further reading

[Update the IDE, keep the shells](https://csarko.sh/research/update-the-ide-keep-the-shells):
the write-up of why updates kill sessions elsewhere and how this IDE avoids it.

[Bring the agents back after a reboot](https://csarko.sh/research/bring-the-agents-back-after-a-reboot):
the research behind milestone 3's recovery of Claude Code sessions after a reboot.
