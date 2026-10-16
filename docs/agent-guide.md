# Arkos IDE: the guide for coding agents

You are inside Arkos IDE when `ARKOS_SESSION_ID` is set in your environment: every shell the Arkos
daemon starts carries it. If you are Claude Code, Arkos's SessionStart hook placed this guide in
your own config folder as the skill `arkos-ide` (`$CLAUDE_CONFIG_DIR/skills`, else
`~/.claude/skills`), and `arkos guide` prints the same text. Read it all before you change anything.

## 1. What you can change, and what you cannot

Arkos is a macOS IDE of terminal panes, a file tree and a markdown viewer. Through this guide you
may change its theme (the colours and the optional fonts) and the settings on a short allow-list.
Nothing else is supported: no opening files, no running commands in its terminals, no session
access. Do not edit the app bundle. Do not look for the built-in theme files: they are compiled in
and do not exist on disk. Keep the user's comments in `config.toml`; the commands below do that
for you, so never write the file by hand when a command exists.

## 2. Where things are

- The config: `~/.config/arkos/config.toml` (`$XDG_CONFIG_HOME/arkos/config.toml` when that is set).
- Theme files: `~/.config/arkos/themes/<name>.toml`. A file shadows a built-in of the same name.
- The daemon's socket: `~/Library/Application Support/arkos/arkosd.sock`.
- The logs, in the same folder: `window.log`, `arkosd.log`, and `updates/journal.log`.
- The binary: `/Applications/Arkos IDE.app/Contents/MacOS/arkos`. When `arkos` is not on your
  PATH, call it by that full path (quote it: the name has a space).
- This guide as a file: `arkos guide --path` writes it to the Application Support folder above
  and prints the path.

Every command that reads or writes files works with no window open and no daemon running, so you
can prepare a theme before Arkos is even started.

## 3. The theme file

One TOML file: a top-level `name`, five required tables of `#rrggbb` colours, and an optional
`[font]` table. Every colour key is required; an unknown or missing key is an error.
`arkos theme template --json` prints the full schema as data; the summary:

- `name`: the name `[theme] dark` and `[theme] light` use. `add` and `use` store the file as
  `<name>.toml` whatever the source file is called; a file placed in the themes folder by hand
  should be named `<name>.toml`. Letters, digits, `-` and `_` are the recommended characters.
- `[chrome]`: surfaces `window_bg`, `title_bar`, `status_bar`, `sidebar`, `tab_bar`,
  `tab_active`, `tab_inactive`, `banner`; lines `border`, `split_handle`, `split_handle_hover`;
  the accent `focus_ring`; text `text` and `text_dim`.
- `[terminal]`: `background`, `foreground`, `cursor`, `selection`, then the sixteen ANSI colours
  `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white` and their `bright_` forms.
- `[semantic]`: tab marks `working`, `waiting`, `bell`; backgrounds `error_banner`,
  `find_match`, `find_current`.
- `[tree]`: git status text in the file tree: `added`, `modified`, `deleted`, `untracked`, `ignored`.
- `[viewer]`: syntax colours: `keyword`, `string`, `comment`, `number`, `function`, `type_name`,
  `constant`, `operator`, `punctuation`.
- `[font]` (optional): `prose`, `ui`, `terminal`, each an installed family name; `terminal` must
  be monospaced. A family that is not installed is left out.

The contrast rules, which `arkos theme contrast` checks pair by pair:

- `chrome.text` needs 7:1 on `window_bg` and 4.5:1 on `title_bar`, `banner` and `tab_inactive`.
- `terminal.foreground` needs 11:1 on `terminal.background` and 4.5:1 on `selection`,
  `semantic.find_match` and `semantic.find_current`.
- Every other text or mark colour needs 4.5:1 on the surface it sits on: `text_dim` on
  `window_bg`, `status_bar`, `tab_bar`, `banner`, `sidebar` and `tab_inactive`; the ANSI colours
  and the semantic marks on `terminal.background`; the tree and viewer colours on `window_bg`.
- `terminal.black` is checked only on a light theme, where it is a text colour.

The live reload: every window watches `config.toml` and the file of the theme the config names. A
saved edit applies within a second. A file that stops validating keeps the last good theme and the
window's banner names the file and the error until the next good save. While `config.toml` itself
is broken, a theme file edit waits: it applies once the config loads again.

## 4. The commands

Exit codes, for every command: 0 done; 1 refused or failed (a bad file, an unknown name, a low
contrast pair, a window that reported an error, an old daemon); 2 usage (the usage text is on
stderr); 3 a window was needed and none is running. `--json` anywhere prints one JSON value on
stdout and nothing else. Errors are plain lines on stderr. Window lines read
`window {pid} ({project}): {text}`, `window {pid}: no answer` or
`window {pid}: too old for this command`.

- `arkos theme list [--json]`: every theme with its source. JSON
  `[{ "name", "source": "built-in" | "file" | "invalid", "error"?, "path"? }]`.
- `arkos theme validate <file> [--json]`: `ok: {name}`, or the error and exit 1. JSON
  `{ "ok": true, "name" }` or `{ "ok": false, "error" }`.
- `arkos theme template [<built-in>] [--json]`: a built-in's TOML (default `navy`); an unknown
  built-in exits 1. `--json` gives the schema instead:
  `{ "tables": [{ "name", "keys": [{ "key", "meaning", "contrast_with"?, "needs"? }] }] }`.
- `arkos theme contrast <file-or-name> [--json]`: one line per pair, `{what} {ratio}:1 ok` or
  `{what} {ratio}:1 LOW (needs {needs})` with the ratio to one decimal, as in
  `tree.ignored on window_bg 4.2:1 LOW (needs 4.5)`; exit 1 when any pair is low. An argument ending in
  `.toml` is a file. JSON `{ "name", "ok", "pairs": [{ "what", "fg", "bg", "ratio", "needs", "ok" }] }`.
- `arkos theme add <file> [--replace] [--json]`: copies the file into the themes folder:
  `added {name}` or `replaced {name}`; a name taken by a file or a built-in prints
  `exists: {name}; pass --replace` and exits 1. JSON `{ "name", "replaced" }`.
- `arkos theme use <name-or-file> [--dark|--light] [--replace] [--json]`: a `.toml` path is added
  first (`exists: {name}; pass --replace`, exit 1, when that name is already in the themes
  folder: running `use draft.toml` again after more edits needs `--replace`); the name must load (else `unknown theme: {name}` or the file's error, exit 1, nothing
  written); then `[theme] dark` or `light` is written (the key from the flag, else from
  `[appearance] mode`, else from the window's system appearance, else dark) and every window
  reloads. Lines: `window {pid} ({project}): {name} applied`; a window whose active key is the
  other one prints `window {pid} ({project}): saved for {key} mode; shows {theme} in {active} mode`
  (exit 0 unless it reports a theme or config error, which is printed instead and exits 1); a
  window that shows another theme in the written key's mode prints `shows {other}` and exits 1.
  No window: `saved; no window running`, exit 0. No window answered (all silent or too old):
  `saved; no window answered`, then their `no answer` / `too old for this command` lines, exit 0.
  `use` refuses a `config.toml` that does not load. JSON
  `{ "name", "key", "saved": true, "windows": [{ "pid", "project", "theme"?, "error"? }], "missing": [pids], "too_old": [pids] }`.
- `arkos theme preview <name-or-file> [--json]` and `arkos theme preview --end [--json]`: shows a
  theme in every window without writing anything; the TOML is validated first. Lines
  `window {pid} ({project}): previewing {name}` and `window {pid} ({project}): preview ended, {theme}`.
  Exit 3 with no window, 1 when none answered. JSON `{ "windows", "missing", "too_old" }`. It ends at
  `--end`, at the next config or theme reload, when the system appearance changes, or when the
  window closes. Ending one while the
  configured theme's file is broken keeps the preview's colours on screen with a theme error set.
- `arkos theme current [--json]`: `config: dark {d}, light {l}, mode {m}`, then per window
  `window {pid} ({project}): {theme} ({dark|light}, system is {dark|light}), preset {preset}, family {family}`,
  plus `, preview {name}`, `, config error: {text}` and `, theme error: {text}` when set. Exit 0
  with no window. JSON `{ "config": { "dark", "light", "mode" } | { "error" }, "windows": [...], "missing", "too_old" }`,
  each window `{ "pid", "project", "theme", "dark", "system_is_dark", "preset", "family", "preview", "config_error", "theme_error" }`.
- `arkos settings get [<key>] [--json]`: no key prints the config file verbatim, then the window
  lines as `current` does (JSON `{ "file", "windows" }`). A dotted key prints its value bare
  (JSON `{ "key", "value" }`). It reads the file only, so a key left at its default answers
  `no such key` (exit 1).
- `arkos settings set <key> <value> [--json]`: the keys are `appearance.mode`
  (system, dark, light), `theme.dark`, `theme.light`, `font.family`, `font.preset`,
  `font.presets.<name>.size` (8 to 48), `font.presets.<name>.line_height` (1.0 to 2.5),
  `update.auto` (true, false) and `recovery.start_at_login` (ask, on, off). Another key exits 2
  with the list. A bad value is refused before anything is written. Then every window reloads:
  `window {pid} ({project}): {key} applied` or its config error (exit 1); `saved; no window running`
  and `saved; no window answered` as for `use`. Unlike `use`, it repairs a broken `config.toml`
  when the edit makes it load. JSON `{ "key", "value", "saved", "windows", "missing", "too_old" }`.
- `arkos config reload [--json]`: asks every window to read its config now:
  `window {pid} ({project}): reloaded, {theme}` or its errors. Exit 3: no window; 1: none answered.
- `arkos guide [--path | --install]`: prints this guide; `--path` writes it to a file and prints
  the path; `--install` installs the skill into your Claude config folder and prints a line to
  paste into `AGENTS.md` or `CLAUDE.md`.

## 5. The workflow

1. `arkos theme template > draft.toml` (or `arkos theme template navy-light` for a light start).
2. Set `name` to a new bare name and edit the colours.
3. `arkos theme validate draft.toml`, then `arkos theme contrast draft.toml`; fix every LOW pair
   and run it again until it exits 0.
4. `arkos theme preview draft.toml`, so the user sees it; then `arkos theme preview --end` if they
   do not want it.
5. `arkos theme use draft.toml` (add `--dark` or `--light` to choose the slot).
6. `arkos theme current` to confirm what every window shows.

To revert, `arkos theme use navy` (or `arkos theme use navy-light --light`). The user can also
pick any theme by hand in the Settings window.

## 6. A worked example: Timo Purple

The brief was an accent, `#00ffff`, and a background, `#330076`. The built-in `timo-purple` is
the result (`arkos theme template timo-purple` prints it). The choices, and why each passes:

- `window_bg` and `terminal.background` are the background `#330076`. The bars, sidebar and
  inactive tabs are a step darker, `#2a0062`, and `banner` a step lighter, `#3b1a86`, so the
  layout reads without borders.
- `chrome.text` and `terminal.foreground` are a near-white with a violet tint, `#f3edff`: 13.1:1
  on the background, clearing both 7:1 and 11:1, and 11.0:1 on the banner.
- `chrome.text_dim` is `#c4b6ea`: 8.0:1 on the background and 6.7:1 on the banner, its hardest
  surface.
- The accent goes where a mark is wanted: `focus_ring`, `cursor` and the ANSI `cyan` are
  `#00ffff`, 11.9:1 on the background.
- The ANSI colours are pastels, since saturated reds and blues sink into a deep purple: red
  `#ff8a9e` 6.7:1, blue `#8fb8ff` 7.5:1; `bright_black` `#9a86d0` sits close to the line at 4.8:1.
- `tree.ignored` and `viewer.comment` are the dimmest text, `#a08ed6`, at 5.2:1.
- `selection` `#1a6b45`, `find_match` `#15553a` and `find_current` `#1f7a52` are greens dark
  enough for the foreground to keep 5.7:1, 7.7:1 and 4.6:1 on them; the accent was too light to
  carry white text, so it is not used as a background.

## 7. Rules

- Validate before you preview, and preview before you use.
- Never leave a preview running: end it, or make it permanent with `use`.
- Never write `config.toml` by hand when a command exists; never edit the app bundle.
- Prefer `--json` when you parse output, and branch on the exit code.
- When you finish, run `arkos theme current` and report its lines to the user.
