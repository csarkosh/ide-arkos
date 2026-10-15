//! Two small logs for when an update goes wrong. The update journal (`updates/journal.log`):
//! one timestamped line per step of a check, an apply and its confirmation or rollback, written
//! by the daemon whatever its stderr is (a daemon a window started inherits nothing useful), and
//! rotated to `journal.log.1` at `JOURNAL_CAP`. And `window.log` next to the socket: the stdout
//! and stderr of every window the daemon launches, rotated to `window.log.1` at a launch once
//! over `WINDOW_LOG_CAP`, so the output of the window that died before it is kept. `arkos
//! doctor` prints the end of both.
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const JOURNAL_CAP: u64 = 200 * 1024;
pub const WINDOW_LOG_CAP: u64 = 1024 * 1024;

struct Paths {
    journal: PathBuf,
    window_log: PathBuf,
}

static PATHS: OnceLock<Paths> = OnceLock::new();
/// One writer at a time, so a rotation never splits a line.
static WRITING: Mutex<()> = Mutex::new(());

pub fn journal_path(updates_dir: &Path) -> PathBuf {
    updates_dir.join("journal.log")
}

pub fn window_log_path(socket: &Path) -> PathBuf {
    socket.with_file_name("window.log")
}

/// Sets where both logs go, once per process, before anything is noted. Until then (the unit
/// tests) `note` writes nothing and windows are launched with no stdio.
pub fn init(updates_dir: &Path, socket: &Path) {
    let _ = PATHS.set(Paths { journal: journal_path(updates_dir), window_log: window_log_path(socket) });
}

/// Appends one timestamped line to the journal. Never creates the updates folder (the poll
/// makes it, private): before the first check there is nothing of an update to record. Cheap:
/// one small append, and a rename when the cap is reached.
pub fn note(line: impl std::fmt::Display) {
    let Some(p) = PATHS.get() else { return };
    if !p.journal.parent().is_some_and(Path::is_dir) {
        return;
    }
    let _one = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = append_rotating(&p.journal, JOURNAL_CAP, &format!("{} {line}\n", timestamp())) {
        eprintln!("arkosd: cannot write the update journal {}: {e}", p.journal.display());
    }
}

/// Appends `text` to `path`, first moving a file of `cap` bytes or more to `<path>.1` (replacing
/// the one there), so the two together keep between one and two caps of history.
pub fn append_rotating(path: &Path, cap: u64, text: &str) -> std::io::Result<()> {
    if std::fs::metadata(path).is_ok_and(|m| m.len() >= cap) {
        std::fs::rename(path, rotated(path))?;
    }
    OpenOptions::new().create(true).append(true).open(path)?.write_all(text.as_bytes())
}

/// `journal.log.1` for `journal.log`.
pub fn rotated(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".1");
    PathBuf::from(s)
}

/// `window.log`, opened to append (O_APPEND, so windows writing at once never overwrite each
/// other), for a window's stdout and stderr. None before `init`.
pub fn open_window_log() -> Option<File> {
    let p = PATHS.get()?;
    match open_capped(&p.window_log, WINDOW_LOG_CAP) {
        Ok(f) => Some(f),
        Err(e) => {
            eprintln!("arkosd: cannot open {}: {e}", p.window_log.display());
            None
        }
    }
}

/// Opens `path` to append, first moving it to `<path>.1` (replacing the one there) when it is
/// over `cap` bytes. Never emptied in place: the end of it may be why this launch happens.
pub fn open_capped(path: &Path, cap: u64) -> std::io::Result<File> {
    if std::fs::metadata(path).is_ok_and(|m| m.len() > cap) {
        std::fs::rename(path, rotated(path))?;
    }
    OpenOptions::new().create(true).append(true).open(path)
}

/// Local time with its offset, `2026-10-15 08:04:12 -0400`, to match what the user saw.
fn timestamp() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as libc::time_t;
    // SAFETY: zeroed is a valid `tm`; localtime_r is reentrant and gets valid pointers.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&now, &mut tm) }.is_null() {
        return format!("@{now}");
    }
    let off = tm.tm_gmtoff / 60;
    let sign = if off < 0 { '-' } else { '+' };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} {sign}{:02}{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        off.abs() / 60,
        off.abs() % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_journal_rotates_to_dot_one_at_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = journal_path(dir.path());
        for i in 0..10 {
            append_rotating(&path, 50, &format!("line {i:02} of the journal\n")).unwrap();
        }
        let current = std::fs::read_to_string(&path).unwrap();
        let old = std::fs::read_to_string(rotated(&path)).unwrap();
        assert!(current.len() < 50 + 24, "{current:?}");
        assert!(current.ends_with("line 09 of the journal\n"), "{current:?}");
        assert!(old.lines().all(|l| l.starts_with("line ")), "whole lines only: {old:?}");
        assert_eq!(rotated(&path).file_name().unwrap(), "journal.log.1");
    }

    #[test]
    fn the_window_log_appends_and_rotates_once_over_its_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("window.log");
        open_capped(&path, 10).unwrap().write_all(b"first\n").unwrap();
        open_capped(&path, 10).unwrap().write_all(b"second\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first\nsecond\n", "appended");
        open_capped(&path, 10).unwrap().write_all(b"third\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "third\n", "over the cap: a new file");
        assert_eq!(std::fs::read_to_string(rotated(&path)).unwrap(), "first\nsecond\n", "the old output is kept");
        std::fs::write(&path, "0123456789ab\n").unwrap();
        open_capped(&path, 10).unwrap();
        assert_eq!(std::fs::read_to_string(rotated(&path)).unwrap(), "0123456789ab\n", "the older .1 is replaced");
    }

    #[test]
    fn a_timestamp_is_local_time_with_its_offset() {
        let t = timestamp();
        assert_eq!(t.len(), "2026-10-15 08:04:12 -0400".len(), "{t}");
        assert!(t.starts_with("20"), "{t}");
    }
}
