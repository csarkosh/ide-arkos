# Arkos-matched Claude Code colours

Status: approved by Cyrus in chat, 2026-10-28 (option "A+").

## Intent

Claude Code in an Arkos tab should always read as part of the Arkos theme, including lines it
printed before the theme changed. Today Claude Code's `auto` theme draws with fixed colours for a
dark or a light background; when Arkos switches (for example macOS going dark at sunset while the
config fixes light mode), new lines follow but the lines already on screen keep the old theme's
colours: a dark green diff block and pale grey text on Peach Light's cream (Cyrus's screenshots,
2026-10-28).

## How

Claude Code accepts colours from the terminal's palette in a custom theme (`ansi:<name>` for the
16, `ansi256(n)` for the 256), reloads theme files live, and a terminal redraws palette-indexed
cells in its current palette. So Arkos:

1. **Derives the 256-colour palette from the theme** (Cyrus chose this, including the cube):
   - Indices 0 to 15 stay the theme's own ANSI colours.
   - The 216-colour cube (16 to 231) is generated from the theme: cube coordinates (r, g, b),
     each 0 to 5, are a trilinear blend between eight corners, interpolated in Oklab: (0,0,0) is
     the terminal background, (5,5,5) the foreground, (5,0,0) ANSI red, (0,5,0) green, (0,0,5)
     blue, (5,5,0) yellow, (5,0,5) magenta, (0,5,5) cyan. So low steps are faint tints of the
     background toward a hue, in any theme.
   - The grey ramp (232 to 255) runs in 24 even Oklab steps from the background (232 is one
     step off it, not equal) to the foreground (255 is one step short of it).
   - This replaces the fixed xterm values in `Palette::indexed` for every program, a known and
     accepted effect (Cyrus, 2026-10-28): a program's 256-colour scheme now takes the theme's
     colours.
2. **Writes a Claude Code theme** `~/.claude/themes/arkos.json`, `"name": "Arkos"`, built so it
   reads well on every Arkos theme with no rewrite on a switch:
   - `base` is `dark-ansi`, and every documented override key is set explicitly, so nothing is
     left to the base's choices.
   - Text-like keys use ANSI slots 1 to 15 only, never `ansi:black` (slot 0 is a background in
     dark themes; every Arkos theme holds slots 1 to 15 to 4.5:1 on its background, checked by
     `theme::contrast_pairs`). Normal text `ansi:whiteBright`, dim and inactive text
     `ansi:blackBright`, Claude's own colour `ansi:redBright` (as the ansi bases), success green,
     error red, warning yellow, suggestion and permission blue, and so on as the ansi bases
     choose, moved off `black` where they use it.
   - Background-like keys use the derived indices: the user message background and the memory
     and bash message backgrounds a faint grey (`ansi256(234)` to `ansi256(235)`, two or three
     steps off the background), its hover one step more; the selection a faint blue
     (`ansi256(18)`); diff added and removed bands faint green and red (`ansi256(22)`,
     `ansi256(52)`), their dimmed forms the same, and the changed words one step stronger
     (`ansi256(28)`, `ansi256(88)`).
   - The builder picks the remaining keys by the same two rules, from Claude Code's documented
     key list (code.claude.com/docs/en/terminal-config), and records the table in
     `docs/guide.md`.
   - Arkos writes the file at startup when its content differs (so an Arkos update can change
     it), atomically, and never touches any other file in `~/.claude/themes`.
3. **Selects it.** At startup, where Arkos today sets a stock Claude Code theme to `auto`
   (`follow_theme_in_claude`, `hook_cli::set_claude_theme_auto_at`), it sets `theme` to
   `custom:arkos` instead, when the current value is unset, `dark`, `light` or `auto` (`auto` is
   what Arkos itself set until now). Any other value (daltonized, an ansi base, another custom
   theme) is a deliberate choice: left alone, and the doctor's row offers the switch as it offers
   `auto` today.

## Non-goals

- No change to Claude Code itself; no escape sequences beyond what Arkos already answers.
- No recolouring of cells drawn with 24-bit colours (only palette-indexed cells follow a switch).

## Anti-goals

- Text in any built-in theme that falls under 4.5:1 because of this theme (a test holds it).
- Touching a user's own Claude Code theme choice without asking.

## Tests the builder adds (named as sentences)

- The cube's corners are the theme's own colours, and its low steps sit between the background
  and their hue, for Navy and Peach Light.
- The grey ramp runs from just off the background to just short of the foreground, evenly.
- Every text-like key of the generated Claude theme names a slot 1 to 15, and every such slot
  reaches 4.5:1 on each built-in theme's background; every background-like key under the user
  message's text (`ansi:whiteBright`) keeps 4.5:1 on each built-in theme.
- The theme file is written when missing or different and left untouched when equal.
- A stock or `auto` Claude theme becomes `custom:arkos`; any other is left alone.

## Hand checks (docs/development.md Status)

Claude Code in Navy then Peach Light then back: old and new lines both take the current theme,
diffs show green and red bands in both; the user message band is a faint tint; vim or htop with a
256-colour scheme takes the theme's colours.

## Rulings from the builder check (2026-10-28, my calls)

1. `ansi256(n)` stays a palette index: Claude Code colours through chalk, whose `ansi256` model
   writes `38;5;n` / `48;5;n` at every colour level (seen in the 2.1.312 bundle). The plan still
   runs a live probe (Claude Code started on `custom:arkos` in a pseudo-terminal, its output
   scanned for `38;5;` and `48;5;` with the chosen indices) before the generator is relied on.
2. The shoot uses `custom:arkos` (the file the dev Arkos it runs writes at startup) instead of its
   own `arkos-shoot.json`, so the screenshots show the shipped look; `claude_theme_for` and the
   story's `claude_theme` key give way to it. The generator lives in the `arkos` crate.
3. `arkos.json` is Arkos's own file and is rewritten whenever it differs; the guide says to fork
   it under another name (Claude Code's theme editor can) for hand edits.
4. If `~/.claude/themes` did not exist, Claude Code watches it only after a restart; the guide
   says so, and Arkos's notice for the first switch says "restart running Claude Code sessions to
   take the Arkos colours".
