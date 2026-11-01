# Contrast matrix: plan

Spec: `docs/superpowers/specs/2026-11-01-contrast-matrix-design.md`. Each task starts with its
tests.

1. **`contrast` module.** Move `legible`, `dim_alpha` and their constants out of
   `terminal_element.rs` into `crates/arkos/src/contrast.rs`, add `drawn(fg, bg, dim)`, and
   have `text_run` and the underline use it. Tests: a drawn colour is the composite the painter
   makes; the existing floor and fade tests move with the code.
2. **The fixture.** `scripts/claude-code-colours.py` extracts the six stock themes' colours and
   the diff renderer's from a Claude Code binary into `crates/arkos/data/claude-code-colours.json`;
   `--check` compares. Rust reads it with `include_str!`. Tests: it parses, has all six themes
   and the diff colours; an ignored test runs `--check` against the installed Claude Code.
3. **Claude Code pairs.** `claude_theme::contrast_pairs(theme)`; the four band tests call it.
4. **The matrix.** `contrast::terminal_report(theme)`: foregrounds and backgrounds, plain and
   faint. Tests: every built-in theme draws every pair clear, plain and under each built-in
   effect; a renderer without the floor would fail (the report finds a stock colour under
   4.5:1 when drawn raw).
5. **`arkos theme contrast`.** The Claude Code rows, the terminal summary and its LOW lines,
   `--effect`, and the JSON keys `claude_code` and `terminal`. Tests in `agent_cli.rs`.
6. **Telemetry.** `text_moved` in `telemetry/events.toml`, its "when" line, `Recorder::text_moved`
   and `telemetry::text_moved`, called once per paint that moved a colour. Re-render
   `docs/telemetry.md`. Tests: the allowlist admits each value; a repeat in one report writes
   nothing.
7. **Docs.** Agent guide sections 4, 5, 6 and 9; the user guide; `docs/development.md`, Tests
   (the matrix and refreshing the fixture).
