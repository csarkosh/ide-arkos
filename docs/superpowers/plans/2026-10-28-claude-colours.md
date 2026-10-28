# Arkos-matched Claude Code Colours Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Claude Code in an Arkos tab always takes the current Arkos theme's colours, including lines printed before a theme switch.

**Architecture:** The terminal's 256-colour palette is derived from the theme (cube by Oklab trilinear blend of background, foreground and the six hues; grey ramp from background to foreground). Arkos writes one fixed Claude Code theme, `~/.claude/themes/arkos.json`, whose colours are all palette references, and selects it when the user's Claude Code theme is stock or `auto`.

**Tech Stack:** Rust, serde_json, alacritty_terminal's colour types.

**Spec:** `docs/superpowers/specs/2026-10-28-claude-colours-design.md` (with its rulings).

## Global Constraints

- Builds: `nice -n 15 ~/.cargo/bin/cargo` via the scratchpad `ct.sh`, never `--release`, one cargo at a time.
- Tests headless, named as sentences, written first. Doc comments plain prose, no em-dashes. `cargo check --workspace --all-targets` warning-free.
- Cube corners: (0,0,0) background, (5,5,5) foreground, (5,0,0) red, (0,5,0) green, (0,0,5) blue, (5,5,0) yellow, (5,0,5) magenta, (0,5,5) cyan; trilinear in Oklab. Grey ramp 232..255: 24 even Oklab steps strictly between background and foreground.
- Claude theme: `name` "Arkos", `base` "dark-ansi", every documented override key set; text-like keys only `ansi:` slots 1 to 15 (never `ansi:black`); background-like keys `ansi256(n)`: user message 234, hover 235, memory and bash message 234, selection 18, diff added 22, removed 52, dimmed forms the same, changed words 28 and 88.
- Select `custom:arkos` only when the Claude Code theme is unset, `dark`, `light` or `auto`.
- Never touch other files in `~/.claude/themes`; tests use temp dirs, never the real `~/.claude`.

## Review Focus

1. A light theme's grey ramp runs light to dark (232 near the cream background), so Claude's faint bands stay faint on Peach Light.
2. Every text-like key reaches 4.5:1 on every built-in theme's background, and text on the user-message band keeps 4.5:1 (Task 2 test).
3. A user's own Claude theme (daltonized, ansi, custom) is never replaced (Task 2 test).
4. The file is not rewritten when unchanged (no needless Claude Code reload; Task 2 test).
5. Programs using the 256 colours see the theme's colours: the cube's low steps are tints, not black (Task 1 test).

---

### Task 1: The 256 colours from the theme

**Files:** Modify `crates/arkos/src/palette.rs` (`Palette` gains `cube: [Rgb; 216]` and `ramp: [Rgb; 24]` filled by `Palette::derive_256(&mut self)` or computed in a constructor; `indexed` reads them), `crates/arkos/src/theme.rs` (`Theme::palette` fills them), the `NAVY` constant's users, docs/development.md (a Status row: vim or htop with a 256-colour scheme takes the theme's colours).

**Interfaces:** Produces `pub fn oklab_mix(a: Rgb, b: Rgb, t: f32) -> Rgb` and `Palette::indexed(i)` returning derived colours for 16..=255.

- [ ] Tests: `the_cube_s_corners_are_the_theme_s_own_colours`, `the_cube_s_low_steps_are_tints_of_the_background_toward_their_hue` (index 22 lies between bg and green, nearer bg; for Navy and Peach Light), `the_grey_ramp_runs_evenly_from_just_off_the_background_to_just_short_of_the_foreground` (232 != bg, 255 != fg, monotonic in Oklab lightness, both directions for dark and light themes). Replace `indexed_cube_and_grey_ramp`'s fixed xterm values.
- [ ] RED, implement, GREEN; full `ct.sh test -p arkos`; check clean; commit.

### Task 2: The Arkos Claude Code theme, selected

**Files:** Create `crates/arkos/src/claude_theme.rs` (`pub fn arkos_theme_json() -> serde_json::Value`, `pub fn ensure_theme_file(dir: &Path) -> Result<bool, String>` writing atomically only when different). Modify `crates/arkos/src/hook_cli.rs` (`claude_theme_is_stock` adds `auto`; `set_claude_theme_at(path, value)`), `crates/arkos/src/main.rs` (`follow_theme_in_claude` writes the file and selects `custom:arkos`; a notice on the first switch: "Claude Code now takes the Arkos colours; restart running sessions to see them"), `crates/arkos/src/doctor.rs` (its row offers `custom:arkos`), `crates/shoot/src/main.rs` and its story keys (use `custom:arkos`; drop `claude_theme_for`'s file), notices and telemetry lists if a new notice kind is needed (follow `ThemeRetired`'s pattern), `docs/guide.md` (the key table and the fork advice), `docs/development.md` (Status rows from the spec).

- [ ] Tests (sentences): every text-like key is an `ansi:` name other than `black`; every slot so named reaches 4.5:1 on each built-in theme's terminal background; text (`ansi:whiteBright`) on each background-like `ansi256` index keeps 4.5:1 on each built-in theme; the file is written when missing or different and untouched when equal (mtime unchanged); a stock or `auto` theme becomes `custom:arkos` and any other is left alone; the doctor row's offer.
- [ ] RED, implement, GREEN.
- [ ] Probe (controller runs it after the commit): `claude --settings '{"theme":"custom:arkos"}'` in a pseudo-terminal for a few seconds with the file written to a temp `CLAUDE_CONFIG_DIR` copy of nothing else, output scanned for `38;5;` / `48;5;` with the chosen indices and for the absence of `48;2;` on the message band. If it fails, stop and report.
- [ ] Full tests, check clean, commit.
