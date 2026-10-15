//! The doctor's table (spec 6d, section 5): each check is a probe of the user's setup and the one action that fixes it. Probes read files and flags handed in by Root; nothing here prompts, runs launchctl, or opens a window.
#![cfg_attr(not(test), allow(dead_code))] // Used from Task 6c on.

use std::path::Path;

use crate::config::StartAtLogin;
use crate::hook_cli;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckId {
    WheelScroll,
    StartAtLogin,
    TitleHook,
    Accessibility,
}

/// How a check stands; the text is what the row shows under its title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Good(String),
    Fix(String),
    Broken(String),
    Pinned(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub id: CheckId,
    pub title: &'static str,
    pub state: State,
    /// The label of the one action that fixes the row, if the doctor can make it.
    pub button: Option<&'static str>,
}

/// What the probes read, gathered by Root so the table stays pure.
pub struct Probe<'a> {
    pub settings: &'a Path,
    pub exe: &'a Path,
    pub start_at_login: StartAtLogin,
    pub login_item_installed: bool,
    pub accessibility: bool,
    pub scroll_applied: bool,
}

/// The four checks, in the order the popover lists them.
pub fn probe_all(p: &Probe) -> Vec<Check> {
    let (scroll_state, scroll_button) = match hook_cli::wheel_scroll_acceleration_at(p.settings) {
        Ok(Some(false)) => (State::Good(if p.scroll_applied { "on; restart claude for it to take effect" } else { "on" }.into()), None),
        Ok(_) => (State::Fix("off: fast scrolls jump in Claude Code".into()), Some("Apply")),
        Err(e) => (State::Broken(e), None),
    };
    let (login_state, login_button) = match (p.start_at_login, p.login_item_installed) {
        (StartAtLogin::Ask, false) => (State::Fix("off: sessions do not come back after a restart".into()), Some("Install")),
        (StartAtLogin::Ask, true) => (State::Good("on".into()), Some("Remove")),
        (StartAtLogin::On, _) => (State::Pinned("on, set in config.toml ([recovery] start_at_login)".into()), None),
        (StartAtLogin::Off, _) => (State::Pinned("off, set in config.toml ([recovery] start_at_login)".into()), None),
    };
    let (hook_state, hook_button) = if hook_cli::hooks_installed_at(p.settings, p.exe) {
        (State::Good("installed".into()), None)
    } else {
        (State::Fix("missing: tabs show no Claude Code state".into()), Some("Install"))
    };
    let (ax_state, ax_button) = if p.accessibility {
        (State::Good("granted".into()), None)
    } else {
        (State::Fix("not granted: browser tabs cannot dock".into()), Some("Open System Settings"))
    };
    vec![
        Check { id: CheckId::WheelScroll, title: "Claude Code: smooth wheel scrolling", state: scroll_state, button: scroll_button },
        Check { id: CheckId::StartAtLogin, title: "Start at login", state: login_state, button: login_button },
        Check { id: CheckId::TitleHook, title: "Claude Code title hook", state: hook_state, button: hook_button },
        Check { id: CheckId::Accessibility, title: "Accessibility for browser docking", state: ax_state, button: ax_button },
    ]
}

/// The count behind the dot and the palette detail: the rows the doctor can fix.
pub fn needs_fix(checks: &[Check]) -> usize {
    checks.iter().filter(|c| matches!(c.state, State::Fix(_))).count()
}

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
        assert_eq!(checks[2].title, "Claude Code title hook");
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
