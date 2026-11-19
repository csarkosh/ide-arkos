# Appearance Stage 4: The Fonts Ship in the App Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every font a built-in preset uses is in the app, registered before the first window, so a preset looks the same on every Mac.

**Architecture:** The font files are committed in `crates/arkos/fonts/files/<family>/` with their licences (done, b071f64). A new module embeds them with `include_bytes!` and registers them with gpui's `TextSystem::add_fonts` at launch; gpui looks a family up in fonts added before the Mac's own, so the bundled family wins. The licences are copied into the app bundle by `packaging/bundle.sh`.

**Tech Stack:** Rust, gpui (pinned fork, `TextSystem::add_fonts`), shell packaging.

**Spec:** `docs/superpowers/specs/2026-11-18-appearance-entities-design.md`, "Bundled fonts", stage 4 of Stages.

## Global Constraints

- Files bundled (exactly these): `dm-sans/DMSans-{Regular,Italic,Light,LightItalic,SemiBold,SemiBoldItalic,Bold,BoldItalic}.ttf`, `roboto-mono/RobotoMono-{Regular,Italic,Bold,BoldItalic}.ttf`, `cascadia-code/CascadiaCode-{Regular,Italic,Bold,BoldItalic}.ttf`, `monaspace-radon/MonaspaceRadonFrozen-{Regular,Italic,Bold,BoldItalic}.ttf`, `departure-mono/DepartureMono-Regular.otf`. Licence files beside them: `OFL.txt` (dm-sans, roboto-mono), `LICENSE` (cascadia-code, monaspace-radon, departure-mono).
- Monaspace's static files name their family "Monaspace Radon Frozen", and its licence reserves the name "Monaspace" (no renamed copies), so the `notebook` font set's terminal family becomes "Monaspace Radon Frozen" (the same design with its default features fixed). This is the one deliberate font change to a built-in preset; the 0.8.54 fixture test asserts it explicitly rather than skipping it.
- The families macOS 14 ships, not bundled: Avenir Next, Baskerville, Optima, PT Mono, Chalkboard SE.
- The bundled fonts add no more than 10 MB (they are 7.4 MB).
- No font is synthesised; a family is drawn in the styles its files have (Departure Mono has one).
- Fonts are registered once, at launch, before any window opens, for this app only (never installed on the Mac).
- Tests headless, first, named as sentences; comments plain prose, no em-dashes; `nice -n 15`, never `--release`, one cargo at a time; `cargo check --workspace --all-targets` clean at the end.

## Review Focus

- A user who has a different version of one of these families installed: the bundled one is drawn (lookup order), and Settings' Font list no longer marks the set "(not installed)" (the families set includes added fonts: `all_font_names` returns the memory source's families too).
- A font set of the user's own naming a family that is neither bundled nor installed still falls back with its line in the log, as before.
- The release bundle carries the licences (`packaging/bundle.sh`), and the bundle check in `.github/workflows/release-notarized.yml` can find them.

---

### Task 1: Register the bundled fonts; the notebook set names the Frozen family; licences in the bundle; docs

**Files:** Create `crates/arkos/src/bundled_fonts.rs` (`mod bundled_fonts;` in main.rs). Modify where the app starts (main.rs, the `Application` run closure, before the first window), `crates/arkos/fonts/notebook.toml`, the fixture test in `crates/arkos/src/theme.rs` and the font-set test in `font_sets.rs`, `packaging/bundle.sh` (copy each family's licence to `Contents/Resources/Fonts/<family>/`), `.github/workflows/release-notarized.yml` (one check line that `Contents/Resources/Fonts/dm-sans/OFL.txt` exists, beside the channel check), `docs/themes.md` (the font table: the five families ship with Arkos; drop the `brew install --cask font-…` instructions; a short "Fonts that ship with Arkos" list with each licence), `docs/guide.md` (Appearance, fonts), `docs/development.md` (Status row).

**Interfaces:**
- `pub const FILES: &[(&str, &[u8])]`: (the family the file is in, its bytes), every file above, by `include_bytes!`.
- `pub const MACOS_FAMILIES: [&str; 5]`.
- `pub fn register(cx: &gpui::App)`: `cx.text_system().add_fonts(...)` with `Cow::Borrowed` bytes; a failure is said on stderr (`arkos: bundled fonts: …`) and the app goes on.

- [ ] **Step 1: Failing tests:**
  - `every_family_a_built_in_font_set_names_is_bundled_or_comes_with_macos` (every family in `font_sets::BUILTIN` is in `FILES` or `MACOS_FAMILIES`).
  - `every_bundled_file_is_in_the_family_it_is_listed_under`. Read each file's `name` table (family name ID 16 if present, else 1; a small reader in the test, or the `ttf-parser` crate if it is already resolvable as a direct dependency without a new download, else hand-rolled) and check it against the listed family.
  - `every_bundled_family_has_its_licence_beside_it`.
  - `the_bundled_fonts_add_no_more_than_10_mb`.
  - `the_notebook_set_draws_monaspace_radon_frozen`, with the fixture test asserting that notebook's terminal family is the one deliberate difference.
  - A packaging test if the repository has a pattern for testing `bundle.sh` (look for one); otherwise the workflow check line is the guard.
- [ ] **Step 2: Run, expect FAIL** (paste the line). **Step 3: Implement.** **Step 4: `nice -n 15 cargo test --workspace --no-fail-fast`**. Only the two known arkosd pty tests may fail. Then `nice -n 15 cargo check --workspace --all-targets` clean.
- [ ] **Step 5: Status row.** "Bundled fonts: on a Mac without DM Sans, Roboto Mono, Cascadia Code, Monaspace or Departure Mono installed (Font Book shows none), Material Navy draws DM Sans and Roboto Mono, Watercolor Lavender Cascadia Code, Composition Notebook Monaspace Radon Frozen and Timo Purple Departure Mono; Settings' Font list marks none of them not installed".
- [ ] **Step 6: Commit** "Appearance: the presets' fonts ship in the app".
