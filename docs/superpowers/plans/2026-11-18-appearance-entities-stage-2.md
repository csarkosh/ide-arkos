# Appearance Stage 2: The City Is the Effect's Choice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Runic's sidebar scene is chosen with the effect, not by the colour: Custom's Effect list shows Runic as a group row (Tokyo, Matrix City; Seattle, San Francisco and London join in stage 3), and every existing look keeps its scene.

**Architecture:** Scenes get names and titles in runic's `effect.toml` (`tokyo`, `matrix-city`; the old `city` and `code` kept as aliases). The scene in force is worked out in one function from the look: a preset's own `[effect] scene`, or Custom's new `custom.scene`, or, for a Custom look saved before this, the scene its colour's preset gave. Settings' Custom Effect list reuses the Presets lists' group row and card.

**Tech Stack:** Rust, gpui (Settings), TOML manifests.

**Spec:** `docs/superpowers/specs/2026-11-18-appearance-entities-design.md`, sections "Effects and scenes", "Settings, Appearance" (Custom, Effect), "The config" (`custom.scene` and its migration), "The commands", "Settled before planning" (Scenes and the looks); stage 2 of Stages.

## Global Constraints

- Every built-in preset draws exactly as before (the stage 1 fixture test, adapted in Task 1 to compare a scene by the index it resolves to, since `code` is now `matrix-city`).
- A Custom look saved before this keeps its scene: `custom.effect = "runic"` with no `custom.scene` shows the scene its colour's preset named (The Matrix's gives Matrix City; any other, Tokyo); `custom.effect = "theme"` shows the effect and scene of the preset its colour loads.
- While Presets is in force, `custom.scene` is ignored.
- The wash's scene numbers do not change: `tokyo` is 0, `matrix-city` is 1 (`u.scene` in `wash.metal`).
- Group rows read as the Presets lists' do: one line of the list's lower-case text, "runic tokyo ▾", the whole row a select opening a floating card of members with a check on the current one; keys open and close it as today. Material 3 motion rules apply to anything that animates (it reuses the existing card, so nothing new should).
- Tests headless, first, named as sentences; doc comments plain prose, no em-dashes; `nice -n 15`, never `--release`, one cargo at a time; `cargo check --workspace --all-targets` with no warnings at the end.

## Review Focus

- A 0.8.54 config, Custom with colour `matrix` and effect `runic`, no scene: Matrix City still (Task 2 test).
- A Custom look with colour `lavender` and effect `runic`, no scene: Tokyo (Task 2 test).
- `custom.scene = "matrix-city"` while Presets is in force with Tokyo Rain: Tokyo (Task 2 test).
- An effect with no scenes (material) and a stray `custom.scene`: drawn as without one, and Settings shows a plain row (Tasks 2, 3).
- A user effect folder whose manifest has scenes but no titles: its scenes listed by name in title case (Task 1 test).

---

### Task 1: Scenes have names and titles; the old names still work

**Files:** Modify `crates/arkos/effects/runic/effect.toml`, `crates/arkos/src/effects/manifest.rs`, `crates/arkos/themes/matrix.toml`, `crates/arkos/themes/tokyo-rain.toml`, the stage 1 fixture test in `crates/arkos/src/theme.rs`, the theme docs' scene names (`docs/themes.md`).

**Interfaces:**
- `effect.toml`: `scenes = ["tokyo", "matrix-city"]`, `scene_titles = ["Tokyo", "Matrix City"]`, `scene_aliases = { city = "tokyo", code = "matrix-city" }`.
- Manifest gains `pub scene_titles: Vec<String>` (empty or the same length as `scenes`, else a load error) and `pub scene_aliases: BTreeMap<String, String>` (each target one of `scenes`, else a load error). Produces `pub fn scene_name(&self, name: &str) -> Option<&str>` (a scene's own name, or an alias's target), `pub fn scene_title(&self, i: usize) -> String` (its title, else the name in title case), and `scene_index` resolving aliases.
- `matrix.toml` names `scene = "matrix-city"`, `tokyo-rain.toml` `scene = "tokyo"`.
- The fixture test compares `[effect]` by name and by `scene_index` of the scene, not the raw string.

- [ ] **Step 1: Failing tests:** `runic_s_scenes_are_tokyo_and_matrix_city_titled_for_people`; `the_old_scene_names_city_and_code_still_name_the_same_scenes` (`scene_index(Some("city")) == scene_index(Some("tokyo")) == 0.0`, `code` and `matrix-city` 1.0); `a_manifest_whose_scene_titles_do_not_match_its_scenes_is_refused`; `a_scene_alias_to_no_scene_is_refused`; `a_scene_without_a_title_is_shown_in_title_case`. Update the existing `m.scenes == ["city", "code"]` assertion (manifest.rs ~769).
- [ ] **Step 2: FAIL. Step 3: implement. Step 4: `nice -n 15 cargo test -p arkos --bin arkos` PASS. Step 5: Commit** "Runic: scenes named tokyo and matrix-city, titled, the old names kept".

### Task 2: The scene in force comes from the look

**Files:** Modify `crates/arkos/src/config.rs` (CustomConfig, Config), `crates/arkos/src/effects/mod.rs` or a small new `crates/arkos/src/effects/scene.rs`, `crates/arkos/src/main.rs` (~6230 and ~6275, where `scene_index(self.theme.effect...scene)` is), `crates/arkos/src/settings.rs` (`look_edits`: the first Custom click pins the scene on screen too).

**Interfaces:**
- `CustomConfig` gains `#[serde(skip_serializing_if = "Option::is_none")] pub scene: Option<String>`. Produces `Config::scene_choice(&self) -> Option<&str>`: `custom.scene` while Custom is in force, else None.
- Produces `pub fn look_scene(config: &Config, theme: &Theme, themes_dir: &Path) -> Option<String>` (the scene name the look shows, before the manifest resolves it): Presets → `theme.effect`'s scene; Custom with `scene_choice()` → it; Custom with `effect_choice() == Theme` → `theme.effect`'s scene; Custom with a named effect and no scene → the scene of the preset the custom colour names, if it is a preset whose `[effect]` names that same effect (`theme::load(color).effect`), else None.
- `main.rs` passes `manifest.scene_index(look_scene(..).as_deref())` where it passed the theme's scene.
- `look_edits` (the first click on Custom, and `arkos color/font use` which reuse it) writes `custom.scene` from what is on screen when the effect showing has scenes.

- [ ] **Step 1: Failing tests:** `a_0_8_54_custom_matrix_look_with_runic_keeps_the_matrix_city`; `a_custom_look_with_runic_over_lavender_shows_tokyo`; `custom_scene_is_ignored_while_presets_are_in_force`; `a_custom_scene_picks_the_scene_whatever_the_colour`; `a_stray_scene_on_an_effect_without_scenes_changes_nothing` (resolves to index 0 and the wash input is unchanged); `the_first_click_on_custom_pins_the_scene_showing` (look_edits from Presets on The Matrix writes `custom.scene = "matrix-city"`).
- [ ] **Step 2: FAIL. Step 3: implement. Step 4: whole arkos crate PASS. Step 5: Commit** "Appearance: the scene in force comes from the look, custom.scene for Custom".

### Task 3: Custom's Effect list shows Runic as a group row

**Files:** Modify `crates/arkos/src/settings.rs` (`custom_effect_rows`, `custom()`, the Presets group row and card code ~1290 to ~1385, `open_group`, `GroupPicks`).

**Interfaces:**
- Produces `pub enum EffectRow { One(Row), Group { effect: String, title: String, members: Vec<(String, String)>, pick: String, current: bool } }` (members: (scene name, scene title)); `custom_effect_rows(choice, own, own_scene: Option<&str>, listed, manifests) -> Vec<EffectRow>`: an effect with two or more scenes is a `Group` titled by the effect (`Runic`), its pick the scene in force when it is current, else the remembered pick, else its first scene; others as today.
- Factor the Presets lists' group row and floating card into one helper used by both lists (same look, same keys, same close-on-outside-click), so the Custom Effect list gets the identical control. `open_group` becomes keyed so a Presets group and the Runic group never both count as open (an enum of where the group lives, or a string key; implementer's choice).
- A pick writes `custom.effect = "<effect>"` and `custom.scene = "<scene>"`; picking the row itself (not a member) picks its shown member. Remember the pick in `GroupPicks` under the key `effect:<effect>`.

- [ ] **Step 1: Failing tests:** `runic_is_one_group_row_with_tokyo_and_matrix_city`; `the_runic_row_shows_the_scene_in_force`; `picking_a_runic_scene_writes_the_effect_and_the_scene`; `an_effect_without_scenes_stays_a_plain_row`; `the_runic_group_remembers_its_last_pick`.
- [ ] **Step 2: FAIL. Step 3: implement. Step 4: whole arkos crate PASS. Step 5: Commit** "Settings: Runic is a group row in Custom's Effect list".

### Task 4: Commands and docs

**Files:** Modify `crates/arkos/src/effects/cli.rs`, `crates/arkos/src/agent_cli.rs` (`settings set custom.scene`), `docs/themes.md`, `docs/guide.md`, `docs/agent-guide.md`, `docs/development.md`.

**Interfaces:**
- `arkos effect use <name> [--scene <scene>]` and `arkos effect use <name>:<scene>`: writes `custom.effect` and `custom.scene` (an alias is written as its scene's own name); `use` without a scene on an effect with scenes leaves `custom.scene` as it was. An unknown scene exits 2 listing the effect's scenes. `arkos effect list` shows each effect's scenes ("runic: tokyo (Tokyo), matrix-city (Matrix City)"), and its `--json` gains a `scenes` array of `{name, title}`. `arkos effect current` includes the scene in force.
- `arkos settings set custom.scene <scene>` accepted (and validated against the custom effect's scenes when it has some).

- [ ] **Step 1: Failing tests:** `effect_use_with_a_scene_writes_both`; `effect_use_runic_colon_matrix_city_is_the_same`; `an_unknown_scene_exits_2_listing_the_scenes`; `effect_list_shows_runic_s_scenes`; `effect_current_says_the_scene`.
- [ ] **Step 2: FAIL. Step 3: implement. Step 4: PASS.**
- [ ] **Step 5: Docs.** themes.md: scenes are named `tokyo` and `matrix-city` (old `city`, `code` still read); guide.md Appearance: Custom's Runic row and its scenes; agent-guide.md: `--scene`, `custom.scene`; development.md Status row: "Runic scenes: Settings, Custom, Effect: Runic is one row 'runic tokyo ▾' whose card lists Tokyo and Matrix City; picking Matrix City over Lavender's colours draws the Matrix street; a Custom Matrix look from before keeps the Matrix City".
- [ ] **Step 6: Whole suite and check** (`nice -n 15 cargo test --workspace --no-fail-fast`; only the two known arkosd pty tests may fail), `nice -n 15 cargo check --workspace --all-targets` clean. **Commit** "Appearance: effect scenes in the commands and the docs".
