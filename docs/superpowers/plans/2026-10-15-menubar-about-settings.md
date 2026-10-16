# Menubar, About, Settings and Timo Purple (milestone 8a) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One "Arkos IDE" menu (About, New Window, Settings, Quit), an About window with the build, update, install and path details and the attribution, a Settings window that writes `config.toml` in place and applies live, TOML theme import with `arkos theme list` and `validate`, and the Timo Purple theme built in.

**Architecture:** Pure first: `config::set_value` on `toml_edit` keeps the file's comments; `about::Details` is one struct the window and the clipboard both render; `startup::installed_since` writes the first-installed marker; the theme schema is documented and Timo Purple is a built-in file with a contrast test; `arkos theme` is a headless subcommand. Then two gpui windows (`about.rs`, `settings.rs`) opened through one registry (`aux.rs`, a gpui global holding the window handles and a weak Root) from global actions registered once in `main()`, so the menu works in every state, and the one menu.

**Tech Stack:** Rust 2021, gpui 0.2.2 (`Menu`, `MenuItem::separator`, `open_window` with `WindowOptions { is_resizable, window_min_size }`, `prompt_for_paths`, `reveal_path`, `text_system().all_font_names()`), `toml_edit` 0.22 (new dependency), `objc2-foundation` `NSProcessInfo` (already used by app_nap.rs). No other new dependency.

**Spec:** `docs/superpowers/specs/2026-10-15-menubar-about-settings-design.md` (approved 2026-10-15; plan reviewed on 2026-10-15 and amended, the rulings in the ledger; its section 1 rulings are binding: the one menu in that order, the attribution and donation lines, first installed go-forward, TOML import only, Timo Purple from Timo's three Codex values and the screenshot).

## Global Constraints

- Machine rules: every cargo call is `nice -n 15 ~/.cargo/bin/cargo ...`; the worktree's `.cargo/config.toml` (`jobs = 3`, shared `target-dir`) stays; no `--release`; never two cargo processes at once; focused filters while iterating, the workspace suite once per task; `cargo check --workspace --all-targets` must print no warning from our crates, in the test build too. Two daemon tests (`concurrent_spawns_give_each_shell_only_its_own_terminal`, `the_grace_counts_from_the_relaunch_so_a_slow_relaunch_keeps_the_old_bundle`) are known timing flakes under load: rerun alone and report both results.
- Dead code between tasks: an item a later task first uses, reached by a test here, gets `#[cfg_attr(not(test), allow(dead_code))] // Used from Task N on.`; unreached items get `#[allow(dead_code)] // Used from Task N on.`; the task that first uses it removes the attribute; after Task 5 `grep -rn "Used from Task" crates/arkos/src` prints nothing.
- Menu (exact labels, in order): "About Arkos IDE", separator, "New Window", "Settings", "Quit"; one menu named "Arkos IDE"; the File menu removed. Keymap: "about" (no key), "settings" (`cmd-,`); "open config" keeps its palette entry and loses its default key. The `ACTIONS` rows for "about" and "settings" go immediately BEFORE "open config", because gpui ranks bindings at the same depth later-first: a user's `"open config" = "cmd-,"` then wins over Settings. Palette: an explicit label arm "About Arkos IDE"; "Settings" comes from `keymap::label`; the Settings detail "Cmd+,".
- Global routing: `aux.rs` holds a `gpui::Global` `Aux { about: Option<WindowHandle<AboutWindow>>, settings: Option<WindowHandle<SettingsWindow>>, root: Option<WeakEntity<Root>>, launch: LaunchState }` where `LaunchState` carries the config, the theme, the config path and `installed` loaded in `main()`. `cx.on_action` for `About` and `OpenSettings` is registered once in `main()` and calls `aux::open_about(cx)` / `aux::open_settings(cx)`, which activate an open window or open one, taking details, config and callbacks from the Root when `aux.root` upgrades and from `launch` before that. Root's `run` arms call the same functions. About and Settings handle `NewWindow` by forwarding to the Root's window when there is one. `after_window_closed(windows_left, project_open, aux_open)` closes the chooser, About and Settings when the project window is gone, so the quit follows; before a project window exists, About and Settings do not count toward `windows_left` in `startup::quits_on_close`.
- Chooser-state edits: `open_project_window` reloads `config::load(&config_path)` and re-resolves the theme when the file's `config_watch::fingerprint` differs from the one `main()` took, so a Settings change made from the chooser is not lost.
- About window: title "About Arkos IDE", 420 by 520 px, not resizable, centred, single instance, a `FocusHandle` tracked so keys reach it; Escape and `CloseTab` (Cmd+W) close it. Rows (labels exact, in this order): "Version", "Build" ("{hash}, {date}"), "Last updated" ("unknown" when nothing is known), "First installed", "Daemon" ("{version}, pid {pid}", or "{version}" when the pid is unknown, or "not connected"), "Protocol", "macOS" ("{major}.{minor}.{patch} {arch}" from `NSProcessInfo::operatingSystemVersion`), "Config", "Themes", "App Support", "Window log", "Daemon log". Dates render "today HH:MM" (local) for the current day, else "YYYY-MM-DD". "Copy details" copies `Details::clipboard_text()`: "Arkos IDE {version}" then one "label: value" line per row. Attribution: "Made by Cyrus Sarkosh" (the name links to https://github.com/csarkosh); under it "Buy me more tokens" followed by "✦" in `#d97757` and "❤" in `#e5484d`, the whole line opening `about::DONATE_URL`, drawn only when `donation_visible(DONATE_URL)` (the constant ships empty).
- "Last updated": `updates/applied.json`'s `at_unix` when the file is present, else the running executable's creation time (`metadata(exe).created()`, the moment this copy was put in place), else its modification time, else unknown; `exe` is `update::launch_path()`. The daemon pid: `Client::peer_pid()` read once at connect with `getsockopt(SOL_LOCAL, LOCAL_PEERPID)`; `None` when the call fails.
- Settings window: title "Settings", 560 by 480 px, resizable, minimum 480 by 360, single instance, a `FocusHandle`; Escape and Cmd+W close it; sections in order "Appearance", "Text", "Startup", "Updates", "Keyboard"; the control texts are in Task 4. Every change goes through one write (`config::set_values`) and then this process's `Root::reload_config` at once (other Arkos windows are other processes and follow through their config watcher within a second); in the chooser state the write is the whole change. While the config fails to load (`config::load`'s error, or a `set_values` error), the window shows that exact text in a banner at the top and every control is drawn dimmed with no click handler.
- `config::set_values(path, edits: &[(&[&str], toml_edit::Value)]) -> Result<(), String>` (`set_value` is the one-pair case): comments, order, formatting and a line's trailing comment kept (the old value's decor copied onto the new value); the walk uses `as_table_like_mut`, so inline tables and dotted keys work, and a path segment that is not a table is an error; a key added at the end of its existing table; a missing table appended at the end of the file; the path is canonicalised and the temp file written beside the resolved target, then renamed (a symlinked config stays a symlink); an unparseable file is refused with its parse error and left as it was. Settings writes a preset's `size` and `line_height` together (`TextPreset` requires both), and steppers compute in f64 rounded to 0.1 so no `1.600000023841858` reaches the file.
- First installed: `<app support>/installed` holds the unix seconds of the first run as text; `startup::installed_since(file, now) -> SystemTime` reads or writes it; garbage counts as missing. `Root.installed: SystemTime` and `LaunchState.installed` carry it.
- Build date: `ARKOS_BUILD_DATE` is `SOURCE_DATE_EPOCH` as "YYYY-MM-DD" when set, else `git log -1 --format=%cs` (the commit date, which matches the hash), else "unknown"; `build.rs` adds `cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH`.
- Themes: `docs/themes.md` documents every table and key; `timo-purple.toml` (name `timo-purple`) joins `theme::BUILTIN` as the third entry with the exact values in Task 2; the existing general contrast test (`every_foreground_clears_4_5_to_1_and_the_terminal_text_12_to_1`, which covers every built-in) must pass, plus the surface pairs Task 2 adds. `arkos theme list` prints one line per theme, "{name} (built in)", "{name} (file)" or "{name} (file, invalid: {error})", built-ins first then the folder's files sorted, a file shadowing a built-in listed once as "(file)"; `arkos theme validate <file>` prints "ok: {name}" and exits 0, or the error and exits 1; anything else prints usage and exits 2. `theme::import_file(src, dir, replace)` accepts only `.toml`, names the copy after the file's `name`, rejects a name that is not a bare stem (`is_bare_name`) or starts with ".", creates `dir`, and returns `Err("exists")` when a same-named file is there and `replace` is false.
- Import in the Settings window: "Add theme…" opens `prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Add theme".into()) })` and awaits it in `cx.spawn` with a `picking` guard, as `open_folder` does; on `Err("exists")` a confirm line "Replace the existing {name}?" with "Replace" and "Keep"; on another error the message under the button; on success the theme is written to `[theme] dark` or `[theme] light` by `theme::is_dark(window.appearance())` when the mode is System, else by the mode. "Show themes folder" runs `create_dir_all` then `reveal_path`.
- Start at login in Settings writes `[recovery] start_at_login` only; `reload_config` then applies it through `apply_recovery` (one install path). The row reads "set by config to ask" with the switch off while the value is `ask`.
- No em-dashes in any file. Comments are full sentences. Headless tests only: no test opens a window, a file picker, Finder, or touches real user files.

## Review Focus

1. A `config.toml` with no `[font.presets.comfortable]` table: the size stepper writes both `size` and `line_height` in one write and the file still parses (Task 1 `set_values_writes_a_preset_whole`).
2. "Copy details" when the daemon is not connected and the journal is absent: every row has a value, no "None" leaks, the rows are in the stated order (Task 3b `details_render_with_nothing_known`).
3. A theme file whose `name` is `../../x`: import refuses it and writes nothing (Task 2 `import_uses_the_theme_s_own_name_and_asks_before_replacing`).
4. Cmd+, with a config that binds "open config" to `cmd-,`: the config file opens, not Settings (Task 3a `a_user_s_cmd_comma_override_beats_settings`).
5. About open, the project window closed with the red button: the process quits rather than lingering with About alone (Task 3a `after_window_closed` tests).

## File structure

- Create `crates/arkos/src/aux.rs` (the registry and the two openers), `crates/arkos/src/about.rs`, `crates/arkos/src/settings.rs`, `crates/arkos/themes/timo-purple.toml`, `docs/themes.md`.
- Modify `crates/arkos/src/config.rs` (`set_values`, `set_value`), `startup.rs` (`installed_since`, `quits_on_close`), `paths.rs` (`installed_file`), `build.rs`, `theme.rs` (`BUILTIN`, `validate_file`, `list`, `import_file`, `theme_cli_to`, `is_bare_name` pub(crate)), `client.rs` (`peer_pid`), `main.rs` (`menus`, actions, the `theme` subcommand, `after_window_closed`, `Root.installed`, the chooser-state reload, `build_date`, `open_config`'s doc), `keymap.rs`, `command_palette.rs`, `README.md`, `Cargo.toml` and `crates/arkos/Cargo.toml` (`toml_edit`).

---

### Task 1: Config edits that keep comments, the install marker, the build date

**Files:**
- Modify: `Cargo.toml` (workspace dependency `toml_edit = "0.22"`), `crates/arkos/Cargo.toml` (`toml_edit.workspace = true`), `Cargo.lock`
- Modify: `crates/arkos/src/config.rs` (add `set_values` and `set_value` near `write_default` at ~328; tests at the end), `crates/arkos/src/startup.rs` (`installed_since`; tests in its module), `crates/arkos/src/paths.rs` (`installed_file`; one test), `crates/arkos/build.rs`, `crates/arkos/src/main.rs` (`pub fn build_date() -> &'static str { env!("ARKOS_BUILD_DATE") }` next to `version_string` at ~3490)

**Interfaces:**
- Produces (attributes: `set_values`/`set_value` "Used from Task 4 on."; `installed_since`, `installed_file`, `build_date` "Used from Task 3b on."):
  ```rust
  pub fn set_values(path: &Path, edits: &[(&[&str], toml_edit::Value)]) -> Result<(), String>;
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
    std::fs::write(&path, "# top comment\n[font]\n# the family\nfamily = \"Menlo\" # trailing\npreset = \"comfortable\"\n\n[font.presets.comfortable]\nsize = 16.0\nline_height = 1.5\n\n[update]\nauto = true\n").unwrap();
    set_value(&path, &["font", "presets", "comfortable", "size"], toml_edit::Value::from(18.0)).unwrap();
    set_value(&path, &["appearance", "mode"], toml_edit::Value::from("dark")).unwrap();
    set_value(&path, &["font", "family"], toml_edit::Value::from("SF Mono")).unwrap();
    set_value(&path, &["update", "idle_after_secs"], toml_edit::Value::from(30)).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# top comment\n[font]\n# the family\nfamily = \"SF Mono\" # trailing\n"), "comments and a trailing comment survive: {text}");
    assert!(text.contains("size = 18.0\nline_height = 1.5\n"), "{text}");
    assert!(text.contains("[update]\nauto = true\nidle_after_secs = 30\n"), "a key lands at the end of its table: {text}");
    assert!(text.ends_with("[appearance]\nmode = \"dark\"\n"), "a missing table is appended: {text}");
    let parsed: Config = toml::from_str(&text).unwrap();
    assert_eq!(parsed.font.family, "SF Mono");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "no temp file left");
}

#[test]
fn set_values_writes_a_preset_whole_and_refuses_a_bad_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[font]\npreset = \"big\"\n").unwrap();
    set_values(&path, &[(&["font", "presets", "big", "size"], toml_edit::Value::from(20.0)), (&["font", "presets", "big", "line_height"], toml_edit::Value::from(1.6))]).unwrap();
    let parsed: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(parsed.font.presets["big"].size, 20.0);
    assert!(set_values(&path, &[(&["font", "preset", "x"], toml_edit::Value::from(1))]).is_err(), "a value is not a table");
}

#[test]
fn set_value_refuses_a_file_it_cannot_parse_follows_a_symlink_and_round_trips_the_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[font\nfamily = 1\n").unwrap();
    let err = set_value(&path, &["font", "family"], toml_edit::Value::from("x")).unwrap_err();
    assert!(!err.is_empty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[font\nfamily = 1\n", "left as it was");
    std::fs::write(&path, DEFAULT_CONFIG).unwrap();
    let link = dir.path().join("link.toml");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    set_value(&link, &["update", "auto"], toml_edit::Value::from(false)).unwrap();
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link is still a link");
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

And in `paths.rs` tests: `assert_eq!(installed_file(), app_support().join("installed"))`.

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- config::tests::set_value startup:: paths::`

- [ ] **Step 3: Implement**

`set_values`: `let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())`; read; `parse::<toml_edit::DocumentMut>()` (the error's first line); for each edit walk `keys[..n-1]` with `as_table_like_mut()` creating missing tables (`toml_edit::table()`, `set_implicit(false)` on the leaf's parent chain so headers print) and erroring on a non-table segment; at the leaf, when a value exists copy its `decor()` onto the new value, then assign `toml_edit::value(v)`; write `<target>.<pid>.tmp` beside `target` and rename. `set_value` wraps one pair. `installed_since` as the constraint says. `build.rs`: `SOURCE_DATE_EPOCH` (days since epoch to a civil date with the standard algorithm) else `git log -1 --format=%cs` else "unknown"; `rerun-if-env-changed=SOURCE_DATE_EPOCH`.

- [ ] **Step 4: Run, check, commit**

```bash
git add Cargo.toml Cargo.lock crates/arkos/Cargo.toml crates/arkos/src/config.rs crates/arkos/src/startup.rs crates/arkos/src/paths.rs crates/arkos/build.rs crates/arkos/src/main.rs
git commit -m "Config edits keep comments; an install marker; the build date"
```

---

### Task 2: The theme schema, Timo Purple, import, and `arkos theme`

**Files:**
- Create: `docs/themes.md`, `crates/arkos/themes/timo-purple.toml`
- Modify: `crates/arkos/src/theme.rs` (`BUILTIN` at 104 becomes three entries; `is_bare_name` becomes `pub(crate)`; add `validate_file`, `list`, `import_file`, `theme_cli_to`, `theme_cli`; existing tests: `extract_writes_builtins_without_overwriting` expects 3, `user_files_win_and_names_are_bare_stems` adds "timo-purple"; the general contrast test runs over the new built-in), `crates/arkos/src/main.rs` (the `theme` subcommand next to `doctor` at ~3501), `crates/arkos/src/config.rs` (DEFAULT_CONFIG's `[theme]` comment names timo-purple; `the_default_file_...` tests still pass), `crates/arkos/themes/navy.toml` and `navy-light.toml` (a one-line pointer to `docs/themes.md` in their header comment)

**Interfaces:**
- Produces (attributes "Used from Task 4 on." on `import_file`, `list`; `validate_file` and `theme_cli` are used by `main()` here):
  ```rust
  pub const BUILTIN: [(&str, &str); 3];
  pub(crate) fn is_bare_name(name: &str) -> bool;
  pub fn validate_file(path: &Path) -> Result<String, String>; // Ok(name) or the error; a missing file is an error
  #[derive(Debug, Clone, PartialEq, Eq)] pub enum Source { BuiltIn, File, Invalid(String) }
  pub fn list(dir: &Path) -> Vec<(String, Source)>;
  pub fn import_file(src: &Path, dir: &Path, replace: bool) -> Result<(String, bool), String>; // Ok((name, existed))
  pub fn theme_cli_to(args: &[String], dir: &Path, out: &mut dyn std::io::Write, err: &mut dyn std::io::Write) -> i32;
  pub fn theme_cli(args: &[String]) -> i32; // theme_cli_to with paths::themes_dir() and the real stdout and stderr
  ```

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn timo_purple_is_built_in_and_reads_on_the_surfaces_the_general_test_skips() {
    let t = builtin("timo-purple").expect("ships");
    assert_eq!(t.name, "timo-purple");
    assert_eq!(BUILTIN.len(), 3);
    let c = &t.chrome;
    // The general contrast test covers every foreground on window_bg; these are the other surfaces.
    let pairs = [(c.text_dim, c.banner), (c.text_dim, c.sidebar), (c.text_dim, c.tab_inactive), (c.text, c.tab_inactive), (t.terminal.foreground, t.terminal.selection), (t.terminal.foreground, t.semantic.find_match), (t.terminal.foreground, t.semantic.find_current)];
    for (fg, bg) in pairs {
        assert!(crate::palette::contrast(fg, bg) >= 4.5, "{fg:?} on {bg:?} is {:.2}", crate::palette::contrast(fg, bg));
    }
    assert_eq!(c.focus_ring, t.terminal.cyan, "the accent is the cyan");
    assert_eq!(t.terminal.cursor, t.terminal.cyan);
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
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(theme_cli_to(&["list".into()], dir.path(), &mut out, &mut err), 0);
    let lines: Vec<&str> = std::str::from_utf8(&out).unwrap().lines().collect();
    assert_eq!(lines[0], "navy (file)");
    assert_eq!(lines[1], "navy-light (built in)");
    assert_eq!(lines[2], "timo-purple (built in)");
    assert!(lines[3].starts_with("broken (file, invalid: "));
    assert_eq!(lines[4], "mine (file)");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(theme_cli_to(&["validate".into(), dir.path().join("mine.toml").display().to_string()], dir.path(), &mut out, &mut err), 0);
    assert_eq!(std::str::from_utf8(&out).unwrap(), "ok: mine\n");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(theme_cli_to(&["validate".into(), dir.path().join("broken.toml").display().to_string()], dir.path(), &mut out, &mut err), 1);
    assert!(!err.is_empty());
    assert_eq!(theme_cli_to(&["frob".into()], dir.path(), &mut Vec::new(), &mut Vec::new()), 2);
}

#[test]
fn import_uses_the_theme_s_own_name_and_asks_before_replacing() {
    let src_dir = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap().path().join("themes"); // created on demand
    let src = src_dir.path().join("whatever.toml");
    std::fs::write(&src, BUILTIN[0].1.replace("name = \"navy\"", "name = \"ocean\"")).unwrap();
    assert_eq!(import_file(&src, &dir, false), Ok(("ocean".into(), false)));
    assert!(dir.join("ocean.toml").is_file());
    assert_eq!(import_file(&src, &dir, false), Err("exists".into()));
    assert_eq!(import_file(&src, &dir, true), Ok(("ocean".into(), true)));
    std::fs::write(&src, BUILTIN[0].1.replace("name = \"navy\"", "name = \"../../x\"")).unwrap();
    assert!(import_file(&src, &dir, true).is_err(), "a name that leaves the folder");
    assert!(!dir.parent().unwrap().join("x.toml").exists());
    std::fs::write(&src, BUILTIN[0].1.replace("name = \"navy\"", "name = \".hidden\"")).unwrap();
    assert!(import_file(&src, &dir, true).is_err());
    std::fs::write(&src, "nope").unwrap();
    assert!(!import_file(&src, &dir, true).unwrap_err().is_empty());
    assert!(import_file(&src_dir.path().join("x.txt"), &dir, true).is_err(), "only .toml");
}
```

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- theme::`

- [ ] **Step 3: Implement**

`timo-purple.toml`, the exact values (every one estimated at or above 4.5:1 on the surface it sits on; the general test is the gate):

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
selection = "#1a6b45"
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
working = "#5fe39a"
waiting = "#ffd166"
bell = "#ff8a9e"
error_banner = "#5a1a3a"
find_match = "#15553a"
find_current = "#1f7a52"
[tree]
added = "#8fd9a8"
modified = "#ffd166"
deleted = "#ff8a9e"
untracked = "#c9b8e8"
ignored = "#a08ed6"
[viewer]
keyword = "#e39bff"
string = "#8fd9a8"
comment = "#a08ed6"
number = "#ffd166"
function = "#8fb8ff"
type_name = "#7fd4d4"
constant = "#ffb3c0"
operator = "#e9e2ff"
punctuation = "#c4b6ea"
```

(Use exactly the `[tree]` and `[viewer]` keys navy.toml has; if navy has more or fewer, match navy's key set and keep these hues for the ones named. If the general contrast test still flags a pair, lighten that foreground toward white until it passes and record the change in the report.)

`docs/themes.md`: one section per table listing each key, what it paints and which surface it must contrast with; the 4.5:1 rule and the 11:1 terminal text rule the general test enforces; how to add a theme (a file in the themes folder, `arkos theme validate`, then Settings or `[theme]`). `list`, `validate_file`, `import_file` (`.toml` only, parse, `is_bare_name` and no leading ".", `create_dir_all`, `Err("exists")` unless `replace`), `theme_cli_to` printing the constraint's lines (usage to `err`, exit 2); `theme_cli` with the real paths and streams; wired in `main()`: `if args.get(1).map(String::as_str) == Some("theme") { std::process::exit(theme::theme_cli(&args[2..])); }`.

- [ ] **Step 4: Run, check, commit**

Run the theme and config tests, the warning check, the suite.

```bash
git add docs/themes.md crates/arkos/themes crates/arkos/src/theme.rs crates/arkos/src/main.rs crates/arkos/src/config.rs
git commit -m "Timo Purple ships; the theme schema is documented; arkos theme list and validate"
```

---

### Task 3a: The menu, the actions, the registry, and the close rules

**Files:**
- Create: `crates/arkos/src/aux.rs`
- Modify: `crates/arkos/src/keymap.rs` (`actions!`: `About`, `OpenSettings`; `Command::{About, OpenSettings}` with doc comments; `ACTIONS`: `row("about", "")` and `row("settings", "cmd-,")` inserted immediately before `row("open config", "")` (which loses its key) at ~141; `binding()`, `command_for`, `route!`, SPEC_ROWS (`("", "about", "About")`, `("cmd-,", "settings", "OpenSettings")`, `("", "open config", "OpenConfig")`); the count assertions at ~603 and ~612 go from `- 12`/`- 11` to `- 14`/`- 13` with "about, open config" added to their messages), `crates/arkos/src/command_palette.rs` (label arm `Command::About => "About Arkos IDE"`; tests find "About Arkos IDE" and "Settings" with detail "Cmd+,"), `crates/arkos/src/config.rs` (DEFAULT_CONFIG keybinding comments: `# "about" = ""`, `# "settings" = "cmd-,"`, `# "open config" = ""`), `crates/arkos/src/main.rs` (`menus()` at ~3462; `after_window_closed` at 448 gains `aux_open: bool` and returns `CloseAux` (closing the chooser, About and Settings) when the project window is gone; `Root.installed: SystemTime`; `open_project_window` sets `aux::set_root(cx, cx.weak_entity())`, stores `installed`, and reloads the config when its fingerprint changed since `main()`; the two `on_action(Quit)` sites get, once in `main()` only, `cx.on_action(|_: &About, cx| aux::open_about(cx))` and the same for `OpenSettings`; `run` arms `About => aux::open_about(cx)`, `OpenSettings => aux::open_settings(cx)`; `open_config`'s doc at ~1470 no longer says Cmd+,), `crates/arkos/src/startup.rs` (`quits_on_close` ignores aux windows: the caller passes `windows_left` minus the open aux windows; add a test)

**Interfaces:**
- Produces:
  ```rust
  // aux.rs
  pub struct LaunchState { pub config: Config, pub theme: Theme, pub config_path: PathBuf, pub installed: SystemTime, pub fingerprint: Option<(u128, u64)> }
  #[derive(Default)] pub struct Aux { pub about: Option<WindowHandle<about::AboutWindow>>, pub settings: Option<WindowHandle<settings::SettingsWindow>>, pub root: Option<WeakEntity<Root>>, pub launch: Option<LaunchState> }
  impl gpui::Global for Aux {}
  pub fn init(cx: &mut App, launch: LaunchState);
  pub fn set_root(cx: &mut App, root: WeakEntity<Root>);
  pub fn open_about(cx: &mut App);   // activates or opens; in this task opens nothing yet but shows nothing either: it calls `about::open` from Task 3b, so this task ships it as a stub that only logs (replaced in 3b)
  pub fn open_settings(cx: &mut App); // a stub that shows a notice on the Root ("Settings arrive in the next task") until Task 4
  pub fn aux_open(cx: &App) -> usize; // open About and Settings windows
  pub fn close_all(cx: &mut App);
  pub fn forward_new_window(cx: &mut App); // dispatches NewWindow to the Root's window, or opens the chooser through the launch state's path
  ```
  `after_window_closed(windows_left, project_open, aux_open) -> AfterClose::{Quit, CloseAux, Stay}` where `CloseAux` closes the chooser and the aux windows and then quits when nothing is left.

- [ ] **Step 1: Failing tests**

```rust
// keymap.rs
#[test]
fn a_user_s_cmd_comma_override_beats_settings() {
    use gpui::{KeyContext, Keymap, Keystroke};
    let ctx = [KeyContext::parse("Root").unwrap(), KeyContext::parse("Terminal").unwrap()];
    let k = Keystroke::parse("cmd-,").unwrap();
    let keymap = Keymap::new(bindings_for(&BTreeMap::new()));
    let (matched, _) = keymap.bindings_for_input(&[k.clone()], &ctx);
    assert_eq!(matched[0].action().name(), "arkos::OpenSettings", "by default Cmd+, opens Settings");
    let mut o = BTreeMap::new();
    o.insert("open config".to_string(), "cmd-,".to_string());
    let keymap = Keymap::new(bindings_for(&o));
    let (matched, _) = keymap.bindings_for_input(&[k], &ctx);
    assert_eq!(matched[0].action().name(), "arkos::OpenConfig", "a user's override comes later in the list and wins");
    assert_eq!(keys_for("open config", &BTreeMap::new()), None);
    assert_eq!(keys_for("settings", &BTreeMap::new()), Some("cmd-,".to_string()));
    assert_eq!(command_for("about"), Some(Command::About));
}

// main.rs tests
#[test]
fn an_aux_window_left_alone_does_not_keep_the_app_alive() {
    assert_eq!(after_window_closed(0, false, false), AfterClose::Quit);
    assert_eq!(after_window_closed(1, false, true), AfterClose::CloseAux, "About stayed after the project window closed");
    assert_eq!(after_window_closed(2, false, true), AfterClose::CloseAux, "the chooser and About both");
    assert_eq!(after_window_closed(1, true, true), AfterClose::Stay, "a project window remains");
}
```

And in `startup.rs`: `quits_on_close(false, false, 0, false, false)` after subtracting one aux window from a `windows_left` of 1 (the caller's arithmetic; add a tiny `windows_without_aux(left, aux_open) -> usize` with `saturating_sub` and test it).

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- keymap:: main::tests:: startup:: command_palette::`

- [ ] **Step 3: Implement**

The keymap, palette and DEFAULT_CONFIG edits as listed. `menus()`: `vec![Menu { name: "Arkos IDE".into(), items: vec![MenuItem::action("About Arkos IDE", keymap::About), MenuItem::separator(), MenuItem::action("New Window", keymap::NewWindow), MenuItem::action("Settings", keymap::OpenSettings), MenuItem::action("Quit", Quit)] }]`. `aux.rs` with the global; `aux::init` in `main()` right after the config and theme load with the fingerprint; `aux::set_root` in `open_project_window` after the Root exists; the chooser-state reload: in `open_project_window`, `if config_watch::fingerprint(&config_path) != launch.fingerprint { reload config and theme into Startup before building Root }`. `after_window_closed` and its call site (`on_window_closed` ~3894): on `CloseAux`, `aux::close_all(cx)` then the chooser close as today; `aux_open` counts the two handles whose `update` succeeds. The stubs for `open_about`/`open_settings` as the interface says.

- [ ] **Step 4: Run, check, commit**

```bash
git add crates/arkos/src/aux.rs crates/arkos/src/main.rs crates/arkos/src/keymap.rs crates/arkos/src/command_palette.rs crates/arkos/src/config.rs crates/arkos/src/startup.rs
git commit -m "One menu, two actions, a window registry, and the quit rules for About and Settings"
```

---

### Task 3b: The About window

**Files:**
- Create: `crates/arkos/src/about.rs`
- Modify: `crates/arkos/src/aux.rs` (`open_about` opens or activates), `crates/arkos/src/client.rs` (`peer_pid`), `crates/arkos/src/main.rs` (`Root::details(&self) -> about::Details`; `mod about;`), `crates/arkos/src/startup.rs`, `paths.rs` (remove the "Used from Task 3b" attributes)

**Interfaces:**
- Produces:
  ```rust
  pub const DONATE_URL: &str = "";
  pub const AUTHOR: &str = "Cyrus Sarkosh"; pub const AUTHOR_URL: &str = "https://github.com/csarkosh";
  pub fn donation_visible(url: &str) -> bool;
  #[derive(Debug, Clone, PartialEq, Eq)] pub struct Details { pub version: String, pub build_hash: String, pub build_date: String, pub last_updated: Option<SystemTime>, pub first_installed: SystemTime, pub daemon: Option<(String, Option<u32>)>, pub protocol: u32, pub macos: String, pub arch: &'static str, pub config: PathBuf, pub themes: PathBuf, pub app_support: PathBuf, pub window_log: PathBuf, pub daemon_log: PathBuf }
  impl Details { pub fn rows(&self, now: SystemTime) -> Vec<(&'static str, String)>; pub fn clipboard_text(&self, now: SystemTime) -> String; }
  pub fn format_when(t: SystemTime, now: SystemTime) -> String;
  pub fn last_updated(updates_dir: &Path, exe: Option<&Path>) -> Option<SystemTime>;
  pub fn macos_version() -> String; // "{major}.{minor}.{patch}"
  pub struct AboutWindow { details: Details, theme: Theme, focus: FocusHandle }
  pub fn open(cx: &mut App, details: Details, theme: Theme) -> Option<WindowHandle<AboutWindow>>;
  // client.rs
  impl Client { pub fn peer_pid(&self) -> Option<u32>; } // read once at connect with getsockopt(SOL_LOCAL, LOCAL_PEERPID)
  ```

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn details_render_with_nothing_known() {
    let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let now = t0 + Duration::from_secs(30 * 86_400);
    let d = Details { version: "0.3.1".into(), build_hash: "abc1234".into(), build_date: "2026-10-15".into(), last_updated: None, first_installed: t0, daemon: None, protocol: 5, macos: "26.1.0".into(), arch: "arm64", config: "/c/config.toml".into(), themes: "/c/themes".into(), app_support: "/a".into(), window_log: "/a/window.log".into(), daemon_log: "/a/arkosd.log".into() };
    let rows = d.rows(now);
    assert_eq!(rows.iter().map(|(k, _)| *k).collect::<Vec<_>>(), vec!["Version", "Build", "Last updated", "First installed", "Daemon", "Protocol", "macOS", "Config", "Themes", "App Support", "Window log", "Daemon log"]);
    let get = |l: &str| rows.iter().find(|(k, _)| *k == l).map(|(_, v)| v.clone()).unwrap();
    assert_eq!(get("Version"), "0.3.1");
    assert_eq!(get("Build"), "abc1234, 2026-10-15");
    assert_eq!(get("Last updated"), "unknown");
    let first = get("First installed");
    assert_eq!(first, format_when(t0, now));
    assert!(first.len() == 10 && first.as_bytes()[4] == b'-' && first.as_bytes()[7] == b'-', "{first}");
    assert_eq!(get("Daemon"), "not connected");
    assert_eq!(get("Protocol"), "5");
    assert_eq!(get("macOS"), "26.1.0 arm64");
    assert_eq!(get("Config"), "/c/config.toml");
    let text = d.clipboard_text(now);
    assert!(text.starts_with("Arkos IDE 0.3.1\n"));
    assert_eq!(text.lines().count(), rows.len() + 1);
    assert!(text.contains("\nDaemon: not connected\n"));
    assert!(!text.contains("None"));
    let with = Details { daemon: Some(("0.3.1".into(), Some(4242))), ..d.clone() };
    assert_eq!(with.rows(now).iter().find(|(k, _)| *k == "Daemon").unwrap().1, "0.3.1, pid 4242");
    let no_pid = Details { daemon: Some(("0.3.1".into(), None)), ..d };
    assert_eq!(no_pid.rows(now).iter().find(|(k, _)| *k == "Daemon").unwrap().1, "0.3.1");
}

#[test]
fn dates_say_today_or_the_day_and_the_donation_line_hides_without_a_url() {
    let now = SystemTime::now();
    assert!(format_when(now, now).starts_with("today "));
    let s = format_when(now - Duration::from_secs(3 * 86_400), now);
    assert!(s.len() == 10 && s.as_bytes()[4] == b'-');
    assert!(!donation_visible(""));
    assert!(donation_visible("https://example.org/tip"));
    assert!(DONATE_URL.is_empty(), "ships hidden until Cyrus sets it");
}

#[test]
fn last_updated_prefers_the_journal_then_the_executable() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("arkos");
    std::fs::write(&exe, "x").unwrap();
    assert!(last_updated(dir.path(), Some(&exe)).is_some(), "the executable's creation or modification time");
    assert_eq!(last_updated(dir.path(), None), None);
    std::fs::write(dir.path().join("applied.json"), r#"{"version":"0.3.0","at_unix":1800000000}"#).unwrap();
    assert_eq!(last_updated(dir.path(), Some(&exe)), Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)));
}
```

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- about::`

- [ ] **Step 3: Implement**

`Details::rows` in the constraint's order; `format_when` through `libc::localtime_r` as `notices::relative_time` does; `last_updated` reads `applied.json` (`at_unix`) else `metadata(exe).created().or_else(modified)`; `macos_version` through `NSProcessInfo::processInfo().operatingSystemVersion()` (fields `majorVersion`, `minorVersion`, `patchVersion`); `arch` is `std::env::consts::ARCH`. `Client::peer_pid` with `libc::getsockopt(fd, libc::SOL_LOCAL, libc::LOCAL_PEERPID, ..)` at connect. The window per the constraint: `open_window` with `is_resizable: false`, `focus: true`; a view with a tracked `FocusHandle` (`window.focus(&view.focus)` at open), rows as a two-column table, path rows clickable (`cx.reveal_path`), "Copy details" in the jump-pill style, the attribution block, `on_key_down` Escape and `.on_action(|_: &keymap::CloseTab, window, _| window.remove_window())` and `.on_action` for `NewWindow` calling `aux::forward_new_window`. `Root::details()` fills from `version_string` parts, `build_date()`, `last_updated(&paths::app_support().join("updates"), update::launch_path().as_deref())`, `self.installed`, `self.daemon_version.clone().map(|v| (v, self.sessions.client.as_ref().and_then(|c| c.peer_pid())))`, `PROTOCOL_VERSION`, `macos_version()`, the paths. `aux::open_about`: from the Root when it upgrades, else from `LaunchState` with `daemon: None`.

- [ ] **Step 4: Run, check, commit**

```bash
git add crates/arkos/src/about.rs crates/arkos/src/aux.rs crates/arkos/src/client.rs crates/arkos/src/main.rs crates/arkos/src/startup.rs crates/arkos/src/paths.rs
git commit -m "The About window: build, update and install details, and the attribution"
```

---

### Task 4: The Settings window

**Files:**
- Create: `crates/arkos/src/settings.rs`
- Modify: `crates/arkos/src/aux.rs` (`open_settings` opens or activates; the stub goes), `crates/arkos/src/main.rs` (`reload_config` pushes the new config, error and `update_ready` into an open Settings window through `aux`; `open_folder`'s picker pattern at ~2529 is the model), `crates/arkos/src/config.rs`, `theme.rs` (remove the "Used from Task 4" attributes)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Section { Appearance, Text, Startup, Updates, Keyboard }
  pub struct Shown { pub config: Config, pub error: Option<String>, pub update_ready: Option<String> }
  pub struct SettingsWindow { shown: Shown, section: Section, themes: Vec<(String, theme::Source)>, fonts: Vec<String>, import_error: Option<String>, pending_replace: Option<(PathBuf, String)>, picking: bool, focus: FocusHandle, theme: Theme, on_change: Rc<dyn Fn(&[(&[&str], toml_edit::Value)], &mut App)>, on_update: Rc<dyn Fn(&mut App)> }
  pub fn open(cx: &mut App, shown: Shown, theme: Theme, fonts: Vec<String>, on_change: Rc<..>, on_update: Rc<..>) -> Option<WindowHandle<SettingsWindow>>;
  impl SettingsWindow { pub fn set_shown(&mut self, shown: Shown, cx: &mut Context<Self>); }
  pub fn preset_rows(cfg: &FontConfig) -> (Vec<String>, String, f32, f32); // names sorted, the chosen one, its size and line height
  pub fn monospace_families(all: &[String]) -> Vec<String>; // the pure name filter, the fallback
  pub fn round_tenth(v: f64) -> f64;
  pub fn theme_key_for(mode: AppearanceMode, system_is_dark: bool) -> &'static str; // "dark" or "light"
  ```

Control texts (exact), by section:
- Appearance: "Mode" with segments "System", "Dark", "Light" (`["appearance", "mode"]` = "system" | "dark" | "light"); "Dark theme" and "Light theme", each a list of `theme::list(themes_dir)` plus the built-ins with the current one marked "✓" and an invalid file shown with "(invalid)" and not clickable (`["theme", "dark"]` / `["theme", "light"]`); "Add theme…"; "Show themes folder".
- Text: "Font family": a list of the monospace families, the current value first and marked "(not installed)" when it is not in the list (`["font", "family"]`); "Text preset": a list of the presets with the current marked (`["font", "preset"]`); "Size": a stepper 10 to 32 by 1 and "Line height": a stepper 1.2 to 2.0 by 0.1, both writing `["font", "presets", <name>, "size"]` and `[.., "line_height"]` together in one `set_values`; a value outside the range is shown as it is and clamped on the first step.
- Startup: "Start at login": a switch "On" / "Off" writing `["recovery", "start_at_login"]` = "on" | "off"; while the value is `ask` the row reads "set by config to ask" with the switch off.
- Updates: "Automatic updates": a switch writing `["update", "auto"]`; under it, when `update_ready` is Some, "{version} is ready" with a button "Update now" calling `on_update`.
- Keyboard: "Keybindings live in config.toml" with a button "Open config" (the palette's `open -t` behaviour through `on_change`'s sibling `on_open_config`, or `std::process::Command::new("open").arg("-t")` on the background executor).
- The banner at the top while `shown.error` is Some: that text, and every control dimmed (`text_dim`) with no click handler.

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn the_text_section_reads_the_presets_filters_fixed_pitch_families_and_rounds() {
    let cfg: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
    let (names, chosen, size, lh) = preset_rows(&cfg.font);
    assert_eq!(names, vec!["comfortable", "compact", "relaxed"]);
    assert_eq!((chosen.as_str(), size, lh), ("comfortable", 16.0, 1.5));
    let fams = monospace_families(&["Helvetica".into(), "SF Mono".into(), "Menlo".into(), "Fira Code".into(), "JetBrains Mono".into(), "Menlo".into()]);
    assert_eq!(fams, vec!["Fira Code", "JetBrains Mono", "Menlo", "SF Mono"]);
    assert_eq!(round_tenth(1.6000000238), 1.6);
    assert_eq!(theme_key_for(AppearanceMode::System, true), "dark");
    assert_eq!(theme_key_for(AppearanceMode::System, false), "light");
    assert_eq!(theme_key_for(AppearanceMode::Light, true), "light");
}
```

- [ ] **Step 2: Run to see it fail**

- [ ] **Step 3: Implement**

The window per the constraint and the control texts above: a left column of the five section names, the right column built from `shown.config`; segments and switches as rows of pill buttons (the jump-pill style), lists as rows with a check mark, steppers as "−" and "+" around the value. Every control calls `on_change(&edits, cx)` through `cx.defer` (the closure runs outside the window's update, so Root's push back into the window is not refused); Root's `on_change` runs `config::set_values(&paths::config_file(), edits)` and then `reload_config`; the chooser-state `on_change` only writes. `reload_config` (Root) and the chooser state both push `Shown { config, error, update_ready }` into an open window with `handle.update(cx, |w, _window, cx| w.set_shown(shown, cx))`. Fonts: `monospace_families` over `cx.text_system().all_font_names()` at open, with the advance check (`text_system.advance(font_id, px(16.), 'i') == advance of 'W'`) as the primary filter where gpui exposes it, the name filter as the fallback. The import flow per the constraint. `aux::open_settings` builds `Shown` from the Root (`config`, `config_error`, `update_ready`) or from `LaunchState`.

- [ ] **Step 4: Run, check, commit**

```bash
git add crates/arkos/src/settings.rs crates/arkos/src/aux.rs crates/arkos/src/main.rs crates/arkos/src/config.rs crates/arkos/src/theme.rs
git commit -m "The Settings window: appearance, themes with import, text, startup, updates"
```

---

### Task 5: README, Status rows, and the gates

**Files:**
- Modify: `README.md` (the Status section; the keybinding table at ~386; the themes paragraph at ~469; lines 123 and 439 that say Cmd+, opens the config file), `crates/arkos/src/main.rs` (`open_config`'s doc comment at ~1470 if Task 3a left it)

- [ ] **Step 1: README**

A "Menu" paragraph (the one menu and its five items; with two projects open, each process has its own About and Settings windows); an "About" section (the rows, Copy details, the attribution and the donation line that appears once a URL is set); a "Settings" section (the five sections, that every change writes `config.toml` keeping comments and applies live, "Add theme…" and the themes folder, that keybindings stay in the file and Cmd+, now opens Settings, `arkos theme list` and `validate`); the Timo Purple line under themes; the keybinding table rows ("settings" Cmd+, ; "open config" unbound; "about" unbound); lines 123 and 439 corrected. Status rows: the menu's five items in order; About's rows match `arkos --version` and the paths exist; Copy details; the donation line hidden while the URL is empty; Settings switches the theme live and `config.toml` keeps its comments; "Add theme…" with a bad file shows the error and copies nothing, with a same-named file asks before replacing; `arkos theme list` shows the three built-ins; Timo Purple matches the screenshot's purple, cyan and green at the offsite demo; closing the project window with About open quits the app; a Settings change made from the chooser is in effect when the project opens.

- [ ] **Step 2: Gates**

`grep -rn "Used from Task" crates/arkos/src` prints nothing; no em-dash in README.md, docs/themes.md or `crates/arkos/src/*.rs`; the warning check and the suite pass.

```bash
git add README.md crates/arkos/src/main.rs
git commit -m "README: the menu, About, Settings, Timo Purple"
```

---

## Deferred, for the user to rule on

- macOS conventions the one-menu ruling leaves out: Hide Arkos IDE (Cmd+H), Hide Others, Show All; a Window menu with Minimize (Cmd+M); an Edit menu so Services and the standard Edit shortcuts work in About and Settings; the labels "Settings…" and "Quit Arkos IDE".
- "Check for Updates" in About; About and Settings following a live theme change; Settings remembering the last section.
- A keybinding editor; a theme editor; a theme generator from accent and background (8b may cover it); per-workspace settings; a dropdown element for the pickers; font previews; "Copy details" including the update journal.

## Self-review notes

- Spec section 3: Task 3a. Section 4: Tasks 1, 3b. Section 5: Tasks 1, 3b. Section 6: Task 4. Section 7: Tasks 1, 4. Section 8: Tasks 2, 4. Section 9: Task 5. Section 10: as named; the windows are hand checks.
- Deviations from the spec's letter, each for a reason found in the code or the review: the About and Settings actions live in one registry registered once in `main()` (so the menu works from the chooser, About and Settings windows alike); `text_dim` is `#c4b6ea` (both it and the spec's value pass); "First installed" on the day of the first run reads "today HH:MM"; the import confirm line and the usage exit code are additions.
