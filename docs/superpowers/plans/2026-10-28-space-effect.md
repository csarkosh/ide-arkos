# Space Effect Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A built-in `space` effect (a living night sky, glowing edges, holo text that shimmers), recommended by Navy, through a second gpui fork hook that gives the glyph shader a clock; and Navy Light removed with Peach Light the default light theme.

**Architecture:** The effect is a folder of three files compiled in beside watercolor. The wash uses only the blocks the app already sends. The text's motion comes from a clock the fork binds to the glyph pass (buffer 5), set by `Root` before each frame and driven by a capped redraw timer while an effect whose manifest says `glyph_motion = true` is settled on and seen.

**Tech Stack:** Rust, gpui (fork csarkosh/gpui), Metal Shading Language, toml/toml_edit.

**Spec:** `docs/superpowers/specs/2026-10-28-space-effect-design.md` (with its builder-check rulings). The holo's reference shader is `/Volumes/CyrusExtSSD/arkos-lab/2026-10-28-space-text/holo-motion-dim.html` (the `FS` string, `motion == 5`).

## Global Constraints

- Builds: `nice -n 15 ~/.cargo/bin/cargo`, never `--release`, one cargo at a time, the shared `target-dir`.
- Tests headless, named as sentences, written first. Doc comments plain prose, no em-dashes.
- Finish every task with `cargo check --workspace --all-targets` warning-free.
- Every shader in its own `.metal` file loaded with `include_str!`, never a Rust string.
- Holo values fixed: brightness 0.80, tint 0.25, keep 0.15, scanlines 0.08, glow 0.90, Shimmer at 1.00.
- `space/effect.toml`: `frame_rate = 30`, `scale = 0.5`, `ui_text_bump = 0`, `ink_borders = true`, `highlights = "effect"`, `paper_alpha = 0.8`, `chrome_alpha = 0.0`, `active_tab_alpha = 0.5`, `text_contrast = 0.0`, `glyph_motion = true`.
- Event rates: a shooting star every 20 to 40 s; aurora rise ~20 s, hold ~60 s, fade ~20 s, gap 60 to 120 s; planets cross over several minutes.
- Text redraw cap: 20 frames a second.
- Navy Light notice text, exactly: "Navy Light was removed; your light theme is now Peach Light."
- The gpui fork is hooks only; tag `v0.2.2-arkos.2`, pushed to github.com/csarkosh/gpui; never leave a `target/` in `~/Projects/gpui`.

## Review Focus

1. A frozen clock (Reduce Motion) shows the aurora at its hold and no streak mid-flight: pin with a test on the schedule offset constant mirrored in Rust (Task 4).
2. A config naming `navy-light` while a user file `navy-light.toml` exists is left alone (Task 3).
3. The text stops asking for redraws while the window is hidden, covered, faded or under Reduce Motion, and with watercolor (no `glyph_motion`) (Task 2).
4. A light theme with `space` chosen keeps dark, readable text (dark-glyph branch in the shader; Task 4 test that the source has it, and the hand check).
5. A user effect copied from `arkos effect template space` that declares the time buffer passes `arkos effect check` (Task 1 and 4).

---

### Task 1: The fork's clock hook, and Arkos on it

**Files:**
- Modify (fork): `~/Projects/gpui/src/platform/mac/metal_renderer.rs` (`SpriteInputIndex::GlyphTime = 5`, a `glyph_time: f32` field, bound to vertex and fragment in the glyph pass; test), `src/platform.rs` (default no-op `set_glyph_shader_time`), `src/platform/mac/window.rs`, `src/window.rs` (`Window::set_glyph_shader_time(&mut self, seconds: f32)`), `ARKOS.md`.
- Modify: `Cargo.toml` (`[patch.crates-io]` tag `v0.2.2-arkos.2`), `crates/arkos/src/effects/active.rs` (`GlyphSink::set_glyph_time`), `crates/arkos/effects/glyph-signatures.metal`, `crates/arkos/src/effects/cli.rs` (contract test).

**Interfaces:**
- Produces: `gpui::Window::set_glyph_shader_time(&mut self, seconds: f32)`; in Metal, buffer index 5 holds one `float`, bound in both glyph stages on every glyph draw. `effects::active::GlyphSink::set_glyph_time(&mut self, seconds: f32)`.

- [ ] **Step 1:** In the fork, write the test `the_glyph_time_set_is_the_time_the_glyph_pass_binds` (the renderer stores what `set_glyph_shader_time` is given; read back through a `glyph_time()` accessor used by the draw), and a test compiling a glyph shader whose fragment declares `constant float *time [[buffer(5)]]` and builds a pipeline.
- [ ] **Step 2:** Run the fork's tests (`cargo test --lib metal_renderer` with `nice`): FAIL.
- [ ] **Step 3:** Implement the field, the binding (`set_vertex_bytes` and `set_fragment_bytes` at 5 beside the viewport size), the platform default and the `Window` method. Document it in `ARKOS.md`'s patch list.
- [ ] **Step 4:** Fork tests PASS. Delete the fork's `target/` (`cargo clean`). Commit "Window::set_glyph_shader_time, a clock for the glyph stages", tag `v0.2.2-arkos.2`, push the commit and the tag.
- [ ] **Step 5:** In Arkos, repoint the patch to the tag, `cargo update -p gpui`. Add the contract line to `glyph-signatures.metal` (the buffer and its index); extend `effect_contract_prints_the_preamble_and_the_glyph_prelude_on_request` to assert the line names `[[buffer(5)]]`. Add `set_glyph_time` to `GlyphSink` (the `Recorder` in the tests records it).
- [ ] **Step 6:** `cargo test -p arkos` and the workspace check PASS. Commit.

### Task 2: `glyph_motion` and the text's redraw timer

**Files:**
- Modify: `crates/arkos/src/effects/manifest.rs` (`pub glyph_motion: bool`, default false, key `glyph_motion`), `crates/arkos/src/effects/active.rs` (`pub fn glyph_wait(...)`), `crates/arkos/src/main.rs` (the timer, the time set before each frame, `Root::any_motion` untouched), `docs/agent-guide.md` (the key in the manifest table).

**Interfaces:**
- Produces: `effects::active::GLYPH_RATE_CAP: u32 = 20`; `pub fn glyph_wait(frame_rate: u32, motion: bool, settled: bool, seen: bool, reduce: bool) -> Option<Duration>`: `None` unless `motion && settled && seen && !reduce`, else one frame at `min(frame_rate, 20)`.

- [ ] **Step 1:** Tests: `a_manifest_may_say_its_glyphs_move_and_watercolor_s_do_not` (parses `glyph_motion = true`; default false; the built-in watercolor false) and `the_text_redraws_only_while_a_moving_glyph_shader_is_on_and_seen` (every `None` case from the interface, and `Some(50 ms)` for frame rates 30 and 60, `Some(1/12 s)` for 12).
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Implement. In `Root`: a second timer beside the wash's (its own generation counter, as `effect_timer_gen`) that, while `glyph_wait` says so, calls `cx.notify()` after each wait; started from `render` when it is not running and `glyph_wait` is `Some`. In `render`, before the glyph shader is applied, `window.set_glyph_shader_time(e.time(now, reduce))` while an effect is on (0 otherwise).
- [ ] **Step 4:** PASS, workspace check clean. Commit.

### Task 3: Navy Light removed, Peach Light the default light theme

**Files:**
- Delete: `crates/arkos/themes/navy-light.toml`.
- Modify: `crates/arkos/src/theme.rs` (`BUILTIN` entry and tests), `crates/arkos/src/config.rs` (default light `peach-light`, the template comment, a `pub fn retire_navy_light(text: &str, themes_dir: &Path) -> Option<String>` returning the rewritten config text), `crates/arkos/src/main.rs` (call it where the window loads its config; write the file; show the notice once), and every test and doc naming `navy-light` (`command_palette.rs`, `settings.rs`, `agent_cli.rs`, `doctor.rs`, `crates/shoot/src/story.rs`, `docs/themes.md`, `docs/guide.md`, `docs/agent-guide.md`, `docs/development.md`).

**Interfaces:**
- Produces: `config::retire_navy_light(text, themes_dir) -> Option<String>`: `Some(new text)` when `theme.light` or `theme.dark` is `"navy-light"` and `themes_dir/navy-light.toml` does not exist, with that value set to `"peach-light"` and every comment kept; `None` otherwise. `config::NAVY_LIGHT_NOTICE: &str` (the exact notice).

- [ ] **Step 1:** Tests: `navy_light_is_no_longer_built_in_and_peach_light_is_the_default_light_theme`; `a_config_naming_navy_light_is_moved_to_peach_light_keeping_its_comments`; `a_user_s_own_navy_light_file_keeps_its_config_as_it_is`; `a_config_without_navy_light_is_left_alone` (returns `None`).
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Implement; update the other tests that used `navy-light` as "a light theme" to `peach-light` (or `lavender-light` where the test needs a light theme without an effect).
- [ ] **Step 4:** Docs: every built-in list drops `navy-light`; the agent guide's light start is `arkos theme template peach-light`; a Status row in `docs/development.md` for the notice.
- [ ] **Step 5:** PASS (`cargo test --workspace` minus the two known sandbox-bound arkosd pty tests), check clean. Commit.

### Task 4: The space effect

**Files:**
- Create: `crates/arkos/effects/space/effect.toml`, `crates/arkos/effects/space/wash.metal`, `crates/arkos/effects/space/glyphs.metal`.
- Modify: `crates/arkos/src/effects/mod.rs` (`BUILTIN` gains `space`), `crates/arkos/themes/navy.toml` (`[effect] recommends = "space"`), `crates/arkos/src/theme.rs` test, `crates/arkos/src/effects/manifest.rs` tests, `docs/guide.md` (Effects: space), `docs/agent-guide.md`, `docs/development.md` (Status rows: the sky's events, the glow edges, holo text and shimmer, Reduce Motion still with the aurora, a light theme with space, the idle CPU in a release build).

**Interfaces:**
- Consumes: Task 1's buffer 5 (`constant float *time [[buffer(5)]]` in the glyph fragment), `ARKOS_SCALE`, Task 2's `glyph_motion`.
- Produces: `effects::BUILTIN` with `space`; `theme::builtin("navy").recommended_effect() == Some("space")`.

- [ ] **Step 1:** Tests: `the_built_in_space_parses_with_its_fixed_values` (every manifest value in Global Constraints); `navy_recommends_space_and_peach_light_watercolor`; `the_space_shaders_compile_as_a_window_and_check_would` (through `cli`'s compile path, as watercolor's check test); `the_holo_text_is_measured_in_points_reads_the_clock_and_keeps_dark_text_readable` (the glyph source names `ARKOS_SCALE`, `[[buffer(5)]]`, and the luminance test of the dark-glyph branch); `a_frozen_clock_shows_the_aurora_at_its_hold` (a Rust mirror `effects::space::aurora_level(t: f32) -> f32` and `meteor_visible(t: f32) -> bool` of the shader's schedule, with the same constants; `aurora_level(0.0) == 1.0`, `!meteor_visible(0.0)`, the gaps within the Global Constraints' ranges over an hour). The mirror lives in `crates/arkos/src/effects/space.rs` and the shader's constants carry a comment naming it.
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** Write the three files. The glyph shader ports the reference `FS` (motion 5) line for line in points, with the time from buffer 5, the widened quad (reach 5.5 pt), tile-bound sampling and the 55% core floor; the dark-glyph branch per ruling 2. The wash per the spec's sections.
- [ ] **Step 4:** PASS; `arkos effect bench space` (debug build is fine for a first look; the budget figure goes in the report) and `arkos effect check space`. Check clean. Commit.

### Task 5: See it

- [ ] **Step 1:** Push the branch; `gh workflow run shoot --ref space-effect -f scenes="multitasker"` (the multitasker story is already Navy, so the effect comes on by itself). Download to `/Volumes/CyrusExtSSD/arkos-lab/<date>-space-effect/`.
- [ ] **Step 2:** Build the dev binary and leave the launch line for Cyrus (`dev-arkos.sh`, Navy, `effect.name = "theme"`).
- [ ] **Step 3:** Report the still, the bench figure, and the hand checks owed. Nothing merges until Cyrus has looked.
