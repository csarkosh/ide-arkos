# Matrix Theme and Effect Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A built-in `matrix` theme (black, phosphor green) and `matrix` effect: emissive green text with a rare idle glitch, a terminal that decodes changed cells, and faint rune rain under the window.

**Architecture:** The effect is a folder of Metal shaders compiled in like `space` (`crates/arkos/effects/matrix/`). The glyph shader glows and glitches every letter from the existing clock at buffer 5. The terminal's decode is app-side Rust: a pure module (`effects/decode.rs`) remembers each visible cell's last character and change time and says what to draw; `TerminalElement` applies it when the manifest asks (`terminal_decode`). Two manifest keys are added, `glyph_rate` and `terminal_decode`.

**Tech Stack:** Rust, gpui (the `csarkosh/gpui` fork at `v0.2.2-arkos.4`, unchanged), Metal, `alacritty_terminal`.

**Spec:** `docs/superpowers/specs/2026-10-29-matrix-effect-design.md`

## Global Constraints

- Build and test from the worktree `~/Projects/ide-arkos/.claude/worktrees/matrix` (branch `matrix`): `nice -n 15 cargo test --workspace -p arkos` and `nice -n 15 cargo check --workspace --all-targets`; never `--release`; one cargo at a time (the machine is a MacBook Air).
- Tests are headless, written before the code, and named as sentences. A Metal test returns early when `Device::system_default()` is `None`, as the bench tests do.
- Doc comments are plain prose with no em-dashes. Every shader lives in its own `.metal` file loaded with `include_str!`; no shader text in a Rust string.
- The glyph shader keeps a stroke's core at least 55% of its coverage (`core * 0.55`), everything measured in points through `ARKOS_SCALE`, samples inside the glyph's own tile.
- `paper_alpha` stays at or above 0.5. The wash stays within 4 ms at `scale = 0.5` (`bench::verdict` says `within budget`).
- Reduce Motion: the clock is 0.0 and no glyph timer runs (`glyph_wait` is `None`); the decode settles at once.
- Never run a dev window against Cyrus's real config: use `arkos sandbox` (its own design; built before this plan runs) or the env-var script in the spec's Constraints.
- Every tuning constant named in the spec is a named constant at the top of its file with one comment, so a later edit is one line.
- Commit after each task with the attribution line `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

## Review Focus

1. A terminal resized while cells are decoding: the state is sized to the old grid; indexing must not panic and the new grid starts clean (Task 5 tests a resize reset).
2. Scrolling into history (display offset above 0) while output streams: history rows must never decode or brighten, and coming back to the live screen must not flash the whole screen as "changed" (Task 5: an offset change resets the state without marking cells changed).
3. A `dim` or `inverse` cell that is fresh: the brightening mixes `fg` toward white-green; `inverse` cells swap fg and bg at paint, so the mix must apply to the colour the glyph is drawn in, never the background (Task 6 tests that `shown` touches only `fg`).
4. An effect folder with `terminal_decode = true` and no `glyph_motion`: refused with an error naming both keys, so random characters never stick (Task 1).
5. A light theme with `matrix` chosen: dark text (luminance under 0.3) takes the teal branch and draws no halo, so it stays readable; the matrix theme itself has no text under 0.35 (Task 2, Task 4).

---

### Task 1: Two manifest keys, `glyph_rate` and `terminal_decode`

**Files:**
- Modify: `crates/arkos/src/effects/manifest.rs:12-61` (struct and defaults), `:101-122` (parse), tests at `:231-290`
- Modify: `crates/arkos/src/effects/active.rs:28-85` (`Style`, `style_at`), `:115-128` (`glyph_wait`), tests at `:427-436`
- Modify: `crates/arkos/src/main.rs:3800-3805` (`Root::glyph_wait`)
- Modify: `docs/agent-guide.md:527` (the manifest table: two rows after `glyph_motion`)

**Interfaces:**
- Produces: `Manifest.glyph_rate: Option<u32>`, `Manifest::glyph_rate(&self) -> u32` (the key, else `frame_rate.min(active::GLYPH_RATE_CAP)`), `Manifest.terminal_decode: bool`, `Style.terminal_decode: bool` (true only at full fade), `active::glyph_wait(glyph_rate: u32, motion: bool, settled: bool, seen: bool, reduce: bool) -> Option<Duration>` (first argument now the resolved rate).

- [ ] **Step 1: Write the failing manifest tests** in `manifest.rs` beside `a_manifest_may_say_its_glyphs_move_and_watercolor_s_do_not`:

```rust
#[test]
fn a_manifest_may_set_the_text_s_own_frame_rate_and_it_defaults_to_the_capped_wash_rate() {
    assert_eq!(parse(MINIMAL).unwrap().glyph_rate, None);
    assert_eq!(parse(MINIMAL).unwrap().glyph_rate(), 20, "frame_rate 30 capped at 20");
    assert_eq!(parse(&with("frame_rate = 12")).unwrap().glyph_rate(), 12);
    assert_eq!(parse(&with("glyph_rate = 10")).unwrap().glyph_rate(), 10);
    let e = parse(&with("glyph_rate = 30")).unwrap_err();
    assert!(e.contains("glyph_rate must be 1 to 20 (was 30)"), "{e}");
    assert!(parse(&with("glyph_rate = 0")).is_err());
}

#[test]
fn a_manifest_may_ask_the_terminal_to_decode_changed_cells_only_with_moving_glyphs() {
    assert!(!parse(MINIMAL).unwrap().terminal_decode);
    assert!(parse(&with("glyph_motion = true\nterminal_decode = true")).unwrap().terminal_decode);
    let e = parse(&with("terminal_decode = true")).unwrap_err();
    assert_eq!(e, "terminal_decode = true requires glyph_motion = true");
    for b in BUILTIN.iter().filter(|b| b.name != "matrix") {
        assert!(!parse(b.manifest).unwrap().terminal_decode, "{} does not decode", b.name);
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `nice -n 15 cargo test -p arkos effects::manifest 2>&1 | tail -20`
Expected: compile errors naming `glyph_rate` and `terminal_decode`.

- [ ] **Step 3: Add the fields and the checks.** In `Manifest`: `pub glyph_rate: Option<u32>` (doc: the frames a second the text is redrawn at while `glyph_motion` is on; absent, the wash's rate capped at `active::GLYPH_RATE_CAP`) and `pub terminal_decode: bool` (doc: the terminal decodes changed cells and brightens fresh ones while the effect is settled on; needs `glyph_motion`). Defaults `None` and `false`. In `parse`, after the existing ranges: `if let Some(r) = m.glyph_rate { in_range("glyph_rate", r, 1, 20)?; }` and `if m.terminal_decode && !m.glyph_motion { return Err("terminal_decode = true requires glyph_motion = true".into()); }`. Add `pub fn glyph_rate(&self) -> u32`.

- [ ] **Step 4: Write the failing `Style` and `glyph_wait` tests** in `active.rs`. Change the existing `the_text_redraws_only_while_a_moving_glyph_shader_is_on_and_seen` so its first argument is the resolved rate: `glyph_wait(20, true, true, true, false) == Some(50 ms)`, `glyph_wait(10, true, true, true, false) == Some(100 ms)`, `glyph_wait(12, ...) == 1/12 s`, and `glyph_wait(60, ...) == Some(50 ms)` (a rate above the cap is still clamped). Add:

```rust
#[test]
fn the_terminal_decodes_only_once_the_effect_has_fully_faded_in() {
    let m = manifest::parse("name = \"m\"\nwash = \"w.metal\"\nglyph_motion = true\nterminal_decode = true\n").unwrap();
    assert!(!style_at(&m, 0.5).terminal_decode);
    assert!(style_at(&m, 1.0).terminal_decode);
    assert!(!Style::plain().terminal_decode);
    let still = manifest::parse("name = \"m\"\nwash = \"w.metal\"\nglyph_motion = true\n").unwrap();
    assert!(!style_at(&still, 1.0).terminal_decode);
}
```

- [ ] **Step 5: Run them to verify they fail**

Run: `nice -n 15 cargo test -p arkos effects::active 2>&1 | tail -20`
Expected: compile errors on `terminal_decode`.

- [ ] **Step 6: Implement.** `Style.terminal_decode: bool` (doc: the terminal decodes changed cells; the effect's `terminal_decode`, once fully on). `Style::plain()` sets it false; `style_at` sets `full && manifest.terminal_decode`; `ActiveEffect::style` sets it false while leaving (add to the `leaving()` struct update). `glyph_wait`'s first parameter becomes `glyph_rate: u32`, clamped `1..=GLYPH_RATE_CAP`, and its doc says so. In `main.rs:3804` pass `e.effect.manifest.glyph_rate()`.

- [ ] **Step 7: Run the effects tests and check**

Run: `nice -n 15 cargo test -p arkos effects:: 2>&1 | tail -5 && nice -n 15 cargo check --workspace --all-targets 2>&1 | tail -3`
Expected: all pass, no warnings.

- [ ] **Step 8: Document the keys** in `docs/agent-guide.md` after the `glyph_motion` row, with the spec's table text:

```
| `glyph_rate` | 1 to 20 | `frame_rate`, capped at 20 | The frames a second the text is redrawn at while `glyph_motion` is on. A glitch that lasts a tenth of a second needs ten. |
| `terminal_decode` | `true`, `false` | `false` | The terminal shows random characters in a cell whose character changed for a few frames before settling, and brightens fresh text. Needs `glyph_motion = true`; without it the manifest is invalid. |
```

Also change the `glyph_motion` row's "up to 20 times a second" to "at `glyph_rate` frames a second".

- [ ] **Step 9: Commit**

```bash
git add crates/arkos/src/effects/manifest.rs crates/arkos/src/effects/active.rs crates/arkos/src/main.rs docs/agent-guide.md
git commit -m "Effects: glyph_rate and terminal_decode manifest keys"
```

---

### Task 2: The `matrix` theme

**Files:**
- Create: `crates/arkos/themes/matrix.toml`
- Modify: `crates/arkos/src/theme.rs:163-168` (`BUILTIN`, now 5), tests at `:700-725`, `:796`, `:839-851` (the lists that enumerate the built-ins)
- Modify: `crates/arkos/src/config.rs:711` (the template's list), `docs/themes.md:3`, `docs/guide.md:638-639`, `docs/development.md:554`

**Interfaces:**
- Produces: `theme::builtin("matrix")`, recommending the effect `matrix`; `theme::luminance(c: Rgb) -> f32` (the shader's sRGB dot product, `0.2126 r + 0.7152 g + 0.0722 b` of the components over 255).

- [ ] **Step 1: Write the failing tests** in `theme.rs`:

```rust
#[test]
fn matrix_is_built_in_black_with_green_text_and_recommends_its_effect() {
    let m = builtin("matrix").expect("matrix is built in");
    assert!(!m.is_light());
    assert_eq!(m.chrome.window_bg, Rgb { r: 0, g: 0, b: 0 });
    assert_eq!(m.terminal.background, Rgb { r: 0, g: 0, b: 0 });
    assert_eq!(m.recommended_effect(), Some("matrix"));
    assert_eq!(BUILTIN.len(), 5);
}

#[test]
fn every_text_colour_of_matrix_is_bright_enough_for_the_glyph_shader_s_emissive_branch() {
    let m = builtin("matrix").unwrap();
    let (c, t, s, tr, v) = (&m.chrome, &m.terminal, &m.semantic, &m.tree, &m.viewer);
    let text = [c.text, c.text_dim, c.focus_ring, t.foreground, t.cursor,
        t.red, t.green, t.yellow, t.blue, t.magenta, t.cyan, t.white,
        t.bright_black, t.bright_red, t.bright_green, t.bright_yellow, t.bright_blue, t.bright_magenta, t.bright_cyan, t.bright_white,
        s.working, s.waiting, s.bell, tr.added, tr.modified, tr.deleted, tr.untracked, tr.ignored,
        v.keyword, v.string, v.comment, v.number, v.function, v.type_name, v.constant, v.operator, v.punctuation];
    for x in text {
        assert!(luminance(x) >= 0.35, "{} is {:.2}, under the shader's 0.35", x.hex(), luminance(x));
    }
    assert!((luminance(Rgb { r: 255, g: 255, b: 255 }) - 1.0).abs() < 1e-6);
}
```

`terminal.black` is a background colour on a dark theme and is left out. Update `both_builtins_parse_and_name_themselves`, `names` (`:796`), `list` (`:839`, `:851`) and the config template test for five built-ins sorted (`lavender-light`, `matrix`, `navy`, `peach-light`, `timo-purple`).

- [ ] **Step 2: Run them to verify they fail**

Run: `nice -n 15 cargo test -p arkos theme:: 2>&1 | tail -20`
Expected: `luminance` undefined, `matrix` not built in.

- [ ] **Step 3: Write `matrix.toml`** with `name = "matrix"`, `[effect] name = "matrix"`, and colours in the spec's ranges: `window_bg`, `terminal.background`, `tab_active` `#000000`; `title_bar`, `status_bar`, `sidebar`, `tab_bar`, `tab_inactive` about `#060b07`; `banner` a step lighter; `border` and `split_handle` a dark green; `text`, `focus_ring`, `foreground`, `cursor` about `#4af626`; `text_dim` about `#2e9e3a` raised until luminance 0.35 passes; `selection`, `find_match`, `find_current` dark greens the foreground keeps 4.5:1 on; ANSI colours leaning to the phosphor family with their meanings kept; `semantic.working` the text green, `waiting` an amber. Add `("matrix", include_str!("../themes/matrix.toml"))` to `BUILTIN` and `pub fn luminance(c: Rgb) -> f32`. Iterate on the values with `nice -n 15 cargo run -q -p arkos -- theme contrast crates/arkos/themes/matrix.toml` (the command takes a `.toml` path) until every pair is `ok`.

- [ ] **Step 4: Run the theme tests**

Run: `nice -n 15 cargo test -p arkos theme:: 2>&1 | tail -20`
Expected: all pass, `every_contrast_pair_of_every_builtin_clears_what_it_needs` included.

- [ ] **Step 5: Name it in the docs**: `config.rs:711` template comment, `docs/themes.md:3` ("five themes built in"), `docs/guide.md:639` (`matrix` (black and phosphor green, recommends the `matrix` effect)), `docs/development.md:554`, and the `README.md:35` list only if it names every built-in (it names the three in the pictures; leave it).

- [ ] **Step 6: Commit**

```bash
git add crates/arkos/themes/matrix.toml crates/arkos/src/theme.rs crates/arkos/src/config.rs docs/themes.md docs/guide.md docs/development.md
git commit -m "Theme: matrix, black with phosphor green text"
```

---

### Task 3: The `matrix` effect folder and its wash

**Files:**
- Create: `crates/arkos/effects/matrix/effect.toml`, `crates/arkos/effects/matrix/wash.metal`, `crates/arkos/effects/matrix/glyphs.metal` (a copy of space's for now; Task 4 rewrites it)
- Create: `crates/arkos/src/effects/matrix.rs` (the Rust mirror: constants and `hash01` reuse; grows in Task 4)
- Modify: `crates/arkos/src/effects/mod.rs:33-46` (`BUILTIN`), `:11` (`pub mod matrix` under `#[cfg(test)]`)
- Modify: `crates/arkos/src/effects/manifest.rs` tests (a `the_built_in_matrix_parses_with_its_fixed_values`), `crates/arkos/src/effects/cli.rs:595` (compile test covers matrix), `crates/arkos/src/effects/bench.rs` (a render test)

**Interfaces:**
- Consumes: `manifest::load("matrix", dir)`, `bench::render_frame`, `bench::bench`, `cli::compile`.
- Produces: the built-in `matrix`; `effects::matrix::{RAIN_COL, RAIN_ROW, RAIN_SPEED_MIN, RAIN_SPEED_MAX, RUNE_CHANGE, RAIN_ALPHA}` as `f32` constants matching the shader's lines.

- [ ] **Step 1: Write the failing tests.** In `manifest.rs`:

```rust
#[test]
fn the_built_in_matrix_parses_with_its_fixed_values() {
    let b = BUILTIN.iter().find(|b| b.name == "matrix").expect("matrix is built in");
    let m = parse(b.manifest).unwrap();
    assert_eq!((m.frame_rate, m.scale, m.ui_text_bump), (30, 0.5, 0.0));
    assert!(m.ink_borders && m.glyph_motion && m.terminal_decode);
    assert_eq!(m.highlights, Highlights::Effect);
    assert_eq!(m.menus, MenuLook::Glow);
    assert_eq!((m.paper_alpha, m.chrome_alpha, m.active_tab_alpha, m.text_contrast), (0.8, 0.0, 0.5, 0.0));
    assert_eq!(m.glyph_rate(), 10);
    assert_eq!(m.glyphs.as_deref(), Some("glyphs.metal"));
}
```

In `effects/matrix.rs` (test module): `the_wash_carries_the_rain_s_constants_and_names_this_mirror`, asserting `wash.contains("effects/matrix.rs")` and each of `constant float RAIN_COL = 14.0;`, `RAIN_ROW = 18.0`, `RAIN_SPEED_MIN = 2.0`, `RAIN_SPEED_MAX = 6.0`, `RUNE_CHANGE = 0.5`, `RAIN_ALPHA = 0.12`; and `the_menus_edges_are_drawn_with_the_panes_own_code`, as space's test at `space.rs:501` but for the function names this wash uses (`nearest_edge(`, `edge_glow(`). In `cli.rs`, extend `the_space_shaders_compile_as_a_window_and_check_would` to loop over `["space", "matrix"]`. In `bench.rs`:

```rust
#[test]
fn the_matrix_rain_is_faint_under_the_text_and_within_budget() {
    if Device::system_default().is_none() { return; }
    let matrix = manifest::load("matrix", Path::new("/nonexistent")).unwrap();
    let theme = crate::theme::builtin("matrix").unwrap();
    let frame = render_frame(&matrix.wash, &theme, (W, H), 7.0, &small_window());
    // Inside a pane body, away from the edges' glow: the rain never brightens the ground past
    // RAIN_ALPHA of the text green, with a margin for the head, and it does show.
    let interior = pixels(&frame, 30, 60, 100, 110); // the left pane's body, as `mean` reads it
    let text = theme.terminal.foreground.g as f32 / 255.0;
    let max = interior.iter().map(|p| p.g as f32 / 255.0).fold(0.0, f32::max);
    assert!(max <= text * 0.3, "the rain is too bright: green channel {max:.2}");
    assert!(interior.iter().any(|p| p.g > 4), "the rain does not show at all");
    let ms = bench(&matrix, &theme, (1200.0, 800.0), 1.0).unwrap();
    assert_eq!(verdict(ms), "within budget", "{ms} ms");
}
```

`pixels(frame, x0, y0, x1, y1) -> Vec<Rgb>` is a small test helper beside `mean` returning the rectangle's pixels; the rectangle is the left pane's body in `small_window()` (read its blocks to pick the coordinates).

- [ ] **Step 2: Run them to verify they fail**

Run: `nice -n 15 cargo test -p arkos effects:: 2>&1 | tail -20`
Expected: `matrix` not built in.

- [ ] **Step 3: Write `effect.toml`** with the spec's values (`name = "matrix"`, a one-sentence description, `wash`, `glyphs`, `frame_rate = 30`, `scale = 0.5`, `ink_borders = true`, `highlights = "effect"`, `menus = "glow"`, `glyph_motion = true`, `glyph_rate = 10`, `terminal_decode = true`, `paper_alpha = 0.8`, `chrome_alpha = 0.0`, `active_tab_alpha = 0.5`, `text_contrast = 0.0`), with a comment per key as space's has.

- [ ] **Step 4: Write `wash.metal`.** Header comment naming the mirror `crates/arkos/src/effects/matrix.rs`. Constants at the top with one comment each: `RAIN_COL`, `RAIN_ROW`, `RAIN_SPEED_MIN`, `RAIN_SPEED_MAX`, `RAIN_TRAIL_MIN = 8.0`, `RAIN_TRAIL_MAX = 24.0`, `RAIN_GAP = 3.0` (seconds between a column's streams), `RUNE_CHANGE`, `RAIN_ALPHA`, `RAIN_HEAD = 0.35` (the head's extra brightness, toward white), and the edge constants copied from space (`EDGE_HALO`, `EDGE_REACH`, line widths) without the query bars. Copy `hash01`, `hash2`, `sd_box`, `nearest_edge`, `edge_glow` and the `Glow` struct from `space/wash.metal`, dropping `bar_cover_at` and the shimmer. Functions: `float rune(float2 cell, float2 local, float t)` (a 5 by 7 bit pattern from `hash01` of the cell and `floor(t / RUNE_CHANGE)`, bits drawn as soft-edged squares); `float4 rain(float2 p, float t)` (the column's speed, trail and phase from `hash01(column)`, the stream's head at `fract(t * speed / period)` scaled to the column's rows plus trail, the trail fading linearly to zero behind the head, the head mixed `RAIN_HEAD` toward white, returning the green and its alpha before `u.intensity`). `arkos_wash`: ground `ARKOS_WINDOW_BG`; add `rain * RAIN_ALPHA * u.intensity`; the bars' and sidebar's faint darkening as space does; highlights as space (`highlight_strength` by ordinal); the edge glow in `ARKOS_FOCUS_RING`. `arkos_overlay`: the menus' edges with `nearest_edge` and `edge_glow`, as space's. The green is `ARKOS_TERMINAL_FG` when it is greener than `ARKOS_FOCUS_RING` (compare `g - max(r, b)`), else the focus ring, so another theme reads in its own colour.

- [ ] **Step 5: Add the folder to `BUILTIN`** in `effects/mod.rs` (manifest, wash, glyphs by `include_str!`), copy `space/glyphs.metal` to `matrix/glyphs.metal` unchanged for this task, and add `#[cfg(test)] pub mod matrix;` with the constants. Move `hash01` out of the test-only `space.rs` into a new runtime module `crates/arkos/src/effects/hash.rs` (`pub fn hash01(n: u32) -> f32`, its doc comment with it; `pub mod hash;` in `mod.rs`), and have `space.rs` use `super::hash::hash01`: Task 5's decode runs in the app and needs it.

- [ ] **Step 6: Run the tests, then the check and the bench**

Run: `nice -n 15 cargo test -p arkos effects:: 2>&1 | tail -20 && nice -n 15 cargo run -q -p arkos -- effect bench matrix`
Expected: all pass; `matrix: … ms per frame … within budget`. Over 4 ms: lower the rune's soft edge to one `smoothstep`, or sample the rain at the cell's centre only, until within budget; report the figure.

- [ ] **Step 7: Commit**

```bash
git add crates/arkos/effects/matrix crates/arkos/src/effects/mod.rs crates/arkos/src/effects/matrix.rs crates/arkos/src/effects/manifest.rs crates/arkos/src/effects/cli.rs crates/arkos/src/effects/bench.rs
git commit -m "Effect: matrix wash, rune rain under a black ground with green edges"
```

---

### Task 4: The matrix glyph shader: emissive green and the idle glitch

**Files:**
- Modify: `crates/arkos/effects/matrix/glyphs.metal` (rewrite from space's)
- Modify: `crates/arkos/src/effects/matrix.rs` (the glitch mirror and its tests)
- Modify: `crates/arkos/src/effects/cli.rs:575` (the clock test's shape is the model; add matrix to the compile loop if Task 3 did not)

**Interfaces:**
- Consumes: `hash::hash01(n: u32) -> f32` (Task 3 moved it to `effects/hash.rs`).
- Produces: `matrix::GLITCH_SLOT: f32 = 0.1`, `GLITCH_CHANCE: f32 = 0.0001`, `GLITCH_FLASH: f32 = 1.4`, `pub fn glitch_key(origin_px: (u32, u32), slot: u32) -> u32` (the hash input the shader builds: `origin.x * 73856093 ^ origin.y * 19349663 ^ slot * 83492791`, wrapping), `pub fn glitches(origin_px: (u32, u32), slot: u32) -> Option<u8>` (`None` when `slot == 0` or `hash01(key) >= GLITCH_CHANCE`; else the mirror mode `1 + (hash01(key ^ 0x9E3779B9) * 3.0) as u8`, 1 horizontal, 2 vertical, 3 both).

- [ ] **Step 1: Write the failing mirror tests** in `matrix.rs`:

```rust
#[test]
fn slot_zero_glitches_nothing_so_a_frozen_clock_shows_still_text() {
    for x in (0..3000u32).step_by(7) {
        assert_eq!(glitches((x * 11, x * 17), 0), None);
    }
}

#[test]
fn about_three_glyphs_in_three_thousand_glitch_each_second() {
    // 3,000 glyph positions over 100 slots (10 seconds): expect about 30, within a factor of two.
    let mut n = 0;
    for slot in 1..=100u32 {
        for g in 0..3000u32 {
            if glitches(((g % 120) * 14, (g / 120) * 18), slot).is_some() { n += 1; }
        }
    }
    assert!((15..=60).contains(&n), "{n} glitches in ten seconds");
}

#[test]
fn a_glitch_picks_one_of_three_mirrors_and_the_same_one_every_time() {
    let found: Vec<u8> = (1..=2000u32).flat_map(|s| (0..200u32).filter_map(move |g| glitches((g * 14, 36), s))).collect();
    assert!(found.iter().all(|m| (1..=3).contains(m)));
    assert!([1u8, 2, 3].iter().all(|m| found.contains(m)), "{found:?}");
    assert_eq!(glitches((14, 36), 5), glitches((14, 36), 5));
}

#[test]
fn the_glyph_shader_carries_the_glitch_s_constants_reads_the_clock_and_keeps_the_floors() {
    let g = matrix().glyphs.unwrap();
    for line in ["constant float GLITCH_SLOT = 0.1;", "constant float GLITCH_CHANCE = 0.0001;", "constant float GLITCH_FLASH = 1.4;",
                 "constant float TINT = 0.45;", "constant float KEEP = 0.15;", "constant float GLOW = 1.6;", "constant float BRIGHTNESS = 1.0;",
                 "float3(0.35, 1.0, 0.45)", "float3(0.0, 0.4, 0.15)", "bool dark = luminance < 0.3;", "core * 0.55", "ARKOS_SCALE", "[[buffer(5)]]", "effects/matrix.rs"] {
        assert!(g.contains(line), "glyphs.metal lacks {line}");
    }
    assert!(!g.contains("SCANLINES"), "no scanlines on the matrix display");
    let emoji = &g[g.find("arkos_emoji_vertex(").expect("matrix draws its emoji")..];
    assert!(emoji.contains("arkos_emoji_fragment(") && emoji.contains("GLITCH_") && emoji.contains("[[buffer(5)]]"));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `nice -n 15 cargo test -p arkos effects::matrix 2>&1 | tail -20`
Expected: `glitches` undefined; the shader lacks the lines.

- [ ] **Step 3: Implement the mirror** (`glitch_key`, `glitches`) with the constants, and **rewrite `glyphs.metal`** from space's: constants `BRIGHTNESS 1.0`, `TINT 0.45`, `KEEP 0.15`, `GLOW 1.6`, `REACH 5.5`, `GLITCH_SLOT`, `GLITCH_CHANCE`, `GLITCH_FLASH`, `MATRIX_GREEN float3(0.35, 1.0, 0.45)`, `MATRIX_DEEP float3(0.0, 0.4, 0.15)`; no `SCANLINES`. Both vertex stages take `constant float *time [[buffer(5)]]`, compute `slot = uint(floor(*time / GLITCH_SLOT))`, `key` as the mirror from `uint2(sprite.bounds.origin)` and the slot, and pass two flat fields to the fragment: `float flash` (1 or `GLITCH_FLASH`) and `float2 mirror` (1 or -1 per axis). When a mirror is set the fragment reflects its sample position inside the glyph's own tile before every atlas read: on a mirrored axis `uv = rect.xy + rect.zw - uv` for that component of `tile_rect`, so the halo rings reflect with the letter and no sample leaves the tile. Light is `BRIGHTNESS * flash`, the halo `GLOW * flash`. The dark branch at `luminance < 0.3` with `MATRIX_DEEP`, no halo, no flash. The `hash01` in Metal is space's (copy the function; the glyph file cannot include the wash). Emoji stages follow the same rule, keeping their colours as space's do.

- [ ] **Step 4: Run the mirror tests and the compile test**

Run: `nice -n 15 cargo test -p arkos effects:: 2>&1 | tail -20`
Expected: all pass, `the_space_shaders_compile_as_a_window_and_check_would` (now covering matrix) included.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/effects/matrix/glyphs.metal crates/arkos/src/effects/matrix.rs crates/arkos/src/effects/cli.rs
git commit -m "Effect: matrix glyphs glow green and glitch now and then"
```

---

### Task 5: The decode rules (pure, tested)

**Files:**
- Create: `crates/arkos/src/effects/decode.rs`
- Modify: `crates/arkos/src/effects/mod.rs` (`pub mod decode;`)

**Interfaces:**
- Consumes: `crate::layout::{CellStyle, CellKind}` (`CellStyle` has no `Default`: the tests build one with a `plain()` helper, green `fg` on black `bg`, every flag false), `crate::palette::Rgb`, `crate::theme::mix(a: Rgb, b: Rgb, t: f32) -> Rgb` (`pub(crate)`), `super::hash::hash01`.
- Produces:
  - `pub const DECODE_MS_WORKING: u64 = 400; DECODE_MS_IDLE: u64 = 120; FRESH_MS: u64 = 1500; FRESH_MIX: f32 = 0.6; DECODE_CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789@#$%&*+=<>/\\|";` and `FRESH_WHITE: Rgb = Rgb { r: 217, g: 255, b: 217 }`, `TICK_MS: u64 = 50` (the frame index's step).
  - `#[derive(Clone, Copy, PartialEq, Debug)] pub struct Timing { pub decode: Duration, pub fresh: Duration }` and `pub fn timing(working: bool, reduce_motion: bool) -> Timing` (reduce: both zero).
  - `pub struct Decode { .. }` with `pub fn new(cols: usize, rows: usize, display_offset: usize) -> Decode`, `pub fn observe(&mut self, rows: &[Vec<(char, CellStyle, CellKind)>], display_offset: usize, now: Instant)` (a size or offset change resets to this grid with no cell marked changed; else every cell whose char differs from the remembered one is remembered with `changed = Some(now)` when the new char is not a space, or cleared when it is), and `pub fn shown(&self, col: usize, row: usize, cell: (char, CellStyle, CellKind), now: Instant, t: Timing) -> (char, CellStyle)`.
  - `pub fn random_char(col: usize, row: usize, changed_nanos: u64, frame: u64) -> char` (from `DECODE_CHARS` by `space::hash01` of a wrapping mix of the four), `pub fn fresh_mix(since: Duration, fresh: Duration) -> f32` (`FRESH_MIX * (1 - since/fresh)` clamped, 0 when `fresh` is zero).

- [ ] **Step 1: Write the failing tests** in `decode.rs`:

```rust
fn plain() -> CellStyle { CellStyle { fg: Rgb { r: 74, g: 246, b: 38 }, bg: Rgb { r: 0, g: 0, b: 0 }, bold: false, italic: false, dim: false, underline: false, inverse: false } }
fn cell(c: char) -> (char, CellStyle, CellKind) { (c, plain(), CellKind::Normal) }
fn grid(s: &[&str]) -> Vec<Vec<(char, CellStyle, CellKind)>> { s.iter().map(|r| r.chars().map(cell).collect()).collect() }

#[test]
fn working_decodes_longer_than_idle_and_reduce_motion_settles_at_once() {
    assert_eq!(timing(true, false), Timing { decode: Duration::from_millis(400), fresh: Duration::from_millis(1500) });
    assert_eq!(timing(false, false).decode, Duration::from_millis(120));
    assert_eq!(timing(true, true), Timing { decode: Duration::ZERO, fresh: Duration::ZERO });
}

#[test]
fn a_changed_cell_shows_random_characters_then_settles_on_its_own() {
    let t0 = Instant::now();
    let mut d = Decode::new(3, 1, 0);
    d.observe(&grid(&["   "]), 0, t0);
    d.observe(&grid(&["ok "]), 0, t0);
    let t = timing(true, false);
    let (c, _) = d.shown(0, 0, cell('o'), t0 + Duration::from_millis(100), t);
    assert_ne!(c, 'o'); assert!(c.is_ascii() && DECODE_CHARS.contains(c));
    let (c2, _) = d.shown(0, 0, cell('o'), t0 + Duration::from_millis(160), t);
    assert!(DECODE_CHARS.contains(c2), "every frame draws from DECODE_CHARS");
    assert_eq!(d.shown(0, 0, cell('o'), t0 + Duration::from_millis(401), t).0, 'o');
    // The cell that stayed a space is untouched.
    assert_eq!(d.shown(2, 0, cell(' '), t0 + Duration::from_millis(100), t).0, ' ');
}

#[test]
fn an_unchanged_cell_is_never_touched_and_a_cleared_one_clears_at_once() {
    let t0 = Instant::now();
    let mut d = Decode::new(2, 1, 0);
    d.observe(&grid(&["ab"]), 0, t0);
    d.observe(&grid(&["a "]), 0, t0 + Duration::from_millis(10));
    let t = timing(true, false);
    assert_eq!(d.shown(0, 0, cell('a'), t0 + Duration::from_millis(20), t).0, 'a');
    assert_eq!(d.shown(0, 0, cell('a'), t0 + Duration::from_millis(20), t).1, plain(), "not fresh: its colour is its own");
    assert_eq!(d.shown(1, 0, cell(' '), t0 + Duration::from_millis(20), t).0, ' ');
}

#[test]
fn a_wide_character_decodes_over_two_cells_and_a_spacer_never() {
    let t0 = Instant::now();
    let mut d = Decode::new(2, 1, 0);
    d.observe(&grid(&["  "]), 0, t0);
    let wide = vec![vec![('漢', plain(), CellKind::Wide), (' ', plain(), CellKind::Spacer)]];
    d.observe(&wide, 0, t0);
    let t = timing(false, false);
    let (c, _) = d.shown(0, 0, wide[0][0], t0 + Duration::from_millis(50), t);
    assert!(c.is_ascii() && c != '漢');
    assert_eq!(d.shown(1, 0, wide[0][1], t0 + Duration::from_millis(50), t).0, ' ');
    assert_eq!(d.shown(0, 0, wide[0][0], t0 + Duration::from_millis(121), t).0, '漢');
}

#[test]
fn fresh_text_starts_brighter_and_cools_over_a_second_and_a_half() {
    assert!((fresh_mix(Duration::ZERO, Duration::from_millis(1500)) - 0.6).abs() < 1e-6);
    assert!((fresh_mix(Duration::from_millis(750), Duration::from_millis(1500)) - 0.3).abs() < 1e-6);
    assert_eq!(fresh_mix(Duration::from_millis(1500), Duration::from_millis(1500)), 0.0);
    assert_eq!(fresh_mix(Duration::ZERO, Duration::ZERO), 0.0, "Reduce Motion: never brightened");
    let t0 = Instant::now();
    let mut d = Decode::new(1, 1, 0);
    d.observe(&grid(&[" "]), 0, t0);
    d.observe(&grid(&["x"]), 0, t0);
    let t = timing(false, false);
    let own = plain();
    let (_, s) = d.shown(0, 0, ('x', own, CellKind::Normal), t0 + Duration::from_millis(121), t);
    assert_eq!(s.fg, crate::theme::mix(own.fg, FRESH_WHITE, fresh_mix(Duration::from_millis(121), t.fresh)));
    assert_eq!(s.bg, own.bg, "only the foreground brightens");
    assert_eq!(d.shown(0, 0, ('x', own, CellKind::Normal), t0 + Duration::from_millis(1600), t).1, own);
}

#[test]
fn a_resize_or_a_scroll_into_history_resets_the_state_without_marking_cells_changed() {
    let t0 = Instant::now();
    let mut d = Decode::new(2, 1, 0);
    d.observe(&grid(&["ab"]), 0, t0);
    d.observe(&grid(&["abc"]), 0, t0);               // wider: a fresh grid
    let t = timing(true, false);
    assert_eq!(d.shown(2, 0, cell('c'), t0 + Duration::from_millis(10), t).0, 'c');
    d.observe(&grid(&["xyz"]), 3, t0);               // scrolled into history: never fresh
    assert_eq!(d.shown(0, 0, cell('x'), t0 + Duration::from_millis(10), t).0, 'x');
    d.observe(&grid(&["abc"]), 0, t0);               // back to the live screen: no flash
    assert_eq!(d.shown(0, 0, cell('a'), t0 + Duration::from_millis(10), t).0, 'a');
    // Out-of-range cells (a grid shrunk between observe and shown) are drawn as their own.
    assert_eq!(d.shown(9, 9, cell('q'), t0, t).0, 'q');
}

#[test]
fn the_random_character_is_ascii_and_predicted_by_the_cell_the_instant_and_the_frame() {
    let a = random_char(3, 4, 123_456_789, 2);
    assert_eq!(a, random_char(3, 4, 123_456_789, 2));
    assert!(DECODE_CHARS.contains(a));
    let frames: std::collections::HashSet<char> = (0..40).map(|f| random_char(3, 4, 123_456_789, f)).collect();
    assert!(frames.len() > 5, "the frames vary");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `nice -n 15 cargo test -p arkos effects::decode 2>&1 | tail -10`
Expected: module missing.

- [ ] **Step 3: Implement `decode.rs`** to the interface above. `shown`: a cell out of range, a `Spacer`, or one with `changed == None` is its own; else `since = now - changed`; if `since < t.decode` and the cell's char is not a space, the char is `random_char(col, row, changed nanos since the state's epoch, since.as_millis() / TICK_MS)`; the style's `fg` is `mix(fg, FRESH_WHITE, fresh_mix(since, t.fresh))` while `since < t.fresh`. A `Wide` cell decodes as its ASCII char; the paint still draws it at double width from `CellKind`, so `shown` returns the char only and the kind is the caller's. Store the state's epoch `Instant` in `Decode::new` so `changed_nanos` is a `u64`.

- [ ] **Step 4: Run the tests**

Run: `nice -n 15 cargo test -p arkos effects::decode 2>&1 | tail -10`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/effects/decode.rs crates/arkos/src/effects/mod.rs
git commit -m "Effects: the terminal decode rules, a cell scrambles then settles"
```

---

### Task 6: The terminal applies the decode under a `terminal_decode` effect

**Files:**
- Modify: `crates/arkos/src/terminal.rs:49-53` (`RowsForPaint.display_offset` is now read: drop its `#[allow(dead_code)]` and its comment), `:875-935` (`Terminal` gains `pub decode: Option<effects::decode::Decode>`, `None` in the constructor)
- Modify: `crates/arkos/src/terminal_element.rs:22-33` (`TerminalElement` gains `decode: Option<effects::decode::Timing>`), `:135-215` (paint)
- Modify: `crates/arkos/src/pane_view.rs:359-361`, `:967` (compute the timing and pass it)
- Test: `crates/arkos/src/terminal_element.rs` (a headless test on the row transform), `crates/arkos/src/pane_view.rs` (the timing choice)

**Interfaces:**
- Consumes: Task 1's `Style.terminal_decode`, Task 5's `decode::{Decode, Timing, timing}`, `root.sessions.states.get(&session)`, `motion::reduce_motion()`.
- Produces: `TerminalElement::new(terminal, font_size, line_height, family, paper_alpha, decode: Option<Timing>)`; `pub fn decode_timing(style_decode: bool, state: Option<ClaudeState>, reduce: bool) -> Option<Timing>` in `pane_view.rs`; `pub fn decoded_rows(decode: &mut Option<Decode>, rows: &RowsForPaint, timing: Option<Timing>, cols: usize, now: Instant) -> Vec<Vec<(char, CellStyle, CellKind)>>` in `terminal_element.rs` (the visible rows as they are drawn: observed and transformed when `timing` is `Some`, else cloned as they are; the state is created when missing and dropped when `timing` is `None`).

- [ ] **Step 1: Write the failing tests.** In `pane_view.rs`:

```rust
#[test]
fn the_terminal_decodes_under_a_decoding_effect_longer_while_its_session_works() {
    use protocol::ClaudeState::*;
    assert_eq!(decode_timing(false, Some(Working), false), None);
    assert_eq!(decode_timing(true, Some(Working), false), Some(decode::timing(true, false)));
    assert_eq!(decode_timing(true, Some(Idle), false), Some(decode::timing(false, false)));
    assert_eq!(decode_timing(true, None, false), Some(decode::timing(false, false)));
    assert_eq!(decode_timing(true, Some(Working), true), Some(decode::timing(true, true)));
}
```

In `terminal_element.rs`:

```rust
#[test]
fn the_rows_are_drawn_as_the_decode_says_and_the_state_goes_when_the_effect_does() {
    let t0 = Instant::now();
    let plain = CellStyle { fg: Rgb { r: 74, g: 246, b: 38 }, bg: Rgb { r: 0, g: 0, b: 0 }, bold: false, italic: false, dim: false, underline: false, inverse: false };
    let rows = |s: &str| RowsForPaint { display_offset: 0, above: None, visible: vec![s.chars().map(|c| (c, plain, CellKind::Normal)).collect()], links: vec![] };
    let mut state = None;
    let t = Some(decode::timing(true, false));
    let first = decoded_rows(&mut state, &rows("   "), t, 3, t0);
    assert!(state.is_some()); assert_eq!(first[0][0].0, ' ');
    let second = decoded_rows(&mut state, &rows("ok "), t, 3, t0 + Duration::from_millis(20));
    assert_ne!(second[0][0].0, 'o', "a changed cell scrambles");
    let plain = decoded_rows(&mut state, &rows("ok "), None, 3, t0 + Duration::from_millis(30));
    assert!(state.is_none(), "no effect: the state is dropped"); assert_eq!(plain[0][0].0, 'o');
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `nice -n 15 cargo test -p arkos pane_view terminal_element 2>&1 | tail -10`
Expected: `decode_timing` and `decoded_rows` undefined.

- [ ] **Step 3: Implement.** `decode_timing` as `style_decode.then(|| decode::timing(state == Some(ClaudeState::Working), reduce))`. In `render_pane` (`pane_view.rs:359`), for a terminal tab: `let decode = decode_timing(style.terminal_decode, root.sessions.states.get(session).copied(), motion::reduce_motion());` and pass it as the last argument at `:967`. `decoded_rows`: when `timing` is `None`, set `*decode = None` and return `rows.visible.clone()`; else get or create `Decode::new(cols, rows.visible.len(), rows.display_offset)`, call `observe(&rows.visible, rows.display_offset, now)`, and map every `(col, row, cell)` through `shown`, keeping `CellKind`. In `paint` (`terminal_element.rs:143`), inside the `self.terminal.update` closure after `rows_for_paint()`, call `decoded_rows(&mut t.decode, &rows, self.decode, t.core.columns(), now)` and paint those rows in place of `rows.visible`; `rows.above` is painted as it is. Observing happens once per paint, and the timer at `glyph_rate` supplies the frames.

- [ ] **Step 4: Run the whole crate's tests and the check**

Run: `nice -n 15 cargo test -p arkos 2>&1 | tail -5 && nice -n 15 cargo check --workspace --all-targets 2>&1 | tail -3`
Expected: all pass (the two `arkosd` pty tests that always fail in this sandbox are in another crate), no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/terminal.rs crates/arkos/src/terminal_element.rs crates/arkos/src/pane_view.rs
git commit -m "Terminal: changed cells decode and fresh text cools under a decoding effect"
```

---

### Task 7: Docs, the Status list and the first look

**Files:**
- Modify: `docs/agent-guide.md:474-478` (three built-ins; matrix described), `docs/guide.md` (the effects paragraph naming matrix), `docs/development.md:218-300` (Status: hand checks), `CLAUDE.md` only if it lists the effects
- Modify: `docs/superpowers/specs/2026-10-29-matrix-effect-design.md` (Status line: "implemented on branch matrix, awaiting Cyrus's look")

- [ ] **Step 1: Write a doc test** in `effects/mod.rs` tests: `the_guide_names_every_built_in_effect`, asserting `include_str!("../../../docs/agent-guide.md")` contains each `BUILTIN` name in backticks and the rows `` `glyph_rate` `` and `` `terminal_decode` ``.

- [ ] **Step 2: Run it to verify it fails**, then **write the docs**: the agent guide's section 8 opening ("Three are built in: … and `matrix` (black with faint rune rain, emissive green text that glitches now and then, and a terminal that decodes its changed cells)"), the guide's effects paragraph, and these Status hand checks:

```
- effects: Matrix (`effect.name = "theme"`) at rest for five minutes: a letter glitches
  (mirrored and brighter for a tenth of a second) a few times a second across the window and
  never more; between glitches every letter is still and whole; the text is not tiring to read
- effects: Matrix, Claude Code streaming output: whole rows scramble through ASCII for under half
  a second and settle on the real text, which starts brighter and cools over about 1.5 s; typing
  at the prompt shows a brief flicker on the cell typed and nothing else
- effects: Matrix, Claude Code's fullscreen renderer redrawing: readable; if the whole screen
  decodes too often, note it (DECODE_MS_WORKING is the first lever)
- effects: Matrix, the rain: faint columns of runes falling at different speeds under the panes,
  through the bars and the sidebar, never competing with the text; a menu's backdrop shows it
- effects: Matrix, a markdown tab and the sidebar: green glow, the idle glitch, no decode
- effects: Matrix, emoji on the display: their colours tinted green, haloed, glitching as letters do
- effects: Matrix with Reduce Motion on: the rain is still, no glitches, changed cells settle at once
- effects: Matrix, the window's CPU idle in front (debug now; release after the release): write the figure here
- colours: vim or htop with a 256-colour scheme in Matrix shows green-cast colours, not xterm's
- colours: Claude Code's colours after a restart in Matrix are the theme's greens
```

- [ ] **Step 3: Run the doc test and the full check**

Run: `nice -n 15 cargo test -p arkos effects::tests 2>&1 | tail -5 && nice -n 15 cargo check --workspace --all-targets 2>&1 | tail -3`

- [ ] **Step 4: Commit**

```bash
git add docs/agent-guide.md docs/guide.md docs/development.md docs/superpowers/specs/2026-10-29-matrix-effect-design.md crates/arkos/src/effects/mod.rs
git commit -m "Docs: the matrix effect, its keys and its hand checks"
```

- [ ] **Step 5: Build the dev binary and open the sandbox window for Cyrus**

Run: `nice -n 15 cargo build -p arkos 2>&1 | tail -3`, then `target/debug/arkos sandbox ~/Projects/ide-arkos/.claude/worktrees/matrix` (or the spec's env-var script when the sandbox command is not built yet), then in that instance `theme use matrix` and `effect use theme`, and `effect current` for the cost line. Tell Cyrus which window it is and the checks above; iterate on the constants from what he says (`effect use none`, a second, `effect use matrix` to reload after an edit). Nothing merges until he has looked.
