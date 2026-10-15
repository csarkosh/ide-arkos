//! The handover file (spec section 2): everything the new daemon image needs to rebuild the
//! session table after `execve`, written by the old image just before. Descriptor numbers are
//! carried as numbers because the descriptors themselves pass through the exec. The format
//! version gates a resume; `written_by` is for the log.
use protocol::{AgentRecord, ClaudeState};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

pub const FORMAT: u32 = 3;
/// The oldest format `read` still decodes. `--check-handover` prints this, `--handover-formats`
/// prints it with FORMAT. A daemon accepts a candidate that answers exactly `handover <FORMAT>`
/// to `--check-handover`, or whose `--handover-formats` range covers FORMAT.
pub const OLDEST_READ: u32 = 1;
/// How long output must have stopped before a session's ring is written to disk, unless
/// `--flush-quiet-millis` says otherwise; also what a format 1 file, which lacks it, resumes with.
pub const DEFAULT_FLUSH_QUIET_MILLIS: u64 = 2000;
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
    /// The shell the session runs, so a restart can recreate it.
    pub shell: String,
    /// The shell's priority, which its children inherit.
    pub nice: i32,
    /// The caller's extra variables only, not `TERM` and the session id.
    pub env: Vec<(String, String)>,
    /// The agent inside, from its SessionStart hook.
    pub agent: Option<AgentRecord>,
    /// Recreated by `--recover` after a restart.
    pub recovered: bool,
    /// What is left of a closed tab's grace, so the new image ends the session on the old
    /// image's deadline (format 3).
    pub grace_left_ms: Option<u64>,
    /// The grace's length as asked for, for the journal line (format 3).
    pub grace_secs: Option<u32>,
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
    /// How long output must have stopped before a session's ring is written to disk.
    pub flush_quiet_millis: u64,
    /// The id counter, so ids stay unique across the swap (spec 2.1 of the main spec).
    pub next_session: u64,
    /// Project folders with a window open and the binary each runs, carried so a restart after
    /// the swap still knows them.
    pub windows: Vec<(PathBuf, Option<PathBuf>)>,
    pub sessions: Vec<SessionRecord>,
}

/// Format 2, as 0.2.12 writes it. Never edited: a format 2 file must decode with this exact
/// shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SessionRecordV2 {
    pub id: u64,
    pub cwd: PathBuf,
    pub pid: i32,
    pub alive: bool,
    pub exit_code: Option<i32>,
    pub exited_ago_ms: Option<u64>,
    pub claude: ClaudeState,
    pub last_activity_unix_ms: u64,
    pub master_fd: i32,
    pub rows: u16,
    pub cols: u16,
    pub kill_requested: bool,
    pub replay: Vec<u8>,
    pub shell: String,
    pub nice: i32,
    pub env: Vec<(String, String)>,
    pub agent: Option<AgentRecord>,
    pub recovered: bool,
}

/// Format 2, as 0.2.12 writes it. Never edited: a format 2 file must decode with this exact
/// shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct HandoverV2 {
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
    pub flush_quiet_millis: u64,
    pub next_session: u64,
    pub windows: Vec<(PathBuf, Option<PathBuf>)>,
    pub sessions: Vec<SessionRecordV2>,
}

impl From<HandoverV2> for Handover {
    fn from(v: HandoverV2) -> Self {
        Handover {
            format: FORMAT,
            written_by: v.written_by,
            listener_fd: v.listener_fd,
            replay_capacity: v.replay_capacity,
            watchdog_secs: v.watchdog_secs,
            reaper_millis: v.reaper_millis,
            dead_session_secs: v.dead_session_secs,
            client_pending_cap: v.client_pending_cap,
            high_water: v.high_water,
            low_water: v.low_water,
            stall_secs: v.stall_secs,
            flush_quiet_millis: v.flush_quiet_millis,
            next_session: v.next_session,
            windows: v.windows,
            sessions: v
                .sessions
                .into_iter()
                .map(|s| SessionRecord {
                    id: s.id,
                    cwd: s.cwd,
                    pid: s.pid,
                    alive: s.alive,
                    exit_code: s.exit_code,
                    exited_ago_ms: s.exited_ago_ms,
                    claude: s.claude,
                    last_activity_unix_ms: s.last_activity_unix_ms,
                    master_fd: s.master_fd,
                    rows: s.rows,
                    cols: s.cols,
                    kill_requested: s.kill_requested,
                    replay: s.replay,
                    shell: s.shell,
                    nice: s.nice,
                    env: s.env,
                    agent: s.agent,
                    recovered: s.recovered,
                    grace_left_ms: None,
                    grace_secs: None,
                })
                .collect(),
        }
    }
}

/// Format 1, as the current main writes it. Never edited: a format 1 file must decode with this
/// exact shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SessionRecordV1 {
    pub id: u64,
    pub cwd: PathBuf,
    pub pid: i32,
    pub alive: bool,
    pub exit_code: Option<i32>,
    pub exited_ago_ms: Option<u64>,
    pub claude: ClaudeState,
    pub last_activity_unix_ms: u64,
    pub master_fd: i32,
    pub rows: u16,
    pub cols: u16,
    pub kill_requested: bool,
    pub replay: Vec<u8>,
}

/// Format 1, as the current main writes it. Never edited: a format 1 file must decode with this
/// exact shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct HandoverV1 {
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
    pub next_session: u64,
    pub sessions: Vec<SessionRecordV1>,
}

impl From<HandoverV1> for Handover {
    fn from(v: HandoverV1) -> Self {
        Handover {
            format: FORMAT,
            written_by: v.written_by,
            listener_fd: v.listener_fd,
            replay_capacity: v.replay_capacity,
            watchdog_secs: v.watchdog_secs,
            reaper_millis: v.reaper_millis,
            dead_session_secs: v.dead_session_secs,
            client_pending_cap: v.client_pending_cap,
            high_water: v.high_water,
            low_water: v.low_water,
            stall_secs: v.stall_secs,
            flush_quiet_millis: DEFAULT_FLUSH_QUIET_MILLIS,
            next_session: v.next_session,
            windows: Vec::new(),
            sessions: v
                .sessions
                .into_iter()
                .map(|s| SessionRecord {
                    id: s.id,
                    cwd: s.cwd,
                    pid: s.pid,
                    alive: s.alive,
                    exit_code: s.exit_code,
                    exited_ago_ms: s.exited_ago_ms,
                    claude: s.claude,
                    last_activity_unix_ms: s.last_activity_unix_ms,
                    master_fd: s.master_fd,
                    rows: s.rows,
                    cols: s.cols,
                    kill_requested: s.kill_requested,
                    replay: s.replay,
                    shell: String::new(),
                    nice: 0,
                    env: Vec::new(),
                    agent: None,
                    recovered: false,
                    grace_left_ms: None,
                    grace_secs: None,
                })
                .collect(),
        }
    }
}

/// Magic, then postcard, written with `atomic_write`.
pub fn write(path: &Path, h: &Handover) -> io::Result<()> {
    atomic_write(path, &[MAGIC.as_slice(), &postcard::to_allocvec(h).map_err(io::Error::other)?].concat())
}

/// Writes `body` to a sibling temp file with mode 0600, syncs it and renames it over `path`, so
/// a crash mid-write leaves nothing a reader could misread. The manifest uses it too.
pub fn atomic_write(path: &Path, body: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| io::Error::other("the path has no parent"))?;
    let name = path.file_name().ok_or_else(|| io::Error::other("the path has no file name"))?;
    let tmp = parent.join(format!(".{}.{}.tmp", name.to_string_lossy(), std::process::id()));
    let written = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(body)?;
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
    match format {
        FORMAT => postcard::from_bytes(body).map_err(decode_err),
        2 => postcard::from_bytes::<HandoverV2>(body).map(Handover::from).map_err(decode_err),
        1 => {
            // Raising OLDEST_READ past 1 must remove this arm and the V1 structs with it.
            const _: () = assert!(OLDEST_READ == 1);
            postcard::from_bytes::<HandoverV1>(body).map(Handover::from).map_err(decode_err)
        }
        other => Err(format!("handover format {other} is not one this arkosd reads ({OLDEST_READ} to {FORMAT})")),
    }
}

/// The records whose descriptors the new image may adopt, checked before any is: the numbers
/// come from a file, and adopting one that is stdio (below 3), the listener, or named by two
/// records would hand a session someone else's descriptor. A bad listener fails the resume; a
/// bad master loses that session, with a log line.
pub fn adoptable(h: &Handover) -> Result<Vec<&SessionRecord>, String> {
    if h.listener_fd < 3 {
        return Err(format!("the handover names fd {} as the listener, below 3", h.listener_fd));
    }
    let mut uses = std::collections::HashMap::new();
    for rec in &h.sessions {
        *uses.entry(rec.master_fd).or_insert(0usize) += 1;
    }
    let mut ok = Vec::new();
    for rec in &h.sessions {
        let why = if rec.master_fd < 3 {
            "below 3"
        } else if rec.master_fd == h.listener_fd {
            "the listener's"
        } else if uses[&rec.master_fd] > 1 {
            "named by another session too"
        } else {
            ok.push(rec);
            continue;
        };
        eprintln!("arkosd: session {} lost in the handover: its fd {} is {why}", rec.id, rec.master_fd);
    }
    Ok(ok)
}

/// The master numbers of records `adoptable` skipped, which this image must close: they came
/// through the exec without close-on-exec, so every shell spawned later would inherit them.
/// Stdio and the listener stay open whatever a record claims (the skip already logged them).
pub fn strays(h: &Handover, adopted: &[&SessionRecord]) -> Vec<i32> {
    let mut fds: Vec<i32> = h
        .sessions
        .iter()
        .map(|r| r.master_fd)
        .filter(|&fd| fd >= 3 && fd != h.listener_fd && !adopted.iter().any(|a| a.master_fd == fd))
        .collect();
    fds.sort_unstable();
    fds.dedup();
    fds
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::ClaudeState;

    fn sample() -> Handover {
        Handover {
            format: FORMAT,
            written_by: "arkosd 0.1.0 (abc1234)".into(),
            listener_fd: 5,
            replay_capacity: 1024,
            watchdog_secs: 30,
            reaper_millis: 1000,
            dead_session_secs: 600,
            client_pending_cap: 16 << 20,
            high_water: 4 << 20,
            low_water: 1 << 20,
            stall_secs: 10,
            flush_quiet_millis: 2000,
            next_session: 99,
            windows: vec![("/tmp".into(), Some("/usr/local/bin/arkos".into()))],
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
                shell: "/bin/zsh".into(),
                nice: 0,
                env: vec![],
                agent: None,
                recovered: false,
                grace_left_ms: Some(41_500),
                grace_secs: Some(60),
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
    fn descriptors_below_3_or_named_twice_are_not_adopted() {
        let mut h = sample();
        h.listener_fd = 2;
        assert!(adoptable(&h).unwrap_err().contains("listener"), "stdio is never the listener");
        let base = sample().sessions[0].clone();
        let with = |id: u64, fd: i32| SessionRecord { id, master_fd: fd, ..base.clone() };
        let mut h = sample();
        // 5 is the listener; 9 is named twice, so neither record can be trusted with it.
        h.sessions = vec![with(1, 9), with(2, 9), with(3, 1), with(4, 5), with(5, 10), with(6, -1)];
        let ids: Vec<u64> = adoptable(&h).unwrap().iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![5]);
        assert_eq!(adoptable(&sample()).unwrap().len(), 1);
    }

    #[test]
    fn skipped_masters_are_closed_but_never_stdio_or_the_listener() {
        let base = sample().sessions[0].clone();
        let with = |id: u64, fd: i32| SessionRecord { id, master_fd: fd, ..base.clone() };
        let mut h = sample();
        // 5 is the listener; 9 is named twice; 10 is adopted.
        h.sessions = vec![with(1, 9), with(2, 9), with(3, 1), with(4, 5), with(5, 10), with(6, -1)];
        assert_eq!(strays(&h, &adoptable(&h).unwrap()), vec![9], "closed once");
        assert!(strays(&sample(), &adoptable(&sample()).unwrap()).is_empty());
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

    /// A file written by the current main (format 1) resumes under this daemon with defaults for
    /// what it lacks: "Update now" from that build must work without a pkill.
    #[test]
    fn a_format_1_file_is_read_with_defaults_for_the_new_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h.bin");
        let v1 = HandoverV1 {
            format: 1,
            written_by: "arkosd 0.1.0 (old)".into(),
            listener_fd: 5,
            replay_capacity: 1024,
            watchdog_secs: 30,
            reaper_millis: 1000,
            dead_session_secs: 600,
            client_pending_cap: 1 << 20,
            high_water: 1 << 16,
            low_water: 1 << 15,
            stall_secs: 10,
            next_session: 42,
            sessions: vec![SessionRecordV1 {
                id: 9,
                cwd: "/p".into(),
                pid: 123,
                alive: true,
                exit_code: None,
                exited_ago_ms: None,
                claude: ClaudeState::Working,
                last_activity_unix_ms: 7,
                master_fd: 6,
                rows: 24,
                cols: 80,
                kill_requested: false,
                replay: b"abc".to_vec(),
            }],
        };
        let body = postcard::to_allocvec(&v1).unwrap();
        atomic_write(&path, &[MAGIC.as_slice(), &body].concat()).unwrap();
        let h = read(&path).unwrap();
        assert_eq!(h.format, FORMAT);
        assert_eq!(h.written_by, "arkosd 0.1.0 (old)");
        assert_eq!(h.next_session, 42);
        assert_eq!(h.flush_quiet_millis, 2000);
        assert!(h.windows.is_empty());
        let s = &h.sessions[0];
        assert_eq!((s.id, s.pid, s.master_fd, s.rows, s.cols), (9, 123, 6, 24, 80));
        assert_eq!(s.claude, ClaudeState::Working);
        assert_eq!(s.replay, b"abc");
        assert_eq!(s.shell, "");
        assert_eq!(s.nice, 0);
        assert!(s.env.is_empty());
        assert_eq!(s.agent, None);
        assert!(!s.recovered);
        assert_eq!((s.grace_left_ms, s.grace_secs), (None, None));
    }

    #[test]
    fn a_format_3_file_round_trips_with_the_new_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h.bin");
        let agent = protocol::AgentRecord { kind: "claude".into(), id: "a1".into(), resume: vec!["claude".into(), "--resume".into(), "a1".into()] };
        let h = Handover {
            format: FORMAT,
            written_by: "me".into(),
            listener_fd: 5,
            replay_capacity: 1024,
            watchdog_secs: 30,
            reaper_millis: 1000,
            dead_session_secs: 600,
            client_pending_cap: 1 << 20,
            high_water: 1 << 16,
            low_water: 1 << 15,
            stall_secs: 10,
            flush_quiet_millis: 250,
            next_session: 42,
            windows: vec![("/p".into(), Some("/opt/homebrew/bin/arkos".into())), ("/q".into(), None)],
            sessions: vec![SessionRecord {
                id: 9,
                cwd: "/p".into(),
                pid: 123,
                alive: true,
                exit_code: None,
                exited_ago_ms: None,
                claude: ClaudeState::Idle,
                last_activity_unix_ms: 7,
                master_fd: 6,
                rows: 24,
                cols: 80,
                kill_requested: false,
                replay: b"abc".to_vec(),
                shell: "/bin/zsh".into(),
                nice: 5,
                env: vec![("A".into(), "B".into())],
                agent: Some(agent),
                recovered: true,
                grace_left_ms: None,
                grace_secs: None,
            }],
        };
        write(&path, &h).unwrap();
        assert_eq!(read(&path).unwrap(), h);
    }

    /// A file written by 0.2.12 (format 2) resumes under this daemon: "Update now" from that
    /// build must work, with no grace on any session.
    #[test]
    fn a_format_2_file_is_read_with_no_grace() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h.bin");
        let live = sample();
        let s = &live.sessions[0];
        let v2 = HandoverV2 {
            format: 2,
            written_by: "arkosd 0.2.12 (old)".into(),
            listener_fd: live.listener_fd,
            replay_capacity: live.replay_capacity,
            watchdog_secs: live.watchdog_secs,
            reaper_millis: live.reaper_millis,
            dead_session_secs: live.dead_session_secs,
            client_pending_cap: live.client_pending_cap,
            high_water: live.high_water,
            low_water: live.low_water,
            stall_secs: live.stall_secs,
            flush_quiet_millis: 250,
            next_session: live.next_session,
            windows: live.windows.clone(),
            sessions: vec![SessionRecordV2 {
                id: s.id,
                cwd: s.cwd.clone(),
                pid: s.pid,
                alive: s.alive,
                exit_code: s.exit_code,
                exited_ago_ms: s.exited_ago_ms,
                claude: s.claude,
                last_activity_unix_ms: s.last_activity_unix_ms,
                master_fd: s.master_fd,
                rows: s.rows,
                cols: s.cols,
                kill_requested: s.kill_requested,
                replay: s.replay.clone(),
                shell: s.shell.clone(),
                nice: 3,
                env: vec![("A".into(), "B".into())],
                agent: None,
                recovered: true,
            }],
        };
        let body = postcard::to_allocvec(&v2).unwrap();
        atomic_write(&path, &[MAGIC.as_slice(), &body].concat()).unwrap();
        let h = read(&path).unwrap();
        assert_eq!(h.format, FORMAT);
        assert_eq!(h.written_by, "arkosd 0.2.12 (old)");
        assert_eq!(h.flush_quiet_millis, 250);
        assert_eq!(h.windows, live.windows);
        let r = &h.sessions[0];
        assert_eq!((r.id, r.pid, r.master_fd, r.nice, r.recovered), (7, 4242, 9, 3, true));
        assert_eq!(r.env, vec![("A".to_string(), "B".to_string())]);
        assert_eq!(r.replay, b"hello\r\n");
        assert_eq!((r.grace_left_ms, r.grace_secs), (None, None));
    }

    #[test]
    fn an_unknown_format_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h.bin");
        let body = postcard::to_allocvec(&(FORMAT + 1)).unwrap();
        atomic_write(&path, &[MAGIC.as_slice(), &body].concat()).unwrap();
        let e = read(&path).unwrap_err();
        assert!(e.contains(&format!("format {}", FORMAT + 1)), "{e}");
    }
}
