# A session's name and a tab's name are their own

Approved by Cyrus, 2026-11-18.

## Why

Cyrus, 2026-11-18: "I also want to decouple renaming tabs and sessions. When I right-click on the
session row in Pane 0 and choose rename, I want the rename to be in-place on that row. It should
not rename the tab in the other panes. Likewise, renaming a tab should not affect the session's
name in Pane 0. You can keep the behavior where the session row updates its name from Claude's
activity if the user hasn't manually set it."

## The design

- A session (a page with a terminal) has a name of its own, kept with the layout by its page
  (`Tree::session_names`). Its row shows that name, else its first terminal's title (Claude's
  topic), else the shell's name, as before.
- The row menu's Rename edits the name in place on the row. Enter keeps it, Escape drops the edit,
  a blank name gives the automatic one back, and a press elsewhere keeps what was typed (as a tab's
  rename does). The editor holds the keys while it is open; they go back where they were after.
- A tab's rename (double-click on its title) names the tab alone; a tab never shows the session's
  name.
- An ended session keeps the session's name under Done, and a resumed one takes it back as the
  session's name, not the tab's.
- A layout saved before this has the names on tabs only: on load, each page's first terminal's tab
  name is copied to its session, once (`Tree::names_split`), so no name the user gave changes.

## Approaches

One sensible way: a name per page beside the names per tab. Keying the session's name by its first
terminal's tab id instead was rejected: that tab can close while the session goes on.

## Left to the builder

Where the row's editor draws (the row's name cell, in the row's font and height).
