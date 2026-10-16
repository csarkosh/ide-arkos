# Arkos IDE: the menubar, About, Settings and the Timo Purple theme (milestone 8a)

Date: 2026-10-15. Status: draft for Cyrus's review. Builds on the title bar icons design
(`2026-10-15-title-bar-icons-design.md`) and the config and theme work of milestones 3 and 7.
Written against main at `f64c013` (0.3.0). Milestone 8b (the agent-first theme interface) follows
and relies on the theme schema and the `arkos theme` commands this milestone adds.

## 1. Why this exists

Arkos has no About and no Settings: the version hides behind `arkos --version`, the config is a
file you open in an editor, and the theme choice is a line in it. Cyrus wants the app to carry its
own identity (who made it, what build this is, where to send thanks) and a place where the settings
people change live, with themes first among them, so a team offsite demo can switch to a coworker's
theme in two clicks. The menubar also gets one menu, since the app has too few commands for more.

Rulings taken in chat on 2026-10-15:

- The menubar shows one menu, "Arkos IDE": About Arkos IDE, a separator, New Window, Settings,
  Quit, in that order. The File menu goes.
- About shows what a user and a developer both need; the attribution line at the bottom reads
  "Made by Cyrus Sarkosh" and links to github.com/csarkosh; under it a donation link "Buy me more
  tokens" with a Claude spark and a red heart (Cyrus supplies the URL; until then the link is
  hidden).
- "First installed" is a go-forward record: a marker written the first time this build runs.
- Settings holds the user-changeable settings, font size first; theme import is TOML only.
- The first shipped theme beyond navy is "Timo Purple", from Timo's Codex theme: accent `#00FFFF`,
  background `#330076`, contrast 70 (Codex export: base `[14.71, 69.09, 307.03]`, accent
  `[95.42, 129.32, 223.75]`, contrast 70); the green filled boxes in the screenshot are part of
  the look.

## 2. Scope

| # | Item | Where |
|---|---|---|
| 1 | One menu: About, separator, New Window, Settings, Quit | `main.rs` (`menus`), `keymap.rs` |
| 2 | The About window | new `about.rs`, `build.rs`, `paths.rs` |
| 3 | The first-installed marker | `paths.rs`, `startup.rs` |
| 4 | The Settings window: appearance, theme, text, startup, updates | new `settings.rs`, `config.rs` |
| 5 | Writing settings back to `config.toml` without losing comments | `config.rs` (`toml_edit`) |
| 6 | Theme import (TOML), the themes folder, `arkos theme list` and `arkos theme validate` | `settings.rs`, `theme.rs`, `main.rs` |
| 7 | The Timo Purple theme, built in | `crates/arkos/themes/timo-purple.toml`, `theme.rs` |
| 8 | README and the palette | `README.md`, `command_palette.rs` |

Out of scope: a keybinding editor (the file stays the place for keys); theme editing inside the
app (8b gives agents the interface; humans edit the TOML); a theme generator from accent and
background (Timo Purple is hand-derived once, in the file); per-workspace settings; syncing.

## 3. The menu (item 1)

`menus()` returns one `Menu { name: "Arkos IDE" }` with `MenuItem::action("About Arkos IDE",
keymap::About)`, `MenuItem::separator()`, `MenuItem::action("New Window", keymap::NewWindow)`,
`MenuItem::action("Settings", keymap::OpenSettings)`, `MenuItem::action("Quit", Quit)`. macOS puts
the app menu first and shows the bound keys itself. Keymap: "about" (no key), "settings"
(`cmd-,`); "open config" keeps its palette entry and loses its default key, since Cmd+, now opens
Settings (a config file that binds "open config" to `cmd-,` keeps it; the keymap's override rule
already handles that). The chooser window's menu is the same one; About and Settings work from it
too (they need no project window).

## 4. About (item 2)

A single window, title "About Arkos IDE", 420 by 520 px, not resizable, opened centred on the
main display, brought forward if it is already open (the chooser's `WindowHandle` pattern). Its
content, top to bottom, in the UI font:

- The app name "Arkos IDE" at 20 px semibold, the version under it ("0.3.0").
- A two-column table, 13 px, labels in `text_dim`:
  - Build: the short git hash (`ARKOS_GIT_HASH`) and the build date (`ARKOS_BUILD_DATE`, a new
    `build.rs` variable, UTC, "2026-10-15").
  - Last updated: the time the running build was put in place: `updates/applied.json`'s `at_unix`
    when the daemon applied it, else the bundle's modification time; "today 14:05" or a date.
  - First installed: the marker's date (section 5); "today" on the first run.
  - Daemon: its version and pid from the connection's Hello, or "not connected".
  - Protocol: `PROTOCOL_VERSION`.
  - macOS: the version string from `NSProcessInfo` and the architecture (`arm64`).
  - Paths: config file, themes folder, Application Support folder, the two logs. Each path is a
    row the user can click to reveal in Finder.
- "Copy details": puts the table as plain text on the clipboard (what a bug report needs).
- The attribution block at the bottom, centred: "Made by Cyrus Sarkosh" (the name links to
  https://github.com/csarkosh), and under it the donation link "Buy me more tokens ✦ ❤" where the
  spark is drawn in Claude's orange (`#d97757`) and the heart in red; the link opens
  `about::DONATE_URL`, a constant that is empty until Cyrus sets it, and the line is not drawn
  while it is empty.

Everything on the page comes from one pure `about::Details` struct built by `Root` (or the chooser)
so the text rows and the clipboard text are the same data; tests cover the formatting.

## 5. First installed (item 3)

`paths::installed_file()` is `<app support>/installed`. At startup, next to the version file,
`startup::installed_since(file, now) -> SystemTime` reads it, or writes the current time and
returns it when it is missing. The About window shows it as a date. An existing install therefore
reports the day it first ran this build; that is the go-forward ruling.

## 6. Settings (item 4)

A single window, title "Settings", 560 by 480 px, resizable, brought forward if open. A left
column of sections, the chosen section's controls on the right, in the UI font. Every control
writes the config at once (section 7) and the change applies live through the config watcher's
path (`reload_config`), so a theme or font change shows within the same second; the window also
calls `reload_config` directly so there is no visible wait.

Sections and controls:

- **Appearance.** Mode: three segments, System, Dark, Light (`[appearance] mode`). Dark theme and
  Light theme: two pickers listing `theme::names(themes_dir)` plus the built-ins, the current one
  marked (`[theme] dark`, `[theme] light`). "Add theme…": the native file picker filtered to
  `.toml`; the file is parsed with `theme::parse`; a valid one is copied to
  `<themes dir>/<name>.toml` (the name from the file's `name` field) and selected for the current
  mode; an invalid one shows the parse error under the button and copies nothing. "Show themes
  folder": reveals the folder in Finder.
- **Text.** Font family: a picker of the monospace families installed (gpui's
  `text_system().all_font_names()` filtered to fixed-pitch faces; the current value first even
  when it is not installed, marked "not installed"). Text preset: a picker of the presets in the
  config (`[font] preset`). Size and line height of the chosen preset: a stepper (10 to 32 px by
  1) and a stepper (1.2 to 2.0 by 0.1) writing `[font.presets.<name>]`.
- **Startup.** Start at login: a switch bound to `[recovery] start_at_login` (`on` or `off`;
  turning it on installs the launch agent as the palette command does, off removes it); the row
  shows "set by config to ask" with the switch off when the value is `ask`, and the first flip
  writes `on` or `off`.
- **Updates.** Automatic updates: a switch bound to `[update] auto`. A line under it with the
  staged version and "Update now" when one is ready (the bell's action, reused).
- **Keyboard.** One line: "Keybindings live in config.toml" with "Open config".

The window is a gpui window with its own entity (`settings::Settings`), the Root (or the chooser)
hands it the current config and a callback that writes and reloads; it reads the config file again
on `reload_config` so an edit made in an editor shows in the window too.

## 7. Writing the config (item 5)

Settings edits one value at a time in `config.toml` with the `toml_edit` crate (a new dependency;
it keeps comments, order and formatting), through `config::set_value(path, key: &[&str], value)`
which reads, edits and writes atomically (temp and rename, as the layout file). A key under a
table that the file lacks is added at the end of that table; a missing table is appended. The
file's comments and the user's other values are untouched; tests cover each case and a round trip
with the default config. An unparseable file makes every control read-only with the parse error
shown at the top of the window (the same banner text the app shows under the title bar).

## 8. Themes (items 6 and 7)

- The theme TOML schema is documented at the top of each built-in file and in a new
  `docs/themes.md` (every table and key, what each colour paints, the 4.5:1 rule the built-ins
  follow). This is the file milestone 8b hands to agents.
- `arkos theme list` prints the built-ins and the files in the themes folder, one per line, with
  "(built in)", "(file)" or "(file, invalid: <error>)"; `arkos theme validate <file>` parses one
  file and prints "ok: <name>" or the error, exit code 0 or 1. Both are headless and tested.
- "Timo Purple" ships as `crates/arkos/themes/timo-purple.toml`, name `timo-purple`, in
  `theme::BUILTIN`, so it appears in the pickers and the palette's "Theme:" entries. Its palette
  is derived by hand from the three Codex values and the screenshot:
  - window and terminal background `#330076`; title bar, sidebar, tab bar and inactive tabs a
    step darker `#2a0062`; the active tab the background; borders `#4b2a98`; the banner `#3b1a86`;
  - text `#f3edff`, dim text `#bfb0e6`; the focus ring, the terminal cursor and links the accent
    `#00ffff`;
  - the green filled boxes of the screenshot give the selection and the find-match colours
    (`#1a6b45` family) and the "working" mark; "waiting" is a warm yellow readable on purple;
  - the sixteen ANSI colours tuned for at least 4.5:1 against `#330076`, cyan being the accent.
  The exact values are the plan's to fix with a contrast test (every text colour at least 4.5:1
  against the surface it sits on, as `palette::contrast` measures); the look is checked by hand
  against the screenshot at the offsite demo.

## 9. README and the palette (item 8)

The README gets a "Menu" paragraph (what the one menu holds), an "About" and a "Settings" section
(what each shows and writes, that keybindings stay in the file, how to add a theme), the Timo
Purple line under Themes, and Status rows: the menu's five items; About's rows match
`arkos --version` and the paths exist; Copy details; the donation line hidden while the URL is
empty; Settings switches the theme live and the file keeps its comments; "Add theme…" with a bad
file shows the error and copies nothing; `arkos theme list` and `validate`; Timo Purple's look
against the screenshot. The palette gets "About Arkos IDE" and "Settings" entries (the keymap rows
give them their labels).

## 10. Tests

- `about::Details` formatting (dates, "today", "not connected", the clipboard text); the
  donation line hidden on an empty URL.
- `startup::installed_since`: missing file written and returned; existing file read; garbage
  treated as missing.
- `config::set_value`: comments kept, value replaced, key added to an existing table, table
  appended, atomic write, the default config round trip, an unparseable file refused.
- `theme`: `timo-purple` parses, every text colour passes the contrast rule, `BUILTIN` has three
  entries; the import validator; `arkos theme list` and `validate` outputs.
- Keymap and palette: the two new actions, counts, DEFAULT_CONFIG, the key change for "open
  config".
- Windows (About, Settings) by hand checks.

## 11. Decisions left to the plan

- Whether the Settings window is one gpui view with a section enum or one entity per section.
  Default: one view.
- The exact Timo Purple values (section 8) and whether the terminal's "green" is the box green or
  a brighter one for readability.
- Whether "Copy details" includes the recent update journal lines. Default: no.
