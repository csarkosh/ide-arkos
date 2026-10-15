# Title Bar Icons, Notifications and the Doctor (milestone 6d) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The title bar's right side becomes two icons, a bell and a doctor, each with a dot and a popover; news (an update ready, an update applied, the daemon's state, config and theme errors) lives behind the bell, and fixes the app can make for the user (Claude Code's wheel scrolling, start at login, the title hook, Accessibility) live behind the doctor.

**Architecture:** A pure `notices` list on `Root`, derived every frame from the state the bar read today (so no assignment site needs a hook); a `doctor_panel` table of checks (title, probe, action) probed when its popover opens and when an action completes; a shared `hook_cli::edit_settings` that reads, backs up and writes Claude Code's settings file for both the hook install and the new scroll key; a `last_version` file so a relaunch knows it was an update; one `Transient<Popover>` on `Root` rendered after the title bar in the style of the context menu. `chrome::title_bar` draws the icons with gpui paths and loses its text items.

**Tech Stack:** Rust 2021, gpui 0.2.2 (`PathBuilder`, `canvas`, `Transient` from `motion.rs`), serde_json, `libc::localtime_r`. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-10-15-title-bar-icons-design.md` (sections 3 to 9 map to the tasks below; its rulings in section 1 are binding: the left side of the bar does not change, "N waiting" leaves the bar, the scroll action sets only the acceleration switch, nothing runs when a popover opens). Reviewed once (plan review of 2026-10-15); its corrections are folded in. Deliberate deviations from the spec's letter, each for a reason found in the code: one `Transient<Popover>` instead of two menus (one fade, never both open); the Accessibility probe is `ax.trusted(false)` rather than `dock.trusted()` (the latter reports true before the first dock tick); the "update failed" and "updates unreachable" texts ride on the Daemon kind (the bar showed them in one slot); the doctor is probed once at startup so its dot is right before the first open (file reads only).

## Global Constraints

- Machine rules (the user's MacBook Air locked up from build load): every cargo call is `nice -n 15 ~/.cargo/bin/cargo ...`; the worktree's local `.cargo/config.toml` (`jobs = 3`, shared `target-dir`) stays; no `--release` builds; never two cargo processes at once; focused filters while iterating, the workspace suite once per task; the warning check is `cargo check --workspace --all-targets` and must print no warning from our crates.
- Dead code between tasks: an item a later task first uses gets `#[cfg_attr(not(test), allow(dead_code))] // Used from Task N on.` (the crate's convention: theme.rs:159, palette.rs:72). The task that first uses it removes the attribute. At the end of Task 6d, `grep -n "Used from Task" crates/arkos/src` must print nothing.
- Texts (exact). Notices: "Update ready: {version}", "Updated to {version}", the daemon label as the bar showed it ("arkosd reconnecting", "arkosd: see banner"; "arkosd connecting" and "arkosd connected" give no notice), "update failed: {error}" (never doubled when the error already starts with "update"), "updates unreachable for {n} days" (the existing `update_unreachable` text, verbatim), the config and theme banner texts unchanged; popover fallback "Nothing new". Doctor rows: "Claude Code: smooth wheel scrolling" ("on" / "off: fast scrolls jump in Claude Code" / "on; restart claude for it to take effect"), "Start at login" ("on" / "off: sessions do not come back after a restart" / "on, set in config.toml ([recovery] start_at_login)" / "off, set in config.toml ([recovery] start_at_login)"), "Claude Code title hook" ("installed" / "missing: tabs show no Claude Code state"), "Accessibility for browser docking" ("granted" / "not granted: browser tabs cannot dock"); an unreadable settings file "cannot read {path}: {error}", one that is not JSON "cannot parse {path}: {error}; fix it by hand first" (the existing hook_cli text). Buttons: "Update now", "Open config", "Open theme", "Apply", "Install", "Remove", "Open System Settings", "Copy report". Notice after the copy: "doctor report copied to the clipboard". Palette: "Notifications" with "{n} unread" or "nothing new", "Doctor" with "{n} to fix" or "all good".
- Geometry: bar height stays `TITLE_BAR_H` (28); two 24 px square targets, 8 px apart, 12 px from the right edge; icons 16 px, stroke 1.5 px, `chrome.text_dim`, `chrome.text` while hovered or open; dot 6 px, `chrome.focus_ring`, at the icon's top right; popovers 320 px (bell) and 360 px (doctor) wide, never wider than the window minus 24 px, right-aligned under their icon, 4 px below the bar, with the context menu's card (`banner` background, 1 px `border`, 6 px radius, `py(4)`, 12 px text, `MENU_LIFT` lift with the `Transient` fade).
- Relative times: "just now" under 60 s, "{n} min ago" under 60 min, "{n} h ago" under 24 h, "yesterday {HH:MM}" (local time) under 48 h, else "{d} days ago". A clock that went back reads "just now".
- Files: `<app support>/last_version` holds the bare version ("0.2.14\n"), written through `last_version.tmp` and `rename`; Claude Code's settings file is `hook_cli::claude_settings_path()`, backed up to `settings.json.bak` before every write of an existing file, pretty-printed with a trailing newline. `wheelScrollAccelerationEnabled` is a real Claude Code setting (confirmed in the installed binary's settings schema: "Ramp mouse-wheel scroll speed during fast scrolls (fullscreen mode only)").
- Reduce Motion: the popover uses `Transient`, whose fade and lift already honour it; nothing else animates.
- No em-dashes in any file. Headless tests only: nothing in `cargo test` opens a window, prompts for Accessibility, runs `launchctl` or `open`, reads the daemon socket, or touches the user's real settings file. There is no gpui test harness in this crate (no `gpui::test`), so UI is tested through pure helpers and hand checks.

## Review Focus

1. A Claude Code settings file that is not JSON (a BOM, a trailing comment): the doctor row shows "cannot parse ..." and no Apply, the file is untouched, no backup is written (Task 3 `an_unparseable_settings_file_is_reported_and_left_alone`).
2. The daemon drops and reconnects repeatedly: one Daemon notice at most, cleared on connect, not a growing list (Task 1 `the_daemon_notice_ranks_connection_over_update_trouble` and `setting_a_kind_again_replaces_it`).
3. A `last_version` file left by a newer build (a downgrade through Homebrew): no "Updated to" notice, the file is rewritten with the running version (Task 2 `a_downgrade_gives_no_notice`).
4. The user opens the bell while an update is ready and closes it: the dot stays, because the update is still waiting (Task 1 `an_update_ready_keeps_the_dot_until_cleared`).
5. Both popovers asked to open: the second closes the first; Escape with nothing open is not taken from the terminal (Task 5 `opening_one_popover_closes_the_other`, `escape_with_nothing_open_is_not_taken`).

## File structure

- Create `crates/arkos/src/notices.rs`: `Notices`, `Notice`, `Kind`, `Action`, `relative_time`, `daemon_notice`. Pure; no gpui.
- Create `crates/arkos/src/doctor_panel.rs`: `Check`, `CheckId`, `State`, `Probe`, `probe_all`, `needs_fix`. Pure.
- Modify `crates/arkos/src/hook_cli.rs`: `edit_settings`, `set_wheel_scroll_acceleration_at`, `wheel_scroll_acceleration_at`, `hooks_installed_at`; `install_hooks_at` becomes a caller of `edit_settings`.
- Modify `crates/arkos/src/paths.rs`: `last_version_file`.
- Modify `crates/arkos/src/startup.rs`: `updated_from`.
- Modify `crates/arkos/src/chrome.rs`: `Right`, `Popover`, `Icon`, `icon_points`, `icon_path`, `title_bar(project, right, fullscreen, theme, on_bell, on_doctor)`, `popover_card`, `popover_right_inset`; `status_items`, `is_update_item`, `UpdateClick` removed.
- Modify `crates/arkos/src/keymap.rs`, `command_palette.rs`: the two commands.
- Modify `crates/arkos/src/main.rs`: `Root` fields `notices`, `popover`, `icon_hover`, `bell_read_before`, `doctor_states`, `scroll_applied`; `sync_notices` in render; the popover rendering; the doctor actions; the two commands.
- Modify `crates/arkos/src/sidebar.rs` (`escape_closes_menu` at 1070), `sessions.rs` (drop `waiting_count`), `update_check.rs` (drop `title_text`), `doctor.rs` (`Where::real` becomes `pub(crate)`).
- Modify `README.md`: Status rows, the Claude Code tips line, every stale "title bar says" line.

---

### Task 1: The notices list

**Files:**
- Create: `crates/arkos/src/notices.rs`
- Modify: `crates/arkos/src/main.rs` (add `mod notices;` next to the other `mod` lines)

**Interfaces:**
- Produces:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Kind { UpdateReady, Updated, Daemon, ConfigError, ThemeError }
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Action { UpdateNow, OpenConfig, OpenTheme }
  #[derive(Debug, Clone, PartialEq)] pub struct Notice { pub kind: Kind, pub text: String, pub at: SystemTime, pub action: Option<Action> }
  #[derive(Default)] pub struct Notices { items: Vec<Notice>, opened_at: Option<SystemTime>, seen_updated: bool }
  impl Notices {
      pub fn set(&mut self, kind: Kind, text: impl Into<String>, action: Option<Action>, now: SystemTime); // replaces the kind's notice; the same text keeps its time
      pub fn clear(&mut self, kind: Kind) -> bool;
      pub fn items(&self) -> &[Notice]; // newest first
      pub fn unread_since(&self, n: &Notice, since: Option<SystemTime>) -> bool; // n.at later than `since`, or no `since`
      pub fn unread(&self, n: &Notice) -> bool; // unread_since(n, self.opened_at)
      pub fn unread_count(&self) -> usize;
      pub fn dot(&self) -> bool; // any unread, or an UpdateReady present
      pub fn opened_at(&self) -> Option<SystemTime>;
      pub fn opened(&mut self, now: SystemTime); // marks everything read
      pub fn closed(&mut self); // drops the Updated notice once it has been seen (opened while present)
      pub fn has(&self, kind: Kind) -> bool;
  }
  pub fn relative_time(at: SystemTime, now: SystemTime) -> String;
  /// What the Daemon kind shows: connection trouble first, then a failed update, then an unreachable update server, else nothing.
  pub fn daemon_notice(label: &str, update_error: Option<&str>, unreachable: Option<&str>) -> Option<String>;
  ```
- The module gets `#[cfg_attr(not(test), allow(dead_code))] // Used from Task 5 on.` on `Notices` (one attribute on the struct and one on each free function suffices; the compiler reports dead items per item, so add the attribute wherever `cargo check` asks).

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
        assert_eq!(n.opened_at(), Some(t(20)));
        assert!(!n.dot());
        assert_eq!(n.unread_count(), 0);
        n.set(Kind::ThemeError, "theme x: missing color", Some(Action::OpenTheme), t(30));
        assert!(n.dot(), "news after the last opening is unread");
        assert!(n.unread(&n.items()[0]));
        assert!(!n.unread(&n.items()[1]));
        assert!(n.unread_since(&n.items()[1], None), "with no opening on record everything is unread");
        assert!(!n.unread_since(&n.items()[1], Some(t(20))));
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
    fn the_daemon_notice_ranks_connection_over_update_trouble() {
        assert_eq!(daemon_notice("arkosd reconnecting", Some("update failed: x"), Some("updates unreachable for 2 days")), Some("arkosd reconnecting".into()));
        assert_eq!(daemon_notice("arkosd: see banner", None, None), Some("arkosd: see banner".into()));
        assert_eq!(daemon_notice("arkosd connected", Some("bad signature"), Some("updates unreachable for 2 days")), Some("update failed: bad signature".into()));
        assert_eq!(daemon_notice("arkosd connected", Some("update failed: bad signature"), None), Some("update failed: bad signature".into()), "never doubled");
        assert_eq!(daemon_notice("arkosd connected", None, Some("updates unreachable for 2 days")), Some("updates unreachable for 2 days".into()));
        assert_eq!(daemon_notice("arkosd connected", None, None), None);
        assert_eq!(daemon_notice("arkosd connecting", None, None), None, "the moment at every launch is not news");
        assert_eq!(daemon_notice("arkosd connecting", Some("x"), None), Some("update failed: x".into()));
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
        assert!(yesterday.starts_with("yesterday ") && yesterday.len() == 15 && yesterday.as_bytes()[12] == b':', "{yesterday}");
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- notices::`
Expected: compile error, `notices` unknown.

- [ ] **Step 3: Implement**

Module doc: "What the bell shows: one notice per kind, newest first, read once the popover has been opened. An update waiting keeps its dot whatever was read." `set` finds the kind: same text keeps `at`, else replaces text, time and action; a new kind pushes; then `items.sort_by(|a, b| b.at.cmp(&a.at))`. `unread_since(n, since)` is `since.map_or(true, |o| n.at > o)`. `dot()` is `unread_count() > 0 || has(UpdateReady)`. `opened(now)` sets `opened_at = Some(now)` and `seen_updated = has(Updated)`; `closed()` clears Updated when `seen_updated`, then resets `seen_updated`. `daemon_notice`: `if label != "arkosd connected" && label != "arkosd connecting" { Some(label) } else if let Some(e) = update_error { Some(if e.starts_with("update") { e.into() } else { format!("update failed: {e}") }) } else { unreachable.map(str::to_string) }`. For "yesterday" use `libc::localtime_r` on the `SystemTime`'s seconds (`libc` is a direct dependency; `recent.rs` shows a `libc` use) and format `"yesterday {:02}:{:02}"`. Any `duration_since` error means "just now".

- [ ] **Step 4: Run, check, commit**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- notices::` (7 passed) and `nice -n 15 ~/.cargo/bin/cargo check --workspace --all-targets` (no warnings; add the dead-code attribute where it asks).

```bash
git add crates/arkos/src/notices.rs crates/arkos/src/main.rs
git commit -m "Notices: what the bell shows, one per kind, read once opened"
```

---

### Task 2: The version file and the "Updated to" notice

**Files:**
- Modify: `crates/arkos/src/paths.rs` (add `last_version_file`; a test in the module at line 91)
- Modify: `crates/arkos/src/startup.rs` (add `updated_from` and `parse_version`; two tests in the existing `mod tests` at line 179)

**Interfaces:**
- Produces: `paths::last_version_file() -> PathBuf` (`app_support().join("last_version")`); `startup::updated_from(file: &Path, current: &str) -> Option<String>`: reads the file, writes `current` (through `<file>.tmp` and rename, creating the parent), and returns `Some(current.to_string())` when the file held a version older than `current` by semver, else `None`. Both get the dead-code attribute "Used from Task 6b on."

- [ ] **Step 1: Failing tests** (add to the existing `mod tests` at startup.rs:179)

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

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- startup:: paths::` and the warning check.

```bash
git add crates/arkos/src/startup.rs crates/arkos/src/paths.rs
git commit -m "A version file tells a relaunch it was an update"
```

---

### Task 3: One settings editor for the hook install and the scroll key

**Files:**
- Modify: `crates/arkos/src/hook_cli.rs` (`install_hooks_at` at 216-233 and around; tests at 409-447)

**Interfaces:**
- Produces:
  ```rust
  /// Reads Claude Code's settings file (a missing one is `{}`), backs it up to `settings.json.bak`
  /// when it exists, lets `edit` change the value, and writes it back pretty with a trailing
  /// newline. A file that does not parse is left untouched (no backup either) and the error returned.
  pub fn edit_settings(path: &Path, edit: impl FnOnce(serde_json::Value) -> serde_json::Value) -> Result<(), String>;
  pub fn set_wheel_scroll_acceleration_at(path: &Path, enabled: bool) -> Result<(), String>;
  /// Ok(Some(value)) when the key is a bool, Ok(None) when absent or the file is missing, Err("cannot read ..." or "cannot parse ...") otherwise.
  pub fn wheel_scroll_acceleration_at(path: &Path) -> Result<Option<bool>, String>;
  /// Whether every event in `hooks_json(exe)` has, in the file, a group whose commands equal ours for that event (so hooks installed from an old or moved binary count as missing, which is what the palette's install would replace); false for a missing or unreadable file.
  pub fn hooks_installed_at(path: &Path, exe: &Path) -> bool;
  ```
- `install_hooks_at` keeps its signature and result and goes through `edit_settings` (its existing tests `install_writes_a_backup_and_the_merged_file` and `merging_is_idempotent_replaces_an_old_path_and_keeps_other_hooks` must pass unchanged). The three new functions get the dead-code attribute "Used from Task 4 on." (`hooks_installed_at`, `wheel_scroll_acceleration_at`) and "Used from Task 6c on." (`set_wheel_scroll_acceleration_at`).

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
    assert!(err.starts_with(&format!("cannot parse {}", path.display())) && err.ends_with("; fix it by hand first"), "{err}");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(!path.with_extension("json.bak").exists(), "no backup of a file we did not change");
    assert!(wheel_scroll_acceleration_at(&path).is_err());
    assert!(!hooks_installed_at(&path, Path::new("/usr/local/bin/arkos")));
}

#[test]
fn hooks_count_as_installed_only_when_every_event_has_ours_from_this_binary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let exe = Path::new("/Applications/Arkos IDE.app/Contents/MacOS/arkos");
    assert!(!hooks_installed_at(&path, exe), "no file");
    install_hooks_at(&path, Path::new("/old/place/arkos")).unwrap();
    assert!(!hooks_installed_at(&path, exe), "hooks from another binary are what an install would replace");
    install_hooks_at(&path, exe).unwrap();
    assert!(hooks_installed_at(&path, exe));
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    v["hooks"].as_object_mut().unwrap().remove("Stop");
    std::fs::write(&path, v.to_string()).unwrap();
    assert!(!hooks_installed_at(&path, exe), "one event's group removed by hand");
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

`set_wheel_scroll_acceleration_at` is `edit_settings(path, |mut v| { if !v.is_object() { v = json!({}) } v["wheelScrollAccelerationEnabled"] = json!(enabled); v })`. A shared `read_settings(path) -> Result<Option<serde_json::Value>, String>` returns `Ok(None)` on NotFound, `Err("cannot read {path}: {e}")` on other read errors, `Err("cannot parse {path}: {e}; fix it by hand first")` on a parse error; `edit_settings`, `wheel_scroll_acceleration_at` and `hooks_installed_at` all use it. `hooks_installed_at`: for every `(event, groups)` in `hooks_json(exe)["hooks"]`, the file's `hooks[event]` must be an array containing a group whose `"hooks"` array equals ours (`serde_json::Value` equality on the group's `hooks` value); any error or missing event gives false.

- [ ] **Step 4: Run all hook tests, check warnings, commit**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- hook_cli::` and `nice -n 15 ~/.cargo/bin/cargo check --workspace --all-targets`.

```bash
git add crates/arkos/src/hook_cli.rs
git commit -m "One editor for Claude Code's settings file: the hooks and the wheel scroll key"
```

---

### Task 4: The doctor's checks

**Files:**
- Create: `crates/arkos/src/doctor_panel.rs`
- Modify: `crates/arkos/src/main.rs` (`mod doctor_panel;`), `crates/arkos/src/hook_cli.rs` (remove the "Used from Task 4" attributes)

**Interfaces:**
- Consumes: Task 3's `hook_cli::wheel_scroll_acceleration_at`, `hooks_installed_at`; `config::StartAtLogin` (see `command_palette.rs:44-48` for how the palette words the four cases).
- Produces (the module gets the dead-code attribute "Used from Task 6c on."):
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum CheckId { WheelScroll, StartAtLogin, TitleHook, Accessibility }
  #[derive(Debug, Clone, PartialEq, Eq)] pub enum State { Good(String), Fix(String), Broken(String), Pinned(String) } // the text under the title
  #[derive(Debug, Clone, PartialEq, Eq)] pub struct Check { pub id: CheckId, pub title: &'static str, pub state: State, pub button: Option<&'static str> }
  /// What the probes read, gathered by Root so the table stays pure.
  pub struct Probe<'a> { pub settings: &'a Path, pub exe: &'a Path, pub start_at_login: config::StartAtLogin, pub login_item_installed: bool, pub accessibility: bool, pub scroll_applied: bool }
  pub fn probe_all(p: &Probe) -> Vec<Check>; // the four, in the spec's order
  pub fn needs_fix(checks: &[Check]) -> usize; // the count behind the dot and the palette detail: rows in State::Fix
  ```

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
        p.scroll_applied = false;
        assert_eq!(probe_all(&p)[0].state, State::Good("on".into()));
    }

    #[test]
    fn the_scroll_key_set_to_true_still_needs_the_fix() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        crate::hook_cli::set_wheel_scroll_acceleration_at(&settings, true).unwrap();
        let checks = probe_all(&probe(&settings, Path::new("/x/arkos")));
        assert_eq!(checks[0].state, State::Fix("off: fast scrolls jump in Claude Code".into()));
        assert_eq!(checks[0].button, Some("Apply"));
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

- [ ] **Step 4: Run, check, commit**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- doctor_panel::` and the warning check (remove hook_cli's "Used from Task 4" attributes now that this module calls them; `set_wheel_scroll_acceleration_at` keeps its attribute: the tests use it, the binary not yet).

```bash
git add crates/arkos/src/doctor_panel.rs crates/arkos/src/main.rs crates/arkos/src/hook_cli.rs
git commit -m "The doctor's checks: four probes and the one action each"
```

---

### Task 5: The bar's icons and the popover

**Files:**
- Modify: `crates/arkos/src/chrome.rs` (all of `status_items`, `is_update_item`, `UpdateClick`, `title_bar`, the tests at 98-125)
- Modify: `crates/arkos/src/main.rs`: `Root` fields near `context_menu` (179) and their init near 495; `any_motion` (577) and `settle` (595); `chrome_state` (2311); the title bar call (2848-2852); `.children(sidebar::render_context_menu(self, cx))` (2978); `on_any_mouse_down` (2929); `observe_window_activation` in `open_project_window` (3341)
- Modify: `crates/arkos/src/sidebar.rs:1070` (`escape_closes_menu`)
- Modify: `crates/arkos/src/sessions.rs` (delete `waiting_count` at 347 and its assertion at 710), `crates/arkos/src/update_check.rs` (delete `title_text` at 143, its test `the_title_bar_ranks_ready_then_failed_then_unreachable` at 227, and the `title_text` assertion inside `the_ready_text_survives_a_failure_and_outranks_it_in_the_title_bar` at 234, keeping that test's `ready_text` assertions)
- Modify: `crates/arkos/src/notices.rs` (remove its dead-code attribute: Root now holds one)

**Interfaces:**
- Produces in `chrome.rs`:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)] pub enum Popover { #[default] Closed, Bell, Doctor }
  impl Popover { pub fn toggled(self, which: Popover) -> Popover; } // the same one closes, another one opens
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Icon { Bell, Doctor }
  pub fn icon_points(icon: Icon) -> Vec<(f32, f32)>; // every anchor and control point, in the 16 px box
  pub fn icon_path(icon: Icon, origin: Point<Pixels>, stroke: Pixels) -> gpui::Path<Pixels>;
  pub struct Right { pub bell_dot: bool, pub doctor_dot: bool, pub open: Popover, pub hover: Popover } // hover: which icon the pointer is over (Closed: none)
  pub type IconClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
  pub type IconHover = Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>;
  pub fn title_bar(project: &Path, right: Right, fullscreen: bool, theme: &Theme, on_bell: (IconClick, IconHover), on_doctor: (IconClick, IconHover)) -> Div;
  pub const ICON_TARGET: f32 = 24.0; pub const ICON_GAP: f32 = 8.0; pub const BAR_RIGHT_PAD: f32 = 12.0;
  pub fn popover_right_inset(which: Popover) -> f32; // Doctor: BAR_RIGHT_PAD; Bell: BAR_RIGHT_PAD + ICON_TARGET + ICON_GAP
  pub fn popover_width(which: Popover, window_w: f32) -> f32; // 320 or 360, at most window_w - 24
  pub fn popover_card(which: Popover, window_w: f32, opacity: f32, live: bool, theme: &Theme, family: SharedString) -> Div;
  ```
- Produces on `Root`: `pub notices: notices::Notices`, `pub popover: Transient<chrome::Popover>`, `pub icon_hover: chrome::Popover`, `fn toggle_bell(&mut self, cx)`, `fn toggle_doctor(&mut self, cx)`, `fn close_popovers(&mut self, now: Instant) -> bool` (true when one was open), `fn render_popover(&mut self, cx) -> Option<Div>`, a stub `fn doctor_needs_fix(&self) -> usize { 0 }`. Task 6b and 6c fill the cards' rows; this task renders each card with one dim row ("Nothing new" for the bell, "Checking..." for the doctor).
- Produces in `sidebar.rs`: `pub(crate) fn escape_closes(key: &str, any_open: bool) -> bool { key == "escape" && any_open }`, used by `escape_closes_menu`.
- Consumes: `motion::Transient` (`show`, `hide`, `value`, `current`, `settle`, `animating`), `motion::MENU_LIFT`, `motion::lift`.

- [ ] **Step 1: Failing tests** (`chrome.rs` tests replace the three removed ones; `sidebar.rs` gets one)

```rust
// chrome.rs
#[test]
fn opening_one_popover_closes_the_other() {
    assert_eq!(Popover::Closed.toggled(Popover::Bell), Popover::Bell);
    assert_eq!(Popover::Bell.toggled(Popover::Doctor), Popover::Doctor);
    assert_eq!(Popover::Doctor.toggled(Popover::Doctor), Popover::Closed);
}

#[test]
fn the_popovers_hang_under_their_icons_and_fit_the_window() {
    assert_eq!(popover_right_inset(Popover::Doctor), BAR_RIGHT_PAD, "the doctor is the last icon");
    assert_eq!(popover_right_inset(Popover::Bell), BAR_RIGHT_PAD + ICON_TARGET + ICON_GAP);
    assert_eq!(popover_width(Popover::Bell, 1200.0), 320.0);
    assert_eq!(popover_width(Popover::Doctor, 1200.0), 360.0);
    assert_eq!(popover_width(Popover::Doctor, 300.0), 276.0, "a narrow window");
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

// sidebar.rs
#[test]
fn escape_with_nothing_open_is_not_taken() {
    assert!(!escape_closes("escape", false));
    assert!(escape_closes("escape", true));
    assert!(!escape_closes("a", true));
}
```

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- chrome:: sidebar::tests::escape`

- [ ] **Step 3: Implement the bar**

In `chrome.rs`, delete `status_items`, `is_update_item`, `UpdateClick` and their tests. `title_bar` keeps everything up to and including the project path child and replaces the last `.child(...)` with a right group:

```rust
.child(
    div().flex_none().flex().items_center().gap(px(ICON_GAP))
        .child(icon_button("bell", Icon::Bell, right.bell_dot, right.open == Popover::Bell || right.hover == Popover::Bell, c, on_bell))
        .child(icon_button("doctor", Icon::Doctor, right.doctor_dot, right.open == Popover::Doctor || right.hover == Popover::Doctor, c, on_doctor)),
)
```

`icon_button(id, icon, dot, lit, c, (on_click, on_hover))` is `div().id(id).size(px(ICON_TARGET)).flex().items_center().justify_center().cursor_pointer().relative()` with `.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())` (so the bar's window-drag does not start), `.on_click(on_click)`, `.on_hover(on_hover)`, holding a 16 px `canvas` that paints `icon_path(icon, bounds.origin, px(1.5))` in `c.text` when `lit` else `c.text_dim`: `canvas(|_, _, _| (), move |bounds, _, window, _| window.paint_path(icon_path(icon, bounds.origin, px(1.5)), color))` (`Window::paint_path(Path<Pixels>, color)` is at gpui window.rs:2860; the path is built per paint, which is cheap). The dot is a sibling `div().absolute().top(px(3.0)).right(px(3.0)).size(px(6.0)).rounded_full().bg(c.focus_ring.hsla())` when `dot`.

Points (16 px box, y down). `PathBuilder::cubic_bezier_to(to, ctrl_a, ctrl_b)` takes the end point first; `curve_to(to, ctrl)` is quadratic. Bell body: `move_to(4, 5.33)`; `cubic_bezier_to((8, 1.33), (4, 3.12), (5.79, 1.33))`; `cubic_bezier_to((12, 5.33), (10.21, 1.33), (12, 3.12))`; `cubic_bezier_to((14, 11.33), (12, 10), (14, 11.33))`; `line_to(2, 11.33)`; `cubic_bezier_to((4, 5.33), (2, 11.33), (4, 10))`; `close()`. Bell clapper: `move_to(6.87, 14)`; `cubic_bezier_to((9.13, 14), (7.2, 14.8), (8.8, 14.8))`. Doctor: `move_to(8, 2)`; four quarter arcs `cubic_bezier_to((14, 8), (11.31, 2), (14, 4.69))`, `((8, 14), (14, 11.31), (11.31, 14))`, `((2, 8), (4.69, 14), (2, 11.31))`, `((8, 2), (2, 4.69), (4.69, 2))`; `close()`; then the plus: `move_to(8, 5)`, `line_to(8, 11)`, `move_to(5, 8)`, `line_to(11, 8)`. `icon_points` lists exactly these points; `icon_path` builds from the same list (keep one table of segments, `enum Seg { Move(P), Line(P), Cubic(P, P, P), Close }`, and derive both from it) with `PathBuilder::stroke(stroke)` and `.build().expect("a fixed path builds")`.

`popover_card(which, window_w, opacity, live, theme, family)` is the context menu's card (copy the style from `sidebar::render_context_menu` at 414): `absolute`, `top(px(TITLE_BAR_H + 4.0) - px(motion::lift(opacity, motion::MENU_LIFT)))`, `right(px(popover_right_inset(which)))`, `w(px(popover_width(which, window_w)))`, `opacity`, `flex_col`, `py(4)`, `rounded(px(6.0))`, `bg(c.banner)`, `border_1`, `border_color(c.border)`, `font_family(family)`, `text_size(12)`, `text_color(c.text)`, and `.when(live, |m| m.occlude().on_any_mouse_down(|_, _, cx| cx.stop_propagation()))`.

- [ ] **Step 4: Wire `Root`**

Fields: `pub notices: notices::Notices` (init `Default::default()`), `pub popover: Transient<chrome::Popover>` (init `Transient::Hidden`), `pub icon_hover: chrome::Popover` (init `Closed`). `toggle_bell`: `let now = Instant::now(); self.context_menu.hide(now); self.wt_menu.hide(now); self.tab_menu.hide(now); let next = self.popover.value().copied().unwrap_or_default().toggled(chrome::Popover::Bell); if next == chrome::Popover::Closed { self.popover.hide(now); } else { self.popover.show(next, now); } cx.notify();` and the same for the doctor. `close_popovers(now)` is `self.popover.hide(now)`.

- `any_motion` (577): add `|| self.popover.animating(now)`. `settle` (595): add `self.popover.settle(now);`. `chrome_state` (2311): add `|| self.popover.value().is_some()` to `menu`, so an idle auto-update does not relaunch under an open popover.
- `on_any_mouse_down` (2929): add `| root.close_popovers(now)` to the condition.
- `sidebar.rs:1070`: `escape_closes_menu` becomes `if !escape_closes(&ks.key, self.context_menu.value().is_some() || self.wt_menu.value().is_some() || self.popover.value().is_some()) { return false; }` then hides the two menus and calls `self.close_popovers(now)`.
- `observe_window_activation` (3341): after `root.window_active = window.is_window_active();` add `if !root.window_active && root.close_popovers(Instant::now()) { cx.notify(); }`.
- Render: replace the three lines at 2848-2852 (`daemon`, `update_text`, `items`, `title_bar`) with:

```rust
let right = chrome::Right { bell_dot: self.notices.dot(), doctor_dot: self.doctor_needs_fix() > 0, open: self.popover.value().copied().unwrap_or_default(), hover: self.icon_hover };
let hover = |which: chrome::Popover| Box::new(cx.listener(move |root, on: &bool, _, cx| { let next = if *on { which } else { chrome::Popover::Closed }; if root.icon_hover != next { root.icon_hover = next; cx.notify(); } })) as chrome::IconHover;
let title_bar = chrome::title_bar(&self.project, right, window.is_fullscreen(), &theme, (Box::new(cx.listener(|root, _, _, cx| root.toggle_bell(cx))), hover(chrome::Popover::Bell)), (Box::new(cx.listener(|root, _, _, cx| root.toggle_doctor(cx))), hover(chrome::Popover::Doctor)));
```

After the `.children(sidebar::render_context_menu(self, cx))` line (2978) add `.children(self.render_popover(window, cx))`: `let (which, opacity) = self.popover.current(self.frame_now)?; let which = *which; let live = self.popover.value().is_some(); let w = f32::from(window.viewport_size().width);` then `chrome::popover_card(which, w, opacity, live, &self.theme, self.family())` with one child row `div().px(px(10.0)).py(px(4.0)).text_color(c.text_dim.hsla()).child(if which == Bell { "Nothing new" } else { "Checking..." })`.

- Dead code after this step: delete `sessions::waiting_count` and `update_check::title_text` as listed in Files; annotate `update_clicked` and `daemon_label_with` with "Used from Task 6b on." and the `update_unreachable` field with the same (it is written but no longer read until 6b). `update_check::ready_text` stays in use.

- [ ] **Step 5: Run everything, check warnings, commit**

Run: `nice -n 15 ~/.cargo/bin/cargo test --workspace` then `nice -n 15 ~/.cargo/bin/cargo check --workspace --all-targets` (no warnings).

Hand check to record in the ledger for the user: screenshot the bar at 1x and 2x, light and dark; the icons read as a bell and a circled plus; the dot sits at the top right of the icon; hover lights the icon; the popover card hangs under its icon and closes on Escape, a click outside, the icon again, and when the window loses focus.

```bash
git add crates/arkos/src/chrome.rs crates/arkos/src/main.rs crates/arkos/src/sidebar.rs crates/arkos/src/sessions.rs crates/arkos/src/update_check.rs crates/arkos/src/notices.rs
git commit -m "The title bar's right side is two icons with a popover; the text items go"
```

---

### Task 6a: The two palette commands

**Files:**
- Modify: `crates/arkos/src/keymap.rs`: `actions!` (11-24), `Command` (30-70), `ACTIONS` rows (112-147; `row("check for updates", "")` at 136), `binding()` (183-237), `command_for` (357), `route!` (440-450), `SPEC_ROWS` (466-503), the counts at 548 and 557
- Modify: `crates/arkos/src/command_palette.rs`: `items` (28) and its `detail`/label matches (43-60); tests (300-320)
- Modify: `crates/arkos/src/main.rs`: `run` (the `match cmd` at 1873; arms near 2015-2049), the palette items call at 1108

**Interfaces:**
- Produces: `Command::Notifications`, `Command::Doctor`; palette names "notifications" and "doctor"; `command_palette::items(..., unread: usize, to_fix: usize)` (two new trailing parameters); `run` arms calling `self.toggle_bell(cx)` and `self.toggle_doctor(cx)`.

- [ ] **Step 1: Failing tests**

In `command_palette.rs` tests, next to the Start at login assertions (304-316), with the `ask_off` fixture built with `2, 1` and another fixture with `0, 0`:

```rust
let bell = find(&ask_off, "Notifications");
assert_eq!((bell.detail.as_str(), bell.command), ("2 unread", Command::Notifications));
let doc = find(&ask_off, "Doctor");
assert_eq!((doc.detail.as_str(), doc.command), ("1 to fix", Command::Doctor));
assert_eq!(find(&quiet, "Notifications").detail, "nothing new");
assert_eq!(find(&quiet, "Doctor").detail, "all good");
let labels: Vec<&str> = ask_off.iter().map(|i| i.label.as_str()).collect();
let check = labels.iter().position(|l| *l == "Check for updates").unwrap();
assert_eq!(&labels[check + 1..check + 3], &["Notifications", "Doctor"]);
```

In `keymap.rs` tests: `assert_eq!(command_for("notifications"), Some(Command::Notifications)); assert_eq!(command_for("doctor"), Some(Command::Doctor));` next to the `command_for` assertions at 557-560.

- [ ] **Step 2: Run to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- command_palette:: keymap::`

- [ ] **Step 3: Implement**

keymap.rs: add `Notifications, Doctor` to `actions!`; add `Command::Notifications` and `Command::Doctor` with doc comments ("Opens the bell's popover." / "Opens the doctor's popover."); in `ACTIONS` add `row("notifications", ""), row("doctor", "")` right after `row("check for updates", "")`; in `binding()` add `"notifications" => KeyBinding::new(keys, Notifications, context)` and `"doctor" => KeyBinding::new(keys, Doctor, context)`; in `command_for` add the two names; in `route!` add `Notifications => Command::Notifications, Doctor => Command::Doctor`; in `SPEC_ROWS` add `("", "notifications", "Notifications"), ("", "doctor", "Doctor")` after check for updates; change `ACTIONS.len() - 11` (548) to `- 13` and `ACTIONS.len() - 10` (557) to `- 12`, adding "notifications, doctor" to both messages. command_palette.rs: `items` gains `unread: usize, to_fix: usize`; the label match gets `Command::Notifications => "Notifications"`, `Command::Doctor => "Doctor"`, the detail match `Command::Notifications => if unread == 0 { "nothing new".into() } else { format!("{unread} unread") }` and `Command::Doctor => if to_fix == 0 { "all good".into() } else { format!("{to_fix} to fix") }`; fix every `items(` call in its tests. main.rs: `Notifications => self.toggle_bell(cx), Doctor => self.toggle_doctor(cx)` in `run` (the match is exhaustive, so the build fails until both arms exist); the call at 1108 passes `self.notices.unread_count(), self.doctor_needs_fix()`.

- [ ] **Step 4: Run, check, commit**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- command_palette:: keymap::` and the warning check.

```bash
git add crates/arkos/src/keymap.rs crates/arkos/src/command_palette.rs crates/arkos/src/main.rs
git commit -m "Palette commands open the bell and the doctor"
```

---

### Task 6b: Feeding the bell and its rows

**Files:**
- Modify: `crates/arkos/src/main.rs`: `Root` fields; `render` (the frame loop after `settle`); `render_popover` from Task 5; `main()` near the `version_string()` log at 3060; `Startup` (3259) and `open_project_window` where `config_error`/`theme_error` are copied onto the root (3369-3372); remove the "Used from Task 6b" attributes on `update_clicked`, `daemon_label_with`, `update_unreachable`
- Modify: `crates/arkos/src/startup.rs`, `paths.rs` (remove the attributes)

**Interfaces:**
- Consumes: Tasks 1, 2, 5.
- Produces on `Root`: `pub bell_read_before: Option<SystemTime>`, `fn sync_notices(&mut self, now: SystemTime)` (called at the top of `render` after `settle`), `fn seed_notices(&mut self, updated: Option<String>)`, `fn run_notice_action(&mut self, action: notices::Action, cx)`, `fn open_theme(&mut self, cx)`.

- [ ] **Step 1: Failing test**

A pure helper decides the UpdateReady text from the field the bar used: in `notices.rs` add `pub fn update_ready_text(bar_text: &str) -> String` ("update ready: 0.2.15" gives "Update ready: 0.2.15"; a text without the prefix is returned as is) with a test `the_update_badge_text_becomes_the_notice_text`.

- [ ] **Step 2: Implement `sync_notices`**

```rust
/// Derives the bell's notices from the state the bar used to show, every frame; idempotent
/// because a notice with the same text keeps its time. No assignment site needs to know.
fn sync_notices(&mut self, now: SystemTime) {
    match self.update_ready.as_deref().filter(|_| self.updating.is_none()) {
        Some(t) => self.notices.set(notices::Kind::UpdateReady, notices::update_ready_text(t), Some(notices::Action::UpdateNow), now),
        None => { self.notices.clear(notices::Kind::UpdateReady); }
    }
    let label = daemon_label_with(&self.status, self.reconnecting_since.is_some());
    match notices::daemon_notice(label, self.update_error.as_deref(), self.update_unreachable.as_deref()) {
        Some(t) => self.notices.set(notices::Kind::Daemon, t, None, now),
        None => { self.notices.clear(notices::Kind::Daemon); }
    }
    match &self.config_error { Some(e) => self.notices.set(notices::Kind::ConfigError, e.clone(), Some(notices::Action::OpenConfig), now), None => { self.notices.clear(notices::Kind::ConfigError); } }
    match &self.theme_error { Some(e) => self.notices.set(notices::Kind::ThemeError, e.clone(), Some(notices::Action::OpenTheme), now), None => { self.notices.clear(notices::Kind::ThemeError); } }
}
```

Call it in `render` right after `self.settle(now, last_frame);` with `SystemTime::now()`. `seed_notices(updated)`: `if let Some(v) = updated { self.notices.set(Kind::Updated, format!("Updated to {v}"), None, SystemTime::now()); }`. In `main()`, next to the `version_string()` log (3060), compute `let updated = startup::updated_from(&paths::last_version_file(), env!("CARGO_PKG_VERSION"));` once per process (a synchronous read and write of one tiny file before any window), add `pub updated: Option<String>` to `Startup`, and call `root.seed_notices(start.updated.clone())` where `config_error` and `theme_error` are copied onto the root (3369-3372).

- [ ] **Step 3: The bell's rows and actions**

`toggle_bell`: when it opens the bell, set `self.bell_read_before = self.notices.opened_at()` then `self.notices.opened(SystemTime::now())`. Every close path (`close_popovers`, and the toggle closing) calls `self.notices.closed()` when the popover being closed is the bell: implement inside `close_popovers`: `let was_bell = self.popover.value() == Some(&chrome::Popover::Bell); let closed = self.popover.hide(now); if closed && was_bell { self.notices.closed(); } closed`, and make the toggle's closing branch go through `close_popovers`.

In `render_popover` for the bell: rows from `self.notices.items()` (clone them first): `div().id(("notice", i)).flex().flex_row().items_center().gap(px(8.0)).px(px(10.0)).py(px(4.0))` with the text (`flex_1().min_w_0().truncate()`, `c.text` when `self.notices.unread_since(n, self.bell_read_before)` else `c.text_dim`), the time (`notices::relative_time(n.at, SystemTime::now())`, `c.text_dim`, `flex_none`), and when `n.action` is Some a `flex_none` text button `div().id(("notice-action", i)).cursor_pointer().hover(|s| s.underline())` labelled "Update now" / "Open config" / "Open theme" whose `on_click` runs `root.run_notice_action(action, cx)` and closes the popover. The UpdateReady row is itself clickable too: its row gets `.cursor_pointer().on_click(...)` running the same action. Empty: one dim row "Nothing new". `run_notice_action`: `UpdateNow => self.update_clicked(cx)` (staged apply, else update from disk, as the badge did), `OpenConfig => self.open_config(cx)`, `OpenTheme => self.open_theme(cx)`. `open_theme`: `let dir = paths::themes_dir(); let file = dir.join(format!("{}.toml", self.theme.name)); let target = if file.is_file() { file } else { dir };` then `std::process::Command::new("open").arg("-t")` only for a file (plain `open` for the directory), spawned like `open_config` (1074), an error to `show_notice`.

- [ ] **Step 4: Run, check, commit**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- notices::` then the workspace suite and the warning check (the "Used from Task 6b" attributes are removed in this task).

```bash
git add crates/arkos/src/main.rs crates/arkos/src/notices.rs crates/arkos/src/startup.rs crates/arkos/src/paths.rs
git commit -m "The bell carries the news: updates, the daemon, config and theme errors"
```

---

### Task 6c: The doctor's rows and actions

**Files:**
- Modify: `crates/arkos/src/main.rs`: `Root` fields; `render_popover`; `toggle_doctor`; `set_start_at_login` (2197) and the `InstallHooks` arm (2037-2049, factored into `fn install_hooks(&mut self, cx)`); `seed_notices`
- Modify: `crates/arkos/src/doctor.rs` (`fn real()` at 32 becomes `pub(crate) fn real()`), `crates/arkos/src/doctor_panel.rs` and `hook_cli.rs` (remove the remaining attributes)

**Interfaces:**
- Consumes: Tasks 3, 4, 5, 6a.
- Produces on `Root`: `pub doctor_states: Vec<doctor_panel::Check>`, `pub scroll_applied: bool`, `fn probe_doctor(&mut self)`, `fn doctor_needs_fix(&self) -> usize` (replacing the stub), `fn doctor_action(&mut self, id: doctor_panel::CheckId, cx)`, `fn copy_doctor_report(&mut self, cx)`.

- [ ] **Step 1: `probe_doctor`**

```rust
/// Reads the doctor's table: files and flags only, no prompt, no launchctl. Run when the
/// popover opens, when an action finishes, and once at startup so the dot is right.
fn probe_doctor(&mut self) {
    let settings = hook_cli::claude_settings_path();
    let exe = update::launch_path().unwrap_or_else(|| PathBuf::from("arkos"));
    let p = doctor_panel::Probe { settings: &settings, exe: &exe, start_at_login: self.config.recovery.start_at_login, login_item_installed: launch_agent::installed(&launch_agent::agent_path()), accessibility: self.ax.trusted(false), scroll_applied: self.scroll_applied };
    self.doctor_states = doctor_panel::probe_all(&p);
}
```

`self.ax.trusted(false)` is the `ax::Ax` trait's trust query without a prompt (ax.rs:158). `doctor_needs_fix` is `doctor_panel::needs_fix(&self.doctor_states)`. `toggle_doctor` calls `probe_doctor` when it opens; `seed_notices` calls it once.

- [ ] **Step 2: Rows**

In `render_popover` for the doctor, one row per check: `div().flex().flex_row().items_center().gap(px(8.0)).px(px(10.0)).py(px(4.0))` holding a `flex_1 min_w_0` column with the title (`c.text`) and the state text under it (`c.text_dim`; `theme.terminal.red` for `State::Broken`), and when `check.button` is Some a `flex_none` text button `div().id(("doctor-action", i)).cursor_pointer().hover(|s| s.underline()).child(label)` whose `on_click` runs `root.doctor_action(check.id, cx)`. The last row is a button "Copy report" running `copy_doctor_report`.

- [ ] **Step 3: Actions, each re-probing when it completes**

The three file actions finish on the background executor, so the re-probe happens in their completion callback inside `root.update`, never right after the call:

- WheelScroll: `let path = hook_cli::claude_settings_path(); let root = cx.entity(); cx.spawn(async move |_, cx| { let r = cx.background_executor().spawn(async move { hook_cli::set_wheel_scroll_acceleration_at(&path, false) }).await; let _ = root.update(cx, |root, cx| { match r { Ok(()) => root.scroll_applied = true, Err(e) => { root.probe_doctor(); if let Some(c) = root.doctor_states.iter_mut().find(|c| c.id == CheckId::WheelScroll) { c.state = State::Broken(e); c.button = None; } cx.notify(); return; } } root.probe_doctor(); cx.notify(); }); }).detach();`
- StartAtLogin: `self.toggle_start_at_login(cx)`; and at the end of `set_start_at_login`'s spawn (2197, inside its `root.update`) add `root.probe_doctor(); cx.notify();` so the palette command refreshes the dot too.
- TitleHook: factor the `InstallHooks` arm into `fn install_hooks(&mut self, cx)` and add `root.probe_doctor(); cx.notify();` inside its completion `root.update`; `run`'s arm and `doctor_action` both call it.
- Accessibility: `cx.open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")`.

`copy_doctor_report`: `doctor::report` opens the daemon socket with a timeout and runs a subprocess, so it runs on the background executor: `let root = cx.entity(); cx.spawn(async move |_, cx| { let text = cx.background_executor().spawn(async { doctor::report(&doctor::Where::real(), SystemTime::now()) }).await; let _ = root.update(cx, |root, cx| { cx.write_to_clipboard(gpui::ClipboardItem::new_string(text)); root.show_notice("doctor report copied to the clipboard".into(), cx); }); }).detach();`

- [ ] **Step 4: Run, check, commit**

Run: the workspace suite and the warning check; `grep -n "Used from Task" crates/arkos/src` must print nothing after this task except nothing (6d touches no code).

```bash
git add crates/arkos/src/main.rs crates/arkos/src/doctor.rs crates/arkos/src/doctor_panel.rs crates/arkos/src/hook_cli.rs
git commit -m "The doctor's rows and actions: scroll key, start at login, title hook, Accessibility"
```

---

### Task 6d: README

**Files:**
- Modify: `README.md`: Status rows after line 77; the Session awareness paragraph (159-162); the Config section after the `update` paragraph (374 onward); every stale "title bar" line: 15, 54, 66, 69, 73-74, 392, 429, 441-442, 444, 454

- [ ] **Step 1: Status rows** (after "a relaunched window opens without taking focus from the app in use")

```
- the title bar's right side shows a bell and a circled plus (the doctor) and no text; with an
  update staged the bell wears a dot and its popover says "Update ready: X" with "Update now";
  after an update the bell says "Updated to X" once, gone after you open and close it; a
  daemon drop shows "arkosd reconnecting" there and the row goes when it is back
- with `wheelScrollAccelerationEnabled` absent from `~/.claude/settings.json` the doctor wears a
  dot; "Apply" writes the key and a `settings.json.bak`; a restarted `claude` scrolls smoothly
  in a fullscreen conversation; the doctor's Start at login and title hook rows match the
  palette's commands; "Copy report" puts `arkos doctor`'s text on the clipboard
- Escape, a click outside, the icon again, or switching to another app closes a popover; opening
  one closes the other; Reduce Motion shows them without the lift
```

- [ ] **Step 2: Stale lines**

Rewrite each line that says the title bar shows text so it names the bell: line 15 ("the title bar counts the sessions waiting" becomes "the tab marks show which sessions wait"), 54 (drop the "1 waiting" clause), 66 and 454 ("update ready" in the bell), 69 ("the bell too"), 73-74 ("see 'update ready: X' in the bell within five minutes"), 392, 429, 441-442 and 444 ("the bell says ..."). Read each in place; keep the sentence's meaning.

- [ ] **Step 3: Tips and the hook paragraph**

Under Config, after the `update` paragraph: "Claude Code tips: fast trackpad scrolling in a fullscreen `claude` jumps unless `wheelScrollAccelerationEnabled` is `false` in `~/.claude/settings.json`; the doctor (the circled plus at the right of the title bar) sets it for you." In the Session awareness paragraph (159-162) add "or the doctor's "Claude Code title hook" row" after "Install Claude Code hooks".

- [ ] **Step 4: Check and commit**

A `grep -c` for the em-dash character (U+2014) over README.md must be 0; `grep -n "title bar" README.md` must show no line claiming the bar shows status text.

```bash
git add README.md
git commit -m "README: the bell and the doctor replace the title bar's text"
```

---

## Deferred, for the user to rule on (not in this milestone)

- Tooltips on the two icons.
- A way to dismiss a doctor check that does not apply (someone who never docks a browser keeps the Accessibility dot forever).
- Without `~/.claude` at all, the scroll and hook rows still show a dot; Apply creates the folder.
- A Dock-launched app may not see `$CLAUDE_CONFIG_DIR` from the shell, so the doctor reads a different settings file than `claude` does.
- With several project windows relaunched after an update, only the first to start says "Updated to X".
- A docked Chrome window (a separate OS window) covers a popover that hangs over its pane.
- Notices do not survive a relaunch (spec section 2).

## Self-review notes

- Spec section 3 (bar): Task 5. Sections 4.1 to 4.4: Tasks 1, 2, 6b. Section 5: Tasks 3, 4, 6c. Section 6: Task 5 (including window deactivation). Section 7: Task 6a. Section 8: as named in each task; the chrome "renders two targets" test is a hand check (no gpui test harness). Section 9: Task 6d. Section 10 defaults taken: "update failed" rides on the Daemon kind through `daemon_notice`, error dots are unread-only.
- The bar's old precedence (6a item 4: ready outranks unreachable and failed) survives: UpdateReady is its own notice; `daemon_notice` ranks connection trouble, then a failed update, then unreachable.
- One `Transient<Popover>` gives one fade and makes "both open" impossible by construction.
