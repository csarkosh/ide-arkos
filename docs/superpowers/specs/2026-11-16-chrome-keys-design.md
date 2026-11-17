# Keys you already know: a Chrome-style key map

Cyrus, 2026-11-16: "I'm not a fan of the hotkey approach. I prefer something similar to Chrome's
keyboard navigation. I like the `Cmd+{0-N}` hotkey for quickly selecting panels. But I don't
really use the other's because it's too much to memorize. I prefer efficient keyboard
navigation so there's less to remember."

His choices, the same day: the Chrome map below with Cmd+0 for the session list; today's other
navigation defaults dropped from the default keymap (still in the palette and bindable in
`config.toml`); Cmd+T opens the tab bar's "+" menu.

## 1. The map

| Keys | What it does |
| --- | --- |
| Cmd+0 | the session list: the sidebar opens on Sessions if it is not, and the keys go to its filter |
| Cmd+1 to Cmd+8 | pane 1 to 8 of the session shown |
| Cmd+9 | the last pane, as Chrome's Cmd+9 is the last tab |
| Ctrl+Tab, Ctrl+Shift+Tab | next and previous tab in the focused pane |
| Cmd+Option+Right, Left | the same (Chrome's other pair; Cmd+Shift+] and [ also stay) |
| Cmd+Option+Down, Up | next and previous session in the list's order, folded groups included |
| Cmd+T | the focused pane's "+" menu: Claude, Codex, Shell |
| Cmd+W, Cmd+Shift+T | close the tab, bring the last closed one back |
| Cmd+L, Cmd+F | a web tab's address field, find |
| Cmd+Shift+P | the command palette: everything else |

## 2. In the session list (Cmd+0)

The filter field has the keys, and one row is marked as the cursor (the session shown, else the
first row). While the list has the keys every group is open, as a typed filter opens them
(Quiet and Done are folded at first, and most sessions are Quiet most of the time; found on
screen, 2026-11-16). Typing narrows the list; Up and Down move the cursor through the rows;
Enter shows the cursor's session (a closed one is resumed) and keeps the keys in the list, the
cursor on that session (Cyrus, 2026-11-17: "Only mouse clicks should change focus to the
selected list option's pane"); Escape clears the filter, and on an empty filter hands the keys to
the pane shown. A click anywhere else takes the keys as before.

Up from the first row (or from the file tree's first row) rings the Sessions/Files switcher at
the top of the sidebar (Cyrus, 2026-11-16: "the session list navigation can reach the
Sessions/Files switcher and I can use left/right arrow keys to switch between the 2"). Left
shows the sessions, Right the files; Down or Enter goes into the list shown (the filter with its
ring on the first row, or the file tree on its first row); Escape gives the keys back to the
pane. "Focus file tree" and "Focus worktrees" show the Files view too, which they did not.

On Files, the Explorer and Worktrees headings are stops too (Cyrus, 2026-11-17: "a way to
collapse the Explorer and Worktrees groups"): Left folds the group, Right opens it, Enter or
Space flips it; Up and Down go switcher, Explorer heading, its files, Worktrees heading, its
worktrees, and back. Up from the switcher rings the title bar's bell (Cyrus: "how can I reach
the notifications and wrench icon buttons"): Left and Right move between the bell and the
doctor, Enter or Space opens its menu, where Up and Down move a ring over its buttons and Enter
presses one, Escape closes it; Down goes back to the switcher. Both icons have tooltips saying
what their menus hold.

Cmd+0 only moves the keys (Cyrus, 2026-11-17: "Cmd+0 is just meant to change focus; not reset
the pane's state"): the tab shown stays, and the keys go back to where they last were in it (a
session row, the switcher, a heading, the files, the worktrees), else the tab's first stop.

The focus glow goes with the keys (Cyrus, 2026-11-17: "When the Cmd+0 pane is in focus, the
pane highlight should move to the Cmd+0 pane too"): while the sidebar has them it wears the glow
a focused pane wears, and no pane glows; on the title bar's icons no pane glows either. The
fade between is the panes' own. Under the overworld effect (Timo Purple, Arabia Solar), whose
panes wear a thick frame lit in the focus colour, the sidebar's frame lights the same way
(`Scene::sidebar_focus`).

## 3. In the "+" menu

Opened by Cmd+T or a click, the menu takes Up, Down, Enter and Escape: the first row is marked,
the arrows move the mark, Enter opens that row, Escape closes the menu. The mouse works as
before and moves the mark with it.

## 4. Dropped from the defaults

No default keys (the palette and the config still have every one): focus left, right, up and
down; split down; maximize pane; close all in pane; move tab left and right; move tab to the
previous and next pane; most recently used tab (Ctrl+Tab is the next tab now); focus file tree;
focus worktrees; next session that needs you; fold all and unfold all; focus pane 9 (Cmd+9 is
the last pane); a web tab's zoom reset (Cmd+0 is the session list in a web tab too). Cmd+K alone
stays Terminal.app's clear, which no chord delays any more.

Kept as they were: split right (Cmd+\\), the sidebar (Cmd+B), new window, open folder, settings,
keybindings, the font size, the markdown, video, browser and workbook keys, save, copy as shown,
and the new terminal tab (Ctrl+Shift+\`).

## 5. What is tested

Headless: the default table (every row's keys), the commands the new names run, the rows a
cursor moves through and where it lands at either end, the next and previous session in list
order with wrapping, and the menu's mark. On screen (a person): the cursor row's look, and each
key in a terminal, a markdown tab and a web tab.
