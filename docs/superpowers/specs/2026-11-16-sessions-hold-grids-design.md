# A session holds a grid; Scheduled; a date and time picker with repeats

Cyrus, 2026-11-16: "For the Inbox layout, I want to be able to create new Claude Code sessions
within a session and drag & drop all the panes in that session. That should include the original
pane created for the session. My plan is to drop the "Grid" layout option and fully support
"Grid" layout within an "Inbox" layout's session." Also: "Later" becomes "Scheduled", as Slack
names it, and the row menu's "Remind me in", Custom opens a date and time picker like Slack's,
with a recurring reminder.

His answers to the three open questions, the same day:

- An Inbox session holding several Claude Code sessions is **one row only**, sorted by its most
  urgent Claude Code session, with a small count of its sessions.
- A recurring reminder that fires comes up as Unread; once looked at and left, it goes **back to
  Scheduled** until its next time. "While it's in unread, have some label on its sessions list
  option that it's a scheduled message."
- A window saved in the Grid opens with **each terminal its own session**, keeping the files it
  opened, as switching to the Inbox does today.

## 1. A session is a page

The window shows one **session** at a time, as before, and a session is now a whole grid: a
**page** of panes of tabs that the user splits and arranges as the Grid layout did. The page is
made from the session's first terminal (its **original** pane) and grows as the session opens
files, pages and videos and as the user opens tabs and terminals in it.

- Every pane of the page has its own tab bar, the original included: the session header that
  stood in the sessions pane's tab bar goes. Its name and state are the sidebar row's; "Mark
  unread" and "Done" are in the row's "Move to" menu.
- Any pane drags by its number chip and any tab drags by its tab, the original pane included,
  with the zones the Grid had (Inbox stacking spec, section 3). Nothing is held in shape any more.
- A new terminal opened in the page (Cmd+T, a split, the tab bar's "+") stays in the page: it is
  another Claude Code session of the same Inbox session. "+ New session" in the sidebar starts a
  new page with a fresh terminal and shows it.
- Panes are numbered within the page shown, Cmd+1 to Cmd+9 as before.
- A session's `arkos open` lands by the placement setting (Neighbor by default) within its own
  page, also while another page is shown, which is left as it is.
- Choosing a row shows its page as the user left it, focused where it was, unless the row is
  there for one of its Claude Code sessions in particular (section 2), whose pane is focused.
- A page whose last terminal closes closes with its other tabs (a file's edits saved first; a
  tab that will not close stops it), as ending a session closed its side. "End session" on a row
  ends the whole page that way. Then the row below is shown, else the one above, else the empty
  screen.

The Grid layout goes: Settings has no Layout choice, `[layout] name` is read and ignored, and
`[layout] placement` says where a session's files open within its page. The `arkos open` guide
text keeps "your own pane" meaning the pane of your session's terminal.

## 2. One row per session

The sidebar lists one row per page.

- **Where it is listed:** the most urgent of its terminals' states (Needs you, Unread, New,
  Unanswered, Working, Quiet, Scheduled, Done), with the page's reminder over it (section 4).
- **Its name:** its original terminal's (the user's name for it, else the program's title); when
  that one has closed, the oldest terminal left in the page.
- **Its line:** the line of the terminal that decides where it is listed (the **lead**): what it
  asks or last said. The lead is the most urgent terminal; among equals, the one focused in the
  page, else the oldest.
- **A count:** with two or more terminals, the row says so beside its age, as "3 sessions".
- **Choosing it** shows the page with the lead's pane focused when the lead calls for the user
  (Needs you, Unread), else the page as the user left it.
- **Its menu:** "Move to" moves every terminal of the page (Unread marks only the lead, which a
  look then clears); "Remind me in" sets the page's reminder; Rename renames the original
  terminal; "End session" ends the page.
- The Dock count, Shift+Cmd+U (the next session that needs you), the filter and the held unread
  row all work on rows. A filter matches the row's name, its line, or any of its terminals'
  names.

A closed Claude Code session kept under Done is still one row each, and resuming one starts a new
page with it.

## 3. Scheduled

"Later" is renamed "Scheduled" everywhere the user reads it: the section heading, the Move to
menu, the guide. The saved name `later` still reads.

## 4. Reminders

A reminder belongs to the page: a time, and a repeat (never, every day, every weekday, every
week on that weekday, every month on that day of the month).

- **Pending** (its time to come): the row is under Scheduled, whatever its terminals do, and says
  how long until its time ("in 3 h"), with a repeat glyph when it repeats.
- **It comes up** when its time comes: the row is under Unread, or under Needs you or Working when
  a terminal of it is so, and carries a **Reminder** label in place of its age while it is up.
- **A terminal's new turn** (an agent finishes or asks the user) while it is pending brings the
  row up as that turn says, without the label; a one-time reminder is then dropped, as today.
- **A look**, the user on the page while the window is in front: a one-time reminder is done; a
  repeating one is set to its next time after now, so the row goes back under Scheduled. The
  row the user chose stays where it was listed until they choose another (the held row).
- **Move to** any section clears the reminder, the repeat with it.

A repeat's next time is computed in local time from the time chosen: every day at that time;
every weekday at that time, Saturday and Sunday skipped; every week on that weekday; every month
on that day, the month's last day when it is shorter.

Saved in the workspace by page. A file from before, whose terminals carried `later_until`, gives
each page the earliest such time, one-time.

## 5. The date and time picker

"Remind me in" keeps its five times; Custom opens a card beside the menu, like Slack's "Custom"
reminder:

- A month of days: the month and year, previous and next month arrows, the weekdays' initials,
  and the days, today ringed, days before today dimmed and not chosen, the chosen day filled.
  Arrow keys move the chosen day, Page Up and Page Down the month.
- A time: a field that shows the chosen time ("9:00 AM", or 24-hour when the system clock is)
  with a list of times every 15 minutes under it to pick from; typed text in the forms
  `parse_when` reads (`14:30`, `2pm`, `9:15am`) sets it too.
- Repeat: Never, Every day, Every weekday, Every week on <weekday>, Every month on the <nth>,
  named for the chosen day.
- A summary line ("Tomorrow at 9:00 AM, every weekday"), Cancel and Save. Save is dimmed for a
  time already past. Enter saves, Escape cancels.

It opens on tomorrow at 9:00, never repeating, or on the page's own reminder when it has one.
Motion: the card enters and leaves as the menu's cards do.

## 6. Moving from the old shapes

On load, a workspace from before becomes pages:

- The Inbox's shape (the sessions pane and its side): each terminal of the sessions pane is a
  page, its side beside it where the side sat (the saved place and divider), the shown one shown.
- A Grid arrangement, saved with the Grid on or kept aside while the Inbox was: the tabs are
  gathered as switching to the Inbox did (`Sides::enter`), then made pages as above.
- The kept sides, owners, side places, dividers and the Grid's arrangement are read for this and
  never written again.

## 7. What is tested

Pure, headless: the pages (showing one, keeping the others, every tab and terminal of every page
found, a hidden page's tabs changed in place, a page closed with its last terminal); the
conversion from each old shape; the rows (lead, name, line, count, where listed, holding);
reminders (pending, coming up, a look, a turn, a move, every repeat's next time across a month's
end and daylight time); the picker's model (the month grid, the chosen day, the time list,
parsing, the summary). On screen, by a shoot storyboard: a page with two terminals and a file,
the original pane dragged; the picker open.

## 8. Not in this

The macOS notification for Needs you (still to come). The marketing page's "Two layouts" band
and the README's Grid pictures, which this makes out of date: a reshoot after Cyrus has seen it.
