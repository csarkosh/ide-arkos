# Arkos IDE: title bar icons, notifications and the doctor (milestone 6d)

Date: 2026-10-15. Status: approved by Cyrus on 2026-10-15 ("approve, write the plan"). Builds on the update and launcher polish
(`2026-10-15-update-launcher-polish-design.md`) and on `arkos doctor` (the read-only printout
from the update-in-place design). Written against main at `1052c56` (0.2.14).

## 1. Why this exists

The title bar is the one strip the user sees in every state, and today it carries plain text
that comes and goes: "update ready: X", "arkosd reconnecting", "2 waiting". Each new piece of
news would add another string to that row, and Cyrus does not want the bar to fill up. At the
same time, two things the app knows how to fix for the user (a Claude Code setting that makes
wheel scrolling smooth, the launch agent, the title hook, the Accessibility grant) live in the
palette where nobody looks for them, or nowhere at all.

This milestone gives the bar two icons on the right and nothing else. News goes behind a bell;
fixes go behind a doctor. The left side of the bar does not change.

Rulings taken in chat on 2026-10-15:

- The left side stays exactly as it is: traffic lights, "Arkos IDE", the absolute path.
- Two right-aligned icon buttons, each opening a small popover under itself.
- The "N waiting" count leaves the bar; the tab marks already show it per tab.
- The doctor's scroll action sets Claude Code's acceleration switch only, not its speed.
- Nothing runs when a popover opens; every fix is an explicit click.

## 2. Scope

| # | Item | Where |
|---|---|---|
| 1 | The bar's right side becomes two icons with a dot each | `chrome.rs` |
| 2 | Notices: a list fed by the update, daemon, config and theme state; "Updated to X" once | new `notices.rs`, `main.rs` |
| 3 | The notifications popover | `chrome.rs`, `main.rs` |
| 4 | Doctor checks: a table of probe and action | new `doctor_panel.rs` |
| 5 | The doctor popover | `chrome.rs`, `main.rs` |
| 6 | The scroll fix: one key in Claude Code's settings file, with a backup | `hook_cli.rs` (shared JSON edit), `doctor_panel.rs` |
| 7 | Palette commands "Notifications" and "Doctor" | `command_palette.rs` |
| 8 | A version file so a relaunch knows it was an update | `paths.rs`, `startup.rs` |

Out of scope: a notification history across launches (notices live in memory, except the one
file in item 8), sound or system notifications, any check that needs the network, and changing
what the palette offers (its commands stay; the doctor duplicates three of them on purpose).

## 3. The bar (item 1)

`chrome::title_bar` keeps its signature's spirit but takes, instead of `items` and the update
click, a `Right` value: for each icon whether it shows a dot and whether its popover is open.
The right side is a flex row with two 24 px square hit targets, 8 px apart, 12 px from the
edge, vertically centred in the 28 px bar.

Each icon is 16 px, stroked 1.5 px in `chrome.text_dim`, `chrome.text` on hover and while its
popover is open. They are drawn in code with gpui paths (no asset pipeline exists and two
icons do not justify one): the bell is an outline with a clapper; the doctor is a circle with a
plus inside. The dot is 6 px, `chrome.accent` (the glow colour), at the icon's top right,
present when there is something unread (bell) or something to fix (doctor). Under Reduce
Motion nothing here animates anyway; the hover colour is a plain change.

A press on an icon stops propagation so the bar's window-drag does not start, and toggles its
popover. The `status_items` function and `is_update_item` go; their tests go with them.

## 4. Notices (items 2, 3, 8)

### 4.1 The list

`notices.rs` holds `Notices { items: Vec<Notice>, opened_at: Option<Instant> }` and

```
pub struct Notice { pub kind: Kind, pub text: String, pub at: SystemTime, pub action: Option<Action> }
pub enum Kind { UpdateReady, Updated, Daemon, ConfigError, ThemeError }
pub enum Action { UpdateNow, OpenConfig, OpenTheme }
```

One notice per kind at most: setting a kind replaces its text and time; clearing a kind removes
it. The list renders newest first. A notice is unread while its `at` is later than `opened_at`;
opening the popover sets `opened_at` to now. The dot shows when any notice is unread, except
that `UpdateReady` keeps the dot until it is applied: an update waiting is always news.

### 4.2 Sources

The same places that set the bar's text today set the notices; the bar text itself is removed.

| Kind | Set when | Text | Cleared when |
|---|---|---|---|
| UpdateReady | `update_ready` becomes Some | "Update ready: 0.2.15" | `update_ready` becomes None, or `updating` starts |
| Updated | at startup when the version file (4.3) holds an older version than this binary | "Updated to 0.2.14" | the user opens the popover and closes it (read once), or the next launch |
| Daemon | `status` leaves the connected label | the status text as today ("arkosd reconnecting", "arkosd restarted; shells came back as new") | `status` returns to connected |
| ConfigError | `config_error` becomes Some | the banner text as today, with "Open config" | it becomes None |
| ThemeError | `theme_error` becomes Some | the banner text, with "Open theme" | it becomes None |

The config and theme banners under the title bar stay (they are errors, not news) and the
notice is in addition, so the popover shows what was wrong after the banner is gone. The
update error ("update failed: ...") also goes under Daemon as "update failed: ..." with no
action; "Update now" on the UpdateReady notice does what the badge did.

### 4.3 The version file (item 8)

`paths::last_version_file()` is `<app support>/last_version`. At startup, before the window
opens, the binary reads it; if it holds a version string different from `CARGO_PKG_VERSION` and
older by semver, the window starts with an `Updated` notice ("Updated to X") and an unread dot.
Then the file is written with the current version, atomically (temp file and rename, as the
workspace file is written). A missing or unreadable file means no notice and is written. A
newer version in the file (a downgrade) gives no notice.

### 4.4 The popover

Opens under the bell, right-aligned to it, 320 px wide, the card style of the context menus
(`banner` background, 1 px `border`, 6 px radius, 12 px text, 4 px vertical padding), with the
menu's lift and fade through `Transient`. Each notice is a row: the text, then the relative time
in `text_dim` ("just now", "3 min ago", "2 h ago", or the time for yesterday or earlier), then
the action as a small text button on the right where there is one. With no notices the card
shows one dim line, "Nothing new". The row for `UpdateReady` is the only one that is itself
clickable (it runs Update now); other rows are not buttons.

## 5. The doctor (items 4, 5, 6)

### 5.1 The checks

`doctor_panel.rs` holds a table of checks, each `Check { title, probe, action }`, probed when
the popover opens and after each action (never on a timer and never during render):

| Check | Probe | State text | Action |
|---|---|---|---|
| Claude Code: smooth wheel scrolling | `wheelScrollAccelerationEnabled` is `false` in Claude Code's settings file (`hook_cli::claude_settings_path`) | "on" or "off: fast scrolls jump in Claude Code" | "Apply": sets the key to `false` |
| Start at login | `launch_agent::installed` | "on" or "off: sessions do not come back after a restart" | "Install" or "Remove", the palette's actions; shown as fixed by config when `[recovery] start_at_login` pins it |
| Claude Code title hook | the hook entries are present in the settings file (`hook_cli` gains `hooks_installed_at(path, exe)`) | "installed" or "missing: tabs show no Claude Code state" | "Install", the palette's action |
| Accessibility for browser docking | `dock.trusted()` | "granted" or "not granted: browser tabs cannot dock" | "Open System Settings" (the pane URL the browser screen uses) |

A check whose probe cannot run (settings file unreadable, invalid JSON) shows "cannot read
<path>: <error>" and no action, so a broken file is never written to. The dot on the doctor
icon shows when any check is in its "off", "missing" or "not granted" state. The popover's
last row is "Copy report", which copies `doctor::report` for the current `Where` to the
clipboard; the palette's `arkos doctor` printout is the same text.

### 5.2 The settings edit (item 6)

`hook_cli::install_hooks_at` already reads the file, backs it up to `settings.json.bak`,
merges and writes. Its read, backup and write steps are lifted into
`hook_cli::edit_settings(path, |value| ...)`, used by both the hook install and the new
`set_wheel_scroll_acceleration(path, false)`. The edit sets exactly one top-level key and
leaves every other key, including unknown ones, as it was; pretty-printed with a trailing
newline as today. A missing file is created as `{ "wheelScrollAccelerationEnabled": false }`.
After the apply, the row reads "on; restart claude for it to take effect".

### 5.3 The popover

Same card as the notifications, 360 px wide, under the doctor icon. One row per check: the
title on the first line, the state text dim on the second, the action button on the right,
vertically centred. A row with nothing to do has no button. Actions run on the main thread
(they are small file operations and one `launchctl` call, as the palette runs them today) and
re-probe when done; a failure replaces the state text with the error in the error colour.

## 6. Popover behaviour (items 3, 5)

- One popover at a time: opening one closes the other.
- Closes on a press outside it (the root's `on_any_mouse_down`, as the menus do), on Escape
  (`escape_closes_menu` gains the two), on the icon again, and when the window loses focus.
- It does not take keyboard focus; the pane keeps typing focus and only Escape is caught.
- Opening the notifications popover marks everything read when it closes (4.1).
- Both are `Transient` values on `Root` (`notice_menu`, `doctor_menu`) rendered by
  `chrome::render_popovers` after the title bar, absolutely positioned under their icon with the
  bar's bounds known at render.

## 7. Palette (item 7)

Two commands, "Notifications" and "Doctor", each opening its popover; details show the unread
count ("2 unread") and the number of things to fix ("1 to fix"). The three existing setup
commands stay as they are.

## 8. Tests

- `notices`: one notice per kind replaces; newest first; unread follows `opened_at`;
  UpdateReady keeps the dot until cleared; relative time text at the boundaries.
- Version file: older gives Updated, same or newer gives nothing, missing gives nothing and is
  written, the write is atomic (a temp file is never left behind).
- `doctor_panel`: each probe on a fixture (settings present with the key true, false, absent;
  invalid JSON gives the "cannot read" state and `action` None); the dot rule.
- `hook_cli::edit_settings`: missing file created, other keys and nesting preserved, backup
  written, invalid JSON untouched with the error; `set_wheel_scroll_acceleration` sets the one
  key; the hook install still passes its existing tests through the shared edit.
- `chrome`: the right side renders two targets; a dot when asked; no `status_items` left.
- Palette: the two commands and their details.

## 9. README

Status rows: the bar shows two icons and no text; an update ready puts a dot on the bell and
"Update now" in its popover; after an update the bell says "Updated to X" once; a Claude Code
tab with `wheelScrollAccelerationEnabled` absent puts a dot on the doctor, Apply writes the key
and a `.bak`, and a restarted `claude` scrolls smoothly; the doctor's Start at login and hook
rows match the palette; Escape and a click outside close a popover; Reduce Motion shows them
without the lift. A "Claude Code tips" line under Config explains the scroll setting for anyone
who prefers to set it by hand.

## 10. Decisions left to the plan

- Whether the Daemon notice for "update failed" is worth its own kind. Default: no.
- Whether the bell's dot for ConfigError and ThemeError is unread-only or persistent like
  UpdateReady. Default: unread-only; the banner is the persistent signal.
