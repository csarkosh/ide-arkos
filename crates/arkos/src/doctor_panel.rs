//! The doctor's table (spec 6d, section 5): each check is a probe of the user's setup and the one
//! action that fixes it. Probes read files and flags handed in by Root; nothing here prompts, runs
//! launchctl, or opens a window.

use std::path::{Path, PathBuf};

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
    /// Claude Code's settings file in each config folder Arkos knows, the default folder's
    /// first (`claude_folders`). Never empty.
    pub settings: &'a [PathBuf],
    /// The home folder, which a row's text writes `~`.
    pub home: Option<&'a Path>,
    /// The tabs whose Claude Code reports nothing (`agent_probe`), as the tab bar names them.
    pub silent: &'a [String],
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

/// How one config folder stands on a row.
enum Answer {
    Good,
    /// It needs the row's fix; the word says how it stands ("off", "missing", "set to dark").
    Needs(String),
    Broken(String),
}

fn scroll_answer(settings: &Path) -> Answer {
    match hook_cli::wheel_scroll_acceleration_at(settings) {
        Ok(Some(false)) => Answer::Good,
        Ok(_) => Answer::Needs("off".into()),
        Err(e) => Answer::Broken(e),
    }
}

fn theme_answer(settings: &Path) -> Answer {
    match hook_cli::claude_theme_at(settings) {
        Ok(Some(t)) if t == crate::claude_theme::SELECTED => Answer::Good,
        Ok(t) => Answer::Needs(t.map_or("not set".to_string(), |t| format!("set to {t}"))),
        Err(e) => Answer::Broken(e),
    }
}

fn hook_answer(settings: &Path, exe: &Path) -> Answer {
    match hook_cli::hooks_installed_at(settings, exe) {
        Ok(true) => Answer::Good,
        Ok(false) => Answer::Needs("missing".into()),
        Err(e) => Answer::Broken(e),
    }
}

fn answer(id: CheckId, settings: &Path, exe: &Path) -> Option<Answer> {
    match id {
        CheckId::WheelScroll => Some(scroll_answer(settings)),
        CheckId::ClaudeTheme => Some(theme_answer(settings)),
        CheckId::TitleHook => Some(hook_answer(settings, exe)),
        CheckId::StartAtLogin | CheckId::AgentGuide => None,
    }
}

/// The settings files `id`'s action writes: those of the folders where the row needs its fix.
/// Empty for a row that reads no settings file.
pub fn needing(id: CheckId, settings: &[PathBuf], exe: &Path) -> Vec<PathBuf> {
    settings.iter().filter(|s| matches!(answer(id, s, exe), Some(Answer::Needs(_)))).cloned().collect()
}

/// A settings file's folder as a row names it.
fn folder_name(settings: &Path, home: Option<&Path>) -> String {
    crate::claude_folders::tilde(settings.parent().unwrap_or(settings), home)
}

/// One row over every folder: broken when any folder's file cannot be read (the error names the
/// file), a fix when any folder needs it, good otherwise. With one folder the fix reads
/// "<how it stands>: <why>", as it always has; with more it names the folders that need it,
/// "<how> in <folders>: <why_among>", folders that stand the same way sharing the word.
fn over_folders(p: &Probe, answers: Vec<Answer>, good: &str, why: &str, why_among: &str, button: &'static str) -> (State, Option<&'static str>) {
    let mut needs: Vec<(String, Vec<String>)> = Vec::new();
    for (settings, a) in p.settings.iter().zip(answers) {
        match a {
            Answer::Good => {}
            Answer::Broken(e) => return (State::Broken(e), None),
            Answer::Needs(how) => {
                let folder = folder_name(settings, p.home);
                match needs.iter_mut().find(|(h, _)| *h == how) {
                    Some((_, folders)) => folders.push(folder),
                    None => needs.push((how, vec![folder])),
                }
            }
        }
    }
    if needs.is_empty() {
        return (State::Good(good.into()), None);
    }
    let text = if p.settings.len() == 1 {
        format!("{}: {why}", needs[0].0)
    } else {
        let parts: Vec<String> = needs.iter().map(|(how, folders)| format!("{how} in {}", folders.join(", "))).collect();
        format!("{}: {why_among}", parts.join(", "))
    };
    (State::Fix(text), Some(button))
}

/// The checks, in the order the popover lists them. The three rows that read Claude Code's
/// settings read them in every folder of `p.settings`. In a sandbox every row is pinned as
/// `not in a sandbox` with no button: each one's action writes the user's own setup (their Claude
/// Code settings and colours, the login item, the hooks or the skill).
pub fn probe_all(p: &Probe, sandbox: bool) -> Vec<Check> {
    let (scroll_state, scroll_button) = over_folders(
        p,
        p.settings.iter().map(|s| scroll_answer(s)).collect(),
        if p.scroll_applied { "on; restart claude for it to take effect" } else { "on" },
        "fast scrolls jump in Claude Code",
        "fast scrolls jump in Claude Code there",
        "Apply",
    );
    let (theme_state, theme_button) = over_folders(
        p,
        p.settings.iter().map(|s| theme_answer(s)).collect(),
        "on: Claude Code takes the Arkos colours",
        "Claude Code keeps its own colours, unreadable on some Arkos themes",
        "Claude Code keeps its own colours there, unreadable on some Arkos themes",
        "Apply",
    );
    let (login_state, login_button) = match (p.start_at_login, p.login_item_installed) {
        (StartAtLogin::Ask, false) => (State::Fix("off: sessions do not come back after a restart".into()), Some("Install")),
        (StartAtLogin::Ask, true) => (State::Good("on".into()), Some("Remove")),
        (StartAtLogin::On, _) => (State::Pinned("on, set in config.toml ([recovery] start_at_login)".into()), None),
        (StartAtLogin::Off, _) => (State::Pinned("off, set in config.toml ([recovery] start_at_login)".into()), None),
    };
    let (hook_state, hook_button) = match over_folders(
        p,
        p.settings.iter().map(|s| hook_answer(s, p.exe)).collect(),
        "installed",
        "tabs show no Claude Code state",
        "tabs running there show no Claude Code state",
        "Install",
    ) {
        // Every folder has the hooks, and a Claude Code still reports nothing: it was started
        // before they were installed, and reads them only when it starts. Nothing to press.
        (State::Good(_), _) if !p.silent.is_empty() => {
            let tabs = if p.silent.len() == 1 { "the tab" } else { "the tabs" };
            (State::Fix(format!("installed; restart claude in {} for {tabs} to show its state", p.silent.join(", "))), None)
        }
        row => row,
    };
    let dir = p.skills.display();
    let (guide_state, guide_button) = match (&p.skill, p.claude_present) {
        (_, false) => (State::Pinned("no Claude Code config found".into()), None),
        (SkillState::Missing, true) => (State::Fix(format!("not installed in {dir}: agents cannot find the toolkit")), Some("Install")),
        (SkillState::Current, true) => (State::Good(format!("installed in {dir}; other Claude config folders get it when a session starts inside Arkos")), None),
        (SkillState::Outdated(old), true) => (State::Fix(format!("older than this build ({old})")), Some("Update")),
        (SkillState::Foreign, true) => (State::Broken(format!("a different skill sits at {}", p.skills.join(crate::guide::SKILL_NAME).join("SKILL.md").display())), None),
    };
    let mut checks = vec![
        Check { id: CheckId::WheelScroll, title: "Claude Code: smooth wheel scrolling", state: scroll_state, button: scroll_button },
        Check { id: CheckId::ClaudeTheme, title: "Claude Code: follows the Arkos theme", state: theme_state, button: theme_button },
        Check { id: CheckId::StartAtLogin, title: "Start at login", state: login_state, button: login_button },
        Check { id: CheckId::TitleHook, title: "Claude Code title hook", state: hook_state, button: hook_button },
        Check { id: CheckId::AgentGuide, title: "Agent guide for Claude Code", state: guide_state, button: guide_button },
    ];
    if sandbox {
        for c in checks.iter_mut() {
            c.state = State::Pinned(crate::sandbox::REFUSAL.into());
            c.button = None;
        }
    }
    checks
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

    fn probe<'a>(settings: &'a PathBuf, exe: &'a Path) -> Probe<'a> {
        Probe {
            settings: std::slice::from_ref(settings),
            home: None,
            silent: &[],
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
        let checks = probe_all(&probe(&settings, exe), false);
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
        crate::hook_cli::install_hooks_at(&settings, exe, false).unwrap();
        crate::hook_cli::set_wheel_scroll_acceleration_at(&settings, false).unwrap();
        crate::hook_cli::set_claude_theme_at(&settings, crate::claude_theme::SELECTED).unwrap();
        let mut p = probe(&settings, exe);
        p.login_item_installed = true;
        p.scroll_applied = true;
        let checks = probe_all(&p, false);
        assert_eq!(checks[0].state, State::Good("on; restart claude for it to take effect".into()));
        assert_eq!(checks[0].button, None);
        assert_eq!((checks[2].state.clone(), checks[2].button), (State::Good("on".into()), Some("Remove")));
        assert_eq!(checks[1].state, State::Good("on: Claude Code takes the Arkos colours".into()));
        assert_eq!(checks[3].state, State::Good("installed".into()));
        assert_eq!(needs_fix(&checks), 0);
        p.scroll_applied = false;
        assert_eq!(probe_all(&p, false)[0].state, State::Good("on".into()));
    }

    #[test]
    fn the_theme_row_offers_the_arkos_colours_for_any_other_claude_theme() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        let row = |s: &PathBuf| probe_all(&probe(s, Path::new("/x/arkos")), false).into_iter().find(|c| c.id == CheckId::ClaudeTheme).unwrap();
        for (theme, shown) in [("auto", "set to auto"), ("dark-daltonized", "set to dark-daltonized"), ("custom:mine", "set to custom:mine")] {
            crate::hook_cli::set_claude_theme_at(&settings, theme).unwrap();
            assert_eq!(row(&settings).state, State::Fix(format!("{shown}: Claude Code keeps its own colours, unreadable on some Arkos themes")));
            assert_eq!(row(&settings).button, Some("Apply"));
        }
        crate::hook_cli::set_claude_theme_at(&settings, "custom:arkos").unwrap();
        assert_eq!((row(&settings).state, row(&settings).button), (State::Good("on: Claude Code takes the Arkos colours".into()), None));
    }

    #[test]
    fn the_scroll_key_set_to_true_still_needs_the_fix() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        crate::hook_cli::set_wheel_scroll_acceleration_at(&settings, true).unwrap();
        let checks = probe_all(&probe(&settings, Path::new("/x/arkos")), false);
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
        let checks = probe_all(&p, false);
        assert_eq!(checks[2], Check { id: CheckId::StartAtLogin, title: "Start at login", state: State::Pinned("off, set in config.toml ([recovery] start_at_login)".into()), button: None });
        p.start_at_login = StartAtLogin::On;
        assert_eq!(probe_all(&p, false)[2].state, State::Pinned("on, set in config.toml ([recovery] start_at_login)".into()));
        assert_eq!(needs_fix(&probe_all(&p, false)), 3, "pinned rows never count; scroll, theme and hook do");
    }

    #[test]
    fn an_unreadable_settings_file_offers_no_action() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        std::fs::write(&settings, "{ not json").unwrap();
        let checks = probe_all(&probe(&settings, Path::new("/x/arkos")), false);
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
        let guide = |p: &Probe| probe_all(p, false).into_iter().find(|c| c.id == CheckId::AgentGuide).unwrap();
        assert_eq!(guide(&p).title, "Agent guide for Claude Code");
        assert_eq!(guide(&p).state, State::Good("installed in /h/.claude/skills; other Claude config folders get it when a session starts inside Arkos".into()));
        assert_eq!(guide(&p).button, None);
        let base = needs_fix(&probe_all(&p, false));
        p.skill = SkillState::Missing;
        assert_eq!(guide(&p).state, State::Fix("not installed in /h/.claude/skills: agents cannot find the toolkit".into()));
        assert_eq!(guide(&p).button, Some("Install"));
        assert_eq!(needs_fix(&probe_all(&p, false)), base + 1);
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
        assert_eq!(needs_fix(&probe_all(&p, false)), base, "no Claude Code, nothing to fix");
    }

    #[test]
    fn a_sandbox_pins_every_row_since_each_writes_the_users_setup() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        let p = probe(&settings, Path::new("/x/arkos"));
        let (outside, inside) = (probe_all(&p, false), probe_all(&p, true));
        assert_eq!(inside.iter().map(|c| c.id).collect::<Vec<_>>(), outside.iter().map(|c| c.id).collect::<Vec<_>>());
        for c in &inside {
            assert_eq!((&c.state, c.button), (&State::Pinned("not in a sandbox".into()), None), "{:?}", c.id);
        }
        assert_eq!(inside[0].title, "Claude Code: smooth wheel scrolling", "the scroll row too: it writes ~/.claude/settings.json");
        assert_eq!(needs_fix(&inside), 0, "nothing to fix in a sandbox");
    }

    /// Three config folders under one home, the default first, and their settings files.
    fn three_folders(home: &Path) -> Vec<PathBuf> {
        [".claude", ".claude2", ".claude3"].iter().map(|f| home.join(f).join("settings.json")).collect()
    }

    fn over<'a>(settings: &'a [PathBuf], home: &'a Path, exe: &'a Path, silent: &'a [String]) -> Probe<'a> {
        Probe { settings, home: Some(home), silent, exe, start_at_login: StartAtLogin::Ask, login_item_installed: true, scroll_applied: false, skills: Path::new("/h/.claude/skills"), skill: SkillState::Current, claude_present: true }
    }

    fn row(checks: &[Check], id: CheckId) -> (State, Option<&'static str>) {
        let c = checks.iter().find(|c| c.id == id).unwrap();
        (c.state.clone(), c.button)
    }

    #[test]
    fn a_folder_without_hooks_is_named_and_the_row_needs_a_fix() {
        let dir = tempfile::tempdir().unwrap();
        let exe = Path::new("/Applications/Arkos IDE.app/Contents/MacOS/arkos");
        let settings = three_folders(dir.path());
        crate::hook_cli::install_hooks_at(&settings[0], exe, false).unwrap();
        let checks = probe_all(&over(&settings, dir.path(), exe, &[]), false);
        assert_eq!(row(&checks, CheckId::TitleHook), (State::Fix("missing in ~/.claude2, ~/.claude3: tabs running there show no Claude Code state".into()), Some("Install")));
        assert_eq!(needing(CheckId::TitleHook, &settings, exe), settings[1..].to_vec(), "the action writes the two that lack them");
        crate::hook_cli::install_hooks_at(&settings[2], exe, false).unwrap();
        let checks = probe_all(&over(&settings, dir.path(), exe, &[]), false);
        assert_eq!(row(&checks, CheckId::TitleHook).0, State::Fix("missing in ~/.claude2: tabs running there show no Claude Code state".into()));
        crate::hook_cli::install_hooks_at(&settings[1], exe, false).unwrap();
        let checks = probe_all(&over(&settings, dir.path(), exe, &[]), false);
        assert_eq!(row(&checks, CheckId::TitleHook), (State::Good("installed".into()), None));
        assert!(needing(CheckId::TitleHook, &settings, exe).is_empty());
        assert!(needing(CheckId::StartAtLogin, &settings, exe).is_empty(), "a row that reads no settings file writes none");
    }

    #[test]
    fn the_default_folder_is_named_too_once_there_is_another() {
        let dir = tempfile::tempdir().unwrap();
        let exe = Path::new("/x/arkos");
        let settings = three_folders(dir.path())[..2].to_vec();
        crate::hook_cli::install_hooks_at(&settings[1], exe, false).unwrap();
        let checks = probe_all(&over(&settings, dir.path(), exe, &[]), false);
        assert_eq!(row(&checks, CheckId::TitleHook).0, State::Fix("missing in ~/.claude: tabs running there show no Claude Code state".into()));
    }

    #[test]
    fn the_scroll_and_theme_rows_name_the_folders_that_need_them() {
        let dir = tempfile::tempdir().unwrap();
        let exe = Path::new("/x/arkos");
        let settings = three_folders(dir.path());
        crate::hook_cli::set_wheel_scroll_acceleration_at(&settings[0], false).unwrap();
        crate::hook_cli::set_claude_theme_at(&settings[0], crate::claude_theme::SELECTED).unwrap();
        crate::hook_cli::set_claude_theme_at(&settings[1], "dark").unwrap();
        let checks = probe_all(&over(&settings, dir.path(), exe, &[]), false);
        assert_eq!(row(&checks, CheckId::WheelScroll), (State::Fix("off in ~/.claude2, ~/.claude3: fast scrolls jump in Claude Code there".into()), Some("Apply")));
        assert_eq!(row(&checks, CheckId::ClaudeTheme), (State::Fix("set to dark in ~/.claude2, not set in ~/.claude3: Claude Code keeps its own colours there, unreadable on some Arkos themes".into()), Some("Apply")));
        assert_eq!(needing(CheckId::WheelScroll, &settings, exe), settings[1..].to_vec());
        assert_eq!(needing(CheckId::ClaudeTheme, &settings, exe), settings[1..].to_vec());
        for s in &settings[1..] {
            crate::hook_cli::set_wheel_scroll_acceleration_at(s, false).unwrap();
            crate::hook_cli::set_claude_theme_at(s, crate::claude_theme::SELECTED).unwrap();
        }
        let checks = probe_all(&over(&settings, dir.path(), exe, &[]), false);
        assert_eq!(row(&checks, CheckId::WheelScroll), (State::Good("on".into()), None));
        assert_eq!(row(&checks, CheckId::ClaudeTheme), (State::Good("on: Claude Code takes the Arkos colours".into()), None));
    }

    #[test]
    fn one_unparseable_folder_breaks_its_rows_and_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let exe = Path::new("/x/arkos");
        let settings = three_folders(dir.path());
        std::fs::create_dir_all(settings[2].parent().unwrap()).unwrap();
        std::fs::write(&settings[2], "{ not json").unwrap();
        let checks = probe_all(&over(&settings, dir.path(), exe, &[]), false);
        for id in [CheckId::WheelScroll, CheckId::ClaudeTheme, CheckId::TitleHook] {
            let (state, button) = row(&checks, id);
            assert!(matches!(&state, State::Broken(t) if t.starts_with(&format!("cannot parse {}", settings[2].display()))), "{id:?}: {state:?}");
            assert_eq!(button, None, "{id:?}");
        }
        assert_eq!(needing(CheckId::TitleHook, &settings, exe), settings[..2].to_vec(), "the folders that can be written still are");
    }

    #[test]
    fn hooks_everywhere_and_a_silent_tab_asks_for_a_restart_with_no_button() {
        let dir = tempfile::tempdir().unwrap();
        let exe = Path::new("/x/arkos");
        let settings = three_folders(dir.path());
        for s in &settings {
            crate::hook_cli::install_hooks_at(s, exe, false).unwrap();
        }
        let one = ["Film".to_string()];
        let checks = probe_all(&over(&settings, dir.path(), exe, &one), false);
        assert_eq!(row(&checks, CheckId::TitleHook), (State::Fix("installed; restart claude in Film for the tab to show its state".into()), None));
        let before = needs_fix(&probe_all(&over(&settings, dir.path(), exe, &[]), false));
        assert_eq!(needs_fix(&checks), before + 1, "the doctor's dot shows for it");
        let two = ["Film".to_string(), "Game".to_string()];
        let checks = probe_all(&over(&settings, dir.path(), exe, &two), false);
        assert_eq!(row(&checks, CheckId::TitleHook).0, State::Fix("installed; restart claude in Film, Game for the tabs to show its state".into()));
        // A folder still without hooks comes first: the install, then the restart.
        std::fs::write(&settings[1], "{}").unwrap();
        let checks = probe_all(&over(&settings, dir.path(), exe, &two), false);
        assert_eq!(row(&checks, CheckId::TitleHook), (State::Fix("missing in ~/.claude2: tabs running there show no Claude Code state".into()), Some("Install")));
        assert_eq!(row(&probe_all(&over(&settings, dir.path(), exe, &two), true), CheckId::TitleHook), (State::Pinned("not in a sandbox".into()), None));
    }
}
