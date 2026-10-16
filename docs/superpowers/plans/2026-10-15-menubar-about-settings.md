# Menubar, About, Settings and Timo Purple (milestone 8a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One "Arkos IDE" menu (About, New Window, Settings, Quit), an About window with the build, update, install and path details and the attribution, a Settings window that writes `config.toml` in place and applies live, TOML theme import with `arkos theme list` and `validate`, and the Timo Purple theme built in.

**Architecture:** Pure first: `config::set_value` on `toml_edit` keeps the file's comments; `about::Details` is one struct the window and the clipboard both render; `startup::installed_since` writes the first-installed marker; the theme schema is documented and Timo Purple is a built-in file with a contrast test; `arkos theme` is a headless subcommand. Then two gpui windows (`about.rs`, `settings.rs`) opened from keymap actions through the chooser's single-window pattern, and the one menu.

**Tech Stack:** Rust 2021, gpui 0.2.2 (`Menu`, `MenuItem::separator`, `open_window` with `WindowOptions { is_resizable, window_min_size }`, `prompt_for_paths`, `reveal_path`, `text_system().all_font_names()`), `toml_edit` 0.22 (new dependency), `objc2-foundation` `NSProcessInfo` (already used by app_nap.rs). No other new dependency.

**Spec:** `docs/superpowers/specs/2026-10-15-menubar-about-settings-design.md` (approved 2026-10-15; its section 1 rulings are binding: the one menu in that order, the attribution and donation lines, first installed go-forward, TOML import only, Timo Purple from Timo's three Codex values and the screenshot).

## Global Constraints

- Machine rules: every cargo call is `nice -n 15 ~/.cargo/bin/cargo ...`; the worktree's `.cargo/config.toml` (`jobs = 3`, shared `target-dir`) stays; no `--release`; never two cargo processes at once; focused filters while iterating, the workspace suite once per task; `cargo check --workspace --all-targets` must print no warning from our crates, in the test build too. Two daemon tests (`concurrent_spawns_give_each_shell_only_its_own_terminal`, `the_grace_counts_from_the_relaunch_so_a_slow_relaunch_keeps_the_old_bundle`) are known timing flakes under load: rerun alone and report both results.
- Dead code between tasks: an item a later task first uses, reached by a test here, gets `#[cfg_attr(not(test), allow(dead_code))] // Used from Task N on.`; unreached items get `#[allow(dead_code)] // Used from Task N on.`; the task that first uses it removes the attribute; after Task 5 `grep -rn "Used from Task" crates/arkos/src` prints nothing.
- Menu (exact labels, in order): "About Arkos IDE", separator, "New Window", "Settings", "Quit"; one menu named "Arkos IDE"; the File menu removed. Keymap: "about" (no key), "settings" (`cmd-,`); "open config" keeps its palette entry and loses its default key. Palette labels: "About Arkos IDE" and "Settings" (not the keymap's capitalised names, so the label match in `command_palette::items` gets explicit arms).
- About window: title "About Arkos IDE", 420 by 520 px, not resizable, centred, single instance. Rows (labels exact): "Version", "Build" ("{hash}, {date}"), "Last updated", "First installed", "Daemon" ("{version}, pid {pid}" or "not connected"), "Protocol", "macOS" ("{version} {arch}"), "Config", "Themes", "App Support", "Window log", "Daemon log". Dates render "today HH:MM" for the current day, else "YYYY-MM-DD". "Copy details" copies `Details::clipboard_text()`: one "label: value" line per row with a "Arkos IDE {version}" first line. Attribution: "Made by Cyrus Sarkosh" (the name links to https://github.com/csarkosh); under it "Buy me more tokens" followed by a spark glyph "✦" in `#d97757` and a heart "❤" in `#e5484d`, the whole line opening `about::DONATE_URL` and drawn only when that constant is not empty (it ships empty).
- Settings window: title "Settings", 560 by 480 px, resizable, minimum 480 by 360, single instance; sections in order "Appearance", "Text", "Startup", "Updates", "Keyboard"; control texts in section 6 of the spec. Every change writes `config.toml` through `config::set_value` and calls `reload_config` on every open project window at once.
- `config::set_value(path, keys: &[&str], value: toml_edit::Value) -> Result<(), String>`: comments, order and formatting kept; a key added at the end of its existing table; a missing table appended at the end of the file; the write goes through a temp file beside the target and a rename; an unparseable file is refused with its parse error and left as it was.
- First installed: `<app support>/installed` holds the unix seconds of the first run as text; `startup::installed_since(file, now) -> SystemTime` reads or writes it; garbage counts as missing.
- Build date: `build.rs` sets `ARKOS_BUILD_DATE` to the UTC date "YYYY-MM-DD" (from `date -u +%F` or `SOURCE_DATE_EPOCH` when set); `main.rs` exposes `build_date()`.
- Themes: `docs/themes.md` documents every table and key; `timo-purple.toml` (name `timo-purple`) joins `theme::BUILTIN` as the third entry; every text colour in it has contrast at least 4.5:1 against the surface it sits on (`palette::contrast`); the plan's starting values are in Task 2 and may be tuned, with the test as the gate. `arkos theme list` prints one line per theme, "{name} (built in)", "{name} (file)" or "{name} (file, invalid: {error})", files from `paths::themes_dir()`, sorted, built-ins first; `arkos theme validate <file>` prints "ok: {name}" and exits 0, or the error and exits 1.
- Import: "Add theme…" accepts only `.toml`; a valid file is copied to `<themes dir>/<name>.toml` (the file's `name`), replacing an older copy of the same name after a confirm line "Replace the existing {name}?" with "Replace" and "Keep"; the imported theme is selected for the current mode. The error shown for a bad file is `theme::parse`'s message.
- No em-dashes in any file. Comments are full sentences. Headless tests only: no test opens a window, a file picker, Finder, or touches real user files.

## Review Focus

1. A `config.toml` with a `[font.presets.comfortable]` table and no `[appearance]` table: setting the mode appends `[appearance]` at the end and leaves every comment in place (Task 1 `set_value_keeps_comments_and_adds_what_is_missing`).
2. "Copy details" when the daemon is not connected and the update journal is absent: every row still has a value, no "None" leaks (Task 3 `details_render_with_nothing_known`).
3. A theme file whose `name` differs from its file name: import copies it under the `name` and the picker shows that name (Task 4 `import_uses_the_theme_s_own_name`).
4. Timo Purple's dim text on the banner surface and its ANSI yellow on the background: both at least 4.5:1 (Task 2 `timo_purple_reads_on_every_surface`).
5. Cmd+, in a terminal pane opens Settings rather than the config file, and a config that binds "open config" to `cmd-,` keeps the old behaviour (Task 3 keymap tests).

## File structure

- Create `crates/arkos/src/about.rs`: `Details`, `DONATE_URL`, formatting helpers, the About window.
- Create `crates/arkos/src/settings.rs`: the Settings window and its controls, the import flow.
- Create `crates/arkos/themes/timo-purple.toml`, `docs/themes.md`.
- Modify `crates/arkos/src/config.rs` (`set_value`), `crates/arkos/src/startup.rs` (`installed_since`), `crates/arkos/src/paths.rs` (`installed_file`), `crates/arkos/build.rs` (`ARKOS_BUILD_DATE`), `crates/arkos/src/theme.rs` (`BUILTIN`, `validate_file`, `list`), `crates/arkos/src/main.rs` (`menus`, actions, the `theme` subcommand, window openers, `build_date`), `crates/arkos/src/keymap.rs`, `command_palette.rs`, `README.md`, `Cargo.toml` and `crates/arkos/Cargo.toml` (`toml_edit`).

---

### Task 1: Config edits that keep comments, the install marker, the build date

**Files:**
- Modify: `Cargo.toml` (workspace dependency `toml_edit = "0.22"`), `crates/arkos/Cargo.toml` (`toml_edit.workspace = true`), `Cargo.lock`
- Modify: `crates/arkos/src/config.rs` (add `set_value` near `write_default` at ~333; tests at the end), `crates/arkos/src/startup.rs` (`installed_since`; tests in its module), `crates/arkos/src/paths.rs` (`installed_file`; one test), `crates/arkos/build.rs` (the date), `crates/arkos/src/main.rs` (`pub fn build_date() -> &'static str { env!("ARKOS_BUILD_DATE") }` next to `version_string` at ~3490)

**Interfaces:**
- Produces (all with the dead-code attribute "Used from Task 3 on." for `installed_since`, `installed_file`, `build_date`; "Used from Task 4 on." for `set_value`):
  ```rust
  pub fn set_value(path: &Path, keys: &[&str], value: toml_edit::Value) -> Result<(), String>;
  pub fn installed_since(file: &Path, now: SystemTime) -> SystemTime;
  pub fn installed_file() -> PathBuf;
  ```

- [ ] **Step 1: Failing tests**

```rust
// config.rs
#[test]
fn set_value_keeps_comments_and_adds_what_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "# top comment\n[font]\n# the family\nfamily = \"Menlo\"\npreset = \"comfortable\"\n\n[font.presets.comfortable]\nsize = 16.0\nline_height = 1.5\n").unwrap();
    set_value(&path, &["font", "presets", "comfortable", "size"], toml_edit::Value::from(18.0)).unwrap();
    set_value(&path, &["appearance", "mode"], toml_edit::Value::from("dark")).unwrap();
    set_value(&path, &["font", "family"], toml_edit::Value::from("SF Mono")).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# top comment\n[font]\n# the family\nfamily = \"SF Mono\"\n"), "{text}");
    assert!(text.contains("size = 18.0\nline_height = 1.5\n"), "{text}");
    assert!(text.ends_with("[appearance]\nmode = \"dark\"\n"), "a missing table is appended: {text}");
    let parsed: Config = toml::from_str(&text).unwrap();
    assert_eq!(parsed.font.family, "SF Mono");
    assert!(std::fs::read_dir(dir.path()).unwrap().count() == 1, "no temp file left");
}

#[test]
fn set_value_refuses_a_file_it_cannot_parse_and_round_trips_the_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[font\nfamily = 1\n").unwrap();
    let err = set_value(&path, &["font", "family"], toml_edit::Value::from("x")).unwrap_err();
    assert!(!err.is_empty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[font\nfamily = 1\n", "left as it was");
    std::fs::write(&path, DEFAULT_CONFIG).unwrap();
    set_value(&path, &["update", "auto"], toml_edit::Value::from(false)).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches('#').count(), DEFAULT_CONFIG.matches('#').count(), "every comment survives");
    assert!(toml::from_str::<Config>(&text).is_ok());
}

// startup.rs
#[test]
fn the_install_marker_is_written_once_and_read_after() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("installed");
    let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    assert_eq!(installed_since(&file, t0), t0, "missing: written now");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "1800000000\n");
    assert_eq!(installed_since(&file, t0 + Duration::from_secs(99)), t0, "read back, not rewritten");
    std::fs::write(&file, "garbage").unwrap();
    let t1 = t0 + Duration::from_secs(5);
    assert_eq!(installed_since(&file, t1), t1, "garbage counts as missing and is replaced");
}
```

And in `paths.rs` tests: `assert!(installed_file().ends_with("arkos/installed"))`.

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- config::tests::set_value startup:: paths::`

- [ ] **Step 3: Implement**

`set_value`: read the text; `text.parse::<toml_edit::DocumentMut>()` (the error's first line on failure); walk `keys[..n-1]` creating implicit tables with `entry(key).or_insert(toml_edit::table())` and setting `set_implicit(false)` only on the leaf table so headers print; set the leaf `doc[..][last] = toml_edit::value(value)`; write to `<file>.<pid>.tmp` beside the target and rename (reuse the pattern in `workspace::save_to`). `installed_since`: parse the file's trimmed text as `u64` seconds; else write `format!("{}\n", now_secs)` through a temp file and return `now`. `build.rs`: `let date = std::env::var("SOURCE_DATE_EPOCH").ok().and_then(|s| s.parse::<i64>().ok()).map(ymd_from_epoch).unwrap_or_else(|| today_utc())` where `today_utc` runs `date -u +%F` (fallback "unknown"); `println!("cargo:rustc-env=ARKOS_BUILD_DATE={date}")`.

- [ ] **Step 4: Run, check, commit**

```bash
git add Cargo.toml Cargo.lock crates/arkos/Cargo.toml crates/arkos/src/config.rs crates/arkos/src/startup.rs crates/arkos/src/paths.rs crates/arkos/build.rs crates/arkos/src/main.rs
git commit -m "Config edits keep comments; an install marker; the build date"
```

---

### Task 2: The theme schema, Timo Purple, and `arkos theme`

**Files:**
- Create: `docs/themes.md`, `crates/arkos/themes/timo-purple.toml`
- Modify: `crates/arkos/src/theme.rs` (`BUILTIN` at 104; add `validate_file`, `list`; tests), `crates/arkos/src/main.rs` (the `theme` subcommand next to `doctor` at ~3502), `crates/arkos/themes/navy.toml` and `navy-light.toml` (a two-line header pointing at `docs/themes.md`)

**Interfaces:**
- Produces:
  ```rust
  pub const BUILTIN: [(&str, &str); 3]; // navy, navy-light, timo-purple
  /// Parses a theme file: Ok(name) or the parse error (a missing file is an error too).
  pub fn validate_file(path: &Path) -> Result<String, String>;
  #[derive(Debug, Clone, PartialEq, Eq)] pub enum Source { BuiltIn, File, Invalid(String) }
  /// Every theme the pickers offer: built-ins first, then the folder's files sorted by name; a file that shadows a built-in is listed once as File.
  pub fn list(dir: &Path) -> Vec<(String, Source)>;
  pub fn theme_cli(args: &[String]) -> i32; // "list" | "validate <file>"; usage on anything else, exit 2
  ```

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn timo_purple_is_built_in_and_reads_on_every_surface() {
    let t = builtin("timo-purple").expect("ships");
    assert_eq!(t.name, "timo-purple");
    assert_eq!(BUILTIN.len(), 3);
    let c = &t.chrome;
    let pairs = [
        (c.text, c.window_bg), (c.text, c.title_bar), (c.text, c.sidebar), (c.text, c.tab_active), (c.text, c.tab_inactive), (c.text, c.banner),
        (c.text_dim, c.window_bg), (c.text_dim, c.banner), (c.text_dim, c.sidebar),
        (t.terminal.foreground, t.terminal.background),
        (t.terminal.red, t.terminal.background), (t.terminal.green, t.terminal.background), (t.terminal.yellow, t.terminal.background), (t.terminal.blue, t.terminal.background), (t.terminal.magenta, t.terminal.background), (t.terminal.cyan, t.terminal.background), (t.terminal.white, t.terminal.background),
        (t.terminal.bright_black, t.terminal.background),
    ];
    for (fg, bg) in pairs {
        assert!(crate::palette::contrast(fg, bg) >= 4.5, "{fg:?} on {bg:?} is {:.2}", crate::palette::contrast(fg, bg));
    }
    assert_eq!(t.chrome.focus_ring, t.terminal.cyan, "the accent is the cyan");
}

#[test]
fn the_list_and_the_validator_name_every_theme() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mine.toml"), BUILTIN[0].1.replace("name = \"navy\"", "name = \"mine\"")).unwrap();
    std::fs::write(dir.path().join("broken.toml"), "name = \"broken\"\n[chrome]\nwindow_bg = 1\n").unwrap();
    std::fs::write(dir.path().join("navy.toml"), BUILTIN[0].1).unwrap();
    let l = list(dir.path());
    assert_eq!(l[0], ("navy".into(), Source::File), "a file shadows the built-in and is listed once");
    assert_eq!(l[1], ("navy-light".into(), Source::BuiltIn));
    assert_eq!(l[2], ("timo-purple".into(), Source::BuiltIn));
    assert!(matches!(&l[3], (n, Source::Invalid(e)) if n == "broken" && !e.is_empty()));
    assert_eq!(l[4], ("mine".into(), Source::File));
    assert_eq!(validate_file(&dir.path().join("mine.toml")), Ok("mine".into()));
    assert!(validate_file(&dir.path().join("broken.toml")).is_err());
    assert!(validate_file(&dir.path().join("none.toml")).is_err());
}
```

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- theme::`

- [ ] **Step 3: Implement**

`timo-purple.toml`, starting values (tune only to pass the contrast test; keep the look of the screenshot: a deep purple ground, cyan accents, green filled boxes):

```toml
# Timo Purple: Timo's Codex theme (accent #00ffff, background #330076, contrast 70) carried over
# to Arkos. The schema is in docs/themes.md.
name = "timo-purple"
[chrome]
window_bg = "#330076"
title_bar = "#2a0062"
status_bar = "#2a0062"
sidebar = "#2a0062"
tab_bar = "#2a0062"
tab_active = "#330076"
tab_inactive = "#2a0062"
border = "#4b2a98"
split_handle = "#4b2a98"
split_handle_hover = "#7a5ad0"
focus_ring = "#00ffff"
text = "#f3edff"
text_dim = "#c4b6ea"
banner = "#3b1a86"
[terminal]
background = "#330076"
foreground = "#f3edff"
cursor = "#00ffff"
selection = "#1f7a52"
black = "#2a0062"
red = "#ff8a9e"
green = "#5fe39a"
yellow = "#ffd166"
blue = "#8fb8ff"
magenta = "#e39bff"
cyan = "#00ffff"
white = "#e9e2ff"
bright_black = "#9a86d0"
bright_red = "#ffb3c0"
bright_green = "#8df2b8"
bright_yellow = "#ffe08a"
bright_blue = "#b6cfff"
bright_magenta = "#efbfff"
bright_cyan = "#9dffff"
bright_white = "#ffffff"
[semantic]
working = "#00ffff"
waiting = "#ffd166"
bell = "#ff8a9e"
error_banner = "#5a1a3a"
find_match = "#1f7a52"
find_current = "#2fa56f"
```

plus the `[tree]` and `[viewer]` tables with the same keys as navy.toml (added, modified, deleted and the viewer colours; take navy's hues lifted to pass contrast on `#330076`). `docs/themes.md`: one section per table listing each key, what it paints and which surface it must contrast with; the 4.5:1 rule; how to add a theme (the file in the themes folder, `arkos theme validate`, pick it in Settings or the config). `list` and `validate_file` as the interface says; `theme_cli` in `theme.rs` (prints with `println!`, errors to stderr), wired in `main()` like `doctor`: `if args.get(1).map(String::as_str) == Some("theme") { std::process::exit(theme::theme_cli(&args[2..])); }`.

- [ ] **Step 4: Run, check, commit**

Run the theme tests, the warning check, the suite.

```bash
git add docs/themes.md crates/arkos/themes crates/arkos/src/theme.rs crates/arkos/src/main.rs
git commit -m "Timo Purple ships; the theme schema is documented; arkos theme list and validate"
```

---

### Task 3: The one menu, the About window, and the two actions

**Files:**
- Create: `crates/arkos/src/about.rs`
- Modify: `crates/arkos/src/keymap.rs` (`actions!`, `Command`, `ACTIONS` rows: `row("about", "")` and `row("settings", "cmd-,")` after `row("open config", "cmd-,")`, which becomes `row("open config", "")`; `binding()`, `command_for`, `route!`, SPEC_ROWS; the count assertions at ~603 and ~612 go from `- 12`/`- 11` to `- 14`/`- 13` with "open config, about" added to their messages), `crates/arkos/src/command_palette.rs` (label arms "About Arkos IDE" and "Settings"; detail for Settings "Cmd+,"; tests), `crates/arkos/src/config.rs` (DEFAULT_CONFIG: `# "open config" = ""`, `# "about" = ""`, `# "settings" = "cmd-,"`), `crates/arkos/src/main.rs` (`menus()` at ~3462; `Root.about: Option<WindowHandle<about::AboutWindow>>`; `open_about`; `run` arms `About => self.open_about(cx)`, `OpenSettings => self.open_settings(cx)` where `open_settings` is a stub that shows the notice "Settings arrive in the next task" until Task 4 replaces it; `on_action` for `About` and `OpenSettings` on the chooser's app too, next to the Quit handlers at ~3588 and ~3876, so the menu works with no project window), `crates/arkos/src/startup.rs` (`installed_since` called in `main()` next to `updated_from` at ~3542 and carried in `Startup.installed: SystemTime`)

**Interfaces:**
- Produces in `about.rs`:
  ```rust
  pub const DONATE_URL: &str = "";
  pub const AUTHOR: &str = "Cyrus Sarkosh"; pub const AUTHOR_URL: &str = "https://github.com/csarkosh";
  #[derive(Debug, Clone, PartialEq, Eq)] pub struct Details { pub version: String, pub build_hash: String, pub build_date: String, pub last_updated: Option<SystemTime>, pub first_installed: SystemTime, pub daemon: Option<(String, u32)>, pub protocol: u32, pub macos: String, pub arch: &'static str, pub config: PathBuf, pub themes: PathBuf, pub app_support: PathBuf, pub window_log: PathBuf, pub daemon_log: PathBuf }
  impl Details { pub fn rows(&self, now: SystemTime) -> Vec<(&'static str, String)>; pub fn clipboard_text(&self, now: SystemTime) -> String; }
  pub fn format_when(t: SystemTime, now: SystemTime) -> String; // "today HH:MM" (local) or "YYYY-MM-DD"
  pub fn last_updated(updates_dir: &Path, bundle: Option<&Path>) -> Option<SystemTime>; // applied.json's at_unix, else the bundle's mtime
  pub fn macos_version() -> String; // NSProcessInfo operatingSystemVersionString
  pub struct AboutWindow { details: Details, theme: Theme }
  pub fn open(cx: &mut App, details: Details, theme: Theme) -> WindowHandle<AboutWindow>;
  ```

- [ ] **Step 1: Failing tests**

```rust
// about.rs
#[test]
fn details_render_with_nothing_known() {
    let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let d = Details { version: "0.3.1".into(), build_hash: "abc1234".into(), build_date: "2026-10-15".into(), last_updated: None, first_installed: t0, daemon: None, protocol: 5, macos: "26.1".into(), arch: "arm64", config: "/c/config.toml".into(), themes: "/c/themes".into(), app_support: "/a".into(), window_log: "/a/window.log".into(), daemon_log: "/a/arkosd.log".into() };
    let rows = d.rows(t0 + Duration::from_secs(30 * 86_400));
    let get = |l: &str| rows.iter().find(|(k, _)| *k == l).map(|(_, v)| v.clone()).unwrap_or_else(|| panic!("{l}"));
    assert_eq!(get("Version"), "0.3.1");
    assert_eq!(get("Build"), "abc1234, 2026-10-15");
    assert_eq!(get("Last updated"), "unknown");
    assert_eq!(get("First installed"), "2027-01-15".to_string().replace("2027-01-15", &format_when(t0, t0 + Duration::from_secs(30 * 86_400))));
    assert_eq!(get("Daemon"), "not connected");
    assert_eq!(get("Protocol"), "5");
    assert_eq!(get("macOS"), "26.1 arm64");
    assert_eq!(get("Config"), "/c/config.toml");
    let text = d.clipboard_text(t0);
    assert!(text.starts_with("Arkos IDE 0.3.1\n"));
    assert!(text.contains("\nDaemon: not connected\n"));
    assert!(!text.contains("None"));
}

#[test]
fn dates_say_today_or_the_day() {
    let now = SystemTime::now();
    assert!(format_when(now - Duration::from_secs(60), now).starts_with("today "));
    let old = now - Duration::from_secs(3 * 86_400);
    let s = format_when(old, now);
    assert_eq!(s.len(), 10);
    assert_eq!(s.as_bytes()[4], b'-');
}

#[test]
fn last_updated_prefers_the_journal_then_the_bundle() {
    let dir = tempfile::tempdir().unwrap();
    let bundle = dir.path().join("Arkos IDE.app");
    std::fs::create_dir(&bundle).unwrap();
    assert!(last_updated(dir.path(), Some(&bundle)).is_some(), "the bundle's mtime");
    assert_eq!(last_updated(dir.path(), None), None);
    std::fs::write(dir.path().join("applied.json"), r#"{"version":"0.3.0","at_unix":1800000000}"#).unwrap();
    assert_eq!(last_updated(dir.path(), Some(&bundle)), Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)));
}
```

Keymap and palette tests: `command_for("about") == Some(Command::About)`, `command_for("settings") == Some(Command::OpenSettings)`, `keys_for("settings", &BTreeMap::new()) == Some("cmd-,")`, `keys_for("open config", &BTreeMap::new()) == None`, and an override `"open config" = "cmd-,"` makes `keys_for("open config", &o)` Some again; the palette finds "About Arkos IDE" and "Settings" with detail "Cmd+,".

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- about:: keymap:: command_palette:: config::`

- [ ] **Step 3: Implement**

- `Details::rows` in the constraint's order; `format_when` through `libc::localtime_r` as `notices::relative_time` does; `last_updated` reads `applied.json` with serde (`at_unix`) else `std::fs::metadata(bundle)?.modified()`; `macos_version` via `objc2_foundation::NSProcessInfo::processInfo().operatingSystemVersionString()`; `arch` is `std::env::consts::ARCH`.
- The window: `cx.open_window(WindowOptions { window_bounds: Some(Windowed(Bounds::centered(None, size(px(420.), px(520.)), cx))), titlebar: Some(TitlebarOptions { title: Some("About Arkos IDE".into()), appears_transparent: false, traffic_light_position: None }), is_resizable: false, focus: true, ..Default::default() }, ..)` with a view rendering: the name at 20 px semibold, the version, the rows as a two-column `div` table (label `text_dim` 13 px, value `text`), path rows with `cursor_pointer` and `on_click` calling `cx.reveal_path(&path)`, a "Copy details" button (the pill style from `pane_view`'s jump pill) calling `cx.write_to_clipboard(ClipboardItem::new_string(details.clipboard_text(now)))`, then the attribution block: "Made by " plus the name as a link (`cx.open_url(AUTHOR_URL)`), and, when `!DONATE_URL.is_empty()`, the donation line with "Buy me more tokens " then "✦" in `#d97757` and "❤" in `#e5484d`, the whole row clickable to `cx.open_url(DONATE_URL)`. Colours from the project's theme (`chrome.window_bg`, `text`, `text_dim`, `focus_ring` for links). Escape or Cmd+W closes it (`on_key_down` for escape: `window.remove_window()`).
- `Root::open_about`: build `Details` (`daemon` from `self.daemon_version` and the connection's pid if known, else None; `last_updated(&paths::app_support().join("updates"), update::launch_path().as_deref().and_then(bundle_of))`; `first_installed` from `Startup.installed`), then `about::open`; keep the handle and `activate_window` when it is already open (the chooser pattern at `new_window`, main.rs ~2551).
- The chooser's app (`main()` near the first `on_action(Quit)` at ~3588) gets `on_action` handlers for `About` and `OpenSettings` that open the windows with the loaded config and theme, so the menu works before any project window.
- `menus()` as the constraint says; `MenuItem::separator()`.

- [ ] **Step 4: Run, check, commit**

Run the four modules' tests, the warning check, the suite.

```bash
git add crates/arkos/src/about.rs crates/arkos/src/main.rs crates/arkos/src/keymap.rs crates/arkos/src/command_palette.rs crates/arkos/src/config.rs crates/arkos/src/startup.rs
git commit -m "One menu; the About window with the build, update and install details"
```

---

### Task 4: The Settings window

**Files:**
- Create: `crates/arkos/src/settings.rs`
- Modify: `crates/arkos/src/main.rs` (`Root.settings: Option<WindowHandle<settings::SettingsWindow>>`, `open_settings` replacing the stub, `reload_config` also refreshes an open Settings window, the chooser app's handler), `crates/arkos/src/config.rs` (remove the "Used from Task 4" attribute), `crates/arkos/src/theme.rs` (import helper `import_file(src, dir) -> Result<(String, bool /* replaced */), String>`, with a test)

**Interfaces:**
- Produces:
  ```rust
  // settings.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Section { Appearance, Text, Startup, Updates, Keyboard }
  pub struct SettingsWindow { config: Config, section: Section, themes: Vec<(String, theme::Source)>, fonts: Vec<String>, error: Option<String>, pending_replace: Option<(PathBuf, String)>, on_change: Box<dyn Fn(&[&str], toml_edit::Value, &mut App)>, ... }
  pub fn open(cx: &mut App, config: Config, theme: Theme, on_change: ..., on_start_at_login: Box<dyn Fn(bool, &mut App)>, update_ready: Option<String>, on_update: Box<dyn Fn(&mut App)>) -> WindowHandle<SettingsWindow>;
  /// Pure: the presets' names and the chosen one's size and line height, for the Text section.
  pub fn preset_rows(cfg: &FontConfig) -> (Vec<String>, String, f32, f32);
  pub fn monospace_families(all: &[String]) -> Vec<String>; // pure filter by the known fixed-pitch names plus anything containing "Mono", "Code", "Courier", "Menlo", "Monaco", "Consolas"; sorted, deduplicated
  // theme.rs
  pub fn import_file(src: &Path, dir: &Path, replace: bool) -> Result<(String, bool), String>; // Ok((name, existed)); Err("exists") when a same-named file exists and replace is false
  ```

- [ ] **Step 1: Failing tests**

```rust
// theme.rs
#[test]
fn import_uses_the_theme_s_own_name_and_asks_before_replacing() {
    let src_dir = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let src = src_dir.path().join("whatever.toml");
    std::fs::write(&src, BUILTIN[0].1.replace("name = \"navy\"", "name = \"ocean\"")).unwrap();
    assert_eq!(import_file(&src, dir.path(), false), Ok(("ocean".into(), false)));
    assert!(dir.path().join("ocean.toml").is_file());
    assert_eq!(import_file(&src, dir.path(), false), Err("exists".into()));
    assert_eq!(import_file(&src, dir.path(), true), Ok(("ocean".into(), true)));
    std::fs::write(&src, "nope").unwrap();
    assert!(import_file(&src, dir.path(), true).unwrap_err().len() > 0);
    assert!(import_file(&src_dir.path().join("x.txt"), dir.path(), true).is_err(), "only .toml");
}

// settings.rs
#[test]
fn the_text_section_reads_the_presets_and_filters_fixed_pitch_families() {
    let cfg: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
    let (names, chosen, size, lh) = preset_rows(&cfg.font);
    assert_eq!(names, vec!["comfortable", "compact", "relaxed"]);
    assert_eq!((chosen.as_str(), size, lh), ("comfortable", 16.0, 1.5));
    let fams = monospace_families(&["Helvetica".into(), "SF Mono".into(), "Menlo".into(), "Fira Code".into(), "JetBrains Mono".into(), "Menlo".into()]);
    assert_eq!(fams, vec!["Fira Code", "JetBrains Mono", "Menlo", "SF Mono"]);
}
```

- [ ] **Step 2: Run to see them fail**

- [ ] **Step 3: Implement**

The window per spec section 6: a left column of section names (`tab_inactive` background for the chosen one), the right column with the chosen section's controls built from `self.config`: segmented choices as rows of pill buttons (the About/jump pill style), pickers as vertical lists of rows with a check mark on the current one (a dropdown is out of reach without a popover element; a list is fine at this size), steppers as "−" and "+" buttons around the value, switches as a pill reading "On" or "Off". Every control calls `on_change(keys, value, cx)`, which Root implements as `config::set_value(&paths::config_file(), keys, value)` followed by `reload_config` on every project window (the chooser app's version reloads nothing). "Add theme…": `cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false })`, then `theme::import_file(&path, &paths::themes_dir(), false)`; on `Err("exists")` show the confirm line with "Replace" (calls with `replace = true`) and "Keep"; on another error show it under the button; on success refresh `themes` and set the current mode's theme key. "Show themes folder": `cx.reveal_path(&paths::themes_dir())`. Start at login: `on_start_at_login(on, cx)` runs Root's `set_start_at_login(on, true, cx)`. Updates: the switch writes `["update", "auto"]`; the "Update now" row calls `on_update`. Keyboard: the sentence and an "Open config" button (`cx.open_with_system(&paths::config_file())`). `reload_config` on Root pushes the new config into the open window (`settings.update(cx, |w, cx| w.set_config(cfg, cx))`). Escape closes the window. Fonts: `monospace_families(&cx.text_system().all_font_names())`, computed once at open.

- [ ] **Step 4: Run, check, commit**

```bash
git add crates/arkos/src/settings.rs crates/arkos/src/main.rs crates/arkos/src/config.rs crates/arkos/src/theme.rs
git commit -m "The Settings window: appearance, themes with import, text, startup, updates"
```

---

### Task 5: README, Status rows, and the gates

**Files:**
- Modify: `README.md`

- [ ] **Step 1: README**

A "Menu" paragraph after the Status section's intro (the one menu and its five items); an "About" section (the rows, Copy details, the attribution and donation line, that the donation line appears once a URL is set); a "Settings" section (the five sections, that every change writes `config.toml` keeping comments and applies live, "Add theme…" and the themes folder, that keybindings stay in the file, `arkos theme list` and `validate`); the Timo Purple line in the themes paragraph (README ~469); the keybinding table rows ("settings" Cmd+, ; "open config" now unbound; "about"); Status rows: the menu's five items in order; About's rows match `arkos --version` and the paths exist; Copy details; the donation line hidden while the URL is empty; Settings switches the theme live and `config.toml` keeps its comments; "Add theme…" with a bad file shows the error and copies nothing, with a same-named file asks before replacing; `arkos theme list` shows the three built-ins; Timo Purple matches the screenshot's purple, cyan and green at the offsite demo.

- [ ] **Step 2: Gates**

`grep -rn "Used from Task" crates/arkos/src` prints nothing; no em-dash in README.md, docs/themes.md or `crates/arkos/src/*.rs`; the warning check and the suite pass.

```bash
git add README.md
git commit -m "README: the menu, About, Settings, Timo Purple"
```

---

## Deferred, for the user to rule on

- A keybinding editor in Settings; a theme editor; a theme generator from accent and background (the agent interface of 8b may cover it); per-workspace settings.
- A dropdown element for the pickers (lists are used instead); font previews.
- "Copy details" including the update journal.

## Self-review notes

- Spec section 3: Task 3. Section 4: Tasks 1 (build date), 3. Section 5: Tasks 1, 3. Section 6: Task 4. Section 7: Task 1. Section 8: Tasks 2, 4. Section 9: Task 5. Section 10 tests: as named; the windows are hand checks.
- The chooser app and the project app both register the About and Settings actions, so the menu works in every state Arkos can be in.
