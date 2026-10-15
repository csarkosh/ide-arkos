//! What happens on `ApplyUpdate` and at every daemon start afterwards: the marker that says a
//! swap happened, and the decision to keep it (a window connected) or roll it back (none did
//! within the grace).
use crate::server::Shared;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long the new window has to connect, counted from its relaunch or from a daemon start
/// that finds the marker (`restart_clock`). `--connect-grace-secs` changes it for the tests.
pub const CONNECT_GRACE: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Applied {
    pub version: String,
    /// Written as true for daemons that read it; read by none since 2026-10-15.
    #[serde(default)]
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

/// A window connected since the swap: keep it. None within `grace` of the clock's start (the
/// marker's `at_unix`, rewritten by `restart_clock`): roll back. Before the clock runs (the
/// swap is done, the new window not yet launched) and with a clock before the marker's time
/// (it cannot say how long it has been), it waits.
pub fn decide(m: &Applied, connected_since_start: bool, clock_running: bool, now_unix: u64, grace: Duration) -> Decision {
    if connected_since_start {
        Decision::Confirm
    } else if clock_running && now_unix >= m.at_unix.saturating_add(grace.as_secs()) {
        Decision::Rollback
    } else {
        Decision::Wait
    }
}

/// Starts the grace from now: when the new window is launched (`disconnect` with a pending
/// relaunch) and when a daemon starts with a marker, so the window gets the whole grace from its
/// launch whatever came before. Rewrites the marker too, so a daemon that dies now restarts from
/// a time close to this one. Nothing when no update waits. Returns the journal line, which the
/// caller notes once the state lock is released.
#[must_use]
pub fn restart_clock(u: &mut super::UpdateState, dir: &Path) -> Option<String> {
    let m = u.applied.as_mut()?;
    m.at_unix = now_unix();
    let mut line = format!("the grace for {} starts now: a window not connected at the apply must say Hello", m.version);
    if let Err(e) = write_marker(dir, m) {
        eprintln!("arkosd: cannot rewrite the update marker for {}: {e}", m.version);
        line.push_str(&format!(" (cannot rewrite the marker: {e})"));
    }
    u.clock_running = true;
    Some(line)
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
pub fn watch(state: Shared, bundle: PathBuf, dir: PathBuf, grace: Duration) {
    std::thread::Builder::new()
        .name("update-watch".into())
        .spawn(move || loop {
            std::thread::sleep(WATCH_EVERY);
            let (m, connected, running) = {
                let st = state.lock().unwrap();
                match &st.update.applied {
                    Some(m) => (m.clone(), st.update.new_window_seen, st.update.clock_running),
                    None => return,
                }
            };
            match decide(&m, connected, running, now_unix(), grace) {
                Decision::Wait => continue,
                Decision::Confirm => {
                    // The marker goes first: a daemon dying between the two finds an `.old` with
                    // no marker at its start, which it removes; the other order would leave a
                    // marker that a start could take for an unconfirmed swap.
                    clear_marker(&dir);
                    if let Err(e) = super::swap::finish(&bundle) {
                        eprintln!("arkosd: update {} confirmed, but the old bundle stays: {e}", m.version);
                        crate::journal::note(format_args!("confirm {}: cannot remove the previous app: {e}", m.version));
                    }
                    state.lock().unwrap().update.applied = None;
                    eprintln!("arkosd: update {} confirmed by a window", m.version);
                    crate::journal::note(format_args!("confirm {}: a new window said Hello; the previous app removed", m.version));
                    return;
                }
                Decision::Rollback => {
                    let result = super::swap::rollback(&bundle);
                    let mut rejected = None;
                    let (folders, window_open): (Vec<PathBuf>, bool) = {
                        let mut st = state.lock().unwrap();
                        st.update.applied = None;
                        match &result {
                            Ok(()) => {
                                // Rejected before the marker goes: a daemon dying between the two
                                // never stages the rolled-back version again.
                                rejected = Some(match reject(&dir, &m.version) {
                                    Ok(()) => format!("reject {}: recorded in {}", m.version, rejected_path(&dir).display()),
                                    Err(e) => {
                                        eprintln!("arkosd: cannot record {} as rejected: {e}", m.version);
                                        format!("reject {}: cannot record it in {}: {e}", m.version, rejected_path(&dir).display())
                                    }
                                });
                                clear_marker(&dir);
                                st.update.last_error = Some(format!("update {} rolled back: no window came up within {}s", m.version, grace.as_secs()));
                                // A folder whose window is connected already has one.
                                let open: Vec<&PathBuf> = st.clients.values().filter(|c| c.connected).filter_map(|c| c.ui_cwd.as_ref()).collect();
                                let window_open = st.clients.values().any(|c| c.connected && c.ui_pid.is_some());
                                (st.windows.keys().filter(|f| !open.contains(f)).cloned().collect(), window_open)
                            }
                            Err(e) => {
                                st.update.last_error = Some(format!("update {}: no window came up within {}s and the rollback failed: {e}", m.version, grace.as_secs()));
                                // Nothing was put back: no window of the previous app to open.
                                (Vec::new(), true)
                            }
                        }
                    };
                    if let Some(line) = rejected {
                        crate::journal::note(line);
                    }
                    let why = state.lock().unwrap().update.last_error.clone().unwrap_or_default();
                    eprintln!("arkosd: {why}");
                    crate::journal::note(format_args!("rollback of {}: {why}; reopening {} folders", m.version, folders.len()));
                    let exe = bundle.join("Contents/MacOS/arkos");
                    if folders.is_empty() && !window_open {
                        // No folder remembered (the window was at `/`, or a chooser) and no window
                        // up: the chooser opens, so the user is never left with no app at all.
                        if let Err(e) = crate::watchdog::launch_ui(&exe, None, "the previous app after a rollback, no folder remembered") {
                            eprintln!("arkosd: cannot open the previous app after the rollback: {e}");
                        }
                    }
                    for cwd in folders {
                        if let Err(e) = crate::watchdog::launch_ui(&exe, Some(&cwd), "the previous app after a rollback") {
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
/// for its window (the watch runs, its clock started afresh); no marker means any `.old` or
/// `.new` is a leftover of an interrupted swap and is removed (or put back, when the bundle
/// itself is missing). With a marker and the bundle missing, the leftover is put back first. A
/// marker with no `.old` left is stale: the swap never ran (the marker is written first) or it
/// died between its renames and `.old` was just put back. The newest rejected version becomes
/// the poll's floor.
pub fn on_start(state: &Shared, bundle: Option<&Path>, dir: &Path, grace: Duration) {
    if let Some(v) = newest_rejected(dir) {
        state.lock().unwrap().update.swapped = Some(v);
    }
    let Some(bundle) = bundle else { return };
    let marker = read_marker(dir);
    if marker.is_some() {
        super::swap::restore_missing(bundle);
    }
    match marker {
        Some(m) if !super::swap::old_of(bundle).exists() => {
            eprintln!("arkosd: update marker for {} found with no previous bundle beside it; clearing it", m.version);
            crate::journal::note(format_args!("marker for {} found at start with no .old beside {}: cleared", m.version, bundle.display()));
            clear_marker(dir);
            super::swap::cleanup_leftovers(bundle);
        }
        Some(m) => {
            eprintln!("arkosd: update marker found: {} applied, waiting {}s for a window", m.version, grace.as_secs());
            crate::journal::note(format_args!("marker found at start: {} applied, pids at the apply {:?}; waiting {}s for a window", m.version, m.pids_at_apply, grace.as_secs()));
            let line = {
                let mut st = state.lock().unwrap();
                st.update.swapped = Some(m.version.clone());
                st.update.applied = Some(m);
                restart_clock(&mut st.update, dir)
            };
            if let Some(line) = line {
                crate::journal::note(line);
            }
            watch(state.clone(), bundle.to_path_buf(), dir.to_path_buf(), grace);
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
        let g = CONNECT_GRACE;
        assert_eq!(decide(&m, false, true, 1_005, g), Decision::Wait);
        assert_eq!(decide(&m, true, true, 1_005, g), Decision::Confirm, "a window connected: keep the new build");
        assert_eq!(decide(&m, false, true, 1_000 + g.as_secs(), g), Decision::Rollback);
        assert_eq!(decide(&m, false, true, 900, g), Decision::Wait, "a clock that went backwards waits rather than rolling back");
        clear_marker(dir.path());
        assert_eq!(read_marker(dir.path()), None);
        std::fs::write(marker_path(dir.path()), b"junk").unwrap();
        assert_eq!(read_marker(dir.path()), None, "an unreadable marker is no marker");
        std::fs::write(marker_path(dir.path()), br#"{"version":"0.2.1","old_kept":true,"at_unix":1}"#).unwrap();
        assert_eq!(read_marker(dir.path()).unwrap().pids_at_apply, Vec::<u32>::new(), "a marker without pids reads");
    }

    #[test]
    fn the_grace_counts_from_the_relaunch_not_from_the_swap() {
        let g = Duration::from_secs(20);
        let m = Applied { version: "0.2.1".into(), old_kept: true, at_unix: 1_000, pids_at_apply: vec![] };
        assert_eq!(decide(&m, false, false, 1_000 + 600, g), Decision::Wait, "no relaunch yet: the clock has not started");
        assert_eq!(decide(&m, true, false, 1_000 + 600, g), Decision::Confirm, "a new window confirms whenever it comes");
        // The relaunch, 30 s after the swap, restarts the clock: 25 s after the swap would have
        // rolled back, 25 s after the relaunch waits.
        let dir = tempfile::tempdir().unwrap();
        write_marker(dir.path(), &m).unwrap();
        let mut u = crate::updates::UpdateState { applied: Some(m), ..Default::default() };
        assert!(restart_clock(&mut u, dir.path()).is_some());
        assert!(u.clock_running);
        let restarted = u.applied.clone().unwrap();
        assert!(restarted.at_unix >= now_unix() - 1, "{restarted:?}");
        assert_eq!(read_marker(dir.path()), Some(restarted.clone()), "the marker carries the new start");
        assert_eq!(decide(&restarted, false, true, restarted.at_unix + 19, g), Decision::Wait);
        assert_eq!(decide(&restarted, false, true, restarted.at_unix + 20, g), Decision::Rollback);
        let mut none = crate::updates::UpdateState::default();
        assert_eq!(restart_clock(&mut none, dir.path()), None);
        assert!(!none.clock_running, "nothing applied: no clock");
    }

    fn fake_app(at: &Path, tag: &str) {
        std::fs::create_dir_all(at.join("Contents/MacOS")).unwrap();
        std::fs::write(at.join("Contents/MacOS/arkosd"), tag).unwrap();
    }

    fn state() -> Shared {
        std::sync::Arc::new(std::sync::Mutex::new(crate::server::State::default()))
    }

    #[test]
    fn a_marker_with_only_new_beside_a_missing_bundle_puts_it_back_and_clears_the_marker() {
        let home = tempfile::tempdir().unwrap();
        let installed = home.path().join("Arkos IDE.app");
        fake_app(&home.path().join("Arkos IDE.app.new"), "new");
        let dir = tempfile::tempdir().unwrap();
        write_marker(dir.path(), &Applied { version: "0.2.1".into(), old_kept: true, at_unix: 1, pids_at_apply: vec![] }).unwrap();
        let st = state();
        on_start(&st, Some(&installed), dir.path(), CONNECT_GRACE);
        assert_eq!(std::fs::read_to_string(installed.join("Contents/MacOS/arkosd")).unwrap(), "new", "the app is never left missing");
        assert_eq!(read_marker(dir.path()), None, "no .old to roll back to: the marker is stale");
        assert!(st.lock().unwrap().update.applied.is_none());
    }

    #[test]
    fn a_marker_with_old_beside_a_missing_bundle_puts_old_back_and_clears_the_marker() {
        // The swap died between its renames: the old bundle goes back and nothing was applied.
        let home = tempfile::tempdir().unwrap();
        let installed = home.path().join("Arkos IDE.app");
        fake_app(&home.path().join("Arkos IDE.app.old"), "old");
        let dir = tempfile::tempdir().unwrap();
        write_marker(dir.path(), &Applied { version: "0.2.1".into(), old_kept: true, at_unix: 1, pids_at_apply: vec![] }).unwrap();
        let st = state();
        on_start(&st, Some(&installed), dir.path(), CONNECT_GRACE);
        assert_eq!(std::fs::read_to_string(installed.join("Contents/MacOS/arkosd")).unwrap(), "old");
        assert_eq!(read_marker(dir.path()), None);
        assert_eq!(newest_rejected(dir.path()), None, "an interrupted swap rejects nothing");
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
