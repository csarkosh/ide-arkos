# Arkos IDE

A macOS IDE for Claude Code sessions: Rust, gpui, a daemon (`arkosd`) that owns the shells. The
workspace's crates live in `crates/`; the app is `crates/arkos`.

## Conventions for agents working on Arkos

- Read `docs/development.md` first: how to build and run, the tests, the Motion rules and the
  Status checklist (what only a person at the screen can check; add a row when you add such a
  thing). `docs/guide.md` is the user guide and `docs/agent-guide.md` the agent CLI; keep them
  true when behaviour changes.

- Designs live in `docs/superpowers/specs`, implementation plans in `docs/superpowers/plans`.
  A feature starts from a spec there; a plan names its tasks.
- Motion rule: every transition and animation is Google Material 3 compliant: its duration is
  one of Material's duration tokens and its curve one of Material's easing tokens, entering on a
  decelerate curve, leaving on an accelerate one and a little quicker than it came (the tokens
  and the full rule are in `docs/development.md`, Motion). It goes through
  `crates/arkos/src/motion.rs` (Reduce Motion means instant), and a new animated thing adds itself
  to `Root::any_motion`. Every colour is read from `root.theme` at paint time and
  never cached, because for 200 ms after a theme switch the theme is a blend remade each frame;
  anything derived from the theme once (highlighter spans, a program's theme push) is done from
  the final theme in `Root::finish_theme`, never per frame.
- Colour: a change to a theme, the palette, the terminal's drawing or an effect is checked by the
  contrast matrix in the tests and by `arkos theme contrast <theme> [--effect <name>]` on the
  themes it touches (`docs/development.md`, Tests). Never loosen a contrast floor to pass.
- Tests are headless (`cargo test --workspace`), named as sentences, and written before the code
  they check. Doc comments are plain prose; no em-dashes. Finish with
  `cargo check --workspace --all-targets` showing no warnings.
- Builds on the development MacBook Air are run with `nice -n 15`, never `--release`, and one
  cargo at a time; a worktree keeps the shared `target-dir` in its `.cargo/config.toml`.
- Disk: the shared target folder is trimmed by `scripts/trim-target.py` (the shoot and the
  repository's Claude Code hook run it when due; run it by hand after a big build). Never give a
  worktree its own target folder or turn debug info back on for good. Throwaway output (shoot
  captures, sandbox binaries) goes in the session's scratch folder and is deleted when done. See
  `docs/development.md`, Disk.
