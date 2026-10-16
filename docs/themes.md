# Themes

A theme is one TOML file with a `name` and five tables: `chrome`, `terminal`, `semantic`, `tree` and `viewer`. Every key is required and every value is a `#rrggbb` colour; an unknown or missing key is an error. Arkos ships three themes built in (`navy`, `navy-light`, `timo-purple`); the files in `crates/arkos/themes/` are complete examples to copy.

## Contrast rules

The theme tests hold every built-in to two rules, and a theme of your own should meet them too.

- Every colour drawn as text or a mark must reach 4.5:1 contrast against the background it sits on (the WCAG AA ratio for normal text).
- The terminal text (`terminal.foreground` on `terminal.background`) must reach 11:1, which is about the 12:1 the design asks for.

Black is a background colour on a dark theme and a text colour on a light one, so `terminal.black` is checked against `terminal.background` only on a light theme.

## name

The theme's own name, a bare file stem: no slashes, not `.` or `..`, not starting with a dot. It is the name used in `[theme] dark` and `[theme] light` in `config.toml`, and the file is stored as `<name>.toml`.

## [chrome]

The window around the terminal.

| Key | What it paints | Must contrast with |
| --- | --- | --- |
| `window_bg` | The window background behind everything | `text`, `text_dim`, `focus_ring` |
| `title_bar` | The title bar | `text` |
| `status_bar` | The status bar at the bottom | `text`, `text_dim` |
| `sidebar` | The file tree and project sidebar | `text`, `text_dim`, the `tree` colours |
| `tab_bar` | The strip behind the tabs | `text_dim` |
| `tab_active` | The selected tab | `text` |
| `tab_inactive` | The other tabs | `text`, `text_dim` |
| `border` | Lines between panes and panels | none (a divider, not text) |
| `split_handle` | The draggable divider between split panes | none |
| `split_handle_hover` | The same divider under the pointer | none |
| `focus_ring` | The outline and accent on the focused item | `window_bg` |
| `text` | Primary interface text | `window_bg`, `tab_active`, `tab_inactive` |
| `text_dim` | Secondary text such as hints and paths | `window_bg`, `sidebar`, `tab_inactive`, `banner` |
| `banner` | The background of notice banners | `text`, `text_dim` |

## [terminal]

The terminal grid. `background`, `foreground`, `cursor` and `selection` are the base; the sixteen ANSI colours follow.

| Key | What it paints | Must contrast with |
| --- | --- | --- |
| `background` | The terminal background | `foreground` (11:1) and every ANSI colour |
| `foreground` | Default terminal text | `background`, `selection`, `find_match`, `find_current` |
| `cursor` | The cursor block | `background` |
| `selection` | The background of selected text | `foreground` |
| `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white` | ANSI colours 0 to 7 | `background` |
| `bright_black`, `bright_red`, `bright_green`, `bright_yellow`, `bright_blue`, `bright_magenta`, `bright_cyan`, `bright_white` | ANSI colours 8 to 15 | `background` |

## [semantic]

Colours that carry meaning in the interface.

| Key | What it paints | Must contrast with |
| --- | --- | --- |
| `working` | The mark on a tab whose agent is working | `window_bg` |
| `waiting` | The mark on a tab waiting for you | `window_bg` |
| `bell` | The mark on a tab that rang the bell | `window_bg` |
| `error_banner` | The background of an error banner | `text` |
| `find_match` | The background of a search match in the terminal | `terminal.foreground` |
| `find_current` | The background of the current search match | `terminal.foreground` |

## [tree]

Git status colours in the file tree. Each is text on `chrome.sidebar` and must reach 4.5:1 there.

| Key | What it paints |
| --- | --- |
| `added` | Files added to the index |
| `modified` | Files changed in the working tree |
| `deleted` | Files removed |
| `untracked` | Files git does not know yet |
| `ignored` | Files matched by an ignore rule |

## [viewer]

Syntax colours in the file viewer. Each is text on `terminal.background` and must reach 4.5:1 there.

| Key | What it paints |
| --- | --- |
| `keyword` | Language keywords |
| `string` | String literals |
| `comment` | Comments |
| `number` | Number literals |
| `function` | Function names |
| `type_name` | Type names |
| `constant` | Constants |
| `operator` | Operators |
| `punctuation` | Brackets, commas and other punctuation |

## Adding a theme

1. Copy a built-in as a starting point. "Extract built-in themes" in the command palette writes them into the themes folder (`~/.config/arkos/themes/`), or copy one from `crates/arkos/themes/`.
2. Give the copy its own `name` and edit the colours.
3. Check it with `arkos theme validate <file>`. It prints `ok: <name>` when the file parses and names the problem otherwise. `arkos theme list` shows every theme and marks a broken file as invalid.
4. Put the file in the themes folder as `<name>.toml`, or use "Add theme" in Settings, which does the same copy.
5. Pick it in Settings, or set `dark` or `light` under `[theme]` in `config.toml`. A file in the folder with a built-in's name replaces that built-in.
