# Appearance Stage 3: Seattle, San Francisco and London Rain Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Runic gains three rainy city scenes, Seattle, San Francisco and London, each with its landmarks, and three presets join Tokyo Rain in a Rain group.

**Architecture:** The rainy skyline (`city_scene` in `crates/arkos/effects/runic/wash.metal`) keeps its sky, bolt, clouds, two tower layers, windows and rain for every city; which landmarks stand in it follows the scene number. The wash today draws the Matrix street for any scene number over 0.5, so the street becomes scene 1 exactly and every other number is a rainy city.

**Tech Stack:** Metal, Rust tests, TOML presets.

**Spec:** `docs/superpowers/specs/2026-11-18-appearance-entities-design.md`, "The new skylines", "Presets" (the Rain group), stage 3 of Stages.

## Global Constraints

- Scene order in runic's `effect.toml`: `tokyo` 0, `matrix-city` 1, `seattle` 2, `san-francisco` 3, `london` 4; titles Tokyo, Matrix City, Seattle, San Francisco, London. Tokyo and the Matrix street draw exactly as before.
- Landmarks as the spec names them. Seattle: the Space Needle, the Columbia Center, Mount Rainier faint behind. San Francisco: the Transamerica Pyramid, Salesforce Tower (its lit crown), a Golden Gate tower pair at the right edge with the cable's curve, in the theme's red. London: the Elizabeth Tower with a lit clock face, the Shard, the London Eye turning slowly, the Gherkin. Lit as Tokyo's landmarks are, colours from `u.colors` only.
- Presets: `seattle-rain` ("Seattle Rain"), `frisco-rain` ("Frisco Rain"), `london-rain` ("London Rain"): `color = "rain"`, no font, `[effect] name = "runic"` with their scene, `group = "Rain"`; Tokyo Rain joins the group as its default. Seventeen built-in presets.
- Each scene within the wash's budget: no more than Tokyo's time plus 0.3 ms at 1200 by 800 (effects bench, before and after in one session).
- Tests headless, first, named as sentences; comments plain prose, no em-dashes; `nice -n 15`, never `--release`, one cargo at a time; `cargo check --workspace --all-targets` clean at the end. Every new thing only a person can judge gets a Status row.

## Review Focus

- A scene number of 2, 3 or 4 must never draw the Matrix street (Task 1 GPU test).
- The menu backdrops and the "scene under the names" contrast test still hold for every city (Task 1 runs the existing contrast test over each scene).
- The Rain group in the dark Presets list shows one row, Tokyo Rain its default, and remembers its pick (Task 2 test).

---

### Task 1: The three cities in the wash (implementer: the session itself)

**Files:** `crates/arkos/effects/runic/effect.toml`, `crates/arkos/effects/runic/wash.metal`, `crates/arkos/src/effects/stills.rs`, `crates/arkos/src/effects/manifest.rs` tests.

- [ ] **Step 1: Failing tests:** `runic_has_five_scenes_in_order_with_their_titles` (manifest); `on_the_gpu_the_new_cities_are_rainy_skylines_not_the_matrix_street` (Seattle, San Francisco and London each differ from Tokyo in far fewer points at the sidebar's foot than Tokyo differs from the Matrix City); `on_the_gpu_each_rainy_city_draws_its_own_landmarks` (the four rainy cities pairwise differ in over 800 points of the band); the existing scene-under-the-names contrast test run over every scene.
- [ ] **Step 2: FAIL. Step 3: the manifest, the street as scene 1 exactly, the landmarks. Judge each from stills at the sidebar's size. Step 4: PASS; the effects tests and `on_a_1x_screen_the_city_s_edges_do_not_flash`. Step 5: Commit** "Runic: Seattle, San Francisco and London in the rain".

### Task 2: The Rain presets, the group, the budget and the docs

**Files:** `crates/arkos/themes/{seattle-rain,frisco-rain,london-rain}.toml`, `tokyo-rain.toml`, `crates/arkos/src/theme.rs` (BUILTIN, tests that list names and counts), `docs/themes.md`, `docs/guide.md`, `docs/agent-guide.md` where presets or scenes are listed, `docs/development.md` Status rows, the runic design spec (`docs/superpowers/specs/2026-11-17-runic-world-design.md`, a section for the three cities with the measured cost).

- [ ] **Step 1: Failing tests:** `the_rain_group_holds_tokyo_seattle_frisco_and_london_rain_with_tokyo_its_default`; `each_rain_preset_draws_the_rain_colours_and_its_own_city`; `every_built_in_preset_meets_the_contrast_rules` still over seventeen; the presets list test in settings showing one Rain row in the dark list.
- [ ] **Step 2: FAIL. Step 3: implement. Step 4: whole suite PASS.**
- [ ] **Step 5: Bench** each scene with `nice -n 15 cargo test -p arkos --bin arkos effects::bench -- --ignored --nocapture` for Tokyo Rain and each new preset (temporarily pointing the bench at each preset, not committed), and write the times into the spec section. Over the budget: report it.
- [ ] **Step 6: Status rows:** "Rain cities: Presets, dark list: one Rain row; Seattle Rain shows the Space Needle, the Columbia Center and Rainier faint behind; Frisco Rain the Transamerica Pyramid, Salesforce Tower's lit crown and the Golden Gate's red towers; London Rain the Elizabeth Tower's lit clock, the Shard, the London Eye turning and the Gherkin; each in the rain, smooth on a 1x screen".
- [ ] **Step 7:** `cargo test --workspace --no-fail-fast` (only the two known arkosd pty tests may fail), `cargo check --workspace --all-targets` clean. **Commit** "Appearance: Seattle, Frisco and London Rain in a Rain group".
