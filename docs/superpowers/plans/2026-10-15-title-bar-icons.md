# Title Bar Icons, Notifications and the Doctor (milestone 6d) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The title bar's right side becomes two icons, a bell and a doctor, each with a dot and a popover; news (an update ready, an update applied, the daemon's state, config and theme errors) lives behind the bell, and fixes the app can make for the user (Claude Code's wheel scrolling, start at login, the title hook, Accessibility) live behind the doctor.

**Architecture:** A pure `notices` list on `Root` fed from the places that set the bar's text today; a `doctor_panel` table of checks (title, probe, action) probed only when its popover opens or after an action; a shared `hook_cli::edit_settings` that reads, backs up and writes Claude Code's settings file for both the hook install and the new scroll key; a `last_version` file so a relaunch knows it was an update; two `Transient` popovers on `Root` rendered after the title bar in the style of the context menu. `chrome::title_bar` draws the icons with gpui paths and loses its text items.

**Tech Stack:** Rust 2021, gpui 0.2.2 (`PathBuilder`, `canvas`, `Transient` from `motion.rs`), serde_json, objc2 only through the existing `dock` and `ax` modules. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-10-15-title-bar-icons-design.md` (sections 3 to 9 map to the tasks below; its rulings in section 1 are binding: the left side of the bar does not change, "N waiting" leaves the bar, the scroll action sets only the acceleration switch, nothing runs when a popover opens).

## Global Constraints

- Machine rules (the user's MacBook Air locked up from build load): every cargo call is `nice -n 15 ~/.cargo/bin/cargo ...`; the worktree's local `.cargo/config.toml` (`jobs = 3`, shared `target-dir`) stays; no `--release` builds; never two cargo processes at once; focused filters while iterating, the workspace suite once per task; the warning check is `cargo check --workspace --all-targets` and must print no warning from our crates.
- Texts (exact): notices "Update ready: {version}", "Updated to {version}", the daemon text as the bar showed it ("arkosd reconnecting", "arkosd connecting", "arkosd: see banner"), "update failed: {error}", the config and theme banner texts unchanged; popover fallback "Nothing new"; doctor rows "Claude Code: smooth wheel scrolling" ("on" / "off: fast scrolls jump in Claude Code" / "on; restart claude for it to take effect"), "Start at login" ("on" / "off: sessions do not come back after a restart" / "on, set in config.toml ([recovery] start_at_login)" / "off, set in config.toml ([recovery] start_at_login)"), "Claude Code title hook" ("installed" / "missing: tabs show no Claude Code state"), "Accessibility for browser docking" ("granted" / "not granted: browser tabs cannot dock"); unreadable settings "cannot read {path}: {error}"; buttons "Update now", "Open config", "Open theme", "Apply", "Install", "Remove", "Open System Settings", "Copy report"; palette "Notifications" and "Doctor" with details "{n} unread" / "nothing new" and "{n} to fix" / "all good".
- Geometry: bar height stays `TITLE_BAR_H` (28); two 24 px square targets, 8 px apart, 12 px from the right edge; icons 16 px, stroke 1.5 px, `chrome.text_dim`, `chrome.text` on hover or while open; dot 6 px, `chrome.focus_ring`, at the icon's top right; notifications popover 320 px wide, doctor 360 px, both right-aligned under their icon, 4 px below the bar, the context menu's card (`banner` background, 1 px `border`, `py(4)`, 12 px text, `MENU_LIFT` lift with the `Transient` fade).
- Relative times: "just now" under 60 s, "{n} min ago" under 60 min, "{n} h ago" under 24 h, else the local time "HH:MM" of the day before or "{d} days ago" past that.
- Files: `<app support>/last_version` holds the bare version ("0.2.14\n"), written through `last_version.tmp` and `rename`; Claude Code's settings file is `hook_cli::claude_settings_path()`, backed up to `settings.json.bak` before every write, pretty-printed with a trailing newline.
- Reduce Motion: the popovers use `Transient`, whose fade and lift already honour it; nothing else animates.
- No em-dashes in any file (en dashes in code are not used either; write "to" or a hyphen). Headless tests only; nothing in `cargo test` opens a window, prompts for Accessibility, runs `launchctl`, or touches the user's real settings file.

## Review Focus

1. A Claude Code settings file with a trailing comment or a BOM (not JSON): the doctor row shows "cannot read ...: ..." and no Apply, the file is untouched, no backup is written (Task 3 `an_unparseable_settings_file_is_reported_and_left_alone`).
2. The daemon reconnects and drops again within a minute: one Daemon notice, its time updated, not a growing list (Task 1 `setting_a_kind_again_replaces_it`).
3. A `last_version` file left by a newer build (a downgrade through Homebrew): no "Updated to" notice, the file is rewritten with the running version (Task 2 `a_downgrade_gives_no_notice`).
4. The user opens the bell while an update is ready and closes it: the dot stays, because the update is still waiting (Task 1 `an_update_ready_keeps_the_dot_until_cleared`).
5. Both popovers asked to open: the second closes the first; Escape with neither open is not swallowed from the terminal (Task 5 `opening_one_popover_closes_the_other` and `escape_with_nothing_open_is_not_taken`).

## File structure

- Create `crates/arkos/src/notices.rs`: `Notices`, `Notice`, `Kind`, `Action`, `relative_time`. Pure; no gpui.
- Create `crates/arkos/src/doctor_panel.rs`: `Check`, `CheckId`, `State`, `Probe` (the inputs a probe reads), `probe_all`, `needs_fix`, `scroll_state`, `report_for_clipboard`. Pure except the actions, which live on `Root` in Task 6.
- Modify `crates/arkos/src/hook_cli.rs`: `edit_settings`, `set_wheel_scroll_acceleration_at`, `wheel_scroll_acceleration_at`, `hooks_installed_at`; `install_hooks_at` becomes a caller of `edit_settings`.
- Modify `crates/arkos/src/paths.rs`: `last_version_file`.
- Modify `crates/arkos/src/startup.rs`: `updated_from(file, current) -> Option<String>` (read, compare, write).
- Modify `crates/arkos/src/chrome.rs`: `Right`, `title_bar(project, right, fullscreen, theme, on_bell, on_doctor)`, `Icon`, `icon_points`, `icon_path`, `Popover`, `popover_card`; `status_items` and `is_update_item` removed.
- Modify `crates/arkos/src/keymap.rs`: actions `Notifications`, `Doctor`; `Command::Notifications`, `Command::Doctor`; names "notifications", "doctor".
- Modify `crates/arkos/src/command_palette.rs`: the two items.
- Modify `crates/arkos/src/main.rs`: `Root` fields `notices`, `notice_menu`, `doctor_menu`, `doctor_states`; feeding the notices; opening, closing and rendering the popovers; the doctor actions; the two commands.
- Modify `crates/arkos/src/sidebar.rs:1070` (`escape_closes_menu`) and `main.rs:2929` (`on_any_mouse_down`): the popovers close like the menus.
- Modify `README.md`: Status rows (section 9 of the spec), the "Claude Code tips" line under Config.

---

### Task 1: The notices list

**Files:**
- Create: `crates/arkos/src/notices.rs`
- Modify: `crates/arkos/src/main.rs` (add `mod notices;` next to the other `mod` lines)

**Interfaces:**
- Produces:
  ```rust
  pub enum Kind { UpdateReady, Updated, Daemon, ConfigError, ThemeError }
  pub enum Action { UpdateNow, OpenConfig, OpenTheme }
  pub struct Notice { pub kind: Kind, pub text: String, pub at: SystemTime, pub action: Option<Action> }
  #[derive(Default)] pub struct Notices { items: Vec<Notice>, opened_at: Option<SystemTime> }
  impl Notices {
      pub fn set(&mut self, kind: Kind, text: impl Into<String>, action: Option<Action>, now: SystemTime); // replaces the kind's notice; same text keeps its time
      pub fn clear(&mut self, kind: Kind) -> bool;
      pub fn items(&self) -> &[Notice]; // newest first
      pub fn unread(&self, n: &Notice) -> bool;
      pub fn unread_count(&self) -> usize;
      pub fn dot(&self) -> bool; // any unread, or an UpdateReady present
      pub fn opened(&mut self, now: SystemTime); // marks everything read
      pub fn closed(&mut self); // drops the Updated notice once it has been seen
      pub fn has(&self, kind: Kind) -> bool;
  }
  pub fn relative_time(at: SystemTime, now: SystemTime) -> String;
  ```

- [ ] **Step 1: Failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn t(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + secs)
    }

    #[test]
    fn notices_are_newest_first_and_one_per_kind() {
        let mut n = Notices::default();
        n.set(Kind::Daemon, "arkosd reconnecting", None, t(10));
        n.set(Kind::UpdateReady, "Update ready: 0.2.15", Some(Action::UpdateNow), t(20));
        assert_eq!(n.items().iter().map(|x| x.text.as_str()).collect::<Vec<_>>(), vec!["Update ready: 0.2.15", "arkosd reconnecting"]);
        assert_eq!(n.items()[0].action, Some(Action::UpdateNow));
    }

    #[test]
    fn setting_a_kind_again_replaces_it() {
        let mut n = Notices::default();
        n.set(Kind::Daemon, "arkosd reconnecting", None, t(10));
        n.set(Kind::Daemon, "arkosd reconnecting", None, t(50));
        assert_eq!(n.items().len(), 1);
        assert_eq!(n.items()[0].at, t(10), "the same text keeps its time");
        n.set(Kind::Daemon, "arkosd: see banner", None, t(60));
        assert_eq!((n.items().len(), n.items()[0].at), (1, t(60)), "a new text is new news");
        assert!(n.clear(Kind::Daemon));
        assert!(!n.clear(Kind::Daemon));
        assert!(n.items().is_empty());
    }

    #[test]
    fn unread_follows_the_last_opening() {
        let mut n = Notices::default();
        n.set(Kind::ConfigError, "config.toml line 3: bad key", Some(Action::OpenConfig), t(10));
        assert!(n.dot());
        assert_eq!(n.unread_count(), 1);
        n.opened(t(20));
        assert!(!n.dot());
        assert_eq!(n.unread_count(), 0);
        n.set(Kind::ThemeError, "theme x: missing color", Some(Action::OpenTheme), t(30));
        assert!(n.dot(), "news after the last opening is unread");
        assert!(n.unread(&n.items()[0]));
        assert!(!n.unread(&n.items()[1]));
    }

    #[test]
    fn an_update_ready_keeps_the_dot_until_cleared() {
        let mut n = Notices::default();
        n.set(Kind::UpdateReady, "Update ready: 0.2.15", Some(Action::UpdateNow), t(10));
        n.opened(t(20));
        assert!(n.dot(), "an update waiting is always news");
        assert_eq!(n.unread_count(), 0, "but it is not counted as unread");
        n.clear(Kind::UpdateReady);
        assert!(!n.dot());
    }

    #[test]
    fn the_updated_notice_goes_once_seen() {
        let mut n = Notices::default();
        n.set(Kind::Updated, "Updated to 0.2.14", None, t(0));
        n.closed();
        assert!(n.has(Kind::Updated), "closing without opening is not seeing it");
        n.opened(t(5));
        n.closed();
        assert!(!n.has(Kind::Updated));
    }

    #[test]
    fn relative_times_read_naturally() {
        let now = t(100_000);
        assert_eq!(relative_time(now - Duration::from_secs(5), now), "just now");
        assert_eq!(relative_time(now - Duration::from_secs(59), now), "just now");
        assert_eq!(relative_time(now - Duration::from_secs(60), now), "1 min ago");
        assert_eq!(relative_time(now - Duration::from_secs(59 * 60), now), "59 min ago");
        assert_eq!(relative_time(now - Duration::from_secs(3600), now), "1 h ago");
        assert_eq!(relative_time(now - Duration::from_secs(23 * 3600), now), "23 h ago");
        assert_eq!(relative_time(now - Duration::from_secs(3 * 86_400), now), "3 days ago");
        assert_eq!(relative_time(now + Duration::from_secs(5), now), "just now", "a clock that went back");
        let yesterday = relative_time(now - Duration::from_secs(30 * 3600), now);
        assert_eq!(yesterday.len(), 5, "yesterday shows the local time HH:MM: {yesterday}");
        assert_eq!(yesterday.as_bytes()[2], b':');
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- notices::`
Expected: compile error, `notices` unknown.

- [ ] **Step 3: Implement**

Module doc: "What the bell shows: one notice per kind, newest first, read once the popover has been opened. An update waiting keeps its dot whatever was read." `Kind` and `Action` derive `Debug, Clone, Copy, PartialEq, Eq`. `set` finds the kind: same text keeps `at`, else replaces text, time and action; a new kind pushes. `items()` returns a sorted view: keep `items` sorted newest first on every `set` (`sort_by(|a, b| b.at.cmp(&a.at))`). `unread(n)` is `opened_at.map_or(true, |o| n.at > o)`. `dot()` is `unread_count() > 0 || has(UpdateReady)`. `opened(now)` sets `opened_at = Some(now)` and `seen_updated = has(Updated)`; `closed()` clears Updated when `seen_updated`. For "yesterday" use the local hour and minute: `chrono` is not a dependency, so compute from `SystemTime` with `libc::localtime_r` (the `libc` crate is already a dependency of the window crate; see `recent.rs` for a `libc` use) and format `"{:02}:{:02}"`. Keep `relative_time` total: any `duration_since` error (clock went back) means "just now".

- [ ] **Step 4: Run and commit**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- notices::`
Expected: 6 passed.

```bash
git add crates/arkos/src/notices.rs crates/arkos/src/main.rs
git commit -m "Notices: what the bell shows, one per kind, read once opened"
```

---

### Task 2: The version file and the "Updated to" notice

**Files:**
- Modify: `crates/arkos/src/paths.rs` (add `last_version_file`; a test in its module at line 90)
- Modify: `crates/arkos/src/startup.rs` (add `updated_from`; tests)

**Interfaces:**
- Produces: `paths::last_version_file() -> PathBuf` (`app_support().join("last_version")`); `startup::updated_from(file: &Path, current: &str) -> Option<String>`: reads the file, writes `current` (through `<file>.tmp` and rename, creating the parent), and returns `Some(current.to_string())` when the file held a version older than `current` by semver, else `None`.
- Consumed by Task 6, which calls it once at startup and seeds `Notices` with `Kind::Updated`.

- [ ] **Step 1: Failing tests** (`startup.rs` has no test module yet: add `#[cfg(test)] mod tests { use super::*; ... }` at the end)

```rust
#[test]
fn an_older_recorded_version_means_this_launch_is_an_update() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("deep").join("last_version");
    assert_eq!(updated_from(&file, "0.2.14"), None, "no file: nothing to say");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "0.2.14\n", "and the file is written, parent created");
    assert_eq!(updated_from(&file, "0.2.14"), None, "same version: nothing");
    assert_eq!(updated_from(&file, "0.2.15"), Some("0.2.15".to_string()));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "0.2.15\n");
    assert_eq!(updated_from(&file, "0.10.0"), Some("0.10.0".to_string()), "semver, not string order");
}

#[test]
fn a_downgrade_gives_no_notice() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("last_version");
    std::fs::write(&file, "0.3.0\n").unwrap();
    assert_eq!(updated_from(&file, "0.2.14"), None);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "0.2.14\n", "the running version is recorded anyway");
    std::fs::write(&file, "garbage").unwrap();
    assert_eq!(updated_from(&file, "0.2.14"), None, "an unreadable version is no version");
    assert!(!dir.path().join("last_version.tmp").exists(), "no temp file left behind");
}
```

And in `paths.rs` tests: `assert!(last_version_file().ends_with("arkos/last_version"))`.

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- startup:: paths::`

- [ ] **Step 3: Implement**

In `startup.rs`:

```rust
/// Reads the version the last launch recorded in `file`, records `current`, and says whether
/// this launch is an update: the recorded version was older. A missing or unreadable file, the
/// same version, or a newer one (a downgrade) gives None. The write goes through a temp file
/// and a rename, as the workspace file does, so a crash leaves the old record, not a torn one.
pub fn updated_from(file: &Path, current: &str) -> Option<String> {
    let previous = std::fs::read_to_string(file).ok().and_then(|s| parse_version(s.trim()));
    let now = parse_version(current);
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = file.with_extension("tmp");
    if std::fs::write(&tmp, format!("{current}\n")).and_then(|_| std::fs::rename(&tmp, file)).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    match (previous, now) {
        (Some(p), Some(n)) if p < n => Some(current.to_string()),
        _ => None,
    }
}

/// "major.minor.patch" as a tuple; anything else is None.
fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let mut it = s.split('.').map(|p| p.parse::<u64>().ok());
    match (it.next()?, it.next()?, it.next()?, it.next()) {
        (Some(a), Some(b), Some(c), None) => Some((a, b, c)),
        _ => None,
    }
}
```

In `paths.rs`: `/// The version the last launch ran, so a relaunch can say "Updated to X" (notices.rs).` `pub fn last_version_file() -> PathBuf { app_support().join("last_version") }`.

- [ ] **Step 4: Run, check, commit**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- startup:: paths::`

```bash
git add crates/arkos/src/startup.rs crates/arkos/src/paths.rs
git commit -m "A version file tells a relaunch it was an update"
```

---

### Task 3: One settings editor for the hook install and the scroll key

**Files:**
- Modify: `crates/arkos/src/hook_cli.rs:207-237` (`install_hooks_at` and around; tests at 389-460)

**Interfaces:**
- Produces:
  ```rust
  /// Reads Claude Code's settings file (a missing one is `{}`), backs it up to `settings.json.bak`
  /// when it exists, lets `edit` change the value, and writes it back pretty with a trailing
  /// newline. A file that does not parse is left untouched (no backup either) and the error returned.
  pub fn edit_settings(path: &Path, edit: impl FnOnce(serde_json::Value) -> serde_json::Value) -> Result<(), String>;
  pub fn set_wheel_scroll_acceleration_at(path: &Path, enabled: bool) -> Result<(), String>;
  /// Ok(Some(value)) when the key is a bool, Ok(None) when absent or the file is missing, Err when unreadable or not JSON.
  pub fn wheel_scroll_acceleration_at(path: &Path) -> Result<Option<bool>, String>;
  /// Whether every event in `hooks_json(exe)` has a group of ours in the file (`is_ours`); false for a missing or unreadable file.
  pub fn hooks_installed_at(path: &Path, exe: &Path) -> bool;
  ```
- `install_hooks_at` keeps its signature and result and goes through `edit_settings` (its two existing tests `install_writes_a_backup_and_the_merged_file` and `merging_is_idempotent_replaces_an_old_path_and_keeps_other_hooks` must still pass unchanged).

- [ ] **Step 1: Failing tests**

```rust
#[test]
fn the_scroll_key_is_set_and_everything_else_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    assert_eq!(wheel_scroll_acceleration_at(&path), Ok(None), "no file");
    set_wheel_scroll_acceleration_at(&path, false).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\n  \"wheelScrollAccelerationEnabled\": false\n}\n");
    assert!(!path.with_extension("json.bak").exists(), "nothing to back up the first time");
    std::fs::write(&path, r#"{"theme":"dark","hooks":{"Stop":[{"hooks":[]}]},"wheelScrollAccelerationEnabled":true}"#).unwrap();
    assert_eq!(wheel_scroll_acceleration_at(&path), Ok(Some(true)));
    set_wheel_scroll_acceleration_at(&path, false).unwrap();
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(v["wheelScrollAccelerationEnabled"], false);
    assert_eq!(v["theme"], "dark");
    assert!(v["hooks"]["Stop"].is_array(), "other keys and nesting kept");
    assert!(path.with_extension("json.bak").exists());
    assert_eq!(wheel_scroll_acceleration_at(&path), Ok(Some(false)));
}

#[test]
fn an_unparseable_settings_file_is_reported_and_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, "\u{feff}{ \"a\": 1 } // comment").unwrap();
    let before = std::fs::read(&path).unwrap();
    let err = set_wheel_scroll_acceleration_at(&path, false).unwrap_err();
    assert!(err.starts_with(&format!("cannot parse {}", path.display())), "{err}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(!path.with_extension("json.bak").exists(), "no backup of a file we did not change");
    assert!(wheel_scroll_acceleration_at(&path).is_err());
    assert!(!hooks_installed_at(&path, Path::new("/usr/local/bin/arkos")));
}

#[test]
fn hooks_count_as_installed_only_when_every_event_has_ours() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let exe = Path::new("/Applications/Arkos IDE.app/Contents/MacOS/arkos");
    assert!(!hooks_installed_at(&path, exe), "no file");
    install_hooks_at(&path, exe).unwrap();
    assert!(hooks_installed_at(&path, exe));
    // Someone removed one event's group by hand.
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    v["hooks"].as_object_mut().unwrap().remove("Stop");
    std::fs::write(&path, v.to_string()).unwrap();
    assert!(!hooks_installed_at(&path, exe));
}
```

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- hook_cli::`

- [ ] **Step 3: Implement**

Lift the read, parse, backup, create-dir and write out of `install_hooks_at` into `edit_settings`; `install_hooks_at` becomes:

```rust
pub fn install_hooks_at(path: &Path, exe: &Path) -> Result<(PathBuf, usize, usize), String> {
    let mut counts = (0, 0);
    edit_settings(path, |existing| {
        let (merged, added, replaced) = merge_hooks(existing, &hooks_json(exe));
        counts = (added, replaced);
        merged
    })?;
    Ok((path.to_path_buf(), counts.0, counts.1))
}
```

`set_wheel_scroll_acceleration_at` is `edit_settings(path, |mut v| { if !v.is_object() { v = json!({}) } v["wheelScrollAccelerationEnabled"] = json!(enabled); v })`. `wheel_scroll_acceleration_at` reads: NotFound gives `Ok(None)`; another read error or a parse error gives `Err(format!("cannot read {}: {e}", ...))` / `cannot parse`; else `Ok(v["wheelScrollAccelerationEnabled"].as_bool())`. `hooks_installed_at` parses the file the same way, returns false on any error, and checks every event key of `hooks_json(exe)["hooks"]` has an array in the file with a group where `is_ours` holds. The error text for a parse failure stays "cannot parse {path}: {e}; fix it by hand first" (the existing message).

- [ ] **Step 4: Run all hook tests, check warnings, commit**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- hook_cli::` (all pass, including the two existing install tests) and `nice -n 15 ~/.cargo/bin/cargo check --workspace --all-targets`.

```bash
git add crates/arkos/src/hook_cli.rs
git commit -m "One editor for Claude Code's settings file: the hooks and the wheel scroll key"
```

---

### Task 4: The doctor's checks

**Files:**
- Create: `crates/arkos/src/doctor_panel.rs`
- Modify: `crates/arkos/src/main.rs` (`mod doctor_panel;`)

**Interfaces:**
- Consumes: Task 3's `hook_cli::wheel_scroll_acceleration_at`, `hooks_installed_at`; `launch_agent::installed(&launch_agent::agent_path())` and `config::StartAtLogin` (see `command_palette.rs:44-48` for how the palette words the four cases); `doctor::report`.
- Produces:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum CheckId { WheelScroll, StartAtLogin, TitleHook, Accessibility }
  #[derive(Debug, Clone, PartialEq, Eq)] pub enum State { Good(String), Fix(String), Broken(String), Pinned(String) } // the text shown under the title
  #[derive(Debug, Clone, PartialEq, Eq)] pub struct Check { pub id: CheckId, pub title: &'static str, pub state: State, pub button: Option<&'static str> }
  /// What the probes read, gathered by Root so the table stays pure.
  pub struct Probe<'a> { pub settings: &'a Path, pub exe: &'a Path, pub start_at_login: config::StartAtLogin, pub login_item_installed: bool, pub accessibility: bool, pub scroll_applied: bool }
  pub fn probe_all(p: &Probe) -> Vec<Check>; // the four, in the spec's order
  pub fn needs_fix(checks: &[Check]) -> usize; // the count behind the dot and the palette detail
  ```
  `button` is Some for `State::Fix` (and for Start at login in the Good state: "Remove"; for Accessibility `Fix`: "Open System Settings"), None for `Broken` and `Pinned`. `State::Good` for WheelScroll reads "on; restart claude for it to take effect" when `scroll_applied` (this process applied it) else "on".

- [ ] **Step 1: Failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::StartAtLogin;

    fn probe<'a>(settings: &'a Path, exe: &'a Path) -> Probe<'a> {
        Probe { settings, exe, start_at_login: StartAtLogin::Ask, login_item_installed: false, accessibility: true, scroll_applied: false }
    }

    #[test]
    fn a_fresh_machine_has_three_things_to_fix() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        let exe = Path::new("/Applications/Arkos IDE.app/Contents/MacOS/arkos");
        let checks = probe_all(&probe(&settings, exe));
        assert_eq!(checks.iter().map(|c| c.id).collect::<Vec<_>>(), vec![CheckId::WheelScroll, CheckId::StartAtLogin, CheckId::TitleHook, CheckId::Accessibility]);
        assert_eq!(checks[0], Check { id: CheckId::WheelScroll, title: "Claude Code: smooth wheel scrolling", state: State::Fix("off: fast scrolls jump in Claude Code".into()), button: Some("Apply") });
        assert_eq!(checks[1].state, State::Fix("off: sessions do not come back after a restart".into()));
        assert_eq!(checks[1].button, Some("Install"));
        assert_eq!(checks[2].state, State::Fix("missing: tabs show no Claude Code state".into()));
        assert_eq!(checks[2].button, Some("Install"));
        assert_eq!(checks[3], Check { id: CheckId::Accessibility, title: "Accessibility for browser docking", state: State::Good("granted".into()), button: None });
        assert_eq!(needs_fix(&checks), 3);
    }

    #[test]
    fn a_set_up_machine_has_nothing_to_fix() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        let exe = Path::new("/Applications/Arkos IDE.app/Contents/MacOS/arkos");
        crate::hook_cli::install_hooks_at(&settings, exe).unwrap();
        crate::hook_cli::set_wheel_scroll_acceleration_at(&settings, false).unwrap();
        let mut p = probe(&settings, exe);
        p.login_item_installed = true;
        p.scroll_applied = true;
        let checks = probe_all(&p);
        assert_eq!(checks[0].state, State::Good("on; restart claude for it to take effect".into()));
        assert_eq!(checks[0].button, None);
        assert_eq!((checks[1].state.clone(), checks[1].button), (State::Good("on".into()), Some("Remove")));
        assert_eq!(checks[2].state, State::Good("installed".into()));
        assert_eq!(needs_fix(&checks), 0);
    }

    #[test]
    fn config_pins_start_at_login_and_accessibility_opens_settings() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        let exe = Path::new("/x/arkos");
        let mut p = probe(&settings, exe);
        p.start_at_login = StartAtLogin::Off;
        p.accessibility = false;
        let checks = probe_all(&p);
        assert_eq!(checks[1], Check { id: CheckId::StartAtLogin, title: "Start at login", state: State::Pinned("off, set in config.toml ([recovery] start_at_login)".into()), button: None });
        assert_eq!(checks[3].state, State::Fix("not granted: browser tabs cannot dock".into()));
        assert_eq!(checks[3].button, Some("Open System Settings"));
        p.start_at_login = StartAtLogin::On;
        assert_eq!(probe_all(&p)[1].state, State::Pinned("on, set in config.toml ([recovery] start_at_login)".into()));
        assert_eq!(needs_fix(&probe_all(&p)), 3, "pinned rows never count; scroll, hook and accessibility do");
    }

    #[test]
    fn an_unreadable_settings_file_offers_no_action() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        std::fs::write(&settings, "{ not json").unwrap();
        let checks = probe_all(&probe(&settings, Path::new("/x/arkos")));
        assert!(matches!(&checks[0].state, State::Broken(t) if t.starts_with(&format!("cannot parse {}", settings.display()))), "{:?}", checks[0].state);
        assert_eq!(checks[0].button, None);
        assert_eq!(checks[2].state, State::Fix("missing: tabs show no Claude Code state".into()), "the hook probe reads false, as the palette would");
        assert_eq!(needs_fix(&checks), 2, "a broken row is not a fix the doctor can make");
    }
}
```

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- doctor_panel::`

- [ ] **Step 3: Implement**

Module doc: "The doctor's table (spec 6d, section 5): each check is a probe of the user's setup and the one action that fixes it. Probes read files and flags handed in by Root; nothing here prompts, runs launchctl, or opens a window." `probe_all` builds the four `Check`s:

- WheelScroll: `match hook_cli::wheel_scroll_acceleration_at(p.settings) { Ok(Some(false)) => Good(if p.scroll_applied { "on; restart claude for it to take effect" } else { "on" }), Ok(_) => Fix("off: fast scrolls jump in Claude Code") with "Apply", Err(e) => Broken(e) }`.
- StartAtLogin: `match (p.start_at_login, p.login_item_installed)`: `(Ask, false)` Fix "off: sessions do not come back after a restart" with "Install"; `(Ask, true)` Good "on" with "Remove"; `(On, _)` Pinned "on, set in config.toml ([recovery] start_at_login)"; `(Off, _)` Pinned "off, set in config.toml ([recovery] start_at_login)".
- TitleHook: `hook_cli::hooks_installed_at(p.settings, p.exe)` gives Good "installed" else Fix "missing: tabs show no Claude Code state" with "Install".
- Accessibility: `p.accessibility` gives Good "granted" else Fix "not granted: browser tabs cannot dock" with "Open System Settings".

`needs_fix` counts `State::Fix`.

- [ ] **Step 4: Run and commit**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- doctor_panel::`

```bash
git add crates/arkos/src/doctor_panel.rs crates/arkos/src/main.rs
git commit -m "The doctor's checks: four probes and the one action each"
```

---

### Task 5: The bar's icons and the two popovers

**Files:**
- Modify: `crates/arkos/src/chrome.rs` (all of `status_items`, `is_update_item`, `UpdateClick`, `title_bar`, the tests at 98-125)
- Modify: `crates/arkos/src/main.rs`: `Root` fields near `context_menu` (line 179); their init near line 505; the title bar call at 2850-2852; `.children(sidebar::render_context_menu(self, cx))` at 2978; `on_any_mouse_down` at 2929; `on_mouse_move`'s frame loop is untouched
- Modify: `crates/arkos/src/sidebar.rs:1070` (`escape_closes_menu`)

**Interfaces:**
- Produces in `chrome.rs`:
  ```rust
  pub struct Right { pub bell_dot: bool, pub bell_open: bool, pub doctor_dot: bool, pub doctor_open: bool }
  pub type IconClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
  pub fn title_bar(project: &Path, right: Right, fullscreen: bool, theme: &Theme, on_bell: IconClick, on_doctor: IconClick) -> Div;
  pub const ICON_TARGET: f32 = 24.0; pub const ICON_GAP: f32 = 8.0; pub const BAR_RIGHT_PAD: f32 = 12.0;
  /// Where a popover's right edge sits: the bar's right padding plus, for the bell, the doctor's target and the gap.
  pub fn popover_right_inset(bell: bool) -> f32;
  /// The card every popover is drawn in, positioned under the bar at `right` from the window's right edge, `width` wide, with `opacity` from the Transient.
  pub fn popover_card(width: f32, right: f32, opacity: f32, live: bool, theme: &Theme, family: SharedString) -> Div;
  ```
- Produces on `Root`: `pub notice_menu: Transient<()>`, `pub doctor_menu: Transient<()>`, `fn toggle_bell(&mut self, cx)`, `fn toggle_doctor(&mut self, cx)`, `fn close_popovers(&mut self, now: Instant) -> bool` (true when one was open). Task 6 fills the cards' rows; this task renders the cards with one dim line each ("Nothing new" and "Checking...") so the shell is visible.
- Consumes: `motion::Transient` (`show`, `hide`, `value`, `current`), `motion::MENU_LIFT`, `motion::lift`.

- [ ] **Step 1: Failing tests** (`chrome.rs` tests replace the three removed ones; `main.rs` tests at 3443)

```rust
// chrome.rs
#[test]
fn the_popovers_hang_under_their_icons() {
    assert_eq!(popover_right_inset(false), BAR_RIGHT_PAD, "the doctor is the last icon");
    assert_eq!(popover_right_inset(true), BAR_RIGHT_PAD + ICON_TARGET + ICON_GAP);
}

#[test]
fn the_bell_and_the_doctor_icons_stay_in_a_16_px_box() {
    for icon in [Icon::Bell, Icon::Doctor] {
        let pts = icon_points(icon);
        assert!(pts.len() >= 6, "{icon:?} has a shape");
        for (x, y) in pts {
            assert!((0.75..=15.25).contains(&x) && (0.75..=15.25).contains(&y), "{icon:?} point {x},{y} leaves room for a 1.5 px stroke");
        }
    }
}
```

(`#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Icon { Bell, Doctor }`; `icon_points(icon) -> Vec<(f32, f32)>` lists every anchor and control point of the icon's strokes in the 16 px box, and `icon_path(icon, origin: Point<Pixels>, stroke: Pixels) -> gpui::Path<Pixels>` builds the path from those same points offset by `origin`, with `PathBuilder::stroke(width)` and `move_to`/`line_to`/`curve_to`/`cubic_bezier_to`, then `.build().expect("a fixed path builds")`. The path is built once per paint, which is cheap. `gpui::Path`'s own bounds are in `ScaledPixels`, whose value is not readable from outside gpui, hence the test on the points.)

```rust
// main.rs tests
#[gpui::test]
fn opening_one_popover_closes_the_other(cx: &mut gpui::TestAppContext) { ... }
```

If the existing `main.rs` tests do not use `gpui::test` (check the module at 3443; if they are plain `#[test]` functions on pure helpers), do not add a gpui test. Instead make the open and close logic a pure helper and test that:

```rust
/// Which popover is open, with the rule that opening one closes the other and a second press closes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Popover { #[default] None, Bell, Doctor }
impl Popover {
    pub fn toggled(self, which: Popover) -> Popover { if self == which { Popover::None } else { which } }
}
#[test]
fn opening_one_popover_closes_the_other() {
    assert_eq!(Popover::None.toggled(Popover::Bell), Popover::Bell);
    assert_eq!(Popover::Bell.toggled(Popover::Doctor), Popover::Doctor);
    assert_eq!(Popover::Doctor.toggled(Popover::Doctor), Popover::None);
}
```

With `Popover`, `Root` carries one `pub popover: Transient<Popover>` instead of two fields, which also gives one fade; `toggle_bell` is `let next = self.popover.value().copied().unwrap_or_default().toggled(Popover::Bell); if next == Popover::None { self.popover.hide(now) } else { self.popover.show(next, now) }`. Use this form.

And for Escape:

```rust
#[test]
fn escape_with_nothing_open_is_not_taken() // in sidebar.rs's tests if `escape_closes_menu` is testable there; else assert through the pure rule: `escape_closes_menu` returns false when `context_menu`, `wt_menu` and `popover` are all hidden. If it needs a Root, skip the test and note it in the ledger; the hand check covers it.
```

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- chrome:: popover`

- [ ] **Step 3: Implement the bar**

In `chrome.rs`, delete `status_items`, `is_update_item`, `UpdateClick` and their tests. `title_bar` keeps everything up to and including the project path child and replaces the last `.child(...)` with a right group:

```rust
.child(
    div().flex_none().flex().items_center().gap(px(ICON_GAP))
        .child(icon_button("bell", Icon::Bell, right.bell_dot, right.bell_open, c, on_bell))
        .child(icon_button("doctor", Icon::Doctor, right.doctor_dot, right.doctor_open, c, on_doctor)),
)
```

`icon_button(id, icon, dot, open, c, on_click)` is a `div().id(id).size(px(ICON_TARGET)).flex().items_center().justify_center().cursor_pointer().relative()` with `.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())` (so the bar's window-drag does not start) and `.on_click(on_click)`, holding a `canvas` of 16 px that paints the path in `c.text_dim` (or `c.text` when `open`), hovered colour via a `.group("icon")` and `.group_hover` is not needed: paint the colour from `open` and let the hit target's `.hover(|s| s.bg(c.tab_active.hsla()))` show the hover (a faint square), which keeps the canvas colour static. The dot is a sibling `div().absolute().top(px(3.0)).right(px(3.0)).size(px(6.0)).rounded_full().bg(c.focus_ring.hsla())` when `dot`.

Painting a path in a canvas: `canvas(|_, _, _| (), move |bounds, _, window, _| window.paint_path(icon_path(icon, bounds.origin, px(1.5)), color))`: the path is built at the canvas's origin in the paint closure (`Path::translate` in gpui 0.2.2 takes scaled pixels, so building in place is simpler). `window.paint_path(path: Path<Pixels>, color)` is at gpui window.rs:2860.

Points (16 px box, y down). Bell: body from (4, 11) up the left side with `curve_to((8, 2), ctrl (3.5, 4))`, over the top to (12, 11) with `curve_to((12, 11), ctrl (12.5, 4))`, close across the bottom (12, 11) to (4, 11) with a line; the rim as a line (3, 11) to (13, 11); the clapper as a short arc under it: `move_to((6.5, 13))`, `curve_to((9.5, 13), ctrl (8, 15))`. Doctor: a circle of radius 6 at (8, 8) made of four `cubic_bezier_to` quarter arcs (control distance 6 by 0.5523), then a plus: (8, 5) to (8, 11) and (5, 8) to (11, 8). Keep every point inside 0.75 of the box edge so the stroke stays inside 16 px (the test checks the path bounds, which `PathBuilder` grows by the stroke).

`popover_card` is the context menu's card (copy the style from `sidebar::render_context_menu`: `absolute`, `top(px(TITLE_BAR_H + 4.0) - px(motion::lift(opacity, motion::MENU_LIFT)))`, `right(px(right))`, `w(px(width))`, `opacity`, `flex_col`, `py(4)`, `bg(c.banner)`, `border_1`, `border_color(c.border)`, `font_family(family)`, `text_size(12)`, `text_color(c.text)`, and `.when(live, |m| m.occlude().on_any_mouse_down(|_, _, cx| cx.stop_propagation()))`).

- [ ] **Step 4: Wire `Root`**

Add `pub popover: Transient<Popover>` (init `Transient::Hidden`). `toggle_bell`/`toggle_doctor` as above, plus `cx.notify()`. `close_popovers(now) -> bool` is `self.popover.hide(now)`. In `on_any_mouse_down` (main.rs:2929) add `| root.close_popovers(now)` to the condition. In `sidebar.rs:1070` extend the early return with `&& self.popover.value().is_none()` and call `self.popover.hide(now)` with the others. In render, replace the three lines at 2850-2852 with:

```rust
let open = self.popover.value().copied().unwrap_or_default();
let right = chrome::Right { bell_dot: self.notices.dot(), bell_open: open == chrome::Popover::Bell, doctor_dot: self.doctor_needs_fix() > 0, doctor_open: open == chrome::Popover::Doctor };
let title_bar = chrome::title_bar(&self.project, right, window.is_fullscreen(), &theme, Box::new(cx.listener(|root, _, _, cx| root.toggle_bell(cx))), Box::new(cx.listener(|root, _, _, cx| root.toggle_doctor(cx))));
```

Until Task 6 lands, `self.notices` does not exist: add the field `pub notices: notices::Notices` (init `Default::default()`) in this task, and a stub `fn doctor_needs_fix(&self) -> usize { 0 }` that Task 6 replaces. Put `Popover` in `chrome.rs` (it is UI state the bar reads). After the `.children(sidebar::render_context_menu(self, cx))` line add `.children(self.render_popover(cx))`, where `render_popover` returns `Option<Div>`: `let (which, opacity) = self.popover.current(self.frame_now)?; let live = self.popover.value().is_some();` then `chrome::popover_card(320.0 or 360.0, chrome::popover_right_inset(which == Bell), opacity, live, &self.theme, self.family())` with one child row `div().px(px(10.0)).py(px(4.0)).text_color(c.text_dim.hsla()).child("Nothing new")` for the bell and `"Checking..."` for the doctor. The `daemon_label_with` computation at 2848 and `update_check::title_text` become unused: remove `title_text` only if nothing else uses it (keep `daemon_label_with`; Task 6 feeds its text to the notices).

- [ ] **Step 5: Run everything, check warnings, commit**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- chrome:: popover` then `nice -n 15 ~/.cargo/bin/cargo check --workspace --all-targets` (no warnings: an unused `update_text`, `items`, or `title_text` must be removed, not allowed).

```bash
git add crates/arkos/src/chrome.rs crates/arkos/src/main.rs crates/arkos/src/sidebar.rs
git commit -m "The title bar's right side is two icons with popovers; the text items go"
```

---

### Task 6: Feeding the bell, the doctor's rows and actions, the palette, the README

**Files:**
- Modify: `crates/arkos/src/main.rs`: `Root::new` (seed `notices` from `startup::updated_from`), `refresh_update_badge` (2304), `switch_theme_to` (664), `reload_config` (672), the three `self.status =` sites (2483, 2517, 2672) and `self.update_error =` sites (2395, 2590), `run` arms near 2015-2049, `render_popover` from Task 5, the palette items call at 1108
- Modify: `crates/arkos/src/keymap.rs` (`actions!` at 11-24, `Command` enum at 30-70, `route!` list at 440-450, names at 385-396)
- Modify: `crates/arkos/src/command_palette.rs` (`items` at 28; tests near 304-316)
- Modify: `README.md` (Status rows after line 77; a "Claude Code tips" line in the Config section after line 382; the Session awareness paragraph at 161 mentions the doctor)

**Interfaces:**
- Consumes: Tasks 1 to 5.
- Produces on `Root`: `pub doctor_states: Vec<doctor_panel::Check>` (probed on open and after an action), `pub scroll_applied: bool`, `fn probe_doctor(&mut self)`, `fn doctor_action(&mut self, id: CheckId, cx)`, `fn doctor_needs_fix(&self) -> usize` (replacing the stub), `fn note_daemon(&mut self)` (called wherever `status` or `reconnecting_since` changes; sets or clears `Kind::Daemon` from `daemon_label_with`), `fn note_update(&mut self)` (from `update_ready` and `update_error`).

- [ ] **Step 1: Failing tests** (palette and keymap)

```rust
// command_palette.rs tests, next to the Start at login assertions at 304-316
let bell = find(&ask_off, "Notifications");
assert_eq!((bell.detail.as_str(), bell.command), ("2 unread", Command::Notifications));
let doc = find(&ask_off, "Doctor");
assert_eq!((doc.detail.as_str(), doc.command), ("1 to fix", Command::Doctor));
```

`items` gains two parameters, `unread: usize` and `to_fix: usize`, at the end; details are `"{n} unread"` / `"nothing new"` and `"{n} to fix"` / `"all good"`. The two items go right after "Check for updates". Fix every call of `items` in the tests (pass `2, 1` in the `ask_off` fixture and `0, 0` elsewhere; the fixture names are in that test module). `keymap.rs` has a test that every action has a palette name (find it with `grep -n "names\|every action" crates/arkos/src/keymap.rs`); adding `Notifications` and `Doctor` to `actions!`, to `Command`, to `route!` and to the names table ("notifications", "doctor") keeps it passing.

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- command_palette:: keymap::`

- [ ] **Step 3: Feed the notices**

In `Root::new`: `notices: Default::default()` stays; in `open_project_window` (after `Root::new`, where `config_error` and `theme_error` are copied onto the root, around line 3330) call `root.seed_notices(updated)` where `updated = startup::updated_from(&paths::last_version_file(), env!("CARGO_PKG_VERSION"))` is computed once in `main` before the window (next to the `version_string()` log line at 3060, so it runs once per process, not per window) and handed through `Startup` (add `pub updated: Option<String>`). `seed_notices` sets `Kind::Updated` "Updated to {v}" with `SystemTime::now()` when Some, then calls `note_update`, `note_daemon`, and sets ConfigError and ThemeError from the two fields.

`note_update`: `match (self.update_ready.as_deref(), self.update_error.as_deref())`: ready gives `set(UpdateReady, format!("Update ready: {v}"), Some(UpdateNow))` where `v` is the version after "update ready: " (the field holds the bar's old text; strip the prefix with `strip_prefix("update ready: ").unwrap_or(text)`), else `clear(UpdateReady)`; an error gives `set(Daemon, format!("update failed: {e}"), None)` only when the daemon kind is not already carrying a connection text (connection trouble outranks). Call `note_update` at the end of `refresh_update_badge`, after both `self.update_error =` assignments, and when `updating` becomes Some (clear UpdateReady there: an update under way is no longer waiting).

`note_daemon`: `let label = daemon_label_with(&self.status, self.reconnecting_since.is_some()); if label == CONNECTED_LABEL { clear(Daemon) } else { set(Daemon, label, None) }`. Call it after each `self.status =` and wherever `reconnecting_since` is set or cleared (grep `reconnecting_since =`).

Config and theme: after `self.theme_error = banner;` set or clear `ThemeError` (action `OpenTheme`: opens the current theme's file in the editor the way "Open config" opens the config; if there is no such command today, the action opens the themes directory with `cx.open_url(file URL)`); after `self.config_error = banner;` the same with `ConfigError` and `OpenConfig` (run `Command::OpenConfig`).

- [ ] **Step 4: The bell's rows**

In `render_popover` for the bell: `self.notices.items()` as rows, newest first; each row `flex_row items_center gap(8) px(10) py(4)`: the text (`flex_1 min_w_0 truncate`, `c.text` when unread else `c.text_dim`), the time (`relative_time(n.at, SystemTime::now())`, `c.text_dim`), and when `n.action` is Some a text button (`div().id(("notice-action", i)).cursor_pointer().hover(underline)`) labelled "Update now" / "Open config" / "Open theme" that runs the action and closes the popover. Empty: one dim row "Nothing new". Opening the bell (`toggle_bell` when it opens) calls `self.notices.opened(SystemTime::now())`; every close path (`close_popovers`, the toggle, Escape) calls `self.notices.closed()` when the bell was the one open. The dot keeps following `self.notices.dot()`.

- [ ] **Step 5: The doctor's rows and actions**

`probe_doctor`: builds `doctor_panel::Probe { settings: &hook_cli::claude_settings_path(), exe: &update::launch_path().unwrap_or_else(|| PathBuf::from("arkos")), start_at_login: self.config.recovery.start_at_login, login_item_installed: launch_agent::installed(&launch_agent::agent_path()), accessibility: self.ax.trusted(false), scroll_applied: self.scroll_applied }` and stores `probe_all`. `self.ax.trusted(false)` is the `ax::Ax` trait's trust query without a prompt (ax.rs:158; `request_accessibility` at main.rs:1405 is the prompting form). Called when the doctor popover opens and after each action. `doctor_needs_fix` is `doctor_panel::needs_fix(&self.doctor_states)`; probe once at startup too (in `seed_notices`) so the dot is right before the first open; this is file reads only.

Rows: title (`c.text`), under it the state text (`c.text_dim`, or the error colour `theme.palette.red` equivalent used by banners for `Broken`), the button on the right when `check.button` is Some. The last row is "Copy report": `cx.write_to_clipboard(ClipboardItem::new_string(doctor::report(&doctor::Where::real(), SystemTime::now())))` (make `Where::real` `pub(crate)`), then a notice "report copied".

`doctor_action(id)`:
- WheelScroll: `hook_cli::set_wheel_scroll_acceleration_at(&path, false)` on the background executor like `InstallHooks` does, then `scroll_applied = true`, `probe_doctor`, `cx.notify()`; an error becomes the row's `Broken` text (store it in `doctor_states` directly).
- StartAtLogin: `self.toggle_start_at_login(cx)` then `probe_doctor` (the toggle is synchronous today).
- TitleHook: the body of the `InstallHooks` arm, with `probe_doctor` after the notice (factor the arm into `fn install_hooks(&mut self, cx)` and call it from both).
- Accessibility: `cx.open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")`.

- [ ] **Step 6: Palette and commands**

`Command::Notifications => self.toggle_bell(cx)`, `Command::Doctor => self.toggle_doctor(cx)` in `run`; `items(..., self.notices.unread_count(), self.doctor_needs_fix())` at the call site.

- [ ] **Step 7: README**

Status rows (after the "a relaunched window opens without taking focus" row):

```
- the title bar's right side shows a bell and a doctor and no text; with an update staged the
  bell wears a dot and its popover says "Update ready: X" with "Update now"; after an update
  the bell says "Updated to X" once, gone after you open and close it
- with `wheelScrollAccelerationEnabled` absent from `~/.claude/settings.json` the doctor wears a
  dot; "Apply" writes the key and a `settings.json.bak`; a restarted `claude` scrolls smoothly
  in a fullscreen conversation; the doctor's Start at login and title hook rows match the
  palette's commands
- Escape or a click outside closes a popover; opening one closes the other; Reduce Motion
  shows them without the lift
```

Under Config, after the `update` paragraph, a line: "Claude Code tips: fast trackpad scrolling in a fullscreen `claude` jumps unless `wheelScrollAccelerationEnabled` is `false` in `~/.claude/settings.json`; the doctor (the stethoscope in the title bar) sets it for you." In the Session awareness paragraph (161) add "or the doctor's "Claude Code title hook" row".

- [ ] **Step 8: Run the suite, check warnings, commit**

Run: `nice -n 15 ~/.cargo/bin/cargo test --workspace` and `nice -n 15 ~/.cargo/bin/cargo check --workspace --all-targets`; a `grep -c` for the em-dash character (U+2014) over README.md and `crates/arkos/src/*.rs` must be 0 everywhere.

```bash
git add README.md crates/arkos/src/main.rs crates/arkos/src/keymap.rs crates/arkos/src/command_palette.rs crates/arkos/src/startup.rs
git commit -m "The bell carries the news and the doctor the fixes; palette commands for both"
```

---

## Self-review notes

- Spec section 3 (bar): Task 5. Section 4.1 to 4.4 (notices, sources, version file, popover): Tasks 1, 2, 6. Section 5 (doctor, settings edit, popover): Tasks 3, 4, 6. Section 6 (behaviour): Task 5, with the "window loses focus" close left to `observe_window_activation` (add `root.close_popovers(now)` there in Task 5 step 4). Section 7 (palette): Task 6. Section 8 (tests): spread as named. Section 9 (README): Task 6. Section 10 defaults taken: no separate kind for "update failed" (it rides on Daemon), error dots unread-only.
- The bar's old `title_text` precedence (6a item 4: ready outranks unreachable and failed) survives as: UpdateReady is its own notice, "update failed" only fills Daemon when no connection text is there.
- `Popover` is one `Transient` so the two cards share one fade and cannot both be open.
