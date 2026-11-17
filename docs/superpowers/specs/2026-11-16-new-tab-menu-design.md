# The tab bar's "+" offers Claude, Codex or a shell

Cyrus, 2026-11-16: "I want the `+` button to open a menu of `Claude | Codex | Shell`. Click on
Claude or Codex opens the new tab with their interactive CLIs started automatically. Shell opens
a new terminal instance as the IDE does today." The README's line asking a new user to "start
`claude` in a tab" goes with it.

## 1. The menu

- A click on a pane's "+" (its tab bar's right end) opens a small menu under it, its right edge at
  the press, kept inside the window: **Claude**, **Codex**, **Shell**. A second click on the "+"
  closes it.
- **Claude** and **Codex** open a terminal tab in that pane, as "+" did, and its shell types the
  agent's command once it has started (`claude`, `codex`, then Enter), as a resumed session's
  shell types its resume command. The shell stays under the agent: quitting the agent leaves a
  prompt in the tab, not a closed tab.
- **Shell** is the "+" of before: a terminal tab with the shell alone. Its row shows the new
  terminal tab shortcut (Ctrl+Shift+` unless rebound), which still opens a shell directly.
- A command that is not installed says so in the tab, as the shell would (`command not found`);
  Arkos does not look the agents up first, since the app's PATH is not the login shell's.
- The menu is drawn, lifted and faded as the terminal's right-click menu is (`link_menu`); a press
  anywhere else or Escape closes it, and opening another menu closes it.

Not in this: "+ New session" in the sidebar still starts a plain shell, and so does the new
terminal tab shortcut.

## 2. What is tested

Headless: the rows and their order, what each row's shell is given to type, and where the menu
sits for a press near either edge of the window. On screen (a person, the Status table): the menu
under the "+", and a Claude row starting Claude Code in the new tab.
