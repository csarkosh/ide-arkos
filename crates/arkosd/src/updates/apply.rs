//! What happens on `ApplyUpdate` and at every daemon start afterwards: the marker that says a
//! swap happened, and the decision to keep it (a window connected) or roll it back (none did
//! within the grace).
use crate::server::Shared;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const CONNECT_GRACE: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Applied {
    pub version: String,
    pub old_kept: bool,
    pub at_unix: u64,
    /// The `ui_pid`s of the windows connected at the swap. Their reconnects after the upgrade
    /// are not the new window, so their Hellos confirm nothing.
    #[serde(default)]
    pub pids_at_apply: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Wait,
    Confirm,
    Rollback,
}

pub fn marker_path(updates_dir: &Path) -> PathBuf {
    updates_dir.join("applied.json")
}

pub fn write_marker(updates_dir: &Path, m: &Applied) -> std::io::Result<()> {
    std::fs::create_dir_all(updates_dir)?;
    let tmp = updates_dir.join("applied.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(m).expect("serializes"))?;
    std::fs::rename(tmp, marker_path(updates_dir))
}

pub fn read_marker(updates_dir: &Path) -> Option<Applied> {
    serde_json::from_slice(&std::fs::read(marker_path(updates_dir)).ok()?).ok()
}

pub fn clear_marker(updates_dir: &Path) {
    let _ = std::fs::remove_file(marker_path(updates_dir));
}

/// A window connected since the swap: keep it. None within the grace: roll back. A clock before
/// the marker's time waits (it cannot say how long it has been).
pub fn decide(m: &Applied, connected_since_start: bool, now_unix: u64) -> Decision {
    if connected_since_start {
        Decision::Confirm
    } else if now_unix >= m.at_unix.saturating_add(CONNECT_GRACE.as_secs()) {
        Decision::Rollback
    } else {
        Decision::Wait
    }
}

/// The versions rolled back, one per line, so a later daemon does not stage them again (they
/// stay until a newer release replaces them as the floor).
pub fn rejected_path(updates_dir: &Path) -> PathBuf {
    updates_dir.join("rejected")
}

pub fn reject(updates_dir: &Path, version: &str) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(updates_dir)?;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(rejected_path(updates_dir))?;
    writeln!(f, "{version}")
}

/// The newest version in `rejected`, if any.
pub fn newest_rejected(updates_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(rejected_path(updates_dir)).ok()?;
    text.lines().map(str::trim).filter_map(|v| super::version::parse_semver(v).map(|k| (k, v.to_string()))).max_by_key(|(k, _)| *k).map(|(_, v)| v)
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// How often the watch looks at the decision.
const WATCH_EVERY: Duration = Duration::from_millis(500);

/// Watches `st.update.applied` until a window confirms the new bundle (the `.old` one is removed)
/// or none connects within the grace (the old bundle is put back and its window launched in
/// every remembered project folder). Stops once either happened, or once nothing is applied.
pub fn watch(state: Shared, bundle: PathBuf, dir: PathBuf) {
    std::thread::Builder::new()
        .name("update-watch".into())
        .spawn(move || loop {
            std::thread::sleep(WATCH_EVERY);
            let (m, connected) = {
                let st = state.lock().unwrap();
                match &st.update.applied {
                    Some(m) => (m.clone(), st.update.new_window_seen),
                    None => return,
                }
            };
            match decide(&m, connected, now_unix()) {
                Decision::Wait => continue,
                Decision::Confirm => {
                    if let Err(e) = super::swap::finish(&bundle) {
                        eprintln!("arkosd: update {} confirmed, but the old bundle stays: {e}", m.version);
                    }
                    clear_marker(&dir);
                    state.lock().unwrap().update.applied = None;
                    eprintln!("arkosd: update {} confirmed by a window", m.version);
                    return;
                }
                Decision::Rollback => {
                    let result = super::swap::rollback(&bundle);
                    let folders: Vec<PathBuf> = {
                        let mut st = state.lock().unwrap();
                        st.update.applied = None;
                        match &result {
                            Ok(()) => {
                                clear_marker(&dir);
                                if let Err(e) = reject(&dir, &m.version) {
                                    eprintln!("arkosd: cannot record {} as rejected: {e}", m.version);
                                }
                                st.update.last_error = Some(format!("update {} rolled back: no window came up within {}s", m.version, CONNECT_GRACE.as_secs()));
                                // A folder whose window is connected already has one.
                                let open: Vec<&PathBuf> = st.clients.values().filter(|c| c.connected).filter_map(|c| c.ui_cwd.as_ref()).collect();
                                st.windows.keys().filter(|f| !open.contains(f)).cloned().collect()
                            }
                            Err(e) => {
                                st.update.last_error = Some(format!("update {}: no window came up within {}s and the rollback failed: {e}", m.version, CONNECT_GRACE.as_secs()));
                                Vec::new()
                            }
                        }
                    };
                    eprintln!("arkosd: {}", state.lock().unwrap().update.last_error.as_deref().unwrap_or(""));
                    let exe = bundle.join("Contents/MacOS/arkos");
                    for cwd in folders {
                        if let Err(e) = crate::watchdog::launch_ui(&exe, Some(&cwd)) {
                            eprintln!("arkosd: cannot reopen the window in {}: {e}", cwd.display());
                        }
                    }
                    return;
                }
            }
        })
        .expect("spawn the update watch thread");
}

/// At every daemon start, after the socket listens: a marker means a swap happened and waits
/// for its window (the watch runs); no marker means any `.old` or `.new` is a leftover of an
/// interrupted swap and is removed (or put back, when the bundle itself is missing). A marker
/// with no `.old` beside an installed bundle is stale: the swap never ran (the marker is written
/// first) or the confirm died before clearing it. The newest rejected version becomes the poll's
/// floor.
pub fn on_start(state: &Shared, bundle: Option<&Path>, dir: &Path) {
    if let Some(v) = newest_rejected(dir) {
        state.lock().unwrap().update.swapped = Some(v);
    }
    let Some(bundle) = bundle else { return };
    match read_marker(dir) {
        Some(m) if bundle.exists() && !super::swap::old_of(bundle).exists() => {
            eprintln!("arkosd: update marker for {} found with no previous bundle beside it; clearing it", m.version);
            clear_marker(dir);
            super::swap::cleanup_leftovers(bundle);
        }
        Some(m) => {
            eprintln!("arkosd: update marker found: {} applied, waiting for a window", m.version);
            {
                let mut st = state.lock().unwrap();
                st.update.swapped = Some(m.version.clone());
                st.update.applied = Some(m);
            }
            watch(state.clone(), bundle.to_path_buf(), dir.to_path_buf());
        }
        None => super::swap::cleanup_leftovers(bundle),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marker_round_trips_and_the_decision_waits_then_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        let m = Applied { version: "0.2.1".into(), old_kept: true, at_unix: 1_000, pids_at_apply: vec![42] };
        write_marker(dir.path(), &m).unwrap();
        assert_eq!(read_marker(dir.path()), Some(m.clone()));
        assert_eq!(decide(&m, false, 1_005), Decision::Wait);
        assert_eq!(decide(&m, true, 1_005), Decision::Confirm, "a window connected: keep the new build");
        assert_eq!(decide(&m, false, 1_000 + CONNECT_GRACE.as_secs()), Decision::Rollback);
        assert_eq!(decide(&m, false, 900), Decision::Wait, "a clock that went backwards waits rather than rolling back");
        clear_marker(dir.path());
        assert_eq!(read_marker(dir.path()), None);
        std::fs::write(marker_path(dir.path()), b"junk").unwrap();
        assert_eq!(read_marker(dir.path()), None, "an unreadable marker is no marker");
        std::fs::write(marker_path(dir.path()), br#"{"version":"0.2.1","old_kept":true,"at_unix":1}"#).unwrap();
        assert_eq!(read_marker(dir.path()).unwrap().pids_at_apply, Vec::<u32>::new(), "a marker without pids reads");
    }

    #[test]
    fn rejected_versions_accumulate_and_the_newest_is_the_floor() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(newest_rejected(dir.path()), None);
        reject(dir.path(), "0.3.0").unwrap();
        reject(dir.path(), "0.10.0").unwrap();
        reject(dir.path(), "0.4.0").unwrap();
        assert_eq!(newest_rejected(dir.path()).as_deref(), Some("0.10.0"));
    }
}
