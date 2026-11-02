# Composition Notebook and Material: plan

Spec: `docs/superpowers/specs/2026-11-01-notebook-and-material-themes-design.md`. Each task
starts with its tests. Branch `light-themes` in `.worktrees/light-themes`.

1. **The theme files.** `crates/arkos/themes/material.toml` and
   `crates/arkos/themes/composition-notebook.toml`, added to `theme::BUILTIN`. Tests: each is
   built in, light, recommends its effect, and names its fonts; the built-in count is eight
   wherever a test says six; the contrast tests cover them through `BUILTIN`.
2. **The contract's row grid.** `ArkosBlock.row_pitch` and `row_top` in `arkos.metal` and
   `inputs::ShaderBlock` (the two padding floats after `busy`), `blocks::Block` and
   `Scene.grids`, filled in `effects::active` from each terminal's painted origin and the
   window's cell height. Tests: a pane body whose active tab is a terminal carries its pitch and
   top; any other block 0; the layout is still 64 bytes.
3. **The material effect.** `crates/arkos/effects/material/{effect.toml,wash.metal}`; the
   ripple's and the progress bar's curves mirrored in Rust for their tests. `arkos effect
   check`, `bench`, `arkos theme contrast material` and `navy --effect material` clear.
4. **The notebook effect.** `crates/arkos/effects/notebook/{effect.toml,wash.metal,glyphs.metal}`
   from watercolor's; the same checks on `composition-notebook` and `navy`.
5. **Docs.** `docs/themes.md`, `docs/guide.md`, `docs/agent-guide.md`, `docs/development.md`
   (Status rows), `README.md` counts; `config.rs`'s comment; the guide test's built-in count.
6. **On screen.** A throwaway storyboard per theme (a shell printing colours and a diff, a
   markdown page), shot plain and with the effect, Radon and Comic Mono; the stills in a
   markdown page opened in pane 1 for Cyrus.
