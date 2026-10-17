//! Sealing a window's journal when its connection ends (telemetry spec, section 2.2). The window
//! named its session in a `Session` request after its `Hello`; when its socket closes, the
//! daemon turns `journal/<sid>.jsonl` into `queue/session-<sid>.json`. The end is `quit` when the
//! window said `Goodbye` or `SessionQuit` first (a quit, a macOS logout or restart included), or
//! when the daemon was asked to relaunch it (an update's relaunch, which closes the old window on
//! purpose), and `lost` otherwise: a crash or a force quit, the connection dropped without a
//! quit. A window that named no session has nothing sealed.
use ::telemetry::visits_app::{seal, End};
use std::path::{Path, PathBuf};

/// How a window's session ended, from what its record held when it left: the end its `Goodbye`
/// or `SessionQuit` set, else `quit` for a window the daemon relaunches, else `lost`.
pub fn end_of(said: Option<End>, relaunch: bool) -> End {
    said.unwrap_or(if relaunch { End::Quit } else { End::Lost })
}

/// Seals the journal of the session `sid` with `end` at `now`, the state file loaded just before
/// and saved just after. Returns the queued report's path; None when there was nothing to seal
/// (no journal: the window kept none) or the seal failed, which is logged without the folder's
/// path. Called after the state lock is released, so no other client waits on the disk.
pub fn window_gone(dir: &Path, sid: &str, end: End, now: u64) -> Option<PathBuf> {
    match super::update_state(dir, |s| seal(dir, sid, end, now, s)) {
        Ok(Ok(report)) => report,
        Ok(Err(e)) | Err(e) => {
            eprintln!("arkosd: cannot seal a window's usage session: {e}");
            None
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::telemetry::tests::release_env;
    use ::telemetry::visits_app::{Journal, State};

    const SID: &str = "0123456789abcdef0123456789abcdef";
    const NOW: u64 = 20_000 * 86_400;

    /// The `end` the queued report for `sid` carries.
    pub fn sealed_end(dir: &Path, sid: &str) -> Option<String> {
        let bytes = std::fs::read(dir.join("queue").join(format!("session-{sid}.json"))).ok()?;
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        v["end"].as_str().map(str::to_string)
    }

    #[test]
    fn goodbye_or_a_relaunch_is_quit_and_anything_else_is_lost() {
        assert_eq!(end_of(Some(End::Quit), false), End::Quit);
        assert_eq!(end_of(Some(End::Quit), true), End::Quit);
        assert_eq!(end_of(None, true), End::Quit, "the update relaunch closes the old window on purpose");
        assert_eq!(end_of(None, false), End::Lost);
    }

    #[test]
    fn a_window_gone_has_its_journal_sealed_with_its_end() {
        let dir = tempfile::tempdir().unwrap();
        Journal::open(dir.path(), SID, &release_env(), ::telemetry::allowlist().clone(), NOW).unwrap();
        let path = window_gone(dir.path(), SID, End::Lost, NOW + 120).expect("a report");
        assert_eq!(path, dir.path().join("queue").join(format!("session-{SID}.json")));
        assert_eq!(sealed_end(dir.path(), SID).as_deref(), Some("lost"));
        assert!(!dir.path().join("journal").join(format!("{SID}.jsonl")).exists(), "the journal is gone once sealed");
        assert_eq!(State::load(dir.path()).last_report_day, Some(20_000), "the state file was saved");
    }

    #[test]
    fn a_window_that_kept_no_journal_seals_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(window_gone(dir.path(), SID, End::Quit, NOW), None);
        assert!(!dir.path().join("queue").exists());
    }
}
