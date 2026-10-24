//! The doctor's table (spec 6d, section 5): each check is a probe of the user's setup and the one
//! action that fixes it. Probes read files and flags handed in by Root; nothing here prompts, runs
//! launchctl, or opens a window.

use std::path::Path;

use crate::config::StartAtLogin;
use crate::guide::SkillState;
use crate::hook_cli;

/// Which row of the doctor a check is; Root uses it to route an action and to mark a row broken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckId {
    WheelScroll,
    ClaudeTheme,
    StartAtLogin,
    TitleHook,
    AgentGuide,
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
    pub scroll_applied: bool,
    /// The skills folder of this process's Claude config (`guide::skills_dir`).
    pub skills: &'a Path,
    /// What that folder holds under `arkos-ide`.
    pub skill: SkillState,
    /// Whether the Claude config folder around `skills` exists at all.
    pub claude_present: bool,
}

/// The six checks, in the order the popover lists them.
pub fn probe_all(p: &Probe) -> Vec<Check> {
    let (scroll_state, scroll_button) = match hook_cli::wheel_scroll_acceleration_at(p.settings) {
        Ok(Some(false)) => (State::Good(if p.scroll_applied { "on; restart claude for it to take effect" } else { "on" }.into()), None),
        Ok(_) => (State::Fix("off: fast scrolls jump in Claude Code".into()), Some("Apply")),
        Err(e) => (State::Broken(e), None),
    };
    let (theme_state, theme_button) = match hook_cli::claude_theme_at(p.settings) {
        Ok(Some(t)) if t == "auto" => (State::Good("on: Claude Code matches each Arkos theme".into()), None),
        Ok(t) => (State::Fix(format!("{}: Claude Code keeps its own colours, unreadable on some Arkos themes", t.as_deref().map_or("not set".to_string(), |t| format!("set to {t}")))), Some("Apply")),
        Err(e) => (State::Broken(e), None),
    };
    let (login_state, login_button) = match (p.start_at_login, p.login_item_installed) {
        (StartAtLogin::Ask, false) => (State::Fix("off: sessions do not come back after a restart".into()), Some("Install")),
        (StartAtLogin::Ask, true) => (State::Good("on".into()), Some("Remove")),
        (StartAtLogin::On, _) => (State::Pinned("on, set in config.toml ([recovery] start_at_login)".into()), None),
        (StartAtLogin::Off, _) => (State::Pinned("off, set in config.toml ([recovery] start_at_login)".into()), None),
    };
    let (hook_state, hook_button) = match hook_cli::hooks_installed_at(p.settings, p.exe) {
        Ok(true) => (State::Good("installed".into()), None),
        Ok(false) => (State::Fix("missing: tabs show no Claude Code state".into()), Some("Install")),
        Err(e) => (State::Broken(e), None),
    };
    let dir = p.skills.display();
    let (guide_state, guide_button) = match (&p.skill, p.claude_present) {
        (_, false) => (State::Pinned("no Claude Code config found".into()), None),
        (SkillState::Missing, true) => (State::Fix(format!("not installed in {dir}: agents cannot find the toolkit")), Some("Install")),
        (SkillState::Current, true) => (State::Good(format!("installed in {dir}; other Claude config folders get it when a session starts inside Arkos")), None),
        (SkillState::Outdated(old), true) => (State::Fix(format!("older than this build ({old})")), Some("Update")),
        (SkillState::Foreign, true) => (State::Broken(format!("a different skill sits at {}", p.skills.join(crate::guide::SKILL_NAME).join("SKILL.md").display())), None),
    };
    vec![
        Check { id: CheckId::WheelScroll, title: "Claude Code: smooth wheel scrolling", state: scroll_state, button: scroll_button },
        Check { id: CheckId::ClaudeTheme, title: "Claude Code: follows the Arkos theme", state: theme_state, button: theme_button },
        Check { id: CheckId::StartAtLogin, title: "Start at login", state: login_state, button: login_button },
        Check { id: CheckId::TitleHook, title: "Claude Code title hook", state: hook_state, button: hook_button },
        Check { id: CheckId::AgentGuide, title: "Agent guide for Claude Code", state: guide_state, button: guide_button },
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
    use crate::guide::SkillState;

    fn probe<'a>(settings: &'a Path, exe: &'a Path) -> Probe<'a> {
        Probe {
            settings,
            exe,
            start_at_login: StartAtLogin::Ask,
            login_item_installed: false,
            scroll_applied: false,
            skills: Path::new("/h/.claude/skills"),
            skill: SkillState::Current,
            claude_present: true,
        }
    }

    #[test]
    fn a_fresh_machine_has_four_things_to_fix() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        let exe = Path::new("/Applications/Arkos IDE.app/Contents/MacOS/arkos");
        let checks = probe_all(&probe(&settings, exe));
        assert_eq!(checks.iter().map(|c| c.id).collect::<Vec<_>>(), vec![CheckId::WheelScroll, CheckId::ClaudeTheme, CheckId::StartAtLogin, CheckId::TitleHook, CheckId::AgentGuide]);
        assert_eq!(checks[0], Check { id: CheckId::WheelScroll, title: "Claude Code: smooth wheel scrolling", state: State::Fix("off: fast scrolls jump in Claude Code".into()), button: Some("Apply") });
        assert_eq!(checks[2].state, State::Fix("off: sessions do not come back after a restart".into()));
        assert_eq!(checks[2].button, Some("Install"));
        assert_eq!(checks[3].state, State::Fix("missing: tabs show no Claude Code state".into()));
        assert_eq!(checks[3].button, Some("Install"));
        assert_eq!(checks[3].title, "Claude Code title hook");
        assert_eq!(checks[1], Check { id: CheckId::ClaudeTheme, title: "Claude Code: follows the Arkos theme", state: State::Fix("not set: Claude Code keeps its own colours, unreadable on some Arkos themes".into()), button: Some("Apply") });
        assert_eq!(needs_fix(&checks), 4);
    }

    #[test]
    fn a_set_up_machine_has_nothing_to_fix() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        let exe = Path::new("/Applications/Arkos IDE.app/Contents/MacOS/arkos");
        crate::hook_cli::install_hooks_at(&settings, exe).unwrap();
        crate::hook_cli::set_wheel_scroll_acceleration_at(&settings, false).unwrap();
        crate::hook_cli::set_claude_theme_auto_at(&settings).unwrap();
        let mut p = probe(&settings, exe);
        p.login_item_installed = true;
        p.scroll_applied = true;
        let checks = probe_all(&p);
        assert_eq!(checks[0].state, State::Good("on; restart claude for it to take effect".into()));
        assert_eq!(checks[0].button, None);
        assert_eq!((checks[2].state.clone(), checks[2].button), (State::Good("on".into()), Some("Remove")));
        assert_eq!(checks[1].state, State::Good("on: Claude Code matches each Arkos theme".into()));
        assert_eq!(checks[3].state, State::Good("installed".into()));
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
    fn config_pins_start_at_login() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        let exe = Path::new("/x/arkos");
        let mut p = probe(&settings, exe);
        p.start_at_login = StartAtLogin::Off;
        let checks = probe_all(&p);
        assert_eq!(checks[2], Check { id: CheckId::StartAtLogin, title: "Start at login", state: State::Pinned("off, set in config.toml ([recovery] start_at_login)".into()), button: None });
        p.start_at_login = StartAtLogin::On;
        assert_eq!(probe_all(&p)[2].state, State::Pinned("on, set in config.toml ([recovery] start_at_login)".into()));
        assert_eq!(needs_fix(&probe_all(&p)), 3, "pinned rows never count; scroll, theme and hook do");
    }

    #[test]
    fn an_unreadable_settings_file_offers_no_action() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        std::fs::write(&settings, "{ not json").unwrap();
        let checks = probe_all(&probe(&settings, Path::new("/x/arkos")));
        assert!(matches!(&checks[0].state, State::Broken(t) if t.starts_with(&format!("cannot parse {}", settings.display()))), "{:?}", checks[0].state);
        assert_eq!(checks[0].button, None);
        assert!(matches!(&checks[3].state, State::Broken(t) if t.starts_with("cannot parse")), "{:?}", checks[3].state);
        assert_eq!(checks[3].button, None);
        assert!(matches!(&checks[1].state, State::Broken(t) if t.starts_with("cannot parse")), "{:?}", checks[1].state);
        assert_eq!(needs_fix(&checks), 1, "a broken row is not a fix the doctor can make; only start at login is left");
    }

    #[test]
    fn the_agent_guide_check_installs_updates_or_leaves_a_stranger_alone() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        let exe = Path::new("/x/arkos");
        let mut p = probe(&settings, exe);
        let guide = |p: &Probe| probe_all(p).into_iter().find(|c| c.id == CheckId::AgentGuide).unwrap();
        assert_eq!(guide(&p).title, "Agent guide for Claude Code");
        assert_eq!(guide(&p).state, State::Good("installed in /h/.claude/skills; other Claude config folders get it when a session starts inside Arkos".into()));
        assert_eq!(guide(&p).button, None);
        let base = needs_fix(&probe_all(&p));
        p.skill = SkillState::Missing;
        assert_eq!(guide(&p).state, State::Fix("not installed in /h/.claude/skills: agents cannot find the toolkit".into()));
        assert_eq!(guide(&p).button, Some("Install"));
        assert_eq!(needs_fix(&probe_all(&p)), base + 1);
        p.skill = SkillState::Outdated("x".into());
        assert_eq!(guide(&p).state, State::Fix("older than this build (x)".into()));
        assert_eq!(guide(&p).button, Some("Update"));
        p.skill = SkillState::Foreign;
        assert_eq!(guide(&p).state, State::Broken("a different skill sits at /h/.claude/skills/arkos-ide/SKILL.md".into()));
        assert_eq!(guide(&p).button, None);
        p.skill = SkillState::Missing;
        p.claude_present = false;
        assert_eq!(guide(&p).state, State::Pinned("no Claude Code config found".into()));
        assert_eq!(guide(&p).button, None);
        assert_eq!(needs_fix(&probe_all(&p)), base, "no Claude Code, nothing to fix");
    }
}
