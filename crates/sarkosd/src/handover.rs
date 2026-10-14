//! The handover file (spec section 2): everything the new daemon image needs to rebuild the
//! session table after `execve`, written by the old image just before. Descriptor numbers are
//! carried as numbers because the descriptors themselves pass through the exec. The format
//! version gates a resume; `written_by` is for the log.
use protocol::ClaudeState;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

pub const FORMAT: u32 = 1;
pub const MAGIC: [u8; 4] = *b"SKHO";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: u64,
    pub cwd: PathBuf,
    pub pid: i32,
    pub alive: bool,
    pub exit_code: Option<i32>,
    /// How long ago the shell exited, so the reaper's grace carries over.
    pub exited_ago_ms: Option<u64>,
    pub claude: ClaudeState,
    pub last_activity_unix_ms: u64,
    pub master_fd: i32,
    pub rows: u16,
    pub cols: u16,
    /// A SIGHUP was sent and not yet answered: the new image escalates as the old one would have.
    pub kill_requested: bool,
    pub replay: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Handover {
    pub format: u32,
    pub written_by: String,
    pub listener_fd: i32,
    pub replay_capacity: usize,
    pub watchdog_secs: u64,
    pub reaper_millis: u64,
    pub dead_session_secs: u64,
    pub client_pending_cap: usize,
    pub high_water: usize,
    pub low_water: usize,
    pub stall_secs: u64,
    /// The id counter, so ids stay unique across the swap (spec 2.1 of the main spec).
    pub next_session: u64,
    pub sessions: Vec<SessionRecord>,
}

/// Magic, then postcard. A sibling temp file is written with mode 0600, synced and renamed over
/// `path`, so a crash mid-write leaves nothing a resume could misread.
pub fn write(path: &Path, h: &Handover) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| io::Error::other("handover path has no parent"))?;
    let name = path.file_name().ok_or_else(|| io::Error::other("handover path has no file name"))?;
    let tmp = parent.join(format!(".{}.{}.tmp", name.to_string_lossy(), std::process::id()));
    let body = postcard::to_allocvec(h).map_err(io::Error::other)?;
    let written = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(&MAGIC)?;
        f.write_all(&body)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

pub fn read(path: &Path) -> Result<Handover, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read handover {}: {e}", path.display()))?;
    if bytes.len() < MAGIC.len() || bytes[..MAGIC.len()] != MAGIC {
        return Err(format!("{} is not a handover file (bad magic)", path.display()));
    }
    let body = &bytes[MAGIC.len()..];
    let decode_err = |e: postcard::Error| format!("cannot decode handover {}: {e}", path.display());
    // `format` is the first field, so it is checked before the rest, whose shape it governs.
    let (format, _) = postcard::take_from_bytes::<u32>(body).map_err(decode_err)?;
    if format != FORMAT {
        return Err(format!("handover format {format} is not the {FORMAT} this sarkosd reads"));
    }
    postcard::from_bytes(body).map_err(decode_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::ClaudeState;

    fn sample() -> Handover {
        Handover {
            format: FORMAT,
            written_by: "sarkosd 0.1.0 (abc1234)".into(),
            listener_fd: 5,
            replay_capacity: 1024,
            watchdog_secs: 30,
            reaper_millis: 1000,
            dead_session_secs: 600,
            client_pending_cap: 16 << 20,
            high_water: 4 << 20,
            low_water: 1 << 20,
            stall_secs: 10,
            next_session: 99,
            sessions: vec![SessionRecord {
                id: 7,
                cwd: "/tmp".into(),
                pid: 4242,
                alive: true,
                exit_code: None,
                exited_ago_ms: None,
                claude: ClaudeState::Working,
                last_activity_unix_ms: 123,
                master_fd: 9,
                rows: 40,
                cols: 120,
                kill_requested: false,
                replay: b"hello\r\n".to_vec(),
            }],
        }
    }

    #[test]
    fn round_trips_through_a_file_with_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("handover.bin");
        write(&file, &sample()).unwrap();
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(read(&file).unwrap(), sample());
        let names: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 1, "no temp file left behind: {names:?}");
    }

    #[test]
    fn refuses_a_wrong_magic_format_or_truncated_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("h.bin");
        std::fs::write(&file, b"NOPE....").unwrap();
        assert!(read(&file).unwrap_err().contains("magic"));
        let mut other = sample();
        other.format = FORMAT + 1;
        write(&file, &other).unwrap();
        assert!(read(&file).unwrap_err().contains("format"));
        write(&file, &sample()).unwrap();
        let bytes = std::fs::read(&file).unwrap();
        std::fs::write(&file, &bytes[..bytes.len() / 2]).unwrap();
        assert!(read(&file).is_err());
        assert!(read(&dir.path().join("missing.bin")).unwrap_err().contains("missing.bin"));
    }
}
