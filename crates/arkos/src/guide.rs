//! The agent guide (milestone 8b, spec sections 7 and 11): `docs/agent-guide.md` compiled in,
//! printed by `arkos guide`, and installed as a Claude Code skill. The skill goes into the one
//! Claude config folder of the environment this process runs in (`$CLAUDE_CONFIG_DIR/skills`,
//! else `~/.claude/skills`): the SessionStart hook runs inside the starting Claude Code, so that
//! is the folder of the Claude Code that will load it. No other folder is ever looked at.

use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The guide's text, as `docs/agent-guide.md` holds it.
pub const TEXT: &str = include_str!("../../../docs/agent-guide.md");
/// The skill's folder name and its front matter `name`.
pub const SKILL_NAME: &str = "arkos-ide";
/// The skill's front matter `description`, which is how an agent decides to load it.
pub const SKILL_DESCRIPTION: &str = "Show a video or a markdown file in an Arkos IDE pane, control its playback, or customize Arkos: themes, fonts and settings. Use when the user asks to see, preview, play or watch a video or a document, or to change how Arkos looks or behaves.";
/// The line `arkos guide --install` offers for `AGENTS.md` or `CLAUDE.md`.
pub const AGENTS_MD_LINE: &str = "Arkos IDE: run `arkos guide` before showing the user a file in a pane or changing its theme or settings.";

const USAGE: &str = "usage: arkos guide [--path | --install]\n";
const STAMP_PREFIX: &str = "<!-- arkos-guide: ";
const STAMP_SUFFIX: &str = " -->";

/// The skill's YAML front matter with its closing `---` line. The description is quoted: it holds
/// a second ": ", which YAML refuses in a plain value (it has no `"` or `\` to escape).
fn front_matter() -> String {
    format!("---\nname: {SKILL_NAME}\ndescription: \"{SKILL_DESCRIPTION}\"\n---\n")
}

/// The first 16 hex digits of the SHA-256 of the front matter followed by `TEXT` (everything in
/// `SKILL.md` but the stamp line itself): the version an installed skill carries, so a change to
/// either part gives a new stamp.
pub fn stamp() -> String {
    let mut h = Sha256::new();
    h.update(front_matter().as_bytes());
    h.update(TEXT.as_bytes());
    h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// The whole `SKILL.md`: the front matter, the stamp line, then the guide.
pub fn skill_text() -> String {
    format!("{}{STAMP_PREFIX}{}{STAMP_SUFFIX}\n{TEXT}", front_matter(), stamp())
}

/// What a skills folder holds under `arkos-ide/SKILL.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillState {
    /// No file there.
    Missing,
    /// Exactly what this build would write.
    Current,
    /// Ours from another build; the stamp it carries (`unstamped` when it has none).
    Outdated(String),
    /// A file that is not ours (its first lines are not `---` and `name: arkos-ide`), or one that
    /// cannot be read; it is never written over.
    Foreign,
}

/// Where the skill lives inside a skills folder.
fn skill_path(dir: &Path) -> PathBuf {
    dir.join(SKILL_NAME).join("SKILL.md")
}

/// Reads one skills folder; nothing is written.
pub fn skill_state(dir: &Path) -> SkillState {
    let text = match std::fs::read_to_string(skill_path(dir)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return SkillState::Missing,
        Err(_) => return SkillState::Foreign,
    };
    let mut lines = text.lines();
    if lines.next() != Some("---") || lines.next() != Some(&format!("name: {SKILL_NAME}")) {
        return SkillState::Foreign;
    }
    if text == skill_text() {
        return SkillState::Current;
    }
    let old = text.lines().find_map(|l| l.strip_prefix(STAMP_PREFIX)?.strip_suffix(STAMP_SUFFIX)).unwrap_or("unstamped");
    SkillState::Outdated(old.to_string())
}

/// What an install wrote: the file, and whether it replaced one of ours.
#[derive(Debug)]
pub struct Installed {
    pub path: PathBuf,
    pub replaced: bool,
}

/// Writes `<dir>/arkos-ide/SKILL.md` (through a temporary file and a rename, so a Claude Code
/// reading it never sees half a file). Refuses, naming the file, when a skill that is not ours
/// is there.
pub fn install_skill(dir: &Path) -> Result<Installed, String> {
    let path = skill_path(dir);
    let replaced = match skill_state(dir) {
        SkillState::Foreign => return Err(format!("{} is a different skill or cannot be read; leaving it alone", path.display())),
        SkillState::Missing => false,
        SkillState::Current | SkillState::Outdated(_) => true,
    };
    let folder = dir.join(SKILL_NAME);
    std::fs::create_dir_all(&folder).map_err(|e| format!("cannot create {}: {e}", folder.display()))?;
    let tmp = folder.join(format!(".SKILL.md.{}", std::process::id()));
    std::fs::write(&tmp, skill_text()).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("cannot write {}: {e}", path.display())
    })?;
    Ok(Installed { path, replaced })
}

/// The hook's quiet refresh: installs when the skill is missing or stale, leaves a current one
/// alone (`None`), and refuses a stranger as `install_skill` does.
pub fn ensure_skill(dir: &Path) -> Result<Option<Installed>, String> {
    match skill_state(dir) {
        SkillState::Current => Ok(None),
        _ => install_skill(dir).map(Some),
    }
}

/// `<app_support>/agent-guide.md`, written only when it is missing or differs from `TEXT`.
pub fn write_copy(app_support: &Path) -> Result<PathBuf, String> {
    let path = app_support.join("agent-guide.md");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(TEXT) {
        std::fs::create_dir_all(app_support).map_err(|e| format!("cannot create {}: {e}", app_support.display()))?;
        std::fs::write(&path, TEXT).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    Ok(path)
}

/// `$CLAUDE_CONFIG_DIR/skills` when the variable is set and not empty, else `$HOME/.claude/skills`;
/// `None` when neither is set (an empty value counts as unset).
pub fn skills_dir_from(config_dir_env: Option<&OsStr>, home_env: Option<&OsStr>) -> Option<PathBuf> {
    match (config_dir_env, home_env) {
        (Some(dir), _) if !dir.is_empty() => Some(PathBuf::from(dir).join("skills")),
        (_, Some(home)) if !home.is_empty() => Some(Path::new(home).join(".claude").join("skills")),
        _ => None,
    }
}

/// The skills folder of the Claude Code whose environment this process runs in, if it names one.
/// The hook and `arkos guide` use this and say so when it is `None`.
pub fn find_skills_dir() -> Option<PathBuf> {
    skills_dir_from(std::env::var_os("CLAUDE_CONFIG_DIR").as_deref(), std::env::var_os("HOME").as_deref())
}

/// `find_skills_dir` for the window and the doctor, which always run with `HOME` set.
pub fn skills_dir() -> PathBuf {
    find_skills_dir().expect("HOME is set")
}

/// `arkos guide [--path | --install]` with its folders and streams explicit, for tests. Exit 0
/// on success, 1 when a write fails, a stranger's skill is in the way or `--install` has no skills
/// folder (`None`), 2 for a usage error. `--install` leaves a current skill as it is.
pub fn guide_cli_to(args: &[String], skills_dir: Option<&Path>, app_support: &Path, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] => {
            let _ = out.write_all(TEXT.as_bytes());
            0
        }
        ["--path"] => match write_copy(app_support) {
            Ok(path) => {
                let _ = writeln!(out, "{}", path.display());
                0
            }
            Err(e) => {
                let _ = writeln!(err, "arkos guide: {e}");
                1
            }
        },
        ["--install"] => match install_line(skills_dir) {
            Ok(line) => {
                let _ = writeln!(out, "{line}");
                let _ = writeln!(out, "For agents that read AGENTS.md or CLAUDE.md, add this line there:");
                let _ = writeln!(out, "{AGENTS_MD_LINE}");
                0
            }
            Err(e) => {
                let _ = writeln!(err, "arkos guide: {e}");
                1
            }
        },
        _ => {
            let _ = err.write_all(USAGE.as_bytes());
            2
        }
    }
}

/// `--install`'s first line: `already current: {path}` with nothing written, else the install's
/// `installed` or `updated the skill at {path}; ...`.
fn install_line(skills_dir: Option<&Path>) -> Result<String, String> {
    let dir = skills_dir.ok_or("cannot find the Claude config folder: neither CLAUDE_CONFIG_DIR nor HOME is set")?;
    if skill_state(dir) == SkillState::Current {
        return Ok(format!("already current: {}", skill_path(dir).display()));
    }
    let i = install_skill(dir)?;
    let verb = if i.replaced { "updated" } else { "installed" };
    Ok(format!("{verb} the skill at {}; Claude Code loads it at its next start", i.path.display()))
}

/// `arkos guide ...` against this environment's folders and the process streams.
pub fn guide_cli(args: &[String]) -> i32 {
    guide_cli_to(args, find_skills_dir().as_deref(), &crate::paths::app_support(), &mut std::io::stdout(), &mut std::io::stderr())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skill_is_installed_once_refreshed_when_stale_and_never_over_a_stranger() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(skill_state(dir.path()), SkillState::Missing);
        let i = install_skill(dir.path()).unwrap();
        assert_eq!(i.path, dir.path().join("arkos-ide").join("SKILL.md"));
        assert!(!i.replaced);
        let text = std::fs::read_to_string(&i.path).unwrap();
        assert!(text.starts_with("---\nname: arkos-ide\ndescription: \"Show a video or a markdown file in an Arkos IDE pane"), "{text}");
        assert!(text.contains(&format!("<!-- arkos-guide: {} -->\n", stamp())));
        assert!(text.ends_with(TEXT));
        assert_eq!(skill_state(dir.path()), SkillState::Current);
        let stale = text.replace(&stamp(), "0000000000000000");
        std::fs::write(&i.path, &stale).unwrap();
        assert_eq!(skill_state(dir.path()), SkillState::Outdated("0000000000000000".into()));
        assert!(install_skill(dir.path()).unwrap().replaced);
        std::fs::write(&i.path, "---\nname: other\n---\nhi").unwrap();
        assert_eq!(skill_state(dir.path()), SkillState::Foreign);
        let e = install_skill(dir.path()).unwrap_err();
        assert!(e.contains("SKILL.md"), "{e}");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(guide_cli_to(&[], Some(dir.path()), dir.path(), &mut out, &mut err), 0);
        assert_eq!(String::from_utf8(out).unwrap(), TEXT);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(guide_cli_to(&["--path".into()], Some(dir.path()), dir.path(), &mut out, &mut err), 0);
        let p = PathBuf::from(String::from_utf8(out).unwrap().trim());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), TEXT);
        let mtime = std::fs::metadata(&p).unwrap().modified().unwrap();
        assert_eq!(guide_cli_to(&["--path".into()], Some(dir.path()), dir.path(), &mut Vec::new(), &mut Vec::new()), 0);
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), mtime, "an unchanged copy is not rewritten");
        std::fs::write(&p, "an older guide").unwrap();
        assert_eq!(guide_cli_to(&["--path".into()], Some(dir.path()), dir.path(), &mut Vec::new(), &mut Vec::new()), 0);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), TEXT, "a copy that differs is rewritten");
        assert_eq!(guide_cli_to(&["--install".into()], Some(dir.path()), dir.path(), &mut Vec::new(), &mut Vec::new()), 1, "the stranger stays");
        assert_eq!(skills_dir_from(Some(OsStr::new("/h/.claude2")), Some(OsStr::new("/h"))), Some(PathBuf::from("/h/.claude2/skills")));
        assert_eq!(skills_dir_from(None, Some(OsStr::new("/h"))), Some(PathBuf::from("/h/.claude/skills")));
        assert_eq!(skills_dir_from(Some(OsStr::new("/c")), None), Some(PathBuf::from("/c/skills")), "a config folder needs no HOME");
        assert_eq!(skills_dir_from(None, None), None);
        assert_eq!(skills_dir_from(Some(OsStr::new("")), Some(OsStr::new(""))), None, "empty values are unset");
        let fresh = tempfile::tempdir().unwrap();
        assert!(ensure_skill(fresh.path()).unwrap().is_some(), "missing: installed");
        assert!(ensure_skill(fresh.path()).unwrap().is_none(), "current: left alone");
        assert_eq!(skill_state(fresh.path()), SkillState::Current);
        assert!(!TEXT.contains('\u{2014}'));
        assert!(TEXT.lines().count() > 100);
        for cmd in ["arkos theme template", "arkos theme contrast", "arkos theme preview", "arkos theme use", "arkos theme current", "arkos settings set", "arkos guide"] {
            assert!(TEXT.contains(cmd), "the guide names {cmd}");
        }
    }

    #[test]
    fn the_guide_names_every_open_video_and_panes_command_and_flag() {
        let named = [
            "arkos open", "arkos video status", "arkos video play", "arkos video pause", "arkos video seek", "arkos video step", "arkos video loop", "arkos video reload",
            "arkos video close", "arkos video mute", "arkos video unmute", "arkos panes", "--pane", "--split", "--at", "--paused", "--playing", "--loop", "--loop off", "--raw",
            "--new", "--wait", "--focus", "--window", "--all", "ARKOS_SESSION_ID",
        ];
        for s in named {
            assert!(TEXT.contains(s), "the guide names {s}");
        }
        // Every usage line the commands print is in the guide as printed.
        for usage in [crate::open_cli::OPEN_USAGE, crate::open_cli::VIDEO_USAGE, crate::open_cli::PANES_USAGE] {
            for line in usage.lines().filter(|l| l.contains("arkos ")) {
                let form = line.trim_start_matches("usage:").trim();
                assert!(TEXT.contains(form), "the guide gives the form {form}");
            }
        }
        assert!(SKILL_DESCRIPTION.contains("video"), "{SKILL_DESCRIPTION}");
        assert!(SKILL_DESCRIPTION.contains(": "), "it stays quoted in the front matter");
    }

    #[test]
    fn the_agents_md_line_names_showing_a_file_as_well_as_the_theme() {
        assert_eq!(AGENTS_MD_LINE, "Arkos IDE: run `arkos guide` before showing the user a file in a pane or changing its theme or settings.");
        assert!(TEXT.contains(AGENTS_MD_LINE), "the guide quotes the line as printed");
    }

    #[test]
    fn the_stamp_is_sixteen_lowercase_hex_digits() {
        let s = stamp();
        assert_eq!(s.len(), 16);
        assert!(s.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()), "{s}");
        assert_eq!(stamp(), s, "stable");
    }

    #[test]
    fn install_prints_the_path_and_the_line_for_agents_md() {
        let dir = tempfile::tempdir().unwrap();
        let mut out = Vec::new();
        assert_eq!(guide_cli_to(&["--install".into()], Some(dir.path()), dir.path(), &mut out, &mut Vec::new()), 0);
        let path = dir.path().join("arkos-ide").join("SKILL.md");
        let want = format!("installed the skill at {}; Claude Code loads it at its next start\nFor agents that read AGENTS.md or CLAUDE.md, add this line there:\n{AGENTS_MD_LINE}\n", path.display());
        assert_eq!(String::from_utf8(out).unwrap(), want);
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let mut out = Vec::new();
        assert_eq!(guide_cli_to(&["--install".into()], Some(dir.path()), dir.path(), &mut out, &mut Vec::new()), 0);
        let want = format!("already current: {}\nFor agents that read AGENTS.md or CLAUDE.md, add this line there:\n{AGENTS_MD_LINE}\n", path.display());
        assert_eq!(String::from_utf8(out).unwrap(), want);
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), mtime, "a current skill is not rewritten");
        std::fs::write(&path, skill_text().replace(&stamp(), "0000000000000000")).unwrap();
        let mut out = Vec::new();
        assert_eq!(guide_cli_to(&["--install".into()], Some(dir.path()), dir.path(), &mut out, &mut Vec::new()), 0);
        assert!(String::from_utf8(out).unwrap().starts_with("updated the skill at "), "only an outdated skill is updated");
        assert_eq!(skill_state(dir.path()), SkillState::Current);
        let mut err = Vec::new();
        assert_eq!(guide_cli_to(&["--install".into()], None, dir.path(), &mut Vec::new(), &mut err), 1);
        assert_eq!(String::from_utf8(err).unwrap(), "arkos guide: cannot find the Claude config folder: neither CLAUDE_CONFIG_DIR nor HOME is set\n");
        assert_eq!(guide_cli_to(&[], None, dir.path(), &mut Vec::new(), &mut Vec::new()), 0, "printing the guide needs no skills folder");
        let mut err = Vec::new();
        assert_eq!(guide_cli_to(&["--bogus".into()], Some(dir.path()), dir.path(), &mut Vec::new(), &mut err), 2);
        assert_eq!(String::from_utf8(err).unwrap(), USAGE);
        assert!(skills_dir_from(Some(OsStr::new("")), Some(OsStr::new("/h"))).unwrap().ends_with(".claude/skills"), "an empty variable is unset");
        assert!(std::fs::read_dir(dir.path().join("arkos-ide")).unwrap().all(|e| e.unwrap().file_name() == "SKILL.md"), "no temporary file left");
    }

    #[test]
    fn the_front_matter_is_yaml_a_skill_loader_accepts() {
        let text = skill_text();
        let body = text.strip_prefix("---\n").unwrap();
        let (front, _) = body.split_once("\n---\n").unwrap();
        for line in front.lines() {
            let (key, value) = line.split_once(": ").unwrap_or_else(|| panic!("not a key: value line: {line}"));
            assert!(key.chars().all(|c| c.is_ascii_lowercase()), "{line}");
            match value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                Some(inner) => assert!(!inner.contains('"') && !inner.contains('\\'), "nothing to escape inside the quotes: {line}"),
                None => assert!(!value.contains(": ") && !value.starts_with(['"', '\'', '{', '[', '&', '*', '!', '|', '>', '%', '@', '`']), "a plain value YAML would refuse: {line}"),
            }
        }
        assert_eq!(front.lines().map(|l| l.split_once(": ").unwrap().0).collect::<Vec<_>>(), ["name", "description"]);
    }

    #[test]
    fn the_stamp_covers_the_front_matter_as_well_as_the_guide() {
        let mut h = Sha256::new();
        h.update(TEXT.as_bytes());
        let text_only: String = h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect();
        assert_ne!(stamp(), text_only, "the front matter is hashed too");
        let skill = skill_text();
        let without_stamp = skill.replace(&format!("{STAMP_PREFIX}{}{STAMP_SUFFIX}\n", stamp()), "");
        let mut h = Sha256::new();
        h.update(without_stamp.as_bytes());
        let whole: String = h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect();
        assert_eq!(stamp(), whole, "everything in SKILL.md but the stamp line");
    }
}
