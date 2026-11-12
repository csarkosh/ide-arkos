//! The Claude Code config folders Arkos knows (spec 2026-11-12, section 1): the default one,
//! then every other folder a Claude Code inside an Arkos session was seen to run under
//! (`agent_probe`), remembered one absolute path a line in `claude-folders`. The doctor's Claude
//! Code rows read each folder's settings file, and their actions write each one that needs it.

use std::path::{Path, PathBuf};

/// The folder Claude Code uses when its environment names none: this process's
/// `CLAUDE_CONFIG_DIR`, else `~/.claude` (the parent of `hook_cli::claude_settings_path`).
pub fn default_folder() -> PathBuf {
    crate::hook_cli::claude_settings_path().parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".claude"))
}

/// A folder's settings file.
pub fn settings_of(folder: &Path) -> PathBuf {
    folder.join("settings.json")
}

/// The remembered folders in `text`, in its order: absolute paths, each once, without `default`.
fn remembered(text: &str, default: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for line in text.lines() {
        let path = PathBuf::from(line.trim());
        if path.is_absolute() && path != default && !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

/// The list: `default` first, then the folders `file` remembers that are still directories. A
/// missing or unreadable file is the default alone.
pub fn read(file: &Path, default: &Path) -> Vec<PathBuf> {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let mut list = vec![default.to_path_buf()];
    list.extend(remembered(&text, default).into_iter().filter(|p| p.is_dir()));
    list
}

/// Adds `found` to the folders `file` remembers and returns the list as `read` would now give
/// it. The file is written whole, through a temporary name and a rename, and only when what it
/// should hold differs from what it holds: a folder that is gone is dropped then, and `default`
/// is never written. Two windows writing at once each write what they read and what they found,
/// so a lost race costs nothing the next probe does not put back.
pub fn add(file: &Path, default: &Path, found: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let mut keep: Vec<PathBuf> = remembered(&text, default).into_iter().filter(|p| p.is_dir()).collect();
    for f in found {
        if f.is_absolute() && f != default && f.is_dir() && !keep.contains(f) {
            keep.push(f.clone());
        }
    }
    let want: String = keep.iter().map(|p| format!("{}\n", p.display())).collect();
    if want != text {
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        let tmp = file.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&tmp, &want).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, file).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    }
    let mut list = vec![default.to_path_buf()];
    list.extend(keep);
    Ok(list)
}

/// `path` as a row shows it: the home folder written `~`.
pub fn tilde(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// The folder of a Claude Code whose environment names none: `~/.claude`. It is `default_folder`
/// unless this process was itself started with a `CLAUDE_CONFIG_DIR`.
pub fn home_default() -> PathBuf {
    home().unwrap_or_default().join(".claude")
}

/// The user's home folder, for `tilde`.
pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_folder_is_first_and_is_never_written() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("claude-folders");
        let (default, second) = (dir.path().join(".claude"), dir.path().join(".claude2"));
        std::fs::create_dir_all(&second).unwrap();
        assert_eq!(read(&file, &default), vec![default.clone()], "no file: the default alone, whether or not it exists");
        let list = add(&file, &default, &[default.clone(), second.clone()]).unwrap();
        assert_eq!(list, vec![default.clone(), second.clone()]);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), format!("{}\n", second.display()));
        assert_eq!(read(&file, &default), list);
    }

    #[test]
    fn a_folder_that_is_gone_is_skipped_and_dropped_at_the_next_write() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("claude-folders");
        let default = dir.path().join(".claude");
        let (gone, kept, new) = (dir.path().join("gone"), dir.path().join("kept"), dir.path().join("new"));
        std::fs::create_dir_all(&kept).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(&file, format!("{}\n{}\nnot/absolute\n\n", gone.display(), kept.display())).unwrap();
        assert_eq!(read(&file, &default), vec![default.clone(), kept.clone()]);
        assert!(std::fs::read_to_string(&file).unwrap().contains("gone"), "reading writes nothing");
        assert_eq!(add(&file, &default, &[new.clone()]).unwrap(), vec![default.clone(), kept.clone(), new.clone()]);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), format!("{}\n{}\n", kept.display(), new.display()));
        assert_eq!(add(&file, &default, &[gone.clone()]).unwrap().len(), 3, "a folder that does not exist is not remembered");
    }

    #[test]
    fn a_folder_is_listed_once_and_an_unchanged_list_is_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub").join("claude-folders");
        let default = dir.path().join(".claude");
        let second = dir.path().join(".claude2");
        std::fs::create_dir_all(&second).unwrap();
        assert_eq!(add(&file, &default, &[]).unwrap(), vec![default.clone()]);
        assert!(!file.exists(), "nothing to remember: no file made");
        add(&file, &default, &[second.clone(), second.clone()]).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap().lines().count(), 1);
        let written = std::fs::metadata(&file).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(add(&file, &default, &[second.clone()]).unwrap(), vec![default, second]);
        assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), written, "not written again");
        assert_eq!(std::fs::read_dir(file.parent().unwrap()).unwrap().count(), 1, "no temporary file left");
    }

    #[test]
    fn a_row_writes_the_home_folder_as_a_tilde() {
        let home = Path::new("/Users/me");
        assert_eq!(tilde(Path::new("/Users/me/.claude2"), Some(home)), "~/.claude2");
        assert_eq!(tilde(Path::new("/Users/me"), Some(home)), "~");
        assert_eq!(tilde(Path::new("/opt/claude"), Some(home)), "/opt/claude");
        assert_eq!(tilde(Path::new("/Users/me/.claude"), None), "/Users/me/.claude");
        assert_eq!(settings_of(Path::new("/Users/me/.claude2")), Path::new("/Users/me/.claude2/settings.json"));
    }
}
