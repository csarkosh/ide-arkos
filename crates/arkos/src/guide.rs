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
pub const SKILL_DESCRIPTION: &str = "Customize Arkos IDE: themes, fonts and settings. Use when the user asks to change how Arkos looks or behaves.";
/// The line `arkos guide --install` offers for `AGENTS.md` or `CLAUDE.md`.
pub const AGENTS_MD_LINE: &str = "Arkos IDE: run `arkos guide` before changing its theme or settings.";

const USAGE: &str = "usage: arkos guide [--path | --install]\n";
const STAMP_PREFIX: &str = "<!-- arkos-guide: ";
const STAMP_SUFFIX: &str = " -->";

/// The first 16 hex digits of the SHA-256 of `TEXT`: the version an installed skill carries.
pub fn stamp() -> String {
    Sha256::digest(TEXT.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// The whole `SKILL.md`: the front matter, the stamp line, then the guide.
pub fn skill_text() -> String {
    format!("---\nname: {SKILL_NAME}\ndescription: {SKILL_DESCRIPTION}\n---\n{STAMP_PREFIX}{}{STAMP_SUFFIX}\n{TEXT}", stamp())
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

/// `$CLAUDE_CONFIG_DIR/skills` when the variable is set and not empty, else `~/.claude/skills`.
pub fn skills_dir_from(config_dir_env: Option<&OsStr>, home: &Path) -> PathBuf {
    match config_dir_env {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir).join("skills"),
        _ => home.join(".claude").join("skills"),
    }
}

/// The skills folder of the Claude Code whose environment this process runs in.
pub fn skills_dir() -> PathBuf {
    let home = PathBuf::from(std::env::var("HOME").expect("HOME is set"));
    skills_dir_from(std::env::var_os("CLAUDE_CONFIG_DIR").as_deref(), &home)
}

/// `arkos guide [--path | --install]` with its folders and streams explicit, for tests. Exit 0
/// on success, 1 when a write fails or a stranger's skill is in the way, 2 for a usage error.
pub fn guide_cli_to(args: &[String], skills_dir: &Path, app_support: &Path, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
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
        ["--install"] => match install_skill(skills_dir) {
            Ok(i) => {
                let verb = if i.replaced { "updated" } else { "installed" };
                let _ = writeln!(out, "{verb} the skill at {}; Claude Code loads it at its next start", i.path.display());
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

/// `arkos guide ...` against this environment's folders and the process streams.
pub fn guide_cli(args: &[String]) -> i32 {
    guide_cli_to(args, &skills_dir(), &crate::paths::app_support(), &mut std::io::stdout(), &mut std::io::stderr())
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
        assert!(text.starts_with("---\nname: arkos-ide\ndescription: Customize Arkos IDE"), "{text}");
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
        assert_eq!(guide_cli_to(&[], dir.path(), dir.path(), &mut out, &mut err), 0);
        assert_eq!(String::from_utf8(out).unwrap(), TEXT);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(guide_cli_to(&["--path".into()], dir.path(), dir.path(), &mut out, &mut err), 0);
        let p = PathBuf::from(String::from_utf8(out).unwrap().trim());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), TEXT);
        assert_eq!(guide_cli_to(&["--install".into()], dir.path(), dir.path(), &mut Vec::new(), &mut Vec::new()), 1, "the stranger stays");
        assert_eq!(skills_dir_from(Some(std::ffi::OsStr::new("/h/.claude2")), Path::new("/h")), PathBuf::from("/h/.claude2/skills"));
        assert_eq!(skills_dir_from(None, Path::new("/h")), PathBuf::from("/h/.claude/skills"));
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
    fn the_stamp_is_sixteen_hex_digits_of_the_guide_s_hash() {
        let s = stamp();
        assert_eq!(s.len(), 16);
        assert!(s.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()), "{s}");
        assert_eq!(stamp(), s, "stable");
    }

    #[test]
    fn install_prints_the_path_and_the_line_for_agents_md() {
        let dir = tempfile::tempdir().unwrap();
        let mut out = Vec::new();
        assert_eq!(guide_cli_to(&["--install".into()], dir.path(), dir.path(), &mut out, &mut Vec::new()), 0);
        let path = dir.path().join("arkos-ide").join("SKILL.md");
        let want = format!("installed the skill at {}; Claude Code loads it at its next start\nFor agents that read AGENTS.md or CLAUDE.md, add this line there:\n{AGENTS_MD_LINE}\n", path.display());
        assert_eq!(String::from_utf8(out).unwrap(), want);
        let mut out = Vec::new();
        assert_eq!(guide_cli_to(&["--install".into()], dir.path(), dir.path(), &mut out, &mut Vec::new()), 0);
        assert!(String::from_utf8(out).unwrap().starts_with("updated the skill at "));
        let mut err = Vec::new();
        assert_eq!(guide_cli_to(&["--bogus".into()], dir.path(), dir.path(), &mut Vec::new(), &mut err), 2);
        assert_eq!(String::from_utf8(err).unwrap(), USAGE);
        assert!(skills_dir_from(Some(OsStr::new("")), Path::new("/h")).ends_with(".claude/skills"), "an empty variable is unset");
        assert!(std::fs::read_dir(dir.path().join("arkos-ide")).unwrap().all(|e| e.unwrap().file_name() == "SKILL.md"), "no temporary file left");
    }
}
