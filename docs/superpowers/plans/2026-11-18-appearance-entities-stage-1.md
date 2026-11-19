# Appearance Stage 1: Colour Schemes and Font Sets as Their Own Things Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Colour schemes and font sets become files of their own; presets name them; Custom lists each once; old configs and old-form theme files keep working.

**Architecture:** The app's runtime look stays `theme::Theme` (colours + fonts + effect, read from `root.theme` everywhere), so nothing that draws changes. What changes is how a name becomes a `Theme`: a preset file now names a colour scheme (`crates/arkos/colors/`, new module `color_schemes`) and a font set (`crates/arkos/fonts/`, new module `font_sets`), and `theme::load` resolves them; a name that is no preset but is a colour scheme loads as that scheme alone. User folders `colors/` and `fonts/` sit beside the themes folder.

**Tech Stack:** Rust, serde/toml, gpui (Settings view).

**Spec:** `docs/superpowers/specs/2026-11-18-appearance-entities-design.md` (sections The model, Settings, The config, The commands, Settled before planning; stage 1 of Stages).

## Global Constraints

- Every built-in preset draws exactly as in 0.8.54: same colours, fonts, effect, scene, appearance, title, group, group_default (Task 3's fixture test).
- A file of one's own with a built-in's name shadows the built-in (the file wins), as theme files do today (`theme::list`).
- Colour files: `name`, `title`, `appearance`, `[chrome]`, `[terminal]`, `[semantic]`, `[tree]`, `[viewer]` only (deny unknown fields). Font files: `name`, `title`, `prose`, `ui`, `terminal`, `grid` only. `grid` 1.0 to 24.0.
- New-form preset files: `name`, `title`, `group`, `group_default`, `color` (required), `font` (a font set's name or `default`; absent is `default`), `[effect]`. A new-form preset may not carry `appearance` or colour or font tables (its appearance is its colour's).
- User folders: `config_dir()/colors` and `config_dir()/fonts`, derived from a themes dir as `themes_dir.with_file_name("colors")` / `("fonts")` so tests can use a temp dir.
- Colours are read from `root.theme` at paint, never cached. Contrast floors are never loosened.
- Tests headless, written first, named as sentences; doc comments plain prose, no em-dashes; `nice -n 15`, never `--release`, one cargo at a time; `cargo check --workspace --all-targets` with no warnings at the end.

## Review Focus

- A config from 0.8.54 with `custom.color = "lavender-watercolor"` and no `custom.font`: the fonts are still that preset's (Avenir Next + Cascadia Code), the colours Lavender's, and the Color list checks Lavender (Task 4 migration test, Task 5 row test).
- A user themes folder holding extracted copies of all fourteen built-ins in the old all-in-one form (`theme extract` wrote them): they shadow the built-in presets and still load; their colours, identical to the built-in schemes, are not listed twice (Task 4 test).
- `custom.color = "navy"`: both a preset and a colour scheme are named navy; the colours are the same either way and the row checked is Navy (Task 4 test).
- A preset of one's own in the new form naming a colour in the user's `colors/` folder (Task 3 test), and one naming a colour that does not exist: an error naming the colour, and the window keeps its theme as `keep_or_switch` does today (Task 3 test).
- "Add color…" given a preset file: its colours are written out as a colour file and selected; given a colour file: copied in (Task 5 test).

---

### Task 1: Colour schemes as files

**Files:**
- Create: `crates/arkos/colors/{navy,ocean,palenight,sublime,bright,solar,lavender,peach,timo-purple,matrix,rain}.toml`
- Create: `crates/arkos/src/color_schemes.rs` (and `mod color_schemes;` in `main.rs`)

**Interfaces:**
- Produces `pub struct ColorScheme { pub name: String, pub title: Option<String>, pub appearance: Option<theme::Appearance>, pub chrome: theme::Chrome, pub terminal: theme::TerminalColors, pub semantic: theme::Semantic, pub tree: theme::TreeColors, pub viewer: theme::ViewerColors }` (serde, deny_unknown_fields, the same field attributes as `Theme`'s).
- Produces `pub const BUILTIN: [(&str, &str); 11]`, `pub fn parse(text: &str) -> Result<ColorScheme, String>` (the appearance-against-window_bg check `theme::parse` does), `pub fn builtin(name: &str) -> Option<ColorScheme>`, `pub fn dir_of(themes_dir: &Path) -> PathBuf`, `pub fn load(name: &str, themes_dir: &Path) -> Result<ColorScheme, String>` (the file `dir_of(themes_dir)/<name>.toml` shadows the built-in), `impl ColorScheme { pub fn to_theme(&self) -> theme::Theme; pub fn same_colors(&self, t: &theme::Theme) -> bool }`. `to_theme` gives a Theme with the scheme's name, title and appearance, no font, no effect, no group.
- Source of each file (copy the five tables verbatim from the theme file, then `name`, `title`, `appearance`): navy ← navy.toml ("Navy"), ocean ← material-ocean ("Ocean"), palenight ← material-palenight ("Palenight"), sublime ← material-sublime ("Sublime"), bright ← material ("Bright"), solar ← material-solar ("Solar"), lavender ← lavender-watercolor ("Lavender"), peach ← peach-watercolor ("Peach"), timo-purple ← timo-purple ("Timo Purple"), matrix ← matrix ("Matrix"), rain ← tokyo-rain ("Rain"). Header comment: one line saying what the scheme is and which presets use it, and "The schema of every key is in docs/themes.md."

- [ ] **Step 1: Write the failing tests** in `color_schemes.rs`:
  - `every_built_in_colour_scheme_is_the_colours_of_the_presets_it_came_from`: for each (scheme, preset) in navy/navy, ocean/material-ocean, palenight/material-palenight, sublime/material-sublime, bright/material, solar/material-solar, solar/composition-notebook, lavender/lavender-watercolor, lavender/lavender-material, peach/peach-watercolor, peach/arabia-solar, timo-purple/timo-purple, matrix/matrix, rain/tokyo-rain: `builtin(scheme).same_colors(&theme::builtin(preset))` and the appearances agree.
  - `every_built_in_colour_scheme_meets_the_contrast_rules`: `theme::contrast_report(&s.to_theme()).0` for all eleven.
  - `a_colour_file_in_the_colors_folder_shadows_the_built_in_of_its_name` (temp dir: `themes/` and `colors/navy.toml` with one colour changed; `load("navy", themes)` gives the changed colour).
  - `a_colour_file_with_a_font_or_effect_table_is_refused` and `a_colour_file_whose_appearance_disagrees_with_its_background_is_refused`.
- [ ] **Step 2: Run, expect FAIL** (`nice -n 15 cargo test -p arkos --bin arkos color_schemes`).
- [ ] **Step 3: Write the eleven files** (a throwaway script is fine; do not commit it) **and the module.**
- [ ] **Step 4: Run, expect PASS**; also `theme::` tests still pass.
- [ ] **Step 5: Commit** "Appearance: colour schemes as files of their own".

### Task 2: Font sets as files

**Files:** Create `crates/arkos/fonts/{material,lavender,peach,notebook,departure}.toml`, `crates/arkos/src/font_sets.rs` (`mod font_sets;`).

**Interfaces:**
- Produces `pub struct FontSet { pub name: String, pub title: Option<String>, pub prose: Option<String>, pub ui: Option<String>, pub terminal: Option<String>, pub grid: Option<f32> }` (deny_unknown_fields), `impl FontSet { pub fn font(&self) -> theme::ThemeFont; pub fn title(&self) -> String }` (title or `theme::title_case(name)`).
- Produces `pub const BUILTIN: [(&str, &str); 5]`, `pub fn parse(text) -> Result<FontSet, String>` (grid 1.0 to 24.0), `pub fn builtin(name) -> Option<FontSet>`, `pub fn dir_of(themes_dir: &Path) -> PathBuf`, `pub fn load(name, themes_dir) -> Result<FontSet, String>` (file shadows built-in).
- Files, copying the `[font]` values exactly from the source theme: material ("DM Sans + Roboto Mono") ← material.toml; lavender ("Avenir Next + Cascadia Code") ← lavender-watercolor; peach ("Baskerville + Optima + PT Mono") ← peach-watercolor; notebook ("Chalkboard + Monaspace Radon") ← composition-notebook; departure ("Departure Mono") ← timo-purple.

- [ ] **Step 1: Failing tests:** `every_built_in_font_set_is_the_fonts_of_the_presets_it_came_from` (material ← navy, material-ocean, material-palenight, material-sublime, material, material-solar; lavender ← lavender-watercolor, lavender-material; peach ← peach-watercolor; notebook ← composition-notebook; departure ← timo-purple, arabia-solar: `builtin(set).font() == theme::builtin(preset).font`); `every_built_in_font_set_has_the_title_the_design_gives` (the five titles above); `a_font_file_in_the_fonts_folder_shadows_the_built_in_of_its_name`; `a_font_file_with_a_grid_out_of_range_is_refused`.
- [ ] **Step 2: Run, expect FAIL.** **Step 3: files and module.** **Step 4: PASS.** **Step 5: Commit** "Appearance: font sets as files of their own".

### Task 3: Presets name their colour scheme and font set

**Files:**
- Create: `crates/arkos/tests-data/presets-0.8.54/*.toml` (the fourteen theme files exactly as on main before this task; commit them first, unchanged)
- Modify: `crates/arkos/src/theme.rs` (Theme, parse, builtin, load), the fourteen `crates/arkos/themes/*.toml`, every struct literal of `Theme` the compiler finds.

**Interfaces:**
- Theme gains `#[serde(skip)] pub color: String`: the name of the colour scheme it draws (a new-form preset's `color`; an old-form file's own `name`; `ColorScheme::to_theme` sets the scheme's name).
- Produces `struct PresetFile { name: String, title: Option<String>, group: Option<String>, #[serde(default)] group_default: bool, color: String, font: Option<String>, effect: Option<ThemeEffect> }` (deny_unknown_fields).
- Produces `pub fn parse_in(text: &str, themes_dir: Option<&Path>) -> Result<Theme, String>`: a text whose TOML has a top-level `color` key is a new-form preset, resolved with `color_schemes::load`/`font_sets::load` against `themes_dir` (or `builtin` when None; `font` absent or `"default"` is `ThemeFont::default()`); otherwise the old form exactly as `parse` does today, with `color = name`. A missing colour or font set is an error naming it ("colour scheme \"x\" is not built in and …/colors/x.toml does not exist"). `parse(text)` becomes `parse_in(text, None)`; `load` and `validate_file` use `parse_in(text, Some(dir))`; `builtin` uses `None`.
- Each built-in preset file becomes: its header comment (trim to what is still true; Tokyo Rain's must stop claiming Navy's colours), `name`, `title`, `group`/`group_default` as before, `color`, `font` (omitted when the preset had no `[font]`), and its `[effect]` table unchanged. Colour and font names per the tables in Tasks 1 and 2.

- [ ] **Step 1: Commit the fixture copies** "Appearance: the fourteen presets as they were, for the regression test".
- [ ] **Step 2: Failing tests:** `every_built_in_preset_draws_exactly_as_it_did_in_0_8_54` (for each fixture: `parse(fixture)` against `builtin(name)`: chrome, terminal, semantic, tree, viewer, font, effect, kind(), title, group, group_default all equal); `every_built_in_preset_is_in_the_new_form` (each embedded text has a top-level `color` and no `[chrome]`); `a_preset_in_the_old_all_in_one_form_still_loads` (a fixture copied into a temp themes dir under another name loads, `color` = its name); `a_preset_naming_a_colour_of_one_s_own_loads_it_from_the_colors_folder`; `a_preset_naming_a_missing_colour_scheme_is_an_error_naming_it`; `a_new_form_preset_may_not_set_its_own_appearance`. Replace `tokyo_rain_keeps_plain_navy_s_colours_with_the_runic_effect` with `tokyo_rain_draws_the_rain_colours_with_the_runic_city`.
- [ ] **Step 3: Run, expect FAIL.** **Step 4: Implement and rewrite the fourteen files.** **Step 5: Run `nice -n 15 cargo test -p arkos --bin arkos`, expect PASS** (the existing theme tests that list names and counts still hold: still fourteen presets). **Step 6: Commit** "Appearance: presets name their colour scheme and font set".

### Task 4: A name resolves to a scheme; the lists show each once; old configs read as before

**Files:** Modify `crates/arkos/src/theme.rs`.

**Interfaces:**
- `theme::load(name, dir)`: a preset (file, then built-in) first; else a colour scheme of that name (`color_schemes::load(name, dir)`), as `to_theme()`. Errors name both kinds when neither exists.
- Produces `pub struct ColorEntry { pub name: String, pub title: String, pub appearance: Option<Appearance>, pub source: Source }` and `pub fn colors(themes_dir: &Path) -> Vec<ColorEntry>`: the built-in schemes in `BUILTIN` order (a shadowing file listed in its place as `File`), the other colour files sorted (`Invalid` when they do not parse), then each old-form preset file in `themes_dir` whose colours match no entry already listed (`same_colors`), named by the preset's name and titled by its title.
- Produces `pub struct FontEntry { pub name: String, pub title: String, pub font: ThemeFont }` and `pub fn font_sets(themes_dir: &Path) -> Vec<FontEntry>`: the built-in sets, the other font files sorted, then each old-form preset file's non-default fonts matching no entry already listed, named and titled by the preset.
- `font_for(choice, theme, dir)`: `None` → `theme.font`; `PLAIN_FONT` → default; `Some(name)` → the font set `name` (file, then built-in) if there is one, else the fonts of the preset `name` (`load(name).font`, so a 0.8.54 `custom.font` naming a preset reads as before), else the error.

- [ ] **Step 1: Failing tests:**
  - `a_0_8_54_custom_colour_naming_a_preset_shows_that_preset_s_colour_scheme`: the full table from the spec (navy → navy, material-ocean → ocean, material-palenight → palenight, material-sublime → sublime, material → bright, material-solar → solar, composition-notebook → solar, lavender-material → lavender, lavender-watercolor → lavender, peach-watercolor → peach, arabia-solar → peach, timo-purple → timo-purple, matrix → matrix, tokyo-rain → rain), each `load(old).color == new`, and `RENAMED` old names too (`navy-runic` → rain, `peach-light` → peach, `lavender-light` → lavender).
  - `a_0_8_54_custom_font_naming_a_preset_draws_that_preset_s_font_set`: the spec's font table, `font_for(Some(preset), ..) == font_sets::builtin(set).font()` (matrix and tokyo-rain → default).
  - `a_colour_name_that_is_no_preset_loads_as_that_scheme_alone` (`load("solar")`: Solar's colours, default font, no effect).
  - `the_colour_list_shows_each_scheme_once` (built-ins only: eleven entries, names as Task 1).
  - `extracted_copies_of_the_built_in_presets_are_not_listed_twice` (run `theme::extract` into a temp themes dir from the fixtures' old-form texts; `colors()` still eleven, `font_sets()` still five).
  - `an_old_form_preset_with_colours_of_its_own_is_listed_under_its_title` (one colour changed).
  - `a_font_set_file_is_listed_by_its_title`.
- [ ] **Step 2: FAIL. Step 3: implement. Step 4: PASS (`cargo test -p arkos --bin arkos`). Step 5: Commit** "Appearance: names resolve to colour schemes and font sets, each listed once".

### Task 5: Settings, Custom: Color and Font lists by scheme and set; Add color; folders

**Files:** Modify `crates/arkos/src/settings.rs`, `crates/arkos/src/color_schemes.rs` (import).

**Interfaces:**
- `color_rows(colors: &[theme::ColorEntry], current: &str) -> Vec<Row>`: as today but over entries: label "Title (dark|light)", value the entry's name, `current` when `entry.name == current`; the "(not installed)" first row and "(invalid)" rows as today. The view passes `self.theme.color` as current.
- `font_rows(fonts: &[theme::FontEntry], choice, shown, plain, has)`: as today but each row labelled by the entry's title (no dedupe needed; keep the "not installed" marking by families).
- The view's `theme_fonts` becomes `Vec<theme::FontEntry>` and gains `theme_colors: Vec<theme::ColorEntry>`, refreshed wherever `themes`/`theme_fonts` are refreshed today.
- Produces `pub fn color_schemes::import_file(src: &Path, themes_dir: &Path, replace: bool) -> Result<(String, bool), String>`: a colour file is copied into `dir_of(themes_dir)` under its own name; a preset file (either form, parsed with `theme::parse_in(text, Some(themes_dir))`) has its colours written out as a colour file named after the preset (`name`, `title`, `appearance`, five tables). Existing name without `replace`: `Err("exists")`, as `theme::import_file`.
- "Add color…" uses it and selects the colour (`color_edits` as today). Custom gets "Show fonts folder" beside "Show colors folder"; "Show colors folder" opens `color_schemes::dir_of(themes_dir)` (create the folder if missing, as the effects folder pill does).

- [ ] **Step 1: Failing tests** (settings tests): `the_custom_colour_list_shows_lavender_once`, `the_custom_colour_row_checked_is_the_scheme_showing_for_an_old_preset_name` (current "lavender" from a theme loaded as `lavender-watercolor`), `the_custom_font_list_labels_each_set_by_its_title`, `adding_a_preset_as_a_colour_writes_its_colours_out_as_a_colour_file`, `adding_a_colour_file_copies_it_into_the_colors_folder`. Update the existing color_rows/font_rows tests to the new inputs.
- [ ] **Step 2: FAIL. Step 3: implement. Step 4: PASS. Step 5: Commit** "Settings: Custom lists each colour scheme and font set once".

### Task 6: Commands and docs

**Files:** Modify `crates/arkos/src/agent_cli.rs` (or a new `color_cli.rs` beside `effects/cli.rs`, wired where `arkos effect` is), `docs/themes.md`, `docs/guide.md`, `docs/agent-guide.md`, `docs/development.md`.

**Interfaces:**
- `arkos color list [--json]` (name, title, dark/light, built in/file/invalid), `arkos color use <name>`, `arkos color current`; `arkos font list [--json]`, `arkos font use <name|default>`, `arkos font current`. `use` behaves as `arkos effect use` does for the effect: writes `custom.color` / `custom.font` and puts Custom in force, keeping the other two; when Custom had no colour yet, it first pins what is on screen exactly as Settings' first Custom click does (reuse `color_edits` or the helper `effect use` uses). An unknown name exits 2 naming the list command.
- `arkos theme contrast <name|file>` accepts a colour scheme's name or file as well as a preset's.
- `arkos settings set custom.color/custom.font` accept scheme and set names (and still preset names).

- [ ] **Step 1: Failing tests** in the CLI test module: `color_list_names_each_scheme_once`, `color_use_puts_custom_in_force_with_that_scheme`, `font_use_default_sets_the_plain_fonts`, `an_unknown_colour_exits_2_naming_color_list`, `theme_contrast_takes_a_colour_scheme_s_name`.
- [ ] **Step 2: FAIL. Step 3: implement. Step 4: PASS.**
- [ ] **Step 5: Docs.** `docs/themes.md`: a colour file, a font file and a preset file each described (keys, the folders, shadowing, old form still loading); `docs/guide.md` Appearance: Custom's lists show each scheme and set once, the colors and fonts folders; `docs/agent-guide.md`: the `color` and `font` commands and what `custom.color`/`custom.font` hold. `docs/development.md` Status row: "Appearance entities: Settings, Custom: Lavender, Solar and Peach each listed once; a 0.8.54 Custom look (colour and font picked by preset name) looks the same after the update; Add color with a preset file adds its colours; Show fonts folder opens it".
- [ ] **Step 6: Whole suite and check:** `nice -n 15 cargo test --workspace --no-fail-fast` (the two `arkosd` pty tests fail in this environment every run; any other failure is real), `nice -n 15 cargo check --workspace --all-targets` with no warnings. **Commit** "Appearance: color and font commands, and the docs".
