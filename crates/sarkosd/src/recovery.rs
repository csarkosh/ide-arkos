//! Recovery after a restart (update-in-place spec, section 11): each session's replay ring goes
//! to `<sessions dir>/<id>.replay` a couple of seconds after output stops and in full on
//! SIGTERM, with a manifest of the sessions (folder, shell, agent record) and the windows open;
//! `--recover` (Task 5) rebuilds everything from them. The sessions dir sits next to the socket,
//! so tests get their own under their temp socket.

use crate::server::{Shared, State};
use crate::session::{ReplayDelta, Session};
use protocol::{AgentRecord, SessionId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const MANIFEST_FORMAT: u32 = 1;
const MANIFEST_MAGIC: [u8; 4] = *b"SKMF";
/// A session printing without pause is still written this often.
pub const MAX_WAIT: Duration = Duration::from_secs(15);

pub fn sessions_dir(socket: &Path) -> PathBuf {
    socket.with_file_name("sessions")
}

pub fn replay_path(dir: &Path, id: SessionId) -> PathBuf {
    dir.join(format!("{}.replay", id.0))
}

pub fn manifest_path(dir: &Path) -> PathBuf {
    dir.join("manifest.bin")
}

/// One session as a restart recreates it: the folder, the shell and its priority, the extra
/// environment the window asked for, and the agent inside. Dead sessions are not listed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestSession {
    pub id: u64,
    pub cwd: PathBuf,
    /// Empty means the daemon's `$SHELL` (a session adopted from a format 1 handover).
    pub shell: String,
    pub nice: i32,
    pub env: Vec<(String, String)>,
    pub agent: Option<AgentRecord>,
    pub rows: u16,
    pub cols: u16,
}

/// A project folder with a window open, and the binary that window runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestWindow {
    pub cwd: PathBuf,
    pub exe: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub written_by: String,
    pub sessions: Vec<ManifestSession>,
    pub windows: Vec<ManifestWindow>,
}

/// The live sessions' ids, sorted: a death changes the manifest without anyone marking it.
fn live_ids(st: &State) -> Vec<SessionId> {
    let mut ids: Vec<SessionId> = st.sessions.values().filter(|s| s.info().alive).map(|s| s.id).collect();
    ids.sort_by_key(|id| id.0);
    ids
}

/// The manifest for the table as it is: live sessions by id, windows by folder.
pub fn manifest_of(st: &State) -> Manifest {
    let mut sessions: Vec<ManifestSession> = st
        .sessions
        .values()
        .filter(|s| s.info().alive)
        .map(|s| {
            let (rows, cols) = s.size();
            ManifestSession { id: s.id.0, cwd: s.cwd().to_path_buf(), shell: s.shell().to_string(), nice: s.nice(), env: s.env().to_vec(), agent: s.agent(), rows, cols }
        })
        .collect();
    sessions.sort_by_key(|s| s.id);
    let mut windows: Vec<ManifestWindow> = st.windows.iter().map(|(cwd, exe)| ManifestWindow { cwd: cwd.clone(), exe: exe.clone() }).collect();
    windows.sort_by(|a, b| a.cwd.cmp(&b.cwd));
    Manifest { format: MANIFEST_FORMAT, written_by: crate::version_string(), sessions, windows }
}

/// Creates the dir when missing and makes it private whatever mode it had.
fn ensure_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    if std::fs::metadata(dir)?.permissions().mode() & 0o777 != 0o700 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub fn write_manifest(dir: &Path, m: &Manifest) -> io::Result<()> {
    ensure_dir(dir)?;
    let body = postcard::to_allocvec(m).map_err(io::Error::other)?;
    crate::handover::atomic_write(&manifest_path(dir), &[MANIFEST_MAGIC.as_slice(), &body].concat())
}

/// `Ok(None)` when there is no manifest; an error names what is wrong with one that exists.
#[cfg_attr(not(test), expect(dead_code))] // read by --recover in a later task
pub fn read_manifest(dir: &Path) -> Result<Option<Manifest>, String> {
    let path = manifest_path(dir);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    if bytes.len() < MANIFEST_MAGIC.len() || bytes[..MANIFEST_MAGIC.len()] != MANIFEST_MAGIC {
        return Err(format!("{} is not a manifest (bad magic)", path.display()));
    }
    let body = &bytes[MANIFEST_MAGIC.len()..];
    let decode_err = |e: postcard::Error| format!("cannot decode {}: {e}", path.display());
    let (format, _) = postcard::take_from_bytes::<u32>(body).map_err(decode_err)?;
    if format != MANIFEST_FORMAT {
        // Garbage after the magic reads as some other format too, so this is a decode error.
        return Err(format!("cannot decode {}: manifest format {format} is not the {MANIFEST_FORMAT} this sarkosd reads", path.display()));
    }
    postcard::from_bytes(body).map(Some).map_err(decode_err)
}

/// The ring's bytes as they are, mode 0600: appended while the file is sane, else rewritten.
pub fn write_replay(path: &Path, bytes: &[u8], append: bool) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        ensure_dir(dir)?;
    }
    let mut f = std::fs::OpenOptions::new().write(true).create(true).append(append).truncate(!append).mode(0o600).open(path)?;
    f.write_all(bytes)
}

/// Where the next flush starts: `Some(offset)` appends from there, `None` rewrites the whole
/// ring. A file past twice the ring is compacted; one that vanished is rewritten.
pub fn plan(offset: Option<u64>, file_len: u64, cap: usize) -> Option<u64> {
    match offset {
        Some(0) if file_len == 0 => Some(0),
        Some(o) if file_len > 0 && file_len <= 2 * cap as u64 => Some(o),
        _ => None,
    }
}

/// Writes rings and the manifest. One per daemon, shared by the periodic thread and the
/// SIGTERM watcher, which locks it so no two writers touch a file at once.
pub struct Flusher {
    dir: PathBuf,
    /// The stream position each session's file ends at, for this daemon life.
    offsets: HashMap<SessionId, u64>,
    /// The live sessions the last manifest written listed, sorted.
    last_live: Vec<SessionId>,
}

impl Flusher {
    pub fn new(dir: PathBuf) -> Self {
        Flusher { dir, offsets: HashMap::new(), last_live: Vec::new() }
    }

    /// One pass: every session due (all of them when `force`), the files of sessions gone from
    /// the table, then the manifest when it changed (or `force`). A lock poisoned by a panic
    /// elsewhere still holds usable data, so it is taken anyway; a panic flushing one session
    /// costs only that session its write.
    pub fn tick(&mut self, state: &Shared, quiet: Duration, force: bool) {
        let sessions: Vec<Arc<Session>> = state.lock().unwrap_or_else(|p| p.into_inner()).sessions.values().cloned().collect();
        for s in &sessions {
            let flushed = catch_unwind(AssertUnwindSafe(|| {
                if force || s.flush_due(quiet, MAX_WAIT) {
                    self.flush_session(s);
                }
            }));
            if flushed.is_err() {
                eprintln!("sarkosd: writing session {}'s history panicked; the other sessions are still written", s.id.0);
            }
        }
        let gone: Vec<SessionId> = self.offsets.keys().filter(|id| !sessions.iter().any(|s| s.id == **id)).copied().collect();
        for id in gone {
            self.offsets.remove(&id);
            let _ = std::fs::remove_file(replay_path(&self.dir, id));
        }
        let manifest = {
            let mut st = state.lock().unwrap_or_else(|p| p.into_inner());
            let live = live_ids(&st);
            if force || st.manifest_dirty || live != self.last_live {
                st.manifest_dirty = false;
                self.last_live = live;
                Some(manifest_of(&st))
            } else {
                None
            }
        };
        if let Some(m) = manifest {
            if let Err(e) = write_manifest(&self.dir, &m) {
                eprintln!("sarkosd: cannot write the manifest in {}: {e}", self.dir.display());
            }
        }
    }

    fn flush_session(&mut self, s: &Arc<Session>) {
        let path = replay_path(&self.dir, s.id);
        let file_len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let from = plan(self.offsets.get(&s.id).copied(), file_len, s.replay_capacity());
        let (total, bytes, append) = match s.take_delta(from) {
            ReplayDelta::Append { total, bytes } => (total, bytes, true),
            ReplayDelta::Rewrite { total, all } => (total, all, false),
        };
        if append && bytes.is_empty() {
            self.offsets.insert(s.id, total);
            return;
        }
        match write_replay(&path, &bytes, append) {
            Ok(()) => {
                self.offsets.insert(s.id, total);
            }
            Err(e) => {
                eprintln!("sarkosd: cannot write {}: {e}", path.display());
                // Next time, rewrite from the ring rather than append to a file of unknown state.
                self.offsets.remove(&s.id);
            }
        }
    }
}

/// The periodic flusher: a pass every half of `quiet` (at least 50 ms, at most a second).
pub fn start(state: Shared, dir: PathBuf, quiet: Duration) -> Arc<Mutex<Flusher>> {
    let flusher = Arc::new(Mutex::new(Flusher::new(dir)));
    let tick = (quiet / 2).clamp(Duration::from_millis(50), Duration::from_secs(1));
    let worker = flusher.clone();
    std::thread::Builder::new()
        .name("recovery-flusher".into())
        .spawn(move || {
            let mut logged = false;
            loop {
                std::thread::sleep(tick);
                let pass = catch_unwind(AssertUnwindSafe(|| worker.lock().unwrap_or_else(|p| p.into_inner()).tick(&state, quiet, false)));
                if pass.is_err() && !logged {
                    eprintln!("sarkosd: a flush pass panicked; the flusher keeps going");
                    logged = true;
                }
            }
        })
        .expect("spawn flusher");
    flusher
}

fn termination_set() -> libc::sigset_t {
    // SAFETY: sigemptyset and sigaddset only write the set they are given.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGTERM);
        libc::sigaddset(&mut set, libc::SIGHUP);
        set
    }
}

/// Blocks SIGTERM and SIGHUP in the calling thread. Called first thing in `main`, before any
/// other thread exists, so every thread inherits the mask and `watch_termination` alone takes
/// the signals with `sigwait`. Children started through `Command` get an empty mask from the
/// standard library, so shells see signals as before; the exec'd image blocks them again itself.
pub fn block_termination_signals() {
    let set = termination_set();
    // SAFETY: the set is initialised above; a null old-set pointer is allowed.
    unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()) };
}

/// On SIGTERM (what launchd sends at logout and shutdown) or SIGHUP: write every ring and the
/// manifest, then exit 0. Closing the masters sends the shells SIGHUP, as the shutdown would.
pub fn watch_termination(state: Shared, flusher: Arc<Mutex<Flusher>>) {
    std::thread::Builder::new()
        .name("termination".into())
        .spawn(move || {
            let set = termination_set();
            let mut sig: libc::c_int = 0;
            // SAFETY: the set is initialised and `sig` is a valid out-pointer.
            let rc = unsafe { libc::sigwait(&set, &mut sig) };
            if rc != 0 {
                eprintln!("sarkosd: sigwait failed ({rc}); a shutdown will not flush histories");
                return;
            }
            let name = if sig == libc::SIGTERM { "SIGTERM" } else { "SIGHUP" };
            eprintln!("sarkosd: {name}: writing every session's history and the manifest before exiting");
            // SIGTERM and SIGHUP are blocked in every thread, so this thread must reach the exit:
            // a panic here would leave a daemon only SIGKILL can stop.
            let pass = catch_unwind(AssertUnwindSafe(|| flusher.lock().unwrap_or_else(|p| p.into_inner()).tick(&state, Duration::ZERO, true)));
            if pass.is_err() {
                eprintln!("sarkosd: the final flush panicked; exiting anyway");
            }
            eprintln!("sarkosd: exiting");
            std::process::exit(0);
        })
        .expect("spawn termination watcher");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sessions_dir_and_file_names_sit_next_to_the_socket() {
        let dir = sessions_dir(Path::new("/x/sarkos/sarkosd.sock"));
        assert_eq!(dir, PathBuf::from("/x/sarkos/sessions"));
        assert_eq!(replay_path(&dir, SessionId(42)), PathBuf::from("/x/sarkos/sessions/42.replay"));
        assert_eq!(manifest_path(&dir), PathBuf::from("/x/sarkos/sessions/manifest.bin"));
    }

    #[test]
    fn a_flush_appends_while_the_file_is_sane_and_rewrites_otherwise() {
        assert_eq!(plan(None, 0, 1024), None, "first flush of this daemon life: rewrite");
        assert_eq!(plan(Some(0), 0, 1024), Some(0), "an empty file and nothing flushed yet: append from the start");
        assert_eq!(plan(Some(5), 100, 1024), Some(5));
        assert_eq!(plan(Some(5), 2048, 1024), Some(5), "twice the ring is still fine");
        assert_eq!(plan(Some(5), 2049, 1024), None, "past twice the ring: compact");
        assert_eq!(plan(Some(5), 0, 1024), None, "the file vanished under us: rewrite");
    }

    #[test]
    fn the_manifest_round_trips_and_a_missing_one_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_manifest(dir.path()), Ok(None));
        let m = Manifest {
            format: MANIFEST_FORMAT,
            written_by: "me".into(),
            sessions: vec![ManifestSession {
                id: 9,
                cwd: "/p".into(),
                shell: "/bin/zsh".into(),
                nice: 0,
                env: vec![],
                agent: Some(protocol::AgentRecord { kind: "claude".into(), id: "a".into(), resume: vec!["claude".into(), "--resume".into(), "a".into()] }),
                rows: 24,
                cols: 80,
            }],
            windows: vec![ManifestWindow { cwd: "/p".into(), exe: Some("/opt/homebrew/bin/sarkos".into()) }],
        };
        write_manifest(dir.path(), &m).unwrap();
        assert_eq!(read_manifest(dir.path()), Ok(Some(m)));
        std::fs::write(manifest_path(dir.path()), b"SKMFgarbage").unwrap();
        assert!(read_manifest(dir.path()).unwrap_err().contains("cannot decode"));
        std::fs::write(manifest_path(dir.path()), b"nope").unwrap();
        assert!(read_manifest(dir.path()).unwrap_err().contains("not a manifest"));
    }

    #[test]
    fn replay_files_are_private_and_appended() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join("sessions");
        let path = replay_path(&sessions, SessionId(1));
        write_replay(&path, b"abc", false).unwrap();
        write_replay(&path, b"de", true).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"abcde");
        write_replay(&path, b"xy", false).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"xy");
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(&sessions).unwrap().permissions().mode() & 0o777, 0o700);
    }

    fn seeded(id: u64, seed: &[u8]) -> Arc<Session> {
        let spec = crate::session::SpawnSpec {
            id: SessionId(id),
            cwd: std::env::temp_dir(),
            shell: Some("/bin/sh".into()),
            env: vec![],
            nice: 0,
            rows: 24,
            cols: 80,
            replay_capacity: 1024,
            backpressure: crate::session::Backpressure::default(),
            seed: seed.to_vec(),
            recovered: false,
            agent: None,
        };
        Session::spawn_for_test(spec, false).unwrap()
    }

    #[test]
    fn poisoned_locks_cost_no_session_its_history_and_never_panic_the_flusher() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (seeded(1, b"history-a"), seeded(2, b"history-b"));
        let state: Shared = Arc::new(Mutex::new(State::default()));
        state.lock().unwrap().sessions.extend([(a.id, a.clone()), (b.id, b.clone())]);
        a.poison_state_for_test();
        let table = state.clone();
        let _ = std::thread::spawn(move || {
            let _held = table.lock().unwrap();
            panic!("deliberate panic to poison the table");
        })
        .join();
        assert!(state.is_poisoned());
        let mut f = Flusher::new(dir.path().join("sessions"));
        f.tick(&state, Duration::ZERO, true);
        assert_eq!(std::fs::read(replay_path(&f.dir, a.id)).unwrap(), b"history-a", "a poisoned ring is still bytes");
        assert_eq!(std::fs::read(replay_path(&f.dir, b.id)).unwrap(), b"history-b");
        let m = read_manifest(&f.dir).unwrap().expect("the manifest is written under a poisoned table");
        assert_eq!(m.sessions.iter().map(|s| s.id).collect::<Vec<_>>(), vec![1, 2]);
        // `kill` wakes subscribers under the poisoned lock, so end `a`'s shell by its pid.
        // SAFETY: the pid is this test's unreaped child.
        unsafe { libc::kill(a.info().pid.unwrap() as libc::pid_t, libc::SIGHUP) };
        let _ = b.kill();
    }

    #[test]
    fn a_death_rewrites_the_manifest_without_the_dirty_mark() {
        let dir = tempfile::tempdir().unwrap();
        let s = seeded(5, b"");
        let state: Shared = Arc::new(Mutex::new(State::default()));
        state.lock().unwrap().sessions.insert(s.id, s.clone());
        let mut f = Flusher::new(dir.path().join("sessions"));
        f.tick(&state, Duration::ZERO, true);
        assert_eq!(read_manifest(&f.dir).unwrap().unwrap().sessions.len(), 1);
        s.kill().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while s.info().alive {
            assert!(std::time::Instant::now() < deadline, "the shell never exited");
            s.poll_exit();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!state.lock().unwrap().manifest_dirty);
        f.tick(&state, Duration::from_secs(60), false);
        assert!(read_manifest(&f.dir).unwrap().unwrap().sessions.is_empty(), "the dead session left the manifest");
    }
}
