# Overworld, the whole window: plan

Spec: `docs/superpowers/specs/2026-11-12-overworld-whole-window-design.md`. Branch `timo-world`.
Each task writes its tests first and ends with `cargo test -p arkos` green for what it touched.

## Task 1. The wash is told which pane has the focus and which waits

- `motion::Fades::opacity(&key, now)`: a key's opacity without changing what it wants.
- `SessionManager::waiting_panes(&tree)`, beside `working_panes`.
- `blocks::Block` and `inputs::ShaderBlock` gain `focus` and `waiting`; `blocks::Scene` gains
  `focus: &[(PaneId, f32)]` and `waiting: &[PaneId]`; a pane's tab bar and body carry them, every
  other block 0. `arkos.metal`'s `ArkosBlock` declares them in the spare floats (still 64 bytes).
- `Root::effect_tick` fills them from `root.glow` and the sessions.
- Tests: the block's offsets and size; a focused and a waiting pane's blocks carry the values and
  no other block does; `waiting_panes`; `Fades::opacity`.

## Task 2. The wash

- `effects/overworld/wash.metal`: the ground, the frames (notched, the focused one in the focus
  colour, a busy one's glint), the ledge and its doorway, the title sky with its stars and cloud,
  the cursor on the selected row, `MUTE` 0.1, the night behind a terminal (dusk, stars, hills),
  the continue arrow.
- `effects/overworld/effect.toml`: `gap = 15`, `ink_borders = true`, `paper_alpha`.
- `src/effects/overworld.rs` mirrors the new layout arithmetic (the frame's rectangle, the glint's
  place, the arrow's place, the cloud's place, the title's stars) and the constants test covers
  the new constants.
- Tests, on the GPU where the layout matters:
  - the frame is drawn in the gap and nowhere inside the pane;
  - the focused pane's outer line is the focus colour, another pane's is not;
  - behind a terminal nothing but the stars is lighter than the background, and a star is under
    its limit after the paper;
  - a document's body is plain;
  - the title bar is plain under its groups, stars and cloud included, at several times;
  - the names in the sidebar sit on plain colour but for the cursor in the left padding;
  - the glint stays on the frame; the arrow shows only on a waiting pane;
  - at time 0 everything is lit.

## Task 3. The text

- `effects/overworld/glyphs.metal`: gpui's glyph, hidden before a stamp's start, flashed for
  150 ms after it, every other stamp plain. `terminal_paint = true` in the manifest.
- Tests: the built-in loads and compiles (`effects::cli` checks every built-in); the shader names
  its flash as short3; the terminal's paint variants it must handle are all named in it.

## Task 4. Checks and words

- `arkos effect bench overworld` within budget; `arkos theme contrast` for timo-purple, navy,
  peach-watercolor and composition-notebook with `--effect overworld`; the contrast matrix.
- A throwaway shoot storyboard: Timo Purple with two terminals, a document and a selected file;
  then one light theme. Look, fix, reshoot.
- `docs/guide.md`, `docs/agent-guide.md` (and the installed skill's source of it), `docs/themes.md`,
  `docs/development.md` Status rows. The effect's description in `effect.toml`.
- `cargo test --workspace`, `cargo check --workspace --all-targets` with no warnings.
